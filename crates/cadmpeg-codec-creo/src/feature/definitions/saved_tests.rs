// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::feature::definitions::placement_instruction_rows;
use crate::feature::definitions::positional_saved_section as parse_positional_saved_section;
use crate::feature::definitions::saved_arc_scalar;
use crate::feature::definitions::saved_circular_entities as parse_saved_circular_entities;
use crate::feature::definitions::saved_conic_entities as parse_saved_conic_entities;
use crate::feature::definitions::saved_line_entities as parse_saved_line_entities;
use crate::feature::definitions::saved_positional_generated_entities as parse_saved_positional_generated_entities;
use crate::feature::definitions::saved_section as parse_saved_section;
use crate::feature::definitions::saved_section_scalar;
use crate::feature::definitions::saved_spline_entities as parse_saved_spline_entities;
use crate::feature::definitions::saved_spline_parameter;
use crate::feature::definitions::variable_table as parse_variable_table;
use crate::feature::definitions::FeatureOrderRow;
use crate::feature::definitions::FeatureOrderTable;
use crate::feature::definitions::FeatureSavedEntity;
use crate::feature::definitions::FeatureSegment;
use crate::feature::definitions::FeatureSegmentKind;
use crate::feature::definitions::FeatureSegmentTable;

use crate::feature::operations::reference_names;
use crate::feature::operations::FeatureRecipe;
use crate::feature::operations::FeatureReferenceName;
use crate::feature::rows::FeatureFieldValue;
use crate::psb;
use crate::scalar;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn saved_section(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
    order_table: Option<&FeatureOrderTable>,
    segments: Option<&FeatureSegmentTable>,
) -> Option<crate::feature::definitions::FeatureSavedSection> {
    crate::decode::with_test_decode_ctx(|ctx| {
        parse_saved_section(ctx, payload, start, end, cache, order_table, segments)
    })
    .expect("saved section admitted")
}

