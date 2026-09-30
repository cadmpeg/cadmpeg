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

struct ConfigurationDefinitions<'features> {
    definitions: HashMap<&'features FeatureId, &'features FeatureDefinition>,
    key_bytes: usize,
}

impl<'features> ConfigurationDefinitions<'features> {
    fn new(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        features: &'features [cadmpeg_ir::features::Feature],
    ) -> Result<Self, cadmpeg_core::CodecError> {
        const OPERATION: &str = "index SLDPRT configuration base definitions";
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(features.len()),
            OPERATION,
        )?;
        let key_bytes = features.iter().try_fold(0_usize, |bytes, feature| {
            bytes
                .checked_add(feature.id.as_str().len())
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))
        })?;
        let mut definitions = HashMap::new();
        ctx.charge_collection_items(
            cadmpeg_core::decode::u64_from_index(features.len()),
            OPERATION,
        )?;
        definitions
            .try_reserve(features.len())
            .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        for feature in features {
            let work = key_bytes
                .checked_add(feature.id.as_str().len())
                .and_then(|bytes| bytes.checked_add(1))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), OPERATION)?;
            definitions.insert(&feature.id, feature.evaluation.definition());
        }
        Ok(Self {
            definitions,
            key_bytes,
        })
    }

    fn get(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        id: &FeatureId,
    ) -> Result<Option<&'features FeatureDefinition>, cadmpeg_core::CodecError> {
        const OPERATION: &str = "match SLDPRT configuration base definition";
        let work = self
            .key_bytes
            .checked_add(id.as_str().len())
            .and_then(|bytes| bytes.checked_add(1))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), OPERATION)?;
        Ok(self.definitions.get(id).copied())
    }
}

fn apply_configuration_state(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature: &mut cadmpeg_ir::features::Feature,
    state: &cadmpeg_ir::features::ConfigurationFeatureState,
) -> Result<(), cadmpeg_core::CodecError> {
    let state = state.try_clone_charged(ctx, "retain SLDPRT configuration feature state")?;
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
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(features.len())
            .checked_mul(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                cadmpeg_ir::features::Feature,
            >()))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
        OPERATION,
    )?;
    let mut copied = Vec::new();
    ctx.reserve_collection_vec(&mut copied, features.len(), OPERATION)?;
    for feature in features {
        copied.push(feature.try_clone_charged(ctx, OPERATION)?);
    }
    Ok(copied)
}

fn copy_configuration_state_features(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    states: &BTreeMap<FeatureId, cadmpeg_ir::features::ConfigurationFeatureState>,
) -> Result<Vec<cadmpeg_ir::features::Feature>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "retain SLDPRT evaluated configuration features";
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(states.len()),
        OPERATION,
    )?;
    let key_bytes = states
        .keys()
        .try_fold(0u64, |bytes, key| {
            bytes.checked_add(cadmpeg_core::decode::u64_from_index(key.as_str().len()))
        })
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    let mut copied = Vec::new();
    for feature in features {
        ctx.charge_work(
            key_bytes
                .checked_add(cadmpeg_core::decode::u64_from_index(
                    feature.id.as_str().len(),
                ))
                .and_then(|bytes| bytes.checked_mul(8))
                .and_then(|work| {
                    work.checked_add(
                        cadmpeg_core::decode::u64_from_index(states.len()).checked_mul(64)?,
                    )
                })
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        let Some(state) = states.get(&feature.id) else {
            continue;
        };
        ctx.reserve_collection_vec(&mut copied, 1, OPERATION)?;
        let mut feature = feature.try_clone_charged(ctx, OPERATION)?;
        apply_configuration_state(ctx, &mut feature, state)?;
        copied.push(feature);
    }
    Ok(copied)
}

fn charge_configuration_state_lookup(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &BTreeMap<FeatureId, cadmpeg_ir::features::ConfigurationFeatureState>,
    id: &FeatureId,
) -> Result<(), cadmpeg_core::CodecError> {
    const OPERATION: &str = "match SLDPRT configuration feature state";
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(states.len()),
        OPERATION,
    )?;
    let bytes = states
        .keys()
        .try_fold(
            cadmpeg_core::decode::u64_from_index(id.as_str().len()),
            |bytes, key| {
                bytes.checked_add(cadmpeg_core::decode::u64_from_index(key.as_str().len()))
            },
        )
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(
        bytes
            .checked_mul(8)
            .and_then(|work| {
                work.checked_add(
                    cadmpeg_core::decode::u64_from_index(states.len()).checked_mul(64)?,
                )
            })
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
        OPERATION,
    )
}

