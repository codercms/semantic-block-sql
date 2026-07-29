use std::collections::{BTreeMap, HashMap, HashSet};

use thiserror::Error;
use tree_sitter::{Node, Parser, Tree};

use crate::config::GoConfig;
use crate::{Diagnostic, FormatDiagnostic, FormatOptions, FormatWarning, SourceRange, format_sql};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormattedGo {
    pub output: String,
    pub warnings: Vec<FormatWarning>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Error)]
pub enum GoError {
    #[error("failed to initialize Go parser: {0}")]
    Parser(String),
    #[error("Go parse failed")]
    Parse,
    #[error("misplaced Go directive {directive} at line {line}")]
    MisplacedDirective { directive: String, line: usize },
    #[error("conflicting semblock:ignore and SQL marker at line {line}")]
    ConflictingDirectives { line: usize },
    #[error(
        "explicit SQL marker targets an interpreted Go string at line {line}; interpreted strings are disabled in the MVP"
    )]
    InterpretedString { line: usize },
    #[error("raw Go SQL strings are disabled by configuration")]
    RawStringsDisabled,
    #[error("embedded SQL at Go line {line}: {source}")]
    EmbeddedSql {
        line: usize,
        #[source]
        source: FormatDiagnostic,
    },
    #[error("rewritten Go source does not parse")]
    Reparse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GoDirective {
    FileIgnore,
    Ignore,
    Sql,
}

#[derive(Debug, Clone, Copy)]
struct CommentDirective {
    kind: GoDirective,
    start: usize,
    line: usize,
}

#[derive(Debug)]
struct Owner<'tree> {
    node: Node<'tree>,
    raw: Vec<Node<'tree>>,
    interpreted: Vec<Node<'tree>>,
}

#[derive(Debug)]
struct Replacement {
    start: usize,
    end: usize,
    text: String,
}

pub fn format_go_source(
    source: &str,
    options: &FormatOptions,
    config: &GoConfig,
) -> Result<FormattedGo, GoError> {
    let tree = parse_go(source)?;
    let root = tree.root_node();
    if root.has_error() {
        return Err(GoError::Parse);
    }

    let mut raw = Vec::new();
    let mut interpreted = Vec::new();
    let mut comments = Vec::new();
    collect_nodes(root, &mut raw, &mut interpreted, &mut comments);

    let directives = comments
        .iter()
        .filter_map(|comment| parse_comment_directive(*comment, source))
        .collect::<Vec<_>>();
    let package_start = find_first_kind(root, "package_clause")
        .map(|node| node.start_byte())
        .unwrap_or(0);

    if directives.iter().any(|directive| {
        directive.kind == GoDirective::FileIgnore && directive.start < package_start
    }) {
        return Ok(FormattedGo {
            output: source.into(),
            warnings: Vec::new(),
            diagnostics: Vec::new(),
        });
    }
    if let Some(directive) = directives
        .iter()
        .find(|directive| directive.kind == GoDirective::FileIgnore)
    {
        return Err(GoError::MisplacedDirective {
            directive: "semblock:file-ignore".into(),
            line: directive.line,
        });
    }

    let mut owners = BTreeMap::<(usize, usize), Owner<'_>>::new();
    for node in raw {
        if let Some(owner) = supported_owner(node) {
            owners
                .entry((owner.start_byte(), owner.end_byte()))
                .or_insert_with(|| Owner {
                    node: owner,
                    raw: Vec::new(),
                    interpreted: Vec::new(),
                })
                .raw
                .push(node);
        }
    }
    for node in interpreted {
        if let Some(owner) = supported_owner(node) {
            owners
                .entry((owner.start_byte(), owner.end_byte()))
                .or_insert_with(|| Owner {
                    node: owner,
                    raw: Vec::new(),
                    interpreted: Vec::new(),
                })
                .interpreted
                .push(node);
        }
    }

    let directive_by_start = directives
        .iter()
        .map(|directive| (directive.start, *directive))
        .collect::<HashMap<_, _>>();
    let mut consumed = HashSet::new();
    let mut replacements = Vec::new();
    let mut warnings = Vec::new();
    let mut diagnostics = Vec::new();

    for owner in owners.values() {
        let attached = attached_directives(owner.node, &comments, source)
            .into_iter()
            .filter_map(|start| directive_by_start.get(&start).copied())
            .collect::<Vec<_>>();
        for directive in &attached {
            consumed.insert(directive.start);
        }

        let ignored = attached
            .iter()
            .any(|directive| directive.kind == GoDirective::Ignore);
        let explicit = attached
            .iter()
            .any(|directive| directive.kind == GoDirective::Sql);
        if ignored && explicit {
            return Err(GoError::ConflictingDirectives {
                line: attached[0].line,
            });
        }
        if ignored {
            continue;
        }
        if explicit && !owner.interpreted.is_empty() {
            return Err(GoError::InterpretedString {
                line: owner.node.start_position().row + 1,
            });
        }
        if owner.raw.is_empty() {
            continue;
        }
        if !config.raw_strings {
            if explicit {
                return Err(GoError::RawStringsDisabled);
            }
            continue;
        }
        if !explicit && !config.auto_detect {
            continue;
        }

        for literal in &owner.raw {
            let content_start = literal.start_byte() + 1;
            let content_end = literal.end_byte().saturating_sub(1);
            let content = &source[content_start..content_end];
            let envelope = RawEnvelope::new(content);
            if !explicit && !looks_like_complete_sql_prefix(&envelope.sql) {
                continue;
            }

            let formatted =
                format_sql(&envelope.sql, options).map_err(|source| GoError::EmbeddedSql {
                    line: literal.start_position().row + 1,
                    source,
                })?;
            warnings.extend(formatted.warnings);
            diagnostics.extend(formatted.diagnostics.into_iter().map(|mut diagnostic| {
                diagnostic.message = format!("embedded SQL: {}", diagnostic.message);
                diagnostic.with_source_range(SourceRange::new(content_start, content_end))
            }));
            replacements.push(Replacement {
                start: content_start,
                end: content_end,
                text: envelope.wrap(&formatted.output),
            });
        }
    }

    if let Some(directive) = directives
        .iter()
        .find(|directive| !consumed.contains(&directive.start))
    {
        return Err(GoError::MisplacedDirective {
            directive: directive_name(directive.kind).into(),
            line: directive.line,
        });
    }

    replacements.sort_by_key(|replacement| replacement.start);
    let mut output = source.to_string();
    for replacement in replacements.iter().rev() {
        output.replace_range(replacement.start..replacement.end, &replacement.text);
    }

    let reparsed = parse_go(&output)?;
    if reparsed.root_node().has_error() {
        return Err(GoError::Reparse);
    }
    Ok(FormattedGo {
        output,
        warnings,
        diagnostics,
    })
}

