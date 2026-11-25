// SPDX-License-Identifier: Apache-2.0
//! Cone, sphere and torus faces under a transform that is not a similarity:
//! the writer computes their parameter curves, closes each parameter loop at
//! a pole or apex, and writes the support as the exact rational Type 128.

use std::f64::consts::PI;
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::eval::decode::{pcurve_uv, surface_point_solved};
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{
    CircleCurve, ConeSurface, LineCurve, SphereSurface, TorusSurface,
};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
    VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, EdgeCarrier, EdgeEnds, Face, FaceLoops, Loop, LoopBoundary,
    LoopRing, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use crate::{IgesCodec, IgesVersion};

/// The sweep of the sphere lune and the cone sector, narrower than 45°.
const SECTOR: f64 = PI / 6.0;
const SPHERE_RADIUS: f64 = 10.0;
const CONE_RADIUS: f64 = 10.0;
/// `ρ(v) = CONE_RADIUS + CONE_SLOPE v`, so the apex lies at `v = -20`.
const CONE_SLOPE: f64 = 0.5;
const TORUS_MAJOR: f64 = 20.0;
const TORUS_MINOR: f64 = 5.0;
/// Bound on the distance of a read-back point from the exact image.
const ON_SURFACE: f64 = 1.0e-7;
/// Bound on the gap between two parameter curve ends that meet.
const JOINED: f64 = 1.0e-9;

fn id<T>(
    mint: fn(String) -> Result<T, cadmpeg_ir::ids::IdentityError>,
    kind: &str,
    key: &str,
) -> T {
    mint(format!("test:model:{kind}#{key}")).expect("identity grammar")
}

/// One sheet face, built record by record.
struct Sheet {
    ir: CadIr,
}

impl Sheet {
    fn new(surface: SolvedSurfaceGeometry) -> Self {
        let mut ir = CadIr::empty();
        ir.geometry.surfaces.push(Surface {
            id: id(SurfaceId::mint, "surface", "face"),
            geometry: SurfaceGeometry::Solved(surface),
            source_object: None,
            native_ref: None,
        });
        Self { ir }
    }

    fn vertex(&mut self, key: &str, position: Point3) {
        self.ir.geometry.points.push(Point::new(
            id(PointId::mint, "point", key),
            FinitePoint3::new(position).expect("a finite position"),
            None,
        ));
        self.ir.geometry.vertices.push(Vertex {
            id: id(VertexId::mint, "vertex", key),
            point: id(PointId::mint, "point", key),
            tolerance: None,
            native_ref: None,
        });
    }

    fn edge(&mut self, key: &str, curve: SolvedCurveGeometry, range: [f64; 2], ends: [&str; 2]) {
        self.ir.geometry.curves.push(Curve {
            parameter_range: None,
            id: id(CurveId::mint, "curve", key),
            geometry: CurveGeometry::Solved(curve),
            source_object: None,
            native_ref: None,
        });
        self.ir.geometry.edges.push(Edge {
            id: id(EdgeId::mint, "edge", key),
            carrier: EdgeCarrier::new(Some(id(CurveId::mint, "curve", key)), Some(range))
                .expect("a bounded edge"),
            ends: Some(EdgeEnds::new(
                id(VertexId::mint, "vertex", ends[0]),
                id(VertexId::mint, "vertex", ends[1]),
            )),
            tolerance: None,
            native_ref: None,
        });
    }

    /// The sheet body whose one forward face is bounded by `ring`, each edge
    /// used in the given sense.
    fn finished(mut self, ring: &[(&str, Sense)]) -> CadIr {
        let geometry = &mut self.ir.geometry;
        for (edge, sense) in ring {
            geometry.coedges.push(Coedge {
                id: id(CoedgeId::mint, "coedge", edge),
                owner_loop: id(LoopId::mint, "loop", "face"),
                edge: id(EdgeId::mint, "edge", edge),
                radial_next: id(CoedgeId::mint, "coedge", edge),
                sense: *sense,
                pcurves: Vec::new(),
                use_curve: None,
                native_ref: None,
            });
        }
        geometry.loops.push(Loop {
            id: id(LoopId::mint, "loop", "face"),
            face: id(FaceId::mint, "face", "face"),
            boundary: LoopBoundary::Ring(
                LoopRing::new(
                    ring.iter()
                        .map(|(edge, _)| id(CoedgeId::mint, "coedge", edge))
                        .collect(),
                    Vec::new(),
                )
                .expect("a ring"),
            ),
            native_ref: None,
        });
        geometry.faces.push(Face {
            id: id(FaceId::mint, "face", "face"),
            shell: id(ShellId::mint, "shell", "sheet"),
            surface: id(SurfaceId::mint, "surface", "face"),
            sense: Sense::Forward,
            loops: FaceLoops::classified(id(LoopId::mint, "loop", "face"), Vec::new()),
            name: None,
            color: None,
            tolerance: None,
            native_ref: None,
        });
        geometry.shells.push(Shell::new(
            id(ShellId::mint, "shell", "sheet"),
            id(RegionId::mint, "region", "sheet"),
        ));
        geometry.regions.push(Region::new(
            id(RegionId::mint, "region", "sheet"),
            id(BodyId::mint, "body", "sheet"),
            vec![id(ShellId::mint, "shell", "sheet")],
        ));
        geometry.bodies.push(Body {
            id: id(BodyId::mint, "body", "sheet"),
            kind: BodyKind::Sheet,
            transform: None,
            name: None,
            color: None,
            visible: None,
            native_ref: None,
            unstated: None,
        });
        self.ir.finalize();
        self.ir
    }
}

