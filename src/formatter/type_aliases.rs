use pg_query::NodeRef;
use serde::Deserialize;

use super::tokens::{SqlToken, tokenize};
use super::{Diagnostic, FormatDiagnostic, FormatOptions, Severity, SourceRange};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeAliasFamily {
    Smallint,
    Integer,
    Bigint,
    Smallserial,
    Serial,
    Bigserial,
    Boolean,
    Character,
    CharacterVarying,
    BitVarying,
    Numeric,
    Real,
    DoublePrecision,
    TimeWithTimeZone,
    TimestampWithoutTimeZone,
    TimestampWithTimeZone,
}

impl TypeAliasFamily {
    pub(crate) fn config_name(self) -> &'static str {
        match self {
            Self::Smallint => "smallint",
            Self::Integer => "integer",
            Self::Bigint => "bigint",
            Self::Smallserial => "smallserial",
            Self::Serial => "serial",
            Self::Bigserial => "bigserial",
            Self::Boolean => "boolean",
            Self::Character => "character",
            Self::CharacterVarying => "character_varying",
            Self::BitVarying => "bit_varying",
            Self::Numeric => "numeric",
            Self::Real => "real",
            Self::DoublePrecision => "double_precision",
            Self::TimeWithTimeZone => "time_with_time_zone",
            Self::TimestampWithoutTimeZone => "timestamp_without_time_zone",
            Self::TimestampWithTimeZone => "timestamp_with_time_zone",
        }
    }

    pub(crate) fn accepts(self, spelling: &str) -> bool {
        match self {
            Self::Smallint => matches!(spelling, "smallint" | "int2"),
            Self::Integer => matches!(spelling, "integer" | "int" | "int4"),
            Self::Bigint => matches!(spelling, "bigint" | "int8"),
            Self::Smallserial => matches!(spelling, "smallserial" | "serial2"),
            Self::Serial => matches!(spelling, "serial" | "serial4"),
            Self::Bigserial => matches!(spelling, "bigserial" | "serial8"),
            Self::Boolean => matches!(spelling, "boolean" | "bool"),
            Self::Character => matches!(spelling, "character" | "char"),
            Self::CharacterVarying => matches!(spelling, "character varying" | "varchar"),
            Self::BitVarying => matches!(spelling, "bit varying" | "varbit"),
            Self::Numeric => matches!(spelling, "numeric" | "decimal"),
            Self::Real => matches!(spelling, "real" | "float4"),
            Self::DoublePrecision => matches!(spelling, "double precision" | "float" | "float8"),
            Self::TimeWithTimeZone => matches!(spelling, "time with time zone" | "timetz"),
            Self::TimestampWithoutTimeZone => {
                matches!(spelling, "timestamp" | "timestamp without time zone")
            }
            Self::TimestampWithTimeZone => {
                matches!(spelling, "timestamp with time zone" | "timestamptz")
            }
        }
    }
}

pub(super) struct NormalizedAliases {
    pub output: String,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone)]
struct Replacement {
    family: TypeAliasFamily,
    source_range: SourceRange,
    authored: String,
    preferred: String,
}

