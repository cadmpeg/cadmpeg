// SPDX-License-Identifier: Apache-2.0
//! Neutral feature synchronization into native history records.

use crate::records::{Feature, FeatureContent, FeatureHistory, HistoryContent};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::parameters::DesignParameter;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, FeatureOperation, FeatureSourceContent};
use cadmpeg_ir::topology::Body;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::history::classify::{
    feature_tree_node_role, is_custom_property, principal_plane_in_history,
};
use crate::history::configuration::enrich_history_parameters_semantic;
use crate::history::parameters::apply_evaluated_parameters;
use crate::history::project::{neutral_feature_id, referenced_sources};

use super::parameters::restore_equivalent_parameter_expressions;
use super::project_features_with_native_inputs;
use super::xml::{feature_xml_tag, solidworks_source_tag};
use crate::history::encode::{NeutralFeatureEncoder, NeutralFeatureEncoding};
use crate::history::extrude_direction::cleared_reversal;
use crate::loss::SldprtLossCode;
use crate::records::FeatureSource;
use crate::resolved_features::terminations::EndSpecAnchor;

/// What preparing the native history for a write produced.
#[derive(Debug, Default)]
pub(crate) struct PreparedFeatures {
    /// The serialized object names to rewrite.
    pub(crate) renames: Vec<FeatureInputRename>,
    /// The feature edits the written records hold only in part, or that the
    /// stored solid does not show.
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
}

/// A write-side request to rewrite one serialized object name.
///
/// A stored `FeatureInputName::value` states the payload bytes at that record's
/// offset and is never edited. A neutral feature rename is this distinct input,
/// which the writer splices into the payload it emits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureInputRename {
    /// Identifier of the lane that owns the renamed object name.
    pub(crate) lane: String,
    /// Position of the renamed record in that lane's `names` arena.
    pub(crate) name_index: usize,
    /// The object name to write.
    pub(crate) value: cadmpeg_core::text::NonBlankString,
}

fn synchronize_feature_input_names(
    features: &[cadmpeg_ir::features::Feature],
    native: &crate::native::SldprtNative,
) -> Result<Vec<FeatureInputRename>, CodecError> {
    let renames = features
        .iter()
        .filter_map(|feature| {
            let record = native
                .feature_histories
                .iter()
                .flat_map(|history| &history.features)
                .find(|record| feature.native_ref.as_deref() == Some(record.id.as_str()))?;
            let new_name = feature.name.as_deref().unwrap_or_default();
            if new_name == record.name {
                return None;
            }
            Some((
                record.name.clone(),
                new_name.to_string(),
                record.input_class.clone()?,
            ))
        })
        .collect::<Vec<_>>();

    let mut requested = Vec::new();
    for (old_name, new_name, input_class) in renames {
        let mut matches = Vec::<(usize, usize)>::new();
        for (lane_index, lane) in native.feature_input_lanes.iter().enumerate() {
            for class in lane
                .classes
                .iter()
                .filter(|class| class.name == input_class)
            {
                let name_offset =
                    class.offset + 6 + cadmpeg_core::decode::u64_from_index(class.name.len());
                if let Some((name_index, _)) = lane
                    .names
                    .iter()
                    .enumerate()
                    .find(|(_, name)| name.offset == name_offset && name.value == old_name)
                {
                    matches.push((lane_index, name_index));
                }
            }
        }
        let [(lane_index, name_index)] = matches.as_slice() else {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT feature-input name for {old_name:?} is not uniquely linked"
            )));
        };
        let value = cadmpeg_core::text::NonBlankString::new(new_name).ok_or_else(|| {
            CodecError::NotImplemented(format!(
                "SLDPRT feature-input name for {old_name:?} has no non-blank replacement"
            ))
        })?;
        requested.push(FeatureInputRename {
            lane: native.feature_input_lanes[*lane_index].id.clone(),
            name_index: *name_index,
            value,
        });
    }
    Ok(requested)
}

pub(crate) fn generated_feature_record_id(feature: &FeatureId) -> String {
    format!(
        "sldprt:generated:feature#{}",
        feature.as_str().replace('%', "%25").replace('#', "%23")
    )
}

