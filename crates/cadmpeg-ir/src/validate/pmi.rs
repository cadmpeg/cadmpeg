// SPDX-License-Identifier: Apache-2.0
//! Product-manufacturing information reference validation.

use super::record_finding;
use crate::document::CadIr;
use crate::index::identities::BorrowedIdentities;
use crate::pmi::{PmiDefinition, PmiTarget};
use crate::report::check::{Check, Finding};

pub(super) fn check_pmi(ir: &CadIr, findings: &mut Vec<Finding>) {
    let definitions = BorrowedIdentities::build(|add| {
        for annotation in &ir.model.pmi {
            add(annotation.id.as_str(), &annotation.definition);
        }
    });
    macro_rules! typed_identities {
        ($arena:ident) => {
            BorrowedIdentities::build(|add| {
                for item in &ir.model.$arena {
                    add(item.id.as_str(), ());
                }
            })
        };
    }
    let bodies = typed_identities!(bodies);
    let faces = typed_identities!(faces);
    let edges = typed_identities!(edges);
    let vertices = typed_identities!(vertices);
    let points = typed_identities!(points);
    let curves = typed_identities!(curves);
    let products = typed_identities!(product_definitions);
    let occurrences = typed_identities!(occurrences);
    for annotation in &ir.model.pmi {
        let zone_targets = match &annotation.definition {
            PmiDefinition::GeometricTolerance { constraints, .. } => constraints
                .zones
                .iter()
                .flat_map(|zone| &zone.projections)
                .flat_map(crate::pmi::zones::ToleranceProjection::targets)
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        for target in annotation.targets.iter().chain(zone_targets) {
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
                record_finding(
                    findings,
                    Check::Pmi,
                    crate::report::Severity::Error,
                    Some(annotation.id.as_str()),
                    format_args!("{}", "unresolved PMI target"),
                );
            }
        }
        match &annotation.definition {
            PmiDefinition::DatumSystem { references } => {
                for reference in references.as_slice() {
                    if !matches!(
                        definitions.get(reference.datum.as_str()).copied(),
                        Some(PmiDefinition::Datum { .. })
                    ) {
                        record_finding(
                            findings,
                            Check::Pmi,
                            crate::report::Severity::Error,
                            Some(annotation.id.as_str()),
                            format_args!("{}", "unresolved datum reference"),
                        );
                    }
                }
            }
            PmiDefinition::GeometricTolerance {
                datum_system,
                magnitude,
                modifiers,
                constraints,
                ..
            } => {
                let invalid_maximum = constraints.maximum.is_some_and(|maximum| {
                    maximum.get() <= magnitude.get().value.get()
                        || !modifiers.iter().any(|modifier| {
                            matches!(
                                modifier.as_str(),
                                "maximum_material_requirement" | "least_material_requirement"
                            )
                        })
                });
                if (!constraints.is_empty()
                    && magnitude.get().quantity != crate::pmi::PmiQuantity::Length)
                    || invalid_maximum
                {
                    record_finding(
                        findings,
                        Check::Pmi,
                        crate::report::Severity::Error,
                        Some(annotation.id.as_str()),
                        format_args!("inconsistent geometric tolerance-zone constraints"),
                    );
                }
                if let Some(id) = datum_system {
                    if !matches!(
                        definitions.get(id.as_str()).copied(),
                        Some(PmiDefinition::DatumSystem { .. })
                    ) {
                        record_finding(
                            findings,
                            Check::Pmi,
                            crate::report::Severity::Error,
                            Some(annotation.id.as_str()),
                            format_args!("unresolved datum system"),
                        );
                    }
                }
            }
            PmiDefinition::Dimension(_) => {}
            PmiDefinition::Presentation { semantics, .. } => {
                for id in semantics {
                    if !definitions.contains(id.as_str()) {
                        record_finding(
                            findings,
                            Check::Pmi,
                            crate::report::Severity::Error,
                            Some(annotation.id.as_str()),
                            format_args!("unresolved semantic annotation"),
                        );
                        break;
                    }
                }
            }
            PmiDefinition::Datum { .. } => {}
            PmiDefinition::DatumTarget { .. } => {}
        }
    }
}
