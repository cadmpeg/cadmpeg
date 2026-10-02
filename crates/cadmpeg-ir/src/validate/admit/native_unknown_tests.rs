// SPDX-License-Identifier: Apache-2.0

use super::{admit_with_annotations, admit_with_native_unknowns, RHINO_DRAFT_CHECKS};
use crate::unknown::{NativeUnknownRecord, UnknownRecord};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn native_unknown_admission_matches_product_projection_without_mutation() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut ir = crate::examples::unit_cube().unwrap();
    let old_id = "test:source:unknown#old";
    let new_id = "test:source:unknown#new";
    ir.set_native_unknowns(&ctx, "rhino", &[NativeUnknownRecord {
        id: old_id.try_into().unwrap(),
        links: vec!["test:model:missing#old".try_into().unwrap()],
    }]).unwrap();
    for (format, arena) in [("alpha", "records"), ("rhino", "zz"), ("zeta", "records")] {
        let id = format!("test:native:record#{format}");
        ir.native.namespace_mut(format).arenas_mut().insert(arena.into(), vec![crate::NativeRecord::new(
            id.as_str().try_into().unwrap(), serde_json::Map::new(),
        ).unwrap()]);
    }
    let raw = vec![UnknownRecord::retained(new_id.try_into().unwrap(), 72, vec![23; 65536], vec![ir.model.points[0].id.as_str().to_owned()])];
    let pointer = raw[0].data().unwrap().as_ptr();
    let mut builder = crate::AnnotationBuilder::new();
    let stream = crate::annotations::StreamHandle::new(crate::stream_name!("test:source"));
    builder.note(&cadmpeg_test_support::service_decode_context(), new_id, &stream, 72, None).unwrap();
    for path in ["id", "links", "links.0", "links.1", "offset", "retention"] {
        builder.derived(&cadmpeg_test_support::service_decode_context(), new_id, path).unwrap();
    }
    let annotations = builder.build();
    let before = serde_json::to_value(&ir).unwrap();
    let actual = admit_with_native_unknowns(&ctx, &ir, ("rhino", &raw), Some(&annotations), RHINO_DRAFT_CHECKS, Vec::new()).unwrap().unwrap();
    let mut projected = ir.clone();
    projected.set_native_unknowns_from(&ctx, "rhino", raw.iter().map(|record| NativeUnknownRecord::try_from(record).unwrap())).unwrap();
    let expected = admit_with_annotations(&ctx, &projected, &annotations, RHINO_DRAFT_CHECKS, Vec::new()).unwrap();
    assert_eq!(serde_json::to_value(&actual).unwrap(), serde_json::to_value(&expected).unwrap());
    assert_eq!(serde_json::to_value(&ir).unwrap(), before);
    assert_eq!(raw[0].data().unwrap().as_ptr(), pointer);
    assert_eq!(actual.findings.iter().filter(|finding| finding.check == crate::report::check::Check::Annotations).count(), 3);
    assert!(!actual.findings.iter().any(|finding| finding.message.contains("missing#old")));
}

#[test]
fn native_unknown_admission_keeps_unresolved_outgoing_link_findings() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let ir = crate::CadIr::empty();
    let raw = [UnknownRecord::retained("test:source:unknown#0".try_into().unwrap(), 0, Vec::new(), vec!["test:model:missing#0".into()])];
    let actual = admit_with_native_unknowns(&ctx, &ir, ("rhino", &raw), None, RHINO_DRAFT_CHECKS, Vec::new()).unwrap().unwrap();
    let links = actual.findings.iter().filter(|finding| finding.check == crate::report::check::Check::NativeLinks).collect::<Vec<_>>();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].entity.as_deref(), Some("test:source:unknown#0"));
    assert_eq!(links[0].message, "native-record link `test:model:missing#0` does not resolve");
}

#[test]
fn native_unknown_admission_preserves_outer_resource_refusals() {
    let ir = crate::CadIr::empty();
    let raw = [UnknownRecord::retained("test:source:unknown#0".try_into().unwrap(), 0, vec![42; 8192], Vec::new())];
    for dimension in [ResourceDimension::WorkUnits, ResourceDimension::RecursionDepth, ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems, ResourceDimension::RetainedBytes] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(limit)) = admit_with_native_unknowns(&ctx, &ir, ("rhino", &raw), None, RHINO_DRAFT_CHECKS, Vec::new()) else { panic!("resource refusal remains outside semantic result"); };
        assert_eq!(limit.dimension, dimension);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
        assert!(ir.native.0.is_empty());
        assert_eq!(raw[0].data().unwrap(), &[42; 8192]);
    }
}

#[test]
fn native_unknown_admission_preserves_product_link_grammar_failures() {
    let ir = crate::CadIr::empty();
    for link in ["", "missing", "test:model:body#", "test:model:body#with space"] {
        let ctx = cadmpeg_test_support::service_decode_context();
        let raw = [UnknownRecord::retained("test:source:unknown#0".try_into().unwrap(), 0, vec![9], vec![link.into()])];
        let expected = NativeUnknownRecord::try_from(&raw[0]).unwrap_err();
        let error = admit_with_native_unknowns(&ctx, &ir, ("rhino", &raw), None, RHINO_DRAFT_CHECKS, Vec::new()).unwrap().unwrap_err();
        assert_eq!(error.to_string(), expected.to_string());
        ctx.finish_session().unwrap();
        assert!(ir.native.0.is_empty());
    }
}

