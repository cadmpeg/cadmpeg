// SPDX-License-Identifier: Apache-2.0
//! High-level `.sldprt` decoding.
//!
//! [`decode`] scans the outer [`crate::container`], groups related Parasolid
//! `partition` and `deltas` streams, and preserves the native active site when
//! one is identified. Other sites are merged with qualified identities; when
//! no active site is identified, every site is merged with qualified
//! identities. It then adds appearances, display meshes, document attributes,
//! feature history, feature-input lanes, provenance, and retained source data.
//!
//! The returned [`Decoded`] carries the IR and its diagnostics body; the sealed
//! codec wrapper stamps the identity the IR source authored onto the report.
//! Untyped surface and curve carriers become opaque geometry linked to the
//! retained Parasolid source record. If no body stream yields geometry, decoding returns a
//! metadata-only IR and blocking loss notes. [`DecodeOptions::container_only`]
//! requests the metadata-only path.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::Hash;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::{AppearanceId, UnknownId};

use crate::loss::SldprtLossCode;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

mod digest_partition;

use crate::container::configuration_index;

use crate::brep::feature_source::FeatureSourceId;
use crate::brep::graph::{decode_bodies, Brep};
use crate::container::{self, ActiveParasolidSite, ContainerScan};
use crate::parasolid::StreamHeader;
use crate::records::ObjectId;
use cadmpeg_ir::geometry::SolvedCurveGeometry;

/// A configuration membership identity held as temporary storage until selected.
struct ConfigurationBodyIdentity<'ctx> {
    id: cadmpeg_ir::ids::BodyId,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

struct DecodedBrep<'ctx> {
    /// Representative stream whose header is common to every merged site.
    /// This can be present for an unresolved merge without selecting a site.
    metadata_header: Option<StreamHeader>,
    _workspace: cadmpeg_core::decode::ScopedReservation<'ctx>,
    brep: Brep,
    configuration_bodies: Vec<(usize, Vec<ConfigurationBodyIdentity<'ctx>>)>,
}

struct EvaluatedFeatureState<'a> {
    feature: &'a cadmpeg_ir::features::Feature,
    dependencies: &'a cadmpeg_ir::features::DistinctMembers<cadmpeg_ir::features::FeatureId>,
    outputs: &'a [cadmpeg_ir::ids::BodyId],
    definition: &'a cadmpeg_ir::features::FeatureDefinition,
}

/// Return whether a native definition represents an operation that needs a
/// neutral definition, rather than a metadata-only tree node.
///
/// Some localized feature-manager records have no neutral tree-node role. The
/// native definition preserves those records, but they do not describe a
/// modeling operation. Operation evidence is carried by the state edges,
/// source content, or source properties; without it, a native record is only
/// a retained tree item and must not create an operation-completeness loss.
fn native_feature_has_operation_evidence(state: &EvaluatedFeatureState<'_>) -> bool {
    matches!(
        state.definition,
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Native { .. }
        )
    ) && (!state.dependencies.is_empty()
        || !state.outputs.is_empty()
        || !state.feature.source_content.is_empty()
        || !state.feature.source_properties.is_empty())
}

/// Decode one seekable `.sldprt` stream into IR and diagnostics.
///
/// The function reads and retains the complete source image. Container framing
/// or I/O failures return [`CodecError`]; unsupported model records are reported
/// through the decode body when a partial result can be represented.
pub(crate) fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
    let mut scan = container::scan(ctx, root)?;
    let classification = crate::dialect::classify_layers(ctx, &scan)?;
    let form_padding = classification.host().form_code_padding();
    // Marker identities are admitted during scanning. Compound stream identities
    // are admitted here before B-rep and IR construction.
    let container_entities = u64_from_index(scan.compound_streams.len());
    ctx.charge_entities(container_entities, "admit SLDPRT container entities")?;
    let mut admitted_entities = 0_u64;

    if ctx.container_only() {
        let (ir, annotations, unknowns, mut pmi_losses, _native) = build_metadata_ir(
            ctx,
            &scan,
            &classification,
            form_padding,
            &mut admitted_entities,
        )?;
        let mut report = build_container_report(
            ctx,
            &scan,
            &classification,
            container::notes_charged(ctx, &scan)?,
        )?;
        ctx.append_vec(
            &mut report.losses,
            &mut pmi_losses,
            "append SLDPRT PMI losses",
        )?;
        return decode_result(ctx, ir, report, annotations, unknowns);
    }

    let (streams, _stream_storage) = ctx
        .with_scoped_storage("SLDPRT active body stream workspace", || {
            active_body_streams(ctx, &scan)
        })?;
    if !streams.is_empty() {
        ctx.charge_entities(u64_from_index(streams.len()), "admit SLDPRT body streams")?;
        if let Some((decoded, mut report)) = try_decode_brep(ctx, &scan, &streams, &classification)?
        {
            let (ir, annotations, unknowns, mut pmi_losses, native) = build_geometry_ir(
                ctx,
                &mut scan,
                &classification,
                decoded,
                form_padding,
                &mut admitted_entities,
            )?;
            ctx.append_vec(
                &mut report.losses,
                &mut pmi_losses,
                "append SLDPRT PMI losses",
            )?;
            append_tessellation_losses(ctx, &ir, &mut report)?;
            append_design_losses_with(ctx, &ir, Some(&native), &mut report)?;
            return decode_result(ctx, ir, report, annotations, unknowns);
        }
    }

    let (ir, annotations, unknowns, mut pmi_losses, native) = build_metadata_ir(
        ctx,
        &scan,
        &classification,
        form_padding,
        &mut admitted_entities,
    )?;
    let mut report = build_container_report(
        ctx,
        &scan,
        &classification,
        container::notes_charged(ctx, &scan)?,
    )?;
    ctx.append_vec(
        &mut report.losses,
        &mut pmi_losses,
        "append SLDPRT PMI losses",
    )?;
    append_design_losses_with(ctx, &ir, Some(&native), &mut report)?;
    decode_result(ctx, ir, report, annotations, unknowns)
}

fn push_report_loss(
    ctx: &DecodeContext<'_>,
    report: &mut DecodeBody,
    loss: cadmpeg_ir::report::loss::LossNote,
) -> Result<(), CodecError> {
    const OPERATION: &str = "append SLDPRT decode loss";
    ctx.push_vec(&mut report.losses, loss, OPERATION)
}

fn append_tessellation_losses(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    report: &mut DecodeBody,
) -> Result<(), CodecError> {
    let unresolved = ctx
        .admit_iter(
            &ir.model.tessellations[..],
            "scan SLDPRT append_tessellation_losses values",
        )?
        .filter(|mesh| mesh.body.is_none() || mesh.faces.is_empty())
        .count();
    if unresolved > 0 {
        push_report_loss(ctx, report, SldprtLossCode::TessellationFaceOwnershipUnresolved.note(format!(
                "{unresolved} DisplayLists tessellation table(s) do not resolve to B-rep face ownership. Geometry and native channels are retained without fabricating body or face references."
            )))?;
    }
    Ok(())
}

fn decode_result(
    ctx: &DecodeContext<'_>,
    mut ir: CadIr,
    body: DecodeBody,
    annotations: Annotations,
    mut unknowns: Vec<UnknownRecord>,
) -> Result<Decoded, CodecError> {
    const SOURCE_IMAGE: &str = "retain SLDPRT source image";
    let mut source_fidelity = cadmpeg_ir::SourceFidelity::with_annotations(annotations);
    let source_image = match ctx.position_by(
        &unknowns,
        |record| Ok(record.id().as_str() == "sldprt:file:source-image#0"),
        SOURCE_IMAGE,
    )? {
        Some(index) => ctx
            .drain_vec(&mut unknowns, index..index + 1, SOURCE_IMAGE)?
            .pop(),
        None => None,
    };
    source_fidelity.attach_native_unknown_records(&mut ir, "sldprt", unknowns, ctx)?;
    if let Some(source_image) = source_image {
        source_fidelity.retain_unknown_records("source", [source_image])?;
    }
    stamp_local_digests(ctx, &mut ir)?;
    Ok(Decoded {
        ir,
        body,
        source_fidelity,
    })
}

/// Each feature's tree parent: the first tree node, in feature order, that
/// lists it among its children.
fn tree_parents<'m>(
    ctx: &DecodeContext<'_>,
    features: &'m [cadmpeg_ir::features::Feature],
) -> Result<
    BTreeMap<&'m cadmpeg_ir::features::FeatureId, &'m cadmpeg_ir::features::FeatureId>,
    CodecError,
> {
    const OPERATION: &str = "index SLDPRT feature tree parents";
    let mut parents = BTreeMap::new();
    for candidate in ctx.admit_iter(features, OPERATION)? {
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::TreeNode { children, .. },
        ) = candidate.evaluation.definition()
        else {
            continue;
        };
        for child in ctx.admit_iter(&children[..], OPERATION)? {
            if !ctx.contains_key_btree_map(&parents, child, OPERATION)? {
                ctx.insert_btree_map(&mut parents, child, &candidate.id, OPERATION)?;
            }
        }
    }
    Ok(parents)
}

/// A feature's tree parent, else its regeneration predecessor.
fn feature_parent<'m>(
    ctx: &DecodeContext<'_>,
    ir: &'m CadIr,
    tree_parents: &BTreeMap<
        &'m cadmpeg_ir::features::FeatureId,
        &'m cadmpeg_ir::features::FeatureId,
    >,
    child: &cadmpeg_ir::features::FeatureId,
) -> Result<Option<&'m cadmpeg_ir::features::FeatureId>, CodecError> {
    Ok(
        match ctx.get_btree_map(tree_parents, child, "look up SLDPRT feature tree parents")? {
            Some(parent) => Some(*parent),
            None => ir.model.feature_regeneration_parent(child),
        },
    )
}

fn incomplete_pattern<C: cadmpeg_ir::features::patterns::CompositeStages>(
    ctx: &DecodeContext<'_>,
    pattern: &cadmpeg_ir::features::patterns::PatternKind<C>,
    incomplete_path: &dyn Fn(&cadmpeg_ir::features::PathRef) -> bool,
) -> Result<bool, CodecError> {
    use cadmpeg_ir::features::patterns::{PatternScaleCenter, PatternTransform};

    Ok(match pattern.definition() {
        cadmpeg_ir::features::patterns::PatternTransform::Unresolved { .. } => true,
        PatternTransform::Linear { direction, .. }
        | PatternTransform::LinearOffsets { direction, .. } => direction.is_none(),
        PatternTransform::Circular { .. } | PatternTransform::Mirror { .. } => false,
        PatternTransform::MirrorReference { .. } => true,
        PatternTransform::CircularAngles { .. } => false,
        PatternTransform::CurveDriven { path, .. } => path.as_ref().is_none_or(incomplete_path),
        PatternTransform::Scale { center, .. } => matches!(center, PatternScaleCenter::Native(_)),
        PatternTransform::Composite { stages } => ctx.any_by(
            stages.stages(),
            |stage| incomplete_pattern(ctx, &stage.pattern, incomplete_path),
            "scan SLDPRT composite pattern stages",
        )?,
    })
}

fn incomplete_binder_target(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    target: &cadmpeg_ir::features::BinderTarget,
    feature_positions: &BTreeMap<&cadmpeg_ir::features::FeatureId, u64>,
    consumer_ordinal: u64,
    dependencies: &[cadmpeg_ir::features::FeatureId],
) -> Result<bool, cadmpeg_core::CodecError> {
    Ok::<_, cadmpeg_core::CodecError>(match target {
        cadmpeg_ir::features::BinderTarget::Feature { feature } => {
            ctx.get_btree_map(feature_positions, feature, "look up SLDPRT ordered key")?
                .is_none_or(|ordinal| *ordinal >= consumer_ordinal)
                || !ctx.contains(dependencies, feature, "check SLDPRT binder dependencies")?
        }
        cadmpeg_ir::features::BinderTarget::External { document, object } => {
            !ctx.any_by(
                document.as_str().chars(),
                |character| Ok(!character.is_whitespace()),
                "check SLDPRT binder targets",
            )? || !ctx.any_by(
                object.as_str().chars(),
                |character| Ok(!character.is_whitespace()),
                "check SLDPRT binder targets",
            )?
        }
        cadmpeg_ir::features::BinderTarget::Native { .. } => true,
    })
}

fn sketch_constraint_has_complete_neutral_semantics(
    definition: &cadmpeg_ir::sketches::SketchConstraintDefinitionInput,
) -> bool {
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Constraint;

    match definition {
        Constraint::Native { .. } => false,
        Constraint::Disabled {}
        | Constraint::Coincident { .. }
        | Constraint::Polygon { .. }
        | Constraint::SplineGroup { .. }
        | Constraint::RectangularPattern { .. }
        | Constraint::CircularPattern { .. }
        | Constraint::TextFrame { .. }
        | Constraint::TextPath { .. }
        | Constraint::CoincidentLoci { .. }
        | Constraint::SameCoordinate { .. }
        | Constraint::PointOnObject { .. }
        | Constraint::Midpoint { .. }
        | Constraint::PointCoordinateValues { .. }
        | Constraint::MidpointCoordinate { .. }
        | Constraint::Offset { .. }
        | Constraint::ProjectedCopy { .. }
        | Constraint::AtIntersection { .. }
        | Constraint::Concentric { .. }
        | Constraint::Coradial { .. }
        | Constraint::Collinear { .. }
        | Constraint::Symmetric { .. }
        | Constraint::PointSymmetric { .. }
        | Constraint::Horizontal { .. }
        | Constraint::Vertical { .. }
        | Constraint::Parallel { .. }
        | Constraint::Perpendicular { .. }
        | Constraint::Tangent { .. }
        | Constraint::TangentLoci { .. }
        | Constraint::Curvature { .. }
        | Constraint::Equal { .. }
        | Constraint::EqualDistance { .. }
        | Constraint::Fixed { .. }
        | Constraint::ArcAngle { .. }
        | Constraint::EllipseAngle { .. }
        | Constraint::Distance { .. }
        | Constraint::DistanceLoci { .. }
        | Constraint::DistanceLociValue { .. }
        | Constraint::PolarDistance { .. }
        | Constraint::AngleDifference { .. }
        | Constraint::ScalarEquality { .. }
        | Constraint::HorizontalDistance { .. }
        | Constraint::VerticalDistance { .. }
        | Constraint::RepeatedDistance { .. }
        | Constraint::RepeatedLength { .. }
        | Constraint::ParallelLineSetDistance { .. }
        | Constraint::Angle { .. }
        | Constraint::AngleToAxis { .. }
        | Constraint::Radius { .. }
        | Constraint::RepeatedRadius { .. }
        | Constraint::Diameter { .. }
        | Constraint::RepeatedDiameter { .. }
        | Constraint::SnellsLaw { .. }
        | Constraint::Weight { .. }
        | Constraint::InternalAlignment { .. }
        | Constraint::Group { .. }
        | Constraint::Text { .. } => true,
    }
}

fn spatial_sketch_constraint_has_complete_neutral_semantics(
    definition: &cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput<
        cadmpeg_ir::units::UnitVector3,
        cadmpeg_ir::scalar::PositiveLength,
    >,
) -> bool {
    use cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput as Constraint;

    match definition {
        Constraint::Native { .. } => false,
        Constraint::Coincident { .. }
        | Constraint::Symmetric { .. }
        | Constraint::PointOnSurface { .. }
        | Constraint::Midpoint { .. }
        | Constraint::Tangent { .. }
        | Constraint::PointDistance { .. }
        | Constraint::PointLineDistance { .. }
        | Constraint::LineLength { .. }
        | Constraint::RepeatedLineLength { .. }
        | Constraint::ParallelLineDistance { .. }
        | Constraint::RepeatedParallelLineDistance { .. }
        | Constraint::ParallelLineSetDistance { .. }
        | Constraint::Offset { .. }
        | Constraint::ParallelToDirection { .. }
        | Constraint::SplineGroup { .. } => true,
    }
}

/// Count keys from an admitted traversal, charging each keyed update.
fn count_keys<K: Ord + cadmpeg_core::decode::cost::DecodeCost>(
    ctx: &DecodeContext<'_>,
    keys: impl IntoIterator<Item = K>,
    operation: &'static str,
) -> Result<BTreeMap<K, usize>, CodecError> {
    let mut counts = BTreeMap::<K, usize>::new();
    for key in keys {
        if let Some(count) = ctx.get_mut_btree_map(&mut (counts), &key, operation)? {
            let next = count
                .checked_add(1)
                .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
            *count = next;
        } else {
            ctx.insert_btree_map(&mut counts, key, 1, operation)?;
        }
    }
    Ok(counts)
}

fn has_incoherent_refs<T: Eq + Hash + cadmpeg_core::decode::cost::DecodeCost>(
    ctx: &DecodeContext<'_>,
    references: &[T],
    known: &HashSet<&T>,
    operation: &'static str,
) -> Result<bool, CodecError> {
    let mut seen = HashSet::new();
    let mut storage = ctx.reserve_scoped(0, operation)?;
    let mut references = references.iter();
    while let Some(reference) = ctx.next_charged(&mut references, operation)? {
        if ctx.contains_hash_set(&seen, reference, operation)?
            || !ctx.contains_hash_set(known, reference, operation)?
        {
            return Ok(true);
        }
        storage.with_storage(|| ctx.insert_hash_set(&mut seen, reference, operation))?;
    }
    Ok(false)
}

/// Scan a model read back from its native namespace, as a test fixture does.
#[cfg(test)]
fn append_design_losses(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    report: &mut DecodeBody,
) -> Result<(), CodecError> {
    let native = match ir.native.namespace("sldprt") {
        None => None,
        Some(namespace) => match crate::native::SldprtNative::load_charged(ctx, namespace) {
            Ok(native) => Some(native),
            Err(error) => match CodecError::from(error) {
                limit @ CodecError::ResourceLimit(_) => return Err(limit),
                _ => None,
            },
        },
    };
    append_design_losses_with(ctx, ir, native.as_ref(), report)
}