fn insert_configuration_value<K: Ord, V>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    values: &mut BTreeMap<K, V>,
    key: K,
    value: V,
    key_len: usize,
    key_bytes: &mut usize,
) -> Result<(), cadmpeg_core::CodecError> {
    const OPERATION: &str = "collect SLDPRT configuration values";
    let work = values
        .len()
        .checked_add(1)
        .and_then(|count| count.checked_mul(key_len))
        .and_then(|bytes| bytes.checked_add(*key_bytes))
        .and_then(|bytes| bytes.checked_add(1))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), OPERATION)?;
    let bytes = key_bytes
        .checked_add(key_len)
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    ctx.charge_collection_items(1, OPERATION)?;
    values.insert(key, value);
    *key_bytes = bytes;
    Ok(())
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
    let base_definitions = ConfigurationDefinitions::new(ctx, &ir.model.features)?;
    for configuration in &mut ir.model.configurations {
        configuration.parameter_values.clear();
        configuration.feature_states.clear();
    }
    for (configuration_index, lane_index) in
        configuration_lane_assignments(ctx, &ir.model.configurations, lanes)?
    {
        let scoped_lanes = &lanes[lane_index..=lane_index];
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
        let mut parameter_values = BTreeMap::new();
        let mut parameter_key_bytes = 0;
        for parameter in project_parameters(ctx, &projection)? {
            let Some(value) = parameter.value else {
                continue;
            };
            let key_len = parameter.id.as_str().len();
            insert_configuration_value(
                ctx,
                &mut parameter_values,
                parameter.id,
                value,
                key_len,
                &mut parameter_key_bytes,
            )?;
        }
        ir.model.configurations[configuration_index].parameter_values = parameter_values;

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
        let mut feature_key_bytes = 0;
        for mut feature in features {
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
                            &feature,
                            histories,
                            scoped_lanes,
                        );
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
            let key_len = id.as_str().len();
            insert_configuration_value(
                ctx,
                &mut feature_states,
                id,
                state,
                key_len,
                &mut feature_key_bytes,
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
    for lane in lanes
        .iter()
        .filter(|lane| crate::resolved_features::assembly::is_supplemental_config_lane(lane))
    {
        let Some(slot_index) = lane
            .configuration
            .as_deref()
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        let Some(configuration_index) =
            configuration_index_for_slot(ctx, &ir.model.configurations, slot_index)?
        else {
            continue;
        };
        let states = &ir.model.configurations[configuration_index].feature_states;
        let mut features = copy_configuration_features(ctx, &ir.model.features)?;
        for feature in &mut features {
            charge_configuration_state_lookup(ctx, states, &feature.id)?;
            let Some(state) = states.get(&feature.id) else {
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
        for feature in features {
            charge_configuration_state_lookup(ctx, states, &feature.id)?;
            let Some(state) = states.get_mut(&feature.id) else {
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
    for (configuration_index, lane_index) in
        configuration_lane_assignments(ctx, &ir.model.configurations, lanes)?
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
        for feature in features {
            charge_configuration_state_lookup(ctx, states, &feature.id)?;
            let Some(state) = states.get_mut(&feature.id) else {
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
    for feature in features {
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
    for (configuration_index, lane_index) in
        configuration_lane_assignments(ctx, &ir.model.configurations, lanes)?
    {
        let surfaces = configuration_surface_carriers(ctx, ir, configuration_index)?;
        let scoped_lanes = &lanes[lane_index..=lane_index];
        let states = &ir.model.configurations[configuration_index].feature_states;
        let mut features = copy_configuration_state_features(ctx, &ir.model.features, states)?;
        inherit_configuration_reference_plane_semantics(ctx, &mut features, &ir.model.features)?;
        let mut reusable_spatial_sketches = ConfigurationIdentitySet::new(
            "index SLDPRT configuration spatial sketches",
            "match SLDPRT configuration spatial sketch",
        );
        for sketch in &ir.model.spatial_sketches {
            const OPERATION: &str = "match SLDPRT configuration spatial sketch scope";
            let work = sketch
                .configuration
                .as_deref()
                .map_or(0, str::len)
                .checked_add(sketch.native_ref.as_deref().map_or(0, str::len))
                .and_then(|bytes| bytes.checked_add(scoped_lanes[0].id.len()))
                .and_then(|bytes| {
                    bytes.checked_add(scoped_lanes[0].configuration.as_deref().map_or(0, str::len))
                })
                .and_then(|bytes| bytes.checked_add(1))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), OPERATION)?;
            if sketch.configuration.is_none()
                || sketch.native_ref.as_deref() == Some(scoped_lanes[0].id.as_str())
                || scoped_lanes[0]
                    .configuration
                    .as_deref()
                    .is_some_and(|configuration| {
                        sketch.configuration.as_deref() == Some(configuration)
                    })
            {
                reusable_spatial_sketches.insert(ctx, &sketch.id, sketch.id.as_str())?;
            }
        }
        let base_definitions = ConfigurationDefinitions::new(ctx, &ir.model.features)?;
        for feature in &mut features {
            if let FeatureDefinition::Operation(FeatureOperation::SpatialSketch { sketch }) =
                feature.evaluation.definition()
            {
                const OPERATION: &str = "retain SLDPRT configuration spatial sketch identity";
                let id = feature.id.as_str();
                let work = id
                    .len()
                    .checked_mul(4)
                    .and_then(|bytes| bytes.checked_add(32))
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), OPERATION)?;
                let text = if let Some((prefix, suffix)) = id.split_once(":model:feature#") {
                    crate::text_admission::format_retained(
                        ctx,
                        format_args!("{prefix}:model:spatial-sketch#{suffix}"),
                        OPERATION,
                    )?
                } else {
                    crate::text_admission::format_retained(ctx, format_args!("{id}"), OPERATION)?
                };
                let Ok(expected) = cadmpeg_ir::sketches::SpatialSketchId::mint(text) else {
                    continue;
                };
                if sketch.is_none()
                    && reusable_spatial_sketches.contains(ctx, &expected, expected.as_str())?
                {
                    feature
                        .evaluation
                        .set_definition(FeatureDefinition::Operation(
                            FeatureOperation::SpatialSketch {
                                sketch: Some(expected),
                            },
                        ));
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
                && reusable_spatial_sketches.contains(ctx, base_sketch, base_sketch.as_str())?
            {
                const OPERATION: &str = "copy SLDPRT configuration spatial sketch identity";
                let work = base_sketch
                    .as_str()
                    .len()
                    .checked_mul(4)
                    .and_then(|bytes| bytes.checked_add(1))
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), OPERATION)?;
                let text = crate::text_admission::format_retained(
                    ctx,
                    format_args!("{base_sketch}"),
                    OPERATION,
                )?;
                let copied = cadmpeg_ir::sketches::SpatialSketchId::mint(text)
                    .map_err(cadmpeg_core::CodecError::malformed)?;
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
        let mut saved_values = Vec::new();
        let result = (|| -> Result<(), cadmpeg_core::CodecError> {
            const OPERATION: &str = "overlay SLDPRT configuration parameter values";
            let values = &ir.model.configurations[configuration_index].parameter_values;
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(values.len()),
                OPERATION,
            )?;
            let key_bytes = values.keys().try_fold(0_usize, |bytes, id| {
                bytes
                    .checked_add(id.as_str().len())
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))
            })?;
            for (index, parameter) in parameters.iter_mut().enumerate() {
                let work = key_bytes
                    .checked_add(parameter.id.as_str().len())
                    .and_then(|bytes| bytes.checked_add(1))
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), OPERATION)?;
                let Some(value) = values.get(&parameter.id) else {
                    continue;
                };
                let work = saved_values
                    .len()
                    .checked_add(1)
                    .and_then(|count| {
                        count.checked_mul(std::mem::size_of::<(usize, Option<ParameterValue>)>())
                    })
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), OPERATION)?;
                ctx.reserve_collection_vec(&mut saved_values, 1, OPERATION)?;
                let copied = value.try_clone_charged(ctx, OPERATION)?;
                saved_values.push((index, parameter.value.replace(copied)));
            }
            crate::resolved_features::profiles::bind_sketch_profiles(
                ctx,
                &mut features,
                &mut ir.model.sketches,
                &mut ir.model.sketch_entities,
                &mut ir.model.sketch_constraints,
                &parameters,
                histories,
                scoped_lanes,
                annotations,
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
            for feature in features {
                let Some(state) = ir.model.configurations[configuration_index]
                    .feature_states
                    .get_mut(&feature.id)
                else {
                    continue;
                };
                *state = configuration_feature_state(feature).1;
            }
            Ok(())
        })();
        for (index, value) in saved_values {
            parameters[index].value = value;
        }
        ir.model.parameters = parameters;
        result?;
    }
    let scoped_configuration_indices =
        configuration_lane_assignments(ctx, &ir.model.configurations, lanes)?;
    let base = ConfigurationDefinitions::new(ctx, &ir.model.features)?;
    for (configuration_index, configuration) in ir.model.configurations.iter_mut().enumerate() {
        // DI-55: a valid configuration lane owns its unresolved slots. The
        // document definition is a fallback only for an unscoped snapshot.
        if scoped_configuration_indices
            .iter()
            .any(|(assigned, _)| *assigned == configuration_index)
        {
            continue;
        }
        for (feature_id, state) in &mut configuration.feature_states {
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
                *face = base_face.try_clone_charged(ctx, "copy SLDPRT configuration datum face")?;
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
    let mut construction = shape.construction().try_clone_charged(ctx, OPERATION)?;
    let mut exit_kind = *shape.exit_kind();
    let mut diameter = shape.diameter();
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
            .map(|value| value.try_clone_charged(ctx, "copy SLDPRT configuration hole face"))
            .transpose()?;
    }
    if profile.is_none() {
        *profile = base_profile
            .as_ref()
            .map(|value| value.try_clone_charged(ctx, "copy SLDPRT configuration hole profile"))
            .transpose()?;
    }
    if profile_filter.is_none() {
        profile_filter.clone_from(base_profile_filter);
    }
    if inherit_placements && placements.is_none() {
        if let Some(base_placements) = base_placements {
            const OPERATION: &str = "copy SLDPRT configuration hole placements";
            let work = base_placements
                .len()
                .checked_mul(std::mem::size_of::<
                    cadmpeg_ir::features::holes::HolePlacement,
                >())
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), OPERATION)?;
            let mut copied = Vec::new();
            ctx.reserve_collection_vec(&mut copied, base_placements.len(), OPERATION)?;
            copied.extend(base_placements.iter().cloned());
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
                    .map(|value| value.try_clone_boxed_charged(ctx, OPERATION))
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
            *construction = base_construction.try_clone_charged(ctx, OPERATION)?;
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
            .map(|value| value.try_clone_charged(ctx, "copy SLDPRT configuration hole termination"))
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
    features: &ConfigurationDefinitions<'features>,
    visiting: &mut HashSet<&'features FeatureId>,
) -> Result<Option<ConfigurationPlaneFrame>, cadmpeg_core::CodecError> {
    match reference {
        DatumPlaneReference::Feature {
            feature: feature_id,
        } => {
            const OPERATION: &str = "resolve SLDPRT configuration datum frame";
            let _depth = ctx.enter_nested(OPERATION)?;
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(visiting.len()),
                OPERATION,
            )?;
            let bytes = visiting
                .iter()
                .try_fold(feature_id.as_str().len(), |bytes, id| {
                    bytes
                        .checked_add(id.as_str().len())
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))
                })?;
            let work = bytes
                .checked_add(1)
                .and_then(|bytes| bytes.checked_mul(4))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), OPERATION)?;
            if visiting.contains(feature_id) {
                return Ok(None);
            }
            ctx.charge_collection_items(1, OPERATION)?;
            visiting
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            visiting.insert(feature_id);
            let Some(definition) = features.get(ctx, feature_id)? else {
                visiting.remove(feature_id);
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
            visiting.remove(feature_id);
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
    let work = id
        .as_str()
        .len()
        .checked_mul(4)
        .and_then(|bytes| bytes.checked_add(1))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), operation)?;
    FeatureId::mint(crate::text_admission::format_retained(
        ctx,
        format_args!("{}", id.as_str()),
        operation,
    )?)
    .map_err(cadmpeg_core::CodecError::malformed)
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
            face: face.try_clone_charged(ctx, OPERATION)?,
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
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(dependencies.len()),
        OPERATION,
    )?;
    let bytes = dependencies
        .iter()
        .try_fold(feature.as_str().len(), |bytes, id| {
            bytes
                .checked_add(id.as_str().len())
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))
        })?;
    let work = dependencies
        .len()
        .checked_add(1)
        .and_then(|count| count.checked_mul(std::mem::size_of::<FeatureId>()))
        .and_then(|slots| {
            bytes
                .checked_mul(4)
                .and_then(|bytes| bytes.checked_add(slots))
        })
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), OPERATION)?;
    if !dependencies.contains(feature) {
        let id = copy_configuration_feature_id(ctx, feature, OPERATION)?;
        dependencies.try_insert_charged(id, ctx, OPERATION)?;
    }
    Ok(())
}

