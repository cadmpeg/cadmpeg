// SPDX-License-Identifier: Apache-2.0

use crate::feature::definitions::test_support::{reconciled_points, with_trim_limits};
use super::entity_intersection as parse_entity_intersection;
use crate::feature::definitions::trim::positional_trim_entity_table as parse_positional_trim_entity_table;
use crate::feature::definitions::trim::positional_trim_vertex_table as parse_positional_trim_vertex_table;
use crate::feature::definitions::test_support::with_points;
use crate::feature::definitions::trim::trim_buckets as parse_trim_buckets;
use crate::feature::definitions::trim::trim_vertex_entry as parse_trim_vertex_entry;
use crate::feature::definitions::{
    FeatureSectionPoint, FeatureSegment, FeatureSegmentKind,
    FeatureSegmentTable, FeatureVariableRow, FeatureVariableTable, ScalarLane, TrimEntityKind,
    VariableType,
};
use crate::feature::definitions::trim::{trim_table_header, TrimEntryKind, TrimTableClasses, TrimTableHeader};
use cadmpeg_core::decode::ResourceDimension;
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


fn trim_bucket_with_limits(
    collection_limit: u64,
    work_limit: u64,
) -> Result<Vec<super::super::FeatureTrimBucket>, cadmpeg_core::CodecError> {
    let payload = b"bucket_index\0\x00bucket_xar\0\xf8\x01\xf7\x43\xfb\xe3\
            \xf7\x44\x01\x02\x03\x00\xe0";
    let header = TrimTableHeader {
        declared_count: 1,
        classes: TrimTableClasses {
            table: 66,
            bucket: 67,
            entry: 68,
        },
    };
    with_trim_limits(collection_limit, work_limit, |ctx| {
        parse_trim_buckets(
            ctx,
            payload,
            0,
            payload.len(),
            header,
            TrimEntryKind::Vertex,
        )
    })
}

fn trim_entity_with_limit(
    limit: u64,
) -> Result<Option<super::super::FeatureTrimEntityTable>, cadmpeg_core::CodecError> {
    let payload = b"prefix\xf8\x07\xf7\x42\xfb\xe2\xf7\x43\x00\xe3\
            \x09\x00\x03\x04\xf6\x00\
            \xf4\x04\xf7\x42\xe2\x01\xf8\x13\xf7\x44\xfb\xe2";
    with_trim_limits(limit, u64::MAX, |ctx| {
        parse_positional_trim_entity_table(
            ctx,
            payload,
            0,
            payload.len(),
            TrimTableClasses {
                table: 66,
                bucket: 67,
                entry: 67,
            },
            Some(68),
        )
    })
}

fn trim_vertex_with_limit(
    limit: u64,
) -> Result<Option<super::super::FeatureTrimVertexTable>, cadmpeg_core::CodecError> {
    let payload = b"prefix\xf8\x13\xf7\x44\xfb\xe2\xf7\x45\
            \x01\x02\x03\x00\xe2";
    with_trim_limits(limit, u64::MAX, |ctx| {
        parse_positional_trim_vertex_table(
            ctx,
            payload,
            0,
            payload.len(),
            TrimTableClasses {
                table: 68,
                bucket: 69,
                entry: 69,
            },
            None,
            None,
        )
    })
}

macro_rules! trim_bucket_collection_limit_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let limit = crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some($operation), |cap| trim_bucket_with_limits(cap, u64::MAX));
            assert!(matches!(trim_bucket_with_limits(limit, u64::MAX),
                Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
                    if refusal.dimension == ResourceDimension::CollectionItems
                        && refusal.operation == $operation));
            assert_eq!(trim_bucket_with_limits(u64::MAX, u64::MAX)
                .expect("all bucket allocations admitted").len(), 1);
        }
    };
}

