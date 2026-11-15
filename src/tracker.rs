use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;
use chrono::{Local, Timelike};

use crate::db::Database;
use crate::tray::TrayService;
use crate::windows_api::{get_foreground_window_info, get_idle_time_seconds, infer_category};

pub struct TrackerService {
    db: Arc<Database>,
    running: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    session_reset_requested: Arc<AtomicBool>,
    tray: Arc<RwLock<Option<Arc<TrayService>>>>,
}

impl TrackerService {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            running: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(AtomicBool::new(false)),
            session_reset_requested: Arc::new(AtomicBool::new(false)),
            tray: Arc::new(RwLock::new(None)),
        }
    }

    pub fn paused_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.paused)
    }

    pub fn reset_active_session(&self) {
        self.session_reset_requested.store(true, Ordering::SeqCst);
    }

    pub fn set_tray(&self, tray: Arc<TrayService>) {
        if let Ok(mut lock) = self.tray.write() {
            *lock = Some(tray);
        }
    }

    pub fn start(&self) {
        if self.running.swap(true, Ordering::SeqCst) {
            return; // Already running
        }

        let db = Arc::clone(&self.db);
        let running = Arc::clone(&self.running);
        let paused = Arc::clone(&self.paused);
        let session_reset_requested = Arc::clone(&self.session_reset_requested);
        let _tray_ref = Arc::clone(&self.tray);

        thread::spawn(move || {
            let mut current_session_id: Option<i64> = None;
            let mut current_exec_path: Option<String> = None;
            let mut was_idle = false;

            println!("⚡ [ScreenTime] Background Tracker Loop Started.");

            while running.load(Ordering::SeqCst) {
                // Read configuration
                let interval_secs: u64 = db
                    .get_setting("tracking_interval_seconds")
                    .ok()
                    .flatten()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(2);

                let idle_threshold: u64 = db
                    .get_setting("idle_threshold_seconds")
                    .ok()
                    .flatten()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(300);

                let is_paused = paused.load(Ordering::SeqCst)
                    || db
                        .get_setting("tracking_paused")
                        .ok()
                        .flatten()
                        .map(|v| v == "true")
                        .unwrap_or(false);

                if is_paused {
                    thread::sleep(Duration::from_secs(interval_secs));
                    continue;
                }

                if session_reset_requested.swap(false, Ordering::SeqCst) {
                    if let Some(sid) = current_session_id.take() {
                        let _ = db.close_session(sid);
                    }
                    current_exec_path = None;
                    was_idle = false;
                }

                let now = Local::now();
                let today_str = now.format("%Y-%m-%d").to_string();
                let current_hour = now.hour();

                // 1. Check user idle status
                let idle_secs = get_idle_time_seconds();
                let is_idle_now = idle_secs >= idle_threshold;

                if is_idle_now {
                    if !was_idle {
                        // Just became idle: close active app session
                        if let Some(sid) = current_session_id.take() {
                            let _ = db.close_session(sid);
                        }
                        // Start idle session
                        current_session_id = db.start_session("__idle__", "Idle", "User Idle", true).ok();
                        current_exec_path = Some("__idle__".to_string());
                        was_idle = true;
                        println!("🌙 User is now IDLE ({}s inactive)", idle_secs);
                    }

                    // Record idle ticks
                    let _ = db.record_idle_time(&today_str, interval_secs as i64);
                    if let Some(sid) = current_session_id {
                        let _ = db.update_session_seconds(sid, interval_secs as i64);
                    }
                } else {
                    if was_idle {
                        // User returned from idle
                        if let Some(sid) = current_session_id.take() {
                            let _ = db.close_session(sid);
                        }
                        was_idle = false;
                        current_exec_path = None;
                        println!("☀️ User is ACTIVE again");
                    }

                    // 2. Query active foreground window
                    if let Some(win_info) = get_foreground_window_info() {
                        let is_new_app = match &current_exec_path {
                            Some(prev_path) => prev_path != &win_info.executable_path,
                            None => true,
                        };

                        if is_new_app {
                            // Close prior session if open
                            if let Some(sid) = current_session_id.take() {
                                let _ = db.close_session(sid);
                            }

                            // Start new session
                            current_session_id = db.start_session(
                                &win_info.executable_path,
                                &win_info.app_name,
                                &win_info.window_title,
                                false,
                            ).ok();
                            current_exec_path = Some(win_info.executable_path.clone());
                        }

                        let category = infer_category(&win_info.app_name, &win_info.executable_path);

                        // Record foreground seconds
                        let _ = db.record_foreground_usage(
                            &today_str,
                            current_hour,
                            &win_info.executable_path,
                            &win_info.app_name,
                            &win_info.display_name,
                            category,
                            &win_info.window_title,
                            interval_secs as i64,
                            is_new_app,
                        );

                        if let Some(sid) = current_session_id {
                            let _ = db.update_session_seconds(sid, interval_secs as i64);
                        }
                    }
                }

                thread::sleep(Duration::from_secs(interval_secs));
            }

            // Close any lingering session upon shutdown
            if let Some(sid) = current_session_id {
                let _ = db.close_session(sid);
            }
            println!("🛑 [ScreenTime] Background Tracker Loop Terminated.");
        });
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    pub fn toggle_pause(&self) -> bool {
        let current = self.paused.load(Ordering::SeqCst);
        let next = !current;
        self.paused.store(next, Ordering::SeqCst);
        let _ = self.db.set_setting("tracking_paused", if next { "true" } else { "false" });
        next
    }
}
