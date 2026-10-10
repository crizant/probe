// Shared hang guard for streaming network tests, independent of Tokio's clock.

use std::{future::Future, time::Duration};
use tokio::sync::oneshot;

// This bounds genuine hangs, not protocol deadlines or runner scheduling latency.
const HANG_GUARD: Duration = Duration::from_secs(30);

pub async fn bounded<T>(future: impl Future<Output = T>) -> T {
    let (done, waiting) = std::sync::mpsc::channel();
    let (expired, deadline) = oneshot::channel();
    let watchdog = std::thread::spawn(move || {
        if waiting.recv_timeout(HANG_GUARD).is_err() {
            let _ = expired.send(());
        }
    });
    let result = tokio::select! {
        result = future => result,
        _ = deadline => panic!("WebSocket test exceeded its wall-clock hang guard"),
    };
    // Cancellation wakes the watchdog immediately, including with paused Tokio time.
    let _ = done.send(());
    watchdog.join().unwrap();
    result
}
