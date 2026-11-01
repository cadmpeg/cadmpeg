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
//! metadata-only IR and blocking loss notes. `DecodeDepth::Container`
//! requests the metadata-only path.

mod input_topologies;
mod previews;
mod retention;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::Hash;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::{CadIr, ContentScope, SourceMeta};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::{AppearanceId, UnknownId};

use crate::loss::SldprtLossCode;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

mod digest_partition;

use crate::container::configuration_index;
use crate::container::contains_ascii_case_insensitive;

use crate::brep::feature_source::FeatureSourceId;
use crate::brep::graph::{decode_bodies, Brep};
use crate::container::{self, ActiveParasolidSite, ContainerScan};
use crate::parasolid::StreamHeader;
use crate::records::ObjectId;
use crate::resolved_features::scalars::FeatureObjectNames;
use cadmpeg_ir::geometry::SolvedCurveGeometry;

struct DecodedBrep {
    /// Representative stream header whose body is common to every merged site.
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
fn native_feature_has_operation_evidence(
    state: &EvaluatedFeatureState<'_>,
    native: Option<&crate::native::SldprtNative>,
) -> bool {
    matches!(
        state.definition,
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Native { .. }
        )
    ) && (!state.dependencies.is_empty()
        || !state.outputs.is_empty()
        || !state.feature.source_content.is_empty()
        || !state.feature.source_properties.is_empty()
        || native.is_some_and(|native| {
            native
                .feature_histories
                .iter()
                .flat_map(|history| &history.features)
                .find(|record| state.feature.native_ref.as_deref() == Some(record.id.as_str()))
                .is_some_and(|record| !record.content.is_empty() || !record.properties.is_empty())
        }))
}

/// Decode one seekable `.sldprt` stream into IR and diagnostics.
///
/// The function reads and retains the complete source image. Container framing
/// or I/O failures return [`CodecError`]; unsupported model records are reported
/// through the decode body when a partial result can be represented.
pub(crate) fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
    let mut scan = container::scan(ctx, root)?;
    let classification = crate::dialect::classify_layers(&scan)?;
    let form_padding = classification.host().form_code_padding();
    // Marker identities are admitted during scanning. Compound stream identities
    // are admitted here before B-rep and IR construction.
    let container_entities = u64_from_index(scan.compound_streams.len());
    ctx.charge_entities(container_entities, "admit SLDPRT container entities")?;
    let mut admitted_entities = 0_u64;

    if ctx.container_only() {
        let (ir, annotations, unknowns, mut pmi_losses, retention) = build_metadata_ir(
            ctx,
            &scan,
            &classification,
            form_padding,
            &mut admitted_entities,
            false,
        )?;
        let mut report =
            build_container_report(ctx, &scan, &classification, container::notes(&scan), 0)?;
        report.transfer = cadmpeg_ir::report::decode::DecodeTransfer {
            scope: ContentScope::container(),
            geometry_transferred: false,
        };
        report.losses.append(&mut pmi_losses);
        let active_block = active_parasolid_block(&scan);
        append_design_losses(ctx, &ir, &mut report, active_block.as_deref())?;
        append_thin_operation_losses(&retention.ambiguous_thin_features, &ir, &mut report);
        return decode_result(ctx, ir, report, annotations, unknowns, retention);
    }

    let streams = active_body_streams(&scan);
    let mut brep_fallback_losses = Vec::new();
    if !streams.is_empty() {
        ctx.charge_entities(u64_from_index(streams.len()), "admit SLDPRT body streams")?;
        if let Some((decoded, mut report)) = try_decode_brep(
            ctx,
            &scan,
            &streams,
            &classification,
            &mut brep_fallback_losses,
        )? {
            let (ir, annotations, unknowns, mut pmi_losses, retention) = build_geometry_ir(
                ctx,
                &mut scan,
                &classification,
                decoded,
                form_padding,
                &mut admitted_entities,
            )?;
            report.losses.append(&mut pmi_losses);
            append_tessellation_losses(&ir, &mut report);
            let active_block = active_parasolid_block(&scan);
            append_design_losses(ctx, &ir, &mut report, active_block.as_deref())?;
            append_thin_operation_losses(&retention.ambiguous_thin_features, &ir, &mut report);
            return decode_result(ctx, ir, report, annotations, unknowns, retention);
        }
    }

    let (ir, annotations, unknowns, mut pmi_losses, retention) = build_metadata_ir(
        ctx,
        &scan,
        &classification,
        form_padding,
        &mut admitted_entities,
        true,
    )?;
    let display_meshes = ir.model.tessellations.len();
    let mut report = build_container_report(
        ctx,
        &scan,
        &classification,
        container::notes(&scan),
        display_meshes,
    )?;
    report.transfer.scope = ir.scope;
    report.losses.append(&mut brep_fallback_losses);
    report.losses.append(&mut pmi_losses);
    append_tessellation_losses(&ir, &mut report);
    let active_block = active_parasolid_block(&scan);
    append_design_losses(ctx, &ir, &mut report, active_block.as_deref())?;
    append_thin_operation_losses(&retention.ambiguous_thin_features, &ir, &mut report);
    decode_result(ctx, ir, report, annotations, unknowns, retention)
}

/// Report thin extrusions whose ambiguous inline operation stayed unresolved.
fn append_thin_operation_losses(
    ambiguous_thin_features: &BTreeSet<String>,
    ir: &CadIr,
    report: &mut DecodeBody,
) {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, FeatureOperation};
    let unresolved = |definition: &FeatureDefinition| {
        matches!(
            definition,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                op: BooleanOp::Unresolved,
                ..
            })
        )
    };
    let ambiguous = |feature: &cadmpeg_ir::features::Feature| {
        feature
            .native_ref
            .as_ref()
            .is_some_and(|reference| ambiguous_thin_features.contains(reference))
    };
    let base = ir
        .model
        .features
        .iter()
        .filter(|feature| ambiguous(feature) && unresolved(feature.evaluation.definition()))
        .count();
    let configured = ir
        .model
        .configurations
        .iter()
        .flat_map(|configuration| {
            ir.model.features.iter().filter(move |feature| {
                ambiguous(feature)
                    && configuration
                        .feature_states
                        .get(&feature.id)
                        .and_then(|state| state.definition())
                        .is_some_and(unresolved)
            })
        })
        .count();
    let ambiguous_thin_operations = base + configured;
    if ambiguous_thin_operations > 0 {
        report.losses.push(SldprtLossCode::FeatureThinOperationAmbiguous.note(format!(
            "{ambiguous_thin_operations} thin extrusion state(s) retain an ambiguous inline Boolean operation."
        )));
    }
}

fn append_tessellation_losses(ir: &CadIr, report: &mut DecodeBody) {
    let unresolved = ir
        .model
        .tessellations
        .iter()
        .filter(|mesh| mesh.body.is_none() || mesh.faces.is_empty())
        .count();
    if unresolved > 0 {
        report.losses.push(
            SldprtLossCode::TessellationFaceOwnershipUnresolved.note(format!(
                "{unresolved} DisplayLists tessellation table(s) do not resolve to B-rep face \
                 ownership. Geometry and native channels are retained without fabricating body or \
                 face references."
            )),
        );
    }
}

