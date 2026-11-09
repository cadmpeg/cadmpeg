// SPDX-License-Identifier: Apache-2.0
//! Lower source-bound directed line cycles into planar sheet components.

use std::collections::HashSet;
use std::fmt::Display;

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::AnnotationStorage;
use cadmpeg_ir::document::{CadIr, Model};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{LineCurve, PlaneSurface};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, IdentityError, LoopId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::index::{ModelIndex, StandardIndex};
use cadmpeg_ir::math::{planar::polygon_area_twice, Point2, Vector3};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, EdgeCarrier, Face, FaceLoops, IncreasingParameterInterval, Loop,
    LoopBoundary, LoopRing, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::checked::{ExactHypotUnitVector3, UnitFrame3};

use super::{SheetLine, SheetPlan};

const ADMIT: &str = "admit outer sketch sheet entities";

struct Annotation {
    id: String,
    offset: u64,
    tag: &'static str,
    fields: &'static [&'static str],
}

#[derive(Default)]
struct Prepared {
    model: Model,
    annotations: Vec<Annotation>,
    identities: HashSet<String>,
}

fn sheet_id<T>(
    kind: &str,
    key: impl Display,
    mint: impl FnOnce(String) -> Result<T, IdentityError>,
) -> Result<T, CodecError> {
    mint(format!("catia:outer-sheet:{kind}#{key}")).map_err(CodecError::malformed)
}

fn lifted(plan: &SheetPlan, point: Point2) -> Option<FinitePoint3> {
    FinitePoint3::new(
        plan.origin
            .translated(plan.u_axis, point.u)
            .translated(plan.v_axis, point.v),
    )
}

fn direction(plan: &SheetPlan, line: &SheetLine) -> Vector3 {
    Vector3::new(
        plan.u_axis
            .x
            .mul_add(line.direction.u, plan.v_axis.x * line.direction.v),
        plan.u_axis
            .y
            .mul_add(line.direction.u, plan.v_axis.y * line.direction.v),
        plan.u_axis
            .z
            .mul_add(line.direction.u, plan.v_axis.z * line.direction.v),
    )
}

fn plane(plan: &SheetPlan) -> Option<PlaneSurface> {
    let unit = |v: Vector3| ExactHypotUnitVector3::new([v.x, v.y, v.z]);
    let frame = unit(plan.u_axis.cross(plan.v_axis))
        .zip(unit(plan.u_axis))
        .zip(unit(plan.v_axis))
        .and_then(|((normal, horizontal), vertical)| {
            UnitFrame3::right_handed(normal, horizontal, vertical)
        })?;
    let origin = FinitePoint3::new(plan.origin)?;
    Some(PlaneSurface::new(origin, frame.into()))
}

fn ports(line: &SheetLine) -> [(u32, Point2); 2] {
    let mut result = [(line.start_id, line.start), (line.end_id, line.end)];
    if !line.forward {
        result.reverse();
    }
    result
}

fn annotation(
    prepared: &mut Prepared,
    (model, staged): (&ModelIndex<'_>, &HashSet<String>),
    id: impl Display,
    offset: u64,
    tag: &'static str,
    fields: &'static [&'static str],
) -> Result<(), CodecError> {
    let id = id.to_string();
    if model.contains(&id, StandardIndex)
        || staged.contains(&id)
        || !prepared.identities.insert(id.clone())
    {
        return Err(CodecError::malformed(
            "outer sheet entity identity is repeated",
        ));
    }
    prepared.annotations.push(Annotation {
        id,
        offset,
        tag,
        fields,
    });
    Ok(())
}

