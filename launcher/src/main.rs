#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use mimalloc::MiMalloc;
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use rustale_engine::frontend::FrontendConfig;
#[cfg(feature = "gui")]
use rustale_engine::frontend::FrontendRunner;
use rustale_shared::java;
use single_instance::SingleInstance;

mod cli;

pub fn main() -> std::process::ExitCode {
    // 1. PROXY MODE CHECK (must be first — no allocator warmup needed)
    if java::is_running_as_java_proxy() {
        return cli::run_proxy();
    }

    // ACTIVATE SHIELD IMMEDIATELY
    rustale_shared::init_shield();

    // 2. CLI Parsing
    let args = cli::parse();

    // 3. Early memory optimization (engine-level, frontend-agnostic)
    rustale_engine::util::trim_memory_with_level(rustale_engine::util::TrimLevel::Extreme);

    // 3.5. CLEANUP ORPHANED JAVA PROCESSES
    // If a previous launcher instance crashed, java_original processes may be orphaned.
    // We kill them before checking for single-instance to avoid "Instance already running"
    // when the lock file is stale but no actual launcher is running.
    cleanup_orphaned_java_processes();

    // 4. Single Instance Check
    let lock_name = if args.dedicated_server {
        "RusTaleServer_Lock"
    } else {
        "RusTaleLauncher_Lock"
    };
    let instance = SingleInstance::new(lock_name).unwrap();
    if !instance.is_single() {
        #[cfg(feature = "gui")]
        {
            // Show dialog asking user if they want to force-close the existing instance
            let force_close = show_instance_dialog();
            if force_close {
                println!("[Launcher] Force-closing existing instance...");
                kill_existing_launcher_instance();
                // Wait a moment for cleanup
                std::thread::sleep(std::time::Duration::from_millis(500));
                // Create new instance lock
                let instance = SingleInstance::new(lock_name).unwrap();
                if !instance.is_single() {
                    eprintln!("Failed to acquire lock after force-close. Exiting.");
                    return std::process::ExitCode::FAILURE;
                }
            } else {
                eprintln!("Instance already running. User chose to exit.");
                return std::process::ExitCode::FAILURE;
            }
        }
        #[cfg(not(feature = "gui"))]
        {
            // Headless mode: just exit with error, can't show dialog
            eprintln!("Instance already running. Use --force to kill existing instance.");
            // Check for --force flag
            if std::env::args().any(|a| a == "--force") {
                println!("[Launcher] Force-closing existing instance...");
                kill_existing_launcher_instance();
                std::thread::sleep(std::time::Duration::from_millis(500));
                let instance = SingleInstance::new(lock_name).unwrap();
                if !instance.is_single() {
                    eprintln!("Failed to acquire lock after force-close. Exiting.");
                    return std::process::ExitCode::FAILURE;
                }
            } else {
                return std::process::ExitCode::FAILURE;
            }
        }
    }

    // 5. Mode Selection — dispatch to the appropriate runner
    #[cfg(feature = "gui")]
    if args.dedicated_server {
        run_server_mode(&args)
    } else {
        run_client_mode(&args)
    }

    #[cfg(not(feature = "gui"))]
    {
        // Without GUI, default to dedicated server mode unless explicitly running client
        // (which would fail anyway since there's no frontend)
        if !args.dedicated_server {
            println!("[Headless] No GUI available, running in dedicated server mode.");
            println!("[Headless] Use --help for available options.");
        }
        run_server_mode(&args)
    }
}

/// Runs the interactive client (GUI/TUI/etc).
///
/// To add a new frontend (e.g. egui, TUI, headless CLI):
///   1. Create a crate that implements `FrontendRunner`
///   2. Add a CLI flag (e.g. `--frontend tui`)
///   3. Dispatch here
fn run_client_mode(args: &cli::Args) -> std::process::ExitCode {
    // Platform-specific init that must happen in the binary crate
    #[cfg(windows)]
    {
        if std::env::args().any(|a| a == "--dedicated-server" || a == "--help" || a == "-h") {
            use windows_sys::Win32::System::Console::{
                ATTACH_PARENT_PROCESS, AllocConsole, AttachConsole,
            };
            unsafe {
                if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                    AllocConsole();
                }
            }
        }
    }

    let config = FrontendConfig {
        quickplay: args.quickplay,
        width: 0.0,
        height: 0.0,
    };

    // Dispatch to the active frontend.
    // Each arm is compiled only when its feature flag is enabled.
    // Future: add `#[cfg(feature = "tui")]` arm here for rustale-tui.
    #[cfg(feature = "gui")]
    return rustale_iced::IcedFrontend::new(config).run();

    // If no frontend feature is enabled, exit gracefully with a clear message.
    #[cfg(not(feature = "gui"))]
    {
        let _ = config; // suppress unused warning
        eprintln!(
            "No frontend feature enabled. Rebuild with --features gui (or tui when available)."
        );
        std::process::ExitCode::FAILURE
    }
}

