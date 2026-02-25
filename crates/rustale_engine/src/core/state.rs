use crate::core::logic::profiles::ProfileManager;
use rustale_shared::profiles::ProfilesConfig;
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

/// Default timeouts for different task types (in seconds)
const MOD_INSTALLATION_TIMEOUT_SECS: u64 = 600; // 10 minutes
const GENERIC_IO_TIMEOUT_SECS: u64 = 60; // 1 minute
const DEFAULT_TASK_TIMEOUT_SECS: u64 = 300; // 5 minutes

/// Enum identifying unique task types to prevent duplicates
#[derive(Hash, Eq, PartialEq, Debug, Clone)]
pub enum TaskType {
    GameLaunch,
    ModInstallation(String), // By Mod ID (legacy, kept for compat)
    ModOperation {
        mod_id: String,
        op: String,
    }, // Granular per-entity locking
    ModSearch,
    AppUpdate,
    GenericIO,
    /// Dedicated type for settings persistence so it never collides with
    /// GenericIO tasks (version checks, mod loads, news, etc.).
    /// Without this, a long-running GenericIO task causes `spawn_managed`
    /// to silently discard the settings save, losing user configuration.
    SettingsSave,
}

/// A handle to a supervised task
pub struct SupervisedTask {
    pub handle: JoinHandle<()>,
    pub cancel_token: Arc<AtomicBool>,
    pub created_at: Instant,
}

impl SupervisedTask {
    pub fn cancel(&self) {
        self.cancel_token.store(true, Ordering::Relaxed);
        self.handle.abort();
    }
}

pub struct LogicState {
    pub http_client: rustale_shared::reqwest::Client,
    pub download_client: rustale_shared::reqwest::Client,

    // CHANGED: We wrap handles in a struct for clarity
    pub tasks: HashMap<TaskType, SupervisedTask>,

    // NEW: Synchronous lock to prevent double-clicks before the thread spawns
    pub pending_locks: std::collections::HashSet<TaskType>,

    pub game_process: Option<tokio::process::Child>,
    pub auth_server_stop: Option<tokio::sync::oneshot::Sender<()>>,
    pub security_guard: Option<crate::game::aurora::FileCleanupGuard>,
    /// Set to true by StopGame so that a stale GameProcessReady event arriving
    /// after the stop request kills the child instead of registering it.
    pub stop_requested: bool,
    pub profile_manager: Option<ProfileManager>,
    pub version_service: Option<crate::core::services::version_service::VersionService>,

    // NEW: The Engine maintains the authoritative copy of the settings
    pub settings: rustale_shared::config::GameSettings,

    // NEW: Localization instance for localized strings
    pub localization: rustale_shared::lang::Localization,

    // NEW: Offline mode flag - set when network fails but game is installed
    pub is_offline: bool,
}

impl LogicState {
    pub fn new() -> Self {
        Self {
            http_client: rustale_shared::HTTP_CLIENT.clone(),
            download_client: rustale_shared::HTTP_CLIENT.clone(),
            tasks: HashMap::new(),
            pending_locks: std::collections::HashSet::new(),
            game_process: None,
            auth_server_stop: None,
            security_guard: None,
            stop_requested: false,
            profile_manager: None,
            version_service: None,
            settings: rustale_shared::config::GameSettings::default(),
            localization: rustale_shared::lang::Localization::new(),
            is_offline: false,
        }
    }

    pub fn update_settings(&mut self, new_settings: rustale_shared::config::GameSettings) {
        self.settings = new_settings;
    }

    pub fn with_profiles(profiles: ProfilesConfig) -> Self {
        let mut state = Self::new();
        state.profile_manager = Some(ProfileManager::new(profiles));
        state
    }

    /// Abort a specific task if running
    pub fn cancel_task(&mut self, task_type: TaskType) {
        if let Some(task) = self.tasks.remove(&task_type) {
            task.cancel();
            println!("[Core] Cancelled task: {:?}", task_type);
        }
    }

