// SPDX-License-Identifier: Apache-2.0
//! Document builders and the decode context the crate's tests measure with.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::geometry::analytic::{
    CircleCurve, ConeSurface, CylinderSurface, LineCurve, PlaneSurface,
};
use cadmpeg_ir::geometry::pcurve::{LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, DirectedParameterRange, SolvedCurveGeometry, SolvedSurfaceGeometry,
    Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing,
    PcurveUse, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::CadIr;

use crate::report::MeasureReport;
use crate::select::Selection;
use crate::session::{MeasureInput, MeasureOptions, Measurer};

/// A service-policy decode context for tests that read no source file.
pub(crate) fn service_context() -> DecodeContext<'static> {
    let arena = Box::leak(Box::new(DecodeArena::new()));
    DecodeContext::from_root_bytes(&[], arena, &DecodePolicy::service())
        .expect("empty test input fits the service policy")
        .0
}

/// Measure `ir` with `selection` under the desktop profile.
pub(crate) fn measure(ir: &CadIr, selection: &Selection) -> MeasureReport {
    let ctx = service_context();
    let input = MeasureInput {
        ir,
        fidelity: None,
        geometry_transferred: true,
    };
    let report = Measurer::new(input, MeasureOptions::default())
        .measure(&ctx, selection)
        .expect("measurement completes");
    ctx.finish_session().expect("session stays unrefused");
    report
}

/// The measured value of a scalar quantity.
pub(crate) fn value(quantity: &crate::value::Quantity<crate::value::Scalar>) -> f64 {
    match quantity {
        crate::value::Quantity::Measured(measured) => measured.value().value.get(),
        crate::value::Quantity::Unavailable(unavailable) => panic!(
            "expected a measured value, got {} ({})",
            unavailable.reason().code(),
            unavailable.message()
        ),
    }
}

/// `|computed − expected| ≤ numeric error + TEST_ABSOLUTE_FLOOR × max(|expected|, scale^d)`.
pub(crate) const TEST_ABSOLUTE_FLOOR: f64 = 1.0e-9;

/// Assert a measured scalar equals `expected` within its numeric error and
/// the test floor relative to `scale`.
pub(crate) fn assert_measured(
    quantity: &crate::value::Quantity<crate::value::Scalar>,
    expected: f64,
    scale: f64,
) {
    let computed = value(quantity);
    let numeric = quantity
        .measured()
        .map_or(0.0, |measured| measured.numeric_error().get());
    let allowed = numeric + TEST_ABSOLUTE_FLOOR * expected.abs().max(scale);
    assert!(
        (computed - expected).abs() <= allowed,
        "computed {computed}, expected {expected}, allowed {allowed}"
    );
}

/// An origin with an axis and a reference direction.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Frame {
    pub(crate) origin: [f64; 3],
    pub(crate) axis: [f64; 3],
    pub(crate) reference: [f64; 3],
}

impl Frame {
    /// The frame at `origin` with axis `+z` and reference `+x`.
    pub(crate) const fn z_up(origin: [f64; 3]) -> Self {
        Self {
            origin,
            axis: [0.0, 0.0, 1.0],
            reference: [1.0, 0.0, 0.0],
        }
    }

    fn vectors(self) -> (Point3, Vector3, Vector3) {
        let [ox, oy, oz] = self.origin;
        let [ax, ay, az] = self.axis;
        let [rx, ry, rz] = self.reference;
        (
            Point3::new(ox, oy, oz),
            Vector3::new(ax, ay, az),
            Vector3::new(rx, ry, rz),
        )
    }
}

/// A circle: its frame and radius.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Circle {
    pub(crate) frame: Frame,
    pub(crate) radius: f64,
}

impl Circle {
    fn curve(self) -> CircleCurve {
        let (center, axis, reference) = self.frame.vectors();
        CircleCurve::try_new(center, axis, reference, self.radius).expect("circle")
    }
}

/// Assert a measured range equals `[min, max]` within its numeric error
/// and the test floor relative to `scale`.
pub(crate) fn assert_range(
    quantity: Option<&crate::value::Quantity<crate::value::Range>>,
    [min, max]: [f64; 2],
    scale: f64,
) {
    let measured = quantity
        .and_then(crate::value::Quantity::measured)
        .unwrap_or_else(|| panic!("expected a measured range, got {quantity:?}"));
    let numeric = measured.numeric_error().get();
    for (computed, expected) in [
        (measured.value().min.get(), min),
        (measured.value().max.get(), max),
    ] {
        let allowed = numeric + TEST_ABSOLUTE_FLOOR * expected.abs().max(scale);
        assert!(
            (computed - expected).abs() <= allowed,
            "computed {computed}, expected {expected}, allowed {allowed}"
        );
    }
}

