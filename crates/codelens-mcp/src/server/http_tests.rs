//! Integration tests for the Streamable HTTP transport.
//!
//! Uses tower::ServiceExt::oneshot to test axum handlers without starting a real server.
//! Run with: `cargo test --features http`

#![cfg(feature = "http")]

use super::auth::{HttpAuthConfig, StaticJwks};
use super::compat::ServerCompatMode;
use super::session::SessionStore;
use super::transport_http::{TlsConfig, build_router, load_rustls_config};
use crate::AppState;
use axum::http::{Request, StatusCode};
use codelens_engine::ProjectRoot;
use http_body_util::BodyExt;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use serde::Serialize;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tower::ServiceExt;

fn test_state() -> Arc<AppState> {
    let dir = std::env::temp_dir().join(format!(
        "codelens-http-test-{}-{:?}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        std::thread::current().id(),
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("hello.txt"), "world\n").unwrap();
    let project = ProjectRoot::new(dir.to_str().unwrap()).unwrap();
    let state = AppState::new(project, crate::tool_defs::ToolPreset::Balanced);
    Arc::new(state.with_session_store())
}

fn anthropic_remote_state() -> Arc<AppState> {
    let state = test_state();
    state.configure_compat_mode(ServerCompatMode::AnthropicRemote);
    state
}

fn auth_state() -> Arc<AppState> {
    let state = test_state();
    state.configure_http_auth(HttpAuthConfig::jwks_with_static_keys_for_tests(
        StaticJwks::new(serde_json::json!({
            "keys": [{
                "kty": "oct",
                "kid": "test-key",
                "alg": "HS256",
                "k": "c2VjcmV0"
            }]
        })),
        "https://auth.example.com",
        "https://codelens.example.com/mcp",
        Some("codelens:tools"),
    ));
    state
}

#[derive(Serialize)]
struct TestClaims<'a> {
    iss: &'a str,
    aud: &'a str,
    exp: usize,
    nbf: usize,
    scope: &'a str,
}

fn hs256_token_with_key(
    issuer: &str,
    audience: &str,
    scope: &str,
    exp: usize,
    kid: &str,
    secret: &[u8],
) -> String {
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some(kid.to_owned());
    let claims = TestClaims {
        iss: issuer,
        aud: audience,
        exp,
        nbf: 0,
        scope,
    };
    encode(&header, &claims, &EncodingKey::from_secret(secret)).expect("test token")
}

fn hs_token_with_algorithm_and_key(
    algorithm: Algorithm,
    issuer: &str,
    audience: &str,
    scope: &str,
    exp: usize,
    kid: &str,
    secret: &[u8],
) -> String {
    let mut header = Header::new(algorithm);
    header.kid = Some(kid.to_owned());
    let claims = TestClaims {
        iss: issuer,
        aud: audience,
        exp,
        nbf: 0,
        scope,
    };
    encode(&header, &claims, &EncodingKey::from_secret(secret)).expect("test token")
}

fn hs256_token(issuer: &str, audience: &str, scope: &str, exp: usize) -> String {
    hs256_token_with_key(issuer, audience, scope, exp, "test-key", b"secret")
}

fn future_exp() -> usize {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize
        + 3600
}

const TEST_CERT_PEM: &str = r#"-----BEGIN CERTIFICATE-----
MIIDCTCCAfGgAwIBAgIUHvpnp51ZJw0LX2EZ+9/m6DBsPngwDQYJKoZIhvcNAQEL
BQAwFDESMBAGA1UEAwwJbG9jYWxob3N0MB4XDTI2MDQyNDA2MjkzMVoXDTI2MDQy
NTA2MjkzMVowFDESMBAGA1UEAwwJbG9jYWxob3N0MIIBIjANBgkqhkiG9w0BAQEF
AAOCAQ8AMIIBCgKCAQEAuAt28l2WiPwRNWaYMzUhneza54tnI0NbPF1+cP1ux4Ql
bbKsPjm2GdSrsqvFDFhSa7HEm0o835v6qWdfISoP1oQxpMmnehSHenE5L+ivSZrw
SClvhzpi2xR58iRxeSyFRMPue8gtdW4l1BrFjpg5t3jOrsfgPqw6YzSftyE91V9b
ciRtGFFNXJ6rViDZKOHOnS2ecz98WwT5jrbCcXRrzC2ytWKNN3iGLmJ3wp5RJ+Xx
ncTi4K2ZuP+wlSeHsrj4MUflbzphKoz02qPRqHwkCRk8ch5jPkaJUOplZWWeGqb2
ZhRcQPwLu6TOMQdNjTQXtyfzSBAB0UHe+o2IOJt0kwIDAQABo1MwUTAdBgNVHQ4E
FgQUb/JwrX4jPAosodmauYHLHHmCGJEwHwYDVR0jBBgwFoAUb/JwrX4jPAosodma
uYHLHHmCGJEwDwYDVR0TAQH/BAUwAwEB/zANBgkqhkiG9w0BAQsFAAOCAQEATNtO
+JpO7+lgd2Sx5zc/5zBajg+HVX2N1SjoX95y5zdSrMn9ST3vy0KQHXns6sC0BBOX
DCE8JV67lahHxisq5juzar/evbx6IKZ/Ycv/YE2sTMSVevx3mFw4ZFMADfljqq2g
CUnOiYgsTkQsBkCM2J3Xh2kUJ5jECTNCr5kqbtenJ8Dew0gVrGEuqwMbx+f5gX39
LZNEHJjuq3ykGy2YixzmOvA1QF44AwUf2B3byb7E2ulY3pnhsUF6eOUdweUDCGXn
32wMDRrA9iNDTKW2O79HXS6gtTLjekdM0GpF56eNLaOlB7P4nJayzvGQlEqTpD/r
u0qPhYSuuk/vKl7pfw==
-----END CERTIFICATE-----
"#;

