// SPDX-License-Identifier: Apache-2.0

use crate::feature::definitions::{FeatureOrderRow, FeatureSegmentTable};
use crate::feature::segment_rows::SegmentRow;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn assert_saved_line_boundary(has_previous: bool) {
    let (mut definition, mut segment) = super::saved_arc_carrier_definition([None; 3], None);
    segment.kind = super::FeatureSegmentKind::Line([1, 2]);
    definition.saved_section = None;
    let order = definition.order_table.as_mut().expect("order table");
    order.rows.clear();
    order.declared_count = u32::from(has_previous);
    let mut rows = Vec::new();
    if has_previous {
        let mut previous = segment.clone();
        previous.external_id = 3;
        previous.offset = 0;
        rows.push(SegmentRow::Ordinary(previous));
        order.rows.push(FeatureOrderRow {
            external_id: 3, internal_id: 30, bitmask: 0, offset: 0,
        });
    }
    rows.push(SegmentRow::Ordinary(segment.clone()));
    definition.segments = Some(FeatureSegmentTable {
        declared_count: 1 + u32::from(has_previous), has_elided_prototype: false,
        entity_ref: None, rows: rows.into_iter().collect(), offset: 0,
    });
    // One position visit for the first row. The last-row route has two
    // position visits and one previous-row visit; its next source is empty.
    let visits = if has_previous { 3 } else { 1 };
    for cap in 0..=visits {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let run = || super::super::saved_section_line_geometry(&ctx, &definition, &segment);
        if cap == visits {
            assert_eq!(run().expect("only present rows visited"), None);
            let original = ctx.charge_work_limit(1, "after saved line boundary").expect_err("exact work");
            assert_eq!((original.used, original.additional), (visits, 1));
            for _ in 0..2 {
                assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            }
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        } else {
            let Err(CodecError::ResourceLimit(original)) = run() else { panic!("present row must refuse"); };
            assert_eq!((original.dimension, original.used, original.additional, original.limit),
                (ResourceDimension::WorkUnits, cap, 1, cap));
            assert_eq!(original.operation, if cap < 1 + u64::from(has_previous) {
                "creo saved line segment position rows"
            } else { "creo saved line previous segment rows" });
            for _ in 0..2 {
                assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            }
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
    }
}

#[test]
fn saved_line_first_row_has_no_previous_visit() { assert_saved_line_boundary(false); }

#[test]
fn saved_line_last_row_has_no_next_visit() { assert_saved_line_boundary(true); }
