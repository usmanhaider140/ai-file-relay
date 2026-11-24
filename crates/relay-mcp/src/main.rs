//! `relay-mcp` command line.
//!
//! ```text
//! relay-mcp                 run the MCP server over stdio (what Claude Desktop starts)
//! relay-mcp serve           run the hub: MCP over HTTP + the file server
//! relay-mcp init            create the config file
//! relay-mcp allow <folder>  let the AI use a folder
//! relay-mcp disallow <dir>  remove a folder
//! relay-mcp config          show where the config is and what it says
//! relay-mcp install-native-host    let the Chrome extension start relay-mcp
//! relay-mcp uninstall-native-host  undo that
//! ```
//!
//! When Chrome starts `relay-mcp` (with a `chrome-extension://…` argument),
//! it runs as the native messaging host instead.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use relay_core::Config;
use relay_mcp::proxy::{HubProxy, probe_hub};
use relay_mcp::{ExtensionBridge, Relay, RelayServer, http, install, native_host};
use rmcp::ServiceExt;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "relay-mcp",
    version,
    about = "AI File Relay: MCP server and local hub"
)]
struct Cli {
    /// Use this config file instead of the default one.
    #[arg(long, global = true, env = "AI_FILE_RELAY_CONFIG")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the MCP server over stdio (the default).
    Stdio,
    /// Run the hub: MCP over HTTP and the file server, on 127.0.0.1.
    Serve,
    /// Create the config file if it does not exist, and print its path.
    Init,
    /// Let the AI use a folder.
    Allow { folder: PathBuf },
    /// Stop the AI from using a folder.
    Disallow { folder: PathBuf },
    /// Show the config file path and settings (the token is hidden).
    Config {
        /// Show the token too.
        #[arg(long)]
        show_token: bool,
    },
    /// Register relay-mcp with Chrome (and Chromium, Edge, Brave) so the
    /// extension can start it.
    InstallNativeHost {
        /// Also allow this extension ID (for example the Chrome Web Store ID).
        #[arg(long = "extension-id")]
        extension_ids: Vec<String>,
        /// Write the manifest into this folder only (for testing with a
        /// custom browser profile).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Remove the Chrome registration.
    UninstallNativeHost,
    /// Run as the native messaging host (Chrome normally does this).
    #[command(hide = true)]
    NativeHost,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if native_host::started_by_chrome(&args) {
        return native_host::run(config_path(None)?).map_err(anyhow::Error::msg);
    }

    let cli = Cli::parse();
    let command = cli.command.unwrap_or(Command::Stdio);

    // Logs always go to stderr: in stdio mode stdout belongs to MCP.
    let default_level = if matches!(command, Command::Serve) {
        "info"
    } else {
        "warn"
    };
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level)),
        )
        .init();

    let path = config_path(cli.config)?;
    run(command, path)
}

fn config_path(given: Option<PathBuf>) -> Result<PathBuf> {
    match given.or_else(|| std::env::var_os("AI_FILE_RELAY_CONFIG").map(PathBuf::from)) {
        Some(p) => Ok(p),
        None => Ok(Config::default_path()?),
    }
}

