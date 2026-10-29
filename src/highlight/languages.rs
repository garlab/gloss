use tree_sitter::Query;

use super::CaptureKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Language {
    Bash,
    Go,
    JavaScript,
    Json,
    Python,
    Rust,
    Toml,
    TypeScript,
    Tsx,
    Yaml,
}

pub(super) struct Compiled {
    pub grammar: tree_sitter::Language,
    pub query: Query,
    pub kinds: Vec<CaptureKind>,
}

impl Language {
    pub fn find(label: &str) -> Option<Self> {
        const ALIASES: &[(&[&str], Language)] = &[
            (&["bash", "sh", "shell"], Language::Bash),
            (&["go", "golang"], Language::Go),
            (
                &["javascript", "js", "jsx", "mjs", "cjs"],
                Language::JavaScript,
            ),
            (&["json"], Language::Json),
            (&["python", "py"], Language::Python),
            (&["rust", "rs"], Language::Rust),
            (&["toml"], Language::Toml),
            (&["typescript", "ts", "mts", "cts"], Language::TypeScript),
            (&["tsx"], Language::Tsx),
            (&["yaml", "yml"], Language::Yaml),
        ];
        ALIASES.iter().find_map(|(aliases, language)| {
            aliases
                .iter()
                .any(|alias| label.eq_ignore_ascii_case(alias))
                .then_some(*language)
        })
    }

    pub fn compile(self) -> Option<Compiled> {
        let (grammar, sources): (tree_sitter::Language, &[&str]) = match self {
            Self::Bash => (
                tree_sitter_bash::LANGUAGE.into(),
                &[tree_sitter_bash::HIGHLIGHT_QUERY],
            ),
            Self::Go => (
                tree_sitter_go::LANGUAGE.into(),
                &[tree_sitter_go::HIGHLIGHTS_QUERY],
            ),
            Self::JavaScript => (
                tree_sitter_javascript::LANGUAGE.into(),
                &[
                    tree_sitter_javascript::HIGHLIGHT_QUERY,
                    tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
                ],
            ),
            Self::Json => (
                tree_sitter_json::LANGUAGE.into(),
                &[tree_sitter_json::HIGHLIGHTS_QUERY],
            ),
            Self::Python => (
                tree_sitter_python::LANGUAGE.into(),
                &[tree_sitter_python::HIGHLIGHTS_QUERY],
            ),
            Self::Rust => (
                tree_sitter_rust::LANGUAGE.into(),
                &[tree_sitter_rust::HIGHLIGHTS_QUERY],
            ),
            Self::Toml => (
                tree_sitter_toml_ng::LANGUAGE.into(),
                &[tree_sitter_toml_ng::HIGHLIGHTS_QUERY],
            ),
            Self::TypeScript => (
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                &[
                    tree_sitter_typescript::HIGHLIGHTS_QUERY,
                    tree_sitter_javascript::HIGHLIGHT_QUERY,
                ],
            ),
            Self::Tsx => (
                tree_sitter_typescript::LANGUAGE_TSX.into(),
                &[
                    tree_sitter_typescript::HIGHLIGHTS_QUERY,
                    tree_sitter_javascript::HIGHLIGHT_QUERY,
                    tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
                ],
            ),
            Self::Yaml => (
                tree_sitter_yaml::LANGUAGE.into(),
                &[tree_sitter_yaml::HIGHLIGHTS_QUERY],
            ),
        };
        // Queries are bundled, trusted data, compiled only when this language is
        // used. Documents cannot supply queries or cause runtime downloads.
        let query = Query::new(&grammar, &sources.join("\n")).ok()?;
        let kinds = query
            .capture_names()
            .iter()
            .map(|name| CaptureKind::from_name(name))
            .collect();
        Some(Compiled {
            grammar,
            query,
            kinds,
        })
    }
}