fn prepare_component(
    prepared: &mut Prepared,
    plan: &SheetPlan,
    lines: &[SheetLine],
    component: usize,
    carrier: PlaneSurface,
    tolerance: f64,
    existing: (&ModelIndex<'_>, &HashSet<String>),
) -> Result<bool, CodecError> {
    if lines.len() < 3 {
        return Ok(false);
    }
    let key = format!("{}:{component}", plan.source_offset);
    let body = sheet_id("body", &key, BodyId::mint)?;
    let region = sheet_id("region", &key, RegionId::mint)?;
    let shell = sheet_id("shell", &key, ShellId::mint)?;
    let surface = sheet_id("surface", &key, SurfaceId::mint)?;
    let face = sheet_id("face", &key, FaceId::mint)?;
    let loop_id = sheet_id("loop", &key, LoopId::mint)?;
    let mut vertices = HashSet::new();
    let mut sources = HashSet::new();
    let mut polygon = Vec::new();
    let mut coedges = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let [start, end] = ports(line);
        let next = ports(&lines[(index + 1) % lines.len()])[0];
        if start.0 == end.0 || start.1 == end.1 || end != next || !vertices.insert(start.0) {
            return Ok(false);
        }
        if !sources.insert(line.source_offset) {
            return Err(CodecError::malformed(
                "outer sheet line identity is repeated",
            ));
        }
        polygon.push(start.1);
        let Some(interval) = IncreasingParameterInterval::new(line.interval) else {
            return Ok(false);
        };
        let (Some(origin), Some(start_point), Some(end_point)) = (
            lifted(plan, line.origin),
            lifted(plan, line.start),
            lifted(plan, line.end),
        ) else {
            return Ok(false);
        };
        let Ok(geometry) = LineCurve::try_new(origin.get(), direction(plan, line)) else {
            return Ok(false);
        };
        let endpoints = [start_point, end_point];
        for (parameter, endpoint) in line.interval.into_iter().zip(endpoints) {
            let evaluated = geometry
                .origin()
                .get()
                .translated(*geometry.direction().as_raw(), parameter);
            if !evaluated.is_finite() || evaluated.distance(endpoint.get()) > tolerance {
                return Ok(false);
            }
        }
        let vertex_key = format!("{key}:{}", start.0);
        let point = sheet_id("point", &vertex_key, PointId::mint)?;
        let vertex = sheet_id("vertex", &vertex_key, VertexId::mint)?;
        annotation(
            prepared,
            existing,
            &point,
            line.source_offset,
            "placed_sketch_endpoint",
            &["position"],
        )?;
        annotation(
            prepared,
            existing,
            &vertex,
            line.source_offset,
            "sketch_endpoint_identity",
            &["point"],
        )?;
        prepared.model.points.push(Point::new(
            point.clone(),
            if line.forward { start_point } else { end_point },
            None,
        ));
        prepared.model.vertices.push(Vertex {
            id: vertex,
            point,
            tolerance: None,
        });

        let line_key = format!("{key}:{}", line.source_offset);
        let curve = sheet_id("curve", &line_key, CurveId::mint)?;
        let edge = sheet_id("edge", &line_key, EdgeId::mint)?;
        let coedge = sheet_id("coedge", &line_key, CoedgeId::mint)?;
        annotation(
            prepared,
            existing,
            &curve,
            line.source_offset,
            "placed_native_sketch_line",
            &["geometry", "parameter_range"],
        )?;
        annotation(
            prepared,
            existing,
            &edge,
            line.source_offset,
            "sketch_boundary_edge",
            &["carrier", "start", "end"],
        )?;
        annotation(
            prepared,
            existing,
            &coedge,
            line.source_offset,
            "directed_sketch_boundary",
            &["owner_loop", "edge", "radial_next", "sense"],
        )?;
        prepared.model.curves.push(Curve {
            id: curve.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(geometry)),
            parameter_range: Some(interval),
            source_object: None,
        });
        prepared.model.edges.push(Edge {
            id: edge.clone(),
            carrier: EdgeCarrier::new(Some(curve), Some(line.interval))
                .map_err(CodecError::malformed)?,
            start: sheet_id(
                "vertex",
                format_args!("{key}:{}", line.start_id),
                VertexId::mint,
            )?,
            end: sheet_id(
                "vertex",
                format_args!("{key}:{}", line.end_id),
                VertexId::mint,
            )?,
            tolerance: None,
        });
        prepared.model.coedges.push(Coedge {
            id: coedge.clone(),
            owner_loop: loop_id.clone(),
            edge,
            radial_next: coedge.clone(),
            sense: if line.forward {
                Sense::Forward
            } else {
                Sense::Reversed
            },
            pcurves: Vec::new(),
            use_curve: None,
        });
        coedges.push(coedge);
    }
    let Some(area) = polygon_area_twice(&polygon).filter(|area| area.get() != 0.0) else {
        return Ok(false);
    };
    for (id, tag, fields) in [
        (
            body.as_str(),
            "split_fill_component",
            &["kind", "regions"][..],
        ),
        (
            region.as_str(),
            "fill_component_region",
            &["body", "shells"][..],
        ),
        (
            shell.as_str(),
            "open_fill_component_shell",
            &["region", "members"][..],
        ),
        (surface.as_str(), "sketch_chart_plane", &["geometry"][..]),
        (
            face.as_str(),
            "planar_fill_component",
            &["shell", "surface", "sense", "loops"][..],
        ),
        (
            loop_id.as_str(),
            "closed_fill_boundary",
            &["face", "boundary"][..],
        ),
    ] {
        annotation(prepared, existing, id, plan.source_offset, tag, fields)?;
    }
    prepared.model.bodies.push(Body {
        removed_by: None,
        id: body.clone(),
        kind: BodyKind::Sheet,
        regions: vec![region.clone()],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    prepared.model.regions.push(Region {
        id: region.clone(),
        body,
        shells: vec![shell.clone()],
    });
    prepared
        .model
        .shells
        .push(Shell::with_face(shell.clone(), region, face.clone()));
    prepared.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(carrier)),
        source_object: None,
    });
    prepared.model.faces.push(Face {
        id: face.clone(),
        shell,
        surface,
        sense: if area.get() > 0.0 {
            Sense::Forward
        } else {
            Sense::Reversed
        },
        loops: FaceLoops::classified(loop_id.clone(), Vec::new()),
        name: None,
        color: None,
        tolerance: None,
    });
    prepared.model.loops.push(Loop {
        id: loop_id,
        face,
        boundary: LoopBoundary::Ring(
            LoopRing::new(coedges, Vec::new()).map_err(CodecError::malformed)?,
        ),
    });
    Ok(true)
}

