// SPDX-License-Identifier: Apache-2.0
mod analytic;
mod nurbs;
mod parameters;
mod pcurves;
mod replicas;
mod trims;
mod units;

pub(crate) use analytic::procedural_step_geometry_round_trips_as_native_entities;
pub(crate) use units::{
    decode_conical_apex_and_context_plane_angle_units,
    decode_resolves_conversion_units_and_linear_uncertainty,
    decode_transfers_placed_analytic_geometry_in_millimetres,
};
