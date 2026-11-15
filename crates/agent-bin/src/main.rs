#![forbid(unsafe_code)]

mod agent;
mod buffer;
mod checks;
mod config;
mod enroll;

use std::{path::PathBuf, process::ExitCode};

use config::{AgentConfig, default_config_path};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

const USAGE: &str = "pingushka-agent <run|enroll> [options]

run     --config <path>
enroll  --hub <url> --enrollment-token <token> [--config <path>] [--buffer <path>]";

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.first().map(String::as_str) {
        Some("run") => match run_command(&arguments[1..]).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        },
        Some("enroll") => match enroll_command(&arguments[1..]).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        },
        Some("--version" | "-V") => {
            println!("pingushka-agent {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help" | "-h") | None => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unknown command: {other}\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

async fn enroll_command(arguments: &[String]) -> Result<(), String> {
    let hub = required(arguments, "--hub")?;
    let token = required(arguments, "--enrollment-token")?;
    let config_path = optional(arguments, "--config")
        .map(PathBuf::from)
        .unwrap_or_else(default_config_path);
    let buffer_path = optional(arguments, "--buffer").map(PathBuf::from);
    let outcome = enroll::enroll(&hub, &token, &config_path, buffer_path.as_deref())
        .await
        .map_err(|error| error.to_string())?;
    println!("agent {} enrolled", outcome.agent_id);
    println!("configuration written to {}", config_path.display());
    Ok(())
}

async fn run_command(arguments: &[String]) -> Result<(), String> {
    let config_path = optional(arguments, "--config")
        .map(PathBuf::from)
        .unwrap_or_else(default_config_path);
    let config = AgentConfig::load(&config_path).map_err(|error| error.to_string())?;
    let buffer_path = config.buffer_path(&config_path);
    agent::run(config, &buffer_path).await;
    Ok(())
}

fn required(arguments: &[String], name: &str) -> Result<String, String> {
    optional(arguments, name).ok_or_else(|| format!("missing required option {name}"))
}

fn optional(arguments: &[String], name: &str) -> Option<String> {
    arguments
        .iter()
        .position(|argument| argument == name)
        .and_then(|position| arguments.get(position + 1))
        .cloned()
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(false))
        .init();
}