/// Report what the decoded design leaves unresolved, reading the native
/// records the decode stored.
fn append_design_losses_with(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    native: Option<&crate::native::SldprtNative>,
    report: &mut DecodeBody,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{
        AngularTermination, BodyRetentionMode, BodySelection, BooleanOp, EdgeSelection,
        ExtrudeExtent, FaceSelection, FeatureDefinition, FeatureOperation, FeatureSourceContent,
        LinearTermination, PathRef, PlanarProfileRef, ProfileRef, RevolveExtent, SplitFaceTool,
    };
    use cadmpeg_ir::sketches::{SketchGeometryDefinition, SpatialSketchGeometryDefinition};

    let active_configurations = ctx
        .admit_iter(
            &ir.model.configurations[..],
            "scan SLDPRT append_design_losses values",
        )?
        .filter(|configuration| configuration.active)
        .count();
    if !ir.model.configurations.is_empty() && active_configurations != 1 {
        push_report_loss(ctx, report, SldprtLossCode::ConfigActiveIdentityUnresolved.note(format!(
                "active configuration identity is unresolved; {active_configurations} of {} configuration records are active.",
                ir.model.configurations.len()
            )))?;
    }
    let active_partition = ir
        .source
        .as_ref()
        .map(|source| {
            ctx.get_btree_map(
                &source.attributes,
                "active_parasolid_block",
                "look up SLDPRT source attribute",
            )
        })
        .transpose()?
        .flatten()
        .map(|section| crate::container::configuration_index(ctx, section))
        .transpose()?
        .flatten()
        .and_then(|index| u32::try_from(index).ok());
    let active_partition_mismatch = match active_partition {
        Some(active_partition) => ctx
            .find_by(
                &ir.model.configurations,
                |configuration| Ok(configuration.active),
                "find SLDPRT active geometry configuration",
            )?
            .filter(|configuration| configuration.source_index.as_ref() != Some(&active_partition))
            .map(|_| active_partition),
        None => None,
    };
    if let Some(active_partition) = active_partition_mismatch {
        push_report_loss(ctx, report, SldprtLossCode::ConfigActivePartitionMismatch.note(format!(
                "active configuration identity does not resolve to active geometry partition {active_partition}."
            )))?;
    }
    let inferred_configurations = ctx
        .admit_iter(
            &ir.model.configurations[..],
            "scan SLDPRT append_design_losses values",
        )?
        .filter(|configuration| configuration.native_ref.is_none())
        .count();
    if inferred_configurations > 0 {
        push_report_loss(ctx, report, SldprtLossCode::ConfigInferredWithoutNative.note(format!(
                "{inferred_configurations} configuration state(s) are inferred from geometry partitions without native configuration definitions."
            )))?;
    }
    let unresolved_configuration_parameter_lanes = if let Some(native) = native.as_ref() {
        crate::history::configuration::unresolved_configuration_lanes(
            ctx,
            &ir.model.configurations,
            &native.feature_input_lanes,
        )?
    } else {
        0
    };
    if unresolved_configuration_parameter_lanes > 0 {
        push_report_loss(ctx, report, SldprtLossCode::ConfigLaneIdentityUnresolved.note(format!(
                "{unresolved_configuration_parameter_lanes} configuration-scoped feature-input lane(s) have duplicate or unresolved configuration identity."
            )))?;
    }
    let (configuration_source_counts, _configuration_source_counts_storage) = ctx
        .with_scoped_storage("SLDPRT configuration_source_counts workspace", || {
            count_keys(
                ctx,
                ctx.admit_iter(
                    &ir.model.configurations,
                    "count SLDPRT configuration source indices",
                )?
                .filter_map(|configuration| configuration.source_index),
                "count SLDPRT configuration source indices",
            )
        })?;
    let ambiguous_configuration_sources = ctx
        .admit_iter(
            &configuration_source_counts,
            "scan SLDPRT append_design_losses map values",
        )?
        .map(|(_, value)| value)
        .filter(|count| **count > 1)
        .copied()
        .sum::<usize>();
    if ambiguous_configuration_sources > 0 {
        push_report_loss(ctx, report, SldprtLossCode::ConfigAmbiguousPartition.note(format!(
                "{ambiguous_configuration_sources} configuration record(s) share non-unique geometry partition identities."
            )))?;
    }
    let empty_configuration_names = ctx
        .admit_iter(
            &ir.model.configurations[..],
            "scan SLDPRT append_design_losses values",
        )?
        .filter(|configuration| configuration.name.as_deref().is_none_or(str::is_empty))
        .count();
    let (configuration_ordinal_counts, _configuration_ordinal_counts_storage) = ctx
        .with_scoped_storage("SLDPRT configuration_ordinal_counts workspace", || {
            count_keys(
                ctx,
                ctx.admit_iter(
                    &ir.model.configurations,
                    "count SLDPRT configuration ordinals",
                )?
                .map(|configuration| configuration.ordinal),
                "count SLDPRT configuration ordinals",
            )
        })?;
    let (configuration_name_counts, _configuration_name_counts_storage) =
        ctx.with_scoped_storage("SLDPRT configuration_name_counts workspace", || {
            count_keys(
                ctx,
                ctx.admit_iter(&ir.model.configurations, "count SLDPRT configuration names")?
                    .filter_map(|configuration| configuration.name.as_deref())
                    .filter(|name| !name.is_empty()),
                "count SLDPRT configuration names",
            )
        })?;
    let ambiguous_configuration_names = ctx
        .admit_iter(
            &configuration_name_counts,
            "scan SLDPRT append_design_losses map values",
        )?
        .map(|(_, value)| value)
        .filter(|count| **count > 1)
        .copied()
        .sum::<usize>();
    let ambiguous_configuration_ordinals = ctx
        .admit_iter(
            &configuration_ordinal_counts,
            "scan SLDPRT append_design_losses map values",
        )?
        .map(|(_, value)| value)
        .filter(|count| **count > 1)
        .copied()
        .sum::<usize>();
    if empty_configuration_names > 0
        || ambiguous_configuration_names > 0
        || ambiguous_configuration_ordinals > 0
    {
        push_report_loss(ctx, report, SldprtLossCode::ConfigAmbiguousNaming.note(format!(
                "{empty_configuration_names} configuration record(s) have empty names; {ambiguous_configuration_names} configuration record(s) share non-unique names; {ambiguous_configuration_ordinals} configuration record(s) share regeneration ordinals."
            )))?;
    }
    let (model_body_ids, _model_body_ids_storage) =
        ctx.with_scoped_storage("SLDPRT model_body_ids workspace", || {
            ctx.collect_hash_set(
                ir.model.bodies.iter().map(|body| &body.id),
                "index SLDPRT model body IDs",
            )
        })?;
    let mut incoherent_configuration_bodies = 0;
    for configuration in ctx.admit_iter(
        &ir.model.configurations,
        "scan SLDPRT append_design_losses values",
    )? {
        if has_incoherent_refs(
            ctx,
            configuration.bodies.as_deref().unwrap_or_default(),
            &model_body_ids,
            "check SLDPRT configuration body references",
        )? {
            incoherent_configuration_bodies += 1;
        }
    }
    let unresolved_configuration_bodies = ctx
        .admit_iter(
            &ir.model.configurations[..],
            "scan SLDPRT append_design_losses values",
        )?
        .filter(|configuration| configuration.bodies.is_none())
        .count();
    if unresolved_configuration_bodies > 0 || incoherent_configuration_bodies > 0 {
        push_report_loss(ctx, report, SldprtLossCode::ConfigIncoherentBodyRefs.note(format!(
                "{unresolved_configuration_bodies} configuration record(s) have unresolved body membership; {incoherent_configuration_bodies} configuration record(s) contain missing or repeated body references."
            )))?;
    }

    let (feature_ids, _feature_ids_storage) =
        ctx.with_scoped_storage("SLDPRT feature_ids workspace", || {
            ctx.collect_hash_set(
                ir.model.features.iter().map(|feature| &feature.id),
                "index SLDPRT feature IDs",
            )
        })?;
    let (parameter_ids, _parameter_ids_storage) =
        ctx.with_scoped_storage("SLDPRT parameter_ids workspace", || {
            ctx.collect_hash_set(
                ir.model.parameters.iter().map(|parameter| &parameter.id),
                "index SLDPRT parameter IDs",
            )
        })?;
    let mut incomplete_configuration_feature_snapshots = 0;
    for configuration in ctx.admit_iter(
        &ir.model.configurations,
        "scan SLDPRT configuration feature snapshots",
    )? {
        if !configuration_source_needs_update(ctx, ir, configuration)?
            && (configuration.feature_states.len() != feature_ids.len()
                || ctx.any_by(
                    &configuration.feature_states,
                    |(feature, _)| {
                        Ok(!ctx.contains_hash_set(
                            &feature_ids,
                            feature,
                            "test SLDPRT hashed identity",
                        )?)
                    },
                    "scan SLDPRT configuration feature snapshot keys",
                )?)
        {
            incomplete_configuration_feature_snapshots += 1;
        }
    }
    let mut incomplete_configuration_parameter_snapshots = 0;
    for configuration in ctx.admit_iter(
        &ir.model.configurations,
        "scan SLDPRT configuration parameter snapshots",
    )? {
        if !configuration_source_needs_update(ctx, ir, configuration)?
            && (configuration.parameter_values.len() != parameter_ids.len()
                || ctx.any_by(
                    &configuration.parameter_values,
                    |(parameter, _)| {
                        Ok(!ctx.contains_hash_set(
                            &parameter_ids,
                            parameter,
                            "test SLDPRT hashed identity",
                        )?)
                    },
                    "scan SLDPRT configuration parameter snapshot keys",
                )?)
        {
            incomplete_configuration_parameter_snapshots += 1;
        }
    }
    if incomplete_configuration_feature_snapshots > 0
        || incomplete_configuration_parameter_snapshots > 0
    {
        push_report_loss(ctx, report, SldprtLossCode::ConfigIncompleteSnapshot.note(format!(
                "{incomplete_configuration_feature_snapshots} configuration(s) lack a complete evaluated feature snapshot; {incomplete_configuration_parameter_snapshots} configuration(s) lack a complete evaluated parameter snapshot."
            )))?;
    }
    let mut incoherent_configuration_suppression = 0;
    for configuration in ctx.admit_iter(
        &ir.model.configurations,
        "scan SLDPRT configuration suppression snapshots",
    )? {
        let mut incoherent = !ctx.all_by(
            &configuration.feature_states,
            |(id, _)| ctx.contains_hash_set(&feature_ids, id, "test SLDPRT hashed identity"),
            "scan SLDPRT configuration suppression members",
        )?;
        // The active configuration's states agree with each feature's own
        // suppression; each feature looks its state up.
        if !incoherent && configuration.active {
            incoherent = ctx.any_by(
                &ir.model.features,
                |feature| {
                    let Some(suppressed) = feature.suppressed else {
                        return Ok(false);
                    };
                    Ok(ctx
                        .get_btree_map(
                            &configuration.feature_states,
                            &feature.id,
                            "find SLDPRT configuration suppression state",
                        )?
                        .is_some_and(|state| suppressed != state.evaluation.is_suppressed()))
                },
                "scan SLDPRT configuration suppression features",
            )?;
        }
        incoherent_configuration_suppression += usize::from(incoherent);
    }
    let mut incoherent_configuration_overrides = 0;
    for configuration in ctx.admit_iter(
        &ir.model.configurations,
        "scan SLDPRT configuration override snapshots",
    )? {
        let incoherent = ctx.any_by(
            &configuration.parameter_overrides,
            |(parameter, _)| {
                Ok(!ctx.contains_hash_set(
                    &parameter_ids,
                    parameter,
                    "test SLDPRT hashed identity",
                )?)
            },
            "scan SLDPRT configuration override keys",
        )?;
        incoherent_configuration_overrides += usize::from(incoherent);
    }
    if incoherent_configuration_suppression > 0 || incoherent_configuration_overrides > 0 {
        push_report_loss(ctx, report, SldprtLossCode::ConfigIncompleteSnapshot.note(format!(
            "{incoherent_configuration_suppression} configuration(s) have missing, repeated, or feature-state-inconsistent suppression members; {incoherent_configuration_overrides} configuration(s) reference missing parameter overrides."
        )))?;
    }

    let mut lookup_storage = ctx.reserve_scoped(0, "SLDPRT design parameter lookup storage")?;
    let mut feature_names = HashMap::new();
    for feature in ctx.admit_iter(
        &ir.model.features,
        "scan SLDPRT append_design_losses values",
    )? {
        const OPERATION: &str = "index SLDPRT feature names";

        let Some(name) = &feature.name else {
            continue;
        };
        let id = lookup_storage.with_storage(|| {
            feature
                .id
                .try_clone_for_decode(ctx, "copy SLDPRT feature-name identity")
        })?;
        let name = lookup_storage
            .with_storage(|| ctx.copy_retained_text(name, "copy SLDPRT feature name"))?;
        lookup_storage
            .with_storage(|| ctx.insert_hash_map(&mut feature_names, id, name, OPERATION))?;
    }
    let mut global_parameter_owners = HashSet::new();
    for feature in ctx.admit_iter(
        &ir.model.features,
        "scan SLDPRT append_design_losses values",
    )? {
        const OPERATION: &str = "index SLDPRT global parameter owners";
        if !crate::history::parameters::is_global_parameter_owner(feature)
            || ctx.contains_hash_set(
                &(global_parameter_owners),
                &feature.id,
                "test SLDPRT hashed identity",
            )?
        {
            continue;
        }
        let id = lookup_storage.with_storage(|| {
            feature
                .id
                .try_clone_for_decode(ctx, "copy SLDPRT global parameter owner identity")
        })?;
        lookup_storage
            .with_storage(|| ctx.insert_hash_set(&mut global_parameter_owners, id, OPERATION))?;
    }
    let incomplete_parameters = ctx
        .admit_iter(
            &ir.model.parameters[..],
            "scan SLDPRT append_design_losses values",
        )?
        .try_fold(0_usize, |count, candidate| {
            let parameter = &candidate;
            Ok::<_, cadmpeg_core::CodecError>(
                count
                    + usize::from({
                        parameter.value.is_none()
                            && (ir.model.configurations.is_empty()
                                || ctx.any_by(
                                    &ir.model.configurations,
                                    |configuration| {
                                        Ok(!ctx.contains_key_btree_map(
                                            &configuration.parameter_values,
                                            &parameter.id,
                                            "test SLDPRT map key",
                                        )?)
                                    },
                                    "scan SLDPRT append_design_losses values",
                                )?)
                    }),
            )
        })?;
    let (parameter_aliases, _parameter_aliases_storage) =
        crate::history::parameters::ParameterAliases::scoped(
            ctx,
            &ir.model.parameters,
            &feature_names,
            &global_parameter_owners,
        )?;
    let unresolved_parameter_references =
        crate::history::parameters::parameters_with_unresolved_references(
            ctx,
            &ir.model.parameters,
            &parameter_aliases,
        )?;
    let unevaluable_parameter_expressions =
        crate::history::parameters::parameters_with_unevaluable_expressions(
            ctx,
            &ir.model.parameters,
            &parameter_aliases,
            &ir.model.configurations,
        )?;
    let (feature_ordinals, _feature_ordinals_storage) = ctx.collect_scoped_btree_map(
        ir.model
            .features
            .iter()
            .map(|feature| (&feature.id, feature.ordinal)),
        "index SLDPRT feature ordinals",
    )?;
    let (parameter_positions, _parameter_positions_storage) = ctx.collect_scoped_btree_map(
        ir.model
            .parameters
            .iter()
            .map(|parameter| (&parameter.id, (&parameter.owner, parameter.ordinal))),
        "index SLDPRT parameter positions",
    )?;
    let invalid_parameter_dependency_order = ctx
        .admit_iter(
            &ir.model.parameters[..],
            "scan SLDPRT append_design_losses values",
        )?
        .try_fold(0_usize, |count, candidate| {
            let parameter = &candidate;
            Ok::<_, cadmpeg_core::CodecError>(
                count
                    + usize::from({
                        ctx.any_by(
                            parameter.dependencies.as_slice(),
                            |dependency| {
                                Ok::<_, cadmpeg_core::CodecError>({
                                    let Some((owner, ordinal)) = ctx.get_btree_map(
                                        &(parameter_positions),
                                        dependency,
                                        "look up SLDPRT ordered key",
                                    )?
                                    else {
                                        return Ok(true);
                                    };
                                    if ctx.equal(
                                        *owner,
                                        &parameter.owner,
                                        "compare SLDPRT parameter owners",
                                    )? {
                                        return Ok(*ordinal >= parameter.ordinal);
                                    }
                                    let (Some(owner), Some(parameter_owner)) =
                                        (owner.as_ref(), parameter.owner.as_ref())
                                    else {
                                        return Ok(true);
                                    };
                                    ctx.get_btree_map(
                                        &(feature_ordinals),
                                        owner,
                                        "look up SLDPRT ordered key",
                                    )?
                                    .zip(ctx.get_btree_map(
                                        &(feature_ordinals),
                                        parameter_owner,
                                        "look up SLDPRT ordered key",
                                    )?)
                                    .is_none_or(
                                        |(dependency_owner, consumer_owner)| {
                                            dependency_owner >= consumer_owner
                                        },
                                    )
                                })
                            },
                            "scan SLDPRT append_design_losses values",
                        )?
                    }),
            )
        })?;
    let incoherent_parameter_dependencies =
        crate::history::parameters::parameters_with_incoherent_dependencies(
            ctx,
            &ir.model.parameters,
            &parameter_aliases,
        )?;
    let incoherent_parameter_values =
        crate::history::parameters::parameters_with_incoherent_evaluated_values(
            ctx,
            &ir.model.parameters,
            &parameter_aliases,
            &ir.model.configurations,
        )?;
    if incomplete_parameters > 0
        || unresolved_parameter_references > 0
        || unevaluable_parameter_expressions > 0
        || invalid_parameter_dependency_order > 0
        || incoherent_parameter_dependencies > 0
        || incoherent_parameter_values > 0
    {
        push_report_loss(ctx, report, SldprtLossCode::ParameterUnevaluated.note(format!(
                "{incomplete_parameters} parameter(s) lack an evaluated scalar; {unresolved_parameter_references} parameter expression(s) contain unresolved, ambiguous, or malformed parameter references; {unevaluable_parameter_expressions} parameter expression(s) cannot regenerate a finite typed value; {invalid_parameter_dependency_order} parameter record(s) contain missing or non-preceding dependency edges; {incoherent_parameter_dependencies} parameter record(s) have dependency edges inconsistent with their expressions; {incoherent_parameter_values} dependency-driven parameter(s) disagree with their evaluated expressions."
            )))?;
    }
    let empty_parameter_names = ctx
        .admit_iter(
            &ir.model.parameters[..],
            "scan SLDPRT append_design_losses values",
        )?
        .filter(|parameter| parameter.name.is_empty())
        .count();
    let (parameter_name_counts, _parameter_name_counts_storage) =
        ctx.with_scoped_storage("SLDPRT parameter_name_counts workspace", || {
            count_keys(
                ctx,
                ctx.admit_iter(&ir.model.parameters, "count SLDPRT parameter names")?
                    .filter(|parameter| !parameter.name.is_empty())
                    .map(|parameter| (&parameter.owner, parameter.name.as_str())),
                "count SLDPRT parameter names",
            )
        })?;
    let (parameter_ordinal_counts, _parameter_ordinal_counts_storage) =
        ctx.with_scoped_storage("SLDPRT parameter_ordinal_counts workspace", || {
            count_keys(
                ctx,
                ctx.admit_iter(&ir.model.parameters, "count SLDPRT parameter ordinals")?
                    .map(|parameter| (&parameter.owner, parameter.ordinal)),
                "count SLDPRT parameter ordinals",
            )
        })?;
    let duplicate_parameter_names = ctx
        .admit_iter(
            &parameter_name_counts,
            "scan SLDPRT append_design_losses map values",
        )?
        .map(|(_, value)| value)
        .filter(|count| **count > 1)
        .copied()
        .sum::<usize>();
    let duplicate_parameter_ordinals = ctx
        .admit_iter(
            &parameter_ordinal_counts,
            "scan SLDPRT append_design_losses map values",
        )?
        .map(|(_, value)| value)
        .filter(|count| **count > 1)
        .copied()
        .sum::<usize>();
    if empty_parameter_names > 0
        || duplicate_parameter_names > 0
        || duplicate_parameter_ordinals > 0
    {
        push_report_loss(ctx, report, SldprtLossCode::ParameterAmbiguousIdentity.note(format!(
                "{empty_parameter_names} parameter record(s) have empty names; {duplicate_parameter_names} parameter record(s) share owner-local names; {duplicate_parameter_ordinals} parameter record(s) share owner-local ordinals."
            )))?;
    }

    let mut bound_pmi_storage = ctx.reserve_scoped(0, "SLDPRT bound PMI lookup workspace")?;
    let mut bound_pmi = std::collections::HashSet::new();
    for pmi in ctx
        .admit_iter(&ir.model.parameters, "scan SLDPRT parameter PMI")?
        .filter_map(|parameter| parameter.pmi.as_ref())
    {
        let id = pmi.native_ref.as_str();
        bound_pmi_storage.with_storage(|| {
            ctx.insert_hash_set(&mut bound_pmi, id, "index SLDPRT bound PMI IDs")
        })?;
    }
    let unbound_pmi_dimensions = match native.as_ref() {
        Some(native) => {
            crate::pmi::unbound_dimension_count(ctx, &native.pmi_dimensions, &bound_pmi)?
        }
        None => 0,
    };
    let native_pmi_subtypes = ctx
        .admit_iter(
            &ir.model.parameters[..],
            "scan SLDPRT append_design_losses values",
        )?
        .filter(|parameter| {
            parameter.pmi.as_ref().is_some_and(|pmi| {
                matches!(
                    pmi.subtype,
                    cadmpeg_ir::features::PmiDimensionSubtype::Native(_)
                )
            })
        })
        .count();
    if unbound_pmi_dimensions > 0 || native_pmi_subtypes > 0 {
        push_report_loss(ctx, report, SldprtLossCode::PmiDimensionUnbound.note(format!(
                "{unbound_pmi_dimensions} semantic dimension record(s) are not bound to parameters; {native_pmi_subtypes} parameter dimension(s) retain native subtypes."
            )))?;
    }

    let incomplete_history_references = native
        .as_ref()
        .map(|native| {
            crate::history::project::incomplete_history_reference_features(
                ctx,
                &native.feature_histories,
            )
        })
        .transpose()?
        .unwrap_or(0);
    if incomplete_history_references > 0 {
        push_report_loss(ctx, report, SldprtLossCode::HistoryIncompleteReferences.note(format!(
            "{incomplete_history_references} feature history record(s) contain duplicate identities or unresolved parent, dependency, dimension, or child references."
        )))?;
    }
    let feature_positions = &feature_ordinals;
    let (tree_parents, _tree_parents_storage) = ctx
        .with_scoped_storage("SLDPRT tree_parents workspace", || {
            tree_parents(ctx, &ir.model.features)
        })?;
    let (evaluated_feature_states, _evaluated_feature_states_storage) =
        ctx.with_scoped_storage("SLDPRT evaluated_feature_states workspace", || {
            Ok::<_, CodecError>(
                if ctx.any_by(
                    &ir.model.configurations,
                    |configuration| Ok(!configuration.feature_states.is_empty()),
                    "scan SLDPRT append_design_losses values",
                )? {
                    let mut evaluated = Vec::new();
                    for configuration in ctx.admit_iter(
                        &ir.model.configurations,
                        "scan SLDPRT configured feature states",
                    )? {
                        for feature in ctx.admit_iter(
                            &ir.model.features,
                            "scan SLDPRT configuration feature candidates",
                        )? {
                            let Some(state) = ctx.get_btree_map(
                                &configuration.feature_states,
                                &feature.id,
                                "look up SLDPRT ordered key",
                            )?
                            else {
                                continue;
                            };
                            ctx.push_vec(
                                &mut evaluated,
                                EvaluatedFeatureState {
                                    feature,
                                    dependencies: &state.dependencies,
                                    outputs: state.evaluation.outputs(),
                                    definition: &state.definition,
                                },
                                "collect SLDPRT configured feature states",
                            )?;
                        }
                    }
                    evaluated
                } else {
                    ctx.collect_vec(
                        ir.model
                            .features
                            .iter()
                            .map(|feature| EvaluatedFeatureState {
                                feature,
                                dependencies: &feature.dependencies,
                                outputs: feature.evaluation.outputs(),
                                definition: feature.evaluation.definition(),
                            }),
                        "collect SLDPRT feature states",
                    )?
                },
            )
        })?;
    let incoherent_feature_edges = ctx
        .admit_iter(
            &evaluated_feature_states[..],
            "scan SLDPRT append_design_losses values",
        )?
        .try_fold(0_usize, |count, candidate| {
            let state = &candidate;
            Ok::<_, cadmpeg_core::CodecError>(
                count
                    + usize::from({
                        let feature = state.feature;
                        let parent_incoherent =
                            match feature_parent(ctx, ir, &tree_parents, &feature.id)? {
                                Some(parent) => ctx
                                    .get_btree_map(
                                        feature_positions,
                                        parent,
                                        "look up SLDPRT ordered key",
                                    )?
                                    .is_none_or(|ordinal| *ordinal >= feature.ordinal),
                                None => false,
                            };
                        parent_incoherent
                            || ctx.any_by(
                                state.dependencies.as_slice(),
                                |dependency| {
                                    Ok(ctx
                                        .get_btree_map(
                                            feature_positions,
                                            dependency,
                                            "look up SLDPRT ordered key",
                                        )?
                                        .is_none_or(|ordinal| *ordinal >= feature.ordinal))
                                },
                                "scan SLDPRT append_design_losses values",
                            )?
                    }),
            )
        })?;
    let (feature_ordinal_counts, _feature_ordinal_counts_storage) =
        ctx.with_scoped_storage("SLDPRT feature_ordinal_counts workspace", || {
            count_keys(
                ctx,
                ctx.admit_iter(&ir.model.features, "count SLDPRT feature ordinals")?
                    .map(|feature| feature.ordinal),
                "count SLDPRT feature ordinals",
            )
        })?;
    let duplicate_feature_ordinals = ctx
        .admit_iter(
            &feature_ordinal_counts,
            "scan SLDPRT append_design_losses map values",
        )?
        .map(|(_, value)| value)
        .filter(|count| **count > 1)
        .copied()
        .sum::<usize>();
    if incoherent_feature_edges > 0 || duplicate_feature_ordinals > 0 {
        push_report_loss(ctx, report, SldprtLossCode::FeatureIncoherentEdges.note(format!(
                "{incoherent_feature_edges} feature record(s) contain missing, repeated, or non-preceding parent/dependency edges; {duplicate_feature_ordinals} feature record(s) share regeneration ordinals."
            )))?;
    }
    let (parameter_owners, _parameter_owners_storage) = ctx.collect_scoped_btree_map(
        ir.model
            .parameters
            .iter()
            .map(|parameter| (&parameter.id, &parameter.owner)),
        "index SLDPRT parameter owners",
    )?;
    let (features_by_id, _features_by_id_storage) = ctx.collect_scoped_btree_map(
        ir.model
            .features
            .iter()
            .map(|feature| (&feature.id, feature)),
        "index SLDPRT features by ID",
    )?;
    let incoherent_feature_content = ctx
        .admit_iter(
            &ir.model.features[..],
            "scan SLDPRT append_design_losses values",
        )?
        .try_fold(0_usize, |count, candidate| {
            let feature = &candidate;
            Ok::<_, cadmpeg_core::CodecError>(
                count
                    + usize::from({
                        ctx.any_by(
                            &feature.source_content,
                            |content| {
                                const CONTENT: &str = "check SLDPRT feature source content";
                                Ok(match content {
                                    FeatureSourceContent::Text(_) => false,
                                    FeatureSourceContent::Parameter(parameter) => {
                                        match ctx.get_btree_map(
                                            &(parameter_owners),
                                            parameter,
                                            "look up SLDPRT ordered key",
                                        )? {
                                            Some(Some(owner)) => {
                                                !ctx.equal(owner, &feature.id, CONTENT)?
                                            }
                                            _ => true,
                                        }
                                    }
                                    FeatureSourceContent::Feature(child) => match ctx
                                        .get_btree_map(
                                            &(features_by_id),
                                            child,
                                            "look up SLDPRT ordered key",
                                        )? {
                                        Some(child) => {
                                            child.ordinal <= feature.ordinal
                                                || match feature_parent(
                                                    ctx,
                                                    ir,
                                                    &tree_parents,
                                                    &child.id,
                                                )? {
                                                    Some(parent) => {
                                                        !ctx.equal(parent, &feature.id, CONTENT)?
                                                    }
                                                    None => true,
                                                }
                                        }
                                        None => true,
                                    },
                                })
                            },
                            "scan SLDPRT append_design_losses values",
                        )?
                    }),
            )
        })?;
    if incoherent_feature_content > 0 {
        push_report_loss(ctx, report, SldprtLossCode::FeatureIncoherentContent.note(format!(
                "{incoherent_feature_content} feature record(s) contain missing, repeated, misowned, or structurally inconsistent source-content references."
            )))?;
    }

    let mut unresolved_output_scopes = 0_usize;
    for state in ctx.admit_iter(
        &evaluated_feature_states,
        "scan SLDPRT append_design_losses values",
    )? {
        if !state.outputs.is_empty() {
            continue;
        }
        if let Some(scope) = ctx.get_btree_map(
            &state.feature.source_properties,
            "Scope",
            "check SLDPRT feature output scopes",
        )? {
            if ctx.any_by(
                scope.chars(),
                |character| Ok(!character.is_whitespace()),
                "check SLDPRT feature output scopes",
            )? {
                unresolved_output_scopes += 1;
            }
        }
    }
    if unresolved_output_scopes > 0 {
        push_report_loss(ctx, report, SldprtLossCode::FeatureUnresolvedOutputScope.note(format!(
                "{unresolved_output_scopes} feature(s) retain non-empty native output scopes that do not resolve to model bodies."
            )))?;
    }
    let mut incoherent_feature_outputs = 0;
    for state in ctx.admit_iter(
        &evaluated_feature_states,
        "scan SLDPRT append_design_losses values",
    )? {
        if has_incoherent_refs(
            ctx,
            state.outputs,
            &model_body_ids,
            "check SLDPRT feature output body references",
        )? {
            incoherent_feature_outputs += 1;
        }
    }
    if incoherent_feature_outputs > 0 {
        push_report_loss(ctx, report, SldprtLossCode::FeatureIncoherentOutputs.note(format!(
                "{incoherent_feature_outputs} feature record(s) contain missing or repeated output body references."
            )))?;
    }

    let native_planar_constraints = ctx
        .admit_iter(
            &ir.model.sketch_constraints[..],
            "scan SLDPRT append_design_losses values",
        )?
        .filter(|constraint| {
            !sketch_constraint_has_complete_neutral_semantics(constraint.definition.kind())
                && constraint.active != Some(false)
        })
        .count();
    let native_spatial_constraints = ctx
        .admit_iter(
            &ir.model.spatial_sketch_constraints[..],
            "scan SLDPRT append_design_losses values",
        )?
        .filter(|constraint| {
            !spatial_sketch_constraint_has_complete_neutral_semantics(constraint.definition.kind())
        })
        .count();
    let native_constraints = native_planar_constraints + native_spatial_constraints;
    if native_constraints > 0 {
        push_report_loss(ctx, report, SldprtLossCode::SketchNativeConstraint.note(format!(
                "{native_constraints} planar or spatial sketch constraint(s) retain native relation kinds and operands without complete neutral geometric semantics."
            )))?;
    }

    let native_sketch_geometry = ctx
        .admit_iter(
            &ir.model.sketch_entities[..],
            "scan SLDPRT append_design_losses values",
        )?
        .filter(|entity| {
            matches!(
                *entity.geometry.definition(),
                SketchGeometryDefinition::Native { .. }
            )
        })
        .count()
        + ctx
            .admit_iter(
                &ir.model.spatial_sketch_entities[..],
                "scan SLDPRT append_design_losses values",
            )?
            .filter(|entity| {
                matches!(
                    *entity.geometry.definition(),
                    SpatialSketchGeometryDefinition::Native { .. }
                )
            })
            .count();
    if native_sketch_geometry > 0 {
        push_report_loss(ctx, report, SldprtLossCode::SketchNativeGeometry.note(format!(
                "{native_sketch_geometry} sketch entity geometry record(s) retain native kinds without solved neutral geometry."
            )))?;
    }

    let unprojected_relations = match native.as_ref() {
        Some(native) => unprojected_sketch_relation_records(ctx, ir, native)?,
        None => 0,
    };
    if unprojected_relations > 0 {
        push_report_loss(ctx, report, SldprtLossCode::SketchRelationUnprojected.note(format!(
                "{unprojected_relations} native sketch relation record(s) have no projected neutral constraint."
            )))?;
    }
    let multiply_projected_relations = match native.as_ref() {
        Some(native) => multiply_projected_sketch_relation_records(ctx, ir, native)?,
        None => 0,
    };
    if multiply_projected_relations > 0 {
        push_report_loss(ctx, report, SldprtLossCode::SketchRelationMultiplyProjected.note(format!(
                "{multiply_projected_relations} native sketch relation record(s) are claimed by multiple neutral objects."
            )))?;
    }

    let native_features = ctx
        .admit_iter(
            &evaluated_feature_states[..],
            "scan SLDPRT append_design_losses values",
        )?
        .filter(|state| native_feature_has_operation_evidence(state))
        .count();
    if native_features > 0 {
        push_report_loss(ctx, report, SldprtLossCode::FeatureNativeKindRetained.note(format!(
                "{native_features} feature(s) retain their native kind without a complete neutral operation definition."
            )))?;
    }
    let unbound_feature_input_objects = match native.as_ref() {
        Some(native) => unbound_feature_input_operation_objects(ctx, native)?,
        None => 0,
    };
    if unbound_feature_input_objects > 0 {
        push_report_loss(ctx, report, SldprtLossCode::FeatureInputObjectUnbound.note(format!(
                "{unbound_feature_input_objects} native feature-input operation object(s) do not bind uniquely to a history feature."
            )))?;
    }

    let incomplete_edge_selection = |selection: &EdgeSelection| match selection {
        EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. } => edges.is_empty(),
        EdgeSelection::Historical { .. } => false,
        EdgeSelection::HistoricalPartial { .. } => true,
        EdgeSelection::Generated { .. } => false,
        EdgeSelection::All => false,
        EdgeSelection::Unresolved | EdgeSelection::Native(_) => true,
    };
    let incomplete_face_selection = |selection: &FaceSelection| match selection {
        FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => faces.is_empty(),
        FaceSelection::Historical { .. } => false,
        FaceSelection::HistoricalPartial { .. } => true,
        FaceSelection::Generated { .. } => false,
        FaceSelection::Unresolved | FaceSelection::Native(_) => true,
    };
    let incomplete_optional_face_selection = |selection: &FaceSelection| match selection {
        FaceSelection::Faces(_) | FaceSelection::Resolved { .. } => false,
        FaceSelection::Historical { .. } => false,
        FaceSelection::HistoricalPartial { .. } => true,
        FaceSelection::Generated { .. } => false,
        FaceSelection::Unresolved | FaceSelection::Native(_) => true,
    };
    let incomplete_body_selection = |selection: &BodySelection| match selection {
        BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => bodies.is_empty(),
        BodySelection::Historical { bodies, .. } => bodies.is_empty(),
        BodySelection::ResolvedSet { .. } | BodySelection::HistoricalSet { .. } => false,
        BodySelection::Generated { bodies, .. } => bodies.is_empty(),
        BodySelection::Local { bodies, .. } => bodies.is_empty(),
        BodySelection::Unresolved | BodySelection::Native(_) | BodySelection::NativeSet(_) => true,
    };
    let incomplete_planar_profile = |profile: &PlanarProfileRef| match profile {
        PlanarProfileRef::Faces(faces) => faces.is_empty(),
        PlanarProfileRef::Unresolved(_) | PlanarProfileRef::Native(_) => true,
        PlanarProfileRef::Generated { .. }
        | PlanarProfileRef::SketchProfiles { .. }
        | PlanarProfileRef::SketchRegions { .. }
        | PlanarProfileRef::SketchEntities { .. }
        | PlanarProfileRef::SketchSelection { .. }
        | PlanarProfileRef::HistoricalFaces { .. }
        | PlanarProfileRef::Sketch(_)
        | PlanarProfileRef::Feature(_) => false,
    };
    let incomplete_profile = |profile: &ProfileRef| match profile {
        ProfileRef::SpatialSketchProfiles { .. } | ProfileRef::SpatialSketchSelection { .. } => {
            false
        }
        ProfileRef::Planar(profile) => incomplete_planar_profile(profile),
    };
    let incomplete_path = |path: &PathRef| match path {
        PathRef::Edges(edges) => edges.is_empty(),
        PathRef::Curves(curves) => curves.is_empty(),
        PathRef::HistoricalEdges { .. } => false,
        PathRef::SpatialSketchSelection { .. } => false,
        PathRef::Unresolved(_) | PathRef::Native(_) => true,
        PathRef::Sketch(_) => false,
        PathRef::SketchCurves { .. } => false,
        PathRef::SpatialSketchCurves { .. } => false,
    };
    let incomplete_vertex_selection = |selection: &cadmpeg_ir::features::VertexSelection| {
        matches!(
            selection,
            cadmpeg_ir::features::VertexSelection::Unresolved
                | cadmpeg_ir::features::VertexSelection::Native(_)
        )
    };
    let incomplete_linear_termination = |termination: &LinearTermination| match termination {
        LinearTermination::Unresolved {} => true,
        LinearTermination::ToFace { face, .. }
        | LinearTermination::OffsetFromFace { face, .. }
        | LinearTermination::ToShape { target: face } => incomplete_face_selection(face),
        LinearTermination::ToVertex { vertex } => incomplete_vertex_selection(vertex),
        LinearTermination::Blind { .. }
        | LinearTermination::ThroughAll {}
        | LinearTermination::ThroughNext {}
        | LinearTermination::ToFirst {}
        | LinearTermination::ToLast {} => false,
    };
    let incomplete_angular_termination = |termination: &AngularTermination| match termination {
        AngularTermination::Unresolved {} => true,
        AngularTermination::ToFace { face, .. }
        | AngularTermination::OffsetFromFace { face, .. }
        | AngularTermination::ToShape { target: face } => incomplete_face_selection(face),
        AngularTermination::ToVertex { vertex } => incomplete_vertex_selection(vertex),
        AngularTermination::ThroughAll {}
        | AngularTermination::ThroughNext {}
        | AngularTermination::ToFirst {}
        | AngularTermination::ToLast {}
        | AngularTermination::Angle { .. } => false,
    };
    let incomplete_extrude_extent = |extent: &ExtrudeExtent| match extent {
        ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => {
            incomplete_linear_termination(&side.termination)
        }
        ExtrudeExtent::TwoSided { first, second } => {
            incomplete_linear_termination(&first.termination)
                || incomplete_linear_termination(&second.termination)
        }
    };
    let incomplete_revolve_extent = |extent: &RevolveExtent| match extent {
        RevolveExtent::OneSided { termination } | RevolveExtent::Symmetric { termination } => {
            incomplete_angular_termination(termination)
        }
        RevolveExtent::TwoSided { first, second } => {
            incomplete_angular_termination(first) || incomplete_angular_termination(second)
        }
    };
    let mut occurrence_ids = None;
    let mut occurrence_storage = ctx.reserve_scoped(0, "SLDPRT occurrence identity lookup")?;
    let mut joint_ids = None;
    let mut joint_storage = ctx.reserve_scoped(0, "SLDPRT joint identity lookup")?;
    let mut asset_ids = None;
    let mut asset_storage = ctx.reserve_scoped(0, "SLDPRT asset identity lookup")?;
    let incomplete_typed_features = ctx.admit_iter(&evaluated_feature_states[..], "scan SLDPRT append_design_losses values")?.try_fold(0_usize, |count, candidate| { let state = &candidate; Ok::<_, cadmpeg_core::CodecError>(count + usize::from( {
            let definition = state.definition.operation();
            match definition {
            FeatureOperation::TreeNode { .. }
            | FeatureOperation::DatumPrincipalPlane { .. }
            | FeatureOperation::DatumPlane { .. }
            | FeatureOperation::DatumThreePointPlane { .. }
            | FeatureOperation::DatumAxis { .. }
            | FeatureOperation::DatumPoint { .. }
            | FeatureOperation::DatumCoordinateSystem { .. }
            | FeatureOperation::EquationCurve { .. }
            | FeatureOperation::Helix { .. } => false,
            FeatureOperation::BaseFeature { bodies } => incomplete_body_selection(bodies),
            FeatureOperation::InsertBodies { bodies } => !bodies.is_resolved(),
            FeatureOperation::MeshImport { .. } => false,
            FeatureOperation::InsertComponent { occurrence } => {
                if occurrence_ids.is_none() {
                    occurrence_ids = Some(occurrence_storage.with_storage(|| ctx.collect_hash_set(ir.model.occurrences.iter().map(|value| &value.id), "index SLDPRT occurrence identities"))?);
                }
                match &occurrence_ids {
                    Some(ids) => !ctx.contains_hash_set(ids, occurrence, "find SLDPRT feature references")?,
                    None => false,
                }
            },
            FeatureOperation::AssemblyJoint { joint } => {
                if joint_ids.is_none() {
                    joint_ids = Some(joint_storage.with_storage(|| ctx.collect_hash_set(ir.model.assembly_joints.iter().map(|value| &value.id), "index SLDPRT joint identities"))?);
                }
                match &joint_ids {
                    Some(ids) => !ctx.contains_hash_set(ids, joint, "find SLDPRT feature references")?,
                    None => false,
                }
            },
            FeatureOperation::ReferenceImage { asset, .. }
            | FeatureOperation::Decal { asset, .. } => {
                if asset_ids.is_none() {
                    asset_ids = Some(asset_storage.with_storage(|| ctx.collect_hash_set(ir.model.assets.iter().map(|value| &value.id), "index SLDPRT asset identities"))?);
                }
                match &asset_ids {
                    Some(ids) => !ctx.contains_hash_set(ids, asset, "find SLDPRT feature references")?,
                    None => false,
                }
            },
            FeatureOperation::StoredGeometry {} => state.outputs.is_empty(),
            FeatureOperation::ExtractBody { source } => incomplete_body_selection(source),
            FeatureOperation::DerivedGeometry { source } => {
                ctx.get_btree_map(feature_positions, source, "look up SLDPRT ordered key")?
                    .is_none_or(|ordinal| *ordinal >= state.feature.ordinal)
                    || !ctx.contains(state.dependencies.as_slice(), source, "check SLDPRT derived geometry dependency")?
            }
            FeatureOperation::ImportedGeometry { path, .. } => !ctx.any_by(path.chars(), |character| Ok(!character.is_whitespace()), "check SLDPRT imported geometry path")?,
            FeatureOperation::Form { cages } => cages.is_empty(),
            FeatureOperation::PointGeometry { .. }
            | FeatureOperation::LineSegment { .. }
            | FeatureOperation::CircularArc { .. }
            | FeatureOperation::EllipticArc { .. }
            | FeatureOperation::PlanarPatch { .. } => false,
            FeatureOperation::Polyline { .. } => false,
            FeatureOperation::RegularPolygonCurve { .. } => false,
            FeatureOperation::FaceFromShapes { sources, .. } => incomplete_body_selection(sources),
            FeatureOperation::Block {
                dimensions,
                placement,
                ..
            } => dimensions.is_none() || placement.is_none(),
            FeatureOperation::ProjectOnSurface {
                sources,
                support_face,
                ..
            } => incomplete_path(sources) || incomplete_face_selection(support_face),
            FeatureOperation::Coil {
                construction,
                result,
            } => {
                matches!(
                    construction.placement,
                    cadmpeg_ir::features::CoilPlacement::Native { .. }
                ) || match result {
                    cadmpeg_ir::features::CoilResult::NewBody {} => false,
                    cadmpeg_ir::features::CoilResult::Boolean { targets, .. } => {
                        incomplete_body_selection(targets)
                    }
                }
            }
            FeatureOperation::Sphere { op, .. }
            | FeatureOperation::Torus { op, .. }
            | FeatureOperation::Primitive { op, .. } => *op == BooleanOp::Unresolved,
            FeatureOperation::CosmeticThread {
                face,
                diameter,
                extent,
            } => {
                incomplete_face_selection(face)
                    || diameter.is_none()
                    || extent.is_none()
            }
            FeatureOperation::SketchBlockDefinition { sketch } => sketch.is_none(),
            FeatureOperation::SketchBlockInstance { block, placement } => {
                block.is_none() || placement.is_none()
            }
            FeatureOperation::DatumOffsetPlane { reference, .. } => reference
                .as_ref()
                .is_none_or(|reference| match reference {
                    cadmpeg_ir::features::DatumPlaneReference::Feature { .. } => false,
                    cadmpeg_ir::features::DatumPlaneReference::Face { face } => {
                        incomplete_face_selection(face)
                    }
                    cadmpeg_ir::features::DatumPlaneReference::ResolvedPlane { .. } => false,
                }),
            FeatureOperation::ProjectedCurve {
                source,
                target_faces,
                direction,
                bidirectional,
            } => {
                incomplete_path(source)
                    || incomplete_face_selection(target_faces)
                    || matches!(
                        direction,
                        cadmpeg_ir::features::CurveProjectionDirection::State(
                            cadmpeg_ir::features::CurveProjectionDirectionState::Unresolved
                        )
                    )
                    || bidirectional.is_none()
            }
            FeatureOperation::CompositeCurve { segments, .. } => {
                segments.is_empty() || ctx.any_by(&segments[..], |value| Ok(incomplete_path(value)), "scan SLDPRT append_design_losses values")?
            }
            FeatureOperation::HelixNativeAxis { .. } => true,
            FeatureOperation::Wrap {
                profile, face, ..
            } => {
                incomplete_planar_profile(profile) || incomplete_face_selection(face)
            }
            FeatureOperation::Sketch { sketch, .. } => sketch.id().is_none(),
            FeatureOperation::SpatialSketch { sketch } => sketch.is_none(),
            FeatureOperation::Extrude {
                profile,
                direction,
                start,
                extent,
                op,
                ..
            } => {
                incomplete_profile(profile)
                    || matches!(direction, cadmpeg_ir::features::ExtrudeDirection::Unresolved {})
                    || match start {
                        cadmpeg_ir::features::ExtrudeStart::Unresolved {} => true,
                        cadmpeg_ir::features::ExtrudeStart::FromFace { face, .. } => {
                            incomplete_face_selection(face)
                        }
                        cadmpeg_ir::features::ExtrudeStart::ProfilePlane {}
                        | cadmpeg_ir::features::ExtrudeStart::OffsetProfilePlane { .. } => false,
                    }
                    || matches!(
                        direction,
                        cadmpeg_ir::features::ExtrudeDirection::Explicit {
                            source: Some(cadmpeg_ir::features::ExtrusionDirectionSource::Edge { reference }),
                            ..
                        }
                            if incomplete_path(reference)
                    )
                    || incomplete_extrude_extent(extent)
                    || *op == BooleanOp::Unresolved
            }
            FeatureOperation::Revolve { construction, op } => {
                match construction {
                    cadmpeg_ir::features::RevolveConstruction::Unresolved(_) => true,
                    cadmpeg_ir::features::RevolveConstruction::Resolved {
                        profile, extent, ..
                    } => {
                        incomplete_planar_profile(profile)
                            || incomplete_revolve_extent(extent)
                            || *op == BooleanOp::Unresolved
                    }
                }
            }
            FeatureOperation::Sweep {
                shape,

                path,

                orientation,
                ..
            } => {
                let mode = shape.mode();
                (match shape {
                    cadmpeg_ir::features::SweepShape::Unresolved { section, sections }
                    | cadmpeg_ir::features::SweepShape::Surface { section, sections } => {
                        section.is_unresolved() || ctx.any_by(sections, |section| Ok(section.is_unresolved()), "check SLDPRT sweep sections")?
                    }
                    cadmpeg_ir::features::SweepShape::Solid { section, sections, .. } => {
                        section.is_unresolved() || ctx.any_by(sections, |section| Ok(section.is_unresolved()), "check SLDPRT sweep sections")?
                    }
                }) || (match shape {
                    cadmpeg_ir::features::SweepShape::Unresolved { section, sections }
                    | cadmpeg_ir::features::SweepShape::Surface { section, sections } => {
                        section.referenced_profile().is_some_and(incomplete_planar_profile)
                            || ctx.any_by(sections, |section| Ok(section.referenced_profile().is_some_and(incomplete_planar_profile)), "check SLDPRT sweep profiles")?
                    }
                    cadmpeg_ir::features::SweepShape::Solid { section, sections, .. } => {
                        section.referenced_profile().is_some_and(incomplete_planar_profile)
                            || ctx.any_by(sections, |section| Ok(section.referenced_profile().is_some_and(incomplete_planar_profile)), "check SLDPRT sweep profiles")?
                    }
                })
                    || path.as_ref().is_none_or(incomplete_path)
                    || matches!(
                        orientation,
                        Some(cadmpeg_ir::features::SweepOrientation::Auxiliary { path, .. })
                            if incomplete_path(path)
                    )
                    || matches!(mode, cadmpeg_ir::features::SweepMode::Unresolved {})
            }
            FeatureOperation::HelicalSweep { construction, op } => {
                incomplete_planar_profile(&construction.profile) || *op == BooleanOp::Unresolved
            }
            FeatureOperation::Binder {
                sources,
                construction,
            } => {
                sources.is_empty()
                    || ctx.any_by(sources, |source| { Ok::<_, CodecError>({
                        incomplete_binder_target(ctx,
                            &source.target,
                            feature_positions,
                            state.feature.ordinal,
                            state.dependencies,
                        )? || ctx.any_by(
                            &source.subelements,
                            |subelement| {
                                Ok(!ctx.any_by(subelement.as_str().chars(), |character| Ok(!character.is_whitespace()), "check SLDPRT binder subelements")?)
                            },
                            "check SLDPRT binder subelements",
                        )?
                    }) }, "scan SLDPRT binder sources")?
                    || matches!(
                        construction,
                        cadmpeg_ir::features::BinderConstruction::Shape { .. }
                            if sources.len() != 1
                    )
                    || matches!(
                        construction,
                        cadmpeg_ir::features::BinderConstruction::SubShape {
                            context: Some(context),
                            ..
                        } if incomplete_binder_target(ctx,
                            context,
                            feature_positions,
                            state.feature.ordinal,
                            state.dependencies,
                        )?
                    )
            }
            FeatureOperation::Loft {
                sections,
                guidance,
                op,
                ..
            } => {
                sections.len() < 2
                    || ctx.any_by(&sections[..], |section| Ok(match section {
                        cadmpeg_ir::features::LoftSection::Profile(profile) => incomplete_profile(profile),
                        cadmpeg_ir::features::LoftSection::Point(cadmpeg_ir::features::LoftPointSection::Native(_)) => true,
                        cadmpeg_ir::features::LoftSection::Point(_) => false,
                    }), "scan SLDPRT append_design_losses values")?
                    || match guidance {
                        cadmpeg_ir::features::LoftGuidance::Guides(guides) => {
                            ctx.any_by(&guides[..], |value| Ok(incomplete_path(value)), "scan SLDPRT append_design_losses values")?
                        }
                        cadmpeg_ir::features::LoftGuidance::Centerline(centerline) => {
                            incomplete_path(centerline)
                        }
                    }
                    || *op == BooleanOp::Unresolved
            }
            FeatureOperation::Rib { construction, op } => {
                construction
                    .profile
                    .as_ref()
                    .is_none_or(incomplete_planar_profile)
                    || construction.direction.is_none()
                    || construction.thickness.is_none()
                    || construction.side.is_none()
                    || matches!(construction.draft, cadmpeg_ir::features::RibDraft::Unresolved)
                    || *op == BooleanOp::Unresolved
            }
            FeatureOperation::SheetMetalBaseFlange { profile, .. } => {
                incomplete_planar_profile(profile)
            }
            FeatureOperation::SheetMetalEdgeFlange { edges, .. } => {
                incomplete_edge_selection(edges)
            }
            FeatureOperation::SheetMetalHem { .. } => true,
            FeatureOperation::Fillet { groups } => {
                groups.is_empty()
                    || ctx.any_by(&groups[..], |group| Ok({
                        incomplete_edge_selection(&group.edges)
                            || group.radius.is_unresolved()
                    }), "scan SLDPRT append_design_losses values")?
            }
            FeatureOperation::FullRoundFillet { groups } => {
                groups.is_empty()
                    || ctx.any_by(&groups[..], |group| Ok({
                        incomplete_face_selection(group.center_faces())
                            || matches!(
                                group.side_one_faces(),
                                cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Unresolved
                            )
                            || matches!(
                                group.side_two_faces(),
                                cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Unresolved
                            )
                            || matches!(
                                group.side_one_faces(),
                                cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Explicit(
                                    ref selection
                                ) if incomplete_face_selection(selection)
                            )
                            || matches!(
                                group.side_two_faces(),
                                cadmpeg_ir::features::edge_treatments::FullRoundSideSelection::Explicit(
                                    ref selection
                                ) if incomplete_face_selection(selection)
                            )
                    }), "scan SLDPRT append_design_losses values")?
            }
            FeatureOperation::Chamfer { groups, .. } => groups.is_empty() || ctx.any_by(&groups[..], |group| Ok({
                incomplete_edge_selection(&group.edges) || group.spec.is_unresolved()
            }), "scan SLDPRT append_design_losses values")?,
            FeatureOperation::FaceBlend {
                operands,

                radius,
            } => {
                let first_faces = operands.first_faces();
                let second_faces = operands.second_faces();
                incomplete_face_selection(first_faces)
                    || incomplete_face_selection(second_faces)
                    || radius.is_unresolved()
            }
            FeatureOperation::Shell {
                bodies,
                removed_faces,
                thickness,
                outward,
                mode,
                join,
                resolve_intersections,
                allow_self_intersections,
            } => {
                bodies.as_ref().is_some_and(incomplete_body_selection)
                    || incomplete_optional_face_selection(removed_faces)
                    || thickness.is_none()
                    || outward.is_none()
                    || mode.is_none()
                    || join.is_none()
                    || resolve_intersections.is_none()
                    || allow_self_intersections.is_none()
            }
            FeatureOperation::OffsetShape { source, .. }
            | FeatureOperation::RefineShape { source }
            | FeatureOperation::ReverseShape { source } => incomplete_body_selection(source),
            FeatureOperation::Compound { members } => incomplete_body_selection(members),
            FeatureOperation::RuledBetweenCurves { first, second, .. } => {
                incomplete_path(first) || incomplete_path(second)
            }
            FeatureOperation::SectionShape {
                operands,

                approximate,
            } => {
                let first = operands.first();
                let second = operands.second();
                incomplete_body_selection(first)
                    || incomplete_body_selection(second)
                    || approximate.is_none()
            }
            FeatureOperation::MirrorShape {
                source,
                plane_reference,
                ..
            } => {
                incomplete_body_selection(source)
                    || plane_reference
                        .as_ref()
                        .is_some_and(incomplete_face_selection)
            }
            FeatureOperation::Thicken {
                faces,
                thickness,
                side,
            } => incomplete_face_selection(faces) || thickness.is_none() || side.is_none(),
            FeatureOperation::OffsetSurface { faces, distance } => {
                incomplete_face_selection(faces) || distance.is_none()
            }
            FeatureOperation::KnitSurface {
                faces,
                merge_entities,
                create_solid,
                ..
            } => {
                incomplete_face_selection(faces)
                    || merge_entities.is_none()
                    || create_solid.is_none()
            }
            FeatureOperation::ExtendSurface {
                faces,
                distance,
                method,
            } => {
                incomplete_face_selection(faces)
                    || distance.is_none()
                    || *method == cadmpeg_ir::features::SurfaceExtension::Unresolved
            }
            FeatureOperation::FilledSurface {
                boundary,
                support_faces,
                continuity,
                merge_result,
                ..
            } => {
                (match boundary {
                    cadmpeg_ir::features::SurfaceBoundary::Edges(edges) => incomplete_edge_selection(edges),
                    cadmpeg_ir::features::SurfaceBoundary::Path(path) => incomplete_path(path),
                })
                    || if continuity.uniform_value()
                        == Some(cadmpeg_ir::features::SurfaceContinuity::Contact)
                    {
                        incomplete_optional_face_selection(support_faces)
                    } else {
                        incomplete_face_selection(support_faces)
                    }
                    || continuity.is_unresolved()
                    || merge_result.is_none()
            }
            FeatureOperation::TrimSurface {
                faces,
                tool,
                keep,
                ..
            } => {
                incomplete_face_selection(faces)
                    || incomplete_path(tool)
                    || matches!(keep, cadmpeg_ir::features::TrimRegion::Unresolved)
            }
            FeatureOperation::RuledSurface {
                edges,
                support_faces,
                mode,
                ..
            } => {
                incomplete_edge_selection(edges)
                    || if matches!(mode, cadmpeg_ir::features::RuledSurfaceMode::Direction { .. }) {
                        incomplete_optional_face_selection(support_faces)
                    } else {
                        incomplete_face_selection(support_faces)
                    }
            }
            FeatureOperation::Draft {
                faces,
                anchor,
                angle,
                outward,
            } => {
                incomplete_face_selection(faces)
                    || match anchor {
                        cadmpeg_ir::features::DraftAnchor::NeutralPlane { plane, .. } => {
                            incomplete_face_selection(plane)
                        }
                        cadmpeg_ir::features::DraftAnchor::PartingLine { tool, .. } => {
                            incomplete_face_selection(tool)
                        }
                    }
                    || anchor.pull().is_none()
                    || angle.is_none()
                    || (matches!(
                        anchor,
                        cadmpeg_ir::features::DraftAnchor::NeutralPlane { .. }
                    ) && outward.is_none())
            }
            FeatureOperation::Combine { operands,  .. } => {
                let target = operands.target();
                let tools = operands.tools();
                incomplete_body_selection(target) || incomplete_body_selection(tools)
            }
            FeatureOperation::BoundaryFill { tools, cells } => {
                incomplete_body_selection(tools)
                    || ctx.any_by(cells, |cell| Ok(incomplete_body_selection(cell)), "check SLDPRT boundary fill cells")?
            }
            FeatureOperation::CutWithSurface { targets, tools, .. } => {
                incomplete_body_selection(targets) || incomplete_face_selection(tools)
            }
            FeatureOperation::TrimBodies {
                operands,

                keep,
            } => {
                let targets = operands.targets();
                let tools = operands.tools();
                incomplete_body_selection(targets)
                    || incomplete_body_selection(tools)
                    || *keep == cadmpeg_ir::features::BodyTrimSide::Unresolved
            }
            FeatureOperation::SplitBody { targets, tools } => {
                incomplete_body_selection(targets) || incomplete_face_selection(tools)
            }
            FeatureOperation::SplitFace { targets, tool } => {
                incomplete_face_selection(targets)
                    || match tool {
                        SplitFaceTool::Path(path) => incomplete_path(path),
                        SplitFaceTool::Plane { .. } | SplitFaceTool::Planes { .. } => false,
                    }
            }
            FeatureOperation::SewBodies {
                bodies,
                gap_tolerance,
            } => incomplete_body_selection(bodies) || gap_tolerance.is_none(),
            FeatureOperation::DeleteBody { bodies, mode } => {
                incomplete_body_selection(bodies) || *mode == BodyRetentionMode::Unresolved
            }
            FeatureOperation::DeleteFace { faces, .. } => incomplete_face_selection(faces),
            FeatureOperation::ReplaceFace {
                operands,

            } => {
            let targets = operands.targets();
            let replacements = operands.replacements();
incomplete_face_selection(targets) || incomplete_face_selection(replacements)},
            FeatureOperation::MoveFace { faces, .. } => incomplete_face_selection(faces),
            FeatureOperation::MoveBody { bodies, .. } => incomplete_body_selection(bodies),
            FeatureOperation::Dome {
                faces,
                height,
                elliptical,
                reverse,
            } => {
                incomplete_face_selection(faces)
                    || height.is_none()
                    || elliptical.is_none()
                    || reverse.is_none()
            }
            FeatureOperation::Flex { axis, mode } => {
                axis.is_none()
                || matches!(mode, cadmpeg_ir::features::FlexMode::Unresolved { .. })
            }
            FeatureOperation::Scale {
                bodies,
                center,
                factors,
            } => {
                incomplete_body_selection(bodies)
                    || center.as_ref().is_none_or(|center| {
                        matches!(center, cadmpeg_ir::features::ScaleCenter::Native(_))
                    })
                    || factors.resolved().is_none()
            }
            FeatureOperation::Hole {
                profile,
                face,
                placements,
                shape,
                extent,
                ..
            } => {
                let construction = shape.construction();
                let exit_kind = shape.exit_kind();
                let diameter = shape.diameter();
                let exit_kind_is_unresolved = exit_kind
                    .as_ref()
                    .is_some_and(cadmpeg_ir::features::holes::HoleKind::is_unresolved);
                profile.as_ref().is_some_and(incomplete_planar_profile)
                    || face.as_ref().is_some_and(incomplete_face_selection)
                    || placements.is_none()
                    || matches!(
                        construction,
                        cadmpeg_ir::features::holes::HoleConstruction::Form { kind, .. }
                            if kind.is_unresolved()
                    )
                    || exit_kind_is_unresolved
                    || diameter.is_none()
                    || extent
                        .as_ref()
                        .is_none_or(incomplete_linear_termination)
            }
            FeatureOperation::Pattern { seeds, pattern } => {
                seeds.is_empty()
                    || ctx.any_by(&seeds[..], |seed| Ok(match seed {
                        cadmpeg_ir::features::patterns::PatternSeed::Feature(_) => false,
                        cadmpeg_ir::features::patterns::PatternSeed::Faces(faces) => {
                            incomplete_face_selection(faces)
                        }
                        cadmpeg_ir::features::patterns::PatternSeed::Bodies(bodies) => {
                            incomplete_body_selection(bodies)
                        }
                        cadmpeg_ir::features::patterns::PatternSeed::Occurrences(occurrences) => {
                            occurrences.is_empty()
                        }
                    }), "scan SLDPRT append_design_losses values")?
                    || incomplete_pattern(ctx, pattern, &incomplete_path)?
            }
            FeatureOperation::Native { .. } => false,
            // Unresolved construction retained as native.
            FeatureOperation::Unresolved { .. } => true,
            }
        } )) })?;
    if incomplete_typed_features > 0 {
        push_report_loss(ctx, report, SldprtLossCode::FeatureTypedOperandIncomplete.note(format!(
            "{incomplete_typed_features} typed feature(s) retain native or unresolved required operation operands."
        )))?;
    }

    let unresolved_body_modes = ctx
        .admit_iter(
            &evaluated_feature_states[..],
            "scan SLDPRT append_design_losses values",
        )?
        .filter(|state| {
            matches!(
                state.definition,
                FeatureDefinition::Operation(FeatureOperation::DeleteBody {
                    mode: BodyRetentionMode::Unresolved,
                    ..
                })
            )
        })
        .count();
    if unresolved_body_modes > 0 {
        push_report_loss(ctx, report, SldprtLossCode::FeatureBodyRetentionUnresolved.note(format!(
            "{unresolved_body_modes} body delete/keep feature(s) retain selected native body identities without a decoded retention mode."
        )))?;
    }
    Ok(())
}

