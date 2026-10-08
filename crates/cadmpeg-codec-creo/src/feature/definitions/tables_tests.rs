// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::feature::definitions::decode_variable_scalar as decode_variable_scalar_checked;
use crate::feature::definitions::definitions;
use crate::feature::definitions::definitions_in_ranges;
use crate::feature::definitions::depdb_definitions;
use crate::feature::definitions::dimension_table as parse_dimension_table;
use crate::feature::definitions::equation_table as parse_equation_table;
use crate::feature::definitions::feature_relation_triples as parse_feature_relation_triples;
use crate::feature::definitions::feature_skamps as parse_feature_skamps;
use crate::feature::definitions::named_solver_table_header;

use crate::feature::definitions::positional_dimension as parse_positional_dimension;
use crate::feature::definitions::positional_dimension_table as parse_positional_dimension_table;
use crate::feature::definitions::positional_feature_skamps as parse_positional_feature_skamps;

use crate::feature::definitions::positional_relation_table as parse_positional_relation_table;
use crate::feature::definitions::positional_relation_triples as parse_positional_relation_triples;
use crate::feature::definitions::positional_section_3d as parse_positional_section_3d;

use crate::feature::definitions::positional_variable_table as parse_positional_variable_table;
use crate::feature::definitions::relation_table as parse_relation_table;
use crate::feature::definitions::section_3d as parse_section_3d;
use crate::feature::definitions::self_described_positional_dimension_table as parse_self_described_positional_dimension_table;
use crate::feature::definitions::test_support::{with_points, reconciled_points, with_trim_limits};

use crate::feature::definitions::variable_table as parse_variable_table;
use crate::feature::definitions::BinaryFlag;
use crate::feature::definitions::FeatureDimensionReference;

use crate::feature::definitions::FeatureSectionOrientation;
use crate::feature::definitions::FeatureSectionPoint;

use crate::feature::definitions::FeatureSkampItem;
use crate::feature::definitions::FeatureVariableRow;
use crate::feature::definitions::FeatureVariableTable;
use crate::feature::definitions::ReferencePlanes;
use crate::feature::definitions::ScalarLane;

use crate::psb;
use crate::scalar;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;





const NAMED_DIMENSION_LIMIT_INPUT: &[u8] = b"dimtab_ptr\0\xf3\xf8\x01\xf7\x58\xfb\xe2\
    \xe0\x01type\0\x02\xe0\x02value\0\x18\xe0\x01direct\0\x00\
    \xe0\x02aux_value\0\x18\xe0\x01ext_id\0\x02\
    dim_ref\0\xf1\xf8\x02\xf7\x60\xfb\xe2\
    \xe0\x01item_id\0\x0d\xe0\x01sense\0\x00\
    \xe0\x01point\0\xf8\x02\x03\xe4\
    \xf1\xf7\x60\xe2\x02\x02\x14\xe4\xf3\xf7\x58\xe2";

fn with_dimension_limits<T>(
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
        .expect("dimension input fits root policy");
    run(&ctx)
}

fn named_dimension_with_limits(collection_limit: u64, retained_limit: u64) -> Result<Option<crate::feature::definitions::FeatureDimensionTable>, CodecError> {
    with_dimension_limits(
        NAMED_DIMENSION_LIMIT_INPUT,
        collection_limit,
        retained_limit,
        |ctx| {
            parse_dimension_table(
                ctx,
                NAMED_DIMENSION_LIMIT_INPUT,
                0,
                NAMED_DIMENSION_LIMIT_INPUT.len(),
                &scalar::ScalarCache::default(),
            )
        },
    )
}

macro_rules! named_dimension_collection_limit_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            crate::test_support::assert_refusal_order(ResourceDimension::CollectionItems,
                &["creo dimension reference rows", "creo dimension reference rows", "creo dimension rows"],
                |cap| named_dimension_with_limits(cap, u64::MAX));
            assert!(matches!(named_dimension_with_limits(crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some($operation), |cap| named_dimension_with_limits(cap, u64::MAX)), u64::MAX),
Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == $operation));
            let table = named_dimension_with_limits(u64::MAX, crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::RetainedBytes, None, |cap| named_dimension_with_limits(u64::MAX, cap)))
                .expect("dimension admitted").expect("dimension present");
            assert_eq!(table.rows[0].references.as_ref().expect("references").rows.len(), 2);
        }
    };
}

