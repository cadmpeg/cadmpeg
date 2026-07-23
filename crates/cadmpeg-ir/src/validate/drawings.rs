// SPDX-License-Identifier: Apache-2.0
//! Drawing graph and numeric validation.

use std::collections::HashSet;

use super::{Check, Finding, ModelIndex, Severity};
use crate::document::CadIr;

pub(super) fn check_drawings(ir: &CadIr, index: &ModelIndex<'_>, findings: &mut Vec<Finding>) {
    let mut orders = HashSet::new();
    for drawing in &ir.model.drawings {
        let refs_valid = index.contains(&drawing.object)
            && index.contains(&drawing.native_ref)
            && drawing
                .template
                .as_ref()
                .is_none_or(|id| index.contains(id))
            && drawing.assets.iter().all(|id| index.contains(id))
            && drawing.relationships.values().flatten().all(|target| {
                target.target.as_ref().is_none_or(|id| index.contains(id))
                    && (target.is_null
                        || target.target.is_some()
                        || (target.external_document.is_some() && target.external_object.is_some()))
            });
        let numeric_valid = drawing
            .position
            .iter()
            .flatten()
            .chain(drawing.direction.iter().flatten())
            .chain(drawing.rotation_degrees.iter())
            .chain(drawing.scale.iter())
            .all(|value| value.is_finite())
            && drawing.scale.is_none_or(|value| value > 0.0)
            && drawing.direction.is_none_or(|value| {
                value
                    .iter()
                    .map(|component| component * component)
                    .sum::<f64>()
                    > f64::EPSILON
            });
        let order_valid = orders.insert(drawing.order);
        if !refs_valid || !numeric_valid || !order_valid {
            findings.push(Finding {
                check: Check::ReferentialIntegrity,
                severity: Severity::Error,
                message: "invalid drawing reference, order, or numeric state".into(),
                entity: Some(drawing.id.0.clone()),
            });
        }
    }
}
