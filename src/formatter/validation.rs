use std::collections::HashSet;

use pg_query::protobuf::node::Node as NodeEnum;
use pg_query::protobuf::{
    CmdType, DeleteStmt, InsertStmt, MergeMatchKind, MergeStmt, Node, OnConflictAction,
    OverridingKind, RawStmt, SelectStmt, SetOperation, Token, UpdateStmt,
};
use pg_query::{Context, NodeRef};
use serde_json::Value;

use super::FormatDiagnostic;
use super::ownership::{StatementKind, SupportedDocument, source_statement};
use super::tokens;

/// PostgreSQL server grammar version embedded by the reviewed `pg_query`
/// backend. A dependency upgrade must update this constant deliberately after
/// the support classifier and fixtures have been reviewed against the new AST.
const REVIEWED_POSTGRESQL_VERSION: i32 = 170004;

#[derive(Debug, Default)]
struct SupportedNodes {
    set_operations: HashSet<usize>,
    insert_values: HashSet<usize>,
}

pub(super) fn parse_supported_postgresql(
    source: &str,
) -> Result<SupportedDocument, FormatDiagnostic> {
    let parsed = pg_query::parse(source)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;
    if parsed.protobuf.version != REVIEWED_POSTGRESQL_VERSION {
        return Err(FormatDiagnostic::UnsupportedSyntax {
            feature: format!(
                "unreviewed PostgreSQL parser version {}",
                parsed.protobuf.version
            ),
            start: 0,
            end: source.len(),
        });
    }

    let mut supported = SupportedNodes::default();
    let mut statements = Vec::with_capacity(parsed.protobuf.stmts.len());
    for raw in &parsed.protobuf.stmts {
        let kind = validate_statement(raw, &mut supported)
            .map_err(|feature| unsupported(source, raw, feature))?;
        statements.push(source_statement(source, raw, kind));
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

    Ok(SupportedDocument::new(statements))
}

fn validate_statement(
    raw: &RawStmt,
    supported: &mut SupportedNodes,
) -> Result<StatementKind, &'static str> {
    let node = raw
        .stmt
        .as_deref()
        .and_then(|statement| statement.node.as_ref())
        .ok_or("empty PostgreSQL statement")?;

    match node {
        NodeEnum::SelectStmt(select) => {
            validate_select(select, false, supported)?;
            Ok(StatementKind::Select)
        }
        NodeEnum::InsertStmt(insert) => {
            validate_insert(insert, supported)?;
            Ok(StatementKind::Insert)
        }
        NodeEnum::UpdateStmt(update) => {
            validate_update(update, supported)?;
            Ok(StatementKind::Update)
        }
        NodeEnum::DeleteStmt(delete) => {
            validate_delete(delete, supported)?;
            Ok(StatementKind::Delete)
        }
        NodeEnum::MergeStmt(merge) => {
            validate_merge(merge, supported)?;
            Ok(StatementKind::Merge)
        }
        NodeEnum::CreateStmt(_) => Err("CREATE TABLE statement"),
        NodeEnum::IndexStmt(_) => Err("CREATE INDEX statement"),
        NodeEnum::AlterTableStmt(_) => Err("ALTER TABLE statement"),
        NodeEnum::CreateFunctionStmt(_) => Err("function or procedure definition"),
        NodeEnum::DoStmt(_) => Err("DO block"),
        _ => Err("unimplemented PostgreSQL statement family"),
    }
}

