// SPDX-License-Identifier: Apache-2.0
//! Project native Keywords records into the neutral feature arena.

use crate::classification::{classify, FeatureClass};
use crate::records::{Feature, FeatureContent, FeatureHistory};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::features::{
    ConfigurationBodies, ConfigurationId, DatumPlaneReference, DesignConfiguration, FaceSelection,
    FeatureDefinition, FeatureId, FeatureSourceContent, Length, ParameterId, PathRef, ProfileRef,
    SketchSpace, SplitFaceTool,
};
use cadmpeg_ir::ids::AttributeId;
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::history::classify::{
    feature_tree_node_role, is_custom_property, is_history_metadata_record, is_offset_plane,
    is_semantic_note, principal_plane_in_history,
};
use crate::history::literals::{parse_point3_mm, parse_vector3, valid_plane_frame};

mod datum;
mod modify;
mod pattern;
mod sketch;
mod solid;
mod spin;
mod surface;

pub(crate) use datum::*;
pub(crate) use modify::*;
pub(crate) use pattern::*;
pub(crate) use sketch::*;
pub(crate) use solid::*;
pub(crate) use spin::*;
pub(crate) use surface::*;

const FEATURE_REFERENCE_PROPERTIES: &[&str] = &[
    "Profile",
    "Path",
    "Profiles",
    "Guides",
    "Seeds",
    "Dependency",
    "Dependencies",
    "ParentFeatures",
    "Planes",
    "DissectableChildren",
    "BlockDefinition",
];

pub fn project_features(histories: &[FeatureHistory]) -> Vec<cadmpeg_ir::features::Feature> {
    let mut features = histories
        .iter()
        .flat_map(|history| {
            let source_bindings = unique_source_bindings(history);
            let mut by_source = source_bindings
                .iter()
                .filter_map(|(source, binding)| {
                    binding
                        .as_ref()
                        .map(|(_, neutral)| (*source, neutral.clone()))
                })
                .collect::<HashMap<_, _>>();
            by_source.extend(
                history
                    .features
                    .iter()
                    .map(|feature| (feature.id.as_str(), neutral_feature_id(&feature.id))),
            );
            let by_native = history
                .features
                .iter()
                .filter(|feature| !is_history_metadata_record(feature, &history.features))
                .map(|feature| (feature.id.as_str(), neutral_feature_id(&feature.id)))
                .collect::<HashMap<_, _>>();
            let native_by_source = source_bindings
                .iter()
                .filter_map(|(source, binding)| {
                    binding.as_ref().map(|(native, _)| (*source, *native))
                })
                .collect::<HashMap<_, _>>();
            let features_by_source = history
                .features
                .iter()
                .filter_map(|feature| Some((feature.source_id.as_deref()?, feature)))
                .collect::<HashMap<_, _>>();
            let source_ordered = history.features.iter().any(|feature| {
                feature.input_class.is_none()
                    && feature.xml_tag.eq_ignore_ascii_case("Extrusion")
                    && feature.parameters.len() == 1
                    && feature
                        .source_id
                        .as_deref()
                        .and_then(|source| source.parse::<u32>().ok())
                        .is_some_and(|source| source > 0)
            });
            history
                .features
                .iter()
                .filter(|feature| !is_history_metadata_record(feature, &history.features))
                .map(move |feature| cadmpeg_ir::features::Feature {
                    id: neutral_feature_id(&feature.id),
                    ordinal: source_ordered
                        .then(|| feature.source_id.as_deref()?.parse::<u64>().ok())
                        .flatten()
                        .filter(|source| *source > 0)
                        .unwrap_or(u64::from(feature.ordinal)),
                    name: (!feature.name.is_empty()).then(|| feature.name.clone()),
                    suppressed: Some(feature.suppressed),
                    parent: feature
                        .tree_parent
                        .as_deref()
                        .and_then(|parent| by_native.get(parent).cloned())
                        .or_else(|| {
                            feature
                                .parent_source_id
                                .as_deref()
                                .and_then(|source| by_source.get(source).cloned())
                        }),
                    dependencies: project_feature_dependencies(feature, &by_source),
                    source_properties: feature.properties.clone(),
                    source_tag: Some(feature.xml_tag.clone()),
                    source_text: feature.text.clone(),
                    source_content: project_feature_content(feature, &by_native),
                    outputs: Vec::new(),
                    definition: project_definition(
                        feature,
                        &by_source,
                        &native_by_source,
                        &features_by_source,
                        &history.features,
                    ),
                    native_ref: Some(feature.id.clone()),
                })
        })
        .collect::<Vec<_>>();
    bind_offset_plane_references(&mut features);
    bind_native_construction_features(&mut features, histories);
    features
}

