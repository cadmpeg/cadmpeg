// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic byte-fixture builders for crate tests.

#![allow(clippy::unwrap_used)]

use std::io::Write;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder, TargetRequest};
use cadmpeg_ir::{CadIr, SourceFidelity, WritePath};

use crate::SldprtCodec;

/// Plans an inherited write through the sealed encoder and writes its bytes.
pub(crate) fn plan_inherited_write(
    ir: &CadIr,
    fidelity: &SourceFidelity,
    writer: &mut dyn Write,
) -> Result<WritePath, CodecError> {
    let plan = SldprtCodec.plan(EncodeInput::new(ir, Some(fidelity)), TargetRequest::Inherit)?;
    Ok(plan.write_to(writer)?.write_path().clone())
}

mod appearance;
mod container;
mod history;
mod ir;
mod native;
mod parasolid;
mod pmi;
mod tessellation;

pub(crate) use appearance::{material_payload, sldprt_with_body_and_material};
pub(crate) use container::{
    add_solidworks_version, make_block, make_cache_cell, make_directory_entry, outer_header,
    sldprt_with_body, sldprt_with_body_and_envelope, sldprt_with_colliding_sites,
    sldprt_with_partition_and_deltas, synthetic_sldprt, zlib,
};
pub(crate) use history::{
    resolved_feature_classes_with_ids, resolved_features_payload,
    resolved_features_payload_with_names, resolved_features_payload_with_names_relation_and_scalar,
    sldprt_with_body_and_history, sldprt_with_body_and_resolved_features,
    sldprt_with_compact_relation_pair, sldprt_with_compressed_nested_sketch_profile,
    sldprt_with_nested_arc_sketch, sldprt_with_nested_circular_sketch,
    sldprt_with_nested_elliptical_sketch, sldprt_with_nested_nurbs_sketches,
    sldprt_with_nested_sketch_profile, sldprt_with_nested_sketch_profiles,
    sldprt_with_tagged_compact_relation, sldprt_with_tagged_compact_relation_names,
    sldprt_with_tagged_compact_relation_scalar,
};
pub(crate) use ir::{
    encode_decode, encode_decode_result, sorted_point_positions, source_less_cube, strict_options,
    translate_model, translate_model_x,
};
pub(crate) use native::{sldprt_native, update_sldprt_native};
pub(crate) use parasolid::{
    arc_sketch_body, be16, be32, bef64, blend_triangle_body, bounded_curve_wrapper, bridge,
    bridge_owned, circle_carrier, circular_sketch_body, closed_cylinder_body, coedge,
    compact_counted_nurbs_surface_carrier, compact_f64_array, cone_carrier, count_entity51_family,
    cylinder_carrier, edge_use, ellipse_carrier, entity51, entity53_color, f64_array,
    face_color_definition, line_carrier, linear_nurbs_curve_carrier, loop_head,
    markerless_nurbs_surface_carrier, nurbs_curve_carrier, nurbs_sketch_body,
    nurbs_surface_carrier, nurbs_surface_carrier_with_terminal_knot_slot,
    nurbs_surface_carrier_with_v_knot_storage, offset_surface_carrier, owned_triangle,
    owned_triangle_with_kind, parasolid_payload, parasolid_with_body, plane_carrier,
    prefixed_edge_triangle_body, rational_linear_nurbs_curve_carrier,
    rational_nurbs_surface_carrier, sphere_existing_seam_body, sphere_patch_body,
    suffix_prefixed_edge_triangle_body, torus_carrier, triangle_body,
    triangle_body_with_overlapping_point, tripled_triangle_body, typed_nurbs_curve_carrier,
    u16_array, untyped_triangle, vertex_use, world_point, DIRTY_TERMINAL_KNOT,
    FACE_COLOR_DEFINITION_ID,
};
pub(crate) use pmi::{
    pmi_semantic_payload, pmi_semantic_payload_for, pmi_semantic_payload_for_with_guid,
    pmi_semantic_payload_for_with_guid_and_value, pmi_semantic_payload_record,
    pmi_semantic_payload_record_configured, pmi_semantic_payload_record_with_items,
    PmiPayloadOptions,
};
pub(crate) use tessellation::{
    display_list_payload, extended_display_list_payload, sldprt_with_body_and_display_list,
};
