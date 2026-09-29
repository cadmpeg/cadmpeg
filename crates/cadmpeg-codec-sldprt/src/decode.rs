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
use std::collections::btree_map::Entry;
use std::fmt::Write;
use std::hash::Hash;

use cadmpeg_core::decode::{DecodeContext, View};
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

use crate::container::configuration_index;
use crate::container::contains_ascii_case_insensitive;

use crate::brep::graph::{decode_bodies, Brep};
use crate::brep::feature_source::FeatureSourceId;
use crate::container::{self, ActiveParasolidSite, ContainerScan};
use crate::parasolid::StreamHeader;
use crate::records::ObjectId;
use cadmpeg_ir::geometry::SolvedCurveGeometry;

struct DecodedBrep {
    /// Representative stream whose header is common to every merged site.
    /// This can be present for an unresolved merge without selecting a site.
    metadata_header: Option<StreamHeader>,
    brep: Brep,
    configuration_bodies: Vec<(usize, Vec<cadmpeg_ir::ids::BodyId>)>,
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
    let container_entities = scan.compound_streams.len() as u64;
    ctx.charge_entities(container_entities, "admit SLDPRT container entities")?;
    let mut admitted_entities = 0_u64;

    if ctx.container_only() {
        let (ir, annotations, unknowns, mut pmi_losses) = build_metadata_ir(
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
        ctx.reserve_collection_vec(
            &mut report.losses,
            pmi_losses.len(),
            "append SLDPRT PMI losses",
        )?;
        report.losses.append(&mut pmi_losses);
        return decode_result(ir, report, annotations, unknowns);
    }

    let streams = active_body_streams(ctx, &scan)?;
    if !streams.is_empty() {
        ctx.charge_entities(streams.len() as u64, "admit SLDPRT body streams")?;
        if let Some((decoded, mut report)) = try_decode_brep(ctx, &scan, &streams, &classification)?
        {
            let (ir, annotations, unknowns, mut pmi_losses) = build_geometry_ir(
                ctx,
                &mut scan,
                &classification,
                decoded,
                form_padding,
                &mut admitted_entities,
            )?;
            ctx.reserve_collection_vec(
                &mut report.losses,
                pmi_losses.len(),
                "append SLDPRT PMI losses",
            )?;
            report.losses.append(&mut pmi_losses);
            append_tessellation_losses(ctx, &ir, &mut report)?;
            append_design_losses(ctx, &ir, &mut report)?;
            return decode_result(ir, report, annotations, unknowns);
        }
    }

    let (ir, annotations, unknowns, mut pmi_losses) = build_metadata_ir(
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
    ctx.reserve_collection_vec(
        &mut report.losses,
        pmi_losses.len(),
        "append SLDPRT PMI losses",
    )?;
    report.losses.append(&mut pmi_losses);
    append_design_losses(ctx, &ir, &mut report)?;
    decode_result(ir, report, annotations, unknowns)
}

fn push_report_loss(
    ctx: &DecodeContext<'_>,
    report: &mut DecodeBody,
    loss: cadmpeg_ir::report::loss::LossNote,
) -> Result<(), CodecError> {
    const OPERATION: &str = "append SLDPRT decode loss";
    ctx.charge_collection_items(1, OPERATION)?;
    report.losses.try_reserve(1).map_err(|_| {
        ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
    })?;
    report.losses.push(loss);
    Ok(())
}

fn append_tessellation_losses(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    report: &mut DecodeBody,
) -> Result<(), CodecError> {
    let unresolved = ir
        .model
        .tessellations
        .iter()
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
    mut ir: CadIr,
    body: DecodeBody,
    annotations: Annotations,
    mut unknowns: Vec<UnknownRecord>,
) -> Result<Decoded, CodecError> {
    let mut source_fidelity = cadmpeg_ir::SourceFidelity::with_annotations(annotations);
    let source_image = unknowns
        .iter()
        .position(|record| record.id().as_str() == "sldprt:file:source-image#0")
        .map(|index| unknowns.remove(index));
    source_fidelity.attach_native_unknown_records(&mut ir, "sldprt", unknowns)?;
    if let Some(source_image) = source_image {
        source_fidelity.retain_unknown_records("source", [source_image])?;
    }
    stamp_local_digests(&mut ir)?;
    Ok(Decoded {
        ir,
        body,
        source_fidelity,
    })
}

fn incomplete_pattern<C: cadmpeg_ir::features::patterns::CompositeStages>(
    pattern: &cadmpeg_ir::features::patterns::PatternKind<C>,
    incomplete_path: &dyn Fn(&cadmpeg_ir::features::PathRef) -> bool,
) -> bool {
    use cadmpeg_ir::features::patterns::{PatternScaleCenter, PatternTransform};

    match pattern.definition() {
        cadmpeg_ir::features::patterns::PatternTransform::Unresolved { .. } => true,
        PatternTransform::Linear { direction, .. }
        | PatternTransform::LinearOffsets { direction, .. } => direction.is_none(),
        PatternTransform::Circular { .. } | PatternTransform::Mirror { .. } => false,
        PatternTransform::MirrorReference { .. } => true,
        PatternTransform::CircularAngles { .. } => false,
        PatternTransform::CurveDriven { path, .. } => path.as_ref().is_none_or(incomplete_path),
        PatternTransform::Scale { center, .. } => matches!(center, PatternScaleCenter::Native(_)),
        PatternTransform::Composite { stages } => stages
            .stages()
            .iter()
            .any(|stage| incomplete_pattern(&stage.pattern, incomplete_path)),
    }
}

fn incomplete_binder_target(
    target: &cadmpeg_ir::features::BinderTarget,
    feature_positions: &BTreeMap<&cadmpeg_ir::features::FeatureId, u64>,
    consumer_ordinal: u64,
    dependencies: &[cadmpeg_ir::features::FeatureId],
) -> bool {
    match target {
        cadmpeg_ir::features::BinderTarget::Feature { feature } => {
            feature_positions
                .get(feature)
                .is_none_or(|ordinal| *ordinal >= consumer_ordinal)
                || !dependencies.contains(feature)
        }
        cadmpeg_ir::features::BinderTarget::External { document, object } => {
            document.as_str().trim().is_empty() || object.as_str().trim().is_empty()
        }
        cadmpeg_ir::features::BinderTarget::Native { .. } => true,
    }
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

fn count_keys<K: Ord>(
    ctx: &DecodeContext<'_>,
    keys: impl IntoIterator<Item = K>,
    operation: &'static str,
) -> Result<BTreeMap<K, usize>, CodecError> {
    let mut counts = BTreeMap::<K, usize>::new();
    for key in keys {
        ctx.charge_work(1, operation)?;
        match counts.entry(key) {
            Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, operation)?;
                entry.insert(1);
            }
            Entry::Occupied(mut entry) => {
                let next = entry.get().checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
                })?;
                *entry.get_mut() = next;
            }
        }
    }
    Ok(counts)
}

fn charged_map<K: Ord, V>(
    ctx: &DecodeContext<'_>,
    entries: impl IntoIterator<Item = (K, V)>,
    operation: &'static str,
) -> Result<BTreeMap<K, V>, CodecError> {
    let mut map = BTreeMap::new();
    for (key, value) in entries {
        ctx.charge_work(1, operation)?;
        match map.entry(key) {
            Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, operation)?;
                entry.insert(value);
            }
            Entry::Occupied(mut entry) => {
                entry.insert(value);
            }
        }
    }
    Ok(map)
}

fn charged_btree_set<T: Ord>(
    ctx: &DecodeContext<'_>,
    values: impl IntoIterator<Item = T>,
    operation: &'static str,
) -> Result<BTreeSet<T>, CodecError> {
    let mut set = BTreeSet::new();
    for value in values {
        ctx.charge_work(1, operation)?;
        if !set.contains(&value) {
            ctx.charge_collection_items(1, operation)?;
            set.insert(value);
        }
    }
    Ok(set)
}

fn charged_vec<T>(
    ctx: &DecodeContext<'_>,
    values: impl IntoIterator<Item = T>,
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let mut result = Vec::new();
    for value in values {
        ctx.charge_work(1, operation)?;
        ctx.charge_collection_items(1, operation)?;
        result.try_reserve(1).map_err(|_| {
            ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
        })?;
        result.push(value);
    }
    Ok(result)
}

fn charged_set<'a, T: Eq + Hash + ?Sized + 'a>(
    ctx: &DecodeContext<'_>,
    values: impl IntoIterator<Item = &'a T>,
    operation: &'static str,
) -> Result<HashSet<&'a T>, CodecError> {
    let mut set = HashSet::new();
    for value in values {
        insert_charged_set(ctx, &mut set, value, operation)?;
    }
    Ok(set)
}

fn insert_charged_set<'a, T: Eq + Hash + ?Sized>(
    ctx: &DecodeContext<'_>,
    set: &mut HashSet<&'a T>,
    value: &'a T,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_work(1, operation)?;
    if !set.contains(value) {
        ctx.charge_collection_items(1, operation)?;
        set.try_reserve(1).map_err(|_| {
            ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
        })?;
        set.insert(value);
    }
    Ok(())
}

fn charged_hash_map<K: Eq + Hash, V>(
    ctx: &DecodeContext<'_>,
    entries: impl IntoIterator<Item = (K, V)>,
    operation: &'static str,
) -> Result<HashMap<K, V>, CodecError> {
    let mut map = HashMap::new();
    for (key, value) in entries {
        ctx.charge_work(1, operation)?;
        if !map.contains_key(&key) {
            ctx.charge_collection_items(1, operation)?;
            map.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
            })?;
        }
        map.insert(key, value);
    }
    Ok(map)
}

fn has_incoherent_refs<T: Eq + Hash>(
    ctx: &DecodeContext<'_>,
    references: &[T],
    known: &HashSet<&T>,
    operation: &'static str,
) -> Result<bool, CodecError> {
    let mut seen = HashSet::new();
    for reference in references {
        ctx.charge_work(1, operation)?;
        if seen.contains(reference) || !known.contains(reference) {
            return Ok(true);
        }
        ctx.charge_collection_items(1, operation)?;
        seen.try_reserve(1).map_err(|_| {
            ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX)
        })?;
        seen.insert(reference);
    }
    Ok(false)
}

