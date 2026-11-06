use std::{
    io::Write,
    process::{Command, Stdio},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use gloss_cli::{document::Document, export, layout, math::MathEngine, render, terminal::Geometry};
use katex_native::Color;
use pulldown_cmark::Event;

const FIXTURE: &str = include_str!("fixtures/math.md");

#[test]
fn standalone_inputs_detect_extensions_and_keep_literal_source() {
    use gloss_cli::input::{Format, Input};
    for (name, expected) in [
        ("chart.MMD", Format::Mermaid),
        ("chart.mermaid", Format::Mermaid),
        ("game.sgf", Format::Sgf),
        ("a.PNG", Format::Image),
        ("a.jpeg", Format::Image),
        ("a.svg", Format::Image),
        ("config.YaMl", Format::Code),
        ("config.yml", Format::Code),
        ("Cargo.toml", Format::Code),
        ("Cargo.lock", Format::Code),
        ("nested/CARGO.LOCK", Format::Code),
        ("Dockerfile", Format::Code),
        ("nested/dockerfile.dev", Format::Code),
        ("build.Dockerfile", Format::Code),
        ("Containerfile", Format::Code),
        ("Containerfile.test", Format::Code),
        ("build.containerfile", Format::Code),
        ("Dockerfile.md", Format::Markdown),
        ("other.lock", Format::Markdown),
        ("data.json", Format::Code),
        ("main.rs", Format::Code),
        ("script.py", Format::Code),
        ("types.pyi", Format::Code),
        ("script.pyw", Format::Code),
        ("app.js", Format::Code),
        ("view.jsx", Format::Code),
        ("module.mjs", Format::Code),
        ("module.cjs", Format::Code),
        ("app.ts", Format::Code),
        ("module.mts", Format::Code),
        ("module.cts", Format::Code),
        ("view.tsx", Format::Code),
        ("main.go", Format::Code),
        ("script.sh", Format::Code),
        ("script.bash", Format::Code),
        ("notes.md", Format::Markdown),
        ("README", Format::Markdown),
        ("notes.unknown", Format::Markdown),
    ] {
        assert_eq!(
            Input::new(Some(name.into()), Format::Auto, None)
                .unwrap()
                .format,
            expected
        );
    }
    assert_eq!(
        Input::new(Some("a.mmd".into()), Format::Markdown, None)
            .unwrap()
            .format,
        Format::Markdown
    );
    assert_eq!(
        Input::new(None, Format::Auto, None).unwrap().format,
        Format::Markdown
    );
    assert!(Input::new(None, Format::Image, None).is_err());
    for format in [Format::Code, Format::Mermaid, Format::Sgf] {
        let source = "literal **bold** $x$ &amp;\r\n```\r\n# heading\r\nlast";
        let input = Input::new(None, format, None).unwrap();
        let document = input.document(source.into());
        assert_eq!(document.source(), source);
        let events: Vec<_> = document.events().collect();
        assert_eq!(events.len(), 3);
        assert!(matches!(&events[1].event, Event::Text(text) if text.as_ref() == source));
        assert_eq!(events[1].source, 0..source.len());
        assert!(export::text(&document).contains("**bold** $x$ &amp;"));
    }
}

#[cfg(unix)]
#[test]
fn special_code_filenames_keep_literal_content_and_select_language() {
    use gloss_cli::input::{Format, Input};
    use pulldown_cmark::{CodeBlockKind, Tag};

    let dir = tempfile::tempdir().unwrap();
    for (name, language, source) in [
        (
            "Cargo.lock",
            "toml",
            "# Generated lockfile\nversion = 4\n[[package]]\nname = \"demo\"\n",
        ),
        (
            "Dockerfile",
            "dockerfile",
            "# Build image\nFROM scratch\nLABEL note=\"**literal** $VALUE$\"\n",
        ),
        (
            "Dockerfile.dev",
            "dockerfile",
            "# Build image\nFROM scratch\n",
        ),
        ("Dockerfile.json", "json", "{\"enabled\": true}\n"),
    ] {
        let path = dir.path().join(name);
        std::fs::write(&path, source).unwrap();
        let input = Input::new(Some(path.clone()), Format::Auto, None).unwrap();
        let document = input.document(input.read(false).unwrap());
        assert!(matches!(document.events().next().unwrap().event,
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(label))) if label.as_ref() == language));
        let output = cli(&[path.to_str().unwrap()], "");
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .contains(source.trim_end())
        );
    }
}

#[test]
fn code_line_numbers_never_change_plain_or_source_exports() {
    let source = "# Comment\r\n\r\nvalue: '**literal** $x$ &amp;'\r\nlast: café";
    let plain = cli(&["--language", "yaml", "-"], source);
    assert!(plain.status.success());
    for numbers in ["auto", "on", "off"] {
        for output in ["auto", "text", "source"] {
            let result = cli(
                &[
                    "--language",
                    "yaml",
                    "--line-numbers",
                    numbers,
                    "--output",
                    output,
                    "--width",
                    "5",
                    "--pager",
                    "-",
                ],
                source,
            );
            assert!(result.status.success());
            assert!(result.stderr.is_empty());
            assert_eq!(
                result.stdout,
                if output == "source" {
                    source.as_bytes()
                } else {
                    &plain.stdout
                }
            );
        }
    }
}

