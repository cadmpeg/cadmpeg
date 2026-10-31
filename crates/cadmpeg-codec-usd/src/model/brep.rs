// SPDX-License-Identifier: Apache-2.0
//! The neutral topology and carriers of B-rep records: the B-reps of a
//! `BrepArray` prim (spec §17) and of the intrinsic primitives (spec §11.5).
//!
//! Lengths are scaled to millimetres and angles to radians. Each analytic
//! carrier keeps its record parameterization, so a parameter maps to the
//! IR parameter by a positive factor: the length scale, the angle scale, or
//! one.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{
    CircleCurve, ConeSurface, CylinderSurface, EllipseCurve, HyperbolaCurve, LineCurve,
    ParabolaCurve, PlaneSurface, SphereSurface, TorusSurface,
};
use cadmpeg_ir::geometry::nurbs::{
    NurbsCurve, NurbsPoleGrid, NurbsPoles3, NurbsSurface, NurbsSurfaceAxis,
};
use cadmpeg_ir::geometry::pcurve::{
    CirclePcurve, EllipsePcurve, Pcurve, PcurveGeometry, PcurveMetadata, PcurveNurbs,
};
use cadmpeg_ir::geometry::surface_payloads::{
    AxisRevolutionSurfaceConstruction, LinearSweepSurfaceConstruction,
};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, DirectedParameterRange, ProceduralSurface, ProceduralSurfaceDefinition,
    SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, EdgeId, FaceId, LoopId, PcurveId, SurfaceId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::topology::{
    BodyKind, Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing, PcurveUse,
    Point, Region, Sense, Shell, Vertex,
};

use crate::brep::{
    AnalyticSurface, Brep, Catalog, CurveKind, Frame, PcurveKind, Reference, SurfaceKind,
};
use crate::ids::brep as id;

/// The entities of one B-rep, in arena order.
#[derive(Debug, Default)]
pub(crate) struct Imported {
    pub(crate) points: Vec<Point>,
    pub(crate) vertices: Vec<Vertex>,
    pub(crate) curves: Vec<Curve>,
    pub(crate) surfaces: Vec<Surface>,
    pub(crate) constructions: Vec<(SurfaceId, ProceduralSurface)>,
    pub(crate) pcurves: Vec<Pcurve>,
    pub(crate) edges: Vec<Edge>,
    pub(crate) coedges: Vec<Coedge>,
    pub(crate) loops: Vec<Loop>,
    pub(crate) faces: Vec<Face>,
    pub(crate) shells: Vec<Shell>,
    pub(crate) regions: Vec<Region>,
    pub(crate) kind: Option<BodyKind>,
    /// The IR face of each `brep:face:id`.
    pub(crate) face_ids: BTreeMap<i64, FaceId>,
    /// The body, when its identity is the B-rep's CADIR identity.
    pub(crate) body: Option<cadmpeg_ir::ids::BodyId>,
}

/// The IR identity of the body and of each face, edge, and vertex record:
/// the record's CADIR identity when it is valid and not yet used, and
/// otherwise the identity of its position.
struct Ids {
    body: cadmpeg_ir::ids::BodyId,
    faces: Vec<FaceId>,
    edges: Vec<EdgeId>,
    vertices: Vec<cadmpeg_ir::ids::VertexId>,
}

impl Ids {
    fn new(brep: &Brep, key: &str, used: &mut BTreeSet<String>) -> Self {
        fn kept<T>(
            value: Option<&Option<String>>,
            used: &mut BTreeSet<String>,
            mint: impl Fn(String) -> Option<T>,
        ) -> Option<T> {
            let text = value.cloned().flatten()?;
            if used.contains(&text) {
                return None;
            }
            let id = mint(text.clone())?;
            used.insert(text);
            Some(id)
        }
        let identities = &brep.identities;
        Self {
            body: kept(Some(&identities.body), used, |text| {
                cadmpeg_ir::ids::BodyId::mint(text).ok()
            })
            .unwrap_or_else(|| crate::ids::body(key)),
            faces: (0..brep.faces.len())
                .map(|index| {
                    kept(identities.faces.get(index), used, |text| {
                        FaceId::mint(text).ok()
                    })
                    .unwrap_or_else(|| id::face(key, index))
                })
                .collect(),
            edges: (0..brep.edges.len())
                .map(|index| {
                    kept(identities.edges.get(index), used, |text| {
                        EdgeId::mint(text).ok()
                    })
                    .unwrap_or_else(|| id::edge(key, index))
                })
                .collect(),
            vertices: (0..brep.vertices.len())
                .map(|index| {
                    kept(identities.vertices.get(index), used, |text| {
                        cadmpeg_ir::ids::VertexId::mint(text).ok()
                    })
                    .unwrap_or_else(|| id::vertex(key, index))
                })
                .collect(),
        }
    }
}

/// Why a B-rep has no neutral form.
type Refusal = String;

