// SPDX-License-Identifier: Apache-2.0
//! Neutral topology to subshape-first OCCT text `BRep`.

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::topology::{Body, BodyKind, Sense};
use std::collections::HashMap;

use super::geometry;
mod facets;
pub(super) mod native_tables;
pub(super) mod occurrences;
#[cfg(test)]
mod prototypes;
mod reachable;
use reachable::Reach;

const LOCATION_SIMILARITY_TOLERANCE: f64 = 1.0e-12;

#[derive(Clone, PartialEq)]
struct Shape {
    kind: &'static str,
    geometry: String,
    children: Vec<ShapeUse>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ShapeUse {
    index: usize,
    sense: Sense,
    location: usize,
}

impl ShapeUse {
    fn new(index: usize, sense: Sense) -> Self {
        Self {
            index,
            sense,
            location: 0,
        }
    }
}

pub(super) struct Serialization {
    pub(super) bytes: Vec<u8>,
    pub(super) meshes: std::collections::HashSet<String>,
    pub(super) topology: Vec<occurrences::EmittedOccurrence>,
    pub(super) polygon_homes: HashMap<String, Vec<usize>>,
}

/// A carrier that has no `FCStd` encoding is an entity loss, not an invalid
/// document. The caller omits its containing body and reports the reason.
pub(super) fn serialize(ir: &CadIr, bodies: &[&Body]) -> Result<Option<Serialization>, CodecError> {
    if bodies.iter().any(|body| {
        body.transform
            .is_some_and(|transform| !location_supported(transform))
    }) {
        return Ok(None);
    }
    serialize_inner(ir, bodies, false, &std::collections::HashSet::new(), None)
}

/// Regenerate a canonical payload from its complete current forward-linked graph.
pub(super) fn serialize_payload(
    ir: &CadIr,
    payload: &crate::brep::ShapePayloadRecord,
) -> Result<Option<Serialization>, CodecError> {
    let bodies = ir
        .model
        .bodies
        .iter()
        .filter(|body| body.native_ref.as_deref() == Some(payload.id.as_str()))
        .collect::<Vec<_>>();
    if bodies.iter().any(|body| {
        body.transform
            .is_some_and(|value| !location_supported(value))
    }) {
        return Ok(None);
    }
    serialize_inner(
        ir,
        &bodies,
        true,
        &std::collections::HashSet::new(),
        Some(&payload.id),
    )
}

pub(super) fn unowned(
    ir: &CadIr,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    payload_owners: &std::collections::HashSet<String>,
) -> Result<Option<Serialization>, CodecError> {
    let row_owners = ir
        .native
        .namespace("fcstd")
        .map(crate::brep::records::row_owners)
        .transpose()?
        .unwrap_or_default();
    let reachable = Reach::unowned(ir, &row_owners);
    for curve in ir
        .model
        .curves
        .iter()
        .filter(|curve| reachable.curves.contains(curve.id.as_str()))
    {
        if geometry::curve_record(ir, curve).is_none()
            && (facets::polyline(curve).is_none()
                || !ir.model.edges.iter().any(|edge| {
                    reachable.edges.contains(edge.id.as_str()) && edge.curve() == Some(&curve.id)
                }))
        {
            losses.push(
                crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                    "standalone curve {} has no OCCT encoding and is omitted",
                    curve.id
                )),
            );
        }
    }
    for surface in ir
        .model
        .surfaces
        .iter()
        .filter(|surface| reachable.surfaces.contains(surface.id.as_str()))
    {
        if geometry::surface_record(ir, surface).is_none() {
            losses.push(
                crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                    "standalone surface {} has no OCCT encoding and is omitted",
                    surface.id
                )),
            );
        }
    }
    for pcurve in ir.model.pcurves.iter().filter(|pcurve| {
        !ir.model
            .coedges
            .iter()
            .any(|coedge| coedge.pcurves.iter().any(|usage| usage.pcurve == pcurve.id))
    }) {
        losses.push(
            crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                "unbound parameter-space curve {} is omitted",
                pcurve.id
            )),
        );
    }
    let bytes = serialize_inner(ir, &[], true, payload_owners, None)?;
    if bytes.is_none()
        && (!reachable.faces.is_empty()
            || !reachable.edges.is_empty()
            || !reachable.vertices.is_empty())
    {
        losses.push(
            crate::loss::FreecadLossCode::ExportGeometryNotTransferred
                .note("independent topology has an unsupported required carrier and is omitted"),
        );
    }
    Ok(bytes)
}

