# Contributing

Issues and pull requests are welcome. This guide covers building gloss from a
checkout, the checks to run, and where behavior and decisions are documented.

## Building from a checkout

You need Rust 1.90 or newer and a C compiler for the syntax grammars. Run Cargo
from the repository root.

```sh
cargo install --path . --locked
gloss tests/fixtures/math.md
gloss --show-math-source tests/fixtures/inline.md
gloss tests/fixtures/style.md
gloss tests/fixtures/code.md
gloss tests/fixtures/tables.md
gloss tests/fixtures/images.md
gloss tests/fixtures/mermaid.md
gloss tests/fixtures/gantt-time.md
gloss tests/fixtures/sgf.md
gloss tests/fixtures/extensions.md
gloss tests/fixtures/latex.tex
```

To run without installing, use `cargo run --locked -- <arguments>`.

gloss builds on three libraries published on crates.io:
[`katex-native`](https://crates.io/crates/katex-native) for equation layout,
[`document-graphics`](https://crates.io/crates/document-graphics) (with its
`images` and `sgf` features) for diagrams, boards, and images, and
[`latex-reader`](https://crates.io/crates/latex-reader) for LaTeX documents.
To change one of them alongside gloss, place its checkout beside gloss and add
an override to an untracked `.cargo/config.toml`:

```toml
[patch.crates-io]
latex-reader = { path = "../latex-reader" }
document-graphics = { path = "../document-graphics" }
document-graphics-mermaid = { path = "../document-graphics-mermaid" }
```

Remove the overrides and run `cargo check` to return to the published versions
in `Cargo.lock`. Changes that need a library change land there first, in a
published version.

## Checks

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo +1.90.0 check --locked --all-targets
cargo build --locked
python3 tests/terminal_smoke.py target/debug/gloss  # Unix, Python 3
python3 tests/pager_smoke.py target/debug/gloss
```

Rust tests cover parsing, source preservation, wrapping, math, LaTeX, and
output contracts. The pseudo-terminal tests cover graphics selection,
fallbacks, narrow windows, Unicode, reading width, themes, and image placement;
the pager tests add input, search, image cropping and reuse, resizing, watched
saves, and terminal restoration. They check the protocol, not what a terminal
draws: check font alignment, scrolling, and selection in Ghostty or Kitty when
changing rendering.

[CI](.github/workflows/ci.yml) runs the Rust checks on Linux, macOS, and
Windows, the pseudo-terminal checks on Linux and macOS, and the minimum Rust
version and package verification on Linux.

## Guidelines

- Keep everything in the repository suitable for publication: generic examples,
  relative paths, and no personal or machine-specific details. Local overrides
  belong in untracked Cargo configuration.
- Source stays authoritative: unsupported input keeps its source, exports stay
  exact, and failures stay local to the block that failed. See the
  [architecture](docs/architecture.md#contracts-to-preserve).
- Bound new work and measure its cost. The
  [resource and performance requirements](docs/feature-design.md#resource-and-performance-requirements)
  explain what to measure and when a change needs review; `bench/` holds the
  tools, and inputs and results stay outside the repository.
- Update documentation with behavior: user-facing behavior in the
  [README](README.md), details and limits in the matching
  [feature reference](docs/README.md#feature-references), module boundaries in
  the [architecture](docs/architecture.md), decisions in
  [design decisions](docs/feature-design.md), and remaining work in the
  [roadmap](docs/roadmap.md). Add user-facing changes to the
  [changelog](CHANGELOG.md).

## Releases

Maintainers publish releases from tags on `main`; see
[releasing](docs/releasing.md).
