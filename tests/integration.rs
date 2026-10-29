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
                tex.as_ref(),
                document.source_slice(item.source.clone()).unwrap(),
            )),
            Event::InlineMath(tex) => inline.push(tex.as_ref()),
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
