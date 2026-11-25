// SPDX-License-Identifier: Apache-2.0
//! Body transforms stated in the geometry that each body owns. The decoder
//! reads a Directory transformation back onto a body only from a Type 186, so
//! the writer places every other transformed body into its points, curves and
//! surfaces (CADIR §3.4).

use super::degrade::{naming, Unwritten};
use super::{is_decoder_free_geometry_body, lifted_pcurve, reverse_nurbs, rigid_placement};
use crate::loss::IgesLossCode;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{
    CircleCurve, ConeSurface, CylinderSurface, DegenerateCurve, EllipseCurve, HyperbolaCurve,
    LineCurve, ParabolaCurve, PlaneSurface, SphereSurface, TorusSurface,
};
use cadmpeg_ir::geometry::pcurve::{Pcurve, PcurveGeometry, PcurveMetadata, PcurveNurbs};
use cadmpeg_ir::geometry::sampled::GeometryLayoutError;
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::{
    Body, BodyKind, IncreasingParameterInterval, LoopBoundary, LoopRing, ParameterInterval, Sense,
    ShellMember,
};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::UnitVector3;
use cadmpeg_ir::CadIr;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

/// Relative bound under which a linear map counts as a similarity: the bound
/// that `Transform::is_proper_rigid` applies to a rigid motion.
const SIMILARITY_TOLERANCE: f64 = 1.0e-9;

/// The transform that the writer places into the geometry of `body`. A solid
/// keeps a rigid transform as the Type 124 of its Type 186.
fn placed_transform(body: &Body) -> Option<Transform> {
    let transform = body
        .transform
        .filter(|transform| *transform != Transform::identity())?;
    let kept = body.kind == BodyKind::Solid
        && !is_decoder_free_geometry_body(body)
        && rigid_placement(transform).is_some();
    (!kept).then_some(transform)
}

/// The records that one body owns, by arena.
#[derive(Default)]
struct Owned<'ir> {
    points: BTreeSet<&'ir str>,
    curves: BTreeSet<&'ir str>,
    surfaces: BTreeSet<&'ir str>,
    faces: BTreeSet<&'ir str>,
    edges: BTreeSet<&'ir str>,
}

fn owned<'ir>(ir: &'ir CadIr, body: &Body) -> Owned<'ir> {
    let geometry = &ir.geometry;
    let mut owned = Owned::default();
    let mut vertices = BTreeSet::new();
    for region in geometry.stated_body_regions(&body.id) {
        for shell_id in region.stated_shells() {
            for shell in geometry.shells.iter().filter(|shell| shell.id == *shell_id) {
                for member in &shell.members {
                    match member {
                        ShellMember::WireEdge { id } => {
                            owned.edges.insert(id.as_str());
                        }
                        ShellMember::FreeVertex { id } => {
                            vertices.insert(id.as_str());
                        }
                        ShellMember::Subd { .. } => {}
                    }
                }
            }
            for face in geometry.stated_shell_faces(shell_id) {
                owned.faces.insert(face.id.as_str());
                owned.surfaces.insert(face.surface.as_str());
                for loop_ in face
                    .loops
                    .stated()
                    .filter_map(|id| geometry.loops.iter().find(|loop_| loop_.id == *id))
                {
                    vertices.extend(loop_.vertices().map(cadmpeg_ir::ids::VertexId::as_str));
                    for coedge in loop_
                        .coedges()
                        .iter()
                        .filter_map(|id| geometry.coedges.iter().find(|coedge| coedge.id == *id))
                    {
                        owned.edges.insert(coedge.edge.as_str());
                        if let Some(use_curve) = &coedge.use_curve {
                            owned.curves.insert(use_curve.curve.as_str());
                        }
                    }
                }
            }
        }
    }
    for edge in geometry
        .edges
        .iter()
        .filter(|edge| owned.edges.contains(edge.id.as_str()))
    {
        vertices.extend(edge.vertices().map(cadmpeg_ir::ids::VertexId::as_str));
        if let Some(curve) = edge.curve() {
            owned.curves.insert(curve.as_str());
        }
    }
    owned.points.extend(
        geometry
            .vertices
            .iter()
            .filter(|vertex| vertices.contains(vertex.id.as_str()))
            .map(|vertex| vertex.point.as_str()),
    );
    owned
}

/// The placed bodies and, for each record they own, its transform and body.
#[derive(Default)]
struct Placements<'ir> {
    bodies: BTreeMap<&'ir str, Transform>,
    points: BTreeMap<&'ir str, (Transform, &'ir str)>,
    curves: BTreeMap<&'ir str, (Transform, &'ir str)>,
    surfaces: BTreeMap<&'ir str, (Transform, &'ir str)>,
    faces: BTreeSet<&'ir str>,
}

