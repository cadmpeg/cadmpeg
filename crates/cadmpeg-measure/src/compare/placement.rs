// SPDX-License-Identifier: Apache-2.0
//! Whether a document's model-space bodies are its instances.
//!
//! The placement is trivial when the document has no occurrences, or when
//! every occurrence has a local prototype, unit scale and an effective
//! transform that is the identity, every product definition that lists
//! bodies is the prototype of exactly one occurrence, and no body is listed
//! by two definitions. Then each body is placed once, as in model space.

use std::collections::BTreeSet;

use cadmpeg_ir::products::PrototypeReference;
use cadmpeg_ir::CadIr;

use super::PlacementClass;
use crate::tolerance::SIMILARITY_RELATIVE_TOLERANCE;

/// The placement class of a document.
pub(super) fn class(ir: &CadIr) -> PlacementClass {
    if trivial(ir) {
        PlacementClass::Trivial
    } else {
        PlacementClass::Occurrences
    }
}

fn trivial(ir: &CadIr) -> bool {
    let model = &ir.model;
    if model.occurrences.is_empty() {
        return true;
    }
    let linear = ir.tolerances.linear.get();
    for occurrence in &model.occurrences {
        if !matches!(occurrence.prototype, PrototypeReference::Local { .. }) {
            return false;
        }
        if occurrence
            .scale
            .iter()
            .any(|scale| (scale.get() - 1.0).abs() > SIMILARITY_RELATIVE_TOLERANCE)
        {
            return false;
        }
        let Ok(transform) = occurrence.effective_transform() else {
            return false;
        };
        for (row, coefficients) in transform.affine_rows().iter().enumerate() {
            for (column, coefficient) in coefficients.iter().take(3).enumerate() {
                let identity = if row == column { 1.0 } else { 0.0 };
                if (coefficient - identity).abs() > SIMILARITY_RELATIVE_TOLERANCE {
                    return false;
                }
            }
            if coefficients[3].abs() > linear {
                return false;
            }
        }
    }
    let mut listed = BTreeSet::new();
    for definition in &model.product_definitions {
        if definition.bodies.is_empty() {
            continue;
        }
        let instances = model
            .occurrences
            .iter()
            .filter(|occurrence| {
                matches!(
                    &occurrence.prototype,
                    PrototypeReference::Local { definition: prototype } if *prototype == definition.id
                )
            })
            .count();
        if instances != 1 {
            return false;
        }
        for body in &definition.bodies {
            if !listed.insert(body.as_str()) {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests;
