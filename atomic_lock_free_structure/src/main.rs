use std::{
    sync::{Arc, atomic::AtomicBool, atomic::Ordering},
    thread,
    time::Duration,
};

fn main() {
    let running = Arc::new(AtomicBool::new(true));
    let state = Arc::clone(&running);
    thread::spawn(move || {
        loop {
            if state.load(Ordering::Acquire) {
                println!("thread is running");
                thread::sleep(Duration::from_millis(1000));
            } else {
                println!("worker stopping");
                return;
            }
        }
    });

    for _i in 1..30 {
        println!("running main thread");
        thread::sleep(Duration::from_secs(2));
        running.store(false, Ordering::Release);
    }
}
