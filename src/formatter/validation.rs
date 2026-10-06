use super::FormatDiagnostic;
use pg_query::protobuf::a_const::Val as AConstValue;
use pg_query::protobuf::node::Node as NodeEnum;
use pg_query::protobuf::{
    AlterTableStmt, AlterTableType, CmdType, ColumnDef, ConstrType, Constraint, CreateDomainStmt,
    CreatePolicyStmt, CreateSeqStmt, CreateStmt, CreateTableAsStmt, CreateTrigStmt, DefElemAction,
    DeleteStmt, DropBehavior, DropStmt, GrantRoleStmt, GrantStmt, GrantTargetType, IndexStmt,
    InsertStmt, JoinExpr, JoinType, LockClauseStrength, LockWaitPolicy, MergeMatchKind, MergeStmt,
    Node, ObjectType, OnCommitAction, OnConflictAction, OverridingKind, PartitionStrategy, RawStmt,
    SelectStmt, SetOperation, TransactionStmt, TransactionStmtKind, TruncateStmt, UpdateStmt,
    ViewCheckOption, ViewStmt,
};
use pg_query::{Context, NodeRef};
pub(super) mod equivalence;

pub use equivalence::validate_equivalent;

use super::ownership::{
    AggregateSignatureSpec, AliasSpec, AlterTableActionGroup, AlterTableActionSpec, AlterTableSpec,
    ArrayListSpec, ConflictActionSpec, ConflictSpec, CreateIndexSpec, CreateTableElementSpec,
    CreateTableSpec, CteStatementSpec, DeleteSpec, ForeignKeyAction, ForeignKeySpec,
    FunctionCallSpec, IdentitySpec, InsertSourceSpec, InsertSpec, MaterializedViewSpec,
    MergeActionSpec, MergeBranchSpec, MergeSpec, OverrideSpec, QuerySpec, RelationIdentifierSpec,
    RelationItemSpec, RelationJoinConstraintSpec, RelationJoinSpec, RelationJoinTypeSpec,
    RelationListSpec, SelectSpec, SequenceOptionKind, SequenceSpec, StatementSpec,
    SupportedDocument, TriggerSpec, TriggerTiming, UpdateSpec, UtilityStatementKind,
    ValuesRelationSpec, ValuesSpec, ViewCheckSpec, ViewSpec, source_statement,
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
        let ctes = validated_cte_specs(raw).map_err(|feature| unsupported(source, raw, feature))?;
        statements.push(source_statement(source, raw, spec, ctes));
    }

    for raw in &parsed.protobuf.stmts {
        let root = raw
            .stmt
            .as_deref()
            .and_then(|statement| statement.node.as_ref())
            .ok_or_else(|| FormatDiagnostic::UnsupportedSyntax {
                feature: "empty PostgreSQL statement".into(),
                start: 0,
                end: source.len(),
            })?;
        validate_complete_tree(root).map_err(|feature| FormatDiagnostic::UnsupportedSyntax {
            feature: feature.into(),
            start: 0,
            end: source.len(),
        })?;
    }

    let mut queries = Vec::new();
    let mut values_relations = Vec::new();
    let mut function_calls = Vec::new();
    let source_tokens = super::tokens::tokenize(source)?;
    let mut arrays = Vec::new();
    for (statement_index, raw) in parsed.protobuf.stmts.iter().enumerate() {
        let root = raw
            .stmt
            .as_deref()
            .and_then(|statement| statement.node.as_ref())
            .ok_or_else(|| FormatDiagnostic::UnsupportedSyntax {
                feature: "empty PostgreSQL statement".into(),
                start: 0,
                end: source.len(),
            })?;
        collect_query_specs(
            root,
            statement_index,
            &mut queries,
            &mut values_relations,
            &source_tokens,
            &mut function_calls,
            &mut arrays,
        )
        .map_err(|feature| FormatDiagnostic::UnsupportedSyntax {
            feature: feature.into(),
            start: 0,
            end: source.len(),
        })?;
    }
    queries.sort_by_key(|query| (query.statement_index, query.anchor.unwrap_or(usize::MAX)));

    Ok(SupportedDocument::with_queries(
        statements,
        queries,
        values_relations,
        function_calls,
        arrays,
    ))
}

/// Walk every reviewed PostgreSQL child field that semblock depends on.
///
/// `pg_query::NodeEnum::nodes()` deliberately omits several expression-bearing
/// protobuf fields. Query ownership and unsupported-syntax validation must use
/// the same completed traversal contract; otherwise a nested SELECT can be
/// discovered in a field whose unsupported expression nodes were never checked.
fn walk_complete_tree<F>(root: &NodeEnum, visitor: &mut F) -> Result<(), &'static str>
where
    F: for<'a> FnMut(NodeRef<'a>, Context) -> Result<(), &'static str>,
{
    for (node, _, context, _) in root.nodes() {
        visitor(node, context)?;
        walk_omitted_children(node, visitor)?;
    }
    Ok(())
}

fn walk_omitted_children<F>(node: NodeRef<'_>, visitor: &mut F) -> Result<(), &'static str>
where
    F: for<'a> FnMut(NodeRef<'a>, Context) -> Result<(), &'static str>,
{
    match node {
        NodeRef::CreateStmt(statement) => walk_nodes(&statement.table_elts, visitor)?,
        NodeRef::AlterTableStmt(statement) => walk_nodes(&statement.cmds, visitor)?,
        NodeRef::AlterTableCmd(action) => walk_optional_node(action.def.as_deref(), visitor)?,
        NodeRef::ColumnDef(column) => {
            walk_optional_node(column.raw_default.as_deref(), visitor)?;
            walk_nodes(&column.constraints, visitor)?;
        }
        NodeRef::CreateDomainStmt(statement) => walk_nodes(&statement.constraints, visitor)?,
        NodeRef::CreateTrigStmt(statement) => {
            walk_optional_node(statement.when_clause.as_deref(), visitor)?
        }
        NodeRef::CreatePolicyStmt(statement) => {
            walk_optional_node(statement.qual.as_deref(), visitor)?;
            walk_optional_node(statement.with_check.as_deref(), visitor)?;
        }
        NodeRef::AlterPolicyStmt(statement) => {
            walk_optional_node(statement.qual.as_deref(), visitor)?;
            walk_optional_node(statement.with_check.as_deref(), visitor)?;
        }
        NodeRef::AArrayExpr(array) => walk_nodes(&array.elements, visitor)?,
        NodeRef::Constraint(constraint) => {
            walk_optional_node(constraint.raw_expr.as_deref(), visitor)?;
            walk_optional_node(constraint.where_clause.as_deref(), visitor)?;
            walk_nodes(&constraint.exclusions, visitor)?;
        }
        NodeRef::SelectStmt(select) => {
            walk_nodes(&select.distinct_clause, visitor)?;
            walk_nodes(&select.window_clause, visitor)?;
            walk_nodes(&select.values_lists, visitor)?;
            walk_optional_node(select.limit_offset.as_deref(), visitor)?;
            walk_optional_node(select.limit_count.as_deref(), visitor)?;
        }
        NodeRef::InsertStmt(insert) => walk_nodes(&insert.returning_list, visitor)?,
        NodeRef::UpdateStmt(update) => walk_nodes(&update.returning_list, visitor)?,
        NodeRef::DeleteStmt(delete) => walk_nodes(&delete.returning_list, visitor)?,
        NodeRef::MergeStmt(merge) => walk_nodes(&merge.returning_list, visitor)?,
        NodeRef::MergeWhenClause(branch) => {
            walk_optional_node(branch.condition.as_deref(), visitor)?;
            walk_nodes(&branch.target_list, visitor)?;
            walk_nodes(&branch.values, visitor)?;
        }
        NodeRef::OnConflictClause(conflict) => {
            if let Some(infer) = conflict.infer.as_deref() {
                walk_nodes(&infer.index_elems, visitor)?;
                walk_optional_node(infer.where_clause.as_deref(), visitor)?;
            }
            walk_nodes(&conflict.target_list, visitor)?;
            walk_optional_node(conflict.where_clause.as_deref(), visitor)?;
        }
        NodeRef::WindowDef(window) => {
            walk_nodes(&window.partition_clause, visitor)?;
            walk_nodes(&window.order_clause, visitor)?;
            walk_optional_node(window.start_offset.as_deref(), visitor)?;
            walk_optional_node(window.end_offset.as_deref(), visitor)?;
        }
        NodeRef::RuleStmt(rule) => {
            walk_optional_node(rule.where_clause.as_deref(), visitor)?;
            walk_nodes(&rule.actions, visitor)?;
        }
        _ => {}
    }
    Ok(())
}

fn walk_nodes<F>(nodes: &[Node], visitor: &mut F) -> Result<(), &'static str>
where
    F: for<'a> FnMut(NodeRef<'a>, Context) -> Result<(), &'static str>,
{
    for node in nodes {
        walk_optional_node(Some(node), visitor)?;
    }
    Ok(())
}

fn walk_optional_node<F>(node: Option<&Node>, visitor: &mut F) -> Result<(), &'static str>
where
    F: for<'a> FnMut(NodeRef<'a>, Context) -> Result<(), &'static str>,
{
    let Some(root) = node.and_then(|node| node.node.as_ref()) else {
        return Ok(());
    };
    walk_complete_tree(root, visitor)
}

fn validate_complete_tree(root: &NodeEnum) -> Result<(), &'static str> {
    walk_complete_tree(root, &mut |node, context| {
        validate_nested_node(node, context)
    })
}

