use super::*;
use crate::server::auth::HttpAuthConfig;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, EncodingKey, Header};

fn test_state_with_auth(auth: HttpAuthConfig) -> Arc<AppState> {
    let state = test_state();
    state.configure_http_auth(auth);
    state
}

fn hs256_jwks(kid: &str, secret: &[u8]) -> serde_json::Value {
    json!({
        "keys": [{
            "kty": "oct",
            "kid": kid,
            "alg": "HS256",
            "k": URL_SAFE_NO_PAD.encode(secret),
        }]
    })
}

fn hs256_token_with_key(
    issuer: &str,
    audience: &str,
    scope: &str,
    exp: usize,
    kid: &str,
    secret: &str,
) -> String {
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some(kid.to_owned());
    let claims = json!({
        "iss": issuer,
        "aud": audience,
        "scope": scope,
        "exp": exp,
        "nbf": 0,
        "sub": "codelens-test",
    });
    jsonwebtoken::encode(
        &header,
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap()
}

fn hs256_token(issuer: &str, audience: &str, scope: &str, exp: usize) -> String {
    hs256_token_with_key(issuer, audience, scope, exp, "test-key", "secret")
}

fn future_exp() -> usize {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize
        + 3600
}

#[tokio::test]
async fn auth_rejects_missing_bearer_token() {
    let auth = HttpAuthConfig::jwks_static_for_test(
        hs256_jwks("test-key", b"secret"),
        "https://issuer.example",
        "codelens",
        Some("mcp:tools"),
    );
    let app = build_router(test_state_with_auth(auth));
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let challenge = resp
        .headers()
        .get("www-authenticate")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(challenge.contains("Bearer"));
    assert!(challenge.contains("resource_metadata="));
    assert!(challenge.contains("scope=\"mcp:tools\""));
}

#[tokio::test]
async fn auth_accepts_valid_hs256_jwks_token() {
    let auth = HttpAuthConfig::jwks_static_for_test(
        hs256_jwks("test-key", b"secret"),
        "https://issuer.example",
        "codelens",
        Some("mcp:tools"),
    );
    let app = build_router(test_state_with_auth(auth));
    let token = hs256_token(
        "https://issuer.example",
        "codelens",
        "mcp:tools",
        future_exp(),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn auth_rejects_wrong_issuer() {
    let auth = HttpAuthConfig::jwks_static_for_test(
        hs256_jwks("test-key", b"secret"),
        "https://issuer.example",
        "codelens",
        Some("mcp:tools"),
    );
    let app = build_router(test_state_with_auth(auth));
    let token = hs256_token(
        "https://wrong.example",
        "codelens",
        "mcp:tools",
        future_exp(),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_rejects_wrong_audience() {
    let auth = HttpAuthConfig::jwks_static_for_test(
        hs256_jwks("test-key", b"secret"),
        "https://issuer.example",
        "codelens",
        Some("mcp:tools"),
    );
    let app = build_router(test_state_with_auth(auth));
    let token = hs256_token(
        "https://issuer.example",
        "wrong-audience",
        "mcp:tools",
        future_exp(),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_rejects_expired_token() {
    let auth = HttpAuthConfig::jwks_static_for_test(
        hs256_jwks("test-key", b"secret"),
        "https://issuer.example",
        "codelens",
        Some("mcp:tools"),
    );
    let app = build_router(test_state_with_auth(auth));
    let token = hs256_token("https://issuer.example", "codelens", "mcp:tools", 1);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_rejects_missing_scope() {
    let auth = HttpAuthConfig::jwks_static_for_test(
        hs256_jwks("test-key", b"secret"),
        "https://issuer.example",
        "codelens",
        Some("mcp:tools"),
    );
    let app = build_router(test_state_with_auth(auth));
    let token = hs256_token(
        "https://issuer.example",
        "codelens",
        "other:scope",
        future_exp(),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let challenge = resp
        .headers()
        .get("www-authenticate")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(challenge.contains("error=\"insufficient_scope\""));
}

#[tokio::test]
async fn auth_rejects_wrong_signature() {
    let auth = HttpAuthConfig::jwks_static_for_test(
        hs256_jwks("test-key", b"secret"),
        "https://issuer.example",
        "codelens",
        Some("mcp:tools"),
    );
    let app = build_router(test_state_with_auth(auth));
    let token = hs256_token_with_key(
        "https://issuer.example",
        "codelens",
        "mcp:tools",
        future_exp(),
        "test-key",
        "wrong-secret",
    );
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_refreshes_jwks_on_kid_miss() {
    use axum::{Router, extract::State, routing};
    use std::sync::atomic::{AtomicUsize, Ordering};

    async fn jwks_handler(State(calls): State<Arc<AtomicUsize>>) -> String {
        let count = calls.fetch_add(1, Ordering::SeqCst);
        let jwks = if count == 0 {
            hs256_jwks("old-key", b"old-secret")
        } else {
            hs256_jwks("new-key", b"new-secret")
        };
        serde_json::to_string(&jwks).unwrap()
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let jwks_app = Router::new()
        .route("/jwks", routing::get(jwks_handler))
        .with_state(calls.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, jwks_app).await.unwrap();
    });

    let auth = HttpAuthConfig::jwks_remote_for_test(
        format!("http://{addr}/jwks"),
        "https://issuer.example",
        "codelens",
        Some("mcp:tools"),
    );
    let app = build_router(test_state_with_auth(auth));

    let old_token = hs256_token_with_key(
        "https://issuer.example",
        "codelens",
        "mcp:tools",
        future_exp(),
        "old-key",
        "old-secret",
    );
    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {old_token}"))
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    let new_token = hs256_token_with_key(
        "https://issuer.example",
        "codelens",
        "mcp:tools",
        future_exp(),
        "new-key",
        "new-secret",
    );
    let second = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {new_token}"))
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    server.abort();
}

#[tokio::test]
async fn protected_resource_metadata_is_public() {
    let auth = HttpAuthConfig::jwks_static_for_test(
        hs256_jwks("test-key", b"secret"),
        "https://issuer.example",
        "codelens",
        Some("mcp:tools"),
    );
    let app = build_router(test_state_with_auth(auth));
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/.well-known/oauth-protected-resource/mcp")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    let metadata: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(metadata["resource"], json!("/mcp"));
    assert_eq!(
        metadata["authorization_servers"][0],
        json!("https://issuer.example")
    );
    assert_eq!(metadata["scopes_supported"][0], json!("mcp:tools"));
}

#[tokio::test]
async fn https_transport_accepts_initialize_over_tls() {
    let temp = temp_project_dir("https_smoke");
    let cert_path = temp.join("cert.pem");
    let key_path = temp.join("key.pem");
    let rcgen::CertifiedKey { cert, key_pair } =
        rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_owned(), "localhost".to_owned()])
            .unwrap();
    std::fs::write(&cert_path, cert.pem()).unwrap();
    std::fs::write(&key_path, key_pair.serialize_pem()).unwrap();

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    crate::server::transport_http::install_default_rustls_provider();
    let tls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert_path, &key_path)
        .await
        .unwrap();
    let server = tokio::spawn(async move {
        axum_server::from_tcp_rustls(listener, tls_config)
            .serve(build_router(test_state()).into_make_service())
            .await
            .unwrap();
    });

    let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
    let response = tokio::task::spawn_blocking(move || {
        use std::io::{Read, Write};
        let connector = native_tls::TlsConnector::builder()
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true)
            .build()
            .unwrap();
        let stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut stream = connector.connect("127.0.0.1", stream).unwrap();
        write!(
            stream,
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    })
    .await
    .unwrap();

    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains(r#""result""#), "{response}");
    assert!(response.contains(r#""protocolVersion""#), "{response}");
    server.abort();
}

// ── POST /mcp ────────────────────────────────────────────────────────