fn run(command: Command, path: PathBuf) -> Result<()> {
    match command {
        Command::Init => {
            let (_, created) = Config::load_or_create(&path)?;
            if created {
                println!("Created {}", path.display());
            } else {
                println!("Already exists: {}", path.display());
            }
            println!("Next: relay-mcp allow <folder>");
            Ok(())
        }
        Command::Allow { folder } => {
            let (mut config, _) = Config::load_or_create(&path)?;
            let real = config.allow(&folder)?;
            config.save(&path)?;
            println!("Allowed: {}", real.display());
            Ok(())
        }
        Command::Disallow { folder } => {
            let (mut config, _) = Config::load_or_create(&path)?;
            if config.disallow(&folder) {
                config.save(&path)?;
                println!("Removed: {}", folder.display());
            } else {
                println!("Was not in the list: {}", folder.display());
            }
            Ok(())
        }
        Command::Config { show_token } => {
            let (mut config, _) = Config::load_or_create(&path)?;
            if !show_token {
                config.token = "(hidden, use --show-token)".into();
            }
            println!("# {}\n{}", path.display(), config.to_toml()?);
            Ok(())
        }
        Command::InstallNativeHost { extension_ids, dir } => {
            let exe = std::env::current_exe().context("could not find relay-mcp's own path")?;
            let ids = install::extension_ids(&extension_ids);
            match dir {
                Some(dir) => {
                    let file = install::write_manifest(&dir, &exe, &ids)?;
                    println!("Wrote {}", file.display());
                }
                None => {
                    for done in install::install(&exe, &ids).map_err(anyhow::Error::msg)? {
                        println!("{}: {}", done.browser, done.location);
                    }
                }
            }
            println!("Chrome will start: {}", exe.display());
            println!("Now load the extension (see the README) and restart Chrome.");
            Ok(())
        }
        Command::UninstallNativeHost => {
            let removed = install::uninstall().map_err(anyhow::Error::msg)?;
            if removed.is_empty() {
                println!("Nothing to remove.");
            }
            for r in removed {
                println!("Removed {r}");
            }
            Ok(())
        }
        Command::NativeHost => native_host::run(path).map_err(anyhow::Error::msg),
        Command::Stdio => runtime()?.block_on(run_stdio(path)),
        Command::Serve => runtime()?.block_on(run_serve(path)),
    }
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("could not start the async runtime")
}

fn warn_missing(relay: &Relay) {
    for f in &relay.missing_folders() {
        tracing::warn!("allowed folder no longer exists, skipping: {}", f.display());
    }
}

async fn run_stdio(path: PathBuf) -> Result<()> {
    let (config, _) = Config::load_or_create(&path)?;
    let port = config.port;

    // A hub is already running (the desktop app, `relay-mcp serve`, or
    // another AI app's relay-mcp): forward everything to it, so all AI apps
    // share one browser link, one approval popup and one history.
    if let Some(hub) = probe_hub(port).await {
        tracing::info!(
            "forwarding to the {} hub {} on port {port}",
            hub.role,
            hub.version
        );
        let proxy = HubProxy::connect(&config)
            .await
            .context("found the hub but could not connect to its MCP endpoint")?;
        let service = proxy
            .serve(rmcp::transport::stdio())
            .await
            .context("MCP over stdio failed to start")?;
        service.waiting().await?;
        return Ok(());
    }

    let mut relay = Relay::from_config(config).with_hub_role("stdio");
    let bridge = ExtensionBridge::new();

    // Also start the file server, so uploads can work without the hub.
    // If the port is taken (for example the hub is already running), the
    // tools still work but uploads will say the file server is off.
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => {
            relay = relay
                .with_file_server(format!("http://{addr}"))
                .with_extension(bridge);
            Some(l)
        }
        Err(e) => {
            tracing::warn!("file server not started on {addr}: {e}");
            None
        }
    };
    warn_missing(&relay);
    let relay = Arc::new(relay);

    let shutdown = CancellationToken::new();
    if let Some(listener) = listener {
        let app = http::router(relay.clone(), shutdown.clone());
        let stop = shutdown.clone();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async move { stop.cancelled_owned().await })
                .await;
        });
    }

    let service = RelayServer::new(relay)
        .serve(rmcp::transport::stdio())
        .await
        .context("MCP over stdio failed to start")?;
    service.waiting().await?;
    shutdown.cancel();
    Ok(())
}

async fn run_serve(path: PathBuf) -> Result<()> {
    let (config, created) = Config::load_or_create(&path)?;
    if created {
        tracing::info!("created config at {}", path.display());
    }
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, config.port));
    let relay = Relay::from_config(config)
        .with_file_server(format!("http://{addr}"))
        .with_extension(ExtensionBridge::new());
    warn_missing(&relay);
    if relay.allowed_roots().is_empty() {
        tracing::warn!("no folders are allowed yet; run `relay-mcp allow <folder>`");
    }
    let relay = Arc::new(relay);

    let shutdown = CancellationToken::new();
    let app = http::router(relay, shutdown.clone());
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("could not listen on {addr}"))?;
    tracing::info!("hub running: MCP at http://{addr}/mcp, files at http://{addr}/files/");

    let stop = shutdown.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        stop.cancel();
    });
    axum::serve(listener, app)
        .with_graceful_shutdown(async move { shutdown.cancelled_owned().await })
        .await?;
    Ok(())
}
