# Terminal discovery and overrides

Discovery does not read terminal replies, change input modes, or consume keyboard
input. A file invocation leaves stdin untouched. A stdin invocation reads the
Markdown stream normally. Pipes and explicit text/source exports bypass graphics
and hyperlinks regardless of the requested theme or geometry.

## Geometry

On Unix, `TIOCGWINSZ` supplies rows, columns, and available pixel dimensions.
Reported rows and columns take precedence over the `COLUMNS` and `LINES`
environment hints; valid positive hints supply missing values. The final defaults
are 80 columns and 24 rows. Missing pixel dimensions use 10 by 20 pixel cells.

`--cell-size WIDTHxHEIGHT` overrides cell dimensions. Each dimension must be from
1 through 512 pixels, for example `--cell-size 12x24`. This affects equation size,
baselines, image fitting, and cell padding. It does not change the actual window
size or terminal font. `--font-size` independently overrides equation font size.
A font baseline is still estimated at 78% of the cell height.

Direct output captures geometry once and does not redraw existing scrollback.
The [pager](pager.md) refreshes geometry before redraws and during idle polling,
and reflows the document when the available size changes.

## Theme

`--theme auto` uses the background palette index in a valid `COLORFGBG` hint,
when present, to select the light or dark palette. This is an estimate based on
conventional palette colors, not a query of the terminal's actual background.
Missing or invalid hints select dark. Custom palettes and terminal configurations
that omit the hint may need an explicit choice.

`--theme dark` and `--theme light` override the hint. `--light` remains an alias
for the light choice. `NO_COLOR` suppresses text colors while leaving graphics
and explicitly selected geometry intact.

## Capabilities and queries

Automatic graphics selection still uses Ghostty/Kitty environment hints.
Explicit graphics and link overrides remain available. Multiplexers, dumb
terminals, and very small windows retain their existing graphics restrictions.
No extra subprocess, startup timeout, or terminal-query traffic is introduced.

Active capability, cell-size, and OSC color queries remain future work. A
short-lived scrollback writer cannot portably restore unrelated keypresses to
the shell after reading a terminal reply. The pager owns terminal input and is
the appropriate place to add reply handling, but does not query capabilities yet.
It would need to separate bounded replies from navigation input.
Queries must preserve document stdin, have strict byte/time limits, restore
terminal state, and fall back when a terminal does not answer.

## Verification boundary

The PTY suite checks theme precedence, cell overrides at multiple sizes, narrow
windows, compressed graphics, link scoping, and fallback policies. Payload tests
inflate transmitted images and validate their dimensions and pixels. These
checks do not measure emulator rendering, GPU work, terminal memory, native mouse
selection, or font-specific visual alignment. Live Ghostty/Kitty checks across
fonts, zoom, scrolling, and selection remain on the roadmap.

Protocol references: [Kitty graphics](https://sw.kovidgoyal.net/kitty/graphics-protocol/)
and [xterm control sequences](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html).
