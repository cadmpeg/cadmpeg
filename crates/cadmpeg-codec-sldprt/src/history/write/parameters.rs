// SPDX-License-Identifier: Apache-2.0
//! Parameter records for native write.

use crate::records::{Feature, FeatureContent};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::parameters::{DesignParameter, ParameterValue};
use cadmpeg_ir::features::{FeatureId, ParameterId};
use std::collections::{BTreeMap, HashMap, HashSet};

use super::edits::{SourceEdits, UnwrittenPart};
use super::features::generated_feature_record_id;
use crate::history::hash::{native_parameter_hash, parameter_hash};
use crate::history::literals::{
    dimension_display, format_parameter_value, parse_neutral_parameter_literal,
    parse_parameter_literal,
};
use crate::history::parameters::eval::exact_integer_f64;
use crate::history::parameters::{
    equivalent_parameter_values, expression_identifier_tokens, global_parameter_owners,
    parameters_with_incoherent_dependencies, parse_native_parameter_literal, project_parameters,
    stated_parameter_value,
};
use crate::history::project::neutral_feature_id;
use crate::resolved_features::relation_geometry::is_reference_relation_parameter;

/// The parameters of a write that leaves out `omitted` whose expression
/// reads a parameter the write leaves out. `valued` are written with their
/// value in place of the expression. `dropped` hold no value a record entry
/// can hold, so they are left out too, and a parameter that reads one of
/// them is in turn valued or dropped.
pub(crate) struct OmittedReaders<'a> {
    pub(crate) valued: Vec<&'a DesignParameter>,
    pub(crate) dropped: Vec<&'a DesignParameter>,
}

pub(crate) fn omitted_readers<'a>(
    parameters: &'a [DesignParameter],
    omitted: &HashSet<&ParameterId>,
) -> OmittedReaders<'a> {
    let mut unavailable = omitted.iter().copied().collect::<HashSet<&ParameterId>>();
    let mut valued_ids = HashSet::new();
    let mut readers = OmittedReaders {
        valued: Vec::new(),
        dropped: Vec::new(),
    };
    loop {
        let mut dropped_more = false;
        for parameter in parameters {
            if unavailable.contains(&parameter.id)
                || valued_ids.contains(&parameter.id)
                || !parameter
                    .dependencies
                    .iter()
                    .any(|dependency| unavailable.contains(dependency))
            {
                continue;
            }
            if value_literal(parameter).is_some() {
                valued_ids.insert(&parameter.id);
                readers.valued.push(parameter);
            } else {
                unavailable.insert(&parameter.id);
                readers.dropped.push(parameter);
                dropped_more = true;
            }
        }
        if !dropped_more {
            return readers;
        }
    }
}

/// The literal of `parameter`'s value, when a record entry can hold it.
fn value_literal(parameter: &DesignParameter) -> Option<String> {
    parameter
        .value
        .as_ref()
        .map(format_parameter_value)
        .filter(|text| parse_parameter_literal(text).is_some())
}

/// `parameters` without those of `omitted` and the [`omitted_readers`] it
/// drops, each valued reader written with its value in place of its
/// expression, since the record the expression names is left out.
fn without_omitted_driven(
    parameters: Vec<DesignParameter>,
    omitted: &HashSet<&ParameterId>,
) -> Vec<DesignParameter> {
    let readers = omitted_readers(&parameters, omitted);
    let ids = |readers: &[&DesignParameter]| {
        readers
            .iter()
            .map(|parameter| parameter.id.clone())
            .collect::<HashSet<_>>()
    };
    let valued = ids(&readers.valued);
    let dropped = ids(&readers.dropped);
    parameters
        .into_iter()
        .filter(|parameter| !omitted.contains(&parameter.id) && !dropped.contains(&parameter.id))
        .map(|mut parameter| {
            if let Some(literal) = valued
                .contains(&parameter.id)
                .then(|| value_literal(&parameter))
                .flatten()
            {
                parameter.display = dimension_display(&literal);
                parameter.expression = literal;
                parameter.dependencies =
                    cadmpeg_ir::features::selections::DistinctMembers::default();
            }
            parameter
        })
        .collect()
}

