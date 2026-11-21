// SPDX-License-Identifier: Apache-2.0

use super::{bind_chamfer_groups, group_edges, transition_bevels, Bevel};
use crate::history::fillet_blends::GroupBinding;
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCurveAxis,
    AsmHistoricalEdgeSample, AsmHistoricalEntityDelta, AsmHistoricalOptionalCarrierBinding,
    AsmHistoricalPlane, AsmHistoricalRelation, AsmHistoricalRowsDelta, AsmHistoricalTopology,
    AsmHistoricalTopologyDelta, AsmHistoricalTransition,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use cadmpeg_ir::features::edge_treatments::{ChamferGroup, ChamferSpec};
use cadmpeg_ir::features::selections::EdgeSelection;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::PositiveLength;

const SCOPE_ID: &str = "f3d:Design/BulkStream.dat:scope#42";
const GROUP_ID: &str = "f3d:Design/BulkStream.dat:group#43";

fn x() -> Vector3 {
    Vector3::new(1.0, 0.0, 0.0)
}

/// Faces bounded by one loop each, with one coedge per boundary edge.
fn faces(boundaries: &[(i64, &[i64])]) -> AsmHistoricalTopology {
    let mut topology = AsmHistoricalTopology::default();
    for &(face, edges) in boundaries {
        let loop_slot = 1_000 + face;
        topology.faces.push(face);
        topology.surfaces.push(100 + face);
        topology.face_surfaces.push(AsmHistoricalCarrierBinding {
            entity: face,
            carrier: 100 + face,
        });
        topology.face_loops.push(AsmHistoricalRelation {
            owner_ref: face,
            member_refs: vec![loop_slot],
        });
        let coedges = edges
            .iter()
            .map(|edge| 10_000 * face + edge)
            .collect::<Vec<_>>();
        topology.loop_coedges.push(AsmHistoricalRelation {
            owner_ref: loop_slot,
            member_refs: coedges.clone(),
        });
        for (coedge, edge) in coedges.into_iter().zip(edges) {
            topology.coedge_topology.push(AsmHistoricalCoedge {
                coedge,
                owner_loop: loop_slot,
                edge: *edge,
                next: coedge,
                previous: coedge,
                radial_next: coedge,
            });
        }
    }
    topology
}

/// A straight edge through `at`, with its axis row and midpoint tangent along
/// `direction`.
fn line_at(topology: &mut AsmHistoricalTopology, edge: i64, at: Point3, direction: Vector3) {
    topology.edges.push(edge);
    topology
        .edge_curves
        .push(AsmHistoricalOptionalCarrierBinding {
            entity: edge,
            carrier: Some(100 + edge),
        });
    topology.curve_axes.push(AsmHistoricalCurveAxis {
        curve: 100 + edge,
        origin: at,
        direction,
    });
    topology.edge_samples.push(AsmHistoricalEdgeSample {
        bounds: None,
        edge,
        interior: at,
        tangent: Some(direction),
        carrier: None,
    });
}

fn plane(topology: &mut AsmHistoricalTopology, surface: i64, origin: Point3, normal: Vector3) {
    topology.surface_planes.push(AsmHistoricalPlane {
        surface,
        origin,
        normal,
    });
}

/// Face 10 (plane y = 0) and face 11 (plane z = 0) meet at edge 17 along x
/// through the origin; face 12 (plane x = 0) caps both at edges 18 and 19.
fn input() -> AsmHistoricalTopology {
    let mut topology = faces(&[(10, &[17, 18]), (11, &[17, 19]), (12, &[18, 19])]);
    line_at(&mut topology, 17, Point3::new(2.0, 0.0, 0.0), x());
    line_at(
        &mut topology,
        18,
        Point3::new(0.0, 0.0, 1.0),
        Vector3::new(0.0, 0.0, 1.0),
    );
    line_at(
        &mut topology,
        19,
        Point3::new(0.0, 1.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    );
    plane(
        &mut topology,
        110,
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    );
    plane(
        &mut topology,
        111,
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    );
    plane(
        &mut topology,
        112,
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
    );
    topology
}

/// The chamfer of edge 17 at `first` along face 10 and `second` along face
/// 11: bevel face 20 on the plane through (x, 0, first) and (x, second, 0)
/// meets faces 10, 11, and 12.
fn output(first: f64, second: f64) -> AsmHistoricalTopology {
    let mut topology = faces(&[(10, &[30]), (11, &[31]), (12, &[32]), (20, &[30, 31, 32])]);
    let normal = Vector3::new(0.0, first, second);
    plane(&mut topology, 120, Point3::new(0.0, second, 0.0), normal);
    plane(
        &mut topology,
        110,
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    );
    topology
}

fn transition(inserted: &[i64], deleted_edges: &[i64]) -> AsmHistoricalTransition {
    AsmHistoricalTransition {
        geometry_unchanged: false,
        previous_state_id: Some(1),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta {
            faces: AsmHistoricalEntityDelta {
                inserted: inserted.to_vec(),
                ..AsmHistoricalEntityDelta::default()
            },
            edges: AsmHistoricalEntityDelta {
                deleted: deleted_edges.to_vec(),
                ..AsmHistoricalEntityDelta::default()
            },
            ..AsmHistoricalTopologyDelta::default()
        },
        record_revisions: Vec::new(),
        rows: AsmHistoricalRowsDelta::default(),
    }
}

fn bevels(output: &AsmHistoricalTopology, deleted: &[i64]) -> Option<Vec<Bevel>> {
    transition_bevels(&transition(&[20], deleted), &input(), output)
}

#[test]
fn a_bevel_replaces_the_deleted_edge_along_its_plane() {
    // The plane y + z = 0.5 cuts each face 0.5 from edge 17.
    assert_eq!(
        bevels(&output(0.5, 0.5), &[17, 18, 19]),
        Some(vec![Bevel {
            setbacks: [0.5, 0.5],
            edge: 17
        }])
    );
}

#[test]
fn the_setbacks_follow_the_plane_across_each_support() {
    // The plane y + 2z = 1 cuts face 10 (y = 0) 0.5 from the edge along z and
    // face 11 (z = 0) 1 from it along y.
    let found = bevels(&output(0.5, 1.0), &[17, 18, 19]).expect("a bevel");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].edge, 17);
    for (have, want) in found[0].setbacks.iter().zip([0.5, 1.0]) {
        assert!((have - want).abs() < 1.0e-12, "{have} != {want}");
    }
}