fn serialize_inner(
    ir: &CadIr,
    bodies: &[&Body],
    free: bool,
    payload_owners: &std::collections::HashSet<String>,
    payload: Option<&str>,
) -> Result<Option<Serialization>, CodecError> {
    let row_owners = ir
        .native
        .namespace("fcstd")
        .map(crate::brep::records::row_owners)
        .transpose()?
        .unwrap_or_default();
    let rooted = Reach::bodies(ir, bodies, &row_owners);
    let mut reachable = if let Some(payload) = payload {
        Reach::payload(ir, bodies, payload, &row_owners)
    } else if free {
        Reach::unowned(ir, &row_owners)
    } else {
        Reach::bodies(ir, bodies, &row_owners)
    };
    if free && payload.is_none() {
        reachable.curves.retain(|id| {
            !ir.model.curves.iter().any(|value| {
                value.id.as_str() == id
                    && value.native_ref.as_ref().is_some_and(|owner| {
                        payload_owners.contains(row_owners.get(owner).unwrap_or(owner))
                    })
            })
        });
        reachable.surfaces.retain(|id| {
            !ir.model.surfaces.iter().any(|value| {
                value.id.as_str() == id
                    && value.native_ref.as_ref().is_some_and(|owner| {
                        payload_owners.contains(row_owners.get(owner).unwrap_or(owner))
                    })
            })
        });
    }
    let kernel_entries = ir
        .native
        .namespace("fcstd")
        .map(|namespace| namespace.arena_as::<crate::native::EntryRecord>("entries"))
        .transpose()?
        .unwrap_or_default();
    let kernel_point = |point: &cadmpeg_ir::topology::Point| {
        point.native_ref.as_deref().is_some_and(|native| {
            kernel_entries.iter().any(|entry| {
                entry.id() == native
                    && entry.contents().iter().any(|contents| {
                        matches!(
                            contents,
                            crate::native::entry_contents::EntryContents::Points { .. }
                        )
                    })
            })
        })
    };
    let Reach {
        faces,
        loops,
        coedges,
        edges,
        vertices,
        curves: curves_needed,
        surfaces: surfaces_needed,
        pcurves: _pcurves_needed,
    } = reachable;
    let mut locations = Vec::new();
    let mut curves = Vec::new();
    let mut curve_ids = HashMap::new();
    let mut curve_slots = HashMap::new();
    let mut polygons = Vec::new();
    let mut polygon_ids = HashMap::new();
    let mut polygon_homes = HashMap::<String, Vec<usize>>::new();
    let native_polygons = ir
        .native
        .namespace("fcstd")
        .and_then(|namespace| namespace.arenas().get("polygons3d"));
    for curve in ir
        .model
        .curves
        .iter()
        .filter(|curve| curves_needed.contains(curve.id.as_str()))
    {
        if let Some((row, location)) = geometry::located_curve(ir, curve) {
            if !location_supported(location)
                || (free
                    && location != cadmpeg_ir::transform::Transform::identity()
                    && !ir.model.edges.iter().any(|edge| {
                        edges.contains(edge.id.as_str()) && edge.curve() == Some(&curve.id)
                    }))
            {
                continue;
            }
            let key = geometry::curve_layout_key(ir, curve, &curves_needed);
            let index = *curve_slots.entry(key).or_insert_with(|| {
                curves.push(row);
                curves.len()
            });
            curve_ids.insert(
                curve.id.as_str(),
                (index, register_location(location, &mut locations)),
            );
        } else if let Some(row) = facets::polyline(curve) {
            polygons.push(row);
            polygon_ids.insert(curve.id.as_str(), polygons.len());
            if let Some(home) = curve.native_ref.as_ref().filter(|home| {
                native_polygons.is_some_and(|rows| rows.iter().any(|row| row.id() == home.as_str()))
            }) {
                polygon_homes
                    .entry(home.clone())
                    .or_default()
                    .push(polygons.len());
            }
        }
    }
    let mut surfaces = Vec::new();
    let mut surface_ids = HashMap::new();
    let mut surface_slots = HashMap::new();
    let trimmed_supports = trimmed_supports(ir, &surfaces_needed);
    let mut support_charts = Vec::new();
    for surface in ir
        .model
        .surfaces
        .iter()
        .filter(|surface| surfaces_needed.contains(surface.id.as_str()))
    {
        if let Some((row, location)) = geometry::located_surface(ir, surface) {
            if let Some(trimmed) = trimmed_supports.get(surface.id.as_str()) {
                if location == cadmpeg_ir::transform::Transform::identity() {
                    support_charts.push((surface.id.as_str(), *trimmed, row));
                    continue;
                }
            }
            if !location_supported(location)
                || (free
                    && location != cadmpeg_ir::transform::Transform::identity()
                    && !ir
                        .model
                        .faces
                        .iter()
                        .any(|face| faces.contains(face.id.as_str()) && face.surface == surface.id))
            {
                continue;
            }
            let affine = crate::brep::generated_surface_parameters(&row)?;
            let Some(chart) = cadmpeg_ir::transform::Transform2::affine([
                [1.0 / affine.u_scale, 0.0, -affine.u_offset / affine.u_scale],
                [0.0, 1.0 / affine.v_scale, -affine.v_offset / affine.v_scale],
            ]) else {
                continue;
            };
            let key = geometry::surface_layout_key(ir, surface, &surfaces_needed);
            let index = *surface_slots.entry(key).or_insert_with(|| {
                surfaces.push(row);
                surfaces.len()
            });
            surface_ids.insert(
                surface.id.as_str(),
                (index, register_location(location, &mut locations), chart),
            );
        }
    }
    // A face on the support of a trimmed surface uses the trimmed row, which
    // keeps the support's parameters.
    for (support, trimmed, row) in support_charts {
        let affine = crate::brep::generated_surface_parameters(&row)?;
        let (index, location) = match surface_ids.get(trimmed) {
            Some(&(index, location, _)) => (index, location),
            None => {
                surfaces.push(row);
                (
                    surfaces.len(),
                    register_location(cadmpeg_ir::transform::Transform::identity(), &mut locations),
                )
            }
        };
        let Some(chart) = cadmpeg_ir::transform::Transform2::affine([
            [1.0 / affine.u_scale, 0.0, -affine.u_offset / affine.u_scale],
            [0.0, 1.0 / affine.v_scale, -affine.v_offset / affine.v_scale],
        ]) else {
            continue;
        };
        surface_ids.insert(support, (index, location, chart));
    }
    let mut facet_faces = facets::faces(ir, &faces)?;
    if let Some(payload) = payload {
        for mesh in ir
            .model
            .tessellations
            .iter()
            .filter(|mesh| mesh.native_ref.as_deref() == Some(payload))
        {
            if !facet_faces.emitted.contains(mesh.id.as_str()) {
                if let Some(row) = facets::mesh_record(mesh, None)? {
                    facet_faces.rows.push(row);
                    facet_faces.emitted.insert(mesh.id.to_string());
                }
            }
        }
    }
    let mut pcurves = Vec::new();
    let mut pcurve_ids = HashMap::new();
    let mut shared_pcurves = HashMap::new();
    for coedge in ir
        .model
        .coedges
        .iter()
        .filter(|coedge| coedges.contains(coedge.id.as_str()))
    {
        let Some(face) = ir
            .model
            .loops
            .iter()
            .find(|wire| wire.id == coedge.owner_loop)
            .and_then(|wire| ir.model.faces.iter().find(|face| face.id == wire.face))
        else {
            continue;
        };
        let Some((_, _, chart)) = surface_ids.get(face.surface.as_str()) else {
            continue;
        };
        for usage in &coedge.pcurves {
            let Some(pcurve) = ir
                .model
                .pcurves
                .iter()
                .find(|pcurve| pcurve.id == usage.pcurve)
            else {
                continue;
            };
            if pcurve_ids.contains_key(&(pcurve.id.as_str(), face.surface.as_str())) {
                continue;
            }
            let domain = pcurve
                .parameter_range()
                .map(cadmpeg_ir::units::FiniteVector::get)
                .or_else(|| {
                    usage
                        .parameter_range
                        .map(cadmpeg_ir::geometry::DirectedParameterRange::endpoints)
                });
            if let Some(row) = super::pcurves::serialize(&pcurve.geometry, domain, *chart) {
                let index =
                    if let Some(index) = shared_pcurves.get(&(pcurve.id.as_str(), row.clone())) {
                        *index
                    } else {
                        pcurves.push(row.clone());
                        shared_pcurves.insert((pcurve.id.as_str(), row), pcurves.len());
                        pcurves.len()
                    };
                pcurve_ids.insert((pcurve.id.as_str(), face.surface.as_str()), index);
            }
        }
    }
    let mut shapes = Vec::new();
    let mut indices = HashMap::new();
    // OCCT carries a numeric tolerance. Zero preserves the absence of an
    // entity-specific positive tolerance instead of inventing document defaults.
    let tolerance = ir.tolerances.linear.get();
    for vertex in ir
        .model
        .vertices
        .iter()
        .filter(|vertex| vertices.contains(vertex.id.as_str()))
    {
        let point = ir
            .model
            .points
            .iter()
            .find(|point| point.id == vertex.point)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "vertex {} references absent point {}",
                    vertex.id, vertex.point
                ))
            })?;
        indices.insert(vertex.id.as_str(), shapes.len());
        shapes.push(Shape {
            kind: "Ve",
            geometry: format!(
                "{} {} 0 0",
                vertex
                    .tolerance
                    .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get),
                geometry::point(point.position().get())
            ),
            children: Vec::new(),
        });
    }
    if free {
        for point in ir.model.points.iter().filter(|point| {
            !kernel_point(point)
                && payload.is_none_or(|payload| point.native_ref.as_deref() == Some(payload))
                && !ir
                    .model
                    .vertices
                    .iter()
                    .any(|vertex| vertex.point == point.id)
        }) {
            indices.insert(point.id.as_str(), shapes.len());
            shapes.push(Shape {
                kind: "Ve",
                geometry: format!(
                    "{tolerance} {} 0 0",
                    geometry::point(point.position().get())
                ),
                children: Vec::new(),
            });
        }
    }
    for edge in ir
        .model
        .edges
        .iter()
        .filter(|edge| edges.contains(edge.id.as_str()))
    {
        let Some(start) = indices.get(edge.start.as_str()).copied() else {
            return Err(CodecError::malformed(format_args!(
                "edge {} references absent start vertex",
                edge.id
            )));
        };
        let Some(end) = indices.get(edge.end.as_str()).copied() else {
            return Err(CodecError::malformed(format_args!(
                "edge {} references absent end vertex",
                edge.id
            )));
        };
        let degenerate = edge.curve().is_none()
            || edge
                .curve()
                .and_then(|id| ir.model.curves.iter().find(|curve| curve.id == *id))
                .and_then(|curve| curve.geometry.solved())
                .is_some_and(|curve| {
                    matches!(
                        curve,
                        cadmpeg_ir::geometry::SolvedCurveGeometry::Degenerate(_)
                    )
                });
        let mut representation = if degenerate {
            String::new()
        } else {
            match edge.curve() {
                Some(curve) if polygon_ids.contains_key(curve.as_str()) => {
                    format!("5 {} 0", polygon_ids[curve.as_str()])
                }
                Some(curve) => {
                    let Some((curve, location)) = curve_ids.get(curve.as_str()) else {
                        continue;
                    };
                    let Some(range) = edge.param_range() else {
                        continue;
                    };
                    let geometry = edge
                        .curve()
                        .and_then(|id| ir.model.curves.iter().find(|curve| curve.id == *id))
                        .and_then(|curve| curve.geometry.solved());
                    let Some(range) = occt_curve_range(geometry, range.get()) else {
                        continue;
                    };
                    format!("1 {curve} {location} {} {}", range[0], range[1])
                }
                None if start == end => String::new(),
                None => continue,
            }
        };
        let mut complete = true;
        let mut seen = std::collections::HashSet::new();
        let mut pcurve_groups =
            std::collections::BTreeMap::<(usize, usize), Vec<(usize, [f64; 2], Sense)>>::new();
        for coedge in ir
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.edge == edge.id && coedges.contains(coedge.id.as_str()))
        {
            let face = ir
                .model
                .loops
                .iter()
                .find(|wire| wire.id == coedge.owner_loop)
                .and_then(|wire| ir.model.faces.iter().find(|face| face.id == wire.face))
                .ok_or_else(|| CodecError::malformed("coedge has an absent loop or face"))?;
            if coedge.pcurves.is_empty() {
                continue;
            }
            let Some((surface, surface_location, _)) = surface_ids.get(face.surface.as_str())
            else {
                complete = false;
                continue;
            };
            for usage in &coedge.pcurves {
                if !seen.insert((face.surface.as_str(), usage.pcurve.as_str())) {
                    continue;
                }
                let Some(curve) = pcurve_ids.get(&(usage.pcurve.as_str(), face.surface.as_str()))
                else {
                    complete = false;
                    continue;
                };
                let range = usage
                    .parameter_range
                    .map(cadmpeg_ir::geometry::DirectedParameterRange::endpoints)
                    .or_else(|| {
                        ir.model
                            .pcurves
                            .iter()
                            .find(|curve| curve.id == usage.pcurve)
                            .and_then(cadmpeg_ir::geometry::pcurve::Pcurve::parameter_range)
                            .map(cadmpeg_ir::units::FiniteVector::get)
                    })
                    .or_else(|| edge.param_range().map(cadmpeg_ir::units::FiniteVector::get));
                let Some(range) = range else {
                    complete = false;
                    continue;
                };
                if range[0] > range[1] {
                    complete = false;
                    continue;
                }
                pcurve_groups
                    .entry((*surface, *surface_location))
                    .or_default()
                    .push((*curve, range, coedge.sense));
            }
        }
        for ((surface, location), mut rows) in pcurve_groups {
            rows.sort_by_key(|(_, _, sense)| *sense == Sense::Reversed);
            match rows.as_slice() {
                [(curve, range, _)] => {
                    representation.push_str(" 2 ");
                    representation.push_str(&curve.to_string());
                    representation.push(' ');
                    representation.push_str(&surface.to_string());
                    representation.push(' ');
                    representation.push_str(&location.to_string());
                    representation.push(' ');
                    representation.push_str(&range[0].to_string());
                    representation.push(' ');
                    representation.push_str(&range[1].to_string());
                }
                [(first, range, _), (second, other_range, _)] if range == other_range => {
                    representation.push_str(" 3 ");
                    representation.push_str(&first.to_string());
                    representation.push(' ');
                    representation.push_str(&second.to_string());
                    representation.push_str(" C0 ");
                    representation.push_str(&surface.to_string());
                    representation.push(' ');
                    representation.push_str(&location.to_string());
                    representation.push(' ');
                    representation.push_str(&range[0].to_string());
                    representation.push(' ');
                    representation.push_str(&range[1].to_string());
                }
                _ => complete = false,
            }
        }
        if !complete {
            continue;
        }
        indices.insert(edge.id.as_str(), shapes.len());
        shapes.push(Shape {
            kind: "Ed",
            geometry: format!(
                "{} 1 1 {} {representation} 0",
                edge.tolerance
                    .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get),
                u8::from(degenerate)
            ),
            children: vec![
                ShapeUse::new(start, Sense::Forward),
                ShapeUse::new(end, Sense::Reversed),
            ],
        });
    }
    for wire in ir
        .model
        .loops
        .iter()
        .filter(|wire| loops.contains(wire.id.as_str()))
    {
        let mut children = Vec::new();
        let mut complete = true;
        for coedge in wire.coedges() {
            let coedge = ir
                .model
                .coedges
                .iter()
                .find(|value| value.id == *coedge)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!("loop {} references absent coedge", wire.id))
                })?;
            if let Some(index) = indices.get(coedge.edge.as_str()) {
                children.push(ShapeUse::new(*index, coedge.sense));
            } else {
                complete = false;
            }
        }
        if complete {
            indices.insert(wire.id.as_str(), shapes.len());
            shapes.push(Shape {
                kind: "Wi",
                geometry: String::new(),
                children,
            });
        }
    }
    for face in ir
        .model
        .faces
        .iter()
        .filter(|face| faces.contains(face.id.as_str()))
    {
        if facet_faces.ambiguous.contains(face.id.as_str()) {
            continue;
        }
        let analytic = surface_ids.get(face.surface.as_str());
        let triangulation = facet_faces.indices.get(face.id.as_str());
        if analytic.is_none() && triangulation.is_none() {
            continue;
        }
        let (surface, surface_location) =
            analytic.map_or((0, 0), |(surface, location, _)| (*surface, *location));
        let mut children = Vec::new();
        let mut complete = true;
        for wire in &face.loops {
            if let Some(index) = indices.get(wire.as_str()) {
                // Neutral coedge senses already include the face orientation.
                // Cancel that parent orientation on the wire reference.
                children.push(ShapeUse::new(*index, face.sense));
            } else {
                complete = false;
            }
        }
        if complete {
            indices.insert(face.id.as_str(), shapes.len());
            shapes.push(Shape {
                kind: "Fa",
                geometry: format!(
                    "0 {} {surface} {surface_location}{}",
                    face.tolerance
                        .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get),
                    triangulation.map_or_else(String::new, |index| format!(" 2 {index}"))
                ),
                children,
            });
        }
    }
    let mut roots = Vec::new();
    let mut body_roots = Vec::new();
    for body in bodies {
        let mut regions = Vec::new();
        for region_id in &body.regions {
            let region = ir
                .model
                .regions
                .iter()
                .find(|value| value.id == *region_id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!("body {} references absent region", body.id))
                })?;
            let mut shells = Vec::new();
            for shell_id in &region.shells {
                let shell = ir
                    .model
                    .shells
                    .iter()
                    .find(|value| value.id == *shell_id)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "region {} references absent shell",
                            region.id
                        ))
                    })?;
                let mut children = Vec::new();
                for face_id in shell.faces() {
                    let Some(index) = indices.get(face_id.as_str()) else {
                        return Ok(None);
                    };
                    let face = ir
                        .model
                        .faces
                        .iter()
                        .find(|value| value.id == *face_id)
                        .ok_or_else(|| CodecError::malformed("shell references absent face"))?;
                    children.push(ShapeUse::new(*index, face.sense));
                }
                for edge in shell.wire_edges() {
                    let Some(index) = indices.get(edge.as_str()) else {
                        return Ok(None);
                    };
                    children.push(ShapeUse::new(*index, Sense::Forward));
                }
                for vertex in shell.free_vertices() {
                    let Some(index) = indices.get(vertex.as_str()) else {
                        return Ok(None);
                    };
                    children.push(ShapeUse::new(*index, Sense::Forward));
                }
                let index = shapes.len();
                shapes.push(Shape {
                    kind: if shell.faces().is_empty() { "Co" } else { "Sh" },
                    geometry: String::new(),
                    children,
                });
                if shapes[index].kind == "Sh" {
                    indices.insert(shell.id.as_str(), index);
                }
                shells.push(ShapeUse::new(index, Sense::Forward));
            }
            let index = shapes.len();
            shapes.push(Shape {
                kind: if body.kind == BodyKind::Solid
                    || (body.kind == BodyKind::General
                        && region.shells.iter().all(|id| {
                            ir.model
                                .shells
                                .iter()
                                .any(|shell| shell.id == *id && !shell.faces().is_empty())
                        }))
                {
                    "So"
                } else {
                    "Co"
                },
                geometry: String::new(),
                children: shells,
            });
            if shapes[index].kind == "So" {
                indices.insert(region.id.as_str(), index);
            }
            regions.push(ShapeUse::new(index, Sense::Forward));
        }
        if regions.is_empty() {
            return Ok(None);
        }
        let root = if regions.len() == 1 {
            let region = regions[0].index;
            if body.kind == BodyKind::Sheet && shapes[region].children.len() == 1 {
                shapes[region].children[0].index
            } else if body.kind == BodyKind::Wire {
                let children = shapes[region]
                    .children
                    .iter()
                    .flat_map(|usage| shapes[usage.index].children.clone())
                    .collect();
                let root = shapes.len();
                shapes.push(Shape {
                    kind: "Wi",
                    geometry: String::new(),
                    children,
                });
                root
            } else if body.kind == BodyKind::General && shapes[region].kind == "So" {
                let root = shapes.len();
                shapes.push(Shape {
                    kind: "Co",
                    geometry: String::new(),
                    children: vec![ShapeUse::new(region, Sense::Forward)],
                });
                root
            } else {
                region
            }
        } else {
            let root = shapes.len();
            shapes.push(Shape {
                kind: "Co",
                geometry: String::new(),
                children: regions,
            });
            root
        };
        if shapes[root].kind == "Co" {
            indices.insert(body.id.as_str(), root);
        }
        if shapes[root].kind == "Wi" && body.regions.len() == 1 {
            if let Some(region) = ir
                .model
                .regions
                .iter()
                .find(|value| value.id == body.regions[0])
            {
                if region.shells.len() == 1 {
                    indices.insert(region.shells[0].as_str(), root);
                }
            }
        }
        body_roots.push((
            root,
            body.id.as_str().to_owned(),
            body.transform.unwrap_or_default(),
        ));
        let location = register_location(body.transform.unwrap_or_default(), &mut locations);
        roots.push(ShapeUse {
            index: root,
            sense: Sense::Forward,
            location,
        });
    }
    if free {
        for point in ir.model.points.iter().filter(|point| {
            !kernel_point(point)
                && payload.is_none_or(|payload| point.native_ref.as_deref() == Some(payload))
                && !ir
                    .model
                    .vertices
                    .iter()
                    .any(|vertex| vertex.point == point.id)
        }) {
            if let Some(index) = indices.get(point.id.as_str()) {
                roots.push(ShapeUse::new(*index, Sense::Forward));
            }
        }
        for face in ir.model.faces.iter().filter(|face| {
            faces.contains(face.id.as_str()) && !rooted.faces.contains(face.id.as_str())
        }) {
            let Some(index) = indices.get(face.id.as_str()) else {
                return Ok(None);
            };
            roots.push(ShapeUse::new(*index, Sense::Forward));
        }
        for wire in ir.model.loops.iter().filter(|wire| {
            loops.contains(wire.id.as_str())
                && !rooted.loops.contains(wire.id.as_str())
                && !ir.model.faces.iter().any(|face| {
                    faces.contains(face.id.as_str()) && face.loops.iter().any(|id| id == &wire.id)
                })
        }) {
            let Some(index) = indices.get(wire.id.as_str()) else {
                return Ok(None);
            };
            roots.push(ShapeUse::new(*index, Sense::Forward));
        }
        for edge in
            ir.model.edges.iter().filter(|edge| {
                edges.contains(edge.id.as_str())
                    && !rooted.edges.contains(edge.id.as_str())
                    && !ir.model.coedges.iter().any(|coedge| {
                        coedges.contains(coedge.id.as_str()) && coedge.edge == edge.id
                    })
            })
        {
            let Some(index) = indices.get(edge.id.as_str()) else {
                return Ok(None);
            };
            roots.push(ShapeUse::new(*index, Sense::Forward));
        }
        for vertex in ir.model.vertices.iter().filter(|vertex| {
            vertices.contains(vertex.id.as_str())
                && !rooted.vertices.contains(vertex.id.as_str())
                && !ir.model.edges.iter().any(|edge| {
                    edges.contains(edge.id.as_str())
                        && (edge.start == vertex.id || edge.end == vertex.id)
                })
        }) {
            let Some(index) = indices.get(vertex.id.as_str()) else {
                return Ok(None);
            };
            roots.push(ShapeUse::new(*index, Sense::Forward));
        }
        if roots.is_empty()
            && curves.is_empty()
            && surfaces.is_empty()
            && polygons.is_empty()
            && facet_faces.rows.is_empty()
        {
            return Ok(None);
        }
    }
    let uses = indices
        .iter()
        .map(|(id, index)| (*id, ShapeUse::new(*index, Sense::Forward)))
        .collect();
    let location_rows = locations.iter().map(location_row).collect::<Vec<_>>();
    let mut output = format!("\nCASCADE Topology V3, (c) Open Cascade\nLocations {}\n{}\nCurve2ds {}\n{}\nCurves {}\n{}\nPolygon3D {}\n{}\nPolygonOnTriangulations 0\nSurfaces {}\n{}\nTriangulations {}\n{}\nTShapes {}\n", locations.len(), location_rows.join("\n"), pcurves.len(), pcurves.join("\n"), curves.len(), curves.join("\n"), polygons.len(), polygons.join("\n"), surfaces.len(), surfaces.join("\n"), facet_faces.rows.len(), facet_faces.rows.join("\n"), shapes.len());
    for shape in &shapes {
        output.push_str(shape.kind);
        output.push('\n');
        output.push_str(&shape.geometry);
        output.push_str("\n\n0101101\n");
        for usage in &shape.children {
            output.push(if usage.sense == Sense::Forward {
                '+'
            } else {
                '-'
            });
            output.push_str(&(shapes.len() - usage.index).to_string());
            output.push(' ');
            output.push_str(&usage.location.to_string());
            output.push(' ');
        }
        output.push_str("*\n");
    }
    if roots.is_empty() {
        output.push_str("*\n");
    }
    let topology = occurrences::emitted(ir, &shapes, &uses, &body_roots, &roots, &locations)?;
    for usage in roots {
        output.push(if usage.sense == Sense::Forward {
            '+'
        } else {
            '-'
        });
        output.push_str(&(shapes.len() - usage.index).to_string());
        output.push(' ');
        output.push_str(&usage.location.to_string());
        output.push('\n');
    }
    Ok(Some(Serialization {
        bytes: output.into_bytes(),
        meshes: facet_faces.emitted,
        topology,
        polygon_homes,
    }))
}

