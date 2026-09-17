//! The diagram in a real window on the GPU renderer, dragging a table
//! continuously from a timer, so frame rate can be read off
//! SLINT_DEBUG_PERFORMANCE rather than inferred from the software renderer.
//!
//!     SLINT_DEBUG_PERFORMANCE=refresh_full_speed,console \
//!       cargo run --release -p dbhive-slint --example diagram_live

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use dbhive_slint::diagram_model::{Diagram, Link, Node};
use dbhive_slint::DiagramSpike;
use slint::ComponentHandle;

fn main() {
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
    c.set_nodes(diagram.borrow().node_model());
    c.set_edges(diagram.borrow().edge_model());
    diagram.borrow_mut().select(Some(11));

    // Move table_11 in a circle, one step per timer tick, the way a drag
    // would: each tick changes one node and the edges touching it.
    let d = diagram.clone();
    let start = Instant::now();
    let mut last = (0.0_f32, 0.0_f32);
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(8), move || {
        let t = start.elapsed().as_secs_f32();
        let pos = ((t * 2.0).cos() * 150.0, (t * 2.0).sin() * 100.0);
        d.borrow_mut().drag(11, pos.0 - last.0, pos.1 - last.1);
        last = pos;
    });

    // Stop after a fixed time so the measurement is repeatable.
    let weak = c.as_weak();
    let stop = slint::Timer::default();
    stop.start(slint::TimerMode::SingleShot, Duration::from_secs(8), move || {
        if let Some(c) = weak.upgrade() {
            let _ = c.hide();
        }
        let _ = slint::quit_event_loop();
    });

    c.run().unwrap();
}
