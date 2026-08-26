#[test]
fn om_numeric_expression_types_only_canonical_parameter_names() {
    for name in ["p12foo", "p12_", "p4294967296_radius"] {
        let text = format!("(Number [mm]) {name}: 5; ");
        let mut bytes = b"hostglobalvariables".to_vec();
        bytes.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0);

        let expressions = super::super::numeric_expressions(&bytes);
        assert_eq!(expressions.len(), 1);
        assert_eq!(expressions[0].name, name);
        assert_eq!(expressions[0].parameter_index, None);
        assert_eq!(expressions[0].qualifier, None);
    }
    assert!(super::super::expression_declaration_name(b"\x04\x08p12foo\0").is_none());
    assert!(super::super::expression_declaration_name(b"\x04\x06p12_\0").is_none());
}

#[test]
fn om_numeric_expression_evaluates_constant_arithmetic_formula() {
    let text = b"(Number [mm]) p9: (193.94 - 6) / 2 + 1.5e1; ";
    let mut bytes = b"hostglobalvariables".to_vec();
    bytes.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
    bytes.extend_from_slice(text);
    bytes.push(0);

    let expressions = super::super::numeric_expressions(&bytes);
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].expression, "(193.94 - 6) / 2 + 1.5e1");
    assert_eq!(expressions[0].value, Some(108.97));
}

#[test]
fn om_numeric_expression_accepts_inches_and_terminal_comments() {
    let texts = [
        b"(Number [in]) p1: 0.5; ".as_slice(),
        b"(Number [in]) p2: p1 * 2; // Used By ...\n".as_slice(),
        b"(Number [custom/unit]) p3: 4; ".as_slice(),
    ];
    let mut bytes = b"hostglobalvariables".to_vec();
    for text in texts {
        bytes.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
        bytes.extend_from_slice(text);
        bytes.push(0);
    }

    let expressions = super::super::numeric_expressions(&bytes);

    assert_eq!(expressions.len(), 3);
    assert_eq!(expressions[0].unit, super::super::ExpressionUnit::Inch);
    assert_eq!(expressions[0].expression, "0.5");
    assert_eq!(expressions[0].value, Some(0.5));
    assert_eq!(expressions[1].expression, "p1 * 2");
    assert_eq!(expressions[1].value, None);
    assert_eq!(
        expressions[2].unit,
        super::super::ExpressionUnit::Native("custom/unit".into())
    );
    assert_eq!(expressions[2].value, Some(4.0));
}

#[test]
fn om_numeric_expression_applies_power_before_unary_sign() {
    for (formula, expected) in [
        ("-2^2", -4.0),
        ("(-2)^2", 4.0),
        ("2^-2", 0.25),
        ("2^3^2", 512.0),
    ] {
        assert_eq!(
            super::super::evaluate_constant_expression(formula),
            Some(expected),
            "{formula}"
        );
    }
}

#[test]
fn om_numeric_expression_parser_handles_deep_nesting_without_recursion() {
    const DEPTH: usize = 16 * 1024;

    let nested = format!("{}1{}", "(".repeat(DEPTH), ")".repeat(DEPTH));
    assert_eq!(
        super::super::evaluate_constant_expression(&nested),
        Some(1.0)
    );

    let unary = format!("{}1", "+".repeat(DEPTH));
    assert_eq!(
        super::super::evaluate_constant_expression(&unary),
        Some(1.0)
    );

    let malformed = format!("{}1", "(".repeat(DEPTH));
    assert_eq!(super::super::evaluate_constant_expression(&malformed), None);
}

#[test]
fn om_string_value_requires_marker_length_printability_and_terminator() {
    let bytes = b"\x66\x32\x03\x0cSKETCH_001\0\x66\x32\x03\x03A\0\x66\x32\x03\x03A\x01";
    let values = super::super::string_values(bytes, 100);
    assert_eq!(values.len(), 2);
    assert_eq!(values[0].offset, 100);
    assert_eq!(values[0].value, "SKETCH_001");
    assert_eq!(values[1].value, "A");
}

#[test]
fn om_tagged_references_preserve_family_value_order_and_bounds() {
    let bytes = b"\xe0\x12\x34\x56\x78\xca\xbc\xde\xf0\xe0\x01";
    let references = super::super::references(bytes, 20);
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].offset, 20);
    assert_eq!(
        references[0].kind,
        super::super::ReferenceKind::PersistentHandle
    );
    assert_eq!(references[0].value, 0x1234_5678);
    assert_eq!(references[1].offset, 25);
    assert_eq!(references[1].kind, super::super::ReferenceKind::Tagged28);
    assert_eq!(references[1].value, 0x0abc_def0);
}

#[test]
fn om_counted_record_references_require_a_complete_in_bounds_run() {
    let bytes = b"\xff\x01\x03\x90\x00\x02\x90\x00\x04\x01\x02\x90\x00\x05";
    let references = super::super::counted_record_references(bytes, 100, 5);
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].offset, 103);
    assert_eq!(
        references[0].kind,
        super::super::ReferenceKind::RecordOrdinal16
    );
    assert_eq!(references[0].value, 2);
    assert_eq!(references[1].value, 4);
}

#[test]
fn om_record_references_require_adjacent_persistent_tagged_pairs() {
    let mut dense = b"ordinary-prefix".to_vec();
    for value in 1..=8u32 {
        dense.push(0xe0);
        dense.extend_from_slice(&value.to_be_bytes());
        dense.extend_from_slice(&(0xc000_0000 | value).to_be_bytes());
    }
    let references = super::super::record_references(&dense, 100);
    assert_eq!(references.len(), 16);
    assert_eq!(references[0].offset, 115);

    let mut unpaired = dense;
    unpaired.extend_from_slice(&[0xc0, 0, 0, 2]);
    assert_eq!(
        super::super::record_references(&unpaired, 0)
            .into_iter()
            .filter(|reference| reference.kind == super::super::ReferenceKind::Tagged28)
            .count(),
        8
    );

    let lone_tagged = [0xc0, 0, 0, 1];
    assert!(super::super::record_references(&lone_tagged, 0)
        .into_iter()
        .all(|reference| reference.kind != super::super::ReferenceKind::Tagged28));
}

#[test]
fn om_numeric_expression_table_is_independent_of_entity_indexing() {
    let bytes = b"hostglobalvariables\x99\x04P(Number [degrees]) p8_CircularPattern_pattern_Circular_Dir_offset_angle: 120; \x00";
    let expressions = super::super::numeric_expressions(bytes);
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].object_id, None);
    assert_eq!(
        expressions[0].name,
        "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
    );
    assert_eq!(expressions[0].parameter_index, Some(8));
    assert_eq!(
        expressions[0].qualifier,
        Some("CircularPattern_pattern_Circular_Dir_offset_angle")
    );
    assert_eq!(expressions[0].value, Some(120.0));
}
