// SPDX-License-Identifier: Apache-2.0
//! Exact restatements that let the writer encode a document in IGES entities.

use super::{
    edge_span, effective_topology_tolerance, flatten_curve, polyline_knots, polyline_parameters,
    reverse_nurbs, solved_curve, topology_edge_explicit_tolerance, TopologyIndex,
};
use cadmpeg_ir::geometry::analytic::PlaneSurface;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::pcurve::{Pcurve, PcurveGeometry, PcurveMetadata, PcurveNurbs};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::PcurveId;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::{Coedge, LoopBoundaryRole, PcurveUse, Sense};
use cadmpeg_ir::units::FiniteVector;
use cadmpeg_ir::CadIr;
use std::borrow::Cow;

/// The document with a parameter curve for each coedge of a plane face that
/// Type 144, or a Type 143 whose loops already carry parameter curves,
/// requires. Each computed curve is the image of the edge curve under the
/// plane's inverse frame, so it is exact.
pub(super) fn state_plane_pcurves(ir: &CadIr) -> Cow<'_, CadIr> {
    if ir
        .geometry
        .coedges
        .iter()
        .all(|coedge| !coedge.pcurves.is_empty())
    {
        return Cow::Borrowed(ir);
    }
    let index = TopologyIndex::new(ir);
    let mut computed = Vec::new();
    for face in &ir.geometry.faces {
        let Some(surface) = index.surfaces.get(face.surface.as_str()) else {
            continue;
        };
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) = &surface.geometry else {
            continue;
        };
        let Some(loops) = face
            .loops
            .stated()
            .map(|loop_id| index.loops.get(loop_id.as_str()).copied())
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Some(coedges) = loops
            .iter()
            .flat_map(|loop_| loop_.coedges())
            .map(|coedge_id| index.coedges.get(coedge_id.as_str()).copied())
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let trimmed = loops
            .iter()
            .any(|loop_| face.loop_role(&loop_.id) != LoopBoundaryRole::Unspecified);
        if !trimmed && coedges.iter().all(|coedge| coedge.pcurves.is_empty()) {
            continue;
        }
        for coedge in coedges {
            if !coedge.pcurves.is_empty() {
                continue;
            }
            if let Some(pcurve) = plane_pcurve(&index, plane, coedge) {
                computed.push((coedge.id.clone(), pcurve));
            }
        }
    }
    if computed.is_empty() {
        return Cow::Borrowed(ir);
    }
    let mut stated = ir.clone();
    for (coedge_id, pcurve) in computed {
        if let Some(coedge) = stated
            .geometry
            .coedges
            .iter_mut()
            .find(|coedge| coedge.id == coedge_id)
        {
            coedge.pcurves = vec![PcurveUse {
                pcurve: pcurve.id.clone(),
                isoparametric: None,
                parameter_range: None,
            }];
            stated.geometry.pcurves.push(pcurve);
        }
    }
    Cow::Owned(stated)
}

/// The parameter curve of `coedge` on `plane`, in the coedge's traversal.
fn plane_pcurve(
    index: &TopologyIndex<'_>,
    plane: &PlaneSurface,
    coedge: &Coedge,
) -> Option<Pcurve> {
    let id = PcurveId::try_from(format!(
        "iges:writer:pcurve#{}",
        coedge.id.as_str().replace('#', "/")
    ))
    .ok()?;
    if index.pcurves.contains_key(id.as_str()) {
        return None;
    }
    let edge = index.edges.get(coedge.edge.as_str()).copied()?;
    let curve = index.curves.get(edge.curve()?.as_str()).copied()?;
    let geometry = flatten_curve(solved_curve(curve).ok()?).ok()?;
    let span = edge_span(index, edge, &geometry).ok()?;
    let nurbs = nurbs_form(&geometry, span.range)?;
    let (nurbs, range) = if coedge.sense == Sense::Reversed {
        reverse_nurbs(&nurbs, span.range).ok()?
    } else {
        (nurbs, span.range)
    };
    let tolerance = effective_topology_tolerance(topology_edge_explicit_tolerance(index, edge));
    let origin = plane.origin().get();
    let normal = plane.frame().axis().as_raw();
    let u_axis = plane.frame().reference().as_raw();
    let v_axis = plane.frame().binormal();
    let v_axis = v_axis.as_raw();
    let points = nurbs
        .control_points()
        .into_iter()
        .map(|point| {
            let point = point.get();
            let offset = cadmpeg_ir::math::Vector3::new(
                point.x - origin.x,
                point.y - origin.y,
                point.z - origin.z,
            );
            (offset.dot(*normal).abs() <= tolerance)
                .then(|| Point2::new(offset.dot(*u_axis), offset.dot(*v_axis)))
        })
        .collect::<Option<Vec<_>>>()?;
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
    let nurbs =
        PcurveNurbs::from_lanes(nurbs.degree(), knots, points, weights, nurbs.periodic()).ok()?;
    Some(Pcurve {
        id,
        geometry: PcurveGeometry::Nurbs { nurbs },
        metadata: PcurveMetadata::general(None, Some(range), None),
        native_ref: None,
    })
}

/// The exact NURBS form of `geometry` whose domain contains `range`.
fn nurbs_form(geometry: &CurveGeometry, range: FiniteVector<2>) -> Option<NurbsCurve> {
    let interval = range.finite_components().map(FiniteReal::get);
    match geometry.solved()? {
        SolvedCurveGeometry::Line(line) => {
            let origin = line.origin().get();
            let direction = *line.direction().as_raw();
            let at = |parameter: f64| {
                Point3::new(
                    direction.x.mul_add(parameter, origin.x),
                    direction.y.mul_add(parameter, origin.y),
                    direction.z.mul_add(parameter, origin.z),
                )
            };
            let [start, end] = interval;
            NurbsCurve::from_lanes(
                1,
                vec![start, start, end, end],
                vec![at(start), at(end)],
                None,
                false,
            )
            .ok()
        }
        SolvedCurveGeometry::Circle(circle) => {
            crate::entities::curve_conversion::circular_arc_nurbs(
                circle.center().get(),
                *circle.frame().axis().as_raw(),
                *circle.frame().reference().as_raw(),
                circle.radius(),
                interval,
            )
            .ok()?
        }
        SolvedCurveGeometry::Ellipse(ellipse) => {
            crate::entities::curve_conversion::elliptical_arc_nurbs(
                ellipse.center().get(),
                *ellipse.frame().axis().as_raw(),
                *ellipse.frame().reference().as_raw(),
                ellipse.major_radius(),
                ellipse.minor_radius(),
                interval,
            )
            .ok()?
        }
        SolvedCurveGeometry::Parabola(parabola) => {
            crate::entities::curve_conversion::parabolic_arc_nurbs(
                parabola.vertex().get(),
                *parabola.frame().axis().as_raw(),
                *parabola.frame().reference().as_raw(),
                parabola.focal_distance(),
                interval,
            )
            .ok()?
        }
        SolvedCurveGeometry::Nurbs(nurbs) => Some(nurbs.clone()),
        SolvedCurveGeometry::Polyline(polyline) => {
            let parameters = polyline_parameters(polyline).ok()?;
            NurbsCurve::from_lanes(
                1,
                polyline_knots(&parameters),
                polyline.points().collect(),
                None,
                false,
            )
            .ok()
        }
        _ => None,
    }
}
