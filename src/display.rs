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
