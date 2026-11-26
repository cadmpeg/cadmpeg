use crate::features::evaluation::{
    HistoricalEdgeCarrier, HistoricalEdgeCurve, HistoricalEdgeGeometry,
};
use crate::geometry::nurbs::NurbsCurve;
use crate::geometry::{
    CompositeCurveSegment, CompositeCurveSegments, CompositeCurveTransition, SolvedCurveGeometry,
};
use crate::ids::{CurveId, HistoricalEdgeId};
use crate::math::Point3;
use crate::topology::ParameterInterval;
use cadmpeg_core::serde_erase::json;

fn spline() -> NurbsCurve {
    NurbsCurve::from_lanes(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(5.0, 8.0, 0.0),
            Point3::new(10.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .unwrap()
}

fn geometry(carrier: Option<HistoricalEdgeCarrier>) -> HistoricalEdgeGeometry {
    HistoricalEdgeGeometry {
        edge: HistoricalEdgeId::mint("test:model:historical-edge#one").unwrap(),
        interior: crate::features::geometry::FinitePoint3::new(Point3::new(5.0, 4.0, 0.0)).unwrap(),
        tangent: None,
        start: None,
        end: None,
        faces: crate::features::selections::DistinctMembers::default(),
        carrier,
        supports: Vec::new(),
    }
}

#[test]
fn a_historical_edge_carrier_round_trips_through_json() {
    let edge = geometry(Some(HistoricalEdgeCarrier {
        curve: HistoricalEdgeCurve::Nurbs(spline()),
        range: ParameterInterval::new([0.25, 1.0]).unwrap(),
    }));
    let json = json::to_value(&edge).unwrap();
    assert_eq!(json["carrier"]["curve"]["kind"], "nurbs");
    assert_eq!(
        json::from_value::<HistoricalEdgeGeometry>(json).unwrap(),
        edge
    );

    let bare = json::to_value(geometry(None)).unwrap();
    assert!(bare.get("carrier").is_none());
    assert_eq!(
        json::from_value::<HistoricalEdgeGeometry>(bare).unwrap(),
        geometry(None)
    );
}

#[test]
fn a_carrier_with_a_reversed_range_or_a_referencing_curve_is_refused_on_wire() {
    let mut json = json::to_value(geometry(Some(HistoricalEdgeCarrier {
        curve: HistoricalEdgeCurve::Nurbs(spline()),
        range: ParameterInterval::new([0.0, 1.0]).unwrap(),
    })))
    .unwrap();
    json["carrier"]["range"] = serde_json::json!([1.0, 0.0]);
    assert!(json::from_value::<HistoricalEdgeGeometry>(json.clone()).is_err());
    json["carrier"]["range"] = serde_json::json!([0.0, 1.0]);
    json["carrier"]["curve"] = serde_json::json!({
        "kind": "composite",
        "segments": [{"curve": "test:model:curve#one", "same_sense": true, "transition": "continuous"}],
    });
    assert!(json::from_value::<HistoricalEdgeGeometry>(json).is_err());
}

#[test]
fn only_curves_without_arena_references_become_carriers() {
    let nurbs = SolvedCurveGeometry::Nurbs(spline());
    assert_eq!(
        HistoricalEdgeCurve::from_solved(&nurbs).map(|curve| curve.to_solved()),
        Some(nurbs)
    );
    let composite = SolvedCurveGeometry::Composite {
        segments: CompositeCurveSegments::try_from(vec![CompositeCurveSegment {
            curve: CurveId::mint("test:model:curve#one").unwrap(),
            same_sense: true,
            transition: CompositeCurveTransition::Continuous,
        }])
        .unwrap(),
        self_intersect: None,
    };
    assert!(HistoricalEdgeCurve::from_solved(&composite).is_none());
}
