// SPDX-License-Identifier: Apache-2.0
//! Native B-rep, prototype surfaces, positional solids, and carrier intersections.

#[allow(clippy::wildcard_imports)]
use super::*;

pub(super) fn transfer_native_brep(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    derived_intersection_curves: &BTreeSet<CurveId>,
    analytic_pcurve_carriers: &BTreeSet<CurveId>,
    nurbs_endpoint_witnesses: &BTreeSet<CurveId>,
) -> (usize, usize) {
    let carriers = placed_carriers(scan, ir);
    let planes = carriers
        .iter()
        .filter_map(|(id, carrier)| match carrier {
            CarrierEquation::Plane(plane) => Some((*id, *plane)),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let face_orientations = native_face_orientations(scan, ir);
    let half_edges = scan
        .topology
        .half_edges
        .iter()
        .map(|half_edge| (half_edge.id, half_edge))
        .collect::<BTreeMap<_, _>>();
    let incidence = scan
        .topology
        .half_edge_vertex_incidence
        .iter()
        .map(|binding| (binding.half_edge, binding))
        .collect::<BTreeMap<_, _>>();
    let solved_vertices =
        solved_topological_vertices(scan, ir, &carriers, nurbs_endpoint_witnesses);
    let mut native_pcurves = NativePcurveCandidates::new();
    for (curve_id, faces, face_0_endpoints, face_1_endpoints, offset) in scan
        .curves
        .pcurves
        .iter()
        .map(|pcurve| {
            (
                pcurve.curve_id,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
                pcurve.offset,
            )
        })
        .chain(scan.curves.bound_prototype_pcurves.iter().map(|pcurve| {
            (
                pcurve.curve_id,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
                pcurve.offset,
            )
        }))
    {
        native_pcurves
            .entry((curve_id, faces[0]))
            .or_default()
            .push((face_0_endpoints, offset));
        native_pcurves
            .entry((curve_id, faces[1]))
            .or_default()
            .push((face_1_endpoints, offset));
    }
    let native_edge_vertices =
        crate::topology::edge_vertex_pairs(&scan.topology.half_edge_vertex_incidence);
    let edge_vertices = scan
        .curves
        .topology_rows
        .iter()
        .filter_map(|row| {
            let vertices = native_edge_vertices.get(&row.id).copied()?;
            vertices
                .iter()
                .all(|vertex| solved_vertices.contains_key(vertex))
                .then_some((row.id, vertices))
        })
        .collect::<BTreeMap<_, _>>();
    let mut loops_by_face = BTreeMap::<u32, Vec<&crate::topology::Loop>>::new();
    for lp in &scan.topology.loops {
        loops_by_face.entry(lp.face_id).or_default().push(lp);
    }
    let eligible_faces = loops_by_face
        .into_iter()
        .filter_map(|(face_id, loops)| {
            face_orientations.contains_key(&face_id).then_some(())?;
            loops
                .iter()
                .all(|lp| {
                    lp.half_edges
                        .iter()
                        .all(|half_edge| edge_vertices.contains_key(&half_edge.curve_id))
                })
                .then_some(())?;
            let ordered = ordered_face_loops(
                loops,
                planes.get(&face_id).copied(),
                &incidence,
                &solved_vertices,
            )?;
            Some((face_id, ordered))
        })
        .collect::<BTreeMap<_, _>>();
    let eligible_loops = eligible_faces
        .values()
        .flatten()
        .copied()
        .collect::<Vec<_>>();

    let emitted_half_edges = eligible_loops
        .iter()
        .flat_map(|lp| lp.half_edges.iter().copied())
        .collect::<BTreeSet<_>>();
    let face_curves = emitted_half_edges
        .iter()
        .map(|half_edge| half_edge.curve_id)
        .collect::<BTreeSet<_>>();
    let closed_single_edge_curves = face_curves
        .iter()
        .filter(|curve_id| {
            let uses = eligible_loops
                .iter()
                .filter(|lp| {
                    lp.half_edges
                        .iter()
                        .any(|half_edge| half_edge.curve_id == **curve_id)
                })
                .collect::<Vec<_>>();
            !uses.is_empty() && uses.iter().all(|lp| lp.half_edges.len() == 1)
        })
        .copied()
        .collect::<BTreeSet<_>>();
    let emitted_curves = face_curves.clone();
    let used_vertices = emitted_curves
        .iter()
        .filter_map(|curve| edge_vertices.get(curve))
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>();
    let row_offsets = scan
        .curves
        .topology_rows
        .iter()
        .map(|row| (row.id, row.offset))
        .collect::<BTreeMap<_, _>>();
    let curve_faces = crate::topology::uniquely_identified_rows(&scan.curves.topology_rows)
        .into_iter()
        .map(|row| (row.id, row.faces))
        .collect::<BTreeMap<_, _>>();

    let solved_point_count = used_vertices.len();
    for vertex_id in used_vertices {
        let point_id = PointId(format!("creo:visibgeom:point#{vertex_id}"));
        let vertex = VertexId(format!("creo:visibgeom:vertex#{vertex_id}"));
        if ir.model.vertices.iter().any(|item| item.id == vertex) {
            continue;
        }
        annotate(
            annotations,
            &point_id,
            "VisibGeom",
            0,
            "topological_vertex_point",
            Exactness::Derived,
        );
        let position = solved_vertices[&vertex_id];
        ir.model.points.push(Point {
            id: point_id.clone(),
            position: Point3::new(position[0], position[1], position[2]),
            source_object: None,
        });
        annotate(
            annotations,
            &vertex,
            "VisibGeom",
            0,
            "topological_vertex_orbit",
            Exactness::Derived,
        );
        ir.model.vertices.push(Vertex {
            id: vertex,
            point: point_id,
            tolerance: None,
        });
    }
    for curve_id in &emitted_curves {
        let [start, end] = edge_vertices[curve_id];
        let curve = CurveId(format!("creo:visibgeom:curve#{curve_id}"));
        let points = [solved_vertices[&start], solved_vertices[&end]];
        let unbacked_closed_edge = start == end
            && closed_single_edge_curves.contains(curve_id)
            && curve_faces.get(curve_id).is_some_and(|face_ids| {
                !face_ids
                    .iter()
                    .any(|face_id| native_pcurves.contains_key(&(*curve_id, *face_id)))
            });
        let derived_line = (derived_intersection_curves.contains(&curve)
            || analytic_pcurve_carriers.contains(&curve))
            && ir.model.curves.iter().any(|candidate| {
                candidate.id == curve && matches!(candidate.geometry, CurveGeometry::Line { .. })
            });
        let param_range = if derived_line {
            ir.model
                .curves
                .iter_mut()
                .find(|candidate| candidate.id == curve)
                .and_then(|candidate| orient_line_edge_carrier(&mut candidate.geometry, points))
        } else {
            ir.model
                .curves
                .iter()
                .find(|candidate| candidate.id == curve)
                .and_then(|candidate| {
                    exact_line_edge_parameter_range(&candidate.geometry, points).or_else(|| {
                        nonperiodic_nurbs_edge_parameter_range(&candidate.geometry, points).or_else(
                            || {
                                nonperiodic_conic_edge_parameter_range(&candidate.geometry, points)
                                    .or_else(|| {
                                        pcurve_backed_periodic_conic_parameter_range(
                                            &candidate.geometry,
                                            *curve_id,
                                            *curve_faces.get(curve_id)?,
                                            &native_pcurves,
                                            &ir.model.surfaces,
                                            points,
                                        )
                                    })
                                    .or_else(|| {
                                        unbacked_closed_edge.then_some(()).and_then(|()| {
                                            full_periodic_conic_edge_parameter_range(
                                                &candidate.geometry,
                                                points[0],
                                            )
                                        })
                                    })
                                    .or_else(|| {
                                        unbacked_closed_edge.then_some(()).and_then(|()| {
                                            full_periodic_nurbs_edge_parameter_range(
                                                &candidate.geometry,
                                                points[0],
                                            )
                                        })
                                    })
                            },
                        )
                    })
                })
        };
        let id = EdgeId(format!("creo:visibgeom:edge#{curve_id}"));
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row_offsets.get(curve_id).copied().unwrap_or(0) as u64,
            "curve_topology_edge",
            Exactness::Derived,
        );
        ir.model.edges.push(Edge {
            id,
            curve: Some(curve.clone()),
            start: VertexId(format!("creo:visibgeom:vertex#{start}")),
            end: VertexId(format!("creo:visibgeom:vertex#{end}")),
            param_range,
            tolerance: None,
        });
        if !ir.model.curves.iter().any(|item| item.id == curve) {
            let offset = row_offsets.get(curve_id).copied().unwrap_or(0);
            annotate(
                annotations,
                &curve,
                "VisibGeom",
                offset as u64,
                "opaque_native_curve_carrier",
                Exactness::Unknown,
            );
            ir.model.curves.push(Curve {
                id: curve,
                geometry: CurveGeometry::Unknown {
                    record: geometry_section_record(scan, offset),
                },
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{curve_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
        }
    }

    let mut face_adjacency = BTreeMap::<u32, BTreeSet<u32>>::new();
    for face_id in eligible_faces.keys() {
        face_adjacency.entry(*face_id).or_default();
    }
    for curve_id in &emitted_curves {
        let faces = emitted_half_edges
            .iter()
            .filter(|half_edge| half_edge.curve_id == *curve_id)
            .filter_map(|half_edge| half_edges.get(half_edge))
            .map(|half_edge| half_edge.face_id)
            .collect::<Vec<_>>();
        if let [first, second] = faces.as_slice() {
            if eligible_faces.contains_key(first) && eligible_faces.contains_key(second) {
                face_adjacency.entry(*first).or_default().insert(*second);
                face_adjacency.entry(*second).or_default().insert(*first);
            }
        }
    }
    let mut remaining = face_adjacency.keys().copied().collect::<BTreeSet<_>>();
    let mut components = Vec::new();
    while let Some(start) = remaining.pop_first() {
        let mut component = BTreeSet::from([start]);
        let mut pending = vec![start];
        while let Some(face) = pending.pop() {
            for neighbour in face_adjacency.get(&face).into_iter().flatten() {
                if remaining.remove(neighbour) {
                    component.insert(*neighbour);
                    pending.push(*neighbour);
                }
            }
        }
        components.push(component);
    }

    for (component_index, faces) in components.iter().enumerate() {
        let body_id = BodyId(format!("creo:visibgeom:body#{}", component_index + 1));
        let region_id = RegionId(format!("creo:visibgeom:region#{}", component_index + 1));
        let shell_id = ShellId(format!("creo:visibgeom:shell#{}", component_index + 1));
        for (id, tag) in [
            (body_id.to_string(), "native_component_body"),
            (region_id.to_string(), "native_component_region"),
            (shell_id.to_string(), "native_component_shell"),
        ] {
            annotate(annotations, id, "VisibGeom", 0, tag, Exactness::Derived);
        }
        let component_curves = eligible_loops
            .iter()
            .filter(|lp| faces.contains(&lp.face_id))
            .flat_map(|lp| lp.half_edges.iter().map(|half_edge| half_edge.curve_id))
            .collect::<BTreeSet<_>>();
        let closed = component_curves.iter().all(|curve_id| {
            let adjacent = emitted_half_edges
                .iter()
                .filter(|half_edge| half_edge.curve_id == *curve_id)
                .filter_map(|half_edge| half_edges.get(half_edge))
                .map(|half_edge| half_edge.face_id)
                .collect::<BTreeSet<_>>();
            adjacent.len() == 2 && adjacent.iter().all(|face| faces.contains(face))
        });
        ir.model.bodies.push(Body {
            id: body_id.clone(),
            kind: if closed {
                BodyKind::Solid
            } else {
                BodyKind::Sheet
            },
            regions: vec![region_id.clone()],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id,
            shells: vec![shell_id.clone()],
        });
        ir.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region_id,
            faces: faces
                .iter()
                .map(|face| FaceId(format!("creo:visibgeom:face#{face}")))
                .collect(),
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        for face_id in faces {
            let native_loops = &eligible_faces[face_id];
            let face = FaceId(format!("creo:visibgeom:face#{face_id}"));
            let loop_ids = (0..native_loops.len())
                .map(|index| {
                    if index == 0 {
                        LoopId(format!("creo:visibgeom:loop#{face_id}"))
                    } else {
                        LoopId(format!("creo:visibgeom:loop#{face_id}:{index}"))
                    }
                })
                .collect::<Vec<_>>();
            let face_offset = crate::surface::unique_surface_row(&scan.surfaces.rows, *face_id)
                .map_or(0, |row| row.offset);
            let surface = SurfaceId(format!("creo:visibgeom:surface#{face_id}"));
            if !ir.model.surfaces.iter().any(|item| item.id == surface) {
                annotate(
                    annotations,
                    &surface,
                    "VisibGeom",
                    face_offset as u64,
                    "opaque_native_surface_carrier",
                    Exactness::Unknown,
                );
                ir.model.surfaces.push(Surface {
                    id: surface.clone(),
                    geometry: SurfaceGeometry::Unknown {
                        record: geometry_section_record(scan, face_offset),
                    },
                    source_object: Some(SourceObjectAssociation {
                        format: "creo".to_string(),
                        object_id: format!("VisibGeom:{face_id}"),
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                });
            }
            let face_sense = if face_orientations[face_id] {
                Sense::Reversed
            } else {
                Sense::Forward
            };
            annotate(
                annotations,
                &face,
                "VisibGeom",
                face_offset as u64,
                "native_face",
                Exactness::Derived,
            );
            for loop_id in &loop_ids {
                annotate(
                    annotations,
                    loop_id,
                    "VisibGeom",
                    face_offset as u64,
                    "native_face_loop",
                    Exactness::Derived,
                );
            }
            ir.model.faces.push(Face {
                id: face.clone(),
                shell: shell_id.clone(),
                surface,
                sense: face_sense,
                loops: loop_ids.clone(),
                name: None,
                color: None,
                tolerance: None,
            });
            for (boundary_index, (native_loop, loop_id)) in
                native_loops.iter().zip(loop_ids).enumerate()
            {
                let coedge_ids = native_loop
                    .half_edges
                    .iter()
                    .map(|half_edge| {
                        CoedgeId(format!(
                            "creo:visibgeom:coedge#{}:{}",
                            half_edge.curve_id, half_edge.side
                        ))
                    })
                    .collect::<Vec<_>>();
                ir.model.loops.push(IrLoop {
                    id: loop_id.clone(),
                    face: face.clone(),
                    boundary_role: if boundary_index == 0 {
                        cadmpeg_ir::topology::LoopBoundaryRole::Outer
                    } else {
                        cadmpeg_ir::topology::LoopBoundaryRole::Inner
                    },
                    coedges: coedge_ids.clone(),
                    vertex_uses: Vec::new(),
                });
                for (index, half_edge) in native_loop.half_edges.iter().enumerate() {
                    let id = coedge_ids[index].clone();
                    let twin = HalfEdgeId {
                        curve_id: half_edge.curve_id,
                        side: 1 - half_edge.side,
                    };
                    let radial_next = if emitted_half_edges.contains(&twin) {
                        CoedgeId(format!(
                            "creo:visibgeom:coedge#{}:{}",
                            twin.curve_id, twin.side
                        ))
                    } else {
                        id.clone()
                    };
                    annotate(
                        annotations,
                        &id,
                        "VisibGeom",
                        row_offsets.get(&half_edge.curve_id).copied().unwrap_or(0) as u64,
                        "native_half_edge",
                        Exactness::Derived,
                    );
                    let native_candidates = native_pcurves.get(&(half_edge.curve_id, *face_id));
                    let pcurve_geometry = native_candidates
                        .and_then(|candidates| {
                            let incidence = incidence.get(half_edge)?;
                            let end = incidence.end_vertex_id?;
                            let traversal = [
                                solved_vertices[&incidence.start_vertex_id],
                                solved_vertices[&end],
                            ];
                            let surface = ir.model.surfaces.iter().find(|candidate| {
                                candidate.id
                                    == SurfaceId(format!("creo:visibgeom:surface#{face_id}"))
                            })?;
                            unique_oriented_native_pcurve(&surface.geometry, candidates, traversal)
                        })
                        .map(|(endpoints, offset)| {
                            (
                                line_pcurve(endpoints[0], endpoints[1]),
                                Some([0.0, 1.0]),
                                offset,
                                "native_endpoint_pcurve",
                            )
                        })
                        .or_else(|| {
                            native_candidates.is_none().then_some(())?;
                            let surface = ir.model.surfaces.iter().find(|candidate| {
                                candidate.id
                                    == SurfaceId(format!("creo:visibgeom:surface#{face_id}"))
                            })?;
                            let curve = ir.model.curves.iter().find(|candidate| {
                                candidate.id
                                    == CurveId(format!(
                                        "creo:visibgeom:curve#{}",
                                        half_edge.curve_id
                                    ))
                            })?;
                            let edge = ir.model.edges.iter().find(|candidate| {
                                candidate.id
                                    == EdgeId(format!("creo:visibgeom:edge#{}", half_edge.curve_id))
                            })?;
                            let (geometry, tag) =
                                planar_curve_pcurve(&surface.geometry, &curve.geometry)
                                    .map(|geometry| (geometry, "projected_planar_pcurve"))
                                    .or_else(|| {
                                        surface_of_revolution_parallel_pcurve(
                                            &surface.geometry,
                                            &curve.geometry,
                                        )
                                        .map(|geometry| {
                                            (geometry, "projected_parallel_conic_pcurve")
                                        })
                                    })
                                    .or_else(|| {
                                        meridian_circle_pcurve(&surface.geometry, &curve.geometry)
                                            .map(|geometry| (geometry, "projected_meridian_pcurve"))
                                    })
                                    .or_else(|| {
                                        ruled_generator_line_pcurve(
                                            &surface.geometry,
                                            &curve.geometry,
                                        )
                                        .map(|geometry| {
                                            (geometry, "projected_ruled_generator_pcurve")
                                        })
                                    })?;
                            Some((
                                geometry,
                                edge.param_range,
                                row_offsets.get(&half_edge.curve_id).copied().unwrap_or(0),
                                tag,
                            ))
                        });
                    let pcurves = pcurve_geometry
                        .map(|(geometry, parameter_range, offset, tag)| {
                            let pcurve = PcurveId(format!(
                                "creo:visibgeom:pcurve#{}:{face_id}",
                                half_edge.curve_id
                            ));
                            if !ir.model.pcurves.iter().any(|item| item.id == pcurve) {
                                annotate(
                                    annotations,
                                    &pcurve,
                                    "VisibGeom",
                                    offset as u64,
                                    tag,
                                    Exactness::Derived,
                                );
                                ir.model.pcurves.push(Pcurve {
                                    id: pcurve.clone(),
                                    geometry,
                                    wrapper_reversed: None,
                                    native_tail_flags: None,
                                    parameter_range,
                                    fit_tolerance: None,
                                });
                            }
                            PcurveUse {
                                pcurve,
                                isoparametric: None,
                                parameter_range: None,
                            }
                        })
                        .into_iter()
                        .collect();
                    ir.model.coedges.push(Coedge {
                        id,
                        owner_loop: loop_id.clone(),
                        edge: EdgeId(format!("creo:visibgeom:edge#{}", half_edge.curve_id)),
                        next: coedge_ids[(index + 1) % coedge_ids.len()].clone(),
                        previous: coedge_ids[(index + coedge_ids.len() - 1) % coedge_ids.len()]
                            .clone(),
                        radial_next,
                        sense: if half_edge.side == 0 {
                            Sense::Forward
                        } else {
                            Sense::Reversed
                        },
                        pcurves,
                        use_curve: None,
                        use_curve_parameter_range: None,
                    });
                }
            }
        }
    }
    (solved_point_count, emitted_curves.len())
}

pub(super) fn transfer_cap_pair_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    for pair in &scan.curves.fc05_cylinder_cap_pairs {
        let placed_caps = pair
            .cap_plane_ids
            .iter()
            .zip(&pair.curve_cap_ordinates_row_frame)
            .filter_map(|(id, ordinate)| {
                crate::surface::unique_outline_plane(&scan.planes.outlines, *id)
                    .map(|plane| (plane, *ordinate))
            })
            .collect::<Vec<_>>();
        let Some((first_cap, first_ordinate)) = placed_caps.first().copied() else {
            continue;
        };
        let Some(axis_index) = (0..3).find(|axis| first_cap.normal[*axis].abs() > 1.0 - 1e-9)
        else {
            continue;
        };
        if placed_caps
            .iter()
            .any(|(plane, _)| plane.normal != first_cap.normal)
        {
            continue;
        }
        let translations = placed_caps
            .iter()
            .map(|(plane, ordinate)| plane.origin[axis_index] - ordinate)
            .collect::<Vec<_>>();
        if translations
            .iter()
            .any(|translation| (translation - translations[0]).abs() > 1e-9)
        {
            continue;
        }
        let axis_origin = first_ordinate + translations[0];
        let axis_sign = -f64::from(pair.parameter_sign);
        let (origin, axis, ref_direction) = fc05_model_frame(
            axis_index,
            axis_origin,
            pair.center_row_frame,
            pair.reference_direction_row_frame,
            axis_sign,
        );
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", pair.surface_id));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            pair.offset as u64,
            "fc05_cap_pair_cylinder",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(origin[0], origin[1], origin[2]),
                axis: Vector3::new(axis[0], axis[1], axis[2]),
                ref_direction: Vector3::new(ref_direction[0], ref_direction[1], ref_direction[2]),
                radius: pair.radius_mm,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", pair.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        for ((curve_id, ordinate), cap_plane_id) in pair
            .curve_ids
            .iter()
            .zip(&pair.curve_cap_ordinates_row_frame)
            .zip(&pair.cap_plane_ids)
        {
            let cap_offset =
                crate::surface::unique_outline_plane(&scan.planes.outlines, *cap_plane_id)
                    .map_or_else(
                        || ordinate + translations[0],
                        |plane| plane.origin[axis_index],
                    );
            let (center, _, _) = fc05_model_frame(
                axis_index,
                cap_offset,
                pair.center_row_frame,
                pair.reference_direction_row_frame,
                axis_sign,
            );
            let id = CurveId(format!("creo:visibgeom:curve#{curve_id}"));
            if ir.model.curves.iter().any(|curve| curve.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "VisibGeom",
                scan.curves
                    .fc05_circles
                    .iter()
                    .find(|circle| circle.curve_id == *curve_id)
                    .map_or(pair.offset, |circle| circle.offset) as u64,
                "fc05_cap_circle",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id,
                geometry: CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(
                        ref_direction[0],
                        ref_direction[1],
                        ref_direction[2],
                    ),
                    radius: pair.radius_mm,
                },
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{curve_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
        }
    }
}

pub(super) fn prototype_scalar(
    record: &crate::surface::SurfacePrototypeRecord,
    name: &str,
) -> Option<f64> {
    match &record.field(name)?.value {
        crate::surface::SurfaceNamedValue::ScalarSequence(values) if values.len() == 1 => {
            Some(values[0])
        }
        _ => None,
    }
}

pub(super) fn prototype_vector_array(
    record: &crate::surface::SurfacePrototypeRecord,
    name: &str,
) -> Option<Vec<[f64; 3]>> {
    let crate::surface::SurfaceNamedValue::ScalarArray {
        dimensions,
        count: 3,
        values,
        ..
    } = &record.field(name)?.value
    else {
        return None;
    };
    let vector_count = usize::try_from(*dimensions).ok()?;
    (values.len() == vector_count.checked_mul(3)?).then_some(())?;
    values
        .chunks_exact(3)
        .map(|coordinates| Some([coordinates[0]?, coordinates[1]?, coordinates[2]?]))
        .collect()
}

pub(super) fn prototype_parameter_array(
    record: &crate::surface::SurfacePrototypeRecord,
    name: &str,
) -> Option<Vec<f64>> {
    let crate::surface::SurfaceNamedValue::CountedScalarArray { count, values, .. } =
        &record.field(name)?.value
    else {
        return None;
    };
    (values.len() == usize::try_from(*count).ok()?).then_some(())?;
    values.iter().copied().collect()
}

pub(super) fn prototype_spline_nurbs(
    record: &crate::surface::SurfacePrototypeRecord,
) -> Option<NurbsSurface> {
    interpolation_spline_surface(
        &prototype_vector_array(record, "i_points")?,
        &prototype_parameter_array(record, "u_params")?,
        &prototype_parameter_array(record, "v_params")?,
        &prototype_vector_array(record, "end_u_tangts")?,
        &prototype_vector_array(record, "end_v_tangts")?,
        &prototype_vector_array(record, "end_uv_deriv")?,
    )
}

pub(super) fn prototype_local_frame(
    record: &crate::surface::SurfacePrototypeRecord,
) -> Option<([f64; 3], [f64; 3], [f64; 3])> {
    let crate::surface::SurfaceNamedValue::ScalarArray {
        dimensions: 4,
        count: 3,
        values,
        ..
    } = &record.field("local_sys")?.value
    else {
        return None;
    };
    let slots = values.iter().copied().collect::<Option<Vec<_>>>()?;
    let slots: [f64; 12] = slots.try_into().ok()?;
    let first: [f64; 3] = slots[0..3].try_into().ok()?;
    let middle: [f64; 3] = slots[3..6].try_into().ok()?;
    let third: [f64; 3] = slots[6..9].try_into().ok()?;
    let first_norm = dot(first, first).sqrt();
    let reference = normalized(first)?;
    let torus = matches!(record.family, crate::surface::SurfacePrototypeFamily::Torus);
    let mut second_candidates =
        [(middle, torus), (third, true)]
            .into_iter()
            .filter_map(|(candidate, eligible)| {
                let candidate_norm = dot(candidate, candidate).sqrt();
                let equal_scale =
                    (first_norm - candidate_norm).abs() <= 1e-10 * first_norm.max(candidate_norm);
                eligible
                    .then_some(())
                    .filter(|()| {
                        equal_scale && dot(reference, candidate).abs() <= 1e-10 * candidate_norm
                    })
                    .and_then(|()| normalized(candidate))
            });
    let second = second_candidates.next()?;
    second_candidates.next().is_none().then_some(())?;
    let axis = normalized(cross(reference, second))?;
    let origin = slots[9..12].try_into().ok()?;
    Some((origin, axis, reference))
}

pub(super) fn first_instance_surface_row(
    rows: &[crate::surface::SurfaceRow],
    frame_start: usize,
    frame_end: usize,
    prototype_offset: usize,
    row_kind: crate::surface::SurfaceKind,
) -> Option<&crate::surface::SurfaceRow> {
    let rows = rows
        .iter()
        .filter(|row| row.offset >= frame_start && row.offset < frame_end)
        .collect::<Vec<_>>();
    let previous = rows
        .iter()
        .copied()
        .filter(|row| row.offset < prototype_offset)
        .max_by_key(|row| row.offset);
    if previous.is_some_and(|row| row.kind == row_kind) {
        return previous;
    }
    rows.into_iter()
        .filter(|row| row.offset > prototype_offset && row.kind == row_kind)
        .min_by_key(|row| row.offset)
}

pub(super) fn unique_surface_prototype_associations<'a>(
    scan: &'a ContainerScan<'_>,
) -> Vec<(
    &'a crate::surface::SurfacePrototypeRecord,
    &'a crate::surface::SurfaceRow,
    &'a crate::container::Section,
)> {
    let mut associations = Vec::new();
    for record in &scan.surfaces.prototype_records {
        let row_kind = match record.family {
            crate::surface::SurfacePrototypeFamily::Plane => crate::surface::SurfaceKind::Plane,
            crate::surface::SurfacePrototypeFamily::Cylinder => {
                crate::surface::SurfaceKind::Cylinder
            }
            crate::surface::SurfacePrototypeFamily::Torus => {
                crate::surface::SurfaceKind::TorusOrSphere
            }
            crate::surface::SurfacePrototypeFamily::Cone => crate::surface::SurfaceKind::Cone,
            crate::surface::SurfacePrototypeFamily::Spline => crate::surface::SurfaceKind::Spline,
            _ => continue,
        };
        let Some(section) = scan.framing.sections.iter().find(|section| {
            record.offset >= section.offset
                && record.offset < section.offset.saturating_add(section.length)
        }) else {
            continue;
        };
        let section_limit = section.offset.saturating_add(section.length);
        let frame_bounds = if section.offset < scan.framing.data.len() {
            let section_end = section_limit.min(scan.framing.data.len());
            crate::surface::complete_surface_array_bounds(
                &scan.framing.data[section.offset..section_end],
            )
        } else {
            Vec::new()
        };
        let (adjacent_start, adjacent_end) = if frame_bounds.is_empty() {
            if !scan.framing.data.is_empty() {
                continue;
            }
            (section.offset, section_limit)
        } else {
            let relative_record_offset = record.offset.saturating_sub(section.offset);
            let Some((start, end)) = frame_bounds.into_iter().find(|(start, end)| {
                relative_record_offset >= *start && relative_record_offset < *end
            }) else {
                continue;
            };
            (section.offset + start, section.offset + end)
        };
        let Some(row) = first_instance_surface_row(
            &scan.surfaces.rows,
            adjacent_start,
            adjacent_end,
            record.offset,
            row_kind,
        ) else {
            continue;
        };
        if crate::surface::unique_surface_row(&scan.surfaces.rows, row.id)
            .is_none_or(|unique| unique.offset != row.offset)
        {
            continue;
        }
        associations.push((record, row, section));
    }
    let mut association_counts = BTreeMap::<usize, usize>::new();
    for (_, row, _) in &associations {
        *association_counts.entry(row.offset).or_default() += 1;
    }
    associations
        .into_iter()
        .filter(|(_, row, _)| association_counts.get(&row.offset) == Some(&1))
        .collect()
}

pub(super) fn transfer_first_instance_prototype_surfaces(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    if scan.framing.layout != crate::container::Layout::Nd {
        return 0;
    }
    let mut transferred = 0;
    for (record, row, section) in unique_surface_prototype_associations(scan) {
        let geometry = match record.family {
            crate::surface::SurfacePrototypeFamily::Plane => {
                let Some((origin, axis, reference)) = prototype_local_frame(record) else {
                    continue;
                };
                SurfaceGeometry::Plane {
                    origin: Point3::new(origin[0], origin[1], origin[2]),
                    normal: Vector3::new(axis[0], axis[1], axis[2]),
                    u_axis: Vector3::new(reference[0], reference[1], reference[2]),
                }
            }
            crate::surface::SurfacePrototypeFamily::Cylinder => {
                let Some((origin, axis, reference)) = prototype_local_frame(record) else {
                    continue;
                };
                let Some(radius) = prototype_scalar(record, "radius")
                    .filter(|radius| radius.is_finite() && *radius > 0.0)
                else {
                    continue;
                };
                SurfaceGeometry::Cylinder {
                    origin: Point3::new(origin[0], origin[1], origin[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius,
                }
            }
            crate::surface::SurfacePrototypeFamily::Torus => {
                let Some((origin, axis, reference)) = prototype_local_frame(record) else {
                    continue;
                };
                let point = Point3::new(origin[0], origin[1], origin[2]);
                let axis = Vector3::new(axis[0], axis[1], axis[2]);
                let reference = Vector3::new(reference[0], reference[1], reference[2]);
                let radii = match (
                    prototype_scalar(record, "radius1")
                        .filter(|radius| radius.is_finite() && *radius >= 0.0),
                    prototype_scalar(record, "radius2")
                        .filter(|radius| radius.is_finite() && *radius > 0.0),
                ) {
                    (Some(radius1), Some(radius2)) => Some([radius1, radius2]),
                    _ => None,
                };
                let Some([radius1, radius2]) = radii else {
                    continue;
                };
                if radius1 == 0.0 {
                    SurfaceGeometry::Sphere {
                        center: point,
                        axis,
                        ref_direction: reference,
                        radius: radius2,
                    }
                } else {
                    SurfaceGeometry::Torus {
                        center: point,
                        axis,
                        ref_direction: reference,
                        major_radius: radius1,
                        minor_radius: radius2,
                    }
                }
            }
            crate::surface::SurfacePrototypeFamily::Cone => {
                let Some(frame) = crate::surface::prototype_cone_frame(record) else {
                    continue;
                };
                SurfaceGeometry::Cone {
                    origin: Point3::new(frame.apex[0], frame.apex[1], frame.apex[2]),
                    axis: Vector3::new(frame.axis[0], frame.axis[1], frame.axis[2]),
                    ref_direction: Vector3::new(
                        frame.ref_direction[0],
                        frame.ref_direction[1],
                        frame.ref_direction[2],
                    ),
                    radius: 0.0,
                    ratio: 1.0,
                    half_angle: frame.half_angle,
                }
            }
            crate::surface::SurfacePrototypeFamily::Spline => {
                let Some(nurbs) = prototype_spline_nurbs(record) else {
                    continue;
                };
                SurfaceGeometry::Nurbs(nurbs)
            }
            _ => unreachable!("prototype family was filtered above"),
        };
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            &section.name,
            record.offset as u64,
            "first_instance_surface_prototype",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry,
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("{}:{}", section.name, row.id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

pub(super) fn transfer_paired_envelope_spheres(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    if scan.framing.layout != crate::container::Layout::Nd {
        return 0;
    }
    let mut transferred = 0;
    for (prototype, associated_row, section) in unique_surface_prototype_associations(scan) {
        if prototype.family != crate::surface::SurfacePrototypeFamily::Torus
            || prototype_scalar(prototype, "radius1") != Some(0.0)
        {
            continue;
        }
        let Some(radius) = prototype_scalar(prototype, "radius2")
            .filter(|radius| radius.is_finite() && *radius > 0.0)
        else {
            continue;
        };
        let rows = scan
            .surfaces
            .rows
            .iter()
            .filter(|row| {
                row.feature_id == associated_row.feature_id
                    && row.kind == crate::surface::SurfaceKind::TorusOrSphere
            })
            .collect::<Vec<_>>();
        let [first_row, second_row] = rows.as_slice() else {
            continue;
        };
        let envelopes = [first_row, second_row].map(|row| {
            unique_surface_parameter_record(scan, row)?
                .type26_five_coordinate_envelope(row.type_byte)
        });
        let [Some(first_envelope), Some(second_envelope)] = envelopes else {
            continue;
        };
        let Some(center) =
            paired_five_coordinate_sphere_center([first_envelope, second_envelope], radius)
        else {
            continue;
        };
        for row in rows {
            let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                &section.name,
                row.offset as u64,
                "paired_type26_sphere_envelope",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry: SurfaceGeometry::Sphere {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                    ref_direction: Vector3::new(1.0, 0.0, 0.0),
                    radius,
                },
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("{}:{}", section.name, row.id),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }
    }
    transferred
}

pub(super) fn transfer_positional_tori(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for record in &scan.surfaces.parameters {
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id)
        else {
            continue;
        };
        if row.kind != crate::surface::SurfaceKind::TorusOrSphere
            || crate::surface::unique_surface_parameter(
                &scan.surfaces.parameters,
                record.surface_id,
            )
            .is_none_or(|unique| unique.offset != record.offset)
        {
            continue;
        }
        let Some(frame) = record.positional_torus_frame else {
            continue;
        };
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        let Some(section) = scan.framing.sections.iter().find(|section| {
            row.offset >= section.offset
                && row.offset < section.offset.saturating_add(section.length)
        }) else {
            continue;
        };
        annotate(
            annotations,
            &id,
            &section.name,
            row.offset as u64,
            "positional_torus_frame",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Torus {
                center: Point3::new(frame.center[0], frame.center[1], frame.center[2]),
                axis: Vector3::new(frame.axis[0], frame.axis[1], frame.axis[2]),
                ref_direction: Vector3::new(
                    frame.ref_direction[0],
                    frame.ref_direction[1],
                    frame.ref_direction[2],
                ),
                major_radius: frame.major_radius,
                minor_radius: frame.minor_radius,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("{}:{}", section.name, row.id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

pub(super) fn transfer_positional_line_extrusion_planes(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let replay_bound_surfaces = scan
        .curves
        .tabulated_cylinder_replays
        .iter()
        .map(|replay| replay.surface_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for record in &scan.surfaces.parameters {
        if replay_bound_surfaces.contains(&record.surface_id) {
            continue;
        }
        if crate::surface::unique_surface_parameter(&scan.surfaces.parameters, record.surface_id)
            .is_none_or(|unique| unique.offset != record.offset)
        {
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id)
        else {
            continue;
        };
        if row.kind != crate::surface::SurfaceKind::Extrusion {
            continue;
        }
        let type_byte = row.type_byte;
        let Some(frame) = record.line_extrusion_frame(type_byte) else {
            continue;
        };
        let directrix =
            std::array::from_fn(|axis| frame.directrix[1][axis] - frame.directrix[0][axis]);
        let (Some(_direction), Some(u_axis), Some(normal)) = (
            normalized(frame.direction),
            normalized(directrix),
            normalized(cross(directrix, frame.direction)),
        ) else {
            continue;
        };
        let surface_id = SurfaceId(format!("creo:visibgeom:surface#{}", record.surface_id));
        if ir
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id == surface_id)
        {
            continue;
        }
        let curve_id = CurveId(format!(
            "creo:visibgeom:surface_directrix#{}",
            record.surface_id
        ));
        let procedural_id = ProceduralSurfaceId(format!(
            "creo:visibgeom:surface_extrusion#{}",
            record.surface_id
        ));
        annotate(
            annotations,
            &curve_id,
            "VisibGeom",
            record.body_offset as u64,
            "positional_line_extrusion_directrix",
            Exactness::Derived,
        );
        annotate(
            annotations,
            &surface_id,
            "VisibGeom",
            record.body_offset as u64,
            "positional_line_extrusion_plane",
            Exactness::Derived,
        );
        annotate(
            annotations,
            &procedural_id,
            "VisibGeom",
            record.body_offset as u64,
            "positional_line_extrusion_construction",
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Line {
                origin: Point3::new(
                    frame.directrix[0][0],
                    frame.directrix[0][1],
                    frame.directrix[0][2],
                ),
                direction: Vector3::new(u_axis[0], u_axis[1], u_axis[2]),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:surface_directrix#{}", record.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(
                    frame.directrix[0][0],
                    frame.directrix[0][1],
                    frame.directrix[0][2],
                ),
                normal: Vector3::new(normal[0], normal[1], normal[2]),
                u_axis: Vector3::new(u_axis[0], u_axis[1], u_axis[2]),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", record.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: procedural_id,
            surface: surface_id,
            definition: ProceduralSurfaceDefinition::Extrusion {
                directrix: curve_id,
                parameter_interval: None,
                direction: Vector3::new(frame.direction[0], frame.direction[1], frame.direction[2]),
                native_position: None,
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
        transferred += 1;
    }
    transferred
}

pub(super) fn section_contains_offset(section: &crate::container::Section, offset: usize) -> bool {
    offset >= section.offset && offset < section.offset.saturating_add(section.length)
}

pub(super) fn unique_tabulated_cylinder_prototype<'a>(
    scan: &'a ContainerScan<'_>,
    replay: &crate::surface::TabulatedCylinderCurveReplay,
) -> Option<&'a crate::surface::SurfacePrototypeRecord> {
    let section = exactly_one(
        scan.framing
            .sections
            .iter()
            .filter(|section| section_contains_offset(section, replay.surface_row_offset)),
    )?;
    exactly_one(scan.surfaces.prototype_records.iter().filter(|record| {
        section_contains_offset(section, record.offset)
            && record.tabulated_cylinder_control_point_ids() == Some(replay.control_point_ids)
    }))
}

pub(super) fn transfer_tabulated_cylinder_spline_extrusions(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut replay_counts = BTreeMap::<u32, usize>::new();
    for replay in &scan.curves.tabulated_cylinder_replays {
        *replay_counts.entry(replay.surface_id).or_default() += 1;
    }
    let mut transferred = 0;
    for replay in &scan.curves.tabulated_cylinder_replays {
        if replay_counts.get(&replay.surface_id) != Some(&1) {
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, replay.surface_id)
        else {
            continue;
        };
        if row.type_byte != 0x2c || row.offset != replay.surface_row_offset {
            continue;
        }
        let Some(parameters) =
            crate::surface::unique_surface_parameter(&scan.surfaces.parameters, replay.surface_id)
        else {
            continue;
        };
        let chart_origin = unique_tabulated_cylinder_prototype(scan, replay)
            .and_then(crate::surface::SurfacePrototypeRecord::tabulated_cylinder_chart_origin);
        let Some((directrix, sweep)) =
            placed_tabulated_cylinder_directrix(replay, parameters, chart_origin)
        else {
            continue;
        };
        let Some(surface) = extruded_nurbs_surface(&directrix, sweep) else {
            continue;
        };
        let curve_id = CurveId(format!(
            "creo:visibgeom:tabulated_directrix#{}",
            replay.surface_id
        ));
        let surface_id = SurfaceId(format!("creo:visibgeom:surface#{}", replay.surface_id));
        if ir
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id == surface_id)
        {
            continue;
        }
        let procedural_id = ProceduralSurfaceId(format!(
            "creo:visibgeom:tabulated_extrusion#{}",
            replay.surface_id
        ));
        annotate(
            annotations,
            &curve_id,
            "VisibGeom",
            replay.offset as u64,
            "tabulated_cylinder_directrix",
            Exactness::Derived,
        );
        annotate(
            annotations,
            &surface_id,
            "VisibGeom",
            replay.surface_row_offset as u64,
            "tabulated_cylinder_surface",
            Exactness::Derived,
        );
        annotate(
            annotations,
            &procedural_id,
            "VisibGeom",
            replay.surface_row_offset as u64,
            "tabulated_cylinder_extrusion",
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Nurbs(directrix),
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:curve#{}", replay.curve_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Nurbs(surface),
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", replay.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: procedural_id,
            surface: surface_id,
            definition: ProceduralSurfaceDefinition::Extrusion {
                directrix: curve_id,
                parameter_interval: Some([0.0, 1.0]),
                direction: Vector3::new(sweep[0], sweep[1], sweep[2]),
                native_position: None,
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
        transferred += 1;
    }
    transferred
}

pub(super) fn transfer_part_product(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> bool {
    let Some(model_name) = scan.framing.model_name.as_ref() else {
        return false;
    };
    let Some(model_name_offset) = scan.framing.model_name_offset else {
        return false;
    };
    let product_id = ProductDefinitionId("creo:model:product_definition#root".to_string());
    let occurrence_id = OccurrenceId("creo:model:occurrence#root".to_string());
    annotate(
        annotations,
        &product_id,
        "archive_header",
        model_name_offset as u64,
        "part_product",
        Exactness::Derived,
    );
    annotate(
        annotations,
        &occurrence_id,
        "archive_header",
        model_name_offset as u64,
        "part_product_occurrence",
        Exactness::Derived,
    );
    ir.model.product_definitions.push(ProductDefinition {
        id: product_id.clone(),
        kind: ProductDefinitionKind::Part,
        source_name: Some(model_name.clone()),
        label: Some(model_name.clone()),
        description: None,
        part_number: Some(model_name.clone()),
        bom_properties: BTreeMap::default(),
        bodies: ir.model.bodies.iter().map(|body| body.id.clone()).collect(),
        native_ref: None,
    });
    ir.model.occurrences.push(Occurrence {
        id: occurrence_id,
        prototype: PrototypeReference::Local {
            definition: product_id,
        },
        parent: OccurrenceParent::Root,
        ordinal: 0,
        transform: Transform::identity(),
        prototype_transform: Transform::identity(),
        scale: [1.0; 3],
        name: Some(model_name.clone()),
        linked_subelements: Vec::new(),
        visible: None,
        element_component: None,
        claim_child: None,
        copy_on_change: None,
        copy_on_change_source: None,
        copy_on_change_group: None,
        copy_on_change_touched: None,
        link_transform: None,
        native_ref: None,
    });
    true
}

pub(super) fn fc05_model_frame(
    axis_index: usize,
    axis_ordinate: f64,
    center_row_frame: [f64; 2],
    reference_row_frame: [f64; 2],
    axis_sign: f64,
) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let [first, second] = center_row_frame;
    let [reference_x, reference_z] = reference_row_frame;
    match axis_index {
        0 => (
            [axis_ordinate, second, first],
            [axis_sign, 0.0, 0.0],
            [0.0, reference_z, reference_x],
        ),
        1 => (
            [first, axis_ordinate, second],
            [0.0, axis_sign, 0.0],
            [reference_x, 0.0, reference_z],
        ),
        2 => (
            [second, first, axis_ordinate],
            [0.0, 0.0, axis_sign],
            [reference_z, reference_x, 0.0],
        ),
        _ => unreachable!("model-space axis index is bounded by XYZ"),
    }
}

pub(super) fn transfer_fc05_cap_circles(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    for circle in &scan.curves.fc05_circles {
        let topology = scan
            .curves
            .topology_rows
            .iter()
            .filter(|row| row.id == circle.curve_id)
            .collect::<Vec<_>>();
        let [topology] = topology.as_slice() else {
            continue;
        };
        let cap_planes = topology
            .faces
            .iter()
            .filter_map(|face| {
                crate::surface::unique_surface_row(&scan.surfaces.rows, *face)
                    .filter(|row| row.kind == crate::surface::SurfaceKind::Plane)?;
                crate::surface::unique_outline_plane(&scan.planes.outlines, *face)
            })
            .collect::<Vec<_>>();
        let cylinders = topology
            .faces
            .iter()
            .filter(|face| {
                crate::surface::unique_surface_row(&scan.surfaces.rows, **face)
                    .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
            })
            .copied()
            .collect::<Vec<_>>();
        let ([cap], [cylinder_id], Some(_)) = (
            cap_planes.as_slice(),
            cylinders.as_slice(),
            circle.cap_ordinate_row_frame,
        ) else {
            continue;
        };
        let Some(axis_index) = (0..3).find(|axis| cap.normal[*axis].abs() > 1.0 - 1e-9) else {
            continue;
        };
        let [first, second] = circle.center_row_frame;
        let (reference, axis_sign) = circle
            .reference_direction_row_frame
            .zip(circle.parameter_sign)
            .map_or(
                (
                    circle.sample_direction_row_frame,
                    cap.normal[axis_index].signum(),
                ),
                |(reference, parameter_sign)| (reference, -f64::from(parameter_sign)),
            );
        let (center, axis, ref_direction) = fc05_model_frame(
            axis_index,
            cap.origin[axis_index],
            [first, second],
            reference,
            axis_sign,
        );
        let id = CurveId(format!("creo:visibgeom:curve#{}", circle.curve_id));
        if !ir.model.curves.iter().any(|curve| curve.id == id) {
            annotate(
                annotations,
                &id,
                "VisibGeom",
                circle.offset as u64,
                "fc05_cap_circle",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id,
                geometry: CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(
                        ref_direction[0],
                        ref_direction[1],
                        ref_direction[2],
                    ),
                    radius: circle.radius_mm,
                },
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{}", circle.curve_id),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
        }
        let surface_id = SurfaceId(format!("creo:visibgeom:surface#{cylinder_id}"));
        if ir
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id == surface_id)
        {
            continue;
        }
        annotate(
            annotations,
            &surface_id,
            "VisibGeom",
            circle.offset as u64,
            "fc05_axis_cylinder",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id: surface_id,
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(center[0], center[1], center[2]),
                axis: Vector3::new(axis[0], axis[1], axis[2]),
                ref_direction: Vector3::new(ref_direction[0], ref_direction[1], ref_direction[2]),
                radius: circle.radius_mm,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{cylinder_id}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
}

pub(super) fn carrier_intersection_curve(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Option<(CurveGeometry, &'static str)> {
    match (first, second) {
        (CarrierEquation::Plane(first), CarrierEquation::Plane(second)) => {
            let direction = cross(first.normal, second.normal);
            let denominator = dot(direction, direction);
            if denominator <= 1e-18 {
                return None;
            }
            let first_distance = dot(first.normal, first.origin);
            let second_distance = dot(second.normal, second.origin);
            let weighted = [0, 1, 2].map(|axis| {
                first_distance * second.normal[axis] - second_distance * first.normal[axis]
            });
            let point_numerator = cross(weighted, direction);
            let origin = point_numerator.map(|value| value / denominator);
            let direction = normalized(direction)?;
            Some((
                CurveGeometry::Line {
                    origin: Point3::new(origin[0], origin[1], origin[2]),
                    direction: Vector3::new(direction[0], direction[1], direction[2]),
                },
                "plane_intersection_line",
            ))
        }
        (CarrierEquation::Plane(plane), CarrierEquation::Cylinder(cylinder))
        | (CarrierEquation::Cylinder(cylinder), CarrierEquation::Plane(plane)) => {
            let normal = normalized(plane.normal)?;
            let axis = normalized(cylinder.axis)?;
            let cosine = dot(normal, axis);
            if cosine.abs() <= 1e-10 {
                let signed_distance = dot(
                    normal,
                    std::array::from_fn(|index| cylinder.origin[index] - plane.origin[index]),
                );
                let scale = cylinder.radius.max(1.0);
                if (signed_distance.abs() - cylinder.radius).abs() > 1e-9 * scale {
                    return None;
                }
                let origin: [f64; 3] = std::array::from_fn(|index| {
                    cylinder.origin[index] - signed_distance * normal[index]
                });
                return Some((
                    CurveGeometry::Line {
                        origin: Point3::new(origin[0], origin[1], origin[2]),
                        direction: Vector3::new(axis[0], axis[1], axis[2]),
                    },
                    "plane_cylinder_tangent_line",
                ));
            }
            let axis_parameter = dot(
                normal,
                std::array::from_fn(|index| plane.origin[index] - cylinder.origin[index]),
            ) / cosine;
            let center: [f64; 3] =
                std::array::from_fn(|index| cylinder.origin[index] + axis_parameter * axis[index]);
            if (cosine.abs() - 1.0).abs() <= 1e-10 {
                let reference = normalized(cylinder.ref_direction)?;
                return Some((
                    CurveGeometry::Circle {
                        center: Point3::new(center[0], center[1], center[2]),
                        axis: Vector3::new(normal[0], normal[1], normal[2]),
                        ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                        radius: cylinder.radius,
                    },
                    "plane_cylinder_circle",
                ));
            }
            let projected_axis = normalized(std::array::from_fn(|index| {
                axis[index] - cosine * normal[index]
            }))?;
            Some((
                CurveGeometry::Ellipse {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(normal[0], normal[1], normal[2]),
                    major_direction: Vector3::new(
                        projected_axis[0],
                        projected_axis[1],
                        projected_axis[2],
                    ),
                    major_radius: cylinder.radius / cosine.abs(),
                    minor_radius: cylinder.radius,
                },
                "plane_cylinder_ellipse",
            ))
        }
        (CarrierEquation::Plane(plane), CarrierEquation::Sphere(sphere))
        | (CarrierEquation::Sphere(sphere), CarrierEquation::Plane(plane)) => {
            let normal = normalized(plane.normal)?;
            let signed_distance = dot(
                normal,
                std::array::from_fn(|index| sphere.center[index] - plane.origin[index]),
            );
            let radius_squared = sphere
                .radius
                .mul_add(sphere.radius, -(signed_distance * signed_distance));
            let scale = sphere.radius.max(1.0);
            if radius_squared <= 1e-18 * scale * scale {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] - signed_distance * normal[index]);
            let reference = normalized(std::array::from_fn(|index| {
                sphere.ref_direction[index] - dot(sphere.ref_direction, normal) * normal[index]
            }))
            .unwrap_or_else(|| {
                let reference = cadmpeg_ir::geometry::derive_reference_direction(Vector3::new(
                    normal[0], normal[1], normal[2],
                ));
                [reference.x, reference.y, reference.z]
            });
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(normal[0], normal[1], normal[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: radius_squared.sqrt(),
                },
                "plane_sphere_circle",
            ))
        }
        (CarrierEquation::Plane(plane), CarrierEquation::Cone(cone))
        | (CarrierEquation::Cone(cone), CarrierEquation::Plane(plane)) => {
            let normal = normalized(plane.normal)?;
            let axis = normalized(cone.axis)?;
            let alignment = dot(normal, axis);
            let slope = cone.half_angle.tan();
            if circular_cone(cone) && slope.abs() > 1e-12 {
                let apex: [f64; 3] = std::array::from_fn(|index| {
                    cone.origin[index] - (cone.radius / slope) * axis[index]
                });
                let plane_distance = dot(
                    normal,
                    std::array::from_fn(|index| apex[index] - plane.origin[index]),
                );
                let scale = cone.radius.max(1.0);
                if plane_distance.abs() <= 1e-9 * scale
                    && (alignment.abs() - cone.half_angle.sin()).abs() <= 1e-10
                {
                    let direction = normalized(std::array::from_fn(|index| {
                        axis[index] - alignment * normal[index]
                    }))?;
                    return Some((
                        CurveGeometry::Line {
                            origin: Point3::new(apex[0], apex[1], apex[2]),
                            direction: Vector3::new(direction[0], direction[1], direction[2]),
                        },
                        "plane_cone_tangent_line",
                    ));
                }
            }
            let apex_generators = apex_plane_cone_generator_candidates(
                CarrierEquation::Plane(plane),
                CarrierEquation::Cone(cone),
            );
            if apex_generators.len() == 1 {
                return apex_generators.into_iter().next();
            }
            if (alignment.abs() - 1.0).abs() <= 1e-10 {
                let axial = dot(
                    axis,
                    std::array::from_fn(|index| plane.origin[index] - cone.origin[index]),
                );
                let radius = (cone.radius + axial * cone.half_angle.tan()).abs();
                if radius <= 1e-12 {
                    return None;
                }
                let center: [f64; 3] =
                    std::array::from_fn(|index| cone.origin[index] + axial * axis[index]);
                let reference = normalized(cone.ref_direction)?;
                let (geometry, tag) = if circular_cone(cone) {
                    (
                        CurveGeometry::Circle {
                            center: Point3::new(center[0], center[1], center[2]),
                            axis: Vector3::new(normal[0], normal[1], normal[2]),
                            ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                            radius,
                        },
                        "plane_cone_circle",
                    )
                } else {
                    (
                        CurveGeometry::Ellipse {
                            center: Point3::new(center[0], center[1], center[2]),
                            axis: Vector3::new(normal[0], normal[1], normal[2]),
                            major_direction: Vector3::new(reference[0], reference[1], reference[2]),
                            major_radius: radius,
                            minor_radius: radius * cone.ratio,
                        },
                        "plane_cone_parallel_ellipse",
                    )
                };
                return Some((geometry, tag));
            }
            plane_cone_conic(plane, cone)
        }
        (CarrierEquation::Plane(plane), CarrierEquation::Torus(torus))
        | (CarrierEquation::Torus(torus), CarrierEquation::Plane(plane)) => {
            let normal = normalized(plane.normal)?;
            let axis = normalized(torus.axis)?;
            if (dot(normal, axis).abs() - 1.0).abs() > 1e-10 {
                return None;
            }
            let axial = dot(
                axis,
                std::array::from_fn(|index| plane.origin[index] - torus.center[index]),
            );
            let scale = torus.minor_radius.max(torus.major_radius).max(1.0);
            if (axial.abs() - torus.minor_radius).abs() > 1e-9 * scale {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| torus.center[index] + axial * axis[index]);
            let reference = normalized(torus.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(normal[0], normal[1], normal[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: torus.major_radius,
                },
                "plane_torus_tangent_circle",
            ))
        }
        (CarrierEquation::Cylinder(first), CarrierEquation::Cylinder(second)) => {
            let first_axis = normalized(first.axis)?;
            let second_axis = normalized(second.axis)?;
            let alignment = dot(first_axis, second_axis);
            if (alignment.abs() - 1.0).abs() > 1e-10 {
                return None;
            }
            let relative = std::array::from_fn(|index| second.origin[index] - first.origin[index]);
            let axial = dot(relative, first_axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * first_axis[index]);
            let distance = dot(transverse, transverse).sqrt();
            if distance <= 1e-12 {
                return None;
            }
            let external = first.radius + second.radius;
            let internal = (first.radius - second.radius).abs();
            let scale = external.max(distance).max(1.0);
            let first_fraction = if (distance - external).abs() <= 1e-9 * scale {
                first.radius / distance
            } else if (distance - internal).abs() <= 1e-9 * scale {
                let signed = if first.radius >= second.radius {
                    first.radius
                } else {
                    -first.radius
                };
                signed / distance
            } else {
                return None;
            };
            let origin: [f64; 3] = std::array::from_fn(|index| {
                first.origin[index] + first_fraction * transverse[index]
            });
            Some((
                CurveGeometry::Line {
                    origin: Point3::new(origin[0], origin[1], origin[2]),
                    direction: Vector3::new(first_axis[0], first_axis[1], first_axis[2]),
                },
                "parallel_cylinder_tangent_line",
            ))
        }
        (CarrierEquation::Sphere(first), CarrierEquation::Sphere(second)) => {
            let center_delta: [f64; 3] =
                std::array::from_fn(|index| second.center[index] - first.center[index]);
            let distance = dot(center_delta, center_delta).sqrt();
            if distance <= 1e-12
                || distance >= first.radius + second.radius
                || distance <= (first.radius - second.radius).abs()
            {
                return None;
            }
            let axis = center_delta.map(|value| value / distance);
            let axial = (distance * distance + first.radius * first.radius
                - second.radius * second.radius)
                / (2.0 * distance);
            let radius_squared = first.radius.mul_add(first.radius, -(axial * axial));
            if radius_squared <= 1e-18 {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| first.center[index] + axial * axis[index]);
            let reference = cadmpeg_ir::geometry::derive_reference_direction(Vector3::new(
                axis[0], axis[1], axis[2],
            ));
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: reference,
                    radius: radius_squared.sqrt(),
                },
                "sphere_intersection_circle",
            ))
        }
        (CarrierEquation::Cylinder(cylinder), CarrierEquation::Sphere(sphere))
        | (CarrierEquation::Sphere(sphere), CarrierEquation::Cylinder(cylinder)) => {
            let axis = normalized(cylinder.axis)?;
            let relative: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] - cylinder.origin[index]);
            let axial = dot(relative, axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * axis[index]);
            let scale = sphere.radius.max(cylinder.radius).max(1.0);
            if dot(transverse, transverse).sqrt() > 1e-9 * scale
                || (sphere.radius - cylinder.radius).abs() > 1e-9 * scale
            {
                return None;
            }
            let reference = normalized(cylinder.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(sphere.center[0], sphere.center[1], sphere.center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: cylinder.radius,
                },
                "coaxial_cylinder_sphere_circle",
            ))
        }
        (CarrierEquation::Cylinder(cylinder), CarrierEquation::Torus(torus))
        | (CarrierEquation::Torus(torus), CarrierEquation::Cylinder(cylinder)) => {
            let cylinder_axis = normalized(cylinder.axis)?;
            let torus_axis = normalized(torus.axis)?;
            if (dot(cylinder_axis, torus_axis).abs() - 1.0).abs() > 1e-10 {
                return None;
            }
            let relative: [f64; 3] =
                std::array::from_fn(|index| torus.center[index] - cylinder.origin[index]);
            let axial = dot(relative, cylinder_axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * cylinder_axis[index]);
            let scale = torus
                .major_radius
                .max(torus.minor_radius)
                .max(cylinder.radius)
                .max(1.0);
            if dot(transverse, transverse).sqrt() > 1e-9 * scale {
                return None;
            }
            let outer_radius = torus.major_radius + torus.minor_radius;
            let inner_radius = (torus.major_radius - torus.minor_radius).abs();
            if (cylinder.radius - outer_radius).abs() > 1e-9 * scale
                && (inner_radius <= 1e-12 || (cylinder.radius - inner_radius).abs() > 1e-9 * scale)
            {
                return None;
            }
            let reference = normalized(cylinder.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(torus.center[0], torus.center[1], torus.center[2]),
                    axis: Vector3::new(cylinder_axis[0], cylinder_axis[1], cylinder_axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: cylinder.radius,
                },
                "coaxial_cylinder_torus_tangent_circle",
            ))
        }
        (CarrierEquation::Cone(cone), CarrierEquation::Sphere(sphere))
        | (CarrierEquation::Sphere(sphere), CarrierEquation::Cone(cone)) => {
            if !circular_cone(cone) {
                return None;
            }
            let cone_axis = normalized(cone.axis)?;
            let relative: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] - cone.origin[index]);
            let axial = dot(relative, cone_axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * cone_axis[index]);
            let scale = cone.radius.max(sphere.radius).max(1.0);
            if dot(transverse, transverse).sqrt() > 1e-9 * scale {
                return None;
            }
            let slope = cone.half_angle.tan();
            if slope.abs() <= 1e-12 {
                return None;
            }
            let quadratic = 1.0 + slope * slope;
            let linear = 2.0 * (cone.radius * slope - axial);
            let constant =
                cone.radius * cone.radius + axial * axial - sphere.radius * sphere.radius;
            let discriminant = linear.mul_add(linear, -4.0 * quadratic * constant);
            let discriminant_scale = linear
                .abs()
                .max((4.0 * quadratic * constant).abs().sqrt())
                .max(1.0);
            if discriminant.abs() > 1e-9 * discriminant_scale * discriminant_scale {
                return None;
            }
            let cone_parameter = -linear / (2.0 * quadratic);
            let radius = (cone.radius + cone_parameter * slope).abs();
            if radius <= 1e-12 * scale {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| cone.origin[index] + cone_parameter * cone_axis[index]);
            let reference = normalized(cone.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(cone_axis[0], cone_axis[1], cone_axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius,
                },
                "coaxial_cone_sphere_tangent_circle",
            ))
        }
        (CarrierEquation::Sphere(sphere), CarrierEquation::Torus(torus))
        | (CarrierEquation::Torus(torus), CarrierEquation::Sphere(sphere)) => {
            let axis = normalized(torus.axis)?;
            let relative: [f64; 3] =
                std::array::from_fn(|index| torus.center[index] - sphere.center[index]);
            let axial = dot(relative, axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * axis[index]);
            let scale = torus
                .major_radius
                .max(torus.minor_radius)
                .max(sphere.radius)
                .max(1.0);
            if dot(transverse, transverse).sqrt() > 1e-9 * scale {
                return None;
            }
            let meridian_distance = torus.major_radius.hypot(axial);
            if meridian_distance <= 1e-12 {
                return None;
            }
            let external = sphere.radius + torus.minor_radius;
            let internal = (sphere.radius - torus.minor_radius).abs();
            if (meridian_distance - external).abs() > 1e-9 * scale
                && (meridian_distance - internal).abs() > 1e-9 * scale
            {
                return None;
            }
            let sphere_parameter = (meridian_distance * meridian_distance
                + sphere.radius * sphere.radius
                - torus.minor_radius * torus.minor_radius)
                / (2.0 * meridian_distance);
            let radius = (sphere_parameter * torus.major_radius / meridian_distance).abs();
            if radius <= 1e-12 * scale {
                return None;
            }
            let center_axial = sphere_parameter * axial / meridian_distance;
            let center: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] + center_axial * axis[index]);
            let reference = normalized(torus.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius,
                },
                "coaxial_sphere_torus_tangent_circle",
            ))
        }
        (CarrierEquation::Torus(first), CarrierEquation::Torus(second)) => {
            let first_axis = normalized(first.axis)?;
            let second_axis = normalized(second.axis)?;
            if (dot(first_axis, second_axis).abs() - 1.0).abs() > 1e-10 {
                return None;
            }
            let relative: [f64; 3] =
                std::array::from_fn(|index| second.center[index] - first.center[index]);
            let axial = dot(relative, first_axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * first_axis[index]);
            let scale = first
                .major_radius
                .max(first.minor_radius)
                .max(second.major_radius)
                .max(second.minor_radius)
                .max(1.0);
            if dot(transverse, transverse).sqrt() > 1e-9 * scale {
                return None;
            }
            let radial_delta = second.major_radius - first.major_radius;
            let meridian_distance = radial_delta.hypot(axial);
            if meridian_distance <= 1e-12 {
                return None;
            }
            let external = first.minor_radius + second.minor_radius;
            let internal = (first.minor_radius - second.minor_radius).abs();
            if (meridian_distance - external).abs() > 1e-9 * scale
                && (meridian_distance - internal).abs() > 1e-9 * scale
            {
                return None;
            }
            let first_parameter = (meridian_distance * meridian_distance
                + first.minor_radius * first.minor_radius
                - second.minor_radius * second.minor_radius)
                / (2.0 * meridian_distance);
            let radius =
                (first.major_radius + first_parameter * radial_delta / meridian_distance).abs();
            if radius <= 1e-12 * scale {
                return None;
            }
            let center_axial = first_parameter * axial / meridian_distance;
            let center: [f64; 3] =
                std::array::from_fn(|index| first.center[index] + center_axial * first_axis[index]);
            let reference = normalized(first.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(first_axis[0], first_axis[1], first_axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius,
                },
                "coaxial_tori_tangent_circle",
            ))
        }
        (
            CarrierEquation::Cone(_),
            CarrierEquation::Cylinder(_) | CarrierEquation::Cone(_) | CarrierEquation::Torus(_),
        )
        | (CarrierEquation::Cylinder(_) | CarrierEquation::Torus(_), CarrierEquation::Cone(_)) => {
            None
        }
    }
}

