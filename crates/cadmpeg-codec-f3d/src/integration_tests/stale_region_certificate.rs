// SPDX-License-Identifier: Apache-2.0
//! Counted partial-curve selections bind trimmed neutral Extrude regions.

use std::collections::HashMap;

use cadmpeg_ir::features::{Feature, PlanarProfileRef, ProfileRef, SketchProfileRegion};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchGeometry, SketchPlacement};
use serde_json::json;

use crate::design::geometry::ProfileGeometryWork;
use crate::design::profile_select::{
    bind_extrude_profile_selections, ExtrudeProfileResolution, SketchCurveSelectionResolution,
};
use crate::loss::F3dLossCode;
use crate::records::feature::scope::{
    DesignExtrudeScope, DesignParameterScope, DesignScopePayload,
};
use crate::records::sketch_geometry::SketchCurveIdentity;
use crate::records::topology::sketch_profile::DesignSketchProfileOperand;
use cadmpeg_ir::report::loss::LossNote;

const REGION_TOLERANCE: f64 = 1.0e-7;
const ANGULAR_TOLERANCE: f64 = 1.0e-9;

use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCurveAxis, AsmHistoricalEdge,
    AsmHistoricalOptionalCarrierBinding, AsmHistoricalPoint, AsmHistoricalRelation,
    AsmHistoricalTopology,
};
fn fixture_with_flags(
    geometry: Vec<serde_json::Value>,
    profile_entities: &[&[usize]],
    members: &[(u32, bool)],
) -> (
    Sketch,
    Vec<SketchEntity>,
    Vec<SketchCurveIdentity>,
    DesignParameterScope,
) {
    let sketch_id = cadmpeg_ir::sketches::SketchId::mint("synthetic:test:sketch#42").unwrap();
    let mut entities = Vec::new();
    let mut curves = Vec::new();
    for (index, geometry) in geometry.into_iter().enumerate() {
        let primary = u32::try_from(index + 1).unwrap();
        entities.push(SketchEntity::new(
            crate::ids::neutral_sketch_curve_id(&sketch_id, u64::from(primary), 0),
            sketch_id.clone(),
            serde_json::from_value::<SketchGeometry>(geometry).unwrap(),
        ));
        curves.push(SketchCurveIdentity {
            id: format!("f3d:Design/BulkStream.dat:curve#{primary}"),
            record_index: primary,
            owner_reference: Some(42),
            geometry_group: None,
            class_tag: "300".to_owned().try_into().unwrap(),
            byte_offset: 0,
            geometry_offset: 0,
            entity_genesis: None,
            primary_id: std::num::NonZeroU64::new(u64::from(primary)).unwrap(),
            secondary_id: 0,
            geometry: None,
            planar_z_noise: None,
        });
    }
    let profiles = profile_entities
        .iter()
        .map(|indices| {
            indices
                .iter()
                .map(|index| json!({"entity": entities[*index].id(), "reversed": false}))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 10.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: serde_json::from_value(json!(profiles)).unwrap(),
        native_ref: None,
    };
    let members = members
        .iter()
        .map(|(primary, reversed)| {
            json!({
                "kind": 3, "kind_offset": 0, "curve_primary_id": primary,
                "curve_primary_id_offset": 4,
                "incidence_words": [0, 0, 0, u32::from(*reversed), 3, 8, 0, 0],
                "incidence_words_offset": 8,
            })
        })
        .collect::<Vec<_>>();
    let operand: DesignSketchProfileOperand = serde_json::from_value(json!({
        "scope_reference_ordinal": 0, "record_index": 100, "byte_offset": 10,
        "class_tag": "301", "asset_id": "01234567-89ab-cdef-0123-456789abcdef",
        "asset_id_offset": 20, "entity_id": "0_42", "entity_suffix": 42,
        "entity_reference_offset": 100, "paired_class_tag": "259", "paired_byte_offset": 200,
        "region_selection": {
            "record_index": 103, "byte_offset": 300, "class_tag": "302", "region_count_offset": 336,
            "regions": [{"member_count_offset": 340, "members": members}],
            "companion_class_tag": "260", "companion_byte_offset": 700,
        }
    }))
    .unwrap();
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#10",
        DesignScopePayload::Extrude(Some(DesignExtrudeScope {
            extrude_profile: Some(operand),
            ..DesignExtrudeScope::default()
        })),
        10,
    );
    (sketch, entities, curves, scope)
}

