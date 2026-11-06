use std::{
    error::Error,
    io::{self, IsTerminal, Write},
    path::PathBuf,
    process::ExitCode,
};

use clap::{CommandFactory, Parser, ValueEnum};
use mdviewer::{
    export,
    input::{Format, Input},
    render,
    terminal::Terminal,
};

#[derive(Parser)]
#[command(
    version,
    about = "A terminal Markdown viewer with native inline and display equations"
)]
struct Args {
    /// Markdown, code, Mermaid, SGF or image file; omit or use '-' for text stdin.
    file: Option<PathBuf>,
    /// Auto detects named-file extensions; stdin defaults to Markdown.
    #[arg(long, value_enum, default_value_t = Format::Auto)]
    input_format: Format,
    /// Treat input as code in this language; overrides extension detection.
    #[arg(long)]
    language: Option<String>,
    /// Open an interactive pager; ignored for pipes and text/source exports.
    #[arg(short = 'p', long)]
    pager: bool,
    /// Reload a named file when it changes; implies --pager and requires a terminal.
    #[arg(long)]
    watch: bool,
    /// Auto styles terminal text; pipes receive clean text. Source preserves original text.
    #[arg(long, value_enum, default_value_t = Output::Auto)]
    output: Output,
    /// Auto enables graphics for known Ghostty/Kitty sessions. Pipes always use TeX.
    #[arg(long, value_enum, default_value_t = Math::Auto)]
    math: Math,
    /// Local PNG/JPEG/SVG images: auto detects Kitty graphics; off retains text.
    #[arg(long, value_enum, default_value_t = Images::Auto)]
    images: Images,
    /// Mermaid diagrams: auto uses known Kitty graphics sessions; off keeps code.
    #[arg(long, value_enum, default_value_t = Images::Auto)]
    mermaid: Images,
    /// Print selectable Mermaid source beneath rendered diagrams.
    #[arg(long)]
    show_mermaid_source: bool,
    /// SGF Go diagrams: auto uses known Kitty graphics sessions; off keeps code.
    #[arg(long, value_enum, default_value_t = Images::Auto)]
    sgf: Images,
    /// One-based game to preview in an SGF collection; fences can override it.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    sgf_game: u32,
    /// Main-line move to preview (passes count); 0 selects the root, default is last.
    #[arg(long)]
    sgf_move: Option<u32>,
    /// Print selectable SGF source below rendered boards.
    #[arg(long)]
    show_sgf_source: bool,
    #[arg(long, hide = true)]
    mermaid_worker: bool,
    /// Base directory for image paths; defaults to the document directory or cwd for stdin.
    #[arg(long)]
    base_dir: Option<PathBuf>,
    /// Print selectable TeX beneath lines with rendered equations.
    #[arg(long)]
    show_math_source: bool,
    /// Terminal hyperlinks: auto enables known terminals; off keeps visible targets.
    #[arg(long, value_enum, default_value_t = Links::Auto)]
    links: Links,
    /// Theme: auto uses COLORFGBG when available and otherwise defaults to dark.
    #[arg(long, value_enum, default_value_t = Theme::Auto)]
    theme: Theme,
    /// Terminal cell dimensions in pixels, e.g. 10x20; overrides reported geometry.
    #[arg(long, value_parser = mdviewer::terminal::cell_size)]
    cell_size: Option<(u16, u16)>,
    /// Use a light theme for text and equations (alias for --theme light).
    #[arg(long, conflicts_with = "theme")]
    light: bool,
    /// Keep code uncolored while retaining the rest of the theme.
    #[arg(long)]
    no_highlight: bool,
    /// Code line numbers: auto enables them for standalone code; on includes Markdown blocks.
    #[arg(long, value_enum, default_value_t = LineNumbers::Auto)]
    line_numbers: LineNumbers,
    /// Maximum content width, excluding margins; 0 uses the terminal width.
    #[arg(short = 'w', long, default_value_t = 80)]
    width: u16,
    /// Horizontal margin in columns; reduced automatically in narrow terminals.
    #[arg(long, default_value_t = 2)]
    margin: u16,
    /// Equation font size in pixels; defaults to 90% of the terminal cell height.
    #[arg(long, value_parser = clap::value_parser!(u16).range(4..=256))]
    font_size: Option<u16>,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Output {
    Auto,
    Text,
    Source,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Math {
    Auto,
    Tex,
    Kitty,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Images {
    Auto,
    Off,
    Kitty,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Links {
    Auto,
    Off,
    Always,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Theme {
    Auto,
    Dark,
    Light,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum LineNumbers {
    Auto,
    On,
    Off,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error)
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|err| err.kind() == io::ErrorKind::BrokenPipe) =>
        {
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("mdviewer: {}", render::text::sanitize(&error.to_string()));
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    if args.mermaid_worker {
        mdviewer::mermaid::worker_main()?;
        return Ok(());
    }
    let input = Input::new(args.file.clone(), args.input_format, args.language.clone())?;
    let input_format = input.format;
    let named_file = input.path.as_deref();
    if input.format == Format::Image && args.output == Output::Source {
        return Err(
            "--output source is for text inputs; use the original image file for binary copying"
                .into(),
        );
    }
    let terminal = Terminal::detect();
    let interactive = terminal.interactive
        && args.output == Output::Auto
        && std::env::var("TERM").as_deref() != Ok("dumb");
    if args.watch && (named_file.is_none() || !interactive) {
        return Err(
            "--watch requires a named file, --output auto, and an interactive terminal".into(),
        );
    }
    if args.file.is_none() && io::stdin().is_terminal() {
        Args::command().print_help()?;
        println!();
        return Ok(());
    }
    let paging = (args.pager || args.watch) && interactive;
    let source = input.read(paging)?;
    if paging {
        #[cfg(unix)]
        return mdviewer::pager::run(source, input, args.watch, || {
            args.render_options(Terminal::detect(), input_format)
        })
        .map_err(Into::into);
        #[cfg(not(unix))]
        return Err("the interactive pager currently requires a Unix terminal".into());
    }
    let mut out = io::BufWriter::new(io::stdout().lock());
    if args.output == Output::Source {
        // Bypass parsing altogether: preserve CRLF, spacing, and final-newline state.
        out.write_all(source.as_bytes())?;
    } else {
        let document = input.document(source);
        if args.output == Output::Text || !terminal.interactive {
            export::write(&mut out, &document)?;
        } else {
            if args.math == Math::Kitty && terminal.multiplexer {
                eprintln!(
                    "mdviewer: multiplexer graphics passthrough is not implemented; showing TeX"
                );
            }
            let options = args.render_options(terminal, input_format);
            let warnings = render::write_document(&mut out, &document, &options)?;
            out.flush()?;
            for warning in warnings {
                eprintln!("mdviewer: {}", render::text::sanitize(&warning));
            }
        }
    }
    out.flush()?;
    Ok(())
}

impl Args {
    fn render_options(&self, mut terminal: Terminal, input_format: Format) -> render::Options {
        if let Some((width, height)) = self.cell_size {
            terminal.geometry.cell_width = width;
            terminal.geometry.cell_height = height;
        }
        let kitty = terminal.graphics_allowed
            && match self.math {
                Math::Auto => terminal.kitty_hint,
                Math::Kitty => true,
                Math::Tex => false,
            };
        render::Options {
            styled: terminal.styled,
            links: terminal.links_allowed
                && match self.links {
                    Links::Auto => terminal.kitty_hint,
                    Links::Off => false,
                    Links::Always => true,
                },
            highlight: !self.no_highlight,
            line_numbers: match self.line_numbers {
                LineNumbers::Auto => input_format == Format::Code,
                LineNumbers::On => true,
                LineNumbers::Off => false,
            },
            kitty: kitty.then_some(terminal.geometry),
            terminal: Some(terminal.geometry),
            width: self.width,
            margin: self.margin,
            font_size: self.font_size.map_or_else(
                || (f64::from(terminal.geometry.cell_height) * 0.9).clamp(4., 256.),
                f64::from,
            ),
            light_background: self.light
                || match self.theme {
                    Theme::Auto => terminal.light_background,
                    Theme::Dark => false,
                    Theme::Light => true,
                },
            show_math_source: self.show_math_source,
            mermaid: terminal.graphics_allowed
                && match self.mermaid {
                    Images::Auto => terminal.kitty_hint,
                    Images::Off => false,
                    Images::Kitty => true,
                },
            show_mermaid_source: self.show_mermaid_source,
            sgf: terminal.graphics_allowed
                && match self.sgf {
                    Images::Auto => terminal.kitty_hint,
                    Images::Off => false,
                    Images::Kitty => true,
                },
            sgf_selection: mdviewer::sgf::Selection {
                game: self.sgf_game as usize,
                at_move: self.sgf_move.map(|n| n as usize),
            },
            show_sgf_source: self.show_sgf_source,
            image_base: (terminal.graphics_allowed
                && match self.images {
                    Images::Auto => terminal.kitty_hint,
                    Images::Off => false,
                    Images::Kitty => true,
                })
            .then(|| {
                self.base_dir.clone().unwrap_or_else(|| {
                    self.file
                        .as_deref()
                        .filter(|path| *path != std::path::Path::new("-"))
                        .and_then(std::path::Path::parent)
                        .unwrap_or(std::path::Path::new("."))
                        .to_path_buf()
                })
            }),
        }
    }
}
