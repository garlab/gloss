# native-preview

Bounded native Mermaid and SVG rendering for terminal and GUI applications. The
crate returns straight RGBA pixels and logical dimensions. It has no UI framework,
Markdown document model, application cache, broker or background service.

Mermaid supports an ASCII subset: flowcharts, sequence, class and state diagrams,
and ER relationships. Unsupported syntax returns `Ok(None)` through the worker;
callers can retain the original source. SVG rendering uses bundled KaTeX fonts
and rejects external resources, filters, reuse and foreign content.

## Integration

Handle the private worker argument before initializing the application:

```no_run
use native_preview::{RasterOptions, worker::{self, Worker}};
use std::{sync::atomic::AtomicBool, time::Duration};

fn main() -> std::io::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--preview-worker") {
        return worker::worker_main();
    }
    // Run blocking rendering on a background thread in interactive applications.
    let mut worker = Worker::spawn(&std::env::current_exe()?, "--preview-worker")?;
    let cancelled = AtomicBool::new(false);
    let _image = worker.render(
        "flowchart LR; Read-->Preview", false,
        RasterOptions::scaled(2.), Duration::from_secs(2), &cancelled,
    )?;
    // Move image.rgba into the application's image type; no pixel copy is needed.
    Ok(())
}
```

`RasterOptions::fit((width, height))` fits within physical bounds without upscaling.
`RasterOptions::scaled(2.)` requests Retina output while preserving logical SVG
size. Applications can set both fields explicitly. Source and raster limits apply
to every mode: 16 KiB / 256 lines of Mermaid, 128 nodes, 256 edges, 256 KiB SVG,
one million output pixels, and an 8,192-pixel edge. Additional nesting, geometry,
annotation and label limits are enforced before expensive work.

Use `svg::rasterize(source, options)` for bounded local SVG. It accepts no file
paths and does not fetch resources. Only the internal Mermaid route permits
bounded arrow markers. Mermaid parsing and layout are private to the worker;
there is no public in-process Mermaid entry point.

## Worker lifecycle

Applications choose cache sizes, scheduling, idle retirement and total invocation
budgets. Each worker handles at most `worker::MAX_REQUESTS` (32) requests; check
`requests_completed()` and replace the worker when needed. Rejected syntax does
not break the worker. An I/O error, cancellation or deadline stops and reaps it
before returning; an errored handle must be replaced. Dropping a healthy handle
also stops and reaps its child. Process creation itself is outside the per-render
deadline.

The child receives source, theme and raster options over bounded private pipes;
its inherited environment is cleared. Unix workers disable core dumps and have
an eight-second cumulative CPU backstop. On macOS the sandbox denies file-data
reads, writes, networking and child processes, and the parent samples resident
memory against 256 MiB. Other platforms retain input/output bounds and deadlines,
but do not yet have equivalent OS sandbox and resident-memory enforcement.

The library owns these mechanisms. A terminal app can retain a five-second total
budget; a desktop app can reuse a worker through a bounded queue and retire it on
idle. Neither policy is imposed on the other application.

## Development

This package is maintained in the mdviewer workspace under `crates/native-preview`.
It depends on the published `katex-native` and `mermaid-rs-renderer` crates. Shared
renderer, SVG and protocol tests belong here; application policy tests stay with
their consumers. Run `cargo test -p native-preview` from the workspace. On macOS,
worker tests must run outside an enclosing sandbox so the child can install its
own sandbox.

MIT; see [LICENSE](LICENSE). `katex-native` includes KaTeX fonts under OFL-1.1.
