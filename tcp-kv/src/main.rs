use std::collections::HashMap;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, mpsc};
use std::{thread, vec};
use std::{io, num};
use std::io::Read;


#[derive(Debug)]
enum AppError {
    Io(io::Error),
    Parse(num::ParseIntError),
    Json(serde_json::Error),
    NotFound(String),
    
}

struct ThreadPool {
    workers: Vec<Worker>,
    sender: mpsc::Sender<TcpStream>,
}

struct Worker {
    id: usize,
    thread: thread::JoinHandle<()>,
}

impl ThreadPool {
    fn new(size: usize, store: Arc<Mutex<HashMap<String,String>>>) -> ThreadPool {
        let (tx, rx) = mpsc::channel::<TcpStream>();
        let shared_rx = Arc::new(Mutex::new(rx));
        let mut pool = vec![];
        for i in 0..size {
            let thread_rx = Arc::clone(&shared_rx);
            let worker_store = Arc::clone(&store); 
            let handle = thread::spawn(move || {
                loop {
                    let stream = thread_rx.lock().unwrap().recv().unwrap();
                    handle_connection(stream, Arc::clone(&worker_store));
                }
            });
            let worker = Worker {
                id: i,
                thread: handle,
            };
            pool.push(worker);
        }
        ThreadPool {
            workers: pool,
            sender: tx,
        }
    }

    fn execute(&self, stream: TcpStream) {
        self.sender.send(stream).unwrap();
    }
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


fn get(data: Arc<Mutex<HashMap<String, String>>>, key: &str) -> Result<String, String> {
    match data.lock().unwrap().get(key) {
        Some(value) => Ok(value.to_string()),
        None => Ok(format!("{} Not Found", key)),
    }
    
}

fn set(data: Arc<Mutex<HashMap<String, String>>>, key: &str, value: &str) -> Result<(), AppError> {
    match data.lock().unwrap().insert(key.to_string(), value.to_string()) {
        Some(old) => println!("updated to value {}", old),
        None => println!("inserted value {}", value),
    }
    Ok(())
}



fn list(data: Arc<Mutex<HashMap<String, String>>>) -> String {
    data.lock()
        .unwrap()
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<String>>()
        .join(", ")
}

fn handle_connection(mut stream: TcpStream, store: Arc<Mutex<HashMap<String, String>>>) -> Result<(), AppError>  {
    let mut buf = [0u8; 512];
    let n = stream.read(&mut buf).unwrap();
    let command = String::from_utf8_lossy(&buf[..n]);
    let response = if command.contains("get") {
        let key = command.trim().split(" ").last().unwrap();
        let value = get(store, key).unwrap();
        value.to_string()

    } else if command.contains("set") {
        let value : Vec<&str> = command.trim().split(" ").collect::<Vec<&str>>();
        if value.len() < 3 {
            "Missing Key or Value".to_string()
        } else {
            set(store, value[1], value[2])?;
            "OK".to_string()
        }
        
    } else if command.contains("list") {
        list(store)
    } else {
        "Unknown command".to_string()
    };

    let http = format!("HTTP/1.1 200 OK\r\n\r\n{}\r\n", response);
    stream.write_all(http.as_bytes()).unwrap();
    Ok(())
}


fn main() -> Result<(), AppError> {
    let store: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
    let listener = TcpListener::bind("127.0.0.1:8080").unwrap();

    println!("Listening on port 8080");
    let thread_pool = ThreadPool::new(4, store);

    for stream in listener.incoming() {
        thread_pool.execute(stream.unwrap());
    }

    Ok(())
}
