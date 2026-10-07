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

mod limits;

use crate::history::bind_scope_histories;

use crate::history::bound_history_state_pair;
use crate::history::bound_scope_history;
use crate::history::edge_changes_across_state_chain;

use crate::history::effective_scope_previous_history_state_id;
use crate::history::face_changes_across_state_chain;

use crate::history::resolve_pattern_face_by_surface_radius;
use crate::history::selection::boundary_edges_in_changes;

use crate::history::selection::historical_identity_edge;
use crate::history::selection::historical_pattern_identity_axes;
use crate::history::selection::historical_pattern_identity_axes_for_selection;
use crate::history::selection::incident_loop_counts_satisfy_sides;

use crate::history::selection::snapshot_edge_identity_revision;
use crate::history::selection::HistoricalIdentityIndex;
use crate::history::terminal_edge_recipe_faces;
use crate::history::terminal_edge_recipe_reference_faces;
use crate::history::unique_history_state;
use crate::history::unique_history_state_pair;

use crate::history_records::AsmDeltaState;

use crate::history_records::AsmHistoricalEntityDelta;
use crate::history_records::AsmHistoricalTopology;
use crate::history_records::AsmHistoricalTopologyDelta;
use crate::history_records::AsmHistoricalTransition;
use crate::history_records::AsmHistory;
use crate::history_records::AsmHistoryRecord;
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use std::collections::{HashMap, HashSet};

fn scoped_history_with_limits(
    max_items: u64,
    max_retained_bytes: u64,
) -> Result<HashMap<String, String>, cadmpeg_core::CodecError> {
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:native:scope#1",
        crate::records::feature::scope::DesignFeatureKind::BaseFlange,
        1,
    );
    scope
        .try_edit(|draft| draft.history_state_id = Some(1))
        .unwrap();
    let history = AsmHistory {
        id: "f3d:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![AsmDeltaState {
            id: "f3d:history:state#1".into(),
            parent: "f3d:history".into(),
            byte_offset: 0,
            state_id: 1,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 1,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            entity_versions: Vec::new(),
            topology_cache: crate::history_records::AsmTopologyCache::Complete(
                AsmHistoricalTopology::default(),
            ),
            transition: None,
        }],
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = max_retained_bytes;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    bind_scope_histories(&ctx, &[scope], &[], &[], &[history])
}

#[test]
fn scope_history_candidate_refuses_collection_limit() {
    let error = scoped_history_with_limits(0, u64::MAX).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D matching scope histories")
    );
}

#[test]
fn scope_history_binding_refuses_collection_limit() {
    let error = scoped_history_with_limits(2, u64::MAX).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D scope history bindings")
    );
}

#[test]
fn scope_history_identity_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D bound scope identity",
        |cap| scoped_history_with_limits(u64::MAX, cap).map(|_| ()),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D bound scope identity")
    );
}

#[test]
fn state_pairs_are_resolved_within_one_reachable_history() {
    let state = |history: &str, state_id: i64, previous_state_id: Option<i64>| AsmDeltaState {
        id: format!("{history}:state#{state_id}"),
        parent: history.into(),
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
        topology_cache: crate::history_records::AsmTopologyCache::Complete(
            AsmHistoricalTopology::default(),
        ),
        transition: previous_state_id.map(|previous_state_id| {
            crate::history_records::AsmHistoricalTransition {
                previous_state_id: Some(previous_state_id),
                records: Default::default(),
                topology: Default::default(),
            }
        }),
    };
    let history = |id: &str, current| AsmHistory {
        id: id.into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![state(id, current, Some(2)), state(id, 2, None)],
    };
    let histories = [history("first", 7), history("second", 9)];
    let (resolved, current, previous) = crate::test_support::with_decode_context(|decode_ctx| {
        unique_history_state_pair(decode_ctx, &histories, 9, 2)
    })
    .unwrap()
    .expect("state-local pair");
    assert_eq!(resolved.id, "second");
    assert_eq!(current.state_id, 9);
    assert_eq!(previous.state_id, 2);
    assert!(crate::test_support::with_decode_context(|decode_ctx| {
        unique_history_state(decode_ctx, &histories, 2)
    })
    .unwrap()
    .is_none());

    let duplicate_pair = [history("first", 9), history("second", 9)];
    assert!(crate::test_support::with_decode_context(|decode_ctx| {
        unique_history_state_pair(decode_ctx, &duplicate_pair, 9, 2)
    })
    .unwrap()
    .is_none());

    let indirect = AsmHistory {
        id: "indirect".into(),
        states: vec![
            state("indirect", 23, Some(21)),
            state("indirect", 21, Some(11)),
            state("indirect", 11, None),
        ],
        ..history("indirect", 23)
    };
    let direct = AsmHistory {
        id: "direct".into(),
        states: vec![state("direct", 23, Some(11)), state("direct", 11, None)],
        ..history("direct", 23)
    };
    let histories = [indirect, direct];
    let (resolved, _, _) = crate::test_support::with_decode_context(|decode_ctx| {
        unique_history_state_pair(decode_ctx, &histories, 23, 11)
    })
    .unwrap()
    .expect("direct transition takes precedence over a reachable pair");
    assert_eq!(resolved.id, "direct");
}

