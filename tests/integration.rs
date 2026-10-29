use std::{
    io::Write,
    process::{Command, Stdio},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use katex_native::Color;
use mdviewer::{document::Document, export, layout, math::MathEngine, render, terminal::Geometry};
use pulldown_cmark::Event;

const FIXTURE: &str = include_str!("fixtures/math.md");

fn cli(args: &[&str], input: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mdviewer"))
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
    assert!(commands.len() > 1, "fixture should exercise chunking");
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
    let rgba = STANDARD.decode(encoded).unwrap();
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
