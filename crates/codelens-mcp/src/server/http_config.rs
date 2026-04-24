#![cfg(feature = "http")]

use crate::cli::cli_option_value;
use crate::env_compat::dual_prefix_env;
use crate::runtime_types::RuntimeTransportMode;
use crate::server::http_auth::HttpAuthConfig;
use anyhow::{Context, Result, bail};
use std::net::IpAddr;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub(crate) struct TlsConfig {
    pub(crate) cert_path: PathBuf,
    pub(crate) key_path: PathBuf,
}

#[derive(Clone, Debug)]
pub(crate) struct HttpServerConfig {
    pub(crate) listen: IpAddr,
    pub(crate) port: u16,
    pub(crate) tls: Option<TlsConfig>,
}

#[derive(Clone, Debug)]
pub(crate) struct HttpRuntimeOptions {
    pub(crate) transport: RuntimeTransportMode,
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
    let transport = RuntimeTransportMode::from_str(
        &cli_or_env(args, "--transport", "CODELENS_TRANSPORT").unwrap_or_else(|| "stdio".into()),
    );
    if matches!(transport, RuntimeTransportMode::Stdio) {
        return Ok(HttpRuntimeOptions {
            transport,
            listen: "127.0.0.1".parse().expect("valid loopback"),
            port: 7837,
            auth: HttpAuthConfig::off(),
            tls: None,
        });
    }
    let listen = cli_or_env(args, "--listen", "CODELENS_LISTEN")
        .unwrap_or_else(|| "127.0.0.1".to_owned())
        .parse::<IpAddr>()
        .context("--listen must be an IP address")?;
    let port = cli_or_env(args, "--port", "CODELENS_PORT")
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(7837);
    let tls = configured_tls(args)?;
    let auth = configured_http_auth(args)?;

    validate_remote_transport_config(transport, listen, tls.as_ref(), &auth)?;

    Ok(HttpRuntimeOptions {
        transport,
        listen,
        port,
        auth,
        tls,
    })
}

fn cli_or_env(args: &[String], flag: &str, env_key: &str) -> Option<String> {
    cli_option_value(args, flag).or_else(|| dual_prefix_env(env_key))
}

fn configured_tls(args: &[String]) -> Result<Option<TlsConfig>> {
    let cert = cli_or_env(args, "--tls-cert", "CODELENS_TLS_CERT");
    let key = cli_or_env(args, "--tls-key", "CODELENS_TLS_KEY");
    match (cert, key) {
        (None, None) => Ok(None),
        (Some(cert_path), Some(key_path)) => Ok(Some(TlsConfig {
            cert_path: PathBuf::from(cert_path),
            key_path: PathBuf::from(key_path),
        })),
        _ => bail!("--tls-cert and --tls-key must be provided together"),
    }
}

fn configured_http_auth(args: &[String]) -> Result<HttpAuthConfig> {
    let mode = cli_or_env(args, "--auth", "CODELENS_AUTH").unwrap_or_else(|| "off".to_owned());
    match mode.as_str() {
        "off" => Ok(HttpAuthConfig::off()),
        "jwks" => {
            let jwks_url = required_arg(args, "--auth-jwks-url", "CODELENS_AUTH_JWKS_URL")?;
            let issuer = required_arg(args, "--auth-issuer", "CODELENS_AUTH_ISSUER")?;
            let audience = required_arg(args, "--auth-audience", "CODELENS_AUTH_AUDIENCE")?;
            let scope = cli_or_env(args, "--auth-scope", "CODELENS_AUTH_SCOPE");
            Ok(HttpAuthConfig::jwks(jwks_url, issuer, audience, scope))
        }
        other => bail!("unsupported --auth mode `{other}`; expected off or jwks"),
    }
}

fn required_arg(args: &[String], flag: &str, env_key: &str) -> Result<String> {
    cli_or_env(args, flag, env_key).with_context(|| format!("{flag} is required when --auth jwks"))
}

fn validate_remote_transport_config(
    transport: RuntimeTransportMode,
    listen: IpAddr,
    tls: Option<&TlsConfig>,
    auth: &HttpAuthConfig,
) -> Result<()> {
    if matches!(transport, RuntimeTransportMode::Https) && tls.is_none() {
        bail!("--transport https requires --tls-cert and --tls-key");
    }
    if !listen.is_loopback()
        && (!matches!(transport, RuntimeTransportMode::Https) || !auth.is_enabled())
    {
        bail!("non-loopback --listen requires --transport https and --auth jwks");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<String> {
        std::iter::once("codelens-mcp".to_owned())
            .chain(items.iter().map(|item| (*item).to_owned()))
            .collect()
    }

    #[test]
    fn loopback_http_allows_auth_off() {
        let runtime =
            configured_http_runtime(&argv(&["--transport", "http", "--listen", "127.0.0.1"]))
                .unwrap();
        assert_eq!(runtime.transport, RuntimeTransportMode::Http);
        assert!(!runtime.auth.is_enabled());
    }

    #[test]
    fn https_requires_tls_pair() {
        let error = configured_http_runtime(&argv(&["--transport", "https"]))
            .unwrap_err()
            .to_string();
        assert!(error.contains("--transport https requires"));
    }

    #[test]
    fn non_loopback_requires_https_and_jwks() {
        let error = configured_http_runtime(&argv(&["--transport", "http", "--listen", "0.0.0.0"]))
            .unwrap_err()
            .to_string();
        assert!(error.contains("non-loopback --listen requires"));
    }

    #[test]
    fn non_loopback_accepts_https_jwks() {
        let runtime = configured_http_runtime(&argv(&[
            "--transport",
            "https",
            "--listen",
            "0.0.0.0",
            "--tls-cert",
            "cert.pem",
            "--tls-key",
            "key.pem",
            "--auth",
            "jwks",
            "--auth-jwks-url",
            "https://issuer.example/jwks.json",
            "--auth-issuer",
            "https://issuer.example",
            "--auth-audience",
            "codelens",
        ]))
        .unwrap();
        assert_eq!(runtime.transport, RuntimeTransportMode::Https);
        assert!(runtime.auth.is_enabled());
    }
}
