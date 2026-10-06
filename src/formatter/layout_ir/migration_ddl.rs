use pg_query::protobuf::Token;

use super::{FormatDiagnostic, SqlToken, TokenRange, TokenStructure};
use crate::formatter::ownership::{
    ForeignKeyAction, ForeignKeySpec, SequenceOptionKind, SequenceSpec, StatementTokens,
    TriggerSpec, TriggerTiming,
};
use crate::formatter::tokens::next_non_comment;

fn missing(message: &str) -> FormatDiagnostic {
    FormatDiagnostic::Ownership(message.into())
}

pub(super) fn bind_sequence(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    range: TokenRange,
    base: usize,
    spec: SequenceSpec,
) -> Result<Vec<usize>, FormatDiagnostic> {
    let mut clauses = Vec::new();
    for &(kind, location) in spec.options.iter().flatten() {
        let index = (range.start..range.end)
            .find(|&index| tokens[index].start == location && structure.depth(index) == base)
            .ok_or_else(|| missing("sequence option location disagrees with its AST"))?;
        let expected = match kind {
            SequenceOptionKind::As => Token::As,
            SequenceOptionKind::Increment => Token::Increment,
            SequenceOptionKind::MinValue => Token::Minvalue,
            SequenceOptionKind::MaxValue => Token::Maxvalue,
            SequenceOptionKind::Start => Token::Start,
            SequenceOptionKind::Cache => Token::Cache,
            SequenceOptionKind::Cycle => Token::Cycle,
            SequenceOptionKind::OwnedBy => Token::Owned,
            SequenceOptionKind::SequenceName => Token::Sequence,
        };
        let keyword = if tokens[index].kind == Token::No
            && matches!(
                kind,
                SequenceOptionKind::MinValue
                    | SequenceOptionKind::MaxValue
                    | SequenceOptionKind::Cycle
            ) {
            next_non_comment(tokens, index)
                .ok_or_else(|| missing("sequence NO option is incomplete"))?
        } else {
            index
        };
        if tokens[keyword].kind != expected {
            return Err(missing("sequence option keyword disagrees with its AST"));
        }
        clauses.push(index);
    }
    Ok(clauses)
}

pub(super) fn bind_identity(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    range: TokenRange,
    base: usize,
    spec: crate::formatter::ownership::IdentitySpec,
) -> Result<super::IdentityBlock, FormatDiagnostic> {
    let introducer = (range.start..range.end)
        .find(|&index| {
            tokens[index].start == spec.location
                && structure.depth(index) == base
                && matches!(tokens[index].kind, Token::AddP | Token::Generated)
        })
        .ok_or_else(|| missing("identity clause location disagrees with its AST"))?;
    let identity = (introducer..range.end)
        .find(|&index| structure.depth(index) == base && tokens[index].kind == Token::IdentityP)
        .ok_or_else(|| missing("identity keyword is missing"))?;
    let open = next_non_comment(tokens, identity)
        .filter(|&index| index < range.end && tokens[index].kind == Token::Ascii40);
    let options = open
        .map(|open| {
            structure
                .matching_parenthesis(open)
                .filter(|&close| close < range.end)
                .map(|close| (open, close))
                .ok_or_else(|| missing("identity option list is unclosed"))
        })
        .transpose()?;
    let has_options = spec.sequence.options.iter().any(Option::is_some);
    if options.is_some() != has_options {
        return Err(missing("identity option list disagrees with its AST"));
    }
    let clauses = if let Some((open, close)) = options {
        bind_sequence(
            tokens,
            structure,
            TokenRange::new(open + 1, close)?,
            base + 1,
            spec.sequence,
        )?
    } else {
        Vec::new()
    };
    Ok(super::IdentityBlock {
        introducer,
        options,
        clauses,
    })
}

