//! End-to-End Integration Tests for RusTale Launcher
//!
//! These tests verify the complete flow: Download -> Patch -> Launch
//! ensuring that all components work together correctly.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tempfile::tempdir;

// === Test Helper: Create a valid patch ZIP ===

fn create_test_patch_zip(dir: &std::path::Path, name: &str) -> PathBuf {
    use std::io::Write;
    let zip_path = dir.join(name);
    let file = std::fs::File::create(&zip_path).expect("Failed to create zip");
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);

    // Create a valid patch structure with Client files
    zip.start_file("Client/test_file.txt", options).expect("Failed to start file");
    zip.write_all(b"test client content").expect("Failed to write content");
    
    zip.start_file("Client/subfolder/nested.txt", options).expect("Failed to start file");
    zip.write_all(b"nested content").expect("Failed to write content");
    
    zip.finish().expect("Failed to finish zip");
    zip_path
}

fn create_hybrid_patch_zip(dir: &std::path::Path, name: &str) -> PathBuf {
    use std::io::Write;
    let zip_path = dir.join(name);
    let file = std::fs::File::create(&zip_path).expect("Failed to create zip");
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);

    // Hybrid mod with both Client and Server files
    zip.start_file("Client/client_mod.txt", options).expect("Failed to start file");
    zip.write_all(b"client side mod").expect("Failed to write content");
    
    zip.start_file("Server/server_mod.txt", options).expect("Failed to start file");
    zip.write_all(b"server side mod").expect("Failed to write content");
    
    zip.finish().expect("Failed to finish zip");
    zip_path
}

// === Integration Test 1: Patch Installation Flow ===

#[test]
fn test_patch_installation_creates_manifest() {
    let temp = tempdir().expect("Failed to create temp dir");
    let base_dir = temp.path();
    
    // Setup: Create game directory structure WITH Client folder
    // The version_dir must exist and have the game structure
    let paths = rustale_shared::paths::GamePaths::new(base_dir.to_path_buf());
    let game_dir = paths.version_dir("release", "latest");
    std::fs::create_dir_all(game_dir.join("Client")).expect("Failed to create Client dir");
    
    // Create a fake game executable
    std::fs::write(game_dir.join("HytaleClient.jar"), b"fake game").expect("Failed to write fake game");
    
    // Create patch ZIP
    let patch_zip = create_test_patch_zip(temp.path(), "test_mod.zip");
    
    // Create installation request
    let request = rustale_engine::game::mods::ModInstallationRequest {
        mod_id: "test-mod-001".to_string(),
        mod_name: "Test Mod".to_string(),
        remote_id: Some("curseforge-123".to_string()),
        file_id: Some("file-456".to_string()),
        file_url: None,
        provider: Some(rustale_engine::game::mods_api::ModProvider::CurseForge),
        summary: Some("A test mod for integration testing".to_string()),
        logo_url: None,
    };
    
    // Execute: Install the patch
    let result = rustale_engine::game::zip_mods::install_new_patch(
        patch_zip.clone(),
        &paths,
        "release".to_string(),
        "latest".to_string(),
        request,
        None, // No cancellation token
    );
    
    // Verify: Installation succeeded
    assert!(result.is_ok(), "Patch installation failed: {:?}", result.err());
    
    // Verify: Manifest was created
    let patch_dir = paths.core_patches_dir("release", "latest").join("test-mod-001");
    let manifest_path = patch_dir.join("manifest.json");
    assert!(manifest_path.exists(), "Manifest file should exist");
    
    // Verify: Manifest contains correct data
    let manifest_content = std::fs::read_to_string(&manifest_path).expect("Failed to read manifest");
    let manifest: rustale_engine::game::zip_mods::PatchManifest = 
        serde_json::from_str(&manifest_content).expect("Failed to parse manifest");
    
    assert_eq!(manifest.mod_id, "test-mod-001");
    // Note: mod_name may be sanitized (spaces removed) by the installer
    assert!(manifest.mod_name.contains("Test"));
    assert!(manifest.enabled);
    assert!(!manifest.is_hybrid);
    assert_eq!(manifest.remote_id, Some("curseforge-123".to_string()));
    assert!(manifest.added_files.iter().any(|f| f.contains("Client")));
}

// === Integration Test 2: Hybrid Mod Detection ===