fn saved_spline_entities(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Vec<FeatureSavedEntity> {
    crate::decode::with_test_decode_ctx(|ctx| {
        parse_saved_spline_entities(ctx, payload, start, end, cache)
    })
    .expect("saved spline entities admitted")
}

fn saved_line_entities(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Vec<FeatureSavedEntity> {
    crate::decode::with_test_decode_ctx(|ctx| {
        parse_saved_line_entities(ctx, payload, start, end, cache)
    })
    .expect("saved line entities admitted")
}

fn saved_circular_entities(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
    order_table: Option<&FeatureOrderTable>,
    segments: Option<&FeatureSegmentTable>,
) -> Vec<FeatureSavedEntity> {
    crate::decode::with_test_decode_ctx(|ctx| {
        parse_saved_circular_entities(ctx, payload, start, end, cache, order_table, segments)
    })
    .expect("saved circular entities admitted")
}

fn saved_conic_entities(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Vec<FeatureSavedEntity> {
    crate::decode::with_test_decode_ctx(|ctx| {
        parse_saved_conic_entities(ctx, payload, start, end, cache)
    })
    .expect("saved conic entities admitted")
}

fn positional_saved_section(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
    order_table: Option<&FeatureOrderTable>,
    segments: Option<&FeatureSegmentTable>,
) -> Option<crate::feature::definitions::FeatureSavedSection> {
    crate::decode::with_test_decode_ctx(|ctx| {
        parse_positional_saved_section(ctx, payload, start, end, cache, order_table, segments)
    })
    .expect("positional saved section admitted")
}

fn saved_positional_generated_entities(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
    order_table: Option<&FeatureOrderTable>,
    segments: Option<&FeatureSegmentTable>,
) -> Vec<FeatureSavedEntity> {
    crate::decode::with_test_decode_ctx(|ctx| {
        parse_saved_positional_generated_entities(
            ctx,
            payload,
            start,
            end,
            cache,
            order_table,
            segments,
        )
    })
    .expect("positional generated entities admitted")
}

fn generated_row_with_limits(
    collection_limit: u64,
    retained_limit: u64,
    kind: FeatureSegmentKind,
    vertical_horizontal: Option<u32>,
    scalar_count: usize,
) -> Result<Vec<FeatureSavedEntity>, CodecError> {
    let mut payload = vec![0xe3, 7, 0xe2];
    payload.extend(vec![0x0f; scalar_count]);
    payload.push(0xe3);
    let order = FeatureOrderTable {
        declared_count: 1,
        has_prototype: false,
        entity_ref: None,
        rows: vec![FeatureOrderRow {
            external_id: 42,
            internal_id: 7,
            bitmask: 0,
            offset: 0,
        }],
        offset: 0,
    };
    let segments = FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: vec![FeatureSegment {
            kind,
            directions: [None; 3],
            center_id: Some(3),
            arc_orientation: Some(0),
            vertical_horizontal,
            radius_ref: None,
            radius2_ref: None,
            external_id: 42,
            body: Vec::new(),
            offset: 0,
        }]
        .into_iter()
        .map(crate::feature::segment_rows::SegmentRow::Ordinary)
        .collect(),
        offset: 0,
    };
    with_saved_leaf_limits(&payload, collection_limit, retained_limit, |ctx| {
        parse_saved_positional_generated_entities(
            ctx,
            &payload,
            0,
            payload.len(),
            &scalar::ScalarCache::default(),
            Some(&order),
            Some(&segments),
        )
    })
}

fn generated_arc_with_limits(
    collection_limit: u64,
    retained_limit: u64,
) -> Result<Vec<FeatureSavedEntity>, CodecError> {
    generated_row_with_limits(
        collection_limit,
        retained_limit,
        FeatureSegmentKind::Arc([1, 2]),
        None,
        12,
    )
}

fn generated_line_with_limits(
    collection_limit: u64,
    retained_limit: u64,
) -> Result<Vec<FeatureSavedEntity>, CodecError> {
    generated_row_with_limits(
        collection_limit,
        retained_limit,
        FeatureSegmentKind::Line([1, 2]),
        Some(0),
        6,
    )
}

macro_rules! generated_arc_collection_limit_test {
    ($name:ident, $limit:expr, $operation:literal) => {
        #[test]
        fn $name() {
            assert!(matches!(generated_arc_with_limits($limit, u64::MAX),
                Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == $operation));
            let entities = generated_arc_with_limits(3, 14).expect("generated arc admitted");
            let [FeatureSavedEntity::Arc(arc)] = entities.as_slice() else {
                panic!("generated arc");
            };
            assert_eq!(arc.entity_id, 7);
            assert_eq!(arc.body.len(), 14);
        }
    };
}

generated_arc_collection_limit_test!(
    saved_generated_segment_node_refuses_before_btree_insertion,
    0,
    "creo saved generated segment nodes"
);
generated_arc_collection_limit_test!(
    saved_generated_row_start_refuses_before_vec_growth,
    1,
    "creo saved generated row starts"
);
generated_arc_collection_limit_test!(
    saved_generated_arc_refuses_before_entity_append,
    2,
    "creo saved generated entities"
);

#[test]
fn saved_generated_arc_body_refuses_before_retained_copy() {
    assert!(
        matches!(generated_arc_with_limits(3, 13), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo saved generated arc body")
    );
    assert_eq!(
        generated_arc_with_limits(3, 14)
            .expect("generated arc admitted")
            .len(),
        1
    );
}

#[test]
fn saved_generated_line_body_refuses_before_retained_copy() {
    assert!(
        matches!(generated_line_with_limits(3, 7), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo saved generated line body")
    );
    let entities = generated_line_with_limits(3, 8).expect("generated line admitted");
    let [FeatureSavedEntity::Line(line)] = entities.as_slice() else {
        panic!("generated line");
    };
    assert_eq!(line.body.len(), 8);
}

fn with_saved_leaf_limits<T>(
    payload: &[u8],
    collection_limit: u64,
    retained_limit: u64,
    run: impl FnOnce(&DecodeContext<'_>) -> Result<T, CodecError>,
) -> Result<T, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
        .expect("saved leaf fits root policy");
    run(&ctx)
}

const SAVED_CIRCULAR_LIMIT_INPUT: &[u8] = b"\xe0\x00entity(arc)\0\xe0\x01id\0\x07\
    \xe0\x00entity(circle)\0\xe0\x01id\0\x08";
const SAVED_CONIC_LIMIT_INPUT: &[u8] = b"\xe0\x00entity(conic)\0\xe0\x01id\0\x02\xe0\x01type\0\x3a";
const SAVED_DUMMY_LIMIT_INPUT: &[u8] = b"\xe0\x00entity(dummy_ent)\0\x07";

fn saved_circular_with_limits(
    collection_limit: u64,
    retained_limit: u64,
) -> Result<Vec<FeatureSavedEntity>, CodecError> {
    with_saved_leaf_limits(
        SAVED_CIRCULAR_LIMIT_INPUT,
        collection_limit,
        retained_limit,
        |ctx| {
            parse_saved_circular_entities(
                ctx,
                SAVED_CIRCULAR_LIMIT_INPUT,
                0,
                SAVED_CIRCULAR_LIMIT_INPUT.len(),
                &scalar::ScalarCache::default(),
                None,
                None,
            )
        },
    )
}

#[test]
fn saved_arc_body_refuses_before_retained_copy() {
    assert!(
        matches!(saved_circular_with_limits(2, 5), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo saved arc body")
    );
    assert_eq!(
        saved_circular_with_limits(2, 12)
            .expect("arc and circle admitted")
            .len(),
        2
    );
}

#[test]
fn saved_circle_body_refuses_before_retained_copy() {
    assert!(
        matches!(saved_circular_with_limits(2, 11), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo saved circle body")
    );
    assert_eq!(
        saved_circular_with_limits(2, 12)
            .expect("arc and circle admitted")
            .len(),
        2
    );
}

#[test]
fn saved_circular_entities_refuse_before_each_append() {
    for limit in [0, 1] {
        assert!(matches!(saved_circular_with_limits(limit, u64::MAX),
            Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "creo saved circular entities"));
    }
    assert_eq!(
        saved_circular_with_limits(2, u64::MAX)
            .expect("arc and circle admitted")
            .len(),
        2
    );
}

#[test]
fn saved_conic_body_refuses_before_retained_copy() {
    let run = |collection, retained| {
        with_saved_leaf_limits(SAVED_CONIC_LIMIT_INPUT, collection, retained, |ctx| {
            parse_saved_conic_entities(
                ctx,
                SAVED_CONIC_LIMIT_INPUT,
                0,
                SAVED_CONIC_LIMIT_INPUT.len(),
                &scalar::ScalarCache::default(),
            )
        })
    };
    assert!(matches!(run(1, 13), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo saved conic body"));
    assert_eq!(run(1, 14).expect("conic admitted").len(), 1);
}

#[test]
fn saved_conic_entity_refuses_before_append() {
    assert!(
        matches!(with_saved_leaf_limits(SAVED_CONIC_LIMIT_INPUT, 0, u64::MAX, |ctx| {
        parse_saved_conic_entities(ctx, SAVED_CONIC_LIMIT_INPUT, 0,
            SAVED_CONIC_LIMIT_INPUT.len(), &scalar::ScalarCache::default())
    }), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo saved conic entities")
    );
    assert_eq!(
        with_saved_leaf_limits(SAVED_CONIC_LIMIT_INPUT, 1, u64::MAX, |ctx| {
            parse_saved_conic_entities(
                ctx,
                SAVED_CONIC_LIMIT_INPUT,
                0,
                SAVED_CONIC_LIMIT_INPUT.len(),
                &scalar::ScalarCache::default(),
            )
        })
        .expect("conic admitted")
        .len(),
        1
    );
}

#[test]
fn saved_dummy_body_refuses_before_retained_copy() {
    assert!(
        matches!(with_saved_leaf_limits(SAVED_DUMMY_LIMIT_INPUT, 1, 0, |ctx| {
        crate::feature::definitions::saved_dummy_entities(ctx, SAVED_DUMMY_LIMIT_INPUT, 0,
            SAVED_DUMMY_LIMIT_INPUT.len())
    }), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo saved dummy body")
    );
    assert_eq!(
        with_saved_leaf_limits(SAVED_DUMMY_LIMIT_INPUT, 1, 1, |ctx| {
            crate::feature::definitions::saved_dummy_entities(
                ctx,
                SAVED_DUMMY_LIMIT_INPUT,
                0,
                SAVED_DUMMY_LIMIT_INPUT.len(),
            )
        })
        .expect("dummy admitted")
        .len(),
        1
    );
}

#[test]
fn saved_dummy_entity_refuses_before_append() {
    assert!(
        matches!(with_saved_leaf_limits(SAVED_DUMMY_LIMIT_INPUT, 0, u64::MAX, |ctx| {
        crate::feature::definitions::saved_dummy_entities(ctx, SAVED_DUMMY_LIMIT_INPUT, 0,
            SAVED_DUMMY_LIMIT_INPUT.len())
    }), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo saved dummy entities")
    );
    assert_eq!(
        with_saved_leaf_limits(SAVED_DUMMY_LIMIT_INPUT, 1, u64::MAX, |ctx| {
            crate::feature::definitions::saved_dummy_entities(
                ctx,
                SAVED_DUMMY_LIMIT_INPUT,
                0,
                SAVED_DUMMY_LIMIT_INPUT.len(),
            )
        })
        .expect("dummy admitted")
        .len(),
        1
    );
}

#[test]
fn positional_saved_section_conic_refuses_before_aggregate_growth() {
    assert!(
        matches!(with_saved_leaf_limits(SAVED_CONIC_LIMIT_INPUT, 1, u64::MAX, |ctx| {
        parse_positional_saved_section(ctx, SAVED_CONIC_LIMIT_INPUT, 0,
            SAVED_CONIC_LIMIT_INPUT.len(), &scalar::ScalarCache::default(), None, None)
    }), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo positional saved section entities")
    );
    assert_eq!(
        with_saved_leaf_limits(SAVED_CONIC_LIMIT_INPUT, 2, u64::MAX, |ctx| {
            parse_positional_saved_section(
                ctx,
                SAVED_CONIC_LIMIT_INPUT,
                0,
                SAVED_CONIC_LIMIT_INPUT.len(),
                &scalar::ScalarCache::default(),
                None,
                None,
            )
        })
        .expect("section admitted")
        .expect("conic section present")
        .entities
        .len(),
        1
    );
}

const SAVED_LINE_LIMIT_INPUT: &[u8] =
    b"\xe0\x00entity(line)\0\xf7\x2a\xeb\x01\x02\x03\x04\x05\x07\xe2\x0f\x0f\x0f\xe3";

fn saved_line_with_limits(
    collection_limit: u64,
    retained_limit: u64,
) -> Result<Vec<FeatureSavedEntity>, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(SAVED_LINE_LIMIT_INPUT, &arena, &policy)
        .expect("saved line fits root policy");
    parse_saved_line_entities(
        &ctx,
        SAVED_LINE_LIMIT_INPUT,
        0,
        SAVED_LINE_LIMIT_INPUT.len(),
        &scalar::ScalarCache::default(),
    )
}

macro_rules! saved_line_collection_limit_test {
    ($name:ident, $limit:expr, $operation:literal) => {
        #[test]
        fn $name() {
            assert!(matches!(saved_line_with_limits($limit, u64::MAX),
                Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == $operation));
            let entities = saved_line_with_limits(4, u64::MAX).expect("saved line admitted");
            let [FeatureSavedEntity::Line(line)] = entities.as_slice() else {
                panic!("saved line");
            };
            assert_eq!(line.references, [42]);
            assert_eq!(line.attributes, [[1, 2, 3, 4, 5]]);
        }
    };
}

saved_line_collection_limit_test!(
    saved_line_references_refuse_before_vec_growth,
    0,
    "creo saved line references"
);
saved_line_collection_limit_test!(
    saved_line_attributes_refuse_before_vec_growth,
    1,
    "creo saved line attributes"
);
saved_line_collection_limit_test!(
    saved_line_block_refuses_before_entity_append,
    2,
    "creo saved line block entities"
);
saved_line_collection_limit_test!(
    saved_line_group_refuses_before_entity_extend,
    3,
    "creo saved line entities"
);

#[test]
fn saved_line_body_refuses_before_retained_copy() {
    assert!(
        matches!(saved_line_with_limits(4, 12), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo saved line body")
    );
    assert_eq!(
        saved_line_with_limits(4, 13)
            .expect("saved line admitted")
            .len(),
        1
    );
}

const SAVED_SPLINE_LIMIT_INPUT: &[u8] = b"\xe0\x00save_entity_ptr(spline)\0\xe3\
    \xe0\x01id\0\x07\
    \xe0\x02i_pnts\0\xf9\x02\x03\
    \xe4\x0f\x0d\x0f\xe4\x0f\
    \xe0\x02end_tangts\0\xf9\x02\x03\
    \xe4\x0f\x0f\xe4\x0f\x0f\
    \xe0\x02params\0\xf8\x02\x0f\xe4\
    \xe0\x01tan_cond\0\x00";

fn saved_spline_with_limits(
    collection_limit: u64,
    retained_limit: u64,
) -> Result<Vec<FeatureSavedEntity>, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(SAVED_SPLINE_LIMIT_INPUT, &arena, &policy)
        .expect("spline input fits root policy");
    parse_saved_spline_entities(
        &ctx,
        SAVED_SPLINE_LIMIT_INPUT,
        0,
        SAVED_SPLINE_LIMIT_INPUT.len(),
        &scalar::ScalarCache::default(),
    )
}

macro_rules! saved_spline_collection_limit_test {
    ($name:ident, $limit:expr, $operation:literal) => {
        #[test]
        fn $name() {
            assert!(matches!(saved_spline_with_limits($limit, u64::MAX),
                Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == $operation));
            assert_eq!(saved_spline_with_limits(5, u64::MAX).expect("spline admitted").len(), 1);
        }
    };
}

saved_spline_collection_limit_test!(
    saved_spline_points_refuse_before_reserve,
    1,
    "creo saved spline points"
);
saved_spline_collection_limit_test!(
    saved_spline_parameters_refuse_before_reserve,
    3,
    "creo saved spline parameters"
);
saved_spline_collection_limit_test!(
    saved_spline_entity_refuses_before_append,
    4,
    "creo saved spline entities"
);

macro_rules! saved_spline_retained_limit_test {
    ($name:ident, $limit:expr, $operation:literal) => {
        #[test]
        fn $name() {
            assert!(matches!(saved_spline_with_limits(u64::MAX, $limit),
                Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == $operation));
            assert_eq!(saved_spline_with_limits(u64::MAX, 22).expect("spline admitted").len(), 1);
        }
    };
}

saved_spline_retained_limit_test!(
    saved_spline_point_body_refuses_before_copy,
    8,
    "creo saved spline point body"
);
saved_spline_retained_limit_test!(
    saved_spline_tangent_body_refuses_before_copy,
    17,
    "creo saved spline tangent body"
);
saved_spline_retained_limit_test!(
    saved_spline_parameter_body_refuses_before_copy,
    21,
    "creo saved spline parameter body"
);

#[test]
fn saved_section_entity_refuses_before_aggregate_growth() {
    let mut payload = b"\xe0\x00p_saved_result\0".to_vec();
    payload.extend_from_slice(SAVED_SPLINE_LIMIT_INPUT);
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy)
            .expect("saved section fits root policy");
        parse_saved_section(
            &ctx,
            &payload,
            0,
            payload.len(),
            &scalar::ScalarCache::default(),
            None,
            None,
        )
    };
    assert!(matches!(run(5), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo saved section entities"));
    assert_eq!(
        run(6)
            .expect("section admitted")
            .expect("section present")
            .entities
            .len(),
        1
    );
}

fn variable_table(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<crate::feature::definitions::FeatureVariableTable> {
    crate::decode::with_test_decode_ctx(|ctx| parse_variable_table(ctx, payload, start, end, cache))
        .expect("variable table admitted")
}

fn variable_row_with_limits(
    collection_limit: u64,
    retained_limit: u64,
) -> Result<crate::feature::definitions::FeatureVariableTable, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let payload = b"var_arr\0\xf8\x01\xf7\x77\xfb\xe2\xf1\xf7\x77\xe2\
            \x00\x41\x18\x20\x96\x61\x01\x01\x82\x06\xe2";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)?;
    Ok(parse_variable_table(
        &ctx,
        payload,
        0,
        payload.len(),
        &scalar::ScalarCache::default(),
    )?
    .expect("complete variable table"))
}

#[test]
fn named_variable_row_vec_refuses_before_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    assert_eq!(
        variable_row_with_limits(1, u64::MAX)
            .expect("one row admitted")
            .rows
            .len(),
        1
    );
    let error = variable_row_with_limits(0, u64::MAX).expect_err("one row needs one item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo variable rows"));
}

#[test]
fn named_variable_value_body_refuses_before_retention() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let error = variable_row_with_limits(1, 0).expect_err("value needs one byte");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo variable value body"));
}

#[test]
fn named_variable_guess_body_refuses_before_retention() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let error = variable_row_with_limits(1, 1).expect_err("guess needs three bytes");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo variable guess body"));
}

fn operation_states(payload: &[u8]) -> Vec<crate::feature::operations::FeatureOperationState> {
    crate::decode::with_test_decode_ctx(|ctx| {
        crate::feature::operations::operation_states(ctx, payload)
    })
    .expect("feature operation states are admitted")
}

fn operations(payload: &[u8]) -> Vec<crate::feature::operations::FeatureOperation> {
    crate::decode::with_test_decode_ctx(|ctx| crate::feature::operations::operations(ctx, payload))
        .expect("feature operations are admitted")
}

fn field_value(payload: &[u8]) -> crate::feature::rows::FeatureFieldValue {
    crate::decode::with_test_decode_ctx(|ctx| crate::feature::rows::field_value(ctx, payload))
        .expect("field value is admitted")
}

#[test]
fn saved_line_accepts_bare_entity_reference_before_coordinates() {
    let payload = b"\xe0\0entity(line)\0\x05\xe2\xf7\x2a\
            \x2f\x20\0\x2f\x20\0\x2f\x20\0\
            \x2f\x20\0\x2f\x20\0\x2f\x20\0\xf1\xf7\x2b\xe3";
    let entities = saved_line_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());

    assert_eq!(entities.len(), 1);
    let FeatureSavedEntity::Line(line) = &entities[0] else {
        panic!("expected saved line");
    };
    assert_eq!(line.entity_id, 5);
    assert_eq!(line.references, [42, 43]);
    assert_eq!(line.endpoints, [[Some(8.0); 3]; 2]);
    let body_start = b"\xe0\0entity(line)\0".len();
    assert_eq!(line.body, payload[body_start..payload.len() - 1]);
}

#[test]
fn saved_line_expands_compact_basis_triple() {
    let payload = b"\xe0\0entity(line)\0\x05\xe2\x18\xe5\x2f\x20\0\x2f\x20\0\x2f\x20\0\xe3";
    let entities = saved_line_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
    let FeatureSavedEntity::Line(line) = &entities[0] else {
        panic!("expected saved line");
    };
    assert_eq!(
        line.endpoints,
        [
            [Some(0.0), Some(1.0), Some(0.0)],
            [Some(8.0), Some(8.0), Some(8.0)]
        ]
    );
    let body_start = b"\xe0\0entity(line)\0".len();
    assert_eq!(line.body, payload[body_start..payload.len() - 1]);
}

#[test]
fn saved_line_replay_continues_after_point_prototype() {
    let scalar_triple = b"\x2f\x20\0\x2f\x20\0\x2f\x20\0";
    let mut payload = b"\xe0\0entity(line)\0\x05\xe2".to_vec();
    payload.extend_from_slice(scalar_triple);
    payload.extend_from_slice(scalar_triple);
    payload.push(0xe3);
    payload.extend_from_slice(b"\xe0\0entity(point)\0\xe0\x01id\0\x04\xf1\xf7\x2a\xe3\x06\xe2");
    payload.extend_from_slice(scalar_triple);
    payload.extend_from_slice(scalar_triple);
    payload.extend_from_slice(b"\xe0\0entity(arc)\0");

    let entities = saved_line_entities(&payload, 0, payload.len(), &scalar::ScalarCache::default());

    assert_eq!(entities.len(), 2);
    assert_eq!(
        entities
            .iter()
            .filter_map(|entity| match entity {
                FeatureSavedEntity::Line(line) => Some(line.entity_id),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [5, 6]
    );
}

#[test]
fn saved_line_accepts_named_record_boundary() {
    let payload = b"\xe0\0entity(line)\0\x03\xe2\xf1\xf7\x80\xc4\
            \x48\x20\0\x46\x15\xff\xff\xff\xff\xff\x8f\x18\
            \x48\x1e\0\x46\x15\xff\xff\xff\xff\xff\x8f\x18\x8a\x01\x02\x03\x04\x05\x0f\
            \xe0\0entity(point)\0\xf1\xf7\x2a\xe3\xe0\0entity(arc)\0";
    let entities = saved_line_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());

    assert_eq!(entities.len(), 1);
    let FeatureSavedEntity::Line(line) = &entities[0] else {
        panic!("expected saved line");
    };
    assert_eq!(line.entity_id, 3);
    assert_eq!(line.references, [196]);
    let body_start = b"\xe0\0entity(line)\0".len();
    let body_end = payload[body_start..]
        .windows(b"\xe0\0entity(point)\0".len())
        .position(|window| window == b"\xe0\0entity(point)\0")
        .map(|relative| body_start + relative)
        .expect("point boundary");
    assert_eq!(line.body, payload[body_start..body_end]);
}

#[test]
fn saved_line_retains_its_identity_and_coordinate_prefix() {
    let payload = b"\xe0\0entity(line)\0\x07\xe2\x0f\x0f\x0f\
            \xe0\0entity(arc)\0";

    let entities = saved_line_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());

    let [FeatureSavedEntity::Line(line)] = entities.as_slice() else {
        panic!("saved line");
    };
    assert_eq!(line.entity_id, 7);
    assert_eq!(
        line.endpoints,
        [[Some(0.0), Some(0.0), Some(0.0)], [None; 3]]
    );
}

#[test]
fn saved_section_retains_an_empty_named_table() {
    let payload = b"\xe0\0p_saved_result\0\xe0\x02local_sys\0";

    let section = saved_section(
        payload,
        0,
        payload.len(),
        &scalar::ScalarCache::default(),
        None,
        None,
    )
    .expect("saved section header");

    assert_eq!(section.offset, 0);
    assert!(section.entities.is_empty());
}

#[test]
fn saved_section_41_form_occupies_eight_bytes() {
    let bytes = [0x41, 0xfd, 0x6b, 0xf1, 0xa1, 0xc2, 0x1f, 0xf0];
    let (value, next) =
        saved_section_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default());
    assert_eq!(next, bytes.len());
    assert_eq!(
        value,
        Some(f64::from_be_bytes([
            0x3f, 0xfd, 0x6b, 0xf1, 0xa1, 0xc2, 0x1f, 0xf0
        ]))
    );
}

#[test]
fn saved_section_zero_does_not_consume_named_record_opener() {
    let mut section = Vec::new();
    for index in 0_u16..=224 {
        section.extend_from_slice(&[
            0x46,
            0x08,
            u8::try_from(index >> 8).expect("fixture value fits u8"),
            u8::try_from(index).expect("fixture value fits u8"),
            0,
            0,
            0,
            0,
        ]);
    }
    let cache = scalar::ScalarCache::from_section(&section);

    assert_eq!(
        saved_section_scalar(&[0x18, 0xe0], 0, 2, &cache),
        (Some(0.0), 1)
    );
}

#[test]
fn saved_section_consecutive_zero_slots_remain_distinct() {
    let cache = scalar::ScalarCache::default();
    let bytes = [0x18, 0x18, 0x81, 0, 0, 0, 0, 0, 0];
    assert_eq!(
        saved_section_scalar(&bytes, 0, bytes.len(), &cache),
        (Some(0.0), 1)
    );
    assert_eq!(
        saved_section_scalar(&bytes, 1, bytes.len(), &cache),
        (Some(0.0), 2)
    );
}

#[test]
fn saved_section_dd_form_supplies_ieee_high_bytes() {
    let bytes = [0xdd, 0xe6, 0x8a, 0x84, 0x79, 0xd0, 0x62];
    assert_eq!(
        saved_section_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
        (
            Some(f64::from_be_bytes([
                0x40, 0x0c, 0xe6, 0x8a, 0x84, 0x79, 0xd0, 0x62,
            ])),
            7,
        )
    );
}

#[test]
fn saved_section_negative_dict_forms_supply_ieee_high_bytes() {
    for (bytes, head) in [
        ([0xb3, 1, 2, 3, 4, 5, 6], [0xbf, 0xe0]),
        ([0xcb, 1, 2, 3, 4, 5, 6], [0xbf, 0xf8]),
        ([0xd6, 1, 2, 3, 4, 5, 6], [0xc0, 0x04]),
    ] {
        assert_eq!(
            saved_section_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
            (
                Some(f64::from_be_bytes([
                    head[0], head[1], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
                ])),
                7,
            )
        );
    }
}

#[test]
fn saved_arc_negative_dict_forms_supply_ieee_high_bytes() {
    for (bytes, head) in [
        ([0x9b, 1, 2, 3, 4, 5, 6], [0x40, 0x10]),
        ([0x9c, 1, 2, 3, 4, 5, 6], [0x40, 0x11]),
        ([0x9d, 1, 2, 3, 4, 5, 6], [0x40, 0x12]),
        ([0x9e, 1, 2, 3, 4, 5, 6], [0x40, 0x13]),
        ([0x9f, 1, 2, 3, 4, 5, 6], [0x40, 0x14]),
        ([0xa0, 1, 2, 3, 4, 5, 6], [0x40, 0x15]),
        ([0x5e, 1, 2, 3, 4, 5, 6], [0x3f, 0xd3]),
        ([0x60, 1, 2, 3, 4, 5, 6], [0x3f, 0xd5]),
        ([0x64, 1, 2, 3, 4, 5, 6], [0x3f, 0xd9]),
        ([0xad, 1, 2, 3, 4, 5, 6], [0x3f, 0xd9]),
        ([0xcc, 1, 2, 3, 4, 5, 6], [0xbf, 0xf9]),
        ([0xd0, 1, 2, 3, 4, 5, 6], [0xbf, 0xfe]),
        ([0xd2, 1, 2, 3, 4, 5, 6], [0xc0, 0x00]),
        ([0xd5, 1, 2, 3, 4, 5, 6], [0xc0, 0x03]),
        ([0xde, 1, 2, 3, 4, 5, 6], [0xc0, 0x10]),
        ([0xdf, 1, 2, 3, 4, 5, 6], [0xc0, 0x11]),
    ] {
        let expected = f64::from_be_bytes([
            head[0], head[1], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
        ]);
        assert_eq!(
            saved_arc_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
            (Some(expected), 7)
        );
    }
    let d5 = [0xd5, 1, 2, 3, 4, 5, 6];
    assert_eq!(
        saved_section_scalar(&d5, 0, d5.len(), &scalar::ScalarCache::default()),
        (Some(f64::from_be_bytes([0xbf, 1, 2, 3, 4, 5, 6, 0])), 7)
    );
}

#[test]
fn saved_arc_28_form_supplies_ieee_high_byte() {
    let bytes = [0x28, 1, 2, 3, 4, 5, 6, 7];
    assert_eq!(
        saved_arc_scalar(&bytes, 0, bytes.len(), &scalar::ScalarCache::default()),
        (Some(f64::from_be_bytes([0x3f, 1, 2, 3, 4, 5, 6, 7])), 8)
    );
}

#[test]
fn saved_arc_zero_does_not_consume_arc_scalar_opener() {
    let bytes = [0x18, 0x5e, 1, 2, 3, 4, 5, 6];
    let cache = scalar::ScalarCache::default();
    assert_eq!(
        saved_arc_scalar(&bytes, 0, bytes.len(), &cache),
        (Some(0.0), 1)
    );
    assert_eq!(
        saved_arc_scalar(&bytes, 1, bytes.len(), &cache),
        (Some(f64::from_be_bytes([0x3f, 0xd3, 1, 2, 3, 4, 5, 6])), 8)
    );
}

#[test]
fn saved_circular_entities_retain_ids_and_independent_fields() {
    let payload = b"\xe0\x00entity(arc)\0\
            \xe0\x01id\0\x07\xe0\x02center\0\x0f\x0f\x0f\
            \xe0\x00entity(circle)\0\
            \xe0\x01id\0\x08\xe0\x02radius\0\x0f";

    let entities = saved_circular_entities(
        payload,
        0,
        payload.len(),
        &scalar::ScalarCache::default(),
        None,
        None,
    );

    let [FeatureSavedEntity::Arc(arc), FeatureSavedEntity::Circle(circle)] = entities.as_slice()
    else {
        panic!("saved circular entities");
    };
    assert_eq!(arc.entity_id, 7);
    assert_eq!(arc.center, [Some(0.0); 3]);
    assert_eq!(arc.radius, None);
    assert_eq!(arc.endpoints, [[None; 3]; 2]);
    assert_eq!(arc.parameters, [None; 2]);
    let arc_body_start = b"\xe0\x00entity(arc)\0".len();
    let circle_label = b"\xe0\x00entity(circle)\0";
    let circle_offset = payload
        .windows(circle_label.len())
        .position(|window| window == circle_label)
        .expect("circle boundary");
    assert_eq!(arc.body, payload[arc_body_start..circle_offset]);
    assert_eq!(circle.entity_id, 8);
    assert_eq!(circle.center, [None; 3]);
    assert_eq!(circle.radius, Some(0.0));
    assert_eq!(circle.body, payload[circle_offset + circle_label.len()..]);
}

#[test]
fn saved_conic_retains_coefficients_parameters_and_planar_frame() {
    let payload = b"\xe0\x00entity(conic)\0\
            \xe0\x01id\0\x02\xe0\x01type\0\x3a\
            \xe0\x02end1\0\xf8\x03\x18\xe5\
            \xe0\x02end2\0\xf8\x03\x18\xe5\
            \xe0\x02t0\0\x0f\xe0\x02t1\0\xf6\
            \xe0\x02c1\0\xe4\xe0\x02c2\0\xe4\
            \xe0\x02local_sys\0\xf9\x04\x03\
            \xe4\x0f\x0f\x0f\xe4\x18\xe5\x0f\x0f\x0f\x0f\
            \xe0\x01trailing_field\0\x07";

    let entities = saved_conic_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
    let [FeatureSavedEntity::Conic(conic)] = entities.as_slice() else {
        panic!("one saved conic");
    };

    assert_eq!(conic.entity_id, 2);
    assert_eq!(conic.endpoints, [[Some(0.0), Some(1.0), Some(0.0)]; 2]);
    assert_eq!(conic.parameters, [Some(0.0), None]);
    assert_eq!(conic.coefficients, [Some(1.0); 2]);
    assert_eq!(
        conic.local_system.map(cadmpeg_ir::units::FiniteVector::get),
        Some([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0])
    );
    assert_eq!(conic.body, payload[b"\xe0\x00entity(conic)\0".len()..]);
}

#[test]
fn saved_arc_replay_uses_order_table_row_boundaries() {
    let mut payload = vec![0xe3, 7, 0xe2];
    payload.extend([0x0f; 12]);
    payload.push(0xe3);
    let order = FeatureOrderTable {
        declared_count: 1,
        has_prototype: false,
        entity_ref: None,
        rows: vec![FeatureOrderRow {
            external_id: 42,
            internal_id: 7,
            bitmask: 0,
            offset: 0,
        }],
        offset: 0,
    };
    let segments = FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: (vec![FeatureSegment {
            kind: FeatureSegmentKind::Arc([1, 2]),
            directions: [None; 3],
            center_id: Some(3),
            arc_orientation: Some(0),
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 42,
            body: Vec::new(),
            offset: 0,
        }])
        .into_iter()
        .map(crate::feature::segment_rows::SegmentRow::Ordinary)
        .collect(),
        offset: 0,
    };

    let entities = saved_positional_generated_entities(
        &payload,
        0,
        payload.len(),
        &scalar::ScalarCache::default(),
        Some(&order),
        Some(&segments),
    );

    assert_eq!(entities.len(), 1);
    let FeatureSavedEntity::Arc(arc) = &entities[0] else {
        panic!("expected saved arc");
    };
    assert_eq!(arc.entity_id, 7);
    assert_eq!(arc.center, [Some(0.0); 3]);
    assert_eq!(arc.radius, Some(0.0));
    assert_eq!(arc.body, payload[1..payload.len() - 1]);
    let mut incomplete_segments = segments.clone();
    incomplete_segments.declared_count += 1;
    let incomplete_entities = saved_positional_generated_entities(
        &payload,
        0,
        payload.len(),
        &scalar::ScalarCache::default(),
        Some(&order),
        Some(&incomplete_segments),
    );
    assert_eq!(incomplete_entities, entities);
    let section = positional_saved_section(
        &payload,
        0,
        payload.len(),
        &scalar::ScalarCache::default(),
        Some(&order),
        Some(&segments),
    )
    .expect("positional saved section");
    assert_eq!(section.entities.len(), 1);
    assert_eq!(section.offset, 1);

    let named_prefix = b"\xe0\x00entity(arc)\0\xe0\x01id\0\x09";
    let mut named_payload = named_prefix.to_vec();
    named_payload.extend_from_slice(&payload);
    let named_entities = saved_circular_entities(
        &named_payload,
        0,
        named_payload.len(),
        &scalar::ScalarCache::default(),
        Some(&order),
        Some(&segments),
    );
    let [FeatureSavedEntity::Arc(named), FeatureSavedEntity::Arc(replay)] =
        named_entities.as_slice()
    else {
        panic!("named arc and replay");
    };
    assert_eq!(
        named.body, b"\xe0\x01id\0\x09",
        "named body must stop before the replay separator"
    );
    assert_eq!(replay.body, payload[1..payload.len() - 1]);
}

#[test]
fn saved_arc_replay_retains_a_structurally_terminated_scalar_prefix() {
    let mut payload = vec![0xe3, 7, 0xe2];
    payload.extend([0x0f; 6]);
    payload.push(0xe3);
    let order = FeatureOrderTable {
        declared_count: 1,
        has_prototype: false,
        entity_ref: None,
        rows: vec![FeatureOrderRow {
            external_id: 42,
            internal_id: 7,
            bitmask: 0,
            offset: 0,
        }],
        offset: 0,
    };
    let segments = FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: (vec![FeatureSegment {
            kind: FeatureSegmentKind::Arc([1, 2]),
            directions: [None; 3],
            center_id: Some(3),
            arc_orientation: Some(0),
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id: 42,
            body: Vec::new(),
            offset: 0,
        }])
        .into_iter()
        .map(crate::feature::segment_rows::SegmentRow::Ordinary)
        .collect(),
        offset: 0,
    };

    let entities = saved_positional_generated_entities(
        &payload,
        0,
        payload.len(),
        &scalar::ScalarCache::default(),
        Some(&order),
        Some(&segments),
    );

    let [FeatureSavedEntity::Arc(arc)] = entities.as_slice() else {
        panic!("expected saved arc");
    };
    assert_eq!(arc.entity_id, 7);
    assert_eq!(arc.center, [Some(0.0); 3]);
    assert_eq!(arc.radius, Some(0.0));
    assert_eq!(arc.endpoints[0], [Some(0.0), Some(0.0), None]);
    assert_eq!(arc.endpoints[1], [None; 3]);
    assert_eq!(arc.parameters, [None; 2]);
}

#[test]
fn saved_generated_line_requires_its_orientation_invariant() {
    let payload = [0xe3, 8, 0xe2, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0x0f, 0xe3];
    let order = FeatureOrderTable {
        declared_count: 1,
        has_prototype: false,
        entity_ref: None,
        rows: vec![FeatureOrderRow {
            external_id: 43,
            internal_id: 8,
            bitmask: 0,
            offset: 0,
        }],
        offset: 0,
    };
    let segments = FeatureSegmentTable {
        declared_count: 1,
        has_elided_prototype: false,
        entity_ref: None,
        rows: (vec![FeatureSegment {
            kind: FeatureSegmentKind::Line([1, 2]),
            directions: [None; 3],
            center_id: None,
            arc_orientation: Some(0),
            vertical_horizontal: Some(1),
            radius_ref: None,
            radius2_ref: None,
            external_id: 43,
            body: Vec::new(),
            offset: 0,
        }])
        .into_iter()
        .map(crate::feature::segment_rows::SegmentRow::Ordinary)
        .collect(),
        offset: 0,
    };

    let entities = saved_positional_generated_entities(
        &payload,
        0,
        payload.len(),
        &scalar::ScalarCache::default(),
        Some(&order),
        Some(&segments),
    );

    assert_eq!(entities.len(), 1);
    let FeatureSavedEntity::Line(line) = &entities[0] else {
        panic!("expected saved line");
    };
    assert_eq!(line.entity_id, 8);
    assert_eq!(line.endpoints[0], [Some(0.0); 3]);
    assert_eq!(line.endpoints[1], [Some(1.0), Some(0.0), Some(0.0)]);
    assert_eq!(line.body, payload[1..payload.len() - 1]);
}

#[test]
fn decodes_mdlstatus_recipe_discriminators_within_their_records() {
    let payload = b"\xe3icon\0protextrude\0Protrusion id 40\0\xe2\xe3\
            icon\0protrevolve\0Revolve id 41\0\xe2\xe3\
            icon\0cutextrude\0Cut id 42\0\xe2\xe3\
            icon\0cutrevolve\0Cut id 43\0\xe2\xe3Datum Plane id 44\0\xe3K\xc3\xb6rper ID 45\0";
    let operations = operations(payload);
    assert_eq!(operations.len(), 6);
    assert_eq!(
        operations[0].recipe.resolved(),
        Some(FeatureRecipe::ProtrudeExtrude)
    );
    assert_eq!(
        operations[1].recipe.resolved(),
        Some(FeatureRecipe::ProtrudeRevolve)
    );
    assert_eq!(
        operations[2].recipe.resolved(),
        Some(FeatureRecipe::CutExtrude)
    );
    assert_eq!(
        operations[3].recipe.resolved(),
        Some(FeatureRecipe::CutRevolve)
    );
    assert_eq!(
        operations[4].recipe,
        crate::feature::operations::RecipeResolution::None
    );
    assert_eq!(operations[5].kind.as_str(), "Körper");
    assert_eq!(operations[5].feature_id, 45);
}

#[test]
fn preserves_mdlstatus_name_prefixes_without_using_them_as_state_selectors() {
    let payload = b"\xe3oExtrude id 7\0\xe3xExtrude id 7\0\xe3yExtrude id 7\0\xe3zExtrude ID 7\0";

    let states = operation_states(payload);
    assert_eq!(states.len(), 4);
    for (state, (prefix, expected_name)) in states.iter().zip([
        (b'o', "oExtrude id 7"),
        (b'x', "xExtrude id 7"),
        (b'y', "yExtrude id 7"),
        (b'z', "zExtrude ID 7"),
    ]) {
        assert_eq!(state.feature_id, 7);
        assert_eq!(state.kind.as_str(), "Extrude");
        assert_eq!(state.stored_name_prefix(), Some(prefix));
        assert!(state.display_state_conflict);
        assert_eq!(state.state_offset + 1, state.offset);
        assert_eq!(state.stored_name().as_deref(), Some(expected_name));
    }
    assert_eq!(states[3].name.identifier_keyword(), Some("ID"));

    let current_operations = operations(payload);
    let [current] = current_operations.as_slice() else {
        panic!("one current operation");
    };
    assert_eq!(current.kind.as_str(), "Extrude");
    assert!(!current.display_name_stored());
    assert_eq!(current.stored_name(), None);
    assert_eq!(current.name.stored_name_bytes(), None);
    assert_eq!(current.name.identifier_keyword(), None);
    assert_eq!(current.stored_name_prefix(), None);
    assert!(current.display_state_conflict);
}

#[test]
fn conflicting_inline_recipes_across_display_states_remain_conflicting() {
    let payload = b"\xe3protextrude\0Extrude id 7\0\xe3cutextrude\0Extrude id 7\0";

    let states = operation_states(payload);
    assert_eq!(states.len(), 2);
    assert_eq!(
        states[0].recipe.resolved(),
        Some(FeatureRecipe::ProtrudeExtrude)
    );
    assert_eq!(states[1].recipe.resolved(), Some(FeatureRecipe::CutExtrude));

    let current_operations = operations(payload);
    let [current] = current_operations.as_slice() else {
        panic!("one consensus operation");
    };
    assert_eq!(current.kind.as_str(), "Extrude");
    assert!(current.display_state_conflict);
    assert!(current.recipe.is_conflicting());
    assert_eq!(
        current.recipe,
        crate::feature::operations::RecipeResolution::Conflicting
    );
}

#[test]
fn binds_depdb_recipe_records_to_compact_feature_ids() {
    let payload = b"\xe3K\xc3\xb6rper ID 247\0\xe3\
            \xf7\x3b\x80\xf7\x83\x95\xf6\x20Drehen 1\0\xf6\0protrevolve\0\
            \xe3Body ID 8053\0\xe3\
            \xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0";

    let operations = operations(payload);
    assert_eq!(operations.len(), 2);
    assert_eq!(operations[0].feature_id, 247);
    assert_eq!(
        operations[0].recipe.resolved(),
        Some(FeatureRecipe::ProtrudeRevolve)
    );
    assert_eq!(
        operations[0]
            .root_schema_class()
            .map(crate::feature::schema::SchemaClass::code),
        Some(917)
    );
    assert_eq!(operations[0].parent_feature_id(), Some(32));
    assert_eq!(operations[1].feature_id, 8053);
    assert_eq!(
        operations[1].recipe.resolved(),
        Some(FeatureRecipe::ProtrudeExtrude)
    );
    assert_eq!(
        operations[1]
            .root_schema_class()
            .map(crate::feature::schema::SchemaClass::code),
        Some(917)
    );
    assert_eq!(operations[1].parent_feature_id(), Some(8051));
}

#[test]
fn preserves_competing_depdb_recipe_bindings() {
    let payload = b"\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0\
            \xf7\x50\x9f\x75\x83\x94\xf6\x9f\x73Profile 2\0\xf6\0cutextrude\0";

    let states = operation_states(payload);
    assert_eq!(states.len(), 2);
    assert_eq!(states[0].feature_id, 8053);
    assert_eq!(
        states[0].recipe.candidate(),
        Some(FeatureRecipe::ProtrudeExtrude)
    );
    assert!(states[0].recipe.is_conflicting());
    assert_eq!(
        states[0]
            .root_schema_class()
            .map(crate::feature::schema::SchemaClass::code),
        Some(917)
    );
    assert_eq!(states[1].feature_id, 8053);
    assert_eq!(
        states[1].recipe.candidate(),
        Some(FeatureRecipe::CutExtrude)
    );
    assert!(states[1].recipe.is_conflicting());
    assert_eq!(
        states[1]
            .root_schema_class()
            .map(crate::feature::schema::SchemaClass::code),
        Some(916)
    );

    let current = operations(payload);
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].feature_id, 8053);
    assert_eq!(current[0].kind.as_str(), "Native Feature");
    assert_eq!(
        current[0].recipe,
        crate::feature::operations::RecipeResolution::Conflicting
    );
    assert!(current[0].recipe.is_conflicting());
    assert_eq!(
        current[0]
            .root_schema_class()
            .map(crate::feature::schema::SchemaClass::code),
        None
    );
    assert_eq!(current[0].parent_feature_id(), None);

    let repeated = b"\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0\
            \xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 2\0\xf6\0protextrude\0";
    let repeated_states = operation_states(repeated);
    assert_eq!(repeated_states.len(), 2);
    assert_eq!(repeated_states[0].recipe, repeated_states[1].recipe);
    assert_eq!(
        repeated_states[0].recipe.resolved(),
        Some(FeatureRecipe::ProtrudeExtrude)
    );
    assert_ne!(repeated_states[0].offset, repeated_states[1].offset);
    let repeated_current = operations(repeated);
    assert_eq!(repeated_current.len(), 1);
    assert_eq!(repeated_current[0].kind.as_str(), "Extrude");
    assert_eq!(
        repeated_current[0].recipe.resolved(),
        Some(FeatureRecipe::ProtrudeExtrude)
    );
    assert_eq!(
        repeated_current[0]
            .root_schema_class()
            .map(crate::feature::schema::SchemaClass::code),
        Some(917)
    );
    assert_eq!(repeated_current[0].parent_feature_id(), Some(8051));
}

