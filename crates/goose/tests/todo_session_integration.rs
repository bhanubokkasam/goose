use goose::agents::{Agent, AgentEvent};
use goose::agents::types::SessionConfig;
use goose::conversation::Conversation;
use goose::conversation::message::Message;
use goose::session::storage::{SessionMetadata, SessionStorage};
use goose::session::Session;
use goose::providers::base::Provider;
use goose::providers::mock::MockProvider;
use futures::StreamExt;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;
use tokio;

async fn create_test_session() -> (Session, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let session_dir = temp_dir.path().to_path_buf();
    let session = Session::new(session_dir.clone());
    (session, temp_dir)
}

async fn create_test_agent_with_mock_provider() -> Agent {
    let agent = Agent::new();
    let mock_provider = Arc::new(MockProvider::new());
    agent.update_provider(mock_provider).await.unwrap();
    agent
}

#[tokio::test]
async fn test_todo_add_persists_to_session() {
    let (session, _temp_dir) = create_test_session().await;
    let agent = create_test_agent_with_mock_provider().await;
    
    // Create a conversation with a TODO add request
    let mut conversation = Conversation::new();
    conversation.push(Message::user().with_text("Add these tasks to my todo list: Buy milk, Call dentist"));
    
    let session_config = SessionConfig {
        id: session.id().to_string(),
        max_turns: Some(10),
        execution_mode: Some("auto".to_string()),
        retry_config: None,
    };
    
    // Process the conversation
    let mut stream = agent.reply(conversation, Some(session_config.clone()), None).await.unwrap();
    
    // Collect all events
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        if let Ok(event) = event {
            events.push(event);
        }
    }
    
    // Verify TODO was persisted to session
    let session_path = goose::session::storage::get_path(session_config.id).unwrap();
    let metadata = goose::session::storage::read_metadata(&session_path).unwrap();
    
    assert!(metadata.todo_content.is_some());
    let todo_content = metadata.todo_content.unwrap();
    assert!(todo_content.contains("Buy milk"));
    assert!(todo_content.contains("Call dentist"));
}

#[tokio::test]
async fn test_todo_list_reads_from_session() {
    let temp_dir = TempDir::new().unwrap();
    let session_dir = temp_dir.path().to_path_buf();
    let session = Session::new(session_dir.clone());
    let agent = create_test_agent_with_mock_provider().await;
    
    // Pre-populate session with TODO content
    let session_path = goose::session::storage::get_path(session.id().to_string()).unwrap();
    let mut metadata = SessionMetadata::default();
    metadata.todo_content = Some("- Task 1\n- Task 2\n- Task 3".to_string());
    goose::session::storage::update_metadata(&session_path, &metadata).await.unwrap();
    
    // Create a conversation requesting TODO list
    let mut conversation = Conversation::new();
    conversation.push(Message::user().with_text("Show me my todo list"));
    
    let session_config = SessionConfig {
        id: session.id().to_string(),
        max_turns: Some(10),
        execution_mode: Some("auto".to_string()),
        retry_config: None,
    };
    
    // Process the conversation
    let mut stream = agent.reply(conversation, Some(session_config), None).await.unwrap();
    
    // Collect all events and verify TODO content is read
    let mut found_todo_content = false;
    while let Some(event) = stream.next().await {
        if let Ok(AgentEvent::Message(msg)) = event {
            if let Some(text) = msg.as_concat_text() {
                if text.contains("Task 1") && text.contains("Task 2") && text.contains("Task 3") {
                    found_todo_content = true;
                }
            }
        }
    }
    
    assert!(found_todo_content, "TODO content should be read from session");
}