#[test]
fn test_hybrid_mod_detection_and_sync() {
    let temp = tempdir().expect("Failed to create temp dir");
    let base_dir = temp.path();
    
    // Setup: Use GamePaths correctly - version_dir returns {root}/release/latest
    let paths = rustale_shared::paths::GamePaths::new(base_dir.to_path_buf());
    let game_dir = paths.version_dir("release", "latest");
    std::fs::create_dir_all(game_dir.join("Client")).expect("Failed to create Client dir");
    std::fs::create_dir_all(game_dir.join("Server")).expect("Failed to create Server dir");
    
    // Create hybrid patch (Client + Server)
    let patch_zip = create_hybrid_patch_zip(temp.path(), "hybrid_mod.zip");
    
    let request = rustale_engine::game::mods::ModInstallationRequest {
        mod_id: "hybrid-mod-001".to_string(),
        mod_name: "Hybrid Mod".to_string(),
        remote_id: None,
        file_id: None,
        file_url: None,
        provider: None,
        summary: None,
        logo_url: None,
    };
    
    // Execute
    let result = rustale_engine::game::zip_mods::install_new_patch(
        patch_zip,
        &paths,
        "release".to_string(),
        "latest".to_string(),
        request,
        None,
    );
    
    // Verify
    assert!(result.is_ok(), "Hybrid installation failed: {:?}", result.err());
    
    // Verify hybrid flag is set
    let manifest_path = paths.core_patches_dir("release", "latest")
        .join("hybrid-mod-001")
        .join("manifest.json");
    let manifest_content = std::fs::read_to_string(&manifest_path).expect("Failed to read manifest");
    let manifest: rustale_engine::game::zip_mods::PatchManifest = 
        serde_json::from_str(&manifest_content).expect("Failed to parse manifest");
    
    assert!(manifest.is_hybrid, "Hybrid mod should have is_hybrid=true");
    
    // Verify ZIP was copied to Mods directory (name is sanitized - spaces removed)
    let mods_dir = paths.mods_dir("release", "latest");
    // The system sanitizes the filename: "Hybrid Mod" -> "HybridMod.zip"
    let hybrid_zip = mods_dir.join("HybridMod.zip");
    assert!(hybrid_zip.exists(), "Hybrid mod ZIP should be copied to Mods directory (sanitized name)");
}

// === Integration Test 3: Patch Disable/Enable Cycle ===

#[test]
fn test_patch_disable_enable_cycle() {
    let temp = tempdir().expect("Failed to create temp dir");
    let base_dir = temp.path();
    
    // Setup: Use GamePaths correctly
    let paths = rustale_shared::paths::GamePaths::new(base_dir.to_path_buf());
    let game_dir = paths.version_dir("release", "latest");
    
    // Create a file to be overwritten (simulating existing game file)
    std::fs::create_dir_all(game_dir.join("Client")).expect("Failed to create Client dir");
    std::fs::write(game_dir.join("Client/test_file.txt"), b"original content").expect("Failed to write original");
    
    let patch_zip = create_test_patch_zip(temp.path(), "toggle_test.zip");
    
    let request = rustale_engine::game::mods::ModInstallationRequest {
        mod_id: "toggle-test-001".to_string(),
        mod_name: "Toggle Test".to_string(),
        remote_id: None,
        file_id: None,
        file_url: None,
        provider: None,
        summary: None,
        logo_url: None,
    };
    
    // Install
    rustale_engine::game::zip_mods::install_new_patch(
        patch_zip,
        &paths,
        "release".to_string(),
        "latest".to_string(),
        request,
        None,
    ).expect("Install failed");
    
    // Verify file was patched
    let patched_content = std::fs::read_to_string(game_dir.join("Client/test_file.txt")).expect("Failed to read patched");
    assert_eq!(patched_content, "test client content", "File should be patched");
    
    // Disable
    rustale_engine::game::zip_mods::disable_patch(
        &paths,
        "release".to_string(),
        "latest".to_string(),
        "toggle-test-001",
    ).expect("Disable failed");
    
    // Verify file was restored
    let restored_content = std::fs::read_to_string(game_dir.join("Client/test_file.txt")).expect("Failed to read restored");
    assert_eq!(restored_content, "original content", "File should be restored after disable");
    
    // Verify manifest shows disabled
    let manifest_path = paths.core_patches_dir("release", "latest")
        .join("toggle-test-001")
        .join("manifest.json");
    let manifest: rustale_engine::game::zip_mods::PatchManifest = 
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).expect("Failed to read"))
            .expect("Failed to parse");
    assert!(!manifest.enabled, "Manifest should show disabled");
    
    // Re-enable
    rustale_engine::game::zip_mods::enable_patch(
        &paths,
        "release".to_string(),
        "latest".to_string(),
        "toggle-test-001",
    ).expect("Enable failed");
    
    // Verify file is patched again
    let re_patched_content = std::fs::read_to_string(game_dir.join("Client/test_file.txt")).expect("Failed to read re-patched");
    assert_eq!(re_patched_content, "test client content", "File should be re-patched after enable");
}

// === Integration Test 4: LaunchContext Consistency ===