const TEST_KEY_PEM: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC4C3byXZaI/BE1
ZpgzNSGd7Nrni2cjQ1s8XX5w/W7HhCVtsqw+ObYZ1Kuyq8UMWFJrscSbSjzfm/qp
Z18hKg/WhDGkyad6FId6cTkv6K9JmvBIKW+HOmLbFHnyJHF5LIVEw+57yC11biXU
GsWOmDm3eM6ux+A+rDpjNJ+3IT3VX1tyJG0YUU1cnqtWINko4c6dLZ5zP3xbBPmO
tsJxdGvMLbK1Yo03eIYuYnfCnlEn5fGdxOLgrZm4/7CVJ4eyuPgxR+VvOmEqjPTa
o9GofCQJGTxyHmM+RolQ6mVlZZ4apvZmFFxA/Au7pM4xB02NNBe3J/NIEAHRQd76
jYg4m3STAgMBAAECggEAKxmIN/bhv1+kWgiWIPvSzQyAMRQsyY3HCmpsp2I6NKAG
MdvTSVkzg3YR5WwjX6I5Xv4I6ELo4YbCGzTZiscyYU6g35HX1heDqJFmToljr02I
8qU9eIIcT2jKrAGLz1A1P2bQ7QzyVFtAoZzJYfzVG1m/sR+erJ6hp8TVmEnBFLvz
dAHgefeerqSvLyad8qMEHv86Ddri0Rpgxt1Z0wjrMix5peD1Mp3h3QXCR1n9N5jj
B3ZgaVRC5dQWImEu7jGFUSaytnBLHTQXci4x87tOGzyxO2t3+Snb+1wA5fqYZpBU
qbGxmJEd3wduagYUR+0PIE7Spwvuwp0l9OshXz4VpQKBgQDt095ypOVdgoB0vfCk
RIyzzy5GI2n4SjZpvEZ1kLBC5Ew/HgaKJQbXBd84P6qr74EVOSr+jTwbuJhc8VmV
lJVyxvW5gg8ryYu08RWzIZVRNkLtCsnxhW/ggtHPA4EEpmnAr8LJ01louLCqFWH0
6+qRV87qTPvqw6p8MZcoNTpulQKBgQDGG42agjtpXxyfiFqOXEn7p3Oyd5nKFuia
BrqmOkSzxGpmN1RawCAxd0QzOYVWetJBk+yvtomqmmESfukrzNCnToPH6NCs9aoC
bslGBMZZjJd3xrqpm2JNz4oEYWrta3P+Blgwb5MzQoRb/vKYfYCebXXK38NehvMx
V/oyxSmUhwKBgCgPOvX2nofctoRzhfg1b7nN2Q6JYo0m+vledEPTRk1OJSWwigt0
5y0K2SmhV780TXrksUBFS+2jb06gfKV8bJvztWo05RdMEJM+1JfivUL7r9Q7r/5V
qp2Xi32iKnY9Da0eLeJPDk1cZq2Pgnt9zXoD31+J7hkCMlJPDBYCuT/tAoGAKoTr
ZYgiHEGPsSXg2cExF9Qe3uUQmvFDxxs+oELNUBAODhY+AqRNxJAmkR/9YExIKE8J
c8Un0vgDcabPgNkax23wls1/TEAF1zPT+zU3JS0prUl38sMo3C55HuuRuZdgc9sE
vpCT9WKHaf9ULipxmo8/wuU312f7dlG9n3v1qq0CgYEA6SjrZDlmCB0clg6jc0cc
YAkw1ATk+V3zj2M2Hde6cWlOyGD58TzelqLLUiOyf5peng1RACQCxBy7B/tuS9fR
Bavyn/FflCXjn619I/P/1x1ubefX8MVhTQBX5rOpujPn2TFehlk1/I6y1YZraTx8
A5Il+b48wMPazPRl3wSwbU4=
-----END PRIVATE KEY-----
"#;

fn temp_project_dir(name: &str) -> std::path::PathBuf {
    crate::test_helpers::fixtures::temp_project_dir(name)
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

async fn next_sse_chunk(resp: axum::response::Response) -> String {
    let mut body = resp.into_body();
    let frame = tokio::time::timeout(Duration::from_secs(1), body.frame())
        .await
        .expect("timed out waiting for SSE frame")
        .expect("SSE stream ended before first frame")
        .expect("SSE frame error");
    let bytes = frame.into_data().expect("expected SSE data frame");
    String::from_utf8(bytes.to_vec()).expect("SSE chunk should be utf-8")
}

fn first_resource_text(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value.get("result").cloned())
        .and_then(|result| result.get("contents").cloned())
        .and_then(|contents| contents.as_array().cloned())
        .and_then(|contents| contents.first().cloned())
        .and_then(|content| content.get("text").cloned())
        .and_then(|text| text.as_str().map(ToOwned::to_owned))
        .unwrap_or_default()
}

fn first_tool_payload(body: &str) -> serde_json::Value {
    let value = serde_json::from_str::<serde_json::Value>(body).unwrap_or_default();
    let mut payload = value
        .get("result")
        .and_then(|result| result.get("content"))
        .and_then(|contents| contents.as_array())
        .and_then(|contents| contents.first())
        .and_then(|content| content.get("text"))
        .and_then(|text| text.as_str())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .unwrap_or_default();

    if let Some(structured_content) = value
        .get("result")
        .and_then(|result| result.get("structuredContent"))
        .cloned()
    {
        if !payload.is_object() {
            payload = serde_json::json!({});
        }
        payload
            .as_object_mut()
            .expect("payload object")
            .insert("data".to_owned(), structured_content);
    }

    payload
}

#[derive(Debug)]
struct AcceptSelfSignedCertForHttpsSmoke;

impl ServerCertVerifier for AcceptSelfSignedCertForHttpsSmoke {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
        ]
    }
}

// ── POST /mcp ────────────────────────────────────────────────────────

#[tokio::test]
async fn post_initialize_returns_session_id() {
    let app = build_router(test_state());
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

    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers().get("mcp-session-id").is_some(),
        "initialize should return Mcp-Session-Id header"
    );
    let body = body_string(resp).await;
    assert!(body.contains("\"jsonrpc\":\"2.0\""));
    assert!(body.contains("\"id\":1"));
}

#[tokio::test]
async fn initialize_negotiates_latest_2025_11_25_by_default() {
    let app = build_router(test_state());
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

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    let value: serde_json::Value = serde_json::from_str(&body).expect("initialize json");
    assert_eq!(
        value["result"]["protocolVersion"],
        serde_json::json!("2025-11-25")
    );
}

#[tokio::test]
async fn post_with_2025_11_25_protocol_version_header_is_accepted() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-protocol-version", "2025-11-25")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    let value: serde_json::Value = serde_json::from_str(&body).expect("initialize json");
    assert_eq!(
        value["result"]["protocolVersion"],
        serde_json::json!("2025-11-25")
    );
}

#[tokio::test]
async fn anthropic_remote_compat_initialize_advertises_tools_only() {
    let app = build_router(anthropic_remote_state());
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

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    let value: serde_json::Value = serde_json::from_str(&body).expect("initialize json");
    let capabilities = value["result"]["capabilities"]
        .as_object()
        .expect("capabilities object");
    assert!(capabilities.contains_key("tools"));
    assert!(!capabilities.contains_key("resources"));
    assert!(!capabilities.contains_key("prompts"));
}

