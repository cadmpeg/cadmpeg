// SPDX-License-Identifier: Apache-2.0
//! The B-rep records of an IR body.
//!
//! Each carrier maps to a record by a parameter map: an affine map of each
//! parameter, with a flag when the map reverses the surface normal. A face
//! whose surface has no record, and an edge whose curve has no record, are
//! counted and the B-rep is written with `brep:valid = false`.

use std::collections::BTreeMap;

use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsSurface};
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::geometry::{
    CurveGeometry, ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, EdgeId, FaceId, SurfaceId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Body, BodyKind, Coedge, LoopBoundary, Sense};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use cadmpeg_tessellate::{CoedgeTrace, FaceMesh, Tessellator};

use crate::brep::nurbs::{harmonic_arc, Nurbs};
use crate::brep::{
    AnalyticCurve, AnalyticPcurve, AnalyticSurface, Brep, Catalog, CurveKind, EdgeRecord,
    FaceRecord, Form, Frame, LoopRecord, NurbsCurveRecord, NurbsSurfaceRecord, OrientedEdgeRecord,
    PcurveKind, Reference, ShellRecord, SolidRecord, SurfaceKind,
};

/// Relative tolerance of a similarity transform's column lengths and of
/// orthonormal pcurve axes.
const SIMILARITY_TOLERANCE: f64 = 1e-9;
/// Factor by which a measured surface deviation is widened when it becomes
/// the stated tolerance, so that a validator's own sampling stays inside it.
const DEVIATION_MARGIN: f64 = 2.0;
/// Distance in parameter units within which a pcurve end lies on a pole.
const POLE_TOLERANCE: f64 = 1e-7;
/// Axial distance from a cone apex to the origin of its written frame, in
/// millimetres.
const APEX_OFFSET: f64 = 1.0;

/// An affine map `t' = scale · t + offset` from an IR parameter to a record
/// parameter.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Affine1 {
    scale: f64,
    offset: f64,
}

impl Affine1 {
    const IDENTITY: Self = Self {
        scale: 1.0,
        offset: 0.0,
    };

    fn apply(self, value: f64) -> f64 {
        self.scale * value + self.offset
    }

    fn then(self, next: Self) -> Self {
        Self {
            scale: next.scale * self.scale,
            offset: next.scale * self.offset + next.offset,
        }
    }
}

/// The map from IR `(u, v)` to record `(u, v)`. `normal_flipped` is true
/// when the record's normal is opposite the IR surface normal.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Chart {
    u: Affine1,
    v: Affine1,
    normal_flipped: bool,
}

impl Chart {
    const IDENTITY: Self = Self {
        u: Affine1::IDENTITY,
        v: Affine1::IDENTITY,
        normal_flipped: false,
    };

    fn apply(self, [u, v]: [f64; 2]) -> [f64; 2] {
        [self.u.apply(u), self.v.apply(v)]
    }
}

fn raw(vector: &Vector3) -> [f64; 3] {
    [vector.x, vector.y, vector.z]
}