/// Retain parser-validated ownership for every lexical SELECT that the ordinary
/// formatter may lay out, using the same completed traversal as capability
/// validation.
fn collect_query_specs(
    root: &NodeEnum,
    statement_index: usize,
    queries: &mut Vec<QuerySpec>,
    values_relations: &mut Vec<ValuesRelationSpec>,
    source_tokens: &[super::tokens::SqlToken<'_>],
    function_calls: &mut Vec<FunctionCallSpec>,
    arrays: &mut Vec<ArrayListSpec>,
) -> Result<(), &'static str> {
    walk_complete_tree(root, &mut |node, _| {
        if let NodeRef::AArrayExpr(array) = node {
            arrays.push(ArrayListSpec {
                location: usize::try_from(array.location)
                    .map_err(|_| "unlocated array constructor")?,
                elements: array.elements.len(),
            });
        }
        if let NodeRef::FuncCall(call) = node {
            match pg_query::protobuf::CoercionForm::try_from(call.funcformat) {
                Ok(pg_query::protobuf::CoercionForm::CoerceExplicitCall) if call.location >= 0 => {
                    let location = usize::try_from(call.location)
                        .map_err(|_| "unlocated explicit function call")?;
                    let name = string_nodes(&call.funcname, "function call name")?;
                    let keyword = source_tokens
                        .iter()
                        .find(|token| token.start == location)
                        .map(|token| token.kind);
                    let operator_escape = match (name.as_slice(), keyword) {
                        (
                            [schema, function],
                            Some(
                                keyword @ (pg_query::protobuf::Token::Like
                                | pg_query::protobuf::Token::Ilike),
                            ),
                        ) if schema == "pg_catalog" && function == "like_escape" => Some(keyword),
                        ([schema, function], Some(pg_query::protobuf::Token::Similar))
                            if schema == "pg_catalog" && function == "similar_to_escape" =>
                        {
                            Some(pg_query::protobuf::Token::Similar)
                        }
                        _ => None,
                    };
                    function_calls.push(match operator_escape {
                        Some(keyword) => FunctionCallSpec::OperatorEscape { location, keyword },
                        None => FunctionCallSpec::Named { location, name },
                    });
                }
                // PostgreSQL synthesizes unlocated helper calls for LIKE /
                // SIMILAR escapes. They have no authored call-name token.
                Ok(
                    pg_query::protobuf::CoercionForm::CoerceExplicitCall
                    | pg_query::protobuf::CoercionForm::CoerceSqlSyntax,
                ) => {}
                _ => return Err("unreviewed function call coercion form"),
            }
        }
        if let NodeRef::SelectStmt(select) = node {
            push_query_spec(select, statement_index, queries)?;
        }
        if let NodeRef::RangeSubselect(source) = node
            && let Some(NodeEnum::SelectStmt(query)) = source
                .subquery
                .as_deref()
                .and_then(|node| node.node.as_ref())
            && is_values_select_shape(query)
        {
            validate_values_select(query)?;
            values_relations.push(ValuesRelationSpec {
                statement_index,
                values: values_spec(query),
            });
        }
        Ok(())
    })
}

fn push_query_spec(
    select: &SelectStmt,
    statement_index: usize,
    queries: &mut Vec<QuerySpec>,
) -> Result<(), &'static str> {
    if is_values_select_shape(select) {
        return Ok(());
    }
    queries.push(QuerySpec {
        statement_index,
        anchor: select_query_anchor(select),
        select: validate_select(select, false)?,
    });
    Ok(())
}

fn select_query_anchor(select: &SelectStmt) -> Option<usize> {
    select_target_anchor(select)
        .or_else(|| select.larg.as_deref().and_then(select_query_anchor))
        .or_else(|| select.rarg.as_deref().and_then(select_query_anchor))
}

fn select_target_anchor(select: &SelectStmt) -> Option<usize> {
    select.target_list.iter().find_map(|target| {
        let NodeEnum::ResTarget(target) = target.node.as_ref()? else {
            return None;
        };
        usize::try_from(target.location).ok()
    })
}

fn validated_cte_specs(raw: &RawStmt) -> Result<Vec<CteStatementSpec>, &'static str> {
    let node = raw
        .stmt
        .as_deref()
        .and_then(|statement| statement.node.as_ref())
        .ok_or("empty PostgreSQL statement")?;
    validated_nested_cte_specs(node)
}

fn validated_nested_cte_specs(node: &NodeEnum) -> Result<Vec<CteStatementSpec>, &'static str> {
    let with_clause = match node {
        NodeEnum::SelectStmt(statement) => statement.with_clause.as_ref(),
        NodeEnum::InsertStmt(statement) => statement.with_clause.as_ref(),
        NodeEnum::UpdateStmt(statement) => statement.with_clause.as_ref(),
        NodeEnum::DeleteStmt(statement) => statement.with_clause.as_ref(),
        NodeEnum::MergeStmt(statement) => statement.with_clause.as_ref(),
        _ => None,
    };
    let Some(with_clause) = with_clause else {
        return Ok(Vec::new());
    };

    let mut result = Vec::with_capacity(with_clause.ctes.len());
    for cte_node in &with_clause.ctes {
        let cte = match cte_node.node.as_ref() {
            Some(NodeEnum::CommonTableExpr(cte)) => cte,
            _ => return Err("unrecognized common table expression"),
        };
        let query = cte
            .ctequery
            .as_deref()
            .and_then(|query| query.node.as_ref())
            .ok_or("empty common table expression")?;
        let spec = match query {
            NodeEnum::SelectStmt(select) if is_values_select_shape(select) => {
                validate_values_select(select)?;
                StatementSpec::Values(values_spec(select))
            }
            NodeEnum::SelectStmt(select) => {
                StatementSpec::Select(validate_select(select, with_clause.recursive)?)
            }
            NodeEnum::InsertStmt(insert) => StatementSpec::Insert(validate_insert(insert)?),
            NodeEnum::UpdateStmt(update) => StatementSpec::Update(validate_update(update)?),
            NodeEnum::DeleteStmt(delete) => StatementSpec::Delete(validate_delete(delete)?),
            NodeEnum::MergeStmt(merge) => StatementSpec::Merge(validate_merge(merge)?),
            _ => return Err("unreviewed common table expression body"),
        };
        result.push(CteStatementSpec {
            name: cte.ctename.clone(),
            columns: string_nodes(&cte.aliascolnames, "CTE column alias")?,
            spec,
            ctes: validated_nested_cte_specs(query)?,
        });
    }
    Ok(result)
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
            Ok(StatementSpec::Values(values_spec(select)))
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
        NodeEnum::ViewStmt(view) => Ok(StatementSpec::View(validate_view(view)?)),
        NodeEnum::CreateTableAsStmt(create)
            if ObjectType::try_from(create.objtype).unwrap_or(ObjectType::Undefined)
                == ObjectType::ObjectMatview =>
        {
            Ok(StatementSpec::MaterializedView(validate_materialized_view(
                create,
            )?))
        }
        NodeEnum::CreateStmt(create) => {
            Ok(StatementSpec::CreateTable(validate_create_table(create)?))
        }
        NodeEnum::IndexStmt(index) => Ok(StatementSpec::CreateIndex(validate_create_index(index)?)),
        NodeEnum::AlterTableStmt(alter)
            if ObjectType::try_from(alter.objtype).unwrap_or(ObjectType::Undefined)
                == ObjectType::ObjectIndex =>
        {
            validate_index_attachment(alter)?;
            Ok(StatementSpec::Utility(
                UtilityStatementKind::AttachIndexPartition,
            ))
        }
        NodeEnum::VariableSetStmt(setting) => {
            validate_session_setting(setting)?;
            Ok(StatementSpec::Utility(UtilityStatementKind::Set))
        }
        NodeEnum::DefineStmt(aggregate)
            if ObjectType::try_from(aggregate.kind).unwrap_or(ObjectType::Undefined)
                == ObjectType::ObjectAggregate =>
        {
            Ok(StatementSpec::Utility(validate_aggregate(aggregate)?))
        }
        NodeEnum::AlterTableStmt(alter) => {
            Ok(StatementSpec::AlterTable(validate_alter_table(alter)?))
        }
        NodeEnum::TransactionStmt(statement) => {
            Ok(StatementSpec::Utility(validate_transaction(statement)?))
        }
        NodeEnum::DropStmt(statement) => {
            validate_drop(statement)?;
            Ok(StatementSpec::Utility(UtilityStatementKind::Drop))
        }
        NodeEnum::TruncateStmt(statement) => {
            validate_truncate(statement)?;
            Ok(StatementSpec::Utility(UtilityStatementKind::Truncate))
        }
        NodeEnum::GrantStmt(statement) => {
            let kind = validate_grant(statement)?;
            Ok(StatementSpec::Utility(kind))
        }
        NodeEnum::GrantRoleStmt(statement) => {
            let kind = validate_grant_role(statement)?;
            Ok(StatementSpec::Utility(kind))
        }
        NodeEnum::CommentStmt(statement) => {
            validate_comment(statement)?;
            Ok(StatementSpec::Utility(UtilityStatementKind::Comment))
        }
        NodeEnum::CreateEnumStmt(statement) => {
            if statement.type_name.is_empty() || statement.vals.is_empty() {
                return Err("empty CREATE TYPE AS ENUM");
            }
            Ok(StatementSpec::Utility(UtilityStatementKind::CreateEnum))
        }
        NodeEnum::CompositeTypeStmt(statement) => {
            if statement.typevar.is_none() || statement.coldeflist.is_empty() {
                return Err("empty CREATE TYPE composite definition");
            }
            for column in &statement.coldeflist {
                let Some(NodeEnum::ColumnDef(column)) = column.node.as_ref() else {
                    return Err("unrecognized composite type column");
                };
                validate_column_def(column)?;
            }
            Ok(StatementSpec::Utility(
                UtilityStatementKind::CreateCompositeType {
                    fields: statement.coldeflist.len(),
                },
            ))
        }
        NodeEnum::CreateDomainStmt(statement) => {
            validate_domain(statement)?;
            Ok(StatementSpec::Utility(UtilityStatementKind::CreateDomain))
        }
        NodeEnum::CreateSeqStmt(statement) => Ok(StatementSpec::Utility(
            UtilityStatementKind::CreateSequence(validate_sequence(statement)?),
        )),
        NodeEnum::CreateTrigStmt(statement) => Ok(StatementSpec::Utility(
            UtilityStatementKind::CreateTrigger(validate_trigger(statement)?),
        )),
        NodeEnum::CreatePolicyStmt(statement) => {
            validate_policy(statement)?;
            Ok(StatementSpec::Utility(UtilityStatementKind::CreatePolicy))
        }
        NodeEnum::CopyStmt(statement) => {
            validate_copy(statement)?;
            Ok(StatementSpec::Utility(UtilityStatementKind::Copy))
        }
        NodeEnum::CallStmt(statement) => {
            let call = statement
                .funccall
                .as_ref()
                .ok_or("CALL without function call")?;
            for argument in &call.args {
                validate_ddl_expression(argument)?;
            }
            Ok(StatementSpec::Utility(UtilityStatementKind::Call))
        }
        NodeEnum::ExplainStmt(statement) => {
            validate_def_elements(&statement.options, "EXPLAIN option")?;
            let query = statement
                .query
                .as_deref()
                .ok_or("EXPLAIN without statement")?;
            validate_nested_statement(query)?;
            Ok(StatementSpec::Utility(UtilityStatementKind::Explain))
        }
        NodeEnum::VacuumStmt(statement) => {
            validate_def_elements(&statement.options, "VACUUM/ANALYZE option")?;
            for relation in &statement.rels {
                let Some(NodeEnum::VacuumRelation(relation)) = relation.node.as_ref() else {
                    return Err("unrecognized VACUUM/ANALYZE relation");
                };
                if relation.relation.is_none() {
                    return Err("VACUUM/ANALYZE relation without target");
                }
                for column in &relation.va_cols {
                    if !matches!(column.node.as_ref(), Some(NodeEnum::String(_))) {
                        return Err("unrecognized VACUUM/ANALYZE column");
                    }
                }
            }
            Ok(StatementSpec::Utility(if statement.is_vacuumcmd {
                UtilityStatementKind::Vacuum
            } else {
                UtilityStatementKind::Analyze
            }))
        }
        NodeEnum::RefreshMatViewStmt(statement) => {
            if statement.relation.is_none() {
                return Err("REFRESH MATERIALIZED VIEW without relation");
            }
            Ok(StatementSpec::Utility(
                UtilityStatementKind::RefreshMaterializedView,
            ))
        }
        NodeEnum::ListenStmt(statement) if !statement.conditionname.is_empty() => {
            Ok(StatementSpec::Utility(UtilityStatementKind::Listen))
        }
        NodeEnum::NotifyStmt(statement) if !statement.conditionname.is_empty() => {
            Ok(StatementSpec::Utility(UtilityStatementKind::Notify))
        }
        NodeEnum::CreateExtensionStmt(statement) => {
            if statement.extname.is_empty() {
                return Err("CREATE EXTENSION without name");
            }
            validate_def_elements(&statement.options, "CREATE EXTENSION option")?;
            Ok(StatementSpec::Utility(
                UtilityStatementKind::CreateExtension,
            ))
        }
        NodeEnum::AlterEnumStmt(statement) => {
            if statement.type_name.is_empty() || statement.new_val.is_empty() {
                return Err("incomplete ALTER TYPE enum action");
            }
            Ok(StatementSpec::Utility(UtilityStatementKind::AlterType))
        }
        NodeEnum::AlterDomainStmt(statement) => {
            if statement.type_name.is_empty() || statement.subtype.is_empty() {
                return Err("incomplete ALTER DOMAIN action");
            }
            if let Some(definition) = statement.def.as_deref() {
                validate_ddl_expression(definition)?;
            }
            validate_drop_behavior(statement.behavior)?;
            Ok(StatementSpec::Utility(UtilityStatementKind::AlterDomain))
        }
        NodeEnum::AlterPolicyStmt(statement) => {
            if statement.policy_name.is_empty() || statement.table.is_none() {
                return Err("incomplete ALTER POLICY");
            }
            if let Some(qual) = statement.qual.as_deref() {
                validate_ddl_expression(qual)?;
            }
            if let Some(check) = statement.with_check.as_deref() {
                validate_ddl_expression(check)?;
            }
            Ok(StatementSpec::Utility(UtilityStatementKind::AlterPolicy))
        }
        NodeEnum::RuleStmt(statement) => {
            if statement.rulename.is_empty()
                || statement.relation.is_none()
                || statement.actions.is_empty()
            {
                return Err("incomplete CREATE RULE");
            }
            if let Some(predicate) = statement.where_clause.as_deref() {
                validate_ddl_expression(predicate)?;
            }
            for action in &statement.actions {
                validate_nested_statement(action)?;
            }
            Ok(StatementSpec::Utility(UtilityStatementKind::CreateRule))
        }
        NodeEnum::CreateStatsStmt(statement) => {
            if statement.exprs.is_empty() || statement.relations.is_empty() || statement.transformed
            {
                return Err("incomplete or transformed CREATE STATISTICS");
            }
            for expression in &statement.exprs {
                validate_ddl_expression(expression)?;
            }
            Ok(StatementSpec::Utility(
                UtilityStatementKind::CreateStatistics,
            ))
        }
        NodeEnum::DefineStmt(statement)
            if ObjectType::try_from(statement.kind).unwrap_or(ObjectType::Undefined)
                == ObjectType::ObjectCollation =>
        {
            if statement.defnames.is_empty() || statement.oldstyle {
                return Err("incomplete or old-style CREATE COLLATION");
            }
            validate_def_elements(&statement.definition, "CREATE COLLATION option")?;
            Ok(StatementSpec::Utility(
                UtilityStatementKind::CreateCollation,
            ))
        }
        NodeEnum::CreateCastStmt(statement) => {
            if statement.sourcetype.is_none() || statement.targettype.is_none() {
                return Err("CREATE CAST without source or target type");
            }
            if !statement.inout && statement.func.is_none() {
                return Err("CREATE CAST without function or INOUT");
            }
            Ok(StatementSpec::Utility(UtilityStatementKind::CreateCast))
        }
        NodeEnum::CreateSchemaStmt(statement) => {
            if statement.schemaname.is_empty() && statement.authrole.is_none() {
                return Err("CREATE SCHEMA without name or authorization");
            }
            if !statement.schema_elts.is_empty() {
                return Err("CREATE SCHEMA with nested elements");
            }
            Ok(StatementSpec::Utility(UtilityStatementKind::CreateSchema))
        }
        NodeEnum::AlterSeqStmt(statement) => {
            if statement.sequence.is_none() {
                return Err("ALTER SEQUENCE without sequence");
            }
            validate_def_elements(&statement.options, "ALTER SEQUENCE option")?;
            Ok(StatementSpec::Utility(UtilityStatementKind::AlterSequence))
        }
        NodeEnum::RenameStmt(statement) => {
            validate_rename(statement)?;
            Ok(StatementSpec::Utility(UtilityStatementKind::RenameObject))
        }
        NodeEnum::CreateFunctionStmt(_) => Err("function or procedure definition"),
        NodeEnum::DoStmt(_) => Err("DO block"),
        _ => Err("unimplemented PostgreSQL statement family"),
    }
}

