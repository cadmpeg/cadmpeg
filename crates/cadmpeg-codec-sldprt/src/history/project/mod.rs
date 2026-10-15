// SPDX-License-Identifier: Apache-2.0
//! Project native Keywords records into the neutral feature arena.

use crate::classification::{classify, FeatureClass};
use crate::records::{Feature, FeatureContent, FeatureHistory};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::AttributeId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::{
    features::{
        ConfigurationId, DatumPlaneReference, DesignConfiguration, FeatureDefinition, FeatureId,
        FeatureOperation, FeatureSourceContent, ParameterId, PathRef, PlanarProfileRef, ProfileRef,
        SplitFaceTool, UnresolvedFamily,
    },
    scalar::Length,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::{self, Write};

use crate::history::classify::{
    feature_tree_node_role, is_custom_property, is_history_metadata_record, is_offset_plane,
    is_semantic_note, principal_plane_in_history,
};
use crate::history::literals::{parse_point3_mm, parse_vector3, valid_plane_frame};
use crate::records::FeatureSource;

#[cfg(test)]
mod content_tests;
#[cfg(test)]
mod custom_property_tests;
pub(super) mod datum;
#[cfg(test)]
mod dimension_projection_tests;
#[cfg(test)]
mod feature_projection_tests;
pub(crate) mod modify;
pub(crate) mod pattern;
#[cfg(test)]
mod regeneration_tests;
pub(super) mod sketch;
pub(crate) mod solid;
mod spin;
#[cfg(test)]
mod split_and_identity_tests;
mod surface;

use self::datum::{
    project_composite_curve, project_datum_axis, project_datum_coordinate_system,
    project_datum_plane, project_datum_point, project_equation_curve, project_helix,
    project_native_axis_helix, project_offset_plane, project_projected_curve, project_wrap,
};
use self::modify::{
    project_chamfer, project_combine, project_cut_with_surface, project_delete_body,
    project_delete_face, project_dome, project_draft, project_fillet, project_flex,
    project_move_body, project_move_face, project_replace_face, project_scale, project_shell,
    project_thicken,
};
use self::pattern::project_pattern;
use self::sketch::{project_cosmetic_thread, project_split_face, sketch_block_placement};
use self::solid::{project_extrude, project_hole};
use self::spin::{project_loft, project_revolve, project_rib, project_sweep};
use self::surface::{
    project_extend_surface, project_filled_surface, project_knit_surface, project_offset_surface,
    project_ruled_surface, project_trim_surface,
};

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

pub(crate) struct FeatureProjection {
    pub(super) features: Vec<cadmpeg_ir::features::Feature>,
    regeneration_parents: Vec<(FeatureId, FeatureId)>,
}

pub(super) fn neutral_feature_id(native_id: &str) -> Result<FeatureId, CodecError> {
    let key = native_id
        .strip_prefix("sldprt:history:feature#")
        .unwrap_or(native_id);
    let id = format!("sldprt:model:feature#{}", EncodedNativeKey(key));
    FeatureId::mint(id).map_err(CodecError::malformed)
}

fn source_lookup_key(source: FeatureSource) -> String {
    match source {
        FeatureSource::Reserved => "-1".to_owned(),
        FeatureSource::Id(id) => id.value().to_string(),
    }
}

impl FeatureProjection {
    /// Install every projected feature and every regeneration edge the source
    /// states. An edge the model refuses is reported as a loss naming both
    /// features, both ordinals and the model's refusal; nothing is dropped
    /// silently, and the projection recomputes no condition the model owns.
    pub(crate) fn install(
        self,
        model: &mut cadmpeg_ir::document::Model,
        losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    ) -> Result<(), CodecError> {
        model.features = self.features;
        for (child, parent) in self.regeneration_parents {
            let error = match model.set_feature_regeneration_parent(&child, &parent) {
                Ok(()) => continue,
                Err(CodecError::Malformed(error)) => error,
                Err(error) => return Err(error),
            };
            let ordinal = |id: &FeatureId| {
                model
                    .features
                    .iter()
                    .find(|feature| feature.id == *id)
                    .map(|feature| feature.ordinal)
            };
            let child_ordinal = ordinal(&child);
            let parent_ordinal = ordinal(&parent);
            let message = format!(
                "regeneration edge from child `{child}` (ordinal {}) to parent \
             `{parent}` (ordinal {}) was not installed: {error}",
                FeatureOrdinal(child_ordinal),
                FeatureOrdinal(parent_ordinal),
            );
            losses.push(crate::loss::SldprtLossCode::FeatureIncoherentEdges.note(message));
        }
        Ok(())
    }

    pub(super) fn into_model(
        self,
    ) -> Result<
        (
            cadmpeg_ir::document::Model,
            Vec<cadmpeg_ir::report::loss::LossNote>,
        ),
        CodecError,
    > {
        let mut model = cadmpeg_ir::document::Model::default();
        let mut losses = Vec::new();
        self.install(&mut model, &mut losses)?;
        Ok((model, losses))
    }
}

struct FeatureOrdinal(Option<u64>);

impl fmt::Display for FeatureOrdinal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(value) => value.fmt(formatter),
            None => formatter.write_str("missing"),
        }
    }
}