fn point(point: Point3) -> [f64; 3] {
    [point.x, point.y, point.z]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn negated(a: [f64; 3]) -> [f64; 3] {
    a.map(|value| -value)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let length = dot(a, a).sqrt();
    (length.is_finite() && length > 0.0).then(|| a.map(|value| value / length))
}

/// A unit vector perpendicular to `axis`.
fn perpendicular(axis: [f64; 3]) -> [f64; 3] {
    let helper = if axis[0].abs() <= axis[1].abs() && axis[0].abs() <= axis[2].abs() {
        [1.0, 0.0, 0.0]
    } else if axis[1].abs() <= axis[2].abs() {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    unit(cross(axis, helper)).unwrap_or([1.0, 0.0, 0.0])
}

/// The scale of a transform whose linear part is a positive similarity.
fn similarity(transform: &Transform) -> Option<f64> {
    let rows = transform.affine_rows();
    let column = |axis: usize| [rows[0][axis], rows[1][axis], rows[2][axis]];
    let columns = [column(0), column(1), column(2)];
    let scale = dot(columns[0], columns[0]).sqrt();
    if !(scale.is_finite() && scale > 0.0) {
        return None;
    }
    for i in 0..3 {
        for j in 0..3 {
            let expected = if i == j { scale * scale } else { 0.0 };
            if (dot(columns[i], columns[j]) - expected).abs() > SIMILARITY_TOLERANCE * scale * scale
            {
                return None;
            }
        }
    }
    (transform.orientation()? > 0.0).then_some(scale)
}

fn transform_point(transform: &Transform, value: [f64; 3]) -> Option<[f64; 3]> {
    transform
        .apply_point(Point3::new(value[0], value[1], value[2]))
        .map(|point| [point.get().x, point.get().y, point.get().z])
}

fn transform_direction(transform: &Transform, value: [f64; 3]) -> Option<[f64; 3]> {
    let vector = transform.apply_vector(Vector3::new(value[0], value[1], value[2]))?;
    unit([vector.get().x, vector.get().y, vector.get().z])
}

fn place_frame(transform: &Transform, frame: &Frame) -> Option<Frame> {
    Some(Frame {
        position: transform_point(transform, frame.position)?,
        axis: transform_direction(transform, frame.axis)?,
        ref_direction: transform_direction(transform, frame.ref_direction)?,
    })
}

/// A record of either catalog, before it is added to the B-rep.
enum CurveRecord {
    Analytic(AnalyticCurve),
    Nurbs(NurbsCurveRecord<3>),
}

enum SurfaceRecord {
    Analytic(AnalyticSurface),
    Nurbs(NurbsSurfaceRecord),
}

fn nurbs_curve_record(nurbs: &NurbsCurve) -> Option<NurbsCurveRecord<3>> {
    let points: Vec<[f64; 3]> = nurbs
        .control_points()
        .into_iter()
        .map(|point_value| point(point_value.get()))
        .collect();
    let weights = nurbs.weights().map_or_else(
        || std::iter::repeat_n(1.0, points.len()).collect(),
        |weights| {
            weights
                .iter()
                .map(|weight| weight.get())
                .collect::<Vec<_>>()
        },
    );
    if weights.iter().any(|weight| *weight <= 0.0) {
        return None;
    }
    Some(NurbsCurveRecord {
        order: usize::try_from(nurbs.degree()).ok()? + 1,
        points,
        weights,
        knots: nurbs.knots().as_slice().to_vec(),
        form: if nurbs.periodic() {
            Form::Periodic
        } else {
            Form::Open
        },
    })
}

fn nurbs_surface_record(nurbs: &NurbsSurface) -> Option<NurbsSurfaceRecord> {
    let (u_count, v_count) = (nurbs.u_count(), nurbs.v_count());
    let grid = nurbs.control_grid();
    let weights = nurbs.weights();
    let mut points = Vec::new();
    let mut lane = Vec::new();
    for i in 0..u_count {
        for j in 0..v_count {
            points.push(point(grid.get(i)?.get(j)?.get()));
            let weight = match &weights {
                Some(weights) => weights.get(i)?.get(j)?.get(),
                None => 1.0,
            };
            if weight <= 0.0 {
                return None;
            }
            lane.push(weight);
        }
    }
    let form = |periodic: bool| if periodic { Form::Periodic } else { Form::Open };
    Some(NurbsSurfaceRecord {
        u_order: usize::try_from(nurbs.u_degree()).ok()? + 1,
        v_order: usize::try_from(nurbs.v_degree()).ok()? + 1,
        u_count,
        v_count,
        u_form: form(nurbs.u_periodic()),
        v_form: form(nurbs.v_periodic()),
        points,
        weights: lane,
        u_knots: nurbs.u_knots().as_slice().to_vec(),
        v_knots: nurbs.v_knots().as_slice().to_vec(),
    })
}

/// The record of a solved curve and the map from its IR parameter.
fn solved_curve(geometry: &SolvedCurveGeometry) -> Option<(CurveRecord, Affine1)> {
    let frame = |origin: Point3, axis: &Vector3, reference: &Vector3| Frame {
        position: point(origin),
        axis: raw(axis),
        ref_direction: raw(reference),
    };
    let analytic = |kind, frame, params| {
        Some((
            CurveRecord::Analytic(AnalyticCurve {
                kind,
                frame,
                params,
            }),
            Affine1::IDENTITY,
        ))
    };
    match geometry {
        SolvedCurveGeometry::Line(line) => analytic(
            CurveKind::Line,
            Frame {
                position: point(line.origin().get()),
                axis: raw(line.direction().as_raw()),
                ref_direction: [0.0; 3],
            },
            Vec::new(),
        ),
        SolvedCurveGeometry::Circle(circle) => analytic(
            CurveKind::Circle,
            frame(
                circle.center().get(),
                circle.frame().axis().as_raw(),
                circle.frame().reference().as_raw(),
            ),
            vec![circle.radius().get()],
        ),
        SolvedCurveGeometry::Ellipse(ellipse) => {
            let (major, minor) = (ellipse.major_radius().get(), ellipse.minor_radius().get());
            let axis = raw(ellipse.frame().axis().as_raw());
            let reference = raw(ellipse.frame().reference().as_raw());
            if major >= minor {
                return analytic(
                    CurveKind::Ellipse,
                    Frame {
                        position: point(ellipse.center().get()),
                        axis,
                        ref_direction: reference,
                    },
                    vec![major, minor],
                );
            }
            // Re-seat on the longer axis: X' = Z × X and t' = t − π/2.
            Some((
                CurveRecord::Analytic(AnalyticCurve {
                    kind: CurveKind::Ellipse,
                    frame: Frame {
                        position: point(ellipse.center().get()),
                        axis,
                        ref_direction: cross(axis, reference),
                    },
                    params: vec![minor, major],
                }),
                Affine1 {
                    scale: 1.0,
                    offset: -std::f64::consts::FRAC_PI_2,
                },
            ))
        }
        SolvedCurveGeometry::Parabola(parabola) => analytic(
            CurveKind::Parabola,
            frame(
                parabola.vertex().get(),
                parabola.frame().axis().as_raw(),
                parabola.frame().reference().as_raw(),
            ),
            vec![parabola.focal_distance().get()],
        ),
        SolvedCurveGeometry::Hyperbola(hyperbola) => analytic(
            CurveKind::Hyperbola,
            frame(
                hyperbola.center().get(),
                hyperbola.frame().axis().as_raw(),
                hyperbola.frame().reference().as_raw(),
            ),
            vec![
                hyperbola.major_radius().magnitude().get(),
                cadmpeg_ir::scalar::Length::from(hyperbola.minor_radius())
                    .magnitude()
                    .get(),
            ],
        ),
        SolvedCurveGeometry::Nurbs(nurbs) => Some((
            CurveRecord::Nurbs(nurbs_curve_record(nurbs)?),
            Affine1::IDENTITY,
        )),
        SolvedCurveGeometry::Transformed(placed) => {
            let (record, map) = solved_curve(placed.basis())?;
            let transform = placed.transform();
            match record {
                CurveRecord::Nurbs(mut nurbs) => {
                    for control in &mut nurbs.points {
                        *control = transform_point(transform, *control)?;
                    }
                    Some((CurveRecord::Nurbs(nurbs), map))
                }
                CurveRecord::Analytic(mut analytic) => {
                    let scale = similarity(transform)?;
                    let line = analytic.kind == CurveKind::Line;
                    analytic.frame = Frame {
                        position: transform_point(transform, analytic.frame.position)?,
                        axis: transform_direction(transform, analytic.frame.axis)?,
                        ref_direction: if line {
                            [0.0; 3]
                        } else {
                            transform_direction(transform, analytic.frame.ref_direction)?
                        },
                    };
                    for param in &mut analytic.params {
                        *param *= scale;
                    }
                    // A line parameter is arc length, so it scales with the
                    // transform.
                    let map = if line {
                        map.then(Affine1 { scale, offset: 0.0 })
                    } else {
                        map
                    };
                    Some((CurveRecord::Analytic(analytic), map))
                }
            }
        }
        _ => None,
    }
}

fn curve_record(geometry: &CurveGeometry) -> Option<(CurveRecord, Affine1)> {
    match geometry {
        CurveGeometry::Solved(solved) => solved_curve(solved),
        CurveGeometry::Procedural { cache, .. } => cache.as_ref().and_then(solved_curve),
    }
}

/// The record of a solved surface and its chart.
fn solved_surface(geometry: &SolvedSurfaceGeometry) -> Option<(SurfaceRecord, Chart)> {
    let analytic = |kind, position: Point3, axis: &Vector3, reference: &Vector3, params| {
        SurfaceRecord::Analytic(AnalyticSurface {
            kind,
            frame: Frame {
                position: point(position),
                axis: raw(axis),
                ref_direction: raw(reference),
            },
            params,
            profile: None,
        })
    };
    let pi = std::f64::consts::PI;
    match geometry {
        SolvedSurfaceGeometry::Plane(plane) => Some((
            analytic(
                SurfaceKind::Plane,
                plane.origin().get(),
                plane.frame().axis().as_raw(),
                plane.frame().reference().as_raw(),
                Vec::new(),
            ),
            Chart::IDENTITY,
        )),
        SolvedSurfaceGeometry::Cylinder(cylinder) => Some((
            analytic(
                SurfaceKind::Cylinder,
                cylinder.origin().get(),
                cylinder.frame().axis().as_raw(),
                cylinder.frame().reference().as_raw(),
                vec![cylinder.radius().get()],
            ),
            Chart::IDENTITY,
        )),
        SolvedSurfaceGeometry::Cone(cone) => {
            if cone.ratio().get() != 1.0 {
                return None;
            }
            let mut axis = raw(cone.frame().axis().as_raw());
            let reference = raw(cone.frame().reference().as_raw());
            let mut angle = cone.half_angle().get();
            let mut chart = Chart::IDENTITY;
            if angle < 0.0 {
                // (half_angle, axis) and (−half_angle, −axis) carry one locus
                // under (u, v) → (−u, −v), which keeps the normal.
                angle = -angle;
                axis = negated(axis);
                chart.u.scale = -1.0;
                chart.v.scale = -1.0;
            }
            let mut origin = point(cone.origin().get());
            let mut radius = cone.radius().get();
            if angle == 0.0 {
                return Some((
                    SurfaceRecord::Analytic(AnalyticSurface {
                        kind: SurfaceKind::Cylinder,
                        frame: Frame {
                            position: origin,
                            axis,
                            ref_direction: reference,
                        },
                        params: vec![radius],
                        profile: None,
                    }),
                    chart,
                ));
            }
            if angle >= std::f64::consts::FRAC_PI_2 {
                return None;
            }
            if radius <= 0.0 {
                origin = [0, 1, 2].map(|k| origin[k] + APEX_OFFSET * axis[k]);
                radius = APEX_OFFSET * angle.tan();
                chart.v.offset -= APEX_OFFSET;
            }
            Some((
                SurfaceRecord::Analytic(AnalyticSurface {
                    kind: SurfaceKind::Cone,
                    frame: Frame {
                        position: origin,
                        axis,
                        ref_direction: reference,
                    },
                    params: vec![radius, angle],
                    profile: None,
                }),
                chart,
            ))
        }
        SolvedSurfaceGeometry::Sphere(sphere) => {
            let radius = sphere.radius().get();
            let record = analytic(
                SurfaceKind::Sphere,
                sphere.center().get(),
                sphere.frame().axis().as_raw(),
                sphere.frame().reference().as_raw(),
                vec![radius.abs()],
            );
            let chart = if radius < 0.0 {
                // S(u, v) at −R is S(u + π, −v) at R, with the normal reversed.
                Chart {
                    u: Affine1 {
                        scale: 1.0,
                        offset: pi,
                    },
                    v: Affine1 {
                        scale: -1.0,
                        offset: 0.0,
                    },
                    normal_flipped: true,
                }
            } else {
                Chart::IDENTITY
            };
            Some((record, chart))
        }
        SolvedSurfaceGeometry::Torus(torus) => {
            let (major, minor) = (torus.major_radius().get(), torus.minor_radius().get());
            if major <= minor.abs() {
                return None;
            }
            let record = analytic(
                SurfaceKind::Torus,
                torus.center().get(),
                torus.frame().axis().as_raw(),
                torus.frame().reference().as_raw(),
                vec![major, minor.abs()],
            );
            let chart = if minor < 0.0 {
                // S(u, v) at −r is S(u, v + π) at r.
                Chart {
                    u: Affine1::IDENTITY,
                    v: Affine1 {
                        scale: 1.0,
                        offset: pi,
                    },
                    normal_flipped: false,
                }
            } else {
                Chart::IDENTITY
            };
            Some((record, chart))
        }
        SolvedSurfaceGeometry::Nurbs(nurbs) => Some((
            SurfaceRecord::Nurbs(nurbs_surface_record(nurbs)?),
            Chart {
                normal_flipped: nurbs.normal_reversed(),
                ..Chart::IDENTITY
            },
        )),
        SolvedSurfaceGeometry::Transformed(placed) => {
            let (record, chart) = solved_surface(placed.basis())?;
            let transform = placed.transform();
            match record {
                SurfaceRecord::Nurbs(mut nurbs) => {
                    for control in &mut nurbs.points {
                        *control = transform_point(transform, *control)?;
                    }
                    let flipped = transform.orientation()? < 0.0;
                    Some((
                        SurfaceRecord::Nurbs(nurbs),
                        Chart {
                            normal_flipped: chart.normal_flipped != flipped,
                            ..chart
                        },
                    ))
                }
                SurfaceRecord::Analytic(mut analytic) => {
                    let scale = similarity(transform)?;
                    analytic.frame = place_frame(transform, &analytic.frame)?;
                    let mut chart = chart;
                    match analytic.kind {
                        SurfaceKind::Plane => {
                            chart.u = chart.u.then(Affine1 { scale, offset: 0.0 });
                            chart.v = chart.v.then(Affine1 { scale, offset: 0.0 });
                        }
                        SurfaceKind::Cylinder => {
                            analytic.params[0] *= scale;
                            chart.v = chart.v.then(Affine1 { scale, offset: 0.0 });
                        }
                        SurfaceKind::Cone => {
                            analytic.params[0] *= scale;
                            chart.v = chart.v.then(Affine1 { scale, offset: 0.0 });
                        }
                        SurfaceKind::Sphere => analytic.params[0] *= scale,
                        SurfaceKind::Torus => {
                            analytic.params[0] *= scale;
                            analytic.params[1] *= scale;
                        }
                        SurfaceKind::Extrusion | SurfaceKind::Revolution => return None,
                    }
                    Some((SurfaceRecord::Analytic(analytic), chart))
                }
            }
        }
        _ => None,
    }
}

/// Writer state for one body.
struct Exporter<'t, 'ir> {
    ir: &'ir CadIr,
    tessellator: &'t mut Tessellator<'ir>,
    /// Every face needs a mesh: the body has no tessellation of its own.
    meshes_wanted: bool,
    /// The mesh of each face record, when it has one.
    meshes: Vec<Option<FaceMesh>>,
    brep: Brep,
    vertices: BTreeMap<VertexId, usize>,
    edges: BTreeMap<EdgeId, usize>,
    /// The IR parameter range of each written edge with a curve record,
    /// from the IR edge or derived from its vertices.
    edge_ranges: BTreeMap<EdgeId, [f64; 2]>,
    curves: BTreeMap<CurveId, Option<(Reference, Affine1)>>,
    surfaces: BTreeMap<SurfaceId, Option<(Reference, Chart)>>,
    /// The face ids in record order.
    faces: Vec<FaceId>,
    unwritten: Unwritten,
}

/// What a body's B-rep does not carry.
#[derive(Debug, Default, Clone, PartialEq)]
pub(super) struct Unwritten {
    /// Faces whose surface has no record.
    pub(super) faces: usize,
    /// Edges whose curve has no record.
    pub(super) edges: usize,
    /// Oriented edges on non-planar faces with no pcurve.
    pub(super) pcurves: usize,
    /// Loops whose pole-vertex uses were dropped.
    pub(super) pole_uses: usize,
    /// Faces with no mesh, with the reason.
    pub(super) unmeshed: Vec<String>,
}

/// The B-rep of one body.
pub(super) struct Exported {
    pub(super) brep: Brep,
    pub(super) unwritten: Unwritten,
}

impl<'t, 'ir> Exporter<'t, 'ir> {
    fn new(ir: &'ir CadIr, tessellator: &'t mut Tessellator<'ir>, meshes_wanted: bool) -> Self {
        Self {
            ir,
            tessellator,
            meshes_wanted,
            meshes: Vec::new(),
            brep: Brep {
                tolerance: ir.tolerances.linear.get(),
                angle_scale: 1.0,
                valid: true,
                ..Brep::default()
            },
            vertices: BTreeMap::new(),
            edges: BTreeMap::new(),
            edge_ranges: BTreeMap::new(),
            curves: BTreeMap::new(),
            surfaces: BTreeMap::new(),
            faces: Vec::new(),
            unwritten: Unwritten::default(),
        }
    }

    fn add_curve(&mut self, record: CurveRecord) -> Reference {
        match record {
            CurveRecord::Analytic(curve) => {
                self.brep.analytic_curves.push(curve);
                (Catalog::Analytic, self.brep.analytic_curves.len() - 1)
            }
            CurveRecord::Nurbs(curve) => {
                self.brep.nurbs_curves.push(curve);
                (Catalog::Nurbs, self.brep.nurbs_curves.len() - 1)
            }
        }
    }

    fn curve(&mut self, id: &CurveId) -> Option<(Reference, Affine1)> {
        if let Some(found) = self.curves.get(id) {
            return *found;
        }
        let record = self
            .ir
            .model
            .curves
            .iter()
            .find(|curve| &curve.id == id)
            .and_then(|curve| curve_record(&curve.geometry));
        let out = record.map(|(record, map)| (self.add_curve(record), map));
        self.curves.insert(id.clone(), out);
        out
    }

    fn surface(&mut self, id: &SurfaceId) -> Option<(Reference, Chart)> {
        if let Some(found) = self.surfaces.get(id) {
            return *found;
        }
        let surface = self
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| &surface.id == id);
        let record = match surface.map(|surface| &surface.geometry) {
            Some(SurfaceGeometry::Solved(solved)) => solved_surface(solved),
            Some(SurfaceGeometry::Procedural {
                construction,
                cache,
            }) => self
                .procedural_surface(construction.as_str())
                .or_else(|| cache.as_ref().and_then(solved_surface)),
            None => None,
        };
        let out = record.map(|(record, chart)| {
            let reference = match record {
                SurfaceRecord::Analytic(surface) => {
                    self.brep.analytic_surfaces.push(surface);
                    (Catalog::Analytic, self.brep.analytic_surfaces.len() - 1)
                }
                SurfaceRecord::Nurbs(surface) => {
                    self.brep.nurbs_surfaces.push(surface);
                    (Catalog::Nurbs, self.brep.nurbs_surfaces.len() - 1)
                }
            };
            (reference, chart)
        });
        self.surfaces.insert(id.clone(), out);
        out
    }

    /// The extrusion or revolution record of a linear sweep or an axis
    /// revolution.
    fn procedural_surface(&mut self, construction: &str) -> Option<(SurfaceRecord, Chart)> {
        let procedural = self
            .ir
            .model
            .procedural_surfaces
            .iter()
            .find(|procedural| procedural.id.as_str() == construction)?;
        match procedural.definition() {
            ProceduralSurfaceDefinition::LinearSweep(sweep) => {
                let (profile, map) = self.curve(sweep.directrix())?;
                let direction = raw(&sweep.direction().get());
                let length = dot(direction, direction).sqrt();
                let axis = unit(direction)?;
                Some((
                    SurfaceRecord::Analytic(AnalyticSurface {
                        kind: SurfaceKind::Extrusion,
                        frame: Frame {
                            position: [0.0; 3],
                            axis,
                            ref_direction: perpendicular(axis),
                        },
                        params: Vec::new(),
                        profile: Some(profile),
                    }),
                    Chart {
                        u: map,
                        v: Affine1 {
                            scale: length,
                            offset: 0.0,
                        },
                        normal_flipped: map.scale < 0.0,
                    },
                ))
            }
            ProceduralSurfaceDefinition::AxisRevolution(revolution) => {
                let (profile, map) = self.curve(revolution.directrix())?;
                let axis = raw(revolution.axis_direction().as_raw());
                Some((
                    SurfaceRecord::Analytic(AnalyticSurface {
                        kind: SurfaceKind::Revolution,
                        frame: Frame {
                            position: point(revolution.axis_origin().get()),
                            axis,
                            ref_direction: perpendicular(axis),
                        },
                        params: Vec::new(),
                        profile: Some(profile),
                    }),
                    Chart {
                        u: Affine1::IDENTITY,
                        v: map,
                        normal_flipped: map.scale < 0.0,
                    },
                ))
            }
            _ => None,
        }
    }

    fn vertex(&mut self, id: &VertexId) -> Option<usize> {
        if let Some(index) = self.vertices.get(id) {
            return Some(*index);
        }
        let model = &self.ir.model;
        let vertex = model.vertices.iter().find(|vertex| &vertex.id == id)?;
        let position = model
            .points
            .iter()
            .find(|candidate| candidate.id == vertex.point)?
            .position()
            .get();
        self.brep.vertices.push(point(position));
        let index = self.brep.vertices.len() - 1;
        self.vertices.insert(id.clone(), index);
        Some(index)
    }

    /// The edge record of an IR edge. The record runs from the IR start
    /// vertex to the IR end vertex.
    fn edge(&mut self, id: &EdgeId) -> Option<usize> {
        if let Some(index) = self.edges.get(id) {
            return Some(*index);
        }
        let edge = self.ir.model.edges.iter().find(|edge| &edge.id == id)?;
        let vertices = [self.vertex(&edge.start)?, self.vertex(&edge.end)?];
        let curve = edge.curve().and_then(|curve| self.curve(curve));
        let range = edge
            .param_range()
            .map(cadmpeg_ir::units::FiniteVector::finite_components)
            .map(|range| [range[0].get(), range[1].get()]);
        let closed = edge.start == edge.end;
        let ends = curve.and_then(|(reference, map)| {
            let ends = match range {
                Some([start, end]) => [map.apply(start), map.apply(end)],
                None => self.derived_range(reference, vertices, closed, map.scale > 0.0)?,
            };
            (ends[0] != ends[1]).then_some((reference, map, ends))
        });
        if let Some((_, map, ends)) = ends {
            let to_ir = |value: f64| (value - map.offset) / map.scale;
            self.edge_ranges
                .insert(id.clone(), [to_ir(ends[0]), to_ir(ends[1])]);
        }
        let record = match ends {
            Some((reference, map, ends)) => EdgeRecord {
                vertices,
                curve: Some(reference),
                range: [ends[0].min(ends[1]), ends[0].max(ends[1])],
                same_sense: map.scale > 0.0,
                tolerance: None,
            },
            None => {
                if edge.curve().is_some() {
                    self.unwritten.edges += 1;
                    self.brep.valid = false;
                }
                EdgeRecord {
                    vertices,
                    curve: None,
                    range: [0.0, 0.0],
                    same_sense: true,
                    tolerance: None,
                }
            }
        };
        let tolerance = edge
            .tolerance
            .map(cadmpeg_ir::scalar::PositiveReal::get)
            .filter(|tolerance| *tolerance > self.brep.tolerance);
        self.brep.edges.push(EdgeRecord {
            tolerance,
            ..record
        });
        let index = self.brep.edges.len() - 1;
        self.edges.insert(id.clone(), index);
        Some(index)
    }

    /// The record parameters at the start and end vertex of an edge with no
    /// IR range, ordered so that the edge runs toward increasing record
    /// parameter when `increasing`.
    fn derived_range(
        &self,
        reference: Reference,
        vertices: [usize; 2],
        closed: bool,
        increasing: bool,
    ) -> Option<[f64; 2]> {
        let positions = vertices.map(|vertex| self.brep.vertices[vertex]);
        let (start, period) = match reference {
            (Catalog::Analytic, index) => {
                let curve = &self.brep.analytic_curves[index];
                let frame = &curve.frame;
                let y = cross(frame.axis, frame.ref_direction);
                let local = |position: [f64; 3]| {
                    let offset = [0, 1, 2].map(|k| position[k] - frame.position[k]);
                    (
                        dot(offset, frame.ref_direction),
                        dot(offset, y),
                        dot(offset, frame.axis),
                    )
                };
                let parameter = |position: [f64; 3]| {
                    let (x, y, z) = local(position);
                    let param = |at: usize| curve.params.get(at).copied().unwrap_or(1.0);
                    match curve.kind {
                        CurveKind::Line => z,
                        CurveKind::Circle => y.atan2(x),
                        CurveKind::Ellipse => (y / param(1)).atan2(x / param(0)),
                        CurveKind::Parabola => y / (2.0 * param(0)),
                        CurveKind::Hyperbola => (y / param(1)).asinh(),
                    }
                };
                let period = matches!(curve.kind, CurveKind::Circle | CurveKind::Ellipse)
                    .then_some(std::f64::consts::TAU);
                let ends = positions.map(parameter);
                (ends, period)
            }
            (Catalog::Nurbs, index) => {
                let record = &self.brep.nurbs_curves[index];
                let curve = Nurbs::<3> {
                    degree: record.order - 1,
                    knots: record.knots.clone(),
                    points: record.points.clone(),
                    weights: record.weights.clone(),
                };
                let [low, high] = curve.domain()?;
                if closed {
                    return Some(if increasing { [low, high] } else { [high, low] });
                }
                let period = (record.form != Form::Open).then_some(high - low);
                (
                    [curve.nearest(positions[0])?, curve.nearest(positions[1])?],
                    period,
                )
            }
        };
        let [first, mut last] = start;
        match period {
            Some(period) => {
                if closed {
                    last = first;
                }
                while increasing && last <= first {
                    last += period;
                }
                while !increasing && last >= first {
                    last -= period;
                }
            }
            None if closed => return None,
            None => {}
        }
        Some([first, last])
    }

    /// The pcurve record of a coedge on a face with chart `chart`, traversed
    /// in the coedge direction.
    fn pcurve(&mut self, coedge: &Coedge, chart: Chart, planar: bool) -> Option<Reference> {
        let [use_] = coedge.pcurves.as_slice() else {
            return None;
        };
        let pcurve = self
            .ir
            .model
            .pcurves
            .iter()
            .find(|pcurve| pcurve.id == use_.pcurve)?;
        // A use with no range runs on the edge's parameter range, in the
        // coedge direction.
        let range = use_
            .parameter_range
            .map(cadmpeg_ir::geometry::DirectedParameterRange::endpoints)
            .or_else(|| {
                let [start, end] = *self.edge_ranges.get(&coedge.edge)?;
                Some(if coedge.sense == Sense::Forward {
                    [start, end]
                } else {
                    [end, start]
                })
            });
        let curve = match &pcurve.geometry {
            PcurveGeometry::Nurbs { nurbs } => {
                let curve = Nurbs::<2> {
                    degree: usize::try_from(nurbs.degree()).ok()?,
                    knots: nurbs.knots().as_slice().to_vec(),
                    points: nurbs
                        .control_points()
                        .into_iter()
                        .map(|point| [point.get().u, point.get().v])
                        .collect(),
                    weights: nurbs.weights().map_or_else(
                        || std::iter::repeat_n(1.0, nurbs.control_points().len()).collect(),
                        |weights| weights.iter().map(|weight| weight.get()).collect(),
                    ),
                };
                let range = range.filter(|[start, end]| {
                    curve.domain().is_some_and(|[low, high]| {
                        start.min(*end) >= low && start.max(*end) <= high
                    })
                });
                match range {
                    None => curve,
                    Some([start, end]) if start < end => {
                        if Some([start, end]) == curve.domain() {
                            curve
                        } else {
                            curve.segment(start, end)?
                        }
                    }
                    Some([start, end]) => curve.segment(end, start)?.reversed()?,
                }
            }
            PcurveGeometry::Line(line) => {
                let [start, end] = range?;
                let origin = line.origin().get();
                let direction = *line.direction().as_raw();
                let at = |t: f64| [origin.u + t * direction.u, origin.v + t * direction.v];
                Nurbs::<2> {
                    degree: 1,
                    knots: vec![0.0, 0.0, 1.0, 1.0],
                    points: vec![at(start), at(end)],
                    weights: vec![1.0, 1.0],
                }
            }
            geometry @ (PcurveGeometry::Circle(_)
            | PcurveGeometry::Ellipse(_)
            | PcurveGeometry::Harmonic(_)) => {
                let (center, a, b) = harmonic(geometry)?;
                let [start, end] = range.unwrap_or([0.0, std::f64::consts::TAU]);
                if planar {
                    if let Some(analytic) = analytic_pcurve(center, a, b, [start, end]) {
                        self.brep.analytic_pcurves.push(analytic);
                        return Some((Catalog::Analytic, self.brep.analytic_pcurves.len() - 1));
                    }
                }
                harmonic_arc(center, a, b, start, end)?
            }
            _ => return None,
        };
        let mapped = curve.map(|point| chart.apply(point));
        self.brep.nurbs_pcurves.push(NurbsCurveRecord {
            order: mapped.degree + 1,
            points: mapped.points,
            weights: mapped.weights,
            knots: mapped.knots,
            form: Form::Open,
        });
        Some((Catalog::Nurbs, self.brep.nurbs_pcurves.len() - 1))
    }

    fn face(&mut self, face_id: &FaceId) {
        let model = &self.ir.model;
        let Some(face) = model.faces.iter().find(|face| &face.id == face_id) else {
            return;
        };
        let Some((surface, chart)) = self.surface(&face.surface) else {
            self.unwritten.faces += 1;
            self.brep.valid = false;
            return;
        };
        let planar = matches!(surface, (Catalog::Analytic, index)
            if self.brep.analytic_surfaces[index].kind == SurfaceKind::Plane);
        let mesh = if self.meshes_wanted || !planar {
            match self.tessellator.face(face_id) {
                Ok(mesh) => Some(mesh),
                Err(refusal) => {
                    self.unwritten
                        .unmeshed
                        .push(format!("{face_id}: {refusal}"));
                    None
                }
            }
        } else {
            None
        };
        let mut loops = Vec::new();
        for loop_id in &face.loops {
            let Some(loop_) = model.loops.iter().find(|loop_| &loop_.id == loop_id) else {
                continue;
            };
            match &loop_.boundary {
                LoopBoundary::Vertex { vertex, .. } => {
                    let vertex = self.vertex(vertex);
                    self.brep.loops.push(LoopRecord {
                        oriented_edges: Vec::new(),
                        vertex,
                    });
                }
                LoopBoundary::Ring(ring) => {
                    if !ring.vertex_uses().is_empty() {
                        self.unwritten.pole_uses += 1;
                    }
                    let mut oriented = Vec::new();
                    for coedge_id in ring.coedges() {
                        let Some(coedge) =
                            model.coedges.iter().find(|coedge| &coedge.id == coedge_id)
                        else {
                            continue;
                        };
                        let Some(edge) = self.edge(&coedge.edge) else {
                            continue;
                        };
                        let trace = mesh.as_ref().and_then(|mesh| {
                            mesh.coedges.iter().find(|trace| &trace.coedge == coedge_id)
                        });
                        // A pcurve that the tessellator found off its edge is
                        // replaced by the traced one.
                        let trusted = trace.is_none_or(|trace| trace.from_pcurve);
                        let pcurve = trusted
                            .then(|| self.pcurve(coedge, chart, planar))
                            .flatten()
                            .or_else(|| {
                                if planar {
                                    return None;
                                }
                                self.derived_pcurve(trace?, chart)
                            });
                        if pcurve.is_none() && !planar {
                            self.unwritten.pcurves += 1;
                        }
                        self.brep.oriented_edges.push(OrientedEdgeRecord {
                            edge,
                            orientation: coedge.sense == Sense::Forward,
                            pcurve,
                        });
                        oriented.push(self.brep.oriented_edges.len() - 1);
                    }
                    self.join_pcurves(&mut oriented, surface);
                    self.brep.loops.push(LoopRecord {
                        oriented_edges: oriented,
                        vertex: None,
                    });
                }
            }
            loops.push(self.brep.loops.len() - 1);
        }
        let Ok(id) = i64::try_from(self.brep.faces.len()) else {
            self.unwritten.faces += 1;
            self.brep.valid = false;
            return;
        };
        self.brep.faces.push(FaceRecord {
            loops,
            surface,
            same_sense: (face.sense == Sense::Forward) != chart.normal_flipped,
            id,
        });
        self.faces.push(face_id.clone());
        self.meshes.push(mesh);
    }

    /// The period of each record parameter of a surface record.
    fn record_periods(&self, surface: Reference) -> [Option<f64>; 2] {
        let turn = Some(std::f64::consts::TAU);
        match surface {
            (Catalog::Analytic, index) => match self.brep.analytic_surfaces[index].kind {
                SurfaceKind::Cylinder
                | SurfaceKind::Cone
                | SurfaceKind::Sphere
                | SurfaceKind::Revolution => [turn, None],
                SurfaceKind::Torus => [turn, turn],
                SurfaceKind::Plane | SurfaceKind::Extrusion => [None, None],
            },
            (Catalog::Nurbs, index) => {
                let record = &self.brep.nurbs_surfaces[index];
                let period = |form: Form, knots: &[f64], order: usize, count: usize| {
                    (form != Form::Open)
                        .then(|| Some(knots.get(count)? - knots.get(order - 1)?))
                        .flatten()
                };
                [
                    period(
                        record.u_form,
                        &record.u_knots,
                        record.u_order,
                        record.u_count,
                    ),
                    period(
                        record.v_form,
                        &record.v_knots,
                        record.v_order,
                        record.v_count,
                    ),
                ]
            }
        }
    }

    /// Whether record parameters `uv` lie on a pole of a sphere or the apex
    /// of a cone, where the `u` parameter has no value.
    fn at_pole(&self, surface: Reference, uv: [f64; 2]) -> bool {
        let (Catalog::Analytic, index) = surface else {
            return false;
        };
        let record = &self.brep.analytic_surfaces[index];
        match record.kind {
            SurfaceKind::Sphere => {
                (uv[1].abs() - std::f64::consts::FRAC_PI_2).abs() <= POLE_TOLERANCE
            }
            SurfaceKind::Cone => {
                let [radius, angle] = [record.params[0], record.params[1]];
                (radius + uv[1] * angle.tan()).abs() <= POLE_TOLERANCE * radius.max(1.0)
            }
            _ => false,
        }
    }

    /// Moves the NURBS pcurves of a loop by whole periods so that each one
    /// starts where the one before it ends.
    fn join_pcurves(&mut self, oriented: &mut [usize], surface: Reference) {
        let periods = self.record_periods(surface);
        if periods.iter().all(Option::is_none) {
            return;
        }
        // A loop that jumps at a pole starts after the pole, so that the jump
        // is its closing pair.
        if let Some(start) = (1..oriented.len()).find(|at| {
            matches!(self.brep.oriented_edges[oriented[at - 1]].pcurve, Some((Catalog::Nurbs, index))
                if self.brep.nurbs_pcurves[index].points.last().is_some_and(|end| self.at_pole(surface, *end)))
        }) {
            oriented.rotate_left(start);
        }
        let mut end: Option<[f64; 2]> = None;
        for at in oriented.iter() {
            let Some((Catalog::Nurbs, index)) = self.brep.oriented_edges[*at].pcurve else {
                end = None;
                continue;
            };
            let pole = end.is_some_and(|previous| self.at_pole(surface, previous));
            let record = &mut self.brep.nurbs_pcurves[index];
            // At a pole the parameter jumps between branches; joining there
            // would fold a seam onto one branch.
            if let (Some(previous), Some(first), false) =
                (end, record.points.first().copied(), pole)
            {
                for axis in 0..2 {
                    if let Some(period) = periods[axis] {
                        let shift = ((previous[axis] - first[axis]) / period).round() * period;
                        if shift != 0.0 {
                            for point in &mut record.points {
                                point[axis] += shift;
                            }
                        }
                    }
                }
            }
            end = record.points.last().copied();
        }
    }

    /// A degree-one NURBS pcurve through a derived trace, parameterized by
    /// its chord length in the record chart.
    fn derived_pcurve(&mut self, trace: &CoedgeTrace, chart: Chart) -> Option<Reference> {
        let mut points: Vec<[f64; 2]> = Vec::new();
        for uv in &trace.uv {
            let mapped = chart.apply(*uv);
            if points.last() != Some(&mapped) {
                points.push(mapped);
            }
        }
        if points.len() < 2 {
            return None;
        }
        let mut lengths = vec![0.0];
        for pair in points.windows(2) {
            let step = (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]);
            lengths.push(lengths.last().copied().unwrap_or_default() + step);
        }
        let total = lengths.last().copied().unwrap_or_default();
        if !(total.is_finite() && total > 0.0) {
            return None;
        }
        let mut knots = vec![0.0];
        knots.extend(lengths.iter().map(|length| length / total));
        knots.push(1.0);
        let weights = std::iter::repeat_n(1.0, points.len()).collect();
        self.brep.nurbs_pcurves.push(NurbsCurveRecord {
            order: 2,
            points,
            weights,
            knots,
            form: Form::Open,
        });
        Some((Catalog::Nurbs, self.brep.nurbs_pcurves.len() - 1))
    }
}

