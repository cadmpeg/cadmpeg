// SPDX-License-Identifier: Apache-2.0
//! Spreadsheet reference and layout validation.

use std::collections::{HashMap, HashSet};

use super::{error_finding, CadIr, Finding};
use crate::report::check::Check;

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
            error_finding(
                findings,
                Check::ReferentialIntegrity,
                sheet.id.as_str(),
                "spreadsheet feature does not resolve",
            );
        }
        for cell in sheet.cells() {
            let Some(parameter) = parameters.get(&cell.parameter) else {
                error_finding(
                    findings,
                    Check::ReferentialIntegrity,
                    sheet.id.as_str(),
                    "spreadsheet cell does not resolve",
                );
                continue;
            };
            if parameter.owner.as_ref() != Some(&sheet.feature) {
                error_finding(
                    findings,
                    Check::ReferentialIntegrity,
                    sheet.id.as_str(),
                    "spreadsheet cell has a different owner",
                );
            }
        }
    }
}