fn validate_merge(merge: &MergeStmt, supported: &mut SupportedNodes) -> Result<(), &'static str> {
    let relation = merge
        .relation
        .as_ref()
        .ok_or("MERGE without a target relation")?;
    if !relation.inh {
        return Err("MERGE INTO ONLY target");
    }
    match merge
        .source_relation
        .as_deref()
        .and_then(|source| source.node.as_ref())
    {
        Some(NodeEnum::RangeVar(range)) if range.inh => {}
        Some(NodeEnum::RangeVar(_)) => return Err("MERGE USING ONLY relation"),
        _ => return Err("complex MERGE source"),
    }
    if let Some(with_clause) = &merge.with_clause {
        validate_with_clause(with_clause, supported)?;
    }
    let join_condition = merge
        .join_condition
        .as_deref()
        .ok_or("MERGE without a join condition")?;
    validate_dml_expression(join_condition, supported)?;
    if merge.merge_when_clauses.is_empty() {
        return Err("MERGE without WHEN branches");
    }

    for branch in &merge.merge_when_clauses {
        let branch = match branch.node.as_ref() {
            Some(NodeEnum::MergeWhenClause(branch)) => branch,
            _ => return Err("unrecognized MERGE branch"),
        };
        match MergeMatchKind::try_from(branch.match_kind).unwrap_or(MergeMatchKind::Undefined) {
            MergeMatchKind::Undefined => return Err("unknown MERGE match kind"),
            MergeMatchKind::MergeWhenMatched
            | MergeMatchKind::MergeWhenNotMatchedBySource
            | MergeMatchKind::MergeWhenNotMatchedByTarget => {}
        }
        if let Some(condition) = branch.condition.as_deref() {
            validate_dml_expression(condition, supported)?;
        }

        match CmdType::try_from(branch.command_type).unwrap_or(CmdType::Undefined) {
            CmdType::CmdDelete | CmdType::CmdNothing => {
                if !branch.target_list.is_empty() || !branch.values.is_empty() {
                    return Err("invalid MERGE branch action");
                }
            }
            CmdType::CmdUpdate => {
                if branch.target_list.is_empty() || !branch.values.is_empty() {
                    return Err("invalid MERGE UPDATE action");
                }
                for target in &branch.target_list {
                    let target = match target.node.as_ref() {
                        Some(NodeEnum::ResTarget(target)) => target,
                        _ => return Err("unrecognized MERGE UPDATE assignment"),
                    };
                    if target.name.is_empty() || !target.indirection.is_empty() {
                        return Err("complex MERGE UPDATE assignment target");
                    }
                    let value = target
                        .val
                        .as_deref()
                        .ok_or("MERGE UPDATE assignment without a value")?;
                    validate_dml_expression(value, supported)?;
                }
            }
            CmdType::CmdInsert => {
                match OverridingKind::try_from(branch.r#override)
                    .unwrap_or(OverridingKind::Undefined)
                {
                    OverridingKind::Undefined => {
                        return Err("unknown MERGE INSERT OVERRIDING clause");
                    }
                    OverridingKind::OverridingNotSet
                    | OverridingKind::OverridingUserValue
                    | OverridingKind::OverridingSystemValue => {}
                }
                for target in &branch.target_list {
                    let target = match target.node.as_ref() {
                        Some(NodeEnum::ResTarget(target)) => target,
                        _ => return Err("unrecognized MERGE INSERT target"),
                    };
                    if target.name.is_empty()
                        || !target.indirection.is_empty()
                        || target.val.is_some()
                    {
                        return Err("complex MERGE INSERT target");
                    }
                }
                if branch.values.is_empty() {
                    return Err("MERGE INSERT without VALUES");
                }
                if !branch.target_list.is_empty() && branch.target_list.len() != branch.values.len()
                {
                    return Err("MERGE INSERT target/value arity mismatch");
                }
                for value in &branch.values {
                    validate_dml_expression(value, supported)?;
                }
            }
            CmdType::Undefined
            | CmdType::CmdUnknown
            | CmdType::CmdSelect
            | CmdType::CmdMerge
            | CmdType::CmdUtility => return Err("unknown MERGE action"),
        }
    }

    for result in &merge.returning_list {
        let result = match result.node.as_ref() {
            Some(NodeEnum::ResTarget(result)) => result,
            _ => return Err("unrecognized MERGE RETURNING expression"),
        };
        let value = result
            .val
            .as_deref()
            .ok_or("MERGE RETURNING expression without a value")?;
        validate_dml_expression(value, supported)?;
    }

    Ok(())
}

