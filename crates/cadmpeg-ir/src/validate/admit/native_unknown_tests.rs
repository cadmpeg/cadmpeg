// SPDX-License-Identifier: Apache-2.0

use super::{admit_with_annotations, admit_with_native_unknowns, RHINO_DRAFT};
use crate::unknown::{NativeUnknownRecord, UnknownRecord};

/// The native-link rules and the arena order rule.
const NATIVE_LINKS_AND_ORDER: &super::Admission =
    &super::Admission::new(NATIVE_LINKS_AND_ORDER_RULES, false);

const NATIVE_LINKS_AND_ORDER_RULES: &[crate::report::check::RuleName] = &[
    crate::rule_name!("annotations_native.helix-axis-native-ref-does-not"),
    crate::rule_name!("annotations_native.native-record-link-does-not-resolve"),
    crate::rule_name!("annotations_native.native-record-link-must-identity-string"),
    crate::rule_name!("annotations_native.native-record-links-must-array-identities"),
    crate::rule_name!("annotations_native.native-ref-does-not-resolve"),
    crate::rule_name!("annotations_native.operand-native-ref-does-not-resolve"),
    crate::rule_name!("annotations_native.pmi-native-ref-does-not-resolve"),
    crate::rule_name!("identity_order.arena-not-strictly-sorted-id"),
];

#[test]
fn native_unknown_admission_matches_product_projection_without_mutation() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut ir = crate::examples::unit_cube().unwrap();
    let old_id = "test:source:unknown#old";
    let new_id = "test:source:unknown#new";
    ir.set_native_unknowns(
        &ctx,
        "rhino",
        &[NativeUnknownRecord {
            id: old_id.try_into().unwrap(),
            links: vec!["test:model:missing#old".try_into().unwrap()],
        }],
    )
    .unwrap();
    for (format, arena) in [("alpha", "records"), ("rhino", "zz"), ("zeta", "records")] {
        let id = format!("test:native:record#{format}");
        ir.native
            .namespace_mut(crate::design::names::CodecId::new(format).expect("a codec id"))
            .arenas_mut()
            .insert(
                cadmpeg_core::text::NonBlankString::new(arena).expect("an arena name"),
                vec![crate::NativeRecord::new(
                    id.as_str().try_into().unwrap(),
                    serde_json::Map::new(),
                )
                .unwrap()],
            );
    }
    let raw = vec![UnknownRecord::retained(
        new_id.try_into().unwrap(),
        72,
        vec![23; 16],
        vec![ir.geometry.points[0].id.as_str().to_owned()],
    )];
    let pointer = raw[0].data().unwrap().as_ptr();
    let mut builder = crate::AnnotationBuilder::new();
    let stream = crate::annotations::StreamHandle::new(crate::stream_name!("test:source"));
    builder.note(new_id, &stream, 72, None);
    for path in ["id", "links", "links.0", "links.1", "offset", "retention"] {
        builder.derived(new_id, path).unwrap();
    }
    let annotations = builder.build();
    let before = cadmpeg_core::serde_erase::json::to_value(&ir).unwrap();
    let actual = admit_with_native_unknowns(
        &ctx,
        &ir,
        ("rhino", &raw),
        Some(&annotations),
        RHINO_DRAFT,
        Vec::new(),
    )
    .unwrap()
    .unwrap();
    let mut projected = ir.clone();
    projected
        .set_native_unknowns_from(
            &ctx,
            "rhino",
            raw.iter()
                .map(|record| NativeUnknownRecord::try_from(record).unwrap()),
        )
        .unwrap();
    let expected =
        admit_with_annotations(&ctx, &projected, &annotations, RHINO_DRAFT, Vec::new()).unwrap();
    assert_eq!(
        cadmpeg_core::serde_erase::json::to_value(&actual).unwrap(),
        cadmpeg_core::serde_erase::json::to_value(&expected).unwrap()
    );
    assert_eq!(
        cadmpeg_core::serde_erase::json::to_value(&ir).unwrap(),
        before
    );
    assert_eq!(raw[0].data().unwrap().as_ptr(), pointer);
    assert_eq!(
        actual
            .findings
            .iter()
            .filter(|finding| finding.check == crate::report::check::Check::Annotations)
            .count(),
        3
    );
    assert!(!actual
        .findings
        .iter()
        .any(|finding| finding.message.contains("missing#old")));
}

#[test]
fn native_unknown_admission_keeps_unresolved_outgoing_link_findings() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let ir = crate::CadIr::empty();
    let raw = [UnknownRecord::retained(
        "test:source:unknown#0".try_into().unwrap(),
        0,
        Vec::new(),
        vec!["test:model:missing#0".into()],
    )];
    let actual =
        admit_with_native_unknowns(&ctx, &ir, ("rhino", &raw), None, RHINO_DRAFT, Vec::new())
            .unwrap()
            .unwrap();
    let links = actual
        .findings
        .iter()
        .filter(|finding| finding.check == crate::report::check::Check::NativeLinks)
        .collect::<Vec<_>>();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].entity.as_deref(), Some("test:source:unknown#0"));
    assert_eq!(
        links[0].message,
        "native-record link `test:model:missing#0` does not resolve"
    );
}

