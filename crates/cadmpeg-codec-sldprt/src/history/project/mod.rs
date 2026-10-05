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

fn charge_projected_text_work(
    ctx: &DecodeContext<'_>,
    text: &str,
    operation: &'static str,
) -> Result<(), CodecError> {
    // Escaping can emit three bytes per input byte; identity validation scans the result.
    let work = u64::try_from(text.len())
        .ok()
        .and_then(|bytes| bytes.checked_mul(4))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(work, operation)
}

pub(super) fn copy_projected_feature_text(
    ctx: &DecodeContext<'_>,
    text: &str,
) -> Result<String, CodecError> {
    charge_projected_text_work(ctx, text, "copy SLDPRT projected feature text")?;
    ctx.format_retained(format_args!("{text}"), "copy SLDPRT projected feature text")
}

pub(super) fn neutral_feature_id_charged(
    ctx: &DecodeContext<'_>,
    native_id: &str,
) -> Result<FeatureId, CodecError> {
    charge_projected_text_work(ctx, native_id, "retain SLDPRT projected feature ID")?;
    let key = native_id
        .strip_prefix("sldprt:history:feature#")
        .unwrap_or(native_id);
    let id = ctx.format_retained(
        format_args!("sldprt:model:feature#{}", EncodedNativeKey(key)),
        "retain SLDPRT projected feature ID",
    )?;
    FeatureId::mint(id).map_err(CodecError::malformed)
}

pub(super) fn copy_projected_feature_id(
    ctx: &DecodeContext<'_>,
    id: &FeatureId,
) -> Result<FeatureId, CodecError> {
    FeatureId::mint(copy_projected_feature_text(ctx, id.as_str())?).map_err(CodecError::malformed)
}

fn source_lookup_key(ctx: &DecodeContext<'_>, source: FeatureSource) -> Result<String, CodecError> {
    match source {
        FeatureSource::Reserved => {
            ctx.format_retained(format_args!("-1"), "retain SLDPRT source lookup key")
        }
        FeatureSource::Id(id) => ctx.format_retained(
            format_args!("{}", id.value()),
            "retain SLDPRT source lookup key",
        ),
    }
}

pub(super) fn copy_projected_feature_properties(
    ctx: &DecodeContext<'_>,
    properties: &BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    operation: &'static str,
) -> Result<BTreeMap<cadmpeg_core::text::NonBlankString, String>, CodecError> {
    let mut copied = BTreeMap::new();
    for (key, value) in properties {
        ctx.charge_work(1, operation)?;
        let key = cadmpeg_core::text::NonBlankString::for_decode(
            ctx,
            copy_projected_feature_text(ctx, key.as_str())?,
            "validate nonblank text",
        )?
        .ok_or_else(|| CodecError::malformed("blank SLDPRT projected feature property"))?;
        let value = copy_projected_feature_text(ctx, value)?;
        ctx.insert_btree_map(&mut copied, key, value, operation)?;
    }
    Ok(copied)
}

impl FeatureProjection {
    /// Install every projected feature and every regeneration edge the source
    /// states. An edge the model refuses is reported as a loss naming both
    /// features, both ordinals and the model's refusal; nothing is dropped
    /// silently, and the projection recomputes no condition the model owns.
    pub(crate) fn install(
        self,
        ctx: &DecodeContext<'_>,
        model: &mut cadmpeg_ir::document::Model,
        losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    ) -> Result<(), CodecError> {
        model.features = self.features;
        for (child, parent) in self.regeneration_parents {
            let error = match model.set_feature_regeneration_parent(ctx, &child, &parent) {
                Ok(()) => continue,
                Err(CodecError::Malformed(error)) => error,
                Err(error) => return Err(error),
            };
            let ordinal = |id: &FeatureId| {
                Ok::<_, CodecError>(
                    ctx.admit_iter(&model.features, "find SLDPRT regeneration ordinal")?
                        .find(|feature| feature.id == *id)
                        .map(|feature| feature.ordinal),
                )
            };
            let child_ordinal = ordinal(&child)?;
            let parent_ordinal = ordinal(&parent)?;
            charge_projected_text_work(
                ctx,
                child.as_str(),
                "retain SLDPRT regeneration edge loss",
            )?;
            charge_projected_text_work(
                ctx,
                parent.as_str(),
                "retain SLDPRT regeneration edge loss",
            )?;
            charge_projected_text_work(ctx, &error, "retain SLDPRT regeneration edge loss")?;
            let message = ctx.format_retained(
                format_args!(
                    "regeneration edge from child `{child}` (ordinal {}) to parent \
                     `{parent}` (ordinal {}) was not installed: {error}",
                    FeatureOrdinal(child_ordinal),
                    FeatureOrdinal(parent_ordinal),
                ),
                "retain SLDPRT regeneration edge loss",
            )?;
            ctx.reserve_vec(losses, 1, "collect SLDPRT regeneration edge losses")?;
            losses.push(crate::loss::SldprtLossCode::FeatureIncoherentEdges.note(message));
        }
        Ok(())
    }

