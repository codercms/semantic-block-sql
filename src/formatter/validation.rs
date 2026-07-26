use std::collections::HashSet;

use pg_query::protobuf::node::Node as NodeEnum;
use pg_query::protobuf::{
    DeleteStmt, InsertStmt, Node, OnConflictAction, OverridingKind, RawStmt, SelectStmt,
    SetOperation, Token, UpdateStmt,
};
use pg_query::{Context, NodeRef};
use serde_json::Value;

use super::FormatDiagnostic;
use super::tokens;

#[derive(Debug, Default)]
struct SupportedNodes {
    recursive_unions: HashSet<usize>,
    insert_values: HashSet<usize>,
}

pub(super) fn parse_supported_postgresql(source: &str) -> Result<(), FormatDiagnostic> {
    let parsed = pg_query::parse(source)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;

    let mut supported = SupportedNodes::default();
    for raw in &parsed.protobuf.stmts {
        validate_statement(raw, &mut supported)
            .map_err(|feature| unsupported(source, raw, feature))?;
    }

    for (node, _, context, _) in parsed.protobuf.nodes() {
        validate_nested_node(node, context, &supported).map_err(|feature| {
            FormatDiagnostic::UnsupportedSyntax {
                feature: feature.into(),
                start: 0,
                end: source.len(),
            }
        })?;
    }

    Ok(())
}

fn validate_statement(raw: &RawStmt, supported: &mut SupportedNodes) -> Result<(), &'static str> {
    let node = raw
        .stmt
        .as_deref()
        .and_then(|statement| statement.node.as_ref())
        .ok_or("empty PostgreSQL statement")?;

    match node {
        NodeEnum::SelectStmt(select) => validate_select(select, false, supported),
        NodeEnum::InsertStmt(insert) => validate_insert(insert, supported),
        NodeEnum::UpdateStmt(update) => validate_update(update, supported),
        NodeEnum::DeleteStmt(delete) => validate_delete(delete, supported),
        NodeEnum::MergeStmt(_) => Err("MERGE statement"),
        NodeEnum::CreateStmt(_) => Err("CREATE TABLE statement"),
        NodeEnum::IndexStmt(_) => Err("CREATE INDEX statement"),
        NodeEnum::AlterTableStmt(_) => Err("ALTER TABLE statement"),
        NodeEnum::CreateFunctionStmt(_) => Err("function or procedure definition"),
        NodeEnum::DoStmt(_) => Err("DO block"),
        _ => Err("unimplemented PostgreSQL statement family"),
    }
}

fn validate_insert(
    insert: &InsertStmt,
    supported: &mut SupportedNodes,
) -> Result<(), &'static str> {
    if insert.relation.is_none() {
        return Err("INSERT without a target relation");
    }
    if insert.with_clause.is_some() {
        return Err("INSERT WITH clause");
    }
    if let Some(conflict) = &insert.on_conflict_clause {
        validate_on_conflict(conflict)?;
    }
    if matches!(
        OverridingKind::try_from(insert.r#override).unwrap_or(OverridingKind::Undefined),
        OverridingKind::OverridingUserValue | OverridingKind::OverridingSystemValue
    ) {
        return Err("INSERT OVERRIDING clause");
    }

    let select = insert
        .select_stmt
        .as_deref()
        .and_then(|node| node.node.as_ref())
        .and_then(|node| match node {
            NodeEnum::SelectStmt(select) => Some(select),
            _ => None,
        })
        .ok_or("INSERT without VALUES")?;

    validate_select_fields(select, true)?;
    if select.values_lists.is_empty() {
        return Err("INSERT source query");
    }
    if SetOperation::try_from(select.op).unwrap_or(SetOperation::Undefined)
        != SetOperation::SetopNone
        || select.larg.is_some()
        || select.rarg.is_some()
        || select.with_clause.is_some()
        || !select.target_list.is_empty()
        || !select.from_clause.is_empty()
        || select.where_clause.is_some()
    {
        return Err("INSERT source query");
    }

    supported
        .insert_values
        .insert(select.as_ref() as *const SelectStmt as usize);
    Ok(())
}

fn validate_update(update: &UpdateStmt, supported: &SupportedNodes) -> Result<(), &'static str> {
    let relation = update
        .relation
        .as_ref()
        .ok_or("UPDATE without a target relation")?;
    if !relation.inh {
        return Err("UPDATE ONLY target");
    }
    if update.with_clause.is_some() {
        return Err("UPDATE WITH clause");
    }
    if update.target_list.is_empty() {
        return Err("UPDATE without assignments");
    }

    for target in &update.target_list {
        let target = match target.node.as_ref() {
            Some(NodeEnum::ResTarget(target)) => target,
            _ => return Err("unrecognized UPDATE assignment"),
        };
        if target.name.is_empty() || !target.indirection.is_empty() {
            return Err("complex UPDATE assignment target");
        }
        let value = target
            .val
            .as_deref()
            .ok_or("UPDATE assignment without a value")?;
        if matches!(value.node.as_ref(), Some(NodeEnum::MultiAssignRef(_))) {
            return Err("multi-column UPDATE assignment");
        }
        validate_dml_expression(value, supported)?;
    }

    if update.from_clause.len() > 1 {
        return Err("multiple UPDATE FROM relations");
    }
    if let Some(source) = update.from_clause.first() {
        match source.node.as_ref() {
            Some(NodeEnum::RangeVar(range)) if range.inh => {}
            Some(NodeEnum::RangeVar(_)) => return Err("UPDATE FROM ONLY relation"),
            _ => return Err("complex UPDATE FROM source"),
        }
    }

    if let Some(predicate) = update.where_clause.as_deref() {
        validate_dml_expression(predicate, supported)?;
    }
    for result in &update.returning_list {
        let result = match result.node.as_ref() {
            Some(NodeEnum::ResTarget(result)) => result,
            _ => return Err("unrecognized UPDATE RETURNING expression"),
        };
        let value = result
            .val
            .as_deref()
            .ok_or("UPDATE RETURNING expression without a value")?;
        validate_dml_expression(value, supported)?;
    }

    Ok(())
}