pub(crate) fn project_feature_model(
    ctx: &DecodeContext<'_>,
    histories: &[FeatureHistory],
) -> Result<FeatureProjection, cadmpeg_core::CodecError> {
    let (mut features, parents) = histories.iter().try_fold(
        (Vec::new(), Vec::new()),
        |(mut features, mut parents), history| -> Result<_, CodecError> {
            let source_bindings = unique_source_bindings(history)?;
            let mut by_source = HashMap::new();
            let mut native_by_source = HashMap::new();
            for (source, binding) in &source_bindings {
                let Some(SourceBinding { native, neutral }) = binding else {
                    continue;
                };
                by_source.insert(source_lookup_key(*source), neutral.clone());
                native_by_source.insert(source_lookup_key(*source), *native);
            }
            for feature in &history.features {
                by_source.insert(feature.id.clone(), neutral_feature_id(&feature.id)?);
            }
            let mut by_native = HashMap::new();
            let mut features_by_source = HashMap::new();
            for feature in &history.features {
                if !is_history_metadata_record(feature, &history.features) {
                    by_native.insert(feature.id.as_str(), neutral_feature_id(&feature.id)?);
                }
                if let Some(source) = feature.source_id {
                    features_by_source.insert(source, feature);
                }
            }
            let source_ordered = history.features.iter().any(|feature| {
                feature.input_class.is_none()
                    && feature.xml_tag.eq_ignore_ascii_case("Extrusion")
                    && feature.parameters.len() == 1
                    && feature.source_value().is_some_and(|source| source > 0)
            });
            for feature in history
                .features
                .iter()
                .filter(|feature| !is_history_metadata_record(feature, &history.features))
            {
                let parent = if let Some(parent) = feature
                    .tree_parent_record_id()
                    .and_then(|parent| by_native.get(parent))
                {
                    Some(parent.clone())
                } else if let Some(source) = feature.parent_source_id() {
                    let key = source_lookup_key(source);
                    by_source.get(&key).cloned()
                } else {
                    None
                };
                let projected = cadmpeg_ir::features::Feature {
                    id: neutral_feature_id(&feature.id)?,
                    ordinal: source_ordered
                        .then(|| feature.source_value().map(u64::from))
                        .flatten()
                        .filter(|source| *source > 0)
                        .unwrap_or(u64::from(feature.ordinal)),
                    name: (!feature.name.is_empty()).then(|| feature.name.clone()),
                    suppressed: Some(feature.suppressed),
                    dependencies: project_feature_dependencies(ctx, feature, &by_source)?,
                    source_properties: feature.properties.clone(),
                    source_tag: Some(feature.xml_tag.clone()),
                    source_text: feature.text.as_deref().map(str::to_owned),
                    source_content: project_feature_content(feature, &by_native)?,

                    evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                        project_definition(
                            ctx,
                            feature,
                            &by_source,
                            &native_by_source,
                            &features_by_source,
                            &history.features,
                        )?,
                    ),
                    native_ref: Some(feature.id.clone()),
                };
                features.push(projected);
                parents.push(parent);
            }
            Ok((features, parents))
        },
    )?;
    let mut regeneration_parents = Vec::new();
    for (child_index, parent) in parents.into_iter().enumerate() {
        let Some(parent) = parent else {
            continue;
        };
        let child_text = features[child_index].id.as_str().to_string();
        let child = FeatureId::mint(child_text).map_err(CodecError::malformed)?;
        let mut tree_parent_index = None;
        for (index, feature) in features.iter().enumerate() {
            if feature.id == parent
                && matches!(
                    feature.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::TreeNode { .. })
                )
            {
                tree_parent_index = Some(index);
                break;
            }
        }
        let Some(tree_parent_index) = tree_parent_index else {
            regeneration_parents.push((child, parent));
            continue;
        };
        let mut result = Ok(());
        features[tree_parent_index]
            .evaluation
            .edit(|definition, _| {
                if let FeatureDefinition::Operation(FeatureOperation::TreeNode {
                    children, ..
                }) = definition
                {
                    result = children.insert(ctx, child, "collect SLDPRT tree children");
                }
            });
        result?;
    }
    bind_offset_plane_references(ctx, &mut features)?;
    bind_native_construction_features(ctx, &mut features, histories)?;
    Ok(FeatureProjection {
        features,
        regeneration_parents,
    })
}

