// SPDX-License-Identifier: Apache-2.0
//! Neutral document content from the typed property and `UFRx` inventories.

use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::{
    assets::{Asset, AssetContent, AssetId},
    attributes::{AttributeTarget, AttributeValue, SourceAttribute},
    document::CadIr,
    features::{ConfigurationId, DesignConfiguration},
    ids::AttributeId,
    report::loss::LossNote,
};

use crate::{
    loss::InventorLossCode,
    native::{ufrx::UfrxRecord, PropertyRecord, PropertyValueKind},
};

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    ufrx: &UfrxRecord,
    properties: &[PropertyRecord],
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let before = ir.model.entity_count();
    project_properties(ctx, ir, properties, losses)?;
    for reference in ufrx.embedded_references() {
        if reference.path.is_empty() {
            losses.push(InventorLossCode::NeutralRecordUnprojected.note(format!(
                "Embedded reference {} has no resource path; its typed native record remains held.",
                reference.id
            )));
            continue;
        }
        ir.model.assets.push(
            Asset::try_new(
                AssetId::compose(
                    &cadmpeg_ir::identity_namespace!("inventor", "document", "asset"),
                    cadmpeg_ir::identity_key!("embedded-").then(reference.ordinal),
                ),
                (!reference.display_name.is_empty()).then(|| reference.display_name.clone()),
                None,
                AssetContent::External {
                    uri: cadmpeg_core::text::NonBlankString::new(reference.path.clone())
                        .ok_or_else(|| CodecError::malformed("empty embedded resource path"))?,
                },
                Some(reference.id.clone()),
            )
            .map_err(CodecError::Malformed)?,
        );
    }
    let active = ufrx
        .representation()
        .map(|representation| representation.active_design_view.as_str());
    for state in ufrx.model_states().iter().filter(|_| ir.scope.design) {
        let is_active = active == Some(state.name.as_str());
        let mut overrides = std::collections::BTreeMap::new();
        for parameter in &state.parameters {
            let matches = ir
                .model
                .parameters
                .iter()
                .filter(|candidate| candidate.name == parameter.name)
                .collect::<Vec<_>>();
            if let [target] = matches.as_slice() {
                if !is_active {
                    overrides.insert(
                        target.id.clone(),
                        configuration_expression(&parameter.value, &ir.model.parameters)
                            .map_err(CodecError::malformed)?,
                    );
                }
            } else {
                losses.push(InventorLossCode::NeutralRecordUnprojected.note(format!(
                    "Model state {} parameter {:?} has no unique neutral parameter; its typed native value remains held.",
                    state.id, parameter.name
                )));
            }
        }
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId::compose(
                &cadmpeg_ir::identity_namespace!("inventor", "document", "configuration"),
                cadmpeg_ir::identity_key!("state-").then(state.ordinal),
            ),
            ordinal: state.ordinal,
            active: is_active,
            source_index: Some(state.ordinal),
            name: Some(state.name.to_string()),
            material: None,
            properties: std::collections::BTreeMap::new(),
            parameter_overrides: overrides,
            bodies: None,
            parameter_values: std::collections::BTreeMap::new(),
            feature_states: std::collections::BTreeMap::new(),
            native_ref: Some(state.id.clone()),
        });
    }
    let projected_design = ir
        .model
        .parameters
        .iter()
        .filter_map(|record| record.native_ref.as_deref())
        .chain(
            ir.model
                .features
                .iter()
                .filter_map(|record| record.native_ref.as_deref()),
        )
        .chain(
            ir.model
                .sketches
                .iter()
                .filter_map(|record| record.native_ref.as_deref()),
        )
        .chain(
            ir.model
                .sketch_entities
                .iter()
                .filter_map(|record| record.native_ref.as_deref()),
        )
        .chain(
            ir.model
                .sketch_constraints
                .iter()
                .filter_map(|record| record.native_ref.as_deref()),
        )
        .collect::<std::collections::BTreeSet<_>>();
    if let Some(namespace) = ir.native.namespace("inventor").filter(|_| ir.scope.design) {
        for arena in [
            "pm_dc_parameters",
            "pm_dc_features",
            "pm_dc_pattern_features",
            "pm_dc_sketches",
            "pm_dc_sketch_entities",
            "pm_dc_sketch_constraints",
        ] {
            if let Some(records) = namespace.arenas().get(arena) {
                for record in records {
                    if !projected_design.contains(record.id()) {
                        losses.push(InventorLossCode::NeutralRecordUnprojected.note(format!(
                            "Typed design record {} ({arena}) has no constructible neutral projection; its source-specific semantic fields remain held.", record.id()
                        )));
                    }
                }
            }
        }
    }
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(ir.model.entity_count() - before),
        "admit Inventor neutral document records",
    )?;
    Ok(())
}