#[test]
fn a_bevel_needs_its_edge_deleted() {
    assert_eq!(bevels(&output(0.5, 0.5), &[18, 19]), None);
}

#[test]
fn a_bevel_on_a_carrier_that_is_not_a_plane_replaces_no_edge() {
    let mut curved = output(0.5, 0.5);
    curved
        .surface_radii
        .push(crate::history_records::AsmHistoricalSurfaceRadius {
            surface: 120,
            radius: 2.0,
        });
    assert_eq!(bevels(&curved, &[17, 18, 19]), None);
}

#[test]
fn a_bevel_across_a_curved_support_replaces_no_edge() {
    let mut topology = input();
    topology
        .surface_radii
        .push(crate::history_records::AsmHistoricalSurfaceRadius {
            surface: 110,
            radius: 2.0,
        });
    assert_eq!(
        transition_bevels(
            &transition(&[20], &[17, 18, 19]),
            &topology,
            &output(0.5, 0.5)
        ),
        None
    );
}

fn distance(value: f64) -> ChamferSpec {
    ChamferSpec::Distance {
        distance: PositiveLength::new(value).unwrap(),
    }
}

#[test]
fn bevels_assign_to_the_group_with_their_setbacks() {
    let bevel = |a, b, edge| Bevel {
        setbacks: [a, b],
        edge,
    };
    let specs = [Some([0.5, 0.5]), Some([0.5, 1.0]), Some([2.0, 2.0]), None];
    let found = [bevel(0.5, 0.5, 4), bevel(0.5, 1.0, 3), bevel(0.5, 0.5, 2)];
    assert_eq!(
        group_edges(&specs, &found),
        Some(vec![vec![2, 4], vec![3], vec![], vec![]])
    );
    assert_eq!(group_edges(&specs, &[bevel(0.7, 0.7, 5)]), None);
    assert_eq!(
        group_edges(&[Some([0.5, 0.5]), Some([0.5, 0.5])], &found[..1]),
        None,
        "a bevel that two groups match"
    );
}

fn scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(SCOPE_ID, DesignFeatureKind::Chamfer, 42);
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
            draft.previous_history_state_id = Some(1);
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn group() -> DesignConstructionOperandGroup {
    serde_json::from_value(serde_json::json!({
        "id": GROUP_ID,
        "byte_offset": 0,
        "class_tag": "325",
        "record_index": 43,
        "scope_record_index": 42,
        "scope_reference_ordinal": 0,
        "members": [7],
        "member_offsets": [0],
        "role": 34_359_738_368_u64,
        "role_offset": 0,
        "paired_byte_offset": 200,
        "paired_class_tag": "258",
        "frame": {
            "member_count_offset": 0,
            "opaque_index": 1,
            "opaque_index_offset": 18,
            "opaque_scalar": 0.0,
            "opaque_scalar_offset": 22,
            "trailing_flags": [],
            "trailing_record_indices": [],
            "trailing_record_offsets": [],
            "variant": false
        }
    }))
    .unwrap()
}

fn chamfer(edges: EdgeSelection, spec: ChamferSpec) -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::Chamfer {
        groups: cadmpeg_ir::features::selections::NonEmptyMembers::one(ChamferGroup {
            edges,
            spec,
        }),
        flip_direction: false,
    })
}

fn bound(
    edges: EdgeSelection,
    spec: ChamferSpec,
    output: &AsmHistoricalTopology,
) -> (GroupBinding, EdgeSelection) {
    let mut definition = chamfer(edges, spec);
    let binding = bind_chamfer_groups(
        &mut definition,
        &FeatureId::mint("f3d:test:feature#42").expect("identity grammar"),
        &scope(),
        &[group()],
        1,
        (&transition(&[20], &[17, 18, 19]), &input(), output),
    );
    let FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) = definition else {
        panic!("expected a chamfer");
    };
    (binding, groups[0].edges.clone())
}

fn slots(selection: &EdgeSelection) -> Vec<String> {
    let EdgeSelection::Historical { edges, .. } = selection else {
        panic!("expected a historical selection, found {selection:?}");
    };
    edges
        .iter()
        .map(|edge| edge.as_str().rsplit(':').next().unwrap().to_owned())
        .collect()
}

#[test]
fn a_group_binds_to_the_edge_its_bevel_replaces() {
    let native = EdgeSelection::Native(GROUP_ID.into());
    let (binding, selection) = bound(native, distance(0.5), &output(0.5, 0.5));
    assert_eq!(binding, GroupBinding::Bound);
    assert_eq!(slots(&selection), ["17"]);
}

#[test]
fn a_group_whose_distance_the_bevel_does_not_cut_stays_native() {
    let native = EdgeSelection::Native(GROUP_ID.into());
    let (binding, selection) = bound(native.clone(), distance(0.6), &output(0.5, 0.5));
    assert_eq!(binding, GroupBinding::Unbound);
    assert_eq!(selection, native);
}
