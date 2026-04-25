use axum::http::HeaderMap;
use jsonwebtoken::jwk::{Jwk, JwkSet, KeyAlgorithm};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::RwLock;
use std::time::{Duration, Instant};

const JWKS_CACHE_TTL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone)]
pub(crate) struct StaticJwks {
    value: Value,
}

impl StaticJwks {
    #[cfg(test)]
    pub(crate) fn new(value: Value) -> Self {
        Self { value }
    }
}

/// Box<JwksAuthConfig> would equalise variant size, but the enum is
/// constructed once at server startup and stored in Arc<HttpAuthState>;
/// the Off variant is rarely chosen in production and the per-request
/// match cost is dwarfed by jsonwebtoken validation. `#[allow]` instead
/// of boxing keeps construction code straightforward.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Default)]
pub(crate) enum HttpAuthConfig {
    #[default]
    Off,
    Jwks(JwksAuthConfig),
}

impl HttpAuthConfig {
    pub(crate) fn jwks(
        jwks_url: String,
        issuer: String,
        audience: String,
        required_scope: Option<String>,
    ) -> Self {
        Self::Jwks(JwksAuthConfig {
            jwks_url: Some(jwks_url),
            issuer: issuer.clone(),
            audience: audience.clone(),
            authorization_server: issuer,
            resource: audience,
            required_scope,
            static_jwks: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn jwks_with_static_keys_for_tests(
        jwks: StaticJwks,
        authorization_server: &str,
        resource: &str,
        required_scope: Option<&str>,
    ) -> Self {
        Self::Jwks(JwksAuthConfig {
            jwks_url: None,
            issuer: authorization_server.to_owned(),
            audience: resource.to_owned(),
            authorization_server: authorization_server.to_owned(),
            resource: resource.to_owned(),
            required_scope: required_scope.map(ToOwned::to_owned),
            static_jwks: Some(jwks),
        })
    }

    #[cfg(test)]
    pub(crate) fn jwks_static_for_test(
        jwks: Value,
        authorization_server: &str,
        resource: &str,
        required_scope: Option<&str>,
    ) -> Self {
        Self::jwks_with_static_keys_for_tests(
            StaticJwks::new(jwks),
            authorization_server,
            resource,
            required_scope,
        )
    }

    #[cfg(test)]
    pub(crate) fn jwks_remote_for_test(
        jwks_url: String,
        authorization_server: &str,
        resource: &str,
        required_scope: Option<&str>,
    ) -> Self {
        Self::Jwks(JwksAuthConfig {
            jwks_url: Some(jwks_url),
            issuer: authorization_server.to_owned(),
            audience: resource.to_owned(),
            authorization_server: authorization_server.to_owned(),
            resource: resource.to_owned(),
            required_scope: required_scope.map(ToOwned::to_owned),
            static_jwks: None,
        })
    }

    pub(crate) fn enabled(&self) -> bool {
        matches!(self, Self::Jwks(_))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct JwksAuthConfig {
    jwks_url: Option<String>,
    issuer: String,
    audience: String,
    authorization_server: String,
    resource: String,
    required_scope: Option<String>,
    static_jwks: Option<StaticJwks>,
}

#[derive(Debug)]
struct CachedJwks {
    value: Value,
    fetched_at: Instant,
}

#[derive(Debug, Default)]
pub(crate) struct HttpAuthState {
    config: RwLock<HttpAuthConfig>,
    cached_jwks: RwLock<Option<CachedJwks>>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AuthFailure {
    Missing,
    Invalid,
    InsufficientScope,
}

#[derive(Debug, Deserialize)]
struct Claims {
    scope: Option<String>,
    scp: Option<Vec<String>>,
}

impl HttpAuthState {
    pub(crate) fn configure(&self, config: HttpAuthConfig) {
        *self
            .config
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = config;
        *self
            .cached_jwks
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }

    pub(crate) fn config(&self) -> HttpAuthConfig {
        self.config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub(crate) async fn authorize(&self, headers: &HeaderMap) -> Result<(), AuthFailure> {
        let config = match self.config() {
            HttpAuthConfig::Off => return Ok(()),
            HttpAuthConfig::Jwks(config) => config,
        };
        let token = bearer_token(headers).ok_or(AuthFailure::Missing)?;
        let claims = self.validate_jwt(token, &config).await?;
        if let Some(required) = config.required_scope.as_deref()
            && !claims_has_scope(&claims, required)
        {
            return Err(AuthFailure::InsufficientScope);
        }
        Ok(())
    }

    async fn validate_jwt(
        &self,
        token: &str,
        config: &JwksAuthConfig,
    ) -> Result<Claims, AuthFailure> {
        let header = decode_header(token).map_err(|_| AuthFailure::Invalid)?;
        let kid = header.kid.as_deref().ok_or(AuthFailure::Invalid)?;
        let key = self.decoding_key_for_kid(kid, header.alg, config).await?;
        let mut validation = Validation::new(header.alg);
        validation.set_issuer(&[config.issuer.as_str()]);
        validation.set_audience(&[config.audience.as_str()]);
        decode::<Claims>(token, &key, &validation)
            .map(|data| data.claims)
            .map_err(|_| AuthFailure::Invalid)
    }

    async fn decoding_key_for_kid(
        &self,
        kid: &str,
        algorithm: Algorithm,
        config: &JwksAuthConfig,
    ) -> Result<DecodingKey, AuthFailure> {
        let keys = self.jwks_value(config, false).await?;
        if let Some(key) = decoding_key_from_jwks(keys, kid, algorithm)? {
            return Ok(key);
        }

        let refreshed_keys = self.jwks_value(config, true).await?;
        decoding_key_from_jwks(refreshed_keys, kid, algorithm)?.ok_or(AuthFailure::Invalid)
    }

    async fn jwks_value(
        &self,
        config: &JwksAuthConfig,
        force_refresh: bool,
    ) -> Result<Value, AuthFailure> {
        if let Some(static_jwks) = &config.static_jwks {
            return Ok(static_jwks.value.clone());
        }
        if !force_refresh {
            let guard = self
                .cached_jwks
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(cached) = guard.as_ref()
                && cached.fetched_at.elapsed() < JWKS_CACHE_TTL
            {
                return Ok(cached.value.clone());
            }
        }
        self.fetch_and_cache_jwks(config).await
    }

    async fn fetch_and_cache_jwks(&self, config: &JwksAuthConfig) -> Result<Value, AuthFailure> {
        let url = config.jwks_url.clone().ok_or(AuthFailure::Invalid)?;
        let fetched = tokio::task::spawn_blocking(move || fetch_jwks(&url))
            .await
            .map_err(|_| AuthFailure::Invalid)?
            .map_err(|_| AuthFailure::Invalid)?;
        *self
            .cached_jwks
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(CachedJwks {
            value: fetched.clone(),
            fetched_at: Instant::now(),
        });
        Ok(fetched)
    }

    pub(crate) fn protected_resource_metadata(&self) -> Option<Value> {
        match self.config() {
            HttpAuthConfig::Off => None,
            HttpAuthConfig::Jwks(config) => {
                let scopes = config
                    .required_scope
                    .as_deref()
                    .map(|scope| vec![scope.to_owned()])
                    .unwrap_or_default();
                Some(json!({
                    "resource": config.resource,
                    "authorization_servers": [config.authorization_server],
                    "scopes_supported": scopes,
                    "bearer_methods_supported": ["header"]
                }))
            }
        }
    }

    pub(crate) fn www_authenticate(&self) -> String {
        match self.config() {
            HttpAuthConfig::Off => "Bearer".to_owned(),
            HttpAuthConfig::Jwks(config) => {
                let metadata = protected_resource_metadata_url(&config.resource);
                match config.required_scope {
                    Some(scope) => {
                        format!("Bearer resource_metadata=\"{metadata}\", scope=\"{scope}\"")
                    }
                    None => format!("Bearer resource_metadata=\"{metadata}\""),
                }
            }
        }
    }
}

fn decoding_key_from_jwks(
    keys: Value,
    kid: &str,
    algorithm: Algorithm,
) -> Result<Option<DecodingKey>, AuthFailure> {
    let jwks: JwkSet = serde_json::from_value(keys).map_err(|_| AuthFailure::Invalid)?;
    let Some(jwk) = jwks.find(kid) else {
        return Ok(None);
    };
    ensure_jwk_algorithm_matches(jwk, algorithm)?;
    DecodingKey::from_jwk(jwk)
        .map(Some)
        .map_err(|_| AuthFailure::Invalid)
}

fn ensure_jwk_algorithm_matches(jwk: &Jwk, algorithm: Algorithm) -> Result<(), AuthFailure> {
    let Some(jwk_algorithm) = jwk.common.key_algorithm else {
        return Ok(());
    };
    if key_algorithm_to_jwt_algorithm(jwk_algorithm) == Some(algorithm) {
        Ok(())
    } else {
        Err(AuthFailure::Invalid)
    }
}

fn key_algorithm_to_jwt_algorithm(algorithm: KeyAlgorithm) -> Option<Algorithm> {
    match algorithm {
        KeyAlgorithm::HS256 => Some(Algorithm::HS256),
        KeyAlgorithm::HS384 => Some(Algorithm::HS384),
        KeyAlgorithm::HS512 => Some(Algorithm::HS512),
        KeyAlgorithm::ES256 => Some(Algorithm::ES256),
        KeyAlgorithm::ES384 => Some(Algorithm::ES384),
        KeyAlgorithm::RS256 => Some(Algorithm::RS256),
        KeyAlgorithm::RS384 => Some(Algorithm::RS384),
        KeyAlgorithm::RS512 => Some(Algorithm::RS512),
        KeyAlgorithm::PS256 => Some(Algorithm::PS256),
        KeyAlgorithm::PS384 => Some(Algorithm::PS384),
        KeyAlgorithm::PS512 => Some(Algorithm::PS512),
        KeyAlgorithm::EdDSA => Some(Algorithm::EdDSA),
        KeyAlgorithm::RSA1_5 | KeyAlgorithm::RSA_OAEP | KeyAlgorithm::RSA_OAEP_256 => None,
    }
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn claims_has_scope(claims: &Claims, required: &str) -> bool {
    claims
        .scope
        .as_deref()
        .map(|scope| scope.split_whitespace().any(|scope| scope == required))
        .unwrap_or(false)
        || claims
            .scp
            .as_ref()
            .map(|scopes| scopes.iter().any(|scope| scope == required))
            .unwrap_or(false)
}

fn protected_resource_metadata_url(resource: &str) -> String {
    match url::Url::parse(resource) {
        Ok(mut url) => {
            url.set_path("/.well-known/oauth-protected-resource");
            url.set_query(None);
            url.set_fragment(None);
            url.to_string()
        }
        Err(_) => "/.well-known/oauth-protected-resource".to_owned(),
    }
}

fn fetch_jwks(url: &str) -> Result<Value, String> {
    let body = ureq::get(url)
        .call()
        .map_err(|error| error.to_string())?
        .into_string()
        .map_err(|error| error.to_string())?;
    serde_json::from_str(&body).map_err(|error| error.to_string())
}