pub(super) fn parallel_plane_cylinder_generator_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Plane(plane), CarrierEquation::Cylinder(cylinder))
    | (CarrierEquation::Cylinder(cylinder), CarrierEquation::Plane(plane))) = (first, second)
    else {
        return Vec::new();
    };
    let Some(normal) = normalized(plane.normal) else {
        return Vec::new();
    };
    let Some(axis) = normalized(cylinder.axis) else {
        return Vec::new();
    };
    if dot(normal, axis).abs() > 1e-10 || cylinder.radius <= 0.0 {
        return Vec::new();
    }
    let signed_distance = dot(
        normal,
        std::array::from_fn(|index| cylinder.origin[index] - plane.origin[index]),
    );
    let scale = cylinder.radius.max(1.0);
    let offset_squared = cylinder
        .radius
        .mul_add(cylinder.radius, -(signed_distance * signed_distance));
    if offset_squared <= 1e-18 * scale * scale {
        return Vec::new();
    }
    let closest: [f64; 3] =
        std::array::from_fn(|index| cylinder.origin[index] - signed_distance * normal[index]);
    let Some(transverse) = normalized(cross(axis, normal)) else {
        return Vec::new();
    };
    let offset = offset_squared.sqrt();
    [-1.0, 1.0]
        .into_iter()
        .map(|sense| {
            let origin: [f64; 3] =
                std::array::from_fn(|index| closest[index] + sense * offset * transverse[index]);
            (
                CurveGeometry::Line {
                    origin: Point3::new(origin[0], origin[1], origin[2]),
                    direction: Vector3::new(axis[0], axis[1], axis[2]),
                },
                "plane_cylinder_secant_generator",
            )
        })
        .collect()
}

