use std::{
    error::Error,
    io::{self, IsTerminal, Read, Write},
    path::PathBuf,
    process::ExitCode,
};

use clap::{CommandFactory, Parser, ValueEnum};
use mdviewer::{document::Document, export, render, terminal::Terminal};

#[derive(Parser)]
#[command(
    version,
    about = "A terminal Markdown viewer with native inline and display equations"
)]
struct Args {
    /// Markdown file; omit or use '-' to read stdin.
    file: Option<PathBuf>,
    /// Auto styles terminal text; pipes always receive clean text. Source is exact Markdown.
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
    /// Keep code blocks uncolored while retaining the rest of the theme.
    #[arg(long)]
    no_highlight: bool,
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
        mdviewer::mermaid::serve(&mut io::stdin().lock(), &mut io::stdout().lock())?;
        return Ok(());
    }
    if args.file.is_none() && io::stdin().is_terminal() {
        Args::command().print_help()?;
        println!();
        return Ok(());
    }
    let mut source = String::new();
    match args.file.as_deref() {
        Some(path) if path != std::path::Path::new("-") => source = std::fs::read_to_string(path)?,
        _ => {
            io::stdin().read_to_string(&mut source)?;
        }
    }
    let mut terminal = Terminal::detect();
    if let Some((width, height)) = args.cell_size {
        terminal.geometry.cell_width = width;
        terminal.geometry.cell_height = height;
    }
    let mut out = io::BufWriter::new(io::stdout().lock());
    if args.output == Output::Source {
        // Bypass parsing altogether: preserve CRLF, spacing, and final-newline state.
        out.write_all(source.as_bytes())?;
    } else {
        let document = Document::parse(source);
        if args.output == Output::Text || !terminal.interactive {
            export::write(&mut out, &document)?;
        } else {
            let kitty = terminal.graphics_allowed
                && match args.math {
                    Math::Auto => terminal.kitty_hint,
                    Math::Kitty => true,
                    Math::Tex => false,
                };
            if args.math == Math::Kitty && terminal.multiplexer {
                eprintln!(
                    "mdviewer: multiplexer graphics passthrough is not implemented; showing TeX"
                );
            }
            let options = render::Options {
                styled: terminal.styled,
                links: terminal.links_allowed
                    && match args.links {
                        Links::Auto => terminal.kitty_hint,
                        Links::Off => false,
                        Links::Always => true,
                    },
                highlight: !args.no_highlight,
                kitty: kitty.then_some(terminal.geometry),
                terminal: Some(terminal.geometry),
                width: args.width,
                margin: args.margin,
                font_size: args.font_size.map_or_else(
                    || (f64::from(terminal.geometry.cell_height) * 0.9).clamp(4., 256.),
                    f64::from,
                ),
                light_background: args.light
                    || match args.theme {
                        Theme::Auto => terminal.light_background,
                        Theme::Dark => false,
                        Theme::Light => true,
                    },
                show_math_source: args.show_math_source,
                mermaid: terminal.graphics_allowed
                    && match args.mermaid {
                        Images::Auto => terminal.kitty_hint,
                        Images::Off => false,
                        Images::Kitty => true,
                    },
                show_mermaid_source: args.show_mermaid_source,
                image_base: (terminal.graphics_allowed
                    && match args.images {
                        Images::Auto => terminal.kitty_hint,
                        Images::Off => false,
                        Images::Kitty => true,
                    })
                .then(|| {
                    args.base_dir.clone().unwrap_or_else(|| {
                        args.file
                            .as_deref()
                            .filter(|path| *path != std::path::Path::new("-"))
                            .and_then(std::path::Path::parent)
                            .unwrap_or(std::path::Path::new("."))
                            .to_path_buf()
                    })
                }),
            };
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
