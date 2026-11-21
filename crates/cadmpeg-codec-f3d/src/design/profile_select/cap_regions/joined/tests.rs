// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::history_records::{
    AsmDeltaState, AsmEntityVersion, AsmHistoricalCarrierBinding, AsmHistoricalCoedge,
    AsmHistoricalCurveAxis, AsmHistoricalEdge, AsmHistoricalOptionalCarrierBinding,
    AsmHistoricalPlane, AsmHistoricalPoint,
};
use cadmpeg_ir::features::{
    ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, LinearTermination,
};
use cadmpeg_ir::scalar::NonZeroLength;
use cadmpeg_ir::sketches::{
    SketchEntity, SketchGeometry, SketchId, SketchPlacement, SketchProfiles,
};

const TOLERANCE: f64 = 1.0e-6;

#[test]
fn a_partial_face_envelope_keeps_the_selection_native() {
    let mut after = notch(0.5, true);
    assert!(after
        .face_envelopes
        .iter_mut()
        .find(|row| row.entity == 51)
        .unwrap()
        .boundary
        .pop()
        .is_some());
    assert!(run(after, 2.1, false).is_none());
}

#[test]
fn a_face_envelope_that_misses_a_current_boundary_bound_is_rejected() {
    let mut after = notch(0.5, true);
    let edge = after
        .face_envelopes
        .iter()
        .find(|row| row.entity == 51)
        .unwrap()
        .boundary[0];
    after
        .edge_envelopes
        .iter_mut()
        .find(|row| row.entity == edge)
        .unwrap()
        .high
        .z = 0.25;
    assert!(run(after, 2.1, false).is_none());
}

fn notch(cut: f64, newer: bool) -> AsmHistoricalTopology {
    notch_depth(cut, newer, -3.0)
}

