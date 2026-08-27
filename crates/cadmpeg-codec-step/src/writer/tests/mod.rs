// SPDX-License-Identifier: Apache-2.0
//! STEP writer unit tests.

mod reports;
mod round_trips;
mod targets;

pub(crate) use reports::{
    rejected_step_write_detects_incomplete_datum_system,
    strict_writer_refuses_retained_opaque_step_records_atomically,
    strict_writer_rejects_before_emitting_bytes,
};
pub(crate) use round_trips::{
    analytic_conics_round_trip_through_step,
    ap242_writer_round_trips_indexed_tessellation_and_exact_body_link,
    nurbs_surface_grid_orientation_is_u_major,
    standalone_geometry_uses_general_shape_representation,
    writer_round_trips_edge_based_wire_bodies, writer_round_trips_product_body_ownership,
    writer_round_trips_rational_nurbs_pcurves, writer_round_trips_rigid_body_placements,
};