pub(super) fn parallel_cylinder_generator_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let (CarrierEquation::Cylinder(first), CarrierEquation::Cylinder(second)) = (first, second)
    else {
        return Vec::new();
    };
    let (Some(first_axis), Some(second_axis)) = (normalized(first.axis), normalized(second.axis))
    else {
        return Vec::new();
    };
    if (dot(first_axis, second_axis).abs() - 1.0).abs() > 1e-10
        || first.radius <= 0.0
        || second.radius <= 0.0
    {
        return Vec::new();
    }
    let relative: [f64; 3] =
        std::array::from_fn(|index| second.origin[index] - first.origin[index]);
    let axial = dot(relative, first_axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - axial * first_axis[index]);
    let distance = dot(transverse, transverse).sqrt();
    let scale = first.radius.max(second.radius).max(distance).max(1.0);
    if distance <= 1e-12 * scale
        || distance >= first.radius + second.radius - 1e-9 * scale
        || distance <= (first.radius - second.radius).abs() + 1e-9 * scale
    {
        return Vec::new();
    }
    let center_direction = transverse.map(|value| value / distance);
    let along = (first.radius * first.radius - second.radius * second.radius + distance * distance)
        / (2.0 * distance);
    let height_squared = first.radius.mul_add(first.radius, -(along * along));
    if height_squared <= 1e-12 * scale * scale {
        return Vec::new();
    }
    let Some(perpendicular) = normalized(cross(first_axis, center_direction)) else {
        return Vec::new();
    };
    let base: [f64; 3] =
        std::array::from_fn(|index| first.origin[index] + along * center_direction[index]);
    let height = height_squared.sqrt();
    [-height, height]
        .into_iter()
        .map(|offset| {
            let origin: [f64; 3] =
                std::array::from_fn(|index| base[index] + offset * perpendicular[index]);
            (
                CurveGeometry::Line {
                    origin: Point3::new(origin[0], origin[1], origin[2]),
                    direction: Vector3::new(first_axis[0], first_axis[1], first_axis[2]),
                },
                "parallel_cylinder_secant_generator",
            )
        })
        .collect()
}