    pub(super) fn into_model(
        self,
        ctx: &DecodeContext<'_>,
    ) -> Result<
        (
            cadmpeg_ir::document::Model,
            Vec<cadmpeg_ir::report::loss::LossNote>,
        ),
        CodecError,
    > {
        let mut model = cadmpeg_ir::document::Model::default();
        let mut losses = Vec::new();
        self.install(ctx, &mut model, &mut losses)?;
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
    let (mut features, parents) = ctx
        .admit_iter(&histories[..], "scan SLDPRT project_feature_model values")?
        .try_fold(
            (Vec::new(), Vec::new()),
            |(mut features, mut parents), history| -> Result<_, CodecError> {
                let source_bindings = unique_source_bindings(ctx, history)?;
                let mut by_source = HashMap::new();
                let mut native_by_source = HashMap::new();
                for (source, binding) in
                    ctx.admit_iter(&source_bindings, "scan SLDPRT project_feature_model values")?
                {
                    let Some(SourceBinding { native, neutral }) = binding else {
                        continue;
                    };
                    ctx.insert_hash_map(
                        &mut by_source,
                        source_lookup_key(ctx, *source)?,
                        copy_projected_feature_id(ctx, neutral)?,
                        "index SLDPRT projected source features",
                    )?;
                    ctx.insert_hash_map(
                        &mut native_by_source,
                        source_lookup_key(ctx, *source)?,
                        *native,
                        "index SLDPRT native source features",
                    )?;
                }
                for feature in ctx.admit_iter(
                    &history.features,
                    "scan SLDPRT project_feature_model values",
                )? {
                    ctx.insert_hash_map(
                        &mut by_source,
                        copy_projected_feature_text(ctx, &feature.id)?,
                        neutral_feature_id_charged(ctx, &feature.id)?,
                        "index SLDPRT projected source features",
                    )?;
                }
                let mut by_native = HashMap::new();
                let mut features_by_source = BTreeMap::new();
                for feature in ctx.admit_iter(
                    &history.features,
                    "scan SLDPRT project_feature_model values",
                )? {
                    if !is_history_metadata_record(ctx, feature, &history.features)? {
                        ctx.insert_hash_map(
                            &mut by_native,
                            feature.id.as_str(),
                            neutral_feature_id_charged(ctx, &feature.id)?,
                            "index SLDPRT projected native features",
                        )?;
                    }
                    if let Some(source) = feature.source_id {
                        ctx.insert_btree_map(
                            &mut features_by_source,
                            source,
                            feature,
                            "index SLDPRT native features by source",
                        )?;
                    }
                }
                let source_ordered = ctx
                    .admit_iter(
                        &history.features,
                        "scan SLDPRT source ordered history features",
                    )?
                    .any(|feature| {
                        feature.input_class.is_none()
                            && feature.xml_tag.eq_ignore_ascii_case("Extrusion")
                            && feature.parameters.len() == 1
                            && feature.source_value().is_some_and(|source| source > 0)
                    });
                for feature in
                    ctx.admit_iter(&history.features, "scan SLDPRT history metadata features")?
                {
                    if is_history_metadata_record(ctx, feature, &history.features)? {
                        continue;
                    }
                    let parent = if let Some(parent) = feature
                        .tree_parent_record_id()
                        .map(|parent| {
                            Ok::<_, cadmpeg_core::CodecError>(ctx.get_hash_map(
                                &(by_native),
                                parent,
                                "look up SLDPRT hash key",
                            )?)
                        })
                        .transpose()?
                        .flatten()
                    {
                        Some(copy_projected_feature_id(ctx, parent)?)
                    } else if let Some(source) = feature.parent_source_id() {
                        let key = source_lookup_key(ctx, source)?;
                        ctx.get_hash_map(&(by_source), &key, "look up SLDPRT hash key")?
                            .map(|parent| copy_projected_feature_id(ctx, parent))
                            .transpose()?
                    } else {
                        None
                    };
                    let projected = cadmpeg_ir::features::Feature {
                        id: neutral_feature_id_charged(ctx, &feature.id)?,
                        ordinal: source_ordered
                            .then(|| feature.source_value().map(u64::from))
                            .flatten()
                            .filter(|source| *source > 0)
                            .unwrap_or(u64::from(feature.ordinal)),
                        name: (!feature.name.is_empty())
                            .then(|| copy_projected_feature_text(ctx, &feature.name))
                            .transpose()?,
                        suppressed: Some(feature.suppressed),
                        dependencies: project_feature_dependencies(ctx, feature, &by_source)?,
                        source_properties: copy_projected_feature_properties(
                            ctx,
                            &feature.properties,
                            "collect SLDPRT projected feature properties",
                        )?,
                        source_tag: Some(copy_projected_feature_text(ctx, &feature.xml_tag)?),
                        source_text: feature
                            .text
                            .as_deref()
                            .map(|text| copy_projected_feature_text(ctx, text))
                            .transpose()?,
                        source_content: project_feature_content(ctx, feature, &by_native)?,

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
                        native_ref: Some(copy_projected_feature_text(ctx, &feature.id)?),
                    };
                    ctx.reserve_vec(&mut features, 1, "collect SLDPRT projected features")?;
                    features.push(projected);
                    ctx.reserve_vec(&mut parents, 1, "collect SLDPRT projected feature parents")?;
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
        charge_projected_text_work(
            ctx,
            features[child_index].id.as_str(),
            "copy SLDPRT tree child ID",
        )?;
        let child_text = ctx.format_retained(
            format_args!("{}", features[child_index].id.as_str()),
            "copy SLDPRT tree child ID",
        )?;
        let child = FeatureId::mint(child_text).map_err(CodecError::malformed)?;
        let mut tree_parent_index = None;
        for (index, feature) in ctx
            .admit_iter(&features, "scan SLDPRT tree parent")?
            .enumerate()
        {
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
            ctx.reserve_vec(
                &mut regeneration_parents,
                1,
                "collect SLDPRT regeneration parents",
            )?;
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
    ctx: &DecodeContext<'_>,
    histories: &[FeatureHistory],
) -> Result<Vec<cadmpeg_ir::semantic_annotations::SemanticAnnotation>, CodecError> {
    const OPERATION: &str = "project SLDPRT semantic notes";
    let mut notes = Vec::new();
    let copy = |value: &str| -> Result<String, CodecError> {
        charge_projected_text_work(ctx, value, OPERATION)?;
        let mut copied = String::new();
        ctx.try_reserve_retained_text(&mut copied, value.len(), OPERATION)?;
        copied.push_str(value);
        Ok(copied)
    };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(histories.len()),
        OPERATION,
    )?;
    for history in histories {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(history.features.len()),
            OPERATION,
        )?;
        for feature in &history.features {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(feature.xml_tag.len()),
                OPERATION,
            )?;
            if !is_semantic_note(feature) {
                continue;
            }
            charge_projected_text_work(ctx, &feature.id, OPERATION)?;
            let native_id = feature
                .id
                .strip_prefix("sldprt:history:feature#")
                .unwrap_or(&feature.id);
            let id = ctx.format_retained(
                format_args!(
                    "sldprt:semantic-annotation:note#{}",
                    EncodedNativeKey(native_id)
                ),
                OPERATION,
            )?;
            let id = cadmpeg_ir::semantic_annotations::SemanticAnnotationId::mint(id)
                .map_err(|_| CodecError::malformed("invalid SLDPRT semantic note ID"))?;
            let mut text = Vec::new();
            if let Some(value) = &feature.text {
                let value = copy(value)?;
                ctx.reserve_vec(&mut text, 1, OPERATION)?;
                text.push(value);
            }
            ctx.reserve_vec(&mut notes, 1, OPERATION)?;
            notes.push(cadmpeg_ir::semantic_annotations::SemanticAnnotation {
                id,
                object: copy(&feature.id)?,
                kind: cadmpeg_ir::semantic_annotations::SemanticAnnotationKind::Text,
                runtime_type: copy(&feature.kind)?,
                order: feature.ordinal,
                text,
                references: BTreeMap::new(),
                value: None,
                format: None,
                position: None,
                parameters: BTreeMap::new(),
                assets: Vec::new(),
                native_ref: copy(&feature.id)?,
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
        ctx: &DecodeContext<'_>,
        id: &'a FeatureId,
        parents: &'a HashMap<FeatureId, FeatureId>,
    ) -> Result<&'a FeatureId, CodecError> {
        let mut current = id;
        let mut visited = HashSet::new();
        loop {
            ctx.charge_work(1, "walk SLDPRT zero offset plane parents")?;
            if ctx.contains_hash_set(&(visited), current, "test SLDPRT hashed identity")? {
                break;
            }
            ctx.insert_hash_set(
                &mut visited,
                current,
                "walk SLDPRT zero offset plane parents",
            )?;
            let Some(parent) = ctx.get_hash_map(&(parents), current, "look up SLDPRT hash key")?
            else {
                break;
            };
            current = parent;
        }
        Ok(current)
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
        ctx.charge_work(1, "index SLDPRT offset plane ordinals")?;
        let id = copy_projected_feature_id(ctx, &feature.id)?;
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
        ctx.insert_hash_map(
            &mut ordinals,
            id,
            value,
            "index SLDPRT offset plane ordinals",
        )?;
    }
    // A zero-distance offset with an explicit feature reference is a geometric
    // alias. Collapse only that provenance chain; independent coincident
    // planes remain distinct candidates and stay ambiguous.
    let mut zero_offset_parents = HashMap::new();
    for feature in features.iter() {
        ctx.charge_work(1, "index SLDPRT zero offset plane parents")?;
        let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature { feature: reference }),
            distance,
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        if same_scalar(distance.get(), 0.0) {
            let id = copy_projected_feature_id(ctx, &feature.id)?;
            let reference = copy_projected_feature_id(ctx, reference)?;
            ctx.insert_hash_map(
                &mut zero_offset_parents,
                id,
                reference,
                "index SLDPRT zero offset plane parents",
            )?;
        }
    }
    for feature in features.iter_mut() {
        ctx.charge_work(1, "validate SLDPRT offset plane references")?;
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
        let reference_id = copy_projected_feature_id(ctx, reference_id)?;
        let invalid = reference_id == feature.id
            || match ctx.get_hash_map(&(ordinals), &reference_id, "look up SLDPRT hash key")? {
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
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(feature.dependencies.as_slice().len()),
            "validate SLDPRT offset plane dependencies",
        )?;
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
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(feature.dependencies.as_slice().len()),
                "bind SLDPRT offset plane dependencies",
            )?;
            feature.dependencies.insert(
                ctx,
                reference_id,
                "bind SLDPRT offset plane dependencies",
            )?;
        }
    }
    let mut frames = HashMap::new();
    for feature in features.iter() {
        ctx.charge_work(1, "index SLDPRT offset plane frames")?;
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
        let id = copy_projected_feature_id(ctx, &feature.id)?;
        ctx.insert_hash_map(&mut frames, id, frame, "index SLDPRT offset plane frames")?;
    }

    loop {
        let mut changed = false;
        for feature in features.iter() {
            ctx.charge_work(1, "propagate SLDPRT offset plane frames")?;
            let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference: Some(DatumPlaneReference::Feature { feature: reference }),
                distance,
            }) = feature.evaluation.definition()
            else {
                continue;
            };
            if ctx.contains_key_hash_map(&(frames), &feature.id, "test SLDPRT map key")? {
                continue;
            }
            let Some(&(origin, normal, u_axis)) =
                ctx.get_hash_map(&(frames), reference, "look up SLDPRT hash key")?
            else {
                continue;
            };
            let normal_length = normal.norm();
            let id = copy_projected_feature_id(ctx, &feature.id)?;
            ctx.insert_hash_map(
                &mut frames,
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
                "propagate SLDPRT offset plane frames",
            )?;
            changed = true;
        }