pub(crate) fn prepare_parameters_for_write(
    ir: &cadmpeg_ir::CadIr,
    digests: &cadmpeg_ir::source_fidelity::CodecDigests,
    native: &mut Option<crate::native::SldprtNative>,
    feature_parameter_changes_authorized: bool,
    edits: &mut SourceEdits<'_>,
) -> Result<(), CodecError> {
    let hash_arena = cadmpeg_core::decode::DecodeArena::new();
    let (hash_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &hash_arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )?;

    let neutral_hash = parameter_hash(&hash_ctx, &ir.model.parameters)?;
    let native_hash = native
        .as_ref()
        .map(|value| native_parameter_hash(&hash_ctx, &value.feature_histories))
        .transpose()?;
    let baseline_neutral =
        crate::codec_digests::baseline(digests, "sldprt_neutral_parameter_local_sha256");
    let baseline_native = crate::codec_digests::baseline(digests, "sldprt_native_parameter_sha256");
    let neutral_changed = baseline_neutral.is_none_or(|hash| hash != neutral_hash);
    let native_changed = !edits.native_unedited()
        && match (&native_hash, baseline_native) {
            (Some(current), Some(baseline)) => current != baseline,
            (Some(_), None) | (None, Some(_)) => true,
            (None, None) => false,
        };
    if baseline_neutral.is_none() && baseline_native.is_none() {
        if ir.model.parameters.is_empty() {
            return Ok(());
        }
        return sync_neutral_parameters(ir, digests, native, edits);
    }
    match (neutral_changed, native_changed) {
        (false, true) if feature_parameter_changes_authorized => Ok(()),
        (false, _) => Ok(()),
        (true, true) => {
            if feature_parameter_changes_authorized {
                return sync_neutral_parameters(ir, digests, native, edits);
            }
            let lane_bytes = native
                .as_ref()
                .into_iter()
                .flat_map(|native| &native.feature_input_lanes)
                .flat_map(|lane| lane.native_payload.iter().copied())
                .collect::<Vec<_>>();
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &lane_bytes,
                &arena,
                &cadmpeg_core::decode::DecodePolicy::service(),
            )?;
            let projected = native
                .as_ref()
                .map(|value| project_parameters(&ctx, &value.feature_histories))
                .transpose()?
                .unwrap_or_default();
            if parameter_hash(&hash_ctx, &projected)? == neutral_hash {
                Ok(())
            } else {
                Err(CodecError::Malformed(
                    "conflicting neutral and native SLDPRT parameter edits".into(),
                ))
            }
        }
        (true, false) => sync_neutral_parameters(ir, digests, native, edits),
    }
}

/// The dimension properties the record states for `parameter`: a
/// non-literal expression carries an edited value in the `Value` property.
fn written_dimension_properties(
    parameter: &DesignParameter,
    value_edited: bool,
) -> BTreeMap<cadmpeg_core::text::NonBlankString, String> {
    let mut properties = parameter.properties.clone();
    if value_edited && parse_parameter_literal(&parameter.expression).is_none() {
        if let Some(value) = &parameter.value {
            properties.insert(
                cadmpeg_core::nonblank_literal!("Value"),
                format_parameter_value(value),
            );
        } else {
            properties.remove("Value");
        }
    }
    properties
}

