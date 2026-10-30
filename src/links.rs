//! OSC 8 is emitted only for bounded, printable absolute web/mail links.
//! Unsupported targets remain visible text; this module never opens a URL.

pub fn allowed(target: &str) -> bool {
    if target.is_empty()
        || target.len() > 2048
        || target
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace() || ch == '\\')
    {
        return false;
    }
    let Some((scheme, rest)) = target.split_once(':') else {
        return false;
    };
    if scheme.eq_ignore_ascii_case("mailto") {
        return !rest.is_empty() && !rest.starts_with(['?', '#', '/']);
    }
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    let Some(rest) = rest.strip_prefix("//") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    !authority.is_empty() && !authority.ends_with('@')
}
