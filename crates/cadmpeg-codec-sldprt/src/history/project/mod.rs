// SPDX-License-Identifier: Apache-2.0
//! Project native Keywords records into the neutral feature arena.

use crate::classification::{classify, FeatureClass};
use crate::records::{Feature, FeatureContent, FeatureHistory};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::AttributeId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::{
    features::{
        ConfigurationId, DatumPlaneReference, DesignConfiguration, FeatureDefinition, FeatureId,
        FeatureOperation, FeatureSourceContent, FinitePoint3, ParameterId, PathRef,
        PlanarProfileRef, ProfileRef, SplitFaceTool, UnresolvedFamily,
    },
    scalar::Length,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::{self, Write};

use crate::history::classify::{
    is_custom_property, is_offset_plane, is_semantic_note, HistoryIndex,
};
use crate::history::literals::{named_literal, parse_point3_mm, parse_vector3, valid_plane_frame};
use crate::records::FeatureSource;

/// The value of `$read`, or `Ok(None)` from the enclosing function when it is absent.
macro_rules! require {
    ($read:expr) => {
        match $read {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

#[cfg(test)]
mod budget_tests;
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
use self::sketch::{block_placement, project_cosmetic_thread, project_split_face};
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

/// The record identity prefix the neutral identities replace.
const HISTORY_FEATURE_PREFIX: &str = "sldprt:history:feature#";

/// Neutral feature identities of one history by native source key or record identity.
pub(super) type NeutralByKey<'k, 'n> = HashMap<&'k str, &'n FeatureId>;

pub(crate) struct FeatureProjection {
    pub(super) features: Vec<cadmpeg_ir::features::Feature>,
    regeneration_parents: Vec<(FeatureId, FeatureId)>,
}

const FEATURE_LITERAL: &str = "read SLDPRT feature literal";

/// A named parameter of `feature`, admitted for one literal reading.
pub(super) fn parameter_literal<'f>(
    ctx: &DecodeContext<'_>,
    feature: &'f Feature,
    name: &str,
) -> Result<Option<&'f str>, CodecError> {
    named_literal(ctx, &feature.parameters, name, FEATURE_LITERAL)
}

/// The first of two named parameters present, admitted for one literal reading.
pub(super) fn either_parameter<'f>(
    ctx: &DecodeContext<'_>,
    feature: &'f Feature,
    name: &str,
    fallback: &str,
) -> Result<Option<&'f str>, CodecError> {
    match parameter_literal(ctx, feature, name)? {
        Some(value) => Ok(Some(value)),
        None => parameter_literal(ctx, feature, fallback),
    }
}

/// A named property of `feature`, admitted for one literal reading.
pub(super) fn property_literal<'f>(
    ctx: &DecodeContext<'_>,
    feature: &'f Feature,
    name: &str,
) -> Result<Option<&'f str>, CodecError> {
    named_literal(ctx, &feature.properties, name, FEATURE_LITERAL)
}

/// A named property of `feature` that the caller compares with literal tokens
/// or copies; only the lookup is charged.
pub(super) fn property_value<'f>(
    ctx: &DecodeContext<'_>,
    feature: &'f Feature,
    name: &str,
) -> Result<Option<&'f str>, CodecError> {
    Ok(ctx
        .get_btree_map(&feature.properties, name, FEATURE_LITERAL)?
        .map(String::as_str))
}

/// A copy of a named property of `feature`, such as a native selection.
pub(super) fn property_text(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    name: &str,
) -> Result<Option<String>, CodecError> {
    property_value(ctx, feature, name)?
        .map(|value| ctx.copy_retained_text(value, "retain SLDPRT feature property"))
        .transpose()
}

/// Mints a typed identity from formatted text after admitting the grammar scan of it.
fn mint_identity<T: TryFrom<String, Error = cadmpeg_ir::ids::IdentityError>>(
    ctx: &DecodeContext<'_>,
    text: String,
    operation: &'static str,
) -> Result<T, CodecError> {
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(text.len()), operation)?;
    T::try_from(text).map_err(CodecError::malformed)
}

/// The native key of a record identity, without its history-record prefix.
fn native_key(id: &str) -> &str {
    id.strip_prefix(HISTORY_FEATURE_PREFIX).unwrap_or(id)
}

pub(super) fn copy_projected_feature_text(
    ctx: &DecodeContext<'_>,
    text: &str,
) -> Result<String, CodecError> {
    ctx.copy_retained_text(text, "copy SLDPRT projected feature text")
}

pub(super) fn neutral_feature_id_charged(
    ctx: &DecodeContext<'_>,
    native_id: &str,
) -> Result<FeatureId, CodecError> {
    const OPERATION: &str = "retain SLDPRT projected feature ID";
    let id = ctx.format_retained(
        format_args!(
            "sldprt:model:feature#{}",
            EncodedNativeKey(native_key(native_id))
        ),
        OPERATION,
    )?;
    mint_identity(ctx, id, OPERATION)
}

pub(super) fn copy_projected_feature_id(
    ctx: &DecodeContext<'_>,
    id: &FeatureId,
) -> Result<FeatureId, CodecError> {
    id.try_clone_for_decode(ctx, "copy SLDPRT projected feature ID")
}

/// The text a native property or parameter names a reserved or numbered source by.
fn source_lookup_key(ctx: &DecodeContext<'_>, source: FeatureSource) -> Result<String, CodecError> {
    match source {
        FeatureSource::Reserved => {
            ctx.format_retained(format_args!("-1"), "format SLDPRT source lookup key")
        }
        FeatureSource::Id(id) => ctx.format_retained(
            format_args!("{}", id.value()),
            "format SLDPRT source lookup key",
        ),
    }
}

