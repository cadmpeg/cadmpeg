// SPDX-License-Identifier: Apache-2.0
//! Configuration-lane enrichment and design-state projection.

#[cfg(test)]
mod admission_tests;

#[cfg(test)]
mod lane_tests;

use crate::records::FeatureHistory;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::{
    features::{
        ConfigurationEvaluation, DatumPlaneReference, DesignConfiguration, FaceSelection,
        FeatureDefinition, FeatureId, FeatureOperation, LinearTermination, ParameterValue,
    },
    scalar::{Angle, Length},
};
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::history::bind::bind_unique_sketch_feature;
use crate::history::literals::valid_plane_frame;
use crate::history::parameters::eval::exact_integer_f64;
use crate::history::parameters::{apply_evaluated_parameters, project_parameters};
use crate::history::project::project_features;

const EPS_CONFIGURATION_ALIGN_CONFIGURATION_PARAMETER_KINDS_E9: f64 = 1.0e-9;

/// Base feature definitions by identity: scratch held by its reservation.
struct ConfigurationDefinitions<'features, 'ctx> {
    definitions: HashMap<&'features FeatureId, &'features FeatureDefinition>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'features, 'ctx> ConfigurationDefinitions<'features, 'ctx> {
    fn new(
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
        features: &'features [cadmpeg_ir::features::Feature],
    ) -> Result<Self, cadmpeg_core::CodecError> {
        const OPERATION: &str = "index SLDPRT configuration base definitions";
        let mut storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut definitions = HashMap::new();
        for feature in ctx.admit_iter(features, "scan SLDPRT new values")? {
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut definitions,
                    &feature.id,
                    feature.evaluation.definition(),
                    OPERATION,
                )
            })?;
        }
        Ok(Self {
            definitions,
            _storage: storage,
        })
    }

    fn get(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        id: &FeatureId,
    ) -> Result<Option<&'features FeatureDefinition>, cadmpeg_core::CodecError> {
        Ok(ctx
            .get_hash_map(&self.definitions, id, "look up SLDPRT hash key")?
            .copied())
    }
}

fn apply_configuration_state(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature: &mut cadmpeg_ir::features::Feature,
    state: &cadmpeg_ir::features::ConfigurationFeatureState,
) -> Result<(), cadmpeg_core::CodecError> {
    let state = state.try_clone_for_decode(ctx, "retain SLDPRT configuration feature state")?;
    let (outputs, suppressed) = match state.evaluation {
        ConfigurationEvaluation::Suppressed {} => {
            (cadmpeg_ir::features::DistinctMembers::default(), true)
        }
        ConfigurationEvaluation::Active { outputs } => (outputs, false),
    };
    feature.suppressed = Some(suppressed);
    feature.dependencies = state.dependencies;
    feature.evaluation = cadmpeg_ir::features::FeatureEvaluation::new(state.definition, outputs);
    Ok(())
}

fn copy_configuration_features(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
) -> Result<Vec<cadmpeg_ir::features::Feature>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "retain SLDPRT configuration features";
    let mut copied = Vec::new();
    ctx.reserve_capacity(&mut copied, features.len(), OPERATION)?;
    for feature in ctx.admit_iter(features, "scan SLDPRT copy_configuration_features values")? {
        ctx.push_vec(
            &mut (copied),
            feature.try_clone_for_decode(ctx, OPERATION)?,
            OPERATION,
        )?;
    }
    Ok(copied)
}

fn copy_configuration_state_features(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    states: &BTreeMap<FeatureId, cadmpeg_ir::features::ConfigurationFeatureState>,
) -> Result<Vec<cadmpeg_ir::features::Feature>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "retain SLDPRT evaluated configuration features";
    let mut copied = Vec::new();
    for feature in ctx.admit_iter(
        features,
        "scan SLDPRT copy_configuration_state_features values",
    )? {
        let Some(state) =
            ctx.get_btree_map(states, &feature.id, "look up SLDPRT ordered key")?
        else {
            continue;
        };
        ctx.reserve_vec(&mut copied, 1, OPERATION)?;
        let mut feature = feature.try_clone_for_decode(ctx, OPERATION)?;
        apply_configuration_state(ctx, &mut feature, state)?;
        copied.push(feature);
    }
    Ok(copied)
}

fn configuration_feature_state(
    feature: cadmpeg_ir::features::Feature,
) -> (FeatureId, cadmpeg_ir::features::ConfigurationFeatureState) {
    let (definition, outputs) = feature.evaluation.into_parts();
    let evaluation = if feature.suppressed.unwrap_or(false) {
        ConfigurationEvaluation::Suppressed {}
    } else {
        ConfigurationEvaluation::Active { outputs }
    };
    (
        feature.id,
        cadmpeg_ir::features::ConfigurationFeatureState {
            evaluation,
            dependencies: feature.dependencies,
            definition,
        },
    )
}

/// Which side of the codec drives the history-enrichment prefix.
///
/// The read (decode) path and the write-side reprojections run the same ordered
/// choreography of native-lane enrichments, with one direction-specific step:
/// the read path applies hole-construction enrichment, and the write and
/// configuration-reprojection paths omit it. Any other divergence between the
/// directions lives in the callers, around the shared calls below, not inside
/// this prefix.
#[derive(Clone, Copy)]
pub(crate) enum HistoryEnrichment {
    /// Decode path: includes `enrich_history_hole_constructions`.
    Read,
    /// Write path and configuration reprojection: omits hole constructions.
    Write,
}

