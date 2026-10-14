// SPDX-License-Identifier: Apache-2.0
//! Product-manufacturing information reference validation.

use super::record_finding;
use crate::document::CadIr;
use crate::index::identities::BorrowedIdentities;
use crate::pmi::{PmiDefinition, PmiTarget};
use crate::report::check::{Check, Finding};

pub(super) fn check_pmi(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), cadmpeg_core::CodecError> {
    let definitions = BorrowedIdentities::build(ctx, |add| {
        for annotation in &ir.model.pmi {
            add(annotation.id.as_str(), &annotation.definition)?;
        }
        Ok(())
    })?;
    macro_rules! typed_identities {
        ($arena:ident) => {
            BorrowedIdentities::build(ctx, |add| {
                for item in &ir.model.$arena {
                    add(item.id.as_str(), ())?;
                }
                Ok(())
            })?
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
        for target in &annotation.targets {
            let resolved = match target {
                PmiTarget::Body { body } => bodies.contains(ctx, body.as_str())?,
                PmiTarget::Face { face } => faces.contains(ctx, face.as_str())?,
                PmiTarget::Edge { edge } => edges.contains(ctx, edge.as_str())?,
                PmiTarget::Vertex { vertex } => vertices.contains(ctx, vertex.as_str())?,
                PmiTarget::Point { point } => points.contains(ctx, point.as_str())?,
                PmiTarget::Curve { curve } => curves.contains(ctx, curve.as_str())?,
                PmiTarget::Product { product } => products.contains(ctx, product.as_str())?,
                PmiTarget::Occurrence { occurrence } => {
                    occurrences.contains(ctx, occurrence.as_str())?
                }
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
                        definitions.get(ctx, reference.datum.as_str())?.copied(),
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
            PmiDefinition::GeometricTolerance { datum_system, .. } => {
                if let Some(id) = datum_system {
                    if !matches!(
                        definitions.get(ctx, id.as_str())?.copied(),
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
                    if !definitions.contains(ctx, id.as_str())? {
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
    Ok(())
}