#[tokio::test]
async fn anthropic_remote_compat_rejects_prompt_and_resource_methods() {
    let app = build_router(anthropic_remote_state());
    for method in ["resources/list", "prompts/list"] {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(format!(
                        r#"{{"jsonrpc":"2.0","id":1,"method":"{method}"}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        let value: serde_json::Value = serde_json::from_str(&body).expect("json-rpc error");
        assert_eq!(value["error"]["code"], serde_json::json!(-32601));
    }
}

#[tokio::test]
async fn anthropic_remote_compat_tools_list_uses_connector_safe_shape() {
    let app = build_router(anthropic_remote_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"full":true}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    let value: serde_json::Value = serde_json::from_str(&body).expect("tools/list json");
    let result = value["result"].as_object().expect("result object");
    assert_eq!(
        result.keys().cloned().collect::<Vec<_>>(),
        vec!["tools".to_owned()]
    );
    let first_tool = value["result"]["tools"]
        .as_array()
        .and_then(|tools| tools.first())
        .expect("at least one tool");
    assert!(first_tool.get("name").is_some());
    assert!(first_tool.get("description").is_some());
    assert!(first_tool.get("inputSchema").is_some());
    assert!(first_tool.get("title").is_some());
    assert!(first_tool.get("outputSchema").is_none());
    assert!(first_tool.get("annotations").is_none());
}

#[tokio::test]
async fn auth_metadata_is_served_at_root_and_mcp_endpoint_paths() {
    let app = build_router(auth_state());
    for path in [
        "/.well-known/oauth-protected-resource",
        "/.well-known/oauth-protected-resource/mcp",
    ] {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(path)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        let value: serde_json::Value = serde_json::from_str(&body).expect("metadata json");
        assert_eq!(
            value["resource"],
            serde_json::json!("https://codelens.example.com/mcp")
        );
        assert_eq!(
            value["authorization_servers"],
            serde_json::json!(["https://auth.example.com"])
        );
        assert_eq!(
            value["scopes_supported"],
            serde_json::json!(["codelens:tools"])
        );
    }
}

#[tokio::test]
async fn auth_rejects_missing_bearer_token_with_resource_metadata_challenge() {
    let app = build_router(auth_state());
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
        .expect("WWW-Authenticate header");
    assert!(challenge.contains("Bearer"));
    assert!(challenge.contains("resource_metadata="));
    assert!(challenge.contains("scope=\"codelens:tools\""));
}

#[tokio::test]
async fn auth_accepts_valid_bearer_token() {
    let app = build_router(auth_state());
    let token = hs256_token(
        "https://auth.example.com",
        "https://codelens.example.com/mcp",
        "codelens:tools",
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
    assert!(resp.headers().get("mcp-session-id").is_some());
}

#[tokio::test]
async fn auth_rejects_wrong_audience() {
    let app = build_router(auth_state());
    let token = hs256_token(
        "https://auth.example.com",
        "https://other.example.com/mcp",
        "codelens:tools",
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
async fn auth_rejects_wrong_issuer() {
    let app = build_router(auth_state());
    let token = hs256_token(
        "https://other-auth.example.com",
        "https://codelens.example.com/mcp",
        "codelens:tools",
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
async fn auth_rejects_wrong_signature() {
    let app = build_router(auth_state());
    let token = hs256_token_with_key(
        "https://auth.example.com",
        "https://codelens.example.com/mcp",
        "codelens:tools",
        future_exp(),
        "test-key",
        b"wrong-secret",
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
async fn auth_rejects_jwt_algorithm_that_does_not_match_jwk_algorithm() {
    let app = build_router(auth_state());
    let token = hs_token_with_algorithm_and_key(
        Algorithm::HS512,
        "https://auth.example.com",
        "https://codelens.example.com/mcp",
        "codelens:tools",
        future_exp(),
        "test-key",
        b"secret",
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
    let app = build_router(auth_state());
    let token = hs256_token(
        "https://auth.example.com",
        "https://codelens.example.com/mcp",
        "codelens:tools",
        1,
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
async fn auth_rejects_missing_required_scope() {
    let app = build_router(auth_state());
    let token = hs256_token(
        "https://auth.example.com",
        "https://codelens.example.com/mcp",
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
}

#[tokio::test]
async fn auth_refreshes_jwks_on_kid_miss() {
    let jwks = Arc::new(RwLock::new(serde_json::json!({
        "keys": [{
            "kty": "oct",
            "kid": "old-key",
            "alg": "HS256",
            "k": "c2VjcmV0"
        }]
    })));
    let jwks_app = axum::Router::new().route(
        "/jwks",
        axum::routing::get({
            let jwks = Arc::clone(&jwks);
            move || {
                let jwks = Arc::clone(&jwks);
                async move {
                    axum::Json(
                        jwks.read()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .clone(),
                    )
                }
            }
        }),
    );
    let jwks_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("jwks listener should bind");
    let jwks_port = jwks_listener.local_addr().unwrap().port();
    let jwks_server = tokio::spawn(async move {
        axum::serve(jwks_listener, jwks_app).await.unwrap();
    });

    let state = test_state();
    state.configure_http_auth(HttpAuthConfig::jwks(
        format!("http://127.0.0.1:{jwks_port}/jwks"),
        "https://auth.example.com".to_owned(),
        "https://codelens.example.com/mcp".to_owned(),
        Some("codelens:tools".to_owned()),
    ));
    let app = build_router(state);
    let old_token = hs256_token_with_key(
        "https://auth.example.com",
        "https://codelens.example.com/mcp",
        "codelens:tools",
        future_exp(),
        "old-key",
        b"secret",
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

    *jwks
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = serde_json::json!({
        "keys": [{
            "kty": "oct",
            "kid": "rotated-key",
            "alg": "HS256",
            "k": "c2VjcmV0"
        }]
    });
    let rotated_token = hs256_token_with_key(
        "https://auth.example.com",
        "https://codelens.example.com/mcp",
        "codelens:tools",
        future_exp(),
        "rotated-key",
        b"secret",
    );
    let second = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {rotated_token}"))
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    jwks_server.abort();

    assert_eq!(second.status(), StatusCode::OK);
}

#[tokio::test]
async fn https_tls_config_loads_pem_pair() {
    let dir = temp_project_dir("https-tls-config");
    let cert = dir.join("cert.pem");
    let key = dir.join("key.pem");
    std::fs::write(&cert, TEST_CERT_PEM).unwrap();
    std::fs::write(&key, TEST_KEY_PEM).unwrap();

    load_rustls_config(&TlsConfig {
        cert_path: cert,
        key_path: key,
    })
    .await
    .expect("test PEM pair should load");
}

#[tokio::test]
async fn https_transport_accepts_initialize_over_tls() {
    let dir = temp_project_dir("https-smoke");
    let cert = dir.join("cert.pem");
    let key = dir.join("key.pem");
    std::fs::write(&cert, TEST_CERT_PEM).unwrap();
    std::fs::write(&key, TEST_KEY_PEM).unwrap();
    let rustls_config = load_rustls_config(&TlsConfig {
        cert_path: cert,
        key_path: key,
    })
    .await
    .expect("test PEM pair should load");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("https smoke listener should bind");
    let port = listener.local_addr().unwrap().port();
    let app = build_router(test_state());
    let server = tokio::spawn(async move {
        axum_server::from_tcp_rustls(listener.into_std().unwrap(), rustls_config)
            .serve(app.into_make_service())
            .await
            .unwrap();
    });

    let tls = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptSelfSignedCertForHttpsSmoke))
        .with_no_client_auth();
    let agent = ureq::builder()
        .tls_config(Arc::new(tls))
        .timeout(Duration::from_secs(5))
        .build();
    let response = tokio::task::spawn_blocking(move || {
        agent
            .post(&format!("https://127.0.0.1:{port}/mcp"))
            .set("content-type", "application/json")
            .send_string(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#)
    })
    .await
    .expect("https client task should join")
    .expect("https request should succeed");
    server.abort();

    assert_eq!(response.status(), 200);
    let body = response.into_string().expect("https response body");
    let value: serde_json::Value = serde_json::from_str(&body).expect("initialize json");
    assert!(value["result"]["protocolVersion"].is_string());
}

#[tokio::test]
async fn initialize_advertises_tools_list_changed_in_http_mode() {
    let app = build_router(test_state());
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

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    let value: serde_json::Value = serde_json::from_str(&body).expect("initialize json");
    assert_eq!(
        value["result"]["capabilities"]["tools"]["listChanged"],
        serde_json::json!(true)
    );
    assert_eq!(
        value["result"]["capabilities"]["resources"]["listChanged"],
        serde_json::json!(false)
    );
}

#[tokio::test]
async fn initialize_persists_client_metadata() {
    let state = test_state();
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("x-codelens-trusted-client", "true")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA","version":"2.1.0"},"profile":"reviewer-graph"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = resp
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();
    let session = state.session_store.as_ref().unwrap().get(&sid).unwrap();
    let metadata = session.client_metadata();
    assert_eq!(metadata.client_name.as_deref(), Some("HarnessQA"));
    assert_eq!(metadata.client_version.as_deref(), Some("2.1.0"));
    assert_eq!(
        metadata.requested_profile.as_deref(),
        Some("reviewer-graph")
    );
    assert_eq!(metadata.trusted_client, Some(true));
    assert_eq!(metadata.deferred_tool_loading, None);
    assert!(metadata.loaded_namespaces.is_empty());
    assert!(metadata.loaded_tiers.is_empty());
    assert_eq!(metadata.full_tool_exposure, None);
}

#[tokio::test]
async fn initialize_profile_sets_http_session_surface_and_tools_list() {
    let state = test_state();
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA","version":"2.1.0"},"profile":"reviewer-graph"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let session = state.session_store.as_ref().unwrap().get(&sid).unwrap();
    assert_eq!(
        session.surface(),
        crate::tool_defs::ToolSurface::Profile(crate::tool_defs::ToolProfile::ReviewerGraph)
    );
    assert_eq!(
        session.token_budget(),
        crate::tool_defs::default_budget_for_profile(crate::tool_defs::ToolProfile::ReviewerGraph)
    );

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("\"active_surface\":\"reviewer-graph\""));
    assert!(body.contains("\"get_ranked_context\""));
    assert!(body.contains("\"get_callers\""));
    assert!(body.contains("\"start_analysis_job\""));
    assert!(!body.contains("\"review_architecture\""));
    assert!(!body.contains("\"review_changes\""));
    assert!(!body.contains("\"cleanup_duplicate_logic\""));
    assert!(!body.contains("\"analyze_change_impact\""));
    assert!(!body.contains("\"audit_security_context\""));
    assert!(!body.contains("\"assess_change_readiness\""));
}

#[tokio::test]
async fn initialize_persists_deferred_loading_preference() {
    let state = test_state();
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA","version":"2.1.0"},"profile":"reviewer-graph","deferredToolLoading":true}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = resp
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();
    let session = state.session_store.as_ref().unwrap().get(&sid).unwrap();
    let metadata = session.client_metadata();
    assert_eq!(metadata.deferred_tool_loading, Some(true));
    assert!(metadata.loaded_namespaces.is_empty());
    assert!(metadata.loaded_tiers.is_empty());
    assert_eq!(metadata.full_tool_exposure, None);
}

#[tokio::test]
async fn initialize_codex_defaults_to_deferred_loading() {
    let state = test_state();
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"CodexHarness","version":"1.0.0"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = resp
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();
    let session = state.session_store.as_ref().unwrap().get(&sid).unwrap();
    let metadata = session.client_metadata();
    assert_eq!(metadata.deferred_tool_loading, Some(true));
}

#[tokio::test]
async fn initialize_claude_defaults_to_full_contract() {
    let state = test_state();
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"Claude Code","version":"1.0.0"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = resp
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();
    let session = state.session_store.as_ref().unwrap().get(&sid).unwrap();
    let metadata = session.client_metadata();
    assert_eq!(metadata.deferred_tool_loading, Some(false));
}

#[tokio::test]
async fn codex_session_client_name_affects_activate_project_budget() {
    let state = test_state();
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"CodexHarness","version":"1.0.0"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let primitive_list = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"tier":"primitive"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(primitive_list.status(), StatusCode::OK);

    let activate = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"activate_project","arguments":{}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(activate.status(), StatusCode::OK);
    let payload = first_tool_payload(&body_string(activate).await);
    assert_eq!(payload["success"], serde_json::json!(true));
    assert_eq!(
        payload["data"]["auto_surface"],
        serde_json::json!("builder-minimal")
    );
    assert_eq!(payload["data"]["auto_budget"], serde_json::json!(6000));
}

#[tokio::test]
async fn session_binding_rebinds_project_per_request() {
    let project_a = temp_project_dir("project-a");
    let project_b = temp_project_dir("project-b");
    std::fs::write(
        project_a.join("first.py"),
        "def first_only():\n    return 1\n",
    )
    .unwrap();
    std::fs::write(
        project_b.join("second.py"),
        "def second_only():\n    return 2\n",
    )
    .unwrap();

    let project = ProjectRoot::new(project_a.to_str().unwrap()).unwrap();
    let state = Arc::new(
        AppState::new(project, crate::tool_defs::ToolPreset::Balanced).with_session_store(),
    );
    let app = build_router(state.clone());

    let init_a = app
        .clone()
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
    let sid_a = init_a
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let init_b = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let sid_b = init_b
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let activate_b = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_b)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"activate_project","arguments":{{"project":"{}"}}}}}}"#,
                    project_b.display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(activate_b.status(), StatusCode::OK);

    let find_second = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_b)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"find_symbol","arguments":{"name":"second_only","include_body":false,"max_matches":5}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let second_body = body_string(find_second).await;
    assert!(second_body.contains("second_only"));
    assert!(second_body.contains("second.py"));

    let find_first = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_a)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"find_symbol","arguments":{"name":"first_only","include_body":false,"max_matches":5}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let first_body = body_string(find_first).await;
    assert!(first_body.contains("first_only"));
    assert!(first_body.contains("first.py"));
}

#[tokio::test]
async fn analysis_jobs_follow_session_bound_project_scope() {
    let project_a = temp_project_dir("analysis-a");
    let project_b = temp_project_dir("analysis-b");
    std::fs::write(
        project_a.join("first.py"),
        "def first_only():\n    return 1\n",
    )
    .unwrap();
    std::fs::write(
        project_b.join("second.py"),
        "def second_only():\n    return 2\n",
    )
    .unwrap();

    let project = ProjectRoot::new(project_a.to_str().unwrap()).unwrap();
    let state = Arc::new(
        AppState::new(project, crate::tool_defs::ToolPreset::Balanced).with_session_store(),
    );
    let app = build_router(state);

    let init_a = app
        .clone()
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
    let sid_a = init_a
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let init_b = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let sid_b = init_b
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let activate_b = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_b)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"activate_project","arguments":{{"project":"{}"}}}}}}"#,
                    project_b.display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(activate_b.status(), StatusCode::OK);

    let set_profile_b = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_b)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"set_profile","arguments":{"profile":"reviewer-graph"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(set_profile_b.status(), StatusCode::OK);

    let start = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_b)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"start_analysis_job","arguments":{"kind":"impact_report","path":"second.py","profile_hint":"reviewer-graph"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start.status(), StatusCode::OK);
    let start_payload = first_tool_payload(&body_string(start).await);
    let job_id = start_payload["data"]["job_id"]
        .as_str()
        .expect("job id")
        .to_owned();
    assert!(start_payload["data"]["summary_resource"].is_null());
    assert_eq!(
        start_payload["data"]["section_handles"],
        serde_json::json!([])
    );

    // Poll schedule: fewer calls, longer sleeps to keep us well under the
    // 300-calls/minute per-session rate limit. Total wall budget: ~30 s
    // (200 polls × 150 ms), which is plenty for impact_report on a
    // 2-file tempdir project even on congested CI.
    let mut analysis_id = None;
    let mut last_poll_payload = None;
    for _ in 0..200 {
        let poll = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header("content-type", "application/json")
                    .header("mcp-session-id", &sid_b)
                    .body(axum::body::Body::from(format!(
                        r#"{{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{{"name":"get_analysis_job","arguments":{{"job_id":"{}"}}}}}}"#,
                        job_id
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        let poll_payload = first_tool_payload(&body_string(poll).await);
        last_poll_payload = Some(poll_payload.clone());
        if let Some(id) = poll_payload["data"]["analysis_id"].as_str() {
            assert!(
                poll_payload["data"]["summary_resource"]["uri"]
                    .as_str()
                    .map(|uri| uri.ends_with("/summary"))
                    .unwrap_or(false)
            );
            assert!(
                poll_payload["data"]["section_handles"]
                    .as_array()
                    .map(|items| !items.is_empty())
                    .unwrap_or(false)
            );
            analysis_id = Some(id.to_owned());
            break;
        }
        if matches!(
            poll_payload["data"]["status"].as_str(),
            Some("error") | Some("cancelled")
        ) {
            panic!("analysis job did not complete successfully: {poll_payload}");
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    let analysis_id = analysis_id.unwrap_or_else(|| {
        panic!(
            "analysis_id after completion; last poll payload: {}",
            last_poll_payload.unwrap_or_default()
        )
    });
    let section = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_b)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{{"name":"get_analysis_section","arguments":{{"analysis_id":"{}","section":"impact_rows"}}}}}}"#,
                    analysis_id
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    let section_payload = first_tool_payload(&body_string(section).await);
    assert_eq!(section_payload["success"], serde_json::json!(true));
    assert!(
        section_payload["data"]["content"]
            .to_string()
            .contains("second.py")
    );

    let foreign_poll = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_a)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{{"name":"get_analysis_job","arguments":{{"job_id":"{}"}}}}}}"#,
                    job_id
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    let foreign_body = body_string(foreign_poll).await;
    assert!(foreign_body.contains("unknown job_id"));
}

