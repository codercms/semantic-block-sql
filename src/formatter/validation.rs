use std::collections::HashSet;

use pg_query::NodeRef;
use pg_query::protobuf::node::Node as NodeEnum;
use pg_query::protobuf::{RawStmt, SelectStmt, SetOperation, Token};
use serde_json::Value;

use super::FormatDiagnostic;
use super::tokens;

pub(super) fn parse_supported_postgresql(source: &str) -> Result<(), FormatDiagnostic> {
    let parsed = pg_query::parse(source)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;

    let mut allowed_recursive_unions = HashSet::new();
    for raw in &parsed.protobuf.stmts {
        validate_statement(raw, &mut allowed_recursive_unions)
            .map_err(|feature| unsupported(source, raw, feature))?;
    }

    for (node, _, _, _) in parsed.protobuf.nodes() {
        validate_nested_node(node, &allowed_recursive_unions).map_err(|feature| {
            FormatDiagnostic::UnsupportedSyntax {
                feature: feature.into(),
                start: 0,
                end: source.len(),
            }
        })?;
    }

    Ok(())
}

fn validate_statement(
    raw: &RawStmt,
    allowed_recursive_unions: &mut HashSet<usize>,
) -> Result<(), &'static str> {
    let node = raw
        .stmt
        .as_deref()
        .and_then(|statement| statement.node.as_ref())
        .ok_or("empty PostgreSQL statement")?;

    match node {
        NodeEnum::SelectStmt(select) => validate_select(select, false, allowed_recursive_unions),
        NodeEnum::InsertStmt(_) => Err("INSERT statement"),
        NodeEnum::UpdateStmt(_) => Err("UPDATE statement"),
        NodeEnum::DeleteStmt(_) => Err("DELETE statement"),
        NodeEnum::MergeStmt(_) => Err("MERGE statement"),
        NodeEnum::CreateStmt(_) => Err("CREATE TABLE statement"),
        NodeEnum::IndexStmt(_) => Err("CREATE INDEX statement"),
        NodeEnum::AlterTableStmt(_) => Err("ALTER TABLE statement"),
        NodeEnum::CreateFunctionStmt(_) => Err("function or procedure definition"),
        NodeEnum::DoStmt(_) => Err("DO block"),
        _ => Err("unimplemented PostgreSQL statement family"),
    }
}

fn validate_select(
    select: &SelectStmt,
    allow_recursive_union: bool,
    allowed_recursive_unions: &mut HashSet<usize>,
) -> Result<(), &'static str> {
    validate_select_fields(select)?;

    if let Some(with_clause) = &select.with_clause {
        for cte_node in &with_clause.ctes {
            let cte = match cte_node.node.as_ref() {
                Some(NodeEnum::CommonTableExpr(cte)) => cte,
                _ => return Err("unrecognized common table expression"),
            };
            if cte.search_clause.is_some() || cte.cycle_clause.is_some() {
                return Err("CTE SEARCH or CYCLE clause");
            }
            let query = cte
                .ctequery
                .as_deref()
                .and_then(|query| query.node.as_ref())
                .ok_or("empty common table expression")?;
            match query {
                NodeEnum::SelectStmt(select) => {
                    validate_select(select, with_clause.recursive, allowed_recursive_unions)?
                }
                _ => return Err("data-modifying common table expression"),
            }
        }
    }

    let operation = SetOperation::try_from(select.op).unwrap_or(SetOperation::Undefined);
    match operation {
        SetOperation::SetopNone => Ok(()),
        SetOperation::SetopUnion if allow_recursive_union && select.all => {
            allowed_recursive_unions.insert(select as *const SelectStmt as usize);
            let left = select.larg.as_deref().ok_or("incomplete UNION ALL")?;
            let right = select.rarg.as_deref().ok_or("incomplete UNION ALL")?;
            validate_select(left, false, allowed_recursive_unions)?;
            validate_select(right, false, allowed_recursive_unions)
        }
        SetOperation::SetopUnion => Err("general UNION or UNION ALL expression"),
        SetOperation::SetopIntersect => Err("INTERSECT expression"),
        SetOperation::SetopExcept => Err("EXCEPT expression"),
        SetOperation::Undefined => Err("unknown set operation"),
    }
}

