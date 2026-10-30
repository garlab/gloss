//! Exercise the same isolated worker entry point as the app, without a window.
use native_preview::{
    RasterOptions,
    worker::{self, Worker},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

fn main() {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--mermaid-worker")) {
        worker::worker_main().unwrap();
        return;
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let mut worker = Worker::spawn(&std::env::current_exe().unwrap(), "--mermaid-worker").unwrap();
    let cases = [
        (
            "flowchart",
            "flowchart LR\n A[Read Markdown] --> B{Diagram?}\n B -->|Yes| C[Preview]\n B -->|No| D[Text]\n C --> E[Read]\n D --> E\n",
        ),
        (
            "sequence",
            "sequenceDiagram\n participant R as Reader\n participant V as Viewer\n R->>V: Open file\n activate V\n V-->>R: Rendered preview\n deactivate V\n",
        ),
        (
            "class",
            "classDiagram\n class Document {\n +String source\n +render()\n }\n Document --> Renderer\n",
        ),
        (
            "state",
            "stateDiagram-v2\n [*] --> Reading\n Reading --> Rendering: diagram\n Rendering --> Reading: next block\n Reading --> [*]\n",
        ),
        ("er", "erDiagram\n DOCUMENT ||--o{ BLOCK : contains\n"),
        (
            "subgraph",
            "flowchart LR\n subgraph Input\n A[File] --> B[Parser]\n end\n subgraph Output\n C[Diagram] --> D[Preview]\n end\n B --> C\n",
        ),
    ];
    let mut times = Vec::new();
    for (name, source) in cases {
        for light in [false, true] {
            let start = Instant::now();
            let p = worker
                .render(
                    source,
                    light,
                    RasterOptions::scaled(2.),
                    std::time::Duration::from_secs(2),
                    &cancel,
                )
                .unwrap()
                .expect(name);
            let elapsed = start.elapsed().as_secs_f64() * 1000.;
            times.push(elapsed);
            assert!(p.width as usize * p.height as usize <= native_preview::MAX_PIXELS);
            assert_eq!(p.rgba.len(), (p.width * p.height * 4) as usize);
            assert!(p.rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
            println!(
                "{name}, light={light}: {:.2} ms, {}x{} logical, {} raster bytes",
                elapsed,
                p.original_width,
                p.original_height,
                p.rgba.len()
            );
        }
    }
    assert!(
        worker
            .render(
                "pie\n\"A\":1\n",
                false,
                RasterOptions::fit((500, 300)),
                std::time::Duration::from_secs(2),
                &cancel
            )
            .unwrap()
            .is_none()
    );
    assert!(
        worker
            .render(
                "erDiagram\n BOOK {\n string title\n }\n",
                false,
                RasterOptions::fit((500, 300)),
                std::time::Duration::from_secs(2),
                &cancel
            )
            .unwrap()
            .is_none()
    );
    // The renderer stays available after failure and after worker recycling.
    for _ in 0..40 {
        if worker.requests_completed() >= worker::MAX_REQUESTS {
            worker = Worker::spawn(&std::env::current_exe().unwrap(), "--mermaid-worker").unwrap();
        }
        assert!(
            worker
                .render(
                    "flowchart LR; A-->B",
                    false,
                    RasterOptions::fit((500, 300)),
                    std::time::Duration::from_secs(2),
                    &cancel
                )
                .unwrap()
                .is_some()
        );
    }
    cancel.store(true, Ordering::Relaxed);
    assert_eq!(
        worker
            .render(
                "flowchart LR; A-->B",
                false,
                RasterOptions::fit((500, 300)),
                std::time::Duration::from_secs(2),
                &cancel
            )
            .err()
            .unwrap()
            .kind(),
        std::io::ErrorKind::Interrupted
    );
    println!(
        "Mermaid integration passed ({} fixture renders; cold {:.2} ms, warm max {:.2} ms).",
        times.len(),
        times[0],
        times[1..].iter().copied().fold(0., f64::max)
    );
}