pub(crate) fn project_features(
    ctx: &DecodeContext<'_>,
    histories: &[FeatureHistory],
) -> Result<Vec<cadmpeg_ir::features::Feature>, cadmpeg_core::CodecError> {
    project_feature_model(ctx, histories).map(|projection| projection.features)
}

/// Project standalone history notes into the semantic-annotation arena.
pub(crate) fn project_semantic_notes(
    histories: &[FeatureHistory],
) -> Result<Vec<cadmpeg_ir::semantic_annotations::SemanticAnnotation>, CodecError> {
    let mut notes = Vec::new();
    for history in histories {
        for feature in &history.features {
            if !is_semantic_note(feature) {
                continue;
            }
            let native_id = feature
                .id
                .strip_prefix("sldprt:history:feature#")
                .unwrap_or(&feature.id);
            let id = format!(
                "sldprt:semantic-annotation:note#{}",
                EncodedNativeKey(native_id)
            );
            let id = cadmpeg_ir::semantic_annotations::SemanticAnnotationId::mint(id)
                .map_err(|_| CodecError::malformed("invalid SLDPRT semantic note ID"))?;
            let mut text = Vec::new();
            if let Some(value) = &feature.text {
                let value = value.to_owned();
                text.push(value);
            }
            notes.push(cadmpeg_ir::semantic_annotations::SemanticAnnotation {
                id,
                object: feature.id.clone(),
                kind: cadmpeg_ir::semantic_annotations::SemanticAnnotationKind::Text,
                runtime_type: feature.kind.clone(),
                order: feature.ordinal,
                text,
                references: BTreeMap::new(),
                value: None,
                format: None,
                position: None,
                parameters: BTreeMap::new(),
                assets: Vec::new(),
                native_ref: feature.id.clone(),
            });
        }
    }
    Ok(notes)
}