pub(super) fn coaxial_cylinder_sphere_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Cylinder(cylinder), CarrierEquation::Sphere(sphere))
    | (CarrierEquation::Sphere(sphere), CarrierEquation::Cylinder(cylinder))) = (first, second)
    else {
        return Vec::new();
    };
    let Some(axis) = normalized(cylinder.axis) else {
        return Vec::new();
    };
    let relative: [f64; 3] =
        std::array::from_fn(|index| sphere.center[index] - cylinder.origin[index]);
    let axial = dot(relative, axis);
    let transverse: [f64; 3] = std::array::from_fn(|index| relative[index] - axial * axis[index]);
    let scale = sphere.radius.max(cylinder.radius).max(1.0);
    if sphere.radius <= 0.0
        || cylinder.radius <= 0.0
        || dot(transverse, transverse).sqrt() > 1e-9 * scale
    {
        return Vec::new();
    }
    let offset_squared = sphere
        .radius
        .mul_add(sphere.radius, -(cylinder.radius * cylinder.radius));
    if offset_squared <= 1e-9 * scale * scale {
        return Vec::new();
    }
    let Some(reference) = normalized(cylinder.ref_direction) else {
        return Vec::new();
    };
    let offset = offset_squared.sqrt();
    [-offset, offset]
        .into_iter()
        .map(|offset| {
            let center: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] + offset * axis[index]);
            (
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: cylinder.radius,
                },
                "coaxial_cylinder_sphere_secant_circle",
            )
        })
        .collect()
}

pub(super) fn coaxial_cone_cylinder_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Cone(cone), CarrierEquation::Cylinder(cylinder))
    | (CarrierEquation::Cylinder(cylinder), CarrierEquation::Cone(cone))) = (first, second)
    else {
        return Vec::new();
    };
    if !circular_cone(cone) {
        return Vec::new();
    }
    let (Some(cone_axis), Some(cylinder_axis), Some(reference)) = (
        normalized(cone.axis),
        normalized(cylinder.axis),
        normalized(cone.ref_direction),
    ) else {
        return Vec::new();
    };
    if (dot(cone_axis, cylinder_axis).abs() - 1.0).abs() > 1e-10 {
        return Vec::new();
    }
    let relative: [f64; 3] =
        std::array::from_fn(|index| cylinder.origin[index] - cone.origin[index]);
    let axial = dot(relative, cone_axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - axial * cone_axis[index]);
    let scale = cone.radius.max(cylinder.radius).max(1.0);
    let slope = cone.half_angle.tan();
    if dot(transverse, transverse).sqrt() > 1e-9 * scale
        || cylinder.radius <= 1e-12 * scale
        || cone.radius < 0.0
        || slope.abs() <= 1e-12
        || !slope.is_finite()
    {
        return Vec::new();
    }
    [cylinder.radius, -cylinder.radius]
        .into_iter()
        .map(|signed_radius| {
            let parameter = (signed_radius - cone.radius) / slope;
            let center: [f64; 3] =
                std::array::from_fn(|index| cone.origin[index] + parameter * cone_axis[index]);
            (
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(cone_axis[0], cone_axis[1], cone_axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: cylinder.radius,
                },
                "coaxial_cone_cylinder_secant_circle",
            )
        })
        .collect()
}