/// Project standalone history notes into the semantic-annotation arena.
pub fn project_semantic_notes(
    histories: &[FeatureHistory],
) -> Vec<cadmpeg_ir::semantic_annotations::SemanticAnnotation> {
    histories
        .iter()
        .flat_map(|history| &history.features)
        .filter(|feature| is_semantic_note(feature))
        .map(|feature| {
            let key = feature
                .id
                .strip_prefix("sldprt:history:feature#")
                .unwrap_or(&feature.id);
            cadmpeg_ir::semantic_annotations::SemanticAnnotation {
                id: cadmpeg_ir::semantic_annotations::SemanticAnnotationId(format!(
                    "sldprt:semantic-annotation:note#{key}"
                )),
                object: feature.id.clone(),
                kind: cadmpeg_ir::semantic_annotations::SemanticAnnotationKind::Text,
                runtime_type: feature.kind.clone(),
                order: feature.ordinal,
                text: feature.text.iter().cloned().collect(),
                references: BTreeMap::new(),
                value: None,
                format: None,
                position: None,
                parameters: BTreeMap::new(),
                assets: Vec::new(),
                native_ref: feature.id.clone(),
            }
        })
        .collect()
}

pub(crate) fn bind_offset_plane_references(features: &mut [cadmpeg_ir::features::Feature]) {
    fn history_key(feature: &cadmpeg_ir::features::Feature) -> Option<&str> {
        feature
            .native_ref
            .as_deref()
            .and_then(|native| native.rsplit_once(':').map(|(history, _)| history))
    }

    const FRAME_TOLERANCE: f64 = 1.0e-8;

    let stored_frame = |feature: &cadmpeg_ir::features::Feature| {
        let origin = parse_point3_mm(feature.source_properties.get("Origin")?)?;
        let normal = parse_vector3(feature.source_properties.get("Normal")?)?;
        let u_axis = parse_vector3(feature.source_properties.get("UAxis")?)?;
        valid_plane_frame(normal, u_axis).then_some((origin, normal, u_axis))
    };
    let principal_frame = |plane| match plane {
        cadmpeg_ir::features::PrincipalPlane::Front => (
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(0.0, 0.0, -1.0),
        ),
        cadmpeg_ir::features::PrincipalPlane::Top => (
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ),
        cadmpeg_ir::features::PrincipalPlane::Right => (
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, -1.0),
        ),
    };
    let same_scalar = |left: f64, right: f64| {
        (left - right).abs() <= FRAME_TOLERANCE * left.abs().max(right.abs()).max(1.0)
    };
    let plane_frames_match = |left: (Point3, Vector3, Vector3),
                              right: (Point3, Vector3, Vector3)| {
        let left_normal_length = left.1.norm();
        let right_normal_length = right.1.norm();
        if !left_normal_length.is_finite()
            || !right_normal_length.is_finite()
            || left_normal_length <= f64::EPSILON
            || right_normal_length <= f64::EPSILON
        {
            return false;
        }
        let normal_alignment = (left.1.x * right.1.x + left.1.y * right.1.y + left.1.z * right.1.z)
            / (left_normal_length * right_normal_length);
        if !same_scalar(normal_alignment.abs(), 1.0) {
            return false;
        }
        let displacement = Vector3::new(
            right.0.x - left.0.x,
            right.0.y - left.0.y,
            right.0.z - left.0.z,
        );
        let signed_distance =
            (displacement.x * left.1.x + displacement.y * left.1.y + displacement.z * left.1.z)
                / left_normal_length;
        same_scalar(signed_distance, 0.0)
    };
    let plane_normal_matches = |left: (Point3, Vector3, Vector3),
                                right: (Point3, Vector3, Vector3)| {
        let left_normal_length = left.1.norm();
        let right_normal_length = right.1.norm();
        left_normal_length.is_finite()
            && right_normal_length.is_finite()
            && left_normal_length > f64::EPSILON
            && right_normal_length > f64::EPSILON
            && same_scalar(
                ((left.1.x * right.1.x + left.1.y * right.1.y + left.1.z * right.1.z)
                    / (left_normal_length * right_normal_length))
                    .abs(),
                1.0,
            )
    };
    let serialized_reference_frame = |feature: &cadmpeg_ir::features::Feature| {
        Some((
            parse_point3_mm(feature.source_properties.get("ReferenceFaceOrigin")?)?,
            parse_vector3(feature.source_properties.get("ReferenceFaceNormal")?)?,
            parse_vector3(feature.source_properties.get("ReferenceFaceUAxis")?)?,
        ))
    };
    let offset_frame_matches = |reference: (Point3, Vector3, Vector3),
                                result: (Point3, Vector3, Vector3),
                                distance: Length| {
        let reference_normal_length = reference.1.norm();
        let result_normal_length = result.1.norm();
        let normal_dot =
            (reference.1.x * result.1.x + reference.1.y * result.1.y + reference.1.z * result.1.z)
                / (reference_normal_length * result_normal_length);
        if !same_scalar(normal_dot.abs(), 1.0) {
            return false;
        }
        let displacement = Vector3::new(
            result.0.x - reference.0.x,
            result.0.y - reference.0.y,
            result.0.z - reference.0.z,
        );
        let signed_distance = (displacement.x * reference.1.x
            + displacement.y * reference.1.y
            + displacement.z * reference.1.z)
            / reference_normal_length;
        let tangent = Vector3::new(
            displacement.x - reference.1.x * signed_distance / reference_normal_length,
            displacement.y - reference.1.y * signed_distance / reference_normal_length,
            displacement.z - reference.1.z * signed_distance / reference_normal_length,
        );
        same_scalar(tangent.norm(), 0.0) && same_scalar(signed_distance.abs(), distance.0.abs())
    };
    let ordinals = features
        .iter()
        .map(|feature| {
            (
                feature.id.clone(),
                (
                    feature.ordinal,
                    match feature.definition {
                        FeatureDefinition::DatumPrincipalPlane { plane } => {
                            Some(principal_frame(plane))
                        }
                        FeatureDefinition::DatumPlane {
                            origin,
                            normal,
                            u_axis,
                        } => Some((origin, normal, u_axis)),
                        FeatureDefinition::DatumOffsetPlane { .. } => stored_frame(feature),
                        _ => None,
                    },
                    matches!(
                        feature.definition,
                        FeatureDefinition::DatumPrincipalPlane { .. }
                    ),
                    matches!(
                        feature.definition,
                        FeatureDefinition::DatumPrincipalPlane { .. }
                            | FeatureDefinition::DatumPlane { .. }
                    ),
                ),
            )
        })
        .collect::<HashMap<_, _>>();
    // A zero-distance offset with an explicit feature reference is a geometric
    // alias. Collapse only that provenance chain; independent coincident
    // planes remain distinct candidates and stay ambiguous.
    let zero_offset_parents = features
        .iter()
        .filter_map(|feature| {
            let FeatureDefinition::DatumOffsetPlane {
                reference: Some(DatumPlaneReference::Feature(reference)),
                distance,
            } = &feature.definition
            else {
                return None;
            };
            same_scalar(distance.0, 0.0).then_some((feature.id.clone(), reference.clone()))
        })
        .collect::<HashMap<_, _>>();
    let canonical_plane_id = |id: &str| {
        let mut current = FeatureId(id.to_owned());
        let mut visited = HashSet::new();
        while visited.insert(current.clone()) {
            let Some(parent) = zero_offset_parents.get(&current).cloned() else {
                break;
            };
            current = parent;
        }
        current
    };
    for feature in features.iter_mut() {
        let explicit_native_reference = feature.source_properties.contains_key("Reference")
            || feature.source_properties.contains_key("Plane");
        let result_frame = stored_frame(feature);
        let source_reference_frame = serialized_reference_frame(feature);
        let FeatureDefinition::DatumOffsetPlane {
            reference,
            distance,
        } = &mut feature.definition
        else {
            continue;
        };
        let Some(DatumPlaneReference::Feature(reference_id)) = reference.as_ref() else {
            continue;
        };
        let reference_id = reference_id.clone();
        let invalid = reference_id == feature.id
            || match ordinals.get(&reference_id) {
                None => true,
                Some((reference_ordinal, reference_frame, is_principal, is_base_plane)) => {
                    let geometrically_compatible =
                        reference_frame
                            .zip(result_frame)
                            .map(|(reference_frame, result_frame)| {
                                offset_frame_matches(reference_frame, result_frame, *distance)
                            });
                    let explicit_principal_identity_without_face_fallback =
                        explicit_native_reference
                            && *is_principal
                            && !(feature
                                .source_properties
                                .contains_key("ReferenceFaceOrigin")
                                || feature
                                    .source_properties
                                    .contains_key("ReferenceFaceNormal")
                                || feature.source_properties.contains_key("ReferenceFaceUAxis"));
                    let explicit_frame_identity = explicit_native_reference
                        && *is_base_plane
                        && reference_frame.zip(result_frame).is_some_and(
                            |(reference_frame, result_frame)| {
                                plane_normal_matches(reference_frame, result_frame)
                            },
                        )
                        && source_reference_frame.zip(*reference_frame).is_none_or(
                            |(serialized, reference)| plane_frames_match(serialized, reference),
                        );
                    *reference_ordinal >= feature.ordinal
                        && !(explicit_native_reference && geometrically_compatible == Some(true)
                            || explicit_principal_identity_without_face_fallback
                            || explicit_frame_identity)
                }
            };
        if invalid {
            *reference = None;
            feature
                .dependencies
                .retain(|dependency| dependency != &reference_id);
            continue;
        }
        if !feature.dependencies.contains(&reference_id) {
            feature.dependencies.push(reference_id);
        }
    }
    let mut frames = features
        .iter()
        .filter_map(|feature| {
            let frame = match feature.definition {
                FeatureDefinition::DatumPrincipalPlane { plane } => principal_frame(plane),
                FeatureDefinition::DatumPlane {
                    origin,
                    normal,
                    u_axis,
                } => (origin, normal, u_axis),
                _ => return None,
            };
            Some((feature.id.clone(), frame))
        })
        .collect::<HashMap<_, _>>();

    loop {
        let mut changed = false;
        for feature in features.iter() {
            let FeatureDefinition::DatumOffsetPlane {
                reference: Some(DatumPlaneReference::Feature(reference)),
                distance,
            } = &feature.definition
            else {
                continue;
            };
            if frames.contains_key(&feature.id) {
                continue;
            }
            let Some(&(origin, normal, u_axis)) = frames.get(reference) else {
                continue;
            };
            let normal_length = normal.norm();
            frames.insert(
                feature.id.clone(),
                (
                    Point3::new(
                        origin.x + normal.x * distance.0 / normal_length,
                        origin.y + normal.y * distance.0 / normal_length,
                        origin.z + normal.z * distance.0 / normal_length,
                    ),
                    normal,
                    u_axis,
                ),
            );
            changed = true;
        }

        let bindings = features
            .iter()
            .enumerate()
            .filter_map(|(index, feature)| {
                let FeatureDefinition::DatumOffsetPlane {
                    reference,
                    distance,
                } = &feature.definition
                else {
                    return None;
                };
                let frame_reference_pending = matches!(
                    reference,
                    None | Some(DatumPlaneReference::Face {
                        face: FaceSelection::Unresolved,
                        ..
                    })
                );
                if !frame_reference_pending {
                    return None;
                }
                let (origin, normal, _) = stored_frame(feature)?;
                if same_scalar(distance.0, 0.0) {
                    return None;
                }
                let history = history_key(feature)?;
                let serialized_reference_frame = serialized_reference_frame(feature);
                let candidates = features
                    .iter()
                    .filter(|candidate| {
                        candidate.ordinal < feature.ordinal
                            || (serialized_reference_frame.is_some()
                                && ordinals
                                    .get(&candidate.id)
                                    .is_some_and(|(_, _, is_principal, _)| *is_principal))
                    })
                    .filter(|candidate| history_key(candidate) == Some(history))
                    .filter_map(|candidate| {
                        let &(candidate_origin, candidate_normal, candidate_u_axis) =
                            frames.get(&candidate.id)?;
                        if let Some(serialized_reference_frame) = serialized_reference_frame {
                            if !plane_frames_match(
                                serialized_reference_frame,
                                (candidate_origin, candidate_normal, candidate_u_axis),
                            ) {
                                return None;
                            }
                        }
                        let candidate_normal_length = candidate_normal.norm();
                        let result_normal_length = normal.norm();
                        let normal_dot = (normal.x * candidate_normal.x
                            + normal.y * candidate_normal.y
                            + normal.z * candidate_normal.z)
                            / (result_normal_length * candidate_normal_length);
                        if !same_scalar(normal_dot.abs(), 1.0) {
                            return None;
                        }
                        let displacement = Vector3::new(
                            origin.x - candidate_origin.x,
                            origin.y - candidate_origin.y,
                            origin.z - candidate_origin.z,
                        );
                        let signed_distance = (displacement.x * candidate_normal.x
                            + displacement.y * candidate_normal.y
                            + displacement.z * candidate_normal.z)
                            / candidate_normal_length;
                        let tangent = Vector3::new(
                            displacement.x
                                - candidate_normal.x * signed_distance / candidate_normal_length,
                            displacement.y
                                - candidate_normal.y * signed_distance / candidate_normal_length,
                            displacement.z
                                - candidate_normal.z * signed_distance / candidate_normal_length,
                        );
                        (same_scalar(tangent.norm(), 0.0)
                            && same_scalar(signed_distance.abs(), distance.0.abs()))
                        .then_some((
                            canonical_plane_id(candidate.id.as_str()),
                            distance.0.abs().copysign(signed_distance),
                        ))
                    });
                let mut candidates_by_root = HashMap::new();
                for (candidate, distance) in candidates {
                    candidates_by_root.entry(candidate).or_insert(distance);
                }
                let mut candidates = candidates_by_root.into_iter();
                let candidate = candidates.next()?;
                candidates.next().is_none().then_some((index, candidate))
            })
            .collect::<Vec<_>>();
        for (index, (reference, distance)) in bindings {
            let FeatureDefinition::DatumOffsetPlane {
                reference: slot,
                distance: stored_distance,
            } = &mut features[index].definition
            else {
                continue;
            };
            *slot = Some(DatumPlaneReference::Feature(reference.clone()));
            *stored_distance = Length(distance);
            if !features[index].dependencies.contains(&reference) {
                features[index].dependencies.push(reference);
            }
            changed = true;
        }
        if !changed {
            break;
        }
    }
    for feature in features {
        let FeatureDefinition::DatumOffsetPlane {
            reference: reference @ None,
            ..
        } = &mut feature.definition
        else {
            continue;
        };
        *reference = (|| {
            Some(DatumPlaneReference::Face {
                face: FaceSelection::Unresolved,
                origin: parse_point3_mm(feature.source_properties.get("ReferenceFaceOrigin")?)?,
                normal: parse_vector3(feature.source_properties.get("ReferenceFaceNormal")?)?,
                u_axis: parse_vector3(feature.source_properties.get("ReferenceFaceUAxis")?)?,
            })
        })();
    }
}

