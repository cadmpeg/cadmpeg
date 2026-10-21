// SPDX-License-Identifier: Apache-2.0
//! Document scalar parameters authored from neutral values and expressions.

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{DesignParameter as NeutralParameter, ParameterValue};
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::F3dLossCode;
use crate::native::F3dNative;
use crate::records::parameters::{
    DesignParameter, DesignParameterContent, DesignParameterDiscriminator, DesignParameterOwner,
    DesignParameterOwnerContent, DesignParameterSource,
};

pub(super) fn regenerate(
    target: &mut CadIr,
    native: &mut F3dNative,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let original_native = std::mem::take(&mut native.design_parameters);
    let mut parameters = target.model.parameters.iter().collect::<Vec<_>>();
    parameters.sort_by(|left, right| (left.ordinal, &left.id).cmp(&(right.ordinal, &right.id)));
    for parameter in parameters {
        let original = original_native
            .iter()
            .find(|record| parameter.native_ref.as_deref() == Some(record.id.as_str()))
            .or_else(|| {
                let mut named = original_native
                    .iter()
                    .filter(|record| record.name() == parameter.name);
                let first = named.next()?;
                named.next().is_none().then_some(first)
            });
        let hint = original.and_then(|record| record.unit().map(|unit| unit.value.as_str()));
        let Some((unit, scalar)) = scalar(parameter, hint) else {
            losses.push(F3dLossCode::WriterDesignRecordsOmitted.note(format!(
                "parameter {} has no document-scalar F3D encoding",
                parameter.id,
            )));
            continue;
        };
        let expression = parameter
            .expression
            .render_with(|id| {
                target
                    .model
                    .parameters
                    .iter()
                    .find(|candidate| &candidate.id == id)
                    .map(|candidate| candidate.name.clone())
            })
            .ok_or_else(|| {
                CodecError::InvalidInput(format!(
                    "F3D parameter {} has a dangling expression reference",
                    parameter.id
                ))
            })?;
        let owned = parameter.owner.as_ref().and_then(|feature_id| {
            let feature = target
                .model
                .features
                .iter()
                .find(|feature| &feature.id == feature_id)?;
            let scope = native
                .design_parameter_scopes
                .iter()
                .find(|scope| feature.native_ref.as_deref() == Some(scope.id.as_str()))?;
            let record = original?;
            let owner = native.design_parameter_owners.iter().find(|owner| {
                Some(owner.record_index()) == record.owner_record_index()
                    && owner.scope_record_index() == Some(scope.record_index)
                    && owner.parameter_record_index() == record.record_index
            })?;
            Some((record, owner))
        });
        if (parameter.owner.is_some() && owned.is_none())
            || parameter.name.trim().is_empty()
            || expression.trim().is_empty()
        {
            losses.push(F3dLossCode::WriterDesignRecordsOmitted.note(format!(
                "parameter {} requires an unsupported owner or has no name/expression",
                parameter.id,
            )));
            continue;
        }
        // (O-capacity): indexed Design records and source ordinals are u32.
        let index = u32::try_from(native.design_parameters.len()).map_err(|_| {
            CodecError::InvalidInput("F3D document parameter count exceeds u32::MAX".into())
        })?;
        let record_index = index.checked_add(1).ok_or_else(|| {
            CodecError::InvalidInput("F3D document parameter index exceeds u32::MAX".into())
        })?;
        let generated = DesignParameter::authored(DesignParameterContent {
            id: owned.map_or_else(
                || format!("f3d:design:design-parameter#{record_index}"),
                |(record, _)| record.id.clone(),
            ),
            class_tag: match owned {
                Some((record, _)) => record.class_tag.clone(),
                None => "256".to_owned().try_into().map_err(CodecError::Malformed)?,
            },
            record_index: owned.map_or(record_index, |(record, _)| record.record_index),
            source_ordinal: owned.map_or(index, |(record, _)| record.source_ordinal),
            source: DesignParameterSource::authored(
                owned.map_or_else(
                    || "User Parameter".into(),
                    |(record, _)| record.source_kind_str().into(),
                ),
                owned.map(|(_, owner)| owner.record_index()),
                owned.map_or(Some(DesignParameterDiscriminator::Code0), |(record, _)| {
                    record.family_discriminator().map(|value| value.value)
                }),
            )
            .map_err(CodecError::Malformed)?,
            expression,
            unit,
            name: parameter.name.clone(),
            evaluated_value: scalar,
        })
        .map_err(CodecError::Malformed)?;
        if let Some((_, owner)) = owned {
            let replacement = DesignParameterOwner::try_from(DesignParameterOwnerContent {
                id: owner.id().clone(),
                class_tag: owner.class_tag().clone(),
                record_index: owner.record_index(),
                scope_record_index: owner.scope_record_index(),
                local_ordinal: owner.local_ordinal().map(|_| parameter.ordinal),
                evaluated_value: scalar,
                parameter_record_index: owner.parameter_record_index(),
                owned_ordinal: owner.owned_ordinal(),
                variant: owner.variant(),
                companion_record_index: owner.companion_record_index(),
            })
            .map_err(CodecError::Malformed)?;
            let owner_id = owner.id().clone();
            if let Some(slot) = native
                .design_parameter_owners
                .iter_mut()
                .find(|owner| owner.id() == &owner_id)
            {
                *slot = replacement;
            }
        }
        native.design_parameters.push(generated);
        if parameter.display.is_some()
            || parameter.pmi.is_some()
            || parameter.properties.keys().any(|key| {
                parameter.value.is_some() || !matches!(key.as_str(), "unit" | "evaluated_scalar")
            })
        {
            losses.push(F3dLossCode::WriterDesignRecordsOmitted.note(format!(
                "parameter {} keeps its value and expression; unsupported display, PMI or extension fields are omitted", parameter.id,
            )));
        }
    }
    if native.design_parameter_scopes.is_empty() {
        target.model.parameters = super::with_context(|ctx| {
            Ok(
                crate::design::feature_project::project_parameter_design_with_edge_identities(
                    ctx,
                    &crate::design::feature_project::ProjectInputs {
                        native: &native.design_parameters,
                        owners: &[],
                        scopes: &[],
                        timelines: &[],
                        construction_groups: &[],
                        fillet_radius_groups: &[],
                        edge_operands: &[],
                        edge_identity_operands: &[],
                        edge_treatment_vertex_operands: &[],
                        entity_selection_operands: &[],
                        curve_identities: &[],
                        face_operands: &[],
                        body_recipe_operands: &[],
                        legacy_loft_body_carriers: &[],
                        placements: &[],
                        body_bindings: &[],
                        component_naming_spaces: &[],
                        histories: &[],
                        face_boundaries: &[],
                    },
                )?
                .parameters,
            )
        })?;
    } else {
        native.design_parameter_owners.retain(|owner| {
            native
                .design_parameters
                .iter()
                .any(|parameter| parameter.owner_record_index() == Some(owner.record_index()))
        });
        native.design_parameter_companions.retain(|companion| {
            native
                .design_parameter_owners
                .iter()
                .any(|owner| owner.companion_record_index() == companion.record_index())
        });
        target.model.parameters = super::sketch::project(native)?.parameters;
    }
    Ok(())
}

