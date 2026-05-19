mod cli;
mod commands;
mod output;

use clap::Parser;

#[tokio::main]
async fn main() {
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