fn generated_feature_source_ids(
    features: &[cadmpeg_ir::features::Feature],
    native: &crate::native::SldprtNative,
) -> Result<HashMap<FeatureId, FeatureSource>, CodecError> {
    let held = native.object_ids().collect::<HashSet<_>>();
    let existing = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .filter_map(|feature| Some((feature.id.as_str(), feature.source_value()?)))
        .collect::<HashMap<_, _>>();
    // An added feature takes an identifier past every identifier the part's
    // objects hold, and the lowest one they do not hold once those run out.
    // A part that holds none is numbered as a generated part is.
    let start = held
        .iter()
        .max()
        .map_or(Some(crate::writer::native::ids::FIRST_USER), |largest| {
            largest.checked_add(1)
        });
    let below = (1..u32::MAX).take_while(|&candidate| start.is_none_or(|start| candidate < start));
    let mut free = start
        .into_iter()
        .flat_map(|start| start..u32::MAX)
        .chain(below)
        .filter(|candidate| !held.contains(candidate));
    let mut allocated = HashMap::new();
    for feature in features
        .iter()
        .filter(|feature| feature.native_ref.is_none())
    {
        let record_id = generated_feature_record_id(&feature.id);
        let source_id = match existing.get(record_id.as_str()) {
            Some(&source_id) => Some(source_id),
            None => free.next(),
        }
        .and_then(FeatureSource::from_value)
        .ok_or_else(|| {
            CodecError::NotImplemented("SLDPRT feature source-id space is exhausted".into())
        })?;
        allocated.insert(feature.id.clone(), source_id);
    }
    Ok(allocated)
}

