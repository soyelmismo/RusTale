use std::collections::HashMap;
use std::path::PathBuf;

pub struct ServerState {
    pub username: String,
    pub uuid: String,
    pub skins: HashMap<String, serde_json::Value>,
    pub game_dir: PathBuf,
    pub last_server_uuid: Option<String>,
}

impl ServerState {
    pub fn new(username: String, uuid: String, game_dir: PathBuf) -> Self {
        Self {
            username,
            uuid,
            skins: HashMap::new(),
            game_dir,
            last_server_uuid: None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use uuid::Uuid;

    // === Concurrency Tests ===
    // CRITICAL: These tests verify thread-safety of ServerState under concurrent access
    // The production code uses Arc<Mutex<ServerState>> which should handle this correctly

    #[tokio::test]
    async fn test_concurrent_skin_updates() {
        // Scenario: Multiple concurrent skin updates for different users
        // Expected: All updates succeed without data loss or corruption
        let state = Arc::new(Mutex::new(ServerState {
            username: "test-user".to_string(),
            uuid: "test-uuid".to_string(),
            skins: HashMap::new(),
            game_dir: std::env::temp_dir(),
            last_server_uuid: None,
        }));

        let mut handles = vec![];

        // Spawn 10 concurrent tasks, each updating a different user's skin
        for i in 0..10 {
            let state_clone = state.clone();
            let handle = tokio::spawn(async move {
                let mut state = state_clone.lock().await;
                let user_uuid = format!("user-{}", i);
                let skin_id = Uuid::new_v4().to_string();
                
                state.skins.insert(
                    user_uuid.clone(),
                    serde_json::json!({
                        "playerSkins": [{
                            "id": skin_id,
                            "name": format!("Skin {}", i),
                            "skin_data": "{}",
                            "created_at": "2024-01-01T00:00:00Z"
                        }],
                        "active_skin": skin_id
                    }),
                );
                
                user_uuid
            });
            handles.push(handle);
        }

        // Wait for all tasks to complete
        let results: Vec<_> = futures::future::join_all(handles).await;

        // Verify all updates succeeded
        let state = state.lock().await;
        assert_eq!(state.skins.len(), 10, "All 10 skin updates should be present");
        
        for result in results {
            let user_uuid = result.expect("Task should complete successfully");
            assert!(state.skins.contains_key(&user_uuid), "User {} should have skin data", user_uuid);
        }
    }

    #[tokio::test]
    async fn test_concurrent_read_write_same_user() {
        // Scenario: Multiple concurrent reads and writes to the same user's skins
        // Expected: No data corruption, reads see consistent state
        let state = Arc::new(Mutex::new(ServerState {
            username: "test-user".to_string(),
            uuid: "test-uuid".to_string(),
            skins: HashMap::new(),
            game_dir: std::env::temp_dir(),
            last_server_uuid: None,
        }));

        // Initial data
        {
            let mut state = state.lock().await;
            state.skins.insert(
                "shared-user".to_string(),
                serde_json::json!({
                    "playerSkins": [{
                        "id": "initial-skin",
                        "name": "Initial",
                        "skin_data": "{}"
                    }],
                    "active_skin": "initial-skin"
                }),
            );
        }

        let mut handles = vec![];

        // Spawn 5 readers and 5 writers
        for i in 0..10 {
            let state_clone = state.clone();
            let handle = if i % 2 == 0 {
                // Writer
                tokio::spawn(async move {
                    let mut state = state_clone.lock().await;
                    let data = state.skins.get_mut("shared-user");
                    if let Some(skin_data) = data {
                        if let Some(skins) = skin_data.get_mut("playerSkins") {
                            if let Some(skins_arr) = skins.as_array_mut() {
                                skins_arr.push(serde_json::json!({
                                    "id": format!("skin-{}", i),
                                    "name": format!("Skin {}", i),
                                    "skin_data": "{}"
                                }));
                            }
                        }
                    }
                    i
                })
            } else {
                // Reader
                tokio::spawn(async move {
                    let state = state_clone.lock().await;
                    // Just read the data
                    let _ = state.skins.get("shared-user").cloned();
                    i
                })
            };
            handles.push(handle);
        }

        // Wait for all
        let results: Vec<_> = futures::future::join_all(handles).await;
        
        // All should complete without panic
        for result in results {
            assert!(result.is_ok(), "All operations should complete without error");
        }

        // Final state should be consistent
        let state = state.lock().await;
        let skin_data = state.skins.get("shared-user").expect("User should exist");
        let skins = skin_data.get("playerSkins").expect("Should have playerSkins");
        let skins_arr = skins.as_array().expect("Should be array");
        
        // Initial + 5 writers = 6 skins minimum (could be more if interleaved correctly)
        assert!(skins_arr.len() >= 6, "Should have at least 6 skins");
    }

    #[tokio::test]
    async fn test_concurrent_token_refresh_simulation() {
        // Scenario: Simulate concurrent token refresh requests
        // This tests the session handling under load
        let state = Arc::new(Mutex::new(ServerState {
            username: "test-user".to_string(),
            uuid: "test-uuid".to_string(),
            skins: HashMap::new(),
            game_dir: std::env::temp_dir(),
            last_server_uuid: None,
        }));

        let mut handles = vec![];

        // Simulate 20 concurrent "token refresh" operations
        for i in 0..20 {
            let state_clone = state.clone();
            let handle = tokio::spawn(async move {
                // Simulate the pattern from session refresh handler:
                // 1. Lock state
                // 2. Read last_server_uuid
                // 3. Possibly update it
                // 4. Release lock
                
                let mut state = state_clone.lock().await;
                let current_uuid = state.last_server_uuid.clone();
                
                // Simulate some processing
                tokio::time::sleep(std::time::Duration::from_micros(100)).await;
                
                // Update if this is a "new" server
                if current_uuid.is_none() || i % 5 == 0 {
                    state.last_server_uuid = Some(format!("server-{}", i));
                }
                
                current_uuid
            });
            handles.push(handle);
        }

        // All should complete
        let results: Vec<_> = futures::future::join_all(handles).await;
        
        for result in results {
            assert!(result.is_ok(), "Token refresh should not fail");
        }

        // Final state should have a valid server_uuid
        let state = state.lock().await;
        assert!(state.last_server_uuid.is_some(), "Should have a server UUID");
    }

    // === Mutex Contention Tests ===

    #[tokio::test]
    async fn test_mutex_does_not_deadlock_under_stress() {
        // Stress test: Many operations under lock contention
        let state = Arc::new(Mutex::new(ServerState {
            username: "stress-test".to_string(),
            uuid: "stress-uuid".to_string(),
            skins: HashMap::new(),
            game_dir: std::env::temp_dir(),
            last_server_uuid: None,
        }));

        let iterations = 100;
        let mut handles = vec![];

        for _ in 0..iterations {
            let state_clone = state.clone();
            
            // Alternate between read-heavy and write-heavy operations
            handles.push(tokio::spawn(async move {
                for _ in 0..10 {
                    let state = state_clone.lock().await;
                    let _ = state.skins.len(); // Read
                    drop(state); // Release lock
                    
                    let mut state = state_clone.lock().await;
                    state.skins.insert(
                        Uuid::new_v4().to_string(),
                        serde_json::json!({"test": "data"}),
                    ); // Write
                    drop(state); // Release lock
                }
            }));
        }

        // Use timeout to detect deadlock
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            futures::future::join_all(handles)
        ).await;

        assert!(result.is_ok(), "Should not deadlock within 10 seconds");
    }

    // === Edge Cases ===

    #[test]
    fn test_server_state_creation() {
        let state = ServerState::new(
            "testuser".to_string(),
            "test-uuid-123".to_string(),
            std::path::PathBuf::from("/tmp/test"),
        );

        assert_eq!(state.username, "testuser");
        assert_eq!(state.uuid, "test-uuid-123");
        assert!(state.skins.is_empty());
        assert!(state.last_server_uuid.is_none());
    }

    #[tokio::test]
    async fn test_large_skin_data_handling() {
        // Test handling of large skin data (base64 images can be ~100KB+)
        let state = Arc::new(Mutex::new(ServerState {
            username: "test".to_string(),
            uuid: "test".to_string(),
            skins: HashMap::new(),
            game_dir: std::env::temp_dir(),
            last_server_uuid: None,
        }));

        // Create a large skin data (simulate base64 encoded image)
        let large_data = "x".repeat(100_000); // 100KB of data
        
        {
            let mut state = state.lock().await;
            state.skins.insert(
                "user-with-large-skin".to_string(),
                serde_json::json!({
                    "playerSkins": [{
                        "id": "large-skin-id",
                        "name": "Large Skin",
                        "skin_data": large_data.clone()
                    }]
                }),
            );
        }

        // Should be able to read it back
        let state = state.lock().await;
        let data = state.skins.get("user-with-large-skin").expect("Should exist");
        let skin_data = data["playerSkins"][0]["skin_data"].as_str().unwrap();
        assert_eq!(skin_data.len(), 100_000);
    }

    #[tokio::test]
    async fn test_unicode_in_usernames() {
        // Test handling of Unicode characters in usernames
        let state = Arc::new(Mutex::new(ServerState {
            username: "用户名🎯".to_string(),
            uuid: "test-uuid".to_string(),
            skins: HashMap::new(),
            game_dir: std::env::temp_dir(),
            last_server_uuid: None,
        }));

        {
            let mut state = state.lock().await;
            state.skins.insert(
                "unicode-user-用户".to_string(),
                serde_json::json!({
                    "playerSkins": [{
                        "id": "skin-id",
                        "name": "皮肤名称 🎨",
                        "skin_data": "{}"
                    }]
                }),
            );
        }

        let state = state.lock().await;
        assert_eq!(state.username, "用户名🎯");
        assert!(state.skins.contains_key("unicode-user-用户"));
        
        let skin_data = state.skins.get("unicode-user-用户").unwrap();
        assert_eq!(skin_data["playerSkins"][0]["name"], "皮肤名称 🎨");
    }
}
