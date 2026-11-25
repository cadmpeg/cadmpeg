// SPDX-License-Identifier: Apache-2.0
//! The carriers of a Brep scope in the forms a 3DM Brep holds.
//!
//! Each edge curve becomes a line with a unit direction or a clamped NURBS
//! curve over the edge's range, each face surface a plane or a clamped NURBS
//! patch, and each pcurve a line or a clamped NURBS curve over its own
//! interval in its face's surface parameters. Every conversion is exact
//! (see [`super::exact`]). A face whose carriers have no such form is left
//! out with the reason; its records go with it.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::pcurve::{Pcurve, PcurveGeometry, PcurveMetadata};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Coedge, Edge};

use super::exact::{self, ParameterMap, Spline};
use super::EPS_WRITE_DEGENERATE;

/// A Brep scope with its carriers in 3DM form, and each face the writer
/// leaves out with the reason.
pub(super) struct ConvertedScope {
    pub(super) ir: CadIr,
    pub(super) dropped: Vec<(String, String)>,
}

/// The solved form of `geometry`: its solved carrier, or the cache of its
/// procedural record.
fn solved_curve(geometry: &CurveGeometry) -> Option<&SolvedCurveGeometry> {
    match geometry {
        CurveGeometry::Solved(solved) => Some(solved),
        CurveGeometry::Procedural { cache, .. } => cache.as_ref(),
    }
}

fn solved_surface(geometry: &SurfaceGeometry) -> Option<&SolvedSurfaceGeometry> {
    match geometry {
        SurfaceGeometry::Solved(solved) => Some(solved),
        SurfaceGeometry::Procedural { cache, .. } => cache.as_ref(),
    }
}

fn knot_domain(curve: &NurbsCurve) -> Option<[f64; 2]> {
    let degree = usize::try_from(curve.degree()).ok()?;
    Some([
        *curve.knots().get(degree)?,
        *curve.knots().get(curve.pole_count())?,
    ])
}

/// The NURBS curve that equals `curve` over `range`.
pub(super) fn curve_nurbs(
    curve: &SolvedCurveGeometry,
    range: [f64; 2],
) -> Result<NurbsCurve, String> {
    let [lo, hi] = range;
    let frame_vectors = |frame: &cadmpeg_ir::units::OrthonormalFrame3| {
        (*frame.reference().as_raw(), *frame.binormal().as_raw())
    };
    let scaled = |vector: Vector3, scale: f64| {
        Vector3::new(vector.x * scale, vector.y * scale, vector.z * scale)
    };
    match curve {
        SolvedCurveGeometry::Line(line) => {
            let origin = line.origin().get();
            let direction = *line.direction().as_raw();
            let at = |parameter: f64| {
                Point3::new(
                    origin.x + direction.x * parameter,
                    origin.y + direction.y * parameter,
                    origin.z + direction.z * parameter,
                )
            };
            exact::segment_curve(at(lo), at(hi), range)
        }
        SolvedCurveGeometry::Nurbs(nurbs) => exact::clamped_curve(nurbs, range),
        SolvedCurveGeometry::Circle(circle) => {
            let (r, s) = frame_vectors(circle.frame());
            let radius = circle.radius().get();
            exact::trigonometric_curve(
                circle.center().get(),
                scaled(r, radius),
                scaled(s, radius),
                false,
                range,
            )
            .map(|(curve, _)| curve)
        }
        SolvedCurveGeometry::Ellipse(ellipse) => {
            let (r, s) = frame_vectors(ellipse.frame());
            exact::trigonometric_curve(
                ellipse.center().get(),
                scaled(r, ellipse.major_radius().get()),
                scaled(s, ellipse.minor_radius().get()),
                false,
                range,
            )
            .map(|(curve, _)| curve)
        }
        SolvedCurveGeometry::Hyperbola(hyperbola) => {
            let (r, s) = frame_vectors(hyperbola.frame());
            exact::trigonometric_curve(
                hyperbola.center().get(),
                scaled(r, hyperbola.major_radius().get()),
                scaled(s, hyperbola.minor_radius().get()),
                true,
                range,
            )
            .map(|(curve, _)| curve)
        }
        SolvedCurveGeometry::Parabola(parabola) => {
            let (r, s) = frame_vectors(parabola.frame());
            let focal = parabola.focal_distance().get();
            exact::parabolic_curve(
                parabola.vertex().get(),
                scaled(r, focal),
                scaled(s, 2.0 * focal),
                range,
            )
        }
        SolvedCurveGeometry::Transformed(placed) => {
            let mut nurbs = curve_nurbs(placed.basis(), range)?;
            let transform = *placed.transform();
            nurbs
                .try_map_control_points(|_, pole| {
                    transform
                        .apply_point(pole.get())
                        .ok_or_else(|| "the placed curve leaves the finite range".to_owned())
                })
                .map_err(|error| error.clone())?;
            Ok(nurbs)
        }
        _ => Err("its curve kind has no NURBS form the writer builds".into()),
    }
}