#[test]
fn trim_bucket_entry_work_refuses_before_scan() {
    let cap = crate::test_support::allocation_limit_at(ResourceDimension::WorkUnits, Some("creo trim bucket entry scan"), |cap| trim_bucket_with_limits(u64::MAX, cap));
    let error = trim_bucket_with_limits(u64::MAX, cap).expect_err("entry work refusal");
    let refused: Result<Vec<super::super::FeatureTrimBucket>, _> = Err(error);
    assert!(matches!(refused,
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
            if refusal.dimension == ResourceDimension::WorkUnits
                && refusal.operation == "creo trim bucket entry scan"));
}

macro_rules! trim_entity_collection_limit_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let limit = crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some($operation), trim_entity_with_limit);
            assert!(matches!(trim_entity_with_limit(limit),
                Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
                    if refusal.dimension == ResourceDimension::CollectionItems
                        && refusal.operation == $operation));
            assert_eq!(trim_entity_with_limit(u64::MAX)
                .expect("entity table admitted")
                .expect("one table").rows.len(), 1);
        }
    };
}

#[test]
fn trim_vertex_entities_refuse_before_vec_growth() {
    let limit = crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some("creo trim vertex entities"), trim_vertex_with_limit);
    assert!(matches!(trim_vertex_with_limit(limit),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "creo trim vertex entities"));
    assert_eq!(
        trim_vertex_with_limit(u64::MAX)
            .expect("vertex table admitted")
            .expect("one table")
            .rows[0]
            .entities,
        [1, 2]
    );
}

#[test]
fn trim_vertex_rows_refuse_before_vec_growth() {
    let limit = crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, Some("creo trim vertex rows"), trim_vertex_with_limit);
    assert!(matches!(trim_vertex_with_limit(limit),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "creo trim vertex rows"));
}

fn positional_trim_entity_table(
    payload: &[u8],
    start: usize,
    end: usize,
    classes: TrimTableClasses,
    next_table_class: Option<u32>,
) -> Option<super::super::FeatureTrimEntityTable> {
    crate::decode::with_test_decode_ctx(|ctx| {
        parse_positional_trim_entity_table(ctx, payload, start, end, classes, next_table_class)
    })
    .expect("trim entity table admitted")
}

fn positional_trim_vertex_table(
    payload: &[u8],
    start: usize,
    end: usize,
    classes: TrimTableClasses,
    segments: Option<&FeatureSegmentTable>,
    variables: Option<&FeatureVariableTable>,
) -> Option<super::super::FeatureTrimVertexTable> {
    crate::decode::with_test_decode_ctx(|ctx| {
        parse_positional_trim_vertex_table(ctx, payload, start, end, classes, segments, variables)
    })
    .expect("trim vertex table admitted")
}

fn trim_buckets(
    payload: &[u8],
    table: usize,
    end: usize,
    header: TrimTableHeader,
    kind: TrimEntryKind,
) -> Vec<super::super::FeatureTrimBucket> {
    crate::decode::with_test_decode_ctx(|ctx| {
        parse_trim_buckets(ctx, payload, table, end, header, kind)
    })
    .expect("trim buckets admitted")
}

fn trim_vertex_entry(payload: &[u8], offset: usize, end: usize) -> Option<(Vec<u32>, u32, usize)> {
    crate::decode::with_test_decode_ctx(|ctx| parse_trim_vertex_entry(ctx, payload, offset, end))
        .expect("trim vertex entities admitted")
}









#[test]
fn positional_trim_entity_table_decodes_without_segments() {
    let payload = b"prefix\xf8\x07\xf7\x42\xfb\xe2\xf7\x43\x00\xe3\
            \x09\x00\x03\x04\xf6\x00\
            \xf4\x04\xf7\x42\xe2\x01\xf8\x13\xf7\x44\xfb\xe2";
    let entities = positional_trim_entity_table(
        payload,
        0,
        payload.len(),
        TrimTableClasses {
            table: 66,
            bucket: 67,
            entry: 67,
        },
        Some(68),
    )
    .expect("positional ent_tab");

    assert_eq!(entities.declared_count, Some(7));
    assert_eq!(entities.entity_ref, Some(66));
    assert_eq!(entities.entry_ref, Some(67));
    assert_eq!(entities.solved_external_ids, vec![9]);
    assert_eq!(entities.rows[0].vertices, [3, 4]);
    assert_eq!(entities.rows[0].kind, TrimEntityKind::Line);
}

