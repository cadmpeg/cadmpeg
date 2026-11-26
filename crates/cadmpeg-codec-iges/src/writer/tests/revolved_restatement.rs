// SPDX-License-Identifier: Apache-2.0
//! Cone, sphere and torus faces under a transform that is not a similarity:
//! the writer computes their parameter curves, closes each parameter loop at
//! a pole or apex, and writes the support as the exact rational Type 128.

use std::f64::consts::{PI, TAU};
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::eval::decode::{curve_point_solved, pcurve_uv, surface_point_solved};
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{
    CircleCurve, ConeSurface, CylinderSurface, EllipseCurve, LineCurve, SphereSurface, TorusSurface,
};
use cadmpeg_ir::geometry::pcurve::{LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata};
use cadmpeg_ir::geometry::procedural_curves::DirectedParameterRange;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::{
    AnchoredVertexUse, Body, BodyKind, Coedge, Edge, EdgeCarrier, EdgeEnds, Face, FaceLoops, Loop,
    LoopBoundary, LoopRing, PcurveUse, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::FiniteVector;
use cadmpeg_ir::CadIr;

use crate::{IgesCodec, IgesVersion};

mod boundary_start;
mod loop_periods;

/// The sweep of the sphere lune and the cone sector, narrower than 45°.
const SECTOR: f64 = PI / 6.0;
const SPHERE_RADIUS: f64 = 10.0;
const CONE_RADIUS: f64 = 10.0;
/// `ρ(v) = CONE_RADIUS + CONE_SLOPE v`, so the apex lies at `v = -20`.
const CONE_SLOPE: f64 = 0.5;
const TORUS_MAJOR: f64 = 20.0;
const TORUS_MINOR: f64 = 5.0;
const CYLINDER_RADIUS: f64 = 10.0;
/// The plane `z = CUT_HEIGHT + CUT_SLOPE x` cuts the cylinder sector.
const CUT_HEIGHT: f64 = 8.0;
const CUT_SLOPE: f64 = 0.5;
/// Bound on the distance of a read-back point from the exact image.
const ON_SURFACE: f64 = 1.0e-7;
/// Bound on the angle between consecutive support samples around a turn.
const TURN_GAP: f64 = 0.5;
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

    /// The sheet body whose one forward face states no loop.
    fn loopless(self) -> CadIr {
        self.faced(FaceLoops::unspecified(Vec::new()))
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
        self.faced(FaceLoops::classified(
            id(LoopId::mint, "loop", "face"),
            Vec::new(),
        ))
    }

    /// The sheet body whose one forward face has `loops`.
    fn faced(mut self, loops: FaceLoops) -> CadIr {
        let geometry = &mut self.ir.geometry;
        geometry.faces.push(Face {
            id: id(FaceId::mint, "face", "face"),
            shell: id(ShellId::mint, "shell", "sheet"),
            surface: id(SurfaceId::mint, "surface", "face"),
            sense: Sense::Forward,
            loops,
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
    assert_eq!(read.geometry.faces.len(), 1);
    face_boundary(read, &read.geometry.faces[0])
}

/// The support points at sixteen equal steps of each parameter curve on the
/// boundary of `face` of `read`, and the end points of each curve.
fn face_boundary(read: &CadIr, face: &Face) -> (Vec<Point3>, Vec<[f64; 2]>) {
    let geometry = &read.geometry;
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

/// The point of the cylinder of radius [`CYLINDER_RADIUS`] about `+z` at
/// azimuth `angle` on the plane cut `z = CUT_HEIGHT + CUT_SLOPE x`.
fn cut_point(angle: f64) -> Point3 {
    let (sin, cos) = angle.sin_cos();
    Point3::new(
        CYLINDER_RADIUS * cos,
        CYLINDER_RADIUS * sin,
        (CUT_SLOPE * CYLINDER_RADIUS).mul_add(cos, CUT_HEIGHT),
    )
}

/// A sector of the cylinder about `+z` between the generators at azimuth 0
/// and `sweep`, from `z = 0` to the ellipse where the plane
/// `z = CUT_HEIGHT + CUT_SLOPE x` cuts the cylinder. With `bottom_pcurve`,
/// the bottom coedge carries its parameter curve and the others carry none.
fn cut_cylinder_sector(sweep: f64, bottom_pcurve: bool) -> CadIr {
    let mut sheet = Sheet::new(SolvedSurfaceGeometry::Cylinder(
        CylinderSurface::try_new(
            origin(),
            z_axis(),
            Vector3::new(1.0, 0.0, 0.0),
            CYLINDER_RADIUS,
        )
        .expect("a cylinder"),
    ));
    for (key, angle) in [("start", 0.0), ("end", sweep)] {
        let base = azimuth(angle).scale(CYLINDER_RADIUS);
        let top = cut_point(angle);
        sheet.vertex(&format!("base-{key}"), Point3::new(base.x, base.y, 0.0));
        sheet.vertex(&format!("top-{key}"), top);
        sheet.edge(
            key,
            SolvedCurveGeometry::Line(
                LineCurve::try_new(Point3::new(base.x, base.y, 0.0), z_axis())
                    .expect("a generator"),
            ),
            [0.0, top.z],
            [&format!("base-{key}"), &format!("top-{key}")],
        );
    }
    sheet.edge(
        "bottom",
        SolvedCurveGeometry::Circle(
            CircleCurve::try_new(
                origin(),
                z_axis(),
                Vector3::new(1.0, 0.0, 0.0),
                CYLINDER_RADIUS,
            )
            .expect("a base circle"),
        ),
        [0.0, sweep],
        ["base-start", "base-end"],
    );
    let stretch = CUT_SLOPE.hypot(1.0);
    sheet.edge(
        "cut",
        SolvedCurveGeometry::Ellipse(
            EllipseCurve::try_new(
                Point3::new(0.0, 0.0, CUT_HEIGHT),
                Vector3::new(-CUT_SLOPE / stretch, 0.0, 1.0 / stretch),
                Vector3::new(1.0 / stretch, 0.0, CUT_SLOPE / stretch),
                CYLINDER_RADIUS * stretch,
                CYLINDER_RADIUS,
            )
            .expect("the cut ellipse"),
        ),
        [0.0, sweep],
        ["top-start", "top-end"],
    );
    let mut ir = sheet.finished(&[
        ("bottom", Sense::Forward),
        ("end", Sense::Forward),
        ("cut", Sense::Reversed),
        ("start", Sense::Reversed),
    ]);
    if bottom_pcurve {
        let pcurve = id(PcurveId::mint, "pcurve", "bottom");
        ir.geometry.pcurves.push(Pcurve {
            id: pcurve.clone(),
            geometry: PcurveGeometry::Line(
                LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0))
                    .expect("a unit direction"),
            ),
            metadata: PcurveMetadata::general(None, FiniteVector::new([0.0, sweep]), None),
            native_ref: None,
        });
        ir.geometry
            .coedges
            .iter_mut()
            .find(|coedge| coedge.id == id(CoedgeId::mint, "coedge", "bottom"))
            .expect("the bottom coedge")
            .pcurves = vec![PcurveUse {
            pcurve,
            isoparametric: Some(true),
            parameter_range: None,
        }];
        ir.finalize();
    }
    ir
}

/// `ir` with its body under `stretch`, written, with the losses of the
/// write and the document that reads back.
fn written_under(mut ir: CadIr, stretch: Stretch) -> (Vec<LossNote>, CadIr) {
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
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("the file is written");
    let read = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the written file decodes")
        .ir()
        .clone();
    (losses, read)
}

/// Points at 32 equal steps of each edge curve of `read` over its range.
fn edge_points(read: &CadIr) -> Vec<Point3> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
        .expect("an evaluation context");
    let mut points = Vec::new();
    for edge in &read.geometry.edges {
        let curve = read
            .geometry
            .curves
            .iter()
            .find(|curve| Some(&curve.id) == edge.carrier.curve())
            .expect("the edge curve reads back");
        let geometry = match &curve.geometry {
            CurveGeometry::Solved(solved) => solved,
            CurveGeometry::Procedural { cache, .. } => {
                cache.as_ref().expect("the edge curve reads back solved")
            }
        };
        let [start, end] = edge
            .param_range()
            .map(FiniteVector::get)
            .expect("a bounded edge");
        for step in 0..=32 {
            let parameter = (f64::from(step) / 32.0).mul_add(end - start, start);
            points.push(
                curve_point_solved(&ctx, geometry, parameter)
                    .expect("a point on the edge")
                    .get(),
            );
        }
    }
    points
}

#[test]
fn a_cylinder_face_whose_parameter_curves_have_no_exact_form_keeps_its_model_space_boundary() {
    let sweep = PI / 3.0;
    for (stretch, bottom_pcurve) in [(STRETCH, false), (MIRROR_STRETCH, false), (STRETCH, true)] {
        let (losses, read) = written_under(cut_cylinder_sector(sweep, bottom_pcurve), stretch);
        for code in ["iges/writer.body-omitted", "iges/writer.face-omitted"] {
            assert!(charged(&losses, code).is_empty(), "{losses:#?}");
        }
        let omitted = charged(&losses, "iges/writer.pcurve-omitted");
        assert_eq!(omitted.len(), usize::from(bottom_pcurve), "{losses:#?}");
        assert_eq!(read.geometry.faces.len(), 1);
        let support = read
            .geometry
            .surfaces
            .iter()
            .find(|surface| surface.id == read.geometry.faces[0].surface)
            .expect("the face support reads back");
        assert!(
            matches!(
                &support.geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_))
                    | SurfaceGeometry::Procedural {
                        cache: Some(SolvedSurfaceGeometry::Nurbs(_)),
                        ..
                    }
            ),
            "the support reads back as {:?}",
            support.geometry
        );
        assert!(read
            .geometry
            .coedges
            .iter()
            .all(|coedge| coedge.pcurves.is_empty()));
        let points = edge_points(&read);
        assert!(!points.is_empty());
        for point in points.into_iter().map(|point| stretch.source(point)) {
            assert!(
                (point.x.hypot(point.y) - CYLINDER_RADIUS).abs() <= ON_SURFACE,
                "{point:?} lies off the cylinder"
            );
            assert!(
                within_sector(point, sweep),
                "{point:?} lies outside the sector"
            );
            let top = (CUT_SLOPE * point.x) + CUT_HEIGHT;
            assert!(
                (-ON_SURFACE..=top + ON_SURFACE).contains(&point.z),
                "{point:?} lies outside the cut sector"
            );
        }
    }
}

