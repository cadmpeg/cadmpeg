// SPDX-License-Identifier: Apache-2.0
//! Spreadsheet reference and layout validation.

use std::collections::{HashMap, HashSet};

use super::{CadIr, Check, Finding, Severity};

pub(super) fn check_spreadsheets(ir: &CadIr, findings: &mut Vec<Finding>) {
    let features = ir
        .model
        .features
        .iter()
        .map(|feature| &feature.id)
        .collect::<HashSet<_>>();
    let parameters = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    for sheet in &ir.model.spreadsheets {
        if !features.contains(&sheet.feature) {
            spreadsheet_finding(
                findings,
                &sheet.id.0,
                "spreadsheet feature does not resolve",
            );
        }
        let mut cells = HashSet::new();
        let mut addresses = HashSet::new();
        for cell in &sheet.cells {
            let Some(parameter) = parameters.get(&cell.parameter) else {
                spreadsheet_finding(findings, &sheet.id.0, "spreadsheet cell does not resolve");
                continue;
            };
            if !cells.insert(&cell.parameter) {
                spreadsheet_finding(findings, &sheet.id.0, "spreadsheet repeats a cell identity");
            }
            if parameter.owner.as_ref() != Some(&sheet.feature) {
                spreadsheet_finding(
                    findings,
                    &sheet.id.0,
                    "spreadsheet cell has a different owner",
                );
            }
            if !addresses.insert(cell.address) {
                spreadsheet_finding(
                    findings,
                    &sheet.id.0,
                    "spreadsheet cell address is invalid or repeated",
                );
            }
        }
        check_dimensions(findings, &sheet.id.0, &sheet.column_widths);
        check_dimensions(findings, &sheet.id.0, &sheet.row_heights);
        let mut ranges = Vec::new();
        for range in &sheet.merged_ranges {
            if !addresses.contains(&range.start()) {
                spreadsheet_finding(findings, &sheet.id.0, "merged range is invalid");
                continue;
            }
            let span = (range.start(), range.end());
            if ranges.iter().any(|other| overlaps(*other, span)) {
                spreadsheet_finding(findings, &sheet.id.0, "merged ranges overlap");
            }
            ranges.push(span);
        }
    }
}

fn check_dimensions(
    findings: &mut Vec<Finding>,
    sheet: &str,
    dimensions: &[crate::spreadsheets::SpreadsheetDimension],
) {
    let mut names = HashSet::new();
    for dimension in dimensions {
        if dimension.index == 0 || !names.insert(dimension.index) {
            spreadsheet_finding(
                findings,
                sheet,
                "spreadsheet dimension is invalid or repeated",
            );
        }
    }
}

fn overlaps(
    left: (
        crate::spreadsheets::CellAddress,
        crate::spreadsheets::CellAddress,
    ),
    right: (
        crate::spreadsheets::CellAddress,
        crate::spreadsheets::CellAddress,
    ),
) -> bool {
    left.0.row() <= right.1.row()
        && right.0.row() <= left.1.row()
        && left.0.col() <= right.1.col()
        && right.0.col() <= left.1.col()
}

fn spreadsheet_finding(findings: &mut Vec<Finding>, entity: &str, message: &str) {
    findings.push(Finding {
        check: Check::ReferentialIntegrity,
        severity: Severity::Error,
        message: message.into(),
        entity: Some(entity.into()),
    });
}