fn validate_session_setting(
    setting: &pg_query::protobuf::VariableSetStmt,
) -> Result<(), &'static str> {
    use pg_query::protobuf::VariableSetKind;
    let kind = VariableSetKind::try_from(setting.kind).unwrap_or(VariableSetKind::Undefined);
    if setting.name.is_empty()
        || !matches!(
            kind,
            VariableSetKind::VarSetValue
                | VariableSetKind::VarSetDefault
                | VariableSetKind::VarSetCurrent
        )
    {
        return Err("unreviewed session setting form");
    }
    if setting
        .args
        .iter()
        .any(|argument| !matches!(argument.node.as_ref(), Some(NodeEnum::AConst(_))))
    {
        return Err("unreviewed session setting value");
    }
    Ok(())
}

fn validate_index_attachment(alter: &AlterTableStmt) -> Result<(), &'static str> {
    if alter.relation.is_none() || alter.missing_ok || alter.cmds.len() != 1 {
        return Err("unreviewed ALTER INDEX attachment shape");
    }
    let Some(NodeEnum::AlterTableCmd(command)) = alter.cmds[0].node.as_ref() else {
        return Err("unrecognized ALTER INDEX action");
    };
    if AlterTableType::try_from(command.subtype).unwrap_or(AlterTableType::Undefined)
        != AlterTableType::AtAttachPartition
        || command.missing_ok
        || command.recurse
    {
        return Err("unreviewed ALTER INDEX action");
    }
    let Some(NodeEnum::PartitionCmd(partition)) =
        command.def.as_deref().and_then(|node| node.node.as_ref())
    else {
        return Err("ALTER INDEX attachment without partition ownership");
    };
    if partition.name.is_none() || partition.bound.is_some() || partition.concurrent {
        return Err("unreviewed index partition attachment");
    }
    Ok(())
}

fn validate_aggregate(
    aggregate: &pg_query::protobuf::DefineStmt,
) -> Result<UtilityStatementKind, &'static str> {
    if aggregate.oldstyle || aggregate.defnames.is_empty() || aggregate.if_not_exists {
        return Err("unreviewed aggregate definition form");
    }
    let [parameters, ordered] = aggregate.args.as_slice() else {
        return Err("aggregate without signature ownership");
    };
    let (signature, parameters) = match parameters.node.as_ref() {
        None => (AggregateSignatureSpec::Star, &[][..]),
        Some(NodeEnum::List(parameters)) => (
            AggregateSignatureSpec::Parameters {
                count: parameters.items.len(),
            },
            parameters.items.as_slice(),
        ),
        _ => return Err("unrecognized aggregate signature"),
    };
    if !matches!(ordered.node.as_ref(), Some(NodeEnum::Integer(value)) if value.ival == -1) {
        return Err("ordered-set aggregate signature");
    }
    for parameter in parameters {
        let Some(NodeEnum::FunctionParameter(parameter)) = parameter.node.as_ref() else {
            return Err("unrecognized aggregate parameter");
        };
        if parameter.arg_type.is_none()
            || parameter.defexpr.is_some()
            || !matches!(
                pg_query::protobuf::FunctionParameterMode::try_from(parameter.mode),
                Ok(pg_query::protobuf::FunctionParameterMode::FuncParamIn
                    | pg_query::protobuf::FunctionParameterMode::FuncParamDefault)
            )
        {
            return Err("unreviewed aggregate parameter shape");
        }
    }
    validate_def_elements(&aggregate.definition, "aggregate option")?;
    for option in &aggregate.definition {
        let Some(NodeEnum::DefElem(option)) = option.node.as_ref() else {
            return Err("unrecognized aggregate option");
        };
        if !matches!(
            option.defname.as_str(),
            "sfunc"
                | "stype"
                | "sspace"
                | "finalfunc"
                | "finalfunc_extra"
                | "finalfunc_modify"
                | "combinefunc"
                | "serialfunc"
                | "deserialfunc"
                | "initcond"
                | "msfunc"
                | "minvfunc"
                | "mstype"
                | "msspace"
                | "mfinalfunc"
                | "mfinalfunc_extra"
                | "mfinalfunc_modify"
                | "minitcond"
                | "sortop"
                | "parallel"
        ) {
            return Err("unreviewed aggregate option");
        }
    }
    Ok(UtilityStatementKind::CreateAggregate {
        signature,
        options: aggregate.definition.len(),
    })
}

