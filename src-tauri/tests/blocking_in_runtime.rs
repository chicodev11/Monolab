
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

fn build_and_drop_client() {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("client should build");
    drop(client);
}

fn off_runtime<T, F>(work: F) -> T
where
    F: FnOnce() -> T + Send,
    T: Send,
{
    std::thread::scope(|scope| scope.spawn(work).join().expect("worker thread panicked"))
}

#[test]
fn dropping_a_blocking_client_on_a_worker_would_hang() {
    let (tx, rx) = mpsc::channel();
    tauri::async_runtime::spawn(async move {
        build_and_drop_client();
        let _ = tx.send(());
    });
    assert!(
        rx.recv_timeout(Duration::from_secs(10)).is_err(),
        "If this is safe now, check whether the off_runtime wrapper is still needed."
    );
}

#[test]
fn off_runtime_keeps_the_command_responsive() {
    let (tx, rx) = mpsc::channel();
    tauri::async_runtime::spawn(async move {
        let started = Instant::now();
        off_runtime(build_and_drop_client);
        let _ = tx.send(started.elapsed());
    });
    let took = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("wrapped in off_runtime the command should finish");
    println!("off_runtime took {took:?}");
}