pub(super) fn copy_projected_feature_properties(
    ctx: &DecodeContext<'_>,
    properties: &BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    operation: &'static str,
) -> Result<BTreeMap<cadmpeg_core::text::NonBlankString, String>, CodecError> {
    let mut copied = BTreeMap::new();
    for (key, value) in ctx.admit_iter(properties, operation)? {
        let key = key.try_clone_for_decode(ctx, operation)?;
        let value = ctx.copy_retained_text(value, operation)?;
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
        const OPERATION: &str = "retain SLDPRT regeneration edge loss";
        model.features = self.features;
        let mut refused_storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut refused = Vec::new();
        for (child, parent) in ctx.admit_iter(self.regeneration_parents, OPERATION)? {
            let error = match model.set_feature_regeneration_parent(ctx, &child, &parent) {
                Ok(()) => continue,
                Err(CodecError::Malformed(error)) => error,
                Err(error) => return Err(error),
            };
            ctx.push_scoped_vec(
                &mut refused_storage,
                &mut refused,
                (child, parent, error),
                OPERATION,
            )?;
        }
        if refused.is_empty() {
            return Ok(());
        }
        let mut ordinals_storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut ordinals = HashMap::new();
        for feature in ctx.admit_iter(&model.features, OPERATION)? {
            ordinals_storage.with_storage(|| {
                ctx.entry_hash_map(&mut ordinals, feature.id.as_str(), OPERATION)
                    .map(|entry| {
                        entry.or_insert(feature.ordinal);
                    })
            })?;
        }
        for (child, parent, error) in ctx.admit_iter(&refused, OPERATION)? {
            let child_ordinal = ctx
                .get_hash_map(&ordinals, child.as_str(), OPERATION)?
                .copied();
            let parent_ordinal = ctx
                .get_hash_map(&ordinals, parent.as_str(), OPERATION)?
                .copied();
            let message = ctx.format_retained(
                format_args!(
                    "regeneration edge from child `{child}` (ordinal {}) to parent \
                     `{parent}` (ordinal {}) was not installed: {error}",
                    FeatureOrdinal(child_ordinal),
                    FeatureOrdinal(parent_ordinal),
                ),
                OPERATION,
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
    const OPERATION: &str = "project SLDPRT feature histories";
    let mut features = Vec::new();
    let mut parents_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut parents = Vec::new();
    for history in ctx.admit_iter(histories, OPERATION)? {
        project_history(
            ctx,
            history,
            &mut features,
            &mut parents_storage,
            &mut parents,
        )?;
    }
    let regeneration_parents = link_tree_children(ctx, &mut features, parents)?;
    drop(parents_storage);
    bind_offset_plane_references(ctx, &mut features)?;
    bind_native_construction_features(ctx, &mut features, histories)?;
    Ok(FeatureProjection {
        features,
        regeneration_parents,
    })
}

/// Project one history's non-metadata records, appending each projected
/// feature and the parent its record names.
fn project_history<'c>(
    ctx: &'c DecodeContext<'_>,
    history: &FeatureHistory,
    features: &mut Vec<cadmpeg_ir::features::Feature>,
    parents_storage: &mut ScopedReservation<'c>,
    parents: &mut Vec<Option<FeatureId>>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "index SLDPRT projected source features";
    let index = HistoryIndex::new(ctx, &history.features)?;
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    let mut neutral_ids = Vec::new();
    let mut metadata = Vec::new();
    for feature in ctx.admit_iter(&history.features, OPERATION)? {
        let id = scratch.with_storage(|| neutral_feature_id_charged(ctx, &feature.id))?;
        ctx.push_scoped_vec(&mut scratch, &mut neutral_ids, id, OPERATION)?;
        let is_metadata = index.is_metadata(ctx, feature)?;
        ctx.push_scoped_vec(&mut scratch, &mut metadata, is_metadata, OPERATION)?;
    }
    let source_bindings =
        scratch.with_storage(|| unique_source_bindings(ctx, history, &metadata))?;
    let mut source_keys = Vec::new();
    for (source, binding) in ctx.admit_iter(&source_bindings, OPERATION)? {
        let Some(position) = *binding else {
            continue;
        };
        scratch.with_storage(|| {
            let key = source_lookup_key(ctx, *source)?;
            ctx.push_vec(&mut source_keys, (key, position), OPERATION)
        })?;
    }
    let mut native_by_source = HashMap::new();
    let mut by_source: NeutralByKey<'_, '_> = HashMap::new();
    for (key, position) in ctx.admit_iter(&source_keys, OPERATION)? {
        scratch.with_storage(|| {
            ctx.insert_hash_map(
                &mut native_by_source,
                key.as_str(),
                history.features[*position].id.as_str(),
                OPERATION,
            )?;
            ctx.insert_hash_map(
                &mut by_source,
                key.as_str(),
                &neutral_ids[*position],
                OPERATION,
            )
        })?;
    }
    let mut by_native = HashMap::new();
    let mut source_features = self::solid::SourceFeatures::new(ctx, &history.features)?;
    for ((feature, neutral), is_metadata) in ctx
        .admit_iter(&history.features, OPERATION)?
        .zip(&neutral_ids)
        .zip(&metadata)
    {
        scratch.with_storage(|| {
            ctx.insert_hash_map(&mut by_source, feature.id.as_str(), neutral, OPERATION)?;
            if !*is_metadata {
                ctx.insert_hash_map(&mut by_native, feature.id.as_str(), neutral, OPERATION)?;
            }
            Ok::<_, CodecError>(())
        })?;
    }
    let (records, _records_storage) = ctx.unique_index(
        history
            .features
            .iter()
            .map(|feature| (feature.id.as_str(), feature)),
        "index SLDPRT history records by identity",
    )?;
    let source_ordered = ctx.any_by(
        &history.features,
        |feature| {
            Ok(feature.input_class.is_none()
                && feature.xml_tag.eq_ignore_ascii_case("Extrusion")
                && feature.parameters.len() == 1
                && feature.source_value().is_some_and(|source| source > 0))
        },
        OPERATION,
    )?;
    for ((feature, neutral), is_metadata) in ctx
        .admit_iter(&history.features, OPERATION)?
        .zip(&neutral_ids)
        .zip(&metadata)
    {
        if *is_metadata {
            continue;
        }
        let record_parent = match feature.tree_parent_record_id() {
            Some(parent) => ctx.get_hash_map(&by_native, parent, OPERATION)?.copied(),
            None => None,
        };
        let parent = match (record_parent, feature.parent_source_id()) {
            (Some(parent), _) => Some(parent),
            (None, Some(source)) => {
                let (key, _key_storage) =
                    ctx.with_scoped_storage(OPERATION, || source_lookup_key(ctx, source))?;
                ctx.get_hash_map(&by_source, key.as_str(), OPERATION)?
                    .copied()
            }
            (None, None) => None,
        }
        .map(|parent| copy_projected_feature_id(ctx, parent))
        .transpose()?;
        let projected = cadmpeg_ir::features::Feature {
            id: copy_projected_feature_id(ctx, neutral)?,
            ordinal: source_ordered
                .then(|| feature.source_value().map(u64::from))
                .flatten()
                .filter(|source| *source > 0)
                .unwrap_or(u64::from(feature.ordinal)),
            name: (!feature.name.is_empty())
                .then(|| copy_projected_feature_text(ctx, &feature.name))
                .transpose()?,
            suppressed: Some(feature.suppressed),
            dependencies: project_feature_dependencies(ctx, feature, neutral, &by_source)?,
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
                    &mut source_features,
                    &records,
                    &index,
                )?,
            ),
            native_ref: Some(copy_projected_feature_text(ctx, &feature.id)?),
        };
        ctx.push_vec(features, projected, "collect SLDPRT projected features")?;
        parents_storage.with_storage(|| {
            ctx.push_vec(parents, parent, "collect SLDPRT projected feature parents")
        })?;
    }
    Ok(())
}

