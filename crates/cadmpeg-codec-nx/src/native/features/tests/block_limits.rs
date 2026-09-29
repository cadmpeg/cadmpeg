// SPDX-License-Identifier: Apache-2.0

fn block_construction_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let references = (0..19_u32)
        .map(|ordinal| {
            crate::native::features::block_reference::FeatureBlockConstructionReference {
                id: format!("reference#{ordinal}"),
                operation_label: "operation".into(),
                control: 0x26,
                position: crate::native::features::block_reference::BlockReferencePosition::new(
                    ordinal,
                )
                .expect("block reference position"),
                token: crate::om::reference_index::PayloadIndexToken::from_wire(
                    ordinal + 100,
                    &[
                        0xf0,
                        u8::try_from(ordinal + 100).expect("small object index"),
                    ],
                )
                .expect("payload index token"),
                data_block: Some(format!("block#{ordinal}")),
                source_offset: u64::from(ordinal),
            }
        })
        .collect::<Vec<_>>();
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        crate::native::features::construction_records::feature_block_constructions(ctx, &references)
    };
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted block construction")
            .len(),
        1
    );
    
    
    
    crate::test_support::with_decode_context_over(&[], |policy| { configure(policy); }, |ctx| {

    decode(ctx).expect_err("block construction resource limit")

})
}

fn block_payload_input(
    first_block: &[u8],
) -> (
    crate::container::Container<'static>,
    crate::native::features::FeatureBlockConstruction,
) {
    let mut store = vec![b"A".as_slice(); 20];
    store[0] = first_block;
    let part = crate::test_support::test_om::composed_feature_history_payload(&[], &store);
    let file =
        crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    let container = crate::test_support::with_decode_context(move |ctx| {
        crate::container::scan_bytes(ctx, file)
    })
    .expect("block payload container");
    let construction = crate::native::features::FeatureBlockConstruction {
        id: "block-construction#0".into(),
        operation_label: "operation".into(),
        control: 0x26,
        members: std::array::from_fn(|ordinal| {
            crate::native::features::FeatureConstructionMember {
                reference: format!("reference#{ordinal}"),
                data_block: format!("nx:om-data-blocks-0:block#{}", ordinal + 1),
            }
        }),
        terminal_reference: "reference#18".into(),
        terminal_data_block: "nx:om-data-blocks-0:block#19".into(),
    };
    (container, construction)
}

fn block_payload_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let (container, construction) = block_payload_input(b"A");
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        crate::native::features::construction_records::feature_block_construction_payloads(
            ctx,
            &container,
            std::slice::from_ref(&construction),
        )
    };
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted block payload")
            .len(),
        1
    );
    
    
    
    crate::test_support::with_decode_context_over(&[], |policy| { configure(policy); }, |ctx| {

    decode(ctx).expect_err("block payload resource limit")

})
}

#[derive(Clone, Copy)]
enum BlockFieldRoute {
    Scalar,
    Name,
}

fn block_field_refusal(
    route: BlockFieldRoute,
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let bytes = b"\x03\x08Point1\0\x50\x59\x66\x64\x00\x30\x43\x0c\xcc\xcc\xcc\xcd\x72";
    let (container, construction) = block_payload_input(bytes);
    let payloads = crate::test_support::with_decode_context(|ctx| {
        crate::native::features::construction_records::feature_block_construction_payloads(
            ctx,
            &container,
            std::slice::from_ref(&construction),
        )
    })
    .expect("block construction payloads");
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| match route {
        BlockFieldRoute::Scalar => {
            crate::native::features::construction_records::feature_block_payload_scalars(
                ctx, &container, &payloads,
            )
            .map(|rows| rows.len())
        }
        BlockFieldRoute::Name => {
            crate::native::features::construction_records::feature_block_payload_names(
                ctx, &container, &payloads,
            )
            .map(|rows| rows.len())
        }
    };
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| decode(ctx)).expect("admitted block field"),
        1
    );
    
    
    
    crate::test_support::with_decode_context_over(&[], |policy| { configure(policy); }, |ctx| {

    decode(ctx).expect_err("block field resource limit")

})
}