/// Project source properties from the staged native property records.
pub(super) fn project_source_properties(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    native: &cadmpeg_ir::native::Native,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let Some(namespace) = native.namespace("inventor") else {
        return Ok(());
    };
    let before = ir.model.entity_count();
    for property in namespace.arena_iter_as_for_decode::<PropertyRecord>(ctx, "properties") {
        project_properties(ctx, ir, std::slice::from_ref(&property?), losses)?;
    }
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(ir.model.entity_count() - before),
        "admit Inventor neutral document records",
    )?;
    Ok(())
}

fn project_properties(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    properties: &[PropertyRecord],
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    for property in properties {
        let Some(value) = property.scalar_value.as_deref() else {
            if !matches!(property.value_kind, PropertyValueKind::Empty { .. }) {
                losses.push(InventorLossCode::NeutralRecordUnprojected.note(format!(
                    "Property {} has no typed scalar neutral projection; its native inventory remains held.",
                    property.id
                )));
            }
            continue;
        };
        let Some(name) = property.name.as_ref() else {
            losses.push(InventorLossCode::MetadataPropertyUnmapped.note(format!(
                "Property {} has no decoded attribute name.",
                property.id
            )));
            continue;
        };
        // Identity and BOM fields are consumed by the root product projection.
        if super::is_bom_property_name(name) && !ir.model.product_definitions.is_empty() {
            continue;
        }
        let typed = match property.value_kind {
            PropertyValueKind::Signed { .. } => value.parse().ok().map(AttributeValue::Integer),
            PropertyValueKind::Float { .. } => value.parse().ok().and_then(AttributeValue::float),
            PropertyValueKind::Bool { .. } => value.parse().ok().map(AttributeValue::Boolean),
            PropertyValueKind::String { .. } | PropertyValueKind::Guid { .. } => {
                Some(AttributeValue::String(value.to_owned()))
            }
            // The IR integer range is signed. An unsigned source value outside
            // that range stays typed text instead of changing its numeric value.
            PropertyValueKind::Unsigned { .. } | PropertyValueKind::Filetime { .. } => {
                Some(value.parse().map_or_else(
                    |_| AttributeValue::String(value.to_owned()),
                    AttributeValue::Integer,
                ))
            }
            _ => None,
        };
        if let Some(value) = typed {
            ir.model.attributes.push(SourceAttribute::new(
                ctx,
                AttributeId::mint(format!("{}:attribute", property.id))
                    .map_err(CodecError::malformed)?,
                Some(property.id.clone()),
                AttributeTarget::Document,
                name.clone(),
                vec![value],
            )?);
        } else {
            losses.push(InventorLossCode::NeutralRecordUnprojected.note(format!(
                "Property {} cannot be projected from its decoded scalar kind.",
                property.id
            )));
        }
    }
    Ok(())
}

/// `UFRx` model-state values are source expressions. Resolve their identifier
/// incidences to the same neutral parameter identities as the base document.
fn configuration_expression(
    text: &str,
    parameters: &[cadmpeg_ir::features::DesignParameter],
) -> Result<cadmpeg_ir::features::Expression, cadmpeg_ir::features::ExpressionError> {
    let mut names =
        std::collections::BTreeMap::<&str, Option<&cadmpeg_ir::features::ParameterId>>::new();
    for parameter in parameters {
        names
            .entry(parameter.name.as_str())
            .and_modify(|id| *id = None)
            .or_insert(Some(&parameter.id));
    }
    let mut spans = Vec::new();
    let mut start = None;
    let mut quote = None;
    let mut escape = false;
    for (at, character) in text
        .char_indices()
        .chain(std::iter::once((text.len(), '\0')))
    {
        if let Some(delimiter) = quote {
            if escape {
                escape = false;
            } else if character == '\\' {
                escape = true;
            } else if character == delimiter {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"') {
            quote = Some(character);
        }
        if character.is_alphabetic()
            || character == '_'
            || (start.is_some() && character.is_ascii_digit())
        {
            start.get_or_insert(at);
        } else if let Some(begin) = start.take() {
            if let Some(Some(id)) = names.get(&text[begin..at]) {
                spans.push((begin..at, (*id).clone()));
            }
        }
    }
    cadmpeg_ir::features::Expression::from_spans(text, spans)
}