fn configuration_source_needs_update(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    configuration: &cadmpeg_ir::features::DesignConfiguration,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "check SLDPRT configuration update state";
    let parsed = match ctx.get_btree_map(&configuration.properties, "id", OPERATION)? {
        Some(value) => ctx.parse_text::<u32>(value, OPERATION)?.ok(),
        None => None,
    };
    let slot = parsed
        .or(configuration.source_index)
        .unwrap_or(configuration.ordinal);
    let Some(source) = &ir.source else {
        return Ok(false);
    };
    let (key, _storage) = ctx.format_scoped(
        format_args!("sw_configuration_{slot}_needs_update"),
        OPERATION,
    )?;
    Ok(
        match ctx.get_btree_map(&source.attributes, key.as_str(), OPERATION)? {
            Some(value) => value.eq_ignore_ascii_case("yes"),
            None => false,
        },
    )
}

fn unbound_feature_input_operation_objects(
    ctx: &DecodeContext<'_>,
    native: &crate::native::SldprtNative,
) -> Result<usize, CodecError> {
    use crate::classification::{classify, native_object_class, FeatureClass};
    use crate::records::FeatureInputClassRole;
    const OPERATION: &str = "count SLDPRT unbound feature-input operation objects";
    fn increment<K: Ord + cadmpeg_core::decode::cost::DecodeCost>(
        ctx: &DecodeContext<'_>,
        counts: &mut BTreeMap<K, usize>,
        key: K,
    ) -> Result<(), CodecError> {
        let count = ctx.entry_btree_map(counts, key, OPERATION)?.or_insert(0);
        *count = count
            .checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        Ok(())
    }

    let mut workspace = ctx.reserve_scoped(0, OPERATION)?;
    // One pass over the history records: how often each source and each
    // source-class pair occurs, and the operation class of each classless
    // record by source.
    let mut source_counts = BTreeMap::<u32, usize>::new();
    let mut binding_counts = BTreeMap::<(u32, &str), usize>::new();
    let mut classless_kinds = HashMap::<u32, Vec<FeatureClass>>::new();
    for history in ctx.admit_iter(&native.feature_histories, OPERATION)? {
        for feature in ctx.admit_iter(&history.features, OPERATION)? {
            let Some(source) = feature.source_value() else {
                continue;
            };
            workspace.with_storage(|| {
                increment(ctx, &mut source_counts, source)?;
                match feature.input_class.as_deref() {
                    Some(class) => increment(ctx, &mut binding_counts, (source, class)),
                    None => match classify(feature) {
                        Some(kind) => ctx.push_hash_group(
                            &mut classless_kinds,
                            source,
                            kind,
                            OPERATION,
                            OPERATION,
                        ),
                        None => Ok(()),
                    },
                }
            })?;
        }
    }
    let mut named_binding_counts = BTreeMap::<(&str, &str, &str), usize>::new();
    for lane in ctx.admit_iter(&native.feature_input_lanes, OPERATION)? {
        let mut object_names = None;
        for history in ctx.admit_iter(&native.feature_histories, OPERATION)? {
            for feature in ctx.admit_iter(&history.features, OPERATION)? {
                let Some(class) = feature.input_class.as_deref() else {
                    continue;
                };
                let names = match &object_names {
                    Some(names) => names,
                    None => object_names.insert(crate::resolved_features::scalars::ObjectNames::new(ctx, lane)?),
                };
                let name = names.of(ctx, feature)?;
                if let Some(name) = name {
                    workspace.with_storage(|| {
                        increment(
                            ctx,
                            &mut named_binding_counts,
                            (lane.id.as_str(), name.id.as_str(), class),
                        )
                    })?;
                }
            }
        }
    }
    let mut count = 0_usize;
    for lane in ctx.admit_iter(&native.feature_input_lanes, OPERATION)? {
        // The first name at an offset answers, so the index is filled in
        // reverse and the last write wins.
        let mut names_by_offset = HashMap::new();
        for name in ctx.admit_iter(&lane.names, OPERATION)?.rev() {
            workspace.with_storage(|| {
                ctx.insert_hash_map(&mut names_by_offset, name.offset, name, OPERATION)
            })?;
        }
        for class in ctx
            .admit_iter(&lane.classes, OPERATION)?
            .filter(|class| class.role() == FeatureInputClassRole::Feature)
        {
            let name_offset = class.offset + 6 + u64_from_index(class.name.len());
            let Some(name) = ctx.get_hash_map(&names_by_offset, &name_offset, OPERATION)? else {
                continue;
            };
            let source_bound = match name.object_id.and_then(ObjectId::value) {
                Some(id) => {
                    ctx.get_btree_map(&source_counts, &id, OPERATION)?.copied() == Some(1)
                        && (ctx
                            .get_btree_map(&binding_counts, &(id, class.name.as_str()), OPERATION)?
                            .copied()
                            == Some(1)
                            || match native_object_class(&class.name).feature() {
                                Some(expected) => {
                                    match ctx.get_hash_map(&classless_kinds, &id, OPERATION)? {
                                        Some(kinds) => ctx.any_by(
                                            kinds,
                                            |kind| Ok(*kind == expected),
                                            OPERATION,
                                        )?,
                                        None => false,
                                    }
                                }
                                None => false,
                            })
                }
                None => false,
            };
            let name_bound = ctx
                .get_btree_map(
                    &named_binding_counts,
                    &(lane.id.as_str(), name.id.as_str(), class.name.as_str()),
                    OPERATION,
                )?
                .copied()
                == Some(1);
            count += usize::from(!(source_bound || name_bound));
        }
    }
    Ok(count)
}

/// The native records every projected sketch constraint and entity names, in
/// model order.
fn projected_native_refs<'ir>(
    ctx: &DecodeContext<'_>,
    ir: &'ir CadIr,
) -> Result<Vec<&'ir str>, CodecError> {
    const OPERATION: &str = "collect SLDPRT projected sketch relations";
    let model = &ir.model;
    ctx.collect_vec(
        ctx.admit_iter(&model.sketch_constraints, OPERATION)?
            .filter_map(|constraint| constraint.native_ref.as_deref())
            .chain(
                ctx.admit_iter(&model.sketch_entities, OPERATION)?
                    .filter_map(|entity| entity.native_ref.as_deref()),
            )
            .chain(
                ctx.admit_iter(&model.spatial_sketch_entities, OPERATION)?
                    .filter_map(|entity| entity.native_ref.as_deref()),
            )
            .chain(
                ctx.admit_iter(&model.spatial_sketch_constraints, OPERATION)?
                    .filter_map(|constraint| constraint.native_ref.as_deref()),
            ),
        OPERATION,
    )
}

