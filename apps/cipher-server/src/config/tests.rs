use std::collections::HashMap;

use super::{REQUIRED_KEYS, ServerConfig};

fn valid_values() -> HashMap<&'static str, String> {
    let mut values = include_str!("../../../../.env.example")
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
    let example = include_str!("../../../../.env.example");

    for key in REQUIRED_KEYS {
        assert_eq!(example.matches(&format!("{key}=")).count(), 1, "{key}");
    }

    for forbidden in ["PASSWORD=", "SECRET=", "TOKEN=", "PRIVATE_KEY="] {
        assert!(!example.contains(forbidden));
    }

    assert!(parse(&valid_values()).is_ok());
}