/// Semantic-projection mode of `resolved_features::parameters::enrich_history_parameters`
/// (the historical `true` argument): projects parameters together with their
/// downstream semantic feature inputs.
pub(crate) fn enrich_history_parameters_semantic(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    histories: &mut [FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    crate::resolved_features::parameters::enrich_history_parameters(ctx, histories, lanes, true)
}

/// Parameter-only mode of `resolved_features::parameters::enrich_history_parameters` (the
/// historical `false` argument): projects parameter values without the semantic
/// feature-input projection.
pub(crate) fn enrich_history_parameters_values_only(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    histories: &mut [FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    crate::resolved_features::parameters::enrich_history_parameters(ctx, histories, lanes, false)
}

/// The shared native-lane enrichment prefix, declared once for both codec
/// directions. Runs the ordered extrusion-termination, combine, sweep-path,
/// sketch-block, split-line, parameter, reference-plane, reference-point,
/// coordinate-system, PMI, evaluated-parameter, reference-axis, and
/// revolution-input enrichments; the read path additionally applies
/// hole-construction enrichment (selected by `mode`).
pub(crate) fn enrich_history_semantic(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    histories: &mut [FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
    pmi_dimensions: &[crate::records::PmiDimension],
    mode: HistoryEnrichment,
) -> Result<(), cadmpeg_core::CodecError> {
    crate::resolved_features::terminations::enrich_history_extrusion_terminations(
        ctx, histories, lanes,
    )?;
    crate::resolved_features::terminations::enrich_history_combine_selections(
        ctx, histories, lanes,
    )?;
    crate::resolved_features::terminations::enrich_history_sweep_paths(ctx, histories, lanes)?;
    crate::resolved_features::reference_geometry::enrich_history_sketch_block_references(
        ctx, histories, lanes,
    )?;
    crate::resolved_features::operations::enrich_history_split_lines(ctx, histories, lanes)?;
    crate::resolved_features::direct_edits::enrich_history_move_face_translations(
        ctx, histories, lanes,
    )?;
    crate::resolved_features::direct_edits::enrich_history_move_body_translations(
        ctx, histories, lanes,
    )?;
    enrich_history_parameters_semantic(ctx, histories, lanes)?;
    if matches!(mode, HistoryEnrichment::Read) {
        crate::resolved_features::holes::enrich_history_hole_constructions(ctx, histories, lanes)?;
        crate::resolved_features::holes::enrich_history_cosmetic_thread_diameters(
            ctx, histories, lanes,
        )?;
    } else {
        crate::resolved_features::holes::
            enrich_history_cosmetic_thread_diameters_without_hole_constructions(ctx, histories, lanes)?;
    }
    crate::resolved_features::reference_geometry::enrich_history_reference_planes(
        ctx, histories, lanes,
    )?;
    crate::resolved_features::reference_geometry::enrich_history_reference_points(
        ctx, histories, lanes,
    )?;
    crate::resolved_features::reference_geometry::enrich_history_coordinate_systems(
        ctx, histories, lanes,
    )?;
    crate::pmi::enrich_history_parameters(ctx, histories, pmi_dimensions)?;
    apply_evaluated_parameters(ctx, histories)?;
    crate::resolved_features::reference_geometry::enrich_history_reference_axes(
        ctx, histories, lanes,
    )?;
    crate::resolved_features::axes::enrich_history_revolution_inputs(ctx, histories, lanes)?;
    Ok(())
}

/// The shared compact/generated projection block, declared once for both codec
/// directions. Applies the seven ordered projections that read, write, and
/// configuration reprojection all run against a freshly projected feature list.
/// Direction-specific operation and profile bindings (pattern inputs,
/// sweep/revolution/extrusion operations, spatial sketches, tree-node restore)
/// stay in each caller, around this block.
pub(crate) fn project_compact_and_generated(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    projection: &[FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    crate::resolved_features::projections::project_compact_body_selections(ctx, features, lanes)?;
    crate::resolved_features::terminations::project_compact_combine_paths(
        ctx, features, projection, lanes,
    )?;
    crate::resolved_features::projections::project_compact_edge_selections(
        ctx, features, projection, lanes,
    )?;
    crate::resolved_features::projections::project_compact_surface_selections(
        ctx, features, projection, lanes,
    )?;
    crate::resolved_features::projections::project_draft_operands(
        ctx, features, projection, lanes,
    )?;
    crate::resolved_features::terminations::project_surface_sweep_profiles(
        ctx, features, projection, lanes,
    )?;
    crate::resolved_features::holes::project_helix_axes(ctx, features, projection, lanes)?;
    crate::resolved_features::component_paths::project_adjacent_extrusion_profiles(
        ctx, features, projection, lanes,
    )?;

    Ok(())
}

/// Reproject configuration-local evaluated parameters and feature operations from native lanes.
pub(crate) fn project_configuration_design_states(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut cadmpeg_ir::CadIr,
    histories: &[FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
    pmi_dimensions: &[crate::records::PmiDimension],
    form_padding: Option<usize>,
) -> Result<(), cadmpeg_core::CodecError> {
    let (resolved_base_features, _base_storage) = ctx.with_scoped_storage("SLDPRT configuration base features", || {
    let mut resolved_base_features = copy_configuration_features(ctx, &ir.model.features)?;
    crate::resolved_features::operations::bind_extrusion_operations(
        ctx,
        &mut resolved_base_features,
        histories,
        lanes,
        form_padding,
    )?;
    crate::resolved_features::operations::bind_revolution_operations(
        ctx,
        &mut resolved_base_features,
        histories,
        lanes,
        form_padding,
    )?;
    crate::resolved_features::operations::bind_sweep_operations(
        ctx,
        &mut resolved_base_features,
        histories,
        lanes,
        form_padding,
    )?;
        Ok::<_, cadmpeg_core::CodecError>(resolved_base_features)
    })?;
    let base_definitions = ConfigurationDefinitions::new(ctx, &ir.model.features)?;
    for configuration in ctx.admit_iter(
        &mut ir.model.configurations,
        "scan SLDPRT configuration lane assignments",
    )? {
        configuration.parameter_values.clear();
        configuration.feature_states.clear();
    }
    let (lane_assignments, _lane_assignments_storage) = ctx.with_scoped_storage("SLDPRT configuration lane assignments", || configuration_lane_assignments(ctx, &ir.model.configurations, lanes))?;
    for (configuration_index, lane_index) in ctx
        .admit_iter(
            &lane_assignments,
            "scan SLDPRT configuration lane assignments",
        )?
        .copied()
    {
        let scoped_lanes = &lanes[lane_index..=lane_index];
        let parameter_values = {
        let (projection, _parameter_projection_storage) = ctx.with_scoped_storage("SLDPRT configuration parameter histories", || {
        let mut projection = crate::records::charged_clone::clone_histories_charged(
            ctx,
            histories,
            "clone SLDPRT configuration parameter histories",
        )?;
        // Seed PMI types before lane enrichment so dimension semantics reject
        // incompatible native scalar candidates. Reapply afterward to add PMI
        // parameters that the lane does not carry without replacing overrides.
        crate::pmi::enrich_history_parameters_with_features(
            ctx,
            &mut projection,
            pmi_dimensions,
            &ir.model.features,
        )?;
        enrich_history_parameters_semantic(ctx, &mut projection, scoped_lanes)?;
        crate::resolved_features::holes::
            enrich_history_cosmetic_thread_diameters_without_hole_constructions(
                ctx,
                &mut projection,
                scoped_lanes,
            )?;
        crate::pmi::enrich_history_parameters_with_features(
            ctx,
            &mut projection,
            pmi_dimensions,
            &ir.model.features,
        )?;
            Ok::<_, cadmpeg_core::CodecError>(projection)
        })?;
        let mut parameter_values = BTreeMap::new();
        for parameter in ctx.admit_iter(
            project_parameters(ctx, &projection)?,
            "collect SLDPRT configuration values",
        )? {
            let Some(value) = parameter.value else {
                continue;
            };
            ctx.insert_btree_map(
                &mut parameter_values,
                parameter.id,
                value,
                "collect SLDPRT configuration values",
            )?;
        }
        parameter_values
        };
        ir.model.configurations[configuration_index].parameter_values = parameter_values;

        let (projection, _feature_projection_storage) = ctx.with_scoped_storage("SLDPRT configuration feature histories", || {
        let mut projection = crate::records::charged_clone::clone_histories_charged(
            ctx,
            histories,
            "clone SLDPRT configuration feature histories",
        )?;
        enrich_history_semantic(
            ctx,
            &mut projection,
            scoped_lanes,
            pmi_dimensions,
            HistoryEnrichment::Write,
        )?;
            Ok::<_, cadmpeg_core::CodecError>(projection)
        })?;
        let mut features = project_features(ctx, &projection)?;
        crate::resolved_features::bindings::bind_pattern_inputs(
            ctx,
            &mut features,
            &projection,
            scoped_lanes,
        )?;
        project_compact_and_generated(ctx, &mut features, &projection, scoped_lanes)?;
        crate::resolved_features::operations::bind_extrusion_operations(
            ctx,
            &mut features,
            histories,
            scoped_lanes,
            form_padding,
        )?;
        crate::resolved_features::operations::bind_revolution_operations(
            ctx,
            &mut features,
            histories,
            scoped_lanes,
            form_padding,
        )?;
        crate::resolved_features::operations::bind_sweep_operations(
            ctx,
            &mut features,
            histories,
            scoped_lanes,
            form_padding,
        )?;
        crate::resolved_features::operations::inherit_configuration_operations(
            ctx,
            &mut features,
            &resolved_base_features,
            histories,
            scoped_lanes,
            form_padding,
        )?;
        inherit_configuration_reference_plane_semantics(
            ctx,
            &mut features,
            &resolved_base_features,
        )?;
        crate::resolved_features::bindings::bind_sweep_adjacent_profiles(
            ctx,
            &mut features,
            histories,
            scoped_lanes,
        )?;
        restore_configuration_tree_node_definitions(ctx, &mut features, &ir.model.features)?;
        let mut feature_states = BTreeMap::new();
        for mut feature in ctx.admit_iter(features, "collect SLDPRT configuration values")? {
            if let Some(base_definition) = base_definitions.get(ctx, &feature.id)? {
                if matches!(
                    feature.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Hole { .. })
                ) {
                    // A scoped lane may author positions without repeating
                    // shared hole construction. Copy missing construction
                    // fields while preserving authored local placements.
                    let inherit_placements =
                        !crate::resolved_features::holes::hole_position_carrier_present(
                            ctx,
                            &feature,
                            histories,
                            scoped_lanes,
                        )?;
                    let mut result = Ok(());
                    feature.evaluation.edit(|definition, _| {
                        result = inherit_configuration_hole_semantics(
                            ctx,
                            definition,
                            base_definition,
                            inherit_placements,
                        );
                    });
                    result?;
                }
            }
            let (id, state) = configuration_feature_state(feature);
            ctx.insert_btree_map(
                &mut feature_states,
                id,
                state,
                "collect SLDPRT configuration values",
            )?;
        }
        ir.model.configurations[configuration_index].feature_states = feature_states;
    }

    Ok(())
}

/// Project edge operands carried only by supplemental config-object lanes into
/// the matching configuration-local feature snapshots.
pub(crate) fn project_configuration_supplemental_edge_selections(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut cadmpeg_ir::CadIr,
    lanes: &[crate::records::FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    let slots = ConfigurationSlots::new(ctx, &ir.model.configurations)?;
    for lane in ctx
        .admit_iter(lanes, "scan SLDPRT supplemental configuration lanes")?
        .filter(|lane| crate::resolved_features::assembly::is_supplemental_config_lane(lane))
    {
        let Some(slot_index) = lane_slot(ctx, lane)? else {
            continue;
        };
        let Some(configuration_index) = slots.configuration(ctx, slot_index)? else {
            continue;
        };
        let states = &ir.model.configurations[configuration_index].feature_states;
        let mut features = copy_configuration_features(ctx, &ir.model.features)?;
        for feature in ctx.admit_iter(
            &mut features,
            "scan SLDPRT supplemental configuration lanes",
        )? {
            let Some(state) =
                ctx.get_btree_map(states, &feature.id, "look up SLDPRT ordered key")?
            else {
                continue;
            };
            apply_configuration_state(ctx, feature, state)?;
        }
        crate::resolved_features::projections::project_compact_edge_selections(
            ctx,
            &mut features,
            &[],
            std::slice::from_ref(lane),
        )?;
        let states = &mut ir.model.configurations[configuration_index].feature_states;
        for feature in ctx.admit_iter(features, "scan SLDPRT supplemental configuration lanes")? {
            let Some(state) = ctx.get_mut_btree_map(
                &mut *states,
                &feature.id,
                "look up mutable SLDPRT ordered key",
            )?
            else {
                continue;
            };
            state.dependencies = feature.dependencies;
            state.definition = feature.evaluation.into_parts().0;
        }
    }

    Ok(())
}

/// Resolve topology operands in configuration-local feature snapshots.
pub(crate) fn bind_configuration_topology_selections(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut cadmpeg_ir::CadIr,
    histories: &[FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
    face_identities: &[(cadmpeg_ir::ids::FaceId, crate::brep::PersistentFaceIdentity)],
) -> Result<(), cadmpeg_core::CodecError> {
    let (lane_assignments, _lane_assignments_storage) = ctx.with_scoped_storage("SLDPRT configuration lane assignments", || configuration_lane_assignments(ctx, &ir.model.configurations, lanes))?;
    for (configuration_index, lane_index) in ctx
        .admit_iter(
            &lane_assignments,
            "scan SLDPRT configuration lane assignments",
        )?
        .copied()
    {
        let body_membership_resolved = ir.model.configurations[configuration_index]
            .bodies
            .is_some();
        let scoped_lanes = &lanes[lane_index..=lane_index];
        let states = &ir.model.configurations[configuration_index].feature_states;
        let mut features = copy_configuration_state_features(ctx, &ir.model.features, states)?;
        if body_membership_resolved {
            let topology_selection_inputs = crate::history::selections::TopologySelectionInputs {
                bodies: &ir.model.bodies,
                faces: &ir.model.faces,
                surfaces: &ir.model.surfaces,
                edges: &ir.model.edges,
                curves: &ir.model.curves,
                lanes: scoped_lanes,
                face_identities,
            };
            crate::history::selections::bind_topology_selections(
                ctx,
                &mut features,
                histories,
                &topology_selection_inputs,
            )?;
        }
        // A legacy offset-plane alias carries a complete support frame. That
        // frame can bind a unique planar face even when the configuration has
        // no independently established body membership.
        crate::resolved_features::projections::project_unbound_offset_plane_faces(
            ctx,
            &mut features,
            &ir.model.faces,
            &ir.model.surfaces,
        )?;
        let states = &mut ir.model.configurations[configuration_index].feature_states;
        for feature in ctx.admit_iter(features, "scan SLDPRT configuration lane assignments")? {
            let Some(state) = ctx.get_mut_btree_map(
                &mut *states,
                &feature.id,
                "look up mutable SLDPRT ordered key",
            )?
            else {
                continue;
            };
            *state = configuration_feature_state(feature).1;
        }
    }

    Ok(())
}

pub(super) fn restore_configuration_tree_node_definitions(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    base_features: &[cadmpeg_ir::features::Feature],
) -> Result<(), cadmpeg_core::CodecError> {
    let base = ConfigurationDefinitions::new(ctx, base_features)?;
    for feature in ctx.admit_iter(features, "restore SLDPRT configuration tree nodes")? {
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ) {
            continue;
        }
        let Some(FeatureDefinition::Operation(FeatureOperation::TreeNode { role, .. })) =
            base.get(ctx, &feature.id)?
        else {
            continue;
        };
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::TreeNode {
                role: *role,
                children: cadmpeg_ir::features::TreeChildren::default(),
            }));
    }
    Ok(())
}

/// Apply sketch ownership projection to configuration-local feature snapshots.
pub(crate) fn project_configuration_sketch_states(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut cadmpeg_ir::CadIr,
    histories: &[FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
    annotations: &mut cadmpeg_ir::Annotations,
) -> Result<Vec<cadmpeg_ir::report::loss::LossNote>, cadmpeg_core::CodecError> {
    let mut losses = Vec::new();
    let (lane_assignments, _lane_assignments_storage) = ctx.with_scoped_storage("SLDPRT configuration lane assignments", || configuration_lane_assignments(ctx, &ir.model.configurations, lanes))?;
    for (configuration_index, lane_index) in ctx
        .admit_iter(
            &lane_assignments,
            "scan SLDPRT configuration lane assignments",
        )?
        .copied()
    {
        let (surfaces, _surface_storage) = ctx.with_scoped_storage("SLDPRT configuration surface carriers", || configuration_surface_carriers(ctx, ir, configuration_index))?;
        let scoped_lanes = &lanes[lane_index..=lane_index];
        let states = &ir.model.configurations[configuration_index].feature_states;
        let mut features = copy_configuration_state_features(ctx, &ir.model.features, states)?;
        inherit_configuration_reference_plane_semantics(ctx, &mut features, &ir.model.features)?;
        let mut spatial_sketches_storage = ctx.reserve_scoped(
            0,
            "SLDPRT temporary configuration spatial sketch identities",
        )?;
        let mut reusable_spatial_sketches = HashMap::new();
        for sketch in ctx.admit_iter(
            &ir.model.spatial_sketches,
            "scan SLDPRT project_configuration_sketch_states values",
        )? {
            const OPERATION: &str = "match SLDPRT configuration spatial sketch scope";
            let lane = &scoped_lanes[0];
            if sketch.configuration.is_none()
                || match sketch.native_ref.as_deref() {
                    Some(native) => ctx.equal(native, lane.id.as_str(), OPERATION)?,
                    None => false,
                }
                || match (
                    lane.configuration.as_deref(),
                    sketch.configuration.as_deref(),
                ) {
                    (Some(lane_configuration), Some(sketch_configuration)) => {
                        ctx.equal(sketch_configuration, lane_configuration, OPERATION)?
                    }
                    _ => false,
                }
            {
                spatial_sketches_storage.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut reusable_spatial_sketches,
                        sketch.id.as_str(),
                        &sketch.id,
                        "index SLDPRT configuration spatial sketches",
                    )
                })?;
            }
        }
        let base_definitions = ConfigurationDefinitions::new(ctx, &ir.model.features)?;
        for feature in ctx.admit_iter(&mut features, "scan SLDPRT configuration sketch features")? {
            if let FeatureDefinition::Operation(FeatureOperation::SpatialSketch { sketch }) =
                feature.evaluation.definition()
            {
                if sketch.is_some() { continue; }
                const OPERATION: &str = "match SLDPRT configuration spatial sketch identity";
                let id = feature.id.as_str();
                let (text, _text_storage) = if let Some((prefix, suffix)) = ctx.split_once(id, ":model:feature#", OPERATION)? {
                    let (text, storage) = ctx.format_scoped(format_args!("{prefix}:model:spatial-sketch#{suffix}"), OPERATION)?;
                    (std::borrow::Cow::Owned(text), storage)
                } else {
                    (std::borrow::Cow::Borrowed(id), ctx.reserve_scoped(0, OPERATION)?)
                };
                if let Some(existing) = ctx.get_hash_map(&reusable_spatial_sketches, text.as_ref(), "match SLDPRT configuration spatial sketch")? {
                    let copied = existing.try_clone_for_decode(ctx, "copy SLDPRT configuration spatial sketch identity")?;
                    feature.evaluation.set_definition(FeatureDefinition::Operation(FeatureOperation::SpatialSketch { sketch: Some(copied) }));
                }
                continue;
            }
            let FeatureDefinition::Operation(FeatureOperation::Sketch { sketch }) =
                feature.evaluation.definition()
            else {
                continue;
            };
            let Some(FeatureDefinition::Operation(FeatureOperation::SpatialSketch {
                sketch: Some(base_sketch),
            })) = base_definitions.get(ctx, &feature.id)?
            else {
                continue;
            };
            if sketch.id().is_none()
                && ctx.contains_key_hash_map(
                    &reusable_spatial_sketches,
                    base_sketch.as_str(),
                    "match SLDPRT configuration spatial sketch",
                )?
            {
                let copied = base_sketch.try_clone_for_decode(
                    ctx,
                    "copy SLDPRT configuration spatial sketch identity",
                )?;
                feature
                    .evaluation
                    .set_definition(FeatureDefinition::Operation(
                        FeatureOperation::SpatialSketch {
                            sketch: Some(copied),
                        },
                    ));
            }
        }
        let mut parameters = std::mem::take(&mut ir.model.parameters);
        let mut saved_storage =
            ctx.reserve_scoped(0, "overlay SLDPRT configuration parameter values")?;
        let mut saved_values = Vec::new();
        let result = (|| -> Result<(), cadmpeg_core::CodecError> {
            const OPERATION: &str = "overlay SLDPRT configuration parameter values";
            let values = &ir.model.configurations[configuration_index].parameter_values;
            for (index, parameter) in ctx.admit_iter(&mut parameters, OPERATION)?.enumerate() {
                let Some(value) =
                    ctx.get_btree_map(values, &parameter.id, "look up SLDPRT ordered key")?
                else {
                    continue;
                };
                let copied = saved_storage.with_storage(|| value.try_clone_for_decode(ctx, OPERATION))?;
                saved_storage.with_storage(|| ctx.reserve_vec(&mut saved_values, 1, OPERATION))?;
                ctx.charge_work(1, "restore SLDPRT configuration parameter values")?;
                saved_values.push((index, parameter.value.replace(copied)));
            }
            crate::resolved_features::profiles::bind_sketch_profiles(
                ctx,
                &mut features,
                crate::resolved_features::profiles::SketchArenas {
                    sketches: &mut ir.model.sketches,
                    sketch_entities: &mut ir.model.sketch_entities,
                    sketch_constraints: &mut ir.model.sketch_constraints,
                    annotations,
                },
                &parameters,
                histories,
                scoped_lanes,
            )?;
            crate::resolved_features::profiles::project_compact_sketch_profiles(
                ctx,
                &mut features,
                &mut ir.model.sketches,
                &mut ir.model.sketch_entities,
                histories,
                scoped_lanes,
                &mut losses,
            )?;
            crate::resolved_features::profiles::project_marker_backed_sketches(
                ctx,
                &mut features,
                &mut ir.model.sketches,
                &mut ir.model.sketch_entities,
                histories,
                scoped_lanes,
            )?;
            crate::resolved_features::profiles::project_sketch_block_profiles(
                ctx,
                &mut features,
                &mut ir.model.sketches,
                &mut ir.model.sketch_entities,
                histories,
                scoped_lanes,
            )?;
            bind_unique_sketch_feature(ctx, &mut features, &ir.model.sketches, histories)?;
            crate::resolved_features::component_paths::project_dissected_sketches(
                ctx,
                &mut features,
                &ir.model.sketches,
                histories,
            )?;
            crate::resolved_features::axes::bind_profile_revolution_axes(
                ctx,
                &mut features,
                histories,
                scoped_lanes,
                &ir.model.sketches,
                &surfaces,
            )?;
            crate::resolved_features::bindings::bind_pattern_inputs(
                ctx,
                &mut features,
                histories,
                scoped_lanes,
            )?;
            crate::resolved_features::component_paths::project_adjacent_extrusion_profiles(
                ctx,
                &mut features,
                histories,
                scoped_lanes,
            )?;
            crate::resolved_features::bindings::bind_sweep_adjacent_profiles(
                ctx,
                &mut features,
                histories,
                scoped_lanes,
            )?;
            crate::resolved_features::dimensions::project_dimensioned_sketch_geometry(
                ctx,
                &mut ir.model.sketch_entities,
                &ir.model.sketches,
                &surfaces,
                &features,
                &parameters,
                scoped_lanes,
            )?;
            crate::resolved_features::dimensions::project_marker_dimensioned_circles(
                ctx,
                &mut ir.model.sketch_entities,
                &mut ir.model.sketches,
                &features,
                &parameters,
                scoped_lanes,
            )?;
            crate::resolved_features::relation_geometry::project_relation_point_geometry(
                ctx,
                &mut ir.model.sketch_entities,
                &ir.model.sketches,
                &features,
                scoped_lanes,
            )?;
            crate::resolved_features::dimensions::project_relation_point_dimensioned_circles(
                ctx,
                &mut ir.model.sketch_entities,
                &features,
                &parameters,
                scoped_lanes,
            )?;
            crate::resolved_features::relation_geometry::project_relation_solved_line_geometry(
                ctx,
                &mut ir.model.sketch_entities,
                &ir.model.sketches,
                &features,
                &parameters,
                scoped_lanes,
            )?;
            crate::resolved_features::relation_geometry::project_relation_solved_point_geometry(
                ctx,
                &mut ir.model.sketch_entities,
                &ir.model.sketches,
                &features,
                &parameters,
                scoped_lanes,
            )?;
            crate::resolved_features::relation_geometry::project_relation_bindings(
                ctx,
                &mut ir.model.sketch_constraints,
                &ir.model.sketches,
                &features,
                &ir.model.sketch_entities,
                &parameters,
                scoped_lanes,
            )?;
            crate::resolved_features::holes::project_profiled_hole_constructions(
                ctx,
                &mut features,
                &ir.model.sketch_entities,
                histories,
                scoped_lanes,
            )?;
            crate::resolved_features::holes::project_hole_position_sketches(
                ctx,
                &mut features,
                &ir.model.sketches,
                &ir.model.sketch_entities,
                histories,
                scoped_lanes,
            )?;
            crate::resolved_features::holes::project_spatial_hole_position_sketches(
                ctx,
                &mut features,
                &ir.model.spatial_sketches,
                &ir.model.spatial_sketch_entities,
                &surfaces,
                histories,
                scoped_lanes,
            )?;
            crate::resolved_features::holes::project_topological_hole_constructions(
                ctx,
                &mut features,
                &crate::resolved_features::holes::HoleTopology {
                    surfaces: &surfaces,
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
                &mut features,
                &ir.model.sketch_entities,
                &crate::resolved_features::holes::HoleTopology {
                    surfaces: &surfaces,
                    faces: &ir.model.faces,
                    loops: &ir.model.loops,
                    coedges: &ir.model.coedges,
                    edges: &ir.model.edges,
                    vertices: &ir.model.vertices,
                    points: &ir.model.points,
                },
                histories,
                scoped_lanes,
            )?;
            crate::resolved_features::relation_geometry::project_relation_bindings(
                ctx,
                &mut ir.model.sketch_constraints,
                &ir.model.sketches,
                &features,
                &ir.model.sketch_entities,
                &parameters,
                scoped_lanes,
            )?;
            for feature in ctx.admit_iter(features, "scan SLDPRT configuration sketch features")? {
                let Some(state) = ctx.get_mut_btree_map(
                    &mut (ir.model.configurations[configuration_index].feature_states),
                    &feature.id,
                    "look up mutable SLDPRT ordered key",
                )?
                else {
                    continue;
                };
                *state = configuration_feature_state(feature).1;
            }
            Ok(())
        })();
        // Restoring cannot refuse: each saved value was admitted when it was saved.
        for (index, value) in saved_values {
            parameters[index].value = value;
        }
        ir.model.parameters = parameters;
        result?;
    }
    let base = ConfigurationDefinitions::new(ctx, &ir.model.features)?;
    let (mut scoped, _scoped_storage) = ctx.with_scoped_storage(
        "scan SLDPRT project_configuration_sketch_states values",
        || {
            ctx.alloc_filled(
                ir.model.configurations.len(),
                false,
                "scan SLDPRT project_configuration_sketch_states values",
            )
        },
    )?;
    for (assigned, _) in ctx.admit_iter(
        &lane_assignments,
        "scan SLDPRT project_configuration_sketch_states values",
    )? {
        scoped[*assigned] = true;
    }
    for (configuration_index, configuration) in ctx
        .admit_iter(
            &mut ir.model.configurations,
            "scan SLDPRT project_configuration_sketch_states values",
        )?
        .enumerate()
    {
        // DI-55: a valid configuration lane owns its unresolved slots. The
        // document definition is a fallback only for an unscoped snapshot.
        if scoped[configuration_index] {
            continue;
        }
        for (feature_id, state) in ctx.admit_iter(
            &mut configuration.feature_states,
            "scan SLDPRT project_configuration_sketch_states values",
        )? {
            if let Some(base_definition) = base.get(ctx, feature_id)? {
                inherit_configuration_shared_semantics(
                    ctx,
                    &mut state.definition,
                    base_definition,
                )?;
                if let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                    reference: Some(DatumPlaneReference::Feature { feature: reference }),
                    ..
                }) = &state.definition
                {
                    insert_configuration_dependency(ctx, &mut state.dependencies, reference)?;
                }
            }
        }
    }

    Ok(losses)
}