fn validate_insert(
    insert: &InsertStmt,
    supported: &mut SupportedNodes,
) -> Result<(), &'static str> {
    if insert.relation.is_none() {
        return Err("INSERT without a target relation");
    }
    if let Some(with_clause) = &insert.with_clause {
        validate_with_clause(with_clause, supported)?;
    }
    if let Some(conflict) = &insert.on_conflict_clause {
        validate_on_conflict(conflict)?;
    }
    match OverridingKind::try_from(insert.r#override).unwrap_or(OverridingKind::Undefined) {
        OverridingKind::Undefined => return Err("unknown INSERT OVERRIDING clause"),
        OverridingKind::OverridingNotSet
        | OverridingKind::OverridingUserValue
        | OverridingKind::OverridingSystemValue => {}
    }

    let Some(select_node) = insert.select_stmt.as_deref() else {
        // PostgreSQL represents DEFAULT VALUES as an INSERT with no source node.
        return Ok(());
    };
    let select = select_node
        .node
        .as_ref()
        .and_then(|node| match node {
            NodeEnum::SelectStmt(select) => Some(select),
            _ => None,
        })
        .ok_or("unrecognized INSERT source")?;

    if !select.values_lists.is_empty() {
        validate_select_fields(select, true)?;
        if SetOperation::try_from(select.op).unwrap_or(SetOperation::Undefined)
            != SetOperation::SetopNone
            || select.larg.is_some()
            || select.rarg.is_some()
            || select.with_clause.is_some()
            || !select.target_list.is_empty()
            || !select.from_clause.is_empty()
            || select.where_clause.is_some()
        {
            return Err("invalid INSERT VALUES source");
        }
        supported
            .insert_values
            .insert(select.as_ref() as *const SelectStmt as usize);
        return Ok(());
    }

    validate_select(select, false, supported)
}

fn validate_update(
    update: &UpdateStmt,
    supported: &mut SupportedNodes,
) -> Result<(), &'static str> {
    let relation = update
        .relation
        .as_ref()
        .ok_or("UPDATE without a target relation")?;
    if !relation.inh {
        return Err("UPDATE ONLY target");
    }
    if let Some(with_clause) = &update.with_clause {
        validate_with_clause(with_clause, supported)?;
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

fn validate_delete(
    delete: &DeleteStmt,
    supported: &mut SupportedNodes,
) -> Result<(), &'static str> {
    let relation = delete
        .relation
        .as_ref()
        .ok_or("DELETE without a target relation")?;
    if !relation.inh {
        return Err("DELETE FROM ONLY target");
    }
    if let Some(with_clause) = &delete.with_clause {
        validate_with_clause(with_clause, supported)?;
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

fn validate_with_clause(
    with_clause: &pg_query::protobuf::WithClause,
    supported: &mut SupportedNodes,
) -> Result<(), &'static str> {
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
    Ok(())
}

fn validate_select(
    select: &SelectStmt,
    allow_recursive_union: bool,
    supported: &mut SupportedNodes,
) -> Result<(), &'static str> {
    validate_select_fields(select, false)?;

    if let Some(with_clause) = &select.with_clause {
        validate_with_clause(with_clause, supported)?;
    }

    let operation = SetOperation::try_from(select.op).unwrap_or(SetOperation::Undefined);
    match operation {
        SetOperation::SetopNone => Ok(()),
        SetOperation::SetopUnion | SetOperation::SetopIntersect | SetOperation::SetopExcept => {
            if allow_recursive_union && operation != SetOperation::SetopUnion {
                return Err("recursive CTE with non-UNION set operation");
            }
            supported
                .set_operations
                .insert(select as *const SelectStmt as usize);
            let left = select.larg.as_deref().ok_or("incomplete set operation")?;
            let right = select.rarg.as_deref().ok_or("incomplete set operation")?;
            validate_select(left, false, supported)?;
            validate_select(right, false, supported)
        }
        SetOperation::Undefined => Err("unknown set operation"),
    }
}

fn validate_select_fields(select: &SelectStmt, allow_values: bool) -> Result<(), &'static str> {
    if select.into_clause.is_some() {
        return Err("SELECT INTO clause");
    }
    if !select.window_clause.is_empty() {
        return Err("WINDOW clause");
    }
    if !allow_values && !select.values_lists.is_empty() {
        return Err("VALUES statement");
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
            if operation != SetOperation::SetopNone && !supported.set_operations.contains(&pointer)
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

#[cfg(test)]
mod parser_version_tests {
    use super::REVIEWED_POSTGRESQL_VERSION;

    #[test]
    fn pinned_pg_query_uses_the_reviewed_postgresql_grammar() {
        let parsed = pg_query::parse("SELECT 1;").expect("PostgreSQL parse succeeds");
        assert_eq!(parsed.protobuf.version, REVIEWED_POSTGRESQL_VERSION);
    }
}