fn inherit_configuration_reference_plane_definition(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    id: &FeatureId,
    definition: &mut FeatureDefinition,
    dependencies: &mut cadmpeg_ir::features::DistinctMembers<FeatureId>,
    base: &ConfigurationDefinitions<'_>,
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
    let Some(base_frame) =
        configuration_reference_plane_frame(ctx, base_reference, base, &mut HashSet::new())?
    else {
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
    for feature in features {
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
    for configuration in &mut ir.model.configurations {
        const OPERATION: &str = "match SLDPRT configuration datum states";
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(configuration.feature_states.len()),
            OPERATION,
        )?;
        let key_bytes = configuration
            .feature_states
            .keys()
            .try_fold(0_usize, |bytes, id| {
                bytes
                    .checked_add(id.as_str().len())
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))
            })?;
        for feature in &ir.model.features {
            let work = key_bytes
                .checked_add(feature.id.as_str().len())
                .and_then(|bytes| bytes.checked_add(1))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), OPERATION)?;
            let Some(state) = configuration.feature_states.get_mut(&feature.id) else {
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

struct ConfigurationIdentitySet<'id, T> {
    ids: HashSet<&'id T>,
    key_bytes: usize,
    insert_operation: &'static str,
    match_operation: &'static str,
}

impl<'id, T: Eq + std::hash::Hash> ConfigurationIdentitySet<'id, T> {
    fn new(insert_operation: &'static str, match_operation: &'static str) -> Self {
        Self {
            ids: HashSet::new(),
            key_bytes: 0,
            insert_operation,
            match_operation,
        }
    }

