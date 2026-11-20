// SPDX-License-Identifier: Apache-2.0

use super::{design_length, design_positive_length};
use crate::design::face_resolve::design_angle;
use crate::ids::native_stream;
use crate::records::feature::coil::{
    DesignCoilExtent, DesignCoilSection, DesignCoilSectionPlacement,
};
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameter;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};

pub(super) fn project(
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
            None
        } else {
            let mut body_groups = construction_groups.iter().filter(|group| {
                native_stream(&group.id) == Some(stream)
                    && group.scope_record_index == scope.record_index
                    && scope.coil_target_group_admits(group.role(), group.scope_reference_ordinal)
            });
            let first_body_group = body_groups.next();
            if body_groups.next().is_some() {
                if !scope.coil_has_short_compact_prologue() {
                    return None;
                }
                None
            } else {
                first_body_group
            }
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
        (operation, group) if group.is_some() || scope.coil_has_short_compact_prologue() => {
            SolidFeatureResult::Boolean {
                operation: match operation {
                    DesignExtrudeOperation::Join => cadmpeg_ir::features::BooleanKind::Join,
                    DesignExtrudeOperation::Cut => cadmpeg_ir::features::BooleanKind::Cut,
                    DesignExtrudeOperation::Intersect => {
                        cadmpeg_ir::features::BooleanKind::Intersect
                    }
                    DesignExtrudeOperation::NewBody => return Ok(None),
                },
                targets: group.map_or(BodySelection::Unresolved, |group| {
                    BodySelection::Native(group.id.clone())
                }),
            }
        }
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
    if scope.coil_has_short_compact_prologue() {
        losses.push(crate::loss::F3dLossCode::CoilShortControlsAssumed.note(format!(
            "Coil scope {} reads the short form through the compact discriminator lanes despite unresolved control bytes (DR-800).",
            scope.id
        )));
    }
    if matches!(
        result,
        SolidFeatureResult::Boolean {
            targets: BodySelection::Unresolved,
            ..
        }
    ) {
        losses.push(crate::loss::F3dLossCode::CoilTargetUnresolved.note(format!(
            "Coil scope {} retains an unresolved Boolean target selection.",
            scope.id
        )));
    }
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