fn azimuth(angle: f64) -> Vector3 {
    Vector3::new(angle.cos(), angle.sin(), 0.0)
}

fn z_axis() -> Vector3 {
    Vector3::new(0.0, 0.0, 1.0)
}

fn origin() -> Point3 {
    Point3::new(0.0, 0.0, 0.0)
}

/// A lune of the sphere about `+z` between the meridians at azimuth 0 and
/// `sweep`, from pole to pole.
fn sphere_lune(sweep: f64) -> CadIr {
    let r = SPHERE_RADIUS;
    let mut sheet = Sheet::new(SolvedSurfaceGeometry::Sphere(
        SphereSurface::try_new(origin(), z_axis(), Vector3::new(1.0, 0.0, 0.0), r)
            .expect("a sphere"),
    ));
    sheet.vertex("south", Point3::new(0.0, 0.0, -r));
    sheet.vertex("north", Point3::new(0.0, 0.0, r));
    for (key, angle) in [("start", 0.0), ("end", sweep)] {
        // From the south pole at 0 through the equator at π/2 to the north
        // pole at π.
        let meridian = CircleCurve::try_new(
            origin(),
            azimuth(angle).cross(z_axis()),
            Vector3::new(0.0, 0.0, -1.0),
            r,
        )
        .expect("a meridian");
        sheet.edge(
            key,
            SolvedCurveGeometry::Circle(meridian),
            [0.0, PI],
            ["south", "north"],
        );
    }
    sheet.finished(&[("end", Sense::Forward), ("start", Sense::Reversed)])
}

/// A sector of the cone about `+z` between the generators at azimuth 0 and
/// `sweep`, from the apex to the circle of radius [`CONE_RADIUS`] at `z = 0`.
fn cone_sector(sweep: f64) -> CadIr {
    let apex = Point3::new(0.0, 0.0, -CONE_RADIUS / CONE_SLOPE);
    let mut sheet = Sheet::new(SolvedSurfaceGeometry::Cone(
        ConeSurface::try_new(
            origin(),
            z_axis(),
            Vector3::new(1.0, 0.0, 0.0),
            CONE_RADIUS,
            1.0,
            CONE_SLOPE.atan(),
        )
        .expect("a cone"),
    ));
    sheet.vertex("apex", apex);
    for (key, angle) in [("start", 0.0), ("end", sweep)] {
        let base = azimuth(angle).scale(CONE_RADIUS);
        sheet.vertex(&format!("base-{key}"), Point3::new(base.x, base.y, 0.0));
        let generator = Vector3::new(base.x - apex.x, base.y - apex.y, -apex.z);
        let length = generator.norm();
        sheet.edge(
            key,
            SolvedCurveGeometry::Line(
                LineCurve::try_new(apex, generator.scale(1.0 / length)).expect("a generator"),
            ),
            [0.0, length],
            ["apex", &format!("base-{key}")],
        );
    }
    sheet.edge(
        "arc",
        SolvedCurveGeometry::Circle(
            CircleCurve::try_new(origin(), z_axis(), Vector3::new(1.0, 0.0, 0.0), CONE_RADIUS)
                .expect("a base circle"),
        ),
        [0.0, sweep],
        ["base-start", "base-end"],
    );
    sheet.finished(&[
        ("end", Sense::Forward),
        ("arc", Sense::Reversed),
        ("start", Sense::Reversed),
    ])
}

fn torus_point(u: f64, v: f64) -> Point3 {
    let rho = TORUS_MINOR.mul_add(v.cos(), TORUS_MAJOR);
    Point3::new(rho * u.cos(), rho * u.sin(), TORUS_MINOR * v.sin())
}

