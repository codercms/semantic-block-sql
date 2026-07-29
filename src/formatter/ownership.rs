use pg_query::protobuf::{RawStmt, Token};

use super::tokens::SqlToken;
use super::{FormatDiagnostic, SourceRange};

/// Exact top-level SELECT capabilities proven by PostgreSQL AST validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SelectSpec {
    pub has_with: bool,
    pub has_into: bool,
    pub set_operations: usize,
    pub named_windows: usize,
    pub locking_clauses: usize,
}

/// Top-level VALUES capabilities proven by PostgreSQL AST validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ValuesSpec {
    pub rows: usize,
}

/// INSERT source shape accepted by the validator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InsertSourceSpec {
    DefaultValues,
    Values { rows: usize },
    Query { set_operations: usize },
}

/// PostgreSQL OVERRIDING mode accepted by INSERT or MERGE INSERT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OverrideSpec {
    None,
    User,
    System,
}

/// ON CONFLICT action shape accepted by the validator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConflictActionSpec {
    Nothing,
    Update { assignments: usize, has_where: bool },
}

/// ON CONFLICT ownership capabilities proven by AST validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ConflictSpec {
    pub has_target: bool,
    pub has_target_where: bool,
    pub action: ConflictActionSpec,
}

/// Exact top-level INSERT capabilities proven by PostgreSQL AST validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct InsertSpec {
    pub has_with: bool,
    pub target_columns: usize,
    pub overriding: OverrideSpec,
    pub source: InsertSourceSpec,
    pub conflict: Option<ConflictSpec>,
    pub returning_items: usize,
}

/// One top-level relation-source item accepted in FROM, USING, or MERGE USING.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RelationItemSpec {
    Relation,
    Subquery,
    Function,
    Join,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum RelationJoinTypeSpec {
    Inner,
    Left,
    Right,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum RelationJoinConstraintSpec {
    On,
    Using { columns: usize },
    Natural,
    Cross,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct RelationJoinSpec {
    pub kind: RelationJoinTypeSpec,
    pub constraint: RelationJoinConstraintSpec,
}

/// Exact relation-list capabilities proven by PostgreSQL AST validation.
///
/// Top-level item kinds preserve authored order. Join specifications preserve
/// recursive source order plus the exact join type and constraint mode, so a
/// token binder cannot satisfy the contract with only aggregate counters.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct RelationListSpec {
    pub items: Vec<RelationItemSpec>,
    pub joins: Vec<RelationJoinSpec>,
}

/// Exact top-level UPDATE capabilities proven by PostgreSQL AST validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UpdateSpec {
    pub has_with: bool,
    pub assignments: usize,
    pub from: RelationListSpec,
    pub has_where: bool,
    pub returning_items: usize,
}

/// Exact top-level DELETE capabilities proven by PostgreSQL AST validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DeleteSpec {
    pub has_with: bool,
    pub using: RelationListSpec,
    pub has_where: bool,
    pub returning_items: usize,
}

/// MERGE branch action shape accepted by the validator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MergeActionSpec {
    Delete,
    Nothing,
    Update {
        assignments: usize,
    },
    Insert {
        target_columns: usize,
        values: usize,
        overriding: OverrideSpec,
    },
}

/// Exact MERGE branch capabilities proven by PostgreSQL AST validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MergeBranchSpec {
    pub has_condition: bool,
    pub action: MergeActionSpec,
}

/// Exact top-level MERGE capabilities proven by PostgreSQL AST validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MergeSpec {
    pub has_with: bool,
    pub source: RelationListSpec,
    pub branches: Vec<MergeBranchSpec>,
    pub returning_items: usize,
}

/// CREATE VIEW check-option mode accepted by the validator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ViewCheckSpec {
    None,
    Local,
    Cascaded,
}

