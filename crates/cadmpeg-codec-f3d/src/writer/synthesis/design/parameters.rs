// SPDX-License-Identifier: Apache-2.0
//! Document scalar parameters authored from neutral values and expressions.

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{DesignParameter as NeutralParameter, ParameterValue};
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::F3dLossCode;
use crate::native::F3dNative;
use crate::records::parameters::{
    DesignParameter, DesignParameterContent, DesignParameterDiscriminator, DesignParameterSource,
};

pub(super) fn regenerate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    target: &mut CadIr,
    native: &mut F3dNative,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let original_native = std::mem::take(&mut native.design_parameters);
    let mut parameters = target.model.parameters.iter().collect::<Vec<_>>();
    parameters.sort_by(|left, right| (left.ordinal, &left.id).cmp(&(right.ordinal, &right.id)));
    for parameter in parameters {
        let hint = original_native
            .iter()
            .find(|record| parameter.native_ref.as_deref() == Some(record.id.as_str()))
            .or_else(|| {
                let mut named = original_native
                    .iter()
                    .filter(|record| record.name() == parameter.name);
                let first = named.next()?;
                named.next().is_none().then_some(first)
            })
            .and_then(|record| record.unit().map(|unit| unit.value.as_str()));
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
        if parameter.owner.is_some()
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
            id: format!("f3d:design:design-parameter#{record_index}"),
            class_tag: "256".to_owned().try_into().map_err(CodecError::Malformed)?,
            record_index,
            source_ordinal: index,
            source: DesignParameterSource::authored(
                "User Parameter".into(),
                None,
                Some(DesignParameterDiscriminator::Code0),
            )
            .map_err(CodecError::Malformed)?,
            expression,
            unit,
            name: parameter.name.clone(),
            evaluated_value: scalar,
        })
        .map_err(CodecError::Malformed)?;
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
    target.model.parameters =
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
        .parameters;
    Ok(())
}

fn scalar(
    parameter: &NeutralParameter,
    native_unit: Option<&str>,
) -> Option<(Option<String>, f64)> {
    match parameter.value.as_ref() {
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
        None => {
            let unit = parameter.properties.get("unit")?.clone();
            let value = parameter
                .properties
                .get("evaluated_scalar")?
                .parse::<f64>()
                .ok()?;
            value.is_finite().then_some((Some(unit), value))
        }
    }
}