fn decode_result(
    ctx: &DecodeContext<'_>,
    mut ir: CadIr,
    mut body: DecodeBody,
    annotations: Annotations,
    mut unknowns: Vec<UnknownRecord>,
    mut retention: retention::Retention,
) -> Result<Decoded, CodecError> {
    let mut source_fidelity = cadmpeg_ir::SourceFidelity::with_annotations(annotations);
    let source_image = unknowns
        .iter()
        .position(|record| record.id().as_str() == "sldprt:file:source-image#0")
        .map(|index| unknowns.remove(index));
    let (lane_images, unknowns): (Vec<_>, Vec<_>) = unknowns.into_iter().partition(|record| {
        record
            .id()
            .as_str()
            .starts_with("sldprt:source:feature-input#")
    });
    source_fidelity.retain_unknown_records("sldprt", lane_images)?;
    let opaque = retention.opaque_records(&unknowns, &source_fidelity.annotations);
    for record in opaque {
        body.losses.push(
            SldprtLossCode::SourceOpaqueRecordsUnrepresented.note(format!(
                "source record {record} has no typed neutral or native payload representation"
            )),
        );
    }
    source_fidelity.attach_native_unknown_records(&mut ir, "sldprt", unknowns, ctx)?;
    if let Some(source_image) = source_image {
        source_fidelity.retain_unknown_records("source", [source_image])?;
    }
    crate::history::configuration::normalize_configuration_differences(ctx, &mut ir)?;
    // The container projection retains native records without claiming a neutral model.
    if ctx.container_only() {
        ir.model = cadmpeg_ir::document::Model::default();
        ir.scope = ContentScope::container();
    } else {
        ir.scope = body.transfer.scope;
    }
    body.transfer.scope = ir.scope;
    if ir
        .source
        .as_mut()
        .and_then(|source| {
            source
                .attributes
                .remove("sldprt_active_partition_unresolved")
        })
        .is_some()
        && ir
            .model
            .configurations
            .iter()
            .all(|configuration| !configuration.active)
    {
        body.losses.push(
            SldprtLossCode::ConfigActiveIdentityUnresolved
                .note("the source does not resolve an active geometry partition"),
        );
    }

    let native = ir
        .native
        .namespace("sldprt")
        .map(crate::native::SldprtNative::load)
        .transpose()?;
    if let Some(native) = native.as_ref() {
        let projected = ir
            .model
            .sketch_entities
            .iter()
            .filter_map(|entity| entity.native_ref.as_deref())
            .chain(
                ir.model
                    .sketch_constraints
                    .iter()
                    .filter_map(|constraint| constraint.native_ref.as_deref()),
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
            .collect::<HashSet<_>>();
        for lane in &native.feature_input_lanes {
            for marker in &lane.sketch_entities {
                if projected.contains(marker.id()) {
                    continue;
                }
                if ctx.depth().required_scope().design {
                    body.losses.push(SldprtLossCode::SketchNativeGeometry.note(format!(
                        "sketch-input entity {} has no neutral geometry or constraint projection; its typed native marker is retained", marker.id()
                    )));
                }
                if matches!(
                    marker.kind(),
                    crate::records::SketchInputKind::Native(_)
                        | crate::records::SketchInputKind::NativeHandle(_)
                ) {
                    let id = crate::records::lane_payload::retained_id(&lane.id)?;
                    retention.record(id.as_str(), cadmpeg_ir::document::ArenaClass::Design);
                }
            }
        }
    }
    retention.apply(&mut source_fidelity)?;
    // Layout oracles describe the delivered neutral state, including an
    // explicit Container request and derived configuration snapshots.
    stamp_local_digests(ctx, &mut ir, &mut source_fidelity)?;
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
        cadmpeg_ir::scalar::NonZeroLength,
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

fn count_keys<K: Ord>(keys: impl IntoIterator<Item = K>) -> BTreeMap<K, usize> {
    let mut counts = BTreeMap::<K, usize>::new();
    for key in keys {
        *counts.entry(key).or_default() += 1;
    }
    counts
}

fn has_incoherent_refs<T: Eq + Hash>(references: &[T], known: &HashSet<&T>) -> bool {
    let mut seen = HashSet::new();
    for reference in references {
        if seen.contains(reference) || !known.contains(reference) {
            return true;
        }
        seen.insert(reference);
    }
    false
}

fn append_design_losses(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    report: &mut DecodeBody,
    active_block: Option<&str>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::edge_treatments::{ChamferGroup, FullRoundSideSelection};
    use cadmpeg_ir::features::{
        AngularTermination, BodyRetentionMode, BodySelection, BooleanOp, EdgeSelection,
        ExtrudeExtent, FaceSelection, FeatureDefinition, FeatureOperation, FeatureSourceContent,
        LinearTermination, PathRef, PlanarProfileRef, ProfileRef, RevolveExtent, SplitBodyTool,
        SplitFaceTool,
    };
    use cadmpeg_ir::features::{
        ExtrusionDirectionSource, LoftPointSection, LoftSection, SurfaceBoundary,
    };
    use cadmpeg_ir::sketches::{SketchGeometryDefinition, SpatialSketchGeometryDefinition};

    let native = match ir.native.namespace("sldprt") {
        None => None,
        Some(namespace) => match crate::native::SldprtNative::load_with_context(ctx, namespace) {
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
        report.losses.push(SldprtLossCode::ConfigActiveIdentityUnresolved.note(format!(
                "active configuration identity is unresolved; {active_configurations} of {} configuration records are active.",
                ir.model.configurations.len()
            )));
    }
    let active_partition = active_block
        .and_then(crate::container::configuration_index)
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
        report.losses.push(SldprtLossCode::ConfigActivePartitionMismatch.note(format!(
                "active configuration identity does not resolve to active geometry partition {active_partition}."
            )));
    }
    let inferred_configurations = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| configuration.native_ref.is_none())
        .count();
    if inferred_configurations > 0 {
        report.losses.push(SldprtLossCode::ConfigInferredWithoutNative.note(format!(
                "{inferred_configurations} configuration state(s) are inferred from geometry partitions without native configuration definitions."
            )));
    }
    let unresolved_configuration_parameter_lanes = if let Some(native) = native.as_ref() {
        crate::history::configuration::unresolved_configuration_lanes(
            &ir.model.configurations,
            &native.feature_input_lanes,
        )
    } else {
        0
    };
    if unresolved_configuration_parameter_lanes > 0 {
        report.losses.push(SldprtLossCode::ConfigLaneIdentityUnresolved.note(format!(
                "{unresolved_configuration_parameter_lanes} configuration-scoped feature-input lane(s) have duplicate or unresolved configuration identity."
            )));
    }
    let configuration_source_counts = count_keys(
        ir.model
            .configurations
            .iter()
            .filter_map(|configuration| configuration.source_index),
    );
    let ambiguous_configuration_sources = configuration_source_counts
        .values()
        .filter(|count| **count > 1)
        .copied()
        .sum::<usize>();
    if ambiguous_configuration_sources > 0 {
        report.losses.push(SldprtLossCode::ConfigAmbiguousPartition.note(format!(
                "{ambiguous_configuration_sources} configuration record(s) share non-unique geometry partition identities."
            )));
    }
    let empty_configuration_names = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| configuration.name.as_deref().is_none_or(str::is_empty))
        .count();
    let configuration_ordinal_counts = count_keys(
        ir.model
            .configurations
            .iter()
            .map(|configuration| configuration.ordinal),
    );
    let configuration_name_counts = count_keys(
        ir.model
            .configurations
            .iter()
            .filter_map(|configuration| configuration.name.as_deref())
            .filter(|name| !name.is_empty()),
    );
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
        report.losses.push(SldprtLossCode::ConfigAmbiguousNaming.note(format!(
                "{empty_configuration_names} configuration record(s) have empty names; {ambiguous_configuration_names} configuration record(s) share non-unique names; {ambiguous_configuration_ordinals} configuration record(s) share regeneration ordinals."
            )));
    }
    let model_body_ids = ir
        .model
        .bodies
        .iter()
        .map(|body| &body.id)
        .collect::<HashSet<_>>();
    let mut incoherent_configuration_bodies = 0;
    for configuration in &ir.model.configurations {
        if has_incoherent_refs(
            configuration.bodies.as_deref().unwrap_or_default(),
            &model_body_ids,
        ) {
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
        report.losses.push(SldprtLossCode::ConfigIncoherentBodyRefs.note(format!(
                "{unresolved_configuration_bodies} configuration record(s) have unresolved body membership; {incoherent_configuration_bodies} configuration record(s) contain missing or repeated body references."
            )));
    }

    let feature_ids = ir
        .model
        .features
        .iter()
        .map(|feature| &feature.id)
        .collect::<HashSet<_>>();
    let parameter_ids = ir
        .model
        .parameters
        .iter()
        .map(|parameter| &parameter.id)
        .collect::<HashSet<_>>();
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
                                feature
                                    .suppressed
                                    .is_some_and(|suppressed| suppressed != state.is_suppressed())
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
        report.losses.push(SldprtLossCode::ConfigIncompleteSnapshot.note(format!(
            "{incoherent_configuration_suppression} configuration(s) have missing, repeated, or feature-state-inconsistent suppression members; {incoherent_configuration_overrides} configuration(s) reference missing parameter overrides."
        )));
    }

    let mut feature_names = HashMap::new();
    for feature in &ir.model.features {
        if let Some(name) = &feature.name {
            feature_names.insert(feature.id.clone(), name.clone());
        }
    }
    let global_parameter_owners = ir
        .model
        .features
        .iter()
        .filter(|feature| crate::history::parameters::is_global_parameter_owner(feature))
        .map(|feature| feature.id.clone())
        .collect::<HashSet<_>>();
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
            ctx,
            &ir.model.parameters,
            &feature_names,
            &global_parameter_owners,
            &ir.model.configurations,
        )?;
    let feature_ordinals = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature.ordinal))
        .collect::<BTreeMap<_, _>>();
    let parameter_positions = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (&parameter.id, (&parameter.owner, parameter.ordinal)))
        .collect::<BTreeMap<_, _>>();
    let invalid_parameter_dependency_order = ir
        .model
        .parameters
        .iter()
        .filter(|parameter| {
            parameter.expression.dependencies().any(|dependency| {
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
            ctx,
            &ir.model.parameters,
            &feature_names,
            &global_parameter_owners,
            &ir.model.configurations,
        )?;
    if incomplete_parameters > 0
        || unresolved_parameter_references > 0
        || unevaluable_parameter_expressions > 0
        || invalid_parameter_dependency_order > 0
        || incoherent_parameter_dependencies > 0
        || incoherent_parameter_values > 0
    {
        report
            .losses
            .push(SldprtLossCode::ParameterUnevaluated.note(format!(
                "{incomplete_parameters} parameter(s) lack an evaluated scalar; \
             {unresolved_parameter_references} parameter expression(s) contain unresolved, \
             ambiguous, or malformed parameter references; {unevaluable_parameter_expressions} \
             parameter expression(s) cannot regenerate a finite typed value; \
             {invalid_parameter_dependency_order} parameter record(s) contain missing or \
             non-preceding dependency edges; {incoherent_parameter_dependencies} parameter \
             record(s) have dependency edges inconsistent with their expressions; \
             {incoherent_parameter_values} dependency-driven parameter(s) disagree with their \
             evaluated expressions."
            )));
    }
    let empty_parameter_names = ir
        .model
        .parameters
        .iter()
        .filter(|parameter| parameter.name.is_empty())
        .count();
    let parameter_name_counts = count_keys(
        ir.model
            .parameters
            .iter()
            .filter(|parameter| !parameter.name.is_empty())
            .map(|parameter| (&parameter.owner, parameter.name.as_str())),
    );
    let parameter_ordinal_counts = count_keys(
        ir.model
            .parameters
            .iter()
            .map(|parameter| (&parameter.owner, parameter.ordinal)),
    );
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
        report.losses.push(SldprtLossCode::ParameterAmbiguousIdentity.note(format!(
                "{empty_parameter_names} parameter record(s) have empty names; {duplicate_parameter_names} parameter record(s) share owner-local names; {duplicate_parameter_ordinals} parameter record(s) share owner-local ordinals."
            )));
    }

    let mut bound_pmi = std::collections::HashSet::new();
    if let Some(native) = native.as_ref() {
        for record in &native.pmi_dimensions {
            let id = crate::pmi::annotation_id(record)?;
            if ir.model.pmi.iter().any(|annotation| annotation.id == id && matches!(
                &annotation.definition,
                cadmpeg_ir::pmi::PmiDefinition::Dimension(dimension) if dimension.nominal().is_some_and(|nominal| nominal.parameter().is_some())
            )) {
                bound_pmi.insert(record.id.as_str());
            }
        }
    }
    let unbound_pmi_dimensions = native.as_ref().map_or(0, |native| {
        crate::pmi::unbound_dimension_count(&native.pmi_dimensions, &bound_pmi)
    });
    if unbound_pmi_dimensions > 0 && ir.scope.model {
        report.losses.push(SldprtLossCode::PmiDimensionUnbound.note(format!(
            "{unbound_pmi_dimensions} semantic dimension record(s) have no canonical parameter binding; their independent annotations or typed native payloads are retained"
        )));
    }

    let incomplete_history_references = native
        .as_ref()
        .map(|native| {
            crate::history::project::incomplete_history_reference_features(
                &native.feature_histories,
            )
        })
        .transpose()?
        .unwrap_or(0);
    if incomplete_history_references > 0 {
        let duplicates = native
            .as_ref()
            .map(|native| {
                crate::history::project::duplicate_history_source_diagnostics(
                    &native.feature_histories,
                )
            })
            .unwrap_or_default();
        let detail = if duplicates.is_empty() {
            String::new()
        } else {
            format!(" Duplicate source evidence: {}.", duplicates.join("; "))
        };
        let inferred = ir
            .model
            .features
            .iter()
            .filter(|feature| {
                feature
                    .source_properties
                    .get("dependencies_decoded")
                    .is_some_and(|value| value == "false")
            })
            .count();
        let state = if inferred > 0 {
            format!(
                " {inferred} feature(s) carry dependencies_decoded=false (inferred, not saved)."
            )
        } else {
            " Dependencies without saved references carry dependencies_decoded=false.".to_owned()
        };
        report.losses.push(SldprtLossCode::HistoryIncompleteReferences.note(format!(
            "{incomplete_history_references} feature history record(s) contain duplicate identities or unresolved parent, dependency, dimension, or child references.{detail}{state}"
        )));
    }
    let feature_positions = &feature_ordinals;
    let evaluated_feature_states = ir
        .model
        .features
        .iter()
        .map(|feature| EvaluatedFeatureState {
            feature,
            dependencies: &feature.dependencies,
            outputs: feature.evaluation.outputs(),
            definition: feature.evaluation.definition(),
        })
        .chain(ir.model.configurations.iter().flat_map(|configuration| {
            ir.model.features.iter().filter_map(move |feature| {
                configuration
                    .feature_states
                    .get(&feature.id)
                    .and_then(|state| state.definition())
                    .map(|definition| EvaluatedFeatureState {
                        feature,
                        dependencies: &feature.dependencies,
                        outputs: feature.evaluation.outputs(),
                        definition,
                    })
            })
        }))
        .collect::<Vec<_>>();
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
    let feature_ordinal_counts =
        count_keys(ir.model.features.iter().map(|feature| feature.ordinal));
    let duplicate_feature_ordinals = feature_ordinal_counts
        .values()
        .filter(|count| **count > 1)
        .copied()
        .sum::<usize>();
    if incoherent_feature_edges > 0 || duplicate_feature_ordinals > 0 {
        report.losses.push(SldprtLossCode::FeatureIncoherentEdges.note(format!(
                "{incoherent_feature_edges} feature record(s) contain missing, repeated, or non-preceding parent/dependency edges; {duplicate_feature_ordinals} feature record(s) share regeneration ordinals."
            )));
    }
    let parameter_owners = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (&parameter.id, &parameter.owner))
        .collect::<BTreeMap<_, _>>();
    let features_by_id = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature))
        .collect::<BTreeMap<_, _>>();
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
        report.losses.push(SldprtLossCode::FeatureIncoherentContent.note(format!(
                "{incoherent_feature_content} feature record(s) contain missing, repeated, misowned, or structurally inconsistent source-content references."
            )));
    }

    let unresolved_output_scopes = evaluated_feature_states
        .iter()
        .filter(|state| {
            state.outputs.is_empty()
                && native.as_ref().is_some_and(|native| {
                    native
                        .feature_histories
                        .iter()
                        .flat_map(|history| &history.features)
                        .find(|record| {
                            state.feature.native_ref.as_deref() == Some(record.id.as_str())
                        })
                        .and_then(|record| record.properties.get("Scope"))
                        .is_some_and(|scope| !scope.trim().is_empty())
                })
        })
        .count();
    if unresolved_output_scopes > 0 {
        report.losses.push(SldprtLossCode::FeatureUnresolvedOutputScope.note(format!(
                "{unresolved_output_scopes} feature(s) retain non-empty native output scopes that do not resolve to model bodies."
            )));
    }
    let mut incoherent_feature_outputs = 0;
    for state in &evaluated_feature_states {
        if has_incoherent_refs(state.outputs, &model_body_ids) {
            incoherent_feature_outputs += 1;
        }
    }
    if incoherent_feature_outputs > 0 {
        report.losses.push(SldprtLossCode::FeatureIncoherentOutputs.note(format!(
                "{incoherent_feature_outputs} feature record(s) contain missing or repeated output body references."
            )));
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
        report.losses.push(SldprtLossCode::SketchNativeConstraint.note(format!(
                "{native_constraints} planar or spatial sketch constraint(s) retain native relation kinds and operands without complete neutral geometric semantics."
            )));
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
        report.losses.push(SldprtLossCode::SketchNativeGeometry.note(format!(
                "{native_sketch_geometry} sketch entity geometry record(s) retain native kinds without solved neutral geometry."
            )));
    }

    let unprojected_relations = match native.as_ref() {
        Some(native) => unprojected_sketch_relation_records(ir, native),
        None => 0,
    };
    if unprojected_relations > 0 {
        report.losses.push(SldprtLossCode::SketchRelationUnprojected.note(format!(
                "{unprojected_relations} native sketch relation record(s) have no projected neutral constraint."
            )));
    }
    let multiply_projected_relations = match native.as_ref() {
        Some(native) => multiply_projected_sketch_relation_records(ir, native),
        None => 0,
    };
    if multiply_projected_relations > 0 {
        report.losses.push(SldprtLossCode::SketchRelationMultiplyProjected.note(format!(
                "{multiply_projected_relations} native sketch relation record(s) are claimed by multiple neutral objects."
            )));
    }

    for record in native
        .iter()
        .flat_map(|native| &native.feature_histories)
        .flat_map(|history| &history.features)
    {
        if crate::classification::classify(record)
            == Some(crate::classification::FeatureClass::Helix)
            && ["D3", "D4", "D5"]
                .iter()
                .all(|name| record.parameters.contains_key(*name))
        {
            report.losses.push(SldprtLossCode::FeatureNativeKindRetained.note(format!(
                "feature {}: native helix authoring law is unresolved; height, pitch and turns remain separate canonical native parameter bindings",
                record.id
            )));
        }
    }
    let native_features = evaluated_feature_states
        .iter()
        .filter(|state| native_feature_has_operation_evidence(state, native.as_ref()))
        .count();
    if native_features > 0 {
        report.losses.push(SldprtLossCode::FeatureNativeKindRetained.note(format!(
                "{native_features} feature(s) retain their native kind without a complete neutral operation definition."
            )));
    }
    let unbound_feature_input_objects = match native.as_ref() {
        Some(native) => unbound_feature_input_operation_objects(native),
        None => 0,
    };
    if unbound_feature_input_objects > 0 {
        report.losses.push(SldprtLossCode::FeatureInputObjectUnbound.note(format!(
                "{unbound_feature_input_objects} native feature-input operation object(s) do not bind uniquely to a history feature."
            )));
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
        | LinearTermination::ToLast {}
        | LinearTermination::ToPlane { .. } => false,
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
    let incomplete_chamfer_groups =
        |groups: &cadmpeg_ir::features::NonEmptyMembers<ChamferGroup>| {
            groups.is_empty()
                || groups.iter().any(|group| {
                    incomplete_edge_selection(&group.edges) || group.spec.is_unresolved()
                })
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
                | FeatureOperation::Decal { asset, .. } => !ir
                    .model
                    .assets
                    .iter()
                    .any(|candidate| candidate.id == *asset),
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
                FeatureOperation::FaceFromShapes { sources, .. } => {
                    incomplete_body_selection(sources)
                }
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
                    ..
                } => incomplete_face_selection(face) || diameter.is_none() || extent.is_none(),
                FeatureOperation::SketchBlockDefinition { sketch } => sketch.is_none(),
                FeatureOperation::SketchBlockInstance { block, placement } => {
                    block.is_none() || placement.is_none()
                }
                FeatureOperation::DatumOffsetPlane { reference, .. } => {
                    reference.as_ref().is_none_or(|reference| match reference {
                        cadmpeg_ir::features::DatumPlaneReference::Feature { .. } => false,
                        cadmpeg_ir::features::DatumPlaneReference::Face { face } => {
                            incomplete_face_selection(face)
                        }
                        cadmpeg_ir::features::DatumPlaneReference::ResolvedPlane { .. } => false,
                    })
                }
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
                FeatureOperation::Wrap { profile, face, .. } => {
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
                        || matches!(
                            direction,
                            cadmpeg_ir::features::ExtrudeDirection::Unresolved {}
                        )
                        || match start {
                            cadmpeg_ir::features::ExtrudeStart::Unresolved {} => true,
                            cadmpeg_ir::features::ExtrudeStart::FromFace { face, .. } => {
                                incomplete_face_selection(face)
                            }
                            cadmpeg_ir::features::ExtrudeStart::ProfilePlane {}
                            | cadmpeg_ir::features::ExtrudeStart::OffsetProfilePlane { .. }
                            | cadmpeg_ir::features::ExtrudeStart::FromPlane { .. } => false,
                        }
                        || matches!(
                            direction,
                            cadmpeg_ir::features::ExtrudeDirection::Explicit {
                                source: Some(ExtrusionDirectionSource::Edge { reference }),
                                ..
                            }
                                if incomplete_path(reference)
                        )
                        || incomplete_extrude_extent(extent)
                        || *op == BooleanOp::Unresolved
                }
                FeatureOperation::Revolve { construction, op } => match construction {
                    cadmpeg_ir::features::RevolveConstruction::Unresolved(_) => true,
                    cadmpeg_ir::features::RevolveConstruction::Resolved {
                        profile, extent, ..
                    } => {
                        incomplete_planar_profile(profile)
                            || incomplete_revolve_extent(extent)
                            || *op == BooleanOp::Unresolved
                    }
                },
                FeatureOperation::Sweep {
                    shape,
                    path,
                    orientation,
                    ..
                } => {
                    let mode = shape.mode();
                    shape.any_section_is_unresolved()
                        || shape.referenced_profiles().any(incomplete_planar_profile)
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
                                feature_positions,
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
                                feature_positions,
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
                            LoftSection::Profile(profile) => incomplete_profile(profile),
                            LoftSection::Point(LoftPointSection::Native(_)) => true,
                            LoftSection::Point(_) => false,
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
                        || matches!(
                            construction.draft,
                            cadmpeg_ir::features::RibDraft::Unresolved
                        )
                        || *op == BooleanOp::Unresolved
                }
                FeatureOperation::SheetMetalBaseFlange { profile, .. } => {
                    incomplete_planar_profile(profile)
                }
                FeatureOperation::SheetMetalAttachedPlate {
                    profile,
                    attachment,
                    ..
                } => incomplete_planar_profile(profile) || incomplete_edge_selection(attachment),
                FeatureOperation::SheetMetalBend { attachment, .. } => {
                    incomplete_edge_selection(attachment)
                }
                FeatureOperation::SheetMetalCorner { .. } => false,
                FeatureOperation::SheetMetalEdgeFlange { edges, .. } => {
                    incomplete_edge_selection(edges)
                }
                FeatureOperation::SheetMetalHem { .. } => true,
                FeatureOperation::Fillet { groups } => {
                    groups.is_empty()
                        || groups.iter().any(|group| {
                            incomplete_edge_selection(&group.edges) || group.radius.is_unresolved()
                        })
                }
                FeatureOperation::FullRoundFillet { groups } => {
                    groups.is_empty()
                        || groups.iter().any(|group| {
                            incomplete_face_selection(group.center_faces())
                                || matches!(
                                    group.side_one_faces(),
                                    FullRoundSideSelection::Unresolved
                                )
                                || matches!(
                                    group.side_two_faces(),
                                    FullRoundSideSelection::Unresolved
                                )
                                || matches!(
                                    group.side_one_faces(),
                                    FullRoundSideSelection::Explicit(
                                        ref selection
                                    ) if incomplete_face_selection(selection)
                                )
                                || matches!(
                                    group.side_two_faces(),
                                    FullRoundSideSelection::Explicit(
                                        ref selection
                                    ) if incomplete_face_selection(selection)
                                )
                        })
                }
                FeatureOperation::Chamfer { groups, .. } => incomplete_chamfer_groups(groups),
                FeatureOperation::FaceBlend { operands, radius } => {
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
                        SurfaceBoundary::Edges(edges) => incomplete_edge_selection(edges),
                        SurfaceBoundary::Path(path) => incomplete_path(path),
                    }) || if continuity.uniform_value()
                        == Some(cadmpeg_ir::features::SurfaceContinuity::Contact)
                    {
                        incomplete_optional_face_selection(support_faces)
                    } else {
                        incomplete_face_selection(support_faces)
                    } || continuity.is_unresolved()
                        || merge_result.is_none()
                }
                FeatureOperation::TrimSurface {
                    faces, tool, keep, ..
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
                        || if matches!(
                            mode,
                            cadmpeg_ir::features::RuledSurfaceMode::Direction { .. }
                        ) {
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
                FeatureOperation::Combine { operands, .. } => {
                    let target = operands.target();
                    let tools = operands.tools();
                    incomplete_body_selection(target) || incomplete_body_selection(tools)
                }
                FeatureOperation::BoundaryFill { tools, cells } => {
                    incomplete_body_selection(tools) || cells.iter().any(incomplete_body_selection)
                }
                FeatureOperation::CutWithSurface { targets, tools, .. } => {
                    incomplete_body_selection(targets) || incomplete_face_selection(tools)
                }
                FeatureOperation::TrimBodies { operands, keep } => {
                    let targets = operands.targets();
                    let tools = operands.tools();
                    incomplete_body_selection(targets)
                        || incomplete_body_selection(tools)
                        || *keep == cadmpeg_ir::features::BodyTrimSide::Unresolved
                }
                FeatureOperation::SplitBody { targets, tool } => {
                    incomplete_body_selection(targets)
                        || match tool {
                            SplitBodyTool::Faces(faces) => incomplete_face_selection(faces),
                            SplitBodyTool::Plane { .. } => false,
                        }
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
                FeatureOperation::ReplaceFace { operands } => {
                    let targets = operands.targets();
                    let replacements = operands.replacements();
                    incomplete_face_selection(targets) || incomplete_face_selection(replacements)
                }
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
                        || factors.resolved(ir).is_none()
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
                        || extent.as_ref().is_none_or(incomplete_linear_termination)
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
                            cadmpeg_ir::features::patterns::PatternSeed::Occurrences(
                                occurrences,
                            ) => occurrences.is_empty(),
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
        report.losses.push(SldprtLossCode::FeatureTypedOperandIncomplete.note(format!(
            "{incomplete_typed_features} typed feature(s) retain native or unresolved required operation operands."
        )));
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
        report.losses.push(SldprtLossCode::FeatureBodyRetentionUnresolved.note(format!(
            "{unresolved_body_modes} body delete/keep feature(s) retain selected native body identities without a decoded retention mode."
        )));
    }
    Ok(())
}

/// The first name record at each offset of a lane.
fn names_by_offset(
    names: &[crate::records::FeatureInputName],
) -> HashMap<u64, &crate::records::FeatureInputName> {
    let mut by_offset = HashMap::new();
    for name in names {
        by_offset.entry(name.offset).or_insert(name);
    }
    by_offset
}

/// History features without an input class by source id, in history order.
fn unclassed_features_by_source<'a>(
    features: impl IntoIterator<Item = &'a crate::records::Feature>,
) -> HashMap<u32, Vec<&'a crate::records::Feature>> {
    let mut by_source = HashMap::<_, Vec<_>>::new();
    for feature in features {
        if feature.input_class.is_some() {
            continue;
        }
        if let Some(source) = feature.source_value() {
            by_source.entry(source).or_default().push(feature);
        }
    }
    by_source
}

