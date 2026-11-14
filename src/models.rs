use serde::{Deserialize, Serialize};

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub process_id: u32,
    pub executable_path: String,
    pub app_name: String,
    pub display_name: String,
    pub window_title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppUsageDaily {
    pub id: Option<i64>,
    pub date: String,
    pub app_name: String,
    pub display_name: String,
    pub icon_path: Option<String>,
    pub executable_path: String,
    pub latest_window_title: String,
    pub foreground_seconds: i64,
    pub background_seconds: i64,
    pub idle_seconds: i64,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub session_count: i64,
    pub category: String,
    pub is_game: i64,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSession {
    pub id: Option<i64>,
    pub executable_path: String,
    pub app_name: String,
    pub window_title: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub foreground_seconds: i64,
    pub background_seconds: i64,
    pub was_idle: i64,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppCatalogEntry {
    pub executable_path: String,
    pub display_name: String,
    pub icon_path: Option<String>,
    pub category: String,
    pub is_game: i64,
    pub source: String,
    pub updated_at: String,
}

#[allow(dead_code)]
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppLimit {
    pub id: Option<i64>,
    pub target_type: String, // "app" or "category"
    pub target_value: String, // executable_path or category name
    pub daily_minutes: i64,
    pub enabled: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DowntimeSchedule {
    pub id: Option<i64>,
    pub start_minute: i32,
    pub end_minute: i32,
    pub days_mask: i32,
    pub enabled: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodaySummary {
    pub date: String,
    pub total_foreground_seconds: i64,
    pub total_idle_seconds: i64,
    pub app_count: usize,
    pub top_apps: Vec<AppUsageDaily>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryStat {
    pub name: String,
    pub seconds: i64,
    pub color: String,
    pub percentage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HourlyCategoryBreakdown {
    pub name: String,
    pub seconds: i64,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HourlyStat {
    pub hour: u32,
    pub seconds: i64,
    pub categories: Vec<HourlyCategoryBreakdown>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PickupsInfo {
    pub total_openings: i64,
    pub first_app: String,
    pub most_opened_app: String,
    pub most_opened_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardData {
    pub date: String,
    pub total_foreground_seconds: i64,
    pub total_idle_seconds: i64,
    pub is_paused: bool,
    pub categories: Vec<CategoryStat>,
    pub hourly: Vec<HourlyStat>,
    pub top_apps: Vec<AppUsageDaily>,
    pub pickups: PickupsInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeeklyDayStat {
    pub date: String,
    pub day_name: String,
    pub seconds: i64,
    pub idle_seconds: i64,
    pub categories: Vec<HourlyCategoryBreakdown>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeeklyData {
    pub start_date: String,
    pub end_date: String,
    pub total_foreground_seconds: i64,
    pub daily_average_seconds: i64,
    pub days: Vec<WeeklyDayStat>,
    pub categories: Vec<CategoryStat>,
    pub top_apps: Vec<AppUsageDaily>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppDayTrend {
    pub date: String,
    pub day_name: String,
    pub foreground_seconds: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppDetailData {
    pub app_name: String,
    pub display_name: String,
    pub category: String,
    pub icon_path: Option<String>,
    pub total_foreground_seconds: i64,
    pub daily_average_seconds: i64,
    pub total_background_seconds: i64,
    pub total_openings: i64,
    pub trend: Vec<AppDayTrend>,
}