/// Exact CREATE VIEW capabilities proven by PostgreSQL AST validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ViewSpec {
    pub replace: bool,
    pub aliases: usize,
    pub options: usize,
    pub check: ViewCheckSpec,
    pub query: SelectSpec,
}

/// Exact CREATE MATERIALIZED VIEW capabilities proven by PostgreSQL AST
/// validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MaterializedViewSpec {
    pub if_not_exists: bool,
    pub aliases: usize,
    pub options: usize,
    pub has_access_method: bool,
    pub has_tablespace: bool,
    pub skip_data: bool,
    pub query: SelectSpec,
}

/// CREATE TABLE element kind used to preserve the authored order while still
/// enforcing the column/constraint boundary in layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CreateTableElementSpec {
    Column,
    Constraint,
}

/// Exact CREATE TABLE capabilities proven by PostgreSQL AST validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CreateTableSpec {
    pub if_not_exists: bool,
    pub elements: Vec<CreateTableElementSpec>,
}

/// Exact CREATE INDEX capabilities proven by PostgreSQL AST validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CreateIndexSpec {
    pub unique: bool,
    pub concurrent: bool,
    pub if_not_exists: bool,
    pub key_items: usize,
    pub include_items: usize,
    pub options: usize,
    pub has_tablespace: bool,
    pub has_where: bool,
}

/// Coarse ALTER TABLE action groups used only for stable blank-line grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AlterTableActionGroup {
    Add,
    Alter,
    Drop,
    Set,
    Other,
}

/// Exact ALTER TABLE capabilities proven by PostgreSQL AST validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AlterTableSpec {
    pub if_exists: bool,
    pub action_groups: Vec<AlterTableActionGroup>,
}

/// Reviewed top-level migration/utility statement family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UtilityStatementKind {
    Drop,
    Truncate,
    Grant,
    Revoke,
    GrantRole,
    RevokeRole,
    Comment,
    CreateEnum,
    CreateCompositeType,
    CreateDomain,
    CreateSequence,
    CreateTrigger,
    CreatePolicy,
}

impl UtilityStatementKind {
    pub fn expected_token(self) -> Token {
        match self {
            Self::Drop => Token::Drop,
            Self::Truncate => Token::Truncate,
            Self::Grant | Self::GrantRole => Token::Grant,
            Self::Revoke | Self::RevokeRole => Token::Revoke,
            Self::Comment => Token::Comment,
            Self::CreateEnum
            | Self::CreateCompositeType
            | Self::CreateDomain
            | Self::CreateSequence
            | Self::CreateTrigger
            | Self::CreatePolicy => Token::Create,
        }
    }

    pub fn family_name(self) -> &'static str {
        match self {
            Self::Drop => "DROP",
            Self::Truncate => "TRUNCATE",
            Self::Grant => "GRANT",
            Self::Revoke => "REVOKE",
            Self::GrantRole => "GRANT ROLE",
            Self::RevokeRole => "REVOKE ROLE",
            Self::Comment => "COMMENT ON",
            Self::CreateEnum => "CREATE TYPE AS ENUM",
            Self::CreateCompositeType => "CREATE TYPE AS",
            Self::CreateDomain => "CREATE DOMAIN",
            Self::CreateSequence => "CREATE SEQUENCE",
            Self::CreateTrigger => "CREATE TRIGGER",
            Self::CreatePolicy => "CREATE POLICY",
        }
    }
}

/// Exact statement shape accepted by the PostgreSQL AST support gate.
///
/// This is deliberately a closed sum type. Adding a statement family requires
/// an explicit validation branch and an explicit layout dispatcher. Adding a
/// syntax variant to an existing family requires extending its capability
/// record, which makes the token binder verify the same shape that validation
/// accepted rather than rediscovering support implicitly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum StatementSpec {
    Select(SelectSpec),
    Values(ValuesSpec),
    Insert(InsertSpec),
    Update(UpdateSpec),
    Delete(DeleteSpec),
    Merge(MergeSpec),
    View(ViewSpec),
    MaterializedView(MaterializedViewSpec),
    CreateTable(CreateTableSpec),
    CreateIndex(CreateIndexSpec),
    AlterTable(AlterTableSpec),
    Utility(UtilityStatementKind),
}

