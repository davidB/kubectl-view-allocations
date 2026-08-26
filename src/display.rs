use crate::qty::Qty;
#[cfg(feature = "prettytable")]
use crate::tree;
use crate::{GroupBy, QtyByQualifier};
use chrono::prelude::*;
#[cfg(feature = "prettytable")]
use prettytable::{Cell, Row, Table, format, row};
#[cfg(not(feature = "prettytable"))]
use tracing::warn;

pub fn display_as_csv(
    data: &[(Vec<String>, Option<QtyByQualifier>, Option<Qty>)],
    group_by: &[GroupBy],
    show_utilization: bool,
) {
    use itertools::Itertools;
    println!(
        "Date,Kind,{}{},Requested,%Requested,Limit,%Limit,Allocatable,Free",
        group_by.iter().map(|x| x.to_string()).join(","),
        if show_utilization {
            ",Utilization,%Utilization"
        } else {
            ""
        }
    );

    let empty = "".to_string();
    let datetime = Utc::now().to_rfc3339();
    for (k, oqtys, ofree) in data {
        if let Some(qtys) = oqtys {
            let mut row = vec![
                datetime.clone(),
                group_by
                    .get(k.len() - 1)
                    .map(|x| x.to_string())
                    .unwrap_or_else(|| empty.clone()),
            ];
            for i in 0..group_by.len() {
                row.push(csv_escape(k.get(i).map(|s| s.as_str()).unwrap_or("")));
            }

            if show_utilization {
                add_cells_for_csv(&mut row, &qtys.utilization, &qtys.allocatable);
            }
            add_cells_for_csv(&mut row, &qtys.requested, &qtys.allocatable);
            add_cells_for_csv(&mut row, &qtys.limit, &qtys.allocatable);

            row.push(
                qtys.allocatable
                    .as_ref()
                    .map(|qty| format!("{:.2}", f64::from(qty)))
                    .unwrap_or_else(|| empty.clone()),
            );
            row.push(
                ofree
                    .as_ref()
                    .map(|qty| format!("{:.2}", f64::from(qty)))
                    .unwrap_or_else(|| empty.clone()),
            );
            println!("{}", &row.join(","));
        }
    }
}

/// Emit the same data as the CSV output, but as a JSON array of records so that
/// downstream consumers (dashboards, CI checks) don't have to re-parse the flat
/// CSV. Quantities and percentages are numbers (or null when not available)
/// rather than the 2-decimal strings used by CSV.
pub fn display_as_json(
    data: &[(Vec<String>, Option<QtyByQualifier>, Option<Qty>)],
    group_by: &[GroupBy],
    show_utilization: bool,
) {
    let value = to_json_value(data, group_by, show_utilization);
    // Serializing a `serde_json::Value` can't fail, but fall back rather than panic.
    let out = serde_json::to_string_pretty(&value).unwrap_or_else(|_| "[]".to_string());
    println!("{out}");
}

fn to_json_value(
    data: &[(Vec<String>, Option<QtyByQualifier>, Option<Qty>)],
    group_by: &[GroupBy],
    show_utilization: bool,
) -> serde_json::Value {
    use serde_json::{Map, Value};

    let datetime = Utc::now().to_rfc3339();
    let mut rows: Vec<Value> = Vec::new();
    for (k, oqtys, ofree) in data {
        let Some(qtys) = oqtys else { continue };
        let mut obj = Map::new();
        obj.insert("date".to_string(), Value::String(datetime.clone()));
        obj.insert(
            "kind".to_string(),
            Value::String(
                group_by
                    .get(k.len() - 1)
                    .map(|x| x.to_string())
                    .unwrap_or_default(),
            ),
        );
        for (i, g) in group_by.iter().enumerate() {
            obj.insert(
                g.to_string(),
                k.get(i)
                    .map(|s| Value::String(s.clone()))
                    .unwrap_or(Value::Null),
            );
        }
        if show_utilization {
            obj.insert("utilization".to_string(), qty_to_json(&qtys.utilization));
            obj.insert(
                "utilization_percentage".to_string(),
                percentage_to_json(&qtys.utilization, &qtys.allocatable),
            );
        }
        obj.insert("requested".to_string(), qty_to_json(&qtys.requested));
        obj.insert(
            "requested_percentage".to_string(),
            percentage_to_json(&qtys.requested, &qtys.allocatable),
        );
        obj.insert("limit".to_string(), qty_to_json(&qtys.limit));
        obj.insert(
            "limit_percentage".to_string(),
            percentage_to_json(&qtys.limit, &qtys.allocatable),
        );
        obj.insert("allocatable".to_string(), qty_to_json(&qtys.allocatable));
        obj.insert("free".to_string(), qty_to_json(ofree));
        rows.push(Value::Object(obj));
    }
    Value::Array(rows)
}