pub(crate) fn bind_native_construction_features(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[FeatureHistory],
) {
    let construction_native_refs = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter(|feature| {
            matches!(
                classify(feature),
                Some(
                    FeatureClass::Sketch
                        | FeatureClass::SketchBlockInstance
                        | FeatureClass::EquationCurve
                        | FeatureClass::ProjectedCurve
                        | FeatureClass::CompositeCurve
                )
            )
        })
        .map(|feature| feature.id.as_str())
        .collect::<HashSet<_>>();
    let feature_ids_by_native = features
        .iter()
        .filter_map(|feature| {
            let native = feature.native_ref.as_deref()?;
            construction_native_refs
                .contains(native)
                .then_some((native.to_string(), feature.id.clone()))
        })
        .collect::<HashMap<_, _>>();

    for feature in features {
        let mut dependencies = Vec::new();
        let mut bind = |profile: &mut ProfileRef| {
            let ProfileRef::Native(native) = profile else {
                return;
            };
            let Some(target) = feature_ids_by_native.get(native.as_str()) else {
                return;
            };
            *profile = ProfileRef::Feature(target.clone());
            dependencies.push(target.clone());
        };
        match &mut feature.definition {
            FeatureDefinition::Extrude { profile, .. }
            | FeatureDefinition::Wrap { profile, .. } => bind(profile),
            FeatureDefinition::Revolve { construction, .. } => {
                if let Some(profile) = &mut construction.profile {
                    bind(profile);
                }
            }
            FeatureDefinition::Rib { construction, .. } => {
                if let Some(profile) = &mut construction.profile {
                    bind(profile);
                }
            }
            FeatureDefinition::Sweep { section, .. } => {
                if let Some(profile) = section.referenced_profile_mut() {
                    bind(profile);
                }
            }
            FeatureDefinition::Loft { sections, .. } => {
                for section in sections {
                    if let cadmpeg_ir::features::LoftSection::Profile(profile) = section {
                        bind(profile);
                    }
                }
            }
            FeatureDefinition::SplitFace {
                tool: SplitFaceTool::Path(PathRef::Native(native)),
                ..
            } => {
                if let Some(target) = feature_ids_by_native.get(native.as_str()) {
                    dependencies.push(target.clone());
                }
            }
            _ => {}
        }
        for dependency in dependencies {
            if dependency != feature.id && !feature.dependencies.contains(&dependency) {
                feature.dependencies.push(dependency);
            }
        }
    }
}