#[test]
fn conflicting_bindings_do_not_use_an_inline_recipe_fallback() {
    let payload = b"\xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0\
            \xf7\x50\x9f\x75\x83\x94\xf6\x9f\x73Profile 2\0\xf6\0cutextrude\0\
            \xe3icon\0protextrude\0Extrude id 8053\0";

    let states = operation_states(payload);
    let display = states
        .iter()
        .find(|state| state.display_name_stored())
        .expect("stored display state");
    assert_eq!(display.kind.as_str(), "Extrude");
    assert_eq!(display.recipe.candidate(), None);
    assert!(display.recipe.is_conflicting());
    assert_eq!(
        display
            .root_schema_class()
            .map(crate::feature::schema::SchemaClass::code),
        None
    );
    assert_eq!(display.parent_feature_id(), None);

    let current = operations(payload);
    let [current] = current.as_slice() else {
        panic!("one current operation");
    };
    assert_eq!(current.kind.as_str(), "Extrude");
    assert!(current.display_name_stored());
    assert_eq!(
        current.recipe,
        crate::feature::operations::RecipeResolution::Conflicting
    );
    assert!(current.recipe.is_conflicting());
}

#[test]
fn leaves_inline_recipe_conflicts_unresolved() {
    let payload = b"\xe3icon\0protextrude\0cutextrude\0Extrude id 9\0";

    let states = operation_states(payload);
    let [state] = states.as_slice() else {
        panic!("one operation state");
    };
    assert_eq!(state.feature_id, 9);
    assert_eq!(state.kind.as_str(), "Extrude");
    assert_eq!(state.recipe.candidate(), None);
    assert!(state.recipe.is_conflicting());
    assert_eq!(
        state
            .root_schema_class()
            .map(crate::feature::schema::SchemaClass::code),
        None
    );
    assert_eq!(state.parent_feature_id(), None);
}

