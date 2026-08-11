// SPDX-License-Identifier: Apache-2.0
//! High-level `.sldprt` decoding.
//!
//! [`decode`] scans the outer [`crate::container`], groups related Parasolid
//! `partition` and `deltas` streams, and selects the group that yields the
//! richest B-rep. It then adds appearances, display meshes, document attributes,
//! feature history, feature-input lanes, provenance, and retained source data.
//!
//! The returned [`DecodeResult`] contains both the IR and its diagnostics.
//! Untyped surface and curve carriers become opaque geometry linked to the
//! retained partition. If no body stream yields geometry, decoding returns a
//! metadata-only IR and blocking loss notes. [`DecodeOptions::container_only`]
//! requests the metadata-only path.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::be::u32_at as be_u32;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::le::{i32_at as le_i32, u16_at as le_u16, u32_at as le_u32};
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
    selected: usize,
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

    if ctx.container_only() {
        let (ir, annotations, unknowns) = build_metadata_ir(&scan)?;
        let report = build_container_report(&scan, true);
        return decode_result(ctx, ir, report, annotations, unknowns);
    }

    let streams = active_body_streams(&scan);
    if !streams.is_empty() {
        if let Some((decoded, mut report)) = try_decode_brep(&scan, &streams) {
            let (ir, annotations, unknowns) = build_geometry_ir(
                &scan,
                &streams[decoded.selected].header,
                decoded.brep,
                &decoded.configuration_bodies,
            )?;
            append_tessellation_losses(&ir, &mut report);
            append_design_losses(&ir, &mut report);
            return decode_result(ctx, ir, report, annotations, unknowns);
        }
    }

    let (ir, annotations, unknowns) = build_metadata_ir(&scan)?;
    let mut report = build_container_report(&scan, false);
    append_design_losses(&ir, &mut report);
    decode_result(ctx, ir, report, annotations, unknowns)
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
    ctx: &DecodeContext<'_>,
    mut ir: CadIr,
    report: DecodeReport,
    annotations: Annotations,
    mut unknowns: Vec<UnknownRecord>,
) -> Result<DecodeResult, CodecError> {
    ctx.charge_entities(ir.model.entity_count() as u64, "admit SLDPRT entities")?;
    let mut source_fidelity = cadmpeg_ir::SourceFidelity {
        annotations,
        ..cadmpeg_ir::SourceFidelity::default()
    };
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
        .filter(|configuration| configuration.active)
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
        .filter(|configuration| configuration.name.is_empty())
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
        .map(|configuration| configuration.name.as_str())
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
            FeatureDefinition::ReferenceImage { asset, .. } => {
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
                pull_plane,
                pull_direction,
                angle,
                outward,
            } => {
                parting_tool.is_some()
                    || pull_plane.is_some()
                    || incomplete_face_selection(faces)
                    || parting_tool.as_ref().map_or_else(
                        || incomplete_face_selection(neutral_plane),
                        incomplete_face_selection,
                    )
                    || pull_direction.is_none()
                    || angle.is_none()
                    || outward.is_none()
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
            // These variants explicitly retain unresolved construction semantics. Keep
            // the match exhaustive so a new common-IR family cannot silently pass L6.
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
                    && binding_counts.get(&(id, class.name.as_str())).copied() == Some(1)
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

/// Decode the active Parasolid stream's B-rep. Returns `None` when the stream
/// frames but yields no geometry, so the caller falls back to metadata.
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
        let score = (
            decoded.faces.len(),
            decoded.bodies.len(),
            decoded.points.len(),
        );
        decoded_sites.push((site.clone(), first, score, decoded));
    }
    let active_site = container::select_active_parasolid(scan)
        .map(|(block, _)| format!("block@{}", block.offset));
    let resolved_active_site = active_site.as_ref().and_then(|active| {
        decoded_sites
            .iter()
            .position(|(site, _, _, _)| site == active)
    });
    let selected_site = resolved_active_site.or_else(|| {
        decoded_sites
            .iter()
            .enumerate()
            .max_by_key(|(index, (_, _, score, _))| (*score, Reverse(*index)))
            .map(|(index, _)| index)
    })?;
    let selected_is_empty_model = decoded_sites[selected_site].3.stats.source_entity_records == 0
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
    if !selected_is_empty_model
        && decoded_sites[selected_site].3.faces.is_empty()
        && decoded_sites[selected_site].3.surfaces.is_empty()
        && decoded_sites[selected_site].3.points.is_empty()
    {
        return None;
    }
    let (selected_site_key, selected, _, mut decoded) = decoded_sites.swap_remove(selected_site);
    if resolved_active_site.is_none() {
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
    for (site, first, _, mut alternate) in decoded_sites {
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
        merge_brep(&mut decoded, alternate);
    }
    let report = build_geometry_report(scan, &decoded);
    Some((
        DecodedBrep {
            selected,
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
    target.stats.source_entity_records += source.stats.source_entity_records;
    target.stats.ambiguous_body_assignments += source.stats.ambiguous_body_assignments;
    target.stats.unresolved_face_colors += source.stats.unresolved_face_colors;
    target.stats.ambiguous_face_owners += source.stats.ambiguous_face_owners;
    target.stats.unclaimed_faces += source.stats.unclaimed_faces;
    target.stats.synthetic_body_grouping |= source.stats.synthetic_body_grouping;
}

fn build_geometry_ir(
    scan: &ContainerScan,
    header: &StreamHeader,
    mut brep: Brep,
    configuration_bodies: &[(usize, Vec<cadmpeg_ir::ids::BodyId>)],
) -> Result<(CadIr, Annotations, Vec<UnknownRecord>), CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let materials = crate::appearance::materials(scan);
    let unique_material = materials.len() == 1;
    if let [material] = materials.as_slice() {
        for body in &mut brep.bodies {
            body.color = Some(material.color);
            if body.name.is_none() {
                body.name = Some(material.name.clone());
            }
        }
    }
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
    let pmi_dimensions = crate::pmi::dimensions(scan, &mut annotations);
    project_design_history(&mut ir, &histories, &lanes, &pmi_dimensions, scan);
    let (spatial_sketches, spatial_sketch_entities) =
        crate::resolved_features::markers::spatial_sketches(
            &mut ir.model.features,
            &histories,
            &lanes,
        );
    ir.model.spatial_sketches = spatial_sketches;
    ir.model.spatial_sketch_entities = spatial_sketch_entities;
    crate::resolved_features::operations::bind_extrusion_operations(
        &mut ir.model.features,
        &histories,
        &lanes,
        form_padding,
    );
    crate::resolved_features::operations::bind_revolution_operations(
        &mut ir.model.features,
        &histories,
        &lanes,
        form_padding,
    );
    crate::resolved_features::operations::bind_sweep_operations(
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
        &lanes,
    );
    crate::resolved_features::dimensions::project_marker_dimensioned_circles(
        &mut sketch_entities,
        &mut sketches,
        &ir.model.features,
        &ir.model.parameters,
        &lanes,
    );
    crate::resolved_features::relation_geometry::project_relation_point_geometry(
        &mut sketch_entities,
        &sketches,
        &ir.model.features,
        &lanes,
    );
    crate::resolved_features::dimensions::project_relation_point_dimensioned_circles(
        &mut sketch_entities,
        &ir.model.features,
        &ir.model.parameters,
        &lanes,
    );
    crate::resolved_features::relation_geometry::project_relation_solved_line_geometry(
        &mut sketch_entities,
        &sketches,
        &ir.model.features,
        &ir.model.parameters,
        &lanes,
    );
    crate::resolved_features::relation_geometry::project_relation_solved_point_geometry(
        &mut sketch_entities,
        &sketches,
        &ir.model.features,
        &ir.model.parameters,
        &lanes,
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
    let face_identities = brep
        .face_atoms
        .iter()
        .filter_map(|atom| {
            atom.target
                .clone()
                .map(|target| (target, atom.feature_source_id, atom.local_face_id))
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
    crate::history::bind_topology_selections(
        &mut ir.model.features,
        &histories,
        &ir.model.bodies,
        &ir.model.faces,
        &ir.model.surfaces,
        &ir.model.edges,
        &ir.model.curves,
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
    sync_active_configuration_resolutions(&mut ir);
    crate::history::order_model_features_for_regeneration(&mut ir);
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
    native.store(ir.native.namespace_mut("sldprt"))?;
    // The baseline has to describe native-backed configuration state only, so
    // it is stamped before the read-side snapshot is fabricated. Stamping after
    // would bake fabricated state into a hash the write path compares against a
    // projection that can only ever re-derive the native-backed part.
    stamp_configuration_baseline(&mut ir);
    snapshot_active_configuration(&mut ir);
    let mut unknowns = brep.unknowns;
    for face_color in brep.face_colors {
        let id = AppearanceId(format!(
            "sldprt:appearance:entity53#{}",
            face_color.color_attr
        ));
        crate::annotations::note(
            &mut annotations,
            id.0.clone(),
            header.description.clone(),
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
    for (index, material) in materials.into_iter().enumerate() {
        let id = AppearanceId(format!("sldprt:appearance:material#{index}"));
        let material_stream = material.source_name;
        crate::annotations::note(
            &mut annotations,
            id.0.clone(),
            material_stream.clone(),
            material.record_offset as u64,
            "moVisualProperties_c",
            Exactness::ByteExact,
        );
        ir.model.appearances.push(Appearance {
            id: id.clone(),
            name: Some(material.name),
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: Some("moVisualProperties_c".to_string()),
            category: None,
            base_color: Some(material.color),
            textures: Vec::new(),
            properties: BTreeMap::new(),
        });
        if unique_material {
            for (body_index, body) in ir.model.bodies.iter().enumerate() {
                ir.model.appearance_bindings.push(AppearanceBinding {
                    id: format!("sldprt:appearance:binding#body:{body_index}:{index}"),
                    target: AppearanceTarget::Body(body.id.clone()),
                    appearance: id.clone(),
                    source_entity_id: None,
                    object_type: Some("Body".to_string()),
                    channels: BTreeMap::new(),
                });
            }
        }
    }
    for display in scan
        .sections()
        .filter(|section| crate::tessellation::section_summary(*section).is_some())
    {
        for (index, mesh) in crate::tessellation::section_meshes(display)
            .into_iter()
            .enumerate()
        {
            let id = format!("sldprt:displaylist:record#{}:{index}", display.ordinal());
            let display_stream = display.display_name();
            crate::annotations::note(
                &mut annotations,
                id.clone(),
                display_stream,
                0,
                "displaylist_tessellation",
                Exactness::ByteExact,
            );
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
            links: Vec::new(),
        });
    }
    for id in crate::tessellation::assign_unique_analytic_owners(&mut ir.model) {
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
    stamp_local_digests(&mut ir);
    Ok((ir, annotations, unknowns))
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

fn source_meta(scan: &ContainerScan, header: &StreamHeader) -> SourceMeta {
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
    attributes.insert("parasolid_schema".to_string(), header.schema.clone());
    attributes.insert(
        "parasolid_description".to_string(),
        header.description.clone(),
    );
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
                let Some(width) = be_u32(payload, 16) else {
                    continue;
                };
                let Some(height) = be_u32(payload, 20) else {
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
                let (Some(width), Some(height), Some(image_size)) =
                    (le_i32(payload, 8), le_i32(payload, 12), le_u32(payload, 24))
                else {
                    continue;
                };
                let (Some(planes), Some(bits_per_pixel), Some(compression)) = (
                    le_u16(payload, 16),
                    le_u16(payload, 18),
                    le_u32(payload, 20),
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
    scan: &ContainerScan,
) -> Result<(CadIr, Annotations, Vec<UnknownRecord>), CodecError> {
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
    let pmi_dimensions = crate::pmi::dimensions(scan, &mut annotations);
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
    let (spatial_sketches, spatial_sketch_entities) =
        crate::resolved_features::markers::spatial_sketches(
            &mut ir.model.features,
            &histories,
            &lanes,
        );
    ir.model.spatial_sketches = spatial_sketches;
    ir.model.spatial_sketch_entities = spatial_sketch_entities;
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
        &lanes,
    );
    crate::resolved_features::relation_geometry::project_relation_point_geometry(
        &mut ir.model.sketch_entities,
        &ir.model.sketches,
        &ir.model.features,
        &lanes,
    );
    crate::resolved_features::dimensions::project_relation_point_dimensioned_circles(
        &mut ir.model.sketch_entities,
        &ir.model.features,
        &ir.model.parameters,
        &lanes,
    );
    crate::resolved_features::relation_geometry::project_relation_solved_line_geometry(
        &mut ir.model.sketch_entities,
        &ir.model.sketches,
        &ir.model.features,
        &ir.model.parameters,
        &lanes,
    );
    crate::resolved_features::relation_geometry::project_relation_solved_point_geometry(
        &mut ir.model.sketch_entities,
        &ir.model.sketches,
        &ir.model.features,
        &ir.model.parameters,
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
    crate::history::order_model_features_for_regeneration(&mut ir);
    stamp_feature_baseline(&mut ir);
    lanes.extend(supplemental_config_lanes);
    let native = crate::native::SldprtNative {
        version: crate::native::SLDPRT_NATIVE_VERSION,
        feature_histories: histories.clone(),
        feature_input_lanes: lanes,
        pmi_dimensions,
    };
    native.store(ir.native.namespace_mut("sldprt"))?;
    stamp_sketch_baseline(&mut ir, &native);
    mark_active_configuration(&mut ir);
    stamp_configuration_baseline(&mut ir);
    snapshot_active_configuration(&mut ir);
    preserve_source_image(scan, &mut annotations, &mut unknowns);
    stamp_local_digests(&mut ir);
    Ok((ir, annotations, unknowns))
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
            .filter(|(_, configuration)| &configuration.name == name)
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
    // This design state is a read-side presentation of model-level state, not
    // configuration-local data the native records carry. Naming the
    // configuration it was fabricated on lets the write path tell it apart from
    // state that came out of a feature-input lane.
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
        .filter(|(_, configuration)| configuration.active)
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
    if let (Some(active_name), Some(active_index)) = (active_name, active_index) {
        let matches = ir
            .model
            .configurations
            .iter()
            .enumerate()
            .filter(|(_, configuration)| {
                configuration.source_index.is_none() && &configuration.name == active_name
            })
            .map(|(position, _)| position)
            .collect::<Vec<_>>();
        if matches.len() == 1 {
            if let Some(bodies) = partition_map.remove(&active_index) {
                let position = matches[0];
                let configuration = &mut ir.model.configurations[position];
                configuration.source_index = Some(active_index);
                configuration.bodies = cadmpeg_ir::ConfigurationBodies::Resolved(bodies);
            }
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
                active: false,
                source_index: Some(source_index),
                name: format!("Config-{source_index}"),
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
/// Two of the three are machine-local and say so with the `_local_sha256`
/// suffix: they cover projected neutral sketch geometry, which reaches its
/// values through `f64::cos` and friends.
///
/// `sldprt_native_sketch_sha256` carries no such suffix because it is portable
/// by construction, not merely portable today. It digests
/// `SldprtNative::feature_input_lanes`, whose every field is a `String`, an
/// integer, or retained source bytes, with exactly three exceptions:
/// `FeatureInputScalar::value`, `SketchInputEntity::state_value`, and
/// `SketchInputEntity::coordinates_m`. Each of the three is one
/// `f64::from_le_bytes` of the payload at the byte offset the record stores
/// beside it, with no arithmetic between the read and the field. Reading an
/// IEEE 754 bit pattern is exact on every platform, so no libm can move this
/// digest. Any future enrichment that computes a lane float rather than reading
/// one makes the digest machine-local and forces the rename.
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
    let brep_hash = brep_local_sha256(ir);
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
    use cadmpeg_ir::appearance::AppearanceTarget;

    // Normalize with a field-by-field clone so the dropped namespaces (source
    // image, native records, annotations) are never copied.
    let mut normalized = CadIr {
        ir_version: ir.ir_version.clone(),
        source: None,
        units: ir.units.clone(),
        tolerances: ir.tolerances,
        model: ir.model.clone(),
        native: cadmpeg_ir::Native::default(),
    };
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
    normalized.model.tessellations.clear();
    normalized.model.attributes.clear();
    normalized.model.features.clear();
    normalized.model.parameters.clear();
    normalized.model.sketches.clear();
    normalized.model.sketch_entities.clear();
    normalized.model.sketch_constraints.clear();
    cadmpeg_ir::hash::canonical_json_sha256(&normalized)
}

/// The machine-local content digest recorded as the SLDPRT
/// `document_local_sha256` attribute.
///
/// A bitwise digest over the decoded neutral document. Its one consumer is
/// [`crate::SldprtCodec`]'s write path, which replays the retained source bytes
/// when the recorded digest still equals a freshly computed one and writes the
/// document through the semantic writer otherwise. It is not portable across
/// platforms, because the decoded content includes values derived through libm
/// transcendentals, and it is intentionally not tolerance-aware, because tolerant
/// equality is not transitive and cannot back a hash. The `_local_sha256` suffix
/// states that; see [`cadmpeg_ir::hash::document_local_sha256`].
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

#[cfg(test)]
mod design_loss_tests {
    use super::*;
    use crate::container::{Block, CompoundStream, ContainerScan};
    use crate::native::SldprtNative;
    use crate::records::{
        Feature as NativeFeature, FeatureHistory, FeatureInputClass, FeatureInputClassRole,
        FeatureInputLane, FeatureInputName, FeatureInputRelationBinding,
        FeatureInputRelationFamily, FeatureInputRelationInstance, SketchInputEntity,
        SketchInputKind, SketchInputLink, SketchRelationKind,
    };
    use cadmpeg_ir::features::{
        Angle, BodyRetentionMode, BodySelection, BooleanOp, ConfigurationFeatureState,
        ConfigurationId, DesignConfiguration, DesignParameter, EdgeSelection, FaceSelection,
        Feature, FeatureDefinition, FeatureId, FeatureSourceContent, FeatureTreeNodeRole,
        HoleBottom, HoleKind, HolePlacement, Length, ParameterId, ParameterPmi, ParameterValue,
        PathRef, PatternKind, PatternSeed, PmiDimensionSubtype, RadiusSpec, RuledSurfaceMode,
        SurfaceContinuity, Termination,
    };
    use cadmpeg_ir::ids::{BodyId, EdgeId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::report::DecodeReport;
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinition, SketchConstraintId, SketchEntity, SketchEntityId,
        SketchGeometry, SketchId, SpatialSketchConstraint, SpatialSketchConstraintDefinition,
        SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometry, SpatialSketchId,
    };
    use cadmpeg_ir::units::Units;
    use cadmpeg_ir::CadIr;
    use std::collections::BTreeMap;

    #[test]
    fn site_keys_use_outer_container_identity() {
        let first = Block {
            offset: 100,
            type_id: 0,
            comp_sz: 0,
            uncomp_sz: 0,
            section: Some("Contents/Config-0-Partition".into()),
            family: "parasolid",
            payload: Vec::new(),
            ps_stream: None,
            ps_streams: Vec::new(),
            ps_stream_offsets: Vec::new(),
        };
        let second = Block {
            offset: 200,
            section: first.section.clone(),
            ..first.clone()
        };
        assert_ne!(
            super::BodyOrigin::Block(&first).site_key(),
            super::BodyOrigin::Block(&second).site_key()
        );

        let compound = CompoundStream {
            path: "Contents/Config-0-Partition".into(),
            directory_id: 300,
            start_sector: 0,
            payload: Vec::new(),
            decoded_payload: None,
            ps_streams: Vec::new(),
            ps_stream_offsets: Vec::new(),
        };
        assert_eq!(
            super::BodyOrigin::Compound(&compound).site_key(),
            "compound@300"
        );
    }

    #[test]
    fn sketch_constraint_completeness_distinguishes_neutral_and_native_semantics() {
        assert!(sketch_constraint_has_complete_neutral_semantics(
            &SketchConstraintDefinition::Disabled
        ));
        assert!(!sketch_constraint_has_complete_neutral_semantics(
            &SketchConstraintDefinition::Native {
                native_kind: "unresolved".into(),
                native_state: None,
                native_flags: None,
                native_properties: BTreeMap::new(),
                entities: Vec::new(),
                parameter: None,
                operands: Vec::new(),
            }
        ));
        assert!(spatial_sketch_constraint_has_complete_neutral_semantics(
            &SpatialSketchConstraintDefinition::Coincident {
                first: SpatialSketchEntityId("first".into()),
                second: SpatialSketchEntityId("second".into()),
            }
        ));
        assert!(!spatial_sketch_constraint_has_complete_neutral_semantics(
            &SpatialSketchConstraintDefinition::Native {
                native_kind: "unresolved".into(),
                native_state: None,
                parameter: None,
                operands: Vec::new(),
            }
        ));
    }

    #[test]
    fn native_spatial_sketch_constraints_are_reported_as_design_losses() {
        let mut ir = CadIr::empty(Units::default());
        ir.model
            .spatial_sketch_constraints
            .push(SpatialSketchConstraint {
                id: SketchConstraintId("native-spatial".into()),
                sketch: SpatialSketchId("spatial-sketch".into()),
                definition: SpatialSketchConstraintDefinition::Native {
                    native_kind: "unresolved".into(),
                    native_state: None,
                    parameter: None,
                    operands: Vec::new(),
                },
                native_ref: None,
            });
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "1 planar or spatial sketch constraint(s) retain native relation kinds and operands without complete neutral geometric semantics."
        }));
    }

    #[test]
    fn typed_native_operands_are_reported_as_design_losses() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.features.push(Feature {
            id: FeatureId("combine".into()),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Combine {
                target: BodySelection::Native("target".into()),
                tools: BodySelection::Native("tools".into()),
                op: BooleanOp::Unresolved,
                keep_tools: false,
            },
            native_ref: None,
        });
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: std::collections::BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "1 typed feature(s) retain native or unresolved required operation operands."
        }));
    }

    #[test]
    fn configuration_feature_states_drive_design_completeness_accounting() {
        let mut ir = CadIr::empty(Units::default());
        let feature_id = FeatureId("configured".into());
        ir.model.features.push(Feature {
            id: feature_id.clone(),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::from([("Scope".into(), "Body1".into())]),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: Vec::new(),
                active_child: None,
            },
            native_ref: None,
        });
        for (ordinal, definition) in [
            (
                0,
                FeatureDefinition::Native {
                    kind: "Unprojected".into(),
                    parameters: BTreeMap::new(),
                    properties: BTreeMap::new(),
                },
            ),
            (
                1,
                FeatureDefinition::Combine {
                    target: BodySelection::Native("target".into()),
                    tools: BodySelection::Native("tools".into()),
                    op: BooleanOp::Unresolved,
                    keep_tools: false,
                },
            ),
            (
                2,
                FeatureDefinition::DeleteBody {
                    bodies: BodySelection::Native("bodies".into()),
                    mode: BodyRetentionMode::Unresolved,
                },
            ),
        ] {
            ir.model.configurations.push(DesignConfiguration {
                id: ConfigurationId(format!("configuration-{ordinal}")),
                ordinal,
                active: ordinal == 0,
                source_index: Some(ordinal),
                name: format!("Configuration {ordinal}"),
                material: None,
                properties: BTreeMap::new(),
                bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
                parameter_values: BTreeMap::new(),
                suppressed_features: Vec::new(),
                parameter_overrides: BTreeMap::new(),
                feature_states: BTreeMap::from([(
                    feature_id.clone(),
                    ConfigurationFeatureState {
                        suppressed: false,
                        dependencies: (ordinal == 0)
                            .then(|| FeatureId("missing-dependency".into()))
                            .into_iter()
                            .collect(),
                        outputs: (ordinal == 0)
                            .then(|| BodyId("missing-output".into()))
                            .into_iter()
                            .collect(),
                        definition,
                    },
                )]),
                native_ref: None,
            });
        }
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: std::collections::BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        for expected in [
            "1 feature record(s) contain missing, repeated, or non-preceding parent/dependency edges; 0 feature record(s) share regeneration ordinals.",
            "2 feature(s) retain non-empty native output scopes that do not resolve to model bodies.",
            "1 feature record(s) contain missing or repeated output body references.",
            "1 feature(s) retain their native kind without a complete neutral operation definition.",
            "2 typed feature(s) retain native or unresolved required operation operands.",
            "1 body delete/keep feature(s) retain selected native body identities without a decoded retention mode.",
        ] {
            assert!(report.losses.iter().any(|loss| loss.message == expected));
        }
    }

    #[test]
    fn active_configuration_inherits_late_feature_resolutions() {
        let mut ir = CadIr::empty(Units::default());
        let feature_id = FeatureId("mirror".into());
        let seed = PatternSeed::Feature(FeatureId("seed".into()));
        ir.model.features.push(Feature {
            id: feature_id.clone(),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Pattern {
                seeds: vec![seed.clone()],
                pattern: PatternKind::Mirror {
                    plane_origin: Point3::new(1.0, 2.0, 3.0),
                    plane_normal: Vector3::new(0.0, 0.0, 1.0),
                },
            },
            native_ref: None,
        });
        let hole_id = FeatureId("hole".into());
        ir.model.features.push(Feature {
            id: hole_id.clone(),
            ordinal: 1,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Hole {
                profile: None,
                profile_filter: None,
                face: None,
                position: None,
                direction: None,
                placements: vec![HolePlacement::Axis {
                    origin: Point3::new(1.0, 2.0, 3.0),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                }],
                kind: HoleKind::Simple,
                exit_kind: None,
                diameter: Some(Length(4.0)),
                extent: Some(Termination::Blind {
                    length: Length(12.0),
                }),
                bottom: Some(HoleBottom::Flat),
                taper_angle: None,
                specification: None,
                allow_multi_profile_faces: None,
            },
            native_ref: None,
        });
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId("configuration".into()),
            ordinal: 0,
            active: true,
            source_index: Some(0),
            name: "Configuration".into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
            parameter_values: BTreeMap::new(),
            suppressed_features: Vec::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::from([
                (
                    feature_id.clone(),
                    ConfigurationFeatureState {
                        suppressed: false,
                        dependencies: Vec::new(),
                        outputs: Vec::new(),
                        definition: FeatureDefinition::Pattern {
                            seeds: vec![seed],
                            pattern: PatternKind::Unresolved { form: None },
                        },
                    },
                ),
                (
                    hole_id.clone(),
                    ConfigurationFeatureState {
                        suppressed: false,
                        dependencies: Vec::new(),
                        outputs: Vec::new(),
                        definition: FeatureDefinition::Hole {
                            profile: None,
                            profile_filter: None,
                            face: None,
                            position: None,
                            direction: None,
                            placements: Vec::new(),
                            kind: HoleKind::Simple,
                            exit_kind: None,
                            diameter: None,
                            extent: None,
                            bottom: None,
                            taper_angle: None,
                            specification: None,
                            allow_multi_profile_faces: None,
                        },
                    },
                ),
            ]),
            native_ref: None,
        });

        sync_active_configuration_resolutions(&mut ir);

        assert!(matches!(
            ir.model.configurations[0].feature_states[&feature_id].definition,
            FeatureDefinition::Pattern {
                pattern: PatternKind::Mirror { .. },
                ..
            }
        ));
        assert!(matches!(
            &ir.model.configurations[0].feature_states[&hole_id].definition,
            FeatureDefinition::Hole {
                placements,
                diameter: Some(Length(4.0)),
                extent: Some(Termination::Blind {
                    length: Length(12.0)
                }),
                bottom: Some(HoleBottom::Flat),
                ..
            } if placements.len() == 1
        ));

        let FeatureDefinition::Hole {
            placements,
            diameter,
            extent,
            bottom,
            ..
        } = &mut ir.model.configurations[0]
            .feature_states
            .get_mut(&hole_id)
            .expect("hole state")
            .definition
        else {
            unreachable!();
        };
        placements.clear();
        *diameter = Some(Length(8.0));
        *extent = Some(Termination::ThroughAll);
        *bottom = None;
        sync_active_configuration_resolutions(&mut ir);
        assert!(matches!(
            &ir.model.configurations[0].feature_states[&hole_id].definition,
            FeatureDefinition::Hole {
                placements,
                diameter: Some(Length(8.0)),
                extent: Some(Termination::ThroughAll),
                bottom: None,
                ..
            } if placements.len() == 1
        ));
    }

    #[test]
    fn incomplete_configuration_snapshots_are_reported_as_design_losses() {
        let mut ir = CadIr::empty(Units::default());
        let feature_id = FeatureId("feature".into());
        ir.model.features.push(Feature {
            id: feature_id.clone(),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: Vec::new(),
                active_child: None,
            },
            native_ref: None,
        });
        ir.model.parameters.push(DesignParameter {
            id: ParameterId("parameter".into()),
            owner: Some(feature_id),
            ordinal: 0,
            name: "D1".into(),
            expression: "1".into(),
            display: None,
            value: Some(ParameterValue::Integer(1)),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        ir.model.parameters.push(DesignParameter {
            id: ParameterId("unevaluated-parameter".into()),
            owner: None,
            ordinal: 1,
            name: "Text".into(),
            expression: "native text".into(),
            display: None,
            value: None,
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId("configuration".into()),
            ordinal: 0,
            active: true,
            source_index: Some(0),
            name: "Configuration".into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
            parameter_values: BTreeMap::new(),
            suppressed_features: Vec::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: None,
        });
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: std::collections::BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "1 configuration(s) lack a complete evaluated feature snapshot; 1 configuration(s) lack a complete evaluated parameter snapshot."
        }));

        ir.source = Some(cadmpeg_ir::document::SourceMeta {
            format: "sldprt".into(),
            attributes: BTreeMap::from([("sw_configuration_0_needs_update".into(), "YES".into())]),
        });
        report.losses.clear();
        append_design_losses(&ir, &mut report);
        assert!(!report
            .losses
            .iter()
            .any(|loss| { loss.message.contains("complete evaluated feature snapshot") }));
    }

    #[test]
    fn active_configuration_snapshots_final_neutral_design_state() {
        let mut ir = CadIr::empty(Units::default());
        let feature_id = FeatureId("feature".into());
        ir.model.features.push(Feature {
            id: feature_id.clone(),
            ordinal: 0,
            name: None,
            suppressed: Some(true),
            parent: None,
            dependencies: vec![FeatureId("dependency".into())],
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: vec![BodyId("body".into())],
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: Vec::new(),
                active_child: None,
            },
            native_ref: None,
        });
        let parameter_id = ParameterId("parameter".into());
        ir.model.parameters.push(DesignParameter {
            id: parameter_id.clone(),
            owner: Some(feature_id.clone()),
            ordinal: 0,
            name: "D1".into(),
            expression: "12mm".into(),
            value: Some(ParameterValue::Length(Length(12.0))),
            dependencies: Vec::new(),
            display: None,
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        for (ordinal, active) in [(0, true), (1, false)] {
            ir.model.configurations.push(DesignConfiguration {
                id: ConfigurationId(format!("configuration-{ordinal}")),
                ordinal,
                active,
                source_index: Some(ordinal),
                name: format!("Configuration {ordinal}"),
                material: None,
                properties: BTreeMap::new(),
                bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
                parameter_values: BTreeMap::new(),
                suppressed_features: Vec::new(),
                parameter_overrides: BTreeMap::new(),
                feature_states: BTreeMap::new(),
                native_ref: None,
            });
        }

        snapshot_active_configuration(&mut ir);

        assert_eq!(
            ir.model.configurations[0].parameter_values[&parameter_id],
            ParameterValue::Length(Length(12.0))
        );
        assert_eq!(
            ir.model.configurations[0].feature_states[&feature_id],
            ConfigurationFeatureState {
                suppressed: true,
                dependencies: vec![FeatureId("dependency".into())],
                outputs: vec![BodyId("body".into())],
                definition: FeatureDefinition::TreeNode {
                    role: FeatureTreeNodeRole::History,
                    children: Vec::new(),
                    active_child: None,
                },
            }
        );
        assert!(ir.model.configurations[1].parameter_values.is_empty());
        assert!(ir.model.configurations[1].feature_states.is_empty());

        ir.model.configurations[0]
            .parameter_values
            .insert(parameter_id.clone(), ParameterValue::Length(Length(25.0)));
        ir.model.configurations[0]
            .feature_states
            .get_mut(&feature_id)
            .expect("active feature state")
            .suppressed = false;
        snapshot_active_configuration(&mut ir);
        assert_eq!(
            ir.model.configurations[0].parameter_values[&parameter_id],
            ParameterValue::Length(Length(25.0))
        );
        assert!(!ir.model.configurations[0].feature_states[&feature_id].suppressed);
    }

    #[test]
    fn design_completeness_rejects_unresolved_and_unaudited_typed_families() {
        let mut ir = CadIr::empty(Units::default());
        let feature = |id: &str, ordinal, definition| Feature {
            id: FeatureId(id.into()),
            ordinal,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: None,
        };
        ir.model.features.push(feature(
            "complete-helix",
            0,
            FeatureDefinition::Helix {
                axis_origin: Point3::new(0.0, 0.0, 0.0),
                axis_direction: Vector3::new(0.0, 0.0, 1.0),
                radius: Length(1.0),
                pitch: Length(2.0),
                revolutions: 3.0,
                start_angle: Angle(0.0),
                clockwise: false,
                radial_growth: None,
                cone_angle: None,
                segment_turns: None,
                construction_style: None,
            },
        ));
        ir.model.features.push(feature(
            "incomplete-dome",
            1,
            FeatureDefinition::Dome {
                faces: FaceSelection::Native("face".into()),
                height: None,
                elliptical: None,
                reverse: None,
            },
        ));
        ir.model.features.push(feature(
            "unresolved-plane",
            2,
            FeatureDefinition::DatumPlaneUnresolved,
        ));
        ir.model.features.push(feature(
            "unaudited-stored-geometry",
            3,
            FeatureDefinition::StoredGeometry,
        ));
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: std::collections::BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "3 typed feature(s) retain native or unresolved required operation operands."
        }));
    }

    #[test]
    fn design_completeness_audits_direct_body_and_shape_families() {
        let mut ir = CadIr::empty(Units::default());
        let body = BodyId("body".into());
        let source = FeatureId("base".into());
        let mut push = |id: &str, ordinal, dependencies, outputs, definition| {
            ir.model.features.push(Feature {
                id: FeatureId(id.into()),
                ordinal,
                name: None,
                suppressed: Some(false),
                parent: None,
                dependencies,
                source_properties: BTreeMap::new(),
                source_tag: None,
                source_text: None,
                source_content: Vec::new(),
                outputs,
                definition,
                native_ref: None,
            });
        };
        push(
            "base",
            0,
            Vec::new(),
            Vec::new(),
            FeatureDefinition::BaseFeature {
                bodies: BodySelection::Bodies(vec![body.clone()]),
            },
        );
        push(
            "stored",
            1,
            Vec::new(),
            vec![body.clone()],
            FeatureDefinition::StoredGeometry,
        );
        push(
            "derived",
            2,
            vec![source.clone()],
            Vec::new(),
            FeatureDefinition::DerivedGeometry { source },
        );
        push(
            "mirror",
            3,
            Vec::new(),
            Vec::new(),
            FeatureDefinition::MirrorShape {
                source: BodySelection::Bodies(vec![body.clone()]),
                plane_origin: Point3::new(0.0, 0.0, 0.0),
                plane_normal: Vector3::new(0.0, 0.0, 1.0),
                plane_reference: Some(FaceSelection::Native("plane".into())),
            },
        );
        push(
            "sew",
            4,
            Vec::new(),
            Vec::new(),
            FeatureDefinition::SewBodies {
                bodies: BodySelection::Bodies(vec![body.clone()]),
                gap_tolerance: None,
            },
        );
        push(
            "trim",
            5,
            Vec::new(),
            Vec::new(),
            FeatureDefinition::TrimBodies {
                targets: BodySelection::Bodies(vec![body.clone()]),
                tools: BodySelection::Bodies(vec![body.clone()]),
                keep: cadmpeg_ir::features::BodyTrimSide::Unresolved,
            },
        );
        push(
            "import",
            6,
            Vec::new(),
            Vec::new(),
            FeatureDefinition::ImportedGeometry {
                path: "  ".into(),
                format: cadmpeg_ir::features::GeometryImportFormat::Step,
            },
        );
        push(
            "section",
            7,
            Vec::new(),
            Vec::new(),
            FeatureDefinition::SectionShape {
                first: BodySelection::Bodies(vec![body.clone()]),
                second: BodySelection::Bodies(vec![body]),
                approximate: None,
            },
        );
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "5 typed feature(s) retain native or unresolved required operation operands."
        }));
    }

    #[test]
    fn design_completeness_audits_typed_construction_families() {
        let mut ir = CadIr::empty(Units::default());
        let body = BodyId("body".into());
        let sketch = cadmpeg_ir::sketches::SketchId("sketch".into());
        let face = FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId("face".into())]);
        let definitions = [
            FeatureDefinition::PointGeometry {
                position: Point3::new(0.0, 0.0, 0.0),
            },
            FeatureDefinition::Primitive {
                solid: cadmpeg_ir::features::PrimitiveSolid::Box {
                    length: Length(1.0),
                    width: Length(2.0),
                    height: Length(3.0),
                },
                op: BooleanOp::NewBody,
            },
            FeatureDefinition::SheetMetalBaseFlange {
                profile: cadmpeg_ir::features::ProfileRef::Sketch(sketch),
                thickness: Length(1.0),
                side: cadmpeg_ir::features::SheetMetalThicknessSide::Symmetric,
            },
            FeatureDefinition::Polyline {
                points: vec![Point3::new(0.0, 0.0, 0.0)],
                closed: false,
            },
            FeatureDefinition::Block {
                dimensions: None,
                placement: None,
                op: BooleanOp::Unresolved,
            },
            FeatureDefinition::ProjectOnSurface {
                sources: PathRef::Native("sources".into()),
                support_face: face.clone(),
                direction: Vector3::new(0.0, 0.0, 1.0),
                mode: cadmpeg_ir::features::SurfaceProjectionMode::All,
                height: Length(0.0),
                offset: Length(0.0),
            },
            FeatureDefinition::Coil {
                construction: cadmpeg_ir::features::CoilConstruction {
                    placement: cadmpeg_ir::features::CoilPlacement::Native {
                        native_ref: "placement".into(),
                    },
                    diameter: Length(10.0),
                    extent: cadmpeg_ir::features::CoilExtent::RevolutionsHeight {
                        revolutions: 2.0,
                        height: Length(5.0),
                    },
                    section: cadmpeg_ir::features::CoilSection::Circular {
                        diameter: Length(1.0),
                    },
                    section_placement: cadmpeg_ir::features::CoilSectionPlacement::Center,
                    clockwise: false,
                    taper: Angle(0.0),
                },
                result: cadmpeg_ir::features::CoilResult::NewBody,
            },
            FeatureDefinition::Sphere {
                center: Point3::new(0.0, 0.0, 0.0),
                radius: Length(1.0),
                op: BooleanOp::Unresolved,
            },
            FeatureDefinition::FaceBlend {
                first_faces: face.clone(),
                second_faces: face.clone(),
                radius: RadiusSpec::Variable { points: Vec::new() },
            },
            FeatureDefinition::BoundaryFill {
                tools: BodySelection::Bodies(vec![body]),
                cells: Vec::new(),
            },
        ];
        for (ordinal, definition) in definitions.into_iter().enumerate() {
            ir.model.features.push(Feature {
                id: FeatureId(format!("construction-{ordinal}")),
                ordinal: ordinal as u64,
                name: None,
                suppressed: Some(false),
                parent: None,
                dependencies: Vec::new(),
                source_properties: BTreeMap::new(),
                source_tag: None,
                source_text: None,
                source_content: Vec::new(),
                outputs: Vec::new(),
                definition,
                native_ref: None,
            });
        }
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "7 typed feature(s) retain native or unresolved required operation operands."
        }));
    }

    #[test]
    fn binder_completeness_requires_resolved_targets_and_shape_arity() {
        let mut ir = CadIr::empty(Units::default());
        let source = FeatureId("source".into());
        let feature = |id: &str, ordinal, dependencies, definition| Feature {
            id: FeatureId(id.into()),
            ordinal,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies,
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: None,
        };
        ir.model.features.push(feature(
            "source",
            0,
            Vec::new(),
            FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: Vec::new(),
                active_child: None,
            },
        ));
        let shape = |sources| FeatureDefinition::Binder {
            sources,
            construction: cadmpeg_ir::features::BinderConstruction::Shape {
                trace_support: false,
            },
        };
        ir.model.features.push(feature(
            "complete",
            1,
            vec![source.clone()],
            shape(vec![cadmpeg_ir::features::BinderSource {
                target: cadmpeg_ir::features::BinderTarget::Feature {
                    feature: source.clone(),
                },
                subelements: vec!["Face1".into()],
            }]),
        ));
        ir.model.features.push(feature(
            "native",
            2,
            Vec::new(),
            shape(vec![cadmpeg_ir::features::BinderSource {
                target: cadmpeg_ir::features::BinderTarget::Native {
                    reference: "source".into(),
                },
                subelements: Vec::new(),
            }]),
        ));
        ir.model.features.push(feature(
            "multiple-shape-sources",
            3,
            Vec::new(),
            shape(vec![
                cadmpeg_ir::features::BinderSource {
                    target: cadmpeg_ir::features::BinderTarget::External {
                        document: "a.FCStd".into(),
                        object: "Body".into(),
                    },
                    subelements: Vec::new(),
                },
                cadmpeg_ir::features::BinderSource {
                    target: cadmpeg_ir::features::BinderTarget::External {
                        document: "b.FCStd".into(),
                        object: "Body".into(),
                    },
                    subelements: Vec::new(),
                },
            ]),
        ));
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "2 typed feature(s) retain native or unresolved required operation operands."
        }));
    }

    #[test]
    fn post_process_completeness_delegates_to_the_wrapped_operation() {
        let mut ir = CadIr::empty(Units::default());
        let post_process = |operation| FeatureDefinition::PostProcess {
            operation: Box::new(operation),
            refine: true,
            fuzzy_tolerance: cadmpeg_ir::features::FuzzyTolerance::KernelDefault,
        };
        for (ordinal, definition) in [
            post_process(FeatureDefinition::Helix {
                axis_origin: Point3::new(0.0, 0.0, 0.0),
                axis_direction: Vector3::new(0.0, 0.0, 1.0),
                radius: Length(1.0),
                pitch: Length(2.0),
                revolutions: 3.0,
                start_angle: Angle(0.0),
                clockwise: false,
                radial_growth: None,
                cone_angle: None,
                segment_turns: None,
                construction_style: None,
            }),
            post_process(post_process(FeatureDefinition::DatumPlaneUnresolved)),
        ]
        .into_iter()
        .enumerate()
        {
            ir.model.features.push(Feature {
                id: FeatureId(format!("post-process-{ordinal}")),
                ordinal: ordinal as u64,
                name: None,
                suppressed: Some(false),
                parent: None,
                dependencies: Vec::new(),
                source_properties: BTreeMap::new(),
                source_tag: None,
                source_text: None,
                source_content: Vec::new(),
                outputs: Vec::new(),
                definition,
                native_ref: None,
            });
        }
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "1 typed feature(s) retain native or unresolved required operation operands."
        }));
    }

    #[test]
    fn design_completeness_recurses_through_pattern_operands() {
        let mut ir = CadIr::empty(Units::default());
        let seed = cadmpeg_ir::features::PatternSeed::Feature(FeatureId("seed".into()));
        for (ordinal, pattern) in [
            (
                0,
                PatternKind::LinearOffsets {
                    direction: None,
                    offsets: vec![Length(0.0), Length(10.0)],
                },
            ),
            (
                1,
                PatternKind::CurveDriven {
                    path: Some(PathRef::Native("path".into())),
                    spacing: Length(10.0),
                    count: 2,
                },
            ),
            (
                2,
                PatternKind::Scale {
                    center: cadmpeg_ir::features::PatternScaleCenter::Native("center".into()),
                    final_factor: 2.0,
                    count: 2,
                },
            ),
            (
                3,
                PatternKind::Composite {
                    stages: vec![cadmpeg_ir::features::PatternStage {
                        pattern: Box::new(PatternKind::CurveDriven {
                            path: None,
                            spacing: Length(10.0),
                            count: 2,
                        }),
                        combination: cadmpeg_ir::features::PatternStageCombination::Initialize,
                    }],
                },
            ),
            (
                4,
                PatternKind::Circular {
                    axis_origin: Point3::new(0.0, 0.0, 0.0),
                    axis_dir: Vector3::new(0.0, 0.0, 1.0),
                    angle: Angle(std::f64::consts::TAU),
                    count: 4,
                },
            ),
        ] {
            ir.model.features.push(Feature {
                id: FeatureId(format!("pattern-{ordinal}")),
                ordinal,
                name: None,
                suppressed: Some(false),
                parent: None,
                dependencies: Vec::new(),
                source_properties: BTreeMap::new(),
                source_tag: None,
                source_text: None,
                source_content: Vec::new(),
                outputs: Vec::new(),
                definition: FeatureDefinition::Pattern {
                    seeds: vec![seed.clone()],
                    pattern,
                },
                native_ref: None,
            });
        }
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "4 typed feature(s) retain native or unresolved required operation operands."
        }));
    }

    #[test]
    fn design_completeness_checks_secondary_sweep_and_loft_paths() {
        let mut ir = CadIr::empty(Units::default());
        let sketch = cadmpeg_ir::sketches::SketchId("sketch".into());
        let profile = cadmpeg_ir::features::ProfileRef::Sketch(sketch.clone());
        let path = PathRef::Sketch(sketch);
        let sweep = |sections, orientation| FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Profile(profile.clone()),
            sections,
            path: Some(path.clone()),
            mode: cadmpeg_ir::features::SweepMode::Surface,
            orientation,
            transition: None,
            transformation: None,
            path_tangent: false,
            linearize: false,
            twist: None,
            path_extent: None,
            guide_rail: None,
            taper: None,
            scale: None,
            allow_multi_profile_faces: None,
        };
        let definitions = [
            sweep(
                vec![cadmpeg_ir::features::SweepSection::Profile(
                    cadmpeg_ir::features::ProfileRef::Native("section".into()),
                )],
                None,
            ),
            sweep(
                Vec::new(),
                Some(cadmpeg_ir::features::SweepOrientation::Auxiliary {
                    path: PathRef::Native("auxiliary".into()),
                    tangent: false,
                    curvilinear: false,
                }),
            ),
            FeatureDefinition::Loft {
                sections: vec![
                    cadmpeg_ir::features::LoftSection::Profile(profile.clone()),
                    cadmpeg_ir::features::LoftSection::Profile(profile.clone()),
                ],
                guides: Vec::new(),
                centerline: Some(PathRef::Native("centerline".into())),
                op: BooleanOp::NewBody,
                closed: false,
                solid: false,
                ruled: false,
                max_degree: None,
                check_compatibility: None,
                allow_multi_profile_faces: None,
            },
            sweep(Vec::new(), None),
        ];
        for (ordinal, definition) in definitions.into_iter().enumerate() {
            ir.model.features.push(Feature {
                id: FeatureId(format!("path-feature-{ordinal}")),
                ordinal: ordinal as u64,
                name: None,
                suppressed: Some(false),
                parent: None,
                dependencies: Vec::new(),
                source_properties: BTreeMap::new(),
                source_tag: None,
                source_text: None,
                source_content: Vec::new(),
                outputs: Vec::new(),
                definition,
                native_ref: None,
            });
        }
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "3 typed feature(s) retain native or unresolved required operation operands."
        }));
    }

    #[test]
    fn design_completeness_rejects_explicitly_unresolved_operation_fields() {
        let mut ir = CadIr::empty(Units::default());
        let sketch = cadmpeg_ir::sketches::SketchId("sketch".into());
        let profile = cadmpeg_ir::features::ProfileRef::Sketch(sketch.clone());
        let path = PathRef::Sketch(sketch);
        let face = FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId("face".into())]);
        let extrude = |direction, termination| FeatureDefinition::Extrude {
            profile: profile.clone(),
            direction,
            start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination,
                    draft: None,
                    offset: None,
                },
            },
            op: BooleanOp::NewBody,
            direction_source: None,
            solid: Some(true),
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        };
        let definitions = [
            FeatureDefinition::ProjectedCurve {
                source: path.clone(),
                target_faces: face.clone(),
                direction: cadmpeg_ir::features::CurveProjectionDirection::State(
                    cadmpeg_ir::features::CurveProjectionDirectionState::Unresolved,
                ),
                bidirectional: Some(false),
            },
            extrude(
                cadmpeg_ir::features::ExtrudeDirection::Unresolved,
                cadmpeg_ir::features::Termination::Blind {
                    length: Length(10.0),
                },
            ),
            extrude(
                cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
                cadmpeg_ir::features::Termination::ToVertex {
                    vertex: cadmpeg_ir::features::VertexSelection::Native("vertex".into()),
                },
            ),
            FeatureDefinition::OffsetSurface {
                faces: face.clone(),
                distance: None,
            },
            FeatureDefinition::KnitSurface {
                faces: face.clone(),
                merge_entities: None,
                create_solid: None,
                gap_tolerance: None,
            },
            FeatureDefinition::ExtendSurface {
                faces: face.clone(),
                distance: Some(Length(10.0)),
                method: cadmpeg_ir::features::SurfaceExtension::Unresolved,
            },
            FeatureDefinition::FilledSurface {
                boundary: cadmpeg_ir::features::SurfaceBoundary::Path(path.clone()),
                support_faces: face.clone(),
                continuity: None,
                boundary_continuities: Vec::new(),
                merge_result: Some(false),
            },
            FeatureDefinition::TrimSurface {
                faces: face.clone(),
                tool: path.clone(),
                keep: cadmpeg_ir::features::TrimRegion::Unresolved,
            },
            FeatureDefinition::Draft {
                faces: face.clone(),
                neutral_plane: face.clone(),
                parting_tool: None,
                pull_direction: None,
                pull_plane: None,
                angle: None,
                outward: None,
            },
            FeatureDefinition::ProjectedCurve {
                source: path,
                target_faces: face,
                direction: cadmpeg_ir::features::CurveProjectionDirection::State(
                    cadmpeg_ir::features::CurveProjectionDirectionState::TargetNormal,
                ),
                bidirectional: Some(false),
            },
        ];
        for (ordinal, definition) in definitions.into_iter().enumerate() {
            ir.model.features.push(Feature {
                id: FeatureId(format!("operation-{ordinal}")),
                ordinal: ordinal as u64,
                name: None,
                suppressed: Some(false),
                parent: None,
                dependencies: Vec::new(),
                source_properties: BTreeMap::new(),
                source_tag: None,
                source_text: None,
                source_content: Vec::new(),
                outputs: Vec::new(),
                definition,
                native_ref: None,
            });
        }
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "9 typed feature(s) retain native or unresolved required operation operands."
        }));
    }

    #[test]
    fn empty_required_operands_are_incomplete_design_semantics() {
        let mut ir = CadIr::empty(Units::default());
        let feature = |ordinal, definition| Feature {
            id: FeatureId(format!("feature-{ordinal}")),
            ordinal,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: None,
        };
        ir.model.features.extend([
            feature(
                0,
                FeatureDefinition::Fillet {
                    groups: vec![cadmpeg_ir::features::FilletGroup {
                        edges: EdgeSelection::Edges(Vec::new()),
                        radius: RadiusSpec::Constant {
                            radius: Length(1.0),
                        },
                        tangency_weight: None,
                    }],
                },
            ),
            feature(
                1,
                FeatureDefinition::DeleteFace {
                    faces: FaceSelection::Faces(Vec::new()),
                    heal: false,
                },
            ),
            feature(
                2,
                FeatureDefinition::DeleteBody {
                    bodies: BodySelection::Bodies(Vec::new()),
                    mode: BodyRetentionMode::DeleteSelected,
                },
            ),
            feature(
                3,
                FeatureDefinition::CompositeCurve {
                    segments: vec![PathRef::Edges(Vec::new())],
                    closed: false,
                },
            ),
            feature(
                4,
                FeatureDefinition::Shell {
                    bodies: None,
                    removed_faces: FaceSelection::Faces(Vec::new()),
                    thickness: Some(Length(1.0)),
                    outward: Some(false),
                    mode: None,
                    join: None,
                    resolve_intersections: None,
                    allow_self_intersections: None,
                },
            ),
            feature(
                5,
                FeatureDefinition::FilledSurface {
                    boundary: cadmpeg_ir::features::SurfaceBoundary::Edges(EdgeSelection::Edges(
                        vec![EdgeId("boundary".into())],
                    )),
                    support_faces: FaceSelection::Faces(Vec::new()),
                    continuity: Some(SurfaceContinuity::Contact),
                    boundary_continuities: Vec::new(),
                    merge_result: Some(false),
                },
            ),
            feature(
                6,
                FeatureDefinition::RuledSurface {
                    edges: EdgeSelection::Edges(vec![EdgeId("boundary".into())]),
                    support_faces: FaceSelection::Faces(Vec::new()),
                    mode: RuledSurfaceMode::Direction {
                        direction: Vector3::new(0.0, 0.0, 1.0),
                        distance: Length(1.0),
                    },
                    angle: None,
                    alternate_face: None,
                    corner: None,
                },
            ),
            feature(
                7,
                FeatureDefinition::Fillet {
                    groups: vec![cadmpeg_ir::features::FilletGroup {
                        edges: EdgeSelection::Edges(vec![EdgeId("edge".into())]),
                        radius: RadiusSpec::Variable { points: Vec::new() },
                        tangency_weight: None,
                    }],
                },
            ),
        ]);
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: std::collections::BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "6 typed feature(s) retain native or unresolved required operation operands."
        }));
    }

    #[test]
    fn hole_completeness_checks_optional_operands_when_present() {
        let mut ir = CadIr::empty(Units::default());
        let hole = |profile, exit_kind| FeatureDefinition::Hole {
            profile,
            profile_filter: None,
            face: None,
            position: None,
            direction: None,
            placements: vec![cadmpeg_ir::features::HolePlacement::Directed {
                position: Point3::new(0.0, 0.0, 0.0),
                direction: Vector3::new(0.0, 0.0, 1.0),
            }],
            kind: cadmpeg_ir::features::HoleKind::Simple,
            exit_kind,
            diameter: Some(Length(5.0)),
            extent: Some(cadmpeg_ir::features::Termination::ThroughAll),
            bottom: None,
            taper_angle: None,
            specification: None,
            allow_multi_profile_faces: None,
        };
        for (ordinal, definition) in [
            hole(
                Some(cadmpeg_ir::features::ProfileRef::Native("profile".into())),
                None,
            ),
            hole(
                None,
                Some(cadmpeg_ir::features::HoleKind::Unresolved {
                    form: None,
                    counterbore_diameter: None,
                    counterbore_depth: None,
                    countersink_diameter: None,
                    countersink_angle: None,
                }),
            ),
            hole(None, None),
        ]
        .into_iter()
        .enumerate()
        {
            ir.model.features.push(Feature {
                id: FeatureId(format!("hole-{ordinal}")),
                ordinal: ordinal as u64,
                name: None,
                suppressed: Some(false),
                parent: None,
                dependencies: Vec::new(),
                source_properties: BTreeMap::new(),
                source_tag: None,
                source_text: None,
                source_content: Vec::new(),
                outputs: Vec::new(),
                definition,
                native_ref: None,
            });
        }
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "2 typed feature(s) retain native or unresolved required operation operands."
        }));
    }

    #[test]
    fn incomplete_parameter_semantics_are_reported_as_design_losses() {
        let mut ir = CadIr::empty(Units::default());
        let owner = FeatureId("owner".into());
        ir.model.features.push(Feature {
            id: owner.clone(),
            ordinal: 0,
            name: Some("Boss-Extrude1".into()),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: Vec::new(),
                active_child: None,
            },
            native_ref: None,
        });
        ir.model.parameters.push(DesignParameter {
            id: ParameterId("base-parameter".into()),
            owner: Some(owner.clone()),
            ordinal: 0,
            name: "D0".into(),
            expression: "1mm".into(),
            display: None,
            value: Some(cadmpeg_ir::features::ParameterValue::Length(Length(1.0))),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        ir.model.parameters.push(DesignParameter {
            id: ParameterId("parameter".into()),
            owner: Some(owner.clone()),
            ordinal: 1,
            name: "D1".into(),
            expression: "\"D0@Boss-Extrude1\" + Missing@Sketch1".into(),
            display: None,
            value: None,
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        ir.model.parameters.push(DesignParameter {
            id: ParameterId("bare-reference".into()),
            owner: Some(owner.clone()),
            ordinal: 2,
            name: "D2".into(),
            expression: "D99 + 1".into(),
            display: None,
            value: Some(cadmpeg_ir::features::ParameterValue::Real(1.0)),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        ir.model.parameters.push(DesignParameter {
            id: ParameterId("malformed-reference".into()),
            owner: Some(owner.clone()),
            ordinal: 3,
            name: "D3".into(),
            expression: "\"D0@Boss-Extrude1".into(),
            display: None,
            value: Some(cadmpeg_ir::features::ParameterValue::Real(1.0)),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        let future = ParameterId("future".into());
        ir.model.parameters.push(DesignParameter {
            id: ParameterId("forward-reference".into()),
            owner: Some(owner.clone()),
            ordinal: 4,
            name: "D4".into(),
            expression: "D5".into(),
            display: None,
            value: Some(cadmpeg_ir::features::ParameterValue::Real(2.0)),
            dependencies: vec![future.clone()],
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        ir.model.parameters.push(DesignParameter {
            id: future,
            owner: Some(owner.clone()),
            ordinal: 5,
            name: "D5".into(),
            expression: "1".into(),
            display: None,
            value: Some(cadmpeg_ir::features::ParameterValue::Real(1.0)),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        ir.model.parameters.push(DesignParameter {
            id: ParameterId("omitted-dependency".into()),
            owner: Some(owner.clone()),
            ordinal: 6,
            name: "D6".into(),
            expression: "D0 + 1mm".into(),
            display: None,
            value: Some(cadmpeg_ir::features::ParameterValue::Length(Length(2.0))),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        ir.model.parameters.push(DesignParameter {
            id: ParameterId("cached-unsupported-expression".into()),
            owner: Some(owner.clone()),
            ordinal: 7,
            name: "D7".into(),
            expression: "unsupported(1)".into(),
            display: None,
            value: Some(cadmpeg_ir::features::ParameterValue::Real(1.0)),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        for (id, ordinal, name) in [
            ("empty", 8, ""),
            ("shared-a", 9, "Shared"),
            ("shared-b", 10, "Shared"),
            ("ordinal", 10, "Unique"),
        ] {
            ir.model.parameters.push(DesignParameter {
                id: ParameterId(format!("identity:{id}")),
                owner: Some(owner.clone()),
                ordinal,
                name: name.into(),
                expression: "1".into(),
                display: None,
                value: Some(cadmpeg_ir::features::ParameterValue::Real(1.0)),
                dependencies: Vec::new(),
                properties: BTreeMap::new(),
                pmi: None,
                native_ref: None,
            });
        }
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: std::collections::BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "1 parameter(s) lack an evaluated scalar; 3 parameter expression(s) contain unresolved, ambiguous, or malformed parameter references; 4 parameter expression(s) cannot regenerate a finite typed value; 1 parameter record(s) contain missing or non-preceding dependency edges; 2 parameter record(s) have dependency edges inconsistent with their expressions; 1 dependency-driven parameter(s) disagree with their evaluated expressions."
        }));
        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "1 parameter record(s) have empty names; 2 parameter record(s) share owner-local names; 2 parameter record(s) share owner-local ordinals."
        }));
    }

    #[test]
    fn incoherent_feature_graph_is_reported_as_design_loss() {
        let mut ir = CadIr::empty(Units::default());
        let first = FeatureId("first".into());
        let second = FeatureId("second".into());
        let missing = FeatureId("missing".into());
        let feature = |id, ordinal, parent, dependencies| Feature {
            id,
            ordinal,
            name: None,
            suppressed: Some(false),
            parent,
            dependencies,
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: Vec::new(),
                active_child: None,
            },
            native_ref: None,
        };
        ir.model
            .features
            .push(feature(first.clone(), 0, None, vec![second.clone()]));
        ir.model
            .features
            .push(feature(second, 1, Some(first.clone()), vec![first]));
        ir.model.features.push(feature(
            FeatureId("third".into()),
            1,
            Some(missing),
            Vec::new(),
        ));
        ir.model.features[0].source_content = vec![
            FeatureSourceContent::Feature(FeatureId("second".into())),
            FeatureSourceContent::Feature(FeatureId("second".into())),
        ];
        ir.model.features[1].source_content =
            vec![FeatureSourceContent::Feature(FeatureId("third".into()))];
        ir.model.features[2].source_content = vec![FeatureSourceContent::Parameter(ParameterId(
            "missing-parameter".into(),
        ))];
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: std::collections::BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "2 feature record(s) contain missing, repeated, or non-preceding parent/dependency edges; 2 feature record(s) share regeneration ordinals."
        }));
        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "3 feature record(s) contain missing, repeated, misowned, or structurally inconsistent source-content references."
        }));
    }

    #[test]
    fn incoherent_feature_outputs_are_reported_as_design_loss() {
        let mut ir = cadmpeg_ir::examples::unit_cube();
        ir.model.features.clear();
        ir.model.parameters.clear();
        let body = ir.model.bodies[0].id.clone();
        let feature = |id: &str, ordinal: u64, outputs: Vec<BodyId>| Feature {
            id: FeatureId(id.into()),
            ordinal,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs,
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: Vec::new(),
                active_child: None,
            },
            native_ref: None,
        };
        ir.model
            .features
            .push(feature("duplicate", 0, vec![body.clone(), body]));
        ir.model
            .features
            .push(feature("missing", 1, vec![BodyId("missing-body".into())]));
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: std::collections::BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "2 feature record(s) contain missing or repeated output body references."
        }));
    }

    #[test]
    fn configuration_partitions_require_explicit_source_identity() {
        let mut ir = CadIr::empty(Units::default());
        let configuration = |id: &str, ordinal, source_index| DesignConfiguration {
            id: ConfigurationId(id.into()),
            ordinal,
            active: false,
            source_index,
            name: id.into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: cadmpeg_ir::ConfigurationBodies::Unresolved,
            parameter_values: BTreeMap::new(),
            suppressed_features: Vec::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: Some(format!("native:{id}")),
        };
        ir.model
            .configurations
            .push(configuration("explicit", 0, Some(5)));
        ir.model
            .configurations
            .push(configuration("inferred", 9, None));
        ir.model
            .configurations
            .push(configuration("empty", 10, Some(8)));
        let first = BodyId("body:first".into());
        let second = BodyId("body:second".into());
        let third = BodyId("body:third".into());

        assign_configuration_bodies(
            &mut ir,
            &[
                (7, vec![third.clone()]),
                (5, vec![first.clone()]),
                (5, vec![second.clone()]),
            ],
        );

        assert_eq!(ir.model.configurations[0].source_index, Some(5));
        assert_eq!(ir.model.configurations[0].bodies, vec![first, second]);
        assert_eq!(ir.model.configurations[1].source_index, None);
        assert!(ir.model.configurations[1].bodies.is_unresolved());
        assert_eq!(ir.model.configurations[2].source_index, Some(8));
        assert!(ir.model.configurations[2].bodies.is_empty());
        assert_eq!(ir.model.configurations[3].source_index, Some(7));
        assert_eq!(ir.model.configurations[3].bodies, vec![third]);
        assert!(ir.model.configurations[3].native_ref.is_none());
    }

    #[test]
    fn duplicate_configuration_source_identity_does_not_select_a_partition() {
        let mut ir = CadIr::empty(Units::default());
        for ordinal in 0..2 {
            ir.model.configurations.push(DesignConfiguration {
                id: ConfigurationId(format!("configuration:{ordinal}")),
                ordinal,
                active: false,
                source_index: Some(5),
                name: format!("Configuration {ordinal}"),
                material: None,
                properties: BTreeMap::new(),
                bodies: cadmpeg_ir::ConfigurationBodies::Unresolved,
                parameter_values: BTreeMap::new(),
                suppressed_features: Vec::new(),
                parameter_overrides: BTreeMap::new(),
                feature_states: BTreeMap::new(),
                native_ref: Some(format!("native:{ordinal}")),
            });
        }
        let body = BodyId("body:partition".into());

        assign_configuration_bodies(&mut ir, &[(5, vec![body.clone()])]);

        assert!(ir.model.configurations[0].bodies.is_unresolved());
        assert!(ir.model.configurations[1].bodies.is_unresolved());
        assert_eq!(ir.model.configurations[2].source_index, Some(5));
        assert_eq!(ir.model.configurations[2].bodies, vec![body]);
        assert!(ir.model.configurations[2].native_ref.is_none());
    }

    #[test]
    fn inferred_partition_does_not_fabricate_active_configuration_identity() {
        let mut ir = CadIr::empty(Units::default());
        ir.source = Some(cadmpeg_ir::document::SourceMeta {
            attributes: BTreeMap::from([
                (
                    "active_parasolid_block".into(),
                    "Contents/Config-3-Partition".into(),
                ),
                ("sw_configuration_name".into(), "Default".into()),
            ]),
            ..Default::default()
        });
        let body = BodyId("body:active".into());

        assign_configuration_bodies(&mut ir, &[(3, vec![body.clone()])]);
        mark_active_configuration(&mut ir);

        assert_eq!(ir.model.configurations.len(), 1);
        let configuration = &ir.model.configurations[0];
        assert!(!configuration.active);
        assert_eq!(configuration.source_index, Some(3));
        assert_eq!(configuration.bodies, vec![body]);

        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };
        append_design_losses(&ir, &mut report);
        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "active configuration identity is unresolved; 0 of 1 configuration records are active."
        }));
    }

    #[test]
    fn duplicate_configuration_partition_identities_are_reported() {
        let mut ir = CadIr::empty(Units::default());
        for id in ["first", "second"] {
            ir.model.configurations.push(DesignConfiguration {
                id: ConfigurationId(id.into()),
                ordinal: ir.model.configurations.len() as u32,
                active: false,
                source_index: Some(5),
                name: id.into(),
                material: None,
                properties: BTreeMap::new(),
                bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
                parameter_values: BTreeMap::new(),
                suppressed_features: Vec::new(),
                parameter_overrides: BTreeMap::new(),
                feature_states: BTreeMap::new(),
                native_ref: Some(format!("native:{id}")),
            });
        }
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: std::collections::BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "2 configuration record(s) share non-unique geometry partition identities."
        }));
    }

    #[test]
    fn incomplete_configuration_names_are_reported() {
        let mut ir = CadIr::empty(Units::default());
        for (position, (ordinal, name)) in [(0, ""), (1, "Shared"), (2, "Shared"), (2, "Unique")]
            .into_iter()
            .enumerate()
        {
            ir.model.configurations.push(DesignConfiguration {
                id: ConfigurationId(format!("configuration:{position}")),
                ordinal,
                active: position == 1,
                source_index: Some(position as u32),
                name: name.into(),
                material: None,
                properties: BTreeMap::new(),
                bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
                parameter_values: BTreeMap::new(),
                suppressed_features: Vec::new(),
                parameter_overrides: BTreeMap::new(),
                feature_states: BTreeMap::new(),
                native_ref: Some(format!("native:{position}")),
            });
        }
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: std::collections::BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "1 configuration record(s) have empty names; 2 configuration record(s) share non-unique names; 2 configuration record(s) share regeneration ordinals."
        }));
    }

    #[test]
    fn active_configuration_partition_disagreement_is_reported() {
        let mut ir = CadIr::empty(Units::default());
        ir.source = Some(cadmpeg_ir::document::SourceMeta {
            format: "sldprt".into(),
            attributes: BTreeMap::from([(
                "active_parasolid_block".into(),
                "Contents/Config-3-Partition".into(),
            )]),
        });
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId("configuration".into()),
            ordinal: 0,
            active: true,
            source_index: Some(5),
            name: "Default".into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
            parameter_values: BTreeMap::new(),
            suppressed_features: Vec::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: Some("native:configuration".into()),
        });
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: std::collections::BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "active configuration identity does not resolve to active geometry partition 3."
        }));
    }

    #[test]
    fn incoherent_configuration_bodies_are_reported() {
        let mut ir = cadmpeg_ir::examples::unit_cube();
        let body = ir.model.bodies[0].id.clone();
        let configuration = |id: &str, ordinal, bodies| DesignConfiguration {
            id: ConfigurationId(id.into()),
            ordinal,
            active: ordinal == 0,
            source_index: Some(ordinal),
            name: id.into(),
            material: None,
            properties: BTreeMap::new(),
            bodies,
            parameter_values: BTreeMap::new(),
            suppressed_features: Vec::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: Some(format!("native:{id}")),
        };
        ir.model.configurations = vec![
            configuration(
                "duplicate",
                0,
                cadmpeg_ir::ConfigurationBodies::Resolved(vec![body.clone(), body]),
            ),
            configuration(
                "missing",
                1,
                cadmpeg_ir::ConfigurationBodies::Resolved(vec![BodyId("missing-body".into())]),
            ),
            configuration("unresolved", 2, cadmpeg_ir::ConfigurationBodies::Unresolved),
        ];
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: std::collections::BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message == "1 configuration record(s) have unresolved body membership; 2 configuration record(s) contain missing or repeated body references."
        }));
    }

    #[test]
    fn configuration_values_complete_parameters_without_baseline_values() {
        let mut ir = CadIr::empty(Units::default());
        let parameter = ParameterId("configured-parameter".into());
        ir.model.parameters.push(DesignParameter {
            id: parameter.clone(),
            owner: None,
            ordinal: 0,
            name: "Configured".into(),
            expression: "12mm".into(),
            display: None,
            value: None,
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId("configuration".into()),
            ordinal: 0,
            active: true,
            source_index: Some(0),
            name: "Default".into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
            parameter_values: BTreeMap::from([(parameter, ParameterValue::Length(Length(12.0)))]),
            suppressed_features: Vec::new(),
            parameter_overrides: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: Some("native:configuration".into()),
        });
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(!report.losses.iter().any(|loss| {
            loss.message
                .contains("complete evaluated parameter snapshot")
                || loss.message.contains("lack an evaluated scalar")
        }));
    }

    #[test]
    fn configuration_suppression_and_override_references_are_coherent() {
        let mut ir = CadIr::empty(Units::default());
        let feature = FeatureId("feature".into());
        let definition = FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: Vec::new(),
            active_child: None,
        };
        ir.model.features.push(Feature {
            id: feature.clone(),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: definition.clone(),
            native_ref: None,
        });
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId("configuration".into()),
            ordinal: 0,
            active: true,
            source_index: Some(0),
            name: "Default".into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
            parameter_values: BTreeMap::new(),
            suppressed_features: Vec::new(),
            parameter_overrides: BTreeMap::from([(ParameterId("missing".into()), "1mm".into())]),
            feature_states: BTreeMap::from([(
                feature,
                ConfigurationFeatureState {
                    suppressed: true,
                    dependencies: Vec::new(),
                    outputs: Vec::new(),
                    definition,
                },
            )]),
            native_ref: Some("native:configuration".into()),
        });
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message == "1 configuration(s) have missing, repeated, or feature-state-inconsistent suppression members; 1 configuration(s) reference missing parameter overrides."
        }));
    }

    #[test]
    fn native_planar_and_spatial_sketch_geometry_is_reported() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.sketch_entities.push(SketchEntity {
            id: SketchEntityId("planar-entity".into()),
            sketch: SketchId("planar-sketch".into()),
            construction: false,
            native_ref: Some("native:planar".into()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Native {
                native_kind: "SplineHandle".into(),
            },
        });
        ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
            id: SpatialSketchEntityId("spatial-entity".into()),
            sketch: SpatialSketchId("spatial-sketch".into()),
            construction: false,
            native_ref: Some("native:spatial".into()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SpatialSketchGeometry::Native {
                native_kind: "ReferenceCurve".into(),
            },
        });
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: std::collections::BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "2 sketch entity geometry record(s) retain native kinds without solved neutral geometry."
        }));
    }

    #[test]
    fn only_sketch_owned_relation_records_without_constraints_are_counted() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.features.push(Feature {
            id: FeatureId("sketch-feature".into()),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::default(),
                sketch: Some(SketchId("sketch".into())),
            },
            native_ref: Some("feature".into()),
        });
        ir.model.sketch_entities.push(SketchEntity {
            id: SketchEntityId("represented-geometry".into()),
            sketch: SketchId("sketch".into()),
            construction: false,
            native_ref: Some("geometry-marker".into()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Native {
                native_kind: "UnknownGeometry".into(),
            },
        });
        let marker = |id: &str, ordinal, kind| SketchInputEntity {
            id: id.into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal,
            offset: u64::from(ordinal),
            object_index: None,
            local_id: None,
            kind,
            state_value: None,
            coordinates_m: None,
            links: Vec::new(),
            link_selector: None,
        };
        let relation = FeatureInputRelationInstance {
            id: "relation-instance".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            family: FeatureInputRelationFamily::PointPointDistance,
            class_ref: "class".into(),
            feature_ref: "feature".into(),
            scalar_refs: vec!["scalar".into()],
            parameter_scalar_ref: Some("scalar".into()),
            display_scalar_ref: None,
            operands: Vec::new(),
        };
        let binding =
            |id: &str, class_ref: &str, scalar_ref: &str, ordinal| FeatureInputRelationBinding {
                id: id.into(),
                parent: "lane".into(),
                ordinal,
                offset: u64::from(ordinal),
                class_ref: class_ref.into(),
                family: FeatureInputRelationFamily::PointPointDistance,
                scalar_ref: scalar_ref.into(),
                feature_ref: Some("feature".into()),
            };
        let mut relation_marker = marker(
            "relation-marker",
            0,
            SketchInputKind::Relation(SketchRelationKind::Horizontal),
        );
        relation_marker.links.push(SketchInputLink {
            local_id: 1,
            entity_ref: "geometry-marker".into(),
        });
        let native = SldprtNative {
            feature_input_lanes: vec![FeatureInputLane {
                id: "lane".into(),
                configuration: None,
                native_payload: Vec::new(),
                classes: Vec::new(),
                names: Vec::new(),
                scalars: Vec::new(),
                relation_bindings: vec![
                    binding("grouped-binding", "class", "scalar", 0),
                    binding("orphan-binding", "other-class", "other-scalar", 1),
                ],
                relation_instances: vec![relation],
                body_selections: Vec::new(),
                edge_selections: Vec::new(),
                surface_selections: Vec::new(),
                generated_surface_identities: Vec::new(),
                references: Vec::new(),
                sketch_entities: vec![
                    relation_marker,
                    marker(
                        "dimension-handle",
                        1,
                        SketchInputKind::Relation(SketchRelationKind::Distance),
                    ),
                    marker("geometry-marker", 2, SketchInputKind::Native(99)),
                    marker(
                        "operandless-relation-marker",
                        3,
                        SketchInputKind::Relation(SketchRelationKind::Vertical),
                    ),
                ],
            }],
            ..SldprtNative::default()
        };

        assert_eq!(unprojected_sketch_relation_records(&ir, &native), 3);

        ir.model.features[0].definition = FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: Vec::new(),
            active_child: None,
        };
        assert_eq!(unprojected_sketch_relation_records(&ir, &native), 0);
    }

    #[test]
    fn native_relation_records_have_at_most_one_neutral_owner() {
        let mut ir = CadIr::empty(Units::default());
        let entity = |id: &str, native_ref: &str| SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: SketchId("sketch".into()),
            construction: false,
            native_ref: Some(native_ref.into()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Native {
                native_kind: "UnknownGeometry".into(),
            },
        };
        ir.model.sketch_entities = vec![
            entity("first", "relation-marker"),
            entity("second", "relation-marker"),
            entity("profile", "profile-stream-record"),
        ];
        let native = SldprtNative {
            feature_input_lanes: vec![FeatureInputLane {
                id: "lane".into(),
                configuration: None,
                native_payload: Vec::new(),
                classes: Vec::new(),
                names: Vec::new(),
                scalars: Vec::new(),
                relation_bindings: Vec::new(),
                relation_instances: Vec::new(),
                body_selections: Vec::new(),
                edge_selections: Vec::new(),
                surface_selections: Vec::new(),
                generated_surface_identities: Vec::new(),
                references: Vec::new(),
                sketch_entities: vec![
                    SketchInputEntity {
                        id: "relation-marker".into(),
                        parent: "lane".into(),
                        feature_ref: Some("feature".into()),
                        ordinal: 0,
                        offset: 0,
                        object_index: None,
                        local_id: None,
                        kind: SketchInputKind::Relation(SketchRelationKind::Horizontal),
                        state_value: None,
                        coordinates_m: None,
                        links: vec![SketchInputLink {
                            local_id: 1,
                            entity_ref: "geometry-marker".into(),
                        }],
                        link_selector: None,
                    },
                    SketchInputEntity {
                        id: "geometry-marker".into(),
                        parent: "lane".into(),
                        feature_ref: Some("feature".into()),
                        ordinal: 1,
                        offset: 1,
                        object_index: None,
                        local_id: Some(1),
                        kind: SketchInputKind::Native(99),
                        state_value: None,
                        coordinates_m: None,
                        links: Vec::new(),
                        link_selector: None,
                    },
                ],
            }],
            ..SldprtNative::default()
        };

        assert_eq!(multiply_projected_sketch_relation_records(&ir, &native), 1);
    }

    #[test]
    fn direct_feature_input_operations_require_unique_history_bindings() {
        let class_name = "moExtrusion_c";
        let mut lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: Vec::new(),
            classes: vec![FeatureInputClass {
                id: "class".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 10,
                name: class_name.into(),
                role: FeatureInputClassRole::Feature,
            }],
            names: vec![FeatureInputName {
                id: "name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 10 + 6 + class_name.len() as u64,
                object_id: Some(42),
                value: "Boss".into(),
            }],
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };
        let mut native = SldprtNative {
            feature_input_lanes: vec![lane.clone()],
            ..SldprtNative::default()
        };
        assert_eq!(unbound_feature_input_operation_objects(&native), 1);

        native.feature_histories.push(FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![NativeFeature {
                id: "feature".into(),
                parent: "history".into(),
                xml_tag: "Extrusion".into(),
                tree_parent: None,
                source_id: Some("42".into()),
                parent_source_id: None,
                ordinal: 0,
                name: "Boss".into(),
                kind: "Extrusion".into(),
                input_class: Some(class_name.into()),
                suppressed: false,
                parameters: BTreeMap::new(),
                dimension_properties: BTreeMap::new(),
                properties: BTreeMap::new(),
                text: None,
                content: Vec::new(),
            }],
        });
        assert_eq!(unbound_feature_input_operation_objects(&native), 0);
        native.feature_histories[0].features[0].input_class = Some("moSweep_c".into());
        assert_eq!(unbound_feature_input_operation_objects(&native), 1);
        native.feature_histories[0].features[0].input_class = Some(class_name.into());
        native.feature_histories[0].features[0].source_id = None;
        assert_eq!(unbound_feature_input_operation_objects(&native), 0);
        let mut duplicate = native.feature_histories[0].features[0].clone();
        duplicate.id = "duplicate-feature".into();
        native.feature_histories[0].features.push(duplicate);
        assert_eq!(unbound_feature_input_operation_objects(&native), 1);

        lane.names[0].offset += 1;
        native.feature_input_lanes = vec![lane];
        assert_eq!(unbound_feature_input_operation_objects(&native), 0);
    }

    #[test]
    fn native_dimension_subtypes_are_reported() {
        let mut ir = CadIr::empty(Units::default());
        let owner = FeatureId("owner".into());
        ir.model.features.push(Feature {
            id: owner.clone(),
            ordinal: 0,
            name: Some("Feature".into()),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: Vec::new(),
                active_child: None,
            },
            native_ref: None,
        });
        ir.model.parameters.push(DesignParameter {
            id: ParameterId("parameter".into()),
            owner: Some(owner),
            ordinal: 0,
            name: "D1".into(),
            expression: "1".into(),
            display: None,
            value: Some(ParameterValue::Real(1.0)),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: Some(ParameterPmi {
                subtype: PmiDimensionSubtype::Native("Ordinate".into()),
                precision: 3,
                display_text: None,
                basic: false,
                inspection: false,
                reference_only: false,
                native_ref: "native:pmi".into(),
            }),
            native_ref: None,
        });
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: std::collections::BTreeMap::new(),
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "0 semantic dimension record(s) are not bound to parameters; 1 parameter dimension(s) retain native subtypes."
        }));
    }

    #[test]
    fn geometry_report_surfaces_ambiguous_pcurve_loss() {
        let scan = ContainerScan {
            source_image: &[],
            version: 0,
            blocks: Vec::new(),
            directory: Vec::new(),
            cache_cells: Vec::new(),
            compound_streams: Vec::new(),
        };
        let mut decoded = Brep::default();
        decoded.stats.ambiguous_pcurve_parameters = 2;

        let report = super::build_geometry_report(&scan, &decoded);
        assert!(report.losses.iter().any(|loss| {
            loss.code == cadmpeg_ir::report::LossKind::PcurveOmitted
                && loss.message.contains("2 pcurve(s)")
        }));
    }
}