/// The patch of the torus about `+z` over `[0, sweeps[0]] × [0, sweeps[1]]`,
/// bounded by two parallels and two meridians.
fn torus_patch(sweeps: [f64; 2]) -> CadIr {
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
    for (key, u, v) in [
        ("00", 0.0, 0.0),
        ("10", sweeps[0], 0.0),
        ("01", 0.0, sweeps[1]),
        ("11", sweeps[0], sweeps[1]),
    ] {
        sheet.vertex(key, torus_point(u, v));
    }
    for (key, v, ends) in [
        ("low", 0.0, ["00", "10"]),
        ("high", sweeps[1], ["01", "11"]),
    ] {
        let parallel = CircleCurve::try_new(
            Point3::new(0.0, 0.0, TORUS_MINOR * v.sin()),
            z_axis(),
            Vector3::new(1.0, 0.0, 0.0),
            TORUS_MINOR.mul_add(v.cos(), TORUS_MAJOR),
        )
        .expect("a parallel");
        sheet.edge(
            key,
            SolvedCurveGeometry::Circle(parallel),
            [0.0, sweeps[0]],
            ends,
        );
    }
    for (key, u, ends) in [
        ("start", 0.0, ["00", "01"]),
        ("end", sweeps[0], ["10", "11"]),
    ] {
        let centre = azimuth(u).scale(TORUS_MAJOR);
        let meridian = CircleCurve::try_new(
            Point3::new(centre.x, centre.y, 0.0),
            azimuth(u).cross(z_axis()),
            azimuth(u),
            TORUS_MINOR,
        )
        .expect("a meridian");
        sheet.edge(
            key,
            SolvedCurveGeometry::Circle(meridian),
            [0.0, sweeps[1]],
            ends,
        );
    }
    sheet.finished(&[
        ("low", Sense::Forward),
        ("end", Sense::Forward),
        ("high", Sense::Reversed),
        ("start", Sense::Reversed),
    ])
}

/// A map `x ↦ diag(scale) x + shift` that is not a similarity.
#[derive(Clone, Copy)]
struct Stretch {
    scale: [f64; 3],
    shift: [f64; 3],
}

impl Stretch {
    fn transform(self) -> Option<Transform> {
        let [scale, shift] = [self.scale, self.shift];
        Transform::affine([
            [scale[0], 0.0, 0.0, shift[0]],
            [0.0, scale[1], 0.0, shift[1]],
            [0.0, 0.0, scale[2], shift[2]],
        ])
    }

    /// The source point of an image point.
    fn source(self, point: Point3) -> Point3 {
        Point3::new(
            (point.x - self.shift[0]) / self.scale[0],
            (point.y - self.shift[1]) / self.scale[1],
            (point.z - self.shift[2]) / self.scale[2],
        )
    }
}

const STRETCH: Stretch = Stretch {
    scale: [2.0, 1.0, 1.5],
    shift: [1.0, -2.0, 0.5],
};

const MIRROR_STRETCH: Stretch = Stretch {
    scale: [-2.0, 1.0, 1.5],
    shift: [1.0, -2.0, 0.5],
};

fn charged<'a>(losses: &'a [LossNote], code: &str) -> Vec<&'a str> {
    losses
        .iter()
        .filter(|loss| loss.code.to_string() == code)
        .map(|loss| loss.message.as_str())
        .collect()
}

/// `ir` with its body under `stretch`, written and read back with every face
/// and parameter curve.
fn read_back(mut ir: CadIr, stretch: Stretch) -> CadIr {
    for body in &mut ir.geometry.bodies {
        body.transform = stretch.transform();
    }
    let plan = IgesCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(IgesVersion::V5_3.descriptor().id.as_str()),
        )
        .expect("the document is written");
    let losses = plan.report().losses.clone();
    for code in [
        "iges/writer.body-omitted",
        "iges/writer.face-omitted",
        "iges/writer.pcurve-omitted",
    ] {
        assert!(charged(&losses, code).is_empty(), "{losses:#?}");
    }
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("the file is written");
    IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the written file decodes")
        .ir()
        .clone()
}

