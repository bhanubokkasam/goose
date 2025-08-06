use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

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

/// Zipkin-compatible span format
/// See: https://zipkin.io/zipkin-api/#/default/post_spans
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZipkinSpan {
    pub trace_id: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub name: String,
    pub timestamp: u64, // microseconds since epoch
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<u64>, // microseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>, // CLIENT, SERVER, PRODUCER, CONSUMER
    pub local_endpoint: ZipkinEndpoint,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_endpoint: Option<ZipkinEndpoint>,
    #[serde(default)]
    pub annotations: Vec<ZipkinAnnotation>,
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZipkinEndpoint {
    pub service_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv4: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv6: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZipkinAnnotation {
    pub timestamp: u64, // microseconds since epoch
    pub value: String,
}

/// Active span information for tracking durations and relationships
#[derive(Debug, Clone)]
struct ActiveSpan {
    span_id: String,
    parent_id: Option<String>,
    start_time: DateTime<Utc>,
    name: String,
    kind: Option<String>,
    tags: HashMap<String, String>,
}

/// Span context for maintaining parent-child relationships
#[derive(Debug, Clone)]
struct SpanContext {
    trace_id: String,
    current_span_stack: Vec<String>, // Stack of active span IDs
    active_spans: HashMap<String, ActiveSpan>, // Key is event type (e.g., "WAITING_LLM_START")
    completed_spans: Vec<ZipkinSpan>,
}

impl SpanContext {
    fn new() -> Self {
        // Generate a 32-character hex trace ID (Zipkin standard)
        let trace_id = format!("{:032x}", Uuid::new_v4().as_u128());
        Self {
            trace_id,
            current_span_stack: Vec::new(),
            active_spans: HashMap::new(),
            completed_spans: Vec::new(),
        }
    }

    fn get_current_parent(&self) -> Option<String> {
        self.current_span_stack.last().cloned()
    }

    fn push_span(&mut self, span_id: String) {
        self.current_span_stack.push(span_id);
    }

    fn pop_span(&mut self) -> Option<String> {
        self.current_span_stack.pop()
    }
}

/// Logger for telemetry events
pub struct TelemetryLogger {
    log_file_path: PathBuf,
    zipkin_file_path: PathBuf,
    file_mutex: Arc<Mutex<()>>,
    span_context: Arc<Mutex<SpanContext>>,
}

impl TelemetryLogger {
    /// Create a new telemetry logger for a specific session
    pub fn new_for_session(session_id: &str) -> Result<Self> {
        // Use the same directory structure as session files
        let log_dir = crate::session::ensure_session_dir()?.join("telemetry");

        // Create the telemetry subdirectory if it doesn't exist
        fs::create_dir_all(&log_dir)?;

        // Create telemetry files with the same name as the session
        let log_file_path = log_dir.join(format!("{}.jsonl", session_id));
        let zipkin_file_path = log_dir.join(format!("{}_zipkin.json", session_id));

        Ok(Self {
            log_file_path,
            zipkin_file_path,
            file_mutex: Arc::new(Mutex::new(())),
            span_context: Arc::new(Mutex::new(SpanContext::new())),
        })
    }

    /// Get the path to the log file
    pub fn log_file_path(&self) -> &PathBuf {
        &self.log_file_path
    }

    /// Get the path to the Zipkin file
    pub fn zipkin_file_path(&self) -> &PathBuf {
        &self.zipkin_file_path
    }

    /// Generate a 16-character hex span ID (Zipkin standard)
    fn generate_span_id() -> String {
        format!("{:016x}", rand::random::<u64>())
    }

    /// Create endpoint for local service
    fn local_endpoint() -> ZipkinEndpoint {
        ZipkinEndpoint {
            service_name: "goose".to_string(),
            ipv4: Some("127.0.0.1".to_string()),
            ipv6: None,
            port: Some(8080),
        }
    }

    /// Create endpoint for remote service
    fn remote_endpoint(provider: &str) -> Option<ZipkinEndpoint> {
        if provider.is_empty() || provider == "agent" || provider == "n/a" {
            return None;
        }

        // Extract service name from provider
        let service_name = if provider.starts_with("http") {
            // Extract domain from URL
            provider
                .split('/')
                .nth(2)
                .unwrap_or(provider)
                .split(':')
                .next()
                .unwrap_or(provider)
                .to_string()
        } else {
            provider.to_string()
        };

        Some(ZipkinEndpoint {
            service_name,
            ipv4: None,
            ipv6: None,
            port: None,
        })
    }

    /// Determine span kind based on request type
    fn determine_span_kind(request_type: &str) -> Option<String> {
        match request_type {
            // Client operations (calling external services)
            "complete" | "stream" | "stream_start" | "stream_end" | "api_post" => {
                Some("CLIENT".to_string())
            }
            // Server operations (waiting for input)
            "wait_event" => Some("SERVER".to_string()),
            // Specific waiting events
            t if t.starts_with("WAITING_FOR_USER") => Some("SERVER".to_string()),
            t if t.starts_with("WAITING_LLM") => Some("CLIENT".to_string()),
            t if t.starts_with("WAITING_TOOL") => Some("CLIENT".to_string()),
            _ => None,
        }
    }

    /// Create a meaningful span name
    fn create_span_name(entry: &TelemetryLogEntry) -> String {
        match entry.request_type.as_str() {
            "complete" => format!("LLM Complete: {}", entry.model),
            "stream" | "stream_start" => format!("LLM Stream: {}", entry.model),
            "stream_end" => format!("LLM Stream End: {}", entry.model),
            "api_post" => {
                if entry.provider.starts_with("http") {
                    format!("HTTP POST: {}", entry.provider.split('/').nth(2).unwrap_or(&entry.provider))
                } else {
                    format!("API Call: {}", entry.provider)
                }
            }
            "wait_event" => {
                // Parse the event from the request
                if let Some(event) = entry.request.get("event").and_then(|v| v.as_str()) {
                    match event {
                        "WAITING_FOR_USER_START" => "User Input Start".to_string(),
                        "WAITING_FOR_USER_END" => "User Input Complete".to_string(),
                        "WAITING_LLM_START" => format!("LLM Processing Start: {}", entry.model),
                        "WAITING_LLM_END" => format!("LLM Processing Complete: {}", entry.model),
                        "WAITING_LLM_STREAM_START" => format!("LLM Stream Start: {}", entry.model),
                        "WAITING_LLM_STREAM_END" => format!("LLM Stream Complete: {}", entry.model),
                        "WAITING_LLM_STREAM_CONNECTED" => format!("LLM Stream Connected: {}", entry.model),
                        e if e.starts_with("WAITING_TOOL_START:") => {
                            format!("Tool Start: {}", e.replace("WAITING_TOOL_START:", "").trim())
                        }
                        e if e.starts_with("WAITING_TOOL_END:") => {
                            format!("Tool Complete: {}", e.replace("WAITING_TOOL_END:", "").trim())
                        }
                        _ => event.replace('_', " ").to_lowercase(),
                    }
                } else {
                    "Wait Event".to_string()
                }
            }
            _ => entry.request_type.replace('_', " "),
        }
    }

    /// Extract event type from wait_event request
    fn extract_event_type(entry: &TelemetryLogEntry) -> Option<String> {
        if entry.request_type == "wait_event" {
            entry.request.get("event").and_then(|v| v.as_str()).map(|s| s.to_string())
        } else {
            None
        }
    }

    /// Create tags for a span
    fn create_tags(entry: &TelemetryLogEntry) -> HashMap<String, String> {
        let mut tags = HashMap::new();
        
        tags.insert("request_type".to_string(), entry.request_type.clone());
        tags.insert("provider".to_string(), entry.provider.clone());
        tags.insert("model".to_string(), entry.model.clone());

        // Add request size and preview
        if let Ok(request_str) = serde_json::to_string(&entry.request) {
            tags.insert("request.size".to_string(), request_str.len().to_string());
            let preview = if request_str.len() <= 200 {
                request_str
            } else {
                format!("{}...", &request_str[..197])
            };
            tags.insert("request.preview".to_string(), preview);
        }

        // Add response size if present
        if let Some(ref response) = entry.response {
            if let Ok(response_str) = serde_json::to_string(response) {
                tags.insert("response.size".to_string(), response_str.len().to_string());
            }
        }

        // Add error if present
        if let Some(ref error) = entry.error {
            tags.insert("error".to_string(), error.clone());
            tags.insert("error.kind".to_string(), "true".to_string());
        }

        // Add duration if present
        if let Some(duration_ms) = entry.duration_ms {
            tags.insert("duration_ms".to_string(), duration_ms.to_string());
        }

        tags
    }

    /// Log a telemetry event and handle Zipkin span creation
    pub async fn log(&self, entry: TelemetryLogEntry) -> Result<()> {
        // Write original telemetry log entry
        {
            let _lock = self.file_mutex.lock().await;
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.log_file_path)?;

            let json = serde_json::to_string(&entry)?;
            writeln!(file, "{}", json)?;
            file.flush()?;
        }

        // Handle Zipkin span creation
        let mut context = self.span_context.lock().await;
        let timestamp_micros = entry.timestamp.timestamp_micros() as u64;

        // Extract event type for wait_event entries
        let event_type = Self::extract_event_type(&entry);
        
        // Determine how to handle this event
        match (&entry.request_type[..], event_type.as_deref()) {
            // Handle START events - create span but don't complete it yet
            ("wait_event", Some(event)) if event.ends_with("_START") => {
                let span_id = Self::generate_span_id();
                let parent_id = context.get_current_parent();
                
                // Store active span
                let active = ActiveSpan {
                    span_id: span_id.clone(),
                    parent_id: parent_id.clone(),
                    start_time: entry.timestamp,
                    name: Self::create_span_name(&entry),
                    kind: Self::determine_span_kind(event),
                    tags: Self::create_tags(&entry),
                };
                
                context.active_spans.insert(event.to_string(), active);
                
                // Push onto stack if this creates a new context
                if event.starts_with("WAITING_LLM") || event.starts_with("WAITING_TOOL") {
                    context.push_span(span_id);
                }
            }
            
            // Handle END events - complete the matching START span
            ("wait_event", Some(event)) if event.ends_with("_END") => {
                let start_event = event.replace("_END", "_START");
                
                if let Some(active) = context.active_spans.remove(&start_event) {
                    // Calculate duration
                    let duration_micros = timestamp_micros - active.start_time.timestamp_micros() as u64;
                    
                    // Create annotations
                    let mut annotations = Vec::new();
                    if let Some(ref kind) = active.kind {
                        match kind.as_str() {
                            "CLIENT" => {
                                annotations.push(ZipkinAnnotation {
                                    timestamp: active.start_time.timestamp_micros() as u64,
                                    value: "cs".to_string(),
                                });
                                annotations.push(ZipkinAnnotation {
                                    timestamp: timestamp_micros,
                                    value: "cr".to_string(),
                                });
                            }
                            "SERVER" => {
                                annotations.push(ZipkinAnnotation {
                                    timestamp: active.start_time.timestamp_micros() as u64,
                                    value: "sr".to_string(),
                                });
                                annotations.push(ZipkinAnnotation {
                                    timestamp: timestamp_micros,
                                    value: "ss".to_string(),
                                });
                            }
                            _ => {}
                        }
                    }
                    
                    // Create completed span
                    let span = ZipkinSpan {
                        trace_id: context.trace_id.clone(),
                        id: active.span_id.clone(),
                        parent_id: active.parent_id,
                        name: active.name,
                        timestamp: active.start_time.timestamp_micros() as u64,
                        duration: Some(duration_micros),
                        kind: active.kind,
                        local_endpoint: Self::local_endpoint(),
                        remote_endpoint: Self::remote_endpoint(&entry.provider),
                        annotations,
                        tags: active.tags,
                    };
                    
                    context.completed_spans.push(span);
                    
                    // Pop from stack if this was a context-creating span
                    if start_event.starts_with("WAITING_LLM") || start_event.starts_with("WAITING_TOOL") {
                        context.pop_span();
                    }
                }
            }
            
            // Handle CONNECTED events - add as annotation to active stream span
            ("wait_event", Some("WAITING_LLM_STREAM_CONNECTED")) => {
                if let Some(active) = context.active_spans.get_mut("WAITING_LLM_STREAM_START") {
                    // Add connected timestamp to tags
                    active.tags.insert("stream.connected_at".to_string(), timestamp_micros.to_string());
                }
                // Don't create a separate span
            }
            
            // Handle regular API calls and other events - create immediate spans
            ("api_post", _) | ("complete", _) | ("stream", _) | ("stream_start", _) | ("stream_end", _) => {
                let span_id = Self::generate_span_id();
                let parent_id = context.get_current_parent();
                
                // Calculate duration if provided
                let duration_micros = entry.duration_ms.map(|ms| ms * 1000);
                
                // Create annotations
                let mut annotations = Vec::new();
                let kind = Self::determine_span_kind(&entry.request_type);
                
                if let Some(ref k) = kind {
                    match k.as_str() {
                        "CLIENT" => {
                            annotations.push(ZipkinAnnotation {
                                timestamp: timestamp_micros,
                                value: "cs".to_string(),
                            });
                            if let Some(dur) = duration_micros {
                                annotations.push(ZipkinAnnotation {
                                    timestamp: timestamp_micros + dur,
                                    value: "cr".to_string(),
                                });
                            }
                        }
                        "SERVER" => {
                            annotations.push(ZipkinAnnotation {
                                timestamp: timestamp_micros,
                                value: "sr".to_string(),
                            });
                            if let Some(dur) = duration_micros {
                                annotations.push(ZipkinAnnotation {
                                    timestamp: timestamp_micros + dur,
                                    value: "ss".to_string(),
                                });
                            }
                        }
                        _ => {}
                    }
                }
                
                // Create span
                let span = ZipkinSpan {
                    trace_id: context.trace_id.clone(),
                    id: span_id,
                    parent_id,
                    name: Self::create_span_name(&entry),
                    timestamp: timestamp_micros,
                    duration: duration_micros,
                    kind,
                    local_endpoint: Self::local_endpoint(),
                    remote_endpoint: Self::remote_endpoint(&entry.provider),
                    annotations,
                    tags: Self::create_tags(&entry),
                };
                
                context.completed_spans.push(span);
            }
            
            // Handle other wait events that don't follow START/END pattern
            ("wait_event", Some(_)) | ("wait_event", None) => {
                let span_id = Self::generate_span_id();
                let parent_id = context.get_current_parent();
                
                // Create immediate span for standalone wait events
                let duration_micros = entry.duration_ms.map(|ms| ms * 1000);
                
                let mut annotations = Vec::new();
                let kind = Some("SERVER".to_string());
                
                annotations.push(ZipkinAnnotation {
                    timestamp: timestamp_micros,
                    value: "sr".to_string(),
                });
                if let Some(dur) = duration_micros {
                    annotations.push(ZipkinAnnotation {
                        timestamp: timestamp_micros + dur,
                        value: "ss".to_string(),
                    });
                }
                
                let span = ZipkinSpan {
                    trace_id: context.trace_id.clone(),
                    id: span_id,
                    parent_id,
                    name: Self::create_span_name(&entry),
                    timestamp: timestamp_micros,
                    duration: duration_micros,
                    kind,
                    local_endpoint: Self::local_endpoint(),
                    remote_endpoint: Self::remote_endpoint(&entry.provider),
                    annotations,
                    tags: Self::create_tags(&entry),
                };
                
                context.completed_spans.push(span);
            }
            
            _ => {
                // Unknown event type - create a basic span
                let span_id = Self::generate_span_id();
                let parent_id = context.get_current_parent();
                let duration_micros = entry.duration_ms.map(|ms| ms * 1000);
                
                let span = ZipkinSpan {
                    trace_id: context.trace_id.clone(),
                    id: span_id,
                    parent_id,
                    name: Self::create_span_name(&entry),
                    timestamp: timestamp_micros,
                    duration: duration_micros,
                    kind: None,
                    local_endpoint: Self::local_endpoint(),
                    remote_endpoint: Self::remote_endpoint(&entry.provider),
                    annotations: Vec::new(),
                    tags: Self::create_tags(&entry),
                };
                
                context.completed_spans.push(span);
            }
        }
        
        // Write all completed spans to file
        self.write_zipkin_spans(&context.completed_spans).await?;
        
        Ok(())
    }

    /// Write Zipkin spans to file
    async fn write_zipkin_spans(&self, spans: &[ZipkinSpan]) -> Result<()> {
        if spans.is_empty() {
            return Ok(());
        }
        
        let _lock = self.file_mutex.lock().await;
        
        // Write as JSON array
        let json = serde_json::to_string_pretty(spans)?;
        fs::write(&self.zipkin_file_path, json)?;
        
        Ok(())
    }

    /// Clear the log files
    pub async fn clear(&self) -> Result<()> {
        let _lock = self.file_mutex.lock().await;
        fs::write(&self.log_file_path, "")?;
        fs::write(&self.zipkin_file_path, "[]")?;
        
        // Reset span context
        let mut context = self.span_context.lock().await;
        *context = SpanContext::new();
        
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
    static ref TELEMETRY_LOGGERS: Arc<Mutex<HashMap<String, TelemetryLogger>>> = Arc::new(Mutex::new(HashMap::new()));
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
            zipkin_file_path: l.zipkin_file_path.clone(),
            file_mutex: l.file_mutex.clone(),
            span_context: l.span_context.clone(),
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
        zipkin_file_path: l.zipkin_file_path.clone(),
        file_mutex: l.file_mutex.clone(),
        span_context: l.span_context.clone(),
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