fn unbound_feature_input_operation_objects(native: &crate::native::SldprtNative) -> usize {
    use crate::classification::{classify, native_object_class};
    use crate::records::FeatureInputClassRole;

    let features = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features);
    let unclassed_by_source = unclassed_features_by_source(features.clone());
    let source_counts = count_keys(
        features
            .clone()
            .filter_map(super::records::Feature::source_value),
    );
    let binding_counts = count_keys(
        features
            .clone()
            .filter_map(|feature| feature.source_value().zip(feature.input_class.as_deref())),
    );
    let named_binding_counts = count_keys(native.feature_input_lanes.iter().flat_map(|lane| {
        let names = FeatureObjectNames::new(&lane.names);
        features.clone().filter_map(move |feature| {
            feature
                .input_class
                .as_deref()
                .zip(names.of(feature))
                .map(|(class, name)| (lane.id.as_str(), name.id.as_str(), class))
        })
    }));
    native
        .feature_input_lanes
        .iter()
        .flat_map(|lane| {
            let names = names_by_offset(&lane.names);
            lane.classes
                .iter()
                .filter(|class| class.role() == FeatureInputClassRole::Feature)
                .filter_map(move |class| {
                    let name_offset = class
                        .offset
                        .checked_add(6 + u64_from_index(class.name.len()))?;
                    names.get(&name_offset).map(|&name| (lane, class, name))
                })
        })
        .filter(|(lane, class, name)| {
            let source_bound = name.object_id.and_then(ObjectId::value).is_some_and(|id| {
                source_counts.get(&id).copied() == Some(1)
                    && (binding_counts.get(&(id, class.name.as_str())).copied() == Some(1)
                        || native_object_class(&class.name)
                            .feature()
                            .is_some_and(|expected| {
                                unclassed_by_source.get(&id).is_some_and(|features| {
                                    features
                                        .iter()
                                        .any(|feature| classify(feature) == Some(expected))
                                })
                            }))
            });
            let name_bound = named_binding_counts
                .get(&(lane.id.as_str(), name.id.as_str(), class.name.as_str()))
                .copied()
                == Some(1);
            !(source_bound || name_bound)
        })
        .count()
}

fn unprojected_sketch_relation_records(ir: &CadIr, native: &crate::native::SldprtNative) -> usize {
    use crate::resolved_features::relation_table;
    let constraint_ids = ir
        .model
        .sketch_constraints
        .iter()
        .map(|constraint| constraint.id.as_str())
        .collect::<HashSet<_>>();
    let sketch_feature_refs = ir
        .model
        .features
        .iter()
        .filter(|feature| {
            matches!(
                feature.evaluation.definition(),
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::Sketch { .. }
                        | cadmpeg_ir::features::FeatureOperation::SpatialSketch { .. }
                )
            )
        })
        .filter_map(|feature| feature.native_ref.as_deref())
        .collect::<HashSet<_>>();
    let projected = ir
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
        .collect::<HashSet<_>>();
    let owned_instances = crate::resolved_features::relation_geometry::owned_relation_parameters(
        &ir.model.features,
        &ir.model.parameters,
        &native.feature_input_lanes,
    );

    let mut total = 0;
    for lane in &native.feature_input_lanes {
        let markers_by_id = lane
            .sketch_entities
            .iter()
            .map(|marker| (marker.id(), marker))
            .collect::<HashMap<_, _>>();
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
        let mut markers = 0;
        for marker in &lane.sketch_entities {
            if marker
                .feature_ref
                .as_deref()
                .is_some_and(|feature_ref| sketch_feature_refs.contains(feature_ref))
                && crate::resolved_features::typed_relations::marker_owns_constraint(
                    marker,
                    &markers_by_id,
                )
                && !projected.contains(marker.id())
            {
                markers += 1;
            }
        }
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        let table_rows = relation_table::lane_tables(&lane.native_payload)
            .iter()
            .flat_map(|sketch| &sketch.rows)
            .map(|row| relation_table::row_id(lane_key, row.offset))
            .filter(|id| !constraint_ids.contains(id.as_str()))
            .count();
        total += instances + bindings + markers + table_rows;
    }
    total
}

fn multiply_projected_sketch_relation_records(
    ir: &CadIr,
    native: &crate::native::SldprtNative,
) -> usize {
    let mut native_relation_ids = HashSet::new();
    for lane in &native.feature_input_lanes {
        let markers_by_id = lane
            .sketch_entities
            .iter()
            .map(|marker| (marker.id(), marker))
            .collect::<HashMap<_, _>>();
        for relation in &lane.relation_instances {
            native_relation_ids.insert(relation.id.as_str());
        }
        for marker in &lane.sketch_entities {
            if crate::resolved_features::typed_relations::marker_owns_constraint(
                marker,
                &markers_by_id,
            ) {
                native_relation_ids.insert(marker.id());
            }
        }
    }
    let projection_counts = count_keys(
        ir.model
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
    );
    projection_counts
        .values()
        .filter(|count| **count > 1)
        .count()
}

fn conflicting_display_reference(
    stream: &str,
    table_index: usize,
    candidates: &BTreeSet<FeatureSourceId>,
) -> String {
    let candidates = candidates
        .iter()
        .map(|source| source.value().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    format!("{stream}::DisplayFace[{table_index}] ({candidates})")
}

fn appearance_assignment_loss_message(
    assigned: &BTreeSet<FeatureSourceId>,
    matched: &BTreeSet<FeatureSourceId>,
    conflicts: &[String],
) -> Option<String> {
    const PREFIX: &str = "VisualStates feature appearance assignment unresolved: ";
    const MISSING_PREFIX: &str = "feature source ID(s) ";
    const MISSING_SUFFIX: &str = " have no agreeing DisplayFace persistent reference";
    const CONFLICT_PREFIX: &str = "conflicting references rejected for ";

    let has_unmatched = assigned.difference(matched).next().is_some();
    if !has_unmatched && conflicts.is_empty() {
        return None;
    }
    let mut message = PREFIX.to_owned();
    if has_unmatched {
        message.push_str(MISSING_PREFIX);
        let missing = assigned
            .difference(matched)
            .map(|source| source.value().to_string())
            .collect::<Vec<_>>();
        message.push_str(&missing.join(", "));
        message.push_str(MISSING_SUFFIX);
    }
    if !conflicts.is_empty() {
        if has_unmatched {
            message.push_str("; ");
        }
        message.push_str(CONFLICT_PREFIX);
        message.push_str(&conflicts.join("; "));
    }
    message.push('.');
    Some(message)
}

/// Collect the available Parasolid body streams, excluding auxiliary sites.
fn active_body_streams<'a>(scan: &'a ContainerScan<'_>) -> Vec<ActiveParasolidSite<'a>> {
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
            streams.push(ActiveParasolidSite {
                section,
                payload: &stream.payload,
                header: &stream.header,
            });
        }
    }
    streams.sort_by(|left, right| {
        let key = |stream: &ActiveParasolidSite<'_>| {
            (
                !contains_ascii_case_insensitive(stream.source_stream().as_str(), "partition"),
                !contains_ascii_case_insensitive(&stream.header.description, "partition"),
            )
        };
        key(left).cmp(&key(right))
    });
    streams
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
    fallback_losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Option<(DecodedBrep, DecodeBody)>, CodecError> {
    let mut sites: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, stream) in streams.iter().enumerate() {
        sites.entry(stream.site_key()).or_default().push(index);
    }
    let mut decoded_sites = Vec::new();
    for (site, indices) in &sites {
        let first = indices[0];
        let bodies = indices
            .iter()
            .map(|index| (streams[*index].payload, streams[*index].header))
            .collect::<Vec<_>>();
        let decoded = decode_bodies(ctx, &bodies, streams[first].source_stream())?;
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
            append_topology_record_losses(&decoded_sites[selected_site].2.stats, fallback_losses);
            return Ok(None);
        }
    } else {
        let any_site_has_geometry = decoded_sites.iter().any(|(_, _, decoded)| {
            !decoded.faces.is_empty() || !decoded.surfaces.is_empty() || !decoded.points.is_empty()
        });
        let any_empty_model = decoded_sites.iter().any(|(site, _, decoded)| {
            decoded.stats.source_entity_records == 0
                && sites[site].iter().any(|index| {
                    contains_ascii_case_insensitive(
                        &streams[*index].header.description,
                        "partition",
                    )
                })
                && sites[site].iter().any(|index| {
                    contains_ascii_case_insensitive(&streams[*index].header.description, "deltas")
                })
        });
        if !any_site_has_geometry && !any_empty_model {
            for (_, _, decoded) in &decoded_sites {
                append_topology_record_losses(&decoded.stats, fallback_losses);
            }
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
            let description = header.description.clone();
            let schema = header.schema.value().to_owned();
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
        decoded.qualify_ids(&selected_site_key)?;
    }
    bind_opaque_geometry(&mut decoded, &streams[selected].section.native_id());
    let mut configuration_bodies = Vec::new();
    if let Some(index) = configuration_index(streams[selected].source_stream().as_str()) {
        configuration_bodies.push((index, copy_body_ids(&decoded.bodies)));
    }
    for (site, first, mut alternate) in decoded_sites {
        alternate.qualify_ids(&site)?;
        bind_opaque_geometry(&mut alternate, &streams[first].section.native_id());
        if let Some(index) = configuration_index(streams[first].source_stream().as_str()) {
            configuration_bodies.push((index, copy_body_ids(&alternate.bodies)));
        }
        // Keep only the selected source's bridge sequence namespace. Alternate
        // configuration sites are qualified into the model but do not own the
        // active SWIFT CadIdentifier lane.
        merge_brep(&mut decoded, alternate)?;
    }
    let report = build_geometry_report(
        ctx,
        scan,
        &mut decoded,
        classification,
        container::notes(scan),
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

fn copy_body_ids(bodies: &[cadmpeg_ir::topology::Body]) -> Vec<cadmpeg_ir::ids::BodyId> {
    bodies.iter().map(|body| body.id.clone()).collect()
}

fn bind_opaque_geometry(brep: &mut Brep, source: &UnknownId) {
    for surface in &mut brep.surfaces {
        if let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record }) =
            &mut surface.geometry
        {
            if record.is_none() {
                *record = Some(source.clone());
            }
        }
    }
    for curve in &mut brep.curves {
        if let cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
            record,
        }) = &mut curve.geometry
        {
            if record.is_none() {
                *record = Some(source.clone());
            }
        }
    }
}

