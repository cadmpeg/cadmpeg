// SPDX-License-Identifier: Apache-2.0
//! The chain rules on profiles that pick the curves of a sketch.

use std::collections::BTreeMap;
use std::f64::consts::FRAC_PI_2;

use super::super::super::fixture::{self, id};
use super::{bend, positive, ruled, sheet, with};
use crate::design::evaluation::EvaluationStatus;
use crate::design::feature::FeatureOperation;
use crate::design::ops::sheet_metal::SheetMetalKind;
use crate::design::ops::sketch::{
    BoundedCurve, EntityDefinition, PlanarCurve, Sketch, SketchEntity, SketchPlane, SketchSolve,
    SpatialCurve,
};
use crate::design::ops::swept::Revolve;
use crate::design::options::{BodyForm, CornerRound, SolidOutput, Thickness, ThinWall, WallSide};
use crate::design::selection::{
    Anchor, BoundaryUse, MemberOrder, Members, OrderedSelection, PickTarget, Picked, Picks,
    Selection, SketchRegion,
};
use crate::document::CadIr;
use crate::features::geometry::FinitePoint3;
use crate::geometry::analytic::LineCurve;
use crate::geometry::nurbs::NurbsCurve;
use crate::geometry::pcurve::PcurveNurbs;
use crate::geometry::InlineCurve;
use crate::math::{Point2, Point3};
use crate::scalar::{Angle, PositiveLength};
use crate::sketches::SketchEntityId;
use crate::units::{FinitePoint2, UnitVector3};

fn point(u: f64, v: f64) -> FinitePoint2 {
    FinitePoint2::new(Point2 { u, v }).unwrap()
}

fn line(start: [f64; 2], end: [f64; 2]) -> PlanarCurve {
    PlanarCurve::Line {
        start: point(start[0], start[1]),
        end: point(end[0], end[1]),
    }
}

/// The square of side 10 at the sketch origin, counterclockwise from the
/// origin.
fn square() -> Vec<PlanarCurve> {
    vec![
        line([0.0, 0.0], [10.0, 0.0]),
        line([10.0, 0.0], [10.0, 10.0]),
        line([10.0, 10.0], [0.0, 10.0]),
        line([0.0, 10.0], [0.0, 0.0]),
    ]
}

/// Gives the fixture the current sketch of the extrude that holds `curves`,
/// keyed by their place.
fn sketched(ir: &mut CadIr, curves: Vec<PlanarCurve>) -> Vec<SketchEntityId> {
    ir.design.sketches = vec![Sketch {
        id: id("sketch", "profile"),
        owner: fixture::feature("extrude"),
        name: None,
        plane: SketchPlane::Block,
        solve: SketchSolve {
            state: None,
            status: EvaluationStatus::Current,
        },
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
        native_ref: None,
    }];
    ir.design.sketch_entities = curves
        .into_iter()
        .enumerate()
        .map(|(at, curve)| SketchEntity {
            id: id("sketch-entity", &format!("c{at}")),
            sketch: id("sketch", "profile"),
            definition: EntityDefinition::Free(curve),
            roles: Vec::new(),
            construction: None,
            centerline: None,
            native: None,
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
            native_ref: None,
        })
        .collect();
    ir.design
        .sketch_entities
        .iter()
        .map(|entity| entity.id.clone())
        .collect()
}

fn selection<O: MemberOrder>(targets: Vec<PickTarget>) -> Selection<O> {
    let picks = targets
        .into_iter()
        .map(|target| Picked {
            target,
            context: None,
            native: None,
            domain: None,
            witness: None,
            resolved: None,
        })
        .collect();
    Selection {
        at: Anchor::Input,
        picks: Picks::Named(Members::new(picks).unwrap()),
        native: None,
    }
}

fn curves<O: MemberOrder>(ids: &[SketchEntityId]) -> Selection<O> {
    selection(ids.iter().cloned().map(PickTarget::SketchEntity).collect())
}

fn region<O: MemberOrder>(ids: &[SketchEntityId]) -> Selection<O> {
    let outer = ids
        .iter()
        .map(|entity| BoundaryUse {
            entity: entity.clone(),
            fragment: None,
        })
        .collect();
    selection(vec![PickTarget::SketchRegion {
        sketch: id("sketch", "profile"),
        region: SketchRegion {
            outer,
            holes: Vec::new(),
        },
    }])
}

fn revolve(
    profile: OrderedSelection,
    side: WallSide,
    corners: Option<CornerRound>,
) -> FeatureOperation {
    let wall = ThinWall {
        thickness: Thickness::OneSide {
            value: positive(1.0),
            side: Some(side),
        },
        ends: None,
        corner_round: corners,
    };
    FeatureOperation::Revolve(Box::new(Revolve {
        profile,
        loops: None,
        axis: None,
        span: None,
        fit: None,
        output: SolidOutput {
            body: Some(BodyForm::Thin(wall)),
            boolean: None,
        },
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }))
}