/// Add each child to the tree node its record names. A parent that is not a
/// tree node becomes a regeneration edge.
fn link_tree_children(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    parents: Vec<Option<FeatureId>>,
) -> Result<Vec<(FeatureId, FeatureId)>, CodecError> {
    const OPERATION: &str = "link SLDPRT feature tree children";
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    let mut tree_nodes = HashMap::new();
    for (index, feature) in ctx.admit_iter(&*features, OPERATION)?.enumerate() {
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::TreeNode { .. })
        ) {
            continue;
        }
        scratch.with_storage(|| {
            ctx.entry_hash_map(&mut tree_nodes, feature.id.as_str(), OPERATION)
                .map(|entry| {
                    entry.or_insert(index);
                })
        })?;
    }
    let mut regeneration_parents = Vec::new();
    let mut tree_children = Vec::new();
    for (child_index, parent) in ctx.admit_iter(parents, OPERATION)?.enumerate() {
        let Some(parent) = parent else {
            continue;
        };
        let child = copy_projected_feature_id(ctx, &features[child_index].id)?;
        match ctx.get_hash_map(&tree_nodes, parent.as_str(), OPERATION)? {
            Some(&tree_index) => scratch.with_storage(|| {
                ctx.push_vec(&mut tree_children, (tree_index, child), OPERATION)
            })?,
            None => ctx.push_vec(
                &mut regeneration_parents,
                (child, parent),
                "collect SLDPRT regeneration parents",
            )?,
        }
    }
    drop(tree_nodes);
    for (tree_index, child) in ctx.admit_iter(tree_children, OPERATION)? {
        let mut result = Ok(());
        features[tree_index].evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::TreeNode { children, .. }) =
                definition
            {
                result = children.insert(ctx, child, "collect SLDPRT tree children");
            }
        });
        result?;
    }
    Ok(regeneration_parents)
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
    for history in ctx.admit_iter(histories, OPERATION)? {
        for feature in ctx.admit_iter(&history.features, OPERATION)? {
            if !is_semantic_note(feature) {
                continue;
            }
            let id = ctx.format_retained(
                format_args!(
                    "sldprt:semantic-annotation:note#{}",
                    EncodedNativeKey(native_key(&feature.id))
                ),
                OPERATION,
            )?;
            ctx.charge_work(cadmpeg_core::decode::u64_from_index(id.len()), OPERATION)?;
            let id = cadmpeg_ir::semantic_annotations::SemanticAnnotationId::mint(id)
                .map_err(|_| CodecError::malformed("invalid SLDPRT semantic note ID"))?;
            let mut text = Vec::new();
            if let Some(value) = &feature.text {
                let value = ctx.copy_retained_text(value, OPERATION)?;
                ctx.push_vec(&mut text, value, OPERATION)?;
            }
            let note = cadmpeg_ir::semantic_annotations::SemanticAnnotation {
                id,
                object: ctx.copy_retained_text(&feature.id, OPERATION)?,
                kind: cadmpeg_ir::semantic_annotations::SemanticAnnotationKind::Text,
                runtime_type: ctx.copy_retained_text(&feature.kind, OPERATION)?,
                order: feature.ordinal,
                text,
                references: BTreeMap::new(),
                value: None,
                format: None,
                position: None,
                parameters: BTreeMap::new(),
                assets: Vec::new(),
                native_ref: ctx.copy_retained_text(&feature.id, OPERATION)?,
            };
            ctx.push_vec(&mut notes, note, OPERATION)?;
        }
    }
    Ok(notes)
}

/// A plane frame: origin, normal and in-plane axis.
type PlaneFrame = (Point3, Vector3, Vector3);

/// A principal or explicit datum plane usable as a base frame.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BasePlaneKind {
    Principal,
    Datum,
}

/// What one projected feature contributes to offset-plane binding, read once.
#[derive(Clone, Copy, Default)]
struct PlaneFacts {
    ordinal: u64,
    /// The frame a reference to this feature is checked against: the principal
    /// or datum plane frame, or an offset plane's stored frame.
    frame: Option<PlaneFrame>,
    base: Option<BasePlaneKind>,
    /// An offset plane's stored result frame.
    stored: Option<PlaneFrame>,
    /// An offset plane's serialized reference-face frame.
    serialized: Option<(FinitePoint3, Vector3, Vector3)>,
    /// An offset plane names its reference plane natively.
    explicit_native_reference: bool,
    /// An offset plane stores a reference-face frame property.
    has_face_fallback: bool,
    /// The history owning the native record.
    history: Option<usize>,
}

const FRAME_TOLERANCE: f64 = 1.0e-8;

fn same_scalar(left: f64, right: f64) -> bool {
    (left - right).abs() <= FRAME_TOLERANCE * left.abs().max(right.abs()).max(1.0)
}

fn principal_frame(plane: cadmpeg_ir::features::PrincipalPlane) -> PlaneFrame {
    match plane {
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
    }
}

fn plane_frames_match(left: PlaneFrame, right: PlaneFrame) -> bool {
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
}

fn plane_normal_matches(left: PlaneFrame, right: PlaneFrame) -> bool {
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
}