#[test]
fn native_unknown_annotation_collision_preserves_first_native_arena() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for native_format in ["alpha", "zeta"] {
        let mut ir = crate::CadIr::empty();
        let id = "test:source:unknown#collision";
        ir.native.namespace_mut(native_format).arenas_mut().insert("records".into(), vec![crate::NativeRecord::new(
            id.try_into().unwrap(), serde_json::Map::from_iter([("native_only".into(), serde_json::Value::Bool(true))]),
        ).unwrap()]);
        let raw = [UnknownRecord::retained(id.try_into().unwrap(), 0, Vec::new(), Vec::new())];
        let mut builder = crate::AnnotationBuilder::new();
        builder.derived(&cadmpeg_test_support::service_decode_context(), id, "native_only").unwrap();
        let annotations = builder.build();
        let actual = admit_with_native_unknowns(&ctx, &ir, ("rhino", &raw), Some(&annotations), RHINO_DRAFT_CHECKS, Vec::new()).unwrap().unwrap();
        let mut projected = ir.clone();
        projected.set_native_unknowns_from(&ctx, "rhino", raw.iter().map(|record| NativeUnknownRecord::try_from(record).unwrap())).unwrap();
        let expected = admit_with_annotations(&ctx, &projected, &annotations, RHINO_DRAFT_CHECKS, Vec::new()).unwrap();
        assert_eq!(serde_json::to_value(actual).unwrap(), serde_json::to_value(expected).unwrap(), "{native_format}");
    }
}

#[test]
fn native_unknown_admission_preserves_sorted_source_finding_order() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let ir = crate::CadIr::empty();
    let raw = (0..12).map(|position| UnknownRecord::retained(
        format!("test:source:unknown#{position}").as_str().try_into().unwrap(), 0, Vec::new(),
        vec![format!("test:model:missing#{position}")],
    )).collect::<Vec<_>>();
    let actual = admit_with_native_unknowns(&ctx, &ir, ("rhino", &raw), None, &[crate::report::check::Check::NativeLinks, crate::report::check::Check::ArenaOrder], Vec::new()).unwrap().unwrap();
    let mut projected = ir.clone();
    projected.set_native_unknowns_from(&ctx, "rhino", raw.iter().map(|record| NativeUnknownRecord::try_from(record).unwrap())).unwrap();
    let expected = super::admit(&ctx, &projected, &[crate::report::check::Check::NativeLinks, crate::report::check::Check::ArenaOrder], Vec::new()).unwrap();
    assert_eq!(serde_json::to_value(&actual).unwrap(), serde_json::to_value(expected).unwrap());
    assert_eq!(actual.findings[2].entity.as_deref(), Some("test:source:unknown#10"));
    assert_eq!(raw[2].id().as_str(), "test:source:unknown#2");
}

#[test]
fn native_unknown_admission_preserves_duplicate_source_product_failure() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let ir = crate::CadIr::empty();
    let raw = [7_u8, 19].map(|offset| UnknownRecord::retained("test:source:unknown#duplicate".try_into().unwrap(), u64::from(offset), vec![offset], Vec::new()));
    let actual = admit_with_native_unknowns(&ctx, &ir, ("rhino", &raw), None, RHINO_DRAFT_CHECKS, Vec::new()).unwrap().unwrap_err();
    let mut projected = ir.clone();
    let expected = projected.set_native_unknowns_from(&ctx, "rhino", raw.iter().map(|record| NativeUnknownRecord::try_from(record).unwrap())).unwrap_err();
    assert_eq!(actual.to_string(), expected.to_string());
    assert_eq!(actual.to_string(), "native collection is invalid: duplicate native unknown record test:source:unknown#duplicate");
    assert!(ir.native.0.is_empty());
}

#[test]
fn source_product_validation_borrows_large_images_and_releases_order_storage() {
    let records = [UnknownRecord::retained(
        format!("test:source:unknown#{}", "x".repeat(16384)).as_str().try_into().unwrap(),
        23, vec![31; 65536], vec!["test:model:point#target".into()],
    )];
    let pointer = records[0].data().unwrap().as_ptr();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::validate_native_unknowns(&ctx, &records).unwrap().unwrap();
    assert_eq!(records[0].data().unwrap().as_ptr(), pointer);
    let released = ctx.reserve_scoped(1024, "source order released").unwrap();
    drop(released);
    ctx.finish_session().unwrap();
}

#[test]
fn source_product_validation_preserves_link_duplicate_and_resource_failures() {
    let invalid = [UnknownRecord::retained("test:source:unknown#one".try_into().unwrap(), 0, vec![7], vec!["invalid".into()])];
    let ctx = cadmpeg_test_support::service_decode_context();
    let actual = super::validate_native_unknowns(&ctx, &invalid).unwrap().unwrap_err();
    assert_eq!(actual.to_string(), NativeUnknownRecord::try_from(&invalid[0]).unwrap_err().to_string());
    let duplicates = [0, 1].map(|offset| UnknownRecord::retained("test:source:unknown#same".try_into().unwrap(), offset, vec![7], Vec::new()));
    assert_eq!(super::validate_native_unknowns(&ctx, &duplicates).unwrap().unwrap_err().to_string(), "native collection is invalid: duplicate native unknown record test:source:unknown#same");
    for dimension in [ResourceDimension::CollectionItems, ResourceDimension::MaterializedBytes, ResourceDimension::WorkUnits] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(limit)) = super::validate_native_unknowns(&ctx, &duplicates) else { panic!("source validation resource refusal stays outer"); };
        assert_eq!(limit.dimension, dimension);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    }
}