fn validate_delete(delete: &DeleteStmt, supported: &SupportedNodes) -> Result<(), &'static str> {
    let relation = delete
        .relation
        .as_ref()
        .ok_or("DELETE without a target relation")?;
    if !relation.inh {
        return Err("DELETE FROM ONLY target");
    }
    if delete.with_clause.is_some() {
        return Err("DELETE WITH clause");
    }
    if delete.using_clause.len() > 1 {
        return Err("multiple DELETE USING relations");
    }
    if let Some(source) = delete.using_clause.first() {
        match source.node.as_ref() {
            Some(NodeEnum::RangeVar(range)) if range.inh => {}
            Some(NodeEnum::RangeVar(_)) => return Err("DELETE USING ONLY relation"),
            _ => return Err("complex DELETE USING source"),
        }
    }
    if let Some(predicate) = delete.where_clause.as_deref() {
        validate_dml_expression(predicate, supported)?;
    }
    for result in &delete.returning_list {
        let result = match result.node.as_ref() {
            Some(NodeEnum::ResTarget(result)) => result,
            _ => return Err("unrecognized DELETE RETURNING expression"),
        };
        let value = result
            .val
            .as_deref()
            .ok_or("DELETE RETURNING expression without a value")?;
        validate_dml_expression(value, supported)?;
    }
    Ok(())
}

fn validate_dml_expression(
    expression: &Node,
    supported: &SupportedNodes,
) -> Result<(), &'static str> {
    let root = expression
        .node
        .as_ref()
        .ok_or("empty data-modifying expression")?;
    for (node, _, context, _) in root.nodes() {
        if matches!(node, NodeRef::SubLink(_)) {
            return Err("subquery in data-modifying expression");
        }
        validate_nested_node(node, context, supported)?;
    }
    Ok(())
}

fn validate_on_conflict(
    conflict: &pg_query::protobuf::OnConflictClause,
) -> Result<(), &'static str> {
    match OnConflictAction::try_from(conflict.action).unwrap_or(OnConflictAction::Undefined) {
        OnConflictAction::OnconflictNothing => {
            if !conflict.target_list.is_empty() || conflict.where_clause.is_some() {
                return Err("invalid ON CONFLICT DO NOTHING action");
            }
        }
        OnConflictAction::OnconflictUpdate => {
            if conflict.target_list.is_empty() {
                return Err("ON CONFLICT DO UPDATE without assignments");
            }
        }
        OnConflictAction::Undefined | OnConflictAction::OnconflictNone => {
            return Err("unknown ON CONFLICT action");
        }
    }

    if let Some(infer) = &conflict.infer {
        if infer.index_elems.is_empty() && infer.conname.is_empty() {
            return Err("empty ON CONFLICT target");
        }
    }
    Ok(())
}

fn validate_select(
    select: &SelectStmt,
    allow_recursive_union: bool,
    supported: &mut SupportedNodes,
) -> Result<(), &'static str> {
    validate_select_fields(select, false)?;

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
                    validate_select(select, with_clause.recursive, supported)?
                }
                _ => return Err("data-modifying common table expression"),
            }
        }
    }

    let operation = SetOperation::try_from(select.op).unwrap_or(SetOperation::Undefined);
    match operation {
        SetOperation::SetopNone => Ok(()),
        SetOperation::SetopUnion if allow_recursive_union && select.all => {
            supported
                .recursive_unions
                .insert(select as *const SelectStmt as usize);
            let left = select.larg.as_deref().ok_or("incomplete UNION ALL")?;
            let right = select.rarg.as_deref().ok_or("incomplete UNION ALL")?;
            validate_select(left, false, supported)?;
            validate_select(right, false, supported)
        }
        SetOperation::SetopUnion => Err("general UNION or UNION ALL expression"),
        SetOperation::SetopIntersect => Err("INTERSECT expression"),
        SetOperation::SetopExcept => Err("EXCEPT expression"),
        SetOperation::Undefined => Err("unknown set operation"),
    }
}

fn validate_select_fields(select: &SelectStmt, allow_values: bool) -> Result<(), &'static str> {
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
    if !allow_values && !select.values_lists.is_empty() {
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
    context: Context,
    supported: &SupportedNodes,
) -> Result<(), &'static str> {
    match node {
        NodeRef::SelectStmt(select) => {
            let pointer = select as *const SelectStmt as usize;
            validate_select_fields(select, supported.insert_values.contains(&pointer))?;
            let operation = SetOperation::try_from(select.op).unwrap_or(SetOperation::Undefined);
            if operation != SetOperation::SetopNone
                && !supported.recursive_unions.contains(&pointer)
            {
                return Err("general set-operation expression");
            }
            Ok(())
        }
        NodeRef::SubLink(_) if context == Context::DML => {
            Err("subquery in data-modifying statement")
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
