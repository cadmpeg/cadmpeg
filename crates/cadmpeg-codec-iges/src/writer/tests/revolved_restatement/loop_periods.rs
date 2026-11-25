// SPDX-License-Identifier: Apache-2.0
//! Faces whose loops the writer moves by whole turns of a periodic parameter
//! to the period of a reference loop.

use std::f64::consts::{PI, TAU};

use cadmpeg_ir::geometry::analytic::{CircleCurve, CylinderSurface, LineCurve, TorusSurface};
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};
use cadmpeg_ir::ids::{CoedgeId, EdgeId, FaceId, LoopId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Coedge, FaceLoops, Loop, LoopBoundary, LoopRing, Sense};
use cadmpeg_ir::CadIr;

use super::{
    assert_boundary_in_domain, azimuth, boundary, id, origin, parameter_line, read_back, ring_loop,
    torus_point, z_axis, Sheet, BAND_HEIGHT, CYLINDER_RADIUS, MIRROR_STRETCH, ON_SURFACE, STRETCH,
    TORUS_MAJOR, TORUS_MINOR,
};

/// A support in its parameters: the point at `(u, v)`, the curve of constant
/// `v` in the parameter `u`, and the curve of constant `u` in the parameter
/// `v`.
struct Chart {
    point: fn(f64, f64) -> Point3,
    along_u: fn(f64) -> SolvedCurveGeometry,
    along_v: fn(f64) -> SolvedCurveGeometry,
}

/// Adds to `sheet` the loop `key` of its face around the parameter rectangle
/// `us × vs` of `chart`, counterclockwise in the parameters, or clockwise for
/// a `hole`. Each coedge carries the parameter line of its edge moved by
/// `shift`.
fn rectangle_loop(
    sheet: &mut Sheet,
    chart: &Chart,
    key: &str,
    [us, vs]: [[f64; 2]; 2],
    shift: [f64; 2],
    hole: bool,
) -> LoopId {
    let corner = |u: usize, v: usize| format!("{key}-{u}{v}");
    for (i, u) in us.into_iter().enumerate() {
        for (j, v) in vs.into_iter().enumerate() {
            sheet.vertex(&corner(i, j), (chart.point)(u, v));
        }
    }
    let sides = [
        ("bottom", [[us[0], vs[0]], [us[1], vs[0]]], [(0, 0), (1, 0)]),
        ("top", [[us[0], vs[1]], [us[1], vs[1]]], [(0, 1), (1, 1)]),
        ("left", [[us[0], vs[0]], [us[0], vs[1]]], [(0, 0), (0, 1)]),
        ("right", [[us[1], vs[0]], [us[1], vs[1]]], [(1, 0), (1, 1)]),
    ];
    for (side, [from, to], ends) in &sides {
        let (curve, range) = if from[1] == to[1] {
            ((chart.along_u)(from[1]), us)
        } else {
            ((chart.along_v)(from[0]), vs)
        };
        let ends = ends.map(|(u, v)| corner(u, v));
        sheet.edge(&format!("{key}-{side}"), curve, range, [&ends[0], &ends[1]]);
    }
    let ring = if hole {
        [
            ("left", Sense::Forward),
            ("top", Sense::Forward),
            ("right", Sense::Reversed),
            ("bottom", Sense::Reversed),
        ]
    } else {
        [
            ("bottom", Sense::Forward),
            ("right", Sense::Forward),
            ("top", Sense::Reversed),
            ("left", Sense::Reversed),
        ]
    };
    let mut coedges = Vec::new();
    for (side, sense) in ring {
        let name = format!("{key}-{side}");
        let [from, to] = sides
            .iter()
            .find(|(other, _, _)| *other == side)
            .map(|(_, ends, _)| *ends)
            .expect("a side of the rectangle");
        let [from, to] = if sense == Sense::Forward {
            [from, to]
        } else {
            [to, from]
        };
        let length = (to[0] - from[0]).hypot(to[1] - from[1]);
        let pcurve = parameter_line(
            &mut sheet.ir,
            &name,
            [from[0] + shift[0], from[1] + shift[1]],
            [(to[0] - from[0]) / length, (to[1] - from[1]) / length],
            length,
        );
        sheet.ir.geometry.coedges.push(Coedge {
            id: id(CoedgeId::mint, "coedge", &name),
            owner_loop: id(LoopId::mint, "loop", key),
            edge: id(EdgeId::mint, "edge", &name),
            radial_next: id(CoedgeId::mint, "coedge", &name),
            sense,
            pcurves: vec![pcurve],
            use_curve: None,
            native_ref: None,
        });
        coedges.push(id(CoedgeId::mint, "coedge", &name));
    }
    sheet.ir.geometry.loops.push(Loop {
        id: id(LoopId::mint, "loop", key),
        face: id(FaceId::mint, "face", "face"),
        boundary: LoopBoundary::Ring(LoopRing::new(coedges, Vec::new()).expect("a ring")),
        native_ref: None,
    });
    id(LoopId::mint, "loop", key)
}