/// Project Keywords custom-property records into document-owned attributes.
pub(crate) fn custom_property_attributes(histories: &[FeatureHistory]) -> Vec<SourceAttribute> {
    histories
        .iter()
        .flat_map(|history| &history.features)
        .filter(|feature| is_custom_property(feature))
        .map(|feature| {
            let key = feature
                .id
                .strip_prefix("sldprt:history:feature#")
                .unwrap_or(&feature.id);
            SourceAttribute {
                id: AttributeId(format!("sldprt:history:custom-property#{key}")),
                target: AttributeTarget::Document,
                name: feature.name.clone(),
                values: feature
                    .text
                    .iter()
                    .cloned()
                    .map(AttributeValue::String)
                    .collect(),
            }
        })
        .collect()
}

pub(crate) fn unique_source_bindings(
    history: &FeatureHistory,
) -> HashMap<&str, Option<(&str, FeatureId)>> {
    let mut bindings = HashMap::new();
    for feature in &history.features {
        if is_history_metadata_record(feature, &history.features) {
            continue;
        }
        let Some(source) = feature.source_id.as_deref() else {
            continue;
        };
        let binding = (feature.id.as_str(), neutral_feature_id(&feature.id));
        bindings
            .entry(source)
            .and_modify(|existing| *existing = None)
            .or_insert(Some(binding));
    }
    bindings
}

