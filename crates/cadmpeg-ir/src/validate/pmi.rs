// SPDX-License-Identifier: Apache-2.0
//! Product-manufacturing information reference validation.

use std::collections::HashSet;

use crate::document::CadIr;
use crate::pmi::{PmiDefinition, PmiTarget};
use crate::report::{Check, Finding, Severity};

pub(super) fn check_pmi(ir: &CadIr, findings: &mut Vec<Finding>) {
    let ids = ir
        .model
        .pmi
        .iter()
        .map(|annotation| annotation.id.as_str())
        .collect::<HashSet<_>>();
    for annotation in &ir.model.pmi {
        for target in &annotation.targets {
            let resolved = match target {
                PmiTarget::Body { body } => ir.model.bodies.iter().any(|item| item.id == *body),
                PmiTarget::Face { face } => ir.model.faces.iter().any(|item| item.id == *face),
                PmiTarget::Edge { edge } => ir.model.edges.iter().any(|item| item.id == *edge),
                PmiTarget::Vertex { vertex } => {
                    ir.model.vertices.iter().any(|item| item.id == *vertex)
                }
                PmiTarget::Product { product } => {
                    ir.model.products.iter().any(|item| item.id == *product)
                }
                PmiTarget::Occurrence { occurrence } => ir
                    .model
                    .occurrences
                    .iter()
                    .any(|item| item.id == *occurrence),
                PmiTarget::ShapeAspect { source_id } => !source_id.is_empty(),
            };
            if !resolved {
                invalid(findings, annotation.id.as_str(), "unresolved PMI target");
            }
        }
        match &annotation.definition {
            PmiDefinition::DatumSystem { references } => {
                let mut precedence = HashSet::new();
                for reference in references {
                    if !ids.contains(reference.datum.as_str()) {
                        invalid(
                            findings,
                            annotation.id.as_str(),
                            "unresolved datum reference",
                        );
                    }
                    if reference.precedence == 0 || !precedence.insert(reference.precedence) {
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
                if datum_system
                    .as_ref()
                    .is_some_and(|id| !ids.contains(id.as_str()))
                {
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
            PmiDefinition::Presentation { semantics, .. } => {
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
