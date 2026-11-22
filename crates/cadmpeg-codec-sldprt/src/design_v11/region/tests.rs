// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::f64::consts::PI;

use cadmpeg_ir::design::ops::sketch::PlanarCurve;
use cadmpeg_ir::design::selection::{BoundaryUse, SketchRegion};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::{Angle, PositiveLength};
use cadmpeg_ir::sketches::{SketchEntityId, SketchEntityUse};
use cadmpeg_ir::units::FinitePoint2;

use super::regions;

fn id(key: &str) -> SketchEntityId {
    SketchEntityId::mint(format!("synthetic:test:id#{key}")).unwrap()
}

fn at(u: f64, v: f64) -> FinitePoint2 {
    FinitePoint2::new(Point2::new(u, v)).unwrap()
}

fn line(from: [f64; 2], to: [f64; 2]) -> PlanarCurve {
    PlanarCurve::Line {
        start: at(from[0], from[1]),
        end: at(to[0], to[1]),
    }
}

fn circle(center: [f64; 2], radius: f64) -> PlanarCurve {
    PlanarCurve::Circle {
        center: at(center[0], center[1]),
        radius: PositiveLength::new(radius).unwrap(),
    }
}

/// The curves of a sketch, by key.
struct Sketch(HashMap<SketchEntityId, PlanarCurve>);

impl Sketch {
    fn new(curves: &[(&str, PlanarCurve)]) -> Self {
        Self(
            curves
                .iter()
                .map(|(key, curve)| (id(key), curve.clone()))
                .collect(),
        )
    }

    fn regions(&self, loops: &[&[(&str, bool)]]) -> Result<Vec<SketchRegion>, &'static str> {
        let loops = loops
            .iter()
            .map(|uses| {
                uses.iter()
                    .map(|(key, reversed)| SketchEntityUse {
                        entity: id(key),
                        reversed: *reversed,
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        regions(&loops, |entity| self.0.get(entity))
    }
}

fn ring(keys: &[&str]) -> Vec<BoundaryUse> {
    keys.iter()
        .map(|key| BoundaryUse {
            entity: id(key),
            fragment: None,
        })
        .collect()
}

/// A counterclockwise square of side `side` about the origin, keyed
/// `{name}0` to `{name}3`.
fn square(name: &str, side: f64) -> Vec<(String, PlanarCurve)> {
    let h = side / 2.0;
    let corners = [[-h, -h], [h, -h], [h, h], [-h, h]];
    (0..4)
        .map(|at| {
            (
                format!("{name}{at}"),
                line(corners[at], corners[(at + 1) % 4]),
            )
        })
        .collect()
}

fn sketch_of(groups: &[Vec<(String, PlanarCurve)>], more: &[(&str, PlanarCurve)]) -> Sketch {
    let mut curves = more.to_vec();
    for group in groups {
        curves.extend(
            group
                .iter()
                .map(|(key, curve)| (key.as_str(), curve.clone())),
        );
    }
    Sketch::new(&curves)
}

const OUTER: &[(&str, bool)] = &[("o0", false), ("o1", false), ("o2", false), ("o3", false)];

/// A loop inside a hole is the outer ring of a region of its own.
#[test]
fn a_loop_inside_a_hole_is_a_region_of_its_own() {
    let sketch = sketch_of(
        &[square("o", 10.0)],
        &[
            ("hole", circle([0.0, 0.0], 3.0)),
            ("island", circle([0.0, 0.0], 1.0)),
        ],
    );
    let regions = sketch
        .regions(&[OUTER, &[("hole", false)], &[("island", false)]])
        .unwrap();
    assert_eq!(
        regions,
        vec![
            SketchRegion {
                outer: ring(&["o0", "o1", "o2", "o3"]),
                holes: vec![ring(&["hole"])],
            },
            SketchRegion {
                outer: ring(&["island"]),
                holes: Vec::new(),
            },
        ]
    );
}

/// A hole ring runs clockwise: a hole stored counterclockwise is reversed.
#[test]
fn a_hole_ring_runs_clockwise() {
    let sketch = sketch_of(&[square("o", 10.0), square("h", 2.0)], &[]);
    let regions = sketch
        .regions(&[
            OUTER,
            &[("h0", false), ("h1", false), ("h2", false), ("h3", false)],
        ])
        .unwrap();
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].holes, vec![ring(&["h3", "h2", "h1", "h0"])]);
}

/// A loop of an arc and its chord takes the arc's sweep for its side: the
/// upper half disc traversed from the arc runs counterclockwise.
#[test]
fn an_arc_loop_is_oriented_by_its_sweep() {
    let arc = PlanarCurve::Arc {
        center: at(0.0, 0.0),
        radius: PositiveLength::new(1.0).unwrap(),
        start: Angle::new(0.0).unwrap(),
        end: Angle::new(PI).unwrap(),
    };
    let sketch = Sketch::new(&[("arc", arc), ("chord", line([1.0, 0.0], [-1.0, 0.0]))]);
    let forward = sketch
        .regions(&[&[("arc", false), ("chord", true)]])
        .unwrap();
    assert_eq!(forward[0].outer, ring(&["arc", "chord"]));
    let backward = sketch
        .regions(&[&[("chord", false), ("arc", true)]])
        .unwrap();
    assert_eq!(backward[0].outer, ring(&["arc", "chord"]));
}

/// A curve and its duplicate enclose nothing; crossing loops do not nest;
/// a loop whose curves do not join is open.
#[test]
fn loops_that_bound_no_region_are_refused() {
    let sketch = sketch_of(
        &[square("o", 10.0)],
        &[
            ("a", line([0.0, 0.0], [1.0, 0.0])),
            ("b", line([0.0, 0.0], [1.0, 0.0])),
            ("cross", circle([5.0, 0.0], 2.0)),
            ("far", line([20.0, 0.0], [30.0, 0.0])),
        ],
    );
    assert_eq!(
        sketch.regions(&[&[("a", false), ("b", true)]]),
        Err("a profile loop encloses no area")
    );
    assert_eq!(
        sketch.regions(&[OUTER, &[("cross", false)]]),
        Err("its profile loops cross")
    );
    assert_eq!(
        sketch.regions(&[&[("o0", false), ("far", false)]]),
        Err("a profile loop is open")
    );
    assert_eq!(
        sketch.regions(&[&[("missing", false)]]),
        Err("its profile holds a construction curve or one its sketch does not state")
    );
}
