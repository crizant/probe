//! Opt-in startup milestones. The driver owns a disposable session/cache directory.
use std::{io::Write, path::PathBuf, time::Instant};

use gpui_kit::{App, Global, Window};

pub(crate) struct StartupBenchmark {
    started: Instant,
    first_scheduled: bool,
    ready_scheduled: bool,
    restored: bool,
}

impl Global for StartupBenchmark {}

pub(crate) fn directory() -> Option<PathBuf> {
    std::env::var_os("PROBE_STARTUP_BENCHMARK_DIR").map(PathBuf::from)
}

impl StartupBenchmark {
    pub(crate) fn from_environment() -> Option<Self> {
        directory().map(|_| Self {
            started: Instant::now(),
            first_scheduled: false,
            ready_scheduled: false,
            restored: false,
        })
    }
}

pub(crate) fn restored(cx: &mut App) {
    if cx.has_global::<StartupBenchmark>() {
        cx.global_mut::<StartupBenchmark>().restored = true;
    }
}

pub(crate) fn failed(error: &str, cx: &mut App) {
    if cx.has_global::<StartupBenchmark>() {
        println!("PROBE_STARTUP {}", serde_json::json!({"error": error}));
        let _ = std::io::stdout().flush();
        cx.quit();
    }
}

pub(crate) fn render_frame(settled: bool, window: &mut Window, cx: &mut App) {
    if !cx.has_global::<StartupBenchmark>() {
        return;
    }
    let state = cx.global_mut::<StartupBenchmark>();
    let first = !state.first_scheduled;
    let ready = settled && state.restored && !state.ready_scheduled;
    state.first_scheduled = true;
    state.ready_scheduled |= ready;
    if !first && !ready {
        return;
    }
    // Registered during render: the callback runs after this frame has drawn.
    // This measures GPUI rendering, not physical display/compositor presentation.
    window.on_next_frame(move |_, cx| {
        let elapsed = cx
            .global::<StartupBenchmark>()
            .started
            .elapsed()
            .as_secs_f64()
            * 1000.0;
        for (enabled, milestone) in [(first, "first_frame"), (ready, "session_ready")] {
            if enabled {
                println!(
                    "PROBE_STARTUP {}",
                    serde_json::json!({"milestone": milestone, "main_ms": elapsed})
                );
            }
        }
        let _ = std::io::stdout().flush();
        if ready {
            cx.quit();
        }
    });
}