#[tokio::test]
async fn session_bound_missing_project_fails_closed() {
    let state = test_state();
    let app = build_router(state.clone());

    let init = app
        .clone()
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
    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let missing = temp_project_dir("missing").join("gone");
    state
        .session_store
        .as_ref()
        .unwrap()
        .get(&sid)
        .unwrap()
        .set_project_path(missing.to_string_lossy().to_string());

    let find = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"find_symbol","arguments":{"name":"hello","include_body":false,"max_matches":5}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let body = body_string(find).await;
    assert!(body.contains("automatic rebind failed"));
}

#[tokio::test]
async fn session_profiles_are_isolated_across_tools_list() {
    let state = test_state();
    let app = build_router(state.clone());

    let init_a = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"CodexHarness","version":"1.0.0"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let sid_a = init_a
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let init_b = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"clientInfo":{"name":"CodexHarness","version":"1.0.0"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let sid_b = init_b
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let set_a = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_a)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"set_profile","arguments":{"profile":"builder-minimal"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(set_a.status(), StatusCode::OK);
    let set_a_body = body_string(set_a).await;
    let set_a_payload = first_tool_payload(&set_a_body);
    assert_eq!(
        set_a_payload["success"],
        serde_json::json!(true),
        "set_profile(session A) failed: {set_a_body}"
    );
    assert_eq!(
        set_a_payload["data"]["current_profile"],
        serde_json::json!("builder-minimal"),
        "unexpected set_profile(session A) payload: {set_a_body}"
    );

    let set_b = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_b)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"set_profile","arguments":{"profile":"reviewer-graph"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(set_b.status(), StatusCode::OK);
    let set_b_body = body_string(set_b).await;
    let set_b_payload = first_tool_payload(&set_b_body);
    assert_eq!(
        set_b_payload["success"],
        serde_json::json!(true),
        "set_profile(session B) failed: {set_b_body}"
    );
    assert_eq!(
        set_b_payload["data"]["current_profile"],
        serde_json::json!("reviewer-graph"),
        "unexpected set_profile(session B) payload: {set_b_body}"
    );

    let list_a = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_a)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":5,"method":"tools/list","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let list_a_body = body_string(list_a).await;
    assert!(list_a_body.contains("\"active_surface\":\"builder-minimal\""));

    let list_b = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid_b)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":6,"method":"tools/list","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let list_b_body = body_string(list_b).await;
    assert!(list_b_body.contains("\"active_surface\":\"reviewer-graph\""));
}

