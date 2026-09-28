mod auth;
mod logger;
mod paths;
mod server;
mod storage;
mod token;

use std::env;
use std::io::{self, Write};
use std::sync::Arc;
use tokio::net::TcpListener;

const VERSION: &str = "0.1.0";
const USAGE: &str = "Usage: copilot-openai-proxy [serve] [--shared] [--agent_whitelist=PATTERN]";

fn print_usage_and_exit() -> ! {
    eprintln!("{}", USAGE);
    std::process::exit(2);
}

fn prompt_line(prompt_text: &str) -> String {
    print!("{}", prompt_text);
    io::stdout().flush().ok();
    let mut buffer = String::new();
    if io::stdin().read_line(&mut buffer).is_err() {
        return String::new();
    }
    buffer.trim().to_string()
}

async fn ensure_authenticated(
    client: &reqwest::Client,
    storage: &storage::AuthStorage,
) -> Result<storage::CopilotOAuthData, String> {
    let existing = storage
        .get_copilot_credentials()
        .map_err(|e| e.to_string())?;

    if let Some(creds) = existing {
        if auth::is_copilot_token_valid(client, &creds).await {
            return Ok(creds);
        }
        println!("Existing GitHub Copilot token is expired or invalid. Re-authenticating...");
    } else {
        println!("No GitHub Copilot login found. Initiating login...");
    }

    let domain_input =
        prompt_line("GitHub Enterprise domain (leave empty for github.com) [github.com]: ");
    let domain = if domain_input.is_empty() {
        "github.com"
    } else {
        &domain_input
    };

    let new_creds = auth::login_device_flow(client, domain).await?;
    storage.save_copilot_credentials(&new_creds)?;
    println!("GitHub Copilot login saved.\n");
    Ok(new_creds)
}

#[tokio::main]
async fn main() {
    let mut shared = false;
    let mut agent_whitelist = "*".to_string();
    for arg in env::args().skip(1) {
        match arg.as_str() {
            "serve" => {}
            "--shared" => shared = true,
            "--help" | "-h" => {
                println!("{}", USAGE);
                return;
            }
            _ if arg.starts_with("--agent_whitelist=") => {
                let pattern = arg.trim_start_matches("--agent_whitelist=");
                if pattern.is_empty() {
                    print_usage_and_exit();
                }
                agent_whitelist = pattern.to_string();
            }
            _ => print_usage_and_exit(),
        }
    }
    let paths = match paths::resolve_proxy_paths() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error resolving paths: {}", e);
            std::process::exit(1);
        }
    };

    let token = match token::ensure_client_token(&paths.api_token) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Error ensuring client token: {}", e);
            std::process::exit(1);
        }
    };

    let storage = match storage::AuthStorage::open(&paths.auth_db) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error opening database: {}", e);
            std::process::exit(1);
        }
    };

    let client = reqwest::Client::builder()
        .build()
        .expect("Failed to build reqwest client");
    let creds = match ensure_authenticated(&client, &storage).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Authentication failed: {}", e);
            std::process::exit(1);
        }
    };

    let bind_addr = if shared {
        "0.0.0.0:4000"
    } else {
        "127.0.0.1:4000"
    };
    let display_host = if shared {
        "localhost:4000"
    } else {
        "127.0.0.1:4000"
    };
    let listener = match TcpListener::bind(bind_addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind to {}: {}", bind_addr, e);
            std::process::exit(1);
        }
    };

    let logger = logger::JsonLogger::init();

    let state = Arc::new(server::AppState {
        client,
        creds,
        client_token: token.clone(),
        version: VERSION,
        agent_whitelist,
        logger,
    });
    let app = server::create_router(state);

    println!("\n======================================================");
    println!(" GitHub Copilot OpenAI Proxy is READY");
    println!("======================================================\n");
    if shared {
        println!("Proxy Server:    http://0.0.0.0:4000 (all interfaces)");
        println!(
            "OpenAI Base URL: http://<your-ip-or-host>:4000/v1 (or http://{}/v1)",
            display_host
        );
    } else {
        println!("Proxy Server:    http://{}", bind_addr);
        println!("OpenAI Base URL: http://{}/v1", bind_addr);
    }
    println!("API Key:         {}\n", token);
    println!("------------------------------------------------------");
    println!("To use in your OpenAI client or editor:");
    if shared {
        println!("  Base URL: http://<your-ip-or-host>:4000/v1");
    } else {
        println!("  Base URL: http://{}/v1", bind_addr);
    }
    println!("  API Key:  {}", token);
    println!("------------------------------------------------------");
    println!("Keep this terminal window open while using the proxy.");
    println!("Press Ctrl+C to stop.\n");

    let server = axum::serve(listener, app);

    tokio::select! {
        res = server => {
            if let Err(e) = res {
                eprintln!("Server error: {}", e);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            println!("\nShutting down proxy...");
        }
    }
}