fn inherit_configuration_shared_semantics(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &mut FeatureDefinition,
    base_definition: &FeatureDefinition,
) -> Result<(), cadmpeg_core::CodecError> {
    if let (
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane { reference, .. }),
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: base_reference,
            ..
        }),
    ) = (&mut *definition, base_definition)
    {
        if reference.is_none() {
            *reference = base_reference
                .as_ref()
                .map(|reference| copy_configuration_plane_reference(ctx, reference))
                .transpose()?;
        } else if let (
            Some(cadmpeg_ir::features::DatumPlaneReference::Face { face }),
            Some(cadmpeg_ir::features::DatumPlaneReference::Face { face: base_face }),
        ) = (reference, base_reference)
        {
            let incomplete = match face {
                cadmpeg_ir::features::FaceSelection::Faces(faces)
                | cadmpeg_ir::features::FaceSelection::Resolved { faces, .. } => faces.is_empty(),
                cadmpeg_ir::features::FaceSelection::Historical { .. } => false,
                cadmpeg_ir::features::FaceSelection::Generated { .. } => false,
                cadmpeg_ir::features::FaceSelection::HistoricalPartial { .. } => true,
                cadmpeg_ir::features::FaceSelection::Unresolved
                | cadmpeg_ir::features::FaceSelection::Native(_) => true,
            };
            if incomplete {
                *face =
                    base_face.try_clone_for_decode(ctx, "copy SLDPRT configuration datum face")?;
            }
        }
        return Ok(());
    }
    inherit_configuration_hole_semantics(ctx, definition, base_definition, true)?;

    Ok(())
}