fn vector(values: [f64; 3]) -> Vector3 {
    Vector3::new(values[0], values[1], values[2])
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(values: [f64; 3]) -> Option<[f64; 3]> {
    let length = dot(values, values).sqrt();
    (length.is_finite() && length > 0.0).then(|| values.map(|value| value / length))
}

/// A frame's origin, unit axis, and unit reference direction, with the
/// reference direction made orthogonal to the axis.
fn orthonormal(frame: &Frame, scale: f64) -> Result<(Point3, Vector3, Vector3), Refusal> {
    let axis = unit(frame.axis).ok_or("a frame axis is zero or not finite")?;
    let along = dot(frame.ref_direction, axis);
    let reference = unit([
        frame.ref_direction[0] - along * axis[0],
        frame.ref_direction[1] - along * axis[1],
        frame.ref_direction[2] - along * axis[2],
    ])
    .ok_or("a frame reference direction is parallel to its axis")?;
    let [x, y, z] = frame.position.map(|value| value * scale);
    Ok((Point3::new(x, y, z), vector(axis), vector(reference)))
}

/// The factor from a record curve parameter to its IR parameter.
fn curve_factor(brep: &Brep, curve: Reference, scale: f64) -> f64 {
    match curve {
        (Catalog::Analytic, index) => match brep.analytic_curves.get(index).map(|curve| curve.kind)
        {
            Some(CurveKind::Line) => scale,
            Some(CurveKind::Circle | CurveKind::Ellipse) => brep.angle_scale,
            Some(CurveKind::Parabola | CurveKind::Hyperbola) | None => 1.0,
        },
        (Catalog::Nurbs, _) => 1.0,
    }
}

/// Whether a curve is a circle or an ellipse, whose parameter has period
/// `2π`.
fn is_periodic_conic(brep: &Brep, curve: Reference) -> bool {
    matches!(curve, (Catalog::Analytic, index)
    if matches!(
        brep.analytic_curves.get(index).map(|curve| curve.kind),
        Some(CurveKind::Circle | CurveKind::Ellipse)
    ))
}

/// The factors from the `(u, v)` of a record surface to its IR
/// parameters.
fn chart(brep: &Brep, surface: Reference, scale: f64) -> [f64; 2] {
    let angle = brep.angle_scale;
    let profile = |record: &AnalyticSurface| {
        record
            .profile
            .map_or(1.0, |profile| curve_factor(brep, profile, scale))
    };
    match surface {
        (Catalog::Analytic, index) => match brep.analytic_surfaces.get(index) {
            Some(record) => match record.kind {
                SurfaceKind::Plane => [scale, scale],
                SurfaceKind::Cylinder | SurfaceKind::Cone => [angle, scale],
                SurfaceKind::Sphere | SurfaceKind::Torus => [angle, angle],
                SurfaceKind::Extrusion => [profile(record), scale],
                SurfaceKind::Revolution => [angle, profile(record)],
            },
            None => [1.0, 1.0],
        },
        (Catalog::Nurbs, _) => [1.0, 1.0],
    }
}

/// The weights of a rational record, or `None` when every weight is one.
fn rational(weights: &[f64]) -> Option<Vec<f64>> {
    weights
        .iter()
        .any(|weight| *weight != 1.0)
        .then(|| weights.to_vec())
}

fn finite_point(point: [f64; 3], scale: f64) -> Result<FinitePoint3, Refusal> {
    FinitePoint3::new(Point3::new(
        point[0] * scale,
        point[1] * scale,
        point[2] * scale,
    ))
    .ok_or_else(|| "a position is not finite".to_owned())
}

fn curve_geometry(
    brep: &Brep,
    curve: Reference,
    scale: f64,
) -> Result<SolvedCurveGeometry, Refusal> {
    match curve {
        (Catalog::Analytic, index) => {
            let record = &brep.analytic_curves[index];
            let (origin, axis, reference) = match record.kind {
                CurveKind::Line => {
                    let axis = unit(record.frame.axis).ok_or("a line direction is zero")?;
                    let [x, y, z] = record.frame.position.map(|value| value * scale);
                    (Point3::new(x, y, z), vector(axis), vector(axis))
                }
                _ => orthonormal(&record.frame, scale)?,
            };
            let param = |position: usize| record.params.get(position).copied().unwrap_or_default();
            let geometry = match record.kind {
                CurveKind::Line => LineCurve::try_new(origin, axis).map(SolvedCurveGeometry::Line),
                CurveKind::Circle => {
                    CircleCurve::try_new(origin, axis, reference, param(0) * scale)
                        .map(SolvedCurveGeometry::Circle)
                }
                CurveKind::Ellipse => EllipseCurve::try_new(
                    origin,
                    axis,
                    reference,
                    param(0) * scale,
                    param(1) * scale,
                )
                .map(SolvedCurveGeometry::Ellipse),
                CurveKind::Parabola => {
                    ParabolaCurve::try_new(origin, axis, reference, param(0) * scale)
                        .map(SolvedCurveGeometry::Parabola)
                }
                CurveKind::Hyperbola => HyperbolaCurve::try_new(
                    origin,
                    axis,
                    reference,
                    param(0) * scale,
                    param(1) * scale,
                )
                .map(SolvedCurveGeometry::Hyperbola),
            };
            geometry.map_err(|error| format!("analytic curve {index}: {error}"))
        }
        (Catalog::Nurbs, index) => {
            let record = &brep.nurbs_curves[index];
            let points = record
                .points
                .iter()
                .map(|point| finite_point(*point, scale).map(FinitePoint3::get))
                .collect::<Result<Vec<_>, _>>()?;
            let degree =
                u32::try_from(record.order - 1).map_err(|_| "a curve order exceeds u32")?;
            let build = |periodic| {
                NurbsPoles3::from_lanes(points.clone(), rational(&record.weights)).and_then(
                    |poles| NurbsCurve::new(degree, record.knots.clone(), poles, periodic),
                )
            };
            build(record.form == crate::brep::Form::Periodic)
                .or_else(|_| build(false))
                .map(SolvedCurveGeometry::Nurbs)
                .map_err(|error| format!("NURBS curve {index}: {error}"))
        }
    }
}

/// The surface geometry of an analytic or NURBS record. An extrusion or a
/// revolution returns its construction.
enum SurfaceShape {
    Solved(SolvedSurfaceGeometry),
    Constructed(Box<ProceduralSurfaceDefinition>),
}

fn surface_shape(
    brep: &Brep,
    surface: Reference,
    scale: f64,
    profile: impl Fn(Reference) -> Option<CurveId>,
) -> Result<SurfaceShape, Refusal> {
    let angle = brep.angle_scale;
    match surface {
        (Catalog::Analytic, index) => {
            let record = &brep.analytic_surfaces[index];
            let param = |position: usize| record.params.get(position).copied().unwrap_or_default();
            let solved = match record.kind {
                SurfaceKind::Extrusion => {
                    let directrix = record
                        .profile
                        .and_then(&profile)
                        .ok_or("an extrusion has no profile curve")?;
                    let axis = unit(record.frame.axis).ok_or("an extrusion direction is zero")?;
                    return LinearSweepSurfaceConstruction::try_new(directrix, vector(axis))
                        .map(|construction| {
                            SurfaceShape::Constructed(Box::new(
                                ProceduralSurfaceDefinition::LinearSweep(construction),
                            ))
                        })
                        .map_err(|error| format!("extrusion {index}: {error}"));
                }
                SurfaceKind::Revolution => {
                    let directrix = record
                        .profile
                        .and_then(&profile)
                        .ok_or("a revolution has no profile curve")?;
                    let axis = unit(record.frame.axis).ok_or("a revolution axis is zero")?;
                    let origin = finite_point(record.frame.position, scale)?;
                    let direction = cadmpeg_ir::units::UnitVector3::new(vector(axis))
                        .ok_or("a revolution axis is not a unit vector")?;
                    return Ok(SurfaceShape::Constructed(Box::new(
                        ProceduralSurfaceDefinition::AxisRevolution(
                            AxisRevolutionSurfaceConstruction::from_parts(
                                directrix, origin, direction,
                            ),
                        ),
                    )));
                }
                kind => {
                    let (origin, axis, reference) = orthonormal(&record.frame, scale)?;
                    match kind {
                        SurfaceKind::Plane => PlaneSurface::try_new(origin, axis, reference)
                            .map(SolvedSurfaceGeometry::Plane),
                        SurfaceKind::Cylinder => {
                            CylinderSurface::try_new(origin, axis, reference, param(0) * scale)
                                .map(SolvedSurfaceGeometry::Cylinder)
                        }
                        SurfaceKind::Cone => ConeSurface::try_new(
                            origin,
                            axis,
                            reference,
                            param(0) * scale,
                            1.0,
                            param(1) * angle,
                        )
                        .map(SolvedSurfaceGeometry::Cone),
                        SurfaceKind::Sphere => {
                            SphereSurface::try_new(origin, axis, reference, param(0) * scale)
                                .map(SolvedSurfaceGeometry::Sphere)
                        }
                        _ => TorusSurface::try_new(
                            origin,
                            axis,
                            reference,
                            param(0) * scale,
                            param(1) * scale,
                        )
                        .map(SolvedSurfaceGeometry::Torus),
                    }
                }
            };
            solved
                .map(SurfaceShape::Solved)
                .map_err(|error| format!("analytic surface {index}: {error}"))
        }
        (Catalog::Nurbs, index) => {
            let record = &brep.nurbs_surfaces[index];
            let mut rows = Vec::new();
            let mut weights = Vec::new();
            for i in 0..record.u_count {
                let mut row = Vec::new();
                let mut weight_row = Vec::new();
                for j in 0..record.v_count {
                    let at = i * record.v_count + j;
                    row.push(finite_point(record.points[at], scale)?.get());
                    weight_row.push(record.weights[at]);
                }
                rows.push(row);
                weights.push(weight_row);
            }
            let degree =
                |order: usize| u32::try_from(order - 1).map_err(|_| "an order exceeds u32");
            let (u_degree, v_degree) = (degree(record.u_order)?, degree(record.v_order)?);
            let build = |periodic: [bool; 2]| {
                let lanes = rational(&record.weights).map(|_| weights.clone());
                NurbsPoleGrid::from_lanes(rows.clone(), lanes).and_then(|poles| {
                    NurbsSurface::new(
                        NurbsSurfaceAxis::new(u_degree, record.u_knots.clone(), periodic[0]),
                        NurbsSurfaceAxis::new(v_degree, record.v_knots.clone(), periodic[1]),
                        poles,
                        false,
                    )
                })
            };
            let periodic =
                [record.u_form, record.v_form].map(|form| form == crate::brep::Form::Periodic);
            build(periodic)
                .or_else(|_| build([false, false]))
                .map(|surface| SurfaceShape::Solved(SolvedSurfaceGeometry::Nurbs(surface)))
                .map_err(|error| format!("NURBS surface {index}: {error}"))
        }
    }
}

fn pcurve_geometry(
    brep: &Brep,
    pcurve: Reference,
    chart: [f64; 2],
    scale: f64,
) -> Result<(PcurveGeometry, Option<DirectedParameterRange>), Refusal> {
    match pcurve {
        (Catalog::Analytic, index) => {
            let record = &brep.analytic_pcurves[index];
            let center = Point2::new(record.position[0] * scale, record.position[1] * scale);
            let length = (record.ref_direction[0].powi(2) + record.ref_direction[1].powi(2)).sqrt();
            if !(length.is_finite() && length > 0.0) {
                return Err(format!(
                    "analytic pcurve {index} has a zero reference direction"
                ));
            }
            let x = Point2::new(
                record.ref_direction[0] / length,
                record.ref_direction[1] / length,
            );
            let y = Point2::new(-x.v, x.u);
            let param = |position: usize| record.params.get(position).copied().unwrap_or_default();
            let geometry = match record.kind {
                PcurveKind::Circle => CirclePcurve::try_new(center, x, y, param(0) * scale)
                    .map(PcurveGeometry::Circle),
                PcurveKind::Ellipse => {
                    EllipsePcurve::try_new(center, x, y, param(0) * scale, param(1) * scale)
                        .map(PcurveGeometry::Ellipse)
                }
            }
            .map_err(|error| format!("analytic pcurve {index}: {error}"))?;
            let range =
                DirectedParameterRange::new(record.range.map(|value| value * brep.angle_scale))
                    .map_err(|_| format!("analytic pcurve {index} has an empty range"))?;
            Ok((geometry, Some(range)))
        }
        (Catalog::Nurbs, index) => {
            let record = &brep.nurbs_pcurves[index];
            let points = record
                .points
                .iter()
                .map(|[u, v]| Point2::new(u * chart[0], v * chart[1]))
                .collect::<Vec<_>>();
            let degree = u32::try_from(record.order - 1).map_err(|_| "an order exceeds u32")?;
            let domain = [
                *record
                    .knots
                    .get(record.order - 1)
                    .ok_or("a pcurve has too few knots")?,
                *record
                    .knots
                    .get(record.points.len())
                    .ok_or("a pcurve has too few knots")?,
            ];
            let range = DirectedParameterRange::new(domain)
                .map_err(|_| format!("NURBS pcurve {index} has an empty domain"))?;
            PcurveNurbs::from_lanes(
                degree,
                record.knots.clone(),
                points,
                rational(&record.weights),
                false,
            )
            .map(|nurbs| (PcurveGeometry::Nurbs { nurbs }, Some(range)))
            .map_err(|error| format!("NURBS pcurve {index}: {error}"))
        }
    }
}

fn catalog_name(catalog: Catalog) -> &'static str {
    catalog.token()
}

