//! The diagram canvas at a realistic schema size, dragged with real pointer
//! events.
//!
//!     cargo run --release -p dbhive-slint --example diagram_perf -- <out-prefix>

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use dbhive_slint::diagram_model::{Diagram, Link, Node};
use dbhive_slint::DiagramSpike;
use slint::platform::software_renderer::{
    MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType,
};
use slint::platform::{Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, LogicalPosition};

struct P(Rc<MinimalSoftwareWindow>, Instant);
impl Platform for P {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
    fn duration_since_start(&self) -> core::time::Duration {
        self.1.elapsed()
    }
}

thread_local! {
    static BUF: std::cell::RefCell<Vec<PremultipliedRgbaColor>> = std::cell::RefCell::new(Vec::new());
}

fn frame(w: &MinimalSoftwareWindow, pw: u32, ph: u32, save: Option<&str>) -> f64 {
    let t = Instant::now();
    slint::platform::update_timers_and_animations();
    BUF.with(|b| {
        let mut buf = b.borrow_mut();
        if buf.len() != (pw * ph) as usize {
            *buf = vec![PremultipliedRgbaColor::default(); (pw * ph) as usize];
        }
    });
    let partial = std::env::var("PARTIAL").is_ok();
    if !partial {
        w.request_redraw();
    }
    BUF.with(|b| {
        let mut buf = b.borrow_mut();
        w.draw_if_needed(|r| {
            r.render(&mut buf, pw as usize);
        });
    });
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    if let Some(path) = save {
        let buf = BUF.with(|b| b.borrow().clone());
        let mut ppm = format!("P6\n{} {}\n255\n", pw, ph).into_bytes();
        for px in &buf {
            ppm.extend_from_slice(&[px.red, px.green, px.blue]);
        }
        std::fs::write(path, ppm).unwrap();
    }
    ms
}

fn main() {
    let prefix = std::env::args().nth(1).unwrap_or_else(|| "diagram".into());
    // PARTIAL=1 repaints only the dirty region into a kept buffer, which is
    // how an interactive window behaves; the default repaints everything.
    let partial = std::env::var("PARTIAL").is_ok();
    let window = MinimalSoftwareWindow::new(if partial {
        RepaintBufferType::ReusedBuffer
    } else {
        RepaintBufferType::NewBuffer
    });
    slint::platform::set_platform(Box::new(P(window.clone(), Instant::now()))).unwrap();
    let (pw, ph) = (2400u32, 1400u32);
    window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 2.0 });
    window.set_size(slint::PhysicalSize::new(pw, ph));

    // 60 tables of 3-9 columns in a grid, with ~90 relationships.
    const TABLES: usize = 60;
    let nodes: Vec<Node> = (0..TABLES)
        .map(|i| {
            let n = 3 + i % 7;
            Node {
                title: format!("table_{:02}", i),
                columns: (0..n)
                    .map(|c| if c == 0 { "PK  id".into() } else { format!("column_{}   varchar", c) })
                    .collect(),
                x: 20.0 + (i % 10) as f32 * 250.0,
                y: 20.0 + (i / 10) as f32 * 230.0,
                width: 210.0,
                height: Node::height_for(n),
            }
        })
        .collect();
    let links: Vec<Link> = (1..TABLES)
        .flat_map(|i| {
            let mut v = vec![Link { from: (i * 7) % i, to: i }];
            if i % 2 == 0 {
                v.push(Link { from: i / 2, to: i });
            }
            v
        })
        .collect();

    let c = DiagramSpike::new().unwrap();
    let diagram = Rc::new(RefCell::new(Diagram::new(nodes, links)));
    c.set_canvas_width(2600.0);
    c.set_canvas_height(1500.0);
    c.set_shadows(std::env::var("NO_SHADOWS").is_err());
    c.set_nodes(diagram.borrow().node_model());
    c.set_edges(diagram.borrow().edge_model());
    {
        let d = diagram.clone();
        c.on_node_dragged(move |i, dx, dy| d.borrow_mut().drag(i as usize, dx, dy));
    }
    {
        let d = diagram.clone();
        c.on_node_clicked(move |i| d.borrow_mut().select(Some(i as usize)));
    }
    c.show().unwrap();

    let first = frame(&window, pw, ph, Some(&format!("{}-before.ppm", prefix)));

    // Drag table_11 by its header with real pointer events.
    let start = LogicalPosition::new(20.0 + 250.0 + 40.0, 20.0 + 230.0 + 15.0);
    window.dispatch_event(WindowEvent::PointerPressed { position: start, button: PointerEventButton::Left });
    let mut drag_frames = Vec::new();
    for step in 1..=30 {
        let p = LogicalPosition::new(start.x + step as f32 * 8.0, start.y + step as f32 * 5.0);
        window.dispatch_event(WindowEvent::PointerMoved { position: p });
        drag_frames.push(frame(&window, pw, ph, None));
    }
    let end = LogicalPosition::new(start.x + 240.0, start.y + 150.0);
    window.dispatch_event(WindowEvent::PointerReleased { position: end, button: PointerEventButton::Left });
    let after = frame(&window, pw, ph, Some(&format!("{}-after.ppm", prefix)));

    // Select it to highlight its relationships.
    window.dispatch_event(WindowEvent::PointerPressed { position: end, button: PointerEventButton::Left });
    window.dispatch_event(WindowEvent::PointerReleased { position: end, button: PointerEventButton::Left });
    let selected = frame(&window, pw, ph, Some(&format!("{}-selected.ppm", prefix)));

    let d = diagram.borrow();
    drag_frames.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let avg = drag_frames.iter().sum::<f64>() / drag_frames.len() as f64;
    println!("tables                 {}", d.nodes.len());
    println!("relationships          {}", d.edge_count());
    println!("first frame            {:>7.1} ms", first);
    println!("drag frame avg         {:>7.1} ms", avg);
    println!("drag frame p95         {:>7.1} ms", drag_frames[(drag_frames.len() * 95) / 100 - 1]);
    println!("after drag             {:>7.1} ms", after);
    println!("after select           {:>7.1} ms", selected);
    println!("table_11 moved to      ({:.0}, {:.0})  expected (510, 400)", d.nodes[11].x, d.nodes[11].y);
    println!("selected               {:?}", d.selected);
}