#[test]
fn standalone_language_selection_is_explicit_and_keeps_markdown_inert() {
    use gloss_cli::input::{Format, Input};
    use pulldown_cmark::{CodeBlockKind, Tag};

    let source = "# Comment\nvalue: '**literal** $x$ &amp;'\n```\n";
    for (name, format, language) in [
        (Some("config.YAML"), Format::Auto, None),
        (Some("config.yml"), Format::Code, None),
        (Some("settings"), Format::Auto, Some("yaml")),
        (Some("notes.md"), Format::Auto, Some("yaml")),
        (None, Format::Code, Some("yaml")),
        (Some("-"), Format::Auto, Some("yaml")),
    ] {
        let input = Input::new(name.map(Into::into), format, language.map(Into::into)).unwrap();
        assert_eq!(input.format, Format::Code);
        let document = input.document(source.into());
        let events: Vec<_> = document.events().collect();
        assert_eq!(events.len(), 3);
        assert!(matches!(&events[0].event,
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(label))) if label.as_ref() == "yaml"));
        assert!(matches!(&events[1].event, Event::Text(text) if text.as_ref() == source));
        assert_eq!(document.source(), source);
    }
    for format in [
        Format::Markdown,
        Format::Mermaid,
        Format::Sgf,
        Format::Image,
    ] {
        let error =
            Input::new(Some("config.yaml".into()), format, Some("yaml".into())).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("--language"));
    }
}

#[cfg(unix)]
#[test]
fn standalone_yaml_cli_keeps_comments_and_exact_source_and_allows_markdown_override() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.YAML");
    let source = "# Comment\r\nmessage: \"**bold** $x$ &amp;\"\r\nlast: café";
    std::fs::write(&path, source).unwrap();
    let path = path.to_str().unwrap();
    for args in [vec![path], vec!["--output", "text", path]] {
        let result = cli(&args, "");
        assert!(result.status.success());
        assert!(result.stderr.is_empty());
        let text = String::from_utf8(result.stdout).unwrap();
        assert!(text.contains("# Comment\nmessage: \"**bold** $x$ &amp;\"\nlast: café"));
        assert!(!text.contains('\x1b'));
    }
    let exact = cli(&["--output", "source", path], "");
    assert!(exact.status.success());
    assert_eq!(exact.stdout, source.as_bytes());
    let stdin = cli(&["--language", "YML", "--output", "source", "-"], source);
    assert!(stdin.status.success());
    assert_eq!(stdin.stdout, source.as_bytes());
    let markdown = cli(&["--input-format", "markdown", path], "");
    assert!(markdown.status.success());
    assert!(
        !String::from_utf8(markdown.stdout)
            .unwrap()
            .contains("# Comment")
    );
    let conflict = cli(
        &["--input-format", "markdown", "--language", "yaml", path],
        "",
    );
    assert!(!conflict.status.success());
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("--language"));
}

#[test]
fn standalone_code_exports_preserve_bytes_and_bypass_graphics_selection() {
    for format in ["code", "mermaid", "sgf"] {
        let source = "literal **source**\r\n$unparsed$\n```\nno final newline";
        let exact = cli(
            &[
                "--input-format",
                format,
                "--output",
                "source",
                "--sgf-game",
                "99",
                "-",
            ],
            source,
        );
        assert!(exact.status.success());
        assert_eq!(exact.stdout, source.as_bytes());
        assert!(exact.stderr.is_empty());
        let text = cli(
            &["--input-format", format, "--pager", "--sgf", "kitty", "-"],
            source,
        );
        assert!(text.status.success());
        assert!(text.stderr.is_empty());
        assert!(
            String::from_utf8(text.stdout)
                .unwrap()
                .contains("literal **source**\n$unparsed$\n```")
        );
    }
}

#[cfg(unix)]
#[test]
fn standalone_images_export_references_and_reject_binary_source_output() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a [100%] café.png");
    std::fs::write(&path, b"not an image; export must not decode").unwrap();
    let path = path.to_str().unwrap();
    let text = cli(&[path], "");
    assert!(text.status.success());
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(text.contains("[image: a [100%] café.png]"));
    assert!(text.contains(path));
    let source = cli(&["--output", "source", path], "");
    assert!(!source.status.success());
    assert!(source.stdout.is_empty());
    assert!(String::from_utf8_lossy(&source.stderr).contains("original image file"));
    let stdin = cli(&["--input-format", "image", "-"], "");
    assert!(!stdin.status.success());
    assert!(String::from_utf8_lossy(&stdin.stderr).contains("named file"));
}

