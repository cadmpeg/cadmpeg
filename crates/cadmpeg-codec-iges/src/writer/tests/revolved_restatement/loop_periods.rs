// SPDX-License-Identifier: Apache-2.0
//! Faces whose loops the writer moves by whole turns of a periodic parameter
//! into one window of a turn.

use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, PI, TAU};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::eval::decode::curve_point_solved;
use cadmpeg_ir::geometry::analytic::{CircleCurve, CylinderSurface, LineCurve, TorusSurface};
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::{
    CompositeCurveSegment, CompositeCurveTransition, Curve, CurveGeometry, SolvedCurveGeometry,
    SolvedSurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, RegionId, ShellId, SurfaceId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::PositiveLength;
use cadmpeg_ir::topology::{
    Coedge, Face, FaceLoops, IncreasingParameterInterval, Loop, LoopBoundary, LoopBoundaryRole,
    LoopRing, Region, Sense, Shell,
};
use cadmpeg_ir::units::FiniteVector;
use cadmpeg_ir::CadIr;
use cadmpeg_measure::test_support::round_trip::measured;

use crate::entities::curve_conversion::elliptical_arc_nurbs;

use super::{
    assert_boundary_in_domain, azimuth, boundary, charged, face_boundary, id, largest_gap, origin,
    parameter_line, read_back, ring_loop, torus_point, written_under, z_axis, Sheet, Stretch,
    BAND_HEIGHT, BAND_LOW, CYLINDER_RADIUS, MIRROR_STRETCH, ON_SURFACE, STRETCH, TORUS_MAJOR,
    TORUS_MINOR,
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
    let mut sheet = cylinder_sheet();
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

fn torus_sheet() -> Sheet {
    Sheet::new(SolvedSurfaceGeometry::Torus(
        TorusSurface::try_new(
            origin(),
            z_axis(),
            Vector3::new(1.0, 0.0, 0.0),
            TORUS_MAJOR,
            TORUS_MINOR,
        )
        .expect("a torus"),
    ))
}

/// The face loops `lower`, `upper` and `hole`: the lower loop is the outer
/// loop where `classified`.
fn band_loops(lower: LoopId, upper: LoopId, hole: LoopId, classified: bool) -> FaceLoops {
    if classified {
        FaceLoops::classified(lower, vec![upper, hole])
    } else {
        FaceLoops::unspecified(vec![lower, upper, hole])
    }
}

/// The band of the torus about `+z` above the parallel at tube angle `low`
/// and below the parallel at `high`, each one closed edge in a loop of its
/// own, with the parameter rectangle `hole` cut out. The parameter lines of
/// the hole move by `shift`.
fn torus_parallel_band(
    [low, high]: [f64; 2],
    hole: [[f64; 2]; 2],
    shift: [f64; 2],
    classified: bool,
) -> CadIr {
    let mut sheet = torus_sheet();
    for (key, v) in [("lower", low), ("upper", high)] {
        sheet.vertex(key, (TORUS.point)(0.0, v));
        sheet.edge(key, (TORUS.along_u)(v), [0.0, TAU], [key, key]);
    }
    let lower = parameter_line(&mut sheet.ir, "lower", [0.0, low], [1.0, 0.0], TAU);
    let upper = parameter_line(&mut sheet.ir, "upper", [TAU, high], [-1.0, 0.0], TAU);
    let lower = ring_loop(
        &mut sheet.ir,
        ["lower", "face", "lower"],
        Sense::Forward,
        lower,
    );
    let upper = ring_loop(
        &mut sheet.ir,
        ["upper", "face", "upper"],
        Sense::Reversed,
        upper,
    );
    let hole = rectangle_loop(&mut sheet, &TORUS, "hole", hole, shift, true);
    sheet.faced(band_loops(lower, upper, hole, classified))
}

/// The band of the torus about `+z` between the meridians at azimuths `low`
/// and `high`, each one closed edge in a loop of its own, with the parameter
/// rectangle `hole` cut out.
fn torus_meridian_band([low, high]: [f64; 2], hole: [[f64; 2]; 2], classified: bool) -> CadIr {
    let mut sheet = torus_sheet();
    for (key, u) in [("lower", low), ("upper", high)] {
        sheet.vertex(key, (TORUS.point)(u, 0.0));
        sheet.edge(key, (TORUS.along_v)(u), [0.0, TAU], [key, key]);
    }
    let lower = parameter_line(&mut sheet.ir, "lower", [low, TAU], [0.0, -1.0], TAU);
    let upper = parameter_line(&mut sheet.ir, "upper", [high, 0.0], [0.0, 1.0], TAU);
    let lower = ring_loop(
        &mut sheet.ir,
        ["lower", "face", "lower"],
        Sense::Reversed,
        lower,
    );
    let upper = ring_loop(
        &mut sheet.ir,
        ["upper", "face", "upper"],
        Sense::Forward,
        upper,
    );
    let hole = rectangle_loop(&mut sheet, &TORUS, "hole", hole, [0.0, 0.0], true);
    sheet.faced(band_loops(lower, upper, hole, classified))
}

/// Assert that the face `band` builds, with its loops unclassified and
/// classified, is written and read back under each stretch with its three
/// loops in the domain of its support, and that its boundary lies on the
/// torus where `inside` holds for the azimuth and tube angle.
fn assert_torus_band(band: impl Fn(bool) -> CadIr, inside: impl Fn(f64, f64) -> bool) {
    for classified in [false, true] {
        for stretch in [STRETCH, MIRROR_STRETCH] {
            let read = read_back(band(classified), stretch);
            assert_eq!(read.geometry.faces.len(), 1);
            assert_eq!(read.geometry.faces[0].loops.stated().count(), 3);
            assert_boundary_in_domain(&read);
            let (points, _) = boundary(&read);
            for point in points.into_iter().map(|point| stretch.source(point)) {
                let radial = point.x.hypot(point.y) - TORUS_MAJOR;
                assert!(
                    (radial.hypot(point.z) - TORUS_MINOR).abs() <= ON_SURFACE,
                    "{point:?} lies off the torus"
                );
                assert!(
                    inside(point.y.atan2(point.x), point.z.atan2(radial)),
                    "{point:?} lies outside the band"
                );
            }
        }
    }
}

/// The tube angles of the parallels of the band that holds a hole near its
/// upper parallel.
const TUBE_BAND: [f64; 2] = [-1.5, 2.5];

#[test]
fn a_hole_near_the_upper_parallel_of_a_torus_band_stays_in_the_band() {
    assert_torus_band(
        |classified| {
            torus_parallel_band(TUBE_BAND, [[1.0, 1.3], [1.8, 2.1]], [0.0, 0.0], classified)
        },
        |_, tube| (TUBE_BAND[0] - ON_SURFACE..=TUBE_BAND[1] + ON_SURFACE).contains(&tube),
    );
}

#[test]
fn a_hole_across_the_azimuth_seam_of_a_torus_band_cuts_each_parallel_there() {
    assert_torus_band(
        |classified| {
            torus_parallel_band(TUBE_BAND, [[-0.1, 0.1], [0.5, 0.9]], [0.0, 0.0], classified)
        },
        |_, tube| (TUBE_BAND[0] - ON_SURFACE..=TUBE_BAND[1] + ON_SURFACE).contains(&tube),
    );
}

/// The azimuths of the meridians of the band that holds a hole near its
/// upper meridian.
const AZIMUTH_BAND: [f64; 2] = [-1.5, 2.5];

#[test]
fn a_hole_near_the_upper_meridian_of_a_torus_band_stays_in_the_band() {
    assert_torus_band(
        |classified| torus_meridian_band(AZIMUTH_BAND, [[1.8, 2.1], [0.5, 0.9]], classified),
        |azimuth, _| {
            (AZIMUTH_BAND[0] - ON_SURFACE..=AZIMUTH_BAND[1] + ON_SURFACE).contains(&azimuth)
        },
    );
}

#[test]
fn a_hole_across_the_tube_seam_of_a_torus_band_moves_into_the_band() {
    // The band runs up from `BAND_LOW` across the tube angle `π`; the hole
    // states its parameter lines one turn below its edges.
    assert_torus_band(
        |classified| {
            torus_parallel_band(
                [BAND_LOW, -BAND_LOW],
                [[1.0, 1.3], [PI - 0.15, PI + 0.15]],
                [0.0, -TAU],
                classified,
            )
        },
        |_, tube| tube.abs() >= BAND_LOW - ON_SURFACE,
    );
}

/// The heights of the second face on the cylinder in the fixtures with two
/// faces.
const PATCH_HEIGHTS: [f64; 2] = [10.0, 12.0];

fn cylinder_sheet() -> Sheet {
    Sheet::new(SolvedSurfaceGeometry::Cylinder(
        CylinderSurface::try_new(
            origin(),
            z_axis(),
            Vector3::new(1.0, 0.0, 0.0),
            CYLINDER_RADIUS,
        )
        .expect("a cylinder"),
    ))
}

/// `sheet` with the face of `loops` and, for each key and loop of `sheet` in
/// `others`, the face of that key that the loop bounds, on the same support
/// and in a sheet body, region, and shell of that key.
fn faces(mut sheet: Sheet, loops: FaceLoops, others: Vec<(&str, LoopId)>) -> CadIr {
    for (key, other) in &others {
        sheet
            .ir
            .geometry
            .loops
            .iter_mut()
            .find(|loop_| loop_.id == *other)
            .expect("the loop of another face")
            .face = id(FaceId::mint, "face", key);
    }
    let mut ir = sheet.faced(loops);
    if others.is_empty() {
        return ir;
    }
    for (key, other) in others {
        let shell = id(ShellId::mint, "shell", key);
        let region = id(RegionId::mint, "region", key);
        let body = id(BodyId::mint, "body", key);
        ir.geometry
            .shells
            .push(Shell::new(shell.clone(), region.clone()));
        ir.geometry
            .regions
            .push(Region::new(region, body.clone(), vec![shell.clone()]));
        let mut sheet_body = ir.geometry.bodies[0].clone();
        sheet_body.id = body;
        ir.geometry.bodies.push(sheet_body);
        ir.geometry.faces.push(Face {
            id: id(FaceId::mint, "face", key),
            shell,
            surface: id(SurfaceId::mint, "surface", "face"),
            sense: Sense::Forward,
            loops: FaceLoops::classified(other, Vec::new()),
            name: None,
            color: None,
            tolerance: None,
            native_ref: None,
        });
    }
    ir.finalize();
    ir
}

/// Two faces on one cylinder about `+z`: the parameter rectangle `first`,
/// and the parameter rectangle `second` with its parameter lines moved by
/// `shift`.
fn two_patches(first: [[f64; 2]; 2], second: [[f64; 2]; 2], shift: [f64; 2]) -> CadIr {
    let mut sheet = cylinder_sheet();
    let first = rectangle_loop(&mut sheet, &CYLINDER, "first", first, [0.0, 0.0], false);
    let second = rectangle_loop(&mut sheet, &CYLINDER, "second", second, shift, false);
    faces(
        sheet,
        FaceLoops::classified(first, Vec::new()),
        vec![("second", second)],
    )
}

/// Assert that each boundary point of each face of `read`, mapped back from
/// `stretch`, lies on the cylinder in one of the parameter rectangles
/// `regions`, at some whole turn of the azimuth.
fn assert_on_cylinder_regions(read: &CadIr, stretch: Stretch, regions: &[[[f64; 2]; 2]]) {
    for face in &read.geometry.faces {
        let (points, _) = face_boundary(read, face);
        assert!(!points.is_empty());
        for point in points.into_iter().map(|point| stretch.source(point)) {
            assert!(
                (point.x.hypot(point.y) - CYLINDER_RADIUS).abs() <= ON_SURFACE,
                "{point:?} lies off the cylinder"
            );
            let azimuth = point.y.atan2(point.x);
            assert!(
                regions.iter().any(|[us, vs]| {
                    [-TAU, 0.0, TAU].iter().any(|turn| {
                        (us[0] - ON_SURFACE..=us[1] + ON_SURFACE).contains(&(azimuth + turn))
                    }) && (vs[0] - ON_SURFACE..=vs[1] + ON_SURFACE).contains(&point.z)
                }),
                "{point:?} lies on no boundary of the faces"
            );
        }
    }
}

#[test]
fn a_face_stated_a_turn_from_the_faces_before_it_moves_by_that_turn() {
    let first = [[1.0, 2.0], [0.0, 2.0]];
    let second = [[1.0, 2.0], PATCH_HEIGHTS];
    for stretch in [STRETCH, MIRROR_STRETCH] {
        let read = read_back(two_patches(first, second, [TAU, 0.0]), stretch);
        assert_eq!(read.geometry.faces.len(), 2);
        assert_boundary_in_domain(&read);
        assert_on_cylinder_regions(&read, stretch, &[first, second]);
    }
}

#[test]
fn a_face_that_no_turn_holds_with_the_other_faces_is_dropped() {
    let first = [[1.0, 3.5], [0.0, 2.0]];
    for stretch in [STRETCH, MIRROR_STRETCH] {
        let (losses, read) = written_under(
            two_patches(first, [[3.0, 7.5], PATCH_HEIGHTS], [0.0, 0.0]),
            stretch,
            &[],
        );
        let omitted = charged(&losses, "iges/writer.face-omitted");
        assert_eq!(omitted.len(), 1, "{losses:#?}");
        assert!(
            omitted[0].starts_with("face test:model:face#second ")
                && omitted[0].contains("the turn of the azimuth from ")
                && omitted[0].ends_with("does not hold face test:model:face#second"),
            "{omitted:#?}"
        );
        assert!(
            charged(&losses, "iges/writer.body-omitted").is_empty(),
            "{losses:#?}"
        );
        assert_eq!(read.geometry.faces.len(), 1);
        assert_boundary_in_domain(&read);
        assert_on_cylinder_regions(&read, stretch, &[first]);
    }
}

/// The band of the cylinder about `+z` between the circles at heights 0 and
/// [`BAND_HEIGHT`], each one closed edge whose vertex and parameter line
/// start at the azimuth in `starts`, with the parameter rectangle `hole` cut
/// out. The lower loop is the outer loop where `classified`. Where `patch`
/// holds, a second face on the cylinder bounds the azimuths `[1, 2]` at
/// [`PATCH_HEIGHTS`].
fn cylinder_band(
    starts: [f64; 2],
    hole: Option<[[f64; 2]; 2]>,
    classified: bool,
    patch: bool,
) -> CadIr {
    let mut sheet = cylinder_sheet();
    for ((key, height), start) in [("lower", 0.0), ("upper", BAND_HEIGHT)]
        .into_iter()
        .zip(starts)
    {
        sheet.vertex(key, (CYLINDER.point)(start, height));
        sheet.edge(
            key,
            (CYLINDER.along_u)(height),
            [start, start + TAU],
            [key, key],
        );
    }
    let lower = parameter_line(&mut sheet.ir, "lower", [starts[0], 0.0], [1.0, 0.0], TAU);
    let upper = parameter_line(
        &mut sheet.ir,
        "upper",
        [starts[1] + TAU, BAND_HEIGHT],
        [-1.0, 0.0],
        TAU,
    );
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
    if let Some(hole) = hole {
        loops.push(rectangle_loop(
            &mut sheet,
            &CYLINDER,
            "hole",
            hole,
            [0.0, 0.0],
            true,
        ));
    }
    let patch = patch.then(|| {
        rectangle_loop(
            &mut sheet,
            &CYLINDER,
            "patch",
            [[1.0, 2.0], PATCH_HEIGHTS],
            [0.0, 0.0],
            false,
        )
    });
    let loops = if classified {
        let lower = loops.remove(0);
        FaceLoops::classified(lower, loops)
    } else {
        FaceLoops::unspecified(loops)
    };
    faces(
        sheet,
        loops,
        patch.into_iter().map(|patch| ("second", patch)).collect(),
    )
}

/// Assert that the face that `band` builds, with its loops unclassified and
/// classified, is written and read back under each stretch, alone and beside
/// a second face, with each boundary point on a circle of the band, on
/// `hole`, or on the second face, and with each circle written over its
/// whole turn.
fn assert_band_written(band: impl Fn(bool, bool) -> CadIr, hole: Option<[[f64; 2]; 2]>) {
    for (classified, patch) in [(false, false), (true, false), (false, true), (true, true)] {
        for stretch in [STRETCH, MIRROR_STRETCH] {
            let read = read_back(band(classified, patch), stretch);
            assert_eq!(read.geometry.faces.len(), 1 + usize::from(patch));
            assert_boundary_in_domain(&read);
            let mut regions = vec![
                [[0.0, TAU], [0.0, 0.0]],
                [[0.0, TAU], [BAND_HEIGHT, BAND_HEIGHT]],
                [[1.0, 2.0], PATCH_HEIGHTS],
            ];
            regions.extend(hole);
            assert_on_cylinder_regions(&read, stretch, &regions);
            let (points, _) = face_boundary(&read, &read.geometry.faces[0]);
            for height in [0.0, BAND_HEIGHT] {
                let azimuths = points
                    .iter()
                    .map(|point| stretch.source(*point))
                    .filter(|point| (point.z - height).abs() <= ON_SURFACE)
                    .map(|point| point.y.atan2(point.x))
                    .collect::<Vec<_>>();
                assert!(
                    largest_gap(azimuths) <= CIRCLE_GAP,
                    "the circle at height {height} has a gap"
                );
            }
        }
    }
}

/// The largest azimuth gap between boundary samples on a circle that the
/// writer writes over its whole turn.
const CIRCLE_GAP: f64 = PI / 4.0;

/// A hole across the azimuth `0` of a band whose circles start there.
const SEAM_HOLE: [[f64; 2]; 2] = [[-0.1, 0.1], [3.0, 5.0]];

#[test]
fn a_band_with_a_hole_across_the_seam_of_its_circles_cuts_each_circle_there() {
    assert_band_written(
        |classified, patch| cylinder_band([0.0, 0.0], Some(SEAM_HOLE), classified, patch),
        Some(SEAM_HOLE),
    );
}

#[test]
fn band_circles_that_start_at_different_azimuths_lie_in_one_turn() {
    assert_band_written(
        |classified, patch| cylinder_band([0.0, PI], None, classified, patch),
        None,
    );
}

/// A band whose circles turn once around the cylinder from one azimuth or
/// from azimuths half a turn apart reads back with loops whose winding gives
/// the orientation of the face sense.
#[test]
fn a_band_reads_back_with_loops_that_keep_the_face_on_their_left() {
    for starts in [[0.0, 0.0], [0.0, PI]] {
        let read = read_back(cylinder_band(starts, None, true, false), STRETCH);
        let report = measured("band", &read).expect("the band is measured");
        for body in report.bodies() {
            let orientation = body.orientation();
            assert_eq!(orientation.consistent(), &Some(true), "{orientation:#?}");
            assert!(orientation.inconsistent().is_empty(), "{orientation:#?}");
            assert!(orientation.mixed().is_empty(), "{orientation:#?}");
        }
    }
}

/// Faces on one cylinder about `+z`, one for each parameter rectangle of
/// `regions`, in that order.
fn patches(regions: &[[[f64; 2]; 2]]) -> CadIr {
    let mut sheet = cylinder_sheet();
    let keys = (0..regions.len())
        .map(|position| format!("patch-{position}"))
        .collect::<Vec<_>>();
    let mut loops = keys
        .iter()
        .zip(regions)
        .map(|(key, region)| rectangle_loop(&mut sheet, &CYLINDER, key, *region, [0.0, 0.0], false))
        .collect::<Vec<_>>();
    let first = loops.remove(0);
    faces(
        sheet,
        FaceLoops::classified(first, Vec::new()),
        keys[1..].iter().map(String::as_str).zip(loops).collect(),
    )
}

#[test]
fn faces_that_one_turn_holds_are_all_written() {
    // The turn from azimuth 3 holds the three faces; no turn from the first
    // face holds the third.
    let regions = [
        [[0.0, 0.5], [0.0, 1.0]],
        [[3.0, 3.5], [0.0, 1.0]],
        [[3.4, 6.5], [2.0, 3.0]],
    ];
    for stretch in [STRETCH, MIRROR_STRETCH] {
        let read = read_back(patches(&regions), stretch);
        assert_eq!(read.geometry.faces.len(), 3);
        assert_boundary_in_domain(&read);
        assert_on_cylinder_regions(&read, stretch, &regions);
    }
}

/// `band` with the geometry of the edge curve of its lower circle set to
/// `geometry`.
fn with_lower_curve(mut band: CadIr, geometry: SolvedCurveGeometry) -> CadIr {
    band.geometry
        .curves
        .iter_mut()
        .find(|curve| curve.id == id(CurveId::mint, "curve", "lower"))
        .expect("the lower circle")
        .geometry = CurveGeometry::Solved(geometry);
    band
}

/// The lower circle of the cylinder band as a rational B-spline over the
/// azimuths `[0, 2π]`.
fn lower_b_spline() -> SolvedCurveGeometry {
    let radius = PositiveLength::new(CYLINDER_RADIUS).expect("a radius");
    SolvedCurveGeometry::Nurbs(
        elliptical_arc_nurbs(
            origin(),
            z_axis(),
            Vector3::new(1.0, 0.0, 0.0),
            radius,
            radius,
            [0.0, TAU],
        )
        .expect("a rational circle")
        .expect("a rational circle over one turn"),
    )
}

#[test]
fn a_band_whose_b_spline_circle_crosses_the_seam_cut_cuts_the_b_spline_there() {
    assert_band_written(
        |classified, patch| {
            with_lower_curve(
                cylinder_band([0.0, 0.0], Some(SEAM_HOLE), classified, patch),
                lower_b_spline(),
            )
        },
        Some(SEAM_HOLE),
    );
    for stretch in [STRETCH, MIRROR_STRETCH] {
        let band = with_lower_curve(
            cylinder_band([0.0, 0.0], Some(SEAM_HOLE), true, false),
            lower_b_spline(),
        );
        let (losses, read) = written_under(band, stretch, &[]);
        assert!(
            charged(&losses, "iges/writer.loop-classification-omitted").is_empty(),
            "{losses:#?}"
        );
        let face = &read.geometry.faces[0];
        assert!(face
            .loops
            .stated()
            .any(|loop_| face.loop_role(loop_) == LoopBoundaryRole::Outer));
    }
}

/// `band` with the edge curve of its lower circle stated as a composite of
/// the circle.
fn with_composite_lower_circle(mut band: CadIr) -> CadIr {
    let circle = id(CurveId::mint, "curve", "lower-circle");
    band.geometry.curves.push(Curve {
        id: circle.clone(),
        geometry: CurveGeometry::Solved((CYLINDER.along_u)(0.0)),
        parameter_range: IncreasingParameterInterval::new([0.0, TAU]),
        source_object: None,
        native_ref: None,
    });
    let mut band = with_lower_curve(
        band,
        SolvedCurveGeometry::Composite {
            segments: vec![CompositeCurveSegment {
                curve: circle,
                same_sense: true,
                transition: CompositeCurveTransition::Continuous,
            }]
            .try_into()
            .expect("one segment"),
            self_intersect: None,
        },
    );
    band.finalize();
    band
}

#[test]
fn a_classified_band_whose_composite_edge_curve_the_parameter_search_does_not_invert_is_written_unclassified(
) {
    for stretch in [STRETCH, MIRROR_STRETCH] {
        let band =
            with_composite_lower_circle(cylinder_band([0.0, 0.0], Some(SEAM_HOLE), true, false));
        let (losses, read) = written_under(band, stretch, &[]);
        let unclassified = charged(&losses, "iges/writer.loop-classification-omitted");
        assert_eq!(unclassified.len(), 1, "{losses:#?}");
        assert!(
            unclassified[0].starts_with("IGES face test:model:face#face ")
                && unclassified[0].contains("coedge test:model:coedge#lower "),
            "{unclassified:#?}"
        );
        assert!(
            charged(&losses, "iges/writer.face-omitted").is_empty(),
            "{losses:#?}"
        );
        assert_eq!(read.geometry.faces.len(), 1);
        let face = &read.geometry.faces[0];
        assert!(face
            .loops
            .stated()
            .all(|loop_| face.loop_role(loop_) == LoopBoundaryRole::Unspecified));
        assert_eq!(face.loops.stated().count(), 3);
        assert_on_cylinder_regions(
            &read,
            stretch,
            &[
                [[0.0, TAU], [0.0, 0.0]],
                [[0.0, TAU], [BAND_HEIGHT, BAND_HEIGHT]],
                SEAM_HOLE,
            ],
        );
    }
}

/// The source position of the vertex where the traversal of the loop of
/// `read` whose edges lie on the circle of the band at `height` starts, and
/// the source azimuths along that traversal, unwrapped across the seam.
fn circle_travel(read: &CadIr, stretch: Stretch, height: f64) -> (Point3, Vec<f64>) {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
        .expect("an evaluation context");
    let geometry = &read.geometry;
    let mut loops = geometry.loops.iter().map(|loop_| {
        let mut first = None;
        let mut points = Vec::new();
        for coedge_id in loop_.coedges() {
            let coedge = geometry
                .coedges
                .iter()
                .find(|coedge| coedge.id == *coedge_id)
                .expect("the loop coedge reads back");
            let edge = geometry
                .edges
                .iter()
                .find(|edge| edge.id == coedge.edge)
                .expect("the coedge edge reads back");
            let curve = geometry
                .curves
                .iter()
                .find(|curve| Some(&curve.id) == edge.carrier.curve())
                .expect("the edge curve reads back");
            let solved = match &curve.geometry {
                CurveGeometry::Solved(solved) => solved,
                CurveGeometry::Procedural { cache, .. } => {
                    cache.as_ref().expect("the edge curve reads back solved")
                }
            };
            let [start, end] = edge
                .param_range()
                .map(FiniteVector::get)
                .expect("a bounded edge");
            let mut samples = (0..=CIRCLE_SAMPLES)
                .map(|step| {
                    let parameter =
                        (f64::from(step) / f64::from(CIRCLE_SAMPLES)).mul_add(end - start, start);
                    stretch.source(
                        curve_point_solved(&ctx, solved, parameter)
                            .expect("a point on the edge")
                            .get(),
                    )
                })
                .collect::<Vec<_>>();
            if coedge.sense == Sense::Reversed {
                samples.reverse();
            }
            let ends = edge.ends.as_ref().expect("a bounded edge");
            let vertex = if coedge.sense == Sense::Forward {
                &ends.start
            } else {
                &ends.end
            };
            first.get_or_insert_with(|| {
                let vertex = geometry
                    .vertices
                    .iter()
                    .find(|other| other.id == *vertex)
                    .expect("the edge vertex reads back");
                let point = geometry
                    .points
                    .iter()
                    .find(|point| point.id == vertex.point)
                    .expect("the vertex point reads back");
                stretch.source(point.position().get())
            });
            points.extend(samples);
        }
        (first.expect("a loop with a coedge"), points)
    });
    let (start, points) = loops
        .find(|(_, points)| {
            points
                .iter()
                .all(|point| (point.z - height).abs() <= ON_SURFACE)
        })
        .expect("a loop on the circle");
    let mut azimuths = vec![points[0].y.atan2(points[0].x)];
    for pair in points.windows(2) {
        let step =
            (pair[1].y.atan2(pair[1].x) - pair[0].y.atan2(pair[0].x) + PI).rem_euclid(TAU) - PI;
        azimuths.push(azimuths[azimuths.len() - 1] + step);
    }
    (start, azimuths)
}

/// The largest difference between the azimuth span of a read-back edge
/// curve over the range of its edge and the span between its vertices.
const READ_BACK_SPAN: f64 = 1.0e-4;

/// The samples of each edge curve of a loop in [`circle_travel`].
const CIRCLE_SAMPLES: u32 = 64;

#[test]
fn a_cut_band_circle_reads_back_from_the_cut_once_around_in_one_direction() {
    // Both circles cross the end of the turn that starts at the low side of
    // the seam hole, so the boundary of each starts at that azimuth.
    for (lower, label) in [
        (None, "circle"),
        (Some(lower_b_spline()), "b-spline"),
        (
            Some(lower_b_spline_that_comes_back()),
            "b-spline that comes back",
        ),
        (
            Some(lower_b_spline_with_a_knot_near_the_cut()),
            "b-spline with a knot near the cut",
        ),
    ] {
        for stretch in [MIRROR_STRETCH, STRETCH] {
            let band = cylinder_band([0.0, 0.0], Some(SEAM_HOLE), true, false);
            let band = match &lower {
                Some(geometry) => with_lower_curve(band, geometry.clone()),
                None => band,
            };
            let read = read_back(band, stretch);
            for height in [0.0, BAND_HEIGHT] {
                let (start, azimuths) = circle_travel(&read, stretch, height);
                let cut = start.y.atan2(start.x);
                assert!(
                    ((cut - SEAM_HOLE[0][0] + PI).rem_euclid(TAU) - PI).abs() <= ON_SURFACE
                        && (start.z - height).abs() <= ON_SURFACE,
                    "the {label} at height {height} starts at {start:?} (scale {:?})",
                    stretch.scale
                );
                let first = azimuths[0];
                let last = azimuths[azimuths.len() - 1];
                assert!(
                    ((first - cut + PI).rem_euclid(TAU) - PI).abs() <= READ_BACK_SPAN
                        && ((last - first).abs() - TAU).abs() <= READ_BACK_SPAN,
                    "the circle at height {height} runs from {first} to {last}"
                );
                let direction = (last - first).signum();
                assert!(
                    azimuths
                        .windows(2)
                        .all(|pair| (pair[1] - pair[0]) * direction >= -ON_SURFACE),
                    "the circle at height {height} turns back: {azimuths:?}"
                );
            }
        }
    }
}

/// The azimuth between the seam cut of the lower circle and the knot of
/// [`lower_b_spline_with_a_knot_near_the_cut`] before it. The knot point lies
/// inside the topology tolerance of the cut point.
const KNOT_GAP: f64 = 5.0e-4;

/// The lower circle of the cylinder band as a rational B-spline of quadratic
/// arcs whose knots are the azimuths at which the arcs meet. One knot lies
/// [`KNOT_GAP`] before the azimuth of the seam cut.
fn lower_b_spline_with_a_knot_near_the_cut() -> SolvedCurveGeometry {
    let breaks = [
        0.0,
        FRAC_PI_2,
        PI,
        3.0 * FRAC_PI_2,
        TAU + SEAM_HOLE[0][0] - KNOT_GAP,
        TAU,
    ];
    let on_circle = |azimuth: f64, radius: f64| {
        Point3::new(radius * azimuth.cos(), radius * azimuth.sin(), 0.0)
    };
    let mut poles = vec![on_circle(0.0, CYLINDER_RADIUS)];
    let mut weights = vec![1.0];
    let mut knots = vec![0.0; 3];
    for pair in breaks.windows(2) {
        let half = 0.5 * (pair[1] - pair[0]);
        poles.push(on_circle(pair[0] + half, CYLINDER_RADIUS / half.cos()));
        poles.push(on_circle(pair[1], CYLINDER_RADIUS));
        weights.extend([half.cos(), 1.0]);
        knots.extend([pair[1]; 2]);
    }
    knots.push(TAU);
    SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(2, knots, poles, Some(weights), false).expect("a rational circle"),
    )
}

/// The gap, inside the topology tolerance, between the seam cut of the lower
/// circle and the end of [`lower_b_spline_that_comes_back`].
const RETURN_GAP: f64 = 0.002;

/// The lower circle of the cylinder band as a rational B-spline over the
/// knots `[0, 2π]`, which then runs straight on over the knots `[2π, 2π + 1]`
/// to [`RETURN_GAP`] outside the circle at the azimuth of the seam cut.
fn lower_b_spline_that_comes_back() -> SolvedCurveGeometry {
    let radius = CYLINDER_RADIUS;
    let azimuth = SEAM_HOLE[0][0];
    let end = Point3::new(
        (radius + RETURN_GAP) * azimuth.cos(),
        (radius + RETURN_GAP) * azimuth.sin(),
        0.0,
    );
    let mut poles = [
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [-1.0, 1.0],
        [-1.0, 0.0],
        [-1.0, -1.0],
        [0.0, -1.0],
        [1.0, -1.0],
        [1.0, 0.0],
    ]
    .map(|[x, y]| Point3::new(radius * x, radius * y, 0.0))
    .to_vec();
    poles.push(Point3::new(0.5 * (radius + end.x), 0.5 * end.y, 0.0));
    poles.push(end);
    let mut weights = vec![1.0; poles.len()];
    for corner in [1, 3, 5, 7] {
        weights[corner] = FRAC_1_SQRT_2;
    }
    let quarter = TAU / 4.0;
    let knots = vec![
        0.0,
        0.0,
        0.0,
        quarter,
        quarter,
        2.0 * quarter,
        2.0 * quarter,
        3.0 * quarter,
        3.0 * quarter,
        TAU,
        TAU,
        TAU + 1.0,
        TAU + 1.0,
        TAU + 1.0,
    ];
    SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(2, knots, poles, Some(weights), false)
            .expect("a rational circle and a line"),
    )
}