pub(crate) fn incomplete_history_reference_features(histories: &[FeatureHistory]) -> usize {
    histories
        .iter()
        .map(|history| {
            let sources = unique_source_bindings(history);
            let native_ids = history
                .features
                .iter()
                .map(|feature| feature.id.as_str())
                .collect::<HashSet<_>>();
            history
                .features
                .iter()
                .filter(|feature| {
                    let duplicate_source = feature
                        .source_id
                        .as_deref()
                        .is_some_and(|source| sources.get(source).is_some_and(Option::is_none));
                    let parent_requested =
                        feature.tree_parent.is_some() || feature.parent_source_id.is_some();
                    let parent_resolved = feature
                        .tree_parent
                        .as_deref()
                        .is_some_and(|parent| native_ids.contains(parent))
                        || feature
                            .parent_source_id
                            .as_deref()
                            .is_some_and(|source| sources.get(source).is_some_and(Option::is_some));
                    let incomplete_content = feature.content.iter().any(|item| match item {
                        FeatureContent::Feature(child) => !native_ids.contains(child.as_str()),
                        FeatureContent::Dimension(name) => !feature.parameters.contains_key(name),
                        FeatureContent::Text(_) => false,
                    });
                    let unresolved_dependency = FEATURE_REFERENCE_PROPERTIES
                        .iter()
                        .filter_map(|name| feature.properties.get(*name))
                        .flat_map(|value| {
                            value.split(|character: char| {
                                character == ',' || character == ';' || character.is_whitespace()
                            })
                        })
                        .filter(|reference| !reference.is_empty())
                        .any(|reference| {
                            sources.get(reference).and_then(Option::as_ref).is_none_or(
                                |(_, dependency)| dependency == &neutral_feature_id(&feature.id),
                            )
                        });
                    duplicate_source
                        || (parent_requested && !parent_resolved)
                        || incomplete_content
                        || unresolved_dependency
                })
                .count()
        })
        .sum()
}