/// The edge curve in 3DM form where it needs another one, `None` where the
/// stated solved carrier is written as it is.
fn edge_curve(
    edge: &Edge,
    curve: &cadmpeg_ir::geometry::Curve,
) -> Result<Option<SolvedCurveGeometry>, String> {
    let range = edge
        .param_range()
        .ok_or("the edge states no parameter range")?
        .get();
    if range[0] == range[1] {
        return Err("the edge has a zero-length parameter range".into());
    }
    let solved = solved_curve(&curve.geometry)
        .ok_or_else(|| format!("curve {} has no solved form", curve.id.as_str()))?;
    let procedural = !matches!(curve.geometry, CurveGeometry::Solved(_));
    let writable = match solved {
        SolvedCurveGeometry::Line(line) => {
            (line.direction().as_raw().norm() - 1.0).abs() <= EPS_WRITE_DEGENERATE
        }
        SolvedCurveGeometry::Nurbs(nurbs) => !nurbs.periodic() && knot_domain(nurbs) == Some(range),
        _ => false,
    };
    if writable {
        return Ok(procedural.then(|| solved.clone()));
    }
    curve_nurbs(solved, range)
        .map(|nurbs| Some(SolvedCurveGeometry::Nurbs(nurbs)))
        .map_err(|reason| format!("curve {}: {reason}", curve.id.as_str()))
}

/// The pcurve uses of `coedge` that lie on `surface`: each use whose pcurve
/// states no other support surface.
fn face_uses<'a>(
    coedge: &'a Coedge,
    surface: &str,
    pcurves: &HashMap<&str, &'a Pcurve>,
) -> Vec<(&'a cadmpeg_ir::topology::PcurveUse, &'a Pcurve)> {
    coedge
        .pcurves
        .iter()
        .filter_map(|use_| Some((use_, *pcurves.get(use_.pcurve.as_str())?)))
        .filter(|(_, pcurve)| match &pcurve.metadata {
            PcurveMetadata::General { form } => form
                .support_surface
                .as_ref()
                .is_none_or(|support| support.as_str() == surface),
            PcurveMetadata::AsmInline { .. } => true,
        })
        .collect()
}

/// The interval of `pcurve` that `use_` runs over, in the coedge's direction.
fn use_interval(
    use_: &cadmpeg_ir::topology::PcurveUse,
    pcurve: &Pcurve,
    edge_range: Option<[f64; 2]>,
) -> Result<[f64; 2], String> {
    if let Some(range) = use_.parameter_range {
        return Ok(range.endpoints());
    }
    if let Some(range) = pcurve.parameter_range() {
        return Ok(range.get());
    }
    match &pcurve.geometry {
        PcurveGeometry::Nurbs { nurbs } => {
            let degree = usize::try_from(nurbs.degree()).map_err(|_| "the degree overflows")?;
            let knots = nurbs.knots();
            match (knots.get(degree), knots.get(nurbs.pole_rows().count())) {
                (Some(lo), Some(hi)) => Ok([*lo, *hi]),
                _ => Err("the pcurve has no knot domain".into()),
            }
        }
        _ => edge_range.ok_or_else(|| "the pcurve and its edge state no interval".into()),
    }
}

