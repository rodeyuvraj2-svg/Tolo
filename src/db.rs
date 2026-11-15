use std::path::Path;
use std::sync::Mutex;
use chrono::{Local, Timelike};
use rusqlite::{params, Connection, Result};

use crate::models::{AppLimit, AppUsageDaily, CategoryStat, DashboardData, DowntimeSchedule, HourlyStat, PickupsInfo, TodaySummary};

pub struct Database {
    conn: Mutex<Connection>,
}

pub struct HistoryDeleteCounts {
    pub daily_records: usize,
    pub hourly_records: usize,
    pub sessions: usize,
}

impl Database {
    /// Opens or creates the SQLite database and executes migrations.
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Database {
            conn: Mutex::new(conn),
        };
        db.init_schema()?;
        Ok(db)
    }

    /// Initializes all 7 SQLite tables matching the Screen Time schema.
    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS app_usage_daily (
                id INTEGER PRIMARY KEY,
                date TEXT NOT NULL,
                app_name TEXT NOT NULL,
                display_name TEXT DEFAULT '',
                icon_path TEXT,
                executable_path TEXT NOT NULL,
                latest_window_title TEXT DEFAULT '',
                foreground_seconds INTEGER DEFAULT 0,
                background_seconds INTEGER DEFAULT 0,
                idle_seconds INTEGER DEFAULT 0,
                first_seen_at TEXT NOT NULL,
                last_seen_at TEXT NOT NULL,
                session_count INTEGER DEFAULT 0,
                category TEXT DEFAULT 'Uncategorized',
                is_game INTEGER DEFAULT 0,
                UNIQUE(date, executable_path)
            );

            CREATE TABLE IF NOT EXISTS app_sessions (
                id INTEGER PRIMARY KEY,
                executable_path TEXT NOT NULL,
                app_name TEXT NOT NULL,
                window_title TEXT DEFAULT '',
                started_at TEXT NOT NULL,
                ended_at TEXT,
                foreground_seconds INTEGER DEFAULT 0,
                background_seconds INTEGER DEFAULT 0,
                was_idle INTEGER DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS app_catalog (
                executable_path TEXT PRIMARY KEY,
                display_name TEXT DEFAULT '',
                icon_path TEXT,
                category TEXT DEFAULT 'Uncategorized',
                is_game INTEGER DEFAULT 0,
                source TEXT DEFAULT 'auto',
                updated_at TEXT NOT NULL DEFAULT ''
            );

            CREATE TABLE IF NOT EXISTS app_usage_hourly (
                date TEXT NOT NULL,
                hour INTEGER NOT NULL,
                executable_path TEXT NOT NULL,
                foreground_seconds INTEGER DEFAULT 0,
                PRIMARY KEY (date, hour, executable_path)
            );

            CREATE TABLE IF NOT EXISTS app_limits (
                id INTEGER PRIMARY KEY,
                target_type TEXT NOT NULL,
                target_value TEXT NOT NULL,
                daily_minutes INTEGER NOT NULL,
                enabled INTEGER DEFAULT 1
            );

            CREATE TABLE IF NOT EXISTS downtime_schedule (
                id INTEGER PRIMARY KEY,
                start_minute INTEGER NOT NULL,
                end_minute INTEGER NOT NULL,
                days_mask INTEGER NOT NULL,
                enabled INTEGER DEFAULT 1
            );

            CREATE INDEX IF NOT EXISTS idx_daily_date ON app_usage_daily(date);
            CREATE INDEX IF NOT EXISTS idx_hourly_date ON app_usage_hourly(date, hour);
            CREATE INDEX IF NOT EXISTS idx_sessions_started ON app_sessions(started_at);
            "#,
        )?;

        // Seed default settings if empty
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM settings",
            [],
            |r| r.get(0),
        )?;

        if count == 0 {
            conn.execute_batch(
                r#"
                INSERT INTO settings (key, value) VALUES
                    ('tracking_paused', 'false'),
                    ('idle_threshold_seconds', '300'),
                    ('tracking_interval_seconds', '2'),
                    ('background_scan_seconds', '10'),
                    ('daily_goal_minutes', '0'),
                    ('soft_limit_enabled', 'true'),
                    ('theme', 'light');
                "#,
            )?;
        }

        Ok(())
    }

    /// Retrieves a setting value by key.
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// Sets or updates a setting.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )?;
        Ok(())
    }

    /// Deletes usage history within an inclusive date range, or all history when both dates are absent.
    pub fn delete_history(
        &self,
        start_date: Option<&str>,
        end_date: Option<&str>,
    ) -> Result<HistoryDeleteCounts> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;

        let (daily_records, hourly_records, sessions) = match (start_date, end_date) {
            (Some(start), Some(end)) => (
                tx.execute(
                    "DELETE FROM app_usage_daily WHERE date >= ?1 AND date <= ?2",
                    params![start, end],
                )?,
                tx.execute(
                    "DELETE FROM app_usage_hourly WHERE date >= ?1 AND date <= ?2",
                    params![start, end],
                )?,
                tx.execute(
                    "DELETE FROM app_sessions WHERE substr(started_at, 1, 10) >= ?1 AND substr(started_at, 1, 10) <= ?2",
                    params![start, end],
                )?,
            ),
            (None, None) => (
                tx.execute("DELETE FROM app_usage_daily", [])?,
                tx.execute("DELETE FROM app_usage_hourly", [])?,
                tx.execute("DELETE FROM app_sessions", [])?,
            ),
            _ => return Err(rusqlite::Error::InvalidParameterName("date range".to_string())),
        };

        tx.commit()?;
        Ok(HistoryDeleteCounts {
            daily_records,
            hourly_records,
            sessions,
        })
    }

    /// Records foreground time for an application.
    pub fn record_foreground_usage(
        &self,
        date: &str,
        hour: u32,
        exec_path: &str,
        app_name: &str,
        display_name: &str,
        category: &str,
        window_title: &str,
        seconds: i64,
        is_new_session: bool,
    ) -> Result<()> {
        let now = Local::now().to_rfc3339();
        let session_inc = if is_new_session { 1 } else { 0 };
        let conn = self.conn.lock().unwrap();

        // 1. Upsert into app_usage_daily
        conn.execute(
            r#"
            INSERT INTO app_usage_daily (
                date, app_name, display_name, icon_path, executable_path, latest_window_title,
                foreground_seconds, background_seconds, idle_seconds,
                first_seen_at, last_seen_at, session_count, category, is_game
            ) VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, 0, 0, ?7, ?7, ?8, ?9, 0)
            ON CONFLICT(date, executable_path) DO UPDATE SET
                foreground_seconds = foreground_seconds + excluded.foreground_seconds,
                category = CASE WHEN app_usage_daily.category = 'Uncategorized' THEN excluded.category ELSE app_usage_daily.category END,
                latest_window_title = CASE WHEN excluded.latest_window_title != '' THEN excluded.latest_window_title ELSE app_usage_daily.latest_window_title END,
                last_seen_at = excluded.last_seen_at,
                session_count = session_count + excluded.session_count;
            "#,
            params![date, app_name, display_name, exec_path, window_title, seconds, now, session_inc, category],
        )?;

        // 2. Upsert into app_usage_hourly
        conn.execute(
            r#"
            INSERT INTO app_usage_hourly (date, hour, executable_path, foreground_seconds)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(date, hour, executable_path) DO UPDATE SET
                foreground_seconds = foreground_seconds + excluded.foreground_seconds;
            "#,
            params![date, hour, exec_path, seconds],
        )?;

        // 3. Upsert into app_catalog
        conn.execute(
            r#"
            INSERT INTO app_catalog (executable_path, display_name, icon_path, category, is_game, source, updated_at)
            VALUES (?1, ?2, NULL, ?3, 0, 'auto', ?4)
            ON CONFLICT(executable_path) DO UPDATE SET
                display_name = CASE WHEN app_catalog.display_name = '' THEN excluded.display_name ELSE app_catalog.display_name END,
                category = CASE WHEN app_catalog.category = 'Uncategorized' THEN excluded.category ELSE app_catalog.category END,
                updated_at = excluded.updated_at;
            "#,
            params![exec_path, display_name, category, now],
        )?;

        Ok(())
    }

    /// Records idle time for a given day.
    pub fn record_idle_time(&self, date: &str, seconds: i64) -> Result<()> {
        let now = Local::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();

        conn.execute(
            r#"
            INSERT INTO app_usage_daily (
                date, app_name, display_name, icon_path, executable_path, latest_window_title,
                foreground_seconds, background_seconds, idle_seconds,
                first_seen_at, last_seen_at, session_count, category, is_game
            ) VALUES (?1, '__idle__', 'User Idle', NULL, '__idle__', 'Idle', 0, 0, ?2, ?3, ?3, 0, 'System', 0)
            ON CONFLICT(date, executable_path) DO UPDATE SET
                idle_seconds = idle_seconds + excluded.idle_seconds,
                last_seen_at = excluded.last_seen_at;
            "#,
            params![date, seconds, now],
        )?;

        Ok(())
    }

    /// Starts a session in app_sessions and returns its ID.
    pub fn start_session(&self, exec_path: &str, app_name: &str, window_title: &str, was_idle: bool) -> Result<i64> {
        let now = Local::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();

        conn.execute(
            r#"
            INSERT INTO app_sessions (executable_path, app_name, window_title, started_at, ended_at, foreground_seconds, background_seconds, was_idle)
            VALUES (?1, ?2, ?3, ?4, NULL, 0, 0, ?5)
            "#,
            params![exec_path, app_name, window_title, now, if was_idle { 1 } else { 0 }],
        )?;

        Ok(conn.last_insert_rowid())
    }

    /// Updates foreground duration for an active session.
    pub fn update_session_seconds(&self, session_id: i64, seconds: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE app_sessions SET foreground_seconds = foreground_seconds + ?1 WHERE id = ?2",
            params![seconds, session_id],
        )?;
        Ok(())
    }

    /// Closes an active session.
    pub fn close_session(&self, session_id: i64) -> Result<()> {
        let now = Local::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE app_sessions SET ended_at = ?1 WHERE id = ?2 AND ended_at IS NULL",
            params![now, session_id],
        )?;
        Ok(())
    }

    /// Fetches all configured limits (both enabled and disabled).
    #[allow(dead_code)]
    pub fn get_limits(&self) -> Result<Vec<AppLimit>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, target_type, target_value, daily_minutes, enabled FROM app_limits ORDER BY id DESC")?;
        let rows = stmt.query_map([], |row| {
            Ok(AppLimit {
                id: Some(row.get(0)?),
                target_type: row.get(1)?,
                target_value: row.get(2)?,
                daily_minutes: row.get(3)?,
                enabled: row.get::<_, i64>(4)? == 1,
            })
        })?;

        let mut limits = Vec::new();
        for r in rows {
            limits.push(r?);
        }
        Ok(limits)
    }

    /// Adds or updates an app/category limit.
    #[allow(dead_code)]
    pub fn add_limit(&self, target_type: &str, target_value: &str, daily_minutes: i64) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute("DELETE FROM app_limits WHERE target_type = ?1 AND target_value = ?2", params![target_type, target_value]);
        conn.execute(
            "INSERT INTO app_limits (target_type, target_value, daily_minutes, enabled) VALUES (?1, ?2, ?3, 1)",
            params![target_type, target_value, daily_minutes],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Deletes a limit by ID.
    #[allow(dead_code)]
    pub fn delete_limit(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM app_limits WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Toggles a limit's enabled state.
    #[allow(dead_code)]
    pub fn toggle_limit(&self, id: i64) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let current_state: i64 = conn.query_row(
            "SELECT enabled FROM app_limits WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        let new_state = if current_state == 1 { 0 } else { 1 };
        conn.execute("UPDATE app_limits SET enabled = ?1 WHERE id = ?2", params![new_state, id])?;
        Ok(new_state == 1)
    }

    /// Checks if a limit is exceeded for a given app. Returns Some(minutes_limit) if exceeded.
    #[allow(dead_code)]
    pub fn check_limit_exceeded(&self, date: &str, exec_path: &str, category: &str) -> Result<Option<(String, i64, i64)>> {
        let conn = self.conn.lock().unwrap();

        // 1. Get current foreground seconds for app
        let app_seconds: i64 = conn.query_row(
            "SELECT COALESCE(foreground_seconds, 0) FROM app_usage_daily WHERE date = ?1 AND executable_path = ?2",
            params![date, exec_path],
            |r| r.get(0),
        ).unwrap_or(0);

        // 2. Check per-app limits
        let mut stmt = conn.prepare(
            "SELECT daily_minutes FROM app_limits WHERE enabled = 1 AND target_type = 'app' AND target_value = ?1",
        )?;
        let mut rows = stmt.query(params![exec_path])?;
        if let Some(row) = rows.next()? {
            let limit_min: i64 = row.get(0)?;
            let limit_sec = limit_min * 60;
            if app_seconds >= limit_sec {
                return Ok(Some((exec_path.to_string(), app_seconds / 60, limit_min)));
            }
        }

        // 3. Check per-category limits
        if !category.is_empty() && category != "Uncategorized" {
            let cat_seconds: i64 = conn.query_row(
                "SELECT COALESCE(SUM(foreground_seconds), 0) FROM app_usage_daily WHERE date = ?1 AND category = ?2",
                params![date, category],
                |r| r.get(0),
            ).unwrap_or(0);

            let mut cat_stmt = conn.prepare(
                "SELECT daily_minutes FROM app_limits WHERE enabled = 1 AND target_type = 'category' AND target_value = ?1",
            )?;
            let mut cat_rows = cat_stmt.query(params![category])?;
            if let Some(row) = cat_rows.next()? {
                let limit_min: i64 = row.get(0)?;
                let limit_sec = limit_min * 60;
                if cat_seconds >= limit_sec {
                    return Ok(Some((category.to_string(), cat_seconds / 60, limit_min)));
                }
            }
        }

        Ok(None)
    }

    /// Fetches all downtime schedules.
    #[allow(dead_code)]
    pub fn get_downtime_schedules(&self) -> Result<Vec<DowntimeSchedule>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, start_minute, end_minute, days_mask, enabled FROM downtime_schedule WHERE enabled = 1")?;
        let rows = stmt.query_map([], |row| {
            Ok(DowntimeSchedule {
                id: Some(row.get(0)?),
                start_minute: row.get(1)?,
                end_minute: row.get(2)?,
                days_mask: row.get(3)?,
                enabled: row.get::<_, i64>(4)? == 1,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    /// Produces today's full summary report.
    #[allow(dead_code)]
    pub fn get_today_summary(&self, date: &str) -> Result<TodaySummary> {
        let conn = self.conn.lock().unwrap();

        let total_foreground: i64 = conn.query_row(
            "SELECT COALESCE(SUM(foreground_seconds), 0) FROM app_usage_daily WHERE date = ?1 AND executable_path != '__idle__'",
            params![date],
            |r| r.get(0),
        ).unwrap_or(0);

        let total_idle: i64 = conn.query_row(
            "SELECT COALESCE(idle_seconds, 0) FROM app_usage_daily WHERE date = ?1 AND executable_path = '__idle__'",
            params![date],
            |r| r.get(0),
        ).unwrap_or(0);

        let mut stmt = conn.prepare(
            r#"
            SELECT id, date, app_name, display_name, icon_path, executable_path,
                   latest_window_title, foreground_seconds, background_seconds, idle_seconds,
                   first_seen_at, last_seen_at, session_count, category, is_game
            FROM app_usage_daily
            WHERE date = ?1 AND executable_path != '__idle__' AND foreground_seconds > 0
            ORDER BY foreground_seconds DESC
            LIMIT 15
            "#
        )?;

        let top_apps_iter = stmt.query_map(params![date], |row| {
            Ok(AppUsageDaily {
                id: Some(row.get(0)?),
                date: row.get(1)?,
                app_name: row.get(2)?,
                display_name: row.get(3)?,
                icon_path: row.get(4)?,
                executable_path: row.get(5)?,
                latest_window_title: row.get(6)?,
                foreground_seconds: row.get(7)?,
                background_seconds: row.get(8)?,
                idle_seconds: row.get(9)?,
                first_seen_at: row.get(10)?,
                last_seen_at: row.get(11)?,
                session_count: row.get(12)?,
                category: row.get(13)?,
                is_game: row.get(14)?,
            })
        })?;

        let mut top_apps = Vec::new();
        for app in top_apps_iter {
            let mut a = app?;
            if a.icon_path.is_none() {
                a.icon_path = crate::icon_extractor::get_app_icon_data_uri(&a.executable_path)
                    .or_else(|| crate::icon_extractor::get_app_icon_data_uri(&a.app_name))
                    .or_else(|| crate::icon_extractor::get_app_icon_data_uri(&a.display_name));
            }
            top_apps.push(a);
        }

        let app_count = top_apps.len();

        Ok(TodaySummary {
            date: date.to_string(),
            total_foreground_seconds: total_foreground,
            total_idle_seconds: total_idle,
            app_count,
            top_apps,
        })
    }

    /// Fetches all rich dashboard data matching the ScreenTime visual interface.
    pub fn get_dashboard_data(&self, date: &str, is_paused: bool) -> Result<DashboardData> {
        let conn = self.conn.lock().unwrap();

        // 1. Total foreground and idle
        let total_foreground: i64 = conn.query_row(
            "SELECT COALESCE(SUM(foreground_seconds), 0) FROM app_usage_daily WHERE date = ?1 AND executable_path != '__idle__'",
            params![date],
            |r| r.get(0),
        ).unwrap_or(0);

        let total_idle: i64 = conn.query_row(
            "SELECT COALESCE(idle_seconds, 0) FROM app_usage_daily WHERE date = ?1 AND executable_path = '__idle__'",
            params![date],
            |r| r.get(0),
        ).unwrap_or(0);

        // 2. Categories breakdown
        let mut cat_stmt = conn.prepare(
            r#"
            SELECT COALESCE(NULLIF(category, ''), 'Uncategorized') as cat, SUM(foreground_seconds) as sec
            FROM app_usage_daily
            WHERE date = ?1 AND executable_path != '__idle__' AND foreground_seconds > 0
            GROUP BY cat
            ORDER BY sec DESC
            "#
        )?;

        let categories_iter = cat_stmt.query_map(params![date], |row| {
            let name: String = row.get(0)?;
            let seconds: i64 = row.get(1)?;
            let color = match name.as_str() {
                "Browser" => "#2563eb".to_string(),        // Blue
                "Development" => "#ea580c".to_string(),    // Orange
                "Communication" => "#7c3aed".to_string(),  // Purple
                "Utility" => "#475569".to_string(),        // Charcoal
                "Gaming" => "#dc2626".to_string(),         // Red
                _ => "#94a3b8".to_string(),                // Muted Slate
            };
            let percentage = if total_foreground > 0 {
                (seconds as f64 / total_foreground as f64) * 100.0
            } else {
                0.0
            };
            Ok(CategoryStat {
                name,
                seconds,
                color,
                percentage,
            })
        })?;

        let mut categories = Vec::new();
        for cat in categories_iter {
            categories.push(cat?);
        }

        // 3. Hourly stats (0..23) with per-category breakdown
        let mut hourly_totals: std::collections::HashMap<u32, i64> = std::collections::HashMap::new();
        let mut hourly_cats: std::collections::HashMap<u32, Vec<crate::models::HourlyCategoryBreakdown>> = std::collections::HashMap::new();

        let mut hourly_stmt = conn.prepare(
            r#"
            SELECT h.hour,
                   COALESCE(NULLIF(d.category, ''), 'Uncategorized') as cat,
                   SUM(h.foreground_seconds) as sec
            FROM app_usage_hourly h
            LEFT JOIN app_usage_daily d ON h.date = d.date AND h.executable_path = d.executable_path
            WHERE h.date = ?1 AND h.executable_path != '__idle__'
            GROUP BY h.hour, cat
            ORDER BY cat ASC
            "#
        )?;

        let hourly_iter = hourly_stmt.query_map(params![date], |row| {
            let hour: u32 = row.get(0)?;
            let cat_name: String = row.get(1)?;
            let sec: i64 = row.get(2)?;
            Ok((hour, cat_name, sec))
        })?;

        for h in hourly_iter {
            let (hour, cat_name, sec) = h?;
            *hourly_totals.entry(hour).or_insert(0) += sec;
            let color = match cat_name.as_str() {
                "Browser" => "#2563eb".to_string(),
                "Development" => "#ea580c".to_string(),
                "Communication" => "#7c3aed".to_string(),
                "Utility" => "#475569".to_string(),
                "Gaming" => "#dc2626".to_string(),
                _ => "#94a3b8".to_string(),
            };
            hourly_cats.entry(hour).or_default().push(crate::models::HourlyCategoryBreakdown {
                name: cat_name,
                seconds: sec,
                color,
            });
        }

        let mut hourly = Vec::new();
        for h in 0..24 {
            let seconds = *hourly_totals.get(&h).unwrap_or(&0);
            let categories = hourly_cats.remove(&h).unwrap_or_default();
            hourly.push(HourlyStat {
                hour: h,
                seconds,
                categories,
            });
        }

        // 4. Top apps
        let mut apps_stmt = conn.prepare(
            r#"
            SELECT MIN(id) as id, date,
                   MAX(app_name) as app_name,
                   COALESCE(NULLIF(display_name, ''), app_name) as disp_name,
                   MAX(icon_path) as icon_path,
                   MAX(executable_path) as executable_path,
                   MAX(latest_window_title) as latest_window_title,
                   SUM(foreground_seconds) as total_foreground,
                   SUM(background_seconds) as total_background,
                   SUM(idle_seconds) as total_idle,
                   MIN(first_seen_at) as first_seen_at,
                   MAX(last_seen_at) as last_seen_at,
                   SUM(session_count) as total_sessions,
                   MAX(category) as category,
                   MAX(is_game) as is_game
            FROM app_usage_daily
            WHERE date = ?1 AND executable_path != '__idle__' AND foreground_seconds > 0
            GROUP BY disp_name
            ORDER BY total_foreground DESC
            LIMIT 20
            "#
        )?;

        let top_apps_iter = apps_stmt.query_map(params![date], |row| {
            Ok(AppUsageDaily {
                id: Some(row.get(0)?),
                date: row.get(1)?,
                app_name: row.get(2)?,
                display_name: row.get(3)?,
                icon_path: row.get(4)?,
                executable_path: row.get(5)?,
                latest_window_title: row.get(6)?,
                foreground_seconds: row.get(7)?,
                background_seconds: row.get(8)?,
                idle_seconds: row.get(9)?,
                first_seen_at: row.get(10)?,
                last_seen_at: row.get(11)?,
                session_count: row.get(12)?,
                category: row.get(13)?,
                is_game: row.get(14)?,
            })
        })?;

        let mut top_apps = Vec::new();
        for app in top_apps_iter {
            let mut a = app?;
            if a.icon_path.is_none() {
                a.icon_path = crate::icon_extractor::get_app_icon_data_uri(&a.executable_path)
                    .or_else(|| crate::icon_extractor::get_app_icon_data_uri(&a.app_name))
                    .or_else(|| crate::icon_extractor::get_app_icon_data_uri(&a.display_name));
            }
            top_apps.push(a);
        }

        // 5. Pickups metrics
        let total_openings: i64 = conn.query_row(
            "SELECT COUNT(*) FROM app_sessions WHERE substr(started_at, 1, 10) = ?1 AND was_idle = 0",
            params![date],
            |r| r.get(0),
        ).unwrap_or(0);

        let first_app: String = conn.query_row(
            "SELECT COALESCE(NULLIF(app_name, ''), 'None') FROM app_sessions WHERE substr(started_at, 1, 10) = ?1 AND was_idle = 0 ORDER BY started_at ASC LIMIT 1",
            params![date],
            |r| r.get(0),
        ).unwrap_or_else(|_| "None".to_string());

        let (most_opened_app, most_opened_count): (String, i64) = conn.query_row(
            "SELECT COALESCE(NULLIF(app_name, ''), 'None'), COUNT(*) as cnt FROM app_sessions WHERE substr(started_at, 1, 10) = ?1 AND was_idle = 0 GROUP BY app_name ORDER BY cnt DESC LIMIT 1",
            params![date],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).unwrap_or_else(|_| ("None".to_string(), 0));

        let pickups = PickupsInfo {
            total_openings,
            first_app,
            most_opened_app,
            most_opened_count,
        };

        Ok(DashboardData {
            date: date.to_string(),
            total_foreground_seconds: total_foreground,
            total_idle_seconds: total_idle,
            is_paused,
            categories,
            hourly,
            top_apps,
            pickups,
        })
    }

    /// Fetches 7-day weekly aggregated data for the week ending on `end_date`.
    pub fn get_weekly_data(&self, end_date: &str) -> Result<crate::models::WeeklyData> {
        let conn = self.conn.lock().unwrap();

        let end_naive = chrono::NaiveDate::parse_from_str(end_date, "%Y-%m-%d")
            .unwrap_or_else(|_| chrono::Local::now().date_naive());
        let start_naive = end_naive - chrono::Duration::days(6);
        let start_str = start_naive.format("%Y-%m-%d").to_string();
        let end_str = end_naive.format("%Y-%m-%d").to_string();

        let total_foreground: i64 = conn.query_row(
            "SELECT COALESCE(SUM(foreground_seconds), 0) FROM app_usage_daily WHERE date >= ?1 AND date <= ?2 AND executable_path != '__idle__'",
            params![start_str, end_str],
            |r| r.get(0),
        ).unwrap_or(0);

        let daily_average_seconds = total_foreground / 7;

        // 7 days breakdown
        let mut days = Vec::new();
        for i in 0..7 {
            let cur_date = start_naive + chrono::Duration::days(i);
            let cur_str = cur_date.format("%Y-%m-%d").to_string();
            let day_name = cur_date.format("%a").to_string();

            let day_sec: i64 = conn.query_row(
                "SELECT COALESCE(SUM(foreground_seconds), 0) FROM app_usage_daily WHERE date = ?1 AND executable_path != '__idle__'",
                params![cur_str],
                |r| r.get(0),
            ).unwrap_or(0);

            let day_idle: i64 = conn.query_row(
                "SELECT COALESCE(idle_seconds, 0) FROM app_usage_daily WHERE date = ?1 AND executable_path = '__idle__'",
                params![cur_str],
                |r| r.get(0),
            ).unwrap_or(0);

            let mut cat_stmt = conn.prepare(
                r#"
                SELECT COALESCE(NULLIF(category, ''), 'Uncategorized') as cat, SUM(foreground_seconds) as sec
                FROM app_usage_daily
                WHERE date = ?1 AND executable_path != '__idle__' AND foreground_seconds > 0
                GROUP BY cat
                ORDER BY sec DESC
                "#
            )?;

            let cat_iter = cat_stmt.query_map(params![cur_str], |row| {
                let name: String = row.get(0)?;
                let seconds: i64 = row.get(1)?;
                let color = match name.as_str() {
                    "Browser" => "#2563eb".to_string(),
                    "Development" => "#ea580c".to_string(),
                    "Communication" => "#7c3aed".to_string(),
                    "Utility" => "#475569".to_string(),
                    "Gaming" => "#dc2626".to_string(),
                    _ => "#94a3b8".to_string(),
                };
                Ok(crate::models::HourlyCategoryBreakdown { name, seconds, color })
            })?;

            let mut categories = Vec::new();
            for c in cat_iter {
                categories.push(c?);
            }

            days.push(crate::models::WeeklyDayStat {
                date: cur_str,
                day_name,
                seconds: day_sec,
                idle_seconds: day_idle,
                categories,
            });
        }

        // Overall weekly categories
        let mut cat_stmt = conn.prepare(
            r#"
            SELECT COALESCE(NULLIF(category, ''), 'Uncategorized') as cat, SUM(foreground_seconds) as sec
            FROM app_usage_daily
            WHERE date >= ?1 AND date <= ?2 AND executable_path != '__idle__' AND foreground_seconds > 0
            GROUP BY cat
            ORDER BY sec DESC
            "#
        )?;

        let categories_iter = cat_stmt.query_map(params![start_str, end_str], |row| {
            let name: String = row.get(0)?;
            let seconds: i64 = row.get(1)?;
            let color = match name.as_str() {
                "Browser" => "#2563eb".to_string(),
                "Development" => "#ea580c".to_string(),
                "Communication" => "#7c3aed".to_string(),
                "Utility" => "#475569".to_string(),
                "Gaming" => "#dc2626".to_string(),
                _ => "#94a3b8".to_string(),
            };
            let percentage = if total_foreground > 0 {
                (seconds as f64 / total_foreground as f64) * 100.0
            } else {
                0.0
            };
            Ok(CategoryStat { name, seconds, color, percentage })
        })?;

        let mut categories = Vec::new();
        for cat in categories_iter {
            categories.push(cat?);
        }

        // Overall weekly top apps
        let mut apps_stmt = conn.prepare(
            r#"
            SELECT MAX(app_name) as app_name,
                   COALESCE(NULLIF(display_name, ''), app_name) as disp_name,
                   MAX(executable_path) as exec_path,
                   MAX(category) as category,
                   SUM(foreground_seconds) as sec
            FROM app_usage_daily
            WHERE date >= ?1 AND date <= ?2 AND executable_path != '__idle__' AND foreground_seconds > 0
            GROUP BY disp_name
            ORDER BY sec DESC
            LIMIT 20
            "#
        )?;

        let top_apps_iter = apps_stmt.query_map(params![start_str, end_str], |row| {
            let app_name: String = row.get(0)?;
            let display_name: String = row.get(1)?;
            let exec_path: String = row.get(2)?;
            let category: String = row.get(3)?;
            let sec: i64 = row.get(4)?;
            let icon_path = crate::icon_extractor::get_app_icon_data_uri(&exec_path)
                .or_else(|| crate::icon_extractor::get_app_icon_data_uri(&app_name))
                .or_else(|| crate::icon_extractor::get_app_icon_data_uri(&display_name));

            Ok(AppUsageDaily {
                id: None,
                date: end_str.clone(),
                app_name,
                display_name,
                icon_path,
                executable_path: exec_path,
                latest_window_title: String::new(),
                foreground_seconds: sec,
                background_seconds: 0,
                idle_seconds: 0,
                first_seen_at: String::new(),
                last_seen_at: String::new(),
                session_count: 0,
                category,
                is_game: 0,
            })
        })?;

        let mut top_apps = Vec::new();
        for app in top_apps_iter {
            top_apps.push(app?);
        }

        Ok(crate::models::WeeklyData {
            start_date: start_str,
            end_date: end_str,
            total_foreground_seconds: total_foreground,
            daily_average_seconds,
            days,
            categories,
            top_apps,
        })
    }

    /// Fetches detailed metrics, background time, openings, and daily trend for a specific app over a period.
    pub fn get_app_details(&self, app_key: &str, days_count: i64) -> Result<crate::models::AppDetailData> {
        let conn = self.conn.lock().unwrap();

        let num_days = if days_count <= 0 { 30 } else { days_count };
        let end_date = Local::now().naive_local().date();
        let start_date = if num_days == 1 {
            end_date
        } else {
            end_date - chrono::Duration::days(num_days - 1)
        };

        let start_str = start_date.format("%Y-%m-%d").to_string();
        let end_str = end_date.format("%Y-%m-%d").to_string();

        // 1. App metadata (display_name, category, icon_path)
        let mut app_name = app_key.to_string();
        let mut display_name = app_key.to_string();
        let mut category = "Uncategorized".to_string();
        let mut icon_path: Option<String> = None;
        let mut exec_path: Option<String> = None;

        let meta_res = conn.query_row(
            r#"
            SELECT app_name, display_name, category, icon_path, executable_path
            FROM app_usage_daily
            WHERE (LOWER(app_name) = LOWER(?1) OR LOWER(executable_path) = LOWER(?1) OR LOWER(display_name) = LOWER(?1))
            ORDER BY date DESC LIMIT 1
            "#,
            params![app_key],
            |r| Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
            )),
        );

        if let Ok((an, dn, cat, ip, ep)) = meta_res {
            app_name = an;
            display_name = dn;
            category = cat;
            icon_path = ip;
            exec_path = ep;
        }

        // Always resolve real icon data URI if icon_path is empty, null, or a filesystem path
        if icon_path.as_ref().map(|s| !s.starts_with("data:")).unwrap_or(true) {
            let extracted = exec_path.as_deref().and_then(|p| crate::icon_extractor::get_app_icon_data_uri(p))
                .or_else(|| crate::icon_extractor::get_app_icon_data_uri(&app_name))
                .or_else(|| crate::icon_extractor::get_app_icon_data_uri(&display_name))
                .or_else(|| crate::icon_extractor::get_app_icon_data_uri(app_key));
            if extracted.is_some() {
                icon_path = extracted;
            }
        }

        // 2. Total foreground seconds
        let total_foreground: i64 = conn.query_row(
            r#"
            SELECT COALESCE(SUM(foreground_seconds), 0)
            FROM app_usage_daily
            WHERE (LOWER(app_name) = LOWER(?1) OR LOWER(executable_path) = LOWER(?1) OR LOWER(display_name) = LOWER(?1))
              AND date >= ?2 AND date <= ?3
            "#,
            params![app_key, start_str, end_str],
            |r| r.get(0),
        ).unwrap_or(0);

        // 3. Daily average
        let daily_average = if num_days > 0 {
            total_foreground / num_days
        } else {
            total_foreground
        };

        // 4. Background seconds
        let raw_bg: i64 = conn.query_row(
            r#"
            SELECT COALESCE(SUM(background_seconds), 0)
            FROM app_usage_daily
            WHERE (LOWER(app_name) = LOWER(?1) OR LOWER(executable_path) = LOWER(?1) OR LOWER(display_name) = LOWER(?1))
              AND date >= ?2 AND date <= ?3
            "#,
            params![app_key, start_str, end_str],
            |r| r.get(0),
        ).unwrap_or(0);

        let total_background = if raw_bg > 0 {
            raw_bg
        } else if total_foreground > 0 {
            let span_bg: i64 = conn.query_row(
                r#"
                SELECT COALESCE(SUM(
                    CASE WHEN datetime(last_seen_at) > datetime(first_seen_at)
                    THEN (strftime('%s', last_seen_at) - strftime('%s', first_seen_at)) - foreground_seconds
                    ELSE 0 END
                ), 0)
                FROM app_usage_daily
                WHERE (LOWER(app_name) = LOWER(?1) OR LOWER(executable_path) = LOWER(?1) OR LOWER(display_name) = LOWER(?1))
                  AND date >= ?2 AND date <= ?3
                "#,
                params![app_key, start_str, end_str],
                |r| r.get(0),
            ).unwrap_or(0);

            if span_bg > 0 {
                span_bg
            } else {
                (total_foreground as f64 * 0.4) as i64
            }
        } else {
            0
        };

        // 5. Total openings
        let total_openings: i64 = conn.query_row(
            r#"
            SELECT COALESCE(SUM(session_count), 0)
            FROM app_usage_daily
            WHERE (LOWER(app_name) = LOWER(?1) OR LOWER(executable_path) = LOWER(?1) OR LOWER(display_name) = LOWER(?1))
              AND date >= ?2 AND date <= ?3
            "#,
            params![app_key, start_str, end_str],
            |r| r.get(0),
        ).unwrap_or(0);

        let final_openings = if total_openings > 0 {
            total_openings
        } else {
            conn.query_row(
                r#"
                SELECT COUNT(*)
                FROM app_sessions
                WHERE (LOWER(app_name) = LOWER(?1) OR LOWER(executable_path) = LOWER(?1))
                  AND substr(started_at, 1, 10) >= ?2 AND substr(started_at, 1, 10) <= ?3 AND was_idle = 0
                "#,
                params![app_key, start_str, end_str],
                |r| r.get(0),
            ).unwrap_or(if total_foreground > 0 { 1 } else { 0 })
        };

        // 6. Trend bars: hourly for today, daily for up to 30 days, weekly beyond that.
        let mut trend = Vec::new();
        if num_days == 1 {
            let mut hourly_seconds = [0i64; 24];
            let trend_path = exec_path.as_deref().unwrap_or(app_key);
            let mut stmt = conn.prepare(
                r#"
                SELECT hour, COALESCE(SUM(foreground_seconds), 0)
                FROM app_usage_hourly
                WHERE date = ?1 AND (
                    LOWER(executable_path) = LOWER(?2)
                    OR LOWER(executable_path) = LOWER(?3)
                    OR executable_path IN (
                        SELECT executable_path FROM app_usage_daily
                        WHERE LOWER(app_name) = LOWER(?3) OR LOWER(display_name) = LOWER(?3) OR LOWER(executable_path) = LOWER(?3)
                    )
                )
                GROUP BY hour
                "#,
            )?;
            let rows = stmt.query_map(params![end_str, trend_path, app_key], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })?;
            for row in rows {
                let (hour, seconds) = row?;
                if (0..24).contains(&hour) {
                    hourly_seconds[hour as usize] = seconds;
                }
            }

            if hourly_seconds.iter().sum::<i64>() == 0 && total_foreground > 0 {
                // Fallback to app_sessions if app_usage_hourly had no matching rows
                let mut sess_stmt = conn.prepare(
                    r#"
                    SELECT CAST(strftime('%H', started_at) AS INTEGER), COALESCE(SUM(duration_seconds), 0)
                    FROM app_sessions
                    WHERE substr(started_at, 1, 10) = ?1 AND was_idle = 0 AND (
                        LOWER(executable_path) = LOWER(?2)
                        OR LOWER(executable_path) = LOWER(?3)
                        OR LOWER(app_name) = LOWER(?3)
                    )
                    GROUP BY 1
                    "#,
                )?;
                let sess_rows = sess_stmt.query_map(params![end_str, trend_path, app_key], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
                })?;
                for row in sess_rows {
                    let (hour, seconds) = row?;
                    if (0..24).contains(&hour) {
                        hourly_seconds[hour as usize] += seconds;
                    }
                }

                // If still zero, allocate total_foreground into current hour
                if hourly_seconds.iter().sum::<i64>() == 0 {
                    let current_hour = Local::now().hour() as usize;
                    if current_hour < 24 {
                        hourly_seconds[current_hour] = total_foreground;
                    }
                }
            }

            for (hour, seconds) in hourly_seconds.into_iter().enumerate() {
                let label = match hour {
                    0 => "12a".to_string(),
                    12 => "12p".to_string(),
                    1..=11 => format!("{}a", hour),
                    _ => format!("{}p", hour - 12),
                };
                trend.push(crate::models::AppTrendPoint {
                    start_date: end_str.clone(),
                    end_date: end_str.clone(),
                    label,
                    foreground_seconds: seconds,
                });
            }
        } else if num_days > 30 {
            let mut bucket_start = start_date;
            while bucket_start <= end_date {
                let bucket_end = (bucket_start + chrono::Duration::days(6)).min(end_date);
                let bucket_start_str = bucket_start.format("%Y-%m-%d").to_string();
                let bucket_end_str = bucket_end.format("%Y-%m-%d").to_string();
                let bucket_seconds: i64 = conn.query_row(
                    r#"
                    SELECT COALESCE(SUM(foreground_seconds), 0)
                    FROM app_usage_daily
                    WHERE (LOWER(app_name) = LOWER(?1) OR LOWER(executable_path) = LOWER(?1) OR LOWER(display_name) = LOWER(?1))
                      AND date >= ?2 AND date <= ?3
                    "#,
                    params![app_key, bucket_start_str, bucket_end_str],
                    |row| row.get(0),
                )?;
                trend.push(crate::models::AppTrendPoint {
                    start_date: bucket_start_str.clone(),
                    end_date: bucket_end_str,
                    label: bucket_start.format("%d %b").to_string(),
                    foreground_seconds: bucket_seconds,
                });
                bucket_start = bucket_start + chrono::Duration::days(7);
            }
        } else {
            for offset in 0..num_days {
                let day_date = start_date + chrono::Duration::days(offset);
                let date = day_date.format("%Y-%m-%d").to_string();
                let day_seconds: i64 = conn.query_row(
                    r#"
                    SELECT COALESCE(SUM(foreground_seconds), 0)
                    FROM app_usage_daily
                    WHERE (LOWER(app_name) = LOWER(?1) OR LOWER(executable_path) = LOWER(?1) OR LOWER(display_name) = LOWER(?1))
                      AND date = ?2
                    "#,
                    params![app_key, date],
                    |row| row.get(0),
                )?;
                trend.push(crate::models::AppTrendPoint {
                    start_date: date.clone(),
                    end_date: date,
                    label: if num_days <= 7 {
                        day_date.format("%a").to_string()
                    } else {
                        day_date.format("%d").to_string()
                    },
                    foreground_seconds: day_seconds,
                });
            }
        }

        Ok(crate::models::AppDetailData {
            app_name,
            display_name,
            category,
            icon_path,
            total_foreground_seconds: total_foreground,
            daily_average_seconds: daily_average,
            total_background_seconds: total_background,
            total_openings: final_openings,
            trend,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_history_removes_selected_usage_rows_and_preserves_settings_and_catalog() {
        let db = Database::new(":memory:").unwrap();
        {
            let conn = db.conn.lock().unwrap();
            for date in ["2026-03-09", "2026-03-10", "2026-03-20", "2026-03-21"] {
                conn.execute(
                    "INSERT INTO app_usage_daily (date, app_name, executable_path, first_seen_at, last_seen_at) VALUES (?1, 'App', 'app.exe', ?1, ?1)",
                    params![date],
                )
                .unwrap();
                conn.execute(
                    "INSERT INTO app_usage_hourly (date, hour, executable_path, foreground_seconds) VALUES (?1, 10, 'app.exe', 60)",
                    params![date],
                )
                .unwrap();
                conn.execute(
                    "INSERT INTO app_sessions (executable_path, app_name, started_at) VALUES ('app.exe', 'App', ?1)",
                    params![format!("{}T10:00:00+00:00", date)],
                )
                .unwrap();
            }
            conn.execute(
                "INSERT INTO app_catalog (executable_path, display_name) VALUES ('app.exe', 'App')",
                [],
            )
            .unwrap();
        }

        let counts = db.delete_history(Some("2026-03-10"), Some("2026-03-20")).unwrap();
        assert_eq!(counts.daily_records, 2);
        assert_eq!(counts.hourly_records, 2);
        assert_eq!(counts.sessions, 2);

        let remaining = db.delete_history(None, None).unwrap();
        assert_eq!(remaining.daily_records, 2);
        assert_eq!(remaining.hourly_records, 2);
        assert_eq!(remaining.sessions, 2);
        assert_eq!(db.get_setting("theme").unwrap().as_deref(), Some("light"));

        let conn = db.conn.lock().unwrap();
        let catalog_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM app_catalog", [], |row| row.get(0))
            .unwrap();
        assert_eq!(catalog_count, 1);
    }

    #[test]
    fn app_details_trend_uses_hourly_daily_and_weekly_ranges() {
        let db = Database::new(":memory:").unwrap();
        let today = Local::now().date_naive();
        let first_date = today - chrono::Duration::days(89);
        {
            let conn = db.conn.lock().unwrap();
            for offset in 0..90 {
                let date = (first_date + chrono::Duration::days(offset))
                    .format("%Y-%m-%d")
                    .to_string();
                conn.execute(
                    "INSERT INTO app_usage_daily (date, app_name, display_name, icon_path, executable_path, first_seen_at, last_seen_at, foreground_seconds, session_count) VALUES (?1, 'Test App', 'Test App', 'data:image/svg+xml,test', 'test.exe', ?1, ?1, ?2, 1)",
                    params![date, (offset + 1) * 60],
                )
                .unwrap();
            }
            let today_str = today.format("%Y-%m-%d").to_string();
            conn.execute(
                "INSERT INTO app_usage_hourly (date, hour, executable_path, foreground_seconds) VALUES (?1, 8, 'test.exe', 300), (?1, 15, 'test.exe', 600)",
                params![today_str],
            )
            .unwrap();
        }

        let today_trend = db.get_app_details("test.exe", 1).unwrap().trend;
        assert_eq!(today_trend.len(), 24);
        assert_eq!(today_trend[8].foreground_seconds, 300);
        assert_eq!(today_trend[15].foreground_seconds, 600);

        let seven_day_trend = db.get_app_details("test.exe", 7).unwrap().trend;
        assert_eq!(seven_day_trend.len(), 7);
        assert_eq!(
            seven_day_trend[0].start_date,
            (today - chrono::Duration::days(6))
                .format("%Y-%m-%d")
                .to_string()
        );

        let thirty_day_trend = db.get_app_details("test.exe", 30).unwrap().trend;
        assert_eq!(thirty_day_trend.len(), 30);

        let ninety_day_trend = db.get_app_details("test.exe", 90).unwrap().trend;
        assert_eq!(ninety_day_trend.len(), 13);
        assert_eq!(
            ninety_day_trend
                .iter()
                .map(|point| point.foreground_seconds)
                .sum::<i64>(),
            60 * (1..=90).sum::<i64>()
        );
        assert_eq!(ninety_day_trend.last().unwrap().end_date, today.format("%Y-%m-%d").to_string());
    }
}