fn notch_depth(cut: f64, newer: bool, bottom: f64) -> AsmHistoricalTopology {
    let yz = [
        (0.0, bottom),
        (2.0, bottom),
        (2.0, 0.0),
        (cut, 0.0),
        (cut, -2.0),
        (0.0, -2.0),
    ];
    let points: Vec<_> = [0.0, 1.0]
        .into_iter()
        .flat_map(|x| yz.map(|(y, z)| Point3::new(x, y, z)))
        .collect();
    let relation = |owner_ref, member_refs| crate::history_records::AsmHistoricalRelation {
        owner_ref,
        member_refs,
    };
    let mut topology = AsmHistoricalTopology {
        bodies: vec![1],
        regions: vec![2],
        shells: vec![3],
        body_regions: vec![relation(1, vec![2])],
        region_shells: vec![relation(2, vec![3])],
        ..AsmHistoricalTopology::default()
    };
    for (i, point) in points.iter().enumerate() {
        let i = i64::try_from(i).unwrap();
        topology.vertices.push(1000 + i);
        topology.points.push(2000 + i);
        topology.vertex_points.push(AsmHistoricalCarrierBinding {
            entity: 1000 + i,
            carrier: 2000 + i,
        });
        topology.point_positions.push(AsmHistoricalPoint {
            point: 2000 + i,
            position: *point,
        });
    }
    let mut facets = vec![
        (10, vec![0, 5, 4, 3, 2, 1], Vector3::new(-1.0, 0.0, 0.0)),
        (11, vec![6, 7, 8, 9, 10, 11], Vector3::new(1.0, 0.0, 0.0)),
    ];
    let names = [
        12,
        13,
        14,
        if newer { 51 } else { 50 },
        if newer { 61 } else { 60 },
        17,
    ];
    let normals = [
        Vector3::new(0.0, 0.0, -1.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(0.0, -1.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(0.0, -1.0, 0.0),
    ];
    for i in 0..6 {
        facets.push((
            names[i],
            vec![i, (i + 1) % 6, (i + 1) % 6 + 6, i + 6],
            normals[i],
        ));
    }
    let mut edge_ids = HashMap::new();
    for (face, vertices, normal) in facets {
        topology.faces.push(face);
        topology.surfaces.push(100 + face);
        topology.face_surfaces.push(AsmHistoricalCarrierBinding {
            entity: face,
            carrier: 100 + face,
        });
        topology.surface_planes.push(AsmHistoricalPlane {
            surface: 100 + face,
            origin: points[vertices[0]],
            normal,
        });
        topology.face_loops.push(relation(face, vec![200 + face]));
        topology.loops.push(200 + face);
        let mut coedges = Vec::new();
        let mut low = Point3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let mut high = Point3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for i in 0..vertices.len() {
            let a = vertices[i];
            let b = vertices[(i + 1) % vertices.len()];
            let key = (a.min(b), a.max(b));
            let edge = i64::try_from(key.0 * 12 + key.1).unwrap() + 300;
            if edge_ids.insert(key, edge).is_none() {
                topology.edges.push(edge);
                topology.curves.push(edge + 1000);
                topology.edge_vertices.push(AsmHistoricalEdge {
                    edge,
                    start_vertex: 1000 + i64::try_from(a).unwrap(),
                    end_vertex: 1000 + i64::try_from(b).unwrap(),
                });
                topology
                    .edge_curves
                    .push(AsmHistoricalOptionalCarrierBinding {
                        entity: edge,
                        carrier: Some(edge + 1000),
                    });
                topology.curve_axes.push(AsmHistoricalCurveAxis {
                    curve: edge + 1000,
                    origin: points[a],
                    direction: points[b].vector_from(points[a]),
                });
                let p = points[a];
                let q = points[b];
                topology.edge_envelopes.push(AsmHistoricalEnvelope {
                    entity: edge,
                    boundary: Vec::new(),
                    low: Point3::new(p.x.min(q.x), p.y.min(q.y), p.z.min(q.z)),
                    high: Point3::new(p.x.max(q.x), p.y.max(q.y), p.z.max(q.z)),
                });
            }
            let coedge = face * 1000 + i64::try_from(i).unwrap();
            coedges.push(coedge);
            topology.coedges.push(coedge);
            topology.coedge_topology.push(AsmHistoricalCoedge {
                coedge,
                owner_loop: 200 + face,
                edge,
                next: 0,
                previous: 0,
                radial_next: 0,
            });
            let point = points[a];
            low = Point3::new(low.x.min(point.x), low.y.min(point.y), low.z.min(point.z));
            high = Point3::new(
                high.x.max(point.x),
                high.y.max(point.y),
                high.z.max(point.z),
            );
        }
        topology.loop_coedges.push(relation(200 + face, coedges));
        let mut boundary: Vec<_> = topology
            .coedge_topology
            .iter()
            .filter(|row| row.owner_loop == 200 + face)
            .map(|row| row.edge)
            .collect();
        boundary.sort_unstable();
        topology.face_envelopes.push(AsmHistoricalEnvelope {
            entity: face,
            boundary,
            low,
            high,
        });
    }
    topology.shell_faces = vec![relation(3, topology.faces.clone())];
    topology.faces.sort_unstable();
    topology.surfaces.sort_unstable();
    topology.loops.sort_unstable();
    topology.edges.sort_unstable();
    topology.curves.sort_unstable();
    topology.coedges.sort_unstable();
    topology.face_surfaces.sort_by_key(|r| r.entity);
    topology.surface_planes.sort_by_key(|r| r.surface);
    topology.face_loops.sort_by_key(|r| r.owner_ref);
    topology.loop_coedges.sort_by_key(|r| r.owner_ref);
    topology.coedge_topology.sort_by_key(|r| r.coedge);
    topology.edge_vertices.sort_by_key(|r| r.edge);
    topology.edge_curves.sort_by_key(|r| r.entity);
    topology.curve_axes.sort_by_key(|r| r.curve);
    topology.face_envelopes.sort_by_key(|r| r.entity);
    topology.edge_envelopes.sort_by_key(|r| r.entity);
    topology
}

fn run(after: AsmHistoricalTopology, length: f64, ambiguous: bool) -> Option<PlanarProfileRef> {
    run_members(after, length, ambiguous, false)
}

fn run_members(
    after: AsmHistoricalTopology,
    length: f64,
    ambiguous: bool,
    aliases: bool,
) -> Option<PlanarProfileRef> {
    run_snapshot(after, length, ambiguous, aliases, false)
}

fn run_snapshot(
    after: AsmHistoricalTopology,
    length: f64,
    ambiguous: bool,
    aliases: bool,
    vertex_revision: bool,
) -> Option<PlanarProfileRef> {
    let sketch = Sketch {
        id: SketchId::mint("synthetic:test:sketch#buried-join").unwrap(),
        name: None,
        configuration: None,
        visible: None,
        native_ref: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: SketchProfiles::default(),
    };
    let mut lines = vec![
        ((0.0, 0.5), (1.0, 0.5)),
        ((1.0, 0.5), (1.0, 2.0)),
        ((1.0, 2.0), (0.0, 2.0)),
        ((0.0, 2.0), (0.0, 0.5)),
    ];
    if ambiguous {
        lines.push(((0.0, 1.0), (1.0, 1.0)));
    }
    if aliases {
        lines.extend([((-0.25, 0.5), (1.25, 0.5)), ((1.0, 0.25), (1.0, 1.25))]);
    }
    let entities: Vec<_> = lines
        .into_iter()
        .enumerate()
        .map(|(i, (a, b))| {
            SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:curve#{i}")).unwrap(),
                sketch.id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(a.0, a.1),
                    end: Point2::new(b.0, b.1),
                })
                .unwrap(),
            )
        })
        .collect();
    let states = [(2, after), (1, notch(1.0, false))]
        .into_iter()
        .map(|(state_id, topology)| {
            let mut versions: Vec<_> = topology
                .faces
                .iter()
                .map(|face| AsmEntityVersion {
                    entity_ref: *face,
                    record_ref: if state_id == 2 && [10, 11, 14].contains(face) {
                        face + 10000
                    } else {
                        *face
                    },
                })
                .collect();
            versions.extend(topology.vertices.iter().map(|vertex| AsmEntityVersion {
                entity_ref: *vertex,
                record_ref: if vertex_revision && state_id == 2 && *vertex == 1002 {
                    vertex + 10000
                } else {
                    *vertex
                },
            }));
            (
                AsmDeltaState {
                    id: format!("synthetic:test:state#{state_id}"),
                    parent: "synthetic:test:history#buried-join".into(),
                    byte_offset: 0,
                    state_id,
                    version_flag: 1,
                    state_flag: 0,
                    previous_ref: None,
                    next_ref: None,
                    node_index: state_id,
                    partner_ref: None,
                    owner_ref: 0,
                    bulletin_boards: Vec::new(),
                    records: Vec::new(),
                    transition: None,
                },
                Some(topology),
                versions,
            )
        })
        .collect();
    let history = crate::test_support::history::history(
        "synthetic:test:history#buried-join".into(),
        0,
        None,
        false,
        states,
    );
    crate::test_support::with_decode_context(|ctx| {
        let work = ProfileGeometryWork::new();
        candidate(
            &sketch,
            ScopedExtrudeProfileResolution {
                entities: &entities,
                spatial_entities: &[],
                histories: &[history],
                linear_tolerance: TOLERANCE,
                angular_tolerance: TOLERANCE,
                profile_work: &work,
                ctx,
            },
            (2, 1),
            super::super::compact::Sweep {
                operation: &BooleanOp::Join,
                start: &ExtrudeStart::ProfilePlane {},
                direction: &ExtrudeDirection::ReversedProfileNormal {},
                extent: &ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: NonZeroLength::new(length).unwrap(),
                        },
                        draft: None,
                    },
                },
            },
            &entities
                .iter()
                .map(|entity| entity.id().clone())
                .collect::<Vec<_>>(),
        )
    })
    .unwrap()
}

