# Gantt charts

Mermaid Gantt charts use the shared native renderer. Both Markdown fences and
standalone `.mmd` / `.mermaid` files work, including pager/watch mode and
`--show-mermaid-source`. The canvas is transparent and labels follow terminal
cell size. See the [calendar fixture](../tests/fixtures/gantt.md) and
[time fixture](../tests/fixtures/gantt-time.md) for examples.

```mermaid
gantt
    title Release
    dateFormat YYYY-MM-DD
    axisFormat %d %b
    excludes weekends
    section Delivery
        Build  :active, build, 2026-11-02, 1w
        Review :crit, after build, 2d
        Ship   :milestone, 0d
```

Supported scheduling:

- Strict ISO dates, clock formats `HH:mm`, `HH:mm:ss`, `HH:mm:ss.SSS`, and full
  date-time formats combining `YYYY-MM-DD` with those clock formats, separated
  by a space or literal `T` (`[T]` is also accepted).
- Explicit ends and hour/minute/second/millisecond durations (`h`, `m`, `s`,
  `ms`). Fractions such as `1.5h` work when they resolve to whole milliseconds.
  Integral day/week/month/year durations (`d`, `w`, `M`, `y`) also work;
  months and years respect the calendar.
- Omitted starts continue from the previous task's end, across sections too.
- `after ID` or multiple dependencies, including forward references. IDs are
  case sensitive; unknown, duplicate and cyclic references retain source.
- Titles, sections, combined `done`/`active`/`crit`/`milestone` tags and accented labels.
- Weekend exclusions and explicit ISO holidays, including repeated `excludes`
  lines. Explicit end dates stay exact. Trailing excluded days are omitted from
  the visible bar while dependencies use the next included day. Excluded dates
  are not shaded as separate columns.
- `axisFormat` supports `%Y`, `%y`, `%m`, `%d`, `%e`, `%b`, `%H`, `%M`, `%S`,
  `%L` (milliseconds), `%%` and literal text. Date fields require dated input.
  Month abbreviations are English; tick spacing adjusts to label width.
- `todayMarker off`; no current-date marker is drawn by default.

Timezones, AM/PM, other date formats, fractional `d`/`w`/`M`/`y` durations,
fractional milliseconds, `until`, custom tick
intervals, weekday exclusions, custom weekends, `includes`, inclusive end dates,
vertical markers, click handlers and front matter retain the complete source.
The first task needs a date/time or a resolvable dependency; rendering never guesses
today's date. Lowercase `m` means minutes; uppercase `M` means calendar months.

```mermaid
gantt
    title Morning release
    dateFormat HH:mm
    axisFormat %H:%M
    Build :build, 09:00, 1.5h
    Check :after build, 15m
    Ship  :milestone, 0ms
```

Clock-only schedules use anonymous day zero. Durations and dependencies can
cross midnight, and default axis labels distinguish days with `+1d`, `+2d`, etc.
An explicit clock end before its start retains source; use a duration or a dated
timestamp for an overnight task. Weekend/holiday exclusions, month/year durations
and date axis fields require dated input. Exclusions are evaluated at whole-day
offsets from a task's start time, following Mermaid's duration semantics.

The shared library permits 128 tasks, 16 sections, 128 excluded-date entries,
36,600 days per task/chart, 128 bytes per axis format and 1 KiB per label/title.
The existing 16 KiB Mermaid source limit, worker deadline/memory limits,
one-million-pixel raster cap and 64 MiB output budget still apply. Large charts
use the existing vertical bands and may shrink at the raster cap. Parsing and
scheduling stay in the worker. The cache and plain/source exports are unchanged.

The backend uses integer milliseconds and iterative dependency resolution.
Relative pixel offsets preserve short tasks even near year 9999. Calendar
work and axis ticks are bounded without scanning every day or millisecond in a
long schedule. No date library,
JavaScript runtime or additional helper process is introduced.

## Measurements

Measured on 2026-11-05 with release builds, thin LTO/one codegen unit, Rust 1.99.0,
Apple M2 Pro / 16 GiB and macOS 26.4.1. Medians of five fresh CLI processes after
one warm-up per build, alternating builds, with a drained 100×40 PTY, 10×20-pixel
cells, 80 content columns and two-column margins. Times include startup, worker,
encoding and PTY output; they exclude emulator painting and scrollback storage.
Builds and tests finished before timing. Inputs and raw results remain external;
the fixed [performance baseline](performance-baseline.md) is unchanged.

