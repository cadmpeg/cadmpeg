// SPDX-License-Identifier: Apache-2.0
//! Document-asset payload validation.

use super::{CadIr, Check, Finding, Severity};
use crate::assets::AssetContent;

pub(super) fn check_assets(ir: &CadIr, findings: &mut Vec<Finding>) {
    for asset in &ir.model.assets {
        let metadata_is_valid = asset.name.as_ref().is_none_or(|name| !name.is_empty())
            && asset
                .media_type
                .as_ref()
                .is_none_or(|media_type| !media_type.is_empty());
        let content_is_valid = match &asset.content {
            AssetContent::Embedded { data } => !data.is_empty(),
            AssetContent::External { uri } => !uri.is_empty(),
        };
        if !metadata_is_valid || !content_is_valid {
            findings.push(Finding {
                check: Check::PayloadIntegrity,
                severity: Severity::Error,
                message: "document asset has invalid metadata or empty content".into(),
                entity: Some(asset.id.0.clone()),
            });
        }
    }
}