#[test]
fn ambiguous_scope_histories_use_exact_result_body_sources() {
    use crate::records::{
        bodies::DesignBodyBinding,
        topology::{
            body_recipe::DesignBodyRecipeOperand, body_recipe::DesignBodyRecipeReference,
            body_recipe::DesignOperandOwner,
        },
    };
    use cadmpeg_ir::ids::FaceId;

    let state = |history: &str, state_id: i64, previous_state_id: Option<i64>| AsmDeltaState {
        id: format!("{history}:asm-delta-state#{state_id}"),
        parent: history.into(),
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
        topology_cache: crate::history_records::AsmTopologyCache::Complete(
            AsmHistoricalTopology::default(),
        ),
        transition: previous_state_id.map(|previous_state_id| {
            crate::history_records::AsmHistoricalTransition {
                previous_state_id: Some(previous_state_id),
                records: Default::default(),
                topology: Default::default(),
            }
        }),
    };
    let history = |source: &str| {
        let id = format!("f3d:asset/Breps.BlobParts/BREP.{source}.smbh:asm-history#1");
        AsmHistory {
            id: id.clone(),
            byte_offset: 0,
            preamble: None,
            record_table_binding_budget_exceeded: false,
            states: vec![state(&id, 9, Some(2)), state(&id, 2, None)],
        }
    };
    let histories = [history("first"), history("second")];
    let stream = "f3d:Design/BulkStream.dat";
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#100"),
        crate::records::feature::scope::DesignFeatureKind::Revolve,
        100,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(9);
            draft.previous_history_state_id = Some(2);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let next_scope = crate::records::feature::scope::DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#200"),
        crate::records::feature::scope::DesignFeatureKind::Sketch,
        200,
    );
    let binding =
        DesignBodyBinding::try_from(crate::records::bodies::DesignBodyBindingWire::<String> {
            id: format!("{stream}:design-body-binding#0"),
            stream: "Design/BulkStream.dat".into(),
            pair_count: 1,
            pair_ordinal: 0,
            asm_body_key: 1,
            asm_body_key_offset: 0,
            entity_suffix: 150,
            entity_suffix_offset: 8,
            blob_name: "BREP.second.smbh".into(),
            blob_name_offset: 16,
            body: None,
        })
        .unwrap();
    let scopes = vec![scope.clone(), next_scope];
    let bindings = crate::test_support::with_decode_context(|decode_ctx| {
        bind_scope_histories(
            decode_ctx,
            &scopes,
            std::slice::from_ref(&binding),
            &[],
            &histories,
        )
    })
    .unwrap();
    assert_eq!(bindings[&scope.id], histories[1].id);
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            bound_scope_history(decode_ctx, &scope.id, &bindings, &histories)
        })
        .unwrap()
        .expect("scope binding resolves one history")
        .id,
        histories[1].id
    );

    let operand = DesignBodyRecipeOperand::try_new(
        crate::records::topology::body_recipe::DesignBodyRecipeOperandDraft {
            id: format!("{stream}:design-body-recipe-operand#120"),
            scope_record_index: scope.record_index,
            owner: DesignOperandOwner::ScopeReference {
                scope_reference_ordinal: 0,
            },
            record_index: 120,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned())
                .unwrap(),
            asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 56,
            context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
            )
            .unwrap(),
            context_id_offset: 132,
            selector_tail: None,

            references: vec![DesignBodyRecipeReference {
                design_reference: 1,
                design_reference_offset: 25,
                form: 4,
                form_offset: 33,
                candidate_faces: vec![
                    FaceId::mint("f3d:brep/second.smbh/brep:entity#1").expect("identity grammar")
                ],
                preceding_candidate_faces: Vec::new(),
                preceding_body_slots: Vec::new(),
            }],
            nested_record_index: 123,
            nested_record_index_offset: 38,
            recipe_id: format!("{stream}:construction-recipe#1"),
            resolved_face_slot: None,
            resolved_body_state_id: None,
            resolved_body_slot: None,
            resolved_body_face_slots: Vec::new(),
            next_record_index: 124,
            next_byte_offset: 256,
        },
    )
    .unwrap();
    let bindings = crate::test_support::with_decode_context(|decode_ctx| {
        bind_scope_histories(
            decode_ctx,
            &scopes,
            &[],
            std::slice::from_ref(&operand),
            &histories,
        )
    })
    .unwrap();
    assert_eq!(bindings[&scope.id], histories[1].id);

    let unmatched_binding =
        DesignBodyBinding::try_from(crate::records::bodies::DesignBodyBindingWire::<String> {
            id: format!("{stream}:design-body-binding#0"),
            stream: "Design/BulkStream.dat".into(),
            pair_count: 1,
            pair_ordinal: 0,
            asm_body_key: 1,
            asm_body_key_offset: 0,
            entity_suffix: 150,
            entity_suffix_offset: 8,
            blob_name: "BREP.unmatched.smbh".into(),
            blob_name_offset: 16,
            body: None,
        })
        .unwrap();
    let bindings = crate::test_support::with_decode_context(|decode_ctx| {
        bind_scope_histories(
            decode_ctx,
            &scopes,
            std::slice::from_ref(&unmatched_binding),
            std::slice::from_ref(&operand),
            &histories,
        )
    })
    .unwrap();
    assert_eq!(bindings[&scope.id], histories[1].id);
}

