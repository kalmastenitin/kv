//Raft log
use std::fs::{File, OpenOptions};
use std::io::{Write, Read, BufWriter, BufReader};

use crate::rpc::LogEntry;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
pub enum WalRecord {
    Term(u64),
    Vote(Option<u64>),
    AppendLog(LogEntry),
}

pub struct Wal {
    writer: BufWriter<File>,
    path: String,
}

impl Wal {
    pub fn open(path: &str) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Ok(Wal {
            writer: BufWriter::new(file),
            path: path.to_string(),
        })
    }

    pub fn append(&mut self, record: &WalRecord) -> std::io::Result<()> {
        let data = serde_json::to_vec(record).unwrap();
        let len = data.len() as u32;
        self.writer.write_all(&len.to_le_bytes())?;
        self.writer.write_all(&data)?;
        self.writer.flush()?;  // ← critical: flush before returning
        Ok(())
    }

    pub fn recover(path: &str) -> std::io::Result<Vec<WalRecord>> {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => return Ok(vec![]),  // no WAL file = fresh node
        };
        let mut reader = BufReader::new(file);
        let mut records = vec![];

        loop {
            let mut len_buf = [0u8; 4];
            match reader.read_exact(&mut len_buf) {
                Ok(_) => {}
                Err(_) => break,  // end of file
            }
            let len = u32::from_le_bytes(len_buf) as usize;
            let mut data = vec![0u8; len];
            reader.read_exact(&mut data)?;
            let record: WalRecord = serde_json::from_slice(&data).unwrap();
            records.push(record);
        }
        Ok(records)
    }
}