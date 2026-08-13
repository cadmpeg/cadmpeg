// SPDX-License-Identifier: Apache-2.0
mod draft;
mod faces;
mod sheets;
mod shells;
mod wires;

pub(crate) use faces::face_outer_bound_is_canonicalized_ahead_of_inner_bounds;
pub(crate) use sheets::{
    decode_and_write_singular_vertex_loops, decode_builds_a_valid_ap203_sheet_brep,
    decode_builds_a_valid_connected_sheet_brep, reader_recovers_a_valid_solid_from_writer_output,
};
pub(crate) use shells::every_region_of_a_body_is_retained_as_a_shape_item;
