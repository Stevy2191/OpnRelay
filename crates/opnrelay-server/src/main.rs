// SPDX-License-Identifier: AGPL-3.0-only
use std::net::SocketAddr;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use opnrelay_server::{MIGRATOR, app, bootstrap, config::Config, connect, state::AppState};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "opnrelay-server",
    version,
    about = "OpnRelay self-hosted guide server"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the HTTP server (applies database migrations first).
    Serve,
    /// Apply database migrations and exit.
    Migrate,
    /// Administrative tasks.
    Admin {
        #[command(subcommand)]
        cmd: AdminCmd,
    },
    /// Key management.
    Keys {
        #[command(subcommand)]
        cmd: KeysCmd,
    },
    /// Backups: run one now, or restore one into a fresh installation.
    Backup {
        #[command(subcommand)]
        cmd: BackupCmd,
    },
    /// Exit 0 if the local server answers /healthz (for container health checks).
    Health,
    /// Audit log tools.
    Audit {
        #[command(subcommand)]
        cmd: AuditCmd,
    },
}

#[derive(Subcommand)]
enum AdminCmd {
    /// Create (or recover) an owner account. Prints a one-time setup link.
    CreateOwner {
        #[arg(long)]
        email: String,
        #[arg(long, default_value = "Owner")]
        name: String,
        /// Organisation name (used on first run only).
        #[arg(long, default_value = "My Company")]
        org_name: String,
    },
}

#[derive(Subcommand)]
enum KeysCmd {
    /// Print a new random 256-bit key (for OPNRELAY_MASTER_KEY).
    Generate,
}

#[derive(Subcommand)]
enum BackupCmd {
    /// Create a backup now (and upload it to OneDrive if connected).
    Run,
    /// Restore a backup file into an empty database (fresh install).
    Restore {
        /// Path to the .orbk file (inside the container, e.g. /data/backups/...).
        file: std::path::PathBuf,
    },
}

#[derive(Subcommand)]
enum AuditCmd {
    /// Verify the audit log hash chain.
    Verify,
}

fn init_tracing() {
    let filter = EnvFilter::try_from_env("OPNRELAY_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn"));
    let json = std::env::var("OPNRELAY_LOG_FORMAT").is_ok_and(|v| v == "json");
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    if json {
        builder.json().init();
    } else {
        builder.init();
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Cmd::Keys {
        cmd: KeysCmd::Generate,
    } = cli.cmd
    {
        println!("{}", opnrelay_crypto::SecretKey::generate().to_hex());
        return Ok(());
    }
    if let Cmd::Health = cli.cmd {
        std::process::exit(if health_probe() { 0 } else { 1 });
    }
    init_tracing();
    let cfg = Config::from_env()?;
    let db = connect(&cfg.database_url)
        .await
        .context("connecting to the database")?;
    match cli.cmd {
        Cmd::Migrate => {
            MIGRATOR.run(&db).await?;
            println!("migrations applied");
        }
        Cmd::Serve => {
            MIGRATOR.run(&db).await.context("applying migrations")?;
            let state = AppState::new(db, cfg.clone());
            state.storage.ensure_root().await?;
            tokio::fs::create_dir_all(&cfg.backup_dir).await.ok();
            opnrelay_server::backup::spawn_scheduler(state.clone());
            opnrelay_server::group_sync::spawn_scheduler(state.clone());
            let listener = tokio::net::TcpListener::bind(cfg.bind).await?;
            tracing::info!(addr = %cfg.bind, url = %cfg.public_url, "OpnRelay listening");
            axum::serve(
                listener,
                app(state).into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(shutdown())
            .await?;
        }
        Cmd::Admin {
            cmd:
                AdminCmd::CreateOwner {
                    email,
                    name,
                    org_name,
                },
        } => {
            MIGRATOR.run(&db).await?;
            let url =
                bootstrap::create_owner(&db, &cfg.public_url, &org_name, &email, &name).await?;
            println!("Owner account ready for {email}.");
            println!("Open this one-time link within 7 days to set the password:\n\n  {url}\n");
        }
        Cmd::Backup {
            cmd: BackupCmd::Run,
        } => {
            MIGRATOR.run(&db).await?;
            let state = AppState::new(db, cfg.clone());
            let path =
                opnrelay_server::backup::run(&state, opnrelay_server::audit::AuditActor::system())
                    .await?;
            println!("Backup written to {}", path.display());
        }
        Cmd::Backup {
            cmd: BackupCmd::Restore { file },
        } => {
            MIGRATOR.run(&db).await?;
            let m = opnrelay_server::backup::restore(&db, &cfg.storage_dir, &cfg.master_key, &file)
                .await?;
            let rows: u64 = m.tables.iter().map(|t| t.rows).sum();
            println!(
                "Restored backup from {} ({rows} rows, {} files).",
                m.created_at, m.media_files
            );
        }
        Cmd::Audit {
            cmd: AuditCmd::Verify,
        } => {
            let r = opnrelay_server::audit::verify(&db).await?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            if !r.ok {
                std::process::exit(2);
            }
        }
        Cmd::Keys { .. } | Cmd::Health => unreachable!(),
    }
    Ok(())
}

async fn shutdown() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
    tracing::info!("shutting down");
}

/// Minimal HTTP probe without extra dependencies (the runtime image has no curl).
fn health_probe() -> bool {
    use std::io::{Read, Write};
    let bind = std::env::var("OPNRELAY_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let port = bind.rsplit(':').next().unwrap_or("8080");
    let Ok(mut s) = std::net::TcpStream::connect(format!("127.0.0.1:{port}")) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(3)));
    if s.write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut buf = [0u8; 64];
    let n = s.read(&mut buf).unwrap_or(0);
    buf[..n].starts_with(b"HTTP/1.1 200")
}