fn unprojected_sketch_relation_records(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    native: &crate::native::SldprtNative,
) -> Result<usize, CodecError> {
    let (sketch_feature_refs, _sketch_feature_refs_storage) =
        ctx.with_scoped_storage("SLDPRT sketch_feature_refs workspace", || {
            ctx.collect_hash_set(
                ctx.admit_iter(&ir.model.features, "index SLDPRT sketch feature references")?
                    .filter(|feature| {
                        matches!(
                            feature.evaluation.definition(),
                            cadmpeg_ir::features::FeatureDefinition::Operation(
                                cadmpeg_ir::features::FeatureOperation::Sketch { .. }
                                    | cadmpeg_ir::features::FeatureOperation::SpatialSketch { .. }
                            )
                        )
                    })
                    .filter_map(|feature| feature.native_ref.as_deref()),
                "index SLDPRT sketch feature references",
            )
        })?;
    let (native_refs, _native_refs_storage) = ctx
        .with_scoped_storage("collect SLDPRT projected sketch relations", || {
            projected_native_refs(ctx, ir)
        })?;
    let (projected, _projected_storage) =
        ctx.with_scoped_storage("SLDPRT projected workspace", || {
            ctx.collect_hash_set(
                native_refs.iter().copied(),
                "index SLDPRT projected sketch relations",
            )
        })?;
    let (owned_instances, _ownership_storage) =
        ctx.with_scoped_storage("SLDPRT relation ownership index", || {
            crate::resolved_features::relation_geometry::owned_relation_parameters(
                ctx,
                &ir.model.features,
                &ir.model.parameters,
                native.feature_input_lanes.as_slice(),
            )
        })?;

    let mut total = 0;
    for lane in ctx.admit_iter(
        &native.feature_input_lanes,
        "scan SLDPRT unprojected_sketch_relation_records values",
    )? {
        let lane_markers =
            crate::resolved_features::typed_relations::RelationMarkers::of_lane(ctx, lane)?;
        let instances = ctx
            .admit_iter(
                &lane.relation_instances[..],
                "scan SLDPRT unprojected_sketch_relation_records values",
            )?
            .try_fold(0_usize, |count, candidate| {
                let relation = &candidate;
                Ok::<_, cadmpeg_core::CodecError>(
                    count
                        + usize::from({
                            ctx.contains_hash_set(
                                &(sketch_feature_refs),
                                relation.feature_ref.as_str(),
                                "test SLDPRT hashed identity",
                            )? && ctx.contains_key_hash_map(
                                &(owned_instances),
                                &relation.id,
                                "test SLDPRT map key",
                            )? && !ctx.contains_hash_set(
                                &(projected),
                                relation.id.as_str(),
                                "test SLDPRT hashed identity",
                            )?
                        }),
                )
            })?;
        // Each instance's class with each of its scalars, for the binding check.
        let mut instance_scalars = HashSet::new();
        let mut instance_scalars_storage =
            ctx.reserve_scoped(0, "index SLDPRT relation instance scalars")?;
        for relation in ctx.admit_iter(
            &lane.relation_instances,
            "index SLDPRT relation instance scalars",
        )? {
            for scalar in ctx.admit_iter(
                relation.scalar_refs(),
                "index SLDPRT relation instance scalars",
            )? {
                instance_scalars_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut instance_scalars,
                        (relation.class_ref.as_str(), scalar.as_str()),
                        "index SLDPRT relation instance scalars",
                    )
                })?;
            }
        }
        let mut bindings = 0_usize;
        for binding in ctx.admit_iter(
            &lane.relation_bindings,
            "scan SLDPRT unprojected relation bindings",
        )? {
            let sketch_binding = match binding.feature_ref.as_deref() {
                Some(feature_ref) => ctx.contains_hash_set(
                    &sketch_feature_refs,
                    feature_ref,
                    "scan SLDPRT unprojected relation bindings",
                )?,
                None => false,
            };
            if sketch_binding
                && !ctx.contains_hash_set(
                    &instance_scalars,
                    &(binding.class_ref.as_str(), binding.scalar_ref.as_str()),
                    "scan SLDPRT unprojected relation bindings",
                )?
            {
                bindings += 1;
            }
        }
        let mut markers = 0;
        for marker in ctx.admit_iter(
            &lane.sketch_entities,
            "scan SLDPRT unprojected_sketch_relation_records values",
        )? {
            if match marker.feature_ref.as_deref() {
                Some(feature_ref) => ctx.contains_hash_set(
                    &(sketch_feature_refs),
                    feature_ref,
                    "test SLDPRT hashed identity",
                )?,
                None => false,
            } && crate::resolved_features::typed_relations::marker_owns_constraint_in(
                ctx,
                marker,
                &lane_markers,
            )? && !ctx.contains_hash_set(
                &(projected),
                marker.id(),
                "test SLDPRT hashed identity",
            )? {
                markers += 1;
            }
        }
        total += instances + bindings + markers;
    }
    Ok(total)
}

fn multiply_projected_sketch_relation_records(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    native: &crate::native::SldprtNative,
) -> Result<usize, CodecError> {
    let (result, _workspace) = ctx.with_scoped_storage(
        "SLDPRT relation projection lookup workspace",
        || -> Result<usize, CodecError> {
            let mut native_relation_ids = HashSet::new();
            for lane in ctx.admit_iter(
                &native.feature_input_lanes,
                "scan SLDPRT multiply_projected_sketch_relation_records values",
            )? {
                let markers =
                    crate::resolved_features::typed_relations::RelationMarkers::of_lane(ctx, lane)?;
                for relation in ctx.admit_iter(
                    &lane.relation_instances,
                    "scan SLDPRT multiply_projected_sketch_relation_records values",
                )? {
                    ctx.insert_hash_set(
                        &mut native_relation_ids,
                        relation.id.as_str(),
                        "index SLDPRT native relation IDs",
                    )?;
                }
                for marker in ctx.admit_iter(
                    &lane.sketch_entities,
                    "scan SLDPRT multiply_projected_sketch_relation_records values",
                )? {
                    if crate::resolved_features::typed_relations::marker_owns_constraint_in(
                        ctx, marker, &markers,
                    )? {
                        ctx.insert_hash_set(
                            &mut native_relation_ids,
                            marker.id(),
                            "index SLDPRT native relation IDs",
                        )?;
                    }
                }
            }
            let mut projection_counts = BTreeMap::<&str, usize>::new();
            let (native_refs, _native_refs_storage) = ctx
                .with_scoped_storage("collect SLDPRT projected sketch relations", || {
                    projected_native_refs(ctx, ir)
                })?;
            for &native_ref in ctx.admit_iter(&native_refs, "count SLDPRT relation projections")? {
                const OPERATION: &str = "count SLDPRT relation projections";
                if !ctx.contains_hash_set(
                    &native_relation_ids,
                    native_ref,
                    "test SLDPRT hashed identity",
                )? {
                    continue;
                }
                if let Some(count) = ctx.get_mut_btree_map(
                    &mut (projection_counts),
                    &native_ref,
                    "look up mutable SLDPRT ordered key",
                )? {
                    *count = count
                        .checked_add(1)
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                } else {
                    ctx.insert_btree_map(&mut projection_counts, native_ref, 1, OPERATION)?;
                }
            }
            Ok(ctx
                .admit_iter(
                    &projection_counts,
                    "scan SLDPRT multiply_projected_sketch_relation_records map values",
                )?
                .map(|(_, value)| value)
                .filter(|count| **count > 1)
                .count())
        },
    )?;
    Ok(result)
}

fn conflicting_display_reference(
    ctx: &DecodeContext<'_>,
    stream: &str,
    table_index: usize,
    candidates: &BTreeSet<FeatureSourceId>,
) -> Result<String, CodecError> {
    const OPERATION: &str = "retain SLDPRT conflicting display reference";
    let mut message = ctx.format_retained(
        format_args!("{stream}::DisplayFace[{table_index}] ("),
        OPERATION,
    )?;
    for (position, source) in ctx
        .admit_iter(
            candidates,
            "scan SLDPRT conflicting_display_reference values",
        )?
        .enumerate()
    {
        if position > 0 {
            ctx.append_retained(&mut message, ", ", OPERATION)?;
        }
        ctx.append_formatted_retained(&mut message, format_args!("{}", source.value()), OPERATION)?;
    }
    ctx.push_retained_char(&mut message, ')', OPERATION)?;
    Ok(message)
}

fn appearance_assignment_loss_message(
    ctx: &DecodeContext<'_>,
    assigned: &BTreeSet<FeatureSourceId>,
    matched: &BTreeSet<FeatureSourceId>,
    conflicts: &[String],
) -> Result<Option<String>, CodecError> {
    const PREFIX: &str = "VisualStates feature appearance assignment unresolved: ";
    const MISSING_PREFIX: &str = "feature source ID(s) ";
    const MISSING_SUFFIX: &str = " have no agreeing DisplayFace persistent reference";
    const CONFLICT_PREFIX: &str = "conflicting references rejected for ";

    const OPERATION: &str = "retain SLDPRT appearance assignment loss";
    let (mut unmatched, mut storage) = ctx.scoped_vector_storage(0, OPERATION)?;
    for source in ctx.admit_iter(assigned, OPERATION)? {
        if !ctx.contains_btree_set(matched, source, OPERATION)? {
            storage.with_storage(|| ctx.push_vec(&mut unmatched, source, OPERATION))?;
        }
    }
    if unmatched.is_empty() && conflicts.is_empty() {
        return Ok(None);
    }
    let mut message = ctx.copy_retained_text(PREFIX, OPERATION)?;
    if !unmatched.is_empty() {
        ctx.append_retained(&mut message, MISSING_PREFIX, OPERATION)?;
        for (position, source) in ctx.admit_iter(&unmatched, OPERATION)?.enumerate() {
            if position > 0 {
                ctx.append_retained(&mut message, ", ", OPERATION)?;
            }
            ctx.append_formatted_retained(
                &mut message,
                format_args!("{}", source.value()),
                OPERATION,
            )?;
        }
        ctx.append_retained(&mut message, MISSING_SUFFIX, OPERATION)?;
    }
    if !conflicts.is_empty() {
        if !unmatched.is_empty() {
            ctx.append_retained(&mut message, "; ", OPERATION)?;
        }
        ctx.append_retained(&mut message, CONFLICT_PREFIX, OPERATION)?;
        for (position, conflict) in ctx
            .admit_iter(
                conflicts,
                "scan SLDPRT appearance_assignment_loss_message values",
            )?
            .enumerate()
        {
            if position > 0 {
                ctx.append_retained(&mut message, "; ", OPERATION)?;
            }
            ctx.append_retained(&mut message, conflict, OPERATION)?;
        }
    }
    ctx.push_retained_char(&mut message, '.', OPERATION)?;
    Ok(Some(message))
}

/// Collect the available Parasolid body streams, excluding auxiliary sites.
fn active_body_streams<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
) -> Result<Vec<ActiveParasolidSite<'a>>, CodecError> {
    let mut streams = Vec::new();
    for section in scan.sections(ctx)? {
        if section.name_words().ghost() || section.name_words().resolved_features() {
            continue;
        }
        for stream in ctx.admit_iter(section.ps_streams(), "scan SLDPRT topology members")? {
            if !stream.header.is_body_stream() {
                continue;
            }
            ctx.reserve_vec(&mut streams, 1, "collect SLDPRT body streams")?;
            streams.push(ActiveParasolidSite {
                section,
                payload: &stream.payload,
                header: &stream.header,
            });
        }
    }
    ctx.stable_sort_by_key(
        &mut streams,
        |value| value.header.words.partition(),
        |left, right| right.cmp(left),
        "sort SLDPRT active body streams",
    )?;
    ctx.stable_sort_by_key(
        &mut streams,
        |value| value.section.name_words().partition(),
        |left, right| right.cmp(left),
        "sort SLDPRT active body streams",
    )?;
    Ok(streams)
}

/// Decode the available Parasolid body streams into one B-rep. Returns
/// `Ok(None)` when the streams frame but yield neither geometry nor a valid
/// empty partition/deltas model, so the caller falls back to metadata. A
/// framed stream that fails semantic decoding returns its error to the caller;
/// it must not be mistaken for a metadata-only document.
fn try_decode_brep<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &ContainerScan,
    streams: &[ActiveParasolidSite<'_>],
    classification: &crate::dialect::LayerClassification,
) -> Result<Option<(DecodedBrep<'ctx>, DecodeBody)>, CodecError> {
    let mut sites: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, stream) in ctx
        .admit_iter(streams, "scan SLDPRT try_decode_brep values")?
        .enumerate()
    {
        ctx.push_btree_group(
            &mut sites,
            stream.site_key(),
            index,
            "collect SLDPRT B-rep sites",
            "collect SLDPRT site streams",
        )?;
    }
    let mut decoded_sites = Vec::new();
    for (site, indices) in ctx.admit_iter(&sites, "scan SLDPRT try_decode_brep values")? {
        let first = indices[0];
        let mut bodies_storage = ctx.reserve_scoped(0, "SLDPRT temporary vector storage")?;
        let mut bodies = Vec::new();
        bodies_storage.with_storage(|| {
            ctx.reserve_capacity(&mut bodies, indices.len(), "collect SLDPRT site bodies")
        })?;
        for index in ctx.admit_iter(indices, "scan SLDPRT indices values")? {
            bodies_storage.with_storage(|| {
                ctx.push_vec(
                    &mut (bodies),
                    (streams[*index].payload, streams[*index].header),
                    "collect SLDPRT site bodies",
                )
            })?;
        }
        let decoded = decode_bodies(ctx, &bodies, streams[first].source_stream())?;
        ctx.reserve_vec(&mut decoded_sites, 1, "collect decoded SLDPRT sites")?;
        decoded_sites.push((site, first, decoded));
    }
    if decoded_sites.is_empty() {
        return Ok(None);
    }
    let active_site =
        container::select_active_parasolid_site(ctx, scan)?.map(|site| site.site_key());
    let resolved_active_site = match active_site.as_ref() {
        Some(active) => ctx.position_by(
            &decoded_sites,
            |(site, _, _)| ctx.equal(site.as_str(), active.as_str(), "select SLDPRT active site"),
            "select SLDPRT active site",
        )?,
        None => None,
    };
    // Without a resolved active site, this is only a deterministic merge
    // accumulator. All site identities are qualified below.
    let selected_site = resolved_active_site.unwrap_or(0);
    let selected_is_empty_model = if decoded_sites[selected_site].2.stats.source_entity_records == 0 {
        let selected_streams = ctx
            .get_btree_map(
                &sites,
                decoded_sites[selected_site].0,
                "look up SLDPRT site streams",
            )?
            .ok_or_else(|| CodecError::malformed("decoded SLDPRT site has no streams"))?;
        ctx.any_by(
            selected_streams,
            |index| Ok(streams[*index].header.words.partition()),
            "scan SLDPRT selected site body streams",
        )? && ctx.any_by(
            selected_streams,
            |index| Ok(streams[*index].header.words.deltas()),
            "scan SLDPRT selected site body streams",
        )?
    } else {
        false
    };
    let selected_has_geometry = !decoded_sites[selected_site].2.faces.is_empty()
        || !decoded_sites[selected_site].2.surfaces.is_empty()
        || !decoded_sites[selected_site].2.points.is_empty();
    if resolved_active_site.is_some() {
        if !selected_is_empty_model && !selected_has_geometry {
            return Ok(None);
        }
    } else {
        let any_site_has_geometry = ctx.any_by(
            &decoded_sites[..],
            |(_, _, decoded)| {
                Ok({
                    !decoded.faces.is_empty()
                        || !decoded.surfaces.is_empty()
                        || !decoded.points.is_empty()
                })
            },
            "scan SLDPRT try_decode_brep values",
        )?;
        let any_empty_model = ctx.any_by(
            &decoded_sites,
            |(site, _, decoded)| {
                if decoded.stats.source_entity_records != 0 {
                    return Ok(false);
                }
                let indices = ctx
                    .get_btree_map(&sites, *site, "look up SLDPRT site streams")?
                    .ok_or_else(|| CodecError::malformed("decoded SLDPRT site has no streams"))?;
                Ok(ctx.any_by(
                    indices,
                    |index| Ok(streams[*index].header.words.partition()),
                    "scan SLDPRT empty site streams",
                )? && ctx.any_by(
                    indices,
                    |index| Ok(streams[*index].header.words.deltas()),
                    "scan SLDPRT empty site streams",
                )?)
            },
            "scan SLDPRT empty model sites",
        )?;
        if !any_site_has_geometry && !any_empty_model {
            return Ok(None);
        }
    }
    let active_stream = resolved_active_site.map(|site| decoded_sites[site].1);
    let shared_header = match decoded_sites.first() {
        Some(&(_, first, _)) if active_stream.is_none() => {
            const HEADERS: &str = "compare SLDPRT site headers";
            let first_header = streams[first].header;
            ctx.all_by(
                &decoded_sites,
                |(_, representative, _)| {
                    let header = streams[*representative].header;
                    Ok(
                        ctx.equal(header.schema.value(), first_header.schema.value(), HEADERS)?
                            && ctx.equal(
                                header.description.as_str(),
                                first_header.description.as_str(),
                                HEADERS,
                            )?,
                    )
                },
                HEADERS,
            )?
            .then_some(first_header)
        }
        _ => None,
    };
    let mut workspace =
        ctx.reserve_scoped(0, "SLDPRT decoded header and configuration workspace")?;
    let metadata_header = workspace.with_storage(|| -> Result<_, CodecError> {
        let metadata_header = active_stream
            .map(|index| streams[index].header)
            .or(shared_header)
            .map(|header| {
                let description = ctx.copy_retained_text(
                    header.description.as_str(),
                    "retain SLDPRT B-rep header description",
                )?;
                let schema = ctx.copy_retained_text(
                    header.schema.value(),
                    "retain SLDPRT B-rep header schema",
                )?;
                let schema =
                    cadmpeg_parasolid::OwnedSchemaToken::parse(ctx, schema)?.map_err(|_| {
                        CodecError::Malformed("invalid admitted Parasolid schema".into())
                    })?;
                Ok::<_, CodecError>(StreamHeader {
                    description,
                    words: header.words,
                    schema,
                    body_offset: header.body_offset,
                })
            })
            .transpose()?;
        Ok(metadata_header)
    })?;
    let (selected_site_key, selected, mut decoded) = decoded_sites.swap_remove(selected_site);
    if active_stream.is_none() {
        decoded.qualify_ids(ctx, selected_site_key)?;
    }
    bind_opaque_geometry(ctx, &mut decoded, &streams[selected].section.native_id())?;
    let mut configuration_bodies = Vec::new();
    if let Some(index) = configuration_index(ctx, streams[selected].source_stream().as_str())? {
        let bodies = copy_body_ids(ctx, &decoded.bodies, &mut workspace)?;
        workspace.with_storage(|| {
            ctx.push_vec(
                &mut configuration_bodies,
                (index, bodies),
                "collect SLDPRT configuration bodies",
            )
        })?;
    }
    for (site, first, mut alternate) in
        ctx.admit_iter(decoded_sites, "merge SLDPRT alternate sites")?
    {
        alternate.qualify_ids(ctx, site)?;
        bind_opaque_geometry(ctx, &mut alternate, &streams[first].section.native_id())?;
        if let Some(index) = configuration_index(ctx, streams[first].source_stream().as_str())? {
            let bodies = copy_body_ids(ctx, &alternate.bodies, &mut workspace)?;
            workspace.with_storage(|| {
                ctx.push_vec(
                    &mut configuration_bodies,
                    (index, bodies),
                    "collect SLDPRT configuration bodies",
                )
            })?;
        }
        // Keep only the selected source's bridge sequence namespace. Alternate
        // configuration sites are qualified into the model but do not own the
        // active SWIFT CadIdentifier lane.
        merge_brep(ctx, &mut decoded, alternate)?;
    }
    let report = build_geometry_report(
        ctx,
        scan,
        &mut decoded,
        classification,
        container::notes_charged(ctx, scan)?,
    )?;
    Ok(Some((
        DecodedBrep {
            _workspace: workspace,
            metadata_header,
            brep: decoded,
            configuration_bodies,
        },
        report,
    )))
}

fn copy_body_ids<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bodies: &[cadmpeg_ir::topology::Body],
    workspace: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<Vec<ConfigurationBodyIdentity<'ctx>>, CodecError> {
    let mut ids = Vec::new();
    workspace
        .with_storage(|| ctx.reserve_capacity(&mut ids, bodies.len(), "collect SLDPRT body IDs"))?;
    for body in ctx.admit_iter(bodies, "scan SLDPRT copy_body_ids values")? {
        let (id, storage) = ctx.with_scoped_storage("retain SLDPRT body ID", || {
            body.id.try_clone_for_decode(ctx, "retain SLDPRT body ID")
        })?;
        workspace.with_storage(|| {
            ctx.push_vec(
                &mut ids,
                ConfigurationBodyIdentity { id, storage },
                "collect SLDPRT body IDs",
            )
        })?;
    }
    Ok(ids)
}

fn bind_opaque_geometry(
    ctx: &DecodeContext<'_>,
    brep: &mut Brep,
    source: &UnknownId,
) -> Result<(), CodecError> {
    for surface in ctx.admit_iter(&mut brep.surfaces, "bind SLDPRT opaque surfaces")? {
        if let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record }) =
            &mut surface.geometry
        {
            if record.is_none() {
                *record =
                    Some(source.try_clone_for_decode(ctx, "retain SLDPRT opaque surface source")?);
            }
        }
    }
    for curve in ctx.admit_iter(&mut brep.curves, "bind SLDPRT opaque curves")? {
        if let cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
            record,
        }) = &mut curve.geometry
        {
            if record.is_none() {
                *record =
                    Some(source.try_clone_for_decode(ctx, "retain SLDPRT opaque curve source")?);
            }
        }
    }
    Ok(())
}

fn append_brep_arena<T>(
    ctx: &DecodeContext<'_>,
    target: &mut Vec<T>,
    source: &mut Vec<T>,
) -> Result<(), CodecError> {
    ctx.append_vec(target, source, "merge SLDPRT B-rep arena")
}

fn merge_brep(
    ctx: &DecodeContext<'_>,
    target: &mut Brep,
    mut source: Brep,
) -> Result<(), CodecError> {
    // Sequence links are source-local and belong only to the selected SWIFT
    // source. Alternate configuration sequences must not enter its namespace.
    target
        .annotations
        .append(
            ctx,
            source.annotations,
            "merge SLDPRT annotation identities",
        )?
        .map_err(CodecError::from)?;
    append_brep_arena(ctx, &mut target.bodies, &mut source.bodies)?;
    append_brep_arena(ctx, &mut target.regions, &mut source.regions)?;
    append_brep_arena(ctx, &mut target.shells, &mut source.shells)?;
    append_brep_arena(ctx, &mut target.faces, &mut source.faces)?;
    append_brep_arena(ctx, &mut target.loops, &mut source.loops)?;
    append_brep_arena(ctx, &mut target.coedges, &mut source.coedges)?;
    append_brep_arena(ctx, &mut target.edges, &mut source.edges)?;
    append_brep_arena(ctx, &mut target.vertices, &mut source.vertices)?;
    append_brep_arena(ctx, &mut target.points, &mut source.points)?;
    append_brep_arena(ctx, &mut target.surfaces, &mut source.surfaces)?;
    append_brep_arena(
        ctx,
        &mut target.procedural_surfaces,
        &mut source.procedural_surfaces,
    )?;
    append_brep_arena(ctx, &mut target.curves, &mut source.curves)?;
    append_brep_arena(ctx, &mut target.pcurves, &mut source.pcurves)?;
    append_brep_arena(ctx, &mut target.unknowns, &mut source.unknowns)?;
    append_brep_arena(ctx, &mut target.face_colors, &mut source.face_colors)?;
    append_brep_arena(ctx, &mut target.face_atoms, &mut source.face_atoms)?;
    append_brep_arena(ctx, &mut target.body_modifiers, &mut source.body_modifiers)?;
    append_brep_arena(ctx, &mut target.losses, &mut source.losses)?;
    target.stats.unknown_surface_faces += source.stats.unknown_surface_faces;
    target.stats.unknown_procedural_supports += source.stats.unknown_procedural_supports;
    target.stats.unknown_curve_edges += source.stats.unknown_curve_edges;
    target.stats.ambiguous_pcurve_parameters += source.stats.ambiguous_pcurve_parameters;
    target.stats.off_surface_nurbs_pcurves += source.stats.off_surface_nurbs_pcurves;
    target.stats.source_entity_records += source.stats.source_entity_records;
    target.stats.unresolved_face_colors += source.stats.unresolved_face_colors;
    target.stats.ambiguous_face_owners += source.stats.ambiguous_face_owners;
    target.stats.unclaimed_faces += source.stats.unclaimed_faces;
    target.stats.synthetic_body_grouping |= source.stats.synthetic_body_grouping;
    Ok(())
}

fn ensure_display_appearance(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    definition: &crate::appearance::AppearanceDefinition,
    section_ordinal: usize,
    annotations: &mut Annotations,
) -> Result<AppearanceId, CodecError> {
    if let Some(existing) = ctx.find_by(
        &ir.model.appearances[..],
        |appearance| {
            Ok({
                appearance.base_color == Some(definition.color)
                    && ctx.equal(
                        &appearance.name.as_deref(),
                        &Some(definition.name.as_str()),
                        "compare SLDPRT appearance names",
                    )?
            })
        },
        "scan SLDPRT ensure_display_appearance values",
    )? {
        return existing
            .id
            .try_clone_for_decode(ctx, "SLDPRT display appearance identity");
    }
    let id = AppearanceId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "appearance", "displaylist"),
        cadmpeg_ir::ids::IdentityKey::from(section_ordinal).colon(definition.record_offset),
    );
    crate::annotations::note(
        ctx,
        annotations,
        id.as_str(),
        &definition.source_name,
        u64_from_index(definition.record_offset),
        "displaylist_visual_properties",
        Exactness::ByteExact,
    )?;
    ctx.reserve_vec(
        &mut ir.model.appearances,
        1,
        "admit SLDPRT display appearance",
    )?;
    ir.model.appearances.push(Appearance {
        id: id.try_clone_for_decode(ctx, "copy SLDPRT display appearance identity")?,
        name: Some(
            ctx.copy_retained_text(&definition.name, "retain SLDPRT display appearance name")?,
        ),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: Some("moVisualProperties_c".into()),
        category: None,
        base_color: Some(definition.color),
        properties: BTreeMap::new(),
        textures: Vec::new(),
    });
    Ok(id)
}

/// A decoded model, its source annotations, unknown records, PMI losses, and
/// the native records it stored, which the design-loss scan reads directly.
type BuiltIr = (
    CadIr,
    Annotations,
    Vec<UnknownRecord>,
    Vec<cadmpeg_ir::report::loss::LossNote>,
    crate::native::SldprtNative,
);