impl<'ir> Placements<'ir> {
    fn of(ir: &'ir CadIr) -> Result<Self, Unwritten> {
        let mut placements = Self {
            bodies: ir
                .geometry
                .bodies
                .iter()
                .filter_map(|body| Some((body.id.as_str(), placed_transform(body)?)))
                .collect(),
            ..Self::default()
        };
        if placements.bodies.is_empty() {
            return Ok(placements);
        }
        let mut owners = BTreeMap::<(&str, &str), (Option<Transform>, &str)>::new();
        for body in &ir.geometry.bodies {
            let body_id = body.id.as_str();
            let transform = placements.bodies.get(body_id).copied();
            let owned = owned(ir, body);
            for (arena, ids) in [
                ("point", &owned.points),
                ("curve", &owned.curves),
                ("surface", &owned.surfaces),
            ] {
                for id in ids {
                    if let Some((other, other_body)) =
                        owners.insert((arena, id), (transform, body_id))
                    {
                        if other != transform {
                            return Err(naming(body_id)(CodecError::NotImplemented(format!(
                                "IGES writer places the transform of body {body_id} into its \
                                 geometry, and {arena} {id} is shared with body {other_body} \
                                 under another transform"
                            ))));
                        }
                    }
                }
            }
            let Some(transform) = transform else {
                continue;
            };
            for (ids, map) in [
                (&owned.points, &mut placements.points),
                (&owned.curves, &mut placements.curves),
                (&owned.surfaces, &mut placements.surfaces),
            ] {
                map.extend(ids.iter().map(|id| (*id, (transform, body_id))));
            }
            placements.faces.extend(&owned.faces);
        }
        Ok(placements)
    }
}

/// The cylinder, cone, sphere and torus supports of bodies that the writer
/// places under a transform that is not a similarity. The exact image of
/// each is a rational B-spline surface.
pub(super) fn dissimilar_supports(ir: &CadIr) -> BTreeSet<String> {
    let mut supports = BTreeSet::new();
    for body in &ir.geometry.bodies {
        let Some(transform) = placed_transform(body) else {
            continue;
        };
        if Linear::of(transform).similarity.is_some() {
            continue;
        }
        for surface in owned(ir, body).surfaces {
            if ir.geometry.surfaces.iter().any(|record| {
                record.id.as_str() == surface
                    && matches!(
                        record.geometry.solved(),
                        Some(
                            SolvedSurfaceGeometry::Cylinder(_)
                                | SolvedSurfaceGeometry::Cone(_)
                                | SolvedSurfaceGeometry::Sphere(_)
                                | SolvedSurfaceGeometry::Torus(_)
                        )
                    )
            }) {
                supports.insert(surface.to_owned());
            }
        }
    }
    supports
}

