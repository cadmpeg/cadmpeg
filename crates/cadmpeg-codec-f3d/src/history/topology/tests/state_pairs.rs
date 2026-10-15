// SPDX-License-Identifier: Apache-2.0
//! History-module unit tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use crate::history::topology::bodies_intersecting;

use crate::history::topology::edge_recipe_reference_context;

use crate::history::topology::historical_edge_axis;
use crate::history::topology::historical_face_support_contexts;
use crate::history::topology::historical_loop_boundary;
use crate::history::topology::historical_topology;
use crate::history::topology::preceding_support_face_slots;

use crate::history::selection::faces_in_topology;
use crate::history::selection::historical_edge_context;

use crate::history::selection::incident_loop_counts_satisfy_sides;
use crate::history::selection::recipe_selector_candidates;

use crate::history::topology::EdgeBoundaryContext;
use crate::history_records::AsmDeltaState;
use crate::history_records::AsmHistoricalEdge;
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCurveAxis, AsmHistoricalOptionalCarrierBinding,
};

use crate::history_records::AsmHistoricalTopology;

use crate::history_records::AsmHistory;

use std::collections::{BTreeSet, HashSet};

#[test]
fn historical_edge_axis_uses_the_state_specific_curve_carrier() {
    let topology = AsmHistoricalTopology {
        edge_curves: vec![AsmHistoricalOptionalCarrierBinding {
            entity: 7,
            carrier: Some(27),
        }],
        curve_axes: vec![AsmHistoricalCurveAxis {
            curve: 27,
            origin: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            direction: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        }],
        ..AsmHistoricalTopology::default()
    };
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| historical_edge_axis(
            decode_ctx, 7, &topology
        ))
        .unwrap(),
        Some((
            cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        ))
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| historical_edge_axis(
            decode_ctx, 8, &topology
        ))
        .unwrap(),
        None
    );
}

#[test]
fn historical_edge_axis_uses_a_unique_incident_surface_axis() {
    let surface_axis =
        |surface, origin, direction| crate::history_records::AsmHistoricalSurfaceAxis {
            surface,
            origin,
            direction,
        };
    let mut topology = AsmHistoricalTopology {
        face_loops: vec![crate::history_records::AsmHistoricalRelation {
            owner_ref: 11,
            member_refs: vec![21],
        }],
        loop_coedges: vec![crate::history_records::AsmHistoricalRelation {
            owner_ref: 21,
            member_refs: vec![31],
        }],
        coedge_topology: vec![crate::history_records::AsmHistoricalCoedge {
            coedge: 31,
            owner_loop: 21,
            edge: 7,
            next: 31,
            previous: 31,
            radial_next: 31,
        }],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 11,
            carrier: 41,
        }],
        surface_axes: vec![surface_axis(
            41,
            cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        )],
        ..AsmHistoricalTopology::default()
    };
    let expected = Some((
        cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
        cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
    ));
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| historical_edge_axis(
            decode_ctx, 7, &topology
        ))
        .unwrap(),
        expected
    );

    topology.face_surfaces.push(AsmHistoricalCarrierBinding {
        entity: 11,
        carrier: 42,
    });
    topology.surface_axes.push(surface_axis(
        42,
        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
    ));
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| historical_edge_axis(
            decode_ctx, 7, &topology
        ))
        .unwrap(),
        None
    );

    topology.face_surfaces.pop();
    topology
        .face_loops
        .push(crate::history_records::AsmHistoricalRelation {
            owner_ref: 12,
            member_refs: vec![22],
        });
    topology
        .loop_coedges
        .push(crate::history_records::AsmHistoricalRelation {
            owner_ref: 22,
            member_refs: vec![32],
        });
    topology
        .coedge_topology
        .push(crate::history_records::AsmHistoricalCoedge {
            coedge: 32,
            owner_loop: 22,
            edge: 7,
            next: 32,
            previous: 32,
            radial_next: 32,
        });
    topology.face_surfaces.push(AsmHistoricalCarrierBinding {
        entity: 12,
        carrier: 42,
    });
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| historical_edge_axis(
            decode_ctx, 7, &topology
        ))
        .unwrap(),
        None
    );
}

