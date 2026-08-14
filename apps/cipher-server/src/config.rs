//! Loading and validation for the server's runtime configuration.

use std::{collections::HashSet, fmt, net::SocketAddr};

/// Environment variables required to start the server.
pub const REQUIRED_KEYS: [&str; 12] = [
    "CIPHER_SERVER_BIND",
    "CIPHER_AWS_REGION",
    "CIPHER_AWS_ACCOUNT_ID",
    "CIPHER_API_ORIGIN",
    "CIPHER_REALTIME_URL",
    "CIPHER_COGNITO_USER_POOL_ID",
    "CIPHER_COGNITO_CLIENT_ID",
    "CIPHER_USERS_TABLE",
    "CIPHER_CONVERSATIONS_TABLE",
    "CIPHER_MESSAGES_TABLE",
    "CIPHER_MEDIA_TABLE",
    "CIPHER_MEDIA_BUCKET",
];

/// Validated settings required to start the Cipher server.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerConfig {
    /// Address on which the HTTP server listens.
    pub bind: SocketAddr,
    /// AWS service and resource settings.
    pub aws: AwsConfig,
    /// Public HTTP and WebSocket endpoints.
    pub endpoints: PublicEndpoints,
}

/// Validated settings for the production AWS deployment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AwsConfig {
    /// AWS region containing the deployment.
    pub region: String,
    /// AWS account that owns the deployment.
    pub account_id: String,
    /// Cognito user pool identifier.
    pub cognito_user_pool_id: String,
    /// Cognito public client identifier.
    pub cognito_client_id: String,
    /// DynamoDB users table name.
    pub users_table: String,
    /// DynamoDB conversations table name.
    pub conversations_table: String,
    /// DynamoDB messages table name.
    pub messages_table: String,
    /// DynamoDB media table name.
    pub media_table: String,
    /// S3 media bucket name.
    pub media_bucket: String,
}

/// Public endpoints advertised to Cipher clients.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicEndpoints {
    /// HTTPS origin for API requests.
    pub api_origin: String,
    /// Secure WebSocket endpoint for realtime traffic.
    pub realtime_url: String,
}

/// Describes an invalid server setting without retaining its value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigError {
    key: &'static str,
    reason: &'static str,
}

impl ConfigError {
    fn new(key: &'static str, reason: &'static str) -> Self {
        Self { key, reason }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.key, self.reason)
    }
}

impl std::error::Error for ConfigError {}

impl ServerConfig {
    /// Loads and validates server settings from environment variables.
    ///
    /// Returns an error naming the first missing or invalid setting.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    fn from_lookup(mut lookup: impl FnMut(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let bind = required(&mut lookup, "CIPHER_SERVER_BIND")?
            .parse::<SocketAddr>()
            .map_err(|_| ConfigError::new("CIPHER_SERVER_BIND", "must be a socket address"))?;
        if bind.port() == 0 {
            return Err(ConfigError::new(
                "CIPHER_SERVER_BIND",
                "must use a nonzero port",
            ));
        }

        let region = required(&mut lookup, "CIPHER_AWS_REGION")?;
        if !valid_aws_region(&region) {
            return Err(ConfigError::new(
                "CIPHER_AWS_REGION",
                "must be an AWS region name",
            ));
        }

        let account_id = required(&mut lookup, "CIPHER_AWS_ACCOUNT_ID")?;
        if account_id.len() != 12
            || !account_id.bytes().all(|byte| byte.is_ascii_digit())
            || account_id == "000000000000"
        {
            return Err(ConfigError::new(
                "CIPHER_AWS_ACCOUNT_ID",
                "must contain exactly 12 digits",
            ));
        }

        let api_origin = required(&mut lookup, "CIPHER_API_ORIGIN")?;
        let realtime_url = required(&mut lookup, "CIPHER_REALTIME_URL")?;
        let api_host = https_host(&api_origin)
            .ok_or_else(|| ConfigError::new("CIPHER_API_ORIGIN", "must be an HTTPS origin"))?;
        let realtime_host = wss_host(&realtime_url).ok_or_else(|| {
            ConfigError::new("CIPHER_REALTIME_URL", "must be a secure WebSocket URL")
        })?;
        if api_host != realtime_host {
            return Err(ConfigError::new(
                "CIPHER_REALTIME_URL",
                "must use the API origin host",
            ));
        }

        let cognito_user_pool_id = required(&mut lookup, "CIPHER_COGNITO_USER_POOL_ID")?;
        let pool_prefix = format!("{region}_");
        if !cognito_user_pool_id.starts_with(&pool_prefix)
            || cognito_user_pool_id.contains("EXAMPLE")
            || !cognito_user_pool_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        {
            return Err(ConfigError::new(
                "CIPHER_COGNITO_USER_POOL_ID",
                "must be a user pool in the production region",
            ));
        }