pub(super) fn coaxial_cones_section_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let (CarrierEquation::Cone(first), CarrierEquation::Cone(second)) = (first, second) else {
        return Vec::new();
    };
    if first.ratio <= 0.0
        || second.ratio <= 0.0
        || !first.ratio.is_finite()
        || !second.ratio.is_finite()
    {
        return Vec::new();
    }
    let (Some(first_axis), Some(second_axis), Some(reference), Some(second_reference)) = (
        normalized(first.axis),
        normalized(second.axis),
        normalized(first.ref_direction),
        normalized(second.ref_direction),
    ) else {
        return Vec::new();
    };
    let axis_alignment = dot(first_axis, second_axis);
    if (axis_alignment.abs() - 1.0).abs() > 1e-10
        || dot(first_axis, reference).abs() > 1e-10
        || dot(second_axis, second_reference).abs() > 1e-10
    {
        return Vec::new();
    }
    let first_y = cross(first_axis, reference);
    let second_y = cross(second_axis, second_reference);
    let second_metric = |direction: [f64; 3]| {
        let x = dot(direction, second_reference);
        let y = dot(direction, second_y) / second.ratio;
        x.mul_add(x, y * y)
    };
    let metric_xx = second_metric(reference);
    let metric_yy = second_metric(first_y);
    let metric_xy = dot(reference, second_reference).mul_add(
        dot(first_y, second_reference),
        dot(reference, second_y) * dot(first_y, second_y) / (second.ratio * second.ratio),
    );
    let metric_scale_squared = metric_xx;
    let metric_coefficient_scale = metric_xx.abs().max(metric_yy.abs()).max(1.0);
    if metric_scale_squared <= 0.0
        || !metric_scale_squared.is_finite()
        || !metric_yy.is_finite()
        || !metric_xy.is_finite()
        || metric_xy.abs() > 1e-10 * metric_coefficient_scale
        || (metric_yy - metric_scale_squared / (first.ratio * first.ratio)).abs()
            > 1e-10 * metric_coefficient_scale
    {
        return Vec::new();
    }
    let metric_scale = metric_scale_squared.sqrt();
    let relative: [f64; 3] =
        std::array::from_fn(|index| second.origin[index] - first.origin[index]);
    let second_origin_axial = dot(relative, first_axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - second_origin_axial * first_axis[index]);
    let scale = first.radius.max(second.radius).max(1.0);
    let first_slope = first.half_angle.tan();
    let second_slope = axis_alignment * second.half_angle.tan();
    let second_intercept = second.radius - second_slope * second_origin_axial;
    if dot(transverse, transverse).sqrt() > 1e-9 * scale
        || first.radius < 0.0
        || second.radius < 0.0
        || first_slope.abs() <= 1e-12
        || second_slope.abs() <= 1e-12
        || !first_slope.is_finite()
        || !second_slope.is_finite()
    {
        return Vec::new();
    }

    let mut parameters = Vec::<f64>::new();
    let scaled_first_slope = metric_scale * first_slope;
    let scaled_first_radius = metric_scale * first.radius;
    let slope_scale = scaled_first_slope.abs().max(second_slope.abs()).max(1.0);
    let intercept_scale = first
        .radius
        .max(scaled_first_radius.abs())
        .max(second_intercept.abs())
        .max(second.radius)
        .max(1.0);
    for radial_sense in [-1.0, 1.0] {
        let denominator = scaled_first_slope - radial_sense * second_slope;
        let numerator = radial_sense * second_intercept - scaled_first_radius;
        if denominator.abs() <= 1e-12 * slope_scale {
            if numerator.abs() <= 1e-9 * intercept_scale {
                return Vec::new();
            }
            continue;
        }
        let parameter = numerator / denominator;
        let radius = (first.radius + parameter * first_slope).abs();
        if radius <= 1e-12 * scale {
            continue;
        }
        if !parameters
            .iter()
            .any(|known| (parameter - known).abs() <= 1e-9 * scale)
        {
            parameters.push(parameter);
        }
    }
    parameters
        .into_iter()
        .map(|parameter| {
            let radius = (first.radius + parameter * first_slope).abs();
            let center: [f64; 3] =
                std::array::from_fn(|index| first.origin[index] + parameter * first_axis[index]);
            let (geometry, tag) = if circular_cone(first) {
                (
                    CurveGeometry::Circle {
                        center: Point3::new(center[0], center[1], center[2]),
                        axis: Vector3::new(first_axis[0], first_axis[1], first_axis[2]),
                        ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                        radius,
                    },
                    "coaxial_cones_circle",
                )
            } else {
                (
                    CurveGeometry::Ellipse {
                        center: Point3::new(center[0], center[1], center[2]),
                        axis: Vector3::new(first_axis[0], first_axis[1], first_axis[2]),
                        major_direction: Vector3::new(reference[0], reference[1], reference[2]),
                        major_radius: radius,
                        minor_radius: radius * first.ratio,
                    },
                    "coaxial_cones_ellipse",
                )
            };
            (geometry, tag)
        })
        .collect()
}

pub(super) fn apex_plane_cone_generator_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Plane(plane), CarrierEquation::Cone(cone))
    | (CarrierEquation::Cone(cone), CarrierEquation::Plane(plane))) = (first, second)
    else {
        return Vec::new();
    };
    let Some(normal) = normalized(plane.normal) else {
        return Vec::new();
    };
    let Some(axis) = normalized(cone.axis) else {
        return Vec::new();
    };
    let Some(x_axis) = normalized(cone.ref_direction) else {
        return Vec::new();
    };
    let slope = cone.half_angle.tan();
    if slope <= 1e-12
        || !slope.is_finite()
        || cone.radius < 0.0
        || cone.ratio <= 0.0
        || !cone.ratio.is_finite()
        || dot(axis, x_axis).abs() > 1e-10
    {
        return Vec::new();
    }
    let apex: [f64; 3] =
        std::array::from_fn(|index| cone.origin[index] - cone.radius / slope * axis[index]);
    let plane_distance = dot(
        normal,
        std::array::from_fn(|index| apex[index] - plane.origin[index]),
    );
    let scale = cone.radius.max(1.0);
    if plane_distance.abs() > 1e-9 * scale {
        return Vec::new();
    }
    let reference = cadmpeg_ir::geometry::derive_reference_direction(Vector3::new(
        normal[0], normal[1], normal[2],
    ));
    let plane_u = [reference.x, reference.y, reference.z];
    let plane_v = cross(normal, plane_u);
    let y_axis = cross(axis, x_axis);
    let cone_coordinates = |direction: [f64; 3]| {
        [
            dot(direction, x_axis),
            dot(direction, y_axis) / cone.ratio,
            dot(direction, axis),
        ]
    };
    let quadratic = |first: [f64; 3], second: [f64; 3]| {
        first[0].mul_add(
            second[0],
            first[1] * second[1] - slope * slope * first[2] * second[2],
        )
    };
    let u_coordinates = cone_coordinates(plane_u);
    let v_coordinates = cone_coordinates(plane_v);
    let quadratic_uu = quadratic(u_coordinates, u_coordinates);
    let quadratic_uv = quadratic(u_coordinates, v_coordinates);
    let quadratic_vv = quadratic(v_coordinates, v_coordinates);
    let coefficient_scale = quadratic_uu
        .abs()
        .max(quadratic_uv.abs())
        .max(quadratic_vv.abs())
        .max(1.0);
    let determinant = quadratic_uu.mul_add(quadratic_vv, -quadratic_uv * quadratic_uv);
    let determinant_tolerance = 1e-12 * coefficient_scale * coefficient_scale;
    if determinant > determinant_tolerance {
        return Vec::new();
    }
    let angle = 0.5 * (2.0 * quadratic_uv).atan2(quadratic_uu - quadratic_vv);
    let (sine, cosine) = angle.sin_cos();
    let first_direction: [f64; 3] =
        std::array::from_fn(|index| cosine * plane_u[index] + sine * plane_v[index]);
    let second_direction: [f64; 3] =
        std::array::from_fn(|index| -sine * plane_u[index] + cosine * plane_v[index]);
    let first_value = quadratic_uu * cosine * cosine
        + 2.0 * quadratic_uv * cosine * sine
        + quadratic_vv * sine * sine;
    let second_value = quadratic_uu * sine * sine - 2.0 * quadratic_uv * cosine * sine
        + quadratic_vv * cosine * cosine;
    let directions = if determinant.abs() <= determinant_tolerance {
        if first_value.abs() <= second_value.abs() {
            vec![first_direction]
        } else {
            vec![second_direction]
        }
    } else {
        let (negative_value, negative_direction, positive_value, positive_direction) =
            if first_value < 0.0 {
                (first_value, first_direction, second_value, second_direction)
            } else {
                (second_value, second_direction, first_value, first_direction)
            };
        let negative_weight = positive_value.sqrt();
        let positive_weight = (-negative_value).sqrt();
        [-1.0, 1.0]
            .into_iter()
            .filter_map(|sense| {
                normalized(std::array::from_fn(|index| {
                    negative_weight * negative_direction[index]
                        + sense * positive_weight * positive_direction[index]
                }))
            })
            .collect()
    };
    let tag = if directions.len() == 1 {
        "plane_cone_tangent_line"
    } else {
        "plane_cone_secant_generator"
    };
    directions
        .into_iter()
        .map(|direction| {
            (
                CurveGeometry::Line {
                    origin: Point3::new(apex[0], apex[1], apex[2]),
                    direction: Vector3::new(direction[0], direction[1], direction[2]),
                },
                tag,
            )
        })
        .collect()
}

pub(super) fn coaxial_cone_sphere_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Cone(cone), CarrierEquation::Sphere(sphere))
    | (CarrierEquation::Sphere(sphere), CarrierEquation::Cone(cone))) = (first, second)
    else {
        return Vec::new();
    };
    if !circular_cone(cone) {
        return Vec::new();
    }
    let Some(axis) = normalized(cone.axis) else {
        return Vec::new();
    };
    let relative: [f64; 3] = std::array::from_fn(|index| sphere.center[index] - cone.origin[index]);
    let sphere_axial = dot(relative, axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - sphere_axial * axis[index]);
    let scale = cone.radius.max(sphere.radius).max(1.0);
    if dot(transverse, transverse).sqrt() > 1e-9 * scale {
        return Vec::new();
    }
    let slope = cone.half_angle.tan();
    if slope.abs() <= 1e-12 || !slope.is_finite() || cone.radius < 0.0 {
        return Vec::new();
    }
    let quadratic = 1.0 + slope * slope;
    let linear = 2.0 * (cone.radius * slope - sphere_axial);
    let constant =
        cone.radius * cone.radius + sphere_axial * sphere_axial - sphere.radius * sphere.radius;
    let discriminant = linear.mul_add(linear, -4.0 * quadratic * constant);
    let discriminant_scale = linear
        .abs()
        .max((4.0 * quadratic * constant).abs().sqrt())
        .max(1.0);
    if discriminant <= 1e-9 * discriminant_scale * discriminant_scale {
        return Vec::new();
    }
    let Some(reference) = normalized(cone.ref_direction) else {
        return Vec::new();
    };
    let root_delta = discriminant.sqrt();
    [-root_delta, root_delta]
        .into_iter()
        .filter_map(|delta| {
            let parameter = (-linear + delta) / (2.0 * quadratic);
            let radius = (cone.radius + parameter * slope).abs();
            if radius <= 1e-12 * scale {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| cone.origin[index] + parameter * axis[index]);
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius,
                },
                "coaxial_cone_sphere_secant_circle",
            ))
        })
        .collect()
}

pub(super) fn coaxial_cone_torus_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Cone(cone), CarrierEquation::Torus(torus))
    | (CarrierEquation::Torus(torus), CarrierEquation::Cone(cone))) = (first, second)
    else {
        return Vec::new();
    };
    if !circular_cone(cone) {
        return Vec::new();
    }
    let (Some(cone_axis), Some(torus_axis), Some(reference)) = (
        normalized(cone.axis),
        normalized(torus.axis),
        normalized(cone.ref_direction),
    ) else {
        return Vec::new();
    };
    if (dot(cone_axis, torus_axis).abs() - 1.0).abs() > 1e-10 {
        return Vec::new();
    }
    let relative: [f64; 3] = std::array::from_fn(|index| torus.center[index] - cone.origin[index]);
    let torus_axial = dot(relative, cone_axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - torus_axial * cone_axis[index]);
    let scale = cone
        .radius
        .max(torus.major_radius)
        .max(torus.minor_radius)
        .max(1.0);
    let slope = cone.half_angle.tan();
    if dot(transverse, transverse).sqrt() > 1e-9 * scale
        || cone.radius < 0.0
        || torus.major_radius <= 1e-12 * scale
        || torus.minor_radius <= 1e-12 * scale
        || slope.abs() <= 1e-12
        || !slope.is_finite()
    {
        return Vec::new();
    }

    let quadratic = 1.0 + slope * slope;
    let mut parameters = Vec::<f64>::new();
    for radial_sense in [-1.0, 1.0] {
        let radial_offset = radial_sense * cone.radius - torus.major_radius;
        let radial_slope = radial_sense * slope;
        let linear = 2.0 * (radial_offset * radial_slope - torus_axial);
        let constant = radial_offset * radial_offset + torus_axial * torus_axial
            - torus.minor_radius * torus.minor_radius;
        let discriminant = linear.mul_add(linear, -4.0 * quadratic * constant);
        let discriminant_scale = linear
            .abs()
            .max((4.0 * quadratic * constant).abs().sqrt())
            .max(1.0);
        let tolerance = 1e-9 * discriminant_scale * discriminant_scale;
        let deltas = if discriminant < -tolerance {
            continue;
        } else if discriminant.abs() <= tolerance {
            vec![0.0]
        } else {
            let root = discriminant.sqrt();
            vec![-root, root]
        };
        for delta in deltas {
            let parameter = (-linear + delta) / (2.0 * quadratic);
            let radius = radial_sense * (cone.radius + parameter * slope);
            if radius <= 1e-12 * scale {
                continue;
            }
            if !parameters
                .iter()
                .any(|known| (parameter - known).abs() <= 1e-9 * scale)
            {
                parameters.push(parameter);
            }
        }
    }
    parameters
        .into_iter()
        .map(|parameter| {
            let radius = (cone.radius + parameter * slope).abs();
            let center: [f64; 3] =
                std::array::from_fn(|index| cone.origin[index] + parameter * cone_axis[index]);
            (
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(cone_axis[0], cone_axis[1], cone_axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius,
                },
                "coaxial_cone_torus_circle",
            )
        })
        .collect()
}

pub(super) fn coaxial_cylinder_torus_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Cylinder(cylinder), CarrierEquation::Torus(torus))
    | (CarrierEquation::Torus(torus), CarrierEquation::Cylinder(cylinder))) = (first, second)
    else {
        return Vec::new();
    };
    let (Some(cylinder_axis), Some(torus_axis), Some(reference)) = (
        normalized(cylinder.axis),
        normalized(torus.axis),
        normalized(cylinder.ref_direction),
    ) else {
        return Vec::new();
    };
    if (dot(cylinder_axis, torus_axis).abs() - 1.0).abs() > 1e-10 {
        return Vec::new();
    }
    let relative: [f64; 3] =
        std::array::from_fn(|index| torus.center[index] - cylinder.origin[index]);
    let axial = dot(relative, cylinder_axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - axial * cylinder_axis[index]);
    let scale = torus
        .major_radius
        .max(torus.minor_radius)
        .max(cylinder.radius)
        .max(1.0);
    if dot(transverse, transverse).sqrt() > 1e-9 * scale {
        return Vec::new();
    }
    let radial_delta = cylinder.radius - torus.major_radius;
    let height_squared = torus
        .minor_radius
        .mul_add(torus.minor_radius, -(radial_delta * radial_delta));
    if height_squared <= 1e-9 * scale * scale || cylinder.radius <= 1e-12 * scale {
        return Vec::new();
    }
    let height = height_squared.sqrt();
    [-height, height]
        .into_iter()
        .map(|offset| {
            let center: [f64; 3] =
                std::array::from_fn(|index| torus.center[index] + offset * torus_axis[index]);
            (
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(torus_axis[0], torus_axis[1], torus_axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: cylinder.radius,
                },
                "coaxial_cylinder_torus_secant_circle",
            )
        })
        .collect()
}

pub(super) fn axis_normal_plane_torus_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Plane(plane), CarrierEquation::Torus(torus))
    | (CarrierEquation::Torus(torus), CarrierEquation::Plane(plane))) = (first, second)
    else {
        return Vec::new();
    };
    let (Some(normal), Some(axis), Some(reference)) = (
        normalized(plane.normal),
        normalized(torus.axis),
        normalized(torus.ref_direction),
    ) else {
        return Vec::new();
    };
    if (dot(normal, axis).abs() - 1.0).abs() > 1e-10 {
        return Vec::new();
    }
    let relative: [f64; 3] = std::array::from_fn(|index| plane.origin[index] - torus.center[index]);
    let axial = dot(relative, axis);
    let scale = torus.major_radius.max(torus.minor_radius).max(1.0);
    let radial_offset_squared = torus
        .minor_radius
        .mul_add(torus.minor_radius, -(axial * axial));
    if radial_offset_squared <= 1e-9 * scale * scale {
        return Vec::new();
    }
    let center: [f64; 3] = std::array::from_fn(|index| torus.center[index] + axial * axis[index]);
    let radial_offset = radial_offset_squared.sqrt();
    [
        torus.major_radius - radial_offset,
        torus.major_radius + radial_offset,
    ]
    .into_iter()
    .filter(|radius| *radius > 1e-12 * scale)
    .map(|radius| {
        (
            CurveGeometry::Circle {
                center: Point3::new(center[0], center[1], center[2]),
                axis: Vector3::new(axis[0], axis[1], axis[2]),
                ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                radius,
            },
            "plane_torus_secant_circle",
        )
    })
    .collect()
}

pub(super) fn meridian_circle_intersections(
    first_center: [f64; 2],
    first_radius: f64,
    second_center: [f64; 2],
    second_radius: f64,
    scale: f64,
) -> Vec<[f64; 2]> {
    let delta = [
        second_center[0] - first_center[0],
        second_center[1] - first_center[1],
    ];
    let distance = delta[0].hypot(delta[1]);
    if distance <= 1e-12 * scale
        || distance >= first_radius + second_radius - 1e-9 * scale
        || distance <= (first_radius - second_radius).abs() + 1e-9 * scale
    {
        return Vec::new();
    }
    let along = (distance * distance + first_radius * first_radius - second_radius * second_radius)
        / (2.0 * distance);
    let height_squared = first_radius.mul_add(first_radius, -(along * along));
    if height_squared <= 1e-12 * scale * scale {
        return Vec::new();
    }
    let unit = [delta[0] / distance, delta[1] / distance];
    let height = height_squared.sqrt();
    [-height, height]
        .into_iter()
        .map(|sense| {
            [
                first_center[0] + along * unit[0] - sense * unit[1],
                first_center[1] + along * unit[1] + sense * unit[0],
            ]
        })
        .collect()
}

