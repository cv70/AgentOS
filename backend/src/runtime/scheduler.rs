use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};

use uuid::Uuid;

#[derive(Debug, Default)]
pub struct InMemoryScheduler {
    pub running: HashSet<Uuid>,
    pub queued: VecDeque<Uuid>,
}

pub type SharedScheduler = Arc<Mutex<InMemoryScheduler>>;

pub fn remove_running(scheduler: &SharedScheduler, task_id: Uuid) {
    if let Ok(mut state) = scheduler.lock() {
        state.running.remove(&task_id);
    }
}

pub fn pop_next_if_capacity(
    scheduler: &SharedScheduler,
    max_concurrent_tasks: usize,
) -> Option<Uuid> {
    let mut state = scheduler.lock().ok()?;
    if state.running.len() >= max_concurrent_tasks {
        return None;
    }
    state.queued.pop_front()
}

pub fn try_mark_running(
    scheduler: &SharedScheduler,
    task_id: Uuid,
    max_concurrent_tasks: usize,
) -> bool {
    let Ok(mut state) = scheduler.lock() else {
        return false;
    };
    if state.running.len() >= max_concurrent_tasks {
        return false;
    }
    state.running.insert(task_id);
    true
}

pub fn requeue_front(scheduler: &SharedScheduler, task_id: Uuid) {
    if let Ok(mut state) = scheduler.lock() {
        state.queued.push_front(task_id);
    }
}
