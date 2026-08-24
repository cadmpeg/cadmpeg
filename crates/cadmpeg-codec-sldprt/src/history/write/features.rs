// SPDX-License-Identifier: Apache-2.0
//! Neutral feature synchronization into native history records.

#![allow(unused_imports)]
use crate::classification::{
    classify, classify_type_token, classify_xml_element, native_object_class,
    principal_plane_with_siblings, FeatureClass, NativeClassKind,
};
use crate::records::{Configuration, Feature, FeatureContent, FeatureHistory, HistoryContent};
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::features::{
    Angle, AxisAngle, BodyRetentionMode, BodySelection, BooleanOp, ChamferForm, ChamferSpec,
    ConfigurationBodies, ConfigurationId, CosmeticThreadExtent, CurveProjectionDirection,
    CurveProjectionDirectionState, DatumPlaneReference, DesignConfiguration, DesignParameter,
    DimensionDisplay, EdgeSelection, ExtrudeExtent, ExtrudeSide, FaceMotion, FaceSelection,
    FeatureDefinition, FeatureId, FeatureSourceContent, FeatureTreeNodeRole, FlexForm, FlexMode,
    HoleBottom, HoleForm, HoleKind, Length, ParameterId, ParameterValue, PathRef, PatternForm,
    PatternKind, PatternSeed, ProfileRef, RadiusForm, RadiusSpec, RevolutionAxis,
    RevolutionConstruction, RevolveExtent, RibConstruction, RibDraft, RibSide, RuledSurfaceMode,
    ScaleCenter, ScaleFactors, SketchSpace, SplitFaceTool, SurfaceExtension, SweepMode,
    Termination, TrimRegion, VariableRadius, VertexSelection, WrapMode,
};
use cadmpeg_ir::geometry::{Curve, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::AttributeId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Body, Edge, Face};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::Exactness;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

use crate::history::classify::{
    feature_tree_node_role, is_custom_property, principal_plane_in_history,
};
use crate::history::configuration::enrich_history_parameters_semantic;
use crate::history::parameters::apply_evaluated_parameters;
use crate::history::project::neutral_feature_id;

use super::parameters::restore_equivalent_parameter_expressions;
use super::project_features_with_native_inputs;
use super::xml::{feature_xml_tag, valid_xml_name};
use crate::history::encode::{NeutralFeatureEncoder, NeutralFeatureEncoding};