type PlaneFace = (i64, f64, Vec<Vec<(f64, f64)>>);

/// Faces whose edges all have straight carriers.
fn plane_faces(faces: &[PlaneFace]) -> AsmHistoricalTopology {
    plane_faces_from(faces, 10_000)
}

fn plane_faces_from(faces: &[PlaneFace], mut next: i64) -> AsmHistoricalTopology {
    let mut topology = AsmHistoricalTopology::default();
    let mut mint = || {
        next += 1;
        next
    };
    for (face, height, loops) in faces {
        let surface = mint();
        topology.faces.push(*face);
        topology.surfaces.push(surface);
        topology.face_surfaces.push(AsmHistoricalCarrierBinding {
            entity: *face,
            carrier: surface,
        });
        topology
            .surface_planes
            .push(crate::history_records::AsmHistoricalPlane {
                surface,
                origin: Point3::new(0.0, 0.0, *height),
                normal: Vector3::new(0.0, 0.0, 1.0),
            });
        let mut face_loops = Vec::new();
        for ring in loops {
            let loop_ = mint();
            face_loops.push(loop_);
            topology.loops.push(loop_);
            let vertices: Vec<i64> = ring
                .iter()
                .map(|(u, v)| {
                    let (vertex, point) = (mint(), mint());
                    topology.vertices.push(vertex);
                    topology.points.push(point);
                    topology.vertex_points.push(AsmHistoricalCarrierBinding {
                        entity: vertex,
                        carrier: point,
                    });
                    topology.point_positions.push(AsmHistoricalPoint {
                        point,
                        position: Point3::new(*u, *v, *height),
                    });
                    vertex
                })
                .collect();
            let mut coedges = Vec::new();
            for k in 0..ring.len() {
                let (edge, coedge, curve) = (mint(), mint(), mint());
                let (start, end) = (ring[k], ring[(k + 1) % ring.len()]);
                topology.edges.push(edge);
                topology.coedges.push(coedge);
                topology.curves.push(curve);
                coedges.push(coedge);
                topology.coedge_topology.push(AsmHistoricalCoedge {
                    coedge,
                    owner_loop: loop_,
                    edge,
                    next: 0,
                    previous: 0,
                    radial_next: 0,
                });
                topology.edge_vertices.push(AsmHistoricalEdge {
                    edge,
                    start_vertex: vertices[k],
                    end_vertex: vertices[(k + 1) % ring.len()],
                });
                topology
                    .edge_curves
                    .push(AsmHistoricalOptionalCarrierBinding {
                        entity: edge,
                        carrier: Some(curve),
                    });
                topology.curve_axes.push(AsmHistoricalCurveAxis {
                    curve,
                    origin: Point3::new(start.0, start.1, *height),
                    direction: Vector3::new(end.0 - start.0, end.1 - start.1, 0.0),
                });
            }
            topology.loop_coedges.push(AsmHistoricalRelation {
                owner_ref: loop_,
                member_refs: coedges,
            });
        }
        topology.face_loops.push(AsmHistoricalRelation {
            owner_ref: *face,
            member_refs: face_loops,
        });
    }
    topology
}

