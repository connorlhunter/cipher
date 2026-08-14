#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let config = cipher_server::config::ServerConfig::from_env().unwrap_or_else(|error| {
        eprintln!("Invalid Cipher server configuration: {error}");
        std::process::exit(2);
    });

    tracing::info!("Cipher server configured");
    cipher_server::run(config.bind).await
}