        let mut bindings = Vec::new();
        for (index, feature) in features.iter().enumerate() {
            ctx.charge_work(1, "bind SLDPRT offset plane frames")?;
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
            let mut candidates_by_root = HashMap::new();
            for candidate in ctx.admit_iter(&*features, "find SLDPRT offset plane candidates")? {
                if !(candidate.ordinal < feature.ordinal
                    || (serialized_reference_frame.is_some()
                        && ctx
                            .get_hash_map(&ordinals, &candidate.id, "look up SLDPRT hash key")?
                            .is_some_and(|(_, _, is_principal, _)| *is_principal)))
                    || history_key(candidate) != Some(history)
                {
                    continue;
                }
                let Some(&(candidate_origin, candidate_normal, candidate_u_axis)) =
                    ctx.get_hash_map(&frames, &candidate.id, "look up SLDPRT hash key")?
                else {
                    continue;
                };
                if let Some(serialized_reference_frame) = serialized_reference_frame {
                    if !plane_frames_match(
                        serialized_reference_frame,
                        (candidate_origin, candidate_normal, candidate_u_axis),
                    ) {
                        continue;
                    }
                }
                let candidate_normal_length = candidate_normal.norm();
                let result_normal_length = normal.norm();
                let normal_dot = (normal.x * candidate_normal.x
                    + normal.y * candidate_normal.y
                    + normal.z * candidate_normal.z)
                    / (result_normal_length * candidate_normal_length);
                if !same_scalar(normal_dot.abs(), 1.0) {
                    continue;
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
                    displacement.x - candidate_normal.x * signed_distance / candidate_normal_length,
                    displacement.y - candidate_normal.y * signed_distance / candidate_normal_length,
                    displacement.z - candidate_normal.z * signed_distance / candidate_normal_length,
                );
                if !(same_scalar(tangent.norm(), 0.0)
                    && same_scalar(signed_distance.abs(), distance.get().abs()))
                {
                    continue;
                }
                let candidate = &candidate.id;
                let distance = distance.get().abs().copysign(signed_distance);
                let root = canonical_plane_id(ctx, candidate, &zero_offset_parents)?;
                if !ctx.contains_key_hash_map(&(candidates_by_root), root, "test SLDPRT map key")? {
                    ctx.insert_hash_map(
                        &mut candidates_by_root,
                        root,
                        distance,
                        "index SLDPRT offset plane candidate roots",
                    )?;
                }
            }
            let mut candidates = candidates_by_root.into_iter();
            let Some((reference, distance)) = candidates.next() else {
                continue;
            };
            if candidates.next().is_some() {
                continue;
            }
            let reference = copy_projected_feature_id(ctx, reference)?;
            ctx.reserve_vec(&mut bindings, 1, "collect SLDPRT offset plane bindings")?;
            bindings.push((index, (reference, distance)));
        }
        for (index, (reference, distance)) in bindings {
            let Some(distance) = Length::new(distance) else {
                continue;
            };
            let reference_copy = copy_projected_feature_id(ctx, &reference)?;
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
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(features[index].dependencies.as_slice().len()),
                "bind SLDPRT offset plane dependencies",
            )?;
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
        ctx.charge_work(1, "resolve SLDPRT offset plane support frames")?;
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
    for history in ctx.admit_iter(histories, "scan SLDPRT feature histories")? {
        for feature in ctx.admit_iter(
            &history.features,
            "index SLDPRT native construction sources",
        )? {
            if !matches!(
                classify(feature),
                Some(
                    FeatureClass::Sketch
                        | FeatureClass::SketchBlockInstance
                        | FeatureClass::EquationCurve
                        | FeatureClass::ProjectedCurve
                        | FeatureClass::CompositeCurve
                )
            ) || ctx.contains_hash_set(
                &(construction_native_refs),
                feature.id.as_str(),
                "test SLDPRT hashed identity",
            )? {
                continue;
            }
            ctx.insert_hash_set(
                &mut construction_native_refs,
                feature.id.as_str(),
                "index SLDPRT native construction sources",
            )?;
        }
    }
    let mut feature_ids_by_native = HashMap::new();
    for feature in features.iter() {
        ctx.charge_work(1, "index SLDPRT native construction features")?;
        let Some(native) = feature.native_ref.as_deref() else {
            continue;
        };
        if ctx.contains_hash_set(
            &(construction_native_refs),
            native,
            "test SLDPRT hashed identity",
        )? {
            let native = copy_projected_feature_text(ctx, native)?;
            let id = copy_projected_feature_id(ctx, &feature.id)?;
            ctx.insert_hash_map(
                &mut feature_ids_by_native,
                native,
                id,
                "index SLDPRT native construction features",
            )?;
        }
    }