#[test]
fn native_unknown_admission_preserves_outer_depth_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let ir = crate::CadIr::empty();
    let raw = [UnknownRecord::retained(
        "test:source:unknown#0".try_into().unwrap(),
        0,
        vec![42; 16],
        Vec::new(),
    )];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) =
        admit_with_native_unknowns(&ctx, &ir, ("rhino", &raw), None, RHINO_DRAFT, Vec::new())
    else {
        panic!("resource refusal remains outside semantic result");
    };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
    assert!(ir.native.0.is_empty());
    assert_eq!(raw[0].data().unwrap(), &[42; 16]);
}

#[test]
fn native_unknown_admission_preserves_product_link_grammar_failures() {
    let ir = crate::CadIr::empty();
    for link in [
        "",
        "missing",
        "test:model:body#",
        "test:model:body#with space",
    ] {
        let ctx = cadmpeg_core::decode::test_support::service_decode_context();
        let raw = [UnknownRecord::retained(
            "test:source:unknown#0".try_into().unwrap(),
            0,
            vec![9],
            vec![link.into()],
        )];
        let expected = NativeUnknownRecord::try_from(&raw[0]).unwrap_err();
        let error =
            admit_with_native_unknowns(&ctx, &ir, ("rhino", &raw), None, RHINO_DRAFT, Vec::new())
                .unwrap()
                .unwrap_err();
        assert_eq!(error.to_string(), expected.to_string());
        ctx.finish_session().unwrap();
        assert!(ir.native.0.is_empty());
    }
}

#[test]
fn native_unknown_annotation_collision_preserves_first_native_arena() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    for native_format in ["alpha", "zeta"] {
        let mut ir = crate::CadIr::empty();
        let id = "test:source:unknown#collision";
        ir.native
            .namespace_mut(crate::design::names::CodecId::new(native_format).expect("a codec id"))
            .arenas_mut()
            .insert(
                cadmpeg_core::nonblank_literal!("records"),
                vec![crate::NativeRecord::new(
                    id.try_into().unwrap(),
                    serde_json::Map::from_iter([(
                        "native_only".into(),
                        serde_json::Value::Bool(true),
                    )]),
                )
                .unwrap()],
            );
        let raw = [UnknownRecord::retained(
            id.try_into().unwrap(),
            0,
            Vec::new(),
            Vec::new(),
        )];
        let mut builder = crate::AnnotationBuilder::new();
        builder.derived(id, "native_only").unwrap();
        let annotations = builder.build();
        let actual = admit_with_native_unknowns(
            &ctx,
            &ir,
            ("rhino", &raw),
            Some(&annotations),
            RHINO_DRAFT,
            Vec::new(),
        )
        .unwrap()
        .unwrap();
        let mut projected = ir.clone();
        projected
            .set_native_unknowns_from(
                &ctx,
                "rhino",
                raw.iter()
                    .map(|record| NativeUnknownRecord::try_from(record).unwrap()),
            )
            .unwrap();
        let expected =
            admit_with_annotations(&ctx, &projected, &annotations, RHINO_DRAFT, Vec::new())
                .unwrap();
        assert_eq!(
            cadmpeg_core::serde_erase::json::to_value(actual).unwrap(),
            cadmpeg_core::serde_erase::json::to_value(expected).unwrap(),
            "{native_format}"
        );
    }
}

#[test]
fn native_unknown_admission_preserves_sorted_source_finding_order() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let ir = crate::CadIr::empty();
    let raw = (0..12)
        .map(|position| {
            UnknownRecord::retained(
                format!("test:source:unknown#{position}")
                    .as_str()
                    .try_into()
                    .unwrap(),
                0,
                Vec::new(),
                vec![format!("test:model:missing#{position}")],
            )
        })
        .collect::<Vec<_>>();
    let actual = admit_with_native_unknowns(
        &ctx,
        &ir,
        ("rhino", &raw),
        None,
        NATIVE_LINKS_AND_ORDER,
        Vec::new(),
    )
    .unwrap()
    .unwrap();
    let mut projected = ir.clone();
    projected
        .set_native_unknowns_from(
            &ctx,
            "rhino",
            raw.iter()
                .map(|record| NativeUnknownRecord::try_from(record).unwrap()),
        )
        .unwrap();
    let expected = super::admit(&ctx, &projected, NATIVE_LINKS_AND_ORDER, Vec::new()).unwrap();
    assert_eq!(
        cadmpeg_core::serde_erase::json::to_value(&actual).unwrap(),
        cadmpeg_core::serde_erase::json::to_value(expected).unwrap()
    );
    assert_eq!(
        actual.findings[2].entity.as_deref(),
        Some("test:source:unknown#10")
    );
    assert_eq!(raw[2].id().as_str(), "test:source:unknown#2");
}

#[test]
fn native_unknown_admission_preserves_duplicate_source_product_failure() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let ir = crate::CadIr::empty();
    let raw = [7_u8, 19].map(|offset| {
        UnknownRecord::retained(
            "test:source:unknown#duplicate".try_into().unwrap(),
            u64::from(offset),
            vec![offset],
            Vec::new(),
        )
    });
    let actual =
        admit_with_native_unknowns(&ctx, &ir, ("rhino", &raw), None, RHINO_DRAFT, Vec::new())
            .unwrap()
            .unwrap_err();
    let mut projected = ir.clone();
    let expected = projected
        .set_native_unknowns_from(
            &ctx,
            "rhino",
            raw.iter()
                .map(|record| NativeUnknownRecord::try_from(record).unwrap()),
        )
        .unwrap_err();
    assert_eq!(actual.to_string(), expected.to_string());
    assert_eq!(actual.to_string(), "native collection is invalid: duplicate native unknown record test:source:unknown#duplicate");
    assert!(ir.native.0.is_empty());
}