/// The document with the transform of each body that IGES does not state on
/// the body placed into the geometry that the body owns, and a
/// `procedural.reduced` note for each procedural carrier that the placement
/// states by its solved form.
pub(super) fn placed_bodies(ir: &CadIr) -> Result<(Cow<'_, CadIr>, Vec<LossNote>), Unwritten> {
    let placements = Placements::of(ir)?;
    if placements.bodies.is_empty() {
        return Ok((Cow::Borrowed(ir), Vec::new()));
    }
    let failure = |body: &str, reason: String| {
        naming(body)(CodecError::NotImplemented(format!(
            "IGES writer places the transform of body {body} into its geometry, and {reason}"
        )))
    };
    let mut placed = ir.clone();
    let mut losses = Vec::new();
    let mut reduced = BTreeSet::new();

    let mut curve_maps = BTreeMap::new();
    for curve in &mut placed.geometry.curves {
        let Some(&(transform, body)) = placements.curves.get(curve.id.as_str()) else {
            continue;
        };
        let (geometry, map, construction) = placed_curve(&curve.geometry, transform)
            .map_err(|reason| failure(body, format!("curve {} {reason}", curve.id)))?;
        if let Some(range) = curve.parameter_range {
            curve.parameter_range = Some(
                IncreasingParameterInterval::new(map.interval(range.endpoints())).ok_or_else(
                    || failure(body, format!("curve {} has no placed range", curve.id)),
                )?,
            );
        }
        curve.geometry = geometry;
        if let Some(construction) = construction {
            losses.push(IgesLossCode::ProceduralReduced.note(format!(
                "IGES writer states procedural curve {construction} of body {body} by its \
                 placed solved form"
            )));
            reduced.insert(construction);
        }
        curve_maps.insert(curve.id.as_str().to_owned(), (map, body));
    }
    for point in &mut placed.geometry.points {
        let Some(&(transform, body)) = placements.points.get(point.id.as_str()) else {
            continue;
        };
        let position = transform
            .apply_point(point.position().get())
            .ok_or_else(|| failure(body, format!("point {} leaves the finite range", point.id)))?;
        point.set_position(position);
    }
    for edge in &mut placed.geometry.edges {
        let (Some(curve), Some(range)) = (edge.curve(), edge.param_range()) else {
            continue;
        };
        let Some(&(map, body)) = curve_maps.get(curve.as_str()) else {
            continue;
        };
        let range = ParameterInterval::new(map.interval(range.get()))
            .map_err(|_| failure(body, format!("edge {} has no placed range", edge.id)))?;
        edge.set_param_range(Some(range));
    }
    for coedge in &mut placed.geometry.coedges {
        let Some(use_curve) = &mut coedge.use_curve else {
            continue;
        };
        let Some(&(map, body)) = curve_maps.get(use_curve.curve.as_str()) else {
            continue;
        };
        use_curve.parameter_range =
            ParameterInterval::new(map.interval(use_curve.parameter_range.endpoints()))
                .map_err(|_| failure(body, format!("coedge {} has no placed range", coedge.id)))?;
    }

    let mut surface_maps = BTreeMap::new();
    for surface in &mut placed.geometry.surfaces {
        let Some(&(transform, body)) = placements.surfaces.get(surface.id.as_str()) else {
            continue;
        };
        let (geometry, map, construction) = placed_surface(&surface.geometry, transform)
            .map_err(|reason| failure(body, format!("surface {} {reason}", surface.id)))?;
        surface.geometry = geometry;
        if let Some(construction) = construction {
            losses.push(IgesLossCode::ProceduralReduced.note(format!(
                "IGES writer states procedural surface {construction} of body {body} by its \
                 placed solved form"
            )));
            reduced.insert(construction);
        }
        surface_maps.insert(surface.id.as_str().to_owned(), (map, transform, body));
    }
    placed
        .geometry
        .procedural_curves
        .retain(|procedural| !reduced.contains(procedural.id.as_str()));
    placed
        .geometry
        .procedural_surfaces
        .retain(|procedural| !reduced.contains(procedural.id.as_str()));

    // A reflection turns each loop against its face's placed normal, so the
    // writer reverses the loops of a reflected face.
    let mut reversed_loops = BTreeMap::new();
    let mut pcurve_maps = BTreeMap::<String, (UvMap, bool, &str)>::new();
    for face in &mut placed.geometry.faces {
        if !placements.faces.contains(face.id.as_str()) {
            continue;
        }
        let Some(&(map, transform, body)) = surface_maps.get(face.surface.as_str()) else {
            continue;
        };
        let reflected = Linear::of(transform).determinant < 0.0;
        if reflected != (map.determinant() < 0.0) {
            face.sense = opposite(face.sense);
        }
        for loop_ in face
            .loops
            .stated()
            .filter_map(|id| ir.geometry.loops.iter().find(|loop_| loop_.id == *id))
        {
            if reflected {
                reversed_loops.insert(loop_.id.as_str(), body);
            }
            let uses = match &loop_.boundary {
                LoopBoundary::Vertex { pcurves, .. } => pcurves
                    .iter()
                    .map(|use_| (use_.pcurve.as_str(), false))
                    .collect::<Vec<_>>(),
                LoopBoundary::Ring(ring) => ring
                    .coedges()
                    .iter()
                    .filter_map(|id| ir.geometry.coedges.iter().find(|coedge| coedge.id == *id))
                    .flat_map(|coedge| coedge.pcurves.iter())
                    .chain(
                        ring.vertex_uses()
                            .iter()
                            .flat_map(|use_| use_.pcurves.iter()),
                    )
                    .map(|use_| (use_.pcurve.as_str(), reflected))
                    .collect(),
            };
            for (pcurve, reversed) in uses {
                if let Some((other_map, other, _)) =
                    pcurve_maps.insert(pcurve.to_owned(), (map, reversed, body))
                {
                    if other != reversed || other_map != map {
                        return Err(failure(
                            body,
                            format!("pcurve {pcurve} has two placed images"),
                        ));
                    }
                }
            }
        }
    }
    for loop_ in &mut placed.geometry.loops {
        let Some(&body) = reversed_loops.get(loop_.id.as_str()) else {
            continue;
        };
        let LoopBoundary::Ring(ring) = &loop_.boundary else {
            continue;
        };
        if !ring.vertex_uses().is_empty() {
            return Err(failure(
                body,
                format!("reflected loop {} has pole vertex uses", loop_.id),
            ));
        }
        let coedges = ring.coedges().iter().rev().cloned().collect();
        loop_.boundary =
            LoopBoundary::Ring(LoopRing::new(coedges, Vec::new()).map_err(|error| {
                failure(
                    body,
                    format!("reflected loop {} is invalid: {error}", loop_.id),
                )
            })?);
    }
    for coedge in &mut placed.geometry.coedges {
        if let Some(&body) = reversed_loops.get(coedge.owner_loop.as_str()) {
            if coedge.use_curve.is_some()
                || coedge
                    .pcurves
                    .iter()
                    .any(|use_| use_.parameter_range.is_some())
            {
                return Err(failure(
                    body,
                    format!("reflected coedge {} states a directed use range", coedge.id),
                ));
            }
            coedge.sense = opposite(coedge.sense);
        }
        for use_ in &mut coedge.pcurves {
            if pcurve_maps
                .get(use_.pcurve.as_str())
                .is_some_and(|(map, _, _)| !map.is_diagonal())
            {
                use_.isoparametric = None;
            }
        }
    }
    for pcurve in &mut placed.geometry.pcurves {
        let Some(&(map, reversed, body)) = pcurve_maps.get(pcurve.id.as_str()) else {
            continue;
        };
        if map == UvMap::IDENTITY && !reversed {
            continue;
        }
        *pcurve = mapped_pcurve(pcurve, map, reversed)
            .map_err(|reason| failure(body, format!("pcurve {} {reason}", pcurve.id)))?;
    }
    for body in &mut placed.geometry.bodies {
        if placements.bodies.contains_key(body.id.as_str()) {
            body.transform = None;
        }
    }
    Ok((Cow::Owned(placed), losses))
}