/// Copy source bytes that the decoded output keeps.
fn retained_copy(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    operation: &'static str,
) -> Result<Vec<u8>, CodecError> {
    ctx.retain_bytes(u64_from_index(bytes.len()), operation)?;
    Ok(bytes.to_vec())
}

fn merge_brep(target: &mut Brep, mut source: Brep) -> Result<(), CodecError> {
    // Sequence links are source-local and belong only to the selected SWIFT
    // source. Alternate configuration sequences must not enter its namespace.
    target
        .annotations
        .append(source.annotations)
        .map_err(CodecError::from)?;
    target.bodies.append(&mut source.bodies);
    target.regions.append(&mut source.regions);
    target.shells.append(&mut source.shells);
    target.faces.append(&mut source.faces);
    target.loops.append(&mut source.loops);
    target.coedges.append(&mut source.coedges);
    target.edges.append(&mut source.edges);
    target.vertices.append(&mut source.vertices);
    target.points.append(&mut source.points);
    target.surfaces.append(&mut source.surfaces);
    target
        .procedural_surfaces
        .append(&mut source.procedural_surfaces);
    target.curves.append(&mut source.curves);
    target.pcurves.append(&mut source.pcurves);
    target.unknowns.append(&mut source.unknowns);
    target.face_colors.append(&mut source.face_colors);
    target.face_atoms.append(&mut source.face_atoms);
    target
        .native_face_identities
        .append(&mut source.native_face_identities);
    target
        .native_body_identities
        .append(&mut source.native_body_identities);
    target.body_modifiers.append(&mut source.body_modifiers);
    target.losses.append(&mut source.losses);
    target.untyped_model |= source.untyped_model;
    target.stats.unknown_surface_faces += source.stats.unknown_surface_faces;
    target.stats.unknown_procedural_supports += source.stats.unknown_procedural_supports;
    target.stats.unknown_curve_edges += source.stats.unknown_curve_edges;
    target.stats.ambiguous_pcurve_parameters += source.stats.ambiguous_pcurve_parameters;
    target.stats.off_surface_nurbs_pcurves += source.stats.off_surface_nurbs_pcurves;
    target
        .stats
        .surface_solver_capped
        .append(&mut source.stats.surface_solver_capped);
    target.stats.source_entity_records += source.stats.source_entity_records;
    target.stats.unresolved_face_colors += source.stats.unresolved_face_colors;
    target.stats.unresolved_topology_points += source.stats.unresolved_topology_points;
    target.stats.unresolved_topology_loops += source.stats.unresolved_topology_loops;
    target.stats.unresolved_topology_fins += source.stats.unresolved_topology_fins;
    target.stats.unresolved_topology_vertices += source.stats.unresolved_topology_vertices;
    target.stats.withheld_fin_partners += source.stats.withheld_fin_partners;
    target.stats.withheld_fin_links += source.stats.withheld_fin_links;
    target.stats.withheld_ring_edges += source.stats.withheld_ring_edges;
    target.stats.unclaimed_faces += source.stats.unclaimed_faces;
    target.stats.synthetic_body_grouping |= source.stats.synthetic_body_grouping;
    Ok(())
}

#[derive(Clone, Copy)]
enum ScopedAppearanceOrigin {
    DisplayList,
    VisualState,
}

fn ensure_display_appearance(
    ir: &mut CadIr,
    definition: &crate::appearance::AppearanceDefinition,
    section_ordinal: usize,
    annotations: &mut Annotations,
    origin: ScopedAppearanceOrigin,
) -> Result<AppearanceId, CodecError> {
    if let Some(existing) = ir.model.appearances.iter().find(|appearance| {
        appearance.name.as_deref() == Some(definition.name.as_str())
            && appearance.base_color == Some(definition.color)
    }) {
        return Ok(existing.id.clone());
    }
    let (namespace, kind, schema) = match origin {
        ScopedAppearanceOrigin::DisplayList => (
            cadmpeg_ir::identity_namespace!("sldprt", "appearance", "displaylist"),
            "displaylist_visual_properties",
            "moVisualProperties_c",
        ),
        ScopedAppearanceOrigin::VisualState => (
            cadmpeg_ir::identity_namespace!("sldprt", "appearance", "visualstate"),
            "visual_states_scoped_appearance",
            "SolidWorks.VisualStates",
        ),
    };
    let id = AppearanceId::compose(
        &namespace,
        cadmpeg_ir::ids::IdentityKey::from(section_ordinal).colon(definition.record_offset),
    );
    crate::annotations::note(
        annotations,
        id.as_str(),
        &definition.source_name,
        u64_from_index(definition.record_offset),
        kind,
        Exactness::ByteExact,
    )?;
    ir.model.appearances.push(Appearance {
        native_ref: None,
        id: id.clone(),
        name: Some(definition.name.clone()),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: Some(schema.into()),
        category: None,
        base_color: Some(definition.color),
        properties: BTreeMap::new(),
        textures: Vec::new(),
    });
    Ok(id)
}

type BuiltDocument = (
    CadIr,
    Annotations,
    Vec<UnknownRecord>,
    Vec<cadmpeg_ir::report::loss::LossNote>,
    retention::Retention,
);

struct DisplayPush {
    persistent_face_bindings: Vec<crate::tessellation::PersistentFaceBinding>,
    display_appearance_ranks: BTreeMap<String, u8>,
    matched_feature_sources: BTreeSet<FeatureSourceId>,
    conflicting_display_references: Vec<String>,
    unresolved: Vec<crate::appearance::UnresolvedAppearanceBinding>,
    feature_appearance_sources: BTreeSet<FeatureSourceId>,
    mesh_count: usize,
    withheld_reused_tags: Vec<usize>,
}

type CollectedDisplaySections<'s> = Vec<(
    crate::container::Section<'s>,
    Vec<crate::tessellation::DisplayFace>,
)>;

fn collect_display_sections<'s>(
    ctx: &DecodeContext<'_>,
    scan: &'s ContainerScan<'s>,
) -> Result<(CollectedDisplaySections<'s>, crate::tessellation::Summary), CodecError> {
    let mut display_sections = Vec::new();
    let mut display_summary = crate::tessellation::Summary::default();
    for section in scan.sections() {
        let faces = crate::tessellation::section_display_faces(ctx, section)?;
        let summary = crate::tessellation::summary_for_faces(section.payload(), &faces);
        display_summary.vertices += summary.vertices;
        display_summary.triangles += summary.triangles;
        for (total, count) in display_summary
            .extended_forms
            .iter_mut()
            .zip(summary.extended_forms)
        {
            *total += count;
        }
        display_summary.opaque_extended_tables += summary.opaque_extended_tables;
        display_sections.push((section, faces));
    }
    Ok((display_sections, display_summary))
}

fn display_binding_channels(
    definition: &crate::appearance::AppearanceDefinition,
) -> BTreeMap<cadmpeg_core::text::NonBlankString, String> {
    let mut channels = BTreeMap::new();
    if let Some(key) = cadmpeg_core::text::NonBlankString::new("packed_color_hex".to_owned()) {
        channels.insert(key, definition.packed_hex.clone());
    }
    channels
}

fn retain_unresolved_appearance(
    ir: &mut CadIr,
    unresolved: &crate::appearance::UnresolvedAppearanceBinding,
) {
    let Some(hex) = unresolved.packed_hex.as_deref() else {
        return;
    };
    let Ok(packed) = u32::from_str_radix(hex, 16) else {
        return;
    };
    let [red, green, blue, _] = packed.to_le_bytes();
    let color = cadmpeg_ir::topology::Color::from_rgba8(red, green, blue, 255);
    let appearance_id = cadmpeg_ir::ids::AppearanceId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "appearance", "unresolved"),
        cadmpeg_ir::ids::IdentityKey::from(u64_from_index(unresolved.appearance_index))
            .colon(unresolved.source_offset),
    );
    if !ir
        .model
        .appearances
        .iter()
        .any(|appearance| appearance.id == appearance_id)
    {
        ir.model
            .appearances
            .push(cadmpeg_ir::appearance::Appearance {
                native_ref: None,
                id: appearance_id.clone(),
                name: Some("Unresolved appearance".to_owned()),
                asset_guid: None,
                library_id: None,
                visual_guid: None,
                physical_token: None,
                schema: Some("unresolved".to_owned()),
                category: None,
                base_color: Some(color),
                properties: BTreeMap::new(),
                textures: Vec::new(),
            });
    }
    let binding_id = cadmpeg_ir::ids::AppearanceBindingId::compose(
        &cadmpeg_ir::identity_namespace!("sldprt", "appearance", "unresolved-binding"),
        cadmpeg_ir::ids::IdentityKey::from(u64_from_index(unresolved.appearance_index))
            .colon(unresolved.source_offset),
    );
    if ir
        .model
        .appearance_bindings
        .iter()
        .any(|binding| binding.id == binding_id)
    {
        return;
    }
    ir.model.appearance_bindings.push(AppearanceBinding {
        native_ref: None,
        id: binding_id,
        target: AppearanceTarget::Source {
            source_id: unresolved.identity.clone(),
        },
        appearance: appearance_id,
        source_entity_id: Some(unresolved.identity.clone()),
        object_type: Some(unresolved.owner_class.clone()),
        visible: None,
        channels: unresolved_binding_channels(unresolved),
    });
}

fn unresolved_binding_channels(
    unresolved: &crate::appearance::UnresolvedAppearanceBinding,
) -> BTreeMap<cadmpeg_core::text::NonBlankString, String> {
    let mut channels = BTreeMap::new();
    for (key, value) in [
        ("owner_class", unresolved.owner_class.clone()),
        ("identity", unresolved.identity.clone()),
        ("appearance_index", unresolved.appearance_index.to_string()),
        ("reason", unresolved.reason.clone()),
        ("source_offset", unresolved.source_offset.to_string()),
    ] {
        if let Some(key) = cadmpeg_core::text::NonBlankString::new(key.to_owned()) {
            channels.insert(key, value);
        }
    }
    if let Some(hex) = &unresolved.packed_hex {
        if let Some(key) = cadmpeg_core::text::NonBlankString::new("packed_color_hex".to_owned()) {
            channels.insert(key, hex.clone());
        }
    }
    channels
}

