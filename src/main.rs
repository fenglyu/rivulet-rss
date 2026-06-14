use std::sync::Arc;

use clap::Parser;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use rivulet::app::AppContext;
use rivulet::cli::{commands, AuthAction, Cli, Commands, DaemonAction};
use rivulet::config::Config;
use rivulet::daemon::{Daemon, DaemonConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    init_tracing(&cli.command)?;

    // Load and validate scraper settings before creating worker pools.
    let config = Config::load()?;

    // Create app context with scraper enabled based on config
    let ctx = AppContext::with_scraper_config(None, cli.workers, Some(config.scraper.clone()))?;

    match cli.command {
        Commands::Init { force } => {
            commands::init_config(force)?;
            return Ok(());
        }
        Commands::Add { url } => {
            commands::add_feed(&ctx, &url).await?;
        }
        Commands::Remove { url } => {
            commands::remove_feed(&ctx, &url).await?;
        }
        Commands::Import { path } => {
            commands::import_opml(&ctx, &path).await?;
        }
        Commands::Update => {
            commands::update_feeds(&ctx).await?;
        }
        Commands::List {
            items,
            unread,
            starred,
            queued,
            saved,
            archived,
        } => {
            let filter =
                commands::list_filter_from_flags(unread, starred, queued, saved, archived)?;
            if items || filter.is_some() {
                commands::list_items(&ctx, filter)?;
            } else {
                commands::list_feeds(&ctx)?;
            }
        }
        Commands::Search {
            query,
            limit,
            unread,
            starred,
            queued,
            saved,
            archived,
        } => {
            let filter =
                commands::list_filter_from_flags(unread, starred, queued, saved, archived)?
                    .unwrap_or(rivulet::store::ItemListFilter::All);
            commands::search_items(&ctx, &query, filter, limit)?;
        }
        Commands::Tui => {
            rivulet::tui::run(Arc::new(ctx), Arc::new(config)).await?;
        }
        Commands::Scrape {
            feed,
            limit,
            concurrency,
            visible,
            auth_profile,
        } => {
            commands::scrape_content(
                &ctx,
                feed.as_deref(),
                limit,
                concurrency,
                visible,
                auth_profile.as_deref(),
            )
            .await?;
        }
        Commands::Auth { action } => match action {
            AuthAction::Add {
                name,
                site,
                profile_dir,
            } => {
                commands::auth_add(&ctx, &name, &site, profile_dir).await?;
            }
            AuthAction::Check { name, url, visible } => {
                commands::auth_check(&ctx, &name, url.as_deref(), visible).await?;
            }
            AuthAction::List => {
                commands::auth_list(&ctx)?;
            }
        },
        Commands::Daemon { action } => {
            match action {
                DaemonAction::Start {
                    interval,
                    no_initial_update,
                    log,
                    foreground,
                    instance_token,
                } => {
                    let interval_secs =
                        DaemonConfig::parse_interval(&interval).map_err(|e| anyhow::anyhow!(e))?;

                    let daemon_config = DaemonConfig {
                        update_interval_secs: interval_secs,
                        update_on_start: !no_initial_update,
                        log_file: log.clone(),
                        instance_token: instance_token.clone(),
                    };

                    if foreground {
                        // Run in foreground
                        let daemon = Daemon::new(Arc::new(ctx), daemon_config);
                        daemon.run().await?;
                    } else {
                        // Detach and run in background
                        #[cfg(unix)]
                        {
                            use std::process::Command;

                            let instance_token = rivulet::daemon::generate_instance_token();
                            let mut args = vec![
                                "daemon".to_string(),
                                "start".to_string(),
                                "--foreground".to_string(),
                                "--interval".to_string(),
                                interval,
                                "--instance-token".to_string(),
                                instance_token,
                            ];

                            if no_initial_update {
                                args.push("--no-initial-update".to_string());
                            }

                            if let Some(log_path) = log {
                                args.push("--log".to_string());
                                args.push(log_path.to_string_lossy().to_string());
                            }

                            let exe = std::env::current_exe()?;
                            Command::new(&exe)
                                .args(&args)
                                .stdin(std::process::Stdio::null())
                                .stdout(std::process::Stdio::null())
                                .stderr(std::process::Stdio::null())
                                .spawn()?;

                            println!("Daemon started in background");
                            println!("Use 'rivulet daemon status' to check status");
                            println!("Use 'rivulet daemon stop' to stop");
                        }

                        #[cfg(windows)]
                        {
                            use std::os::windows::process::CommandExt;
                            use std::process::Command;

                            const CREATE_NO_WINDOW: u32 = 0x08000000;
                            const DETACHED_PROCESS: u32 = 0x00000008;

                            let instance_token = rivulet::daemon::generate_instance_token();
                            let mut args = vec![
                                "daemon".to_string(),
                                "start".to_string(),
                                "--foreground".to_string(),
                                "--interval".to_string(),
                                interval,
                                "--instance-token".to_string(),
                                instance_token,
                            ];

                            if no_initial_update {
                                args.push("--no-initial-update".to_string());
                            }

                            if let Some(log_path) = log {
                                args.push("--log".to_string());
                                args.push(log_path.to_string_lossy().to_string());
                            }

                            let exe = std::env::current_exe()?;
                            Command::new(&exe)
                                .args(&args)
                                .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
                                .spawn()?;

                            println!("Daemon started in background");
                            println!("Use 'rivulet daemon status' to check status");
                            println!("Use 'rivulet daemon stop' to stop");
                        }
                    }
                }
                DaemonAction::Stop => match rivulet::daemon::stop_daemon() {
                    Ok(()) => println!("Daemon stopped"),
                    Err(e) => eprintln!("Error: {}", e),
                },
                DaemonAction::Status => {
                    println!("{}", rivulet::daemon::daemon_status());
                }
            }
        }
    }

    Ok(())
}

fn init_tracing(command: &Commands) -> anyhow::Result<()> {
    if matches!(command, Commands::Tui) {
        init_tui_tracing()
    } else {
        tracing_subscriber::registry()
            .with(fmt::layer())
            .with(env_filter("rivulet=info"))
            .init();
        Ok(())
    }
}

fn init_tui_tracing() -> anyhow::Result<()> {
    let log_path = std::env::var_os("RIVULET_TUI_LOG")
        .map(std::path::PathBuf::from)
        .unwrap_or(default_tui_log_path()?);

    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_ansi(false)
                .with_writer(move || file.try_clone().expect("failed to clone TUI log file")),
        )
        .with(env_filter("rivulet=debug,chromiumoxide=warn"))
        .init();

    Ok(())
}

fn default_tui_log_path() -> anyhow::Result<std::path::PathBuf> {
    let data_dir =
        dirs::data_dir().ok_or_else(|| anyhow::anyhow!("could not determine data directory"))?;
    Ok(data_dir.join("rivulet").join("tui.log"))
}

fn env_filter(default_directive: &str) -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_directive))
}