#[test]
fn result_face_support_maps_only_to_one_preceding_owner() {
    use cadmpeg_ir::ids::FaceId;

    let result_faces = [FaceId::mint("f3d:brep:entity#40").expect("identity grammar")];
    let result = AsmHistoricalTopology {
        faces: vec![40],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 40,
            carrier: 20,
        }],
        ..AsmHistoricalTopology::default()
    };
    let preceding = AsmHistoricalTopology {
        faces: vec![4, 5],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 4,
                carrier: 20,
            },
            AsmHistoricalCarrierBinding {
                entity: 5,
                carrier: 21,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| preceding_support_face_slots(
            decode_ctx,
            &result_faces,
            &result,
            &preceding
        ))
        .unwrap(),
        [4]
    );

    let mut ambiguous = preceding.clone();
    ambiguous.face_surfaces[1].carrier = 20;
    assert!(
        crate::test_support::with_decode_context(|decode_ctx| preceding_support_face_slots(
            decode_ctx,
            &result_faces,
            &result,
            &ambiguous
        ))
        .unwrap()
        .is_empty()
    );

    let mut ambiguous_result = result.clone();
    ambiguous_result
        .face_surfaces
        .push(AsmHistoricalCarrierBinding {
            entity: 40,
            carrier: 21,
        });
    assert!(
        crate::test_support::with_decode_context(|decode_ctx| preceding_support_face_slots(
            decode_ctx,
            &result_faces,
            &ambiguous_result,
            &preceding
        ))
        .unwrap()
        .is_empty()
    );
}

#[test]
fn active_face_support_retains_invariant_preceding_owners() {
    use cadmpeg_ir::ids::FaceId;

    let state = |state_id, topology| AsmDeltaState {
        id: format!("state-{state_id}"),
        parent: "history".into(),
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
        entity_versions: Vec::new(),
        topology_cache: crate::history_records::AsmTopologyCache::Complete(topology),
        transition: None,
    };
    let active = AsmHistoricalTopology {
        faces: vec![40],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 40,
            carrier: 20,
        }],
        ..AsmHistoricalTopology::default()
    };
    let history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![state(2, active.clone()), state(3, active)],
    };
    let preceding = AsmHistoricalTopology {
        faces: vec![4, 5],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 4,
                carrier: 20,
            },
            AsmHistoricalCarrierBinding {
                entity: 5,
                carrier: 20,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    let changed_faces = HashSet::from([5]);
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| historical_face_support_contexts(
            decode_ctx,
            &[FaceId::mint("f3d:brep:entity#40").expect("identity grammar")],
            &history,
            &preceding,
            &changed_faces
        ))
        .unwrap(),
        [
            crate::records::topology::historical_context::DesignHistoricalFaceSupportContext {
                active_face_slot: 40,
                surface_slot: 20,
                preceding_face_slots: vec![4, 5],
                preceding_face_boundaries: Vec::new(),
                changed_preceding_face_slots: vec![5],
            }
        ]
    );

    let mut variant = history;
    variant.states[1].topology_mut().unwrap().face_surfaces[0].carrier = 21;
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| historical_face_support_contexts(
            decode_ctx,
            &[FaceId::mint("f3d:brep:entity#4").expect("identity grammar")],
            &variant,
            &preceding,
            &changed_faces
        ))
        .unwrap(),
        [
            crate::records::topology::historical_context::DesignHistoricalFaceSupportContext {
                active_face_slot: 4,
                surface_slot: 20,
                preceding_face_slots: vec![4, 5],
                preceding_face_boundaries: Vec::new(),
                changed_preceding_face_slots: vec![5],
            }
        ]
    );
    assert!(crate::test_support::with_decode_context(|decode_ctx| {
        historical_face_support_contexts(
            decode_ctx,
            &[FaceId::mint("f3d:brep:entity#40").expect("identity grammar")],
            &variant,
            &preceding,
            &changed_faces,
        )
    })
    .unwrap()
    .is_empty());
}