fn sync_neutral_parameters(
    ir: &cadmpeg_ir::CadIr,
    digests: &cadmpeg_ir::source_fidelity::CodecDigests,
    native: &mut Option<crate::native::SldprtNative>,
    edits: &mut SourceEdits<'_>,
) -> Result<(), CodecError> {
    // The decoder projects a display-only relation parameter from its
    // relation's solved display scalar; no feature record holds it.
    for parameter in ir
        .model
        .parameters
        .iter()
        .filter(|parameter| is_reference_relation_parameter(parameter))
    {
        if edits.parameter_edited(&parameter.id) {
            edits.parameter_unwritten(&parameter.id, UnwrittenPart::Whole, format!(
                "Display-only relation parameter {} ({} = {}) is not written; the stored relation keeps its solved value.",
                parameter.id.as_str(),
                parameter.name,
                parameter.expression
            ));
        }
    }
    let mut parameters = ir.model.parameters.clone();
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
    let lane_bytes = native
        .as_ref()
        .into_iter()
        .flat_map(|native| &native.feature_input_lanes)
        .flat_map(|lane| lane.native_payload.iter().copied())
        .collect::<Vec<_>>();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &lane_bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )?;
    let global_owners = global_parameter_owners(&ir.model.features);
    if let Some(native) = native.as_ref() {
        let original = project_parameters(&ctx, &native.feature_histories)?;
        let original_feature_names = native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .map(|feature| Ok((neutral_feature_id(&feature.id)?, feature.name.clone())))
            .collect::<Result<HashMap<_, _>, CodecError>>()?;
        rewrite_renamed_parameter_references(
            &mut parameters,
            &original,
            &original_feature_names,
            &feature_names,
        );
    }
    if parameters_with_incoherent_dependencies(&parameters, &feature_names, &global_owners) > 0 {
        return Err(CodecError::Malformed(
            "SLDPRT parameter dependencies are inconsistent with their expressions".into(),
        ));
    }
    // A record entry is an editable dimension, so the parameter of a driven
    // dimension whose reference relation the write does not write stays out
    // of its feature's record; the export charges it.
    let omitted = crate::resolved_features::write_prepare::omitted_driven_parameters(ir, digests);
    let stated = parameters;
    let parameters = without_omitted_driven(stated.clone(), &omitted);
    let left_out = stated
        .into_iter()
        .filter(|parameter| !parameters.iter().any(|kept| kept.id == parameter.id))
        .collect::<Vec<_>>();
    let features = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature))
        .collect::<HashMap<_, _>>();
    let records = native
        .iter()
        .flat_map(|native| &native.feature_histories)
        .flat_map(|history| &history.features)
        .map(|record| (record.id.as_str(), record))
        .collect::<HashMap<_, _>>();
    // The source parameters by id: a parameter equal to its source keeps the
    // stored record fields. `None` without a decoded source, where every
    // record states exactly the document's parameters.
    let source_parameters = edits.source_parameters().cloned();
    let mut carrier_only = HashSet::<ParameterId>::new();
    let mut desired = HashMap::<FeatureId, Vec<&DesignParameter>>::new();
    for parameter in &parameters {
        if is_reference_relation_parameter(parameter) {
            continue;
        }
        let Some(owner_id) = parameter.owner.as_ref() else {
            if !edits.has_source() {
                return Err(CodecError::NotImplemented(format!(
                    "global SLDPRT parameter {} cannot be written to a feature record",
                    parameter.id.as_str()
                )));
            }
            if edits.parameter_edited(&parameter.id) {
                edits.parameter_unwritten(
                    &parameter.id,
                    UnwrittenPart::Whole,
                    format!(
                        "Global SLDPRT parameter {} is not written: each written parameter is an \
                         entry of its owner feature's record, and the parameter has no owner \
                         feature.",
                        parameter.id.as_str()
                    ),
                );
            }
            continue;
        };
        let Some(owner) = features.get(owner_id) else {
            return Err(CodecError::malformed(format_args!(
                "SLDPRT parameter {} references a missing feature",
                parameter.id.as_str()
            )));
        };
        let edited = edits.parameter_edited(&parameter.id);
        if edited && parameter.display != dimension_display(&parameter.expression) {
            edits.parameter_detail_unwritten(format!(
                "Parameter {} is written with expression {}; its display semantics {:?} are not written.",
                parameter.id.as_str(),
                parameter.expression,
                parameter.display
            ));
        }
        // A scalar-bound parameter carries its value in the bound scalar and
        // its expression in the record; the record alone states the value of
        // any other parameter, read as the decoder reads it.
        let record_id = owner
            .native_ref
            .clone()
            .unwrap_or_else(|| generated_feature_record_id(owner_id));
        let record = records.get(record_id.as_str()).copied();
        if edited
            && parameter.native_ref.is_none()
            && !(parameter.pmi.is_some() && carried_outside_record(record, parameter))
        {
            let stated = record.map_or_else(
                || parse_neutral_parameter_literal(owner, &parameter.name, &parameter.expression),
                |record| {
                    stated_parameter_value(
                        record,
                        &parameter.name,
                        &parameter.expression,
                        &written_dimension_properties(parameter, true),
                    )
                },
            );
            if let Some(stated) = stated.filter(|stated| {
                !parameter
                    .value
                    .as_ref()
                    .is_some_and(|value| equivalent_parameter_values(value, stated))
            }) {
                edits.parameter_unwritten(&parameter.id, UnwrittenPart::Value, format!(
                    "Parameter {} is written with expression {}, which states {}; its value {} is not written.",
                    parameter.id.as_str(),
                    parameter.expression,
                    format_parameter_value(&stated),
                    parameter.value.as_ref().map_or_else(|| "none".to_owned(), format_parameter_value)
                ));
            }
        }
        // An edited parameter that a feature-input scalar or a PMI record
        // carries and the stored record does not hold stays out of the
        // record: adding it changes how the decoder reads the record.
        if let Some(source) = source_parameters.as_ref() {
            if source.get(&parameter.id).copied() != Some(parameter)
                && (parameter.native_ref.is_some() || parameter.pmi.is_some())
                && source.get(&parameter.id).is_some_and(|original| {
                    original.owner == parameter.owner && carried_outside_record(record, original)
                })
            {
                carrier_only.insert(parameter.id.clone());
                if source.get(&parameter.id).is_some_and(|original| {
                    original.name != parameter.name || original.properties != parameter.properties
                }) {
                    edits.parameter_detail_unwritten(format!(
                        "Parameter {} is stated only by its feature-input scalar or PMI record; its name and dimension properties keep their stored state.",
                        parameter.id.as_str()
                    ));
                }
            }
        }
        let owner_parameters = desired.entry(owner_id.clone()).or_default();
        if owner_parameters
            .iter()
            .any(|candidate| candidate.name == parameter.name)
        {
            return Err(CodecError::malformed(format_args!(
                "duplicate SLDPRT parameter {} on feature {}",
                parameter.name, owner_id
            )));
        }
        if owner_parameters
            .iter()
            .any(|candidate| candidate.ordinal == parameter.ordinal)
        {
            return Err(CodecError::malformed(format_args!(
                "duplicate SLDPRT parameter ordinal {} on feature {}",
                parameter.ordinal, owner_id
            )));
        }
        owner_parameters.push(parameter);
    }
    let Some(native) = native.as_mut() else {
        return Err(CodecError::NotImplemented(
            "SLDPRT parameters require feature records".into(),
        ));
    };
    let record_ids = ir
        .model
        .features
        .iter()
        .map(|feature| {
            (
                &feature.id,
                feature
                    .native_ref
                    .clone()
                    .unwrap_or_else(|| generated_feature_record_id(&feature.id)),
            )
        })
        .collect::<HashMap<_, _>>();
    // A parameter the write leaves out is out of its feature's record.
    for parameter in &left_out {
        let Some(record) = parameter
            .owner
            .as_ref()
            .and_then(|owner| record_of(native, &record_ids, owner))
        else {
            continue;
        };
        record.parameters.remove(parameter.name.as_str());
        record.dimension_properties.remove(parameter.name.as_str());
        record.content.retain(
            |item| !matches!(item, FeatureContent::Dimension(stated) if stated == &parameter.name),
        );
    }
    if let Some(source) = source_parameters.as_ref() {
        let current = parameters
            .iter()
            .map(|parameter| (&parameter.id, parameter))
            .collect::<HashMap<_, _>>();
        // A source parameter removed from its record, or renamed in it.
        let mut originals = source
            .values()
            .copied()
            .filter(|original| {
                !is_reference_relation_parameter(original)
                    && !left_out.iter().any(|parameter| parameter.id == original.id)
            })
            .collect::<Vec<_>>();
        originals.sort_by(|left, right| left.id.cmp(&right.id));
        for original in originals {
            let now = current.get(&original.id).copied();
            if now.is_some_and(|now| now.owner == original.owner && now.name == original.name) {
                continue;
            }
            let Some(owner_id) = original.owner.as_ref() else {
                continue;
            };
            let Some(record) = record_of(native, &record_ids, owner_id) else {
                continue;
            };
            if !record.parameters.contains_key(original.name.as_str()) {
                if now.is_none() {
                    edits.parameter_unwritten(
                        &original.id,
                        UnwrittenPart::Whole,
                        format!(
                            "Parameter {} ({}) is removed, and the feature-input record that states it is kept; the written part keeps it.",
                            original.id.as_str(),
                            original.name
                        ),
                    );
                }
                continue;
            }
            let stored = record.parameters.remove(original.name.as_str());
            let stored_properties = record.dimension_properties.remove(original.name.as_str());
            let renamed = now
                .filter(|now| now.owner == original.owner)
                .map(|now| now.name.clone());
            // A renamed parameter keeps its stored text under its new name.
            if let (Some(name), Some(stored)) = (renamed.as_ref(), stored) {
                let key =
                    cadmpeg_core::text::NonBlankString::new(name.clone()).ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "SLDPRT parameter {} has a blank name",
                            original.id.as_str()
                        ))
                    })?;
                record.parameters.insert(key, stored);
                if let Some(properties) = stored_properties {
                    record.dimension_properties.insert(name.clone(), properties);
                }
            }
            record.content = record
                .content
                .iter()
                .filter_map(|item| match item {
                    FeatureContent::Dimension(name) if name == &original.name => {
                        renamed.clone().map(FeatureContent::Dimension)
                    }
                    other => Some(other.clone()),
                })
                .collect();
        }
        // A parameter the caller edited, or whose expression a rename
        // rewrote, writes its record entry; every other entry keeps its
        // stored state.
        let mut reordered = HashSet::<&FeatureId>::new();
        for parameter in &parameters {
            if is_reference_relation_parameter(parameter)
                || source.get(&parameter.id).copied() == Some(parameter)
                || carrier_only.contains(&parameter.id)
            {
                continue;
            }
            let Some(owner_id) = parameter.owner.as_ref() else {
                continue;
            };
            let value_edited = source
                .get(&parameter.id)
                .is_none_or(|original| original.value != parameter.value);
            if source
                .get(&parameter.id)
                .is_none_or(|original| original.ordinal != parameter.ordinal)
            {
                reordered.insert(owner_id);
            }
            let record = record_of(native, &record_ids, owner_id).ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "SLDPRT parameters for feature {owner_id} require a retained feature record"
                ))
            })?;
            let name = cadmpeg_core::text::NonBlankString::new(parameter.name.clone()).ok_or_else(
                || {
                    CodecError::malformed(format_args!(
                        "SLDPRT parameter {} has a blank name",
                        parameter.id.as_str()
                    ))
                },
            )?;
            let held = record.parameters.contains_key(name.as_str());
            // Each field the edit moves is written; the stored text of an
            // unedited expression is how the decoder reads the value.
            let original = source.get(&parameter.id).copied();
            if !held || original.is_none_or(|original| original.expression != parameter.expression)
            {
                record
                    .parameters
                    .insert(name.clone(), parameter.expression.clone());
            }
            let properties = written_dimension_properties(parameter, value_edited);
            if original.is_none_or(|original| {
                original.properties != properties || original.name != parameter.name
            }) {
                record
                    .dimension_properties
                    .insert(parameter.name.clone(), properties);
            }
            if !held
                && !record.content.is_empty()
                && !record.content.iter().any(
                    |item| matches!(item, FeatureContent::Dimension(stated) if stated == &parameter.name),
                )
            {
                record
                    .content
                    .push(FeatureContent::Dimension(parameter.name.clone()));
            }
        }
        // An edited parameter order places the record's dimensions in that
        // order.
        for owner_id in reordered {
            let Some(record) = record_of(native, &record_ids, owner_id) else {
                continue;
            };
            let mut owned = desired
                .get(owner_id)
                .into_iter()
                .flatten()
                .filter(|parameter| record.parameters.contains_key(parameter.name.as_str()))
                .collect::<Vec<_>>();
            owned.sort_by_key(|parameter| parameter.ordinal);
            let placed = owned
                .iter()
                .map(|parameter| parameter.name.as_str())
                .collect::<HashSet<_>>();
            let mut names = owned.iter().map(|parameter| parameter.name.clone());
            let mut content = record
                .content
                .iter()
                .filter_map(|item| match item {
                    FeatureContent::Dimension(name) if placed.contains(name.as_str()) => {
                        names.next().map(FeatureContent::Dimension)
                    }
                    other => Some(other.clone()),
                })
                .collect::<Vec<_>>();
            if !content.is_empty() {
                content.extend(names.map(FeatureContent::Dimension));
            }
            record.content = content;
        }
    } else {
        for feature in &ir.model.features {
            let feature_id = &feature.id;
            let record = record_of(native, &record_ids, feature_id).ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "SLDPRT parameters for feature {feature_id} require a retained feature record"
                ))
            })?;
            let mut parameters = desired.remove(feature_id).unwrap_or_default();
            parameters.sort_by_key(|parameter| parameter.ordinal);
            record.parameters = cadmpeg_core::text::named_entries(
                format_args!("the sldprt feature {feature_id} record"),
                parameters
                    .iter()
                    .map(|parameter| (parameter.name.clone(), parameter.expression.clone())),
            )?;
            record.dimension_properties = parameters
                .iter()
                .map(|parameter| {
                    (
                        parameter.name.clone(),
                        written_dimension_properties(parameter, true),
                    )
                })
                .collect();
            let mut names = parameters
                .iter()
                .map(|parameter| parameter.name.clone())
                .collect::<Vec<_>>()
                .into_iter();
            let mut content = record
                .content
                .iter()
                .filter_map(|item| match item {
                    FeatureContent::Dimension(_) => names.next().map(FeatureContent::Dimension),
                    other => Some(other.clone()),
                })
                .collect::<Vec<_>>();
            content.extend(names.map(FeatureContent::Dimension));
            record.content = content;
        }
    }
    // The stored scalar of an unedited parameter keeps its bytes: the value
    // the decoder derived from it does not convert back exactly.
    for parameter in &parameters {
        if !edits.parameter_edited(&parameter.id) {
            continue;
        }
        let carrier_only = carrier_only.contains(&parameter.id);
        let Some(native_ref) = parameter.native_ref.as_deref() else {
            if carrier_only {
                expression_unwritten(edits, parameter, "its PMI record");
            }
            continue;
        };
        let location = native
            .feature_input_lanes
            .iter()
            .enumerate()
            .find_map(|(lane_index, lane)| {
                lane.scalars
                    .iter()
                    .position(|scalar| scalar.id == native_ref)
                    .map(|scalar_index| (lane_index, scalar_index))
            })
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "SLDPRT parameter {} references missing scalar {native_ref}",
                    parameter.id.as_str()
                ))
            })?;
        let lane = &mut native.feature_input_lanes[location.0];
        let scalar = &mut lane.scalars[location.1];
        if scalar.role == crate::records::FeatureInputScalarRole::Display {
            return Err(CodecError::malformed(format_args!(
                "SLDPRT parameter {} references a display scalar",
                parameter.id.as_str()
            )));
        }
        // The decoder keeps an integer or boolean value only for a scalar
        // that equals it exactly.
        let value = match &parameter.value {
            Some(ParameterValue::Length(length)) => Some(length.get() / 1000.0),
            Some(ParameterValue::Angle(angle)) => Some(angle.get()),
            Some(ParameterValue::Real(value)) => Some(value.get()),
            Some(ParameterValue::Integer(value)) => exact_integer_f64(*value),
            Some(ParameterValue::Boolean(value)) => Some(if *value { 1.0 } else { 0.0 }),
            Some(ParameterValue::String(_)) | None => None,
        };
        let Some(value) = value else {
            // A parameter the scalar alone carries writes nothing.
            edits.parameter_unwritten(
                &parameter.id,
                if carrier_only {
                    UnwrittenPart::Whole
                } else {
                    UnwrittenPart::Value
                },
                format!(
                    "Parameter {} has no real value for its native scalar {}; the scalar keeps its stored value.",
                    parameter.id.as_str(),
                    scalar.id
                ),
            );
            continue;
        };
        let offset = usize::try_from(scalar.offset).map_err(|_| {
            CodecError::Malformed("SLDPRT scalar offset exceeds address space".into())
        })?;
        let bytes = lane
            .native_payload
            .get_mut(offset..offset + 8)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "SLDPRT scalar {} lies outside its payload",
                    scalar.id
                ))
            })?;
        let checked = cadmpeg_ir::scalar::FiniteReal::new(value)
            .ok_or_else(|| CodecError::malformed("SLDPRT scalar update is non-finite"))?;
        bytes.copy_from_slice(&value.to_le_bytes());
        scalar.value = checked;
        if carrier_only {
            expression_unwritten(edits, parameter, "its feature-input scalar");
        }
    }
    Ok(())
}

