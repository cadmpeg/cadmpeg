use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::record_invalid;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsSurface},
    pcurve::Pcurve,
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::schema::EntityKind;
use cadmpeg_ir::topology::{Body, BodyKind, Coedge, Edge, Face, Loop, Sense, Vertex};

use super::{
    admit_pcurve, check_frame, check_nurbs_curve, check_nurbs_surface, close_point,
    generated_projected_brep_c2_curve, singular_trim_iso, stated_write_tolerance,
    validate_nurbs_trim, validate_singular_trim, EPS_WRITE_DEGENERATE,
};

pub(super) struct WritableModel<'a> {
    pub(super) body: &'a Body,
    pub(super) vertices: Vec<WritableVertex<'a>>,
    pub(super) edges: Vec<WritableEdge<'a>>,
    pub(super) coedges: Vec<WritableCoedge<'a>>,
    pub(super) loops: Vec<WritableLoop<'a>>,
    pub(super) faces: Vec<WritableFace<'a>>,
    pub(super) surfaces: Vec<WritableFaceSurface<'a>>,
    /// The losses of the Brep write: `export:record.invalid` for each record
    /// it writes although its geometry disagrees, and `export:value.unwritable`
    /// for the face values a Brep face does not hold.
    pub(super) losses: Vec<LossNote>,
}

pub(super) struct WritableVertex<'a> {
    pub(super) source: &'a Vertex,
    pub(super) point: Point3,
}

pub(super) struct WritableEdge<'a> {
    pub(super) source: &'a Edge,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) domain: [FiniteReal; 2],
    pub(super) curve_id: &'a str,
    pub(super) curve: WritableEdgeCurve<'a>,
    pub(super) uses: Vec<usize>,
}

#[derive(Clone, Copy)]
pub(super) enum WritableEdgeCurve<'a> {
    Line(cadmpeg_ir::geometry::analytic::LineCurve),
    Nurbs(&'a NurbsCurve),
}

impl WritableEdgeCurve<'_> {
    /// The edge curve's point at `parameter`, or why it has no finite point
    /// there. A NURBS parameter outside the evaluable domain has no value.
    pub(super) fn point(
        self,
        parameter: FiniteReal,
    ) -> Result<
        cadmpeg_ir::features::geometry::FinitePoint3,
        cadmpeg_ir::eval::EvaluationFailure<Point3>,
    > {
        match self {
            Self::Line(line) => cadmpeg_ir::eval::decode::curve_point(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &CurveGeometry::Solved(SolvedCurveGeometry::Line(line)),
                parameter.get(),
            ),
            Self::Nurbs(nurbs) => cadmpeg_ir::eval::decode::nurbs_curve_point_at(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                nurbs,
                cadmpeg_ir::eval::map_nurbs_curve_parameter(nurbs, parameter)
                    .ok_or(cadmpeg_ir::eval::EvaluationFailure::NoValue)?
                    .get(),
            ),
        }
    }
}

/// The edge a trim runs along.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TrimEdge {
    /// The edge at this position of [`WritableModel::edges`].
    Edge(usize),
    /// A singular edge, which a 3DM Brep holds as no edge: the trim's ends
    /// meet at the vertex at this position.
    Singular(usize),
}

pub(super) struct WritableCoedge<'a> {
    pub(super) source: &'a Coedge,
    pub(super) edge: TrimEdge,
    pub(super) owner_loop: usize,
    radial_next: usize,
    /// Whether the trim runs against its edge. A 3DM loop runs
    /// counterclockwise in its surface's parameters, so the trim runs the
    /// coedge's way on a forward face and the other way on a reversed face.
    pub(super) reversed: bool,
    pub(super) c2: ([u8; 16], Vec<u8>),
    /// The trim's parameter interval: its pcurve's, or its edge's where the
    /// trim is the edge's projection.
    pub(super) domain: [f64; 2],
    pub(super) fit_tolerance: f64,
    /// The 3DM iso value of the trim.
    pub(super) iso: i32,
}

pub(super) struct WritablePcurve<'a> {
    pub(super) source: &'a Pcurve,
    pub(super) payload: ([u8; 16], Vec<u8>),
    pub(super) domain_extent_points: Vec<cadmpeg_ir::math::Point2>,
    /// The parameter interval the trim runs over.
    pub(super) domain: [f64; 2],
    /// Whether `payload` runs the pcurve record backwards over `domain`.
    pub(super) reversed: bool,
}

pub(super) struct WritableLoop<'a> {
    pub(super) source: &'a Loop,
    pub(super) face: usize,
    pub(super) coedges: Vec<usize>,
}

pub(super) struct WritableFace<'a> {
    pub(super) source: &'a Face,
    pub(super) surface: usize,
    pub(super) loops: Vec<usize>,
}

/// A free curve admitted for native Rhino object writing.
pub(super) struct WritableObjectCurve<'a> {
    geometry: ObjectCurveGeometry<'a>,
}