#[test]
fn promotes_depdb_recipe_without_operation_display_name() {
    let payload = b"\xe3\
            \xf7\x50\x9f\x75\x83\x95\xf6\x9f\x73Profile 1\0\xf6\0protextrude\0";

    let operations = operations(payload);
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0].feature_id, 8053);
    assert_eq!(operations[0].kind.as_str(), "Extrude");
    assert_eq!(
        operations[0].recipe.resolved(),
        Some(FeatureRecipe::ProtrudeExtrude)
    );
    assert_eq!(
        operations[0]
            .root_schema_class()
            .map(crate::feature::schema::SchemaClass::code),
        Some(917)
    );
    assert_eq!(operations[0].parent_feature_id(), Some(8051));
    assert_eq!(operations[0].offset, 1);
}

#[test]
fn decodes_count_bounded_saved_spline_interpolation_points() {
    let payload = b"\xe0\x00save_entity_ptr(spline)\0\xe3\
            \xe0\x01id\0\x07\
            \xe0\x02i_pnts\0\xf9\x02\x03\
            \xe4\x0f\x0d\x0f\xe4\x0f\
            \xe0\x02end_tangts\0\xf9\x02\x03\
            \xe4\x0f\x0f\xe4\x0f\x0f\
            \xe0\x02params\0\xf8\x02\x0f\xe4\
            \xe0\x01tan_cond\0\x00";

    let entities =
        saved_spline_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
    let [FeatureSavedEntity::Spline(spline)] = entities.as_slice() else {
        panic!("saved spline");
    };
    assert_eq!(spline.entity_id, Some(7));
    assert_eq!(spline.declared_point_count, Some(2));
    assert_eq!(
        spline.interpolation_points,
        [[1.0, 0.0, -1.0], [0.0, 1.0, 0.0]]
    );
    assert_eq!(
        spline.interpolation_points_body,
        b"\xf9\x02\x03\xe4\x0f\x0d\x0f\xe4\x0f"
    );
    assert_eq!(
        spline.endpoint_tangents.as_ref().map(|field| field.value),
        Some([[1.0, 0.0, 0.0], [1.0, 0.0, 0.0]])
    );
    assert_eq!(
        spline
            .endpoint_tangents
            .as_ref()
            .map(|field| field.body.as_slice()),
        Some(b"\xf9\x02\x03\xe4\x0f\x0f\xe4\x0f\x0f".as_slice())
    );
    assert_eq!(
        spline.parameters.as_ref().map(|field| field.value.clone()),
        Some(vec![0.0, 1.0])
    );
    assert_eq!(
        spline
            .parameters
            .as_ref()
            .map(|field| field.body.as_slice()),
        Some(b"\xf8\x02\x0f\xe4".as_slice())
    );
}