    for feature in features {
        let feature_id = &feature.id;
        let dependencies = &mut feature.dependencies;
        let insert_dependency =
            |dependencies: &mut cadmpeg_ir::features::DistinctMembers<FeatureId>,
             target: &FeatureId|
             -> Result<(), CodecError> {
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(dependencies.as_slice().len()),
                    "bind SLDPRT native construction references",
                )?;
                if target != feature_id && !dependencies.contains(target) {
                    ctx.charge_work(
                        cadmpeg_core::decode::u64_from_index(dependencies.as_slice().len()),
                        "bind SLDPRT native construction references",
                    )?;
                    dependencies.insert(
                        ctx,
                        copy_projected_feature_id(ctx, target)?,
                        "bind SLDPRT native construction dependencies",
                    )?;
                }
                Ok(())
            };
        let bind_planar = |profile: &mut PlanarProfileRef,
                           dependencies: &mut cadmpeg_ir::features::DistinctMembers<FeatureId>|
         -> Result<(), CodecError> {
            ctx.charge_work(1, "bind SLDPRT native construction references")?;
            let PlanarProfileRef::Native(native) = profile else {
                return Ok(());
            };
            let Some(target) = ctx.get_hash_map(
                &(feature_ids_by_native),
                native.as_str(),
                "look up SLDPRT hash key",
            )?
            else {
                return Ok(());
            };
            let target_id = copy_projected_feature_id(ctx, target)?;
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
                        ctx.charge_work(1, "bind SLDPRT native construction references")?;
                        if let Some(target) = ctx.get_hash_map(
                            &(feature_ids_by_native),
                            native.as_str(),
                            "look up SLDPRT hash key",
                        )? {
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
    ctx: &DecodeContext<'_>,
    histories: &[FeatureHistory],
) -> Result<Vec<SourceAttribute>, CodecError> {
    const OPERATION: &str = "project SLDPRT custom properties";
    let mut attributes = Vec::new();
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(histories.len()),
        OPERATION,
    )?;
    for history in histories {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(history.features.len()),
            OPERATION,
        )?;
        for feature in &history.features {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(feature.xml_tag.len()),
                OPERATION,
            )?;
            if !is_custom_property(feature) {
                continue;
            }
            charge_projected_text_work(ctx, &feature.id, OPERATION)?;
            let native_id = feature
                .id
                .strip_prefix("sldprt:history:feature#")
                .unwrap_or(&feature.id);
            let id = ctx.format_retained(
                format_args!(
                    "sldprt:history:custom-property#{}",
                    EncodedNativeKey(native_id)
                ),
                OPERATION,
            )?;
            let id = AttributeId::mint(id)
                .map_err(|_| CodecError::malformed("invalid SLDPRT custom-property ID"))?;
            charge_projected_text_work(ctx, &feature.name, OPERATION)?;
            let mut name = String::new();
            ctx.try_reserve_retained_text(&mut name, feature.name.len(), OPERATION)?;
            name.push_str(&feature.name);
            let mut values = Vec::new();
            if let Some(text) = &feature.text {
                charge_projected_text_work(ctx, text, OPERATION)?;
                let mut value = String::new();
                ctx.try_reserve_retained_text(&mut value, text.len(), OPERATION)?;
                value.push_str(text);
                ctx.reserve_vec(&mut values, 1, OPERATION)?;
                values.push(AttributeValue::String(value));
            }
            ctx.reserve_vec(&mut attributes, 1, OPERATION)?;
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

fn unique_source_bindings<'a>(
    ctx: &DecodeContext<'_>,
    history: &'a FeatureHistory,
) -> Result<BTreeMap<FeatureSource, Option<SourceBinding<'a>>>, CodecError> {
    const OPERATION: &str = "index SLDPRT unique feature sources";
    let mut sources = BTreeMap::<FeatureSource, Option<&'a crate::records::Feature>>::new();
    for feature in ctx.admit_iter(&history.features, OPERATION)? {
        if is_history_metadata_record(ctx, feature, &history.features)? {
            continue;
        }
        let Some(source) = feature.source_id else {
            continue;
        };
        ctx.entry_btree_map(&mut sources, source, OPERATION)?
            .and_modify(|existing| *existing = None)
            .or_insert(Some(feature));
    }
    let mut bindings = BTreeMap::new();
    for (source, feature) in ctx.admit_iter(sources, OPERATION)? {
        let binding = feature
            .map(|feature| {
                Ok::<_, CodecError>(SourceBinding {
                    native: feature.id.as_str(),
                    neutral: neutral_feature_id_charged(ctx, &feature.id)?,
                })
            })
            .transpose()?;
        ctx.insert_btree_map(&mut bindings, source, binding, OPERATION)?;
    }
    Ok(bindings)
}

