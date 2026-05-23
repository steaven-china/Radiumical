//! Performance monitor — tracks FPS, frame times, buffer sizes.
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Instant;

pub struct PerfMonitor {
    pub fps: f32,
    pub frame_time_ms: f32,
    pub output_lines: usize,
    pub render_time_ms: f32,
    last_frame: Instant,
    frame_count: u32,
    frame_acc: f32,
}

impl PerfMonitor {
    pub fn new() -> Self {
        Self {
            fps: 0.0, frame_time_ms: 0.0, output_lines: 0,
            render_time_ms: 0.0, last_frame: Instant::now(),
            frame_count: 0, frame_acc: 0.0,
        }
    }

    pub fn tick(&mut self, render_us: u64, output_lines: usize) {
        self.render_time_ms = render_us as f32 / 1000.0;
        self.output_lines = output_lines;
        let elapsed = self.last_frame.elapsed();
        self.last_frame = Instant::now();
        self.frame_time_ms = elapsed.as_secs_f32() * 1000.0;
        self.frame_acc += elapsed.as_secs_f32();
        self.frame_count += 1;
        if self.frame_acc >= 1.0 {
            self.fps = self.frame_count as f32 / self.frame_acc;
            self.frame_count = 0;
            self.frame_acc = 0.0;
        }
    }

    pub fn report(&self) -> String {
        format!(
            "FPS: {:.0} | Frame: {:.1}ms | Render: {:.1}ms | Lines: {}",
            self.fps, self.frame_time_ms, self.render_time_ms, self.output_lines
        )
    }
}

fn perf() -> &'static Mutex<PerfMonitor> {
    static P: OnceLock<Mutex<PerfMonitor>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(PerfMonitor::new()))
}

pub fn tick(render_us: u64, output_lines: usize) {
    if let Ok(mut p) = perf().lock() { p.tick(render_us, output_lines); }
}

pub fn report() -> String {
    perf().lock().map(|p| p.report()).unwrap_or_default()
}