        let cognito_client_id = required(&mut lookup, "CIPHER_COGNITO_CLIENT_ID")?;
        if cognito_client_id.len() > 128
            || !cognito_client_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err(ConfigError::new(
                "CIPHER_COGNITO_CLIENT_ID",
                "must be an alphanumeric public client id",
            ));
        }

        let users_table = table_name(&mut lookup, "CIPHER_USERS_TABLE")?;
        let conversations_table = table_name(&mut lookup, "CIPHER_CONVERSATIONS_TABLE")?;
        let messages_table = table_name(&mut lookup, "CIPHER_MESSAGES_TABLE")?;
        let media_table = table_name(&mut lookup, "CIPHER_MEDIA_TABLE")?;

        let tables = [
            users_table.as_str(),
            conversations_table.as_str(),
            messages_table.as_str(),
            media_table.as_str(),
        ];
        if tables.iter().copied().collect::<HashSet<_>>().len() != tables.len() {
            return Err(ConfigError::new(
                "CIPHER_*_TABLE",
                "values must name distinct tables",
            ));
        }

        let media_bucket = required(&mut lookup, "CIPHER_MEDIA_BUCKET")?;
        if !valid_bucket_name(&media_bucket) || media_bucket.contains("example") {
            return Err(ConfigError::new(
                "CIPHER_MEDIA_BUCKET",
                "must be an S3 bucket name",
            ));
        }

        Ok(Self {
            bind,
            aws: AwsConfig {
                region,
                account_id,
                cognito_user_pool_id,
                cognito_client_id,
                users_table,
                conversations_table,
                messages_table,
                media_table,
                media_bucket,
            },
            endpoints: PublicEndpoints {
                api_origin,
                realtime_url,
            },
        })
    }
}

fn required(
    lookup: &mut impl FnMut(&str) -> Option<String>,
    key: &'static str,
) -> Result<String, ConfigError> {
    let value = lookup(key).ok_or_else(|| ConfigError::new(key, "is required"))?;
    if value.is_empty() {
        return Err(ConfigError::new(key, "cannot be empty"));
    }
    if value.trim() != value {
        return Err(ConfigError::new(
            key,
            "cannot contain surrounding whitespace",
        ));
    }
    Ok(value)
}

fn table_name(
    lookup: &mut impl FnMut(&str) -> Option<String>,
    key: &'static str,
) -> Result<String, ConfigError> {
    let value = required(lookup, key)?;
    if !(3..=255).contains(&value.len())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(ConfigError::new(key, "must be a DynamoDB table name"));
    }
    Ok(value)
}

fn valid_aws_region(value: &str) -> bool {
    let parts = value.split('-').collect::<Vec<_>>();
    let Some(number) = parts.last() else {
        return false;
    };
    (parts.len() == 3 || (parts.len() == 4 && parts[1] == "gov"))
        && parts.iter().all(|part| !part.is_empty())
        && number.parse::<u8>().is_ok()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn https_host(value: &str) -> Option<&str> {
    endpoint_host(value, "https://", false)
}

fn wss_host(value: &str) -> Option<&str> {
    endpoint_host(value, "wss://", true)
}

fn endpoint_host<'a>(value: &'a str, scheme: &str, require_path: bool) -> Option<&'a str> {
    let remainder = value.strip_prefix(scheme)?;
    if remainder.is_empty()
        || remainder.contains(['?', '#', '@'])
        || remainder.bytes().any(|byte| byte.is_ascii_whitespace())
    {
        return None;
    }
    let (host, path) = remainder.split_once('/').unwrap_or((remainder, ""));
    if host.is_empty()
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':'))
        || (require_path && path.is_empty())
        || (!require_path && !path.is_empty())
    {
        return None;
    }
    Some(host)
}