pub(super) fn bind_offset_plane_references(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
) -> Result<(), CodecError> {
    fn canonical_plane_id<'a>(
        id: &'a FeatureId,
        parents: &'a HashMap<FeatureId, FeatureId>,
    ) -> &'a FeatureId {
        let mut current = id;
        let mut visited = HashSet::new();
        loop {
            if visited.contains(current) {
                break;
            }
            visited.insert(current);
            let Some(parent) = parents.get(current) else {
                break;
            };
            current = parent;
        }
        current
    }

    fn history_key(feature: &cadmpeg_ir::features::Feature) -> Option<&str> {
        feature
            .native_ref
            .as_deref()
            .and_then(|native| native.rsplit_once(':').map(|(history, _)| history))
    }

    const FRAME_TOLERANCE: f64 = 1.0e-8;

    let stored_frame = |feature: &cadmpeg_ir::features::Feature| {
        let origin = parse_point3_mm(feature.source_properties.get("Origin")?)?.get();
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
            parse_point3_mm(feature.source_properties.get("ReferenceFaceOrigin")?)?.get(),
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
        same_scalar(tangent.norm(), 0.0) && same_scalar(signed_distance.abs(), distance.get().abs())
    };
    let mut ordinals = HashMap::new();
    for feature in features.iter() {
        let id = feature.id.clone();
        let value = (
            feature.ordinal,
            match feature.evaluation.definition() {
                FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane { plane }) => {
                    Some(principal_frame(*plane))
                }
                FeatureDefinition::Operation(FeatureOperation::DatumPlane { frame }) => Some((
                    frame.origin().get(),
                    frame.normal().get(),
                    frame.u_axis().get(),
                )),
                FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane { .. }) => {
                    stored_frame(feature)
                }
                _ => None,
            },
            matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane { .. })
            ),
            matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(
                    FeatureOperation::DatumPrincipalPlane { .. }
                        | FeatureOperation::DatumPlane { .. }
                )
            ),
        );
        ordinals.insert(id, value);
    }
    // A zero-distance offset with an explicit feature reference is a geometric
    // alias. Collapse only that provenance chain; independent coincident
    // planes remain distinct candidates and stay ambiguous.
    let mut zero_offset_parents = HashMap::new();
    for feature in features.iter() {
        let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature { feature: reference }),
            distance,
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        if same_scalar(distance.get(), 0.0) {
            let id = feature.id.clone();
            let reference = reference.clone();
            zero_offset_parents.insert(id, reference);
        }
    }
    for feature in features.iter_mut() {
        let explicit_native_reference = feature.source_properties.contains_key("Reference")
            || feature.source_properties.contains_key("Plane");
        let result_frame = stored_frame(feature);
        let source_reference_frame = serialized_reference_frame(feature);
        let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference,
            distance,
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        let Some(DatumPlaneReference::Feature {
            feature: reference_id,
        }) = reference.as_ref()
        else {
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
            feature.evaluation.edit(|definition, _| {
                if let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                    reference,
                    ..
                }) = definition
                {
                    *reference = None;
                }
            });
            feature
                .dependencies
                .retain(|dependency| dependency != &reference_id);
        } else if !feature.dependencies.contains(&reference_id) {
            feature.dependencies.insert(
                ctx,
                reference_id,
                "bind SLDPRT offset plane dependencies",
            )?;
        }
    }
    let mut frames = HashMap::new();
    for feature in features.iter() {
        let frame = match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane { plane }) => {
                principal_frame(*plane)
            }
            FeatureDefinition::Operation(FeatureOperation::DatumPlane { frame }) => (
                frame.origin().get(),
                frame.normal().get(),
                frame.u_axis().get(),
            ),
            _ => continue,
        };
        let id = feature.id.clone();
        frames.insert(id, frame);
    }

    loop {
        let mut changed = false;
        for feature in features.iter() {
            let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference: Some(DatumPlaneReference::Feature { feature: reference }),
                distance,
            }) = feature.evaluation.definition()
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
            let id = feature.id.clone();
            frames.insert(
                id,
                (
                    Point3::new(
                        origin.x + normal.x * distance.get() / normal_length,
                        origin.y + normal.y * distance.get() / normal_length,
                        origin.z + normal.z * distance.get() / normal_length,
                    ),
                    normal,
                    u_axis,
                ),
            );
            changed = true;
        }

        let mut bindings = Vec::new();
        for (index, feature) in features.iter().enumerate() {
            let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference,
                distance,
            }) = feature.evaluation.definition()
            else {
                continue;
            };
            if !matches!(
                reference,
                None | Some(DatumPlaneReference::ResolvedPlane { .. })
            ) {
                continue;
            }
            let Some((origin, normal, _)) = stored_frame(feature) else {
                continue;
            };
            if same_scalar(distance.get(), 0.0) {
                continue;
            }
            let Some(history) = history_key(feature) else {
                continue;
            };
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
                        && same_scalar(signed_distance.abs(), distance.get().abs()))
                    .then_some((
                        &candidate.id,
                        distance.get().abs().copysign(signed_distance),
                    ))
                });
            let mut candidates_by_root = HashMap::new();
            for (candidate, distance) in candidates {
                let root = canonical_plane_id(candidate, &zero_offset_parents);
                candidates_by_root.entry(root).or_insert(distance);
            }
            let mut candidates = candidates_by_root.into_iter();
            let Some((reference, distance)) = candidates.next() else {
                continue;
            };
            if candidates.next().is_some() {
                continue;
            }
            let reference = reference.clone();
            bindings.push((index, (reference, distance)));
        }
        for (index, (reference, distance)) in bindings {
            let Some(distance) = Length::new(distance) else {
                continue;
            };
            let reference_copy = reference.clone();
            let mut bound = false;
            features[index].evaluation.edit(|definition, _| {
                if let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                    reference: slot,
                    distance: stored_distance,
                }) = definition
                {
                    *slot = Some(DatumPlaneReference::Feature {
                        feature: reference_copy,
                    });
                    *stored_distance = distance;
                    bound = true;
                }
            });
            if !bound {
                continue;
            }
            features[index].dependencies.insert(
                ctx,
                reference,
                "bind SLDPRT offset plane dependencies",
            )?;
            changed = true;
        }
        if !changed {
            break;
        }
    }
    for feature in features {
        let properties = &feature.source_properties;
        feature.evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference: reference @ None,
                ..
            }) = definition
            {
                *reference = (|| {
                    Some(DatumPlaneReference::ResolvedPlane {
                        frame: cadmpeg_ir::features::FeatureSupportPlaneFrame::from_parts(
                            parse_point3_mm(properties.get("ReferenceFaceOrigin")?)?,
                            cadmpeg_ir::features::FeatureDirection3::new(parse_vector3(
                                properties.get("ReferenceFaceNormal")?,
                            )?)?,
                            cadmpeg_ir::features::FeatureDirection3::new(parse_vector3(
                                properties.get("ReferenceFaceUAxis")?,
                            )?)?,
                        )?,
                    })
                })();
            }
        });
    }
    Ok(())
}

