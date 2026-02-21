/// Maps phase identifiers to localized text
pub fn get_phase_localization_text(phase: &str, localization: &crate::lang::Localization) -> String {
    let key = match phase {
        "download" => "launcher.status.downloading",
        "extract" => "launcher.status.extracting",
        "verify" => "launcher.status.verifying",
        "install" => "launcher.status.installing",
        "prepare" => "launcher.status.preparing",
        "cleanup" => "launcher.status.cleanup",
        "patch" => "launcher.status.patching",
        _ => "launcher.status.working",
    };
    localization.t(key).to_string()
}

/// Formats step progress as "Step X of Y"
pub fn format_step_progress(current_step: Option<usize>, total_steps: Option<usize>) -> String {
    match (current_step, total_steps) {
        (Some(step), Some(total)) => format!("Step {}/{}", step, total),
        (Some(step), None) => format!("Step {}", step),
        // Eliminar el caso "of X steps" que se veia mal
        _ => String::new(),
    }
}
