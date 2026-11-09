# Documentation

Start with the [project README](../README.md) for installation, command-line
options, supported inputs, terminal compatibility, and copying.

## Feature references

These describe implemented behavior, limits, fallbacks, and validation.

| Topic | Reference |
| --- | --- |
| Code languages, embedded syntax, and highlighting limits | [Syntax highlighting](syntax-highlighting.md) |
| Cell layout, inline math, and narrow-terminal fallbacks | [Tables](tables.md) |
| Local PNG/JPEG/SVG files, fonts, and decoding limits | [Images](images.md) |
| Native diagram coverage, worker limits, and fitting | [Mermaid](mermaid.md) and [Gantt](gantt.md) |
| Go records, position selection, and board rendering | [SGF](sgf.md) |
| Alerts, front matter, details, links, and math delimiters | [Markdown extensions](markdown-extensions.md) |
| Interactive navigation, reloads, and resource limits | [Pager](pager.md) |
| Geometry, themes, graphics selection, and overrides | [Terminal discovery](terminal-detection.md) |

## Contributing and releases

- [Architecture](architecture.md): module boundaries and output contracts.
- [Design decisions](feature-design.md): rendering choices, dependency boundaries,
  and performance requirements for changes.
- [Roadmap](roadmap.md): remaining work and deferred features.
- [Release guide](releasing.md): checks, registry publication, and Homebrew setup.
- [Native ABC design](abc-native-design.md): deferred music-notation research;
  ABC rendering is not implemented.
- [LaTeX preview design](latex-design.md): proposed native LaTeX front end
  shared with Plastext; `.tex` files currently render as Markdown.

## Performance evidence

The feature references include measurements of their implementation costs.
The following reports cover changes across rendering paths:

| Report | Scope |
| --- | --- |
| [Initial baseline](performance-baseline.md) | Fixed Glow comparison and [raw samples](../bench/baseline-0d4c347.json) |
| [Rendering optimizations](performance-optimizations.md) | Allocation, memory, and executable-size reductions |
| [Streaming and graphics](streaming-and-graphics.md) | Completed-block output, math caching, compression, and native-pixel transport |
| [Text performance](text-performance.md) | Wrapping and benchmark timing |

Measurements apply to the recorded revision, hardware, inputs, and methodology;
they are not current performance guarantees. Some build identifiers predate the
public commit history. The fixed baseline and raw samples retain the former
`mdviewer` name. For current runs, use `gloss` in binary paths and benchmark
renderer names, with dependencies from crates.io. See
[bench/run.py](../bench/run.py), [bench/generate.py](../bench/generate.py), and
[bench/pager.py](../bench/pager.py) for reproduction tools. Keep generated inputs
and new local measurements outside the repository.