/// Kills orphaned java_original processes that may have been left behind
/// by a previous launcher instance that crashed or was force-killed.
fn cleanup_orphaned_java_processes() {
    use sysinfo::System;

    let mut sys = System::new_all();
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::All,
        true,
        sysinfo::ProcessRefreshKind::everything(),
    );

    let current_pid = std::process::id();
    let mut killed_count = 0;

    for (pid, process) in sys.processes() {
        let name = process.name().to_string_lossy().to_lowercase();

        // Look for java_original processes (orphaned from proxy)
        if name.contains("java_original") || name == "java_original" {
            // Check if parent process exists - if not, it's orphaned
            let is_orphaned = process.parent().map_or(true, |parent_pid| {
                // Check if parent is still running
                !sys.processes().contains_key(&parent_pid)
            });

            // Also kill if the parent is us (shouldn't happen, but safety check)
            let is_our_child = process
                .parent()
                .map_or(false, |p| p.as_u32() == current_pid);

            if is_orphaned || is_our_child {
                println!(
                    "[Cleanup] Killing orphaned java process: PID {} ({})",
                    pid, name
                );
                if process.kill() {
                    killed_count += 1;
                }
            }
        }
    }

    if killed_count > 0 {
        println!(
            "[Cleanup] Killed {} orphaned java process(es)",
            killed_count
        );
    }
}

/// Shows a dialog asking the user if they want to force-close the existing instance.
/// Returns true if the user wants to force-close, false otherwise.
#[cfg(feature = "gui")]
fn show_instance_dialog() -> bool {
    use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};

    // Load localization for dialog messages
    let localization = rustale_shared::lang::Localization::new();

    let result = MessageDialog::new()
        .set_title(localization.t("dialog.instance_exists_title").to_string())
        .set_description(localization.t("dialog.instance_exists_desc").to_string())
        .set_level(MessageLevel::Warning)
        .set_buttons(MessageButtons::YesNo)
        .show();

    matches!(result, MessageDialogResult::Yes)
}

/// Kills any existing RusTale launcher processes (except the current one).
fn kill_existing_launcher_instance() {
    use sysinfo::System;

    let mut sys = System::new_all();
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::All,
        true,
        sysinfo::ProcessRefreshKind::everything(),
    );

    let current_pid = std::process::id();
    let mut killed_count = 0;

    for (pid, process) in sys.processes() {
        let name = process.name().to_string_lossy().to_lowercase();

        // Look for rustale processes (launcher) - exclude current process
        if (name.contains("rustale") || name == "rustale") && pid.as_u32() != current_pid {
            // Also kill any java processes that might be children of the old launcher
            println!(
                "[Force-Close] Killing launcher process: PID {} ({})",
                pid, name
            );
            if process.kill() {
                killed_count += 1;
            }
        }

        // Also kill java_original processes that might be orphaned by the old instance
        if name.contains("java_original") || name == "java" {
            // Check if it's a child of a rustale process
            let is_launcher_child = process.parent().map_or(false, |parent_pid| {
                sys.processes()
                    .get(&parent_pid)
                    .map(|p| {
                        p.name()
                            .to_string_lossy()
                            .to_lowercase()
                            .contains("rustale")
                    })
                    .unwrap_or(false)
            });

            if is_launcher_child {
                println!(
                    "[Force-Close] Killing orphaned java process: PID {} ({})",
                    pid, name
                );
                let _ = process.kill();
            }
        }
    }

    if killed_count > 0 {
        println!("[Force-Close] Killed {} launcher process(es)", killed_count);
    }
}

fn run_server_mode(args: &cli::Args) -> std::process::ExitCode {
    unsafe {
        std::env::set_var("MIMALLOC_ARENA_RESERVE", "0");
        std::env::set_var("MIMALLOC_DECOMMIT_DELAY", "0");
    }
    println!(">>> DEDICATED SERVER mode <<<");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("Runtime failure");

    rt.block_on(async {
        let engine_args = rustale_engine::cli::CliArgs {
            headless: true,
            online_mode: args.online_mode.clone(),
            branch: args.branch.clone(),
            game_version: args.game_version.clone(),
            server_args: args.server_args.clone(),
            java_exec_args: args.java_exec_args.clone(),
            tunnel: args.tunnel.clone(),
            ..Default::default()
        };
        let config = rustale_server::config::load_or_create(&engine_args).await;
        if let Err(e) = rustale_server::runner::run_server_flow(config).await {
            eprintln!("Server Error: {}", e);
            1
        } else {
            0
        }
    })
    .into()
}
