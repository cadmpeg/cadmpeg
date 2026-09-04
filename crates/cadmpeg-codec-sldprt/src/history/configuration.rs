// SPDX-License-Identifier: Apache-2.0
//! Configuration-lane enrichment and design-state projection.

use crate::records::FeatureHistory;
use cadmpeg_ir::features::{
    Angle, ConfigurationBodies, DatumPlaneReference, DesignConfiguration, FaceSelection,
    FeatureDefinition, FeatureId, Length, LinearTermination, ParameterValue,
};
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::history::bind::bind_unique_sketch_feature;
use crate::history::literals::valid_plane_frame;
use crate::history::parameters::{
    apply_evaluated_parameters, exact_integer_f64, project_parameters,
};
use crate::history::project::project_features;

const EPS_CONFIGURATION_ALIGN_CONFIGURATION_PARAMETER_KINDS_E9: f64 = 1.0e-9;

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
    histories: &mut [FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
) {
    crate::resolved_features::parameters::enrich_history_parameters(histories, lanes, true);
}

/// Parameter-only mode of `resolved_features::parameters::enrich_history_parameters` (the
/// historical `false` argument): projects parameter values without the semantic
/// feature-input projection.
pub(crate) fn enrich_history_parameters_values_only(
    histories: &mut [FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
) {
    crate::resolved_features::parameters::enrich_history_parameters(histories, lanes, false);
}

/// The shared native-lane enrichment prefix, declared once for both codec
/// directions. Runs the ordered extrusion-termination, combine, sweep-path,
/// sketch-block, split-line, parameter, reference-plane, reference-point,
/// coordinate-system, PMI, evaluated-parameter, reference-axis, and
/// revolution-input enrichments; the read path additionally applies
/// hole-construction enrichment (selected by `mode`).
pub(crate) fn enrich_history_semantic(
    histories: &mut [FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
    pmi_dimensions: &[crate::records::PmiDimension],
    mode: HistoryEnrichment,
) {
    crate::resolved_features::terminations::enrich_history_extrusion_terminations(histories, lanes);
    crate::resolved_features::terminations::enrich_history_combine_selections(histories, lanes);
    crate::resolved_features::terminations::enrich_history_sweep_paths(histories, lanes);
    crate::resolved_features::reference_geometry::enrich_history_sketch_block_references(
        histories, lanes,
    );
    crate::resolved_features::operations::enrich_history_split_lines(histories, lanes);
    crate::resolved_features::direct_edits::enrich_history_move_face_translations(histories, lanes);
    crate::resolved_features::direct_edits::enrich_history_move_body_translations(histories, lanes);
    enrich_history_parameters_semantic(histories, lanes);
    if matches!(mode, HistoryEnrichment::Read) {
        crate::resolved_features::holes::enrich_history_hole_constructions(histories, lanes);
        crate::resolved_features::holes::enrich_history_cosmetic_thread_diameters(histories, lanes);
    } else {
        crate::resolved_features::holes::
            enrich_history_cosmetic_thread_diameters_without_hole_constructions(histories, lanes);
    }
    crate::resolved_features::reference_geometry::enrich_history_reference_planes(histories, lanes);
    crate::resolved_features::reference_geometry::enrich_history_reference_points(histories, lanes);
    crate::resolved_features::reference_geometry::enrich_history_coordinate_systems(
        histories, lanes,
    );
    crate::pmi::enrich_history_parameters(histories, pmi_dimensions);
    apply_evaluated_parameters(histories);
    crate::resolved_features::reference_geometry::enrich_history_reference_axes(histories, lanes);
    crate::resolved_features::axes::enrich_history_revolution_inputs(histories, lanes);
}

/// The shared compact/generated projection block, declared once for both codec
/// directions. Applies the seven ordered projections that read, write, and
/// configuration reprojection all run against a freshly projected feature list.
/// Direction-specific operation and profile bindings (pattern inputs,
/// sweep/revolution/extrusion operations, spatial sketches, tree-node restore)
/// stay in each caller, around this block.
pub(crate) fn project_compact_and_generated(
    features: &mut [cadmpeg_ir::features::Feature],
    projection: &[FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
) {
    crate::resolved_features::projections::project_compact_body_selections(features, lanes);
    crate::resolved_features::terminations::project_compact_combine_paths(
        features, projection, lanes,
    );
    crate::resolved_features::projections::project_compact_edge_selections(
        features, projection, lanes,
    );
    crate::resolved_features::projections::project_compact_surface_selections(
        features, projection, lanes,
    );
    crate::resolved_features::projections::project_draft_operands(features, projection, lanes);
    crate::resolved_features::terminations::project_surface_sweep_profiles(
        features, projection, lanes,
    );
    crate::resolved_features::holes::project_helix_axes(features, projection, lanes);
    crate::resolved_features::component_paths::project_adjacent_extrusion_profiles(
        features, projection, lanes,
    );
}

/// Reproject configuration-local evaluated parameters and feature operations from native lanes.
pub(crate) fn project_configuration_design_states(
    ir: &mut cadmpeg_ir::CadIr,
    histories: &[FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
    pmi_dimensions: &[crate::records::PmiDimension],
    form_padding: Option<usize>,
) {
    let mut resolved_base_features = ir.model.features.clone();
    crate::resolved_features::operations::bind_extrusion_operations(
        &mut resolved_base_features,
        histories,
        lanes,
        form_padding,
    );
    crate::resolved_features::operations::bind_revolution_operations(
        &mut resolved_base_features,
        histories,
        lanes,
        form_padding,
    );
    crate::resolved_features::operations::bind_sweep_operations(
        &mut resolved_base_features,
        histories,
        lanes,
        form_padding,
    );
    let base_definitions = ir
        .model
        .features
        .iter()
        .map(|feature| (feature.id.clone(), feature.definition.clone()))
        .collect::<HashMap<_, _>>();
    for configuration in &mut ir.model.configurations {
        configuration.parameter_values.clear();
        configuration.feature_states.clear();
    }
    for (configuration_index, lane_index) in
        configuration_lane_assignments(&ir.model.configurations, lanes)
    {
        let scoped_lanes = &lanes[lane_index..=lane_index];
        let mut projection = histories.to_vec();
        // Seed PMI types before lane enrichment so dimension semantics reject
        // incompatible native scalar candidates. Reapply afterward to add PMI
        // parameters that the lane does not carry without replacing overrides.
        crate::pmi::enrich_history_parameters_with_features(
            &mut projection,
            pmi_dimensions,
            &ir.model.features,
        );
        enrich_history_parameters_semantic(&mut projection, scoped_lanes);
        crate::resolved_features::holes::
            enrich_history_cosmetic_thread_diameters_without_hole_constructions(
                &mut projection,
                scoped_lanes,
            );
        crate::pmi::enrich_history_parameters_with_features(
            &mut projection,
            pmi_dimensions,
            &ir.model.features,
        );
        ir.model.configurations[configuration_index].parameter_values =
            project_parameters(&projection)
                .into_iter()
                .filter_map(|parameter| parameter.value.map(|value| (parameter.id, value)))
                .collect();

        let mut projection = histories.to_vec();
        enrich_history_semantic(
            &mut projection,
            scoped_lanes,
            pmi_dimensions,
            HistoryEnrichment::Write,
        );
        let mut features = project_features(&projection);
        crate::resolved_features::bindings::bind_pattern_inputs(
            &mut features,
            &projection,
            scoped_lanes,
        );
        project_compact_and_generated(&mut features, &projection, scoped_lanes);
        crate::resolved_features::operations::bind_extrusion_operations(
            &mut features,
            histories,
            scoped_lanes,
            form_padding,
        );
        crate::resolved_features::operations::bind_revolution_operations(
            &mut features,
            histories,
            scoped_lanes,
            form_padding,
        );
        crate::resolved_features::operations::bind_sweep_operations(
            &mut features,
            histories,
            scoped_lanes,
            form_padding,
        );
        crate::resolved_features::operations::inherit_configuration_operations(
            &mut features,
            &resolved_base_features,
            histories,
            scoped_lanes,
            form_padding,
        );
        inherit_configuration_reference_plane_semantics(&mut features, &resolved_base_features);
        crate::resolved_features::bindings::bind_sweep_adjacent_profiles(
            &mut features,
            histories,
            scoped_lanes,
        );
        restore_configuration_tree_node_definitions(&mut features, &ir.model.features);
        ir.model.configurations[configuration_index].feature_states = features
            .into_iter()
            .map(|mut feature| {
                if let Some(base_definition) = base_definitions.get(&feature.id) {
                    if matches!(feature.definition, FeatureDefinition::Hole { .. }) {
                        // A scoped lane may author positions without repeating
                        // shared hole construction. Copy missing construction
                        // fields while preserving authored local placements.
                        let inherit_placements =
                            !crate::resolved_features::holes::hole_position_carrier_present(
                                &feature,
                                histories,
                                scoped_lanes,
                            );
                        inherit_configuration_hole_semantics(
                            &mut feature.definition,
                            base_definition,
                            inherit_placements,
                        );
                    }
                }
                (
                    feature.id,
                    cadmpeg_ir::features::ConfigurationFeatureState {
                        suppressed: feature.suppressed.unwrap_or(false),
                        dependencies: feature.dependencies,
                        outputs: feature.outputs,
                        definition: feature.definition,
                    },
                )
            })
            .collect();
    }
}

/// Project edge operands carried only by supplemental config-object lanes into
/// the matching configuration-local feature snapshots.
pub(crate) fn project_configuration_supplemental_edge_selections(
    ir: &mut cadmpeg_ir::CadIr,
    lanes: &[crate::records::FeatureInputLane],
) {
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
            configuration_index_for_slot(&ir.model.configurations, slot_index)
        else {
            continue;
        };
        let states = &ir.model.configurations[configuration_index].feature_states;
        let mut features = ir.model.features.clone();
        for feature in &mut features {
            let Some(state) = states.get(&feature.id) else {
                continue;
            };
            feature.suppressed = Some(state.suppressed);
            feature.dependencies.clone_from(&state.dependencies);
            feature.outputs.clone_from(&state.outputs);
            feature.definition.clone_from(&state.definition);
        }
        crate::resolved_features::projections::project_compact_edge_selections(
            &mut features,
            &[],
            std::slice::from_ref(lane),
        );
        let states = &mut ir.model.configurations[configuration_index].feature_states;
        for feature in features {
            let Some(state) = states.get_mut(&feature.id) else {
                continue;
            };
            state.dependencies = feature.dependencies;
            state.definition = feature.definition;
        }
    }
}

/// Resolve topology operands in configuration-local feature snapshots.
pub(crate) fn bind_configuration_topology_selections(
    ir: &mut cadmpeg_ir::CadIr,
    histories: &[FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
    face_identities: &[(String, u32, u32)],
) {
    for (configuration_index, lane_index) in
        configuration_lane_assignments(&ir.model.configurations, lanes)
    {
        let body_membership_resolved = matches!(
            ir.model.configurations[configuration_index].bodies,
            ConfigurationBodies::Resolved(_)
        );
        let scoped_lanes = &lanes[lane_index..=lane_index];
        let mut features = {
            let states = &ir.model.configurations[configuration_index].feature_states;
            ir.model
                .features
                .iter()
                .filter_map(|feature| {
                    let state = states.get(&feature.id)?;
                    let mut feature = feature.clone();
                    feature.suppressed = Some(state.suppressed);
                    feature.dependencies.clone_from(&state.dependencies);
                    feature.outputs.clone_from(&state.outputs);
                    feature.definition.clone_from(&state.definition);
                    Some(feature)
                })
                .collect::<Vec<_>>()
        };
        if body_membership_resolved {
            let topology_selection_inputs = crate::history::TopologySelectionInputs {
                bodies: &ir.model.bodies,
                faces: &ir.model.faces,
                surfaces: &ir.model.surfaces,
                edges: &ir.model.edges,
                curves: &ir.model.curves,
                lanes: scoped_lanes,
                face_identities,
            };
            crate::history::bind_topology_selections(
                &mut features,
                histories,
                &topology_selection_inputs,
            );
        }
        // A legacy offset-plane alias carries a complete support frame. That
        // frame can bind a unique planar face even when the configuration has
        // no independently established body membership.
        crate::resolved_features::projections::project_unbound_offset_plane_faces(
            &mut features,
            &ir.model.faces,
            &ir.model.surfaces,
        );
        let states = &mut ir.model.configurations[configuration_index].feature_states;
        for feature in features {
            let Some(state) = states.get_mut(&feature.id) else {
                continue;
            };
            state.definition = feature.definition;
            state.dependencies = feature.dependencies;
            state.outputs = feature.outputs;
        }
    }
}

pub(crate) fn restore_configuration_tree_node_definitions(
    features: &mut [cadmpeg_ir::features::Feature],
    base_features: &[cadmpeg_ir::features::Feature],
) {
    let base = base_features
        .iter()
        .map(|feature| (&feature.id, &feature.definition))
        .collect::<HashMap<_, _>>();
    for feature in features {
        if !matches!(feature.definition, FeatureDefinition::Native { .. }) {
            continue;
        }
        let Some(FeatureDefinition::TreeNode { role, .. }) = base.get(&feature.id).copied() else {
            continue;
        };
        feature.definition = FeatureDefinition::TreeNode {
            role: *role,
            children: Vec::new(),
            active_child: None,
        };
    }
}

/// Apply sketch ownership projection to configuration-local feature snapshots.
pub(crate) fn project_configuration_sketch_states(
    ir: &mut cadmpeg_ir::CadIr,
    histories: &[FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
    annotations: &mut cadmpeg_ir::Annotations,
) {
    for (configuration_index, lane_index) in
        configuration_lane_assignments(&ir.model.configurations, lanes)
    {
        let surfaces = configuration_surface_carriers(ir, configuration_index);
        let scoped_lanes = &lanes[lane_index..=lane_index];
        let states = &ir.model.configurations[configuration_index].feature_states;
        let mut features = ir
            .model
            .features
            .iter()
            .filter_map(|feature| {
                let state = states.get(&feature.id)?;
                let mut feature = feature.clone();
                feature.suppressed = Some(state.suppressed);
                feature.dependencies.clone_from(&state.dependencies);
                feature.outputs.clone_from(&state.outputs);
                feature.definition.clone_from(&state.definition);
                Some(feature)
            })
            .collect::<Vec<_>>();
        inherit_configuration_reference_plane_semantics(&mut features, &ir.model.features);
        let reusable_spatial_sketches = ir
            .model
            .spatial_sketches
            .iter()
            .filter(|sketch| {
                sketch.configuration.is_none()
                    || sketch.native_ref.as_deref() == Some(scoped_lanes[0].id.as_str())
                    || scoped_lanes[0]
                        .configuration
                        .as_deref()
                        .is_some_and(|configuration| {
                            sketch.configuration.as_deref() == Some(configuration)
                        })
            })
            .map(|sketch| &sketch.id)
            .collect::<HashSet<_>>();
        let base_definitions = ir
            .model
            .features
            .iter()
            .map(|feature| (feature.id.clone(), feature.definition.clone()))
            .collect::<HashMap<_, _>>();
        for feature in &mut features {
            if let FeatureDefinition::SpatialSketch { sketch } = &mut feature.definition {
                let expected = cadmpeg_ir::sketches::SpatialSketchId(feature.id.0.replacen(
                    ":model:feature#",
                    ":model:spatial-sketch#",
                    1,
                ));
                if sketch.is_none() && reusable_spatial_sketches.contains(&expected) {
                    *sketch = Some(expected);
                }
                continue;
            }
            let FeatureDefinition::Sketch { sketch } = &mut feature.definition else {
                continue;
            };
            let Some(FeatureDefinition::SpatialSketch {
                sketch: Some(base_sketch),
            }) = base_definitions.get(&feature.id)
            else {
                continue;
            };
            if sketch.is_none() && reusable_spatial_sketches.contains(base_sketch) {
                feature.definition = FeatureDefinition::SpatialSketch {
                    sketch: Some(base_sketch.clone()),
                };
            }
        }
        let mut parameters = ir.model.parameters.clone();
        for parameter in &mut parameters {
            if let Some(value) = ir.model.configurations[configuration_index]
                .parameter_values
                .get(&parameter.id)
            {
                parameter.value = Some(value.clone());
            }
        }
        crate::resolved_features::profiles::bind_sketch_profiles(
            &mut features,
            &mut ir.model.sketches,
            &mut ir.model.sketch_entities,
            &mut ir.model.sketch_constraints,
            &parameters,
            histories,
            scoped_lanes,
            annotations,
        );
        crate::resolved_features::profiles::project_compact_sketch_profiles(
            &mut features,
            &mut ir.model.sketches,
            &mut ir.model.sketch_entities,
            histories,
            scoped_lanes,
        );
        crate::resolved_features::profiles::project_marker_backed_sketches(
            &mut features,
            &mut ir.model.sketches,
            &mut ir.model.sketch_entities,
            histories,
            scoped_lanes,
        );
        crate::resolved_features::profiles::project_sketch_block_profiles(
            &mut features,
            &mut ir.model.sketches,
            &mut ir.model.sketch_entities,
            histories,
            scoped_lanes,
        );
        bind_unique_sketch_feature(&mut features, &ir.model.sketches, histories);
        crate::resolved_features::component_paths::project_dissected_sketches(
            &mut features,
            &ir.model.sketches,
            histories,
        );
        crate::resolved_features::axes::bind_profile_revolution_axes(
            &mut features,
            histories,
            scoped_lanes,
            &ir.model.sketches,
            &surfaces,
        );
        crate::resolved_features::bindings::bind_pattern_inputs(
            &mut features,
            histories,
            scoped_lanes,
        );
        crate::resolved_features::component_paths::project_adjacent_extrusion_profiles(
            &mut features,
            histories,
            scoped_lanes,
        );
        crate::resolved_features::bindings::bind_sweep_adjacent_profiles(
            &mut features,
            histories,
            scoped_lanes,
        );
        crate::resolved_features::dimensions::project_dimensioned_sketch_geometry(
            &mut ir.model.sketch_entities,
            &ir.model.sketches,
            &surfaces,
            &features,
            &parameters,
            scoped_lanes,
        );
        crate::resolved_features::dimensions::project_marker_dimensioned_circles(
            &mut ir.model.sketch_entities,
            &mut ir.model.sketches,
            &features,
            &parameters,
            scoped_lanes,
        );
        crate::resolved_features::relation_geometry::project_relation_point_geometry(
            &mut ir.model.sketch_entities,
            &ir.model.sketches,
            &features,
            scoped_lanes,
        );
        crate::resolved_features::dimensions::project_relation_point_dimensioned_circles(
            &mut ir.model.sketch_entities,
            &features,
            &parameters,
            scoped_lanes,
        );
        crate::resolved_features::relation_geometry::project_relation_solved_line_geometry(
            &mut ir.model.sketch_entities,
            &ir.model.sketches,
            &features,
            &parameters,
            scoped_lanes,
        );
        crate::resolved_features::relation_geometry::project_relation_solved_point_geometry(
            &mut ir.model.sketch_entities,
            &ir.model.sketches,
            &features,
            &parameters,
            scoped_lanes,
        );
        crate::resolved_features::relation_geometry::project_relation_bindings(
            &mut ir.model.sketch_constraints,
            &ir.model.sketches,
            &features,
            &ir.model.sketch_entities,
            &parameters,
            scoped_lanes,
        );
        crate::resolved_features::holes::project_profiled_hole_constructions(
            &mut features,
            &ir.model.sketch_entities,
            histories,
            scoped_lanes,
        );
        crate::resolved_features::holes::project_hole_position_sketches(
            &mut features,
            &ir.model.sketches,
            &ir.model.sketch_entities,
            histories,
            scoped_lanes,
        );
        crate::resolved_features::holes::project_spatial_hole_position_sketches(
            &mut features,
            &ir.model.spatial_sketches,
            &ir.model.spatial_sketch_entities,
            &surfaces,
            histories,
            scoped_lanes,
        );
        crate::resolved_features::holes::project_topological_hole_constructions(
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
        );
        crate::resolved_features::holes::project_hole_axes(
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
        );
        crate::resolved_features::relation_geometry::project_relation_bindings(
            &mut ir.model.sketch_constraints,
            &ir.model.sketches,
            &features,
            &ir.model.sketch_entities,
            &parameters,
            scoped_lanes,
        );
        for feature in features {
            let Some(state) = ir.model.configurations[configuration_index]
                .feature_states
                .get_mut(&feature.id)
            else {
                continue;
            };
            state.suppressed = feature.suppressed.unwrap_or(false);
            state.dependencies = feature.dependencies;
            state.outputs = feature.outputs;
            state.definition = feature.definition;
        }
    }
    let scoped_configuration_indices =
        configuration_lane_assignments(&ir.model.configurations, lanes)
            .into_iter()
            .map(|(configuration_index, _)| configuration_index)
            .collect::<HashSet<_>>();
    let base = ir
        .model
        .features
        .iter()
        .map(|feature| (feature.id.clone(), feature.definition.clone()))
        .collect::<HashMap<_, _>>();
    for (configuration_index, configuration) in ir.model.configurations.iter_mut().enumerate() {
        // DI-55: a valid configuration lane owns its unresolved slots. The
        // document definition is a fallback only for an unscoped snapshot.
        if scoped_configuration_indices.contains(&configuration_index) {
            continue;
        }
        for (feature_id, state) in &mut configuration.feature_states {
            if let Some(base_definition) = base.get(feature_id) {
                inherit_configuration_shared_semantics(&mut state.definition, base_definition);
                if let FeatureDefinition::DatumOffsetPlane {
                    reference: Some(DatumPlaneReference::Feature(reference)),
                    ..
                } = &state.definition
                {
                    if !state.dependencies.contains(reference) {
                        state.dependencies.push(reference.clone());
                    }
                }
            }
        }
    }
}

pub(crate) fn inherit_configuration_shared_semantics(
    definition: &mut FeatureDefinition,
    base_definition: &FeatureDefinition,
) {
    if let (
        FeatureDefinition::DatumOffsetPlane { reference, .. },
        FeatureDefinition::DatumOffsetPlane {
            reference: base_reference,
            ..
        },
    ) = (&mut *definition, base_definition)
    {
        if reference.is_none() {
            reference.clone_from(base_reference);
        } else if let (
            Some(cadmpeg_ir::features::DatumPlaneReference::Face { face, .. }),
            Some(cadmpeg_ir::features::DatumPlaneReference::Face {
                face: base_face, ..
            }),
        ) = (reference, base_reference)
        {
            let incomplete = match face {
                cadmpeg_ir::features::FaceSelection::Faces(faces)
                | cadmpeg_ir::features::FaceSelection::Resolved { faces, .. } => faces.is_empty(),
                cadmpeg_ir::features::FaceSelection::Historical { faces, .. } => faces.is_empty(),
                cadmpeg_ir::features::FaceSelection::Generated { faces, .. } => faces.is_empty(),
                cadmpeg_ir::features::FaceSelection::HistoricalPartial {
                    faces,
                    unresolved,
                    ..
                } => faces.is_empty() || !unresolved.is_empty(),
                cadmpeg_ir::features::FaceSelection::Unresolved
                | cadmpeg_ir::features::FaceSelection::Native(_) => true,
            };
            if incomplete {
                face.clone_from(base_face);
            }
        }
        return;
    }
    inherit_configuration_hole_semantics(definition, base_definition, true);
}

pub(crate) fn inherit_configuration_hole_semantics(
    definition: &mut FeatureDefinition,
    base_definition: &FeatureDefinition,
    inherit_placements: bool,
) {
    let FeatureDefinition::Hole {
        profile,
        profile_filter,
        face,
        placements,
        kind,
        exit_kind,
        diameter,
        extent,
        bottom,
        taper_angle,
        specification,
        allow_multi_profile_faces,
    } = definition
    else {
        return;
    };
    let FeatureDefinition::Hole {
        profile: base_profile,
        profile_filter: base_profile_filter,
        face: base_face,
        placements: base_placements,
        kind: base_kind,
        exit_kind: base_exit_kind,
        diameter: base_diameter,
        extent: base_extent,
        bottom: base_bottom,
        taper_angle: base_taper_angle,
        specification: base_specification,
        allow_multi_profile_faces: base_allow_multi_profile_faces,
    } = base_definition
    else {
        return;
    };
    let missing_construction = diameter.is_none() && extent.is_none();
    let missing_face = face
        .as_ref()
        .is_none_or(|face| !complete_configuration_face_selection(face));
    if missing_face {
        face.clone_from(base_face);
    }
    if profile.is_none() {
        profile.clone_from(base_profile);
    }
    if profile_filter.is_none() {
        profile_filter.clone_from(base_profile_filter);
    }
    if inherit_placements && placements.is_none() {
        placements.clone_from(base_placements);
    }
    if missing_construction || kind.is_unresolved() {
        kind.clone_from(base_kind);
    }
    if exit_kind.is_none() || exit_kind.is_some_and(|kind| kind.is_unresolved()) {
        exit_kind.clone_from(base_exit_kind);
    }
    if diameter.is_none() {
        diameter.clone_from(base_diameter);
    }
    if extent
        .as_ref()
        .is_none_or(|extent| matches!(extent, LinearTermination::Unresolved))
    {
        extent.clone_from(base_extent);
    }
    if bottom.is_none() {
        bottom.clone_from(base_bottom);
    }
    if taper_angle.is_none() {
        taper_angle.clone_from(base_taper_angle);
    }
    if specification.is_none() {
        specification.clone_from(base_specification);
    }
    if allow_multi_profile_faces.is_none() {
        allow_multi_profile_faces.clone_from(base_allow_multi_profile_faces);
    }
}

type ConfigurationPlaneFrame = (Point3, Vector3, Vector3);

const CONFIGURATION_PLANE_FRAME_TOLERANCE: f64 = 1.0e-8;

fn complete_configuration_face_selection(selection: &FaceSelection) -> bool {
    match selection {
        FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => !faces.is_empty(),
        FaceSelection::Historical { faces, .. } => !faces.is_empty(),
        FaceSelection::Generated { faces, .. } => !faces.is_empty(),
        FaceSelection::HistoricalPartial {
            faces, unresolved, ..
        } => !faces.is_empty() && unresolved.is_empty(),
        FaceSelection::Unresolved | FaceSelection::Native(_) => false,
    }
}

fn configuration_principal_plane_frame(
    plane: cadmpeg_ir::features::PrincipalPlane,
) -> ConfigurationPlaneFrame {
    use cadmpeg_ir::features::PrincipalPlane;

    match plane {
        PrincipalPlane::Front => (
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(0.0, 0.0, -1.0),
        ),
        PrincipalPlane::Top => (
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ),
        PrincipalPlane::Right => (
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, -1.0),
        ),
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

fn configuration_feature_plane_frame(
    feature_id: &FeatureId,
    features: &HashMap<FeatureId, &cadmpeg_ir::features::Feature>,
    visiting: &mut HashSet<FeatureId>,
) -> Option<ConfigurationPlaneFrame> {
    if !visiting.insert(feature_id.clone()) {
        return None;
    }
    let Some(feature) = features.get(feature_id) else {
        visiting.remove(feature_id);
        return None;
    };
    let frame = match &feature.definition {
        FeatureDefinition::DatumPrincipalPlane { plane } => {
            Some(configuration_principal_plane_frame(*plane))
        }
        FeatureDefinition::DatumPlane {
            origin,
            normal,
            u_axis,
        } => valid_plane_frame(*normal, *u_axis).then_some((*origin, *normal, *u_axis)),
        FeatureDefinition::DatumOffsetPlane {
            reference: Some(reference),
            distance,
        } => configuration_reference_plane_frame(reference, features, visiting).and_then(
            |(origin, normal, u_axis)| {
                let normal_length = normal.norm();
                (normal_length.is_finite()
                    && normal_length > f64::EPSILON
                    && distance.0.is_finite())
                .then_some((
                    Point3::new(
                        origin.x + normal.x * distance.0 / normal_length,
                        origin.y + normal.y * distance.0 / normal_length,
                        origin.z + normal.z * distance.0 / normal_length,
                    ),
                    normal,
                    u_axis,
                ))
            },
        ),
        _ => None,
    };
    visiting.remove(feature_id);
    frame
}

fn configuration_reference_plane_frame(
    reference: &DatumPlaneReference,
    features: &HashMap<FeatureId, &cadmpeg_ir::features::Feature>,
    visiting: &mut HashSet<FeatureId>,
) -> Option<ConfigurationPlaneFrame> {
    match reference {
        DatumPlaneReference::Feature(feature_id) => {
            configuration_feature_plane_frame(feature_id, features, visiting)
        }
        DatumPlaneReference::Face {
            face,
            origin,
            normal,
            u_axis,
        } => (complete_configuration_face_selection(face) && valid_plane_frame(*normal, *u_axis))
            .then_some((*origin, *normal, *u_axis)),
    }
}

/// Reuse a document-level datum reference when a scoped state omits the
/// reference or retains its frame with only the face selector unresolved.
pub(crate) fn inherit_configuration_reference_plane_semantics(
    features: &mut [cadmpeg_ir::features::Feature],
    base_features: &[cadmpeg_ir::features::Feature],
) {
    let base_by_id = base_features
        .iter()
        .map(|feature| (feature.id.clone(), feature))
        .collect::<HashMap<_, _>>();
    for feature in features {
        let Some(base_feature) = base_by_id.get(&feature.id) else {
            continue;
        };
        let Some(base_reference) = (match &base_feature.definition {
            FeatureDefinition::DatumOffsetPlane {
                reference: Some(reference),
                ..
            } => Some(reference),
            _ => None,
        }) else {
            continue;
        };
        let replacement = (|| {
            let state_frame = match &feature.definition {
                FeatureDefinition::DatumOffsetPlane {
                    reference: None, ..
                } => None,
                FeatureDefinition::DatumOffsetPlane {
                    reference:
                        Some(DatumPlaneReference::Face {
                            face: FaceSelection::Unresolved,
                            origin,
                            normal,
                            u_axis,
                        }),
                    ..
                } if valid_plane_frame(*normal, *u_axis) => Some((*origin, *normal, *u_axis)),
                _ => return None,
            };
            let base_frame = configuration_reference_plane_frame(
                base_reference,
                &base_by_id,
                &mut HashSet::new(),
            )?;
            if let Some(state_frame) = state_frame {
                if !configuration_plane_frame_matches(state_frame, base_frame) {
                    return None;
                }
            }
            match base_reference {
                DatumPlaneReference::Feature(_) => Some(base_reference.clone()),
                DatumPlaneReference::Face { face, .. }
                    if complete_configuration_face_selection(face) =>
                {
                    Some(base_reference.clone())
                }
                DatumPlaneReference::Face { .. } => None,
            }
        })();
        let Some(replacement) = replacement else {
            continue;
        };
        let dependency = match &replacement {
            DatumPlaneReference::Feature(reference) => Some(reference.clone()),
            DatumPlaneReference::Face { .. } => None,
        };
        let FeatureDefinition::DatumOffsetPlane { reference, .. } = &mut feature.definition else {
            continue;
        };
        *reference = Some(replacement);
        if let Some(dependency) = dependency {
            if !feature.dependencies.contains(&dependency) {
                feature.dependencies.push(dependency);
            }
        }
    }
}

/// Apply late-resolved document datum references to every configuration state.
pub(crate) fn inherit_configuration_reference_plane_states(ir: &mut cadmpeg_ir::CadIr) {
    let base_features = ir.model.features.clone();
    for configuration in &mut ir.model.configurations {
        let mut features = base_features
            .iter()
            .filter_map(|base_feature| {
                let state = configuration.feature_states.get(&base_feature.id)?;
                let mut feature = base_feature.clone();
                feature.suppressed = Some(state.suppressed);
                feature.dependencies.clone_from(&state.dependencies);
                feature.outputs.clone_from(&state.outputs);
                feature.definition.clone_from(&state.definition);
                Some(feature)
            })
            .collect::<Vec<_>>();
        inherit_configuration_reference_plane_semantics(&mut features, &base_features);
        for feature in features {
            let Some(state) = configuration.feature_states.get_mut(&feature.id) else {
                continue;
            };
            state.dependencies = feature.dependencies;
            state.definition = feature.definition;
        }
    }
}

pub(crate) fn configuration_surface_carriers(
    ir: &cadmpeg_ir::CadIr,
    configuration_index: usize,
) -> Vec<cadmpeg_ir::geometry::Surface> {
    let configuration = &ir.model.configurations[configuration_index];
    let Some(body_ids) = configuration.bodies.resolved() else {
        // An unresolved body membership record does not establish an empty
        // configuration. The neutral model is the only established geometry
        // carrier available until the source partition is resolved.
        return ir.model.surfaces.clone();
    };
    let body_ids = body_ids.iter().collect::<HashSet<_>>();
    let region_ids = ir
        .model
        .bodies
        .iter()
        .filter(|body| body_ids.contains(&body.id))
        .flat_map(|body| &body.regions)
        .collect::<HashSet<_>>();
    let shell_ids = ir
        .model
        .regions
        .iter()
        .filter(|region| region_ids.contains(&region.id))
        .flat_map(|region| &region.shells)
        .collect::<HashSet<_>>();
    let face_ids = ir
        .model
        .shells
        .iter()
        .filter(|shell| shell_ids.contains(&shell.id))
        .flat_map(|shell| &shell.faces)
        .collect::<HashSet<_>>();
    let surface_ids = ir
        .model
        .faces
        .iter()
        .filter(|face| face_ids.contains(&face.id))
        .map(|face| &face.surface)
        .collect::<HashSet<_>>();
    ir.model
        .surfaces
        .iter()
        .filter(|surface| surface_ids.contains(&surface.id))
        .cloned()
        .collect()
}

/// Give configuration-local numeric overrides the kind established by their
/// neutral parameter definition and discard incompatible native candidates.
pub(crate) fn align_configuration_parameter_kinds(ir: &mut cadmpeg_ir::CadIr) {
    let parameter_kinds = ir
        .model
        .parameters
        .iter()
        .filter_map(|parameter| Some((&parameter.id, parameter.value.as_ref()?)))
        .collect::<HashMap<_, _>>();
    for value in ir
        .model
        .configurations
        .iter_mut()
        .flat_map(|configuration| &mut configuration.parameter_values)
    {
        let (parameter, value) = value;
        let Some(canonical) = parameter_kinds.get(parameter) else {
            continue;
        };
        let aligned = match (&**canonical, &*value) {
            (ParameterValue::Length(_), ParameterValue::Integer(integer)) => {
                exact_integer_f64(*integer).map(|value| ParameterValue::Length(Length(value)))
            }
            (ParameterValue::Length(_), ParameterValue::Real(real)) if real.is_finite() => {
                Some(ParameterValue::Length(Length(*real)))
            }
            (ParameterValue::Angle(_), ParameterValue::Integer(integer)) => {
                exact_integer_f64(*integer).map(|value| ParameterValue::Angle(Angle(value)))
            }
            (ParameterValue::Angle(_), ParameterValue::Real(real)) if real.is_finite() => {
                Some(ParameterValue::Angle(Angle(*real)))
            }
            (ParameterValue::Real(_), ParameterValue::Integer(integer)) => {
                exact_integer_f64(*integer).map(ParameterValue::Real)
            }
            (ParameterValue::Integer(_), ParameterValue::Real(real)) => {
                let integer = *real as i64;
                (integer as f64 == *real).then_some(ParameterValue::Integer(integer))
            }
            // Configuration lanes can provisionally classify an untyped scalar
            // as a length. The canonical integer wins only when the values agree.
            (ParameterValue::Integer(expected), ParameterValue::Length(Length(candidate))) => {
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
        configuration.parameter_values.retain(|parameter, value| {
            let Some(canonical) = parameter_kinds.get(parameter) else {
                return true;
            };
            std::mem::discriminant(&**canonical) == std::mem::discriminant(value)
        });
    }
}

pub(crate) fn configuration_lane_assignments(
    configurations: &[DesignConfiguration],
    lanes: &[crate::records::FeatureInputLane],
) -> Vec<(usize, usize)> {
    let mut lanes_by_configuration = BTreeMap::<u32, Vec<usize>>::new();
    for (lane_index, lane) in lanes
        .iter()
        .enumerate()
        .filter(|(_, lane)| configuration_state_lane(lane))
    {
        let Some(slot_index) = lane
            .configuration
            .as_deref()
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        lanes_by_configuration
            .entry(slot_index)
            .or_default()
            .push(lane_index);
    }
    lanes_by_configuration
        .into_iter()
        .filter_map(|(slot_index, lane_indices)| {
            let [lane_index] = lane_indices.as_slice() else {
                return None;
            };
            Some((
                configuration_index_for_slot(configurations, slot_index)?,
                *lane_index,
            ))
        })
        .collect()
}

pub(crate) fn configuration_index_for_slot(
    configurations: &[DesignConfiguration],
    slot_index: u32,
) -> Option<usize> {
    let explicit_candidates = configurations
        .iter()
        .enumerate()
        .filter(|(_, configuration)| {
            configuration
                .properties
                .get("id")
                .and_then(|value| value.parse::<u32>().ok())
                == Some(slot_index)
        })
        .map(|(position, _)| position)
        .collect::<Vec<_>>();
    let candidates = if explicit_candidates.is_empty() {
        configurations
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
            .map(|(position, _)| position)
            .collect::<Vec<_>>()
    } else {
        explicit_candidates
    };
    let [configuration_index] = candidates.as_slice() else {
        return None;
    };
    Some(*configuration_index)
}

pub(crate) fn unresolved_configuration_lanes(
    configurations: &[DesignConfiguration],
    lanes: &[crate::records::FeatureInputLane],
) -> usize {
    let assigned_lanes = configuration_lane_assignments(configurations, lanes)
        .into_iter()
        .map(|(_, lane_index)| lane_index)
        .collect::<HashSet<_>>();
    let mut occurrences = HashMap::<&str, usize>::new();
    for lane in lanes
        .iter()
        .filter(|lane| configuration_state_lane(lane))
        .filter_map(|lane| lane.configuration.as_deref())
    {
        *occurrences.entry(lane).or_default() += 1;
    }
    lanes
        .iter()
        .enumerate()
        .filter(|(_, lane)| configuration_state_lane(lane))
        .filter(|(lane_index, lane)| {
            lane.configuration.as_deref().is_some_and(|slot| {
                occurrences.get(slot).copied() != Some(1) || !assigned_lanes.contains(lane_index)
            })
        })
        .count()
}

pub(crate) fn configuration_state_lane(lane: &crate::records::FeatureInputLane) -> bool {
    !crate::resolved_features::assembly::is_supplemental_config_lane(lane)
}