fn qty_to_json(oqty: &Option<Qty>) -> serde_json::Value {
    oqty.as_ref()
        .and_then(|qty| serde_json::Number::from_f64(f64::from(qty)))
        .map(serde_json::Value::Number)
        .unwrap_or(serde_json::Value::Null)
}

fn percentage_to_json(oqty: &Option<Qty>, o100: &Option<Qty>) -> serde_json::Value {
    match (oqty, o100) {
        (Some(qty), Some(q100)) => serde_json::Number::from_f64(qty.calc_percentage(q100))
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        _ => serde_json::Value::Null,
    }
}

fn csv_escape(s: &str) -> String {
    if s.starts_with(['=', '+', '-', '@']) || s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn add_cells_for_csv(row: &mut Vec<String>, oqty: &Option<Qty>, o100: &Option<Qty>) {
    match oqty {
        None => {
            row.push("".to_string());
            row.push("".to_string());
        }
        Some(qty) => {
            row.push(format!("{:.2}", f64::from(qty)));
            row.push(match o100 {
                None => "".to_string(),
                Some(q100) => format!("{:.0}%", qty.calc_percentage(q100)),
            });
        }
    };
}

fn is_empty(oqty: &Option<Qty>) -> bool {
    match oqty {
        Some(qty) => qty.is_zero(),
        None => true,
    }
}

fn is_full_zero(qtys: &QtyByQualifier) -> bool {
    qtys.utilization.is_none()
        && is_empty(&qtys.requested)
        && is_empty(&qtys.limit)
        && is_empty(&qtys.allocatable)
}

#[cfg(not(feature = "prettytable"))]
pub fn display_with_prettytable(
    _data: &[(Vec<String>, Option<QtyByQualifier>, Option<Qty>)],
    _filter_full_zero: bool,
    _show_utilization: bool,
) {
    warn!("feature 'prettytable' not enabled");
}

#[cfg(feature = "prettytable")]
pub fn display_with_prettytable(
    data: &[(Vec<String>, Option<QtyByQualifier>, Option<Qty>)],
    filter_full_zero: bool,
    show_utilization: bool,
) {
    let mut table = Table::new();
    let format = format::FormatBuilder::new()
        .separators(&[], format::LineSeparator::new('-', '+', '+', '+'))
        .padding(1, 1)
        .build();
    table.set_format(format);
    let mut row_titles = row![bl->"Resource", br->"Utilization", br->"Requested", br->"Limit", br->"Allocatable", br->"Free"];
    if !show_utilization {
        row_titles.remove_cell(1);
    }
    table.set_titles(row_titles);

    let data2 = data
        .iter()
        .filter(|d| !filter_full_zero || !d.1.as_ref().map(is_full_zero).unwrap_or(false))
        .collect::<Vec<_>>();
    let prefixes = tree::provide_prefix(&data2, |parent, item| parent.0.len() + 1 == item.0.len());

    for ((k, oqtys, ofree), prefix) in data2.iter().zip(prefixes.iter()) {
        let name = k.last().map(|x| x.as_str()).unwrap_or("???");
        let column0 = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{} {}", prefix, name)
        };
        if let Some(qtys) = oqtys {
            let style = if qtys.requested > qtys.limit
                || qtys.utilization > qtys.limit
                || is_empty(&qtys.requested)
                || is_empty(&qtys.limit)
            {
                "rFy"
            } else {
                "rFg"
            };
            let mut row = Row::new(vec![
                Cell::new(&column0),
                make_cell_for_prettytable(&qtys.utilization, &qtys.allocatable).style_spec(style),
                make_cell_for_prettytable(&qtys.requested, &qtys.allocatable).style_spec(style),
                make_cell_for_prettytable(&qtys.limit, &qtys.allocatable).style_spec(style),
                make_cell_for_prettytable(&qtys.allocatable, &None).style_spec(style),
                make_cell_for_prettytable(ofree, &None).style_spec(style),
            ]);
            if !show_utilization {
                row.remove_cell(1);
            }
            table.add_row(row);
        } else {
            table.add_row(Row::new(vec![Cell::new(&column0)]));
        }
    }

    table.printstd();
}