pub(super) fn bind_trigger(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    statement: &StatementTokens,
    spec: TriggerSpec,
) -> Result<(Vec<usize>, Vec<usize>), FormatDiagnostic> {
    let range = statement.range;
    let base = statement.base_depth;
    let trigger = (range.start..range.end)
        .find(|&index| tokens[index].kind == Token::Trigger && structure.depth(index) == base)
        .ok_or_else(|| missing("trigger keyword is missing"))?;
    let name =
        next_non_comment(tokens, trigger).ok_or_else(|| missing("trigger name is missing"))?;
    let timing =
        next_non_comment(tokens, name).ok_or_else(|| missing("trigger timing is missing"))?;
    let timing_kind = match spec.timing {
        TriggerTiming::Before => Token::Before,
        TriggerTiming::After => Token::After,
        TriggerTiming::InsteadOf => Token::Instead,
    };
    if tokens[timing].kind != timing_kind {
        return Err(missing("trigger timing disagrees with its AST"));
    }
    let mut clauses = vec![timing];
    let mut identifiers = vec![name];
    let on = (timing + 1..range.end)
        .find(|&index| tokens[index].kind == Token::On && structure.depth(index) == base)
        .ok_or_else(|| missing("trigger ON clause is missing"))?;
    if spec.columns > 0 {
        let of = (timing + 1..on)
            .find(|&index| tokens[index].kind == Token::Of && structure.depth(index) == base)
            .ok_or_else(|| missing("trigger UPDATE OF is missing"))?;
        let columns = (of + 1..on)
            .filter(|&index| !tokens[index].is_comment() && tokens[index].kind != Token::Ascii44)
            .collect::<Vec<_>>();
        if columns.len() != spec.columns {
            return Err(missing("trigger column count disagrees with its AST"));
        }
        identifiers.extend(columns);
    }
    clauses.push(on);
    let referencing = (on + 1..range.end)
        .find(|&index| tokens[index].kind == Token::Referencing && structure.depth(index) == base);
    if referencing.is_some() != (spec.old_table || spec.new_table) {
        return Err(missing("trigger transition clause disagrees with its AST"));
    }
    if let Some(referencing) = referencing {
        clauses.push(referencing);
        let mut cursor = next_non_comment(tokens, referencing)
            .ok_or_else(|| missing("trigger transition is missing"))?;
        let mut old_table = false;
        let mut new_table = false;
        while matches!(tokens[cursor].kind, Token::Old | Token::New) {
            if tokens[cursor].line_breaks_before > 0 {
                clauses.push(cursor);
            }
            let slot = if tokens[cursor].kind == Token::Old {
                &mut old_table
            } else {
                &mut new_table
            };
            if *slot {
                return Err(missing("trigger transition is duplicated"));
            }
            *slot = true;
            let table = next_non_comment(tokens, cursor)
                .ok_or_else(|| missing("transition TABLE is missing"))?;
            let after_table = next_non_comment(tokens, table)
                .ok_or_else(|| missing("transition alias is missing"))?;
            let alias = if tokens[after_table].kind == Token::As {
                next_non_comment(tokens, after_table)
                    .ok_or_else(|| missing("transition alias is missing"))?
            } else {
                after_table
            };
            if tokens[table].kind != Token::Table {
                return Err(missing("trigger transition grammar disagrees with its AST"));
            }
            identifiers.push(alias);
            cursor = next_non_comment(tokens, alias)
                .ok_or_else(|| missing("trigger transition is unterminated"))?;
        }
        if old_table != spec.old_table || new_table != spec.new_table {
            return Err(missing("trigger transition kinds disagree with its AST"));
        }
    }
    let when = (on + 1..range.end)
        .find(|&index| tokens[index].kind == Token::When && structure.depth(index) == base);
    if when.is_some() != spec.has_when {
        return Err(missing("trigger condition disagrees with its AST"));
    }
    clauses.extend(when);
    clauses.extend((on + 1..range.end).filter(|&index| {
        structure.depth(index) == base && matches!(tokens[index].kind, Token::For | Token::Execute)
    }));
    clauses.sort_unstable();
    Ok((clauses, identifiers))
}