    /// Abort all tasks (e.g. on exit)
    pub fn cancel_all(&mut self) {
        let tasks: Vec<_> = self.tasks.drain().collect();
        for (ty, task) in tasks {
            task.cancel();
            println!("[Core] Cancelled task: {:?}", ty);
        }

        // Also clear pending locks
        self.pending_locks.clear();
        println!("[Core] Cleared {} pending locks", self.pending_locks.len());
    }

    /// Spawns a managed task with automatic cleanup and timeout protection.
    ///
    /// This method ensures that:
    /// - Only one task of each type can run simultaneously (prevents race conditions)
    /// - Tasks are automatically cleaned up when they complete or timeout
    /// - Panics are caught and logged without crashing the application
    /// - Tasks can be cancelled gracefully using the provided cancellation token
    /// - Pending locks prevent double-spawning during the async gap
    ///
    /// # Arguments
    /// * `task_type` - The type identifier for this task (used for deduplication)
    /// * `f` - An async function that receives a cancellation token and performs the work
    ///
    /// # Example
    /// ```rust
    /// state.spawn_managed(TaskType::GenericIO, |cancel_token| async move {
    ///     while !cancel_token.load(Ordering::Relaxed) {
    ///         // Do some work here
    ///         tokio::time::sleep(Duration::from_millis(100)).await;
    ///     }
    /// });
    /// ```
    ///
    /// # Thread Safety
    /// This method is thread-safe and can be called from any async context.
    /// The spawned task runs in the Tokio runtime and has access to all async APIs.
    pub fn spawn_managed<F, Fut>(&mut self, task_type: TaskType, f: F)
    where
        F: FnOnce(Arc<AtomicBool>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        // Check both running tasks and pending locks to prevent race conditions
        if self.is_task_running(&task_type) {
            println!("[Core] blocked duplicate task: {:?}", task_type);
            return;
        }

        // Lock immediately (Synchronous) - prevents double-clicks during async gap
        self.pending_locks.insert(task_type.clone());

        let cancel_token = Arc::new(AtomicBool::new(false));
        let token_clone = cancel_token.clone();
        let task_type_clone = task_type.clone(); // Clone for use in async block
        let task_type_for_cleanup = task_type.clone(); // Clone for cleanup logic

        let handle = tokio::spawn(async move {
            // Safe guard against panics - catch and log without crashing
            let result = catch_unwind(AssertUnwindSafe(|| {
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async {
                        f(token_clone).await;
                    });
                });
            }));

