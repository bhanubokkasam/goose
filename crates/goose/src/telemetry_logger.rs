use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// A log entry for telemetry events (API requests, tool calls, wait events, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryLogEntry {
    pub timestamp: DateTime<Utc>,
    pub request_type: String, // "complete", "stream", "wait_event", "api_post", etc.
    pub provider: String,
    pub model: String,
    pub request: serde_json::Value,
    pub response: Option<serde_json::Value>,
    pub error: Option<String>,
    pub duration_ms: Option<u64>,
}

/// Logger for telemetry events
pub struct TelemetryLogger {
    log_file_path: PathBuf,
    file_mutex: Arc<Mutex<()>>,
}

impl TelemetryLogger {
    /// Create a new telemetry logger
    pub fn new() -> Result<Self> {
        let log_dir = dirs::data_local_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine local data directory"))?
            .join("goose")
            .join("logs");

        // Create the logs directory if it doesn't exist
        fs::create_dir_all(&log_dir)?;

        let log_file_path = log_dir.join("telemetry.jsonl");

        Ok(Self {
            log_file_path,
            file_mutex: Arc::new(Mutex::new(())),
        })
    }

    /// Get the path to the log file
    pub fn log_file_path(&self) -> &PathBuf {
        &self.log_file_path
    }

    /// Log a telemetry event
    pub async fn log(&self, entry: TelemetryLogEntry) -> Result<()> {
        let _lock = self.file_mutex.lock().await;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_file_path)?;

        let json = serde_json::to_string(&entry)?;
        writeln!(file, "{}", json)?;
        file.flush()?;

        Ok(())
    }

    /// Clear the log file
    pub async fn clear(&self) -> Result<()> {
        let _lock = self.file_mutex.lock().await;
        fs::write(&self.log_file_path, "")?;
        Ok(())
    }

    /// Get the size of the log file in bytes
    pub async fn size(&self) -> Result<u64> {
        let metadata = fs::metadata(&self.log_file_path)?;
        Ok(metadata.len())
    }
}

// Global singleton for the telemetry logger
lazy_static::lazy_static! {
    static ref TELEMETRY_LOGGER: Arc<Mutex<Option<TelemetryLogger>>> = Arc::new(Mutex::new(None));
}

/// Initialize the global telemetry logger
pub async fn init_telemetry_logger() -> Result<()> {
    let mut logger = TELEMETRY_LOGGER.lock().await;
    *logger = Some(TelemetryLogger::new()?);
    Ok(())
}

/// Get the global telemetry logger
pub async fn get_telemetry_logger() -> Option<TelemetryLogger> {
    let logger = TELEMETRY_LOGGER.lock().await;
    logger.as_ref().map(|l| TelemetryLogger {
        log_file_path: l.log_file_path.clone(),
        file_mutex: l.file_mutex.clone(),
    })
}

/// Log a telemetry event using the global logger
pub async fn log_telemetry_event(entry: TelemetryLogEntry) -> Result<()> {
    if let Some(logger) = get_telemetry_logger().await {
        logger.log(entry).await?;
    }
    Ok(())
}
