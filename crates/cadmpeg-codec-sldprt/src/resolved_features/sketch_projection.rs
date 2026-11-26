//! Sketch projection from B-rep geometry.

use super::assembly::contains_ascii_case_insensitive;
use super::names::configuration;
use super::sketch_edges::{
    nurbs_edge_trim_range, project_edge, project_endpoint_constraints, project_point,
    EndpointConstraintSource,
};
use crate::container::ContainerScan;
use cadmpeg_core::decode::{u64_from_index, ArenaClass};
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId,
};
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::unknown::part::{DecodeParts, GivenUpPart, Part};
use cadmpeg_ir::Exactness;
use std::collections::{HashMap, HashSet};

/// Sketches and their projected entities and constraints.
#[derive(Default)]
pub(crate) struct ProjectedSketches {
    pub(crate) sketches: Vec<Sketch>,
    pub(crate) entities: Vec<SketchEntity>,
    pub(crate) constraints: Vec<SketchConstraint>,
}

/// Decode nested feature-input Parasolid streams as placed planar sketches.
/// A nested stream that does not decode is given up into `given_up`.
pub(crate) fn sketches(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    annotations: &mut Annotations,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    given_up: &mut Vec<GivenUpPart>,
) -> Result<ProjectedSketches, cadmpeg_core::CodecError> {
    let mut projected = ProjectedSketches::default();
    for source in scan.sections() {
        let Some(section) = source.name() else {
            continue;
        };
        if crate::container::is_stock_bodies_section(section) {
            let count = source
                .ps_streams()
                .iter()
                .filter(|stream| crate::parasolid::is_body_stream(&stream.header))
                .count();
            if count > 0 {
                losses.push(crate::loss::SldprtLossCode::FeatureInputSolidBodyRetained.note(
                    format!(
                        "{count} stock-body partition stream(s) remain in {} as derived-feature inputs.",
                        source.native_id()
                    ),
                ));
            }
            continue;
        }
        if !contains_ascii_case_insensitive(section, "resolvedfeatures") {
            continue;
        }
        let source_stream = source.source_stream();
        let native_ref = format!(
            "sldprt:feature-input:resolved-features#{}",
            source.ordinal()
        );
        for (stream_ordinal, stream) in source.ps_streams().iter().enumerate() {
            let brep = match ctx.run_part(|| {
                crate::brep::graph::decode(ctx, &stream.payload, &stream.header, source_stream)
            })? {
                Ok(brep) => brep,
                Err(error) => {
                    let part = Part::at(
                        "sldprt",
                        "feature-input-parasolid-stream",
                        u64_from_index(stream.offset),
                        &stream.payload,
                        [ArenaClass::Design],
                    )
                    .named(stream.header.description.as_str())
                    .in_stream(source_stream.clone())
                    .costing(format!("{native_ref} states no sketch from this stream"))
                    .digest_only();
                    given_up.push(ctx.give_up_part(part, error)?);
                    continue;
                }
            };
            if is_kernel_transmit_header(&stream.header.description)
                && !brep.stats.synthetic_body_grouping
                && brep
                    .bodies
                    .iter()
                    .any(|body| body.kind == cadmpeg_ir::topology::BodyKind::Solid)
            {
                losses.push(crate::loss::SldprtLossCode::FeatureInputSolidBodyRetained.note(
                    format!(
                        "Nested solid-body transmit {stream_ordinal} at offset {} is retained in {native_ref}; its faces are not sketch profiles.",
                        stream.offset
                    ),
                ));
                continue;
            }
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
                losses,
                &mut projected,
            )?;
        }
    }
    Ok(projected)
}

/// Whether a nested stream's Parasolid description is the kernel transmit
/// header, which names no sketch.
fn is_kernel_transmit_header(description: &str) -> bool {
    description.starts_with(": TRANSMIT FILE")
}

/// The id of the sketch that planar face `face_ordinal` of nested stream
/// `stream_ordinal` in the section at `block_offset` gives.
fn profile_stream_sketch_id(
    block_offset: usize,
    stream_ordinal: usize,
    face_ordinal: usize,
) -> Option<SketchId> {
    SketchId::mint(format!(
        "sldprt:model:sketch#{block_offset}:{stream_ordinal}:{face_ordinal}"
    ))
    .ok()
}

