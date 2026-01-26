// src/game/progress.rs

#[derive(Clone, Debug)]
pub struct ProgressStep {
    pub id: &'static str,
    pub weight: f32,
}

#[derive(Clone, Debug)]
pub struct ProgressTracker {
    steps: Vec<ProgressStep>,
    total_weight: f32,
}

impl ProgressTracker {
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            total_weight: 0.0,
        }
    }

    pub fn add_step(mut self, id: &'static str, weight: f32) -> Self {
        self.steps.push(ProgressStep { id, weight });
        self.total_weight += weight;
        self
    }

    /// Calcula el porcentaje global (0.0 a 100.0) basado en la fase actual y su sub-progreso.
    pub fn calculate(&self, current_phase: &str, sub_progress: f32) -> f32 {
        if current_phase == "complete" {
            return 100.0;
        }

        let mut accumulated_weight = 0.0;
        let mut current_step_weight = 0.0;
        let mut found = false;

        // 1. Sumar el peso de todos los pasos ANTERIORES al actual
        for step in &self.steps {
            if step.id == current_phase {
                current_step_weight = step.weight;
                found = true;
                break; // Encontramos la fase actual, dejamos de sumar anteriores
            }
            accumulated_weight += step.weight;
        }

        // Si la fase reportada no está en nuestra lista (ej. un paso nuevo no registrado),
        // devolvemos un cálculo seguro o el ultimo valor conocido.
        if !found {
            // Fallback: si no conocemos el paso, asumimos 0% de progreso extra
            // o podrias retornar sub_progress si quieres comportamiento por defecto.
            return (accumulated_weight / self.total_weight) * 100.0;
        }

        // 2. Calcular progreso parcial dentro del paso actual
        // sub_progress viene de 0 a 100, lo normalizamos a 0..1
        let current_progress_weighted = current_step_weight * (sub_progress / 100.0);

        // 3. Calcular porcentaje total
        let total = accumulated_weight + current_progress_weighted;

        // Regla de tres simple basada en el peso total configurado
        (total / self.total_weight) * 100.0
    }
}