fn cli(args: &[&str], input: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_gloss"))
        .args(args)
        // Even when inherited from Ghostty, a pipe must not receive graphics.
        .env("TERM_PROGRAM", "ghostty")
        .env("TERM", "xterm-ghostty")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn preserves_math_sources_and_excludes_code() {
    let document = Document::parse(FIXTURE.into());
    let mut display = Vec::new();
    let mut inline = Vec::new();
    for item in document.events() {
        match &item.event {
            Event::DisplayMath(tex) => display.push((
                tex.to_string(),
                document.source_slice(item.source.clone()).unwrap(),
            )),
            Event::InlineMath(tex) => inline.push(tex.to_string()),
            _ => {}
        }
    }
    assert_eq!(display.len(), 3);
    assert!(display[0].1.starts_with("$$"));
    assert!(display[1].1.starts_with("```math"));
    assert!(display[1].1.trim_end().ends_with("```"));
    assert_eq!(inline, ["E = mc^2", r"\alpha + \beta"]);
    assert_eq!(export::source(&document), FIXTURE);
}

#[test]
fn source_pipe_preserves_exact_utf8_crlf_and_final_newline_state() {
    let input = "# café\r\n\r\n$α^2$  \r\nno final newline";
    let output = cli(
        &["--output", "source", "--width", "12", "--margin", "9", "-"],
        input,
    );
    assert!(output.status.success());
    assert_eq!(output.stdout, input.as_bytes());
    assert!(output.stderr.is_empty());
}

#[test]
fn pager_does_not_change_exports_and_watch_rejects_pipes() {
    for mode in ["auto", "text", "source"] {
        let direct = cli(&["--output", mode, "-"], FIXTURE);
        let pager = cli(&["--pager", "--output", mode, "-"], FIXTURE);
        assert!(pager.status.success());
        assert_eq!(pager.stdout, direct.stdout);
        assert_eq!(pager.stderr, direct.stderr);
    }
    for args in [
        &["--watch", "-"][..],
        &["--watch", "tests/fixtures/math.md"][..],
    ] {
        let result = cli(args, "");
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8_lossy(&result.stderr).contains("--watch requires"));
    }
}

#[test]
fn redirected_output_ignores_graphics_theme_width_and_margins() {
    let output = cli(
        &[
            "--math",
            "kitty",
            "--show-math-source",
            "--width",
            "12",
            "--margin",
            "9",
            "--light",
            "-",
        ],
        FIXTURE,
    );
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains('\x1b'));
    assert!(!text.contains('\u{10eeee}'));
    assert!(text.contains("Ordinary bold, italic, and code remain terminal text."));
    assert!(text.contains(r"\int_0^1"));
    assert!(text.contains("$E = mc^2$"));
    assert!(text.contains("$this is code, not math$"));
    assert_eq!(text.matches("\\int_0^1").count(), 1);
}

#[test]
fn text_export_keeps_long_lines_and_list_markers_attached() {
    let long = "a".repeat(300);
    let input = format!("# Title\n\n{long}\n\n- one\n- **two**\n\n3. three\n4. four\n");
    let text = export::text(&Document::parse(input));
    assert!(text.contains(&long));
    assert!(text.contains("- one\n- two\n"), "{text:?}");
    assert!(text.contains("3. three\n4. four\n"), "{text:?}");
}

#[test]
fn block_prefixes_stay_with_the_first_paragraph() {
    let input = "> A quote\n> with two lines.\n\n- Loose item\n\n- Second item\n\n[^a]: A note.\n";
    let text = export::text(&Document::parse(input.into()));
    assert!(text.contains("> A quote with two lines."), "{text:?}");
    assert!(text.contains("- Loose item\n\n- Second item"), "{text:?}");
    assert!(text.contains("[^a]: A note."), "{text:?}");
}

#[test]
fn rendered_content_cannot_emit_input_control_sequences() {
    let document = Document::parse("hello\x1b]52;c;payload\x07\n\n```\n\x1b[2J\n```\n".into());
    let text = export::text(&document);
    assert!(!text.contains('\x1b'));
    assert!(!text.contains('\x07'));
    assert!(document.source().contains('\x1b'));
}

#[test]
fn fallback_rendering_matches_plain_export() {
    let document = Document::parse(FIXTURE.into());
    let mut output = Vec::new();
    let warnings = render::write(
        &mut output,
        &document,
        &layout::layout(&document),
        &render::Options::default(),
    )
    .unwrap();
    assert!(warnings.is_empty());
    assert_eq!(String::from_utf8(output).unwrap(), export::text(&document));
}