pub(crate) fn incomplete_history_reference_features(
    ctx: &DecodeContext<'_>,
    histories: &[FeatureHistory],
) -> Result<usize, CodecError> {
    let mut incomplete = 0usize;
    for history in ctx.admit_iter(
        histories,
        "scan SLDPRT incomplete_history_reference_features values",
    )? {
        let sources = unique_source_bindings(ctx, history)?;
        let mut native_ids = HashSet::new();
        for feature in &history.features {
            ctx.charge_work(1, "index SLDPRT history feature references")?;
            if ctx.contains_hash_set(
                &(native_ids),
                feature.id.as_str(),
                "test SLDPRT hashed identity",
            )? {
                continue;
            }
            ctx.insert_hash_set(
                &mut native_ids,
                feature.id.as_str(),
                "index SLDPRT history feature references",
            )?;
        }
        for feature in &history.features {
            ctx.charge_work(1, "scan SLDPRT incomplete history references")?;
            let owner_id = neutral_feature_id_charged(ctx, &feature.id)?;
            let duplicate_source = match feature.source_id {
                Some(source) => ctx
                    .get_btree_map(&sources, &source, "look up SLDPRT feature source")?
                    .is_some_and(Option::is_none),
                None => false,
            };
            let parent_requested = feature.tree_parent.is_some();
            let parent_resolved = match feature.tree_parent_record_id() {
                Some(parent) => {
                    ctx.contains_hash_set(&(native_ids), parent, "test SLDPRT hashed identity")?
                }
                None => false,
            } || match feature.parent_source_id() {
                Some(source) => ctx
                    .get_btree_map(&sources, &source, "look up SLDPRT feature source")?
                    .is_some_and(Option::is_some),
                None => false,
            };
            let incomplete_content = ctx
                .admit_iter(
                    &feature.content[..],
                    "scan SLDPRT incomplete_history_reference_features values",
                )?
                .try_fold(false, |found, item| {
                    Ok::<_, cadmpeg_core::CodecError>(
                        found
                            || (match item {
                                FeatureContent::Feature(child) => !ctx.contains_hash_set(
                                    &(native_ids),
                                    child.as_str(),
                                    "test SLDPRT hashed identity",
                                )?,
                                FeatureContent::Dimension(name) => !ctx.contains_key_btree_map(
                                    &(feature.parameters),
                                    name.as_str(),
                                    "test SLDPRT map key",
                                )?,
                                FeatureContent::Text(_) => false,
                            }),
                    )
                })?;
            let mut unresolved_dependency = false;
            for name in ctx.admit_iter(
                FEATURE_REFERENCE_PROPERTIES,
                "scan SLDPRT incomplete_history_reference_features values",
            )? {
                let Some(value) =
                    ctx.get_btree_map(&feature.properties, *name, "look up SLDPRT ordered key")?
                else {
                    continue;
                };
                for reference in value
                    .split(|character: char| {
                        character == ',' || character == ';' || character.is_whitespace()
                    })
                    .filter(|reference| !reference.is_empty())
                {
                    unresolved_dependency = unresolved_dependency
                        || (FeatureSource::try_from(reference)
                            .ok()
                            .map(|reference| {
                                Ok::<_, cadmpeg_core::CodecError>(ctx.get_btree_map(
                                    &sources,
                                    &reference,
                                    "look up SLDPRT feature source",
                                )?)
                            })
                            .transpose()?
                            .flatten()
                            .and_then(Option::as_ref)
                            .is_none_or(|binding| binding.neutral == owner_id));
                }
            }
            if duplicate_source
                || (parent_requested && !parent_resolved)
                || incomplete_content
                || unresolved_dependency
            {
                incomplete = incomplete.checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "count SLDPRT incomplete history references",
                        u64::MAX - 1,
                        u64::MAX,
                    )
                })?;
            }
        }
    }
    Ok(incomplete)
}

