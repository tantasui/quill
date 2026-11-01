use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use quill::{auth, backup, db, settings, AppState};

#[derive(Parser)]
#[command(
    name = "quill",
    version,
    about = "A lightweight, self-hosted blogging engine."
)]
struct Cli {
    /// Folder for the database, uploads and backups.
    #[arg(long, env = "QUILL_DATA_DIR", default_value = "./data", global = true)]
    data_dir: PathBuf,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the blog server (the default when no command is given).
    Serve(ServeArgs),
    /// Write a backup (database snapshot + media). Safe while the server runs.
    Backup {
        /// Destination folder. Defaults to <data-dir>/backups/backup-<time>.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Replace the blog's data with a backup. Stop the server first.
    Restore {
        /// A folder created by `quill backup`.
        from: PathBuf,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
    /// Export all articles as Markdown files with front matter.
    Export {
        #[arg(long, default_value = "export")]
        out: PathBuf,
    },
    /// Set a new password for an administrator (reads it from standard input).
    ResetPassword {
        /// Defaults to the first administrator account.
        username: Option<String>,
    },
}

#[derive(Parser, Clone)]
struct ServeArgs {
    /// Address and port to listen on. Put a reverse proxy in front for HTTPS.
    #[arg(long, env = "QUILL_LISTEN", default_value = "127.0.0.1:3000")]
    listen: String,
    /// Public address of the blog, e.g. https://example.com/blog. Overrides the
    /// dashboard setting on every start.
    #[arg(long, env = "QUILL_BASE_URL")]
    base_url: Option<String>,
    /// Fixed setup code for unattended installs (otherwise one is generated).
    #[arg(long, env = "QUILL_SETUP_CODE")]
    setup_code: Option<String>,
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("QUILL_LOG")
                .unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();
    let result = match cli.command {
        None => serve(&cli.data_dir, ServeArgs::parse_from(["quill"])).await,
        Some(Command::Serve(args)) => serve(&cli.data_dir, args).await,
        Some(Command::Backup { out }) => backup::backup(&cli.data_dir, out)
            .await
            .map(|p| println!("Backup written to {}", p.display())),
        Some(Command::Restore { from, yes }) => restore(&cli.data_dir, &from, yes).await,
        Some(Command::Export { out }) => backup::export(&cli.data_dir, &out)
            .await
            .map(|n| println!("Exported {n} article(s) to {}", out.display())),
        Some(Command::ResetPassword { username }) => reset_password(&cli.data_dir, username).await,
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("\nError: {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn open_data_dir(data_dir: &Path) -> Result<sqlx::SqlitePool> {
    std::fs::create_dir_all(data_dir.join("media")).with_context(|| {
        format!(
            "could not create the data folder {}. Make sure it is writable, or choose another location with --data-dir or QUILL_DATA_DIR",
            data_dir.display()
        )
    })?;
    let pool = db::connect(&data_dir.join(db::DB_FILE)).await?;
    db::migrate(&pool, Some(&data_dir.join("backups"))).await?;
    Ok(pool)
}

async fn serve(data_dir: &Path, args: ServeArgs) -> Result<()> {
    let pool = open_data_dir(data_dir).await?;
    let state = AppState::new(
        pool,
        data_dir.to_path_buf(),
        args.listen.clone(),
        args.setup_code,
    )
    .await?;

    if let Some(url) = args.base_url {
        let url = settings::normalize_base_url(&url)
            .map_err(|e| anyhow::anyhow!("QUILL_BASE_URL: {e}"))?;
        let mut s = state.settings();
        if s.base_url != url {
            s.set_base_url(&url);
            state.save_settings(s).await?;
        }
    }

    let listener = tokio::net::TcpListener::bind(&args.listen).await.map_err(|e| match e.kind() {
        std::io::ErrorKind::AddrInUse => anyhow::anyhow!(
            "{} is already in use. Another program (or another copy of Quill) is listening there. \
             Stop it, or pick a different port with --listen or QUILL_LISTEN (e.g. 127.0.0.1:3001).",
            args.listen
        ),
        std::io::ErrorKind::PermissionDenied => anyhow::anyhow!(
            "permission denied listening on {}. Ports below 1024 need extra privileges; listen on a port \
             like 3000 and let your web server (nginx, Caddy, Apache) forward traffic to it.",
            args.listen
        ),
        _ => anyhow::anyhow!("could not listen on {}: {e}", args.listen),
    })?;

    let local = format!("http://{}", args.listen.replace("0.0.0.0", "localhost"));
    let s = state.settings();
    println!(
        "\n  Quill {} is running at {local}{}",
        env!("CARGO_PKG_VERSION"),
        s.base_path
    );
    if let Some(code) = state.setup_code() {
        println!("\n  Finish setting up in your browser:");
        println!("    {local}/admin/setup");
        println!("  Setup code: {code}\n");
    } else {
        println!(
            "  Dashboard: {local}{}/admin   Public address: {}\n",
            s.base_path, s.base_url
        );
    }

    axum::serve(listener, quill::app(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    tracing::info!("stopped");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

fn prompt(question: &str) -> Result<String> {
    print!("{question}");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

async fn restore(data_dir: &Path, from: &Path, yes: bool) -> Result<()> {
    if !yes {
        println!(
            "This replaces the blog in {} with the backup in {}.",
            data_dir.display(),
            from.display()
        );
        println!(
            "Stop the server before continuing. The current data will be moved aside, not deleted."
        );
        if !prompt("Continue? [y/N] ")?.eq_ignore_ascii_case("y") {
            bail!("restore cancelled");
        }
    }
    let aside = backup::restore(data_dir, from).await?;
    println!(
        "Restored. The previous data was moved to {}.",
        aside.display()
    );
    println!("Start the server again; any needed database upgrades run automatically.");
    Ok(())
}

async fn reset_password(data_dir: &Path, username: Option<String>) -> Result<()> {
    let pool = open_data_dir(data_dir).await?;
    let user: Option<(i64, String)> = match &username {
        Some(name) => {
            sqlx::query_as("SELECT id, username FROM users WHERE username = ?")
                .bind(name)
                .fetch_optional(&pool)
                .await?
        }
        None => {
            sqlx::query_as("SELECT id, username FROM users ORDER BY id LIMIT 1")
                .fetch_optional(&pool)
                .await?
        }
    };
    let Some((id, name)) = user else {
        bail!("no such administrator; finish setup in the browser first");
    };
    let password = prompt(&format!("New password for {name}: "))?;
    if password.chars().count() < auth::MIN_PASSWORD_LEN {
        bail!("use at least {} characters", auth::MIN_PASSWORD_LEN);
    }
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(auth::hash_password(&password)?)
        .bind(id)
        .execute(&pool)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = ?")
        .bind(id)
        .execute(&pool)
        .await?;
    println!("Password updated for {name}. All existing sessions were signed out.");
    Ok(())
}