const fn opposite(sense: Sense) -> Sense {
    match sense {
        Sense::Forward => Sense::Reversed,
        Sense::Reversed => Sense::Forward,
    }
}

/// `value`, or `target` where the two agree within the similarity bound.
fn snapped(value: f64, target: f64) -> f64 {
    if (value - target).abs() <= SIMILARITY_TOLERANCE * target.abs().max(1.0) {
        target
    } else {
        value
    }
}

/// The linear part of a transform, by the images of the unit axes.
struct Linear {
    columns: [Vector3; 3],
    determinant: f64,
    /// The scale of a similarity.
    similarity: Option<f64>,
}

impl Linear {
    fn of(transform: Transform) -> Self {
        let rows = transform.affine_rows();
        let columns =
            [0, 1, 2].map(|column| Vector3::new(rows[0][column], rows[1][column], rows[2][column]));
        let determinant = columns[0].dot(columns[1].cross(columns[2]));
        let squares = columns.map(|column| column.dot(column));
        let mean = (squares[0] + squares[1] + squares[2]) / 3.0;
        let bound = SIMILARITY_TOLERANCE * mean;
        let similar = mean > 0.0
            && squares.iter().all(|square| (square - mean).abs() <= bound)
            && columns[0].dot(columns[1]).abs() <= bound
            && columns[0].dot(columns[2]).abs() <= bound
            && columns[1].dot(columns[2]).abs() <= bound;
        Self {
            columns,
            determinant,
            similarity: similar.then(|| snapped(mean.sqrt(), 1.0)),
        }
    }

    fn apply(&self, vector: Vector3) -> Vector3 {
        self.columns[0].scale(vector.x)
            + self.columns[1].scale(vector.y)
            + self.columns[2].scale(vector.z)
    }
}

/// The placed curve parameter `scale t + shift` of curve parameter `t`.
#[derive(Clone, Copy)]
struct ParameterMap {
    scale: f64,
    shift: f64,
}

impl ParameterMap {
    const IDENTITY: Self = Self {
        scale: 1.0,
        shift: 0.0,
    };

    fn interval(self, [lower, upper]: [f64; 2]) -> [f64; 2] {
        [
            self.scale.mul_add(lower, self.shift),
            self.scale.mul_add(upper, self.shift),
        ]
    }
}

/// The placed surface parameters `(a u + b v, c u + d v)` of `(u, v)`.
#[derive(Clone, Copy, PartialEq)]
struct UvMap {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
}

