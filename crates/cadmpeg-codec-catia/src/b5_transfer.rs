// SPDX-License-Identifier: Apache-2.0
//! Transfer of reference-closed `b5 03` object topology into neutral IR.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::{curve_point, pcurve_uv, surface_point};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, NurbsCurve, NurbsSurface,
    Pcurve, PcurveGeometry, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, Surface, SurfaceCurveFamily, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
    ProceduralSurfaceId, RegionId, ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::b5::{B5Graph, B5Loop, B5Pcurve, B5Profile, B5Surface};

const POINT_TOLERANCE: f64 = 1.5e-3;

struct RevolutionPlan {
    directrix: NurbsCurve,
    axis_origin: Point3,
    axis_direction: Vector3,
    angular_interval: [f64; 2],
    parameter_interval: [f64; 2],
}

struct SurfacePlan {
    geometry: SurfaceGeometry,
    revolution: Option<RevolutionPlan>,
}

#[derive(Debug, Clone, PartialEq)]
struct CurvePlan {
    geometry: CurveGeometry,
    parameter_range: Option<[f64; 2]>,
    edge_tolerance: Option<f64>,
    cache_fit_tolerance: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
struct HelixPlan {
    definition: ProceduralCurveDefinition,
    cache: NurbsCurve,
    parameter_range: [f64; 2],
    fit_tolerance: f64,
}

/// Transfer a complete B5 graph. Returns `false` without mutation when any
/// referenced face, pcurve, edge endpoint, or loop chain remains unresolved.
pub(crate) fn transfer(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    mut graph: B5Graph,
    payload: &UnknownId,
) -> bool {
    if !graph.complete {
        graph.loops.retain(|_, loop_| {
            loop_
                .pcurves
                .iter()
                .zip(&loop_.edges)
                .all(|(pcurve, edge)| {
                    (graph
                        .pcurves
                        .get(pcurve)
                        .is_some_and(|pcurve| pcurve.surface == loop_.surface)
                        || graph
                            .opaque_pcurves
                            .get(pcurve)
                            .is_some_and(|pcurve| pcurve.surface == loop_.surface)
                        || graph.implicit_pcurves.get(pcurve) == Some(&loop_.surface))
                        && graph.edge_vertices.contains_key(edge)
                })
                && solve_loop_chain(loop_, &graph.edge_vertices).is_some()
        });
        graph.faces.retain(|face| {
            graph.surfaces.contains_key(&face.surface)
                && !face.loops.is_empty()
                && face.loops.iter().all(|loop_id| {
                    graph
                        .loops
                        .get(loop_id)
                        .is_some_and(|loop_| loop_.surface == face.surface)
                })
        });
        let referenced_loops: HashSet<u32> = graph
            .faces
            .iter()
            .flat_map(|face| face.loops.iter().copied())
            .collect();
        graph
            .loops
            .retain(|loop_id, _| referenced_loops.contains(loop_id));
        if graph.faces.is_empty() || graph.loops.is_empty() {
            return false;
        }
        graph.complete = true;
    }
    graph.records.clear();
    transfer_complete(ir, annotations, &graph, payload)
}

fn transfer_complete(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    graph: &B5Graph,
    payload: &UnknownId,
) -> bool {
    if graph.faces.is_empty() {
        return false;
    }

    let Some(body_kind) = body_kind_if_owned(graph) else {
        return false;
    };

    let mut referenced_surfaces: HashSet<u32> =
        graph.faces.iter().map(|face| face.surface).collect();
    let mut pending_surfaces: Vec<u32> = referenced_surfaces.iter().copied().collect();
    while let Some(surface_id) = pending_surfaces.pop() {
        let Some(offset) = graph.offset_surfaces.get(&surface_id) else {
            continue;
        };
        if referenced_surfaces.insert(offset.source_surface) {
            pending_surfaces.push(offset.source_surface);
        }
    }
    let mut surface_plan = BTreeMap::new();
    for surface_id in referenced_surfaces {
        let Some(surface) = graph.surfaces.get(&surface_id) else {
            return false;
        };
        surface_plan.insert(
            surface_id,
            neutral_surface(surface, graph, surface_id, payload),
        );
    }

    let mut pcurve_plan = BTreeMap::new();
    let mut edge_curve_plan = HashMap::<u32, CurvePlan>::new();
    let mut conflicting_edge_curves = HashSet::<u32>::new();
    let mut edge_helix_plan = HashMap::<u32, HelixPlan>::new();
    let mut edge_support_plan = HashMap::<u32, Vec<(u32, u32, [f64; 2])>>::new();
    let mut loop_senses = BTreeMap::new();
    let mut edge_ids = BTreeSet::new();
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
                if graph
                    .opaque_pcurves
                    .get(&pcurve_id)
                    .is_some_and(|pcurve| pcurve.surface == loop_.surface)
                    || graph.implicit_pcurves.get(&pcurve_id) == Some(&loop_.surface)
                {
                    edge_ids.insert(edge_id);
                    continue;
                }
                return false;
            };
            if pcurve.surface != loop_.surface || !graph.edge_vertices.contains_key(&edge_id) {
                return false;
            }
            let Some(knots) = expand_knots(&pcurve.distinct_knots, &pcurve.multiplicities) else {
                return false;
            };
            let Some(degree) = usize::try_from(pcurve.degree).ok() else {
                return false;
            };
            let Some(parameter_range) = knots
                .get(degree)
                .copied()
                .zip(
                    knots
                        .len()
                        .checked_sub(degree + 1)
                        .and_then(|index| knots.get(index))
                        .copied(),
                )
                .map(|(start, end)| [start, end])
                .filter(|range| range[0].is_finite() && range[0] < range[1])
            else {
                return false;
            };
            let Some(surface) = graph.surfaces.get(&loop_.surface) else {
                return false;
            };
            let cylinder_reparameterized = matches!(surface, B5Surface::Cylinder { .. });
            let geometry = PcurveGeometry::Nurbs {
                degree: pcurve.degree,
                knots,
                control_points: pcurve
                    .control_points
                    .iter()
                    .map(|point| neutral_pcurve_point(*point, surface))
                    .collect(),
                weights: pcurve.weights.clone(),
                periodic: false,
            };
            pcurve_plan.entry(pcurve_id).or_insert((
                geometry,
                cylinder_reparameterized,
                parameter_range,
            ));
            let supports = edge_support_plan.entry(edge_id).or_default();
            let support_range = edge_pcurve_parameters(graph, edge_id, pcurve_id)
                .and_then(|parameters| ordered_subrange(parameters, parameter_range))
                .unwrap_or(parameter_range);
            if !supports
                .iter()
                .any(|(surface, pcurve, _)| *surface == loop_.surface && *pcurve == pcurve_id)
            {
                supports.push((loop_.surface, pcurve_id, support_range));
            }
            let lifted = lifted_curve_geometry(pcurve, surface).or_else(|| {
                let SurfaceGeometry::Nurbs(cache) = &surface_plan.get(&loop_.surface)?.geometry
                else {
                    return None;
                };
                nurbs_isocurve(pcurve, cache).map(CurveGeometry::Nurbs)
            });
            if let Some(geometry) = lifted {
                let endpoints = graph.edge_vertices[&edge_id];
                let (Some(edge_start), Some(edge_end)) = (
                    b5_vertex_point(graph, endpoints[0]),
                    b5_vertex_point(graph, endpoints[1]),
                ) else {
                    return false;
                };
                let oriented_plan = if matches!(surface, B5Surface::Plane { .. }) {
                    edge_pcurve_parameters(graph, edge_id, pcurve_id).and_then(|parameters| {
                        oriented_nurbs_range(geometry.clone(), parameters, edge_start, edge_end)
                    })
                } else if matches!(surface, B5Surface::Nurbs(_)) {
                    edge_pcurve_parameters(graph, edge_id, pcurve_id)
                        .and_then(|parameters| isocurve_endpoint_parameters(pcurve, parameters))
                        .and_then(|parameters| {
                            oriented_nurbs_range(geometry.clone(), parameters, edge_start, edge_end)
                        })
                } else if matches!(geometry, CurveGeometry::Line { .. }) {
                    oriented_line_plan(&geometry, edge_start, edge_end)
                } else if matches!(geometry, CurveGeometry::Circle { .. }) {
                    edge_pcurve_parameters(graph, edge_id, pcurve_id).and_then(|parameters| {
                        oriented_circle_plan(
                            pcurve, surface, &geometry, parameters, edge_start, edge_end,
                        )
                    })
                } else {
                    None
                };
                let plan = oriented_plan.unwrap_or(CurvePlan {
                    geometry,
                    parameter_range: None,
                    edge_tolerance: None,
                    cache_fit_tolerance: None,
                });
                merge_curve_plan(
                    &mut edge_curve_plan,
                    &mut conflicting_edge_curves,
                    edge_id,
                    plan,
                );
                if conflicting_edge_curves.contains(&edge_id) {
                    edge_helix_plan.remove(&edge_id);
                }
            } else {
                let endpoint_indices = graph.edge_vertices[&edge_id];
                let (Some(edge_start), Some(edge_end)) = (
                    b5_vertex_point(graph, endpoint_indices[0]),
                    b5_vertex_point(graph, endpoint_indices[1]),
                ) else {
                    return false;
                };
                let Some(endpoint_parameters) = edge_pcurve_parameters(graph, edge_id, pcurve_id)
                else {
                    edge_ids.insert(edge_id);
                    continue;
                };
                let Some(helix) =
                    cylinder_helix(pcurve, surface, endpoint_parameters, edge_start, edge_end)
                else {
                    edge_ids.insert(edge_id);
                    continue;
                };
                if edge_helix_plan
                    .get(&edge_id)
                    .is_some_and(|existing| existing != &helix)
                {
                    return false;
                }
                merge_curve_plan(
                    &mut edge_curve_plan,
                    &mut conflicting_edge_curves,
                    edge_id,
                    CurvePlan {
                        geometry: CurveGeometry::Nurbs(helix.cache.clone()),
                        parameter_range: Some(helix.parameter_range),
                        edge_tolerance: Some(helix.fit_tolerance),
                        cache_fit_tolerance: Some(helix.fit_tolerance),
                    },
                );
                if conflicting_edge_curves.contains(&edge_id) {
                    edge_helix_plan.remove(&edge_id);
                } else {
                    edge_helix_plan.entry(edge_id).or_insert(helix);
                }
            }
            edge_ids.insert(edge_id);
        }
    }
    let vertex_tolerances = transfer_vertex_tolerances(graph, &pcurve_plan, &surface_plan);

    let body_id = BodyId("catia:b5:body#0".to_string());
    let region_id = RegionId("catia:b5:region#0".to_string());
    let shell_id = ShellId("catia:b5:shell#0".to_string());
    let used_vertices: HashSet<usize> = edge_ids
        .iter()
        .flat_map(|edge| graph.edge_vertices[edge])
        .collect();

    for (index, coordinates) in graph.vertex_points.iter().enumerate() {
        if !used_vertices.contains(&index) {
            continue;
        }
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
            tolerance: vertex_tolerances.get(&index).copied(),
        });
    }
    for (rank, coordinates) in graph.logical_vertex_points.iter().enumerate() {
        let index = graph.vertex_points.len() + rank;
        if !used_vertices.contains(&index) {
            continue;
        }
        let point_id = PointId(format!("catia:b5:point#{index}"));
        annotate(
            annotations,
            &point_id,
            "object_stream_b5_03",
            "5d_logical_vertex",
            Exactness::Derived,
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
            "5d_logical_vertex",
            Exactness::ByteExact,
        );
        annotations.derived(&vertex_id, "point");
        ir.model.vertices.push(Vertex {
            id: vertex_id,
            point: point_id,
            tolerance: vertex_tolerances.get(&index).copied(),
        });
    }

    let mut surface_ids = HashMap::new();
    for (object_id, plan) in surface_plan {
        let id = SurfaceId(format!("catia:b5:surface#{object_id}"));
        let revolution_cache = plan.revolution.is_some();
        annotate(
            annotations,
            &id,
            "object_stream_b5_03",
            "face_surface",
            if matches!(plan.geometry, SurfaceGeometry::Unknown { .. }) {
                Exactness::Unknown
            } else if revolution_cache {
                Exactness::Derived
            } else {
                Exactness::ByteExact
            },
        );
        if revolution_cache {
            annotations.derived(&id, "geometry");
        }
        surface_ids.insert(object_id, id.clone());
        ir.model.surfaces.push(Surface {
            id: id.clone(),
            geometry: plan.geometry,
            source_object: None,
        });
        if let Some(revolution) = plan.revolution {
            let directrix_id = CurveId(format!("catia:b5:profile#{object_id}"));
            annotate(
                annotations,
                &directrix_id,
                "object_stream_b5_03",
                "2d_profile_curve",
                Exactness::Derived,
            );
            annotations.derived(&directrix_id, "geometry");
            ir.model.curves.push(Curve {
                id: directrix_id.clone(),
                geometry: CurveGeometry::Nurbs(revolution.directrix),
                source_object: None,
            });
            let procedural_id =
                ProceduralSurfaceId(format!("catia:b5:procedural-surface#{object_id}"));
            annotate(
                annotations,
                &procedural_id,
                "object_stream_b5_03",
                "2d_surface_of_revolution",
                Exactness::Derived,
            );
            ir.model.procedural_surfaces.push(ProceduralSurface {
                id: procedural_id,
                surface: id,
                definition: ProceduralSurfaceDefinition::Revolution {
                    directrix: directrix_id,
                    axis_origin: revolution.axis_origin,
                    axis_direction: revolution.axis_direction,
                    angular_interval: revolution.angular_interval,
                    parameter_interval: revolution.parameter_interval,
                    transposed: false,
                },
                cache_fit_tolerance: None,
            });
        }
    }
    for offset in graph.offset_surfaces.values() {
        let (Some(surface), Some(support)) = (
            surface_ids.get(&offset.object_id),
            surface_ids.get(&offset.source_surface),
        ) else {
            continue;
        };
        let procedural_id = ProceduralSurfaceId(format!("catia:b5:offset#{}", offset.object_id));
        annotate(
            annotations,
            &procedural_id,
            "object_stream_b5_03",
            "30_offset_surface",
            Exactness::Derived,
        );
        annotations.derived(&procedural_id, "definition.u_sense");
        annotations.derived(&procedural_id, "definition.v_sense");
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: procedural_id,
            surface: surface.clone(),
            definition: ProceduralSurfaceDefinition::Offset {
                support: support.clone(),
                distance: offset.distance,
                u_sense: 0,
                v_sense: 0,
                extension_flags: Vec::new(),
            },
            cache_fit_tolerance: None,
        });
    }

    let mut occurrence_groups = BTreeMap::<u32, BTreeMap<[u64; 2], Vec<(u32, usize)>>>::new();
    for loop_ in graph.loops.values() {
        for (index, (&object_id, &edge_id)) in loop_.pcurves.iter().zip(&loop_.edges).enumerate() {
            let Some((_, _, native_range)) = pcurve_plan.get(&object_id) else {
                continue;
            };
            let parameter_range = edge_pcurve_parameters(graph, edge_id, object_id)
                .and_then(|parameters| ordered_subrange(parameters, *native_range))
                .unwrap_or(*native_range);
            occurrence_groups
                .entry(object_id)
                .or_default()
                .entry(parameter_range.map(|parameter| {
                    if parameter == 0.0 {
                        0.0f64.to_bits()
                    } else {
                        parameter.to_bits()
                    }
                }))
                .or_default()
                .push((loop_.object_id, index));
        }
    }
    let mut pcurve_ids = HashMap::new();
    for (object_id, ranges) in occurrence_groups {
        let (geometry, cylinder_reparameterized, _) = &pcurve_plan[&object_id];
        let range_count = ranges.len();
        for (rank, (range_bits, occurrences)) in ranges.into_iter().enumerate() {
            let id = if range_count == 1 {
                PcurveId(format!("catia:b5:pcurve#{object_id}"))
            } else {
                PcurveId(format!("catia:b5:pcurve#{object_id}@{rank}"))
            };
            annotate(
                annotations,
                &id,
                "object_stream_b5_03",
                "21_pcurve",
                Exactness::ByteExact,
            );
            if *cylinder_reparameterized {
                annotations.derived(&id, "geometry.control_points");
            }
            annotations.derived(&id, "parameter_range");
            for occurrence in occurrences {
                pcurve_ids.insert(occurrence, id.clone());
            }
            ir.model.pcurves.push(Pcurve {
                id,
                geometry: geometry.clone(),
                wrapper_reversed: None,
                parameter_range: Some(range_bits.map(f64::from_bits)),
                fit_tolerance: None,
                native_tail_flags: None,
            });
        }
    }

    let mut edge_id_map = HashMap::new();
    for edge_id in edge_ids {
        let id = EdgeId(format!("catia:b5:edge#{edge_id}"));
        let curve_id = CurveId(format!("catia:b5:curve#{edge_id}"));
        let endpoints = graph.edge_vertices[&edge_id];
        let curve_plan = edge_curve_plan
            .remove(&edge_id)
            .unwrap_or_else(|| CurvePlan {
                geometry: CurveGeometry::Unknown {
                    record: Some(payload.clone()),
                },
                parameter_range: None,
                edge_tolerance: None,
                cache_fit_tolerance: None,
            });
        let geometry = curve_plan.geometry;
        annotate(
            annotations,
            &curve_id,
            "object_stream_b5_03",
            "pcurve_lifted_3d_curve",
            if matches!(geometry, CurveGeometry::Unknown { .. }) {
                Exactness::Unknown
            } else {
                Exactness::Derived
            },
        );
        if !matches!(geometry, CurveGeometry::Unknown { .. }) {
            annotations.derived(&curve_id, "geometry");
        }
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry,
            source_object: None,
        });
        let helix = edge_helix_plan.remove(&edge_id);
        let edge_range = curve_plan.parameter_range;
        let edge_tolerance = curve_plan.edge_tolerance;
        let cache_fit_tolerance = curve_plan.cache_fit_tolerance;
        let procedural = helix
            .as_ref()
            .map(|plan| {
                (
                    "helix",
                    "cylinder_parametric_helix",
                    plan.definition.clone(),
                )
            })
            .or_else(|| {
                b5_edge_support_definition(
                    edge_support_plan.get(&edge_id)?,
                    &surface_ids,
                    &pcurve_plan,
                )
            });
        if let Some((kind, tag, definition)) = procedural {
            let procedural_id = ProceduralCurveId(format!("catia:b5:{kind}#{edge_id}"));
            annotate(
                annotations,
                &procedural_id,
                "object_stream_b5_03",
                tag,
                Exactness::Derived,
            );
            annotations
                .derived(&procedural_id, "curve")
                .derived(&procedural_id, "definition");
            if cache_fit_tolerance.is_some() {
                annotations.derived(&procedural_id, "cache_fit_tolerance");
            }
            ir.model.procedural_curves.push(ProceduralCurve {
                id: procedural_id,
                curve: curve_id.clone(),
                definition,
                cache_fit_tolerance,
            });
        }
        annotate(
            annotations,
            &id,
            "object_stream_b5_03",
            "5e_edge",
            Exactness::ByteExact,
        );
        annotations.derived(&id, "start").derived(&id, "end");
        if edge_range.is_some() {
            annotations.derived(&id, "param_range");
        }
        if edge_tolerance.is_some() {
            annotations.derived(&id, "tolerance");
        }
        edge_id_map.insert(edge_id, id.clone());
        ir.model.edges.push(Edge {
            id,
            curve: Some(curve_id),
            start: VertexId(format!("catia:b5:vertex#{}", endpoints[0])),
            end: VertexId(format!("catia:b5:vertex#{}", endpoints[1])),
            param_range: edge_range,
            tolerance: edge_tolerance,
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
        kind: body_kind,
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
        .derived(&shell_id, "faces");
    ir.model.shells.push(Shell {
        id: shell_id.clone(),
        region: region_id,
        faces: graph
            .faces
            .iter()
            .map(|face| FaceId(format!("catia:b5:face#{}", face.object_id)))
            .collect(),
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
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
            for (index, (&edge, &reversed)) in loop_.edges.iter().zip(senses).enumerate() {
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
                    pcurve: pcurve_ids.get(&(loop_.object_id, index)).cloned(),
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
    true
}

fn transfer_vertex_tolerances(
    graph: &B5Graph,
    pcurves: &BTreeMap<u32, (PcurveGeometry, bool, [f64; 2])>,
    surfaces: &BTreeMap<u32, SurfacePlan>,
) -> BTreeMap<usize, f64> {
    let mut tolerances = graph.vertex_tolerances.clone();
    for loop_ in graph.loops.values() {
        let Some(surface) = surfaces.get(&loop_.surface) else {
            continue;
        };
        for (&pcurve_id, &edge_id) in loop_.pcurves.iter().zip(&loop_.edges) {
            let (Some((pcurve, _, parameter_range)), Some(&vertices)) =
                (pcurves.get(&pcurve_id), graph.edge_vertices.get(&edge_id))
            else {
                continue;
            };
            let occurrence_parameters = edge_pcurve_parameters(graph, edge_id, pcurve_id)
                .filter(|parameters| ordered_subrange(*parameters, *parameter_range).is_some())
                .unwrap_or(*parameter_range);
            let Some(lifted) = occurrence_parameters
                .map(|parameter| {
                    let uv = pcurve_uv(pcurve, parameter)?;
                    let point = surface_point(&surface.geometry, uv.u, uv.v)?;
                    Some([point.x, point.y, point.z])
                })
                .into_iter()
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let [Some(first), Some(second)] = vertices.map(|vertex| b5_vertex_point(graph, vertex))
            else {
                continue;
            };
            let coordinates = [first, second];
            let forward = [
                distance(coordinates[0], lifted[0]),
                distance(coordinates[1], lifted[1]),
            ];
            let reverse = [
                distance(coordinates[1], lifted[0]),
                distance(coordinates[0], lifted[1]),
            ];
            let residuals = if forward[0].max(forward[1]) <= reverse[0].max(reverse[1]) {
                [(vertices[0], forward[0]), (vertices[1], forward[1])]
            } else {
                [(vertices[1], reverse[0]), (vertices[0], reverse[1])]
            };
            for (vertex, residual) in residuals {
                if residual > 1e-9 && residual.is_finite() {
                    tolerances
                        .entry(vertex)
                        .and_modify(|tolerance| *tolerance = tolerance.max(residual + 1e-9))
                        .or_insert(residual + 1e-9);
                }
            }
        }
    }
    tolerances
}

fn merge_curve_plan(
    plans: &mut HashMap<u32, CurvePlan>,
    conflicts: &mut HashSet<u32>,
    edge: u32,
    candidate: CurvePlan,
) {
    if conflicts.contains(&edge) {
        return;
    }
    let Some(existing) = plans.get_mut(&edge) else {
        plans.insert(edge, candidate);
        return;
    };
    let range_conflict = existing
        .parameter_range
        .zip(candidate.parameter_range)
        .is_some_and(|(left, right)| left != right);
    let edge_tolerance_conflict = existing
        .edge_tolerance
        .zip(candidate.edge_tolerance)
        .is_some_and(|(left, right)| left != right);
    let cache_tolerance_conflict = existing
        .cache_fit_tolerance
        .zip(candidate.cache_fit_tolerance)
        .is_some_and(|(left, right)| left != right);
    if existing.geometry != candidate.geometry
        || range_conflict
        || edge_tolerance_conflict
        || cache_tolerance_conflict
    {
        plans.remove(&edge);
        conflicts.insert(edge);
        return;
    }
    if existing.parameter_range.is_none() {
        existing.parameter_range = candidate.parameter_range;
    }
    if existing.edge_tolerance.is_none() {
        existing.edge_tolerance = candidate.edge_tolerance;
    }
    if existing.cache_fit_tolerance.is_none() {
        existing.cache_fit_tolerance = candidate.cache_fit_tolerance;
    }
}

fn b5_vertex_point(graph: &B5Graph, vertex: usize) -> Option<[f64; 3]> {
    graph.vertex_points.get(vertex).copied().or_else(|| {
        vertex
            .checked_sub(graph.vertex_points.len())
            .and_then(|index| graph.logical_vertex_points.get(index))
            .copied()
    })
}

fn edge_pcurve_parameters(graph: &B5Graph, edge: u32, pcurve: u32) -> Option<[f64; 2]> {
    graph
        .edge_parameter_incidences
        .get(&edge)?
        .map(|incidence_id| {
            let incidence = graph.parameter_incidences.get(&incidence_id)?;
            let mut parameters = incidence
                .curves
                .iter()
                .zip(&incidence.parameters)
                .filter_map(|(&curve, &parameter)| (curve == pcurve).then_some(parameter));
            let parameter = parameters.next()?;
            parameters
                .all(|other| other == parameter)
                .then_some(parameter)
        })
        .into_iter()
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

fn ordered_subrange(parameters: [f64; 2], domain: [f64; 2]) -> Option<[f64; 2]> {
    const PARAMETER_TOLERANCE: f64 = 1e-9;

    if !parameters.into_iter().all(f64::is_finite)
        || parameters[0] == parameters[1]
        || parameters.iter().any(|parameter| {
            *parameter < domain[0] - PARAMETER_TOLERANCE
                || *parameter > domain[1] + PARAMETER_TOLERANCE
        })
    {
        return None;
    }
    let parameters = parameters.map(|parameter| parameter.clamp(domain[0], domain[1]));
    Some(if parameters[0] < parameters[1] {
        parameters
    } else {
        [parameters[1], parameters[0]]
    })
}

fn oriented_line_plan(
    geometry: &CurveGeometry,
    edge_start: [f64; 3],
    edge_end: [f64; 3],
) -> Option<CurvePlan> {
    let CurveGeometry::Line { origin, direction } = geometry else {
        return None;
    };
    let origin = [origin.x, origin.y, origin.z];
    let mut direction = [direction.x, direction.y, direction.z];
    let direction_length = length(direction);
    if !direction_length.is_finite() || direction_length <= f64::EPSILON {
        return None;
    }
    direction = scale(direction, 1.0 / direction_length);
    let parameter = |point| dot(subtract(point, origin), direction);
    let mut range = [parameter(edge_start), parameter(edge_end)];
    if !range.into_iter().all(f64::is_finite) || range[0] == range[1] {
        return None;
    }
    let projected = range.map(|value| add(origin, scale(direction, value)));
    let residual = distance(projected[0], edge_start).max(distance(projected[1], edge_end));
    if residual > POINT_TOLERANCE {
        return None;
    }
    if range[0] > range[1] {
        direction = scale(direction, -1.0);
        range = [-range[0], -range[1]];
    }
    Some(CurvePlan {
        geometry: CurveGeometry::Line {
            origin: point3(origin),
            direction: vector(direction),
        },
        parameter_range: Some(range),
        edge_tolerance: (residual > 1e-9).then_some(residual + 1e-9),
        cache_fit_tolerance: None,
    })
}

fn oriented_circle_plan(
    pcurve: &B5Pcurve,
    surface: &B5Surface,
    geometry: &CurveGeometry,
    endpoint_parameters: [f64; 2],
    edge_start: [f64; 3],
    edge_end: [f64; 3],
) -> Option<CurvePlan> {
    let (dimension, scale) = isoparametric_angle_coordinate(pcurve, surface)?;
    if !scale.is_finite() || scale.abs() <= f64::EPSILON {
        return None;
    }
    if let Some(weights) = &pcurve.weights {
        if weights.len() != pcurve.control_points.len()
            || weights
                .iter()
                .any(|weight| !weight.is_finite() || *weight <= 0.0)
        {
            return None;
        }
    }
    let endpoints =
        endpoint_parameters.map(|parameter| crate::b5::evaluate_pcurve(pcurve, parameter));
    let [Some(start_uv), Some(end_uv)] = endpoints else {
        return None;
    };
    let angles = [start_uv[dimension] / scale, end_uv[dimension] / scale];
    let delta = angles[1] - angles[0];
    if !delta.is_finite() || delta.abs() <= 1e-12 || delta.abs() > std::f64::consts::TAU + 1e-9 {
        return None;
    }
    let direction = delta.signum();
    if pcurve
        .control_points
        .windows(2)
        .any(|points| direction * (points[1][dimension] - points[0][dimension]) / scale < -1e-12)
    {
        return None;
    }

    let CurveGeometry::Circle {
        center,
        axis,
        ref_direction,
        radius,
    } = geometry
    else {
        return None;
    };
    if !radius.is_finite() || radius.abs() <= f64::EPSILON {
        return None;
    }
    let mut axis = *axis;
    let mut ref_direction = *ref_direction;
    let radius = if *radius < 0.0 {
        ref_direction = Vector3::new(-ref_direction.x, -ref_direction.y, -ref_direction.z);
        -*radius
    } else {
        *radius
    };
    let oriented_angles = if delta < 0.0 {
        axis = Vector3::new(-axis.x, -axis.y, -axis.z);
        [-angles[0], -angles[1]]
    } else {
        angles
    };
    let parameter_range = crate::geometry::canonical_periodic_range(oriented_angles)?;
    let geometry = CurveGeometry::Circle {
        center: *center,
        axis,
        ref_direction,
        radius,
    };
    let evaluated = parameter_range.map(|parameter| curve_point(&geometry, parameter));
    let [Some(start), Some(end)] = evaluated else {
        return None;
    };
    let residual = distance([start.x, start.y, start.z], edge_start)
        .max(distance([end.x, end.y, end.z], edge_end));
    if residual > POINT_TOLERANCE {
        return None;
    }
    Some(CurvePlan {
        geometry,
        parameter_range: Some(parameter_range),
        edge_tolerance: (residual > 1e-9).then_some(residual + 1e-9),
        cache_fit_tolerance: None,
    })
}

fn isoparametric_angle_coordinate(pcurve: &B5Pcurve, surface: &B5Surface) -> Option<(usize, f64)> {
    match surface {
        B5Surface::Cylinder { radius, .. }
            if constant_coordinate(&pcurve.control_points, 1).is_some() =>
        {
            Some((0, *radius))
        }
        B5Surface::Cone { angular_scale, .. }
            if constant_coordinate(&pcurve.control_points, 1).is_some() =>
        {
            Some((0, *angular_scale))
        }
        B5Surface::Torus { minor_scale, .. }
            if constant_coordinate(&pcurve.control_points, 0).is_some() =>
        {
            Some((1, *minor_scale))
        }
        B5Surface::Torus { major_scale, .. }
            if constant_coordinate(&pcurve.control_points, 1).is_some() =>
        {
            Some((0, *major_scale))
        }
        _ => None,
    }
}

fn oriented_nurbs_range(
    geometry: CurveGeometry,
    endpoint_parameters: [f64; 2],
    edge_start: [f64; 3],
    edge_end: [f64; 3],
) -> Option<CurvePlan> {
    let CurveGeometry::Nurbs(mut curve) = geometry else {
        return None;
    };
    let degree = usize::try_from(curve.degree).ok()?;
    let domain_start = *curve.knots.get(degree)?;
    let domain_end = *curve
        .knots
        .len()
        .checked_sub(degree + 1)
        .and_then(|index| curve.knots.get(index))?;
    let mut range = endpoint_parameters;
    if range[0] > range[1] {
        let sum = domain_start + domain_end;
        curve.knots = curve
            .knots
            .into_iter()
            .rev()
            .map(|knot| sum - knot)
            .collect();
        curve.control_points.reverse();
        if let Some(weights) = &mut curve.weights {
            weights.reverse();
        }
        range = [sum - range[0], sum - range[1]];
    }
    if !range[0].is_finite()
        || !range[1].is_finite()
        || range[0] >= range[1]
        || range[0] < domain_start
        || range[1] > domain_end
    {
        return None;
    }
    let geometry = CurveGeometry::Nurbs(curve);
    let start = curve_point(&geometry, range[0])?;
    let end = curve_point(&geometry, range[1])?;
    let residual = distance([start.x, start.y, start.z], edge_start)
        .max(distance([end.x, end.y, end.z], edge_end));
    if residual > POINT_TOLERANCE {
        return None;
    }
    Some(CurvePlan {
        geometry,
        parameter_range: Some(range),
        edge_tolerance: (residual > 1e-9).then_some(residual + 1e-9),
        cache_fit_tolerance: None,
    })
}

fn isocurve_endpoint_parameters(
    pcurve: &B5Pcurve,
    endpoint_parameters: [f64; 2],
) -> Option<[f64; 2]> {
    let varying_dimension = if constant_coordinate(&pcurve.control_points, 0).is_some() {
        1
    } else if constant_coordinate(&pcurve.control_points, 1).is_some() {
        0
    } else {
        return None;
    };
    if let Some(weights) = &pcurve.weights {
        if weights.len() != pcurve.control_points.len()
            || weights
                .iter()
                .any(|weight| !weight.is_finite() || *weight <= 0.0)
        {
            return None;
        }
    }
    if pcurve
        .control_points
        .iter()
        .any(|point| !point[varying_dimension].is_finite())
        || (!pcurve
            .control_points
            .windows(2)
            .all(|pair| pair[0][varying_dimension] <= pair[1][varying_dimension])
            && !pcurve
                .control_points
                .windows(2)
                .all(|pair| pair[0][varying_dimension] >= pair[1][varying_dimension]))
    {
        return None;
    }
    let values = endpoint_parameters
        .map(|parameter| crate::b5::evaluate_pcurve(pcurve, parameter))
        .map(|uv| uv.map(|point| point[varying_dimension]));
    let [Some(start), Some(end)] = values else {
        return None;
    };
    Some([start, end])
}

fn distance(left: [f64; 3], right: [f64; 3]) -> f64 {
    left.into_iter()
        .zip(right)
        .map(|(left, right)| (left - right) * (left - right))
        .sum::<f64>()
        .sqrt()
}

fn neutral_pcurve_point(point: [f64; 2], surface: &B5Surface) -> Point2 {
    match surface {
        B5Surface::Cylinder { radius, .. } => Point2::new(point[0] / radius, point[1]),
        B5Surface::Cone {
            half_angle,
            slant_range,
            angular_scale,
            ..
        } => Point2::new(
            point[0] / angular_scale,
            (point[1] - slant_range[0]) * half_angle.cos(),
        ),
        B5Surface::Torus {
            major_scale,
            minor_scale,
            ..
        } => Point2::new(point[0] / major_scale, point[1] / minor_scale),
        _ => Point2::new(point[0], point[1]),
    }
}

fn lifted_curve_geometry(pcurve: &B5Pcurve, surface: &B5Surface) -> Option<CurveGeometry> {
    let knots = expand_knots(&pcurve.distinct_knots, &pcurve.multiplicities)?;
    match surface {
        B5Surface::UnresolvedNurbs { .. } | B5Surface::Unknown { .. } => None,
        B5Surface::Plane {
            origin,
            direction_u,
            direction_v,
        } => Some(CurveGeometry::Nurbs(NurbsCurve {
            degree: pcurve.degree,
            knots,
            control_points: pcurve
                .control_points
                .iter()
                .map(|uv| {
                    point3(add(
                        *origin,
                        add(scale(*direction_u, uv[0]), scale(*direction_v, uv[1])),
                    ))
                })
                .collect(),
            weights: pcurve.weights.clone(),
            periodic: false,
        })),
        B5Surface::Cylinder {
            origin,
            reference_x,
            axis,
            radius,
        } if constant_coordinate(&pcurve.control_points, 0).is_some() => {
            let first = pcurve.control_points.first()?;
            let line_origin = cylinder_point(*origin, *reference_x, *axis, *radius, *first);
            Some(CurveGeometry::Line {
                origin: point3(line_origin),
                direction: vector(*axis),
            })
        }
        B5Surface::Cone {
            apex,
            direction_x,
            direction_y,
            axis,
            half_angle,
            angular_scale,
            ..
        } if constant_coordinate(&pcurve.control_points, 0).is_some() => {
            let [u, _] = *pcurve.control_points.first()?;
            let angle = u / angular_scale;
            let radial = add(
                scale(*direction_x, angle.cos()),
                scale(*direction_y, angle.sin()),
            );
            Some(CurveGeometry::Line {
                origin: point3(*apex),
                direction: vector(add(
                    scale(*axis, half_angle.cos()),
                    scale(radial, half_angle.sin()),
                )),
            })
        }
        B5Surface::Torus {
            center,
            direction_x,
            direction_y,
            axis,
            major_radius,
            minor_radius,
            major_scale,
            minor_scale,
        } if constant_coordinate(&pcurve.control_points, 0).is_some() => {
            let u = pcurve.control_points.first()?[0];
            let angle = u / major_scale;
            let radial = add(
                scale(*direction_x, angle.cos()),
                scale(*direction_y, angle.sin()),
            );
            Some(CurveGeometry::Circle {
                center: point3(add(*center, scale(radial, *major_radius))),
                axis: vector(cross(radial, *axis)),
                ref_direction: vector(radial),
                radius: *minor_radius,
            })
        }
        B5Surface::Torus {
            center,
            direction_x,
            axis,
            major_radius,
            minor_radius,
            minor_scale,
            ..
        } => {
            let v = constant_coordinate(&pcurve.control_points, 1)?;
            let angle = v / minor_scale;
            let signed_radius = major_radius + minor_radius * angle.cos();
            (signed_radius.abs() > f64::EPSILON).then_some(())?;
            Some(CurveGeometry::Circle {
                center: point3(add(*center, scale(*axis, minor_radius * angle.sin()))),
                axis: vector(*axis),
                ref_direction: vector(scale(*direction_x, signed_radius.signum())),
                radius: signed_radius.abs(),
            })
        }
        B5Surface::Cone {
            apex,
            direction_x,
            axis,
            half_angle,
            ..
        } => {
            let slant = constant_coordinate(&pcurve.control_points, 1)?;
            (slant.abs() > f64::EPSILON).then_some(())?;
            Some(CurveGeometry::Circle {
                center: point3(add(*apex, scale(*axis, slant * half_angle.cos()))),
                axis: vector(*axis),
                ref_direction: vector(*direction_x),
                radius: slant * half_angle.sin(),
            })
        }
        B5Surface::Cylinder {
            origin,
            reference_x,
            axis,
            radius,
        } => {
            let v = constant_coordinate(&pcurve.control_points, 1)?;
            Some(CurveGeometry::Circle {
                center: point3(add(*origin, scale(*axis, v))),
                axis: vector(*axis),
                ref_direction: vector(*reference_x),
                radius: *radius,
            })
        }
        B5Surface::Nurbs(surface) => nurbs_isocurve(pcurve, surface).map(CurveGeometry::Nurbs),
        B5Surface::Revolution { .. } => None,
    }
}

fn nurbs_isocurve(pcurve: &B5Pcurve, surface: &NurbsSurface) -> Option<NurbsCurve> {
    if let Some(u) = constant_coordinate(&pcurve.control_points, 0) {
        crate::geometry::nurbs_surface_isocurve(surface, u, true)
    } else if let Some(v) = constant_coordinate(&pcurve.control_points, 1) {
        crate::geometry::nurbs_surface_isocurve(surface, v, false)
    } else {
        None
    }
}

fn constant_coordinate(points: &[[f64; 2]], dimension: usize) -> Option<f64> {
    let value = points.first()?[dimension];
    points
        .iter()
        .all(|point| (point[dimension] - value).abs() <= 1e-12 * (1.0 + value.abs()))
        .then_some(value)
}

fn cylinder_point(
    origin: [f64; 3],
    reference_x: [f64; 3],
    axis: [f64; 3],
    radius: f64,
    uv: [f64; 2],
) -> [f64; 3] {
    let reference_y = cross(axis, reference_x);
    let angle = uv[0] / radius;
    add(
        origin,
        add(
            scale(
                add(
                    scale(reference_x, angle.cos()),
                    scale(reference_y, angle.sin()),
                ),
                radius,
            ),
            scale(axis, uv[1]),
        ),
    )
}

fn cylinder_helix(
    pcurve: &B5Pcurve,
    surface: &B5Surface,
    endpoint_parameters: [f64; 2],
    edge_start: [f64; 3],
    edge_end: [f64; 3],
) -> Option<HelixPlan> {
    const FIT_TOLERANCE: f64 = 1e-4;

    let B5Surface::Cylinder {
        origin,
        reference_x,
        axis,
        radius,
    } = surface
    else {
        return None;
    };
    if pcurve.degree != 1 || pcurve.control_points.len() != 2 {
        return None;
    }
    let endpoints =
        endpoint_parameters.map(|parameter| crate::b5::evaluate_pcurve(pcurve, parameter));
    let [Some(first), Some(second)] = endpoints else {
        return None;
    };
    let mut endpoints = [first, second];
    let lifted = endpoints.map(|uv| cylinder_point(*origin, *reference_x, *axis, *radius, uv));
    let forward_error = distance(lifted[0], edge_start).max(distance(lifted[1], edge_end));
    let reverse_error = distance(lifted[1], edge_start).max(distance(lifted[0], edge_end));
    if (forward_error - reverse_error).abs() <= 1e-12
        || forward_error.min(reverse_error) > POINT_TOLERANCE
    {
        return None;
    }
    if reverse_error < forward_error {
        endpoints.swap(0, 1);
    }
    let angles = [endpoints[0][0] / radius, endpoints[1][0] / radius];
    let delta_angle = angles[1] - angles[0];
    let delta_height = endpoints[1][1] - endpoints[0][1];
    if delta_angle.abs() <= 1e-12 || delta_height.abs() <= 1e-12 {
        return None;
    }
    let reference_y = cross(*axis, *reference_x);
    let radial = add(
        scale(*reference_x, angles[0].cos()),
        scale(reference_y, angles[0].sin()),
    );
    let tangent = cross(*axis, radial);
    let sweep = delta_angle.abs();
    let definition = ProceduralCurveDefinition::Helix {
        angle_range: [0.0, sweep],
        center: point3(add(*origin, scale(*axis, endpoints[0][1]))),
        major: vector(scale(radial, *radius)),
        minor: vector(scale(tangent, radius * delta_angle.signum())),
        pitch: vector(scale(
            *axis,
            delta_height / sweep * 2.0 * std::f64::consts::PI,
        )),
        apex_factor: 0.0,
        axis: vector(*axis),
    };
    let cache = crate::geometry::circular_helix_cache(&definition, FIT_TOLERANCE)?;
    let cache_start = cache.curve.control_points.first()?;
    let cache_end = cache.curve.control_points.last()?;
    if distance([cache_start.x, cache_start.y, cache_start.z], edge_start) > POINT_TOLERANCE
        || distance([cache_end.x, cache_end.y, cache_end.z], edge_end) > POINT_TOLERANCE
    {
        return None;
    }
    Some(HelixPlan {
        definition,
        cache: cache.curve,
        parameter_range: [0.0, sweep],
        fit_tolerance: cache.fit_tolerance,
    })
}

fn neutral_surface(
    surface: &B5Surface,
    graph: &B5Graph,
    surface_id: u32,
    payload: &UnknownId,
) -> SurfacePlan {
    let mut revolution = None;
    let geometry = match surface {
        B5Surface::UnresolvedNurbs { .. } | B5Surface::Unknown { .. } => SurfaceGeometry::Unknown {
            record: Some(payload.clone()),
        },
        B5Surface::Plane {
            origin,
            direction_u,
            direction_v,
        } => orthonormal_plane(*origin, *direction_u, *direction_v).unwrap_or_else(|| {
            SurfaceGeometry::Unknown {
                record: Some(payload.clone()),
            }
        }),
        B5Surface::Cylinder {
            origin,
            reference_x,
            axis,
            radius,
        } => SurfaceGeometry::Cylinder {
            origin: point(*origin),
            axis: vector(*axis),
            ref_direction: vector(*reference_x),
            radius: *radius,
        },
        B5Surface::Cone {
            apex,
            direction_x,
            axis,
            half_angle,
            slant_range,
            ..
        } => {
            let slant = slant_range[0];
            SurfaceGeometry::Cone {
                origin: point(add(*apex, scale(*axis, slant * half_angle.cos()))),
                axis: vector(*axis),
                ref_direction: vector(*direction_x),
                radius: slant * half_angle.sin(),
                ratio: 1.0,
                half_angle: *half_angle,
            }
        }
        B5Surface::Torus {
            center,
            direction_x,
            axis,
            major_radius,
            minor_radius,
            ..
        } => SurfaceGeometry::Torus {
            center: point(*center),
            axis: vector(*axis),
            ref_direction: vector(*direction_x),
            major_radius: *major_radius,
            minor_radius: *minor_radius,
        },
        B5Surface::Nurbs(surface) => SurfaceGeometry::Nurbs(surface.clone()),
        B5Surface::Revolution {
            profile_curve,
            axis_origin,
            axis_direction,
            gauge_radius,
        } => revolution_surface(
            graph.profiles.get(profile_curve),
            *axis_origin,
            *axis_direction,
            *gauge_radius,
            surface_parameter_bounds(graph, surface_id),
        )
        .map_or_else(
            || SurfaceGeometry::Unknown {
                record: Some(payload.clone()),
            },
            |(surface, plan)| {
                revolution = Some(plan);
                SurfaceGeometry::Nurbs(surface)
            },
        ),
    };
    SurfacePlan {
        geometry,
        revolution,
    }
}

fn surface_parameter_bounds(graph: &B5Graph, surface_id: u32) -> Option<[[f64; 2]; 2]> {
    let mut bounds = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
    for point in graph
        .pcurves
        .values()
        .filter(|pcurve| pcurve.surface == surface_id)
        .flat_map(|pcurve| &pcurve.control_points)
    {
        for dimension in 0..2 {
            bounds[dimension][0] = bounds[dimension][0].min(point[dimension]);
            bounds[dimension][1] = bounds[dimension][1].max(point[dimension]);
        }
    }
    bounds
        .iter()
        .all(|range| range[0].is_finite() && range[0] < range[1])
        .then_some(bounds)
}

fn b5_edge_support_definition(
    supports: &[(u32, u32, [f64; 2])],
    surface_ids: &HashMap<u32, SurfaceId>,
    pcurves: &BTreeMap<u32, (PcurveGeometry, bool, [f64; 2])>,
) -> Option<(&'static str, &'static str, ProceduralCurveDefinition)> {
    const PARAMETER_TOLERANCE: f64 = 1e-9;

    let ([first] | [first, _]) = supports else {
        return None;
    };
    let parameter_range = first.2;
    if supports.iter().skip(1).any(|support| {
        (support.2[0] - parameter_range[0]).abs() > PARAMETER_TOLERANCE
            || (support.2[1] - parameter_range[1]).abs() > PARAMETER_TOLERANCE
    }) {
        return None;
    }
    let mut sides = std::array::from_fn(|_| IntcurveSupportSide {
        surface: None,
        pcurve: None,
    });
    for (side, (surface, pcurve, _)) in sides.iter_mut().zip(supports) {
        side.surface = Some(surface_ids.get(surface)?.clone());
        side.pcurve = Some(pcurves.get(pcurve)?.0.clone());
    }
    let context = IntcurveSupportContext {
        sides,
        parameter_range,
        discontinuities: std::array::from_fn(|_| Vec::new()),
    };
    if supports.len() == 2 && supports[0].0 != supports[1].0 {
        Some((
            "intersection",
            "two_surface_pcurve_intersection",
            ProceduralCurveDefinition::Intersection {
                context,
                discontinuity_flag: false,
            },
        ))
    } else {
        Some((
            "surface-curve",
            "parametric_surface_curve",
            ProceduralCurveDefinition::SurfaceCurve {
                family: SurfaceCurveFamily::Parametric,
                context,
            },
        ))
    }
}

fn revolution_surface(
    profile: Option<&B5Profile>,
    axis_origin: [f64; 3],
    axis_direction: [f64; 3],
    gauge_radius: f64,
    bounds: Option<[[f64; 2]; 2]>,
) -> Option<(NurbsSurface, RevolutionPlan)> {
    let profile = profile?;
    let [parameter_interval, native_angular_interval] = bounds?;
    let directrix = profile_nurbs(profile, parameter_interval)?;
    let sign = gauge_radius.signum();
    if sign == 0.0 {
        return None;
    }
    let effective_axis = scale(axis_direction, sign);
    let angular_interval = [
        native_angular_interval[0] / gauge_radius.abs(),
        native_angular_interval[1] / gauge_radius.abs(),
    ];
    let surface = revolve_nurbs(
        &directrix,
        axis_origin,
        effective_axis,
        angular_interval,
        native_angular_interval,
    )?;
    Some((
        surface,
        RevolutionPlan {
            directrix,
            axis_origin: point(axis_origin),
            axis_direction: vector(effective_axis),
            angular_interval,
            parameter_interval,
        },
    ))
}

fn profile_nurbs(profile: &B5Profile, interval: [f64; 2]) -> Option<NurbsCurve> {
    match profile {
        B5Profile::Line { point, direction } => Some(NurbsCurve {
            degree: 1,
            knots: vec![interval[0], interval[0], interval[1], interval[1]],
            control_points: interval
                .map(|parameter| point3(add(*point, scale(*direction, parameter))))
                .to_vec(),
            weights: None,
            periodic: false,
        }),
        B5Profile::Arc {
            center,
            direction_x,
            direction_y,
            radius,
        } => rational_arc(*center, *direction_x, *direction_y, *radius, interval),
    }
}

fn rational_arc(
    center: [f64; 3],
    direction_x: [f64; 3],
    direction_y: [f64; 3],
    radius: f64,
    interval: [f64; 2],
) -> Option<NurbsCurve> {
    let angles = [interval[0] / radius, interval[1] / radius];
    let span_count = ((angles[1] - angles[0]).abs() / std::f64::consts::FRAC_PI_2).ceil();
    if !span_count.is_finite() || span_count > crate::MAX_EXACT_ARC_SPANS as f64 {
        return None;
    }
    let span_count = (span_count as usize).max(1);
    let control_count = span_count.checked_mul(2)?.checked_add(1)?;
    let mut control_points = Vec::with_capacity(control_count);
    let mut weights = Vec::with_capacity(control_points.capacity());
    let mut knots = Vec::with_capacity(control_points.capacity() + 3);
    for span in 0..span_count {
        let fraction0 = span as f64 / span_count as f64;
        let fraction1 = (span + 1) as f64 / span_count as f64;
        let angle0 = angles[0] + (angles[1] - angles[0]) * fraction0;
        let angle1 = angles[0] + (angles[1] - angles[0]) * fraction1;
        let middle = (angle0 + angle1) * 0.5;
        let middle_weight = ((angle1 - angle0) * 0.5).cos();
        if middle_weight <= f64::EPSILON {
            return None;
        }
        if span == 0 {
            control_points.push(point3(circle_point(
                center,
                direction_x,
                direction_y,
                radius,
                angle0,
            )));
            weights.push(1.0);
        }
        control_points.push(point3(circle_point(
            center,
            direction_x,
            direction_y,
            radius / middle_weight,
            middle,
        )));
        weights.push(middle_weight);
        control_points.push(point3(circle_point(
            center,
            direction_x,
            direction_y,
            radius,
            angle1,
        )));
        weights.push(1.0);
        append_quadratic_span_knots(&mut knots, interval, span, span_count);
    }
    Some(NurbsCurve {
        degree: 2,
        knots,
        control_points,
        weights: Some(weights),
        periodic: false,
    })
}

fn revolve_nurbs(
    profile: &NurbsCurve,
    axis_origin: [f64; 3],
    axis_direction: [f64; 3],
    angular_interval: [f64; 2],
    native_interval: [f64; 2],
) -> Option<NurbsSurface> {
    let span_count =
        ((angular_interval[1] - angular_interval[0]).abs() / std::f64::consts::FRAC_PI_2).ceil();
    if !span_count.is_finite() || span_count > crate::MAX_EXACT_ARC_SPANS as f64 {
        return None;
    }
    let span_count = (span_count as usize).max(1);
    let angular_count = span_count.checked_mul(2)?.checked_add(1)?;
    let mut angles = Vec::with_capacity(angular_count);
    let mut angular_weights = Vec::with_capacity(angular_count);
    let mut v_knots = Vec::with_capacity(angular_count + 3);
    for span in 0..span_count {
        let fraction0 = span as f64 / span_count as f64;
        let fraction1 = (span + 1) as f64 / span_count as f64;
        let angle0 = angular_interval[0] + (angular_interval[1] - angular_interval[0]) * fraction0;
        let angle1 = angular_interval[0] + (angular_interval[1] - angular_interval[0]) * fraction1;
        let middle = (angle0 + angle1) * 0.5;
        let middle_weight = ((angle1 - angle0) * 0.5).cos();
        if middle_weight <= f64::EPSILON {
            return None;
        }
        if span == 0 {
            angles.push((angle0, 1.0));
            angular_weights.push(1.0);
        }
        angles.push((middle, 1.0 / middle_weight));
        angular_weights.push(middle_weight);
        angles.push((angle1, 1.0));
        angular_weights.push(1.0);
        append_quadratic_span_knots(&mut v_knots, native_interval, span, span_count);
    }
    let profile_weights = profile
        .weights
        .clone()
        .unwrap_or_else(|| vec![1.0; profile.control_points.len()]);
    let mut control_points = Vec::with_capacity(profile.control_points.len() * angular_count);
    let mut weights = Vec::with_capacity(control_points.capacity());
    for (profile_point, profile_weight) in profile.control_points.iter().zip(profile_weights) {
        let relative = [
            profile_point.x - axis_origin[0],
            profile_point.y - axis_origin[1],
            profile_point.z - axis_origin[2],
        ];
        let axial = scale(axis_direction, dot(relative, axis_direction));
        let radial = subtract(relative, axial);
        for ((angle, radial_scale), angular_weight) in
            angles.iter().copied().zip(angular_weights.iter().copied())
        {
            let rotated = rotate_vector(radial, axis_direction, angle);
            control_points.push(point3(add(
                axis_origin,
                add(axial, scale(rotated, radial_scale)),
            )));
            weights.push(profile_weight * angular_weight);
        }
    }
    Some(NurbsSurface {
        u_degree: profile.degree,
        v_degree: 2,
        u_knots: profile.knots.clone(),
        v_knots,
        u_count: u32::try_from(profile.control_points.len()).ok()?,
        v_count: u32::try_from(angular_count).ok()?,
        control_points,
        weights: Some(weights),
        u_periodic: false,
        v_periodic: false,
    })
}

fn append_quadratic_span_knots(
    knots: &mut Vec<f64>,
    interval: [f64; 2],
    span: usize,
    span_count: usize,
) {
    let start = interval[0] + (interval[1] - interval[0]) * span as f64 / span_count as f64;
    let end = interval[0] + (interval[1] - interval[0]) * (span + 1) as f64 / span_count as f64;
    if span == 0 {
        knots.extend([start, start, start]);
    } else {
        knots.extend([start, start]);
    }
    if span + 1 == span_count {
        knots.extend([end, end, end]);
    }
}

fn circle_point(
    center: [f64; 3],
    direction_x: [f64; 3],
    direction_y: [f64; 3],
    radius: f64,
    angle: f64,
) -> [f64; 3] {
    add(
        center,
        scale(
            add(
                scale(direction_x, angle.cos()),
                scale(direction_y, angle.sin()),
            ),
            radius,
        ),
    )
}

fn rotate_vector(value: [f64; 3], axis: [f64; 3], angle: f64) -> [f64; 3] {
    add(
        add(
            scale(value, angle.cos()),
            scale(cross(axis, value), angle.sin()),
        ),
        scale(axis, dot(axis, value) * (1.0 - angle.cos())),
    )
}

fn add(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

fn subtract(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn scale(value: [f64; 3], scalar: f64) -> [f64; 3] {
    [value[0] * scalar, value[1] * scalar, value[2] * scalar]
}

fn point3(value: [f64; 3]) -> Point3 {
    Point3::new(value[0], value[1], value[2])
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

fn body_kind_if_owned(graph: &B5Graph) -> Option<BodyKind> {
    let mut face_ids = HashSet::new();
    let mut loop_owners = HashMap::<u32, usize>::new();
    for face in &graph.faces {
        if !face_ids.insert(face.object_id) || face.loops.is_empty() {
            return None;
        }
        for loop_id in &face.loops {
            *loop_owners.entry(*loop_id).or_default() += 1;
        }
    }
    if loop_owners.len() != graph.loops.len()
        || loop_owners.values().any(|owners| *owners != 1)
        || graph.loops.iter().any(|(loop_id, loop_)| {
            loop_id != &loop_.object_id || !loop_owners.contains_key(loop_id)
        })
    {
        return None;
    }

    let vertex_count = graph
        .vertex_points
        .len()
        .checked_add(graph.logical_vertex_points.len())?;
    let mut uses = HashMap::<u32, usize>::new();
    for edge in graph.loops.values().flat_map(|loop_| &loop_.edges) {
        let endpoints = graph.edge_vertices.get(edge)?;
        if endpoints.iter().any(|endpoint| *endpoint >= vertex_count) {
            return None;
        }
        *uses.entry(*edge).or_default() += 1;
    }
    Some(if uses.values().any(|count| *count > 2) {
        BodyKind::General
    } else if !uses.is_empty() && uses.values().all(|count| *count == 2) {
        BodyKind::Solid
    } else {
        BodyKind::Sheet
    })
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
    use super::{
        b5_edge_support_definition, body_kind_if_owned, cylinder_helix, cylinder_point,
        edge_pcurve_parameters, isocurve_endpoint_parameters, lifted_curve_geometry,
        merge_curve_plan, neutral_pcurve_point, ordered_subrange, oriented_circle_plan,
        oriented_line_plan, oriented_nurbs_range, rational_arc, revolution_surface, revolve_nurbs,
        transfer, transfer_vertex_tolerances, CurvePlan, SurfacePlan,
    };
    use crate::b5::{
        B5Face, B5Graph, B5Loop, B5ParameterIncidence, B5Pcurve, B5Profile, B5Surface,
    };
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::eval::surface_point;
    use cadmpeg_ir::geometry::{
        CurveGeometry, NurbsCurve, PcurveGeometry, ProceduralCurveDefinition, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{SurfaceId, UnknownId};
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::topology::BodyKind;
    use cadmpeg_ir::units::Units;
    use cadmpeg_ir::AnnotationBuilder;
    use std::collections::{BTreeMap, HashMap, HashSet};

    #[test]
    fn occurrence_interval_orders_and_bounds_native_stations() {
        assert_eq!(ordered_subrange([8.0, 2.0], [0.0, 10.0]), Some([2.0, 8.0]));
        assert_eq!(
            ordered_subrange([-5e-10, 10.0 + 5e-10], [0.0, 10.0]),
            Some([0.0, 10.0])
        );
        assert!(ordered_subrange([2.0, 2.0], [0.0, 10.0]).is_none());
        assert!(ordered_subrange([-2e-9, 8.0], [0.0, 10.0]).is_none());
        assert!(ordered_subrange([2.0, 12.0], [0.0, 10.0]).is_none());
    }

    #[test]
    fn edge_parameters_follow_ordered_edge_refs_for_a_closed_vertex() {
        let mut graph = B5Graph {
            complete: false,
            records: Vec::new(),
            faces: Vec::new(),
            loops: BTreeMap::new(),
            pcurves: BTreeMap::new(),
            opaque_pcurves: BTreeMap::new(),
            implicit_pcurves: BTreeMap::new(),
            surfaces: BTreeMap::new(),
            offset_surfaces: BTreeMap::new(),
            supported_surfaces: BTreeMap::new(),
            parameter_incidences: BTreeMap::from([
                (
                    40,
                    B5ParameterIncidence {
                        object_id: 40,
                        curves: vec![20],
                        parameters: vec![0.0],
                        controls: vec![0],
                    },
                ),
                (
                    41,
                    B5ParameterIncidence {
                        object_id: 41,
                        curves: vec![20],
                        parameters: vec![1.0],
                        controls: vec![0],
                    },
                ),
            ]),
            vertex_points: Vec::new(),
            logical_vertex_points: vec![[0.0, 0.0, 0.0]],
            logical_vertex_refs: vec![50],
            edge_vertices: BTreeMap::from([(30, [0, 0])]),
            edge_parameter_incidences: BTreeMap::from([(30, [40, 41])]),
            vertex_tolerances: BTreeMap::new(),
            profiles: BTreeMap::new(),
        };

        assert_eq!(edge_pcurve_parameters(&graph, 30, 20), Some([0.0, 1.0]));
        graph.edge_parameter_incidences.insert(30, [41, 40]);
        assert_eq!(edge_pcurve_parameters(&graph, 30, 20), Some([1.0, 0.0]));
    }

    #[test]
    fn repeated_source_pcurve_has_independently_trimmed_occurrence_carriers() {
        let graph = B5Graph {
            complete: true,
            records: Vec::new(),
            faces: vec![B5Face {
                object_id: 1,
                surface: 10,
                loops: vec![2],
            }],
            loops: BTreeMap::from([(
                2,
                B5Loop {
                    object_id: 2,
                    pcurves: vec![20, 20, 20],
                    edges: vec![30, 31, 32],
                    surface: 10,
                },
            )]),
            pcurves: BTreeMap::from([(
                20,
                B5Pcurve {
                    object_id: 20,
                    surface: 10,
                    degree: 1,
                    distinct_knots: vec![0.0, 1.0],
                    multiplicities: vec![2, 2],
                    control_points: vec![[0.0, 0.0], [1.0, 0.0]],
                    weights: None,
                    lifted_endpoints: None,
                },
            )]),
            opaque_pcurves: BTreeMap::new(),
            implicit_pcurves: BTreeMap::new(),
            surfaces: BTreeMap::from([(
                10,
                B5Surface::Plane {
                    origin: [0.0, 0.0, 0.0],
                    direction_u: [1.0, 0.0, 0.0],
                    direction_v: [0.0, 1.0, 0.0],
                },
            )]),
            offset_surfaces: BTreeMap::new(),
            supported_surfaces: BTreeMap::new(),
            parameter_incidences: BTreeMap::from([
                (
                    40,
                    B5ParameterIncidence {
                        object_id: 40,
                        curves: vec![20],
                        parameters: vec![0.0],
                        controls: vec![0],
                    },
                ),
                (
                    41,
                    B5ParameterIncidence {
                        object_id: 41,
                        curves: vec![20],
                        parameters: vec![0.5],
                        controls: vec![0],
                    },
                ),
                (
                    42,
                    B5ParameterIncidence {
                        object_id: 42,
                        curves: vec![20],
                        parameters: vec![1.0],
                        controls: vec![0],
                    },
                ),
            ]),
            vertex_points: Vec::new(),
            logical_vertex_points: vec![[0.0, 0.0, 0.0], [0.5, 0.0, 0.0], [1.0, 0.0, 0.0]],
            logical_vertex_refs: vec![50, 51, 52],
            edge_vertices: BTreeMap::from([(30, [0, 1]), (31, [1, 2]), (32, [2, 0])]),
            edge_parameter_incidences: BTreeMap::from([
                (30, [40, 41]),
                (31, [41, 42]),
                (32, [42, 40]),
            ]),
            vertex_tolerances: BTreeMap::new(),
            profiles: BTreeMap::new(),
        };
        let mut ir = CadIr::empty(Units::default());

        assert!(transfer(
            &mut ir,
            &mut AnnotationBuilder::new(),
            graph,
            &UnknownId("catia:test-payload".to_string()),
        ));
        assert_eq!(ir.model.pcurves.len(), 3);
        assert_eq!(ir.model.coedges.len(), 3);
        assert_eq!(
            ir.model
                .pcurves
                .iter()
                .map(|pcurve| pcurve.parameter_range)
                .collect::<Vec<_>>(),
            [Some([0.0, 0.5]), Some([0.0, 1.0]), Some([0.5, 1.0])]
        );
        assert_eq!(
            ir.model
                .coedges
                .iter()
                .filter_map(|coedge| coedge.pcurve.as_ref().map(|id| id.0.as_str()))
                .collect::<Vec<_>>(),
            [
                "catia:b5:pcurve#20@0",
                "catia:b5:pcurve#20@2",
                "catia:b5:pcurve#20@1",
            ]
        );
    }

    #[test]
    fn edge_supports_preserve_one_sided_and_intersection_constructions() {
        let surfaces = HashMap::from([
            (10, SurfaceId("surface-10".to_string())),
            (11, SurfaceId("surface-11".to_string())),
        ]);
        let pcurve_20 = PcurveGeometry::Line {
            origin: Point2::new(0.0, 0.0),
            direction: Point2::new(1.0, 0.0),
        };
        let pcurve_21 = PcurveGeometry::Line {
            origin: Point2::new(0.0, 1.0),
            direction: Point2::new(1.0, 0.0),
        };
        let pcurves = BTreeMap::from([
            (20, (pcurve_20.clone(), false, [2.0, 4.0])),
            (21, (pcurve_21.clone(), false, [2.0, 4.0])),
        ]);
        let (_, _, one_sided) =
            b5_edge_support_definition(&[(10, 20, [2.0, 4.0])], &surfaces, &pcurves)
                .expect("one-sided surface curve");
        assert!(matches!(
            one_sided,
            ProceduralCurveDefinition::SurfaceCurve { context, .. }
                if context.parameter_range == [2.0, 4.0]
                    && context.sides[0].surface == Some(surfaces[&10].clone())
                    && context.sides[0].pcurve == Some(pcurve_20)
                    && context.sides[1].surface.is_none()
        ));

        let (_, _, intersection) = b5_edge_support_definition(
            &[(10, 20, [2.0, 4.0]), (11, 21, [2.0, 4.0])],
            &surfaces,
            &pcurves,
        )
        .expect("two-sided intersection");
        assert!(matches!(
            intersection,
            ProceduralCurveDefinition::Intersection { context, .. }
                if context.parameter_range == [2.0, 4.0]
                    && context.sides[1].surface == Some(surfaces[&11].clone())
                    && context.sides[1].pcurve == Some(pcurve_21)
        ));
        assert!(b5_edge_support_definition(
            &[(10, 20, [2.0, 4.0]), (11, 21, [2.0, 5.0])],
            &surfaces,
            &pcurves,
        )
        .is_none());
    }

    #[test]
    fn exact_revolution_builders_reject_unbounded_subdivision_counts() {
        assert!(rational_arc(
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            1.0e-300,
            [0.0, 1.0],
        )
        .is_none());
        let profile = cadmpeg_ir::geometry::NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 0.0, 1.0)],
            weights: None,
            periodic: false,
        };
        assert!(revolve_nurbs(
            &profile,
            [0.0; 3],
            [0.0, 0.0, 1.0],
            [0.0, 1.0e300],
            [0.0, 1.0],
        )
        .is_none());
    }

    #[test]
    fn body_kind_requires_unique_complete_loop_ownership() {
        let mut graph = B5Graph {
            complete: true,
            records: Vec::new(),
            faces: vec![B5Face {
                object_id: 1,
                surface: 10,
                loops: vec![2],
            }],
            loops: BTreeMap::from([(
                2,
                B5Loop {
                    object_id: 2,
                    pcurves: vec![4],
                    edges: vec![3],
                    surface: 10,
                },
            )]),
            pcurves: BTreeMap::new(),
            opaque_pcurves: BTreeMap::new(),
            implicit_pcurves: BTreeMap::new(),
            surfaces: BTreeMap::new(),
            offset_surfaces: BTreeMap::new(),
            supported_surfaces: BTreeMap::new(),
            parameter_incidences: BTreeMap::new(),
            vertex_points: vec![[0.0; 3], [1.0, 0.0, 0.0]],
            logical_vertex_points: Vec::new(),
            logical_vertex_refs: Vec::new(),
            edge_vertices: BTreeMap::from([(3, [0, 1])]),
            edge_parameter_incidences: BTreeMap::new(),
            vertex_tolerances: BTreeMap::new(),
            profiles: BTreeMap::new(),
        };

        assert_eq!(body_kind_if_owned(&graph), Some(BodyKind::Sheet));
        graph.faces[0].loops.push(2);
        assert_eq!(body_kind_if_owned(&graph), None);
        graph.faces[0].loops.pop();
        graph.faces.push(B5Face {
            object_id: 5,
            surface: 10,
            loops: vec![2],
        });
        assert_eq!(body_kind_if_owned(&graph), None);
        graph.faces.pop();
        graph.edge_vertices.insert(3, [0, 2]);
        assert_eq!(body_kind_if_owned(&graph), None);
    }

    #[test]
    fn emitted_carriers_determine_logical_vertex_tolerance() {
        let graph = B5Graph {
            complete: true,
            records: Vec::new(),
            faces: Vec::new(),
            loops: BTreeMap::from([(
                1,
                B5Loop {
                    object_id: 1,
                    pcurves: vec![2],
                    edges: vec![3],
                    surface: 4,
                },
            )]),
            pcurves: BTreeMap::new(),
            opaque_pcurves: BTreeMap::new(),
            implicit_pcurves: BTreeMap::new(),
            surfaces: BTreeMap::new(),
            offset_surfaces: BTreeMap::new(),
            supported_surfaces: BTreeMap::new(),
            parameter_incidences: BTreeMap::from([
                (
                    20,
                    B5ParameterIncidence {
                        object_id: 20,
                        curves: vec![2],
                        parameters: vec![0.25],
                        controls: vec![0],
                    },
                ),
                (
                    21,
                    B5ParameterIncidence {
                        object_id: 21,
                        curves: vec![2],
                        parameters: vec![0.75],
                        controls: vec![0],
                    },
                ),
            ]),
            vertex_points: Vec::new(),
            logical_vertex_points: vec![[0.25, 0.0, 1e-4], [0.75, 0.0, 0.0]],
            logical_vertex_refs: vec![10, 11],
            edge_vertices: BTreeMap::from([(3, [0, 1])]),
            edge_parameter_incidences: BTreeMap::from([(3, [20, 21])]),
            vertex_tolerances: BTreeMap::new(),
            profiles: BTreeMap::new(),
        };
        let pcurves = BTreeMap::from([(
            2,
            (
                PcurveGeometry::Nurbs {
                    degree: 1,
                    knots: vec![0.0, 0.0, 1.0, 1.0],
                    control_points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
                    weights: None,
                    periodic: false,
                },
                false,
                [0.0, 1.0],
            ),
        )]);
        let surfaces = BTreeMap::from([(
            4,
            SurfacePlan {
                geometry: SurfaceGeometry::Plane {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    normal: Vector3::new(0.0, 0.0, 1.0),
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
                },
                revolution: None,
            },
        )]);

        let tolerances = transfer_vertex_tolerances(&graph, &pcurves, &surfaces);
        assert!((tolerances[&0] - (1e-4 + 1e-9)).abs() < 1e-12);
        assert!(!tolerances.contains_key(&1));
    }

    #[test]
    fn cylinder_pcurve_arc_length_normalizes_to_neutral_angle() {
        let surface = B5Surface::Cylinder {
            origin: [0.0, 0.0, 0.0],
            reference_x: [1.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: 2.0,
        };
        let point = neutral_pcurve_point([std::f64::consts::PI, 3.0], &surface);
        assert_eq!(point.u, std::f64::consts::FRAC_PI_2);
        assert_eq!(point.v, 3.0);
    }

    #[test]
    fn revolution_cache_preserves_native_profile_and_arc_length_chart() {
        let profile = B5Profile::Line {
            point: [2.0, 0.0, 0.0],
            direction: [0.0, 0.0, 1.0],
        };
        let (surface, plan) = revolution_surface(
            Some(&profile),
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            1.0,
            Some([[-1.0, 1.0], [0.0, std::f64::consts::PI]]),
        )
        .expect("exact revolution cache");
        assert_eq!(plan.parameter_interval, [-1.0, 1.0]);
        assert_eq!(plan.angular_interval, [0.0, std::f64::consts::PI]);
        let evaluated = surface_point(
            &SurfaceGeometry::Nurbs(surface),
            0.5,
            std::f64::consts::FRAC_PI_2,
        )
        .expect("surface point");
        assert!(evaluated.x.abs() < 1e-12);
        assert!((evaluated.y - 2.0).abs() < 1e-12);
        assert!((evaluated.z - 0.5).abs() < 1e-12);
    }

    #[test]
    fn affine_and_isoparametric_pcurves_produce_exact_curve_carriers() {
        let pcurve = B5Pcurve {
            object_id: 1,
            surface: 2,
            degree: 1,
            distinct_knots: vec![0.0, 1.0],
            multiplicities: vec![2, 2],
            control_points: vec![[0.0, 2.0], [3.0, 2.0]],
            weights: None,
            lifted_endpoints: None,
        };
        let plane = B5Surface::Plane {
            origin: [1.0, 2.0, 3.0],
            direction_u: [1.0, 0.0, 0.0],
            direction_v: [0.0, 1.0, 0.0],
        };
        let Some(CurveGeometry::Nurbs(curve)) = lifted_curve_geometry(&pcurve, &plane) else {
            panic!("plane lift must be NURBS");
        };
        assert_eq!(curve.control_points[0], Point3::new(1.0, 4.0, 3.0));
        assert_eq!(curve.control_points[1], Point3::new(4.0, 4.0, 3.0));

        let cylinder = B5Surface::Cylinder {
            origin: [0.0, 0.0, 0.0],
            reference_x: [1.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: 2.0,
        };
        assert!(matches!(
            lifted_curve_geometry(&pcurve, &cylinder),
            Some(CurveGeometry::Circle { radius: 2.0, .. })
        ));
        let meridian = B5Pcurve {
            control_points: vec![[1.0, -2.0], [1.0, 4.0]],
            ..pcurve
        };
        assert!(matches!(
            lifted_curve_geometry(&meridian, &cylinder),
            Some(CurveGeometry::Line { .. })
        ));
    }

    #[test]
    fn affine_plane_lift_preserves_pcurve_weights() {
        let pcurve = B5Pcurve {
            object_id: 1,
            surface: 2,
            degree: 2,
            distinct_knots: vec![0.0, 1.0],
            multiplicities: vec![3, 3],
            control_points: vec![[1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            weights: Some(vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]),
            lifted_endpoints: None,
        };
        let plane = B5Surface::Plane {
            origin: [0.0, 0.0, 2.0],
            direction_u: [1.0, 0.0, 0.0],
            direction_v: [0.0, 1.0, 0.0],
        };
        let Some(CurveGeometry::Nurbs(curve)) = lifted_curve_geometry(&pcurve, &plane) else {
            panic!("expected lifted rational curve");
        };
        assert_eq!(curve.weights, pcurve.weights);
        assert!(curve.control_points.iter().all(|point| point.z == 2.0));
    }

    #[test]
    fn affine_lift_range_orients_and_trims_the_nurbs_carrier() {
        let geometry = CurveGeometry::Nurbs(NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 10.0, 10.0],
            control_points: vec![Point3::new(0.0, 0.0, 2.0), Point3::new(10.0, 0.0, 2.0)],
            weights: None,
            periodic: false,
        });
        let forward = oriented_nurbs_range(
            geometry.clone(),
            [2.0, 8.0],
            [2.0, 0.0, 2.0],
            [8.0, 0.0, 2.0],
        )
        .expect("forward trimmed range");
        assert_eq!(forward.geometry, geometry);
        assert_eq!(forward.parameter_range, Some([2.0, 8.0]));
        assert_eq!(forward.edge_tolerance, None);

        let reversed = oriented_nurbs_range(
            geometry.clone(),
            [8.0, 2.0],
            [8.0, 0.0, 2.0],
            [2.0, 0.0, 2.0],
        )
        .expect("reversed trimmed range");
        assert_eq!(reversed.parameter_range, Some([2.0, 8.0]));
        let CurveGeometry::Nurbs(reversed) = reversed.geometry else {
            unreachable!();
        };
        assert_eq!(
            reversed.control_points,
            [Point3::new(10.0, 0.0, 2.0), Point3::new(0.0, 0.0, 2.0)]
        );
        assert!(
            oriented_nurbs_range(geometry, [2.0, 8.0], [3.0, 0.0, 2.0], [8.0, 0.0, 2.0]).is_none()
        );

        let tolerant = oriented_nurbs_range(
            CurveGeometry::Nurbs(NurbsCurve {
                degree: 1,
                knots: vec![0.0, 0.0, 10.0, 10.0],
                control_points: vec![Point3::new(0.0, 0.0, 2.0), Point3::new(10.0, 0.0, 2.0)],
                weights: None,
                periodic: false,
            }),
            [2.0, 8.0],
            [2.0, 0.0, 2.0 + 1e-4],
            [8.0, 0.0, 2.0],
        )
        .expect("tolerant trimmed range");
        assert!((tolerant.edge_tolerance.expect("edge tolerance") - (1e-4 + 1e-9)).abs() < 1e-15);
    }

    #[test]
    fn isocurve_range_uses_monotone_varying_surface_coordinate() {
        let pcurve = B5Pcurve {
            object_id: 1,
            surface: 2,
            degree: 2,
            distinct_knots: vec![0.0, 1.0],
            multiplicities: vec![3, 3],
            control_points: vec![[4.0, 2.0], [4.0, 6.0], [4.0, 10.0]],
            weights: Some(vec![1.0, 2.0, 1.0]),
            lifted_endpoints: None,
        };
        assert_eq!(
            isocurve_endpoint_parameters(&pcurve, [0.25, 0.75]),
            Some([50.0 / 11.0, 82.0 / 11.0])
        );

        let decreasing = B5Pcurve {
            control_points: pcurve.control_points.iter().copied().rev().collect(),
            ..pcurve.clone()
        };
        assert_eq!(
            isocurve_endpoint_parameters(&decreasing, [0.25, 0.75]),
            Some([82.0 / 11.0, 50.0 / 11.0])
        );

        let turnback = B5Pcurve {
            control_points: vec![[4.0, 2.0], [4.0, 10.0], [4.0, 6.0]],
            ..pcurve.clone()
        };
        assert!(isocurve_endpoint_parameters(&turnback, [0.0, 1.0]).is_none());

        let nonpositive_weight = B5Pcurve {
            weights: Some(vec![1.0, 0.0, 1.0]),
            ..pcurve
        };
        assert!(isocurve_endpoint_parameters(&nonpositive_weight, [0.0, 1.0]).is_none());
    }

    #[test]
    fn analytic_line_range_uses_oriented_signed_distance() {
        let line = CurveGeometry::Line {
            origin: Point3::new(1.0, 2.0, 3.0),
            direction: Vector3::new(0.0, 0.0, 2.0),
        };
        let forward = oriented_line_plan(&line, [1.0, 2.0, 5.0], [1.0, 2.0, 9.0])
            .expect("forward line range");
        assert_eq!(forward.parameter_range, Some([2.0, 6.0]));
        assert!(matches!(
            forward.geometry,
            CurveGeometry::Line { direction, .. }
                if direction == Vector3::new(0.0, 0.0, 1.0)
        ));

        let reversed = oriented_line_plan(&line, [1.0, 2.0, 9.0], [1.0, 2.0, 5.0])
            .expect("reversed line range");
        assert_eq!(reversed.parameter_range, Some([-6.0, -2.0]));
        assert!(matches!(
            reversed.geometry,
            CurveGeometry::Line { direction, .. }
                if direction == Vector3::new(0.0, 0.0, -1.0)
        ));
        let tolerant = oriented_line_plan(&line, [1.001, 2.0, 5.0], [1.0, 2.0, 9.0])
            .expect("tolerant line endpoints");
        assert!(tolerant.edge_tolerance.is_some_and(|value| value > 0.001));
        assert_eq!(tolerant.cache_fit_tolerance, None);
        assert!(oriented_line_plan(&line, [1.01, 2.0, 5.0], [1.0, 2.0, 9.0]).is_none());
        assert!(oriented_line_plan(&line, [1.0, 2.0, 5.0], [1.0, 2.0, 5.0]).is_none());
    }

    #[test]
    fn isoparametric_circle_range_preserves_winding_and_seams() {
        let cylinder = B5Surface::Cylinder {
            origin: [0.0, 0.0, 0.0],
            reference_x: [1.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: 2.0,
        };
        let pcurve = B5Pcurve {
            object_id: 1,
            surface: 2,
            degree: 1,
            distinct_knots: vec![0.0, 1.0],
            multiplicities: vec![2, 2],
            control_points: vec![[11.0, 3.0], [13.0, 3.0]],
            weights: None,
            lifted_endpoints: None,
        };
        let geometry = lifted_curve_geometry(&pcurve, &cylinder).expect("cylinder latitude");
        let edge_start = cylinder_point(
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            2.0,
            pcurve.control_points[0],
        );
        let edge_end = cylinder_point(
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            2.0,
            pcurve.control_points[1],
        );
        let forward = oriented_circle_plan(
            &pcurve,
            &cylinder,
            &geometry,
            [0.0, 1.0],
            edge_start,
            edge_end,
        )
        .expect("seam-crossing circle range");
        assert_eq!(forward.parameter_range, Some([5.5, 6.5]));

        let reversed_pcurve = B5Pcurve {
            control_points: pcurve.control_points.iter().copied().rev().collect(),
            ..pcurve.clone()
        };
        let reversed = oriented_circle_plan(
            &reversed_pcurve,
            &cylinder,
            &geometry,
            [0.0, 1.0],
            edge_end,
            edge_start,
        )
        .expect("reversed circle range");
        let [start, end] = reversed.parameter_range.expect("canonical range");
        assert!(start >= 0.0 && end > start && end - start == 1.0);
        assert!(matches!(
            reversed.geometry,
            CurveGeometry::Circle { axis, .. } if axis == Vector3::new(0.0, 0.0, -1.0)
        ));

        let turnback = B5Pcurve {
            degree: 2,
            multiplicities: vec![3, 3],
            control_points: vec![[0.0, 3.0], [4.0, 3.0], [2.0, 3.0]],
            ..pcurve
        };
        let turnback_geometry =
            lifted_curve_geometry(&turnback, &cylinder).expect("turnback latitude locus");
        let turnback_end =
            cylinder_point([0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 2.0, [2.0, 3.0]);
        assert!(oriented_circle_plan(
            &turnback,
            &cylinder,
            &turnback_geometry,
            [0.0, 1.0],
            [2.0, 0.0, 3.0],
            turnback_end,
        )
        .is_none());

        let half_angle = std::f64::consts::FRAC_PI_6;
        let cone = B5Surface::Cone {
            apex: [0.0; 3],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            half_angle,
            angular_offset: 0.0,
            slant_range: [-4.0, 0.0],
            angular_scale: 2.0,
        };
        let cone_pcurve = B5Pcurve {
            control_points: vec![[0.0, -4.0], [2.0, -4.0]],
            ..reversed_pcurve
        };
        let cone_geometry =
            lifted_curve_geometry(&cone_pcurve, &cone).expect("signed cone latitude");
        let cone_point = |angle: f64| {
            [
                -4.0 * half_angle.sin() * angle.cos(),
                -4.0 * half_angle.sin() * angle.sin(),
                -4.0 * half_angle.cos(),
            ]
        };
        let signed = oriented_circle_plan(
            &cone_pcurve,
            &cone,
            &cone_geometry,
            [0.0, 1.0],
            cone_point(0.0),
            cone_point(1.0),
        )
        .expect("normalized signed-radius circle");
        assert!(matches!(
            signed.geometry,
            CurveGeometry::Circle { radius, ref_direction, .. }
                if radius == 2.0 && ref_direction == Vector3::new(-1.0, 0.0, 0.0)
        ));
    }

    #[test]
    fn edge_curve_plans_merge_proofs_and_discard_conflicting_carriers() {
        let geometry = CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        };
        let mut plans = HashMap::new();
        let mut conflicts = HashSet::new();
        merge_curve_plan(
            &mut plans,
            &mut conflicts,
            4,
            CurvePlan {
                geometry: geometry.clone(),
                parameter_range: None,
                edge_tolerance: None,
                cache_fit_tolerance: None,
            },
        );
        merge_curve_plan(
            &mut plans,
            &mut conflicts,
            4,
            CurvePlan {
                geometry,
                parameter_range: Some([2.0, 8.0]),
                edge_tolerance: None,
                cache_fit_tolerance: None,
            },
        );
        assert_eq!(plans[&4].parameter_range, Some([2.0, 8.0]));

        let conflicting = CurvePlan {
            geometry: CurveGeometry::Line {
                origin: Point3::new(0.0, 1.0, 0.0),
                direction: Vector3::new(1.0, 0.0, 0.0),
            },
            parameter_range: Some([2.0, 8.0]),
            edge_tolerance: None,
            cache_fit_tolerance: None,
        };
        merge_curve_plan(&mut plans, &mut conflicts, 4, conflicting.clone());
        assert!(!plans.contains_key(&4));
        assert!(conflicts.contains(&4));
        merge_curve_plan(&mut plans, &mut conflicts, 4, conflicting);
        assert!(!plans.contains_key(&4));
    }

    #[test]
    fn cone_chart_normalizes_arc_length_and_slant_coordinates() {
        let half_angle = std::f64::consts::FRAC_PI_6;
        let cone = B5Surface::Cone {
            apex: [0.0, 0.0, 0.0],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            half_angle,
            angular_offset: 0.0,
            slant_range: [2.0, 8.0],
            angular_scale: 3.0,
        };
        let pcurve = B5Pcurve {
            object_id: 1,
            surface: 2,
            degree: 1,
            distinct_knots: vec![0.0, 3.0 * std::f64::consts::PI],
            multiplicities: vec![2, 2],
            control_points: vec![[0.0, 4.0], [3.0 * std::f64::consts::PI, 4.0]],
            weights: None,
            lifted_endpoints: None,
        };
        assert_eq!(
            pcurve
                .control_points
                .iter()
                .map(|point| neutral_pcurve_point(*point, &cone))
                .collect::<Vec<_>>(),
            [
                Point2::new(0.0, 2.0 * half_angle.cos()),
                Point2::new(std::f64::consts::PI, 2.0 * half_angle.cos()),
            ]
        );
        let Some(CurveGeometry::Circle {
            center,
            radius,
            axis,
            ..
        }) = lifted_curve_geometry(&pcurve, &cone)
        else {
            panic!("expected cone latitude circle");
        };
        assert_eq!(center, Point3::new(0.0, 0.0, 4.0 * half_angle.cos()));
        assert_eq!(axis, Vector3::new(0.0, 0.0, 1.0));
        assert!((radius - 2.0).abs() < 1e-12);
    }

    #[test]
    fn torus_chart_lifts_meridians_and_latitudes_exactly() {
        let torus = B5Surface::Torus {
            center: [0.0, 0.0, 0.0],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            major_radius: 5.0,
            minor_radius: 2.0,
            major_scale: 5.0,
            minor_scale: 2.0,
        };
        let base = B5Pcurve {
            object_id: 1,
            surface: 2,
            degree: 1,
            distinct_knots: vec![0.0, 1.0],
            multiplicities: vec![2, 2],
            control_points: vec![[0.0, 0.0], [0.0, 4.0 * std::f64::consts::PI]],
            weights: None,
            lifted_endpoints: None,
        };
        assert_eq!(
            neutral_pcurve_point([5.0 * std::f64::consts::PI, 2.0], &torus),
            Point2::new(std::f64::consts::PI, 1.0)
        );
        let Some(CurveGeometry::Circle {
            center,
            axis,
            radius,
            ..
        }) = lifted_curve_geometry(&base, &torus)
        else {
            panic!("expected meridian circle");
        };
        assert_eq!(center, Point3::new(5.0, 0.0, 0.0));
        assert_eq!(axis, Vector3::new(0.0, -1.0, 0.0));
        assert_eq!(radius, 2.0);

        let latitude = B5Pcurve {
            control_points: vec![[0.0, 0.0], [10.0 * std::f64::consts::PI, 0.0]],
            ..base
        };
        let Some(CurveGeometry::Circle {
            center,
            axis,
            radius,
            ..
        }) = lifted_curve_geometry(&latitude, &torus)
        else {
            panic!("expected latitude circle");
        };
        assert_eq!(center, Point3::new(0.0, 0.0, 0.0));
        assert_eq!(axis, Vector3::new(0.0, 0.0, 1.0));
        assert_eq!(radius, 7.0);
    }

    #[test]
    fn tensor_surface_contraction_preserves_exact_isocurve() {
        let surface = cadmpeg_ir::geometry::NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            u_count: 2,
            v_count: 2,
            control_points: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(2.0, 0.0, 0.0),
                Point3::new(2.0, 1.0, 2.0),
            ],
            weights: None,
            u_periodic: false,
            v_periodic: false,
        };
        let curve =
            crate::geometry::nurbs_surface_isocurve(&surface, 0.25, true).expect("u isocurve");
        assert_eq!(curve.degree, 1);
        assert_eq!(curve.knots, surface.v_knots);
        assert_eq!(curve.control_points[0], Point3::new(0.5, 0.0, 0.0));
        assert_eq!(curve.control_points[1], Point3::new(0.5, 1.0, 0.5));
    }

    #[test]
    fn affine_cylinder_pcurve_preserves_exact_helix_construction() {
        let pcurve = B5Pcurve {
            object_id: 1,
            surface: 2,
            degree: 1,
            distinct_knots: vec![0.0, 1.0],
            multiplicities: vec![2, 2],
            control_points: vec![[0.0, 3.0], [4.0, 7.0]],
            weights: None,
            lifted_endpoints: None,
        };
        let cylinder = B5Surface::Cylinder {
            origin: [0.0, 0.0, 0.0],
            reference_x: [1.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: 2.0,
        };
        let end = [2.0 * 2.0_f64.cos(), 2.0 * 2.0_f64.sin(), 7.0];
        let Some(plan) = cylinder_helix(&pcurve, &cylinder, [0.0, 1.0], [2.0, 0.0, 3.0], end)
        else {
            panic!("degree-one cylinder helix");
        };
        let ProceduralCurveDefinition::Helix {
            angle_range,
            center,
            pitch,
            apex_factor,
            ..
        } = &plan.definition
        else {
            unreachable!();
        };
        assert_eq!(*angle_range, [0.0, 2.0]);
        assert_eq!(*center, Point3::new(0.0, 0.0, 3.0));
        assert!((pitch.z - 4.0 * std::f64::consts::PI).abs() < 1e-12);
        assert_eq!(*apex_factor, 0.0);
        assert_eq!(plan.parameter_range, [0.0, 2.0]);
        assert!(plan.fit_tolerance <= 1e-4);
        assert_eq!(
            plan.cache.control_points.first(),
            Some(&Point3::new(2.0, 0.0, 3.0))
        );

        let reversed = cylinder_helix(&pcurve, &cylinder, [0.0, 1.0], end, [2.0, 0.0, 3.0])
            .expect("reversed physical edge helix");
        let ProceduralCurveDefinition::Helix { center, pitch, .. } = reversed.definition else {
            unreachable!();
        };
        assert_eq!(center, Point3::new(0.0, 0.0, 7.0));
        assert!((pitch.z + 4.0 * std::f64::consts::PI).abs() < 1e-12);

        let trimmed_start = [2.0 * 0.5_f64.cos(), 2.0 * 0.5_f64.sin(), 4.0];
        let trimmed_end = [2.0 * 1.5_f64.cos(), 2.0 * 1.5_f64.sin(), 6.0];
        let trimmed = cylinder_helix(&pcurve, &cylinder, [0.25, 0.75], trimmed_start, trimmed_end)
            .expect("trimmed physical edge helix");
        let ProceduralCurveDefinition::Helix {
            angle_range,
            center,
            pitch,
            ..
        } = trimmed.definition
        else {
            unreachable!();
        };
        assert_eq!(angle_range, [0.0, 1.0]);
        assert_eq!(center.z, 4.0);
        assert!((pitch.z - 4.0 * std::f64::consts::PI).abs() < 1e-12);
    }
}