/// Imports the B-rep of definition `key`, its lengths multiplied by `scale`.
pub(super) fn import(
    brep: &Brep,
    key: &str,
    scale: f64,
    used: &mut BTreeSet<String>,
) -> Result<Imported, Refusal> {
    let ids = Ids::new(brep, key, used);
    let mut out = Imported {
        body: (ids.body != crate::ids::body(key)).then(|| ids.body.clone()),
        ..Imported::default()
    };

    // Ownership: each loop in one face, each oriented edge in one loop, each
    // face in one shell, each shell in at most one solid.
    let mut loop_face = vec![None; brep.loops.len()];
    for (face, record) in brep.faces.iter().enumerate() {
        for loop_ in &record.loops {
            if loop_face[*loop_].replace(face).is_some() {
                return Err(format!("loop {loop_} bounds more than one face"));
            }
        }
    }
    let mut oriented_loop = vec![None; brep.oriented_edges.len()];
    for (loop_, record) in brep.loops.iter().enumerate() {
        for oriented in &record.oriented_edges {
            if oriented_loop[*oriented].replace(loop_).is_some() {
                return Err(format!("oriented edge {oriented} is in more than one loop"));
            }
        }
    }
    let mut face_shell = vec![None; brep.faces.len()];
    for (shell, record) in brep.shells.iter().enumerate() {
        for face in &record.faces {
            if face_shell[*face].replace(shell).is_some() {
                return Err(format!("face {face} is in more than one shell"));
            }
        }
    }
    if let Some(face) = face_shell.iter().position(Option::is_none) {
        return Err(format!("face {face} is in no shell"));
    }
    let mut shell_region = vec![None; brep.shells.len()];
    let mut regions: Vec<Vec<usize>> = Vec::new();
    for solid in &brep.solids {
        let region = regions.len();
        let mut shells = vec![solid.outer];
        shells.extend(solid.voids.iter().copied());
        for shell in &shells {
            if shell_region[*shell].replace(region).is_some() {
                return Err(format!("shell {shell} bounds more than one solid"));
            }
        }
        regions.push(shells);
    }
    let solids = regions.len();
    for (shell, region) in shell_region.iter_mut().enumerate() {
        if region.is_none() {
            *region = Some(regions.len());
            regions.push(vec![shell]);
        }
    }
    out.kind = Some(match (solids, regions.len() - solids) {
        (0, _) => BodyKind::Sheet,
        (_, 0) => BodyKind::Solid,
        _ => BodyKind::General,
    });

    // Carriers that the topology reaches.
    let mut curve_refs: BTreeSet<(u8, usize)> = BTreeSet::new();
    let catalog_key = |catalog: Catalog| match catalog {
        Catalog::Analytic => 0_u8,
        Catalog::Nurbs => 1_u8,
    };
    let mut used_edges = BTreeSet::new();
    for oriented in &brep.oriented_edges {
        used_edges.insert(oriented.edge);
    }
    for edge in &used_edges {
        if let Some((catalog, index)) = brep.edges[*edge].curve {
            curve_refs.insert((catalog_key(catalog), index));
        }
    }
    let mut surface_refs = BTreeSet::new();
    for face in &brep.faces {
        surface_refs.insert(face.surface);
        if let (Catalog::Analytic, index) = face.surface {
            if let Some((catalog, curve)) = brep.analytic_surfaces[index].profile {
                curve_refs.insert((catalog_key(catalog), curve));
            }
        }
    }
    let mut curve_ids: BTreeMap<(u8, usize), CurveId> = BTreeMap::new();
    for (catalog, index) in curve_refs {
        let reference = (
            if catalog == 0 {
                Catalog::Analytic
            } else {
                Catalog::Nurbs
            },
            index,
        );
        let curve_id = id::curve(key, catalog_name(reference.0), index);
        let geometry = curve_geometry(brep, reference, scale)?;
        out.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Solved(geometry),
            parameter_range: None,
            source_object: None,
        });
        curve_ids.insert((catalog, index), curve_id);
    }
    let mut surface_ids: BTreeMap<(u8, usize), SurfaceId> = BTreeMap::new();
    for reference in surface_refs {
        let surface_id = id::surface(key, catalog_name(reference.0), reference.1);
        let shape = surface_shape(brep, reference, scale, |profile| {
            curve_ids.get(&(catalog_key(profile.0), profile.1)).cloned()
        })?;
        match shape {
            SurfaceShape::Solved(geometry) => out.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Solved(geometry),
                source_object: None,
            }),
            SurfaceShape::Constructed(definition) => {
                out.surfaces.push(Surface {
                    id: surface_id.clone(),
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                        record: None,
                    }),
                    source_object: None,
                });
                out.constructions.push((
                    surface_id.clone(),
                    ProceduralSurface::new(id::construction(key, reference.1), *definition, None),
                ));
            }
        }
        surface_ids.insert((catalog_key(reference.0), reference.1), surface_id);
    }

    // Vertices that edges or vertex loops reach.
    let mut used_vertices = BTreeSet::new();
    for edge in &used_edges {
        used_vertices.extend(brep.edges[*edge].vertices);
    }
    for loop_ in &brep.loops {
        used_vertices.extend(loop_.vertex);
    }
    for vertex in &used_vertices {
        let point = id::point(key, *vertex);
        out.points.push(Point::new(
            point.clone(),
            finite_point(brep.vertices[*vertex], scale)?,
            None,
        ));
        out.vertices.push(Vertex {
            id: ids.vertices[*vertex].clone(),
            point,
            tolerance: None,
        });
    }

    // Edges. The IR edge runs from its start to its end in the direction of
    // increasing curve parameter.
    let edge_ids: BTreeMap<usize, EdgeId> = used_edges
        .iter()
        .map(|edge| (*edge, ids.edges[*edge].clone()))
        .collect();
    for (edge, edge_id) in &edge_ids {
        let record = &brep.edges[*edge];
        let [first, second] = record.vertices;
        let (start, end) = if record.same_sense {
            (first, second)
        } else {
            (second, first)
        };
        let carrier = match record.curve {
            Some(curve) => {
                let factor = curve_factor(brep, curve, scale);
                let mut range = record.range.map(|value| value * factor);
                if is_periodic_conic(brep, curve) {
                    // The IR anchors a conic range in one turn; whole turns
                    // name the same points.
                    let turns = (range[0] / std::f64::consts::TAU).floor();
                    range = range.map(|value| value - turns * std::f64::consts::TAU);
                }
                if range[0].partial_cmp(&range[1]) != Some(std::cmp::Ordering::Less) {
                    return Err(format!(
                        "edge {edge} has the range ({}, {})",
                        record.range[0], record.range[1]
                    ));
                }
                EdgeCarrier::new(
                    curve_ids.get(&(catalog_key(curve.0), curve.1)).cloned(),
                    Some(range),
                )
                .map_err(|error| format!("edge {edge}: {error}"))?
            }
            None => EdgeCarrier::unbounded(None),
        };
        let tolerance = record
            .tolerance
            .filter(|tolerance| *tolerance > brep.tolerance)
            .and_then(|tolerance| PositiveReal::new(tolerance * scale));
        out.edges.push(Edge {
            id: edge_id.clone(),
            carrier,
            start: ids.vertices[start].clone(),
            end: ids.vertices[end].clone(),
            tolerance,
        });
    }

    // Loops and their coedges.
    let mut radial: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (loop_, record) in brep.loops.iter().enumerate() {
        let Some(face) = loop_face[loop_] else {
            continue;
        };
        let loop_id = id::loop_(key, loop_);
        let face_id = ids.faces[face].clone();
        if record.oriented_edges.is_empty() {
            if let Some(vertex) = record.vertex {
                out.loops.push(Loop {
                    id: loop_id,
                    face: face_id,
                    boundary: LoopBoundary::Vertex {
                        vertex: ids.vertices[vertex].clone(),
                        pcurves: Vec::new(),
                    },
                });
            }
            continue;
        }
        let face_record = &brep.faces[face];
        let surface_chart = chart(brep, face_record.surface, scale);
        let mut coedge_ids = Vec::new();
        for oriented in &record.oriented_edges {
            let use_ = &brep.oriented_edges[*oriented];
            let edge = &brep.edges[use_.edge];
            let coedge_id = id::coedge(key, *oriented);
            let mut pcurves = Vec::new();
            if let Some(pcurve) = use_.pcurve {
                let (geometry, parameter_range) =
                    pcurve_geometry(brep, pcurve, surface_chart, scale)?;
                let pcurve_id: PcurveId = id::pcurve(key, catalog_name(pcurve.0), *oriented);
                out.pcurves.push(Pcurve {
                    id: pcurve_id.clone(),
                    geometry,
                    metadata: PcurveMetadata::General {
                        form: cadmpeg_ir::geometry::pcurve::PcurveGeneralForm::default(),
                    },
                });
                pcurves.push(PcurveUse {
                    pcurve: pcurve_id,
                    isoparametric: None,
                    parameter_range,
                });
            }
            radial.entry(use_.edge).or_default().push(out.coedges.len());
            out.coedges.push(Coedge {
                id: coedge_id.clone(),
                owner_loop: loop_id.clone(),
                edge: ids.edges[use_.edge].clone(),
                radial_next: coedge_id.clone(),
                sense: if edge.same_sense == use_.orientation {
                    Sense::Forward
                } else {
                    Sense::Reversed
                },
                pcurves,
                use_curve: None,
            });
            coedge_ids.push(coedge_id);
        }
        let ring = LoopRing::new(coedge_ids, Vec::new())
            .map_err(|error| format!("loop {loop_}: {error}"))?;
        out.loops.push(Loop {
            id: loop_id,
            face: face_id,
            boundary: LoopBoundary::Ring(ring),
        });
    }
    for members in radial.values() {
        for (position, member) in members.iter().enumerate() {
            let next = out.coedges[members[(position + 1) % members.len()]]
                .id
                .clone();
            out.coedges[*member].radial_next = next;
        }
    }

    // Faces, shells, regions.
    let written_loops: BTreeSet<LoopId> = out.loops.iter().map(|loop_| loop_.id.clone()).collect();
    for (face, record) in brep.faces.iter().enumerate() {
        let face_id = ids.faces[face].clone();
        let loops: Vec<LoopId> = record
            .loops
            .iter()
            .map(|loop_| id::loop_(key, *loop_))
            .filter(|loop_| written_loops.contains(loop_))
            .collect();
        let loops = match loops.split_first() {
            Some((outer, inner)) => FaceLoops::classified(outer.clone(), inner.to_vec()),
            None => FaceLoops::unspecified(Vec::new()),
        };
        let shell = face_shell[face].unwrap_or_default();
        out.faces.push(Face {
            id: face_id.clone(),
            shell: id::shell(key, shell),
            surface: surface_ids[&(catalog_key(record.surface.0), record.surface.1)].clone(),
            sense: if record.same_sense {
                Sense::Forward
            } else {
                Sense::Reversed
            },
            loops,
            name: None,
            color: None,
            tolerance: None,
        });
        out.face_ids.insert(record.id, face_id);
    }
    for (shell, record) in brep.shells.iter().enumerate() {
        let region = shell_region[shell].unwrap_or_default();
        let faces: Vec<FaceId> = record
            .faces
            .iter()
            .map(|face| ids.faces[*face].clone())
            .collect();
        let shell_record = Shell::new(
            id::shell(key, shell),
            id::region(key, region),
            faces,
            Vec::new(),
            Vec::new(),
        )
        .map_err(|_| format!("shell {shell} holds no face"))?;
        out.shells.push(shell_record);
    }
    for (region, shells) in regions.iter().enumerate() {
        out.regions.push(Region {
            id: id::region(key, region),
            body: ids.body.clone(),
            shells: shells.iter().map(|shell| id::shell(key, *shell)).collect(),
        });
    }
    Ok(out)
}