pub(super) fn normalize(
    source: &str,
    options: &FormatOptions,
    opaque_ranges: &[SourceRange],
) -> Result<NormalizedAliases, FormatDiagnostic> {
    if options.type_aliases.is_empty() {
        return Ok(NormalizedAliases {
            output: source.to_owned(),
            diagnostics: Vec::new(),
        });
    }

    if let Some(region) = super::find_copy_stdin_region(source)? {
        let prefix = normalize(
            &source[..region.header_start],
            options,
            &relative_ranges(opaque_ranges, 0, region.header_start),
        )?;
        let header = normalize(
            &source[region.header_start..region.header_end],
            options,
            &relative_ranges(opaque_ranges, region.header_start, region.header_end),
        )?;
        let suffix = normalize(
            &source[region.payload_end..],
            options,
            &relative_ranges(opaque_ranges, region.payload_end, source.len()),
        )?;
        let mut diagnostics = prefix.diagnostics;
        diagnostics.extend(
            header
                .diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.shifted(region.header_start)),
        );
        diagnostics.extend(
            suffix
                .diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.shifted(region.payload_end)),
        );
        return Ok(NormalizedAliases {
            output: format!(
                "{}{}{}{}",
                prefix.output,
                header.output,
                &source[region.header_end..region.payload_end],
                suffix.output
            ),
            diagnostics,
        });
    }

    let parsed = match pg_query::parse(source) {
        Ok(parsed) => parsed,
        Err(_) => {
            return Ok(NormalizedAliases {
                output: source.to_owned(),
                diagnostics: Vec::new(),
            });
        }
    };
    let tokens = tokenize(source)?;
    let mut replacements = Vec::new();

    for raw in &parsed.protobuf.stmts {
        let Some(root) = raw
            .stmt
            .as_deref()
            .and_then(|statement| statement.node.as_ref())
        else {
            continue;
        };
        for (node, _, _, _) in root.nodes() {
            for type_name in owned_type_names(node) {
                let Ok(start) = usize::try_from(type_name.location) else {
                    continue;
                };
                let Some(index) = tokens.iter().position(|token| token.start == start) else {
                    continue;
                };
                let Some((family, end)) = alias_at(&tokens, index) else {
                    continue;
                };
                let source_range = SourceRange::new(tokens[index].start, tokens[end - 1].end);
                if opaque_ranges
                    .iter()
                    .any(|opaque| contains(*opaque, source_range))
                {
                    continue;
                }
                let Some(preferred) = options.type_aliases.get(&family) else {
                    continue;
                };
                let authored = source[source_range.start..source_range.end].to_owned();
                if authored.eq_ignore_ascii_case(preferred) {
                    continue;
                }
                replacements.push(Replacement {
                    family,
                    source_range,
                    authored,
                    preferred: preferred.clone(),
                });
            }
        }
    }

    replacements.sort_by_key(|replacement| replacement.source_range.start);
    replacements.dedup_by_key(|replacement| replacement.source_range);
    let mut output = source.to_owned();
    for replacement in replacements.iter().rev() {
        output.replace_range(
            replacement.source_range.start..replacement.source_range.end,
            &replacement.preferred,
        );
    }
    pg_query::parse(&output)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;

    let diagnostics = replacements
        .into_iter()
        .map(|replacement| Diagnostic {
            rule_id: "type.alias".into(),
            severity: Severity::Error,
            message: format!(
                "{} type spelling must be `{}` instead of `{}`",
                replacement.family.config_name(),
                replacement.preferred,
                replacement.authored
            ),
            source_range: replacement.source_range,
            fix_available: true,
        })
        .collect();
    Ok(NormalizedAliases {
        output,
        diagnostics,
    })
}

fn relative_ranges(ranges: &[SourceRange], start: usize, end: usize) -> Vec<SourceRange> {
    ranges
        .iter()
        .filter(|range| range.start >= start && range.end <= end)
        .map(|range| SourceRange::new(range.start - start, range.end - start))
        .collect()
}

fn owned_type_names(node: NodeRef<'_>) -> Vec<&pg_query::protobuf::TypeName> {
    match node {
        NodeRef::TypeName(type_name) => vec![type_name],
        NodeRef::TypeCast(cast) => cast.type_name.iter().collect(),
        NodeRef::ColumnDef(column) => column.type_name.iter().collect(),
        NodeRef::FunctionParameter(parameter) => parameter.arg_type.iter().collect(),
        NodeRef::CreateFunctionStmt(function) => function
            .parameters
            .iter()
            .filter_map(|node| match node.node.as_ref() {
                Some(pg_query::protobuf::node::Node::FunctionParameter(parameter)) => {
                    parameter.arg_type.as_ref()
                }
                _ => None,
            })
            .chain(function.return_type.iter())
            .collect(),
        NodeRef::CreateStmt(statement) => statement
            .table_elts
            .iter()
            .filter_map(|node| match node.node.as_ref() {
                Some(pg_query::protobuf::node::Node::ColumnDef(column)) => {
                    column.type_name.as_ref()
                }
                _ => None,
            })
            .collect(),
        NodeRef::CreateDomainStmt(domain) => domain.type_name.iter().collect(),
        _ => Vec::new(),
    }
}

