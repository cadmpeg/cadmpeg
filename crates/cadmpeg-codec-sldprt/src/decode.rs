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
//! The returned [`DecodeResult`] contains both the IR and its diagnostics.
//! Untyped surface and curve carriers become opaque geometry linked to the
//! retained Parasolid source record. If no body stream yields geometry, decoding returns a
//! metadata-only IR and blocking loss notes. [`DecodeOptions::container_only`]
//! requests the metadata-only path.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{AppearanceId, UnknownId};
use cadmpeg_ir::report::DecodeReport;

use crate::loss::SldprtLossCode;
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::Exactness;

use crate::container::configuration_index;

use crate::brep::{self, Brep};
use crate::container::{self, Block, CompoundStream, ContainerScan};
use crate::parasolid::StreamHeader;

struct BodyStream<'a> {
    origin: BodyOrigin<'a>,
    payload: &'a [u8],
    header: StreamHeader,
}

#[derive(Clone, Copy)]
enum BodyOrigin<'a> {
    Block(&'a Block),
    Compound(&'a CompoundStream),
}

impl BodyOrigin<'_> {
    fn name(self) -> String {
        match self {
            Self::Block(block) => block
                .section
                .clone()
                .unwrap_or_else(|| format!("block@{}", block.offset)),
            Self::Compound(stream) => stream.path.clone(),
        }
    }

    fn unknown_id(self) -> UnknownId {
        match self {
            Self::Block(block) => UnknownId(format!("sldprt:file:block#{}", block.offset)),
            Self::Compound(stream) => UnknownId(format!(
                "sldprt:file:compound-stream#{}",
                stream.directory_id
            )),
        }
    }

    fn site_key(self) -> String {
        match self {
            Self::Block(block) => format!("block@{}", block.offset),
            Self::Compound(stream) => format!("compound@{}", stream.directory_id),
        }
    }
}

struct DecodedBrep {
    /// Representative stream whose header is common to every merged site.
    /// This can be present for an unresolved merge without selecting a site.
    metadata_stream: Option<usize>,
    brep: Brep,
    configuration_bodies: Vec<(usize, Vec<cadmpeg_ir::ids::BodyId>)>,
}

struct EvaluatedFeatureState<'a> {
    feature: &'a cadmpeg_ir::features::Feature,
    dependencies: &'a [cadmpeg_ir::features::FeatureId],
    outputs: &'a [cadmpeg_ir::ids::BodyId],
    definition: &'a cadmpeg_ir::features::FeatureDefinition,
}

/// Decode one seekable `.sldprt` stream into IR and diagnostics.
///
/// The function reads and retains the complete source image. Container framing
/// or I/O failures return [`CodecError`]; unsupported model records are reported
/// through [`DecodeResult::report`] when a partial result can be represented.
pub fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<DecodeResult, CodecError> {
    let scan = container::scan(ctx, root)?;
    // Charge container cardinality before BREP/IR construction so max_entities
    // can refuse the expensive path rather than only the finalizer.
    let container_entities =
        (scan.blocks.len() + scan.compound_streams.len() + scan.directory.len()) as u64;
    ctx.charge_entities(container_entities, "admit SLDPRT container entities")?;
    let mut admitted_entities = 0_u64;

    if ctx.container_only() {
        let (ir, annotations, unknowns, mut pmi_losses) =
            build_metadata_ir(ctx, &scan, &mut admitted_entities)?;
        let mut report = build_container_report(&scan, true);
        report.losses.append(&mut pmi_losses);
        return decode_result(ir, report, annotations, unknowns);
    }

    let streams = active_body_streams(&scan);
    if !streams.is_empty() {
        ctx.charge_entities(streams.len() as u64, "admit SLDPRT body streams")?;
        if let Some((decoded, mut report)) = try_decode_brep(&scan, &streams) {
            let source_header = decoded
                .metadata_stream
                .and_then(|index| streams.get(index).map(|stream| &stream.header));
            let (ir, annotations, unknowns, mut pmi_losses) = build_geometry_ir(
                ctx,
                &scan,
                source_header,
                decoded.brep,
                &decoded.configuration_bodies,
                &mut admitted_entities,
            )?;
            report.losses.append(&mut pmi_losses);
            append_tessellation_losses(&ir, &mut report);
            append_design_losses(&ir, &mut report);
            return decode_result(ir, report, annotations, unknowns);
        }
    }

    let (ir, annotations, unknowns, mut pmi_losses) =
        build_metadata_ir(ctx, &scan, &mut admitted_entities)?;
    let mut report = build_container_report(&scan, false);
    report.losses.append(&mut pmi_losses);
    append_design_losses(&ir, &mut report);
    decode_result(ir, report, annotations, unknowns)
}

fn append_tessellation_losses(ir: &CadIr, report: &mut DecodeReport) {
    let unresolved = ir
        .model
        .tessellations
        .iter()
        .filter(|mesh| mesh.body.is_none() || mesh.faces.is_empty())
        .count();
    if unresolved > 0 {
        report
            .losses
            .push(SldprtLossCode::TessellationFaceOwnershipUnresolved.note(format!(
                "{unresolved} DisplayLists tessellation table(s) do not resolve to B-rep face ownership. Geometry and native channels are retained without fabricating body or face references."
            )));
    }
}

fn decode_result(
    mut ir: CadIr,
    report: DecodeReport,
    annotations: Annotations,
    mut unknowns: Vec<UnknownRecord>,
) -> Result<DecodeResult, CodecError> {
    let mut source_fidelity = cadmpeg_ir::SourceFidelity::with_annotations(annotations);
    let source_image = unknowns
        .iter()
        .position(|record| record.id.0 == "sldprt:file:source-image#0")
        .map(|index| unknowns.remove(index));
    source_fidelity.attach_native_unknown_records(&mut ir, "sldprt", unknowns)?;
    if let Some(source_image) = source_image {
        source_fidelity.retain_unknown_records("sldprt", [source_image]);
    }
    stamp_local_digests(&mut ir);
    Ok(DecodeResult::new(ir, report, source_fidelity))
}

fn incomplete_pattern(
    pattern: &cadmpeg_ir::features::PatternKind,
    incomplete_path: &dyn Fn(&cadmpeg_ir::features::PathRef) -> bool,
) -> bool {
    use cadmpeg_ir::features::{PatternKind, PatternScaleCenter};

    match pattern {
        PatternKind::Unresolved { .. } => true,
        PatternKind::Linear { direction, .. } | PatternKind::LinearOffsets { direction, .. } => {
            direction.is_none()
        }
        PatternKind::Circular { .. } | PatternKind::Mirror { .. } => false,
        PatternKind::MirrorReference { .. } => true,
        PatternKind::CircularAngles { angles, .. } => angles.is_empty(),
        PatternKind::CurveDriven { path, .. } => path.as_ref().is_none_or(incomplete_path),
        PatternKind::Scale { center, .. } => matches!(center, PatternScaleCenter::Native(_)),
        PatternKind::Composite { stages } => {
            stages.is_empty()
                || stages
                    .iter()
                    .any(|stage| incomplete_pattern(&stage.pattern, incomplete_path))
        }
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
            document.trim().is_empty() || object.trim().is_empty()
        }
        cadmpeg_ir::features::BinderTarget::Native { .. } => true,
    }
}