    fn insert(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        id: &'id T,
        text: &str,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let operation = self.insert_operation;
        let bytes = self
            .key_bytes
            .checked_add(text.len())
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        let work = bytes
            .checked_add(1)
            .and_then(|bytes| bytes.checked_mul(4))
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), operation)?;
        ctx.charge_collection_items(1, operation)?;
        self.ids
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        self.ids.insert(id);
        self.key_bytes = bytes;
        Ok(())
    }

    fn contains(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        id: &T,
        text: &str,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        let operation = self.match_operation;
        let work = self
            .key_bytes
            .checked_add(text.len())
            .and_then(|bytes| bytes.checked_add(1))
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), operation)?;
        Ok(self.ids.contains(id))
    }
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
        ctx.reserve_collection_vec(&mut surfaces, ir.model.surfaces.len(), OPERATION)?;
        for surface in &ir.model.surfaces {
            surfaces.push(surface.try_clone_charged(ctx, OPERATION)?);
        }
        return Ok(surfaces);
    };
    let mut bodies = ConfigurationIdentitySet::new(
        "index SLDPRT configuration surface ancestry",
        "match SLDPRT configuration surface ancestry",
    );
    for id in body_ids {
        bodies.insert(ctx, id, id.as_str())?;
    }
    let mut regions = ConfigurationIdentitySet::new(
        "index SLDPRT configuration surface ancestry",
        "match SLDPRT configuration surface ancestry",
    );
    for body in &ir.model.bodies {
        if bodies.contains(ctx, &body.id, body.id.as_str())? {
            for id in &body.regions {
                regions.insert(ctx, id, id.as_str())?;
            }
        }
    }
    let mut shells = ConfigurationIdentitySet::new(
        "index SLDPRT configuration surface ancestry",
        "match SLDPRT configuration surface ancestry",
    );
    for region in &ir.model.regions {
        if regions.contains(ctx, &region.id, region.id.as_str())? {
            for id in &region.shells {
                shells.insert(ctx, id, id.as_str())?;
            }
        }
    }
    let mut faces = ConfigurationIdentitySet::new(
        "index SLDPRT configuration surface ancestry",
        "match SLDPRT configuration surface ancestry",
    );
    for shell in &ir.model.shells {
        if shells.contains(ctx, &shell.id, shell.id.as_str())? {
            for id in shell.faces() {
                faces.insert(ctx, id, id.as_str())?;
            }
        }
    }
    let mut surface_ids = ConfigurationIdentitySet::new(
        "index SLDPRT configuration surface ancestry",
        "match SLDPRT configuration surface ancestry",
    );
    for face in &ir.model.faces {
        if faces.contains(ctx, &face.id, face.id.as_str())? {
            surface_ids.insert(ctx, &face.surface, face.surface.as_str())?;
        }
    }
    let mut surfaces = Vec::new();
    for surface in &ir.model.surfaces {
        if surface_ids.contains(ctx, &surface.id, surface.id.as_str())? {
            let work = surfaces
                .len()
                .checked_add(1)
                .and_then(|count| {
                    count.checked_mul(std::mem::size_of::<cadmpeg_ir::geometry::Surface>())
                })
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), OPERATION)?;
            ctx.reserve_collection_vec(&mut surfaces, 1, OPERATION)?;
            surfaces.push(surface.try_clone_charged(ctx, OPERATION)?);
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
    let mut parameter_kinds = HashMap::new();
    for parameter in &ir.model.parameters {
        ctx.charge_work(1, "scan SLDPRT configuration parameter kinds")?;
        if let Some(value) = &parameter.value {
            ctx.charge_collection_items(1, "index SLDPRT configuration parameter kinds")?;
            parameter_kinds.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(
                    "index SLDPRT configuration parameter kinds",
                    u64::MAX - 1,
                    u64::MAX,
                )
            })?;
            parameter_kinds.insert(&parameter.id, value);
        }
    }
    for value in ir
        .model
        .configurations
        .iter_mut()
        .flat_map(|configuration| &mut configuration.parameter_values)
    {
        ctx.charge_work(1, "align SLDPRT configuration parameter kinds")?;
        let (parameter, value) = value;
        let Some(canonical) = parameter_kinds.get(parameter) else {
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
                if real < i64::MIN as f64 || real >= -(i64::MIN as f64) {
                    None
                } else {
                    let integer = real as i64;
                    (integer as f64 == real).then_some(ParameterValue::Integer(integer))
                }
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
    for configuration in &mut ir.model.configurations {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(configuration.parameter_values.len()),
            "retain SLDPRT configuration parameter kinds",
        )?;
        configuration.parameter_values.retain(|parameter, value| {
            let Some(canonical) = parameter_kinds.get(parameter) else {
                return true;
            };
            std::mem::discriminant(&**canonical) == std::mem::discriminant(value)
        });
    }
    Ok(())
}