/// Apply neutral native-feature edits to the `SolidWorks` history used for writing.
pub(in crate::history) fn sync_neutral_features(
    ir: &cadmpeg_ir::CadIr,
    parameters: &[DesignParameter],
    bodies: &[Body],
    native: &mut Option<crate::native::SldprtNative>,
) -> Result<PreparedFeatures, CodecError> {
    let features = &ir.model.features;
    if features.is_empty() {
        if let Some(native) = native {
            for history in &mut native.feature_histories {
                history.features.retain(is_custom_property);
            }
        }
        return Ok(PreparedFeatures::default());
    }
    let native = native.get_or_insert_with(|| crate::native::SldprtNative {
        feature_histories: vec![FeatureHistory {
            id: "sldprt:generated:feature-history#0".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: Vec::new(),
        }],
        feature_input_lanes: Vec::new(),
        pmi_dimensions: Vec::new(),
    });
    let contradicted_travel = crate::history::extrude_direction::contradicted_features(ir);
    let original_parameters = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.clone(), feature.parameters.clone()))
        .collect::<HashMap<_, _>>();
    let mut resolved_histories = native.feature_histories.clone();
    let lane_bytes = native
        .feature_input_lanes
        .iter()
        .flat_map(|lane| lane.native_payload.iter().copied())
        .collect::<Vec<_>>();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &lane_bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )?;
    // The end specs whose reversal flag an edit clears, read from the stored
    // records as the decoder read them. A scan the context refuses finds no
    // flag, and the write names each reversal it could not clear.
    let reversed_end_specs = if features.iter().any(cleared_reversal) {
        crate::resolved_features::terminations::reversed_end_specs(
            &ctx,
            &native.feature_histories,
            &native.feature_input_lanes,
        )
        .unwrap_or_default()
    } else {
        HashMap::new()
    };
    let mut cleared = Vec::new();
    enrich_history_parameters_semantic(&mut resolved_histories, &native.feature_input_lanes);
    let resolved_parameter_names = resolved_histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| {
            (
                feature.id.clone(),
                feature.parameters.keys().cloned().collect::<HashSet<_>>(),
            )
        })
        .collect::<HashMap<_, _>>();
    apply_evaluated_parameters(&ctx, &mut resolved_histories)?;
    let evaluated_parameters = resolved_histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.clone(), feature.parameters.clone()))
        .collect::<HashMap<_, _>>();
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

    let renames = synchronize_feature_input_names(features, native)?;

    let generated_sources = generated_feature_source_ids(features, native)?;
    let parent_sources = features
        .iter()
        .map(|feature| {
            let source_id = native
                .feature_histories
                .iter()
                .flat_map(|history| &history.features)
                .find(|candidate| feature.native_ref.as_deref() == Some(candidate.id.as_str()))
                .and_then(|candidate| candidate.source_id)
                .or_else(|| generated_sources.get(&feature.id).copied())
                .map_or_else(|| feature.id.as_str().to_owned(), String::from);
            (feature.id.clone(), source_id)
        })
        .collect::<HashMap<_, _>>();
    let structural_parent_sources = features
        .iter()
        .map(|feature| {
            let source_id = native
                .feature_histories
                .iter()
                .flat_map(|history| &history.features)
                .find(|candidate| feature.native_ref.as_deref() == Some(candidate.id.as_str()))
                .and_then(|candidate| candidate.source_id)
                .or_else(|| generated_sources.get(&feature.id).copied());
            (feature.id.clone(), source_id)
        })
        .collect::<HashMap<_, _>>();
    let record_ids = features
        .iter()
        .map(|feature| {
            let record_id = feature
                .native_ref
                .clone()
                .unwrap_or_else(|| generated_feature_record_id(&feature.id));
            (feature.id.clone(), record_id)
        })
        .collect::<HashMap<_, _>>();
    let desired_record_ids = record_ids
        .values()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    for history in &mut native.feature_histories {
        history.features.retain(|feature| {
            is_custom_property(feature) || desired_record_ids.contains(&feature.id)
        });
    }
    let principal_planes_by_record = native
        .feature_histories
        .iter()
        .flat_map(|history| {
            let by_source = history
                .features
                .iter()
                .filter_map(|feature| Some((feature.source_id?, feature)))
                .collect::<HashMap<_, _>>();
            history.features.iter().filter_map(move |feature| {
                Some((
                    feature.id.clone(),
                    principal_plane_in_history(feature, &by_source, &history.features)?,
                ))
            })
        })
        .collect::<HashMap<_, _>>();
    let record_sources = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .filter_map(|feature| {
            feature
                .source_id
                .map(|source| (feature.id.clone(), String::from(source)))
        })
        .collect::<HashMap<_, _>>();
    let retained_tree_node_roles = native
        .feature_histories
        .iter()
        .flat_map(|history| {
            history.features.iter().filter_map(|feature| {
                Some((
                    feature.id.clone(),
                    feature_tree_node_role(feature, &history.features)?,
                ))
            })
        })
        .collect::<HashMap<_, _>>();
    let feature_sources = features
        .iter()
        .filter_map(|feature| {
            Some((
                &feature.id,
                record_sources.get(feature.native_ref.as_ref()?)?.as_str(),
            ))
        })
        .collect::<HashMap<_, _>>();
    let sketch_sources = features
        .iter()
        .filter_map(|feature| match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
            }) => parent_sources
                .get(&feature.id)
                .map(|source| (sketch.clone(), source.clone())),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let body_sources = bodies
        .iter()
        .map(|body| (body.id.clone(), body.id.as_str().to_owned()))
        .collect::<HashMap<_, _>>();
    for feature in features {
        let mut existing = native
            .feature_histories
            .iter_mut()
            .flat_map(|history| &mut history.features)
            .find(|candidate| feature.native_ref.as_deref() == Some(candidate.id.as_str()));
        let suppressed = feature
            .suppressed
            .or_else(|| existing.as_deref().map(|record| record.suppressed))
            .ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "SLDPRT writing requires resolved suppression for feature {}",
                    feature.id
                ))
            })?;
        let NeutralFeatureEncoding {
            kind,
            mut parameters,
            mut properties,
        } = NeutralFeatureEncoder {
            contradicted_travel: contradicted_travel.contains(&feature.id),
            feature,
            existing: existing.as_deref(),
            principal_planes_by_record: &principal_planes_by_record,
            record_sources: &record_sources,
            retained_tree_node_roles: &retained_tree_node_roles,
            feature_sources: &feature_sources,
            sketch_sources: &sketch_sources,
            parent_sources: &parent_sources,
            resolved_parameter_names: &resolved_parameter_names,
        }
        .encode()?;
        if cleared_reversal(feature) {
            let stated = existing
                .as_deref()
                .is_some_and(|record| record.properties.contains_key("Reverse"));
            cleared.push((feature, stated));
        }
        if let Some(record) = existing.as_deref() {
            restore_equivalent_parameter_expressions(
                record,
                &original_parameters,
                &evaluated_parameters,
                &mut parameters,
            );
        } else {
            state_unreferenced_dependencies(feature, &structural_parent_sources, &mut properties);
        }
        if feature.evaluation.outputs().is_empty() {
            if existing.is_none() {
                properties.remove("Scope");
            }
        } else {
            let scope = feature
                .evaluation
                .outputs()
                .iter()
                .map(|body| body_sources.get(body).cloned())
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "SLDPRT feature {} references a missing output body",
                        feature.id
                    ))
                })?;
            properties.insert(cadmpeg_core::nonblank_literal!("Scope"), scope.join(","));
        }
        let ordinal = u32::try_from(feature.ordinal)
            .map_err(|_| CodecError::NotImplemented("feature ordinal exceeds u32".into()))?;
        let tree_parent = ir.model.feature_parent(&feature.id).and_then(|parent| {
            let source_id = structural_parent_sources.get(parent).copied().flatten();
            match record_ids.get(parent) {
                Some(record_id) => Some(crate::records::TreeParent::Record {
                    record_id: record_id.clone(),
                    source_id,
                }),
                None => source_id.map(crate::records::TreeParent::Source),
            }
        });
        if let Some(existing) = existing.as_mut() {
            if let Some(tag) = solidworks_source_tag(feature) {
                existing.xml_tag.clone_from(tag);
            }
            existing.ordinal = ordinal;
            existing.name = feature.name.clone().unwrap_or_default();
            existing.kind = kind;
            existing.suppressed = suppressed;
            existing.tree_parent = tree_parent;
            existing.parameters = parameters;
            existing.properties = properties;
            if existing
                .content
                .iter()
                .all(|item| matches!(item, FeatureContent::Text(_)))
            {
                existing.content = feature
                    .source_text
                    .iter()
                    .cloned()
                    .map(FeatureContent::Text)
                    .collect();
            }
            existing.text.clone_from(&feature.source_text);
        } else {
            let history = &mut native.feature_histories[0];
            history.features.push(Feature {
                id: record_ids[&feature.id].clone(),
                parent: history.id.clone(),
                xml_tag: feature_xml_tag(feature),
                tree_parent,
                source_id: generated_sources.get(&feature.id).copied(),
                ordinal,
                name: feature.name.clone().unwrap_or_default(),
                kind,
                input_class: None,
                suppressed,
                parameters,
                dimension_properties: BTreeMap::new(),
                properties,
                text: feature.source_text.clone(),
                content: feature
                    .source_text
                    .iter()
                    .cloned()
                    .map(FeatureContent::Text)
                    .collect(),
            });
        }
    }
    let losses = clear_reversals(
        &cleared,
        &reversed_end_specs,
        &mut native.feature_input_lanes,
    );
    synchronize_neutral_feature_content(features, parameters, &record_ids, native)?;
    let changed_parameters = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .flat_map(|feature| {
            let original = original_parameters.get(&feature.id);
            feature
                .parameters
                .iter()
                .filter(move |(name, expression)| {
                    original.and_then(|parameters| parameters.get(*name)) != Some(*expression)
                })
                .map(move |(name, _)| (feature.id.clone(), name.clone()))
        })
        .collect::<std::collections::HashSet<_>>();
    crate::resolved_features::parameters::sync_changed_feature_scalars(
        &native.feature_histories,
        &mut native.feature_input_lanes,
        &changed_parameters,
    )?;
    let projected_features = project_features_with_native_inputs(native)?;
    let projected_features = projected_features
        .into_iter()
        .map(|feature| (feature.id.clone(), feature))
        .collect::<HashMap<_, _>>();
    for feature in features {
        let projected_id = neutral_feature_id(&record_ids[&feature.id])?;
        let expected = feature
            .dependencies
            .iter()
            .map(|dependency| match record_ids.get(dependency) {
                Some(record) => neutral_feature_id(record),
                None => Ok(dependency.clone()),
            })
            .collect::<Result<Vec<_>, CodecError>>()?;
        let consistent = projected_features
            .get(&projected_id)
            .is_some_and(|projected| {
                if feature.native_ref.is_some() {
                    projected.dependencies.as_slice() == expected
                } else {
                    expected
                        .iter()
                        .all(|dependency| projected.dependencies.contains(dependency))
                }
            });
        if !consistent {
            return Err(CodecError::malformed(format_args!(
                "SLDPRT feature {} dependencies are inconsistent with its operands",
                feature.id
            )));
        }
    }
    synchronize_feature_content_order(native);
    synchronize_history_content_order(native);
    Ok(PreparedFeatures { renames, losses })
}