/// The signed distance from `reference` to `result` along the reference
/// normal, when `result` is parallel to `reference` and offset purely along it.
fn parallel_offset(reference: PlaneFrame, result: PlaneFrame) -> Option<f64> {
    let reference_normal_length = reference.1.norm();
    let result_normal_length = result.1.norm();
    let normal_dot =
        (reference.1.x * result.1.x + reference.1.y * result.1.y + reference.1.z * result.1.z)
            / (reference_normal_length * result_normal_length);
    if !same_scalar(normal_dot.abs(), 1.0) {
        return None;
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
    same_scalar(tangent.norm(), 0.0).then_some(signed_distance)
}

fn offset_frame_matches(reference: PlaneFrame, result: PlaneFrame, distance: Length) -> bool {
    parallel_offset(reference, result)
        .is_some_and(|signed| same_scalar(signed.abs(), distance.get().abs()))
}

/// Plane facts and reference positions aligned with feature order.
struct PlaneIndex {
    facts: Vec<PlaneFacts>,
    references: Vec<Option<usize>>,
}

fn plane_facts(
    ctx: &DecodeContext<'_>,
    scratch: &mut ScopedReservation<'_>,
    features: &[cadmpeg_ir::features::Feature],
) -> Result<PlaneIndex, CodecError> {
    const OPERATION: &str = "index SLDPRT offset plane ordinals";
    let read_frame = |properties, origin, normal, u_axis| -> Result<_, CodecError> {
        let origin = named_literal(ctx, properties, origin, OPERATION)?.and_then(parse_point3_mm);
        let normal = named_literal(ctx, properties, normal, OPERATION)?.and_then(parse_vector3);
        let u_axis = named_literal(ctx, properties, u_axis, OPERATION)?.and_then(parse_vector3);
        Ok(origin
            .zip(normal)
            .zip(u_axis)
            .map(|((origin, normal), u_axis)| (origin, normal, u_axis)))
    };
    let mut index_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut positions = HashMap::new();
    let mut histories = HashMap::new();
    let mut facts = Vec::new();
    for (position, feature) in ctx.admit_iter(features, OPERATION)?.enumerate() {
        let history = match feature.native_ref.as_deref() {
            Some(native) => match ctx.rsplit_once(native, ":", OPERATION)? {
                Some((history, _)) => {
                    let next = histories.len();
                    Some(index_storage.with_storage(|| {
                        ctx.entry_hash_map(&mut histories, history, OPERATION)
                            .map(|entry| *entry.or_insert(next))
                    })?)
                }
                None => None,
            },
            None => None,
        };
        index_storage.with_storage(|| {
            ctx.insert_hash_map(&mut positions, feature.id.as_str(), position, OPERATION)
        })?;
        let mut fact = PlaneFacts {
            ordinal: feature.ordinal,
            history,
            ..PlaneFacts::default()
        };
        match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane { plane }) => {
                fact.frame = Some(principal_frame(*plane));
                fact.base = Some(BasePlaneKind::Principal);
            }
            FeatureDefinition::Operation(FeatureOperation::DatumPlane { frame }) => {
                fact.frame = Some((
                    frame.origin().get(),
                    frame.normal().get(),
                    frame.u_axis().get(),
                ));
                fact.base = Some(BasePlaneKind::Datum);
            }
            FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane { .. }) => {
                let properties = &feature.source_properties;
                fact.stored = read_frame(properties, "Origin", "Normal", "UAxis")?
                    .map(|(origin, normal, u_axis)| (origin.get(), normal, u_axis))
                    .filter(|(_, normal, u_axis)| valid_plane_frame(*normal, *u_axis));
                fact.frame = fact.stored;
                fact.serialized = read_frame(
                    properties,
                    "ReferenceFaceOrigin",
                    "ReferenceFaceNormal",
                    "ReferenceFaceUAxis",
                )?;
                let has = |key: &str| ctx.contains_key_btree_map(properties, key, OPERATION);
                fact.explicit_native_reference = has("Reference")? || has("Plane")?;
                fact.has_face_fallback = has("ReferenceFaceOrigin")?
                    || has("ReferenceFaceNormal")?
                    || has("ReferenceFaceUAxis")?;
            }
            _ => {}
        }
        ctx.push_scoped_vec(scratch, &mut facts, fact, OPERATION)?;
    }
    let mut references = Vec::new();
    for feature in ctx.admit_iter(features, OPERATION)? {
        let reference = match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference: Some(DatumPlaneReference::Feature { feature: reference }),
                ..
            }) => ctx
                .get_hash_map(&positions, reference.as_str(), OPERATION)?
                .copied(),
            _ => None,
        };
        ctx.push_scoped_vec(scratch, &mut references, reference, OPERATION)?;
    }
    Ok(PlaneIndex { facts, references })
}

/// The end of each feature's chain of zero-distance offset references: the
/// first feature its chain reaches twice, or the first feature without such a
/// reference.
fn zero_offset_roots(
    ctx: &DecodeContext<'_>,
    scratch: &mut ScopedReservation<'_>,
    parents: &[Option<usize>],
) -> Result<Vec<usize>, CodecError> {
    const OPERATION: &str = "resolve SLDPRT zero offset plane chains";
    let (mut roots, mut path_index) = scratch.with_storage(|| {
        Ok::<_, CodecError>((
            ctx.alloc_filled(parents.len(), None, OPERATION)?,
            ctx.alloc_filled(parents.len(), None, OPERATION)?,
        ))
    })?;
    let mut path = Vec::new();
    for start in ctx.admit_iter(0..parents.len(), OPERATION)? {
        if roots[start].is_some() {
            continue;
        }
        let mut current = start;
        let root = loop {
            ctx.charge_work(1, OPERATION)?;
            if let Some(root) = roots[current] {
                break root;
            }
            if let Some(at) = path_index[current] {
                // Each feature on the cycle reaches itself first.
                for &node in ctx.admit_iter(&path[at..], OPERATION)? {
                    roots[node] = Some(node);
                    path_index[node] = None;
                }
                ctx.truncate_vec(&mut path, at, OPERATION)?;
                break current;
            }
            path_index[current] = Some(path.len());
            ctx.push_scoped_vec(scratch, &mut path, current, OPERATION)?;
            match parents[current] {
                Some(parent) => current = parent,
                None => break current,
            }
        };
        for &node in ctx.admit_iter(&path, OPERATION)? {
            roots[node] = Some(root);
            path_index[node] = None;
        }
        ctx.clear_vec(&mut path, OPERATION)?;
    }
    let mut resolved = Vec::new();
    for (node, root) in ctx.admit_iter(roots, OPERATION)?.enumerate() {
        ctx.push_scoped_vec(scratch, &mut resolved, root.unwrap_or(node), OPERATION)?;
    }
    Ok(resolved)
}

/// Remove `reference` from `dependencies` after a charged search for it.
fn remove_dependency(
    ctx: &DecodeContext<'_>,
    dependencies: &mut cadmpeg_ir::features::DistinctMembers<FeatureId>,
    reference: &FeatureId,
    operation: &'static str,
) -> Result<(), CodecError> {
    let Some(found) = ctx.position_by(
        dependencies.as_slice(),
        |dependency| ctx.equal(dependency, reference, operation),
        operation,
    )?
    else {
        return Ok(());
    };
    let mut index = 0;
    dependencies.retain(|_| {
        let keep = index != found;
        index += 1;
        keep
    });
    Ok(())
}