fn validate_transaction(statement: &TransactionStmt) -> Result<UtilityStatementKind, &'static str> {
    if !statement.savepoint_name.is_empty() || !statement.gid.is_empty() {
        return Err("unreviewed transaction identifier");
    }

    match TransactionStmtKind::try_from(statement.kind).unwrap_or(TransactionStmtKind::Undefined) {
        TransactionStmtKind::TransStmtBegin if !statement.chain => {
            validate_begin_options(&statement.options)?;
            Ok(UtilityStatementKind::Begin)
        }
        TransactionStmtKind::TransStmtCommit
            if statement.options.is_empty() && !statement.chain =>
        {
            Ok(UtilityStatementKind::Commit)
        }
        _ => Err("unreviewed transaction statement"),
    }
}

fn validate_begin_options(options: &[Node]) -> Result<(), &'static str> {
    for option in options {
        let Some(NodeEnum::DefElem(option)) = option.node.as_ref() else {
            return Err("unrecognized BEGIN option");
        };
        if !option.defnamespace.is_empty()
            || DefElemAction::try_from(option.defaction).unwrap_or(DefElemAction::Undefined)
                != DefElemAction::DefelemUnspec
        {
            return Err("unreviewed BEGIN option shape");
        }
        let Some(NodeEnum::AConst(value)) =
            option.arg.as_deref().and_then(|node| node.node.as_ref())
        else {
            return Err("BEGIN option without a constant value");
        };
        if value.isnull {
            return Err("NULL BEGIN option");
        }

        match (option.defname.as_str(), value.val.as_ref()) {
            ("transaction_isolation", Some(AConstValue::Sval(value)))
                if matches!(
                    value.sval.as_str(),
                    "serializable" | "repeatable read" | "read committed" | "read uncommitted"
                ) => {}
            (
                "transaction_read_only" | "transaction_deferrable",
                Some(AConstValue::Ival(value)),
            ) if matches!(value.ival, 0 | 1) => {}
            _ => return Err("unreviewed BEGIN transaction mode"),
        }
    }
    Ok(())
}

fn validate_copy(statement: &pg_query::protobuf::CopyStmt) -> Result<(), &'static str> {
    match (&statement.relation, statement.query.as_deref()) {
        (Some(relation), None) if relation.alias.is_none() => {}
        (None, Some(query)) if !statement.is_from => validate_nested_statement(query)?,
        _ => return Err("unreviewed COPY source/target form"),
    }
    for column in &statement.attlist {
        if !matches!(column.node.as_ref(), Some(NodeEnum::String(_))) {
            return Err("unrecognized COPY column");
        }
    }
    validate_def_elements(&statement.options, "COPY option")?;
    if let Some(predicate) = statement.where_clause.as_deref() {
        validate_ddl_expression(predicate)?;
    }
    Ok(())
}

fn validate_nested_statement(statement: &Node) -> Result<(), &'static str> {
    match statement.node.as_ref() {
        Some(NodeEnum::SelectStmt(select)) => {
            let _ = validate_select(select, false)?;
            Ok(())
        }
        Some(NodeEnum::InsertStmt(insert)) => {
            let _ = validate_insert(insert)?;
            Ok(())
        }
        Some(NodeEnum::UpdateStmt(update)) => {
            let _ = validate_update(update)?;
            Ok(())
        }
        Some(NodeEnum::DeleteStmt(delete)) => {
            let _ = validate_delete(delete)?;
            Ok(())
        }
        Some(NodeEnum::MergeStmt(merge)) => {
            let _ = validate_merge(merge)?;
            Ok(())
        }
        Some(NodeEnum::NotifyStmt(notify)) if !notify.conditionname.is_empty() => Ok(()),
        _ => Err("unsupported nested utility statement"),
    }
}

fn validate_rename(statement: &pg_query::protobuf::RenameStmt) -> Result<(), &'static str> {
    if statement.newname.is_empty() {
        return Err("ALTER ... RENAME without new name");
    }
    let kind = ObjectType::try_from(statement.rename_type).unwrap_or(ObjectType::Undefined);
    if !matches!(
        kind,
        ObjectType::ObjectType
            | ObjectType::ObjectAttribute
            | ObjectType::ObjectIndex
            | ObjectType::ObjectMatview
            | ObjectType::ObjectTrigger
    ) {
        return Err("unreviewed ALTER ... RENAME object kind");
    }
    validate_drop_behavior(statement.behavior)
}

fn validate_drop(statement: &DropStmt) -> Result<(), &'static str> {
    if statement.objects.is_empty() {
        return Err("DROP without objects");
    }
    let object = ObjectType::try_from(statement.remove_type).unwrap_or(ObjectType::Undefined);
    if !matches!(
        object,
        ObjectType::ObjectTable
            | ObjectType::ObjectView
            | ObjectType::ObjectMatview
            | ObjectType::ObjectIndex
            | ObjectType::ObjectSequence
            | ObjectType::ObjectType
            | ObjectType::ObjectDomain
            | ObjectType::ObjectSchema
            | ObjectType::ObjectTrigger
            | ObjectType::ObjectPolicy
    ) {
        return Err("unreviewed DROP object kind");
    }
    if statement.concurrent && object != ObjectType::ObjectIndex {
        return Err("CONCURRENTLY on non-index DROP");
    }
    validate_drop_behavior(statement.behavior)
}

fn validate_truncate(statement: &TruncateStmt) -> Result<(), &'static str> {
    if statement.relations.is_empty() {
        return Err("TRUNCATE without relations");
    }
    for relation in &statement.relations {
        let Some(NodeEnum::RangeVar(relation)) = relation.node.as_ref() else {
            return Err("unrecognized TRUNCATE relation");
        };
        if relation.alias.is_some() {
            return Err("TRUNCATE relation alias");
        }
    }
    validate_drop_behavior(statement.behavior)
}

fn validate_grant(statement: &GrantStmt) -> Result<UtilityStatementKind, &'static str> {
    if GrantTargetType::try_from(statement.targtype).unwrap_or(GrantTargetType::Undefined)
        != GrantTargetType::AclTargetObject
    {
        return Err("unreviewed GRANT target mode");
    }
    let object = ObjectType::try_from(statement.objtype).unwrap_or(ObjectType::Undefined);
    if !matches!(
        object,
        ObjectType::ObjectTable
            | ObjectType::ObjectSequence
            | ObjectType::ObjectSchema
            | ObjectType::ObjectFunction
            | ObjectType::ObjectProcedure
            | ObjectType::ObjectType
            | ObjectType::ObjectDomain
    ) {
        return Err("unreviewed GRANT object kind");
    }
    if statement.objects.is_empty() || statement.grantees.is_empty() {
        return Err("GRANT without objects or grantees");
    }
    for privilege in &statement.privileges {
        let Some(NodeEnum::AccessPriv(privilege)) = privilege.node.as_ref() else {
            return Err("unrecognized GRANT privilege");
        };
        if privilege.priv_name.is_empty() {
            return Err("empty GRANT privilege");
        }
    }
    validate_drop_behavior(statement.behavior)?;
    Ok(if statement.is_grant {
        UtilityStatementKind::Grant
    } else {
        UtilityStatementKind::Revoke
    })
}

fn validate_grant_role(statement: &GrantRoleStmt) -> Result<UtilityStatementKind, &'static str> {
    if statement.granted_roles.is_empty() || statement.grantee_roles.is_empty() {
        return Err("role grant without roles or grantees");
    }
    for option in &statement.opt {
        let Some(NodeEnum::DefElem(option)) = option.node.as_ref() else {
            return Err("unrecognized role-grant option");
        };
        if !matches!(option.defname.as_str(), "admin" | "inherit" | "set") {
            return Err("unreviewed role-grant option");
        }
    }
    validate_drop_behavior(statement.behavior)?;
    Ok(if statement.is_grant {
        UtilityStatementKind::GrantRole
    } else {
        UtilityStatementKind::RevokeRole
    })
}

fn validate_comment(statement: &pg_query::protobuf::CommentStmt) -> Result<(), &'static str> {
    let object = ObjectType::try_from(statement.objtype).unwrap_or(ObjectType::Undefined);
    if !matches!(
        object,
        ObjectType::ObjectTable
            | ObjectType::ObjectColumn
            | ObjectType::ObjectView
            | ObjectType::ObjectMatview
            | ObjectType::ObjectIndex
            | ObjectType::ObjectSequence
            | ObjectType::ObjectType
            | ObjectType::ObjectDomain
            | ObjectType::ObjectTrigger
            | ObjectType::ObjectPolicy
            | ObjectType::ObjectFunction
            | ObjectType::ObjectProcedure
    ) {
        return Err("unreviewed COMMENT object kind");
    }
    if statement.object.is_none() {
        return Err("COMMENT without object");
    }
    Ok(())
}

fn validate_domain(statement: &CreateDomainStmt) -> Result<(), &'static str> {
    if statement.domainname.is_empty() || statement.type_name.is_none() {
        return Err("incomplete CREATE DOMAIN");
    }
    for constraint in &statement.constraints {
        let Some(NodeEnum::Constraint(constraint)) = constraint.node.as_ref() else {
            return Err("unrecognized domain constraint");
        };
        let kind = ConstrType::try_from(constraint.contype).unwrap_or(ConstrType::Undefined);
        if !matches!(
            kind,
            ConstrType::ConstrDefault
                | ConstrType::ConstrNotnull
                | ConstrType::ConstrNull
                | ConstrType::ConstrCheck
        ) {
            return Err("unreviewed domain constraint");
        }
        if let Some(expression) = constraint.raw_expr.as_deref() {
            validate_ddl_expression(expression)?;
        }
    }
    Ok(())
}

fn validate_sequence(statement: &CreateSeqStmt) -> Result<SequenceSpec, &'static str> {
    if statement.sequence.is_none() || statement.for_identity {
        return Err("unreviewed CREATE SEQUENCE form");
    }
    validate_sequence_options(&statement.options, false)
}