fn append_design_losses(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    report: &mut DecodeBody,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{
        AngularTermination, BodyRetentionMode, BodySelection, BooleanOp, EdgeSelection,
        ExtrudeExtent, FaceSelection, FeatureDefinition, FeatureOperation, FeatureSourceContent,
        LinearTermination, PathRef, PlanarProfileRef, ProfileRef, RevolveExtent, SplitFaceTool,
    };
    use cadmpeg_ir::sketches::{SketchGeometryDefinition, SpatialSketchGeometryDefinition};

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

    let active_configurations = ir
        .model
        .configurations
        .iter()
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
        .and_then(|source| source.attributes.get("active_parasolid_block"))
        .and_then(|section| crate::container::configuration_index(section))
        .and_then(|index| u32::try_from(index).ok());
    let active_partition_mismatch = active_partition.filter(|active_partition| {
        ir.model
            .configurations
            .iter()
            .find(|configuration| configuration.active)
            .is_some_and(|configuration| {
                configuration.source_index.as_ref() != Some(active_partition)
            })
    });
    if let Some(active_partition) = active_partition_mismatch {
        push_report_loss(ctx, report, SldprtLossCode::ConfigActivePartitionMismatch.note(format!(
                "active configuration identity does not resolve to active geometry partition {active_partition}."
            )))?;
    }
    let inferred_configurations = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| configuration.native_ref.is_none())
        .count();
    if inferred_configurations > 0 {
        push_report_loss(ctx, report, SldprtLossCode::ConfigInferredWithoutNative.note(format!(
                "{inferred_configurations} configuration state(s) are inferred from geometry partitions without native configuration definitions."
            )))?;
    }
    let unresolved_configuration_parameter_lanes = native.as_ref().map_or(0, |native| {
        crate::history::configuration::unresolved_configuration_lanes(
            &ir.model.configurations,
            &native.feature_input_lanes,
        )
    });
    if unresolved_configuration_parameter_lanes > 0 {
        push_report_loss(ctx, report, SldprtLossCode::ConfigLaneIdentityUnresolved.note(format!(
                "{unresolved_configuration_parameter_lanes} configuration-scoped feature-input lane(s) have duplicate or unresolved configuration identity."
            )))?;
    }
    let configuration_source_counts = count_keys(
        ctx,
        ir.model.configurations.iter().filter_map(|configuration| configuration.source_index),
        "count SLDPRT configuration source indices",
    )?;
    let ambiguous_configuration_sources = configuration_source_counts
        .values()
        .filter(|count| **count > 1)
        .copied()
        .sum::<usize>();
    if ambiguous_configuration_sources > 0 {
        push_report_loss(ctx, report, SldprtLossCode::ConfigAmbiguousPartition.note(format!(
                "{ambiguous_configuration_sources} configuration record(s) share non-unique geometry partition identities."
            )))?;
    }
    let empty_configuration_names = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| configuration.name.as_deref().is_none_or(str::is_empty))
        .count();
    let configuration_ordinal_counts = count_keys(
        ctx,
        ir.model.configurations.iter().map(|configuration| configuration.ordinal),
        "count SLDPRT configuration ordinals",
    )?;
    let configuration_name_counts = count_keys(
        ctx,
        ir.model.configurations.iter()
            .filter_map(|configuration| configuration.name.as_deref())
            .filter(|name| !name.is_empty()),
        "count SLDPRT configuration names",
    )?;
    let ambiguous_configuration_names = configuration_name_counts
        .values()
        .filter(|count| **count > 1)
        .copied()
        .sum::<usize>();
    let ambiguous_configuration_ordinals = configuration_ordinal_counts
        .values()
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
    let model_body_ids = charged_set(
        ctx,
        ir.model.bodies.iter().map(|body| &body.id),
        "index SLDPRT model body IDs",
    )?;
    let mut incoherent_configuration_bodies = 0;
    for configuration in &ir.model.configurations {
        if has_incoherent_refs(
            ctx,
            configuration.bodies.as_deref().unwrap_or_default(),
            &model_body_ids,
            "check SLDPRT configuration body references",
        )? {
            incoherent_configuration_bodies += 1;
        }
    }
    let unresolved_configuration_bodies = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| configuration.bodies.is_none())
        .count();
    if unresolved_configuration_bodies > 0 || incoherent_configuration_bodies > 0 {
        push_report_loss(ctx, report, SldprtLossCode::ConfigIncoherentBodyRefs.note(format!(
                "{unresolved_configuration_bodies} configuration record(s) have unresolved body membership; {incoherent_configuration_bodies} configuration record(s) contain missing or repeated body references."
            )))?;
    }

    let feature_ids = charged_set(
        ctx,
        ir.model.features.iter().map(|feature| &feature.id),
        "index SLDPRT feature IDs",
    )?;
    let parameter_ids = charged_set(
        ctx,
        ir.model.parameters.iter().map(|parameter| &parameter.id),
        "index SLDPRT parameter IDs",
    )?;
    let incomplete_configuration_feature_snapshots = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| {
            !configuration_source_needs_update(ir, configuration)
                && (configuration.feature_states.len() != feature_ids.len()
                    || configuration
                        .feature_states
                        .keys()
                        .any(|feature| !feature_ids.contains(feature)))
        })
        .count();
    let incomplete_configuration_parameter_snapshots = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| {
            !configuration_source_needs_update(ir, configuration)
                && (configuration.parameter_values.len() != parameter_ids.len()
                    || configuration
                        .parameter_values
                        .keys()
                        .any(|parameter| !parameter_ids.contains(parameter)))
        })
        .count();
    if incomplete_configuration_feature_snapshots > 0
        || incomplete_configuration_parameter_snapshots > 0
    {
        push_report_loss(ctx, report, SldprtLossCode::ConfigIncompleteSnapshot.note(format!(
                "{incomplete_configuration_feature_snapshots} configuration(s) lack a complete evaluated feature snapshot; {incomplete_configuration_parameter_snapshots} configuration(s) lack a complete evaluated parameter snapshot."
            )))?;
    }
    let incoherent_configuration_suppression = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| {
            configuration.feature_states.iter().any(|(id, state)| {
                !feature_ids.contains(id)
                    || (configuration.active
                        && ir
                            .model
                            .features
                            .iter()
                            .find(|feature| feature.id == *id)
                            .is_some_and(|feature| {
                                feature.suppressed.is_some_and(|suppressed| {
                                    suppressed != state.evaluation.is_suppressed()
                                })
                            }))
            })
        })
        .count();
    let incoherent_configuration_overrides = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| {
            configuration
                .parameter_overrides
                .keys()
                .any(|parameter| !parameter_ids.contains(parameter))
        })
        .count();
    if incoherent_configuration_suppression > 0 || incoherent_configuration_overrides > 0 {
        push_report_loss(ctx, report, SldprtLossCode::ConfigIncompleteSnapshot.note(format!(
            "{incoherent_configuration_suppression} configuration(s) have missing, repeated, or feature-state-inconsistent suppression members; {incoherent_configuration_overrides} configuration(s) reference missing parameter overrides."
        )))?;
    }

    let mut feature_names = HashMap::new();
    for feature in &ir.model.features {
        let Some(name) = &feature.name else {
            continue;
        };
        const OPERATION: &str = "index SLDPRT feature names";
        ctx.charge_work(1, OPERATION)?;
        if !feature_names.contains_key(&feature.id) {
            ctx.charge_collection_items(1, OPERATION)?;
            feature_names.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
            })?;
        }
        let bytes = feature.id.as_str().len().checked_add(name.len()).ok_or_else(|| {
            ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
        })?;
        ctx.charge_retained(bytes as u64, OPERATION)?;
        feature_names.insert(feature.id.clone(), name.clone());
    }
    let mut global_parameter_owners = HashSet::new();
    for feature in &ir.model.features {
        const OPERATION: &str = "index SLDPRT global parameter owners";
        ctx.charge_work(1, OPERATION)?;
        if !crate::history::parameters::is_global_parameter_owner(feature)
            || global_parameter_owners.contains(&feature.id)
        {
            continue;
        }
        ctx.charge_collection_items(1, OPERATION)?;
        global_parameter_owners.try_reserve(1).map_err(|_| {
            ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
        })?;
        ctx.charge_retained(feature.id.as_str().len() as u64, OPERATION)?;
        global_parameter_owners.insert(feature.id.clone());
    }
    let incomplete_parameters = ir
        .model
        .parameters
        .iter()
        .filter(|parameter| {
            parameter.value.is_none()
                && (ir.model.configurations.is_empty()
                    || ir.model.configurations.iter().any(|configuration| {
                        !configuration.parameter_values.contains_key(&parameter.id)
                    }))
        })
        .count();
    let unresolved_parameter_references =
        crate::history::parameters::parameters_with_unresolved_references(
            &ir.model.parameters,
            &feature_names,
            &global_parameter_owners,
        );
    let unevaluable_parameter_expressions =
        crate::history::parameters::parameters_with_unevaluable_expressions(
            &ir.model.parameters,
            &feature_names,
            &global_parameter_owners,
            &ir.model.configurations,
        );
    let feature_ordinals = charged_map(
        ctx,
        ir.model.features.iter().map(|feature| (&feature.id, feature.ordinal)),
        "index SLDPRT feature ordinals",
    )?;
    let parameter_positions = charged_map(
        ctx,
        ir.model.parameters.iter()
            .map(|parameter| (&parameter.id, (&parameter.owner, parameter.ordinal))),
        "index SLDPRT parameter positions",
    )?;
    let invalid_parameter_dependency_order = ir
        .model
        .parameters
        .iter()
        .filter(|parameter| {
            parameter.dependencies.iter().any(|dependency| {
                let Some((owner, ordinal)) = parameter_positions.get(dependency) else {
                    return true;
                };
                if *owner == &parameter.owner {
                    return *ordinal >= parameter.ordinal;
                }
                let (Some(owner), Some(parameter_owner)) =
                    (owner.as_ref(), parameter.owner.as_ref())
                else {
                    return true;
                };
                feature_ordinals
                    .get(owner)
                    .zip(feature_ordinals.get(parameter_owner))
                    .is_none_or(|(dependency_owner, consumer_owner)| {
                        dependency_owner >= consumer_owner
                    })
            })
        })
        .count();
    let incoherent_parameter_dependencies =
        crate::history::parameters::parameters_with_incoherent_dependencies(
            &ir.model.parameters,
            &feature_names,
            &global_parameter_owners,
        );
    let incoherent_parameter_values =
        crate::history::parameters::parameters_with_incoherent_evaluated_values(
            &ir.model.parameters,
            &feature_names,
            &global_parameter_owners,
            &ir.model.configurations,
        );
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
    let empty_parameter_names = ir
        .model
        .parameters
        .iter()
        .filter(|parameter| parameter.name.is_empty())
        .count();
    let parameter_name_counts = count_keys(
        ctx,
        ir.model.parameters.iter().filter(|parameter| !parameter.name.is_empty())
            .map(|parameter| (&parameter.owner, parameter.name.as_str())),
        "count SLDPRT parameter names",
    )?;
    let parameter_ordinal_counts = count_keys(
        ctx,
        ir.model.parameters.iter().map(|parameter| (&parameter.owner, parameter.ordinal)),
        "count SLDPRT parameter ordinals",
    )?;
    let duplicate_parameter_names = parameter_name_counts
        .values()
        .filter(|count| **count > 1)
        .copied()
        .sum::<usize>();
    let duplicate_parameter_ordinals = parameter_ordinal_counts
        .values()
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

    let mut bound_pmi = std::collections::HashSet::new();
    for pmi in ir.model.parameters.iter().filter_map(|parameter| parameter.pmi.as_ref()) {
        let id = pmi.native_ref.as_str();
        if !bound_pmi.contains(id) {
            ctx.charge_collection_items(1, "index SLDPRT bound PMI IDs")?;
            bound_pmi.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("index SLDPRT bound PMI IDs", u64::MAX - 1, u64::MAX)
            })?;
            bound_pmi.insert(id);
        }
    }
    let unbound_pmi_dimensions = match native.as_ref() {
        Some(native) => crate::pmi::unbound_dimension_count(ctx, &native.pmi_dimensions, &bound_pmi)?,
        None => 0,
    };
    let native_pmi_subtypes = ir
        .model
        .parameters
        .iter()
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

    let incomplete_history_references = native.as_ref().map_or(0, |native| {
        crate::history::project::incomplete_history_reference_features(&native.feature_histories)
    });
    if incomplete_history_references > 0 {
        push_report_loss(ctx, report, SldprtLossCode::HistoryIncompleteReferences.note(format!(
            "{incomplete_history_references} feature history record(s) contain duplicate identities or unresolved parent, dependency, dimension, or child references."
        )))?;
    }
    let feature_positions = &feature_ordinals;
    let evaluated_feature_states = if ir
        .model
        .configurations
        .iter()
        .any(|configuration| !configuration.feature_states.is_empty())
    {
        charged_vec(
            ctx,
            ir.model.configurations.iter().flat_map(|configuration| {
                ir.model.features.iter().filter_map(move |feature| {
                    configuration.feature_states.get(&feature.id).map(|state| {
                        EvaluatedFeatureState {
                            feature,
                            dependencies: &state.dependencies,
                            outputs: state.evaluation.outputs(),
                            definition: &state.definition,
                        }
                    })
                })
            }),
            "collect SLDPRT configured feature states",
        )?
    } else {
        charged_vec(
            ctx,
            ir.model.features.iter().map(|feature| EvaluatedFeatureState {
                feature,
                dependencies: &feature.dependencies,
                outputs: feature.evaluation.outputs(),
                definition: feature.evaluation.definition(),
            }),
            "collect SLDPRT feature states",
        )?
    };
    let incoherent_feature_edges = evaluated_feature_states
        .iter()
        .filter(|state| {
            let feature = state.feature;
            let parent_incoherent = ir.model.feature_parent(&feature.id).is_some_and(|parent| {
                feature_positions
                    .get(parent)
                    .is_none_or(|ordinal| *ordinal >= feature.ordinal)
            });
            parent_incoherent
                || state.dependencies.iter().any(|dependency| {
                    feature_positions
                        .get(dependency)
                        .is_none_or(|ordinal| *ordinal >= feature.ordinal)
                })
        })
        .count();
    let feature_ordinal_counts = count_keys(
        ctx,
        ir.model.features.iter().map(|feature| feature.ordinal),
        "count SLDPRT feature ordinals",
    )?;
    let duplicate_feature_ordinals = feature_ordinal_counts
        .values()
        .filter(|count| **count > 1)
        .copied()
        .sum::<usize>();
    if incoherent_feature_edges > 0 || duplicate_feature_ordinals > 0 {
        push_report_loss(ctx, report, SldprtLossCode::FeatureIncoherentEdges.note(format!(
                "{incoherent_feature_edges} feature record(s) contain missing, repeated, or non-preceding parent/dependency edges; {duplicate_feature_ordinals} feature record(s) share regeneration ordinals."
            )))?;
    }
    let parameter_owners = charged_map(
        ctx,
        ir.model.parameters.iter().map(|parameter| (&parameter.id, &parameter.owner)),
        "index SLDPRT parameter owners",
    )?;
    let features_by_id = charged_map(
        ctx,
        ir.model.features.iter().map(|feature| (&feature.id, feature)),
        "index SLDPRT features by ID",
    )?;
    let incoherent_feature_content = ir
        .model
        .features
        .iter()
        .filter(|feature| {
            feature.source_content.iter().any(|content| match content {
                FeatureSourceContent::Text(_) => false,
                FeatureSourceContent::Parameter(parameter) => parameter_owners
                    .get(parameter)
                    .is_none_or(|owner| owner.as_ref() != Some(&feature.id)),
                FeatureSourceContent::Feature(child) => {
                    features_by_id.get(child).is_none_or(|child| {
                        child.ordinal <= feature.ordinal
                            || ir.model.feature_parent(&child.id) != Some(&feature.id)
                    })
                }
            })
        })
        .count();
    if incoherent_feature_content > 0 {
        push_report_loss(ctx, report, SldprtLossCode::FeatureIncoherentContent.note(format!(
                "{incoherent_feature_content} feature record(s) contain missing, repeated, misowned, or structurally inconsistent source-content references."
            )))?;
    }

    let unresolved_output_scopes = evaluated_feature_states
        .iter()
        .filter(|state| {
            state
                .feature
                .source_properties
                .get("Scope")
                .is_some_and(|scope| !scope.trim().is_empty())
                && state.outputs.is_empty()
        })
        .count();
    if unresolved_output_scopes > 0 {
        push_report_loss(ctx, report, SldprtLossCode::FeatureUnresolvedOutputScope.note(format!(
                "{unresolved_output_scopes} feature(s) retain non-empty native output scopes that do not resolve to model bodies."
            )))?;
    }
    let mut incoherent_feature_outputs = 0;
    for state in &evaluated_feature_states {
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

    let native_planar_constraints = ir
        .model
        .sketch_constraints
        .iter()
        .filter(|constraint| {
            !sketch_constraint_has_complete_neutral_semantics(constraint.definition.kind())
                && constraint.active != Some(false)
        })
        .count();
    let native_spatial_constraints = ir
        .model
        .spatial_sketch_constraints
        .iter()
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

    let native_sketch_geometry = ir
        .model
        .sketch_entities
        .iter()
        .filter(|entity| {
            matches!(
                *entity.geometry.definition(),
                SketchGeometryDefinition::Native { .. }
            )
        })
        .count()
        + ir.model
            .spatial_sketch_entities
            .iter()
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

    let native_features = evaluated_feature_states
        .iter()
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
    let incomplete_typed_features = evaluated_feature_states
        .iter()
        .filter(|state| {
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
            FeatureOperation::InsertComponent { occurrence } => !ir
                .model
                .occurrences
                .iter()
                .any(|candidate| candidate.id == *occurrence),
            FeatureOperation::AssemblyJoint { joint } => !ir
                .model
                .assembly_joints
                .iter()
                .any(|candidate| candidate.id == *joint),
            FeatureOperation::ReferenceImage { asset, .. }
            | FeatureOperation::Decal { asset, .. } => {
                !ir.model.assets.iter().any(|candidate| candidate.id == *asset)
            }
            FeatureOperation::StoredGeometry {} => state.outputs.is_empty(),
            FeatureOperation::ExtractBody { source } => incomplete_body_selection(source),
            FeatureOperation::DerivedGeometry { source } => {
                feature_positions
                    .get(source)
                    .is_none_or(|ordinal| *ordinal >= state.feature.ordinal)
                    || !state.dependencies.contains(source)
            }
            FeatureOperation::ImportedGeometry { path, .. } => path.trim().is_empty(),
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
                segments.is_empty() || segments.iter().any(incomplete_path)
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
                shape.any_section_is_unresolved()
                    || shape
                        .referenced_profiles()
                        .into_iter()
                        .any(incomplete_planar_profile)
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
                    || sources.iter().any(|source| {
                        incomplete_binder_target(
                            &source.target,
                            &feature_positions,
                            state.feature.ordinal,
                            state.dependencies,
                        ) || source
                            .subelements
                            .iter()
                            .any(|subelement| subelement.as_str().trim().is_empty())
                    })
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
                        } if incomplete_binder_target(
                            context,
                            &feature_positions,
                            state.feature.ordinal,
                            state.dependencies,
                        )
                    )
            }
            FeatureOperation::Loft {
                sections,
                guidance,
                op,
                ..
            } => {
                sections.len() < 2
                    || sections.iter().any(|section| match section {
                        cadmpeg_ir::features::LoftSection::Profile(profile) => incomplete_profile(profile),
                        cadmpeg_ir::features::LoftSection::Point(cadmpeg_ir::features::LoftPointSection::Native(_)) => true,
                        cadmpeg_ir::features::LoftSection::Point(_) => false,
                    })
                    || match guidance {
                        cadmpeg_ir::features::LoftGuidance::Guides(guides) => {
                            guides.iter().any(incomplete_path)
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
                    || groups.iter().any(|group| {
                        incomplete_edge_selection(&group.edges)
                            || group.radius.is_unresolved()
                    })
            }
            FeatureOperation::FullRoundFillet { groups } => {
                groups.is_empty()
                    || groups.iter().any(|group| {
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
                    })
            }
            FeatureOperation::Chamfer { groups, .. } => groups.is_empty() || groups.iter().any(|group| {
                incomplete_edge_selection(&group.edges) || group.spec.is_unresolved()
            }),
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
                    || *keep == cadmpeg_ir::features::TrimRegion::Unresolved
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
                    || cells.iter().any(incomplete_body_selection)
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
                    || seeds.iter().any(|seed| match seed {
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
                    })
                    || incomplete_pattern(pattern, &incomplete_path)
            }
            FeatureOperation::Native { .. } => false,
            // Unresolved construction retained as native.
            FeatureOperation::Unresolved { .. } => true,
            }
        })
        .count();
    if incomplete_typed_features > 0 {
        push_report_loss(ctx, report, SldprtLossCode::FeatureTypedOperandIncomplete.note(format!(
            "{incomplete_typed_features} typed feature(s) retain native or unresolved required operation operands."
        )))?;
    }

    let unresolved_body_modes = evaluated_feature_states
        .iter()
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
    ir: &CadIr,
    configuration: &cadmpeg_ir::features::DesignConfiguration,
) -> bool {
    let slot = configuration
        .properties
        .get("id")
        .and_then(|value| value.parse::<u32>().ok())
        .or(configuration.source_index)
        .unwrap_or(configuration.ordinal);
    ir.source
        .as_ref()
        .and_then(|source| {
            source
                .attributes
                .get(format!("sw_configuration_{slot}_needs_update").as_str())
        })
        .is_some_and(|value| value.eq_ignore_ascii_case("yes"))
}