impl UvMap {
    const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
    };

    const fn scaled(u: f64, v: f64) -> Self {
        Self {
            a: u,
            b: 0.0,
            c: 0.0,
            d: v,
        }
    }

    fn determinant(self) -> f64 {
        self.a.mul_add(self.d, -(self.b * self.c))
    }

    fn is_diagonal(self) -> bool {
        self.b == 0.0 && self.c == 0.0
    }

    fn apply(self, point: Point2) -> Point2 {
        Point2::new(
            self.a.mul_add(point.u, self.b * point.v),
            self.c.mul_add(point.u, self.d * point.v),
        )
    }
}

/// The unit axis along `axis` and the unit reference in its normal plane
/// nearest `reference`.
fn unit_frame(axis: Vector3, reference: Vector3) -> Result<(Vector3, Vector3), String> {
    let axis = UnitVector3::normalized(axis).ok_or("has a degenerate placed axis")?;
    let axis = *axis.as_raw();
    let reference = reference - axis.scale(reference.dot(axis));
    let reference =
        UnitVector3::normalized(reference).ok_or("has a degenerate placed reference")?;
    Ok((axis, *reference.as_raw()))
}

fn placed_point(transform: Transform, point: FinitePoint3) -> Result<Point3, String> {
    transform
        .apply_point(point.get())
        .map(FinitePoint3::get)
        .ok_or_else(|| "leaves the finite range".to_owned())
}

fn placed_curve(
    geometry: &CurveGeometry,
    transform: Transform,
) -> Result<(CurveGeometry, ParameterMap, Option<String>), String> {
    match geometry {
        CurveGeometry::Procedural {
            construction,
            cache,
        } => {
            let cache = cache
                .as_ref()
                .ok_or("is a procedural carrier with no solved form")?;
            let (solved, map) = placed_solved_curve(cache, transform)?;
            Ok((
                CurveGeometry::Solved(solved),
                map,
                Some(construction.as_str().to_owned()),
            ))
        }
        CurveGeometry::Solved(solved) => {
            let (solved, map) = placed_solved_curve(solved, transform)?;
            Ok((CurveGeometry::Solved(solved), map, None))
        }
    }
}