#[test]
fn state_pairs_use_raw_next_links_before_transitions_are_derived() {
    let state = |state_id, node_index, previous_ref, next_ref| AsmDeltaState {
        id: format!("history:state-{state_id}"),
        parent: "history".into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref,
        next_ref,
        node_index,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        topology_cache: crate::history_records::AsmTopologyCache::Absent,
        transition: None,
    };
    let history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![
            state(10, 0, None, Some(2)),
            state(6, 2, Some(0), Some(3)),
            state(4, 3, Some(2), Some(1)),
            state(2, 1, Some(3), None),
        ],
    };
    let histories = [history];
    let (resolved, current, previous) = crate::test_support::with_decode_context(|decode_ctx| {
        unique_history_state_pair(decode_ctx, &histories, 10, 6)
    })
    .unwrap()
    .expect("raw direct state pair");
    assert_eq!(resolved.id, "history");
    assert_eq!(current.state_id, 10);
    assert_eq!(previous.state_id, 6);
    let (_, current, previous) = crate::test_support::with_decode_context(|decode_ctx| {
        unique_history_state_pair(decode_ctx, &histories, 10, 4)
    })
    .unwrap()
    .expect("raw reachable state pair");
    assert_eq!(current.state_id, 10);
    assert_eq!(previous.state_id, 4);
    let mut omitted_predecessor = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:native:scope#0",
        crate::records::feature::scope::DesignFeatureKind::Fillet,
        0,
    );
    omitted_predecessor
        .try_edit(|draft| {
            draft.history_state_id = Some(10);
        })
        .unwrap();
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            effective_scope_previous_history_state_id(decode_ctx, &omitted_predecessor, &histories)
        })
        .unwrap(),
        Some(6)
    );

    let mut root = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:native:scope#1",
        crate::records::feature::scope::DesignFeatureKind::BaseFlange,
        1,
    );
    root.try_edit(|draft| {
        draft.history_state_id = Some(4);
    })
    .unwrap();
    let mut successor = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:native:scope#2",
        crate::records::feature::scope::DesignFeatureKind::EdgeFlange,
        2,
    );
    successor
        .try_edit(|draft| {
            draft.history_state_id = Some(10);
            draft.previous_history_state_id = Some(6);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let scopes = vec![root, successor];
    let bindings = crate::test_support::with_decode_context(|decode_ctx| {
        bind_scope_histories(decode_ctx, &scopes, &[], &[], &histories)
    })
    .unwrap();
    assert_eq!(bindings.len(), 2);
    assert_eq!(bindings["f3d:native:scope#1"], "history");
    assert_eq!(bindings["f3d:native:scope#2"], "history");

    let mut inconsistent = histories[0].clone();
    inconsistent.states[0].transition = Some(AsmHistoricalTransition {
        previous_state_id: Some(4),
        records: Default::default(),
        topology: Default::default(),
    });
    assert!(crate::test_support::with_decode_context(|decode_ctx| {
        unique_history_state_pair(decode_ctx, &[inconsistent], 10, 6).map(|pair| pair.is_none())
    })
    .unwrap());
}

use crate::history_records::{AsmHistoricalCarrierBinding, AsmHistoricalSurfaceRadius};

#[test]
fn circular_pattern_face_uses_unique_rigid_surface_radius() {
    let preceding = AsmHistoricalTopology {
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 101,
            },
            AsmHistoricalCarrierBinding {
                entity: 12,
                carrier: 102,
            },
        ],
        surface_radii: vec![
            AsmHistoricalSurfaceRadius {
                surface: 101,
                radius: 2.5,
            },
            AsmHistoricalSurfaceRadius {
                surface: 102,
                radius: 4.0,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    let result = AsmHistoricalTopology {
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 21,
                carrier: 201,
            },
            AsmHistoricalCarrierBinding {
                entity: 22,
                carrier: 202,
            },
        ],
        surface_radii: vec![
            AsmHistoricalSurfaceRadius {
                surface: 201,
                radius: 2.5,
            },
            AsmHistoricalSurfaceRadius {
                surface: 202,
                radius: 2.5,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    let candidates = [
        cadmpeg_ir::ids::FaceId::mint(crate::ids::brep_entity_id(21)).expect("identity grammar"),
        cadmpeg_ir::ids::FaceId::mint(crate::ids::brep_entity_id(22)).expect("identity grammar"),
    ];
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            resolve_pattern_face_by_surface_radius(
                decode_ctx,
                &candidates,
                &preceding,
                &result,
                &HashSet::from([11, 12]),
            )
        })
        .unwrap(),
        Some(11)
    );

    let mut ambiguous = preceding;
    ambiguous.surface_radii[1].radius = 2.5;
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            resolve_pattern_face_by_surface_radius(
                decode_ctx,
                &candidates,
                &ambiguous,
                &result,
                &HashSet::from([11, 12]),
            )
        })
        .unwrap(),
        None
    );
}

