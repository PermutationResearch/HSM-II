use crate::property_graph::PropertyGraphSnapshot;

#[derive(Clone, Debug)]
pub struct Transaction {
    pub id: u64,
    working_copy: PropertyGraphSnapshot,
}

#[derive(Default)]
pub struct TransactionManager {
    next_id: u64,
}

impl TransactionManager {
    pub fn begin(&mut self, snapshot: &PropertyGraphSnapshot) -> Transaction {
        let tx = Transaction {
            id: self.next_id,
            working_copy: snapshot.clone(),
        };
        self.next_id += 1;
        tx
    }
}

impl Transaction {
    pub fn snapshot(&self) -> &PropertyGraphSnapshot {
        &self.working_copy
    }

    pub fn snapshot_mut(&mut self) -> &mut PropertyGraphSnapshot {
        &mut self.working_copy
    }

    pub fn commit(self, target: &mut PropertyGraphSnapshot) {
        *target = self.working_copy;
    }

    pub fn rollback(self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::property_graph::{GraphNodeRecord, PropertyGraphSnapshot};
    use std::collections::HashMap;

    #[test]
    fn commit_replaces_target_snapshot() {
        let mut manager = TransactionManager::default();
        let base = PropertyGraphSnapshot::default();
        let mut tx = manager.begin(&base);
        tx.snapshot_mut().nodes.push(GraphNodeRecord {
            id: "n1".into(),
            labels: vec!["Test".into()],
            properties: HashMap::new(),
        });
        let mut target = PropertyGraphSnapshot::default();
        tx.commit(&mut target);
        assert_eq!(target.nodes.len(), 1);
    }
}
