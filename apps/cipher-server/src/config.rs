//! Loading and validation for the server's runtime configuration.

use std::{collections::HashSet, fmt, net::SocketAddr};

use serde::Deserialize;

const PRODUCTION_CONFIG_JSON: &str = include_str!("../../../config/production.json");

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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProductionBoundary {
    aws_region: String,
    api_origin: String,
    realtime_url: String,
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
        let production = production_boundary()?;
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
        exact(
            "CIPHER_AWS_REGION",
            &region,
            &production.aws_region,
            "must name the production AWS region",
        )?;

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
        exact(
            "CIPHER_API_ORIGIN",
            &api_origin,
            &production.api_origin,
            "must use the production HTTPS origin",
        )?;

        let realtime_url = required(&mut lookup, "CIPHER_REALTIME_URL")?;
        exact(
            "CIPHER_REALTIME_URL",
            &realtime_url,
            &production.realtime_url,
            "must use the production WebSocket URL",
        )?;

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
        if !valid_bucket_name(&media_bucket)
            || !media_bucket.starts_with("cipher-production-")
            || media_bucket.contains("example")
        {
            return Err(ConfigError::new(
                "CIPHER_MEDIA_BUCKET",
                "must be a production Cipher S3 bucket name",
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

fn production_boundary() -> Result<ProductionBoundary, ConfigError> {
    serde_json::from_str(PRODUCTION_CONFIG_JSON).map_err(|_| {
        ConfigError::new(
            "config/production.json",
            "must contain the production boundary",
        )
    })
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

fn exact(
    key: &'static str,
    actual: &str,
    expected: &str,
    reason: &'static str,
) -> Result<(), ConfigError> {
    if actual != expected {
        return Err(ConfigError::new(key, reason));
    }
    Ok(())
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
        || !value.starts_with("cipher-production-")
    {
        return Err(ConfigError::new(
            key,
            "must be a production Cipher DynamoDB table name",
        ));
    }
    Ok(value)
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

    use super::{REQUIRED_KEYS, ServerConfig, production_boundary};

    fn valid_values() -> HashMap<&'static str, String> {
        let production = production_boundary().unwrap();
        HashMap::from([
            ("CIPHER_SERVER_BIND", "127.0.0.1:3000".into()),
            ("CIPHER_AWS_REGION", production.aws_region),
            ("CIPHER_AWS_ACCOUNT_ID", "123456789012".into()),
            ("CIPHER_API_ORIGIN", production.api_origin),
            ("CIPHER_REALTIME_URL", production.realtime_url),
            ("CIPHER_COGNITO_USER_POOL_ID", "us-east-1_a1B2c3D4e".into()),
            ("CIPHER_COGNITO_CLIENT_ID", "1a2b3c4d5e6f7g8h9i".into()),
            ("CIPHER_USERS_TABLE", "cipher-production-users".into()),
            (
                "CIPHER_CONVERSATIONS_TABLE",
                "cipher-production-conversations".into(),
            ),
            ("CIPHER_MESSAGES_TABLE", "cipher-production-messages".into()),
            ("CIPHER_MEDIA_TABLE", "cipher-production-media".into()),
            (
                "CIPHER_MEDIA_BUCKET",
                "cipher-production-123456789012-us-east-1-media".into(),
            ),
        ])
    }

    fn parse(values: &HashMap<&'static str, String>) -> Result<ServerConfig, String> {
        ServerConfig::from_lookup(|key| values.get(key).cloned()).map_err(|error| error.to_string())
    }

    #[test]
    fn parses_complete_production_configuration() {
        let config = parse(&valid_values()).unwrap();
        let production = production_boundary().unwrap();

        assert!(config.bind.ip().is_loopback());
        assert_eq!(config.endpoints.api_origin, production.api_origin);
        assert_eq!(config.aws.region, production.aws_region);
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
        for value in ["", " us-east-1", "us-east-1 "] {
            let mut values = valid_values();
            values.insert("CIPHER_AWS_REGION", value.into());

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
        let production = production_boundary().unwrap();
        for (key, value) in [
            ("CIPHER_AWS_REGION", "us-east-2".to_owned()),
            (
                "CIPHER_API_ORIGIN",
                production.api_origin.replacen("https", "http", 1),
            ),
            (
                "CIPHER_REALTIME_URL",
                production.realtime_url.replace("/v1/realtime", "/realtime"),
            ),
        ] {
            let mut values = valid_values();
            values.insert(key, value);

            assert!(parse(&values).is_err());
        }
    }

    #[test]
    fn rejects_invalid_aws_identifiers() {
        for (key, value) in [
            ("CIPHER_AWS_ACCOUNT_ID", "123"),
            ("CIPHER_AWS_ACCOUNT_ID", "000000000000"),
            ("CIPHER_COGNITO_USER_POOL_ID", "us-east-2_wrong"),
            ("CIPHER_COGNITO_USER_POOL_ID", "us-east-1_EXAMPLE123"),
            ("CIPHER_COGNITO_CLIENT_ID", "not a client"),
            ("CIPHER_USERS_TABLE", "users"),
            ("CIPHER_MEDIA_BUCKET", "Cipher-production-media"),
            ("CIPHER_MEDIA_BUCKET", "cipher-production-example-media"),
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
        let production = production_boundary().unwrap();

        for key in REQUIRED_KEYS {
            assert_eq!(example.matches(&format!("{key}=")).count(), 1, "{key}");
        }

        for forbidden in ["PASSWORD=", "SECRET=", "TOKEN=", "PRIVATE_KEY="] {
            assert!(!example.contains(forbidden));
        }

        for (key, value) in [
            ("CIPHER_AWS_REGION", production.aws_region),
            ("CIPHER_API_ORIGIN", production.api_origin),
            ("CIPHER_REALTIME_URL", production.realtime_url),
        ] {
            assert!(example.contains(&format!("{key}={value}")), "{key}");
        }
    }
}
