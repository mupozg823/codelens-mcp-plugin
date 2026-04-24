#![cfg(feature = "http")]

use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

const DEFAULT_JWKS_TTL: Duration = Duration::from_secs(300);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HttpAuthMode {
    Off,
    Jwks,
}

#[derive(Clone, Debug)]
struct CachedJwks {
    value: Value,
    fetched_at: Instant,
}

#[derive(Clone, Debug)]
pub(crate) struct HttpAuthConfig {
    mode: HttpAuthMode,
    jwks_url: Option<String>,
    issuer: Option<String>,
    audience: Option<String>,
    scope: Option<String>,
    static_jwks: Option<Value>,
    ttl: Duration,
    cache: Arc<RwLock<Option<CachedJwks>>>,
}

#[derive(Debug)]
enum AuthFailure {
    Missing,
    Invalid,
    InsufficientScope,
}

#[derive(Debug, Deserialize)]
struct JwtClaims {
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    scp: Option<Vec<String>>,
}

impl HttpAuthConfig {
    pub(crate) fn off() -> Self {
        Self {
            mode: HttpAuthMode::Off,
            jwks_url: None,
            issuer: None,
            audience: None,
            scope: None,
            static_jwks: None,
            ttl: DEFAULT_JWKS_TTL,
            cache: Arc::new(RwLock::new(None)),
        }
    }

    pub(crate) fn jwks(
        jwks_url: String,
        issuer: String,
        audience: String,
        scope: Option<String>,
    ) -> Self {
        Self {
            mode: HttpAuthMode::Jwks,
            jwks_url: Some(jwks_url),
            issuer: Some(issuer),
            audience: Some(audience),
            scope,
            static_jwks: None,
            ttl: DEFAULT_JWKS_TTL,
            cache: Arc::new(RwLock::new(None)),
        }
    }

    #[cfg(test)]
    pub(crate) fn jwks_static_for_test(
        jwks: Value,
        issuer: &str,
        audience: &str,
        scope: Option<&str>,
    ) -> Self {
        Self {
            mode: HttpAuthMode::Jwks,
            jwks_url: None,
            issuer: Some(issuer.to_owned()),
            audience: Some(audience.to_owned()),
            scope: scope.map(ToOwned::to_owned),
            static_jwks: Some(jwks),
            ttl: DEFAULT_JWKS_TTL,
            cache: Arc::new(RwLock::new(None)),
        }
    }

    #[cfg(test)]
    pub(crate) fn jwks_remote_for_test(
        jwks_url: String,
        issuer: &str,
        audience: &str,
        scope: Option<&str>,
    ) -> Self {
        Self::jwks(
            jwks_url,
            issuer.to_owned(),
            audience.to_owned(),
            scope.map(ToOwned::to_owned),
        )
    }

    pub(crate) fn is_enabled(&self) -> bool {
        matches!(self.mode, HttpAuthMode::Jwks)
    }

    pub(crate) fn issuer(&self) -> Option<&str> {
        self.issuer.as_deref()
    }

    pub(crate) fn scope(&self) -> Option<&str> {
        self.scope.as_deref()
    }
}

impl Default for HttpAuthConfig {
    fn default() -> Self {
        Self::off()
    }
}

pub(crate) async fn authorize(headers: &HeaderMap, config: HttpAuthConfig) -> Result<(), Response> {
    if !config.is_enabled() {
        return Ok(());
    }
    authorize_jwks(headers, &config)
        .await
        .map_err(|failure| challenge_response(&config, failure))
}

pub(crate) fn protected_resource_metadata(config: &HttpAuthConfig) -> Value {
    let mut scopes = Vec::new();
    if let Some(scope) = config.scope() {
        scopes.push(json!(scope));
    }
    let mut authorization_servers = Vec::new();
    if let Some(issuer) = config.issuer() {
        authorization_servers.push(json!(issuer));
    }
    json!({
        "resource": "/mcp",
        "resource_name": "CodeLens MCP",
        "authorization_servers": authorization_servers,
        "scopes_supported": scopes,
        "bearer_methods_supported": ["header"],
    })
}