fn sketch_constraint_has_complete_neutral_semantics(
    definition: &cadmpeg_ir::sketches::SketchConstraintDefinition,
) -> bool {
    use cadmpeg_ir::sketches::SketchConstraintDefinition as Constraint;

    match definition {
        Constraint::Native { .. } => false,
        Constraint::Disabled
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
        | Constraint::Offset { .. }
        | Constraint::ProjectedCopy { .. }
        | Constraint::AtIntersection { .. }
        | Constraint::Concentric { .. }
        | Constraint::Coradial { .. }
        | Constraint::Collinear { .. }
        | Constraint::Symmetric { .. }
        | Constraint::PointSymmetric { .. }
        | Constraint::Horizontal { .. }
        | Constraint::HorizontalLoci { .. }
        | Constraint::Vertical { .. }
        | Constraint::VerticalLoci { .. }
        | Constraint::HorizontalPoints { .. }
        | Constraint::VerticalPoints { .. }
        | Constraint::Parallel { .. }
        | Constraint::Perpendicular { .. }
        | Constraint::Tangent { .. }
        | Constraint::TangentLoci { .. }
        | Constraint::Curvature { .. }
        | Constraint::Equal { .. }
        | Constraint::Fixed { .. }
        | Constraint::ArcAngle { .. }
        | Constraint::EllipseAngle { .. }
        | Constraint::Distance { .. }
        | Constraint::DistanceLoci { .. }
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
    definition: &cadmpeg_ir::sketches::SpatialSketchConstraintDefinition,
) -> bool {
    use cadmpeg_ir::sketches::SpatialSketchConstraintDefinition as Constraint;

    match definition {
        Constraint::Native { .. } => false,
        Constraint::Coincident { .. }
        | Constraint::Symmetric { .. }
        | Constraint::PointOnSurface { .. }
        | Constraint::Midpoint { .. }
        | Constraint::Tangent { .. }
        | Constraint::PointDistance { .. }
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

fn append_design_losses(ir: &CadIr, report: &mut DecodeReport) {
    use cadmpeg_ir::features::{
        BodyRetentionMode, BodySelection, BooleanOp, ChamferSpec, EdgeSelection, ExtrudeExtent,
        FaceSelection, FeatureDefinition, FeatureSourceContent, PathRef, ProfileRef, RadiusSpec,
        RevolveExtent, SplitFaceTool, Termination,
    };
    use cadmpeg_ir::sketches::{SketchGeometry, SpatialSketchGeometry};

    let native = ir
        .native
        .namespace("sldprt")
        .and_then(|namespace| crate::native::SldprtNative::load(namespace).ok());

    let active_configurations = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| configuration.active.is_active())
        .count();
    if !ir.model.configurations.is_empty() && active_configurations != 1 {
        report.losses.push(SldprtLossCode::ConfigActiveIdentityUnresolved.note(format!(
                "active configuration identity is unresolved; {active_configurations} of {} configuration records are active.",
                ir.model.configurations.len()
            )));
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
            .find(|configuration| configuration.active.is_active())
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
    let unresolved_configuration_parameter_lanes = native.as_ref().map_or(0, |native| {
        crate::history::unresolved_configuration_lanes(
            &ir.model.configurations,
            &native.feature_input_lanes,
        )
    });
    if unresolved_configuration_parameter_lanes > 0 {
        report.losses.push(SldprtLossCode::ConfigLaneIdentityUnresolved.note(format!(
                "{unresolved_configuration_parameter_lanes} configuration-scoped feature-input lane(s) have duplicate or unresolved configuration identity."
            )));
    }
    let mut configuration_source_counts = BTreeMap::new();
    for source_index in ir
        .model
        .configurations
        .iter()
        .filter_map(|configuration| configuration.source_index)
    {
        *configuration_source_counts
            .entry(source_index)
            .or_insert(0usize) += 1;
    }
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
        .filter(|configuration| configuration.name.resolved().is_none_or(str::is_empty))
        .count();
    let mut configuration_name_counts = BTreeMap::new();
    let mut configuration_ordinal_counts = BTreeMap::new();
    for configuration in &ir.model.configurations {
        *configuration_ordinal_counts
            .entry(configuration.ordinal)
            .or_insert(0usize) += 1;
    }
    for name in ir
        .model
        .configurations
        .iter()
        .filter_map(|configuration| configuration.name.resolved())
        .filter(|name| !name.is_empty())
    {
        *configuration_name_counts.entry(name).or_insert(0usize) += 1;
    }
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
        .collect::<std::collections::HashSet<_>>();
    let incoherent_configuration_bodies = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| {
            let mut bodies = std::collections::HashSet::new();
            configuration
                .bodies
                .resolved()
                .unwrap_or_default()
                .iter()
                .any(|body| !bodies.insert(body) || !model_body_ids.contains(body))
        })
        .count();
    let unresolved_configuration_bodies = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| configuration.bodies.is_unresolved())
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
        .collect::<std::collections::HashSet<_>>();
    let parameter_ids = ir
        .model
        .parameters
        .iter()
        .map(|parameter| &parameter.id)
        .collect::<std::collections::HashSet<_>>();
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
        report.losses.push(SldprtLossCode::ConfigIncompleteSnapshot.note(format!(
                "{incomplete_configuration_feature_snapshots} configuration(s) lack a complete evaluated feature snapshot; {incomplete_configuration_parameter_snapshots} configuration(s) lack a complete evaluated parameter snapshot."
            )));
    }
    let incoherent_configuration_suppression =
        ir.model
            .configurations
            .iter()
            .filter(|configuration| {
                let mut suppressed = std::collections::HashSet::new();
                configuration
                    .suppressed_features
                    .iter()
                    .any(|feature| !feature_ids.contains(feature) || !suppressed.insert(feature))
                    || (!configuration.feature_states.is_empty()
                        && configuration.feature_states.iter().any(|(feature, state)| {
                            state.suppressed != suppressed.contains(feature)
                        }))
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

    let feature_names = ir
        .model
        .features
        .iter()
        .filter_map(|feature| {
            feature
                .name
                .as_ref()
                .map(|name| (feature.id.clone(), name.clone()))
        })
        .collect();
    let global_parameter_owners = crate::history::global_parameter_owners(&ir.model.features);
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
    let unresolved_parameter_references = crate::history::parameters_with_unresolved_references(
        &ir.model.parameters,
        &feature_names,
        &global_parameter_owners,
    );
    let unevaluable_parameter_expressions = crate::history::parameters_with_unevaluable_expressions(
        &ir.model.parameters,
        &feature_names,
        &global_parameter_owners,
        &ir.model.configurations,
    );
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
    let incoherent_parameter_dependencies = crate::history::parameters_with_incoherent_dependencies(
        &ir.model.parameters,
        &feature_names,
        &global_parameter_owners,
    );
    let incoherent_parameter_values = crate::history::parameters_with_incoherent_evaluated_values(
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
        report.losses.push(SldprtLossCode::ParameterUnevaluated.note(format!(
                "{incomplete_parameters} parameter(s) lack an evaluated scalar; {unresolved_parameter_references} parameter expression(s) contain unresolved, ambiguous, or malformed parameter references; {unevaluable_parameter_expressions} parameter expression(s) cannot regenerate a finite typed value; {invalid_parameter_dependency_order} parameter record(s) contain missing or non-preceding dependency edges; {incoherent_parameter_dependencies} parameter record(s) have dependency edges inconsistent with their expressions; {incoherent_parameter_values} dependency-driven parameter(s) disagree with their evaluated expressions."
            )));
    }
    let empty_parameter_names = ir
        .model
        .parameters
        .iter()
        .filter(|parameter| parameter.name.is_empty())
        .count();
    let mut parameter_name_counts = BTreeMap::new();
    let mut parameter_ordinal_counts = BTreeMap::new();
    for parameter in &ir.model.parameters {
        if !parameter.name.is_empty() {
            *parameter_name_counts
                .entry((&parameter.owner, parameter.name.as_str()))
                .or_insert(0usize) += 1;
        }
        *parameter_ordinal_counts
            .entry((&parameter.owner, parameter.ordinal))
            .or_insert(0usize) += 1;
    }
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

    let bound_pmi = ir
        .model
        .parameters
        .iter()
        .filter_map(|parameter| parameter.pmi.as_ref())
        .map(|pmi| pmi.native_ref.as_str())
        .collect::<std::collections::HashSet<_>>();
    let unbound_pmi_dimensions = native.as_ref().map_or(0, |native| {
        crate::pmi::unbound_dimension_count(&native.pmi_dimensions, &bound_pmi)
    });
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
        report.losses.push(SldprtLossCode::PmiDimensionUnbound.note(format!(
                "{unbound_pmi_dimensions} semantic dimension record(s) are not bound to parameters; {native_pmi_subtypes} parameter dimension(s) retain native subtypes."
            )));
    }

    let incomplete_history_references = native.as_ref().map_or(0, |native| {
        crate::history::incomplete_history_reference_features(&native.feature_histories)
    });
    if incomplete_history_references > 0 {
        report.losses.push(SldprtLossCode::HistoryIncompleteReferences.note(format!(
                "{incomplete_history_references} feature history record(s) contain duplicate identities or unresolved parent, dependency, dimension, or child references."
            )));
    }
    let feature_positions = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature.ordinal))
        .collect::<BTreeMap<_, _>>();
    let evaluated_feature_states = if ir
        .model
        .configurations
        .iter()
        .any(|configuration| !configuration.feature_states.is_empty())
    {
        ir.model
            .configurations
            .iter()
            .flat_map(|configuration| {
                ir.model.features.iter().filter_map(move |feature| {
                    configuration.feature_states.get(&feature.id).map(|state| {
                        EvaluatedFeatureState {
                            feature,
                            dependencies: &state.dependencies,
                            outputs: &state.outputs,
                            definition: &state.definition,
                        }
                    })
                })
            })
            .collect::<Vec<_>>()
    } else {
        ir.model
            .features
            .iter()
            .map(|feature| EvaluatedFeatureState {
                feature,
                dependencies: &feature.dependencies,
                outputs: &feature.outputs,
                definition: &feature.definition,
            })
            .collect::<Vec<_>>()
    };
    let incoherent_feature_edges = evaluated_feature_states
        .iter()
        .filter(|state| {
            let feature = state.feature;
            let parent_incoherent = feature.parent.as_ref().is_some_and(|parent| {
                feature_positions
                    .get(parent)
                    .is_none_or(|ordinal| *ordinal >= feature.ordinal)
            });
            let mut dependencies = std::collections::HashSet::new();
            parent_incoherent
                || state.dependencies.iter().any(|dependency| {
                    !dependencies.insert(dependency)
                        || feature_positions
                            .get(dependency)
                            .is_none_or(|ordinal| *ordinal >= feature.ordinal)
                })
        })
        .count();
    let mut feature_ordinal_counts = BTreeMap::new();
    for feature in &ir.model.features {
        *feature_ordinal_counts
            .entry(feature.ordinal)
            .or_insert(0usize) += 1;
    }
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
            let mut parameters = std::collections::HashSet::new();
            let mut children = std::collections::HashSet::new();
            feature.source_content.iter().any(|content| match content {
                FeatureSourceContent::Text(_) => false,
                FeatureSourceContent::Parameter(parameter) => {
                    !parameters.insert(parameter)
                        || parameter_owners
                            .get(parameter)
                            .is_none_or(|owner| owner.as_ref() != Some(&feature.id))
                }
                FeatureSourceContent::Feature(child) => {
                    !children.insert(child)
                        || features_by_id.get(child).is_none_or(|child| {
                            child.ordinal <= feature.ordinal
                                || child.parent.as_ref() != Some(&feature.id)
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
            state
                .feature
                .source_properties
                .get("Scope")
                .is_some_and(|scope| !scope.trim().is_empty())
                && state.outputs.is_empty()
        })
        .count();
    if unresolved_output_scopes > 0 {
        report.losses.push(SldprtLossCode::FeatureUnresolvedOutputScope.note(format!(
                "{unresolved_output_scopes} feature(s) retain non-empty native output scopes that do not resolve to model bodies."
            )));
    }
    let body_ids = ir
        .model
        .bodies
        .iter()
        .map(|body| &body.id)
        .collect::<std::collections::HashSet<_>>();
    let incoherent_feature_outputs = evaluated_feature_states
        .iter()
        .filter(|state| {
            let mut outputs = std::collections::HashSet::new();
            state
                .outputs
                .iter()
                .any(|body| !outputs.insert(body) || !body_ids.contains(body))
        })
        .count();
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
            !sketch_constraint_has_complete_neutral_semantics(&constraint.definition)
                && constraint.active != Some(false)
        })
        .count();
    let native_spatial_constraints = ir
        .model
        .spatial_sketch_constraints
        .iter()
        .filter(|constraint| {
            !spatial_sketch_constraint_has_complete_neutral_semantics(&constraint.definition)
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
        .filter(|entity| matches!(entity.geometry, SketchGeometry::Native { .. }))
        .count()
        + ir.model
            .spatial_sketch_entities
            .iter()
            .filter(|entity| matches!(entity.geometry, SpatialSketchGeometry::Native { .. }))
            .count();
    if native_sketch_geometry > 0 {
        report.losses.push(SldprtLossCode::SketchNativeGeometry.note(format!(
                "{native_sketch_geometry} sketch entity geometry record(s) retain native kinds without solved neutral geometry."
            )));
    }

    let unprojected_relations = native
        .as_ref()
        .map_or(0, |native| unprojected_sketch_relation_records(ir, native));
    if unprojected_relations > 0 {
        report.losses.push(SldprtLossCode::SketchRelationUnprojected.note(format!(
                "{unprojected_relations} native sketch relation record(s) have no projected neutral constraint."
            )));
    }
    let multiply_projected_relations = native.as_ref().map_or(0, |native| {
        multiply_projected_sketch_relation_records(ir, native)
    });
    if multiply_projected_relations > 0 {
        report.losses.push(SldprtLossCode::SketchRelationMultiplyProjected.note(format!(
                "{multiply_projected_relations} native sketch relation record(s) are claimed by multiple neutral objects."
            )));
    }

    let native_features = evaluated_feature_states
        .iter()
        .filter(|state| matches!(state.definition, FeatureDefinition::Native { .. }))
        .count();
    if native_features > 0 {
        report.losses.push(SldprtLossCode::FeatureNativeKindRetained.note(format!(
                "{native_features} feature(s) retain their native kind without a complete neutral operation definition."
            )));
    }
    let unbound_feature_input_objects = native
        .as_ref()
        .map_or(0, unbound_feature_input_operation_objects);
    if unbound_feature_input_objects > 0 {
        report.losses.push(SldprtLossCode::FeatureInputObjectUnbound.note(format!(
                "{unbound_feature_input_objects} native feature-input operation object(s) do not bind uniquely to a history feature."
            )));
    }

    let incomplete_edge_selection = |selection: &EdgeSelection| match selection {
        EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. } => edges.is_empty(),
        EdgeSelection::Historical { edges, .. } => edges.is_empty(),
        EdgeSelection::HistoricalPartial {
            edges, unresolved, ..
        } => edges.is_empty() || !unresolved.is_empty(),
        EdgeSelection::Generated { edges, .. } => edges.is_empty(),
        EdgeSelection::All => false,
        EdgeSelection::Unresolved | EdgeSelection::Native(_) => true,
    };
    let incomplete_face_selection = |selection: &FaceSelection| match selection {
        FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => faces.is_empty(),
        FaceSelection::Historical { faces, .. } => faces.is_empty(),
        FaceSelection::HistoricalPartial {
            faces, unresolved, ..
        } => faces.is_empty() || !unresolved.is_empty(),
        FaceSelection::Generated { faces, .. } => faces.is_empty(),
        FaceSelection::Unresolved | FaceSelection::Native(_) => true,
    };
    let incomplete_optional_face_selection = |selection: &FaceSelection| match selection {
        FaceSelection::Faces(_) | FaceSelection::Resolved { .. } => false,
        FaceSelection::Historical { faces, .. } => faces.is_empty(),
        FaceSelection::HistoricalPartial {
            faces, unresolved, ..
        } => faces.is_empty() || !unresolved.is_empty(),
        FaceSelection::Generated { faces, .. } => faces.is_empty(),
        FaceSelection::Unresolved | FaceSelection::Native(_) => true,
    };
    let incomplete_body_selection = |selection: &BodySelection| match selection {
        BodySelection::Bodies(bodies)
        | BodySelection::Resolved { bodies, .. }
        | BodySelection::ResolvedSet { bodies, .. } => bodies.is_empty(),
        BodySelection::Historical { bodies, .. }
        | BodySelection::HistoricalSet { bodies, .. }
        | BodySelection::HistoricalUnorderedSet { bodies, .. } => bodies.is_empty(),
        BodySelection::Generated { bodies, .. } => bodies.is_empty(),
        BodySelection::Local { bodies, .. } => bodies.is_empty(),
        BodySelection::Unresolved | BodySelection::Native(_) | BodySelection::NativeSet(_) => true,
    };
    let incomplete_profile = |profile: &ProfileRef| match profile {
        ProfileRef::Faces(faces) => faces.is_empty(),
        ProfileRef::Generated { curves, .. } => curves.is_empty(),
        ProfileRef::SketchProfiles { profiles, .. }
        | ProfileRef::SpatialSketchProfiles { profiles, .. } => profiles.is_empty(),
        ProfileRef::SketchRegions { regions, .. } => regions.is_empty(),
        ProfileRef::SketchEntities { entities, .. } => entities.is_empty(),
        ProfileRef::SketchSelection { selections, .. }
        | ProfileRef::SpatialSketchSelection { selections, .. } => selections.is_empty(),
        ProfileRef::HistoricalFaces { faces, .. } => faces.is_empty(),
        ProfileRef::Unresolved(_) | ProfileRef::Native(_) => true,
        ProfileRef::Sketch(_) | ProfileRef::Feature(_) => false,
    };
    let incomplete_path = |path: &PathRef| match path {
        PathRef::Edges(edges) => edges.is_empty(),
        PathRef::Curves(curves) => curves.is_empty(),
        PathRef::HistoricalEdges { edges, .. } => edges.is_empty(),
        PathRef::SpatialSketchSelection { selections, .. } => selections.is_empty(),
        PathRef::Unresolved(_) | PathRef::Native(_) => true,
        PathRef::Sketch(_) => false,
        PathRef::SketchCurves { curves, .. } => curves.is_empty(),
        PathRef::SpatialSketchCurves { curves, .. } => curves.is_empty(),
    };
    let incomplete_vertex_selection = |selection: &cadmpeg_ir::features::VertexSelection| {
        matches!(
            selection,
            cadmpeg_ir::features::VertexSelection::Unresolved
                | cadmpeg_ir::features::VertexSelection::Native(_)
        )
    };
    let incomplete_termination = |termination: &Termination| match termination {
        Termination::Unresolved => true,
        Termination::ToFace { face, .. }
        | Termination::OffsetFromFace { face, .. }
        | Termination::ToShape { target: face } => incomplete_face_selection(face),
        Termination::ToVertex { vertex } => incomplete_vertex_selection(vertex),
        Termination::Blind { .. }
        | Termination::ThroughAll
        | Termination::ThroughNext
        | Termination::ToFirst
        | Termination::ToLast
        | Termination::Angle { .. } => false,
    };
    let incomplete_extrude_extent = |extent: &ExtrudeExtent| match extent {
        ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => {
            incomplete_termination(&side.termination)
        }
        ExtrudeExtent::TwoSided { first, second } => {
            incomplete_termination(&first.termination)
                || incomplete_termination(&second.termination)
        }
    };
    let incomplete_revolve_extent = |extent: &RevolveExtent| match extent {
        RevolveExtent::OneSided { termination } | RevolveExtent::Symmetric { termination } => {
            incomplete_termination(termination)
        }
        RevolveExtent::TwoSided { first, second } => {
            incomplete_termination(first) || incomplete_termination(second)
        }
    };
    let incomplete_typed_features = evaluated_feature_states
        .iter()
        .filter(|state| {
            let mut definition = state.definition;
            while let FeatureDefinition::PostProcess { operation, .. } = definition {
                definition = operation;
            }
            match definition {
            FeatureDefinition::TreeNode { .. }
            | FeatureDefinition::DatumPrincipalPlane { .. }
            | FeatureDefinition::DatumPlane { .. }
            | FeatureDefinition::DatumThreePointPlane { .. }
            | FeatureDefinition::DatumAxis { .. }
            | FeatureDefinition::DatumPoint { .. }
            | FeatureDefinition::DatumCoordinateSystem { .. }
            | FeatureDefinition::EquationCurve { .. }
            | FeatureDefinition::Helix { .. } => false,
            FeatureDefinition::BaseFeature { bodies }
            | FeatureDefinition::InsertBodies { bodies } => incomplete_body_selection(bodies),
            FeatureDefinition::MeshImport { .. } => false,
            FeatureDefinition::InsertComponent { occurrence } => !ir
                .model
                .occurrences
                .iter()
                .any(|candidate| candidate.id == *occurrence),
            FeatureDefinition::AssemblyJoint { joint } => !ir
                .model
                .assembly_joints
                .iter()
                .any(|candidate| candidate.id == *joint),
            FeatureDefinition::ReferenceImage { asset, .. }
            | FeatureDefinition::Decal { asset, .. } => {
                !ir.model.assets.iter().any(|candidate| candidate.id == *asset)
            }
            FeatureDefinition::StoredGeometry => state.outputs.is_empty(),
            FeatureDefinition::ExtractBody { source } => incomplete_body_selection(source),
            FeatureDefinition::DerivedGeometry { source } => {
                feature_positions
                    .get(source)
                    .is_none_or(|ordinal| *ordinal >= state.feature.ordinal)
                    || !state.dependencies.contains(source)
            }
            FeatureDefinition::ImportedGeometry { path, .. } => path.trim().is_empty(),
            FeatureDefinition::Form { cages } => cages.is_empty(),
            FeatureDefinition::PointGeometry { .. }
            | FeatureDefinition::LineSegment { .. }
            | FeatureDefinition::CircularArc { .. }
            | FeatureDefinition::EllipticArc { .. }
            | FeatureDefinition::PlanarPatch { .. } => false,
            FeatureDefinition::Polyline { points, .. } => points.len() < 2,
            FeatureDefinition::RegularPolygonCurve { sides, .. } => *sides < 3,
            FeatureDefinition::FaceFromShapes {
                sources,
                face_maker_class,
            } => incomplete_body_selection(sources) || face_maker_class.trim().is_empty(),
            FeatureDefinition::Block {
                dimensions,
                placement,
                ..
            } => dimensions.is_none() || placement.is_none(),
            FeatureDefinition::ProjectOnSurface {
                sources,
                support_face,
                ..
            } => incomplete_path(sources) || incomplete_face_selection(support_face),
            FeatureDefinition::Coil {
                construction,
                result,
            } => {
                matches!(
                    construction.placement,
                    cadmpeg_ir::features::CoilPlacement::Native { .. }
                ) || match result {
                    cadmpeg_ir::features::CoilResult::NewBody => false,
                    cadmpeg_ir::features::CoilResult::Boolean {
                        operation,
                        targets,
                    } => {
                        *operation == BooleanOp::Unresolved
                            || incomplete_body_selection(targets)
                    }
                }
            }
            FeatureDefinition::Sphere { op, .. }
            | FeatureDefinition::Torus { op, .. }
            | FeatureDefinition::Primitive { op, .. } => *op == BooleanOp::Unresolved,
            FeatureDefinition::CosmeticThread {
                face,
                diameter,
                extent,
            } => {
                incomplete_face_selection(face)
                    || diameter.is_none()
                    || extent.is_none()
            }
            FeatureDefinition::SketchBlockDefinition { sketch } => sketch.is_none(),
            FeatureDefinition::SketchBlockInstance { block, placement } => {
                block.is_none() || placement.is_none()
            }
            FeatureDefinition::DatumOffsetPlane { reference, .. } => reference
                .as_ref()
                .is_none_or(|reference| match reference {
                    cadmpeg_ir::features::DatumPlaneReference::Feature(_) => false,
                    cadmpeg_ir::features::DatumPlaneReference::Face { face, .. } => {
                        incomplete_face_selection(face)
                    }
                }),
            FeatureDefinition::ProjectedCurve {
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
            FeatureDefinition::CompositeCurve { segments, .. } => {
                segments.is_empty() || segments.iter().any(incomplete_path)
            }
            FeatureDefinition::HelixNativeAxis { .. } => true,
            FeatureDefinition::Wrap {
                profile,
                face,
                mode,
                depth,
            } => {
                incomplete_profile(profile)
                    || incomplete_face_selection(face)
                    || (*mode != cadmpeg_ir::features::WrapMode::Scribe && depth.is_none())
            }
            FeatureDefinition::Sketch { sketch, .. } => sketch.is_none(),
            FeatureDefinition::SpatialSketch { sketch } => sketch.is_none(),
            FeatureDefinition::Extrude {
                profile,
                direction,
                start,
                extent,
                op,
                direction_source,
                ..
            } => {
                incomplete_profile(profile)
                    || matches!(direction, cadmpeg_ir::features::ExtrudeDirection::Unresolved)
                    || match start {
                        cadmpeg_ir::features::ExtrudeStart::Unresolved => true,
                        cadmpeg_ir::features::ExtrudeStart::FromFace { face, .. } => {
                            incomplete_face_selection(face)
                        }
                        cadmpeg_ir::features::ExtrudeStart::ProfilePlane
                        | cadmpeg_ir::features::ExtrudeStart::OffsetProfilePlane { .. } => false,
                    }
                    || matches!(
                        direction_source,
                        Some(cadmpeg_ir::features::ExtrusionDirectionSource::Edge { reference })
                            if incomplete_path(reference)
                    )
                    || incomplete_extrude_extent(extent)
                    || *op == BooleanOp::Unresolved
            }
            FeatureDefinition::Revolve { construction, op } => {
                construction.profile.as_ref().is_none_or(incomplete_profile)
                    || construction.axis.is_none()
                    || construction.extent.as_ref().is_none_or(incomplete_revolve_extent)
                    || *op == BooleanOp::Unresolved
            }
            FeatureDefinition::Sweep {
                section,
                sections,
                path,
                mode,
                orientation,
                ..
            } => {
                matches!(section, cadmpeg_ir::features::SweepSection::Unresolved(_))
                    || section.referenced_profile().is_some_and(incomplete_profile)
                    || sections.iter().any(|section| {
                        matches!(section, cadmpeg_ir::features::SweepSection::Unresolved(_))
                            || section.referenced_profile().is_some_and(incomplete_profile)
                    })
                    || path.as_ref().is_none_or(incomplete_path)
                    || matches!(
                        orientation,
                        Some(cadmpeg_ir::features::SweepOrientation::Auxiliary { path, .. })
                            if incomplete_path(path)
                    )
                    || matches!(mode, cadmpeg_ir::features::SweepMode::Unresolved)
                    || matches!(mode, cadmpeg_ir::features::SweepMode::Solid { op } if *op == BooleanOp::Unresolved)
            }
            FeatureDefinition::HelicalSweep { construction, op } => {
                incomplete_profile(&construction.profile) || *op == BooleanOp::Unresolved
            }
            FeatureDefinition::Binder {
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
                            .any(|subelement| subelement.trim().is_empty())
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
            FeatureDefinition::Loft {
                sections,
                guides,
                centerline,
                op,
                ..
            } => {
                sections.len() < 2
                    || sections.iter().any(|section| match section {
                        cadmpeg_ir::features::LoftSection::Profile(profile) => incomplete_profile(profile),
                        cadmpeg_ir::features::LoftSection::Point(cadmpeg_ir::features::LoftPointSection::Native(_)) => true,
                        cadmpeg_ir::features::LoftSection::Point(_) => false,
                    })
                    || guides.iter().any(incomplete_path)
                    || centerline.as_ref().is_some_and(incomplete_path)
                    || *op == BooleanOp::Unresolved
            }
            FeatureDefinition::Rib { construction, op } => {
                construction.profile.as_ref().is_none_or(incomplete_profile)
                    || construction.direction.is_none()
                    || construction.thickness.is_none()
                    || construction.side.is_none()
                    || matches!(construction.draft, cadmpeg_ir::features::RibDraft::Unresolved)
                    || *op == BooleanOp::Unresolved
            }
            FeatureDefinition::SheetMetalBaseFlange { profile, .. } => {
                incomplete_profile(profile)
            }
            FeatureDefinition::SheetMetalEdgeFlange { edges, .. } => {
                incomplete_edge_selection(edges)
            }
            FeatureDefinition::SheetMetalHem { .. } => true,
            FeatureDefinition::Fillet { groups } => {
                groups.is_empty()
                    || groups.iter().any(|group| {
                        incomplete_edge_selection(&group.edges)
                            || matches!(group.radius, RadiusSpec::Unresolved { .. })
                            || matches!(group.radius, RadiusSpec::Variable { ref points } if points.is_empty())
                    })
            }
            FeatureDefinition::FullRoundFillet { groups } => {
                groups.is_empty()
                    || groups.iter().any(|group| {
                        incomplete_face_selection(&group.center_faces)
                            || matches!(
                                group.side_one_faces,
                                cadmpeg_ir::features::FullRoundSideSelection::Unresolved
                            )
                            || matches!(
                                group.side_two_faces,
                                cadmpeg_ir::features::FullRoundSideSelection::Unresolved
                            )
                            || matches!(
                                group.side_one_faces,
                                cadmpeg_ir::features::FullRoundSideSelection::Explicit(
                                    ref selection
                                ) if incomplete_face_selection(selection)
                            )
                            || matches!(
                                group.side_two_faces,
                                cadmpeg_ir::features::FullRoundSideSelection::Explicit(
                                    ref selection
                                ) if incomplete_face_selection(selection)
                            )
                    })
            }
            FeatureDefinition::Chamfer { groups, .. } => groups.is_empty() || groups.iter().any(|group| {
                incomplete_edge_selection(&group.edges) || matches!(group.spec, ChamferSpec::Unresolved { .. })
            }),
            FeatureDefinition::FaceBlend {
                first_faces,
                second_faces,
                radius,
            } => {
                incomplete_face_selection(first_faces)
                    || incomplete_face_selection(second_faces)
                    || matches!(radius, RadiusSpec::Unresolved { .. })
                    || matches!(radius, RadiusSpec::Variable { points } if points.is_empty())
            }
            FeatureDefinition::Shell {
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
            FeatureDefinition::OffsetShape { source, .. }
            | FeatureDefinition::RefineShape { source }
            | FeatureDefinition::ReverseShape { source } => incomplete_body_selection(source),
            FeatureDefinition::Compound { members } => incomplete_body_selection(members),
            FeatureDefinition::RuledBetweenCurves { first, second, .. } => {
                incomplete_path(first) || incomplete_path(second)
            }
            FeatureDefinition::SectionShape {
                first,
                second,
                approximate,
            } => {
                incomplete_body_selection(first)
                    || incomplete_body_selection(second)
                    || approximate.is_none()
            }
            FeatureDefinition::MirrorShape {
                source,
                plane_reference,
                ..
            } => {
                incomplete_body_selection(source)
                    || plane_reference
                        .as_ref()
                        .is_some_and(incomplete_face_selection)
            }
            FeatureDefinition::Thicken {
                faces,
                thickness,
                side,
            } => incomplete_face_selection(faces) || thickness.is_none() || side.is_none(),
            FeatureDefinition::OffsetSurface { faces, distance } => {
                incomplete_face_selection(faces) || distance.is_none()
            }
            FeatureDefinition::KnitSurface {
                faces,
                merge_entities,
                create_solid,
                ..
            } => {
                incomplete_face_selection(faces)
                    || merge_entities.is_none()
                    || create_solid.is_none()
            }
            FeatureDefinition::ExtendSurface {
                faces,
                distance,
                method,
            } => {
                incomplete_face_selection(faces)
                    || distance.is_none()
                    || *method == cadmpeg_ir::features::SurfaceExtension::Unresolved
            }
            FeatureDefinition::FilledSurface {
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
                    || if *continuity
                        == Some(cadmpeg_ir::features::SurfaceContinuity::Contact)
                    {
                        incomplete_optional_face_selection(support_faces)
                    } else {
                        incomplete_face_selection(support_faces)
                    }
                    || continuity.is_none()
                    || merge_result.is_none()
            }
            FeatureDefinition::TrimSurface { faces, tool, keep } => {
                incomplete_face_selection(faces)
                    || incomplete_path(tool)
                    || *keep == cadmpeg_ir::features::TrimRegion::Unresolved
            }
            FeatureDefinition::RuledSurface {
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
            FeatureDefinition::Draft {
                faces,
                neutral_plane,
                parting_tool,
                pull_plane: _,
                pull_direction,
                angle,
                outward,
            } => {
                incomplete_face_selection(faces)
                    || parting_tool.as_ref().map_or_else(
                        || incomplete_face_selection(neutral_plane),
                        incomplete_face_selection,
                    )
                    || pull_direction.is_none()
                    || angle.is_none()
                    || (parting_tool.is_none() && outward.is_none())
            }
            FeatureDefinition::Combine {
                target, tools, op, ..
            } => {
                incomplete_body_selection(target)
                    || incomplete_body_selection(tools)
                    || *op == BooleanOp::Unresolved
            }
            FeatureDefinition::BoundaryFill { tools, cells } => {
                incomplete_body_selection(tools)
                    || cells.is_empty()
                    || cells.iter().any(incomplete_body_selection)
            }
            FeatureDefinition::CutWithSurface { targets, tools, .. } => {
                incomplete_body_selection(targets) || incomplete_face_selection(tools)
            }
            FeatureDefinition::TrimBodies {
                targets,
                tools,
                keep,
            } => {
                incomplete_body_selection(targets)
                    || incomplete_body_selection(tools)
                    || *keep == cadmpeg_ir::features::BodyTrimSide::Unresolved
            }
            FeatureDefinition::SplitBody { targets, tools } => {
                incomplete_body_selection(targets) || incomplete_face_selection(tools)
            }
            FeatureDefinition::SplitFace { targets, tool } => {
                incomplete_face_selection(targets)
                    || match tool {
                        SplitFaceTool::Path(path) => incomplete_path(path),
                        SplitFaceTool::Plane { .. } | SplitFaceTool::Planes { .. } => false,
                    }
            }
            FeatureDefinition::SewBodies {
                bodies,
                gap_tolerance,
            } => incomplete_body_selection(bodies) || gap_tolerance.is_none(),
            FeatureDefinition::DeleteBody { bodies, mode } => {
                incomplete_body_selection(bodies) || *mode == BodyRetentionMode::Unresolved
            }
            FeatureDefinition::DeleteFace { faces, .. } => incomplete_face_selection(faces),
            FeatureDefinition::ReplaceFace {
                targets,
                replacements,
            } => incomplete_face_selection(targets) || incomplete_face_selection(replacements),
            FeatureDefinition::MoveFace { faces, .. } => incomplete_face_selection(faces),
            FeatureDefinition::MoveBody { bodies, .. } => incomplete_body_selection(bodies),
            FeatureDefinition::Dome {
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
            FeatureDefinition::Flex { axis, mode } => {
                axis.is_none()
                    || matches!(mode, cadmpeg_ir::features::FlexMode::Unresolved { .. })
            }
            FeatureDefinition::Scale {
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
            FeatureDefinition::Hole {
                profile,
                face,
                placements,
                kind,
                exit_kind,
                diameter,
                extent,
                ..
            } => {
                profile.as_ref().is_some_and(incomplete_profile)
                    || face.as_ref().is_some_and(incomplete_face_selection)
                    || placements.is_empty()
                    || matches!(kind, cadmpeg_ir::features::HoleKind::Unresolved { .. })
                    || matches!(exit_kind, Some(cadmpeg_ir::features::HoleKind::Unresolved { .. }))
                    || diameter.is_none()
                    || extent.as_ref().is_none_or(incomplete_termination)
            }
            FeatureDefinition::Pattern { seeds, pattern } => {
                seeds.is_empty()
                    || seeds.iter().any(|seed| match seed {
                        cadmpeg_ir::features::PatternSeed::Feature(_) => false,
                        cadmpeg_ir::features::PatternSeed::Faces(faces) => {
                            incomplete_face_selection(faces)
                        }
                        cadmpeg_ir::features::PatternSeed::Bodies(bodies) => {
                            incomplete_body_selection(bodies)
                        }
                        cadmpeg_ir::features::PatternSeed::Occurrences(occurrences) => {
                            occurrences.is_empty()
                        }
                    })
                    || incomplete_pattern(pattern, &incomplete_path)
            }
            FeatureDefinition::Native { .. } | FeatureDefinition::PostProcess { .. } => false,
            // Unresolved construction retained as native.
            FeatureDefinition::DatumPlaneUnresolved
            | FeatureDefinition::DatumPointUnresolved
            | FeatureDefinition::DatumCoordinateSystemUnresolved
            | FeatureDefinition::LoftUnresolved
            | FeatureDefinition::FreeformSurfaceUnresolved
            | FeatureDefinition::BoundarySurfaceUnresolved
            | FeatureDefinition::DraftUnresolved => true,
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
                FeatureDefinition::DeleteBody {
                    mode: BodyRetentionMode::Unresolved,
                    ..
                }
            )
        })
        .count();
    if unresolved_body_modes > 0 {
        report.losses.push(SldprtLossCode::FeatureBodyRetentionUnresolved.note(format!(
                "{unresolved_body_modes} body delete/keep feature(s) retain selected native body identities without a decoded retention mode."
            )));
    }
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
                .get(&format!("sw_configuration_{slot}_needs_update"))
        })
        .is_some_and(|value| value.eq_ignore_ascii_case("yes"))
}

fn unbound_feature_input_operation_objects(native: &crate::native::SldprtNative) -> usize {
    use crate::classification::{classify, native_object_class};
    use crate::records::FeatureInputClassRole;

    let mut source_counts = BTreeMap::<u32, usize>::new();
    let mut binding_counts = BTreeMap::<(u32, &str), usize>::new();
    for feature in native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
    {
        let Some(source) = feature
            .source_id
            .as_deref()
            .and_then(|source| source.parse::<u32>().ok())
        else {
            continue;
        };
        *source_counts.entry(source).or_default() += 1;
        if let Some(class) = feature.input_class.as_deref() {
            *binding_counts.entry((source, class)).or_default() += 1;
        }
    }
    let mut named_binding_counts = BTreeMap::<(&str, &str, &str), usize>::new();
    for lane in &native.feature_input_lanes {
        for feature in native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
        {
            let (Some(class), Some(name)) = (
                feature.input_class.as_deref(),
                crate::resolved_features::scalars::feature_object_name(feature, lane),
            ) else {
                continue;
            };
            *named_binding_counts
                .entry((lane.id.as_str(), name.id.as_str(), class))
                .or_default() += 1;
        }
    }
    native
        .feature_input_lanes
        .iter()
        .flat_map(|lane| {
            lane.classes
                .iter()
                .filter(|class| class.role == FeatureInputClassRole::Feature)
                .filter_map(move |class| {
                    let name_offset = class.offset + 6 + class.name.len() as u64;
                    lane.names
                        .iter()
                        .find(|name| name.offset == name_offset)
                        .map(|name| (lane, class, name))
                })
        })
        .filter(|(lane, class, name)| {
            let source_bound = name.object_id.is_some_and(|id| {
                source_counts.get(&id).copied() == Some(1)
                    && (binding_counts.get(&(id, class.name.as_str())).copied() == Some(1)
                        || native_object_class(&class.name)
                            .feature
                            .is_some_and(|expected| {
                                native
                                    .feature_histories
                                    .iter()
                                    .flat_map(|history| &history.features)
                                    .any(|feature| {
                                        feature
                                            .source_id
                                            .as_deref()
                                            .and_then(|source| source.parse::<u32>().ok())
                                            == Some(id)
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
        .count()
}

fn unprojected_sketch_relation_records(ir: &CadIr, native: &crate::native::SldprtNative) -> usize {
    let sketch_feature_refs = ir
        .model
        .features
        .iter()
        .filter(|feature| {
            matches!(
                feature.definition,
                cadmpeg_ir::features::FeatureDefinition::Sketch { .. }
            )
        })
        .filter_map(|feature| feature.native_ref.as_deref())
        .collect::<std::collections::HashSet<_>>();
    let projected = ir
        .model
        .sketch_constraints
        .iter()
        .filter_map(|constraint| constraint.native_ref.clone())
        .chain(
            ir.model
                .sketch_entities
                .iter()
                .filter_map(|entity| entity.native_ref.clone()),
        )
        .chain(
            ir.model
                .spatial_sketch_entities
                .iter()
                .filter_map(|entity| entity.native_ref.clone()),
        )
        .collect::<std::collections::HashSet<_>>();
    let owned_instances = crate::resolved_features::relation_geometry::owned_relation_parameters(
        &ir.model.features,
        &ir.model.parameters,
        &native.feature_input_lanes,
    );

    native
        .feature_input_lanes
        .iter()
        .map(|lane| {
            let markers_by_id = lane
                .sketch_entities
                .iter()
                .map(|marker| (marker.id.as_str(), marker))
                .collect();
            let instances = lane
                .relation_instances
                .iter()
                .filter(|relation| {
                    sketch_feature_refs.contains(relation.feature_ref.as_str())
                        && owned_instances.contains_key(&relation.id)
                        && !projected.contains(&relation.id)
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
                                && relation.scalar_refs.contains(&binding.scalar_ref)
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
                        && !projected.contains(&marker.id)
                })
                .count();
            instances + bindings + markers
        })
        .sum()
}

fn multiply_projected_sketch_relation_records(
    ir: &CadIr,
    native: &crate::native::SldprtNative,
) -> usize {
    let native_relation_ids = native
        .feature_input_lanes
        .iter()
        .flat_map(|lane| {
            let markers_by_id = lane
                .sketch_entities
                .iter()
                .map(|marker| (marker.id.as_str(), marker))
                .collect();
            lane.relation_instances
                .iter()
                .map(|relation| relation.id.as_str())
                .chain(lane.sketch_entities.iter().filter_map(move |marker| {
                    crate::resolved_features::typed_relations::marker_owns_constraint(
                        marker,
                        &markers_by_id,
                    )
                    .then_some(marker.id.as_str())
                }))
        })
        .collect::<std::collections::HashSet<_>>();
    let mut projection_counts = BTreeMap::<&str, usize>::new();
    for native_ref in ir
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
        .filter(|native_ref| native_relation_ids.contains(native_ref))
    {
        *projection_counts.entry(native_ref).or_default() += 1;
    }
    projection_counts
        .values()
        .filter(|count| **count > 1)
        .count()
}

/// Collect the available Parasolid body streams, excluding auxiliary sites.
fn active_body_streams<'a>(scan: &'a ContainerScan<'_>) -> Vec<BodyStream<'a>> {
    let block_streams = scan.blocks.iter().flat_map(|block| {
        block.ps_streams.iter().filter_map(move |payload| {
            let header = crate::parasolid::stream_header(payload)?;
            let section = block.section.as_deref().unwrap_or("").to_ascii_lowercase();
            if crate::parasolid::is_body_stream(&header)
                && !section.contains("ghost")
                && !section.contains("resolvedfeatures")
            {
                Some(BodyStream {
                    origin: BodyOrigin::Block(block),
                    payload,
                    header,
                })
            } else {
                None
            }
        })
    });
    let compound_streams = scan.compound_streams.iter().flat_map(|stream| {
        stream.ps_streams.iter().filter_map(move |payload| {
            let header = crate::parasolid::stream_header(payload)?;
            let section = stream.path.to_ascii_lowercase();
            (crate::parasolid::is_body_stream(&header)
                && !section.contains("ghost")
                && !section.contains("resolvedfeatures"))
            .then_some(BodyStream {
                origin: BodyOrigin::Compound(stream),
                payload,
                header,
            })
        })
    });
    let mut streams = block_streams.chain(compound_streams).collect::<Vec<_>>();
    streams.sort_by_key(|stream| {
        let section = stream.origin.name().to_ascii_lowercase();
        (
            !section.contains("partition"),
            !stream
                .header
                .description
                .to_ascii_lowercase()
                .contains("partition"),
        )
    });
    streams
}

/// Decode the available Parasolid body streams into one B-rep. Returns `None`
/// when the streams frame but yield neither geometry nor a valid empty
/// partition/deltas model, so the caller falls back to metadata.
fn try_decode_brep(
    scan: &ContainerScan,
    streams: &[BodyStream<'_>],
) -> Option<(DecodedBrep, DecodeReport)> {
    let mut sites: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, stream) in streams.iter().enumerate() {
        sites
            .entry(stream.origin.site_key())
            .or_default()
            .push(index);
    }
    let mut decoded_sites = Vec::new();
    for (site, indices) in &sites {
        let first = indices[0];
        let name = streams[first].origin.name();
        let bodies: Vec<_> = indices
            .iter()
            .map(|index| (streams[*index].payload, &streams[*index].header))
            .collect();
        let decoded = brep::decode_bodies(&bodies, &name);
        decoded_sites.push((site.clone(), first, decoded));
    }
    if decoded_sites.is_empty() {
        return None;
    }
    let active_site = container::select_active_parasolid(scan)
        .map(|(block, _)| format!("block@{}", block.offset));
    let resolved_active_site = active_site
        .as_ref()
        .and_then(|active| decoded_sites.iter().position(|(site, _, _)| site == active));
    // Without a resolved active site, this is only a deterministic merge
    // accumulator. All site identities are qualified below.
    let selected_site = resolved_active_site.unwrap_or(0);
    let selected_is_empty_model = decoded_sites[selected_site].2.stats.source_entity_records == 0
        && sites[&decoded_sites[selected_site].0].iter().any(|index| {
            streams[*index]
                .header
                .description
                .to_ascii_lowercase()
                .contains("partition")
        })
        && sites[&decoded_sites[selected_site].0].iter().any(|index| {
            streams[*index]
                .header
                .description
                .to_ascii_lowercase()
                .contains("deltas")
        });
    let selected_has_geometry = !decoded_sites[selected_site].2.faces.is_empty()
        || !decoded_sites[selected_site].2.surfaces.is_empty()
        || !decoded_sites[selected_site].2.points.is_empty();
    if resolved_active_site.is_some() {
        if !selected_is_empty_model && !selected_has_geometry {
            return None;
        }
    } else {
        let any_site_has_geometry = decoded_sites.iter().any(|(_, _, decoded)| {
            !decoded.faces.is_empty() || !decoded.surfaces.is_empty() || !decoded.points.is_empty()
        });
        let any_empty_model = decoded_sites.iter().any(|(site, _, decoded)| {
            decoded.stats.source_entity_records == 0
                && sites[site].iter().any(|index| {
                    streams[*index]
                        .header
                        .description
                        .to_ascii_lowercase()
                        .contains("partition")
                })
                && sites[site].iter().any(|index| {
                    streams[*index]
                        .header
                        .description
                        .to_ascii_lowercase()
                        .contains("deltas")
                })
        });
        if !any_site_has_geometry && !any_empty_model {
            return None;
        }
    }
    let active_stream = resolved_active_site.map(|site| decoded_sites[site].1);
    let metadata_stream = active_stream.or_else(|| {
        let first = decoded_sites.first()?.1;
        let first_header = &streams[first].header;
        decoded_sites
            .iter()
            .all(|(_, representative, _)| {
                let header = &streams[*representative].header;
                header.schema == first_header.schema
                    && header.description == first_header.description
            })
            .then_some(first)
    });
    let (selected_site_key, selected, mut decoded) = decoded_sites.swap_remove(selected_site);
    if active_stream.is_none() {
        decoded.qualify_ids(&selected_site_key);
    }
    bind_opaque_geometry(&mut decoded, &streams[selected].origin.unknown_id());
    let mut configuration_bodies = Vec::new();
    if let Some(index) = configuration_index(&streams[selected].origin.name()) {
        configuration_bodies.push((
            index,
            decoded.bodies.iter().map(|body| body.id.clone()).collect(),
        ));
    }
    for (site, first, mut alternate) in decoded_sites {
        alternate.qualify_ids(&site);
        bind_opaque_geometry(&mut alternate, &streams[first].origin.unknown_id());
        if let Some(index) = configuration_index(&streams[first].origin.name()) {
            configuration_bodies.push((
                index,
                alternate
                    .bodies
                    .iter()
                    .map(|body| body.id.clone())
                    .collect(),
            ));
        }
        // Keep only the selected source's bridge sequence namespace. Alternate
        // configuration sites are qualified into the model but do not own the
        // active SWIFT CadIdentifier lane.
        merge_brep(&mut decoded, alternate);
    }
    let report = build_geometry_report(scan, &decoded);
    Some((
        DecodedBrep {
            metadata_stream,
            brep: decoded,
            configuration_bodies,
        },
        report,
    ))
}

fn bind_opaque_geometry(brep: &mut Brep, source: &UnknownId) {
    for surface in &mut brep.surfaces {
        if let SurfaceGeometry::Unknown { record } = &mut surface.geometry {
            if record.is_none() {
                *record = Some(source.clone());
            }
        }
    }
    for curve in &mut brep.curves {
        if let cadmpeg_ir::geometry::CurveGeometry::Unknown { record } = &mut curve.geometry {
            if record.is_none() {
                *record = Some(source.clone());
            }
        }
    }
}

fn merge_brep(target: &mut Brep, mut source: Brep) {
    // Sequence links are source-local and belong only to the selected SWIFT
    // source. Alternate configuration sequences must not enter its namespace.
    let stream_base = target.annotations.streams.len() as u32;
    target
        .annotations
        .streams
        .append(&mut source.annotations.streams);
    for provenance in source.annotations.provenance.values_mut() {
        provenance.stream += stream_base;
    }
    target
        .annotations
        .provenance
        .append(&mut source.annotations.provenance);
    target
        .annotations
        .exactness
        .append(&mut source.annotations.exactness);
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
    target.body_modifiers.append(&mut source.body_modifiers);
    target.stats.unknown_surface_faces += source.stats.unknown_surface_faces;
    target.stats.unknown_procedural_supports += source.stats.unknown_procedural_supports;
    target.stats.unknown_curve_edges += source.stats.unknown_curve_edges;
    target.stats.ambiguous_pcurve_parameters += source.stats.ambiguous_pcurve_parameters;
    target.stats.off_surface_nurbs_pcurves += source.stats.off_surface_nurbs_pcurves;
    target.stats.source_entity_records += source.stats.source_entity_records;
    target.stats.ambiguous_body_assignments += source.stats.ambiguous_body_assignments;
    target.stats.unresolved_face_colors += source.stats.unresolved_face_colors;
    target.stats.ambiguous_face_owners += source.stats.ambiguous_face_owners;
    target.stats.unclaimed_faces += source.stats.unclaimed_faces;
    target.stats.synthetic_body_grouping |= source.stats.synthetic_body_grouping;
}

fn ensure_display_appearance(
    ir: &mut CadIr,
    definition: &crate::appearance::AppearanceDefinition,
    section_ordinal: usize,
    annotations: &mut Annotations,
) -> AppearanceId {
    if let Some(existing) = ir.model.appearances.iter().find(|appearance| {
        appearance.name.as_deref() == Some(definition.name.as_str())
            && appearance.base_color == Some(definition.color)
    }) {
        return existing.id.clone();
    }
    let id = AppearanceId(format!(
        "sldprt:appearance:displaylist#{section_ordinal}:{}",
        definition.record_offset
    ));
    crate::annotations::note(
        annotations,
        id.0.clone(),
        definition.source_name.clone(),
        definition.record_offset as u64,
        "displaylist_visual_properties",
        Exactness::ByteExact,
    );
    ir.model.appearances.push(Appearance {
        id: id.clone(),
        name: Some(definition.name.clone()),
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
    id
}

fn build_geometry_ir(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    header: Option<&StreamHeader>,
    mut brep: Brep,
    configuration_bodies: &[(usize, Vec<cadmpeg_ir::ids::BodyId>)],
    admitted_entities: &mut u64,
) -> Result<
    (
        CadIr,
        Annotations,
        Vec<UnknownRecord>,
        Vec<cadmpeg_ir::LossNote>,
    ),
    CodecError,
> {
    let mut ir = CadIr::empty(Units::default());
    let appearance_definitions = crate::appearance::definitions(scan);
    ir.source = Some(source_meta(scan, header));
    let mut annotations = std::mem::take(&mut brep.annotations);
    let mut histories = crate::history::histories(scan, &mut annotations);
    let mut lanes = crate::resolved_features::assembly::lanes(scan, &mut annotations);
    let mut supplemental_config_lanes =
        crate::resolved_features::assembly::supplemental_config_lanes(scan, &mut annotations);
    let form_padding = ir.source.as_ref().and_then(|source| {
        crate::resolved_features::operations::form_code_padding(
            source.attributes.get("sw_version").map(String::as_str),
        )
    });
    crate::resolved_features::classes::bind_history_classes(&mut histories, &lanes);
    crate::resolved_features::bindings::bind_scalar_operands(&histories, &mut lanes);
    crate::resolved_features::bindings::bind_scalar_operands(
        &histories,
        &mut supplemental_config_lanes,
    );
    let mut pmi_losses = Vec::new();
    let pmi_dimensions = crate::pmi::dimensions(scan, &mut annotations, &mut pmi_losses);
    project_design_history(&mut ir, &histories, &lanes, &pmi_dimensions, scan);
    crate::resolved_features::operations::bind_feature_operations(
        &mut ir.model.features,
        &histories,
        &lanes,
        form_padding,
    );
    crate::pmi::apply_to_parameters(
        &mut ir.model.parameters,
        &ir.model.features,
        &pmi_dimensions,
    );
    crate::resolved_features::projections::bind_parameter_scalars(
        &mut ir.model.parameters,
        &ir.model.features,
        &histories,
        parameter_identity_lanes(&lanes),
    );
    crate::resolved_features::projections::type_display_relation_parameters(
        &mut ir.model.parameters,
        &ir.model.features,
        &lanes,
    );
    crate::history::align_configuration_parameter_kinds(&mut ir);
    complete_resolved_configuration_parameter_snapshots(&mut ir);
    stamp_parameter_baseline(&mut ir);
    let (mut sketches, mut sketch_entities, mut sketch_constraints) =
        crate::resolved_features::sketch_projection::sketches(scan, &mut annotations);
    crate::resolved_features::profiles::bind_sketch_profiles(
        &mut ir.model.features,
        &mut sketches,
        &mut sketch_entities,
        &mut sketch_constraints,
        &ir.model.parameters,
        &histories,
        &lanes,
        &mut annotations,
    );
    crate::resolved_features::bindings::bind_unresolved_detached_sketch_objects(
        &ir.model.features,
        &histories,
        &mut supplemental_config_lanes,
    );
    crate::resolved_features::projections::project_compact_edge_selections(
        &mut ir.model.features,
        &[],
        &supplemental_config_lanes,
    );
    crate::history::project_configuration_supplemental_edge_selections(
        &mut ir,
        &supplemental_config_lanes,
    );
    crate::resolved_features::profiles::project_compact_sketch_profiles(
        &mut ir.model.features,
        &mut sketches,
        &mut sketch_entities,
        &histories,
        &lanes,
    );
    // Marker-backed sketches can originate in either lane family. Their
    // geometry and constraints must use the same complete lane set.
    let mut sketch_lanes = lanes.clone();
    sketch_lanes.extend(supplemental_config_lanes.clone());
    let (spatial_sketches, spatial_sketch_entities) =
        crate::resolved_features::markers::spatial_sketches(
            &mut ir.model.features,
            &histories,
            &sketch_lanes,
        );
    ir.model.spatial_sketches = spatial_sketches;
    ir.model.spatial_sketch_entities = spatial_sketch_entities;
    crate::resolved_features::profiles::project_marker_backed_sketches(
        &mut ir.model.features,
        &mut sketches,
        &mut sketch_entities,
        &histories,
        &sketch_lanes,
    );
    crate::resolved_features::profiles::project_sketch_block_profiles(
        &mut ir.model.features,
        &mut sketches,
        &mut sketch_entities,
        &histories,
        &sketch_lanes,
    );
    crate::history::bind_unique_sketch_feature(&mut ir.model.features, &sketches, &histories);
    crate::resolved_features::component_paths::project_dissected_sketches(
        &mut ir.model.features,
        &sketches,
        &histories,
    );
    crate::resolved_features::axes::bind_profile_revolution_axes(
        &mut ir.model.features,
        &histories,
        &lanes,
        &sketches,
        &brep.surfaces,
    );
    crate::resolved_features::bindings::bind_pattern_inputs(
        &mut ir.model.features,
        &histories,
        &lanes,
    );
    crate::resolved_features::bindings::bind_sweep_adjacent_profiles(
        &mut ir.model.features,
        &histories,
        &lanes,
    );
    crate::resolved_features::dimensions::project_dimensioned_sketch_geometry(
        &mut sketch_entities,
        &sketches,
        &brep.surfaces,
        &ir.model.features,
        &ir.model.parameters,
        &sketch_lanes,
    );
    crate::resolved_features::dimensions::project_marker_dimensioned_circles(
        &mut sketch_entities,
        &mut sketches,
        &ir.model.features,
        &ir.model.parameters,
        &sketch_lanes,
    );
    crate::resolved_features::relation_geometry::project_relation_point_geometry(
        &mut sketch_entities,
        &sketches,
        &ir.model.features,
        &sketch_lanes,
    );
    crate::resolved_features::dimensions::project_relation_point_dimensioned_circles(
        &mut sketch_entities,
        &ir.model.features,
        &ir.model.parameters,
        &sketch_lanes,
    );
    crate::resolved_features::relation_geometry::project_relation_solved_line_geometry(
        &mut sketch_entities,
        &sketches,
        &ir.model.features,
        &ir.model.parameters,
        &sketch_lanes,
    );
    crate::resolved_features::relation_geometry::project_relation_solved_point_geometry(
        &mut sketch_entities,
        &sketches,
        &ir.model.features,
        &ir.model.parameters,
        &sketch_lanes,
    );
    crate::resolved_features::relation_geometry::project_relation_bindings(
        &mut sketch_constraints,
        &sketches,
        &ir.model.features,
        &sketch_entities,
        &ir.model.parameters,
        &sketch_lanes,
    );
    stamp_feature_baseline(&mut ir);
    let mut attributes = crate::metadata::attributes(scan, &mut annotations);
    attributes.extend(crate::history::custom_property_attributes(&histories));
    lanes.extend(supplemental_config_lanes);
    let mut native = crate::native::SldprtNative {
        version: crate::native::SLDPRT_NATIVE_VERSION,
        feature_histories: histories.clone(),
        feature_input_lanes: lanes,
        pmi_dimensions,
    };
    ir.model.attributes = attributes;
    ir.model.sketches = sketches;
    ir.model.sketch_entities = sketch_entities;
    ir.model.sketch_constraints = sketch_constraints;
    stamp_sketch_baseline(&mut ir, &native);

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
    let face_identities = brep
        .face_atoms
        .iter()
        .filter_map(|atom| {
            atom.target
                .clone()
                .map(|target| (target, atom.feature_source_id, atom.local_face_id))
        })
        .collect::<Vec<_>>();
    let persistent_face_identities = brep
        .face_atoms
        .iter()
        .filter_map(|atom| {
            atom.target.clone().map(|target| {
                (
                    target,
                    crate::brep::PersistentFaceIdentity {
                        feature_source_id: atom.feature_source_id,
                        local_id: atom.local_face_id,
                        trailing_fields: atom.persistent_tail.clone(),
                    },
                )
            })
        })
        .collect::<Vec<_>>();
    let face_producers = face_identities
        .iter()
        .map(|(target, source, _)| (target.clone(), *source))
        .collect::<Vec<_>>();
    let body_modifiers = brep
        .body_modifiers
        .iter()
        .filter_map(|modifier| {
            modifier
                .target
                .clone()
                .map(|target| (target, modifier.history_ordinal))
        })
        .collect::<Vec<_>>();
    crate::history::derive_feature_outputs(
        &mut ir.model.features,
        &histories,
        &face_producers,
        &body_modifiers,
        &ir.model.faces,
        &ir.model.shells,
        &ir.model.regions,
    );
    let topology_selection_inputs = crate::history::TopologySelectionInputs {
        bodies: &ir.model.bodies,
        faces: &ir.model.faces,
        surfaces: &ir.model.surfaces,
        edges: &ir.model.edges,
        curves: &ir.model.curves,
        lanes: &native.feature_input_lanes,
        face_identities: &face_identities,
    };
    crate::history::bind_topology_selections(
        &mut ir.model.features,
        &histories,
        &topology_selection_inputs,
    );
    crate::resolved_features::bindings::bind_mirror_surface_planes(
        &mut ir.model.features,
        &histories,
        &native.feature_input_lanes,
        &face_identities,
        &ir.model.faces,
        &ir.model.surfaces,
    );
    crate::resolved_features::holes::project_profiled_hole_constructions(
        &mut ir.model.features,
        &ir.model.sketch_entities,
        &histories,
        &native.feature_input_lanes,
    );
    crate::resolved_features::holes::project_hole_position_sketches(
        &mut ir.model.features,
        &ir.model.sketches,
        &ir.model.sketch_entities,
        &histories,
        &native.feature_input_lanes,
    );
    crate::resolved_features::holes::project_spatial_hole_position_sketches(
        &mut ir.model.features,
        &ir.model.spatial_sketches,
        &ir.model.spatial_sketch_entities,
        &ir.model.surfaces,
        &histories,
        &native.feature_input_lanes,
    );
    crate::resolved_features::holes::project_generated_hole_axes(
        &mut ir.model.features,
        &histories,
        &native.feature_input_lanes,
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
    );
    crate::resolved_features::holes::project_hole_axes(
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
        &native.feature_input_lanes,
    );
    crate::resolved_features::holes::project_hole_topology_axes(
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
    );
    crate::resolved_features::holes::project_bore_backed_position_sketches(
        &mut ir.model.features,
        &mut ir.model.sketches,
        &mut ir.model.sketch_entities,
        &ir.model.surfaces,
        &histories,
        &native.feature_input_lanes,
    );
    crate::resolved_features::relation_geometry::project_relation_bindings(
        &mut ir.model.sketch_constraints,
        &ir.model.sketches,
        &ir.model.features,
        &ir.model.sketch_entities,
        &ir.model.parameters,
        &native.feature_input_lanes,
    );
    crate::history::order_features_for_regeneration(&mut ir.model.features);
    assign_configuration_bodies(&mut ir, configuration_bodies);
    crate::history::project_configuration_sketch_states(
        &mut ir,
        &histories,
        &native.feature_input_lanes,
        &mut annotations,
    );
    crate::history::bind_configuration_topology_selections(
        &mut ir,
        &histories,
        &native.feature_input_lanes,
        &face_identities,
    );
    mark_active_configuration(&mut ir);
    crate::resolved_features::projections::project_unbound_cosmetic_thread_faces(
        &mut ir.model.features,
        &histories,
        &native.feature_input_lanes,
        &ir.model.faces,
        &ir.model.surfaces,
    );
    crate::resolved_features::projections::project_unbound_offset_plane_faces(
        &mut ir.model.features,
        &ir.model.faces,
        &ir.model.surfaces,
    );
    crate::history::inherit_configuration_reference_plane_states(&mut ir);
    sync_active_configuration_resolutions(&mut ir);
    crate::history::order_model_features_for_regeneration(&mut ir);
    let pattern_hole_nominals = crate::swift::pattern_hole_nominal_context(&ir.model.features);
    ir.model.pmi = crate::swift::annotations(
        scan,
        &mut annotations,
        Some(&topology_index),
        Some(&pattern_hole_nominals),
    );
    stamp_feature_baseline(&mut ir);
    assign_native_configuration_indices(&ir, &mut native);
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            "sldprt_native_configuration_sha256".into(),
            crate::history::native_configuration_hash(&native.feature_histories),
        );
        source.attributes.insert(
            "sldprt_native_history_sha256".into(),
            crate::history::history_hash(&native.feature_histories),
        );
    }
    ctx.admit_entities(
        ir.model.entity_count() as u64,
        admitted_entities,
        "admit SLDPRT entities",
    )?;
    native.store(ir.native.namespace_mut("sldprt"))?;
    // Stamp baseline before fabricating the read-side configuration snapshot.
    stamp_configuration_baseline(&mut ir);
    snapshot_active_configuration(&mut ir);
    let mut unknowns = brep.unknowns;
    let annotation_source = header.map_or("unresolved Parasolid stream", |header| {
        header.description.as_str()
    });
    for face_color in brep.face_colors {
        let id = AppearanceId(format!(
            "sldprt:appearance:entity53#{}",
            face_color.color_attr
        ));
        crate::annotations::note(
            &mut annotations,
            id.0.clone(),
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
            let site = target
                .split_once('@')
                .map(|(_, site)| format!("@{site}"))
                .unwrap_or_default();
            let binding_id = format!(
                "sldprt:appearance:binding#face:{}:{}{}",
                face_color.face_attr, face_color.color_attr, site
            );
            if !ir
                .model
                .appearance_bindings
                .iter()
                .any(|binding| binding.id == binding_id)
            {
                ir.model.appearance_bindings.push(AppearanceBinding {
                    id: binding_id,
                    target: AppearanceTarget::Face(cadmpeg_ir::ids::FaceId(target)),
                    appearance: id,
                    source_entity_id: Some(face_color.face_attr.to_string()),
                    object_type: Some("Face".into()),
                    channels: BTreeMap::new(),
                });
            }
        }
    }
    for (index, definition) in appearance_definitions.into_iter().enumerate() {
        let id = AppearanceId(format!("sldprt:appearance:material#{index}"));
        crate::annotations::note(
            &mut annotations,
            id.0.clone(),
            definition.source_name,
            definition.record_offset as u64,
            "moVisualProperties_c",
            Exactness::ByteExact,
        );
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
    let feature_appearance_sources = crate::appearance::feature_assignments(scan)
        .into_iter()
        .map(|assignment| assignment.feature_source_id)
        .collect::<BTreeSet<_>>();
    let mut matched_feature_sources = BTreeSet::new();
    let mut conflicting_display_references = Vec::new();
    let mut persistent_face_bindings = Vec::new();
    for display in scan.sections() {
        let display_faces = crate::tessellation::section_display_faces(display);
        if display_faces.is_empty() {
            continue;
        }
        for face in &display_faces {
            let candidates = face
                .surface_references
                .iter()
                .map(crate::tessellation::PersistentSurfaceReference::feature_source_id)
                .collect::<BTreeSet<_>>();
            if candidates.len() > 1 {
                conflicting_display_references.push(format!(
                    "{}::DisplayFace[{}] ({})",
                    display.display_name(),
                    face.table_index,
                    candidates
                        .iter()
                        .map(u32::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
        let resolved =
            crate::appearance::resolve_display_appearances(scan, display, &display_faces);
        matched_feature_sources.extend(resolved.matched_feature_sources);
        let mut display_links = Vec::with_capacity(display_faces.len());
        for display_face in display_faces {
            let id = format!(
                "sldprt:displaylist:record#{}:{}",
                display.ordinal(),
                display_face.table_index
            );
            if let Some(identity) = display_face.persistent_surface_identity() {
                persistent_face_bindings.push(crate::tessellation::PersistentFaceBinding {
                    tessellation: id.clone(),
                    identity,
                });
            }
            let display_stream = display.display_name();
            crate::annotations::note(
                &mut annotations,
                id.clone(),
                display_stream,
                display_face.table.start as u64,
                "displaylist_tessellation",
                Exactness::ByteExact,
            );
            display_links.push(id.clone());
            if let Some(definition) = resolved.by_face.get(&display_face.table_index) {
                let appearance = ensure_display_appearance(
                    &mut ir,
                    definition,
                    display.ordinal(),
                    &mut annotations,
                );
                ir.model.appearance_bindings.push(AppearanceBinding {
                    id: format!(
                        "sldprt:appearance:binding#display:{}:{}",
                        display.ordinal(),
                        display_face.table_index
                    ),
                    target: AppearanceTarget::Tessellation(id.clone()),
                    appearance,
                    source_entity_id: Some(format!(
                        "{}::DisplayFace[{}]",
                        display.display_name(),
                        display_face.table_index
                    )),
                    object_type: Some("DisplayFace".into()),
                    channels: BTreeMap::new(),
                });
            }
            let mesh = display_face.mesh;
            ir.model
                .tessellations
                .push(cadmpeg_ir::tessellation::Tessellation {
                    id,
                    body: None,
                    faces: Vec::new(),
                    chordal_deflection: None,
                    source_object: None,
                    vertices: mesh.vertices,
                    triangles: mesh.triangles,
                    feature_edges: Vec::new(),
                    strip_lengths: mesh.strip_lengths,
                    normals: mesh.normals,
                    corner_normals: Vec::new(),
                    triangle_groups: Vec::new(),
                    texture_assignments: Vec::new(),
                    channels: mesh.channels,
                });
        }
        let display_id = format!("sldprt:displaylist:record#{}", display.ordinal());
        crate::annotations::note(
            &mut annotations,
            display_id.clone(),
            display.display_name(),
            0,
            "displaylist_tessellation",
            Exactness::Unknown,
        );
        unknowns.push(UnknownRecord {
            id: UnknownId(display_id),
            offset: 0,
            byte_len: display.payload().len() as u64,
            sha256: sha256_hex(display.payload()),
            data: Some(display.payload().to_vec()),
            links: display_links,
        });
    }
    let unmatched_feature_sources = feature_appearance_sources
        .difference(&matched_feature_sources)
        .copied()
        .collect::<Vec<_>>();
    if !unmatched_feature_sources.is_empty() || !conflicting_display_references.is_empty() {
        let mut reasons = Vec::new();
        if !unmatched_feature_sources.is_empty() {
            reasons.push(format!(
                "feature source ID(s) {} have no agreeing DisplayFace persistent reference",
                unmatched_feature_sources
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !conflicting_display_references.is_empty() {
            reasons.push(format!(
                "conflicting references rejected for {}",
                conflicting_display_references.join("; ")
            ));
        }
        pmi_losses.push(SldprtLossCode::AppearanceAssignmentUnresolved.note(format!(
            "VisualStates feature appearance assignment unresolved: {}.",
            reasons.join("; ")
        )));
    }
    let mut assigned_tessellations = crate::tessellation::assign_persistent_owners(
        &mut ir.model,
        &persistent_face_identities,
        &persistent_face_bindings,
    );
    assigned_tessellations.extend(crate::tessellation::assign_unique_analytic_owners(
        &mut ir.model,
    ));
    for id in assigned_tessellations {
        let note = annotations.exactness.entry(id).or_default();
        note.fields.insert("body".into(), Exactness::Derived);
        note.fields.insert("faces".into(), Exactness::Derived);
    }
    for source_block in &scan.blocks {
        if unknowns
            .iter()
            .any(|record| record.id.0 == format!("sldprt:file:block#{}", source_block.offset))
        {
            continue;
        }
        let id = format!("sldprt:file:block#{}", source_block.offset);
        crate::annotations::note(
            &mut annotations,
            id.clone(),
            source_block
                .section
                .clone()
                .unwrap_or_else(|| format!("block@{}", source_block.offset)),
            source_block.offset as u64,
            source_block.family,
            Exactness::ByteExact,
        );
        unknowns.push(UnknownRecord {
            id: UnknownId(id),
            offset: 0,
            byte_len: source_block.payload.len() as u64,
            sha256: sha256_hex(&source_block.payload),
            data: Some(source_block.payload.clone()),
            links: Vec::new(),
        });
    }
    for source_stream in &scan.compound_streams {
        let id = format!("sldprt:file:compound-stream#{}", source_stream.directory_id);
        crate::annotations::note(
            &mut annotations,
            id.clone(),
            source_stream.path.clone(),
            0,
            container::payload_family(&source_stream.payload),
            Exactness::ByteExact,
        );
        unknowns.push(UnknownRecord {
            id: UnknownId(id),
            offset: 0,
            byte_len: source_stream.payload.len() as u64,
            sha256: sha256_hex(&source_stream.payload),
            data: Some(source_stream.payload.clone()),
            links: Vec::new(),
        });
    }
    let mut opaque_links = BTreeMap::<String, Vec<String>>::new();
    for surface in &ir.model.surfaces {
        if let SurfaceGeometry::Unknown {
            record: Some(record),
        } = &surface.geometry
        {
            opaque_links
                .entry(record.0.clone())
                .or_default()
                .push(surface.id.0.clone());
        }
    }
    for curve in &ir.model.curves {
        if let cadmpeg_ir::geometry::CurveGeometry::Unknown {
            record: Some(record),
        } = &curve.geometry
        {
            opaque_links
                .entry(record.0.clone())
                .or_default()
                .push(curve.id.0.clone());
        }
    }
    for (record_id, links) in opaque_links {
        let source = unknowns
            .iter_mut()
            .find(|record| record.id.0 == record_id)
            .expect("opaque geometry source is retained");
        source.links.extend(links);
    }
    preserve_source_image(scan, &mut annotations, &mut unknowns);
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

fn source_meta(scan: &ContainerScan, header: Option<&StreamHeader>) -> SourceMeta {
    let mut attributes = BTreeMap::new();
    attributes.insert(
        "outer_version".to_string(),
        format!("0x{:08x}", scan.version),
    );
    let display = crate::tessellation::summary(scan);
    if display.vertices > 0 {
        attributes.insert(
            "displaylist_vertices".to_string(),
            display.vertices.to_string(),
        );
        attributes.insert(
            "displaylist_triangles".to_string(),
            display.triangles.to_string(),
        );
    }
    attributes.insert("block_count".to_string(), scan.blocks.len().to_string());
    attributes.insert(
        "compound_stream_count".to_string(),
        scan.compound_streams.len().to_string(),
    );
    let active_name = container::active_parasolid_summary(scan).map(|(name, _, _)| name);
    if let Some(active_name) = active_name {
        attributes.insert("active_parasolid_block".to_string(), active_name);
    } else {
        attributes.insert("sldprt_active_partition_unresolved".into(), "true".into());
    }
    if let Some(header) = header {
        attributes.insert("parasolid_schema".to_string(), header.schema.clone());
        attributes.insert(
            "parasolid_description".to_string(),
            header.description.clone(),
        );
    }
    add_preview_metadata(scan, &mut attributes);
    add_solidworks_xml_metadata(scan, &mut attributes);
    SourceMeta {
        format: "sldprt".to_string(),
        attributes,
    }
}

fn add_preview_metadata(scan: &ContainerScan, attributes: &mut BTreeMap<String, String>) {
    let mut png_index = 0;
    let mut bmp_index = 0;
    for section in scan.sections() {
        let payload = section.payload();
        match container::payload_family(payload) {
            "png-preview" => {
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
                let prefix = format!("png_preview_{png_index}");
                attributes.insert(format!("{prefix}_width"), width.to_string());
                attributes.insert(format!("{prefix}_height"), height.to_string());
                attributes.insert(format!("{prefix}_bit_depth"), fields[0].to_string());
                attributes.insert(format!("{prefix}_color_type"), fields[1].to_string());
                attributes.insert(format!("{prefix}_compression"), fields[2].to_string());
                attributes.insert(format!("{prefix}_filter"), fields[3].to_string());
                attributes.insert(format!("{prefix}_interlace"), fields[4].to_string());
                png_index += 1;
            }
            "bmp-thumbnail" => {
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
                let prefix = format!("bmp_thumbnail_{bmp_index}");
                attributes.insert(format!("{prefix}_width"), width.to_string());
                attributes.insert(format!("{prefix}_height"), height.to_string());
                attributes.insert(format!("{prefix}_planes"), planes.to_string());
                attributes.insert(format!("{prefix}_bit_count"), bits_per_pixel.to_string());
                attributes.insert(format!("{prefix}_compression"), compression.to_string());
                attributes.insert(format!("{prefix}_image_size"), image_size.to_string());
                bmp_index += 1;
            }
            _ => {}
        }
    }
    attributes.insert("png_preview_count".into(), png_index.to_string());
    attributes.insert("bmp_thumbnail_count".into(), bmp_index.to_string());
}

fn add_solidworks_xml_metadata(scan: &ContainerScan, attributes: &mut BTreeMap<String, String>) {
    for section in scan.sections() {
        let payload = section.payload();
        if container::payload_family(payload) != "xml"
            || !payload.windows(12).any(|w| w == b"swSolidWorks")
        {
            continue;
        }
        let Ok(text) = std::str::from_utf8(payload) else {
            continue;
        };
        let Ok(document) = roxmltree::Document::parse(text) else {
            continue;
        };
        let root = document.root_element();
        if root.tag_name().name() != "swSolidWorks" {
            continue;
        }
        for (source, target) in [
            ("swVersion", "sw_version"),
            ("swCreationTime", "sw_creation_time_unix"),
            ("swPath", "sw_path"),
        ] {
            if let Some(value) = root.attribute(source) {
                attributes.insert(target.into(), value.into());
            }
        }
        if let Some(model) = root.descendants().find(|node| node.has_tag_name("swModel")) {
            if let Some(value) = model.attribute("swName") {
                attributes.insert("sw_name".into(), value.into());
            }
            if let Some(value) = model.attribute("swConfigurationName") {
                attributes.insert("sw_configuration_name".into(), value.into());
            }
        }
        for configuration in root
            .descendants()
            .filter(|node| node.has_tag_name("swConfiguration"))
        {
            let Some(slot) = configuration.attribute("swID") else {
                continue;
            };
            if !slot.bytes().all(|byte| byte.is_ascii_digit()) {
                continue;
            }
            for (source, target) in [
                ("swConfigurationNeedsUpdate", "needs_update"),
                ("swMostRecentConfiguration", "most_recent"),
                ("swConfigurationFlags", "flags"),
                ("swConfigurationAlternateName", "alternate_name"),
            ] {
                if let Some(value) = configuration.attribute(source) {
                    attributes.insert(
                        format!("sw_configuration_{slot}_{target}"),
                        value.to_string(),
                    );
                }
            }
        }
        break;
    }
}

fn build_geometry_report(scan: &ContainerScan, decoded: &Brep) -> DecodeReport {
    let s = &decoded.stats;
    let mut losses = Vec::new();

    if s.unknown_surface_faces > 0 || s.unknown_procedural_supports > 0 {
        let mut message = Vec::new();
        if s.unknown_surface_faces > 0 {
            message.push(format!(
                "{} face(s) rest on a support surface this codec does not type (swept, blended, \
                 intersection, spline-on-surface, or another unsupported family); the face, its \
                 loops, and trims are emitted with an unknown-geometry surface linking to the \
                 preserved record bytes. Topology is transferred; the underlying surface shape \
                 is not.",
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
            "{} pcurve(s) were withheld because more than one geometric parameter satisfies the stored edge or ruling geometry; the decoder does not choose by residual order.",
            s.ambiguous_pcurve_parameters
        )));
    }
    if s.off_surface_nurbs_pcurves > 0 {
        losses.push(SldprtLossCode::TopologyPcurveCarrierOffSurface.note(format!(
            "{} NURBS edge carrier(s) have vertex ranges off their bound B-spline surface; pcurve derivation is withheld because the defect is upstream of parameter-space geometry.",
            s.off_surface_nurbs_pcurves
        )));
    }
    if s.ambiguous_body_assignments > 0 {
        losses.push(SldprtLossCode::TopologyBodyAssignmentAmbiguous.note(format!(
            "{} schema-33103 body head(s) have tied face-component overlap; their component assignments remain unresolved.",
            s.ambiguous_body_assignments
        )));
    }
    if s.unresolved_face_colors > 0 {
        losses.push(SldprtLossCode::AppearanceFaceColorUnresolved.note(format!(
            "{} face-color binding(s) were withheld because the current face and link records do not select one consistent framed color record.",
            s.unresolved_face_colors
        )));
    }
    if s.ambiguous_face_owners > 0 {
        losses.push(SldprtLossCode::TopologyFaceOwnerAmbiguous.note(format!(
            "{} face owner(s) have non-equivalent bridge uses; all uses for each owner remain unresolved.",
            s.ambiguous_face_owners
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
    append_swift_pmi_losses(scan, &mut losses);
    DecodeReport {
        format: "sldprt".to_string(),
        container_only: false,
        geometry_transferred: true,
        coverage: std::collections::BTreeMap::new(),
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
        losses,
        notes: container::summarize(scan).notes,
    }
}

fn build_metadata_ir(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    admitted_entities: &mut u64,
) -> Result<
    (
        CadIr,
        Annotations,
        Vec<UnknownRecord>,
        Vec<cadmpeg_ir::LossNote>,
    ),
    CodecError,
> {
    let mut ir = CadIr::empty(Units::default());
    let mut unknowns = Vec::new();
    let mut annotations = Annotations::default();
    let mut histories = crate::history::histories(scan, &mut annotations);
    let mut lanes = crate::resolved_features::assembly::lanes(scan, &mut annotations);
    let mut supplemental_config_lanes =
        crate::resolved_features::assembly::supplemental_config_lanes(scan, &mut annotations);
    crate::resolved_features::classes::bind_history_classes(&mut histories, &lanes);
    crate::resolved_features::bindings::bind_scalar_operands(&histories, &mut lanes);
    crate::resolved_features::bindings::bind_scalar_operands(
        &histories,
        &mut supplemental_config_lanes,
    );
    let mut pmi_losses = Vec::new();
    let pmi_dimensions = crate::pmi::dimensions(scan, &mut annotations, &mut pmi_losses);
    ir.model.pmi = crate::swift::annotations(scan, &mut annotations, None, None);
    let (sketches, sketch_entities, sketch_constraints) =
        crate::resolved_features::sketch_projection::sketches(scan, &mut annotations);
    let mut model_attributes = crate::metadata::attributes(scan, &mut annotations);
    model_attributes.extend(crate::history::custom_property_attributes(&histories));
    ir.model.attributes = model_attributes;
    ir.model.sketches = sketches;
    ir.model.sketch_entities = sketch_entities;
    ir.model.sketch_constraints = sketch_constraints;
    let mut attributes = BTreeMap::new();
    attributes.insert(
        "outer_version".to_string(),
        format!("0x{:08x}", scan.version),
    );
    attributes.insert("block_count".to_string(), scan.blocks.len().to_string());
    add_solidworks_xml_metadata(scan, &mut attributes);

    if let Some((block, header)) = container::select_active_parasolid(scan) {
        attributes.insert(
            "active_parasolid_block".to_string(),
            block
                .section
                .clone()
                .unwrap_or_else(|| format!("block@{}", block.offset)),
        );
        attributes.insert("parasolid_schema".to_string(), header.schema.clone());
        let id = format!("sldprt:file:block#{}", block.offset);
        crate::annotations::note(
            &mut annotations,
            id.clone(),
            block
                .section
                .clone()
                .unwrap_or_else(|| format!("block@{}", block.offset)),
            0,
            "parasolid_stream",
            Exactness::Unknown,
        );
        unknowns.push(UnknownRecord {
            id: UnknownId(id),
            offset: block.offset as u64,
            byte_len: block.uncomp_sz as u64,
            sha256: sha256_hex(&block.payload),
            data: Some(block.payload.clone()),
            links: Vec::new(),
        });
    }

    ir.source = Some(SourceMeta {
        format: "sldprt".to_string(),
        attributes,
    });
    project_design_history(&mut ir, &histories, &lanes, &pmi_dimensions, scan);
    let form_padding = ir.source.as_ref().and_then(|source| {
        crate::resolved_features::operations::form_code_padding(
            source.attributes.get("sw_version").map(String::as_str),
        )
    });
    crate::resolved_features::operations::bind_feature_operations(
        &mut ir.model.features,
        &histories,
        &lanes,
        form_padding,
    );
    crate::pmi::apply_to_parameters(
        &mut ir.model.parameters,
        &ir.model.features,
        &pmi_dimensions,
    );
    crate::resolved_features::projections::bind_parameter_scalars(
        &mut ir.model.parameters,
        &ir.model.features,
        &histories,
        parameter_identity_lanes(&lanes),
    );
    crate::resolved_features::projections::type_display_relation_parameters(
        &mut ir.model.parameters,
        &ir.model.features,
        &lanes,
    );
    crate::history::align_configuration_parameter_kinds(&mut ir);
    complete_resolved_configuration_parameter_snapshots(&mut ir);
    stamp_parameter_baseline(&mut ir);
    crate::resolved_features::profiles::bind_sketch_profiles(
        &mut ir.model.features,
        &mut ir.model.sketches,
        &mut ir.model.sketch_entities,
        &mut ir.model.sketch_constraints,
        &ir.model.parameters,
        &histories,
        &lanes,
        &mut annotations,
    );
    crate::resolved_features::bindings::bind_unresolved_detached_sketch_objects(
        &ir.model.features,
        &histories,
        &mut supplemental_config_lanes,
    );
    crate::resolved_features::projections::project_compact_edge_selections(
        &mut ir.model.features,
        &[],
        &supplemental_config_lanes,
    );
    crate::history::project_configuration_supplemental_edge_selections(
        &mut ir,
        &supplemental_config_lanes,
    );
    crate::resolved_features::profiles::project_compact_sketch_profiles(
        &mut ir.model.features,
        &mut ir.model.sketches,
        &mut ir.model.sketch_entities,
        &histories,
        &lanes,
    );
    // Marker-backed sketches can originate in either lane family. Their
    // geometry and constraints must use the same complete lane set.
    let mut sketch_lanes = lanes.clone();
    sketch_lanes.extend(supplemental_config_lanes.clone());
    let (spatial_sketches, spatial_sketch_entities) =
        crate::resolved_features::markers::spatial_sketches(
            &mut ir.model.features,
            &histories,
            &sketch_lanes,
        );
    ir.model.spatial_sketches = spatial_sketches;
    ir.model.spatial_sketch_entities = spatial_sketch_entities;
    crate::resolved_features::profiles::project_marker_backed_sketches(
        &mut ir.model.features,
        &mut ir.model.sketches,
        &mut ir.model.sketch_entities,
        &histories,
        &sketch_lanes,
    );
    crate::resolved_features::profiles::project_sketch_block_profiles(
        &mut ir.model.features,
        &mut ir.model.sketches,
        &mut ir.model.sketch_entities,
        &histories,
        &sketch_lanes,
    );
    crate::history::bind_unique_sketch_feature(
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
        &lanes,
        &ir.model.sketches,
        &ir.model.surfaces,
    );
    crate::resolved_features::bindings::bind_pattern_inputs(
        &mut ir.model.features,
        &histories,
        &lanes,
    );
    crate::resolved_features::bindings::bind_sweep_adjacent_profiles(
        &mut ir.model.features,
        &histories,
        &lanes,
    );
    crate::resolved_features::dimensions::project_dimensioned_sketch_geometry(
        &mut ir.model.sketch_entities,
        &ir.model.sketches,
        &ir.model.surfaces,
        &ir.model.features,
        &ir.model.parameters,
        &sketch_lanes,
    );
    crate::resolved_features::relation_geometry::project_relation_point_geometry(
        &mut ir.model.sketch_entities,
        &ir.model.sketches,
        &ir.model.features,
        &sketch_lanes,
    );
    crate::resolved_features::dimensions::project_relation_point_dimensioned_circles(
        &mut ir.model.sketch_entities,
        &ir.model.features,
        &ir.model.parameters,
        &sketch_lanes,
    );
    crate::resolved_features::relation_geometry::project_relation_solved_line_geometry(
        &mut ir.model.sketch_entities,
        &ir.model.sketches,
        &ir.model.features,
        &ir.model.parameters,
        &sketch_lanes,
    );
    crate::resolved_features::relation_geometry::project_relation_solved_point_geometry(
        &mut ir.model.sketch_entities,
        &ir.model.sketches,
        &ir.model.features,
        &ir.model.parameters,
        &sketch_lanes,
    );
    crate::resolved_features::relation_geometry::project_relation_bindings(
        &mut ir.model.sketch_constraints,
        &ir.model.sketches,
        &ir.model.features,
        &ir.model.sketch_entities,
        &ir.model.parameters,
        &sketch_lanes,
    );
    crate::resolved_features::holes::project_profiled_hole_constructions(
        &mut ir.model.features,
        &ir.model.sketch_entities,
        &histories,
        &lanes,
    );
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
    );
    crate::resolved_features::holes::project_hole_axes(
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
    );
    crate::resolved_features::holes::project_hole_topology_axes(
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
    );
    crate::resolved_features::holes::project_bore_backed_position_sketches(
        &mut ir.model.features,
        &mut ir.model.sketches,
        &mut ir.model.sketch_entities,
        &ir.model.surfaces,
        &histories,
        &lanes,
    );
    crate::resolved_features::relation_geometry::project_relation_bindings(
        &mut ir.model.sketch_constraints,
        &ir.model.sketches,
        &ir.model.features,
        &ir.model.sketch_entities,
        &ir.model.parameters,
        &sketch_lanes,
    );
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
    sync_active_configuration_resolutions(&mut ir);
    crate::history::order_features_for_regeneration(&mut ir.model.features);
    crate::history::project_configuration_sketch_states(
        &mut ir,
        &histories,
        &lanes,
        &mut annotations,
    );
    crate::history::inherit_configuration_reference_plane_states(&mut ir);
    crate::history::order_model_features_for_regeneration(&mut ir);
    stamp_feature_baseline(&mut ir);
    lanes.extend(supplemental_config_lanes);
    let native = crate::native::SldprtNative {
        version: crate::native::SLDPRT_NATIVE_VERSION,
        feature_histories: histories.clone(),
        feature_input_lanes: lanes,
        pmi_dimensions,
    };
    ctx.admit_entities(
        ir.model.entity_count() as u64,
        admitted_entities,
        "admit SLDPRT entities",
    )?;
    native.store(ir.native.namespace_mut("sldprt"))?;
    stamp_sketch_baseline(&mut ir, &native);
    bind_active_configuration_partition(&mut ir);
    mark_active_configuration(&mut ir);
    stamp_configuration_baseline(&mut ir);
    snapshot_active_configuration(&mut ir);
    preserve_source_image(scan, &mut annotations, &mut unknowns);
    // Sort arenas for the order-sensitive loss scans that follow; the local
    // digests are stamped once, in `decode_result`, after native unknown
    // records are attached.
    ir.finalize();
    Ok((ir, annotations, unknowns, pmi_losses))
}

fn project_design_history(
    ir: &mut CadIr,
    histories: &[crate::records::FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
    pmi_dimensions: &[crate::records::PmiDimension],
    scan: &ContainerScan,
) {
    let mut semantic_projection = histories.to_vec();
    crate::history::enrich_scene_classes(
        &mut semantic_projection,
        &crate::tessellation::scene_feature_classes(scan),
    );
    crate::history::enrich_history_semantic(
        &mut semantic_projection,
        lanes,
        pmi_dimensions,
        crate::history::HistoryEnrichment::Read,
    );
    ir.model.semantic_annotations = crate::history::project_semantic_notes(&semantic_projection);
    ir.model.features = crate::history::project_features(&semantic_projection);
    crate::resolved_features::bindings::bind_pattern_inputs(
        &mut ir.model.features,
        &semantic_projection,
        lanes,
    );
    crate::history::project_compact_and_generated(
        &mut ir.model.features,
        &semantic_projection,
        lanes,
    );
    ir.model.configurations = crate::history::project_configurations(&semantic_projection);
    let mut parameter_projection = histories.to_vec();
    crate::resolved_features::direct_edits::enrich_history_move_face_translations(
        &mut parameter_projection,
        lanes,
    );
    crate::history::enrich_history_parameters_values_only(&mut parameter_projection, lanes);
    crate::resolved_features::holes::
        enrich_history_cosmetic_thread_diameters_without_hole_constructions(
            &mut parameter_projection,
            lanes,
        );
    crate::pmi::enrich_history_parameters(&mut parameter_projection, pmi_dimensions);
    ir.model.parameters = crate::history::project_parameters(&parameter_projection);
    crate::history::project_configuration_design_states(ir, histories, lanes, pmi_dimensions);
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            "sldprt_neutral_feature_local_sha256".into(),
            crate::history::feature_hash(&ir.model.features),
        );
        source.attributes.insert(
            "sldprt_native_history_sha256".into(),
            crate::history::history_hash(histories),
        );
        source.attributes.insert(
            "sldprt_native_configuration_sha256".into(),
            crate::history::native_configuration_hash(histories),
        );
        source.attributes.insert(
            "sldprt_neutral_parameter_local_sha256".into(),
            crate::history::parameter_hash(&ir.model.parameters),
        );
        source.attributes.insert(
            "sldprt_native_parameter_sha256".into(),
            crate::history::native_parameter_hash(histories),
        );
    }
}

fn parameter_identity_lanes(
    lanes: &[crate::records::FeatureInputLane],
) -> Vec<&crate::records::FeatureInputLane> {
    let lanes = lanes
        .iter()
        .filter(|lane| !crate::resolved_features::assembly::is_supplemental_config_lane(lane))
        .collect::<Vec<_>>();
    let has_global = lanes.iter().any(|lane| lane.configuration.is_none());
    let scoped_configurations = lanes
        .iter()
        .filter_map(|lane| lane.configuration.as_deref())
        .collect::<BTreeSet<_>>();
    let lane_count = lanes.len();
    lanes
        .into_iter()
        .filter(|lane| {
            if has_global {
                lane.configuration.is_none()
            } else {
                scoped_configurations.len() == 1 && lane_count == 1
            }
        })
        .collect()
}

fn stamp_parameter_baseline(ir: &mut CadIr) {
    let hash = crate::history::parameter_hash(&ir.model.parameters);
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert("sldprt_neutral_parameter_local_sha256".into(), hash);
    }
}

fn complete_resolved_configuration_parameter_snapshots(ir: &mut CadIr) {
    let independent_values = ir
        .model
        .parameters
        .iter()
        .filter(|parameter| parameter.dependencies.is_empty())
        .filter_map(|parameter| {
            parameter
                .value
                .clone()
                .map(|value| (parameter.id.clone(), value))
        })
        .collect::<Vec<_>>();
    for configuration in &mut ir.model.configurations {
        if configuration.parameter_values.is_empty() && configuration.feature_states.is_empty() {
            continue;
        }
        for (parameter, value) in &independent_values {
            configuration
                .parameter_values
                .entry(parameter.clone())
                .or_insert_with(|| value.clone());
        }
    }
}

fn mark_active_configuration(ir: &mut CadIr) {
    let active_name = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sw_configuration_name"))
        .cloned();
    let active_index = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("active_parasolid_block"))
        .and_then(|section| crate::container::configuration_index(section));
    let by_name = active_name.as_ref().and_then(|name| {
        let matches = ir
            .model
            .configurations
            .iter()
            .enumerate()
            .filter(|(_, configuration)| configuration.name.resolved() == Some(name.as_str()))
            .map(|(position, _)| position)
            .collect::<Vec<_>>();
        (matches.len() == 1).then(|| matches[0])
    });
    let by_index = active_index.and_then(|index| {
        let index = u32::try_from(index).ok()?;
        let matches = ir
            .model
            .configurations
            .iter()
            .enumerate()
            .filter(|(_, configuration)| configuration.source_index == Some(index))
            .map(|(position, _)| position)
            .collect::<Vec<_>>();
        (matches.len() == 1).then(|| matches[0])
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
        configuration.active = (selected == Some(position)).into();
    }
}

fn snapshot_active_configuration(ir: &mut CadIr) {
    let mut active = ir
        .model
        .configurations
        .iter()
        .enumerate()
        .filter(|(_, configuration)| configuration.active.is_active())
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
                    suppressed: feature.suppressed.unwrap_or(false),
                    dependencies: feature.dependencies.clone(),
                    outputs: feature.outputs.clone(),
                    definition: feature.definition.clone(),
                },
            )
        })
        .collect();
    let configuration = &mut ir.model.configurations[configuration_index];
    configuration.parameter_values = parameter_values;
    configuration.feature_states = feature_states;
    // Read-side fabricated snapshot of model-level state; tag the configuration
    // so the write path can distinguish it from feature-input lane state.
    let id = configuration.id.0.clone();
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert("sldprt_configuration_snapshot_synthesized".into(), id);
    }
}

fn sync_active_configuration_resolutions(ir: &mut CadIr) {
    let mut active = ir
        .model
        .configurations
        .iter()
        .enumerate()
        .filter(|(_, configuration)| configuration.active.is_active())
        .map(|(index, _)| index);
    let Some(configuration_index) = active.next() else {
        return;
    };
    if active.next().is_some() {
        return;
    }
    let resolved = ir
        .model
        .features
        .iter()
        .filter(|feature| feature.suppressed != Some(true))
        .filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::Hole {
                placements,
                kind,
                diameter,
                extent,
                bottom,
                taper_angle,
                ..
            } = &feature.definition
            else {
                return None;
            };
            Some((
                feature.id.clone(),
                placements.clone(),
                *kind,
                *diameter,
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
        resolved_kind,
        resolved_diameter,
        resolved_extent,
        resolved_bottom,
        resolved_taper_angle,
    ) in resolved
    {
        let Some(state) = configuration.feature_states.get_mut(&feature) else {
            continue;
        };
        if state.suppressed {
            continue;
        }
        let cadmpeg_ir::features::FeatureDefinition::Hole {
            placements,
            kind,
            diameter,
            extent,
            bottom,
            taper_angle,
            ..
        } = &mut state.definition
        else {
            continue;
        };
        if placements.is_empty() && !resolved_placements.is_empty() {
            *placements = resolved_placements;
        }
        let incomplete = diameter.is_none()
            || extent.as_ref().is_none_or(|extent| {
                matches!(extent, cadmpeg_ir::features::Termination::Unresolved)
            })
            || matches!(kind, cadmpeg_ir::features::HoleKind::Unresolved { .. });
        let resolved_complete = resolved_diameter.is_some()
            && resolved_extent.as_ref().is_some_and(|extent| {
                !matches!(extent, cadmpeg_ir::features::Termination::Unresolved)
            })
            && !matches!(
                resolved_kind,
                cadmpeg_ir::features::HoleKind::Unresolved { .. }
            );
        if incomplete && resolved_complete {
            *kind = resolved_kind;
            *diameter = resolved_diameter;
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
            let cadmpeg_ir::features::FeatureDefinition::CosmeticThread {
                face,
                diameter,
                extent,
            } = &feature.definition
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
        let cadmpeg_ir::features::FeatureDefinition::CosmeticThread {
            face,
            diameter,
            extent,
        } = &mut state.definition
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
            let cadmpeg_ir::features::FeatureDefinition::DatumOffsetPlane {
                reference:
                    Some(cadmpeg_ir::features::DatumPlaneReference::Face {
                        face: face @ cadmpeg_ir::features::FaceSelection::Faces(selected),
                        origin,
                        normal,
                        u_axis,
                    }),
                distance,
            } = &feature.definition
            else {
                return None;
            };
            (!selected.is_empty()).then_some((
                feature.id.clone(),
                face.clone(),
                *origin,
                *normal,
                *u_axis,
                *distance,
            ))
        })
        .collect::<Vec<_>>();
    let configuration = &mut ir.model.configurations[configuration_index];
    for (
        feature,
        resolved_face,
        resolved_origin,
        resolved_normal,
        resolved_u_axis,
        resolved_distance,
    ) in resolved
    {
        let Some(state) = configuration.feature_states.get_mut(&feature) else {
            continue;
        };
        let cadmpeg_ir::features::FeatureDefinition::DatumOffsetPlane {
            reference:
                Some(cadmpeg_ir::features::DatumPlaneReference::Face {
                    face,
                    origin,
                    normal,
                    u_axis,
                }),
            distance,
        } = &mut state.definition
        else {
            continue;
        };
        if *origin == resolved_origin
            && *normal == resolved_normal
            && *u_axis == resolved_u_axis
            && *distance == resolved_distance
            && matches!(face, cadmpeg_ir::features::FaceSelection::Unresolved)
        {
            *face = resolved_face;
        }
    }
    let resolved = ir
        .model
        .features
        .iter()
        .filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::Pattern {
                seeds,
                pattern: pattern @ cadmpeg_ir::features::PatternKind::Mirror { .. },
            } = &feature.definition
            else {
                return None;
            };
            Some((feature.id.clone(), seeds.clone(), pattern.clone()))
        })
        .collect::<Vec<_>>();
    let configuration = &mut ir.model.configurations[configuration_index];
    for (feature, resolved_seeds, resolved_pattern) in resolved {
        let Some(state) = configuration.feature_states.get_mut(&feature) else {
            continue;
        };
        let cadmpeg_ir::features::FeatureDefinition::Pattern { seeds, pattern } =
            &mut state.definition
        else {
            continue;
        };
        if *seeds == resolved_seeds
            && matches!(
                pattern,
                cadmpeg_ir::features::PatternKind::Unresolved { .. }
            )
        {
            *pattern = resolved_pattern;
        }
    }
}

fn stamp_feature_baseline(ir: &mut CadIr) {
    let hash = crate::history::feature_hash(&ir.model.features);
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert("sldprt_neutral_feature_local_sha256".into(), hash);
    }
}

fn assign_configuration_bodies(
    ir: &mut CadIr,
    configuration_bodies: &[(usize, Vec<cadmpeg_ir::ids::BodyId>)],
) {
    let mut partition_map = BTreeMap::<u32, Vec<cadmpeg_ir::ids::BodyId>>::new();
    for (index, bodies) in configuration_bodies {
        let Ok(index) = u32::try_from(*index) else {
            continue;
        };
        let merged = partition_map.entry(index).or_default();
        for body in bodies {
            if !merged.contains(body) {
                merged.push(body.clone());
            }
        }
    }

    let source_counts = ir
        .model
        .configurations
        .iter()
        .filter_map(|configuration| configuration.source_index)
        .fold(BTreeMap::<u32, usize>::new(), |mut counts, source_index| {
            *counts.entry(source_index).or_default() += 1;
            counts
        });
    for configuration in &mut ir.model.configurations {
        let Some(source_index) = configuration.source_index else {
            continue;
        };
        if source_counts.get(&source_index) == Some(&1) {
            configuration.bodies = cadmpeg_ir::ConfigurationBodies::Resolved(
                partition_map.remove(&source_index).unwrap_or_default(),
            );
        }
    }
    if let Some((active_index, position)) = bind_active_configuration_partition(ir) {
        if let Some(bodies) = partition_map.remove(&active_index) {
            ir.model.configurations[position].bodies =
                cadmpeg_ir::ConfigurationBodies::Resolved(bodies);
        }
    }
    for (source_index, bodies) in partition_map {
        let ordinal = ir
            .model
            .configurations
            .iter()
            .map(|configuration| configuration.ordinal)
            .max()
            .map_or(0, |ordinal| ordinal.saturating_add(1));
        ir.model
            .configurations
            .push(cadmpeg_ir::features::DesignConfiguration {
                id: cadmpeg_ir::features::ConfigurationId(format!(
                    "sldprt:model:configuration#partition:{source_index}"
                )),
                ordinal,
                active: false.into(),
                source_index: Some(source_index),
                name: format!("Config-{source_index}").into(),
                material: None,
                properties: std::collections::BTreeMap::new(),
                bodies: cadmpeg_ir::ConfigurationBodies::Resolved(bodies),
                parameter_values: std::collections::BTreeMap::new(),
                suppressed_features: Vec::new(),
                parameter_overrides: BTreeMap::new(),
                feature_states: std::collections::BTreeMap::new(),
                native_ref: None,
            });
    }
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
    let matches = ir
        .model
        .configurations
        .iter()
        .enumerate()
        .filter(|(_, configuration)| {
            configuration.source_index.is_none()
                && configuration.name.resolved() == Some(active_name.as_str())
        })
        .map(|(position, _)| position)
        .collect::<Vec<_>>();
    let source_identity_available = !ir
        .model
        .configurations
        .iter()
        .any(|configuration| configuration.source_index == Some(active_index));
    if matches.len() != 1 || !source_identity_available {
        return None;
    }

    // The container's active block and the native configuration name
    // establish the partition identity even when that block yielded no
    // decoded body list. Body membership remains unresolved until a decoded
    // partition supplies its body identities.
    let position = matches[0];
    ir.model.configurations[position].source_index = Some(active_index);
    Some((active_index, position))
}

fn stamp_configuration_baseline(ir: &mut CadIr) {
    let hash = crate::history::configuration_hash(&ir.model.configurations);
    let parameter_value_hash =
        crate::history::configuration_parameter_value_hash(&ir.model.configurations);
    let feature_state_hash =
        crate::history::configuration_feature_state_hash(&ir.model.configurations);
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert("sldprt_neutral_configuration_local_sha256".into(), hash);
        source.attributes.insert(
            "sldprt_configuration_parameter_values_local_sha256".into(),
            parameter_value_hash,
        );
        source.attributes.insert(
            "sldprt_configuration_feature_states_local_sha256".into(),
            feature_state_hash,
        );
    }
}

/// Record the sketch baselines the write path compares against.
///
/// Neutral sketch and constraint digests use `_local_sha256` (projected
/// geometry through libm). `sldprt_native_sketch_sha256` has no suffix: it
/// digests lane fields that are strings, integers, or verbatim `f64` bit
/// patterns from the payload.
fn stamp_sketch_baseline(ir: &mut CadIr, native: &crate::native::SldprtNative) {
    let neutral_hash = crate::resolved_features::hashes::sketch_hash(ir);
    let constraint_hash = crate::resolved_features::hashes::constraint_hash(ir);
    let native_hash = crate::resolved_features::hashes::lane_hash(native);
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert("sldprt_neutral_sketch_local_sha256".into(), neutral_hash);
        source
            .attributes
            .insert("sldprt_native_sketch_sha256".into(), native_hash);
        source.attributes.insert(
            "sldprt_neutral_sketch_constraint_local_sha256".into(),
            constraint_hash,
        );
    }
}

/// Record the document and B-rep partition baselines the write path compares
/// against.
///
/// Both are machine-local content digests and carry the `_local_sha256` suffix
/// that says so; see [`document_local_sha256`] and [`brep_local_sha256`].
fn stamp_local_digests(ir: &mut CadIr) {
    ir.finalize();
    let brep_hash = brep_local_sha256_in_place(ir);
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert("brep_local_sha256".into(), brep_hash);
    }
    let has_swobjects_semantics = ir
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.id.0.starts_with("sldprt:metadata:"))
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
            let identity_hash = crate::writer::swobjects_metadata_identity_local_sha256(ir);
            if let Some(source) = &mut ir.source {
                source.attributes.insert(
                    crate::writer::SWOBJECTS_LOCAL_DIGEST_ATTRIBUTE.into(),
                    swobjects_hash,
                );
                source.attributes.insert(
                    crate::writer::SWOBJECTS_MATERIAL_LOCAL_DIGEST_ATTRIBUTE.into(),
                    material_hash,
                );
                source.attributes.insert(
                    crate::writer::SWOBJECTS_METADATA_IDENTITY_LOCAL_DIGEST_ATTRIBUTE.into(),
                    identity_hash,
                );
            }
        }
    }
    if !ir.model.pmi.is_empty() {
        if let Ok(hash) = crate::writer::pmi_local_sha256(ir) {
            if let Some(source) = &mut ir.source {
                source
                    .attributes
                    .insert(crate::writer::PMI_LOCAL_DIGEST_ATTRIBUTE.into(), hash);
            }
        }
    }
    let hash = document_local_sha256(ir);
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            cadmpeg_ir::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE.into(),
            hash,
        );
    }
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
pub(crate) fn brep_local_sha256(ir: &CadIr) -> String {
    // Admit only B-rep arenas so a new design, presentation, or product arena
    // cannot silently change retained-partition eligibility.
    let partition = cadmpeg_ir::document::Model {
        bodies: ir.model.bodies.clone(),
        regions: ir.model.regions.clone(),
        shells: ir.model.shells.clone(),
        faces: ir.model.faces.clone(),
        loops: ir.model.loops.clone(),
        coedges: ir.model.coedges.clone(),
        edges: ir.model.edges.clone(),
        vertices: ir.model.vertices.clone(),
        points: ir.model.points.clone(),
        surfaces: ir.model.surfaces.clone(),
        curves: ir.model.curves.clone(),
        pcurves: ir.model.pcurves.clone(),
        procedural_surfaces: ir.model.procedural_surfaces.clone(),
        procedural_curves: ir.model.procedural_curves.clone(),
        appearances: ir.model.appearances.clone(),
        appearance_bindings: ir.model.appearance_bindings.clone(),
        ..Default::default()
    };
    brep_partition_sha256(ir.units.clone(), ir.tolerances, partition).0
}

/// [`brep_local_sha256`] without the deep clone, for the decode stamp path.
///
/// Moves the structurally untouched B-rep arenas out of `ir`, hashes the same
/// normalized partition [`brep_local_sha256`] builds, and moves them back in
/// their original order. The two arenas the normalization filters —
/// `appearances` and `appearance_bindings` — and the body display fields it
/// strips are copied, so `ir` is bit-identical afterwards and both entry
/// points produce the same digest for the same document.
fn brep_local_sha256_in_place(ir: &mut CadIr) -> String {
    use std::mem::take;

    let saved_body_display = ir
        .model
        .bodies
        .iter()
        .map(|body| (body.name.clone(), body.color))
        .collect::<Vec<_>>();
    let partition = cadmpeg_ir::document::Model {
        bodies: take(&mut ir.model.bodies),
        regions: take(&mut ir.model.regions),
        shells: take(&mut ir.model.shells),
        faces: take(&mut ir.model.faces),
        loops: take(&mut ir.model.loops),
        coedges: take(&mut ir.model.coedges),
        edges: take(&mut ir.model.edges),
        vertices: take(&mut ir.model.vertices),
        points: take(&mut ir.model.points),
        surfaces: take(&mut ir.model.surfaces),
        curves: take(&mut ir.model.curves),
        pcurves: take(&mut ir.model.pcurves),
        procedural_surfaces: take(&mut ir.model.procedural_surfaces),
        procedural_curves: take(&mut ir.model.procedural_curves),
        appearances: ir.model.appearances.clone(),
        appearance_bindings: ir.model.appearance_bindings.clone(),
        ..Default::default()
    };
    let (hash, mut partition) = brep_partition_sha256(ir.units.clone(), ir.tolerances, partition);
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
    hash
}

/// Normalize and hash one B-rep partition; both digest entry points share it.
///
/// Returns the normalized model with the hash so an in-place caller can move
/// its arenas back; only `bodies` (display fields stripped), `appearances`,
/// and `appearance_bindings` (both filtered to face bindings) are mutated.
fn brep_partition_sha256(
    units: cadmpeg_ir::units::Units,
    tolerances: cadmpeg_ir::units::Tolerances,
    model: cadmpeg_ir::document::Model,
) -> (String, cadmpeg_ir::document::Model) {
    use cadmpeg_ir::appearance::AppearanceTarget;

    let mut normalized = CadIr::empty(units);
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
    (
        cadmpeg_ir::hash::canonical_json_sha256(&normalized),
        normalized.model,
    )
}

/// Machine-local `document_local_sha256` for the SLDPRT write-path edit oracle.
///
/// See [`cadmpeg_ir::hash::document_local_sha256`].
pub(crate) fn document_local_sha256(ir: &CadIr) -> String {
    cadmpeg_ir::hash::document_local_sha256(ir, "sldprt", "sldprt:file:source-image#0")
}

fn preserve_source_image(
    scan: &ContainerScan,
    annotations: &mut Annotations,
    unknowns: &mut Vec<UnknownRecord>,
) {
    crate::annotations::note(
        annotations,
        "sldprt:file:source-image#0",
        "source",
        0,
        "source_image",
        Exactness::ByteExact,
    );
    unknowns.push(UnknownRecord {
        id: UnknownId("sldprt:file:source-image#0".into()),
        offset: 0,
        byte_len: scan.source_image.len() as u64,
        sha256: sha256_hex(scan.source_image),
        data: Some(scan.source_image.to_vec()),
        links: Vec::new(),
    });
}

fn build_container_report(scan: &ContainerScan, container_only: bool) -> DecodeReport {
    let summary = container::summarize(scan);
    let parasolid_sources = scan
        .blocks
        .iter()
        .filter(|b| b.family == "parasolid")
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
        losses.push(
            SldprtLossCode::ContainerNoParasolidStream.note(
                "no Parasolid partition/deltas stream was located in the container".to_string(),
            ),
        );
    }
    append_swift_pmi_losses(scan, &mut losses);

    DecodeReport {
        format: "sldprt".to_string(),
        container_only,
        geometry_transferred: false,
        coverage: std::collections::BTreeMap::new(),
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
        losses,
        notes: summary.notes,
    }
}

fn append_swift_pmi_losses(scan: &ContainerScan<'_>, losses: &mut Vec<cadmpeg_ir::LossNote>) {
    let unsupported = crate::swift::unsupported_annotation_classes(scan);
    if unsupported.is_empty() {
        return;
    }
    let count = unsupported.values().sum::<usize>();
    let classes = unsupported
        .iter()
        .map(|(class, count)| format!("{class} ({count})"))
        .collect::<Vec<_>>()
        .join(", ");
    losses.push(SldprtLossCode::PmiSwiftAnnotationUnsupported.note(format!(
        "{count} SWIFT semantic annotation(s) have no neutral PMI definition: {classes}."
    )));
}

#[cfg(test)]
mod tests;
