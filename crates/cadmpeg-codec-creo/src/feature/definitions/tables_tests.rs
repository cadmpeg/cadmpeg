// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::feature::definitions::decode_variable_scalar as decode_variable_scalar_checked;
use crate::feature::definitions::definitions;
use crate::feature::definitions::definitions_in_ranges;
use crate::feature::definitions::depdb_definitions;
use crate::feature::definitions::dimension_table as parse_dimension_table;
use crate::feature::definitions::entity_intersection as parse_entity_intersection;
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
use crate::feature::definitions::test_support::with_points;

use crate::feature::definitions::variable_table as parse_variable_table;
use crate::feature::definitions::BinaryFlag;
use crate::feature::definitions::FeatureDimensionReference;

use crate::feature::definitions::FeatureSectionOrientation;
use crate::feature::definitions::FeatureSectionPoint;

use crate::feature::definitions::FeatureSegmentTable;
use crate::feature::definitions::FeatureSkampItem;
use crate::feature::definitions::FeatureVariableRow;
use crate::feature::definitions::FeatureVariableTable;
use crate::feature::definitions::ReferencePlanes;
use crate::feature::definitions::ScalarLane;

use crate::psb;
use crate::scalar;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn entity_intersection(
    entity_ids: &[u32],
    segments: Option<&FeatureSegmentTable>,
    variables: Option<&FeatureVariableTable>,
) -> Option<[f64; 2]> {
    crate::decode::with_test_decode_ctx(|ctx| {
        parse_entity_intersection(ctx, entity_ids, segments, variables)
    })
    .expect("test trim intersection")
}

fn reconciled_points(
    variables: &FeatureVariableTable,
) -> (
    std::collections::BTreeMap<u32, [Option<f64>; 2]>,
    std::collections::BTreeSet<u32>,
) {
    crate::decode::with_test_decode_ctx(|ctx| {
        variables
            .reconciled_points(ctx)
            .map(|result| (result.points, result.ambiguous))
    })
    .expect("test point reconciliation")
}

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

fn named_dimension_with_limits(
    collection_limit: u64,
    retained_limit: u64,
) -> Result<Option<crate::feature::definitions::FeatureDimensionTable>, CodecError> {
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
    ($name:ident, $limit:expr, $operation:literal) => {
        #[test]
        fn $name() {
            assert!(matches!(named_dimension_with_limits($limit, u64::MAX),
                Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == $operation));
            let table = named_dimension_with_limits(3, crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::RetainedBytes, None, |cap| named_dimension_with_limits(u64::MAX, cap)))
                .expect("dimension admitted").expect("dimension present");
            assert_eq!(table.rows[0].references.as_ref().expect("references").rows.len(), 2);
        }
    };
}

named_dimension_collection_limit_test!(
    dimension_reference_prototype_refuses_before_row_append,
    0,
    "creo dimension reference rows"
);
named_dimension_collection_limit_test!(
    dimension_reference_replay_refuses_before_row_append,
    1,
    "creo dimension reference rows"
);
named_dimension_collection_limit_test!(
    named_dimension_row_refuses_before_append,
    2,
    "creo dimension rows"
);

#[test]
fn named_dimension_value_body_refuses_before_copy() {
    assert!(
        matches!(named_dimension_with_limits(3, crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo dimension value body"), |cap| named_dimension_with_limits(3, cap))), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo dimension value body")
    );
    assert_eq!(
        named_dimension_with_limits(
            3,
            crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                None,
                |cap| named_dimension_with_limits(u64::MAX, cap)
            )
        )
        .expect("dimension admitted")
        .expect("dimension present")
        .rows
        .len(),
        1
    );
}

#[test]
fn named_dimension_auxiliary_body_refuses_before_copy() {
    assert!(
        matches!(named_dimension_with_limits(3, crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo dimension auxiliary body"), |cap| named_dimension_with_limits(3, cap))), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo dimension auxiliary body")
    );
    assert_eq!(
        named_dimension_with_limits(
            3,
            crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                None,
                |cap| named_dimension_with_limits(u64::MAX, cap)
            )
        )
        .expect("dimension admitted")
        .expect("dimension present")
        .rows
        .len(),
        1
    );
}

const POSITIONAL_DIMENSION_LIMIT_INPUT: &[u8] = &[1, 0x00, 0x04, 0xa6, 0, 0x18, 44];