pub(super) fn configuration_lane_assignments(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    configurations: &[DesignConfiguration],
    lanes: &[crate::records::FeatureInputLane],
) -> Result<Vec<(usize, usize)>, cadmpeg_core::CodecError> {
    let mut lanes_by_configuration = BTreeMap::<u32, Vec<usize>>::new();
    for (lane_index, lane) in lanes
        .iter()
        .enumerate()
        .filter(|(_, lane)| configuration_state_lane(lane))
    {
        ctx.charge_work(1, "scan SLDPRT configuration lane identities")?;
        let Some(slot_index) = lane
            .configuration
            .as_deref()
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        if !lanes_by_configuration.contains_key(&slot_index) {
            ctx.charge_collection_items(1, "index SLDPRT configuration lane identities")?;
        }
        let indices = lanes_by_configuration.entry(slot_index).or_default();
        ctx.reserve_collection_vec(indices, 1, "collect SLDPRT configuration lane indices")?;
        indices.push(lane_index);
    }
    let mut result = Vec::new();
    for (slot_index, lane_indices) in lanes_by_configuration {
        let [lane_index] = lane_indices.as_slice() else {
            continue;
        };
        if let Some(configuration_index) =
            configuration_index_for_slot(ctx, configurations, slot_index)?
        {
            ctx.reserve_collection_vec(
                &mut result,
                1,
                "collect SLDPRT configuration lane assignments",
            )?;
            result.push((configuration_index, *lane_index));
        }
    }
    Ok(result)
}