#[test]
fn historical_topology_retains_ordered_ownership_and_incidence() {
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
        VertexId,
    };
    use cadmpeg_ir::topology::{
        Body, BodyKind, Coedge, Edge, Face, Loop, Region, Sense, Shell, Vertex,
    };

    let id = |slot| format!("f3d:brep:entity#{slot}");
    let mut brep = cadmpeg_asm::brep::AsmBrep::default();
    brep.bodies.push(Body {
        id: BodyId::mint(id(1)).expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: vec![RegionId::mint(id(2)).expect("identity grammar")],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    brep.regions.push(Region {
        id: RegionId::mint(id(2)).expect("identity grammar"),
        body: BodyId::mint(id(1)).expect("identity grammar"),
        shells: vec![ShellId::mint(id(3)).expect("identity grammar")],
    });
    brep.shells.push(Shell::with_face(
        ShellId::mint(id(3)).expect("identity grammar"),
        RegionId::mint(id(2)).expect("identity grammar"),
        FaceId::mint(id(4)).expect("identity grammar"),
    ));
    brep.faces.push(Face {
        id: FaceId::mint(id(4)).expect("identity grammar"),
        shell: ShellId::mint(id(3)).expect("identity grammar"),
        surface: SurfaceId::mint(id(20)).expect("identity grammar"),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![
            LoopId::mint(id(5)).expect("identity grammar")
        ]),
        name: None,
        color: None,
        tolerance: None,
    });
    brep.loops.push(Loop {
        id: LoopId::mint(id(5)).expect("identity grammar"),
        face: FaceId::mint(id(4)).expect("identity grammar"),
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
            cadmpeg_ir::topology::LoopRing::new(
                &cadmpeg_test_support::service_decode_context(),
                vec![CoedgeId::mint(id(6)).expect("identity grammar")],
                Vec::new(),
            )
            .expect("fixture ring admission")
            .expect("valid loop ring"),
        ),
    });
    brep.coedges.push(Coedge {
        id: CoedgeId::mint(id(6)).expect("identity grammar"),
        owner_loop: LoopId::mint(id(5)).expect("identity grammar"),
        edge: EdgeId::mint(id(7)).expect("identity grammar"),
        radial_next: CoedgeId::mint(id(6)).expect("identity grammar"),
        sense: Sense::Forward,
        pcurves: Vec::new(),
        use_curve: None,
    });
    brep.edges.push(Edge {
        id: EdgeId::mint(id(7)).expect("identity grammar"),
        carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(
            CurveId::mint(id(21)).expect("identity grammar"),
        )),
        start: VertexId::mint(id(8)).expect("identity grammar"),
        end: VertexId::mint(id(9)).expect("identity grammar"),
        tolerance: None,
    });
    for slot in [8, 9] {
        brep.vertices.push(Vertex {
            id: VertexId::mint(id(slot)).expect("identity grammar"),
            point: PointId::mint(id(slot + 20)).expect("identity grammar"),
            tolerance: None,
        });
    }

    let ctx = cadmpeg_test_support::service_decode_context();
    let topology = historical_topology(&ctx, &brep)
        .expect("topology budget")
        .expect("stable historical topology");
    assert_eq!(topology.body_regions[0].member_refs, [2]);
    assert_eq!(topology.region_shells[0].member_refs, [3]);
    assert_eq!(topology.shell_faces[0].member_refs, [4]);
    assert_eq!(topology.face_loops[0].member_refs, [5]);
    assert_eq!(topology.loop_coedges[0].member_refs, [6]);
    assert_eq!(topology.coedge_topology[0].edge, 7);
    assert_eq!(topology.coedge_topology[0].radial_next, 6);
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| historical_edge_context(
            decode_ctx, 7, &topology
        ))
        .unwrap(),
        crate::records::topology::historical_context::DesignHistoricalEdgeContext {
            edge_slot: 7,
            incident_loops: vec![
                crate::records::topology::historical_context::DesignHistoricalEdgeLoopContext {
                    coedge_slot: 6,
                    loop_slot: 5,
                    face_slot: 4,
                    boundary_edge_count: 1,
                    coedge_ordinal: 0,
                    previous_edge_slot: 7,
                    next_edge_slot: 7,
                }
            ],
        }
    );
    let entry = |selector, boundary_edge_count| {
        crate::records::topology::edge_recipe::DesignTopologyRecipeEntry {
            selector,
            boundary_edge_count: std::num::NonZeroU32::new(boundary_edge_count).unwrap(),
            topology_triplets: [
                crate::records::topology::edge_recipe::DesignTopologyRecipeTriplet {
                    outer: std::num::NonZeroU32::new(1).unwrap(),
                    middle: 0,
                    incident: Some(crate::records::topology::edge_recipe::DesignTopologyIncident {
                        ordinal: boundary_edge_count - 1,
                        side: crate::records::topology::edge_recipe::DesignTopologyIncidentSide::Preceding,
                    }),
                },
                crate::records::topology::edge_recipe::DesignTopologyRecipeTriplet {
                    outer: std::num::NonZeroU32::new(1).unwrap(),
                    middle: 1,
                    incident: Some(crate::records::topology::edge_recipe::DesignTopologyIncident {
                        ordinal: 0,
                        side: crate::records::topology::edge_recipe::DesignTopologyIncidentSide::Following,
                    }),
                },
            ],
        }
    };
    let side = |entries: Vec<crate::records::topology::edge_recipe::DesignTopologyRecipeEntry>| {
        crate::records::topology::edge_recipe::DesignTopologyRecipeSide {
            header_value: 0,
            scalars: vec![0, 0],
            payload_prefix: vec![0],

            entries,
        }
    };
    let structure = crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
        root: 2,
        sides: vec![
            side(vec![entry(1, 1), entry(2, 1)]),
            side(vec![entry(1, 2)]),
        ],
    };
    let loop_context = |coedge_slot, boundary_edge_count| {
        crate::records::topology::historical_context::DesignHistoricalEdgeLoopContext {
            coedge_slot,
            loop_slot: coedge_slot + 10,
            face_slot: coedge_slot + 20,
            boundary_edge_count,
            coedge_ordinal: 0,
            previous_edge_slot: coedge_slot + 30,
            next_edge_slot: coedge_slot + 40,
        }
    };
    let contexts = [
        crate::records::topology::historical_context::DesignHistoricalEdgeContext {
            edge_slot: 7,
            incident_loops: vec![loop_context(70, 1)],
        },
        crate::records::topology::historical_context::DesignHistoricalEdgeContext {
            edge_slot: 8,
            incident_loops: vec![
                loop_context(80, 1),
                loop_context(81, 2),
                crate::records::topology::historical_context::DesignHistoricalEdgeLoopContext {
                    coedge_ordinal: 1,
                    ..loop_context(82, 2)
                },
            ],
        },
    ];
    let selectors = crate::test_support::with_decode_context(|decode_ctx| {
        recipe_selector_candidates(decode_ctx, Some(&structure), &contexts)
    })
    .unwrap();
    assert_eq!(selectors.len(), 2);
    assert_eq!(selectors[0].selector, 1);
    assert_eq!(selectors[0].boundary_count_matching_edge_slots, [8]);
    assert_eq!(
        selectors[0]
            .clauses
            .iter()
            .map(|clause| clause
                .as_ref()
                .map(|clause| clause.triplet_edge_slots.clone()))
            .collect::<Vec<_>>(),
        [Some([vec![7, 8], vec![7, 8]]), Some([vec![8], vec![8]])]
    );
    assert_eq!(selectors[0].incidence_matching_edge_slots, [8]);
    assert_eq!(selectors[0].unique_incidence_edge_slot(), Some(8));
    assert_eq!(selectors[1].selector, 2);
    assert_eq!(selectors[1].boundary_count_matching_edge_slots, [7, 8]);
    assert_eq!(selectors[1].incidence_matching_edge_slots, [7, 8]);
    assert_eq!(selectors[1].unique_incidence_edge_slot(), None);
    assert_eq!(
        selectors[1]
            .clauses
            .iter()
            .map(|clause| clause
                .as_ref()
                .map(|clause| clause.triplet_edge_slots.clone()))
            .collect::<Vec<_>>(),
        [Some([vec![7, 8], vec![7, 8]]), None]
    );
    crate::test_support::with_decode_context(|decode_ctx| {
        assert!(
            incident_loop_counts_satisfy_sides(decode_ctx, &[4, 5], &[Some(5), Some(4)]).unwrap()
        );
        assert!(
            !incident_loop_counts_satisfy_sides(decode_ctx, &[5, 6], &[Some(5), Some(5)]).unwrap()
        );
        assert!(
            incident_loop_counts_satisfy_sides(decode_ctx, &[5, 5], &[Some(5), Some(5)]).unwrap()
        );
        assert!(incident_loop_counts_satisfy_sides(decode_ctx, &[5], &[None, Some(5)]).unwrap());
    });
    assert_eq!(topology.edge_vertices[0].start_vertex, 8);
    assert_eq!(topology.edge_vertices[0].end_vertex, 9);
    assert_eq!(topology.face_surfaces[0].carrier, 20);
    assert_eq!(topology.edge_curves[0].carrier, Some(21));
    assert_eq!(topology.coedge_pcurves[0].carrier, None);
    assert_eq!(topology.vertex_points[0].carrier, 28);
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| bodies_intersecting(
            decode_ctx,
            &topology,
            &BTreeSet::from([20])
        ))
        .unwrap()
        .unwrap(),
        BTreeSet::from([1])
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| bodies_intersecting(
            decode_ctx,
            &topology,
            &BTreeSet::from([28])
        ))
        .unwrap()
        .unwrap(),
        BTreeSet::from([1])
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| faces_in_topology(
            decode_ctx,
            &[
                FaceId::mint(id(4)).expect("identity grammar"),
                FaceId::mint(id(99)).expect("identity grammar"),
                FaceId::mint("test:model:face#foreign").expect("identity grammar")
            ],
            &topology
        ))
        .unwrap(),
        [FaceId::mint(id(4)).expect("identity grammar")]
    );
    let mut reference = crate::records::dimensions::DesignRecipeReference {
        selector: 1,
        selector_offset: 0,
        token: "1".into(),
        token_offset: 0,
        design_reference: 1,
        design_reference_offset: 1,
        candidate_faces: vec![FaceId::mint(id(4)).expect("identity grammar")],
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    };
    let context = crate::test_support::with_decode_context(|decode_ctx| {
        edge_recipe_reference_context(
            decode_ctx,
            2,
            &reference,
            EdgeBoundaryContext {
                topology: &topology,
                boundary_edges: &[7, 99],
            },
            EdgeBoundaryContext {
                topology: &topology,
                boundary_edges: &[7, 98],
            },
            &HashSet::from([7]),
        )
    })
    .unwrap();
    assert_eq!(context.reference_ordinal, 2);
    assert_eq!(
        context.result_faces,
        [FaceId::mint(id(4)).expect("identity grammar")]
    );
    let boundary = crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext {
        face_slot: 4,
        loops: vec![crate::records::topology::historical_context::DesignHistoricalFaceLoopContext {
            loop_slot: 5,
            boundary: crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Coedges(vec![
                crate::records::topology::historical_context::DesignHistoricalLoopCoedge {
                    coedge_slot: 6,
                    edge_slot: 7,
                },
            ]),
        }],
    };
    assert_eq!(context.result_face_boundaries, [boundary.clone()]);
    assert_eq!(context.result_shared_edge_slots, [7]);
    assert_eq!(
        context.preceding_faces,
        [FaceId::mint(id(4)).expect("identity grammar")]
    );
    assert_eq!(context.preceding_face_boundaries, [boundary]);
    assert_eq!(context.preceding_support_face_slots, [4]);
    assert_eq!(context.preceding_support_face_boundaries.len(), 1);
    assert_eq!(context.shared_edge_slots, [7]);
    assert_eq!(context.changed_shared_edge_slots, [7]);
    assert_eq!(context.changed_reference_edge_slots, [7]);
    reference.candidate_faces.clear();
    reference.alternate_selector_faces = vec![FaceId::mint(id(4)).expect("identity grammar")];
    let alternate_context = crate::test_support::with_decode_context(|decode_ctx| {
        edge_recipe_reference_context(
            decode_ctx,
            2,
            &reference,
            EdgeBoundaryContext {
                topology: &topology,
                boundary_edges: &[7, 99],
            },
            EdgeBoundaryContext {
                topology: &topology,
                boundary_edges: &[7, 98],
            },
            &HashSet::from([7]),
        )
    })
    .unwrap();
    assert_eq!(
        alternate_context.result_faces,
        [FaceId::mint(id(4)).expect("identity grammar")]
    );
    assert_eq!(
        alternate_context.preceding_faces,
        [FaceId::mint(id(4)).expect("identity grammar")]
    );
    assert_eq!(alternate_context.changed_reference_edge_slots, [7]);
    let support_only_context = crate::test_support::with_decode_context(|decode_ctx| {
        edge_recipe_reference_context(
            decode_ctx,
            2,
            &reference,
            EdgeBoundaryContext {
                topology: &topology,
                boundary_edges: &[99],
            },
            EdgeBoundaryContext {
                topology: &topology,
                boundary_edges: &[98],
            },
            &HashSet::from([7]),
        )
    })
    .unwrap();
    assert!(support_only_context.shared_edge_slots.is_empty());
    assert_eq!(support_only_context.changed_reference_edge_slots, [7]);
    let cyclic = AsmHistoricalTopology {
        edge_vertices: vec![
            AsmHistoricalEdge {
                edge: 7,
                start_vertex: 1,
                end_vertex: 2,
            },
            AsmHistoricalEdge {
                edge: 8,
                start_vertex: 3,
                end_vertex: 2,
            },
            AsmHistoricalEdge {
                edge: 9,
                start_vertex: 1,
                end_vertex: 3,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    let ordered_vertices = |edges: &[i64], topology: &AsmHistoricalTopology| {
        let coedges = edges
            .iter()
            .enumerate()
            .map(|(ordinal, edge_slot)| {
                crate::records::topology::historical_context::DesignHistoricalLoopCoedge {
                    coedge_slot: i64::try_from(ordinal).expect("fixture value fits i64"),
                    edge_slot: *edge_slot,
                }
            })
            .collect();
        match crate::test_support::with_decode_context(|decode_ctx| historical_loop_boundary(decode_ctx, coedges, topology)).unwrap() {
            crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Vertices(rows) => Some(
                rows.into_iter()
                    .map(|row| row.vertex_slot)
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        }
    };
    assert_eq!(ordered_vertices(&[7, 8, 9], &cyclic), Some(vec![1, 2, 3]));
    let disconnected = AsmHistoricalTopology {
        edge_vertices: vec![
            AsmHistoricalEdge {
                edge: 7,
                start_vertex: 1,
                end_vertex: 2,
            },
            AsmHistoricalEdge {
                edge: 8,
                start_vertex: 3,
                end_vertex: 4,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    assert_eq!(ordered_vertices(&[7, 8], &disconnected), None);
}