#[test]
fn code_blocks_retain_languages_content_and_source_spans() {
    let source = "> ```Rs title=sample\n> /* comment\n> continued */\n> fn main() {}\n> ```\n\n    indented\n\n```math\nx^2\n```\n";
    let document = Document::parse(source.into());
    let flow = layout::layout(&document);
    let codes: Vec<_> = flow
        .blocks
        .iter()
        .filter_map(|block| match block {
            layout::Block::Code {
                language,
                runs,
                source,
                ..
            } => Some((language, runs, source)),
            _ => None,
        })
        .collect();
    assert_eq!(codes.len(), 2);
    assert_eq!(codes[0].0.as_deref(), Some("Rs"));
    assert!(codes[1].0.is_none());
    let body: String = codes[0]
        .1
        .iter()
        .filter(|run| run.style.role == layout::Role::CodeBlock)
        .map(|run| run.text.as_str())
        .collect();
    assert_eq!(body, "/* comment\ncontinued */\nfn main() {}\n");
    assert!(
        document
            .source_slice(codes[0].2.clone())
            .unwrap()
            .contains("```Rs")
    );
    assert_eq!(export::source(&document), source);
    assert!(export::text(&document).contains("> /* comment\ncontinued */\nfn main() {}\n"));
}

#[test]
fn repeated_event_iteration_preserves_normalized_math_and_source_ranges() {
    let source = "A &amp; **B**.\r\n\r\n```math\r\nx^2\r\n```\r\n\r\nAfter $y$.\r\n";
    let document = Document::parse(source.into());
    let collect = || {
        document
            .events()
            .map(|item| (format!("{:?}", item.event), item.source))
            .collect::<Vec<_>>()
    };
    assert_eq!(collect(), collect());
    assert!(
        collect()
            .iter()
            .any(|(event, _)| event.starts_with("DisplayMath"))
    );
    assert_eq!(export::source(&document), source);
    assert!(export::text(&document).ends_with("After $y$.\n\n"));
}