fn configuration_index_for_slot(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    configurations: &[DesignConfiguration],
    slot_index: u32,
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "match SLDPRT configuration lane identities";
    for configuration in configurations {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(configuration.properties.len())
                .checked_mul(128)
                .and_then(|work| work.checked_add(8))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        for (key, value) in &configuration.properties {
            let work = key
                .as_str()
                .len()
                .checked_add(value.len())
                .and_then(|bytes| bytes.checked_add(2))
                .and_then(|bytes| bytes.checked_mul(16))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), OPERATION)?;
        }
    }
    let mut explicit = configurations
        .iter()
        .enumerate()
        .filter(|(_, configuration)| {
            configuration
                .properties
                .get("id")
                .and_then(|value| value.parse::<u32>().ok())
                == Some(slot_index)
        })
        .map(|(index, _)| index);
    if let Some(index) = explicit.next() {
        return Ok(explicit.next().is_none().then_some(index));
    }
    let mut fallback = configurations
        .iter()
        .enumerate()
        .filter(|(_, configuration)| {
            configuration
                .properties
                .get("id")
                .and_then(|value| value.parse::<u32>().ok())
                .is_none()
                && configuration.ordinal == slot_index
        })
        .map(|(index, _)| index);
    let Some(index) = fallback.next() else {
        return Ok(None);
    };
    Ok(fallback.next().is_none().then_some(index))
}

