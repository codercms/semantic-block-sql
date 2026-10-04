pub mod go;
mod go_string;
pub mod rust;

pub(super) fn looks_like_complete_sql_prefix(source: &str) -> bool {
    let trimmed = source.trim_start();
    let word = trimmed
        .bytes()
        .take_while(|byte| byte.is_ascii_alphabetic())
        .collect::<Vec<_>>();
    let Ok(word) = std::str::from_utf8(&word) else {
        return false;
    };
    matches!(
        word.to_ascii_uppercase().as_str(),
        "WITH"
            | "SELECT"
            | "INSERT"
            | "UPDATE"
            | "DELETE"
            | "MERGE"
            | "CREATE"
            | "ALTER"
            | "DROP"
            | "DO"
            | "CALL"
            | "GRANT"
            | "REVOKE"
            | "TRUNCATE"
            | "COMMENT"
            | "COPY"
            | "EXPLAIN"
            | "VACUUM"
            | "ANALYZE"
            | "REFRESH"
            | "LISTEN"
            | "NOTIFY"
    )
}