fn placed_solved_curve(
    geometry: &SolvedCurveGeometry,
    transform: Transform,
) -> Result<(SolvedCurveGeometry, ParameterMap), String> {
    let linear = Linear::of(transform);
    let invalid = |error: &str| format!("has no valid placed form: {error}");
    match geometry {
        SolvedCurveGeometry::Transformed(placed) => {
            let composed = transform
                .compose(*placed.transform())
                .map_err(|_| "composes to a non-finite transform".to_owned())?;
            placed_solved_curve(placed.basis(), composed)
        }
        SolvedCurveGeometry::Line(line) => {
            let origin = placed_point(transform, line.origin())?;
            let image = linear.apply(*line.direction().as_raw());
            let length = image.norm();
            let line = LineCurve::try_new(origin, image.scale(1.0 / length)).map_err(invalid)?;
            Ok((
                SolvedCurveGeometry::Line(line),
                ParameterMap {
                    scale: snapped(length, 1.0),
                    shift: 0.0,
                },
            ))
        }
        SolvedCurveGeometry::Circle(circle) => {
            let radius = circle.radius().get();
            elliptic(
                placed_point(transform, circle.center())?,
                linear.apply(*circle.frame().reference().as_raw()),
                linear.apply(*circle.frame().binormal().as_raw()),
                [radius, radius],
            )
        }
        SolvedCurveGeometry::Ellipse(ellipse) => elliptic(
            placed_point(transform, ellipse.center())?,
            linear.apply(*ellipse.frame().reference().as_raw()),
            linear.apply(*ellipse.frame().binormal().as_raw()),
            [ellipse.major_radius().get(), ellipse.minor_radius().get()],
        ),
        SolvedCurveGeometry::Hyperbola(hyperbola) => {
            let center = placed_point(transform, hyperbola.center())?;
            let radii = [
                hyperbola.major_radius().get(),
                hyperbola.minor_radius().get(),
            ];
            let major = linear
                .apply(*hyperbola.frame().reference().as_raw())
                .scale(radii[0]);
            let minor = linear
                .apply(*hyperbola.frame().binormal().as_raw())
                .scale(radii[1]);
            // `A cosh t + B sinh t` is `A' cosh(t - t0) + B' sinh(t - t0)`
            // with `A' ⊥ B'` where `tanh 2 t0 = -2 A·B / (|A|² + |B|²)`.
            let shift = if similar_in_plane(major, minor, radii) {
                0.0
            } else {
                0.5 * (-2.0 * major.dot(minor) / (major.dot(major) + minor.dot(minor))).atanh()
            };
            let (sinh, cosh) = (shift.sinh(), shift.cosh());
            let major_image = major.scale(cosh) + minor.scale(sinh);
            let minor_image = major.scale(sinh) + minor.scale(cosh);
            let (axis, reference) = unit_frame(major_image.cross(minor_image), major_image)?;
            let hyperbola = HyperbolaCurve::try_new(
                center,
                axis,
                reference,
                major_image.norm(),
                minor_image.norm(),
            )
            .map_err(invalid)?;
            Ok((
                SolvedCurveGeometry::Hyperbola(hyperbola),
                ParameterMap {
                    scale: 1.0,
                    shift: -shift,
                },
            ))
        }
        SolvedCurveGeometry::Parabola(parabola) => {
            let vertex = placed_point(transform, parabola.vertex())?;
            let focal = parabola.focal_distance().get();
            // `V + t² A + t B` with `A = f X`, `B = 2 f Y` restated as
            // `V' + f' τ² X' + 2 f' τ Y'` with `τ = α t + β`.
            let quadratic = linear
                .apply(*parabola.frame().reference().as_raw())
                .scale(focal);
            let linear_term = linear
                .apply(*parabola.frame().binormal().as_raw())
                .scale(2.0 * focal);
            let (axis, reference) = unit_frame(quadratic.cross(linear_term), quadratic)?;
            let binormal = axis.cross(reference);
            let along = linear_term.dot(reference);
            let across = linear_term.dot(binormal);
            let curvature = quadratic.norm();
            let (scale, shift) = if similar_in_plane(quadratic, linear_term, [1.0, 2.0]) {
                (1.0, 0.0)
            } else {
                (2.0 * curvature / across, along / across)
            };
            let focal = across * across / (4.0 * curvature);
            let offset =
                reference.scale(focal * shift * shift) + binormal.scale(2.0 * focal * shift);
            let vertex = Point3::new(
                vertex.x - offset.x,
                vertex.y - offset.y,
                vertex.z - offset.z,
            );
            let parabola =
                ParabolaCurve::try_new(vertex, axis, reference, focal).map_err(invalid)?;
            Ok((
                SolvedCurveGeometry::Parabola(parabola),
                ParameterMap { scale, shift },
            ))
        }
        SolvedCurveGeometry::Degenerate(degenerate) => Ok((
            SolvedCurveGeometry::Degenerate(
                DegenerateCurve::try_new(placed_point(transform, degenerate.point())?)
                    .map_err(invalid)?,
            ),
            ParameterMap::IDENTITY,
        )),
        SolvedCurveGeometry::Nurbs(nurbs) => {
            let mut nurbs = nurbs.clone();
            nurbs.try_map_control_points(|_, point| {
                transform
                    .apply_point(point.get())
                    .ok_or_else(|| "leaves the finite range".to_owned())
            })?;
            Ok((SolvedCurveGeometry::Nurbs(nurbs), ParameterMap::IDENTITY))
        }
        SolvedCurveGeometry::Polyline(polyline) => {
            let mut polyline = polyline.clone();
            polyline
                .edit_samples(|samples| {
                    samples.edit_points(|point| {
                        *point = transform
                            .apply_point(*point)
                            .ok_or_else(|| {
                                GeometryLayoutError::EditRefused(
                                    "a placed polyline point is not finite".into(),
                                )
                            })?
                            .get();
                        Ok(())
                    })
                })
                .map_err(|error| invalid(&error.to_string()))?;
            Ok((
                SolvedCurveGeometry::Polyline(polyline),
                ParameterMap::IDENTITY,
            ))
        }
        SolvedCurveGeometry::Composite { .. } => {
            Err("is a composite carrier, which the writer does not place".to_owned())
        }
        SolvedCurveGeometry::Unknown { .. } => {
            Err("has no exact form that the writer places".to_owned())
        }
    }
}

/// Whether the linear map takes the conjugate semi-axes `x`, `y` of radii
/// `radii` to perpendicular images in the same ratio, so that the placed
/// conic keeps its parameter.
fn similar_in_plane(x_image: Vector3, y_image: Vector3, radii: [f64; 2]) -> bool {
    let [x, y] = [x_image.norm(), y_image.norm()];
    x_image.dot(y_image).abs() <= SIMILARITY_TOLERANCE * x * y
        && (x * radii[1] - y * radii[0]).abs() <= SIMILARITY_TOLERANCE * x * radii[1]
}

