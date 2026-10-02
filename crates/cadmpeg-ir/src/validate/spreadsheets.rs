// SPDX-License-Identifier: Apache-2.0
//! Spreadsheet reference and layout validation.

use std::collections::{HashMap, HashSet};

use super::{record_finding, CadIr, Finding};
use crate::report::check::Check;

pub(super) fn check_spreadsheets(ctx: &cadmpeg_core::decode::DecodeContext<'_>, ir: &CadIr, findings: &mut Vec<Finding>) -> Result<(), cadmpeg_core::CodecError> {
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
            record_finding(ctx, findings, Check::ReferentialIntegrity, crate::report::Severity::Error, Some(sheet.id.as_str()), format_args!("{}", "spreadsheet feature does not resolve"))?;
        }
        for cell in sheet.cells() {
            let Some(parameter) = parameters.get(&cell.parameter) else {
                record_finding(ctx, findings, Check::ReferentialIntegrity, crate::report::Severity::Error, Some(sheet.id.as_str()), format_args!("{}", "spreadsheet cell does not resolve"))?;
                continue;
            };
            if parameter.owner.as_ref() != Some(&sheet.feature) {
                record_finding(ctx, findings, Check::ReferentialIntegrity, crate::report::Severity::Error, Some(sheet.id.as_str()), format_args!("{}", "spreadsheet cell has a different owner"))?;
            }
        }
    }
    Ok(())
}
