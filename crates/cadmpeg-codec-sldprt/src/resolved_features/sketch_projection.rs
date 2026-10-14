//! Sketch projection from B-rep geometry.

use super::assembly::contains_ascii_case_insensitive;
use super::names::configuration;
use super::sketch_edges::{
    project_edge, project_endpoint_constraints, project_point, EndpointConstraintSource,
};
use crate::container::ContainerScan;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId,
};
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::Exactness;
use std::collections::{HashMap, HashSet};

/// Sketches and their projected entities and constraints.
pub(crate) struct ProjectedSketches {
    pub(crate) sketches: Vec<Sketch>,
    pub(crate) entities: Vec<SketchEntity>,
    pub(crate) constraints: Vec<SketchConstraint>,
}

/// Decode nested feature-input Parasolid streams as placed planar sketches.
pub(crate) fn sketches(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    annotations: &mut Annotations,
) -> Result<ProjectedSketches, cadmpeg_core::CodecError> {
    let mut sketches = Vec::new();
    let mut entities = Vec::new();
    let mut constraints = Vec::new();
    for source in scan.sections() {
        let Some(section) = source.name() else {
            continue;
        };
        if !contains_ascii_case_insensitive(section, "resolvedfeatures") {
            continue;
        }
        let source_stream = source.source_stream();
        let native_ref = format!(
            "sldprt:feature-input:resolved-features#{}",
            source.ordinal()
        );
        for (stream_ordinal, stream) in source.ps_streams().iter().enumerate() {
            let brep =
                crate::brep::graph::decode(ctx, &stream.payload, &stream.header, source_stream)?;
            let configuration = configuration(section);
            project_brep(
                ctx,
                &brep,
                &BrepSketchSource {
                    block_offset: source.ordinal(),
                    stream_ordinal,
                    stream_offset: stream.offset,
                    source_stream,
                    sketch_name: &stream.header.description,
                    configuration: configuration.as_deref(),
                    native_ref: &native_ref,
                },
                annotations,
                &mut sketches,
                &mut entities,
                &mut constraints,
            )?;
        }
    }
    Ok(ProjectedSketches {
        sketches,
        entities,
        constraints,
    })
}

/// Where one decoded B-rep stream sits in its section and how its sketch is named.
struct BrepSketchSource<'a> {
    block_offset: usize,
    stream_ordinal: usize,
    stream_offset: usize,
    source_stream: &'a cadmpeg_ir::StreamName,
    sketch_name: &'a str,
    configuration: Option<&'a str>,
    native_ref: &'a str,
}

