//! Input selection and bounded reloads. Standalone inputs reuse document events.
use crate::{document::Document, highlight::Highlighter};
use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Format {
    #[default]
    Auto,
    Markdown,
    Latex,
    Code,
    Mermaid,
    Sgf,
    Image,
}

#[derive(Clone, Debug)]
pub struct Input {
    pub format: Format,
    pub path: Option<PathBuf>,
    language: Option<String>,
}

impl Input {
    pub fn new(
        path: Option<PathBuf>,
        format: Format,
        language: Option<String>,
    ) -> io::Result<Self> {
        if language.is_some() && !matches!(format, Format::Auto | Format::Code) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--language requires --input-format auto or code",
            ));
        }
        let path = path.filter(|p| p != Path::new("-"));
        let extension = path
            .as_deref()
            .and_then(Path::extension)
            .and_then(|s| s.to_str())
            .map(str::to_ascii_lowercase);
        let detected_language = match extension.as_deref() {
            Some("yaml" | "yml") => Some("yaml"),
            Some("toml") => Some("toml"),
            Some("json") => Some("json"),
            Some("rs") => Some("rust"),
            Some("py" | "pyi" | "pyw") => Some("python"),
            Some("js" | "jsx" | "mjs" | "cjs") => Some("javascript"),
            Some("ts" | "mts" | "cts") => Some("typescript"),
            Some("tsx") => Some("tsx"),
            Some("go") => Some("go"),
            Some("sh" | "bash") => Some("bash"),
            Some("dockerfile" | "containerfile") => Some("dockerfile"),
            _ => None,
        }
        .or_else(|| path.as_deref().and_then(filename_language));
        let format = if format == Format::Auto {
            match extension.as_deref() {
                _ if language.is_some() => Format::Code,
                Some("mmd" | "mermaid") => Format::Mermaid,
                Some("sgf") => Format::Sgf,
                Some("png" | "jpg" | "jpeg" | "svg") => Format::Image,
                Some("md" | "markdown" | "mdown" | "mkd" | "mkdn" | "mdwn") => Format::Markdown,
                Some("tex" | "latex" | "ltx") => Format::Latex,
                _ if detected_language.is_some() => Format::Code,
                _ => Format::Markdown,
            }
        } else {
            format
        };
        if format == Format::Image && path.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "image input requires a named file; binary stdin is not supported",
            ));
        }
        let language = language.or_else(|| detected_language.map(str::to_owned));
        Ok(Self {
            format,
            path,
            language,
        })
    }

    pub fn read(&self, paging: bool) -> io::Result<String> {
        if self.format == Format::Image {
            let path = self.path.as_ref().unwrap();
            if !std::fs::metadata(path)?.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "image input requires a regular file",
                ));
            }
            // Images stay on disk until graphics are enabled. Plain output needs
            // only a reference; watch reloads must not compare this label as data.
            return Ok(path.to_string_lossy().into_owned());
        }
        let bounded = paging || matches!(self.format, Format::Code | Format::Mermaid | Format::Sgf);
        match self.path.as_deref() {
            Some(path) if bounded => read_file(path),
            Some(path) => std::fs::read_to_string(path),
            None if bounded => read(io::stdin().lock()),
            None => {
                let mut source = String::new();
                io::stdin().read_to_string(&mut source)?;
                Ok(source)
            }
        }
    }

    pub fn document(&self, source: String) -> Document {
        match self.format {
            Format::Code => {
                // Forward only syntax labels: code mode must never turn Mermaid
                // or SGF source into graphics, even with an explicit language.
                let language = self
                    .language
                    .as_deref()
                    .filter(|label| Highlighter::supports(label))
                    .unwrap_or_default();
                Document::code(source, language)
            }
            Format::Mermaid => Document::code(source, "mermaid"),
            Format::Sgf => Document::code(source, "sgf"),
            Format::Image => Document::image(self.path.as_ref().unwrap().clone()),
            Format::Latex => Document::latex(source),
            Format::Auto | Format::Markdown => Document::parse(source),
        }
    }
}

fn filename_language(path: &Path) -> Option<&'static str> {
    let name = path.file_name()?.to_str()?;
    if name.eq_ignore_ascii_case("Cargo.lock") {
        return Some("toml");
    }
    let base = name.split('.').next()?;
    if base.eq_ignore_ascii_case("Dockerfile") || base.eq_ignore_ascii_case("Containerfile") {
        return Some("dockerfile");
    }
    None
}

pub const MAX_SOURCE: u64 = 64 * 1024 * 1024;
pub fn read(mut input: impl Read) -> io::Result<String> {
    let mut source = String::new();
    (&mut input)
        .take(MAX_SOURCE + 1)
        .read_to_string(&mut source)?;
    if source.len() as u64 > MAX_SOURCE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pager or standalone text input exceeds 64 MiB; use a smaller document",
        ));
    }
    Ok(source)
}

pub fn read_file(path: &Path) -> io::Result<String> {
    let mut options = File::options();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pager or standalone text input requires a regular file",
        ));
    }
    if metadata.len() > MAX_SOURCE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pager or standalone text input exceeds 64 MiB",
        ));
    }
    read(file)
}