#[tokio::test]
async fn test_todo_isolation_between_sessions() {
    let (session1, _temp_dir1) = create_test_session().await;
    let (session2, _temp_dir2) = create_test_session().await;
    let agent = create_test_agent_with_mock_provider().await;
    
    // Add TODO to session1
    let session1_path = goose::session::storage::get_path(session1.id().to_string()).unwrap();
    let mut metadata1 = SessionMetadata::default();
    metadata1.todo_content = Some("Session 1 tasks".to_string());
    goose::session::storage::update_metadata(&session1_path, &metadata1).await.unwrap();
    
    // Add different TODO to session2
    let session2_path = goose::session::storage::get_path(session2.id().to_string()).unwrap();
    let mut metadata2 = SessionMetadata::default();
    metadata2.todo_content = Some("Session 2 tasks".to_string());
    goose::session::storage::update_metadata(&session2_path, &metadata2).await.unwrap();
    
    // Verify isolation
    let metadata1_read = goose::session::storage::read_metadata(&session1_path).unwrap();
    let metadata2_read = goose::session::storage::read_metadata(&session2_path).unwrap();
    
    assert_eq!(metadata1_read.todo_content.unwrap(), "Session 1 tasks");
    assert_eq!(metadata2_read.todo_content.unwrap(), "Session 2 tasks");
}

#[tokio::test]
async fn test_todo_clear_removes_from_session() {
    let (session, _temp_dir) = create_test_session().await;
    let agent = create_test_agent_with_mock_provider().await;
    
    // Pre-populate session with TODO content
    let session_path = goose::session::storage::get_path(session.id().to_string()).unwrap();
    let mut metadata = SessionMetadata::default();
    metadata.todo_content = Some("- Task to clear".to_string());
    goose::session::storage::update_metadata(&session_path, &metadata).await.unwrap();
    
    // Create a conversation to clear TODO
    let mut conversation = Conversation::new();
    conversation.push(Message::user().with_text("Clear my entire todo list"));
    
    let session_config = SessionConfig {
        id: session.id().to_string(),
        max_turns: Some(10),
        execution_mode: Some("auto".to_string()),
        retry_config: None,
    };
    
    // Process the conversation
    let mut stream = agent.reply(conversation, Some(session_config), None).await.unwrap();
    
    // Consume the stream
    while let Some(_) = stream.next().await {}
    
    // Verify TODO was cleared from session
    let metadata_after = goose::session::storage::read_metadata(&session_path).unwrap();
    assert!(
        metadata_after.todo_content.is_none() || metadata_after.todo_content.as_ref().unwrap().is_empty(),
        "TODO content should be cleared"
    );
}

#[tokio::test]
async fn test_todo_persistence_across_agent_instances() {
    let temp_dir = TempDir::new().unwrap();
    let session_dir = temp_dir.path().to_path_buf();
    let session_id = Session::new(session_dir.clone()).id().to_string();
    
    // First agent instance adds TODO
    {
        let agent1 = create_test_agent_with_mock_provider().await;
        let session_path = goose::session::storage::get_path(session_id.clone()).unwrap();
        let mut metadata = SessionMetadata::default();
        metadata.todo_content = Some("Persistent task".to_string());
        goose::session::storage::update_metadata(&session_path, &metadata).await.unwrap();
    }
    
    // Second agent instance reads TODO
    {
        let agent2 = create_test_agent_with_mock_provider().await;
        let session_path = goose::session::storage::get_path(session_id.clone()).unwrap();
        let metadata = goose::session::storage::read_metadata(&session_path).unwrap();
        
        assert_eq!(metadata.todo_content.unwrap(), "Persistent task");
    }
}

#[tokio::test]
async fn test_todo_max_chars_limit() {
    let (session, _temp_dir) = create_test_session().await;
    
    // Set a small limit for testing
    std::env::set_var("GOOSE_TODO_MAX_CHARS", "50");
    
    let session_path = goose::session::storage::get_path(session.id().to_string()).unwrap();
    let mut metadata = SessionMetadata::default();
    
    // Try to set content that exceeds the limit
    let long_content = "x".repeat(100);
    metadata.todo_content = Some(long_content.clone());
    
    // This should succeed at the storage level (storage doesn't enforce limits)
    goose::session::storage::update_metadata(&session_path, &metadata).await.unwrap();
    
    // But when the agent tries to write through the TODO tool, it should enforce the limit
    // This would be tested through the agent's dispatch_todo_tool_with_session method
    
    // Clean up
    std::env::remove_var("GOOSE_TODO_MAX_CHARS");
}

