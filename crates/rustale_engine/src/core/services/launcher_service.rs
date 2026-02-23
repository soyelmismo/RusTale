use crate::core::signals::FromCore;
use crate::core::state::{LogicState, TaskType};
use crate::game::LauncherStatus;
use tokio::sync::mpsc;
use uuid::Uuid;

pub struct LauncherService;

impl LauncherService {
    pub async fn launch(
        state: &mut LogicState,
        tx: mpsc::Sender<FromCore>,
        internal_tx: mpsc::Sender<crate::core::coordinator::CoordinatorEvent>,
        localization: &crate::lang::Localization,
    ) {
        // 1. Validate State — two independent guards:
        //    a) process guard: game is already running
        //    b) task guard: launch is in-flight (between click and GameProcessReady)
        if state.game_process.is_some() {
            let _ = tx.send(FromCore::Error { message: localization.t("common.game_already_running").into(), fatal: false }).await;
            return;
        }
        if state.is_task_running(&TaskType::GameLaunch) {
            println!("[LauncherService] Launch task already in-flight — ignoring duplicate LaunchGame request");
            return;
        }

        // 2. Resolve Data (Engine is authority)
        // A. Resolver Perfil Activo
        let (p_name, p_uuid) = if let Some(mgr) = &state.profile_manager {
            if let Some(profile) = mgr.get_active_profile() {
                (profile.name.clone(), profile.id)
            } else {
                // Fallback crítico si no hay perfil (raro, pero seguro)
                ("Player".to_string(), Uuid::nil())
            }
        } else {
            // Error de estado: Perfiles no cargados
            let _ = tx.send(FromCore::Error { message: localization.t("common.profiles_not_initialized").into(), fatal: true }).await;
            return;
        };

        // B. Obtener Settings actuales (ya sincronizados)
        let settings = state.settings.clone();

        // C. Resolver Version Hint (Lógica de negocio, no de UI)
        let version_hint = if settings.game_version > 0 {
            Some(settings.game_version as i32)
        } else {
            None // Launcher flow resolverá "latest"
        };

        // D. Check offline mode
        let is_offline = state.is_offline;
        if is_offline {
            println!("[LauncherService] Launching in OFFLINE mode - skipping network checks");
        }

        let _ = tx.send(FromCore::StatusChanged(LauncherStatus::Busy)).await;
        
        let tx_clone = tx.clone();
        let internal = internal_tx.clone();
        let client = state.http_client.clone();
        
        // 3. Spawn Task (using state's managed spawner)
        state.spawn_managed(TaskType::GameLaunch, move |cancel_token| async move {
             // Delegates strict heavy logic to private logic modules
             crate::core::logic::launcher::launch_flow(
                tx_clone,
                internal,
                settings,
                p_name,
                p_uuid,
                version_hint,
                cancel_token, // Passed down from registry
                client,
                is_offline,   // Pass offline mode flag
             ).await;
        });
    }
}