            if let Err(panic_info) = result {
                eprintln!(
                    "[Core] Critical: Managed Task Panicked! Task type: {:?}",
                    task_type_clone
                );
                // In a production environment, you might want to:
                // - Send panic info to a monitoring system
                // - Trigger a graceful shutdown sequence
                // - Log stack traces for debugging
                if let Some(panic_msg) = panic_info.downcast_ref::<&str>() {
                    eprintln!("[Core] Panic message: {}", panic_msg);
                }
            }
        });

        // Move from pending_locks to active tasks
        self.pending_locks.remove(&task_type_for_cleanup);

        let task_type_for_log = task_type.clone(); // Keep a copy for logging
        self.tasks.insert(
            task_type,
            SupervisedTask {
                handle,
                cancel_token,
                created_at: Instant::now(),
            },
        );

        println!("[Core] Spawned managed task: {:?}", task_type_for_log);
    }

    /// Cleans up finished tasks and zombie tasks that have exceeded their TTL.
    ///
    /// This method should be called periodically to prevent memory leaks and handle stuck tasks.
    ///
    /// # Behavior
    /// - Removes tasks that have completed naturally
    /// - Kills and removes tasks that have exceeded their timeout
    /// - GameLaunch tasks have no timeout (handled by process watchdog)
    /// - ModInstallation tasks timeout after 10 minutes
    /// - GenericIO tasks timeout after 1 minute
    /// - Other tasks timeout after 5 minutes
    ///
    /// # Example
    /// ```rust
    /// // Call this periodically in your main loop
    /// state.cleanup_finished_tasks();
    /// ```
    pub fn cleanup_finished_tasks(&mut self) {
        let now = Instant::now();
        let mut removed_count = 0;
        let mut timeout_count = 0;

        self.tasks.retain(|key, task| {
            // 1. Task completed naturally?
            if task.handle.is_finished() {
                removed_count += 1;
                if let TaskType::ModInstallation(mod_id) = key {
                    println!("[Core] Mod installation task completed for: {}", mod_id);
                } else {
                    println!("[Core] Task completed naturally: {:?}", key);
                }
                return false;
            }

            // 2. Task zombie / timeout policy?
            let timeout = match key {
                TaskType::GameLaunch => {
                    // Game launch has no timeout - managed by process watchdog
                    None
                },
                TaskType::ModInstallation(mod_id) => {
                    // mod_id is used in logging below when timeout occurs
                    let _ = mod_id; // Explicit use to silence warning
                    Some(Duration::from_secs(MOD_INSTALLATION_TIMEOUT_SECS))
                },
                TaskType::GenericIO => {
                    Some(Duration::from_secs(GENERIC_IO_TIMEOUT_SECS))
                },
                TaskType::SettingsSave => {
                    // Settings saves are fast (serialize + atomic file write).
                    // 15 seconds is extremely generous — if it takes longer,
                    // something is very wrong (disk unresponsive, etc.).
                    Some(Duration::from_secs(15))
                },
                _ => {
                    Some(Duration::from_secs(DEFAULT_TASK_TIMEOUT_SECS))
                }
            };

            if let Some(ttl) = timeout {
                let elapsed = now.duration_since(task.created_at);
                if elapsed > ttl {
                    timeout_count += 1;
                    // Mostrar información específica para mods installation
                    match key {
                        TaskType::ModInstallation(mod_id) => {
                            eprintln!("[Core] WATCHDOG: Killing stalled mod installation '{}' after {:?} (elapsed: {:?})",
                                     mod_id, ttl, elapsed);
                        },
                        _ => {
                            eprintln!("[Core] WATCHDOG: Killing stalled task {:?} after {:?} (elapsed: {:?})",
                                     key, ttl, elapsed);
                        }
                    }
                    task.cancel();
                    return false; // Remove task
                }
            }

            true // Keep task
        });

        if removed_count > 0 || timeout_count > 0 {
            println!(
                "[Core] Cleanup completed: {} finished, {} timed out tasks removed",
                removed_count, timeout_count
            );
        }
    }

    /// Returns the number of currently active tasks
    pub fn active_task_count(&self) -> usize {
        self.tasks.len()
    }

    /// Returns a vector of all currently active task types
    pub fn active_tasks(&self) -> Vec<TaskType> {
        self.tasks.keys().cloned().collect()
    }

    /// Checks if a specific task type is currently running
    pub fn is_task_running(&self, task_type: &TaskType) -> bool {
        self.tasks.contains_key(task_type) || self.pending_locks.contains(task_type)
    }

    /// Returns task statistics for monitoring and debugging
    pub fn task_stats(&self) -> TaskStats {
        let now = Instant::now();
        let mut stats = TaskStats::default();

        for (task_type, task) in &self.tasks {
            stats.total_tasks += 1;

            let elapsed = now.duration_since(task.created_at);

            match task_type {
                TaskType::GameLaunch => stats.game_launch_tasks += 1,
                TaskType::ModInstallation(_) => stats.mod_installation_tasks += 1,
                TaskType::ModOperation { .. } => stats.mod_installation_tasks += 1,
                TaskType::ModSearch => stats.mod_search_tasks += 1,
                TaskType::AppUpdate => stats.app_update_tasks += 1,
                TaskType::GenericIO => stats.generic_io_tasks += 1,
                TaskType::SettingsSave => stats.generic_io_tasks += 1,
            }

            if elapsed > Duration::from_secs(60) {
                stats.long_running_tasks += 1;
            }
        }

        stats
    }
}