pub(crate) fn unresolved_configuration_lanes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    configurations: &[DesignConfiguration],
    lanes: &[crate::records::FeatureInputLane],
) -> Result<usize, cadmpeg_core::CodecError> {
    let assigned_lanes = configuration_lane_assignments(ctx, configurations, lanes)?;
    let mut occurrences = HashMap::<&str, usize>::new();
    for lane in lanes
        .iter()
        .filter(|lane| configuration_state_lane(lane))
        .filter_map(|lane| lane.configuration.as_deref())
    {
        ctx.charge_work(1, "count SLDPRT configuration lane identities")?;
        if !occurrences.contains_key(lane) {
            ctx.charge_collection_items(1, "index SLDPRT configuration lane occurrences")?;
            occurrences.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit(
                    "index SLDPRT configuration lane occurrences",
                    u64::MAX - 1,
                    u64::MAX,
                )
            })?;
        }
        *occurrences.entry(lane).or_default() += 1;
    }
    let mut count = 0;
    for (lane_index, lane) in lanes
        .iter()
        .enumerate()
        .filter(|(_, lane)| configuration_state_lane(lane))
    {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(assigned_lanes.len()),
            "count unresolved SLDPRT configuration lanes",
        )?;
        ctx.charge_work(1, "count unresolved SLDPRT configuration lanes")?;
        if lane.configuration.as_deref().is_some_and(|slot| {
            occurrences.get(slot).copied() != Some(1)
                || !assigned_lanes
                    .iter()
                    .any(|(_, assigned)| *assigned == lane_index)
        }) {
            count += 1;
        }
    }
    Ok(count)
}

fn configuration_state_lane(lane: &crate::records::FeatureInputLane) -> bool {
    !crate::resolved_features::assembly::is_supplemental_config_lane(lane)
}
