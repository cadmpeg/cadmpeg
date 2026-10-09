// SPDX-License-Identifier: Apache-2.0
//! PMI work admission follows the executed slice prefix.

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::MeasureParameters;
use crate::parse::Value;

#[test]
fn pmi_measure_item_failure_does_not_admit_unused_suffix() {
    let values = vec![Value::Omitted, Value::Omitted];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut visits = 0;
        let result = MeasureParameters::Items(&values).visit(ctx, |_| {
            visits += 1;
            Err(CodecError::malformed("test first item failure"))
        });
        assert!(matches!(result, Err(CodecError::Malformed(message))
            if message == "test first item failure"));
        assert_eq!(visits, 1);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn pmi_measure_record_failure_admits_only_first_partial_and_parameter() {
    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=(ITEM($,$) OTHER($));ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::test_support::with_service_context(
        source.as_bytes(), crate::parse::parse_inner,
    ).expect("complex record");
    let record = exchange.records().get(&1).expect("record");
    let mut policy = DecodePolicy::service();
    // One partial and its first parameter execute before the visitor fails.
    policy.limits.max_work_units = 2;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut visits = 0;
        let result = MeasureParameters::Record(record).visit(ctx, |_| {
            visits += 1;
            Err(CodecError::malformed("test first parameter failure"))
        });
        assert!(matches!(result, Err(CodecError::Malformed(message))
            if message == "test first parameter failure"));
        assert_eq!(visits, 1);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn pmi_measure_empty_items_require_no_work() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut visits = 0;
        MeasureParameters::Items(&[]).visit(ctx, |_| {
            visits += 1;
            Ok(())
        }).expect("empty source");
        assert_eq!(visits, 0);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn pmi_measure_empty_items_preserve_original_refusal() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let CodecError::ResourceLimit(first) = ctx.charge_work(1, "test original caller refusal")
            .expect_err("caller refuses") else { panic!("resource refusal"); };
        let mut visits = 0;
        assert!(matches!(MeasureParameters::Items(&[]).visit(ctx, |_| {
            visits += 1;
            Ok(())
        }), Err(CodecError::ResourceLimit(refusal)) if refusal == first));
        assert_eq!(visits, 0);
        assert_eq!(ctx.resource_refusal(), Some(first));
    });
}

#[test]
fn pmi_measure_first_item_refuses_before_visitor() {
    let values = vec![Value::Omitted];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut visits = 0;
        let CodecError::ResourceLimit(refusal) = MeasureParameters::Items(&values).visit(ctx, |_| {
            visits += 1;
            Ok(())
        }).expect_err("first visit needs one work unit") else { panic!("resource refusal"); };
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!(refusal.operation, "STEP measure item traversal");
        assert_eq!(refusal.used, 0);
        assert_eq!(refusal.additional, 1);
        assert_eq!(visits, 0);
        assert_eq!(ctx.resource_refusal(), Some(refusal));
    });
}

#[test]
fn pmi_measure_single_item_has_no_terminal_work_charge() {
    let values = vec![Value::Omitted];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut visits = 0;
        MeasureParameters::Items(&values).visit(ctx, |_| {
            visits += 1;
            Ok(())
        }).expect("one item fits");
        assert_eq!(visits, 1);
        let CodecError::ResourceLimit(refusal) = ctx.charge_work(1, "test work observation")
            .expect_err("one item used the only unit") else { panic!("resource refusal"); };
        assert_eq!(refusal.operation, "test work observation");
        assert_eq!(refusal.used, 1);
        assert_eq!(refusal.additional, 1);
    });
}

#[test]
fn pmi_reference_first_value_refuses_before_reference_scan() {
    let values = vec![Value::Reference(1), Value::Reference(2)];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let CodecError::ResourceLimit(refusal) = super::super::collect_pmi_references(
            &values, ctx, "test reference collection",
        ).expect_err("first value needs one work unit") else { panic!("resource refusal"); };
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!(refusal.operation, "STEP collect pmi references traversal");
        assert_eq!(refusal.used, 0);
        assert_eq!(refusal.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(refusal));
    });
}

fn invisibility_exchange(records: usize) -> crate::parse::Exchange {
    use std::fmt::Write as _;
    let mut source = String::from("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;");
    for id in 1..=records {
        write!(&mut source, "#{id}=ITEM();").expect("fixture record");
    }
    source.push_str("ENDSEC;END-ISO-10303-21;");
    crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("fixture exchange")
        .0
}

#[test]
fn pmi_hidden_record_first_visit_refuses_before_unused_suffix() {
    let exchange = invisibility_exchange(8193);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let CodecError::ResourceLimit(refusal) = super::super::hidden_presentation_annotation_ids(
            &exchange, ctx,
        ).expect_err("first record visit needs one work unit") else { panic!("resource refusal"); };
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!(refusal.operation, "STEP hidden presentation annotation ids map traversal");
        assert_eq!(refusal.used, 0);
        assert_eq!(refusal.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(refusal));
    });
}

#[test]
fn pmi_hidden_record_callback_refusal_precedes_unused_suffix() {
    let exchange = invisibility_exchange(8193);
    let mut policy = DecodePolicy::service();
    // The first map member executes, then its first partial search refuses.
    policy.limits.max_work_units = 1;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let CodecError::ResourceLimit(refusal) = super::super::hidden_presentation_annotation_ids(
            &exchange, ctx,
        ).expect_err("first partial search needs one more work unit") else { panic!("resource refusal"); };
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!(refusal.operation, "STEP partial record search");
        assert_eq!(refusal.used, 1);
        assert_eq!(refusal.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(refusal));
    });
}

#[test]
fn pmi_hidden_empty_records_require_no_work() {
    let exchange = invisibility_exchange(0);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        assert!(super::super::hidden_presentation_annotation_ids(&exchange, ctx)
            .expect("empty source").is_empty());
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn pmi_hidden_empty_records_preserve_original_refusal() {
    let exchange = invisibility_exchange(0);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let CodecError::ResourceLimit(first) = ctx.charge_work(1, "test original caller refusal")
            .expect_err("caller refuses") else { panic!("resource refusal"); };
        assert!(matches!(super::super::hidden_presentation_annotation_ids(&exchange, ctx),
            Err(CodecError::ResourceLimit(refusal)) if refusal == first));
        assert_eq!(ctx.resource_refusal(), Some(first));
    });
}

#[test]
fn pmi_shape_aspect_target_builder_preserves_deduplication_and_order() {
    use cadmpeg_ir::pmi::PmiTarget;
    crate::test_support::with_service_context(b"", |_, ctx| {
        let targets = super::super::targets([Ok(2), Ok(1), Ok(2)], ctx).expect("targets");
        assert_eq!(targets, [
            PmiTarget::ShapeAspect { source_id: cadmpeg_core::nonblank_literal!("#2") },
            PmiTarget::ShapeAspect { source_id: cadmpeg_core::nonblank_literal!("#1") },
        ]);
    });
}