/// The parameter-space center and axes `a`, `b` of a harmonic pcurve
/// `center + cos t · a + sin t · b`.
fn harmonic(geometry: &PcurveGeometry) -> Option<([f64; 2], [f64; 2], [f64; 2])> {
    let pair = |point: cadmpeg_ir::math::Point2| [point.u, point.v];
    match geometry {
        PcurveGeometry::Circle(circle) => {
            let radius = circle.radius().get();
            Some((
                pair(circle.center().get()),
                pair(circle.x_axis().get()).map(|value| value * radius),
                pair(circle.y_axis().get()).map(|value| value * radius),
            ))
        }
        PcurveGeometry::Ellipse(ellipse) => Some((
            pair(ellipse.center().get()),
            pair(ellipse.x_axis().get()).map(|value| value * ellipse.major_radius().get()),
            pair(ellipse.y_axis().get()).map(|value| value * ellipse.minor_radius().get()),
        )),
        PcurveGeometry::Harmonic(harmonic) => Some((
            pair(harmonic.center().get()),
            pair(harmonic.cosine().get()),
            pair(harmonic.sine().get()),
        )),
        _ => None,
    }
}

/// The analytic record of a harmonic pcurve whose axes are orthogonal, the
/// first at least as long as the second.
fn analytic_pcurve(
    center: [f64; 2],
    a: [f64; 2],
    b: [f64; 2],
    range: [f64; 2],
) -> Option<AnalyticPcurve> {
    let (length_a, length_b) = (a[0].hypot(a[1]), b[0].hypot(b[1]));
    if !(length_a > 0.0 && length_b > 0.0) {
        return None;
    }
    let x = [a[0] / length_a, a[1] / length_a];
    let y = [b[0] / length_b, b[1] / length_b];
    if (x[0] * y[0] + x[1] * y[1]).abs() > SIMILARITY_TOLERANCE || length_a < length_b {
        return None;
    }
    // A second axis at −90° is the reflection: c(−t) on the +90° axis.
    let reflected = x[0] * y[1] - x[1] * y[0] < 0.0;
    let range = if reflected {
        range.map(|value| -value)
    } else {
        range
    };
    let circle = (length_a - length_b).abs() <= SIMILARITY_TOLERANCE * length_a;
    Some(AnalyticPcurve {
        kind: if circle {
            PcurveKind::Circle
        } else {
            PcurveKind::Ellipse
        },
        position: center,
        ref_direction: x,
        params: if circle {
            vec![length_a]
        } else {
            vec![length_a, length_b]
        },
        range,
    })
}