#[test]
fn positional_trim_entity_table_retains_an_empty_extent() {
    let payload = b"prefix\xf8\x00\xf7\x42\xfb\xe2\
            \xf8\x01\xf7\x44\xfb\xe2";

    let entities = positional_trim_entity_table(
        payload,
        0,
        payload.len(),
        TrimTableClasses {
            table: 66,
            bucket: 67,
            entry: 67,
        },
        Some(68),
    )
    .expect("empty positional ent_tab");

    assert_eq!(entities.declared_count, Some(0));
    assert_eq!(entities.entity_ref, Some(66));
    assert_eq!(entities.entry_ref, Some(67));
    assert!(entities.rows.is_empty());
    assert!(entities.solved_external_ids.is_empty());
}

#[test]
fn positional_trim_entity_table_withholds_rows_without_the_entry_class() {
    let payload = b"prefix\xf8\x01\xf7\x42\xfb\xe2\
            \x00\xe3\x09\x00\x03\x04\xf6\x00";

    let entities = positional_trim_entity_table(
        payload,
        0,
        payload.len(),
        TrimTableClasses {
            table: 66,
            bucket: 67,
            entry: 67,
        },
        None,
    )
    .expect("positional ent_tab header");

    assert_eq!(entities.declared_count, Some(1));
    assert!(entities.rows.is_empty());
    assert!(entities.solved_external_ids.is_empty());
}









#[test]
fn positional_trim_vertex_table_is_independent_of_entity_rows() {
    let payload = b"prefix\xf8\x13\xf7\x44\xfb\xe2\xf7\x45\
            \x01\x02\x03\x00\xe2";
    let vertices = positional_trim_vertex_table(
        payload,
        0,
        payload.len(),
        TrimTableClasses {
            table: 68,
            bucket: 69,
            entry: 69,
        },
        None,
        None,
    )
    .expect("positional vert_tab");

    assert_eq!(vertices.declared_count, Some(19));
    assert_eq!(vertices.entity_ref, Some(68));
    assert_eq!(vertices.entry_ref, Some(69));
    assert_eq!(vertices.rows.len(), 1);
    assert_eq!(vertices.rows[0].vertex_id, 3);
    assert_eq!(vertices.rows[0].entities, [1, 2]);
}

#[test]
fn positional_trim_vertex_table_retains_an_empty_extent() {
    let payload = b"prefix\xf8\x00\xf7\x44\xfb\xe2";

    let vertices = positional_trim_vertex_table(
        payload,
        0,
        payload.len(),
        TrimTableClasses {
            table: 68,
            bucket: 69,
            entry: 69,
        },
        None,
        None,
    )
    .expect("empty positional vert_tab");

    assert_eq!(vertices.declared_count, Some(0));
    assert_eq!(vertices.entity_ref, Some(68));
    assert_eq!(vertices.entry_ref, Some(69));
    assert!(vertices.rows.is_empty());
}