/// Writes `0` to the end-spec reversal word of each extrusion whose decoded
/// reversal an edit cleared, each paired with whether its record states the
/// flag as a Keywords attribute, which the encoder already dropped. Names
/// each extrusion whose flag the write could not clear.
fn clear_reversals(
    cleared: &[(&cadmpeg_ir::features::Feature, bool)],
    reversed_end_specs: &HashMap<String, Vec<EndSpecAnchor>>,
    lanes: &mut [crate::records::FeatureInputLane],
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    use crate::layout::extrusion_end_spec_header::REVERSAL;
    let mut losses = Vec::new();
    for (feature, stated) in cleared {
        let anchors = feature
            .native_ref
            .as_deref()
            .and_then(|record| reversed_end_specs.get(record))
            .map_or(&[][..], Vec::as_slice);
        let mut unwritten = anchors.is_empty() && !stated;
        for anchor in anchors {
            let at = anchor.offset + REVERSAL;
            match lanes
                .get_mut(anchor.lane)
                .and_then(|lane| lane.native_payload.get_mut(at..at + 4))
            {
                Some(word) => word.copy_from_slice(&0_u32.to_le_bytes()),
                None => unwritten = true,
            }
        }
        let extrusion = crate::history::extrude_direction::label(feature);
        if unwritten {
            losses.push(SldprtLossCode::WriterExtrudeReversalUnwritten.note(format!(
                "Extrusion {extrusion} is edited to travel along the profile normal, and the writer found no end-spec reversal flag to clear, so the written record keeps the stored reversal."
            )));
        }
    }
    losses
}

