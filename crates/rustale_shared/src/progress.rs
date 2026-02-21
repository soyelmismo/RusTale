use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

/// Standard signature for progress reporting across the app
/// This encapsulates the thread safety requirements for async operations
pub type ProgressCallback = Arc<dyn Fn(String, f64, String, u64, u64, Option<String>, Option<usize>) + Send + Sync + 'static>;

/// Information about a specific file transfer
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DownloadStats {
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub speed_str: String,
    pub eta_str: Option<String>,
}

/// The standard payload that traverses Backend -> Frontend
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProgressPayload {
    /// 1-indexed current step number
    pub current_step: usize,
    /// Total number of steps in the operation
    pub total_steps: usize,
    /// 0.0 to 1.0 progress within the current step
    pub step_progress: f32,
    /// 0.0 to 1.0 Global Operation Progress (Weighted average)
    pub global_progress: f32,
    /// Translation Key (e.g. "status.downloading")
    pub message_key: String,
    /// Optional arguments for the translation key (e.g. filename)
    pub message_args: Vec<String>,
    /// Download statistics (if downloading)
    pub stats: Option<DownloadStats>,
}

/// A specific phase in the task list
#[derive(Clone)]
pub struct OperationPhase {
    pub id: String,
    pub weight: f32,
}

/// Internal tracker to calculate weighted averages
pub struct WeightedProgressTracker {
    // Use Box<dyn> inside Mutex to allow thread-safe mutable access or simple updates
    reporter: Arc<Box<dyn Fn(ProgressPayload) + Send + Sync>>,
    phases: Vec<OperationPhase>,
    current_phase_index: usize,
    total_weight: f32,
    accumulated_weight: f32,
}

impl WeightedProgressTracker {
    pub fn new(
        reporter: impl Fn(ProgressPayload) + Send + Sync + 'static,
        phases: Vec<OperationPhase>,
    ) -> Arc<Mutex<Self>> {
        let total_weight: f32 = phases.iter().map(|p| p.weight).sum();
        Arc::new(Mutex::new(Self {
            reporter: Arc::new(Box::new(reporter)),
            phases,
            current_phase_index: 0,
            total_weight: total_weight.max(1.0),
            accumulated_weight: 0.0,
        }))
    }

    /// Set current active phase by ID
    pub fn set_phase(tracker: &Arc<Mutex<Self>>, phase_id: &str) {
        if let Ok(mut t) = tracker.lock() {
            if let Some(pos) = t.phases.iter().position(|p| p.id == phase_id) {
                t.current_phase_index = pos;
                // Sum weights of all previous phases
                t.accumulated_weight = t.phases.iter().take(pos).map(|p| p.weight).sum();
            }
        }
    }

    /// Update progress for current phase
    pub fn report(
        tracker: &Arc<Mutex<Self>>,
        step_pct: f32,
        key: &str,
        args: Vec<String>,
        stats: Option<DownloadStats>,
    ) {
        if let Ok(t) = tracker.lock() {
            let current_weight = t
                .phases
                .get(t.current_phase_index)
                .map(|p| p.weight)
                .unwrap_or(0.0);

            // Normalized input 0.0-1.0 or 0.0-100.0 detection?
            // We assume input is 0.0 to 1.0. If > 1.0, clamp or normalize.
            let step_norm = if step_pct > 1.0 && step_pct <= 100.0 {
                step_pct / 100.0
            } else {
                step_pct
            }
            .clamp(0.0, 1.0);

            let global = (t.accumulated_weight + (step_norm * current_weight)) / t.total_weight;

            let payload = ProgressPayload {
                current_step: t.current_phase_index + 1,
                total_steps: t.phases.len(),
                global_progress: global.clamp(0.0, 1.0),
                step_progress: step_norm,
                message_key: key.to_string(),
                message_args: args,
                stats,
            };

            (t.reporter)(payload);
        }
    }