async fn authorize_jwks(headers: &HeaderMap, config: &HttpAuthConfig) -> Result<(), AuthFailure> {
    let token = bearer_token(headers).ok_or(AuthFailure::Missing)?;
    let header = decode_header(token).map_err(|_| AuthFailure::Invalid)?;
    let kid = header.kid.as_deref().ok_or(AuthFailure::Invalid)?;
    let key = decoding_key_for_kid(kid, config).await?;
    let issuer = config.issuer.as_deref().ok_or(AuthFailure::Invalid)?;
    let audience = config.audience.as_deref().ok_or(AuthFailure::Invalid)?;
    let mut validation = Validation::new(header.alg);
    validation.validate_nbf = true;
    validation.set_issuer(&[issuer]);
    validation.set_audience(&[audience]);
    validation.required_spec_claims.insert("iss".to_owned());
    validation.required_spec_claims.insert("aud".to_owned());
    let data = decode::<JwtClaims>(token, &key, &validation).map_err(|_| AuthFailure::Invalid)?;
    if !has_required_scope(&data.claims, config.scope.as_deref()) {
        return Err(AuthFailure::InsufficientScope);
    }
    Ok(())
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
}

fn has_required_scope(claims: &JwtClaims, required_scope: Option<&str>) -> bool {
    let Some(required_scope) = required_scope else {
        return true;
    };
    claims
        .scope
        .as_deref()
        .is_some_and(|scope| scope.split_whitespace().any(|item| item == required_scope))
        || claims
            .scp
            .as_ref()
            .is_some_and(|scopes| scopes.iter().any(|scope| scope == required_scope))
}

async fn decoding_key_for_kid(
    kid: &str,
    config: &HttpAuthConfig,
) -> Result<DecodingKey, AuthFailure> {
    let jwks = jwks_value(config, false).await?;
    if let Some(key) = key_from_jwks(kid, jwks)? {
        return Ok(key);
    }
    let refreshed = jwks_value(config, true).await?;
    key_from_jwks(kid, refreshed)?.ok_or(AuthFailure::Invalid)
}

async fn jwks_value(config: &HttpAuthConfig, force_refresh: bool) -> Result<Value, AuthFailure> {
    if let Some(static_jwks) = &config.static_jwks {
        return Ok(static_jwks.clone());
    }
    if !force_refresh
        && let Some(cached) = config
            .cache
            .read()
            .map_err(|_| AuthFailure::Invalid)?
            .as_ref()
            .filter(|cached| cached.fetched_at.elapsed() < config.ttl)
            .cloned()
    {
        return Ok(cached.value);
    }
    fetch_and_cache_jwks(config).await
}

async fn fetch_and_cache_jwks(config: &HttpAuthConfig) -> Result<Value, AuthFailure> {
    let url = config.jwks_url.clone().ok_or(AuthFailure::Invalid)?;
    let value = tokio::task::spawn_blocking(move || {
        ureq::get(&url)
            .call()
            .map_err(|_| AuthFailure::Invalid)?
            .into_json::<Value>()
            .map_err(|_| AuthFailure::Invalid)
    })
    .await
    .map_err(|_| AuthFailure::Invalid)??;
    *config.cache.write().map_err(|_| AuthFailure::Invalid)? = Some(CachedJwks {
        value: value.clone(),
        fetched_at: Instant::now(),
    });
    Ok(value)
}

fn key_from_jwks(kid: &str, jwks: Value) -> Result<Option<DecodingKey>, AuthFailure> {
    let set = serde_json::from_value::<JwkSet>(jwks).map_err(|_| AuthFailure::Invalid)?;
    let Some(jwk) = set.find(kid) else {
        return Ok(None);
    };
    DecodingKey::from_jwk(jwk)
        .map(Some)
        .map_err(|_| AuthFailure::Invalid)
}

fn challenge_response(config: &HttpAuthConfig, failure: AuthFailure) -> Response {
    let mut challenge =
        "Bearer resource_metadata=\"/.well-known/oauth-protected-resource/mcp\"".to_owned();
    if let Some(scope) = config.scope() {
        challenge.push_str(&format!(" scope=\"{scope}\""));
    }
    if matches!(failure, AuthFailure::InsufficientScope) {
        challenge.push_str(" error=\"insufficient_scope\"");
    }
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, challenge)],
        "Unauthorized",
    )
        .into_response()
}

pub(crate) fn metadata_response(config: HttpAuthConfig) -> Response {
    let body = serde_json::to_string_pretty(&protected_resource_metadata(&config))
        .unwrap_or_else(|_| "{}".to_owned());
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        body,
    )
        .into_response()
}