/// Whether `id` is the id of a sketch that a planar face of a nested profile
/// stream gives.
pub(crate) fn is_profile_stream_sketch(id: &SketchId) -> bool {
    id.as_str()
        .strip_prefix("sldprt:model:sketch#")
        .is_some_and(|key| {
            key.split(':').count() == 3 && key.split(':').all(|part| part.parse::<usize>().is_ok())
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
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    projected: &mut ProjectedSketches,
) -> Result<(), cadmpeg_core::CodecError> {
    let ProjectedSketches {
        sketches,
        entities,
        constraints,
    } = projected;
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
    let vertex_tolerances = brep
        .vertices
        .iter()
        .map(|vertex| {
            (
                &vertex.id,
                vertex
                    .tolerance
                    .map(cadmpeg_ir::scalar::PositiveLength::get),
            )
        })
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
        let Some(sketch_id) = profile_stream_sketch_id(block_offset, stream_ordinal, face_ordinal)
        else {
            continue;
        };
        let Ok(placement) =
            cadmpeg_ir::sketches::SketchPlacement::try_resolved(origin, *normal, *u_axis)
        else {
            continue;
        };
        let v_axis = normal.cross(*u_axis);
        let first_entity = entities.len();
        let mut edge_entities = HashMap::<&cadmpeg_ir::ids::EdgeId, (SketchEntityId, bool)>::new();
        let mut used_vertices = HashSet::<&cadmpeg_ir::ids::VertexId>::new();
        let mut profiles = Vec::new();
        // Edge refusals and their loops. The face's profiles end before the
        // first refused loop, so no later loop takes its position.
        let mut refusals = Vec::new();
        let mut refused_edges = Vec::<&cadmpeg_ir::ids::EdgeId>::new();
        let Some(face_loops) = face.loops.boundary() else {
            continue;
        };
        for loop_id in face_loops {
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
                for vertex_id in edge.vertices() {
                    used_vertices.insert(vertex_id);
                }
                let (entity_id, start_aligned) = if let Some((id, start_aligned)) =
                    edge_entities.get(&edge.id)
                {
                    let id = id.as_str().to_owned();
                    (
                        SketchEntityId::mint(id).map_err(cadmpeg_core::CodecError::malformed)?,
                        *start_aligned,
                    )
                } else {
                    let Ok(id) = SketchEntityId::mint(format!(
                        "sldprt:model:sketch-entity#{block_offset}:{stream_ordinal}:{face_ordinal}:{}",
                        edge_entities.len()
                    )) else { continue };
                    let mut edge_refusal = crate::lane_refusal::LaneRefusals::new();
                    let nurbs_trim = nurbs_edge_trim_range(
                        ctx,
                        edge,
                        &vertices,
                        &vertex_tolerances,
                        &points,
                        &curves,
                    )?;
                    let projected = project_edge(
                        edge,
                        &vertices,
                        &points,
                        &curves,
                        super::sketch_edges::SketchPlaneFrame {
                            origin,
                            u_axis: *u_axis,
                            v_axis,
                        },
                        nurbs_trim,
                        &mut edge_refusal,
                    );
                    let edge_refusals = edge_refusal.take_records();
                    if !edge_refusals.is_empty() {
                        refusals.extend(edge_refusals.into_iter().map(|record| (record, loop_id)));
                        if !refused_edges.contains(&&edge.id) {
                            refused_edges.push(&edge.id);
                        }
                        continue;
                    }
                    let Some((geometry, start_aligned)) = projected else {
                        continue;
                    };
                    let Some(start_point) = edge.start().and_then(|start| vertices.get(start))
                    else {
                        continue;
                    };
                    let Some(end_point) = edge.end().and_then(|end| vertices.get(end)) else {
                        continue;
                    };
                    crate::annotations::note(
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
                    let endpoint_refs = if start_aligned {
                        [start_point, end_point]
                    } else {
                        [end_point, start_point]
                    }
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
                    edge_entities.insert(&edge.id, (index_id, start_aligned));
                    (id, start_aligned)
                };
                if edge.curve().is_some()
                    || edge
                        .ends
                        .as_ref()
                        .is_some_and(|ends| ends.start != ends.end)
                {
                    profile.push(SketchEntityUse {
                        entity: entity_id,
                        reversed: (coedge.sense == Sense::Reversed) == start_aligned,
                    });
                }
            }
            if refusals.is_empty() && !profile.is_empty() {
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
        let profiles = cadmpeg_ir::sketches::SketchProfiles::try_from(profiles)
            .ok()
            .filter(|profiles| !profiles.is_empty() || entities.len() > first_entity);
        for (record, loop_id) in refusals {
            let consequence = match profiles {
                Some(_) => format!(
                    "sketch {sketch_id} states no entity for the edge and no profile for loop \
                     {loop_id} or a later loop of face {}",
                    face.id
                ),
                None => format!(
                    "face {} of the feature-input stream states no sketch",
                    face.id
                ),
            };
            losses.push(
                crate::loss::SldprtLossCode::SketchProfileRejected
                    .note(format!("{record}; {consequence}")),
            );
        }
        losses.extend(refused_edges.into_iter().map(|edge| {
            cadmpeg_ir::unknown::part::record_unreadable(
                edge,
                crate::loss::SldprtLossCode::SketchProfileRejected.kind(),
            )
        }));
        let Some(profiles) = profiles else {
            continue;
        };
        crate::annotations::note(
            annotations,
            sketch_id.as_str(),
            source_stream,
            cadmpeg_core::decode::u64_from_index(stream_offset),
            "feature_input_profile",
            Exactness::Derived,
        )?;
        project_endpoint_constraints(
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
        let name = (!sketch_name.is_empty() && !is_kernel_transmit_header(sketch_name))
            .then(|| sketch_name.to_owned());
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
    use super::{project_brep, BrepSketchSource, ProjectedSketches};
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
                native_ref: None,
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
                native_ref: None,
            }],
            vertices: vec![Vertex {
                id: VertexId::mint("test:model:vertex#free").expect("vertex ID"),
                point: point_id.clone(),
                tolerance: None,
                native_ref: None,
            }],
            points: vec![Point::new(
                point_id,
                cadmpeg_ir::features::geometry::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0))
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
                native_ref: None,
            });
        }
        // A missing point must not consume a projected entity ordinal.
        brep.vertices.insert(
            1,
            Vertex {
                id: VertexId::mint("test:model:vertex#missing").unwrap(),
                point: PointId::mint("test:model:point#missing").unwrap(),
                tolerance: None,
                native_ref: None,
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
        let mut projected = ProjectedSketches {
            entities: vec![prior],
            ..ProjectedSketches::default()
        };
        let mut annotations = cadmpeg_ir::annotations::Annotations::default();
        let stream = cadmpeg_ir::stream_name!("test:sketch-projection");
        project_brep(
            &cadmpeg_core::decode::test_support::service_decode_context(),
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
            &mut Vec::new(),
            &mut projected,
        )
        .unwrap();
        let ProjectedSketches {
            sketches, entities, ..
        } = projected;

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
        let stream = cadmpeg_ir::stream_name!("test:sketch-projection");
        let mut annotations = cadmpeg_ir::annotations::Annotations::default();
        let mut projected = ProjectedSketches::default();
        project_brep(
            &cadmpeg_core::decode::test_support::service_decode_context(),
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
            &mut Vec::new(),
            &mut projected,
        )
        .expect("project B-rep");
        let ProjectedSketches {
            sketches, entities, ..
        } = projected;
        assert_eq!(sketches.len(), 1);
        assert_eq!(entities.len(), 1);
        assert_eq!(sketches[0].name.as_deref(), Some("point sketch"));
        assert_eq!(entities[0].endpoint_refs, ["0:test:model:point#free"]);
    }
}

#[cfg(test)]
mod sketch_projection_tests;

#[cfg(test)]
mod stream_sketch_id_tests {
    use super::{is_profile_stream_sketch, profile_stream_sketch_id};
    use cadmpeg_ir::sketches::SketchId;

    #[test]
    fn a_stream_face_id_is_a_profile_stream_sketch_and_reader_ids_are_not() {
        assert!(is_profile_stream_sketch(
            &profile_stream_sketch_id(184_117, 0, 3).unwrap()
        ));
        for other in [
            "sldprt:model:sketch#markers:184117:31",
            "sldprt:model:sketch#compact:184117:31",
            "sldprt:model:sketch#184117:0",
            "sldprt:model:sketch#184117:0:0:1",
        ] {
            assert!(!is_profile_stream_sketch(&SketchId::mint(other).unwrap()));
        }
    }
}
