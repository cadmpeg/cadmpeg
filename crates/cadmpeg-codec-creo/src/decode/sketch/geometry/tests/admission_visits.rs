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
    crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, &[], |cap| {
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
        let result = run();
        let refused = result.is_err();
        if !refused {
            assert_eq!(result.expect("only present rows visited"), None);
            let original = ctx.charge_work_limit(1, "after saved line boundary").expect_err("exact work");
            assert_eq!((original.used, original.additional), (visits, 1));
            for _ in 0..2 {
                assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            }
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            Ok(())
        } else {
            let Err(CodecError::ResourceLimit(original)) = result else { panic!("present row must refuse"); };
            assert_eq!((original.dimension, original.used, original.additional, original.limit),
                (ResourceDimension::WorkUnits, cap, 1, cap));
            assert_eq!(original.operation, if cap < 1 + u64::from(has_previous) {
                "creo saved line segment position rows"
            } else { "creo saved line previous segment rows" });
            for _ in 0..2 {
                assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            }
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            Err(original.into())
        }
    });
}

#[test]
fn saved_line_first_row_has_no_previous_visit() { assert_saved_line_boundary(false); }

#[test]
fn saved_line_last_row_has_no_next_visit() { assert_saved_line_boundary(true); }

fn assert_saved_arc_record_visits(
    run: impl Fn(
        &DecodeContext<'_>,
        &crate::feature::definitions::FeatureDefinition,
        &crate::feature::definitions::FeatureSegment,
    ) -> Result<bool, CodecError>,
) {
    for count in 1..=4 {
        let (mut definition, segment) =
            super::saved_arc_carrier_definition([Some(0.0); 3], Some(1.0));
        let saved = definition.saved_section.as_mut().expect("saved section");
        let target = saved.entities.pop().expect("saved arc");
        for index in 1..count {
            let mut candidate = target.clone();
            let crate::feature::definitions::FeatureSavedEntity::Arc(arc) = &mut candidate else {
                panic!("arc fixture");
            };
            arc.entity_id += u32::try_from(index).expect("fixture index");
            arc.offset += index;
            saved.entities.push(candidate);
        }
        saved.entities.push(target);
        // One complete unique-record walk plus its current core end probe.
        // Fixed-field carrier, angle and point conversions allocate no backing.
        let visits = u64::try_from(count).expect("fixture count") + 1;
        crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, &[], |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_collection_items = 0;
            policy.limits.max_entities = 0;
            policy.limits.max_recursion_depth = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = run(&ctx, &definition, &segment);
            let refused = result.is_err();
            let original = if !refused {
                assert!(result.expect("one record walk admitted"));
                let refusal = ctx.charge_work_limit(
                    1, "after saved arc record projection",
                ).expect_err("exact source work used");
                assert_eq!(refusal.used, visits);
                refusal
            } else {
                let Err(CodecError::ResourceLimit(refusal)) = result else {
                    panic!("present record visit must refuse");
                };
                assert_eq!((refusal.dimension, refusal.operation, refusal.used, refusal.additional, refusal.limit),
                    (ResourceDimension::WorkUnits, "creo saved section entities", cap, 1, cap));
                refusal
            };
            for _ in 0..2 {
                assert!(matches!(run(&ctx, &definition, &segment),
                    Err(CodecError::ResourceLimit(actual)) if actual == original));
            }
            assert!(matches!(ctx.finish_session(),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
            if refused { Err(original.into()) } else { Ok(()) }
        });
    }
}

#[test]
fn saved_arc_geometry_reuses_one_validated_record() {
    assert_saved_arc_record_visits(|ctx, definition, segment| {
        super::super::saved_section_arc(ctx, definition, segment).map(|arc| arc == Some(
            super::super::SavedSectionArc {
                center: cadmpeg_ir::units::FinitePoint2::new(cadmpeg_ir::math::Point2::new(0.0, 0.0)).expect("finite center"),
                radius: cadmpeg_ir::scalar::PositiveLength::new(1.0).expect("positive radius"),
                start_angle: cadmpeg_ir::scalar::Angle::new(std::f64::consts::FRAC_PI_2).expect("start angle"),
                end_angle: cadmpeg_ir::scalar::Angle::FULL_TURN,
            }
        ))
    });
}

#[test]
fn saved_arc_point_projection_reuses_one_validated_record() {
    assert_saved_arc_record_visits(|ctx, definition, segment| {
        super::super::saved_section_segment_point_coordinates(ctx, definition, segment)
            .map(|points| points == Some([
                Some((1, [1.0, 0.0])), Some((2, [0.0, 1.0])), Some((3, [0.0, 0.0])),
            ]))
    });
}