const CYLINDER: Chart = Chart {
    point: |u, v| Point3::new(CYLINDER_RADIUS * u.cos(), CYLINDER_RADIUS * u.sin(), v),
    along_u: |v| {
        SolvedCurveGeometry::Circle(
            CircleCurve::try_new(
                Point3::new(0.0, 0.0, v),
                z_axis(),
                Vector3::new(1.0, 0.0, 0.0),
                CYLINDER_RADIUS,
            )
            .expect("a circle"),
        )
    },
    along_v: |u| {
        let foot = azimuth(u).scale(CYLINDER_RADIUS);
        SolvedCurveGeometry::Line(
            LineCurve::try_new(Point3::new(foot.x, foot.y, 0.0), z_axis()).expect("a line"),
        )
    },
};

const TORUS: Chart = Chart {
    point: torus_point,
    along_u: |v| {
        SolvedCurveGeometry::Circle(
            CircleCurve::try_new(
                Point3::new(0.0, 0.0, TORUS_MINOR * v.sin()),
                z_axis(),
                Vector3::new(1.0, 0.0, 0.0),
                TORUS_MINOR.mul_add(v.cos(), TORUS_MAJOR),
            )
            .expect("a parallel"),
        )
    },
    along_v: |u| {
        let centre = azimuth(u).scale(TORUS_MAJOR);
        SolvedCurveGeometry::Circle(
            CircleCurve::try_new(
                Point3::new(centre.x, centre.y, 0.0),
                azimuth(u).cross(z_axis()),
                azimuth(u),
                TORUS_MINOR,
            )
            .expect("a meridian"),
        )
    },
};

/// The azimuths and heights of the two holes in the cylinder band, one on
/// each side of the azimuth seam.
const BAND_HOLES: [(&str, [[f64; 2]; 2]); 2] = [
    ("near", [[0.1, 0.3], [3.0, 5.0]]),
    ("far", [[5.0, 5.2], [3.0, 5.0]]),
];

/// The band of the cylinder about `+z` between the circles at heights 0 and
/// [`BAND_HEIGHT`] with the two [`BAND_HOLES`]. The face states its loops
/// in the order of `keys`, from `lower`, `upper` and the hole names.
fn cylinder_band_with_holes(keys: [&str; 4]) -> CadIr {
    let mut sheet = Sheet::new(SolvedSurfaceGeometry::Cylinder(
        CylinderSurface::try_new(
            origin(),
            z_axis(),
            Vector3::new(1.0, 0.0, 0.0),
            CYLINDER_RADIUS,
        )
        .expect("a cylinder"),
    ));
    for (key, height) in [("lower", 0.0), ("upper", BAND_HEIGHT)] {
        sheet.vertex(key, (CYLINDER.point)(0.0, height));
        sheet.edge(key, (CYLINDER.along_u)(height), [0.0, TAU], [key, key]);
    }
    let lower = parameter_line(&mut sheet.ir, "lower", [0.0, 0.0], [1.0, 0.0], TAU);
    let upper = parameter_line(&mut sheet.ir, "upper", [TAU, BAND_HEIGHT], [-1.0, 0.0], TAU);
    let mut loops = vec![
        ring_loop(
            &mut sheet.ir,
            ["lower", "face", "lower"],
            Sense::Forward,
            lower,
        ),
        ring_loop(
            &mut sheet.ir,
            ["upper", "face", "upper"],
            Sense::Reversed,
            upper,
        ),
    ];
    for (key, rectangle) in BAND_HOLES {
        loops.push(rectangle_loop(
            &mut sheet,
            &CYLINDER,
            key,
            rectangle,
            [0.0, 0.0],
            true,
        ));
    }
    let order = keys
        .map(|key| id(LoopId::mint, "loop", key))
        .into_iter()
        .filter(|key| loops.contains(key))
        .collect::<Vec<_>>();
    assert_eq!(order.len(), loops.len());
    sheet.faced(FaceLoops::unspecified(order))
}

