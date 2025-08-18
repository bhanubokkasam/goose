#[cfg(test)]
mod tests {
    use crate::agents::agent::Agent;
    use crate::agents::types::SessionConfig;
    use crate::agents::todo_tools::{TODO_READ_TOOL_NAME, TODO_WRITE_TOOL_NAME};
    use crate::session;
    use anyhow::Result;
    use mcp_core::tool::ToolCall;
    use rmcp::model::ErrorCode;
    use serde_json::json;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_todo_session_scoped_persistence() -> Result<()> {
        // Create a temporary directory for test sessions
        let temp_dir = tempdir()?;
        std::env::set_var("GOOSE_SESSION_DIR", temp_dir.path());

        let agent = Agent::new();

        // Create first session
        let session_id_1 = session::generate_session_id();
        let session_1 = SessionConfig {
            id: session::storage::Identifier::Name(session_id_1.clone()),
            working_dir: PathBuf::from("/test/dir1"),
            schedule_id: None,
            execution_mode: None,
            max_turns: None,
            retry_config: None,
        };

        // Write TODO in session 1
        let write_call_1 = ToolCall {
            name: TODO_WRITE_TOOL_NAME.to_string(),
            arguments: json!({
                "content": "Session 1 TODO: Buy milk"
            }),
        };

        let (_, write_result_1) = agent
            .dispatch_todo_tool_with_session(write_call_1, "req1".to_string(), &Some(session_1.clone()))
            .await;

        assert!(write_result_1.is_ok(), "Should write TODO successfully");

        // Read TODO from session 1
        let read_call_1 = ToolCall {
            name: TODO_READ_TOOL_NAME.to_string(),
            arguments: json!({}),
        };

        let (_, read_result_1) = agent
            .dispatch_todo_tool_with_session(read_call_1, "req2".to_string(), &Some(session_1.clone()))
            .await;

        assert!(read_result_1.is_ok());
        let content_1 = read_result_1
            .unwrap()
            .result
            .await
            .unwrap()
            .first()
            .unwrap()
            .as_text()
            .unwrap();
        assert_eq!(content_1, "Session 1 TODO: Buy milk");

        // Create second session
        let session_id_2 = session::generate_session_id();
        let session_2 = SessionConfig {
            id: session::storage::Identifier::Name(session_id_2.clone()),
            working_dir: PathBuf::from("/test/dir2"),
            schedule_id: None,
            execution_mode: None,
            max_turns: None,
            retry_config: None,
        };

        // Write different TODO in session 2
        let write_call_2 = ToolCall {
            name: TODO_WRITE_TOOL_NAME.to_string(),
            arguments: json!({
                "content": "Session 2 TODO: Call dentist"
            }),
        };

        let (_, write_result_2) = agent
            .dispatch_todo_tool_with_session(write_call_2, "req3".to_string(), &Some(session_2.clone()))
            .await;

        assert!(write_result_2.is_ok());

        // Read TODO from session 2 - should be different
        let read_call_2 = ToolCall {
            name: TODO_READ_TOOL_NAME.to_string(),
            arguments: json!({}),
        };

        let (_, read_result_2) = agent
            .dispatch_todo_tool_with_session(read_call_2, "req4".to_string(), &Some(session_2.clone()))
            .await;

        assert!(read_result_2.is_ok());
        let content_2 = read_result_2
            .unwrap()
            .result
            .await
            .unwrap()
            .first()
            .unwrap()
            .as_text()
            .unwrap();
        assert_eq!(content_2, "Session 2 TODO: Call dentist");

        // Read TODO from session 1 again - should still be the original
        let read_call_1_again = ToolCall {
            name: TODO_READ_TOOL_NAME.to_string(),
            arguments: json!({}),
        };

        let (_, read_result_1_again) = agent
            .dispatch_todo_tool_with_session(read_call_1_again, "req5".to_string(), &Some(session_1))
            .await;

        assert!(read_result_1_again.is_ok());
        let content_1_again = read_result_1_again
            .unwrap()
            .result
            .await
            .unwrap()
            .first()
            .unwrap()
            .as_text()
            .unwrap();
        assert_eq!(content_1_again, "Session 1 TODO: Buy milk");

        // Clean up
        std::env::remove_var("GOOSE_SESSION_DIR");

        Ok(())
    }