#[test]
fn decodes_compact_saved_spline_point_count() {
    let mut payload = b"\xe0\x00save_entity_ptr(spline)\0\xe3\
            \xe0\x01id\0\x07\
            \xe0\x02i_pnts\0\xf9\x80\x88\x03"
        .to_vec();
    payload.extend(std::iter::repeat_n(0x0f, 136 * 3));

    let entities =
        saved_spline_entities(&payload, 0, payload.len(), &scalar::ScalarCache::default());
    let [FeatureSavedEntity::Spline(spline)] = entities.as_slice() else {
        panic!("saved spline");
    };
    assert_eq!(spline.declared_point_count, Some(136));
    assert_eq!(spline.interpolation_points.len(), 136);
    assert_eq!(
        spline.interpolation_points_body,
        payload[b"\xe0\x00save_entity_ptr(spline)\0\xe3\xe0\x01id\0\x07\xe0\x02i_pnts\0".len()..]
    );
    assert!(spline
        .interpolation_points
        .iter()
        .all(|point| *point == [0.0; 3]));
}

#[test]
fn saved_spline_retains_its_declared_count_and_complete_point_prefix() {
    let payload = b"\xe0\x00save_entity_ptr(spline)\0\xe3\
            \xe0\x01id\0\x07\
            \xe0\x02i_pnts\0\xf9\x02\x03\
            \x0f\x0f\x0f\xe0\x01tan_cond\0\x00";

    let entities =
        saved_spline_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
    let [FeatureSavedEntity::Spline(spline)] = entities.as_slice() else {
        panic!("saved spline");
    };

    assert_eq!(spline.entity_id, Some(7));
    assert_eq!(spline.declared_point_count, Some(2));
    assert_eq!(spline.interpolation_points, [[0.0; 3]]);
    assert_eq!(
        spline.interpolation_points_body,
        b"\xf9\x02\x03\x0f\x0f\x0f"
    );
    assert_eq!(spline.endpoint_tangents, None);
    assert_eq!(spline.parameters, None);
}