fn inherit_configuration_hole_semantics(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &mut FeatureDefinition,
    base_definition: &FeatureDefinition,
    inherit_placements: bool,
) -> Result<(), cadmpeg_core::CodecError> {
    const OPERATION: &str = "copy SLDPRT configuration hole construction";

    let FeatureDefinition::Operation(FeatureOperation::Hole {
        profile,
        profile_filter,
        face,
        direction: _,
        placements,
        shape,

        extent,
        bottom,
        taper_angle,
        allow_multi_profile_faces,
    }) = definition
    else {
        return Ok(());
    };
    let FeatureDefinition::Operation(FeatureOperation::Hole {
        profile: base_profile,
        profile_filter: base_profile_filter,
        face: base_face,
        direction: _,
        placements: base_placements,
        shape: base_shape,

        extent: base_extent,
        bottom: base_bottom,
        taper_angle: base_taper_angle,
        allow_multi_profile_faces: base_allow_multi_profile_faces,
    }) = base_definition
    else {
        return Ok(());
    };
    let mut construction = shape.construction().try_clone_for_decode(ctx, OPERATION)?;
    let mut exit_kind = *shape.exit_kind();
    let mut diameter = shape.diameter();
    let base_construction = base_shape.construction();
    let base_exit_kind = base_shape.exit_kind();
    let base_diameter = &base_shape.diameter();
    let missing_construction = diameter.is_none() && extent.is_none();
    let missing_face = face
        .as_ref()
        .is_none_or(|face| !complete_configuration_face_selection(face));
    if missing_face {
        *face = base_face
            .as_ref()
            .map(|value| value.try_clone_for_decode(ctx, "copy SLDPRT configuration hole face"))
            .transpose()?;
    }
    if profile.is_none() {
        *profile = base_profile
            .as_ref()
            .map(|value| value.try_clone_for_decode(ctx, "copy SLDPRT configuration hole profile"))
            .transpose()?;
    }
    if profile_filter.is_none() {
        profile_filter.clone_from(base_profile_filter);
    }
    if inherit_placements && placements.is_none() {
        if let Some(base_placements) = base_placements {
            const OPERATION: &str = "copy SLDPRT configuration hole placements";
            let copied =
                ctx.try_collect_retained_with(base_placements, OPERATION, |placement| {
                    placement.try_clone_for_decode(ctx, OPERATION)
                })?;
            *placements = Some(copied);
        }
    }
    match (&mut construction, base_construction) {
        (
            cadmpeg_ir::features::holes::HoleConstruction::Form {
                kind,
                specification,
            },
            cadmpeg_ir::features::holes::HoleConstruction::Form {
                kind: base_kind,
                specification: base_specification,
            },
        ) => {
            if missing_construction || kind.is_unresolved() {
                kind.clone_from(base_kind);
            }
            if specification.is_none() {
                *specification = base_specification
                    .as_deref()
                    .map(|value| value.try_clone_for_decode(ctx, OPERATION))
                    .transpose()?;
            }
        }
        (construction, base_construction)
            if missing_construction
                || matches!(
                    &*construction,
                    cadmpeg_ir::features::holes::HoleConstruction::Form { kind, .. }
                        if kind.is_unresolved()
                ) =>
        {
            *construction = base_construction.try_clone_for_decode(ctx, OPERATION)?;
        }
        _ => {}
    }
    if exit_kind.is_none_or(|kind| kind.is_unresolved()) {
        exit_kind.clone_from(base_exit_kind);
    }
    if diameter.is_none() {
        diameter.clone_from(base_diameter);
    }
    if extent
        .as_ref()
        .is_none_or(|extent| matches!(extent, LinearTermination::Unresolved {}))
    {
        *extent = base_extent
            .as_ref()
            .map(|value| {
                value.try_clone_for_decode(ctx, "copy SLDPRT configuration hole termination")
            })
            .transpose()?;
    }
    if bottom.is_none() {
        bottom.clone_from(base_bottom);
    }
    if taper_angle.is_none() {
        taper_angle.clone_from(base_taper_angle);
    }
    if allow_multi_profile_faces.is_none() {
        allow_multi_profile_faces.clone_from(base_allow_multi_profile_faces);
    }
    *shape = cadmpeg_ir::features::holes::HoleShape::new(construction, exit_kind, diameter)
        .map_err(cadmpeg_core::CodecError::malformed)?;
    Ok(())
}

