// SPDX-License-Identifier: Apache-2.0
//! Semantic-writer unit tests.

pub(crate) use configuration_carriers::semantic_writer_rejects_nonfinite_analytic_carriers;
pub(crate) use configuration_carriers::semantic_writer_rejects_subds;

mod configuration_carriers;
mod flex_history;
mod helix_surfaces;
mod parameters_extrude;
mod patterns_history;
mod round_trip;
mod sketch_tessellation;
mod swobjects;
mod targets;
mod thicken_reference;