/// A history whose state 2 follows state 1.
fn cut_history(
    before: AsmHistoricalTopology,
    after: AsmHistoricalTopology,
) -> crate::history_records::AsmHistory {
    let state = |state_id: i64, topology| {
        (
            crate::history_records::AsmDeltaState {
                id: format!("synthetic:test:state#{state_id}"),
                parent: "synthetic:test:history".into(),
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
            Vec::new(),
        )
    };
    crate::test_support::history::history(
        "synthetic:test:history".into(),
        0,
        None,
        false,
        vec![state(2, after), state(1, before)],
    )
}

fn relation(rows: &[AsmHistoricalRelation], owner: i64) -> Option<&[i64]> {
    Some(&rows.iter().find(|row| row.owner_ref == owner)?.member_refs)
}
fn rotate_sides(after: &mut AsmHistoricalTopology, sides: &[(i64, bool, f64)]) {
    for (face, fixed_x, offset) in sides {
        let transform = |point: Point3| {
            if *fixed_x {
                Point3::new(*offset, point.x, point.y)
            } else {
                Point3::new(point.x, *offset, point.y)
            }
        };
        let transform_vector = |vector: Vector3| {
            if *fixed_x {
                Vector3::new(0.0, vector.x, vector.y)
            } else {
                Vector3::new(vector.x, 0.0, vector.y)
            }
        };
        let loops = relation(&after.face_loops, *face).unwrap();
        let coedges: Vec<_> = loops
            .iter()
            .flat_map(|loop_| relation(&after.loop_coedges, *loop_).unwrap())
            .copied()
            .collect();
        let edges: Vec<_> = after
            .coedge_topology
            .iter()
            .filter(|row| coedges.contains(&row.coedge))
            .map(|row| row.edge)
            .collect();
        let vertices: Vec<_> = after
            .edge_vertices
            .iter()
            .filter(|row| edges.contains(&row.edge))
            .flat_map(|row| [row.start_vertex, row.end_vertex])
            .collect();
        let points: Vec<_> = after
            .vertex_points
            .iter()
            .filter(|row| vertices.contains(&row.entity))
            .map(|row| row.carrier)
            .collect();
        for point in &mut after.point_positions {
            if points.contains(&point.point) {
                point.position = transform(point.position);
            }
        }
        let curves: Vec<_> = after
            .edge_curves
            .iter()
            .filter(|row| edges.contains(&row.entity))
            .filter_map(|row| row.carrier)
            .collect();
        for axis in &mut after.curve_axes {
            if curves.contains(&axis.curve) {
                axis.origin = transform(axis.origin);
                axis.direction = transform_vector(axis.direction);
            }
        }
        let surface = after
            .face_surfaces
            .iter()
            .find(|row| row.entity == *face)
            .unwrap()
            .carrier;
        let plane = after
            .surface_planes
            .iter_mut()
            .find(|row| row.surface == surface)
            .unwrap();
        plane.origin = transform(Point3::new(0.0, 0.0, 0.0));
        plane.normal = if *fixed_x {
            Vector3::new(1.0, 0.0, 0.0)
        } else {
            Vector3::new(0.0, 1.0, 0.0)
        };
    }
}
fn bind_cut(both: bool, surviving: u32) -> (ProfileRef, Vec<LossNote>, Vec<SketchEntity>) {
    let outline = [(0.0, 0.0), (4.0, 0.0), (4.0, 2.0), (0.0, 2.0)];
    let mut geometry = Vec::new();
    for k in 0..4 {
        geometry.push(
            json!({"kind": "line", "start": {"u": outline[k].0, "v": outline[k].1},
            "end": {"u": outline[(k+1)%4].0, "v": outline[(k+1)%4].1}}),
        );
    }
    geometry
        .push(json!({"kind": "line", "start": {"u": 2.0, "v": 0.0}, "end": {"u": 2.0, "v": 2.0}}));
    let (mut sketch, entities, curves, scope) = fixture_with_flags(
        geometry,
        &[&[0, 1, 2, 3]],
        &[(99, false), (surviving, false)],
    );
    sketch.placement = SketchPlacement::try_resolved(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
    .unwrap();
    let mut draft = scope.into_draft();
    draft.frame_length = 124;
    draft.paired_byte_offset = 124;
    draft.history_state_id = Some(2);
    draft.previous_history_state_id = Some(1);
    let previous_offset =
        crate::design::decode::scopes::parameter_scope::parameter_scope_previous_history_offset(
            "Extrude",
            usize::try_from(draft.paired_byte_offset - draft.feature_ordinal_offset).unwrap(),
            false,
            false,
        )
        .unwrap();
    draft.previous_history_state_id_offset =
        Some(draft.feature_ordinal_offset + u64::try_from(previous_offset).unwrap());
    let scope = DesignParameterScope::try_new(draft).unwrap();
    let plate = vec![(-1.0, -1.0), (4.0, -1.0), (4.0, 3.0), (-1.0, 3.0)];
    let left = if both { 0.0 } else { 2.0 };
    let before = plane_faces_from(
        &[
            (1, 0.0, vec![plate.clone()]),
            (2, -2.0, vec![plate.clone()]),
        ],
        50_000,
    );
    let side_x = vec![(0.0, 0.0), (2.0, 0.0), (2.0, -2.0), (0.0, -2.0)];
    let side_y = vec![(left, 0.0), (4.0, 0.0), (4.0, -2.0), (left, -2.0)];
    let holed = vec![vec![
        (-1.0, -1.0),
        (4.0, -1.0),
        (4.0, 0.0),
        (left, 0.0),
        (left, 2.0),
        (4.0, 2.0),
        (4.0, 3.0),
        (-1.0, 3.0),
    ]];
    let mut after = plane_faces(&[
        (11, 0.0, holed.clone()),
        (12, -2.0, holed),
        (13, 0.0, vec![side_x]),
        (15, 0.0, vec![side_y.clone()]),
        (16, 0.0, vec![side_y]),
    ]);
    rotate_sides(
        &mut after,
        &[(13, true, left), (15, false, 0.0), (16, false, 2.0)],
    );
    let history = cut_history(before, after);
    let mut feature: Feature = serde_json::from_value(json!({
        "id": "synthetic:test:feature#cut", "ordinal": 0, "suppressed": false, "native_ref": scope.id,
        "definition": {"definition": "extrude", "profile": {"kind": "sketch", "value": sketch.id},
        "direction": {"kind": "reversed_profile_normal"}, "start": {"kind": "profile_plane"},
        "extent": {"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": 3.0}}},
        "op": "cut", "solid": true}
    })).unwrap();
    let work = ProfileGeometryWork::new();
    let losses = crate::test_support::with_decode_context(|ctx| {
        bind_extrude_profile_selections(
            std::slice::from_mut(&mut feature),
            std::slice::from_ref(&scope),
            &[],
            &[],
            std::slice::from_ref(&sketch),
            &SketchCurveSelectionResolution {
                scopes: std::slice::from_ref(&scope),
                groups: &[],
                operands: &[],
                profile_curves: &[],
                placements: &[],
                curve_identities: &curves,
                sketches: std::slice::from_ref(&sketch),
                sketch_entities: &entities,
                spatial_sketches: &[],
                spatial_sketch_entities: &[],
            },
            ExtrudeProfileResolution {
                entities: &entities,
                spatial_sketches: &[],
                spatial_entities: &[],
                histories: std::slice::from_ref(&history),
                scope_histories: &HashMap::new(),
                linear_tolerance: REGION_TOLERANCE,
                angular_tolerance: ANGULAR_TOLERANCE,
                profile_work: &work,
                ctx,
            },
        )
    })
    .unwrap();
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Extrude { profile, .. },
    ) = feature.evaluation.into_parts().0
    else {
        panic!("extrude");
    };
    (profile, losses, entities)
}

