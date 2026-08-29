// SPDX-License-Identifier: Apache-2.0
//! Configuration records for native write.

use crate::records::{Configuration, FeatureHistory};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{DesignConfiguration, ParameterValue};
use std::collections::{BTreeMap, HashMap, HashSet};

use super::features::synchronize_history_content_order;
use crate::history::configuration::{
    align_configuration_parameter_kinds, configuration_lane_assignments,
    project_configuration_design_states, project_configuration_sketch_states,
};
use crate::history::hash::{
    configuration_feature_state_hash, configuration_hash, configuration_parameter_value_hash,
    native_configuration_hash,
};
use crate::history::parameters::{
    exact_integer_f64, global_parameter_owners, parameters_with_incoherent_evaluated_values,
};
use crate::history::project::project_configurations;
use crate::resolved_features::relation_geometry::is_reference_relation_parameter;

/// Resolve neutral/native configuration edit authority before writing.
///
/// Bitwise comparison against the machine-local document baseline; see
/// [`cadmpeg_ir::hash::document_local_sha256`]. Absent baseline: sync lanes from
/// the neutral side.
pub fn prepare_configurations_for_write(
    ir: &cadmpeg_ir::CadIr,
    native: &mut Option<crate::native::SldprtNative>,
    annotations: &cadmpeg_ir::Annotations,
) -> Result<(), CodecError> {
    let feature_state_hash = configuration_feature_state_hash(&ir.model.configurations);
    let baseline_feature_states = ir.source.as_ref().and_then(|source| {
        source
            .attributes
            .get("sldprt_configuration_feature_states_local_sha256")
    });
    let feature_states_changed = baseline_feature_states
        .is_some_and(|baseline| baseline != &feature_state_hash)
        || baseline_feature_states.is_none()
            && ir
                .model
                .configurations
                .iter()
                .any(|configuration| !configuration.feature_states.is_empty());
    let parameter_value_hash = configuration_parameter_value_hash(&ir.model.configurations);
    let baseline_parameter_values = ir.source.as_ref().and_then(|source| {
        source
            .attributes
            .get("sldprt_configuration_parameter_values_local_sha256")
    });
    let parameter_values_changed = baseline_parameter_values
        .is_some_and(|baseline| baseline != &parameter_value_hash)
        || baseline_parameter_values.is_none()
            && ir
                .model
                .configurations
                .iter()
                .any(|configuration| !configuration.parameter_values.is_empty());
    let neutral_hash = configuration_hash(&ir.model.configurations);
    let native_hash = native
        .as_ref()
        .map(|value| native_configuration_hash(&value.feature_histories));
    let baseline_neutral = ir.source.as_ref().and_then(|source| {
        source
            .attributes
            .get("sldprt_neutral_configuration_local_sha256")
    });
    let baseline_native = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sldprt_native_configuration_sha256"));
    let neutral_changed = baseline_neutral.is_none_or(|hash| hash != &neutral_hash);
    let native_changed = match (&native_hash, baseline_native) {
        (Some(current), Some(baseline)) => current != baseline,
        (Some(_), None) | (None, Some(_)) => true,
        (None, None) => false,
    };
    if baseline_neutral.is_none() && baseline_native.is_none() {
        sync_neutral_configurations(&ir.model.configurations, native);
    } else {
        match (neutral_changed, native_changed) {
            (false, _) => {}
            (true, true) => {
                let projected = native
                    .as_ref()
                    .map(|value| project_configurations(&value.feature_histories))
                    .unwrap_or_default();
                if configuration_hash(&projected) != neutral_hash {
                    return Err(CodecError::Malformed(
                        "conflicting neutral and native SLDPRT configuration edits".into(),
                    ));
                }
            }
            (true, false) => {
                sync_neutral_configurations(&ir.model.configurations, native);
            }
        }
    }
    if feature_states_changed || parameter_values_changed {
        sync_configuration_design_state(ir, native, annotations)?;
    }
    Ok(())
}