/// Validates S3 bucket syntax, including the IPv4-address exclusion.
fn valid_bucket_name(value: &str) -> bool {
    if !(3..=63).contains(&value.len())
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'.')
        })
        || !value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        || !value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        || value.contains("..")
        || value.contains(".-")
        || value.contains("-.")
    {
        return false;
    }

    let labels = value.split('.').collect::<Vec<_>>();
    !(labels.len() == 4
        && labels.iter().all(|label| {
            label.len() <= 3
                && !label.is_empty()
                && label.bytes().all(|byte| byte.is_ascii_digit())
                && label.parse::<u8>().is_ok()
        }))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{REQUIRED_KEYS, ServerConfig};

    fn valid_values() -> HashMap<&'static str, String> {
        let mut values = include_str!("../../../.env.example")
            .lines()
            .filter_map(|line| line.split_once('='))
            .filter(|(key, _)| key.starts_with("CIPHER_"))
            .map(|(key, value)| (key, value.to_owned()))
            .collect::<HashMap<_, _>>();
        let region = values["CIPHER_AWS_REGION"].clone();
        values.extend([
            ("CIPHER_SERVER_BIND", "127.0.0.1:3000".into()),
            ("CIPHER_AWS_ACCOUNT_ID", "123456789012".into()),
            ("CIPHER_COGNITO_USER_POOL_ID", format!("{region}_a1B2c3D4e")),
            ("CIPHER_COGNITO_CLIENT_ID", "1a2b3c4d5e6f7g8h9i".into()),
            ("CIPHER_USERS_TABLE", "cipher-users".into()),
            ("CIPHER_CONVERSATIONS_TABLE", "cipher-conversations".into()),
            ("CIPHER_MESSAGES_TABLE", "cipher-messages".into()),
            ("CIPHER_MEDIA_TABLE", "cipher-media".into()),
            ("CIPHER_MEDIA_BUCKET", "cipher-123456789012-media".into()),
        ]);
        values
    }

    fn parse(values: &HashMap<&'static str, String>) -> Result<ServerConfig, String> {
        ServerConfig::from_lookup(|key| values.get(key).cloned()).map_err(|error| error.to_string())
    }

    #[test]
    fn parses_complete_production_configuration() {
        let config = parse(&valid_values()).unwrap();

        assert!(config.bind.ip().is_loopback());
        assert_eq!(
            config.endpoints.api_origin,
            valid_values()["CIPHER_API_ORIGIN"]
        );
        assert_eq!(config.aws.region, valid_values()["CIPHER_AWS_REGION"]);
    }

    #[test]
    fn requires_every_setting() {
        for key in REQUIRED_KEYS {
            let mut values = valid_values();
            values.remove(key);

            assert_eq!(parse(&values).unwrap_err(), format!("{key} is required"));
        }
    }

    #[test]
    fn rejects_blank_or_padded_values() {
        let region = valid_values()["CIPHER_AWS_REGION"].clone();
        for value in [String::new(), format!(" {region}"), format!("{region} ")] {
            let mut values = valid_values();
            values.insert("CIPHER_AWS_REGION", value);

            assert!(parse(&values).is_err());
        }
    }

    #[test]
    fn rejects_invalid_bind() {
        for value in ["not-a-socket", "127.0.0.1:0"] {
            let mut values = valid_values();
            values.insert("CIPHER_SERVER_BIND", value.into());

            assert!(parse(&values).is_err());
        }
    }

    #[test]
    fn rejects_the_wrong_region_or_endpoints() {
        for (key, value) in [
            ("CIPHER_AWS_REGION", "not a region".to_owned()),
            ("CIPHER_API_ORIGIN", "not-an-origin".to_owned()),
            ("CIPHER_REALTIME_URL", "not-a-websocket-url".to_owned()),
        ] {
            let mut values = valid_values();
            values.insert(key, value);

            assert!(parse(&values).is_err());
        }
    }

    #[test]
    fn requires_matching_api_and_realtime_hosts() {
        let mut values = valid_values();
        let realtime = values["CIPHER_REALTIME_URL"].replacen("cipher.", "other.", 1);
        values.insert("CIPHER_REALTIME_URL", realtime);

        assert_eq!(
            parse(&values).unwrap_err(),
            "CIPHER_REALTIME_URL must use the API origin host"
        );
    }

    #[test]
    fn rejects_invalid_aws_identifiers() {
        for (key, value) in [
            ("CIPHER_AWS_ACCOUNT_ID", "123"),
            ("CIPHER_AWS_ACCOUNT_ID", "000000000000"),
            ("CIPHER_COGNITO_USER_POOL_ID", "wrong_wrong"),
            ("CIPHER_COGNITO_USER_POOL_ID", "invalid_EXAMPLE123"),
            ("CIPHER_COGNITO_CLIENT_ID", "not a client"),
            ("CIPHER_USERS_TABLE", "u"),
            ("CIPHER_MEDIA_BUCKET", "Cipher-media"),
            ("CIPHER_MEDIA_BUCKET", "cipher-example-media"),
        ] {
            let mut values = valid_values();
            values.insert(key, value.into());

            assert!(parse(&values).is_err());
        }
    }

    #[test]
    fn rejects_duplicate_table_names() {
        let mut values = valid_values();
        values.insert("CIPHER_MEDIA_TABLE", values["CIPHER_USERS_TABLE"].clone());

        assert_eq!(
            parse(&values).unwrap_err(),
            "CIPHER_*_TABLE values must name distinct tables"
        );
    }

    #[test]
    fn errors_do_not_echo_values() {
        let sentinel = "sentinel-value-that-must-not-be-logged";
        let mut values = valid_values();
        values.insert("CIPHER_COGNITO_CLIENT_ID", sentinel.into());

        assert!(!parse(&values).unwrap_err().contains(sentinel));
    }

    #[test]
    fn example_and_required_keys_stay_in_sync() {
        let example = include_str!("../../../.env.example");

        for key in REQUIRED_KEYS {
            assert_eq!(example.matches(&format!("{key}=")).count(), 1, "{key}");
        }

        for forbidden in ["PASSWORD=", "SECRET=", "TOKEN=", "PRIVATE_KEY="] {
            assert!(!example.contains(forbidden));
        }

        assert!(parse(&valid_values()).is_ok());
    }
}