/// The record of feature `feature_id` in `native`.
fn record_of<'native>(
    native: &'native mut crate::native::SldprtNative,
    record_ids: &HashMap<&FeatureId, String>,
    feature_id: &FeatureId,
) -> Option<&'native mut Feature> {
    let record_id = record_ids.get(feature_id)?;
    native
        .feature_histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
        .find(|record| &record.id == record_id)
}

/// Whether the stored record `record` does not hold `parameter`: the
/// decoder read it from a feature-input scalar or a PMI record.
fn carried_outside_record(
    record: Option<&crate::records::Feature>,
    parameter: &DesignParameter,
) -> bool {
    record.is_none_or(|record| !record.parameters.contains_key(parameter.name.as_str()))
}

/// Names the expression of a parameter that only `carrier` states, when the
/// value the carrier holds does not restate it.
fn expression_unwritten(edits: &mut SourceEdits<'_>, parameter: &DesignParameter, carrier: &str) {
    if parse_parameter_literal(&parameter.expression)
        .is_some_and(|literal| parameter.value.as_ref() == Some(&literal))
    {
        return;
    }
    edits.parameter_unwritten(
        &parameter.id,
        UnwrittenPart::Expression,
        format!(
            "Parameter {} is stated only by {carrier}; its value is written there, and its expression {} is not written.",
            parameter.id.as_str(),
            parameter.expression
        ),
    );
}

