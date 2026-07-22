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
            let Some(parameter) = parameters.get(cell) else {
                spreadsheet_finding(findings, &sheet.id.0, "spreadsheet cell does not resolve");
                continue;
            };
            if !cells.insert(cell) {
                spreadsheet_finding(findings, &sheet.id.0, "spreadsheet repeats a cell identity");
            }
            if parameter.owner.as_ref() != Some(&sheet.feature) {
                spreadsheet_finding(
                    findings,
                    &sheet.id.0,
                    "spreadsheet cell has a different owner",
                );
            }
            let Some(address) = parameter.properties.get("address") else {
                spreadsheet_finding(findings, &sheet.id.0, "spreadsheet cell has no address");
                continue;
            };
            if cell_address(address).is_none() || !addresses.insert(address) {
                spreadsheet_finding(
                    findings,
                    &sheet.id.0,
                    "spreadsheet cell address is invalid or repeated",
                );
            }
        }
        check_dimensions(findings, &sheet.id.0, &sheet.column_widths, |name| {
            column_index(name).is_some()
        });
        check_dimensions(findings, &sheet.id.0, &sheet.row_heights, |name| {
            name.parse::<u32>().is_ok_and(|row| row > 0)
        });
        let mut ranges = Vec::new();
        for range in &sheet.merged_ranges {
            let Some(start) = cell_address(&range.start) else {
                spreadsheet_finding(findings, &sheet.id.0, "merged range start is invalid");
                continue;
            };
            let Some(end) = cell_address(&range.end) else {
                spreadsheet_finding(findings, &sheet.id.0, "merged range end is invalid");
                continue;
            };
            if start.0 > end.0
                || start.1 > end.1
                || start == end
                || !addresses.contains(&range.start)
            {
                spreadsheet_finding(findings, &sheet.id.0, "merged range is invalid");
                continue;
            }
            if ranges.iter().any(|other| overlaps(*other, (start, end))) {
                spreadsheet_finding(findings, &sheet.id.0, "merged ranges overlap");
            }
            ranges.push((start, end));
        }
    }
}

fn check_dimensions(
    findings: &mut Vec<Finding>,
    sheet: &str,
    dimensions: &[crate::spreadsheets::SpreadsheetDimension],
    valid_name: impl Fn(&str) -> bool,
) {
    let mut names = HashSet::new();
    for dimension in dimensions {
        if !valid_name(&dimension.name) || !names.insert(&dimension.name) {
            spreadsheet_finding(
                findings,
                sheet,
                "spreadsheet dimension is invalid or repeated",
            );
        }
    }
}

fn cell_address(value: &str) -> Option<(u32, u32)> {
    let split = value.find(|character: char| character.is_ascii_digit())?;
    let column = column_index(&value[..split])?;
    let row = value[split..].parse::<u32>().ok()?;
    (row > 0).then_some((row, column))
}

fn column_index(value: &str) -> Option<u32> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return None;
    }
    value.bytes().try_fold(0_u32, |index, byte| {
        index
            .checked_mul(26)?
            .checked_add(u32::from(byte - b'A' + 1))
    })
}

fn overlaps(left: ((u32, u32), (u32, u32)), right: ((u32, u32), (u32, u32))) -> bool {
    left.0 .0 <= right.1 .0
        && right.0 .0 <= left.1 .0
        && left.0 .1 <= right.1 .1
        && right.0 .1 <= left.1 .1
}

fn spreadsheet_finding(findings: &mut Vec<Finding>, entity: &str, message: &str) {
    findings.push(Finding {
        check: Check::ReferentialIntegrity,
        severity: Severity::Error,
        message: message.into(),
        entity: Some(entity.into()),
    });
}
