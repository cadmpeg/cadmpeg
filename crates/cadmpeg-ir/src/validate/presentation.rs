// SPDX-License-Identifier: Apache-2.0
//! Presentation reference and numeric validation.

use std::collections::HashSet;

use crate::document::CadIr;
use crate::report::{Check, Finding, Severity};

pub(super) fn check_presentation(
    ir: &CadIr,
    all_ids: &HashSet<String>,
    findings: &mut Vec<Finding>,
) {
    if ir.model.presentation_documents.len() > 1 {
        invalid(findings, None, "multiple document presentation records");
    }
    for document in &ir.model.presentation_documents {
        let native_valid = document
            .native_ref
            .as_ref()
            .is_none_or(|native| all_ids.contains(native));
        let assets_valid = document
            .states
            .iter()
            .flat_map(|state| &state.assets)
            .all(|asset| all_ids.contains(asset));
        let orders = document
            .states
            .iter()
            .map(|state| state.order)
            .collect::<HashSet<_>>();
        let order_valid = orders.len() == document.states.len();
        let camera_valid = document.camera.as_ref().is_none_or(|camera| {
            let finite = camera
                .position
                .iter()
                .flatten()
                .chain(camera.orientation.iter().flatten())
                .all(|value| value.is_finite());
            let quaternion_valid = camera.orientation.is_none_or(|orientation| {
                orientation.iter().map(|value| value * value).sum::<f64>() > f64::EPSILON
            });
            finite && quaternion_valid
        });
        if !native_valid || !assets_valid || !order_valid || !camera_valid {
            invalid(
                findings,
                Some(document.id.0.clone()),
                "invalid document presentation state",
            );
        }
    }

    let mut orders = HashSet::new();
    for view in &ir.model.view_presentations {
        let references_valid = view
            .object
            .as_ref()
            .is_none_or(|object| all_ids.contains(object))
            && view
                .native_ref
                .as_ref()
                .is_none_or(|native| all_ids.contains(native));
        let sizes_valid = [view.line_width, view.point_size]
            .into_iter()
            .flatten()
            .all(|value| value.is_finite() && value >= 0.0);
        if !references_valid || !sizes_valid || !orders.insert(view.order) {
            invalid(
                findings,
                Some(view.id.0.clone()),
                "invalid view presentation reference, order, or size",
            );
        }
    }
}

fn invalid(findings: &mut Vec<Finding>, entity: Option<String>, message: &str) {
    findings.push(Finding {
        check: Check::ReferentialIntegrity,
        severity: Severity::Error,
        message: message.into(),
        entity,
    });
}
