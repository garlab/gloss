# Changelog

## 0.1.0 (unreleased)

- Render Markdown with colors, readable margins, bounded line widths, tables,
  syntax highlighting, links, alerts, and details blocks.
- Display native math, Mermaid diagrams, local PNG/JPEG/SVG images, and SGF Go
  records through Kitty graphics, with source or text fallbacks.
- Open standalone code, diagrams, images, and Go records. Detect common code
  languages, `Cargo.lock`, Dockerfile, and Containerfile; show code line numbers.
- Read interactively with the Unix pager, literal search, resize reflow, and
  live file watching. Direct output remains the default.
- Export original source or plain text for copying and pipes. Bound expensive
  rendering work, image transport, caches, and worker lifetimes.

The Cargo package is `gloss-cli`; the executable is `gloss`. No browser,
JavaScript runtime, or external TeX installation is required.