/// Points of the support of the one face of `read` at a 24 × 24 grid of its
/// knot domain.
fn support_points(read: &CadIr) -> Vec<Point3> {
    let (support, [u_domain, v_domain]) = rational_support(read);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
        .expect("an evaluation context");
    let at =
        |[start, end]: [f64; 2], step: u32| (f64::from(step) / 24.0).mul_add(end - start, start);
    let mut points = Vec::new();
    for u in 0..=24 {
        for v in 0..=24 {
            points.push(
                surface_point_solved(&ctx, support, at(u_domain, u), at(v_domain, v))
                    .expect("a point on the support")
                    .get(),
            );
        }
    }
    points
}

/// The rational support of the one face of `read` and its parameter domain.
fn rational_support(read: &CadIr) -> (&SolvedSurfaceGeometry, [[f64; 2]; 2]) {
    face_support(read, &read.geometry.faces[0])
}

/// The rational support of `face` of `read` and its parameter domain.
fn face_support<'a>(read: &'a CadIr, face: &Face) -> (&'a SolvedSurfaceGeometry, [[f64; 2]; 2]) {
    let surface = read
        .geometry
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
    let SolvedSurfaceGeometry::Nurbs(nurbs) = support else {
        panic!("the support reads back as {support:?}");
    };
    let domain = |knots: &[f64]| [knots[0], knots[knots.len() - 1]];
    (
        support,
        [
            domain(nurbs.u_knots().as_slice()),
            domain(nurbs.v_knots().as_slice()),
        ],
    )
}

