// SPDX-License-Identifier: Apache-2.0
//! Schema identifier grammar table.
//!
//! The classifier produces an admission form or a resource refusal. This table
//! states one form for each grammar rule. The `FILE_SCHEMA` diagnostic surface
//! that reads the recoverable form is pinned in `parse/tests/envelope.rs`.

use super::{split_schema_identifier, AdmittedSchemaIdentifier};

/// The admitted form of one identifier, as one comparable value.
#[derive(Debug, PartialEq, Eq)]
enum Admitted {
    /// The classifier rejects the identifier.
    Rejected,
    /// The schema name and the optional object identifier are both valid.
    Valid,
    /// The object identifier holds an out-of-range component. The fields are
    /// the schema name and the first out-of-range component in source order.
    OutOfRange(String, String),
}

fn admitted(identifier: &str) -> Admitted {
    let ctx = cadmpeg_test_support::service_decode_context();
    let Some(admitted) = AdmittedSchemaIdentifier::admit(&ctx, identifier.to_owned()).expect("schema text resources admitted") else {
        return Admitted::Rejected;
    };
    assert_eq!(
        admitted.text(),
        identifier,
        "the admission keeps the source text"
    );
    match admitted.out_of_range() {
        None => Admitted::Valid,
        Some((name, component)) => Admitted::OutOfRange(name.to_owned(), component.to_owned()),
    }
}

#[test]
fn classifier_reports_the_first_out_of_range_component_in_source_order() {
    // The root component states a root number as `0`, `1`, or `2`, or as a
    // registered ASN.1 identifier for one of those numbers. Every other root
    // component is out of range. Under root `0` or `1` the second number is in
    // `0..=39`. Every other position permits every non-negative number, and a
    // negative number is out of range in every position. The report names the
    // whole component text, not the number inside it.
    let cases = [
        // A minus sign is out of range in the root position.
        ("AP242 { -1 40 }", "-1"),
        // Root `1` bounds the second number, so `40` is out of range and the
        // root itself is in range.
        ("AP242 { 1 40 }", "40"),
        // Root `0` bounds the second number the same way.
        ("AP242 { 0 40 }", "40"),
        // `iso` is the registered identifier for root `1`, so it bounds the
        // second number as the number `1` does.
        ("AP242 { iso 40 }", "40"),
        // A component with an identifier and parentheses states the number
        // inside the parentheses, and the second-component bound applies to it.
        ("AP242 { 1 part(214) }", "part(214)"),
        // `3` is not a root number, so the root is out of range and no bound
        // reaches the second component.
        ("AP242 { 3 1 }", "3"),
        // An unregistered identifier states no root number, so the root is out
        // of range for the same reason that `3` is.
        ("AP242 { foo 40 }", "foo"),
        // The registered identifiers compare exactly, so a case difference
        // gives another identifier, which states no root number.
        ("AP242 { iSo 40 }", "iSo"),
        // An identifier and parentheses that hold an identifier state no
        // number, so this root is out of range even though it opens with a
        // registered identifier.
        ("AP242 { iso(standard) 40 }", "iso(standard)"),
    ];
    for (identifier, component) in cases {
        assert_eq!(
            admitted(identifier),
            Admitted::OutOfRange("AP242".to_owned(), component.to_owned()),
            "{identifier}"
        );
    }
}

#[test]
fn classifier_admits_in_range_object_identifiers() {
    let cases = [
        // Root `1` permits a second number in `0..=39`.
        "AP242 { 1 39 }",
        // Root `2` bounds no second number.
        "AP242 { 2 40 }",
        // Each registered identifier states its root number.
        "AP242 { iso 39 }",
        "AP242 { itu-t 39 }",
        "AP242 { ccitt 39 }",
        "AP242 { joint-iso-itu-t 40 }",
        "AP242 { joint-iso-ccitt 40 }",
        // A registered root bounds a numeric second component only, so an
        // unnumbered second component states no number to place out of range.
        "AP242 { iso standard 10303 part(214) version(1) }",
        // An identifier and parentheses that hold a number state that number,
        // so this root is root `1`.
        "AP242 { iso(1) 39 }",
        // A schema name alone is a complete identifier.
        "AP242",
    ];
    for identifier in cases {
        assert_eq!(admitted(identifier), Admitted::Valid, "{identifier}");
    }
}