#[tokio::test]
async fn codex_session_prepare_harness_session_bootstraps_without_tools_list() {
    let state = test_state();
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"CodexHarness","version":"1.0.0"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let bootstrap = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"prepare_harness_session","arguments":{{"project":"{}","preferred_entrypoints":["explore_codebase","plan_safe_refactor"]}}}}}}"#,
                    state.project().as_path().display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(bootstrap.status(), StatusCode::OK);
    let payload = first_tool_payload(&body_string(bootstrap).await);
    assert_eq!(payload["success"], serde_json::json!(true));
    assert_eq!(
        payload["data"]["project"]["auto_surface"],
        serde_json::json!("builder-minimal")
    );
    assert_eq!(
        payload["data"]["active_surface"],
        serde_json::json!("builder-minimal")
    );
    assert_eq!(payload["data"]["token_budget"], serde_json::json!(6000));
    assert_eq!(
        payload["data"]["http_session"]["default_tools_list_contract_mode"],
        serde_json::json!("lean")
    );
    assert_eq!(
        payload["data"]["routing"]["recommended_entrypoint"],
        serde_json::json!("explore_codebase")
    );
    let tool_names = payload["data"]["visible_tools"]["tool_names"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        tool_names
            .iter()
            .any(|value| value == "prepare_harness_session")
    );
}

#[tokio::test]
async fn post_get_capabilities_returns_machine_readable_guidance() {
    let state = test_state();
    let app = build_router(state.clone());

    std::fs::write(state.project().as_path().join("notes.unknown"), "hello\n").unwrap();

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_capabilities","arguments":{"file_path":"notes.unknown"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let payload = first_tool_payload(&body_string(resp).await);
    assert_eq!(payload["success"], serde_json::json!(true));
    assert_eq!(
        payload["data"]["diagnostics_guidance"]["status"],
        serde_json::json!("unsupported_extension")
    );
    assert_eq!(
        payload["data"]["diagnostics_guidance"]["reason_code"],
        serde_json::json!("diagnostics_unsupported_extension")
    );
    assert_eq!(
        payload["data"]["diagnostics_guidance"]["recommended_action"],
        serde_json::json!("pass_explicit_lsp_command")
    );
    assert_eq!(
        payload["data"]["diagnostics_guidance"]["file_extension"],
        serde_json::json!("unknown")
    );
    assert!(payload["data"]["daemon_binary_drift"]["status"].is_string());
}

#[tokio::test]
async fn codex_session_uses_lean_tools_list_contract_by_default() {
    let state = test_state();
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"CodexHarness","version":"1.0.0"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("\"client_profile\":\"codex\""));
    assert!(body.contains("\"default_contract_mode\":\"lean\""));
    assert!(body.contains("\"include_output_schema\":false"));
    assert!(body.contains("\"include_annotations\":false"));
    assert!(!body.contains("\"outputSchema\""));
    assert!(!body.contains("\"annotations\""));
    assert!(!body.contains("\"visible_namespaces\""));
}

#[tokio::test]
async fn claude_session_uses_full_tools_list_contract_by_default() {
    let state = test_state();
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"Claude Code","version":"1.0.0"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("\"client_profile\":\"claude\""));
    assert!(body.contains("\"default_contract_mode\":\"full\""));
    assert!(body.contains("\"include_output_schema\":true"));
    assert!(body.contains("\"include_annotations\":true"));
    assert!(body.contains("\"outputSchema\""));
    assert!(body.contains("\"annotations\""));
    assert!(body.contains("\"visible_namespaces\""));
}

#[tokio::test]
async fn codex_session_can_restore_tool_annotations_explicitly() {
    let state = test_state();
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"CodexHarness","version":"1.0.0"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"includeAnnotations":true}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("\"include_annotations\":true"));
    assert!(body.contains("\"annotations\""));
}

#[tokio::test]
async fn mutation_enabled_daemon_rejects_untrusted_client_mutation() {
    let state = test_state();
    state.configure_daemon_mode(crate::state::RuntimeDaemonMode::MutationEnabled);
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA"},"profile":"refactor-full"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let preflight = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"verify_change_readiness","arguments":{"task":"create audit_http.py","changed_files":["audit_http.py"]}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preflight.status(), StatusCode::OK);
    let preflight_body = body_string(preflight).await;
    assert!(
        preflight_body.contains("\\\"success\\\": true")
            || preflight_body.contains("\\\"success\\\":true")
    );

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"create_text_file","arguments":{"relative_path":"audit_http.py","content":"print('hi')","overwrite":true}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("requires a trusted HTTP client"));
}