type ConfigurationPlaneFrame = (Point3, Vector3, Vector3);

const CONFIGURATION_PLANE_FRAME_TOLERANCE: f64 = 1.0e-8;

fn complete_configuration_face_selection(selection: &FaceSelection) -> bool {
    match selection {
        FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => !faces.is_empty(),
        FaceSelection::Historical { .. } => true,
        FaceSelection::Generated { .. } => true,
        FaceSelection::HistoricalPartial { .. } => false,
        FaceSelection::Unresolved | FaceSelection::Native(_) => false,
    }
}

fn configuration_plane_frame_matches(
    left: ConfigurationPlaneFrame,
    right: ConfigurationPlaneFrame,
) -> bool {
    let same = |left: f64, right: f64| {
        (left - right).abs()
            <= CONFIGURATION_PLANE_FRAME_TOLERANCE * left.abs().max(right.abs()).max(1.0)
    };
    [
        (left.0.x, right.0.x),
        (left.0.y, right.0.y),
        (left.0.z, right.0.z),
        (left.1.x, right.1.x),
        (left.1.y, right.1.y),
        (left.1.z, right.1.z),
        (left.2.x, right.2.x),
        (left.2.y, right.2.y),
        (left.2.z, right.2.z),
    ]
    .into_iter()
    .all(|(left, right)| same(left, right))
}