/// Statistics about currently running tasks
#[derive(Debug, Default)]
pub struct TaskStats {
    pub total_tasks: usize,
    pub game_launch_tasks: usize,
    pub mod_installation_tasks: usize,
    pub mod_search_tasks: usize,
    pub app_update_tasks: usize,
    pub generic_io_tasks: usize,
    pub long_running_tasks: usize, // Tasks running longer than 1 minute
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    use std::time::Duration;
    use tokio::time::sleep;

    // === LogicState Core Tests ===

    #[test]
    fn test_logic_state_new() {
        let state = LogicState::new();
        assert!(state.tasks.is_empty());
        assert!(state.pending_locks.is_empty());
        assert!(state.game_process.is_none());
        assert!(state.auth_server_stop.is_none());
        assert!(!state.stop_requested);
        assert!(state.profile_manager.is_none());
    }

    #[test]
    fn test_logic_state_with_profiles() {
        let profiles = ProfilesConfig::default();

        let state = LogicState::with_profiles(profiles.clone());
        assert!(state.profile_manager.is_some());
    }

    #[test]
    fn test_update_settings() {
        let mut state = LogicState::new();
        let mut settings = rustale_shared::config::GameSettings::default();
        settings.channel = "pre-release".to_string();
        settings.game_version = 42;

        state.update_settings(settings.clone());

        assert_eq!(state.settings.channel, "pre-release");
        assert_eq!(state.settings.game_version, 42);
    }

    // === TaskType Hash Tests - Important for HashMap behavior ===

    #[test]
    fn test_task_type_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(TaskType::GameLaunch);
        set.insert(TaskType::GameLaunch); // Duplicate
        set.insert(TaskType::GenericIO);

