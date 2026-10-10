// SPDX-License-Identifier: Apache-2.0
//! Resource admission for STEP representation-body walks.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::BodyId;

use super::super::{admitted_body_clone, representation_bodies, TopologyData};

fn body_id() -> BodyId {
    BodyId::try_from("step:data:body#1").expect("test body id")
}

#[test]
fn representation_body_vector_refuses_before_two_item_copy() {
    let bodies = [
        body_id(),
        BodyId::try_from("step:data:body#2").expect("test body id"),
    ];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(b"bodies", &arena, &policy)
        .expect("root fits selected profile");
    let error = admitted_body_clone(&bodies, &ctx, "step_representation_body_test_copy")
        .expect_err("two body slots exceed one collection item");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "step_representation_body_test_copy"
    ));
}

fn topology_with_body_at(root: u64) -> TopologyData<'static> {
    TopologyData {
        storage: None,
        claim_storage: None,
        body_by_root: BTreeMap::from([(root, vec![body_id()])]),
        shape_representation_relationships: BTreeMap::new(),
        body_by_shell: BTreeMap::new(),
        faces_by_source: BTreeMap::new(),
        edges_by_source: BTreeMap::new(),
        vertices_by_source: BTreeMap::new(),
    }
}

fn source(records: &str) -> String {
    format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;"
    )
}

fn assert_walk_limit(
    records: &str,
    topology: &TopologyData,
    admitted: u64,
    operation: &'static str,
    calls: usize,
) {
    let source = source(records);
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("test exchange parses");
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &service)
        .expect("service root admission");
    let mut cache = BTreeMap::new();
    for _ in 0..calls {
        let bodies = representation_bodies(
            20,
            &exchange,
            topology,
            &mut cache,
            &mut BTreeSet::new(),
            &ctx,
        )
        .expect("service admits representation bodies");
        assert_eq!(bodies.len(), 1);
    }
    let mut limited = service;
    limited.limits.max_collection_items = admitted;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &limited)
        .expect("limited root admission");
    let mut cache = BTreeMap::new();
    let mut error = None;
    for _ in 0..calls {
        match representation_bodies(
            20,
            &exchange,
            topology,
            &mut cache,
            &mut BTreeSet::new(),
            &ctx,
        ) {
            Ok(_) => {}
            Err(refusal) => {
                error = Some(refusal);
                break;
            }
        }
    }
    let error = error.expect("selected limit refuses the next representation allocation");
    assert!(
        matches!(&error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == operation),
        "unexpected refusal: {error}"
    );
}

#[test]
fn representation_root_bodies_charge_before_clone() {
    assert_walk_limit(
        "#20=SHAPE_REPRESENTATION('',(),$);",
        &topology_with_body_at(20),
        0,
        "step_representation_body_root_copy",
        1,
    );
}

#[test]
fn representation_cache_values_charge_before_clone() {
    assert_walk_limit(
        "#20=SHAPE_REPRESENTATION('',(),$);",
        &topology_with_body_at(20),
        1,
        "step_representation_body_cache_values",
        1,
    );
}

#[test]
fn representation_cache_entries_charge_before_insertion() {
    assert_walk_limit(
        "#20=SHAPE_REPRESENTATION('',(),$);",
        &topology_with_body_at(20),
        2,
        "step_representation_body_cache_entries",
        1,
    );
}

#[test]
fn representation_cached_bodies_charge_before_clone() {
    assert_walk_limit(
        "#20=SHAPE_REPRESENTATION('',(),$);",
        &topology_with_body_at(20),
        3,
        "step_representation_body_cache_copy",
        2,
    );
}

#[test]
fn representation_root_bodies_reserve_temporary_bytes_before_clone() {
    let source = source("#20=SHAPE_REPRESENTATION('',(),$);");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("test exchange parses");
    let topology = topology_with_body_at(20);
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &service)
        .expect("service root admission");
    representation_bodies(
        20,
        &exchange,
        &topology,
        &mut BTreeMap::new(),
        &mut BTreeSet::new(),
        &ctx,
    )
    .expect("service admits root body bytes");
    let mut limited = service;
    limited.limits.max_materialized_bytes =
        u64::try_from(std::mem::size_of::<BodyId>() + body_id().as_str().len() - 1)
            .expect("test body byte count fits u64");
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &limited)
        .expect("limited root admission");
    let error = representation_bodies(
        20,
        &exchange,
        &topology,
        &mut BTreeMap::new(),
        &mut BTreeSet::new(),
        &ctx,
    )
    .expect_err("root body bytes exceed the selected temporary allowance");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_representation_body_root_copy")
    );
}

#[test]
fn representation_active_path_charges_before_insertion() {
    assert_walk_limit(
        "#2=CARTESIAN_POINT('',(0.,0.,0.));#20=SHAPE_REPRESENTATION('',(#2),$);",
        &topology_with_body_at(2),
        0,
        "step_representation_body_active",
        1,
    );
}

#[test]
fn representation_body_set_charges_before_clone() {
    assert_walk_limit(
        "#2=CARTESIAN_POINT('',(0.,0.,0.));#20=SHAPE_REPRESENTATION('',(#2),$);",
        &topology_with_body_at(2),
        1,
        "step_representation_body_set",
        1,
    );
}

#[test]
fn representation_output_bodies_charge_before_collection() {
    assert_walk_limit(
        "#2=CARTESIAN_POINT('',(0.,0.,0.));#20=SHAPE_REPRESENTATION('',(#2),$);",
        &topology_with_body_at(2),
        2,
        "step_representation_body_output",
        1,
    );
}

