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
    /// Create a new telemetry logger for a specific session
    pub fn new_for_session(session_id: &str) -> Result<Self> {
        // Use the same directory structure as session files
        let log_dir = crate::session::ensure_session_dir()?
            .join("telemetry");

        // Create the telemetry subdirectory if it doesn't exist
        fs::create_dir_all(&log_dir)?;

        // Create a telemetry file with the same name as the session
        let log_file_path = log_dir.join(format!("{}.jsonl", session_id));

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

// Global map of session-specific telemetry loggers
lazy_static::lazy_static! {
    static ref TELEMETRY_LOGGERS: Arc<Mutex<std::collections::HashMap<String, TelemetryLogger>>> = Arc::new(Mutex::new(std::collections::HashMap::new()));
    static ref CURRENT_SESSION_ID: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
}

/// Initialize a telemetry logger for a specific session
pub async fn init_telemetry_logger_for_session(session_id: String) -> Result<()> {
    let mut loggers = TELEMETRY_LOGGERS.lock().await;
    let logger = TelemetryLogger::new_for_session(&session_id)?;
    loggers.insert(session_id.clone(), logger);
    
    // Set this as the current session
    let mut current = CURRENT_SESSION_ID.lock().await;
    *current = Some(session_id);
    
    Ok(())
}

/// Set the current session ID for telemetry logging
pub async fn set_current_session_id(session_id: Option<String>) {
    let mut current = CURRENT_SESSION_ID.lock().await;
    *current = session_id;
}

/// Get the telemetry logger for the current session
pub async fn get_telemetry_logger() -> Option<TelemetryLogger> {
    let current_session = CURRENT_SESSION_ID.lock().await;
    if let Some(session_id) = current_session.as_ref() {
        let loggers = TELEMETRY_LOGGERS.lock().await;
        loggers.get(session_id).map(|l| TelemetryLogger {
            log_file_path: l.log_file_path.clone(),
            file_mutex: l.file_mutex.clone(),
        })
    } else {
        None
    }
}

/// Get the telemetry logger for a specific session
pub async fn get_telemetry_logger_for_session(session_id: &str) -> Option<TelemetryLogger> {
    let loggers = TELEMETRY_LOGGERS.lock().await;
    loggers.get(session_id).map(|l| TelemetryLogger {
        log_file_path: l.log_file_path.clone(),
        file_mutex: l.file_mutex.clone(),
    })
}

/// Log a telemetry event using the current session's logger
pub async fn log_telemetry_event(entry: TelemetryLogEntry) -> Result<()> {
    if let Some(logger) = get_telemetry_logger().await {
        logger.log(entry).await?;
    }
    Ok(())
}

/// List all telemetry files
pub fn list_telemetry_files() -> Result<Vec<(String, PathBuf)>> {
    let telemetry_dir = crate::session::ensure_session_dir()?.join("telemetry");
    
    if !telemetry_dir.exists() {
        return Ok(Vec::new());
    }
    
    let entries = fs::read_dir(&telemetry_dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            
            if path.extension().is_some_and(|ext| ext == "jsonl") {
                let name = path.file_stem()?.to_string_lossy().to_string();
                Some((name, path))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    
    Ok(entries)
}
