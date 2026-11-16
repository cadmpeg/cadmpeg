// SPDX-License-Identifier: Apache-2.0
//! Solid primitive and coil projection.

use super::{design_length, design_positive_length};
use crate::design::face_resolve::design_angle;
use crate::ids::native_stream;
use crate::records::{
    feature::{
        coil::{DesignCoilExtent, DesignCoilSection, DesignCoilSectionPlacement},
        extrude::DesignExtrudeOperation,
        scope::DesignParameterScope,
    },
    parameters::DesignParameter,
    topology::construction::DesignConstructionOperandGroup,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};

pub(super) fn project_solid_primitive(
    scope: &DesignParameterScope,
    placements: &[crate::records::placement_construction::DesignPlacementConstruction],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, PrimitiveSolid, PrimitiveSolidKind,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::scalar::{Angle, Length};
    let operation = |operation| match operation {
        DesignExtrudeOperation::Join => cadmpeg_ir::features::BooleanOp::Join,
        DesignExtrudeOperation::Cut => cadmpeg_ir::features::BooleanOp::Cut,
        DesignExtrudeOperation::Intersect => cadmpeg_ir::features::BooleanOp::Intersect,
        DesignExtrudeOperation::NewBody => cadmpeg_ir::features::BooleanOp::NewBody,
    };
    Some(match &scope.payload() {
        crate::records::feature::scope::DesignScopePayload::BoxPrimitive(Some(
            crate::records::feature::primitives::DesignBoxPrimitive {
                length,
                width,
                height,
                offset_x,
                offset_y,
                transform,
                operation: result,
                ..
            },
        )) => {
            let (placement, support_assumption) = match transform {
                Some(frame) => {
                    let rows = frame.rows();
                    let row = |index: usize| {
                        let [x, y, z, translation] = rows[index];
                        [
                            x,
                            y,
                            z,
                            (translation + x * offset_x.get() + y * offset_y.get()) * 10.0,
                        ]
                    };
                    let local = cadmpeg_ir::transform::Transform::affine([row(0), row(1), row(2)])?;
                    let support = (|| {
                        let stream = native_stream(&scope.id)?;
                        let target = *scope
                            .reference_members()
                            .values()
                            .nth(scope.reference_members().len().checked_sub(7)?)?;
                        let mut candidates = placements.iter().filter(|placement| {
                            native_stream(&placement.id) == Some(stream)
                                && placement.record_index == u64::from(target)
                        });
                        let placement = candidates.next()?;
                        if candidates.next().is_some() {
                            return None;
                        }
                        let checked =
                            crate::design::decode::placement_construction::checked_solved_frame(
                                placement,
                            );
                        let flags_assumed = checked.is_none()
                            || placement.fields.inferred_x_axis.value != 0
                            || placement.fields.inferred_z_axis.value != 0;
                        let frame = checked
                            .map(|frame| frame.value)
                            .or_else(|| placement.fields.matrix.as_ref().map(|matrix| matrix.value))
                            .or_else(|| {
                                placement
                                    .fields
                                    .identity
                                    .as_ref()
                                    .filter(|identity| identity.value)
                                    .map(|_| crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY)
                            })?;
                        let rows = frame.rows();
                        let [x, y, z, _] = rows;
                        let support = cadmpeg_ir::transform::Transform::affine(
                            [x, y, z].map(|[x, y, z, translation]| [x, y, z, translation * 10.0]),
                        )?;
                        cadmpeg_ir::features::FeatureRigidPlacement::new(support)?;
                        Some((support.compose(local).ok()?, flags_assumed))
                    })();
                    match support {
                        Some((frame, false)) => (frame, None),
                        Some((frame, true)) => (frame, Some((
                            crate::loss::F3dLossCode::FeatureBoxSupportFlagsAssumed,
                            "uses its stored support frame despite unresolved placement flags",
                        ))),
                        None => (local, Some((
                            crate::loss::F3dLossCode::FeatureBoxSupportFrameAssumed,
                            "applies an identity support frame because its placement construction has no unique usable frame",
                        ))),
                    }
                }
                None => (
                    cadmpeg_ir::transform::Transform::affine([
                        [1.0, 0.0, 0.0, offset_x.get() * 10.0],
                        [0.0, 1.0, 0.0, offset_y.get() * 10.0],
                        [0.0, 0.0, 1.0, 0.0],
                    ])?,
                    None,
                ),
            };
            let definition = FeatureDefinition::Operation(FeatureOperation::Block {
                dimensions: Some([
                    cadmpeg_ir::scalar::PositiveLength::new(length.get() * 10.0)?,
                    cadmpeg_ir::scalar::PositiveLength::new(width.get() * 10.0)?,
                    cadmpeg_ir::scalar::PositiveLength::new(height.get() * 10.0)?,
                ]),
                placement: Some(cadmpeg_ir::features::FeatureRigidPlacement::new(placement)?),
                op: operation(*result),
            });
            if let Some((code, assumption)) = support_assumption {
                losses.push(code.note(format!("Box scope {} {assumption} (DR-123)", scope.id)));
            }
            if transform.is_some() && (offset_x.get() != 0.0 || offset_y.get() != 0.0) {
                losses.push(crate::loss::F3dLossCode::FeatureBoxOffsetFrameAssumed.note(format!(
                    "Box scope {} applies its planar offsets along the stored frame's local axes (DR-123)",
                    scope.id
                )));
            }
            definition
        }
        crate::records::feature::scope::DesignScopePayload::CylinderPrimitive(Some(
            crate::records::feature::primitives::DesignCylinderPrimitive {
                height,
                diameter,
                transform,
                operation: result,
                ..
            },
        )) => FeatureDefinition::Operation(FeatureOperation::Primitive {
            solid: PrimitiveSolid::new(PrimitiveSolidKind::Cylinder {
                radius: Length::new(diameter.get() * 5.0)?,
                height: Length::new(height.get() * 10.0)?,
                angle: Angle::FULL_TURN,
            })
            .ok()?,
            placement: match transform {
                Some(frame) => {
                    let rows = frame.value.rows();
                    let row = |index: usize| {
                        let [x, y, z, translation] = rows[index];
                        [x, y, z, translation * 10.0]
                    };
                    Some(cadmpeg_ir::features::FeatureRigidPlacement::new(
                        cadmpeg_ir::transform::Transform::affine([row(0), row(1), row(2)])?,
                    )?)
                }
                None => None,
            },
            op: operation(*result),
        }),
        crate::records::feature::scope::DesignScopePayload::SpherePrimitive(Some(
            crate::records::feature::primitives::DesignSpherePrimitive {
                transform,
                support_transform,
                diameter,
                operation: result,
                ..
            },
        )) => {
            let local = [transform[0][3], transform[1][3], transform[2][3]];
            let support = support_transform.map_or(
                crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
                |support| support.value,
            );
            let center = [0, 1, 2].map(|axis| {
                (support[axis][0] * local[0]
                    + support[axis][1] * local[1]
                    + support[axis][2] * local[2]
                    + support[axis][3])
                    * 10.0
            });
            FeatureDefinition::Operation(FeatureOperation::Sphere {
                center: cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                    center[0], center[1], center[2],
                ))?,
                radius: cadmpeg_ir::scalar::PositiveLength::new(diameter.get() * 5.0)?,
                op: operation(*result),
            })
        }
        crate::records::feature::scope::DesignScopePayload::TorusPrimitive(Some(
            crate::records::feature::primitives::DesignTorusPrimitive {
                transform,
                major_diameter,
                minor_diameter,
                operation: result,
                ..
            },
        )) => FeatureDefinition::Operation(FeatureOperation::Torus {
            center: cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                transform[0][3] * 10.0,
                transform[1][3] * 10.0,
                transform[2][3] * 10.0,
            ))?,
            axis: cadmpeg_ir::units::UnitVector3::new(Vector3::new(
                transform[0][2],
                transform[1][2],
                transform[2][2],
            ))?,
            major_radius: cadmpeg_ir::scalar::PositiveLength::new(major_diameter.get() * 5.0)?,
            minor_radius: cadmpeg_ir::scalar::PositiveLength::new(minor_diameter.get() * 5.0)?,
            op: operation(*result),
        }),
        _ => return None,
    })
}

