use std::net::SocketAddr;

const DEFAULT_LOCAL_BIND: &str = "127.0.0.1:3000";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServerConfig {
    pub bind: SocketAddr,
}

impl ServerConfig {
    pub fn from_env() -> Result<Self, String> {
        Self::parse(std::env::var("CIPHER_SERVER_BIND").ok().as_deref())
    }

    fn parse(bind: Option<&str>) -> Result<Self, String> {
        let bind = bind
            .unwrap_or(DEFAULT_LOCAL_BIND)
            .parse::<SocketAddr>()
            .map_err(|error| format!("CIPHER_SERVER_BIND must be a socket address: {error}"))?;

        Ok(Self { bind })
    }
}

#[cfg(test)]
mod tests {
    use super::ServerConfig;

    #[test]
    fn local_defaults_are_safe() {
        let config = ServerConfig::parse(None).unwrap();

        assert!(config.bind.ip().is_loopback());
    }

    #[test]
    fn rejects_invalid_bind() {
        let result = ServerConfig::parse(Some("not-a-socket"));

        assert!(result.is_err());
    }

    #[test]
    fn accepts_service_bind() {
        let config = ServerConfig::parse(Some("0.0.0.0:3000")).unwrap();

        assert!(!config.bind.ip().is_loopback());
    }
}