fn push_display_meshes(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    display_sections: Vec<(
        crate::container::Section<'_>,
        Vec<crate::tessellation::DisplayFace>,
    )>,
    ir: &mut CadIr,
    annotations: &mut Annotations,
    unknowns: &mut Vec<UnknownRecord>,
    eligible_features: &BTreeSet<FeatureSourceId>,
) -> Result<DisplayPush, CodecError> {
    let feature_appearance_sources = crate::appearance::feature_assignments(scan)
        .into_iter()
        .map(|assignment| assignment.feature_source_id)
        .collect::<BTreeSet<_>>();
    let mut matched_feature_sources = BTreeSet::new();
    let mut conflicting_display_references = Vec::new();
    let mut persistent_face_bindings = Vec::new();
    let mut withheld_reused_tags = Vec::new();
    let mut display_appearance_ranks = BTreeMap::new();
    let mut unresolved = Vec::new();
    let mut mesh_count = 0usize;
    for (display, display_faces) in display_sections {
        if display_faces.is_empty() {
            continue;
        }
        let display_stream = display.source_stream();
        for (table_index, face) in display_faces.iter().enumerate() {
            let candidates = face
                .surface_references
                .iter()
                .map(crate::tessellation::PersistentSurfaceReference::feature_source_id)
                .collect::<BTreeSet<_>>();
            if candidates.len() > 1 {
                conflicting_display_references.push(conflicting_display_reference(
                    display_stream.as_str(),
                    table_index,
                    &candidates,
                ));
                unresolved.push(crate::appearance::UnresolvedAppearanceBinding {
                    owner_class: "DisplayFace".to_owned(),
                    identity: format!("{}::DisplayFace[{table_index}]", display_stream.as_str()),
                    appearance_index: table_index,
                    reason: format!(
                        "conflicting persistent surface references: {}",
                        conflicting_display_references
                            .last()
                            .cloned()
                            .unwrap_or_default()
                    ),
                    source_offset: face.table.start(),
                    packed_hex: None,
                });
            }
        }
        let resolved = crate::appearance::resolve_display_appearances(
            ctx,
            scan,
            display,
            &display_faces,
            eligible_features,
        )?;
        for source in resolved.matched_feature_sources {
            matched_feature_sources.insert(source);
        }
        unresolved.extend(resolved.unresolved);
        let display_id = UnknownId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "displaylist", "record"),
            display.ordinal(),
        );
        let join_verdicts: Vec<crate::tessellation::PersistentJoinVerdict> = if display_faces
            .iter()
            .any(|face| !face.surface_references.is_empty())
        {
            let classes = crate::tessellation::class_intervals(display.payload());
            display_faces
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    crate::tessellation::persistent_join_verdict(
                        display.payload(),
                        &display_faces,
                        index,
                        &classes,
                    )
                })
                .collect()
        } else {
            Vec::new()
        };
        for (table_index, display_face) in display_faces.into_iter().enumerate() {
            let id = format!(
                "sldprt:displaylist:record#{}:{}",
                display.ordinal(),
                table_index
            );
            match join_verdicts.get(table_index) {
                Some(crate::tessellation::PersistentJoinVerdict::Admit(identity)) => {
                    persistent_face_bindings.push(crate::tessellation::PersistentFaceBinding {
                        tessellation: id.clone(),
                        identity: identity.clone(),
                    });
                }
                Some(crate::tessellation::PersistentJoinVerdict::Withhold(
                    crate::tessellation::PersistentJoinWithhold::AmbiguousReusedTag,
                )) => {
                    withheld_reused_tags.push(display_face.table.start());
                }
                Some(crate::tessellation::PersistentJoinVerdict::Withhold(_)) | None => {}
            }
            crate::annotations::note(
                annotations,
                id.as_str(),
                display_stream,
                u64_from_index(display_face.table.start()),
                "displaylist_tessellation",
                Exactness::ByteExact,
            )?;
            if let Some(resolved_definition) = resolved.by_face.get(&table_index) {
                display_appearance_ranks.insert(id.clone(), resolved_definition.rank);
                let definition = &resolved_definition.definition;
                let source_entity_id =
                    format!("{}::DisplayFace[{table_index}]", display_stream.as_str());
                let appearance = ensure_display_appearance(
                    ir,
                    definition,
                    display.ordinal(),
                    annotations,
                    ScopedAppearanceOrigin::DisplayList,
                )?;
                ir.model.appearance_bindings.push(AppearanceBinding {
                    native_ref: None,
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
                    channels: display_binding_channels(definition),
                });
            }
            let mesh = display_face.mesh;
            mesh_count += 1;
            let mut tessellation = mesh
                .into_tessellation(cadmpeg_ir::tessellation::TessellationId::mint(id).map_err(
                    |error| {
                        CodecError::malformed(format_args!("invalid display tessellation: {error}"))
                    },
                )?)
                .map_err(|error| {
                    CodecError::malformed(format_args!("invalid display tessellation: {error}"))
                })?;
            tessellation.native_ref = Some(display_id.as_str().to_owned());
            ir.model.tessellations.push(tessellation);
        }
        crate::annotations::note(
            annotations,
            display_id.as_str(),
            display_stream,
            0,
            "displaylist_tessellation",
            Exactness::Unknown,
        )?;
        unknowns.push(UnknownRecord::retained(
            display_id,
            0,
            retained_copy(ctx, display.payload(), "retain SLDPRT display section")?,
            Vec::new(),
        ));
    }
    Ok(DisplayPush {
        persistent_face_bindings,
        display_appearance_ranks,
        matched_feature_sources,
        conflicting_display_references,
        unresolved,
        feature_appearance_sources,
        mesh_count,
        withheld_reused_tags,
    })
}

fn build_geometry_ir(
    ctx: &DecodeContext<'_>,
    scan: &mut ContainerScan<'_>,
    classification: &crate::dialect::LayerClassification,
    decoded: DecodedBrep,
    form_padding: Option<usize>,
    admitted_entities: &mut u64,
) -> Result<BuiltDocument, CodecError> {
    let DecodedBrep {
        metadata_header,
        mut brep,
        configuration_bodies,
    } = decoded;
    let mut retention = retention::Retention::default();
    // These source records were withheld by the B-rep builder. The exact
    // ownership is not localized for every inverse-solve/topology branch, so
    // the enclosing source image owns the remaining model content.
    if brep.untyped_model
        || brep.stats.unknown_surface_faces != 0
        || brep.stats.unknown_procedural_supports != 0
        || brep.stats.unknown_curve_edges != 0
        || brep.stats.ambiguous_pcurve_parameters != 0
        || brep.stats.off_surface_nurbs_pcurves != 0
        || !brep.stats.surface_solver_capped.is_empty()
        || brep.stats.unresolved_face_colors != 0
        || brep.stats.unclaimed_faces != 0
    {
        retention.source(cadmpeg_ir::document::ArenaClass::Model);
    }
    let appearance_definitions = crate::appearance::definitions(ctx, scan)?;
    let (display_sections, _) = collect_display_sections(ctx, scan)?;
    let mut ir = CadIr::decoded(source_meta(
        scan,
        classification,
        metadata_header.as_ref(),
    )?);
    let mut annotations = std::mem::take(&mut brep.annotations);
    previews::project(ctx, scan, &mut ir, &mut annotations)?;
    let mut pmi_losses = Vec::new();
    let mut histories = crate::history::histories(ctx, scan, &mut annotations, &mut pmi_losses)?;
    let mut lanes =
        crate::resolved_features::assembly::lanes(ctx, scan, &mut annotations, &mut pmi_losses)?;
    let mut supplemental_config_lanes =
        crate::resolved_features::assembly::supplemental_config_lanes(ctx, scan, &mut annotations)?;
    crate::resolved_features::classes::bind_history_classes(&mut histories, &lanes);
    crate::resolved_features::bindings::bind_scalar_operands(ctx, &histories, &mut lanes)?;
    crate::resolved_features::bindings::bind_scalar_operands(
        ctx,
        &histories,
        &mut supplemental_config_lanes,
    )?;
    let mut untyped_pmi_sections = BTreeSet::new();
    let pmi_dimensions = crate::pmi::dimensions(
        ctx,
        scan,
        &mut annotations,
        &mut pmi_losses,
        &mut untyped_pmi_sections,
    )?;
    for id in untyped_pmi_sections {
        // A malformed or capped map's semantic payload was never admitted as
        // a PmiDimension record; retaining its section does not type it.
        retention.record(&id, cadmpeg_ir::document::ArenaClass::Model);
    }
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
        &mut ir.model.features,
        &histories,
        &lanes,
        form_padding,
    );
    let projected_pmi = crate::pmi::apply_to_parameters(
        ctx,
        &mut ir.model.parameters,
        &ir.model.features,
        &pmi_dimensions,
    )?;
    ir.model.pmi.extend(projected_pmi.annotations);
    pmi_losses.extend(projected_pmi.losses);
    crate::resolved_features::projections::bind_parameter_scalars(
        &mut ir.model.parameters,
        &ir.model.features,
        &histories,
        parameter_identity_lanes(&lanes),
    )?;
    crate::resolved_features::projections::synthesize_display_relation_parameters(
        &mut ir.model.parameters,
        &ir.model.features,
        lanes.iter().chain(supplemental_config_lanes.iter()),
    )?;
    crate::resolved_features::projections::type_display_relation_parameters(
        &mut ir.model.parameters,
        &ir.model.features,
        &lanes,
    )?;
    crate::history::configuration::align_configuration_parameter_kinds(&mut ir);
    let crate::resolved_features::sketch_projection::ProjectedSketches {
        mut sketches,
        entities: mut sketch_entities,
        constraints: mut sketch_constraints,
        untyped_lanes,
    } = crate::resolved_features::sketch_projection::sketches(
        ctx,
        scan,
        &mut annotations,
        &mut pmi_losses,
    )?;
    for lane in untyped_lanes {
        let id = crate::records::lane_payload::retained_id(&lane)?;
        retention.record(id.as_str(), cadmpeg_ir::document::ArenaClass::Design);
    }
    crate::resolved_features::profiles::bind_sketch_profiles(
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
    );
    crate::resolved_features::bindings::bind_unresolved_detached_sketch_objects(
        ctx,
        &ir.model.features,
        &histories,
        &mut supplemental_config_lanes,
    )?;
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
    lanes.extend(supplemental_config_lanes);
    let all_lanes = lanes;
    let lanes = &all_lanes[..base_lane_count];
    let sketch_lanes = all_lanes.as_slice();
    let (spatial_sketches, spatial_sketch_entities) =
        crate::resolved_features::markers::spatial_sketches(
            &mut ir.model.features,
            &histories,
            sketch_lanes,
        );
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
        &mut ir.model.features,
        &mut sketches,
        &mut sketch_entities,
        &histories,
        sketch_lanes,
    );
    crate::history::bind::bind_unique_sketch_feature(&mut ir.model.features, &sketches, &histories);
    crate::resolved_features::component_paths::project_dissected_sketches(
        &mut ir.model.features,
        &sketches,
        &histories,
    );
    crate::resolved_features::axes::bind_profile_revolution_axes(
        &mut ir.model.features,
        &histories,
        lanes,
        &sketches,
        &brep.surfaces,
    );
    crate::resolved_features::bindings::bind_pattern_inputs(
        &mut ir.model.features,
        &histories,
        lanes,
    )?;
    crate::resolved_features::bindings::bind_sweep_adjacent_profiles(
        &mut ir.model.features,
        &histories,
        lanes,
    );
    crate::resolved_features::dimensions::project_dimensioned_sketch_geometry(
        &mut sketch_entities,
        &sketches,
        &brep.surfaces,
        &ir.model.features,
        &ir.model.parameters,
        sketch_lanes,
    )?;
    crate::resolved_features::dimensions::project_marker_dimensioned_circles(
        &mut sketch_entities,
        &mut sketches,
        &ir.model.features,
        &ir.model.parameters,
        sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_relation_point_geometry(
        &mut sketch_entities,
        &sketches,
        &ir.model.features,
        sketch_lanes,
    );
    crate::resolved_features::dimensions::project_relation_point_dimensioned_circles(
        &mut sketch_entities,
        &ir.model.features,
        &ir.model.parameters,
        sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_relation_solved_line_geometry(
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
        &mut ir.model.spatial_sketch_constraints,
        &mut ir.model.spatial_sketch_entities,
        &ir.model.spatial_sketches,
        &ir.model.features,
        &ir.model.parameters,
        sketch_lanes,
    );
    let mut attributes = crate::metadata::attributes(ctx, scan, &mut annotations)?;
    let custom_properties = crate::history::project::custom_property_attributes(ctx, &histories)?;
    attributes.extend(custom_properties);
    ir.model.attributes = attributes;
    ir.model.attributes.extend(projected_pmi.attributes);
    ir.model.sketches = sketches;
    ir.model.sketch_entities = sketch_entities;
    ir.model.sketch_constraints = sketch_constraints;

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
        crate::swift::PrimaryTopology {
            bodies: &ir.model.bodies,
            faces: &ir.model.faces,
            edges: &ir.model.edges,
            vertices: &ir.model.vertices,
        },
        &face_bridge_sequences,
        &edge_use_sequences,
        &vertex_use_sequences,
    );
    let face_atoms = std::mem::take(&mut brep.face_atoms);
    let mut face_identities = Vec::new();
    for atom in face_atoms {
        face_identities.push((atom.face, atom.identity));
    }
    let mut face_producers = Vec::new();
    for (target, identity) in &face_identities {
        face_producers.push((
            target.as_str().to_owned(),
            identity.feature_source_id.value(),
        ));
    }
    for identity in &brep.native_face_identities {
        if identity.values.len() == 4 {
            face_producers.push((identity.target.as_str().to_owned(), identity.values[1]));
        }
    }
    let body_modifiers = std::mem::take(&mut brep.body_modifiers)
        .into_iter()
        .filter_map(|modifier| {
            modifier
                .target
                .map(|target| (target, modifier.feature_source_id.value()))
        })
        .collect::<Vec<_>>();
    crate::history::bind::derive_feature_outputs(
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
    );
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
        &ir.model.parameters,
    );
    crate::resolved_features::holes::project_generated_hole_axes(
        &mut ir.model.features,
        &histories,
        &all_lanes,
        &face_identities,
        &ir.model.faces,
        &ir.model.surfaces,
        &ir.model.parameters,
    );
    crate::resolved_features::holes::project_topological_hole_constructions(
        &mut ir.model.features,
        &crate::resolved_features::holes::HoleTopology {
            parameters: &ir.model.parameters,
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
            parameters: &ir.model.parameters,
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
            parameters: &ir.model.parameters,
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
    let active_block = active_parasolid_block(scan);
    assign_configuration_bodies(&mut ir, configuration_bodies, active_block.as_deref())?;
    let configuration_losses = crate::history::configuration::project_configuration_sketch_states(
        ctx,
        &mut ir,
        &histories,
        &all_lanes,
        &mut annotations,
    )?;
    pmi_losses.extend(configuration_losses);
    crate::history::configuration::bind_configuration_topology_selections(
        &mut ir,
        &histories,
        &all_lanes,
        &face_identities,
    )?;
    let active_block = active_parasolid_block(scan);
    mark_active_configuration(&mut ir, active_block.as_deref());
    crate::resolved_features::projections::project_unbound_cosmetic_thread_faces(
        ctx,
        &mut ir.model.features,
        &histories,
        &all_lanes,
        &ir.model.faces,
        &ir.model.surfaces,
        &ir.model.parameters,
    )?;
    crate::resolved_features::projections::project_unbound_offset_plane_faces(
        &mut ir.model.features,
        &ir.model.faces,
        &ir.model.surfaces,
    );
    crate::history::configuration::inherit_configuration_reference_plane_states(ctx, &mut ir)?;
    crate::history::bind::order_model_features_for_regeneration(ctx, &mut ir)?;
    let pattern_hole_nominals =
        crate::swift::pattern_hole_nominal_context(&ir.model.features, &ir.model.parameters);
    ir.model.pmi.extend(crate::swift::annotations(
        ctx,
        scan,
        &mut annotations,
        Some(&topology_index),
        Some(&pattern_hole_nominals),
        &mut pmi_losses,
    )?);
    input_topologies::project(&mut ir, &all_lanes, &mut pmi_losses);
    let native_appearance_plan = crate::appearance::visual_states::resolve(
        ctx,
        scan,
        crate::appearance::visual_states::Inputs {
            model: &ir.model,
            histories: &histories,
            lanes: &all_lanes,
            faces: &brep.native_face_identities,
            bodies: &brep.native_body_identities,
        },
    )?;
    retention.ambiguous_thin_features =
        crate::resolved_features::operations::ambiguous_thin_features(&histories, &all_lanes);
    let mut native = crate::native::SldprtNative {
        feature_histories: histories,
        feature_input_lanes: all_lanes,
        pmi_dimensions,
        document_metadata: Vec::new(),
        decode_baselines: Vec::new(),
    };
    assign_native_configuration_indices(&ir, &mut native);

    ctx.admit_entities(
        u64_from_index(ir.model.entity_count()),
        admitted_entities,
        "admit SLDPRT entities",
    )?;
    let mut unknowns = brep.unknowns;
    retain_lane_images(ctx, &native, &mut annotations, &mut unknowns)?;
    native.store(ctx, ir.native.namespace_mut("sldprt"))?;
    for owned_face_color in brep.face_colors {
        let annotation_source = &owned_face_color.source_stream;
        let site = owned_face_color.site_key.as_deref().or_else(|| {
            owned_face_color
                .value
                .target
                .as_deref()
                .and_then(|target| target.split_once('@').map(|(_, site)| site))
        });
        let qualified_site = site.map_or_else(String::new, |site| format!("@{site}"));
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
            id.as_str(),
            annotation_source,
            u64_from_index(face_color.offset),
            "00_53_color",
            Exactness::ByteExact,
        )?;
        if !ir
            .model
            .appearances
            .iter()
            .any(|appearance| appearance.id == id)
        {
            ir.model.appearances.push(Appearance {
                native_ref: None,
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
                ir.model.appearance_bindings.push(AppearanceBinding {
                    native_ref: None,
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
            id.as_str(),
            &definition.source_name,
            u64_from_index(definition.record_offset),
            "moVisualProperties_c",
            Exactness::ByteExact,
        )?;
        ir.model.appearances.push(Appearance {
            native_ref: None,
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
    let push = push_display_meshes(
        ctx,
        scan,
        display_sections,
        &mut ir,
        &mut annotations,
        &mut unknowns,
        &native_appearance_plan.feature_sources,
    )?;
    let mut matched_feature_sources = native_appearance_plan.feature_sources.clone();
    for source in &push.matched_feature_sources {
        matched_feature_sources.insert(*source);
    }
    let persistent_face_bindings = push.persistent_face_bindings;
    let display_appearance_ranks = push.display_appearance_ranks;
    let conflicting_display_references = push.conflicting_display_references;
    let feature_appearance_sources = push.feature_appearance_sources;
    for start in push.withheld_reused_tags {
        pmi_losses.push(SldprtLossCode::TessellationReusedTagAmbiguous.note(format!(
            "sldprt display-face table at byte {start} carries conflicting reused class tags; its persistent join is withheld."
        )));
    }
    for unresolved in push.unresolved {
        let hex = unresolved
            .packed_hex
            .as_deref()
            .map_or(String::new(), |hex| format!(" packed_hex={hex}"));
        let message = format!(
            "unresolved appearance binding: owner_class={} identity={} appearance_index={} reason={} source_offset={}{hex}",
            unresolved.owner_class,
            unresolved.identity,
            unresolved.appearance_index,
            unresolved.reason,
            unresolved.source_offset,
        );
        pmi_losses.push(SldprtLossCode::AppearanceAssignmentUnresolved.note(message));
        retain_unresolved_appearance(&mut ir, &unresolved);
    }
    for unresolved in &native_appearance_plan.unresolved_details {
        let hex = unresolved
            .packed_hex
            .as_deref()
            .map_or(String::new(), |hex| format!(" packed_hex={hex}"));
        let message = format!(
            "unresolved appearance binding: owner_class={} identity={} appearance_index={} reason={} source_offset={}{hex}",
            unresolved.owner_class,
            unresolved.identity,
            unresolved.appearance_index,
            unresolved.reason,
            unresolved.source_offset,
        );
        pmi_losses.push(SldprtLossCode::NativeAppearanceScopeUnresolved.note(message));
        retain_unresolved_appearance(&mut ir, unresolved);
    }
    if let Some(message) = appearance_assignment_loss_message(
        &feature_appearance_sources,
        &matched_feature_sources,
        &conflicting_display_references,
    ) {
        pmi_losses.push(SldprtLossCode::AppearanceAssignmentUnresolved.note(message));
    }
    let mut assigned_tessellations = crate::tessellation::assign_persistent_owners(
        &mut ir.model,
        &face_identities,
        &persistent_face_bindings,
    );
    let remaining_assignments =
        crate::tessellation::assign_unique_surface_owners(ctx, &mut ir.model, &mut pmi_losses)?;
    assigned_tessellations.extend(remaining_assignments);
    if native_appearance_plan.unresolved > 0 {
        pmi_losses.push(
            SldprtLossCode::NativeAppearanceScopeUnresolved.note(format!(
                "{} native appearance ownership or display-state relation(s) remain unresolved.",
                native_appearance_plan.unresolved
            )),
        );
    }
    let mut scoped_color_faces = Vec::new();
    for (face_index, (face_id, choice)) in native_appearance_plan.faces.iter().enumerate() {
        let Some(definition) = choice.definition.as_ref() else {
            continue;
        };
        let Some(face) = ir.model.faces.iter().find(|face| &face.id == face_id) else {
            continue;
        };
        if face.color.is_some() && choice.rank < 4 {
            continue;
        }
        if face.color.is_some_and(|color| color != definition.color) && choice.rank == 4 {
            pmi_losses.push(
                SldprtLossCode::NativeAppearanceScopeUnresolved
                    .note("Conflicting face appearances remain unresolved."),
            );
            continue;
        }
        let appearance = ensure_display_appearance(
            &mut ir,
            definition,
            choice.ordinal,
            &mut annotations,
            ScopedAppearanceOrigin::VisualState,
        )?;
        if let Some(face) = ir.model.faces.iter_mut().find(|face| &face.id == face_id) {
            face.color = Some(definition.color);
            scoped_color_faces.push(face_id.clone());
        }
        ir.model.appearance_bindings.push(AppearanceBinding {
            native_ref: None,
            id: cadmpeg_ir::ids::AppearanceBindingId::compose(
                &cadmpeg_ir::identity_namespace!("sldprt", "appearance", "native-binding"),
                cadmpeg_ir::ids::IdentityKey::from(choice.ordinal)
                    .colon(definition.record_offset)
                    .colon(face_index),
            ),
            target: AppearanceTarget::Face(face_id.clone()),
            appearance,
            source_entity_id: Some(format!(
                "{}:{}",
                definition.source_name.as_str(),
                definition.record_offset
            )),
            object_type: Some("Native scoped face".into()),
            visible: None,
            channels: display_binding_channels(definition),
        });
    }
    for tessellation in &ir.model.tessellations {
        let old_rank = display_appearance_ranks
            .get(tessellation.id.as_str())
            .copied()
            .unwrap_or(0);
        let blocked = native_appearance_plan.blocked_display_rank >= old_rank
            && native_appearance_plan.blocked_display_rank != 0
            || tessellation.faces.iter().any(|face| {
                native_appearance_plan
                    .faces
                    .get(face)
                    .is_some_and(|choice| choice.definition.is_none() && choice.rank >= old_rank)
            });
        if blocked {
            ir.model.appearance_bindings.retain(|binding| {
                binding.target != AppearanceTarget::Tessellation(tessellation.id.to_string())
            });
        }
    }
    let mut annotation_builder = AnnotationBuilder::resume(annotations);
    for id in scoped_color_faces {
        annotation_builder.field_exactness(id.as_str(), "color", Exactness::Derived)?;
    }
    for id in assigned_tessellations {
        annotation_builder.field_exactness(id.as_str(), "body", Exactness::Derived)?;
        annotation_builder.field_exactness(id.as_str(), "faces", Exactness::Derived)?;
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
            id.as_str(),
            source_block.section.source_stream(),
            u64_from_index(source_block.offset),
            source_block.family.label(),
            Exactness::ByteExact,
        )?;
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
            id.as_str(),
            &source_stream.path,
            0,
            container::payload_family(&source_stream.payload).label(),
            Exactness::ByteExact,
        )?;
        unknowns.push(UnknownRecord::retained(
            id,
            0,
            std::mem::take(&mut source_stream.payload),
            Vec::new(),
        ));
    }
    let mut opaque_links = BTreeMap::<&str, Vec<String>>::new();
    for surface in &ir.model.surfaces {
        if let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
            record: Some(record),
        }) = &surface.geometry
        {
            opaque_links
                .entry(record.as_str())
                .or_default()
                .push(surface.id.as_str().to_owned());
        }
    }
    for curve in &ir.model.curves {
        if let cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
            record: Some(record),
        }) = &curve.geometry
        {
            opaque_links
                .entry(record.as_str())
                .or_default()
                .push(curve.id.as_str().to_owned());
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
        source.links_mut().extend(links);
    }
    preserve_source_image(ctx, scan, &mut annotations, &mut unknowns)?;
    // Sort arenas for the order-sensitive loss scans that follow; the local
    // digests are stamped once, in `decode_result`, after native unknown
    // records are attached.
    ir.finalize();
    Ok((ir, annotations, unknowns, pmi_losses, retention))
}

fn assign_native_configuration_indices(ir: &CadIr, native: &mut crate::native::SldprtNative) {
    for configuration in &ir.model.configurations {
        let Some(native_ref) = configuration.native_ref.as_deref() else {
            continue;
        };
        for history in &mut native.feature_histories {
            let mut found = false;
            for record in &mut history.configurations {
                if record.id == native_ref {
                    record.source_index = configuration.source_index;
                    found = true;
                    break;
                }
            }
            if found {
                break;
            }
        }
    }
}

fn source_meta(
    scan: &ContainerScan,
    classification: &crate::dialect::LayerClassification,
    header: Option<&StreamHeader>,
) -> Result<SourceMeta, CodecError> {
    let mut attributes = BTreeMap::new();
    attributes.insert(
        cadmpeg_core::nonblank_literal!("outer_version"),
        format!("0x{:08x}", scan.version),
    );
    if container::select_active_parasolid_site(scan).is_none() {
        attributes.insert(
            cadmpeg_core::nonblank_literal!("sldprt_active_partition_unresolved"),
            "true".into(),
        );
    }
    if let Some(header) = header {
        attributes.insert(
            cadmpeg_core::nonblank_literal!("parasolid_schema"),
            header.schema.value().to_owned(),
        );
        attributes.insert(
            cadmpeg_core::nonblank_literal!("parasolid_description"),
            header.description.clone(),
        );
    }
    add_solidworks_xml_metadata(scan, &mut attributes)?;
    Ok(SourceMeta::classified(
        classification.layers().clone(),
        attributes,
    ))
}

fn add_solidworks_xml_metadata(
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
                attributes.insert(key, value.clone());
            }
        }
        if let Some(value) = active_configuration_name {
            attributes.insert(
                cadmpeg_core::nonblank_literal!("sw_configuration_name"),
                value.to_owned(),
            );
        } else if let Some(value) = &envelope.configuration_name {
            attributes.insert(
                cadmpeg_core::nonblank_literal!("sw_configuration_name"),
                value.clone(),
            );
        }
        for (key, value) in &envelope.configuration_attributes {
            let name = cadmpeg_core::text::NonBlankString::new(key.to_owned())
                .ok_or_else(|| CodecError::Malformed("invalid SLDPRT configuration key".into()))?;
            attributes.insert(name, value.clone());
        }
    }
    Ok(())
}