    #[tokio::test]
    async fn test_todo_without_session_context() -> Result<()> {
        let agent = Agent::new();

        // Try to read TODO without session context
        let read_call = ToolCall {
            name: TODO_READ_TOOL_NAME.to_string(),
            arguments: json!({}),
        };

        let (_, read_result) = agent
            .dispatch_todo_tool_with_session(read_call, "req1".to_string(), &None)
            .await;

        // Should succeed but return empty string
        assert!(read_result.is_ok());
        let content = read_result
            .unwrap()
            .result
            .await
            .unwrap()
            .first()
            .unwrap()
            .as_text()
            .unwrap();
        assert_eq!(content, "");

        // Try to write TODO without session context
        let write_call = ToolCall {
            name: TODO_WRITE_TOOL_NAME.to_string(),
            arguments: json!({
                "content": "Test TODO"
            }),
        };

        let (_, write_result) = agent
            .dispatch_todo_tool_with_session(write_call, "req2".to_string(), &None)
            .await;

        // Should fail with appropriate error
        assert!(write_result.is_err());
        let error = write_result.unwrap_err();
        assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
        assert!(error.message.contains("TODO tools require an active session"));

        Ok(())
    }

    #[tokio::test]
    async fn test_todo_character_limit() -> Result<()> {
        let temp_dir = tempdir()?;
        std::env::set_var("GOOSE_SESSION_DIR", temp_dir.path());

        let agent = Agent::new();

        let session_id = session::generate_session_id();
        let session = SessionConfig {
            id: session::storage::Identifier::Name(session_id.clone()),
            working_dir: PathBuf::from("/test/dir"),
            schedule_id: None,
            execution_mode: None,
            max_turns: None,
            retry_config: None,
        };

        // Set a small character limit for testing
        std::env::set_var("GOOSE_TODO_MAX_CHARS", "20");

        // Try to write TODO that exceeds limit
        let write_call = ToolCall {
            name: TODO_WRITE_TOOL_NAME.to_string(),
            arguments: json!({
                "content": "This is a very long TODO that exceeds the character limit"
            }),
        };

        let (_, write_result) = agent
            .dispatch_todo_tool_with_session(write_call, "req1".to_string(), &Some(session))
            .await;

        // Should fail with character limit error
        assert!(write_result.is_err());
        let error = write_result.unwrap_err();
        assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
        assert!(error.message.contains("Todo list too large"));
        assert!(error.message.contains("max: 20"));

        // Clean up
        std::env::remove_var("GOOSE_TODO_MAX_CHARS");
        std::env::remove_var("GOOSE_SESSION_DIR");

        Ok(())
    }

    #[tokio::test]
    async fn test_todo_persistence_across_agent_instances() -> Result<()> {
        let temp_dir = tempdir()?;
        std::env::set_var("GOOSE_SESSION_DIR", temp_dir.path());

        let session_id = session::generate_session_id();
        let session = SessionConfig {
            id: session::storage::Identifier::Name(session_id.clone()),
            working_dir: PathBuf::from("/test/dir"),
            schedule_id: None,
            execution_mode: None,
            max_turns: None,
            retry_config: None,
        };

        // First agent writes TODO
        {
            let agent1 = Agent::new();
            let write_call = ToolCall {
                name: TODO_WRITE_TOOL_NAME.to_string(),
                arguments: json!({
                    "content": "Persistent TODO"
                }),
            };

            let (_, write_result) = agent1
                .dispatch_todo_tool_with_session(write_call, "req1".to_string(), &Some(session.clone()))
                .await;

            assert!(write_result.is_ok());
        }

        // Second agent reads the same TODO
        {
            let agent2 = Agent::new();
            let read_call = ToolCall {
                name: TODO_READ_TOOL_NAME.to_string(),
                arguments: json!({}),
            };

            let (_, read_result) = agent2
                .dispatch_todo_tool_with_session(read_call, "req2".to_string(), &Some(session))
                .await;

            assert!(read_result.is_ok());
            let content = read_result
                .unwrap()
                .result
                .await
                .unwrap()
                .first()
                .unwrap()
                .as_text()
                .unwrap();
            assert_eq!(content, "Persistent TODO");
        }

        // Clean up
        std::env::remove_var("GOOSE_SESSION_DIR");

        Ok(())
    }
}