pub(super) fn bind_offset_plane_references(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
) -> Result<(), CodecError> {
    const OPERATION: &str = "bind SLDPRT offset plane references";
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    let PlaneIndex {
        facts,
        mut references,
    } = plane_facts(ctx, &mut scratch, features)?;
    // A zero-distance offset with an explicit feature reference is a geometric
    // alias. Collapse only that provenance chain; independent coincident
    // planes remain distinct candidates and stay ambiguous.
    let mut zero_offset_parents = Vec::new();
    for (feature, reference) in ctx.admit_iter(&*features, OPERATION)?.zip(&references) {
        let parent = match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference: Some(DatumPlaneReference::Feature { .. }),
                distance,
            }) if same_scalar(distance.get(), 0.0) => *reference,
            _ => None,
        };
        ctx.push_scoped_vec(&mut scratch, &mut zero_offset_parents, parent, OPERATION)?;
    }
    let roots = zero_offset_roots(ctx, &mut scratch, &zero_offset_parents)?;

    for (position, feature) in ctx.admit_iter(&mut *features, OPERATION)?.enumerate() {
        let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference:
                Some(DatumPlaneReference::Feature {
                    feature: reference_id,
                }),
            distance,
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        let own = facts[position];
        let invalid = match references[position] {
            None => true,
            Some(reference) if reference == position => true,
            Some(reference) => {
                let reference = facts[reference];
                let geometrically_compatible = reference
                    .frame
                    .zip(own.stored)
                    .map(|(reference, result)| offset_frame_matches(reference, result, *distance));
                let explicit_principal_identity_without_face_fallback = own
                    .explicit_native_reference
                    && reference.base == Some(BasePlaneKind::Principal)
                    && !own.has_face_fallback;
                let explicit_frame_identity = own.explicit_native_reference
                    && reference.base.is_some()
                    && reference
                        .frame
                        .zip(own.stored)
                        .is_some_and(|(reference, result)| plane_normal_matches(reference, result))
                    && own
                        .serialized
                        .map(|(origin, normal, u_axis)| (origin.get(), normal, u_axis))
                        .zip(reference.frame)
                        .is_none_or(|(serialized, reference)| {
                            plane_frames_match(serialized, reference)
                        });
                reference.ordinal >= own.ordinal
                    && !(own.explicit_native_reference && geometrically_compatible == Some(true)
                        || explicit_principal_identity_without_face_fallback
                        || explicit_frame_identity)
            }
        };
        if invalid {
            remove_dependency(ctx, &mut feature.dependencies, reference_id, OPERATION)?;
            feature.evaluation.edit(|definition, _| {
                if let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                    reference,
                    ..
                }) = definition
                {
                    *reference = None;
                }
            });
            references[position] = None;
        } else if !ctx.contains(feature.dependencies.as_slice(), reference_id, OPERATION)? {
            let reference_id = copy_projected_feature_id(ctx, reference_id)?;
            feature.dependencies.insert(ctx, reference_id, OPERATION)?;
        }
    }
    let mut frames = Vec::new();
    for fact in ctx.admit_iter(&facts, OPERATION)? {
        let frame = if fact.base.is_some() {
            fact.frame
        } else {
            None
        };
        ctx.push_scoped_vec(&mut scratch, &mut frames, frame, OPERATION)?;
    }

    let mut offset_positions = Vec::new();
    let mut candidates_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut candidates_by_history = BTreeMap::<usize, Vec<usize>>::new();
    let mut candidates_indexed = false;
    for (position, feature) in ctx.admit_iter(&*features, OPERATION)?.enumerate()
    {
        let offset = matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane { .. })
        );
        if offset {
            ctx.push_scoped_vec(&mut scratch, &mut offset_positions, position, OPERATION)?;
        }
    }

    loop {
        let mut changed = false;
        for &position in ctx.admit_iter(&offset_positions, OPERATION)? {
            let feature = &features[position];
            let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference: Some(DatumPlaneReference::Feature { .. }),
                distance,
            }) = feature.evaluation.definition()
            else {
                continue;
            };
            if frames[position].is_some() {
                continue;
            }
            let Some((origin, normal, u_axis)) =
                references[position].and_then(|reference| frames[reference])
            else {
                continue;
            };
            let normal_length = normal.norm();
            frames[position] = Some((
                Point3::new(
                    origin.x + normal.x * distance.get() / normal_length,
                    origin.y + normal.y * distance.get() / normal_length,
                    origin.z + normal.z * distance.get() / normal_length,
                ),
                normal,
                u_axis,
            ));
            changed = true;
        }

        let mut binding_storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut bindings = Vec::new();
        for &position in ctx.admit_iter(&offset_positions, OPERATION)? {
            let feature = &features[position];
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
            let own = facts[position];
            let Some(stored) = own.stored else {
                continue;
            };
            if same_scalar(distance.get(), 0.0) {
                continue;
            }
            let Some(history) = own.history else {
                continue;
            };
            let serialized = own
                .serialized
                .map(|(origin, normal, u_axis)| (origin.get(), normal, u_axis));
            // The one zero-offset root every matching earlier plane reaches.
            let mut found: Option<(usize, f64)> = None;
            if !candidates_indexed {
                const INDEX: &str = "index SLDPRT offset plane candidates";
                for (candidate_position, (candidate, fact)) in ctx
                    .admit_iter(&*features, INDEX)?
                    .zip(&facts)
                    .enumerate()
                {
                    if fact.base.is_some()
                        || matches!(candidate.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane { .. }))
                    {
                        if let Some(history) = fact.history {
                            candidates_storage.with_storage(|| {
                                let candidates = ctx.entry_btree_map(&mut candidates_by_history, history, INDEX)?.or_default();
                                ctx.push_vec(candidates, candidate_position, INDEX)
                            })?;
                        }
                    }
                }
                candidates_indexed = true;
            }
            let Some(candidates) =
                ctx.get_btree_map(&candidates_by_history, &history, OPERATION)?
            else {
                continue;
            };
            let ambiguous = ctx.any_by(
                candidates,
                |&candidate_position| {
                    let candidate = &facts[candidate_position];
                    let frame = frames[candidate_position];
                    let root = roots[candidate_position];
                    if !(candidate.ordinal < own.ordinal
                        || (serialized.is_some()
                            && candidate.base == Some(BasePlaneKind::Principal)))
                    {
                        return Ok(false);
                    }
                    let Some(frame) = frame else {
                        return Ok(false);
                    };
                    if serialized.is_some_and(|serialized| !plane_frames_match(serialized, frame)) {
                        return Ok(false);
                    }
                    let Some(signed_distance) = parallel_offset(frame, stored)
                        .filter(|signed| same_scalar(signed.abs(), distance.get().abs()))
                    else {
                        return Ok(false);
                    };
                    let candidate_distance = distance.get().abs().copysign(signed_distance);
                    Ok(match found {
                        None => {
                            found = Some((root, candidate_distance));
                            false
                        }
                        Some((known, _)) => known != root,
                    })
                },
                OPERATION,
            )?;
            let (Some(binding), false) = (found, ambiguous) else {
                continue;
            };
            ctx.push_scoped_vec(
                &mut binding_storage,
                &mut bindings,
                (position, binding),
                OPERATION,
            )?;
        }
        for (position, (root, distance)) in ctx.admit_iter(bindings, OPERATION)? {
            let Some(distance) = Length::new(distance) else {
                continue;
            };
            let reference = copy_projected_feature_id(ctx, &features[root].id)?;
            let dependency = copy_projected_feature_id(ctx, &reference)?;
            let mut bound = false;
            features[position].evaluation.edit(|definition, _| {
                if let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                    reference: slot,
                    distance: stored_distance,
                }) = definition
                {
                    *slot = Some(DatumPlaneReference::Feature { feature: reference });
                    *stored_distance = distance;
                    bound = true;
                }
            });
            if !bound {
                continue;
            }
            references[position] = Some(root);
            features[position]
                .dependencies
                .insert(ctx, dependency, OPERATION)?;
            changed = true;
        }
        if !changed {
            break;
        }
    }
    for (feature, fact) in ctx.admit_iter(features, OPERATION)?.zip(&facts) {
        feature.evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference: reference @ None,
                ..
            }) = definition
            {
                *reference = fact.serialized.and_then(|(origin, normal, u_axis)| {
                    Some(DatumPlaneReference::ResolvedPlane {
                        frame: cadmpeg_ir::features::FeatureSupportPlaneFrame::from_parts(
                            origin,
                            cadmpeg_ir::features::FeatureDirection3::new(normal)?,
                            cadmpeg_ir::features::FeatureDirection3::new(u_axis)?,
                        )?,
                    })
                });
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
    const OPERATION: &str = "bind SLDPRT native construction references";
    const SOURCES: &str = "index SLDPRT native construction sources";
    const TARGETS: &str = "index SLDPRT native construction features";
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    let mut construction_native_refs = HashSet::new();
    for history in ctx.admit_iter(histories, "scan SLDPRT feature histories")? {
        for feature in ctx.admit_iter(&history.features, SOURCES)? {
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
                &construction_native_refs,
                feature.id.as_str(),
                OPERATION,
            )? {
                continue;
            }
            scratch.with_storage(|| {
                ctx.insert_hash_set(&mut construction_native_refs, feature.id.as_str(), SOURCES)
            })?;
        }
    }
    let mut feature_ids_by_native = HashMap::new();
    for feature in ctx.admit_iter(&*features, TARGETS)? {
        let Some(native) = feature.native_ref.as_deref() else {
            continue;
        };
        if ctx.contains_hash_set(&construction_native_refs, native, TARGETS)? {
            scratch.with_storage(|| {
                let native = ctx.copy_retained_text(native, TARGETS)?;
                let id = copy_projected_feature_id(ctx, &feature.id)?;
                ctx.insert_hash_map(&mut feature_ids_by_native, native, id, TARGETS)
            })?;
        }
    }

    for feature in ctx.admit_iter(features, OPERATION)? {
        let feature_id = &feature.id;
        let dependencies = &mut feature.dependencies;
        let insert_dependency =
            |dependencies: &mut cadmpeg_ir::features::DistinctMembers<FeatureId>,
             target: &FeatureId|
             -> Result<(), CodecError> {
                if !ctx.equal(target, feature_id, OPERATION)?
                    && !ctx.contains(dependencies.as_slice(), target, OPERATION)?
                {
                    dependencies.insert(ctx, copy_projected_feature_id(ctx, target)?, OPERATION)?;
                }
                Ok(())
            };
        let bind_planar = |profile: &mut PlanarProfileRef,
                           dependencies: &mut cadmpeg_ir::features::DistinctMembers<FeatureId>|
         -> Result<(), CodecError> {
            let PlanarProfileRef::Native(native) = profile else {
                return Ok(());
            };
            let Some(target) =
                ctx.get_hash_map(&feature_ids_by_native, native.as_str(), OPERATION)?
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
                        for section in ctx.admit_iter(&mut sections[..], OPERATION)? {
                            if let cadmpeg_ir::features::LoftSection::Profile(profile) = section {
                                bind(profile, dependencies)?;
                            }
                        }
                    }
                    FeatureDefinition::Operation(FeatureOperation::SplitFace {
                        tool: SplitFaceTool::Path(PathRef::Native(native)),
                        ..
                    }) => {
                        if let Some(target) =
                            ctx.get_hash_map(&feature_ids_by_native, native.as_str(), OPERATION)?
                        {
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
    for history in ctx.admit_iter(histories, OPERATION)? {
        for feature in ctx.admit_iter(&history.features, OPERATION)? {
            if !is_custom_property(feature) {
                continue;
            }
            let id = ctx.format_retained(
                format_args!(
                    "sldprt:history:custom-property#{}",
                    EncodedNativeKey(native_key(&feature.id))
                ),
                OPERATION,
            )?;
            ctx.charge_work(cadmpeg_core::decode::u64_from_index(id.len()), OPERATION)?;
            let id = AttributeId::mint(id)
                .map_err(|_| CodecError::malformed("invalid SLDPRT custom-property ID"))?;
            let name = ctx.copy_retained_text(&feature.name, OPERATION)?;
            let mut values = Vec::new();
            if let Some(text) = &feature.text {
                let value = ctx.copy_retained_text(text, OPERATION)?;
                ctx.push_vec(&mut values, AttributeValue::String(value), OPERATION)?;
            }
            ctx.push_vec(
                &mut attributes,
                SourceAttribute {
                    id,
                    target: AttributeTarget::Document,
                    name,
                    values,
                },
                OPERATION,
            )?;
        }
    }
    Ok(attributes)
}

/// The record holding each native source of one history, by position, when
/// exactly one non-metadata record holds it; `None` when several do.
fn unique_source_bindings(
    ctx: &DecodeContext<'_>,
    history: &FeatureHistory,
    metadata: &[bool],
) -> Result<BTreeMap<FeatureSource, Option<usize>>, CodecError> {
    const OPERATION: &str = "index SLDPRT unique feature sources";
    let mut sources = BTreeMap::new();
    for (position, (feature, is_metadata)) in ctx
        .admit_iter(&history.features, OPERATION)?
        .zip(metadata)
        .enumerate()
    {
        if *is_metadata {
            continue;
        }
        let Some(source) = feature.source_id else {
            continue;
        };
        ctx.entry_btree_map(&mut sources, source, OPERATION)?
            .and_modify(|existing| *existing = None)
            .or_insert(Some(position));
    }
    Ok(sources)
}

pub(crate) fn incomplete_history_reference_features(
    ctx: &DecodeContext<'_>,
    histories: &[FeatureHistory],
) -> Result<usize, CodecError> {
    const OPERATION: &str = "count SLDPRT incomplete history references";
    let mut incomplete = 0usize;
    for history in ctx.admit_iter(histories, OPERATION)? {
        let index = HistoryIndex::new(ctx, &history.features)?;
        let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
        let mut metadata = Vec::new();
        let mut native_ids = HashSet::new();
        for feature in ctx.admit_iter(&history.features, OPERATION)? {
            let is_metadata = index.is_metadata(ctx, feature)?;
            ctx.push_scoped_vec(&mut scratch, &mut metadata, is_metadata, OPERATION)?;
            if !ctx.contains_hash_set(&native_ids, feature.id.as_str(), OPERATION)? {
                scratch.with_storage(|| {
                    ctx.insert_hash_set(&mut native_ids, feature.id.as_str(), OPERATION)
                })?;
            }
        }
        let sources = scratch.with_storage(|| unique_source_bindings(ctx, history, &metadata))?;
        for (position, feature) in ctx.admit_iter(&history.features, OPERATION)?.enumerate() {
            let source_binding = |source: &FeatureSource| -> Result<_, CodecError> {
                Ok(ctx.get_btree_map(&sources, source, OPERATION)?.copied())
            };
            let duplicate_source = match &feature.source_id {
                Some(source) => source_binding(source)? == Some(None),
                None => false,
            };
            let parent_requested = feature.tree_parent.is_some();
            let parent_resolved = match feature.tree_parent_record_id() {
                Some(parent) => ctx.contains_hash_set(&native_ids, parent, OPERATION)?,
                None => false,
            } || match feature.parent_source_id() {
                Some(source) => matches!(source_binding(&source)?, Some(Some(_))),
                None => false,
            };
            let incomplete_content = ctx.any_by(
                &feature.content,
                |item| {
                    Ok(match item {
                        FeatureContent::Feature(child) => {
                            !ctx.contains_hash_set(&native_ids, child.as_str(), OPERATION)?
                        }
                        FeatureContent::Dimension(name) => !ctx.contains_key_btree_map(
                            &feature.parameters,
                            name.as_str(),
                            OPERATION,
                        )?,
                        FeatureContent::Text(_) => false,
                    })
                },
                OPERATION,
            )?;
            let mut unresolved_dependency = false;
            for name in FEATURE_REFERENCE_PROPERTIES {
                let Some(value) = ctx.get_btree_map(&feature.properties, *name, OPERATION)? else {
                    continue;
                };
                let mut characters = value.char_indices();
                let mut start = 0;
                let mut finished = false;
                while !finished {
                    let delimiter = ctx.find_map(
                        &mut characters,
                        |(offset, character)| {
                            Ok(
                                (character == ',' || character == ';' || character.is_whitespace())
                                    .then_some((offset, character.len_utf8())),
                            )
                        },
                        OPERATION,
                    )?;
                    let (end, width) = delimiter.unwrap_or((value.len(), 0));
                    finished = delimiter.is_none();
                    let reference = &value[start..end];
                    start = end + width;
                    if reference.is_empty() {
                        continue;
                    }
                    // A reference resolves to one other record holding its source.
                    let parsed_source = if reference == "-1" {
                        Some(FeatureSource::Reserved)
                    } else {
                        ctx.parse_text::<u32>(reference, "parse SLDPRT feature dependency source")?
                            .ok()
                            .and_then(FeatureSource::from_value)
                    };
                    let resolved = match parsed_source {
                        Some(reference) => {
                            matches!(source_binding(&reference)?, Some(Some(bound)) if bound != position)
                        }
                        None => false,
                    };
                    if !resolved {
                        unresolved_dependency = true;
                        break;
                    }
                }
                if unresolved_dependency {
                    break;
                }
            }
            if duplicate_source
                || (parent_requested && !parent_resolved)
                || incomplete_content
                || unresolved_dependency
            {
                incomplete = incomplete
                    .checked_add(1)
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            }
        }
    }
    Ok(incomplete)
}

/// The feature's parameter names in projection order: the dimension children
/// naming a parameter, in content order, then the remaining parameters in key
/// order, each once. The order and the position of each name are scratch.
struct ParameterOrder<'f, 'c> {
    names: Vec<&'f str>,
    positions: HashMap<&'f str, usize>,
    storage: ScopedReservation<'c>,
}

fn parameter_order<'f, 'c>(
    ctx: &'c DecodeContext<'_>,
    feature: &'f Feature,
) -> Result<ParameterOrder<'f, 'c>, CodecError> {
    const OPERATION: &str = "order SLDPRT feature parameters";
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut order = Vec::new();
    let mut positions = HashMap::new();
    for content in ctx.admit_iter(&feature.content, OPERATION)? {
        let FeatureContent::Dimension(name) = content else {
            continue;
        };
        let name = name.as_str();
        if !ctx.contains_key_btree_map(&feature.parameters, name, OPERATION)?
            || ctx.contains_key_hash_map(&positions, name, OPERATION)?
        {
            continue;
        }
        storage.with_storage(|| {
            ctx.insert_hash_map(&mut positions, name, order.len(), OPERATION)?;
            ctx.push_vec(&mut order, name, OPERATION)
        })?;
    }
    for (name, _) in ctx.admit_iter(&feature.parameters, OPERATION)? {
        let name = name.as_str();
        if ctx.contains_key_hash_map(&positions, name, OPERATION)? {
            continue;
        }
        storage.with_storage(|| {
            ctx.insert_hash_map(&mut positions, name, order.len(), OPERATION)?;
            ctx.push_vec(&mut order, name, OPERATION)
        })?;
    }
    Ok(ParameterOrder {
        names: order,
        positions,
        storage,
    })
}

fn project_feature_content(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    by_native: &HashMap<&str, &FeatureId>,
) -> Result<cadmpeg_ir::features::FeatureContent, CodecError> {
    const OPERATION: &str = "project SLDPRT feature content";
    if feature.text.is_some() {
        return Ok(cadmpeg_ir::features::FeatureContent::default());
    }
    let ParameterOrder {
        names: order,
        positions,
        mut storage,
    } = parameter_order(ctx, feature)?;
    let mut projected_parameters =
        storage.with_storage(|| ctx.alloc_filled(order.len(), false, OPERATION))?;
    let mut result = cadmpeg_ir::features::FeatureContent::default();
    for content in ctx.admit_iter(&feature.content, OPERATION)? {
        let value = match content {
            FeatureContent::Text(text) => {
                FeatureSourceContent::Text(copy_projected_feature_text(ctx, text)?)
            }
            FeatureContent::Dimension(name) => {
                let Some(&ordinal) = ctx.get_hash_map(&positions, name.as_str(), OPERATION)? else {
                    continue;
                };
                if std::mem::replace(&mut projected_parameters[ordinal], true) {
                    continue;
                }
                FeatureSourceContent::Parameter(neutral_parameter_id(ctx, feature, ordinal)?)
            }
            FeatureContent::Feature(id) => {
                let Some(target) = ctx.get_hash_map(by_native, id.as_str(), OPERATION)? else {
                    continue;
                };
                FeatureSourceContent::Feature(copy_projected_feature_id(ctx, target)?)
            }
        };
        result.push(value, ctx, OPERATION)?;
    }
    Ok(result)
}

fn project_feature_dependencies(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    owner: &FeatureId,
    by_source: &NeutralByKey<'_, '_>,
) -> Result<cadmpeg_ir::features::DistinctMembers<FeatureId>, CodecError> {
    const OPERATION: &str = "collect SLDPRT feature dependencies";
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut seen = HashSet::new();
    let mut dependencies = Vec::new();
    for property in FEATURE_REFERENCE_PROPERTIES {
        let Some(value) = ctx.get_btree_map(&feature.properties, *property, OPERATION)? else {
            continue;
        };
        let mut characters = value.char_indices();
        let mut start = 0;
        let mut finished = false;
        while !finished {
            let delimiter = ctx.find_map(
                &mut characters,
                |(offset, character)| {
                    Ok(
                        (character == ',' || character == ';' || character.is_whitespace())
                            .then_some((offset, character.len_utf8())),
                    )
                },
                OPERATION,
            )?;
            let (end, width) = delimiter.unwrap_or((value.len(), 0));
            finished = delimiter.is_none();
            let reference = &value[start..end];
            start = end + width;
            if reference.is_empty() {
                continue;
            }
            let Some(&dependency) = ctx.get_hash_map(by_source, reference, OPERATION)? else {
                continue;
            };
            if ctx.equal(dependency, owner, OPERATION)?
                || ctx.contains_hash_set(&seen, dependency, OPERATION)?
            {
                continue;
            }
            storage.with_storage(|| ctx.insert_hash_set(&mut seen, dependency, OPERATION))?;
            ctx.push_vec(
                &mut dependencies,
                copy_projected_feature_id(ctx, dependency)?,
                OPERATION,
            )?;
        }
    }
    Ok(cadmpeg_ir::features::DistinctMembers::try_from(
        dependencies,
        ctx,
    )?)
}

/// Project decoded native configurations under the caller's resource budget.
pub(crate) fn project_configurations_charged(
    ctx: &DecodeContext<'_>,
    histories: &[FeatureHistory],
) -> Result<Vec<DesignConfiguration>, CodecError> {
    const OPERATION: &str = "project SLDPRT configurations";
    let mut projected = Vec::new();
    for history in ctx.admit_iter(histories, OPERATION)? {
        for configuration in ctx.admit_iter(&history.configurations, OPERATION)? {
            let native_id = configuration
                .id
                .strip_prefix("sldprt:history:configuration#")
                .unwrap_or(&configuration.id);
            let id = ctx.format_retained(
                format_args!("sldprt:model:configuration#{}", EncodedNativeKey(native_id)),
                OPERATION,
            )?;
            ctx.charge_work(cadmpeg_core::decode::u64_from_index(id.len()), OPERATION)?;
            let id = ConfigurationId::mint(id)
                .map_err(|_| CodecError::malformed("invalid SLDPRT configuration ID"))?;
            let properties =
                copy_projected_feature_properties(ctx, &configuration.properties, OPERATION)?;
            let material = configuration
                .material
                .as_deref()
                .map(|material| ctx.copy_retained_text(material, OPERATION))
                .transpose()?;
            let native_ref = ctx.copy_retained_text(&configuration.id, OPERATION)?;
            let name = ctx.copy_retained_text(&configuration.name, OPERATION)?;
            ctx.push_vec(
                &mut projected,
                DesignConfiguration {
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
                },
                OPERATION,
            )?;
        }
    }
    Ok(projected)
}
/// Project one native feature's neutral operation definition.
fn project_definition(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    by_source: &NeutralByKey<'_, '_>,
    native_by_source: &HashMap<&str, &str>,
    source_features: &mut self::solid::SourceFeatures<'_, '_>,
    records: &self::solid::RecordsById<'_>,
    index: &HistoryIndex<'_, '_>,
) -> Result<FeatureDefinition, CodecError> {
    const OPERATION: &str = "project SLDPRT feature definition";
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
    if let Some(role) = index.tree_node_role(ctx, feature)? {
        return Ok(FeatureDefinition::Operation(FeatureOperation::TreeNode {
            role,
            children: cadmpeg_ir::features::TreeChildren::default(),
        }));
    }
    let class = classify(feature);
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
                block: match ctx.get_btree_map(&feature.properties, "BlockDefinition", OPERATION)? {
                    Some(source) => ctx
                        .get_hash_map(by_source, source.as_str(), OPERATION)?
                        .map(|id| copy_projected_feature_id(ctx, id))
                        .transpose()?,
                    None => None,
                },
                placement: named_literal(ctx, &feature.properties, "BlockOrigin", OPERATION)?
                    .and_then(block_placement),
            },
        ));
    }
    if class == Some(FeatureClass::ReferencePlane) && is_offset_plane(ctx, feature)? {
        return project_offset_plane(ctx, feature, by_source)?
            .map_or_else(|| native_definition(ctx, feature), Ok);
    }
    if let Some(plane) = index.principal_plane(ctx, feature)? {
        return Ok(FeatureDefinition::Operation(
            FeatureOperation::DatumPrincipalPlane { plane },
        ));
    }
    if class == Some(FeatureClass::ReferencePlane) {
        return project_datum_plane(ctx, feature)?.map_or_else(
            || {
                if ctx.contains_key_btree_map(&feature.properties, "NativeRole", OPERATION)? {
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
        return project_datum_axis(ctx, feature)?
            .map_or_else(|| native_definition(ctx, feature), Ok);
    }
    if class == Some(FeatureClass::ReferencePoint) {
        return project_datum_point(ctx, feature)?
            .map_or_else(|| native_definition(ctx, feature), Ok);
    }
    if class == Some(FeatureClass::CoordinateSystem) {
        return Ok(project_datum_coordinate_system(ctx, feature)?.unwrap_or(
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
        return Ok(match project_helix(ctx, feature)? {
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
        project_extrude(ctx, feature, native_by_source, source_features)?
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
        project_flex(ctx, feature)?
    } else if class == Some(FeatureClass::Scale) {
        project_scale(ctx, feature)?
    } else if class == Some(FeatureClass::Hole) {
        project_hole(ctx, feature, &source_features.records, records)?
            .map_or_else(|| native_definition(ctx, feature), Ok)?
    } else if class == Some(FeatureClass::Revolve) {
        project_revolve(ctx, feature, native_by_source)?
    } else if class == Some(FeatureClass::Pattern) {
        project_pattern(ctx, feature, by_source, native_by_source)?
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

/// The feature's projected parameter names, each once, in projection order.
pub(super) fn projected_parameter_names(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Vec<String>, CodecError> {
    const OPERATION: &str = "collect SLDPRT parameter names";
    let order = parameter_order(ctx, feature)?;
    let mut names = Vec::new();
    for name in ctx.admit_iter(&order.names, OPERATION)? {
        let name = ctx.copy_retained_text(name, OPERATION)?;
        ctx.push_vec(&mut names, name, OPERATION)?;
    }
    Ok(names)
}

pub(super) fn neutral_parameter_id(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    ordinal: usize,
) -> Result<ParameterId, CodecError> {
    const OPERATION: &str = "retain SLDPRT projected parameter ID";
    let id = ctx.format_retained(
        format_args!(
            "sldprt:model:parameter#{}:{ordinal}",
            EncodedNativeKey(native_key(&feature.id))
        ),
        OPERATION,
    )?;
    mint_identity(ctx, id, OPERATION)
}

fn native_definition(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    Ok(FeatureDefinition::Operation(FeatureOperation::Native {
        kind: ctx
            .copy_retained_text(&feature.kind, "retain SLDPRT native definition kind")?
            .into(),
        parameters: copy_projected_feature_properties(
            ctx,
            &feature.parameters,
            "collect SLDPRT native definition parameters",
        )?,
    }))
}