#[test]
fn trim_vertex_uses_unique_shared_point_for_mixed_curves() {
    let segment = |kind, external_id| FeatureSegment {
        kind,
        directions: [None; 3],
        center_id: matches!(kind, FeatureSegmentKind::Arc(_)).then_some(4),
        arc_orientation: matches!(kind, FeatureSegmentKind::Arc(_)).then_some(0),
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id,
        body: Vec::new(),
        offset: 0,
    };
    let segments = FeatureSegmentTable {
        declared_count: 2,
        has_elided_prototype: false,
        entity_ref: None,
        rows: (vec![
            segment(FeatureSegmentKind::Line([1, 2]), 9),
            segment(FeatureSegmentKind::Arc([2, 3]), 10),
        ])
        .into_iter()
        .map(crate::feature::segment_rows::SegmentRow::Ordinary)
        .collect(),
        offset: 0,
    };
    let variables = with_points(
        FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            offset: 0,
        },
        vec![FeatureSectionPoint {
            point_id: 2,
            u: Some(3.0),
            v: Some(4.0),
        }],
    );

    assert_eq!(
        entity_intersection(&[9, 10], Some(&segments), Some(&variables)),
        Some([3.0, 4.0])
    );

    let mut incomplete_segments = segments.clone();
    incomplete_segments.declared_count += 1;
    assert!(!incomplete_segments.is_complete());
    assert!(incomplete_segments.segment(9).is_none());
    assert_eq!(
        incomplete_segments.unique_segment(9),
        Some(&segments.rows.ordinary().cloned().collect::<Vec<_>>()[0])
    );
    assert_eq!(
        entity_intersection(&[9, 10], Some(&incomplete_segments), Some(&variables)),
        Some([3.0, 4.0])
    );

    let mut duplicate_segments = segments.clone();
    duplicate_segments
        .rows
        .insert(crate::feature::segment_rows::SegmentRow::Ordinary(
            segments.rows.ordinary().cloned().collect::<Vec<_>>()[0].clone(),
        ));
    assert!(duplicate_segments.segment(9).is_none());
    assert!(entity_intersection(&[9, 10], Some(&duplicate_segments), Some(&variables)).is_none());

    let mut duplicate_points = variables.clone();
    duplicate_points.rows.extend(variables.rows.clone());
    duplicate_points.declared_count += variables.declared_count;
    assert_eq!(
        reconciled_points(&duplicate_points).0.get(&2),
        Some(&[Some(3.0), Some(4.0)])
    );
    assert_eq!(
        entity_intersection(&[9, 10], Some(&segments), Some(&duplicate_points)),
        Some([3.0, 4.0])
    );
    duplicate_points.rows[2].value = ScalarLane::Value(5.0);
    assert!(reconciled_points(&duplicate_points).1.contains(&2));
    assert!(entity_intersection(&[9, 10], Some(&segments), Some(&duplicate_points)).is_none());
    let row = |variable_type, value, offset| FeatureVariableRow {
        variable_type: crate::feature::definitions::VariableType::from(variable_type),
        key: 2,
        value: ScalarLane::Value(value),
        value_body: Vec::new(),
        guess: ScalarLane::Undefined,
        guess_body: Vec::new(),
        known: None,
        homogeneity: None,
        uvar_id: None,
        offset,
    };
    let mut repeated_raw = variables.clone();
    repeated_raw.rows = vec![row(1, 3.0, 30), row(1, 3.0, 31), row(2, 4.0, 32)];
    assert_eq!(
        reconciled_points(&repeated_raw).0.get(&2),
        Some(&[Some(3.0), Some(4.0)])
    );
    repeated_raw.rows[1].value = ScalarLane::Value(5.0);
    assert!(reconciled_points(&repeated_raw).1.contains(&2));
}

#[test]
fn trim_intersection_refuses_before_entity_node() {
    let segment = |external_id, points| FeatureSegment {
        kind: FeatureSegmentKind::Line(points),
        directions: [None; 3],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id,
        body: Vec::new(),
        offset: 0,
    };
    let segments = FeatureSegmentTable {
        declared_count: 2,
        has_elided_prototype: false,
        entity_ref: None,
        rows: vec![segment(9, [1, 2]), segment(10, [2, 3])]
            .into_iter()
            .map(crate::feature::segment_rows::SegmentRow::Ordinary)
            .collect(),
        offset: 0,
    };
    let variables = with_points(
        FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            offset: 0,
        },
        vec![FeatureSectionPoint {
            point_id: 2,
            u: Some(3.0),
            v: Some(4.0),
        }],
    );
    let error = crate::test_support::last_refusal_at(
        &[0], ResourceDimension::CollectionItems, "creo trim intersection entity nodes",
        |ctx| parse_entity_intersection(ctx, &[9, 10], Some(&segments), Some(&variables)),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo trim intersection entity nodes"));
    crate::test_support::assert_work_boundaries(
        &["creo trim intersection entities", "creo point variable traversal", "creo trim common point"],
        |ctx| parse_entity_intersection(ctx, &[9, 10], Some(&segments), Some(&variables)),
    );
    assert_eq!(
        entity_intersection(&[9, 10], Some(&segments), Some(&variables)),
        Some([3.0, 4.0])
    );
}

