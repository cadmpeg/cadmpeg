// SPDX-License-Identifier: Apache-2.0
//! The one depth the IR admits over an inline geometry basis.

use crate::geometry::analytic::{LineCurve, PlaneSurface};
use crate::geometry::pcurve::{LinePcurve, OffsetPcurve, PcurveGeometry, TrimmedPcurve};
use crate::geometry::{
    PlacedSurface, SolvedCurveGeometry, SolvedSurfaceGeometry, MAX_GEOMETRY_NESTING,
};
use crate::math::{Point3, Vector3};
use crate::transform::{Transform, Transform2};

/// A chain of `placements` placements over a plane leaf, or the constructor's
/// refusal at the placement that would pass the bound.
fn placed_plane(placements: usize) -> Result<SolvedSurfaceGeometry, &'static str> {
    let mut geometry = SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("a unit-axis plane"),
    );
    for _ in 0..placements {
        geometry = SolvedSurfaceGeometry::Transformed(PlacedSurface::try_new(
            Box::new(geometry),
            Transform::identity(),
        )?);
    }
    Ok(geometry)
}

fn placed_line(placements: usize) -> SolvedCurveGeometry {
    let mut geometry = SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0))
            .expect("a unit-direction line"),
    );
    for _ in 0..placements {
        geometry = SolvedCurveGeometry::Transformed {
            basis: Box::new(geometry),
            transform: Transform::identity(),
        };
    }
    geometry
}

/// A chain that uses each of the three pcurve nesting carriers in turn, so the
/// admitted depth is the count of all three together and not of one of them.
fn nested_pcurve(carriers: usize) -> PcurveGeometry {
    let mut geometry = PcurveGeometry::Line(LinePcurve::U_AXIS);
    for index in 0..carriers {
        geometry = match index % 3 {
            0 => PcurveGeometry::Transformed {
                basis: Box::new(geometry),
                transform: Transform2::identity(),
            },
            1 => PcurveGeometry::Trimmed(
                TrimmedPcurve::try_new([0.0, 1.0], true, Box::new(geometry))
                    .expect("an ordered finite trim"),
            ),
            _ => PcurveGeometry::Offset(
                OffsetPcurve::try_new(1.0, Box::new(geometry)).expect("a finite offset distance"),
            ),
        };
    }
    geometry
}

#[test]
fn a_surface_placement_chain_one_past_the_bound_is_refused() {
    assert!(placed_plane(MAX_GEOMETRY_NESTING).is_ok());
    assert_eq!(
        placed_plane(MAX_GEOMETRY_NESTING + 1),
        Err("PlacedSurface.basis nests past the admitted inline basis depth")
    );
}

#[test]
fn a_curve_placement_chain_one_past_the_bound_is_refused() {
    assert!(placed_line(MAX_GEOMETRY_NESTING).nesting_within_bound());
    assert!(!placed_line(MAX_GEOMETRY_NESTING + 1).nesting_within_bound());
}

#[test]
fn a_pcurve_nesting_chain_one_past_the_bound_is_refused() {
    assert!(nested_pcurve(MAX_GEOMETRY_NESTING).nesting_within_bound());
    assert!(!nested_pcurve(MAX_GEOMETRY_NESTING + 1).nesting_within_bound());
}

#[test]
fn a_leaf_carrier_is_within_the_bound() {
    assert!(placed_line(0).nesting_within_bound());
    assert!(nested_pcurve(0).nesting_within_bound());
}

#[test]
fn the_json_parser_refuses_a_chain_well_under_the_bound() {
    // The text route into the IR is `CadIr::from_json`, and serde_json's own
    // parser gives up at 128 nested structures. A chain deep enough to matter
    // therefore cannot arrive through a document at all; it arrives from a
    // decoder that builds the carrier in process.
    let deep = placed_plane(MAX_GEOMETRY_NESTING).expect("admitted nesting");
    let json = serde_json::to_string(&deep).expect("serialized");
    let error = serde_json::from_str::<SolvedSurfaceGeometry>(&json)
        .expect_err("serde_json refuses this nesting depth");
    assert!(
        error.to_string().contains("recursion limit exceeded"),
        "unexpected parser error: {error}"
    );

    let shallow_chain = placed_plane(60).expect("admitted nesting");
    let shallow = serde_json::to_string(&shallow_chain).expect("serialized");
    assert_eq!(
        serde_json::from_str::<SolvedSurfaceGeometry>(&shallow).expect("admitted nesting"),
        shallow_chain
    );
}

#[test]
fn deserialization_refuses_a_surface_placement_past_the_bound() {
    // `serde_json::value::de` has no recursion guard, so it is the deepest
    // route a document can take into the carrier. The constructor the wire
    // routes through is what refuses the extra placement.
    let admitted =
        serde_json::to_value(placed_plane(MAX_GEOMETRY_NESTING).expect("admitted nesting"))
            .expect("serialized");
    let past_the_bound = serde_json::json!({
        "kind": "transformed",
        "basis": admitted,
        "transform": serde_json::to_value(Transform::identity()).expect("serialized"),
    });
    let error = serde_json::from_value::<SolvedSurfaceGeometry>(past_the_bound)
        .expect_err("one placement past the bound");
    assert_eq!(
        error.to_string(),
        "PlacedSurface.basis nests past the admitted inline basis depth"
    );
}