fn build_geometry_ir(
    ctx: &DecodeContext<'_>,
    scan: &mut ContainerScan<'_>,
    classification: &crate::dialect::LayerClassification,
    decoded: DecodedBrep<'_>,
    form_padding: Option<usize>,
    admitted_entities: &mut u64,
) -> Result<BuiltIr, CodecError> {
    const BLOCKS: &str = "retain SLDPRT source blocks";
    const FACE_COLORS: &str = "admit SLDPRT face appearance";
    fn add_opaque_link<'a>(
        ctx: &DecodeContext<'_>,
        opaque_links: &mut BTreeMap<&'a str, Vec<String>>,
        workspace: &mut cadmpeg_core::decode::ScopedReservation<'_>,
        record: &'a str,
        entity: &str,
    ) -> Result<(), CodecError> {
        const OPERATION: &str = "index SLDPRT opaque geometry record";
        let link = ctx.copy_retained_text(entity, "retain SLDPRT opaque geometry link")?;
        workspace.with_storage(|| {
            ctx.push_btree_group(
                opaque_links,
                record,
                link,
                OPERATION,
                "index SLDPRT opaque geometry link",
            )
        })
    }

    let DecodedBrep {
        _workspace,
        metadata_header,
        mut brep,
        configuration_bodies,
    } = decoded;
    let appearance_definitions = crate::appearance::definitions(ctx, scan)?;
    let mut display_sections = Vec::new();
    let mut display_summary = crate::tessellation::Summary::default();
    for section in scan.sections(ctx)? {
        let faces = crate::tessellation::section_display_faces(ctx, section)?;
        let summary = crate::tessellation::summary_for_faces(ctx, &faces)?;
        display_summary.vertices += summary.vertices;
        display_summary.triangles += summary.triangles;
        ctx.reserve_vec(&mut display_sections, 1, "collect SLDPRT display sections")?;
        display_sections.push((section, faces));
    }
    let mut ir = CadIr::decoded(source_meta(
        ctx,
        scan,
        classification,
        metadata_header.as_ref(),
        display_summary,
    )?);
    let mut annotations = std::mem::take(&mut brep.annotations);
    let mut pmi_losses = Vec::new();
    let mut histories = crate::history::histories(ctx, scan, &mut annotations, &mut pmi_losses)?;
    let mut lanes = crate::resolved_features::assembly::lanes(ctx, scan, &mut annotations)?;
    let mut supplemental_config_lanes =
        crate::resolved_features::assembly::supplemental_config_lanes(ctx, scan, &mut annotations)?;
    crate::resolved_features::classes::bind_history_classes(ctx, &mut histories, &lanes)?;
    crate::resolved_features::bindings::bind_scalar_operands(ctx, &histories, &mut lanes)?;
    crate::resolved_features::bindings::bind_scalar_operands(
        ctx,
        &histories,
        &mut supplemental_config_lanes,
    )?;
    let pmi_dimensions = crate::pmi::dimensions(ctx, scan, &mut annotations, &mut pmi_losses)?;
    project_design_history(
        ctx,
        &mut ir,
        DesignHistoryInput {
            histories: &histories,
            lanes: &lanes,
            pmi_dimensions: &pmi_dimensions,
        },
        scan,
        form_padding,
        &mut pmi_losses,
    )?;
    crate::resolved_features::operations::bind_feature_operations(
        ctx,
        &mut ir.model.features,
        &histories,
        &lanes,
        form_padding,
    )?;
    crate::pmi::apply_to_parameters(
        ctx,
        &mut ir.model.parameters,
        &ir.model.features,
        &pmi_dimensions,
    )?;
    let (identity_lanes, identity_lane_storage) = ctx
        .with_scoped_storage("SLDPRT parameter identity lane workspace", || {
            parameter_identity_lanes(ctx, &lanes)
        })?;
    crate::resolved_features::projections::bind_parameter_scalars(
        ctx,
        &mut ir.model.parameters,
        &ir.model.features,
        &histories,
        identity_lanes,
    )?;
    drop(identity_lane_storage);
    crate::resolved_features::projections::synthesize_display_relation_parameters(
        ctx,
        &mut ir.model.parameters,
        &ir.model.features,
        lanes.iter().chain(supplemental_config_lanes.iter()),
    )?;
    crate::resolved_features::projections::type_display_relation_parameters(
        ctx,
        &mut ir.model.parameters,
        &ir.model.features,
        &lanes,
    )?;
    crate::history::configuration::align_configuration_parameter_kinds(ctx, &mut ir)?;
    complete_resolved_configuration_parameter_snapshots(ctx, &mut ir)?;
    stamp_parameter_baseline(ctx, &mut ir)?;
    let crate::resolved_features::sketch_projection::ProjectedSketches {
        mut sketches,
        entities: mut sketch_entities,
        constraints: mut sketch_constraints,
    } = crate::resolved_features::sketch_projection::sketches(ctx, scan, &mut annotations)?;
    crate::resolved_features::profiles::bind_sketch_profiles(
        ctx,
        &mut ir.model.features,
        crate::resolved_features::profiles::SketchArenas {
            sketches: &mut sketches,
            sketch_entities: &mut sketch_entities,
            sketch_constraints: &mut sketch_constraints,
            annotations: &mut annotations,
        },
        &ir.model.parameters,
        &histories,
        &lanes,
    )?;
    crate::resolved_features::bindings::bind_unresolved_detached_sketch_objects(
        ctx,
        &ir.model.features,
        &histories,
        &mut supplemental_config_lanes,
    )?;
    crate::resolved_features::projections::project_compact_edge_selections(
        ctx,
        &mut ir.model.features,
        &[],
        &supplemental_config_lanes,
    )?;
    crate::history::configuration::project_configuration_supplemental_edge_selections(
        ctx,
        &mut ir,
        &supplemental_config_lanes,
    )?;
    crate::resolved_features::profiles::project_compact_sketch_profiles(
        ctx,
        &mut ir.model.features,
        &mut sketches,
        &mut sketch_entities,
        &histories,
        &lanes,
        &mut pmi_losses,
    )?;
    // Marker-backed sketches can originate in either lane family. Their
    // geometry and constraints must use the same complete lane set.
    let base_lane_count = lanes.len();
    ctx.extend_vec(
        &mut lanes,
        supplemental_config_lanes,
        "merge SLDPRT feature input lanes",
    )?;
    let all_lanes = lanes;
    let lanes = &all_lanes[..base_lane_count];
    let sketch_lanes = all_lanes.as_slice();
    let (spatial_sketches, spatial_sketch_entities) =
        crate::resolved_features::markers::spatial_sketches(
            ctx,
            &mut ir.model.features,
            &histories,
            sketch_lanes,
        )?;
    ir.model.spatial_sketches = spatial_sketches;
    ir.model.spatial_sketch_entities = spatial_sketch_entities;
    crate::resolved_features::profiles::project_marker_backed_sketches(
        ctx,
        &mut ir.model.features,
        &mut sketches,
        &mut sketch_entities,
        &histories,
        sketch_lanes,
    )?;
    crate::resolved_features::profiles::project_sketch_block_profiles(
        ctx,
        &mut ir.model.features,
        &mut sketches,
        &mut sketch_entities,
        &histories,
        sketch_lanes,
    )?;
    crate::history::bind::bind_unique_sketch_feature(
        ctx,
        &mut ir.model.features,
        &sketches,
        &histories,
    )?;
    crate::resolved_features::component_paths::project_dissected_sketches(
        ctx,
        &mut ir.model.features,
        &sketches,
        &histories,
    )?;
    crate::resolved_features::axes::bind_profile_revolution_axes(
        ctx,
        &mut ir.model.features,
        &histories,
        lanes,
        &sketches,
        &brep.surfaces,
    )?;
    crate::resolved_features::bindings::bind_pattern_inputs(
        ctx,
        &mut ir.model.features,
        &histories,
        lanes,
    )?;
    crate::resolved_features::bindings::bind_sweep_adjacent_profiles(
        ctx,
        &mut ir.model.features,
        &histories,
        lanes,
    )?;
    crate::resolved_features::dimensions::project_dimensioned_sketch_geometry(
        ctx,
        &mut sketch_entities,
        &sketches,
        &brep.surfaces,
        &ir.model.features,
        &ir.model.parameters,
        sketch_lanes,
    )?;
    crate::resolved_features::dimensions::project_marker_dimensioned_circles(
        ctx,
        &mut sketch_entities,
        &mut sketches,
        &ir.model.features,
        &ir.model.parameters,
        sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_relation_point_geometry(
        ctx,
        &mut sketch_entities,
        &sketches,
        &ir.model.features,
        sketch_lanes,
    )?;
    crate::resolved_features::dimensions::project_relation_point_dimensioned_circles(
        ctx,
        &mut sketch_entities,
        &ir.model.features,
        &ir.model.parameters,
        sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_relation_solved_line_geometry(
        ctx,
        &mut sketch_entities,
        &sketches,
        &ir.model.features,
        &ir.model.parameters,
        sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_relation_solved_point_geometry(
        ctx,
        &mut sketch_entities,
        &sketches,
        &ir.model.features,
        &ir.model.parameters,
        sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_relation_bindings(
        ctx,
        &mut sketch_constraints,
        &sketches,
        &ir.model.features,
        &sketch_entities,
        &ir.model.parameters,
        sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_spatial_relation_bindings(
        ctx,
        &mut ir.model.spatial_sketch_constraints,
        &mut ir.model.spatial_sketch_entities,
        &ir.model.spatial_sketches,
        &ir.model.features,
        &ir.model.parameters,
        sketch_lanes,
    )?;
    stamp_feature_baseline(ctx, &mut ir)?;
    let mut attributes = crate::metadata::attributes(ctx, scan, &mut annotations)?;
    let custom_properties = crate::history::project::custom_property_attributes(ctx, &histories)?;
    ctx.extend_vec(
        &mut attributes,
        custom_properties,
        "append SLDPRT custom properties",
    )?;
    ir.model.attributes = attributes;
    ir.model.sketches = sketches;
    ir.model.sketch_entities = sketch_entities;
    ir.model.sketch_constraints = sketch_constraints;
    stamp_sketch_baseline(ctx, &mut ir, &all_lanes)?;

    let brep_entities = brep.neutral_entity_count()?;
    ir.model.bodies = brep.bodies;
    ir.model.regions = brep.regions;
    ir.model.shells = brep.shells;
    ir.model.faces = brep.faces;
    ir.model.loops = brep.loops;
    ir.model.coedges = brep.coedges;
    ir.model.edges = brep.edges;
    ir.model.vertices = brep.vertices;
    ir.model.points = brep.points;
    ir.model.surfaces = brep.surfaces;
    ir.model.procedural_surfaces = brep.procedural_surfaces;
    ir.model.curves = brep.curves;
    ir.model.pcurves = brep.pcurves;
    *admitted_entities = brep_entities;
    let face_bridge_sequences = std::mem::take(&mut brep.face_bridge_sequences);
    let edge_use_sequences = std::mem::take(&mut brep.edge_use_sequences);
    let vertex_use_sequences = std::mem::take(&mut brep.vertex_use_sequences);
    let topology_index = crate::swift::TopologyIdentityIndex::from_model(
        ctx,
        crate::swift::PrimaryTopology {
            bodies: &ir.model.bodies,
            faces: &ir.model.faces,
            edges: &ir.model.edges,
            vertices: &ir.model.vertices,
        },
        &face_bridge_sequences,
        &edge_use_sequences,
        &vertex_use_sequences,
    )?;
    let face_atoms = std::mem::take(&mut brep.face_atoms);
    let mut face_identities = Vec::new();
    ctx.reserve_vec(
        &mut face_identities,
        face_atoms.len(),
        "collect SLDPRT face identities",
    )?;
    for atom in ctx.admit_iter(face_atoms, "collect SLDPRT face identities")? {
        face_identities.push((atom.face, atom.identity));
    }
    let mut face_producers_storage = ctx.reserve_scoped(0, "SLDPRT temporary vector storage")?;
    let mut face_producers = Vec::new();
    face_producers_storage.with_storage(|| {
        ctx.reserve_capacity(
            &mut face_producers,
            face_identities.len(),
            "collect SLDPRT face producers",
        )
    })?;
    for (target, identity) in
        ctx.admit_iter(&face_identities, "scan SLDPRT build_geometry_ir values")?
    {
        face_producers_storage.with_storage(|| {
            ctx.push_vec(
                &mut (face_producers),
                (
                    ctx.copy_retained_text(target.as_str(), "retain SLDPRT face producer ID")?,
                    identity.feature_source_id.value(),
                ),
                "collect SLDPRT face producers",
            )
        })?;
    }
    let mut body_modifiers_storage = ctx.reserve_scoped(0, "SLDPRT temporary vector storage")?;
    let mut body_modifiers = Vec::new();
    for modifier in ctx.admit_iter(
        std::mem::take(&mut brep.body_modifiers),
        "collect SLDPRT body modifiers",
    )? {
        if let Some(target) = modifier.target {
            body_modifiers_storage.with_storage(|| {
                ctx.push_vec(
                    &mut (body_modifiers),
                    (target, modifier.history_ordinal),
                    "collect SLDPRT body modifiers",
                )
            })?;
        }
    }
    crate::history::bind::derive_feature_outputs(
        ctx,
        &mut ir.model.features,
        &histories,
        crate::history::bind::FeatureOutputSources {
            face_producers: &face_producers,
            body_modifiers: &body_modifiers,
        },
        &ir.model.faces,
        &ir.model.shells,
        &ir.model.regions,
    )?;
    let topology_selection_inputs = crate::history::selections::TopologySelectionInputs {
        bodies: &ir.model.bodies,
        faces: &ir.model.faces,
        surfaces: &ir.model.surfaces,
        edges: &ir.model.edges,
        curves: &ir.model.curves,
        lanes: &all_lanes,
        face_identities: &face_identities,
    };
    crate::history::selections::bind_topology_selections(
        ctx,
        &mut ir.model.features,
        &histories,
        &topology_selection_inputs,
    )?;
    crate::resolved_features::bindings::bind_mirror_surface_planes(
        ctx,
        &mut ir.model.features,
        &histories,
        &all_lanes,
        &face_identities,
        &ir.model.faces,
        &ir.model.surfaces,
    )?;
    crate::resolved_features::holes::project_profiled_hole_constructions(
        ctx,
        &mut ir.model.features,
        &ir.model.sketch_entities,
        &histories,
        &all_lanes,
    )?;
    crate::resolved_features::holes::project_hole_position_sketches(
        ctx,
        &mut ir.model.features,
        &ir.model.sketches,
        &ir.model.sketch_entities,
        &histories,
        &all_lanes,
    )?;
    crate::resolved_features::holes::project_spatial_hole_position_sketches(
        ctx,
        &mut ir.model.features,
        &ir.model.spatial_sketches,
        &ir.model.spatial_sketch_entities,
        &ir.model.surfaces,
        &histories,
        &all_lanes,
    )?;
    crate::resolved_features::holes::project_generated_hole_axes(
        ctx,
        &mut ir.model.features,
        &histories,
        &all_lanes,
        &face_identities,
        &ir.model.faces,
        &ir.model.surfaces,
    )?;
    crate::resolved_features::holes::project_topological_hole_constructions(
        ctx,
        &mut ir.model.features,
        &crate::resolved_features::holes::HoleTopology {
            surfaces: &ir.model.surfaces,
            faces: &ir.model.faces,
            loops: &ir.model.loops,
            coedges: &ir.model.coedges,
            edges: &ir.model.edges,
            vertices: &ir.model.vertices,
            points: &ir.model.points,
        },
    )?;
    crate::resolved_features::holes::project_hole_axes(
        ctx,
        &mut ir.model.features,
        &ir.model.sketch_entities,
        &crate::resolved_features::holes::HoleTopology {
            surfaces: &ir.model.surfaces,
            faces: &ir.model.faces,
            loops: &ir.model.loops,
            coedges: &ir.model.coedges,
            edges: &ir.model.edges,
            vertices: &ir.model.vertices,
            points: &ir.model.points,
        },
        &histories,
        &all_lanes,
    )?;
    crate::resolved_features::holes::project_hole_topology_axes(
        ctx,
        &mut ir.model.features,
        &crate::resolved_features::holes::HoleTopology {
            surfaces: &ir.model.surfaces,
            faces: &ir.model.faces,
            loops: &ir.model.loops,
            coedges: &ir.model.coedges,
            edges: &ir.model.edges,
            vertices: &ir.model.vertices,
            points: &ir.model.points,
        },
    )?;
    crate::resolved_features::holes::project_bore_backed_position_sketches(
        ctx,
        &mut ir.model.features,
        &mut ir.model.sketches,
        &mut ir.model.sketch_entities,
        &ir.model.surfaces,
        &histories,
        &all_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_relation_bindings(
        ctx,
        &mut ir.model.sketch_constraints,
        &ir.model.sketches,
        &ir.model.features,
        &ir.model.sketch_entities,
        &ir.model.parameters,
        &all_lanes,
    )?;
    crate::history::bind::order_features_for_regeneration(ctx, &mut ir.model.features)?;
    assign_configuration_bodies(ctx, &mut ir, configuration_bodies)?;
    let configuration_losses = crate::history::configuration::project_configuration_sketch_states(
        ctx,
        &mut ir,
        &histories,
        &all_lanes,
        &mut annotations,
    )?;
    ctx.extend_vec(
        &mut pmi_losses,
        configuration_losses,
        "append SLDPRT configuration PMI losses",
    )?;
    crate::history::configuration::bind_configuration_topology_selections(
        ctx,
        &mut ir,
        &histories,
        &all_lanes,
        &face_identities,
    )?;
    mark_active_configuration(ctx, &mut ir)?;
    crate::resolved_features::projections::project_unbound_cosmetic_thread_faces(
        ctx,
        &mut ir.model.features,
        &histories,
        &all_lanes,
        &ir.model.faces,
        &ir.model.surfaces,
    )?;
    crate::resolved_features::projections::project_unbound_offset_plane_faces(
        ctx,
        &mut ir.model.features,
        &ir.model.faces,
        &ir.model.surfaces,
    )?;
    crate::history::configuration::inherit_configuration_reference_plane_states(ctx, &mut ir)?;
    sync_active_configuration_resolutions(ctx, &mut ir)?;
    crate::history::bind::order_model_features_for_regeneration(ctx, &mut ir)?;
    let (pattern_hole_nominals, _nominal_storage) = ctx
        .with_scoped_storage("SLDPRT hole-pattern nominal workspace", || {
            crate::swift::pattern_hole_nominal_context(ctx, &ir.model.features)
        })?;
    ir.model.pmi = crate::swift::annotations(
        ctx,
        scan,
        &mut annotations,
        Some(&topology_index),
        Some(&pattern_hole_nominals),
    )?;
    stamp_feature_baseline(ctx, &mut ir)?;
    let mut native = crate::native::SldprtNative {
        feature_histories: histories,
        feature_input_lanes: all_lanes,
        pmi_dimensions,
    };
    assign_native_configuration_indices(ctx, &ir, &mut native)?;
    if let Some(source) = &mut ir.source {
        ctx.insert_btree_map(
            &mut (source.attributes),
            cadmpeg_core::nonblank_literal!("sldprt_native_configuration_sha256"),
            crate::history::hash::native_configuration_hash(ctx, &native.feature_histories)?,
            "insert SLDPRT ordered entry",
        )?;
        ctx.insert_btree_map(
            &mut (source.attributes),
            cadmpeg_core::nonblank_literal!("sldprt_native_history_sha256"),
            crate::history::hash::history_hash(ctx, &native.feature_histories)?,
            "insert SLDPRT ordered entry",
        )?;
    }
    ctx.admit_entities(
        u64_from_index(ir.model.entity_count()),
        admitted_entities,
        "admit SLDPRT entities",
    )?;
    native.store(ctx, ir.native.namespace_mut("sldprt"))?;
    // Stamp baseline before fabricating the read-side configuration snapshot.
    stamp_configuration_baseline(ctx, &mut ir)?;
    snapshot_active_configuration(ctx, &mut ir)?;
    let mut unknowns = brep.unknowns;
    // Appearance and binding identities in the model, kept current as face
    // colours add to them.
    let mut appearance_ids_storage =
        ctx.reserve_scoped(0, "SLDPRT appearance_ids lookup workspace")?;
    let mut appearance_ids = BTreeSet::new();
    for appearance in ctx.admit_iter(&ir.model.appearances, FACE_COLORS)? {
        appearance_ids_storage.with_storage(|| {
            let id = appearance.id.try_clone_for_decode(ctx, FACE_COLORS)?;
            ctx.insert_btree_set(&mut appearance_ids, id, FACE_COLORS)
        })?;
    }
    let mut binding_ids_storage = ctx.reserve_scoped(0, "SLDPRT binding_ids lookup workspace")?;
    let mut binding_ids = BTreeSet::new();
    for binding in ctx.admit_iter(&ir.model.appearance_bindings, FACE_COLORS)? {
        binding_ids_storage.with_storage(|| {
            let id = binding.id.try_clone_for_decode(ctx, FACE_COLORS)?;
            ctx.insert_btree_set(&mut binding_ids, id, FACE_COLORS)
        })?;
    }
    for owned_face_color in ctx.admit_iter(brep.face_colors, FACE_COLORS)? {
        let annotation_source = &owned_face_color.source_stream;
        let site = match owned_face_color.site_key.as_deref() {
            Some(site) => Some(site),
            None => match owned_face_color.value.target.as_deref() {
                Some(target) => ctx
                    .split_once(target, "@", "split SLDPRT colour target site")?
                    .map(|(_, site)| site),
                None => None,
            },
        };
        let mut qualified_site = String::new();
        if let Some(site) = site {
            const OPERATION: &str = "retain SLDPRT colour site qualifier";
            ctx.push_retained_char(&mut qualified_site, '@', OPERATION)?;
            ctx.append_retained(&mut qualified_site, site, OPERATION)?;
        }
        let face_color = owned_face_color.value;
        // The site qualifier is admitted once here and appended as a key tail,
        // so the colour id and its binding below share one proof.
        let site = cadmpeg_ir::ids::IdentityKeyTail::try_new(qualified_site).map_err(|error| {
            CodecError::malformed(format_args!(
                "SLDPRT colour site qualifier is not identity key text: {error}"
            ))
        })?;
        let id = AppearanceId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "appearance", "entity53"),
            cadmpeg_ir::ids::IdentityKey::from(face_color.color_attr).with_tail(&site),
        );
        crate::annotations::note(
            ctx,
            &mut annotations,
            id.as_str(),
            annotation_source,
            u64_from_index(face_color.offset),
            "00_53_color",
            Exactness::ByteExact,
        )?;
        if !ctx.contains_btree_set(&appearance_ids, &id, FACE_COLORS)? {
            appearance_ids_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut appearance_ids,
                    id.try_clone_for_decode(ctx, FACE_COLORS)?,
                    FACE_COLORS,
                )
            })?;
            ctx.reserve_vec(&mut ir.model.appearances, 1, "admit SLDPRT face appearance")?;
            ir.model.appearances.push(Appearance {
                id: id.try_clone_for_decode(ctx, FACE_COLORS)?,
                name: None,
                asset_guid: None,
                library_id: None,
                visual_guid: None,
                physical_token: None,
                schema: Some("entity-53".into()),
                category: None,
                base_color: Some(face_color.color),
                textures: Vec::new(),
                properties: BTreeMap::new(),
            });
        }
        if let Some(target) = face_color.target {
            let binding_id = cadmpeg_ir::ids::AppearanceBindingId::compose(
                &cadmpeg_ir::identity_namespace!("sldprt", "appearance", "binding"),
                cadmpeg_ir::identity_key!("face:")
                    .then(face_color.face_attr)
                    .colon(face_color.color_attr)
                    .with_tail(&site),
            );
            let target = cadmpeg_ir::ids::FaceId::mint(target).map_err(|error| {
                CodecError::malformed(format_args!(
                    "SLDPRT colour target is not an identity: {error}"
                ))
            })?;
            if !ctx.contains_btree_set(&binding_ids, &binding_id, FACE_COLORS)? {
                binding_ids_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut binding_ids,
                        binding_id.try_clone_for_decode(ctx, FACE_COLORS)?,
                        FACE_COLORS,
                    )
                })?;
                ctx.reserve_vec(
                    &mut ir.model.appearance_bindings,
                    1,
                    "admit SLDPRT face appearance binding",
                )?;
                ir.model.appearance_bindings.push(AppearanceBinding {
                    id: binding_id,
                    target: AppearanceTarget::Face(target),
                    appearance: id,
                    source_entity_id: Some(face_color.face_attr.to_string()),
                    object_type: Some("Face".into()),
                    visible: None,
                    channels: BTreeMap::new(),
                });
            }
        }
    }
    for (index, definition) in ctx
        .admit_iter(
            appearance_definitions,
            "collect SLDPRT appearance definitions",
        )?
        .enumerate()
    {
        let id = AppearanceId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "appearance", "material"),
            index,
        );
        crate::annotations::note(
            ctx,
            &mut annotations,
            id.as_str(),
            &definition.source_name,
            u64_from_index(definition.record_offset),
            "moVisualProperties_c",
            Exactness::ByteExact,
        )?;
        ctx.reserve_vec(
            &mut ir.model.appearances,
            1,
            "admit SLDPRT material appearance",
        )?;
        ir.model.appearances.push(Appearance {
            id,
            name: Some(definition.name),
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: Some("moVisualProperties_c".to_string()),
            category: None,
            base_color: Some(definition.color),
            textures: Vec::new(),
            properties: BTreeMap::new(),
        });
    }
    let (feature_appearance_sources, _feature_appearance_sources_storage) = ctx
        .with_scoped_storage("SLDPRT feature_appearance_sources workspace", || {
            ctx.collect_btree_set(
                crate::appearance::feature_assignments(ctx, scan)?
                    .into_iter()
                    .map(|assignment| assignment.feature_source_id),
                "index SLDPRT feature appearance sources",
            )
        })?;
    let mut matched_storage =
        ctx.reserve_scoped(0, "SLDPRT matched appearance source workspace")?;
    let mut conflict_storage = ctx.reserve_scoped(0, "SLDPRT display conflict workspace")?;
    let mut persistent_storage =
        ctx.reserve_scoped(0, "SLDPRT persistent face binding workspace")?;
    let mut matched_feature_sources = BTreeSet::new();
    let mut conflicting_display_references = Vec::new();
    let mut persistent_face_bindings = Vec::new();
    for (display, display_faces) in
        ctx.admit_iter(display_sections, "scan SLDPRT display sections")?
    {
        if display_faces.is_empty() {
            continue;
        }
        let display_stream = display.source_stream();
        for (table_index, face) in ctx
            .admit_iter(&display_faces, "scan SLDPRT build_geometry_ir values")?
            .enumerate()
        {
            let (candidates, _candidates_storage) =
                ctx.with_scoped_storage("SLDPRT candidates workspace", || {
                    ctx.collect_btree_set(
                        face.surface_references.iter().map(
                            crate::tessellation::PersistentSurfaceReference::feature_source_id,
                        ),
                        "index SLDPRT display surface sources",
                    )
                })?;
            if candidates.len() > 1 {
                conflict_storage.with_storage(|| {
                    let message = conflicting_display_reference(
                        ctx,
                        display_stream.as_str(),
                        table_index,
                        &candidates,
                    )?;
                    ctx.reserve_vec(
                        &mut conflicting_display_references,
                        1,
                        "collect SLDPRT conflicting display references",
                    )?;
                    conflicting_display_references.push(message);
                    Ok::<_, CodecError>(())
                })?;
            }
        }
        let (resolved, _appearance_storage) = ctx
            .with_scoped_storage("SLDPRT resolved display appearance workspace", || {
                crate::appearance::resolve_display_appearances(ctx, scan, display, &display_faces)
            })?;
        for source in ctx.admit_iter(
            resolved.matched_feature_sources,
            "index SLDPRT matched appearance sources",
        )? {
            matched_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut matched_feature_sources,
                    source,
                    "index SLDPRT matched appearance sources",
                )
            })?;
        }
        let mut display_links = Vec::new();
        ctx.reserve_vec(
            &mut display_links,
            display_faces.len(),
            "collect SLDPRT display links",
        )?;
        for (table_index, display_face) in ctx
            .admit_iter(display_faces, "scan SLDPRT display faces")?
            .enumerate()
        {
            let id = ctx.format_retained(
                format_args!(
                    "sldprt:displaylist:record#{}:{table_index}",
                    display.ordinal()
                ),
                "retain SLDPRT display tessellation identity",
            )?;
            if let Some(identity) = display_face.persistent_surface_identity(ctx)? {
                persistent_storage.with_storage(|| {
                    let mut trailing_fields = Vec::new();
                    ctx.extend_from_slice(
                        &mut trailing_fields,
                        &identity.trailing_fields,
                        "copy SLDPRT persistent face identity fields",
                    )?;
                    ctx.reserve_vec(
                        &mut persistent_face_bindings,
                        1,
                        "collect SLDPRT persistent face bindings",
                    )?;
                    persistent_face_bindings.push(crate::tessellation::PersistentFaceBinding {
                        tessellation: ctx
                            .copy_retained_text(&id, "copy SLDPRT persistent display identity")?,
                        identity: crate::brep::PersistentFaceIdentity {
                            feature_source_id: identity.feature_source_id,
                            local_id: identity.local_id,
                            trailing_fields,
                        },
                    });
                    Ok::<_, CodecError>(())
                })?;
            }
            crate::annotations::note(
                ctx,
                &mut annotations,
                id.as_str(),
                display_stream,
                u64_from_index(display_face.table.start()),
                "displaylist_tessellation",
                Exactness::ByteExact,
            )?;
            display_links.push(ctx.copy_retained_text(&id, "copy SLDPRT display link identity")?);
            if let Some(definition) = ctx.get_btree_map(
                &resolved.by_face,
                &table_index,
                "look up SLDPRT face appearances",
            )? {
                let source_entity_id = ctx.format_retained(
                    format_args!("{}::DisplayFace[{table_index}]", display_stream.as_str()),
                    "retain SLDPRT DisplayFace source identity",
                )?;
                let appearance = ensure_display_appearance(
                    ctx,
                    &mut ir,
                    definition,
                    display.ordinal(),
                    &mut annotations,
                )?;
                ctx.reserve_vec(
                    &mut ir.model.appearance_bindings,
                    1,
                    "admit SLDPRT display appearance binding",
                )?;
                ir.model.appearance_bindings.push(AppearanceBinding {
                    id: cadmpeg_ir::ids::AppearanceBindingId::compose(
                        &cadmpeg_ir::identity_namespace!("sldprt", "appearance", "binding"),
                        cadmpeg_ir::identity_key!("display:")
                            .then(display.ordinal())
                            .colon(table_index),
                    ),
                    target: AppearanceTarget::Tessellation(
                        ctx.copy_retained_text(&id, "copy SLDPRT display appearance target")?,
                    ),
                    appearance,
                    source_entity_id: Some(source_entity_id),
                    object_type: Some("DisplayFace".into()),
                    visible: None,
                    channels: BTreeMap::new(),
                });
            }
            let mesh = display_face.mesh;
            ctx.reserve_vec(
                &mut ir.model.tessellations,
                1,
                "admit SLDPRT display tessellation",
            )?;
            ir.model.tessellations.push(
                mesh.into_tessellation(
                    cadmpeg_ir::tessellation::TessellationId::mint(id).map_err(|error| {
                        CodecError::malformed(format_args!("invalid display tessellation: {error}"))
                    })?,
                )
                .map_err(|error| {
                    CodecError::malformed(format_args!("invalid display tessellation: {error}"))
                })?,
            );
        }
        let display_id = UnknownId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "displaylist", "record"),
            display.ordinal(),
        );
        crate::annotations::note(
            ctx,
            &mut annotations,
            display_id.as_str(),
            display_stream,
            0,
            "displaylist_tessellation",
            Exactness::Unknown,
        )?;
        ctx.reserve_vec(&mut unknowns, 1, "retain SLDPRT display unknown")?;
        unknowns.push(UnknownRecord::retained(
            display_id,
            0,
            ctx.copy_retained(display.payload(), "retain SLDPRT display section")?,
            display_links,
        ));
    }
    if let Some(message) = appearance_assignment_loss_message(
        ctx,
        &feature_appearance_sources,
        &matched_feature_sources,
        &conflicting_display_references,
    )? {
        ctx.reserve_vec(&mut pmi_losses, 1, "append SLDPRT appearance loss")?;
        pmi_losses.push(SldprtLossCode::AppearanceAssignmentUnresolved.note(message));
    }
    let mut assigned_tessellations = crate::tessellation::assign_persistent_owners(
        ctx,
        &mut ir.model,
        &face_identities,
        &persistent_face_bindings,
    )?;
    let remaining_assignments =
        crate::tessellation::assign_unique_surface_owners(ctx, &mut ir.model)?;
    ctx.extend_vec(
        &mut assigned_tessellations,
        remaining_assignments,
        "merge SLDPRT assigned tessellations",
    )?;
    let mut annotation_builder = AnnotationBuilder::resume(annotations);
    for id in ctx.admit_iter(
        assigned_tessellations,
        "annotate SLDPRT assigned tessellations",
    )? {
        annotation_builder.field_exactness(ctx, id.as_str(), "body", Exactness::Derived)?;
        annotation_builder.field_exactness(ctx, id.as_str(), "faces", Exactness::Derived)?;
    }
    let mut annotations = annotation_builder.build();
    // Unknown record identities already retained, kept current as source
    // blocks are retained.
    let mut retained_ids_storage = ctx.reserve_scoped(0, "SLDPRT retained_ids lookup workspace")?;
    let mut retained_ids = BTreeSet::new();
    for record in ctx.admit_iter(&unknowns, BLOCKS)? {
        retained_ids_storage.with_storage(|| {
            let id = record.id().try_clone_for_decode(ctx, BLOCKS)?;
            ctx.insert_btree_set(&mut retained_ids, id, BLOCKS)
        })?;
    }
    for source_block in ctx.admit_iter(&mut scan.blocks, BLOCKS)? {
        let id = UnknownId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "file", "block"),
            source_block.offset,
        );
        if !retained_ids_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut retained_ids,
                id.try_clone_for_decode(ctx, BLOCKS)?,
                BLOCKS,
            )
        })? {
            continue;
        }
        crate::annotations::note(
            ctx,
            &mut annotations,
            id.as_str(),
            source_block.section.source_stream(),
            u64_from_index(source_block.offset),
            source_block.family.label(),
            Exactness::ByteExact,
        )?;
        ctx.push_vec(
            &mut (unknowns),
            UnknownRecord::retained(id, 0, std::mem::take(&mut source_block.payload), Vec::new()),
            "collect SLDPRT decoded vector items",
        )?;
    }
    for source_stream in
        ctx.admit_iter(&mut scan.compound_streams, "retain SLDPRT compound streams")?
    {
        let id = UnknownId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "file", "compound-stream"),
            source_stream.directory_id,
        );
        crate::annotations::note(
            ctx,
            &mut annotations,
            id.as_str(),
            &source_stream.path,
            0,
            container::payload_family(
                ctx,
                &source_stream.payload,
                "classify SLDPRT compound source payload",
            )?
            .label(),
            Exactness::ByteExact,
        )?;
        ctx.push_vec(
            &mut (unknowns),
            UnknownRecord::retained(
                id,
                0,
                std::mem::take(&mut source_stream.payload),
                Vec::new(),
            ),
            "collect SLDPRT decoded vector items",
        )?;
    }
    let mut opaque_storage = ctx.reserve_scoped(0, "SLDPRT opaque geometry index workspace")?;
    let mut opaque_links = BTreeMap::<&str, Vec<String>>::new();
    for surface in ctx.admit_iter(&ir.model.surfaces, "scan SLDPRT build_geometry_ir values")? {
        if let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
            record: Some(record),
        }) = &surface.geometry
        {
            add_opaque_link(
                ctx,
                &mut opaque_links,
                &mut opaque_storage,
                record.as_str(),
                surface.id.as_str(),
            )?;
        }
    }
    for curve in ctx.admit_iter(&ir.model.curves, "scan SLDPRT build_geometry_ir values")? {
        if let cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
            record: Some(record),
        }) = &curve.geometry
        {
            add_opaque_link(
                ctx,
                &mut opaque_links,
                &mut opaque_storage,
                record.as_str(),
                curve.id.as_str(),
            )?;
        }
    }
    if !opaque_links.is_empty() {
        let (unknown_positions, unknown_position_storage) = ctx.collect_scoped_string_map(
            unknowns.len(),
            unknowns
                .iter()
                .enumerate()
                .rev()
                .map(|(position, record)| (record.id().as_str(), position)),
            "index SLDPRT opaque geometry records",
        )?;
        let (mut linked_positions, mut position_storage) =
            ctx.scoped_vector_storage(0, "SLDPRT opaque geometry link positions")?;
        for (record_id, links) in
            ctx.admit_iter(opaque_links, "append SLDPRT opaque geometry links")?
        {
            let Some(&position) = ctx.get_hash_map(
                &unknown_positions,
                record_id,
                "append SLDPRT opaque geometry links",
            )?
            else {
                return Err(CodecError::malformed(format_args!(
                    "opaque geometry record {record_id} was not retained"
                )));
            };
            position_storage.with_storage(|| {
                ctx.push_vec(
                    &mut linked_positions,
                    (position, links),
                    "SLDPRT opaque geometry link positions",
                )
            })?;
        }
        drop((unknown_positions, unknown_position_storage));
        for (position, links) in
            ctx.admit_iter(linked_positions, "append SLDPRT opaque geometry links")?
        {
            ctx.extend_vec(
                unknowns[position].links_mut(),
                links,
                "append SLDPRT opaque geometry links",
            )?;
        }
    }
    drop(opaque_storage);
    preserve_source_image(ctx, scan, &mut annotations, &mut unknowns)?;
    // Sort arenas for the order-sensitive loss scans that follow; the local
    // digests are stamped once, in `decode_result`, after native unknown
    // records are attached.
    ir.finalize(ctx)?;
    Ok((ir, annotations, unknowns, pmi_losses, native))
}