fn unbound_feature_input_operation_objects(
    ctx: &DecodeContext<'_>,
    native: &crate::native::SldprtNative,
) -> Result<usize, CodecError> {
    use crate::classification::{classify, native_object_class};
    use crate::records::FeatureInputClassRole;

    let features = native.feature_histories.iter().flat_map(|history| &history.features);
    let source_counts = count_keys(
        ctx,
        features.clone().filter_map(|feature| feature.source_value()),
        "count SLDPRT feature-input sources",
    )?;
    let binding_counts = count_keys(
        ctx,
        features.clone().filter_map(|feature| {
            feature.source_value().zip(feature.input_class.as_deref())
        }),
        "count SLDPRT feature-input bindings",
    )?;
    let named_binding_counts = count_keys(
        ctx,
        native.feature_input_lanes.iter().flat_map(|lane| {
            features.clone().filter_map(move |feature| {
                feature.input_class.as_deref().zip(
                    crate::resolved_features::scalars::feature_object_name(feature, lane),
                ).map(|(class, name)| (lane.id.as_str(), name.id.as_str(), class))
            })
        }),
        "count SLDPRT named feature-input bindings",
    )?;
    Ok(native
        .feature_input_lanes
        .iter()
        .flat_map(|lane| {
            lane.classes
                .iter()
                .filter(|class| class.role() == FeatureInputClassRole::Feature)
                .filter_map(move |class| {
                    let name_offset = class.offset + 6 + class.name.len() as u64;
                    lane.names
                        .iter()
                        .find(|name| name.offset == name_offset)
                        .map(|name| (lane, class, name))
                })
        })
        .filter(|(lane, class, name)| {
            let source_bound = name.object_id.and_then(ObjectId::value).is_some_and(|id| {
                source_counts.get(&id).copied() == Some(1)
                    && (binding_counts.get(&(id, class.name.as_str())).copied() == Some(1)
                        || native_object_class(&class.name)
                            .feature()
                            .is_some_and(|expected| {
                                native
                                    .feature_histories
                                    .iter()
                                    .flat_map(|history| &history.features)
                                    .any(|feature| {
                                        feature.source_value() == Some(id)
                                            && feature.input_class.is_none()
                                            && classify(feature) == Some(expected)
                                    })
                            }))
            });
            let name_bound = named_binding_counts
                .get(&(lane.id.as_str(), name.id.as_str(), class.name.as_str()))
                .copied()
                == Some(1);
            !(source_bound || name_bound)
        })
        .count())
}

fn unprojected_sketch_relation_records(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    native: &crate::native::SldprtNative,
) -> Result<usize, CodecError> {
    let sketch_feature_refs = charged_set(
        ctx,
        ir.model.features.iter().filter(|feature| {
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
    )?;
    let projected = charged_set(
        ctx,
        ir.model.sketch_constraints.iter()
        .filter_map(|constraint| constraint.native_ref.as_deref())
        .chain(
            ir.model
                .sketch_entities
                .iter()
                .filter_map(|entity| entity.native_ref.as_deref()),
        )
        .chain(
            ir.model
                .spatial_sketch_entities
                .iter()
                .filter_map(|entity| entity.native_ref.as_deref()),
        )
        .chain(
            ir.model
                .spatial_sketch_constraints
                .iter()
                .filter_map(|constraint| constraint.native_ref.as_deref()),
        ),
        "index SLDPRT projected sketch relations",
    )?;
    let owned_instances = crate::resolved_features::relation_geometry::owned_relation_parameters(
        ctx,
        &ir.model.features,
        &ir.model.parameters,
        &native.feature_input_lanes,
    )?;

    let mut total = 0;
    for lane in &native.feature_input_lanes {
            let markers_by_id = charged_hash_map(
                ctx,
                lane.sketch_entities.iter().map(|marker| (marker.id(), marker)),
                "index SLDPRT sketch relation markers",
            )?;
            let instances = lane
                .relation_instances
                .iter()
                .filter(|relation| {
                    sketch_feature_refs.contains(relation.feature_ref.as_str())
                        && owned_instances.contains_key(&relation.id)
                        && !projected.contains(relation.id.as_str())
                })
                .count();
            let bindings = lane
                .relation_bindings
                .iter()
                .filter(|binding| {
                    binding
                        .feature_ref
                        .as_deref()
                        .is_some_and(|feature_ref| sketch_feature_refs.contains(feature_ref))
                        && !lane.relation_instances.iter().any(|relation| {
                            relation.class_ref == binding.class_ref
                                && relation.scalar_refs().contains(&binding.scalar_ref)
                        })
                })
                .count();
            let markers = lane
                .sketch_entities
                .iter()
                .filter(|marker| {
                    marker
                        .feature_ref
                        .as_deref()
                        .is_some_and(|feature_ref| sketch_feature_refs.contains(feature_ref))
                        && crate::resolved_features::typed_relations::marker_owns_constraint(
                            marker,
                            &markers_by_id,
                        )
                        && !projected.contains(marker.id())
                })
                .count();
            total += instances + bindings + markers;
    }
    Ok(total)
}

fn multiply_projected_sketch_relation_records(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    native: &crate::native::SldprtNative,
) -> Result<usize, CodecError> {
    let mut native_relation_ids = HashSet::new();
    for lane in &native.feature_input_lanes {
        let markers_by_id = charged_hash_map(
            ctx,
            lane.sketch_entities.iter().map(|marker| (marker.id(), marker)),
            "index SLDPRT sketch relation markers",
        )?;
        for relation in &lane.relation_instances {
            insert_charged_set(
                ctx,
                &mut native_relation_ids,
                relation.id.as_str(),
                "index SLDPRT native relation IDs",
            )?;
        }
        for marker in &lane.sketch_entities {
            if crate::resolved_features::typed_relations::marker_owns_constraint(
                marker,
                &markers_by_id,
            ) {
                insert_charged_set(
                    ctx,
                    &mut native_relation_ids,
                    marker.id(),
                    "index SLDPRT native relation IDs",
                )?;
            }
        }
    }
    let projection_counts = count_keys(
        ctx,
        ir
        .model
        .sketch_constraints
        .iter()
        .filter_map(|constraint| constraint.native_ref.as_deref())
        .chain(
            ir.model
                .sketch_entities
                .iter()
                .filter_map(|entity| entity.native_ref.as_deref()),
        )
        .chain(
            ir.model
                .spatial_sketch_entities
                .iter()
                .filter_map(|entity| entity.native_ref.as_deref()),
        )
        .chain(
            ir.model
                .spatial_sketch_constraints
                .iter()
                .filter_map(|constraint| constraint.native_ref.as_deref()),
        )
        .filter(|native_ref| native_relation_ids.contains(native_ref)),
        "count SLDPRT relation projections",
    )?;
    Ok(projection_counts
        .values()
        .filter(|count| **count > 1)
        .count())
}

fn copy_retained_string(
    ctx: &DecodeContext<'_>,
    value: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    let mut copy = String::new();
    ctx.reserve_retained_string(&mut copy, value.len(), operation)?;
    copy.push_str(value);
    Ok(copy)
}

fn conflicting_display_reference(
    ctx: &DecodeContext<'_>,
    stream: &str,
    table_index: usize,
    candidates: &BTreeSet<FeatureSourceId>,
) -> Result<String, CodecError> {
    const OPERATION: &str = "retain SLDPRT conflicting display reference";
    let index_digits = usize::try_from(table_index.checked_ilog10().unwrap_or(0)).map_err(|_| {
        ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
    })? + 1;
    let mut bytes = stream.len();
    for part in ["::DisplayFace[".len(), index_digits, "] (".len(), 1] {
        bytes = bytes.checked_add(part).ok_or_else(|| {
            ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
        })?;
    }
    for (position, source) in candidates.iter().enumerate() {
        let digits = usize::try_from(source.value().ilog10()).map_err(|_| {
            ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
        })? + 1;
        bytes = bytes.checked_add(digits + usize::from(position > 0) * 2).ok_or_else(|| {
            ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
        })?;
    }
    let mut message = String::new();
    ctx.reserve_retained_string(&mut message, bytes, OPERATION)?;
    message.push_str(stream);
    message.push_str("::DisplayFace[");
    write!(&mut message, "{table_index}").map_err(|_| {
        ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
    })?;
    message.push_str("] (");
    for (position, source) in candidates.iter().enumerate() {
        if position > 0 {
            message.push_str(", ");
        }
        write!(&mut message, "{}", source.value()).map_err(|_| {
            ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
        })?;
    }
    message.push(')');
    Ok(message)
}

fn appearance_assignment_loss_message(
    ctx: &DecodeContext<'_>,
    assigned: &BTreeSet<FeatureSourceId>,
    matched: &BTreeSet<FeatureSourceId>,
    conflicts: &[String],
) -> Result<Option<String>, CodecError> {
    const OPERATION: &str = "retain SLDPRT appearance assignment loss";
    const PREFIX: &str = "VisualStates feature appearance assignment unresolved: ";
    const MISSING_PREFIX: &str = "feature source ID(s) ";
    const MISSING_SUFFIX: &str = " have no agreeing DisplayFace persistent reference";
    const CONFLICT_PREFIX: &str = "conflicting references rejected for ";
    let has_unmatched = assigned.difference(matched).next().is_some();
    if !has_unmatched && conflicts.is_empty() {
        return Ok(None);
    }
    let mut bytes = PREFIX.len();
    let add = |bytes: &mut usize, part: usize| -> Result<(), CodecError> {
        *bytes = bytes.checked_add(part).ok_or_else(|| {
            ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
        })?;
        Ok(())
    };
    add(&mut bytes, 1)?;
    if has_unmatched {
        add(&mut bytes, MISSING_PREFIX.len())?;
        add(&mut bytes, MISSING_SUFFIX.len())?;
        for (position, source) in assigned.difference(matched).enumerate() {
            let digits = usize::try_from(source.value().ilog10()).map_err(|_| {
                ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
            })? + 1;
            add(&mut bytes, digits)?;
            if position > 0 {
                add(&mut bytes, ", ".len())?;
            }
        }
    }
    if !conflicts.is_empty() {
        if has_unmatched {
            add(&mut bytes, "; ".len())?;
        }
        add(&mut bytes, CONFLICT_PREFIX.len())?;
        for (position, conflict) in conflicts.iter().enumerate() {
            add(&mut bytes, conflict.len())?;
            if position > 0 {
                add(&mut bytes, "; ".len())?;
            }
        }
    }
    let mut message = String::new();
    ctx.reserve_retained_string(&mut message, bytes, OPERATION)?;
    message.push_str(PREFIX);
    if has_unmatched {
        message.push_str(MISSING_PREFIX);
        for (position, source) in assigned.difference(matched).enumerate() {
            if position > 0 {
                message.push_str(", ");
            }
            write!(&mut message, "{}", source.value()).map_err(|_| {
                ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
            })?;
        }
        message.push_str(MISSING_SUFFIX);
    }
    if !conflicts.is_empty() {
        if has_unmatched {
            message.push_str("; ");
        }
        message.push_str(CONFLICT_PREFIX);
        for (position, conflict) in conflicts.iter().enumerate() {
            if position > 0 {
                message.push_str("; ");
            }
            message.push_str(conflict);
        }
    }
    message.push('.');
    Ok(Some(message))
}

/// Collect the available Parasolid body streams, excluding auxiliary sites.
fn active_body_streams<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
) -> Result<Vec<ActiveParasolidSite<'a>>, CodecError> {
    let mut streams = Vec::new();
    for section in scan.sections() {
        let name = section.name().unwrap_or("");
        if contains_ascii_case_insensitive(name, "ghost")
            || contains_ascii_case_insensitive(name, "resolvedfeatures")
        {
            continue;
        }
        for stream in section.ps_streams() {
            if !crate::parasolid::is_body_stream(&stream.header) {
                continue;
            }
            ctx.reserve_collection_vec(&mut streams, 1, "collect SLDPRT body streams")?;
            streams.push(ActiveParasolidSite {
                section,
                payload: &stream.payload,
                header: &stream.header,
            });
        }
    }
    streams.sort_by_key(|stream| {
        (
            !contains_ascii_case_insensitive(stream.source_stream().as_str(), "partition"),
            !contains_ascii_case_insensitive(&stream.header.description, "partition"),
        )
    });
    Ok(streams)
}