fn project_feature_content(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    by_native: &HashMap<&str, FeatureId>,
) -> Result<cadmpeg_ir::features::FeatureContent, CodecError> {
    const OPERATION: &str = "project SLDPRT feature content";
    if feature.text.is_some() {
        return Ok(cadmpeg_ir::features::FeatureContent::default());
    }
    let mut names = Vec::<&str>::new();
    for content in &feature.content {
        ctx.charge_work(1, OPERATION)?;
        if let FeatureContent::Dimension(name) = content {
            ctx.charge_work(cadmpeg_core::decode::u64_from_index(names.len()), OPERATION)?;
            if ctx.contains_key_btree_map(
                &(feature.parameters),
                name.as_str(),
                "test SLDPRT map key",
            )? && !names.contains(&name.as_str())
            {
                ctx.reserve_vec(&mut names, 1, OPERATION)?;
                names.push(name);
            }
        }
    }
    for name in ctx
        .admit_iter(
            &feature.parameters,
            "scan SLDPRT project_feature_content map keys",
        )?
        .map(|(key, _)| key)
    {
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(names.len()), OPERATION)?;
        if !names.contains(&name.as_str()) {
            ctx.reserve_vec(&mut names, 1, OPERATION)?;
            names.push(name.as_str());
        }
    }
    let mut result = cadmpeg_ir::features::FeatureContent::default();
    for content in &feature.content {
        ctx.charge_work(1, OPERATION)?;
        let value = match content {
            FeatureContent::Text(text) => {
                FeatureSourceContent::Text(copy_projected_feature_text(ctx, text)?)
            }
            FeatureContent::Dimension(name) => {
                ctx.charge_work(cadmpeg_core::decode::u64_from_index(names.len()), OPERATION)?;
                let Some(ordinal) = ctx
                    .admit_iter(&names[..], "scan SLDPRT project_feature_content values")?
                    .position(|known| *known == name)
                else {
                    continue;
                };
                charge_projected_text_work(ctx, &feature.id, OPERATION)?;
                let key = feature
                    .id
                    .strip_prefix("sldprt:history:feature#")
                    .unwrap_or(&feature.id);
                let id = ctx.format_retained(
                    format_args!("sldprt:model:parameter#{}:{ordinal}", EncodedNativeKey(key)),
                    OPERATION,
                )?;
                let parameter = ParameterId::mint(id).map_err(CodecError::malformed)?;
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(result.len()),
                    OPERATION,
                )?;
                if ctx.admit_iter(&result[..], "scan SLDPRT project_feature_content values")?.any(|value| matches!(value, FeatureSourceContent::Parameter(known) if known == &parameter)) {
                    continue;
                }
                FeatureSourceContent::Parameter(parameter)
            }
            FeatureContent::Feature(id) => {
                let Some(target) =
                    ctx.get_hash_map(&(by_native), id.as_str(), "look up SLDPRT hash key")?
                else {
                    continue;
                };
                FeatureSourceContent::Feature(copy_projected_feature_id(ctx, target)?)
            }
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(result.len()),
            OPERATION,
        )?;
        result.push(value, ctx, OPERATION)?;
    }
    Ok(result)
}

fn project_feature_dependencies(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    by_source: &HashMap<String, FeatureId>,
) -> Result<cadmpeg_ir::features::DistinctMembers<FeatureId>, CodecError> {
    let owner = neutral_feature_id_charged(ctx, &feature.id)?;
    let mut dependencies = cadmpeg_ir::features::DistinctMembers::default();
    for property in FEATURE_REFERENCE_PROPERTIES {
        let Some(value) = ctx.get_btree_map(
            &(feature.properties),
            *property,
            "look up SLDPRT ordered key",
        )?
        else {
            continue;
        };
        charge_projected_text_work(ctx, value, "scan SLDPRT feature dependencies")?;
        for reference in value
            .split(|character: char| {
                character == ',' || character == ';' || character.is_whitespace()
            })
            .filter(|reference| !reference.is_empty())
        {
            ctx.charge_work(1, "scan SLDPRT feature dependencies")?;
            let Some(dependency) =
                ctx.get_hash_map(&(by_source), reference, "look up SLDPRT hash key")?
            else {
                continue;
            };
            if dependency == &owner || dependencies.contains(dependency) {
                continue;
            }
            dependencies.insert(
                ctx,
                copy_projected_feature_id(ctx, dependency)?,
                "collect SLDPRT feature dependencies",
            )?;
        }
    }
    Ok(dependencies)
}