#[test]
fn a_thin_wall_side_and_corners_fit_whether_its_sketch_profile_closes() {
    let side = "design.wall-side-profile-closure";
    let closed = "design.corner-round-closed-profile";
    let lines = "design.round-corner-profile-not-lines";
    let mut ir = fixture::document();
    let around = sketched(&mut ir, square());
    with(&mut ir, |_| {
        revolve(curves(&around), WallSide::Inside, None)
    });
    assert!(ruled(&ir, side).is_empty());
    with(&mut ir, |_| revolve(curves(&around), WallSide::Left, None));
    assert_eq!(ruled(&ir, side).len(), 1);
    with(&mut ir, |_| revolve(region(&around), WallSide::Left, None));
    assert_eq!(ruled(&ir, side).len(), 1);
    with(&mut ir, |_| {
        revolve(curves(&around), WallSide::Inside, Some(CornerRound::Sharp))
    });
    assert_eq!(ruled(&ir, closed).len(), 1);
    let three = &around[..3];
    with(&mut ir, |_| revolve(curves(three), WallSide::Outside, None));
    assert_eq!(ruled(&ir, side).len(), 1);
    let round = || Some(CornerRound::Round(positive(1.0)));
    with(&mut ir, |_| revolve(curves(three), WallSide::Left, round()));
    assert!(ruled(&ir, side).is_empty());
    assert!(ruled(&ir, closed).is_empty());
    assert!(ruled(&ir, lines).is_empty());
    let mut bent = square();
    bent[1] = PlanarCurve::Arc {
        center: point(10.0, 5.0),
        radius: PositiveLength::new(5.0).unwrap(),
        start: Angle::new(-FRAC_PI_2).unwrap(),
        end: Angle::new(FRAC_PI_2).unwrap(),
    };
    let bent = sketched(&mut ir, bent);
    with(&mut ir, |_| {
        revolve(curves(&bent[..3]), WallSide::Left, round())
    });
    assert!(ruled(&ir, side).is_empty());
    assert_eq!(ruled(&ir, lines).len(), 1);
}

#[test]
fn a_contour_flange_sketch_profile_is_one_open_chain() {
    let contour = |profile| {
        sheet(SheetMetalKind::ContourFlange {
            profile,
            edges: None,
            width: None,
            extent: None,
            thickness: None,
            bend: bend(),
            vertex_bends: Vec::new(),
            rule: None,
        })
    };
    let open = "design.contour-flange-profile-not-open-chain";
    let mut ir = fixture::document();
    let around = sketched(&mut ir, square());
    with(&mut ir, |_| contour(curves(&around[..3])));
    assert!(ruled(&ir, open).is_empty());
    with(&mut ir, |_| contour(curves(&around)));
    assert_eq!(ruled(&ir, open).len(), 1);
    let apart = sketched(
        &mut ir,
        vec![line([0.0, 0.0], [1.0, 0.0]), line([5.0, 0.0], [6.0, 0.0])],
    );
    with(&mut ir, |_| contour(curves(&apart)));
    assert_eq!(ruled(&ir, open).len(), 1);
}

/// The clamped quadratic NURBS curve through three control points.
fn quadratic<P>(
    points: [P; 3],
    build: impl FnOnce(Vec<f64>, Vec<P>) -> PlanarCurve,
) -> PlanarCurve {
    build(vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], points.into())
}

fn spline(points: [Point2; 3]) -> PlanarCurve {
    quadratic(points, |knots, points| {
        PlanarCurve::Nurbs(PcurveNurbs::from_lanes(2, knots, points, None, false).unwrap())
    })
}

fn spatial(carrier: InlineCurve) -> PlanarCurve {
    PlanarCurve::Spatial(SpatialCurve::Bounded(BoundedCurve {
        carrier,
        range: None,
    }))
}

fn spatial_spline(points: [Point3; 3]) -> PlanarCurve {
    quadratic(points, |knots, points| {
        spatial(InlineCurve::Nurbs(
            NurbsCurve::from_lanes(2, knots, points, None, false).unwrap(),
        ))
    })
}

#[test]
fn round_corners_judge_spline_and_spatial_sketch_curves() {
    let lines = "design.round-corner-profile-not-lines";
    let round = || Some(CornerRound::Round(positive(1.0)));
    let found = |curve: PlanarCurve| {
        let mut ir = fixture::document();
        let picked = sketched(&mut ir, vec![curve]);
        with(&mut ir, |_| {
            revolve(curves(&picked), WallSide::Left, round())
        });
        ruled(&ir, lines)
    };
    let p2 = |u, v| Point2 { u, v };
    assert!(found(spline([p2(0.0, 0.0), p2(5.0, 0.0), p2(10.0, 0.0)])).is_empty());
    let bent = found(spline([p2(0.0, 0.0), p2(5.0, 5.0), p2(10.0, 0.0)]));
    assert_eq!(bent.len(), 1);
    assert!(bent[0].contains("picks curve "), "{bent:?}");
    let p3 = |x, y| Point3::new(x, y, 0.0);
    assert!(found(spatial_spline([p3(0.0, 0.0), p3(5.0, 0.0), p3(10.0, 0.0)])).is_empty());
    assert_eq!(
        found(spatial_spline([p3(0.0, 0.0), p3(5.0, 5.0), p3(10.0, 0.0)])).len(),
        1
    );
    let axis = LineCurve::new(FinitePoint3::ZERO, UnitVector3::X_AXIS);
    assert!(found(spatial(InlineCurve::Line(axis))).is_empty());
}