/// Whether `pcurve`, used by `use_` on an unchanged plane or NURBS face, is
/// written as it is: the form the Brep writer admits directly.
fn written_as_is(
    use_: &cadmpeg_ir::topology::PcurveUse,
    pcurve: &Pcurve,
    edge_range: Option<[f64; 2]>,
) -> bool {
    if pcurve.wrapper_reversed() == Some(true) || pcurve.native_tail_flags().is_some() {
        return false;
    }
    let stated = pcurve
        .parameter_range()
        .map(cadmpeg_ir::units::FiniteVector::get);
    let Ok(interval) = use_interval(use_, pcurve, edge_range) else {
        return false;
    };
    if interval[0] >= interval[1] || stated.is_some_and(|stated| stated != interval) {
        return false;
    }
    match &pcurve.geometry {
        PcurveGeometry::Line(_) => true,
        PcurveGeometry::Nurbs { nurbs } => {
            !nurbs.periodic()
                && usize::try_from(nurbs.degree()).is_ok_and(|degree| {
                    let knots = nurbs.knots();
                    knots.get(degree) == Some(&interval[0])
                        && knots.get(nurbs.pole_rows().count()) == Some(&interval[1])
                })
        }
        _ => false,
    }
}

/// How one face's surface is written.
enum FaceSurface {
    /// The stated solved plane or nonperiodic NURBS patch, as it is.
    AsIs,
    /// A plane or a NURBS patch whose parameters are the stated ones.
    Same(SolvedSurfaceGeometry),
    /// A NURBS patch whose parameters the maps give from the stated ones.
    Mapped(Box<SolvedSurfaceGeometry>, ParameterMap, ParameterMap),
}

/// The parameter box of `splines`: the hull of their control points, which
/// holds each spline.
fn parameter_hull(splines: &[Spline]) -> Option<([f64; 2], [f64; 2])> {
    let mut u = [f64::INFINITY, f64::NEG_INFINITY];
    let mut v = [f64::INFINITY, f64::NEG_INFINITY];
    for point in splines.iter().flat_map(Spline::points) {
        u = [u[0].min(point.x), u[1].max(point.x)];
        v = [v[0].min(point.y), v[1].max(point.y)];
    }
    (u[0] < u[1] && v[0] < v[1]).then_some((u, v))
}

fn nurbs_surface_domain(
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
) -> Option<([f64; 2], [f64; 2])> {
    let u_degree = usize::try_from(surface.u_degree()).ok()?;
    let v_degree = usize::try_from(surface.v_degree()).ok()?;
    Some((
        [
            *surface.u_knots().get(u_degree)?,
            *surface.u_knots().get(surface.u_count())?,
        ],
        [
            *surface.v_knots().get(v_degree)?,
            *surface.v_knots().get(surface.v_count())?,
        ],
    ))
}

/// The written form of `surface` for a face whose pcurves are `splines`.
fn face_surface(surface: &SurfaceGeometry, splines: &[Spline]) -> Result<FaceSurface, String> {
    let solved = solved_surface(surface).ok_or("its surface has no solved form")?;
    let procedural = !matches!(surface, SurfaceGeometry::Solved(_));
    let hull = || {
        parameter_hull(splines)
            .ok_or_else(|| "its pcurves span no parameter area of its surface".to_owned())
    };
    match solved {
        SolvedSurfaceGeometry::Plane(_) => Ok(if procedural {
            FaceSurface::Same(solved.clone())
        } else {
            FaceSurface::AsIs
        }),
        SolvedSurfaceGeometry::Nurbs(nurbs) if !nurbs.u_periodic() && !nurbs.v_periodic() => {
            Ok(if procedural {
                FaceSurface::Same(solved.clone())
            } else {
                FaceSurface::AsIs
            })
        }
        SolvedSurfaceGeometry::Nurbs(nurbs) => {
            let (u_domain, v_domain) =
                nurbs_surface_domain(nurbs).ok_or("its NURBS surface has no knot domain")?;
            let (u_hull, v_hull) = hull()?;
            let u = if nurbs.u_periodic() { u_hull } else { u_domain };
            let v = if nurbs.v_periodic() { v_hull } else { v_domain };
            exact::clamped_surface(nurbs, u, v)
                .map(|patch| FaceSurface::Same(SolvedSurfaceGeometry::Nurbs(patch)))
        }
        SolvedSurfaceGeometry::Cylinder(_)
        | SolvedSurfaceGeometry::Cone(_)
        | SolvedSurfaceGeometry::Sphere(_)
        | SolvedSurfaceGeometry::Torus(_) => {
            let (u, v) = hull()?;
            let patch = exact::analytic_patch(solved, u, v)?;
            Ok(FaceSurface::Mapped(
                Box::new(SolvedSurfaceGeometry::Nurbs(patch.surface)),
                patch.u,
                patch.v,
            ))
        }
        SolvedSurfaceGeometry::Transformed(placed) => {
            let basis = match placed.basis() {
                SolvedSurfaceGeometry::Plane(plane) => {
                    let (u, v) = hull()?;
                    FaceSurface::Same(SolvedSurfaceGeometry::Nurbs(exact::plane_patch(
                        plane.origin().get(),
                        *plane.frame().reference().as_raw(),
                        *plane.frame().binormal().as_raw(),
                        u,
                        v,
                    )?))
                }
                basis => match face_surface(&SurfaceGeometry::Solved(basis.clone()), splines)? {
                    FaceSurface::AsIs => FaceSurface::Same(basis.clone()),
                    other => other,
                },
            };
            let transform = *placed.transform();
            let place = |geometry: SolvedSurfaceGeometry| match geometry {
                SolvedSurfaceGeometry::Nurbs(mut nurbs) => {
                    nurbs
                        .try_map_control_points(|_, pole| {
                            transform.apply_point(pole.get()).ok_or_else(|| {
                                "the placed surface leaves the finite range".to_owned()
                            })
                        })
                        .map_err(|error| error.clone())?;
                    Ok(SolvedSurfaceGeometry::Nurbs(nurbs))
                }
                _ => Err("its placed surface has no NURBS form the writer builds".to_owned()),
            };
            match basis {
                FaceSurface::AsIs => Err("its placed surface has no NURBS form".into()),
                FaceSurface::Same(geometry) => Ok(FaceSurface::Same(place(geometry)?)),
                FaceSurface::Mapped(geometry, u, v) => {
                    Ok(FaceSurface::Mapped(Box::new(place(*geometry)?), u, v))
                }
            }
        }
        _ => Err("its surface kind has no NURBS form the writer builds".into()),
    }
}