pub(crate) fn synchronize_feature_input_names(
    features: &[cadmpeg_ir::features::Feature],
    native: &mut crate::native::SldprtNative,
) -> Result<(), CodecError> {
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

    for (old_name, new_name, input_class) in renames {
        let mut matches = Vec::<(usize, usize)>::new();
        for (lane_index, lane) in native.feature_input_lanes.iter().enumerate() {
            for class in lane
                .classes
                .iter()
                .filter(|class| class.name == input_class)
            {
                let name_offset = class.offset + 6 + class.name.len() as u64;
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
        native.feature_input_lanes[*lane_index].names[*name_index].value = new_name;
    }
    Ok(())
}

pub(crate) fn generated_feature_record_id(feature: &FeatureId) -> String {
    format!("sldprt:generated:feature#{}", feature.0)
}

pub(crate) fn generated_feature_source_ids(
    features: &[cadmpeg_ir::features::Feature],
    native: &crate::native::SldprtNative,
) -> Result<HashMap<FeatureId, String>, CodecError> {
    let mut used = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .filter_map(|feature| feature.source_id.as_deref()?.parse::<u32>().ok())
        .collect::<HashSet<_>>();
    let existing = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .filter_map(|feature| {
            Some((
                feature.id.as_str(),
                feature.source_id.as_deref()?.parse::<u32>().ok()?,
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut next = 1u32;
    let mut allocated = HashMap::new();
    for feature in features
        .iter()
        .filter(|feature| feature.native_ref.is_none())
    {
        let record_id = generated_feature_record_id(&feature.id);
        let source_id = if let Some(source_id) = existing.get(record_id.as_str()).copied() {
            source_id
        } else {
            while used.contains(&next) {
                next = next.checked_add(1).ok_or_else(|| {
                    CodecError::Malformed("SLDPRT feature source-id space is exhausted".into())
                })?;
            }
            let source_id = next;
            used.insert(source_id);
            next = next.checked_add(1).unwrap_or(next);
            source_id
        };
        allocated.insert(feature.id.clone(), source_id.to_string());
    }
    Ok(allocated)
}

/// Apply neutral native-feature edits to the `SolidWorks` history used for writing.
pub fn sync_neutral_features(
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[DesignParameter],
    bodies: &[Body],
    native: &mut Option<crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    if features.is_empty() {
        if let Some(native) = native {
            for history in &mut native.feature_histories {
                history.features.retain(is_custom_property);
            }
        }
        return Ok(());
    }
    if native.is_none() {
        *native = Some(crate::native::SldprtNative {
            version: crate::native::SLDPRT_NATIVE_VERSION,
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
    }
    let native = native.as_mut().expect("initialized above");
    let original_parameters = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.clone(), feature.parameters.clone()))
        .collect::<HashMap<_, _>>();
    let mut resolved_histories = native.feature_histories.clone();
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
    apply_evaluated_parameters(&mut resolved_histories);
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

    synchronize_feature_input_names(features, native)?;

    let generated_sources = generated_feature_source_ids(features, native)?;
    let parent_sources = features
        .iter()
        .map(|feature| {
            let source_id = native
                .feature_histories
                .iter()
                .flat_map(|history| &history.features)
                .find(|candidate| feature.native_ref.as_deref() == Some(candidate.id.as_str()))
                .and_then(|candidate| candidate.source_id.clone())
                .or_else(|| generated_sources.get(&feature.id).cloned())
                .unwrap_or_else(|| feature.id.0.clone());
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
                .and_then(|candidate| candidate.source_id.clone())
                .or_else(|| generated_sources.get(&feature.id).cloned());
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
                .filter_map(|feature| Some((feature.source_id.as_deref()?, feature)))
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
                .as_ref()
                .map(|source| (feature.id.clone(), source.clone()))
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
        .filter_map(|feature| match &feature.definition {
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch),
                ..
            } => parent_sources
                .get(&feature.id)
                .map(|source| (sketch.clone(), source.clone())),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let body_sources = bodies
        .iter()
        .map(|body| (body.id.clone(), body.id.0.clone()))
        .collect::<HashMap<_, _>>();

    for feature in features {
        if feature
            .source_tag
            .as_deref()
            .is_some_and(|tag| !valid_xml_name(tag))
        {
            return Err(CodecError::malformed(format_args!(
                "SLDPRT feature {} has an invalid source tag",
                feature.id
            )));
        }
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
        if let Some(record) = existing.as_deref() {
            restore_equivalent_parameter_expressions(
                record,
                &original_parameters,
                &evaluated_parameters,
                &mut parameters,
            );
        }
        if feature.outputs.is_empty() {
            if existing.is_none() {
                properties.remove("Scope");
            }
        } else {
            let scope = feature
                .outputs
                .iter()
                .map(|body| body_sources.get(body).cloned())
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "SLDPRT feature {} references a missing output body",
                        feature.id
                    ))
                })?;
            properties.insert("Scope".into(), scope.join(","));
        }
        let ordinal = u32::try_from(feature.ordinal)
            .map_err(|_| CodecError::Malformed("feature ordinal exceeds u32".into()))?;
        let parent_source_id = feature
            .parent
            .as_ref()
            .and_then(|parent| structural_parent_sources.get(parent).cloned().flatten());
        let tree_parent = feature
            .parent
            .as_ref()
            .and_then(|parent| record_ids.get(parent).cloned());
        if let Some(existing) = existing.as_mut() {
            if let Some(tag) = &feature.source_tag {
                existing.xml_tag.clone_from(tag);
            }
            existing.ordinal = ordinal;
            existing.name = feature.name.clone().unwrap_or_default();
            existing.kind = kind;
            existing.suppressed = suppressed;
            existing.parent_source_id = parent_source_id;
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
                source_id: generated_sources.get(&feature.id).cloned(),
                parent_source_id,
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
    let projected_features = project_features_with_native_inputs(native);
    let projected_features = projected_features
        .into_iter()
        .map(|feature| (feature.id.clone(), feature))
        .collect::<HashMap<_, _>>();
    for feature in features {
        let projected_id = neutral_feature_id(&record_ids[&feature.id]);
        let expected = feature
            .dependencies
            .iter()
            .map(|dependency| {
                record_ids
                    .get(dependency)
                    .map_or_else(|| dependency.clone(), |record| neutral_feature_id(record))
            })
            .collect::<Vec<_>>();
        let consistent = projected_features
            .get(&projected_id)
            .is_some_and(|projected| {
                if feature.native_ref.is_some() {
                    projected.dependencies == expected
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
    Ok(())
}

pub(crate) fn synchronize_neutral_feature_content(
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
                            feature.id, id.0
                        ))
                    })?;
                    if parameter.owner.as_ref() != Some(&feature.id) {
                        return Err(CodecError::malformed(format_args!(
                            "SLDPRT feature {} content references parameter {} owned by another feature",
                            feature.id, id.0
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

pub(crate) fn synchronize_history_content_order(native: &mut crate::native::SldprtNative) {
    for history in &mut native.feature_histories {
        let configurations = history
            .configurations
            .iter()
            .map(|configuration| (configuration.ordinal, configuration.id.clone()))
            .collect::<Vec<_>>();
        let mut features = history
            .features
            .iter()
            .filter(|feature| feature.tree_parent.is_none() && feature.parent_source_id.is_none())
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

pub(crate) fn synchronize_feature_content_order(native: &mut crate::native::SldprtNative) {
    for history in &mut native.feature_histories {
        let mut children = HashMap::<String, Vec<(u32, String)>>::new();
        for feature in &history.features {
            if let Some(parent) = &feature.tree_parent {
                children
                    .entry(parent.clone())
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