impl StatementSpec {
    pub fn expected_token(&self) -> Token {
        match self {
            Self::Select(_) => Token::Select,
            Self::Values(_) => Token::Values,
            Self::Insert(_) => Token::Insert,
            Self::Update(_) => Token::Update,
            Self::Delete(_) => Token::DeleteP,
            Self::Merge(_) => Token::Merge,
            Self::View(_)
            | Self::MaterializedView(_)
            | Self::CreateTable(_)
            | Self::CreateIndex(_) => Token::Create,
            Self::AlterTable(_) => Token::Alter,
            Self::Utility(kind) => kind.expected_token(),
        }
    }

    pub fn family_name(&self) -> &'static str {
        match self {
            Self::Select(_) => "SELECT",
            Self::Values(_) => "VALUES",
            Self::Insert(_) => "INSERT",
            Self::Update(_) => "UPDATE",
            Self::Delete(_) => "DELETE",
            Self::Merge(_) => "MERGE",
            Self::View(_) => "CREATE VIEW",
            Self::MaterializedView(_) => "CREATE MATERIALIZED VIEW",
            Self::CreateTable(_) => "CREATE TABLE",
            Self::CreateIndex(_) => "CREATE INDEX",
            Self::AlterTable(_) => "ALTER TABLE",
            Self::Utility(kind) => kind.family_name(),
        }
    }

    pub fn has_with(&self) -> bool {
        match self {
            Self::Select(spec) => spec.has_with,
            Self::Values(_) => false,
            Self::Insert(spec) => spec.has_with,
            Self::Update(spec) => spec.has_with,
            Self::Delete(spec) => spec.has_with,
            Self::Merge(spec) => spec.has_with,
            Self::View(_)
            | Self::MaterializedView(_)
            | Self::CreateTable(_)
            | Self::CreateIndex(_)
            | Self::AlterTable(_)
            | Self::Utility(_) => false,
        }
    }
}

/// UTF-8 byte range owned by one top-level PostgreSQL statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SourceStatement {
    pub spec: StatementSpec,
    pub range: SourceRange,
}

/// AST-validated top-level ownership model shared by validation and layout.
#[derive(Debug, Default)]
pub(super) struct SupportedDocument {
    statements: Vec<SourceStatement>,
}

impl SupportedDocument {
    pub fn new(statements: Vec<SourceStatement>) -> Self {
        Self { statements }
    }

    pub fn statements(&self) -> &[SourceStatement] {
        &self.statements
    }
}

/// Half-open token range `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TokenRange {
    pub start: usize,
    pub end: usize,
}

impl TokenRange {
    pub fn new(start: usize, end: usize) -> Result<Self, FormatDiagnostic> {
        if start >= end {
            return Err(FormatDiagnostic::Ownership(format!(
                "invalid empty token range {start}..{end}"
            )));
        }
        Ok(Self { start, end })
    }
}

/// Token-indexed form of [`SourceStatement`].
///
/// `range` is always half-open and excludes a terminal semicolon. The optional
/// semicolon is stored separately, so callers cannot accidentally interpret one
/// field as inclusive in one statement and exclusive in another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StatementTokens {
    pub spec: StatementSpec,
    pub range: TokenRange,
    pub semicolon: Option<usize>,
    pub base_depth: usize,
}

pub(super) fn source_statement(
    source: &str,
    raw: &RawStmt,
    spec: StatementSpec,
) -> SourceStatement {
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
    SourceStatement {
        spec,
        range: SourceRange::new(start, end),
    }
}