fn synchronize_neutral_feature_content(
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[DesignParameter],
    record_ids: &HashMap<FeatureId, String>,
    native: &mut crate::native::SldprtNative,
) -> Result<(), CodecError> {
    let parameters = parameters
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    for feature in features {
        if feature.source_content.is_empty() {
            continue;
        }
        let content = feature
            .source_content
            .iter()
            .map(|item| match item {
                FeatureSourceContent::Text(text) => Ok(FeatureContent::Text(text.clone())),
                FeatureSourceContent::Parameter(id) => {
                    let parameter = parameters.get(id).ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "SLDPRT feature {} content references missing parameter {}",
                            feature.id, id.as_str()
                        ))
                    })?;
                    if parameter.owner.as_ref() != Some(&feature.id) {
                        return Err(CodecError::malformed(format_args!(
                            "SLDPRT feature {} content references parameter {} owned by another feature",
                            feature.id, id.as_str()
                        )));
                    }
                    Ok(FeatureContent::Dimension(parameter.name.clone()))
                }
                FeatureSourceContent::Feature(id) => record_ids
                    .get(id)
                    .cloned()
                    .map(FeatureContent::Feature)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "SLDPRT feature {} content references missing feature {}",
                            feature.id, id
                        ))
                    }),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let record_id = &record_ids[&feature.id];
        let record = native
            .feature_histories
            .iter_mut()
            .flat_map(|history| &mut history.features)
            .find(|record| &record.id == record_id)
            .ok_or_else(|| CodecError::Malformed("missing SLDPRT feature record".into()))?;
        record.content = content;
    }
    Ok(())
}

/// Name in `Dependencies` each dependency of a generated record that none of
/// its feature-reference properties names, so the record projects with every
/// dependency its feature states. A dependency with no source id stays
/// unnamed.
fn state_unreferenced_dependencies(
    feature: &cadmpeg_ir::features::Feature,
    sources: &HashMap<FeatureId, Option<FeatureSource>>,
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
) {
    let referenced = referenced_sources(properties)
        .map(str::to_owned)
        .collect::<HashSet<_>>();
    let unreferenced = feature
        .dependencies
        .iter()
        .filter_map(|dependency| sources.get(dependency).copied().flatten())
        .map(String::from)
        .filter(|source| !referenced.contains(source))
        .collect::<Vec<_>>();
    if unreferenced.is_empty() {
        return;
    }
    let key = cadmpeg_core::nonblank_literal!("Dependencies");
    let value = match properties.get(key.as_str()) {
        Some(stated) => format!("{stated},{}", unreferenced.join(",")),
        None => unreferenced.join(","),
    };
    properties.insert(key, value);
}

