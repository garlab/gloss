//! Input selection and bounded reloads. Standalone graphics reuse document events.
use crate::document::Document;
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
    Mermaid,
    Sgf,
    Image,
}

#[derive(Clone, Debug)]
pub struct Input {
    pub format: Format,
    pub path: Option<PathBuf>,
}

impl Input {
    pub fn new(path: Option<PathBuf>, format: Format) -> io::Result<Self> {
        let path = path.filter(|p| p != Path::new("-"));
        let format = if format == Format::Auto {
            match path
                .as_deref()
                .and_then(Path::extension)
                .and_then(|s| s.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref()
            {
                Some("mmd" | "mermaid") => Format::Mermaid,
                Some("sgf") => Format::Sgf,
                Some("png" | "jpg" | "jpeg" | "svg") => Format::Image,
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
        Ok(Self { format, path })
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
        let bounded = paging || matches!(self.format, Format::Mermaid | Format::Sgf);
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
            Format::Mermaid => Document::code(source, "mermaid"),
            Format::Sgf => Document::code(source, "sgf"),
            Format::Image => Document::image(self.path.as_ref().unwrap().clone()),
            Format::Auto | Format::Markdown => Document::parse(source),
        }
    }
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
            "pager or diagram input exceeds 64 MiB; use direct Markdown output or a smaller document",
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
            "pager or diagram input requires a regular file",
        ));
    }
    if metadata.len() > MAX_SOURCE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pager or diagram input exceeds 64 MiB",
        ));
    }
    read(file)
}