fn scalar(
    parameter: &NeutralParameter,
    native_unit: Option<&str>,
) -> Option<(Option<String>, f64)> {
    let literal = if parameter.value.is_none() && native_unit.is_none() {
        // A unit-bearing literal states its quantity without native hints.
        // Try dimensionless first so scientific notation remains scalar.
        parameter
            .expression
            .literal_value_as(&ParameterValue::Real(cadmpeg_ir::scalar::FiniteReal::ZERO))
            .or_else(|| {
                parameter
                    .expression
                    .literal_value_as(&ParameterValue::Length(cadmpeg_ir::scalar::Length::ZERO))
            })
            .or_else(|| {
                parameter
                    .expression
                    .literal_value_as(&ParameterValue::Angle(cadmpeg_ir::scalar::Angle::ZERO))
            })
    } else if parameter.value.is_none() {
        let quantity = match native_unit {
            Some(unit) if crate::design::feature_project::design_length_unit(unit) => {
                ParameterValue::Length(cadmpeg_ir::scalar::Length::ZERO)
            }
            Some("deg" | "rad") => ParameterValue::Angle(cadmpeg_ir::scalar::Angle::ZERO),
            Some(_) => return None,
            None => ParameterValue::Real(cadmpeg_ir::scalar::FiniteReal::ZERO),
        };
        parameter.expression.literal_value_as(&quantity)
    } else {
        None
    };
    match parameter.value.as_ref().or(literal.as_ref()) {
        Some(ParameterValue::Length(value)) => Some((
            Some(
                native_unit
                    .filter(|unit| crate::design::feature_project::design_length_unit(unit))
                    .unwrap_or("mm")
                    .into(),
            ),
            value.get() / 10.0,
        )),
        Some(ParameterValue::Angle(value)) => Some((
            Some(
                native_unit
                    .filter(|unit| matches!(*unit, "deg" | "rad"))
                    .unwrap_or("rad")
                    .into(),
            ),
            value.get(),
        )),
        Some(ParameterValue::Real(value)) => Some((None, value.get())),
        Some(
            ParameterValue::Integer(_) | ParameterValue::Boolean(_) | ParameterValue::String(_),
        ) => None,
        None => None,
    }
}

#[cfg(test)]
mod tests;
