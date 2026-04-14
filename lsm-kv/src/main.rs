// CORE IDEA BEHIND LSM TREES

// Writes → MemTable (in-memory, sorted)
// ↓ when full (e.g. 1MB)
// SSTable (immutable sorted file on disk) — Level 0
// ↓ when too many L0 files
// Merge + compact → Level 1 (larger sorted files)
// ↓
// Level 2 → Level 3 → ...

use std::collections::BTreeMap;
use std::fs::{File};
use std::io::{Write, Read, BufWriter, BufReader};
use std::collections::{HashMap, HashSet};


#[derive(Debug, Clone)]
pub struct ORSet {
    node_id: u64,
    counter: u64,
    // element → set of (node_id, counter) tags that added it
    entries: HashMap<String, HashSet<(u64, u64)>>,
    // tombstones — tags that have been removed
    tombstones: HashSet<(u64, u64)>,
}

impl ORSet {
    pub fn new(node_id: u64) -> Self {
        ORSet {
            node_id,
            counter: 0,
            entries: HashMap::new(),
            tombstones: HashSet::new(),
        }
    }

    pub fn add(&mut self, element: String) {
        self.counter += 1;
        let tag = (self.node_id, self.counter);
        self.entries.entry(element).or_default().insert(tag);
    }

    pub fn remove(&mut self, element: &str) {
        // remove all current tags for this element
        if let Some(tags) = self.entries.get(element) {
            for &tag in tags {
                self.tombstones.insert(tag);
            }
        }
    }

    pub fn contains(&self, element: &str) -> bool {
        if let Some(tags) = self.entries.get(element) {
            // element exists if any of its tags are not tombstoned
            tags.iter().any(|tag| !self.tombstones.contains(tag))
        } else {
            false
        }
    }