#[test]
fn stale_region_identity_uses_the_certified_side_of_an_open_chain() {
    let (profile, losses, entities) = bind_cut(false, 2);
    let ProfileRef::Planar(PlanarProfileRef::SketchRegions { regions, .. }) = profile else {
        panic!("certified region");
    };
    assert_eq!(regions.len(), 1);
    let SketchProfileRegion::Trimmed {
        outer_boundary,
        hole_boundaries,
    } = &regions[0]
    else {
        panic!("trimmed region");
    };
    assert!(hole_boundaries.is_empty());
    assert!(outer_boundary
        .iter()
        .any(|use_| &use_.entity == entities[1].id()));
    assert!(outer_boundary
        .iter()
        .any(|use_| &use_.entity == entities[4].id()));
    assert!(outer_boundary
        .iter()
        .all(|use_| &use_.entity != entities[3].id()));
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureProfileRegionIdentitiesStale.kind()
    );
    assert!(losses[0].message.contains("synthetic:test:feature#cut"));
}

fn assert_native(profile: ProfileRef, losses: &[LossNote]) {
    let ProfileRef::Planar(PlanarProfileRef::SketchSelection { selections, .. }) = profile else {
        panic!("native selection");
    };
    assert_eq!(
        selections.as_slice(),
        &["f3d:Design/BulkStream.dat:scope#10".to_string()]
    );
    assert!(losses.is_empty());
}

#[test]
fn stale_region_count_keeps_the_native_selection_when_both_regions_are_cut() {
    let (profile, losses, _) = bind_cut(true, 2);
    assert_native(profile, &losses);
}

#[test]
fn stale_region_keeps_the_native_selection_when_an_owned_member_bounds_the_other_side() {
    let (profile, losses, _) = bind_cut(false, 4);
    assert_native(profile, &losses);
}
