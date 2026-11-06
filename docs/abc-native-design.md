# Native ABC music notation

Status: deferred design. ABC engraving is not implemented and does not block the
initial gloss release. An `abc` fence currently remains a code block. No music
backend or JavaScript runtime is selected by this document.

The intended feature is sheet music from fenced ABC notation, rendered entirely
by native code. The first step on resumption is a bounded evaluation of existing
Rust libraries, followed by a decision to extend one or build the missing layers.
A separate `abc-native` repository/crate is a working name for substantial shared
implementation, not a reserved package name or a reason to create a wrapper now.

## Direction and reuse candidates

The approach used by [katex-native](https://github.com/garlab/katex-native) is a
useful precedent: reuse parsing, expose native layout independently of the UI,
and validate against a pinned upstream corpus. There is an important difference:
KaTeX's intermediate output already carries substantial typesetting information.
An ABC parser alone does not solve music engraving. Note spacing, beams, slurs,
collisions, and system breaks still need an engine.

Source and manifest inspection produced this shortlist. These are candidates,
not independently verified compatibility or performance results; no native ABC
candidate has been benchmarked for gloss.

| Candidate | Reusable pieces | What needs evaluation |
| --- | --- | --- |
| [acorde](https://github.com/kent-tokyo/acorde/tree/a1c726b009fe33ee8e4bb585e8eae6fd25ddb750) | Separate Rust ABC I/O, score model, layout, and SVG crates; MIT OR Apache-2.0. | Evaluate the complete path first. Its [coverage matrix](https://github.com/kent-tokyo/acorde/blob/a1c726b009fe33ee8e4bb585e8eae6fd25ddb750/docs/notation-coverage.md) marks many ABC features partial. Review loss diagnostics, timing precision, source spans, cancellation, SVG compatibility, and glyph quality. |
| [croma-core](https://github.com/ro-ag/croma/tree/f1bd4588eee0af130b78d2e212277083558922fd/crates/croma-core) | ABC parser, diagnostics, source spans, rational score model, and MusicXML conversion; Apache-2.0. Its default core manifest has no runtime dependencies. | Strong parser/model candidate, with engraving still to supply. The inspected workspace declares Rust 1.96, above gloss's 1.90; investigate an earlier compatible release or a justified backport before adoption. Public model types also have a narrower stability promise than conversion functions. |
| [abc-parser 0.4.0](https://docs.rs/crate/abc-parser/0.4.0) | Focused PEG parser and ABC data types; MIT. | A smaller parser fork candidate. Upstream explicitly describes incomplete ABC 2.1 coverage. Establish semantic lowering, diagnostics/source mapping, exact-duration behavior, and bounded parsing requirements before choosing it. |
| [stave](https://github.com/manavbabel/stave/tree/c8750489e6a97a6fb311094b72c36d3ef22eb3f0) | Native ABC parser, rational-duration model, and notation tests; MIT OR Apache-2.0 for its own code. | Extraction candidate, not a terminal dependency. Its graphical path uses Verovio; reusing the parser does not supply a Rust engraver. Audit retained notation and source locations before extraction. |

Prefer a dependency plus upstream contributions when the necessary hooks exist.
Fork only for demonstrated gaps, with a pinned base, small explained patches,
license notices, and conformance tests. Keep application contracts independent of
the chosen parser's AST. A stable adapter can translate an existing score model;
there is no need to duplicate a capable model merely to rename its types.

For acorde, select individual crates and disable unrelated I/O defaults. Its SVG
renderer uses hand-authored vector glyphs; assess their appearance rather than
assuming SMuFL font fidelity. Inspect the actual dependency tree and initialization
behavior. For each candidate, test the supported Rust/platform matrix and malformed
input, and distinguish declared coverage from results reproduced locally.

[Verovio](https://github.com/rism-digital/verovio) is another genuinely native
option: a C++ engraving library with ABC import and SVG output. It merits a
comparison if extending Rust libraries proves impractical, but adds FFI/build,
distribution, and LGPL obligations to evaluate. It is not a JavaScript requirement.
No C++ backend is selected here either.

## Scope and correctness contract

Use [ABC 2.1](https://abcnotation.com/wiki/abc:standard:v2.1) as the initial
semantic reference. Track common implementation extensions separately. Publish a
feature matrix with distinct parse, semantic, and engraving coverage: accepting
input is not sufficient evidence that the score is correct.

Develop in these increments:

1. A single tune, voice, and staff: common headers (`X`, `T`, `M`, `L`, `K`),
   treble/bass clefs, key signatures, notes/rests, accidentals, octaves, ordinary
   durations, barlines, stems, flags, beams, and readable system breaks.
2. Useful melody notation: chords, broken rhythm, tuplets, ties, slurs, grace
   notes, repeat signs and endings, common decorations, chord labels, lyrics,
   inline field changes, and accented titles/text. Preserve written repeat
   structure; engraving must not expand playback repeats.
3. Multiple tunes, voices and staves: synchronized rhythmic columns, shared-staff
   collisions, staff groups, cross-system spans, and additional lyric verses.

Treat these as milestones, not promises of support for every spelling in a
category. Define the accepted forms and rejection tests before enabling each one.
Initially fall back for the entire fence if any tune has unsupported notation
that would change or omit the written music. Later partial rendering would need
an explicit mixed-output design with source ranges for every omitted region.

Playback, MIDI synthesis, score editing, tablature, percussion, microtonal
engraving, and arbitrary renderer-specific formatting are outside the initial
scope. Document-supplied code, embedded SVG/PostScript, remote resources, file
includes, and font discovery are not part of the runtime contract. Implement only
explicitly supported inert directives; unsupported directives retain source.

## Native pipeline

```text
ABC source
  -> parser + source spans
  -> semantic score + diagnostics
  -> engraving + system layout
  -> display list / SVG per system
  -> bounded rasterization and application placement
```

The semantic score must retain spelling and written structure, not only MIDI
pitches and elapsed time. It needs:

- Checked rational onset/duration values, with written duration separate from
  tuplet or broken-rhythm scaling. Reject arithmetic overflow and bound numeric
  input; floating-point coordinates are appropriate only after semantic timing.
- Header inheritance, default note-length rules, per-voice key/clef/meter state,
  accidental scope/reset, and spelled pitches. Preserve beaming whitespace and
  explicit line-break intent where they affect notation.
- Chords and rests, voice clocks, bar boundaries, pickup measures, repeat/ending
  structure, and distinct tie/slur relationships. Keep lyrics and decorations
  attached to their intended events.
- UTF-8 byte spans into the original fence, stable event identities, and
  diagnostics that distinguish malformed input, unsupported features, and limits.
  Preserve Unicode labels without byte/character index confusion.

If a suitable layout engine cannot be adapted, native engraving requires these
stages:

1. Measure glyphs and text in staff-space units; determine staff positions,
   noteheads, accidentals, ledger lines, and attachment anchors.
2. Build time-aligned columns with minimum collision spacing plus rhythmic
   spacing. Account for chords, accidentals, and text before choosing line breaks.
3. Break at suitable bar boundaries within the available width, honoring supported
   explicit breaks. Use bounded lookahead/work rather than an unrestricted global
   optimization. A system is one horizontal row of one or more staves.
4. Resolve stems/beams, ties/slurs, tuplets, lyrics and vertical staff spacing;
   include their extents in the returned bounds. Long spans need continuation
   geometry across system breaks.
5. Produce deterministic drawing primitives and source-linked bounds. Keep logical
   dimensions independent of display scale and raster pixel limits.

For new engraving work, evaluate a pinned
[SMuFL](https://w3c-cg.github.io/smufl/latest/) font such as
[Bravura](https://github.com/steinbergmedia/bravura), including its metrics and
anchors, before drawing music symbols by hand. Bravura is OFL-1.1; retain its
notices and check reserved-name requirements before modifying/subsetting it.
Music glyph mapping does not solve engraving, nor does it provide title/lyric
text shaping. Choose deterministic text fonts separately and measure their cost.

Expose owned glyph outlines or explicit font resources; do not rely on fonts
installed on the user's machine. SVG must stay within document-graphics' accepted
subset. In particular, do not assume external fonts, scripts, `<use>` references,
or arbitrary definitions can pass its validation. Emitting resolved paths is an
option, but repeated path data must count against geometry/output budgets.

## Library and application boundaries

If substantial new code is needed, keep it in an independent repository with one
library crate initially:

```text
abc-native/
  Cargo.toml
  src/
    lib.rs              # small public API, diagnostics and limits
    parse/              # adapter or maintained parser implementation
    model/              # notation semantics and source mappings
    engrave/            # spacing, systems, beams, spans and collisions
    fonts/              # metrics and font access
    render/             # display list and SVG; optional raster support
  fonts/                # pinned assets, provenance and licenses
  tests/
    conformance/        # small licensed ABC cases and semantic expectations
    layout/             # structural invariants and reviewed snapshots
    visual/             # representative native render regressions
  tools/reference/      # optional reference extraction/comparison tools
  benches/              # generators; large corpora and outputs stay external
  docs/                 # supported subset, architecture, limits, measurements
  LICENSE
  THIRD_PARTY_NOTICES
```

This is a proposed layout, not a new checkout to create while the feature is
deferred. Reuse an existing library directly if it meets the contract; extract
additional crates only when independent consumers need them.

The conceptual API separates `parse(source, limits, cancellation)`,
`layout(score, width, fonts, limits, cancellation)`, and rendering a selected
system. Exact signatures remain open. Return diagnostics, ordered systems,
logical bounds, and source mappings. Bound the retained score and layout even
when rasterization is lazy. Reflow should reuse semantic parsing, while zoom-only
changes should reuse logical layout when its width is unchanged.

- **Music library:** syntax, semantics, engraving, glyph resources, display list,
  and SVG. No Markdown, terminal transport, GUI framework, or JavaScript runtime.
- **document-graphics:** optional music adapter, shared raster representation and
  SVG validation, raster limits, and worker/isolation integration where needed.
  Keep engine work behind its existing bounded request model until cancellation
  and allocation behavior justify an in-process path. Extend the protocol for
  system-level output without permitting an unbounded batch response.
- **gloss and other consumers:** fence recognition, capability decisions,
  reading width, theme, source fallback, queues, cache budgets, scrolling,
  copying/export, and watch/resize invalidation. Add the adapter to shared graphics
  so terminal and GUI consumers can reuse it independently.

Music parsing and engraving do not belong in katex-native. General graphics
limits or drawing helpers are candidates for sharing only when both consumers
need the same contract; the math display list should not become a music model.

For gloss, recognize `abc` before generic fence rendering only once the feature
is available. Unsupported terminals, disabled graphics, errors, or exhausted
budgets retain the complete fence. Plain/source exports continue to carry ABC
text and never initialize the engine. Raster notes are not selectable text;
selectable title/caption text and later source-copy actions can help, but source
export remains the reliable copying path, including pipes to clipboard tools.

## Reference suites and conformance

Reference-driven development is recommended. Reuse the method from katex-native:
pin a corpus, preserve provenance, record deterministic native layout snapshots,
and review visual comparisons. Do not require identical pixels from different
engravers: valid spacing, fonts, and line breaks can differ.

[ABCJS's test suite](https://github.com/paulrosen/abcjs/tree/2e399a65fdf12a0dfaf80a0837cc0f182c67fb37/tests)
is a concrete starting point. Its parse cases include note/source positions,
voices, and ties/slurs; visual cases exercise spacing, collisions, decorations,
wrapping, and multiple staves. The repository is
[MIT-licensed](https://github.com/paulrosen/abcjs/blob/2e399a65fdf12a0dfaf80a0837cc0f182c67fb37/LICENSE.md).
Extract selected ABC strings and assertions into portable fixtures rather than
copying the browser test harness or adopting its object model as the public API.

Use [abc2svg](https://chiselapp.com/user/moinejf/repository/abc2svg/doc/trunk/README.md)
and native [abcm2ps](https://github.com/lewdlime/abcm2ps) or Verovio as secondary
references when engines disagree. abcm2ps describes itself as end-of-life, so it
is a useful historical comparison, not an assumed maintained backend.
Compare ABC semantics against the specification first; record deliberate
compatibility extensions and reference bugs instead of silently copying them.

JavaScript reference tools, if used, belong exclusively in optional developer
fixture-generation jobs. Normal builds, `cargo test`, and installed applications
must work without Node, a browser, or a JS engine. Generate reference artifacts
externally and commit only small reviewed fixtures/snapshots with provenance.
Native reference tools can provide additional comparisons without JavaScript.

Each imported case needs its original URL/path, pinned revision, license/notice,
feature tags, and expected outcome. Record renderer version, flags, font hashes,
viewport, and artifact hashes when regenerating references. Check fixture and
music rights separately from the implementation's code license; prefer short
synthetic examples. Do not copy copyleft implementation/test code into a
permissive crate without resolving its licensing. Running a separate reference
tool on project-owned inputs avoids needing to import its implementation.

Build distinct test layers:

| Layer | What it should establish |
| --- | --- |
| Parsing and rejection | Accepted grammar, source spans, diagnostics, and complete rejection/fallback for unsupported music. |
| Semantic fixtures | Spelled pitches, exact durations/onsets, voice/bar structure, accidental state, lyrics, and tie/slur targets. Normalize reference representations into this small schema. |
| Layout invariants | Aligned simultaneous notes, valid beam/stem connections, non-overlapping required bounds, finite geometry, and no lost/duplicated events across systems. |
| Native snapshots | Stable rounded display-list geometry and pinned-font raster goldens, updated through explicit review. |
| Reference comparisons | Side-by-side scores and toleranced image differences for representative cases; geometry disagreements are investigated, not automatically treated as musical errors. |
| Adversarial/property tests | No panic or runaway allocation/work on malformed input, huge numbers, dense chords, many voices, long lyrics, nested spans, or tiny widths; bounded cancellation. |

Use safe metamorphic changes such as changing a title without changing musical
events. Do not assume arbitrary whitespace is neutral: ABC whitespace can change
beaming. Keep intentional unsupported cases in the corpus so expanding the parser
cannot accidentally produce incomplete but successful images.

## Performance and resource gates

Set measured numeric budgets before enabling the backend; this design does not
claim an ABC latency or memory baseline. Reuse the
[feature resource requirements](feature-design.md#resource-and-performance-requirements)
and compare unchanged paths with both the last accepted build and the fixed
[performance baseline](performance-baseline.md).

- Bound source bytes/lines, tokens, tunes, voices, notes/chord size, lyric bytes,
  span depth, numeric magnitudes, diagnostics, display-list items, SVG geometry,
  system count, and raster pixels. Validate before large allocations and check
  cancellation/work budgets inside parsing, collision, and line-breaking loops.
- Treat total score/layout/cache memory separately from per-system raster memory.
  A small output image does not bound intermediate allocations. Worker deadlines
  and memory enforcement remain necessary if dependencies cannot be interrupted.
- Render systems individually; never rasterize an entire long tune book into one
  tall image. Bound aggregate output and outstanding work. Lazy rasterization
  does not imply parsing/layout can process unlimited input.
- Load fonts/backend state only on the first render request. Keep them absent
  from non-ABC and plain/source output paths. Account for font bytes, dependency
  growth, and retained worker memory as well as CPU time.
- Cache by source, engine/version, notation options, logical width, glyph/font
  identity, theme and pixel scale as appropriate to each layer. Byte-account
  caches and discard obsolete queued work on watch edits or resize.

Benchmark feature absent, disabled, enabled, and rejected cases. Include a short
melody, repeated fences/cache hits, lyrics, dense notation, multiple voices, long
scores, narrow-width reflow, and adversarial inputs. Measure cold startup, parse,
layout, first-system/first-output latency, rasterization, resize/cache reuse,
cancellation latency, peak process-tree RSS, binary/font size, and encoded output.
Keep large generated corpora and local results outside the repository. Include
real-terminal responsiveness and terminal-process memory separately from renderer
measurements.

On unchanged paths, investigate repeatable time/first-output increases above the
larger of 10% or 5 ms, or peak RSS above the larger of 10% or 8 MiB, as specified
in the feature design. These are review triggers, not per-feature allowances.
New music rendering costs need their own measured acceptance limits.

## Resuming implementation

1. Define a small representative corpus and capability matrix, including required
   rejections. Audit fixture/font licensing and pin reference versions.
2. Evaluate acorde's complete native path and the parser-only candidates on that
   corpus. Record correctness gaps, source mapping, cancellation hooks, Rust
   compatibility, dependency/binary cost, and timing/RSS. Decide dependency,
   upstream extension, or maintained fork from this evidence.
3. If new engraving is necessary, establish the standalone library, semantic
   fixtures and one-staff layout before widening notation coverage. Add bounds
   and cancellation alongside each algorithm, not after graphical success.
4. Add optional document-graphics integration and validate worker cleanup,
   system output, fonts/SVG/raster limits, and shared-library regressions.
5. Integrate gloss fences, source fallbacks, pager/watch/reflow and export tests;
   repeat resource and terminal checks before documenting ABC as supported.

The release can proceed while this work remains deferred. No runtime dependency,
package publication, or additional repository is needed for the design alone.
