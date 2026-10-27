use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Resource {
    pub id: String,
    pub path: String,
    pub kind: String,
    pub size: u64,
    pub enabled: bool,
    pub order: usize,
    pub key: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct IndexStatus {
    pub state: String,
    pub indexed: usize,
    pub total: usize,
    pub error: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Dictionary {
    pub id: String,
    pub name: String,
    pub revision: String,
    pub enabled: bool,
    pub icon: Option<String>,
    pub entries: usize,
    pub metadata: Value,
    pub resources: Vec<Resource>,
    pub trust_scripts: bool,
    pub custom_css: String,
    pub font_family: String,
    #[serde(default = "global_typography")]
    pub typography_mode: String,
    pub warnings: Vec<String>,
    pub index_status: IndexStatus,
    #[serde(default)]
    pub mdx: String,
    #[serde(default)]
    pub created: String,
}
pub fn global_typography() -> String {
    "global".into()
}
#[derive(Clone, Serialize, Deserialize)]
pub struct SortRule {
    pub field: String,
    pub direction: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub dictionary_ids: Vec<String>,
    pub sort: Vec<SortRule>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct HistoryItem {
    pub id: String,
    pub word: String,
    pub group_id: String,
    pub at: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct LogItem {
    pub id: String,
    pub at: String,
    pub level: String,
    pub scope: String,
    pub message: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub kind: String,
    pub dictionary_id: Option<String>,
    pub state: String,
    pub progress: usize,
    pub total: usize,
    pub error: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub dictionaries: Vec<Dictionary>,
    pub groups: Vec<Group>,
    pub settings: Value,
    pub history: Vec<HistoryItem>,
}
pub fn default_settings() -> Value {
    json!({"application":{"default_group":"all","history_enabled":true,"history_limit":200},"appearance":{"theme":"system","font_size":18,"line_height":1.8,"font_family":"","content_width":840,"motion":true,"reading_mode":false},"spelling":{"enabled":true,"distance":1},"security":{"external_links":"ask"},"performance":{"max_open_readers":8,"cache_mb":128,"query_workers":4}})
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            version: 1,
            dictionaries: vec![],
            groups: vec![],
            settings: default_settings(),
            history: vec![],
        }
    }
}
