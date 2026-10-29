use std::{
    error::Error,
    io::{self, IsTerminal, Read, Write},
    path::PathBuf,
    process::ExitCode,
};

use clap::{CommandFactory, Parser, ValueEnum};
use mdviewer::{document::Document, export, layout, render, terminal::Terminal};

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
    /// Print selectable TeX beneath lines with rendered equations.
    #[arg(long)]
    show_math_source: bool,
    /// Use dark equation ink on a light terminal background.
    #[arg(long)]
    light: bool,
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
    let terminal = Terminal::detect();
    let mut out = io::BufWriter::new(io::stdout().lock());
    if args.output == Output::Source {
        // Bypass parsing altogether: preserve CRLF, spacing, and final-newline state.
        out.write_all(source.as_bytes())?;
    } else {
        let document = Document::parse(source);
        if args.output == Output::Text || !terminal.interactive {
            out.write_all(export::text(&document).as_bytes())?;
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
                kitty: kitty.then_some(terminal.geometry),
                font_size: args.font_size.map_or_else(
                    || (f64::from(terminal.geometry.cell_height) * 0.9).clamp(4., 256.),
                    f64::from,
                ),
                light_background: args.light,
                show_math_source: args.show_math_source,
            };
            let warnings =
                render::write(&mut out, &document, &layout::layout(&document), &options)?;
            out.flush()?;
            for warning in warnings {
                eprintln!("mdviewer: {}", render::text::sanitize(&warning));
            }
        }
    }
    out.flush()?;
    Ok(())
}