#[test]
fn trim_vertex_intersection_resolves_settled_carrier_pairs() {
    let segment = |kind, center_id, radius_ref, external_id| FeatureSegment {
        kind,
        directions: [None; 3],
        center_id,
        arc_orientation: center_id.map(|_| 0),
        vertical_horizontal: None,
        radius_ref,
        radius2_ref: None,
        external_id,
        body: Vec::new(),
        offset: 0,
    };
    let segment_table = |rows: Vec<FeatureSegment>| FeatureSegmentTable {
        declared_count: u32::try_from(rows.len()).expect("fixture value fits u32"),
        has_elided_prototype: false,
        entity_ref: None,
        rows: (rows)
            .into_iter()
            .map(crate::feature::segment_rows::SegmentRow::Ordinary)
            .collect(),
        offset: 0,
    };
    let point = |point_id, u, v| FeatureSectionPoint {
        point_id,
        u: Some(u),
        v: Some(v),
    };
    let radius = |key, value| FeatureVariableRow {
        variable_type: crate::feature::definitions::VariableType::Radius,
        key,
        value: ScalarLane::Value(value),
        value_body: Vec::new(),
        guess: ScalarLane::Undefined,
        guess_body: Vec::new(),
        known: None,
        homogeneity: None,
        uvar_id: None,
        offset: 0,
    };
    let variables = |points: Vec<FeatureSectionPoint>, rows: Vec<FeatureVariableRow>| {
        with_points(
            FeatureVariableTable {
                declared_count: u32::try_from(rows.len()).expect("fixture value fits u32"),
                entity_ref: None,
                rows,
                offset: 0,
            },
            points,
        )
    };

    let bounded_unique = segment_table(vec![
        segment(FeatureSegmentKind::Line([1, 2]), None, None, 9),
        segment(FeatureSegmentKind::Arc([3, 4]), Some(5), Some(6), 10),
    ]);
    let bounded_unique_variables = variables(
        vec![
            point(1, 0.0, 0.0),
            point(2, 2.0, 0.0),
            point(3, 0.0, 1.0),
            point(4, 0.0, -1.0),
            point(5, 0.0, 0.0),
        ],
        vec![radius(6, 1.0)],
    );
    assert_eq!(
        entity_intersection(
            &[9, 10],
            Some(&bounded_unique),
            Some(&bounded_unique_variables),
        ),
        Some([1.0, 0.0])
    );
    let mut incomplete_bounded_unique = bounded_unique.clone();
    incomplete_bounded_unique.declared_count += 1;
    assert_eq!(
        entity_intersection(
            &[9, 10],
            Some(&incomplete_bounded_unique),
            Some(&bounded_unique_variables),
        ),
        Some([1.0, 0.0])
    );
    let mut derived_radius = bounded_unique_variables.clone();
    derived_radius
        .rows
        .retain(|row| row.variable_type != VariableType::Radius);
    derived_radius.declared_count =
        u32::try_from(derived_radius.rows.len()).expect("fixture value fits u32");
    assert_eq!(
        entity_intersection(&[9, 10], Some(&bounded_unique), Some(&derived_radius)),
        Some([1.0, 0.0])
    );
    let conflicting_radius = variables(bounded_unique_variables.points(), vec![radius(6, 2.0)]);
    assert!(
        entity_intersection(&[9, 10], Some(&bounded_unique), Some(&conflicting_radius),).is_none()
    );

    let secant = segment_table(vec![
        segment(FeatureSegmentKind::Line([1, 2]), None, None, 9),
        segment(FeatureSegmentKind::Arc([3, 4]), Some(5), Some(6), 10),
    ]);
    let secant_variables = variables(
        vec![
            point(1, -2.0, 0.0),
            point(2, 2.0, 0.0),
            point(3, 0.0, 1.0),
            point(4, 0.0, -1.0),
            point(5, 0.0, 0.0),
        ],
        vec![radius(6, 1.0)],
    );
    assert!(entity_intersection(&[9, 10], Some(&secant), Some(&secant_variables)).is_none());

    let tangent_circles = segment_table(vec![
        segment(FeatureSegmentKind::Arc([1, 2]), Some(5), Some(6), 9),
        segment(FeatureSegmentKind::Arc([3, 4]), Some(7), Some(8), 10),
    ]);
    let tangent_circle_variables = variables(
        vec![
            point(1, 1.0, 0.0),
            point(2, -1.0, 0.0),
            point(3, 3.0, 0.0),
            point(4, 1.0, 0.0),
            point(5, 0.0, 0.0),
            point(7, 2.0, 0.0),
        ],
        vec![radius(6, 1.0), radius(8, 1.0)],
    );
    assert_eq!(
        entity_intersection(
            &[9, 10],
            Some(&tangent_circles),
            Some(&tangent_circle_variables),
        ),
        Some([1.0, 0.0])
    );

    let secant_circles = segment_table(vec![
        segment(FeatureSegmentKind::Arc([1, 2]), Some(5), Some(6), 9),
        segment(FeatureSegmentKind::Arc([3, 4]), Some(7), Some(8), 10),
    ]);
    let secant_circle_variables = variables(
        vec![
            point(1, 1.0, 0.0),
            point(2, -1.0, 0.0),
            point(3, 1.0, 1.0),
            point(4, 1.0, -1.0),
            point(5, 0.0, 0.0),
            point(7, 1.0, 0.0),
        ],
        vec![radius(6, 1.0), radius(8, 1.0)],
    );
    assert!(entity_intersection(
        &[9, 10],
        Some(&secant_circles),
        Some(&secant_circle_variables),
    )
    .is_none());
}