    /// Update progress for current phase without message args (convenience method)
    pub fn report_simple(
        tracker: &Arc<Mutex<Self>>,
        step_pct: f32,
        key: &str,
        stats: Option<DownloadStats>,
    ) {
        Self::report(tracker, step_pct, key, vec![], stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // === Weighted Progress Logic Tests (actual business logic) ===

    #[test]
    fn test_weighted_progress_tracker_creation() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let count_clone = call_count.clone();
        
        let phases = vec![
            OperationPhase { id: "download".to_string(), weight: 0.6 },
            OperationPhase { id: "extract".to_string(), weight: 0.4 },
        ];
        
        let tracker = WeightedProgressTracker::new(
            move |_| {
                count_clone.fetch_add(1, Ordering::SeqCst);
            },
            phases,
        );
        
        assert!(tracker.lock().is_ok());
    }

    #[test]
    fn test_weighted_progress_tracker_report() {
        let last_progress = Arc::new(Mutex::new(0.0f32));
        let last_clone = last_progress.clone();
        
        let phases = vec![
            OperationPhase { id: "download".to_string(), weight: 0.5 },
            OperationPhase { id: "extract".to_string(), weight: 0.5 },
        ];
        
        let tracker = WeightedProgressTracker::new(
            move |payload| {
                *last_clone.lock().unwrap() = payload.global_progress;
            },
            phases,
        );
        
        // Report 50% progress on first phase (weight 0.5)
        WeightedProgressTracker::report(&tracker, 0.5, "test", vec![], None);
        
        let progress = *last_progress.lock().unwrap();
        // Should be 0.25 (0.5 * 0.5) since we're halfway through first phase
        assert!((progress - 0.25).abs() < 0.01);
    }

    #[test]
    fn test_weighted_progress_tracker_set_phase() {
        let last_step = Arc::new(Mutex::new(1usize));
        let last_clone = last_step.clone();
        
        let phases = vec![
            OperationPhase { id: "download".to_string(), weight: 0.5 },
            OperationPhase { id: "extract".to_string(), weight: 0.5 },
        ];
        
        let tracker = WeightedProgressTracker::new(
            move |payload| {
                *last_clone.lock().unwrap() = payload.current_step;
            },
            phases,
        );
        
        // Start at phase 1
        WeightedProgressTracker::report(&tracker, 0.0, "test", vec![], None);
        assert_eq!(*last_step.lock().unwrap(), 1);
        
        // Switch to phase 2
        WeightedProgressTracker::set_phase(&tracker, "extract");
        WeightedProgressTracker::report(&tracker, 0.0, "test", vec![], None);
        assert_eq!(*last_step.lock().unwrap(), 2);
    }

    #[test]
    fn test_weighted_progress_tracker_full_operation() {
        let results = Arc::new(Mutex::new(Vec::new()));
        let results_clone = results.clone();
        
        let phases = vec![
            OperationPhase { id: "download".to_string(), weight: 1.0 },
            OperationPhase { id: "extract".to_string(), weight: 2.0 },
            OperationPhase { id: "verify".to_string(), weight: 0.5 },
        ];
        
        let tracker = WeightedProgressTracker::new(
            move |payload| {
                results_clone.lock().unwrap().push((
                    payload.current_step,
                    payload.global_progress,
                ));
            },
            phases,
        );
        
        // Total weight = 3.5
        // Phase 1 (weight 1.0): 0% -> 100%
        WeightedProgressTracker::report(&tracker, 1.0, "download", vec![], None);
        
        // Phase 2 (weight 2.0): start
        WeightedProgressTracker::set_phase(&tracker, "extract");
        WeightedProgressTracker::report(&tracker, 0.5, "extract", vec![], None);
        
        // Phase 3 (weight 0.5): 100%
        WeightedProgressTracker::set_phase(&tracker, "verify");
        WeightedProgressTracker::report(&tracker, 1.0, "verify", vec![], None);
        
        let results = results.lock().unwrap();
        assert!(results.len() >= 3);
        
        // Final progress should be 1.0 (or very close)
        let final_progress = results.last().unwrap().1;
        assert!((final_progress - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_progress_normalization() {
        // Test that values > 1.0 but <= 100.0 are normalized
        let last_step_progress = Arc::new(Mutex::new(0.0f32));
        let last_clone = last_step_progress.clone();
        
        let phases = vec![
            OperationPhase { id: "test".to_string(), weight: 1.0 },
        ];
        
        let tracker = WeightedProgressTracker::new(
            move |payload| {
                *last_clone.lock().unwrap() = payload.step_progress;
            },
            phases,
        );
        
        // Report 50 as percentage (should be normalized to 0.5)
        WeightedProgressTracker::report(&tracker, 50.0, "test", vec![], None);
        let progress = *last_step_progress.lock().unwrap();
        assert!((progress - 0.5).abs() < 0.01);
    }
}
