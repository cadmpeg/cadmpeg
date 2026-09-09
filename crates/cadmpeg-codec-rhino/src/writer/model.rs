use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Body, BodyKind, Coedge, Edge, Face, Loop, Sense, Vertex};

use super::{
    admit_pcurve, check_frame, check_nurbs_curve, check_nurbs_surface, check_object_attributes,
    close_point, generated_projected_brep_c2_curve, validate_nurbs_trim, EPS_WRITE_DEGENERATE,
};

pub(super) struct WritableModel<'a> {
    pub(super) body: &'a Body,
    pub(super) vertices: Vec<WritableVertex<'a>>,
    pub(super) edges: Vec<WritableEdge<'a>>,
    pub(super) coedges: Vec<WritableCoedge<'a>>,
    pub(super) loops: Vec<WritableLoop<'a>>,
    pub(super) faces: Vec<WritableFace<'a>>,
    pub(super) surfaces: Vec<WritableFaceSurface<'a>>,
}

pub(super) struct WritableVertex<'a> {
    pub(super) source: &'a Vertex,
    pub(super) point: Point3,
}

pub(super) struct WritableEdge<'a> {
    pub(super) source: &'a Edge,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) domain: [f64; 2],
    pub(super) curve_id: &'a str,
    pub(super) curve: WritableEdgeCurve<'a>,
    pub(super) uses: Vec<usize>,
}

#[derive(Clone, Copy)]
pub(super) enum WritableEdgeCurve<'a> {
    Line(cadmpeg_ir::geometry::LineCurve),
    Nurbs(&'a NurbsCurve),
}

impl WritableEdgeCurve<'_> {
    pub(super) fn point(self, parameter: f64) -> Option<Point3> {
        match self {
            Self::Line(line) => {
                cadmpeg_ir::eval::curve_point(&CurveGeometry::Line(line), parameter)
            }
            Self::Nurbs(nurbs) => cadmpeg_ir::eval::nurbs_curve_point(
                nurbs.degree(),
                nurbs.knots(),
                nurbs.control_points(),
                nurbs.weights(),
                cadmpeg_ir::eval::map_nurbs_curve_parameter(nurbs, parameter)?,
            ),
        }
    }
}

pub(super) struct WritableCoedge<'a> {
    pub(super) source: &'a Coedge,
    pub(super) edge: usize,
    pub(super) owner_loop: usize,
    pub(super) radial_next: usize,
    pub(super) c2: ([u8; 16], Vec<u8>),
    pub(super) fit_tolerance: f64,
}

pub(super) struct WritablePcurve<'a> {
    pub(super) source: &'a Pcurve,
    pub(super) payload: ([u8; 16], Vec<u8>),
    pub(super) hull: Vec<cadmpeg_ir::math::Point2>,
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

#[derive(Clone, Copy)]
pub(super) enum WritableFaceSurface<'a> {
    Plane {
        origin: Point3,
        normal: Vector3,
        u_axis: Vector3,
    },
    Nurbs(&'a NurbsSurface),
}

impl<'a> WritableFaceSurface<'a> {
    pub(super) fn try_new(surface: &'a cadmpeg_ir::geometry::Surface) -> Result<Self, CodecError> {
        if surface.source_object.is_some() {
            return Err(CodecError::NotImplemented(format!(
                "surface {} source-object state is not writable",
                surface.id.as_str(),
            )));
        }
        match &surface.geometry {
            SurfaceGeometry::Plane(plane) => {
                let (origin, normal, u_axis) = plane.parts();
                check_frame(surface.id.as_str(), *normal, *u_axis, "plane")?;
                Ok(Self::Plane {
                    origin: *origin,
                    normal: *normal,
                    u_axis: *u_axis,
                })
            }
            SurfaceGeometry::Nurbs(nurbs) => {
                check_nurbs_surface(surface.id.as_str(), nurbs)?;
                Ok(Self::Nurbs(nurbs))
            }
            _ => Err(CodecError::NotImplemented(format!(
                "surface {} is not a plane or NURBS patch",
                surface.id.as_str(),
            ))),
        }
    }