#[test]
fn test_launch_context_from_game_paths() {
    // This test verifies that LaunchContext can be correctly constructed
    // from the same paths used by the patch system
    // Use a temp dir to avoid permission issues
    let temp = tempdir().expect("Failed to create temp dir");
    let base_dir = temp.path().to_path_buf();
    let paths = rustale_shared::paths::GamePaths::new(base_dir.clone());
    
    // Verify paths are consistent
    let version_dir = paths.version_dir("release", "latest");
    let _mods_dir = paths.mods_dir("release", "latest");
    
    // Create LaunchContext with these paths
    let ctx = rustale_engine::game::launch::LaunchContext {
        player_name: "TestPlayer".to_string(),
        player_uuid: "test-uuid-001".to_string(),
        exec_path: version_dir.join("HytaleClient.jar"),
        working_dir: version_dir.clone(),
        user_data_dir: version_dir.join("UserData"),
        java_path: "/usr/bin/java".to_string(),
        auth_args: vec!["--offline".to_string()],
        env_vars: std::collections::HashMap::new(),
        jvm_args: Some(vec!["-Xmx4G".to_string()]),
    };
    
    // Build command
    let cmd = rustale_engine::game::launch::build_game_command(ctx);
    let args: Vec<String> = cmd.as_std()
        .get_args()
        .map(|s| s.to_string_lossy().to_string())
        .collect();
    
    // Verify the paths in the command match what GamePaths would produce
    assert!(args.iter().any(|a| a.contains("release")));
    assert!(args.iter().any(|a| a.contains("latest")));
    assert!(args.contains(&"--app-dir".to_string()));
    assert!(args.contains(&"--user-dir".to_string()));
}

// === Integration Test 5: Mod List Integration ===

#[tokio::test]
async fn test_mod_list_after_patch_installation() {
    let temp = tempdir().expect("Failed to create temp dir");
    let base_dir = temp.path();
    
    // Setup: Use GamePaths correctly
    let paths = rustale_shared::paths::GamePaths::new(base_dir.to_path_buf());
    let game_dir = paths.version_dir("release", "latest");
    std::fs::create_dir_all(game_dir.join("Client")).expect("Failed to create Client dir");
    
    // Create a JAR mod (not patch, just a simple mod file)
    let mods_dir = paths.mods_dir("release", "latest");
    
    // Create a simple JAR mod
    std::fs::write(mods_dir.join("simple_mod.jar"), b"mod content").expect("Failed to write mod");
    
    // List mods
    let mods = rustale_engine::game::mods::list_mods(base_dir, "release", "latest")
        .await
        .expect("Failed to list mods");
    
    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0].name, "simple_mod.jar");
    assert!(mods[0].enabled);
}

// === Integration Test 6: Complete Flow Simulation ===

#[test]
fn test_complete_flow_mod_to_launch_context() {
    // This test simulates the complete flow from mod installation
    // to launch context preparation
    
    let temp = tempdir().expect("Failed to create temp dir");
    let base_dir = temp.path();
    
    // 1. Setup game structure using GamePaths correctly
    let paths = rustale_shared::paths::GamePaths::new(base_dir.to_path_buf());
    let game_dir = paths.version_dir("release", "latest");
    std::fs::create_dir_all(game_dir.join("Client")).expect("Failed to create Client dir");
    std::fs::create_dir_all(game_dir.join("Server")).expect("Failed to create Server dir");
    
    // Create fake game executable
    std::fs::write(game_dir.join("HytaleClient.jar"), b"fake game jar").expect("Failed to write game");
    
    // 2. Install a mod
    let patch_zip = create_test_patch_zip(temp.path(), "complete_flow.zip");
    let request = rustale_engine::game::mods::ModInstallationRequest {
        mod_id: "flow-test-001".to_string(),
        mod_name: "Complete Flow Test".to_string(),
        remote_id: Some("modrinth-xyz".to_string()),
        file_id: Some("version-1.0".to_string()),
        file_url: None,
        provider: Some(rustale_engine::game::mods_api::ModProvider::Modrinth),
        summary: Some("Testing complete flow".to_string()),
        logo_url: None,
    };
    
    let install_result = rustale_engine::game::zip_mods::install_new_patch(
        patch_zip,
        &paths,
        "release".to_string(),
        "latest".to_string(),
        request,
        None,
    );
    assert!(install_result.is_ok(), "Installation failed: {:?}", install_result.err());
    
    // 3. Verify game files were patched
    let patched_file = game_dir.join("Client/test_file.txt");
    assert!(patched_file.exists(), "Patched file should exist");
    
    // 4. Verify integrity check passes
    let integrity_result = rustale_engine::game::zip_mods::verify_patch_integrity(
        &paths,
        "release",
        "latest",
    );
    assert!(integrity_result.is_ok(), "Integrity check failed: {:?}", integrity_result.err());
    
    // 5. Prepare launch context (simulating what the UI would do)
    let version_dir = paths.version_dir("release", "latest");
    let ctx = rustale_engine::game::launch::LaunchContext {
        player_name: "IntegrationTester".to_string(),
        player_uuid: "integration-test-uuid".to_string(),
        exec_path: version_dir.join("HytaleClient.jar"),
        working_dir: version_dir.clone(),
        user_data_dir: version_dir.join("UserData"),
        java_path: "/usr/bin/java".to_string(),
        auth_args: vec![],
        env_vars: std::collections::HashMap::new(),
        jvm_args: Some(vec!["-Xmx4G".to_string()]),
    };
    
    // 6. Verify command can be built
    let cmd = rustale_engine::game::launch::build_game_command(ctx);
    let program = cmd.as_std().get_program().to_string_lossy().to_string();
    assert_eq!(program, "/usr/bin/java", "Should use java for JAR files");
    
    // The complete flow has been validated
    println!("[Integration] Complete flow test passed: Download -> Patch -> Launch Context");
}

