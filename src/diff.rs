use similar::TextDiff;

pub fn unified(path: &str, original: &str, formatted: &str) -> String {
    TextDiff::from_lines(original, formatted)
        .unified_diff()
        .header(&format!("a/{path}"), &format!("b/{path}"))
        .to_string()
}
