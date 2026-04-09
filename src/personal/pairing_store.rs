//! Thread-safe store for channel/session pairings.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Clone, Default)]
pub struct PairingStore {
    inner: Arc<RwLock<HashMap<String, String>>>,
}

impl PairingStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_pairing(
        &self,
        scope_key: impl Into<String>,
        session_key: impl Into<String>,
    ) -> Option<String> {
        let mut guard = self.inner.write().ok()?;
        guard.insert(scope_key.into(), session_key.into())
    }

    pub fn get_pairing(&self, scope_key: &str) -> Option<String> {
        let guard = self.inner.read().ok()?;
        guard.get(scope_key).cloned()
    }

    pub fn remove_pairing(&self, scope_key: &str) -> Option<String> {
        let mut guard = self.inner.write().ok()?;
        guard.remove(scope_key)
    }

    pub fn len(&self) -> usize {
        self.inner
            .read()
            .map(|g| g.len())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::PairingStore;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn pairing_store_is_thread_safe() {
        let store = Arc::new(PairingStore::new());
        let mut joins = Vec::new();
        for i in 0..20 {
            let s = Arc::clone(&store);
            joins.push(thread::spawn(move || {
                let key = format!("channel-{i}");
                let val = format!("session-{i}");
                s.set_pairing(key.clone(), val.clone());
                assert_eq!(s.get_pairing(&key).as_deref(), Some(val.as_str()));
            }));
        }
        for j in joins {
            j.join().expect("thread join");
        }
        assert_eq!(store.len(), 20);
    }
}