/// Assert a measured tight box equals `min .. max` within its numeric error
/// and the test floor relative to `scale`.
pub(crate) fn assert_tight_box(
    quantity: &crate::value::Quantity<crate::value::BoxValue>,
    min: [f64; 3],
    max: [f64; 3],
    scale: f64,
) {
    let measured = quantity
        .measured()
        .unwrap_or_else(|| panic!("expected a measured box, got {quantity:?}"));
    assert_eq!(measured.value().kind, crate::value::BoxKind::Tight);
    let numeric = measured.numeric_error().get();
    let (low, high) = (measured.value().min.get(), measured.value().max.get());
    for (computed, expected) in [
        (low.x, min[0]),
        (low.y, min[1]),
        (low.z, min[2]),
        (high.x, max[0]),
        (high.y, max[1]),
        (high.z, max[2]),
    ] {
        let allowed = numeric + TEST_ABSOLUTE_FLOOR * expected.abs().max(scale);
        assert!(
            (computed - expected).abs() <= allowed,
            "computed {computed}, expected {expected}, allowed {allowed}"
        );
    }
}

/// Builds a document from vertices, edges and faces.
pub(crate) struct Builder {
    ir: CadIr,
    next: usize,
    faces: Vec<FaceId>,
}

fn id<T>(mint: impl Fn(String) -> Result<T, cadmpeg_ir::IdentityError>, kind: &str, n: usize) -> T {
    mint(format!("test:model:{kind}#{n:04}")).expect("test identity grammar")
}

impl Builder {
    pub(crate) fn new() -> Self {
        Self {
            ir: CadIr::empty(),
            next: 0,
            faces: Vec::new(),
        }
    }

    fn serial(&mut self) -> usize {
        self.next += 1;
        self.next
    }