    pub(super) fn payload(self) -> ([u8; 16], Vec<u8>) {
        match self {
            Self::Plane {
                origin,
                normal,
                u_axis,
            } => (
                super::PLANE_SURFACE_CLASS,
                super::plane_surface_payload(origin, normal, u_axis),
            ),
            Self::Nurbs(nurbs) => (
                super::NURBS_SURFACE_CLASS,
                super::nurbs_surface_payload(nurbs),
            ),
        }
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
        let Some(body) = model
            .bodies
            .iter()
            .find(|body| matches!(body.kind, BodyKind::Sheet | BodyKind::Solid))
        else {
            return Err(CodecError::NotImplemented(
                "body topology is not a writable Brep".into(),
            ));
        };
        let layout = if model.faces.len() > 1 || body.kind == BodyKind::Solid {
            Layout::MultiFace
        } else {
            Layout::SingleFace
        };
        let edge_count = model.coedges.len();
        let invalid_counts = match layout {
            Layout::SingleFace => {
                model.faces.len() != 1
                    || model.loops.is_empty()
                    || edge_count < 3
                    || model.edges.len() != edge_count
                    || model.vertices.len() != edge_count
                    || model.points.len() != edge_count
                    || model.curves.len() != edge_count
                    || model.surfaces.len() != 1
            }
            Layout::MultiFace => {
                model.faces.len() < 2
                    || model.loops.len() < model.faces.len()
                    || edge_count < 3 * model.faces.len()
                    || model.edges.is_empty()
                    || model.vertices.is_empty()
                    || model.points.len() != model.vertices.len()
                    || model.curves.len() != model.edges.len()
                    || model.surfaces.len() != model.faces.len()
            }
        };
        if model.bodies.len() != 1
            || model.regions.len() != 1
            || model.shells.len() != 1
            || !model.tessellations.is_empty()
            || invalid_counts
        {
            return Err(CodecError::NotImplemented("Brep writing requires one shell with distinct explicit carriers and polygonal loops".into()));
        }
        if body.regions.len() != 1 || body.transform.is_some() {
            return Err(CodecError::NotImplemented(
                "Brep body placement is not writable".into(),
            ));
        }
        check_object_attributes(body.id.as_str(), body.name.as_deref())?;
        let region = &model.regions[0];
        let shell = &model.shells[0];
        if region.id != body.regions[0]
            || region.body != body.id
            || region.shells != [shell.id.clone()]
            || shell.region != region.id
            || shell.faces()
                != model
                    .faces
                    .iter()
                    .map(|face| face.id.clone())
                    .collect::<Vec<_>>()
            || !shell.wire_edges().is_empty()
            || !shell.free_vertices().is_empty()
        {
            return Err(CodecError::Malformed(
                "Brep ownership graph is inconsistent".into(),
            ));
        }
        let original_coedges = positions(model.coedges.iter().map(|coedge| coedge.id.as_str()))?;
        let original_edges = positions(model.edges.iter().map(|edge| edge.id.as_str()))?;
        let original_vertices = positions(model.vertices.iter().map(|vertex| vertex.id.as_str()))?;
        let mut ordered_coedges = Vec::new();
        let mut ordered_edges = Vec::new();
        let mut ordered_vertices = Vec::new();
        match layout {
            Layout::SingleFace => {
                if model.faces[0].loops
                    != model
                        .loops
                        .iter()
                        .map(|loop_| loop_.id.clone())
                        .collect::<Vec<_>>()
                {
                    return Err(CodecError::Malformed(
                        "single-face loop order is inconsistent".into(),
                    ));
                }
                let mut used_edges = BTreeSet::new();
                let mut used_vertices = BTreeSet::new();
                for loop_ in &model.loops {
                    for id in loop_.coedges() {
                        let coedge = &model.coedges[resolve(&original_coedges, id.as_str())?];
                        let edge = &model.edges[resolve(&original_edges, coedge.edge.as_str())?];
                        if !used_edges.insert(edge.id.as_str()) {
                            return Err(CodecError::NotImplemented(
                                "planar sheet cannot reuse an edge in one loop".into(),
                            ));
                        }
                        let start = if coedge.sense == Sense::Forward {
                            &edge.start
                        } else {
                            &edge.end
                        };
                        let vertex = &model.vertices[resolve(&original_vertices, start.as_str())?];
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
                ordered_coedges.extend(&model.coedges);
                ordered_edges.extend(&model.edges);
                ordered_vertices.extend(&model.vertices);
            }
        }
        let vertex_positions = positions(ordered_vertices.iter().map(|vertex| vertex.id.as_str()))?;
        let edge_positions = positions(ordered_edges.iter().map(|edge| edge.id.as_str()))?;
        let coedge_positions = positions(ordered_coedges.iter().map(|coedge| coedge.id.as_str()))?;
        let loop_positions = positions(model.loops.iter().map(|loop_| loop_.id.as_str()))?;
        let face_positions = positions(model.faces.iter().map(|face| face.id.as_str()))?;
        let surface_positions =
            positions(model.surfaces.iter().map(|surface| surface.id.as_str()))?;
        let mut used_points = BTreeSet::new();
        let vertices = ordered_vertices
            .into_iter()
            .map(|vertex| {
                let point = model
                    .points
                    .iter()
                    .find(|point| point.id == vertex.point)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "point {} is missing",
                            vertex.point.as_str()
                        ))
                    })?;
                if layout == Layout::MultiFace && !used_points.insert(point.id.as_str())
                    || !point.position.x.is_finite()
                    || !point.position.y.is_finite()
                    || !point.position.z.is_finite()
                {
                    return Err(CodecError::malformed(format_args!(
                        "vertex {} has a shared or invalid point",
                        vertex.id.as_str()
                    )));
                }
                Ok(WritableVertex {
                    source: vertex,
                    point: point.position,
                })
            })
            .collect::<Result<Vec<_>, CodecError>>()?;
        let mut edges = Vec::new();
        let mut used_curves = BTreeSet::new();
        for edge in ordered_edges {
            let curve_id = edge.curve.as_ref().ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "edge {} has no writable curve",
                    edge.id.as_str()
                ))
            })?;
            let curve = model
                .curves
                .iter()
                .find(|curve| curve.id == *curve_id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!("curve {} is missing", curve_id.as_str()))
                })?;
            used_curves.insert(curve_id.as_str());
            if curve.source_object.is_some() {
                return Err(CodecError::NotImplemented(format!(
                    "edge curve {} source-object state is not writable",
                    curve.id.as_str()
                )));
            }
            let domain = edge.param_range.ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "edge {} has no parameter range",
                    edge.id.as_str()
                ))
            })?;
            let [lo, hi] = domain;
            if !lo.is_finite() || !hi.is_finite() || lo >= hi {
                return Err(CodecError::malformed(format_args!(
                    "edge {} has an invalid parameter range",
                    edge.id.as_str()
                )));
            }
            let (geometry, expected_start, expected_end) = match &curve.geometry {
                CurveGeometry::Line(line) => {
                    let (origin, direction) = line.parts();
                    if (direction.norm() - 1.0).abs() > EPS_WRITE_DEGENERATE {
                        return Err(CodecError::malformed(format_args!(
                            "edge {} has an invalid line parameterization",
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
                CurveGeometry::Nurbs(nurbs) => {
                    check_nurbs_curve(curve.id.as_str(), nurbs)?;
                    let count = nurbs.control_points().len();
                    if nurbs.periodic()
                        || [nurbs.knots()[nurbs.degree() as usize], nurbs.knots()[count]] != domain
                    {
                        return Err(CodecError::NotImplemented(format!(
                            "edge {} requires a nonperiodic full-domain NURBS curve",
                            edge.id.as_str()
                        )));
                    }
                    (
                        WritableEdgeCurve::Nurbs(nurbs),
                        nurbs.control_points()[0],
                        nurbs.control_points()[count - 1],
                    )
                }
                _ => {
                    return Err(CodecError::NotImplemented(format!(
                        "edge curve {} is not a line or NURBS curve",
                        curve.id.as_str()
                    )))
                }
            };
            let start = resolve(&vertex_positions, edge.start.as_str())?;
            let end = resolve(&vertex_positions, edge.end.as_str())?;
            let tolerance = edge
                .tolerance
                .map_or(
                    ir.tolerances.linear.get(),
                    cadmpeg_ir::units::PositiveScalar::get,
                )
                .max(EPS_WRITE_DEGENERATE);
            if !close_point(vertices[start].point, expected_start, tolerance)
                || !close_point(vertices[end].point, expected_end, tolerance)
            {
                return Err(CodecError::malformed(format_args!(
                    "edge {} endpoints disagree with its line curve",
                    edge.id.as_str()
                )));
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
        if layout == Layout::MultiFace && used_curves.len() != model.curves.len() {
            return Err(CodecError::NotImplemented(
                "multi-face planar sheet requires one distinct line curve per edge".into(),
            ));
        }
        let surfaces = model
            .surfaces
            .iter()
            .map(WritableFaceSurface::try_new)
            .collect::<Result<Vec<_>, _>>()?;
        for surface in &surfaces {
            if let WritableFaceSurface::Nurbs(nurbs) = surface {
                if nurbs.u_periodic() || nurbs.v_periodic() {
                    return Err(CodecError::NotImplemented(
                        "Brep patch surface must be nonperiodic".into(),
                    ));
                }
            }
        }
        let mut used_surfaces = BTreeSet::new();
        let mut owned_loops = BTreeSet::new();
        let faces = model
            .faces
            .iter()
            .map(|face| {
                if face.shell != shell.id
                    || face.loops.is_empty()
                    || face.name.is_some()
                    || face.color.is_some()
                    || !used_surfaces.insert(face.surface.as_str())
                {
                    return Err(CodecError::NotImplemented(format!(
                        "face {} has unsupported ownership, attributes, or shared surface state",
                        face.id.as_str()
                    )));
                }
                let loops = face
                    .loops
                    .iter()
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
        if owned_loops.len() != model.loops.len() {
            return Err(CodecError::NotImplemented(
                "Brep contains orphan loops".into(),
            ));
        }
        let mut owned_coedges = BTreeSet::new();
        let loops = model
            .loops
            .iter()
            .map(|loop_| {
                let face = resolve(&face_positions, loop_.face.as_str())?;
                if !faces[face].source.loops.contains(&loop_.id) || loop_.coedges().len() < 3 {
                    return Err(CodecError::malformed(format_args!(
                        "loop {} ownership or boundary is invalid",
                        loop_.id.as_str()
                    )));
                }
                let coedges = loop_
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
                Ok(WritableLoop {
                    source: loop_,
                    face,
                    coedges,
                })
            })
            .collect::<Result<Vec<_>, CodecError>>()?;
        if owned_coedges.len() != model.coedges.len() {
            return Err(CodecError::NotImplemented(
                "Brep contains orphan coedges".into(),
            ));
        }
        let mut used_pcurves = BTreeSet::new();
        let coedges = ordered_coedges
            .into_iter()
            .map(|coedge| {
                let edge = resolve(&edge_positions, coedge.edge.as_str())?;
                let owner_loop = resolve(&loop_positions, coedge.owner_loop.as_str())?;
                if !loops[owner_loop].source.coedges().contains(&coedge.id) {
                    return Err(CodecError::malformed(format_args!(
                        "coedge {} ownership is inconsistent",
                        coedge.id.as_str()
                    )));
                }
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
                        let pcurve = model
                            .pcurves
                            .iter()
                            .find(|pcurve| pcurve.id == *id)
                            .ok_or_else(|| {
                                CodecError::malformed(format_args!(
                                    "pcurve {} is missing",
                                    id.as_str()
                                ))
                            })?;
                        Some(admit_pcurve(&edges[edge], pcurve)?)
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
                    .unwrap_or(0.0);
                let face = &faces[loops[owner_loop].face];
                let c2 = match surfaces[face.surface] {
                    WritableFaceSurface::Plane { origin, normal, u_axis } => {
                        let generated = generated_projected_brep_c2_curve(
                            &vertices, &edges[edge], coedge.sense, origin, u_axis,
                            normal.cross(u_axis),
                        )?;
                        match pcurve {
                            None => generated,
                            Some(pcurve) if pcurve.payload == generated => pcurve.payload,
                            Some(pcurve) => {
                                return Err(CodecError::malformed(format_args!(
                                    "pcurve {} does not exactly match its directed planar C3 projection",
                                    pcurve.source.id.as_str(),
                                )))
                            }
                        }
                    }
                    WritableFaceSurface::Nurbs(surface) => {
                        let pcurve = pcurve.ok_or_else(|| {
                            CodecError::NotImplemented(format!(
                                "coedge {} has no explicit pcurve", coedge.id.as_str(),
                            ))
                        })?;
                        validate_nurbs_trim(
                            surface, face.source.tolerance.map_or(ir.tolerances.linear.get(), cadmpeg_ir::units::PositiveScalar::get),
                            &edges[edge], coedge.sense, &pcurve,
                        )?;
                        pcurve.payload
                    }
                };
                Ok(WritableCoedge {
                    source: coedge,
                    edge,
                    owner_loop,
                    radial_next: resolve(&coedge_positions, coedge.radial_next.as_str())?,
                    c2,
                    fit_tolerance,
                })
            })
            .collect::<Result<Vec<_>, CodecError>>()?;
        if used_pcurves.len() != model.pcurves.len() {
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
        for (position, edge) in result.edges.iter_mut().enumerate() {
            let uses = result
                .coedges
                .iter()
                .enumerate()
                .filter(|(_, coedge)| coedge.edge == position)
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if uses.is_empty()
                || uses.len() > 2
                || body.kind == BodyKind::Solid && uses.len() != 2
                || layout == Layout::SingleFace && uses.len() != 1
            {
                return Err(CodecError::NotImplemented(format!(
                    "edge {} incidence is incompatible with the body kind",
                    edge.source.id.as_str()
                )));
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
        for loop_ in &result.loops {
            let face = &result.faces[loop_.face];
            match result.surfaces[face.surface] {
                WritableFaceSurface::Nurbs(_) => {}
                WritableFaceSurface::Plane {
                    origin,
                    normal,
                    u_axis,
                } => {
                    let tolerance = face
                        .source
                        .tolerance
                        .map_or(
                            ir.tolerances.linear.get(),
                            cadmpeg_ir::units::PositiveScalar::get,
                        )
                        .max(EPS_WRITE_DEGENERATE);
                    let mut boundary = Vec::new();
                    for position in &loop_.coedges {
                        let coedge = &result.coedges[*position];
                        let edge = &result.edges[coedge.edge];
                        if let WritableEdgeCurve::Nurbs(nurbs) = edge.curve {
                            for point in nurbs.control_points() {
                                let distance = (point.x - origin.x) * normal.x
                                    + (point.y - origin.y) * normal.y
                                    + (point.z - origin.z) * normal.z;
                                if distance.abs() > tolerance {
                                    return Err(CodecError::malformed(format_args!(
                                        "edge curve {} is outside its face plane tolerance",
                                        edge.curve_id
                                    )));
                                }
                            }
                        }
                        let (start, _) = result.endpoints(*position);
                        boundary.push(super::plane_uv(
                            result.vertices[start].point,
                            origin,
                            u_axis,
                            normal.cross(u_axis),
                        ));
                        for vertex in [edge.start, edge.end] {
                            let point = result.vertices[vertex].point;
                            let distance = (point.x - origin.x) * normal.x
                                + (point.y - origin.y) * normal.y
                                + (point.z - origin.z) * normal.z;
                            if distance.abs() > tolerance {
                                return Err(CodecError::malformed(format_args!(
                                    "loop {} vertex is outside its face plane tolerance",
                                    loop_.source.id.as_str()
                                )));
                            }
                        }
                    }
                    if layout == Layout::MultiFace {
                        let twice_area = boundary
                            .iter()
                            .zip(boundary.iter().cycle().skip(1))
                            .take(boundary.len())
                            .map(|(from, to)| from[0] * to[1] - to[0] * from[1])
                            .sum::<f64>();
                        if !twice_area.is_finite() || twice_area.abs() <= tolerance * tolerance {
                            return Err(CodecError::malformed(format_args!(
                                "loop {} has degenerate planar area",
                                loop_.source.id.as_str()
                            )));
                        }
                    }
                }
            }
        }
        Ok(result)
    }

    pub(super) fn endpoints(&self, coedge: usize) -> (usize, usize) {
        let coedge = &self.coedges[coedge];
        let edge = &self.edges[coedge.edge];
        if coedge.source.sense == Sense::Forward {
            (edge.start, edge.end)
        } else {
            (edge.end, edge.start)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{adjacent_quad_sheet, polygon_sheet};
    use super::*;

    #[test]
    fn single_face_resolves_arena_permutations_in_traversal_order() {
        let mut ir = polygon_sheet(&[
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
        ]);
        let expected = super::super::brep_payload(
            &WritableModel::try_new(&ir).expect("writable triangle"),
            crate::RhinoArchiveVersion::V8,
        )
        .expect("triangle payload");
        ir.model.vertices.reverse();
        ir.model.edges.reverse();
        ir.model.coedges.reverse();
        let model =
            WritableModel::try_new(&ir).expect("references resolve independently of arena order");
        let actual = super::super::brep_payload(&model, crate::RhinoArchiveVersion::V8)
            .expect("permuted triangle payload");
        assert_eq!(actual.body, expected.body);
        assert_eq!(actual.direct, expected.direct);
        assert_eq!(model.loops[0].coedges, vec![0, 1, 2]);
        for (position, edge) in model.edges.iter().enumerate() {
            assert_eq!(edge.start, position);
            assert_eq!(edge.end, (position + 1) % 3);
            assert_eq!(edge.uses, vec![position]);
        }
    }

    #[test]
    fn multi_face_resolves_domains_and_incidence_in_arena_order() {
        let ir = adjacent_quad_sheet();
        let model = WritableModel::try_new(&ir).expect("writable adjacent faces");
        for (position, edge) in model.edges.iter().enumerate() {
            assert_eq!(edge.source.id, ir.model.edges[position].id);
            assert_eq!(Some(edge.domain), ir.model.edges[position].param_range);
            for coedge in &edge.uses {
                assert_eq!(model.coedges[*coedge].edge, position);
            }
        }
        for face in &model.faces {
            assert!(matches!(
                model.surfaces[face.surface],
                WritableFaceSurface::Plane { .. }
            ));
        }
    }
}