/// The placed circle or ellipse `C + A cos t + B sin t` with `A`, `B` the
/// images of the scaled frame reference and binormal.
fn elliptic(
    center: Point3,
    reference_image: Vector3,
    binormal_image: Vector3,
    radii: [f64; 2],
) -> Result<(SolvedCurveGeometry, ParameterMap), String> {
    let invalid = |error: &str| format!("has no valid placed form: {error}");
    let major = reference_image.scale(radii[0]);
    let minor = binormal_image.scale(radii[1]);
    // `A cos t + B sin t` is `A' cos(t - t0) + B' sin(t - t0)` with
    // `A' ⊥ B'` and `|A'| ≥ |B'|` where
    // `tan 2 t0 = 2 A·B / (|A|² - |B|²)`.
    let shift = if similar_in_plane(reference_image, binormal_image, [1.0, 1.0]) {
        0.0
    } else {
        0.5 * (2.0 * major.dot(minor)).atan2(major.dot(major) - minor.dot(minor))
    };
    let (sin, cos) = shift.sin_cos();
    let major_image = major.scale(cos) + minor.scale(sin);
    let minor_image = minor.scale(cos) - major.scale(sin);
    let (axis, reference) = unit_frame(major_image.cross(minor_image), major_image)?;
    let major_radius = major_image.norm();
    let minor_radius = minor_image.norm().min(major_radius);
    let map = ParameterMap {
        scale: 1.0,
        shift: -shift,
    };
    if major_radius - minor_radius <= SIMILARITY_TOLERANCE * major_radius {
        let circle =
            CircleCurve::try_new(center, axis, reference, major_radius).map_err(invalid)?;
        return Ok((SolvedCurveGeometry::Circle(circle), map));
    }
    let ellipse = EllipseCurve::try_new(center, axis, reference, major_radius, minor_radius)
        .map_err(invalid)?;
    Ok((SolvedCurveGeometry::Ellipse(ellipse), map))
}

fn placed_surface(
    geometry: &SurfaceGeometry,
    transform: Transform,
) -> Result<(SurfaceGeometry, UvMap, Option<String>), String> {
    match geometry {
        SurfaceGeometry::Procedural {
            construction,
            cache,
        } => {
            let cache = cache
                .as_ref()
                .ok_or("is a procedural carrier with no solved form")?;
            let (solved, map) = placed_solved_surface(cache, transform)?;
            Ok((
                SurfaceGeometry::Solved(solved),
                map,
                Some(construction.as_str().to_owned()),
            ))
        }
        SurfaceGeometry::Solved(solved) => {
            let (solved, map) = placed_solved_surface(solved, transform)?;
            Ok((SurfaceGeometry::Solved(solved), map, None))
        }
    }
}