fn rewrite_renamed_parameter_references(
    parameters: &mut [DesignParameter],
    original: &[DesignParameter],
    original_feature_names: &HashMap<FeatureId, String>,
    feature_names: &HashMap<FeatureId, String>,
) {
    let original = original
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    let desired = parameters
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    let mut replacements = HashMap::<ParameterId, HashMap<String, String>>::new();
    for (id, parameter) in &desired {
        let Some(previous) = original.get(id) else {
            continue;
        };
        let mut aliases = HashMap::new();
        let previous_owner_name = parameter
            .owner
            .as_ref()
            .and_then(|owner| original_feature_names.get(owner));
        let owner_name = parameter
            .owner
            .as_ref()
            .and_then(|owner| feature_names.get(owner));
        if previous.name != parameter.name {
            aliases.insert(previous.name.clone(), parameter.name.clone());
        }
        if previous.name != parameter.name || previous_owner_name != owner_name {
            if let (Some(previous_owner_name), Some(owner_name)) = (previous_owner_name, owner_name)
            {
                aliases.insert(
                    format!("{}@{previous_owner_name}", previous.name),
                    format!("{}@{owner_name}", parameter.name),
                );
            }
        }
        if let Some(previous_id) = previous.properties.get("EquationId") {
            let replacement = parameter
                .properties
                .get("EquationId")
                .unwrap_or(&parameter.name);
            if previous_id != replacement || previous_owner_name != owner_name {
                aliases.insert(previous_id.clone(), replacement.clone());
                if let (Some(previous_owner_name), Some(owner_name)) =
                    (previous_owner_name, owner_name)
                {
                    if !previous_id.contains('@') {
                        let qualified_replacement = if replacement.contains('@') {
                            replacement.clone()
                        } else {
                            format!("{replacement}@{owner_name}")
                        };
                        aliases.insert(
                            format!("{previous_id}@{previous_owner_name}"),
                            qualified_replacement,
                        );
                    }
                }
            }
        }
        if !aliases.is_empty() {
            replacements.insert((*id).clone(), aliases);
        }
    }
    for parameter in parameters {
        let aliases = parameter
            .dependencies
            .iter()
            .filter_map(|dependency| replacements.get(dependency))
            .flat_map(|aliases| aliases.iter())
            .map(|(alias, replacement)| (alias.clone(), replacement.clone()))
            .collect::<HashMap<_, _>>();
        if aliases.is_empty() {
            continue;
        }
        if let Some(rewritten) = rewrite_parameter_expression(&parameter.expression, &aliases) {
            parameter.expression = rewritten;
        }
    }
}