named_dimension_collection_limit_test!(
    dimension_reference_prototype_refuses_before_row_append,
    "creo dimension reference rows"
);
named_dimension_collection_limit_test!(
    dimension_reference_replay_refuses_before_row_append,
    "creo dimension reference rows"
);
named_dimension_collection_limit_test!(
    named_dimension_row_refuses_before_append,
    "creo dimension rows"
);





const POSITIONAL_DIMENSION_LIMIT_INPUT: &[u8] = &[1, 0x00, 0x04, 0xa6, 0, 0x18, 44];



const POSITIONAL_DIMENSION_TABLE_LIMIT_INPUT: &[u8] = b"prefix\xf8\x02\xf7\x58\xfb\xe2\xf7\x59\
    \x02\xe4\x00\x18\x2b\xf3\xf7\x58\xe2\x02\xe4\x00\x18\x2c";

fn positional_dimension_table_with_limit(
    collection_limit: u64,
) -> Result<Option<crate::feature::definitions::FeatureDimensionTable>, CodecError> {
    with_dimension_limits(
        POSITIONAL_DIMENSION_TABLE_LIMIT_INPUT,
        collection_limit,
        u64::MAX,
        |ctx| {
            parse_positional_dimension_table(
                ctx,
                POSITIONAL_DIMENSION_TABLE_LIMIT_INPUT,
                0,
                POSITIONAL_DIMENSION_TABLE_LIMIT_INPUT.len(),
                88,
                &scalar::ScalarCache::default(),
            )
        },
    )
}