fn configuration_reference_plane_frame<'features>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    reference: &'features DatumPlaneReference,
    features: &ConfigurationDefinitions<'features, '_>,
    visiting: &mut HashSet<&'features FeatureId>,
) -> Result<Option<ConfigurationPlaneFrame>, cadmpeg_core::CodecError> {
    match reference {
        DatumPlaneReference::Feature {
            feature: feature_id,
        } => {
            const OPERATION: &str = "resolve SLDPRT configuration datum frame";
            let _depth = ctx.enter_nested(OPERATION)?;
            if ctx.contains_hash_set(visiting, feature_id, "test SLDPRT hashed identity")? {
                return Ok(None);
            }
            ctx.insert_hash_set(visiting, feature_id, OPERATION)?;
            let Some(definition) = features.get(ctx, feature_id)? else {
                ctx.remove_hash_set(visiting, feature_id, OPERATION)?;
                return Ok(None);
            };
            let frame = match definition {
                FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane { plane }) => {
                    Some(
                        crate::resolved_features::compact_reference_planes::principal_sketch_frame(
                            *plane,
                        ),
                    )
                }
                FeatureDefinition::Operation(FeatureOperation::DatumPlane { frame }) => {
                    valid_plane_frame(frame.normal().get(), frame.u_axis().get()).then_some((
                        frame.origin().get(),
                        frame.normal().get(),
                        frame.u_axis().get(),
                    ))
                }
                FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                    reference: Some(reference),
                    distance,
                }) => configuration_reference_plane_frame(ctx, reference, features, visiting)?
                    .and_then(|(origin, normal, u_axis)| {
                        let normal_length = normal.norm();
                        (normal_length.is_finite() && normal_length > f64::EPSILON).then_some((
                            Point3::new(
                                origin.x + normal.x * distance.get() / normal_length,
                                origin.y + normal.y * distance.get() / normal_length,
                                origin.z + normal.z * distance.get() / normal_length,
                            ),
                            normal,
                            u_axis,
                        ))
                    }),
                _ => None,
            };
            ctx.remove_hash_set(visiting, feature_id, OPERATION)?;
            Ok(frame)
        }
        DatumPlaneReference::ResolvedPlane { frame } => {
            Ok(
                valid_plane_frame(frame.normal().get(), frame.u_axis().get()).then_some((
                    frame.origin().get(),
                    frame.normal().get(),
                    frame.u_axis().get(),
                )),
            )
        }
        DatumPlaneReference::Face { .. } => Ok(None),
    }
}

fn copy_configuration_feature_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    id: &FeatureId,
    operation: &'static str,
) -> Result<FeatureId, cadmpeg_core::CodecError> {
    id.try_clone_for_decode(ctx, operation)
}

fn copy_configuration_plane_reference(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    reference: &DatumPlaneReference,
) -> Result<DatumPlaneReference, cadmpeg_core::CodecError> {
    const OPERATION: &str = "copy SLDPRT configuration datum reference";
    match reference {
        DatumPlaneReference::Feature { feature } => Ok(DatumPlaneReference::Feature {
            feature: copy_configuration_feature_id(ctx, feature, OPERATION)?,
        }),
        DatumPlaneReference::Face { face } => Ok(DatumPlaneReference::Face {
            face: face.try_clone_for_decode(ctx, OPERATION)?,
        }),
        DatumPlaneReference::ResolvedPlane { frame } => {
            Ok(DatumPlaneReference::ResolvedPlane { frame: *frame })
        }
    }
}

fn insert_configuration_dependency(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    dependencies: &mut cadmpeg_ir::features::DistinctMembers<FeatureId>,
    feature: &FeatureId,
) -> Result<(), cadmpeg_core::CodecError> {
    const OPERATION: &str = "retain SLDPRT configuration datum dependency";
    if !ctx.contains(dependencies.as_slice(), feature, OPERATION)? {
        let id = copy_configuration_feature_id(ctx, feature, OPERATION)?;
        dependencies.insert(ctx, id, OPERATION)?;
    }
    Ok(())
}

fn inherit_configuration_reference_plane_definition(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    id: &FeatureId,
    definition: &mut FeatureDefinition,
    dependencies: &mut cadmpeg_ir::features::DistinctMembers<FeatureId>,
    base: &ConfigurationDefinitions<'_, '_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let Some(FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
        reference: Some(base_reference),
        ..
    })) = base.get(ctx, id)?
    else {
        return Ok(());
    };
    let state_frame = match &*definition {
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: None,
            ..
        }) => None,
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::ResolvedPlane { frame }),
            ..
        }) if valid_plane_frame(frame.normal().get(), frame.u_axis().get()) => Some((
            frame.origin().get(),
            frame.normal().get(),
            frame.u_axis().get(),
        )),
        _ => return Ok(()),
    };
    let (base_frame, _visiting_storage) = ctx
        .with_scoped_storage("resolve SLDPRT configuration datum frame", || {
            configuration_reference_plane_frame(ctx, base_reference, base, &mut HashSet::new())
        })?;
    let Some(base_frame) = base_frame else {
        return Ok(());
    };
    if state_frame.is_some_and(|frame| !configuration_plane_frame_matches(frame, base_frame)) {
        return Ok(());
    }
    let replacement = copy_configuration_plane_reference(ctx, base_reference)?;
    if let DatumPlaneReference::Feature { feature } = &replacement {
        insert_configuration_dependency(ctx, dependencies, feature)?;
    }
    if let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane { reference, .. }) =
        definition
    {
        *reference = Some(replacement);
    }
    Ok(())
}