fn validate_sequence_options(
    source_options: &[Node],
    identity: bool,
) -> Result<SequenceSpec, &'static str> {
    let mut options = [None; 9];
    if source_options.len() > options.len() {
        return Err("too many sequence options");
    }
    for (slot, option) in options.iter_mut().zip(source_options) {
        let Some(NodeEnum::DefElem(option)) = option.node.as_ref() else {
            return Err("unrecognized sequence option");
        };
        let kind = match option.defname.as_str() {
            "as" => SequenceOptionKind::As,
            "increment" => SequenceOptionKind::Increment,
            "minvalue" => SequenceOptionKind::MinValue,
            "maxvalue" => SequenceOptionKind::MaxValue,
            "start" => SequenceOptionKind::Start,
            "cache" => SequenceOptionKind::Cache,
            "cycle" => SequenceOptionKind::Cycle,
            "owned_by" => SequenceOptionKind::OwnedBy,
            "sequence_name" if identity => SequenceOptionKind::SequenceName,
            _ => return Err("unreviewed sequence option"),
        };
        *slot = Some((
            kind,
            usize::try_from(option.location).map_err(|_| "unlocated sequence option")?,
        ));
    }
    Ok(SequenceSpec { options })
}

fn validate_trigger(statement: &CreateTrigStmt) -> Result<TriggerSpec, &'static str> {
    if statement.relation.is_none() || statement.funcname.is_empty() {
        return Err("incomplete CREATE TRIGGER");
    }
    let mut old_table = false;
    let mut new_table = false;
    for transition in &statement.transition_rels {
        let Some(NodeEnum::TriggerTransition(transition)) = transition.node.as_ref() else {
            return Err("unrecognized trigger transition relation");
        };
        if !transition.is_table || transition.name.is_empty() {
            return Err("unreviewed trigger transition relation");
        }
        let slot = if transition.is_new {
            &mut new_table
        } else {
            &mut old_table
        };
        if *slot {
            return Err("duplicate trigger transition relation");
        }
        *slot = true;
    }
    if let Some(expression) = statement.when_clause.as_deref() {
        validate_ddl_expression(expression)?;
    }
    Ok(TriggerSpec {
        // PostgreSQL's trigger timing bit flags: BEFORE=2, INSTEAD=64,
        // AFTER=0. These are part of the pinned PostgreSQL AST contract.
        timing: match statement.timing {
            0 => TriggerTiming::After,
            2 => TriggerTiming::Before,
            64 => TriggerTiming::InsteadOf,
            _ => return Err("unreviewed trigger timing"),
        },
        old_table,
        new_table,
        columns: statement.columns.len(),
        has_when: statement.when_clause.is_some(),
    })
}

fn validate_policy(statement: &CreatePolicyStmt) -> Result<(), &'static str> {
    if statement.policy_name.is_empty() || statement.table.is_none() {
        return Err("incomplete CREATE POLICY");
    }
    if !matches!(
        statement.cmd_name.as_str(),
        "all" | "select" | "insert" | "update" | "delete"
    ) {
        return Err("unreviewed CREATE POLICY command");
    }
    if let Some(expression) = statement.qual.as_deref() {
        validate_ddl_expression(expression)?;
    }
    if let Some(expression) = statement.with_check.as_deref() {
        validate_ddl_expression(expression)?;
    }
    Ok(())
}

fn validate_drop_behavior(value: i32) -> Result<(), &'static str> {
    match DropBehavior::try_from(value).unwrap_or(DropBehavior::Undefined) {
        DropBehavior::DropRestrict | DropBehavior::DropCascade => Ok(()),
        DropBehavior::Undefined => Err("unknown DROP behavior"),
    }
}

fn validate_view(view: &ViewStmt) -> Result<ViewSpec, &'static str> {
    let relation = view.view.as_ref().ok_or("CREATE VIEW without a relation")?;
    if !relation.inh {
        return Err("CREATE VIEW ONLY relation");
    }
    for alias in &view.aliases {
        if !matches!(alias.node.as_ref(), Some(NodeEnum::String(_))) {
            return Err("unrecognized CREATE VIEW column alias");
        }
    }
    validate_def_elements(&view.options, "CREATE VIEW option")?;
    let query = match view.query.as_deref().and_then(|query| query.node.as_ref()) {
        Some(NodeEnum::SelectStmt(query)) => validate_select(query, false)?,
        _ => return Err("CREATE VIEW without a SELECT query"),
    };
    let ctes = validated_nested_cte_specs(
        view.query
            .as_deref()
            .and_then(|query| query.node.as_ref())
            .ok_or("CREATE VIEW without a query")?,
    )?;
    let check = match ViewCheckOption::try_from(view.with_check_option)
        .unwrap_or(ViewCheckOption::Undefined)
    {
        ViewCheckOption::NoCheckOption => ViewCheckSpec::None,
        ViewCheckOption::LocalCheckOption => ViewCheckSpec::Local,
        ViewCheckOption::CascadedCheckOption => ViewCheckSpec::Cascaded,
        ViewCheckOption::Undefined => return Err("unknown CREATE VIEW check option"),
    };
    Ok(ViewSpec {
        replace: view.replace,
        aliases: view.aliases.len(),
        options: view.options.len(),
        check,
        query,
        ctes,
    })
}

fn validate_materialized_view(
    create: &CreateTableAsStmt,
) -> Result<MaterializedViewSpec, &'static str> {
    if create.is_select_into {
        return Err("SELECT INTO statement");
    }
    let into = create
        .into
        .as_deref()
        .ok_or("CREATE MATERIALIZED VIEW without INTO ownership")?;
    let relation = into
        .rel
        .as_ref()
        .ok_or("CREATE MATERIALIZED VIEW without a relation")?;
    if !relation.inh {
        return Err("CREATE MATERIALIZED VIEW ONLY relation");
    }
    for alias in &into.col_names {
        if !matches!(alias.node.as_ref(), Some(NodeEnum::String(_))) {
            return Err("unrecognized materialized-view column alias");
        }
    }
    validate_def_elements(&into.options, "materialized-view option")?;
    if OnCommitAction::try_from(into.on_commit).unwrap_or(OnCommitAction::Undefined)
        != OnCommitAction::OncommitNoop
    {
        return Err("materialized-view ON COMMIT clause");
    }
    if into.view_query.is_some() {
        return Err("transformed materialized-view query");
    }
    let query = match create
        .query
        .as_deref()
        .and_then(|query| query.node.as_ref())
    {
        Some(NodeEnum::SelectStmt(query)) => validate_select(query, false)?,
        _ => return Err("CREATE MATERIALIZED VIEW without a SELECT query"),
    };
    let ctes = validated_nested_cte_specs(
        create
            .query
            .as_deref()
            .and_then(|query| query.node.as_ref())
            .ok_or("CREATE MATERIALIZED VIEW without a query")?,
    )?;
    Ok(MaterializedViewSpec {
        if_not_exists: create.if_not_exists,
        aliases: into.col_names.len(),
        options: into.options.len(),
        has_access_method: !into.access_method.is_empty(),
        has_tablespace: !into.table_space_name.is_empty(),
        skip_data: into.skip_data,
        query,
        ctes,
    })
}

fn validate_def_elements(nodes: &[Node], feature: &'static str) -> Result<(), &'static str> {
    for node in nodes {
        let option = match node.node.as_ref() {
            Some(NodeEnum::DefElem(option)) => option,
            _ => return Err(feature),
        };
        if let Some(argument) = option.arg.as_deref() {
            validate_ddl_expression(argument)?;
        }
    }
    Ok(())
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

fn values_spec(select: &SelectStmt) -> ValuesSpec {
    ValuesSpec {
        rows: select.values_lists.len(),
        order_items: select.sort_clause.len(),
        has_limit_count: select.limit_count.is_some(),
        has_limit_offset: select.limit_offset.is_some(),
    }
}

fn validate_create_table(create: &CreateStmt) -> Result<CreateTableSpec, &'static str> {
    if create.relation.is_none() {
        return Err("CREATE TABLE without a relation");
    }
    if !create.constraints.is_empty() {
        return Err("transformed CREATE TABLE constraints");
    }
    if create.partspec.is_some() && create.partbound.is_some() {
        return Err("CREATE TABLE with both PARTITION BY and PARTITION OF bound");
    }

    let typed_table = create.of_typename.is_some();
    if typed_table
        && (!create.inh_relations.is_empty()
            || create.partspec.is_some()
            || create.partbound.is_some())
    {
        return Err("typed table combined with inheritance or partitioning");
    }

    for relation in &create.inh_relations {
        match relation.node.as_ref() {
            Some(NodeEnum::RangeVar(relation)) if relation.inh && relation.alias.is_none() => {}
            _ => return Err("unrecognized CREATE TABLE parent relation"),
        }
    }
    if create.partbound.is_some() && create.inh_relations.len() != 1 {
        return Err("PARTITION OF without exactly one parent relation");
    }
    if let Some(spec) = &create.partspec {
        validate_partition_spec(spec)?;
    }
    if let Some(bound) = &create.partbound {
        validate_partition_bound(bound)?;
    }
    validate_def_elements(&create.options, "CREATE TABLE storage parameter")?;
    match OnCommitAction::try_from(create.oncommit).unwrap_or(OnCommitAction::Undefined) {
        OnCommitAction::OncommitNoop
        | OnCommitAction::OncommitPreserveRows
        | OnCommitAction::OncommitDeleteRows
        | OnCommitAction::OncommitDrop => {}
        OnCommitAction::Undefined => return Err("unknown CREATE TABLE ON COMMIT action"),
    }

    if create.table_elts.is_empty() && create.partbound.is_none() && !typed_table {
        return Err("CREATE TABLE without columns, constraints, type, or partition parent");
    }

    let mut elements = Vec::with_capacity(create.table_elts.len());
    for element in &create.table_elts {
        match element.node.as_ref() {
            Some(NodeEnum::ColumnDef(column)) => {
                if typed_table {
                    validate_typed_column_def(column)?;
                } else {
                    validate_column_def(column)?;
                }
                elements.push(CreateTableElementSpec::Column {
                    check_constraints: column_check_constraint_count(column),
                    identity: column_identity_spec(column)?,
                });
            }
            Some(NodeEnum::Constraint(constraint)) if !typed_table => {
                validate_constraint(constraint)?;
                elements.push(CreateTableElementSpec::Constraint {
                    is_check: constraint_is_check(constraint),
                });
            }
            Some(NodeEnum::TableLikeClause(_)) => return Err("CREATE TABLE LIKE clause"),
            Some(NodeEnum::Constraint(_)) => return Err("typed table-level constraint"),
            _ => return Err("unrecognized CREATE TABLE element"),
        }
    }

    Ok(CreateTableSpec {
        if_not_exists: create.if_not_exists,
        elements,
        inheritance_relations: create.inh_relations.len(),
        has_partition_spec: create.partspec.is_some(),
        has_partition_bound: create.partbound.is_some(),
        typed_table,
        options: create.options.len(),
        has_on_commit: OnCommitAction::try_from(create.oncommit)
            .is_ok_and(|action| action != OnCommitAction::OncommitNoop),
        has_tablespace: !create.tablespacename.is_empty(),
        has_access_method: !create.access_method.is_empty(),
    })
}

fn validate_typed_column_def(column: &ColumnDef) -> Result<(), &'static str> {
    if column.colname.is_empty() || column.type_name.is_some() {
        return Err("typed-table column option must name an inherited column without a type");
    }
    if !column.fdwoptions.is_empty() || column.cooked_default.is_some() {
        return Err("unreviewed typed-table column option");
    }
    if let Some(default) = column.raw_default.as_deref() {
        validate_ddl_expression(default)?;
    }
    for constraint in &column.constraints {
        let constraint = match constraint.node.as_ref() {
            Some(NodeEnum::Constraint(constraint)) => constraint,
            _ => return Err("unrecognized typed-table column constraint"),
        };
        validate_constraint(constraint)?;
    }
    Ok(())
}

