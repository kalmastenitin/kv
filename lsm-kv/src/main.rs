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
}