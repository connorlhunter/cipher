//! Verifies that invalid startup configuration stops the server safely.

use std::process::Command;

use cipher_server::config::REQUIRED_KEYS;

#[test]
fn exits_before_starting_when_production_configuration_is_missing() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cipher-server"));
    for key in REQUIRED_KEYS {
        command.env_remove(key);
    }

    let output = command.output().expect("server process should start");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");

    assert_eq!(output.status.code(), Some(2));
    assert!(stderr.contains("CIPHER_SERVER_BIND is required"));
    assert!(!stderr.contains("Cipher backend listening"));
}
