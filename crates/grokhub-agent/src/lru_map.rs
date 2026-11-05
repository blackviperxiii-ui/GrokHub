//! A small map keyed by chat or run id that drops its least recently used
//! entry when full, so a busy cabin never wipes the chat that is still open.

use std::collections::BTreeMap;

pub(crate) struct LruMap<V> {
    cap: usize,
    tick: u64,
    entries: BTreeMap<String, (u64, V)>,
}

impl<V> LruMap<V> {
    pub(crate) const fn new(cap: usize) -> Self {
        Self {
            cap,
            tick: 0,
            entries: BTreeMap::new(),
        }
    }

    fn next_tick(&mut self) -> u64 {
        self.tick += 1;
        self.tick
    }

    /// Before `key` is added, drop the least recently used entry if the map is
    /// full. Returns the dropped key, so a caller can clear what it keyed by it.
    pub(crate) fn make_room(&mut self, key: &str) -> Option<String> {
        if self.cap == 0 || self.entries.len() < self.cap || self.entries.contains_key(key) {
            return None;
        }
        let oldest = self
            .entries
            .iter()
            .min_by_key(|(_, (used, _))| *used)
            .map(|(k, _)| k.clone())?;
        self.entries.remove(&oldest);
        Some(oldest)
    }

    /// Insert or replace, marking `key` as just used. Returns the old value.
    pub(crate) fn insert(&mut self, key: &str, value: V) -> Option<V> {
        self.make_room(key);
        let tick = self.next_tick();
        self.entries
            .insert(key.to_string(), (tick, value))
            .map(|(_, old)| old)
    }

    /// The value for `key`, marking it as just used.
    pub(crate) fn get(&mut self, key: &str) -> Option<&V> {
        let tick = self.next_tick();
        let slot = self.entries.get_mut(key)?;
        slot.0 = tick;
        Some(&slot.1)
    }

    /// The value for `key` to change, marking it as just used.
    pub(crate) fn get_mut(&mut self, key: &str) -> Option<&mut V> {
        let tick = self.next_tick();
        let slot = self.entries.get_mut(key)?;
        slot.0 = tick;
        Some(&mut slot.1)
    }

    /// The value for `key`, created with `make` if missing; marks it as just used.
    pub(crate) fn get_or_insert_with(&mut self, key: &str, make: impl FnOnce() -> V) -> &mut V {
        if !self.entries.contains_key(key) {
            self.make_room(key);
        }
        let tick = self.next_tick();
        let slot = self
            .entries
            .entry(key.to_string())
            .or_insert_with(|| (tick, make()));
        slot.0 = tick;
        &mut slot.1
    }

    pub(crate) fn remove(&mut self, key: &str) -> Option<V> {
        self.entries.remove(key).map(|(_, v)| v)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::LruMap;

    #[test]
    fn a_chat_in_use_survives_a_flood_of_new_ones() {
        let mut map = LruMap::new(256);
        map.insert("chat-a", 1);
        for i in 0..300 {
            map.insert(&format!("other-{i}"), i);
            assert_eq!(map.get("chat-a"), Some(&1), "chat-a dropped at {i}");
        }
        assert_eq!(map.len(), 256);
    }

    #[test]
    fn the_least_recently_used_entry_goes_first() {
        let mut map = LruMap::new(2);
        map.insert("a", 1);
        map.insert("b", 2);
        map.get("a");
        assert_eq!(map.make_room("c"), Some("b".to_string()));
        map.insert("c", 3);
        assert_eq!(map.get("b"), None);
        assert_eq!(map.get("a"), Some(&1));
        assert_eq!(map.make_room("a"), None, "an existing key needs no room");
    }

    #[test]
    fn get_or_insert_with_reuses_and_touches() {
        let mut map: LruMap<Vec<u8>> = LruMap::new(2);
        map.get_or_insert_with("a", Vec::new).push(1);
        map.get_or_insert_with("b", Vec::new);
        map.get_or_insert_with("a", Vec::new).push(2);
        map.get_or_insert_with("c", Vec::new);
        assert_eq!(map.get("a"), Some(&vec![1, 2]));
        assert_eq!(map.get("b"), None);
    }
}