#[test]
fn saved_spline_retains_its_identity_without_a_point_table() {
    let payload = b"\xe0\x00save_entity_ptr(spline)\0\xe3\xe0\x01id\0\x07";

    let entities =
        saved_spline_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
    let [FeatureSavedEntity::Spline(spline)] = entities.as_slice() else {
        panic!("saved spline");
    };

    assert_eq!(spline.entity_id, Some(7));
    assert_eq!(spline.declared_point_count, None);
    assert!(spline.interpolation_points.is_empty());
    assert!(spline.interpolation_points_body.is_empty());
}

#[test]
fn saved_spline_retains_a_valid_point_wrapper_when_allocation_is_rejected() {
    let payload = b"\xe0\x00save_entity_ptr(spline)\0\xe3\xe0\x02i_pnts\0\xf9\xbf\xff\x03";

    let entities =
        saved_spline_entities(payload, 0, payload.len(), &scalar::ScalarCache::default());
    let [FeatureSavedEntity::Spline(spline)] = entities.as_slice() else {
        panic!("saved spline");
    };

    assert_eq!(spline.declared_point_count, Some(16_383));
    assert!(spline.interpolation_points.is_empty());
    assert_eq!(spline.interpolation_points_body, b"\xf9\xbf\xff\x03");
}