#[test]
fn representation_claims_match_body_resolution_through_mapping_and_cycles() {
    let cases = [
        "#2=DUMMY();#3=DUMMY();#20=SHAPE_REPRESENTATION('',(#2,#3),$);#21=REPRESENTATION_MAP($,#20);#22=MAPPED_ITEM('',#21,$);#23=SHAPE_REPRESENTATION('',(#22),$);",
        "#2=DUMMY();#3=DUMMY();#20=SHAPE_REPRESENTATION('',(#22,#2),$);#21=REPRESENTATION_MAP($,#23);#22=MAPPED_ITEM('',#21,$);#23=SHAPE_REPRESENTATION('',(#25),$);#24=REPRESENTATION_MAP($,#20);#25=MAPPED_ITEM('',#24,$);",
        "#2=DUMMY();#3=DUMMY();#20=SHAPE_REPRESENTATION('',(#22),$);#21=REPRESENTATION_MAP($,#23);#22=MAPPED_ITEM('',#21,$);#23=SHAPE_REPRESENTATION('',(#25),$);#24=REPRESENTATION_MAP($,#20);#25=MAPPED_ITEM('',#24,$);",
        "#2=DUMMY();#3=DUMMY();#20=SHAPE_REPRESENTATION('',(#2),$);#23=SHAPE_REPRESENTATION('',(),$);",
        "#2=DUMMY();#3=DUMMY();#20=SHAPE_REPRESENTATION('',(#2,1.),$);#23=SHAPE_REPRESENTATION('',(#3),$);",
    ];
    for records in cases {
        let input = source(records);
        let (exchange, _) =
            crate::test_support::with_service_context(input.as_bytes(), crate::parse::parse_inner)
                .expect("claim graph parses");
        for related in [false, true] {
            let mut topology = topology_with_body_at(2);
            topology.body_by_root.insert(3, Vec::new());
            if related {
                topology.shape_representation_relationships =
                    BTreeMap::from([(20, vec![23]), (23, vec![20])]);
            }
            let ctx = cadmpeg_test_support::service_decode_context();
            let mut bodies = BTreeMap::new();
            let mut claims = BTreeMap::new();
            let mut storage = ctx.reserve_scoped(0, "test claim cache").unwrap();
            for id in [20, 23, 20, 3, 2] {
                let mut active = BTreeSet::new();
                let expected = !representation_bodies(
                    id,
                    &exchange,
                    &topology,
                    &mut bodies,
                    &mut active,
                    &ctx,
                )
                .expect("body walk")
                .is_empty();
                assert!(active.is_empty());
                let actual = super::super::representation_has_body(
                    id,
                    &exchange,
                    &topology,
                    &mut claims,
                    &mut active,
                    &mut storage,
                    &ctx,
                )
                .expect("claim walk");
                assert_eq!(
                    actual, expected,
                    "representation {id}: {records}, relationships={related}"
                );
                assert!(active.is_empty());
            }
            drop((bodies, claims, storage));
            let CodecError::ResourceLimit(limit) = ctx
                .reserve_scoped(u64::MAX, "claim storage probe")
                .expect_err("probe materialized counter")
            else {
                panic!("materialized refusal");
            };
            assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(limit.used, 0);
        }
    }
}

fn representation_claim_fanout_work(count: u64) -> u64 {
    use std::fmt::Write as _;
    let mut records = String::new();
    let mut topology = topology_with_body_at(1);
    let mut items = Vec::new();
    for id in 1..=count {
        write!(records, "#{id}=DUMMY();").unwrap();
        items.push(format!("#{id}"));
        topology.body_by_root.insert(
            id,
            vec![BodyId::from(crate::ids::data(
                crate::ids::kind!("body"),
                id,
            ))],
        );
    }
    write!(
        records,
        "#1000=SHAPE_REPRESENTATION('',({}),$);#1001=REPRESENTATION_MAP($,#1000);",
        items.join(",")
    )
    .unwrap();
    for id in 0..count {
        write!(
            records,
            "#{}=MAPPED_ITEM('',#1001,$);#{}=SHAPE_REPRESENTATION('',(#{}),$);",
            2000 + id,
            3000 + id,
            2000 + id
        )
        .unwrap();
    }
    let input = source(&records);
    let (exchange, _) =
        crate::test_support::with_service_context(input.as_bytes(), crate::parse::parse_inner)
            .expect("fanout parses");
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut cache = BTreeMap::new();
    let mut storage = ctx.reserve_scoped(0, "test claim cache").unwrap();
    for id in 3000..3000 + count {
        assert!(super::super::representation_has_body(
            id,
            &exchange,
            &topology,
            &mut cache,
            &mut BTreeSet::new(),
            &mut storage,
            &ctx
        )
        .expect("claim resolution"));
    }
    assert_eq!(cache.len(), usize::try_from(count + 1).unwrap());
    let CodecError::ResourceLimit(limit) = ctx
        .charge_work(u64::MAX, "claim work probe")
        .expect_err("probe work counter")
    else {
        panic!("work refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    limit.used
}

#[test]
fn representation_claim_fanout_does_not_expand_cached_body_lists() {
    let small = representation_claim_fanout_work(64);
    let large = representation_claim_fanout_work(128);
    // Body roots and mapping links double; ordered keyed queries grow with n log n.
    assert!(large < 3 * small, "claim work grew from {small} to {large}");
}