pub(super) fn synchronize_history_content_order(native: &mut crate::native::SldprtNative) {
    for history in &mut native.feature_histories {
        let configurations = history
            .configurations
            .iter()
            .map(|configuration| (configuration.ordinal, configuration.id.clone()))
            .collect::<Vec<_>>();
        let mut features = history
            .features
            .iter()
            .filter(|feature| feature.tree_parent.is_none())
            .map(|feature| (feature.ordinal, feature.id.clone()))
            .collect::<Vec<_>>();
        let mut configurations = configurations;
        configurations.sort();
        features.sort();
        let mut configuration_index = 0;
        let mut feature_index = 0;
        for item in &mut history.content {
            match item {
                HistoryContent::Configuration(id) => {
                    *id = configurations
                        .get(configuration_index)
                        .map_or_else(String::new, |(_, id)| id.clone());
                    configuration_index += 1;
                }
                HistoryContent::Feature(id) => {
                    *id = features
                        .get(feature_index)
                        .map_or_else(String::new, |(_, id)| id.clone());
                    feature_index += 1;
                }
                HistoryContent::Text(_) | HistoryContent::Element(_) => {}
            }
        }
        history.content.retain(|item| {
            !matches!(item, HistoryContent::Configuration(id) | HistoryContent::Feature(id) if id.is_empty())
        });
        history.content.extend(
            configurations
                .iter()
                .skip(configuration_index)
                .map(|(_, id)| HistoryContent::Configuration(id.clone())),
        );
        history.content.extend(
            features
                .iter()
                .skip(feature_index)
                .map(|(_, id)| HistoryContent::Feature(id.clone())),
        );
    }
}