| Unchanged case | Before | Gantt build | Peak RSS before → after |
| --- | ---: | ---: | ---: |
| Mixed Markdown, 10 MiB | 938.94 ms | 949.15 ms | 58.36 → 58.31 MiB |
| 1,000 equations | 388.27 ms | 386.16 ms | 11.55 → 11.50 MiB |
| 13 Mermaid diagrams | 45.92 ms | 45.18 ms | 12.78 → 12.58 MiB |
| Dense flowchart | 262.56 ms | 261.25 ms | 19.98 → 19.30 MiB |
| Four SGF boards | 9.61 ms | 10.03 ms | 7.88 → 7.81 MiB |
| Local SVG | 4.28 ms | 4.40 ms | 4.33 → 4.33 MiB |

Thirty-two existing terminal/export cases remain byte-identical, including both
themes and source/text exports. Small timing differences are observations, not
claimed speedups. The binary grows from 17,062,336 to 17,095,504 bytes: 32.4 KiB,
with no additional dependencies.

| Gantt case | Previous code fallback | Native rendering | Peak RSS | Output |
| --- | ---: | ---: | ---: | ---: |
| Three-chart fixture | 4.37 ms | 20.15 ms | 10.59 MiB | 66,683 bytes |
| 128 tasks | 3.68 ms | 31.83 ms | 18.64 MiB | 176,014 bytes / four bands |
| Long span, 25,000 working days | 3.27 ms | 13.14 ms | 9.36 MiB | 8,143 bytes |
| 100 repeated charts | 4.66 ms | 43.31 ms | 9.52 MiB | 1,612,806 bytes |
| 40 unique charts | 4.29 ms | 66.64 ms | 13.41 MiB | 513,346 bytes |

The last case renders 32 charts and retains source for eight, as required by
the existing request budget. RSS above is the `wait4` process statistic, not
simultaneous CLI/worker memory. A separate three-sample run with process-tree
sampling observed combined peaks of 14.42 MiB for 100 repeated charts and
23.27 MiB for 40 unique charts. Sampling can miss short-lived peaks and affects
latency; those timings are not used above.

Reproduce with `bench/generate.py` and `bench/run.py` using an external work
directory prepared as described in the baseline. Select `--renderer
gloss/terminal-mermaid --corpus 'mermaid-gantt-*.md'` and
`--diagram-tiles mermaid-gantt-tasks-128.md=4` at these cell dimensions.
Use `--mermaid-mode off` to measure the disabled path; use `--process-tree` in a
separate run for sampled combined memory, because sampling affects short timings.

### Sub-day extension

The extension was measured on the same machine and date with the same release
profile, PTY and five-sample alternating methodology. The before build already
includes native day-based Gantt rendering. Thirty-six terminal/export cases stay
byte-identical, including the existing Gantt fixture in both themes. These are
observations from one run; small differences are not claimed speedups or regressions.

| Unchanged case | Day-only build | Sub-day build | Peak RSS before → after |
| --- | ---: | ---: | ---: |
| Mixed Markdown, 10 MiB | 999.17 ms | 998.90 ms | 58.16 → 58.09 MiB |
| 1,000 equations | 404.60 ms | 417.04 ms | 11.69 → 12.12 MiB |
| 13 Mermaid diagrams | 48.47 ms | 48.46 ms | 13.80 → 15.02 MiB |
| Dense flowchart | 271.56 ms | 273.91 ms | 19.81 → 19.77 MiB |
| Three day-based Gantt charts | 21.23 ms | 21.69 ms | 10.58 → 10.73 MiB |
| Day-based Gantt, 128 tasks | 33.84 ms | 34.70 ms | 18.64 → 19.19 MiB |
| Four SGF boards | 10.61 ms | 11.45 ms | 7.89 → 8.22 MiB |
| Local SVG | 4.83 ms | 5.08 ms | 4.31 → 4.42 MiB |

| New time-based case | Native rendering | Peak RSS | Output |
| --- | ---: | ---: | ---: |
| Four-chart time fixture | 20.69 ms | 10.17 MiB | 65,240 bytes |
| 128 one-millisecond tasks at year 9999 | 33.76 ms | 18.89 MiB | 172,000 bytes / four bands |
| Long span, 600,001 working hours | 15.78 ms | 9.70 MiB | 8,134 bytes |
| 100 repeated charts | 40.88 ms | 9.33 MiB | 1,334,106 bytes |
| 40 unique charts | 62.69 ms | 13.97 MiB | 421,518 bytes |

The unique case still renders 32 charts and retains source for eight; repeated
charts reuse the cache. RSS uses `wait4`, not simultaneous CLI/worker totals.
There are no added dependencies, and the measured release binary changes from
17,095,488 to 17,095,776 bytes. The original performance baseline is unchanged.
Reproduce the time cases with `--renderer gloss/terminal-mermaid --corpus
'mermaid-gantt-time-*.md' --diagram-tiles mermaid-gantt-time-tasks-128.md=4`.
Generated inputs, PNG reviews and raw samples remain outside the repository.