pub(crate) fn project_feature_content(
    feature: &Feature,
    by_native: &HashMap<&str, FeatureId>,
) -> Vec<FeatureSourceContent> {
    if feature.text.is_some() {
        return Vec::new();
    }
    let parameters = projected_parameter_names(feature)
        .into_iter()
        .enumerate()
        .map(|(ordinal, name)| (name, neutral_parameter_id(feature, ordinal)))
        .collect::<HashMap<_, _>>();
    let mut emitted_parameters = HashSet::new();
    feature
        .content
        .iter()
        .filter_map(|content| match content {
            FeatureContent::Text(text) => Some(FeatureSourceContent::Text(text.clone())),
            FeatureContent::Dimension(name) => parameters
                .get(name)
                .filter(|parameter| emitted_parameters.insert((*parameter).clone()))
                .cloned()
                .map(FeatureSourceContent::Parameter),
            FeatureContent::Feature(id) => by_native
                .get(id.as_str())
                .cloned()
                .map(FeatureSourceContent::Feature),
        })
        .collect()
}

pub(crate) fn project_feature_dependencies(
    feature: &Feature,
    by_source: &HashMap<&str, FeatureId>,
) -> Vec<FeatureId> {
    let owner = neutral_feature_id(&feature.id);
    let mut seen = std::collections::HashSet::new();
    FEATURE_REFERENCE_PROPERTIES
        .iter()
        .filter_map(|name| feature.properties.get(*name))
        .flat_map(|value| {
            value
                .split(|character: char| {
                    character == ',' || character == ';' || character.is_whitespace()
                })
                .filter(|reference| !reference.is_empty())
        })
        .filter_map(|reference| by_source.get(reference).cloned())
        .filter(|dependency| dependency != &owner)
        .filter(|dependency| seen.insert(dependency.clone()))
        .collect()
}

