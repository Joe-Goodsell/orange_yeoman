//! In-memory store of active and completed tasks.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use super::ids::DedupIdentity;
use super::types::{TaskMetadata, TaskStatus};

/// In-memory store of active and completed tasks. Thread-safe via Mutex.
pub(crate) struct TaskStore {
    tasks: Mutex<HashMap<String, TaskMetadata>>,
    active_dedup: Mutex<HashSet<DedupIdentity>>,
}

impl TaskStore {
    pub(crate) fn new() -> Self {
        TaskStore {
            tasks: Mutex::new(HashMap::new()),
            active_dedup: Mutex::new(HashSet::new()),
        }
    }

    /// Insert a new task. Returns the task_id.
    /// If an automatic task with the same DedupIdentity is already active (Queued or Running),
    /// return None to signal a duplicate. Explicit commands (FactCheck or Research trigger)
    /// always insert and never dedup.
    pub(crate) fn insert(
        &self,
        metadata: TaskMetadata,
        identity: &DedupIdentity,
    ) -> Option<String> {
        {
            let mut active = self.active_dedup.lock().unwrap_or_else(|e| e.into_inner());
            if identity.trigger == crate::pipeline::Trigger::Automatic && active.contains(identity)
            {
                return None;
            }
            active.insert(identity.clone());
        }

        let mut tasks = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
        let task_id = metadata.task_id.clone();
        tasks.insert(task_id.clone(), metadata);
        Some(task_id)
    }

    /// Get a copy of a task by id.
    pub(crate) fn get(&self, task_id: &str) -> Option<TaskMetadata> {
        self.tasks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(task_id)
            .cloned()
    }

    /// Update a task's status, stale flag, and error.
    pub(crate) fn update(
        &self,
        task_id: &str,
        status: TaskStatus,
        stale: bool,
        error: Option<String>,
    ) {
        if let Some(task) = self
            .tasks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(task_id)
        {
            task.status = status;
            task.stale = stale;
            task.error = error;
        }
    }

    /// Remove a task's dedup identity from the active set (call when task completes or fails).
    pub(crate) fn clear_dedup(&self, identity: &DedupIdentity) {
        self.active_dedup
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(identity);
    }
}

impl Default for TaskStore {
    fn default() -> Self {
        Self::new()
    }
}