#[test]
fn positional_dimension_unresolved_token_refuses_before_copy() {
    let run = |limit| {
        with_dimension_limits(POSITIONAL_DIMENSION_LIMIT_INPUT, u64::MAX, limit, |ctx| {
            parse_positional_dimension(
                ctx,
                POSITIONAL_DIMENSION_LIMIT_INPUT,
                0,
                POSITIONAL_DIMENSION_LIMIT_INPUT.len(),
                &scalar::ScalarCache::default(),
            )
        })
    };
    assert!(matches!(run(5), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo dimension unresolved token"));
    assert_eq!(
        run(7)
            .expect("dimension admitted")
            .expect("row present")
            .value
            .unresolved_token(),
        Some(&[0x00, 0x04, 0xa6][..])
    );
}

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

#[test]
fn positional_dimension_rows_refuse_before_each_append() {
    for limit in [0, 1] {
        assert!(matches!(positional_dimension_table_with_limit(limit),
            Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "creo dimension rows"));
    }
    let table = positional_dimension_table_with_limit(2)
        .expect("dimension table admitted")
        .expect("table present");
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[0].external_id, 43);
    assert_eq!(table.rows[1].external_id, 44);
}

#[test]
fn positional_dimension_value_body_refuses_before_copy() {
    assert!(matches!(with_dimension_limits(
        POSITIONAL_DIMENSION_LIMIT_INPUT, u64::MAX, 2, |ctx| {
            parse_positional_dimension(ctx, POSITIONAL_DIMENSION_LIMIT_INPUT, 0,
                POSITIONAL_DIMENSION_LIMIT_INPUT.len(), &scalar::ScalarCache::default())
        }), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo dimension value body"));
}

#[test]
fn positional_dimension_auxiliary_body_refuses_before_copy() {
    assert!(matches!(with_dimension_limits(
        POSITIONAL_DIMENSION_LIMIT_INPUT, u64::MAX, 6, |ctx| {
            parse_positional_dimension(ctx, POSITIONAL_DIMENSION_LIMIT_INPUT, 0,
                POSITIONAL_DIMENSION_LIMIT_INPUT.len(), &scalar::ScalarCache::default())
        }), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo dimension auxiliary body"));
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
        parse_positional_feature_skamps(ctx, payload, start, end, table_class)
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
        parse_positional_relation_triples(ctx, payload, start, end, table_class)
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

fn positional_relation_with_limits(
    collection_limit: u64,
    retained_limit: u64,
) -> Result<Option<crate::feature::definitions::FeatureRelationTable>, CodecError> {
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

#[test]
fn relation_operands_refuse_before_retained_copy() {
    assert!(
        matches!(positional_relation_with_limits(1, crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo relation operands"), |cap| positional_relation_with_limits(1, cap))), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo relation operands")
    );
    let table = positional_relation_with_limits(
        1,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            None,
            |cap| positional_relation_with_limits(u64::MAX, cap),
        ),
    )
    .expect("relation admitted")
    .expect("table present");
    assert_eq!(table.rows[0].operands.len(), 12);
}

#[test]
fn relation_row_body_refuses_before_retained_copy() {
    assert!(
        matches!(positional_relation_with_limits(1, crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo relation row body"), |cap| positional_relation_with_limits(1, cap))), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo relation row body")
    );
    let table = positional_relation_with_limits(
        1,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            None,
            |cap| positional_relation_with_limits(u64::MAX, cap),
        ),
    )
    .expect("relation admitted")
    .expect("table present");
    assert_eq!(table.rows[0].body.len(), 17);
}

#[test]
fn relation_row_refuses_before_vec_growth() {
    assert!(
        matches!(positional_relation_with_limits(0, crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::RetainedBytes, None, |cap| positional_relation_with_limits(u64::MAX, cap))), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo relation rows")
    );
    assert_eq!(
        positional_relation_with_limits(
            1,
            crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                None,
                |cap| positional_relation_with_limits(u64::MAX, cap)
            )
        )
        .expect("relation admitted")
        .expect("table present")
        .rows
        .len(),
        1
    );
}

const POSITIONAL_SECTION_LIMIT_INPUT: &[u8] = b"prefix\x07S2D0004\0\x01\xf6\xe1\xf6\x82\x01\xf6\
    \xf8\x02\xf7\x39\xfb\xe2\xf7\x3a\
    \x06\x05\xf6\x03\xf6\x00\xe3tail\xf2\xf7\x39\xe2\
    \x07\x05\xf6\x04\xf6\x01";

#[test]
fn positional_section_reference_planes_refuse_before_each_row() {
    let run = |limit| {
        with_trim_limits(limit, u64::MAX, |ctx| {
            parse_positional_section_3d(
                ctx,
                POSITIONAL_SECTION_LIMIT_INPUT,
                0,
                POSITIONAL_SECTION_LIMIT_INPUT.len(),
            )
        })
    };
    for limit in [0, 1] {
        assert!(matches!(run(limit), Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "creo positional section reference planes"));
    }
    let section = run(2).expect("section admitted").expect("section present");
    assert_eq!(
        section.reference_planes.entity_ids().collect::<Vec<_>>(),
        [6, 7]
    );
}

const NAMED_SECTION_LIMIT_INPUT: &[u8] = b"\xe0\x00gsec3d_ptr\0\
    \xe0\x00ref_planes\0\xf8\x01\xf7\x01\xfb\xe2\
    dim_id_tab\0\xf8\x01\x2a";

#[test]
fn named_section_reference_plane_refuses_before_vec_growth() {
    let run = |limit| {
        with_trim_limits(limit, u64::MAX, |ctx| {
            parse_section_3d(
                ctx,
                NAMED_SECTION_LIMIT_INPUT,
                0,
                NAMED_SECTION_LIMIT_INPUT.len(),
            )
        })
    };
    assert!(matches!(run(0), Err(CodecError::ResourceLimit(refusal))
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "creo named section reference planes"));
    assert_eq!(
        run(2)
            .expect("section admitted")
            .expect("section present")
            .reference_planes
            .entity_ids()
            .collect::<Vec<_>>(),
        [1]
    );
}

#[test]
fn named_section_dimension_id_refuses_before_vec_growth() {
    let run = |limit| {
        with_trim_limits(limit, u64::MAX, |ctx| {
            parse_section_3d(
                ctx,
                NAMED_SECTION_LIMIT_INPUT,
                0,
                NAMED_SECTION_LIMIT_INPUT.len(),
            )
        })
    };
    assert!(matches!(run(1), Err(CodecError::ResourceLimit(refusal))
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "creo section dimension IDs"));
    assert_eq!(
        run(2)
            .expect("section admitted")
            .expect("section present")
            .dimension_ids,
        [42]
    );
}

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

fn positional_variable_rows_with_limits(
    collection_limit: u64,
    retained_limit: u64,
) -> Result<FeatureVariableTable, cadmpeg_core::CodecError> {
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

#[test]
fn positional_variable_row_capacity_refuses_before_reservation() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    assert_eq!(
        positional_variable_rows_with_limits(2, u64::MAX)
            .expect("two rows admitted")
            .rows
            .len(),
        2
    );
    let error =
        positional_variable_rows_with_limits(1, u64::MAX).expect_err("two slots need two items");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo variable rows"));
}

#[test]
fn positional_variable_value_body_refuses_before_retention() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let error = positional_variable_rows_with_limits(
        2,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo variable value body"),
            |cap| positional_variable_rows_with_limits(2, cap),
        ),
    )
    .expect_err("value needs one byte");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo variable value body"));
}

#[test]
fn positional_variable_guess_body_refuses_before_retention() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let error = positional_variable_rows_with_limits(
        2,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo variable guess body"),
            |cap| positional_variable_rows_with_limits(2, cap),
        ),
    )
    .expect_err("guess needs one byte");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo variable guess body"));
}

#[test]
fn positional_dimension_table_uses_the_inherited_table_class() {
    let mut payload = b"prefix\xf8\x02\xf7\x58\xfb\xe2\xf7\x59".to_vec();
    payload.extend_from_slice(&[2, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0, 0x18, 43]);
    payload.extend_from_slice(b"\xf3\xf7\x58\xe2");
    payload.extend_from_slice(&[10, 0x60, 0xc8, 0x1e, 0x15, 0xd4, 0xaf, 0x9f, 0, 0x18, 44]);
    let cache = scalar::ScalarCache::from_section(&payload);

    let dimensions = positional_dimension_table(&payload, 0, payload.len(), 88, &cache)
        .expect("positional dimtab");

    assert_eq!(dimensions.declared_count, 2);
    assert_eq!(dimensions.entity_ref, Some(88));
    assert_eq!(dimensions.rows.len(), 2);
    assert_eq!(dimensions.rows[0].value.resolved(), Some(3.0));
    assert_eq!(
        dimensions.rows[0].value_body,
        [0x46, 0x08, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(dimensions.rows[0].auxiliary_body, [0x18]);
    assert_eq!(dimensions.rows[0].external_id, 43);
    assert_eq!(dimensions.rows[1].dimension_type, 10);
    assert_eq!(
        dimensions.rows[1].value_body,
        [0x60, 0xc8, 0x1e, 0x15, 0xd4, 0xaf, 0x9f]
    );
    assert_eq!(dimensions.rows[1].external_id, 44);
}

#[test]
fn named_dimension_retains_nested_dimension_references() {
    let payload = b"dimtab_ptr\0\xf3\xf8\x01\xf7\x58\xfb\xe2\
            \xe0\x01type\0\x02\xe0\x02value\0\x18\xe0\x01direct\0\x00\
            \xe0\x02aux_value\0\x18\xe0\x01ext_id\0\x02\
            dim_ref\0\xf1\xf8\x02\xf7\x60\xfb\xe2\
            \xe0\x01item_id\0\x0d\xe0\x01sense\0\x00\
            \xe0\x01point\0\xf8\x02\x03\xe4\
            \xf1\xf7\x60\xe2\x02\x02\x14\xe4\xf3\xf7\x58\xe2";
    let cache = scalar::ScalarCache::from_section(payload);

    let dimensions =
        dimension_table(payload, 0, payload.len(), &cache).expect("named dimension table");
    let references = dimensions.rows[0]
        .references
        .as_ref()
        .expect("nested dimension references");

    assert_eq!(references.declared_count, 2);
    assert_eq!(references.entity_ref, Some(0x60));
    assert_eq!(references.rows.len(), 2);
    assert_eq!(
        references.rows[0],
        FeatureDimensionReference {
            item_id: Some(13),
            sense: Some(0),
            point: [Some(3), Some(1)],
            offset: payload
                .windows(b"item_id\0".len())
                .position(|window| window == b"item_id\0")
                .expect("item_id offset"),
        }
    );
    assert_eq!(references.rows[1].item_id, Some(2));
    assert_eq!(references.rows[1].sense, Some(2));
    assert_eq!(references.rows[1].point, [Some(20), Some(1)]);
}

#[test]
fn positional_dimension_table_is_self_describing_when_multiple_rows_close() {
    let mut payload = b"prefix\xf8\x04\xf7\x58\xfb\xe2\xf7\x59".to_vec();
    for (index, row) in [
        [1, 0xe4, 0, 0x18, 2],
        [2, 0x0e, 0, 0x18, 0],
        [2, 0xe4, 0, 0x18, 3],
        [2, 0xe4, 0, 0x18, 1],
    ]
    .into_iter()
    .enumerate()
    {
        payload.extend_from_slice(&row);
        if index < 3 {
            payload.extend_from_slice(b"\xf3\xf7\x58\xe2");
        }
    }
    let cache = scalar::ScalarCache::from_section(&payload);

    let dimensions = self_described_positional_dimension_table(&payload, 0, payload.len(), &cache)
        .expect("self-described dimension table");

    assert_eq!(dimensions.entity_ref, Some(88));
    assert_eq!(dimensions.rows.len(), 4);
    assert_eq!(dimensions.rows[0].external_id, 2);
    assert_eq!(dimensions.rows[1].value.resolved(), Some(-0.5));
}

#[test]
fn one_row_positional_table_does_not_self_identify_as_dimensions() {
    let payload = b"\xf8\x01\xf7\x58\xfb\xe2\xf7\x59\x01\xe4\x00\x18\x02";
    assert_eq!(
        self_described_positional_dimension_table(
            payload,
            0,
            payload.len(),
            &scalar::ScalarCache::default(),
        ),
        None
    );
}

#[test]
fn positional_dimension_table_retains_bounded_opaque_values() {
    let mut payload = b"prefix\xf8\x03\xf7\x58\xfb\xe2\xf7\x59".to_vec();
    payload.extend_from_slice(&[2, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0, 0x18, 43]);
    payload.extend_from_slice(b"\xf3\xf7\x58\xe2");
    payload.extend_from_slice(&[1, 0x00, 0x04, 0xa6, 0, 0x18, 44]);
    payload.extend_from_slice(b"\xf3\xf7\x58\xe2");
    payload.extend_from_slice(&[5, 0x0d, 0, 0x18, 45]);
    let cache = scalar::ScalarCache::from_section(&payload);

    let dimensions = positional_dimension_table(&payload, 0, payload.len(), 88, &cache)
        .expect("positional dimtab");

    assert_eq!(dimensions.rows.len(), 3);
    assert_eq!(dimensions.rows[1].value.resolved(), None);
    assert_eq!(
        dimensions.rows[1].value.unresolved_token(),
        Some(&[0x00, 0x04, 0xa6][..])
    );
    assert_eq!(dimensions.rows[1].value_body, [0x00, 0x04, 0xa6]);
    assert_eq!(dimensions.rows[1].auxiliary_body, [0x18]);
    assert_eq!(dimensions.rows[1].external_id, 44);
    assert_eq!(dimensions.rows[2].value.resolved(), Some(-1.0));
    assert_eq!(dimensions.rows[2].external_id, 45);
}

#[test]
fn positional_dimensions_decode_the_positive_dict_lattice_and_bounded_opaque_forms() {
    let positive = [1, 0x53, 0xa1, 0xca, 0xc0, 0x83, 0x12, 0x6f, 0, 0x18, 46];
    let opaque_three = [1, 0x00, 0x04, 0xa6, 0, 0x18, 47];
    let opaque_four = [1, 0x01, 0x04, 0xfe, 0xf2, 0, 0x18, 48];
    let zero = [2, 0x18, 0, 0x18, 49];
    let negative_half = [1, 0x0e, 0, 0x18, 50];
    let cache = scalar::ScalarCache::default();

    let positive_row = positional_dimension(&positive, 0, positive.len(), &cache)
        .expect("positive dictionary dimension");
    assert_eq!(
        positive_row.value.resolved(),
        Some(f64::from_be_bytes([
            0x3f, 0xc8, 0xa1, 0xca, 0xc0, 0x83, 0x12, 0x6f,
        ]))
    );
    assert_eq!(positive_row.direction_byte, 0);
    assert_eq!(positive_row.auxiliary_value, Some(0.0));
    assert_eq!(positive_row.value_body, positive[1..8]);
    assert_eq!(positive_row.auxiliary_body, [0x18]);
    assert_eq!(positive_row.external_id, 46);
    for (body, external_id, token) in [
        (&opaque_three[..], 47, &[0x00, 0x04, 0xa6][..]),
        (&opaque_four[..], 48, &[0x01, 0x04, 0xfe, 0xf2][..]),
    ] {
        let row =
            positional_dimension(body, 0, body.len(), &cache).expect("bounded opaque dimension");
        assert_eq!(row.value.resolved(), None);
        assert_eq!(row.value.unresolved_token(), Some(token));
        assert_eq!(row.external_id, external_id);
    }
    let zero_row = positional_dimension(&zero, 0, zero.len(), &cache).expect("zero dimension");
    assert_eq!(zero_row.value.resolved(), Some(0.0));
    assert_eq!(zero_row.external_id, 49);
    let negative_half_row = positional_dimension(&negative_half, 0, negative_half.len(), &cache)
        .expect("negative half dimension");
    assert_eq!(negative_half_row.value.resolved(), Some(-0.5));
    assert_eq!(negative_half_row.external_id, 50);
}

#[test]
fn positional_dimension_seven_byte_positive_value_preserves_field_alignment() {
    let body = [2, 0x31, 0x60, 0x07, 0x53, 0x93, 0xb5, 0xe5, 0, 0x18, 27];
    let row = positional_dimension(&body, 0, body.len(), &scalar::ScalarCache::default())
        .expect("seven-byte positive dimension");

    assert_eq!(
        row.value.resolved(),
        Some(f64::from_be_bytes([
            0x40, 0x60, 0x07, 0x53, 0x93, 0xb5, 0xe5, 0,
        ]))
    );
    assert_eq!(row.direction_byte, 0);
    assert_eq!(row.auxiliary_value, Some(0.0));
    assert_eq!(row.external_id, 27);
}

#[test]
fn dimension_tables_retain_extents_without_decoded_rows() {
    let named = b"dimtab_ptr\0\xf8\x02\xf7\x58\xfb\xe2";
    let cache = scalar::ScalarCache::from_section(named);
    let dimensions = dimension_table(named, 0, named.len(), &cache).expect("named dimtab header");
    assert_eq!(dimensions.declared_count, 2);
    assert_eq!(dimensions.entity_ref, Some(88));
    assert!(dimensions.rows.is_empty());

    let positional = b"\xf8\x02\xf7\x58\xfb\xe2\xf7\x59";
    let cache = scalar::ScalarCache::from_section(positional);
    let dimensions = positional_dimension_table(positional, 0, positional.len(), 88, &cache)
        .expect("positional dimtab header");
    assert_eq!(dimensions.declared_count, 2);
    assert_eq!(dimensions.entity_ref, Some(88));
    assert!(dimensions.rows.is_empty());
}

#[test]
fn positional_definition_inherits_the_labeled_dimension_table_class() {
    let mut payload = b"feat_defs_917\0dimtab_ptr\0\xf8\x01\xf7\x58\xfb\xe2\
            type\0\x01value\0\xe4direct\0\x00aux_value\0\x18ext_id\0\x04\
            \xe0\x01feat_id\0\x2a\xe0\x00ref_model_info\0\xe3S2D0004\0\
            \xf8\x01\xf7\x58\xfb\xe2\xf7\x59"
        .to_vec();
    payload.extend_from_slice(&[2, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0, 0x18, 43]);

    let decoded = crate::decode::with_test_decode_ctx(|ctx| definitions(ctx, &payload))
        .expect("definitions admitted");
    let dimensions = decoded[1].dimensions.as_ref().expect("positional dimtab");

    assert_eq!(decoded[1].identity.owner_feature_id(), Some(42));
    assert_eq!(dimensions.entity_ref, Some(88));
    assert_eq!(dimensions.rows.len(), 1);
    assert_eq!(dimensions.rows[0].value.resolved(), Some(3.0));
    assert_eq!(dimensions.rows[0].external_id, 43);
}

#[test]
fn depdb_gsec2d_definition_anchors_positional_table_replay() {
    let mut payload = b"gsec2d_ptr\0\xe0\x0aname\0S2D0002\0\
            dimtab_ptr\0\xf8\x01\xf7\x58\xfb\xe2\
            type\0\x01value\0\xe4direct\0\x00aux_value\0\x18ext_id\0\x04\
            \xe3S2D0003\0\xf8\x01\xf7\x58\xfb\xe2\xf7\x59"
        .to_vec();
    payload.extend_from_slice(&[2, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0, 0x18, 43]);

    let decoded = crate::decode::with_test_decode_ctx(|ctx| depdb_definitions(ctx, &payload))
        .expect("definitions admitted");
    let dimensions = decoded[1].dimensions.as_ref().expect("positional dimtab");

    assert_eq!(decoded.len(), 2);
    assert_eq!(decoded[0].identity.id(), 2);
    assert_eq!(decoded[1].identity.id(), 2);
    assert!(decoded
        .iter()
        .all(|definition| definition.identity.owner_feature_id().is_none()));
    assert_eq!(dimensions.entity_ref, Some(88));
    assert_eq!(dimensions.rows.len(), 1);
    assert_eq!(dimensions.rows[0].value.resolved(), Some(3.0));
    assert_eq!(dimensions.rows[0].external_id, 43);
}

#[test]
fn positional_variable_table_joins_coordinate_rows() {
    let payload = b"prefix\xf8\x02\xf7\x77\xfb\xe2\xf7\x78\
            \x01\x07\x18\x18\x01\x00\x09\xf1\xf7\x77\xe2\
            \x02\x07\x18\x18\x01\x00\x0a";
    let cache = scalar::ScalarCache::from_section(payload);

    let variables = positional_variable_table(payload, 0, payload.len(), 119, &cache)
        .expect("positional var_arr");

    assert_eq!(variables.declared_count, 2);
    assert_eq!(variables.entity_ref, Some(119));
    assert_eq!(variables.rows.len(), 2);
    assert!(variables.is_complete());
    assert_eq!(variables.rows[0].value_body, [0x18]);
    assert_eq!(variables.rows[0].guess_body, [0x18]);
    assert_eq!(
        variables.rows[0].guess,
        crate::feature::definitions::ScalarLane::Value(0.0)
    );
    assert_eq!(variables.rows[0].known, Some(1));
    assert_eq!(variables.rows[0].homogeneity, Some(0));
    assert_eq!(variables.rows[0].uvar_id, Some(9));
    assert_eq!(
        variables.rows[1].guess,
        crate::feature::definitions::ScalarLane::Value(0.0)
    );
    assert_eq!(variables.rows[1].known, Some(1));
    assert_eq!(variables.rows[1].homogeneity, Some(0));
    assert_eq!(variables.rows[1].uvar_id, Some(10));
    assert_eq!(variables.points().len(), 1);
    assert_eq!(variables.points()[0].point_id, 7);
    assert_eq!(variables.points()[0].u, Some(0.0));
    assert_eq!(variables.points()[0].v, Some(0.0));
}

#[test]
fn positional_variable_table_rejects_duplicate_table_headers() {
    let payload = b"\xf8\x02\xf7\x77\xfb\xe2\xf7\x78
            \x01\x07\x18\x18\x01\x00\x09\xf1\xf7\x77\xe2
            \x02\x07\x18\x18\x01\x00\x0a
            \xf8\x02\xf7\x77\xfb\xe2\xf7\x78
            \x01\x08\x18\x18\x01\x00\x0b\xf1\xf7\x77\xe2
            \x02\x08\x18\x18\x01\x00\x0c";
    let cache = scalar::ScalarCache::from_section(payload);

    assert!(positional_variable_table(payload, 0, payload.len(), 119, &cache).is_none());
}

#[test]
fn positional_variable_guess_zero_preserves_compact_trailing_fields_at_table_boundary() {
    let payload = b"prefix\xf8\x02\xf7\x77\xfb\xe2\xf7\x78\
            \x07\x00\x18\x18\x01\x01\x0f\xf1\xf7\x77\xe2\
            \x07\x01\x18\x18\x00\x01\x07\xf2next_table\0";
    let cache = scalar::ScalarCache::from_section(payload);

    let variables = positional_variable_table(payload, 0, payload.len(), 119, &cache)
        .expect("positional var_arr");

    assert!(variables.is_complete());
    assert_eq!(variables.rows.len(), 2);
    assert_eq!(
        variables.rows[0].guess,
        crate::feature::definitions::ScalarLane::Value(0.0)
    );
    assert_eq!(variables.rows[0].known, Some(1));
    assert_eq!(variables.rows[0].homogeneity, Some(1));
    assert_eq!(variables.rows[0].uvar_id, Some(15));
    assert_eq!(
        variables.rows[1].guess,
        crate::feature::definitions::ScalarLane::Value(0.0)
    );
    assert_eq!(variables.rows[1].known, Some(0));
    assert_eq!(variables.rows[1].homogeneity, Some(1));
    assert_eq!(variables.rows[1].uvar_id, Some(7));
}

#[test]
fn variable_tables_retain_extents_without_decoded_rows() {
    let named = b"var_arr\0\xf8\x02\xf7\x77\xfb\xe2\xf1\xf7\x77\xe2";
    let cache = scalar::ScalarCache::from_section(named);
    let variables = variable_table(named, 0, named.len(), &cache).expect("named var_arr header");
    assert_eq!(variables.declared_count, 2);
    assert_eq!(variables.entity_ref, Some(119));
    assert!(variables.rows.is_empty());
    assert!(variables.points().is_empty());
    assert!(!variables.is_complete());

    let positional = b"\xf8\x02\xf7\x77\xfb\xe2\xf7\x78";
    let cache = scalar::ScalarCache::from_section(positional);
    let variables = positional_variable_table(positional, 0, positional.len(), 119, &cache)
        .expect("positional var_arr header");
    assert_eq!(variables.declared_count, 2);
    assert_eq!(variables.entity_ref, Some(119));
    assert!(variables.rows.is_empty());
    assert!(variables.points().is_empty());
    assert!(!variables.is_complete());
}

#[test]
fn variable_table_withholds_duplicate_coordinate_identities() {
    let row = |variable_type, value, offset| FeatureVariableRow {
        variable_type: crate::feature::definitions::VariableType::from(variable_type),
        key: 7,
        value: ScalarLane::Value(value),
        value_body: Vec::new(),
        guess: ScalarLane::Undefined,
        guess_body: Vec::new(),
        known: None,
        homogeneity: None,
        uvar_id: None,
        offset,
    };
    let table = FeatureVariableTable {
        declared_count: 3,
        entity_ref: Some(119),
        rows: vec![row(1, 2.0, 10), row(1, 2.0, 20), row(2, 3.0, 30)],
        offset: 5,
    };

    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.points().len(), 1);
    assert_eq!(table.points()[0].point_id, 7);
    assert_eq!(table.points()[0].u, None);
    assert_eq!(table.points()[0].v, Some(3.0));
}

#[test]
fn radius_variables_do_not_create_section_points() {
    let row = |variable_type, key, value, offset| FeatureVariableRow {
        variable_type: crate::feature::definitions::VariableType::from(variable_type),
        key,
        value: ScalarLane::Value(value),
        value_body: Vec::new(),
        guess: ScalarLane::Undefined,
        guess_body: Vec::new(),
        known: None,
        homogeneity: None,
        uvar_id: None,
        offset,
    };
    let table = FeatureVariableTable {
        declared_count: 3,
        entity_ref: Some(119),
        rows: vec![row(1, 7, 2.0, 10), row(2, 7, 3.0, 20), row(3, 99, 4.0, 30)],
        offset: 5,
    };

    assert_eq!(table.points().len(), 1);
    assert_eq!(table.points()[0].point_id, 7);
    let (points, ambiguous) = reconciled_points(&table);
    assert_eq!(points.get(&7), Some(&[Some(2.0), Some(3.0)]));
    assert!(!points.contains_key(&99));
    assert!(ambiguous.is_empty());
}

#[test]
fn reconciled_points_refuses_before_point_id_node() {
    let table = with_points(
        FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            offset: 0,
        },
        vec![FeatureSectionPoint {
            point_id: 7,
            u: Some(2.0),
            v: Some(3.0),
        }],
    );
    let cap = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems, Some("creo reconciled point ID nodes"),
        |limit| with_dimension_limits(&[0], limit, u64::MAX, |ctx| table.reconciled_points(ctx)),
    );
    assert!(matches!(with_dimension_limits(&[0], cap, u64::MAX,
        |ctx| table.reconciled_points(ctx)),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo reconciled point ID nodes"));
    assert_eq!(
        reconciled_points(&table).0.get(&7),
        Some(&[Some(2.0), Some(3.0)])
    );
}

#[test]
fn variable_coordinate_7e_and_c6_are_the_f3_dict_sign_pair() {
    let positive = [0x7e, 0x6b, 0x37, 0x21, 0xad, 0xb3, 0xb7];
    let negative = [0xc6, 0x6b, 0x37, 0x21, 0xad, 0xb3, 0xb7];
    let cache = scalar::ScalarCache::from_section(&positive);

    assert_eq!(
        decode_variable_scalar(&positive, 0, positive.len(), &cache),
        (
            ScalarLane::Value(f64::from_be_bytes([
                0x3f, 0xf3, 0x6b, 0x37, 0x21, 0xad, 0xb3, 0xb7
            ])),
            7
        )
    );
    assert_eq!(
        decode_variable_scalar(&negative, 0, negative.len(), &cache),
        (
            ScalarLane::Value(f64::from_be_bytes([
                0xbf, 0xf3, 0x6b, 0x37, 0x21, 0xad, 0xb3, 0xb7
            ])),
            7
        )
    );
}

#[test]
fn positional_gsec3d_decodes_placement_and_reference_rows() {
    let payload = b"prefix\x07S2D0004\0\x01\xf6\xe1\xf6\x82\x01\xf6\
            \xf8\x02\xf7\x39\xfb\xe2\xf7\x3a\
            \x06\x05\xf6\x03\xf6\x00\xe3tail\xf2\xf7\x39\xe2\
            \x07\x05\xf6\x04\xf6\x01";

    let section = positional_section_3d(payload, 0, payload.len()).expect("positional gsec3d");

    assert_eq!(section.sketch_plane_entity_id, Some(513));
    assert_eq!(section.sketch_plane_flip, None);
    assert_eq!(
        section.reference_planes.entity_ids().collect::<Vec<_>>(),
        vec![6, 7]
    );
    let ReferencePlanes::Positional(rows) = &section.reference_planes else {
        panic!("positional reference planes");
    };
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].plane_entity_id, 6);
    assert_eq!(rows[0].reference_type, Some(5));
    assert_eq!(rows[0].external_reference_id, None);
    assert_eq!(rows[0].segment_id, Some(3));
    assert_eq!(rows[0].sub_index, None);
    assert_eq!(rows[0].reference_flip, Some(BinaryFlag::Clear));
    assert_eq!(rows[1].plane_entity_id, 7);
    assert_eq!(rows[1].reference_type, Some(5));
    assert_eq!(rows[1].external_reference_id, None);
    assert_eq!(rows[1].segment_id, Some(4));
    assert_eq!(rows[1].sub_index, None);
    assert_eq!(rows[1].reference_flip, Some(BinaryFlag::Set));
    assert_eq!(section.reference_plane_datum_geometry_id, None);
    assert_eq!(section.orientation.section_flip, Some(BinaryFlag::Set));
    assert_eq!(section.orientation.reference_type, None);
    assert_eq!(section.orientation.segment_id, None);
    assert_eq!(section.orientation.reference_flip, None);
}

#[test]
fn positional_gsec3d_retains_its_header_without_a_body() {
    let payload = b"prefix\x07S2D0004\0";

    let section = positional_section_3d(payload, 0, payload.len()).expect("positional gsec3d");

    assert_eq!(section.offset, 6);
    assert_eq!(section.sketch_plane_entity_id, None);
    assert!(section.reference_planes.entity_ids().next().is_none());
    assert_eq!(section.orientation, FeatureSectionOrientation::default());
}

#[test]
fn positional_gsec3d_retains_placement_and_complete_reference_prefix() {
    let payload = b"prefix\x07S2D0004\0\x01\xf6\xe1\xf6\x82\x01\xf6\
            \xf8\x02\xf7\x39\xfb\xe2\xf7\x3a\
            \x06\x05\xf6\x03\xf6\x00\xe3tail\xf2\xf7\x39\xe2\x07";

    let section = positional_section_3d(payload, 0, payload.len()).expect("positional gsec3d");

    assert_eq!(section.sketch_plane_entity_id, Some(513));
    assert_eq!(
        section.reference_planes.entity_ids().collect::<Vec<_>>(),
        [6]
    );
    assert_eq!(section.orientation.section_flip, Some(BinaryFlag::Set));
    assert_eq!(section.orientation.reference_type, None);
    assert_eq!(section.orientation.segment_id, None);
    assert_eq!(section.orientation.reference_flip, None);
}

#[test]
fn named_gsec3d_uses_the_outer_plane_id_before_reference_rows() {
    let payload = b"\xe0\x00gsec3d_ptr\0\
            \xe0\x01plane_id\0\x2a\
            \xe0\x01plane_flip\0\xf6\
            \xe0\x00ref_planes\0\xf8\x01\xf7\x80\x8c\xfb\xe2\
            \xe0\x01plane_id\0\x06\
            \xe0\x01ref_type\0\x05\
            \xe0\x01ext_ref_id\0\xf6\
            \xe0\x01seg_id\0\x02\
            \xe0\x01sub_index\0\xf6\
            \xe0\x01flip_flag\0\x00\
            \xe0\x00p_saved_result\0";

    let definitions = crate::decode::with_test_decode_ctx(|ctx| {
        definitions_in_ranges(
            ctx,
            &payload[..],
            &[crate::feature::definitions::DefinitionStart {
                offset: 0,
                id: std::num::NonZeroU32::new(1),
                owner_override: None,
                positional: false,
            }],
        )
    })
    .expect("definitions admitted");
    let section = definitions[0].section_3d.as_ref().expect("named gsec3d");

    assert_eq!(section.sketch_plane_entity_id, Some(42));
    assert_eq!(section.reference_plane_datum_geometry_id, Some(6));
    assert_eq!(section.sketch_plane_flip, None);
}

#[test]
fn equation_table_replays_direct_and_counted_rows() {
    let payload = b"eqtn_arr\0\xf2\xf8\x04\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\
            \xe0\x05fcn_id\0\x02\
            \xe0\x08arg_arr\0\xf8\x02\x2f\x08\
            \xe0\x01aux_data\0\xf6\
            \xf1\xf7\x80\x9f\xe2\
            \x01\x04\x11\x12\xf6\xe2\
            \x02\x05\xf8\x04\x13\xe4\xe5\xf6\xe2\
            \x03\x06\xf8\x02\xf6\x14\xf6\xe2\
            \xe0\x02scale\0\x99\x88"
        .to_vec();

    let table = equation_table(&payload, 0, payload.len()).expect("eqtn_arr table");

    assert_eq!(table.declared_count, 4);
    assert_eq!(table.entity_ref, Some(159));
    assert_eq!(table.offset, 0);
    assert_eq!(table.rows.len(), 3);
    assert!(table.prototype_body.starts_with(b"\xe0\x01id\0"));
    assert!(table.prototype_body.ends_with(b"\xf1\xf7\x80\x9f\xe2"));

    assert_eq!(table.rows[0].equation_id, 1);
    assert_eq!(table.rows[0].function_id, 4);
    assert_eq!(table.rows[0].explicit_argument_count, None);
    assert_eq!(table.rows[0].arguments, [Some(17), Some(18)]);
    assert_eq!(table.rows[0].arguments_body, [0x11, 0x12]);
    assert_eq!(table.rows[0].auxiliary_body, [0xf6]);
    assert_eq!(table.rows[0].body, [1, 4, 0x11, 0x12, 0xf6, 0xe2]);

    assert_eq!(table.rows[1].equation_id, 2);
    assert_eq!(table.rows[1].function_id, 5);
    assert_eq!(table.rows[1].explicit_argument_count, Some(4));
    assert_eq!(
        table.rows[1].arguments,
        [Some(19), Some(1), Some(0), Some(0)]
    );
    assert_eq!(table.rows[1].arguments_body, [0x13, 0xe4, 0xe5]);
    assert_eq!(table.rows[1].auxiliary_body, [0xf6]);
    assert!(table.rows[1].body.ends_with(&[0xf6, 0xe2]));

    assert_eq!(table.rows[2].equation_id, 3);
    assert_eq!(table.rows[2].function_id, 6);
    assert_eq!(table.rows[2].explicit_argument_count, Some(2));
    assert_eq!(table.rows[2].arguments, [None, Some(20)]);
    assert_eq!(table.rows[2].arguments_body, [0xf6, 0x14]);
    assert_eq!(table.rows[2].auxiliary_body, [0xf6]);
}

#[test]
fn equation_table_accepts_final_row_at_table_separator() {
    let payload = b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\x00\
            \xe0\x05fcn_id\0\x02\
            \xe0\x08arg_arr\0\xf8\x02\x11\x12\
            \xe0\x01aux_data\0\xf6\
            \xf1\xf7\x80\x9f\xe2\
            \x01\x04\x11\x12\xf6\xf2\xf7\x39\x99\x88\
            \xe0\x02scale\0\x99\x88";

    let table = equation_table(payload, 0, payload.len()).expect("eqtn_arr table");

    assert_eq!(table.declared_count, 2);
    assert_eq!(table.rows.len(), 1);
    assert_eq!(table.rows[0].equation_id, 1);
    assert_eq!(table.rows[0].body, [1, 4, 0x11, 0x12, 0xf6]);
}

const EQUATION_LIMIT_INPUT: &[u8] = b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
    \xe0\x01id\0\x00\xf1\xf7\x80\x9f\xe2\
    \x01\x04\x11\x12\xf6\xe2";

fn equation_with_limits(
    collection_limit: u64,
    retained_limit: u64,
) -> Result<Option<crate::feature::definitions::FeatureEquationTable>, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(EQUATION_LIMIT_INPUT, &arena, &policy)
        .expect("equation root admitted");
    parse_equation_table(&ctx, EQUATION_LIMIT_INPUT, 0, EQUATION_LIMIT_INPUT.len())
}

