//! Background daemon for automatic feed updates.
//!
//! Provides Chrome-updater-style background updates without requiring
//! system scheduler configuration.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use chrono::{Local, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::time::interval;

use crate::app::AppContext;
use crate::store::{RefreshSource, Store};

/// Daemon configuration
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// Update interval in seconds (default: 3600 = 1 hour)
    pub update_interval_secs: u64,
    /// Whether to run an update immediately on start
    pub update_on_start: bool,
    /// Log file path (None = stdout)
    pub log_file: Option<PathBuf>,
    /// Internal identity token for detached daemon processes
    pub instance_token: Option<String>,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            update_interval_secs: 3600, // 1 hour
            update_on_start: true,
            log_file: None,
            instance_token: None,
        }
    }
}

impl DaemonConfig {
    /// Parse interval string like "1h", "30m", "6h", "1d"
    pub fn parse_interval(s: &str) -> Result<u64, String> {
        let s = s.trim().to_lowercase();

        let seconds = if let Some(hours) = s.strip_suffix('h') {
            hours
                .parse::<u64>()
                .map_err(|_| format!("Invalid hours: {}", hours))?
                .checked_mul(3600)
                .ok_or_else(|| "Interval is too large".to_string())?
        } else if let Some(minutes) = s.strip_suffix('m') {
            minutes
                .parse::<u64>()
                .map_err(|_| format!("Invalid minutes: {}", minutes))?
                .checked_mul(60)
                .ok_or_else(|| "Interval is too large".to_string())?
        } else if let Some(days) = s.strip_suffix('d') {
            days.parse::<u64>()
                .map_err(|_| format!("Invalid days: {}", days))?
                .checked_mul(86400)
                .ok_or_else(|| "Interval is too large".to_string())?
        } else if let Some(secs) = s.strip_suffix('s') {
            secs.parse::<u64>()
                .map_err(|_| format!("Invalid seconds: {}", secs))?
        } else {
            // Try parsing as raw seconds
            s.parse::<u64>().map_err(|_| {
                format!("Invalid interval: {}. Use format like '1h', '30m', '1d'", s)
            })?
        };

        if seconds == 0 {
            return Err("Update interval must be greater than zero".to_string());
        }
        Ok(seconds)
    }

    /// Format interval for display
    pub fn format_interval(secs: u64) -> String {
        if secs >= 86400 && secs.is_multiple_of(86400) {
            format!("{}d", secs / 86400)
        } else if secs >= 3600 && secs.is_multiple_of(3600) {
            format!("{}h", secs / 3600)
        } else if secs >= 60 && secs.is_multiple_of(60) {
            format!("{}m", secs / 60)
        } else {
            format!("{}s", secs)
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct PidRecord {
    pid: u32,
    executable: String,
    instance_token: Option<String>,
}

struct PidFileGuard {
    path: PathBuf,
    pid: u32,
}

impl Drop for PidFileGuard {
    fn drop(&mut self) {
        if read_pid_record(&self.path).is_ok_and(|record| record.pid == self.pid) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Daemon runner
pub struct Daemon {
    ctx: Arc<AppContext>,
    config: DaemonConfig,
    running: Arc<AtomicBool>,
}

impl Daemon {
    pub fn new(ctx: Arc<AppContext>, config: DaemonConfig) -> Self {
        Self {
            ctx,
            config,
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    /// Get the PID file path
    pub fn pid_file_path() -> Option<PathBuf> {
        dirs::runtime_dir()
            .or_else(dirs::cache_dir)
            .map(|d| d.join("rivulet").join("daemon.pid"))
    }

    /// Check if another daemon is already running
    pub fn is_running() -> bool {
        Self::pid_file_path()
            .and_then(|path| read_pid_record(&path).ok())
            .is_some_and(|record| Self::process_matches(&record))
    }

    #[cfg(unix)]
    fn process_exists(pid: u32) -> bool {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[cfg(windows)]
    fn process_exists(pid: u32) -> bool {
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid)])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }

    fn process_matches(record: &PidRecord) -> bool {
        if record.pid == 0 || !Self::process_exists(record.pid) {
            return false;
        }

        let Some(command_line) = process_command_line(record.pid) else {
            return false;
        };
        let executable_name = Path::new(&record.executable)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("rivulet");
        let daemon_command = command_line.contains(executable_name)
            && command_line.contains("daemon")
            && command_line.contains("start")
            && command_line.contains("--foreground");
        let token_matches = record
            .instance_token
            .as_ref()
            .is_none_or(|token| command_line.contains(token));

        daemon_command && token_matches
    }

    /// Write a structured PID file using exclusive creation.
    fn write_pid_file(&self) -> std::io::Result<PidFileGuard> {
        let pid_path = Self::pid_file_path()
            .ok_or_else(|| std::io::Error::other("Could not determine PID file path"))?;
        if let Some(parent) = pid_path.parent() {
            fs::create_dir_all(parent)?;
        }

        if pid_path.exists() {
            match read_pid_record(&pid_path) {
                Ok(record) if Self::process_matches(&record) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        "Another daemon instance is already running",
                    ));
                }
                _ => {
                    fs::remove_file(&pid_path)?;
                }
            }
        }

        let record = PidRecord {
            pid: std::process::id(),
            executable: std::env::current_exe()?.to_string_lossy().into_owned(),
            instance_token: self.config.instance_token.clone(),
        };
        let payload = serde_json::to_vec(&record).map_err(std::io::Error::other)?;

        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&pid_path)?;
        file.write_all(&payload)?;
        file.sync_all()?;

        Ok(PidFileGuard {
            path: pid_path,
            pid: record.pid,
        })
    }

    /// Log a message with timestamp
    fn log(&self, msg: &str) {
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
        let line = format!("[{}] {}", timestamp, msg);

        if let Some(ref log_path) = self.config.log_file {
            if let Ok(mut file) = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_path)
            {
                let _ = writeln!(file, "{}", line);
            }
        } else {
            println!("{}", line);
        }
    }