pub(in crate::history) fn rewrite_parameter_expression(
    expression: &str,
    aliases: &HashMap<String, String>,
) -> Option<String> {
    let tokens = expression_identifier_tokens(expression)?;
    let mut rewritten = String::with_capacity(expression.len());
    let mut tail = expression;
    let mut replaced = false;
    for token in tokens {
        if token.is_syntax() {
            continue;
        }
        let Some(replacement) = aliases.get(token.value()) else {
            continue;
        };
        let Some(preceding) = token.preceding(tail) else {
            continue;
        };
        rewritten.push_str(preceding);
        if token.is_quoted() || !unquoted_expression_identifier(replacement) {
            rewritten.push('"');
            rewritten.push_str(&replacement.replace('"', "\"\""));
            rewritten.push('"');
        } else {
            rewritten.push_str(replacement);
        }
        tail = token.following();
        replaced = true;
    }
    if !replaced {
        return None;
    }
    rewritten.push_str(tail);
    Some(rewritten)
}

pub(in crate::history) fn unquoted_expression_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    characters.next().is_some_and(|character| {
        !character.is_ascii_digit()
            && character != '.'
            && (character.is_ascii_alphanumeric() || matches!(character, '_' | '@' | '$'))
    }) && characters.all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '_' | '@' | '$' | '.')
    })
}

