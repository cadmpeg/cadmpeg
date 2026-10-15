use super::super::{parse_relation_signature, parse_schema_program, CATALOG_OPEN};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

#[test]
fn missing_schema_prefix_rejects_before_suffix_scan_and_copy() {
    let mut bytes = CATALOG_OPEN.to_vec();
    bytes.extend([0xfe, 0xfe, 0xfd]);
    bytes.extend([0x81; 4096]);
    let run = |ctx: &DecodeContext<'_>| parse_schema_program(ctx, &bytes, 0, None);
    assert!(crate::test_support::with_work_limit(0, run)
        .expect("fixed prefix rejection visits no suffix")
        .is_none());
    assert!(crate::test_support::with_collection_limit(0, run)
        .expect("fixed prefix rejection copies no suffix")
        .is_none());
    bytes[CATALOG_OPEN.len() + 2] = 0xfe;
    crate::test_support::with_work_limit(0, |ctx| {
        let CodecError::ResourceLimit(limit) = parse_schema_program(ctx, &bytes, 0, None)
            .expect_err("present prefix requires the actual suffix scan")
        else {
            panic!("resource refusal");
        };
        assert_eq!(limit.operation, "catia_legacy_schema_scan");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn missing_relation_prefix_rejects_before_reverse_result_search() {
    let source = format!("{}) : Real\n", "x".repeat(4096));
    let run = |ctx: &DecodeContext<'_>| parse_relation_signature(ctx, &source);
    assert!(crate::test_support::with_work_limit(0, run)
        .expect("fixed opening parenthesis rejection visits no tail")
        .is_none());
    assert!(crate::test_support::with_collection_limit(0, run)
        .expect("fixed opening parenthesis rejection allocates no candidate")
        .is_none());
    crate::test_support::with_work_limit(0, |ctx| {
        let CodecError::ResourceLimit(limit) = parse_relation_signature(ctx, "() : Real\n")
            .expect_err("present prefix requires the actual result search")
        else {
            panic!("resource refusal");
        };
        assert_eq!(limit.operation, "catia_legacy_relation_result_split");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn schema_footer_stops_one_forward_scan_before_unread_tail() {
    let mut bytes = CATALOG_OPEN.to_vec();
    bytes.extend_from_slice(super::super::SCHEMA_PROGRAM_PREFIX);
    let offset = bytes.len();
    bytes.push(0xfe);
    let footer_offset = bytes.len();
    bytes.extend_from_slice(super::super::SCHEMA_PROGRAM_FOOTER);
    bytes.extend([0x81; 4096]);
    bytes.extend_from_slice(CATALOG_OPEN);
    let run = |ctx: &DecodeContext<'_>| parse_schema_program(ctx, &bytes, 0, None);
    let program = crate::test_support::with_work_limit(4, run)
        .expect("two boundary positions, one byte copy, one identifier position")
        .expect("first complete footer closes the program");
    assert_eq!(program.offset, offset);
    assert_eq!(program.boundary_offset, footer_offset);
    assert_eq!(
        program.boundary,
        super::super::LegacySchemaProgramBoundary::VendorFooter
    );
    assert_eq!(program.bytes, [0xfe]);
    assert!(program.identifiers.is_empty());
    crate::test_support::with_work_limit(1, |ctx| {
        let CodecError::ResourceLimit(limit) =
            run(ctx).expect_err("footer position needs admission")
        else {
            panic!("resource refusal");
        };
        assert_eq!(limit.operation, "catia_legacy_schema_scan");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
    crate::test_support::with_work_limit(0, |ctx| {
        let CodecError::ResourceLimit(original) = ctx
            .charge_work(1, "before complete schema footer")
            .expect_err("zero work fuse")
        else {
            panic!("resource refusal");
        };
        let CodecError::ResourceLimit(actual) = run(ctx).expect_err("sticky schema refusal") else {
            panic!("resource refusal");
        };
        assert_eq!(actual, original);
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn schema_identifier_candidates_admit_utf8_and_character_scans() {
    let mut bytes = CATALOG_OPEN.to_vec();
    bytes.extend_from_slice(super::super::SCHEMA_PROGRAM_PREFIX);
    bytes.extend([b'A'; 4096]);
    bytes.push(0xfe);
    bytes.extend_from_slice(super::super::SCHEMA_PROGRAM_FOOTER);
    for operation in [
        "catia_legacy_identifier_utf8",
        "catia_legacy_identifier_characters",
    ] {
        let error = crate::test_support::with_work_refusal(operation, |ctx| {
            parse_schema_program(ctx, &bytes, 0, None)
        })
        .expect_err("rejected identifier candidates must admit child scans");
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == operation));
    }
    let program =
        crate::test_support::with_service_context(|ctx| parse_schema_program(ctx, &bytes, 0, None))
            .expect("bounded repeated candidates")
            .expect("footer closes schema");
    assert_eq!(program.bytes.len(), 4097);
    assert_eq!(program.identifiers.len(), 1);
    assert_eq!(program.identifiers[0].value, "A".repeat(64));
}

#[test]
fn malformed_relation_clause_stops_separator_scan_before_unread_tail() {
    let source = format!("(bad,{}) : Real", "x".repeat(4096));
    // Result search: 4108+4; result trim: 4; nonempty test: 1;
    // separator visits: 4; missing-colon search: 3+1. Total: 4125.
    let result =
        crate::test_support::with_work_limit(4125, |ctx| parse_relation_signature(ctx, &source))
            .expect("only the first clause is parsed");
    assert!(result.is_none());
    let error =
        crate::test_support::with_work_refusal("catia_legacy_relation_clause_scan", |ctx| {
            parse_relation_signature(ctx, &source)
        })
        .expect_err("first clause requires separator admission");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "catia_legacy_relation_clause_scan"));
}

#[test]
fn legacy_identifier_validation_stops_at_the_first_invalid_character() {
    let source = format!("!{}", "A".repeat(4096));
    assert!(!crate::test_support::with_work_limit(1, |ctx| {
        super::super::valid_identifier_charged(ctx, &source, "test identifier scan")
    })
    .expect("only the invalid first character is visited"));
    crate::test_support::with_service_context(|ctx| {
        for value in ["", "_", "Äbc_2", "2abc", "A-b", "_λ9"] {
            assert_eq!(
                super::super::valid_identifier_charged(ctx, value, "test identifier scan")?,
                super::super::valid_identifier(value)
            );
        }
        Ok::<_, CodecError>(())
    })
    .expect("identifier rules are preserved");
}

#[test]
fn legacy_text_validation_admits_utf8_and_preserves_control_rules() {
    for operation in ["catia_legacy_text_utf8", "catia_legacy_text_characters"] {
        let error = crate::test_support::with_work_refusal(operation, |ctx| {
            super::super::text_value_allow_empty(ctx, "Äbc\t\n\r".as_bytes())
        })
        .expect_err("text validation must admit its child scans");
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == operation));
    }
    crate::test_support::with_service_context(|ctx| {
        for (bytes, expected) in [
            (&b""[..], Some("")),
            (&b"abc\t\n\r"[..], Some("abc\t\n\r")),
            (&b"abc\x01"[..], None),
            (&b"\xff"[..], None),
        ] {
            assert_eq!(super::super::text_value_allow_empty(ctx, bytes)?, expected);
        }
        Ok::<_, CodecError>(())
    })
    .expect("text rules are preserved");
}