fn append_model(target: &mut Model, source: &mut Model) {
    target.points.append(&mut source.points);
    target.vertices.append(&mut source.vertices);
    target.curves.append(&mut source.curves);
    target.edges.append(&mut source.edges);
    target.coedges.append(&mut source.coedges);
    target.surfaces.append(&mut source.surfaces);
    target.faces.append(&mut source.faces);
    target.loops.append(&mut source.loops);
    target.shells.append(&mut source.shells);
    target.regions.append(&mut source.regions);
    target.bodies.append(&mut source.bodies);
}

pub(super) fn emit(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl AnnotationStorage>,
    plans: &[SheetPlan],
) -> Result<usize, CodecError> {
    let maximum_count =
        plans
            .iter()
            .flat_map(|plan| &plan.components)
            .try_fold(0usize, |count, lines| {
                lines
                    .len()
                    .checked_mul(5)
                    .and_then(|n| n.checked_add(6))
                    .and_then(|n| count.checked_add(n))
                    .ok_or_else(|| CodecError::malformed("outer sheet entity population overflows"))
            })?;
    if maximum_count == 0 {
        return Ok(0);
    }
    let mut prepared = Prepared::default();
    {
        let index = ModelIndex::new_model_only(ir);
        for plan in plans {
            let Some(carrier) = plane(plan) else { continue };
            let mut candidate = Prepared::default();
            let mut supported = true;
            for (component, lines) in plan.components.iter().enumerate() {
                if !prepare_component(
                    &mut candidate,
                    plan,
                    lines,
                    component,
                    carrier,
                    ir.tolerances.linear.get(),
                    (&index, &prepared.identities),
                )? {
                    supported = false;
                    break;
                }
            }
            if supported {
                append_model(&mut prepared.model, &mut candidate.model);
                prepared.annotations.append(&mut candidate.annotations);
                prepared.identities.extend(candidate.identities);
            }
        }
    }
    let bodies = prepared.model.bodies.len();
    if bodies == 0 {
        return Ok(0);
    }
    ctx.charge_entities(u64_from_index(prepared.model.entity_count()), ADMIT)?;
    for note in prepared.annotations {
        crate::assemble::annotate(
            annotations,
            &note.id,
            "outer-sketch",
            note.offset,
            note.tag,
            Exactness::Derived,
        )?;
        for field in note.fields {
            annotations.derived(&note.id, field)?;
        }
    }
    append_model(&mut ir.model, &mut prepared.model);
    Ok(bodies)
}

#[cfg(test)]
mod tests;