fn validate_partition_spec(spec: &pg_query::protobuf::PartitionSpec) -> Result<(), &'static str> {
    if matches!(
        PartitionStrategy::try_from(spec.strategy).unwrap_or(PartitionStrategy::Undefined),
        PartitionStrategy::Undefined
    ) || spec.part_params.is_empty()
    {
        return Err("invalid PARTITION BY specification");
    }
    for parameter in &spec.part_params {
        let parameter = match parameter.node.as_ref() {
            Some(NodeEnum::PartitionElem(parameter)) => parameter,
            _ => return Err("unrecognized partition key"),
        };
        if parameter.name.is_empty() == parameter.expr.is_none() {
            return Err("partition key must contain exactly one column or expression");
        }
        if let Some(expression) = parameter.expr.as_deref() {
            validate_ddl_expression(expression)?;
        }
        if parameter
            .collation
            .iter()
            .chain(parameter.opclass.iter())
            .any(|node| !matches!(node.node.as_ref(), Some(NodeEnum::String(_))))
        {
            return Err("unrecognized partition-key collation or operator class");
        }
    }
    Ok(())
}

fn validate_partition_bound(
    bound: &pg_query::protobuf::PartitionBoundSpec,
) -> Result<(), &'static str> {
    if bound.is_default {
        if bound.modulus != 0
            || bound.remainder != 0
            || !bound.listdatums.is_empty()
            || !bound.lowerdatums.is_empty()
            || !bound.upperdatums.is_empty()
        {
            return Err("DEFAULT partition with explicit bound data");
        }
        return Ok(());
    }
    match bound.strategy.as_str() {
        "l" => {
            if bound.listdatums.is_empty() {
                return Err("empty LIST partition bound");
            }
            for datum in &bound.listdatums {
                validate_ddl_expression(datum)?;
            }
        }
        "r" => {
            if bound.lowerdatums.is_empty() || bound.upperdatums.is_empty() {
                return Err("incomplete RANGE partition bound");
            }
            for datum in bound.lowerdatums.iter().chain(bound.upperdatums.iter()) {
                validate_ddl_expression(datum)?;
            }
        }
        "h" => {
            if bound.modulus <= 0 || bound.remainder < 0 || bound.remainder >= bound.modulus {
                return Err("invalid HASH partition bound");
            }
        }
        _ => return Err("unknown partition-bound strategy"),
    }
    Ok(())
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

    let mut actions = Vec::with_capacity(alter.cmds.len());
    for command in &alter.cmds {
        let command = match command.node.as_ref() {
            Some(NodeEnum::AlterTableCmd(command)) => command,
            _ => return Err("unrecognized ALTER TABLE action"),
        };
        let subtype =
            AlterTableType::try_from(command.subtype).unwrap_or(AlterTableType::Undefined);
        let group = alter_action_group(subtype)?;
        let (relation_options, check_constraints) = if matches!(
            subtype,
            AlterTableType::AtSetRelOptions
                | AlterTableType::AtResetRelOptions
                | AlterTableType::AtReplaceRelOptions
        ) {
            let definitions = match command
                .def
                .as_deref()
                .and_then(|definition| definition.node.as_ref())
            {
                Some(NodeEnum::List(definitions)) if !definitions.items.is_empty() => definitions,
                _ => return Err("ALTER TABLE relation options without an option list"),
            };
            validate_def_elements(
                &definitions.items,
                "unrecognized ALTER TABLE relation option",
            )?;
            (Some(definitions.items.len()), 0)
        } else if let Some(definition) = command.def.as_deref() {
            let check_constraints = match definition.node.as_ref() {
                Some(NodeEnum::ColumnDef(column)) => {
                    validate_column_def(column)?;
                    column_check_constraint_count(column)
                }
                Some(NodeEnum::Constraint(constraint)) => {
                    validate_constraint(constraint)?;
                    usize::from(constraint_is_check(constraint))
                }
                Some(_) => {
                    validate_ddl_expression(definition)?;
                    0
                }
                None => return Err("empty ALTER TABLE action definition"),
            };
            (None, check_constraints)
        } else {
            (None, 0)
        };
        actions.push(AlterTableActionSpec {
            group,
            relation_options,
            check_constraints,
            identity: match command.def.as_deref().and_then(|node| node.node.as_ref()) {
                Some(NodeEnum::Constraint(constraint)) => identity_spec(constraint)?,
                _ => None,
            },
            foreign_key: match command.def.as_deref().and_then(|node| node.node.as_ref()) {
                Some(NodeEnum::Constraint(constraint))
                    if ConstrType::try_from(constraint.contype)
                        == Ok(ConstrType::ConstrForeign) =>
                {
                    Some(ForeignKeySpec {
                        keys: constraint.fk_attrs.len(),
                        referenced_keys: constraint.pk_attrs.len(),
                        update_action: foreign_key_action(&constraint.fk_upd_action)?,
                        delete_action: foreign_key_action(&constraint.fk_del_action)?,
                    })
                }
                _ => None,
            },
        });
    }

    Ok(AlterTableSpec {
        if_exists: alter.missing_ok,
        actions,
    })
}

fn foreign_key_action(value: &str) -> Result<ForeignKeyAction, &'static str> {
    match value {
        "a" => Ok(ForeignKeyAction::NoAction),
        "r" => Ok(ForeignKeyAction::Restrict),
        "c" => Ok(ForeignKeyAction::Cascade),
        "n" => Ok(ForeignKeyAction::SetNull),
        "d" => Ok(ForeignKeyAction::SetDefault),
        _ => Err("unreviewed foreign key action"),
    }
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

fn constraint_is_check(constraint: &Constraint) -> bool {
    ConstrType::try_from(constraint.contype).is_ok_and(|kind| kind == ConstrType::ConstrCheck)
}

fn identity_spec(constraint: &Constraint) -> Result<Option<IdentitySpec>, &'static str> {
    if ConstrType::try_from(constraint.contype) != Ok(ConstrType::ConstrIdentity) {
        return Ok(None);
    }
    if !matches!(constraint.generated_when.as_str(), "a" | "d") {
        return Err("unreviewed identity generation mode");
    }
    Ok(Some(IdentitySpec {
        location: usize::try_from(constraint.location).map_err(|_| "unlocated identity clause")?,
        sequence: validate_sequence_options(&constraint.options, true)?,
    }))
}

fn column_identity_spec(column: &ColumnDef) -> Result<Option<IdentitySpec>, &'static str> {
    let mut identity = None;
    for constraint in &column.constraints {
        let Some(NodeEnum::Constraint(constraint)) = constraint.node.as_ref() else {
            return Err("unrecognized column constraint");
        };
        if let Some(spec) = identity_spec(constraint)? {
            if identity.is_some() {
                return Err("multiple column identity clauses");
            }
            identity = Some(spec);
        }
    }
    Ok(identity)
}

fn column_check_constraint_count(column: &ColumnDef) -> usize {
    column
        .constraints
        .iter()
        .filter_map(|constraint| match constraint.node.as_ref() {
            Some(NodeEnum::Constraint(constraint)) => Some(constraint),
            _ => None,
        })
        .filter(|constraint| constraint_is_check(constraint))
        .count()
}

fn validate_ddl_expression(expression: &Node) -> Result<(), &'static str> {
    let root = expression.node.as_ref().ok_or("empty DDL expression")?;
    walk_complete_tree(root, &mut |node, context| {
        if matches!(node, NodeRef::SubLink(_)) {
            return Err("subquery in DDL expression");
        }
        validate_nested_node(node, context)
    })
}

fn validate_merge(merge: &MergeStmt) -> Result<MergeSpec, &'static str> {
    let relation = merge
        .relation
        .as_ref()
        .ok_or("MERGE without a target relation")?;
    if !relation.inh {
        return Err("MERGE INTO ONLY target");
    }
    let source = merge
        .source_relation
        .as_deref()
        .ok_or("MERGE without a source relation")?;
    let source = validate_relation_list(std::slice::from_ref(source), "MERGE USING source")?;
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
        source,
        branches,
        returning_items: merge.returning_list.len(),
        target_alias: relation.alias.as_ref().map(|alias| alias.aliasname.clone()),
        returning_aliases: result_aliases(&merge.returning_list)?,
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
                if !select.sort_clause.is_empty()
                    || select.limit_count.is_some()
                    || select.limit_offset.is_some()
                {
                    return Err("INSERT VALUES query suffix");
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
        target_alias: insert
            .relation
            .as_ref()
            .and_then(|relation| relation.alias.as_ref())
            .map(|alias| alias.aliasname.clone()),
        returning_aliases: result_aliases(&insert.returning_list)?,
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

    let from = validate_relation_list(&update.from_clause, "UPDATE FROM source")?;

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
        from,
        has_where: update.where_clause.is_some(),
        returning_items: update.returning_list.len(),
        target_alias: relation.alias.as_ref().map(|alias| alias.aliasname.clone()),
        returning_aliases: result_aliases(&update.returning_list)?,
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
    let using = validate_relation_list(&delete.using_clause, "DELETE USING source")?;
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
        using,
        has_where: delete.where_clause.is_some(),
        returning_items: delete.returning_list.len(),
        target_alias: relation.alias.as_ref().map(|alias| alias.aliasname.clone()),
        returning_aliases: result_aliases(&delete.returning_list)?,
    })
}