fn bind_native_construction_features(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[FeatureHistory],
) -> Result<(), CodecError> {
    let mut construction_native_refs = HashSet::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        if !matches!(
            classify(feature),
            Some(
                FeatureClass::Sketch
                    | FeatureClass::SketchBlockInstance
                    | FeatureClass::EquationCurve
                    | FeatureClass::ProjectedCurve
                    | FeatureClass::CompositeCurve
            )
        ) || construction_native_refs.contains(feature.id.as_str())
        {
            continue;
        }
        construction_native_refs.insert(feature.id.as_str());
    }
    let mut feature_ids_by_native = HashMap::new();
    for feature in features.iter() {
        let Some(native) = feature.native_ref.as_deref() else {
            continue;
        };
        if construction_native_refs.contains(native) {
            let native = native.to_owned();
            let id = feature.id.clone();
            feature_ids_by_native.insert(native, id);
        }
    }

    for feature in features {
        let feature_id = &feature.id;
        let dependencies = &mut feature.dependencies;
        let insert_dependency =
            |dependencies: &mut cadmpeg_ir::features::DistinctMembers<FeatureId>,
             target: &FeatureId|
             -> Result<(), CodecError> {
                if target != feature_id && !dependencies.contains(target) {
                    dependencies.insert(
                        ctx,
                        target.clone(),
                        "bind SLDPRT native construction dependencies",
                    )?;
                }
                Ok(())
            };
        let bind_planar = |profile: &mut PlanarProfileRef,
                           dependencies: &mut cadmpeg_ir::features::DistinctMembers<FeatureId>|
         -> Result<(), CodecError> {
            let PlanarProfileRef::Native(native) = profile else {
                return Ok(());
            };
            let Some(target) = feature_ids_by_native.get(native.as_str()) else {
                return Ok(());
            };
            let target_id = target.clone();
            insert_dependency(dependencies, target)?;
            *profile = PlanarProfileRef::Feature(target_id);
            Ok(())
        };
        let bind = |profile: &mut ProfileRef,
                    dependencies: &mut cadmpeg_ir::features::DistinctMembers<FeatureId>|
         -> Result<(), CodecError> {
            if let ProfileRef::Planar(profile) = profile {
                bind_planar(profile, dependencies)?;
            }
            Ok(())
        };
        let mut result: Result<(), CodecError> = Ok(());
        feature.evaluation.edit(|definition, _| {
            result = (|| {
                match definition {
                    FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) => {
                        bind(profile, dependencies)?;
                    }
                    FeatureDefinition::Operation(FeatureOperation::Wrap { profile, .. }) => {
                        bind_planar(profile, dependencies)?;
                    }
                    FeatureDefinition::Operation(FeatureOperation::Revolve {
                        construction,
                        ..
                    }) => {
                        if let Some(profile) = construction.profile_mut() {
                            bind_planar(profile, dependencies)?;
                        }
                    }
                    FeatureDefinition::Operation(FeatureOperation::Rib {
                        construction, ..
                    }) => {
                        if let Some(profile) = &mut construction.profile {
                            bind_planar(profile, dependencies)?;
                        }
                    }
                    FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) => {
                        if let Some(profile) = shape.referenced_profile_mut() {
                            bind_planar(profile, dependencies)?;
                        }
                    }
                    FeatureDefinition::Operation(FeatureOperation::Loft { sections, .. }) => {
                        for section in sections {
                            if let cadmpeg_ir::features::LoftSection::Profile(profile) = section {
                                bind(profile, dependencies)?;
                            }
                        }
                    }
                    FeatureDefinition::Operation(FeatureOperation::SplitFace {
                        tool: SplitFaceTool::Path(PathRef::Native(native)),
                        ..
                    }) => {
                        if let Some(target) = feature_ids_by_native.get(native.as_str()) {
                            insert_dependency(dependencies, target)?;
                        }
                    }
                    _ => {}
                }
                Ok(())
            })();
        });
        result?;
    }
    Ok(())
}

struct EncodedNativeKey<'a>(&'a str);

impl fmt::Display for EncodedNativeKey<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        if self.0.is_empty() {
            return formatter.write_str("%EMPTY");
        }
        for character in self.0.chars() {
            if character == '%' || character == '#' || character.is_whitespace() {
                let mut buffer = [0u8; 4];
                for byte in character.encode_utf8(&mut buffer).as_bytes() {
                    formatter.write_str("%")?;
                    formatter.write_char(char::from(HEX[usize::from(byte >> 4)]))?;
                    formatter.write_char(char::from(HEX[usize::from(byte & 0x0f)]))?;
                }
            } else {
                formatter.write_char(character)?;
            }
        }
        Ok(())
    }
}