    /// Run the daemon
    pub async fn run(&self) -> crate::app::Result<()> {
        if self.config.update_interval_secs == 0 {
            return Err(crate::app::RivuletError::Config(
                "Daemon update interval must be greater than zero".to_string(),
            ));
        }

        // Check for existing daemon
        if Self::is_running() {
            return Err(crate::app::RivuletError::Other(
                "Another daemon instance is already running".to_string(),
            ));
        }

        // Write PID file
        let _pid_guard = self.write_pid_file().map_err(|e| {
            crate::app::RivuletError::Other(format!("Failed to write PID file: {}", e))
        })?;

        // Set up signal handler for graceful shutdown
        let running = self.running.clone();

        #[cfg(unix)]
        {
            let running_clone = running.clone();
            tokio::spawn(async move {
                let mut sigterm =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("Failed to set up SIGTERM handler");
                let mut sigint =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                        .expect("Failed to set up SIGINT handler");

                tokio::select! {
                    _ = sigterm.recv() => {},
                    _ = sigint.recv() => {},
                }
                running_clone.store(false, Ordering::SeqCst);
            });
        }

        #[cfg(windows)]
        {
            let running_clone = running.clone();
            tokio::spawn(async move {
                let _ = tokio::signal::ctrl_c().await;
                running_clone.store(false, Ordering::SeqCst);
            });
        }

        self.log(&format!(
            "Rivulet daemon started (update interval: {}, PID: {})",
            DaemonConfig::format_interval(self.config.update_interval_secs),
            std::process::id()
        ));

        // Run initial update if configured
        if self.config.update_on_start {
            self.log("Running initial update...");
            self.run_update().await;
        }

        // Main loop
        let mut timer = interval(Duration::from_secs(self.config.update_interval_secs));
        timer.tick().await; // Skip the first immediate tick

        while self.running.load(Ordering::SeqCst) {
            timer.tick().await;

            if !self.running.load(Ordering::SeqCst) {
                break;
            }

            self.log("Running scheduled update...");
            self.run_update().await;
        }

        self.log("Daemon shutting down...");

        Ok(())
    }

    /// Run a single update cycle
    async fn run_update(&self) {
        let start = Utc::now();

        match self.ctx.store.get_all_feeds() {
            Ok(feeds) => {
                if feeds.is_empty() {
                    self.log("No feeds to update");
                    return;
                }

                let run_id = match self
                    .ctx
                    .store
                    .begin_refresh_run(RefreshSource::Daemon, feeds.len())
                {
                    Ok(run_id) => run_id,
                    Err(e) => {
                        self.log(&format!("Failed to start refresh run: {}", e));
                        return;
                    }
                };

                let results = self
                    .ctx
                    .parallel_fetcher
                    .fetch_all(feeds, self.ctx.store.clone(), &self.ctx.normalizer, None)
                    .await;

                let mut total_new = 0;
                let mut errors = 0;

                for (feed_id, result) in results {
                    match result {
                        Ok(refresh) => {
                            total_new += refresh.new_count;
                            if let Err(e) = self.ctx.store.record_refresh_run_items(
                                run_id,
                                feed_id,
                                &refresh.inserted_item_ids,
                            ) {
                                self.log(&format!("  Error recording refresh items: {}", e));
                            }
                            if refresh.new_count > 0 {
                                if let Ok(Some(feed)) = self.ctx.store.get_feed(feed_id) {
                                    self.log(&format!(
                                        "  {} new items from {}",
                                        refresh.new_count,
                                        feed.display_title()
                                    ));
                                }
                            }
                        }
                        Err(e) => {
                            errors += 1;
                            if let Ok(Some(feed)) = self.ctx.store.get_feed(feed_id) {
                                self.log(&format!(
                                    "  Error updating {}: {}",
                                    feed.display_title(),
                                    e
                                ));
                            }
                        }
                    }
                }
                if let Err(e) = self
                    .ctx
                    .store
                    .complete_refresh_run(run_id, total_new, errors)
                {
                    self.log(&format!("Failed to complete refresh run: {}", e));
                }

                let elapsed = Utc::now().signed_duration_since(start);
                self.log(&format!(
                    "Update complete: {} new items, {} errors ({:.1}s)",
                    total_new,
                    errors,
                    elapsed.num_milliseconds() as f64 / 1000.0
                ));
            }
            Err(e) => {
                self.log(&format!("Failed to get feeds: {}", e));
            }
        }
    }

