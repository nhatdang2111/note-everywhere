//! A small least-recently-used cache of open page documents.

use std::collections::HashMap;

use ne_core::NodeId;
use ne_core::page::PageDoc;

/// Open pages, bounded in number. The documents are small next to the cost of
/// loading them from the database, but a long session must not keep every page
/// it ever visited in memory.
pub(crate) struct PageCache {
    capacity: usize,
    tick: u64,
    docs: HashMap<NodeId, (PageDoc, u64)>,
}

impl PageCache {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            tick: 0,
            docs: HashMap::new(),
        }
    }

    pub(crate) fn get(&mut self, id: NodeId) -> Option<&PageDoc> {
        self.tick += 1;
        let tick = self.tick;
        self.docs.get_mut(&id).map(|(doc, used)| {
            *used = tick;
            &*doc
        })
    }

    pub(crate) fn contains(&self, id: NodeId) -> bool {
        self.docs.contains_key(&id)
    }

    pub(crate) fn insert(&mut self, id: NodeId, doc: PageDoc) {
        self.tick += 1;
        self.docs.insert(id, (doc, self.tick));
        while self.docs.len() > self.capacity {
            let oldest = self
                .docs
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(id, _)| *id);
            match oldest {
                Some(oldest) => self.docs.remove(&oldest),
                None => break,
            };
        }
    }

    pub(crate) fn remove(&mut self, id: NodeId) {
        self.docs.remove(&id);
    }

    pub(crate) fn clear(&mut self) {
        self.docs.clear();
    }

    pub(crate) fn len(&self) -> usize {
        self.docs.len()
    }
}