#[test]
fn decodes_compact_feature_scalar_array_extents() {
    let mut payload = vec![psb::token::SCALAR_BODY, 0x80, 0x88, 0x03];
    payload.extend(std::iter::repeat_n(0x0f, 136 * 3));

    let FeatureFieldValue::ScalarArray {
        dimensions,
        count,
        body,
        decoded_values,
    } = field_value(&payload)
    else {
        panic!("scalar array");
    };
    assert_eq!(dimensions, 136);
    assert_eq!(count, 3);
    assert_eq!(body.len(), 408);
    assert_eq!(decoded_values, Some(vec![0.0; 408]));
}

#[test]
fn decodes_saved_spline_chord_parameter_lane() {
    let body = [
        0x18, 0x6d, 0x31, 0xd2, 0x2a, 0x7f, 0x68, 0x39, 0x85, 0x06, 0x5f, 0x25, 0x83, 0xf4, 0x6c,
        0x93, 0xd8, 0xd4, 0xfb, 0x45, 0xbc, 0x38, 0x9e, 0x51, 0xef, 0x1e, 0x96, 0xe2, 0x6c, 0x2d,
        0x1a, 0xfc, 0x59, 0x51, 0xbd, 0x0a, 0x38,
    ];
    let cache = scalar::ScalarCache::default();
    let expected = [
        0.0,
        0.568_581_660_273_827_7,
        1.626_555_582_565_994_3,
        3.105_874_980_035_448_4,
        4.830_013_730_963_952,
        6.746_434_476_054_269,
    ];
    let mut cursor = 0;
    for expected in expected {
        let (value, next) = saved_spline_parameter(&body, cursor, &cache).expect("parameter");
        assert_eq!(value, expected);
        cursor = next;
    }
    assert_eq!(cursor, body.len());
}