/// Project Keywords custom-property records into document-owned attributes.
pub(crate) fn custom_property_attributes(
    histories: &[FeatureHistory],
) -> Result<Vec<SourceAttribute>, CodecError> {
    let mut attributes = Vec::new();
    for history in histories {
        for feature in &history.features {
            if !is_custom_property(feature) {
                continue;
            }
            let native_id = feature
                .id
                .strip_prefix("sldprt:history:feature#")
                .unwrap_or(&feature.id);
            let id = format!(
                "sldprt:history:custom-property#{}",
                EncodedNativeKey(native_id)
            );
            let id = AttributeId::mint(id)
                .map_err(|_| CodecError::malformed("invalid SLDPRT custom-property ID"))?;
            let name = feature.name.clone();
            let mut values = Vec::new();
            if let Some(text) = &feature.text {
                values.push(AttributeValue::String(text.clone()));
            }
            attributes.push(SourceAttribute {
                id,
                target: AttributeTarget::Document,
                name,
                values,
            });
        }
    }
    Ok(attributes)
}

struct SourceBinding<'a> {
    native: &'a str,
    neutral: FeatureId,
}

fn unique_source_bindings(
    history: &FeatureHistory,
) -> Result<HashMap<FeatureSource, Option<SourceBinding<'_>>>, CodecError> {
    let mut bindings = HashMap::new();
    for feature in &history.features {
        if is_history_metadata_record(feature, &history.features) {
            continue;
        }
        let Some(source) = feature.source_id else {
            continue;
        };
        if let Some(existing) = bindings.get_mut(&source) {
            *existing = None;
            continue;
        }
        let binding = SourceBinding {
            native: feature.id.as_str(),
            neutral: neutral_feature_id(&feature.id)?,
        };
        bindings.insert(source, Some(binding));
    }
    Ok(bindings)
}

pub(crate) fn incomplete_history_reference_features(
    histories: &[FeatureHistory],
) -> Result<usize, CodecError> {
    let mut incomplete = 0usize;
    for history in histories {
        let sources = unique_source_bindings(history)?;
        let native_ids = history
            .features
            .iter()
            .map(|feature| feature.id.as_str())
            .collect::<HashSet<_>>();
        for feature in &history.features {
            let owner_id = neutral_feature_id(&feature.id)?;
            let duplicate_source = feature
                .source_id
                .is_some_and(|source| sources.get(&source).is_some_and(Option::is_none));
            let parent_requested = feature.tree_parent.is_some();
            let parent_resolved = feature
                .tree_parent_record_id()
                .is_some_and(|parent| native_ids.contains(parent))
                || feature
                    .parent_source_id()
                    .is_some_and(|source| sources.get(&source).is_some_and(Option::is_some));
            let incomplete_content = feature.content.iter().any(|item| match item {
                FeatureContent::Feature(child) => !native_ids.contains(child.as_str()),
                FeatureContent::Dimension(name) => !feature.parameters.contains_key(name.as_str()),
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
                    FeatureSource::try_from(reference)
                        .ok()
                        .and_then(|reference| sources.get(&reference))
                        .and_then(Option::as_ref)
                        .is_none_or(|binding| binding.neutral == owner_id)
                });
            if duplicate_source
                || (parent_requested && !parent_resolved)
                || incomplete_content
                || unresolved_dependency
            {
                incomplete += 1;
            }
        }
    }
    Ok(incomplete)
}

fn project_feature_content(
    feature: &Feature,
    by_native: &HashMap<&str, FeatureId>,
) -> Result<cadmpeg_ir::features::FeatureContent, CodecError> {
    if feature.text.is_some() {
        return Ok(cadmpeg_ir::features::FeatureContent::default());
    }
    let mut names = Vec::<&str>::new();
    for content in &feature.content {
        if let FeatureContent::Dimension(name) = content {
            if feature.parameters.contains_key(name.as_str()) && !names.contains(&name.as_str()) {
                names.push(name);
            }
        }
    }
    for name in feature.parameters.keys() {
        if !names.contains(&name.as_str()) {
            names.push(name.as_str());
        }
    }
    let mut result = cadmpeg_ir::features::FeatureContent::default();
    for content in &feature.content {
        let value = match content {
            FeatureContent::Text(text) => FeatureSourceContent::Text(text.to_owned()),
            FeatureContent::Dimension(name) => {
                let Some(ordinal) = names.iter().position(|known| *known == name) else {
                    continue;
                };
                let key = feature
                    .id
                    .strip_prefix("sldprt:history:feature#")
                    .unwrap_or(&feature.id);
                let id = format!("sldprt:model:parameter#{}:{ordinal}", EncodedNativeKey(key));
                let parameter = ParameterId::mint(id).map_err(CodecError::malformed)?;
                if result.iter().any(|value| matches!(value, FeatureSourceContent::Parameter(known) if known == &parameter)) {
                    continue;
                }
                FeatureSourceContent::Parameter(parameter)
            }
            FeatureContent::Feature(id) => {
                let Some(target) = by_native.get(id.as_str()) else {
                    continue;
                };
                FeatureSourceContent::Feature(target.clone())
            }
        };
        result.push(value)?;
    }
    Ok(result)
}

