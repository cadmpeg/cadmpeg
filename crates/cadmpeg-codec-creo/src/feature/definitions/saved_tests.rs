// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::feature::definitions::placement_instruction_rows;
use crate::feature::definitions::positional_saved_section as parse_positional_saved_section;
use crate::feature::definitions::saved_arc_scalar;
use crate::feature::definitions::saved_circular_entities as append_saved_circular_entities;
use crate::feature::definitions::saved_conic_entities as append_saved_conic_entities;
use crate::feature::definitions::saved_line_entities as append_saved_line_entities;
use crate::feature::definitions::saved_positional_generated_entities as append_saved_positional_generated_entities;
use crate::feature::definitions::saved_section as parse_saved_section;
use crate::feature::definitions::saved_section_scalar;
use crate::feature::definitions::saved_spline_entities as append_saved_spline_entities;
use crate::feature::definitions::saved_spline_parameter;
use crate::feature::definitions::variable_table as parse_variable_table;
use crate::feature::definitions::FeatureOrderRow;
use crate::feature::definitions::FeatureOrderTable;
use crate::feature::definitions::FeatureSavedEntity;
use crate::feature::definitions::FeatureSegment;
use crate::feature::definitions::FeatureSegmentKind;
use crate::feature::definitions::FeatureSegmentTable;

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
        }]
        .into(),
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
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            assert!(matches!(generated_arc_with_limits(crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some($operation), |cap| generated_arc_with_limits(cap, u64::MAX)), u64::MAX),
Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == $operation));
            let entities = generated_arc_with_limits(u64::MAX, crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::RetainedBytes, None, |cap| generated_arc_with_limits(u64::MAX, cap))).expect("generated arc admitted");
            let [FeatureSavedEntity::Arc(arc)] = entities.as_slice() else {
                panic!("generated arc");
            };
            assert_eq!(arc.entity_id, 7);
            assert_eq!(arc.body.len(), 14);
        }
    };
}

generated_arc_collection_limit_test!(
    saved_generated_row_start_refuses_before_vec_growth,
    "creo saved generated row starts"
);
generated_arc_collection_limit_test!(
    saved_generated_arc_refuses_before_entity_append,
    "creo saved generated entities"
);

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
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            assert!(matches!(saved_line_with_limits(crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some($operation), |cap| saved_line_with_limits(cap, u64::MAX)), u64::MAX),
Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == $operation));
            let entities = saved_line_with_limits(u64::MAX, u64::MAX).expect("saved line admitted");
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
    "creo saved line references"
);
saved_line_collection_limit_test!(
    saved_line_attributes_refuse_before_vec_growth,
    "creo saved line attributes"
);
saved_line_collection_limit_test!(
    saved_line_block_refuses_before_entity_append,
    "creo saved line block entities"
);

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
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            assert!(matches!(saved_spline_with_limits(crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some($operation), |cap| saved_spline_with_limits(cap, u64::MAX)), u64::MAX),
Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == $operation));
            assert_eq!(saved_spline_with_limits(u64::MAX, u64::MAX).expect("spline admitted").len(), 1);
        }
    };
}

saved_spline_collection_limit_test!(
    saved_spline_points_refuse_before_reserve,
    "creo saved spline points"
);
saved_spline_collection_limit_test!(
    saved_spline_parameters_refuse_before_reserve,
    "creo saved spline parameters"
);
saved_spline_collection_limit_test!(
    saved_spline_entity_refuses_before_append,
    "creo saved spline entities"
);

macro_rules! saved_spline_retained_limit_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            assert!(matches!(saved_spline_with_limits(u64::MAX, crate::test_support::allocation_limit_at(ResourceDimension::RetainedBytes, Some($operation), |cap| saved_spline_with_limits(u64::MAX, cap))),
Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == $operation));
            assert_eq!(saved_spline_with_limits(u64::MAX, crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::RetainedBytes, None, |cap| saved_spline_with_limits(u64::MAX, cap))).expect("spline admitted").len(), 1);
        }
    };
}

saved_spline_retained_limit_test!(
    saved_spline_point_body_refuses_before_copy,
    "creo saved spline point body"
);
saved_spline_retained_limit_test!(
    saved_spline_tangent_body_refuses_before_copy,
    "creo saved spline tangent body"
);
saved_spline_retained_limit_test!(
    saved_spline_parameter_body_refuses_before_copy,
    "creo saved spline parameter body"
);

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

mod variable_coordinates;

fn parse_saved_line_entities(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Result<Vec<FeatureSavedEntity>, CodecError> {
    let mut entities = Vec::new();
    append_saved_line_entities(ctx, payload, start, end, cache, &mut entities)?;
    Ok(entities)
}

fn parse_saved_positional_generated_entities(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
    order_table: Option<&FeatureOrderTable>,
    segments: Option<&FeatureSegmentTable>,
) -> Result<Vec<FeatureSavedEntity>, CodecError> {
    let mut entities = Vec::new();
    append_saved_positional_generated_entities(
        ctx,
        payload,
        start,
        end,
        cache,
        super::SavedEntityTopology::from_tables(order_table, segments),
        &mut entities,
    )?;
    Ok(entities)
}

fn parse_saved_circular_entities(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
    order_table: Option<&FeatureOrderTable>,
    segments: Option<&FeatureSegmentTable>,
) -> Result<Vec<FeatureSavedEntity>, CodecError> {
    let mut entities = Vec::new();
    append_saved_circular_entities(
        ctx,
        payload,
        start,
        end,
        cache,
        super::SavedEntityTopology::from_tables(order_table, segments),
        &mut entities,
    )?;
    Ok(entities)
}

fn parse_saved_conic_entities(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Result<Vec<FeatureSavedEntity>, CodecError> {
    let mut entities = Vec::new();
    append_saved_conic_entities(ctx, payload, start, end, cache, &mut entities)?;
    Ok(entities)
}

fn parse_saved_dummy_entities(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
) -> Result<Vec<FeatureSavedEntity>, CodecError> {
    let mut entities = Vec::new();
    crate::feature::definitions::saved_dummy_entities(ctx, payload, start, end, &mut entities)?;
    Ok(entities)
}

fn parse_saved_spline_entities(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Result<Vec<FeatureSavedEntity>, CodecError> {
    let mut entities = Vec::new();
    append_saved_spline_entities(ctx, payload, start, end, cache, &mut entities)?;
    Ok(entities)
}

mod curves;

mod scalars;

mod lines;

mod variables;

mod spline;

mod placement;