#[test]
fn trim_vertex_intersection_requires_complete_pairwise_junctions() {
    let segment = |point_ids, external_id| FeatureSegment {
        kind: FeatureSegmentKind::Line(point_ids),
        directions: [None; 3],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id,
        body: Vec::new(),
        offset: 0,
    };
    let segments = FeatureSegmentTable {
        declared_count: 3,
        has_elided_prototype: false,
        entity_ref: None,
        rows: (vec![segment([1, 2], 9), segment([3, 4], 10), segment([5, 6], 11)])
            .into_iter()
            .map(crate::feature::segment_rows::SegmentRow::Ordinary)
            .collect(),
        offset: 0,
    };
    let variables = with_points(
        FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            offset: 0,
        },
        vec![
            FeatureSectionPoint {
                point_id: 1,
                u: Some(-1.0),
                v: Some(-1.0),
            },
            FeatureSectionPoint {
                point_id: 2,
                u: Some(1.0),
                v: Some(1.0),
            },
            FeatureSectionPoint {
                point_id: 3,
                u: Some(-1.0),
                v: Some(1.0),
            },
            FeatureSectionPoint {
                point_id: 4,
                u: Some(1.0),
                v: Some(-1.0),
            },
            FeatureSectionPoint {
                point_id: 5,
                u: Some(0.0),
                v: Some(-2.0),
            },
            FeatureSectionPoint {
                point_id: 6,
                u: Some(0.0),
                v: Some(2.0),
            },
        ],
    );
    assert_eq!(
        entity_intersection(&[9, 10, 11], Some(&segments), Some(&variables)),
        Some([0.0, 0.0])
    );

    let mut incomplete = variables.clone();
    incomplete.declared_count = 1;
    assert!(entity_intersection(&[9, 10, 11], Some(&segments), Some(&incomplete)).is_none());
}