/// Reuse a document datum reference for an omitted or matching resolved frame.
fn inherit_configuration_reference_plane_semantics(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    base_features: &[cadmpeg_ir::features::Feature],
) -> Result<(), cadmpeg_core::CodecError> {
    let base = ConfigurationDefinitions::new(ctx, base_features)?;
    for feature in ctx.admit_iter(features, "inherit SLDPRT configuration datum references")? {
        let mut result = Ok(());
        feature.evaluation.edit(|definition, _| {
            result = inherit_configuration_reference_plane_definition(
                ctx,
                &feature.id,
                definition,
                &mut feature.dependencies,
                &base,
            );
        });
        result?;
    }
    Ok(())
}

/// Apply document datum references directly to each matching configuration state.
pub(crate) fn inherit_configuration_reference_plane_states(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut cadmpeg_ir::CadIr,
) -> Result<(), cadmpeg_core::CodecError> {
    let base = ConfigurationDefinitions::new(ctx, &ir.model.features)?;
    for configuration in ctx.admit_iter(
        &mut ir.model.configurations,
        "scan SLDPRT inherit_configuration_reference_plane_states values",
    )? {
        for feature in ctx.admit_iter(
            &ir.model.features,
            "scan SLDPRT inherit_configuration_reference_plane_states values",
        )? {
            let Some(state) = ctx.get_mut_btree_map(
                &mut (configuration.feature_states),
                &feature.id,
                "look up mutable SLDPRT ordered key",
            )?
            else {
                continue;
            };
            inherit_configuration_reference_plane_definition(
                ctx,
                &feature.id,
                &mut state.definition,
                &mut state.dependencies,
                &base,
            )?;
        }
    }
    Ok(())
}

fn configuration_surface_carriers(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &cadmpeg_ir::CadIr,
    configuration_index: usize,
) -> Result<Vec<cadmpeg_ir::geometry::Surface>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "copy SLDPRT configuration surface carriers";
    let configuration = &ir.model.configurations[configuration_index];
    let Some(body_ids) = configuration.bodies.as_deref() else {
        // Unresolved membership uses every neutral surface carrier.
        let mut surfaces = Vec::new();
        ctx.reserve_capacity(&mut surfaces, ir.model.surfaces.len(), OPERATION)?;
        for surface in ctx.admit_iter(
            &ir.model.surfaces,
            "scan SLDPRT configuration_surface_carriers values",
        )? {
            ctx.push_vec(
                &mut (surfaces),
                surface.try_clone_for_decode(ctx, OPERATION)?,
                OPERATION,
            )?;
        }
        return Ok(surfaces);
    };
    let mut surface_ancestry_storage =
        ctx.reserve_scoped(0, "SLDPRT temporary configuration surface ancestry")?;
    let mut bodies = HashSet::new();
    for id in ctx.admit_iter(
        body_ids,
        "scan SLDPRT configuration_surface_carriers values",
    )? {
        surface_ancestry_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut bodies,
                id,
                "index SLDPRT configuration surface ancestry",
            )
        })?;
    }
    let mut regions = HashSet::new();
    for body in ctx.admit_iter(
        &ir.model.bodies,
        "scan SLDPRT configuration_surface_carriers values",
    )? {
        if ctx.contains_hash_set(
            &bodies,
            &body.id,
            "match SLDPRT configuration surface ancestry",
        )? {
            for id in ctx.admit_iter(
                &body.regions,
                "scan SLDPRT configuration_surface_carriers values",
            )? {
                surface_ancestry_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut regions,
                        id,
                        "index SLDPRT configuration surface ancestry",
                    )
                })?;
            }
        }
    }
    let mut shells = HashSet::new();
    for region in ctx.admit_iter(
        &ir.model.regions,
        "scan SLDPRT configuration_surface_carriers values",
    )? {
        if ctx.contains_hash_set(
            &regions,
            &region.id,
            "match SLDPRT configuration surface ancestry",
        )? {
            for id in ctx.admit_iter(
                &region.shells,
                "scan SLDPRT configuration_surface_carriers values",
            )? {
                surface_ancestry_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut shells,
                        id,
                        "index SLDPRT configuration surface ancestry",
                    )
                })?;
            }
        }
    }
    let mut faces = HashSet::new();
    for shell in ctx.admit_iter(
        &ir.model.shells,
        "scan SLDPRT configuration_surface_carriers values",
    )? {
        if ctx.contains_hash_set(
            &shells,
            &shell.id,
            "match SLDPRT configuration surface ancestry",
        )? {
            for id in ctx.admit_iter(shell.faces(), "scan SLDPRT topology members")? {
                surface_ancestry_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut faces,
                        id,
                        "index SLDPRT configuration surface ancestry",
                    )
                })?;
            }
        }
    }
    let mut surface_ids = HashSet::new();
    for face in ctx.admit_iter(
        &ir.model.faces,
        "scan SLDPRT configuration_surface_carriers values",
    )? {
        if ctx.contains_hash_set(
            &faces,
            &face.id,
            "match SLDPRT configuration surface ancestry",
        )? {
            surface_ancestry_storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut surface_ids,
                    &face.surface,
                    "index SLDPRT configuration surface ancestry",
                )
            })?;
        }
    }
    let mut surfaces = Vec::new();
    for surface in ctx.admit_iter(
        &ir.model.surfaces,
        "scan SLDPRT configuration_surface_carriers values",
    )? {
        if ctx.contains_hash_set(
            &surface_ids,
            &surface.id,
            "match SLDPRT configuration surface ancestry",
        )? {
            let surface = surface.try_clone_for_decode(ctx, OPERATION)?;
            ctx.push_vec(&mut surfaces, surface, OPERATION)?;
        }
    }
    Ok(surfaces)
}

/// Give configuration-local numeric overrides the kind established by their
/// neutral parameter definition and discard incompatible native candidates.
pub(crate) fn align_configuration_parameter_kinds(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut cadmpeg_ir::CadIr,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut kinds_storage = ctx.reserve_scoped(0, "index SLDPRT configuration parameter kinds")?;
    let mut parameter_kinds = HashMap::new();
    for parameter in ctx.admit_iter(
        &ir.model.parameters,
        "scan SLDPRT configuration parameter kinds",
    )? {
        if let Some(value) = &parameter.value {
            kinds_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut parameter_kinds,
                    &parameter.id,
                    value,
                    "index SLDPRT configuration parameter kinds",
                )
            })?;
        }
    }
    for configuration in ctx.admit_iter(&mut ir.model.configurations, "align SLDPRT configuration parameter kinds")? {
        for (parameter, value) in ctx.admit_iter(&mut configuration.parameter_values, "align SLDPRT configuration parameter kinds")? {
        let Some(canonical) =
            ctx.get_hash_map(&(parameter_kinds), parameter, "look up SLDPRT hash key")?
        else {
            continue;
        };
        // The canonical parameter definition declares the kind of each
        // override, and so the quantity family of an untyped real override.
        let aligned = match (&**canonical, &*value) {
            (ParameterValue::Length(_), ParameterValue::Integer(integer)) => {
                exact_integer_f64(*integer)
                    .and_then(Length::new)
                    .map(ParameterValue::Length)
            }
            (ParameterValue::Length(_), ParameterValue::Real(real)) => {
                Some(ParameterValue::Length(Length::from_assigned_real(*real)))
            }
            (ParameterValue::Angle(_), ParameterValue::Integer(integer)) => {
                exact_integer_f64(*integer)
                    .and_then(Angle::new)
                    .map(ParameterValue::Angle)
            }
            (ParameterValue::Angle(_), ParameterValue::Real(real)) => {
                Some(ParameterValue::Angle(Angle::from_assigned_real(*real)))
            }
            (ParameterValue::Real(_), ParameterValue::Integer(integer)) => {
                exact_integer_f64(*integer)
                    .and_then(cadmpeg_ir::scalar::FiniteReal::new)
                    .map(ParameterValue::Real)
            }
            (ParameterValue::Integer(_), ParameterValue::Real(real)) => {
                let real = real.get();
                cadmpeg_core::convert::truncate_f64_to_i64(real).and_then(|integer| {
                    (exact_integer_f64(integer) == Some(real))
                        .then_some(ParameterValue::Integer(integer))
                })
            }
            // Configuration lanes can provisionally classify an untyped scalar
            // as a length. The canonical integer wins only when the values agree.
            (ParameterValue::Integer(expected), ParameterValue::Length(candidate)) => {
                let candidate = candidate.get();

                exact_integer_f64(*expected)
                    .filter(|expected_value| {
                        (candidate - expected_value).abs()
                            <= EPS_CONFIGURATION_ALIGN_CONFIGURATION_PARAMETER_KINDS_E9
                                * candidate.abs().max(expected_value.abs()).max(1.0)
                    })
                    .map(|_| ParameterValue::Integer(*expected))
            }
            _ => None,
        };
        if let Some(aligned) = aligned {
            *value = aligned;
        }
    }
    }
    for configuration in ctx.admit_iter(
        &mut ir.model.configurations,
        "retain SLDPRT configuration parameter kinds",
    )? {
        ctx.retain_btree_map(
            &mut configuration.parameter_values,
            |parameter, value| {
                Ok(
                    match ctx.get_hash_map(
                        &parameter_kinds,
                        parameter,
                        "look up SLDPRT hash key",
                    )? {
                        Some(canonical) => {
                            std::mem::discriminant(&**canonical) == std::mem::discriminant(value)
                        }
                        None => true,
                    },
                )
            },
            "retain SLDPRT configuration parameter kinds",
        )?;
    }
    Ok(())
}