#[tokio::test]
async fn verify_change_readiness_http_response_uses_slim_text_wrapper() {
    let state = test_state();
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"verify_change_readiness","arguments":{"task":"update hello.txt","changed_files":["hello.txt"]}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    let envelope: serde_json::Value = serde_json::from_str(&body).unwrap();
    let text_payload: serde_json::Value = serde_json::from_str(
        envelope["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("{}"),
    )
    .unwrap();
    assert!(text_payload["data"]["analysis_id"].is_string());
    assert!(text_payload["data"]["summary"].is_string());
    assert!(text_payload["data"]["readiness"].is_object());
    assert!(
        text_payload["data"]["summary_resource"]["uri"]
            .as_str()
            .map(|uri| uri.contains("codelens://analysis/"))
            .unwrap_or(false)
    );
    assert!(text_payload["data"]["section_handles"].is_array());
    assert!(text_payload["suggested_next_calls"].is_array());
    assert!(
        text_payload["suggested_next_calls"]
            .as_array()
            .map(|items| {
                items.iter().any(|entry| {
                    entry["tool"].as_str() == Some("get_analysis_section")
                        && entry["arguments"]["analysis_id"].is_string()
                })
            })
            .unwrap_or(false)
    );
    assert_eq!(text_payload["routing_hint"], serde_json::json!("async"));
    assert!(text_payload["data"].get("verifier_checks").is_none());
    assert!(text_payload["data"].get("blockers").is_none());
    assert!(text_payload["data"].get("available_sections").is_none());
    assert!(envelope["result"]["structuredContent"]["analysis_id"].is_string());
    assert!(envelope["result"]["structuredContent"]["verifier_checks"].is_array());
    assert!(envelope["result"]["structuredContent"]["blockers"].is_array());
}

#[tokio::test]
async fn mutation_enabled_daemon_audits_trusted_client_metadata() {
    let state = test_state();
    state.configure_daemon_mode(crate::state::RuntimeDaemonMode::MutationEnabled);
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::RefactorFull,
    ));
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("x-codelens-trusted-client", "true")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA","version":"2.2.0"},"profile":"refactor-full"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    // RefactorFull requires preflight before mutation
    let preflight = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .header("x-codelens-trusted-client", "true")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"verify_change_readiness","arguments":{"task":"create audit_http.py","changed_files":["audit_http.py"]}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preflight.status(), StatusCode::OK);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .header("x-codelens-trusted-client", "true")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"create_text_file","arguments":{"relative_path":"audit_http.py","content":"print('hi')","overwrite":true}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    let envelope: serde_json::Value = serde_json::from_str(&body).unwrap();
    let text = envelope["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("\"success\": true") || text.contains("\"success\":true"));
    let audit_path = state.audit_dir().join("mutation-audit.jsonl");
    let audit_body = std::fs::read_to_string(audit_path).unwrap();
    assert!(audit_body.contains("\"trusted_client\":true"));
    assert!(audit_body.contains("\"requested_profile\":\"refactor-full\""));
    assert!(audit_body.contains("\"client_name\":\"HarnessQA\""));
}

#[tokio::test]
async fn deferred_tools_list_uses_preferred_namespaces_for_session() {
    let state = test_state();
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA"},"profile":"reviewer-graph","deferredToolLoading":true}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("\"deferred_loading_active\":true"));
    assert!(
        body.contains("\"preferred_namespaces\":[\"reports\",\"graph\",\"symbols\",\"session\"]")
    );
    assert!(body.contains("\"preferred_tiers\":[\"workflow\"]"));
    assert!(body.contains("\"loaded_namespaces\":[]"));
    assert!(body.contains("\"loaded_tiers\":[]"));
    assert!(body.contains("\"review_architecture\""));
    assert!(body.contains("\"review_changes\""));
    assert!(body.contains("\"cleanup_duplicate_logic\""));
    assert!(!body.contains("\"analyze_change_impact\""));
    assert!(!body.contains("\"audit_security_context\""));
    assert!(!body.contains("\"find_symbol\""));
    assert!(!body.contains("\"read_file\""));
    let envelope: serde_json::Value = serde_json::from_str(&body).unwrap();
    let tool_names = envelope["result"]["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|tool| {
            tool.get("name")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        tool_names.iter().take(3).cloned().collect::<Vec<_>>(),
        vec![
            "review_architecture".to_owned(),
            "review_changes".to_owned(),
            "cleanup_duplicate_logic".to_owned(),
        ]
    );
}

#[tokio::test]
async fn refactor_deferred_tools_list_starts_preview_first_for_session() {
    let state = test_state();
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::RefactorFull,
    ));
    let app = build_router(state.clone());
    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA"},"profile":"refactor-full","deferredToolLoading":true}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("\"deferred_loading_active\":true"));
    assert!(body.contains("\"preferred_namespaces\":[\"reports\",\"session\"]"));
    assert!(body.contains("\"tool_count\":"));
    assert!(body.contains("\"plan_safe_refactor\""));
    assert!(body.contains("\"review_changes\""));
    assert!(body.contains("\"trace_request_path\""));
    assert!(!body.contains("\"analyze_change_impact\""));
    assert!(body.contains("\"activate_project\""));
    assert!(body.contains("\"set_profile\""));
    assert!(!body.contains("\"name\":\"rename_symbol\""));
    assert!(!body.contains("\"name\":\"replace_symbol_body\""));
    assert!(!body.contains("\"name\":\"refactor_extract_function\""));
    assert!(!body.contains("\"name\":\"verify_change_readiness\""));
    assert!(!body.contains("\"name\":\"refactor_safety_report\""));
    assert!(!body.contains("\"name\":\"safe_rename_report\""));
    let envelope: serde_json::Value = serde_json::from_str(&body).unwrap();
    let tool_names = envelope["result"]["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|tool| {
            tool.get("name")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        tool_names.iter().take(3).cloned().collect::<Vec<_>>(),
        vec![
            "plan_safe_refactor".to_owned(),
            "review_changes".to_owned(),
            "trace_request_path".to_owned(),
        ]
    );
}

#[tokio::test]
async fn deferred_resources_read_tracks_loaded_namespaces_for_session() {
    let state = test_state();
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));
    let app = build_router(state.clone());

    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA"},"profile":"reviewer-graph","deferredToolLoading":true}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let summary = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"codelens://tools/list"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(summary.status(), StatusCode::OK);
    let summary_body = body_string(summary).await;
    let summary_text = first_resource_text(&summary_body);
    assert!(summary_text.contains("\"loaded_namespaces\": []"));
    assert!(summary_text.contains("\"loaded_tiers\": []"));
    assert!(!summary_text.contains("\"filesystem\":"));
    assert!(!summary_text.contains("\"find_symbol\""));

    let expand = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    // Use lsp namespace — get_file_diagnostics is in reviewer-graph but lsp is not preferred
                    r#"{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"codelens://tools/list","namespace":"lsp"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(expand.status(), StatusCode::OK);
    let expand_body = body_string(expand).await;
    let expand_text = first_resource_text(&expand_body);
    assert!(expand_text.contains("\"selected_namespace\": \"lsp\""));
    assert!(expand_text.contains("\"get_file_diagnostics\""));

    let tier_expand = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":"codelens://tools/list","tier":"primitive"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(tier_expand.status(), StatusCode::OK);
    let tier_expand_body = body_string(tier_expand).await;
    let tier_expand_text = first_resource_text(&tier_expand_body);
    assert!(tier_expand_text.contains("\"selected_tier\": \"primitive\""));
    assert!(tier_expand_text.contains("\"find_symbol\""));

    let summary_after = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":5,"method":"resources/read","params":{"uri":"codelens://tools/list"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(summary_after.status(), StatusCode::OK);
    let summary_after_body = body_string(summary_after).await;
    let summary_after_text = first_resource_text(&summary_after_body);
    assert!(summary_after_text.contains("\"loaded_namespaces\": ["));
    assert!(summary_after_text.contains("\"lsp\""));
    assert!(summary_after_text.contains("\"loaded_tiers\": ["));
    assert!(summary_after_text.contains("\"primitive\""));
    assert!(summary_after_text.contains("\"effective_namespaces\": ["));
    assert!(summary_after_text.contains("\"effective_tiers\": ["));
    assert!(summary_after_text.contains("\"get_file_diagnostics\""));
    assert!(summary_after_text.contains("\"find_symbol\""));

    let session_resource = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":6,"method":"resources/read","params":{"uri":"codelens://session/http"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(session_resource.status(), StatusCode::OK);
    let session_body = body_string(session_resource).await;
    let session_text = first_resource_text(&session_body);
    assert!(session_text.contains("\"loaded_namespaces\": ["));
    assert!(session_text.contains("\"lsp\""));
    assert!(session_text.contains("\"loaded_tiers\": ["));
    assert!(session_text.contains("\"primitive\""));
    assert!(session_text.contains("\"full_tool_exposure\": false"));
    assert!(session_text.contains("\"preferred_tiers\": ["));
    assert!(session_text.contains("\"workflow\""));
    assert!(session_text.contains("\"client_profile\": \"generic\""));
    assert!(session_text.contains("\"default_tools_list_contract_mode\": \"full\""));
    assert!(session_text.contains("\"semantic_search_status\":"));
    assert!(session_text.contains("\"supported_files\":"));
    assert!(session_text.contains("\"stale_files\":"));
    assert!(session_text.contains("\"daemon_binary_drift\":"));
    assert!(session_text.contains("\"health_summary\":"));
    assert!(session_text.contains("\"deferred_tier_gate\": true"));
    assert!(session_text.contains("\"requires_tier_listing_before_tool_call\": true"));
}

