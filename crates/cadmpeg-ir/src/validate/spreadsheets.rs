// SPDX-License-Identifier: Apache-2.0
//! Spreadsheet reference and layout validation.

use super::{record_finding, CadIr, Finding};
use crate::index::identities::BorrowedIdentities;
use crate::report::check::Check;

pub(super) fn check_spreadsheets(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), cadmpeg_core::CodecError> {
    let features = BorrowedIdentities::build(ctx, |add| {
        for feature in &ir.model.features {
            add(feature.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let parameters = BorrowedIdentities::build(ctx, |add| {
        for parameter in &ir.model.parameters {
            add(parameter.id.as_str(), parameter)?;
        }
        Ok(())
    })?;
    for sheet in &ir.model.spreadsheets {
        if !features.contains(ctx, sheet.feature.as_str())? {
            record_finding(
                findings,
                Check::ReferentialIntegrity,
                crate::report::Severity::Error,
                Some(sheet.id.as_str()),
                format_args!("{}", "spreadsheet feature does not resolve"),
            );
        }
        for cell in sheet.cells() {
            let Some(parameter) = parameters.get(ctx, cell.parameter.as_str())? else {
                record_finding(
                    findings,
                    Check::ReferentialIntegrity,
                    crate::report::Severity::Error,
                    Some(sheet.id.as_str()),
                    format_args!("{}", "spreadsheet cell does not resolve"),
                );
                continue;
            };

            if parameter.owner.as_ref() != Some(&sheet.feature) {
                record_finding(
                    findings,
                    Check::ReferentialIntegrity,
                    crate::report::Severity::Error,
                    Some(sheet.id.as_str()),
                    format_args!("{}", "spreadsheet cell has a different owner"),
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check_spreadsheets;
    use crate::document::CadIr;

    fn parameter_fixture() -> CadIr {
        let mut ir = CadIr::empty();
        ir.model.parameters.push(
            serde_json::from_value(serde_json::json!({
                "id": "test:model:parameter#cell", "name": "cell", "expression": "7"
            }))
            .unwrap(),
        );
        ir
    }

    #[test]
    fn spreadsheet_reference_findings_keep_row_order_and_missing_owner_semantics() {
        let mut ir = parameter_fixture();
        ir.model.spreadsheets.push(
            serde_json::from_value(serde_json::json!({
                "id": "test:model:spreadsheet#sheet", "feature": "test:model:feature#missing",
                "cells": [
                    {"address": "A1", "parameter": "test:model:parameter#cell"},
                    {"address": "A2", "parameter": "test:model:parameter#missing"}
                ]
            }))
            .unwrap(),
        );
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut findings = Vec::new();
        check_spreadsheets(&ctx, &ir, &mut findings).unwrap();
        assert_eq!(
            findings
                .iter()
                .map(|finding| finding.message.as_str())
                .collect::<Vec<_>>(),
            [
                "spreadsheet feature does not resolve",
                "spreadsheet cell has a different owner",
                "spreadsheet cell does not resolve"
            ]
        );
        for finding in findings {
            assert_eq!(
                finding.check,
                crate::report::check::Check::ReferentialIntegrity
            );
            assert_eq!(
                finding.entity.as_deref(),
                Some("test:model:spreadsheet#sheet")
            );
        }
        ctx.finish_session().unwrap();
    }
}