/// The B-reps of the `BrepArray` prim `prim`.
pub(super) fn decoded(
    prim: &crate::stage::Prim<'_>,
    issues: &mut crate::schema::Issues,
) -> Result<crate::brep::solid::Decoded, String> {
    let packed = crate::brep::solid::read(prim, issues)?;
    crate::brep::solid::decode(&packed)
}

/// Whether `prim` is a fallback of a sibling `BrepArray` prim whose B-reps
/// decode.
pub(super) fn is_fallback(
    stage: &crate::stage::Stage<'_>,
    prim: &crate::stage::Prim<'_>,
    issues: &mut crate::schema::Issues,
) -> bool {
    let Some(name) = prim.custom_data_text(crate::brep::solid::FALLBACK_KEY) else {
        return false;
    };
    let Some((parent, _)) = prim.path.rsplit_once('/') else {
        return false;
    };
    let sibling = format!("{}/{name}", if parent.is_empty() { "" } else { parent });
    stage.find(&sibling).is_some_and(|sibling| {
        sibling.type_name() == Some(crate::brep::solid::PRIM_TYPE)
            && decoded(sibling, issues).is_ok_and(|decoded| !decoded.breps.is_empty())
    })
}

/// The `BrepArray` child of a group prim whose other children are all its
/// fallbacks (spec §17.5), when its B-reps decode.
pub(super) fn folded(
    stage: &crate::stage::Stage<'_>,
    prim: &crate::stage::Prim<'_>,
    issues: &mut crate::schema::Issues,
) -> Option<usize> {
    if !matches!(prim.type_name(), None | Some("Xform")) {
        return None;
    }
    let mut source = None;
    for child in &prim.children {
        let child_prim = stage.prims.get(*child)?;
        if child_prim.type_name() == Some(crate::brep::solid::PRIM_TYPE) && source.is_none() {
            source = Some(*child);
        }
    }
    let source = source?;
    let name = &stage.prims.get(source)?.name;
    let all_fallbacks = prim.children.iter().all(|child| {
        *child == source
            || stage.prims.get(*child).is_some_and(|child| {
                child.custom_data_text(crate::brep::solid::FALLBACK_KEY) == Some(name.as_str())
            })
    });
    (all_fallbacks
        && decoded(stage.prims.get(source)?, issues).is_ok_and(|decoded| !decoded.breps.is_empty()))
    .then_some(source)
}