/// Assert that each parameter curve end on the boundary of each face of
/// `read` lies in the parameter domain of its rational support.
fn assert_boundary_in_domain(read: &CadIr) {
    for face in &read.geometry.faces {
        let (_, domain) = face_support(read, face);
        let (_, ends) = face_boundary(read, face);
        for end in &ends {
            for (value, [low, high]) in end.iter().zip(domain) {
                assert!(
                    (low - ON_SURFACE..=high + ON_SURFACE).contains(value),
                    "the parameter curve end {end:?} lies outside the domain {domain:?}"
                );
            }
        }
    }
}

/// The largest gap between consecutive angles of `angles` around the turn.
fn largest_gap(mut angles: Vec<f64>) -> f64 {
    angles.sort_by(f64::total_cmp);
    let wrap = angles.first().map_or(TAU, |first| first + TAU);
    angles
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .chain(angles.last().map(|last| wrap - last))
        .fold(0.0, f64::max)
}

#[test]
fn a_sphere_or_torus_face_that_states_no_loop_covers_its_whole_support() {
    let sphere = Sheet::new(SolvedSurfaceGeometry::Sphere(
        SphereSurface::try_new(
            origin(),
            z_axis(),
            Vector3::new(1.0, 0.0, 0.0),
            SPHERE_RADIUS,
        )
        .expect("a sphere"),
    ))
    .loopless();
    let torus = Sheet::new(SolvedSurfaceGeometry::Torus(
        TorusSurface::try_new(
            origin(),
            z_axis(),
            Vector3::new(1.0, 0.0, 0.0),
            TORUS_MAJOR,
            TORUS_MINOR,
        )
        .expect("a torus"),
    ))
    .loopless();
    for stretch in [STRETCH, MIRROR_STRETCH] {
        for (ir, circular) in [(sphere.clone(), false), (torus.clone(), true)] {
            let (losses, read) = written_under(ir, stretch);
            for code in [
                "iges/writer.body-omitted",
                "iges/writer.face-omitted",
                "iges/writer.free-geometry-omitted",
            ] {
                assert!(charged(&losses, code).is_empty(), "{losses:#?}");
            }
            assert_eq!(read.geometry.faces.len(), 1);
            assert!(read.geometry.faces[0].loops.stated().next().is_none());
            let points = support_points(&read)
                .into_iter()
                .map(|point| stretch.source(point))
                .collect::<Vec<_>>();
            let azimuths = points.iter().map(|point| point.y.atan2(point.x)).collect();
            assert!(
                largest_gap(azimuths) <= TURN_GAP,
                "the face misses azimuths"
            );
            if circular {
                for point in &points {
                    let tube = (point.x.hypot(point.y) - TORUS_MAJOR).hypot(point.z);
                    assert!(
                        (tube - TORUS_MINOR).abs() <= ON_SURFACE,
                        "{point:?} lies off the torus"
                    );
                }
                let minor = points
                    .iter()
                    .map(|point| point.z.atan2(point.x.hypot(point.y) - TORUS_MAJOR))
                    .collect();
                assert!(
                    largest_gap(minor) <= TURN_GAP,
                    "the face misses tube angles"
                );
            } else {
                for point in &points {
                    let radius = point.x.hypot(point.y).hypot(point.z);
                    assert!(
                        (radius - SPHERE_RADIUS).abs() <= ON_SURFACE,
                        "{point:?} lies off the sphere"
                    );
                }
                for pole in [SPHERE_RADIUS, -SPHERE_RADIUS] {
                    assert!(
                        points
                            .iter()
                            .any(|point| (point.z - pole).abs() <= ON_SURFACE),
                        "the face misses the pole at z = {pole}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_cylinder_face_that_states_no_loop_is_dropped_for_its_unbounded_domain() {
    let ir = Sheet::new(SolvedSurfaceGeometry::Cylinder(
        CylinderSurface::try_new(
            origin(),
            z_axis(),
            Vector3::new(1.0, 0.0, 0.0),
            CYLINDER_RADIUS,
        )
        .expect("a cylinder"),
    ))
    .loopless();
    let plan = IgesCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(IgesVersion::V5_3.descriptor().id.as_str()),
        )
        .expect("the document is written");
    let omitted = charged(&plan.report().losses, "iges/writer.face-omitted");
    assert_eq!(omitted.len(), 1, "{omitted:#?}");
    assert!(
        omitted[0].contains("test:model:face#face")
            && omitted[0].contains("states no loop has no finite domain"),
        "{omitted:#?}"
    );
}

/// A parameter line from `origin` along the unit `direction` over
/// `[0, length]`, and its use.
fn parameter_line(
    ir: &mut CadIr,
    key: &str,
    origin: [f64; 2],
    direction: [f64; 2],
    length: f64,
) -> PcurveUse {
    let pcurve = id(PcurveId::mint, "pcurve", key);
    ir.geometry.pcurves.push(Pcurve {
        id: pcurve.clone(),
        geometry: PcurveGeometry::Line(
            LinePcurve::try_new(
                Point2::new(origin[0], origin[1]),
                Point2::new(direction[0], direction[1]),
            )
            .expect("a unit direction"),
        ),
        metadata: PcurveMetadata::general(None, FiniteVector::new([0.0, length]), None),
        native_ref: None,
    });
    PcurveUse {
        pcurve,
        isoparametric: Some(true),
        parameter_range: None,
    }
}

/// [`sphere_lune`] whose coedges carry their parameter curves and whose loop
/// states each pole as a vertex use with the parameter line along that pole.
fn sphere_lune_with_pole_uses(sweep: f64) -> CadIr {
    let mut ir = sphere_lune(sweep);
    let half = PI / 2.0;
    let end = parameter_line(&mut ir, "end", [sweep, -half], [0.0, 1.0], PI);
    let start = parameter_line(&mut ir, "start", [0.0, half], [0.0, -1.0], PI);
    let north = parameter_line(&mut ir, "north", [sweep, half], [-1.0, 0.0], sweep);
    let south = parameter_line(&mut ir, "south", [0.0, -half], [1.0, 0.0], sweep);
    for (key, pcurve) in [("end", end), ("start", start)] {
        ir.geometry
            .coedges
            .iter_mut()
            .find(|coedge| coedge.id == id(CoedgeId::mint, "coedge", key))
            .expect("the lune coedge")
            .pcurves = vec![pcurve];
    }
    let coedges = vec![
        id(CoedgeId::mint, "coedge", "end"),
        id(CoedgeId::mint, "coedge", "start"),
    ];
    let vertex_uses = vec![
        AnchoredVertexUse {
            vertex: id(VertexId::mint, "vertex", "north"),
            after: id(CoedgeId::mint, "coedge", "end"),
            pcurves: vec![north],
        },
        AnchoredVertexUse {
            vertex: id(VertexId::mint, "vertex", "south"),
            after: id(CoedgeId::mint, "coedge", "start"),
            pcurves: vec![south],
        },
    ];
    ir.geometry.loops[0].boundary =
        LoopBoundary::Ring(LoopRing::new(coedges, vertex_uses).expect("a ring"));
    ir.finalize();
    ir
}

/// The parameter-space start and end of each parameter curve use on the
/// loop of the one face of `read`, in loop order: each coedge's uses, then
/// the uses of the vertex uses anchored after it.
fn ordered_uses(read: &CadIr) -> Vec<[Point2; 2]> {
    let geometry = &read.geometry;
    let face = &geometry.faces[0];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
        .expect("an evaluation context");
    let ends = |pcurve_use: &PcurveUse| {
        let pcurve = geometry
            .pcurves
            .iter()
            .find(|pcurve| pcurve.id == pcurve_use.pcurve)
            .expect("the parameter curve reads back");
        let range = pcurve_use.parameter_range.map_or_else(
            || {
                pcurve
                    .parameter_range()
                    .expect("a bounded parameter curve")
                    .get()
            },
            DirectedParameterRange::endpoints,
        );
        range.map(|parameter| {
            pcurve_uv(&ctx, &pcurve.geometry, parameter)
                .expect("a parameter point")
                .get()
        })
    };
    let mut ordered = Vec::new();
    for loop_ in face
        .loops
        .stated()
        .filter_map(|id| geometry.loops.iter().find(|loop_| loop_.id == *id))
    {
        for coedge_id in loop_.coedges() {
            let coedge = geometry
                .coedges
                .iter()
                .find(|coedge| coedge.id == *coedge_id)
                .expect("the loop coedge reads back");
            ordered.extend(coedge.pcurves.iter().map(ends));
            for vertex_use in loop_
                .anchored_vertex_uses()
                .iter()
                .filter(|vertex_use| vertex_use.after == *coedge_id)
            {
                ordered.extend(vertex_use.pcurves.iter().map(ends));
            }
        }
    }
    ordered
}

#[test]
fn a_reflected_sphere_lune_moves_each_pole_use_to_the_coedge_that_arrives_at_its_pole() {
    let mirror = Stretch {
        scale: [-1.0, 1.0, 1.0],
        shift: [1.0, -2.0, 0.5],
    };
    for stretch in [MIRROR_STRETCH, mirror] {
        let read = read_back(sphere_lune_with_pole_uses(SECTOR), stretch);
        assert_lune_boundary(&read, stretch);
    }
}

/// Assert that each parameter curve use on the loop of the one face of
/// `read` starts where the use before it ends, and that the boundary lies on
/// the sphere lune of sweep [`SECTOR`] under `stretch`.
fn assert_lune_boundary(read: &CadIr, stretch: Stretch) {
    let uses = ordered_uses(read);
    assert_eq!(uses.len(), 4, "{uses:?}");
    for (position, [_, end]) in uses.iter().enumerate() {
        let [next, _] = uses[(position + 1) % uses.len()];
        assert!(
            (next.u - end.u).abs() <= JOINED && (next.v - end.v).abs() <= JOINED,
            "use {position} ends at {end:?} and the next starts at {next:?}: {uses:?}"
        );
    }
    let (points, _) = boundary(read);
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

/// The tube angle of the torus band's lower parallel. The band reaches past
/// the tube angle `π` to the upper parallel at `TAU - BAND_LOW`.
const BAND_LOW: f64 = 5.0 * PI / 6.0;

/// The band of the torus about `+z` between the parallels at tube angles
/// [`BAND_LOW`] and `TAU - BAND_LOW`, each one closed edge in a loop of its
/// own. The coedges carry their parameter lines with the tube angle in
/// `(-π, π]`.
fn torus_band_across_the_seam() -> CadIr {
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
    let high = TAU - BAND_LOW;
    for (key, v) in [("lower", BAND_LOW), ("upper", high)] {
        sheet.vertex(key, torus_point(0.0, v));
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
            [0.0, TAU],
            [key, key],
        );
    }
    let lower = parameter_line(&mut sheet.ir, "lower", [0.0, BAND_LOW], [1.0, 0.0], TAU);
    let upper = parameter_line(&mut sheet.ir, "upper", [TAU, high - TAU], [-1.0, 0.0], TAU);
    let mut loops = Vec::new();
    for (key, sense, pcurve) in [
        ("lower", Sense::Forward, lower),
        ("upper", Sense::Reversed, upper),
    ] {
        let geometry = &mut sheet.ir.geometry;
        geometry.coedges.push(Coedge {
            id: id(CoedgeId::mint, "coedge", key),
            owner_loop: id(LoopId::mint, "loop", key),
            edge: id(EdgeId::mint, "edge", key),
            radial_next: id(CoedgeId::mint, "coedge", key),
            sense,
            pcurves: vec![pcurve],
            use_curve: None,
            native_ref: None,
        });
        geometry.loops.push(Loop {
            id: id(LoopId::mint, "loop", key),
            face: id(FaceId::mint, "face", "face"),
            boundary: LoopBoundary::Ring(
                LoopRing::new(vec![id(CoedgeId::mint, "coedge", key)], Vec::new()).expect("a ring"),
            ),
            native_ref: None,
        });
        loops.push(id(LoopId::mint, "loop", key));
    }
    sheet.faced(FaceLoops::unspecified(loops))
}

#[test]
fn a_torus_band_across_the_tube_seam_takes_its_domain_from_the_side_its_loops_bound() {
    for stretch in [STRETCH, MIRROR_STRETCH] {
        let read = read_back(torus_band_across_the_seam(), stretch);
        assert_eq!(read.geometry.faces.len(), 1);
        let points = support_points(&read)
            .into_iter()
            .map(|point| stretch.source(point))
            .collect::<Vec<_>>();
        let mut tube_angles = Vec::new();
        for point in &points {
            let radial = point.x.hypot(point.y) - TORUS_MAJOR;
            assert!(
                (radial.hypot(point.z) - TORUS_MINOR).abs() <= ON_SURFACE,
                "{point:?} lies off the torus"
            );
            let tube = point.z.atan2(radial);
            assert!(
                tube.abs() >= BAND_LOW - ON_SURFACE,
                "{point:?} at tube angle {tube} lies outside the band"
            );
            tube_angles.push(tube);
        }
        for bound in [BAND_LOW, -BAND_LOW] {
            assert!(
                tube_angles
                    .iter()
                    .any(|tube| (tube - bound).abs() <= ON_SURFACE),
                "the support misses the parallel at tube angle {bound}"
            );
        }
        let azimuths = points.iter().map(|point| point.y.atan2(point.x)).collect();
        assert!(
            largest_gap(azimuths) <= TURN_GAP,
            "the face misses azimuths"
        );
    }
}

/// The map that leaves each point in place.
const UNMOVED: Stretch = Stretch {
    scale: [1.0, 1.0, 1.0],
    shift: [0.0, 0.0, 0.0],
};

/// [`sphere_lune_with_pole_uses`] whose pole uses each cover the middle of a
/// parameter line that reaches one unit past each pole end, the south use
/// against the direction of its line.
fn sphere_lune_with_covering_pole_uses(sweep: f64) -> CadIr {
    let mut ir = sphere_lune_with_pole_uses(sweep);
    let half = PI / 2.0;
    let length = sweep + 2.0;
    let mut north = parameter_line(
        &mut ir,
        "north-long",
        [sweep + 1.0, half],
        [-1.0, 0.0],
        length,
    );
    north.parameter_range = Some(DirectedParameterRange::new([1.0, 1.0 + sweep]).expect("a range"));
    let mut south = parameter_line(
        &mut ir,
        "south-long",
        [sweep + 1.0, -half],
        [-1.0, 0.0],
        length,
    );
    south.parameter_range = Some(DirectedParameterRange::new([1.0 + sweep, 1.0]).expect("a range"));
    let LoopBoundary::Ring(ring) = &ir.geometry.loops[0].boundary else {
        panic!("the lune loop is a ring");
    };
    let mut vertex_uses = ring.vertex_uses().to_vec();
    vertex_uses[0].pcurves = vec![north];
    vertex_uses[1].pcurves = vec![south];
    let coedges = ring.coedges().to_vec();
    ir.geometry.loops[0].boundary =
        LoopBoundary::Ring(LoopRing::new(coedges, vertex_uses).expect("a ring"));
    ir.geometry.pcurves.retain(|pcurve| {
        pcurve.id != id(PcurveId::mint, "pcurve", "north")
            && pcurve.id != id(PcurveId::mint, "pcurve", "south")
    });
    ir.finalize();
    ir
}

#[test]
fn a_pole_use_over_part_of_its_parameter_curve_writes_only_the_part_it_covers() {
    for stretch in [UNMOVED, STRETCH, MIRROR_STRETCH] {
        let read = read_back(sphere_lune_with_covering_pole_uses(SECTOR), stretch);
        assert_lune_boundary(&read, stretch);
    }
}

/// The amount by which a use reaches past the end of its parameter curve,
/// within the tolerance on equal parameter ranges.
const NEAR_RANGE_GAP: f64 = 1.0e-12;

#[test]
fn a_use_almost_equal_to_the_range_of_its_parameter_curve_keeps_the_curve() {
    let mut ir = sphere_lune_with_pole_uses(SECTOR);
    ir.geometry
        .coedges
        .iter_mut()
        .find(|coedge| coedge.id == id(CoedgeId::mint, "coedge", "end"))
        .expect("the lune coedge")
        .pcurves[0]
        .parameter_range =
        Some(DirectedParameterRange::new([0.0, PI + NEAR_RANGE_GAP]).expect("a range"));
    for stretch in [STRETCH, MIRROR_STRETCH] {
        let read = read_back(ir.clone(), stretch);
        assert_lune_boundary(&read, stretch);
    }
}

/// Adds the loop `key` of the face `face` to `ir`: one coedge on the closed
/// edge `edge` in `sense`, which carries `pcurve`.
fn ring_loop(
    ir: &mut CadIr,
    [key, face, edge]: [&str; 3],
    sense: Sense,
    pcurve: PcurveUse,
) -> LoopId {
    let geometry = &mut ir.geometry;
    geometry.coedges.push(Coedge {
        id: id(CoedgeId::mint, "coedge", key),
        owner_loop: id(LoopId::mint, "loop", key),
        edge: id(EdgeId::mint, "edge", edge),
        radial_next: id(CoedgeId::mint, "coedge", key),
        sense,
        pcurves: vec![pcurve],
        use_curve: None,
        native_ref: None,
    });
    geometry.loops.push(Loop {
        id: id(LoopId::mint, "loop", key),
        face: id(FaceId::mint, "face", face),
        boundary: LoopBoundary::Ring(
            LoopRing::new(vec![id(CoedgeId::mint, "coedge", key)], Vec::new()).expect("a ring"),
        ),
        native_ref: None,
    });
    id(LoopId::mint, "loop", key)
}

/// The height of the upper circle of the cylinder band.
const BAND_HEIGHT: f64 = 8.0;

/// The band of the cylinder about `+z` between the circles at heights 0 and
/// [`BAND_HEIGHT`], each one closed edge in a loop of its own. The lower
/// parameter line runs over azimuths `[0, 2π]`, the upper one over
/// `[4π, 2π]`, one turn further.
fn cylinder_band_across_two_periods() -> CadIr {
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
        sheet.vertex(key, Point3::new(CYLINDER_RADIUS, 0.0, height));
        let circle = CircleCurve::try_new(
            Point3::new(0.0, 0.0, height),
            z_axis(),
            Vector3::new(1.0, 0.0, 0.0),
            CYLINDER_RADIUS,
        )
        .expect("a circle");
        sheet.edge(
            key,
            SolvedCurveGeometry::Circle(circle),
            [0.0, TAU],
            [key, key],
        );
    }
    let lower = parameter_line(&mut sheet.ir, "lower", [0.0, 0.0], [1.0, 0.0], TAU);
    let upper = parameter_line(
        &mut sheet.ir,
        "upper",
        [2.0 * TAU, BAND_HEIGHT],
        [-1.0, 0.0],
        TAU,
    );
    let loops = vec![
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
    sheet.faced(FaceLoops::unspecified(loops))
}

#[test]
fn a_face_whose_loops_lie_one_turn_apart_moves_them_into_one_period() {
    for stretch in [STRETCH, MIRROR_STRETCH] {
        let read = read_back(cylinder_band_across_two_periods(), stretch);
        assert_eq!(read.geometry.faces.len(), 1);
        let points = support_points(&read)
            .into_iter()
            .map(|point| stretch.source(point))
            .collect::<Vec<_>>();
        for point in &points {
            assert!(
                (point.x.hypot(point.y) - CYLINDER_RADIUS).abs() <= ON_SURFACE,
                "{point:?} lies off the cylinder"
            );
            assert!(
                (-ON_SURFACE..=BAND_HEIGHT + ON_SURFACE).contains(&point.z),
                "{point:?} lies outside the band"
            );
        }
        for height in [0.0, BAND_HEIGHT] {
            assert!(
                points
                    .iter()
                    .any(|point| (point.z - height).abs() <= ON_SURFACE),
                "the support misses the circle at height {height}"
            );
        }
        let azimuths = points.iter().map(|point| point.y.atan2(point.x)).collect();
        assert!(
            largest_gap(azimuths) <= TURN_GAP,
            "the face misses azimuths"
        );
    }
}

/// The tube angle of the upper parallel of the second torus band.
const SECOND_BAND_HIGH: f64 = PI / 2.0;

/// Two faces on one torus about `+z`. The first is
/// [`torus_band_across_the_seam`]; the second is the band from its upper
/// parallel at tube angle `-BAND_LOW` to the parallel at
/// [`SECOND_BAND_HIGH`]. The two faces use one parameter line on their common
/// parallel, stated with the tube angle `-BAND_LOW`.
fn torus_bands_on_a_common_parallel() -> CadIr {
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
    for (key, v) in [
        ("lower", BAND_LOW),
        ("common", -BAND_LOW),
        ("upper", SECOND_BAND_HIGH),
    ] {
        sheet.vertex(key, torus_point(0.0, v));
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
            [0.0, TAU],
            [key, key],
        );
    }
    let ir = &mut sheet.ir;
    let lower = parameter_line(ir, "lower", [0.0, BAND_LOW], [1.0, 0.0], TAU);
    let common = parameter_line(ir, "common", [0.0, -BAND_LOW], [1.0, 0.0], TAU);
    let upper = parameter_line(ir, "upper", [TAU, SECOND_BAND_HIGH], [-1.0, 0.0], TAU);
    let mut backward = common.clone();
    backward.parameter_range = Some(DirectedParameterRange::new([TAU, 0.0]).expect("a range"));
    let first = vec![
        ring_loop(ir, ["lower", "face", "lower"], Sense::Forward, lower),
        ring_loop(
            ir,
            ["common-first", "face", "common"],
            Sense::Reversed,
            backward,
        ),
    ];
    let second = vec![
        ring_loop(
            ir,
            ["common-second", "second", "common"],
            Sense::Forward,
            common,
        ),
        ring_loop(ir, ["upper", "second", "upper"], Sense::Reversed, upper),
    ];
    for (coedge, next) in [
        ("common-first", "common-second"),
        ("common-second", "common-first"),
    ] {
        ir.geometry
            .coedges
            .iter_mut()
            .find(|record| record.id == id(CoedgeId::mint, "coedge", coedge))
            .expect("the common coedge")
            .radial_next = id(CoedgeId::mint, "coedge", next);
    }
    let mut ir = sheet.faced(FaceLoops::unspecified(first));
    let mut face = ir.geometry.faces[0].clone();
    face.id = id(FaceId::mint, "face", "second");
    face.loops = FaceLoops::unspecified(second);
    ir.geometry.faces.push(face);
    ir.finalize();
    ir
}

/// The tube angle of the point of the support of each face of `read` midway
/// in the support parameters between the two loops of the face.
fn tube_angles_between_loops(read: &CadIr, stretch: Stretch) -> Vec<f64> {
    let geometry = &read.geometry;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
        .expect("an evaluation context");
    geometry
        .faces
        .iter()
        .map(|face| {
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
            let middles = face
                .loops
                .stated()
                .map(|loop_id| {
                    let loop_ = geometry
                        .loops
                        .iter()
                        .find(|loop_| loop_.id == *loop_id)
                        .expect("the face loop reads back");
                    let coedge = geometry
                        .coedges
                        .iter()
                        .find(|coedge| coedge.id == loop_.coedges()[0])
                        .expect("the loop coedge reads back");
                    let pcurve = geometry
                        .pcurves
                        .iter()
                        .find(|pcurve| pcurve.id == coedge.pcurves[0].pcurve)
                        .expect("the parameter curve reads back");
                    let [start, end] = pcurve
                        .parameter_range()
                        .expect("a bounded parameter curve")
                        .get();
                    pcurve_uv(&ctx, &pcurve.geometry, 0.5 * (start + end))
                        .expect("a parameter point")
                        .get()
                })
                .collect::<Vec<_>>();
            assert_eq!(middles.len(), 2, "{middles:?}");
            let point = surface_point_solved(
                &ctx,
                support,
                0.5 * (middles[0].u + middles[1].u),
                0.5 * (middles[0].v + middles[1].v),
            )
            .expect("a point on the support")
            .get();
            let point = stretch.source(point);
            let radial = point.x.hypot(point.y) - TORUS_MAJOR;
            point.z.atan2(radial)
        })
        .collect()
}

#[test]
fn faces_that_place_a_common_parameter_curve_at_different_periods_each_write_their_own() {
    for stretch in [STRETCH, MIRROR_STRETCH] {
        let read = read_back(torus_bands_on_a_common_parallel(), stretch);
        assert_eq!(read.geometry.faces.len(), 2);
        let tubes = tube_angles_between_loops(&read, stretch);
        assert!(
            tubes.iter().any(|tube| tube.abs() > BAND_LOW),
            "no face lies across the tube seam: {tubes:?}"
        );
        assert!(
            tubes
                .iter()
                .any(|tube| (-BAND_LOW..SECOND_BAND_HIGH).contains(tube)),
            "no face lies between its common and upper parallels: {tubes:?}"
        );
    }
}

/// The latitude of the parallel that bounds the sphere cap.
const CAP_LATITUDE: f64 = PI / 6.0;

/// The cap of the sphere about `+z` north of the parallel at latitude
/// [`CAP_LATITUDE`]: one closed edge on that parallel, whose coedge carries
/// its parameter line.
fn sphere_cap() -> CadIr {
    let r = SPHERE_RADIUS;
    let mut sheet = Sheet::new(SolvedSurfaceGeometry::Sphere(
        SphereSurface::try_new(origin(), z_axis(), Vector3::new(1.0, 0.0, 0.0), r)
            .expect("a sphere"),
    ));
    let height = r * CAP_LATITUDE.sin();
    let radius = r * CAP_LATITUDE.cos();
    sheet.vertex("seam", Point3::new(radius, 0.0, height));
    let parallel = CircleCurve::try_new(
        Point3::new(0.0, 0.0, height),
        z_axis(),
        Vector3::new(1.0, 0.0, 0.0),
        radius,
    )
    .expect("a parallel");
    sheet.edge(
        "parallel",
        SolvedCurveGeometry::Circle(parallel),
        [0.0, TAU],
        ["seam", "seam"],
    );
    let line = parameter_line(
        &mut sheet.ir,
        "parallel",
        [0.0, CAP_LATITUDE],
        [1.0, 0.0],
        TAU,
    );
    let loops = vec![ring_loop(
        &mut sheet.ir,
        ["parallel", "face", "parallel"],
        Sense::Forward,
        line,
    )];
    sheet.faced(FaceLoops::unspecified(loops))
}

#[test]
fn a_sphere_cap_with_a_stated_parameter_curve_reaches_its_pole() {
    let r = SPHERE_RADIUS;
    let height = r * CAP_LATITUDE.sin();
    for stretch in [STRETCH, MIRROR_STRETCH] {
        let read = read_back(sphere_cap(), stretch);
        assert_eq!(read.geometry.faces.len(), 1);
        let points = support_points(&read)
            .into_iter()
            .map(|point| stretch.source(point))
            .collect::<Vec<_>>();
        for point in &points {
            assert!(
                (point.x.hypot(point.y).hypot(point.z) - r).abs() <= ON_SURFACE,
                "{point:?} lies off the sphere"
            );
            assert!(
                point.z >= height - ON_SURFACE,
                "{point:?} lies south of the cap"
            );
        }
        assert!(
            points.iter().any(|point| (point.z - r).abs() <= ON_SURFACE),
            "the support misses the pole"
        );
        let (boundary_points, _) = boundary(&read);
        for point in boundary_points
            .into_iter()
            .map(|point| stretch.source(point))
        {
            assert!(
                (point.z - height).abs() <= ON_SURFACE,
                "{point:?} lies off the parallel"
            );
        }
    }
}
