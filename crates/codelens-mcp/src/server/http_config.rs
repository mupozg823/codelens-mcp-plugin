#![cfg(feature = "http")]

use super::auth::HttpAuthConfig;
use super::transport_http::{HttpServerConfig, TlsConfig};
use crate::cli::cli_option_value;
use crate::env_compat::dual_prefix_env;
use anyhow::Result;
use std::net::IpAddr;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(crate) struct HttpRuntimeOptions {
    pub(crate) transport: String,
    pub(crate) listen: IpAddr,
    pub(crate) port: u16,
    pub(crate) auth: HttpAuthConfig,
    pub(crate) tls: Option<TlsConfig>,
}

impl HttpRuntimeOptions {
    pub(crate) fn server_config(&self) -> HttpServerConfig {
        HttpServerConfig {
            listen: self.listen,
            port: self.port,
            tls: self.tls.clone(),
        }
    }
}

pub(crate) fn configured_http_runtime(args: &[String]) -> Result<HttpRuntimeOptions> {
    let transport = cli_option_value(args, "--transport").unwrap_or_else(|| "stdio".to_owned());
    let port = cli_option_value(args, "--port")
        .and_then(|s| s.parse().ok())
        .unwrap_or(7837);
    let listen = configured_listen(args)?;
    let auth = configured_http_auth(args)?;
    let tls = configured_tls(args, &transport)?;
    if matches!(transport.as_str(), "http" | "https") {
        validate_remote_transport_config(&transport, listen, &auth, &tls)?;
    }
    Ok(HttpRuntimeOptions {
        transport,
        listen,
        port,
        auth,
        tls,
    })
}

fn cli_or_env(args: &[String], flag: &str, env_name: &str) -> Option<String> {
    cli_option_value(args, flag).or_else(|| dual_prefix_env(env_name))
}

fn configured_listen(args: &[String]) -> Result<IpAddr> {
    cli_or_env(args, "--listen", "CODELENS_LISTEN")
        .unwrap_or_else(|| "127.0.0.1".to_owned())
        .parse()
        .map_err(|error| anyhow::anyhow!("invalid --listen address: {error}"))
}

fn configured_http_auth(args: &[String]) -> Result<HttpAuthConfig> {
    let auth_mode = cli_or_env(args, "--auth", "CODELENS_AUTH").unwrap_or_else(|| "off".to_owned());
    match auth_mode.as_str() {
        "off" | "none" | "disabled" => Ok(HttpAuthConfig::Off),
        "jwks" => {
            let jwks_url = cli_or_env(args, "--auth-jwks-url", "CODELENS_AUTH_JWKS_URL")
                .ok_or_else(|| anyhow::anyhow!("--auth jwks requires --auth-jwks-url"))?;
            let issuer = cli_or_env(args, "--auth-issuer", "CODELENS_AUTH_ISSUER")
                .ok_or_else(|| anyhow::anyhow!("--auth jwks requires --auth-issuer"))?;
            let audience = cli_or_env(args, "--auth-audience", "CODELENS_AUTH_AUDIENCE")
                .ok_or_else(|| anyhow::anyhow!("--auth jwks requires --auth-audience"))?;
            let scope = cli_or_env(args, "--auth-scope", "CODELENS_AUTH_SCOPE");
            Ok(HttpAuthConfig::jwks(jwks_url, issuer, audience, scope))
        }
        other => anyhow::bail!("unsupported --auth mode `{other}`; expected off|jwks"),
    }
}

fn configured_tls(args: &[String], transport: &str) -> Result<Option<TlsConfig>> {
    let cert = cli_or_env(args, "--tls-cert", "CODELENS_TLS_CERT");
    let key = cli_or_env(args, "--tls-key", "CODELENS_TLS_KEY");
    match (transport, cert, key) {
        ("https", Some(cert_path), Some(key_path)) => Ok(Some(TlsConfig {
            cert_path: PathBuf::from(cert_path),
            key_path: PathBuf::from(key_path),
        })),
        ("https", _, _) => {
            anyhow::bail!("--transport https requires --tls-cert and --tls-key")
        }
        (_, Some(_), None) | (_, None, Some(_)) => {
            anyhow::bail!("--tls-cert and --tls-key must be provided together")
        }
        _ => Ok(None),
    }
}

fn validate_remote_transport_config(
    transport: &str,
    listen: IpAddr,
    auth: &HttpAuthConfig,
    tls: &Option<TlsConfig>,
) -> Result<()> {
    if listen.is_loopback() {
        return Ok(());
    }
    if transport != "https" || tls.is_none() || !auth.enabled() {
        anyhow::bail!(
            "non-loopback HTTP serving requires --transport https, --tls-cert/--tls-key, and --auth jwks"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_http_auth_requires_jwks_parameters() {
        let args = vec![
            "codelens-mcp".to_owned(),
            "--auth".to_owned(),
            "jwks".to_owned(),
        ];
        let error = configured_http_auth(&args).expect_err("incomplete jwks config must fail");
        assert!(error.to_string().contains("--auth-jwks-url"));
    }

    #[test]
    fn non_loopback_listener_requires_https_tls_and_jwks_auth() {
        let listen: IpAddr = "0.0.0.0".parse().unwrap();
        assert!(
            validate_remote_transport_config("http", listen, &HttpAuthConfig::Off, &None).is_err()
        );
        let auth = HttpAuthConfig::jwks(
            "https://auth.example.com/jwks.json".to_owned(),
            "https://auth.example.com".to_owned(),
            "https://codelens.example.com/mcp".to_owned(),
            Some("codelens:tools".to_owned()),
        );
        let tls = Some(TlsConfig {
            cert_path: PathBuf::from("/tmp/cert.pem"),
            key_path: PathBuf::from("/tmp/key.pem"),
        });
        assert!(validate_remote_transport_config("https", listen, &auth, &tls).is_ok());
    }
}
