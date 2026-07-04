use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct Event {
    pub key: String,
    pub source_task: Option<u32>,
    pub payload: Option<String>,
    pub timestamp: u64,
}

pub struct EventBus {
    pub(crate) log: Arc<Mutex<Vec<Event>>>,
    pub(crate) emitted_keys: Arc<Mutex<std::collections::HashSet<String>>>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            log: Arc::new(Mutex::new(Vec::new())),
            emitted_keys: Arc::new(Mutex::new(std::collections::HashSet::new())),
        }
    }

    pub fn emit(&self, event: Event) {
        self.emitted_keys.lock().unwrap().insert(event.key.clone());
        self.log.lock().unwrap().push(event);
    }

    pub fn has_emitted(&self, key: &str) -> bool {
        self.emitted_keys.lock().unwrap().contains(key)
    }

    pub fn log(&self) -> Vec<Event> {
        self.log.lock().unwrap().clone()
    }

    pub fn events_since(&self, ts: u64) -> Vec<Event> {
        self.log.lock().unwrap().iter().filter(|e| e.timestamp > ts).cloned().collect()
    }
}