#[tokio::test]
async fn deferred_session_blocks_hidden_tool_calls_until_namespace_is_loaded() {
    let state = test_state();
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));
    let app = build_router(state.clone());
    let file_path = state.project().as_path().join("deferred-hidden.py");
    std::fs::write(&file_path, "def alpha():\n    return 1\n").unwrap();

    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA"},"profile":"reviewer-graph","deferredToolLoading":true}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    // get_file_diagnostics is in reviewer-graph but namespace "lsp" is not preferred,
    // so it should be hidden by deferred namespace loading.
    let blocked = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"get_file_diagnostics","arguments":{{"file_path":"{}"}}}}}}"#,
                    file_path.display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(blocked.status(), StatusCode::OK);
    let blocked_body = body_string(blocked).await;
    assert!(blocked_body.contains("hidden by deferred loading"));
}

#[tokio::test]
async fn deferred_namespace_load_allows_listed_graph_tool_call() {
    let state = test_state();
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));
    let app = build_router(state.clone());

    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA"},"profile":"reviewer-graph","deferredToolLoading":true}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let graph_list = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"namespace":"graph"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(graph_list.status(), StatusCode::OK);
    let graph_body = body_string(graph_list).await;
    assert!(graph_body.contains("\"selected_namespace\":\"graph\""));
    assert!(graph_body.contains("\"get_callers\""));

    let callers = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_callers","arguments":{"function_name":"missing_smoke_target","max_results":1}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(callers.status(), StatusCode::OK);
    let callers_body = body_string(callers).await;
    assert!(!callers_body.contains("hidden by deferred loading"));
}

#[tokio::test]
async fn deferred_namespace_load_expands_default_surface_and_allows_calls() {
    let state = test_state();
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));
    let app = build_router(state.clone());
    let mock_lsp = concat!(
        "#!/usr/bin/env python3\n",
        "import sys, json\n",
        "def read_msg():\n",
        "    h = ''\n",
        "    while True:\n",
        "        c = sys.stdin.buffer.read(1)\n",
        "        if not c: return None\n",
        "        h += c.decode('ascii')\n",
        "        if h.endswith('\\r\\n\\r\\n'): break\n",
        "    length = int([l for l in h.split('\\r\\n') if l.startswith('Content-Length:')][0].split(': ')[1])\n",
        "    return json.loads(sys.stdin.buffer.read(length).decode('utf-8'))\n",
        "def send(r):\n",
        "    out = json.dumps(r)\n",
        "    b = out.encode('utf-8')\n",
        "    sys.stdout.buffer.write(f'Content-Length: {len(b)}\\r\\n\\r\\n'.encode('ascii'))\n",
        "    sys.stdout.buffer.write(b)\n",
        "    sys.stdout.buffer.flush()\n",
        "while True:\n",
        "    msg = read_msg()\n",
        "    if msg is None: break\n",
        "    rid = msg.get('id')\n",
        "    m = msg.get('method', '')\n",
        "    if m == 'initialized': continue\n",
        "    if rid is None: continue\n",
        "    if m == 'initialize':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':{'capabilities':{'textDocumentSync':1,'diagnosticProvider':{}}}})\n",
        "    elif m == 'textDocument/diagnostic':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':{'kind':'full','items':[]}})\n",
        "    elif m == 'shutdown':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        "    else:\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
    );
    let unique_name = format!("deferred-open-{}.py", std::process::id());
    let file_path = state.project().as_path().join(&unique_name);
    let mock_path = state.project().as_path().join("mock_lsp.py");
    std::fs::write(&mock_path, mock_lsp).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&mock_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(&file_path, "def beta():\n    return 2\n").unwrap();

    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA"},"profile":"reviewer-graph","deferredToolLoading":true}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    // Load tier "primitive" first — then namespace "lsp" becomes visible
    let tier_expand = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"tier":"primitive"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(tier_expand.status(), StatusCode::OK);
    let tier_body = body_string(tier_expand).await;
    assert!(tier_body.contains("\"selected_tier\":\"primitive\""));
    assert!(tier_body.contains("\"find_symbol\""));

    // Now load namespace "lsp" — get_file_diagnostics should appear
    let ns_expand = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/list","params":{"namespace":"lsp"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(ns_expand.status(), StatusCode::OK);
    let ns_body = body_string(ns_expand).await;
    assert!(ns_body.contains("\"selected_namespace\":\"lsp\""));
    assert!(ns_body.contains("\"get_file_diagnostics\""));

    // Verify expanded namespace allows the tool call using a deterministic mock LSP.
    let allowed = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{{"name":"get_file_diagnostics","arguments":{{"file_path":"{}","command":"python3","args":["{}"]}}}}}}"#,
                    file_path.display(),
                    mock_path.display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(allowed.status(), StatusCode::OK);
    let allowed_body = body_string(allowed).await;
    assert!(
        allowed_body.contains("\\\"success\\\": true")
            || allowed_body.contains("\\\"success\\\":true"),
        "deferred_namespace body: {allowed_body}"
    );
}

#[tokio::test]
async fn deferred_tier_load_expands_default_surface_and_allows_calls() {
    let state = test_state();
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));
    let app = build_router(state.clone());
    let file_path = state.project().as_path().join("deferred-tier.py");
    std::fs::write(&file_path, "def beta():\n    return 2\n").unwrap();

    let init = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA"},"profile":"reviewer-graph","deferredToolLoading":true}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let blocked = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"find_symbol","arguments":{{"name":"beta","file_path":"{}","include_body":false}}}}}}"#,
                    file_path.display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(blocked.status(), StatusCode::OK);
    let blocked_body = body_string(blocked).await;
    assert!(blocked_body.contains("hidden by deferred loading in tier `primitive`"));

    let expand = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/list","params":{"tier":"primitive"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(expand.status(), StatusCode::OK);
    let expand_body = body_string(expand).await;
    assert!(expand_body.contains("\"selected_tier\":\"primitive\""));
    assert!(expand_body.contains("\"find_symbol\""));

    let default_list = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":4,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(default_list.status(), StatusCode::OK);
    let default_body = body_string(default_list).await;
    assert!(default_body.contains("\"loaded_tiers\":[\"primitive\"]"));
    assert!(default_body.contains("\"find_symbol\""));

    let allowed = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{{"name":"find_symbol","arguments":{{"name":"beta","file_path":"{}","include_body":false}}}}}}"#,
                    file_path.display()
                )))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(allowed.status(), StatusCode::OK);
    let allowed_body = body_string(allowed).await;
    assert!(
        allowed_body.contains("\\\"success\\\": true")
            || allowed_body.contains("\\\"success\\\":true")
    );
}