#[test]
fn table_cells_retain_alignment_inline_styles_and_source_spans() {
    use pulldown_cmark::Alignment;
    let source = "| Left | Center | Right |\r\n| :--- | :---: | ---: |\r\n| **bold** &amp; a\\|b | `code` | $x^2$ |\r\n| short |\r\n";
    let document = Document::parse(source.into());
    let flow = layout::layout(&document);
    let table = flow
        .blocks
        .iter()
        .find_map(|block| match block {
            layout::Block::Table(table) => Some(table),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        table.alignments,
        [Alignment::Left, Alignment::Center, Alignment::Right]
    );
    assert_eq!(table.rows.len(), 3);
    assert!(table.rows[0].header && !table.rows[1].header);
    assert_eq!(table.rows[2].cells.len(), 3);
    assert!(table.rows[2].cells[1].runs.is_empty());
    let first = &table.rows[1].cells[0];
    assert_eq!(
        first
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "bold & a|b"
    );
    assert!(first.runs[0].style.bold);
    assert!(
        document
            .source_slice(first.source.clone())
            .unwrap()
            .contains("**bold**")
    );
    assert!(table.rows[1].cells[1].runs[0].style.code);
    assert_eq!(table.rows[1].cells[2].runs[0].math.as_deref(), Some("x^2"));
    assert!(
        document
            .source_slice(table.source.clone())
            .unwrap()
            .contains("| short |")
    );
    assert_eq!(export::source(&document), source);
}

fn terminal_table(source: &str, width: u16) -> String {
    let document = Document::parse(source.into());
    let options = render::Options {
        terminal: Some(Geometry::default()),
        width,
        margin: 0,
        ..Default::default()
    };
    let mut output = Vec::new();
    assert!(
        render::write(&mut output, &document, &layout::layout(&document), &options)
            .unwrap()
            .is_empty()
    );
    String::from_utf8(output).unwrap()
}

#[test]
fn tables_align_and_wrap_without_splitting_unicode_graphemes() {
    use unicode_width::UnicodeWidthStr;
    let source = "| Left | Center | Right |\n| :--- | :---: | ---: |\n| a | b | c |\n| wideleft | end | 42 |\n";
    let output = terminal_table(source, 40);
    assert!(output.contains("a        │   b    │     c"), "{output}");
    assert!(output.contains("wideleft │  end   │    42"), "{output}");
    let unicode = "| Name | Text |\n| --- | --- |\n| 中文 | e\u{301}👩‍💻界e\u{301}👩‍💻界 |\n";
    let output = terminal_table(unicode, 12);
    assert!(
        output
            .lines()
            .all(|line| UnicodeWidthStr::width(line) <= 12),
        "{output}"
    );
    assert_eq!(output.matches("👩‍💻").count(), 2);
    assert_eq!(output.matches("e\u{301}").count(), 2);
    assert_eq!(output.matches('界').count(), 2);
}

#[test]
fn table_fallbacks_keep_every_cell_and_rows_after_measurement_limits() {
    let source = "| Alpha | Beta | Gamma |\n| --- | --- | --- |\n| first | second | third |\n";
    let output = terminal_table(source, 14);
    assert!(
        output.contains("1: first") && output.contains("2: second") && output.contains("3: third"),
        "{output}"
    );
    assert!(!output.contains('┼'));
    let source = format!(
        "| H |\n| --- |\n{}| LATE_SENTINEL |\n",
        "| early |\n".repeat(100)
    );
    let output = terminal_table(&source, 20);
    let joined: String = output.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(joined.contains("LATE_SENTINEL"));
    let long = "x".repeat(70 * 1024);
    let output = terminal_table(&format!("| H |\n| --- |\n| {long} |\n| AFTER |\n"), 40);
    assert_eq!(output.matches('x').count(), long.len());
    assert!(output.contains("AFTER"));
    let labels: Vec<_> = (0..17).map(|index| format!("cell_{index}_end")).collect();
    let source = format!(
        "| {} |\n| {} |\n| {} |\n",
        labels.join(" | "),
        ["---"; 17].join(" | "),
        labels.join(" | ")
    );
    let output = terminal_table(&source, 80);
    assert!(!output.contains('┼'));
    for label in labels {
        assert_eq!(output.matches(&label).count(), 2);
    }
}

#[test]
fn table_plain_export_preserves_existing_separators_and_ignores_viewport() {
    for prefix in ["", "> ", "- "] {
        let source = format!(
            "{prefix}| Name | Value |\n{}| --- | ---: |\n{}| one | $x$ |\n",
            if prefix == "- " { "  " } else { prefix },
            if prefix == "- " { "  " } else { prefix }
        );
        let expected = format!(
            "{}Name | Value | \none | $x$ | \n\n",
            if prefix.is_empty() {
                String::new()
            } else {
                format!("{prefix}\n\n")
            }
        );
        let document = Document::parse(source.clone());
        assert_eq!(export::text(&document), expected);
        let mut output = Vec::new();
        render::write(
            &mut output,
            &document,
            &layout::layout(&document),
            &Default::default(),
        )
        .unwrap();
        assert_eq!(output, expected.as_bytes());
        let plain = cli(
            &["--width", "8", "--light", "--output", "text", "-"],
            &source,
        );
        assert!(plain.status.success());
        assert_eq!(plain.stdout, expected.as_bytes());
    }
}

#[test]
fn image_blocks_keep_alt_text_source_ranges_and_independent_exports() {
    let source = "Before ![**Blue** &amp; `green`](assets/picture.png) after.\r\n\r\n| Icon |\r\n| --- |\r\n| ![cell](assets/picture.png) |\r\n";
    let document = Document::parse(source.into());
    let flow = layout::with_images(&document);
    let images: Vec<_> = flow
        .blocks
        .iter()
        .filter_map(|block| match block {
            layout::Block::Image(image) => Some(image),
            _ => None,
        })
        .collect();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].alt, "Blue & green");
    assert_eq!(images[0].target, "assets/picture.png");
    assert_eq!(
        document.source_slice(images[0].source.clone()).unwrap(),
        "![**Blue** &amp; `green`](assets/picture.png)"
    );
    assert!(
        layout::layout(&document)
            .blocks
            .iter()
            .all(|block| !matches!(block, layout::Block::Image(_)))
    );
    let plain = cli(
        &[
            "--images",
            "kitty",
            "--base-dir",
            "missing",
            "--output",
            "text",
            "-",
        ],
        source,
    );
    assert!(plain.status.success() && plain.stderr.is_empty());
    assert_eq!(plain.stdout, export::text(&document).as_bytes());
    assert!(
        String::from_utf8(plain.stdout)
            .unwrap()
            .contains("Before [image: Blue & green] (assets/picture.png) after.")
    );
    let exact = cli(&["--images", "kitty", "--output", "source", "-"], source);
    assert!(exact.status.success());
    assert_eq!(exact.stdout, source.as_bytes());
}

#[test]
fn filtering_preserves_unicode_and_whitespace_but_removes_c0_and_c1_controls() {
    let input = "café\0\u{1b}\u{7}\u{85}\u{9b}\n\t中文 👩‍💻";
    assert_eq!(render::text::sanitize(input), "café\n\t中文 👩‍💻");
}

#[test]
fn highlighting_options_do_not_change_plain_or_exact_source_exports() {
    let source = "```rust\r\nfn café() { let value = \"$not_math$\"; }\r\n```\r\n";
    let plain = cli(&["-"], source);
    let disabled = cli(&["--no-highlight", "-"], source);
    assert!(plain.status.success() && disabled.status.success());
    assert_eq!(plain.stdout, disabled.stdout);
    assert!(!plain.stdout.contains(&0x1b));
    let exact = cli(&["--no-highlight", "--output", "source", "-"], source);
    assert_eq!(exact.stdout, source.as_bytes());
}

#[test]
fn math_errors_fall_back_without_suppressing_the_rest_of_the_document() {
    let document = Document::parse("$$\n\\definitelynotacommand{x}\n$$\n\nStill here.\n".into());
    let mut output = Vec::new();
    let options = render::Options {
        kitty: Some(Geometry::default()),
        ..Default::default()
    };
    let warnings =
        render::write(&mut output, &document, &layout::layout(&document), &options).unwrap();
    let text = String::from_utf8(output).unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(text.contains(r"\definitelynotacommand{x}"));
    assert!(text.contains("Still here."));
    assert!(!text.contains("\x1b_G"));
}