fn placed_solved_surface(
    geometry: &SolvedSurfaceGeometry,
    transform: Transform,
) -> Result<(SolvedSurfaceGeometry, UvMap), String> {
    let linear = Linear::of(transform);
    let invalid = |error: &str| format!("has no valid placed form: {error}");
    // A circular carrier keeps its parameters under a similarity of scale
    // `s`; a reflection turns the azimuth.
    let revolved = |frame: &cadmpeg_ir::units::OrthonormalFrame3| {
        let scale = linear.similarity.ok_or(
            "a cylinder, cone, sphere or torus under a transform that is not a similarity is \
             written only as the rational Type 128 that the writer restates from isoparametric \
             parameter curves of its faces",
        )?;
        let (axis, reference) = unit_frame(
            linear.apply(*frame.axis().as_raw()),
            linear.apply(*frame.reference().as_raw()),
        )?;
        let turn = if linear.determinant < 0.0 { -1.0 } else { 1.0 };
        Ok::<_, String>((axis, reference, scale, turn))
    };
    match geometry {
        SolvedSurfaceGeometry::Transformed(placed) => {
            let composed = transform
                .compose(*placed.transform())
                .map_err(|_| "composes to a non-finite transform".to_owned())?;
            placed_solved_surface(placed.basis(), composed)
        }
        SolvedSurfaceGeometry::Plane(plane) => {
            let origin = placed_point(transform, plane.origin())?;
            let u_image = linear.apply(*plane.frame().reference().as_raw());
            let v_image = linear.apply(*plane.frame().binormal().as_raw());
            let u_length = u_image.norm();
            let reference = u_image.scale(1.0 / u_length);
            let shear = v_image.dot(reference);
            let across = v_image - reference.scale(shear);
            let (axis, reference) = unit_frame(reference.cross(across), reference)?;
            let plane = PlaneSurface::try_new(origin, axis, reference).map_err(invalid)?;
            Ok((
                SolvedSurfaceGeometry::Plane(plane),
                UvMap {
                    a: snapped(u_length, 1.0),
                    b: snapped(shear, 0.0),
                    c: 0.0,
                    d: snapped(across.norm(), 1.0),
                },
            ))
        }
        SolvedSurfaceGeometry::Cylinder(cylinder) => {
            let (axis, reference, scale, turn) = revolved(cylinder.frame())?;
            let cylinder = CylinderSurface::try_new(
                placed_point(transform, cylinder.origin())?,
                axis,
                reference,
                cylinder.radius().get() * scale,
            )
            .map_err(invalid)?;
            Ok((
                SolvedSurfaceGeometry::Cylinder(cylinder),
                UvMap::scaled(turn, scale),
            ))
        }
        SolvedSurfaceGeometry::Cone(cone) => {
            let (axis, reference, scale, turn) = revolved(cone.frame())?;
            let cone = ConeSurface::try_new(
                placed_point(transform, cone.origin())?,
                axis,
                reference,
                cone.radius().get() * scale,
                cone.ratio().get(),
                cone.half_angle().get(),
            )
            .map_err(invalid)?;
            Ok((
                SolvedSurfaceGeometry::Cone(cone),
                UvMap::scaled(turn, scale),
            ))
        }
        SolvedSurfaceGeometry::Sphere(sphere) => {
            let (axis, reference, scale, turn) = revolved(sphere.frame())?;
            let sphere = SphereSurface::try_new(
                placed_point(transform, sphere.center())?,
                axis,
                reference,
                sphere.radius().get() * scale,
            )
            .map_err(invalid)?;
            Ok((
                SolvedSurfaceGeometry::Sphere(sphere),
                UvMap::scaled(turn, 1.0),
            ))
        }
        SolvedSurfaceGeometry::Torus(torus) => {
            let (axis, reference, scale, turn) = revolved(torus.frame())?;
            let torus = TorusSurface::try_new(
                placed_point(transform, torus.center())?,
                axis,
                reference,
                torus.major_radius().get() * scale,
                torus.minor_radius().get() * scale,
            )
            .map_err(invalid)?;
            Ok((
                SolvedSurfaceGeometry::Torus(torus),
                UvMap::scaled(turn, 1.0),
            ))
        }
        SolvedSurfaceGeometry::Nurbs(nurbs) => {
            let mut nurbs = nurbs.clone();
            nurbs.try_map_control_points(|_, point| {
                transform
                    .apply_point(point.get())
                    .ok_or_else(|| "leaves the finite range".to_owned())
            })?;
            Ok((SolvedSurfaceGeometry::Nurbs(nurbs), UvMap::IDENTITY))
        }
        SolvedSurfaceGeometry::Polygonal(_) | SolvedSurfaceGeometry::Unknown { .. } => {
            Err("has no exact form that the writer places".to_owned())
        }
    }
}

/// `pcurve` under the support parameter map `map`, reversed when its coedge
/// is.
fn mapped_pcurve(pcurve: &Pcurve, map: UvMap, reversed: bool) -> Result<Pcurve, String> {
    if pcurve.uv_offset().is_some() {
        return Err("states a support chart offset".to_owned());
    }
    let range = pcurve
        .parameter_range()
        .ok_or("states no parameter range")?;
    let mut nurbs = lifted_pcurve(pcurve, range).map_err(|error| error.to_string())?;
    nurbs.try_map_control_points(|_, point| {
        let point = point.get();
        let image = map.apply(Point2::new(point.x, point.y));
        FinitePoint3::new(Point3::new(image.u, image.v, 0.0))
            .ok_or_else(|| "has a non-finite placed control point".to_owned())
    })?;
    let (nurbs, range) = if reversed {
        reverse_nurbs(&nurbs, range).map_err(|error| error.to_string())?
    } else {
        (nurbs, range)
    };
    let points = nurbs
        .control_points()
        .into_iter()
        .map(|point| {
            let point = point.get();
            Point2::new(point.x, point.y)
        })
        .collect();
    let weights = nurbs.weights().map(|weights| {
        weights
            .into_iter()
            .map(cadmpeg_ir::scalar::NonZeroReal::get)
            .collect()
    });
    let knots = nurbs
        .knots()
        .finite_knots()
        .map(FiniteReal::get)
        .collect::<Vec<_>>();
    let nurbs = PcurveNurbs::from_lanes(nurbs.degree(), knots, points, weights, nurbs.periodic())
        .map_err(|error| error.to_string())?;
    Ok(Pcurve {
        id: pcurve.id.clone(),
        geometry: PcurveGeometry::Nurbs { nurbs },
        metadata: PcurveMetadata::general(None, Some(range), None),
        native_ref: pcurve.native_ref.clone(),
    })
}