#[test]
fn decodes_zero_offset_positional_placement_instruction() {
    let payload = b"place_instruction_ptrs\0\xf8\x03\xf7\x0b\xfb\xe3\
            \xf1\xf7\x0b\xe3\xc0\x4e\x9f\x18\xf6\xf6\x02\xf6\x00\x00\x00\xe6";
    let rows = placement_instruction_rows(payload, 1000).collect::<Vec<_>>();
    let [row] = rows.as_slice() else {
        panic!("placement row");
    };
    assert_eq!(row.kind, 20_127);
    assert!(row.zero_offset);
    assert_eq!(row.dimension_id, None);
    assert_eq!(row.reference_id, None);
    assert_eq!(row.geometry1_id, Some(2));
    assert_eq!(row.geometry2_id, None);
    assert_eq!([row.member1, row.member2], [0, 0]);
    assert_eq!(row.offset, 1029);
}

#[test]
fn model_reference_entry_joins_feature_name_to_feature_id() {
    let payload = b"\0\xf7\x71\x2a\x05\x29Datum Plane id 41\0\x2a\x2a\x10\0\
            \xf7\x71\x30\x05\x2fBroken\0\x30\x31";

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(payload, &arena, &policy)
        .expect("root input is admitted");
    let names = reference_names(&ctx, payload).expect("reference names");
    assert_eq!(
        names,
        [FeatureReferenceName {
            feature_id: 41,
            name_bytes: b"Datum Plane id 41".to_vec(),
            own_reference_id: 42,
            reference_type: 5,
            offset: 1,
        }]
    );
    assert_eq!(names[0].name(), "Datum Plane id 41");
}

mod variable_coordinates;