fn append_topology_record_losses(
    stats: &crate::brep::graph::Stats,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    if stats.unresolved_topology_vertices > 0 {
        losses.push(SldprtLossCode::TopologyVertexRecordUnresolved.note(format!(
            "{} referenced VERTEX record(s) lack a uniquely framed record with a verified owner class and IR-supported tolerance.",
            stats.unresolved_topology_vertices,
        )));
    }
    for (count, code, family) in [
        (
            stats.unresolved_topology_points,
            SldprtLossCode::GeometryPointRecordAmbiguous,
            "POINT",
        ),
        (
            stats.unresolved_topology_loops,
            SldprtLossCode::TopologyLoopRecordAmbiguous,
            "LOOP",
        ),
        (
            stats.unresolved_topology_fins,
            SldprtLossCode::TopologyFinRecordAmbiguous,
            "FIN",
        ),
    ] {
        if count > 0 {
            losses.push(code.note(format!(
                "{count} referenced {family} record(s) were withheld because complete records conflict and no current occurrence is selected."
            )));
        }
    }
    if stats.withheld_fin_partners > 0 {
        losses.push(SldprtLossCode::TopologyFinPartnerUnresolved.note(format!(
            "{} edge(s) were withheld because an unwalked boundary-dummy canonical FIN lacks reciprocal partner corroboration of both ring-order endpoints or a mutual twin FIN contradicts the ring-order endpoint; no dummy enters a public face loop.",
            stats.withheld_fin_partners,
        )));
    }
    if stats.withheld_fin_links > 0 {
        losses.push(SldprtLossCode::TopologyFinLinkUnresolved.note(format!(
            "{} ring(s) were withheld because a FIN backward link names no reciprocal same-loop forward link; the ring is not walked on doubt.",
            stats.withheld_fin_links,
        )));
    }
    if stats.withheld_ring_edges > 0 {
        losses.push(SldprtLossCode::TopologyRingEdgeWithheld.note(format!(
            "{} vertexless ring edge(s) fail strict reciprocal-circle single-loop admission; no vertex is invented and the edge is withheld.",
            stats.withheld_ring_edges,
        )));
    }
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
        losses.push(SldprtLossCode::GeometryFaceSupportSurfaceUntyped.note(message.join(" ")));
    }
    losses.append(&mut decoded.losses);
    if s.unknown_curve_edges > 0 {
        losses.push(
            SldprtLossCode::GeometryEdgeSupportCurveUntyped.note(format!(
                "{} edge(s) reference an untyped support curve; topology references an opaque \
                 curve carrier linked to the retained partition.",
                s.unknown_curve_edges
            )),
        );
    }
    if s.ambiguous_pcurve_parameters > 0 {
        losses.push(SldprtLossCode::GeometryPcurveAmbiguous.note(format!(
            "{} pcurve(s) were withheld because more than one geometric parameter satisfies the \
             stored edge or ruling geometry; the decoder does not choose by residual order.",
            s.ambiguous_pcurve_parameters
        )));
    }
    if !s.surface_solver_capped.is_empty() {
        losses.push(
            SldprtLossCode::GeometrySurfaceSolverWorkCapped.note(format!(
                "{} coedge pcurve(s) were skipped because a NURBS surface solve exceeded the local \
             work cap SURFACE_SOLVER_LOCAL_WORK ({}): {}.",
                s.surface_solver_capped.count(),
                crate::brep::evaluation::SURFACE_SOLVER_LOCAL_WORK,
                s.surface_solver_capped,
            )),
        );
    }
    if s.off_surface_nurbs_pcurves > 0 {
        losses.push(SldprtLossCode::TopologyPcurveCarrierOffSurface.note(format!(
            "{} NURBS edge carrier(s) have vertex ranges off their bound B-spline surface; pcurve derivation is withheld because the defect is upstream of parameter-space geometry.",
            s.off_surface_nurbs_pcurves
        )));
    }
    append_topology_record_losses(s, &mut losses);
    if s.unresolved_face_colors > 0 {
        losses.push(SldprtLossCode::AppearanceFaceColorUnresolved.note(format!(
            "{} face-color binding(s) were withheld because the current face and link records do not select one consistent framed color record.",
            s.unresolved_face_colors
        )));
    }
    if s.unclaimed_faces > 0 {
        losses.push(SldprtLossCode::TopologyFaceUnclaimed.note(format!(
            "{} canonical face(s) are not claimed by an explicit body relation; the decoder withholds them rather than inventing body membership.",
            s.unclaimed_faces
        )));
    }
    if s.synthetic_body_grouping {
        losses.push(
            SldprtLossCode::TopologyBodyHierarchyDerived.note(
                "No body record was available; one body/region/shell hierarchy was derived."
                    .to_string(),
            ),
        );
    }
    append_parasolid_probe_losses(scan, &mut losses);
    append_swift_pmi_losses(ctx, scan, &mut losses)?;
    classification.append_losses(&mut losses);
    Ok(DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer {
            scope: ContentScope::full(),
            geometry_transferred: true,
        },
        coverage: crate::archive::coverage(scan, ctx)?,
        losses,
        notes,
    })
}