fn parse_go(source: &str) -> Result<Tree, GoError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_go::LANGUAGE.into())
        .map_err(|error| GoError::Parser(error.to_string()))?;
    parser.parse(source, None).ok_or(GoError::Parse)
}

fn collect_nodes<'tree>(
    node: Node<'tree>,
    raw: &mut Vec<Node<'tree>>,
    interpreted: &mut Vec<Node<'tree>>,
    comments: &mut Vec<Node<'tree>>,
) {
    match node.kind() {
        "raw_string_literal" => raw.push(node),
        "interpreted_string_literal" => interpreted.push(node),
        "comment" => comments.push(node),
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_nodes(child, raw, interpreted, comments);
    }
}

fn find_first_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find_map(|child| find_first_kind(child, kind))
}

fn supported_owner(mut node: Node<'_>) -> Option<Node<'_>> {
    while let Some(parent) = node.parent() {
        if matches!(
            parent.kind(),
            "const_declaration"
                | "var_declaration"
                | "short_var_declaration"
                | "assignment_statement"
        ) {
            return Some(parent);
        }
        node = parent;
    }
    None
}

fn parse_comment_directive(node: Node<'_>, source: &str) -> Option<CommentDirective> {
    let text = &source[node.start_byte()..node.end_byte()];
    let body = text.trim().strip_prefix("//")?.trim();
    let kind = match body {
        "semblock:file-ignore" => GoDirective::FileIgnore,
        "semblock:ignore" => GoDirective::Ignore,
        "semblock:sql" | "language=SQL" => GoDirective::Sql,
        _ => return None,
    };
    Some(CommentDirective {
        kind,
        start: node.start_byte(),
        line: node.start_position().row + 1,
    })
}

fn attached_directives(owner: Node<'_>, comments: &[Node<'_>], source: &str) -> Vec<usize> {
    let mut previous_start = owner.start_byte();
    let mut attached = Vec::new();
    for comment in comments.iter().rev() {
        if comment.end_byte() > previous_start {
            continue;
        }
        let gap = &source[comment.end_byte()..previous_start];
        if !gap.chars().all(char::is_whitespace) || line_breaks(gap) > 1 {
            if !attached.is_empty() {
                break;
            }
            continue;
        }
        attached.push(comment.start_byte());
        previous_start = comment.start_byte();
    }
    attached
}

fn line_breaks(source: &str) -> usize {
    source.bytes().filter(|byte| *byte == b'\n').count()
}

fn directive_name(directive: GoDirective) -> &'static str {
    match directive {
        GoDirective::FileIgnore => "semblock:file-ignore",
        GoDirective::Ignore => "semblock:ignore",
        GoDirective::Sql => "semblock:sql/language=SQL",
    }
}

fn looks_like_complete_sql_prefix(source: &str) -> bool {
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
    )
}

struct RawEnvelope {
    sql: String,
    newline: &'static str,
    multiline: bool,
    closing_indent: String,
}

impl RawEnvelope {
    fn new(content: &str) -> Self {
        let newline = if content.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let normalized = content.replace("\r\n", "\n");
        let multiline = normalized.starts_with('\n');
        if !multiline {
            return Self {
                sql: normalized,
                newline,
                multiline: false,
                closing_indent: String::new(),
            };
        }

        let without_opening = &normalized[1..];
        let (body, closing_indent) = match without_opening.rsplit_once('\n') {
            Some((body, suffix)) if suffix.chars().all(char::is_whitespace) => (body, suffix),
            _ => (without_opening, ""),
        };
        let content_indent = body
            .lines()
            .find(|line| !line.trim().is_empty())
            .map(leading_whitespace)
            .unwrap_or_default();
        let sql = body
            .lines()
            .map(|line| line.strip_prefix(&content_indent).unwrap_or(line))
            .collect::<Vec<_>>()
            .join("\n");
        Self {
            sql,
            newline,
            multiline: true,
            closing_indent: closing_indent.into(),
        }
    }

    fn wrap(&self, formatted: &str) -> String {
        let formatted = formatted.trim_end_matches('\n');
        if self.multiline {
            let mut output = String::new();
            output.push_str(self.newline);
            for line in formatted.lines() {
                output.push_str(line);
                output.push_str(self.newline);
            }
            output.push_str(&self.closing_indent);
            output
        } else {
            formatted.lines().collect::<Vec<_>>().join(self.newline)
        }
    }
}

fn leading_whitespace(line: &str) -> String {
    line.chars()
        .take_while(|char| char.is_whitespace())
        .collect()
}