fn project_brep(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    brep: &crate::brep::graph::Brep,
    source: &BrepSketchSource<'_>,
    annotations: &mut Annotations,
    sketches: &mut Vec<Sketch>,
    entities: &mut Vec<SketchEntity>,
    constraints: &mut Vec<SketchConstraint>,
) -> Result<(), cadmpeg_core::CodecError> {
    let BrepSketchSource {
        block_offset,
        stream_ordinal,
        stream_offset,
        source_stream,
        sketch_name,
        configuration,
        native_ref,
    } = *source;
    let surfaces = brep
        .surfaces
        .iter()
        .map(|surface| (&surface.id, &surface.geometry))
        .collect::<HashMap<_, _>>();
    let loops = brep
        .loops
        .iter()
        .map(|loop_| (&loop_.id, loop_))
        .collect::<HashMap<_, _>>();
    let coedges = brep
        .coedges
        .iter()
        .map(|coedge| (&coedge.id, coedge))
        .collect::<HashMap<_, _>>();
    let edges = brep
        .edges
        .iter()
        .map(|edge| (&edge.id, edge))
        .collect::<HashMap<_, _>>();
    let vertices = brep
        .vertices
        .iter()
        .map(|vertex| (&vertex.id, &vertex.point))
        .collect::<HashMap<_, _>>();
    let points = brep
        .points
        .iter()
        .map(|point| (&point.id, point.position().get()))
        .collect::<HashMap<_, _>>();
    let curves = brep
        .curves
        .iter()
        .map(|curve| (&curve.id, &curve.geometry))
        .collect::<HashMap<_, _>>();

    for (face_ordinal, face) in brep.faces.iter().enumerate() {
        let Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface))) =
            surfaces.get(&face.surface).copied()
        else {
            continue;
        };
        let origin = plane_surface.origin().get();
        let normal = plane_surface.frame().axis().as_raw();
        let u_axis = plane_surface.frame().reference().as_raw();
        let Ok(sketch_id) = SketchId::mint(format!(
            "sldprt:model:sketch#{block_offset}:{stream_ordinal}:{face_ordinal}"
        )) else {
            continue;
        };
        let Ok(placement) =
            cadmpeg_ir::sketches::SketchPlacement::try_resolved(origin, *normal, *u_axis)
        else {
            continue;
        };
        let v_axis = normal.cross(*u_axis);
        let first_entity = entities.len();
        let mut edge_entities = HashMap::<&cadmpeg_ir::ids::EdgeId, SketchEntityId>::new();
        let mut used_vertices = HashSet::<&cadmpeg_ir::ids::VertexId>::new();
        let mut profiles = Vec::new();
        for loop_id in &face.loops {
            let Some(loop_) = loops.get(loop_id) else {
                continue;
            };
            let mut profile = Vec::new();
            for coedge_id in loop_.coedges() {
                let Some(coedge) = coedges.get(coedge_id) else {
                    continue;
                };
                let Some(edge) = edges.get(&coedge.edge) else {
                    continue;
                };
                for vertex_id in [&edge.start, &edge.end] {
                    used_vertices.insert(vertex_id);
                }
                let entity_id = if let Some(id) = edge_entities.get(&edge.id) {
                    let id = id.as_str().to_owned();
                    SketchEntityId::mint(id).map_err(cadmpeg_core::CodecError::malformed)?
                } else {
                    let Ok(id) = SketchEntityId::mint(format!(
                        "sldprt:model:sketch-entity#{block_offset}:{stream_ordinal}:{face_ordinal}:{}",
                        edge_entities.len()
                    )) else { continue };
                    let mut edge_refusal = crate::lane_refusal::LaneRefusals::new();
                    let projected = project_edge(
                        ctx,
                        edge,
                        &vertices,
                        &points,
                        &curves,
                        super::sketch_edges::SketchPlaneFrame {
                            origin,
                            u_axis: *u_axis,
                            v_axis,
                        },
                        &mut edge_refusal,
                    )?;
                    let edge_refusals = edge_refusal.take_records();
                    if !edge_refusals.is_empty() {
                        return Err(cadmpeg_core::CodecError::Malformed(
                            edge_refusals.join("; "),
                        ));
                    }
                    let Some(geometry) = projected else {
                        continue;
                    };
                    let Some(start_point) = vertices.get(&edge.start) else {
                        continue;
                    };
                    let Some(end_point) = vertices.get(&edge.end) else {
                        continue;
                    };
                    crate::annotations::note(
                        ctx,
                        annotations,
                        id.as_str(),
                        source_stream,
                        0,
                        "feature_input_profile_edge",
                        Exactness::Derived,
                    )?;
                    let native_edge_ref = format!("{stream_ordinal}:{}", edge.id.as_str());
                    let geometry_ref = edge
                        .curve()
                        .map(|curve_id| format!("{stream_ordinal}:{}", curve_id.as_str()));
                    let endpoint_refs = [start_point, end_point]
                        .map(|point| format!("{stream_ordinal}:{}", point.as_str()))
                        .into();
                    let entity_id = id.clone();
                    let entity_sketch_id = sketch_id.clone();
                    entities.push(
                        SketchEntity::new(entity_id, entity_sketch_id, geometry)
                            .with_native_ref(Some(native_edge_ref))
                            .with_geometry_ref(geometry_ref)
                            .with_endpoint_refs(endpoint_refs),
                    );
                    let index_id = id.clone();
                    edge_entities.insert(&edge.id, index_id);
                    id
                };
                if edge.curve().is_some() || edge.start != edge.end {
                    profile.push(SketchEntityUse {
                        entity: entity_id,
                        reversed: coedge.sense == Sense::Reversed,
                    });
                }
            }
            if !profile.is_empty() {
                orient_closed_profile_by_topology(&mut profile, &entities[first_entity..]);
                profiles.push(profile);
            }
        }
        for vertex in &brep.vertices {
            if used_vertices.contains(&vertex.id) {
                continue;
            }
            let Some(position) = points.get(&vertex.point) else {
                continue;
            };
            let Ok(id) = SketchEntityId::mint(format!(
                "sldprt:model:sketch-entity#{block_offset}:{stream_ordinal}:{face_ordinal}:{}",
                edge_entities.len() + (entities.len() - first_entity)
            )) else {
                continue;
            };
            let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: project_point(*position, origin, *u_axis, v_axis),
            }) else {
                continue;
            };
            crate::annotations::note(
                ctx,
                annotations,
                id.as_str(),
                source_stream,
                0,
                "feature_input_profile_point",
                Exactness::Derived,
            )?;
            let entity_sketch_id = sketch_id.clone();
            let native_vertex_ref = format!("{stream_ordinal}:{}", vertex.id.as_str());
            let mut endpoint_refs = Vec::new();
            endpoint_refs.push(format!("{stream_ordinal}:{}", vertex.point.as_str()));
            entities.push(
                SketchEntity::new(id, entity_sketch_id, geometry)
                    .with_native_ref(Some(native_vertex_ref))
                    .with_endpoint_refs(endpoint_refs),
            );
        }
        let Ok(profiles) = cadmpeg_ir::sketches::SketchProfiles::try_from(profiles) else {
            continue;
        };
        if profiles.is_empty() && entities.len() == first_entity {
            continue;
        }
        crate::annotations::note(
            ctx,
            annotations,
            sketch_id.as_str(),
            source_stream,
            cadmpeg_core::decode::u64_from_index(stream_offset),
            "feature_input_profile",
            Exactness::Derived,
        )?;
        project_endpoint_constraints(
            ctx,
            EndpointConstraintSource {
                sketch: &sketch_id,
                entities: &entities[first_entity..],
                block_offset,
                stream_ordinal,
                face_ordinal,
                stream: source_stream,
            },
            annotations,
            constraints,
        )?;
        let name = (!sketch_name.is_empty()).then(|| sketch_name.to_owned());
        let configuration = configuration.map(str::to_owned);
        let native_ref = native_ref.to_owned();
        sketches.push(Sketch {
            id: sketch_id,
            name,
            configuration,
            visible: None,
            placement,
            profiles,
            native_ref: Some(native_ref),
        });
    }
    Ok(())
}