/// The support points at sixteen equal steps of each parameter curve on the
/// boundary of the one face of `read`, and the end points of each curve.
fn boundary(read: &CadIr) -> (Vec<Point3>, Vec<[f64; 2]>) {
    let geometry = &read.geometry;
    assert_eq!(geometry.faces.len(), 1);
    let face = &geometry.faces[0];
    let surface = geometry
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)
        .expect("the face support reads back");
    let support = match &surface.geometry {
        SurfaceGeometry::Solved(solved) => solved,
        SurfaceGeometry::Procedural { cache, .. } => {
            cache.as_ref().expect("the support reads back solved")
        }
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
        .expect("an evaluation context");
    let mut points = Vec::new();
    let mut ends = Vec::new();
    for loop_id in face.loops.stated() {
        let loop_ = geometry
            .loops
            .iter()
            .find(|loop_| loop_.id == *loop_id)
            .expect("the face loop reads back");
        for coedge_id in loop_.coedges() {
            let coedge = geometry
                .coedges
                .iter()
                .find(|coedge| coedge.id == *coedge_id)
                .expect("the loop coedge reads back");
            assert!(
                !coedge.pcurves.is_empty(),
                "coedge {} reads back with no parameter curve",
                coedge.id
            );
            for pcurve_use in &coedge.pcurves {
                let pcurve = geometry
                    .pcurves
                    .iter()
                    .find(|pcurve| pcurve.id == pcurve_use.pcurve)
                    .expect("the parameter curve reads back");
                let [start, end] = pcurve
                    .parameter_range()
                    .expect("a bounded parameter curve")
                    .get();
                for step in 0..=16 {
                    let parameter = (f64::from(step) / 16.0).mul_add(end - start, start);
                    let uv = pcurve_uv(&ctx, &pcurve.geometry, parameter)
                        .expect("a parameter point")
                        .get();
                    if step == 0 || step == 16 {
                        ends.push([uv.u, uv.v]);
                    }
                    points.push(
                        surface_point_solved(&ctx, support, uv.u, uv.v)
                            .expect("a point on the support")
                            .get(),
                    );
                }
            }
        }
    }
    (points, ends)
}

/// Assert that each parameter curve end meets the end of another curve, so
/// that the parameter loop closes.
fn assert_parameter_loop_closes(ends: &[[f64; 2]]) {
    for (position, end) in ends.iter().enumerate() {
        assert!(
            ends.iter()
                .enumerate()
                .any(|(other, partner)| other != position
                    && (partner[0] - end[0]).abs() <= JOINED
                    && (partner[1] - end[1]).abs() <= JOINED),
            "the parameter curve end {end:?} meets no other end: {ends:?}"
        );
    }
}

fn within_sector(point: Point3, sweep: f64) -> bool {
    point.x.hypot(point.y) <= ON_SURFACE || {
        let angle = point.y.atan2(point.x);
        (-ON_SURFACE..=sweep + ON_SURFACE).contains(&angle)
    }
}

#[test]
fn a_sphere_lune_under_a_non_uniform_map_closes_its_parameter_loop_at_the_poles() {
    for stretch in [STRETCH, MIRROR_STRETCH] {
        let (points, ends) = boundary(&read_back(sphere_lune(SECTOR), stretch));
        assert_parameter_loop_closes(&ends);
        for point in points.into_iter().map(|point| stretch.source(point)) {
            let radius = point.x.hypot(point.y).hypot(point.z);
            assert!(
                (radius - SPHERE_RADIUS).abs() <= ON_SURFACE,
                "{point:?} lies off the sphere"
            );
            assert!(
                within_sector(point, SECTOR),
                "{point:?} lies outside the lune"
            );
        }
    }
}

#[test]
fn a_cone_sector_under_a_non_uniform_map_closes_its_parameter_loop_at_the_apex() {
    for stretch in [STRETCH, MIRROR_STRETCH] {
        let (points, ends) = boundary(&read_back(cone_sector(SECTOR), stretch));
        assert_parameter_loop_closes(&ends);
        for point in points.into_iter().map(|point| stretch.source(point)) {
            let rho = CONE_SLOPE.mul_add(point.z, CONE_RADIUS);
            assert!(
                (point.x.hypot(point.y) - rho).abs() <= ON_SURFACE,
                "{point:?} lies off the cone"
            );
            assert!(
                (-CONE_RADIUS / CONE_SLOPE - ON_SURFACE..=ON_SURFACE).contains(&point.z),
                "{point:?} lies outside the sector heights"
            );
            assert!(
                within_sector(point, SECTOR),
                "{point:?} lies outside the sector"
            );
        }
    }
}

#[test]
fn a_torus_patch_under_a_non_uniform_map_writes_its_computed_parameter_curves() {
    let sweeps = [40.0_f64.to_radians(), 60.0_f64.to_radians()];
    for stretch in [STRETCH, MIRROR_STRETCH] {
        let (points, ends) = boundary(&read_back(torus_patch(sweeps), stretch));
        assert_parameter_loop_closes(&ends);
        for point in points.into_iter().map(|point| stretch.source(point)) {
            let tube = (point.x.hypot(point.y) - TORUS_MAJOR).hypot(point.z);
            assert!(
                (tube - TORUS_MINOR).abs() <= ON_SURFACE,
                "{point:?} lies off the torus"
            );
            assert!(
                within_sector(point, sweeps[0]),
                "{point:?} lies outside the patch"
            );
            assert!(
                (-ON_SURFACE..=TORUS_MINOR * sweeps[1].sin() + ON_SURFACE).contains(&point.z),
                "{point:?} lies outside the patch"
            );
        }
    }
}