/// One coedge's pcurve as the face writes it.
struct CoedgePcurve {
    /// The pcurve record the coedge keeps.
    id: String,
    /// The written pcurve in the stated surface parameters, `None` where the
    /// record is written as it is.
    spline: Option<Spline>,
}

/// How the face's surface takes its pcurves.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PcurveRule {
    /// A plane: a pcurve is kept where it is written as it is over its
    /// edge's range, and the trim is otherwise the edge's projection.
    Plane,
    /// A nonperiodic NURBS patch written as it is.
    AsIs,
    /// A surface written in another form: every pcurve is rebuilt.
    Rebuilt,
}

/// The pcurve of `coedge` on `face`, `None` where the trim is the projection
/// of the edge onto the face's plane.
fn coedge_pcurve(
    coedge: &Coedge,
    surface: &str,
    edge_range: Option<[f64; 2]>,
    pcurves: &HashMap<&str, &Pcurve>,
    rule: PcurveRule,
) -> Result<Option<CoedgePcurve>, String> {
    let uses = face_uses(coedge, surface, pcurves);
    if let [(use_, pcurve)] = uses.as_slice() {
        let as_is = written_as_is(use_, pcurve, edge_range);
        let kept = match rule {
            PcurveRule::Plane => as_is && use_interval(use_, pcurve, edge_range).ok() == edge_range,
            PcurveRule::AsIs => as_is,
            PcurveRule::Rebuilt => false,
        };
        if kept {
            return Ok(Some(CoedgePcurve {
                id: pcurve.id.as_str().to_owned(),
                spline: None,
            }));
        }
    }
    let Some((_, first)) = uses.first() else {
        return Ok(None);
    };
    if rule == PcurveRule::Plane {
        return Ok(None);
    }
    let mut joined: Option<Spline> = None;
    for (use_, pcurve) in &uses {
        let interval = use_interval(use_, pcurve, edge_range)
            .map_err(|reason| format!("pcurve {}: {reason}", pcurve.id.as_str()))?;
        let spline = exact::pcurve_spline(&pcurve.geometry, interval)
            .map_err(|reason| format!("pcurve {}: {reason}", pcurve.id.as_str()))?;
        joined = Some(match joined {
            None => spline,
            Some(previous) => previous.join(spline).map_err(|reason| {
                format!(
                    "the pcurves of coedge {} do not join into one NURBS curve: {reason}",
                    coedge.id.as_str()
                )
            })?,
        });
    }
    Ok(Some(CoedgePcurve {
        id: first.id.as_str().to_owned(),
        spline: joined,
    }))
}