#[test]
fn equation_prototype_body_refuses_before_retained_copy() {
    assert!(
        matches!(equation_with_limits(3, 10), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo equation prototype body")
    );
}

#[test]
fn equation_arguments_refuse_before_vec_growth() {
    assert!(
        matches!(equation_with_limits(1, crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::RetainedBytes, None, |cap| equation_with_limits(u64::MAX, cap))), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo equation arguments")
    );
}

#[test]
fn equation_argument_body_refuses_before_retained_copy() {
    assert!(
        matches!(equation_with_limits(3, crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo equation argument body"), |cap| equation_with_limits(3, cap))), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo equation argument body")
    );
}

#[test]
fn equation_auxiliary_body_refuses_before_retained_copy() {
    assert!(
        matches!(equation_with_limits(3, crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo equation auxiliary body"), |cap| equation_with_limits(3, cap))), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo equation auxiliary body")
    );
}

#[test]
fn equation_row_body_refuses_before_retained_copy() {
    assert!(
        matches!(equation_with_limits(3, crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo equation row body"), |cap| equation_with_limits(3, cap))), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo equation row body")
    );
}

#[test]
fn equation_row_refuses_before_vec_growth() {
    assert!(
        matches!(equation_with_limits(2, crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::RetainedBytes, None, |cap| equation_with_limits(u64::MAX, cap))), Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo equation rows")
    );
    let table = equation_with_limits(
        3,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            None,
            |cap| equation_with_limits(u64::MAX, cap),
        ),
    )
    .expect("equation admitted")
    .expect("table present");
    assert_eq!(table.rows[0].arguments, [Some(17), Some(18)]);
}

#[test]
fn positional_relation_table_replays_rows_after_its_prototype() {
    let payload = b"prefix\xf8\x03\xf7\x64\xfb\xe2\xf7\x65\
            prototype\xf1\xf7\x64\xe2\
            \x08\x00\x03\x0f\xf6\xe4\x01\xe4\x00\xe4\x0f\x10\x0f\x18\x00\xf6\x00\xe2";

    let relations =
        positional_relation_table(payload, 0, payload.len(), 100).expect("positional relat_ptr");

    assert_eq!(relations.declared_count, 3);
    assert_eq!(relations.entity_ref, Some(100));
    assert_eq!(relations.rows.len(), 1);
    assert_eq!(relations.rows[0].relation_id, 8);
    assert_eq!(relations.rows[0].used, 0);
    assert_eq!(relations.rows[0].sign, 0);
    assert_eq!(relations.rows[0].dimension_id, 246);
    assert_eq!(relations.rows[0].relation_type, 0);
    assert!(relations.rows[0].operand_vectors.is_some());
}

#[test]
fn relation_table_retains_solver_children_after_an_invalid_row() {
    let payload = b"relat_ptr\0\xf4\x04\xf8\x03\xf7\x6a\xfb\xe2\
            schema\xf1\xf7\x6a\xe2invalid\
            skamp_ptr\0\xf3\xf8\x01\xf7\x6b\xfb\xe2\
            \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
            \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
            \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
            \xf3\xf7\x6b\xe2";

    let relations = relation_table(payload, 0, payload.len()).expect("relat_ptr header");

    assert_eq!(relations.declared_count, 3);
    assert_eq!(relations.entity_ref, Some(106));
    assert!(relations.rows.is_empty());
    assert_eq!(relations.skamps().len(), 1);
    assert_eq!(relations.skamps()[0].id, 5);
}

#[test]
fn relation_tables_retain_extents_without_their_prototypes() {
    let named = b"relat_ptr\0\xf8\x03\xf7\x64\xfb\xe2";
    let relations = relation_table(named, 0, named.len()).expect("named relat_ptr header");
    assert_eq!(relations.declared_count, 3);
    assert_eq!(relations.entity_ref, Some(100));
    assert!(relations.rows.is_empty());

    let positional = b"\xf8\x03\xf7\x64\xfb\xe2";

    let relations = positional_relation_table(positional, 0, positional.len(), 100)
        .expect("positional relat_ptr header");

    assert_eq!(relations.declared_count, 3);
    assert_eq!(relations.entity_ref, Some(100));
    assert!(relations.rows.is_empty());
}

#[test]
fn positional_skamp_table_replays_counted_nested_items() {
    let payload = b"\xf8\x02\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x02\xf7\x60\xfb\xe2\xf7\x61\
            \x06\x03\xf1\xf7\x60\xe2\x07\x02\xf3\xf7\x58\xe2\
            \x02\x01\xea\x22\x00\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x08\x00";

    let skamps = positional_feature_skamps(payload, 0, payload.len(), 88);

    assert_eq!(skamps.len(), 2);
    assert_eq!(skamps[0].id, 1);
    assert_eq!(skamps[0].kind, 0);
    assert_eq!(skamps[0].items.len(), 2);
    assert_eq!(skamps[0].items[0].entity_id, 6);
    assert_eq!(skamps[0].items[1].sense, 2);
    assert_eq!(skamps[1].kind, 1);
    assert_eq!(skamps[1].flags, 34);
    assert_eq!(skamps[1].status, 35);
    assert_eq!(skamps[1].items[0].entity_id, 8);
}

#[test]
fn positional_skamp_table_replays_consecutive_single_item_rows() {
    let payload = b"\xf8\x03\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x06\x00\xe2\
            \x02\x01\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x07\x00\xe2\
            \x03\x02\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x08\x00";

    let skamps = positional_feature_skamps(payload, 0, payload.len(), 88);

    assert_eq!(skamps.len(), 3);
    assert_eq!(
        skamps
            .iter()
            .map(|skamp| (skamp.id, skamp.kind, skamp.items[0].entity_id))
            .collect::<Vec<_>>(),
        [(1, 0, 6), (2, 1, 7), (3, 2, 8)]
    );
}

#[test]
fn positional_skamp_table_accepts_a_following_table_wrapper_boundary() {
    let payload = b"\xf8\x02\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x06\x00\xe2\
            \x02\x01\x00\x23\xf8\x02\xf7\x60\xfb\xe2\xf7\x61\
            \x07\x00\xf1\xf7\x60\xe2\x08\x02\
            \xf4\x04\xf7\x64\xf8\x01\xf7\x64\xfb\xe2";

    let skamps = positional_feature_skamps(payload, 0, payload.len(), 88);

    assert_eq!(skamps.len(), 2);
    assert_eq!(skamps[1].items.len(), 2);
    assert_eq!(
        skamps[1]
            .items
            .iter()
            .map(|item| (item.entity_id, item.sense))
            .collect::<Vec<_>>(),
        [(7, 0), (8, 2)]
    );
}

#[test]
fn positional_skamp_table_accepts_a_following_table_header_boundary() {
    let payload = b"\xf8\x01\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x06\x00\
            \xf8\x02\xf7\x64\xfb\xe2\xf7\x65";

    let skamps = positional_feature_skamps(payload, 0, payload.len(), 88);

    assert_eq!(skamps.len(), 1);
    assert_eq!(skamps[0].items[0].entity_id, 6);
}

#[test]
fn positional_skamp_table_accepts_a_following_wrapper_body_boundary() {
    let payload = b"\xf8\x01\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x06\x00\
            \xf4\x04\xf7\x64\xe1\xe1\xe3";

    let skamps = positional_feature_skamps(payload, 0, payload.len(), 88);

    assert_eq!(skamps.len(), 1);
    assert_eq!(skamps[0].items[0].entity_id, 6);
}

#[test]
fn positional_skamp_table_skips_row_auxiliary_frames() {
    let payload = b"\xf8\x03\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x02\xf7\x60\xfb\xe2\xf7\x61\
            \x06\x03\xf1\xf7\x60\xe2\x07\x02\xf3\xf7\x58\xe2\
            \x02\x04\x00\x22\xe0\x02aux\0\xf8\x02\x0a\x0b\xf7\x60\
            \xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x08\x00\xf3\xf7\x58\xe2\
            \x03\x02\x00\x23\xf7\x60\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x09\x00";

    let skamps = positional_feature_skamps(payload, 0, payload.len(), 88);

    assert_eq!(skamps.len(), 3);
    assert_eq!(skamps[1].id, 2);
    assert_eq!(skamps[1].kind, 4);
    assert_eq!(skamps[1].items[0].entity_id, 8);
    assert_eq!(skamps[2].id, 3);
    assert_eq!(skamps[2].kind, 2);
    assert_eq!(skamps[2].items[0].entity_id, 9);
}

#[test]
fn positional_skamp_table_rejects_ambiguous_nested_item_arrays() {
    let payload = b"\xf8\x02\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x06\x00\
            \xf3\xf7\x58\xe2\x02\x04\x00\x22\
            \xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x07\x00\
            \xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x08\x00";

    let skamps = positional_feature_skamps(payload, 0, payload.len(), 88);

    assert_eq!(skamps.len(), 1);
    assert_eq!(skamps[0].id, 1);
}

#[test]
fn positional_skamp_table_rejects_multiple_matching_nested_item_arrays() {
    let payload = b"\xf8\x01\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x06\x00\
            \xf8\x01\xf7\x62\xfb\xe2\xf7\x63\x07\x00\
            \xf3\xf7\x58\xe2";

    let skamps = positional_feature_skamps(payload, 0, payload.len(), 88);

    assert!(skamps.is_empty());
}

#[test]
fn positional_solver_tables_retain_complete_prefix_rows() {
    let skamps = b"\xf8\x02\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x02\xf7\x60\xfb\xe2\xf7\x61\
            \x06\x03\xf1\xf7\x60\xe2\x07\x02\xf3\xf7\x58\xe2";
    let rows = positional_feature_skamps(skamps, 0, skamps.len(), 88);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, 1);

    let triples = b"\xf8\x02\xf7\x64\xfb\xe2\xf7\x65\
            \x01\xf6\x04\xf1\xf7\x64\xe2";
    let rows = positional_relation_triples(triples, 0, triples.len(), 100);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relation_id, Some(1));
}

#[test]
fn positional_skamp_items_refuse_before_vec_growth() {
    let payload = b"\xf8\x01\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x06\x00";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy).expect("root admitted");
    assert!(
        matches!(parse_positional_feature_skamps(&ctx, payload, 0, payload.len(), 88),
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo skamp items")
    );
}

#[test]
fn positional_skamp_rows_refuse_before_vec_growth() {
    let payload = b"\xf8\x01\xf7\x58\xfb\xe2\xf7\x59\
            \x01\x00\x00\x23\xf8\x01\xf7\x60\xfb\xe2\xf7\x61\x06\x00";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy).expect("root admitted");
    assert!(
        matches!(parse_positional_feature_skamps(&ctx, payload, 0, payload.len(), 88),
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo skamp rows")
    );
    assert_eq!(
        positional_feature_skamps(payload, 0, payload.len(), 88).len(),
        1
    );
}

#[test]
fn named_skamp_prototype_items_refuse_before_vec_growth() {
    let payload = b"skamp_ptr\0\xf3\xf8\x01\xf7\x6b\xfb\xe2\
            \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
            \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
            \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
            \xf3\xf7\x6b\xe2";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy).expect("root admitted");
    assert!(
        matches!(parse_feature_skamps(&ctx, payload, 0, payload.len()),
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo skamp prototype items")
    );
}

#[test]
fn named_skamp_rows_refuse_before_vec_growth() {
    let payload = b"skamp_ptr\0\xf3\xf8\x01\xf7\x6b\xfb\xe2\
            \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
            \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
            \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
            \xf3\xf7\x6b\xe2";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy).expect("root admitted");
    assert!(
        matches!(parse_feature_skamps(&ctx, payload, 0, payload.len()),
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo skamp rows")
    );
}

#[test]
fn named_relation_triples_refuse_before_vec_growth() {
    let payload = b"triples_ptr\0\xf4\x04\xf8\x01\xf7\x6d\xfb\xe2\
            \xe0\x01rel_id\0\x07\xe0\x01eqn_id\0\x08\
            \xe0\x01skamp_id\0\x05\xf1\xf7\x6d\xe2";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy).expect("root admitted");
    assert!(
        matches!(parse_feature_relation_triples(&ctx, payload, 0, payload.len()),
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo relation triples")
    );
}

#[test]
fn positional_relation_triples_refuse_before_vec_growth() {
    let payload = b"\xf8\x01\xf7\x64\xfb\xe2\xf7\x65\x01\xf6\x04";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy).expect("root admitted");
    assert!(
        matches!(parse_positional_relation_triples(&ctx, payload, 0, payload.len(), 100),
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo relation triples")
    );
}

#[test]
fn solver_header_does_not_adopt_a_later_array() {
    let payload = b"skamp_ptr\0opaque\xf8\x02\xf7\x58\xfb\xe2";

    assert!(
        crate::decode::with_test_decode_ctx(|ctx| named_solver_table_header(
            ctx,
            payload,
            b"skamp_ptr\0",
            0,
            payload.len()
        ))
        .expect("solver search admitted")
        .is_none()
    );
}

#[test]
fn optional_solver_header_propagates_the_search_refusal() {
    let payload = b"skamp_ptr\0opaque\xf8\x02\xf7\x58\xfb\xe2";
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(payload, &arena, &policy)
        .expect("root admitted");
    let cadmpeg_core::CodecError::ResourceLimit(limit) =
        named_solver_table_header(&ctx, payload, b"skamp_ptr\0", 0, payload.len())
            .expect_err("search must refuse")
    else {
        panic!("resource refusal");
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(limit.operation, "find Creo solver table");
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn named_solver_tables_retain_complete_prefix_rows() {
    let skamps = b"skamp_ptr\0\xf3\xf8\x02\xf7\x6b\xfb\xe2\
            \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
            \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
            \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
            \xf3\xf7\x6b\xe2invalid";
    let rows = feature_skamps(skamps, 0, skamps.len());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, 5);

    let triples = b"triples_ptr\0\xf4\x04\xf8\x02\xf7\x6d\xfb\xe2\
            \xe0\x01rel_id\0\x07\xe0\x01eqn_id\0\x08\
            \xe0\x01skamp_id\0\x05\xf1\xf7\x6d\xe2\x01\x02\x03";
    let rows = feature_relation_triples(triples, 0, triples.len());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relation_id, Some(7));
}

#[test]
fn named_solver_tables_accept_direct_prototype_item_schema_close() {
    let skamps = b"skamp_ptr\0\xf1\xf8\x02\xf7\x6b\xfb\xe2\
            \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
            \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
            \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\
            \xf3\xf7\x6b\xe2\
            \x07\x02\x03\x23\xf8\x01\xf7\x6c\xfb\xe2\
            \xf7\x6d\x2a\x01\xe2";

    let rows = feature_skamps(skamps, 0, skamps.len());

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, 5);
    assert_eq!(
        rows[0].items,
        vec![FeatureSkampItem {
            entity_id: 42,
            sense: 1
        }]
    );
    assert_eq!(rows[1].id, 7);
    assert_eq!(rows[1].kind, 2);
    assert_eq!(rows[1].flags, 3);
    assert_eq!(rows[1].status, 35);
    assert_eq!(
        rows[1].items,
        vec![FeatureSkampItem {
            entity_id: 42,
            sense: 1
        }]
    );
}

#[test]
fn positional_definition_preserves_its_named_solver_tables() {
    let solver_tables = b"skamp_ptr\0\xf3\xf8\x01\xf7\x6b\xfb\xe2\
            \xe0\x01id\0\x05\xe0\x01type\0\x02\xe0\x01flags\0\x03\
            \xe0\x01status\0\x04\xe0\x00items\0\xf8\x01\xf7\x6c\xfb\xe2\
            \xe0\x01ent_id\0\x2a\xe0\x01sense\0\x01\xf1\xf7\x6c\xe2\
            \xf3\xf7\x6b\xe2\
            triples_ptr\0\xf4\x04\xf8\x01\xf7\x6d\xfb\xe2\
            \xe0\x01rel_id\0\x07\xe0\x01eqn_id\0\x08\
            \xe0\x01skamp_id\0\x05\xf1\xf7\x6d\xe2";
    let mut payload = b"relat_ptr\0\xf4\x04\xf8\x02\xf7\x6a\xfb\xe2schema\xf1\xf7\x6a\xe2".to_vec();
    payload.extend_from_slice(solver_tables);
    let positional_start = payload.len();
    payload.extend_from_slice(solver_tables);
    payload.extend_from_slice(b"\xf8\x02\xf7\x6a\xfb\xe2");
    let prototype_offset = payload.len() + 3;
    assert!((128..=16_383).contains(&prototype_offset));
    payload.extend_from_slice(&[
        psb::token::ENTITY_REF,
        0x80 + u8::try_from(prototype_offset >> 8).expect("prototype offset high byte"),
        u8::try_from(prototype_offset & 0xff).expect("prototype offset low byte"),
    ]);
    payload.extend_from_slice(b"\xf1\xf7\x6a\xe2");

    let definitions = crate::decode::with_test_decode_ctx(|ctx| {
        definitions_in_ranges(
            ctx,
            &payload,
            &[
                crate::feature::definitions::DefinitionStart {
                    offset: 0,
                    id: std::num::NonZeroU32::new(1),
                    owner_override: None,
                    positional: false,
                },
                crate::feature::definitions::DefinitionStart {
                    offset: positional_start,
                    id: std::num::NonZeroU32::new(2),
                    owner_override: None,
                    positional: true,
                },
            ],
        )
    })
    .expect("definitions admitted");
    let relations = definitions[1].relations.as_ref().expect("relations");

    assert_eq!(relations.skamps().len(), 1);
    assert_eq!(relations.skamps()[0].id, 5);
    assert_eq!(
        relations
            .skamps
            .as_ref()
            .expect("skamp table")
            .header()
            .expect("skamp header")
            .declared_count,
        1
    );
    assert_eq!(relations.triples().len(), 1);
    assert_eq!(relations.triples()[0].relation_id, Some(7));
    assert_eq!(
        relations
            .triples
            .as_ref()
            .expect("triples table")
            .header()
            .expect("triples header")
            .declared_count,
        1
    );
}

#[test]
fn positional_triples_replay_nullable_relation_joins() {
    let payload = b"\xf8\x02\xf7\x64\xfb\xe2\xf7\x65\
            \x01\xf6\x04\xf1\xf7\x64\xe2\x02\xf6\x05";

    let triples = positional_relation_triples(payload, 0, payload.len(), 100);

    assert_eq!(triples.len(), 2);
    assert_eq!(triples[0].relation_id, Some(1));
    assert_eq!(triples[0].equation_id, None);
    assert_eq!(triples[0].skamp_id, Some(4));
    assert_eq!(triples[1].relation_id, Some(2));
    assert_eq!(triples[1].skamp_id, Some(5));
}

fn with_trim_limits<T>(
    collection_limit: u64,
    work_limit: u64,
    run: impl FnOnce(&DecodeContext<'_>) -> T,
) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_work_units = work_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root fits the trim policy");
    run(&ctx)
}

mod trim_tables;

fn decode_variable_scalar(payload: &[u8], offset: usize, end: usize, cache: &scalar::ScalarCache) -> (ScalarLane, usize) {
    crate::decode::with_test_decode_ctx(|ctx| decode_variable_scalar_checked(ctx, payload, offset, end, cache))
        .expect("scalar lane admission")
}
