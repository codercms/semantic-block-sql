use pg_query::protobuf::{KeywordKind, Token};

use super::FormatDiagnostic;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SqlToken<'a> {
    pub kind: Token,
    pub keyword_kind: KeywordKind,
    pub text: &'a str,
    pub newline_before: bool,
}

impl SqlToken<'_> {
    pub fn is_comment(&self) -> bool {
        matches!(self.kind, Token::SqlComment | Token::CComment)
    }
}

pub(super) fn tokenize(source: &str) -> Result<Vec<SqlToken<'_>>, FormatDiagnostic> {
    let scanned = pg_query::scan(source)
        .map_err(|error| FormatDiagnostic::PostgreSqlScan(error.to_string()))?;
    let mut previous_end = 0usize;
    let mut result = Vec::with_capacity(scanned.tokens.len());

    for scanned in scanned.tokens {
        let start = usize::try_from(scanned.start)
            .map_err(|_| FormatDiagnostic::PostgreSqlScan("negative token start".into()))?;
        let end = usize::try_from(scanned.end)
            .map_err(|_| FormatDiagnostic::PostgreSqlScan("negative token end".into()))?;
        let gap = source.get(previous_end..start).ok_or_else(|| {
            FormatDiagnostic::PostgreSqlScan("token gap was not valid UTF-8".into())
        })?;
        let text = source.get(start..end).ok_or_else(|| {
            FormatDiagnostic::PostgreSqlScan("token byte range was not valid UTF-8".into())
        })?;
        let kind = Token::try_from(scanned.token).map_err(|_| {
            FormatDiagnostic::PostgreSqlScan(format!("unknown PostgreSQL token {}", scanned.token))
        })?;
        let keyword_kind = KeywordKind::try_from(scanned.keyword_kind).map_err(|_| {
            FormatDiagnostic::PostgreSqlScan(format!(
                "unknown PostgreSQL keyword kind {}",
                scanned.keyword_kind
            ))
        })?;
        result.push(SqlToken {
            kind,
            keyword_kind,
            text,
            newline_before: gap.contains('\n'),
        });
        previous_end = end;
    }

    Ok(result)
}