pub(super) fn restore_equivalent_parameter_expressions(
    feature: &Feature,
    original_parameters: &HashMap<String, BTreeMap<cadmpeg_core::text::NonBlankString, String>>,
    evaluated_parameters: &HashMap<String, BTreeMap<cadmpeg_core::text::NonBlankString, String>>,
    desired_parameters: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
) {
    let Some(original) = original_parameters.get(&feature.id) else {
        return;
    };
    let Some(evaluated) = evaluated_parameters.get(&feature.id) else {
        return;
    };
    for (name, desired) in desired_parameters {
        let Some(expression) = original.get(name) else {
            continue;
        };
        if parse_native_parameter_literal(feature, name.as_str(), expression).is_some() {
            continue;
        }
        let Some(evaluated) = evaluated.get(name) else {
            continue;
        };
        let Some(desired_value) = parse_native_parameter_literal(feature, name.as_str(), desired)
        else {
            continue;
        };
        let Some(evaluated_value) =
            parse_native_parameter_literal(feature, name.as_str(), evaluated)
        else {
            continue;
        };
        if desired_value == evaluated_value {
            desired.clone_from(expression);
        }
    }
}

/// The relative difference up to which a stored dimension text states its
/// millimetre value.
const STORED_SCALE_TOLERANCE: f64 = 1.0e-9;

