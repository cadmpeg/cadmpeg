// SPDX-License-Identifier: Apache-2.0
//! Configuration identities and current configuration scalar emission.

use crate::loss::SldprtLossCode;
use crate::records::{Configuration, FeatureHistory};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{ConfigurationId, DesignConfiguration, DistinctMembers, ParameterValue};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, HashMap, HashSet};

use super::features::synchronize_history_content_order;

/// Write the canonical side of an active configuration that carries state.
///
/// An active configuration is the base state, so its overrides, values and
/// feature states cannot be written as differences (#217 InvalidInput rule).
/// The writer clears them and charges each cleared entry, instead of refusing
/// on the validation Error (#237). Inactive configurations keep their
/// differences for their own write path.
pub(crate) fn normalize_active_configuration_state(
    configurations: &mut [DesignConfiguration],
    losses: &mut Vec<LossNote>,
) {
    for configuration in configurations.iter_mut().filter(|c| c.active) {
        for feature in configuration.feature_states.keys() {
            losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
                "configuration {} feature {feature}: the active configuration is the base state; its structural override was cleared",
                configuration.id
            )));
        }
        configuration.feature_states.clear();
        for parameter in configuration.parameter_overrides.keys() {
            losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
                "configuration {} parameter {parameter}: the active configuration is the base state; its expression override was cleared",
                configuration.id
            )));
        }
        configuration.parameter_overrides.clear();
        for parameter in configuration.parameter_values.keys() {
            losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
                "configuration {} parameter {parameter}: the active configuration is the base state; its value was cleared",
                configuration.id
            )));
        }
        configuration.parameter_values.clear();
    }
}

/// Author the required native default configuration for a document that
/// carries geometry but no configuration.
///
/// Every SolidWorks file selects exactly one active configuration, so the
/// decoder invents partition configurations without native definitions for
/// one and charges `config.inferred-without-native`. A document that holds
/// no configuration still lands in such a file, so synthesis authors the
/// default (named, active, carrying the supported bodies) before the native
/// synchronization below turns it into a bound definition (#237).
pub(crate) fn ensure_default_configuration(
    projected: &mut CadIr,
    supported: &[BodyId],
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    if !projected.scope.design || !projected.model.configurations.is_empty() || supported.is_empty()
    {
        return Ok(());
    }
    let id = ConfigurationId::mint("sldprt:model:configuration#default")
        .map_err(|_| CodecError::malformed("SLDPRT default configuration identity is invalid"))?;
    let bodies = DistinctMembers::try_from(supported.to_vec()).map_err(CodecError::malformed)?;
    losses.push(SldprtLossCode::WriterDesignReduced.note(format!(
        "configuration {id}: the document carries no configuration, so synthesis authored the required native default carrying {} body(s)",
        supported.len()
    )));
    projected.model.configurations.push(DesignConfiguration {
        id,
        ordinal: 0,
        active: true,
        source_index: None,
        name: Some("Default".to_owned()),
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        bodies: Some(bodies),
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });
    Ok(())
}

