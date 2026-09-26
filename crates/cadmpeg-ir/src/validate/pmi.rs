// SPDX-License-Identifier: Apache-2.0
//! Product-manufacturing information reference validation.

use std::collections::{HashMap, HashSet};

use super::error_finding;
use crate::document::CadIr;
use crate::pmi::{PmiDefinition, PmiTarget};
use crate::report::check::{Check, Finding};

pub(super) fn check_pmi(ir: &CadIr, findings: &mut Vec<Finding>) {
    let ids = ir
        .model
        .pmi
        .iter()
        .map(|annotation| annotation.id.as_str())
        .collect::<HashSet<_>>();
    let definitions = ir
        .model
        .pmi
        .iter()
        .map(|annotation| (annotation.id.as_str(), &annotation.definition))
        .collect::<HashMap<_, _>>();
    let bodies = ir
        .model
        .bodies
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    let faces = ir
        .model
        .faces
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    let edges = ir
        .model
        .edges
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    let vertices = ir
        .model
        .vertices
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    let points = ir
        .model
        .points
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    let curves = ir
        .model
        .curves
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    let products = ir
        .model
        .product_definitions
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    let occurrences = ir
        .model
        .occurrences
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    for annotation in &ir.model.pmi {
        for target in &annotation.targets {
            let resolved = match target {
                PmiTarget::Body { body } => bodies.contains(body.as_str()),
                PmiTarget::Face { face } => faces.contains(face.as_str()),
                PmiTarget::Edge { edge } => edges.contains(edge.as_str()),
                PmiTarget::Vertex { vertex } => vertices.contains(vertex.as_str()),
                PmiTarget::Point { point } => points.contains(point.as_str()),
                PmiTarget::Curve { curve } => curves.contains(curve.as_str()),
                PmiTarget::Product { product } => products.contains(product.as_str()),
                PmiTarget::Occurrence { occurrence } => occurrences.contains(occurrence.as_str()),
                PmiTarget::ShapeAspect { .. } => true,
            };
            if !resolved {
                error_finding(
                    findings,
                    Check::Pmi,
                    annotation.id.as_str(),
                    "unresolved PMI target",
                );
            }
        }
        match &annotation.definition {
            PmiDefinition::DatumSystem { references } => {
                for reference in references.as_slice() {
                    if !matches!(
                        definitions.get(reference.datum.as_str()),
                        Some(PmiDefinition::Datum { .. })
                    ) {
                        error_finding(
                            findings,
                            Check::Pmi,
                            annotation.id.as_str(),
                            "unresolved datum reference",
                        );
                    }
                }
            }
            PmiDefinition::GeometricTolerance { datum_system, .. } => {
                if datum_system.as_ref().is_some_and(|id| {
                    !matches!(
                        definitions.get(id.as_str()),
                        Some(PmiDefinition::DatumSystem { .. })
                    )
                }) {
                    error_finding(
                        findings,
                        Check::Pmi,
                        annotation.id.as_str(),
                        "unresolved datum system",
                    );
                }
            }
            PmiDefinition::Dimension(_) => {}
            PmiDefinition::Presentation { semantics, .. } => {
                if semantics.iter().any(|id| !ids.contains(id.as_str())) {
                    error_finding(
                        findings,
                        Check::Pmi,
                        annotation.id.as_str(),
                        "unresolved semantic annotation",
                    );
                }
            }
            PmiDefinition::Datum { .. } => {}
            PmiDefinition::DatumTarget { .. } => {}
        }
    }
}