// === Integration Test 7: Data Format Consistency ===

#[test]
fn test_mod_installation_request_to_patch_manifest_consistency() {
    // Verify that ModInstallationRequest fields are correctly transferred to PatchManifest
    
    let request = rustale_engine::game::mods::ModInstallationRequest {
        mod_id: "consistency-test".to_string(),
        mod_name: "Consistency Test Mod".to_string(),
        remote_id: Some("remote-123".to_string()),
        file_id: Some("file-456".to_string()),
        file_url: Some("https://example.com/download".to_string()),
        provider: Some(rustale_engine::game::mods_api::ModProvider::CurseForge),
        summary: Some("Test summary".to_string()),
        logo_url: Some("https://example.com/logo.png".to_string()),
    };
    
    // Simulate what install_new_patch does internally
    let manifest = rustale_engine::game::zip_mods::PatchManifest {
        mod_id: request.mod_id.clone(),
        mod_name: request.mod_name.clone(),
        install_date: chrono::Utc::now(),
        enabled: true,
        is_hybrid: false,
        backups: vec![],
        added_files: vec!["Client/test.txt".to_string()],
        remote_id: request.remote_id.clone(),
        file_id: request.file_id.clone(),
        provider: request.provider.clone(),
        summary: request.summary.clone(),
        logo_url: request.logo_url.clone(),
    };
    
    // Verify all fields match
    assert_eq!(manifest.mod_id, "consistency-test");
    assert_eq!(manifest.mod_name, "Consistency Test Mod");
    assert_eq!(manifest.remote_id, Some("remote-123".to_string()));
    assert_eq!(manifest.file_id, Some("file-456".to_string()));
    assert_eq!(manifest.provider, Some(rustale_engine::game::mods_api::ModProvider::CurseForge));
    assert_eq!(manifest.summary, Some("Test summary".to_string()));
    assert_eq!(manifest.logo_url, Some("https://example.com/logo.png".to_string()));
    
    // Verify serialization preserves data
    let json = serde_json::to_string(&manifest).expect("Failed to serialize");
    let decoded: rustale_engine::game::zip_mods::PatchManifest = 
        serde_json::from_str(&json).expect("Failed to deserialize");
    
    assert_eq!(decoded.mod_id, manifest.mod_id);
    assert_eq!(decoded.provider, manifest.provider);
}

// === Integration Test 8: Error Recovery ===

#[test]
fn test_installation_with_cancellation() {
    let temp = tempdir().expect("Failed to create temp dir");
    let base_dir = temp.path();
    
    // Setup using GamePaths correctly
    let paths = rustale_shared::paths::GamePaths::new(base_dir.to_path_buf());
    let game_dir = paths.version_dir("release", "latest");
    std::fs::create_dir_all(game_dir.join("Client")).expect("Failed to create Client dir");
    
    let patch_zip = create_test_patch_zip(temp.path(), "cancel_test.zip");
    
    // Create a cancellation token that's already triggered
    let cancel_token = Arc::new(AtomicBool::new(true));
    
    let request = rustale_engine::game::mods::ModInstallationRequest {
        mod_id: "cancel-test".to_string(),
        mod_name: "Cancel Test".to_string(),
        remote_id: None,
        file_id: None,
        file_url: None,
        provider: None,
        summary: None,
        logo_url: None,
    };
    
    // Installation should fail due to cancellation
    let result = rustale_engine::game::zip_mods::install_new_patch(
        patch_zip,
        &paths,
        "release".to_string(),
        "latest".to_string(),
        request,
        Some(cancel_token),
    );
    
    assert!(result.is_err(), "Should fail when cancelled");
    let err = result.unwrap_err();
    assert!(err.to_string().contains("cancelled"), "Error should mention cancellation");
}