/// Render canonical configuration differences without a source baseline.
pub(crate) fn synchronize_configuration_records(
    ir: &cadmpeg_ir::CadIr,
    native: &mut Option<crate::native::SldprtNative>,
    _annotations: &cadmpeg_ir::Annotations,
) -> Result<(), CodecError> {
    if !ir.scope.design {
        return Ok(());
    }
    sync_neutral_configurations(&ir.model.configurations, native);
    if ir.model.configurations.iter().any(|configuration| {
        !configuration.feature_states.is_empty() || !configuration.parameter_overrides.is_empty()
    }) {
        // (P/S-entity): the scalar lane has no encoding for a structural
        // operation override or an expression tree. Synthesis charges each
        // omitted difference and retains its independently writable cache.
        return Err(CodecError::UnsupportedContent(
            "SLDPRT configuration lanes encode evaluated scalar differences; structural and expression differences have no native serializer".into(),
        ));
    }
    let Some(native) = native.as_mut() else {
        return Ok(());
    };
    let parameter_by_id = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    let feature_by_id = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature))
        .collect::<HashMap<_, _>>();
    let mut next_scalar = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .filter_map(crate::records::Feature::source_value)
        .max()
        .unwrap_or(0);
    let mut generated = Vec::new();
    for configuration in &ir.model.configurations {
        if configuration.active || configuration.parameter_values.is_empty() {
            continue;
        }
        let slot = configuration_slot(&configuration.properties, configuration.ordinal).to_string();
        let mut groups = BTreeMap::<
            &cadmpeg_ir::features::FeatureId,
            Vec<(&cadmpeg_ir::features::DesignParameter, &ParameterValue)>,
        >::new();
        for (id, value) in &configuration.parameter_values {
            let parameter = parameter_by_id.get(id).ok_or_else(|| {
                CodecError::malformed("SLDPRT configuration references a missing parameter")
            })?;
            let owner = parameter.owner.as_ref().ok_or_else(|| {
                CodecError::UnsupportedContent(format!(
                    "SLDPRT configuration parameter {id} has no native feature owner"
                ))
            })?;
            groups.entry(owner).or_default().push((parameter, value));
        }
        let mut payload = Vec::new();
        for (owner, values) in groups {
            let feature = feature_by_id.get(owner).ok_or_else(|| {
                CodecError::malformed("SLDPRT configuration parameter references a missing feature")
            })?;
            let record = native
                .feature_histories
                .iter()
                .flat_map(|history| &history.features)
                .find(|record| {
                    feature.native_ref.as_deref() == Some(record.id.as_str())
                        || (feature.native_ref.is_none()
                            && record.id == super::features::generated_feature_record_id(owner))
                })
                .ok_or_else(|| {
                    CodecError::UnsupportedContent(format!(
                        "SLDPRT configuration feature {owner} has no writable native identity"
                    ))
                })?;
            let object_id = record.source_value().ok_or_else(|| {
                CodecError::UnsupportedContent(format!(
                    "SLDPRT configuration feature {owner} has no numeric native identity"
                ))
            })?;
            crate::resolved_features::write_generate::append_generated_object_name(
                &mut payload,
                &record.name,
                object_id,
            )?;
            for (parameter, value) in values {
                let (class, value) = match value {
                    ParameterValue::Length(value) => ("moLengthParameter_c", value.get() / 1000.0),
                    ParameterValue::Angle(value) => ("moAngleParameter_c", value.get()),
                    ParameterValue::Real(_)
                    | ParameterValue::Integer(_)
                    | ParameterValue::Boolean(_)
                    | ParameterValue::String(_) => {
                        return Err(CodecError::UnsupportedContent(format!(
                            "SLDPRT configuration parameter {} has no scalar encoding",
                            parameter.id
                        )))
                    }
                };
                crate::resolved_features::write_generate::append_generated_scalar(
                    &mut payload,
                    class,
                    &parameter.name,
                    value,
                    {
                        // (O-capacity): each scalar requires a distinct u32 native identity.
                        next_scalar = next_scalar.checked_add(1).ok_or_else(|| {
                            CodecError::malformed(
                                "SLDPRT configuration scalar identity space is exhausted",
                            )
                        })?;
                        next_scalar
                    },
                    &[],
                )?;
            }
        }
        generated.push(
            crate::resolved_features::write_prepare::generated_configuration_lane(
                &native.feature_histories,
                &slot,
                payload,
            )?,
        );
    }
    for lane in generated {
        native
            .feature_input_lanes
            .retain(|existing| existing.configuration != lane.configuration);
        native.feature_input_lanes.push(lane);
    }
    Ok(())
}

/// Regenerate configuration identities and metadata from the neutral model.
pub(crate) fn sync_neutral_configurations(
    configurations: &[DesignConfiguration],
    native: &mut Option<crate::native::SldprtNative>,
) {
    if configurations.is_empty() && native.is_none() {
        return;
    }
    let native = native.get_or_insert_with(crate::native::SldprtNative::default);
    if native.feature_histories.is_empty() {
        native.feature_histories.push(FeatureHistory {
            id: "sldprt:generated:feature-history#0".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: Vec::new(),
        });
    }
    let mut configurations = configurations.iter().collect::<Vec<_>>();
    configurations.sort_by_key(|configuration| configuration.ordinal);
    let desired_ids = configurations
        .iter()
        .map(|configuration| {
            configuration
                .native_ref
                .clone()
                .unwrap_or_else(|| generated_configuration_record_id(&configuration.id))
        })
        .collect::<std::collections::HashSet<_>>();
    let previous_slot_owners = native_configuration_slot_owners(&native.feature_histories);
    let deleted_ids = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.configurations)
        .filter(|configuration| !desired_ids.contains(&configuration.id))
        .map(|configuration| configuration.id.clone())
        .collect::<HashSet<_>>();
    native.feature_input_lanes.retain(|lane| {
        let Some(index) = lane
            .configuration
            .as_deref()
            .and_then(|configuration| configuration.parse::<u32>().ok())
        else {
            return true;
        };
        previous_slot_owners
            .get(&index)
            .and_then(Option::as_deref)
            .is_none_or(|owner| !deleted_ids.contains(owner))
    });
    for history in &mut native.feature_histories {
        history
            .configurations
            .retain(|configuration| desired_ids.contains(&configuration.id));
    }
    let mut lane_configuration_remaps = HashMap::<String, String>::new();
    for configuration in configurations {
        let Some(configuration_name) = configuration.name.as_deref() else {
            continue;
        };
        let existing = native
            .feature_histories
            .iter_mut()
            .flat_map(|history| &mut history.configurations)
            .find(|candidate| {
                candidate.id
                    == configuration
                        .native_ref
                        .clone()
                        .unwrap_or_else(|| generated_configuration_record_id(&configuration.id))
            });
        if let Some(existing) = existing {
            let existing_id = existing.id.clone();
            let previous_slot = configuration_slot(&existing.properties, existing.ordinal);
            existing.ordinal = configuration.ordinal;
            existing.source_index = configuration.source_index;
            existing.name = configuration_name.to_string();
            existing.material.clone_from(&configuration.material);
            let display_state = existing.properties.get("DisplayState").cloned();
            existing.properties.clone_from(&configuration.properties);
            if configuration.properties.get("DisplayState").is_none() {
                if let Some(display_state) = display_state {
                    if let Some(key) = cadmpeg_core::text::NonBlankString::new("DisplayState") {
                        existing.properties.insert(key, display_state);
                    }
                }
            }
            let configuration_slot =
                configuration_slot(&configuration.properties, configuration.ordinal);
            if previous_slot != configuration_slot
                && previous_slot_owners
                    .get(&previous_slot)
                    .and_then(Clone::clone)
                    == Some(existing_id)
            {
                lane_configuration_remaps
                    .insert(previous_slot.to_string(), configuration_slot.to_string());
            }
        } else {
            let parent = native.feature_histories[0].id.clone();
            native.feature_histories[0]
                .configurations
                .push(Configuration {
                    id: configuration
                        .native_ref
                        .clone()
                        .unwrap_or_else(|| generated_configuration_record_id(&configuration.id)),
                    parent,
                    ordinal: configuration.ordinal,
                    source_index: configuration.source_index,
                    name: configuration_name.to_string(),
                    material: configuration.material.clone(),
                    properties: configuration.properties.clone(),
                });
        }
    }
    for lane in &mut native.feature_input_lanes {
        let Some(configuration) = lane.configuration.as_ref() else {
            continue;
        };
        if let Some(remapped) = lane_configuration_remaps.get(configuration) {
            lane.configuration = Some(remapped.clone());
        }
    }
    for history in &mut native.feature_histories {
        history
            .configurations
            .sort_by_key(|configuration| configuration.ordinal);
    }
    synchronize_history_content_order(native);
}