/// Decode the available Parasolid body streams into one B-rep. Returns
/// `Ok(None)` when the streams frame but yield neither geometry nor a valid
/// empty partition/deltas model, so the caller falls back to metadata. A
/// framed stream that fails semantic decoding returns its error to the caller;
/// it must not be mistaken for a metadata-only document.
fn try_decode_brep(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    streams: &[ActiveParasolidSite<'_>],
    classification: &crate::dialect::LayerClassification,
) -> Result<Option<(DecodedBrep, DecodeBody)>, CodecError> {
    let mut sites: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, stream) in streams.iter().enumerate() {
        match sites.entry(stream.site_key()) {
            Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "collect SLDPRT B-rep sites")?;
                let mut indices = Vec::new();
                ctx.reserve_collection_vec(&mut indices, 1, "collect SLDPRT site streams")?;
                indices.push(index);
                entry.insert(indices);
            }
            Entry::Occupied(mut entry) => {
                let indices = entry.get_mut();
                ctx.reserve_collection_vec(indices, 1, "collect SLDPRT site streams")?;
                indices.push(index);
            }
        }
    }
    let mut decoded_sites = Vec::new();
    for (site, indices) in &sites {
        let first = indices[0];
        let mut bodies = Vec::new();
        ctx.reserve_collection_vec(&mut bodies, indices.len(), "collect SLDPRT site bodies")?;
        for index in indices {
            bodies.push((streams[*index].payload, streams[*index].header));
        }
        let decoded = decode_bodies(ctx, &bodies, streams[first].source_stream())?;
        ctx.reserve_collection_vec(&mut decoded_sites, 1, "collect decoded SLDPRT sites")?;
        decoded_sites.push((site.clone(), first, decoded));
    }
    if decoded_sites.is_empty() {
        return Ok(None);
    }
    let active_site = container::select_active_parasolid_site(scan).map(|site| site.site_key());
    let resolved_active_site = active_site
        .as_ref()
        .and_then(|active| decoded_sites.iter().position(|(site, _, _)| site == active));
    // Without a resolved active site, this is only a deterministic merge
    // accumulator. All site identities are qualified below.
    let selected_site = resolved_active_site.unwrap_or(0);
    let selected_is_empty_model = decoded_sites[selected_site].2.stats.source_entity_records == 0
        && sites[&decoded_sites[selected_site].0].iter().any(|index| {
            contains_ascii_case_insensitive(&streams[*index].header.description, "partition")
        })
        && sites[&decoded_sites[selected_site].0].iter().any(|index| {
            contains_ascii_case_insensitive(&streams[*index].header.description, "deltas")
        });
    let selected_has_geometry = !decoded_sites[selected_site].2.faces.is_empty()
        || !decoded_sites[selected_site].2.surfaces.is_empty()
        || !decoded_sites[selected_site].2.points.is_empty();
    if resolved_active_site.is_some() {
        if !selected_is_empty_model && !selected_has_geometry {
            return Ok(None);
        }
    } else {
        let any_site_has_geometry = decoded_sites.iter().any(|(_, _, decoded)| {
            !decoded.faces.is_empty() || !decoded.surfaces.is_empty() || !decoded.points.is_empty()
        });
        let any_empty_model = decoded_sites.iter().any(|(site, _, decoded)| {
            decoded.stats.source_entity_records == 0
                && sites[site].iter().any(|index| {
                    contains_ascii_case_insensitive(&streams[*index].header.description, "partition")
                })
                && sites[site].iter().any(|index| {
                    contains_ascii_case_insensitive(&streams[*index].header.description, "deltas")
                })
        });
        if !any_site_has_geometry && !any_empty_model {
            return Ok(None);
        }
    }
    let active_stream = resolved_active_site.map(|site| decoded_sites[site].1);
    let metadata_header = active_stream
        .map(|index| streams[index].header)
        .or_else(|| {
            let first = decoded_sites.first()?.1;
            let first_header = streams[first].header;
            decoded_sites
                .iter()
                .all(|(_, representative, _)| {
                    let header = streams[*representative].header;
                    header.schema == first_header.schema
                        && header.description == first_header.description
                })
                .then_some(first_header)
        })
        .map(|header| {
            let description = copy_retained_string(
                ctx,
                &header.description,
                "retain SLDPRT B-rep header description",
            )?;
            let schema = copy_retained_string(
                ctx,
                header.schema.value(),
                "retain SLDPRT B-rep header schema",
            )?;
            let schema = cadmpeg_parasolid::OwnedSchemaToken::try_from(schema)
                .map_err(|_| CodecError::Malformed("invalid admitted Parasolid schema".into()))?;
            Ok::<_, CodecError>(StreamHeader {
                description,
                schema,
                body_offset: header.body_offset,
            })
        })
        .transpose()?;
    let (selected_site_key, selected, mut decoded) = decoded_sites.swap_remove(selected_site);
    if active_stream.is_none() {
        decoded.qualify_ids(ctx, &selected_site_key)?;
    }
    bind_opaque_geometry(ctx, &mut decoded, &streams[selected].section.native_id())?;
    let mut configuration_bodies = Vec::new();
    if let Some(index) = configuration_index(streams[selected].source_stream().as_str()) {
        ctx.reserve_collection_vec(
            &mut configuration_bodies,
            1,
            "collect SLDPRT configuration bodies",
        )?;
        configuration_bodies.push((
            index,
            copy_body_ids(ctx, &decoded.bodies)?,
        ));
    }
    for (site, first, mut alternate) in decoded_sites {
        alternate.qualify_ids(ctx, &site)?;
        bind_opaque_geometry(ctx, &mut alternate, &streams[first].section.native_id())?;
        if let Some(index) = configuration_index(streams[first].source_stream().as_str()) {
            ctx.reserve_collection_vec(
                &mut configuration_bodies,
                1,
                "collect SLDPRT configuration bodies",
            )?;
            configuration_bodies.push((
                index,
                copy_body_ids(ctx, &alternate.bodies)?,
            ));
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
            metadata_header,
            brep: decoded,
            configuration_bodies,
        },
        report,
    )))
}

fn copy_body_ids(
    ctx: &DecodeContext<'_>,
    bodies: &[cadmpeg_ir::topology::Body],
) -> Result<Vec<cadmpeg_ir::ids::BodyId>, CodecError> {
    let mut ids = Vec::new();
    ctx.reserve_collection_vec(&mut ids, bodies.len(), "collect SLDPRT body IDs")?;
    for body in bodies {
        let value = copy_retained_string(ctx, body.id.as_str(), "retain SLDPRT body ID")?;
        let id = cadmpeg_ir::ids::BodyId::mint(value)
            .map_err(|_| CodecError::Malformed("invalid admitted SLDPRT body ID".into()))?;
        ids.push(id);
    }
    Ok(ids)
}