enum ObjectCurveGeometry<'a> {
    /// A circle, or its arc over an angle range of at most one turn.
    Circle(&'a cadmpeg_ir::geometry::analytic::CircleCurve, [f64; 2]),
    /// A bounded line: its endpoints and its parameter range.
    Line {
        from: Point3,
        to: Point3,
        domain: [f64; 2],
    },
    Nurbs(std::borrow::Cow<'a, NurbsCurve>),
}

impl<'a> WritableObjectCurve<'a> {
    /// Admits a free curve with writable attributes and geometry.
    pub(super) fn try_new(curve: &'a cadmpeg_ir::geometry::Curve) -> Result<Self, CodecError> {
        let geometry = match &curve.geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) => {
                check_frame(curve.id.as_str(), *circle.frame(), "circle")?;
                let range = curve
                    .parameter_range
                    .map_or([0.0, std::f64::consts::TAU], |range| {
                        FiniteReal::raw_array(range.finite_endpoints())
                    });
                let span = range[1] - range[0];
                if !(span > 0.0 && span <= std::f64::consts::TAU) {
                    return Err(CodecError::NotImplemented(format!(
                        "circle {} has a parameter range that is not an arc of at most one turn",
                        curve.id.as_str()
                    )));
                }
                ObjectCurveGeometry::Circle(circle, range)
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Line(line)) => {
                let range = curve.parameter_range.ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "line {} states no parameter range, and 3DM holds no unbounded line",
                        curve.id.as_str()
                    ))
                })?;
                let domain = FiniteReal::raw_array(range.finite_endpoints());
                let origin = line.origin().get();
                let direction = *line.direction().as_raw();
                let at = |parameter: f64| {
                    Point3::new(
                        origin.x + direction.x * parameter,
                        origin.y + direction.y * parameter,
                        origin.z + direction.z * parameter,
                    )
                };
                ObjectCurveGeometry::Line {
                    from: at(domain[0]),
                    to: at(domain[1]),
                    domain,
                }
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Polyline(polyline)) => {
                let nurbs = polyline_nurbs(polyline).map_err(|error| {
                    CodecError::NotImplemented(format!(
                        "polyline {} has no degree-1 NURBS form: {error}",
                        curve.id.as_str()
                    ))
                })?;
                check_nurbs_curve(curve.id.as_str(), &nurbs)?;
                ObjectCurveGeometry::Nurbs(std::borrow::Cow::Owned(nurbs))
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
                check_nurbs_curve(curve.id.as_str(), nurbs)?;
                ObjectCurveGeometry::Nurbs(std::borrow::Cow::Borrowed(nurbs))
            }
            CurveGeometry::Solved(solved) => {
                let range = match (curve.parameter_range, solved) {
                    (Some(range), _) => FiniteReal::raw_array(range.finite_endpoints()),
                    (None, SolvedCurveGeometry::Ellipse(_)) => [0.0, std::f64::consts::TAU],
                    (None, _) => {
                        return Err(CodecError::NotImplemented(format!(
                            "curve {} states no parameter range, and 3DM holds no unbounded \
                             curve",
                            curve.id.as_str()
                        )))
                    }
                };
                let nurbs = super::convert::curve_nurbs(solved, range).map_err(|reason| {
                    CodecError::NotImplemented(format!(
                        "curve {} has no NURBS form the writer builds: {reason}",
                        curve.id.as_str()
                    ))
                })?;
                check_nurbs_curve(curve.id.as_str(), &nurbs)?;
                ObjectCurveGeometry::Nurbs(std::borrow::Cow::Owned(nurbs))
            }
            CurveGeometry::Procedural { .. } => {
                return Err(CodecError::NotImplemented(format!(
                    "Rhino writer cannot represent curve {} as a native object",
                    curve.id.as_str()
                )))
            }
        };
        Ok(Self { geometry })
    }

    /// Encodes the admitted curve as a native class payload.
    pub(super) fn payload(&self) -> Result<([u8; 16], Vec<u8>), CodecError> {
        Ok(match &self.geometry {
            ObjectCurveGeometry::Line { from, to, domain } => (
                super::LINE_CLASS,
                super::bounded_line_payload(
                    [from.x, from.y, from.z],
                    [to.x, to.y, to.z],
                    *domain,
                    3,
                )?,
            ),
            ObjectCurveGeometry::Circle(circle, range) => {
                let center = circle.center().get();
                let axis = circle.frame().axis().as_raw();
                let ref_direction = circle.frame().reference().as_raw();
                let radius = circle.radius().get();
                (
                    super::ARC_CLASS,
                    super::circle_payload(center, *axis, *ref_direction, radius, *range)?,
                )
            }
            ObjectCurveGeometry::Nurbs(nurbs) => {
                (super::NURBS_CURVE_CLASS, super::nurbs_curve_payload(nurbs)?)
            }
        })
    }
}