/// The configuration slot a configuration lane names.
fn lane_slot(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    lane: &crate::records::FeatureInputLane,
) -> Result<Option<u32>, cadmpeg_core::CodecError> {
    Ok(match lane.configuration.as_deref() {
        Some(value) => ctx
            .parse_text(value, "parse SLDPRT configuration lane slot")?
            .ok(),
        None => None,
    })
}

/// The configuration each lane slot names: the one configuration whose `id`
/// property states the slot, or, when none does, the one configuration without
/// an `id` whose ordinal is the slot.
struct ConfigurationSlots<'ctx> {
    explicit: HashMap<u32, Option<usize>>,
    ordinal: HashMap<u32, Option<usize>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'ctx> ConfigurationSlots<'ctx> {
    fn new(
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
        configurations: &[DesignConfiguration],
    ) -> Result<Self, cadmpeg_core::CodecError> {
        const OPERATION: &str = "match SLDPRT configuration lane identities";
        let mut storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut explicit = HashMap::new();
        let mut ordinal = HashMap::new();
        for (index, configuration) in ctx
            .admit_iter(
                configurations,
                "scan SLDPRT configuration_index_for_slot values",
            )?
            .enumerate()
        {
            let id = match ctx.get_btree_map(&configuration.properties, "id", OPERATION)? {
                Some(value) => ctx.parse_text::<u32>(value, OPERATION)?.ok(),
                None => None,
            };
            let (table, slot) = match id {
                Some(id) => (&mut explicit, id),
                None => (&mut ordinal, configuration.ordinal),
            };
            if let Some(previous) = ctx.get_mut_hash_map(table, &slot, OPERATION)? {
                *previous = None;
                continue;
            }
            storage.with_storage(|| ctx.insert_hash_map(table, slot, Some(index), OPERATION))?;
        }
        Ok(Self {
            explicit,
            ordinal,
            _storage: storage,
        })
    }

    fn configuration(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        slot: u32,
    ) -> Result<Option<usize>, cadmpeg_core::CodecError> {
        const OPERATION: &str = "match SLDPRT configuration lane identities";
        if let Some(explicit) = ctx.get_hash_map(&self.explicit, &slot, OPERATION)? {
            return Ok(*explicit);
        }
        Ok(ctx
            .get_hash_map(&self.ordinal, &slot, OPERATION)?
            .copied()
            .flatten())
    }
}

pub(super) fn configuration_lane_assignments(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    configurations: &[DesignConfiguration],
    lanes: &[crate::records::FeatureInputLane],
) -> Result<Vec<(usize, usize)>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "index SLDPRT configuration lane identities")?;
    let mut lanes_by_configuration = BTreeMap::<u32, Vec<usize>>::new();
    for (lane_index, lane) in ctx
        .admit_iter(lanes, "scan SLDPRT configuration lane identities")?
        .enumerate()
    {
        if !configuration_state_lane(lane) {
            continue;
        }
        let Some(slot_index) = lane_slot(ctx, lane)? else {
            continue;
        };
        scratch.with_storage(|| {
            ctx.push_btree_group(
                &mut lanes_by_configuration,
                slot_index,
                lane_index,
                "index SLDPRT configuration lane identities",
                "collect SLDPRT configuration lane indices",
            )
        })?;
    }
    let slots = ConfigurationSlots::new(ctx, configurations)?;
    let mut result = Vec::new();
    for (slot_index, lane_indices) in ctx.admit_iter(
        lanes_by_configuration,
        "collect SLDPRT configuration lane assignments",
    )? {
        let [lane_index] = lane_indices.as_slice() else {
            continue;
        };
        if let Some(configuration_index) = slots.configuration(ctx, slot_index)? {
            ctx.push_vec(
                &mut result,
                (configuration_index, *lane_index),
                "collect SLDPRT configuration lane assignments",
            )?;
        }
    }
    Ok(result)
}

pub(crate) fn unresolved_configuration_lanes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    configurations: &[DesignConfiguration],
    lanes: &[crate::records::FeatureInputLane],
) -> Result<usize, cadmpeg_core::CodecError> {
    const OPERATION: &str = "count unresolved SLDPRT configuration lanes";
    let (assigned_lanes, _assigned_storage) = ctx.with_scoped_storage("SLDPRT configuration lane assignments", || configuration_lane_assignments(ctx, configurations, lanes))?;
    let mut scratch = ctx.reserve_scoped(0, "index SLDPRT configuration lane occurrences")?;
    let mut assigned = scratch.with_storage(|| ctx.alloc_filled(lanes.len(), false, OPERATION))?;
    for (_, lane_index) in ctx.admit_iter(&assigned_lanes, OPERATION)? {
        assigned[*lane_index] = true;
    }
    let mut occurrences = HashMap::<&str, usize>::new();
    for lane in ctx.admit_iter(lanes, "count SLDPRT configuration lane identities")? {
        if !configuration_state_lane(lane) {
            continue;
        }
        let Some(slot) = lane.configuration.as_deref() else {
            continue;
        };
        if let Some(count) = ctx.get_mut_hash_map(
            &mut occurrences,
            slot,
            "index SLDPRT configuration lane occurrences",
        )? {
            *count += 1;
            continue;
        }
        scratch.with_storage(|| {
            ctx.insert_hash_map(
                &mut occurrences,
                slot,
                1,
                "index SLDPRT configuration lane occurrences",
            )
        })?;
    }
    let mut count = 0;
    for (lane_index, lane) in ctx
        .admit_iter(lanes, "scan SLDPRT configuration lane identities")?
        .enumerate()
    {
        if !configuration_state_lane(lane) {
            continue;
        }
        if match lane.configuration.as_deref() {
            Some(slot) => {
                ctx.get_hash_map(&occurrences, slot, "look up SLDPRT hash key")?
                    .copied()
                    != Some(1)
                    || !assigned[lane_index]
            }
            None => false,
        } {
            count += 1;
        }
    }
    Ok(count)
}

fn configuration_state_lane(lane: &crate::records::FeatureInputLane) -> bool {
    !crate::resolved_features::assembly::is_supplemental_config_lane(lane)
}
