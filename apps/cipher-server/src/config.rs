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

const PRODUCTION_REGION: &str = "us-east-1";
const PRODUCTION_API_ORIGIN: &str = "https://cipher.connorhunter.me";
const PRODUCTION_REALTIME_URL: &str = "wss://cipher.connorhunter.me/v1/realtime";

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
        if region != PRODUCTION_REGION {
            return Err(ConfigError::new("CIPHER_AWS_REGION", "must be us-east-1"));
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
        if api_origin != PRODUCTION_API_ORIGIN {
            return Err(ConfigError::new(
                "CIPHER_API_ORIGIN",
                "must be https://cipher.connorhunter.me",
            ));
        }
        if realtime_url != PRODUCTION_REALTIME_URL {
            return Err(ConfigError::new(
                "CIPHER_REALTIME_URL",
                "must be wss://cipher.connorhunter.me/v1/realtime",
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
#[path = "config/tests.rs"]
mod tests;