fn validate_select_fields(select: &SelectStmt) -> Result<(), &'static str> {
    if !select.distinct_clause.is_empty() {
        return Err("SELECT DISTINCT clause");
    }
    if select.into_clause.is_some() {
        return Err("SELECT INTO clause");
    }
    if !select.group_clause.is_empty() || select.group_distinct {
        return Err("GROUP BY clause");
    }
    if select.having_clause.is_some() {
        return Err("HAVING clause");
    }
    if !select.window_clause.is_empty() {
        return Err("WINDOW clause");
    }
    if !select.values_lists.is_empty() {
        return Err("VALUES statement");
    }
    if !select.sort_clause.is_empty() {
        return Err("ORDER BY clause");
    }
    if select.limit_offset.is_some() || select.limit_count.is_some() {
        return Err("LIMIT, OFFSET, or FETCH clause");
    }
    if !select.locking_clause.is_empty() {
        return Err("row-locking clause");
    }
    Ok(())
}

fn validate_nested_node(
    node: NodeRef<'_>,
    allowed_recursive_unions: &HashSet<usize>,
) -> Result<(), &'static str> {
    match node {
        NodeRef::SelectStmt(select) => {
            validate_select_fields(select)?;
            let operation = SetOperation::try_from(select.op).unwrap_or(SetOperation::Undefined);
            if operation != SetOperation::SetopNone
                && !allowed_recursive_unions.contains(&(select as *const SelectStmt as usize))
            {
                return Err("general set-operation expression");
            }
            Ok(())
        }
        NodeRef::FuncCall(call)
            if !call.agg_order.is_empty()
                || call.agg_filter.is_some()
                || call.over.is_some()
                || call.agg_within_group =>
        {
            Err("ordered, filtered, or window function call")
        }
        NodeRef::RangeSubselect(range) if range.lateral => Err("LATERAL subquery"),
        NodeRef::RangeFunction(range) if range.lateral => Err("LATERAL function"),
        NodeRef::RangeTableFunc(range) if range.lateral => Err("LATERAL table function"),
        NodeRef::JsonTable(_) => Err("JSON_TABLE expression"),
        _ => Ok(()),
    }
}

fn unsupported(source: &str, raw: &RawStmt, feature: &'static str) -> FormatDiagnostic {
    let start = usize::try_from(raw.stmt_location)
        .unwrap_or(0)
        .min(source.len());
    let length = usize::try_from(raw.stmt_len).unwrap_or(0);
    let mut end = if length == 0 {
        source.len()
    } else {
        start.saturating_add(length).min(source.len())
    };
    if source.as_bytes().get(end) == Some(&b';') {
        end += 1;
    }
    FormatDiagnostic::UnsupportedSyntax {
        feature: feature.into(),
        start,
        end,
    }
}

/// Validates structural equivalence and exact preservation of protected text.
pub fn validate_equivalent(source: &str, formatted: &str) -> Result<(), FormatDiagnostic> {
    let source_tree = canonical_tree(source)?;
    let formatted_tree = canonical_tree(formatted)?;
    if source_tree != formatted_tree {
        return Err(FormatDiagnostic::SemanticMismatch);
    }

    let before = protected_tokens(source)?;
    let after = protected_tokens(formatted)?;
    if before != after {
        let detail = before
            .iter()
            .zip(after.iter())
            .find(|(left, right)| left != right)
            .map_or_else(
                || format!("token count differs: {} != {}", before.len(), after.len()),
                |(left, right)| format!("{left:?} != {right:?}"),
            );
        return Err(FormatDiagnostic::ProtectedTokenChanged(detail));
    }

    Ok(())
}

fn canonical_tree(source: &str) -> Result<Value, FormatDiagnostic> {
    let parsed = pg_query::parse(source)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;
    let mut tree = serde_json::to_value(&parsed.protobuf)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;
    strip_locations(&mut tree);
    Ok(tree)
}

fn strip_locations(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for name in ["location", "stmt_location", "stmt_len"] {
                fields.remove(name);
            }
            for child in fields.values_mut() {
                strip_locations(child);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(strip_locations),
        _ => {}
    }
}

fn protected_tokens(source: &str) -> Result<Vec<(Token, String)>, FormatDiagnostic> {
    Ok(tokens::tokenize(source)?
        .into_iter()
        .filter(|token| {
            matches!(
                token.kind,
                Token::Sconst
                    | Token::Usconst
                    | Token::Bconst
                    | Token::Xconst
                    | Token::SqlComment
                    | Token::CComment
            ) || token.text.starts_with('"')
        })
        .map(|token| (token.kind, token.text.to_owned()))
        .collect())
}
