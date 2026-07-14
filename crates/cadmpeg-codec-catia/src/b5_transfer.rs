// SPDX-License-Identifier: Apache-2.0
//! Transfer of reference-closed `b5 03` object topology into neutral IR.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, ProceduralCurve,
    ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition, Surface,
    SurfaceGeometry,
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

/// Transfer a complete B5 graph. Returns `false` without mutation when any
/// referenced face, pcurve, edge endpoint, or loop chain remains unresolved.
pub(crate) fn transfer(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    graph: &B5Graph,
    payload: &UnknownId,
) -> bool {
    if !graph.complete {
        let mut subset = graph.clone();
        subset.loops.retain(|_, loop_| {
            loop_
                .pcurves
                .iter()
                .zip(&loop_.edges)
                .all(|(pcurve, edge)| {
                    subset
                        .pcurves
                        .get(pcurve)
                        .is_some_and(|pcurve| pcurve.surface == loop_.surface)
                        && subset.edge_vertices.contains_key(edge)
                })
                && solve_loop_chain(loop_, &subset.edge_vertices).is_some()
        });
        subset.faces.retain(|face| {
            subset.surfaces.contains_key(&face.surface)
                && !face.loops.is_empty()
                && face.loops.iter().all(|loop_id| {
                    subset
                        .loops
                        .get(loop_id)
                        .is_some_and(|loop_| loop_.surface == face.surface)
                })
        });
        let referenced_loops: HashSet<u32> = subset
            .faces
            .iter()
            .flat_map(|face| face.loops.iter().copied())
            .collect();
        subset
            .loops
            .retain(|loop_id, _| referenced_loops.contains(loop_id));
        if subset.faces.is_empty() || subset.loops.is_empty() {
            return false;
        }
        subset.complete = true;
        return transfer(ir, annotations, &subset, payload);
    }
    if graph.faces.is_empty() {
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
    let mut edge_curve_plan = HashMap::<u32, CurveGeometry>::new();
    let mut edge_procedural_plan = HashMap::<u32, ProceduralCurveDefinition>::new();
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
            if pcurve_plan
                .insert(pcurve_id, (geometry, cylinder_reparameterized))
                .is_some()
            {
                return false;
            }
            let lifted = lifted_curve_geometry(pcurve, surface).or_else(|| {
                let SurfaceGeometry::Nurbs(cache) = &surface_plan.get(&loop_.surface)?.geometry
                else {
                    return None;
                };
                nurbs_isocurve(pcurve, cache).map(CurveGeometry::Nurbs)
            });
            if let Some(geometry) = lifted {
                edge_curve_plan.entry(edge_id).or_insert(geometry);
            } else if let Some(definition) = cylinder_helix(pcurve, surface) {
                edge_procedural_plan.entry(edge_id).or_insert(definition);
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
            tolerance: graph.vertex_tolerances.get(&index).copied(),
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
            tolerance: graph.vertex_tolerances.get(&index).copied(),
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

    let mut pcurve_ids = HashMap::new();
    for (object_id, (geometry, cylinder_reparameterized)) in pcurve_plan {
        let id = PcurveId(format!("catia:b5:pcurve#{object_id}"));
        annotate(
            annotations,
            &id,
            "object_stream_b5_03",
            "21_pcurve",
            Exactness::ByteExact,
        );
        if cylinder_reparameterized {
            annotations.derived(&id, "geometry.control_points");
        }
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
        let geometry = edge_curve_plan
            .remove(&edge_id)
            .unwrap_or_else(|| CurveGeometry::Unknown {
                record: Some(payload.clone()),
            });
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
        if let Some(definition) = edge_procedural_plan.remove(&edge_id) {
            let procedural_id = ProceduralCurveId(format!("catia:b5:helix#{edge_id}"));
            annotate(
                annotations,
                &procedural_id,
                "object_stream_b5_03",
                "cylinder_parametric_helix",
                Exactness::Derived,
            );
            ir.model.procedural_curves.push(ProceduralCurve {
                id: procedural_id,
                curve: curve_id.clone(),
                definition,
                cache_fit_tolerance: None,
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
    true
}

fn neutral_pcurve_point(point: [f64; 2], surface: &B5Surface) -> Point2 {
    match surface {
        B5Surface::Cylinder { radius, .. } => Point2::new(point[0] / radius, point[1]),
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
        contract_surface(surface, u, true)
    } else if let Some(v) = constant_coordinate(&pcurve.control_points, 1) {
        contract_surface(surface, v, false)
    } else {
        None
    }
}

fn contract_surface(surface: &NurbsSurface, parameter: f64, fix_u: bool) -> Option<NurbsCurve> {
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    let (fixed_basis, varying_count, degree, knots) = if fix_u {
        (
            basis_values(
                &surface.u_knots,
                usize::try_from(surface.u_degree).ok()?,
                parameter,
                u_count,
            )?,
            v_count,
            surface.v_degree,
            surface.v_knots.clone(),
        )
    } else {
        (
            basis_values(
                &surface.v_knots,
                usize::try_from(surface.v_degree).ok()?,
                parameter,
                v_count,
            )?,
            u_count,
            surface.u_degree,
            surface.u_knots.clone(),
        )
    };
    let mut control_points = Vec::with_capacity(varying_count);
    let mut weights = Vec::with_capacity(varying_count);
    for varying in 0..varying_count {
        let mut numerator = [0.0; 3];
        let mut denominator = 0.0;
        for (fixed, basis) in fixed_basis.iter().copied().enumerate() {
            let index = if fix_u {
                fixed.checked_mul(v_count)?.checked_add(varying)?
            } else {
                varying.checked_mul(v_count)?.checked_add(fixed)?
            };
            let point = surface.control_points.get(index)?;
            let weight = surface
                .weights
                .as_ref()
                .and_then(|values| values.get(index))
                .copied()
                .unwrap_or(1.0);
            let factor = basis * weight;
            numerator[0] += factor * point.x;
            numerator[1] += factor * point.y;
            numerator[2] += factor * point.z;
            denominator += factor;
        }
        if !denominator.is_finite() || denominator.abs() <= f64::EPSILON {
            return None;
        }
        control_points.push(Point3::new(
            numerator[0] / denominator,
            numerator[1] / denominator,
            numerator[2] / denominator,
        ));
        weights.push(denominator);
    }
    let rational = surface.weights.is_some();
    Some(NurbsCurve {
        degree,
        knots,
        control_points,
        weights: rational.then_some(weights),
        periodic: if fix_u {
            surface.v_periodic
        } else {
            surface.u_periodic
        },
    })
}

fn basis_values(knots: &[f64], degree: usize, parameter: f64, count: usize) -> Option<Vec<f64>> {
    if knots.len() != count.checked_add(degree)?.checked_add(1)? || count == 0 {
        return None;
    }
    let mut basis = vec![0.0; count + degree];
    for (index, value) in basis.iter_mut().enumerate() {
        if (knots.get(index)? <= &parameter && &parameter < knots.get(index + 1)?)
            || (parameter == *knots.last()? && index + 1 == count)
        {
            *value = 1.0;
        }
    }
    for level in 1..=degree {
        for index in 0..count + degree - level {
            let left_denominator = knots[index + level] - knots[index];
            let right_denominator = knots[index + level + 1] - knots[index + 1];
            let left = if left_denominator.abs() <= f64::EPSILON {
                0.0
            } else {
                (parameter - knots[index]) / left_denominator * basis[index]
            };
            let right = if right_denominator.abs() <= f64::EPSILON {
                0.0
            } else {
                (knots[index + level + 1] - parameter) / right_denominator * basis[index + 1]
            };
            basis[index] = left + right;
        }
    }
    basis.truncate(count);
    basis.iter().all(|value| value.is_finite()).then_some(basis)
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

fn cylinder_helix(pcurve: &B5Pcurve, surface: &B5Surface) -> Option<ProceduralCurveDefinition> {
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
    let mut endpoints = [pcurve.control_points[0], pcurve.control_points[1]];
    let mut angles = [endpoints[0][0] / radius, endpoints[1][0] / radius];
    if angles[0] == angles[1] || endpoints[0][1] == endpoints[1][1] {
        return None;
    }
    if angles[0] > angles[1] {
        endpoints.swap(0, 1);
        angles.swap(0, 1);
    }
    let rise_per_radian = (endpoints[1][1] - endpoints[0][1]) / (angles[1] - angles[0]);
    let reference_y = cross(*axis, *reference_x);
    Some(ProceduralCurveDefinition::Helix {
        angle_range: angles,
        center: point3(add(*origin, scale(*axis, endpoints[0][1]))),
        major: vector(scale(*reference_x, *radius)),
        minor: vector(scale(reference_y, *radius)),
        pitch: vector(scale(*axis, rise_per_radian * 2.0 * std::f64::consts::PI)),
        apex_factor: 0.0,
        axis: vector(*axis),
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
    let span_count = ((angles[1] - angles[0]).abs() / std::f64::consts::FRAC_PI_2)
        .ceil()
        .max(1.0) as usize;
    let mut control_points = Vec::with_capacity(span_count * 2 + 1);
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
    let span_count = ((angular_interval[1] - angular_interval[0]).abs()
        / std::f64::consts::FRAC_PI_2)
        .ceil()
        .max(1.0) as usize;
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
    use super::{
        contract_surface, cylinder_helix, lifted_curve_geometry, neutral_pcurve_point,
        revolution_surface,
    };
    use crate::b5::{B5Pcurve, B5Profile, B5Surface};
    use cadmpeg_ir::eval::surface_point;
    use cadmpeg_ir::geometry::{CurveGeometry, ProceduralCurveDefinition, SurfaceGeometry};
    use cadmpeg_ir::math::Point3;

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
        let curve = contract_surface(&surface, 0.25, true).expect("u isocurve");
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
        let Some(ProceduralCurveDefinition::Helix {
            angle_range,
            center,
            pitch,
            apex_factor,
            ..
        }) = cylinder_helix(&pcurve, &cylinder)
        else {
            panic!("degree-one cylinder helix");
        };
        assert_eq!(angle_range, [0.0, 2.0]);
        assert_eq!(center, Point3::new(0.0, 0.0, 3.0));
        assert!((pitch.z - 4.0 * std::f64::consts::PI).abs() < 1e-12);
        assert_eq!(apex_factor, 0.0);
    }
}
