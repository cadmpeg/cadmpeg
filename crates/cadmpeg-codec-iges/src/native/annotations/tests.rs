// SPDX-License-Identifier: Apache-2.0
use super::{NativeTextRun, Subject};
use crate::global::GlobalTable;
use crate::graph::ParameterResolver;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceLimit};
use cadmpeg_core::CodecError;

fn with_absent_record(mut run: impl FnMut(&Subject<'_, '_, '_>, Option<ResourceLimit>)) {
    for dimension in [None, Some(ResourceDimension::WorkUnits), Some(ResourceDimension::CollectionItems),
        Some(ResourceDimension::MaterializedBytes), Some(ResourceDimension::RetainedBytes),
        Some(ResourceDimension::Entities), Some(ResourceDimension::RecursionDepth)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty caller input");
        let resolver = ParameterResolver::new(&[], &ctx).expect("caller resolver before refusal");
        let subject = Subject {
            sequence: 1, form: 0, record: None, primary_end: 0,
            entries: &[], parameter_resolver: &resolver, ctx: &ctx,
            v5_null_string_rule: false,
        };
        let original = dimension.map(|dimension| {
            let refused = match dimension {
                ResourceDimension::WorkUnits => ctx.charge_work(1, "test original native annotation refusal"),
                ResourceDimension::CollectionItems => ctx.charge_collection_items(1, "test original native annotation refusal"),
                ResourceDimension::MaterializedBytes => ctx.reserve_scoped(1, "test original native annotation refusal").map(|_| ()),
                ResourceDimension::RetainedBytes => ctx.charge_retained(1, "test original native annotation refusal"),
                ResourceDimension::Entities => ctx.charge_entities(1, "test original native annotation refusal"),
                ResourceDimension::RecursionDepth => ctx.enter_nested("test original native annotation refusal").map(|_| ()),
                _ => panic!("native annotation refusal dimension"),
            };
            let Err(CodecError::ResourceLimit(first)) = refused else { panic!("original refusal"); };
            assert_eq!(first.dimension, dimension);
            first
        });
        for _ in 0..64 { run(&subject, original); }
        drop(resolver);
        match original {
            Some(first) => assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)),
            None => ctx.finish_session().expect("fresh caller session"),
        }
    }
}

fn absent_link(result: Result<Option<String>, CodecError>, original: Option<ResourceLimit>) {
    match original {
        Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
        None => assert!(result.expect("absent native link is free").is_none()),
    }
}

#[test]
fn native_note_without_record_preserves_original_refusal() {
    with_absent_record(|subject, original| absent_link(subject.note_link(1), original));
}

#[test]
fn native_leader_without_record_preserves_original_refusal() {
    with_absent_record(|subject, original| absent_link(subject.leader_link(1), original));
}

#[test]
fn native_witness_without_record_preserves_original_refusal() {
    with_absent_record(|subject, original| absent_link(subject.witness_link(1), original));
}

#[test]
fn native_curve_without_record_preserves_original_refusal() {
    with_absent_record(|subject, original| absent_link(subject.curve_link(1, GlobalTable::V5Later), original));
}

#[test]
fn native_ordinate_without_record_preserves_original_refusal() {
    with_absent_record(|subject, original| absent_link(subject.ordinate_link(1), original));
}

#[test]
fn native_enclosure_without_record_preserves_original_refusal() {
    with_absent_record(|subject, original| absent_link(subject.enclosure_link(1, GlobalTable::V5Later), original));
}

#[test]
fn native_geometry_without_record_preserves_original_refusal() {
    with_absent_record(|subject, original| absent_link(subject.geometry_link(1, GlobalTable::V5Later), original));
}

#[test]
fn native_section_boundary_without_record_preserves_original_refusal() {
    with_absent_record(|subject, original| absent_link(subject.section_boundary_link(1), original));
}

#[test]
fn native_text_run_without_record_preserves_original_refusal() {
    with_absent_record(|subject, original| {
        let result = subject.text_run(2);
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert_eq!(result.expect("absent native text run is free"), NativeTextRun {
                declared_character_count: None, text: None, box_size: [None; 2],
                font_code: None, font_definition: None, slant_angle: None,
                rotation_angle: None, mirror: None, vertical: None, start: [None; 3],
            }),
        }
    });
}