/// The B-rep of `body`, or `None` when it has no faces. Each face gets a
/// mesh when `meshes_wanted`, and each non-planar face gets one for its
/// derived pcurves.
pub(super) fn export<'ir>(
    ir: &'ir CadIr,
    body: &Body,
    tessellator: &mut Tessellator<'ir>,
    meshes_wanted: bool,
) -> Option<Exported> {
    let model = &ir.model;
    let mut exporter = Exporter::new(ir, tessellator, meshes_wanted);
    for region_id in &body.regions {
        let Some(region) = model.regions.iter().find(|region| &region.id == region_id) else {
            continue;
        };
        let mut region_shells = Vec::new();
        for shell_id in &region.shells {
            let Some(shell) = model.shells.iter().find(|shell| &shell.id == shell_id) else {
                continue;
            };
            if shell.faces().is_empty() {
                continue;
            }
            let start = exporter.brep.faces.len();
            for face in shell.faces() {
                exporter.face(face);
            }
            let faces: Vec<usize> = (start..exporter.brep.faces.len()).collect();
            if faces.is_empty() {
                continue;
            }
            exporter.brep.shells.push(ShellRecord {
                faces,
                closed: body.kind == BodyKind::Solid,
            });
            region_shells.push(exporter.brep.shells.len() - 1);
        }
        if body.kind == BodyKind::Solid {
            if let Some((outer, voids)) = region_shells.split_first() {
                exporter.brep.solids.push(SolidRecord {
                    outer: *outer,
                    voids: voids.to_vec(),
                });
            }
        }
    }
    if exporter.brep.faces.is_empty() {
        return None;
    }
    // `brep:tolerance` bounds every vertex-on-face and edge-on-face
    // deviation, so it is the largest entity tolerance of the body.
    let model = &ir.model;
    let mut tolerance = exporter.brep.tolerance;
    for face in &exporter.faces {
        if let Some(value) = model
            .faces
            .iter()
            .find(|candidate| &candidate.id == face)
            .and_then(|face| face.tolerance)
        {
            tolerance = tolerance.max(value.get());
        }
    }
    for vertex in exporter.vertices.keys() {
        if let Some(value) = model
            .vertices
            .iter()
            .find(|candidate| &candidate.id == vertex)
            .and_then(|vertex| vertex.tolerance)
        {
            tolerance = tolerance.max(value.get());
        }
    }
    for edge in &exporter.brep.edges {
        if let Some(value) = edge.tolerance {
            tolerance = tolerance.max(value);
        }
    }
    // A source whose vertices and edges lie off their surfaces states that
    // distance as its tolerance.
    for mesh in exporter.meshes.iter().flatten() {
        tolerance = tolerance.max(mesh.deviation * DEVIATION_MARGIN);
    }
    exporter.brep.tolerance = tolerance;
    for edge in &mut exporter.brep.edges {
        edge.tolerance = edge.tolerance.filter(|value| *value > tolerance);
    }
    Some(Exported {
        brep: exporter.brep,
        unwritten: exporter.unwritten,
    })
}