#[test]
fn admitted_identifiers_keep_numeric_components_with_named_roots() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let admitted = AdmittedSchemaIdentifier::admit(&ctx, "AP242 { iso 0 10303 442 4 1 4 }".to_owned()).expect("schema text resources admitted")
        .expect("named ISO root is admitted");
    crate::test_support::with_service_context(admitted.text().as_bytes(), |_, ctx| {
        assert_eq!(
            admitted
                .numeric_object_identifier(ctx)
                .expect("numeric components fit local storage")
                .as_deref(),
            Some([1, 0, 10303, 442, 4, 1, 4].as_slice())
        );
    });
}

#[test]
fn classifier_rejects_identifiers_that_do_not_parse() {
    let cases = [
        // A negative number has no leading zero either, so `-01` is not a
        // component number and the component has no form.
        "AP242 { 1 0 10303 442 -01 1 4 }",
        // An ASN.1 identifier uses letters, digits, and hyphens only.
        "AP242 { 1 0 10303 442 invalid_ 1 4 }",
        // An ASN.1 identifier starts with a lowercase letter, so an uppercase
        // spelling of a registered identifier has no component form. The
        // identifier does not parse, so the range rule does not apply to it.
        "AP242 { ISO 40 }",
        "AP242 { Iso 40 }",
        // A numeric component has no leading zero.
        "AP242 { 01 0 }",
        // An object identifier has at least two components.
        "AP242 { 1 }",
        // An identifier that opens a brace closes it at the end.
        "AP242 { 1 40",
        // A schema name is an EXPRESS simple_id: a letter, then letters,
        // digits, or underscores.
        "9AP242",
        "_AP242",
        "",
    ];
    for identifier in cases {
        assert_eq!(admitted(identifier), Admitted::Rejected, "{identifier}");
    }
}

#[test]
fn split_separates_the_schema_name_from_the_object_identifier() {
    // The split ignores whitespace around the identifier and around the schema
    // name. An identifier with no brace is a schema name alone. An identifier
    // that opens an object identifier and does not close it at the end of the
    // identifier has no schema name and no object identifier. The split reads
    // the first brace, so a brace inside the object identifier text stays in
    // that text, where no component form accepts it.
    let cases = [
        ("AP242", Some(("AP242", None))),
        ("  AP242  ", Some(("AP242", None))),
        ("AP242 { 1 40 }", Some(("AP242", Some(" 1 40 ")))),
        ("  AP242{1 40}  ", Some(("AP242", Some("1 40")))),
        ("AP242 {}", Some(("AP242", Some("")))),
        ("{ 1 40 }", Some(("", Some(" 1 40 ")))),
        ("A{B{1 2}", Some(("A", Some("B{1 2")))),
        ("AP242 { 1 40", None),
        ("A{1 2", None),
        ("AP242 { 1 40 } x", None),
    ];
    for (identifier, expected) in cases {
        assert_eq!(
            split_schema_identifier(&cadmpeg_test_support::service_decode_context(), identifier).unwrap(),
            expected,
            "{identifier}"
        );
    }
}