/// What one face becomes.
struct FacePlan {
    surface: FaceSurface,
    /// For each coedge of the face, its pcurve.
    pcurves: Vec<(String, Option<CoedgePcurve>)>,
}

/// Gives each face its own surface record and each edge its own curve
/// record, as a 3DM Brep holds them. A copy's id is its source's id with the
/// key of the face or edge that uses it appended. Returns the source id of
/// each copy.
fn unshare_carriers(ir: &mut CadIr) -> HashMap<String, String> {
    let geometry = &mut ir.geometry;
    let key = |id: &str| id.split_once('#').map(|(_, key)| key.to_owned());
    let mut origins = HashMap::new();
    let mut seen = BTreeSet::new();
    let mut surfaces = Vec::new();
    for face in &mut geometry.faces {
        if seen.insert(face.surface.as_str().to_owned()) {
            continue;
        }
        let copy = key(face.id.as_str())
            .and_then(|key| SurfaceId::mint(format!("{}.on-{key}", face.surface.as_str())).ok())
            .zip(
                geometry
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == face.surface),
            );
        if let Some((id, source)) = copy {
            let mut copy = source.clone();
            copy.id = id.clone();
            origins.insert(id.as_str().to_owned(), face.surface.as_str().to_owned());
            surfaces.push(copy);
            face.surface = id;
        }
    }
    geometry.surfaces.extend(surfaces);
    seen.clear();
    let mut curves = Vec::new();
    for edge in &mut geometry.edges {
        let Some(curve) = edge.curve().cloned() else {
            continue;
        };
        if seen.insert(curve.as_str().to_owned()) {
            continue;
        }
        let copy = key(edge.id.as_str())
            .and_then(|key| CurveId::mint(format!("{}.on-{key}", curve.as_str())).ok())
            .zip(geometry.curves.iter().find(|source| source.id == curve));
        if let Some((id, source)) = copy {
            let mut copy = source.clone();
            copy.id = id.clone();
            if edge.set_curve(Some(id)).is_ok() {
                curves.push(copy);
            }
        }
    }
    geometry.curves.extend(curves);
    origins
}