#[test]
fn historical_pattern_face_axis_uses_one_analytic_surface_carrier() {
    let raw_axes = |axes: Vec<crate::records::feature::patterns::DesignAxis>| {
        axes.into_iter()
            .map(|axis| (axis.origin.get(), *axis.direction.as_raw()))
            .collect::<Vec<_>>()
    };
    let origin = cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0);
    let topology = AsmHistoricalTopology {
        faces: vec![11],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 11,
            carrier: 41,
        }],
        surface_axes: vec![crate::history_records::AsmHistoricalSurfaceAxis {
            surface: 41,
            origin,
            direction: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 2.0),
        }],
        ..AsmHistoricalTopology::default()
    };
    let history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![AsmDeltaState {
            id: "state".into(),
            parent: "history".into(),
            byte_offset: 0,
            state_id: 1,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 0,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            entity_versions: Vec::new(),
            topology_cache: crate::history_records::AsmTopologyCache::Complete(topology.clone()),
            transition: None,
        }],
    };
    assert_eq!(
        raw_axes(
            crate::test_support::with_decode_context(|decode_ctx| {
                historical_pattern_identity_axes_for_selection(
                    decode_ctx,
                    Some((AsmHistoricalEntityKind::Face, 11, &[1])),
                    &history,
                )
            })
            .unwrap()
        ),
        vec![(origin, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0))]
    );

    let mut planar_history = history.clone();
    let planar_topology = planar_history.states[0]
        .topology_mut()
        .expect("planar test topology");
    planar_topology.surface_axes.clear();
    planar_topology.surface_planes = vec![crate::history_records::AsmHistoricalPlane {
        surface: 41,
        origin,
        normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 2.0),
    }];
    assert_eq!(
        raw_axes(
            crate::test_support::with_decode_context(|decode_ctx| {
                historical_pattern_identity_axes_for_selection(
                    decode_ctx,
                    Some((AsmHistoricalEntityKind::Face, 11, &[1])),
                    &planar_history,
                )
            })
            .unwrap()
        ),
        vec![(origin, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0))]
    );

    let mut ambiguous = topology;
    ambiguous.face_surfaces.push(AsmHistoricalCarrierBinding {
        entity: 11,
        carrier: 42,
    });
    let ambiguous_history = AsmHistory {
        states: vec![AsmDeltaState {
            topology_cache: crate::history_records::AsmTopologyCache::Complete(ambiguous),
            ..history.states[0].clone()
        }],
        ..history.clone()
    };
    assert!(crate::test_support::with_decode_context(|decode_ctx| {
        historical_pattern_identity_axes_for_selection(
            decode_ctx,
            Some((AsmHistoricalEntityKind::Face, 11, &[1])),
            &ambiguous_history,
        )
    })
    .unwrap()
    .is_empty());

    let mut missing_carrier = history.clone();
    missing_carrier.states.push(AsmDeltaState {
        state_id: 2,
        node_index: 1,
        topology_cache: crate::history_records::AsmTopologyCache::Complete(AsmHistoricalTopology {
            faces: vec![11],
            ..AsmHistoricalTopology::default()
        }),
        ..history.states[0].clone()
    });
    assert!(crate::test_support::with_decode_context(|decode_ctx| {
        historical_pattern_identity_axes_for_selection(
            decode_ctx,
            Some((AsmHistoricalEntityKind::Face, 11, &[1, 2])),
            &missing_carrier,
        )
    })
    .unwrap()
    .is_empty());
    let identities = crate::test_support::with_decode_context(|decode_ctx| {
        HistoricalIdentityIndex::build(
            decode_ctx,
            std::slice::from_ref(&missing_carrier),
            decode_ctx
                .admit_iter(&[11], "scan F3D identity local IDs")
                .expect("test identity local ID admission"),
            |local_id| std::iter::once(*local_id).chain(None),
        )
    })
    .unwrap();
    assert_eq!(
        raw_axes(
            crate::test_support::with_decode_context(
                |decode_ctx| historical_pattern_identity_axes(
                    decode_ctx,
                    11,
                    &identities,
                    &missing_carrier,
                    Some(1)
                )
            )
            .unwrap()
        ),
        vec![(origin, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0))]
    );
    assert!(crate::test_support::with_decode_context(|decode_ctx| {
        historical_pattern_identity_axes(decode_ctx, 11, &identities, &missing_carrier, None)
    })
    .unwrap()
    .is_empty());
}