fn assign_native_configuration_indices(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    native: &mut crate::native::SldprtNative,
) -> Result<(), CodecError> {
    let (result, _workspace) = ctx.with_scoped_storage(
        "SLDPRT native configuration lookup workspace",
        || -> Result<(), CodecError> {
            const OPERATION: &str = "assign SLDPRT native configuration indices";
            // Each native configuration record by identity, the first in history
            // order answering; the assignments are applied in configuration order.
            let mut assignments = Vec::new();
            {
                let mut records = BTreeMap::<&str, (usize, usize)>::new();
                for (history_index, history) in ctx
                    .admit_iter(&native.feature_histories, OPERATION)?
                    .enumerate()
                {
                    for (record_index, record) in ctx
                        .admit_iter(&history.configurations, OPERATION)?
                        .enumerate()
                    {
                        if !ctx.contains_key_btree_map(&records, record.id.as_str(), OPERATION)? {
                            ctx.insert_btree_map(
                                &mut records,
                                record.id.as_str(),
                                (history_index, record_index),
                                OPERATION,
                            )?;
                        }
                    }
                }
                for configuration in ctx.admit_iter(&ir.model.configurations, OPERATION)? {
                    let Some(native_ref) = configuration.native_ref.as_deref() else {
                        continue;
                    };
                    if let Some(&position) = ctx.get_btree_map(&records, native_ref, OPERATION)? {
                        ctx.push_vec(
                            &mut assignments,
                            (position, configuration.source_index),
                            OPERATION,
                        )?;
                    }
                }
            }
            for ((history_index, record_index), source_index) in
                ctx.admit_iter(assignments, OPERATION)?
            {
                if let Some(record) = native
                    .feature_histories
                    .get_mut(history_index)
                    .and_then(|history| history.configurations.get_mut(record_index))
                {
                    record.source_index = source_index;
                }
            }
            Ok(())
        },
    )?;
    Ok(result)
}

fn source_meta(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    classification: &crate::dialect::LayerClassification,
    header: Option<&StreamHeader>,
    display: crate::tessellation::Summary,
) -> Result<SourceMeta, CodecError> {
    let mut attributes = BTreeMap::new();
    ctx.insert_btree_map(
        &mut (attributes),
        cadmpeg_core::nonblank_literal!("outer_version"),
        format!("0x{:08x}", scan.version),
        "insert SLDPRT ordered entry",
    )?;
    if display.vertices > 0 {
        ctx.insert_btree_map(
            &mut (attributes),
            cadmpeg_core::nonblank_literal!("displaylist_vertices"),
            display.vertices.to_string(),
            "insert SLDPRT ordered entry",
        )?;
        ctx.insert_btree_map(
            &mut (attributes),
            cadmpeg_core::nonblank_literal!("displaylist_triangles"),
            display.triangles.to_string(),
            "insert SLDPRT ordered entry",
        )?;
    }
    ctx.insert_btree_map(
        &mut (attributes),
        cadmpeg_core::nonblank_literal!("block_count"),
        scan.blocks.len().to_string(),
        "insert SLDPRT ordered entry",
    )?;
    ctx.insert_btree_map(
        &mut (attributes),
        cadmpeg_core::nonblank_literal!("compound_stream_count"),
        scan.compound_streams.len().to_string(),
        "insert SLDPRT ordered entry",
    )?;
    if let Some(site) = container::select_active_parasolid_site(ctx, scan)? {
        ctx.insert_btree_map(
            &mut (attributes),
            cadmpeg_core::nonblank_literal!("active_parasolid_block"),
            ctx.copy_retained_text(
                site.source_stream().as_str(),
                "retain SLDPRT active site name",
            )?,
            "insert SLDPRT ordered entry",
        )?;
    } else {
        ctx.insert_btree_map(
            &mut (attributes),
            cadmpeg_core::nonblank_literal!("sldprt_active_partition_unresolved"),
            "true".into(),
            "insert SLDPRT ordered entry",
        )?;
    }
    if let Some(header) = header {
        ctx.insert_btree_map(
            &mut (attributes),
            cadmpeg_core::nonblank_literal!("parasolid_schema"),
            ctx.copy_retained_text(header.schema.value(), "retain SLDPRT source schema")?,
            "insert SLDPRT ordered entry",
        )?;
        ctx.insert_btree_map(
            &mut (attributes),
            cadmpeg_core::nonblank_literal!("parasolid_description"),
            ctx.copy_retained_text(
                header.description.as_str(),
                "retain SLDPRT source description",
            )?,
            "insert SLDPRT ordered entry",
        )?;
    }
    add_preview_metadata(ctx, scan, &mut attributes)?;
    add_solidworks_xml_metadata(ctx, scan, &mut attributes)?;
    Ok(SourceMeta::classified(
        classification
            .layers()
            .try_clone_for_decode(ctx, "copy dialect layers")?,
        attributes,
    ))
}

fn add_preview_metadata(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    attributes: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
) -> Result<(), CodecError> {
    let mut png_index = 0;
    let mut bmp_index = 0;
    for section in scan.sections(ctx)? {
        let payload = section.payload();
        match container::payload_family(ctx, payload, "classify SLDPRT preview payload")? {
            container::PayloadFamily::PngPreview => {
                if payload.get(8..16) != Some(&[0, 0, 0, 13, b'I', b'H', b'D', b'R']) {
                    continue;
                }
                let Some(width) = View::u32_be_at(payload, 16) else {
                    continue;
                };
                let Some(height) = View::u32_be_at(payload, 20) else {
                    continue;
                };
                let Some(fields) = payload.get(24..29) else {
                    continue;
                };
                let key = |field: &str| {
                    cadmpeg_core::nonblank_literal!(ctx, "png_preview_{png_index}_{field}")
                };
                ctx.insert_btree_map(
                    &mut *attributes,
                    key("width")?,
                    width.to_string(),
                    "insert SLDPRT ordered entry",
                )?;
                ctx.insert_btree_map(
                    &mut *attributes,
                    key("height")?,
                    height.to_string(),
                    "insert SLDPRT ordered entry",
                )?;
                ctx.insert_btree_map(
                    &mut *attributes,
                    key("bit_depth")?,
                    fields[0].to_string(),
                    "insert SLDPRT ordered entry",
                )?;
                ctx.insert_btree_map(
                    &mut *attributes,
                    key("color_type")?,
                    fields[1].to_string(),
                    "insert SLDPRT ordered entry",
                )?;
                ctx.insert_btree_map(
                    &mut *attributes,
                    key("compression")?,
                    fields[2].to_string(),
                    "insert SLDPRT ordered entry",
                )?;
                ctx.insert_btree_map(
                    &mut *attributes,
                    key("filter")?,
                    fields[3].to_string(),
                    "insert SLDPRT ordered entry",
                )?;
                ctx.insert_btree_map(
                    &mut *attributes,
                    key("interlace")?,
                    fields[4].to_string(),
                    "insert SLDPRT ordered entry",
                )?;
                png_index += 1;
            }
            container::PayloadFamily::BmpThumbnail => {
                let (Some(width), Some(height), Some(image_size)) = (
                    View::i32_le_at(payload, 8),
                    View::i32_le_at(payload, 12),
                    View::u32_le_at(payload, 24),
                ) else {
                    continue;
                };
                let (Some(planes), Some(bits_per_pixel), Some(compression)) = (
                    View::u16_le_at(payload, 16),
                    View::u16_le_at(payload, 18),
                    View::u32_le_at(payload, 20),
                ) else {
                    continue;
                };
                let key = |field: &str| {
                    cadmpeg_core::nonblank_literal!(ctx, "bmp_thumbnail_{bmp_index}_{field}")
                };
                ctx.insert_btree_map(
                    &mut *attributes,
                    key("width")?,
                    width.to_string(),
                    "insert SLDPRT ordered entry",
                )?;
                ctx.insert_btree_map(
                    &mut *attributes,
                    key("height")?,
                    height.to_string(),
                    "insert SLDPRT ordered entry",
                )?;
                ctx.insert_btree_map(
                    &mut *attributes,
                    key("planes")?,
                    planes.to_string(),
                    "insert SLDPRT ordered entry",
                )?;
                ctx.insert_btree_map(
                    &mut *attributes,
                    key("bit_count")?,
                    bits_per_pixel.to_string(),
                    "insert SLDPRT ordered entry",
                )?;
                ctx.insert_btree_map(
                    &mut *attributes,
                    key("compression")?,
                    compression.to_string(),
                    "insert SLDPRT ordered entry",
                )?;
                ctx.insert_btree_map(
                    &mut *attributes,
                    key("image_size")?,
                    image_size.to_string(),
                    "insert SLDPRT ordered entry",
                )?;
                bmp_index += 1;
            }
            _ => {}
        }
    }
    ctx.insert_btree_map(
        &mut *attributes,
        cadmpeg_core::nonblank_literal!("png_preview_count"),
        png_index.to_string(),
        "insert SLDPRT ordered entry",
    )?;
    ctx.insert_btree_map(
        &mut *attributes,
        cadmpeg_core::nonblank_literal!("bmp_thumbnail_count"),
        bmp_index.to_string(),
        "insert SLDPRT ordered entry",
    )?;
    Ok(())
}

fn add_solidworks_xml_metadata(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    attributes: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
) -> Result<(), CodecError> {
    let active_configuration_name = container::active_configuration_name_ref(scan);
    if let Some(envelope) = container::solidworks_envelope(scan) {
        for (key, value) in [
            (
                cadmpeg_core::nonblank_literal!("sw_creation_time_unix"),
                envelope.creation_time.as_ref(),
            ),
            (
                cadmpeg_core::nonblank_literal!("sw_path"),
                envelope.path.as_ref(),
            ),
            (
                cadmpeg_core::nonblank_literal!("sw_name"),
                envelope.model_name.as_ref(),
            ),
        ] {
            if let Some(value) = value {
                ctx.insert_btree_map(
                    &mut *attributes,
                    key,
                    ctx.copy_retained_text(value, "retain SLDPRT XML metadata")?,
                    "insert SLDPRT ordered entry",
                )?;
            }
        }
        if let Some(value) = active_configuration_name {
            ctx.insert_btree_map(
                &mut *attributes,
                cadmpeg_core::nonblank_literal!("sw_configuration_name"),
                ctx.copy_retained_text(value, "retain SLDPRT configuration name")?,
                "insert SLDPRT ordered entry",
            )?;
        } else if let Some(value) = &envelope.configuration_name {
            ctx.insert_btree_map(
                &mut *attributes,
                cadmpeg_core::nonblank_literal!("sw_configuration_name"),
                ctx.copy_retained_text(value, "retain SLDPRT configuration name")?,
                "insert SLDPRT ordered entry",
            )?;
        }
        for (key, value) in ctx.admit_iter(
            &envelope.configuration_attributes,
            "scan SLDPRT add_solidworks_xml_metadata values",
        )? {
            let name = ctx.copy_retained_text(key, "retain SLDPRT configuration key")?;
            let name = cadmpeg_core::text::NonBlankString::for_decode(
                ctx,
                name,
                "validate nonblank text",
            )?
            .ok_or_else(|| CodecError::Malformed("invalid SLDPRT configuration key".into()))?;
            let value = ctx.copy_retained_text(value, "retain SLDPRT configuration value")?;
            ctx.insert_btree_map(
                attributes,
                name,
                value,
                "copy SLDPRT configuration attribute",
            )?;
        }
    }
    Ok(())
}

fn build_geometry_report(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    decoded: &mut Brep,
    classification: &crate::dialect::LayerClassification,
    notes: Vec<String>,
) -> Result<DecodeBody, CodecError> {
    let s = &decoded.stats;
    let mut losses = Vec::new();

    if s.unknown_surface_faces > 0 || s.unknown_procedural_supports > 0 {
        let mut message_storage = ctx.reserve_scoped(0, "SLDPRT temporary vector storage")?;
        let mut message = Vec::new();
        if s.unknown_surface_faces > 0 {
            message_storage.with_storage(|| {
                ctx.push_vec(
                    &mut (message),
                    format!(
                "{} face(s) rest on a support surface whose stored carrier this codec does not \
                 type; the face, its loops, and trims are emitted with an unknown-geometry \
                 surface linking to the preserved record bytes. Topology is transferred; the \
                 underlying surface shape is not.",
                s.unknown_surface_faces
            ),
                    "collect SLDPRT decoded vector items",
                )
            })?;
        }
        if s.unknown_procedural_supports > 0 {
            message_storage.with_storage(|| {
                ctx.push_vec(
                    &mut (message),
                    format!(
                "{} untyped surface carrier(s) are retained as opaque hidden supports of exact \
                 procedural constructions.",
                s.unknown_procedural_supports
            ),
                    "collect SLDPRT decoded vector items",
                )
            })?;
        }
        ctx.reserve_vec(&mut losses, 1, "append SLDPRT geometry loss")?;
        losses.push(SldprtLossCode::GeometryFaceSupportSurfaceUntyped.note(message.join(" ")));
    }
    ctx.append_vec(
        &mut losses,
        &mut decoded.losses,
        "move SLDPRT B-rep losses to report",
    )?;
    if s.unknown_curve_edges > 0 {
        ctx.reserve_vec(&mut losses, 1, "append SLDPRT geometry loss")?;
        losses.push(
            SldprtLossCode::GeometryEdgeSupportCurveUntyped.note(format!(
                "{} edge(s) reference an untyped support curve; topology references an opaque \
                 curve carrier linked to the retained partition.",
                s.unknown_curve_edges
            )),
        );
    }
    if s.ambiguous_pcurve_parameters > 0 {
        ctx.reserve_vec(&mut losses, 1, "append SLDPRT geometry loss")?;
        losses.push(SldprtLossCode::GeometryPcurveAmbiguous.note(format!(
            "{} pcurve(s) were withheld because more than one geometric parameter satisfies the stored edge or ruling geometry; the decoder does not choose by residual order.",
            s.ambiguous_pcurve_parameters
        )));
    }
    if s.off_surface_nurbs_pcurves > 0 {
        ctx.reserve_vec(&mut losses, 1, "append SLDPRT geometry loss")?;
        losses.push(SldprtLossCode::TopologyPcurveCarrierOffSurface.note(format!(
            "{} NURBS edge carrier(s) have vertex ranges off their bound B-spline surface; pcurve derivation is withheld because the defect is upstream of parameter-space geometry.",
            s.off_surface_nurbs_pcurves
        )));
    }
    if s.unresolved_face_colors > 0 {
        ctx.reserve_vec(&mut losses, 1, "append SLDPRT geometry loss")?;
        losses.push(SldprtLossCode::AppearanceFaceColorUnresolved.note(format!(
            "{} face-color binding(s) were withheld because the current face and link records do not select one consistent framed color record.",
            s.unresolved_face_colors
        )));
    }
    if s.ambiguous_face_owners > 0 {
        ctx.reserve_vec(&mut losses, 1, "append SLDPRT geometry loss")?;
        losses.push(SldprtLossCode::TopologyFaceOwnerAmbiguous.note(format!(
            "{} face owner(s) have non-equivalent bridge uses; all uses for each owner remain unresolved.",
            s.ambiguous_face_owners
        )));
    }
    if s.unclaimed_faces > 0 {
        ctx.reserve_vec(&mut losses, 1, "append SLDPRT geometry loss")?;
        losses.push(SldprtLossCode::TopologyFaceUnclaimed.note(format!(
            "{} canonical face(s) are not claimed by an explicit body relation; the decoder withholds them rather than inventing body membership.",
            s.unclaimed_faces
        )));
    }
    if s.synthetic_body_grouping {
        ctx.reserve_vec(&mut losses, 1, "append SLDPRT geometry loss")?;
        losses.push(
            SldprtLossCode::TopologyBodyHierarchyDerived.note(
                "No body record was available; one body/region/shell hierarchy was derived."
                    .to_string(),
            ),
        );
    }
    append_swift_pmi_losses(ctx, scan, &mut losses)?;
    classification.append_losses(ctx, &mut losses)?;
    Ok(DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses,
        notes,
        transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
    })
}