    /// A vertex at `position`.
    pub(crate) fn vertex(&mut self, position: [f64; 3]) -> VertexId {
        let n = self.serial();
        let point = id(PointId::mint, "point", n);
        let vertex = id(VertexId::mint, "vertex", n);
        let [x, y, z] = position;
        self.ir.model.points.push(Point::new(
            point.clone(),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(x, y, z)).expect("finite"),
            None,
        ));
        self.ir.model.vertices.push(Vertex {
            id: vertex.clone(),
            point,
            tolerance: None,
        });
        vertex
    }

    fn position(&self, vertex: &VertexId) -> Point3 {
        let point = self
            .ir
            .model
            .vertices
            .iter()
            .find(|candidate| candidate.id == *vertex)
            .map(|vertex| vertex.point.clone())
            .expect("vertex exists");
        self.ir
            .model
            .points
            .iter()
            .find(|candidate| candidate.id == point)
            .map(|point| point.position().get())
            .expect("point exists")
    }

    fn curve(&mut self, geometry: SolvedCurveGeometry) -> CurveId {
        let n = self.serial();
        let curve = id(CurveId::mint, "curve", n);
        self.ir.model.curves.push(Curve {
            id: curve.clone(),
            geometry: CurveGeometry::Solved(geometry),
            parameter_range: None,
            source_object: None,
        });
        curve
    }

    fn edge(&mut self, carrier: EdgeCarrier, start: VertexId, end: VertexId) -> EdgeId {
        let n = self.serial();
        let edge = id(EdgeId::mint, "edge", n);
        self.ir.model.edges.push(Edge {
            id: edge.clone(),
            carrier,
            start,
            end,
            tolerance: None,
        });
        edge
    }

    /// A straight edge from `start` to `end` on `[0, length]`.
    pub(crate) fn line(&mut self, start: &VertexId, end: &VertexId) -> EdgeId {
        let a = self.position(start);
        let b = self.position(end);
        let direction = b.vector_from(a);
        let length = direction.norm();
        let curve = self.curve(SolvedCurveGeometry::Line(
            LineCurve::try_new(a, direction.scale(1.0 / length)).expect("line"),
        ));
        self.edge(
            EdgeCarrier::new(Some(curve), Some([0.0, length])).expect("carrier"),
            start.clone(),
            end.clone(),
        )
    }

    /// A closed circle edge through `vertex`, with no parameter range.
    pub(crate) fn circle(
        &mut self,
        center: [f64; 3],
        axis: [f64; 3],
        reference: [f64; 3],
        radius: f64,
        vertex: &VertexId,
    ) -> EdgeId {
        let [cx, cy, cz] = center;
        let [ax, ay, az] = axis;
        let [rx, ry, rz] = reference;
        let curve = self.curve(SolvedCurveGeometry::Circle(
            CircleCurve::try_new(
                Point3::new(cx, cy, cz),
                Vector3::new(ax, ay, az),
                Vector3::new(rx, ry, rz),
                radius,
            )
            .expect("circle"),
        ));
        self.edge(
            EdgeCarrier::unbounded(Some(curve)),
            vertex.clone(),
            vertex.clone(),
        )
    }

    /// A circular arc edge from `start` to `end` on the parameter interval
    /// `range` of the circle `c + r(cos t·reference + sin t·(axis × reference))`.
    pub(crate) fn arc(
        &mut self,
        circle: Circle,
        range: [f64; 2],
        start: &VertexId,
        end: &VertexId,
    ) -> EdgeId {
        let curve = self.curve(SolvedCurveGeometry::Circle(circle.curve()));
        self.edge(
            EdgeCarrier::new(Some(curve), Some(range)).expect("carrier"),
            start.clone(),
            end.clone(),
        )
    }

    /// An edge with no 3D curve.
    pub(crate) fn bare_edge(&mut self, start: &VertexId, end: &VertexId) -> EdgeId {
        self.edge(EdgeCarrier::unbounded(None), start.clone(), end.clone())
    }

    /// Append the line pcurve `origin + t·direction` on `range` to the
    /// pcurve uses of the coedge at `position` of ring `ring` of `face`.
    pub(crate) fn line_pcurve(
        &mut self,
        face: &FaceId,
        [ring, position]: [usize; 2],
        origin: [f64; 2],
        direction: [f64; 2],
        range: [f64; 2],
    ) {
        let n = self.serial();
        let pcurve = id(PcurveId::mint, "pcurve", n);
        self.ir.model.pcurves.push(Pcurve {
            id: pcurve.clone(),
            geometry: PcurveGeometry::Line(
                LinePcurve::try_new(
                    Point2::new(origin[0], origin[1]),
                    Point2::new(direction[0], direction[1]),
                )
                .expect("line pcurve"),
            ),
            metadata: PcurveMetadata::default(),
        });
        let loop_id = self
            .ir
            .model
            .faces
            .iter()
            .find(|candidate| candidate.id == *face)
            .and_then(|face| face.loops.iter().nth(ring).cloned())
            .expect("face loop");
        let coedge_id = self
            .ir
            .model
            .loops
            .iter()
            .find(|candidate| candidate.id == loop_id)
            .and_then(|face_loop| face_loop.coedges().get(position).cloned())
            .expect("loop coedge");
        let coedge = self
            .ir
            .model
            .coedges
            .iter_mut()
            .find(|candidate| candidate.id == coedge_id)
            .expect("coedge");
        coedge.pcurves.push(PcurveUse {
            pcurve,
            isoparametric: None,
            parameter_range: Some(DirectedParameterRange::new(range).expect("range")),
        });
    }

    /// A cylinder surface.
    pub(crate) fn cylinder(&mut self, frame: Frame, radius: f64) -> SurfaceId {
        let (origin, axis, reference) = frame.vectors();
        self.surface(SolvedSurfaceGeometry::Cylinder(
            CylinderSurface::try_new(origin, axis, reference, radius).expect("cylinder"),
        ))
    }

    /// A circular cone surface of `radius` at the frame origin.
    pub(crate) fn cone(&mut self, frame: Frame, radius: f64, half_angle: f64) -> SurfaceId {
        let (origin, axis, reference) = frame.vectors();
        self.surface(SolvedSurfaceGeometry::Cone(
            ConeSurface::try_new(origin, axis, reference, radius, 1.0, half_angle).expect("cone"),
        ))
    }

    /// A plane surface.
    pub(crate) fn plane(
        &mut self,
        origin: [f64; 3],
        normal: [f64; 3],
        u_axis: [f64; 3],
    ) -> SurfaceId {
        let [ox, oy, oz] = origin;
        let [nx, ny, nz] = normal;
        let [ux, uy, uz] = u_axis;
        self.surface(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(ox, oy, oz),
                Vector3::new(nx, ny, nz),
                Vector3::new(ux, uy, uz),
            )
            .expect("plane"),
        ))
    }

    /// A surface of `geometry`.
    pub(crate) fn surface(&mut self, geometry: SolvedSurfaceGeometry) -> SurfaceId {
        let n = self.serial();
        let surface = id(SurfaceId::mint, "surface", n);
        self.ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Solved(geometry),
            source_object: None,
        });
        surface
    }

    /// A face on `surface` bounded by `rings`, each a list of edges with the
    /// coedge sense. With `classified`, the first ring is the outer loop.
    pub(crate) fn face(
        &mut self,
        surface: &SurfaceId,
        sense: Sense,
        rings: &[Vec<(EdgeId, Sense)>],
        classified: bool,
    ) -> FaceId {
        let n = self.serial();
        let face = id(FaceId::mint, "face", n);
        let mut loops = Vec::new();
        for ring in rings {
            let m = self.serial();
            let loop_id = id(LoopId::mint, "loop", m);
            let mut coedges = Vec::new();
            for (edge, coedge_sense) in ring {
                let k = self.serial();
                let coedge = id(CoedgeId::mint, "coedge", k);
                self.ir.model.coedges.push(Coedge {
                    id: coedge.clone(),
                    owner_loop: loop_id.clone(),
                    edge: edge.clone(),
                    radial_next: coedge.clone(),
                    sense: *coedge_sense,
                    pcurves: Vec::new(),
                    use_curve: None,
                });
                coedges.push(coedge);
            }
            self.ir.model.loops.push(Loop {
                id: loop_id.clone(),
                face: face.clone(),
                boundary: LoopBoundary::Ring(LoopRing::new(coedges, Vec::new()).expect("ring")),
            });
            loops.push(loop_id);
        }
        let loops = if classified {
            let mut iter = loops.into_iter();
            let outer = iter.next().expect("an outer loop");
            FaceLoops::classified(outer, iter.collect())
        } else {
            FaceLoops::unspecified(loops)
        };
        self.ir.model.faces.push(Face {
            id: face.clone(),
            shell: id(ShellId::mint, "shell", 0),
            surface: surface.clone(),
            sense,
            loops,
            name: None,
            color: None,
            tolerance: None,
        });
        self.faces.push(face.clone());
        face
    }

    /// Reverse the listed coedge order of `face`'s ring at `index`.
    pub(crate) fn reverse_listing(&mut self, face: &FaceId, index: usize) {
        let loop_id = self
            .ir
            .model
            .faces
            .iter()
            .find(|candidate| candidate.id == *face)
            .and_then(|face| face.loops.iter().nth(index).cloned())
            .expect("face loop");
        let face_loop = self
            .ir
            .model
            .loops
            .iter_mut()
            .find(|candidate| candidate.id == loop_id)
            .expect("loop");
        let mut coedges = face_loop.coedges().to_vec();
        coedges.reverse();
        face_loop.replace_ring(coedges, Vec::new()).expect("ring");
    }

    /// One body of `kind` whose single shell holds every face built so far,
    /// or every edge when no face was built.
    pub(crate) fn finish(mut self, kind: BodyKind) -> CadIr {
        let shell = id(ShellId::mint, "shell", 0);
        let region = id(RegionId::mint, "region", 0);
        let body = id(BodyId::mint, "body", 0);
        let wire_edges = if self.faces.is_empty() {
            self.ir
                .model
                .edges
                .iter()
                .map(|edge| edge.id.clone())
                .collect()
        } else {
            Vec::new()
        };
        self.ir.model.shells.push(
            Shell::new(
                shell.clone(),
                region.clone(),
                self.faces.clone(),
                wire_edges,
                Vec::new(),
            )
            .expect("shell"),
        );
        self.ir.model.regions.push(Region {
            id: region.clone(),
            body: body.clone(),
            shells: vec![shell],
        });
        self.ir.model.bodies.push(Body {
            id: body,
            kind,
            regions: vec![region],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        let coedges = self.ir.model.coedges.clone();
        for coedge in &mut self.ir.model.coedges {
            let partners: Vec<&Coedge> = coedges
                .iter()
                .filter(|other| other.edge == coedge.edge)
                .collect();
            let position = partners
                .iter()
                .position(|other| other.id == coedge.id)
                .expect("coedge is its own partner");
            coedge.radial_next = partners[(position + 1) % partners.len()].id.clone();
        }
        self.ir.finalize();
        self.ir
    }
}

/// The plate of #188 case 3: the square `[0, 40]²` at `z = 0` with a hole of
/// diameter 20 centred at `(20, 20)`.
pub(crate) fn plate_with_hole(classified: bool) -> CadIr {
    let mut builder = Builder::new();
    let corners = [
        builder.vertex([0.0, 0.0, 0.0]),
        builder.vertex([40.0, 0.0, 0.0]),
        builder.vertex([40.0, 40.0, 0.0]),
        builder.vertex([0.0, 40.0, 0.0]),
    ];
    let sides: Vec<(EdgeId, Sense)> = (0..4)
        .map(|index| {
            (
                builder.line(&corners[index], &corners[(index + 1) % 4]),
                Sense::Forward,
            )
        })
        .collect();
    let seam = builder.vertex([30.0, 20.0, 0.0]);
    let hole = builder.circle(
        [20.0, 20.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        10.0,
        &seam,
    );
    let surface = builder.plane([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    builder.face(
        &surface,
        Sense::Forward,
        &[sides, vec![(hole, Sense::Reversed)]],
        classified,
    );
    builder.finish(BodyKind::Sheet)
}
