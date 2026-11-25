// SPDX-License-Identifier: Apache-2.0
//! Product-manufacturing information reference validation.

mod nominals;
mod tolerances;

#[cfg(test)]
mod tests;

use super::record_finding;
use crate::document::CadIr;
use crate::index::identities::BorrowedIdentities;
use crate::pmi::{DimensionModifier, PmiDefinition, PmiTarget};
use crate::report::check::Ruled;
use crate::report::check::{Check, Finding};
use crate::report::Severity;
use nominals::BoundParameters;

fn error(findings: &mut Vec<Finding>, annotation: &str, message: Ruled<'_>) {
    record_finding(
        findings,
        Check::Pmi,
        Severity::Error,
        Some(annotation),
        message,
    );
}

pub(super) fn check_pmi(ir: &CadIr, findings: &mut Vec<Finding>) {
    if ir.model.pmi.is_empty() {
        return;
    }
    let bound_parameters = BoundParameters::new(ir);
    let definitions = BorrowedIdentities::build(|add| {
        for annotation in &ir.model.pmi {
            add(annotation.id.as_str(), &annotation.definition);
        }
    });
    macro_rules! typed_identities {
        ($home:ident, $arena:ident) => {
            BorrowedIdentities::build(|add| {
                for item in &ir.$home.$arena {
                    add(item.id.as_str(), ());
                }
            })
        };
    }
    let bodies = typed_identities!(geometry, bodies);
    let faces = typed_identities!(geometry, faces);
    let edges = typed_identities!(geometry, edges);
    let vertices = typed_identities!(geometry, vertices);
    let points = typed_identities!(geometry, points);
    let curves = typed_identities!(geometry, curves);
    let products = typed_identities!(model, product_definitions);
    let occurrences = typed_identities!(model, occurrences);
    for annotation in &ir.model.pmi {
        let zone_targets = match &annotation.definition {
            PmiDefinition::GeometricTolerance { zones, .. } => zones
                .iter()
                .flat_map(|zone| &zone.projections)
                .flat_map(crate::pmi::zones::ToleranceProjection::targets)
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        for target in annotation.targets.iter().chain(zone_targets) {
            let resolved = match &target.entity {
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
                    crate::rule!("pmi.unresolved-pmi-target", "{}", "unresolved PMI target"),
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
                            crate::rule!(
                                "pmi.unresolved-datum-reference",
                                "{}",
                                "unresolved datum reference"
                            ),
                        );
                    }
                }
            }
            PmiDefinition::GeometricTolerance {
                datum_system,
                magnitude,
                modifiers,
                displacement,
                maximum_upper,
                zones,
                ..
            } => {
                let invalid_maximum = maximum_upper.is_some_and(|maximum| {
                    maximum.get() <= magnitude.get().value.get()
                        || !modifiers.iter().any(|modifier| {
                            matches!(
                                modifier.as_str(),
                                "maximum_material_requirement" | "least_material_requirement"
                            )
                        })
                });
                let zone_stated =
                    displacement.is_some() || maximum_upper.is_some() || !zones.is_empty();
                if (zone_stated && magnitude.get().quantity != crate::pmi::PmiQuantity::Length)
                    || invalid_maximum
                {
                    record_finding(
                        findings,
                        Check::Pmi,
                        crate::report::Severity::Error,
                        Some(annotation.id.as_str()),
                        crate::rule!(
                            "pmi.inconsistent-geometric-tolerance-zone-constraints",
                            "inconsistent geometric tolerance-zone constraints"
                        ),
                    );
                }
                if zones
                    .iter()
                    .any(|zone| zone.form.other_names_a_typed_form())
                {
                    record_finding(
                        findings,
                        Check::Pmi,
                        crate::report::Severity::Error,
                        Some(annotation.id.as_str()),
                        crate::rule!(
                            "pmi.tolerance-zone-other-form-names-typed",
                            "tolerance zone's other form names a typed form"
                        ),
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
                            crate::rule!("pmi.unresolved-datum-system", "unresolved datum system"),
                        );
                    }
                }
            }
            PmiDefinition::Dimension(dimension) => {
                if dimension.nominal_disagrees_with_kind() {
                    record_finding(
                        findings,
                        Check::Pmi,
                        crate::report::Severity::Error,
                        Some(annotation.id.as_str()),
                        crate::rule!(
                            "pmi.dimension-nominal-quantity-disagrees-kind",
                            "dimension nominal quantity disagrees with its kind"
                        ),
                    );
                }
                if dimension
                    .modifiers
                    .iter()
                    .any(DimensionModifier::other_names_a_typed_modifier)
                {
                    record_finding(
                        findings,
                        Check::Pmi,
                        crate::report::Severity::Error,
                        Some(annotation.id.as_str()),
                        crate::rule!(
                            "pmi.dimension-other-modifier-names-typed-modifier",
                            "dimension's other modifier names a typed modifier"
                        ),
                    );
                }
                let id = annotation.id.as_str();
                nominals::check_bound_nominal(&bound_parameters, id, dimension, findings);
                tolerances::check_dimension_tolerance(id, dimension, findings);
                tolerances::check_dimension_statements(&bound_parameters, id, dimension, findings);
                if dimension.dimension == crate::pmi::PmiDimensionKind::DirectedLocation
                    && annotation.targets.len() != 2
                {
                    error(
                        findings,
                        id,
                        crate::rule!("pmi.directed-location-dimension-target-count",
                            "directed location dimension has {} targets; it has two, the origin and the target",
                            annotation.targets.len()
                        ),
                    );
                }
            }
            PmiDefinition::Presentation { semantics, .. } => {
                for id in semantics {
                    if !definitions.contains(id.as_str()) {
                        record_finding(
                            findings,
                            Check::Pmi,
                            crate::report::Severity::Error,
                            Some(annotation.id.as_str()),
                            crate::rule!(
                                "pmi.unresolved-semantic-annotation",
                                "unresolved semantic annotation"
                            ),
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