fn validate_relation_list(
    sources: &[Node],
    feature: &'static str,
) -> Result<RelationListSpec, &'static str> {
    let mut result = RelationListSpec::default();
    for source in sources {
        let item = validate_relation_source(source, &mut result, feature)?;
        result.items.push(item);
    }
    result.joins.sort_unstable();
    Ok(result)
}

fn validate_relation_source(
    source: &Node,
    result: &mut RelationListSpec,
    feature: &'static str,
) -> Result<RelationItemSpec, &'static str> {
    match source.node.as_ref() {
        Some(NodeEnum::RangeVar(range)) if range.inh => {
            validate_alias_columns(range.alias.as_ref(), "relation alias column list")?;
            for name in [&range.catalogname, &range.schemaname, &range.relname]
                .into_iter()
                .filter(|name| !name.is_empty())
            {
                result
                    .identifiers
                    .push(RelationIdentifierSpec::Name(name.clone()));
            }
            push_alias(&mut result.identifiers, range.alias.as_ref())?;
            Ok(RelationItemSpec::Relation)
        }
        Some(NodeEnum::RangeVar(_)) => Err("ONLY relation source"),
        Some(NodeEnum::RangeSubselect(source)) => {
            validate_alias_columns(source.alias.as_ref(), "subquery alias column list")?;
            let query = match source
                .subquery
                .as_deref()
                .and_then(|query| query.node.as_ref())
            {
                Some(NodeEnum::SelectStmt(query)) => query,
                _ => return Err(feature),
            };
            let kind = if is_values_select_shape(query) {
                validate_values_select(query)?;
                RelationItemSpec::Values
            } else {
                let _ = validate_select(query, false)?;
                RelationItemSpec::Subquery
            };
            push_alias(&mut result.identifiers, source.alias.as_ref())?;
            Ok(kind)
        }
        Some(NodeEnum::RangeFunction(source)) => {
            validate_alias_columns(source.alias.as_ref(), "function alias column list")?;
            if source.functions.is_empty() {
                return Err("relation function without calls");
            }
            if source.is_rowsfrom {
                if !source.coldeflist.is_empty() {
                    return Err("ROWS FROM with outer column definition list");
                }
                for function in &source.functions {
                    let columns = validate_range_function_entry(function, true)?;
                    if !columns.is_empty() {
                        result
                            .identifiers
                            .push(RelationIdentifierSpec::ColumnDefinitions(columns));
                    }
                }
                push_alias(&mut result.identifiers, source.alias.as_ref())?;
                Ok(RelationItemSpec::RowsFrom)
            } else {
                if source.functions.len() != 1 {
                    return Err("multiple relation functions without ROWS FROM");
                }
                let _ = validate_range_function_entry(&source.functions[0], false)?;
                validate_column_definition_list(&source.coldeflist)?;
                push_alias(&mut result.identifiers, source.alias.as_ref())?;
                if !source.coldeflist.is_empty() {
                    result
                        .identifiers
                        .push(RelationIdentifierSpec::ColumnDefinitions(
                            column_definition_names(&source.coldeflist)?,
                        ));
                }
                Ok(RelationItemSpec::Function)
            }
        }
        Some(NodeEnum::RangeTableSample(sample)) => {
            let relation = sample
                .relation
                .as_deref()
                .ok_or("TABLESAMPLE without a relation")?;
            if !matches!(
                validate_relation_source(relation, result, feature)?,
                RelationItemSpec::Relation
            ) {
                return Err("TABLESAMPLE on a non-relation source");
            }
            if sample.method.is_empty()
                || !sample
                    .method
                    .iter()
                    .all(|node| matches!(node.node.as_ref(), Some(NodeEnum::String(_))))
            {
                return Err("unrecognized TABLESAMPLE method");
            }
            for argument in &sample.args {
                validate_dml_expression(argument)?;
            }
            if let Some(repeatable) = sample.repeatable.as_deref() {
                validate_dml_expression(repeatable)?;
            }
            Ok(RelationItemSpec::TableSample)
        }
        Some(NodeEnum::JoinExpr(join)) => {
            validate_join_source(join, result, feature)?;
            push_alias(&mut result.identifiers, join.alias.as_ref())?;
            Ok(RelationItemSpec::Join)
        }
        Some(NodeEnum::JsonTable(_)) => Err("JSON_TABLE expression"),
        _ => Err(feature),
    }
}

fn validate_alias_columns(
    alias: Option<&pg_query::protobuf::Alias>,
    feature: &'static str,
) -> Result<(), &'static str> {
    let Some(alias) = alias else {
        return Ok(());
    };
    if alias.aliasname.is_empty() {
        return Err("empty relation alias");
    }
    if alias
        .colnames
        .iter()
        .any(|column| !matches!(column.node.as_ref(), Some(NodeEnum::String(_))))
    {
        return Err(feature);
    }
    Ok(())
}

fn push_alias(
    identifiers: &mut Vec<RelationIdentifierSpec>,
    alias: Option<&pg_query::protobuf::Alias>,
) -> Result<(), &'static str> {
    let Some(alias) = alias else {
        return Ok(());
    };
    identifiers.push(RelationIdentifierSpec::Alias(AliasSpec {
        name: alias.aliasname.clone(),
        columns: string_nodes(&alias.colnames, "relation alias column")?,
    }));
    Ok(())
}

fn string_nodes(nodes: &[Node], feature: &'static str) -> Result<Vec<String>, &'static str> {
    nodes
        .iter()
        .map(|node| match node.node.as_ref() {
            Some(NodeEnum::String(value)) => Ok(value.sval.clone()),
            _ => Err(feature),
        })
        .collect()
}

fn result_aliases(results: &[Node]) -> Result<Vec<Option<String>>, &'static str> {
    results
        .iter()
        .map(|result| match result.node.as_ref() {
            Some(NodeEnum::ResTarget(target)) => {
                Ok((!target.name.is_empty()).then(|| target.name.clone()))
            }
            _ => Err("unrecognized result expression"),
        })
        .collect()
}

fn validate_range_function_entry(
    entry: &Node,
    rows_from: bool,
) -> Result<Vec<String>, &'static str> {
    let list = match entry.node.as_ref() {
        Some(NodeEnum::List(list)) if list.items.len() == 2 => list,
        _ => return Err("unrecognized relation function source"),
    };
    validate_dml_expression(&list.items[0])?;
    let definitions = match list.items[1].node.as_ref() {
        None => &[][..],
        Some(NodeEnum::List(definitions)) => definitions.items.as_slice(),
        _ => return Err("unrecognized relation function column definitions"),
    };
    if !rows_from && !definitions.is_empty() {
        return Err("inline function column definitions outside ROWS FROM");
    }
    validate_column_definition_list(definitions)?;
    column_definition_names(definitions)
}

fn column_definition_names(definitions: &[Node]) -> Result<Vec<String>, &'static str> {
    definitions
        .iter()
        .map(|definition| match definition.node.as_ref() {
            Some(NodeEnum::ColumnDef(column)) => Ok(column.colname.clone()),
            _ => Err("unrecognized relation column definition"),
        })
        .collect()
}

fn validate_column_definition_list(definitions: &[Node]) -> Result<(), &'static str> {
    for definition in definitions {
        let column = match definition.node.as_ref() {
            Some(NodeEnum::ColumnDef(column)) => column,
            _ => return Err("unrecognized relation column definition"),
        };
        validate_column_def(column)?;
    }
    Ok(())
}

fn validate_join_source(
    join: &JoinExpr,
    result: &mut RelationListSpec,
    feature: &'static str,
) -> Result<(), &'static str> {
    if join.rtindex != 0 {
        return Err("transformed join source");
    }
    if join.join_using_alias.is_some() {
        return Err("JOIN USING alias");
    }
    validate_alias_columns(join.alias.as_ref(), "join alias column list")?;
    if join
        .alias
        .as_ref()
        .is_some_and(|alias| !alias.colnames.is_empty())
    {
        return Err("join alias column list");
    }
    let kind = match JoinType::try_from(join.jointype).unwrap_or(JoinType::Undefined) {
        JoinType::JoinInner => RelationJoinTypeSpec::Inner,
        JoinType::JoinLeft => RelationJoinTypeSpec::Left,
        JoinType::JoinRight => RelationJoinTypeSpec::Right,
        JoinType::JoinFull => RelationJoinTypeSpec::Full,
        _ => return Err("internal or unknown join type"),
    };
    let left = join.larg.as_deref().ok_or("join without a left source")?;
    let right = join.rarg.as_deref().ok_or("join without a right source")?;
    validate_relation_source(left, result, feature)?;
    validate_relation_source(right, result, feature)?;

    if join.quals.is_some() && !join.using_clause.is_empty() {
        return Err("join with both ON and USING");
    }
    let constraint = if join.is_natural {
        if join.quals.is_some() || !join.using_clause.is_empty() {
            return Err("NATURAL JOIN with an explicit constraint");
        }
        RelationJoinConstraintSpec::Natural
    } else if let Some(predicate) = join.quals.as_deref() {
        validate_dml_expression(predicate)?;
        RelationJoinConstraintSpec::On
    } else if !join.using_clause.is_empty() {
        for column in &join.using_clause {
            if !matches!(column.node.as_ref(), Some(NodeEnum::String(_))) {
                return Err("unrecognized JOIN USING column");
            }
        }
        RelationJoinConstraintSpec::Using {
            columns: join.using_clause.len(),
        }
    } else if kind == RelationJoinTypeSpec::Inner {
        RelationJoinConstraintSpec::Cross
    } else {
        return Err("qualified outer join without ON, USING, or NATURAL");
    };
    result.joins.push(RelationJoinSpec { kind, constraint });
    Ok(())
}