/// Project decoded native configurations under the caller's resource budget.
pub(crate) fn project_configurations_charged(
    ctx: &DecodeContext<'_>,
    histories: &[FeatureHistory],
) -> Result<Vec<DesignConfiguration>, CodecError> {
    const OPERATION: &str = "project SLDPRT configurations";
    let copy = |value: &str| -> Result<String, CodecError> {
        charge_projected_text_work(ctx, value, OPERATION)?;
        let mut copied = String::new();
        ctx.try_reserve_retained_text(&mut copied, value.len(), OPERATION)?;
        copied.push_str(value);
        Ok(copied)
    };
    let mut projected = Vec::new();
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(histories.len()),
        OPERATION,
    )?;
    for history in histories {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(history.configurations.len()),
            OPERATION,
        )?;
        for configuration in &history.configurations {
            charge_projected_text_work(ctx, &configuration.id, OPERATION)?;
            let native_id = configuration
                .id
                .strip_prefix("sldprt:history:configuration#")
                .unwrap_or(&configuration.id);
            let id = ctx.format_retained(
                format_args!("sldprt:model:configuration#{}", EncodedNativeKey(native_id)),
                OPERATION,
            )?;
            let id = ConfigurationId::mint(id)
                .map_err(|_| CodecError::malformed("invalid SLDPRT configuration ID"))?;
            let mut properties = BTreeMap::new();
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(configuration.properties.len()),
                OPERATION,
            )?;
            for (key, value) in &configuration.properties {
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(key.as_str().len()),
                    OPERATION,
                )?;
                let key = cadmpeg_core::text::NonBlankString::for_decode(
                    ctx,
                    copy(key.as_str())?,
                    "validate nonblank text",
                )?
                .ok_or_else(|| CodecError::malformed("blank SLDPRT configuration property"))?;
                let value = copy(value)?;
                ctx.insert_btree_map(&mut properties, key, value, OPERATION)?;
            }
            let material = configuration.material.as_deref().map(copy).transpose()?;
            let native_ref = copy(&configuration.id)?;
            let name = copy(&configuration.name)?;
            ctx.reserve_vec(&mut projected, 1, OPERATION)?;
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
    features_by_source: &BTreeMap<FeatureSource, &Feature>,
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
    if let Some(role) = feature_tree_node_role(ctx, feature, history_features)? {
        return Ok(FeatureDefinition::Operation(FeatureOperation::TreeNode {
            role,
            children: cadmpeg_ir::features::TreeChildren::default(),
        }));
    }
    let class = classify(feature);
    let projected_pattern = if class == Some(FeatureClass::Pattern) {
        Some(project_pattern(ctx, feature, by_source, native_by_source)?)
    } else {
        None
    };
    if class == Some(FeatureClass::CosmeticThread) {
        return project_cosmetic_thread(ctx, feature);
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
                    .map(|source| {
                        Ok::<_, cadmpeg_core::CodecError>(ctx.get_hash_map(
                            &(by_source),
                            source.as_str(),
                            "look up SLDPRT hash key",
                        )?)
                    })
                    .transpose()?
                    .flatten()
                    .map(|id| copy_projected_feature_id(ctx, id))
                    .transpose()?,
                placement: sketch_block_placement(feature),
            },
        ));
    }
    if class == Some(FeatureClass::ReferencePlane) && is_offset_plane(ctx, feature)? {
        return project_offset_plane(ctx, feature, by_source)?
            .map_or_else(|| native_definition(ctx, feature), Ok);
    }
    if let Some(plane) =
        principal_plane_in_history(ctx, feature, features_by_source, history_features)?
    {
        return Ok(FeatureDefinition::Operation(
            FeatureOperation::DatumPrincipalPlane { plane },
        ));
    }
    if class == Some(FeatureClass::ReferencePlane) {
        return project_datum_plane(feature).map_or_else(
            || {
                if feature.properties.contains_key("NativeRole") {
                    native_definition(ctx, feature)
                } else {
                    Ok(FeatureDefinition::Operation(FeatureOperation::Unresolved {
                        family: UnresolvedFamily::DatumPlane,
                    }))
                }
            },
            Ok,
        );
    }
    if class == Some(FeatureClass::ReferenceAxis) {
        return project_datum_axis(feature).map_or_else(|| native_definition(ctx, feature), Ok);
    }
    if class == Some(FeatureClass::ReferencePoint) {
        return project_datum_point(feature).map_or_else(|| native_definition(ctx, feature), Ok);
    }
    if class == Some(FeatureClass::CoordinateSystem) {
        return Ok(project_datum_coordinate_system(feature).unwrap_or(
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumCoordinateSystem,
            }),
        ));
    }
    if class == Some(FeatureClass::EquationCurve) {
        return project_equation_curve(ctx, feature)?
            .map_or_else(|| native_definition(ctx, feature), Ok);
    }
    if class == Some(FeatureClass::ProjectedCurve) {
        return project_projected_curve(ctx, feature, native_by_source)?
            .map_or_else(|| native_definition(ctx, feature), Ok);
    }
    if class == Some(FeatureClass::CompositeCurve) {
        return project_composite_curve(ctx, feature, native_by_source)?
            .map_or_else(|| native_definition(ctx, feature), Ok);
    }
    if class == Some(FeatureClass::Helix) {
        return Ok(match project_helix(feature) {
            Some(definition) => definition,
            None => project_native_axis_helix(ctx, feature)?
                .map_or_else(|| native_definition(ctx, feature), Ok)?,
        });
    }
    if class == Some(FeatureClass::Wrap) {
        return project_wrap(ctx, feature, native_by_source)?
            .map_or_else(|| native_definition(ctx, feature), Ok);
    }
    Ok(if class == Some(FeatureClass::Extrude) {
        project_extrude(ctx, feature, native_by_source, features_by_source)?
            .map_or_else(|| native_definition(ctx, feature), Ok)?
    } else if class == Some(FeatureClass::Fillet) {
        project_fillet(ctx, feature)?
    } else if class == Some(FeatureClass::Chamfer) {
        project_chamfer(ctx, feature)?
    } else if class == Some(FeatureClass::Shell) {
        project_shell(ctx, feature)?
    } else if class == Some(FeatureClass::Thicken) {
        project_thicken(ctx, feature)?
    } else if class == Some(FeatureClass::OffsetSurface) {
        project_offset_surface(ctx, feature)?
    } else if class == Some(FeatureClass::KnitSurface) {
        project_knit_surface(ctx, feature)?
    } else if class == Some(FeatureClass::FilledSurface) {
        project_filled_surface(ctx, feature)?
    } else if class == Some(FeatureClass::TrimSurface) {
        project_trim_surface(ctx, feature, native_by_source)?
    } else if class == Some(FeatureClass::ExtendSurface) {
        project_extend_surface(ctx, feature)?
    } else if class == Some(FeatureClass::RuledSurface) {
        project_ruled_surface(ctx, feature)?.map_or_else(|| native_definition(ctx, feature), Ok)?
    } else if class == Some(FeatureClass::Draft) {
        project_draft(ctx, feature)?
    } else if class == Some(FeatureClass::SplitFace) {
        project_split_face(ctx, feature)?.map_or_else(|| native_definition(ctx, feature), Ok)?
    } else if class == Some(FeatureClass::Combine) {
        project_combine(ctx, feature)?.map_or_else(|| native_definition(ctx, feature), Ok)?
    } else if class == Some(FeatureClass::CutWithSurface) {
        project_cut_with_surface(ctx, feature)?
    } else if class == Some(FeatureClass::DeleteBody) {
        project_delete_body(ctx, feature)?.map_or_else(|| native_definition(ctx, feature), Ok)?
    } else if class == Some(FeatureClass::DeleteFace) {
        project_delete_face(ctx, feature)?.map_or_else(|| native_definition(ctx, feature), Ok)?
    } else if class == Some(FeatureClass::ReplaceFace) {
        project_replace_face(ctx, feature)?.map_or_else(|| native_definition(ctx, feature), Ok)?
    } else if class == Some(FeatureClass::MoveFace) {
        project_move_face(ctx, feature)?.map_or_else(|| native_definition(ctx, feature), Ok)?
    } else if class == Some(FeatureClass::MoveBody) {
        project_move_body(ctx, feature)?.map_or_else(|| native_definition(ctx, feature), Ok)?
    } else if class == Some(FeatureClass::Dome) {
        project_dome(ctx, feature)?
    } else if class == Some(FeatureClass::Flex) {
        project_flex(feature)
    } else if class == Some(FeatureClass::Scale) {
        project_scale(ctx, feature)?
    } else if class == Some(FeatureClass::Hole) {
        project_hole(ctx, feature, features_by_source, history_features)?
            .map_or_else(|| native_definition(ctx, feature), Ok)?
    } else if class == Some(FeatureClass::Revolve) {
        project_revolve(ctx, feature, native_by_source)?
    } else if class == Some(FeatureClass::Pattern) {
        projected_pattern.map_or_else(|| native_definition(ctx, feature), Ok)?
    } else if class == Some(FeatureClass::Sweep) {
        project_sweep(ctx, feature, native_by_source)?
            .map_or_else(|| native_definition(ctx, feature), Ok)?
    } else if class == Some(FeatureClass::Loft) {
        project_loft(ctx, feature, native_by_source)?
            .map_or_else(|| native_definition(ctx, feature), Ok)?
    } else if class == Some(FeatureClass::Rib) {
        project_rib(ctx, feature, native_by_source)?
    } else {
        native_definition(ctx, feature)?
    })
}