fn bind_opaque_geometry(
    ctx: &DecodeContext<'_>,
    brep: &mut Brep,
    source: &UnknownId,
) -> Result<(), CodecError> {
    for surface in &mut brep.surfaces {
        if let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record }) =
            &mut surface.geometry
        {
            if record.is_none() {
                let value = copy_retained_string(
                    ctx,
                    source.as_str(),
                    "retain SLDPRT opaque surface source",
                )?;
                *record = Some(UnknownId::mint(value).map_err(|_| {
                    CodecError::Malformed("invalid admitted SLDPRT source ID".into())
                })?);
            }
        }
    }
    for curve in &mut brep.curves {
        if let cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
            record,
        }) = &mut curve.geometry
        {
            if record.is_none() {
                let value = copy_retained_string(
                    ctx,
                    source.as_str(),
                    "retain SLDPRT opaque curve source",
                )?;
                *record = Some(UnknownId::mint(value).map_err(|_| {
                    CodecError::Malformed("invalid admitted SLDPRT source ID".into())
                })?);
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
    ctx.reserve_precharged_vec(target, source.len(), "merge SLDPRT B-rep arena")?;
    target.append(source);
    Ok(())
}

fn merge_brep(ctx: &DecodeContext<'_>, target: &mut Brep, mut source: Brep) -> Result<(), CodecError> {
    // Sequence links are source-local and belong only to the selected SWIFT
    // source. Alternate configuration sequences must not enter its namespace.
    target.annotations.append(source.annotations)?;
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
    append_brep_arena(ctx, &mut target.procedural_surfaces, &mut source.procedural_surfaces)?;
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
    if let Some(existing) = ir.model.appearances.iter().find(|appearance| {
        appearance.name.as_deref() == Some(definition.name.as_str())
            && appearance.base_color == Some(definition.color)
    }) {
        return Ok(existing.id.clone());
    }
    let id = AppearanceId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "appearance", "displaylist"),
        cadmpeg_ir::ids::IdentityKey::from(section_ordinal).colon(definition.record_offset),
    );
    crate::annotations::note(
        annotations,
        id.as_str().to_owned(),
        &definition.source_name,
        definition.record_offset as u64,
        "displaylist_visual_properties",
        Exactness::ByteExact,
    );
    ctx.reserve_collection_vec(&mut ir.model.appearances, 1, "admit SLDPRT display appearance")?;
    ir.model.appearances.push(Appearance {
        id: id.clone(),
        name: Some(copy_retained_string(
            ctx,
            &definition.name,
            "retain SLDPRT display appearance name",
        )?),
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

fn build_geometry_ir(
    ctx: &DecodeContext<'_>,
    scan: &mut ContainerScan<'_>,
    classification: &crate::dialect::LayerClassification,
    decoded: DecodedBrep,
    form_padding: Option<usize>,
    admitted_entities: &mut u64,
) -> Result<
    (
        CadIr,
        Annotations,
        Vec<UnknownRecord>,
        Vec<cadmpeg_ir::report::loss::LossNote>,
    ),
    CodecError,
> {
    let DecodedBrep {
        metadata_header,
        mut brep,
        configuration_bodies,
    } = decoded;
    let appearance_definitions = crate::appearance::definitions(ctx, scan)?;
    let mut display_sections = Vec::new();
    let mut display_summary = crate::tessellation::Summary::default();
    for section in scan.sections() {
        let faces = crate::tessellation::section_display_faces(ctx, section)?;
        let summary = crate::tessellation::summary_for_faces(&faces);
        display_summary.vertices += summary.vertices;
        display_summary.triangles += summary.triangles;
        ctx.reserve_collection_vec(&mut display_sections, 1, "collect SLDPRT display sections")?;
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
    crate::resolved_features::classes::bind_history_classes(&mut histories, &lanes);
    crate::resolved_features::bindings::bind_scalar_operands(&histories, &mut lanes);
    crate::resolved_features::bindings::bind_scalar_operands(
        &histories,
        &mut supplemental_config_lanes,
    );
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
    crate::resolved_features::projections::bind_parameter_scalars(
        &mut ir.model.parameters,
        &ir.model.features,
        &histories,
        parameter_identity_lanes(ctx, &lanes)?,
    )?;
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
    crate::history::configuration::align_configuration_parameter_kinds(&mut ir);
    complete_resolved_configuration_parameter_snapshots(ctx, &mut ir)?;
    stamp_parameter_baseline(&mut ir)?;
    let crate::resolved_features::sketch_projection::ProjectedSketches {
        mut sketches,
        entities: mut sketch_entities,
        constraints: mut sketch_constraints,
    } = crate::resolved_features::sketch_projection::sketches(ctx, scan, &mut annotations)?;
    crate::resolved_features::profiles::bind_sketch_profiles(
        ctx,
        &mut ir.model.features,
        &mut sketches,
        &mut sketch_entities,
        &mut sketch_constraints,
        &ir.model.parameters,
        &histories,
        &lanes,
        &mut annotations,
    )?;
    crate::resolved_features::bindings::bind_unresolved_detached_sketch_objects(
        &ir.model.features,
        &histories,
        &mut supplemental_config_lanes,
    );
    crate::resolved_features::projections::project_compact_edge_selections(
        &mut ir.model.features,
        &[],
        &supplemental_config_lanes,
    )?;
    crate::history::configuration::project_configuration_supplemental_edge_selections(
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
    ctx.reserve_precharged_vec(
        &mut lanes,
        supplemental_config_lanes.len(),
        "merge SLDPRT feature input lanes",
    )?;
    lanes.extend(supplemental_config_lanes);
    let all_lanes = lanes;
    let lanes = &all_lanes[..base_lane_count];
    let sketch_lanes = all_lanes.as_slice();
    let (spatial_sketches, spatial_sketch_entities) =
        crate::resolved_features::markers::spatial_sketches(
            ctx,
            &mut ir.model.features,
            &histories,
            &sketch_lanes,
        )?;
    ir.model.spatial_sketches = spatial_sketches;
    ir.model.spatial_sketch_entities = spatial_sketch_entities;
    crate::resolved_features::profiles::project_marker_backed_sketches(
        ctx,
        &mut ir.model.features,
        &mut sketches,
        &mut sketch_entities,
        &histories,
        &sketch_lanes,
    )?;
    crate::resolved_features::profiles::project_sketch_block_profiles(
        ctx,
        &mut ir.model.features,
        &mut sketches,
        &mut sketch_entities,
        &histories,
        &sketch_lanes,
    )?;
    crate::history::bind::bind_unique_sketch_feature(ctx, &mut ir.model.features, &sketches, &histories)?;
    crate::resolved_features::component_paths::project_dissected_sketches(
        &mut ir.model.features,
        &sketches,
        &histories,
    );
    crate::resolved_features::axes::bind_profile_revolution_axes(
        ctx,
        &mut ir.model.features,
        &histories,
        &lanes,
        &sketches,
        &brep.surfaces,
    )?;
    crate::resolved_features::bindings::bind_pattern_inputs(
        ctx,
        &mut ir.model.features,
        &histories,
        &lanes,
    )?;
    crate::resolved_features::bindings::bind_sweep_adjacent_profiles(
        &mut ir.model.features,
        &histories,
        &lanes,
    );
    crate::resolved_features::dimensions::project_dimensioned_sketch_geometry(
        ctx,
        &mut sketch_entities,
        &sketches,
        &brep.surfaces,
        &ir.model.features,
        &ir.model.parameters,
        &sketch_lanes,
    )?;
    crate::resolved_features::dimensions::project_marker_dimensioned_circles(
        &mut sketch_entities,
        &mut sketches,
        &ir.model.features,
        &ir.model.parameters,
        &sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_relation_point_geometry(
        ctx,
        &mut sketch_entities,
        &sketches,
        &ir.model.features,
        &sketch_lanes,
    )?;
    crate::resolved_features::dimensions::project_relation_point_dimensioned_circles(
        ctx,
        &mut sketch_entities,
        &ir.model.features,
        &ir.model.parameters,
        &sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_relation_solved_line_geometry(
        ctx,
        &mut sketch_entities,
        &sketches,
        &ir.model.features,
        &ir.model.parameters,
        &sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_relation_solved_point_geometry(
        ctx,
        &mut sketch_entities,
        &sketches,
        &ir.model.features,
        &ir.model.parameters,
        &sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_relation_bindings(
        ctx,
        &mut sketch_constraints,
        &sketches,
        &ir.model.features,
        &sketch_entities,
        &ir.model.parameters,
        &sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_spatial_relation_bindings(
        ctx,
        &mut ir.model.spatial_sketch_constraints,
        &mut ir.model.spatial_sketch_entities,
        &ir.model.spatial_sketches,
        &ir.model.features,
        &ir.model.parameters,
        &sketch_lanes,
    )?;
    stamp_feature_baseline(&mut ir)?;
    let mut attributes = crate::metadata::attributes(ctx, scan, &mut annotations)?;
    let custom_properties = crate::history::project::custom_property_attributes(ctx, &histories)?;
    ctx.reserve_precharged_vec(
        &mut attributes,
        custom_properties.len(),
        "append SLDPRT custom properties",
    )?;
    attributes.extend(custom_properties);
    ir.model.attributes = attributes;
    ir.model.sketches = sketches;
    ir.model.sketch_entities = sketch_entities;
    ir.model.sketch_constraints = sketch_constraints;
    stamp_sketch_baseline(&mut ir, &all_lanes)?;

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
        &ir.model.bodies,
        &ir.model.faces,
        &ir.model.edges,
        &ir.model.vertices,
        &face_bridge_sequences,
        &edge_use_sequences,
        &vertex_use_sequences,
    );
    let face_atoms = std::mem::take(&mut brep.face_atoms);
    let mut face_identities = Vec::new();
    ctx.reserve_collection_vec(
        &mut face_identities,
        face_atoms.len(),
        "collect SLDPRT face identities",
    )?;
    for atom in face_atoms {
        face_identities.push((atom.face, atom.identity));
    }
    let mut face_producers = Vec::new();
    ctx.reserve_collection_vec(
        &mut face_producers,
        face_identities.len(),
        "collect SLDPRT face producers",
    )?;
    for (target, identity) in &face_identities {
        face_producers.push((
            copy_retained_string(ctx, target.as_str(), "retain SLDPRT face producer ID")?,
            identity.feature_source_id.value(),
        ));
    }
    ctx.charge_work(
        u64::try_from(brep.body_modifiers.len()).map_err(|_| {
            ctx.refuse_codec_limit("count SLDPRT body modifiers", u64::MAX - 1, u64::MAX)
        })?,
        "count SLDPRT body modifiers",
    )?;
    let modifier_count = brep
        .body_modifiers
        .iter()
        .filter(|modifier| modifier.target.is_some())
        .count();
    let mut body_modifiers = Vec::new();
    ctx.reserve_collection_vec(
        &mut body_modifiers,
        modifier_count,
        "collect SLDPRT body modifiers",
    )?;
    for modifier in std::mem::take(&mut brep.body_modifiers) {
        ctx.charge_work(1, "collect SLDPRT body modifiers")?;
        if let Some(target) = modifier.target {
            body_modifiers.push((target, modifier.history_ordinal));
        }
    }
    crate::history::bind::derive_feature_outputs(
        ctx,
        &mut ir.model.features,
        &histories,
        &face_producers,
        &body_modifiers,
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
        &mut ir.model.features,
        &histories,
        &topology_selection_inputs,
    )?;
    crate::resolved_features::bindings::bind_mirror_surface_planes(
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
        &mut ir.model.features,
        &ir.model.sketches,
        &ir.model.sketch_entities,
        &histories,
        &all_lanes,
    );
    crate::resolved_features::holes::project_spatial_hole_position_sketches(
        &mut ir.model.features,
        &ir.model.spatial_sketches,
        &ir.model.spatial_sketch_entities,
        &ir.model.surfaces,
        &histories,
        &all_lanes,
    );
    crate::resolved_features::holes::project_generated_hole_axes(
        &mut ir.model.features,
        &histories,
        &all_lanes,
        &face_identities,
        &ir.model.faces,
        &ir.model.surfaces,
    );
    crate::resolved_features::holes::project_topological_hole_constructions(
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
        &mut ir.model.features,
        &mut ir.model.sketches,
        &mut ir.model.sketch_entities,
        &ir.model.surfaces,
        &histories,
        &all_lanes,
    );
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
    let configuration_losses =
        crate::history::configuration::project_configuration_sketch_states(
            ctx,
            &mut ir,
            &histories,
            &all_lanes,
            &mut annotations,
        )?;
    ctx.reserve_collection_vec(
        &mut pmi_losses,
        configuration_losses.len(),
        "append SLDPRT configuration PMI losses",
    )?;
    pmi_losses.extend(configuration_losses);
    crate::history::configuration::bind_configuration_topology_selections(
        &mut ir,
        &histories,
        &all_lanes,
        &face_identities,
    )?;
    mark_active_configuration(&mut ir);
    crate::resolved_features::projections::project_unbound_cosmetic_thread_faces(
        &mut ir.model.features,
        &histories,
        &all_lanes,
        &ir.model.faces,
        &ir.model.surfaces,
    );
    crate::resolved_features::projections::project_unbound_offset_plane_faces(
        &mut ir.model.features,
        &ir.model.faces,
        &ir.model.surfaces,
    );
    crate::history::configuration::inherit_configuration_reference_plane_states(&mut ir);
    sync_active_configuration_resolutions(&mut ir)?;
    crate::history::bind::order_model_features_for_regeneration(ctx, &mut ir)?;
    let pattern_hole_nominals = crate::swift::pattern_hole_nominal_context(&ir.model.features);
    ir.model.pmi = crate::swift::annotations(
        scan,
        &mut annotations,
        Some(&topology_index),
        Some(&pattern_hole_nominals),
    );
    stamp_feature_baseline(&mut ir)?;
    let mut native = crate::native::SldprtNative {
        feature_histories: histories,
        feature_input_lanes: all_lanes,
        pmi_dimensions,
    };
    assign_native_configuration_indices(&ir, &mut native);
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("sldprt_native_configuration_sha256"),
            crate::history::hash::native_configuration_hash(&native.feature_histories)?,
        );
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("sldprt_native_history_sha256"),
            crate::history::hash::history_hash(&native.feature_histories)?,
        );
    }
    ctx.admit_entities(
        ir.model.entity_count() as u64,
        admitted_entities,
        "admit SLDPRT entities",
    )?;
    native.store(ctx, ir.native.namespace_mut("sldprt"))?;
    // Stamp baseline before fabricating the read-side configuration snapshot.
    stamp_configuration_baseline(&mut ir)?;
    snapshot_active_configuration(&mut ir);
    let mut unknowns = brep.unknowns;
    for owned_face_color in brep.face_colors {
        let annotation_source = &owned_face_color.source_stream;
        let site = owned_face_color.site_key.as_deref().or_else(|| {
            owned_face_color
                .value
                .target
                .as_deref()
                .and_then(|target| target.split_once('@').map(|(_, site)| site))
        });
        let mut qualified_site = String::new();
        if let Some(site) = site {
            const OPERATION: &str = "retain SLDPRT colour site qualifier";
            let bytes = site.len().checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
            })?;
            ctx.reserve_retained_string(&mut qualified_site, bytes, OPERATION)?;
            qualified_site.push('@');
            qualified_site.push_str(site);
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
            &mut annotations,
            id.as_str().to_owned(),
            annotation_source,
            face_color.offset as u64,
            "00_53_color",
            Exactness::ByteExact,
        );
        if !ir
            .model
            .appearances
            .iter()
            .any(|appearance| appearance.id == id)
        {
            ctx.reserve_collection_vec(&mut ir.model.appearances, 1, "admit SLDPRT face appearance")?;
            ir.model.appearances.push(Appearance {
                id: id.clone(),
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
            if !ir
                .model
                .appearance_bindings
                .iter()
                .any(|binding| binding.id == binding_id)
            {
                ctx.reserve_collection_vec(
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
    for (index, definition) in appearance_definitions.into_iter().enumerate() {
        let id = AppearanceId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "appearance", "material"),
            index,
        );
        crate::annotations::note(
            &mut annotations,
            id.as_str().to_owned(),
            &definition.source_name,
            definition.record_offset as u64,
            "moVisualProperties_c",
            Exactness::ByteExact,
        );
        ctx.reserve_collection_vec(&mut ir.model.appearances, 1, "admit SLDPRT material appearance")?;
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
    let feature_appearance_sources = charged_btree_set(
        ctx,
        crate::appearance::feature_assignments(ctx, scan)?
            .into_iter()
            .map(|assignment| assignment.feature_source_id),
        "index SLDPRT feature appearance sources",
    )?;
    let mut matched_feature_sources = BTreeSet::new();
    let mut conflicting_display_references = Vec::new();
    let mut persistent_face_bindings = Vec::new();
    for (display, display_faces) in display_sections {
        if display_faces.is_empty() {
            continue;
        }
        let display_stream = display.source_stream();
        for (table_index, face) in display_faces.iter().enumerate() {
            let candidates = charged_btree_set(
                ctx,
                face.surface_references.iter()
                    .map(crate::tessellation::PersistentSurfaceReference::feature_source_id),
                "index SLDPRT display surface sources",
            )?;
            if candidates.len() > 1 {
                let message = conflicting_display_reference(
                    ctx,
                    display_stream.as_str(),
                    table_index,
                    &candidates,
                )?;
                ctx.reserve_collection_vec(
                    &mut conflicting_display_references,
                    1,
                    "collect SLDPRT conflicting display references",
                )?;
                conflicting_display_references.push(message);
            }
        }
        let resolved =
            crate::appearance::resolve_display_appearances(ctx, scan, display, &display_faces)?;
        for source in resolved.matched_feature_sources {
            ctx.charge_work(1, "index SLDPRT matched appearance sources")?;
            if !matched_feature_sources.contains(&source) {
                ctx.charge_collection_items(1, "index SLDPRT matched appearance sources")?;
                matched_feature_sources.insert(source);
            }
        }
        let mut display_links = Vec::new();
        ctx.reserve_collection_vec(
            &mut display_links,
            display_faces.len(),
            "collect SLDPRT display links",
        )?;
        for (table_index, display_face) in display_faces.into_iter().enumerate() {
            let id = format!(
                "sldprt:displaylist:record#{}:{}",
                display.ordinal(),
                table_index
            );
            if let Some(identity) = display_face.persistent_surface_identity() {
                let mut trailing_fields = Vec::new();
                ctx.reserve_collection_vec(
                    &mut trailing_fields,
                    identity.trailing_fields.len(),
                    "copy SLDPRT persistent face identity fields",
                )?;
                trailing_fields.extend_from_slice(&identity.trailing_fields);
                ctx.reserve_collection_vec(
                    &mut persistent_face_bindings,
                    1,
                    "collect SLDPRT persistent face bindings",
                )?;
                persistent_face_bindings.push(crate::tessellation::PersistentFaceBinding {
                    tessellation: id.clone(),
                    identity: crate::brep::PersistentFaceIdentity {
                        feature_source_id: identity.feature_source_id,
                        local_id: identity.local_id,
                        trailing_fields,
                    },
                });
            }
            crate::annotations::note(
                &mut annotations,
                id.clone(),
                display_stream,
                display_face.table.start() as u64,
                "displaylist_tessellation",
                Exactness::ByteExact,
            );
            display_links.push(id.clone());
            if let Some(definition) = resolved.by_face.get(&table_index) {
                let table_index_text = table_index.to_string();
                let source_stream = display_stream.as_str();
                let source_id_len = source_stream
                    .len()
                    .checked_add("::DisplayFace[".len())
                    .and_then(|len| len.checked_add(table_index_text.len()))
                    .and_then(|len| len.checked_add(1))
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit(
                            "retain SLDPRT DisplayFace source identity",
                            u64::MAX - 1,
                            u64::MAX,
                        )
                    })?;
                let mut source_entity_id = String::new();
                ctx.reserve_retained_string(
                    &mut source_entity_id,
                    source_id_len,
                    "retain SLDPRT DisplayFace source identity",
                )?;
                source_entity_id.push_str(source_stream);
                source_entity_id.push_str("::DisplayFace[");
                source_entity_id.push_str(&table_index_text);
                source_entity_id.push(']');
                let appearance = ensure_display_appearance(
                    ctx,
                    &mut ir,
                    definition,
                    display.ordinal(),
                    &mut annotations,
                )?;
                ctx.reserve_collection_vec(
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
                    target: AppearanceTarget::Tessellation(id.clone()),
                    appearance,
                    source_entity_id: Some(source_entity_id),
                    object_type: Some("DisplayFace".into()),
                    visible: None,
                    channels: BTreeMap::new(),
                });
            }
            let mesh = display_face.mesh;
            ctx.reserve_collection_vec(
                &mut ir.model.tessellations,
                1,
                "admit SLDPRT display tessellation",
            )?;
            ir.model.tessellations.push(mesh.into_tessellation(id).map_err(|error| {
                    CodecError::malformed(format_args!("invalid display tessellation: {error}"))
                })?);
        }
        let display_id = UnknownId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "displaylist", "record"),
            display.ordinal(),
        );
        crate::annotations::note(
            &mut annotations,
            display_id.as_str().to_owned(),
            display_stream,
            0,
            "displaylist_tessellation",
            Exactness::Unknown,
        );
        ctx.reserve_collection_vec(&mut unknowns, 1, "retain SLDPRT display unknown")?;
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
        ctx.reserve_collection_vec(&mut pmi_losses, 1, "append SLDPRT appearance loss")?;
        pmi_losses.push(SldprtLossCode::AppearanceAssignmentUnresolved.note(message));
    }
    let mut assigned_tessellations = crate::tessellation::assign_persistent_owners(
        ctx,
        &mut ir.model,
        &face_identities,
        &persistent_face_bindings,
    )?;
    let remaining_assignments = crate::tessellation::assign_unique_surface_owners(
        ctx,
        &mut ir.model,
    )?;
    ctx.reserve_precharged_vec(
        &mut assigned_tessellations,
        remaining_assignments.len(),
        "merge SLDPRT assigned tessellations",
    )?;
    assigned_tessellations.extend(remaining_assignments);
    let mut annotation_builder = AnnotationBuilder::resume(annotations);
    for id in assigned_tessellations {
        annotation_builder
            .derived(&id, "body")
            .map_err(cadmpeg_core::CodecError::malformed)?
            .derived(id, "faces")
            .map_err(cadmpeg_core::CodecError::malformed)?;
    }
    let mut annotations = annotation_builder.build();
    for source_block in &mut scan.blocks {
        let id = UnknownId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "file", "block"),
            source_block.offset,
        );
        if unknowns.iter().any(|record| record.id() == &id) {
            continue;
        }
        crate::annotations::note(
            &mut annotations,
            id.as_str().to_owned(),
            source_block.section.source_stream(),
            source_block.offset as u64,
            source_block.family.label(),
            Exactness::ByteExact,
        );
        unknowns.push(UnknownRecord::retained(
            id,
            0,
            std::mem::take(&mut source_block.payload),
            Vec::new(),
        ));
    }
    for source_stream in &mut scan.compound_streams {
        let id = UnknownId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "file", "compound-stream"),
            source_stream.directory_id,
        );
        crate::annotations::note(
            &mut annotations,
            id.as_str().to_owned(),
            &source_stream.path,
            0,
            container::payload_family(&source_stream.payload).label(),
            Exactness::ByteExact,
        );
        unknowns.push(UnknownRecord::retained(
            id,
            0,
            std::mem::take(&mut source_stream.payload),
            Vec::new(),
        ));
    }
    fn add_opaque_link<'a>(
        ctx: &DecodeContext<'_>,
        opaque_links: &mut BTreeMap<&'a str, Vec<String>>,
        record: &'a str,
        entity: &str,
    ) -> Result<(), CodecError> {
        ctx.charge_work(1, "index SLDPRT opaque geometry link")?;
        let links = match opaque_links.entry(record) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "index SLDPRT opaque geometry record")?;
                entry.insert(Vec::new())
            }
        };
        ctx.reserve_collection_vec(links, 1, "index SLDPRT opaque geometry link")?;
        links.push(copy_retained_string(
            ctx,
            entity,
            "retain SLDPRT opaque geometry link",
        )?);
        Ok(())
    }
    let mut opaque_links = BTreeMap::<&str, Vec<String>>::new();
    for surface in &ir.model.surfaces {
        if let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
            record: Some(record),
        }) = &surface.geometry
        {
            add_opaque_link(ctx, &mut opaque_links, record.as_str(), surface.id.as_str())?;
        }
    }
    for curve in &ir.model.curves {
        if let cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
            record: Some(record),
        }) = &curve.geometry
        {
            add_opaque_link(ctx, &mut opaque_links, record.as_str(), curve.id.as_str())?;
        }
    }
    for (record_id, links) in opaque_links {
        let Some(source) = unknowns
            .iter_mut()
            .find(|record| record.id().as_str() == record_id)
        else {
            return Err(CodecError::malformed(format_args!(
                "opaque geometry record {record_id} was not retained"
            )));
        };
        ctx.reserve_precharged_vec(
            source.links_mut(),
            links.len(),
            "append SLDPRT opaque geometry links",
        )?;
        source.links_mut().extend(links);
    }
    preserve_source_image(ctx, scan, &mut annotations, &mut unknowns)?;
    // Sort arenas for the order-sensitive loss scans that follow; the local
    // digests are stamped once, in `decode_result`, after native unknown
    // records are attached.
    ir.finalize();
    Ok((ir, annotations, unknowns, pmi_losses))
}

fn assign_native_configuration_indices(ir: &CadIr, native: &mut crate::native::SldprtNative) {
    for configuration in &ir.model.configurations {
        let Some(native_ref) = configuration.native_ref.as_deref() else {
            continue;
        };
        if let Some(record) = native
            .feature_histories
            .iter_mut()
            .flat_map(|history| &mut history.configurations)
            .find(|record| record.id == native_ref)
        {
            record.source_index = configuration.source_index;
        }
    }
}

