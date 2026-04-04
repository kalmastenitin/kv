use std::collections::HashMap;
use tokio::fs::read_to_string;

use std::{io, num};

#[derive(Debug)]
enum AppError {
    Io(io::Error),
    Parse(num::ParseIntError),
    Json(serde_json::Error),
    NotFound(String),
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e)
    }
}

impl From<std::num::ParseIntError> for AppError {
    fn from(e: std::num::ParseIntError) -> Self {
        AppError::Parse(e)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Json(e)
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            AppError::Io(e) => write!(f, "IO error: {}", e),
            AppError::Parse(e) => write!(f, "Parse error: {}", e),
            AppError::Json(e) => write!(f, "json error: {}", e),
            AppError::NotFound(k) => write!(f, "Key Not Found: {}", k),
        }
    }
}

async fn load(path: &str) -> Result<HashMap<String, String>, AppError> {
    match read_to_string(path).await {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => return Err(AppError::Io(e)),
        Ok(content) => match serde_json::from_str(&content) {
            Ok(map) => Ok(map),
            Err(e) if e.is_eof() => Ok(HashMap::new()),
            Err(e) => Err(AppError::Json(e)),
        },
    }
}

async fn save(data: &HashMap<String, String>, path: &str) -> Result<(), AppError> {
    let serialized = serde_json::to_string_pretty(&data)?;
    tokio::fs::write(path, serialized).await?;
    Ok(())
}

fn get(data: &HashMap<String, String>, key: &str) -> Result<String, AppError> {
    match data.get(key) {
        Some(value) => Ok(value.to_string()),
        None => Err(AppError::NotFound(key.to_string())),
    }
}

fn set(data: &mut HashMap<String, String>, key: &str, value: &str) -> Result<(), AppError> {
    match data.insert(key.to_string(), value.to_string()) {
        Some(old) => println!("updated to value {}", old),
        None => println!("inserted value {}", value),
    }
    Ok(())
}

fn delete(data: &mut HashMap<String, String>, key: &str) -> Result<(), AppError> {
    match data.remove(key) {
        Some(_) => {
            println!("OK");
            Ok(())
        }
        None => Err(AppError::NotFound(key.to_string())),
    }
}

fn list(data: &HashMap<String, String>) {
    for (key, val) in data.iter() {
        println!("key: {key} val: {val}");
    }
}

#[tokio::main]
async fn main() -> Result<(), AppError> {
    let path = "kv_store.json";
    let mut data = load(&path).await?;

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        println!("Usage: kv <get|set|delete|list> [key] [value]");
        return Ok(());
    }

    match args[1].as_str() {
        "get" => {
            if args.len() < 3 {
                return Err(AppError::NotFound("Missing Key".to_string()));
            }
            println!("{}", get(&data, &args[2])?);
        }
        "set" => {
            if args.len() < 4 {
                return Err(AppError::NotFound("Missing Key or Value".to_string()));
            }
            set(&mut data, &args[2], &args[3])?;
            save(&data, path).await?;
        }
        "delete" => {
            if args.len() < 3 {
                return Err(AppError::NotFound("Missing Key".to_string()));
            }
            delete(&mut data, &args[2])?;
            save(&data, path).await?;
        }
        "list" => list(&data),
        _ => {
            println!("Unknown Command")
        }
    }

    Ok(())
}