impl super::Builder<'_, '_, '_, '_> {
    /// Adds a body for each B-rep of the `BrepArray` prim `prim`, and the
    /// fallback meshes of `fallbacks` as tessellations of its first body.
    pub(super) fn brep_array(
        &mut self,
        prim: &crate::stage::Prim<'_>,
        fallbacks: &[usize],
        key: &str,
    ) -> Result<Vec<cadmpeg_ir::ids::BodyId>, cadmpeg_core::CodecError> {
        let decoded = match decoded(prim, &mut self.issues) {
            Ok(decoded) => decoded,
            Err(reason) => {
                self.tallies
                    .rejected_breps
                    .push(format!("{}: {reason}", prim.path));
                return Ok(Vec::new());
            }
        };
        if decoded.untransferred > 0 {
            *self
                .tallies
                .brep_elements
                .entry(prim.path.clone())
                .or_default() += decoded.untransferred;
        }
        let mut bodies = Vec::new();
        let mut keys: BTreeMap<usize, String> = BTreeMap::new();
        for (ordinal, records) in decoded.breps.iter().enumerate() {
            let body_key = if ordinal == 0 {
                key.to_owned()
            } else {
                format!("{key}#brep{ordinal}")
            };
            match self.brep_body(prim, &body_key, records)? {
                Ok(body) => {
                    self.body_material(prim, &body_key, &body)?;
                    bodies.push(body);
                    keys.insert(ordinal, body_key);
                }
                Err(reason) => self
                    .tallies
                    .rejected_breps
                    .push(format!("{}: {reason}", prim.path)),
            }
        }
        self.brep_subsets(prim, key, &decoded.faces, &keys)?;
        if let Some(body) = bodies.first() {
            for (ordinal, fallback) in fallbacks.iter().enumerate() {
                let Some(fallback) = self.stage.prims.get(*fallback) else {
                    continue;
                };
                if fallback.type_name() == Some("Mesh") {
                    self.fallback_mesh(fallback, key, ordinal, body)?;
                }
                if fallback.api_schemas().contains("PhysicsCollisionAPI") {
                    self.collider(fallback, key, true)?;
                }
            }
        }
        Ok(bodies)
    }

