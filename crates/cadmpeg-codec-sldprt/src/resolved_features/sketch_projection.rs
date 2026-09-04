//! Sketch projection from B-rep geometry.

use super::names::configuration;
use super::sketch_edges::{project_edge, project_endpoint_constraints, project_point};
use crate::container::ContainerScan;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchId,
};
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::Exactness;
use std::collections::{HashMap, HashSet};

#[cfg(test)]
use super::sketch_edges::{circle_contains_point, ellipse_contains_point};

/// Decode nested feature-input Parasolid streams as placed planar sketches.
pub fn sketches(
    scan: &ContainerScan,
    annotations: &mut Annotations,
) -> (Vec<Sketch>, Vec<SketchEntity>, Vec<SketchConstraint>) {
    let mut sketches = Vec::new();
    let mut entities = Vec::new();
    let mut constraints = Vec::new();
    for source in scan.sections() {
        let Some(section) = source.name() else {
            continue;
        };
        if !section.to_ascii_lowercase().contains("resolvedfeatures") {
            continue;
        }
        let native_ref = format!(
            "sldprt:feature-input:resolved-features#{}",
            source.ordinal()
        );
        for (stream_ordinal, stream) in source.ps_streams().iter().enumerate() {
            let brep = crate::brep::decode(&stream.payload, &stream.header, section);
            project_brep(
                &brep,
                source.ordinal(),
                stream_ordinal,
                stream.offset,
                section,
                &stream.header.description,
                configuration(section).as_deref(),
                &native_ref,
                annotations,
                &mut sketches,
                &mut entities,
                &mut constraints,
            );
        }
    }
    (sketches, entities, constraints)
}

#[allow(clippy::too_many_arguments)]
fn project_brep(
    brep: &crate::brep::Brep,
    block_offset: usize,
    stream_ordinal: usize,
    stream_offset: usize,
    section: &str,
    sketch_name: &str,
    configuration: Option<&str>,
    native_ref: &str,
    annotations: &mut Annotations,
    sketches: &mut Vec<Sketch>,
    entities: &mut Vec<SketchEntity>,
    constraints: &mut Vec<SketchConstraint>,
) {
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
        .map(|point| (&point.id, point.position))
        .collect::<HashMap<_, _>>();
    let curves = brep
        .curves
        .iter()
        .map(|curve| (&curve.id, &curve.geometry))
        .collect::<HashMap<_, _>>();

    for (face_ordinal, face) in brep.faces.iter().enumerate() {
        let Some(SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        }) = surfaces.get(&face.surface).copied()
        else {
            continue;
        };
        let sketch_id = SketchId(format!(
            "sldprt:model:sketch#{block_offset}:{stream_ordinal}:{face_ordinal}"
        ));
        let v_axis = normal.cross(*u_axis);
        let first_entity = entities.len();
        let mut edge_entities = HashMap::<&cadmpeg_ir::ids::EdgeId, SketchEntityId>::new();
        let mut used_vertices = HashSet::new();
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
                used_vertices.insert(edge.start.clone());
                used_vertices.insert(edge.end.clone());
                let entity_id = if let Some(id) = edge_entities.get(&edge.id) {
                    id.clone()
                } else {
                    let id = SketchEntityId(format!(
                        "sldprt:model:sketch-entity#{block_offset}:{stream_ordinal}:{face_ordinal}:{}",
                        edge_entities.len()
                    ));
                    let Some(geometry) =
                        project_edge(edge, &vertices, &points, &curves, *origin, *u_axis, v_axis)
                    else {
                        continue;
                    };
                    let Some(start_point) = vertices.get(&edge.start) else {
                        continue;
                    };
                    let Some(end_point) = vertices.get(&edge.end) else {
                        continue;
                    };
                    crate::annotations::note(
                        annotations,
                        id.0.clone(),
                        section,
                        0,
                        "feature_input_profile_edge",
                        Exactness::Derived,
                    );
                    entities.push(
                        SketchEntity::new(id.clone(), sketch_id.clone(), geometry)
                            .with_native_ref(Some(format!("{stream_ordinal}:{}", edge.id.as_str())))
                            .with_geometry_ref(
                                edge.curve
                                    .as_ref()
                                    .map(|id| format!("{stream_ordinal}:{}", id.0)),
                            )
                            .with_endpoint_refs(vec![
                                format!("{stream_ordinal}:{}", start_point.0),
                                format!("{stream_ordinal}:{}", end_point.0),
                            ]),
                    );
                    edge_entities.insert(&edge.id, id.clone());
                    id
                };
                if edge.curve.is_some() || edge.start != edge.end {
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
            let id = SketchEntityId(format!(
                "sldprt:model:sketch-entity#{block_offset}:{stream_ordinal}:{face_ordinal}:{}",
                edge_entities.len()
                    + entities
                        .iter()
                        .filter(|entity| entity.sketch == sketch_id)
                        .count()
            ));
            crate::annotations::note(
                annotations,
                id.0.clone(),
                section,
                0,
                "feature_input_profile_point",
                Exactness::Derived,
            );
            entities.push(
                SketchEntity::new(
                    id,
                    sketch_id.clone(),
                    SketchGeometry::Point {
                        position: project_point(*position, *origin, *u_axis, v_axis),
                    },
                )
                .with_native_ref(Some(format!("{stream_ordinal}:{}", vertex.id.as_str())))
                .with_endpoint_refs(vec![format!("{stream_ordinal}:{}", vertex.point.0)]),
            );
        }
        if profiles.is_empty() && !entities.iter().any(|entity| entity.sketch == sketch_id) {
            continue;
        }
        crate::annotations::note(
            annotations,
            sketch_id.0.clone(),
            section,
            stream_offset as u64,
            "feature_input_profile",
            Exactness::Derived,
        );
        project_endpoint_constraints(
            &sketch_id,
            &entities[first_entity..],
            block_offset,
            stream_ordinal,
            face_ordinal,
            section,
            annotations,
            constraints,
        );
        sketches.push(Sketch {
            id: sketch_id,
            name: (!sketch_name.is_empty()).then(|| sketch_name.to_string()),
            configuration: configuration.map(str::to_string),
            visible: None,
            placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
                origin: *origin,
                normal: *normal,
                u_axis: *u_axis,
            },
            profiles,
            native_ref: Some(native_ref.to_string()),
        });
    }
}

