#[cfg(test)]
mod tests {
    use crate::agents::Agent;
    use crate::agents::types::SessionConfig;
    use crate::session::storage::{SessionMetadata, SessionStorage};
    use mcp_core::tool::ToolCall;
    use serde_json::json;
    use std::path::PathBuf;
    use tempfile::TempDir;

    async fn create_test_session_config() -> (SessionConfig, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let session_id = uuid::Uuid::new_v4().to_string();
        
        // Create the session directory structure
        let session_path = temp_dir.path().join(&session_id);
        std::fs::create_dir_all(&session_path).unwrap();
        
        // Initialize metadata file
        let metadata_path = session_path.join("metadata.json");
        let metadata = SessionMetadata::default();
        let metadata_json = serde_json::to_string_pretty(&metadata).unwrap();
        std::fs::write(&metadata_path, metadata_json).unwrap();
        
        // Set the session storage path for this test
        std::env::set_var("GOOSE_SESSION_PATH", temp_dir.path());
        
        let session_config = SessionConfig {
            id: session_id,
            max_turns: Some(10),
            execution_mode: Some("auto".to_string()),
            retry_config: None,
            working_dir: Some(PathBuf::from("/tmp")),
            schedule_id: None,
        };
        
        (session_config, temp_dir)
    }

    #[tokio::test]
    async fn test_todo_write_with_session() {
        let (session_config, _temp_dir) = create_test_session_config().await;
        let agent = Agent::new();
        
        let tool_call = ToolCall {
            name: "todo_write".to_string(),
            arguments: json!({
                "content": "- Buy milk\n- Call dentist"
            }),
        };
        
        let (_, result) = agent
            .dispatch_todo_tool_with_session(tool_call, "test_id".to_string(), &Some(session_config.clone()))
            .await;
        
        assert!(result.is_ok());
        
        // Verify content was written to session
        let session_path = crate::session::storage::get_path(session_config.id).unwrap();
        let metadata = crate::session::storage::read_metadata(&session_path).unwrap();
        assert_eq!(metadata.todo_content, Some("- Buy milk\n- Call dentist".to_string()));
    }

    #[tokio::test]
    async fn test_todo_read_with_session() {
        let (session_config, _temp_dir) = create_test_session_config().await;
        let agent = Agent::new();
        
        // First write some content
        let session_path = crate::session::storage::get_path(session_config.id.clone()).unwrap();
        let mut metadata = crate::session::storage::read_metadata(&session_path).unwrap();
        metadata.todo_content = Some("- Existing task".to_string());
        tokio::task::spawn_blocking({
            let session_path = session_path.clone();
            let metadata = metadata.clone();
            move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(crate::session::storage::update_metadata(&session_path, &metadata))
            }
        }).await.unwrap().unwrap();
        
        // Now read it back
        let tool_call = ToolCall {
            name: "todo_read".to_string(),
            arguments: json!({}),
        };
        
        let (_, result) = agent
            .dispatch_todo_tool_with_session(tool_call, "test_id".to_string(), &Some(session_config))
            .await;
        
        assert!(result.is_ok());
        let result = result.unwrap();
        let content = result.result.await.unwrap();
        assert_eq!(content[0].as_text().unwrap(), "- Existing task");
    }

    #[tokio::test]
    async fn test_todo_without_session_fails() {
        let agent = Agent::new();
        
        let tool_call = ToolCall {
            name: "todo_write".to_string(),
            arguments: json!({
                "content": "Should fail"
            }),
        };
        
        let (_, result) = agent
            .dispatch_todo_tool_with_session(tool_call, "test_id".to_string(), &None)
            .await;
        
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.message.contains("TODO tools require an active session"));
    }

    #[tokio::test]
    async fn test_todo_character_limit() {
        let (session_config, _temp_dir) = create_test_session_config().await;
        let agent = Agent::new();
        
        // Set a small limit
        std::env::set_var("GOOSE_TODO_MAX_CHARS", "10");
        
        let tool_call = ToolCall {
            name: "todo_write".to_string(),
            arguments: json!({
                "content": "This content is way too long for the limit"
            }),
        };
        
        let (_, result) = agent
            .dispatch_todo_tool_with_session(tool_call, "test_id".to_string(), &Some(session_config))
            .await;
        
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.message.contains("too large"));
        
        // Clean up
        std::env::remove_var("GOOSE_TODO_MAX_CHARS");
    }
}