#[test]
fn a_band_whose_b_spline_comes_back_near_the_seam_cut_cuts_the_circle_there() {
    for stretch in [STRETCH, MIRROR_STRETCH] {
        let band = with_lower_curve(
            cylinder_band([0.0, 0.0], Some(SEAM_HOLE), true, false),
            lower_b_spline_that_comes_back(),
        );
        let (losses, read) = written_under(band, stretch, &[]);
        for code in [
            "iges/writer.face-omitted",
            "iges/writer.loop-classification-omitted",
        ] {
            assert!(charged(&losses, code).is_empty(), "{losses:#?}");
        }
        let face = &read.geometry.faces[0];
        assert!(face
            .loops
            .stated()
            .any(|loop_| face.loop_role(loop_) == LoopBoundaryRole::Outer));
        assert_on_cylinder_regions(
            &read,
            stretch,
            &[
                [[0.0, TAU], [0.0, 0.0]],
                [[0.0, TAU], [BAND_HEIGHT, BAND_HEIGHT]],
                SEAM_HOLE,
            ],
        );
    }
}

/// Adds to `sheet` the loop `key` of its face on the cylinder about `+z`
/// through the parameter points `points` and back to the first one a turn
/// on, of arcs where `v` is constant and axial lines where `u` is. Each
/// coedge carries the parameter line of its edge in the direction of the
/// loop.
fn ring_through(sheet: &mut Sheet, key: &str, points: &[[f64; 2]]) -> LoopId {
    for (position, [u, v]) in points.iter().enumerate() {
        sheet.vertex(&format!("{key}-{position}"), (CYLINDER.point)(*u, *v));
    }
    let mut coedges = Vec::new();
    for position in 0..points.len() {
        let next = (position + 1) % points.len();
        let from = points[position];
        let to = if next == 0 {
            [points[0][0] + TAU, points[0][1]]
        } else {
            points[next]
        };
        let name = format!("{key}-side-{position}");
        let (curve, [from_t, to_t]) = if from[1] == to[1] {
            ((CYLINDER.along_u)(from[1]), [from[0], to[0]])
        } else {
            ((CYLINDER.along_v)(from[0]), [from[1], to[1]])
        };
        let ends = [format!("{key}-{position}"), format!("{key}-{next}")];
        let (range, ends, sense) = if from_t < to_t {
            ([from_t, to_t], [&ends[0], &ends[1]], Sense::Forward)
        } else {
            ([to_t, from_t], [&ends[1], &ends[0]], Sense::Reversed)
        };
        sheet.edge(&name, curve, range, ends.map(String::as_str));
        let length = (to[0] - from[0]).hypot(to[1] - from[1]);
        let pcurve = parameter_line(
            &mut sheet.ir,
            &name,
            from,
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

/// The band of the cylinder about `+z` between the ring through `lower` and
/// the circle at [`BAND_HEIGHT`], with the parameter rectangle `hole` cut
/// out. The ring is the outer loop where `classified`.
fn ring_band(lower: &[[f64; 2]], hole: Option<[[f64; 2]; 2]>, classified: bool) -> CadIr {
    let mut sheet = cylinder_sheet();
    let lower = ring_through(&mut sheet, "ring", lower);
    sheet.vertex("upper", (CYLINDER.point)(0.0, BAND_HEIGHT));
    sheet.edge(
        "upper",
        (CYLINDER.along_u)(BAND_HEIGHT),
        [0.0, TAU],
        ["upper", "upper"],
    );
    let upper = parameter_line(&mut sheet.ir, "upper", [TAU, BAND_HEIGHT], [-1.0, 0.0], TAU);
    let upper = ring_loop(
        &mut sheet.ir,
        ["upper", "face", "upper"],
        Sense::Reversed,
        upper,
    );
    let mut others = vec![upper];
    others.extend(
        hole.map(|hole| rectangle_loop(&mut sheet, &CYLINDER, "hole", hole, [0.0, 0.0], true)),
    );
    sheet.faced(if classified {
        FaceLoops::classified(lower, others)
    } else {
        others.insert(0, lower);
        FaceLoops::unspecified(others)
    })
}

/// A ring along the azimuths `[0, π]` at height 0 and `[π, 2π]` at height 1,
/// with an axial line at each of the azimuths `π` and `2π`.
const STEP: [[f64; 2]; 4] = [[0.0, 0.0], [PI, 0.0], [PI, 1.0], [TAU, 1.0]];

/// The ring of [`STEP`] stated from its arc at height 1.
const STEP_FROM_ITS_UPPER_ARC: [[f64; 2]; 4] = [[PI, 1.0], [TAU, 1.0], [TAU, 0.0], [TAU + PI, 0.0]];

#[test]
fn a_ring_whose_curves_meet_at_the_end_of_the_turn_is_written_with_its_curves_joined() {
    let regions = [
        [[0.0, PI], [0.0, 0.0]],
        [[PI, PI], [0.0, 1.0]],
        [[PI, TAU], [1.0, 1.0]],
        [[TAU, TAU], [0.0, 1.0]],
        [[0.0, TAU], [BAND_HEIGHT, BAND_HEIGHT]],
    ];
    // The seam hole starts the turn at the azimuth -0.1, inside the arc at
    // height 1, so that arc is cut there and the axial line at `2π` moves
    // a turn down.
    for (ring, hole, classified) in [STEP, STEP_FROM_ITS_UPPER_ARC]
        .into_iter()
        .flat_map(|ring| [(ring, None), (ring, Some(SEAM_HOLE))])
        .flat_map(|(ring, hole)| [(ring, hole, false), (ring, hole, true)])
    {
        let regions = hole.map_or(regions.to_vec(), |hole| {
            regions.iter().copied().chain([hole]).collect()
        });
        for stretch in [STRETCH, MIRROR_STRETCH] {
            let (losses, read) = written_under(ring_band(&ring, hole, classified), stretch, &[]);
            for code in [
                "iges/writer.face-omitted",
                "iges/writer.loop-classification-omitted",
            ] {
                assert!(charged(&losses, code).is_empty(), "{losses:#?}");
            }
            assert_eq!(read.geometry.faces.len(), 1);
            let face = &read.geometry.faces[0];
            assert_eq!(
                face.loops
                    .stated()
                    .any(|loop_| face.loop_role(loop_) == LoopBoundaryRole::Outer),
                classified
            );
            assert_boundary_in_domain(&read);
            assert_on_cylinder_regions(&read, stretch, &regions);
        }
    }
}

/// A ring with a notch below the azimuth `2π - 0.1`, which it crosses three
/// times.
const NOTCH: [[f64; 2]; 6] = [
    [0.0, 0.0],
    [6.25, 0.0],
    [6.25, 0.5],
    [6.12, 0.5],
    [6.12, 1.0],
    [TAU, 1.0],
];

#[test]
fn a_classified_band_whose_ring_passes_the_end_of_the_turn_more_than_once_is_written_unclassified()
{
    // The seam hole starts the turn at the azimuth -0.1.
    for stretch in [STRETCH, MIRROR_STRETCH] {
        let (losses, read) = written_under(ring_band(&NOTCH, Some(SEAM_HOLE), true), stretch, &[]);
        let unclassified = charged(&losses, "iges/writer.loop-classification-omitted");
        assert_eq!(unclassified.len(), 1, "{losses:#?}");
        assert!(
            unclassified[0].starts_with("IGES face test:model:face#face ")
                && ["0", "2", "4"]
                    .iter()
                    .all(|side| unclassified[0]
                        .contains(&format!("test:model:coedge#ring-side-{side}"))),
            "{unclassified:#?}"
        );
        assert!(
            charged(&losses, "iges/writer.face-omitted").is_empty(),
            "{losses:#?}"
        );
        assert_eq!(read.geometry.faces.len(), 1);
        let face = &read.geometry.faces[0];
        assert_eq!(face.loops.stated().count(), 3);
        assert!(face
            .loops
            .stated()
            .all(|loop_| face.loop_role(loop_) == LoopBoundaryRole::Unspecified));
    }
    for (hole, classified) in [(None, true), (Some(SEAM_HOLE), false)] {
        for stretch in [STRETCH, MIRROR_STRETCH] {
            let (losses, read) = written_under(ring_band(&NOTCH, hole, classified), stretch, &[]);
            assert!(
                charged(&losses, "iges/writer.loop-classification-omitted").is_empty(),
                "{losses:#?}"
            );
            assert_eq!(read.geometry.faces.len(), 1);
        }
    }
}