pub(super) fn bind_token_statements(
    document: &SupportedDocument,
    tokens: &[SqlToken<'_>],
    depths: &[usize],
) -> Result<Vec<StatementTokens>, FormatDiagnostic> {
    let mut result = Vec::with_capacity(document.statements().len());

    for statement in document.statements() {
        let start = tokens
            .iter()
            .position(|token| {
                token.start >= statement.range.start && token.start < statement.range.end
            })
            .ok_or_else(|| {
                FormatDiagnostic::Ownership(format!(
                    "{} statement at bytes {}..{} has no source token",
                    statement.spec.family_name(),
                    statement.range.start,
                    statement.range.end
                ))
            })?;
        let source_end = tokens
            .iter()
            .position(|token| token.start >= statement.range.end)
            .unwrap_or(tokens.len());
        if source_end <= start {
            return Err(FormatDiagnostic::Ownership(format!(
                "{} statement at bytes {}..{} has an empty token span",
                statement.spec.family_name(),
                statement.range.start,
                statement.range.end
            )));
        }

        let semicolon = (tokens[source_end - 1].kind == Token::Ascii59).then_some(source_end - 1);
        let end = semicolon.unwrap_or(source_end);
        let range = TokenRange::new(start, end)?;
        let base_depth = depths[start];
        result.push(StatementTokens {
            spec: statement.spec.clone(),
            range,
            semicolon,
            base_depth,
        });
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formatter::tokens::tokenize;

    #[test]
    fn binds_multiple_statements_with_half_open_ranges_and_separate_semicolons() {
        let source = "select 1;\nupdate public.items set value = 2;";
        let tokens = tokenize(source).expect("scan succeeds");
        let mut depth = 0usize;
        let depths = tokens
            .iter()
            .map(|token| {
                let current = depth;
                match token.kind {
                    Token::Ascii40 | Token::Ascii91 => depth += 1,
                    Token::Ascii41 | Token::Ascii93 => depth = depth.saturating_sub(1),
                    _ => {}
                }
                current
            })
            .collect::<Vec<_>>();
        let document = SupportedDocument::new(vec![
            SourceStatement {
                spec: StatementSpec::Select(SelectSpec {
                    has_with: false,
                    has_into: false,
                    set_operations: 0,
                    named_windows: 0,
                    locking_clauses: 0,
                }),
                range: SourceRange::new(0, 9),
            },
            SourceStatement {
                spec: StatementSpec::Update(UpdateSpec {
                    has_with: false,
                    assignments: 1,
                    from: RelationListSpec::default(),
                    has_where: false,
                    returning_items: 0,
                }),
                range: SourceRange::new(10, source.len()),
            },
        ]);

        let bound = bind_token_statements(&document, &tokens, &depths).expect("bind succeeds");
        assert_eq!(bound.len(), 2);
        assert_eq!(tokens[bound[0].range.start].kind, Token::Select);
        assert_eq!(tokens[bound[0].range.end].kind, Token::Ascii59);
        assert_eq!(bound[0].semicolon, Some(bound[0].range.end));
        assert_eq!(tokens[bound[1].range.start].kind, Token::Update);
        assert_eq!(tokens[bound[1].range.end].kind, Token::Ascii59);
        assert_eq!(bound[1].semicolon, Some(bound[1].range.end));
    }

    #[test]
    fn no_semicolon_keeps_the_statement_range_half_open() {
        let source = "SELECT 1";
        let tokens = tokenize(source).expect("scan succeeds");
        let document = SupportedDocument::new(vec![SourceStatement {
            spec: StatementSpec::Select(SelectSpec {
                has_with: false,
                has_into: false,
                set_operations: 0,
                named_windows: 0,
                locking_clauses: 0,
            }),
            range: SourceRange::new(0, source.len()),
        }]);
        let depths = vec![0; tokens.len()];

        let bound = bind_token_statements(&document, &tokens, &depths).expect("bind succeeds");
        assert_eq!(bound[0].range.end, tokens.len());
        assert_eq!(bound[0].semicolon, None);
    }
}
