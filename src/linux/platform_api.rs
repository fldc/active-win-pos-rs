use std::fs::read_link;
use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::mpsc;
use wayland_protocols_async::zwlr_foreign_toplevel_management_v1::handler::{
    ToplevelEvent, ToplevelHandler, ToplevelWState,
};

use crate::{common::platform_api::PlatformApi, ActiveWindow, WindowPosition};

pub struct LinuxPlatformApi {}

impl PlatformApi for LinuxPlatformApi {
    fn get_position(&self) -> Result<WindowPosition, ()> {
        // Position not supported, dummy response
        Ok(WindowPosition {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        })
    }

    fn get_active_window(&self) -> Result<ActiveWindow, ()> {
        if std::env::var("WAYLAND_DISPLAY").is_err() {
            return Err(());
        }

        let rt = tokio::runtime::Runtime::new().map_err(|_| ())?;
        rt.block_on(async { get_active_window_async().await })
    }
}

async fn get_active_window_async() -> Result<ActiveWindow, ()> {
    let (_toplevel_msg_tx, toplevel_msg_rx) = mpsc::channel(128);
    let (toplevel_event_tx, mut toplevel_event_rx) = mpsc::channel(128);

    let mut toplevel_handler = ToplevelHandler::new(toplevel_event_tx);

    let handler_task = tokio::spawn(async move {
        let _ = toplevel_handler.run(toplevel_msg_rx).await;
    });

    let mut active_window_data: Option<(String, String)> = None;

    let timeout = tokio::time::sleep(Duration::from_millis(2000));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            _ = &mut timeout => {
                break;
            }
            event = toplevel_event_rx.recv() => {
                if let Some(event) = event {
                    match event {
                        ToplevelEvent::Done { title, app_id, state: Some(states), .. }
                            if states.contains(&ToplevelWState::Activated) => {
                            active_window_data = Some((title, app_id));
                            break;
                        }
                        ToplevelEvent::Done { .. } => {
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    handler_task.abort();

    if let Some((title, app_id)) = active_window_data {
        let process_id = find_process_by_name(&app_id).unwrap_or(0);

        let process_path = if process_id != 0 {
            read_link(format!("/proc/{process_id}/exe")).unwrap_or_default()
        } else {
            PathBuf::new()
        };

        return Ok(ActiveWindow {
            process_id,
            window_id: format!("wayland-toplevel-{app_id}"),
            app_name: app_id,
            position: WindowPosition {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            title,
            process_path,
        });
    }

    Err(())
}

fn find_process_by_name(app_id: &str) -> Option<u64> {
    use std::fs;

    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            if let Ok(file_name) = entry.file_name().into_string() {
                if let Ok(pid) = file_name.parse::<u64>() {
                    if let Ok(cmdline) = fs::read_to_string(format!("/proc/{pid}/cmdline")) {
                        if cmdline.contains(app_id) {
                            return Some(pid);
                        }
                    }

                    if let Ok(comm) = fs::read_to_string(format!("/proc/{pid}/comm")) {
                        if comm.trim() == app_id {
                            return Some(pid);
                        }
                    }

                    if let Ok(exe_path) = read_link(format!("/proc/{pid}/exe")) {
                        if let Some(exe_name) = exe_path.file_name() {
                            if exe_name.to_string_lossy() == app_id {
                                return Some(pid);
                            }
                        }
                    }
                }
            }
        }
    }

    None
}
