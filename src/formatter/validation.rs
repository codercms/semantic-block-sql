use super::FormatDiagnostic;
use pg_query::protobuf::node::Node as NodeEnum;
use pg_query::protobuf::{
    AlterTableStmt, AlterTableType, CmdType, ColumnDef, ConstrType, Constraint, CreateStmt,
    DeleteStmt, IndexStmt, InsertStmt, MergeMatchKind, MergeStmt, Node, ObjectType, OnCommitAction,
    OnConflictAction, OverridingKind, RawStmt, SelectStmt, SetOperation, UpdateStmt,
};
use pg_query::{Context, NodeRef};
mod equivalence;

pub use equivalence::validate_equivalent;

use super::ownership::{
    AlterTableActionGroup, AlterTableSpec, ConflictActionSpec, ConflictSpec, CreateIndexSpec,
    CreateTableElementSpec, CreateTableSpec, DeleteSpec, InsertSourceSpec, InsertSpec,
    MergeActionSpec, MergeBranchSpec, MergeSpec, OverrideSpec, SelectSpec, StatementSpec,
    SupportedDocument, UpdateSpec, ValuesSpec, source_statement,
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
        NodeEnum::SelectStmt(select) if is_values_select_shape(select) => {
            validate_values_select(select)?;
            Ok(StatementSpec::Values(ValuesSpec {
                rows: select.values_lists.len(),
            }))
        }
        NodeEnum::SelectStmt(select) => Ok(StatementSpec::Select(validate_select(select, false)?)),
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
        NodeEnum::CreateStmt(create) => {
            Ok(StatementSpec::CreateTable(validate_create_table(create)?))
        }
        NodeEnum::IndexStmt(index) => Ok(StatementSpec::CreateIndex(validate_create_index(index)?)),
        NodeEnum::AlterTableStmt(alter) => {
            Ok(StatementSpec::AlterTable(validate_alter_table(alter)?))
        }
        NodeEnum::CreateFunctionStmt(_) => Err("function or procedure definition"),
        NodeEnum::DoStmt(_) => Err("DO block"),
        _ => Err("unimplemented PostgreSQL statement family"),
    }
}

fn validate_values_select(select: &SelectStmt) -> Result<(), &'static str> {
    if !is_values_select_shape(select) || select.values_lists.is_empty() {
        return Err("invalid VALUES statement");
    }
    validate_select_fields(select, true)?;
    for row in &select.values_lists {
        let row = match row.node.as_ref() {
            Some(NodeEnum::List(row)) => row,
            _ => return Err("unrecognized VALUES row"),
        };
        if row.items.is_empty() {
            return Err("empty VALUES row");
        }
        for value in &row.items {
            validate_ddl_expression(value)?;
        }
    }
    Ok(())
}

fn validate_create_table(create: &CreateStmt) -> Result<CreateTableSpec, &'static str> {
    if create.relation.is_none() {
        return Err("CREATE TABLE without a relation");
    }
    if !create.inh_relations.is_empty() {
        return Err("CREATE TABLE INHERITS clause");
    }
    if create.partbound.is_some() || create.partspec.is_some() {
        return Err("partitioned CREATE TABLE");
    }
    if create.of_typename.is_some() {
        return Err("CREATE TABLE OF type");
    }
    if !create.constraints.is_empty() {
        return Err("transformed CREATE TABLE constraints");
    }
    if !create.options.is_empty() {
        return Err("CREATE TABLE storage parameters");
    }
    if OnCommitAction::try_from(create.oncommit).unwrap_or(OnCommitAction::Undefined)
        != OnCommitAction::OncommitNoop
    {
        return Err("CREATE TABLE ON COMMIT clause");
    }
    if !create.tablespacename.is_empty() || !create.access_method.is_empty() {
        return Err("CREATE TABLE access method or tablespace");
    }
    if create.table_elts.is_empty() {
        return Err("CREATE TABLE without columns or constraints");
    }

    let mut elements = Vec::with_capacity(create.table_elts.len());
    for element in &create.table_elts {
        match element.node.as_ref() {
            Some(NodeEnum::ColumnDef(column)) => {
                validate_column_def(column)?;
                elements.push(CreateTableElementSpec::Column);
            }
            Some(NodeEnum::Constraint(constraint)) => {
                validate_constraint(constraint)?;
                elements.push(CreateTableElementSpec::Constraint);
            }
            Some(NodeEnum::TableLikeClause(_)) => return Err("CREATE TABLE LIKE clause"),
            _ => return Err("unrecognized CREATE TABLE element"),
        }
    }

    Ok(CreateTableSpec {
        if_not_exists: create.if_not_exists,
        elements,
    })
}