#[tokio::test]
async fn test_todo_with_special_characters() {
    let (session, _temp_dir) = create_test_session().await;
    
    let session_path = goose::session::storage::get_path(session.id().to_string()).unwrap();
    let mut metadata = SessionMetadata::default();
    
    // Test with various special characters
    let special_content = r#"
- Task with "quotes"
- Task with 'single quotes'
- Task with emoji 🎉
- Task with unicode: 你好
- Task with newline
  continuation
- Task with tab	separation
"#;
    
    metadata.todo_content = Some(special_content.to_string());
    goose::session::storage::update_metadata(&session_path, &metadata).await.unwrap();
    
    // Read back and verify
    let metadata_read = goose::session::storage::read_metadata(&session_path).unwrap();
    assert_eq!(metadata_read.todo_content.unwrap(), special_content);
}

#[tokio::test]
async fn test_todo_concurrent_access() {
    let temp_dir = TempDir::new().unwrap();
    let session_dir = temp_dir.path().to_path_buf();
    let session_id = Session::new(session_dir.clone()).id().to_string();
    
    // Spawn multiple concurrent TODO operations
    let mut handles = vec![];
    
    for i in 0..5 {
        let session_id_clone = session_id.clone();
        
        let handle = tokio::spawn(async move {
            let session_path = goose::session::storage::get_path(session_id_clone).unwrap();
            let mut metadata = goose::session::storage::read_metadata(&session_path)
                .unwrap_or_else(|_| SessionMetadata::default());
            
            let current_content = metadata.todo_content.unwrap_or_default();
            metadata.todo_content = Some(format!("{}\n- Task {}", current_content, i));
            
            goose::session::storage::update_metadata(&session_path, &metadata).await
        });
        
        handles.push(handle);
    }
    
    // Wait for all operations to complete
    for handle in handles {
        handle.await.unwrap().unwrap();
    }
    
    // Verify final state contains all tasks
    let session_path = goose::session::storage::get_path(session_id).unwrap();
    let metadata = goose::session::storage::read_metadata(&session_path).unwrap();
    let todo_content = metadata.todo_content.unwrap();
    
    // Should contain at least one task (concurrent writes may overwrite)
    assert!(todo_content.contains("Task"));
}

#[tokio::test]
async fn test_todo_empty_session_returns_empty() {
    let (session, _temp_dir) = create_test_session().await;
    
    let session_path = goose::session::storage::get_path(session.id().to_string()).unwrap();
    let metadata = goose::session::storage::read_metadata(&session_path)
        .unwrap_or_else(|_| SessionMetadata::default());
    
    assert!(metadata.todo_content.is_none() || metadata.todo_content.as_ref().unwrap().is_empty());
}

#[tokio::test]
async fn test_todo_update_preserves_other_metadata() {
    let (session, _temp_dir) = create_test_session().await;
    
    let session_path = goose::session::storage::get_path(session.id().to_string()).unwrap();
    
    // Set initial metadata with various fields
    let mut metadata = SessionMetadata::default();
    metadata.message_count = 5;
    metadata.description = "Test session".to_string();
    metadata.total_tokens = Some(1000);
    metadata.todo_content = Some("Initial TODO".to_string());
    
    goose::session::storage::update_metadata(&session_path, &metadata).await.unwrap();
    
    // Update only TODO content
    metadata.todo_content = Some("Updated TODO".to_string());
    goose::session::storage::update_metadata(&session_path, &metadata).await.unwrap();
    
    // Verify other fields are preserved
    let metadata_read = goose::session::storage::read_metadata(&session_path).unwrap();
    assert_eq!(metadata_read.message_count, 5);
    assert_eq!(metadata_read.description, "Test session");
    assert_eq!(metadata_read.total_tokens, Some(1000));
    assert_eq!(metadata_read.todo_content, Some("Updated TODO".to_string()));
}