fn build_metadata_ir(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    classification: &crate::dialect::LayerClassification,
    form_padding: Option<usize>,
    admitted_entities: &mut u64,
) -> Result<BuiltIr, CodecError> {
    let mut ir = CadIr::empty();
    let mut unknowns = Vec::new();
    let mut annotations = Annotations::default();
    let mut pmi_losses = Vec::new();
    let mut histories = crate::history::histories(ctx, scan, &mut annotations, &mut pmi_losses)?;
    let mut lanes = crate::resolved_features::assembly::lanes(ctx, scan, &mut annotations)?;
    let mut supplemental_config_lanes =
        crate::resolved_features::assembly::supplemental_config_lanes(ctx, scan, &mut annotations)?;
    crate::resolved_features::classes::bind_history_classes(ctx, &mut histories, &lanes)?;
    crate::resolved_features::bindings::bind_scalar_operands(ctx, &histories, &mut lanes)?;
    crate::resolved_features::bindings::bind_scalar_operands(
        ctx,
        &histories,
        &mut supplemental_config_lanes,
    )?;
    let pmi_dimensions = crate::pmi::dimensions(ctx, scan, &mut annotations, &mut pmi_losses)?;
    ir.model.pmi = crate::swift::annotations(ctx, scan, &mut annotations, None, None)?;
    let crate::resolved_features::sketch_projection::ProjectedSketches {
        sketches,
        entities: sketch_entities,
        constraints: sketch_constraints,
    } = crate::resolved_features::sketch_projection::sketches(ctx, scan, &mut annotations)?;
    let mut model_attributes = crate::metadata::attributes(ctx, scan, &mut annotations)?;
    let custom_properties = crate::history::project::custom_property_attributes(ctx, &histories)?;
    ctx.extend_vec(
        &mut model_attributes,
        custom_properties,
        "append SLDPRT custom properties",
    )?;
    ir.model.attributes = model_attributes;
    ir.model.sketches = sketches;
    ir.model.sketch_entities = sketch_entities;
    ir.model.sketch_constraints = sketch_constraints;
    let mut attributes = BTreeMap::new();
    ctx.insert_btree_map(
        &mut (attributes),
        cadmpeg_core::nonblank_literal!("outer_version"),
        format!("0x{:08x}", scan.version),
        "insert SLDPRT ordered entry",
    )?;
    ctx.insert_btree_map(
        &mut (attributes),
        cadmpeg_core::nonblank_literal!("block_count"),
        scan.blocks.len().to_string(),
        "insert SLDPRT ordered entry",
    )?;
    add_solidworks_xml_metadata(ctx, scan, &mut attributes)?;

    if let Some(site) = container::select_active_parasolid_site(ctx, scan)? {
        let id = site.section.native_id();
        let offset = match site.section {
            container::Section::Block(block) => u64_from_index(block.offset),
            container::Section::Compound(_) => 0,
        };
        ctx.insert_btree_map(
            &mut (attributes),
            cadmpeg_core::nonblank_literal!("active_parasolid_block"),
            ctx.copy_retained_text(
                site.source_stream().as_str(),
                "retain SLDPRT metadata active site name",
            )?,
            "insert SLDPRT ordered entry",
        )?;
        ctx.insert_btree_map(
            &mut (attributes),
            cadmpeg_core::nonblank_literal!("parasolid_schema"),
            ctx.copy_retained_text(site.header.schema.value(), "retain SLDPRT metadata schema")?,
            "insert SLDPRT ordered entry",
        )?;
        crate::annotations::note(
            ctx,
            &mut annotations,
            id.as_str(),
            site.source_stream(),
            0,
            "parasolid_stream",
            Exactness::Unknown,
        )?;
        ctx.reserve_vec(&mut unknowns, 1, "retain SLDPRT metadata site")?;
        unknowns.push(UnknownRecord::retained(
            id,
            offset,
            ctx.copy_retained(site.payload, "retain SLDPRT active site")?,
            Vec::new(),
        ));
    }

    ir.source = Some(SourceMeta::classified(
        classification
            .layers()
            .try_clone_for_decode(ctx, "copy dialect layers")?,
        attributes,
    ));
    project_design_history(
        ctx,
        &mut ir,
        DesignHistoryInput {
            histories: &histories,
            lanes: &lanes,
            pmi_dimensions: &pmi_dimensions,
        },
        scan,
        form_padding,
        &mut pmi_losses,
    )?;
    crate::resolved_features::operations::bind_feature_operations(
        ctx,
        &mut ir.model.features,
        &histories,
        &lanes,
        form_padding,
    )?;
    crate::pmi::apply_to_parameters(
        ctx,
        &mut ir.model.parameters,
        &ir.model.features,
        &pmi_dimensions,
    )?;
    let (identity_lanes, identity_lane_storage) = ctx
        .with_scoped_storage("SLDPRT parameter identity lane workspace", || {
            parameter_identity_lanes(ctx, &lanes)
        })?;
    crate::resolved_features::projections::bind_parameter_scalars(
        ctx,
        &mut ir.model.parameters,
        &ir.model.features,
        &histories,
        identity_lanes,
    )?;
    drop(identity_lane_storage);
    crate::resolved_features::projections::synthesize_display_relation_parameters(
        ctx,
        &mut ir.model.parameters,
        &ir.model.features,
        lanes.iter().chain(supplemental_config_lanes.iter()),
    )?;
    crate::resolved_features::projections::type_display_relation_parameters(
        ctx,
        &mut ir.model.parameters,
        &ir.model.features,
        &lanes,
    )?;
    crate::history::configuration::align_configuration_parameter_kinds(ctx, &mut ir)?;
    complete_resolved_configuration_parameter_snapshots(ctx, &mut ir)?;
    stamp_parameter_baseline(ctx, &mut ir)?;
    crate::resolved_features::profiles::bind_sketch_profiles(
        ctx,
        &mut ir.model.features,
        crate::resolved_features::profiles::SketchArenas {
            sketches: &mut ir.model.sketches,
            sketch_entities: &mut ir.model.sketch_entities,
            sketch_constraints: &mut ir.model.sketch_constraints,
            annotations: &mut annotations,
        },
        &ir.model.parameters,
        &histories,
        &lanes,
    )?;
    crate::resolved_features::bindings::bind_unresolved_detached_sketch_objects(
        ctx,
        &ir.model.features,
        &histories,
        &mut supplemental_config_lanes,
    )?;
    crate::resolved_features::projections::project_compact_edge_selections(
        ctx,
        &mut ir.model.features,
        &[],
        &supplemental_config_lanes,
    )?;
    crate::history::configuration::project_configuration_supplemental_edge_selections(
        ctx,
        &mut ir,
        &supplemental_config_lanes,
    )?;
    crate::resolved_features::profiles::project_compact_sketch_profiles(
        ctx,
        &mut ir.model.features,
        &mut ir.model.sketches,
        &mut ir.model.sketch_entities,
        &histories,
        &lanes,
        &mut pmi_losses,
    )?;
    // Marker-backed sketches can originate in either lane family. Their
    // geometry and constraints must use the same complete lane set.
    let base_lane_count = lanes.len();
    ctx.extend_vec(
        &mut lanes,
        supplemental_config_lanes,
        "merge SLDPRT feature input lanes",
    )?;
    let all_lanes = lanes;
    let lanes = &all_lanes[..base_lane_count];
    let sketch_lanes = all_lanes.as_slice();
    let (spatial_sketches, spatial_sketch_entities) =
        crate::resolved_features::markers::spatial_sketches(
            ctx,
            &mut ir.model.features,
            &histories,
            sketch_lanes,
        )?;
    ir.model.spatial_sketches = spatial_sketches;
    ir.model.spatial_sketch_entities = spatial_sketch_entities;
    crate::resolved_features::profiles::project_marker_backed_sketches(
        ctx,
        &mut ir.model.features,
        &mut ir.model.sketches,
        &mut ir.model.sketch_entities,
        &histories,
        sketch_lanes,
    )?;
    crate::resolved_features::profiles::project_sketch_block_profiles(
        ctx,
        &mut ir.model.features,
        &mut ir.model.sketches,
        &mut ir.model.sketch_entities,
        &histories,
        sketch_lanes,
    )?;
    crate::history::bind::bind_unique_sketch_feature(
        ctx,
        &mut ir.model.features,
        &ir.model.sketches,
        &histories,
    )?;
    crate::resolved_features::component_paths::project_dissected_sketches(
        ctx,
        &mut ir.model.features,
        &ir.model.sketches,
        &histories,
    )?;
    crate::resolved_features::axes::bind_profile_revolution_axes(
        ctx,
        &mut ir.model.features,
        &histories,
        lanes,
        &ir.model.sketches,
        &ir.model.surfaces,
    )?;
    crate::resolved_features::bindings::bind_pattern_inputs(
        ctx,
        &mut ir.model.features,
        &histories,
        lanes,
    )?;
    crate::resolved_features::bindings::bind_sweep_adjacent_profiles(
        ctx,
        &mut ir.model.features,
        &histories,
        lanes,
    )?;
    crate::resolved_features::dimensions::project_dimensioned_sketch_geometry(
        ctx,
        &mut ir.model.sketch_entities,
        &ir.model.sketches,
        &ir.model.surfaces,
        &ir.model.features,
        &ir.model.parameters,
        sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_spatial_relation_bindings(
        ctx,
        &mut ir.model.spatial_sketch_constraints,
        &mut ir.model.spatial_sketch_entities,
        &ir.model.spatial_sketches,
        &ir.model.features,
        &ir.model.parameters,
        sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_relation_point_geometry(
        ctx,
        &mut ir.model.sketch_entities,
        &ir.model.sketches,
        &ir.model.features,
        sketch_lanes,
    )?;
    crate::resolved_features::dimensions::project_relation_point_dimensioned_circles(
        ctx,
        &mut ir.model.sketch_entities,
        &ir.model.features,
        &ir.model.parameters,
        sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_relation_solved_line_geometry(
        ctx,
        &mut ir.model.sketch_entities,
        &ir.model.sketches,
        &ir.model.features,
        &ir.model.parameters,
        sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_relation_solved_point_geometry(
        ctx,
        &mut ir.model.sketch_entities,
        &ir.model.sketches,
        &ir.model.features,
        &ir.model.parameters,
        sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_relation_bindings(
        ctx,
        &mut ir.model.sketch_constraints,
        &ir.model.sketches,
        &ir.model.features,
        &ir.model.sketch_entities,
        &ir.model.parameters,
        sketch_lanes,
    )?;
    crate::resolved_features::holes::project_profiled_hole_constructions(
        ctx,
        &mut ir.model.features,
        &ir.model.sketch_entities,
        &histories,
        lanes,
    )?;
    crate::resolved_features::holes::project_hole_position_sketches(
        ctx,
        &mut ir.model.features,
        &ir.model.sketches,
        &ir.model.sketch_entities,
        &histories,
        lanes,
    )?;
    crate::resolved_features::holes::project_spatial_hole_position_sketches(
        ctx,
        &mut ir.model.features,
        &ir.model.spatial_sketches,
        &ir.model.spatial_sketch_entities,
        &ir.model.surfaces,
        &histories,
        lanes,
    )?;
    crate::resolved_features::holes::project_topological_hole_constructions(
        ctx,
        &mut ir.model.features,
        &crate::resolved_features::holes::HoleTopology {
            surfaces: &ir.model.surfaces,
            faces: &ir.model.faces,
            loops: &ir.model.loops,
            coedges: &ir.model.coedges,
            edges: &ir.model.edges,
            vertices: &ir.model.vertices,
            points: &ir.model.points,
        },
    )?;
    crate::resolved_features::holes::project_hole_axes(
        ctx,
        &mut ir.model.features,
        &ir.model.sketch_entities,
        &crate::resolved_features::holes::HoleTopology {
            surfaces: &ir.model.surfaces,
            faces: &ir.model.faces,
            loops: &ir.model.loops,
            coedges: &ir.model.coedges,
            edges: &ir.model.edges,
            vertices: &ir.model.vertices,
            points: &ir.model.points,
        },
        &histories,
        lanes,
    )?;
    crate::resolved_features::holes::project_hole_topology_axes(
        ctx,
        &mut ir.model.features,
        &crate::resolved_features::holes::HoleTopology {
            surfaces: &ir.model.surfaces,
            faces: &ir.model.faces,
            loops: &ir.model.loops,
            coedges: &ir.model.coedges,
            edges: &ir.model.edges,
            vertices: &ir.model.vertices,
            points: &ir.model.points,
        },
    )?;
    crate::resolved_features::holes::project_bore_backed_position_sketches(
        ctx,
        &mut ir.model.features,
        &mut ir.model.sketches,
        &mut ir.model.sketch_entities,
        &ir.model.surfaces,
        &histories,
        lanes,
    )?;
    crate::resolved_features::relation_geometry::project_relation_bindings(
        ctx,
        &mut ir.model.sketch_constraints,
        &ir.model.sketches,
        &ir.model.features,
        &ir.model.sketch_entities,
        &ir.model.parameters,
        sketch_lanes,
    )?;
    crate::resolved_features::projections::project_unbound_cosmetic_thread_faces(
        ctx,
        &mut ir.model.features,
        &histories,
        lanes,
        &ir.model.faces,
        &ir.model.surfaces,
    )?;
    crate::resolved_features::projections::project_unbound_offset_plane_faces(
        ctx,
        &mut ir.model.features,
        &ir.model.faces,
        &ir.model.surfaces,
    )?;
    sync_active_configuration_resolutions(ctx, &mut ir)?;
    crate::history::bind::order_features_for_regeneration(ctx, &mut ir.model.features)?;
    let configuration_losses = crate::history::configuration::project_configuration_sketch_states(
        ctx,
        &mut ir,
        &histories,
        lanes,
        &mut annotations,
    )?;
    ctx.extend_vec(
        &mut pmi_losses,
        configuration_losses,
        "append SLDPRT configuration PMI losses",
    )?;
    crate::history::configuration::inherit_configuration_reference_plane_states(ctx, &mut ir)?;
    crate::history::bind::order_model_features_for_regeneration(ctx, &mut ir)?;
    stamp_feature_baseline(ctx, &mut ir)?;
    let native = crate::native::SldprtNative {
        feature_histories: histories,
        feature_input_lanes: all_lanes,
        pmi_dimensions,
    };
    ctx.admit_entities(
        u64_from_index(ir.model.entity_count()),
        admitted_entities,
        "admit SLDPRT entities",
    )?;
    native.store(ctx, ir.native.namespace_mut("sldprt"))?;
    stamp_sketch_baseline(ctx, &mut ir, &native.feature_input_lanes)?;
    bind_active_configuration_partition(ctx, &mut ir)?;
    mark_active_configuration(ctx, &mut ir)?;
    stamp_configuration_baseline(ctx, &mut ir)?;
    snapshot_active_configuration(ctx, &mut ir)?;
    preserve_source_image(ctx, scan, &mut annotations, &mut unknowns)?;
    // Sort arenas for the order-sensitive loss scans that follow; the local
    // digests are stamped once, in `decode_result`, after native unknown
    // records are attached.
    ir.finalize(ctx)?;
    Ok((ir, annotations, unknowns, pmi_losses, native))
}

#[derive(Clone, Copy)]
struct DesignHistoryInput<'a> {
    histories: &'a [crate::records::FeatureHistory],
    lanes: &'a [crate::records::FeatureInputLane],
    pmi_dimensions: &'a [crate::records::PmiDimension],
}

fn project_design_history(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    input: DesignHistoryInput<'_>,
    scan: &ContainerScan,
    form_padding: Option<usize>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), cadmpeg_core::CodecError> {
    let DesignHistoryInput {
        histories,
        lanes,
        pmi_dimensions,
    } = input;
    let (semantic_projection, semantic_storage) = ctx.with_scoped_storage(
        "SLDPRT semantic history workspace",
        || -> Result<_, CodecError> {
            let mut semantic_projection = crate::records::charged_clone::clone_histories_charged(
                ctx,
                histories,
                "clone SLDPRT semantic histories",
            )?;
            let (scene_feature_classes, scene_storage) = ctx.with_scoped_storage(
                "SLDPRT scene feature class workspace",
                || crate::tessellation::scene_feature_classes(ctx, scan),
            )?;
            crate::history::enrich_scene_classes(
                ctx,
                &mut semantic_projection,
                &scene_feature_classes,
            )?;
            drop((scene_feature_classes, scene_storage));
            crate::history::configuration::enrich_history_semantic(
                ctx,
                &mut semantic_projection,
                lanes,
                pmi_dimensions,
                crate::history::configuration::HistoryEnrichment::Read,
            )?;
            Ok(semantic_projection)
        },
    )?;
    ir.model.semantic_annotations =
        crate::history::project::project_semantic_notes(ctx, &semantic_projection)?;
    crate::history::project::project_feature_model(ctx, &semantic_projection)?.install(
        ctx,
        &mut ir.model,
        losses,
    )?;
    crate::resolved_features::bindings::bind_pattern_inputs(
        ctx,
        &mut ir.model.features,
        &semantic_projection,
        lanes,
    )?;
    crate::history::configuration::project_compact_and_generated(
        ctx,
        &mut ir.model.features,
        &semantic_projection,
        lanes,
    )?;
    ir.model.configurations =
        crate::history::project::project_configurations_charged(ctx, &semantic_projection)?;
    drop((semantic_projection, semantic_storage));
    let (parameter_projection, parameter_storage) = ctx.with_scoped_storage(
        "SLDPRT parameter history workspace",
        || -> Result<_, CodecError> {
            let mut parameter_projection = crate::records::charged_clone::clone_histories_charged(
                ctx,
                histories,
                "clone SLDPRT parameter histories",
            )?;
            crate::resolved_features::direct_edits::enrich_history_move_face_translations(
                ctx,
                &mut parameter_projection,
                lanes,
            )?;
            crate::history::configuration::enrich_history_parameters_values_only(
                ctx,
                &mut parameter_projection,
                lanes,
            )?;
            crate::resolved_features::holes::
        enrich_history_cosmetic_thread_diameters_without_hole_constructions(
            ctx,
            &mut parameter_projection,
            lanes,
        )?;
            crate::pmi::enrich_history_parameters(ctx, &mut parameter_projection, pmi_dimensions)?;
            Ok(parameter_projection)
        },
    )?;
    ir.model.parameters =
        crate::history::parameters::project_parameters(ctx, &parameter_projection)?;
    drop((parameter_projection, parameter_storage));
    crate::history::configuration::project_configuration_design_states(
        ctx,
        ir,
        histories,
        lanes,
        pmi_dimensions,
        form_padding,
    )?;
    if let Some(source) = &mut ir.source {
        ctx.insert_btree_map(
            &mut (source.attributes),
            cadmpeg_core::nonblank_literal!("sldprt_neutral_feature_local_sha256"),
            crate::history::hash::feature_hash(ctx, &ir.model)?,
            "insert SLDPRT ordered entry",
        )?;
        ctx.insert_btree_map(
            &mut (source.attributes),
            cadmpeg_core::nonblank_literal!("sldprt_native_history_sha256"),
            crate::history::hash::history_hash(ctx, histories)?,
            "insert SLDPRT ordered entry",
        )?;
        ctx.insert_btree_map(
            &mut (source.attributes),
            cadmpeg_core::nonblank_literal!("sldprt_native_configuration_sha256"),
            crate::history::hash::native_configuration_hash(ctx, histories)?,
            "insert SLDPRT ordered entry",
        )?;
        ctx.insert_btree_map(
            &mut (source.attributes),
            cadmpeg_core::nonblank_literal!("sldprt_neutral_parameter_local_sha256"),
            crate::history::hash::parameter_hash(ctx, &ir.model.parameters)?,
            "insert SLDPRT ordered entry",
        )?;
        ctx.insert_btree_map(
            &mut (source.attributes),
            cadmpeg_core::nonblank_literal!("sldprt_native_parameter_sha256"),
            crate::history::hash::native_parameter_hash(ctx, histories)?,
            "insert SLDPRT ordered entry",
        )?;
    }

    Ok(())
}

fn parameter_identity_lanes<'a>(
    ctx: &DecodeContext<'_>,
    lanes: &'a [crate::records::FeatureInputLane],
) -> Result<Vec<&'a crate::records::FeatureInputLane>, CodecError> {
    const OPERATION: &str = "select SLDPRT parameter identity lanes";
    let mut selected = Vec::new();
    let mut eligible_count = 0;
    let mut scoped = None;
    for lane in ctx.admit_iter(lanes, OPERATION)? {
        if crate::resolved_features::assembly::is_supplemental_config_lane(lane) {
            continue;
        }
        eligible_count += 1;
        if lane.configuration.is_none() {
            ctx.push_vec(&mut selected, lane, OPERATION)?;
        } else {
            scoped = Some(lane);
        }
    }
    if selected.is_empty() && eligible_count == 1 {
        if let Some(lane) = scoped {
            ctx.push_vec(&mut selected, lane, OPERATION)?;
        }
    }
    Ok(selected)
}

fn stamp_parameter_baseline(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
    let hash = crate::history::hash::parameter_hash(ctx, &ir.model.parameters)?;
    if let Some(source) = &mut ir.source {
        ctx.insert_btree_map(
            &mut source.attributes,
            cadmpeg_core::nonblank_literal!("sldprt_neutral_parameter_local_sha256"),
            hash,
            "insert SLDPRT parameter baseline digest",
        )?;
    }
    Ok(())
}

fn complete_resolved_configuration_parameter_snapshots(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<(), CodecError> {
    for configuration in ctx.admit_iter(
        &mut ir.model.configurations,
        "complete SLDPRT configuration parameter snapshot",
    )? {
        if configuration.parameter_values.is_empty() && configuration.feature_states.is_empty() {
            continue;
        }
        for parameter in ctx.admit_iter(
            &ir.model.parameters,
            "complete SLDPRT configuration parameter snapshot",
        )? {
            let Some(value) = parameter.value.as_ref() else {
                continue;
            };
            if !parameter.dependencies.is_empty()
                || ctx.contains_key_btree_map(
                    &(configuration.parameter_values),
                    &parameter.id,
                    "test SLDPRT map key",
                )?
            {
                continue;
            }
            let id = parameter
                .id
                .try_clone_for_decode(ctx, "retain SLDPRT snapshot parameter ID")?;
            let value =
                value.try_clone_for_decode(ctx, "retain SLDPRT snapshot parameter value")?;
            ctx.insert_btree_map(
                &mut configuration.parameter_values,
                id,
                value,
                "complete SLDPRT configuration parameter snapshot",
            )?;
        }
    }
    Ok(())
}

fn mark_active_configuration(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<(), cadmpeg_core::CodecError> {
    const ACTIVE: &str = "find SLDPRT active configuration";
    let active_name = ir
        .source
        .as_ref()
        .map(|source| {
            ctx.get_btree_map(
                &source.attributes,
                "sw_configuration_name",
                "look up SLDPRT source attribute",
            )
        })
        .transpose()?
        .flatten()
        .map(String::as_str);
    let active_index = ir
        .source
        .as_ref()
        .map(|source| {
            ctx.get_btree_map(
                &source.attributes,
                "active_parasolid_block",
                "look up SLDPRT source attribute",
            )
        })
        .transpose()?
        .flatten()
        .map(|section| crate::container::configuration_index(ctx, section))
        .transpose()?
        .flatten();
    let by_name = match active_name {
        Some(name) => unique_configuration(
            ctx,
            &ir.model.configurations,
            |configuration| {
                Ok(match configuration.name.as_deref() {
                    Some(candidate) => ctx.equal(candidate, name, ACTIVE)?,
                    None => false,
                })
            },
            ACTIVE,
        )?,
        None => None,
    };
    let by_index = match active_index.and_then(|index| u32::try_from(index).ok()) {
        Some(index) => unique_configuration(
            ctx,
            &ir.model.configurations,
            |configuration| Ok(configuration.source_index == Some(index)),
            ACTIVE,
        )?,
        None => None,
    };
    let selected = if active_name.is_some() {
        by_name
    } else if active_index.is_some() {
        by_index
    } else if ir.model.configurations.len() == 1 {
        Some(0)
    } else {
        None
    };
    for (position, configuration) in ctx
        .admit_iter(&mut ir.model.configurations, ACTIVE)?
        .enumerate()
    {
        configuration.active = selected == Some(position);
    }
    Ok(())
}

/// The position of the one configuration `matches` selects, visiting
/// configurations until a second match proves the selection ambiguous.
fn unique_configuration(
    ctx: &DecodeContext<'_>,
    configurations: &[cadmpeg_ir::features::DesignConfiguration],
    mut matches: impl FnMut(&cadmpeg_ir::features::DesignConfiguration) -> Result<bool, CodecError>,
    operation: &'static str,
) -> Result<Option<usize>, CodecError> {
    let mut remaining = configurations.iter().enumerate();
    let Some((position, _)) = ctx.find_by(
        &mut remaining,
        |(_, configuration)| matches(configuration),
        operation,
    )?
    else {
        return Ok(None);
    };
    Ok((!ctx.any_by(
        &mut remaining,
        |(_, configuration)| matches(configuration),
        operation,
    )?)
    .then_some(position))
}

fn snapshot_active_configuration(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<(), CodecError> {
    const FEATURE_SNAPSHOT: &str = "retain SLDPRT configuration feature snapshot";

    let Some(configuration_index) = unique_configuration(
        ctx,
        &ir.model.configurations,
        |configuration| Ok(configuration.active),
        "find SLDPRT active configuration",
    )?
    else {
        return Ok(());
    };
    if !ir.model.configurations[configuration_index]
        .parameter_values
        .is_empty()
        || !ir.model.configurations[configuration_index]
            .feature_states
            .is_empty()
    {
        return Ok(());
    }

    let mut parameter_values = BTreeMap::new();
    for parameter in ctx.admit_iter(
        &ir.model.parameters,
        "snapshot SLDPRT configuration parameters",
    )? {
        let Some(value) = &parameter.value else {
            continue;
        };
        let value =
            value.try_clone_for_decode(ctx, "retain SLDPRT configuration parameter value")?;
        let id = parameter
            .id
            .try_clone_for_decode(ctx, "retain SLDPRT configuration parameter ID")?;
        ctx.insert_btree_map(
            &mut parameter_values,
            id,
            value,
            "snapshot SLDPRT configuration parameter",
        )?;
    }
    let mut feature_states = BTreeMap::new();
    for feature in ctx.admit_iter(
        &ir.model.features,
        "scan SLDPRT snapshot_active_configuration values",
    )? {
        let id = feature.id.try_clone_for_decode(ctx, FEATURE_SNAPSHOT)?;
        let state = feature.configuration_state(ctx, FEATURE_SNAPSHOT)?;
        ctx.insert_btree_map(&mut feature_states, id, state, FEATURE_SNAPSHOT)?;
    }
    let configuration = &mut ir.model.configurations[configuration_index];
    configuration.parameter_values = parameter_values;
    configuration.feature_states = feature_states;
    // Read-side fabricated snapshot of model-level state; tag the configuration
    // so the write path can distinguish it from feature-input lane state.
    let id = ctx.copy_retained_text(
        configuration.id.as_str(),
        "retain SLDPRT snapshot configuration ID",
    )?;
    if let Some(source) = &mut ir.source {
        ctx.insert_btree_map(
            &mut source.attributes,
            cadmpeg_core::nonblank_literal!("sldprt_configuration_snapshot_synthesized"),
            id,
            "mark SLDPRT configuration snapshot",
        )?;
    }
    Ok(())
}

fn sync_active_configuration_resolutions(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<(), cadmpeg_core::CodecError> {
    let Some(configuration_index) = unique_configuration(
        ctx,
        &ir.model.configurations,
        |configuration| Ok(configuration.active),
        "find SLDPRT active configuration",
    )?
    else {
        return Ok(());
    };
    let (features, configurations) = (&ir.model.features, &mut ir.model.configurations);
    let configuration = &mut configurations[configuration_index];
    for feature in ctx
        .admit_iter(features, "scan SLDPRT active configuration holes")?
        .filter(|feature| feature.suppressed != Some(true))
    {
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Hole {
                placements: resolved_placements,
                shape: resolved_shape,
                extent: resolved_extent,
                bottom: resolved_bottom,
                taper_angle: resolved_taper_angle,
                ..
            },
        ) = feature.evaluation.definition()
        else {
            continue;
        };
        let Some(state) = ctx.get_mut_btree_map(
            &mut (configuration.feature_states),
            &feature.id,
            "look up mutable SLDPRT ordered key",
        )?
        else {
            continue;
        };
        if state.evaluation.is_suppressed() {
            continue;
        }
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Hole {
                placements,
                shape,
                extent,
                bottom,
                taper_angle,
                ..
            },
        ) = &mut state.definition
        else {
            continue;
        };
        let copied_placements = if placements.is_none() {
            resolved_placements
                .as_deref()
                .map(|source| {
                    ctx.try_collect_retained_with(
                        source,
                        "copy SLDPRT active configuration hole",
                        |placement| {
                            placement
                                .try_clone_for_decode(ctx, "copy SLDPRT active configuration hole")
                        },
                    )
                })
                .transpose()?
        } else {
            None
        };
        let incomplete = shape.diameter().is_none()
            || extent.as_ref().is_none_or(|extent| {
                matches!(
                    extent,
                    cadmpeg_ir::features::LinearTermination::Unresolved {}
                )
            })
            || matches!(
                shape.construction(),
                cadmpeg_ir::features::holes::HoleConstruction::Form { kind, .. }
                    if kind.is_unresolved()
            );
        let resolved_construction = resolved_shape.construction();
        let resolved_diameter = resolved_shape.diameter();
        let resolved_complete = resolved_diameter.is_some()
            && resolved_extent.as_ref().is_some_and(|extent| {
                !matches!(
                    extent,
                    cadmpeg_ir::features::LinearTermination::Unresolved {}
                )
            })
            && !matches!(
                resolved_construction,
                cadmpeg_ir::features::holes::HoleConstruction::Form {
                    kind: cadmpeg_ir::features::holes::HoleKind::Unresolved(_)
                        | cadmpeg_ir::features::holes::HoleKind::PartialCounterbore(..)
                        | cadmpeg_ir::features::holes::HoleKind::PartialCountersink(..),
                    ..
                }
            );
        let apply_resolution = incomplete && resolved_complete;
        let mut replacement = if apply_resolution
            && !matches!(
                (shape.construction(), resolved_construction),
                (
                    cadmpeg_ir::features::holes::HoleConstruction::Form { .. },
                    cadmpeg_ir::features::holes::HoleConstruction::Form { .. }
                )
            ) {
            Some(
                resolved_construction
                    .try_clone_for_decode(ctx, "copy SLDPRT active configuration hole")?,
            )
        } else {
            None
        };
        let copied_extent = if apply_resolution {
            resolved_extent
                .as_ref()
                .map(|source| {
                    source.try_clone_for_decode(ctx, "copy SLDPRT active configuration hole")
                })
                .transpose()?
        } else {
            None
        };
        if let Some(copied_placements) = copied_placements {
            *placements = Some(copied_placements);
        }
        if apply_resolution {
            shape
                .try_edit(|construction, _, diameter| {
                    if let Some(replacement) = replacement.take() {
                        *construction = replacement;
                    } else if let (
                        cadmpeg_ir::features::holes::HoleConstruction::Form { kind, .. },
                        cadmpeg_ir::features::holes::HoleConstruction::Form {
                            kind: resolved_kind,
                            ..
                        },
                    ) = (construction, resolved_construction)
                    {
                        *kind = *resolved_kind;
                    }
                    *diameter = resolved_diameter;
                })
                .map_err(cadmpeg_core::CodecError::malformed)?;
            *extent = copied_extent;
            *bottom = *resolved_bottom;
            *taper_angle = *resolved_taper_angle;
        }
    }
    let (features, configurations) = (&ir.model.features, &mut ir.model.configurations);
    let configuration = &mut configurations[configuration_index];
    for feature in ctx.admit_iter(features, "scan SLDPRT features values")? {
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::CosmeticThread {
                face: resolved_face,
                diameter: resolved_diameter,
                extent: resolved_extent,
            },
        ) = feature.evaluation.definition()
        else {
            continue;
        };
        let complete = match resolved_face {
            cadmpeg_ir::features::FaceSelection::Faces(selected)
            | cadmpeg_ir::features::FaceSelection::Resolved {
                faces: selected, ..
            } => !selected.is_empty(),
            _ => false,
        };
        if !complete {
            continue;
        }
        let Some(state) = ctx.get_mut_btree_map(
            &mut (configuration.feature_states),
            &feature.id,
            "look up mutable SLDPRT ordered key",
        )?
        else {
            continue;
        };
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::CosmeticThread {
                face,
                diameter,
                extent,
            },
        ) = &mut state.definition
        else {
            continue;
        };
        if ctx.equal(
            &*diameter,
            resolved_diameter,
            "compare SLDPRT cosmetic threads",
        )? && ctx.equal(&*extent, resolved_extent, "compare SLDPRT cosmetic threads")?
            && matches!(
                face,
                cadmpeg_ir::features::FaceSelection::Unresolved
                    | cadmpeg_ir::features::FaceSelection::Native(_)
            )
        {
            *face = resolved_face
                .try_clone_for_decode(ctx, "copy SLDPRT resolved cosmetic thread face")?;
        }
    }
    for feature in ctx.admit_iter(features, "scan SLDPRT features values")? {
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::DatumOffsetPlane {
                reference:
                    Some(cadmpeg_ir::features::DatumPlaneReference::Face {
                        face: resolved_face @ cadmpeg_ir::features::FaceSelection::Faces(selected),
                    }),
                distance: resolved_distance,
            },
        ) = feature.evaluation.definition()
        else {
            continue;
        };
        if selected.is_empty() {
            continue;
        }
        let Some(state) = ctx.get_mut_btree_map(
            &mut (configuration.feature_states),
            &feature.id,
            "look up mutable SLDPRT ordered key",
        )?
        else {
            continue;
        };
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::DatumOffsetPlane {
                reference:
                    reference @ Some(cadmpeg_ir::features::DatumPlaneReference::ResolvedPlane { .. }),
                distance,
            },
        ) = &mut state.definition
        else {
            continue;
        };
        if distance == resolved_distance {
            *reference = Some(cadmpeg_ir::features::DatumPlaneReference::Face {
                face: resolved_face.try_clone_for_decode(ctx, "copy SLDPRT resolved datum face")?,
            });
        }
    }
    for feature in ctx.admit_iter(features, "scan SLDPRT active configuration patterns")? {
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Pattern {
                seeds: resolved_seeds,
                pattern: resolved_pattern,
            },
        ) = feature.evaluation.definition()
        else {
            continue;
        };
        if !matches!(
            resolved_pattern.definition(),
            cadmpeg_ir::features::patterns::PatternTransform::Mirror { .. }
        ) {
            continue;
        }
        let Some(state) = ctx.get_mut_btree_map(
            &mut (configuration.feature_states),
            &feature.id,
            "look up mutable SLDPRT ordered key",
        )?
        else {
            continue;
        };
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Pattern { seeds, pattern },
        ) = &mut state.definition
        else {
            continue;
        };
        if pattern.is_unresolved()
            && ctx.equal(
                seeds.as_slice(),
                resolved_seeds.as_slice(),
                "compare SLDPRT configuration patterns",
            )?
        {
            *pattern = resolved_pattern
                .try_clone_for_decode(ctx, "copy SLDPRT resolved configuration pattern")?;
        }
    }

    Ok(())
}

fn stamp_feature_baseline(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
    let hash = crate::history::hash::feature_hash(ctx, &ir.model)?;
    if let Some(source) = &mut ir.source {
        ctx.insert_btree_map(
            &mut (source.attributes),
            cadmpeg_core::nonblank_literal!("sldprt_neutral_feature_local_sha256"),
            hash,
            "insert SLDPRT ordered entry",
        )?;
    }
    Ok(())
}

fn assign_configuration_bodies(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    configuration_bodies: Vec<(usize, Vec<ConfigurationBodyIdentity<'_>>)>,
) -> Result<(), CodecError> {
    const MERGE: &str = "merge SLDPRT configuration bodies";
    let mut workspace = ctx.reserve_scoped(0, "SLDPRT configuration partition lookup workspace")?;
    let mut partition_map = BTreeMap::<u32, Vec<cadmpeg_ir::ids::BodyId>>::new();
    // Each partition keeps the first copy of every body, in site order.
    let mut seen_storage = ctx.reserve_scoped(0, "SLDPRT configuration body uniqueness workspace")?;
    let mut seen = BTreeMap::<u32, BTreeSet<cadmpeg_ir::ids::BodyId>>::new();
    for (index, bodies) in ctx.admit_iter(configuration_bodies, MERGE)? {
        let Ok(index) = u32::try_from(index) else {
            continue;
        };
        for ConfigurationBodyIdentity { id: body, storage } in ctx.admit_iter(bodies, MERGE)? {
            if let Some(bodies) = ctx.get_btree_map(&seen, &index, MERGE)? {
                if ctx.contains_btree_set(bodies, &body, MERGE)? {
                    continue;
                }
            }
            seen_storage.with_storage(|| {
                ctx.insert_btree_group_set(
                    &mut seen,
                    index,
                    body.try_clone_for_decode(ctx, MERGE)?,
                    MERGE,
                    MERGE,
                )
            })?;
            storage.commit()?;
            if let Some(bodies) = ctx.get_mut_btree_map(
                &mut partition_map,
                &index,
                "index SLDPRT configuration partitions",
            )? {
                ctx.push_vec(bodies, body, MERGE)?;
            } else {
                let mut bodies = Vec::new();
                ctx.push_vec(&mut bodies, body, MERGE)?;
                workspace.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut partition_map,
                        index,
                        bodies,
                        "index SLDPRT configuration partitions",
                    )
                })?;
            }
        }
    }
    drop((seen, seen_storage));

    let mut source_counts = BTreeMap::<u32, usize>::new();
    for source_index in ctx
        .admit_iter(
            &ir.model.configurations,
            "count SLDPRT configuration sources",
        )?
        .filter_map(|configuration| configuration.source_index)
    {
        if let Some(count) = ctx.get_mut_btree_map(
            &mut source_counts,
            &source_index,
            "count SLDPRT configuration sources",
        )? {
            *count += 1;
        } else {
            workspace.with_storage(|| {
                ctx.insert_btree_map(
                    &mut source_counts,
                    source_index,
                    1,
                    "count SLDPRT configuration sources",
                )
            })?;
        }
    }
    for configuration in ctx.admit_iter(&mut ir.model.configurations, MERGE)? {
        let Some(source_index) = configuration.source_index else {
            continue;
        };
        if ctx.get_btree_map(&source_counts, &source_index, MERGE)? == Some(&1) {
            configuration.bodies = Some(cadmpeg_ir::features::DistinctMembers::try_from(
                ctx.remove_btree_map(&mut partition_map, &source_index, MERGE)?
                    .unwrap_or_default(),
                ctx,
            )?);
        }
    }
    if let Some((active_index, position)) = bind_active_configuration_partition(ctx, ir)? {
        if let Some(bodies) = ctx.remove_btree_map(&mut partition_map, &active_index, MERGE)? {
            ir.model.configurations[position].bodies = Some(
                cadmpeg_ir::features::DistinctMembers::try_from(bodies, ctx)?,
            );
        }
    }
    // Partitions no configuration claims become configurations ordered after
    // every existing one.
    let mut next_ordinal = ctx
        .max_by_key(
            &ir.model.configurations,
            |configuration| Ok(configuration.ordinal),
            |left, right| Ok(left.cmp(right)),
            "order SLDPRT partition configurations",
        )?
        .map(|configuration| configuration.ordinal);
    for (source_index, bodies) in ctx.admit_iter(partition_map, MERGE)? {
        let ordinal = match next_ordinal {
            Some(highest) => highest.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "append SLDPRT partition configuration",
                    u64::MAX - 1,
                    u64::MAX,
                )
            })?,
            None => 0,
        };
        next_ordinal = Some(ordinal);
        ctx.reserve_vec(
            &mut ir.model.configurations,
            1,
            "append SLDPRT partition configuration",
        )?;
        ir.model
            .configurations
            .push(cadmpeg_ir::features::DesignConfiguration {
                id: cadmpeg_ir::features::ConfigurationId::compose(
                    &cadmpeg_ir::identity_namespace!("sldprt", "model", "configuration"),
                    cadmpeg_ir::identity_key!("partition:").then(source_index),
                ),
                ordinal,
                active: false,
                source_index: Some(source_index),
                name: format!("Config-{source_index}").into(),
                material: None,
                properties: std::collections::BTreeMap::new(),
                bodies: Some(cadmpeg_ir::features::DistinctMembers::try_from(
                    bodies, ctx,
                )?),
                parameter_values: std::collections::BTreeMap::new(),
                parameter_overrides: BTreeMap::new(),
                feature_states: std::collections::BTreeMap::new(),
                native_ref: None,
            });
    }
    Ok(())
}

