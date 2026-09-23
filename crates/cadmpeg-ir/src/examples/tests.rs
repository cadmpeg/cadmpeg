// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{directed_subd_sum, unit_cube};
use crate::document::CadIr;
use crate::validate::validate_neutral;

#[test]
fn directed_subd_sum_fixture_round_trips_and_validates() {
    let ir = directed_subd_sum().unwrap();
    let report = crate::validate::validate_neutral(&ir, Vec::new());
    assert!(report.is_ok(), "{:?}", report.findings);
    let json = ir.to_canonical_json().expect("serialize fixture");
    assert_eq!(CadIr::from_json(&json).expect("parse fixture"), ir);
}

#[test]
fn directed_subd_sum_fixture_carries_the_literal_axes_and_frame() {
    use crate::geometry::analytic::{LineCurve, PlaneSurface};
    use crate::geometry::{
        CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
    };
    use crate::ids::{CurveId, SurfaceId};
    use crate::math::{Point3, Vector3};

    let ir = directed_subd_sum().unwrap();
    let origin = Point3::new(0.0, 0.0, 0.0);
    for (key, direction) in [
        (crate::identity_key!("u"), Vector3::new(1.0, 0.0, 0.0)),
        (crate::identity_key!("v"), Vector3::new(0.0, 1.0, 0.0)),
    ] {
        let id = v2_id!(CurveId, "curve", key);
        let curve = ir.model.curves.iter().find(|curve| curve.id == id).unwrap();
        let CurveGeometry::Solved(SolvedCurveGeometry::Line(line)) = &curve.geometry else {
            panic!("the directed SubD example curve is a line");
        };
        assert_eq!(
            serde_json::to_string(line).unwrap(),
            serde_json::to_string(&LineCurve::try_new(origin, direction).unwrap()).unwrap()
        );
    }
    let id = v2_id!(SurfaceId, "surface", crate::identity_key!("sum-cache"));
    let surface = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == id)
        .unwrap();
    let SurfaceGeometry::Procedural {
        cache: Some(SolvedSurfaceGeometry::Plane(plane)),
        ..
    } = &surface.geometry
    else {
        panic!("the directed SubD example cache is a plane");
    };
    assert_eq!(
        serde_json::to_string(plane).unwrap(),
        serde_json::to_string(
            &PlaneSurface::try_new(
                origin,
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0)
            )
            .unwrap()
        )
        .unwrap()
    );
}

#[cfg(feature = "schema")]
#[test]
fn directed_subd_sum_fixture_matches_schema_shape() {
    let schema = serde_json::to_value(crate::cadir_json_schema()).expect("serialize schema");
    let schema_text = schema.to_string();
    assert!(schema_text.contains("procedural_surfaces"));
    assert!(schema_text.contains("sharpness"));
    assert!(schema_text.contains("\"sum\""));
}

#[test]
fn unit_cube_has_expected_census() {
    let ir = unit_cube().expect("valid unit cube fixture");
    assert_eq!(ir.model.bodies.len(), 1);
    assert_eq!(ir.model.regions.len(), 1);
    assert_eq!(ir.model.shells.len(), 1);
    assert_eq!(ir.model.faces.len(), 6);
    assert_eq!(ir.model.loops.len(), 6);
    assert_eq!(ir.model.coedges.len(), 24);
    assert_eq!(ir.model.edges.len(), 12);
    assert_eq!(ir.model.vertices.len(), 8);
    assert_eq!(ir.model.points.len(), 8);
    assert_eq!(ir.model.surfaces.len(), 6);
    assert_eq!(ir.model.curves.len(), 12);
}

#[test]
fn unit_cube_validates_clean() {
    let ir = unit_cube().expect("valid unit cube fixture");
    let report = validate_neutral(&ir, Vec::new());
    assert!(
        report.is_ok(),
        "cube should have no error findings, got: {:?}",
        report.findings
    );
    assert_eq!(report.error_count(), 0);
    assert_eq!(report.warning_count(), 0);
    assert_eq!(report.entity_counts.get("coedges"), Some(&24));
}

#[test]
fn every_cube_edge_has_two_opposite_sense_coedges() {
    let ir = unit_cube().expect("valid unit cube fixture");
    for edge in &ir.model.edges {
        let coedges: Vec<_> = ir
            .model
            .coedges
            .iter()
            .filter(|c| c.edge == edge.id)
            .collect();
        assert_eq!(coedges.len(), 2, "edge {} should have 2 coedges", edge.id);
        assert_ne!(
            coedges[0].sense, coedges[1].sense,
            "edge {} coedges should have opposite sense",
            edge.id
        );
        // Partners point at each other.
        assert_eq!(coedges[0].radial_next, coedges[1].id);
        assert_eq!(coedges[1].radial_next, coedges[0].id);
    }
}