/// Converts the carriers of the Brep scope `ir`.
pub(super) fn convert_scope(ir: &CadIr) -> ConvertedScope {
    let mut unshared = ir.clone();
    let origins = unshare_carriers(&mut unshared);
    let ir = &unshared;
    let geometry = &ir.geometry;
    let curves = geometry
        .curves
        .iter()
        .map(|curve| (curve.id.as_str(), curve))
        .collect::<HashMap<_, _>>();
    let surfaces = geometry
        .surfaces
        .iter()
        .map(|surface| (surface.id.as_str(), surface))
        .collect::<HashMap<_, _>>();
    let pcurves = geometry
        .pcurves
        .iter()
        .map(|pcurve| (pcurve.id.as_str(), pcurve))
        .collect::<HashMap<_, _>>();
    let edges = geometry
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<HashMap<_, _>>();
    let coedges = geometry
        .coedges
        .iter()
        .map(|coedge| (coedge.id.as_str(), coedge))
        .collect::<HashMap<_, _>>();
    let loops = geometry
        .loops
        .iter()
        .map(|loop_| (loop_.id.as_str(), loop_))
        .collect::<HashMap<_, _>>();

    let mut curve_users = HashMap::<&str, usize>::new();
    for edge in &geometry.edges {
        if let Some(curve) = edge.curve() {
            *curve_users.entry(curve.as_str()).or_default() += 1;
        }
    }
    let mut edge_curves = BTreeMap::<&str, SolvedCurveGeometry>::new();
    let mut edge_failures = BTreeMap::<&str, String>::new();
    for edge in &geometry.edges {
        let converted = match edge.curve() {
            None => Err("the edge has no curve".to_owned()),
            Some(id) => match curves.get(id.as_str()) {
                None => Err(format!("its curve {} is missing", id.as_str())),
                Some(curve) => edge_curve(edge, curve),
            },
        };
        match converted {
            Ok(None) => {}
            Ok(Some(_)) if edge.curve().is_some_and(|id| curve_users[id.as_str()] > 1) => {
                edge_failures.insert(
                    edge.id.as_str(),
                    "its curve is shared by several edges and needs its own NURBS form".into(),
                );
            }
            Ok(Some(curve)) => {
                if let Some(id) = edge.curve() {
                    edge_curves.insert(id.as_str(), curve);
                }
            }
            Err(reason) => {
                edge_failures.insert(edge.id.as_str(), reason);
            }
        }
    }

    let mut pcurve_users = HashMap::<&str, usize>::new();
    for coedge in &geometry.coedges {
        for use_ in &coedge.pcurves {
            *pcurve_users.entry(use_.pcurve.as_str()).or_default() += 1;
        }
    }
    let mut dropped = Vec::new();
    let mut plans = BTreeMap::<&str, FacePlan>::new();
    for face in &geometry.faces {
        let plan = (|| -> Result<FacePlan, String> {
            let boundary = face
                .loops
                .boundary()
                .filter(|loops| !loops.is_empty())
                .ok_or(
                    "the face has no boundary loop, and a 3DM Brep face holds an outer trim loop",
                )?;
            let mut face_coedges = Vec::new();
            for id in boundary {
                let loop_ = loops
                    .get(id.as_str())
                    .ok_or_else(|| format!("its loop {} is missing", id.as_str()))?;
                for id in loop_.coedges() {
                    face_coedges.push(
                        *coedges
                            .get(id.as_str())
                            .ok_or_else(|| format!("its coedge {} is missing", id.as_str()))?,
                    );
                }
            }
            for coedge in &face_coedges {
                if let Some(reason) = edge_failures.get(coedge.edge.as_str()) {
                    return Err(format!("edge {}: {reason}", coedge.edge.as_str()));
                }
            }
            let origin = origins
                .get(face.surface.as_str())
                .map_or(face.surface.as_str(), String::as_str);
            let surface = surfaces
                .get(face.surface.as_str())
                .ok_or_else(|| format!("its surface {} is missing", face.surface.as_str()))?;
            let rule = match &surface.geometry {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
                | SurfaceGeometry::Procedural {
                    cache: Some(SolvedSurfaceGeometry::Plane(_)),
                    ..
                } => PcurveRule::Plane,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs))
                    if !nurbs.u_periodic() && !nurbs.v_periodic() =>
                {
                    PcurveRule::AsIs
                }
                _ => PcurveRule::Rebuilt,
            };
            let mut face_pcurves = Vec::new();
            for coedge in &face_coedges {
                let edge_range = edges
                    .get(coedge.edge.as_str())
                    .and_then(|edge| edge.param_range())
                    .map(cadmpeg_ir::units::FiniteVector::get);
                let pcurve = coedge_pcurve(coedge, origin, edge_range, &pcurves, rule)?;
                face_pcurves.push((coedge.id.as_str().to_owned(), pcurve));
            }
            let splines = face_pcurves
                .iter()
                .filter_map(|(_, pcurve)| pcurve.as_ref()?.spline.clone())
                .collect::<Vec<_>>();
            let written = face_surface(&surface.geometry, &splines)
                .map_err(|reason| format!("surface {}: {reason}", face.surface.as_str()))?;
            let plane = match &written {
                FaceSurface::AsIs => matches!(
                    &surface.geometry,
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
                ),
                FaceSurface::Same(geometry) => matches!(geometry, SolvedSurfaceGeometry::Plane(_)),
                FaceSurface::Mapped(..) => false,
            };
            for (coedge, pcurve) in &face_pcurves {
                match pcurve {
                    None if !plane => {
                        return Err(format!(
                            "coedge {coedge} has no pcurve on the face's NURBS surface"
                        ))
                    }
                    Some(pcurve)
                        if pcurve.spline.is_some()
                            && pcurve_users.get(pcurve.id.as_str()).copied().unwrap_or(0) > 1 =>
                    {
                        return Err(format!(
                            "pcurve {} is shared by several coedges and needs its own NURBS form",
                            pcurve.id
                        ))
                    }
                    _ => {}
                }
            }
            Ok(FacePlan {
                surface: written,
                pcurves: face_pcurves,
            })
        })();
        match plan {
            Ok(plan) => {
                plans.insert(face.id.as_str(), plan);
            }
            Err(reason) => dropped.push((face.id.as_str().to_owned(), reason)),
        }
    }

    let mut converted = ir.clone();
    let out = &mut converted.geometry;
    let dropped_ids = dropped
        .iter()
        .map(|(id, _)| id.clone())
        .collect::<BTreeSet<_>>();
    out.faces
        .retain(|face| !dropped_ids.contains(face.id.as_str()));
    for curve in &mut out.curves {
        if let Some(solved) = edge_curves.remove(curve.id.as_str()) {
            curve.geometry = CurveGeometry::Solved(solved);
        }
    }
    let mut kept_pcurves = BTreeSet::new();
    let mut new_pcurves = BTreeMap::<String, PcurveGeometry>::new();
    let mut coedge_pcurves = BTreeMap::<String, Option<String>>::new();
    let mut new_surfaces = BTreeMap::<String, SolvedSurfaceGeometry>::new();
    for face in &out.faces {
        let Some(plan) = plans.remove(face.id.as_str()) else {
            continue;
        };
        let maps = match plan.surface {
            FaceSurface::AsIs => None,
            FaceSurface::Same(geometry) => {
                new_surfaces.insert(face.surface.as_str().to_owned(), geometry);
                None
            }
            FaceSurface::Mapped(geometry, u, v) => {
                new_surfaces.insert(face.surface.as_str().to_owned(), *geometry);
                Some((u, v))
            }
        };
        for (coedge, pcurve) in plan.pcurves {
            let Some(pcurve) = pcurve else {
                coedge_pcurves.insert(coedge, None);
                continue;
            };
            kept_pcurves.insert(pcurve.id.clone());
            if let Some(spline) = pcurve.spline {
                let written = match &maps {
                    Some((u, v)) => exact::composed_pcurve(spline, u, v),
                    None => exact::pcurve_geometry(spline),
                };
                match written {
                    Ok(geometry) => {
                        new_pcurves.insert(pcurve.id.clone(), geometry);
                    }
                    Err(reason) => {
                        dropped.push((
                            face.id.as_str().to_owned(),
                            format!("pcurve {}: {reason}", pcurve.id),
                        ));
                    }
                }
            }
            coedge_pcurves.insert(coedge, Some(pcurve.id));
        }
    }
    let late = dropped
        .iter()
        .map(|(id, _)| id.clone())
        .filter(|id| !dropped_ids.contains(id.as_str()))
        .collect::<BTreeSet<_>>();
    if !late.is_empty() {
        out.faces.retain(|face| !late.contains(face.id.as_str()));
    }
    for surface in &mut out.surfaces {
        if let Some(solved) = new_surfaces.remove(surface.id.as_str()) {
            surface.geometry = SurfaceGeometry::Solved(solved);
        }
    }
    for coedge in &mut out.coedges {
        match coedge_pcurves.get(coedge.id.as_str()) {
            Some(Some(id)) => {
                coedge.pcurves.retain(|use_| use_.pcurve.as_str() == id);
                coedge.pcurves.truncate(1);
                if new_pcurves.contains_key(id) {
                    if let Some(use_) = coedge.pcurves.first_mut() {
                        use_.parameter_range = None;
                    }
                }
            }
            Some(None) => coedge.pcurves.clear(),
            None => {}
        }
    }
    for pcurve in &mut out.pcurves {
        if let Some(geometry) = new_pcurves.remove(pcurve.id.as_str()) {
            let range = match &geometry {
                PcurveGeometry::Nurbs { nurbs } => {
                    usize::try_from(nurbs.degree()).ok().and_then(|degree| {
                        Some([
                            *nurbs.knots().get(degree)?,
                            *nurbs.knots().get(nurbs.pole_rows().count())?,
                        ])
                    })
                }
                _ => None,
            };
            let mut metadata = PcurveMetadata::general(
                None,
                range.and_then(cadmpeg_ir::units::FiniteVector::new),
                pcurve.fit_tolerance(),
            );
            if let (PcurveMetadata::General { form }, PcurveMetadata::General { form: source }) =
                (&mut metadata, &pcurve.metadata)
            {
                form.support_surface.clone_from(&source.support_surface);
            }
            pcurve.geometry = geometry;
            pcurve.metadata = metadata;
        }
    }
    let used = out
        .coedges
        .iter()
        .flat_map(|coedge| {
            coedge
                .pcurves
                .iter()
                .map(|use_| use_.pcurve.as_str().to_owned())
        })
        .collect::<BTreeSet<_>>();
    out.pcurves
        .retain(|pcurve| used.contains(pcurve.id.as_str()));
    ConvertedScope {
        ir: converted,
        dropped,
    }
}

#[cfg(test)]
mod tests;
