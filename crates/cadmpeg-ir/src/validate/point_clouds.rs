// SPDX-License-Identifier: Apache-2.0
//! Point cloud references and per-point value counts.

use std::collections::BTreeSet;

use crate::document::CadIr;
use crate::report::check::{Check, Finding};
use crate::report::Severity;

use super::record_finding;

pub(super) fn check_point_clouds(ir: &CadIr, findings: &mut Vec<Finding>) {
    let model = &ir.model;
    let definitions: BTreeSet<&str> = model
        .product_definitions
        .iter()
        .map(|definition| definition.id.as_str())
        .collect();
    for cloud in &model.point_clouds {
        let id = Some(cloud.id.as_str());
        if let Some(definition) = &cloud.product_definition {
            if !definitions.contains(definition.as_str()) {
                record_finding(
                    findings,
                    Check::ReferentialIntegrity,
                    Severity::Error,
                    id,
                    format_args!("references missing product definition `{definition}`"),
                );
            }
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