#[test]
fn kitty_transmits_a_complete_rgba_image_and_optional_selectable_source() {
    let document = Document::parse("$$\n\\frac{a+b}{c}\\tag{42}\n$$\n".into());
    let mut output = Vec::new();
    let options = render::Options {
        kitty: Some(Geometry::default()),
        show_math_source: true,
        ..Default::default()
    };
    assert!(
        render::write(&mut output, &document, &layout::layout(&document), &options)
            .unwrap()
            .is_empty()
    );
    let text = String::from_utf8(output).unwrap();
    let commands: Vec<_> = text
        .split("\x1b_G")
        .skip(1)
        .map(|part| part.split_once("\x1b\\").unwrap().0)
        .collect();
    assert!(!commands.is_empty());
    let header = commands[0].split_once(';').unwrap().0;
    let number = |key: &str| -> usize {
        header
            .split(',')
            .find_map(|pair| pair.strip_prefix(key))
            .unwrap()
            .parse()
            .unwrap()
    };
    assert!(header.contains("a=T,"));
    assert!(header.contains("f=32,"));
    assert!(header.contains("C=1,"));
    let mut encoded = String::new();
    for (index, command) in commands.iter().enumerate() {
        let (control, payload) = command.split_once(';').unwrap();
        assert!(payload.len() <= 4096);
        assert_eq!(payload.len() % 4, 0);
        assert!(control.contains("q=2"));
        assert!(control.ends_with(if index + 1 == commands.len() {
            "m=0"
        } else {
            "m=1"
        }));
        encoded.push_str(payload);
    }
    let mut rgba = STANDARD.decode(encoded).unwrap();
    if header.contains("o=z") {
        use std::io::Read;
        let mut pixels = Vec::new();
        flate2::read::ZlibDecoder::new(rgba.as_slice())
            .read_to_end(&mut pixels)
            .unwrap();
        rgba = pixels;
    }
    assert_eq!(rgba.len(), number("s=") * number("v=") * 4);
    assert!(rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] > 200));
    assert_eq!(text.matches(r"\frac{a+b}{c}\tag{42}").count(), 1);
}

#[test]
fn native_math_fits_bounds_includes_tags_and_validates_dimensions() {
    let color = Color {
        r: 230,
        g: 230,
        b: 230,
        a: 255,
    };
    let mut engine = MathEngine::default();
    let body = engine.render_display("x", 24., color, 800, 400).unwrap();
    let tagged = engine
        .render_display(r"x\tag{42}", 24., color, 800, 400)
        .unwrap();
    assert!(tagged.width > body.width);
    assert!(tagged.baseline > 0.);
    assert_eq!(
        tagged.rgba.len(),
        (tagged.width * tagged.height * 4) as usize
    );
    let wide = engine
        .render_display(
            r"\underbrace{a+b+c+d+e+f+g+h+i+j}_{\text{wide formula}}",
            96.,
            color,
            120,
            80,
        )
        .unwrap();
    assert!(wide.width <= 120 && wide.height <= 80);
    assert!(
        engine
            .render_display("x", f64::NAN, color, 800, 400)
            .is_err()
    );
    assert!(engine.render_display("x", 24., color, 0, 400).is_err());
}

#[test]
fn inline_images_keep_prose_as_text_and_offer_selectable_source() {
    let document = Document::parse("Before $\\frac{a}{b}$ after.\n".into());
    let mut output = Vec::new();
    let mut options = render::Options {
        kitty: Some(Geometry::default()),
        ..Default::default()
    };
    let layout = layout::layout(&document);
    assert!(
        render::write(&mut output, &document, &layout, &options)
            .unwrap()
            .is_empty()
    );
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("Before "));
    assert!(text.contains(" after."));
    assert_eq!(text.matches("\x1b_Ga=T,").count(), 1);
    assert!(!text.contains(r"\frac{a}{b}"));

    output = Vec::new();
    options.show_math_source = true;
    assert!(
        render::write(&mut output, &document, &layout, &options)
            .unwrap()
            .is_empty()
    );
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("TeX: $\\frac{a}{b}$"));
    assert_eq!(text.matches(r"\frac{a}{b}").count(), 1);
    assert!(export::text(&document).contains("Before $\\frac{a}{b}$ after."));
}

#[test]
fn an_invalid_inline_formula_keeps_its_source_and_other_formulas_render() {
    let document = Document::parse("Bad $\\definitelynotacommand{x}$ and good $x^2$.\n".into());
    let mut output = Vec::new();
    let options = render::Options {
        kitty: Some(Geometry::default()),
        ..Default::default()
    };
    let warnings =
        render::write(&mut output, &document, &layout::layout(&document), &options).unwrap();
    let text = String::from_utf8(output).unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(text.contains(r"$\definitelynotacommand{x}$"));
    assert_eq!(text.matches("\x1b_Ga=T,").count(), 1);
}