#[test]
fn a_band_with_holes_on_each_side_of_the_seam_keeps_each_hole_in_the_band() {
    for keys in [
        ["near", "lower", "upper", "far"],
        ["far", "near", "upper", "lower"],
    ] {
        for stretch in [STRETCH, MIRROR_STRETCH] {
            let read = read_back(cylinder_band_with_holes(keys), stretch);
            assert_eq!(read.geometry.faces.len(), 1);
            assert_eq!(read.geometry.faces[0].loops.stated().count(), 4);
            assert_boundary_in_domain(&read);
            let (points, _) = boundary(&read);
            for point in points.into_iter().map(|point| stretch.source(point)) {
                assert!(
                    (point.x.hypot(point.y) - CYLINDER_RADIUS).abs() <= ON_SURFACE,
                    "{point:?} lies off the cylinder"
                );
                let azimuth = point.y.atan2(point.x).rem_euclid(TAU);
                let on_circle = [0.0, BAND_HEIGHT]
                    .iter()
                    .any(|height| (point.z - height).abs() <= ON_SURFACE);
                let on_hole = BAND_HOLES.iter().any(|(_, [us, vs])| {
                    (us[0] - ON_SURFACE..=us[1] + ON_SURFACE).contains(&azimuth)
                        && (vs[0] - ON_SURFACE..=vs[1] + ON_SURFACE).contains(&point.z)
                });
                assert!(
                    on_circle || on_hole,
                    "{point:?} lies on no boundary of the band"
                );
            }
        }
    }
}

/// The azimuths and tube angles of the torus patch and of its hole.
const PATCH: [[f64; 2]; 2] = [[0.0, PI / 2.0], [-PI / 4.0, PI / 4.0]];
const PATCH_HOLE: [[f64; 2]; 2] = [[0.6, 0.9], [-0.1, 0.1]];

/// The [`PATCH`] of the torus about `+z` with the hole [`PATCH_HOLE`],
/// whose parameter lines lie one turn further in azimuth and in tube angle.
/// The face states the hole first, as an unclassified loop or as an inner
/// loop.
fn torus_patch_with_a_hole(classified: bool) -> CadIr {
    let mut sheet = Sheet::new(SolvedSurfaceGeometry::Torus(
        TorusSurface::try_new(
            origin(),
            z_axis(),
            Vector3::new(1.0, 0.0, 0.0),
            TORUS_MAJOR,
            TORUS_MINOR,
        )
        .expect("a torus"),
    ));
    let outer = rectangle_loop(&mut sheet, &TORUS, "outer", PATCH, [0.0, 0.0], false);
    let hole = rectangle_loop(&mut sheet, &TORUS, "hole", PATCH_HOLE, [TAU, TAU], true);
    sheet.faced(if classified {
        FaceLoops::classified(outer, vec![hole])
    } else {
        FaceLoops::unspecified(vec![hole, outer])
    })
}

#[test]
fn a_torus_hole_stated_a_turn_away_around_the_tube_moves_into_the_face() {
    for classified in [false, true] {
        for stretch in [STRETCH, MIRROR_STRETCH] {
            let read = read_back(torus_patch_with_a_hole(classified), stretch);
            assert_eq!(read.geometry.faces.len(), 1);
            assert_eq!(read.geometry.faces[0].loops.stated().count(), 2);
            assert_boundary_in_domain(&read);
            let (points, _) = boundary(&read);
            for point in points.into_iter().map(|point| stretch.source(point)) {
                let radial = point.x.hypot(point.y) - TORUS_MAJOR;
                assert!(
                    (radial.hypot(point.z) - TORUS_MINOR).abs() <= ON_SURFACE,
                    "{point:?} lies off the torus"
                );
                let [azimuths, tubes] = PATCH;
                assert!(
                    (azimuths[0] - ON_SURFACE..=azimuths[1] + ON_SURFACE)
                        .contains(&point.y.atan2(point.x))
                        && (tubes[0] - ON_SURFACE..=tubes[1] + ON_SURFACE)
                            .contains(&point.z.atan2(radial)),
                    "{point:?} lies outside the patch"
                );
            }
        }
    }
}