fn source_meta(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    classification: &crate::dialect::LayerClassification,
    header: Option<&StreamHeader>,
    display: crate::tessellation::Summary,
) -> Result<SourceMeta, CodecError> {
    let mut attributes = BTreeMap::new();
    attributes.insert(
        cadmpeg_core::nonblank_literal!("outer_version"),
        format!("0x{:08x}", scan.version),
    );
    if display.vertices > 0 {
        attributes.insert(
            cadmpeg_core::nonblank_literal!("displaylist_vertices"),
            display.vertices.to_string(),
        );
        attributes.insert(
            cadmpeg_core::nonblank_literal!("displaylist_triangles"),
            display.triangles.to_string(),
        );
    }
    attributes.insert(
        cadmpeg_core::nonblank_literal!("block_count"),
        scan.blocks.len().to_string(),
    );
    attributes.insert(
        cadmpeg_core::nonblank_literal!("compound_stream_count"),
        scan.compound_streams.len().to_string(),
    );
    if let Some(site) = container::select_active_parasolid_site(scan) {
        attributes.insert(
            cadmpeg_core::nonblank_literal!("active_parasolid_block"),
            copy_retained_string(
                ctx,
                site.source_stream().as_str(),
                "retain SLDPRT active site name",
            )?,
        );
    } else {
        attributes.insert(
            cadmpeg_core::nonblank_literal!("sldprt_active_partition_unresolved"),
            "true".into(),
        );
    }
    if let Some(header) = header {
        attributes.insert(
            cadmpeg_core::nonblank_literal!("parasolid_schema"),
            copy_retained_string(ctx, header.schema.value(), "retain SLDPRT source schema")?,
        );
        attributes.insert(
            cadmpeg_core::nonblank_literal!("parasolid_description"),
            copy_retained_string(ctx, &header.description, "retain SLDPRT source description")?,
        );
    }
    add_preview_metadata(ctx, scan, &mut attributes)?;
    add_solidworks_xml_metadata(ctx, scan, &mut attributes)?;
    Ok(SourceMeta::classified(
        classification.layers().clone_charged(ctx)?,
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
    for section in scan.sections() {
        ctx.charge_work(1, "scan SLDPRT preview metadata")?;
        let payload = section.payload();
        match container::payload_family(payload) {
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
                    cadmpeg_core::nonblank_literal!("png_preview_{png_index}_{field}")
                };
                ctx.charge_collection_items(7, "collect SLDPRT PNG preview metadata")?;
                attributes.insert(key("width"), width.to_string());
                attributes.insert(key("height"), height.to_string());
                attributes.insert(key("bit_depth"), fields[0].to_string());
                attributes.insert(key("color_type"), fields[1].to_string());
                attributes.insert(key("compression"), fields[2].to_string());
                attributes.insert(key("filter"), fields[3].to_string());
                attributes.insert(key("interlace"), fields[4].to_string());
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
                    cadmpeg_core::nonblank_literal!("bmp_thumbnail_{bmp_index}_{field}")
                };
                ctx.charge_collection_items(6, "collect SLDPRT BMP preview metadata")?;
                attributes.insert(key("width"), width.to_string());
                attributes.insert(key("height"), height.to_string());
                attributes.insert(key("planes"), planes.to_string());
                attributes.insert(key("bit_count"), bits_per_pixel.to_string());
                attributes.insert(key("compression"), compression.to_string());
                attributes.insert(key("image_size"), image_size.to_string());
                bmp_index += 1;
            }
            _ => {}
        }
    }
    attributes.insert(
        cadmpeg_core::nonblank_literal!("png_preview_count"),
        png_index.to_string(),
    );
    attributes.insert(
        cadmpeg_core::nonblank_literal!("bmp_thumbnail_count"),
        bmp_index.to_string(),
    );
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
                attributes.insert(key, copy_retained_string(ctx, value, "retain SLDPRT XML metadata")?);
            }
        }
        if let Some(value) = active_configuration_name {
            attributes.insert(
                cadmpeg_core::nonblank_literal!("sw_configuration_name"),
                copy_retained_string(ctx, value, "retain SLDPRT configuration name")?,
            );
        } else if let Some(value) = &envelope.configuration_name {
            attributes.insert(
                cadmpeg_core::nonblank_literal!("sw_configuration_name"),
                copy_retained_string(ctx, value, "retain SLDPRT configuration name")?,
            );
        }
        for (key, value) in &envelope.configuration_attributes {
            let name = copy_retained_string(ctx, key, "retain SLDPRT configuration key")?;
            let name = cadmpeg_core::text::NonBlankString::new(name)
                .ok_or_else(|| CodecError::Malformed("invalid SLDPRT configuration key".into()))?;
            let value = copy_retained_string(ctx, value, "retain SLDPRT configuration value")?;
            ctx.charge_collection_items(1, "copy SLDPRT configuration attribute")?;
            attributes.insert(name, value);
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
        let mut message = Vec::new();
        if s.unknown_surface_faces > 0 {
            message.push(format!(
                "{} face(s) rest on a support surface whose stored carrier this codec does not \
                 type; the face, its loops, and trims are emitted with an unknown-geometry \
                 surface linking to the preserved record bytes. Topology is transferred; the \
                 underlying surface shape is not.",
                s.unknown_surface_faces
            ));
        }
        if s.unknown_procedural_supports > 0 {
            message.push(format!(
                "{} untyped surface carrier(s) are retained as opaque hidden supports of exact \
                 procedural constructions.",
                s.unknown_procedural_supports
            ));
        }
        ctx.reserve_collection_vec(&mut losses, 1, "append SLDPRT geometry loss")?;
        losses.push(SldprtLossCode::GeometryFaceSupportSurfaceUntyped.note(message.join(" ")));
    }
    ctx.reserve_precharged_vec(
        &mut losses,
        decoded.losses.len(),
        "move SLDPRT B-rep losses to report",
    )?;
    losses.append(&mut decoded.losses);
    if s.unknown_curve_edges > 0 {
        ctx.reserve_collection_vec(&mut losses, 1, "append SLDPRT geometry loss")?;
        losses.push(
            SldprtLossCode::GeometryEdgeSupportCurveUntyped.note(format!(
                "{} edge(s) reference an untyped support curve; topology references an opaque \
                 curve carrier linked to the retained partition.",
                s.unknown_curve_edges
            )),
        );
    }
    if s.ambiguous_pcurve_parameters > 0 {
        ctx.reserve_collection_vec(&mut losses, 1, "append SLDPRT geometry loss")?;
        losses.push(SldprtLossCode::GeometryPcurveAmbiguous.note(format!(
            "{} pcurve(s) were withheld because more than one geometric parameter satisfies the stored edge or ruling geometry; the decoder does not choose by residual order.",
            s.ambiguous_pcurve_parameters
        )));
    }
    if s.off_surface_nurbs_pcurves > 0 {
        ctx.reserve_collection_vec(&mut losses, 1, "append SLDPRT geometry loss")?;
        losses.push(SldprtLossCode::TopologyPcurveCarrierOffSurface.note(format!(
            "{} NURBS edge carrier(s) have vertex ranges off their bound B-spline surface; pcurve derivation is withheld because the defect is upstream of parameter-space geometry.",
            s.off_surface_nurbs_pcurves
        )));
    }
    if s.unresolved_face_colors > 0 {
        ctx.reserve_collection_vec(&mut losses, 1, "append SLDPRT geometry loss")?;
        losses.push(SldprtLossCode::AppearanceFaceColorUnresolved.note(format!(
            "{} face-color binding(s) were withheld because the current face and link records do not select one consistent framed color record.",
            s.unresolved_face_colors
        )));
    }
    if s.ambiguous_face_owners > 0 {
        ctx.reserve_collection_vec(&mut losses, 1, "append SLDPRT geometry loss")?;
        losses.push(SldprtLossCode::TopologyFaceOwnerAmbiguous.note(format!(
            "{} face owner(s) have non-equivalent bridge uses; all uses for each owner remain unresolved.",
            s.ambiguous_face_owners
        )));
    }
    if s.unclaimed_faces > 0 {
        ctx.reserve_collection_vec(&mut losses, 1, "append SLDPRT geometry loss")?;
        losses.push(SldprtLossCode::TopologyFaceUnclaimed.note(format!(
            "{} canonical face(s) are not claimed by an explicit body relation; the decoder withholds them rather than inventing body membership.",
            s.unclaimed_faces
        )));
    }
    if s.synthetic_body_grouping {
        ctx.reserve_collection_vec(&mut losses, 1, "append SLDPRT geometry loss")?;
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
) -> Result<
    (
        CadIr,
        Annotations,
        Vec<UnknownRecord>,
        Vec<cadmpeg_ir::report::loss::LossNote>,
    ),
    CodecError,