fn dimension_table(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<crate::feature::definitions::FeatureDimensionTable> {
    crate::decode::with_test_decode_ctx(|ctx| {
        parse_dimension_table(ctx, payload, start, end, cache)
    })
    .expect("dimension table admitted")
}

fn positional_dimension(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<crate::feature::definitions::FeatureDimension> {
    crate::decode::with_test_decode_ctx(|ctx| {
        parse_positional_dimension(ctx, payload, start, end, cache)
    })
    .expect("positional dimension admitted")
}

fn positional_dimension_table(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
    cache: &scalar::ScalarCache,
) -> Option<crate::feature::definitions::FeatureDimensionTable> {
    crate::decode::with_test_decode_ctx(|ctx| {
        parse_positional_dimension_table(ctx, payload, start, end, table_class, cache)
    })
    .expect("positional dimension table admitted")
}

fn self_described_positional_dimension_table(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<crate::feature::definitions::FeatureDimensionTable> {
    crate::decode::with_test_decode_ctx(|ctx| {
        parse_self_described_positional_dimension_table(ctx, payload, start, end, cache)
    })
    .expect("self-described dimension table admitted")
}

fn positional_section_3d(
    payload: &[u8],
    start: usize,
    end: usize,
) -> Option<crate::feature::definitions::FeatureSection3d> {
    crate::decode::with_test_decode_ctx(|ctx| parse_positional_section_3d(ctx, payload, start, end))
        .expect("positional section admitted")
}

fn equation_table(
    payload: &[u8],
    start: usize,
    end: usize,
) -> Option<crate::feature::definitions::FeatureEquationTable> {
    crate::decode::with_test_decode_ctx(|ctx| parse_equation_table(ctx, payload, start, end))
        .expect("equation table admitted")
}

fn feature_skamps(
    payload: &[u8],
    start: usize,
    end: usize,
) -> Vec<crate::feature::definitions::FeatureSkamp> {
    crate::decode::with_test_decode_ctx(|ctx| parse_feature_skamps(ctx, payload, start, end))
        .expect("skamps admitted")
}

fn positional_feature_skamps(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
) -> Vec<crate::feature::definitions::FeatureSkamp> {
    crate::decode::with_test_decode_ctx(|ctx| {
        parse_positional_feature_skamps(ctx, payload, start, end, table_class).map(|table| match table { Some(crate::feature::definitions::SolverSubtable::Declared { rows, .. }) => rows, None => Vec::new(), Some(crate::feature::definitions::SolverSubtable::Unframed(_)) => panic!("positional solver table is declared") })
    })
    .expect("positional skamps admitted")
}

fn feature_relation_triples(
    payload: &[u8],
    start: usize,
    end: usize,
) -> Vec<crate::feature::definitions::FeatureRelationTriple> {
    crate::decode::with_test_decode_ctx(|ctx| {
        parse_feature_relation_triples(ctx, payload, start, end)
    })
    .expect("triples admitted")
}

fn positional_relation_triples(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
) -> Vec<crate::feature::definitions::FeatureRelationTriple> {
    crate::decode::with_test_decode_ctx(|ctx| {
        parse_positional_relation_triples(ctx, payload, start, end, table_class).map(|table| match table { Some(crate::feature::definitions::SolverSubtable::Declared { rows, .. }) => rows, None => Vec::new(), Some(crate::feature::definitions::SolverSubtable::Unframed(_)) => panic!("positional solver table is declared") })
    })
    .expect("positional triples admitted")
}

fn relation_table(
    payload: &[u8],
    start: usize,
    end: usize,
) -> Option<crate::feature::definitions::FeatureRelationTable> {
    crate::decode::with_test_decode_ctx(|ctx| parse_relation_table(ctx, payload, start, end))
        .expect("relation table admitted")
}

fn positional_relation_table(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
) -> Option<crate::feature::definitions::FeatureRelationTable> {
    crate::decode::with_test_decode_ctx(|ctx| {
        parse_positional_relation_table(ctx, payload, start, end, table_class)
    })
    .expect("positional relation table admitted")
}

const POSITIONAL_RELATION_LIMIT_INPUT: &[u8] = b"prefix\xf8\x03\xf7\x64\xfb\xe2\xf7\x65\
    prototype\xf1\xf7\x64\xe2\
    \x08\x00\x03\x0f\xf6\xe4\x01\xe4\x00\xe4\x0f\x10\x0f\x18\x00\xf6\x00\xe2";

fn positional_relation_with_limits(collection_limit: u64, retained_limit: u64) -> Result<Option<crate::feature::definitions::FeatureRelationTable>, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(POSITIONAL_RELATION_LIMIT_INPUT, &arena, &policy)
        .expect("relation input fits root policy");
    parse_positional_relation_table(
        &ctx,
        POSITIONAL_RELATION_LIMIT_INPUT,
        0,
        POSITIONAL_RELATION_LIMIT_INPUT.len(),
        100,
    )
}







const POSITIONAL_SECTION_LIMIT_INPUT: &[u8] = b"prefix\x07S2D0004\0\x01\xf6\xe1\xf6\x82\x01\xf6\
    \xf8\x02\xf7\x39\xfb\xe2\xf7\x3a\
    \x06\x05\xf6\x03\xf6\x00\xe3tail\xf2\xf7\x39\xe2\
    \x07\x05\xf6\x04\xf6\x01";



const NAMED_SECTION_LIMIT_INPUT: &[u8] = b"\xe0\x00gsec3d_ptr\0\
    \xe0\x00ref_planes\0\xf8\x01\xf7\x01\xfb\xe2\
    dim_id_tab\0\xf8\x01\x2a";





fn variable_table(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<FeatureVariableTable> {
    crate::decode::with_test_decode_ctx(|ctx| parse_variable_table(ctx, payload, start, end, cache))
        .expect("variable table admitted")
}

fn positional_variable_table(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
    cache: &scalar::ScalarCache,
) -> Option<FeatureVariableTable> {
    crate::decode::with_test_decode_ctx(|ctx| {
        parse_positional_variable_table(ctx, payload, start, end, table_class, cache)
    })
    .expect("variable table admitted")
}

fn positional_variable_rows_with_limits(collection_limit: u64, retained_limit: u64) -> Result<FeatureVariableTable, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let payload = b"prefix\xf8\x02\xf7\x77\xfb\xe2\xf7\x78\
            \x01\x07\x18\x18\x01\x00\x09\xf1\xf7\x77\xe2\
            \x02\x07\x18\x18\x01\x00\x0a";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)?;
    Ok(parse_positional_variable_table(
        &ctx,
        payload,
        0,
        payload.len(),
        119,
        &scalar::ScalarCache::default(),
    )?
    .expect("complete positional variable table"))
}























































const EQUATION_LIMIT_INPUT: &[u8] = b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
    \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
    \x01\x04\x11\x12\xf6\xe2";

fn equation_with_limits(collection_limit: u64, retained_limit: u64) -> Result<Option<crate::feature::definitions::FeatureEquationTable>, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(EQUATION_LIMIT_INPUT, &arena, &policy)
        .expect("equation root admitted");
    parse_equation_table(&ctx, EQUATION_LIMIT_INPUT, 0, EQUATION_LIMIT_INPUT.len())
}































































mod order;

fn decode_variable_scalar(payload: &[u8], offset: usize, end: usize, cache: &scalar::ScalarCache) -> (ScalarLane, usize) {
    crate::decode::with_test_decode_ctx(|ctx| decode_variable_scalar_checked(ctx, payload, offset, end, cache))
        .expect("scalar lane admission")
}

mod dimensions;

mod solver;

mod sections;

mod variables;