#[test]
fn an_updated_start_cap_certifies_a_join_with_a_buried_blind_end() {
    let selected = run(notch(0.5, true), 2.1, false)
        .expect("the rectangular sweep fills the notch and enters the base");
    assert!(
        matches!(selected, PlanarProfileRef::SketchRegions { regions, .. } if regions.len() == 1)
    );
}

#[test]
fn a_sweep_that_leaves_part_of_the_changed_wall_outside_is_rejected() {
    assert!(run(notch(0.5, true), 1.5, false).is_none());
}

#[test]
fn a_blind_end_beyond_the_resulting_base_is_rejected() {
    assert!(run(notch(0.5, true), 3.5, false).is_none());
}

#[test]
fn two_named_bounded_regions_retain_the_native_selection() {
    assert!(run(notch(0.5, true), 2.1, true).is_none());
}

#[test]
fn an_unproven_face_envelope_retains_the_native_selection() {
    let mut after = notch(0.5, true);
    after.face_envelopes.retain(|row| row.entity != 51);
    assert!(run(after, 2.1, false).is_none());
}

#[test]
fn envelope_search_work_is_bounded_before_the_geometry_walk() {
    let before = notch(1.0, false);
    let after = notch(0.5, true);
    let transition = AsmHistoricalTransition {
        geometry_unchanged: false,
        previous_state_id: Some(1),
        records: crate::history_records::AsmHistoricalEntityDelta::default(),
        topology: crate::history_records::AsmHistoricalTopologyDelta::default(),
        record_revisions: Vec::new(),
        rows: crate::history_records::AsmHistoricalRowsDelta::default(),
    };
    assert!(super::comparison_allowance(&before, &after, &transition).is_some());
    let mut many = after;
    many.faces = (0..2000).collect();
    assert!(super::comparison_allowance(&before, &many, &transition).is_none());
}

#[test]
fn coincident_named_lines_keep_one_geometric_region() {
    assert!(run_members(notch(0.5, true), 2.1, false, true).is_some());
}

#[test]
fn material_changed_outside_the_sweep_is_rejected() {
    assert!(run(notch_depth(0.5, true, -4.0), 2.1, false).is_none());
}

#[test]
fn a_nonfinite_projected_envelope_is_unavailable() {
    let frame = PlaneFrame {
        origin: Point3::new(f64::MAX, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u: Vector3::new(1.0, 0.0, 0.0),
        v: Vector3::new(0.0, 1.0, 0.0),
    };
    let bounds = AsmHistoricalEnvelope {
        entity: 1,
        boundary: Vec::new(),
        low: Point3::new(-f64::MAX, 0.0, 0.0),
        high: Point3::new(-f64::MAX, 1.0, 1.0),
    };
    assert!(Box3::of(&frame, &bounds).is_none());
}

#[test]
fn a_vertex_revision_with_the_same_point_preserves_the_certificate() {
    assert!(run_snapshot(notch(0.5, true), 2.1, false, false, true).is_some());
}