> {
    let mut ir = CadIr::empty();
    let mut unknowns = Vec::new();
    let mut annotations = Annotations::default();
    let mut pmi_losses = Vec::new();
    let mut histories = crate::history::histories(ctx, scan, &mut annotations, &mut pmi_losses)?;
    let mut lanes = crate::resolved_features::assembly::lanes(ctx, scan, &mut annotations)?;
    let mut supplemental_config_lanes =
        crate::resolved_features::assembly::supplemental_config_lanes(ctx, scan, &mut annotations)?;
    crate::resolved_features::classes::bind_history_classes(&mut histories, &lanes);
    crate::resolved_features::bindings::bind_scalar_operands(&histories, &mut lanes);
    crate::resolved_features::bindings::bind_scalar_operands(
        &histories,
        &mut supplemental_config_lanes,
    );
    let pmi_dimensions = crate::pmi::dimensions(ctx, scan, &mut annotations, &mut pmi_losses)?;
    ir.model.pmi = crate::swift::annotations(scan, &mut annotations, None, None);
    let crate::resolved_features::sketch_projection::ProjectedSketches {
        sketches,
        entities: sketch_entities,
        constraints: sketch_constraints,
    } = crate::resolved_features::sketch_projection::sketches(ctx, scan, &mut annotations)?;
    let mut model_attributes = crate::metadata::attributes(ctx, scan, &mut annotations)?;
    let custom_properties = crate::history::project::custom_property_attributes(ctx, &histories)?;
    ctx.reserve_precharged_vec(
        &mut model_attributes,
        custom_properties.len(),
        "append SLDPRT custom properties",
    )?;
    model_attributes.extend(custom_properties);
    ir.model.attributes = model_attributes;
    ir.model.sketches = sketches;
    ir.model.sketch_entities = sketch_entities;
    ir.model.sketch_constraints = sketch_constraints;
    let mut attributes = BTreeMap::new();
    attributes.insert(
        cadmpeg_core::nonblank_literal!("outer_version"),
        format!("0x{:08x}", scan.version),
    );
    attributes.insert(
        cadmpeg_core::nonblank_literal!("block_count"),
        scan.blocks.len().to_string(),
    );
    add_solidworks_xml_metadata(ctx, scan, &mut attributes)?;

    if let Some(site) = container::select_active_parasolid_site(scan) {
        let id = site.section.native_id();
        let offset = match site.section {
            container::Section::Block(block) => block.offset as u64,
            container::Section::Compound(_) => 0,
        };
        attributes.insert(
            cadmpeg_core::nonblank_literal!("active_parasolid_block"),
            copy_retained_string(
                ctx,
                site.source_stream().as_str(),
                "retain SLDPRT metadata active site name",
            )?,
        );
        attributes.insert(
            cadmpeg_core::nonblank_literal!("parasolid_schema"),
            copy_retained_string(
                ctx,
                site.header.schema.value(),
                "retain SLDPRT metadata schema",
            )?,
        );
        crate::annotations::note(
            &mut annotations,
            id.as_str().to_owned(),
            site.source_stream(),
            0,
            "parasolid_stream",
            Exactness::Unknown,
        );
        ctx.reserve_collection_vec(&mut unknowns, 1, "retain SLDPRT metadata site")?;
        unknowns.push(UnknownRecord::retained(
            id,
            offset,
            ctx.copy_retained(site.payload, "retain SLDPRT active site")?,
            Vec::new(),
        ));
    }

    ir.source = Some(SourceMeta::classified(
        classification.layers().clone_charged(ctx)?,
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
    crate::resolved_features::projections::bind_parameter_scalars(
        &mut ir.model.parameters,
        &ir.model.features,
        &histories,
        parameter_identity_lanes(ctx, &lanes)?,
    )?;
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
    crate::history::configuration::align_configuration_parameter_kinds(&mut ir);
    complete_resolved_configuration_parameter_snapshots(ctx, &mut ir)?;
    stamp_parameter_baseline(&mut ir)?;
    crate::resolved_features::profiles::bind_sketch_profiles(
        ctx,
        &mut ir.model.features,
        &mut ir.model.sketches,
        &mut ir.model.sketch_entities,
        &mut ir.model.sketch_constraints,
        &ir.model.parameters,
        &histories,
        &lanes,
        &mut annotations,
    )?;
    crate::resolved_features::bindings::bind_unresolved_detached_sketch_objects(
        &ir.model.features,
        &histories,
        &mut supplemental_config_lanes,
    );
    crate::resolved_features::projections::project_compact_edge_selections(
        &mut ir.model.features,
        &[],
        &supplemental_config_lanes,
    )?;
    crate::history::configuration::project_configuration_supplemental_edge_selections(
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
    ctx.reserve_precharged_vec(
        &mut lanes,
        supplemental_config_lanes.len(),
        "merge SLDPRT feature input lanes",
    )?;
    lanes.extend(supplemental_config_lanes);
    let all_lanes = lanes;
    let lanes = &all_lanes[..base_lane_count];
    let sketch_lanes = all_lanes.as_slice();
    let (spatial_sketches, spatial_sketch_entities) =
        crate::resolved_features::markers::spatial_sketches(
            ctx,
            &mut ir.model.features,
            &histories,
            &sketch_lanes,
        )?;
    ir.model.spatial_sketches = spatial_sketches;
    ir.model.spatial_sketch_entities = spatial_sketch_entities;
    crate::resolved_features::profiles::project_marker_backed_sketches(
        ctx,
        &mut ir.model.features,
        &mut ir.model.sketches,
        &mut ir.model.sketch_entities,
        &histories,
        &sketch_lanes,
    )?;
    crate::resolved_features::profiles::project_sketch_block_profiles(
        ctx,
        &mut ir.model.features,
        &mut ir.model.sketches,
        &mut ir.model.sketch_entities,
        &histories,
        &sketch_lanes,
    )?;
    crate::history::bind::bind_unique_sketch_feature(
        ctx,
        &mut ir.model.features,
        &ir.model.sketches,
        &histories,
    )?;
    crate::resolved_features::component_paths::project_dissected_sketches(
        &mut ir.model.features,
        &ir.model.sketches,
        &histories,
    );
    crate::resolved_features::axes::bind_profile_revolution_axes(
        ctx,
        &mut ir.model.features,
        &histories,
        &lanes,
        &ir.model.sketches,
        &ir.model.surfaces,
    )?;
    crate::resolved_features::bindings::bind_pattern_inputs(
        ctx,
        &mut ir.model.features,
        &histories,
        &lanes,
    )?;
    crate::resolved_features::bindings::bind_sweep_adjacent_profiles(
        &mut ir.model.features,
        &histories,
        &lanes,
    );
    crate::resolved_features::dimensions::project_dimensioned_sketch_geometry(
        ctx,
        &mut ir.model.sketch_entities,
        &ir.model.sketches,
        &ir.model.surfaces,
        &ir.model.features,
        &ir.model.parameters,
        &sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_spatial_relation_bindings(
        ctx,
        &mut ir.model.spatial_sketch_constraints,
        &mut ir.model.spatial_sketch_entities,
        &ir.model.spatial_sketches,
        &ir.model.features,
        &ir.model.parameters,
        &sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_relation_point_geometry(
        ctx,
        &mut ir.model.sketch_entities,
        &ir.model.sketches,
        &ir.model.features,
        &sketch_lanes,
    )?;
    crate::resolved_features::dimensions::project_relation_point_dimensioned_circles(
        ctx,
        &mut ir.model.sketch_entities,
        &ir.model.features,
        &ir.model.parameters,
        &sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_relation_solved_line_geometry(
        ctx,
        &mut ir.model.sketch_entities,
        &ir.model.sketches,
        &ir.model.features,
        &ir.model.parameters,
        &sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_relation_solved_point_geometry(
        ctx,
        &mut ir.model.sketch_entities,
        &ir.model.sketches,
        &ir.model.features,
        &ir.model.parameters,
        &sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_relation_bindings(
        ctx,
        &mut ir.model.sketch_constraints,
        &ir.model.sketches,
        &ir.model.features,
        &ir.model.sketch_entities,
        &ir.model.parameters,
        &sketch_lanes,
    )?;
    crate::resolved_features::holes::project_profiled_hole_constructions(
        ctx,
        &mut ir.model.features,
        &ir.model.sketch_entities,
        &histories,
        &lanes,
    )?;
    crate::resolved_features::holes::project_hole_position_sketches(
        &mut ir.model.features,
        &ir.model.sketches,
        &ir.model.sketch_entities,
        &histories,
        &lanes,
    );
    crate::resolved_features::holes::project_spatial_hole_position_sketches(
        &mut ir.model.features,
        &ir.model.spatial_sketches,
        &ir.model.spatial_sketch_entities,
        &ir.model.surfaces,
        &histories,
        &lanes,
    );
    crate::resolved_features::holes::project_topological_hole_constructions(
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
        &lanes,
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
        &mut ir.model.features,
        &mut ir.model.sketches,
        &mut ir.model.sketch_entities,
        &ir.model.surfaces,
        &histories,
        &lanes,
    );
    crate::resolved_features::relation_geometry::project_relation_bindings(
        ctx,
        &mut ir.model.sketch_constraints,
        &ir.model.sketches,
        &ir.model.features,
        &ir.model.sketch_entities,
        &ir.model.parameters,
        &sketch_lanes,
    )?;
    crate::resolved_features::projections::project_unbound_cosmetic_thread_faces(
        &mut ir.model.features,
        &histories,
        &lanes,
        &ir.model.faces,
        &ir.model.surfaces,
    );
    crate::resolved_features::projections::project_unbound_offset_plane_faces(
        &mut ir.model.features,
        &ir.model.faces,
        &ir.model.surfaces,
    );
    sync_active_configuration_resolutions(&mut ir)?;
    crate::history::bind::order_features_for_regeneration(ctx, &mut ir.model.features)?;
    let configuration_losses =
        crate::history::configuration::project_configuration_sketch_states(
            ctx,
            &mut ir,
            &histories,
            &lanes,
            &mut annotations,
        )?;
    ctx.reserve_collection_vec(
        &mut pmi_losses,
        configuration_losses.len(),
        "append SLDPRT configuration PMI losses",
    )?;
    pmi_losses.extend(configuration_losses);
    crate::history::configuration::inherit_configuration_reference_plane_states(&mut ir);
    crate::history::bind::order_model_features_for_regeneration(ctx, &mut ir)?;
    stamp_feature_baseline(&mut ir)?;
    let native = crate::native::SldprtNative {
        feature_histories: histories,
        feature_input_lanes: all_lanes,
        pmi_dimensions,
    };
    ctx.admit_entities(
        ir.model.entity_count() as u64,
        admitted_entities,
        "admit SLDPRT entities",
    )?;
    native.store(ctx, ir.native.namespace_mut("sldprt"))?;
    stamp_sketch_baseline(&mut ir, &native.feature_input_lanes)?;
    bind_active_configuration_partition(&mut ir);
    mark_active_configuration(&mut ir);
    stamp_configuration_baseline(&mut ir)?;
    snapshot_active_configuration(&mut ir);
    preserve_source_image(ctx, scan, &mut annotations, &mut unknowns)?;
    // Sort arenas for the order-sensitive loss scans that follow; the local
    // digests are stamped once, in `decode_result`, after native unknown
    // records are attached.
    ir.finalize();
    Ok((ir, annotations, unknowns, pmi_losses))
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
    let mut semantic_projection = histories.to_vec();
    let scene_feature_classes = crate::tessellation::scene_feature_classes(ctx, scan)?;
    crate::history::enrich_scene_classes(
        ctx,
        &mut semantic_projection,
        &scene_feature_classes,
    )?;
    crate::history::configuration::enrich_history_semantic(
        ctx,
        &mut semantic_projection,
        lanes,
        pmi_dimensions,
        crate::history::configuration::HistoryEnrichment::Read,
    )?;
    ir.model.semantic_annotations =
        crate::history::project::project_semantic_notes(ctx, &semantic_projection)?;
    crate::history::project::project_feature_model(&semantic_projection)?
        .install(&mut ir.model, losses);
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
    let mut parameter_projection = histories.to_vec();
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
            &mut parameter_projection,
            lanes,
        );
    crate::pmi::enrich_history_parameters(ctx, &mut parameter_projection, pmi_dimensions)?;
    ir.model.parameters = crate::history::parameters::project_parameters(&parameter_projection);
    crate::history::configuration::project_configuration_design_states(
        ctx,
        ir,
        histories,
        lanes,
        pmi_dimensions,
        form_padding,
    )?;
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("sldprt_neutral_feature_local_sha256"),
            crate::history::hash::feature_hash(&ir.model)?,
        );
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("sldprt_native_history_sha256"),
            crate::history::hash::history_hash(histories)?,
        );
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("sldprt_native_configuration_sha256"),
            crate::history::hash::native_configuration_hash(histories)?,
        );
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("sldprt_neutral_parameter_local_sha256"),
            crate::history::hash::parameter_hash(&ir.model.parameters)?,
        );
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("sldprt_native_parameter_sha256"),
            crate::history::hash::native_parameter_hash(histories)?,
        );
    }

    Ok(())
}

fn parameter_identity_lanes<'a>(
    ctx: &DecodeContext<'_>,
    lanes: &'a [crate::records::FeatureInputLane],
) -> Result<Vec<&'a crate::records::FeatureInputLane>, CodecError> {
    let eligible = || lanes.iter().filter(|lane| {
        !crate::resolved_features::assembly::is_supplemental_config_lane(lane)
    });
    let has_global = eligible().any(|lane| lane.configuration.is_none());
    let single_scoped = !has_global && eligible().count() == 1;
    let mut selected = Vec::new();
    for lane in eligible() {
        if (has_global && lane.configuration.is_none())
            || (single_scoped && lane.configuration.is_some())
        {
            ctx.reserve_collection_vec(&mut selected, 1, "select SLDPRT parameter identity lanes")?;
            selected.push(lane);
        }
    }
    Ok(selected)
}

fn stamp_parameter_baseline(ir: &mut CadIr) -> Result<(), CodecError> {
    let hash = crate::history::hash::parameter_hash(&ir.model.parameters)?;
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("sldprt_neutral_parameter_local_sha256"),
            hash,
        );
    }
    Ok(())
}

fn complete_resolved_configuration_parameter_snapshots(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<(), CodecError> {
    for configuration in &mut ir.model.configurations {
        if configuration.parameter_values.is_empty() && configuration.feature_states.is_empty() {
            continue;
        }
        for parameter in &ir.model.parameters {
            ctx.charge_work(1, "complete SLDPRT configuration parameter snapshot")?;
            let Some(value) = parameter.value.as_ref() else {
                continue;
            };
            if !parameter.dependencies.is_empty()
                || configuration.parameter_values.contains_key(&parameter.id)
            {
                continue;
            }
            let id = cadmpeg_ir::features::ParameterId::mint(copy_retained_string(
                ctx,
                parameter.id.as_str(),
                "retain SLDPRT snapshot parameter ID",
            )?)
            .map_err(CodecError::malformed)?;
            let value = match value {
                cadmpeg_ir::features::ParameterValue::String(value) => {
                    cadmpeg_ir::features::ParameterValue::String(copy_retained_string(
                        ctx,
                        value,
                        "retain SLDPRT snapshot parameter value",
                    )?)
                }
                value => value.clone(),
            };
            ctx.charge_collection_items(1, "complete SLDPRT configuration parameter snapshot")?;
            configuration.parameter_values.insert(id, value);
        }
    }
    Ok(())
}

fn mark_active_configuration(ir: &mut CadIr) {
    let active_name = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sw_configuration_name"))
        .map(String::as_str);
    let active_index = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("active_parasolid_block"))
        .and_then(|section| crate::container::configuration_index(section));
    let by_name = active_name.and_then(|name| {
        let mut matches = ir
            .model
            .configurations
            .iter()
            .enumerate()
            .filter(|(_, configuration)| configuration.name.as_deref() == Some(name))
            .map(|(position, _)| position);
        matches.next().filter(|_| matches.next().is_none())
    });
    let by_index = active_index.and_then(|index| {
        let index = u32::try_from(index).ok()?;
        let mut matches = ir
            .model
            .configurations
            .iter()
            .enumerate()
            .filter(|(_, configuration)| configuration.source_index == Some(index))
            .map(|(position, _)| position);
        matches.next().filter(|_| matches.next().is_none())
    });
    let selected = if active_name.is_some() {
        by_name
    } else if active_index.is_some() {
        by_index
    } else if ir.model.configurations.len() == 1 {
        Some(0)
    } else {
        None
    };
    for (position, configuration) in ir.model.configurations.iter_mut().enumerate() {
        configuration.active = selected == Some(position);
    }
}

fn snapshot_active_configuration(ir: &mut CadIr) {
    let mut active = ir
        .model
        .configurations
        .iter()
        .enumerate()
        .filter(|(_, configuration)| configuration.active)
        .map(|(index, _)| index);
    let Some(configuration_index) = active.next() else {
        return;
    };
    if active.next().is_some() {
        return;
    }
    if !ir.model.configurations[configuration_index]
        .parameter_values
        .is_empty()
        || !ir.model.configurations[configuration_index]
            .feature_states
            .is_empty()
    {
        return;
    }

    let parameter_values = ir
        .model
        .parameters
        .iter()
        .filter_map(|parameter| {
            parameter
                .value
                .clone()
                .map(|value| (parameter.id.clone(), value))
        })
        .collect();
    let feature_states = ir
        .model
        .features
        .iter()
        .map(|feature| {
            (
                feature.id.clone(),
                cadmpeg_ir::features::ConfigurationFeatureState {
                    evaluation: if feature.suppressed.unwrap_or(false) {
                        cadmpeg_ir::features::ConfigurationEvaluation::Suppressed {}
                    } else {
                        cadmpeg_ir::features::ConfigurationEvaluation::Active {
                            outputs: feature.evaluation.outputs().iter().cloned().collect(),
                        }
                    },
                    dependencies: feature.dependencies.clone(),
                    definition: feature.evaluation.definition().clone(),
                },
            )
        })
        .collect();
    let configuration = &mut ir.model.configurations[configuration_index];
    configuration.parameter_values = parameter_values;
    configuration.feature_states = feature_states;
    // Read-side fabricated snapshot of model-level state; tag the configuration
    // so the write path can distinguish it from feature-input lane state.
    let id = configuration.id.as_str().to_owned();
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("sldprt_configuration_snapshot_synthesized"),
            id,
        );
    }
}

fn sync_active_configuration_resolutions(ir: &mut CadIr) -> Result<(), cadmpeg_core::CodecError> {
    let mut active = ir
        .model
        .configurations
        .iter()
        .enumerate()
        .filter(|(_, configuration)| configuration.active)
        .map(|(index, _)| index);
    let Some(configuration_index) = active.next() else {
        return Ok(());
    };
    if active.next().is_some() {
        return Ok(());
    }
    let resolved = ir
        .model
        .features
        .iter()
        .filter(|feature| feature.suppressed != Some(true))
        .filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Hole {
                    placements,
                    shape,

                    extent,
                    bottom,
                    taper_angle,
                    ..
                },
            ) = feature.evaluation.definition()
            else {
                return None;
            };
            let construction = shape.construction();
            let diameter = shape.diameter();
            Some((
                feature.id.clone(),
                placements.clone(),
                construction.clone(),
                diameter,
                extent.clone(),
                *bottom,
                *taper_angle,
            ))
        })
        .collect::<Vec<_>>();
    let configuration = &mut ir.model.configurations[configuration_index];
    for (
        feature,
        resolved_placements,
        resolved_construction,
        resolved_diameter,
        resolved_extent,
        resolved_bottom,
        resolved_taper_angle,
    ) in resolved
    {
        let Some(state) = configuration.feature_states.get_mut(&feature) else {
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
        if placements.is_none() && resolved_placements.is_some() {
            *placements = resolved_placements;
        }
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
        let resolved_complete = resolved_diameter.is_some()
            && resolved_extent.as_ref().is_some_and(|extent| {
                !matches!(
                    extent,
                    cadmpeg_ir::features::LinearTermination::Unresolved {}
                )
            })
            && !matches!(
                &resolved_construction,
                cadmpeg_ir::features::holes::HoleConstruction::Form {
                    kind: cadmpeg_ir::features::holes::HoleKind::Unresolved(_)
                        | cadmpeg_ir::features::holes::HoleKind::PartialCounterbore(..)
                        | cadmpeg_ir::features::holes::HoleKind::PartialCountersink(..),
                    ..
                }
            );
        if incomplete && resolved_complete {
            shape
                .try_edit(|construction, _, diameter| {
                    match (&mut *construction, resolved_construction) {
                        (
                            cadmpeg_ir::features::holes::HoleConstruction::Form { kind, .. },
                            cadmpeg_ir::features::holes::HoleConstruction::Form {
                                kind: resolved_kind,
                                ..
                            },
                        ) => *kind = resolved_kind,
                        (construction, resolved_construction) => {
                            *construction = resolved_construction;
                        }
                    }
                    *diameter = resolved_diameter;
                })
                .map_err(cadmpeg_core::CodecError::malformed)?;
            *extent = resolved_extent;
            *bottom = resolved_bottom;
            *taper_angle = resolved_taper_angle;
        }
    }
    let resolved = ir
        .model
        .features
        .iter()
        .filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::CosmeticThread {
                    face,
                    diameter,
                    extent,
                },
            ) = feature.evaluation.definition()
            else {
                return None;
            };
            let complete = match face {
                cadmpeg_ir::features::FaceSelection::Faces(selected)
                | cadmpeg_ir::features::FaceSelection::Resolved {
                    faces: selected, ..
                } => !selected.is_empty(),
                _ => false,
            };
            complete.then_some((feature.id.clone(), face.clone(), *diameter, *extent))
        })
        .collect::<Vec<_>>();
    let configuration = &mut ir.model.configurations[configuration_index];
    for (feature, resolved_face, resolved_diameter, resolved_extent) in resolved {
        let Some(state) = configuration.feature_states.get_mut(&feature) else {
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
        if *diameter == resolved_diameter
            && *extent == resolved_extent
            && matches!(
                face,
                cadmpeg_ir::features::FaceSelection::Unresolved
                    | cadmpeg_ir::features::FaceSelection::Native(_)
            )
        {
            *face = resolved_face;
        }
    }
    let resolved = ir
        .model
        .features
        .iter()
        .filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::DatumOffsetPlane {
                    reference:
                        Some(cadmpeg_ir::features::DatumPlaneReference::Face {
                            face: face @ cadmpeg_ir::features::FaceSelection::Faces(selected),
                        }),
                    distance,
                },
            ) = feature.evaluation.definition()
            else {
                return None;
            };
            (!selected.is_empty()).then_some((feature.id.clone(), face.clone(), *distance))
        })
        .collect::<Vec<_>>();
    let configuration = &mut ir.model.configurations[configuration_index];
    for (feature, resolved_face, resolved_distance) in resolved {
        let Some(state) = configuration.feature_states.get_mut(&feature) else {
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
        if *distance == resolved_distance {
            *reference = Some(cadmpeg_ir::features::DatumPlaneReference::Face {
                face: resolved_face,
            });
        }
    }
    let resolved = ir
        .model
        .features
        .iter()
        .filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Pattern { seeds, pattern },
            ) = feature.evaluation.definition()
            else {
                return None;
            };
            if !matches!(
                pattern.definition(),
                cadmpeg_ir::features::patterns::PatternTransform::Mirror { .. }
            ) {
                return None;
            }
            Some((feature.id.clone(), seeds.clone(), pattern.clone()))
        })
        .collect::<Vec<_>>();
    let configuration = &mut ir.model.configurations[configuration_index];
    for (feature, resolved_seeds, resolved_pattern) in resolved {
        let Some(state) = configuration.feature_states.get_mut(&feature) else {
            continue;
        };
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Pattern { seeds, pattern },
        ) = &mut state.definition
        else {
            continue;
        };
        if *seeds == resolved_seeds && pattern.is_unresolved() {
            *pattern = resolved_pattern;
        }
    }

    Ok(())
}