    /// Binds the materials of the face and B-rep subsets of the `BrepArray`
    /// prim `prim` (spec §17.4). `faces` gives the array face of each record
    /// face of each B-rep, and `keys` the body key of each imported B-rep.
    fn brep_subsets(
        &mut self,
        prim: &crate::stage::Prim<'_>,
        key: &str,
        faces: &[Vec<usize>],
        keys: &BTreeMap<usize, String>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let stage = self.stage;
        for child in prim
            .children
            .iter()
            .filter_map(|child| stage.prims.get(*child))
        {
            if child.type_name() != Some("GeomSubset") {
                continue;
            }
            let element = crate::schema::token(child, "elementType", &mut self.issues);
            let indices =
                crate::schema::integers(child, "indices", &mut self.issues).unwrap_or_default();
            let bound = child
                .relationship_targets("material:binding")
                .first()
                .map(|target| crate::schema::shading::material(stage, target, &mut self.issues));
            let targets: Option<Vec<cadmpeg_ir::appearance::AppearanceTarget>> =
                match element.as_deref() {
                    Some("face") => indices
                        .iter()
                        .map(|index| {
                            let index = usize::try_from(*index).ok()?;
                            faces.iter().enumerate().find_map(|(brep, list)| {
                                let record = list.iter().position(|face| *face == index)?;
                                let body_key = keys.get(&brep)?;
                                Some(cadmpeg_ir::appearance::AppearanceTarget::Face(
                                    crate::ids::brep::face(body_key, record),
                                ))
                            })
                        })
                        .collect(),
                    Some("brep") => indices
                        .iter()
                        .map(|index| {
                            let body_key = keys.get(&usize::try_from(*index).ok()?)?;
                            Some(cadmpeg_ir::appearance::AppearanceTarget::Body(
                                self.body_id(body_key),
                            ))
                        })
                        .collect(),
                    _ => None,
                };
            let (Some(targets), Some(crate::schema::shading::Bound::Surface(surface))) =
                (targets, bound)
            else {
                *self
                    .tallies
                    .brep_subsets
                    .entry(prim.path.clone())
                    .or_default() += 1;
                continue;
            };
            let appearance = self.appearance(&surface)?;
            for (ordinal, target) in targets.into_iter().enumerate() {
                self.ir
                    .model
                    .appearance_bindings
                    .push(cadmpeg_ir::appearance::AppearanceBinding {
                        id: crate::ids::subset_binding(key, &format!("{}#{ordinal}", child.path)),
                        target,
                        appearance: appearance.clone(),
                        source_entity_id: Some(surface.material.clone()),
                        object_type: None,
                        visible: None,
                        channels: BTreeMap::new(),
                    });
                self.admit("usd_appearance")?;
            }
        }
        Ok(())
    }

