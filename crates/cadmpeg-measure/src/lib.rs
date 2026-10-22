// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Geometric measurement of decoded cadmpeg models.
//!
//! [`Measurer`] reads a [`cadmpeg_ir::CadIr`] and measures its bodies, faces
//! and edges in model space: carrier parameters, trimmed areas, boundary
//! lengths, boxes and orientation readings. Every value is measured within
//! its reported error under its stated method, or unavailable with a pinned
//! reason and a closed category (see [`reason`]).
//!
//! Measurement is an oracle on decoded output. A decoder or encoder never
//! selects a layout, carrier, sense, unit or interpretation by measuring
//! geometry, so no codec crate depends on this crate outside its tests.
//!
//! ```
//! use cadmpeg_measure::{MeasureInput, MeasureOptions, Measurer, Selection};
//!
//! let cube = cadmpeg_ir::examples::unit_cube().expect("fixture");
//! let arena = cadmpeg_core::decode::DecodeArena::new();
//! let policy = cadmpeg_core::decode::DecodePolicy::service();
//! let ctx = cadmpeg_core::decode::DecodeContext::for_loaded_input(&arena, &policy, 0)
//!     .expect("context");
//! let input = MeasureInput { ir: &cube, fidelity: None, geometry_transferred: true };
//! let report = Measurer::new(input, MeasureOptions::default())
//!     .measure(&ctx, &Selection::default())
//!     .expect("measured");
//! let area = report.bodies()[0].area().as_ref().and_then(|area| area.measured());
//! assert!((area.expect("area").value().value.get() - 600.0).abs() < 1.0e-9);
//! ctx.finish_session().expect("session");
//! ```

mod area;
mod body;
mod boundary;
mod budget;
mod canonical;
mod context;
mod edges;
mod extent;
mod face;
mod failure;
mod geom;
mod length;
mod orientation;
mod owners;
mod quadrature;
pub mod reason;
mod region;
pub mod report;
mod select;
mod session;
mod tolerance;
mod traversal;
pub mod value;

mod bbox;

pub use budget::{
    WorkProfile, MAX_MEASURE_PASS_WORK_PER_ITEM, MAX_MEASURE_PASS_WORK_PER_ITEM_SERVICE,
    MEASURE_PASS_WORK_FLOOR,
};
pub use select::{FaceFilter, RadiusRange, Selection, SurfaceKind};
pub use session::{MeasureError, MeasureInput, MeasureOptions, Measurer};

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod test_support;