pub(super) fn axis_containing_plane_torus_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Plane(plane), CarrierEquation::Torus(torus))
    | (CarrierEquation::Torus(torus), CarrierEquation::Plane(plane))) = (first, second)
    else {
        return Vec::new();
    };
    let (Some(normal), Some(axis)) = (normalized(plane.normal), normalized(torus.axis)) else {
        return Vec::new();
    };
    let scale = torus.major_radius.max(torus.minor_radius).max(1.0);
    let center_offset: [f64; 3] =
        std::array::from_fn(|index| torus.center[index] - plane.origin[index]);
    if dot(normal, axis).abs() > 1e-10
        || dot(normal, center_offset).abs() > 1e-9 * scale
        || !torus.major_radius.is_finite()
        || !torus.minor_radius.is_finite()
        || torus.major_radius <= 1e-12 * scale
        || torus.minor_radius <= 1e-12 * scale
    {
        return Vec::new();
    }
    let Some(radial) = normalized(cross(normal, axis)) else {
        return Vec::new();
    };
    [-1.0, 1.0]
        .into_iter()
        .map(|sense| {
            let center: [f64; 3] = std::array::from_fn(|index| {
                torus.center[index] + sense * torus.major_radius * radial[index]
            });
            (
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(normal[0], normal[1], normal[2]),
                    ref_direction: Vector3::new(axis[0], axis[1], axis[2]),
                    radius: torus.minor_radius,
                },
                "axis_containing_plane_torus_meridian_circle",
            )
        })
        .collect()
}

pub(super) fn coaxial_sphere_torus_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Sphere(sphere), CarrierEquation::Torus(torus))
    | (CarrierEquation::Torus(torus), CarrierEquation::Sphere(sphere))) = (first, second)
    else {
        return Vec::new();
    };
    let (Some(axis), Some(reference)) = (normalized(torus.axis), normalized(torus.ref_direction))
    else {
        return Vec::new();
    };
    let relative: [f64; 3] =
        std::array::from_fn(|index| torus.center[index] - sphere.center[index]);
    let axial = dot(relative, axis);
    let transverse: [f64; 3] = std::array::from_fn(|index| relative[index] - axial * axis[index]);
    let scale = torus
        .major_radius
        .max(torus.minor_radius)
        .max(sphere.radius)
        .max(1.0);
    if dot(transverse, transverse).sqrt() > 1e-9 * scale {
        return Vec::new();
    }
    meridian_circle_intersections(
        [0.0, 0.0],
        sphere.radius,
        [torus.major_radius, axial],
        torus.minor_radius,
        scale,
    )
    .into_iter()
    .filter_map(|[radius, center_axial]| {
        let radius = radius.abs();
        if radius <= 1e-12 * scale {
            return None;
        }
        let center: [f64; 3] =
            std::array::from_fn(|index| sphere.center[index] + center_axial * axis[index]);
        Some((
            CurveGeometry::Circle {
                center: Point3::new(center[0], center[1], center[2]),
                axis: Vector3::new(axis[0], axis[1], axis[2]),
                ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                radius,
            },
            "coaxial_sphere_torus_secant_circle",
        ))
    })
    .collect()
}

pub(super) fn coaxial_tori_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let (CarrierEquation::Torus(first), CarrierEquation::Torus(second)) = (first, second) else {
        return Vec::new();
    };
    let (Some(first_axis), Some(second_axis), Some(reference)) = (
        normalized(first.axis),
        normalized(second.axis),
        normalized(first.ref_direction),
    ) else {
        return Vec::new();
    };
    if (dot(first_axis, second_axis).abs() - 1.0).abs() > 1e-10 {
        return Vec::new();
    }
    let relative: [f64; 3] =
        std::array::from_fn(|index| second.center[index] - first.center[index]);
    let axial = dot(relative, first_axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - axial * first_axis[index]);
    let scale = first
        .major_radius
        .max(first.minor_radius)
        .max(second.major_radius)
        .max(second.minor_radius)
        .max(1.0);
    if dot(transverse, transverse).sqrt() > 1e-9 * scale {
        return Vec::new();
    }
    meridian_circle_intersections(
        [first.major_radius, 0.0],
        first.minor_radius,
        [second.major_radius, axial],
        second.minor_radius,
        scale,
    )
    .into_iter()
    .filter_map(|[radius, center_axial]| {
        let radius = radius.abs();
        if radius <= 1e-12 * scale {
            return None;
        }
        let center: [f64; 3] =
            std::array::from_fn(|index| first.center[index] + center_axial * first_axis[index]);
        Some((
            CurveGeometry::Circle {
                center: Point3::new(center[0], center[1], center[2]),
                axis: Vector3::new(first_axis[0], first_axis[1], first_axis[2]),
                ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                radius,
            },
            "coaxial_tori_secant_circle",
        ))
    })
    .collect()
}

pub(super) fn multi_component_intersection_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let mut candidates = parallel_plane_cylinder_generator_candidates(first, second);
    candidates.extend(parallel_cylinder_generator_candidates(first, second));
    candidates.extend(coaxial_cylinder_sphere_circle_candidates(first, second));
    candidates.extend(coaxial_cone_cylinder_circle_candidates(first, second));
    candidates.extend(coaxial_cones_section_candidates(first, second));
    candidates.extend(apex_plane_cone_generator_candidates(first, second));
    candidates.extend(coaxial_cone_sphere_circle_candidates(first, second));
    candidates.extend(coaxial_cone_torus_circle_candidates(first, second));
    candidates.extend(coaxial_cylinder_torus_circle_candidates(first, second));
    candidates.extend(coaxial_sphere_torus_circle_candidates(first, second));
    candidates.extend(coaxial_tori_circle_candidates(first, second));
    candidates.extend(axis_normal_plane_torus_circle_candidates(first, second));
    candidates.extend(axis_containing_plane_torus_circle_candidates(first, second));
    candidates
}

pub(super) fn carrier_intersection_components(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    carrier_intersection_curve(first, second)
        .into_iter()
        .chain(multi_component_intersection_candidates(first, second))
        .collect()
}

pub(super) fn intersect_plane_with_carrier_components(
    plane: PlaneEquation,
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<[f64; 3]> {
    carrier_intersection_components(first, second)
        .into_iter()
        .filter_map(|(geometry, _)| circle_parameters(&geometry))
        .flat_map(|(center, axis, radius)| intersect_plane_with_circle(plane, center, axis, radius))
        .collect()
}

pub(super) fn curve_contains_points(geometry: &CurveGeometry, points: [[f64; 3]; 2]) -> bool {
    match geometry {
        CurveGeometry::Line { origin, direction } => {
            let origin = [origin.x, origin.y, origin.z];
            let Some(direction) = normalized([direction.x, direction.y, direction.z]) else {
                return false;
            };
            points.into_iter().all(|point| {
                let relative: [f64; 3] = std::array::from_fn(|index| point[index] - origin[index]);
                let residual = cross(relative, direction);
                let scale = dot(relative, relative).sqrt().max(1.0);
                dot(residual, residual).sqrt() <= 1e-7 * scale
            })
        }
        CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. } => {
            let Some(PeriodicConicFrame {
                center,
                normal,
                x_axis,
                y_axis,
                radii,
            }) = periodic_conic_frame(geometry)
            else {
                return false;
            };
            points.into_iter().all(|point| {
                let relative: [f64; 3] = std::array::from_fn(|index| point[index] - center[index]);
                let scale = radii.into_iter().fold(1.0, f64::max);
                let x = dot(relative, x_axis) / radii[0];
                let y = dot(relative, y_axis) / radii[1];
                dot(relative, normal).abs() <= 1e-7 * scale
                    && x.mul_add(x, y * y).is_finite()
                    && (x.mul_add(x, y * y) - 1.0).abs() <= 1e-7
            })
        }
        CurveGeometry::Parabola { .. } | CurveGeometry::Hyperbola { .. } => points
            .into_iter()
            .all(|point| nonperiodic_conic_parameter(geometry, point).is_some()),
        _ => false,
    }
}