        assert_eq!(set.len(), 2);
    }

    // === Task Management Tests ===

    #[test]
    fn test_is_task_running_empty() {
        let state = LogicState::new();
        assert!(!state.is_task_running(&TaskType::GameLaunch));
        assert!(!state.is_task_running(&TaskType::GenericIO));
    }

    #[test]
    fn test_cancel_task_nonexistent() {
        let mut state = LogicState::new();
        // Should not panic
        state.cancel_task(TaskType::GameLaunch);
        assert!(state.tasks.is_empty());
    }

    #[test]
    fn test_cancel_all_empty() {
        let mut state = LogicState::new();
        state.cancel_all();
        assert!(state.tasks.is_empty());
        assert!(state.pending_locks.is_empty());
    }

    #[test]
    fn test_active_task_count() {
        let state = LogicState::new();
        assert_eq!(state.active_task_count(), 0);
    }

    #[test]
    fn test_active_tasks_empty() {
        let state = LogicState::new();
        assert!(state.active_tasks().is_empty());
    }

    #[test]
    fn test_task_stats_empty() {
        let state = LogicState::new();
        let stats = state.task_stats();
        assert_eq!(stats.total_tasks, 0);
        assert_eq!(stats.long_running_tasks, 0);
    }

    // === SupervisedTask Tests ===

    #[tokio::test]
    async fn test_supervised_task_cancel() {
        let cancel_token = Arc::new(AtomicBool::new(false));
        let handle = tokio::spawn(async {
            sleep(Duration::from_secs(10)).await;
        });

        let task = SupervisedTask {
            handle,
            cancel_token: cancel_token.clone(),
            created_at: Instant::now(),
        };

        assert!(!cancel_token.load(Ordering::Relaxed));
        task.cancel();
        assert!(cancel_token.load(Ordering::Relaxed));
    }

    // === Spawn Managed Task Tests ===

    #[tokio::test(flavor = "multi_thread")]
    async fn test_spawn_managed_task() {
        let mut state = LogicState::new();

        state.spawn_managed(TaskType::GenericIO, |cancel_token| async move {
            // Simple task that completes immediately
            assert!(!cancel_token.load(Ordering::Relaxed));
        });

        assert!(state.tasks.contains_key(&TaskType::GenericIO));

        // Wait for task to complete
        sleep(Duration::from_millis(100)).await;

        // Cleanup
        state.cleanup_finished_tasks();
        assert!(!state.tasks.contains_key(&TaskType::GenericIO));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_spawn_managed_prevents_duplicates() {
        let mut state = LogicState::new();

        // Spawn first task
        state.spawn_managed(TaskType::GenericIO, |_token| async move {
            sleep(Duration::from_secs(5)).await;
        });

        assert!(state.tasks.contains_key(&TaskType::GenericIO));
        let initial_count = state.tasks.len();

        // Try to spawn duplicate - should be blocked
        state.spawn_managed(TaskType::GenericIO, |_token| async move {
            // This should not execute
            panic!("Duplicate task should not run");
        });

        assert_eq!(state.tasks.len(), initial_count);

        // Cleanup
        state.cancel_all();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_spawn_managed_with_cancellation() {
        let mut state = LogicState::new();
        let check = Arc::new(AtomicBool::new(false));
        let check_clone = check.clone();

        state.spawn_managed(
            TaskType::ModInstallation("test-mod".to_string()),
            move |token| {
                let check = check_clone.clone();
                async move {
                    // Check cancellation periodically
                    for _ in 0..100 {
                        if token.load(Ordering::Relaxed) {
                            check.store(true, Ordering::Relaxed);
                            return;
                        }
                        sleep(Duration::from_millis(10)).await;
                    }
                }
            },
        );

        // Let task start
        sleep(Duration::from_millis(50)).await;

        // Cancel the task
        state.cancel_task(TaskType::ModInstallation("test-mod".to_string()));

        // Wait for cancellation to propagate
        sleep(Duration::from_millis(50)).await;

        assert!(
            check.load(Ordering::Relaxed),
            "Task should have been cancelled"
        );
    }

    // === Cleanup Tests ===

    #[tokio::test(flavor = "multi_thread")]
    async fn test_cleanup_finished_tasks() {
        let mut state = LogicState::new();

        // Spawn a quick task
        state.spawn_managed(TaskType::GenericIO, |_| async move {
            // Completes immediately
        });

        // Wait for completion
        sleep(Duration::from_millis(100)).await;

        // Cleanup should remove the finished task
        state.cleanup_finished_tasks();

        assert!(state.tasks.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_cleanup_preserves_running_tasks() {
        let mut state = LogicState::new();

        // Spawn a long-running task
        state.spawn_managed(TaskType::GenericIO, |_| async move {
            sleep(Duration::from_secs(10)).await;
        });

        sleep(Duration::from_millis(50)).await;

        // Cleanup should NOT remove the running task
        state.cleanup_finished_tasks();

        assert!(state.tasks.contains_key(&TaskType::GenericIO));

        // Actually cancel for cleanup
        state.cancel_all();
    }

    // === Stop Request Tests ===

    #[test]
    fn test_stop_requested_flag() {
        let mut state = LogicState::new();
        assert!(!state.stop_requested);

        state.stop_requested = true;
        assert!(state.stop_requested);

        state.stop_requested = false;
        assert!(!state.stop_requested);
    }

    // === Pending Locks Tests ===

    #[test]
    fn test_pending_locks() {
        let mut state = LogicState::new();

        state.pending_locks.insert(TaskType::GameLaunch);
        assert!(state.pending_locks.contains(&TaskType::GameLaunch));

        state.pending_locks.remove(&TaskType::GameLaunch);
        assert!(!state.pending_locks.contains(&TaskType::GameLaunch));
    }

    #[test]
    fn test_is_task_running_considers_pending_locks() {
        let mut state = LogicState::new();

        // Add to pending locks
        state.pending_locks.insert(TaskType::AppUpdate);

        // Should be considered running even though not in tasks
        assert!(state.is_task_running(&TaskType::AppUpdate));

        // Cleanup
        state.pending_locks.remove(&TaskType::AppUpdate);
        assert!(!state.is_task_running(&TaskType::AppUpdate));
    }
}