/// Surfaces that are the inline support of a needed trimmed surface, keyed by
/// support id.
fn trimmed_supports<'a>(
    ir: &'a CadIr,
    needed: &std::collections::HashSet<String>,
) -> HashMap<&'a str, &'a str> {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;
    let mut supports = HashMap::new();
    for surface in ir
        .model
        .surfaces
        .iter()
        .filter(|surface| needed.contains(surface.id.as_str()))
    {
        let Some(construction) = surface.geometry.procedural_construction() else {
            continue;
        };
        let Some(ProceduralSurfaceDefinition::Subset(subset)) = ir
            .model
            .procedural_surfaces
            .iter()
            .find(|record| record.id == *construction)
            .map(|record| record.definition())
        else {
            continue;
        };
        if geometry::surface_record(ir, surface).is_some_and(|row| row.starts_with("10 "))
            && needed.contains(subset.support().as_str())
        {
            supports
                .entry(subset.support().as_str())
                .or_insert(surface.id.as_str());
        }
    }
    supports
}

/// Map a neutral curve parameter range to the OCCT parameterization that
/// `topology_transfer::normalize_occt_curve_range` reads.
fn occt_curve_range(
    curve: Option<&cadmpeg_ir::geometry::SolvedCurveGeometry>,
    range: [f64; 2],
) -> Option<[f64; 2]> {
    use cadmpeg_ir::geometry::SolvedCurveGeometry;
    use cadmpeg_ir::scalar::FiniteReal;
    match curve {
        Some(SolvedCurveGeometry::Parabola(parabola)) => {
            let focal_distance = parabola.focal_distance().magnitude();
            let [start, end] = range.map(FiniteReal::new);
            Some([
                cadmpeg_ir::math::multiply_divide(start?, focal_distance, FiniteReal::HALF)?.get(),
                cadmpeg_ir::math::multiply_divide(end?, focal_distance, FiniteReal::HALF)?.get(),
            ])
        }
        Some(SolvedCurveGeometry::Transformed(placed)) => {
            occt_curve_range(Some(placed.basis()), range)
        }
        _ => Some(range),
    }
}