#[test]
fn snapshot_edge_identity_requires_one_edge_record_and_positive_revision() {
    let record = |index, name: &str, revision_id| AsmHistoryRecord {
        id: format!("record-{index}-{name}"),
        parent: "state".into(),
        revision_id,
        byte_offset: 0,
        framing: crate::history_records::AsmHistoryRecordFraming::Framed {
            index,
            name: name.into(),
            entity_references: Vec::new(),
        },
        raw_bytes: Vec::new(),
    };
    let history = |records| AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![AsmDeltaState {
            id: "state".into(),
            parent: "history".into(),
            byte_offset: 0,
            state_id: 1,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 0,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records,
            entity_versions: Vec::new(),
            topology_cache: crate::history_records::AsmTopologyCache::Absent,
            transition: None,
        }],
    };

    assert_eq!(
        crate::test_support::with_decode_context(|decode| snapshot_edge_identity_revision(
            decode,
            3,
            &history(vec![record(3, "edge", Some(17))])
        ))
        .unwrap(),
        Some(17)
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode| snapshot_edge_identity_revision(
            decode,
            3,
            &history(vec![record(3, "face", Some(17))])
        ))
        .unwrap(),
        None
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode| snapshot_edge_identity_revision(
            decode,
            3,
            &history(vec![
                record(3, "edge", Some(17)),
                record(3, "face", Some(18))
            ]),
        ))
        .unwrap(),
        None
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode| snapshot_edge_identity_revision(
            decode,
            3,
            &history(vec![record(3, "edge", Some(0))])
        ))
        .unwrap(),
        None
    );
}

