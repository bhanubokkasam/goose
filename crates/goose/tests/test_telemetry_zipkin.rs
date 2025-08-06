use anyhow::Result;
use goose::telemetry_logger::{TelemetryLogEntry, ZipkinSpan};
use std::fs;
use std::path::PathBuf;

#[tokio::test]
async fn test_zipkin_format_output() -> Result<()> {
    // Generate a unique session ID for this test
    let session_id = format!("test_session_{}", uuid::Uuid::new_v4());

    // Initialize the logger for the session
    goose::telemetry_logger::init_telemetry_logger_for_session(session_id.clone()).await?;

    // Create a test telemetry entry
    let entry = TelemetryLogEntry {
        timestamp: chrono::Utc::now(),
        request_type: "complete".to_string(),
        provider: "openai".to_string(),
        model: "gpt-4".to_string(),
        request: serde_json::json!({
            "messages": [
                {"role": "user", "content": "Hello, world!"}
            ]
        }),
        response: Some(serde_json::json!({
            "choices": [
                {"message": {"content": "Hello! How can I help you?"}}
            ]
        })),
        error: None,
        duration_ms: Some(1500),
    };

    // Log the entry
    if let Some(logger) = goose::telemetry_logger::get_telemetry_logger().await {
        logger.log(entry).await?;

        // Verify the Zipkin file was created and contains valid JSON
        let zipkin_path = logger.zipkin_file_path().clone();
        assert!(zipkin_path.exists(), "Zipkin file should exist");

        let zipkin_content = fs::read_to_string(&zipkin_path)?;
        let spans: Vec<ZipkinSpan> = serde_json::from_str(&zipkin_content)?;

        // Should have at least one span
        assert!(!spans.is_empty(), "Should have at least one span");

        // Find our span (the last one with our characteristics)
        let our_span = spans
            .iter()
            .rev()
            .find(|s| s.tags.get("provider") == Some(&"openai".to_string()))
            .expect("Should find our span");

        assert_eq!(our_span.local_endpoint.service_name, "goose");
        assert_eq!(our_span.kind, Some("CLIENT".to_string()));
        assert!(our_span.tags.contains_key("provider"));
        assert!(our_span.tags.contains_key("model"));
        assert!(our_span.tags.contains_key("request_type"));
        assert_eq!(our_span.duration, Some(1500 * 1000)); // Converted to microseconds

        println!("Zipkin span created successfully:");
        println!("{}", serde_json::to_string_pretty(&our_span)?);

        // Clean up the test files
        let _ = fs::remove_file(&zipkin_path);
        let _ = fs::remove_file(logger.log_file_path());
    }

    Ok(())
}

#[tokio::test]
async fn test_wait_event_span_pairing() -> Result<()> {
    // Generate a unique session ID for this test
    let session_id = format!("test_wait_{}", uuid::Uuid::new_v4());

    // Initialize the logger for the session
    goose::telemetry_logger::init_telemetry_logger_for_session(session_id.clone()).await?;

    if let Some(logger) = goose::telemetry_logger::get_telemetry_logger().await {
        // Log a START event
        let start_entry = TelemetryLogEntry {
            timestamp: chrono::Utc::now(),
            request_type: "WAITING_LLM_START".to_string(),
            provider: "agent".to_string(),
            model: "n/a".to_string(),
            request: serde_json::json!({ "event": "WAITING_LLM_START" }),
            response: None,
            error: None,
            duration_ms: None,
        };
        logger.log(start_entry.clone()).await?;

        // Simulate some delay
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Log an END event
        let end_entry = TelemetryLogEntry {
            timestamp: chrono::Utc::now(),
            request_type: "WAITING_LLM_END".to_string(),
            provider: "agent".to_string(),
            model: "n/a".to_string(),
            request: serde_json::json!({ "event": "WAITING_LLM_END" }),
            response: None,
            error: None,
            duration_ms: None,
        };
        logger.log(end_entry).await?;

        // Verify the Zipkin file contains the completed span with duration
        let zipkin_path = logger.zipkin_file_path().clone();
        let zipkin_content = fs::read_to_string(&zipkin_path)?;
        let spans: Vec<ZipkinSpan> = serde_json::from_str(&zipkin_content)?;

        // Should have at least one span
        assert!(!spans.is_empty(), "Should have at least one span");

        // Find the span with duration (the completed one)
        let completed_span = spans
            .iter()
            .find(|s| s.duration.is_some() && s.name.contains("WAITING_LLM"));
        assert!(
            completed_span.is_some(),
            "Should have a completed span with duration"
        );

        if let Some(span) = completed_span {
            assert!(
                span.duration.unwrap() >= 100 * 1000,
                "Duration should be at least 100ms in microseconds"
            );
            println!("Completed wait event span:");
            println!("{}", serde_json::to_string_pretty(&span)?);
        }

        // Clean up the test files
        let _ = fs::remove_file(&zipkin_path);
        let _ = fs::remove_file(logger.log_file_path());
    }

    Ok(())
}
