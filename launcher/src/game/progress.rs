use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

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
    /// 0.0 to 1.0 Global Operation Progress
    pub global_progress: f32,
    /// 0.0 to 1.0 Current Step Progress
    pub step_progress: f32,
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
                t.accumulated_weight = t.phases.iter()
                    .take(pos)
                    .map(|p| p.weight)
                    .sum();
            }
        }
    }

    /// Update progress for current phase
    pub fn report(tracker: &Arc<Mutex<Self>>, step_pct: f32, key: &str, args: Vec<String>, stats: Option<DownloadStats>) {
        if let Ok(t) = tracker.lock() {
            let current_weight = t.phases.get(t.current_phase_index)
                .map(|p| p.weight)
                .unwrap_or(0.0);
            
            // Normalized input 0.0-1.0 or 0.0-100.0 detection? 
            // We assume input is 0.0 to 1.0. If > 1.0, clamp or normalize.
            let step_norm = if step_pct > 1.0 && step_pct <= 100.0 { step_pct / 100.0 } else { step_pct }.clamp(0.0, 1.0);

            let global = (t.accumulated_weight + (step_norm * current_weight)) / t.total_weight;

            let payload = ProgressPayload {
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
    pub fn report_simple(tracker: &Arc<Mutex<Self>>, step_pct: f32, key: &str, stats: Option<DownloadStats>) {
        Self::report(tracker, step_pct, key, vec![], stats)
    }
}