fn build_metadata_ir(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    classification: &crate::dialect::LayerClassification,
    form_padding: Option<usize>,
    admitted_entities: &mut u64,
    transfer_display: bool,
) -> Result<BuiltDocument, CodecError> {
    let mut ir = CadIr::empty();
    let mut retention = retention::Retention::default();
    for section in scan.sections() {
        if matches!(
            container::payload_family(section.payload()),
            container::PayloadFamily::PngPreview | container::PayloadFamily::BmpThumbnail
        ) {
            // This route does not run the preview projection and has no typed
            // native preview arena; the source image still owns that resource.
            retention.source(cadmpeg_ir::document::ArenaClass::Model);
        }
    }
    let mut unknowns = Vec::new();
    let mut annotations = Annotations::default();
    let mut pmi_losses = Vec::new();
    let mut histories = crate::history::histories(ctx, scan, &mut annotations, &mut pmi_losses)?;
    let mut lanes =
        crate::resolved_features::assembly::lanes(ctx, scan, &mut annotations, &mut pmi_losses)?;
    let mut supplemental_config_lanes =
        crate::resolved_features::assembly::supplemental_config_lanes(ctx, scan, &mut annotations)?;
    crate::resolved_features::classes::bind_history_classes(&mut histories, &lanes);
    crate::resolved_features::bindings::bind_scalar_operands(ctx, &histories, &mut lanes)?;
    crate::resolved_features::bindings::bind_scalar_operands(
        ctx,
        &histories,
        &mut supplemental_config_lanes,
    )?;
    let mut untyped_pmi_sections = BTreeSet::new();
    let pmi_dimensions = crate::pmi::dimensions(
        ctx,
        scan,
        &mut annotations,
        &mut pmi_losses,
        &mut untyped_pmi_sections,
    )?;
    for id in untyped_pmi_sections {
        // A malformed or capped map's semantic payload was never admitted as
        // a PmiDimension record; retaining its section does not type it.
        retention.record(&id, cadmpeg_ir::document::ArenaClass::Model);
    }
    ir.model.pmi =
        crate::swift::annotations(ctx, scan, &mut annotations, None, None, &mut pmi_losses)?;
    let crate::resolved_features::sketch_projection::ProjectedSketches {
        sketches,
        entities: sketch_entities,
        constraints: sketch_constraints,
        untyped_lanes,
    } = crate::resolved_features::sketch_projection::sketches(
        ctx,
        scan,
        &mut annotations,
        &mut pmi_losses,
    )?;
    for lane in untyped_lanes {
        let id = crate::records::lane_payload::retained_id(&lane)?;
        retention.record(id.as_str(), cadmpeg_ir::document::ArenaClass::Design);
    }
    let mut model_attributes = crate::metadata::attributes(ctx, scan, &mut annotations)?;
    let custom_properties = crate::history::project::custom_property_attributes(ctx, &histories)?;
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
    add_solidworks_xml_metadata(scan, &mut attributes)?;

    if let Some(site) = container::select_active_parasolid_site(scan) {
        let id = site.section.native_id();
        // The metadata route deliberately retains this body stream without
        // projecting its geometry into any typed arena.
        retention.record(id.as_str(), cadmpeg_ir::document::ArenaClass::Model);
        let offset = match site.section {
            container::Section::Block(block) => u64_from_index(block.offset),
            container::Section::Compound(_) => 0,
        };
        attributes.insert(
            cadmpeg_core::nonblank_literal!("parasolid_schema"),
            site.header.schema.value().to_owned(),
        );
        crate::annotations::note(
            &mut annotations,
            id.as_str(),
            site.source_stream(),
            0,
            "parasolid_stream",
            Exactness::Unknown,
        )?;
        unknowns.push(UnknownRecord::retained(
            id,
            offset,
            retained_copy(ctx, site.payload, "retain SLDPRT active site")?,
            Vec::new(),
        ));
    } else if scan.sections().any(|section| {
        section
            .ps_streams()
            .iter()
            .any(|stream| crate::parasolid::is_body_stream(&stream.header))
    }) {
        // Body streams without one active site leave the model untyped; the
        // source image still owns them.
        retention.source(cadmpeg_ir::document::ArenaClass::Model);
    }

    ir.source = Some(SourceMeta::classified(
        classification.layers().clone(),
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
        &mut ir.model.features,
        &histories,
        &lanes,
        form_padding,
    );

    crate::resolved_features::projections::bind_parameter_scalars(
        &mut ir.model.parameters,
        &ir.model.features,
        &histories,
        parameter_identity_lanes(&lanes),
    )?;
    crate::resolved_features::projections::synthesize_display_relation_parameters(
        &mut ir.model.parameters,
        &ir.model.features,
        lanes.iter().chain(supplemental_config_lanes.iter()),
    )?;
    crate::resolved_features::projections::type_display_relation_parameters(
        &mut ir.model.parameters,
        &ir.model.features,
        &lanes,
    )?;
    crate::history::configuration::align_configuration_parameter_kinds(&mut ir);
    crate::resolved_features::profiles::bind_sketch_profiles(
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
    );
    crate::resolved_features::bindings::bind_unresolved_detached_sketch_objects(
        ctx,
        &ir.model.features,
        &histories,
        &mut supplemental_config_lanes,
    )?;
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
    lanes.extend(supplemental_config_lanes);
    let all_lanes = lanes;
    let lanes = &all_lanes[..base_lane_count];
    let sketch_lanes = all_lanes.as_slice();
    let (spatial_sketches, spatial_sketch_entities) =
        crate::resolved_features::markers::spatial_sketches(
            &mut ir.model.features,
            &histories,
            sketch_lanes,
        );
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
        &mut ir.model.features,
        &mut ir.model.sketches,
        &mut ir.model.sketch_entities,
        &histories,
        sketch_lanes,
    );
    crate::history::bind::bind_unique_sketch_feature(
        &mut ir.model.features,
        &ir.model.sketches,
        &histories,
    );
    crate::resolved_features::component_paths::project_dissected_sketches(
        &mut ir.model.features,
        &ir.model.sketches,
        &histories,
    );
    crate::resolved_features::axes::bind_profile_revolution_axes(
        &mut ir.model.features,
        &histories,
        lanes,
        &ir.model.sketches,
        &ir.model.surfaces,
    );
    crate::resolved_features::bindings::bind_pattern_inputs(
        &mut ir.model.features,
        &histories,
        lanes,
    )?;
    crate::resolved_features::bindings::bind_sweep_adjacent_profiles(
        &mut ir.model.features,
        &histories,
        lanes,
    );
    crate::resolved_features::dimensions::project_dimensioned_sketch_geometry(
        &mut ir.model.sketch_entities,
        &ir.model.sketches,
        &ir.model.surfaces,
        &ir.model.features,
        &ir.model.parameters,
        sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_spatial_relation_bindings(
        &mut ir.model.spatial_sketch_constraints,
        &mut ir.model.spatial_sketch_entities,
        &ir.model.spatial_sketches,
        &ir.model.features,
        &ir.model.parameters,
        sketch_lanes,
    );
    crate::resolved_features::relation_geometry::project_relation_point_geometry(
        &mut ir.model.sketch_entities,
        &ir.model.sketches,
        &ir.model.features,
        sketch_lanes,
    );
    crate::resolved_features::dimensions::project_relation_point_dimensioned_circles(
        &mut ir.model.sketch_entities,
        &ir.model.features,
        &ir.model.parameters,
        sketch_lanes,
    )?;
    crate::resolved_features::relation_geometry::project_relation_solved_line_geometry(
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
        &mut ir.model.features,
        &ir.model.sketches,
        &ir.model.sketch_entities,
        &histories,
        lanes,
    );
    crate::resolved_features::holes::project_spatial_hole_position_sketches(
        &mut ir.model.features,
        &ir.model.spatial_sketches,
        &ir.model.spatial_sketch_entities,
        &ir.model.surfaces,
        &histories,
        lanes,
        &ir.model.parameters,
    );
    crate::resolved_features::holes::project_topological_hole_constructions(
        &mut ir.model.features,
        &crate::resolved_features::holes::HoleTopology {
            parameters: &ir.model.parameters,
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
            parameters: &ir.model.parameters,
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
            parameters: &ir.model.parameters,
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
        lanes,
    );
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
        &ir.model.parameters,
    )?;
    crate::resolved_features::projections::project_unbound_offset_plane_faces(
        &mut ir.model.features,
        &ir.model.faces,
        &ir.model.surfaces,
    );
    crate::history::bind::order_features_for_regeneration(ctx, &mut ir.model.features)?;
    let configuration_losses = crate::history::configuration::project_configuration_sketch_states(
        ctx,
        &mut ir,
        &histories,
        lanes,
        &mut annotations,
    )?;
    pmi_losses.extend(configuration_losses);
    crate::history::configuration::inherit_configuration_reference_plane_states(ctx, &mut ir)?;
    crate::history::bind::order_model_features_for_regeneration(ctx, &mut ir)?;
    input_topologies::project(&mut ir, &all_lanes, &mut pmi_losses);
    retention.ambiguous_thin_features =
        crate::resolved_features::operations::ambiguous_thin_features(&histories, &all_lanes);
    if transfer_display {
        let (display_sections, _) = collect_display_sections(ctx, scan)?;
        let eligible = BTreeSet::new();
        let push = push_display_meshes(
            ctx,
            scan,
            display_sections,
            &mut ir,
            &mut annotations,
            &mut unknowns,
            &eligible,
        )?;
        for unresolved in push.unresolved {
            let hex = unresolved
                .packed_hex
                .as_deref()
                .map_or(String::new(), |hex| format!(" packed_hex={hex}"));
            let message = format!(
                "unresolved appearance binding: owner_class={} identity={} appearance_index={} reason={} source_offset={}{hex}",
                unresolved.owner_class,
                unresolved.identity,
                unresolved.appearance_index,
                unresolved.reason,
                unresolved.source_offset,
            );
            pmi_losses.push(SldprtLossCode::AppearanceAssignmentUnresolved.note(message));
            retain_unresolved_appearance(&mut ir, &unresolved);
        }
        let _assigned = crate::tessellation::assign_persistent_owners(
            &mut ir.model,
            &[],
            &push.persistent_face_bindings,
        );
        let mut owner_losses = Vec::new();
        let _assigned_unique = crate::tessellation::assign_unique_surface_owners(
            ctx,
            &mut ir.model,
            &mut owner_losses,
        )?;
        pmi_losses.append(&mut owner_losses);
        if push.mesh_count > 0 {
            pmi_losses.push(
                SldprtLossCode::TessellationDisplayWithoutGeometry.note(format!(
                    "{} DisplayLists tessellation mesh(es) transferred without B-rep geometry; ownership is unresolved and geometry_transferred is false.",
                    push.mesh_count
                )),
            );
        }
    }
    let mut native = crate::native::SldprtNative {
        feature_histories: histories,
        feature_input_lanes: all_lanes,
        pmi_dimensions,
        document_metadata: Vec::new(),
        decode_baselines: Vec::new(),
    };
    ctx.admit_entities(
        u64_from_index(ir.model.entity_count()),
        admitted_entities,
        "admit SLDPRT entities",
    )?;
    retain_lane_images(ctx, &native, &mut annotations, &mut unknowns)?;
    native.store(ctx, ir.native.namespace_mut("sldprt"))?;
    let active_block = active_parasolid_block(scan);
    bind_active_configuration_partition(&mut ir, active_block.as_deref());
    mark_active_configuration(&mut ir, active_block.as_deref());
    preserve_source_image(ctx, scan, &mut annotations, &mut unknowns)?;
    let metadata = crate::native::DocumentMetadata {
        id: cadmpeg_core::nonblank_literal!("sldprt:metadata:document#0"),
        attributes: std::mem::take(&mut ir.model.attributes),
        pmi: std::mem::take(&mut ir.model.pmi),
        semantic_annotations: std::mem::take(&mut ir.model.semantic_annotations),
    };
    if !metadata.attributes.is_empty()
        || !metadata.pmi.is_empty()
        || !metadata.semantic_annotations.is_empty()
    {
        native.document_metadata.push(metadata);
        native.store(ctx, ir.native.namespace_mut("sldprt"))?;
    }
    ir.scope = ContentScope {
        model: ir
            .model
            .populated_arena(cadmpeg_ir::document::ArenaClass::Model)
            .is_some(),
        design: ir
            .model
            .populated_arena(cadmpeg_ir::document::ArenaClass::Design)
            .is_some(),
    };
    // Sort arenas for the order-sensitive loss scans that follow; the local
    // digests are stamped once, in `decode_result`, after native unknown
    // records are attached.
    ir.finalize();
    Ok((ir, annotations, unknowns, pmi_losses, retention))
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
    let scene_feature_classes = crate::tessellation::scene_feature_classes(scan);
    crate::history::enrich_scene_classes(&mut semantic_projection, &scene_feature_classes);
    crate::history::configuration::enrich_history_semantic(
        ctx,
        &mut semantic_projection,
        lanes,
        pmi_dimensions,
        crate::history::configuration::HistoryEnrichment::Read,
    )?;
    ir.model.semantic_annotations =
        crate::history::project::project_semantic_notes(&semantic_projection)?;
    crate::history::project::project_feature_model(ctx, &semantic_projection)?
        .install(&mut ir.model, losses)?;
    crate::resolved_features::bindings::bind_pattern_inputs(
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
        crate::history::project::project_configurations(&semantic_projection)?;
    let mut parameter_projection = histories.to_vec();
    crate::resolved_features::direct_edits::enrich_history_move_face_translations(
        &mut parameter_projection,
        lanes,
    );
    crate::history::configuration::enrich_history_parameters_values_only(
        &mut parameter_projection,
        lanes,
    );
    crate::resolved_features::holes::
        enrich_history_cosmetic_thread_diameters_without_hole_constructions(&mut parameter_projection, lanes);
    crate::pmi::enrich_history_parameters(&mut parameter_projection, pmi_dimensions);
    ir.model.parameters =
        crate::history::parameters::project_parameters(ctx, &parameter_projection)?;
    let unprojected = crate::history::configuration::project_configuration_design_states(
        ctx,
        ir,
        histories,
        lanes,
        pmi_dimensions,
        form_padding,
    )?;
    if unprojected > 0 {
        losses.push(
            crate::history::configuration::configuration_projection_capped_loss(
                unprojected,
                crate::history::configuration::configuration_projection_work_limit(
                    ir.model.configurations.len(),
                    histories,
                ),
            ),
        );
    }

    Ok(())
}

fn parameter_identity_lanes(
    lanes: &[crate::records::FeatureInputLane],
) -> Vec<&crate::records::FeatureInputLane> {
    let eligible = || {
        lanes
            .iter()
            .filter(|lane| !crate::resolved_features::assembly::is_supplemental_config_lane(lane))
    };
    let has_global = eligible().any(|lane| lane.configuration.is_none());
    let single_scoped = !has_global && eligible().count() == 1;
    let mut selected = Vec::new();
    for lane in eligible() {
        if (has_global && lane.configuration.is_none())
            || (single_scoped && lane.configuration.is_some())
        {
            selected.push(lane);
        }
    }
    selected
}

fn active_parasolid_block(scan: &ContainerScan) -> Option<String> {
    container::select_active_parasolid_site(scan)
        .map(|site| site.source_stream().as_str().to_owned())
}

fn mark_active_configuration(ir: &mut CadIr, active_block: Option<&str>) {
    let active_name = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sw_configuration_name"))
        .map(String::as_str);
    let active_index = active_block
        .and_then(crate::container::configuration_index);
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
    // SolidWorks exposes this flag per configuration rather than per feature.
    // A positive flag therefore conservatively marks every current feature as
    // possibly stale; no numerical agreement with the B-rep clears that state.
    let active_slot = selected
        .and_then(|position| ir.model.configurations.get(position))
        .map(|configuration| {
            configuration
                .properties
                .get("id")
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(configuration.ordinal)
        })
        .or_else(|| {
            let source = ir.source.as_ref()?;
            let mut slots = source
                .attributes
                .iter()
                .filter(|(_, value)| value.eq_ignore_ascii_case("YES"))
                .filter_map(|(key, _)| {
                    key.as_str()
                        .strip_prefix("sw_configuration_")?
                        .strip_suffix("_most_recent")?
                        .parse::<u32>()
                        .ok()
                });
            let slot = slots.next()?;
            slots.next().is_none().then_some(slot)
        });
    let needs_update = active_slot
        .and_then(|slot| {
            ir.source
                .as_ref()?
                .attributes
                .get(format!("sw_configuration_{slot}_needs_update").as_str())
        })
        .is_some_and(|value| value.eq_ignore_ascii_case("YES"));
    if needs_update {
        for feature in &mut ir.model.features {
            feature.needs_recompute = true;
        }
    }
}

fn assign_configuration_bodies(
    ir: &mut CadIr,
    configuration_bodies: Vec<(usize, Vec<cadmpeg_ir::ids::BodyId>)>,
    active_block: Option<&str>,
) -> Result<(), CodecError> {
    let mut partition_map = BTreeMap::<u32, Vec<cadmpeg_ir::ids::BodyId>>::new();
    for (index, bodies) in configuration_bodies {
        let Ok(index) = u32::try_from(index) else {
            continue;
        };
        let merged = partition_map.entry(index).or_default();
        for body in bodies {
            if !merged.contains(&body) {
                merged.push(body);
            }
        }
    }

    let mut source_counts = BTreeMap::<u32, usize>::new();
    for source_index in ir
        .model
        .configurations
        .iter()
        .filter_map(|configuration| configuration.source_index)
    {
        if let Some(count) = source_counts.get_mut(&source_index) {
            *count += 1;
        } else {
            source_counts.insert(source_index, 1);
        }
    }
    for configuration in &mut ir.model.configurations {
        let Some(source_index) = configuration.source_index else {
            continue;
        };
        if source_counts.get(&source_index) == Some(&1) {
            // A native record without a decoded partition keeps its body
            // membership unresolved instead of resolving to an empty set.
            if let Some(bodies) = partition_map.remove(&source_index) {
                configuration.bodies =
                    Some(cadmpeg_ir::features::DistinctMembers::try_from(bodies)?);
            }
        }
    }
    if let Some((active_index, position)) = bind_active_configuration_partition(ir, active_block) {
        if let Some(bodies) = partition_map.remove(&active_index) {
            ir.model.configurations[position].bodies =
                Some(cadmpeg_ir::features::DistinctMembers::try_from(bodies)?);
        }
    }
    for (source_index, bodies) in partition_map {
        let ordinal = match ir
            .model
            .configurations
            .iter()
            .map(|configuration| configuration.ordinal)
            .max()
        {
            Some(highest) => highest.checked_add(1).ok_or_else(|| {
                CodecError::malformed("SLDPRT partition configuration ordinal overflows")
            })?,
            None => 0,
        };
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
                bodies: Some(cadmpeg_ir::features::DistinctMembers::try_from(bodies)?),
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
fn bind_active_configuration_partition(ir: &mut CadIr, active_block: Option<&str>) -> Option<(u32, usize)> {
    let active_name = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sw_configuration_name"));
    let active_index = active_block
        .and_then(crate::container::configuration_index)
        .and_then(|index| u32::try_from(index).ok());
    let (Some(active_name), Some(active_index)) = (active_name, active_index) else {
        return None;
    };
    let mut matches = ir
        .model
        .configurations
        .iter()
        .enumerate()
        .filter(|(_, configuration)| configuration.name.as_deref() == Some(active_name.as_str()))
        .map(|(position, _)| position);
    let position = matches.next()?;
    let source_identity_available = !ir
        .model
        .configurations
        .iter()
        .any(|configuration| configuration.source_index == Some(active_index));
    if matches.next().is_some()
        || ir.model.configurations[position].source_index.is_some()
        || !source_identity_available
    {
        return None;
    }

    // The container's active block and the native configuration name
    // establish the partition identity even when that block yielded no
    // decoded body list. Body membership remains unresolved until a decoded
    // partition supplies its body identities.
    ir.model.configurations[position].source_index = Some(active_index);
    Some((active_index, position))
}

/// Fingerprint current document state for byte-layout eligibility only.
fn stamp_local_digests(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    fidelity: &mut cadmpeg_ir::SourceFidelity,
) -> Result<(), CodecError> {
    ir.finalize();
    if let Some(source) = ir.source.as_mut() {
        for key in ["block_count", "compound_stream_count"] {
            source.attributes.remove(key);
        }
    }
    let brep = brep_local_sha256_in_place(ctx, ir)?;
    let (swobjects, material, identities) = match (|| -> Result<_, CodecError> {
        Ok((
            crate::writer::swobjects_local_sha256(ctx, ir)?,
            crate::writer::swobjects_material_local_sha256(ctx, ir)?,
            crate::writer::swobjects_metadata_identity_local_sha256(ctx, ir)?,
        ))
    })() {
        Ok((objects, material, identities)) => (
            Some(objects),
            Some(material),
            Some(identities),
        ),
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        // A source-layout optimization is optional when a current attribute
        // cannot be serialized by the source's metadata grammar.
        Err(_) => (None, None, None),
    };
    ir.native.namespace_mut("sldprt").set_arena(
        ctx,
        "decode_baselines",
        &[crate::native::DecodeBaselines {
            id: cadmpeg_core::nonblank_literal!("sldprt:metadata:decode-baselines#0"),
            brep,
            swobjects,
            swobjects_material: material,
            swobjects_metadata_identity: identities,
        }],
    )?;
    fidelity.decode_digest = Some(
        cadmpeg_ir::hash::digest::Sha256Digest::try_from(document_local_sha256(ir)?)
            .map_err(CodecError::malformed)?,
    );
    Ok(())
}

fn retain_lane_images(
    ctx: &DecodeContext<'_>,
    native: &crate::native::SldprtNative,
    annotations: &mut Annotations,
    unknowns: &mut Vec<UnknownRecord>,
) -> Result<(), CodecError> {
    for lane in &native.feature_input_lanes {
        if lane.native_payload.is_empty() {
            continue;
        }
        let id = crate::records::lane_payload::retained_id(&lane.id)?;
        crate::annotations::note(
            annotations,
            id.as_str(),
            &cadmpeg_ir::stream_name!("ResolvedFeatures"),
            0,
            "feature_input_layout",
            Exactness::ByteExact,
        )?;
        unknowns.push(UnknownRecord::retained(
            id,
            0,
            retained_copy(
                ctx,
                &lane.native_payload,
                "retain SLDPRT feature-input lane",
            )?,
            Vec::new(),
        ));
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
    brep_partition_sha256(&ctx, ir.tolerances, partition).0
}

/// [`brep_local_sha256`] without the deep clone, for the decode stamp path.
///
/// Moves the structurally untouched B-rep arenas out of `ir`, hashes the same
/// normalized partition [`brep_local_sha256`] builds, and moves them back in
/// their original order. The two arenas the normalization filters —
/// `appearances` and `appearance_bindings` — are moved into admitted partitions. Body display fields
/// move into a vector and back, so `ir` is bit-identical afterwards and both entry
/// points produce the same digest for the same document.
fn brep_local_sha256_in_place(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<String, CodecError> {
    use std::mem::take;

    let mut appearance_partition =
        digest_partition::DigestPartition::split(&mut ir.model.appearances, |appearance| {
            ir.model.appearance_bindings.iter().any(|binding| {
                matches!(binding.target, AppearanceTarget::Face(_))
                    && binding.appearance == appearance.id
            })
        });
    let mut binding_partition =
        digest_partition::DigestPartition::split(&mut ir.model.appearance_bindings, |binding| {
            matches!(binding.target, AppearanceTarget::Face(_))
        });
    let mut saved_body_display = Vec::new();
    for body in &mut ir.model.bodies {
        saved_body_display.push((take(&mut body.name), body.color));
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

/// Normalize and hash one B-rep partition; both digest entry points share it.
///
/// Returns the normalized model with the hash so an in-place caller can move
/// its arenas back; only `bodies` (display fields stripped), `appearances`,
/// and `appearance_bindings` (both filtered to face bindings) are mutated.
fn brep_partition_sha256(
    ctx: &DecodeContext<'_>,
    tolerances: cadmpeg_ir::units::Tolerances,
    model: cadmpeg_ir::document::Model,
) -> (Result<String, CodecError>, cadmpeg_ir::document::Model) {
    use cadmpeg_ir::appearance::AppearanceTarget;

    let mut normalized = CadIr::empty();
    normalized.tolerances = tolerances;
    normalized.model = model;
    normalized.model.bodies.iter_mut().for_each(|body| {
        body.name = None;
        body.color = None;
    });
    normalized
        .model
        .appearance_bindings
        .retain(|binding| matches!(binding.target, AppearanceTarget::Face(_)));
    let bindings = &normalized.model.appearance_bindings;
    normalized.model.appearances.retain(|appearance| {
        bindings
            .iter()
            .any(|binding| binding.appearance == appearance.id)
    });
    (
        cadmpeg_ir::hash::canonical_json_sha256(ctx, &normalized).map_err(Into::into),
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
        annotations,
        "sldprt:file:source-image#0",
        &cadmpeg_ir::stream_name!("source"),
        0,
        "source_image",
        Exactness::ByteExact,
    )?;
    unknowns.push(UnknownRecord::retained(
        UnknownId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "file", "source-image"),
            cadmpeg_ir::identity_key!("0"),
        ),
        0,
        retained_copy(ctx, scan.source_image, "retain SLDPRT source image")?,
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
    display_meshes: usize,
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
        if display_meshes > 0 {
            SldprtLossCode::MaterialMetadataNotTransferred.note(format!(
                "Body-bound appearances were not transferred because no body graph exists; \
                 {display_meshes} DisplayLists mesh(es) retained without owners."
            ))
        } else {
            SldprtLossCode::MaterialMetadataNotTransferred.note(
                "Body-bound appearances and tessellation were not transferred because no body graph \
                 exists."
                    .to_string(),
            )
        },
    ];

    if !container::has_parasolid_body_stream(scan) {
        losses.push(
            SldprtLossCode::ContainerNoParasolidStream.note(
                "no Parasolid partition/deltas stream was located in the container".to_string(),
            ),
        );
    }
    append_parasolid_probe_losses(scan, &mut losses);
    append_swift_pmi_losses(ctx, scan, &mut losses)?;
    classification.append_losses(&mut losses);

    Ok(DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer {
            scope: ContentScope::full(),
            geometry_transferred: false,
        },
        coverage: crate::archive::coverage(scan, ctx)?,
        losses,
        notes,
    })
}

fn append_parasolid_probe_losses(
    scan: &ContainerScan<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    let mut capped = crate::loss::NamedCount::default();
    for section in scan.sections().filter(|section| section.ps_probe_capped()) {
        capped.push(section.source_stream().as_str());
    }
    if capped.is_empty() {
        return;
    }
    losses.push(SldprtLossCode::ContainerParasolidProbeCapped.note(format!(
        "{} section(s) stopped the legacy Parasolid zlib probe at PROBE_WORK_PER_PAYLOAD_BYTE \
         ({}) times the payload length; later zlib members in those sections were not tried: \
         {capped}.",
        capped.count(),
        crate::parasolid::PROBE_WORK_PER_PAYLOAD_BYTE,
    )));
}

fn append_swift_pmi_losses(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    let unsupported = crate::swift::unsupported_annotation_classes(ctx, scan)?;
    if unsupported.is_empty() {
        return Ok(());
    }
    let count = unsupported.values().sum::<usize>();
    let classes = unsupported
        .iter()
        .map(|(class, class_count)| format!("{class} ({class_count})"))
        .collect::<Vec<_>>()
        .join(", ");
    let message =
        format!("{count} SWIFT semantic annotation(s) have no neutral PMI definition: {classes}.");
    losses.push(SldprtLossCode::PmiSwiftAnnotationUnsupported.note(message));
    Ok(())
}

#[cfg(test)]
mod tests;
