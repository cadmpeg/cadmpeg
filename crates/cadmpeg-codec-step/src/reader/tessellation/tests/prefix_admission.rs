// SPDX-License-Identifier: Apache-2.0
//! First-item admission and scoped owner lifetimes.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{
    u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::transform::Transform;

use crate::parse::{parse_inner, Value};
use crate::test_support::{with_policy_context, with_service_context};

const MEMBERS: usize = 8193;
const PREFIX_WORK: u64 = 4096;

#[test]
fn distinct_tessellation_placement_refuses_first_insert_before_large_suffix() {
    let placements = vec![Transform::identity(); MEMBERS];
    let mut limited = DecodePolicy::service();
    limited.limits.max_work_units = 8192;
    limited.limits.max_collection_items = 0;
    // The first insertion fits this work range; all 8193 source visits do not.
    with_policy_context(&[], &limited, |_, ctx| {
        let error = super::super::distinct_placement_count(&placements, ctx)
            .expect_err("first insertion refuses");
        assert!(matches!(&error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "step_tessellation_distinct_placements"));
        let CodecError::ResourceLimit(limit) = error else {
            unreachable!()
        };
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert_eq!((limit.used, limit.additional), (0, 1));
    });
    with_service_context(&[], |_, ctx| {
        assert_eq!(
            super::super::distinct_placement_count(&placements, ctx)
                .expect("all placements admitted"),
            1
        );
    });
}