#[test]
fn inline_layout_does_not_advance_display_equation_numbers() {
    let color = Color {
        r: 230,
        g: 230,
        b: 230,
        a: 255,
    };
    let formula = r"\begin{equation}x=1\end{equation}";
    let mut engine = MathEngine::default();
    engine
        .render_inline(r"\frac{x}{y}", 24., color, 800, 400)
        .unwrap();
    let actual = engine
        .render_display(formula, 24., color, 800, 400)
        .unwrap();
    let expected = MathEngine::default()
        .render_display(formula, 24., color, 800, 400)
        .unwrap();
    assert_eq!(
        (actual.width, actual.height),
        (expected.width, expected.height)
    );
    assert_eq!(actual.rgba, expected.rgba);
}

#[test]
fn mermaid_modes_preserve_plain_and_exact_exports() {
    let source = "Before.\r\n\r\n```mermaid\r\nflowchart LR; A-->B\r\n```\r\nAfter.";
    let expected = export::text(&Document::parse(source.into()));
    for mode in ["auto", "off", "kitty"] {
        let plain = cli(&["--mermaid", mode, "--show-mermaid-source", "-"], source);
        assert!(plain.status.success() && plain.stderr.is_empty());
        assert_eq!(plain.stdout, expected.as_bytes());
        let exact = cli(&["--mermaid", mode, "--output", "source", "-"], source);
        assert!(exact.status.success() && exact.stderr.is_empty());
        assert_eq!(exact.stdout, source.as_bytes());
    }
}