pub(super) fn project_coil(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    construction_groups: &[DesignConstructionOperandGroup],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        BodySelection, CoilConstruction, CoilExtent, CoilPlacement, CoilSection,
        CoilSectionPlacement, FeatureDefinition, FeatureOperation, SolidFeatureResult,
    };

    let parsed = (|| {
        let unique = |kind: &str| {
            let mut matches = parameters.iter().filter_map(|(_, parameter)| {
                (parameter.source_kind() == kind).then_some(*parameter)
            });
            let parameter = matches.next()?;
            matches.next().is_none().then_some(parameter)
        };
        let diameter = design_positive_length(unique("Diameter")?)?;
        let section_size = design_positive_length(unique("SectionSize")?)?;
        let dimensionless = |kind: &str| {
            let parameter = unique(kind)?;
            parameter.unit().is_none().then_some(())?;
            cadmpeg_ir::scalar::PositiveReal::new(parameter.evaluated_value().get())
        };
        let (extent, taper, expected_parameter_kinds): (_, _, &[&str]) =
            match scope.coil_extent()? {
                DesignCoilExtent::RevolutionsHeight => (
                    CoilExtent::RevolutionsHeight {
                        revolutions: dimensionless("Revolutions")?,
                        height: design_length(unique("Height")?)?,
                    },
                    design_angle(unique("TaperAngle")?)?,
                    &[
                        "Diameter",
                        "SectionSize",
                        "TaperAngle",
                        "Revolutions",
                        "Height",
                    ],
                ),
                DesignCoilExtent::RevolutionsPitch => (
                    CoilExtent::RevolutionsPitch {
                        revolutions: dimensionless("Revolutions")?,
                        pitch: cadmpeg_ir::scalar::NonZeroLength::try_from(design_length(unique(
                            "Pitch",
                        )?)?)
                        .ok()?,
                    },
                    design_angle(unique("TaperAngle")?)?,
                    &[
                        "Diameter",
                        "SectionSize",
                        "TaperAngle",
                        "Revolutions",
                        "Pitch",
                    ],
                ),
                DesignCoilExtent::HeightPitch => (
                    CoilExtent::HeightPitch {
                        height: cadmpeg_ir::scalar::NonZeroLength::try_from(design_length(
                            unique("Height")?,
                        )?)
                        .ok()?,
                        pitch: cadmpeg_ir::scalar::NonZeroLength::try_from(design_length(unique(
                            "Pitch",
                        )?)?)
                        .ok()?,
                    },
                    design_angle(unique("TaperAngle")?)?,
                    &["Diameter", "SectionSize", "TaperAngle", "Height", "Pitch"],
                ),
                DesignCoilExtent::Spiral => (
                    CoilExtent::Spiral {
                        revolutions: dimensionless("Revolutions")?,
                        radial_pitch: cadmpeg_ir::scalar::NonZeroLength::try_from(design_length(
                            unique("Pitch")?,
                        )?)
                        .ok()?,
                    },
                    cadmpeg_ir::scalar::Angle::ZERO,
                    &["Diameter", "SectionSize", "Revolutions", "Pitch"],
                ),
            };
        if parameters.len() != expected_parameter_kinds.len()
            || parameters.iter().any(|(_, parameter)| {
                !expected_parameter_kinds.contains(&parameter.source_kind_str())
            })
        {
            return None;
        }
        let section = match scope.coil_section()? {
            DesignCoilSection::Circular => CoilSection::Circular {
                diameter: section_size,
            },
            DesignCoilSection::Square => CoilSection::Square { size: section_size },
            DesignCoilSection::ExternalTriangle => {
                CoilSection::ExternalTriangle { size: section_size }
            }
            DesignCoilSection::InternalTriangle => {
                CoilSection::InternalTriangle { size: section_size }
            }
        };
        let section_placement = match scope.coil_section_placement()? {
            DesignCoilSectionPlacement::Inside => CoilSectionPlacement::Inside,
            DesignCoilSectionPlacement::Center => CoilSectionPlacement::Center,
            DesignCoilSectionPlacement::Outside => CoilSectionPlacement::Outside,
        };
        let operation = scope.coil_operation()?;
        let stream = native_stream(&scope.id)?;
        let first_body_group = if operation == DesignExtrudeOperation::NewBody {
            // The long Coil form carries one role-4 construction group for its
            // generated body even when the result is a new body. It is not a
            // Boolean target and must not suppress the typed result.
            None
        } else {
            let mut body_groups = construction_groups.iter().filter(|group| {
                native_stream(&group.id) == Some(stream)
                    && group.scope_record_index == scope.record_index
                    && scope.coil_target_group_admits(group.role(), group.scope_reference_ordinal)
            });
            let first_body_group = body_groups.next();
            if body_groups.next().is_some() {
                return None;
            }
            first_body_group
        };
        Some((
            diameter,
            extent,
            taper,
            section,
            section_placement,
            operation,
            first_body_group,
        ))
    })();
    let Some((diameter, extent, taper, section, section_placement, operation, first_body_group)) =
        parsed
    else {
        return Ok(None);
    };
    let result = match (operation, first_body_group) {
        (DesignExtrudeOperation::NewBody, None) => SolidFeatureResult::NewBody {},
        (operation, Some(group)) => SolidFeatureResult::Boolean {
            operation: match operation {
                DesignExtrudeOperation::Join => cadmpeg_ir::features::BooleanKind::Join,
                DesignExtrudeOperation::Cut => cadmpeg_ir::features::BooleanKind::Cut,
                DesignExtrudeOperation::Intersect => cadmpeg_ir::features::BooleanKind::Intersect,
                DesignExtrudeOperation::NewBody => return Ok(None),
            },
            targets: BodySelection::Native(group.id.clone()),
        },
        _ => return Ok(None),
    };
    let placement = scope
        .coil_placement()
        .as_ref()
        .map(|placement| placement.transform())
        .or_else(|| {
            scope
                .coil_transform()
                .as_ref()
                .map(|transform| &transform.transform)
        })
        .and_then(|transform| {
            cadmpeg_ir::features::FeatureUnitPlaneFrame::new(
                Point3::new(
                    transform[0][3] * 10.0,
                    transform[1][3] * 10.0,
                    transform[2][3] * 10.0,
                ),
                Vector3::new(transform[0][2], transform[1][2], transform[2][2]),
                Vector3::new(transform[0][0], transform[1][0], transform[2][0]),
            )
        });
    let placement = match placement {
        Some(frame) => CoilPlacement::Explicit { frame },
        None => CoilPlacement::Native {
            native_ref: match cadmpeg_ir::features::SelectionReference::try_from(scope.id.clone()) {
                Ok(reference) => reference,
                Err(_) => return Ok(None),
            },
        },
    };
    let clockwise = or_none!(scope.coil_clockwise());
    if let Some(group) = first_body_group.filter(|group| group.members().len() != 1) {
        losses.push(crate::loss::F3dLossCode::CoilTargetMembersAssumed.note(format!(
            "Coil scope {} targets group {} with {} members; the group is read as one body selection.",
            scope.id,
            group.id,
            group.members().len()
        )));
    }
    Ok(Some(FeatureDefinition::Operation(FeatureOperation::Coil {
        construction: CoilConstruction {
            placement,
            diameter,
            extent,
            section,
            section_placement,
            clockwise,
            taper,
        },
        result,
    })))
}