/// Project native configuration records into the neutral configuration arena.
pub fn project_configurations(histories: &[FeatureHistory]) -> Vec<DesignConfiguration> {
    histories
        .iter()
        .flat_map(|history| &history.configurations)
        .map(|configuration| DesignConfiguration {
            id: ConfigurationId(format!(
                "sldprt:model:configuration#{}",
                configuration
                    .id
                    .strip_prefix("sldprt:history:configuration#")
                    .unwrap_or(&configuration.id)
            )),
            ordinal: configuration.ordinal,
            active: false.into(),
            source_index: configuration.source_index,
            name: configuration.name.clone().into(),
            material: configuration.material.clone(),
            properties: configuration.properties.clone(),
            bodies: ConfigurationBodies::Unresolved,
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            suppressed_features: Vec::new(),
            parameter_overrides: BTreeMap::new(),
            native_ref: Some(configuration.id.clone()),
        })
        .collect()
}

/// Project every native feature dimension into the neutral parameter arena.
pub(crate) fn project_definition(
    feature: &Feature,
    by_source: &HashMap<&str, FeatureId>,
    native_by_source: &HashMap<&str, &str>,
    features_by_source: &HashMap<&str, &Feature>,
    history_features: &[Feature],
) -> FeatureDefinition {
    if feature.input_class.as_deref() == Some("moBaseBody_c") {
        return FeatureDefinition::StoredGeometry;
    }
    if feature.input_class.as_deref() == Some("moPlanarSurface_c") {
        return FeatureDefinition::DatumPlaneUnresolved;
    }
    if let Some(role) = feature_tree_node_role(feature, history_features) {
        return FeatureDefinition::TreeNode {
            role,
            children: Vec::new(),
            active_child: None,
        };
    }
    let class = classify(feature);
    if class == Some(FeatureClass::CosmeticThread) {
        return project_cosmetic_thread(feature);
    }
    if class == Some(FeatureClass::Sketch) {
        return if feature.kind.eq_ignore_ascii_case("3DSketch")
            || feature.input_class.as_deref() == Some("mo3DProfileFeature_c")
        {
            FeatureDefinition::SpatialSketch { sketch: None }
        } else {
            FeatureDefinition::Sketch {
                space: SketchSpace::Planar,
                sketch: None,
            }
        };
    }
    if class == Some(FeatureClass::SketchBlockDefinition) {
        return FeatureDefinition::SketchBlockDefinition { sketch: None };
    }
    if class == Some(FeatureClass::SketchBlockInstance) {
        return FeatureDefinition::SketchBlockInstance {
            block: feature
                .properties
                .get("BlockDefinition")
                .and_then(|source| by_source.get(source.as_str()).cloned()),
            placement: sketch_block_placement(feature),
        };
    }
    if class == Some(FeatureClass::ReferencePlane) && is_offset_plane(feature) {
        return project_offset_plane(feature, by_source)
            .unwrap_or_else(|| native_definition(feature));
    }
    if let Some(plane) = principal_plane_in_history(feature, features_by_source, history_features) {
        return FeatureDefinition::DatumPrincipalPlane { plane };
    }
    if class == Some(FeatureClass::ReferencePlane) {
        return project_datum_plane(feature).unwrap_or_else(|| {
            if feature.properties.contains_key("NativeRole") {
                native_definition(feature)
            } else {
                FeatureDefinition::DatumPlaneUnresolved
            }
        });
    }
    if class == Some(FeatureClass::ReferenceAxis) {
        return project_datum_axis(feature).unwrap_or_else(|| native_definition(feature));
    }
    if class == Some(FeatureClass::ReferencePoint) {
        return project_datum_point(feature).unwrap_or_else(|| native_definition(feature));
    }
    if class == Some(FeatureClass::CoordinateSystem) {
        return project_datum_coordinate_system(feature)
            .unwrap_or(FeatureDefinition::DatumCoordinateSystemUnresolved);
    }
    if class == Some(FeatureClass::EquationCurve) {
        return project_equation_curve(feature).unwrap_or_else(|| native_definition(feature));
    }
    if class == Some(FeatureClass::ProjectedCurve) {
        return project_projected_curve(feature, native_by_source)
            .unwrap_or_else(|| native_definition(feature));
    }
    if class == Some(FeatureClass::CompositeCurve) {
        return project_composite_curve(feature, native_by_source)
            .unwrap_or_else(|| native_definition(feature));
    }
    if class == Some(FeatureClass::Helix) {
        return project_helix(feature)
            .or_else(|| project_native_axis_helix(feature))
            .unwrap_or_else(|| native_definition(feature));
    }
    if class == Some(FeatureClass::Wrap) {
        return project_wrap(feature, native_by_source)
            .unwrap_or_else(|| native_definition(feature));
    }
    if class == Some(FeatureClass::Extrude) {
        project_extrude(feature, native_by_source, features_by_source)
            .unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::Fillet) {
        project_fillet(feature)
    } else if class == Some(FeatureClass::Chamfer) {
        project_chamfer(feature)
    } else if class == Some(FeatureClass::Shell) {
        project_shell(feature)
    } else if class == Some(FeatureClass::Thicken) {
        project_thicken(feature)
    } else if class == Some(FeatureClass::OffsetSurface) {
        project_offset_surface(feature)
    } else if class == Some(FeatureClass::KnitSurface) {
        project_knit_surface(feature)
    } else if class == Some(FeatureClass::FilledSurface) {
        project_filled_surface(feature)
    } else if class == Some(FeatureClass::TrimSurface) {
        project_trim_surface(feature, native_by_source)
    } else if class == Some(FeatureClass::ExtendSurface) {
        project_extend_surface(feature)
    } else if class == Some(FeatureClass::RuledSurface) {
        project_ruled_surface(feature).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::Draft) {
        project_draft(feature)
    } else if class == Some(FeatureClass::SplitFace) {
        project_split_face(feature).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::Combine) {
        project_combine(feature).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::CutWithSurface) {
        project_cut_with_surface(feature)
    } else if class == Some(FeatureClass::DeleteBody) {
        project_delete_body(feature).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::DeleteFace) {
        project_delete_face(feature).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::ReplaceFace) {
        project_replace_face(feature).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::MoveFace) {
        project_move_face(feature).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::MoveBody) {
        project_move_body(feature).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::Dome) {
        project_dome(feature)
    } else if class == Some(FeatureClass::Flex) {
        project_flex(feature)
    } else if class == Some(FeatureClass::Scale) {
        project_scale(feature)
    } else if class == Some(FeatureClass::Hole) {
        project_hole(feature, features_by_source, history_features)
    } else if class == Some(FeatureClass::Revolve) {
        project_revolve(feature, native_by_source)
    } else if class == Some(FeatureClass::Pattern) {
        project_pattern(feature, by_source, native_by_source)
    } else if class == Some(FeatureClass::Sweep) {
        project_sweep(feature, native_by_source).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::Loft) {
        project_loft(feature, native_by_source).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::Rib) {
        project_rib(feature, native_by_source)
    } else {
        native_definition(feature)
    }
}