pub(crate) fn sync_configuration_design_state(
    ir: &cadmpeg_ir::CadIr,
    native: &mut Option<crate::native::SldprtNative>,
    annotations: &cadmpeg_ir::Annotations,
) -> Result<(), CodecError> {
    let form_padding = ir
        .source
        .as_ref()
        .and_then(|source| source.dialect.as_ref())
        .and_then(crate::dialect::SldprtDialect::from_match)
        .and_then(crate::dialect::SldprtDialect::form_code_padding);
    let feature_names = ir
        .model
        .features
        .iter()
        .filter_map(|feature| {
            feature
                .name
                .as_ref()
                .map(|name| (feature.id.clone(), name.clone()))
        })
        .collect::<HashMap<_, _>>();
    let global_owners = global_parameter_owners(&ir.model.features);
    if parameters_with_incoherent_evaluated_values(
        &ir.model.parameters,
        &feature_names,
        &global_owners,
        &ir.model.configurations,
    ) > 0
    {
        return Err(CodecError::Malformed(
            "SLDPRT configuration parameter values are inconsistent with their expressions".into(),
        ));
    }
    let Some(native) = native.as_mut() else {
        return Err(CodecError::NotImplemented(
            "SLDPRT configuration design state requires retained feature-input lanes".into(),
        ));
    };
    let mut current_projection = ir.clone();
    project_configuration_design_states(
        &mut current_projection,
        &native.feature_histories,
        &native.feature_input_lanes,
        &native.pmi_dimensions,
        form_padding,
    );
    align_configuration_parameter_kinds(&mut current_projection);
    let mut current_annotations = annotations.clone();
    project_configuration_sketch_states(
        &mut current_projection,
        &native.feature_histories,
        &native.feature_input_lanes,
        &mut current_annotations,
    );
    let current_parameter_hash =
        configuration_parameter_value_hash(&current_projection.model.configurations);
    let current_feature_hash =
        configuration_feature_state_hash(&current_projection.model.configurations);
    let current_matches = current_parameter_hash
        == configuration_parameter_value_hash(&ir.model.configurations)
        && current_feature_hash == configuration_feature_state_hash(&ir.model.configurations);
    if current_matches {
        return Ok(());
    }
    let native_design_state_changed = ir.source.as_ref().is_some_and(|source| {
        source
            .attributes
            .get("sldprt_configuration_parameter_values_local_sha256")
            .is_some_and(|baseline| baseline != &current_parameter_hash)
            || source
                .attributes
                .get("sldprt_configuration_feature_states_local_sha256")
                .is_some_and(|baseline| baseline != &current_feature_hash)
    });
    if native_design_state_changed {
        return Err(CodecError::Malformed(
            "conflicting neutral and native SLDPRT configuration design-state edits".into(),
        ));
    }
    let reference_parameters = ir
        .model
        .parameters
        .iter()
        .filter(|parameter| is_reference_relation_parameter(parameter))
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    for configuration in &ir.model.configurations {
        let Some(current) = current_projection
            .model
            .configurations
            .iter()
            .find(|candidate| candidate.id == configuration.id)
        else {
            continue;
        };
        for (parameter_id, desired) in &configuration.parameter_values {
            let Some(parameter) = reference_parameters.get(parameter_id) else {
                continue;
            };
            let current = current.parameter_values.get(parameter_id);
            if current != Some(desired) {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT display-only relation parameter {} has no writable configuration scalar",
                    parameter.id.0
                )));
            }
        }
    }
    patch_configuration_parameter_scalars(ir, native)?;

    let mut projected = ir.clone();
    project_configuration_design_states(
        &mut projected,
        &native.feature_histories,
        &native.feature_input_lanes,
        &native.pmi_dimensions,
        form_padding,
    );
    align_configuration_parameter_kinds(&mut projected);
    let mut projected_annotations = annotations.clone();
    project_configuration_sketch_states(
        &mut projected,
        &native.feature_histories,
        &native.feature_input_lanes,
        &mut projected_annotations,
    );
    if configuration_parameter_value_hash(&projected.model.configurations)
        != configuration_parameter_value_hash(&ir.model.configurations)
        || configuration_feature_state_hash(&projected.model.configurations)
            != configuration_feature_state_hash(&ir.model.configurations)
    {
        return Err(CodecError::NotImplemented(
            "SLDPRT configuration design-state edit has no complete native lane encoding".into(),
        ));
    }
    Ok(())
}