#[test]
fn tessellation_body_association_refuses_first_copy_before_large_suffix() {
    let body = BodyId::mint("step:data:body#1").expect("identity");
    let bodies = vec![&body; MEMBERS];
    let mut limited = DecodePolicy::service();
    limited.limits.max_work_units = PREFIX_WORK;
    // The item entry is inserted before the first body-link entry.
    limited.limits.max_collection_items = 1;
    with_policy_context(&[], &limited, |_, ctx| {
        let mut bytes = ctx.reserve_scoped(0, "test associations").expect("guard");
        let mut associations = BTreeMap::new();
        let error = super::super::associate_bodies(&mut associations, 2, &bodies, ctx, &mut bytes)
            .expect_err("first body link refuses");
        assert!(matches!(&error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "step_tessellation_item_body_links"));
        assert_eq!(associations[&2].len(), 0);
        let CodecError::ResourceLimit(limit) = error else {
            unreachable!()
        };
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
    with_service_context(&[], |_, ctx| {
        let mut bytes = ctx.reserve_scoped(0, "test associations").expect("guard");
        let mut associations = BTreeMap::new();
        super::super::associate_bodies(&mut associations, 2, &bodies, ctx, &mut bytes)
            .expect("all body links admitted");
        assert_eq!(associations[&2], BTreeSet::from([body.clone()]));
    });
}

#[test]
fn tessellation_representation_items_refuse_first_push_before_large_suffix() {
    let mut records = String::from("#1=TESSELLATED_SHAPE_REPRESENTATION('',(#2");
    for _ in 1..MEMBERS {
        records.push_str(",#2");
    }
    records.push_str("),$);#2=TESSELLATED_ITEM();");
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    let (exchange, _) = with_service_context(source.as_bytes(), parse_inner).expect("exchange");
    let mut limited = DecodePolicy::service();
    limited.limits.max_work_units = PREFIX_WORK;
    limited.limits.max_collection_items = 0;
    with_policy_context(&[], &limited, |_, ctx| {
        let error = super::super::admitted_representation_items(&exchange.records()[&1], ctx)
            .expect_err("first item refuses");
        assert!(matches!(&error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "step_tessellation_representation_items"));
        let CodecError::ResourceLimit(limit) = error else {
            unreachable!()
        };
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
    with_service_context(&[], |_, ctx| {
        let (buffer, _storage) =
            super::super::admitted_representation_items(&exchange.records()[&1], ctx)
                .expect("admitted items")
                .expect("items");
        let items = buffer;
        assert_eq!(items, vec![2; MEMBERS]);
    });
}

#[test]
fn tessellation_linked_bodies_refuse_first_insert_before_large_suffix() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=TESSELLATED_SOLID('',(),#2);#2=TESSELLATED_SHELL('',(),#2);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = with_service_context(source, parse_inner).expect("exchange");
    with_service_context(source, |_, owner_ctx| {
        let mut ir = CadIr::empty();
        let index =
            super::super::super::index::CarrierIndex::from_ir(&ir, owner_ctx).expect("index");
        let mut topology =
            super::super::super::topology::decode(&exchange, &mut ir, &index, owner_ctx)
                .expect("topology")
                .value;
        let body = BodyId::mint("step:data:body#2").expect("identity");
        topology.body_by_root.insert(2, vec![body.clone(); MEMBERS]);
        let shell_bodies = (0..MEMBERS)
            .map(|index| BodyId::mint(format!("step:data:body#{index}")).expect("identity"))
            .collect();
        topology.body_by_shell.insert(2, shell_bodies);
        for (id, kind) in [(1, "TESSELLATED_SOLID"), (2, "TESSELLATED_SHELL")] {
            let mut limited = DecodePolicy::service();
            limited.limits.max_work_units = PREFIX_WORK;
            limited.limits.max_collection_items = 0;
            with_policy_context(&[], &limited, |_, ctx| {
                let error =
                    super::super::linked_bodies(&exchange.records()[&id], kind, &topology, ctx)
                        .expect_err("first link refuses");
                assert!(matches!(&error, CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::CollectionItems
                        && limit.operation == "step_tessellation_linked_bodies"));
                let CodecError::ResourceLimit(limit) = error else {
                    unreachable!()
                };
                assert_eq!(ctx.resource_refusal(), Some(limit));
            });
            with_service_context(&[], |_, ctx| {
                let (buffer, _storage) =
                    super::super::linked_bodies(&exchange.records()[&id], kind, &topology, ctx)
                        .expect("admitted links");
                let linked = buffer;
                if kind == "TESSELLATED_SOLID" {
                    assert_eq!(linked, BTreeSet::from([&body]));
                } else {
                    assert_eq!(linked.len(), MEMBERS);
                    assert!(linked.contains(&body));
                }
            });
        }
    });
}

#[test]
fn tessellation_index_rows_release_both_lanes_after_semantic_error() {
    let values = Value::List(vec![
        Value::List(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ]),
        Value::List(vec![Value::Integer(1), Value::Integer(2)]),
    ]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 1024;
    with_policy_context(&[], &policy, |_, ctx| {
        let error = super::super::index_rows(
            Some(&values),
            "COMPLEX_TRIANGULATED_SURFACE_SET",
            1,
            "strip",
            ctx,
        )
        .expect_err("second row is short");
        assert!(
            matches!(error, CodecError::Malformed(message) if message == "COMPLEX_TRIANGULATED_SURFACE_SET #1 strip row 2 is invalid")
        );
        assert_eq!(ctx.resource_refusal(), None);
        let _all_storage = ctx
            .reserve_scoped(1024, "post-error owner release")
            .expect("discarded row and index storage released");
    });
}

#[test]
fn tessellation_index_rows_hold_and_release_actual_nested_capacity() {
    let values = Value::List(vec![Value::List(vec![
        Value::Integer(1),
        Value::Integer(2),
        Value::Integer(3),
    ])]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 1024;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let (buffer, rows_storage, indices_storage) = super::super::index_rows(
        Some(&values),
        "COMPLEX_TRIANGULATED_SURFACE_SET",
        1,
        "strip",
        &ctx,
    )
    .expect("valid strip");
    let rows = buffer;
    let footprint = u64_from_index(
        rows.capacity() * std::mem::size_of::<Vec<u32>>()
            + rows[0].capacity() * std::mem::size_of::<u32>(),
    );
    assert!(footprint > 0);
    let remaining = ctx
        .reserve_scoped(1024 - footprint, "live owner complement")
        .expect("exact remaining storage");
    assert_eq!(rows, vec![vec![1, 2, 3]]);
    drop(remaining);
    drop(rows);
    drop(indices_storage);
    drop(rows_storage);
    let _all_storage = ctx
        .reserve_scoped(1024, "post-success owner release")
        .expect("both backing lanes released");
}

#[test]
fn tessellation_decode_skips_surface_index_without_support_lookup() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT('',(0.,0.,0.));#2=AXIS2_PLACEMENT_3D('',#1,$,$);#3=PLANE('',#2);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = with_service_context(source, parse_inner).expect("exchange");
    with_service_context(source, |_, owner_ctx| {
        let mut ir = CadIr::empty();
        let geometry =
            super::super::super::geometry::decode(&exchange, &mut ir, owner_ctx).expect("geometry");
        let index =
            super::super::super::index::CarrierIndex::from_ir(&ir, owner_ctx).expect("index");
        let topology = super::super::super::topology::decode(&exchange, &mut ir, &index, owner_ctx)
            .expect("topology");
        assert_eq!(ir.model.surfaces.len(), 1);
        ir.model.surfaces = vec![ir.model.surfaces[0].clone(); MEMBERS];
        let mut limited = DecodePolicy::service();
        limited.limits.max_work_units = PREFIX_WORK;
        limited.limits.max_collection_items = 0;
        with_policy_context(&[], &limited, |_, ctx| {
            let mut admitted = u64_from_index(ir.model.entity_count());
            let stage = super::super::decode(&exchange, &geometry.value, &topology.value, &mut ir, ctx, &mut admitted)
                .expect("no support lookup builds no surface index");
            assert!(stage.claims.is_empty());
            assert!(stage.losses.is_empty());
            assert!(ir.model.tessellations.is_empty());
            assert_eq!(ctx.resource_refusal(), None);
        });
        let mut allowed = DecodePolicy::service();
        allowed.limits.max_materialized_bytes = 1024;
        with_policy_context(&[], &allowed, |_, ctx| {
            let mut admitted = u64_from_index(ir.model.entity_count());
            let stage =
                super::super::decode(&exchange, &geometry.value, &topology.value, &mut ir, ctx, &mut admitted)
                    .expect("no surface-index work required");
            assert!(stage.claims.is_empty());
            assert!(stage.losses.is_empty());
            assert!(ir.model.tessellations.is_empty());
            drop(stage);
            let _all_storage = ctx
                .reserve_scoped(
                    ctx.policy().limits.max_materialized_bytes,
                    "post-stage scratch release",
                )
                .expect("all stage scratch released");
        });
    });
}
