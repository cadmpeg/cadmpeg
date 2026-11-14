use super::{
    design_length, design_length_unit, native_stream, ProjectInputs, DESIGN_LENGTH_TO_MILLIMETRE,
};
use crate::loss::F3dLossCode;
use crate::records::{feature::scope::DesignParameterScope, parameters::DesignParameter};
use cadmpeg_ir::{
    features::{
        sheet_metal::{SheetMetalContourFlangeConstruction, SheetMetalContourThicknessSide},
        ExtrudeExtent, ExtrudeSide, FeatureDefinition, FeatureOperation, LinearTermination,
        PlanarProfileRef,
    },
    report::loss::LossNote,
    scalar::{Length, NonNegativeLength, NonZeroLength, PositiveLength},
};

fn parameter_length(parameter: &DesignParameter) -> Option<(Length, bool)> {
    let known_unit = parameter
        .unit()
        .is_some_and(|unit| design_length_unit(unit.value.as_str()));
    let value = if known_unit {
        design_length(parameter)
    } else {
        Length::new(parameter.evaluated_value().get() * DESIGN_LENGTH_TO_MILLIMETRE)
    }?;
    Some((value, !known_unit))
}

pub(super) fn project(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    inputs: &ProjectInputs<'_>,
    losses: &mut Vec<LossNote>,
) -> FeatureDefinition {
    let mut construction = SheetMetalContourFlangeConstruction {
        profile: Some(PlanarProfileRef::Native(scope.id.clone())),
        side: Some(SheetMetalContourThicknessSide::Left),
        ..Default::default()
    };
    losses.push(F3dLossCode::ContourProfileNative.note(format!("ContourFlange scope {} retains its selected source-region profile program through the native scope and its ordered references.", scope.id)));
    losses.push(F3dLossCode::ContourDefaultSideOneLeftAssumed.note(format!("ContourFlange scope {} applies the default SideOne orientation as the profile-plane normal crossed with its ordered tangent; the stored profile control remains native.", scope.id)));
    let mut rule_radius_assumed = false;
    let operation = scope.contour_flange_operation();
    if operation.is_none_or(|operation| {
        operation.carrier.is_none()
            || operation.settings.is_none()
            || operation.profile_control.is_none()
    }) {
        losses.push(F3dLossCode::ContourEnvelopeUnresolved.note(format!(
            "ContourFlange scope {} retains unresolved typed construction-prefix links.",
            scope.id
        )));
    }
    if let Some(operation) = operation {
        construction.direction = operation
            .travel
            .as_ref()
            .map(|travel| travel.direction.value);
        if operation
            .travel
            .as_ref()
            .is_some_and(|travel| travel.owner_role_assumed)
        {
            losses.push(F3dLossCode::ContourVectorOwnerDistanceRoleAssumed.note(format!("ContourFlange scope {} reads the unique registered parameter-owner reference following a unit vector as AlongDistance while its owner frame and parameter definition are absent.", scope.id)));
        }
        if construction.direction.is_some() {
            losses.push(F3dLossCode::ContourOwnerVectorDirectionAssumed.note(format!("ContourFlange scope {} reads the unit vector immediately preceding its AlongDistance owner as travel direction.", scope.id)));
        }
        if operation.operation.value == 4 {
            losses.push(F3dLossCode::ContourOperationFourBodyAssumed.note(format!("ContourFlange scope {} reads operation word four as an independent sheet body in its owning component.", scope.id)));
        } else {
            losses.push(F3dLossCode::ContourResultUnresolved.note(format!(
                "ContourFlange scope {} retains unresolved result word {}.",
                scope.id, operation.operation.value
            )));
        }
        if let Some(rule) = &operation.rule {
            if rule.reference_assumed {
                losses.push(F3dLossCode::ContourSettingsRuleReferenceAssumed.note(format!("ContourFlange scope {} reads the unique registered rule {} before the exact owner-closing settings tail.", scope.id, rule.record_index)));
            }
            construction.thickness =
                PositiveLength::new(rule.thickness.value.get() * DESIGN_LENGTH_TO_MILLIMETRE);
            construction.inside_radius =
                NonNegativeLength::new(rule.first_value.value.get() * DESIGN_LENGTH_TO_MILLIMETRE);
            rule_radius_assumed = construction.inside_radius.is_some();
            construction.neutral_factor = Some(rule.material_scalar.value);
            losses.push(F3dLossCode::ContourMaterialScalarFactorAssumed.note(format!("ContourFlange scope {} reads the linked rule's material scalar as neutral-axis factor.", scope.id)));
        }
        if construction.thickness.is_none() && operation.inline_thickness.is_some() {
            construction.thickness = operation.inline_thickness.as_ref().and_then(|thickness| {
                PositiveLength::new(thickness.value.get() * DESIGN_LENGTH_TO_MILLIMETRE)
            });
            losses.push(F3dLossCode::ContourInlineThicknessAssumed.note(format!("ContourFlange scope {} reads the positive scalar immediately following the complete source-carrier reference as sheet thickness in database centimetres.", scope.id)));
        }
    }
    let wanted_owner = operation
        .and_then(|value| value.travel.as_ref())
        .map(|travel| travel.owner.value);
    let stream = native_stream(&scope.id);
    let matches_owner = |parameter: &DesignParameter| {
        wanted_owner.is_none_or(|wanted| {
            parameter.owner_record_index() == Some(wanted)
                || inputs.owners.iter().any(|owner| {
                    owner.record_index() == wanted
                        && owner.scope_record_index() == scope.record_index
                        && native_stream(owner.id()) == stream
                        && owner.parameter_record_index() == parameter.record_index
                })
        })
    };
    let mut candidates: Vec<_> = parameters
        .iter()
        .map(|(_, parameter)| *parameter)
        .filter(|parameter| parameter.source_kind() == "AlongDistance" && matches_owner(parameter))
        .collect();
    if let Some(owner) = wanted_owner {
        for parameter in inputs.native.iter().filter(|parameter| {
            parameter.source_kind() == "AlongDistance"
                && native_stream(&parameter.id) == stream
                && parameter.owner_record_index() == Some(owner)
        }) {
            if !candidates
                .iter()
                .any(|candidate| candidate.record_index == parameter.record_index)
            {
                candidates.push(parameter);
            }
        }
    }
    let distance = match candidates.as_slice() {
        [parameter] => parameter_length(parameter).map(|(value, unit_assumed)| {
            if unit_assumed {
                losses.push(F3dLossCode::ContourDistanceDatabaseUnitAssumed.note(format!("ContourFlange scope {} reads its AlongDistance evaluated scalar in database centimetres while its length unit is unresolved.", scope.id)));
            }
            value
        }),
        [] => {
            let mut owners = inputs.owners.iter().filter(|owner| {
                Some(owner.record_index()) == wanted_owner
                    && owner.scope_record_index() == scope.record_index
                    && native_stream(owner.id()) == stream
                    && !inputs.native.iter().any(|parameter| {
                        parameter.record_index == owner.parameter_record_index()
                            && native_stream(&parameter.id) == stream
                    })
            });
            let first = owners.next();
            if owners.next().is_none() {
                first.and_then(|owner| {
                    losses.push(F3dLossCode::ContourCachedOwnerDistanceAssumed.note(format!("ContourFlange scope {} reads the cached scalar of its vector-bearing owner {} as the missing AlongDistance definition in database centimetres.", scope.id, owner.record_index())));
                    Length::new(owner.evaluated_value().get() * DESIGN_LENGTH_TO_MILLIMETRE)
                })
            } else {
                None
            }
        }
        _ => None,
    };
    if let Some(length) = distance.and_then(|value| NonZeroLength::new(value.get())) {
        construction.extent = Some(ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Blind { length },
                draft: None,
            },
        });
        losses.push(F3dLossCode::ContourSingleDistanceOneSidedAssumed.note(format!("ContourFlange scope {} reads its signed AlongDistance as one-sided travel from the profile plane.", scope.id)));
    } else {
        losses.push(F3dLossCode::ContourDistanceUnresolved.note(format!(
            "ContourFlange scope {} retains an absent, zero or ambiguous travel distance.",
            scope.id
        )));
    }
    let mut radii = parameters.iter().filter_map(|(_, parameter)| {
        if parameter.source_kind() != "BendRadius" {
            return None;
        }
        let (value, unit_assumed) = parameter_length(parameter)?;
        Some((NonNegativeLength::new(value.get())?, unit_assumed))
    });
    let first_radius = radii.next();
    let multiple_radii = radii.next().is_some();
    if let Some((radius, unit_assumed)) = first_radius {
        construction.inside_radius = Some(radius);
        rule_radius_assumed = false;
        if unit_assumed {
            losses.push(F3dLossCode::ContourRadiusDatabaseUnitAssumed.note(format!("ContourFlange scope {} reads its BendRadius evaluated scalar in database centimetres while its length unit is unresolved.", scope.id)));
        }
        if multiple_radii {
            losses.push(F3dLossCode::ContourFirstRadiusOverrideAssumed.note(format!("ContourFlange scope {} uses the first BendRadius override in scope-reference order.", scope.id)));
        }
    }
    if rule_radius_assumed {
        losses.push(F3dLossCode::ContourRuleFirstRadiusAssumed.note(format!("ContourFlange scope {} reads the linked rule's first expression/value entry as inside bend radius in database centimetres.", scope.id)));
    }
    for (missing, code, member) in [
        (
            construction.direction.is_none(),
            F3dLossCode::ContourDirectionUnresolved,
            "travel direction",
        ),
        (
            construction.thickness.is_none(),
            F3dLossCode::ContourThicknessUnresolved,
            "sheet thickness",
        ),
        (
            construction.inside_radius.is_none(),
            F3dLossCode::ContourRadiusUnresolved,
            "inside bend radius",
        ),
        (
            construction.neutral_factor.is_none(),
            F3dLossCode::ContourRuleFactorUnresolved,
            "rule neutral-axis factor",
        ),
    ] {
        if missing {
            losses.push(code.note(format!(
                "ContourFlange scope {} retains unresolved {member}.",
                scope.id
            )));
        }
    }
    FeatureDefinition::Operation(FeatureOperation::SheetMetalContourFlange { construction })
}

#[cfg(test)]
mod tests;