fn orient_closed_profile_by_topology(profile: &mut [SketchEntityUse], entities: &[SketchEntity]) {
    if profile.len() < 2 {
        return;
    }
    let entities = entities
        .iter()
        .map(|entity| (entity.id(), entity))
        .collect::<HashMap<_, _>>();
    let orientations = profile
        .iter()
        .enumerate()
        .map(|(index, use_)| {
            let current = entities.get(&use_.entity)?;
            let next = entities.get(&profile[(index + 1) % profile.len()].entity)?;
            let [start, end] = current.endpoint_refs.as_slice() else {
                return None;
            };
            let shared = current
                .endpoint_refs
                .iter()
                .filter(|endpoint| next.endpoint_refs.contains(endpoint))
                .collect::<Vec<_>>();
            let [shared] = shared.as_slice() else {
                return None;
            };
            if *shared == end {
                Some(false)
            } else if *shared == start {
                Some(true)
            } else {
                None
            }
        })
        .collect::<Option<Vec<_>>>();
    let Some(orientations) = orientations else {
        return;
    };
    for (use_, reversed) in profile.iter_mut().zip(orientations) {
        use_.reversed = reversed;
    }
}

#[cfg(test)]
mod projected_profile_orientation_tests {
    use super::{circle_contains_point, ellipse_contains_point, orient_closed_profile_by_topology};
    use cadmpeg_ir::{
        math::Point2,
        sketches::{SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchId},
    };

    fn line(id: &str, start_ref: &str, end_ref: &str) -> SketchEntity {
        SketchEntity::new(
            SketchEntityId(id.into()),
            SketchId("sketch".into()),
            SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            },
        )
        .with_endpoint_refs(vec![start_ref.into(), end_ref.into()])
    }

    #[test]
    fn orients_each_closed_profile_edge_toward_its_topological_successor() {
        let entities = [
            line("a", "p0", "p1"),
            line("b", "p1", "p2"),
            line("c", "p0", "p2"),
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
            line("a", "p0", "p1"),
            line("b", "p1", "p2"),
            line("c", "p3", "p4"),
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

    #[test]
    fn rejects_analytic_carriers_that_do_not_contain_the_edge_vertex() {
        assert!(!circle_contains_point(
            Point2::new(-35.0, -5.85),
            1.25,
            Point2::new(-75.0, -8.85),
            1.0e-9,
        ));
        assert!(!ellipse_contains_point(
            Point2::new(-60.0, -150.0),
            0.0,
            7.5,
            f64::MIN_POSITIVE,
            Point2::new(140.0, -70.5),
            1.0e-9,
        ));
    }

    #[test]
    fn accepts_vertices_on_nondegenerate_analytic_carriers() {
        assert!(circle_contains_point(
            Point2::new(2.0, 3.0),
            4.0,
            Point2::new(6.0, 3.0),
            1.0e-9,
        ));
        assert!(ellipse_contains_point(
            Point2::new(2.0, 3.0),
            0.0,
            4.0,
            2.0,
            Point2::new(2.0, 5.0),
            1.0e-9,
        ));
    }
}

#[cfg(test)]
mod sketch_projection_tests;