fn stamp_feature_baseline(ir: &mut CadIr) -> Result<(), CodecError> {
    let hash = crate::history::hash::feature_hash(&ir.model)?;
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("sldprt_neutral_feature_local_sha256"),
            hash,
        );
    }
    Ok(())
}

fn assign_configuration_bodies(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    configuration_bodies: Vec<(usize, Vec<cadmpeg_ir::ids::BodyId>)>,
) -> Result<(), CodecError> {
    let mut partition_map = BTreeMap::<u32, Vec<cadmpeg_ir::ids::BodyId>>::new();
    for (index, bodies) in configuration_bodies {
        let Ok(index) = u32::try_from(index) else {
            continue;
        };
        let merged = match partition_map.entry(index) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "index SLDPRT configuration partitions")?;
                entry.insert(Vec::new())
            }
        };
        for body in bodies {
            let comparisons = merged.len().checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("merge SLDPRT configuration bodies", u64::MAX - 1, u64::MAX)
            })?;
            ctx.charge_work(
                u64::try_from(comparisons).map_err(|_| {
                    ctx.refuse_codec_limit(
                        "merge SLDPRT configuration bodies",
                        u64::MAX - 1,
                        u64::MAX,
                    )
                })?,
                "merge SLDPRT configuration bodies",
            )?;
            if !merged.contains(&body) {
                ctx.reserve_precharged_vec(
                    merged,
                    1,
                    "merge SLDPRT configuration bodies",
                )?;
                merged.push(body);
            }
        }
    }

    let mut source_counts = BTreeMap::<u32, usize>::new();
    for source_index in ir.model.configurations.iter().filter_map(|configuration| {
        configuration.source_index
    }) {
        ctx.charge_work(1, "count SLDPRT configuration sources")?;
        match source_counts.entry(source_index) {
            Entry::Occupied(mut entry) => *entry.get_mut() += 1,
            Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "count SLDPRT configuration sources")?;
                entry.insert(1);
            }
        }
    }
    for configuration in &mut ir.model.configurations {
        let Some(source_index) = configuration.source_index else {
            continue;
        };
        if source_counts.get(&source_index) == Some(&1) {
            configuration.bodies = Some(
                cadmpeg_ir::features::DistinctMembers::try_from_charged(
                    partition_map.remove(&source_index).unwrap_or_default(),
                    ctx,
                )?,
            );
        }
    }
    if let Some((active_index, position)) = bind_active_configuration_partition(ir) {
        if let Some(bodies) = partition_map.remove(&active_index) {
            ir.model.configurations[position].bodies = Some(
                cadmpeg_ir::features::DistinctMembers::try_from_charged(bodies, ctx)?,
            );
        }
    }
    for (source_index, bodies) in partition_map {
        ctx.charge_work(
            u64::try_from(ir.model.configurations.len()).map_err(|_| {
                ctx.refuse_codec_limit("order SLDPRT partition configurations", u64::MAX - 1, u64::MAX)
            })?,
            "order SLDPRT partition configurations",
        )?;
        let ordinal = ir
            .model
            .configurations
            .iter()
            .map(|configuration| configuration.ordinal)
            .max()
            .map_or(0, |ordinal| ordinal.saturating_add(1));
        ctx.reserve_collection_vec(
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
                bodies: Some(
                    cadmpeg_ir::features::DistinctMembers::try_from_charged(bodies, ctx)?,
                ),
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
fn bind_active_configuration_partition(ir: &mut CadIr) -> Option<(u32, usize)> {
    let active_name = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sw_configuration_name"));
    let active_index = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("active_parasolid_block"))
        .and_then(|section| crate::container::configuration_index(section))
        .and_then(|index| u32::try_from(index).ok());
    let (Some(active_name), Some(active_index)) = (active_name, active_index) else {
        return None;
    };
    let mut matches = ir
        .model
        .configurations
        .iter()
        .enumerate()
        .filter(|(_, configuration)| {
            configuration.source_index.is_none()
                && configuration.name.as_deref() == Some(active_name.as_str())
        })
        .map(|(position, _)| position);
    let position = matches.next()?;
    let source_identity_available = !ir
        .model
        .configurations
        .iter()
        .any(|configuration| configuration.source_index == Some(active_index));
    if matches.next().is_some() || !source_identity_available {
        return None;
    }

    // The container's active block and the native configuration name
    // establish the partition identity even when that block yielded no
    // decoded body list. Body membership remains unresolved until a decoded
    // partition supplies its body identities.
    ir.model.configurations[position].source_index = Some(active_index);
    Some((active_index, position))
}

fn stamp_configuration_baseline(ir: &mut CadIr) -> Result<(), CodecError> {
    let hash = crate::history::hash::configuration_hash(&ir.model.configurations)?;
    let parameter_value_hash =
        crate::history::hash::configuration_parameter_value_hash(&ir.model.configurations)?;
    let feature_state_hash =
        crate::history::hash::configuration_feature_state_hash(&ir.model.configurations)?;
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("sldprt_neutral_configuration_local_sha256"),
            hash,
        );
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("sldprt_configuration_parameter_values_local_sha256"),
            parameter_value_hash,
        );
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("sldprt_configuration_feature_states_local_sha256"),
            feature_state_hash,
        );
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
    ir: &mut CadIr,
    lanes: &[crate::records::FeatureInputLane],
) -> Result<(), CodecError> {
    let neutral_hash = crate::resolved_features::hashes::sketch_hash(ir)?;
    let constraint_hash = crate::resolved_features::hashes::constraint_hash(ir)?;
    let native_hash = crate::resolved_features::hashes::lane_hash(lanes)?;
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("sldprt_neutral_sketch_local_sha256"),
            neutral_hash,
        );
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("sldprt_native_sketch_sha256"),
            native_hash,
        );
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("sldprt_neutral_sketch_constraint_local_sha256"),
            constraint_hash,
        );
    }
    Ok(())
}

/// Record the document and B-rep partition baselines the write path compares
/// against.
///
/// Both are machine-local content digests and carry the `_local_sha256` suffix
/// that says so; see [`document_local_sha256`] and [`brep_local_sha256`].
fn stamp_local_digests(ir: &mut CadIr) -> Result<(), CodecError> {
    ir.finalize();
    let brep_hash = brep_local_sha256_in_place(ir)?;
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            cadmpeg_core::nonblank_literal!("brep_local_sha256"),
            brep_hash,
        );
    }
    let has_swobjects_semantics = ir
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.id.as_str().starts_with("sldprt:metadata:"))
        || ir
            .model
            .appearances
            .iter()
            .any(|appearance| appearance.schema.as_deref() == Some("moVisualProperties_c"));
    if has_swobjects_semantics {
        if let (Ok(swobjects_hash), Ok(material_hash)) = (
            crate::writer::swobjects_local_sha256(ir),
            crate::writer::swobjects_material_local_sha256(ir),
        ) {
            let identity_hash = crate::writer::swobjects_metadata_identity_local_sha256(ir)?;
            if let Some(source) = &mut ir.source {
                source.attributes.insert(
                    cadmpeg_core::nonblank_const!(crate::writer::SWOBJECTS_LOCAL_DIGEST_ATTRIBUTE),
                    swobjects_hash,
                );
                source.attributes.insert(
                    cadmpeg_core::nonblank_const!(
                        crate::writer::SWOBJECTS_MATERIAL_LOCAL_DIGEST_ATTRIBUTE
                    ),
                    material_hash,
                );
                source.attributes.insert(
                    cadmpeg_core::nonblank_const!(
                        crate::writer::SWOBJECTS_METADATA_IDENTITY_LOCAL_DIGEST_ATTRIBUTE
                    ),
                    identity_hash,
                );
            }
        }
    }
    if !ir.model.pmi.is_empty() {
        if let Ok(hash) = crate::writer::pmi_local_sha256(ir) {
            if let Some(source) = &mut ir.source {
                source.attributes.insert(
                    cadmpeg_core::nonblank_const!(crate::writer::PMI_LOCAL_DIGEST_ATTRIBUTE),
                    hash,
                );
            }
        }
    }
    let hash = document_local_sha256(ir)?;
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            cadmpeg_core::nonblank_const!(cadmpeg_ir::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE),
            hash,
        );
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
    Ok(brep_partition_sha256(ir.tolerances, partition)?.0)
}

/// [`brep_local_sha256`] without the deep clone, for the decode stamp path.
///
/// Moves the structurally untouched B-rep arenas out of `ir`, hashes the same
/// normalized partition [`brep_local_sha256`] builds, and moves them back in
/// their original order. The two arenas the normalization filters —
/// `appearances` and `appearance_bindings` — and the body display fields it
/// strips are copied, so `ir` is bit-identical afterwards and both entry
/// points produce the same digest for the same document.
fn brep_local_sha256_in_place(ir: &mut CadIr) -> Result<String, CodecError> {
    use std::mem::take;

    let saved_body_display = ir
        .model
        .bodies
        .iter()
        .map(|body| (body.name.clone(), body.color))
        .collect::<Vec<_>>();
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
    partition.appearances.clone_from(&ir.model.appearances);
    partition
        .appearance_bindings
        .clone_from(&ir.model.appearance_bindings);
    let (hash, mut partition) = brep_partition_sha256(ir.tolerances, partition)?;
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
    Ok(hash)
}

/// Normalize and hash one B-rep partition; both digest entry points share it.
///
/// Returns the normalized model with the hash so an in-place caller can move
/// its arenas back; only `bodies` (display fields stripped), `appearances`,
/// and `appearance_bindings` (both filtered to face bindings) are mutated.
fn brep_partition_sha256(
    tolerances: cadmpeg_ir::units::Tolerances,
    model: cadmpeg_ir::document::Model,
) -> Result<(String, cadmpeg_ir::document::Model), CodecError> {
    use cadmpeg_ir::appearance::AppearanceTarget;

    let mut normalized = CadIr::empty();
    normalized.tolerances = tolerances;
    normalized.model = model;
    normalized.model.bodies.iter_mut().for_each(|body| {
        body.name = None;
        body.color = None;
    });
    let face_appearances = normalized
        .model
        .appearance_bindings
        .iter()
        .filter_map(|binding| {
            matches!(binding.target, AppearanceTarget::Face(_))
                .then_some(binding.appearance.clone())
        })
        .collect::<std::collections::HashSet<_>>();
    normalized
        .model
        .appearance_bindings
        .retain(|binding| matches!(binding.target, AppearanceTarget::Face(_)));
    normalized
        .model
        .appearances
        .retain(|appearance| face_appearances.contains(&appearance.id));
    Ok((
        cadmpeg_ir::hash::canonical_json_sha256(&normalized)?,
        normalized.model,
    ))
}

/// Machine-local `document_local_sha256` for the SLDPRT write-path edit oracle.
///
/// See [`cadmpeg_ir::hash::document_local_sha256`].
pub(crate) fn document_local_sha256(ir: &CadIr) -> Result<String, CodecError> {
    Ok(cadmpeg_ir::hash::document_local_sha256(
        ir,
        "sldprt",
        "sldprt:file:source-image#0",
    )?)
}

fn preserve_source_image(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    annotations: &mut Annotations,
    unknowns: &mut Vec<UnknownRecord>,
) -> Result<(), CodecError> {
    crate::annotations::note(
        annotations,
        "sldprt:file:source-image#0",
        &cadmpeg_ir::stream_name!("source"),
        0,
        "source_image",
        Exactness::ByteExact,
    );
    ctx.reserve_collection_vec(unknowns, 1, "retain SLDPRT source image record")?;
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
    let parasolid_sources = scan
        .blocks
        .iter()
        .filter(|b| b.family == container::PayloadFamily::Parasolid)
        .count()
        + scan
            .compound_streams
            .iter()
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

    if !container::has_parasolid_body_stream(scan) {
        ctx.reserve_collection_vec(&mut losses, 1, "append SLDPRT container loss")?;
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
    let unsupported = crate::swift::unsupported_annotation_classes(ctx, scan)?;
    if unsupported.is_empty() {
        return Ok(());
    }
    let count = unsupported.values().try_fold(0usize, |sum, value| {
        sum.checked_add(*value)
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))
    })?;
    let mut classes_len = 0usize;
    for (index, (class, class_count)) in unsupported.iter().enumerate() {
        let digits = if *class_count == 0 {
            1
        } else {
            usize::try_from(class_count.ilog10()).map_err(|_| {
                ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
            })? + 1
        };
        let addition = class.len()
            .checked_add(digits)
            .and_then(|len| len.checked_add(3))
            .and_then(|len| len.checked_add(usize::from(index > 0) * 2))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        classes_len = classes_len.checked_add(addition)
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    }
    let (mut classes, _reservation) = ctx.reserve_scoped_string(classes_len, OPERATION)?;
    for (index, (class, class_count)) in unsupported.iter().enumerate() {
        if index > 0 {
            classes.push_str(", ");
        }
        classes.push_str(class);
        classes.push_str(" (");
        write!(&mut classes, "{class_count}").map_err(|_| {
            ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
        })?;
        classes.push(')');
    }
    let message = ctx.format_retained(
        format_args!(
            "{count} SWIFT semantic annotation(s) have no neutral PMI definition: {classes}."
        ),
        "retain SLDPRT unsupported SWIFT loss",
    )?;
    ctx.reserve_collection_vec(losses, 1, "append SLDPRT unsupported SWIFT loss")?;
    losses.push(SldprtLossCode::PmiSwiftAnnotationUnsupported.note(message));
    Ok(())
}

#[cfg(test)]
mod tests;