fn orient_closed_profile_by_topology(profile: &mut [SketchEntityUse], entities: &[SketchEntity]) {
    if profile.len() < 2 {
        return;
    }
    let entities = entities
        .iter()
        .map(|entity| (entity.id(), entity))
        .collect::<HashMap<_, _>>();
    let mut orientations = Vec::new();
    for (index, use_) in profile.iter().enumerate() {
        let Some(current) = entities.get(&use_.entity) else {
            return;
        };
        let Some(next) = entities.get(&profile[(index + 1) % profile.len()].entity) else {
            return;
        };
        let [start, end] = current.endpoint_refs.as_slice() else {
            return;
        };
        let mut shared = current
            .endpoint_refs
            .iter()
            .filter(|endpoint| next.endpoint_refs.contains(endpoint));
        let Some(first) = shared.next() else {
            return;
        };
        if shared.next().is_some() {
            return;
        }
        orientations.push(if first == end {
            false
        } else if first == start {
            true
        } else {
            return;
        });
    }
    for (use_, reversed) in profile.iter_mut().zip(orientations) {
        use_.reversed = reversed;
    }
}

#[cfg(test)]
mod projected_profile_orientation_tests {
    use super::orient_closed_profile_by_topology;
    use cadmpeg_ir::{
        math::Point2,
        sketches::{
            SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
            SketchGeometryDefinition, SketchId,
        },
    };

    fn line(id: &str, start_ref: &str, end_ref: &str) -> SketchEntity {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("synthetic:test:id#sketch").unwrap(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            })
            .unwrap(),
        )
        .with_endpoint_refs(vec![start_ref.into(), end_ref.into()])
    }

    #[test]
    fn orients_each_closed_profile_edge_toward_its_topological_successor() {
        let entities = [
            line("synthetic:test:id#a", "p0", "p1"),
            line("synthetic:test:id#b", "p1", "p2"),
            line("synthetic:test:id#c", "p0", "p2"),
        ];
        let mut profile = entities
            .iter()
            .map(|entity| SketchEntityUse {
                entity: entity.id().clone(),
                reversed: true,
            })
            .collect::<Vec<_>>();

        orient_closed_profile_by_topology(&mut profile, &entities);

        assert_eq!(
            profile.iter().map(|use_| use_.reversed).collect::<Vec<_>>(),
            [false, false, true]
        );
    }

    #[test]
    fn preserves_all_orientations_when_endpoint_incidence_is_ambiguous() {
        let entities = [
            line("synthetic:test:id#a", "p0", "p1"),
            line("synthetic:test:id#b", "p1", "p2"),
            line("synthetic:test:id#c", "p3", "p4"),
        ];
        let mut profile = entities
            .iter()
            .map(|entity| SketchEntityUse {
                entity: entity.id().clone(),
                reversed: true,
            })
            .collect::<Vec<_>>();

        orient_closed_profile_by_topology(&mut profile, &entities);

        assert!(profile.iter().all(|use_| use_.reversed));
    }
}

