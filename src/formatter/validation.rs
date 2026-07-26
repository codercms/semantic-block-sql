use super::FormatDiagnostic;
use pg_query::protobuf::node::Node as NodeEnum;
use pg_query::protobuf::{
    CmdType, DeleteStmt, InsertStmt, MergeMatchKind, MergeStmt, Node, OnConflictAction,
    OverridingKind, RawStmt, SelectStmt, SetOperation, UpdateStmt,
};
use pg_query::{Context, NodeRef};
mod equivalence;

pub use equivalence::validate_equivalent;

use super::ownership::{
    ConflictActionSpec, ConflictSpec, DeleteSpec, InsertSourceSpec, InsertSpec, MergeActionSpec,
    MergeBranchSpec, MergeSpec, OverrideSpec, SelectSpec, StatementSpec, SupportedDocument,
    UpdateSpec, source_statement,
};

/// PostgreSQL server grammar version embedded by the reviewed `pg_query`
/// backend. A dependency upgrade must update this constant deliberately after
/// the support classifier and fixtures have been reviewed against the new AST.
const REVIEWED_POSTGRESQL_VERSION: i32 = 170004;

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

    let mut statements = Vec::with_capacity(parsed.protobuf.stmts.len());
    for raw in &parsed.protobuf.stmts {
        let spec = validate_statement(raw).map_err(|feature| unsupported(source, raw, feature))?;
        statements.push(source_statement(source, raw, spec));
    }

    for (node, _, context, _) in parsed.protobuf.nodes() {
        validate_nested_node(node, context).map_err(|feature| {
            FormatDiagnostic::UnsupportedSyntax {
                feature: feature.into(),
                start: 0,
                end: source.len(),
            }
        })?;
    }

    Ok(SupportedDocument::new(statements))
}

fn validate_statement(raw: &RawStmt) -> Result<StatementSpec, &'static str> {
    let node = raw
        .stmt
        .as_deref()
        .and_then(|statement| statement.node.as_ref())
        .ok_or("empty PostgreSQL statement")?;

    match node {
        NodeEnum::SelectStmt(select) => {
            let spec = validate_select(select, false)?;
            Ok(StatementSpec::Select(spec))
        }
        NodeEnum::InsertStmt(insert) => {
            let spec = validate_insert(insert)?;
            Ok(StatementSpec::Insert(spec))
        }
        NodeEnum::UpdateStmt(update) => {
            let spec = validate_update(update)?;
            Ok(StatementSpec::Update(spec))
        }
        NodeEnum::DeleteStmt(delete) => {
            let spec = validate_delete(delete)?;
            Ok(StatementSpec::Delete(spec))
        }
        NodeEnum::MergeStmt(merge) => {
            let spec = validate_merge(merge)?;
            Ok(StatementSpec::Merge(spec))
        }
        NodeEnum::CreateStmt(_) => Err("CREATE TABLE statement"),
        NodeEnum::IndexStmt(_) => Err("CREATE INDEX statement"),
        NodeEnum::AlterTableStmt(_) => Err("ALTER TABLE statement"),
        NodeEnum::CreateFunctionStmt(_) => Err("function or procedure definition"),
        NodeEnum::DoStmt(_) => Err("DO block"),
        _ => Err("unimplemented PostgreSQL statement family"),
    }
}

fn validate_merge(merge: &MergeStmt) -> Result<MergeSpec, &'static str> {
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
        validate_with_clause(with_clause)?;
    }
    let join_condition = merge
        .join_condition
        .as_deref()
        .ok_or("MERGE without a join condition")?;
    validate_dml_expression(join_condition)?;
    if merge.merge_when_clauses.is_empty() {
        return Err("MERGE without WHEN branches");
    }

    let mut branches = Vec::with_capacity(merge.merge_when_clauses.len());
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
            validate_dml_expression(condition)?;
        }

        let action = match CmdType::try_from(branch.command_type).unwrap_or(CmdType::Undefined) {
            CmdType::CmdDelete => {
                if !branch.target_list.is_empty() || !branch.values.is_empty() {
                    return Err("invalid MERGE branch action");
                }
                MergeActionSpec::Delete
            }
            CmdType::CmdNothing => {
                if !branch.target_list.is_empty() || !branch.values.is_empty() {
                    return Err("invalid MERGE branch action");
                }
                MergeActionSpec::Nothing
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
                    validate_dml_expression(value)?;
                }
                MergeActionSpec::Update {
                    assignments: branch.target_list.len(),
                }
            }
            CmdType::CmdInsert => {
                let overriding =
                    override_spec(branch.r#override, "unknown MERGE INSERT OVERRIDING clause")?;
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
                    validate_dml_expression(value)?;
                }
                MergeActionSpec::Insert {
                    target_columns: branch.target_list.len(),
                    values: branch.values.len(),
                    overriding,
                }
            }
            CmdType::Undefined
            | CmdType::CmdUnknown
            | CmdType::CmdSelect
            | CmdType::CmdMerge
            | CmdType::CmdUtility => return Err("unknown MERGE action"),
        };
        branches.push(MergeBranchSpec {
            has_condition: branch.condition.is_some(),
            action,
        });
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
        validate_dml_expression(value)?;
    }

    Ok(MergeSpec {
        has_with: merge.with_clause.is_some(),
        branches,
        returning_items: merge.returning_list.len(),
    })
}

