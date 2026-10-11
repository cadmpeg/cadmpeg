// SPDX-License-Identifier: Apache-2.0
//! Coil section, extent, and result projection.

use super::{design_length, design_positive_length, feature_stream_matches};
use crate::design::face_resolve::design_angle;
use crate::ids;
use crate::layout::coil_long_scope_fixed_prologue as coil_long;
use crate::records::{
    feature::{
        coil::{DesignCoilExtent, DesignCoilSection, DesignCoilSectionPlacement},
        extrude::DesignExtrudeOperation,
        scope::DesignParameterScope,
    },
    parameters::DesignParameter,
    topology::{
        construction::DesignConstructionOperandGroup, extrude_selection::DesignOperandRole,
    },
};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};

pub(super) fn project_coil(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    construction_groups: &[DesignConstructionOperandGroup],
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        BodySelection, CoilConstruction, CoilExtent, CoilPlacement, CoilResult, CoilSection,
        CoilSectionPlacement, FeatureDefinition, FeatureOperation,
    };

    if !matches!(parameters.len(), 4 | 5) {
        return Ok(None);
    }
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
            || parameters
                .iter()
                .any(|(_, parameter)| !expected_parameter_kinds.contains(&parameter.source_kind()))
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
        Some((
            diameter,
            extent,
            taper,
            section,
            section_placement,
            operation,
        ))
    })();
    let Some((diameter, extent, taper, section, section_placement, operation)) = parsed else {
        return Ok(None);
    };
    let stream = or_none!(ids::native_stream_charged(
        ctx,
        &scope.id,
        "f3d Coil scope stream"
    )?);
    let first_body_group = if operation == DesignExtrudeOperation::NewBody {
        // The long Coil form carries one role-4 construction group for its
        // generated body even when the result is a new body. It is not a
        // Boolean target and must not suppress the typed result.
        None
    } else {
        let expected_role = if scope.coil_operation_offset()
            == scope
                .byte_offset()
                .checked_add(u64_from_index(coil_long::OPERATION))
        {
            DesignOperandRole::BODIES_A
        } else {
            DesignOperandRole::BODIES_B
        };
        let mut source = construction_groups.iter();
        let mut body_groups = || {
            ctx.find_by(
                &mut source,
                |group| {
                    Ok(feature_stream_matches(
                        ctx,
                        &group.id,
                        Some(stream),
                        "f3d Coil body group stream",
                    )? && group.scope_record_index == scope.record_index
                        && group.role() == expected_role)
                },
                "f3d Coil body group source",
            )
        };
        let first_body_group = body_groups()?;
        if body_groups()?.is_some() {
            return Ok(None);
        }
        first_body_group
    };
    let result = match (operation, first_body_group) {
        (DesignExtrudeOperation::NewBody, None) => CoilResult::NewBody {},
        (operation, Some(group)) => CoilResult::Boolean {
            operation: match operation {
                DesignExtrudeOperation::Join => cadmpeg_ir::features::BooleanKind::Join,
                DesignExtrudeOperation::Cut => cadmpeg_ir::features::BooleanKind::Cut,
                DesignExtrudeOperation::Intersect => cadmpeg_ir::features::BooleanKind::Intersect,
                DesignExtrudeOperation::NewBody => return Ok(None),
            },
            targets: BodySelection::Native(
                ctx.copy_retained_text(&group.id, "f3d Coil Boolean target group id")?,
            ),
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
            native_ref: match cadmpeg_ir::features::SelectionReference::try_from(
                ctx.copy_retained_text(&scope.id, "f3d Coil native placement id")?,
            ) {
                Ok(reference) => reference,
                Err(_) => return Ok(None),
            },
        },
    };
    let clockwise = or_none!(scope.coil_clockwise());
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

#[cfg(test)]
#[allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]
mod tests;
