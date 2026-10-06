//! Shared PostgreSQL AST operations; independent of token layout policy.

use pg_query::{NodeRef, protobuf::node::Node as NodeEnum};
use serde_json::Value;

mod children;

/// A borrowed node in deterministic protobuf-field depth-first order.
/// AST depth is structural and must not be used as display indentation.
#[derive(Clone, Copy, Debug)]
pub(super) struct Visit<'a> {
    pub node: NodeRef<'a>,
    pub parent: Option<NodeRef<'a>>,
    pub depth: usize,
}

/// Complete iterative traversal; never clones or mutates the parser tree.
pub(super) struct DepthFirst<'a> {
    stack: Vec<Visit<'a>>,
    children: Vec<NodeRef<'a>>,
}

impl<'a> DepthFirst<'a> {
    pub fn new(root: &'a NodeEnum) -> Self {
        Self {
            stack: vec![Visit {
                node: root.to_ref(),
                parent: None,
                depth: 0,
            }],
            children: Vec::new(),
        }
    }
}

impl<'a> Iterator for DepthFirst<'a> {
    type Item = Visit<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let visit = self.stack.pop()?;
        self.children.clear();
        children::children(visit.node, &mut self.children);
        self.stack
            .extend(self.children.drain(..).rev().map(|node| Visit {
                node,
                parent: Some(visit.node),
                depth: visit.depth + 1,
            }));
        Some(visit)
    }
}

pub(super) fn walk_complete_tree<'a>(
    root: &'a NodeEnum,
    mut visitor: impl FnMut(NodeRef<'a>) -> Result<(), &'static str>,
) -> Result<(), &'static str> {
    for visit in DepthFirst::new(root) {
        visitor(visit.node)?;
    }
    Ok(())
}

/// Remove only reviewed PostgreSQL source metadata from equivalence trees.
pub(super) fn strip_locations(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for name in [
                "location",
                "stmt_location",
                "stmt_len",
                "arg_location",
                "payload_location",
                "conninfo_location",
            ] {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traversal_is_depth_first_with_exact_parents_and_stable_depths() {
        let parsed = pg_query::parse(
            "SELECT (SELECT inner_value FROM inner_rows), outer_value FROM outer_rows WHERE flag;",
        )
        .unwrap();
        let root = parsed.protobuf.stmts[0]
            .stmt
            .as_ref()
            .unwrap()
            .node
            .as_ref()
            .unwrap();
        let visits = DepthFirst::new(root).collect::<Vec<_>>();
        assert_eq!(visits[0].depth, 0);
        assert!(visits[0].parent.is_none());
        let columns = visits
            .iter()
            .filter_map(|visit| {
                let NodeRef::ColumnRef(column) = visit.node else {
                    return None;
                };
                let Some(NodeEnum::String(name)) = column.fields[0].node.as_ref() else {
                    return None;
                };
                Some((name.sval.as_str(), visit.depth))
            })
            .collect::<Vec<_>>();
        assert_eq!(
            columns,
            [("inner_value", 5), ("outer_value", 2), ("flag", 1)]
        );
        let inner = visits
            .iter()
            .find(|visit| visit.depth > 0 && matches!(visit.node, NodeRef::SelectStmt(_)))
            .unwrap();
        assert!(matches!(inner.parent, Some(NodeRef::SubLink(_))));
    }

    #[test]
    fn expression_and_statement_families_share_complete_child_enumeration() {
        let source = "WITH input AS (VALUES (NULL::int)) SELECT CASE NULL::int WHEN 1 THEN 2 END, ARRAY[NULL::int], count(*) FILTER (WHERE NULL::int IS NULL), array_agg(NULL::int ORDER BY NULL::int) FROM input LIMIT (SELECT NULL::int);";
        let parsed = pg_query::parse(source).unwrap();
        let root = parsed.protobuf.stmts[0]
            .stmt
            .as_ref()
            .unwrap()
            .node
            .as_ref()
            .unwrap();
        let visits = DepthFirst::new(root).collect::<Vec<_>>();
        assert_eq!(
            visits
                .iter()
                .filter(|visit| matches!(visit.node, NodeRef::TypeCast(_)))
                .count(),
            7
        );
        assert_eq!(
            visits
                .iter()
                .filter(|visit| matches!(visit.node, NodeRef::TypeName(_)))
                .count(),
            7
        );
        assert_eq!(
            visits
                .iter()
                .filter(|visit| matches!(visit.node, NodeRef::SelectStmt(_)))
                .count(),
            3
        );
        assert!(
            visits
                .iter()
                .any(|visit| matches!(visit.node, NodeRef::NullTest(_))
                    && matches!(visit.parent, Some(NodeRef::FuncCall(_))))
        );
        assert!(
            visits
                .iter()
                .any(|visit| matches!(visit.node, NodeRef::SortBy(_))
                    && matches!(visit.parent, Some(NodeRef::FuncCall(_))))
        );
    }

    #[test]
    fn deeply_nested_calls_use_the_same_iterative_walk() {
        let depth = 32;
        let source = format!(
            "SELECT {}value{};",
            "COALESCE(".repeat(depth),
            ")".repeat(depth)
        );
        let parsed = pg_query::parse(&source).unwrap();
        let root = parsed.protobuf.stmts[0]
            .stmt
            .as_ref()
            .unwrap()
            .node
            .as_ref()
            .unwrap();
        let visits = DepthFirst::new(root).collect::<Vec<_>>();
        assert_eq!(
            visits
                .iter()
                .filter(|visit| matches!(visit.node, NodeRef::CoalesceExpr(_)))
                .count(),
            depth
        );
        let column = visits
            .iter()
            .find(|visit| matches!(visit.node, NodeRef::ColumnRef(_)))
            .unwrap();
        assert_eq!(column.depth, depth + 2);
        assert!(matches!(column.parent, Some(NodeRef::CoalesceExpr(_))));
    }

    #[test]
    fn traversal_does_not_depend_on_the_rust_call_stack() {
        let parsed = pg_query::parse("SELECT value;").unwrap();
        let mut root = parsed.protobuf.stmts[0]
            .stmt
            .as_ref()
            .unwrap()
            .node
            .as_ref()
            .unwrap()
            .clone();
        for _ in 0..512 {
            root = NodeEnum::CoalesceExpr(Box::new(pg_query::protobuf::CoalesceExpr {
                args: vec![pg_query::protobuf::Node { node: Some(root) }],
                ..Default::default()
            }));
        }
        let visits = DepthFirst::new(&root).collect::<Vec<_>>();
        assert_eq!(
            visits
                .iter()
                .filter(|visit| matches!(visit.node, NodeRef::CoalesceExpr(_)))
                .count(),
            512
        );
        assert_eq!(
            visits
                .iter()
                .find(|visit| matches!(visit.node, NodeRef::SelectStmt(_)))
                .unwrap()
                .depth,
            512
        );
    }
}
