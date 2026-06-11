// =============================================================
// DEPLOY.RS AND MAIN.RS — COMPLETE REFERENCE IMPLEMENTATION
// =============================================================
// USE THIS ONLY IF YOU ARE COMPLETELY STUCK.
// Attempt both files yourself first.
// =============================================================

// =============================================================
// deploy.rs
// =============================================================
// PASTE EVERYTHING BELOW THIS LINE INTO src/deploy.rs
// -------------------------------------------------------------

use std::time::Duration;
use anyhow::{Context, Result};
use tracing::info;
use crate::client::PlcClient;
use crate::config::Config;

/// Runs the full deploy sequence against a live OpenPLC runtime.
///
/// Steps:
///   1. Create user account (safe to call repeatedly — 409 is ok)
///   2. Authenticate and get JWT token
///   3. Read program.zip from disk
///   4. Upload zip to OpenPLC
///   5. Poll until compilation succeeds
///   6. Start PLC execution
///
/// If any step fails, the error is returned immediately with
/// context describing which step failed.
pub async fn deploy(client: &mut PlcClient, config: &Config) -> Result<()> {
    info!("Starting deploy sequence");

    // Step 1: Ensure the user account exists.
    // Safe to call on every startup — 409 (user exists) is treated
    // as success in create_user(), so this is idempotent.
    client
        .create_user()
        .await
        .context("user setup failed")?;

    // Step 2: Authenticate and store the JWT token.
    // After this call, client.jwt_token is Some(token) and
    // auth_header() will work for all subsequent calls.
    client
        .login()
        .await
        .context("authentication failed")?;

    // Step 3: Read the program zip from disk.
    // tokio::fs::read is the async version of std::fs::read.
    // It yields to the Tokio scheduler while waiting for disk I/O
    // instead of blocking the thread.
    // with_context takes a closure so format!() only runs on error.
    let zip_bytes = tokio::fs::read(&config.program_zip_path)
        .await
        .with_context(|| format!(
            "failed to read program zip at '{}'",
            config.program_zip_path
        ))?;

    info!("Loaded program zip ({} bytes)", zip_bytes.len());

    // Step 4: Upload the zip to OpenPLC.
    // OpenPLC validates the zip and immediately begins compilation
    // in a background thread. This returns as soon as the upload
    // is accepted — not when compilation finishes.
    client
        .upload_program(zip_bytes)
        .await
        .context("program upload failed")?;

    // Step 5: Poll compilation status until SUCCESS or FAILED.
    // poll_interval: how often to ask "are you done yet?"
    // timeout: give up and return an error after this long.
    client
        .wait_for_compilation(
            Duration::from_secs(2),    // poll every 2 seconds
            Duration::from_secs(120),  // give up after 2 minutes
        )
        .await
        .context("compilation failed")?;

    // Step 6: Start the PLC execution.
    // After this call the PLC scan cycle begins and the
    // Modbus slave becomes active on port 502.
    client
        .start_plc()
        .await
        .context("failed to start PLC")?;

    info!("Deploy complete — PLC is running");
    Ok(())
}


// =============================================================
// main.rs
// =============================================================
// PASTE EVERYTHING BELOW THIS LINE INTO src/main.rs
// (replace the entire file)
// -------------------------------------------------------------

mod config;
mod client;
mod deploy;

use anyhow::Result;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

// #[tokio::main] is a macro that:
//   1. Creates a Tokio runtime with a thread pool
//   2. Runs this async function on it
//   3. Blocks until the function returns
//   4. Shuts down the runtime
//
// Without it, async fn main() would not compile — Rust needs
// a runtime to drive async code and does not provide one built-in.
//
// -> Result<()> means if main returns Err, Rust prints the error
// and exits with a non-zero code. Docker treats non-zero exit
// codes as container failure.
#[tokio::main]
async fn main() -> Result<()> {
    // Step 1: Initialize logging.
    // This must be first — before any other code — so nothing is missed.
    //
    // EnvFilter::from_default_env() reads the RUST_LOG environment
    // variable. Set RUST_LOG=debug to see everything, RUST_LOG=info
    // for normal operation.
    //
    // add_directive sets your crate's default to info level even if
    // RUST_LOG is not set. "plc_node" matches the name in Cargo.toml
    // with hyphens replaced by underscores.
    //
    // .init() installs this as the global logging handler.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("plc_node=info".parse()?)
        )
        .init();

    // Step 2: Read all configuration from environment variables.
    // Panics immediately with a clear message if PLC_PASSWORD is missing.
    // All other values have sensible defaults.
    let config = config::Config::from_env();

    // Step 3: Log startup so you can identify this node in Docker logs.
    info!("PLC node starting, node_id={}", config.node_id);

    // Step 4: Wait for OpenPLC to initialize.
    // OpenPLC needs time to start Flask, generate TLS certs on first
    // run, and start the C++ runtime. Connecting too early gets
    // "connection refused".
    // Set PLC_STARTUP_DELAY_SECS=0 when OpenPLC is already running
    // (e.g. Stage 1 testing against a separate Docker container).
    info!(
        "Waiting {}s for OpenPLC to initialize...",
        config.plc_startup_delay_secs
    );
    tokio::time::sleep(
        std::time::Duration::from_secs(config.plc_startup_delay_secs)
    ).await;

    // Step 5: Build the HTTP client.
    // mut because deploy() calls login() which needs &mut self.
    // ? propagates if ClientBuilder::build() fails.
    let mut client = client::PlcClient::new(
        &config.plc_url,
        &config.plc_username,
        &config.plc_password,
    )?;

    // Step 6: Run the deploy sequence.
    // On success: log and fall through to Ok(()).
    // On failure: log the full error chain and exit with code 1.
    //
    // {:#} prints the full error chain with all context messages:
    //   "deploy failed: compilation failed: timed out after 120s"
    // Much more useful than just the innermost error message.
    //
    // std::process::exit(1) tells Docker the container failed.
    // Docker can then restart it or report it as unhealthy.
    match deploy::deploy(&mut client, &config).await {
        Ok(()) => {
            info!("Deploy successful — PLC is running");
        }
        Err(e) => {
            error!("Deploy failed: {:#}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}