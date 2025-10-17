// 便签功能模块
// 主要逻辑在 models 和 db 层，这里提供一些辅助功能

use crate::models::Note;

/// 便签管理器
pub struct NoteManager;

impl NoteManager {
    pub fn new() -> Self {
        Self
    }

    /// 搜索便签
    pub fn search_notes(&self, notes: &[Note], query: &str) -> Vec<Note> {
        let query_lower = query.to_lowercase();
        notes
            .iter()
            .filter(|note| {
                note.title.to_lowercase().contains(&query_lower)
                    || note.content.to_lowercase().contains(&query_lower)
            })
            .cloned()
            .collect()
    }

    /// 按任务ID过滤便签
    pub fn notes_by_task(&self, notes: &[Note], task_id: i64) -> Vec<Note> {
        notes
            .iter()
            .filter(|note| note.task_id == Some(task_id))
            .cloned()
            .collect()
    }
}