fn configuration_slot(
    properties: &BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    ordinal: u32,
) -> u32 {
    properties
        .get("id")
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(ordinal)
}

fn native_configuration_slot_owners(histories: &[FeatureHistory]) -> BTreeMap<u32, Option<String>> {
    let configurations = histories
        .iter()
        .flat_map(|history| &history.configurations)
        .collect::<Vec<_>>();
    let mut owners = BTreeMap::<u32, Option<String>>::new();
    for configuration in configurations.iter().filter(|configuration| {
        configuration
            .properties
            .get("id")
            .and_then(|value| value.parse::<u32>().ok())
            .is_some()
    }) {
        let index = configuration_slot(&configuration.properties, configuration.ordinal);
        owners
            .entry(index)
            .and_modify(|owner| *owner = None)
            .or_insert_with(|| Some(configuration.id.clone()));
    }
    let explicit_slots = owners.keys().copied().collect::<HashSet<_>>();
    for configuration in configurations.into_iter().filter(|configuration| {
        configuration
            .properties
            .get("id")
            .and_then(|value| value.parse::<u32>().ok())
            .is_none()
    }) {
        if explicit_slots.contains(&configuration.ordinal) {
            continue;
        }
        owners
            .entry(configuration.ordinal)
            .and_modify(|owner| *owner = None)
            .or_insert_with(|| Some(configuration.id.clone()));
    }
    owners
}

fn generated_configuration_record_id(id: &cadmpeg_ir::features::ConfigurationId) -> String {
    format!(
        "sldprt:generated:configuration#{}",
        id.as_str().replace('%', "%25").replace('#', "%23")
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn configuration_less_document_gets_a_native_default() {
        let mut ir = crate::test_support::ir::source_less_cube();
        ir.scope.design = true;
        ir.model.configurations.clear();
        let bodies = ir
            .model
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect::<Vec<_>>();
        let mut losses = Vec::new();
        super::ensure_default_configuration(&mut ir, &bodies, &mut losses)
            .expect("a default configuration is authored");
        assert_eq!(ir.model.configurations.len(), 1);
        let default = &ir.model.configurations[0];
        assert_eq!(default.name.as_deref(), Some("Default"));
        assert!(default.active);
        assert_eq!(
            default
                .bodies
                .as_ref()
                .map(|bodies| bodies.iter().cloned().collect::<Vec<_>>()),
            Some(bodies)
        );
        assert!(
            losses
                .iter()
                .any(|loss| loss.message.contains("Default") || loss.message.contains("default")),
            "the authored default is charged"
        );
    }

    #[test]
    fn generated_configuration_identity_escapes_embedded_separators() {
        let id =
            cadmpeg_ir::features::ConfigurationId::mint("test:model:configuration#original%23key")
                .expect("fixture identity");
        let record = super::generated_configuration_record_id(&id);
        assert_eq!(
            record,
            "sldprt:generated:configuration#test:model:configuration%23original%2523key"
        );
        assert!(cadmpeg_ir::ids::is_valid_identity(&record));
    }
}