pub(crate) fn patch_configuration_parameter_scalars(
    ir: &cadmpeg_ir::CadIr,
    native: &mut crate::native::SldprtNative,
) -> Result<(), CodecError> {
    let parameters = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    let features = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature))
        .collect::<HashMap<_, _>>();
    for (configuration_index, lane_index) in
        configuration_lane_assignments(&ir.model.configurations, &native.feature_input_lanes)
    {
        let configuration = &ir.model.configurations[configuration_index];
        let lane = &mut native.feature_input_lanes[lane_index];
        let names = lane
            .names
            .iter()
            .map(|name| (name.id.as_str(), name.value.as_str()))
            .collect::<HashMap<_, _>>();
        let mut starts = native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .filter_map(|record| {
                crate::resolved_features::scalars::feature_object_name(record, lane)
                    .map(|name| (name.offset, record))
            })
            .collect::<Vec<_>>();
        starts.sort_by_key(|(offset, _)| *offset);
        for (parameter_id, value) in &configuration.parameter_values {
            let Some(parameter) = parameters.get(parameter_id) else {
                continue;
            };
            if is_reference_relation_parameter(parameter) {
                continue;
            }
            let Some(feature) = parameter
                .owner
                .as_ref()
                .and_then(|owner| features.get(owner))
            else {
                continue;
            };
            let Some(native_ref) = feature.native_ref.as_deref() else {
                continue;
            };
            let Some((position, (start, _))) = starts
                .iter()
                .enumerate()
                .find(|(_, (_, record))| record.id == native_ref)
            else {
                continue;
            };
            let end = starts
                .get(position + 1)
                .map_or(u64::MAX, |(offset, _)| *offset);
            let candidates = lane
                .scalars
                .iter()
                .enumerate()
                .filter(|(_, scalar)| scalar.offset > *start && scalar.offset < end)
                .filter(|(_, scalar)| {
                    names.get(scalar.name.as_str()) == Some(&parameter.name.as_str())
                })
                .collect::<Vec<_>>();
            let driving = candidates
                .iter()
                .filter(|(_, scalar)| {
                    scalar.role == crate::records::FeatureInputScalarRole::Driving
                })
                .map(|(index, _)| *index)
                .collect::<Vec<_>>();
            let candidates = if driving.is_empty() {
                candidates
                    .into_iter()
                    .filter(|(_, scalar)| {
                        scalar.role == crate::records::FeatureInputScalarRole::Native
                    })
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>()
            } else {
                driving
            };
            let [scalar_index] = candidates.as_slice() else {
                continue;
            };
            let encoded = match value {
                ParameterValue::Length(value) => value.0 / 1000.0,
                ParameterValue::Angle(value) => value.0,
                ParameterValue::Real(value) => *value,
                ParameterValue::Integer(value) => exact_integer_f64(*value).ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "SLDPRT configuration parameter {} cannot be represented by a native scalar",
                        parameter.id.0
                    ))
                })?,
                ParameterValue::Boolean(value) => f64::from(*value),
                ParameterValue::String(_) => {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT configuration parameter {} is textual and cannot be represented by a native scalar",
                        parameter.id.0
                    )));
                }
            };
            let scalar = &mut lane.scalars[*scalar_index];
            let offset = usize::try_from(scalar.offset).map_err(|_| {
                CodecError::Malformed("SLDPRT scalar offset exceeds address space".into())
            })?;
            lane.native_payload
                .get_mut(offset..offset + 8)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "SLDPRT scalar {} lies outside its payload",
                        scalar.id
                    ))
                })?
                .copy_from_slice(&encoded.to_le_bytes());
            scalar.value = encoded;
        }
    }
    Ok(())
}

/// Resolve neutral/native parameter edit authority before writing.
///
/// Bitwise comparison against the machine-local document baseline; see
/// [`cadmpeg_ir::hash::document_local_sha256`]. Absent baseline: sync lanes from
/// the neutral side.
pub(crate) fn sync_neutral_configurations(
    configurations: &[DesignConfiguration],
    native: &mut Option<crate::native::SldprtNative>,
) {
    if configurations.is_empty() && native.is_none() {
        return;
    }
    if native.is_none() {
        *native = Some(crate::native::SldprtNative::default());
    }
    let native = native.as_mut().expect("initialized above");
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
                .unwrap_or_else(|| format!("sldprt:generated:configuration#{}", configuration.id.0))
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
        let Some(configuration_name) = configuration.name.resolved() else {
            continue;
        };
        let existing = native
            .feature_histories
            .iter_mut()
            .flat_map(|history| &mut history.configurations)
            .find(|candidate| configuration.native_ref.as_deref() == Some(candidate.id.as_str()));
        if let Some(existing) = existing {
            let existing_id = existing.id.clone();
            let previous_slot = configuration_slot(&existing.properties, existing.ordinal);
            existing.ordinal = configuration.ordinal;
            existing.source_index = configuration.source_index;
            existing.name = configuration_name.to_string();
            existing.material.clone_from(&configuration.material);
            existing.properties.clone_from(&configuration.properties);
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
                    id: configuration.native_ref.clone().unwrap_or_else(|| {
                        format!("sldprt:generated:configuration#{}", configuration.id.0)
                    }),
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

pub(crate) fn configuration_slot(properties: &BTreeMap<String, String>, ordinal: u32) -> u32 {
    properties
        .get("id")
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(ordinal)
}

pub(crate) fn native_configuration_slot_owners(
    histories: &[FeatureHistory],
) -> BTreeMap<u32, Option<String>> {
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
