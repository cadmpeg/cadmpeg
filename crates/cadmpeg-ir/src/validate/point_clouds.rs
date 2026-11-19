// SPDX-License-Identifier: Apache-2.0
//! Point cloud references and per-point value counts.

use std::collections::{BTreeMap, BTreeSet};

use crate::document::CadIr;
use crate::products::Storage;
use crate::report::check::{Check, Finding};
use crate::report::Severity;

use super::record_finding;

pub(super) fn check_point_clouds(ir: &CadIr, findings: &mut Vec<Finding>) {
    let model = &ir.model;
    let definitions: BTreeMap<&str, &Storage> = model
        .product_definitions
        .iter()
        .map(|definition| (definition.id.as_str(), &definition.storage))
        .collect();
    for cloud in &model.point_clouds {
        let id = Some(cloud.id.as_str());
        let definition = &cloud.definition;
        match definitions.get(definition.as_str()) {
            None => record_finding(
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                id,
                format_args!("references missing product definition `{definition}`"),
            ),
            Some(Storage::External(_)) => record_finding(
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                id,
                format_args!("is held by external product definition `{definition}`"),
            ),
            Some(_) => {}
        }
        let points = cloud.positions.len();
        if points == 0 {
            record_finding(
                findings,
                Check::PayloadIntegrity,
                Severity::Error,
                id,
                format_args!("a point cloud holds no point"),
            );
        }
        for (what, count) in [
            ("normals", cloud.normals.len()),
            ("colors", cloud.colors.len()),
        ] {
            if count != 0 && count != points {
                record_finding(
                    findings,
                    Check::PayloadIntegrity,
                    Severity::Error,
                    id,
                    format_args!("{count} {what} for {points} points"),
                );
            }
        }
        let mut names = BTreeSet::new();
        for channel in &cloud.channels {
            if !names.insert(channel.name.as_str()) {
                record_finding(
                    findings,
                    Check::PayloadIntegrity,
                    Severity::Error,
                    id,
                    format_args!("channel `{}` occurs twice", channel.name),
                );
            }
            if channel.values.len() != points {
                record_finding(
                    findings,
                    Check::PayloadIntegrity,
                    Severity::Error,
                    id,
                    format_args!(
                        "channel `{}` holds {} values for {points} points",
                        channel.name,
                        channel.values.len()
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
