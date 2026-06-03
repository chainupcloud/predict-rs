//! `predict-cli shell` — interactive REPL.
//!
//! Each line is parsed as a fresh `Cli` invocation via `clap::Parser::try_parse_from`.
//! Global state (wallet, tenant, credentials) is read from env vars / config file on every
//! command, the same as a normal CLI invocation — there is no "shell session" stickiness
//! beyond what env vars provide.
//!
//! Pattern lifted from the upstream CLI's `shell.rs`; the structure (banner, rustyline loop,
//! reject `shell`-in-`shell`, `Box::pin` to break async recursion) is intentional.

use clap::Parser as _;

use crate::cli::Cli;
use crate::output::Format;

pub async fn run() -> anyhow::Result<()> {
    println!();
    println!("  predict-cli · Interactive Shell");
    println!("  Type 'help' for commands, 'exit' or Ctrl-D to quit.");
    println!();

    let mut rl = rustyline::DefaultEditor::new()?;
    let history_path = history_path();
    if let Some(ref p) = history_path {
        let _ = rl.load_history(p);
    }

    loop {
        match rl.readline("predict-cli> ") {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if line == "exit" || line == "quit" {
                    break;
                }
                if line == "help" {
                    print_help();
                    continue;
                }
                let _ = rl.add_history_entry(line);

                let args = split_args(line);
                let mut full_args = vec!["predict-cli".to_string()];
                full_args.extend(args);

                if let Some(cmd) = full_args.get(1)
                    && cmd == "shell"
                {
                    println!("Already in shell mode.");
                    continue;
                }

                match Cli::try_parse_from(&full_args) {
                    Ok(cli) => {
                        let format = cli.output;
                        // Box::pin breaks the type-level cycle: shell -> commands::run ->
                        // (potential) shell. The runtime guard above already prevents the
                        // call from actually happening; this is just to keep the compiler
                        // happy with the recursive `impl Future` size.
                        if let Err(e) = Box::pin(crate::commands::run(cli)).await {
                            match format {
                                Format::Json => {
                                    let _ = serde_json::to_writer(
                                        std::io::stderr(),
                                        &serde_json::json!({ "error": e.to_string() }),
                                    );
                                    eprintln!();
                                }
                                Format::Table => eprintln!("Error: {e}"),
                            }
                        }
                    }
                    Err(e) => {
                        let _ = e.print();
                    }
                }
            }
            Err(rustyline::error::ReadlineError::Interrupted) => continue,
            Err(rustyline::error::ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("readline error: {e}");
                break;
            }
        }
    }

    if let Some(ref p) = history_path {
        let _ = rl.save_history(p);
    }
    println!("Goodbye!");
    Ok(())
}

fn print_help() {
    println!("predict-cli shell — type any `predict-cli` sub-command without the `predict-cli ` prefix:");
    println!("  ok                                 server health");
    println!("  time                               server time");
    println!("  book <TOKEN>                       order book snapshot");
    println!("  midpoint <TOKEN>                   midpoint price");
    println!("  gamma events list --limit 5        discover markets");
    println!("  balance --asset-type collateral    your USDW balance");
    println!("  order list                         your open orders");
    println!("  wallet show                        wallet identity");
    println!();
    println!("Built-ins: `help`, `exit` / `quit`, Ctrl-D. `shell` itself is blocked.");
    println!("Env vars / `predict-cli wallet`-stored config apply on every line; flags supplied here override.");
}

fn history_path() -> Option<std::path::PathBuf> {
    let dir = dirs::config_dir()?.join("pm");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("history"))
}

/// Tokenize a shell-style line: split on whitespace, honor double quotes for spaces.
/// Adapted from the upstream CLI's shell.rs::split_args.
fn split_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for c in input.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            ' ' if !in_quotes => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::split_args;

    #[test]
    fn split_args_handles_quoted_segments() {
        assert_eq!(
            split_args(r#"gamma events get "how-many-fed-rate-cuts-in-2026-pm-406282""#),
            vec![
                "gamma".to_string(),
                "events".to_string(),
                "get".to_string(),
                "how-many-fed-rate-cuts-in-2026-pm-406282".to_string()
            ]
        );
    }

    #[test]
    fn split_args_collapses_whitespace_outside_quotes() {
        assert_eq!(
            split_args("  order   list  "),
            vec!["order".to_string(), "list".to_string()]
        );
    }

    #[test]
    fn split_args_preserves_spaces_inside_quotes() {
        assert_eq!(
            split_args(r#"echo "hello world""#),
            vec!["echo".to_string(), "hello world".to_string()]
        );
    }
}