    /// Binds the material of `prim` to `body`.
    fn body_material(
        &mut self,
        prim: &crate::stage::Prim<'_>,
        key: &str,
        body: &cadmpeg_ir::ids::BodyId,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let Some(crate::schema::shading::Bound::Surface(surface)) =
            crate::schema::shading::bound_material(self.stage, prim, &mut self.issues)
        else {
            return Ok(());
        };
        let appearance = self.appearance(&surface)?;
        self.ir
            .model
            .appearance_bindings
            .push(cadmpeg_ir::appearance::AppearanceBinding {
                id: crate::ids::appearance_binding(key),
                target: cadmpeg_ir::appearance::AppearanceTarget::Body(body.clone()),
                appearance,
                source_entity_id: Some(surface.material),
                object_type: None,
                visible: None,
                channels: BTreeMap::new(),
            });
        self.admit("usd_appearance")
    }

    /// Adds fallback mesh `ordinal` of definition `key` as a tessellation of
    /// `body`.
    fn fallback_mesh(
        &mut self,
        prim: &crate::stage::Prim<'_>,
        key: &str,
        ordinal: usize,
        body: &cadmpeg_ir::ids::BodyId,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let Some(mesh) = crate::schema::mesh::read(self.stage, prim, &mut self.issues) else {
            self.tallies.rejected_meshes.push(prim.path.clone());
            return Ok(());
        };
        self.ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(
                mesh.points.len() * super::MESH_VERTEX_BYTES
                    + mesh.triangles.len() * super::MESH_TRIANGLE_BYTES,
            ),
            super::RETAIN_MESH,
        )?;
        let tessellation_key = if ordinal == 0 {
            key.to_owned()
        } else {
            format!("{key}/{}", prim.name)
        };
        let tessellation = match self.tessellation(&tessellation_key, &mesh) {
            Ok(tessellation) => tessellation,
            Err(reason) => {
                self.tallies
                    .rejected_meshes
                    .push(format!("{}: {reason}", prim.path));
                return Ok(());
            }
        };
        self.ir
            .model
            .tessellations
            .push(tessellation.with_body(Some(body.clone())));
        self.admit("usd_mesh")
    }

    /// Adds the body of B-rep records `records`, which `prim` defines, or
    /// gives the reason that the records have no neutral form.
    pub(super) fn brep_body(
        &mut self,
        prim: &crate::stage::Prim<'_>,
        key: &str,
        records: &crate::brep::Brep,
    ) -> Result<Result<cadmpeg_ir::ids::BodyId, String>, cadmpeg_core::CodecError> {
        let imported = match import(records, key, self.scale, &mut self.used_ids) {
            Ok(imported) => imported,
            Err(reason) => return Ok(Err(reason)),
        };
        let Imported {
            points,
            vertices,
            curves,
            surfaces,
            constructions,
            pcurves,
            edges,
            coedges,
            loops,
            faces,
            shells,
            regions,
            kind,
            face_ids: _,
            body,
        } = imported;
        let model = &mut self.ir.model;
        let region_ids = regions.iter().map(|region| region.id.clone()).collect();
        model.points.extend(points);
        model.vertices.extend(vertices);
        model.curves.extend(curves);
        model.surfaces.extend(surfaces);
        model.pcurves.extend(pcurves);
        model.edges.extend(edges);
        model.coedges.extend(coedges);
        model.loops.extend(loops);
        model.faces.extend(faces);
        model.shells.extend(shells);
        model.regions.extend(regions);
        if let Err(error) = model.add_procedural_surfaces(constructions)? {
            return Ok(Err(error.to_string()));
        }
        let tolerance = records.tolerance * self.scale;
        if tolerance > self.ir.tolerances.linear.get() {
            if let Some(tolerance) = cadmpeg_ir::scalar::PositiveLength::new(tolerance) {
                self.ir.tolerances.linear = tolerance;
            }
        }
        if let Some(body) = &body {
            self.body_ids.insert(key.to_owned(), body.clone());
        }
        let body = self.body_id(key);
        self.body(prim, key, kind.unwrap_or(BodyKind::General), region_ids);
        self.transferred = true;
        self.admit("usd_brep")?;
        Ok(Ok(body))
    }
}
