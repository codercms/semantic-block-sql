use std::collections::{BTreeMap, HashMap, HashSet};

use thiserror::Error;
use tree_sitter::{Node, Parser, Tree};

use super::go_string::{
    GoStringError, can_encode_raw, decode_literal, encode_interpreted, encode_raw,
};
use crate::config::{GoConfig, GoMultilineStringStyle};
use crate::{
    Diagnostic, FormatDiagnostic, FormatOptions, FormatWarning, Severity, SourceRange,
    UnsupportedPolicy, format_sql,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GoFormatStats {
    pub discovered_expressions: usize,
    pub eligible_candidates: usize,
    pub formatted_expressions: usize,
    pub unchanged_sql_expressions: usize,
    pub unsupported_expressions: usize,
    pub auto_parse_skips: usize,
    pub dynamic_expressions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormattedGo {
    pub output: String,
    pub warnings: Vec<FormatWarning>,
    pub diagnostics: Vec<Diagnostic>,
    pub stats: GoFormatStats,
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
    #[error("explicit SQL marker targets a disabled interpreted Go string at line {line}")]
    InterpretedStringsDisabled { line: usize },
    #[error("explicit SQL marker targets a disabled raw Go string at line {line}")]
    RawStringsDisabled { line: usize },
    #[error("invalid Go string literal at line {line}: {source}")]
    StringLiteral {
        line: usize,
        #[source]
        source: GoStringError,
    },
    #[error("embedded SQL at Go line {line}: {source}")]
    EmbeddedSql {
        line: usize,
        #[source]
        source: FormatDiagnostic,
    },
    #[error("rewritten Go string does not preserve its formatted runtime value at line {line}")]
    RuntimeValueMismatch { line: usize },
    #[error("rewritten Go source does not parse")]
    Reparse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GoDirective {
    FileIgnore,
    Ignore,
    Sql,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GoStringKind {
    Raw,
    Interpreted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GoStringExpressionKind {
    RawLiteral,
    InterpretedLiteral,
    StaticConcat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GoSqlContext {
    DeclarationInitializer,
    AssignmentValue,
    CallArgument,
    ReturnValue,
    CompositeLiteralValue,
    DeferCall,
    GoCall,
    ExpressionStatement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GoSqlOwnerKind {
    ConstDeclaration,
    VarDeclaration,
    ShortVarDeclaration,
    Assignment,
    Return,
    ExpressionStatement,
    Defer,
    Go,
}

impl GoSqlOwnerKind {
    fn from_node_kind(kind: &str) -> Option<Self> {
        match kind {
            "const_declaration" => Some(Self::ConstDeclaration),
            "var_declaration" => Some(Self::VarDeclaration),
            "short_var_declaration" => Some(Self::ShortVarDeclaration),
            "assignment_statement" => Some(Self::Assignment),
            "return_statement" => Some(Self::Return),
            "expression_statement" => Some(Self::ExpressionStatement),
            "defer_statement" => Some(Self::Defer),
            "go_statement" => Some(Self::Go),
            _ => None,
        }
    }

    fn default_context(self) -> GoSqlContext {
        match self {
            Self::ConstDeclaration | Self::VarDeclaration => GoSqlContext::DeclarationInitializer,
            Self::ShortVarDeclaration | Self::Assignment => GoSqlContext::AssignmentValue,
            Self::Return => GoSqlContext::ReturnValue,
            Self::ExpressionStatement => GoSqlContext::ExpressionStatement,
            Self::Defer => GoSqlContext::DeferCall,
            Self::Go => GoSqlContext::GoCall,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct CommentDirective {
    kind: GoDirective,
    start: usize,
    line: usize,
}

#[derive(Debug, Clone, Copy)]
struct GoStringExpression<'tree> {
    node: Node<'tree>,
    owner: Node<'tree>,
    context: GoSqlContext,
    kind: GoStringExpressionKind,
    has_raw: bool,
    has_interpreted: bool,
}

#[derive(Debug)]
struct Owner<'tree> {
    node: Node<'tree>,
    expressions: Vec<GoStringExpression<'tree>>,
    dynamic_ranges: Vec<SourceRange>,
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

    let mut expressions = Vec::new();
    let mut dynamic = Vec::new();
    let mut comments = Vec::new();
    collect_source_nodes(root, source, &mut expressions, &mut dynamic, &mut comments);
    let mut stats = GoFormatStats {
        discovered_expressions: expressions.len(),
        dynamic_expressions: dynamic.len(),
        ..GoFormatStats::default()
    };

    let directives = comments
        .iter()
        .filter_map(|comment| parse_comment_directive(*comment, source))
        .collect::<Vec<_>>();
    let package_start = find_first_kind(root, "package_clause")
        .map(|node| node.start_byte())
        .unwrap_or(0);

    if config.ignore_generated_files && has_generated_header(&comments, source, package_start) {
        return Ok(FormattedGo {
            output: source.into(),
            warnings: Vec::new(),
            diagnostics: Vec::new(),
            stats,
        });
    }

    if directives.iter().any(|directive| {
        directive.kind == GoDirective::FileIgnore && directive.start < package_start
    }) {
        return Ok(FormattedGo {
            output: source.into(),
            warnings: Vec::new(),
            diagnostics: Vec::new(),
            stats,
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
    for expression in expressions {
        owners
            .entry((expression.owner.start_byte(), expression.owner.end_byte()))
            .or_insert_with(|| Owner {
                node: expression.owner,
                expressions: Vec::new(),
                dynamic_ranges: Vec::new(),
            })
            .expressions
            .push(expression);
    }
    for (owner, range) in dynamic {
        owners
            .entry((owner.start_byte(), owner.end_byte()))
            .or_insert_with(|| Owner {
                node: owner,
                expressions: Vec::new(),
                dynamic_ranges: Vec::new(),
            })
            .dynamic_ranges
            .push(range);
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
        if !explicit && !config.auto_detect {
            continue;
        }

        if explicit {
            stats.unsupported_expressions += owner.dynamic_ranges.len();
            diagnostics.extend(owner.dynamic_ranges.iter().map(|range| {
                unsupported_go_diagnostic(
                    *range,
                    options,
                    "dynamic Go string expression containing SQL fragments",
                )
            }));
        }

        for expression in &owner.expressions {
            let _context = expression.context;
            if expression.has_raw && !config.raw_strings {
                if explicit {
                    return Err(GoError::RawStringsDisabled {
                        line: expression.node.start_position().row + 1,
                    });
                }
                continue;
            }
            if expression.has_interpreted && !config.interpreted_strings {
                if explicit {
                    return Err(GoError::InterpretedStringsDisabled {
                        line: expression.node.start_position().row + 1,
                    });
                }
                continue;
            }

            let prepared = match prepare_expression(*expression, source) {
                Ok(prepared) => prepared,
                Err(GoError::StringLiteral {
                    source: GoStringError::InvalidUtf8,
                    ..
                }) if !explicit => continue,
                Err(error) => return Err(error),
            };
            if !explicit && !looks_like_complete_sql_prefix(&prepared.sql) {
                continue;
            }
            stats.eligible_candidates += 1;

            let formatted = match format_sql(&prepared.sql, options) {
                Ok(formatted) => formatted,
                Err(error) if !explicit && is_candidate_parse_failure(&error) => {
                    stats.auto_parse_skips += 1;
                    continue;
                }
                Err(source) => {
                    return Err(GoError::EmbeddedSql {
                        line: expression.node.start_position().row + 1,
                        source,
                    });
                }
            };
            let expression_unsupported = formatted
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.rule_id == "syntax.unsupported");
            if expression_unsupported {
                stats.unsupported_expressions += 1;
            }
            warnings.extend(formatted.warnings);
            let diagnostic_range = expression_diagnostic_range(*expression);
            diagnostics.extend(formatted.diagnostics.into_iter().map(|mut diagnostic| {
                diagnostic.message = format!("embedded SQL: {}", diagnostic.message);
                diagnostic.with_source_range(diagnostic_range)
            }));

            if formatted.output == prepared.sql
                && matches!(
                    expression.kind,
                    GoStringExpressionKind::RawLiteral | GoStringExpressionKind::InterpretedLiteral
                )
            {
                stats.unchanged_sql_expressions += 1;
                continue;
            }

            let rendered = render_expression(*expression, &prepared, &formatted.output, config)?;
            let decoded = decode_literal(&rendered).map_err(|source| GoError::StringLiteral {
                line: expression.node.start_position().row + 1,
                source,
            })?;
            let expected = prepared.expected_runtime(&formatted.output);
            if decoded != expected {
                return Err(GoError::RuntimeValueMismatch {
                    line: expression.node.start_position().row + 1,
                });
            }
            stats.formatted_expressions += 1;
            replacements.push(Replacement {
                start: expression.node.start_byte(),
                end: expression.node.end_byte(),
                text: rendered,
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
    let strict_unsupported = options.unsupported_policy == UnsupportedPolicy::Error
        && diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.rule_id.as_str(),
                "syntax.unsupported" | "format.statement_skipped"
            ) && diagnostic.severity == Severity::Error
        });
    let mut output = source.to_string();
    if !strict_unsupported {
        for replacement in replacements.iter().rev() {
            output.replace_range(replacement.start..replacement.end, &replacement.text);
        }
    }

    let reparsed = parse_go(&output)?;
    if reparsed.root_node().has_error() {
        return Err(GoError::Reparse);
    }
    Ok(FormattedGo {
        output,
        warnings,
        diagnostics,
        stats,
    })
}

fn expression_diagnostic_range(expression: GoStringExpression<'_>) -> SourceRange {
    match expression.kind {
        GoStringExpressionKind::RawLiteral | GoStringExpressionKind::InterpretedLiteral => {
            SourceRange::new(
                expression.node.start_byte().saturating_add(1),
                expression.node.end_byte().saturating_sub(1),
            )
        }
        GoStringExpressionKind::StaticConcat => {
            SourceRange::new(expression.node.start_byte(), expression.node.end_byte())
        }
    }
}

#[derive(Debug)]
struct PreparedExpression {
    sql: String,
    raw_envelope: Option<RawEnvelope>,
}

impl PreparedExpression {
    fn expected_runtime(&self, formatted: &str) -> String {
        self.raw_envelope.as_ref().map_or_else(
            || formatted.to_owned(),
            |envelope| envelope.wrap(formatted).replace('\r', ""),
        )
    }
}

fn prepare_expression(
    expression: GoStringExpression<'_>,
    source: &str,
) -> Result<PreparedExpression, GoError> {
    if expression.kind == GoStringExpressionKind::RawLiteral {
        let text = &source[expression.node.start_byte()..expression.node.end_byte()];
        let content = text
            .strip_prefix('`')
            .and_then(|text| text.strip_suffix('`'))
            .ok_or_else(|| GoError::StringLiteral {
                line: expression.node.start_position().row + 1,
                source: GoStringError::Delimiters,
            })?;
        let envelope = RawEnvelope::new(content);
        return Ok(PreparedExpression {
            sql: envelope.sql.clone(),
            raw_envelope: Some(envelope),
        });
    }

    let sql = decode_static_expression(expression.node, source).map_err(|source| {
        GoError::StringLiteral {
            line: expression.node.start_position().row + 1,
            source,
        }
    })?;
    Ok(PreparedExpression {
        sql,
        raw_envelope: None,
    })
}

fn render_expression(
    expression: GoStringExpression<'_>,
    prepared: &PreparedExpression,
    formatted: &str,
    config: &GoConfig,
) -> Result<String, GoError> {
    if expression.kind == GoStringExpressionKind::RawLiteral {
        let content = prepared
            .raw_envelope
            .as_ref()
            .expect("raw literal has an envelope")
            .wrap(formatted);
        return Ok(format!("`{content}`"));
    }

    if formatted.contains('\n')
        && config.multiline_string_style == GoMultilineStringStyle::PreferRaw
        && can_encode_raw(formatted)
    {
        return encode_raw(formatted).map_err(|source| GoError::StringLiteral {
            line: expression.node.start_position().row + 1,
            source,
        });
    }
    Ok(encode_interpreted(formatted))
}

fn collect_source_nodes<'tree>(
    node: Node<'tree>,
    source: &str,
    expressions: &mut Vec<GoStringExpression<'tree>>,
    dynamic: &mut Vec<(Node<'tree>, SourceRange)>,
    comments: &mut Vec<Node<'tree>>,
) {
    if node.kind() == "comment" {
        comments.push(node);
        return;
    }

    if matches!(
        node.kind(),
        "binary_expression" | "parenthesized_expression"
    ) {
        if is_static_string_expression(node, source) {
            if let Some(expression) = classify_string_expression(node, source) {
                expressions.push(expression);
            }
            return;
        }
        if node.kind() == "binary_expression" && subtree_contains_string(node) {
            if let Some((owner, _)) = classify_context(node) {
                dynamic.push((owner, SourceRange::new(node.start_byte(), node.end_byte())));
            }
            return;
        }
    }

    if let Some(kind) = literal_kind(node) {
        if let Some(mut expression) = classify_string_expression(node, source) {
            expression.kind = match kind {
                GoStringKind::Raw => GoStringExpressionKind::RawLiteral,
                GoStringKind::Interpreted => GoStringExpressionKind::InterpretedLiteral,
            };
            expressions.push(expression);
        }
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_source_nodes(child, source, expressions, dynamic, comments);
    }
}

fn classify_string_expression<'tree>(
    node: Node<'tree>,
    source: &str,
) -> Option<GoStringExpression<'tree>> {
    let (owner, context) = classify_context(node)?;
    let mut kinds = Vec::new();
    collect_literal_kinds(node, source, &mut kinds)?;
    let has_raw = kinds.contains(&GoStringKind::Raw);
    let has_interpreted = kinds.contains(&GoStringKind::Interpreted);
    let kind = match literal_kind(node) {
        Some(GoStringKind::Raw) => GoStringExpressionKind::RawLiteral,
        Some(GoStringKind::Interpreted) => GoStringExpressionKind::InterpretedLiteral,
        None => GoStringExpressionKind::StaticConcat,
    };
    Some(GoStringExpression {
        node,
        owner,
        context,
        kind,
        has_raw,
        has_interpreted,
    })
}

fn classify_context(node: Node<'_>) -> Option<(Node<'_>, GoSqlContext)> {
    let expression_start = node.start_byte();
    let expression_end = node.end_byte();
    let mut current = node;
    let mut context = None;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            "import_spec" | "import_declaration" | "field_declaration" | "package_clause" => {
                return None;
            }
            "keyed_element" => {
                let value = parent.child_by_field_name("value")?;
                if expression_start < value.start_byte() || expression_end > value.end_byte() {
                    return None;
                }
                context.get_or_insert(GoSqlContext::CompositeLiteralValue);
            }
            "literal_element" => {
                context.get_or_insert(GoSqlContext::CompositeLiteralValue);
            }
            "argument_list" => {
                context.get_or_insert(GoSqlContext::CallArgument);
            }
            kind => {
                if let Some(owner_kind) = GoSqlOwnerKind::from_node_kind(kind) {
                    let context = match owner_kind {
                        GoSqlOwnerKind::Defer => GoSqlContext::DeferCall,
                        GoSqlOwnerKind::Go => GoSqlContext::GoCall,
                        _ => context.unwrap_or_else(|| owner_kind.default_context()),
                    };
                    return Some((parent, context));
                }
            }
        }
        current = parent;
    }
    None
}

fn is_static_string_expression(node: Node<'_>, source: &str) -> bool {
    if literal_kind(node).is_some() {
        return true;
    }
    match node.kind() {
        "parenthesized_expression" => named_children(node)
            .into_iter()
            .next()
            .is_some_and(|child| is_static_string_expression(child, source)),
        "binary_expression" => {
            let left = node.child_by_field_name("left");
            let right = node.child_by_field_name("right");
            let operator = node.child_by_field_name("operator");
            matches!(
                (left, right, operator),
                (Some(left), Some(right), Some(operator))
                    if &source[operator.start_byte()..operator.end_byte()] == "+"
                        && is_static_string_expression(left, source)
                        && is_static_string_expression(right, source)
            )
        }
        _ => false,
    }
}

fn collect_literal_kinds(
    node: Node<'_>,
    source: &str,
    kinds: &mut Vec<GoStringKind>,
) -> Option<()> {
    if let Some(kind) = literal_kind(node) {
        kinds.push(kind);
        return Some(());
    }
    if !is_static_string_expression(node, source) {
        return None;
    }
    match node.kind() {
        "parenthesized_expression" => {
            collect_literal_kinds(named_children(node).into_iter().next()?, source, kinds)
        }
        "binary_expression" => {
            collect_literal_kinds(node.child_by_field_name("left")?, source, kinds)?;
            collect_literal_kinds(node.child_by_field_name("right")?, source, kinds)
        }
        _ => None,
    }
}

fn decode_static_expression(node: Node<'_>, source: &str) -> Result<String, GoStringError> {
    if literal_kind(node).is_some() {
        return decode_literal(&source[node.start_byte()..node.end_byte()]);
    }
    match node.kind() {
        "parenthesized_expression" => {
            let child = named_children(node)
                .into_iter()
                .next()
                .ok_or(GoStringError::Delimiters)?;
            decode_static_expression(child, source)
        }
        "binary_expression" => {
            let left = node
                .child_by_field_name("left")
                .ok_or(GoStringError::Delimiters)?;
            let right = node
                .child_by_field_name("right")
                .ok_or(GoStringError::Delimiters)?;
            let mut output = decode_static_expression(left, source)?;
            output.push_str(&decode_static_expression(right, source)?);
            Ok(output)
        }
        _ => Err(GoStringError::Delimiters),
    }
}

fn literal_kind(node: Node<'_>) -> Option<GoStringKind> {
    match node.kind() {
        "raw_string_literal" => Some(GoStringKind::Raw),
        "interpreted_string_literal" => Some(GoStringKind::Interpreted),
        _ => None,
    }
}

fn subtree_contains_string(node: Node<'_>) -> bool {
    if literal_kind(node).is_some() {
        return true;
    }
    let mut cursor = node.walk();
    node.children(&mut cursor).any(subtree_contains_string)
}

fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn unsupported_go_diagnostic(
    range: SourceRange,
    options: &FormatOptions,
    feature: &str,
) -> Diagnostic {
    Diagnostic {
        rule_id: "syntax.unsupported".into(),
        severity: match options.unsupported_policy {
            UnsupportedPolicy::Skip => Severity::Warning,
            UnsupportedPolicy::Error => Severity::Error,
        },
        message: format!("unsupported embedded SQL host expression: {feature}"),
        source_range: range,
        fix_available: false,
    }
}

fn is_candidate_parse_failure(error: &FormatDiagnostic) -> bool {
    matches!(
        error,
        FormatDiagnostic::PostgreSqlParse(_) | FormatDiagnostic::PostgreSqlScan(_)
    )
}

fn parse_go(source: &str) -> Result<Tree, GoError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_go::LANGUAGE.into())
        .map_err(|error| GoError::Parser(error.to_string()))?;
    parser.parse(source, None).ok_or(GoError::Parse)
}

fn find_first_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find_map(|child| find_first_kind(child, kind))
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

fn has_generated_header(comments: &[Node<'_>], source: &str, package_start: usize) -> bool {
    let mut generated = false;
    let mut do_not_edit = false;
    for comment in comments
        .iter()
        .filter(|comment| comment.start_byte() < package_start)
    {
        let text = source[comment.start_byte()..comment.end_byte()].to_ascii_lowercase();
        generated |= text.contains("generated");
        do_not_edit |= text.contains("do not edit");
    }
    generated && do_not_edit
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
            | "COPY"
            | "EXPLAIN"
            | "VACUUM"
            | "ANALYZE"
            | "REFRESH"
            | "LISTEN"
            | "NOTIFY"
    )
}

#[derive(Debug)]
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
        .take_while(|character| character.is_whitespace())
        .collect()
}
