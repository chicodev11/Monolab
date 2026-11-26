use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const EVENT: &str = "log:batch";
pub const WINDOW: Duration = Duration::from_millis(80);
pub const MAX_LINES: usize = 500;

pub struct Batcher<T> {
    tx: Mutex<Sender<T>>,
}

impl<T: Send + 'static> Batcher<T> {
    pub fn start(sink: impl Fn(Vec<T>) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel::<T>();
        std::thread::Builder::new()
            .name("log-batch".into())
            .spawn(move || {
                while let Ok(first) = rx.recv() {
                    sink(gather(&rx, first));
                }
            })
            .expect("log batch thread");
        Self { tx: Mutex::new(tx) }
    }

    pub fn push(&self, item: T) {
        let _ = self.tx.lock().expect("log batch sender poisoned").send(item);
    }
}

fn gather<T>(rx: &Receiver<T>, first: T) -> Vec<T> {
    let mut batch = vec![first];
    let deadline = Instant::now() + WINDOW;
    while batch.len() < MAX_LINES {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        match rx.recv_timeout(deadline - now) {
            Ok(item) => batch.push(item),
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
        }
    }
    batch
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sink() -> (impl Fn(Vec<u32>) + Send + 'static, Receiver<Vec<u32>>) {
        let (tx, rx) = mpsc::channel();
        (move |batch| tx.send(batch).unwrap(), rx)
    }

    #[test]
    fn a_burst_becomes_a_few_batches() {
        let (out, got) = sink();
        let batcher = Batcher::start(out);
        for n in 0..1000 {
            batcher.push(n);
        }
        let mut batches = Vec::new();
        let mut lines = 0;
        while lines < 1000 {
            let batch = got.recv_timeout(Duration::from_secs(2)).expect("batch");
            lines += batch.len();
            batches.push(batch);
        }
        assert!(batches.len() <= 4, "a thousand lines went out in {} batches", batches.len());
        assert!(batches.iter().all(|b| b.len() <= MAX_LINES));
        let flat: Vec<u32> = batches.concat();
        assert_eq!(flat, (0..1000).collect::<Vec<_>>());
    }

    #[test]
    fn a_lone_line_leaves_within_the_window() {
        let (out, got) = sink();
        let batcher = Batcher::start(out);
        let at = Instant::now();
        batcher.push(7);
        let batch = got.recv_timeout(Duration::from_secs(1)).expect("batch");
        assert_eq!(batch, vec![7]);
        assert!(at.elapsed() < WINDOW + Duration::from_millis(60), "{:?}", at.elapsed());
    }

    #[test]
    fn lines_after_the_window_start_a_new_batch() {
        let (out, got) = sink();
        let batcher = Batcher::start(out);
        batcher.push(1);
        std::thread::sleep(WINDOW * 2);
        batcher.push(2);
        assert_eq!(got.recv_timeout(Duration::from_secs(1)).unwrap(), vec![1]);
        assert_eq!(got.recv_timeout(Duration::from_secs(1)).unwrap(), vec![2]);
    }

    #[test]
    fn nobody_emits_a_log_line_by_itself() {
        let lib = include_str!("lib.rs");
        assert!(!lib.contains("emit(\"server-log\""), "something emits server-log line by line");
        assert!(lib.contains("logbatch::EVENT"), "the batch event is not emitted");
    }
}