#[cfg(feature = "prettytable")]
fn make_cell_for_prettytable(oqty: &Option<Qty>, o100: &Option<Qty>) -> Cell {
    let txt = match oqty {
        None => "__".to_string(),
        Some(qty) => match o100 {
            None => format!("{}", qty.adjust_scale()),
            Some(q100) => format!("({:.0}%) {}", qty.calc_percentage(q100), qty.adjust_scale()),
        },
    };
    Cell::new(&txt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qty::Qty;

    fn qty(s: &str) -> Qty {
        s.parse().unwrap()
    }

    #[test]
    fn to_json_value_mirrors_rows_with_numbers_and_nulls() {
        let group_by = vec![GroupBy::Resource, GroupBy::Node, GroupBy::Pod];
        let data = vec![
            // node level: requested + limit + allocatable known, so percentages are present
            (
                vec!["cpu".to_string(), "node-a".to_string()],
                Some(QtyByQualifier {
                    requested: Some(qty("1000m")),
                    limit: Some(qty("2000m")),
                    allocatable: Some(qty("4000m")),
                    ..Default::default()
                }),
                Some(qty("2000m")),
            ),
            // pod level: no allocatable, so percentages must be null; a row without
            // quantities is skipped entirely
            (
                vec!["cpu".to_string(), "node-a".to_string(), "pod-x".to_string()],
                Some(QtyByQualifier {
                    requested: Some(qty("500m")),
                    ..Default::default()
                }),
                None,
            ),
            (vec!["cpu".to_string(), "node-b".to_string()], None, None),
        ];

        let value = to_json_value(&data, &group_by, false);
        let arr = value.as_array().unwrap();
        assert_eq!(arr.len(), 2);

        let node = &arr[0];
        assert_eq!(node["kind"], "node");
        assert_eq!(node["resource"], "cpu");
        assert_eq!(node["node"], "node-a");
        assert_eq!(node["pod"], serde_json::Value::Null);
        assert_eq!(node["requested"].as_f64().unwrap(), 1.0);
        assert_eq!(node["requested_percentage"].as_f64().unwrap(), 25.0);
        assert_eq!(node["limit"].as_f64().unwrap(), 2.0);
        assert_eq!(node["allocatable"].as_f64().unwrap(), 4.0);
        assert_eq!(node["free"].as_f64().unwrap(), 2.0);
        // utilization not requested, so its keys are absent
        assert!(node.get("utilization").is_none());

        let pod = &arr[1];
        assert_eq!(pod["kind"], "pod");
        assert_eq!(pod["pod"], "pod-x");
        assert_eq!(pod["requested_percentage"], serde_json::Value::Null);
        assert_eq!(pod["allocatable"], serde_json::Value::Null);
        assert_eq!(pod["free"], serde_json::Value::Null);
    }

    #[test]
    fn to_json_value_includes_utilization_when_shown() {
        let group_by = vec![GroupBy::Resource, GroupBy::Node];
        let data = vec![(
            vec!["cpu".to_string(), "node-a".to_string()],
            Some(QtyByQualifier {
                utilization: Some(qty("1000m")),
                allocatable: Some(qty("2000m")),
                ..Default::default()
            }),
            None,
        )];

        let value = to_json_value(&data, &group_by, true);
        let row = &value.as_array().unwrap()[0];
        assert_eq!(row["utilization"].as_f64().unwrap(), 1.0);
        assert_eq!(row["utilization_percentage"].as_f64().unwrap(), 50.0);
    }
}