#[test]
fn backslash_math_preserves_spans_and_excludes_escaped_delimiters_and_code() {
    let source = r#"Inline \(a_*b+\alpha\) and $c$.

\[
\frac{x}{y}
\]

`\(code\)` and ``\[also code\]`` and \\(escaped\\).

```tex
\(fenced\)
\[fenced\]
```

    \(indented\)

[Link \(x\)](https://example.com/\(path\))
"#;
    let document = Document::parse(source.into());
    let math: Vec<_> = document
        .events()
        .filter_map(|event| match event.event {
            Event::InlineMath(tex) | Event::DisplayMath(tex) => Some((
                tex.into_string(),
                document.source_slice(event.source).unwrap().to_owned(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        math,
        [
            (r"a_*b+\alpha".into(), r"\(a_*b+\alpha\)".into()),
            ("c".into(), "$c$".into()),
            ("\n\\frac{x}{y}\n".into(), "\\[\n\\frac{x}{y}\n\\]".into()),
            ("x".into(), r"\(x\)".into()),
        ]
    );
    assert_eq!(export::source(&document), source);
    let plain = export::text(&document);
    assert!(plain.contains(r"\(a_*b+\alpha\)"));
    assert!(plain.contains(r"\["));
    assert!(plain.contains(r"\(fenced\)"));
    assert!(plain.contains("https://example.com/(path)"));
    assert_eq!(document.events().count(), document.events().count());
}

#[test]
fn extensions_expand_details_preserve_metadata_and_keep_exact_source() {
    let source = "---\r\ntitle: **literal**\r\nunsafe: !!tag &alias\r\n---\r\n\r\n<details>\r\n<summary>Result \\(x\\)</summary>\r\n\r\nBody **bold**.\r\n\r\n<details open>\r\n<summary>Nested</summary>\r\n\r\nStill here.\r\n\r\n</details>\r\n\r\n</details>";
    let document = Document::parse(source.into());
    let text = export::text(&document);
    assert!(text.contains("title: **literal**"));
    assert!(text.contains("unsafe: !!tag &alias"));
    assert!(text.contains("Body bold.") && text.contains("Still here."));
    assert!(!text.contains("<details") && !text.contains("<summary>"));
    let math = document
        .events()
        .find(|event| matches!(event.event, Event::InlineMath(_)))
        .unwrap();
    assert_eq!(document.source_slice(math.source).unwrap(), r"\(x\)");
    assert_eq!(export::source(&document), source);
    for event in document.events() {
        assert!(document.source_slice(event.source).is_some());
    }
}

#[test]
fn incomplete_oversized_and_code_extensions_keep_content() {
    for source in [
        "---\nunclosed: true\nBody".to_owned(),
        format!("---\ntitle: {}\n---\nBody", "x".repeat(17 * 1024)),
        "<details>\n<summary>Unclosed</summary>\n\nBody".to_owned(),
        "```html\n<details>\n<summary>Code</summary>\n</details>\n```".to_owned(),
        "<!--\n<details>\n<summary>Comment</summary>\n</details>\n-->".to_owned(),
        format!(
            "<details>\n<summary>Large</summary>\n\n{}\n</details>",
            "x".repeat(257 * 1024)
        ),
    ] {
        let document = Document::parse(source.clone());
        assert_eq!(export::source(&document), source);
        let text = export::text(&document);
        if source.contains("<details>") {
            assert!(text.contains("<details>"), "{text}");
        } else {
            assert!(text.contains("Body"));
        }
    }
    let source = "\\(unclosed\n\nOther paragraph \\(x\\) and `code`";
    let document = Document::parse(source.into());
    assert_eq!(
        document
            .events()
            .filter(|event| matches!(event.event, Event::InlineMath(_)))
            .count(),
        1
    );
}

#[test]
fn alerts_have_labels_and_streamed_output_matches_retained_layout() {
    let source = include_str!("fixtures/extensions.md");
    let document = Document::parse(source.into());
    let text = export::text(&document);
    for label in ["Note", "Tip", "Important", "Warning", "Caution"] {
        assert!(text.contains(label));
    }
    assert!(!text.contains("[!NOTE]"));
    for source in [
        source,
        include_str!("fixtures/style.md"),
        include_str!("fixtures/tables.md"),
        include_str!("fixtures/inline.md"),
    ] {
        let document = Document::parse(source.into());
        let options = render::Options {
            terminal: Some(Geometry::default()),
            styled: true,
            links: true,
            ..Default::default()
        };
        let (mut retained, mut streamed) = (Vec::new(), Vec::new());
        render::write(
            &mut retained,
            &document,
            &layout::layout(&document),
            &options,
        )
        .unwrap();
        render::write_document(&mut streamed, &document, &options).unwrap();
        assert_eq!(retained, streamed);
    }
}

#[test]
fn hyperlinks_wrap_without_leaking_and_reject_unsafe_targets() {
    let source = "[A long **link label** that wraps](https://example.com/path?q=1) end. [unsafe](javascript:alert) [relative](file.md)";
    let document = Document::parse(source.into());
    let options = render::Options {
        terminal: Some(Geometry::default()),
        links: true,
        width: 20,
        ..Default::default()
    };
    let mut out = Vec::new();
    render::write_document(&mut out, &document, &options).unwrap();
    let value = String::from_utf8(out).unwrap();
    assert!(value.contains("\x1b]8;;https://example.com/path?q=1\x1b\\"));
    assert!(!value.contains("\x1b]8;;javascript:") && !value.contains("\x1b]8;;file.md"));
    assert!(!export::text(&document).contains('\x1b'));
    let mut active = false;
    let mut rest = value.as_str();
    while let Some((prefix, next)) = rest.split_once("\x1b]8;;") {
        assert!(!active || !prefix.contains('\n'));
        let (target, suffix) = next.split_once("\x1b\\").unwrap();
        active = !target.is_empty();
        rest = suffix;
    }
    assert!(!active);
    for invalid in [
        "",
        "javascript:alert(1)",
        "file:///etc/passwd",
        "https://",
        "https://example.com/\x1b]52;bad",
        "https://example.com/\u{009c}",
        "https://example.com/a\nb",
        "https://example.com/a b",
        "https:\\example.com",
    ] {
        assert!(!gloss_cli::links::allowed(invalid), "{invalid:?}");
    }
    for valid in [
        "https://example.com/path?q=1#anchor",
        "mailto:reader@example.com",
        "HTTP://example.com",
    ] {
        assert!(gloss_cli::links::allowed(valid));
    }
}

#[test]
fn multiline_math_in_quotes_and_tables_retains_tex_without_container_markers() {
    let source = "> \\[\n> x^2\n> \\]\n\n| Equation |\n| --- |\n| \\(x_1\\) |\n";
    let document = Document::parse(source.into());
    let math: Vec<_> = document
        .events()
        .filter_map(|item| match item.event {
            Event::InlineMath(tex) | Event::DisplayMath(tex) => Some(tex.into_string()),
            _ => None,
        })
        .collect();
    assert_eq!(math, ["\nx^2\n", "x_1"]);
}

#[test]
fn adjacent_math_and_unicode_projections_never_leak_placeholders() {
    for source in [
        r"\(α+β\)\(γ\)",
        r"\(x\),\(y\).",
        r"\[x\]\[y\]",
        "é \\(x\\) **bold** \\(界\\)\r\n",
    ] {
        let document = Document::parse(source.into());
        let equations: Vec<_> = document
            .events()
            .filter_map(|item| match item.event {
                Event::InlineMath(tex) | Event::DisplayMath(tex) => Some(tex.into_string()),
                _ => None,
            })
            .collect();
        assert_eq!(equations.len(), 2, "{source}: {equations:?}");
        assert!(!equations.iter().any(|tex| tex == "m"));
        for item in document.events() {
            assert!(document.source_slice(item.source).is_some());
        }
    }
}

#[test]
fn many_openers_before_protected_code_keep_source_without_math() {
    let source = format!("{} `code` \\)\n", "\\(".repeat(4000));
    let document = Document::parse(source.clone());
    assert_eq!(
        document
            .events()
            .filter(|item| matches!(item.event, Event::InlineMath(_)))
            .count(),
        0
    );
    assert_eq!(export::source(&document), source);
}
