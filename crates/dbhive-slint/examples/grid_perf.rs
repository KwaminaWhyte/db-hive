//! How the results grid copes with a maximum-size result.
//!
//! Builds MAX_RESULT_ROWS (50,000) x 8 columns, renders the top, jumps to the
//! middle and renders again, timing each and reporting process memory. Uses
//! the software renderer so it runs headless; a GPU renderer draws faster, so
//! these timings are an upper bound on layout and model cost.
//!
//!     cargo run --release -p dbhive-slint --example grid_perf -- <out-prefix>

use std::rc::Rc;
use std::time::Instant;

use dbhive_slint::grid_model::{layout_columns, Cell, LazyRows};
use dbhive_slint::GridSpike;
use slint::platform::software_renderer::{
    MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType,
};
use slint::platform::{Platform, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, ModelRc};

struct P(Rc<MinimalSoftwareWindow>, Instant);
impl Platform for P {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
    fn duration_since_start(&self) -> core::time::Duration {
        self.1.elapsed()
    }
}

fn rss_mb() -> f64 {
    let pid = std::process::id().to_string();
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().unwrap_or(0.0) / 1024.0
}

fn render(w: &MinimalSoftwareWindow, pw: u32, ph: u32, path: &str) -> f64 {
    let t = Instant::now();
    slint::platform::update_timers_and_animations();
    let mut buf = vec![PremultipliedRgbaColor::default(); (pw * ph) as usize];
    w.request_redraw();
    w.draw_if_needed(|r| {
        r.render(&mut buf, pw as usize);
    });
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let mut ppm = format!("P6\n{} {}\n255\n", pw, ph).into_bytes();
    for px in &buf {
        ppm.extend_from_slice(&[px.red, px.green, px.blue]);
    }
    std::fs::write(path, ppm).unwrap();
    ms
}

fn main() {
    let prefix = std::env::args().nth(1).unwrap_or_else(|| "grid".into());
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(P(window.clone(), Instant::now()))).unwrap();
    let (pw, ph) = (2400u32, 1400u32);
    window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 2.0 });
    window.set_size(slint::PhysicalSize::new(pw, ph));

    let base = rss_mb();

    let t = Instant::now();
    const ROWS: usize = 50_000;
    let data: Vec<Vec<Cell>> = (0..ROWS)
        .map(|i| {
            vec![
                Some(i.to_string()),
                Some(format!("customer_{:05}", i)),
                Some(format!("user{}@example.com", i)),
                if i % 7 == 0 { None } else { Some(format!("{}.{:02}", i % 1000, i % 100)) },
                Some(if i % 3 == 0 { "paid" } else { "pending" }.to_string()),
                Some(format!("2026-{:02}-{:02}", i % 12 + 1, i % 28 + 1)),
                if i % 5 == 0 { None } else { Some("Accra".to_string()) },
                Some(format!("note for row {}, with enough text to need eliding", i)),
            ]
        })
        .collect();
    let build_ms = t.elapsed().as_secs_f64() * 1000.0;
    let after_data = rss_mb();

    let titles: Vec<String> = ["id", "name", "email", "total", "status", "created_at", "city", "note"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let widths = [80.0, 170.0, 220.0, 100.0, 100.0, 120.0, 100.0, 340.0];
    let (columns, total) = layout_columns(&titles, &widths);

    let c = GridSpike::new().unwrap();
    c.set_columns(columns);
    c.set_total_width(total);
    let t = Instant::now();
    c.set_rows(ModelRc::new(LazyRows::new(Rc::new(data))));
    let bind_ms = t.elapsed().as_secs_f64() * 1000.0;
    c.show().unwrap();

    let top_ms = render(&window, pw, ph, &format!("{}-top.ppm", prefix));
    let after_top = rss_mb();

    // Jump to the middle of the result: 25,000 rows * 28px.
    c.set_scroll_y(-(25_000.0 * 28.0));
    let mid_ms = render(&window, pw, ph, &format!("{}-mid.ppm", prefix));

    // And to the very end.
    c.set_scroll_y(-((ROWS as f32 - 20.0) * 28.0));
    let end_ms = render(&window, pw, ph, &format!("{}-end.ppm", prefix));
    let after_scroll = rss_mb();

    println!("rows                   {}", ROWS);
    println!("build raw strings      {:>8.1} ms", build_ms);
    println!("bind model             {:>8.3} ms", bind_ms);
    println!("first frame (top)      {:>8.1} ms", top_ms);
    println!("jump to row 25,000     {:>8.1} ms", mid_ms);
    println!("jump to last row       {:>8.1} ms", end_ms);
    println!("rss baseline           {:>8.1} MB", base);
    println!("rss with raw data      {:>8.1} MB  (+{:.1})", after_data, after_data - base);
    println!("rss after first frame  {:>8.1} MB  (+{:.1})", after_top, after_top - after_data);
    println!("rss after scrolling    {:>8.1} MB  (+{:.1})", after_scroll, after_scroll - after_top);
}
