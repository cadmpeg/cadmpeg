// SPDX-License-Identifier: Apache-2.0
//! Spreadsheet reference and layout validation.

use super::{identities::BorrowedIdentities, record_finding, CadIr, Finding};
use crate::report::check::Check;

pub(super) fn check_spreadsheets(ctx: &cadmpeg_core::decode::DecodeContext<'_>, ir: &CadIr, findings: &mut Vec<Finding>) -> Result<(), cadmpeg_core::CodecError> {
    let features = BorrowedIdentities::build(ctx, |add| {
        for feature in &ir.model.features { add(feature.id.as_str(), ())?; }
        Ok(())
    })?;
    let parameters = BorrowedIdentities::build(ctx, |add| {
        for parameter in &ir.model.parameters { add(parameter.id.as_str(), parameter)?; }
        Ok(())
    })?;
    for sheet in &ir.model.spreadsheets {
        ctx.charge_work(1, "spreadsheet row scan")?;
        if !features.contains(ctx, sheet.feature.as_str())? {
            record_finding(ctx, findings, Check::ReferentialIntegrity, crate::report::Severity::Error, Some(sheet.id.as_str()), format_args!("{}", "spreadsheet feature does not resolve"))?;
        }
        for cell in sheet.cells() {
            ctx.charge_work(1, "spreadsheet cell scan")?;
            let Some(parameter) = parameters.get(ctx, cell.parameter.as_str())? else {
                record_finding(ctx, findings, Check::ReferentialIntegrity, crate::report::Severity::Error, Some(sheet.id.as_str()), format_args!("{}", "spreadsheet cell does not resolve"))?;
                continue;
            };
            ctx.charge_work(cadmpeg_core::decode::u64_from_index(sheet.feature.as_str().len()), "compare spreadsheet parameter owner")?;
            if parameter.owner.as_ref() != Some(&sheet.feature) {
                record_finding(ctx, findings, Check::ReferentialIntegrity, crate::report::Severity::Error, Some(sheet.id.as_str()), format_args!("{}", "spreadsheet cell has a different owner"))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check_spreadsheets;
    use crate::document::CadIr;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    fn parameter_fixture() -> CadIr {
        let mut ir = CadIr::empty();
        ir.model.parameters.push(serde_json::from_value(serde_json::json!({
            "id": "test:model:parameter#cell", "name": "cell", "expression": "7"
        })).unwrap());
        ir
    }

    #[test]
    fn spreadsheet_indexes_preserve_resource_refusals_and_release_storage() {
        let ir = parameter_fixture();
        for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems,
            ResourceDimension::WorkUnits] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                _ => unreachable!(),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut findings = Vec::new();
            let Err(CodecError::ResourceLimit(limit)) = check_spreadsheets(&ctx, &ir, &mut findings) else { panic!("spreadsheet index must refuse"); };
            assert_eq!(limit.dimension, dimension);
            assert!(findings.is_empty());
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 8192;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut findings = Vec::new();
        check_spreadsheets(&ctx, &ir, &mut findings).unwrap();
        assert!(findings.is_empty());
        drop(ctx.reserve_scoped(8192, "spreadsheet indexes released").unwrap());
        ctx.finish_session().unwrap();
    }

    #[test]
    fn spreadsheet_reference_findings_keep_row_order_and_missing_owner_semantics() {
        let mut ir = parameter_fixture();
        ir.model.spreadsheets.push(serde_json::from_value(serde_json::json!({
            "id": "test:model:spreadsheet#sheet", "feature": "test:model:feature#missing",
            "cells": [
                {"address": "A1", "parameter": "test:model:parameter#cell"},
                {"address": "A2", "parameter": "test:model:parameter#missing"}
            ]
        })).unwrap());
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut findings = Vec::new();
        check_spreadsheets(&ctx, &ir, &mut findings).unwrap();
        assert_eq!(findings.iter().map(|finding| finding.message.as_str()).collect::<Vec<_>>(), [
            "spreadsheet feature does not resolve", "spreadsheet cell has a different owner",
            "spreadsheet cell does not resolve"
        ]);
        for finding in findings {
            assert_eq!(finding.check, crate::report::check::Check::ReferentialIntegrity);
            assert_eq!(finding.entity.as_deref(), Some("test:model:spreadsheet#sheet"));
        }
        ctx.finish_session().unwrap();
    }
}
