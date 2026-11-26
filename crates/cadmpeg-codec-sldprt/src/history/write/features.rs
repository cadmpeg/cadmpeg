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

use super::edits::ConstructionEdit;
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
    /// stored solid does not show, and the relation point charges of the
    /// configuration states projected again for the write.
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
        // A later object of the class is a repeated-class record away from
        // the declaration; it is linked when one lane name carries the name.
        if matches.is_empty() {
            matches = native
                .feature_input_lanes
                .iter()
                .enumerate()
                .flat_map(|(lane_index, lane)| {
                    lane.names
                        .iter()
                        .enumerate()
                        .filter(|(_, name)| name.value == old_name)
                        .map(move |(name_index, _)| (lane_index, name_index))
                })
                .collect();
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
    edits: &mut super::edits::SourceEdits<'_>,
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
        edits.ctx().policy(),
    )?;
    // The end specs whose reversal flag an edit clears, read from the stored
    // records as the decoder read them; `None` where the context refuses the
    // scan.
    let reversed_end_specs = if features.iter().any(cleared_reversal) {
        crate::resolved_features::terminations::reversed_end_specs(
            &ctx,
            &native.feature_histories,
            &native.feature_input_lanes,
        )
        .ok()
    } else {
        Some(HashMap::new())
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
    // The millimetre value of each length parameter of the decoded source,
    // by owner and name: what the stored text of that dimension states.
    let source_lengths = edits
        .source()
        .into_iter()
        .flat_map(|source| &source.model.parameters)
        .filter_map(|parameter| match &parameter.value {
            Some(cadmpeg_ir::features::parameters::ParameterValue::Length(length)) => Some((
                (parameter.owner.as_ref()?, parameter.name.as_str()),
                length.get(),
            )),
            _ => None,
        })
        .collect::<HashMap<_, _>>();

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
    let source_contradicted_travel = edits
        .source()
        .map(crate::history::extrude_direction::contradicted_features)
        .unwrap_or_default();
    // Features whose stored content items are kept: the caller did not edit
    // their source content.
    let mut content_kept = HashSet::new();
    // The construction edits of each edited feature with a stored record and
    // a source feature, each with whether it moves no record field.
    let mut edited_constructions = HashMap::<FeatureId, Vec<(ConstructionEdit, bool)>>::new();
    for feature in features {
        let mut existing = native
            .feature_histories
            .iter_mut()
            .flat_map(|history| &mut history.features)
            .find(|candidate| feature.native_ref.as_deref() == Some(candidate.id.as_str()));
        // The source feature of an existing record: each field the caller did
        // not edit keeps its stored state.
        let original = existing
            .as_deref()
            .and_then(|_| edits.source_feature(&feature.id));
        if let (Some(record), Some(original)) = (existing.as_deref_mut(), original) {
            if edits.feature_unedited(ir, feature) {
                if feature.name != original.name {
                    record.name = feature.name.clone().unwrap_or_default();
                }
                content_kept.insert(feature.id.clone());
                continue;
            }
        }
        // The construction edits, and the source feature with each applied
        // alone. A feature with an edit that is not written keeps its stored
        // record, whose loss names them.
        let construction = original
            .filter(|_| !edits.feature_edit_unwritten(&feature.id))
            .map(|original| super::edits::construction_edits(original, feature))
            .unwrap_or_default();
        let applied_edits = original.map_or_else(Vec::new, |original| {
            construction
                .iter()
                .map(|edit| edit.applied(original, feature))
                .collect::<Vec<_>>()
        });
        let suppressed = feature
            .suppressed
            .or_else(|| existing.as_deref().map(|record| record.suppressed))
            .ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "SLDPRT writing requires resolved suppression for feature {}",
                    feature.id
                ))
            })?;
        let encoder = |feature, contradicted_travel: bool| NeutralFeatureEncoder {
            contradicted_travel,
            feature,
            existing: existing.as_deref(),
            principal_planes_by_record: &principal_planes_by_record,
            record_sources: &record_sources,
            retained_tree_node_roles: &retained_tree_node_roles,
            feature_sources: &feature_sources,
            sketch_sources: &sketch_sources,
            parent_sources: &parent_sources,
            resolved_parameter_names: &resolved_parameter_names,
        };
        let encoded = encoder(feature, contradicted_travel.contains(&feature.id)).encode();
        // The source feature encoded the same way: a record field whose
        // encoding the edit does not move keeps its stored state.
        let baseline = original.map(|original| {
            encoder(original, source_contradicted_travel.contains(&original.id)).encode()
        });
        // A construction edit that, applied alone to the source feature,
        // encodes as the source does moves no record field.
        if let (Some(original), false) = (original, construction.is_empty()) {
            let unmoved = applied_edits
                .iter()
                .map(|applied| {
                    let (Some(applied), Some(Ok(baseline))) = (applied, baseline.as_ref()) else {
                        return false;
                    };
                    encoder(applied, source_contradicted_travel.contains(&original.id))
                        .encode()
                        .is_ok_and(|encoding| {
                            encoding.kind == baseline.kind
                                && encoding.parameters == baseline.parameters
                                && encoding.properties == baseline.properties
                        })
                })
                .collect::<Vec<_>>();
            edited_constructions.insert(
                feature.id.clone(),
                construction.into_iter().zip(unmoved).collect(),
            );
        }
        let source_unrestatable = matches!(baseline, Some(Err(CodecError::NotImplemented(_))));
        // A stored record the encoder cannot restate from the neutral
        // definition is written as stored.
        let mut kept = false;
        let NeutralFeatureEncoding {
            kind,
            mut parameters,
            mut properties,
        } = match (encoded, existing.as_deref()) {
            (Ok(encoding), _) => encoding,
            (Err(CodecError::NotImplemented(reason)), Some(record)) if edits.has_source() => {
                if original.is_none_or(|original| {
                    !super::edits::construction_edits(original, feature).is_empty()
                }) {
                    let reason = if source_unrestatable {
                        format!(
                            "SLDPRT cannot restate the stored record of feature {} from its definition",
                            feature.id
                        )
                    } else {
                        reason
                    };
                    edits.feature_unwritten(&feature.id, reason)?;
                }
                kept = true;
                NeutralFeatureEncoding {
                    kind: record.kind.clone(),
                    parameters: record.parameters.clone(),
                    properties: record.properties.clone(),
                }
            }
            (Err(error), _) => return Err(error),
        };
        // A feature written with its stored record keeps its stored flag.
        if !kept && !edits.feature_edit_unwritten(&feature.id) && cleared_reversal(feature) {
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
        // With a source baseline, each record field the edit does not move
        // keeps its stored state.
        let mut kind = kind;
        let field_baseline = original;
        // A feature with an edit the output does not carry keeps its stored
        // definition record whole: the unwritten-edit loss states that.
        if let (false, Some(record)) = (kept, existing.as_deref()) {
            if edits.feature_edit_unwritten(&feature.id) {
                kept = true;
                kind.clone_from(&record.kind);
                parameters.clone_from(&record.parameters);
                properties.clone_from(&record.properties);
            }
        }
        if let (false, Some(record), Some(Ok(mut baseline))) = (kept, existing.as_deref(), baseline)
        {
            restore_equivalent_parameter_expressions(
                record,
                &original_parameters,
                &evaluated_parameters,
                &mut baseline.parameters,
            );
            if kind == baseline.kind {
                kind.clone_from(&record.kind);
            }
            // A lane-derived group whose gate the record does not state is
            // read from the record once a member is written: write it whole.
            let moved_properties = moved_keys(&properties, &baseline.properties);
            let groups = crate::records::LANE_DERIVED_GROUPS
                .iter()
                .filter(|group| {
                    !record.states_any(group.gate)
                        && group
                            .properties
                            .iter()
                            .any(|member| moved_properties.contains(*member))
                })
                .collect::<Vec<_>>();
            let group_parameters = groups
                .iter()
                .flat_map(|group| group.parameters.iter().copied())
                .collect::<HashSet<_>>();
            let group_properties = groups
                .iter()
                .flat_map(|group| group.properties.iter().copied())
                .collect::<HashSet<_>>();
            parameters = stored_unless_moved(
                &record.parameters,
                parameters,
                &baseline.parameters,
                &group_parameters,
            );
            properties = stored_unless_moved(
                &record.properties,
                properties,
                &baseline.properties,
                &group_properties,
            );
        }
        if kept {
            edits.keep_stored_definition(feature)?;
        }
        // Without a source feature every field is written.
        let moved = |unchanged: fn(
            &cadmpeg_ir::features::Feature,
            &cadmpeg_ir::features::Feature,
        ) -> bool| {
            field_baseline.is_none_or(|original| !unchanged(original, feature))
        };
        if moved(|original, feature| original.evaluation.outputs() == feature.evaluation.outputs())
        {
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
        } else if let Some(scope) = existing
            .as_deref()
            .and_then(|record| record.properties.get("Scope"))
        {
            // Unedited output bodies keep the stored scope.
            properties.insert(cadmpeg_core::nonblank_literal!("Scope"), scope.clone());
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
        let parent_moved = field_baseline.is_none() || edits.feature_parent_moved(ir, &feature.id);
        if let Some(existing) = existing.as_mut() {
            if let Some(tag) = solidworks_source_tag(feature) {
                if moved(|original, feature| original.source_tag == feature.source_tag) {
                    existing.xml_tag.clone_from(tag);
                }
            }
            if moved(|original, feature| original.ordinal == feature.ordinal) {
                existing.ordinal = ordinal;
            }
            if moved(|original, feature| original.name == feature.name) {
                existing.name = feature.name.clone().unwrap_or_default();
            }
            existing.kind = kind;
            if moved(|original, feature| original.suppressed == feature.suppressed) {
                existing.suppressed = suppressed;
            }
            if parent_moved {
                existing.tree_parent = tree_parent;
            }
            existing.parameters = parameters;
            existing.properties = properties;
            if moved(|original, feature| original.source_text == feature.source_text) {
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
            }
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
        if field_baseline.is_some_and(|original| original.source_content == feature.source_content)
        {
            content_kept.insert(feature.id.clone());
        }
    }
    let losses = clear_reversals(
        &cleared,
        reversed_end_specs.as_ref(),
        &ir.model.configurations,
        &mut native.feature_input_lanes,
    );
    synchronize_neutral_feature_content(features, parameters, &record_ids, &content_kept, native)?;
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
    // The lane scalars hold the edited millimetre values; each edited text
    // is stated in the scale of the text the record stored.
    let features_by_record = record_ids
        .iter()
        .map(|(feature, record)| (record.as_str(), feature))
        .collect::<HashMap<_, _>>();
    for record in native
        .feature_histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        let (Some(stored), Some(feature)) = (
            original_parameters.get(&record.id),
            features_by_record.get(record.id.as_str()),
        ) else {
            continue;
        };
        super::parameters::state_in_stored_scale(
            stored,
            |name| source_lengths.get(&(*feature, name)).copied(),
            &mut record.parameters,
        );
    }
    let projected_features =
        project_features_with_native_inputs(native, edits.ctx().policy().limits)?;
    let projected_features = projected_features
        .into_iter()
        .map(|feature| (feature.id.clone(), feature))
        .collect::<HashMap<_, _>>();
    // An edited leaf of a feature's construction is not written when it
    // moves no record field, or when the written records read back as the
    // source states it.
    let mut dependencies_unwritten = HashSet::new();
    for feature in features {
        let Some(original) = edits.source_feature(&feature.id) else {
            continue;
        };
        if edits.operands_kept(&feature.id) {
            let construction = super::edits::construction_edits(original, feature);
            edits.claim(
                original,
                feature,
                construction.into_iter().map(|edit| (edit, false)),
                false,
            );
            continue;
        }
        let Some(construction) = edited_constructions.remove(&feature.id) else {
            continue;
        };
        let written = projected_features.get(&neutral_feature_id(&record_ids[&feature.id])?);
        let source_dependencies = original
            .dependencies
            .iter()
            .map(|dependency| match record_ids.get(dependency) {
                Some(record) => neutral_feature_id(record),
                None => Ok(dependency.clone()),
            })
            .collect::<Result<Vec<_>, CodecError>>()?;
        let dependencies_as_source =
            written.is_some_and(|written| written.dependencies.as_slice() == source_dependencies);
        // The definitions in wire form; `None` when one cannot be read back.
        let definitions = written.and_then(|written| {
            Some((
                cadmpeg_core::serde_erase::to_json_value(original.evaluation.definition()).ok()?,
                written,
                cadmpeg_core::serde_erase::to_json_value(written.evaluation.definition()).ok()?,
            ))
        });
        let as_source = construction
            .iter()
            .map(|(edit, _)| {
                definitions.as_ref().is_some_and(
                    |(source_definition, written, written_definition)| {
                        edit.reads_back_as_source(
                            (original, source_definition),
                            (written, written_definition),
                            dependencies_as_source,
                        )
                    },
                )
            })
            .collect::<Vec<_>>();
        let unwritten = construction
            .iter()
            .zip(&as_source)
            .filter(|((_, unmoved), as_source)| *unmoved || **as_source)
            .map(|((edit, _), _)| edit)
            .collect::<Vec<_>>();
        if unwritten.iter().any(|edit| edit.is_dependencies()) {
            dependencies_unwritten.insert(feature.id.clone());
        }
        if !unwritten.is_empty() {
            edits.construction_reads_back_as_stored(
                &feature.id,
                &unwritten
                    .iter()
                    .map(|edit| edit.label.clone())
                    .collect::<Vec<_>>(),
                unwritten.len() == construction.len(),
            );
        }
        // An edit that moves a record field while the records read back as
        // the source would leave the record stating what the output does not
        // carry.
        let withhold = construction
            .iter()
            .zip(&as_source)
            .any(|((_, unmoved), as_source)| !unmoved && *as_source);
        edits.claim(
            original,
            feature,
            construction
                .into_iter()
                .zip(as_source)
                .map(|((edit, unmoved), as_source)| (edit, !unmoved && !as_source)),
            withhold,
        );
    }
    // A stored record keeps its stored operands, and a dependency edit that
    // reads back as the source is named above. With a decoded source, the
    // full decoder reads the written part back and judges its dependencies.
    for feature in features.iter().filter(|feature| {
        !edits.has_source()
            && !edits.operands_kept(&feature.id)
            && !dependencies_unwritten.contains(&feature.id)
            && !edits.feature_construction_unedited(feature)
    }) {
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
/// flag as a Keywords attribute, which the encoder already dropped.
/// `reversed_end_specs` is `None` where the writer could not scan the stored
/// end specs. Names each extrusion whose flag the write could not clear, and
/// each lane outside the active configuration whose flag it cleared. Where
/// the part has fewer than two configurations, every lane holds the model's
/// state.
fn clear_reversals(
    cleared: &[(&cadmpeg_ir::features::Feature, bool)],
    reversed_end_specs: Option<&HashMap<String, Vec<EndSpecAnchor>>>,
    configurations: &[cadmpeg_ir::features::parameters::DesignConfiguration],
    lanes: &mut [crate::records::FeatureInputLane],
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    use crate::layout::extrusion_end_spec_header::REVERSAL;
    use cadmpeg_ir::features::extents::ExtrudeDirection;
    let lane_configurations =
        crate::history::configuration::configuration_lane_assignments(configurations, lanes)
            .into_iter()
            .map(|(configuration, lane)| (lane, &configurations[configuration]))
            .collect::<HashMap<_, _>>();
    let mut losses = Vec::new();
    for (feature, stated) in cleared {
        let anchors = feature
            .native_ref
            .as_deref()
            .and_then(|record| reversed_end_specs?.get(record))
            .map_or(&[][..], Vec::as_slice);
        let mut unwritten = anchors.is_empty() && (!stated || reversed_end_specs.is_none());
        let mut configuration_lanes = std::collections::BTreeMap::new();
        for anchor in anchors {
            let at = anchor.offset + REVERSAL;
            match lanes
                .get_mut(anchor.lane)
                .and_then(|lane| lane.native_payload.get_mut(at..at + 4))
            {
                Some(word) => word.copy_from_slice(&0_u32.to_le_bytes()),
                None => {
                    unwritten = true;
                    continue;
                }
            }
            let lane = &lanes[anchor.lane].id;
            let place = match lane_configurations.get(&anchor.lane) {
                Some(configuration) if configuration.active => continue,
                Some(configuration) => {
                    let state = match configuration
                        .feature_states
                        .get(&feature.id)
                        .map(|state| state.definition.operation())
                    {
                        Some(FeatureOperation::Extrude {
                            direction: ExtrudeDirection::ProfileNormal {},
                            ..
                        }) => "whose state there travels along the normal over that flag: the written flag can change that configuration's extrusion",
                        Some(FeatureOperation::Extrude {
                            direction: ExtrudeDirection::ReversedProfileNormal {},
                            ..
                        }) => "whose state there travels against the normal: that configuration is written along it",
                        _ => "which states no resolved travel for it: that configuration is written along it",
                    };
                    format!(
                        "in lane {lane} of configuration {}, {state}",
                        crate::history::extrude_direction::configuration_label(configuration)
                    )
                }
                None if configurations.len() < 2 => continue,
                None => format!(
                    "in lane {lane}, which the writer assigns to no configuration: the configuration that lane holds is written along it"
                ),
            };
            configuration_lanes.insert(anchor.lane, place);
        }
        let extrusion = crate::history::extrude_direction::label(feature);
        if unwritten {
            let cause = if reversed_end_specs.is_none() {
                "the writer could not read the end specs of the stored records, so the written record can keep the stored reversal"
            } else {
                "the writer found no end-spec reversal flag to clear, so the written record keeps the stored reversal"
            };
            losses.push(SldprtLossCode::WriterExtrudeReversalUnwritten.note(format!(
                "Extrusion {extrusion} is edited to travel along the profile normal, and {cause}."
            )));
        }
        for place in configuration_lanes.into_values() {
            losses.push(SldprtLossCode::WriterExtrudeReversalConfigurationCleared.note(format!(
                "Extrusion {extrusion} is edited to travel along the profile normal, and the writer also cleared its end-spec reversal flag {place}."
            )));
        }
    }
    losses
}

/// The keys of `written` whose entry differs from `baseline`'s.
fn moved_keys<V: PartialEq>(
    written: &BTreeMap<cadmpeg_core::text::NonBlankString, V>,
    baseline: &BTreeMap<cadmpeg_core::text::NonBlankString, V>,
) -> HashSet<String> {
    written
        .iter()
        .filter(|(key, value)| baseline.get(*key) != Some(*value))
        .map(|(key, _)| key.as_str().to_owned())
        .collect()
}

/// The stored entries of a record field, with each entry whose encoding
/// `written` moves from the source feature's encoding `baseline` written as
/// `written` states it, and each entry of `group` that `written` states
/// written too: `group` holds the members of the lane-derived groups the
/// write makes the record state.
fn stored_unless_moved<V: Clone + PartialEq>(
    stored: &BTreeMap<cadmpeg_core::text::NonBlankString, V>,
    written: BTreeMap<cadmpeg_core::text::NonBlankString, V>,
    baseline: &BTreeMap<cadmpeg_core::text::NonBlankString, V>,
    group: &HashSet<&str>,
) -> BTreeMap<cadmpeg_core::text::NonBlankString, V> {
    let moved = moved_keys(&written, baseline);
    let mut merged = stored.clone();
    for key in baseline.keys().filter(|key| !written.contains_key(*key)) {
        merged.remove(key);
    }
    for (key, value) in written {
        if moved.contains(key.as_str()) || group.contains(key.as_str()) {
            merged.insert(key, value);
        }
    }
    merged
}

fn synchronize_neutral_feature_content(
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[DesignParameter],
    record_ids: &HashMap<FeatureId, String>,
    content_kept: &HashSet<FeatureId>,
    native: &mut crate::native::SldprtNative,
) -> Result<(), CodecError> {
    let parameters = parameters
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    for feature in features {
        if feature.source_content.is_empty() || content_kept.contains(&feature.id) {
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
    use cadmpeg_core::serde_erase::json;
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

        let error = sync_neutral_features(
            &ir,
            &[],
            &[],
            &mut Some(native),
            &mut crate::history::write::edits::SourceEdits::without_source(
                &ir,
                &cadmpeg_core::decode::test_support::service_decode_context(),
            ),
        )
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

        let error = sync_neutral_features(
            &ir,
            &[],
            &[],
            &mut Some(sldprt_native(decoded.ir())),
            &mut crate::history::write::edits::SourceEdits::without_source(
                &ir,
                &cadmpeg_core::decode::test_support::service_decode_context(),
            ),
        )
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
            sync_neutral_features(
                &ir,
                &ir.model.parameters,
                &ir.geometry.bodies,
                &mut native,
                &mut crate::history::write::edits::SourceEdits::without_source(
                    &ir,
                    &cadmpeg_core::decode::test_support::service_decode_context(),
                ),
            )
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

    /// A parameter the archive walk does not read (the part holds no
    /// `Config-0` stream to walk with) holds its identifier after a zero byte
    /// and a null tolerance pointer, and an added feature takes the identifier
    /// past it.
    #[test]
    fn an_added_feature_takes_an_identifier_past_a_parameter_the_walk_does_not_read() {
        let mut resolved = resolved_feature_classes_with_ids(&[("moLPattern_c", "MatrizL1", 132)]);
        resolved.extend(crate::test_support::carchive::parameter_prefix());
        resolved.extend(0.015_f64.to_le_bytes());
        let mut trailer = [0_u8; 35];
        trailer[3..7].copy_from_slice(&133_u32.to_le_bytes());
        resolved.extend(trailer);
        let mut source = sldprt_with_body(&triangle_body());
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            br#"<Keywords><Feature Name="MatrizL1" Type="MatrizL" id="132"><Dimension Name="D1">15</Dimension></Feature></Keywords>"#,
        ));
        source.extend(make_block(
            0x42,
            "Contents/Config-0-ResolvedFeatures",
            &resolved,
        ));
        let written = written_with_an_added_datum_point(&source);
        assert_eq!(source_ids(&written, "MatrizL1"), [Some(132)]);
        assert_eq!(source_ids(&written, "Added"), [Some(134)]);
    }

    /// When the part holds `u32::MAX - 1`, no identifier follows the largest,
    /// and an added feature takes the lowest identifier the part does not hold.
    #[test]
    fn an_added_feature_takes_the_lowest_free_identifier_when_none_follows_the_largest() {
        let mut source = sldprt_with_body(&triangle_body());
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            br#"<Keywords><Feature Name="Low" Type="MatrizL" id="1"><Dimension Name="D1">15</Dimension></Feature><Feature Name="High" Type="MatrizL" id="4294967294"><Dimension Name="D1">15</Dimension></Feature></Keywords>"#,
        ));
        let written = written_with_an_added_datum_point(&source);
        assert_eq!(source_ids(&written, "High"), [Some(u32::MAX - 1)]);
        assert_eq!(source_ids(&written, "Added"), [Some(2)]);
    }

    /// A projected one-direction join `Extrude` whose record states the
    /// reversal flag, edited to travel along the profile normal.
    fn cleared_extrusion() -> cadmpeg_ir::features::Feature {
        use cadmpeg_ir::features::extents::ExtrudeDirection;
        use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
        let history: crate::records::FeatureHistory = json::from_value(cadmpeg_core::json!({
            "id":"history", "features":[{
                "id":"native", "parent":"history", "ordinal":0,
                "name":"Extrude", "kind":"BossExtrude", "xml_tag":"Extrusion",
                "input_class":"moExtrusion_c", "parameters":{"Depth":"10mm"},
                "properties":{"EndCondition":"Blind", "Operation":"Join", "Reverse":"true"}
            }]
        }))
        .expect("synthetic history");
        let mut feature = crate::history::project::project_features(&[history])
            .expect("feature projection")
            .into_iter()
            .next()
            .expect("the extrusion");
        feature.evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Extrude { direction, .. }) =
                definition
            {
                *direction = ExtrudeDirection::ProfileNormal {};
            }
        });
        assert!(crate::history::extrude_direction::cleared_reversal(
            &feature
        ));
        feature
    }

    /// A cleared reversal with no end-spec flag to clear is named, unless its
    /// record states the flag as a Keywords attribute, which the encoder
    /// drops. Where the writer could not scan the stored end specs, every
    /// cleared reversal is named, the stated one too.
    #[test]
    fn a_cleared_reversal_without_a_flag_to_clear_is_named() {
        use crate::loss::SldprtLossCode;
        use crate::resolved_features::terminations::EndSpecAnchor;
        use std::collections::HashMap;
        let feature = cleared_extrusion();
        let scanned = HashMap::<String, Vec<EndSpecAnchor>>::new();
        let unwritten = |stated: bool, end_specs: Option<&HashMap<String, Vec<EndSpecAnchor>>>| {
            super::clear_reversals(&[(&feature, stated)], end_specs, &[], &mut [])
                .into_iter()
                .map(|loss| {
                    assert_eq!(
                        loss.code,
                        SldprtLossCode::WriterExtrudeReversalUnwritten.kind()
                    );
                    loss.message
                })
                .collect::<Vec<_>>()
        };
        let named = unwritten(false, Some(&scanned));
        assert_eq!(named.len(), 1, "{named:?}");
        assert!(named[0].contains("Extrusion \"Extrude\" ("), "{named:?}");
        assert!(unwritten(true, Some(&scanned)).is_empty());
        for stated in [false, true] {
            let named = unwritten(stated, None);
            assert_eq!(named.len(), 1, "{named:?}");
            assert!(
                named[0].contains("could not read the end specs")
                    && named[0].contains("can keep the stored reversal"),
                "{named:?}"
            );
        }
    }

    /// The write clears the flag in every lane that holds it, and names each
    /// lane outside the active configuration: a lane of an inactive
    /// configuration, also where that configuration's state reads along the
    /// normal over the flag, and a lane that the writer assigns to no
    /// configuration. A part with one configuration has the model's state in
    /// every lane.
    #[test]
    fn a_cleared_lane_outside_the_active_configuration_is_named() {
        use crate::layout::extrusion_end_spec_header::REVERSAL;
        use crate::loss::SldprtLossCode;
        use crate::resolved_features::terminations::EndSpecAnchor;
        use cadmpeg_ir::features::parameters::{
            ConfigurationEvaluation, ConfigurationFeatureState, DesignConfiguration,
        };
        use cadmpeg_ir::features::selections::DistinctMembers;
        use cadmpeg_ir::features::ConfigurationId;
        use std::collections::{BTreeMap, HashMap};
        let feature = cleared_extrusion();
        let configuration = |name: &str, ordinal, active| DesignConfiguration {
            id: ConfigurationId::mint(format!("synthetic:test:id#{name}"))
                .expect("identity grammar"),
            ordinal,
            active,
            source_index: None,
            name: Some(name.to_owned()),
            material: None,
            properties: BTreeMap::new(),
            bodies: None,
            parameter_values: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: None,
        };
        let mut along = configuration("Along", 2, false);
        along.feature_states.insert(
            feature.id.clone(),
            ConfigurationFeatureState {
                evaluation: ConfigurationEvaluation::Active {
                    outputs: DistinctMembers::default(),
                },
                dependencies: DistinctMembers::default(),
                definition: feature.evaluation.definition().clone(),
            },
        );
        let configurations = [
            configuration("Default", 0, true),
            configuration("Alternate", 1, false),
            along,
        ];
        let lane = |id: &str, configuration: Option<&str>| -> crate::records::FeatureInputLane {
            let payload = format!("{}01{}", "00".repeat(REVERSAL), "00".repeat(31 - REVERSAL));
            let mut value = cadmpeg_core::json!({
                "id": id,
                "native_payload": payload,
                "classes": [],
                "names": []
            });
            if let Some(configuration) = configuration {
                value["configuration"] = configuration.into();
            }
            json::from_value(value).expect("synthetic lane")
        };
        let anchors = HashMap::from([(
            feature.native_ref.clone().expect("the record"),
            (0..4)
                .map(|lane| EndSpecAnchor {
                    lane,
                    offset: 0,
                    reversed: true,
                })
                .collect::<Vec<_>>(),
        )]);
        let named = |configurations: &[DesignConfiguration]| {
            let mut lanes = [
                lane("lane-default", Some("0")),
                lane("lane-alternate", Some("1")),
                lane("lane-along", Some("2")),
                lane("lane-shared", None),
            ];
            let losses = super::clear_reversals(
                &[(&feature, false)],
                Some(&anchors),
                configurations,
                &mut lanes,
            );
            for lane in &lanes {
                assert_eq!(lane.native_payload[REVERSAL..REVERSAL + 4], [0; 4]);
            }
            losses
                .into_iter()
                .map(|loss| {
                    assert_eq!(
                        loss.code,
                        SldprtLossCode::WriterExtrudeReversalConfigurationCleared.kind()
                    );
                    loss.message
                })
                .collect::<Vec<_>>()
        };
        let messages = named(&configurations);
        assert_eq!(messages.len(), 3, "{messages:?}");
        assert!(
            messages[0].contains("lane lane-alternate of configuration \"Alternate\" (")
                && messages[0].contains("which states no resolved travel for it"),
            "{messages:?}"
        );
        assert!(
            messages[1].contains("lane lane-along of configuration \"Along\" (")
                && messages[1].contains("travels along the normal over that flag"),
            "{messages:?}"
        );
        assert!(
            messages[2].contains("lane lane-shared, which the writer assigns to no configuration"),
            "{messages:?}"
        );
        assert!(named(&configurations[..1]).is_empty());
    }
}