fn project_feature_dependencies(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    by_source: &HashMap<String, FeatureId>,
) -> Result<cadmpeg_ir::features::DistinctMembers<FeatureId>, CodecError> {
    let owner = neutral_feature_id(&feature.id)?;
    let mut dependencies = cadmpeg_ir::features::DistinctMembers::default();
    for property in FEATURE_REFERENCE_PROPERTIES {
        let Some(value) = feature.properties.get(*property) else {
            continue;
        };
        for reference in value
            .split(|character: char| {
                character == ',' || character == ';' || character.is_whitespace()
            })
            .filter(|reference| !reference.is_empty())
        {
            let Some(dependency) = by_source.get(reference) else {
                continue;
            };
            if dependency == &owner || dependencies.contains(dependency) {
                continue;
            }
            dependencies.insert(
                ctx,
                dependency.clone(),
                "collect SLDPRT feature dependencies",
            )?;
        }
    }
    Ok(dependencies)
}

/// Project decoded native configurations under the caller's resource budget.
pub(crate) fn project_configurations(
    histories: &[FeatureHistory],
) -> Result<Vec<DesignConfiguration>, CodecError> {
    let mut projected = Vec::new();
    for history in histories {
        for configuration in &history.configurations {
            let native_id = configuration
                .id
                .strip_prefix("sldprt:history:configuration#")
                .unwrap_or(&configuration.id);
            let id = format!("sldprt:model:configuration#{}", EncodedNativeKey(native_id));
            let id = ConfigurationId::mint(id)
                .map_err(|_| CodecError::malformed("invalid SLDPRT configuration ID"))?;
            let properties = configuration.properties.clone();
            let material = configuration.material.clone();
            let native_ref = configuration.id.clone();
            let name = configuration.name.clone();
            projected.push(DesignConfiguration {
                id,
                ordinal: configuration.ordinal,
                active: false,
                source_index: configuration.source_index,
                name: Some(name),
                material,
                properties,
                bodies: None,
                parameter_values: BTreeMap::new(),
                feature_states: BTreeMap::new(),
                parameter_overrides: BTreeMap::new(),
                native_ref: Some(native_ref),
            });
        }
    }
    Ok(projected)
}