fn validate_dml_expression(expression: &Node) -> Result<(), &'static str> {
    let root = expression
        .node
        .as_ref()
        .ok_or("empty data-modifying expression")?;
    validate_complete_tree(root)
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
        if let Some(search) = &cte.search_clause {
            if search.search_col_list.is_empty() || search.search_seq_column.is_empty() {
                return Err("incomplete CTE SEARCH clause");
            }
        }
        if let Some(cycle) = &cte.cycle_clause {
            if cycle.cycle_col_list.is_empty()
                || cycle.cycle_mark_column.is_empty()
                || cycle.cycle_path_column.is_empty()
            {
                return Err("incomplete CTE CYCLE clause");
            }
            if let Some(value) = cycle.cycle_mark_value.as_deref() {
                validate_ddl_expression(value)?;
            }
            if let Some(value) = cycle.cycle_mark_default.as_deref() {
                validate_ddl_expression(value)?;
            }
        }
        let query = cte
            .ctequery
            .as_deref()
            .and_then(|query| query.node.as_ref())
            .ok_or("empty common table expression")?;
        match query {
            NodeEnum::SelectStmt(select) if is_values_select_shape(select) => {
                validate_values_select(select)?;
            }
            NodeEnum::SelectStmt(select) => {
                let _ = validate_select(select, with_clause.recursive)?;
            }
            NodeEnum::InsertStmt(insert) => {
                let _ = validate_insert(insert)?;
            }
            NodeEnum::UpdateStmt(update) => {
                let _ = validate_update(update)?;
            }
            NodeEnum::DeleteStmt(delete) => {
                let _ = validate_delete(delete)?;
            }
            NodeEnum::MergeStmt(merge) => {
                let _ = validate_merge(merge)?;
            }
            _ => return Err("unreviewed common table expression body"),
        }
    }
    Ok(())
}

fn validate_select(
    select: &SelectStmt,
    allow_recursive_union: bool,
) -> Result<SelectSpec, &'static str> {
    validate_select_fields(select, false)?;
    let from = validate_relation_list(&select.from_clause, "SELECT FROM source")?;

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
        has_into: select.into_clause.is_some(),
        set_operations,
        named_windows: select.window_clause.len(),
        has_order_by: !select.sort_clause.is_empty(),
        has_limit_offset: select.limit_offset.is_some(),
        has_limit_count: select.limit_count.is_some(),
        locking_clauses: select.locking_clause.len(),
        from,
        target_aliases: result_aliases(&select.target_list)?,
        window_names: select
            .window_clause
            .iter()
            .map(|window| match window.node.as_ref() {
                Some(NodeEnum::WindowDef(window)) if !window.name.is_empty() => {
                    Ok(window.name.clone())
                }
                _ => Err("unrecognized named window"),
            })
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn validate_select_fields(select: &SelectStmt, allow_values: bool) -> Result<(), &'static str> {
    if let Some(into) = &select.into_clause {
        let relation = into.rel.as_ref().ok_or("SELECT INTO without target")?;
        if relation.alias.is_some() || into.view_query.is_some() || into.skip_data {
            return Err("unreviewed SELECT INTO target form");
        }
        for name in &into.col_names {
            if !matches!(name.node.as_ref(), Some(NodeEnum::String(_))) {
                return Err("unrecognized SELECT INTO column name");
            }
        }
        validate_def_elements(&into.options, "SELECT INTO option")?;
        match OnCommitAction::try_from(into.on_commit).unwrap_or(OnCommitAction::Undefined) {
            OnCommitAction::OncommitNoop
            | OnCommitAction::OncommitPreserveRows
            | OnCommitAction::OncommitDeleteRows
            | OnCommitAction::OncommitDrop => {}
            OnCommitAction::Undefined => return Err("unknown SELECT INTO ON COMMIT mode"),
        }
    }
    if !allow_values && !select.values_lists.is_empty() {
        return Err("VALUES statement");
    }
    for clause in &select.locking_clause {
        let Some(NodeEnum::LockingClause(clause)) = clause.node.as_ref() else {
            return Err("unrecognized row-locking clause");
        };
        if !matches!(
            LockClauseStrength::try_from(clause.strength).unwrap_or(LockClauseStrength::Undefined),
            LockClauseStrength::LcsForkeyshare
                | LockClauseStrength::LcsForshare
                | LockClauseStrength::LcsFornokeyupdate
                | LockClauseStrength::LcsForupdate
        ) {
            return Err("unknown row-locking strength");
        }
        if matches!(
            LockWaitPolicy::try_from(clause.wait_policy).unwrap_or(LockWaitPolicy::Undefined),
            LockWaitPolicy::Undefined
        ) {
            return Err("unknown row-locking wait policy");
        }
        for relation in &clause.locked_rels {
            if !matches!(relation.node.as_ref(), Some(NodeEnum::RangeVar(_))) {
                return Err("unrecognized row-locking relation");
            }
        }
    }
    Ok(())
}

fn validate_nested_node(node: NodeRef<'_>, _context: Context) -> Result<(), &'static str> {
    match node {
        NodeRef::SelectStmt(select) => validate_nested_select(select),
        NodeRef::JsonObjectConstructor(constructor) => {
            validate_simple_json_object_constructor(constructor)
        }
        NodeRef::JsonArrayConstructor(constructor) => {
            validate_simple_json_array_constructor(constructor)
        }
        NodeRef::JsonFuncExpr(_) => Err("JSON query/value/exists expression"),
        NodeRef::JsonSerializeExpr(_) => Err("JSON serialization expression"),
        NodeRef::JsonScalarExpr(_) => Err("JSON scalar expression"),
        NodeRef::JsonParseExpr(_) => Err("JSON parse expression"),
        NodeRef::JsonIsPredicate(_) => Err("IS JSON predicate"),
        NodeRef::JsonTable(_) => Err("JSON_TABLE expression"),
        NodeRef::JsonArrayQueryConstructor(_)
        | NodeRef::JsonAggConstructor(_)
        | NodeRef::JsonObjectAgg(_)
        | NodeRef::JsonArrayAgg(_)
        | NodeRef::JsonConstructorExpr(_)
        | NodeRef::JsonExpr(_) => Err("advanced SQL/JSON expression"),
        _ => Ok(()),
    }
}

fn validate_simple_json_object_constructor(
    constructor: &pg_query::protobuf::JsonObjectConstructor,
) -> Result<(), &'static str> {
    if constructor.output.is_some() || constructor.absent_on_null || constructor.unique {
        return Err("advanced JSON_OBJECT constructor");
    }
    Ok(())
}

fn validate_simple_json_array_constructor(
    constructor: &pg_query::protobuf::JsonArrayConstructor,
) -> Result<(), &'static str> {
    if constructor.output.is_some() || !constructor.absent_on_null {
        return Err("advanced JSON_ARRAY constructor");
    }
    Ok(())
}

fn validate_nested_select(select: &SelectStmt) -> Result<(), &'static str> {
    let values_shape = is_values_select_shape(select);
    if values_shape {
        return validate_values_select(select);
    }
    validate_select_fields(select, false)?;
    let _ = validate_relation_list(&select.from_clause, "nested SELECT FROM source")?;
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
}

/// Localize a rejected VALUES shape using its parser-owned first expression.
/// The location must lie in a structural VALUES wrapper; otherwise retain the
/// enclosing statement range rather than guessing a keyword occurrence.
fn unsupported_values_range(source: &str, raw: &RawStmt, feature: &str) -> Option<(usize, usize)> {
    let root = raw.stmt.as_deref()?.node.as_ref()?;
    let mut anchors = Vec::new();
    walk_complete_tree(root, &mut |node, _| {
        if let NodeRef::SelectStmt(query) = node
            && !query.values_lists.is_empty()
            && validate_select(query, false).err() == Some(feature)
            && let Some(NodeEnum::List(row)) = query.values_lists[0].node.as_ref()
            && let Some(expression) = row.items.first().and_then(|node| node.node.as_ref())
            && let Some(anchor) = first_expression_location(expression)
        {
            anchors.push(anchor);
        }
        Ok(())
    })
    .ok()?;
    if anchors.is_empty() {
        return None;
    }
    let tokens = super::tokens::tokenize(source).ok()?;
    let structure = super::structure::TokenStructure::new(&tokens);
    structure
        .parenthesis_pairs()
        .iter()
        .filter_map(|(open, close)| {
            let keyword = (*open + 1..*close).find(|index| !tokens[*index].is_comment())?;
            if tokens[keyword].kind != pg_query::protobuf::Token::Values {
                return None;
            }
            let first_row = (keyword + 1..*close).find(|index| !tokens[*index].is_comment())?;
            if tokens[first_row].kind != pg_query::protobuf::Token::Ascii40 {
                return None;
            }
            let row_close = structure.matching_parenthesis(first_row)?;
            anchors
                .iter()
                .any(|anchor| tokens[first_row].start <= *anchor && *anchor < tokens[row_close].end)
                .then_some((tokens[keyword].start, tokens[*close].start))
        })
        .min_by_key(|(start, _)| *start)
}

fn first_expression_location(expression: &NodeEnum) -> Option<usize> {
    expression
        .nodes()
        .into_iter()
        .filter_map(|(node, _, _, _)| {
            let location = match node {
                NodeRef::AConst(value) => value.location,
                NodeRef::ColumnRef(value) => value.location,
                NodeRef::ParamRef(value) => value.location,
                NodeRef::FuncCall(value) => value.location,
                NodeRef::TypeCast(value) => value.location,
                NodeRef::AExpr(value) => value.location,
                NodeRef::BoolExpr(value) => value.location,
                NodeRef::CaseExpr(value) => value.location,
                NodeRef::CoalesceExpr(value) => value.location,
                NodeRef::MinMaxExpr(value) => value.location,
                NodeRef::RowExpr(value) => value.location,
                NodeRef::AArrayExpr(value) => value.location,
                NodeRef::SubLink(value) => value.location,
                NodeRef::SetToDefault(value) => value.location,
                _ => return None,
            };
            usize::try_from(location).ok()
        })
        .min()
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
    let (start, end) = unsupported_values_range(source, raw, feature).unwrap_or((start, end));
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