pub(crate) fn parameter_names(feature: &Feature) -> Vec<String> {
    let mut names = feature
        .content
        .iter()
        .filter_map(|content| match content {
            FeatureContent::Dimension(name) if feature.parameters.contains_key(name) => {
                Some(name.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let missing = feature
        .parameters
        .keys()
        .filter(|name| !names.contains(name))
        .cloned()
        .collect::<Vec<_>>();
    names.extend(missing);
    names
}

pub(crate) fn projected_parameter_names(feature: &Feature) -> Vec<String> {
    let mut seen = HashSet::new();
    parameter_names(feature)
        .into_iter()
        .filter(|name| seen.insert(name.clone()))
        .collect()
}

pub(crate) fn neutral_parameter_id(feature: &Feature, ordinal: usize) -> ParameterId {
    let key = feature
        .id
        .strip_prefix("sldprt:history:feature#")
        .unwrap_or(&feature.id);
    ParameterId(format!("sldprt:model:parameter#{key}:{ordinal}"))
}

pub(crate) fn native_definition(feature: &Feature) -> FeatureDefinition {
    FeatureDefinition::Native {
        kind: feature.kind.clone(),
        parameters: feature.parameters.clone(),
        properties: feature.properties.clone(),
    }
}

pub(crate) fn neutral_feature_id(native_id: &str) -> FeatureId {
    let key = native_id
        .strip_prefix("sldprt:history:feature#")
        .unwrap_or(native_id);
    FeatureId(format!("sldprt:model:feature#{key}"))
}
