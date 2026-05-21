mod approve_commands;
mod cli;
mod commands;
mod config_store;
mod ctf_commands;
mod data_commands;
mod gamma_commands;
mod network_config;
mod order_commands;
mod output;
mod safe_exec;
mod setup_commands;
mod shell_commands;
mod wallet_commands;
mod ws_commands;

use clap::Parser;

#[tokio::main]
async fn main() {
    // Pulled in transitively by both `tokio-tungstenite` (rustls 0.21 via 0.23) and
    // `alloy`'s `providers`/`contract` features (rustls 0.23). Rustls 0.23 refuses to start
    // without an explicit default crypto provider when more than one TLS dep is in the
    // graph. Install the `ring` provider at process start; ignore the "already installed"
    // error so re-running in tests is idempotent.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let args = cli::Cli::parse();
    let format = args.output;

    if let Err(e) = commands::run(args).await {
        match format {
            output::Format::Json => {
                let _ = serde_json::to_writer(
                    std::io::stderr(),
                    &serde_json::json!({ "error": e.to_string() }),
                );
                eprintln!();
            }
            output::Format::Table => {
                eprintln!("Error: {e}");
            }
        }
        std::process::exit(1);
    }
}
