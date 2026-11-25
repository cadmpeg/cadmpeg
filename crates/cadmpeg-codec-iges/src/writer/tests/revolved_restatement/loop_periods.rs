// SPDX-License-Identifier: Apache-2.0
//! Faces whose loops the writer moves by whole turns of a periodic parameter
//! into one window of a turn.

use std::f64::consts::{PI, TAU};

use cadmpeg_ir::geometry::analytic::{CircleCurve, CylinderSurface, LineCurve, TorusSurface};
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};
use cadmpeg_ir::ids::{CoedgeId, EdgeId, FaceId, LoopId, ShellId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Coedge, Face, FaceLoops, Loop, LoopBoundary, LoopRing, Sense};
use cadmpeg_ir::CadIr;

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

/// `sheet` with the face of `loops` and, where `second` names a loop of
/// `sheet`, the face `second` that this loop bounds, on the same support.
fn faces(mut sheet: Sheet, loops: FaceLoops, second: Option<LoopId>) -> CadIr {
    let second_face = id(FaceId::mint, "face", "second");
    if let Some(second) = &second {
        sheet
            .ir
            .geometry
            .loops
            .iter_mut()
            .find(|loop_| loop_.id == *second)
            .expect("the second loop")
            .face = second_face.clone();
    }
    let mut ir = sheet.faced(loops);
    if let Some(second) = second {
        ir.geometry.faces.push(Face {
            id: second_face,
            shell: id(ShellId::mint, "shell", "sheet"),
            surface: id(SurfaceId::mint, "surface", "face"),
            sense: Sense::Forward,
            loops: FaceLoops::classified(second, Vec::new()),
            name: None,
            color: None,
            tolerance: None,
            native_ref: None,
        });
        ir.finalize();
    }
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
        Some(second),
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
fn a_face_that_no_turn_holds_with_the_faces_before_it_is_dropped() {
    let first = [[1.0, 3.5], [0.0, 2.0]];
    for stretch in [STRETCH, MIRROR_STRETCH] {
        let (losses, read) = written_under(
            two_patches(first, [[3.0, 7.5], PATCH_HEIGHTS], [0.0, 0.0]),
            stretch,
        );
        let omitted = charged(&losses, "iges/writer.face-omitted");
        assert_eq!(omitted.len(), 1, "{losses:#?}");
        assert!(
            omitted[0].starts_with("face test:model:face#second ")
                && omitted[0].contains("no one turn of the azimuth holds face"),
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
    faces(sheet, loops, patch)
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