/// The degree-1 NURBS curve through the samples of `polyline`, at their
/// stated parameters or, where it states none, at their indexes.
fn polyline_nurbs(
    polyline: &cadmpeg_ir::geometry::sampled::PolylineCurve,
) -> Result<NurbsCurve, String> {
    let points = polyline
        .points()
        .map(cadmpeg_ir::features::geometry::FinitePoint3::get)
        .collect::<Vec<_>>();
    if points.len() < 2 {
        return Err("it holds one sample".into());
    }
    let parameters = match polyline.parameters() {
        Some(parameters) => parameters.map(FiniteReal::get).collect::<Vec<_>>(),
        None => (0..points.len())
            .map(|index| {
                cadmpeg_core::convert::f64_from_index(index)
                    .ok_or_else(|| "a sample index exceeds the f64 range".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?,
    };
    let (Some(first), Some(last)) = (parameters.first(), parameters.last()) else {
        return Err("it holds no sample".into());
    };
    let knots = std::iter::once(*first)
        .chain(parameters.iter().copied())
        .chain(std::iter::once(*last))
        .collect::<Vec<_>>();
    let poles = cadmpeg_ir::geometry::nurbs::NurbsPoles3::from_lanes(points, None)
        .map_err(|error| error.to_string())?;
    NurbsCurve::new(1, knots, poles, false).map_err(|error| error.to_string())
}

#[derive(Clone, Copy)]
pub(super) enum WritableFaceSurface<'a> {
    Plane {
        origin: Point3,
        normal: Vector3,
        u_axis: Vector3,
    },
    Nurbs {
        surface: &'a NurbsSurface,
        /// Pole count at the archive's own lane width, proven by
        /// `check_nurbs_surface` when the surface was admitted.
        pole_count: i32,
    },
}

impl<'a> WritableFaceSurface<'a> {
    pub(super) fn try_new(surface: &'a cadmpeg_ir::geometry::Surface) -> Result<Self, CodecError> {
        match &surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) => {
                let origin = plane.origin().get();
                let normal = plane.frame().axis().as_raw();
                let u_axis = plane.frame().reference().as_raw();
                check_frame(surface.id.as_str(), *plane.frame(), "plane")?;
                Ok(Self::Plane {
                    origin,
                    normal: *normal,
                    u_axis: *u_axis,
                })
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) => {
                let pole_count = check_nurbs_surface(surface.id.as_str(), nurbs)?;
                Ok(Self::Nurbs {
                    surface: nurbs,
                    pole_count,
                })
            }
            _ => Err(CodecError::NotImplemented(format!(
                "surface {} is not a plane or NURBS patch",
                surface.id.as_str(),
            ))),
        }
    }

    pub(super) fn payload(self) -> Result<([u8; 16], Vec<u8>), CodecError> {
        Ok(match self {
            Self::Plane {
                origin,
                normal,
                u_axis,
            } => (
                super::PLANE_SURFACE_CLASS,
                super::plane_surface_payload(origin, normal, u_axis)?,
            ),
            Self::Nurbs {
                surface,
                pole_count,
            } => (
                super::NURBS_SURFACE_CLASS,
                super::nurbs_surface_payload(surface, pole_count)?,
            ),
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
    SingleFace,
    MultiFace,
}

fn positions<'a>(
    ids: impl Iterator<Item = &'a str>,
) -> Result<BTreeMap<&'a str, usize>, CodecError> {
    let mut positions = BTreeMap::new();
    for (position, id) in ids.enumerate() {
        if positions.insert(id, position).is_some() {
            return Err(CodecError::malformed(format_args!(
                "duplicate topology identifier {id}"
            )));
        }
    }
    Ok(positions)
}

fn resolve(positions: &BTreeMap<&str, usize>, id: &str) -> Result<usize, CodecError> {
    positions
        .get(id)
        .copied()
        .ok_or_else(|| CodecError::malformed(format_args!("topology reference {id} is missing")))
}

impl<'a> WritableModel<'a> {
    pub(super) fn try_new(ir: &'a CadIr) -> Result<Self, CodecError> {
        let model = &ir.model;
        let geometry = &ir.geometry;
        let Some(body) = geometry
            .bodies
            .iter()
            .find(|body| matches!(body.kind, BodyKind::Sheet | BodyKind::Solid))
        else {
            return Err(CodecError::NotImplemented(
                "body topology is not a writable Brep".into(),
            ));
        };
        let edge_count = geometry.coedges.len();
        let singular_count = geometry
            .edges
            .iter()
            .filter(|edge| super::is_singular_edge(edge))
            .count();
        let layout = if body.kind != BodyKind::Solid
            && geometry.faces.len() == 1
            && !geometry.loops.is_empty()
            && geometry.edges.len() == edge_count
            && geometry.vertices.len() == edge_count
            && geometry.points.len() == edge_count
            && geometry.curves.len() == edge_count
            && geometry.surfaces.len() == 1
        {
            Layout::SingleFace
        } else {
            Layout::MultiFace
        };
        let invalid_counts = layout == Layout::MultiFace
            && (geometry.faces.is_empty()
                || geometry.loops.len() < geometry.faces.len()
                || geometry.edges.is_empty()
                || geometry.vertices.is_empty()
                || geometry.points.len() != geometry.vertices.len()
                || geometry.curves.len() != geometry.edges.len() - singular_count
                || geometry.surfaces.len() != geometry.faces.len());
        if geometry.bodies.len() != 1
            || geometry.regions.len() != 1
            || geometry.shells.len() != 1
            || !model.tessellations.is_empty()
            || invalid_counts
        {
            return Err(CodecError::NotImplemented(
                "Brep writing requires one shell with one carrier for each face and edge".into(),
            ));
        }
        if geometry
            .complete_body_regions(&body.id)
            .is_none_or(|regions| regions.count() != 1)
            || geometry.complete_shells(&geometry.regions[0]).is_none()
        {
            return Err(CodecError::NotImplemented(
                "Brep writing requires a body that states every region, shell, face, and loop"
                    .into(),
            ));
        }
        if body.transform.is_some() {
            return Err(CodecError::NotImplemented(
                "Brep body placement is not writable".into(),
            ));
        }
        let region = &geometry.regions[0];
        let shell = &geometry.shells[0];
        if region.body != body.id
            || !region.stated_shells().eq(std::iter::once(&shell.id))
            || shell.region != region.id
            || geometry.faces.iter().any(|face| face.shell != shell.id)
            || !shell.members.is_empty()
        {
            return Err(CodecError::Malformed(
                "Brep ownership graph is inconsistent".into(),
            ));
        }
        let original_coedges = positions(geometry.coedges.iter().map(|coedge| coedge.id.as_str()))?;
        let original_edges = positions(geometry.edges.iter().map(|edge| edge.id.as_str()))?;
        let original_vertices =
            positions(geometry.vertices.iter().map(|vertex| vertex.id.as_str()))?;
        let mut ordered_coedges = Vec::new();
        let mut ordered_edges = Vec::new();
        let mut ordered_vertices = Vec::new();
        match layout {
            Layout::SingleFace => {
                if !geometry.faces[0].loops.boundary().is_some_and(|loops| {
                    loops
                        .iter()
                        .eq(geometry.loops.iter().map(|loop_| &loop_.id))
                }) {
                    return Err(CodecError::Malformed(
                        "single-face loop order is inconsistent".into(),
                    ));
                }
                let mut used_edges = BTreeSet::new();
                let mut used_vertices = BTreeSet::new();
                for loop_ in &geometry.loops {
                    for id in loop_.coedges() {
                        let coedge = &geometry.coedges[resolve(&original_coedges, id.as_str())?];
                        let edge = &geometry.edges[resolve(&original_edges, coedge.edge.as_str())?];
                        if !used_edges.insert(edge.id.as_str()) {
                            return Err(CodecError::NotImplemented(
                                "planar sheet cannot reuse an edge in one loop".into(),
                            ));
                        }
                        let ends = super::edge_ends(edge)?;
                        let start = if coedge.sense == Sense::Forward {
                            &ends.start
                        } else {
                            &ends.end
                        };
                        let vertex =
                            &geometry.vertices[resolve(&original_vertices, start.as_str())?];
                        if !used_vertices.insert(vertex.id.as_str()) {
                            return Err(CodecError::Malformed(
                                "planar loop has repeated traversal vertices".into(),
                            ));
                        }
                        ordered_coedges.push(coedge);
                        ordered_edges.push(edge);
                        ordered_vertices.push(vertex);
                    }
                }
            }
            Layout::MultiFace => {
                ordered_coedges.extend(&geometry.coedges);
                ordered_edges.extend(&geometry.edges);
                ordered_vertices.extend(&geometry.vertices);
            }
        }
        let vertex_positions = positions(ordered_vertices.iter().map(|vertex| vertex.id.as_str()))?;
        let (singular_edges, ordered_edges): (Vec<&Edge>, Vec<&Edge>) = ordered_edges
            .into_iter()
            .partition(|edge| super::is_singular_edge(edge));
        let mut singular = BTreeMap::new();
        for edge in singular_edges {
            let vertex = resolve(&vertex_positions, super::edge_ends(edge)?.start.as_str())?;
            singular.insert(edge.id.as_str(), (vertex, edge));
        }
        let edge_positions = positions(ordered_edges.iter().map(|edge| edge.id.as_str()))?;
        let coedge_positions = positions(ordered_coedges.iter().map(|coedge| coedge.id.as_str()))?;
        let loop_positions = positions(geometry.loops.iter().map(|loop_| loop_.id.as_str()))?;
        let face_positions = positions(geometry.faces.iter().map(|face| face.id.as_str()))?;
        let surface_positions =
            positions(geometry.surfaces.iter().map(|surface| surface.id.as_str()))?;
        let vertices = ordered_vertices
            .into_iter()
            .map(|vertex| {
                let point = geometry
                    .points
                    .iter()
                    .find(|point| point.id == vertex.point)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "point {} is missing",
                            vertex.point.as_str()
                        ))
                    })?;
                Ok(WritableVertex {
                    source: vertex,
                    point: point.position().get(),
                })
            })
            .collect::<Result<Vec<_>, CodecError>>()?;
        let mut findings = Vec::new();
        let mut edges = Vec::new();
        let mut used_curves = BTreeSet::new();
        for edge in ordered_edges {
            let curve_id = edge.curve().ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "edge {} has no writable curve",
                    edge.id.as_str()
                ))
            })?;
            let curve = geometry
                .curves
                .iter()
                .find(|curve| curve.id == *curve_id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!("curve {} is missing", curve_id.as_str()))
                })?;
            used_curves.insert(curve_id.as_str());
            let domain = edge
                .param_range()
                .ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "edge {} has no parameter range",
                        edge.id.as_str()
                    ))
                })?
                .finite_components();
            let [lo, hi] = FiniteReal::raw_array(domain);
            if lo == hi {
                return Err(CodecError::NotImplemented(format!(
                    "edge {} has a zero-length parameter range",
                    edge.id.as_str()
                )));
            }
            let (geometry, expected_start, expected_end) = match &curve.geometry {
                CurveGeometry::Solved(SolvedCurveGeometry::Line(line)) => {
                    let origin = line.origin().get();
                    let direction = *line.direction().as_raw();
                    if (direction.norm() - 1.0).abs() > EPS_WRITE_DEGENERATE {
                        return Err(CodecError::NotImplemented(format!(
                            "edge {} line parameterization exceeds Rhino's unit-direction bound",
                            edge.id.as_str()
                        )));
                    }
                    (
                        WritableEdgeCurve::Line(*line),
                        Point3::new(
                            origin.x + direction.x * lo,
                            origin.y + direction.y * lo,
                            origin.z + direction.z * lo,
                        ),
                        Point3::new(
                            origin.x + direction.x * hi,
                            origin.y + direction.y * hi,
                            origin.z + direction.z * hi,
                        ),
                    )
                }
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
                    check_nurbs_curve(curve.id.as_str(), nurbs)?;
                    let count = nurbs.control_points().len();
                    if nurbs.periodic()
                        || [
                            nurbs.knots()[usize::try_from(nurbs.degree()).map_err(|_| {
                                CodecError::Malformed("Rhino count exceeds address space".into())
                            })?],
                            nurbs.knots()[count],
                        ] != [lo, hi]
                    {
                        return Err(CodecError::NotImplemented(format!(
                            "edge {} requires a nonperiodic full-domain NURBS curve",
                            edge.id.as_str()
                        )));
                    }
                    let geometry = WritableEdgeCurve::Nurbs(nurbs);
                    let expected_start =
                        geometry.point(domain[0]).map_err(|failure| match failure {
                            cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit) => {
                                CodecError::ResourceLimit(limit)
                            }
                            cadmpeg_ir::eval::EvaluationFailure::NoValue
                            | cadmpeg_ir::eval::EvaluationFailure::NonFinite(_)
                            | cadmpeg_ir::eval::EvaluationFailure::WorkExhausted => {
                                CodecError::malformed(format_args!(
                                    "edge {} curve has no finite start point",
                                    edge.id.as_str()
                                ))
                            }
                        })?;
                    let expected_end =
                        geometry.point(domain[1]).map_err(|failure| match failure {
                            cadmpeg_ir::eval::EvaluationFailure::ResourceLimit(limit) => {
                                CodecError::ResourceLimit(limit)
                            }
                            cadmpeg_ir::eval::EvaluationFailure::NoValue
                            | cadmpeg_ir::eval::EvaluationFailure::NonFinite(_)
                            | cadmpeg_ir::eval::EvaluationFailure::WorkExhausted => {
                                CodecError::malformed(format_args!(
                                    "edge {} curve has no finite end point",
                                    edge.id.as_str()
                                ))
                            }
                        })?;
                    (geometry, expected_start.get(), expected_end.get())
                }
                _ => {
                    return Err(CodecError::NotImplemented(format!(
                        "edge curve {} is not a line or NURBS curve",
                        curve.id.as_str()
                    )))
                }
            };
            let ends = super::edge_ends(edge)?;
            let start = resolve(&vertex_positions, ends.start.as_str())?;
            let end = resolve(&vertex_positions, ends.end.as_str())?;
            let tolerance = stated_write_tolerance(edge.tolerance, ir);
            if !close_point(vertices[start].point, expected_start, tolerance)
                || !close_point(vertices[end].point, expected_end, tolerance)
            {
                findings.push(record_invalid(
                    EntityKind::Edge,
                    format!(
                        "edge {} endpoints disagree with its curve {}",
                        edge.id.as_str(),
                        curve.id.as_str()
                    ),
                ));
            }
            edges.push(WritableEdge {
                source: edge,
                start,
                end,
                domain,
                curve_id: curve.id.as_str(),
                curve: geometry,
                uses: Vec::new(),
            });
        }
        if layout == Layout::MultiFace && used_curves.len() != geometry.curves.len() {
            return Err(CodecError::NotImplemented(
                "multi-face planar sheet requires one distinct line curve per edge".into(),
            ));
        }
        let surfaces = geometry
            .surfaces
            .iter()
            .map(WritableFaceSurface::try_new)
            .collect::<Result<Vec<_>, _>>()?;
        for surface in &surfaces {
            if let WritableFaceSurface::Nurbs { surface: nurbs, .. } = surface {
                if nurbs.u_periodic() || nurbs.v_periodic() {
                    return Err(CodecError::NotImplemented(
                        "Brep patch surface must be nonperiodic".into(),
                    ));
                }
            }
        }
        let mut used_surfaces = BTreeSet::new();
        let mut owned_loops = BTreeSet::new();
        let faces = geometry
            .faces
            .iter()
            .map(|face| {
                let boundary = face.loops.boundary().filter(|loops| !loops.is_empty());
                if face.shell != shell.id
                    || boundary.is_none()
                    || !used_surfaces.insert(face.surface.as_str())
                {
                    return Err(CodecError::NotImplemented(format!(
                        "face {} has unsupported ownership or shared surface state",
                        face.id.as_str()
                    )));
                }
                let loops = boundary
                    .into_iter()
                    .flatten()
                    .map(|id| {
                        if !owned_loops.insert(id.as_str()) {
                            return Err(CodecError::malformed(format_args!(
                                "loop {} has multiple face owners",
                                id.as_str()
                            )));
                        }
                        resolve(&loop_positions, id.as_str())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(WritableFace {
                    source: face,
                    surface: resolve(&surface_positions, face.surface.as_str())?,
                    loops,
                })
            })
            .collect::<Result<Vec<_>, CodecError>>()?;
        if owned_loops.len() != geometry.loops.len() {
            return Err(CodecError::NotImplemented(
                "Brep contains orphan loops".into(),
            ));
        }
        let mut owned_coedges = BTreeSet::new();
        let loops = geometry
            .loops
            .iter()
            .map(|loop_| {
                let face = resolve(&face_positions, loop_.face.as_str())?;
                if !faces[face].source.loops.contains(&loop_.id) || loop_.coedges().is_empty() {
                    return Err(CodecError::malformed(format_args!(
                        "loop {} ownership or boundary is invalid",
                        loop_.id.as_str()
                    )));
                }
                let mut coedges = loop_
                    .coedges()
                    .iter()
                    .map(|id| {
                        if !owned_coedges.insert(id.as_str()) {
                            return Err(CodecError::malformed(format_args!(
                                "coedge {} has multiple owners",
                                id.as_str()
                            )));
                        }
                        resolve(&coedge_positions, id.as_str())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if faces[face].source.sense == Sense::Reversed {
                    coedges.reverse();
                }
                Ok(WritableLoop {
                    source: loop_,
                    face,
                    coedges,
                })
            })
            .collect::<Result<Vec<_>, CodecError>>()?;
        if owned_coedges.len() != geometry.coedges.len() {
            return Err(CodecError::NotImplemented(
                "Brep contains orphan coedges".into(),
            ));
        }
        let mut used_pcurves = BTreeSet::new();
        let coedges = ordered_coedges
            .into_iter()
            .map(|coedge| {
                let edge = match singular.get(coedge.edge.as_str()) {
                    Some((vertex, _)) => TrimEdge::Singular(*vertex),
                    None => TrimEdge::Edge(resolve(&edge_positions, coedge.edge.as_str())?),
                };
                let edge_domain = match edge {
                    TrimEdge::Edge(position) => Some(FiniteReal::raw_array(edges[position].domain)),
                    TrimEdge::Singular(_) => None,
                };
                let owner_loop = resolve(&loop_positions, coedge.owner_loop.as_str())?;
                if !loops[owner_loop].source.coedges().contains(&coedge.id) {
                    return Err(CodecError::malformed(format_args!(
                        "coedge {} ownership is inconsistent",
                        coedge.id.as_str()
                    )));
                }
                let face_reversed =
                    faces[loops[owner_loop].face].source.sense == Sense::Reversed;
                let reversed = (coedge.sense == Sense::Reversed) != face_reversed;
                let pcurve = match coedge.pcurves.as_slice() {
                    [] => None,
                    [use_] => {
                        let id = &use_.pcurve;
                        if !used_pcurves.insert(id.as_str()) {
                            return Err(CodecError::NotImplemented(format!(
                                "pcurve {} is shared by multiple coedges",
                                id.as_str()
                            )));
                        }
                        let pcurve = geometry
                            .pcurves
                            .iter()
                            .find(|pcurve| pcurve.id == *id)
                            .ok_or_else(|| {
                                CodecError::malformed(format_args!(
                                    "pcurve {} is missing",
                                    id.as_str()
                                ))
                            })?;
                        let against_coedge = use_.parameter_range.is_some_and(|range| {
                            let [start, end] = range.endpoints();
                            start > end
                        });
                        Some(admit_pcurve(
                            edge_domain,
                            pcurve,
                            against_coedge != face_reversed,
                        )?)
                    }
                    _ => {
                        return Err(CodecError::NotImplemented(format!(
                            "coedge {} has multiple pcurve uses; Rhino stores one trim C2 carrier",
                            coedge.id.as_str()
                        )))
                    }
                };
                let fit_tolerance = pcurve
                    .as_ref()
                    .and_then(|pcurve| pcurve.source.fit_tolerance())
                    .map_or(0.0, cadmpeg_ir::geometry::legacy_cache::FitTolerance::get);
                let face = &faces[loops[owner_loop].face];
                let face_tolerance = stated_write_tolerance(face.source.tolerance, ir);
                let edge = match edge {
                    TrimEdge::Singular(vertex) => {
                        let pcurve = pcurve.ok_or_else(|| {
                            CodecError::NotImplemented(format!(
                                "coedge {} runs along singular edge {} and has no pcurve",
                                coedge.id.as_str(),
                                coedge.edge.as_str(),
                            ))
                        })?;
                        let edge_tolerance = singular
                            .get(coedge.edge.as_str())
                            .and_then(|(_, edge)| edge.tolerance)
                            .map_or(0.0, cadmpeg_ir::scalar::PositiveLength::get);
                        if let Some(finding) = validate_singular_trim(
                            surfaces[face.surface],
                            face_tolerance.max(edge_tolerance),
                            vertices[vertex].point,
                            &pcurve,
                        )? {
                            findings.push(record_invalid(EntityKind::Pcurve, finding));
                        }
                        let iso = singular_trim_iso(surfaces[face.surface], &pcurve);
                        return Ok(WritableCoedge {
                            source: coedge,
                            edge: TrimEdge::Singular(vertex),
                            owner_loop,
                            radial_next: resolve(&coedge_positions, coedge.radial_next.as_str())?,
                            reversed,
                            c2: pcurve.payload,
                            domain: pcurve.domain,
                            fit_tolerance,
                            iso,
                        });
                    }
                    TrimEdge::Edge(position) => position,
                };
                let (c2, domain) = match surfaces[face.surface] {
                    WritableFaceSurface::Plane { origin, normal, u_axis } => {
                        let generated = generated_projected_brep_c2_curve(
                            &vertices, &edges[edge], reversed, origin, normal, u_axis,
                        )?;
                        match pcurve {
                            None => (generated, FiniteReal::raw_array(edges[edge].domain)),
                            Some(pcurve) => {
                                if pcurve.payload != generated {
                                    findings.push(record_invalid(
                                        EntityKind::Pcurve,
                                        format!(
                                            "pcurve {} does not exactly match its directed planar C3 projection",
                                            pcurve.source.id.as_str(),
                                        ),
                                    ));
                                }
                                (pcurve.payload, pcurve.domain)
                            }
                        }
                    }
                    WritableFaceSurface::Nurbs { surface, .. } => {
                        let pcurve = pcurve.ok_or_else(|| {
                            CodecError::NotImplemented(format!(
                                "coedge {} has no explicit pcurve", coedge.id.as_str(),
                            ))
                        })?;
                        let (from, to) = if reversed {
                            (edges[edge].end, edges[edge].start)
                        } else {
                            (edges[edge].start, edges[edge].end)
                        };
                        if let Some(finding) = validate_nurbs_trim(
                            surface, face_tolerance, &edges[edge],
                            [vertices[from].point, vertices[to].point], &pcurve,
                        )? {
                            findings.push(record_invalid(EntityKind::Pcurve, finding));
                        }
                        (pcurve.payload, pcurve.domain)
                    }
                };
                Ok(WritableCoedge {
                    source: coedge,
                    edge: TrimEdge::Edge(edge),
                    owner_loop,
                    radial_next: resolve(&coedge_positions, coedge.radial_next.as_str())?,
                    reversed,
                    c2,
                    domain,
                    fit_tolerance,
                    iso: 0,
                })
            })
            .collect::<Result<Vec<_>, CodecError>>()?;
        if used_pcurves.len() != geometry.pcurves.len() {
            return Err(CodecError::NotImplemented(
                "orphan Brep pcurves are not writable".into(),
            ));
        }
        let mut result = Self {
            body,
            vertices,
            edges,
            coedges,
            loops,
            faces,
            surfaces,
            losses: findings,
        };
        for loop_ in &result.loops {
            for (index, position) in loop_.coedges.iter().copied().enumerate() {
                let next = loop_.coedges[(index + 1) % loop_.coedges.len()];
                if result.endpoints(position).1 != result.endpoints(next).0 {
                    return Err(CodecError::malformed(format_args!(
                        "loop {} coedge traversal does not close",
                        loop_.source.id.as_str()
                    )));
                }
            }
        }
        let mut open_solid_edges = Vec::new();
        for (position, edge) in result.edges.iter_mut().enumerate() {
            let uses = result
                .coedges
                .iter()
                .enumerate()
                .filter(|(_, coedge)| coedge.edge == TrimEdge::Edge(position))
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if uses.is_empty() || uses.len() > 2 || layout == Layout::SingleFace && uses.len() != 1
            {
                return Err(CodecError::NotImplemented(format!(
                    "edge {} incidence is incompatible with the body kind",
                    edge.source.id.as_str()
                )));
            }
            if body.kind == BodyKind::Solid && uses.len() == 1 {
                open_solid_edges.push(edge.source.id.as_str().to_owned());
            }
            let start = uses[0];
            let mut current = start;
            let mut ordered = vec![start];
            while ordered.len() < uses.len() {
                let next = result.coedges[current].radial_next;
                if !uses.contains(&next) || ordered.contains(&next) {
                    return Err(CodecError::malformed(format_args!(
                        "edge {} radial ring is inconsistent",
                        edge.source.id.as_str()
                    )));
                }
                ordered.push(next);
                current = next;
            }
            if result.coedges[current].radial_next != start {
                return Err(CodecError::malformed(format_args!(
                    "edge {} radial ring does not close",
                    edge.source.id.as_str()
                )));
            }
            if ordered.len() == 2
                && result.coedges[ordered[0]].source.sense
                    == result.coedges[ordered[1]].source.sense
            {
                return Err(CodecError::malformed(format_args!(
                    "shared edge {} has equal directed uses",
                    edge.source.id.as_str()
                )));
            }
            edge.uses = ordered;
        }
        for id in singular.keys() {
            let uses = result
                .coedges
                .iter()
                .filter(|coedge| coedge.source.edge.as_str() == *id)
                .count();
            if uses != 1 {
                return Err(CodecError::NotImplemented(format!(
                    "singular edge {id} has {uses} uses, and a 3DM singular trim is one use"
                )));
            }
        }
        if layout == Layout::MultiFace {
            let mut reached = BTreeSet::from([0_usize]);
            loop {
                let prior = reached.len();
                for edge in &result.edges {
                    let faces = edge
                        .uses
                        .iter()
                        .map(|coedge| result.loops[result.coedges[*coedge].owner_loop].face)
                        .collect::<Vec<_>>();
                    if faces.iter().any(|face| reached.contains(face)) {
                        reached.extend(faces);
                    }
                }
                if reached.len() == prior {
                    break;
                }
            }
            if reached.len() != result.faces.len() {
                return Err(CodecError::NotImplemented(
                    "multi-face planar sheet must be edge-connected in one shell".into(),
                ));
            }
        }
        let mut plane_findings = Vec::new();
        for loop_ in &result.loops {
            let face = &result.faces[loop_.face];
            match result.surfaces[face.surface] {
                WritableFaceSurface::Nurbs { .. } => {}
                WritableFaceSurface::Plane {
                    origin,
                    normal,
                    u_axis,
                } => {
                    let tolerance = stated_write_tolerance(face.source.tolerance, ir);
                    let mut boundary = Vec::new();
                    for position in &loop_.coedges {
                        let coedge = &result.coedges[*position];
                        let (start, end) = result.endpoints(*position);
                        if let TrimEdge::Edge(edge) = coedge.edge {
                            let edge = &result.edges[edge];
                            if let WritableEdgeCurve::Nurbs(nurbs) = edge.curve {
                                for point in nurbs.control_points() {
                                    let distance = (point.x - origin.x) * normal.x
                                        + (point.y - origin.y) * normal.y
                                        + (point.z - origin.z) * normal.z;
                                    if distance.abs() > tolerance {
                                        plane_findings.push(record_invalid(
                                        EntityKind::Curve,
                                        format!(
                                            "edge curve {} is outside the plane tolerance of face {}",
                                            edge.curve_id,
                                            face.source.id.as_str()
                                        ),
                                    ));
                                        break;
                                    }
                                }
                            }
                        }
                        boundary.push(super::plane_uv(
                            result.vertices[start].point,
                            origin,
                            normal,
                            u_axis,
                        ));
                        for vertex in [start, end] {
                            let point = result.vertices[vertex].point;
                            let distance = (point.x - origin.x) * normal.x
                                + (point.y - origin.y) * normal.y
                                + (point.z - origin.z) * normal.z;
                            if distance.abs() > tolerance {
                                plane_findings.push(record_invalid(
                                    EntityKind::Vertex,
                                    format!(
                                        "vertex {} of loop {} is outside its face plane tolerance",
                                        result.vertices[vertex].source.id.as_str(),
                                        loop_.source.id.as_str()
                                    ),
                                ));
                            }
                        }
                    }
                    let polygonal =
                        loop_
                            .coedges
                            .iter()
                            .all(|position| match result.coedges[*position].edge {
                                TrimEdge::Edge(edge) => {
                                    matches!(result.edges[edge].curve, WritableEdgeCurve::Line(_))
                                }
                                TrimEdge::Singular(_) => true,
                            });
                    if layout == Layout::MultiFace && polygonal {
                        let twice_area = boundary
                            .iter()
                            .zip(boundary.iter().cycle().skip(1))
                            .take(boundary.len())
                            .map(|(from, to)| from[0] * to[1] - to[0] * from[1])
                            .sum::<f64>();
                        if !twice_area.is_finite() || twice_area.abs() <= tolerance * tolerance {
                            plane_findings.push(record_invalid(
                                EntityKind::Loop,
                                format!(
                                    "loop {} has degenerate planar area",
                                    loop_.source.id.as_str()
                                ),
                            ));
                        }
                    }
                }
            }
        }
        result.losses.extend(plane_findings);
        if !open_solid_edges.is_empty() {
            result.losses.push(record_invalid(
                EntityKind::Body,
                format!(
                    "solid body {} has edges with one use and is written as an open Brep: {}",
                    body.id.as_str(),
                    open_solid_edges.join(", ")
                ),
            ));
        }
        result.losses.extend(cadmpeg_ir::codec::write::value_unwritable(
            "Rhino",
            "writes these Brep faces without their name or color, which a 3DM Brep face does not hold",
            geometry
                .faces
                .iter()
                .filter(|face| face.name.is_some() || face.color.is_some())
                .map(|face| face.id.as_str().to_owned()),
        ));
        Ok(result)
    }

    pub(super) fn endpoints(&self, coedge: usize) -> (usize, usize) {
        let coedge = &self.coedges[coedge];
        let edge = match coedge.edge {
            TrimEdge::Edge(edge) => &self.edges[edge],
            TrimEdge::Singular(vertex) => return (vertex, vertex),
        };
        if coedge.reversed {
            (edge.end, edge.start)
        } else {
            (edge.start, edge.end)
        }
    }
}
