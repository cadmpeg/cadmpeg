// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic 3DM byte-fixture builders for `#[cfg(test)]` suites.
//!
//! Helpers hand-build archive bytes only; owner suites own the assertions.
#![allow(clippy::unwrap_used)]

pub(crate) mod test_archive;
pub(crate) mod test_dump;

/// A finite fixture scalar.
pub(crate) fn finite(value: f64) -> cadmpeg_ir::scalar::FiniteReal {
    cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite fixture scalar")
}

/// A finite fixture array.
pub(crate) fn finite_array<const N: usize>(
    values: [f64; N],
) -> [cadmpeg_ir::scalar::FiniteReal; N] {
    values.map(finite)
}

/// A positive fixture scalar.
pub(crate) fn positive(value: f64) -> cadmpeg_ir::scalar::PositiveReal {
    cadmpeg_ir::scalar::PositiveReal::new(value).expect("positive fixture scalar")
}

/// A positive fixture angle.
pub(crate) fn positive_angle(value: f64) -> cadmpeg_ir::scalar::PositiveAngle {
    cadmpeg_ir::scalar::PositiveAngle::new(value).expect("positive fixture angle")
}

/// A finite fixture point.
pub(crate) fn point3(coordinates: [f64; 3]) -> crate::settings::Point3 {
    crate::settings::Point3(
        cadmpeg_ir::units::FiniteVector::new(coordinates).expect("finite fixture point"),
    )
}

/// The millimetre scale a fixture states, bound through the custom unit route
/// a scanned document uses.
pub(crate) fn millimeter_scale(millimeters_per_unit: f64) -> crate::settings::MillimeterScale {
    let unit = crate::settings::UnitSystem::custom(
        millimeters_per_unit / 1000.0,
        "fixture unit".to_owned(),
    )
    .expect("test millimetre scale");
    let units = crate::settings::UnitsAndTolerances {
        unit,
        absolute_tolerance: crate::test_support::positive(0.01),
        absolute_tolerance_millimeters: cadmpeg_ir::scalar::PositiveLength::new(
            0.01 * millimeters_per_unit,
        ),
        angular_tolerance: crate::test_support::positive_angle(0.1),
        relative_tolerance: crate::test_support::positive(0.01),
        distance_display: None,
    };
    let crate::settings::UnitBinding::Millimeters(scale) =
        crate::settings::UnitBinding::from_units(Some(&units))
    else {
        panic!("a custom unit binds a millimetre scale");
    };
    scale
}

/// Plans a write at one archive version, the request the command line builds
/// for an explicit target.
///
/// The suites assert what the writer produces at a version, not how the request
/// that names it is spelled, so the spelling lives in one place. `Inherit` is a
/// different question — it resolves against the source and cannot be pinned per
/// version — and `writer/tests/targets.rs` owns it.
pub(crate) fn plan_at(
    version: crate::RhinoArchiveVersion,
    ir: &cadmpeg_ir::document::CadIr,
) -> Result<cadmpeg_ir::codec::write::ExportPlan, cadmpeg_ir::codec::write::EncodeError> {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};

    crate::RhinoCodec.plan(
        EncodeInput::new(ir, None),
        TargetRequest::Explicit(version.descriptor().id.as_str()),
    )
}

/// The start and end positions of `coedge` in its own direction.
pub(crate) fn coedge_ends(
    ir: &cadmpeg_ir::CadIr,
    coedge: &cadmpeg_ir::topology::Coedge,
) -> [cadmpeg_ir::math::Point3; 2] {
    let geometry = &ir.geometry;
    let edge = geometry
        .edges
        .iter()
        .find(|edge| edge.id == coedge.edge)
        .expect("coedge edge");
    let [start, end] = [edge.start(), edge.end()].map(|id| {
        let id = id.expect("edge vertex");
        let vertex = geometry
            .vertices
            .iter()
            .find(|vertex| vertex.id == *id)
            .expect("vertex");
        geometry
            .points
            .iter()
            .find(|point| point.id == vertex.point)
            .expect("vertex point")
            .position()
            .get()
    });
    if coedge.sense == cadmpeg_ir::topology::Sense::Reversed {
        [end, start]
    } else {
        [start, end]
    }
}

/// Twice the area of the polygon through the start of each coedge of
/// `loop_`, about `normal`: positive where the loop runs counterclockwise
/// about `normal`, and exact for a loop of straight edges.
pub(crate) fn loop_twice_area_about(
    ir: &cadmpeg_ir::CadIr,
    loop_: &cadmpeg_ir::topology::Loop,
    normal: cadmpeg_ir::math::Vector3,
) -> f64 {
    let corners = loop_
        .coedges()
        .iter()
        .map(|id| {
            let coedge = ir
                .geometry
                .coedges
                .iter()
                .find(|coedge| coedge.id == *id)
                .expect("loop coedge");
            coedge_ends(ir, coedge)[0]
        })
        .collect::<Vec<_>>();
    let origin = corners[0];
    corners
        .iter()
        .zip(corners.iter().cycle().skip(1))
        .map(|(point, next)| {
            point
                .vector_from(origin)
                .cross(next.vector_from(origin))
                .dot(normal)
        })
        .sum()
}