fn validate_create_index(index: &IndexStmt) -> Result<CreateIndexSpec, &'static str> {
    if index.relation.is_none() {
        return Err("CREATE INDEX without a relation");
    }
    if index.index_params.is_empty() {
        return Err("CREATE INDEX without key expressions");
    }
    if index.primary
        || index.isconstraint
        || index.deferrable
        || index.initdeferred
        || index.transformed
        || !index.exclude_op_names.is_empty()
    {
        return Err("internal or constraint-backed CREATE INDEX");
    }

    for parameter in &index.index_params {
        validate_index_element(parameter, false)?;
    }
    for parameter in &index.index_including_params {
        validate_index_element(parameter, true)?;
    }
    for option in &index.options {
        match option.node.as_ref() {
            Some(NodeEnum::DefElem(option)) => {
                if let Some(value) = option.arg.as_deref() {
                    validate_ddl_expression(value)?;
                }
            }
            _ => return Err("unrecognized CREATE INDEX storage parameter"),
        }
    }
    if let Some(predicate) = index.where_clause.as_deref() {
        validate_ddl_expression(predicate)?;
    }

    Ok(CreateIndexSpec {
        unique: index.unique,
        concurrent: index.concurrent,
        if_not_exists: index.if_not_exists,
        key_items: index.index_params.len(),
        include_items: index.index_including_params.len(),
        options: index.options.len(),
        has_tablespace: !index.table_space.is_empty(),
        has_where: index.where_clause.is_some(),
    })
}

fn validate_index_element(element: &Node, include: bool) -> Result<(), &'static str> {
    let element = match element.node.as_ref() {
        Some(NodeEnum::IndexElem(element)) => element,
        _ => return Err("unrecognized CREATE INDEX element"),
    };
    if include {
        if element.name.is_empty()
            || element.expr.is_some()
            || !element.collation.is_empty()
            || !element.opclass.is_empty()
            || !element.opclassopts.is_empty()
        {
            return Err("complex CREATE INDEX INCLUDE element");
        }
    } else if element.name.is_empty() && element.expr.is_none() {
        return Err("empty CREATE INDEX key element");
    }
    if let Some(expression) = element.expr.as_deref() {
        validate_ddl_expression(expression)?;
    }
    for option in &element.opclassopts {
        validate_ddl_expression(option)?;
    }
    Ok(())
}

fn validate_alter_table(alter: &AlterTableStmt) -> Result<AlterTableSpec, &'static str> {
    if alter.relation.is_none() {
        return Err("ALTER TABLE without a relation");
    }
    if ObjectType::try_from(alter.objtype).unwrap_or(ObjectType::Undefined)
        != ObjectType::ObjectTable
    {
        return Err("non-table ALTER statement");
    }
    if alter.cmds.is_empty() {
        return Err("ALTER TABLE without actions");
    }

    let mut action_groups = Vec::with_capacity(alter.cmds.len());
    for command in &alter.cmds {
        let command = match command.node.as_ref() {
            Some(NodeEnum::AlterTableCmd(command)) => command,
            _ => return Err("unrecognized ALTER TABLE action"),
        };
        let subtype =
            AlterTableType::try_from(command.subtype).unwrap_or(AlterTableType::Undefined);
        let group = alter_action_group(subtype)?;
        if let Some(definition) = command.def.as_deref() {
            match definition.node.as_ref() {
                Some(NodeEnum::ColumnDef(column)) => validate_column_def(column)?,
                Some(NodeEnum::Constraint(constraint)) => validate_constraint(constraint)?,
                Some(_) => validate_ddl_expression(definition)?,
                None => return Err("empty ALTER TABLE action definition"),
            }
        }
        action_groups.push(group);
    }

    Ok(AlterTableSpec {
        if_exists: alter.missing_ok,
        action_groups,
    })
}