    /// Stop the daemon (called externally)
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

/// Stop a running daemon by reading PID file and sending signal
pub fn stop_daemon() -> Result<(), String> {
    let pid_path =
        Daemon::pid_file_path().ok_or_else(|| "Could not determine PID file path".to_string())?;

    if !pid_path.exists() {
        return Err("No daemon is running (PID file not found)".to_string());
    }

    let record =
        read_pid_record(&pid_path).map_err(|e| format!("Failed to read PID file: {}", e))?;
    if !Daemon::process_matches(&record) {
        let _ = fs::remove_file(&pid_path);
        return Err("PID file is stale or does not identify a Rivulet daemon".to_string());
    }
    let pid = record.pid;

    #[cfg(unix)]
    {
        let status = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status()
            .map_err(|e| format!("Failed to send signal: {}", e))?;

        if status.success() {
            wait_for_daemon_exit(&record, &pid_path)
        } else {
            Err(format!("Failed to stop daemon (PID {})", pid))
        }
    }

    #[cfg(windows)]
    {
        let status = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .status()
            .map_err(|e| format!("Failed to stop process: {}", e))?;

        if status.success() {
            wait_for_daemon_exit(&record, &pid_path)
        } else {
            Err(format!("Failed to stop daemon (PID {})", pid))
        }
    }
}

/// Check daemon status
pub fn daemon_status() -> String {
    if let Some(pid_path) = Daemon::pid_file_path() {
        if pid_path.exists() {
            if let Ok(record) = read_pid_record(&pid_path) {
                if Daemon::process_matches(&record) {
                    return format!("Daemon is running (PID: {})", record.pid);
                }
            }
            let _ = fs::remove_file(pid_path);
            return "Daemon is not running (removed stale PID file)".to_string();
        }
    }
    "Daemon is not running".to_string()
}

pub fn generate_instance_token() -> String {
    let mut hasher = Sha256::new();
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_le_bytes(),
    );
    hasher.update(format!("{:?}", thread::current().id()).as_bytes());
    hex::encode(hasher.finalize())
}

fn read_pid_record(path: &Path) -> std::io::Result<PidRecord> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "PID file must not be a symbolic link",
        ));
    }
    let payload = fs::read(path)?;
    let record: PidRecord = serde_json::from_slice(&payload).map_err(std::io::Error::other)?;
    if record.pid == 0 || record.executable.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "PID file is missing process identity",
        ));
    }
    Ok(record)
}

#[cfg(unix)]
fn process_command_line(pid: u32) -> Option<String> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|command| !command.is_empty())
}

#[cfg(windows)]
fn process_command_line(pid: u32) -> Option<String> {
    let query = format!(
        "(Get-CimInstance Win32_Process -Filter \"ProcessId = {}\").CommandLine",
        pid
    );
    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", &query])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|command| !command.is_empty())
}

fn wait_for_daemon_exit(record: &PidRecord, pid_path: &Path) -> Result<(), String> {
    for _ in 0..100 {
        if !Daemon::process_matches(record) {
            let _ = fs::remove_file(pid_path);
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(format!(
        "Daemon PID {} did not exit within five seconds",
        record.pid
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_interval() {
        assert_eq!(DaemonConfig::parse_interval("1h").unwrap(), 3600);
        assert_eq!(DaemonConfig::parse_interval("30m").unwrap(), 1800);
        assert_eq!(DaemonConfig::parse_interval("1d").unwrap(), 86400);
        assert_eq!(DaemonConfig::parse_interval("60s").unwrap(), 60);
        assert_eq!(DaemonConfig::parse_interval("3600").unwrap(), 3600);
        assert_eq!(DaemonConfig::parse_interval("6h").unwrap(), 21600);
        assert!(DaemonConfig::parse_interval("invalid").is_err());
        assert!(DaemonConfig::parse_interval("0").is_err());
        assert!(DaemonConfig::parse_interval("0s").is_err());
        assert!(DaemonConfig::parse_interval("18446744073709551615d").is_err());
    }

    #[test]
    fn test_format_interval() {
        assert_eq!(DaemonConfig::format_interval(3600), "1h");
        assert_eq!(DaemonConfig::format_interval(1800), "30m");
        assert_eq!(DaemonConfig::format_interval(86400), "1d");
        assert_eq!(DaemonConfig::format_interval(90), "90s");
        assert_eq!(DaemonConfig::format_interval(7200), "2h");
    }
}