#[test]
fn admitted_schema_identifiers_expose_only_proved_text_and_diagnostics() {
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(AdmittedSchemaIdentifier::admit(&ctx, "!".to_owned()).expect("schema text resources admitted").is_none());
    let identifier =
        AdmittedSchemaIdentifier::admit(&ctx, "AP242 { 1 40 }".to_owned()).expect("schema text resources admitted").expect("recoverable OID");
    assert_eq!(identifier.text(), "AP242 { 1 40 }");
    assert_eq!(identifier.out_of_range(), Some(("AP242", "40")));
    let valid = AdmittedSchemaIdentifier::admit(&ctx, "AP242 { 1 39 }".to_owned()).expect("schema text resources admitted").expect("valid OID");
    assert_eq!(valid.out_of_range(), None);
}

#[test]
fn schema_identifier_copy_refusal_stays_error() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let input = "AP242 { 1 40 }";
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let result = crate::test_support::with_policy_context(input.as_bytes(), &policy, |_, ctx| {
        AdmittedSchemaIdentifier::admit(ctx, input.to_owned())
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "STEP admit text copy"));
}

#[test]
fn schema_identifier_iteration_refusal_stays_error() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let input = "AP242 { iso 39 }";
    let mut policy = DecodePolicy::service();
    // Admit the input-byte trim before refusing the character traversal.
    policy.limits.max_work_units = cadmpeg_core::decode::u64_from_index(input.len());
    let result = crate::test_support::with_policy_context(input.as_bytes(), &policy, |_, ctx| {
        AdmittedSchemaIdentifier::admit(ctx, input.to_owned())
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "STEP schema identifier characters"));
}

#[test]
fn schema_oid_words_preserve_unicode_whitespace_and_byte_slices() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for value in ["", "iso 39", "\tiso\n39\r", "\u{2003}iso\u{a0}39\u{85}", "iso\0 39", "iso 39 é"] {
        let expected: Vec<_> = value.split_whitespace().collect();
        let actual: Vec<_> = super::schema_oid_components(&ctx, value)
            .expect("word traversal fits service policy")
            .collect::<Result<Vec<_>, _>>()
            .expect("byte offsets fit the input");
        assert_eq!(actual, expected, "{value:?}");
    }
}

#[test]
fn schema_identifier_trim_preserves_refusal() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let error = split_schema_identifier(&ctx, " AP242 ").unwrap_err();
    let cadmpeg_core::CodecError::ResourceLimit(refusal) = error else { panic!("schema trim must preserve the refusal"); };
    assert_eq!(refusal.operation, "STEP schema identifier trim");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}

#[test]
fn schema_identifier_trailing_name_trim_preserves_refusal() {
    cadmpeg_test_support::refusal::resource_limit_at(cadmpeg_core::decode::ResourceDimension::WorkUnits, "STEP schema identifier name trailing trim", |cap| {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        let result = split_schema_identifier(&ctx, " AP242 { 1 2 } ").map(|_| ());
        if let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) = &result { assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal)); }
        result
    });
}

#[test]
fn schema_identifier_admission_trim_preserves_refusal() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let error = AdmittedSchemaIdentifier::admit(&ctx, " AP242 ".to_owned()).unwrap_err();
    let cadmpeg_core::CodecError::ResourceLimit(refusal) = error else { panic!("schema admission must preserve trim refusal"); };
    assert_eq!(refusal.operation, "STEP schema identifier trim");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}

#[test]
fn schema_identifier_brace_split_preserves_refusal() {
    cadmpeg_test_support::refusal::resource_limit_at(cadmpeg_core::decode::ResourceDimension::WorkUnits, "STEP schema identifier brace split", |cap| {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        let result = split_schema_identifier(&ctx, " AP242 { 1 2 } ").map(|_| ());
        if let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) = &result { assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal)); }
        result
    });
}

#[test]
fn schema_object_identifier_number_split_preserves_refusal() {
    cadmpeg_test_support::refusal::resource_limit_at(cadmpeg_core::decode::ResourceDimension::WorkUnits, "STEP schema object identifier number split", |cap| {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        let result = super::schema_oid_component_form(&ctx, "iso(1)").map(|_| ());
        if let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) = &result { assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal)); }
        result
    });
}
