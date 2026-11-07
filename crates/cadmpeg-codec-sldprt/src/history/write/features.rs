// SPDX-License-Identifier: Apache-2.0
//! Neutral feature synchronization into native history records.

use crate::records::{Feature, FeatureContent, FeatureHistory, HistoryContent};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    DesignParameter, FeatureDefinition, FeatureId, FeatureOperation, FeatureSourceContent,
    ParameterId,
};
use cadmpeg_ir::topology::Body;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::history::classify::{
    feature_tree_node_role, is_custom_property, principal_plane_in_history,
};
use crate::history::project::neutral_feature_id;

use super::project_features_with_native_inputs;
use super::xml::{feature_xml_tag, valid_xml_name};
use crate::history::encode::{NeutralFeatureEncoder, NeutralFeatureEncoding};
use crate::records::FeatureSource;
use crate::writer::write_error::WriteError;

/// A write-side request to rewrite one serialized object name.
///
/// The typed name is current document content. A neutral owner rename takes
/// precedence over that copy. This request changes the retained layout only
/// after the patch writer verifies its physical positions against the source.
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
    // A history-only document has no serialized feature-input names to
    // synchronize. Its current history name is still authored below, without
    // turning an unrelated metadata edit into a refusal of retained B-rep.
    if native.feature_input_lanes.is_empty() {
        return Ok(Vec::new());
    }
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
                record.id.clone(),
                record.source_value()?,
            ))
        })
        .collect::<Vec<_>>();

    let mut requested = Vec::new();
    for (old_name, new_name, owner_id, object_id) in renames {
        let mut matches = Vec::<(usize, usize)>::new();
        for (lane_index, lane) in native.feature_input_lanes.iter().enumerate() {
            for (name_index, name) in lane.names.iter().enumerate() {
                if name.feature_ref.as_deref() == Some(owner_id.as_str())
                    && name.object_id.and_then(crate::records::ObjectId::value) == Some(object_id)
                    && name.value == old_name
                {
                    matches.push((lane_index, name_index));
                }
            }
        }
        let [(lane_index, name_index)] = matches.as_slice() else {
            return Err(CodecError::UnsupportedContent(format!(
                "SLDPRT feature-input name for {old_name:?} is not uniquely linked"
            )));
        };
        let value = cadmpeg_core::text::NonBlankString::new(new_name).ok_or_else(|| {
            CodecError::UnsupportedContent(format!(
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
) -> Result<HashMap<FeatureId, FeatureSource>, crate::writer::write_error::WriteError> {
    let mut used = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .filter_map(crate::records::Feature::source_value)
        .collect::<HashSet<_>>();
    let existing = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .filter_map(|feature| Some((feature.id.as_str(), feature.source_value()?)))
        .collect::<HashMap<_, _>>();
    // `next` is the lowest source id not yet offered. `None` states that the
    // id space is exhausted: every allocation advances it, and the allocation
    // that finds it exhausted is the one refusal.
    let mut next = Some(1u32);
    let mut allocated = HashMap::new();
    for feature in features
        .iter()
        .filter(|feature| feature.native_ref.is_none())
    {
        let record_id = generated_feature_record_id(&feature.id);
        let source_id = if let Some(source_id) = existing.get(record_id.as_str()).copied() {
            source_id
        } else {
            allocate_feature_source_id(&mut used, &mut next)?
        };
        let source_id = FeatureSource::from_value(source_id).ok_or_else(|| {
            CodecError::Malformed("SLDPRT feature source-id space is exhausted".into())
        })?;
        allocated.insert(feature.id.clone(), source_id);
    }
    Ok(allocated)
}

/// Take the lowest source id no feature holds, and advance `next` past it.
///
/// `next` states the lowest id not yet offered, or `None` when the space is exhausted.
pub(crate) fn allocate_feature_source_id(
    used: &mut HashSet<u32>,
    next: &mut Option<u32>,
) -> Result<u32, crate::writer::write_error::WriteError> {
    loop {
        let candidate = next.ok_or(crate::writer::write_error::WriteError::Capacity {
            field: &crate::writer::CAPACITY_FIELDS[32],
            value: u64::from(u32::MAX),
        })?;
        if candidate == u32::MAX {
            *next = None;
            continue;
        }
        *next = candidate.checked_add(1).filter(|value| *value != u32::MAX);
        if used.insert(candidate) {
            return Ok(candidate);
        }
    }
}

/// Apply neutral native-feature edits to the `SolidWorks` history used for writing.
pub(crate) fn sync_neutral_features(
    model: &cadmpeg_ir::document::Model,
    parameters: &[DesignParameter],
    bodies: &[Body],
    native: &mut Option<crate::native::SldprtNative>,
    orphaned: &HashSet<ParameterId>,
) -> Result<Vec<FeatureInputRename>, crate::writer::write_error::WriteError> {
    synchronize_features(model, parameters, bodies, native, true, orphaned)
        .map(|(renames, _)| renames)
}

/// Author history records without retained input-lane layout identities.
pub(crate) fn synthesize_neutral_features(
    model: &cadmpeg_ir::document::Model,
    parameters: &[DesignParameter],
    bodies: &[Body],
    native: &mut Option<crate::native::SldprtNative>,
    orphaned: &HashSet<ParameterId>,
) -> Result<
    (Vec<FeatureInputRename>, std::collections::BTreeSet<String>),
    crate::writer::write_error::WriteError,
> {
    synchronize_features(model, parameters, bodies, native, false, orphaned)
}

/// Neutral record ids for every neutral feature: its retained native record
/// when one exists, else its generated record identity.
pub(crate) fn feature_record_ids(
    features: &[cadmpeg_ir::features::Feature],
) -> HashMap<FeatureId, String> {
    features
        .iter()
        .map(|feature| {
            let record_id = feature
                .native_ref
                .clone()
                .unwrap_or_else(|| generated_feature_record_id(&feature.id));
            (feature.id.clone(), record_id)
        })
        .collect::<HashMap<_, _>>()
}

type DependencyMismatch = (
    usize,
    Vec<FeatureId>,
    Option<cadmpeg_ir::features::DistinctMembers<FeatureId>>,
);

/// Indexes of features whose dependency edges disagree with the canonical
/// operation projection, with the expected edges and the canonical edges.
///
/// A feature with a native record must project exactly its expected edges; a
/// feature without one must project at least them (review E, #237). A missing
/// projection is reported with no canonical edges; it cannot be repaired.
pub(crate) fn dependency_mismatches(
    features: &[cadmpeg_ir::features::Feature],
    record_ids: &HashMap<FeatureId, String>,
    projected: &[cadmpeg_ir::features::Feature],
) -> Result<Vec<DependencyMismatch>, CodecError> {
    let projected = projected
        .iter()
        .map(|feature| (feature.id.clone(), feature))
        .collect::<HashMap<_, _>>();
    let mut mismatches = Vec::new();
    for (index, feature) in features.iter().enumerate() {
        let projected_id = neutral_feature_id(&record_ids[&feature.id])?;
        let expected = feature
            .dependencies
            .iter()
            .map(|dependency| match record_ids.get(dependency) {
                Some(record) => neutral_feature_id(record),
                None => Ok(dependency.clone()),
            })
            .collect::<Result<Vec<_>, CodecError>>()?;
        let consistent = projected.get(&projected_id).is_some_and(|projected| {
            if feature.native_ref.is_some() {
                projected.dependencies.as_slice() == expected
            } else {
                expected
                    .iter()
                    .all(|dependency| projected.dependencies.contains(dependency))
            }
        });
        if !consistent {
            mismatches.push((
                index,
                expected,
                projected
                    .get(&projected_id)
                    .map(|projected| projected.dependencies.clone()),
            ));
        }
    }
    Ok(mismatches)
}

fn synchronize_features(
    model: &cadmpeg_ir::document::Model,
    parameters: &[DesignParameter],
    bodies: &[Body],
    native: &mut Option<crate::native::SldprtNative>,
    patch_input_names: bool,
    orphaned: &HashSet<ParameterId>,
) -> Result<
    (Vec<FeatureInputRename>, std::collections::BTreeSet<String>),
    crate::writer::write_error::WriteError,
> {
    let features = &model.features;
    if features.is_empty() {
        if let Some(native) = native {
            for history in &mut native.feature_histories {
                history.features.retain(is_custom_property);
            }
        }
        return Ok((Vec::new(), std::collections::BTreeSet::new()));
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
        nested_sketch_carriers: Vec::new(),
        pmi_dimensions: Vec::new(),
        document_metadata: Vec::new(),
        decode_baselines: Vec::new(),
    });
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

    let classless_unchanged = if patch_input_names {
        HashSet::new()
    } else {
        let projected = project_features_with_native_inputs(native, &model.sketches)?;
        features
            .iter()
            .filter_map(|feature| {
                let record = native
                    .feature_histories
                    .iter()
                    .flat_map(|history| &history.features)
                    .find(|record| feature.native_ref.as_deref() == Some(record.id.as_str()))?;
                (record.input_class.is_none()
                    && projected.iter().any(|original| {
                        original.native_ref.as_deref() == Some(record.id.as_str())
                            && original.definition() == feature.definition()
                    }))
                .then_some(feature.id.clone())
            })
            .collect::<HashSet<_>>()
    };

    let renames = if patch_input_names {
        synchronize_feature_input_names(features, native)?
    } else {
        Vec::new()
    };

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
    let record_ids = feature_record_ids(features);
    let pmi_parameters = parameters
        .iter()
        .filter(|parameter| super::parameters::is_pmi_parameter(parameter, Some(native)))
        .map(|parameter| parameter.id.clone())
        .collect::<HashSet<_>>();
    let mut resolved_parameter_names =
        HashMap::<String, HashSet<cadmpeg_core::text::NonBlankString>>::new();
    for parameter in parameters {
        if pmi_parameters.contains(&parameter.id) {
            continue;
        }
        if let Some(record) = parameter
            .owner
            .as_ref()
            .and_then(|owner| record_ids.get(owner))
        {
            resolved_parameter_names
                .entry(record.clone())
                .or_default()
                .insert(
                    cadmpeg_core::text::NonBlankString::new(parameter.name.clone())
                        .ok_or_else(|| CodecError::malformed("SLDPRT parameter name is empty"))?,
                );
        }
    }
    let feature_names = features
        .iter()
        .filter_map(|feature| {
            feature
                .name
                .as_ref()
                .map(|name| (feature.id.clone(), name.clone()))
        })
        .collect::<HashMap<_, _>>();
    let global_owners = crate::history::parameters::global_parameter_owners(features);
    let mut canonical_parameter_texts =
        HashMap::<FeatureId, BTreeMap<cadmpeg_core::text::NonBlankString, String>>::new();
    for parameter in parameters {
        if orphaned.contains(&parameter.id) || pmi_parameters.contains(&parameter.id) {
            continue;
        }
        let Some(owner) = parameter.owner.as_ref() else {
            continue;
        };
        let text = crate::history::parameters::render_parameter_expression(
            parameter,
            parameters,
            &feature_names,
            &global_owners,
        )
        .ok_or_else(|| {
            CodecError::malformed(
                "SLDPRT canonical parameter expression references a missing identity",
            )
        })?;
        let name = cadmpeg_core::text::NonBlankString::new(parameter.name.clone())
            .ok_or_else(|| CodecError::malformed("SLDPRT parameter name is empty"))?;
        canonical_parameter_texts
            .entry(owner.clone())
            .or_default()
            .insert(name, text);
    }
    let mut parameter_properties = HashMap::new();
    for record in native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
    {
        for (ordinal, name) in crate::history::project::projected_parameter_names(record)
            .into_iter()
            .enumerate()
        {
            if let Some(properties) = record.dimension_properties.get(name.as_str()) {
                let id = crate::history::project::neutral_parameter_id(record, ordinal)?;
                parameter_properties.insert(id, properties.clone());
            }
        }
    }
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
            structural_parent_sources.get(&feature.id)?.as_ref()?;
            Some((&feature.id, parent_sources.get(&feature.id)?.as_str()))
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
    let mut ordered_features = features.iter().enumerate().collect::<Vec<_>>();
    ordered_features.sort_by_key(|(index, feature)| (feature.ordinal, *index));
    let feature_ranks = ordered_features
        .into_iter()
        .enumerate()
        .map(|(rank, (_, feature))| {
            let rank = u32::try_from(rank).map_err(|_| {
                CodecError::malformed("SLDPRT native history cannot index this many features")
            })?;
            Ok((&feature.id, rank))
        })
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let mut scope_ids = HashMap::new();
    for body in bodies {
        scope_ids.insert(body.id.as_str(), Some(&body.id));
        if let Some(name) = body.name.as_deref().filter(|name| !name.is_empty()) {
            match scope_ids.entry(name) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(Some(&body.id));
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    entry.insert(None);
                }
            }
        }
    }
    for feature in features {
        if feature
            .source_tag
            .as_deref()
            .is_some_and(|tag| !valid_xml_name(tag))
        {
            return Err(CodecError::malformed(format_args!(
                "SLDPRT feature {} has an invalid source tag",
                feature.id
            ))
            .into());
        }
        let mut existing = native
            .feature_histories
            .iter_mut()
            .flat_map(|history| &mut history.features)
            .find(|candidate| record_ids.get(&feature.id) == Some(&candidate.id));
        if patch_input_names {
            if let FeatureDefinition::Operation(FeatureOperation::Extrude {
                solid: Some(solid),
                ..
            }) = feature.evaluation.definition()
            {
                let source_is_surface = existing
                    .as_deref()
                    .and_then(|record| record.input_class.as_deref())
                    == Some("moExtruRefSurface_c");
                if source_is_surface == *solid {
                    return Err(crate::writer::write_error::WriteError::Decline(format!(
                        "feature {} changes the native extrusion class",
                        feature.id
                    )));
                }
            }
        }
        if patch_input_names
            && existing
                .as_deref()
                .and_then(|record| record.input_class.as_deref())
                == Some("moCut_c")
            && matches!(feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. })
                    if *op != cadmpeg_ir::features::BooleanOp::Cut)
        {
            return Err(crate::writer::write_error::WriteError::Decline(format!(
                "feature {} changes the native cut class",
                feature.id
            )));
        }
        let suppressed = feature.suppressed.ok_or_else(|| {
            CodecError::UnsupportedContent(format!(
                "SLDPRT feature {} has unresolved suppression",
                feature.id
            ))
        })?;
        let NeutralFeatureEncoding {
            kind,
            parameters: encoded_parameters,
            mut properties,
        } = NeutralFeatureEncoder {
            feature,
            canonical_parameters: parameters,
            orphaned_parameters: orphaned,
            canonical_features: features,
            canonical_parameter_texts: &canonical_parameter_texts,
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
        if feature.evaluation.outputs().is_empty() {
            let resolved_scope = existing
                .as_deref()
                .and_then(|record| record.properties.get("Scope"))
                .is_some_and(|scope| {
                    let mut tokens = scope
                        .split(',')
                        .map(str::trim)
                        .filter(|token| !token.is_empty())
                        .peekable();
                    tokens.peek().is_some()
                        && tokens.all(|token| matches!(scope_ids.get(token), Some(Some(_))))
                });
            if existing.is_none() || resolved_scope {
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
        let ordinal = feature_ranks[&feature.id];
        let tree_parent = model.feature_parent(&feature.id).and_then(|parent| {
            let source_id = structural_parent_sources.get(parent).copied().flatten();
            match record_ids.get(parent) {
                Some(record_id) => Some(crate::records::TreeParent::Record {
                    record_id: record_id.clone(),
                    source_id,
                }),
                None => source_id.map(crate::records::TreeParent::Source),
            }
        });
        let dimension_properties = parameters
            .iter()
            .filter(|parameter| parameter.owner.as_ref() == Some(&feature.id))
            .filter_map(|parameter| {
                parameter_properties
                    .get(&parameter.id)
                    .map(|properties| (parameter.name.clone(), properties.clone()))
            })
            .collect();
        if let Some(existing) = existing.as_mut() {
            if feature.source_tag.is_some() {
                existing.xml_tag = feature_xml_tag(feature);
            }
            existing.ordinal = ordinal;
            existing.name = feature.name.clone().unwrap_or_default();
            existing.kind = kind;
            existing.suppressed = suppressed;
            existing.tree_parent = tree_parent;
            existing.parameters = encoded_parameters;
            existing.dimension_properties = dimension_properties;
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
                parameters: encoded_parameters,
                dimension_properties,
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
    if !patch_input_names {
        for feature in features {
            if classless_unchanged.contains(&feature.id) {
                continue;
            }
            if let Some(class) = super::input_classes::class(feature.definition().operation()) {
                if let Some(record) = native
                    .feature_histories
                    .iter_mut()
                    .flat_map(|history| &mut history.features)
                    .find(|record| record_ids.get(&feature.id) == Some(&record.id))
                {
                    record.input_class = Some(class.into());
                }
            }
        }
    }
    // Regenerate native evaluation copies before any temporary reprojection.
    // These fields are target-format caches, never an authority for selecting
    // between a parameter binding and a scalar on the feature definition.
    for parameter in parameters {
        if pmi_parameters.contains(&parameter.id) {
            continue;
        }
        let Some(record_id) = parameter
            .owner
            .as_ref()
            .and_then(|owner| record_ids.get(owner))
        else {
            continue;
        };
        let Some(record) = native
            .feature_histories
            .iter_mut()
            .flat_map(|history| &mut history.features)
            .find(|record| &record.id == record_id)
        else {
            continue;
        };
        let properties = record
            .dimension_properties
            .entry(parameter.name.clone())
            .or_default();
        super::parameters::synchronize_dimension_properties(parameter, properties);
    }
    for feature in features {
        let Some(record_id) = record_ids.get(&feature.id) else {
            continue;
        };
        let Some(record) = native
            .feature_histories
            .iter_mut()
            .flat_map(|history| &mut history.features)
            .find(|record| &record.id == record_id)
        else {
            continue;
        };
        let names = record.parameters.keys().cloned().collect::<Vec<_>>();
        for name in names {
            let Some(id) = crate::history::encode::bindings::role_parameter(
                feature,
                &record.parameters,
                name.as_str(),
            )?
            else {
                continue;
            };
            let parameter = parameters
                .iter()
                .find(|parameter| parameter.id == id)
                .ok_or_else(|| {
                    WriteError::Decline(format!(
                        "SLDPRT feature {} references missing parameter {id}",
                        feature.id
                    ))
                })?;
            let properties = record
                .dimension_properties
                .entry(name.to_string())
                .or_default();
            super::parameters::synchronize_dimension_properties(parameter, properties);
        }
        record
            .dimension_properties
            .retain(|_, properties| !properties.is_empty());
    }
    synchronize_neutral_feature_content(features, parameters, &record_ids, native)?;
    for lane in &mut native.feature_input_lanes {
        let inactive = lane.configuration.as_deref().is_some_and(|slot| {
            model.configurations.iter().any(|configuration| {
                !configuration.active
                    && super::configurations::configuration_slot(
                        &configuration.properties,
                        configuration.ordinal,
                    )
                    .to_string()
                        == slot
            })
        });
        if !inactive {
            crate::resolved_features::parameters::sync_feature_scalars(
                &native.feature_histories,
                std::slice::from_mut(lane),
            )?;
        }
    }
    let projected_features = project_features_with_native_inputs(native, &model.sketches)?;
    let mismatches = dependency_mismatches(features, &record_ids, &projected_features)?;
    if let Some((index, expected, canonical)) = mismatches.into_iter().next() {
        let feature = &features[index];
        return Err(CodecError::malformed(format_args!(
            "SLDPRT feature {} dependencies are inconsistent with its operands: expected {expected:?}, projected {canonical:?}; definition {:?}",
            feature.id, feature.definition()
        )).into());
    }
    let written_classes = if patch_input_names {
        std::collections::BTreeSet::new()
    } else {
        crate::resolved_features::write_prepare::synthesize_feature_classes(native)?
    };
    synchronize_feature_content_order(native);
    synchronize_history_content_order(native);
    Ok((renames, written_classes))
}

fn synchronize_neutral_feature_content(
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[DesignParameter],
    record_ids: &HashMap<FeatureId, String>,
    native: &mut crate::native::SldprtNative,
) -> Result<(), WriteError> {
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
                        WriteError::Decline(format!(
                            "SLDPRT feature {} content references missing parameter {}",
                            feature.id, id.as_str()
                        ))
                    })?;
                    if parameter.owner.as_ref() != Some(&feature.id) {
                        return Err(WriteError::Decline(format!(
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
                        WriteError::Decline(format!(
                            "SLDPRT feature {} content references missing feature {}",
                            feature.id, id
                        ))
                    }),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let record_id = record_ids.get(&feature.id).ok_or_else(|| {
            WriteError::Decline(format!(
                "SLDPRT feature {} has no output record",
                feature.id
            ))
        })?;
        let record = native
            .feature_histories
            .iter_mut()
            .flat_map(|history| &mut history.features)
            .find(|record| &record.id == record_id)
            .ok_or_else(|| {
                WriteError::Decline(format!(
                    "SLDPRT feature {} has no output record",
                    feature.id
                ))
            })?;
        record.content = content;
    }
    Ok(())
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
                HistoryContent::Text(_) => {}
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
    use std::collections::HashSet;
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
    fn missing_feature_content_and_output_record_decline_with_owner_identity() {
        use crate::writer::write_error::WriteError;
        use cadmpeg_ir::features::FeatureSourceContent;
        use std::collections::HashMap;

        let decoded = SldprtCodec
            .decode(
                &mut Cursor::new(crate::test_support::container::native_envelope_fixture(
                    &document_with_one_linked_object_name(),
                )),
                &DecodeOptions::default(),
            )
            .expect("synthetic history decodes");
        let mut feature = decoded.ir().model.features[0].clone();
        let owner = feature.id.to_string();
        feature.source_content = vec![FeatureSourceContent::Feature(
            FeatureId::mint("synthetic:test:id#missing").expect("feature identity"),
        )]
        .try_into()
        .expect("distinct feature content");
        let mut native = sldprt_native(decoded.ir());
        let error = super::synchronize_neutral_feature_content(
            std::slice::from_ref(&feature),
            &[],
            &HashMap::new(),
            &mut native,
        )
        .expect_err("missing child declines the patch");
        assert!(
            matches!(error, WriteError::Decline(reason) if reason == format!("SLDPRT feature {owner} content references missing feature synthetic:test:id#missing"))
        );

        feature.source_content = cadmpeg_ir::features::FeatureContent::text(["text".to_owned()]);
        let error = super::synchronize_neutral_feature_content(
            &[feature],
            &[],
            &HashMap::new(),
            &mut native,
        )
        .expect_err("missing output record declines the patch");
        assert!(
            matches!(error, WriteError::Decline(reason) if reason == format!("SLDPRT feature {owner} has no output record"))
        );
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
        let mut model = decoded.ir().model.clone();
        model.features[0].name = Some("PatternRenamed".into());

        let mut native = sldprt_native(decoded.ir());
        let mut second = native.feature_input_lanes[0].clone();
        second.id = format!("{}-second", second.id);
        native.feature_input_lanes.push(second);

        let error = sync_neutral_features(
            &model,
            &[],
            &[],
            &mut Some(native),
            &std::collections::HashSet::new(),
        )
        .expect_err("two lanes state the same object name");
        assert_eq!(
            error.to_string(),
            "unsupported writer content: SLDPRT feature-input name for \"MatrizL1\" is not uniquely linked"
        );
    }

    #[test]
    fn a_history_only_rename_requires_no_feature_input_name_patch() {
        let source = crate::test_support::history::sldprt_with_body_and_history(&triangle_body());
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("history-only fixture decodes");
        let mut model = decoded.ir().model.clone();
        let mut native = Some(sldprt_native(decoded.ir()));
        assert!(native.as_ref().unwrap().feature_input_lanes.is_empty());
        model.features[0].name = Some("RenamedBoss".into());

        let renames = sync_neutral_features(
            &model,
            &model.parameters,
            &model.bodies,
            &mut native,
            &std::collections::HashSet::new(),
        )
        .expect("no absent feature-input lane requires a name patch");
        assert!(renames.is_empty());
        assert_eq!(
            native.unwrap().feature_histories[0].features[0].name,
            "RenamedBoss"
        );
    }

    #[test]
    fn an_existing_feature_input_lane_still_requires_its_name_binding() {
        let decoded = SldprtCodec
            .decode(
                &mut Cursor::new(document_with_one_linked_object_name()),
                &DecodeOptions::default(),
            )
            .expect("linked fixture decodes");
        let mut model = decoded.ir().model.clone();
        model.features[0].name = Some("PatternRenamed".into());
        let mut native = sldprt_native(decoded.ir());
        native.feature_input_lanes[0].names.clear();

        let error = sync_neutral_features(
            &model,
            &[],
            &[],
            &mut Some(native),
            &std::collections::HashSet::new(),
        )
        .expect_err("an existing lane with a missing name binding cannot be patched");
        assert_eq!(
            error.to_string(),
            "unsupported writer content: SLDPRT feature-input name for \"MatrizL1\" is not uniquely linked"
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
        let mut model = decoded.ir().model.clone();
        model.features[0].name = Some(" ".into());

        let error = sync_neutral_features(
            &model,
            &[],
            &[],
            &mut Some(sldprt_native(decoded.ir())),
            &std::collections::HashSet::new(),
        )
        .expect_err("a blank object name is not writable");
        assert_eq!(
            error.to_string(),
            "unsupported writer content: SLDPRT feature-input name for \"MatrizL1\" has no non-blank replacement"
        );
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
    #[test]
    fn the_source_id_allocator_skips_held_ids_and_refuses_an_exhausted_space() {
        let mut used = HashSet::from([1, 2, 4]);
        let mut next = Some(1);
        assert_eq!(
            super::allocate_feature_source_id(&mut used, &mut next).expect("first free id"),
            3
        );
        assert_eq!(
            super::allocate_feature_source_id(&mut used, &mut next).expect("second free id"),
            5
        );

        let mut used = HashSet::new();
        let mut next = Some(u32::MAX - 1);
        assert_eq!(
            super::allocate_feature_source_id(&mut used, &mut next).expect("last present id"),
            u32::MAX - 1
        );
        let error = super::allocate_feature_source_id(&mut used, &mut next)
            .expect_err("reserved absent marker is not an identity");
        assert!(
            matches!(&error, crate::writer::write_error::WriteError::Capacity { field, value } if field.field == "feature.source_id" && field.limit == u64::from(u32::MAX - 1) && *value == u64::from(u32::MAX)),
            "{error:?}"
        );
    }
}