#[test]
fn historical_identity_edge_requires_unique_incidence() {
    let mut topology = AsmHistoricalTopology {
        edges: vec![7, 8],
        coedges: vec![17, 18],
        curves: vec![27],
        pcurves: vec![37],
        coedge_topology: vec![
            crate::history_records::AsmHistoricalCoedge {
                coedge: 17,
                owner_loop: 0,
                edge: 7,
                next: 18,
                previous: 18,
                radial_next: 17,
            },
            crate::history_records::AsmHistoricalCoedge {
                coedge: 18,
                owner_loop: 0,
                edge: 8,
                next: 17,
                previous: 17,
                radial_next: 18,
            },
        ],
        edge_curves: vec![
            crate::history_records::AsmHistoricalOptionalCarrierBinding {
                entity: 7,
                carrier: Some(27),
            },
        ],
        coedge_pcurves: vec![
            crate::history_records::AsmHistoricalOptionalCarrierBinding {
                entity: 17,
                carrier: Some(37),
            },
        ],
        ..Default::default()
    };
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| historical_identity_edge(
            decode_ctx,
            AsmHistoricalEntityKind::Coedge,
            17,
            &topology
        ))
        .unwrap(),
        Some(7)
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| historical_identity_edge(
            decode_ctx,
            AsmHistoricalEntityKind::Curve,
            27,
            &topology
        ))
        .unwrap(),
        Some(7)
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| historical_identity_edge(
            decode_ctx,
            AsmHistoricalEntityKind::Pcurve,
            37,
            &topology
        ))
        .unwrap(),
        Some(7)
    );
    topology.edge_curves.push(
        crate::history_records::AsmHistoricalOptionalCarrierBinding {
            entity: 8,
            carrier: Some(27),
        },
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| historical_identity_edge(
            decode_ctx,
            AsmHistoricalEntityKind::Curve,
            27,
            &topology
        ))
        .unwrap(),
        None
    );
}

#[test]
fn terminal_edge_recipe_faces_use_exact_then_alternate_references() {
    use cadmpeg_ir::ids::FaceId;

    let reference = |candidate_faces, alternate_selector_faces| {
        crate::records::dimensions::DesignRecipeReference {
            selector: 1,
            selector_offset: 0,
            token: "1".into(),
            token_offset: 0,
            design_reference: 1,
            design_reference_offset: 0,
            candidate_faces,
            candidate_edges: Vec::new(),
            alternate_selector_faces,
            alternate_selector_edges: Vec::new(),
        }
    };
    assert_eq!(
        crate::test_support::with_decode_context(
            |decode_ctx| terminal_edge_recipe_reference_faces(
                decode_ctx,
                &[
                    reference(
                        vec![FaceId::mint("test:model:face#face-c").expect("identity grammar")],
                        vec![FaceId::mint("test:model:face#ignored").expect("identity grammar")],
                    ),
                    reference(
                        Vec::new(),
                        vec![FaceId::mint("test:model:face#face-d").expect("identity grammar")]
                    ),
                    reference(
                        vec![FaceId::mint("test:model:face#face-a").expect("identity grammar")],
                        Vec::new()
                    ),
                ],
                None
            )
        )
        .unwrap(),
        vec![
            vec![FaceId::mint("test:model:face#face-c").expect("identity grammar")],
            vec![FaceId::mint("test:model:face#face-d").expect("identity grammar")],
            vec![FaceId::mint("test:model:face#face-a").expect("identity grammar")],
        ]
    );
    let reference_faces = crate::test_support::with_decode_context(|decode_ctx| {
        terminal_edge_recipe_reference_faces(
            decode_ctx,
            &[
                reference(
                    vec![FaceId::mint("test:model:face#face-c").expect("identity grammar")],
                    vec![FaceId::mint("test:model:face#ignored").expect("identity grammar")],
                ),
                reference(
                    Vec::new(),
                    vec![FaceId::mint("test:model:face#face-d").expect("identity grammar")],
                ),
                reference(
                    vec![FaceId::mint("test:model:face#face-e").expect("identity grammar")],
                    Vec::new(),
                ),
            ],
            Some(&[std::num::NonZeroU32::new(2).unwrap()]),
        )
    })
    .unwrap();
    assert_eq!(
        reference_faces,
        vec![vec![
            FaceId::mint("test:model:face#face-d").expect("identity grammar")
        ]]
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| terminal_edge_recipe_faces(
            decode_ctx,
            &[
                FaceId::mint("test:model:face#face-b").expect("identity grammar"),
                FaceId::mint("test:model:face#face-a").expect("identity grammar")
            ],
            &reference_faces
        ))
        .unwrap(),
        vec![
            FaceId::mint("test:model:face#face-a").expect("identity grammar"),
            FaceId::mint("test:model:face#face-b").expect("identity grammar"),
            FaceId::mint("test:model:face#face-d").expect("identity grammar"),
        ]
    );
}