fn parameter_names(ctx: &DecodeContext<'_>, feature: &Feature) -> Result<Vec<String>, CodecError> {
    const OPERATION: &str = "collect SLDPRT parameter names";
    let mut names = Vec::new();
    for content in &feature.content {
        ctx.charge_work(1, OPERATION)?;
        if let FeatureContent::Dimension(name) = content {
            if ctx.contains_key_btree_map(
                &(feature.parameters),
                name.as_str(),
                "test SLDPRT map key",
            )? {
                let name = copy_projected_feature_text(ctx, name)?;
                ctx.reserve_vec(&mut names, 1, OPERATION)?;
                names.push(name);
            }
        }
    }
    for name in ctx
        .admit_iter(&feature.parameters, "scan SLDPRT parameter_names map keys")?
        .map(|(key, _)| key)
    {
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(names.len()), OPERATION)?;
        if !ctx
            .admit_iter(&names[..], "scan SLDPRT parameter_names values")?
            .any(|known| known == name.as_str())
        {
            let name = copy_projected_feature_text(ctx, name.as_str())?;
            ctx.reserve_vec(&mut names, 1, OPERATION)?;
            names.push(name);
        }
    }
    Ok(names)
}

pub(super) fn projected_parameter_names(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Vec<String>, CodecError> {
    const OPERATION: &str = "deduplicate SLDPRT projected parameter names";
    let mut seen = HashSet::new();
    let mut projected = Vec::new();
    for name in parameter_names(ctx, feature)? {
        ctx.charge_work(1, OPERATION)?;
        if ctx.contains_hash_set(&(seen), &name, "test SLDPRT hashed identity")? {
            continue;
        }
        let key = copy_projected_feature_text(ctx, &name)?;
        ctx.insert_hash_set(&mut seen, key, OPERATION)?;
        ctx.reserve_vec(&mut projected, 1, OPERATION)?;
        projected.push(name);
    }
    Ok(projected)
}

pub(super) fn neutral_parameter_id(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    ordinal: usize,
) -> Result<ParameterId, CodecError> {
    charge_projected_text_work(ctx, &feature.id, "retain SLDPRT projected parameter ID")?;
    let key = feature
        .id
        .strip_prefix("sldprt:history:feature#")
        .unwrap_or(&feature.id);
    let id = ctx.format_retained(
        format_args!("sldprt:model:parameter#{}:{ordinal}", EncodedNativeKey(key)),
        "retain SLDPRT projected parameter ID",
    )?;
    ParameterId::mint(id).map_err(CodecError::malformed)
}

fn native_definition(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    charge_projected_text_work(ctx, &feature.kind, "retain SLDPRT native definition kind")?;
    Ok(FeatureDefinition::Operation(FeatureOperation::Native {
        kind: ctx
            .format_retained(
                format_args!("{}", feature.kind),
                "retain SLDPRT native definition kind",
            )?
            .into(),
        parameters: copy_projected_feature_properties(
            ctx,
            &feature.parameters,
            "collect SLDPRT native definition parameters",
        )?,
    }))
}