#[test]
fn trim_vertex_template_identifies_table_and_entry_classes() {
    let payload = b"vert_tab\0\xf8\x13\xf7\x44\xfb\xe2\
            attrs\0\xf1\xf7\x46\xe3bucket_xar\0\xf8\x01\xf7\x46\xfb\xe3\
            \xf7\x45\x09\x0a\x03\x00";

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| trim_table_header(
            ctx,
            payload,
            b"vert_tab\0",
            0,
            payload.len()
        ))
        .expect("trim header search admitted"),
        Some(TrimTableHeader {
            declared_count: 19,
            classes: TrimTableClasses {
                table: 68,
                bucket: 70,
                entry: 69,
            },
        })
    );
}

#[test]
fn trim_buckets_require_the_complete_declared_sequence_and_counts() {
    let payload = b"bucket_index\0\x00bucket_xar\0\xf8\x01\xf7\x43\xfb\xe3\
            \xf7\x44\x09\x0a\x03\x00\xe2\x01\xf8\x01\xf7\x43\xfb\xe3\
            \xf7\x44\x09\x0a\x03\x00\xe2\x02\xf1\xf7\x42\xe2\x03\xe2\
            \x04\xf0\xf7\x43\xf8\x01\xf7\x43\xfb\xe3\xf7\x44\x0b\x0c\
            \x05\x00\xe2\x05\xf8\x01\xf7\x43\xfb\xe3\xf7\x44\x0d\x0e\
            \x06\x00\xe2\x06\xe0\x00next\0";
    let header = TrimTableHeader {
        declared_count: 7,
        classes: TrimTableClasses {
            table: 66,
            bucket: 67,
            entry: 68,
        },
    };

    assert_eq!(
        trim_buckets(payload, 0, payload.len(), header, TrimEntryKind::Vertex)
            .iter()
            .map(|bucket| (
                bucket.index,
                bucket.declared_entry_count,
                bucket.decoded_entry_count
            ))
            .collect::<Vec<_>>(),
        (0..7)
            .zip([1, 1, 0, 0, 1, 1, 0])
            .map(|(index, count)| (index, count, Some(count)))
            .collect::<Vec<_>>()
    );
    let truncated = payload
        .windows(2)
        .position(|bytes| bytes == [0xe2, 0x06])
        .expect("last bucket index");
    assert_eq!(
        trim_buckets(payload, 0, truncated, header, TrimEntryKind::Vertex)
            .iter()
            .map(|bucket| bucket.index)
            .collect::<Vec<_>>(),
        (0..6).collect::<Vec<_>>()
    );
}

#[test]
fn trim_bucket_completeness_rejects_missing_and_extra_vertex_entries() {
    let header = TrimTableHeader {
        declared_count: 1,
        classes: TrimTableClasses {
            table: 66,
            bucket: 67,
            entry: 68,
        },
    };
    let missing = b"bucket_index\0\x00bucket_xar\0\xf8\x02\xf7\x43\xfb\xe3\
            \xf7\x44\x01\x02\x03\x00\xe0";
    let buckets = trim_buckets(missing, 0, missing.len(), header, TrimEntryKind::Vertex);
    assert_eq!(buckets[0].declared_entry_count, 2);
    assert_eq!(buckets[0].decoded_entry_count, Some(1));
    assert!(!buckets[0].is_complete());

    let extra = b"bucket_index\0\x00bucket_xar\0\xf8\x01\xf7\x43\xfb\xe3\
            \xf7\x44\x01\x02\x03\x00\xe3\x04\x05\x06\x00\xe0";
    let buckets = trim_buckets(extra, 0, extra.len(), header, TrimEntryKind::Vertex);
    assert_eq!(buckets[0].declared_entry_count, 1);
    assert_eq!(buckets[0].decoded_entry_count, Some(2));
    assert!(!buckets[0].is_complete());
}

#[test]
fn trim_vertex_entries_retain_variable_incident_entity_counts() {
    let counted = b"\xf8\x03\x0a\x0b\x0c\x07\x00";
    assert_eq!(
        trim_vertex_entry(counted, 0, counted.len()),
        Some((vec![10, 11, 12], 7, counted.len()))
    );
    let direct = b"\x0a\x0b\x0c\x07\x00";
    assert_eq!(
        trim_vertex_entry(direct, 0, direct.len()),
        Some((vec![10, 11, 12], 7, direct.len()))
    );
}

