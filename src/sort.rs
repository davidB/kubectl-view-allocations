use crate::qty::Qty;
use crate::{Error, QtyByQualifier, TableNode};

#[derive(Debug, Clone, PartialEq)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SortColumnName {
    Usage,
    Requested,
    Limits,
    Allocatable,
    Free,
    Name,
}

#[derive(Debug, Clone)]
pub struct SortColumn {
    pub column: SortColumnName,
    pub direction: SortDirection,
}

#[allow(clippy::result_large_err)]
pub fn parse_sort_spec(s: &str) -> Result<Vec<SortColumn>, Error> {
    s.split(',')
        .map(|token| {
            let parts: Vec<&str> = token.split_whitespace().collect();
            let col_name = parts.first().copied().unwrap_or("").to_lowercase();
            let direction_str = parts.get(1).copied().unwrap_or("asc").to_lowercase();

            let column = match col_name.as_str() {
                "usage" | "utilization" => SortColumnName::Usage,
                "requested" => SortColumnName::Requested,
                "limits" | "limit" => SortColumnName::Limits,
                "allocatable" => SortColumnName::Allocatable,
                "free" => SortColumnName::Free,
                "name" => SortColumnName::Name,
                other => {
                    return Err(Error::InvalidSortColumn {
                        name: other.to_string(),
                    });
                }
            };

            let direction = match direction_str.as_str() {
                "desc" => SortDirection::Desc,
                _ => SortDirection::Asc,
            };

            Ok(SortColumn { column, direction })
        })
        .collect()
}

pub fn effective_sort_spec(spec: &[SortColumn], show_utilization: bool) -> Vec<SortColumn> {
    spec.iter()
        .filter(|col| show_utilization || col.column != SortColumnName::Usage)
        .cloned()
        .collect()
}

fn compare_qty(a: Option<&Qty>, b: Option<&Qty>) -> std::cmp::Ordering {
    match (a, b) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
        (Some(a), Some(b)) => a.cmp(b),
    }
}

fn compare_nodes_by(a: &TableNode, b: &TableNode, col: &SortColumn) -> std::cmp::Ordering {
    let ord = match col.column {
        SortColumnName::Name => a.key.cmp(&b.key),
        SortColumnName::Usage => compare_qty(
            a.quantities.as_ref().and_then(|q| q.utilization.as_ref()),
            b.quantities.as_ref().and_then(|q| q.utilization.as_ref()),
        ),
        SortColumnName::Requested => compare_qty(
            a.quantities.as_ref().and_then(|q| q.requested.as_ref()),
            b.quantities.as_ref().and_then(|q| q.requested.as_ref()),
        ),
        SortColumnName::Limits => compare_qty(
            a.quantities.as_ref().and_then(|q| q.limit.as_ref()),
            b.quantities.as_ref().and_then(|q| q.limit.as_ref()),
        ),
        SortColumnName::Allocatable => compare_qty(
            a.quantities.as_ref().and_then(|q| q.allocatable.as_ref()),
            b.quantities.as_ref().and_then(|q| q.allocatable.as_ref()),
        ),
        SortColumnName::Free => compare_qty(a.free.as_ref(), b.free.as_ref()),
    };
    match col.direction {
        SortDirection::Asc => ord,
        SortDirection::Desc => ord.reverse(),
    }
}

pub fn sort_children_recursive(
    nodes: &mut Vec<TableNode>,
    indices: &mut [usize],
    depth: usize,
    resource_depth: usize,
    sort_spec: &[SortColumn],
) {
    // At the resource level (cpu/memory/pods siblings), quantities are incomparable
    // across resource kinds → always sort by name ASC.
    // Ancestors (depth < resource_depth) have None quantities so they naturally fall
    // through to the name ASC tiebreaker anyway.
    let effective: &[SortColumn] = if depth == resource_depth {
        &[]
    } else {
        sort_spec
    };
    indices.sort_by(|&a, &b| {
        for col in effective {
            let ord = compare_nodes_by(&nodes[a], &nodes[b], col);
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
        }
        nodes[a].key.cmp(&nodes[b].key)
    });
    for &i in indices.iter() {
        let mut ch = std::mem::take(&mut nodes[i].children);
        sort_children_recursive(nodes, &mut ch, depth + 1, resource_depth, sort_spec);
        nodes[i].children = ch;
    }
}

pub fn flatten_tree(
    nodes: &[TableNode],
    indices: &[usize],
) -> Vec<(Vec<String>, Option<QtyByQualifier>, Option<Qty>)> {
    let mut out = vec![];
    for &i in indices {
        out.push((
            nodes[i].path.clone(),
            nodes[i].quantities.clone(),
            nodes[i].free.clone(),
        ));
        out.extend(flatten_tree(nodes, &nodes[i].children));
    }
    out
}