fn alter_action_group(subtype: AlterTableType) -> Result<AlterTableActionGroup, &'static str> {
    use AlterTableActionGroup::{Add, Alter, Drop, Other, Set};
    use AlterTableType::*;

    match subtype {
        AtAddColumn | AtAddConstraint | AtAddIndexConstraint | AtAddInherit | AtAddOf
        | AtAttachPartition | AtAddIdentity => Ok(Add),
        AtColumnDefault
        | AtDropNotNull
        | AtSetNotNull
        | AtSetExpression
        | AtDropExpression
        | AtCheckNotNull
        | AtAlterConstraint
        | AtValidateConstraint
        | AtAlterColumnType
        | AtAlterColumnGenericOptions
        | AtSetIdentity
        | AtGenericOptions => Ok(Alter),
        AtDropColumn
        | AtDropConstraint
        | AtDropCluster
        | AtDropInherit
        | AtDropOf
        | AtDropIdentity
        | AtDetachPartition
        | AtDetachPartitionFinalize => Ok(Drop),
        AtSetStatistics | AtSetOptions | AtResetOptions | AtSetStorage | AtSetCompression
        | AtChangeOwner | AtClusterOn | AtSetLogged | AtSetUnLogged | AtSetAccessMethod
        | AtSetTableSpace | AtSetRelOptions | AtResetRelOptions | AtReplaceRelOptions
        | AtEnableTrig | AtEnableAlwaysTrig | AtEnableReplicaTrig | AtDisableTrig
        | AtEnableTrigAll | AtDisableTrigAll | AtEnableTrigUser | AtDisableTrigUser
        | AtEnableRule | AtEnableAlwaysRule | AtEnableReplicaRule | AtDisableRule
        | AtReplicaIdentity | AtEnableRowSecurity | AtDisableRowSecurity | AtForceRowSecurity
        | AtNoForceRowSecurity => Ok(Set),
        AtDropOids => Ok(Other),
        Undefined
        | AtAddColumnToView
        | AtCookedColumnDefault
        | AtAddIndex
        | AtReAddIndex
        | AtReAddConstraint
        | AtReAddDomainConstraint
        | AtReAddComment
        | AtReAddStatistics => Err("internal or unreviewed ALTER TABLE action"),
    }
}

fn validate_column_def(column: &ColumnDef) -> Result<(), &'static str> {
    if column.colname.is_empty() || column.type_name.is_none() {
        return Err("CREATE/ALTER column without a name or type");
    }
    if !column.fdwoptions.is_empty() {
        return Err("foreign-table column options");
    }
    if let Some(default) = column.raw_default.as_deref() {
        validate_ddl_expression(default)?;
    }
    if column.cooked_default.is_some() {
        return Err("transformed column default");
    }
    for constraint in &column.constraints {
        let constraint = match constraint.node.as_ref() {
            Some(NodeEnum::Constraint(constraint)) => constraint,
            _ => return Err("unrecognized column constraint"),
        };
        validate_constraint(constraint)?;
    }
    Ok(())
}

fn validate_constraint(constraint: &Constraint) -> Result<(), &'static str> {
    if ConstrType::try_from(constraint.contype).unwrap_or(ConstrType::Undefined)
        == ConstrType::Undefined
    {
        return Err("unknown table constraint");
    }
    if let Some(expression) = constraint.raw_expr.as_deref() {
        validate_ddl_expression(expression)?;
    }
    if let Some(predicate) = constraint.where_clause.as_deref() {
        validate_ddl_expression(predicate)?;
    }
    for exclusion in &constraint.exclusions {
        validate_ddl_expression(exclusion)?;
    }
    Ok(())
}

fn validate_ddl_expression(expression: &Node) -> Result<(), &'static str> {
    let root = expression.node.as_ref().ok_or("empty DDL expression")?;
    for (node, _, context, _) in root.nodes() {
        if matches!(node, NodeRef::SubLink(_)) {
            return Err("subquery in DDL expression");
        }
        validate_nested_node(node, context)?;
    }
    Ok(())
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
        named_windows: select.window_clause.len(),
    })
}

fn validate_select_fields(select: &SelectStmt, allow_values: bool) -> Result<(), &'static str> {
    if select.into_clause.is_some() {
        return Err("SELECT INTO clause");
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
        NodeRef::JsonTable(_) => Err("JSON_TABLE expression"),
        _ => Ok(()),
    }
}

fn validate_nested_select(select: &SelectStmt) -> Result<(), &'static str> {
    let values_shape = is_values_select_shape(select);
    if values_shape {
        return validate_values_select(select);
    }
    validate_select_fields(select, false)?;
    if !select.values_lists.is_empty() {
        return Err("invalid VALUES expression");
    }
    if let Some(with_clause) = &select.with_clause {
        validate_with_clause(with_clause)?;
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