#[cfg(test)]
mod projected_brep_output_tests {
    use super::{project_brep, BrepSketchSource};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::geometry::{
        analytic::PlaneSurface, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{FaceId, PointId, ShellId, SurfaceId, VertexId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::topology::{Face, FaceLoops, Point, Sense, Vertex};

    fn point_brep() -> crate::brep::graph::Brep {
        let surface_id = SurfaceId::mint("test:model:surface#plane").expect("surface ID");
        let point_id = PointId::mint("test:model:point#free").expect("point ID");
        crate::brep::graph::Brep {
            surfaces: vec![Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("plane frame"),
                )),
                source_object: None,
            }],
            faces: vec![Face {
                id: FaceId::mint("test:model:face#plane").expect("face ID"),
                shell: ShellId::mint("test:model:shell#one").expect("shell ID"),
                surface: surface_id,
                sense: Sense::Forward,
                loops: FaceLoops::unspecified(Vec::new()),
                name: None,
                color: None,
                tolerance: None,
            }],
            vertices: vec![Vertex {
                id: VertexId::mint("test:model:vertex#free").expect("vertex ID"),
                point: point_id.clone(),
                tolerance: None,
            }],
            points: vec![Point::new(
                point_id,
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0))
                    .expect("finite position"),
                None,
            )],
            ..Default::default()
        }
    }

    #[test]
    fn projected_point_ids_use_only_entities_emitted_for_the_current_face() {
        use cadmpeg_ir::sketches::{
            SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
        };

        let mut brep = point_brep();
        for ordinal in 1..32 {
            brep.vertices.push(Vertex {
                id: VertexId::mint(format!("test:model:vertex#{ordinal}")).unwrap(),
                point: brep.points[0].id.clone(),
                tolerance: None,
            });
        }
        // A missing point must not consume a projected entity ordinal.
        brep.vertices.insert(
            1,
            Vertex {
                id: VertexId::mint("test:model:vertex#missing").unwrap(),
                point: PointId::mint("test:model:point#missing").unwrap(),
                tolerance: None,
            },
        );
        let mut second_face = brep.faces[0].clone();
        second_face.id = FaceId::mint("test:model:face#second").unwrap();
        brep.faces.push(second_face);
        let prior = SketchEntity::new(
            SketchEntityId::mint("test:model:sketch-entity#prior").unwrap(),
            SketchId::mint("test:model:sketch#prior").unwrap(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            })
            .unwrap(),
        );
        let mut entities = vec![prior];
        let mut sketches = Vec::new();
        let mut constraints = Vec::new();
        let mut annotations = cadmpeg_ir::annotations::Annotations::default();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let stream = cadmpeg_ir::stream_name!("test:sketch-projection");
        project_brep(
            &ctx,
            &brep,
            &BrepSketchSource {
                block_offset: 7,
                stream_ordinal: 2,
                stream_offset: 0,
                source_stream: &stream,
                sketch_name: "point sketches",
                configuration: None,
                native_ref: "native:points",
            },
            &mut annotations,
            &mut sketches,
            &mut entities,
            &mut constraints,
        )
        .unwrap();

        assert_eq!(sketches.len(), 2);
        assert_eq!(entities.len(), 65);
        for (face, sketch) in sketches.iter().enumerate() {
            for ordinal in 0..32 {
                let entity = &entities[1 + face * 32 + ordinal];
                assert_eq!(entity.sketch, sketch.id);
                assert_eq!(
                    entity.id().as_str(),
                    format!("sldprt:model:sketch-entity#7:2:{face}:{ordinal}")
                );
            }
        }
    }

    #[test]
    fn sketch_projection_output_projects_point_sketch() {
        let brep = point_brep();
        let arena = DecodeArena::new();
        let stream = cadmpeg_ir::stream_name!("test:sketch-projection");
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("empty root fits service policy");
        let mut annotations = cadmpeg_ir::annotations::Annotations::default();
        let mut sketches = Vec::new();
        let mut entities = Vec::new();
        let mut constraints = Vec::new();
        project_brep(
            &ctx,
            &brep,
            &BrepSketchSource {
                block_offset: 0,
                stream_ordinal: 0,
                stream_offset: 0,
                source_stream: &stream,
                sketch_name: "point sketch",
                configuration: None,
                native_ref: "native:point",
            },
            &mut annotations,
            &mut sketches,
            &mut entities,
            &mut constraints,
        )
        .expect("service policy admits the point sketch");
        assert_eq!(sketches.len(), 1);
        assert_eq!(entities.len(), 1);
        assert_eq!(sketches[0].name.as_deref(), Some("point sketch"));
        assert_eq!(entities[0].endpoint_refs, ["0:test:model:point#free"]);
    }
}

#[cfg(test)]
mod sketch_projection_tests;