#[test]
fn bound_state_pair_keeps_repeated_numeric_ids_in_one_history() {
    let state = |parent: &str, state_id: i64, previous_state_id: Option<i64>| AsmDeltaState {
        id: format!("{parent}:state-{state_id}"),
        parent: parent.into(),
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
        topology_cache: crate::history_records::AsmTopologyCache::Absent,
        transition: previous_state_id.map(|previous_state_id| AsmHistoricalTransition {
            previous_state_id: Some(previous_state_id),
            records: Default::default(),
            topology: Default::default(),
        }),
    };
    let history = |id: &str| AsmHistory {
        id: id.into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![
            state(id, 11, Some(10)),
            state(id, 10, Some(9)),
            state(id, 9, None),
        ],
    };
    let histories = [history("history-a"), history("history-b")];
    let bindings = HashMap::from([("scope".into(), "history-b".into())]);

    let (selected, state, previous) = crate::test_support::with_decode_context(|decode_ctx| {
        bound_history_state_pair(decode_ctx, "scope", 11, 9, &bindings, &histories)
    })
    .unwrap()
    .expect("scope-bound repeated state pair");
    assert_eq!(selected.id, "history-b");
    assert_eq!(state.parent, "history-b");
    assert_eq!(previous.parent, "history-b");
}

#[test]
fn boundary_edge_change_partition_preserves_boundary_order() {
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| boundary_edges_in_changes(
            decode_ctx,
            &[8, 3, 5, 2],
            &[2, 8]
        ))
        .unwrap(),
        [8, 2]
    );
    assert!(
        crate::test_support::with_decode_context(|decode_ctx| boundary_edges_in_changes(
            decode_ctx,
            &[8, 3],
            &[1, 2]
        ))
        .unwrap()
        .is_empty()
    );
}

#[test]
fn topology_changes_span_only_complete_acyclic_state_chains() {
    let preceding = limits::change_state(1);
    let mut intermediate = limits::change_state(2);
    let mut result = limits::change_state(3);
    let mut first = AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta::default(),
    };
    first.topology.faces.updated = vec![10];
    first.topology.edges.updated = vec![20];
    intermediate.transition = Some(first);
    let mut second = AsmHistoricalTransition {
        previous_state_id: Some(2),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta::default(),
    };
    second.topology.faces.deleted = vec![11];
    second.topology.edges.deleted = vec![21];
    result.transition = Some(second);
    let states = HashMap::from([
        (1, Some(&preceding)),
        (2, Some(&intermediate)),
        (3, Some(&result)),
    ]);

    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| face_changes_across_state_chain(
            decode_ctx, &result, 1, &states
        ))
        .unwrap(),
        Some(HashSet::from([10, 11]))
    );
    let incomplete = HashMap::from([(1, Some(&preceding)), (3, Some(&result))]);
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| face_changes_across_state_chain(
            decode_ctx,
            &result,
            1,
            &incomplete
        ))
        .unwrap(),
        None
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| edge_changes_across_state_chain(
            decode_ctx, &result, 1, &states
        ))
        .unwrap()
        .map(|changes| (changes.deleted, changes.updated)),
        Some((HashSet::from([21]), HashSet::from([20])))
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| edge_changes_across_state_chain(
            decode_ctx,
            &result,
            1,
            &incomplete
        ))
        .unwrap(),
        None
    );
    let mut cyclic_intermediate = intermediate.clone();
    cyclic_intermediate
        .transition
        .as_mut()
        .unwrap()
        .previous_state_id = Some(3);
    let cyclic = HashMap::from([
        (1, Some(&preceding)),
        (2, Some(&cyclic_intermediate)),
        (3, Some(&result)),
    ]);
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| face_changes_across_state_chain(
            decode_ctx, &result, 1, &cyclic
        ))
        .unwrap(),
        None
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| edge_changes_across_state_chain(
            decode_ctx, &result, 1, &cyclic
        ))
        .unwrap(),
        None
    );
}

mod identity;

#[test]
fn recipe_selector_side_count_scan_refuses_work_limit() {
    let operation = "scan F3D recipe selector required sides";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |decode| incident_loop_counts_satisfy_sides(decode, &[5], &[Some(5)]),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}