/// States each edited text of `written` in the scale of the text the record
/// stored for it, `stored`. A record stores a length dimension's text as a
/// number in the part's display unit, after a radius or diameter sign, and
/// its value in the lane scalar. Where the stored number is not the
/// millimetre value `stated` gives for it, the edited millimetre number is
/// scaled by the same ratio.
pub(super) fn state_in_stored_scale(
    stored: &BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    stated: impl Fn(&str) -> Option<f64>,
    written: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
) {
    // A dimension text: an optional radius or diameter sign, then a number.
    let number = |text: &str| -> Option<(String, f64)> {
        let text = text.trim();
        let digits = text.find(|character: char| {
            character.is_ascii_digit() || character == '-' || character == '.'
        })?;
        let (sign, number) = text.split_at(digits);
        if !matches!(sign, "" | "R" | "r" | "\u{2300}" | "\u{00d8}") {
            return None;
        }
        let number = number
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())?;
        Some((sign.to_owned(), number))
    };
    for (name, text) in written.iter_mut() {
        let Some(original) = stored.get(name).filter(|original| *original != text) else {
            continue;
        };
        let (Some((sign, original_number)), Some((edited_sign, edited)), Some(value)) =
            (number(original), number(text), stated(name.as_str()))
        else {
            continue;
        };
        if sign != edited_sign
            || value == 0.0
            || (original_number - value).abs()
                <= STORED_SCALE_TOLERANCE * original_number.abs().max(value.abs()).max(1.0)
        {
            continue;
        }
        if let Some(scaled) = cadmpeg_ir::scalar::FiniteReal::new(edited * original_number / value)
        {
            *text = format!(
                "{sign}{}",
                crate::history::literals::format_f64_literal(scaled)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{omitted_readers, without_omitted_driven};
    use cadmpeg_ir::features::parameters::{DesignParameter, ParameterValue};
    use cadmpeg_ir::features::ParameterId;
    use std::collections::HashSet;

    fn parameter(
        key: &str,
        expression: &str,
        value: Option<f64>,
        reads: &[&ParameterId],
    ) -> DesignParameter {
        DesignParameter {
            product_definition: None,
            id: ParameterId::mint(format!("synthetic:test:parameter#{key}")).expect("id"),
            owner: None,
            ordinal: 0,
            name: key.into(),
            expression: expression.into(),
            display: None,
            value: value.map(|millimetres| {
                ParameterValue::Length(
                    cadmpeg_ir::scalar::Length::new(millimetres).expect("length"),
                )
            }),
            dependencies: reads.iter().map(|&id| id.clone()).collect(),
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: None,
        }
    }

    /// The driven parameter is left out; a parameter that reads it is
    /// written with its value and reads nothing; another keeps its
    /// expression. A reader with no value is left out too, and its own
    /// readers are valued or left out in turn.
    #[test]
    fn a_parameter_reading_an_omitted_driven_one_is_written_with_its_value() {
        let driven = parameter("D2", "6mm", Some(6.0), &[]);
        let width = parameter("D1", "4mm", Some(4.0), &[]);
        let twice = parameter("D3", "\"D2@Driven\" * 2", Some(12.0), &[&driven.id]);
        let wide = parameter("D4", "\"D1@Driven\" + 1mm", Some(5.0), &[&width.id]);
        let valueless = parameter("D5", "\"D2@Driven\" * 3", None, &[&driven.id]);
        let after = parameter("D6", "\"D5@Driven\" + 1mm", Some(19.0), &[&valueless.id]);
        let unvalued = parameter("D7", "\"D5@Driven\" * 2", None, &[&valueless.id]);
        let omitted = HashSet::from([&driven.id]);
        let all = vec![
            driven.clone(),
            width.clone(),
            twice,
            wide.clone(),
            valueless,
            after,
            unvalued,
        ];
        let readers = omitted_readers(&all, &omitted);
        let names = |readers: &[&DesignParameter]| {
            let mut names = readers
                .iter()
                .map(|parameter| parameter.name.clone())
                .collect::<Vec<_>>();
            names.sort_unstable();
            names
        };
        assert_eq!(names(&readers.valued), ["D3", "D6"]);
        assert_eq!(names(&readers.dropped), ["D5", "D7"]);

        let written = without_omitted_driven(all, &omitted);
        let by_name = |name: &str| written.iter().find(|parameter| parameter.name == name);
        for left_out in ["D2", "D5", "D7"] {
            assert!(by_name(left_out).is_none(), "{left_out} in {written:?}");
        }
        let valued = |name: &str| by_name(name).unwrap_or_else(|| panic!("{name} in {written:?}"));
        assert_eq!(valued("D3").expression, "12mm");
        assert!(valued("D3").dependencies.is_empty());
        assert_eq!(valued("D6").expression, "19mm");
        assert!(valued("D6").dependencies.is_empty());
        assert_eq!(valued("D4").expression, wide.expression);
        assert_eq!(valued("D4").dependencies, wide.dependencies);
        assert_eq!(*valued("D1"), width);
    }
}
