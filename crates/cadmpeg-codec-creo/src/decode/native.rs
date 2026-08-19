// SPDX-License-Identifier: Apache-2.0
//! Native-arena emission layer for the `creo` namespace.

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;
use serde::Serialize;

/// Schema version stamped on the `creo` namespace whenever any arena is stored.
const CREO_NATIVE_VERSION: u32 = 1;

/// Native arena keys `build_ir` and `attach_expanded_sections` may populate.
///
/// [`store_arena`] asserts the key appears here.
const CREO_ARENAS: &[&str] = &[
    "expanded_sections",
    "legacy_integer_values",
    "legacy_objects",
    "legacy_real_values",
    "legacy_string_values",
    "legacy_type_3_values",
    "legacy_type_4_values",
    "legacy_type_5_values",
    "legacy_type_6_values",
    "legacy_type_7_values",
    "legacy_type_9_values",
    "legacy_type_11_values",
    "double_xar_tables",
    "primitive_scalar_arrays",
    "reference_lines",
    "reference_circles",
    "reference_conics",
    "reference_ellipses",
    "surface_rows",
    "nonvisible_surface_rows",
    "cross_section_surface_rows",
    "surface_contours",
    "nonvisible_surface_contours",
    "cross_section_surface_contours",
    "surface_prototypes",
    "nonvisible_surface_prototypes",
    "tabulated_cylinder_curve_replays",
    "curve_parameters",
    "nonvisible_curve_parameters",
    "fc_curve_coordinates",
    "fc05_circles",
    "fc05_cylinder_cap_pairs",
    "prototype_pcurves",
    "curve_prototype_topology",
    "curve_prototypes",
    "nonvisible_curve_prototypes",
    "cross_section_curve_prototypes",
    "curve_topology_rows",
    "nonvisible_curve_topology_rows",
    "cross_section_curve_rows",
    "loop_array_frames",
    "loop_array_records",
    "half_edges",
    "loops",
    "topological_vertices",
    "half_edge_vertex_incidence",
    "face_components",
    "surface_parameters",
    "nonvisible_surface_parameters",
    "cross_section_surface_parameters",
    "plane_local_systems",
    "cross_section_plane_local_systems",
    "plane_envelopes",
    "cross_section_plane_envelopes",
    "outline_planes",
    "positional_frame_planes",
    "cross_section_outline_planes",
    "datum_planes",
    "feature_section_transforms",
    "feature_placement_instructions",
    "pcurve_endpoints",
    "feature_definitions",
    "feature_entities",
    "feature_entity_references",
    "feature_entity_tables",
    "feature_surface_replays",
    "feature_geometry_tables",
    "feature_loop_history_entries",
    "feature_affected_ids",
    "feature_replay_affected_ids",
    "surface_merge_replay_affected_ids",
    "feature_loop_restore_directions",
    "feature_revolution_extents",
    "feature_rows",
    "depdb_recipe_rows",
    "feature_choices",
    "feature_choice_fields",
    "sketches",
    "curve_expressions",
    "feature_operation_states",
    "feature_reference_names",
    "configuration",
    "configuration_driver_tables",
];

/// Record one native-source provenance annotation.
///
/// Names the source stream `creo:{source_stream}`, tags the note at `offset`, and
/// records the transfer exactness. Shared by the model-transfer path and every
/// arena emission.
pub(super) fn annotate(
    annotations: &mut AnnotationBuilder,
    id: impl std::fmt::Display,
    source_stream: &str,
    offset: u64,
    tag: &str,
    exactness: Exactness,
) {
    let stream = annotations.stream(format!("creo:{source_stream}"));
    annotations.note(id.to_string(), stream, offset).tag(tag);
    annotations.exactness(id, exactness);
}

/// Store `records` as native arena `key`, skipping empty input.
///
/// An empty slice returns without touching the namespace, so an arena that was
/// absent for empty input stays absent — flipping it to present-but-empty would
/// be an observable change. On non-empty input the namespace schema version is
/// stamped and the records are serialized under `key`.
pub(super) fn store_arena<T: Serialize>(
    ir: &mut CadIr,
    key: &str,
    records: &[T],
) -> Result<(), CodecError> {
    debug_assert!(
        CREO_ARENAS.contains(&key),
        "native arena {key} is not registered in CREO_ARENAS"
    );
    if records.is_empty() {
        return Ok(());
    }
    let namespace = ir.native.namespace_mut("creo");
    namespace.version = CREO_NATIVE_VERSION;
    namespace.set_arena(key, records)?;
    Ok(())
}

/// Annotate each record with `annotate_each`, then store them as arena `key`.
pub(super) fn emit_arena<T, F>(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    key: &str,
    records: &[T],
    mut annotate_each: F,
) -> Result<(), CodecError>
where
    T: Serialize,
    F: FnMut(&mut AnnotationBuilder, &T),
{
    for record in records {
        annotate_each(annotations, record);
    }
    store_arena(ir, key, records)
}

/// Emit an arena whose provenance comes entirely from each record's own fields.
#[expect(clippy::too_many_arguments)]
pub(super) fn emit_uniform<T: Serialize>(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    key: &str,
    records: &[T],
    id: fn(&T) -> &str,
    stream: fn(&T) -> &str,
    offset: fn(&T) -> u64,
    tag: &str,
    exactness: Exactness,
) -> Result<(), CodecError> {
    emit_arena(ir, annotations, key, records, |annotations, record| {
        annotate(
            annotations,
            id(record),
            stream(record),
            offset(record),
            tag,
            exactness,
        );
    })
}
