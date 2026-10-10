// SPDX-License-Identifier: Apache-2.0

use super::{hem_frame, reference_run, rolled_hem_frame, teardrop_hem_frame, HemFixture};
use super::super::exact_hem_operation;
use crate::records::feature::sheet_metal::DesignHemParameterOwners;
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::cell::Cell;

const ORIGINAL: &str = "original F3D Hem parameter-kind refusal";

fn forms() -> [(super::HemFrame, Vec<u32>, Vec<(u32, &'static str)>); 3] {
    [
        (hem_frame(&HemFixture { header_shift: 0, wrapper: 308, settings: 311, gap_owner: 301, length_owner: 304, aggregate_group: 240, edge_group: 251, bend_radius: 0.25 }), vec![240, 243, 251, 254, 301, 304, 308, 311], vec![(301, "HemGap"), (304, "HemLength")]),
        (rolled_hem_frame(), vec![708, 717, 720, 724, 775, 788, 790, 793], vec![(775, "HemRadius"), (788, "HemAngle")]),
        (teardrop_hem_frame(), vec![703, 706, 708, 717, 720, 724, 775, 777, 780], vec![(703, "HemGap"), (706, "HemLength"), (775, "HemRadius")]),
    ]
}

#[test]
fn hem_each_parameter_callback_keeps_original_refusal_before_bool_or_ordinary_error() {
    for (frame, references, kinds) in forms() {
        let native = frame.bytes.clone();
        for refused_at in 0..kinds.len() {
            for outcome in 0..3 {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = u64::MAX;
                let (ctx, _) = DecodeContext::from_root_bytes(&frame.bytes, &arena, &policy).unwrap();
                let calls = Cell::new(0);
                let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, ORIGINAL, Some(1));
                let result = exact_hem_operation(&ctx, &frame.bytes, 0, frame.paired_at, &reference_run(&references), &|record, expected| {
                    let ordinal = calls.get(); calls.set(ordinal + 1);
                    assert_eq!((record, expected), kinds[ordinal]);
                    if ordinal != refused_at { return Ok(true); }
                    assert!(matches!(ctx.charge_work(1, ORIGINAL), Err(CodecError::ResourceLimit(_))));
                    match outcome { 0 => Ok(false), 1 => Ok(true), _ => Err(CodecError::Malformed("ordinary Hem callback error".into())) }
                });
                let Err(CodecError::ResourceLimit(original)) = result else { panic!("original Hem callback refusal"); };
                assert_eq!(original.operation, ORIGINAL);
                assert_eq!(original.additional, 1);
                assert_eq!(calls.get(), refused_at + 1);
                assert_eq!(ctx.resource_refusal(), Some(original.clone()));
                assert!(matches!(ctx.charge_work(0, "later Hem boundary"), Err(CodecError::ResourceLimit(later)) if later == original));
            }
        }
        assert_eq!(frame.bytes, native);
    }
}

#[test]
fn hem_fixed_forms_and_literal_callbacks_need_no_positive_work_and_keep_owner_order() {
    for (frame, references, kinds) in forms() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service(); policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&frame.bytes, &arena, &policy).unwrap();
        let calls = Cell::new(0);
        let operation = exact_hem_operation(&ctx, &frame.bytes, 0, frame.paired_at, &reference_run(&references), &|record, expected| {
            let ordinal = calls.get(); calls.set(ordinal + 1);
            assert_eq!((record, expected), kinds[ordinal]); Ok(true)
        }).unwrap().expect("exact fixed Hem form");
        assert_eq!(calls.get(), kinds.len());
        assert_eq!(operation.bend_radius.get(), 0.25);
        assert_eq!(operation.bend_radius_offset, frame.bend_radius_offset);
        match operation.parameter_owners {
            DesignHemParameterOwners::GapLength { gap_owner_record_index, length_owner_record_index } => assert_eq!([gap_owner_record_index, length_owner_record_index], [301, 304]),
            DesignHemParameterOwners::RadiusAngle { radius_owner_record_index, angle_owner_record_index } => assert_eq!([radius_owner_record_index, angle_owner_record_index], [775, 788]),
            DesignHemParameterOwners::GapLengthRadius { gap_owner_record_index, length_owner_record_index, radius_owner_record_index } => assert_eq!([gap_owner_record_index, length_owner_record_index, radius_owner_record_index], [703, 706, 775]),
        }
        assert!(ctx.resource_refusal().is_none()); ctx.finish_session().unwrap();
    }
}

#[test]
fn hem_literal_false_stops_before_remaining_kinds_and_keeps_ordinary_error() {
    for (frame, references, kinds) in forms() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service(); policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&frame.bytes, &arena, &policy).unwrap();
        let calls = Cell::new(0);
        assert!(exact_hem_operation(&ctx, &frame.bytes, 0, frame.paired_at, &reference_run(&references), &|record, expected| {
            calls.set(calls.get() + 1); assert_eq!((record, expected), kinds[0]); Ok(false)
        }).unwrap().is_none());
        assert_eq!(calls.get(), 1);
        assert!(matches!(exact_hem_operation(&ctx, &frame.bytes, 0, frame.paired_at, &reference_run(&references), &|_, _| Err(CodecError::Malformed("ordinary Hem callback error".into()))), Err(CodecError::Malformed(message)) if message == "ordinary Hem callback error"));
        assert!(ctx.resource_refusal().is_none());
    }
}

#[test]
fn hem_preexisting_original_fuse_dominates_invalid_fixed_frame_without_callback() {
    let mut policy = DecodePolicy::service(); policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, ORIGINAL) else { panic!("zero-work refusal"); };
        assert!(matches!(exact_hem_operation(ctx, &[], usize::MAX, 0, &reference_run(&[]), &|_, _| panic!("unvisited Hem callback")), Err(CodecError::ResourceLimit(later)) if later == original));
    });
}