fn register_location(
    transform: cadmpeg_ir::transform::Transform,
    locations: &mut Vec<cadmpeg_ir::transform::Transform>,
) -> usize {
    if transform == cadmpeg_ir::transform::Transform::identity() {
        return 0;
    }
    if let Some(index) = locations.iter().position(|existing| *existing == transform) {
        return index + 1;
    }
    locations.push(transform);
    locations.len()
}

fn location_row(transform: &cadmpeg_ir::transform::Transform) -> String {
    format!(
        "1\n{}",
        transform
            .affine_rows()
            .iter()
            .map(|row| row
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" "))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

/// OCCT Locations hold similarities, not arbitrary affine deformations.
fn location_supported(transform: cadmpeg_ir::transform::Transform) -> bool {
    let rows = transform.affine_rows();
    let dot = |a: usize, b: usize| (0..3).map(|row| rows[row][a] * rows[row][b]).sum::<f64>();
    let scale = dot(0, 0);
    scale.is_finite()
        && scale > 0.0
        && (1..3)
            .all(|axis| (dot(axis, axis) - scale).abs() <= LOCATION_SIMILARITY_TOLERANCE * scale)
        && [(0, 1), (0, 2), (1, 2)]
            .into_iter()
            .all(|(a, b)| dot(a, b).abs() <= LOCATION_SIMILARITY_TOLERANCE * scale)
}
