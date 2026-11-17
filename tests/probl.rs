use gloss_cli::{
    document::Document,
    export,
    input::{Format, Input},
};
use std::{
    io::Write,
    process::{Command, Stdio},
};

const SOURCE: &str = "# café 🎲 **literal** $x$\r\nlet x ~ 2d6\r\nreport \"value {x}\"\r\n";

fn cli(arguments: &[&str], source: &str, no_color: bool) -> Vec<u8> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gloss"));
    command
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if no_color {
        command.env("NO_COLOR", "1");
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    output.stdout
}

#[test]
fn files_fences_stdin_and_plain_modes_preserve_source() {
    for name in ["model.probl", "model.test.PROBL"] {
        let input = Input::new(Some(name.into()), Format::Auto, None).unwrap();
        assert_eq!(input.format, Format::Code);
        let document = input.document(SOURCE.to_owned());
        assert_eq!(document.source(), SOURCE);
        assert!(export::text(&document).contains("**literal** $x$"));
    }
    for label in ["probl", "PROBL"] {
        let fenced = format!("```{label}\n{SOURCE}```\n");
        let document = Document::parse(fenced.clone());
        assert_eq!(document.source(), fenced);
        assert!(export::text(&document).contains("let x ~ 2d6"));
        assert_eq!(
            cli(&["--output", "source", "-"], &fenced, false),
            fenced.as_bytes()
        );
    }
    let plain = cli(&["--language", "probl", "-"], SOURCE, false);
    for no_color in [false, true] {
        assert_eq!(
            cli(
                &["--language", "PROBL", "--no-highlight", "-"],
                SOURCE,
                no_color
            ),
            plain
        );
        assert_eq!(
            cli(
                &["--language", "probl", "--output", "source", "-"],
                SOURCE,
                no_color
            ),
            SOURCE.as_bytes()
        );
    }
    assert!(!plain.contains(&0x1b));
    let large = SOURCE.repeat(4000);
    assert_eq!(
        cli(
            &["--language", "probl", "--output", "source", "-"],
            &large,
            false
        ),
        large.as_bytes()
    );
}
