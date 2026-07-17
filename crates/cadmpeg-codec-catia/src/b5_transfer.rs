// SPDX-License-Identifier: Apache-2.0
//! Transfer of reference-closed `b5 03` object topology into neutral IR.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, Pcurve, PcurveGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::{LossCategory, LossCode, LossNote, Severity};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::transfer::{Builder, Transfer};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::b5::{B5Graph, B5Loop, B5Surface};

/// Transfer a complete B5 graph. Returns `false` without mutation when any
/// referenced face, pcurve, edge endpoint, or loop chain remains unresolved.
///
/// Each referenced surface crosses the Phase-4B resolver-to-carrier boundary
/// (§6.2, §10) through a [`Transfer`]: an analytic carrier the decoder can read
/// verbatim resolves [`Exact`](Transfer::Exact), while a plane whose stored
/// axes are not orthonormal or a revolution carrier the transfer cannot express
/// resolves [`Fallback`](Transfer::Fallback) to an opaque `Unknown` surface. An
/// opaque substitute is not a tolerable reduction: the face's mandatory surface
/// geometry was not transferred, so the note carries the
/// [`GeometryNotTransferred`](LossCode::GeometryNotTransferred) code that strict
/// mode refuses. Threading the substitution through a [`Builder`] makes the
/// fallback unwritable without its note, so `losses` gains one entry per
/// substituted carrier. `losses` is left untouched on every `false` return.
pub(crate) fn transfer(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    graph: &B5Graph,
    payload: &UnknownId,
    losses: &mut Vec<LossNote>,
) -> bool {
    let face_record_count = graph
        .records
        .iter()
        .filter(|record| record.class == 0x5f)
        .count();
    if graph.faces.is_empty() || graph.faces.len() != face_record_count {
        return false;
    }

    let referenced_loops: HashSet<u32> = graph
        .faces
        .iter()
        .flat_map(|face| face.loops.iter().copied())
        .collect();
    if referenced_loops.len() != graph.loops.len()
        || graph
            .loops
            .keys()
            .any(|loop_id| !referenced_loops.contains(loop_id))
    {
        return false;
    }

    let referenced_surfaces: HashSet<u32> = graph.faces.iter().map(|face| face.surface).collect();
    let mut surface_plan = BTreeMap::new();
    // Resolve every carrier substitution into a local sink so a later `return
    // false` leaves the caller's `losses` untouched; the sink drains into
    // `losses` only once the graph commits.
    let mut surface_losses: Vec<LossNote> = Vec::new();
    let mut surface_builder = Builder::new(&mut surface_losses);
    for surface_id in referenced_surfaces {
        let Some(surface) = graph.surfaces.get(&surface_id) else {
            return false;
        };
        // A carrier substitution resolves `Fallback` and yields its opaque
        // value; a future `Dropped` branch would yield `None`, which is treated
        // as an unresolved graph (`false`) rather than a panic on hostile input.
        let Some(geometry) = surface_builder.take(neutral_surface(surface, payload, surface_id))
        else {
            return false;
        };
        surface_plan.insert(surface_id, geometry);
    }

    let mut pcurve_plan = BTreeMap::new();
    let mut loop_senses = BTreeMap::new();
    let mut edge_ids = HashSet::new();
    for loop_ in graph.loops.values() {
        if loop_.pcurves.len() != loop_.edges.len() || loop_.pcurves.is_empty() {
            return false;
        }
        if graph
            .faces
            .iter()
            .filter(|face| face.loops.contains(&loop_.object_id))
            .any(|face| face.surface != loop_.surface)
        {
            return false;
        }
        let Some(senses) = solve_loop_chain(loop_, &graph.edge_vertices) else {
            return false;
        };
        loop_senses.insert(loop_.object_id, senses);
        for (&pcurve_id, &edge_id) in loop_.pcurves.iter().zip(&loop_.edges) {
            let Some(pcurve) = graph.pcurves.get(&pcurve_id) else {
                return false;
            };
            if pcurve.surface != loop_.surface || !graph.edge_vertices.contains_key(&edge_id) {
                return false;
            }
            let Some(knots) = expand_knots(&pcurve.distinct_knots, &pcurve.multiplicities) else {
                return false;
            };
            let geometry = PcurveGeometry::Nurbs {
                degree: pcurve.degree,
                knots,
                control_points: pcurve
                    .control_points
                    .iter()
                    .map(|point| Point2::new(point[0], point[1]))
                    .collect(),
                weights: None,
                periodic: false,
            };
            if pcurve_plan.insert(pcurve_id, geometry).is_some() {
                return false;
            }
            edge_ids.insert(edge_id);
        }
    }

    let body_id = BodyId("catia:b5:body#0".to_string());
    let region_id = RegionId("catia:b5:region#0".to_string());
    let shell_id = ShellId("catia:b5:shell#0".to_string());
    let used_vertices: HashSet<usize> = edge_ids
        .iter()
        .flat_map(|edge| graph.edge_vertices[edge])
        .collect();

    for (index, coordinates) in graph.vertex_points.iter().enumerate() {
        let point_id = PointId(format!("catia:b5:point#{index}"));
        annotate(
            annotations,
            &point_id,
            "object_stream_b5_03",
            "05_08_01_vertex",
            Exactness::ByteExact,
        );
        ir.model.points.push(Point {
            id: point_id.clone(),
            position: Point3::new(coordinates[0], coordinates[1], coordinates[2]),
        });
        let vertex_id = VertexId(format!("catia:b5:vertex#{index}"));
        annotate(
            annotations,
            &vertex_id,
            "object_stream_b5_03",
            "05_08_01_vertex",
            Exactness::ByteExact,
        );
        annotations.derived(&vertex_id, "point");
        ir.model.vertices.push(Vertex {
            id: vertex_id,
            point: point_id,
            tolerance: None,
        });
    }

    let mut surface_ids = HashMap::new();
    for (object_id, geometry) in surface_plan {
        let id = SurfaceId(format!("catia:b5:surface#{object_id}"));
        annotate(
            annotations,
            &id,
            "object_stream_b5_03",
            "face_surface",
            if matches!(geometry, SurfaceGeometry::Unknown { .. }) {
                Exactness::Unknown
            } else {
                Exactness::ByteExact
            },
        );
        surface_ids.insert(object_id, id.clone());
        ir.model.surfaces.push(Surface {
            id,
            geometry,
            source_object: None,
        });
    }

    let mut pcurve_ids = HashMap::new();
    for (object_id, geometry) in pcurve_plan {
        let id = PcurveId(format!("catia:b5:pcurve#{object_id}"));
        annotate(
            annotations,
            &id,
            "object_stream_b5_03",
            "21_pcurve",
            Exactness::ByteExact,
        );
        pcurve_ids.insert(object_id, id.clone());
        ir.model.pcurves.push(Pcurve {
            id,
            geometry,
            wrapper_reversed: None,
            parameter_range: None,
            fit_tolerance: None,
            native_tail_flags: None,
        });
    }

    let mut edge_id_map = HashMap::new();
    for edge_id in edge_ids {
        let id = EdgeId(format!("catia:b5:edge#{edge_id}"));
        let curve_id = CurveId(format!("catia:b5:curve#{edge_id}"));
        let endpoints = graph.edge_vertices[&edge_id];
        annotate(
            annotations,
            &curve_id,
            "object_stream_b5_03",
            "pcurve_lifted_3d_curve",
            Exactness::Unknown,
        );
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Unknown {
                record: Some(payload.clone()),
            },
            source_object: None,
        });
        annotate(
            annotations,
            &id,
            "object_stream_b5_03",
            "5e_edge",
            Exactness::ByteExact,
        );
        annotations.derived(&id, "start").derived(&id, "end");
        edge_id_map.insert(edge_id, id.clone());
        ir.model.edges.push(Edge {
            id,
            curve: Some(curve_id),
            start: VertexId(format!("catia:b5:vertex#{}", endpoints[0])),
            end: VertexId(format!("catia:b5:vertex#{}", endpoints[1])),
            param_range: None,
            tolerance: None,
        });
    }

    annotate(
        annotations,
        &body_id,
        "object_stream_b5_03",
        "single_body",
        Exactness::Inferred,
    );
    annotations
        .derived(&body_id, "kind")
        .derived(&body_id, "regions");
    ir.model.bodies.push(Body {
        id: body_id.clone(),
        kind: body_kind(graph),
        regions: vec![region_id.clone()],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    annotate(
        annotations,
        &region_id,
        "object_stream_b5_03",
        "derived_region",
        Exactness::Inferred,
    );
    annotations
        .derived(&region_id, "body")
        .derived(&region_id, "shells");
    ir.model.regions.push(Region {
        id: region_id.clone(),
        body: body_id,
        shells: vec![shell_id.clone()],
    });
    annotate(
        annotations,
        &shell_id,
        "object_stream_b5_03",
        "derived_shell",
        Exactness::Inferred,
    );
    annotations
        .derived(&shell_id, "region")
        .derived(&shell_id, "faces")
        .derived(&shell_id, "free_vertices");
    ir.model.shells.push(Shell {
        id: shell_id.clone(),
        region: region_id,
        faces: graph
            .faces
            .iter()
            .map(|face| FaceId(format!("catia:b5:face#{}", face.object_id)))
            .collect(),
        wire_edges: Vec::new(),
        free_vertices: graph
            .vertex_points
            .iter()
            .enumerate()
            .filter(|(index, _)| !used_vertices.contains(index))
            .map(|(index, _)| VertexId(format!("catia:b5:vertex#{index}")))
            .collect(),
    });

    let mut coedges_by_edge = HashMap::<u32, Vec<usize>>::new();
    for face in &graph.faces {
        let face_id = FaceId(format!("catia:b5:face#{}", face.object_id));
        annotate(
            annotations,
            &face_id,
            "object_stream_b5_03",
            "5f_face",
            Exactness::Inferred,
        );
        annotations
            .derived(&face_id, "shell")
            .derived(&face_id, "surface")
            .derived(&face_id, "sense")
            .derived(&face_id, "loops");
        ir.model.faces.push(Face {
            id: face_id.clone(),
            shell: shell_id.clone(),
            surface: surface_ids[&face.surface].clone(),
            sense: Sense::Forward,
            loops: face
                .loops
                .iter()
                .map(|loop_id| LoopId(format!("catia:b5:loop#{loop_id}")))
                .collect(),
            name: None,
            color: None,
            tolerance: None,
        });
        for loop_id_value in &face.loops {
            let loop_ = &graph.loops[loop_id_value];
            let senses = &loop_senses[loop_id_value];
            let loop_id = LoopId(format!("catia:b5:loop#{loop_id_value}"));
            let coedge_ids: Vec<CoedgeId> = (0..loop_.edges.len())
                .map(|index| CoedgeId(format!("catia:b5:coedge#{loop_id_value}-{index}")))
                .collect();
            annotate(
                annotations,
                &loop_id,
                "object_stream_b5_03",
                "62_loop",
                Exactness::ByteExact,
            );
            annotations
                .derived(&loop_id, "face")
                .derived(&loop_id, "coedges");
            ir.model.loops.push(Loop {
                id: loop_id.clone(),
                face: face_id.clone(),
                coedges: coedge_ids.clone(),
            });
            for (index, ((&edge, &pcurve), &reversed)) in loop_
                .edges
                .iter()
                .zip(&loop_.pcurves)
                .zip(senses)
                .enumerate()
            {
                let id = coedge_ids[index].clone();
                annotate(
                    annotations,
                    &id,
                    "object_stream_b5_03",
                    "serialized_loop_member",
                    Exactness::ByteExact,
                );
                for field in [
                    "owner_loop",
                    "edge",
                    "next",
                    "previous",
                    "radial_next",
                    "sense",
                    "pcurve",
                ] {
                    annotations.derived(&id, field);
                }
                let arena_index = ir.model.coedges.len();
                coedges_by_edge.entry(edge).or_default().push(arena_index);
                ir.model.coedges.push(Coedge {
                    id: id.clone(),
                    owner_loop: loop_id.clone(),
                    edge: edge_id_map[&edge].clone(),
                    next: coedge_ids[(index + 1) % coedge_ids.len()].clone(),
                    previous: coedge_ids[(index + coedge_ids.len() - 1) % coedge_ids.len()].clone(),
                    radial_next: id,
                    sense: if reversed {
                        Sense::Reversed
                    } else {
                        Sense::Forward
                    },
                    pcurve: Some(pcurve_ids[&pcurve].clone()),
                });
            }
        }
    }
    for occurrences in coedges_by_edge.values() {
        for (position, &arena_index) in occurrences.iter().enumerate() {
            let radial = occurrences[(position + 1) % occurrences.len()];
            ir.model.coedges[arena_index].radial_next = ir.model.coedges[radial].id.clone();
        }
    }
    losses.append(&mut surface_losses);
    true
}

/// Resolve one B5 carrier across the Phase-4B resolver boundary. A readable
/// analytic carrier is [`Exact`](Transfer::Exact); a plane whose stored axes are
/// not orthonormal, or a revolution the transfer cannot express, substitutes an
/// opaque `Unknown` carrier. That substitution drops the face's mandatory
/// surface geometry irreversibly, so it carries the
/// [`GeometryNotTransferred`](LossCode::GeometryNotTransferred) code strict mode
/// refuses, not a tolerable-reduction code.
fn neutral_surface(
    surface: &B5Surface,
    payload: &UnknownId,
    surface_id: u32,
) -> Transfer<SurfaceGeometry> {
    match surface {
        B5Surface::Plane {
            origin,
            direction_u,
            direction_v,
        } => match orthonormal_plane(*origin, *direction_u, *direction_v) {
            Some(plane) => Transfer::exact(plane),
            None => Transfer::fallback(
                SurfaceGeometry::Unknown {
                    record: Some(payload.clone()),
                },
                carrier_loss(
                    LossCode::GeometryNotTransferred,
                    format!(
                        "B5 plane carrier #{surface_id} stores non-orthonormal axes; its analytic \
                         surface definition was not transferred and the face carries an opaque \
                         carrier."
                    ),
                ),
            ),
        },
        B5Surface::Cylinder {
            origin,
            reference_x,
            axis,
            radius,
        } => Transfer::exact(SurfaceGeometry::Cylinder {
            origin: point(*origin),
            axis: vector(*axis),
            ref_direction: vector(*reference_x),
            radius: *radius,
        }),
        B5Surface::Nurbs(surface) => Transfer::exact(SurfaceGeometry::Nurbs(surface.clone())),
        B5Surface::Revolution { .. } => Transfer::fallback(
            SurfaceGeometry::Unknown {
                record: Some(payload.clone()),
            },
            carrier_loss(
                LossCode::GeometryNotTransferred,
                format!(
                    "B5 revolution carrier #{surface_id} was replaced by an opaque carrier; its \
                     procedural surface definition was not transferred."
                ),
            ),
        ),
    }
}

fn carrier_loss(code: LossCode, message: String) -> LossNote {
    LossNote {
        code,
        category: LossCategory::Geometry,
        severity: Severity::Warning,
        message,
        provenance: None,
    }
}

fn orthonormal_plane(
    origin: [f64; 3],
    direction_u: [f64; 3],
    direction_v: [f64; 3],
) -> Option<SurfaceGeometry> {
    let u = unit(direction_u)?;
    let v = unit(direction_v)?;
    if (length(direction_u) - 1.0).abs() > 1e-9
        || (length(direction_v) - 1.0).abs() > 1e-9
        || dot(u, v).abs() > 1e-9
    {
        return None;
    }
    Some(SurfaceGeometry::Plane {
        origin: point(origin),
        normal: vector(unit(cross(u, v))?),
        u_axis: vector(u),
    })
}

fn solve_loop_chain(
    loop_: &B5Loop,
    edge_vertices: &BTreeMap<u32, [usize; 2]>,
) -> Option<Vec<bool>> {
    let first = edge_vertices.get(loop_.edges.first()?)?;
    let mut solutions = Vec::new();
    for first_reversed in [false, true] {
        let initial = first[usize::from(first_reversed)];
        let mut current = first[usize::from(!first_reversed)];
        let mut senses = vec![first_reversed];
        for edge_id in &loop_.edges[1..] {
            let endpoints = edge_vertices.get(edge_id)?;
            match (endpoints[0] == current, endpoints[1] == current) {
                (true, false) => {
                    senses.push(false);
                    current = endpoints[1];
                }
                (false, true) => {
                    senses.push(true);
                    current = endpoints[0];
                }
                _ => {
                    senses.clear();
                    break;
                }
            }
        }
        if !senses.is_empty() && current == initial {
            solutions.push(senses);
        }
    }
    (solutions.len() == 1).then(|| solutions.remove(0))
}

fn expand_knots(distinct: &[f64], multiplicities: &[u32]) -> Option<Vec<f64>> {
    if distinct.len() != multiplicities.len() {
        return None;
    }
    let mut knots = Vec::new();
    for (&knot, &multiplicity) in distinct.iter().zip(multiplicities) {
        knots.extend(std::iter::repeat_n(
            knot,
            usize::try_from(multiplicity).ok()?,
        ));
    }
    Some(knots)
}

fn body_kind(graph: &B5Graph) -> BodyKind {
    let mut uses = HashMap::<u32, usize>::new();
    for edge in graph.loops.values().flat_map(|loop_| &loop_.edges) {
        *uses.entry(*edge).or_default() += 1;
    }
    if uses.values().any(|count| *count > 2) {
        BodyKind::General
    } else if !uses.is_empty() && uses.values().all(|count| *count == 2) {
        BodyKind::Solid
    } else {
        BodyKind::Sheet
    }
}

fn annotate(
    annotations: &mut AnnotationBuilder,
    id: impl std::fmt::Display,
    stream: &str,
    tag: &str,
    exactness: Exactness,
) {
    let id = id.to_string();
    let stream = annotations.stream(format!("catia:{stream}"));
    annotations.note(&id, stream, 0).tag(tag);
    annotations.exactness(id, exactness);
}

fn point(value: [f64; 3]) -> Point3 {
    Point3::new(value[0], value[1], value[2])
}

fn vector(value: [f64; 3]) -> Vector3 {
    Vector3::new(value[0], value[1], value[2])
}

fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn length(value: [f64; 3]) -> f64 {
    dot(value, value).sqrt()
}

fn unit(value: [f64; 3]) -> Option<[f64; 3]> {
    let length = length(value);
    (length > f64::EPSILON).then(|| [value[0] / length, value[1] / length, value[2] / length])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> UnknownId {
        UnknownId("catia:payload:unknown#test".to_string())
    }

    #[test]
    fn orthonormal_plane_resolves_exact_without_loss() {
        let mut sink: Vec<LossNote> = Vec::new();
        let surface = B5Surface::Plane {
            origin: [0.0, 0.0, 0.0],
            direction_u: [1.0, 0.0, 0.0],
            direction_v: [0.0, 1.0, 0.0],
        };
        let value = neutral_surface(&surface, &payload(), 7).resolve(&mut sink);
        assert!(matches!(value, Some(SurfaceGeometry::Plane { .. })));
        assert!(sink.is_empty());
    }

    #[test]
    fn non_orthonormal_plane_falls_back_and_rejects_in_strict() {
        use cadmpeg_ir::report::StrictConsequence;
        let mut sink: Vec<LossNote> = Vec::new();
        let surface = B5Surface::Plane {
            origin: [0.0, 0.0, 0.0],
            direction_u: [2.0, 0.0, 0.0],
            direction_v: [0.0, 1.0, 0.0],
        };
        let value = neutral_surface(&surface, &payload(), 7).resolve(&mut sink);
        assert!(matches!(value, Some(SurfaceGeometry::Unknown { .. })));
        assert_eq!(sink.len(), 1);
        assert_eq!(sink[0].code, LossCode::GeometryNotTransferred);
        // The opaque substitution is an irreversible geometry drop strict mode
        // refuses, not a tolerable reduction.
        assert_eq!(sink[0].code.strict_consequence(), StrictConsequence::Reject);
    }

    #[test]
    fn revolution_falls_back_and_rejects_in_strict() {
        use cadmpeg_ir::report::StrictConsequence;
        let mut sink: Vec<LossNote> = Vec::new();
        let surface = B5Surface::Revolution {
            profile_curve: 3,
            axis_origin: [0.0, 0.0, 0.0],
            axis_direction: [0.0, 0.0, 1.0],
            gauge_radius: 1.0,
        };
        let value = neutral_surface(&surface, &payload(), 9).resolve(&mut sink);
        assert!(matches!(value, Some(SurfaceGeometry::Unknown { .. })));
        assert_eq!(sink.len(), 1);
        assert_eq!(sink[0].code, LossCode::GeometryNotTransferred);
        assert_eq!(sink[0].code.strict_consequence(), StrictConsequence::Reject);
    }

    #[test]
    fn cylinder_resolves_exact_without_loss() {
        let mut sink: Vec<LossNote> = Vec::new();
        let surface = B5Surface::Cylinder {
            origin: [0.0, 0.0, 0.0],
            reference_x: [1.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: 2.0,
        };
        let value = neutral_surface(&surface, &payload(), 1).resolve(&mut sink);
        assert!(matches!(value, Some(SurfaceGeometry::Cylinder { radius, .. }) if radius == 2.0));
        assert!(sink.is_empty());
    }
}