fn validate_insert(insert: &InsertStmt) -> Result<InsertSpec, &'static str> {
    if insert.relation.is_none() {
        return Err("INSERT without a target relation");
    }
    if let Some(with_clause) = &insert.with_clause {
        validate_with_clause(with_clause)?;
    }
    let conflict = insert
        .on_conflict_clause
        .as_ref()
        .map(|conflict| validate_on_conflict(conflict.as_ref()))
        .transpose()?;
    let overriding = override_spec(insert.r#override, "unknown INSERT OVERRIDING clause")?;

    let source = match insert.select_stmt.as_deref() {
        None => InsertSourceSpec::DefaultValues,
        Some(select_node) => {
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
                InsertSourceSpec::Values {
                    rows: select.values_lists.len(),
                }
            } else {
                let query = validate_select(select, false)?;
                InsertSourceSpec::Query {
                    set_operations: query.set_operations,
                }
            }
        }
    };

    Ok(InsertSpec {
        has_with: insert.with_clause.is_some(),
        target_columns: insert.cols.len(),
        overriding,
        source,
        conflict,
        returning_items: insert.returning_list.len(),
    })
}

fn validate_update(update: &UpdateStmt) -> Result<UpdateSpec, &'static str> {
    let relation = update
        .relation
        .as_ref()
        .ok_or("UPDATE without a target relation")?;
    if !relation.inh {
        return Err("UPDATE ONLY target");
    }
    if let Some(with_clause) = &update.with_clause {
        validate_with_clause(with_clause)?;
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
        validate_dml_expression(value)?;
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
        validate_dml_expression(predicate)?;
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
        validate_dml_expression(value)?;
    }

    Ok(UpdateSpec {
        has_with: update.with_clause.is_some(),
        assignments: update.target_list.len(),
        from_relations: update.from_clause.len(),
        has_where: update.where_clause.is_some(),
        returning_items: update.returning_list.len(),
    })
}

fn validate_delete(delete: &DeleteStmt) -> Result<DeleteSpec, &'static str> {
    let relation = delete
        .relation
        .as_ref()
        .ok_or("DELETE without a target relation")?;
    if !relation.inh {
        return Err("DELETE FROM ONLY target");
    }
    if let Some(with_clause) = &delete.with_clause {
        validate_with_clause(with_clause)?;
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
        validate_dml_expression(predicate)?;
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
        validate_dml_expression(value)?;
    }
    Ok(DeleteSpec {
        has_with: delete.with_clause.is_some(),
        using_relations: delete.using_clause.len(),
        has_where: delete.where_clause.is_some(),
        returning_items: delete.returning_list.len(),
    })
}

fn validate_dml_expression(expression: &Node) -> Result<(), &'static str> {
    let root = expression
        .node
        .as_ref()
        .ok_or("empty data-modifying expression")?;
    for (node, _, context, _) in root.nodes() {
        if matches!(node, NodeRef::SubLink(_)) {
            return Err("subquery in data-modifying expression");
        }
        validate_nested_node(node, context)?;
    }
    Ok(())
}

fn validate_on_conflict(
    conflict: &pg_query::protobuf::OnConflictClause,
) -> Result<ConflictSpec, &'static str> {
    let action =
        match OnConflictAction::try_from(conflict.action).unwrap_or(OnConflictAction::Undefined) {
            OnConflictAction::OnconflictNothing => {
                if !conflict.target_list.is_empty() || conflict.where_clause.is_some() {
                    return Err("invalid ON CONFLICT DO NOTHING action");
                }
                ConflictActionSpec::Nothing
            }
            OnConflictAction::OnconflictUpdate => {
                if conflict.target_list.is_empty() {
                    return Err("ON CONFLICT DO UPDATE without assignments");
                }
                ConflictActionSpec::Update {
                    assignments: conflict.target_list.len(),
                    has_where: conflict.where_clause.is_some(),
                }
            }
            OnConflictAction::Undefined | OnConflictAction::OnconflictNone => {
                return Err("unknown ON CONFLICT action");
            }
        };

    if let Some(infer) = &conflict.infer {
        if infer.index_elems.is_empty() && infer.conname.is_empty() {
            return Err("empty ON CONFLICT target");
        }
    }

    Ok(ConflictSpec {
        has_target: conflict.infer.is_some(),
        has_target_where: conflict
            .infer
            .as_ref()
            .is_some_and(|infer| infer.where_clause.is_some()),
        action,
    })
}

