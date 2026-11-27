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
//! geometry. The source policy admits one production use in a codec: the
//! Fusion writer may call [`body_boxes`] for the boxes it writes.
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

mod aggregate;
mod area;
mod body;
mod boundary;
mod budget;
mod canonical;
pub mod compare;
mod context;
mod declared;
mod edges;
mod eigen;
mod extent;
mod face;
mod failure;
mod flux;
mod geom;
mod length;
mod loss;
mod occurrence;
mod orientation;
mod owners;
mod quadrature;
pub mod reason;
mod region;
pub mod report;
mod select;
mod session;
mod surface;
mod tolerance;
mod traversal;
pub mod value;
mod witness;

mod bbox;

pub use budget::{
    WorkProfile, MAX_MEASURE_PASS_WORK_PER_ITEM, MAX_MEASURE_PASS_WORK_PER_ITEM_SERVICE,
    MEASURE_PASS_WORK_FLOOR,
};

pub(crate) type FastMap<K, V> = std::collections::HashMap<K, V, cadmpeg_ir::index::FastBuildHasher>;

#[inline]
pub(crate) fn fast_map<K, V>() -> FastMap<K, V> {
    std::collections::HashMap::with_hasher(cadmpeg_ir::index::FastBuildHasher::default())
}

#[inline]
pub(crate) fn fast_map_with_capacity<K, V>(capacity: usize) -> FastMap<K, V> {
    std::collections::HashMap::with_capacity_and_hasher(
        capacity,
        cadmpeg_ir::index::FastBuildHasher::default(),
    )
}
pub use declared::declared_findings;
pub use select::{FaceFilter, RadiusRange, Selection, SurfaceKind};
pub use session::{
    body_boxes, check_findings, findings, Density, MeasureError, MeasureInput, MeasureOptions,
    Measurer,
};

#[cfg(test)]
mod integration_tests;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