fn contains(outer: SourceRange, inner: SourceRange) -> bool {
    inner.start >= outer.start && inner.end <= outer.end
}

pub(super) fn normalize_declaration(
    source: &str,
    options: &FormatOptions,
) -> Result<String, FormatDiagnostic> {
    let tokens = tokenize(source)?;
    let mut type_index = 1;
    if tokens
        .get(type_index)
        .is_some_and(|token| token.text.eq_ignore_ascii_case("constant"))
    {
        type_index += 1;
    }
    let Some((family, end)) = alias_at(&tokens, type_index) else {
        return Ok(source.to_owned());
    };
    let Some(preferred) = options.type_aliases.get(&family) else {
        return Ok(source.to_owned());
    };
    let range = tokens[type_index].start..tokens[end - 1].end;
    if source[range.clone()].eq_ignore_ascii_case(preferred) {
        return Ok(source.to_owned());
    }
    let mut output = source.to_owned();
    output.replace_range(range, preferred);
    Ok(output)
}

fn alias_at(tokens: &[SqlToken<'_>], index: usize) -> Option<(TypeAliasFamily, usize)> {
    for (words, family) in [
        (
            &["timestamp", "without", "time", "zone"][..],
            TypeAliasFamily::TimestampWithoutTimeZone,
        ),
        (
            &["timestamp", "with", "time", "zone"][..],
            TypeAliasFamily::TimestampWithTimeZone,
        ),
        (
            &["time", "with", "time", "zone"][..],
            TypeAliasFamily::TimeWithTimeZone,
        ),
        (
            &["character", "varying"][..],
            TypeAliasFamily::CharacterVarying,
        ),
        (&["bit", "varying"][..], TypeAliasFamily::BitVarying),
        (
            &["double", "precision"][..],
            TypeAliasFamily::DoublePrecision,
        ),
    ] {
        if words.iter().enumerate().all(|(offset, word)| {
            tokens
                .get(index + offset)
                .is_some_and(|token| token.text.eq_ignore_ascii_case(word))
        }) {
            return Some((family, index + words.len()));
        }
    }

    let text = tokens[index].text.to_ascii_lowercase();
    let family = match text.as_str() {
        "smallint" | "int2" => TypeAliasFamily::Smallint,
        "integer" | "int" | "int4" => TypeAliasFamily::Integer,
        "bigint" | "int8" => TypeAliasFamily::Bigint,
        "smallserial" | "serial2" => TypeAliasFamily::Smallserial,
        "serial" | "serial4" => TypeAliasFamily::Serial,
        "bigserial" | "serial8" => TypeAliasFamily::Bigserial,
        "boolean" | "bool" => TypeAliasFamily::Boolean,
        "character" | "char" => TypeAliasFamily::Character,
        "varchar" => TypeAliasFamily::CharacterVarying,
        "varbit" => TypeAliasFamily::BitVarying,
        "numeric" | "decimal" => TypeAliasFamily::Numeric,
        "real" | "float4" => TypeAliasFamily::Real,
        "float8" => TypeAliasFamily::DoublePrecision,
        "float" if tokens.get(index + 1).is_none_or(|token| token.text != "(") => {
            TypeAliasFamily::DoublePrecision
        }
        "timetz" => TypeAliasFamily::TimeWithTimeZone,
        "timestamp" => TypeAliasFamily::TimestampWithoutTimeZone,
        "timestamptz" => TypeAliasFamily::TimestampWithTimeZone,
        _ => return None,
    };
    Some((family, index + 1))
}