fn override_spec(value: i32, unsupported: &'static str) -> Result<OverrideSpec, &'static str> {
    match OverridingKind::try_from(value).unwrap_or(OverridingKind::Undefined) {
        OverridingKind::Undefined => Err(unsupported),
        OverridingKind::OverridingNotSet => Ok(OverrideSpec::None),
        OverridingKind::OverridingUserValue => Ok(OverrideSpec::User),
        OverridingKind::OverridingSystemValue => Ok(OverrideSpec::System),
    }
}

fn validate_with_clause(with_clause: &pg_query::protobuf::WithClause) -> Result<(), &'static str> {
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
                let _ = validate_select(select, with_clause.recursive)?;
            }
            _ => return Err("data-modifying common table expression"),
        }
    }
    Ok(())
}

fn validate_select(
    select: &SelectStmt,
    allow_recursive_union: bool,
) -> Result<SelectSpec, &'static str> {
    validate_select_fields(select, false)?;

    if let Some(with_clause) = &select.with_clause {
        validate_with_clause(with_clause)?;
    }

    let operation = SetOperation::try_from(select.op).unwrap_or(SetOperation::Undefined);
    let set_operations = match operation {
        SetOperation::SetopNone => 0,
        SetOperation::SetopUnion | SetOperation::SetopIntersect | SetOperation::SetopExcept => {
            if allow_recursive_union && operation != SetOperation::SetopUnion {
                return Err("recursive CTE with non-UNION set operation");
            }
            let left = select.larg.as_deref().ok_or("incomplete set operation")?;
            let right = select.rarg.as_deref().ok_or("incomplete set operation")?;
            let left = validate_select(left, false)?;
            let right = validate_select(right, false)?;
            1 + left.set_operations + right.set_operations
        }
        SetOperation::Undefined => return Err("unknown set operation"),
    };

    Ok(SelectSpec {
        has_with: select.with_clause.is_some(),
        set_operations,
    })
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

fn validate_nested_node(node: NodeRef<'_>, context: Context) -> Result<(), &'static str> {
    match node {
        NodeRef::SelectStmt(select) => validate_nested_select(select),
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

fn validate_nested_select(select: &SelectStmt) -> Result<(), &'static str> {
    let values_shape = is_values_select_shape(select);
    validate_select_fields(select, values_shape)?;
    if !select.values_lists.is_empty() && !values_shape {
        return Err("invalid VALUES expression");
    }

    match SetOperation::try_from(select.op).unwrap_or(SetOperation::Undefined) {
        SetOperation::SetopNone => Ok(()),
        SetOperation::SetopUnion | SetOperation::SetopIntersect | SetOperation::SetopExcept => {
            if select.larg.is_none() || select.rarg.is_none() {
                return Err("incomplete set operation");
            }
            Ok(())
        }
        SetOperation::Undefined => Err("unknown set operation"),
    }
}

fn is_values_select_shape(select: &SelectStmt) -> bool {
    !select.values_lists.is_empty()
        && SetOperation::try_from(select.op).unwrap_or(SetOperation::Undefined)
            == SetOperation::SetopNone
        && select.larg.is_none()
        && select.rarg.is_none()
        && select.with_clause.is_none()
        && select.target_list.is_empty()
        && select.from_clause.is_empty()
        && select.where_clause.is_none()
        && select.group_clause.is_empty()
        && select.having_clause.is_none()
        && select.sort_clause.is_empty()
        && select.limit_offset.is_none()
        && select.limit_count.is_none()
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

#[cfg(test)]
mod parser_version_tests {
    use super::REVIEWED_POSTGRESQL_VERSION;

    #[test]
    fn pinned_pg_query_uses_the_reviewed_postgresql_grammar() {
        let parsed = pg_query::parse("SELECT 1;").expect("PostgreSQL parse succeeds");
        assert_eq!(parsed.protobuf.version, REVIEWED_POSTGRESQL_VERSION);
    }
}