#[test]
fn trim_entity_bucket_counts_the_named_prototype_and_complete_bodies() {
    let payload = b"bucket_index\0\x00bucket_xar\0\xf8\x02\xf7\x43\xfb\xe3\
            entry_ptr(entity_entry)\0\xe3xid\0\x00ent_mode\0\x00start_vtx\0\xf6\
            end_vtx\0\xf6center_vtx\0\xf6pers_attribs\0\x00\
            \xf4\x04\xf7\x42\xe2\xe3\
            \x09\x00\x03\x04\xf6\x00\xe0";
    let header = TrimTableHeader {
        declared_count: 1,
        classes: TrimTableClasses {
            table: 66,
            bucket: 67,
            entry: 68,
        },
    };
    let buckets = trim_buckets(payload, 0, payload.len(), header, TrimEntryKind::Entity);
    assert_eq!(buckets[0].decoded_entry_count, Some(2));
    assert!(buckets[0].is_complete());

    let truncated = payload.len() - 2;
    let buckets = trim_buckets(payload, 0, truncated, header, TrimEntryKind::Entity);
    assert_eq!(buckets[0].decoded_entry_count, Some(1));
    assert!(!buckets[0].is_complete());
}

#[test]
fn a_bucket_that_states_no_decoded_entry_count_is_not_complete() {
    // `None` states that the scan decoded more entries than the stored `u32`
    // count can be compared against, so the bucket states no completeness.
    let unstatable = crate::feature::definitions::FeatureTrimBucket {
        index: 0,
        declared_entry_count: 2,
        decoded_entry_count: None,
        offset: 0,
    };
    assert!(!unstatable.is_complete());

    let complete = crate::feature::definitions::FeatureTrimBucket {
        decoded_entry_count: Some(2),
        ..unstatable.clone()
    };
    assert!(complete.is_complete());

    let short = crate::feature::definitions::FeatureTrimBucket {
        decoded_entry_count: Some(1),
        ..unstatable
    };
    assert!(!short.is_complete());
}

trim_bucket_collection_limit_test!(
    trim_bucket_starts_refuse_before_vec_growth,
    "creo trim bucket starts"
);

trim_bucket_collection_limit_test!(
    trim_bucket_results_refuse_before_vec_growth,
    "creo trim buckets"
);

trim_bucket_collection_limit_test!(
    trim_bucket_vertex_nodes_refuse_before_btree_insertion,
    "creo trim bucket vertex nodes"
);

trim_entity_collection_limit_test!(
    trim_entity_id_nodes_refuse_before_btree_insertion,
    "creo trim entity ID nodes"
);

trim_entity_collection_limit_test!(
    trim_entity_rows_refuse_before_vec_growth,
    "creo trim entity rows"
);

trim_entity_collection_limit_test!(
    trim_entity_solved_ids_refuse_before_vec_growth,
    "creo trim entity solved IDs"
);

#[test]
fn trim_parser_scans_refuse_at_work_boundaries() {
    let payload = b"noise\xf8\x02\xf7\x42\xfb\xe2\xf7\x43\x00\xe3\x09\x00\x03\x04\xf6\x00";
    let table = crate::test_support::assert_work_boundaries(
        &["creo positional trim table", "creo positional trim entity class", "creo trim row traversal", "creo trim solved ID traversal"],
        |ctx| parse_positional_trim_entity_table(ctx, payload, 0, payload.len(), TrimTableClasses { table: 66, bucket: 67, entry: 67 }, None),
    ).expect("trim entity table");
    assert_eq!(table.solved_external_ids, [9]);
    let payload = b"\xf8\x02\x09\x0a\x03\x00";
    let entry = crate::test_support::assert_work_boundaries(
        &["creo trim vertex entity traversal"],
        |ctx| parse_trim_vertex_entry(ctx, payload, 0, payload.len()),
    ).expect("trim vertex");
    assert_eq!(entry, (vec![9, 10], 3, payload.len()));
}

mod numerical;
mod range;