#[tokio::test]
async fn initialize_with_existing_session_resumes_same_session() {
    let state = test_state();
    let app = build_router(state.clone());
    let first = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let sid = first
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();

    let second = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"clientInfo":{"name":"HarnessQA","version":"2.2.0"},"profile":"planner-readonly"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        second
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok()),
        Some(sid.as_str())
    );
    assert_eq!(
        second
            .headers()
            .get("x-codelens-session-resumed")
            .and_then(|value| value.to_str().ok()),
        Some("true")
    );
    let body = body_string(second).await;
    assert!(body.contains("\"resumed\":true"));
    assert!(body.contains(&sid));

    let session = state.session_store.as_ref().unwrap().get(&sid).unwrap();
    assert_eq!(session.resume_count(), 1);
    let metadata = session.client_metadata();
    assert_eq!(metadata.client_version.as_deref(), Some("2.2.0"));
    assert_eq!(
        metadata.requested_profile.as_deref(),
        Some("planner-readonly")
    );
}

#[tokio::test]
async fn post_invalid_json_returns_parse_error() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from("not json"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(
        body.contains("-32700"),
        "should return JSON-RPC parse error code"
    );
}

#[tokio::test]
async fn post_non_initialize_without_session_works() {
    // Non-initialize requests without session ID should still work
    // (session validation only rejects unknown session IDs, not missing ones)
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(
        body.contains("get_ranked_context"),
        "tools/list should return tools"
    );
}

#[tokio::test]
async fn post_unknown_session_returns_not_found() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", "nonexistent-session-id")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn post_with_sse_accept_returns_event_stream() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("accept", "text/event-stream")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers().get("mcp-session-id").is_some(),
        "SSE response should also include session ID"
    );
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.contains("text/event-stream"),
        "Accept: text/event-stream should return SSE content-type, got: {ct}"
    );
}

#[tokio::test]
async fn server_card_exposes_daemon_mode() {
    let state = test_state();
    state.configure_daemon_mode(crate::state::RuntimeDaemonMode::ReadOnly);
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/.well-known/mcp.json")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("\"daemon_mode\": \"read-only\""));
    assert!(body.contains("session-client-metadata"));
    assert!(body.contains("\"surface_manifest\""));
}

#[tokio::test]
async fn server_card_advertises_supported_protocol_versions() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/.well-known/mcp.json")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(
        body.contains(r#""latestProtocolVersion": "2025-11-25""#),
        "card should pin latest version, got: {body}"
    );
    assert!(
        body.contains(r#""supportedProtocolVersions""#)
            && body.contains(r#""2025-03-26""#)
            && body.contains(r#""2025-06-18""#)
            && body.contains(r#""2025-11-25""#),
        "card should list all supported versions, got: {body}"
    );
}

#[tokio::test]
async fn post_notification_returns_accepted() {
    // Spec §"Sending Messages to the Server" item 4: JSON-RPC notifications
    // (no `id`) and responses MUST yield 202 Accepted, not 204 No Content.
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::ACCEPTED);
}

// ── GET /mcp (SSE stream) ────────────────────────────────────────────

#[tokio::test]
async fn get_without_session_returns_bad_request() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mcp")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn get_with_unknown_session_returns_not_found() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mcp")
                .header("mcp-session-id", "bogus-id")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn set_profile_emits_tools_list_changed_notification_over_sse() {
    let app = build_router(test_state());
    let init = app
        .clone()
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
    let sid = init
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_owned();
    let _ = body_string(init).await;

    let sse = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mcp")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(sse.status(), StatusCode::OK);

    let set_profile = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"set_profile","arguments":{"profile":"reviewer-graph"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(set_profile.status(), StatusCode::OK);

    let chunk = next_sse_chunk(sse).await;
    assert!(
        chunk.contains("event: message"),
        "unexpected SSE event envelope: {chunk}"
    );
    assert!(
        chunk.contains(r#""method":"notifications/tools/list_changed""#),
        "expected tools/list_changed notification, got: {chunk}"
    );
}

// ── DELETE /mcp (session termination) ────────────────────────────────

#[tokio::test]
async fn delete_returns_no_content() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/mcp")
                .header("mcp-session-id", "any-id")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn delete_removes_session() {
    let state = test_state();
    let session = state.session_store.as_ref().unwrap().create();
    let sid = session.id.clone();

    // Verify session exists
    assert!(state.session_store.as_ref().unwrap().get(&sid).is_some());

    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/mcp")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert!(
        state.session_store.as_ref().unwrap().get(&sid).is_none(),
        "session should be removed after DELETE"
    );
}

// ── Session lifecycle ────────────────────────────────────────────────

#[tokio::test]
async fn full_session_lifecycle() {
    let state = test_state();
    let app = build_router(state.clone());

    // 1. Initialize — get session ID
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

    let sid = resp
        .headers()
        .get("mcp-session-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(!sid.is_empty());

    // 2. Use session for a tool call
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("get_ranked_context"));

    // 3. Terminate session
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/mcp")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // 4. Verify session is gone
    let app = build_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-session-id", &sid)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── Session store edge cases ─────────────────────────────────────────

#[test]
fn concurrent_session_creation() {
    let store = SessionStore::new(Duration::from_secs(300));
    let sessions: Vec<_> = (0..100).map(|_| store.create()).collect();

    // All IDs unique
    let mut ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 100, "all 100 session IDs should be unique");
    assert_eq!(store.len(), 100);
}

#[test]
fn session_touch_refreshes_expiry() {
    let store = SessionStore::new(Duration::from_millis(50));
    let session = store.create();
    let id = session.id.clone();

    std::thread::sleep(Duration::from_millis(30));
    // Touch should reset the timer
    store.get(&id); // get() calls touch()
    std::thread::sleep(Duration::from_millis(30));

    // 60ms total but touched at 30ms, so 30ms since touch < 50ms timeout
    assert!(
        store.get(&id).is_some(),
        "session should still be alive after touch"
    );
}

#[test]
fn cleanup_only_removes_expired() {
    let store = SessionStore::new(Duration::from_millis(20));
    let s1 = store.create();
    std::thread::sleep(Duration::from_millis(30));
    let s2 = store.create(); // created after sleep, still fresh

    let removed = store.cleanup();
    assert_eq!(removed, 1, "only the expired session should be removed");
    assert!(store.get(&s1.id).is_none());
    assert!(store.get(&s2.id).is_some());
}

// ── 2025-06-18 compliance ────────────────────────────────────────────

#[tokio::test]
async fn post_with_unsupported_protocol_version_header_returns_bad_request() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-protocol-version", "1999-01-01")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn post_with_supported_protocol_version_header_is_accepted() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("mcp-protocol-version", "2025-06-18")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn initialize_echoes_requested_supported_protocol_version() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(
        body.contains(r#""protocolVersion":"2025-06-18""#),
        "expected 2025-06-18 echoed, got: {body}"
    );
}

#[tokio::test]
async fn initialize_falls_back_to_latest_for_unknown_client_version() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(
        body.contains(r#""protocolVersion":"2025-11-25""#),
        "expected latest fallback, got: {body}"
    );
}

#[tokio::test]
async fn post_from_remote_origin_is_forbidden() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("origin", "https://evil.example.com")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn post_from_localhost_origin_is_allowed() {
    let app = build_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("content-type", "application/json")
                .header("origin", "http://localhost:5173")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