macro_rules! block_field_limit_tests {
    ($collection:ident, $retained:ident, $scoped:ident, $work:ident, $route:expr) => {
        #[test]
        fn $collection() {
            let error = block_field_refusal($route,
                |policy| policy.limits.max_collection_items = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
        }
        #[test]
        fn $retained() {
            let error = block_field_refusal($route,
                |policy| policy.limits.max_retained_bytes = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
        }
        #[test]
        fn $scoped() {
            let error = block_field_refusal($route,
                |policy| policy.limits.max_materialized_bytes = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
        }
        #[test]
        fn $work() {
            let error = block_field_refusal($route,
                |policy| policy.limits.max_work_units = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
        }
    };
}

block_field_limit_tests!(
    block_scalar_refuses_collection_limit,
    block_scalar_refuses_retained_limit,
    block_scalar_refuses_scoped_limit,
    block_scalar_refuses_work_limit,
    BlockFieldRoute::Scalar
);
block_field_limit_tests!(
    block_name_refuses_collection_limit,
    block_name_refuses_retained_limit,
    block_name_refuses_scoped_limit,
    block_name_refuses_work_limit,
    BlockFieldRoute::Name
);

fn block_named_record_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let bytes = b"\x03\x08Point1\0\x50\x59\x66\x64\x00\x30\x43\x0c\xcc\xcc\xcc\xcd\x72";
    let (container, construction) = block_payload_input(bytes);
    let (payloads, names, scalars) = crate::test_support::with_decode_context(|ctx| {
        let payloads =
            crate::native::features::construction_records::feature_block_construction_payloads(
                ctx,
                &container,
                std::slice::from_ref(&construction),
            )?;
        let names = crate::native::features::construction_records::feature_block_payload_names(
            ctx, &container, &payloads,
        )?;
        let scalars = crate::native::features::construction_records::feature_block_payload_scalars(
            ctx, &container, &payloads,
        )?;
        Ok::<_, cadmpeg_core::CodecError>((payloads, names, scalars))
    })
    .expect("block named record inputs");
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        crate::native::features::construction_records::feature_block_payload_named_records(
            ctx, &payloads, &names, &scalars,
        )
    };
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted block named record")
            .len(),
        1
    );
    
    
    
    crate::test_support::with_decode_context_over(&[], |policy| { configure(policy); }, |ctx| {

    decode(ctx).expect_err("block named record resource limit")

})
}

#[test]
fn block_named_record_refuses_collection_limit() {
    let error = block_named_record_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn block_named_record_refuses_retained_limit() {
    let error = block_named_record_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn block_named_record_refuses_scoped_limit() {
    let error = block_named_record_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn block_named_record_refuses_work_limit() {
    let error = block_named_record_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn block_point_input() -> (
    Vec<crate::native::features::FeatureBlockPayloadNamedRecord>,
    Vec<crate::native::features::payload_name::FeaturePayloadName>,
    Vec<crate::native::features::FeaturePayloadScalar>,
) {
    let bytes = b"\x03\x08Point1\0\x50\x59\x66\x64\x00\x30\x43\x0c\xcc\xcc\xcc\xcd\x72";
    let (container, construction) = block_payload_input(bytes);
    crate::test_support::with_decode_context(|ctx| {
        let payloads =
            crate::native::features::construction_records::feature_block_construction_payloads(
                ctx,
                &container,
                std::slice::from_ref(&construction),
            )?;
        let names = crate::native::features::construction_records::feature_block_payload_names(
            ctx, &container, &payloads,
        )?;
        let mut scalars =
            crate::native::features::construction_records::feature_block_payload_scalars(
                ctx, &container, &payloads,
            )?;
        let mut records =
            crate::native::features::construction_records::feature_block_payload_named_records(
                ctx, &payloads, &names, &scalars,
            )?;
        assert_eq!((records.len(), scalars.len()), (1, 1));
        let mut second = scalars[0].clone();
        second.id = "second-scalar".into();
        records[0].scalar_fields.push(second.id.clone());
        scalars.push(second);
        Ok::<_, cadmpeg_core::CodecError>((records, names, scalars))
    })
    .expect("block point inputs")
}

#[derive(Clone, Copy)]
enum BlockPointRoute {
    Point,
    Group,
}

fn block_point_refusal(
    route: BlockPointRoute,
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let (records, names, scalars) = block_point_input();
    let points = crate::test_support::with_decode_context(|ctx| {
        crate::native::features::construction_records::feature_block_payload_points(
            ctx, &records, &names, &scalars,
        )
    })
    .expect("admitted block point");
    assert_eq!(points.len(), 1);
    let mut second = points[0].clone();
    second.id = "second-point".into();
    let group_points = [points[0].clone(), second];
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| match route {
        BlockPointRoute::Point => {
            crate::native::features::construction_records::feature_block_payload_points(
                ctx, &records, &names, &scalars,
            )
            .map(|rows| rows.len())
        }
        BlockPointRoute::Group => {
            crate::native::features::construction_records::feature_block_payload_point_groups(
                ctx,
                &group_points,
            )
            .map(|rows| rows.len())
        }
    };
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted block point route"),
        1
    );
    
    
    
    crate::test_support::with_decode_context_over(&[], |policy| { configure(policy); }, |ctx| {

    decode(ctx).expect_err("block point resource limit")

})
}

macro_rules! block_point_limit_tests {
    ($collection:ident, $retained:ident, $work:ident, $route:expr) => {
        #[test]
        fn $collection() {
            let error = block_point_refusal($route,
                |policy| policy.limits.max_collection_items = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
        }
        #[test]
        fn $retained() {
            let error = block_point_refusal($route,
                |policy| policy.limits.max_retained_bytes = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
        }
        #[test]
        fn $work() {
            let error = block_point_refusal($route,
                |policy| policy.limits.max_work_units = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
        }
    };
}

block_point_limit_tests!(
    block_point_refuses_collection_limit,
    block_point_refuses_retained_limit,
    block_point_refuses_work_limit,
    BlockPointRoute::Point
);
block_point_limit_tests!(
    block_point_group_refuses_collection_limit,
    block_point_group_refuses_retained_limit,
    block_point_group_refuses_work_limit,
    BlockPointRoute::Group
);

#[test]
fn block_payload_refuses_collection_limit() {
    let error = block_payload_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn block_payload_refuses_retained_limit() {
    let error = block_payload_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn block_payload_refuses_scoped_limit() {
    let error = block_payload_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn block_payload_refuses_work_limit() {
    let error = block_payload_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

#[test]
fn block_construction_refuses_collection_limit() {
    let error = block_construction_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn block_construction_refuses_retained_limit() {
    let error = block_construction_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn block_construction_refuses_scoped_limit() {
    let error = block_construction_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn block_construction_refuses_work_limit() {
    let error = block_construction_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}