/// Project every native feature dimension into the neutral parameter arena.
fn project_definition(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    by_source: &HashMap<String, FeatureId>,
    native_by_source: &HashMap<String, &str>,
    features_by_source: &HashMap<FeatureSource, &Feature>,
    history_features: &[Feature],
) -> Result<FeatureDefinition, CodecError> {
    if feature.input_class.as_deref() == Some("moBaseBody_c") {
        return Ok(FeatureDefinition::Operation(
            FeatureOperation::StoredGeometry {},
        ));
    }
    if feature.input_class.as_deref() == Some("moPlanarSurface_c") {
        return Ok(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane,
        }));
    }
    if let Some(role) = feature_tree_node_role(feature, history_features) {
        return Ok(FeatureDefinition::Operation(FeatureOperation::TreeNode {
            role,
            children: cadmpeg_ir::features::TreeChildren::default(),
        }));
    }
    let class = classify(feature);
    let projected_pattern = if class == Some(FeatureClass::Pattern) {
        Some(project_pattern(feature, by_source, native_by_source))
    } else {
        None
    };
    if class == Some(FeatureClass::CosmeticThread) {
        return Ok(project_cosmetic_thread(feature));
    }
    if class == Some(FeatureClass::Sketch) {
        return Ok(
            if feature.kind.eq_ignore_ascii_case("3DSketch")
                || feature.input_class.as_deref() == Some("mo3DProfileFeature_c")
            {
                FeatureDefinition::Operation(FeatureOperation::SpatialSketch { sketch: None })
            } else {
                FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
                })
            },
        );
    }
    if class == Some(FeatureClass::SketchBlockDefinition) {
        return Ok(FeatureDefinition::Operation(
            FeatureOperation::SketchBlockDefinition { sketch: None },
        ));
    }
    if class == Some(FeatureClass::SketchBlockInstance) {
        return Ok(FeatureDefinition::Operation(
            FeatureOperation::SketchBlockInstance {
                block: feature
                    .properties
                    .get("BlockDefinition")
                    .and_then(|source| by_source.get(source.as_str()))
                    .cloned(),
                placement: sketch_block_placement(feature),
            },
        ));
    }
    if class == Some(FeatureClass::ReferencePlane) && is_offset_plane(feature) {
        return Ok(
            project_offset_plane(feature, by_source).unwrap_or_else(|| native_definition(feature))
        );
    }
    if let Some(plane) = principal_plane_in_history(feature, features_by_source, history_features) {
        return Ok(FeatureDefinition::Operation(
            FeatureOperation::DatumPrincipalPlane { plane },
        ));
    }
    if class == Some(FeatureClass::ReferencePlane) {
        return Ok(project_datum_plane(feature).unwrap_or_else(|| {
            if feature.properties.contains_key("NativeRole") {
                native_definition(feature)
            } else {
                FeatureDefinition::Operation(FeatureOperation::Unresolved {
                    family: UnresolvedFamily::DatumPlane,
                })
            }
        }));
    }
    if class == Some(FeatureClass::ReferenceAxis) {
        return Ok(project_datum_axis(feature).unwrap_or_else(|| native_definition(feature)));
    }
    if class == Some(FeatureClass::ReferencePoint) {
        return Ok(project_datum_point(feature).unwrap_or_else(|| native_definition(feature)));
    }
    if class == Some(FeatureClass::CoordinateSystem) {
        return Ok(project_datum_coordinate_system(feature).unwrap_or(
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumCoordinateSystem,
            }),
        ));
    }
    if class == Some(FeatureClass::EquationCurve) {
        return Ok(project_equation_curve(feature).unwrap_or_else(|| native_definition(feature)));
    }
    if class == Some(FeatureClass::ProjectedCurve) {
        return Ok(project_projected_curve(feature, native_by_source)
            .unwrap_or_else(|| native_definition(feature)));
    }
    if class == Some(FeatureClass::CompositeCurve) {
        return Ok(project_composite_curve(feature, native_by_source)
            .unwrap_or_else(|| native_definition(feature)));
    }
    if class == Some(FeatureClass::Helix) {
        return Ok(match project_helix(feature) {
            Some(definition) => definition,
            None => {
                project_native_axis_helix(feature).unwrap_or_else(|| native_definition(feature))
            }
        });
    }
    if class == Some(FeatureClass::Wrap) {
        return Ok(
            project_wrap(feature, native_by_source).unwrap_or_else(|| native_definition(feature))
        );
    }
    Ok(if class == Some(FeatureClass::Extrude) {
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
        project_combine(ctx, feature)?.unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::CutWithSurface) {
        project_cut_with_surface(feature)
    } else if class == Some(FeatureClass::DeleteBody) {
        project_delete_body(feature).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::DeleteFace) {
        project_delete_face(feature).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::ReplaceFace) {
        project_replace_face(ctx, feature)?.unwrap_or_else(|| native_definition(feature))
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
        project_hole(feature, features_by_source, history_features)?
            .unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::Revolve) {
        project_revolve(feature, native_by_source)
    } else if class == Some(FeatureClass::Pattern) {
        projected_pattern.unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::Sweep) {
        project_sweep(feature, native_by_source).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::Loft) {
        project_loft(feature, native_by_source).unwrap_or_else(|| native_definition(feature))
    } else if class == Some(FeatureClass::Rib) {
        project_rib(feature, native_by_source)
    } else {
        native_definition(feature)
    })
}

fn parameter_names(feature: &Feature) -> Vec<String> {
    let mut names = Vec::new();
    for content in &feature.content {
        if let FeatureContent::Dimension(name) = content {
            if feature.parameters.contains_key(name.as_str()) {
                let name = name.to_owned();
                names.push(name);
            }
        }
    }
    for name in feature.parameters.keys() {
        if !names.iter().any(|known| known == name.as_str()) {
            let name = name.as_str().to_owned();
            names.push(name);
        }
    }
    names
}

pub(super) fn projected_parameter_names(feature: &Feature) -> Vec<String> {
    let mut seen = HashSet::new();
    parameter_names(feature)
        .into_iter()
        .filter(|name| seen.insert(name.clone()))
        .collect()
}

pub(super) fn neutral_parameter_id(
    feature: &Feature,
    ordinal: usize,
) -> Result<ParameterId, CodecError> {
    let key = feature
        .id
        .strip_prefix("sldprt:history:feature#")
        .unwrap_or(&feature.id);
    let id = format!("sldprt:model:parameter#{}:{ordinal}", EncodedNativeKey(key));
    ParameterId::mint(id).map_err(CodecError::malformed)
}

fn native_definition(feature: &Feature) -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::Native {
        kind: feature.kind.clone().into(),
        parameters: feature.parameters.clone(),
    })
}