pub(super) fn select_unique_curve_candidate(
    candidates: Vec<(CurveGeometry, &'static str)>,
    points: [[f64; 3]; 2],
) -> Option<(CurveGeometry, &'static str)> {
    let candidates = candidates
        .into_iter()
        .filter(|(geometry, _)| curve_contains_points(geometry, points))
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

pub(super) fn resolve_curve_candidates(
    candidates: Vec<(CurveGeometry, &'static str)>,
    points: Option<[[f64; 3]; 2]>,
) -> Option<(CurveGeometry, &'static str)> {
    if let Some(points) = points {
        return select_unique_curve_candidate(candidates, points);
    }
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

pub(super) fn fc14_held_coordinate(
    coordinates: &[crate::curve::FcCurveCoordinates],
    curve_id: u32,
) -> Option<f64> {
    let mut records = coordinates
        .iter()
        .filter(|record| record.curve_id == curve_id && record.subtype == 0x14);
    let record = records.next()?;
    records.next().is_none().then_some(())?;
    let tokens = record
        .tokens
        .iter()
        .filter(|token| token.raw.first() == Some(&0x2d))
        .collect::<Vec<_>>();
    (tokens.len() >= 4).then_some(())?;
    let first = tokens[0];
    (first.value_mm.is_finite()
        && tokens
            .iter()
            .all(|token| token.raw == first.raw && token.value_mm == first.value_mm))
    .then_some(first.value_mm)
}

pub(super) fn select_fc14_axis_coordinate_candidate(
    candidates: Vec<(CurveGeometry, &'static str)>,
    held_coordinate: f64,
) -> Option<(CurveGeometry, &'static str)> {
    let matching =
        candidates
            .into_iter()
            .filter(|(geometry, tag)| {
                if *tag != "coaxial_cone_cylinder_secant_circle" {
                    return false;
                }
                let CurveGeometry::Circle { center, axis, .. } = geometry else {
                    return false;
                };
                let axis = [axis.x, axis.y, axis.z];
                let Some(axis_index) = axis.iter().enumerate().find_map(|(index, value)| {
                    ((value.abs() - 1.0).abs() <= 1e-10).then_some(index)
                }) else {
                    return false;
                };
                if axis
                    .iter()
                    .enumerate()
                    .any(|(index, value)| index != axis_index && value.abs() > 1e-10)
                {
                    return false;
                }
                let center = [center.x, center.y, center.z];
                let scale = center[axis_index].abs().max(held_coordinate.abs()).max(1.0);
                (center[axis_index] - held_coordinate).abs() <= 1e-9 * scale
            })
            .collect::<Vec<_>>();
    let [candidate] = matching.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

#[derive(Clone)]
pub(super) struct NurbsSurfaceBoundary {
    pub(super) curve: NurbsCurve,
    pub(super) control_indices: Vec<usize>,
    pub(super) transverse_periodic: bool,
}

pub(super) fn nurbs_surface_boundaries(nurbs: &NurbsSurface) -> Option<[NurbsSurfaceBoundary; 4]> {
    let u_count = usize::try_from(nurbs.u_count).ok()?;
    let v_count = usize::try_from(nurbs.v_count).ok()?;
    let pole_count = u_count.checked_mul(v_count)?;
    (u_count >= 2
        && v_count >= 2
        && nurbs.control_points.len() == pole_count
        && nurbs
            .control_points
            .iter()
            .all(|point| point.x.is_finite() && point.y.is_finite() && point.z.is_finite())
        && nurbs.weights.as_ref().is_none_or(|weights| {
            weights.len() == pole_count
                && weights
                    .iter()
                    .all(|weight| weight.is_finite() && *weight > 0.0)
        }))
    .then_some(())?;
    let boundaries = [
        (false, (0..v_count).collect::<Vec<_>>()),
        (
            false,
            ((u_count - 1) * v_count..u_count * v_count).collect(),
        ),
        (true, (0..u_count).map(|u| u * v_count).collect()),
        (
            true,
            (0..u_count).map(|u| u * v_count + v_count - 1).collect(),
        ),
    ];
    Some(boundaries.map(|(along_u, control_indices)| {
        let (degree, knots, periodic, transverse_periodic) = if along_u {
            (
                nurbs.u_degree,
                nurbs.u_knots.clone(),
                nurbs.u_periodic,
                nurbs.v_periodic,
            )
        } else {
            (
                nurbs.v_degree,
                nurbs.v_knots.clone(),
                nurbs.v_periodic,
                nurbs.u_periodic,
            )
        };
        NurbsSurfaceBoundary {
            curve: NurbsCurve {
                degree,
                knots,
                control_points: control_indices
                    .iter()
                    .map(|index| nurbs.control_points[*index])
                    .collect(),
                weights: nurbs.weights.as_ref().map(|weights| {
                    control_indices
                        .iter()
                        .map(|index| weights[*index])
                        .collect()
                }),
                periodic,
            },
            control_indices,
            transverse_periodic,
        }
    }))
}

pub(super) fn point_tolerance<'a>(points: impl Iterator<Item = &'a Point3>) -> Option<f64> {
    let points = points.collect::<Vec<_>>();
    let anchor = **points.first()?;
    let extent = points
        .iter()
        .flat_map(|point| [point.x - anchor.x, point.y - anchor.y, point.z - anchor.z])
        .map(f64::abs)
        .fold(1.0, f64::max);
    let coordinate_scale = points
        .iter()
        .flat_map(|point| [point.x, point.y, point.z])
        .map(f64::abs)
        .fold(1.0, f64::max);
    Some((1e-9 * extent).max(32.0 * f64::EPSILON * coordinate_scale))
}

pub(super) fn nurbs_plane_boundary_curve(
    nurbs: &NurbsSurface,
    plane: PlaneEquation,
) -> Option<CurveGeometry> {
    let boundaries = nurbs_surface_boundaries(nurbs)?;
    let normal = normalized(plane.normal)?;
    let tolerance = point_tolerance(nurbs.control_points.iter())?
        .max(32.0 * f64::EPSILON * plane.origin.into_iter().map(f64::abs).fold(1.0, f64::max));
    let signed_distances = nurbs
        .control_points
        .iter()
        .map(|point| {
            dot(
                normal,
                [
                    point.x - plane.origin[0],
                    point.y - plane.origin[1],
                    point.z - plane.origin[2],
                ],
            )
        })
        .collect::<Vec<_>>();
    signed_distances
        .iter()
        .all(|distance| distance.is_finite())
        .then_some(())?;
    let candidates = boundaries
        .into_iter()
        .filter(|boundary| {
            !boundary.transverse_periodic
                && boundary
                    .control_indices
                    .iter()
                    .all(|index| signed_distances[*index].abs() <= tolerance)
                && {
                    let outside = signed_distances
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| !boundary.control_indices.contains(index))
                        .map(|(_, distance)| *distance)
                        .collect::<Vec<_>>();
                    !outside.is_empty()
                        && (outside.iter().all(|distance| *distance > tolerance)
                            || outside.iter().all(|distance| *distance < -tolerance))
                }
        })
        .collect::<Vec<_>>();
    let [boundary] = candidates.as_slice() else {
        return None;
    };
    Some(CurveGeometry::Nurbs(boundary.curve.clone()))
}

pub(super) fn scalar_near(left: f64, right: f64, tolerance: f64) -> bool {
    (left - right).abs() <= tolerance
}

pub(super) fn normalized_knot_vector(knots: &[f64]) -> Option<Vec<f64>> {
    let (&minimum, &maximum) = knots.first().zip(knots.last())?;
    let span = maximum - minimum;
    (span.is_finite() && span > 0.0)
        .then(|| knots.iter().map(|knot| (knot - minimum) / span).collect())
}

pub(super) fn nurbs_curves_match(
    left: &NurbsCurve,
    right: &NurbsCurve,
    reversed: bool,
    point_tolerance: f64,
) -> bool {
    if left.degree != right.degree
        || left.periodic != right.periodic
        || left.control_points.len() != right.control_points.len()
        || left.knots.len() != right.knots.len()
        || left.weights.is_some() != right.weights.is_some()
    {
        return false;
    }
    let right_points = if reversed {
        right.control_points.iter().rev().collect::<Vec<_>>()
    } else {
        right.control_points.iter().collect()
    };
    if !left
        .control_points
        .iter()
        .zip(right_points)
        .all(|(left, right)| {
            dot(
                [left.x - right.x, left.y - right.y, left.z - right.z],
                [left.x - right.x, left.y - right.y, left.z - right.z],
            )
            .sqrt()
                <= point_tolerance
        })
    {
        return false;
    }
    let Some(left_knots) = normalized_knot_vector(&left.knots) else {
        return false;
    };
    let Some(right_knots) = normalized_knot_vector(&right.knots) else {
        return false;
    };
    let knots_match = if reversed {
        left_knots
            .iter()
            .zip(right_knots.iter().rev())
            .all(|(left, right)| scalar_near(*left, 1.0 - right, 1e-12))
    } else {
        left_knots
            .iter()
            .zip(&right_knots)
            .all(|(left, right)| scalar_near(*left, *right, 1e-12))
    };
    if !knots_match {
        return false;
    }
    match (&left.weights, &right.weights) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            let right = if reversed {
                right.iter().rev().collect::<Vec<_>>()
            } else {
                right.iter().collect()
            };
            let Some(scale) = left
                .first()
                .zip(right.first())
                .map(|(left, right)| left / **right)
            else {
                return false;
            };
            scale.is_finite()
                && scale > 0.0
                && left.iter().zip(right).all(|(left, right)| {
                    scalar_near(
                        *left,
                        scale * right,
                        1e-12 * left.abs().max((scale * right).abs()).max(1.0),
                    )
                })
        }
        _ => false,
    }
}

pub(super) fn generator_separates_control_nets(
    first: &NurbsSurface,
    first_boundary: &NurbsSurfaceBoundary,
    second: &NurbsSurface,
    second_boundary: &NurbsSurfaceBoundary,
) -> bool {
    let [origin, end] = first_boundary.curve.control_points.as_slice() else {
        return false;
    };
    let generator = [end.x - origin.x, end.y - origin.y, end.z - origin.z];
    let Some(generator) = normalized(generator) else {
        return false;
    };
    let seed = if generator[0].abs() < 0.8 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let Some(first_axis) = normalized(cross(generator, seed)) else {
        return false;
    };
    let second_axis = cross(generator, first_axis);
    let first_outside = first
        .control_points
        .iter()
        .enumerate()
        .filter(|(index, _)| !first_boundary.control_indices.contains(index))
        .map(|(_, point)| point)
        .collect::<Vec<_>>();
    let second_outside = second
        .control_points
        .iter()
        .enumerate()
        .filter(|(index, _)| !second_boundary.control_indices.contains(index))
        .map(|(_, point)| point)
        .collect::<Vec<_>>();
    if first_outside.is_empty() || second_outside.is_empty() {
        return false;
    }
    let offset = |point: &Point3| [point.x - origin.x, point.y - origin.y, point.z - origin.z];
    let mut boundary_angles = first_outside
        .iter()
        .chain(&second_outside)
        .flat_map(|point| {
            let offset = offset(point);
            let angle = dot(second_axis, offset).atan2(dot(first_axis, offset));
            [
                (angle + std::f64::consts::FRAC_PI_2).rem_euclid(std::f64::consts::TAU),
                (angle - std::f64::consts::FRAC_PI_2).rem_euclid(std::f64::consts::TAU),
            ]
        })
        .collect::<Vec<_>>();
    boundary_angles.sort_by(f64::total_cmp);
    let tolerance = point_tolerance(first.control_points.iter().chain(&second.control_points))
        .unwrap_or(f64::INFINITY);
    (0..boundary_angles.len()).any(|index| {
        let start = boundary_angles[index];
        let end = if index + 1 == boundary_angles.len() {
            boundary_angles[0] + std::f64::consts::TAU
        } else {
            boundary_angles[index + 1]
        };
        let angle = f64::midpoint(start, end);
        let normal = [
            angle.cos() * first_axis[0] + angle.sin() * second_axis[0],
            angle.cos() * first_axis[1] + angle.sin() * second_axis[1],
            angle.cos() * first_axis[2] + angle.sin() * second_axis[2],
        ];
        let first_distances = first_outside
            .iter()
            .map(|point| dot(normal, offset(point)))
            .collect::<Vec<_>>();
        let second_distances = second_outside
            .iter()
            .map(|point| dot(normal, offset(point)))
            .collect::<Vec<_>>();
        (first_distances.iter().all(|distance| *distance > tolerance)
            && second_distances
                .iter()
                .all(|distance| *distance < -tolerance))
            || (first_distances
                .iter()
                .all(|distance| *distance < -tolerance)
                && second_distances
                    .iter()
                    .all(|distance| *distance > tolerance))
    })
}

pub(super) fn shared_extrusion_generator_curve(
    first: &NurbsSurface,
    second: &NurbsSurface,
) -> Option<CurveGeometry> {
    let first_boundaries = nurbs_surface_boundaries(first)?;
    let second_boundaries = nurbs_surface_boundaries(second)?;
    let tolerance = point_tolerance(first.control_points.iter().chain(&second.control_points))?;
    let candidates = first_boundaries
        .iter()
        .flat_map(|first_boundary| {
            second_boundaries
                .iter()
                .filter(|second_boundary| {
                    first_boundary.curve.degree == 1
                        && !first_boundary.curve.periodic
                        && !first_boundary.transverse_periodic
                        && !second_boundary.transverse_periodic
                        && first_boundary.curve.control_points.len() == 2
                        && [false, true].into_iter().any(|reversed| {
                            nurbs_curves_match(
                                &first_boundary.curve,
                                &second_boundary.curve,
                                reversed,
                                tolerance,
                            )
                        })
                        && generator_separates_control_nets(
                            first,
                            first_boundary,
                            second,
                            second_boundary,
                        )
                })
                .map(|_| first_boundary.curve.clone())
        })
        .collect::<Vec<_>>();
    let [curve] = candidates.as_slice() else {
        return None;
    };
    Some(CurveGeometry::Nurbs(curve.clone()))
}

pub(super) fn cubic_unit_interval_roots(
    cubic: f64,
    quadratic: f64,
    linear: f64,
    constant: f64,
    value_tolerance: f64,
) -> Vec<f64> {
    let scale = cubic
        .abs()
        .max(quadratic.abs())
        .max(linear.abs())
        .max(constant.abs());
    if scale <= value_tolerance {
        return Vec::new();
    }
    let parameter_tolerance = 1e-11;
    let evaluate = |parameter: f64| {
        ((cubic * parameter + quadratic) * parameter + linear) * parameter + constant
    };
    if cubic.abs() <= 1e-14 * scale {
        let mut roots = quadratic_real_roots(quadratic, linear, constant)
            .into_iter()
            .filter(|root| {
                *root >= -parameter_tolerance
                    && *root <= 1.0 + parameter_tolerance
                    && evaluate(*root).abs() <= value_tolerance
            })
            .map(|root| root.clamp(0.0, 1.0))
            .collect::<Vec<_>>();
        roots.sort_by(f64::total_cmp);
        roots.dedup_by(|left, right| (*left - *right).abs() <= parameter_tolerance);
        return roots;
    }
    let mut stations = vec![0.0, 1.0];
    stations.extend(
        quadratic_real_roots(3.0 * cubic, 2.0 * quadratic, linear)
            .into_iter()
            .filter(|root| *root > parameter_tolerance && *root < 1.0 - parameter_tolerance),
    );
    stations.sort_by(f64::total_cmp);
    stations.dedup_by(|left, right| (*left - *right).abs() <= parameter_tolerance);
    let mut roots = stations
        .iter()
        .copied()
        .filter(|station| evaluate(*station).abs() <= value_tolerance)
        .collect::<Vec<_>>();
    for interval in stations.windows(2) {
        let [mut left, mut right] = *interval else {
            continue;
        };
        let mut left_value = evaluate(left);
        let right_value = evaluate(right);
        if left_value.abs() <= value_tolerance
            || right_value.abs() <= value_tolerance
            || left_value.is_sign_positive() == right_value.is_sign_positive()
        {
            continue;
        }
        for _ in 0..64 {
            let middle = f64::midpoint(left, right);
            let middle_value = evaluate(middle);
            if middle_value == 0.0 {
                left = middle;
                right = middle;
                break;
            }
            if left_value.is_sign_positive() == middle_value.is_sign_positive() {
                left = middle;
                left_value = middle_value;
            } else {
                right = middle;
            }
        }
        roots.push(f64::midpoint(left, right));
    }
    roots.sort_by(f64::total_cmp);
    roots.dedup_by(|left, right| (*left - *right).abs() <= parameter_tolerance);
    roots
}

pub(super) fn cubic_extrusion_plane_generator_curve(
    ctx: &DecodeContext<'_>,
    nurbs: &NurbsSurface,
    plane: PlaneEquation,
) -> Result<Option<CurveGeometry>, CodecError> {
    fn recognize(
        ctx: &DecodeContext<'_>,
        nurbs: &NurbsSurface,
        plane: PlaneEquation,
    ) -> Option<Result<CurveGeometry, CodecError>> {
        let boundaries = nurbs_surface_boundaries(nurbs)?;
        (nurbs.u_degree == 3
            && nurbs.v_degree == 1
            && nurbs.u_count == 4
            && nurbs.v_count == 2
            && !nurbs.u_periodic
            && !nurbs.v_periodic)
            .then_some(())?;
        let u_knots = normalized_knot_vector(&nurbs.u_knots)?;
        let v_knots = normalized_knot_vector(&nurbs.v_knots)?;
        (u_knots.len() == 8
            && u_knots
                .iter()
                .zip([0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0])
                .all(|(actual, expected)| scalar_near(*actual, expected, 1e-12))
            && v_knots.len() == 4
            && v_knots
                .iter()
                .zip([0.0, 0.0, 1.0, 1.0])
                .all(|(actual, expected)| scalar_near(*actual, expected, 1e-12)))
        .then_some(())?;
        let weights = match &nurbs.weights {
            Some(weights) => weights.clone(),
            None => match ctx.alloc_filled(nurbs.control_points.len(), 1.0, "creo_nurbs_weights") {
                Ok(weights) => weights,
                Err(error) => return Some(Err(error)),
            },
        };
        (0..4)
            .all(|u| scalar_near(weights[2 * u], weights[2 * u + 1], 1e-12 * weights[2 * u]))
            .then_some(())?;
        let generator = [
            nurbs.control_points[1].x - nurbs.control_points[0].x,
            nurbs.control_points[1].y - nurbs.control_points[0].y,
            nurbs.control_points[1].z - nurbs.control_points[0].z,
        ];
        normalized(generator)?;
        let normal = normalized(plane.normal)?;
        let tolerance = point_tolerance(nurbs.control_points.iter())?
            .max(32.0 * f64::EPSILON * plane.origin.into_iter().map(f64::abs).fold(1.0, f64::max));
        let structural_tolerance = 64.0
            * f64::EPSILON
            * nurbs
                .control_points
                .iter()
                .flat_map(|point| [point.x, point.y, point.z])
                .chain(plane.origin)
                .map(f64::abs)
                .fold(1.0, f64::max);
        (generator.iter().copied().all(f64::is_finite)
            && dot(normal, generator).abs() <= structural_tolerance
            && (0..4).all(|u| {
                let current = [
                    nurbs.control_points[2 * u + 1].x - nurbs.control_points[2 * u].x,
                    nurbs.control_points[2 * u + 1].y - nurbs.control_points[2 * u].y,
                    nurbs.control_points[2 * u + 1].z - nurbs.control_points[2 * u].z,
                ];
                dot(
                    [
                        current[0] - generator[0],
                        current[1] - generator[1],
                        current[2] - generator[2],
                    ],
                    [
                        current[0] - generator[0],
                        current[1] - generator[1],
                        current[2] - generator[2],
                    ],
                )
                .sqrt()
                    <= structural_tolerance
            }))
        .then_some(())?;
        let signed = (0..4)
            .map(|u| {
                let point = nurbs.control_points[2 * u];
                weights[2 * u]
                    * dot(
                        normal,
                        [
                            point.x - plane.origin[0],
                            point.y - plane.origin[1],
                            point.z - plane.origin[2],
                        ],
                    )
            })
            .collect::<Vec<_>>();
        let cubic = -signed[0] + 3.0 * signed[1] - 3.0 * signed[2] + signed[3];
        let quadratic = 3.0 * signed[0] - 6.0 * signed[1] + 3.0 * signed[2];
        let linear = -3.0 * signed[0] + 3.0 * signed[1];
        let weight_scale = weights.iter().copied().fold(1.0, f64::max);
        let roots = cubic_unit_interval_roots(
            cubic,
            quadratic,
            linear,
            signed[0],
            tolerance * weight_scale,
        );
        let [parameter] = roots.as_slice() else {
            return None;
        };
        let parameter = *parameter;
        let bernstein = [
            (1.0 - parameter).powi(3),
            3.0 * parameter * (1.0 - parameter).powi(2),
            3.0 * parameter.powi(2) * (1.0 - parameter),
            parameter.powi(3),
        ];
        let evaluated = |v| {
            let weight = (0..4)
                .map(|u| bernstein[u] * weights[2 * u + v])
                .sum::<f64>();
            let coordinate = |coordinate: fn(&Point3) -> f64| {
                (0..4)
                    .map(|u| {
                        bernstein[u]
                            * weights[2 * u + v]
                            * coordinate(&nurbs.control_points[2 * u + v])
                    })
                    .sum::<f64>()
                    / weight
            };
            (
                Point3::new(
                    coordinate(|point| point.x),
                    coordinate(|point| point.y),
                    coordinate(|point| point.z),
                ),
                weight,
            )
        };
        let first = evaluated(0);
        let second = evaluated(1);
        let curve = &boundaries[0].curve;
        Some(Ok(CurveGeometry::Nurbs(NurbsCurve {
            degree: curve.degree,
            knots: curve.knots.clone(),
            control_points: vec![first.0, second.0],
            weights: nurbs.weights.as_ref().map(|_| vec![first.1, second.1]),
            periodic: curve.periodic,
        })))
    }
    recognize(ctx, nurbs, plane).transpose()
}

pub(super) fn analytic_curve_branches(
    geometry: &CurveGeometry,
    tag: &'static str,
) -> Vec<(CurveGeometry, &'static str)> {
    let mut branches = vec![(geometry.clone(), tag)];
    if let CurveGeometry::Hyperbola {
        center,
        axis,
        major_direction,
        major_radius,
        minor_radius,
    } = geometry
    {
        branches.push((
            CurveGeometry::Hyperbola {
                center: *center,
                axis: *axis,
                major_direction: Vector3::new(
                    -major_direction.x,
                    -major_direction.y,
                    -major_direction.z,
                ),
                major_radius: *major_radius,
                minor_radius: *minor_radius,
            },
            tag,
        ));
    }
    branches
}

pub(super) fn transfer_carrier_intersection_curves(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> BTreeSet<CurveId> {
    let mut transferred = BTreeSet::new();
    let carriers = placed_carriers(scan, ir);
    let solved_vertices = solved_topological_vertices(scan, ir, &carriers, &BTreeSet::new());
    let edge_vertices =
        crate::topology::edge_vertex_pairs(&scan.topology.half_edge_vertex_incidence);
    for row in crate::topology::uniquely_identified_rows(&scan.curves.topology_rows) {
        let (Some(first), Some(second)) = (
            carriers.get(&row.faces[0]).copied(),
            carriers.get(&row.faces[1]).copied(),
        ) else {
            continue;
        };
        let points = (|| {
            let vertices = edge_vertices.get(&row.id)?;
            let points = [
                *solved_vertices.get(&vertices[0])?,
                *solved_vertices.get(&vertices[1])?,
            ];
            Some(points)
        })();
        let resolved = carrier_intersection_curve(first, second)
            .and_then(|(geometry, tag)| {
                resolve_curve_candidates(analytic_curve_branches(&geometry, tag), points)
            })
            .or_else(|| {
                let candidates = multi_component_intersection_candidates(first, second);
                if points.is_some() {
                    resolve_curve_candidates(candidates, points)
                } else {
                    let held = fc14_held_coordinate(&scan.curves.fc_coordinates, row.id)?;
                    select_fc14_axis_coordinate_candidate(candidates, held)
                }
            });
        let Some((geometry, tag)) = resolved else {
            continue;
        };
        let id = CurveId(format!("creo:visibgeom:curve#{}", row.id));
        if ir.model.curves.iter().any(|curve| curve.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row.offset as u64,
            tag,
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id: id.clone(),
            geometry,
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", row.id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred.insert(id);
    }
    transferred
}

pub(super) struct TransferredNurbsBoundaryCurves {
    pub(super) ids: BTreeSet<CurveId>,
    pub(super) endpoint_witnesses: BTreeSet<CurveId>,
    pub(super) extrusion_plane_count: usize,
    pub(super) extrusion_plane_section_generator_count: usize,
    pub(super) shared_extrusion_generator_count: usize,
}

#[derive(Clone, Copy)]
pub(super) enum NurbsBoundaryKind {
    ExtrusionPlane,
    ExtrusionPlaneSectionGenerator,
    SharedExtrusionGenerator,
}

pub(super) fn transfer_nurbs_boundary_curves(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<TransferredNurbsBoundaryCurves, CodecError> {
    let mut result = TransferredNurbsBoundaryCurves {
        ids: BTreeSet::new(),
        endpoint_witnesses: BTreeSet::new(),
        extrusion_plane_count: 0,
        extrusion_plane_section_generator_count: 0,
        shared_extrusion_generator_count: 0,
    };
    for row in crate::topology::uniquely_identified_rows(&scan.curves.topology_rows) {
        let Some(first) = crate::surface::unique_surface_row(&scan.surfaces.rows, row.faces[0])
        else {
            continue;
        };
        let Some(second) = crate::surface::unique_surface_row(&scan.surfaces.rows, row.faces[1])
        else {
            continue;
        };
        let geometry = |surface_id| {
            let id = SurfaceId(format!("creo:visibgeom:surface#{surface_id}"));
            ir.model
                .surfaces
                .iter()
                .find(|surface| surface.id == id)
                .map(|surface| &surface.geometry)
        };
        let Some(first_geometry) = geometry(first.id) else {
            continue;
        };
        let Some(second_geometry) = geometry(second.id) else {
            continue;
        };
        let resolved = match (first.kind, second.kind, first_geometry, second_geometry) {
            (
                crate::surface::SurfaceKind::Extrusion,
                crate::surface::SurfaceKind::Plane,
                SurfaceGeometry::Nurbs(nurbs),
                SurfaceGeometry::Plane { origin, normal, .. },
            )
            | (
                crate::surface::SurfaceKind::Plane,
                crate::surface::SurfaceKind::Extrusion,
                SurfaceGeometry::Plane { origin, normal, .. },
                SurfaceGeometry::Nurbs(nurbs),
            ) => {
                let plane = PlaneEquation {
                    origin: [origin.x, origin.y, origin.z],
                    normal: [normal.x, normal.y, normal.z],
                };
                if let Some(geometry) = nurbs_plane_boundary_curve(nurbs, plane) {
                    Some((geometry, NurbsBoundaryKind::ExtrusionPlane))
                } else {
                    cubic_extrusion_plane_generator_curve(ctx, nurbs, plane)?.map(|geometry| {
                        (geometry, NurbsBoundaryKind::ExtrusionPlaneSectionGenerator)
                    })
                }
            }
            (
                crate::surface::SurfaceKind::Extrusion,
                crate::surface::SurfaceKind::Extrusion,
                SurfaceGeometry::Nurbs(first),
                SurfaceGeometry::Nurbs(second),
            ) => shared_extrusion_generator_curve(first, second)
                .map(|geometry| (geometry, NurbsBoundaryKind::SharedExtrusionGenerator)),
            _ => None,
        };
        let Some((geometry, kind)) = resolved else {
            continue;
        };
        let id = CurveId(format!("creo:visibgeom:curve#{}", row.id));
        if ir.model.curves.iter().any(|curve| curve.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row.offset as u64,
            match kind {
                NurbsBoundaryKind::ExtrusionPlane => "extrusion_plane_nurbs_boundary",
                NurbsBoundaryKind::ExtrusionPlaneSectionGenerator => {
                    "extrusion_plane_nurbs_section_generator"
                }
                NurbsBoundaryKind::SharedExtrusionGenerator => "shared_extrusion_nurbs_generator",
            },
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id: id.clone(),
            geometry,
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", row.id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        result.ids.insert(id.clone());
        result.endpoint_witnesses.insert(id);
        match kind {
            NurbsBoundaryKind::ExtrusionPlane => result.extrusion_plane_count += 1,
            NurbsBoundaryKind::ExtrusionPlaneSectionGenerator => {
                result.extrusion_plane_section_generator_count += 1;
            }
            NurbsBoundaryKind::SharedExtrusionGenerator => {
                result.shared_extrusion_generator_count += 1;
            }
        }
    }
    Ok(result)
}

pub(super) fn rowless_round_cylinder_pairs(
    round_feature_ids: &BTreeSet<u32>,
    tables: &[crate::feature::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
) -> Vec<(u32, u32, usize)> {
    tables
        .iter()
        .filter_map(|table| {
            let feature_id = table.feature_id?;
            round_feature_ids.contains(&feature_id).then_some(())?;
            let [first, second, rowless, cylinder] = table.entry_ids.as_slice() else {
                return None;
            };
            rows.iter().any(|row| row.id == *first).then_some(())?;
            rows.iter().any(|row| row.id == *second).then_some(())?;
            (!rows.iter().any(|row| row.id == *rowless)).then_some(())?;
            rows.iter()
                .any(|row| {
                    row.id == *cylinder
                        && row.feature_id == feature_id
                        && row.kind == crate::surface::SurfaceKind::Cylinder
                })
                .then_some(())?;
            Some((*rowless, *cylinder, table.offset))
        })
        .collect()
}

pub(super) fn transfer_constrained_slot_fillet_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let round_feature_ids = scan
        .features
        .rows
        .iter()
        .filter(|row| row.root_schema_class == Some(913))
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for feature_id in round_feature_ids {
        let named = agreed_feature_affected_ids(
            &scan.features.affected_ids,
            feature_id,
            crate::feature::AffectedIdKind::Geometry,
        );
        let named_present = has_feature_affected_ids(
            &scan.features.affected_ids,
            feature_id,
            crate::feature::AffectedIdKind::Geometry,
        );
        let replay =
            agreed_feature_replay_geometry_ids(&scan.features.replay_affected_ids, feature_id);
        let affected = match (named, replay) {
            (Some(ids), _) => ids,
            (None, Some(ids)) if !named_present => ids,
            _ => continue,
        };
        let Some((cap_ids, support_ids)) = affected.split_at_checked(2) else {
            continue;
        };
        if support_ids.len() < 4 {
            continue;
        }
        let planes = affected
            .iter()
            .filter_map(|id| {
                let surface_id = SurfaceId(format!("creo:visibgeom:surface#{id}"));
                let surface = ir
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == surface_id)?;
                match surface.geometry {
                    SurfaceGeometry::Plane { origin, normal, .. } => Some(PlaneEquation {
                        origin: [origin.x, origin.y, origin.z],
                        normal: [normal.x, normal.y, normal.z],
                    }),
                    _ => None,
                }
            })
            .collect::<Vec<_>>();
        if planes.len() != affected.len() {
            continue;
        }
        let cap_planes: [PlaneEquation; 2] = planes[..cap_ids.len()].try_into().expect("two caps");
        let Some(cylinder) = slot_fillet_cylinder(cap_planes, &planes[cap_ids.len()..]) else {
            continue;
        };
        let unresolved_rows = scan
            .surfaces
            .rows
            .iter()
            .filter(|row| {
                row.feature_id == feature_id
                    && row.kind == crate::surface::SurfaceKind::Cylinder
                    && !ir.model.surfaces.iter().any(|surface| {
                        surface.id == SurfaceId(format!("creo:visibgeom:surface#{}", row.id))
                    })
            })
            .collect::<Vec<_>>();
        let [row] = unresolved_rows.as_slice() else {
            continue;
        };
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
        annotate(
            annotations,
            &id,
            "AllFeatur",
            row.offset as u64,
            "constrained_slot_fillet_cylinder",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(cylinder.origin[0], cylinder.origin[1], cylinder.origin[2]),
                axis: Vector3::new(cylinder.axis[0], cylinder.axis[1], cylinder.axis[2]),
                ref_direction: Vector3::new(
                    cylinder.ref_direction[0],
                    cylinder.ref_direction[1],
                    cylinder.ref_direction[2],
                ),
                radius: cylinder.radius,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("AllFeatur:{}:{}", feature_id, row.id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

pub(super) fn transfer_rowless_round_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let round_feature_ids = scan
        .features
        .rows
        .iter()
        .filter(|row| row.root_schema_class == Some(913))
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for (rowless_id, sibling_id, offset) in rowless_round_cylinder_pairs(
        &round_feature_ids,
        &scan.features.entity_tables,
        &scan.surfaces.rows,
    ) {
        let sibling = SurfaceId(format!("creo:visibgeom:surface#{sibling_id}"));
        let Some(SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        }) = ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == sibling)
            .map(|surface| &surface.geometry)
        else {
            continue;
        };
        let id = SurfaceId(format!("creo:visibgeom:surface#{rowless_id}"));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "AllFeatur",
            offset as u64,
            "round_rowless_sibling_cylinder",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Cylinder {
                origin: *origin,
                axis: *axis,
                ref_direction: *ref_direction,
                radius: *radius,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("AllFeatur:{rowless_id}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

pub(super) fn transfer_hole_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let hole_feature_ids = scan
        .features
        .rows
        .iter()
        .filter(|row| row.root_schema_class == Some(911))
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for feature_id in hole_feature_ids {
        let cylinders = if let Some(hole) = simple_hole_geometry(scan, feature_id) {
            hole.cylinder_ids
                .into_iter()
                .map(|id| (id, hole.geometry.clone()))
                .collect::<Vec<_>>()
        } else {
            counterbore_patch_geometries(scan, ir, feature_id).unwrap_or_default()
        };
        for (cylinder_id, geometry) in cylinders {
            let row = crate::surface::unique_surface_row(&scan.surfaces.rows, cylinder_id)
                .expect("validated cylinder row");
            let id = SurfaceId(format!("creo:visibgeom:surface#{cylinder_id}"));
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "AllFeatur",
                row.offset as u64,
                "hole_cap_outline_cylinder",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{cylinder_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }
    }
    transferred
}

pub(super) fn transfer_split_outline_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let rows = scan
        .surfaces
        .rows
        .iter()
        .map(|row| (row.id, row))
        .collect::<BTreeMap<_, _>>();
    let mut cylinders_by_plane = BTreeMap::<(u32, u32), BTreeSet<u32>>::new();
    for edge in crate::topology::uniquely_identified_rows(&scan.curves.topology_rows) {
        if edge.type_byte != 0 {
            continue;
        }
        let [left, right] = edge.faces;
        let pair = match (rows.get(&left), rows.get(&right)) {
            (Some(plane), Some(cylinder))
                if plane.kind == crate::surface::SurfaceKind::Plane
                    && cylinder.kind == crate::surface::SurfaceKind::Cylinder =>
            {
                Some(((left, cylinder.feature_id), right))
            }
            (Some(cylinder), Some(plane))
                if plane.kind == crate::surface::SurfaceKind::Plane
                    && cylinder.kind == crate::surface::SurfaceKind::Cylinder =>
            {
                Some(((right, cylinder.feature_id), left))
            }
            _ => None,
        };
        if let Some((plane_and_feature, cylinder)) = pair {
            cylinders_by_plane
                .entry(plane_and_feature)
                .or_default()
                .insert(cylinder);
        }
    }

    let mut transferred = 0;
    for ((plane_id, _), cylinder_ids) in cylinders_by_plane {
        let cylinder_ids = cylinder_ids.into_iter().collect::<Vec<_>>();
        let [first_id, second_id] = cylinder_ids.as_slice() else {
            continue;
        };
        let Some(first) =
            crate::surface::unique_surface_parameter(&scan.surfaces.parameters, *first_id)
        else {
            continue;
        };
        let Some(second) =
            crate::surface::unique_surface_parameter(&scan.surfaces.parameters, *second_id)
        else {
            continue;
        };
        let Some(bounds) = first
            .split_cylinder_outline_bounds
            .zip(second.split_cylinder_outline_bounds)
            .map(|(first, second)| [first, second])
        else {
            continue;
        };
        let plane_id = SurfaceId(format!("creo:visibgeom:surface#{plane_id}"));
        let Some(plane) = ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == plane_id)
        else {
            continue;
        };
        let Some(geometry) = cylinder_from_complementary_outline_bounds(&plane.geometry, bounds)
        else {
            continue;
        };
        for cylinder_id in [*first_id, *second_id] {
            let id = SurfaceId(format!("creo:visibgeom:surface#{cylinder_id}"));
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            let row = rows[&cylinder_id];
            annotate(
                annotations,
                &id,
                "VisibGeom",
                row.offset as u64,
                "split_outline_cylinder",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry: geometry.clone(),
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{cylinder_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }
    }
    transferred
}

pub(super) fn transfer_positional_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for record in &scan.surfaces.parameters {
        if crate::surface::unique_surface_parameter(&scan.surfaces.parameters, record.surface_id)
            != Some(record)
        {
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id)
            .filter(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
        else {
            continue;
        };
        let reference_bound_frame = || {
            let entity_ids = scan
                .features
                .entity_tables
                .iter()
                .filter(|table| table.feature_id == Some(row.feature_id))
                .flat_map(|table| table.entry_ids.iter().copied())
                .collect::<BTreeSet<_>>();
            let circles = scan
                .references
                .circles
                .iter()
                .filter(|circle| entity_ids.contains(&circle.entity_id))
                .collect::<Vec<_>>();
            let generated_cylinder_count = scan
                .surfaces
                .rows
                .iter()
                .filter(|candidate| {
                    candidate.feature_id == row.feature_id
                        && candidate.kind == crate::surface::SurfaceKind::Cylinder
                })
                .count();
            if generated_cylinder_count == 1 {
                if let Some(frame) = reference_circle_pair_cylinder_frame(&circles) {
                    return Some((frame, "reference_circle_pair_cylinder_frame"));
                }
            }
            let envelope = record.type24_scalar_frame_round_envelope(row.type_byte)?;
            reference_cap_bound_round_frame(envelope, &circles)
                .map(|frame| (frame, "round_reference_cap_cylinder_frame"))
        };
        let (frame, mechanism) = match record.positional_cylinder_frame {
            Some(frame) => (frame, "positional_cylinder_frame"),
            None => {
                let Some(frame) = reference_bound_frame() else {
                    continue;
                };
                frame
            }
        };
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", record.surface_id));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row.offset as u64,
            mechanism,
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(frame.origin[0], frame.origin[1], frame.origin[2]),
                axis: Vector3::new(frame.axis[0], frame.axis[1], frame.axis[2]),
                ref_direction: Vector3::new(
                    frame.ref_direction[0],
                    frame.ref_direction[1],
                    frame.ref_direction[2],
                ),
                radius: frame.radius,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", record.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

pub(super) fn reference_circle_pair_cylinder_frame(
    circles: &[&crate::reference::ReferenceCircle],
) -> Option<crate::surface::PositionalCylinderFrame> {
    let [first, second] = circles else {
        return None;
    };
    (first.radius.is_finite()
        && first.radius > 0.0
        && first.center_stored
        && second.center_stored
        && second.radius.is_finite())
    .then_some(())?;
    let radius = first.radius;
    let radius_scale = radius.max(second.radius).max(1.0);
    ((second.radius - radius).abs() <= 1e-9 * radius_scale).then_some(())?;
    let scale = first
        .center
        .iter()
        .chain(&second.center)
        .map(|value| value.abs())
        .fold(radius_scale, f64::max);
    let first_axis = normalized(first.axis)?;
    let second_axis = normalized(second.axis)?;
    ((dot(first_axis, second_axis).abs() - 1.0).abs() <= 1e-9).then_some(())?;
    let displacement: [f64; 3] =
        std::array::from_fn(|index| second.center[index] - first.center[index]);
    let length = dot(displacement, displacement).sqrt();
    (length.is_finite() && length > 1e-9 * scale).then_some(())?;
    let center_direction = displacement.map(|value| value / length);
    ((dot(center_direction, first_axis).abs() - 1.0).abs() <= 1e-9
        && (dot(center_direction, second_axis).abs() - 1.0).abs() <= 1e-9)
        .then_some(())?;
    let validated_radial = |circle: &crate::reference::ReferenceCircle, axis| {
        let vector: [f64; 3] =
            std::array::from_fn(|index| circle.start[index] - circle.center[index]);
        let length = dot(vector, vector).sqrt();
        ((length - radius).abs() <= 1e-9 * radius_scale
            && dot(axis, vector).abs() <= 1e-9 * radius_scale)
            .then_some((vector, length))
    };
    let (radial, radial_length) = validated_radial(first, first_axis)?;
    validated_radial(second, second_axis)?;
    Some(crate::surface::PositionalCylinderFrame {
        origin: first.center,
        axis: first_axis,
        ref_direction: radial.map(|value| value / radial_length),
        radius,
        length: Some(length),
    })
}

pub(super) fn reference_cap_bound_round_frame(
    envelope: crate::surface::Type24RoundEnvelope,
    circles: &[&crate::reference::ReferenceCircle],
) -> Option<crate::surface::PositionalCylinderFrame> {
    let [first, second] = envelope.extent_endpoints;
    let scale = first
        .iter()
        .chain(&second)
        .copied()
        .map(f64::abs)
        .fold(envelope.diameter.max(1.0), f64::max);
    let tolerance = 1.0e-9 * scale;
    let point_matches = |actual: [f64; 3], expected: [f64; 3]| {
        actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| (actual - expected).abs() <= tolerance)
    };
    let mut candidates = Vec::new();
    for axis_index in 0..3 {
        let radial_indices = (0..3)
            .filter(|index| *index != axis_index)
            .collect::<Vec<_>>();
        if radial_indices.iter().any(|index| {
            ((second[*index] - first[*index]).abs() - envelope.diameter).abs() > tolerance
        }) || (second[axis_index] - first[axis_index]).abs() <= tolerance
        {
            continue;
        }
        let cap_pair = |coordinate: f64, crossed: bool| {
            let mut first_corner = first;
            let mut second_corner = second;
            first_corner[axis_index] = coordinate;
            second_corner[axis_index] = coordinate;
            if crossed {
                first_corner[radial_indices[1]] = second[radial_indices[1]];
                second_corner[radial_indices[1]] = first[radial_indices[1]];
            }
            circles.iter().any(|circle| {
                circle.axis.iter().enumerate().all(|(index, component)| {
                    if index == axis_index {
                        (component.abs() - 1.0).abs() <= 1.0e-9
                    } else {
                        component.abs() <= 1.0e-9
                    }
                }) && ((point_matches(circle.start, first_corner)
                    && point_matches(circle.end, second_corner))
                    || (point_matches(circle.end, first_corner)
                        && point_matches(circle.start, second_corner)))
            })
        };
        if ![false, true].into_iter().any(|crossed| {
            cap_pair(first[axis_index], crossed) && cap_pair(second[axis_index], crossed)
        }) {
            continue;
        }
        let mut origin = first;
        for index in &radial_indices {
            origin[*index] = first[*index].midpoint(second[*index]);
        }
        let mut axis = [0.0; 3];
        axis[axis_index] = (second[axis_index] - first[axis_index]).signum();
        let mut ref_direction = [0.0; 3];
        let reference_index = radial_indices[0];
        ref_direction[reference_index] =
            (second[reference_index] - first[reference_index]).signum();
        candidates.push(crate::surface::PositionalCylinderFrame {
            origin,
            axis,
            ref_direction,
            radius: envelope.diameter / 2.0,
            length: Some((second[axis_index] - first[axis_index]).abs()),
        });
    }
    let [frame] = candidates.as_slice() else {
        return None;
    };
    Some(*frame)
}

pub(super) fn transfer_positional_cones(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for record in &scan.surfaces.parameters {
        let Some(frame) = record.positional_cone_frame else {
            continue;
        };
        if crate::surface::unique_surface_parameter(&scan.surfaces.parameters, record.surface_id)
            != Some(record)
        {
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id)
            .filter(|row| row.kind == crate::surface::SurfaceKind::Cone)
        else {
            continue;
        };
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", record.surface_id));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row.offset as u64,
            "positional_cone_frame",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Cone {
                origin: Point3::new(frame.apex[0], frame.apex[1], frame.apex[2]),
                axis: Vector3::new(frame.axis[0], frame.axis[1], frame.axis[2]),
                ref_direction: Vector3::new(
                    frame.ref_direction[0],
                    frame.ref_direction[1],
                    frame.ref_direction[2],
                ),
                radius: 0.0,
                ratio: 1.0,
                half_angle: frame.half_angle,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", record.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

pub(super) fn transfer_circular_sweep_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let sweep_feature_ids = scan
        .features
        .rows
        .iter()
        .filter(|row| {
            row.root_schema_class == Some(917)
                && !feature_section_sweep_semantics_conflict(scan, row.feature_id)
                && section_sweep_allows_linear_extrusion(917, feature_recipe(scan, row.feature_id))
        })
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for feature_id in sweep_feature_ids {
        let Some(sweep) = circular_sweep_geometry(scan, feature_id) else {
            continue;
        };
        for cylinder_id in &sweep.cylinder_ids {
            let row = crate::surface::unique_surface_row(&scan.surfaces.rows, *cylinder_id)
                .expect("validated cylinder row");
            let id = SurfaceId(format!("creo:visibgeom:surface#{cylinder_id}"));
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "AllFeatur",
                row.offset as u64,
                "circular_sweep_cap_outline_cylinder",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry: sweep.geometry.clone(),
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{cylinder_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }
    }
    transferred
}

pub(super) fn transfer_cross_section_planes(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for frame in &scan.planes.cross_section_local_systems {
        let (Some(origin), Some(normal), Some(u_axis)) = (frame.origin, frame.normal, frame.u_axis)
        else {
            continue;
        };
        if is_axis_aligned(normal) {
            continue;
        }
        let id = SurfaceId(format!(
            "creo:cross_section_geometry:surface#{}",
            frame.surface_id
        ));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "Xsections",
            frame.offset as u64,
            "cross_section_plane_local_system",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(origin[0], origin[1], origin[2]),
                normal: Vector3::new(normal[0], normal[1], normal[2]),
                u_axis: Vector3::new(u_axis[0], u_axis[1], u_axis[2]),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("Xsections:{}", frame.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    for plane in &scan.planes.cross_section_outlines {
        let id = SurfaceId(format!(
            "creo:cross_section_geometry:surface#{}",
            plane.surface_id
        ));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "Xsections",
            plane.offset as u64,
            "cross_section_plane_outline_held_coordinate",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(plane.origin[0], plane.origin[1], plane.origin[2]),
                normal: Vector3::new(plane.normal[0], plane.normal[1], plane.normal[2]),
                u_axis: Vector3::new(plane.u_axis[0], plane.u_axis[1], plane.u_axis[2]),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("Xsections:{}", plane.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}