/// Bind the active configuration's partition identity from the two native
/// selectors, without inferring body membership.
fn bind_active_configuration_partition(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<Option<(u32, usize)>, cadmpeg_core::CodecError> {
    const PARTITION: &str = "scan SLDPRT partition configuration identities";
    let active_name = ir
        .source
        .as_ref()
        .map(|source| {
            ctx.get_btree_map(
                &source.attributes,
                "sw_configuration_name",
                "look up SLDPRT source attribute",
            )
        })
        .transpose()?
        .flatten();
    let active_index = ir
        .source
        .as_ref()
        .map(|source| {
            ctx.get_btree_map(
                &source.attributes,
                "active_parasolid_block",
                "look up SLDPRT source attribute",
            )
        })
        .transpose()?
        .flatten()
        .map(|section| crate::container::configuration_index(ctx, section))
        .transpose()?
        .flatten()
        .and_then(|index| u32::try_from(index).ok());
    let (Some(active_name), Some(active_index)) = (active_name, active_index) else {
        return Ok(None);
    };
    let Some(position) = unique_configuration(
        ctx,
        &ir.model.configurations,
        |configuration| {
            Ok(configuration.source_index.is_none()
                && match configuration.name.as_deref() {
                    Some(name) => ctx.equal(name, active_name.as_str(), PARTITION)?,
                    None => false,
                })
        },
        PARTITION,
    )?
    else {
        return Ok(None);
    };
    if ctx.any_by(
        &ir.model.configurations,
        |configuration| Ok(configuration.source_index == Some(active_index)),
        PARTITION,
    )? {
        return Ok(None);
    }

    // The container's active block and the native configuration name
    // establish the partition identity even when that block yielded no
    // decoded body list. Body membership remains unresolved until a decoded
    // partition supplies its body identities.
    ir.model.configurations[position].source_index = Some(active_index);
    Ok(Some((active_index, position)))
}

fn stamp_configuration_baseline(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
    let hash = crate::history::hash::configuration_hash(ctx, &ir.model.configurations)?;
    let parameter_value_hash =
        crate::history::hash::configuration_parameter_value_hash(ctx, &ir.model.configurations)?;
    let feature_state_hash =
        crate::history::hash::configuration_feature_state_hash(ctx, &ir.model.configurations)?;
    if let Some(source) = &mut ir.source {
        ctx.insert_btree_map(
            &mut (source.attributes),
            cadmpeg_core::nonblank_literal!("sldprt_neutral_configuration_local_sha256"),
            hash,
            "insert SLDPRT ordered entry",
        )?;
        ctx.insert_btree_map(
            &mut (source.attributes),
            cadmpeg_core::nonblank_literal!("sldprt_configuration_parameter_values_local_sha256"),
            parameter_value_hash,
            "insert SLDPRT ordered entry",
        )?;
        ctx.insert_btree_map(
            &mut (source.attributes),
            cadmpeg_core::nonblank_literal!("sldprt_configuration_feature_states_local_sha256"),
            feature_state_hash,
            "insert SLDPRT ordered entry",
        )?;
    }
    Ok(())
}

/// Record the sketch baselines the write path compares against.
///
/// Neutral sketch and constraint digests use `_local_sha256` (projected
/// geometry through libm). `sldprt_native_sketch_sha256` has no suffix: it
/// digests lane fields that are strings, integers, or verbatim `f64` bit
/// patterns from the payload.
fn stamp_sketch_baseline(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    lanes: &[crate::records::FeatureInputLane],
) -> Result<(), CodecError> {
    let neutral_hash = crate::resolved_features::hashes::sketch_hash(ctx, ir)?;
    let constraint_hash = crate::resolved_features::hashes::constraint_hash(ctx, ir)?;
    let native_hash = crate::resolved_features::hashes::lane_hash(ctx, lanes)?;
    if let Some(source) = &mut ir.source {
        ctx.insert_btree_map(
            &mut (source.attributes),
            cadmpeg_core::nonblank_literal!("sldprt_neutral_sketch_local_sha256"),
            neutral_hash,
            "insert SLDPRT ordered entry",
        )?;
        ctx.insert_btree_map(
            &mut (source.attributes),
            cadmpeg_core::nonblank_literal!("sldprt_native_sketch_sha256"),
            native_hash,
            "insert SLDPRT ordered entry",
        )?;
        ctx.insert_btree_map(
            &mut (source.attributes),
            cadmpeg_core::nonblank_literal!("sldprt_neutral_sketch_constraint_local_sha256"),
            constraint_hash,
            "insert SLDPRT ordered entry",
        )?;
    }
    Ok(())
}

/// Record the document and B-rep partition baselines the write path compares
/// against.
///
/// Both are machine-local content digests and carry the `_local_sha256` suffix
/// that says so; see [`document_local_sha256`] and [`brep_local_sha256`].
fn stamp_local_digests(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
    ir.finalize(ctx)?;
    let brep_hash = brep_local_sha256_in_place(ctx, ir)?;
    if let Some(source) = &mut ir.source {
        ctx.insert_btree_map(
            &mut (source.attributes),
            cadmpeg_core::nonblank_literal!("brep_local_sha256"),
            brep_hash,
            "insert SLDPRT ordered entry",
        )?;
    }
    let has_swobjects_semantics = ctx.any_by(
        &ir.model.attributes[..],
        |attribute| Ok(attribute.id.as_str().starts_with("sldprt:metadata:")),
        "scan SLDPRT stamp_local_digests values",
    )? || ctx.any_by(
        &ir.model.appearances[..],
        |appearance| Ok(appearance.schema.as_deref() == Some("moVisualProperties_c")),
        "scan SLDPRT stamp_local_digests values",
    )?;
    if has_swobjects_semantics {
        match (
            crate::writer::swobjects_local_sha256(ctx, ir),
            crate::writer::swobjects_material_local_sha256(ctx, ir),
        ) {
            (Ok(swobjects_hash), Ok(material_hash)) => {
                let identity_hash =
                    crate::writer::swobjects_metadata_identity_local_sha256(ctx, ir)?;
                if let Some(source) = &mut ir.source {
                    ctx.insert_btree_map(
                        &mut (source.attributes),
                        cadmpeg_core::nonblank_const!(
                            crate::writer::SWOBJECTS_LOCAL_DIGEST_ATTRIBUTE
                        ),
                        swobjects_hash,
                        "insert SLDPRT ordered entry",
                    )?;
                    ctx.insert_btree_map(
                        &mut (source.attributes),
                        cadmpeg_core::nonblank_const!(
                            crate::writer::SWOBJECTS_MATERIAL_LOCAL_DIGEST_ATTRIBUTE
                        ),
                        material_hash,
                        "insert SLDPRT ordered entry",
                    )?;
                    ctx.insert_btree_map(
                        &mut (source.attributes),
                        cadmpeg_core::nonblank_const!(
                            crate::writer::SWOBJECTS_METADATA_IDENTITY_LOCAL_DIGEST_ATTRIBUTE
                        ),
                        identity_hash,
                        "insert SLDPRT ordered entry",
                    )?;
                }
            }
            (Err(error @ CodecError::ResourceLimit(_)), _)
            | (_, Err(error @ CodecError::ResourceLimit(_))) => return Err(error),
            _ => {}
        }
    }
    if !ir.model.pmi.is_empty() {
        if let Ok(hash) = crate::writer::pmi_local_sha256(ir) {
            if let Some(source) = &mut ir.source {
                ctx.insert_btree_map(
                    &mut (source.attributes),
                    cadmpeg_core::nonblank_const!(crate::writer::PMI_LOCAL_DIGEST_ATTRIBUTE),
                    hash,
                    "insert SLDPRT ordered entry",
                )?;
            }
        }
    }
    let hash = cadmpeg_ir::hash::document_local_sha256(
        ctx,
        ir,
        ir.source.as_ref(),
        "sldprt",
        "sldprt:file:source-image#0",
        "record SLDPRT document digest",
    )?;
    if let Some(source) = &mut ir.source {
        ctx.insert_btree_map(
            &mut (source.attributes),
            cadmpeg_core::nonblank_const!(cadmpeg_ir::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE),
            hash,
            "insert SLDPRT ordered entry",
        )?;
    }
    Ok(())
}

/// The machine-local content digest recorded as the SLDPRT `brep_local_sha256`
/// attribute.
///
/// A bitwise digest over the decoded B-rep alone: geometry, topology, and face
/// appearances, with names, colors, tessellations, history, and every native
/// record excluded. [`crate::writer::retained_partition`] compares it to decide
/// whether the retained Parasolid partition may be replayed verbatim while the
/// rest of the document is written. It carries every limitation
/// [`document_local_sha256`] states, and the `_local_sha256` suffix says so.
pub(crate) fn brep_local_sha256(ir: &CadIr) -> Result<String, CodecError> {
    // Admit only B-rep arenas so a new design, presentation, or product arena
    // cannot silently change retained-partition eligibility.
    let mut partition = cadmpeg_ir::document::Model::default();
    partition.bodies.clone_from(&ir.model.bodies);
    partition.regions.clone_from(&ir.model.regions);
    partition.shells.clone_from(&ir.model.shells);
    partition.faces.clone_from(&ir.model.faces);
    partition.loops.clone_from(&ir.model.loops);
    partition.coedges.clone_from(&ir.model.coedges);
    partition.edges.clone_from(&ir.model.edges);
    partition.vertices.clone_from(&ir.model.vertices);
    partition.points.clone_from(&ir.model.points);
    partition.surfaces.clone_from(&ir.model.surfaces);
    partition.curves.clone_from(&ir.model.curves);
    partition.pcurves.clone_from(&ir.model.pcurves);
    partition
        .procedural_surfaces
        .clone_from(&ir.model.procedural_surfaces);
    partition
        .procedural_curves
        .clone_from(&ir.model.procedural_curves);
    partition.appearances.clone_from(&ir.model.appearances);
    partition
        .appearance_bindings
        .clone_from(&ir.model.appearance_bindings);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::desktop(),
    )?;
    let ctx = &ctx;
    let normalize = (|| {
        for body in ctx.admit_iter(&mut partition.bodies, "normalize SLDPRT digest body fields")? {
            body.name = None;
            body.color = None;
        }
        ctx.retain_vec(
            &mut partition.appearance_bindings,
            |binding| Ok(matches!(binding.target, AppearanceTarget::Face(_))),
            "normalize SLDPRT digest face bindings",
        )?;
        let (used, _used_storage) = ctx.collect_scoped_string_set(
            partition.appearance_bindings.len(),
            partition
                .appearance_bindings
                .iter()
                .map(|binding| binding.appearance.as_str()),
            "index SLDPRT digest face appearances",
        )?;
        ctx.retain_vec(
            &mut partition.appearances,
            |appearance| {
                ctx.contains_hash_set(
                    &used,
                    appearance.id.as_str(),
                    "find SLDPRT digest face appearance",
                )
            },
            "normalize SLDPRT digest appearances",
        )
    })();
    normalize?;
    brep_partition_sha256(ctx, ir.tolerances, partition).0
}

/// [`brep_local_sha256`] without the deep clone, for the decode stamp path.
///
/// Moves the structurally untouched B-rep arenas out of `ir`, hashes the same
/// normalized partition [`brep_local_sha256`] builds, and moves them back in
/// their original order. The two arenas the normalization filters —
/// `appearances` and `appearance_bindings` — are moved into admitted partitions. Body display fields
/// move into a charged vector and back, so `ir` is bit-identical afterwards and both entry
/// points produce the same digest for the same document.
fn brep_local_sha256_in_place(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<String, CodecError> {
    use std::mem::take;

    const FACE_APPEARANCES: &str = "scan SLDPRT digest appearance bindings";
    let mut face_appearances = BTreeSet::new();
    let mut face_appearance_storage = ctx.reserve_scoped(0, FACE_APPEARANCES)?;
    for binding in ctx.admit_iter(&ir.model.appearance_bindings, FACE_APPEARANCES)? {
        if matches!(binding.target, AppearanceTarget::Face(_)) {
            face_appearance_storage.with_storage(|| {
                ctx.insert_btree_set(&mut face_appearances, &binding.appearance, FACE_APPEARANCES)
            })?;
        }
    }
    let appearance_partition = digest_partition::DigestPartition::prepare(
        ctx,
        &mut ir.model.appearances,
        |appearance| ctx.contains_btree_set(&face_appearances, &appearance.id, FACE_APPEARANCES),
        "partition SLDPRT digest appearances",
    )?;
    drop((face_appearances, face_appearance_storage));
    let binding_partition = digest_partition::DigestPartition::prepare(
        ctx,
        &mut ir.model.appearance_bindings,
        |binding| Ok(matches!(binding.target, AppearanceTarget::Face(_))),
        "partition SLDPRT digest bindings",
    )?;
    let (mut saved_body_display, _display_storage) =
        ctx.with_scoped_storage("SLDPRT digest display workspace", || {
            ctx.collection_vec(
                ir.model.bodies.len(),
                "save SLDPRT body display fields for digest",
            )
        })?;
    const DISPLAY_WORK: &str = "save and restore SLDPRT body display fields for digest";
    let display_work = ir.model.bodies.len().checked_mul(2)
        .ok_or_else(|| ctx.refuse_codec_limit(DISPLAY_WORK, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(u64_from_index(display_work), DISPLAY_WORK)?;
    let mut binding_partition = binding_partition.move_from();
    let mut appearance_partition = appearance_partition.move_from();
    // The slots were reserved above, so saving the display fields cannot fail
    // once the partitions have moved.
    for body in &mut ir.model.bodies {
        saved_body_display.push((take(&mut body.name), take(&mut body.color)));
    }
    let mut partition = cadmpeg_ir::document::Model::default();
    partition.bodies = take(&mut ir.model.bodies);
    partition.regions = take(&mut ir.model.regions);
    partition.shells = take(&mut ir.model.shells);
    partition.faces = take(&mut ir.model.faces);
    partition.loops = take(&mut ir.model.loops);
    partition.coedges = take(&mut ir.model.coedges);
    partition.edges = take(&mut ir.model.edges);
    partition.vertices = take(&mut ir.model.vertices);
    partition.points = take(&mut ir.model.points);
    partition.surfaces = take(&mut ir.model.surfaces);
    partition.curves = take(&mut ir.model.curves);
    partition.pcurves = take(&mut ir.model.pcurves);
    partition.procedural_surfaces = take(&mut ir.model.procedural_surfaces);
    partition.procedural_curves = take(&mut ir.model.procedural_curves);
    partition.appearances = appearance_partition.take_kept();
    partition.appearance_bindings = binding_partition.take_kept();
    let (hash, mut partition) = brep_partition_sha256(ctx, ir.tolerances, partition);
    ir.model.bodies = take(&mut partition.bodies);
    for (body, (name, color)) in ir.model.bodies.iter_mut().zip(saved_body_display) {
        body.name = name;
        body.color = color;
    }
    ir.model.regions = take(&mut partition.regions);
    ir.model.shells = take(&mut partition.shells);
    ir.model.faces = take(&mut partition.faces);
    ir.model.loops = take(&mut partition.loops);
    ir.model.coedges = take(&mut partition.coedges);
    ir.model.edges = take(&mut partition.edges);
    ir.model.vertices = take(&mut partition.vertices);
    ir.model.points = take(&mut partition.points);
    ir.model.surfaces = take(&mut partition.surfaces);
    ir.model.curves = take(&mut partition.curves);
    ir.model.pcurves = take(&mut partition.pcurves);
    ir.model.procedural_surfaces = take(&mut partition.procedural_surfaces);
    ir.model.procedural_curves = take(&mut partition.procedural_curves);
    ir.model.appearances = appearance_partition.restore(take(&mut partition.appearances))?;
    ir.model.appearance_bindings =
        binding_partition.restore(take(&mut partition.appearance_bindings))?;
    hash
}

#[cfg(test)]
mod digest_tests {
    use super::brep_local_sha256_in_place;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::ids::{AppearanceBindingId, AppearanceId, BodyId, FaceId};
    use cadmpeg_ir::topology::{Body, BodyKind};
    use std::collections::BTreeMap;

    fn named_body_document() -> CadIr {
        let mut ir = CadIr::empty();
        ir.model.bodies.push(Body {
            id: BodyId::mint("synthetic:test:id#digest-body").unwrap(),
            kind: BodyKind::default(),
            regions: Vec::new(),
            transform: None,
            name: Some("digest body".to_owned()),
            color: None,
            visible: None,
        });
        ir
    }

    fn appearance(id: &str) -> Appearance {
        Appearance {
            id: AppearanceId::mint(id).unwrap(),
            name: None,
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: None,
            category: None,
            base_color: None,
            properties: BTreeMap::new(),
            textures: Vec::new(),
        }
    }

    fn binding(id: &str, target: AppearanceTarget, appearance: &str) -> AppearanceBinding {
        AppearanceBinding {
            id: AppearanceBindingId::mint(id).unwrap(),
            target,
            appearance: AppearanceId::mint(appearance).unwrap(),
            source_entity_id: None,
            object_type: None,
            visible: None,
            channels: BTreeMap::new(),
        }
    }

    #[test]
    fn digest_body_display_reserve_refuses_collection_limit() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"digest", &arena, &policy).unwrap();
        let error = brep_local_sha256_in_place(&ctx, &mut named_body_document()).unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(_)));
    }

    #[test]
    fn digest_body_display_work_refuses_before_mutation() {
        use cadmpeg_core::decode::ResourceDimension;
        let arena = DecodeArena::new();
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            "save and restore SLDPRT body display fields for digest",
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                let mut ir = named_body_document();
                let before = ir.clone();
                let result = brep_local_sha256_in_place(&ctx, &mut ir);
                assert_eq!(ir, before);
                result
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.additional == 2));
    }

    #[test]
    fn digest_restores_body_display_fields() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(b"digest", &arena, &DecodePolicy::service()).unwrap();
        let mut ir = named_body_document();
        let original = ir.model.bodies[0].clone();
        brep_local_sha256_in_place(&ctx, &mut ir).unwrap();
        assert_eq!(ir.model.bodies[0], original);
    }

    #[test]
    fn digest_normalizes_and_restores_colored_body_fields() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut ir = named_body_document();
        ir.model.bodies[0].color = Some(cadmpeg_ir::topology::Color::from_rgba8(17, 29, 43, 255));
        let before = ir.clone();
        let expected = super::brep_local_sha256(&ir).unwrap();
        assert_eq!(brep_local_sha256_in_place(&ctx, &mut ir).unwrap(), expected);
        assert_eq!(ir, before);
        ctx.finish_session().unwrap();
    }

    #[test]
    fn digest_hash_refusal_restores_the_moved_model() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(b"digest", &arena, &policy).unwrap();
        let mut ir = named_body_document();
        ir.model
            .appearances
            .push(appearance("synthetic:test:id#face-appearance"));
        ir.model.appearance_bindings.push(binding(
            "synthetic:test:id#face-binding",
            AppearanceTarget::Face(FaceId::mint("synthetic:test:id#digest-face").unwrap()),
            "synthetic:test:id#face-appearance",
        ));
        let original = ir.clone();
        let CodecError::ResourceLimit(limit) =
            brep_local_sha256_in_place(&ctx, &mut ir).unwrap_err()
        else {
            panic!("digest must refuse recursion");
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::RecursionDepth
        );
        assert_eq!(ir, original);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(first)) if first == limit)
        );
    }

    #[test]
    fn digest_appearance_filter_refuses_work_limit() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"digest", &arena, &policy).unwrap();
        let mut ir = CadIr::empty();
        ir.model.appearance_bindings.push(AppearanceBinding {
            id: AppearanceBindingId::mint("synthetic:test:id#digest-binding").unwrap(),
            target: AppearanceTarget::Face(FaceId::mint("synthetic:test:id#digest-face").unwrap()),
            appearance: AppearanceId::mint("synthetic:test:id#digest-appearance").unwrap(),
            source_entity_id: None,
            object_type: None,
            visible: None,
            channels: BTreeMap::new(),
        });
        let error = brep_local_sha256_in_place(&ctx, &mut ir).unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(_)));
        assert_eq!(ir.model.appearance_bindings.len(), 1);
    }

    #[test]
    fn digest_appearance_partition_refuses_collection_limit() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"digest", &arena, &policy).unwrap();
        let mut ir = CadIr::empty();
        ir.model.appearance_bindings.push(binding(
            "synthetic:test:id#digest-binding",
            AppearanceTarget::Face(FaceId::mint("synthetic:test:id#digest-face").unwrap()),
            "synthetic:test:id#digest-appearance",
        ));
        let error = brep_local_sha256_in_place(&ctx, &mut ir).unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(_)));
        assert_eq!(ir.model.appearance_bindings.len(), 1);
    }

    #[test]
    fn digest_restores_appearance_order() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(b"digest", &arena, &DecodePolicy::service()).unwrap();
        let mut ir = CadIr::empty();
        ir.model
            .appearances
            .push(appearance("synthetic:test:id#body-appearance"));
        ir.model
            .appearances
            .push(appearance("synthetic:test:id#face-appearance"));
        ir.model.appearance_bindings.push(binding(
            "synthetic:test:id#body-binding",
            AppearanceTarget::Body(BodyId::mint("synthetic:test:id#digest-body").unwrap()),
            "synthetic:test:id#body-appearance",
        ));
        ir.model.appearance_bindings.push(binding(
            "synthetic:test:id#face-binding",
            AppearanceTarget::Face(FaceId::mint("synthetic:test:id#digest-face").unwrap()),
            "synthetic:test:id#face-appearance",
        ));
        let appearances = ir.model.appearances.clone();
        let bindings = ir.model.appearance_bindings.clone();
        let expected_hash = super::brep_local_sha256(&ir).unwrap();
        let actual_hash = brep_local_sha256_in_place(&ctx, &mut ir).unwrap();
        assert_eq!(actual_hash, expected_hash);
        assert_eq!(ir.model.appearances, appearances);
        assert_eq!(ir.model.appearance_bindings, bindings);
    }
}

/// Hash a normalized B-rep partition and return its model.
fn brep_partition_sha256(
    ctx: &DecodeContext<'_>,
    tolerances: cadmpeg_ir::units::Tolerances,
    model: cadmpeg_ir::document::Model,
) -> (Result<String, CodecError>, cadmpeg_ir::document::Model) {
    let mut normalized = CadIr::empty();
    normalized.tolerances = tolerances;
    normalized.model = model;
    (
        cadmpeg_ir::hash::canonical_json_sha256(ctx, &normalized, "hash SLDPRT BREP partition")
            .map_err(Into::into),
        normalized.model,
    )
}

/// Machine-local `document_local_sha256` for the SLDPRT write-path edit oracle.
///
/// See [`cadmpeg_ir::hash::document_local_sha256`].
pub(crate) fn document_local_sha256(ir: &CadIr) -> Result<String, CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
    let digest = cadmpeg_ir::hash::document_local_sha256(
        &ctx,
        ir,
        ir.source.as_ref(),
        "sldprt",
        "sldprt:file:source-image#0",
        "record SLDPRT document digest",
    )?;
    ctx.finish_session()?;
    Ok(digest)
}

fn preserve_source_image(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    annotations: &mut Annotations,
    unknowns: &mut Vec<UnknownRecord>,
) -> Result<(), CodecError> {
    crate::annotations::note(
        ctx,
        annotations,
        "sldprt:file:source-image#0",
        &cadmpeg_ir::stream_name!("source"),
        0,
        "source_image",
        Exactness::ByteExact,
    )?;
    ctx.reserve_vec(unknowns, 1, "retain SLDPRT source image record")?;
    unknowns.push(UnknownRecord::retained(
        UnknownId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "file", "source-image"),
            cadmpeg_ir::identity_key!("0"),
        ),
        0,
        ctx.copy_retained(scan.source_image, "retain SLDPRT source image")?,
        Vec::new(),
    ));
    Ok(())
}

/// Builds the metadata-only report from the same classification the report
/// carries, including recoverable layer-identity collisions.
fn build_container_report(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    classification: &crate::dialect::LayerClassification,
    notes: Vec<String>,
) -> Result<DecodeBody, CodecError> {
    let parasolid_sources = ctx
        .admit_iter(
            &scan.blocks[..],
            "scan SLDPRT build_container_report values",
        )?
        .filter(|b| b.family == container::PayloadFamily::Parasolid)
        .count()
        + ctx
            .admit_iter(
                &scan.compound_streams[..],
                "scan SLDPRT build_container_report values",
            )?
            .filter(|stream| !stream.ps_streams.is_empty())
            .count();
    let payload_sources = scan.blocks.len() + scan.compound_streams.len();

    let mut losses = vec![
        SldprtLossCode::GeometryParasolidNotTransferred.note(format!(
            "Parasolid B-rep geometry was not transferred: no partition/deltas stream resolved \
             into a topology graph. {payload_sources} payload source(s) were enumerated, \
             {parasolid_sources} carrying Parasolid streams."
        )),
        SldprtLossCode::TopologyGraphNotTransferred.note(
            "B-rep topology graph (body/region/shell/face/loop/coedge/edge/vertex) was not built \
             for this file."
                .to_string(),
        ),
        SldprtLossCode::MaterialMetadataNotTransferred.note(
            "Body-bound appearances and tessellation were not transferred because no body graph \
             exists."
                .to_string(),
        ),
    ];

    if !container::has_parasolid_body_stream(ctx, scan)? {
        ctx.reserve_vec(&mut losses, 1, "append SLDPRT container loss")?;
        losses.push(
            SldprtLossCode::ContainerNoParasolidStream.note(
                "no Parasolid partition/deltas stream was located in the container".to_string(),
            ),
        );
    }
    append_swift_pmi_losses(ctx, scan, &mut losses)?;
    classification.append_losses(ctx, &mut losses)?;

    Ok(DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(false),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses,
        notes,
        transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
    })
}

fn append_swift_pmi_losses(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "format SLDPRT unsupported SWIFT classes";
    let (unsupported, _unsupported_storage) = ctx
        .with_scoped_storage("SLDPRT unsupported SWIFT workspace", || {
            crate::swift::unsupported_annotation_classes(ctx, scan)
        })?;
    if unsupported.is_empty() {
        return Ok(());
    }
    let mut count = 0_usize;
    let (mut classes, mut reservation) = ctx.scoped_string(0, OPERATION)?;
    for (index, (class, class_count)) in ctx
        .admit_iter(&unsupported, "scan SLDPRT append_swift_pmi_losses values")?
        .enumerate()
    {
        count = count
            .checked_add(*class_count)
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        reservation.with_storage(|| {
            if index > 0 {
                ctx.append_retained(&mut classes, ", ", OPERATION)?;
            }
            ctx.append_formatted_retained(
                &mut classes,
                format_args!("{class} ({class_count})"),
                OPERATION,
            )
        })?;
    }
    let message = ctx.format_retained(
        format_args!(
            "{count} SWIFT semantic annotation(s) have no neutral PMI definition: {classes}."
        ),
        "retain SLDPRT unsupported SWIFT loss",
    )?;
    ctx.reserve_vec(losses, 1, "append SLDPRT unsupported SWIFT loss")?;
    losses.push(SldprtLossCode::PmiSwiftAnnotationUnsupported.note(message));
    Ok(())
}

#[cfg(test)]
mod tests;