fn synchronize_feature_content_order(native: &mut crate::native::SldprtNative) {
    for history in &mut native.feature_histories {
        let mut children = HashMap::<String, Vec<(u32, String)>>::new();
        for feature in &history.features {
            if let Some(parent) = feature.tree_parent_record_id() {
                children
                    .entry(parent.to_owned())
                    .or_default()
                    .push((feature.ordinal, feature.id.clone()));
            }
        }
        for values in children.values_mut() {
            values.sort();
        }
        for feature in &mut history.features {
            let Some(children) = children.get(&feature.id) else {
                feature
                    .content
                    .retain(|item| !matches!(item, FeatureContent::Feature(_)));
                continue;
            };
            let mut index = 0;
            for item in &mut feature.content {
                if matches!(item, FeatureContent::Feature(_)) {
                    *item = FeatureContent::Feature(
                        children
                            .get(index)
                            .map_or_else(String::new, |(_, id)| id.clone()),
                    );
                    index += 1;
                }
            }
            feature
                .content
                .retain(|item| !matches!(item, FeatureContent::Feature(id) if id.is_empty()));
            feature.content.extend(
                children
                    .iter()
                    .skip(index)
                    .map(|(_, id)| FeatureContent::Feature(id.clone())),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{generated_feature_record_id, neutral_feature_id, sync_neutral_features};
    use crate::test_support::container::make_block;
    use crate::test_support::container::sldprt_with_body;
    use crate::test_support::history::resolved_feature_classes_with_ids;
    use crate::test_support::native::sldprt_native;
    use crate::test_support::parasolid::triangle_body;
    use crate::test_support::plan_inherited_write;
    use crate::SldprtCodec;
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::features::FeatureId;
    use std::io::Cursor;

    /// A document whose one history feature is linked to one serialized object
    /// name through a feature-input class declaration, so a neutral rename has
    /// a name to reach.
    fn document_with_one_linked_object_name() -> Vec<u8> {
        let mut source = sldprt_with_body(&triangle_body());
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            br#"<Keywords><Feature Name="MatrizL1" Type="MatrizL" id="132"><Dimension Name="D1">15</Dimension></Feature></Keywords>"#,
        ));
        source.extend(make_block(
            0x42,
            "Contents/Config-0-ResolvedFeatures",
            &resolved_feature_classes_with_ids(&[("moLPattern_c", "MatrizL1", 132)]),
        ));
        source
    }

    #[test]
    fn a_neutral_feature_rename_reaches_the_written_object_name_and_survives_a_round_trip() {
        let decoded = SldprtCodec
            .decode(
                &mut Cursor::new(crate::test_support::container::native_envelope_fixture(
                    &(document_with_one_linked_object_name()),
                )),
                &DecodeOptions::default(),
            )
            .expect("the fixture decodes");
        assert_eq!(
            decoded.ir().model.features[0].name.as_deref(),
            Some("MatrizL1")
        );
        assert_eq!(
            sldprt_native(decoded.ir()).feature_input_lanes[0].names[0].value,
            "MatrizL1"
        );

        let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
        {
            let mut edit = decoded.ir_mut();
            edit.model.features[0].name = Some("PatternRenamed".into());
        }
        let mut encoded = Vec::new();
        plan_inherited_write(decoded.ir(), decoded.source_fidelity(), &mut encoded)
            .expect("the renamed model writes");

        let regenerated = SldprtCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("the written document decodes");
        let native = sldprt_native(regenerated.ir());
        assert_eq!(
            native.feature_input_lanes[0].names[0].value,
            "PatternRenamed"
        );
        assert_eq!(
            native.feature_histories[0].features[0].name,
            "PatternRenamed"
        );
        assert_eq!(
            native.feature_histories[0].features[0]
                .input_class
                .as_deref(),
            Some("moLPattern_c")
        );
        assert_eq!(
            regenerated.ir().model.features[0].name.as_deref(),
            Some("PatternRenamed")
        );
    }

    #[test]
    fn a_rename_of_an_object_name_two_lanes_state_is_refused_naming_the_name() {
        let decoded = SldprtCodec
            .decode(
                &mut Cursor::new(document_with_one_linked_object_name()),
                &DecodeOptions::default(),
            )
            .expect("the fixture decodes");
        let mut ir = decoded.ir().clone();
        ir.model.features[0].name = Some("PatternRenamed".into());

        let mut native = sldprt_native(decoded.ir());
        let mut second = native.feature_input_lanes[0].clone();
        second.id = format!("{}-second", second.id);
        native.feature_input_lanes.push(second);

        let error = sync_neutral_features(&ir, &[], &[], &mut Some(native))
            .expect_err("two lanes state the same object name");
        assert_eq!(
            error.to_string(),
            "not implemented yet: SLDPRT feature-input name for \"MatrizL1\" is not uniquely linked"
        );
    }

    #[test]
    fn a_rename_to_a_blank_feature_name_is_refused_naming_the_name() {
        let decoded = SldprtCodec
            .decode(
                &mut Cursor::new(document_with_one_linked_object_name()),
                &DecodeOptions::default(),
            )
            .expect("the fixture decodes");
        let mut ir = decoded.ir().clone();
        ir.model.features[0].name = Some(" ".into());

        let error = sync_neutral_features(&ir, &[], &[], &mut Some(sldprt_native(decoded.ir())))
            .expect_err("a blank object name is not writable");
        assert_eq!(
            error.to_string(),
            "not implemented yet: SLDPRT feature-input name for \"MatrizL1\" has no non-blank replacement"
        );
    }

    #[test]
    fn a_solidworks_source_tag_xml_cannot_hold_keeps_the_record_tag_and_is_named() {
        let decoded = SldprtCodec
            .decode(
                &mut Cursor::new(document_with_one_linked_object_name()),
                &DecodeOptions::default(),
            )
            .expect("the fixture decodes");
        for (tag, written, named) in [
            ("Pattern Geometry", "Feature", true),
            ("PartDesign::Pad", "Feature", true),
            ("Schlüssel", "Schlüssel", false),
        ] {
            let mut ir = decoded.ir().clone();
            ir.model.features[0].source_tag = Some(tag.into());
            let mut native = Some(sldprt_native(decoded.ir()));
            sync_neutral_features(&ir, &ir.model.parameters, &ir.geometry.bodies, &mut native)
                .expect("the history is written");
            let mut native = native.expect("native history");
            let notes = crate::writer::keywords_text::readable_keywords(&ir, &mut native);
            assert_eq!(
                native.feature_histories[0].features[0].xml_tag, written,
                "{tag}"
            );
            let names = notes
                .iter()
                .filter(|note| {
                    note.code == crate::loss::SldprtLossCode::WriterHistoryNameNotWritten.kind()
                })
                .collect::<Vec<_>>();
            assert_eq!(names.len(), usize::from(named), "{tag}: {notes:?}");
            assert!(
                names
                    .iter()
                    .all(|note| note.message.contains(&format!("{tag:?} as <Feature>"))),
                "{names:?}"
            );
        }
    }

    #[test]
    fn generated_feature_identities_escape_embedded_separators() {
        let feature =
            FeatureId::mint("test:model:feature#original%23key").expect("fixture identity");
        let record = generated_feature_record_id(&feature);
        assert_eq!(
            record,
            "sldprt:generated:feature#test:model:feature%23original%2523key"
        );
        assert!(cadmpeg_ir::ids::is_valid_identity(&record));
        let projected = neutral_feature_id(&record).unwrap();
        assert!(cadmpeg_ir::ids::is_valid_identity(projected.as_str()));
    }
    /// Decode `source`, add a datum point named `Added` that has no native
    /// record, write the part and decode the written bytes.
    fn written_with_an_added_datum_point(source: &[u8]) -> crate::native::SldprtNative {
        use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
        let decoded = SldprtCodec
            .decode(
                &mut Cursor::new(crate::test_support::container::native_envelope_fixture(
                    source,
                )),
                &DecodeOptions::default(),
            )
            .expect("the fixture decodes");
        let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
        let ordinal = u64::try_from(decoded.ir().model.features.len()).expect("ordinal");
        decoded.ir_mut().model.features.push(Feature {
            product_definition: None,
            id: FeatureId::mint("synthetic:test:feature#added").expect("fixture identity"),
            ordinal,
            name: Some("Added".into()),
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::DatumPoint {
                    position: cadmpeg_ir::features::geometry::FinitePoint3::new(
                        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                    )
                    .expect("a finite point"),
                    construction: None,
                }),
            ),
            native_ref: None,
        });
        let mut encoded = Vec::new();
        plan_inherited_write(decoded.ir(), decoded.source_fidelity(), &mut encoded)
            .expect("the edited part writes");
        let written = SldprtCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("the written part decodes");
        sldprt_native(written.ir())
    }

    /// The history source identifiers of the records named `name`.
    fn source_ids(native: &crate::native::SldprtNative, name: &str) -> Vec<Option<u32>> {
        native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .filter(|record| record.name == name)
            .map(crate::records::Feature::source_value)
            .collect()
    }

    /// An added feature takes an identifier past every identifier the part's
    /// objects hold: past the history record `132` and past both parameter
    /// objects, an integer parameter and a parameter with a tolerance, which
    /// hold their identifiers after the trailer's zero byte and tolerance
    /// pointer (sldprt.md §2.3). Either parameter can hold the largest.
    #[test]
    fn an_added_feature_takes_an_identifier_past_every_identifier_the_part_holds() {
        use crate::test_support::carchive::{
            config_object_stream, toleranced_and_integer_parameters_stream,
        };
        for (integer, toleranced) in [(135, 134), (134, 135)] {
            let mut source = sldprt_with_body(&triangle_body());
            source.extend(make_block(
                0x42,
                "Contents/Keywords",
                br#"<Keywords><Feature Name="MatrizL1" Type="MatrizL" id="132"><Dimension Name="D1">15</Dimension></Feature></Keywords>"#,
            ));
            source.extend(make_block(
                0x20,
                "Contents/Config-0",
                &config_object_stream(),
            ));
            source.extend(make_block(
                0x21,
                "Contents/Config-0-ResolvedFeatures",
                &toleranced_and_integer_parameters_stream(integer, toleranced),
            ));
            let decoded = SldprtCodec
                .decode(&mut Cursor::new(source.clone()), &DecodeOptions::default())
                .expect("the fixture decodes");
            let parameters = sldprt_native(decoded.ir())
                .feature_input_lanes
                .iter()
                .flat_map(|lane| &lane.scalars)
                .map(|scalar| scalar.archive_object_id)
                .collect::<Vec<_>>();
            assert_eq!(parameters, [Some(integer), Some(toleranced)]);

            let written = written_with_an_added_datum_point(&source);
            assert_eq!(source_ids(&written, "MatrizL1"), [Some(132)]);
            assert_eq!(source_ids(&written, "Added"), [Some(136)]);
        }
    }

    #[test]
    fn an_added_feature_takes_the_lowest_free_identifier_when_none_follows_the_largest() {
        let mut source = sldprt_with_body(&triangle_body());
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            br#"<Keywords><Feature Name="Low" Type="MatrizL" id="1"><Dimension Name="D1">15</Dimension></Feature><Feature Name="High" Type="MatrizL" id="4294967294"><Dimension Name="D1">15</Dimension></Feature></Keywords>"#,
        ));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let native = sldprt_native(decoded.ir());
        assert_eq!(native.object_ids().max(), Some(u32::MAX - 1));
        let mut added = decoded.ir().model.features[0].clone();
        added.id = FeatureId::mint("synthetic:test:feature#added").expect("fixture identity");
        added.native_ref = None;
        let allocated = super::generated_feature_source_ids(std::slice::from_ref(&added), &native)
            .expect("an identifier is free");
        assert_eq!(allocated[&added.id].value(), Some(2));
    }
}