    pub fn merge(&mut self, other: &ORSet) {
        // merge tombstones
        for &tag in &other.tombstones {
            self.tombstones.insert(tag);
        }
        // merge entries
        for (element, tags) in &other.entries {
            let entry = self.entries.entry(element.clone()).or_default();
            for &tag in tags {
                entry.insert(tag);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct PNCounter {
    increments: GCounter,
    decrements: GCounter,
}

impl PNCounter {
    pub fn new(node_id: u64) -> Self {
        PNCounter {
            increments: GCounter::new(node_id),
            decrements: GCounter::new(node_id),
        }
    }

    pub fn increment(&mut self) {
        self.increments.increment();
    }

    pub fn decrement(&mut self) {
        self.decrements.increment();  // note: incrementing the decrement counter
    }

    pub fn value(&self) -> i64 {
        self.increments.value() as i64 - self.decrements.value() as i64
    }

    pub fn merge(&mut self, other: &PNCounter) {
        self.increments.merge(&other.increments);
        self.decrements.merge(&other.decrements);
    }
}

#[derive(Debug, Clone)]
pub struct GCounter {
    counts: HashMap<u64, u64>,  // node_id → increment count
    node_id: u64,
}

impl GCounter {
    pub fn new(node_id: u64) -> Self {
        GCounter {
            counts: HashMap::new(),
            node_id,
        }
    }

    pub fn increment(&mut self) {
        let count = self.counts.entry(self.node_id).or_insert(0);
        *count += 1;
    }

    pub fn value(&self) -> u64 {
        self.counts.values().sum()
    }

    pub fn merge(&mut self, other: &GCounter) {
        for (&node_id, &count) in &other.counts {
            let entry = self.counts.entry(node_id).or_insert(0);
            *entry = (*entry).max(count);
        }
    }
}

pub struct MemTable {
    data: BTreeMap<String, Option<String>>,
    size_bytes: usize,
    capacity: usize,
}

impl MemTable {
    pub fn new(capacity: usize) -> Self {
        MemTable {
            data: BTreeMap::new(),
            size_bytes: 0,
            capacity,
        }
    }

    pub fn set(&mut self, key: String, value: String) {
        self.size_bytes += key.len() + value.len();
        self.data.insert(key, Some(value));
    }

    pub fn delete(&mut self, key: String) {
        self.size_bytes += key.len();
        self.data.insert(key, None);
    }

    pub fn get(&self, key: &str) -> Option<Option<&str>> {
        match self.data.get(key) {
            Some(Some(v)) => Some(Some(v.as_str())),
            Some(None) => Some(None),
            None => None
        }
    }

    pub fn is_full(&self) -> bool {
        self.size_bytes >= self.capacity
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Option<String>)> {
        self.data.iter()
    }
}


pub struct SSTable {
    path: String
}

impl SSTable {
    pub fn flush(mem: &MemTable, path: &str) -> std::io::Result<Self> {
        let file = File::create(path)?;

        let mut writer = BufWriter::new(file);

        for (key, value) in mem.iter() {
            let key_bytes = key.as_bytes();
            writer.write_all(&(key_bytes.len() as u32).to_le_bytes())?;
            writer.write_all(key_bytes)?;

            match value {
                Some(v) => {
                    let val_bytes = v.as_bytes();
                    writer.write_all(&(val_bytes.len() as u32).to_le_bytes())?;
                    writer.write_all(val_bytes)?; 
                }
                None => {
                    writer.write_all(&0u32.to_le_bytes())?;
                }
            }
        }
        writer.flush()?;
        Ok(SSTable { path: path.to_string() })
    }

    // scan entire SSTable for a key — O(n)
    pub fn get(&self, key: &str) -> std::io::Result<Option<Option<String>>> {
        // None         = key not found
        // Some(None)   = tombstone
        // Some(Some(v)) = found with value
        let file = File::open(&self.path)?;
        let mut reader = BufReader::new(file);

        loop {
            // read key length
            let mut len_buf = [0u8; 4];
            match reader.read_exact(&mut len_buf) {
                Ok(_) => {}
                Err(_) => break,  // EOF
            }
            let key_len = u32::from_le_bytes(len_buf) as usize;
            let mut key_buf = vec![0u8; key_len];
            reader.read_exact(&mut key_buf)?;
            let entry_key = String::from_utf8_lossy(&key_buf).to_string();

            // read value length
            let mut val_len_buf = [0u8; 4];
            reader.read_exact(&mut val_len_buf)?;
            let val_len = u32::from_le_bytes(val_len_buf) as usize;

            if val_len > 0 {
                let mut val_buf = vec![0u8; val_len];
                reader.read_exact(&mut val_buf)?;
                if entry_key == key {
                    return Ok(Some(Some(String::from_utf8_lossy(&val_buf).to_string())));
                }
            } else {
                if entry_key == key {
                    return Ok(Some(None));  // tombstone
                }
            }
        }
        Ok(None)  // not found
    }
}


pub struct LsmEngine {
    mem: MemTable,
    sstables: Vec<SSTable>,  // L0 — newest first
    data_dir: String,
    next_sst_id: u64,
}

impl LsmEngine {
    pub fn new(data_dir: &str) -> std::io::Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        Ok(LsmEngine {
            mem: MemTable::new(1024 * 1024),  // 1MB memtable
            sstables: vec![],
            data_dir: data_dir.to_string(),
            next_sst_id: 0,
        })
    }

    pub fn set(&mut self, key: String, value: String) -> std::io::Result<()> {
        self.mem.set(key, value);
        if self.mem.is_full() {
            self.flush()?;
        }
        Ok(())
    }

    pub fn delete(&mut self, key: String) -> std::io::Result<()> {
        self.mem.delete(key);
        if self.mem.is_full() {
            self.flush()?;
        }
        Ok(())
    }

    pub fn get(&self, key: &str) -> std::io::Result<Option<String>> {
        match self.mem.get(key) {
            Some(Some(v)) => return Ok(Some(v.to_string())),
            Some(None) => return Ok(None),
            None => {}
        }
        for sst in self.sstables.iter().rev() {
            match sst.get(key)? {
                Some(Some(v)) => return Ok(Some(v)),
                Some(None) => return Ok(None),
                None => continue,
            }
        }
        Ok(None)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let path = format!("{}/{}.sst", self.data_dir, self.next_sst_id);
        self.next_sst_id += 1;
        let sst = SSTable::flush(&self.mem, &path)?;
        self.sstables.push(sst);
        self.mem = MemTable::new(1024 * 1024);
        println!("Flushed memtable to {}", path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memtable_set_get() {
        let mut mem = MemTable::new(1024);
        mem.set("name".to_string(), "alice".to_string());
        assert_eq!(mem.get("name"), Some(Some("alice")));
    }

    #[test]
    fn test_memtable_delete_tombstone() {
        let mut mem = MemTable::new(1024);
        mem.set("name".to_string(), "alice".to_string());
        mem.delete("name".to_string());
        assert_eq!(mem.get("name"), Some(None));
    }

    #[test]
    fn test_memtable_missing_key() {
        let mem = MemTable::new(1024);
        assert_eq!(mem.get("missing"), None);  // never existed
    }

    #[test]
    fn test_memtable_is_full() {
        let mut mem = MemTable::new(10);  // tiny capacity
        mem.set("key".to_string(), "value".to_string());  // 8 bytes
        assert!(!mem.is_full());
        mem.set("k2".to_string(), "v2".to_string());  // 4 more = 12 bytes
        assert!(mem.is_full());
    }

    #[test]
    fn test_memtable_overwrite() {
        let mut mem = MemTable::new(1024);
        mem.set("name".to_string(), "alice".to_string());
        mem.set("name".to_string(), "bob".to_string());
        assert_eq!(mem.get("name"), Some(Some("bob")));
    }

    #[test]
    fn test_sstable_flush_and_get() {
        let mut mem = MemTable::new(1024);
        mem.set("age".to_string(), "30".to_string());
        mem.set("city".to_string(), "mumbai".to_string());
        mem.set("name".to_string(), "alice".to_string());

        let path = "/tmp/test_sstable.sst";
        let sst = SSTable::flush(&mem, path).unwrap();

        assert_eq!(sst.get("name").unwrap(), Some(Some("alice".to_string())));
        assert_eq!(sst.get("missing").unwrap(), None);
    }

    #[test]
    fn test_sstable_tombstone() {
        let mut mem = MemTable::new(1024);
        mem.set("name".to_string(), "alice".to_string());
        mem.delete("name".to_string());

        let path = "/tmp/test_sstable_tombstone.sst";
        let sst = SSTable::flush(&mem, path).unwrap();

        assert_eq!(sst.get("name").unwrap(), Some(None));  // tombstone
    }

    #[test]
    fn test_lsm_basic() {
        let mut engine = LsmEngine::new("/tmp/lsm_test_basic").unwrap();
        engine.set("name".to_string(), "alice".to_string()).unwrap();
        engine.set("age".to_string(), "30".to_string()).unwrap();
        assert_eq!(engine.get("name").unwrap(), Some("alice".to_string()));
        assert_eq!(engine.get("missing").unwrap(), None);
    }

    #[test]
    fn test_lsm_delete() {
        let mut engine = LsmEngine::new("/tmp/lsm_test_delete").unwrap();
        engine.set("name".to_string(), "alice".to_string()).unwrap();
        engine.delete("name".to_string()).unwrap();
        assert_eq!(engine.get("name").unwrap(), None);
    }

    #[test]
    fn test_lsm_overwrite() {
        let mut engine = LsmEngine::new("/tmp/lsm_test_overwrite").unwrap();
        engine.set("name".to_string(), "alice".to_string()).unwrap();
        engine.set("name".to_string(), "bob".to_string()).unwrap();
        assert_eq!(engine.get("name").unwrap(), Some("bob".to_string()));
    }

    use super::*;

    #[test]
    fn test_gcounter_increment() {
        let mut c = GCounter::new(1);
        c.increment();
        c.increment();
        assert_eq!(c.value(), 2);
    }

    #[test]
    fn test_gcounter_merge() {
        let mut c1 = GCounter::new(1);
        c1.increment();
        c1.increment();  // node1 incremented twice

        let mut c2 = GCounter::new(2);
        c2.increment();  // node2 incremented once

        c1.merge(&c2);
        assert_eq!(c1.value(), 3);  // 2 + 1
    }

    #[test]
    fn test_gcounter_merge_commutative() {
        let mut c1 = GCounter::new(1);
        c1.increment();

        let mut c2 = GCounter::new(2);
        c2.increment();
        c2.increment();

        let  c1_clone = c1.clone();
        let mut c2_clone = c2.clone();

        c1.merge(&c2);      // c1 merge c2
        c2_clone.merge(&c1_clone);  // c2 merge c1

        // commutativity — same result either way
        assert_eq!(c1.value(), c2_clone.value());
    }

    #[test]
    fn test_gcounter_merge_idempotent() {
        let mut c1 = GCounter::new(1);
        c1.increment();
        c1.increment();

        let c1_clone = c1.clone();
        c1.merge(&c1_clone);  // merge with self

        assert_eq!(c1.value(), 2);  // idempotent — no change
    }

    #[test]
    fn test_pncounter_increment_decrement() {
        let mut c = PNCounter::new(1);
        c.increment();
        c.increment();
        c.decrement();
        assert_eq!(c.value(), 1);
    }

    #[test]
    fn test_pncounter_merge() {
        let mut c1 = PNCounter::new(1);
        c1.increment();
        c1.increment();  // +2

        let mut c2 = PNCounter::new(2);
        c2.increment();
        c2.decrement();  // net 0

        c1.merge(&c2);
        assert_eq!(c1.value(), 2);  // 2 + 0 = 2
    }

    #[test]
    fn test_pncounter_concurrent_decrement() {
        // two nodes decrement concurrently during partition
        let mut c1 = PNCounter::new(1);
        c1.increment();
        c1.increment();
        c1.increment();  // c1 value = 3

        let mut c2 = c1.clone();
        c2.decrements = GCounter::new(2);  // c2 is node 2

        c1.decrement();  // node 1 decrements
        c2.decrement();  // node 2 decrements concurrently

        c1.merge(&c2);
        assert_eq!(c1.value(), 1);  // 3 - 1 - 1 = 1
    }

    #[test]
    fn test_orset_add_contains() {
        let mut s = ORSet::new(1);
        s.add("alice".to_string());
        assert!(s.contains("alice"));
        assert!(!s.contains("bob"));
    }

    #[test]
    fn test_orset_remove() {
        let mut s = ORSet::new(1);
        s.add("alice".to_string());
        s.remove("alice");
        assert!(!s.contains("alice"));
    }

    #[test]
    fn test_orset_add_wins_concurrent() {
        // concurrent add and remove — add should win
        let mut s1 = ORSet::new(1);
        s1.add("alice".to_string());

        let mut s2 = s1.clone();
        s2.node_id = 2;

        s1.remove("alice");      // node 1 removes
        s2.add("alice".to_string()); // node 2 adds concurrently

        s1.merge(&s2);
        assert!(s1.contains("alice"));  // add wins
    }

    #[test]
    fn test_orset_merge_idempotent() {
        let mut s1 = ORSet::new(1);
        s1.add("alice".to_string());

        let s1_clone = s1.clone();
        s1.merge(&s1_clone);
        assert!(s1.contains("alice"));
    }
}