pub(super) fn bind_foreign_key(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    range: TokenRange,
    base: usize,
    spec: ForeignKeySpec,
) -> Result<Vec<usize>, FormatDiagnostic> {
    let foreign = (range.start..range.end)
        .find(|&index| tokens[index].kind == Token::Foreign && structure.depth(index) == base)
        .ok_or_else(|| missing("foreign key introducer is missing"))?;
    let references = (foreign + 1..range.end)
        .find(|&index| tokens[index].kind == Token::References && structure.depth(index) == base)
        .ok_or_else(|| missing("foreign key REFERENCES is missing"))?;
    verify_key_list(tokens, structure, foreign, references, base, spec.keys)?;
    verify_key_list(
        tokens,
        structure,
        references,
        range.end,
        base,
        spec.referenced_keys,
    )?;
    let mut clauses = vec![foreign, references];
    let mut update = None;
    let mut delete = None;
    for index in references + 1..range.end {
        if structure.depth(index) != base {
            continue;
        }
        match tokens[index].kind {
            Token::On => {
                let command = next_non_comment(tokens, index)
                    .ok_or_else(|| missing("foreign key action command is missing"))?;
                let (slot, expected) = match tokens[command].kind {
                    Token::Update => (&mut update, spec.update_action),
                    Token::DeleteP => (&mut delete, spec.delete_action),
                    _ => return Err(missing("unrecognized foreign key action command")),
                };
                if slot.is_some() {
                    return Err(missing("duplicate foreign key action"));
                }
                let action = next_non_comment(tokens, command)
                    .ok_or_else(|| missing("foreign key action is missing"))?;
                let (first, second) = match expected {
                    ForeignKeyAction::NoAction => (Token::No, Some(Token::Action)),
                    ForeignKeyAction::Restrict => (Token::Restrict, None),
                    ForeignKeyAction::Cascade => (Token::Cascade, None),
                    ForeignKeyAction::SetNull => (Token::Set, Some(Token::NullP)),
                    ForeignKeyAction::SetDefault => (Token::Set, Some(Token::Default)),
                };
                if tokens[action].kind != first
                    || second.is_some_and(|kind| {
                        next_non_comment(tokens, action)
                            .is_none_or(|index| tokens[index].kind != kind)
                    })
                {
                    return Err(missing("foreign key action disagrees with its AST"));
                }
                *slot = Some(index);
                clauses.push(index);
            }
            Token::Match | Token::Initially => clauses.push(index),
            Token::Deferrable
                if crate::formatter::tokens::previous_non_comment(tokens, index)
                    .is_none_or(|previous| tokens[previous].kind != Token::Not) =>
            {
                clauses.push(index)
            }
            Token::Not
                if next_non_comment(tokens, index).is_some_and(|next| {
                    matches!(tokens[next].kind, Token::Valid | Token::Deferrable)
                }) =>
            {
                clauses.push(index)
            }
            _ => {}
        }
    }
    if (update.is_none() && spec.update_action != ForeignKeyAction::NoAction)
        || (delete.is_none() && spec.delete_action != ForeignKeyAction::NoAction)
    {
        return Err(missing("foreign key action is absent"));
    }
    Ok(clauses)
}

fn verify_key_list(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    start: usize,
    end: usize,
    base: usize,
    expected: usize,
) -> Result<(), FormatDiagnostic> {
    let open = (start + 1..end)
        .find(|&index| tokens[index].kind == Token::Ascii40 && structure.depth(index) == base);
    let count = if let Some(open) = open {
        let close = structure
            .matching_parenthesis(open)
            .filter(|&close| close < end)
            .ok_or_else(|| missing("foreign key list is unclosed"))?;
        1 + (open + 1..close)
            .filter(|&index| {
                tokens[index].kind == Token::Ascii44 && structure.depth(index) == base + 1
            })
            .count()
    } else {
        0
    };
    if count != expected {
        return Err(missing("foreign key column count disagrees with its AST"));
    }
    Ok(())
}
