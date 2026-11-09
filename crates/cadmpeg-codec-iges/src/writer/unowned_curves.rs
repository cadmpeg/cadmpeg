// SPDX-License-Identifier: Apache-2.0
//! Curves that no written entity has consumed.

use super::{
    append_curve_entity, carrier_display_color, compound, copious_curve, curve_reference_span,
    flatten_curve, mark_curve_descendants, ownership, BTreeMap, BTreeSet, BodyColor,
    BodyPresentation, CadIr, CodecError, CurveEntityRequest, Entity, EntityStatus, IgesLossCode,
    LossNote, Sense,
};

/// The writer state for curves that no written entity has consumed.
pub(super) struct UnownedCurves<'a, 'ir> {
    pub(super) ir: &'ir CadIr,
    pub(super) version: crate::IgesVersion,
    pub(super) ownership: &'a ownership::SourceOwnership<'ir>,
    pub(super) ignored: &'a BTreeSet<String>,
    pub(super) presentations: &'a mut BTreeMap<usize, BodyPresentation>,
}

/// Writes each curve that no written entity has consumed, in identity order,
/// and returns the identities of `ignored` curves skipped. A physically
/// dependent curve that no written entity consumes has no written parent: it
/// writes last, with its source status under a group parent, when no other
/// writer can write it, and is otherwise withheld with a loss.
pub(super) fn append(
    request: UnownedCurves<'_, '_>,
    entities: &mut Vec<Entity>,
    consumed: &mut BTreeSet<String>,
    losses: &mut Vec<LossNote>,
) -> Result<Vec<String>, CodecError> {
    let UnownedCurves {
        ir,
        version,
        ownership,
        ignored,
        presentations,
    } = request;
    let mut curves = ir.model.curves.iter().collect::<Vec<_>>();
    curves.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    let mut ignored_curves = Vec::new();
    let mut dependent = Vec::new();
    let mut output = UnownedCurveOutput {
        ir,
        version,
        entities,
        presentations,
        consumed,
        losses,
    };
    for curve in curves {
        if output.consumed.contains(curve.id.as_str()) {
            continue;
        }
        if ignored.contains(curve.id.as_str()) {
            ignored_curves.push(curve.id.to_string());
            continue;
        }
        let status = ownership.status(curve.source_object.as_ref());
        let owned = ownership.is_owned(curve.source_object.as_ref());
        if status.is_physically_dependent() && !owned {
            dependent.push((curve, status));
            continue;
        }
        output.append(
            curve,
            if owned && ownership.is_annotation_use(curve.source_object.as_ref()) {
                EntityStatus::AnnotationDependent
            } else {
                status
            },
        )?;
    }
    for procedural in &ir.model.procedural_curves {
        let cadmpeg_ir::geometry::ProceduralCurveDefinition::Offset(offset) =
            procedural.definition()
        else {
            continue;
        };
        let Some(owner) = ir.model.procedural_curve_owner(&procedural.id) else {
            continue;
        };
        if output.entities.iter().any(|entity| {
            entity.type_code == 130
                && entity
                    .source_targets
                    .iter()
                    .any(|target| target == owner.as_str())
        }) {
            output.consumed.insert(offset.source().to_string());
        }
    }
    for (curve, status) in dependent {
        if output.consumed.contains(curve.id.as_str()) {
            continue;
        }
        if matches!(status, EntityStatus::ParameterCurve) {
            // A face written after this pass can use the curve; the synthesis
            // writes it standalone only when none does.
            output
                .losses
                .push(copious_curve::withheld_parameter_curve_loss(&curve.id));
        } else if !ownership.writes_standalone(curve.id.as_str()) {
            ownership::report_unowned_support(output.losses, curve.id.as_str());
        } else {
            output.append(curve, status)?;
        }
    }
    Ok(ignored_curves)
}

/// The output state that `append_unowned_curves` writes each curve into.
struct UnownedCurveOutput<'a, 'ir> {
    ir: &'ir CadIr,
    version: crate::IgesVersion,
    entities: &'a mut Vec<Entity>,
    presentations: &'a mut BTreeMap<usize, BodyPresentation>,
    consumed: &'a mut BTreeSet<String>,
    losses: &'a mut Vec<LossNote>,
}

impl UnownedCurveOutput<'_, '_> {
    /// Writes `curve` with `status`, or omits it with a loss when its carrier
    /// has no IGES encoding.
    fn append(
        &mut self,
        curve: &cadmpeg_ir::geometry::Curve,
        status: EntityStatus,
    ) -> Result<(), CodecError> {
        let ir = self.ir;
        let geometry = match compound::geometry(ir, &curve.id) {
            Some(geometry) => geometry,
            None => flatten_curve(curve.geometry.solved().ok_or_else(|| {
                CodecError::InvalidInput("IGES curve carrier has no solved geometry".into())
            })?)?,
        };
        let span = curve
            .parameter_range
            .map(|_| curve_reference_span(ir, &curve.id, &geometry))
            .transpose()?;
        let before = self.entities.len();
        let written = append_curve_entity(
            self.entities,
            ir,
            CurveEntityRequest {
                version: self.version,
                curve_id: &curve.id,
                geometry: &geometry,
                span: span.as_ref(),
                sense: Sense::Forward,
                status,
                reference_offset: 0,
            },
        );
        match written {
            Ok(_) => self
                .losses
                .extend(super::edge_carriers::placement_applied_loss(curve)),
            Err(CodecError::InvalidInput(reason)) => {
                self.entities.truncate(before);
                self.losses.push(
                    IgesLossCode::WriterEntityOmitted
                        .note(format!("IGES curve {} is omitted: {reason}", curve.id)),
                );
                return Ok(());
            }
            Err(error) => return Err(error),
        }
        let color = carrier_display_color(
            ir,
            &cadmpeg_ir::appearance::AppearanceTarget::Curve(curve.id.clone()),
            &format!("curve {}", curve.id),
            self.losses,
        );
        if !matches!(color, BodyColor::Standard(0)) {
            self.presentations.insert(
                self.entities.len() - 1,
                BodyPresentation {
                    label: None,
                    name: None,
                    color,
                    visible: None,
                },
            );
        }
        mark_curve_descendants(ir, &curve.id, self.consumed, &mut BTreeSet::new())
    }
}
