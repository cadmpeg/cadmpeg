// SPDX-License-Identifier: Apache-2.0
//! Product-manufacturing information reference validation.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::{Check, Finding, Severity};
use crate::document::CadIr;
use crate::pmi::{PmiDefinition, PmiTarget};

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
    let products = ir
        .model
        .products
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    let occurrences = ir
        .model
        .product_occurrences
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
                PmiTarget::Product { product } => products.contains(product.as_str()),
                PmiTarget::Occurrence { occurrence } => occurrences.contains(occurrence.as_str()),
                PmiTarget::ShapeAspect { source_id } => !source_id.is_empty(),
            };
            if !resolved {
                invalid(findings, annotation.id.as_str(), "unresolved PMI target");
            }
        }
        match &annotation.definition {
            PmiDefinition::DatumSystem { references } => {
                let mut compartments = BTreeMap::<u32, Vec<_>>::new();
                let mut common_groups = BTreeMap::new();
                for reference in references {
                    if !matches!(
                        definitions.get(reference.datum.as_str()),
                        Some(PmiDefinition::Datum { .. })
                    ) {
                        invalid(
                            findings,
                            annotation.id.as_str(),
                            "unresolved datum reference",
                        );
                    }
                    if reference.precedence == 0 {
                        invalid(findings, annotation.id.as_str(), "invalid datum precedence");
                    }
                    compartments
                        .entry(reference.precedence)
                        .or_default()
                        .push(reference);
                    if let Some(group) = reference.common_group {
                        if common_groups
                            .insert(group, reference.precedence)
                            .is_some_and(|precedence| precedence != reference.precedence)
                        {
                            invalid(
                                findings,
                                annotation.id.as_str(),
                                "common datum group spans precedence compartments",
                            );
                        }
                    }
                }
                for compartment in compartments.values() {
                    let common_group = compartment[0].common_group;
                    let common = common_group.is_some()
                        && compartment.len() >= 2
                        && compartment
                            .iter()
                            .all(|reference| reference.common_group == common_group);
                    if compartment.len() != 1 && !common
                        || compartment.len() == 1 && common_group.is_some()
                    {
                        invalid(findings, annotation.id.as_str(), "invalid datum precedence");
                    }
                }
            }
            PmiDefinition::GeometricTolerance {
                magnitude,
                datum_system,
                ..
            } => {
                if !(magnitude.value.is_finite() && magnitude.value >= 0.0) {
                    invalid(
                        findings,
                        annotation.id.as_str(),
                        "invalid tolerance magnitude",
                    );
                }
                if datum_system.as_ref().is_some_and(|id| {
                    !matches!(
                        definitions.get(id.as_str()),
                        Some(PmiDefinition::DatumSystem { .. })
                    )
                }) {
                    invalid(findings, annotation.id.as_str(), "unresolved datum system");
                }
            }
            PmiDefinition::Dimension {
                nominal,
                lower_deviation,
                upper_deviation,
                ..
            } => {
                if [nominal, lower_deviation, upper_deviation]
                    .into_iter()
                    .flatten()
                    .any(|value| !value.value.is_finite())
                {
                    invalid(
                        findings,
                        annotation.id.as_str(),
                        "non-finite dimension value",
                    );
                }
            }
            PmiDefinition::Presentation {
                semantics,
                placement,
                ..
            } => {
                if placement.is_some_and(|transform| !transform.is_finite()) {
                    invalid(
                        findings,
                        annotation.id.as_str(),
                        "presentation placement contains a non-finite coefficient",
                    );
                }
                if semantics.iter().any(|id| !ids.contains(id.as_str())) {
                    invalid(
                        findings,
                        annotation.id.as_str(),
                        "unresolved semantic annotation",
                    );
                }
            }
            PmiDefinition::Datum { .. } => {}
        }
    }
}

fn invalid(findings: &mut Vec<Finding>, entity: &str, message: &str) {
    findings.push(Finding {
        check: Check::Pmi,
        severity: Severity::Error,
        message: message.into(),
        entity: Some(entity.into()),
    });
}
