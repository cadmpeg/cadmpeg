//! Tests for the `names` module.

use super::super::CLASS_MARKER;
use super::object_names;
use crate::records::operand_tag::NativeOperandTag;

#[test]
fn object_names_follow_the_lane_name_class_token() {
    let mut payload = vec![0x42, 0, 0, 0, 0x13, 0];
    payload.extend_from_slice(CLASS_MARKER);
    payload.extend_from_slice(&18u16.to_le_bytes());
    payload.extend_from_slice(b"moFavoriteFolder_c");
    payload.extend_from_slice(&[0x87, 0x80, 0xff, 0xfe, 0xff]);
    payload.push(9);
    for unit in "Favorites".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.resize(payload.len() + 12, 0);
    payload.extend_from_slice(&[0x87, 0x80, 0xff, 0xfe, 0xff]);
    payload.push(4);
    for unit in "Boss".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.resize(payload.len() + 12, 0);

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&payload, &arena, &policy)
        .expect("test context");
    let names = object_names(&ctx, &payload, "lane").unwrap();
    assert_eq!(
        names
            .iter()
            .map(|name| name.value.as_str())
            .collect::<Vec<_>>(),
        ["Favorites", "Boss"]
    );
}

#[test]
fn operand_kind_names_preserve_wire_spelling() {
    use crate::records::FeatureInputOperandKind;
    for (kind, expected) in [
        (FeatureInputOperandKind::D6, "d6"),
        (FeatureInputOperandKind::E1, "e1"),
        (
            FeatureInputOperandKind::Native(NativeOperandTag::TAG_80D5),
            "d580",
        ),
    ] {
        assert_eq!(
            super::operand_kind_name(&cadmpeg_test_support::service_decode_context(), kind)
                .unwrap()
                .as_str(),
            expected
        );
    }
}

#[test]
fn marker_literals_keep_their_wire_spelling() {
    use cadmpeg_core::nonblank_literal;
    assert_eq!(
        nonblank_literal!("sldprt:marker-local-id").as_str(),
        "sldprt:marker-local-id"
    );
    assert_eq!(
        nonblank_literal!(
            &cadmpeg_test_support::service_decode_context(),
            "sldprt:marker-relation:{}",
            34
        )
        .unwrap()
        .as_str(),
        "sldprt:marker-relation:34"
    );
    assert_eq!(
        nonblank_literal!(
            &cadmpeg_test_support::service_decode_context(),
            "sldprt:marker-geometry:{}",
            2
        )
        .unwrap()
        .as_str(),
        "sldprt:marker-geometry:2"
    );
}

#[test]
fn object_names_utf16_refuses_exact_retained_budget() {
    let mut payload = super::super::NAME_MARKER.to_vec();
    payload.extend_from_slice(&[1, 0, 8]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 2;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    assert!(
        matches!(object_names(&ctx, &payload, "lane"), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes && limit.additional == 3 && limit.operation == "retain SLDPRT feature input name")
    );
}

#[test]
fn object_names_scans_refuse_work_budget() {
    let mut payload = super::super::NAME_MARKER.to_vec();
    payload.extend_from_slice(&[1, 0, 8]);
    for operation in [
        "find SLDPRT feature input name class",
        "scan SLDPRT feature input names",
    ] {
        let error = crate::test_support::work_refusal_at(operation, |ctx| {
            object_names(ctx, &payload, "lane")
        });
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
    }
}

#[test]
fn class_declaration_match_propagates_lane_key_work_refusal() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    assert!(matches!(
        super::class_declarations_match(&ctx, &[], "lane#key", &[]),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == "split SLDPRT feature-input lane key"
    ));
}

#[test]
fn object_name_structure_propagates_lane_key_work_refusal() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    assert!(matches!(
        super::object_names_structure_match(&ctx, &[], "lane#key", &[]),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == "split SLDPRT feature-input lane key"
    ));
}

#[test]
fn configuration_searches_propagate_work_refusal() {
    let section = "Config-name/path";
    for operation in [
        "find SLDPRT configuration prefix",
        "find SLDPRT configuration suffix",
        "find SLDPRT configuration path separator",
    ] {
        crate::test_support::work_refusal_at(operation, |ctx| super::configuration(ctx, section));
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        super::configuration(&ctx, section).unwrap().as_deref(),
        Some("name")
    );
    assert_eq!(
        super::configuration(&ctx, "Config-name-ResolvedFeatures")
            .unwrap()
            .as_deref(),
        Some("name"),
    );
    assert_eq!(super::configuration(&ctx, "Other").unwrap(), None);
}

#[test]
fn native_identity_decimal_parse_propagates_work_refusal() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    assert!(matches!(
        super::decimal_matches(&ctx, "12", 12),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
    ));
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(super::decimal_matches(&ctx, "12", 12).unwrap());
    assert!(!super::decimal_matches(&ctx, "012", 12).unwrap());
}

#[test]
fn class_mismatch_does_not_scan_the_remaining_payload() {
    let mut payload = super::super::CLASS_MARKER.to_vec();
    payload.extend_from_slice(&8u16.to_le_bytes());
    payload.extend_from_slice(b"sgSketch");
    payload.resize(16384, 0);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 4096;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let actual = crate::records::FeatureInputClass {
        id: "invalid".into(), parent: "lane".into(), ordinal: 0, offset: 0, name: "sgSketch".into(),
    };
    assert!(!super::class_declarations_match(&ctx, &payload, "lane", &[actual]).unwrap());
}
