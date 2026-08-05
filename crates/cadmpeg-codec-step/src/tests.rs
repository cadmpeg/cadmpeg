// SPDX-License-Identifier: Apache-2.0
//! Self-contained tests: IR documents are built in code (via the IR crate's
//! fixtures or inline), and expected STEP fragments are asserted inline. No test
//! depends on an external STEP consumer.
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use cadmpeg_ir::codec::{Codec, CodecEntry, Confidence, DecodeOptions};

use cadmpeg_core::decode::{DecodeMode, InspectOptions};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::{LengthUnit, Units};
use cadmpeg_ir::CadIr;
use std::fmt::Write as _;
use std::io::Cursor;

use crate::{
    write_step, StepCodec, StepError, StepSchema, StepUnsupportedPolicy, StepWriteOptions,
};

#[test]
fn string_codec_decodes_all_part21_escape_forms_and_round_trips_unicode() {
    use crate::strings::{decode, encode};

    assert_eq!(decode(b"it''s").unwrap(), "it's");
    assert_eq!(decode(b"a\\\\b").unwrap(), "a\\b");
    assert_eq!(decode(b"\\X\\E9").unwrap(), "é");
    assert_eq!(decode(b"\\X2\\03A9\\X0\\").unwrap(), "Ω");
    assert_eq!(decode(b"\\X4\\0001F642\\X0\\").unwrap(), "🙂");
    assert_eq!(decode(b"\\S\\D").unwrap(), "Ä");
    assert_eq!(decode(b"\\PA\\\\S\\D").unwrap(), "Ä");
    assert_eq!(decode(b"\\PB\\\\S\\A").unwrap(), "Á");
    assert_eq!(decode(b"\\PC\\\\S\\!").unwrap(), "Ħ");
    assert_eq!(decode(b"\\PD\\\\S\\!").unwrap(), "Ą");
    assert_eq!(decode(b"\\PE\\\\S\\0").unwrap(), "А");
    assert_eq!(decode(b"\\PF\\\\S\\G").unwrap(), "ا");
    assert_eq!(decode(b"\\PG\\\\S\\A").unwrap(), "Α");
    assert_eq!(decode(b"\\PH\\\\S\\`").unwrap(), "א");
    assert_eq!(decode(b"\\PI\\\\S\\P").unwrap(), "Ğ");

    for text in ["ASCII", "it's \\ quoted", "café Ω 🙂"] {
        assert_eq!(decode(encode(text).as_bytes()).unwrap(), text);
    }
}

#[test]
fn writer_and_lexer_preserve_apostrophes_and_backslashes_once() {
    use crate::lex::{lex, TokenKind};

    let source = "O'Brien \\ fixtures";
    let encoded = crate::writer::string(source);
    let tokens = lex(encoded.as_bytes()).expect("lex encoded string");
    let TokenKind::String(bytes) = &tokens[0].kind else {
        panic!("encoded text did not lex as a string")
    };
    assert_eq!(crate::strings::decode(bytes).unwrap(), source);
    assert!(encoded.contains("O''Brien"));
    assert!(encoded.contains("\\\\"));
}

#[test]
fn lexer_decodes_binary_literals_and_rejects_invalid_bit_boundaries() {
    use crate::lex::{lex, BinaryValue, TokenKind};

    assert_eq!(
        lex(b"\"0A1F\"").unwrap()[0].kind,
        TokenKind::Binary(BinaryValue {
            bit_len: 12,
            data: vec![0xa1, 0xf0],
        })
    );
    assert_eq!(
        lex(b"\"17E\"").unwrap()[0].kind,
        TokenKind::Binary(BinaryValue {
            bit_len: 7,
            data: vec![0x7e],
        })
    );
    for invalid in [b"\"\"".as_slice(), b"\"4FF\"", b"\"17F\"", b"\"3A7\""] {
        assert!(lex(invalid).is_err(), "accepted {invalid:?}");
    }
}

#[test]
fn parser_rejects_excessive_parameter_nesting_without_recursing_unboundedly() {
    let nested = format!("{}1{}", "(".repeat(300), ")".repeat(300));
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','','',(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM({nested});ENDSEC;END-ISO-10303-21;"
    );
    let error = crate::parse::parse(source.as_bytes()).unwrap_err();
    assert!(error.to_string().contains("nesting exceeds 256 levels"));
}

#[test]
fn parser_bounds_exponential_anchor_expansion() {
    let mut anchors = String::from("<a0>=(1,1);\n");
    for index in 1..40 {
        writeln!(anchors, "<a{index}>=(<a{}>,<a{}>);", index - 1, index - 1)
            .expect("write anchor fixture");
    }
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'3;1');FILE_NAME('','','',(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;{anchors}ENDSEC;DATA;#1=ITEM(<a39>);ENDSEC;END-ISO-10303-21;"
    );
    let error = crate::parse::parse(source.as_bytes()).unwrap_err();
    assert!(error.to_string().contains("expanded anchor value exceeds"));
}

#[test]
fn parser_bounds_aggregate_anchor_materialization() {
    let mut anchors = String::from("<a0>=(1,1);\n");
    for index in 1..18 {
        writeln!(anchors, "<a{index}>=(<a{}>,<a{}>);", index - 1, index - 1)
            .expect("write anchor fixture");
    }
    let records = (1..=8).fold(String::new(), |mut records, id| {
        write!(records, "#{id}=ITEM(<a17>);").expect("write anchor record fixture");
        records
    });
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'3;1');FILE_NAME('','','',(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;{anchors}ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;"
    );
    let error = crate::parse::parse(source.as_bytes()).unwrap_err();
    assert!(error.to_string().contains("expanded anchor"));
}

#[test]
fn parser_rejects_duplicate_complex_partial_names() {
    let source = b"ISO-10303-21;HEADER;ENDSEC;DATA;#1=(B()A()B());ENDSEC;END-ISO-10303-21;";
    let error = crate::parse::parse(source).expect_err("duplicate partial names must fail");
    assert!(matches!(
        error,
        crate::parse::ParseError::Syntax { message, .. }
            if message == "duplicate complex partial name"
    ));
}

#[test]
fn parser_reports_recoverable_noncanonical_complex_partial_order() {
    let source = b"ISO-10303-21;HEADER;ENDSEC;DATA;#1=(NAMED_UNIT(#2)SOLID_ANGLE_UNIT()SI_UNIT($,.STERADIAN.));#2=DIMENSIONAL_EXPONENTS(0.,0.,0.,0.,0.,0.,0.);ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::parse::parse(source).expect("noncanonical partial order is recoverable");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].offset,
        source
            .windows(2)
            .position(|window| window == b"#1")
            .unwrap()
    );
    assert_eq!(
        diagnostics[0].kind,
        crate::parse::ParseDiagnosticKind::ComplexPartialsNotAlphabetical
    );
    assert_eq!(
        diagnostics[0].message,
        "complex partial records are not alphabetical: observed (NAMED_UNIT, SOLID_ANGLE_UNIT, SI_UNIT), expected (NAMED_UNIT, SI_UNIT, SOLID_ANGLE_UNIT)"
    );
    assert_eq!(
        exchange.records[&1]
            .partials
            .iter()
            .map(|partial| partial.name.as_str())
            .collect::<Vec<_>>(),
        ["NAMED_UNIT", "SOLID_ANGLE_UNIT", "SI_UNIT"]
    );
}

#[test]
fn decode_salvages_noncanonical_complex_partial_order_with_provenance() {
    let bytes = include_bytes!("../tests/fixtures/noncanonical_solid_angle.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("salvage mode accepts recoverable source order");
    let losses = result
        .report
        .losses
        .iter()
        .filter(|loss| loss.code == cadmpeg_ir::LossKind::NoncanonicalSourceSyntax)
        .collect::<Vec<_>>();

    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].severity, cadmpeg_ir::Severity::Warning);
    let provenance = losses[0].provenance.as_ref().expect("source provenance");
    assert_eq!(provenance.format, "step");
    assert_eq!(provenance.stream, "");
    assert_eq!(
        provenance.offset,
        bytes.windows(2).position(|window| window == b"#1").unwrap() as u64
    );
    assert_eq!(provenance.tag.as_deref(), Some("complex_entity"));
    assert_eq!(result.ir.native_unknowns("step").unwrap().len(), 0);
    assert_eq!(
        result.ir.source.as_ref().unwrap().attributes["bytes_named_opaque"],
        "0"
    );
}

#[test]
fn strict_decode_rejects_noncanonical_complex_partial_order() {
    let bytes = include_bytes!("../tests/fixtures/noncanonical_solid_angle.p21");
    let mut options = DecodeOptions::default();
    options.policy.mode = DecodeMode::Strict;
    let error = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &options)
        .expect_err("strict mode rejects noncanonical source order");

    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn inspect_accepts_noncanonical_complex_partial_order_and_reports_a_note() {
    let bytes = include_bytes!("../tests/fixtures/noncanonical_solid_angle.p21");
    let summary = StepCodec::default()
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("inspection describes recoverable source order");

    assert!(summary
        .notes
        .iter()
        .any(|note| note.contains("complex partial records are not alphabetical")));
}

#[test]
fn exporting_a_salvaged_noncanonical_unit_repairs_partial_order() {
    let bytes = include_bytes!("../tests/fixtures/noncanonical_solid_angle.p21");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode noncanonical unit fixture");
    let mut output = Vec::new();
    write_step(&decoded.ir, &mut output, &StepWriteOptions::default()).expect("export salvaged IR");

    let (exchange, diagnostics) = crate::parse::parse(&output).expect("parse repaired output");
    assert!(diagnostics.is_empty());
    let unit = exchange
        .records
        .values()
        .find(|record| {
            record
                .partials
                .iter()
                .any(|partial| partial.name == "SOLID_ANGLE_UNIT")
        })
        .expect("exported solid-angle unit");
    assert_eq!(
        unit.partials
            .iter()
            .map(|partial| partial.name.as_str())
            .collect::<Vec<_>>(),
        ["NAMED_UNIT", "SI_UNIT", "SOLID_ANGLE_UNIT"]
    );
}

#[test]
fn codec_detects_and_inspects_ap242_exchange_structure() {
    let bytes = include_bytes!("../tests/fixtures/ap242_minimal.p21");
    let codec = StepCodec::default();

    assert_eq!(codec.detect(bytes), Confidence::High);
    assert_eq!(codec.detect(b"PK\x03\x04"), Confidence::No);

    let summary = codec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("inspect minimal AP242");
    assert_eq!(summary.format, "step");
    assert_eq!(summary.container_kind, "iso-10303-21-clear-text");
    assert_eq!(summary.entries.len(), 2);
    assert_eq!(summary.entries[0].name, "HEADER");
    assert_eq!(summary.entries[1].name, "DATA[0]");
    assert_eq!(summary.entries[1].attributes["entity_count"], "2");
    assert_eq!(
        summary.entries[1].attributes["unknown_entities"],
        "EXAMPLE_RECORD:1,OPAQUE_TARGET:1"
    );
    assert!(summary
        .notes
        .iter()
        .any(|note| note.contains("AP242") && note.contains("edition 2")));
}

#[test]
fn codec_refuses_out_of_envelope_encodings_by_name() {
    let codec = StepCodec::default();
    let cases: &[(&[u8], &str)] = &[
        (b"PK\x03\x04archive", "STEP Part 21 ZIP container"),
        (
            b"\x89HDF\r\n\x1a\ncontent",
            "STEP Part 26 binary/HDF5 encoding",
        ),
        (
            b"<?xml version='1.0'?><iso_10303_28/>",
            "STEP Part 28 XML encoding",
        ),
        (
            b"<?xml version='1.0'?><business_object_model/>",
            "AP242 BO-Model XML sidecar",
        ),
    ];
    for &(bytes, reason) in cases {
        let error = codec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::NotImplemented(message) if message == reason)
        );
    }
    assert_eq!(
        codec.detect(b"<?xml version='1.0'?><iso_10303_28/>"),
        Confidence::Medium
    );
}

#[test]
fn codec_inspects_edition3_sections_and_external_references() {
    let bytes = include_bytes!("../tests/fixtures/ap242_ed3_sections.p21");
    let summary = StepCodec::default()
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("inspect edition 3 sections");

    assert_eq!(
        summary
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        [
            "HEADER",
            "ANCHOR",
            "REFERENCE",
            "DATA[0]",
            "DATA[1]",
            "SIGNATURE"
        ]
    );
    let references = summary
        .entries
        .iter()
        .find(|entry| entry.name == "REFERENCE")
        .unwrap();
    assert_eq!(references.attributes["external_count"], "1");
    assert_eq!(
        references.attributes["external_uris"],
        "https://example.invalid/external-part"
    );
    assert_eq!(summary.entries[3].attributes["unknown_entities"], "");
    assert_eq!(
        summary.entries[4].attributes["unknown_entities"],
        "EXAMPLE_RECORD:1"
    );
    let (exchange, diagnostics) =
        crate::parse::parse(bytes).expect("parse opaque signature payload");
    assert!(diagnostics.is_empty());
    let signature = exchange.signature.expect("signature byte span");
    assert!(bytes[signature].windows(2).any(|bytes| bytes == b"@%"));
    assert_eq!(
        exchange.records[&2].partials[0].parameters,
        vec![crate::parse::Value::Reference(1)]
    );
}

#[test]
fn decode_reports_data_section_external_dependencies() {
    let bytes = include_bytes!("../tests/fixtures/ap242_external_documents.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode external document dependencies");

    assert!(result.report.notes.contains(
        &"external document SPEC-42 (Interface control drawing) from supplier vault".into()
    ));
    assert!(result
        .report
        .notes
        .contains(&"external source https://example.invalid/library item fastener-table".into()));

    let summary = StepCodec::default()
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("inspect external document dependencies");
    let dependencies = summary
        .entries
        .iter()
        .find(|entry| entry.name == "EXTERNAL_DEPENDENCIES")
        .expect("external dependency inventory");
    assert_eq!(dependencies.attributes["dependency_count"], "2");
}

#[test]
fn decode_preserves_named_opaque_records_with_exact_byte_spans() {
    let bytes = include_bytes!("../tests/fixtures/ap242_minimal.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode parsed STEP document");

    assert_eq!(result.ir.source.as_ref().unwrap().format, "step");
    let unknowns = result
        .ir
        .native
        .namespace("step")
        .unwrap()
        .arena_as::<cadmpeg_ir::UnknownRecord>("unknowns")
        .unwrap();
    assert_eq!(unknowns.len(), 2);
    assert_eq!(unknowns[0].id.0, "step:data:example_record#1");
    assert_eq!(
        unknowns[0].data.as_deref(),
        Some(
            &bytes
                [unknowns[0].offset as usize..(unknowns[0].offset + unknowns[0].byte_len) as usize]
        )
    );
    assert!(unknowns[0]
        .links
        .contains(&"step:data:opaque_target#2".to_string()));
    assert!(!result.report.geometry_transferred);
    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("EXAMPLE_RECORD")));
}

#[test]
fn decode_accounts_for_every_part21_byte() {
    let bytes = include_bytes!("../tests/fixtures/ap242_semantic_pmi.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode byte-accounting fixture");
    let attributes = &result.ir.source.as_ref().unwrap().attributes;
    let count = |name: &str| attributes[name].parse::<usize>().unwrap();

    assert!(count("bytes_structural") > 0);
    assert!(count("bytes_typed") > 0);
    assert_eq!(count("bytes_named_opaque"), 0);
    assert_eq!(count("bytes_unclassified"), 0);
    assert_eq!(
        count("bytes_structural") + count("bytes_typed") + count("bytes_named_opaque"),
        bytes.len()
    );
}

#[test]
fn consumed_unit_and_pmi_wrapper_records_are_strictly_writable() {
    for source in [
        include_bytes!("../tests/fixtures/ap242_degree_cone.p21").as_slice(),
        include_bytes!("../tests/fixtures/ap242_semantic_pmi.p21").as_slice(),
    ] {
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("decode typed STEP wrappers");
        assert!(decoded
            .ir
            .native_unknowns("step")
            .expect("STEP unknown arena")
            .is_empty());
        let mut bytes = Vec::new();
        write_step(
            &decoded.ir,
            &mut bytes,
            &StepWriteOptions {
                schema: StepSchema::Ap242Edition3,
                unsupported: StepUnsupportedPolicy::Reject,
                ..StepWriteOptions::default()
            },
        )
        .expect("strictly write typed STEP wrappers");
        assert!(!bytes.is_empty());
    }
}

#[test]
fn every_repository_step_fixture_has_complete_byte_accounting() {
    let fixtures: &[(&str, &[u8])] = &[
        (
            "ap203_sheet",
            include_bytes!("../tests/fixtures/ap203_sheet.p21"),
        ),
        (
            "ap214_sheet",
            include_bytes!("../tests/fixtures/ap214_sheet.p21"),
        ),
        (
            "ap242_assembly",
            include_bytes!("../tests/fixtures/ap242_assembly.p21"),
        ),
        (
            "ap242_conversion_units",
            include_bytes!("../tests/fixtures/ap242_conversion_units.p21"),
        ),
        (
            "ap242_ed3_sections",
            include_bytes!("../tests/fixtures/ap242_ed3_sections.p21"),
        ),
        (
            "ap242_degree_cone",
            include_bytes!("../tests/fixtures/ap242_degree_cone.p21"),
        ),
        (
            "ap242_external_documents",
            include_bytes!("../tests/fixtures/ap242_external_documents.p21"),
        ),
        (
            "ap242_geometry",
            include_bytes!("../tests/fixtures/ap242_geometry.p21"),
        ),
        (
            "ap242_geometric_set",
            include_bytes!("../tests/fixtures/ap242_geometric_set.p21"),
        ),
        (
            "ap242_mapped_assembly",
            include_bytes!("../tests/fixtures/ap242_mapped_assembly.p21"),
        ),
        (
            "ap242_minimal",
            include_bytes!("../tests/fixtures/ap242_minimal.p21"),
        ),
        (
            "ap242_presentation_pmi",
            include_bytes!("../tests/fixtures/ap242_presentation_pmi.p21"),
        ),
        (
            "ap242_semantic_pmi",
            include_bytes!("../tests/fixtures/ap242_semantic_pmi.p21"),
        ),
        (
            "ap242_tessellation",
            include_bytes!("../tests/fixtures/ap242_tessellation.p21"),
        ),
        (
            "ap242_vertex_loop",
            include_bytes!("../tests/fixtures/ap242_vertex_loop.p21"),
        ),
        (
            "complex_instance",
            include_bytes!("../tests/fixtures/complex_instance.p21"),
        ),
        ("strings", include_bytes!("../tests/fixtures/strings.p21")),
    ];
    for &(name, bytes) in fixtures {
        let result = StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let attributes = &result.ir.source.as_ref().unwrap().attributes;
        let count = |key: &str| attributes[key].parse::<usize>().unwrap();
        assert_eq!(count("bytes_unclassified"), 0, "{name}");
        assert_eq!(
            count("bytes_structural") + count("bytes_typed") + count("bytes_named_opaque"),
            bytes.len(),
            "{name}"
        );
    }
}

#[test]
fn decode_transfers_placed_analytic_geometry_in_millimetres() {
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

    let bytes = include_bytes!("../tests/fixtures/ap242_geometry.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode typed STEP geometry");

    assert_eq!(result.ir.model.points.len(), 1);
    let placed = result
        .ir
        .model
        .points
        .iter()
        .find(|point| point.id.0 == "step:data:point#3")
        .unwrap();
    assert_eq!(placed.position.x, 1.0);
    assert_eq!(placed.position.y, 2.0);
    assert_eq!(placed.position.z, 3.0);
    assert_eq!(result.ir.model.curves.len(), 9);
    assert!(result.ir.model.curves.iter().any(|curve| {
        curve.id.as_str() == "step:data:curve#45"
            && matches!(curve.geometry, CurveGeometry::Composite { .. })
    }));
    assert!(result.ir.model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Line { origin, direction }
            if origin.x == 1.0 && origin.y == 2.0 && origin.z == 3.0
                && direction.x == 0.0 && direction.y == 0.0 && direction.z == 1.0
    )));
    assert!(!result.report.losses.iter().any(|loss| loss
        .message
        .contains("GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #51")));
    assert!(result
        .ir
        .model
        .procedural_curves
        .iter()
        .any(|curve| matches!(
            curve.definition,
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                parameter_range: [start, end],
                ..
            } if start == 0.0 && (end - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12
        )));
    assert!(result.ir.model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Ellipse { major_radius, minor_radius, .. }
            if major_radius == 6.0 && minor_radius == 2.0
    )));
    assert!(result.ir.model.curves.iter().any(|curve| matches!(
        &curve.geometry,
        CurveGeometry::Nurbs(nurbs)
            if nurbs.degree == 2
                && nurbs.knots == [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
                && nurbs.weights.as_deref() == Some(&[1.0, 0.5, 1.0][..])
    )));
    assert_eq!(result.ir.model.surfaces.len(), 10);
    assert!(result
        .ir
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Curve(_)
        )));
    assert!(result
        .ir
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Surface(_)
        )));
    assert!(result
        .ir
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Point(_)
        )));
    assert!(!result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("STYLED_ITEM #43")));
    assert!(!result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("STYLED_ITEM #52")));
    assert_eq!(
        result
            .ir
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| binding.source_entity_id.as_deref() == Some("#47"))
            .count(),
        2
    );
    assert!(result
        .ir
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            &binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Source { source_id } if source_id == "#6"
        )));
    assert!(result.ir.model.curves.iter().any(|curve| matches!(
        &curve.geometry,
        CurveGeometry::Nurbs(nurbs)
            if curve.id.as_str() == "step:data:curve#48"
                && nurbs.degree == 1
                && nurbs.knots == [0.0, 0.0, 1.0, 2.0, 2.0]
    )));
    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Plane { origin, normal, .. }
            if origin.x == 1.0 && origin.y == 2.0 && origin.z == 3.0 && normal.z == 1.0
    )));
    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        &surface.geometry,
        SurfaceGeometry::Nurbs(nurbs)
            if nurbs.u_degree == 1
                && nurbs.v_degree == 1
                && nurbs.u_count == 2
                && nurbs.v_count == 2
                && nurbs.u_knots == [0.0, 0.0, 1.0, 1.0]
                && nurbs.v_knots == [0.0, 0.0, 1.0, 1.0]
                && nurbs.weights.as_deref() == Some(&[1.0, 1.0, 1.0, 0.75][..])
    )));
    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cylinder { radius, .. } if radius == 5.0
    )));
    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cone { radius, ratio, half_angle, .. }
            if radius == 5.0 && ratio == 1.0 && half_angle == 0.25
    )));
    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Sphere { radius, .. } if radius == 5.0
    )));
    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Torus { major_radius, minor_radius, .. }
            if major_radius == 8.0 && minor_radius == 2.0
    )));
    assert!(result.ir.model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Circle { center, radius, .. }
            if center.x == 1.0 && center.y == 2.0 && center.z == 3.0 && radius == 4.0
    )));
    assert!(result.report.geometry_transferred);
    assert_eq!(result.ir.model.procedural_curves.len(), 3);
    let cartesian_trim = result
        .ir
        .model
        .procedural_curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:construction:trimmed_curve#29")
        .expect("Cartesian trimmed curve");
    assert!(matches!(
        cartesian_trim.definition,
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
            parameter_range: [start, end],
            ..
        } if start == 0.0 && (end - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12
    ));
    let (source, parameter_range) = result
        .ir
        .model
        .procedural_curves
        .iter()
        .find_map(|curve| match &curve.definition {
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                source,
                parameter_range,
            } => Some((source, *parameter_range)),
            _ => None,
        })
        .expect("trimmed curve was not retained as a subset construction");
    assert_eq!(source.as_str(), "step:data:curve#8");
    assert_eq!(parameter_range, [0.0, std::f64::consts::FRAC_PI_2]);
    assert!(result
        .ir
        .model
        .procedural_curves
        .iter()
        .any(|curve| matches!(
            curve.definition,
            cadmpeg_ir::geometry::ProceduralCurveDefinition::SpatialOffset {
                distance: 1.0,
                self_intersect: None,
                ..
            }
        )));
    assert_eq!(result.ir.model.procedural_surfaces.len(), 4);
    assert!(result
        .ir
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| matches!(
            surface.definition,
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::DegenerateTorus {
                select_outer: true
            }
        )));
    assert!(result
        .ir
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| matches!(
            surface.definition,
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::LinearSweep { direction, .. }
                if direction.z == 2.0
        )));
    assert!(result
        .ir
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| matches!(
            surface.definition,
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::AxisRevolution { axis_direction, .. }
                if axis_direction.z == 1.0
        )));
    assert!(result
        .ir
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| matches!(
            surface.definition,
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::ParallelOffset {
                distance: 0.5,
                self_intersect: Some(false),
                ..
            }
        )));
}

#[test]
fn procedural_step_geometry_round_trips_as_native_entities() {
    let source = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../tests/fixtures/ap242_geometry.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode procedural geometry");

    let mut bytes = Vec::new();
    let report = write_step(
        &source.ir,
        &mut bytes,
        &StepWriteOptions {
            schema: StepSchema::Ap242Edition3,
            ..StepWriteOptions::default()
        },
    )
    .expect("write procedural geometry");
    let text = String::from_utf8(bytes.clone()).expect("utf8 STEP");
    for entity in [
        "GEOMETRIC_SET",
        "TRIMMED_CURVE",
        "OFFSET_CURVE_3D",
        "SURFACE_OF_LINEAR_EXTRUSION",
        "SURFACE_OF_REVOLUTION",
        "OFFSET_SURFACE",
        "DEGENERATE_TOROIDAL_SURFACE",
    ] {
        assert!(text.contains(entity), "missing {entity}");
    }
    assert!(!report.losses.iter().any(|loss| loss
        .message
        .contains("reduced to their solved STEP carriers")));
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains("normalized to positive STEP radii")));

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode written procedural geometry");
    assert_eq!(decoded.ir.model.procedural_curves.len(), 3);
    assert_eq!(decoded.ir.model.procedural_surfaces.len(), 4);

    let bounded = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../tests/fixtures/ap242_geometric_set.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode curve-bounded surface");
    let mut bytes = Vec::new();
    let report = write_step(&bounded.ir, &mut bytes, &StepWriteOptions::default())
        .expect("write curve-bounded surface");
    let text = String::from_utf8(bytes.clone()).expect("utf8 STEP");
    assert!(!text.contains("CURVE_BOUNDED_SURFACE"));
    assert!(text.contains("GEOMETRIC_SET"));
    assert!(report.losses.iter().any(|loss| loss
        .message
        .contains("reduced to their solved STEP carriers")));
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode written curve-bounded surface");
    assert!(!decoded
        .ir
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| matches!(
            surface.definition,
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::CurveBounded { .. }
        )));
    let mut rejected = Vec::new();
    assert!(write_step(
        &bounded.ir,
        &mut rejected,
        &StepWriteOptions {
            unsupported: StepUnsupportedPolicy::Reject,
            ..StepWriteOptions::default()
        }
    )
    .is_err());
    assert!(rejected.is_empty());
}

#[test]
fn decode_conical_apex_and_context_plane_angle_units() {
    let bytes = include_bytes!("../tests/fixtures/ap242_degree_cone.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode degree cone");

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cone { radius, half_angle, .. }
            if radius == 0.0 && (half_angle - std::f64::consts::FRAC_PI_4).abs() < 1.0e-12
    )));
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(
        validation
            .findings
            .iter()
            .all(|finding| finding.check != cadmpeg_ir::Check::CarrierReachability),
        "{:#?}",
        validation.findings
    );
}

#[test]
fn decode_and_write_singular_vertex_loops() {
    let bytes = include_bytes!("../tests/fixtures/ap242_vertex_loop.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode vertex loops");
    assert_eq!(result.ir.model.loops.len(), 2);
    assert!(result
        .ir
        .model
        .loops
        .iter()
        .all(|loop_| loop_.coedges.is_empty() && loop_.vertex_uses.len() == 1));
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    let mut encoded = Vec::new();
    write_step(&result.ir, &mut encoded, &StepWriteOptions::default()).expect("write vertex loops");
    assert_eq!(
        String::from_utf8(encoded)
            .unwrap()
            .matches("VERTEX_LOOP")
            .count(),
        2
    );
}

#[test]
fn decode_resolves_conversion_units_and_linear_uncertainty() {
    let bytes = include_bytes!("../tests/fixtures/ap242_conversion_units.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode conversion-based units");

    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.points[0].position.x, 50.8);
    assert_eq!(result.ir.tolerances.linear, 0.0254);
}

#[test]
fn decode_builds_a_valid_connected_sheet_brep() {
    use cadmpeg_ir::topology::{BodyKind, Sense};

    let bytes = include_bytes!("../tests/fixtures/ap214_sheet.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP214 sheet");

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir.model.regions.len(), 1);
    assert_eq!(result.ir.model.shells.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 3);
    assert_eq!(result.ir.model.edges.len(), 3);
    assert_eq!(result.ir.model.vertices.len(), 3);
    assert_eq!(result.ir.model.pcurves.len(), 1);
    assert_eq!(
        result
            .ir
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
    assert!(matches!(
        result.ir.model.pcurves[0].geometry,
        cadmpeg_ir::geometry::PcurveGeometry::Line { origin, direction }
            if origin == cadmpeg_ir::math::Point2::new(0.0, 0.0)
                && direction == cadmpeg_ir::math::Point2::new(1.0, 0.0)
    ));
    assert!(result
        .ir
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.sense == Sense::Forward));
    assert_eq!(result.ir.model.faces[0].sense, Sense::Reversed);
    assert!(result
        .ir
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Edge(_)
        )));
    assert_eq!(
        result.ir.model.faces[0].color,
        Some(cadmpeg_ir::topology::Color {
            r: 0.9,
            g: 0.1,
            b: 0.1,
            a: 1.0,
        })
    );
    assert_eq!(result.ir.model.presentation_layers.len(), 1);
    assert_eq!(
        result.ir.model.presentation_layers[0].name,
        "machined faces"
    );
    assert!(matches!(
        result.ir.model.presentation_layers[0].items.as_slice(),
        [cadmpeg_ir::PresentationItem::Face { .. }]
    ));
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let mut output = Vec::new();
    let report = write_step(&result.ir, &mut output, &StepWriteOptions::default())
        .expect("write sheet pcurve");
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains("coedge pcurve(s) use unsupported")));
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode written pcurve");
    assert_eq!(roundtrip.ir.model.pcurves.len(), 1);
    assert_eq!(roundtrip.ir.model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(roundtrip.ir.model.presentation_layers.len(), 1);
    assert_eq!(
        roundtrip.ir.model.presentation_layers[0].name,
        "machined faces"
    );
    assert!(roundtrip
        .ir
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Edge(_)
        )));
    assert_eq!(
        roundtrip
            .ir
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
}

#[test]
fn decode_builds_a_valid_ap203_sheet_brep() {
    use cadmpeg_ir::topology::BodyKind;

    let bytes = include_bytes!("../tests/fixtures/ap203_sheet.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP203 sheet");

    assert_eq!(
        result.ir.source.as_ref().unwrap().attributes["schema"],
        "CONFIG_CONTROL_DESIGN"
    );
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 3);
    assert_eq!(result.ir.model.vertices.len(), 3);
    let composite = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#34")
        .expect("outer composite curve");
    assert!(matches!(
        &composite.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Composite {
            segments,
            self_intersect: Some(false)
        } if segments.len() == 1
            && segments[0].curve.as_str() == "step:data:curve#36"
            && segments[0].same_sense
            && segments[0].transition
                == cadmpeg_ir::geometry::CompositeCurveTransition::ContSameGradient
    ));
    assert!(result
        .ir
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| matches!(
            &surface.definition,
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::CurveBounded {
                support,
                boundaries,
                implicit_outer: false
            } if support.as_str() == "step:data:surface#28"
                && boundaries.as_slice() == [cadmpeg_ir::ids::CurveId("step:data:curve#34".into())]
        )));
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let mut encoded = Vec::new();
    write_step(&result.ir, &mut encoded, &StepWriteOptions::default())
        .expect("write composite curve graph");
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("decode written composite curve graph");
    assert!(roundtrip
        .ir
        .model
        .curves
        .iter()
        .any(|curve| matches!(curve.geometry, CurveGeometry::Composite { .. })));
}

#[test]
fn decode_builds_a_face_based_surface_model() {
    use cadmpeg_ir::topology::BodyKind;

    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=FACE_BASED_SURFACE_MODEL('',(#30));",
        )
        .replace(
            "#30=OPEN_SHELL('',(#29));",
            "#30=CONNECTED_FACE_SET('',(#29));",
        );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode face-based surface model");

    assert_eq!(
        result.ir.model.bodies.len(),
        1,
        "{:#?}",
        result.report.losses
    );
    assert_eq!(result.ir.model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("does not resolve to a complete")));
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_faceted_brep_polygon_loops() {
    use cadmpeg_ir::topology::BodyKind;

    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#25=EDGE_LOOP('',(#22,#23,#24));",
            "#25=POLY_LOOP('',(#3,#4,#5,#3));",
        )
        .replace("#30=OPEN_SHELL('',(#29));", "#30=CLOSED_SHELL('',(#29));")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=FACETED_BREP('',#30);",
        );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode faceted brep");

    assert_eq!(
        result.ir.model.bodies.len(),
        1,
        "{:#?}",
        result.report.losses
    );
    assert_eq!(result.ir.model.bodies[0].kind, BodyKind::Solid);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 3);
    assert!(result
        .ir
        .model
        .edges
        .iter()
        .all(|edge| edge.curve.is_none()));
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn base_face_with_polygon_loop_gets_an_inferred_plane() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#25=EDGE_LOOP('',(#22,#23,#24));",
            "#25=POLY_LOOP('',(#3,#4,#5));",
        )
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#29=FACE('',(#26));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode base face");

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(decoded.ir.model.faces.len(), 1);
    assert!(decoded.ir.model.surfaces.iter().any(|surface| {
        surface.id.as_str() == "step:data:surface#implicit-face-29"
            && matches!(surface.geometry, SurfaceGeometry::Plane { .. })
    }));
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn base_edges_without_curve_carriers_remain_topological_edges() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#19=EDGE_CURVE('',#6,#7,#57,.T.);", "#19=EDGE('',#6,#7);")
        .replace("#20=EDGE_CURVE('',#7,#8,#17,.T.);", "#20=EDGE('',#7,#8);")
        .replace("#21=EDGE_CURVE('',#8,#6,#18,.T.);", "#21=EDGE('',#8,#6);")
        .replace("#16=LINE('',#3,#13);", "#16=UNSUPPORTED_CURVE('',());")
        .replace("#17=LINE('',#4,#14);", "#17=UNSUPPORTED_CURVE('',());")
        .replace("#18=LINE('',#5,#15);", "#18=UNSUPPORTED_CURVE('',());")
        .replace(
            "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
            "#57=UNSUPPORTED_CURVE('',());",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode base edges");

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(decoded.ir.model.edges.len(), 3);
    assert!(decoded
        .ir
        .model
        .edges
        .iter()
        .all(|edge| edge.curve.is_none()));
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn unresolved_vertex_point_does_not_enter_a_topology_draft() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#3=CARTESIAN_POINT('',(0.,0.,0.));",
            "#3=UNSUPPORTED_POINT('',());",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode missing vertex point carrier");

    assert!(decoded.ir.model.bodies.is_empty());
    assert!(decoded.ir.model.vertices.is_empty());
    assert!(decoded.report.losses.iter().any(|loss| loss
        .message
        .contains("VERTEX_POINT #6 has unresolved point carrier #3")));
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn sheet_root_salvages_independent_shells() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33,#34));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',#30,.F.);",
            "#33=ORIENTED_OPEN_SHELL('',#30,.F.);\n#34=ORIENTED_OPEN_SHELL('',#99,.T.);\n#99=UNSUPPORTED_SHELL('',());",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode sheet with one invalid shell");

    assert_eq!(
        decoded.ir.model.bodies.len(),
        1,
        "{:#?}",
        decoded.report.losses
    );
    assert!(decoded
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("omitted 1 unresolved shell")));
    assert!(decoded
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("shell carrier #34")));
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn shared_source_face_gets_one_owner_scoped_face_per_shell() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33,#34));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',#30,.F.);",
            "#33=ORIENTED_OPEN_SHELL('',#30,.F.);\n#34=OPEN_SHELL('',(#29));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode shared face shells");

    assert_eq!(
        decoded.ir.model.bodies.len(),
        2,
        "{:#?}",
        decoded.report.losses
    );
    assert_eq!(decoded.ir.model.faces.len(), 2);
    assert_eq!(
        decoded
            .ir
            .model
            .faces
            .iter()
            .map(|face| face.id.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        2
    );
    assert!(decoded
        .ir
        .model
        .faces
        .iter()
        .all(|face| face.color.is_some()));
    assert_eq!(decoded.ir.model.presentation_layers[0].items.len(), 2);
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn brep_with_voids_scopes_edges_and_vertices_per_shell_after_shared_shell_use() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#30=OPEN_SHELL('',(#29));", "#30=CLOSED_SHELL('',(#29));")
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',#30,.F.);",
            "#33=ORIENTED_OPEN_SHELL('',#30,.F.);\n#34=CLOSED_SHELL('',(#29));\n#70=BREP_WITH_VOIDS('',#30,(#34));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode scoped BREP with voids");

    assert!(decoded.ir.model.bodies.iter().any(|body| {
        body.id.as_str() == "step:data:body#70"
            && body.kind == cadmpeg_ir::topology::BodyKind::Solid
    }));
    assert!(!decoded
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("root #70 rejected")));
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn shell_wire_edge_applies_edge_and_occurrence_sense() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=SHELL_BASED_WIREFRAME_MODEL('',(#33));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',#30,.F.);",
            "#33=WIRE_SHELL('',(#25));",
        )
        .replace(
            "#19=EDGE_CURVE('',#6,#7,#57,.T.);",
            "#19=EDGE_CURVE('',#6,#7,#57,.F.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode oriented wire edge");

    let first = decoded
        .ir
        .model
        .edges
        .iter()
        .find(|edge| edge.id.as_str().contains("19-wire-31-33-22-0"))
        .expect("first wire edge");
    assert_eq!(first.start.as_str(), "step:data:vertex#7-wire-31-shell-33");
    assert_eq!(first.end.as_str(), "step:data:vertex#6-wire-31-shell-33");
}

#[test]
fn unowned_pcurve_dependencies_are_retained_as_one_opaque_closure() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#69=PCURVE('',#28,#70);\n#70=DEFINITIONAL_REPRESENTATION('',(#71),#50);\n#71=LINE('',#51,#53);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode unowned pcurve");
    let unknowns = decoded
        .ir
        .native
        .namespace("step")
        .expect("STEP namespace")
        .arena_as::<cadmpeg_ir::UnknownRecord>("unknowns")
        .expect("STEP unknown arena");
    assert!(unknowns
        .iter()
        .any(|record| record.id.0 == "step:data:pcurve#69"));
    assert!(unknowns
        .iter()
        .any(|record| record.id.0 == "step:data:definitional_representation#70"));
    let line = unknowns
        .iter()
        .find(|record| record.id.0 == "step:data:line#71")
        .expect("unowned pcurve line is retained");
    assert!(line
        .data
        .as_deref()
        .is_some_and(|data| data.starts_with(b"#71=LINE")));
    assert!(decoded
        .ir
        .model
        .pcurves
        .iter()
        .all(|pcurve| pcurve.id.as_str() != "step:data:pcurve#69"));
}

#[test]
fn writer_round_trips_rational_nurbs_pcurves() {
    let bytes = include_bytes!("../tests/fixtures/ap214_sheet.p21");
    let mut ir = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode sheet")
        .ir;
    ir.model.pcurves[0].geometry = cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![
            cadmpeg_ir::math::Point2::new(0.0, 0.0),
            cadmpeg_ir::math::Point2::new(1.0, 0.0),
        ],
        weights: Some(vec![1.0, 2.0]),
        periodic: false,
    };

    let mut output = Vec::new();
    write_step(&ir, &mut output, &StepWriteOptions::default()).expect("write NURBS pcurve");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode NURBS pcurve");
    assert!(matches!(
        &decoded.ir.model.pcurves[0].geometry,
        cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
            degree: 1,
            control_points,
            weights: Some(weights),
            periodic: false,
            ..
        } if control_points.len() == 2 && weights == &[1.0, 2.0]
    ));
}

#[test]
fn writer_round_trips_every_exact_step_pcurve_family() {
    use cadmpeg_ir::geometry::PcurveGeometry;
    use cadmpeg_ir::math::Point2;

    let bytes = include_bytes!("../tests/fixtures/ap214_sheet.p21");
    let template = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode sheet")
        .ir;
    let x_axis = Point2::new(0.6, 0.8);
    let y_axis = Point2::new(-0.8, 0.6);
    let cases = [
        PcurveGeometry::Circle {
            center: Point2::new(2.0, 3.0),
            x_axis,
            y_axis,
            radius: 4.0,
        },
        PcurveGeometry::Ellipse {
            center: Point2::new(2.0, 3.0),
            x_axis,
            y_axis,
            major_radius: 4.0,
            minor_radius: 2.0,
        },
        PcurveGeometry::Parabola {
            vertex: Point2::new(2.0, 3.0),
            x_axis,
            y_axis,
            focal_distance: 1.5,
        },
        PcurveGeometry::Hyperbola {
            center: Point2::new(2.0, 3.0),
            x_axis,
            y_axis,
            major_radius: 4.0,
            minor_radius: 2.0,
        },
        PcurveGeometry::Trimmed {
            parameter_range: [0.25, 1.75],
            basis: Box::new(PcurveGeometry::Circle {
                center: Point2::new(2.0, 3.0),
                x_axis,
                y_axis,
                radius: 4.0,
            }),
        },
        PcurveGeometry::Offset {
            distance: -0.5,
            basis: Box::new(PcurveGeometry::Line {
                origin: Point2::new(2.0, 3.0),
                direction: Point2::new(4.0, 0.0),
            }),
        },
    ];

    for geometry in cases {
        let mut ir = template.clone();
        ir.model.pcurves[0].geometry = geometry.clone();
        let mut output = Vec::new();
        write_step(&ir, &mut output, &StepWriteOptions::default()).expect("write exact pcurve");
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(output), &DecodeOptions::default())
            .expect("decode exact pcurve");
        assert_eq!(decoded.ir.model.pcurves[0].geometry, geometry);
        assert_eq!(decoded.ir.model.bodies.len(), 1);
        assert!(decoded
            .report
            .losses
            .iter()
            .all(|loss| !loss.message.contains("has no decoded surface or 2D curve")));
    }
}

#[test]
fn decode_maps_a_two_dimensional_polyline_to_a_pcurve_nurbs() {
    use cadmpeg_ir::geometry::PcurveGeometry;
    use cadmpeg_ir::math::Point2;

    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#52=DIRECTION('',(1.,0.));",
            "#52=CARTESIAN_POINT('',(1.,2.));",
        )
        .replace("#53=VECTOR('',#52,1.);", "#53=CARTESIAN_POINT('',(3.,2.));")
        .replace("#54=LINE('',#51,#53);", "#54=POLYLINE('',(#51,#52,#53));");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode polyline pcurve");

    assert!(matches!(
        &decoded.ir.model.pcurves[0].geometry,
        PcurveGeometry::Nurbs {
            degree: 1,
            control_points,
            weights: None,
            periodic: false,
            ..
        } if control_points == &[
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 2.0),
            Point2::new(3.0, 2.0),
        ]
    ));
    assert_eq!(decoded.ir.model.bodies.len(), 1);
}

#[test]
fn planar_pcurve_coordinates_follow_the_document_length_unit() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("SI_UNIT(.MILLI.,.METRE.)", "SI_UNIT(.CENTI.,.METRE.)");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode non-millimetre planar pcurve");

    let pcurve = decoded
        .ir
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == "step:data:pcurve#56")
        .expect("planar pcurve");
    assert!(matches!(
        pcurve.geometry,
        cadmpeg_ir::geometry::PcurveGeometry::Line { direction, .. }
            if (direction.u - 10.0).abs() < 1.0e-12
    ));
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn unsupported_optional_pcurve_does_not_discard_valid_topology() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#54=LINE('',#51,#53);", "#54=UNSUPPORTED_CURVE('',#51);");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode sheet with unsupported optional pcurve");

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(decoded.ir.model.faces.len(), 1);
    assert_eq!(decoded.ir.model.edges.len(), 3);
    assert!(decoded
        .ir
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.pcurves.is_empty()));
    assert!(decoded.report.losses.iter().any(|loss| loss
        .message
        .contains("PCURVE #56 has no decoded surface or 2D curve")));
    assert!(decoded
        .report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("conflicts with decoded topology")));
}

#[test]
fn unsupported_mandatory_carriers_preserve_topology_as_unknown() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#16=LINE('',#3,#13);", "#16=UNSUPPORTED_CURVE('',#3);")
        .replace("#28=PLANE('',#27);", "#28=UNSUPPORTED_SURFACE('',#27);");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode sheet with unknown mandatory carriers");

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(decoded.ir.model.faces.len(), 1);
    assert_eq!(decoded.ir.model.edges.len(), 3);
    assert!(matches!(
        decoded
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == "step:data:curve#16")
            .map(|curve| &curve.geometry),
        Some(CurveGeometry::Unknown { record: Some(record) })
            if record.as_str() == "step:data:unsupported_curve#16"
    ));
    assert!(matches!(
        decoded
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == "step:data:surface#28")
            .map(|surface| &surface.geometry),
        Some(SurfaceGeometry::Unknown { record: Some(record) })
            if record.as_str() == "step:data:unsupported_surface#28"
    ));
    assert!(decoded
        .report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("conflicts with decoded topology")));
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn unsupported_surface_carrier_on_face_surface_preserves_topology_as_unknown() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#28=PLANE('',#27);", "#28=UNSUPPORTED_SURFACE('',#27);")
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#29=FACE_SURFACE('',(#26),#28,.T.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode FACE_SURFACE with unknown carrier");

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert!(matches!(
        decoded
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == "step:data:surface#28")
            .map(|surface| &surface.geometry),
        Some(SurfaceGeometry::Unknown { record: Some(record) })
            if record.as_str() == "step:data:unsupported_surface#28"
    ));
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn failed_mandatory_point_root_remains_opaque_and_unbound() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#3=CARTESIAN_POINT('',(0.,0.,0.));",
            "#3=UNSUPPORTED_POINT('',(0.,0.,0.));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode source with unsupported mandatory vertex point");

    assert!(decoded.ir.model.bodies.is_empty());
    let unknowns = decoded
        .ir
        .native_unknowns("step")
        .expect("STEP unknown arena");
    assert!(unknowns
        .iter()
        .any(|record| record.id.0 == "step:data:unsupported_point#3"));
    assert!(unknowns
        .iter()
        .any(|record| record.id.0 == "step:data:shell_based_surface_model#31"));
    assert!(decoded.report.losses.iter().any(|loss| loss
        .message
        .contains("STEP topology root #31 rejected: vertex point #3")));
}

#[test]
fn failed_void_shell_does_not_commit_the_outer_brep() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=BREP_WITH_VOIDS('',#30,(#34));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',#30,.F.);",
            "#33=OPEN_SHELL('',(#29));\n#34=OPEN_SHELL('',(#99));\n#99=UNSUPPORTED_FACE('',());",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode BREP with invalid void shell");

    assert!(decoded.ir.model.bodies.is_empty());
    assert!(decoded
        .ir
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.0 == "step:data:brep_with_voids#31"));
    assert!(decoded.report.losses.iter().any(|loss| loss
        .message
        .contains("STEP topology root #31 rejected: face carrier #99")));
}

#[test]
fn disconnected_edge_loop_is_not_committed() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#19=EDGE_CURVE('',#6,#7,#57,.T.);",
            "#19=EDGE_CURVE('',#6,#8,#57,.T.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode disconnected edge loop");

    assert!(decoded.ir.model.bodies.is_empty());
    assert!(decoded
        .ir
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.0 == "step:data:shell_based_surface_model#31"));
    assert!(decoded
        .report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("conflicts with decoded topology")));
}

#[test]
fn single_edge_loop_must_close_at_its_endpoint() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#25=EDGE_LOOP('',(#22,#23,#24));",
            "#25=EDGE_LOOP('',(#22));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode open single-edge loop");

    assert!(decoded.ir.model.bodies.is_empty());
    assert!(decoded.report.losses.iter().any(|loss| loss
        .message
        .contains("STEP topology root #31 rejected: edge loop continuity #25")));
}

#[test]
fn seam_edge_preserves_its_explicit_pcurve_reference() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#22=ORIENTED_EDGE('',*,*,#19,.T.);",
            "#22=SEAM_EDGE('',*,*,#19,.T.,#56);",
        )
        .replace(
            "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
            "#57=SEAM_CURVE('',#16,(#56),.PCURVE_S1.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode seam edge");

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert!(decoded.ir.model.coedges.iter().any(|coedge| {
        coedge
            .pcurves
            .iter()
            .any(|use_| use_.pcurve.as_str() == "step:data:pcurve#56")
    }));
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn surface_curve_without_a_basis_keeps_a_curve_less_edge_and_reports_loss() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#16=LINE('',#3,#13);", "#16=UNSUPPORTED_CURVE('',());")
        .replace(
            "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
            "#57=SURFACE_CURVE('',*,(#56),.PCURVE_S1.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode surface curve without basis");

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert!(decoded
        .ir
        .model
        .edges
        .iter()
        .find(|edge| edge.id.as_str() == "step:data:edge#19")
        .is_some_and(|edge| edge.curve.is_none()));
    assert!(decoded.report.losses.iter().any(|loss| {
        loss.message
            .contains("STEP edge curve #19: surface-curve #57 has no resolvable basis")
    }));
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn subedge_inherits_parent_edge_geometry_without_losing_topology() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#19=EDGE_CURVE('',#6,#7,#57,.T.);",
            "#19=(EDGE('',#6,#7) SUBEDGE('',#58));",
        )
        .replace(
            "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
            "#57=SURFACE_CURVE('',#18,(#56),.PCURVE_S1.);\n#58=EDGE_CURVE('',#6,#7,#18,.F.);",
        )
        .replace(
            "#20=EDGE_CURVE('',#7,#8,#17,.T.);",
            "#20=EDGE_CURVE('',#7,#8,#16,.T.);",
        )
        .replace(
            "#21=EDGE_CURVE('',#8,#6,#18,.T.);",
            "#21=EDGE_CURVE('',#8,#6,#17,.T.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode subedge");

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert!(decoded.ir.model.edges.iter().any(|edge| {
        edge.id.as_str() == "step:data:edge#19"
            && edge
                .curve
                .as_ref()
                .is_some_and(|curve| curve.as_str() == "step:data:curve#18")
    }));
    assert!(decoded
        .ir
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .all(|record| record.id.0 != "step:data:subedge#19"));
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn oriented_face_subtype_composes_face_orientation() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#30=OPEN_SHELL('',(#29));", "#30=OPEN_SHELL('',(#34));")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));\n#34=ORIENTED_FACE('',#29,.F.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode oriented face");

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(decoded.ir.model.faces.len(), 1);
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_oriented_open_shell_preserves_shell_sense() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',#30,.F.);",
            "#33=(OPEN_SHELL('',(#29)) ORIENTED_OPEN_SHELL('',#30,.F.));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex oriented shell");

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(
        decoded.ir.model.faces[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn subface_subtype_reuses_parent_surface_and_own_bounds() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#30=OPEN_SHELL('',(#29));", "#30=OPEN_SHELL('',(#34));")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));\n#34=SUBFACE('',(#26),#29);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode subface");

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(decoded.ir.model.faces.len(), 1);
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn shell_based_wireframe_model_owns_wire_shell_edges() {
    use cadmpeg_ir::topology::BodyKind;

    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=SHELL_BASED_WIREFRAME_MODEL('',(#33));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',#30,.F.);",
            "#33=WIRE_SHELL('',(#25));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode shell-based wireframe model");

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(decoded.ir.model.bodies[0].kind, BodyKind::Wire);
    assert_eq!(decoded.ir.model.edges.len(), 3);
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn shell_based_wireframe_model_retains_vertex_shells() {
    use cadmpeg_ir::topology::BodyKind;

    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=SHELL_BASED_WIREFRAME_MODEL('',(#33));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',#30,.F.);",
            "#33=VERTEX_SHELL('',#34);\n#34=VERTEX_LOOP('',#6);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode vertex shell");

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(decoded.ir.model.bodies[0].kind, BodyKind::Wire);
    assert_eq!(decoded.ir.model.shells[0].free_vertices.len(), 1);
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn connected_edge_sub_set_is_accepted_as_a_wire_boundary() {
    use cadmpeg_ir::topology::BodyKind;

    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=EDGE_BASED_WIREFRAME_MODEL('',(#33));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',#30,.F.);",
            "#33=CONNECTED_EDGE_SUB_SET('',(#19,#20,#21),#34);\n#34=CONNECTED_EDGE_SET('',(#19,#20,#21));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode connected edge sub set");

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(decoded.ir.model.bodies[0].kind, BodyKind::Wire);
    assert_eq!(decoded.ir.model.edges.len(), 3);
    assert!(!decoded
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("has no resolvable parent")));
    assert!(decoded
        .ir
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.0 == "step:data:connected_edge_set#34"));
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn connected_edge_sub_set_keeps_topology_when_parent_is_invalid() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=EDGE_BASED_WIREFRAME_MODEL('',(#33));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',#30,.F.);",
            "#33=CONNECTED_EDGE_SUB_SET('',(#19,#20,#21),#34);\n#34=UNSUPPORTED_SET('',());",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode subset with invalid parent");

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert!(decoded
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("parent #34 does not resolve")));
    assert!(decoded
        .ir
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.0 == "step:data:connected_edge_sub_set#33"));
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn connected_face_sub_set_validates_and_uses_its_own_members() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=FACE_BASED_SURFACE_MODEL('',(#34));",
        )
        .replace(
            "#30=OPEN_SHELL('',(#29));",
            "#30=CONNECTED_FACE_SET('',(#29));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',#30,.F.);",
            "#34=CONNECTED_FACE_SUB_SET('',(#29),#30);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode connected face subset");

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(decoded.ir.model.faces.len(), 1);
    assert!(!decoded
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("CONNECTED_FACE_SUB_SET #34")));
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn connected_edge_set_resolves_direct_oriented_and_seam_members() {
    use cadmpeg_ir::topology::BodyKind;

    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=EDGE_BASED_WIREFRAME_MODEL('',(#70));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',#30,.F.);",
            "#70=CONNECTED_EDGE_SET('',(#71,#72,#73));\n#71=ORIENTED_EDGE('',*,*,#19,.F.);\n#72=SEAM_EDGE('',*,*,#20,.T.,#56);\n#73=ORIENTED_EDGE('',*,*,#21,.T.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode direct oriented and seam edge members");

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(decoded.ir.model.bodies[0].kind, BodyKind::Wire);
    assert_eq!(decoded.ir.model.edges.len(), 3);
    let reversed = decoded
        .ir
        .model
        .edges
        .iter()
        .find(|edge| edge.id.as_str().starts_with("step:data:edge#71-"))
        .expect("oriented edge carrier");
    assert!(reversed.start.as_str().contains("vertex#7"));
    assert!(reversed.end.as_str().contains("vertex#6"));
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn shared_edge_wire_model_marks_every_representation_typed() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=EDGE_BASED_WIREFRAME_MODEL('',(#70));\n#70=CONNECTED_EDGE_SET('',(#19,#20,#21));\n#71=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#31),#2);",
        )
        .replace("#33=ORIENTED_OPEN_SHELL('',#30,.F.);", "#33=OPEN_SHELL('',(#29));");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode shared wire model representations");

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert!(!decoded
        .ir
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| { record.id.0 == "step:data:manifold_surface_shape_representation#71" }));
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_edge_and_oriented_edge_instances_use_named_attributes() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#19=EDGE_CURVE('',#6,#7,#57,.T.);",
            "#19=(EDGE('',#6,#7) EDGE_CURVE('',#57,.T.));",
        )
        .replace(
            "#22=ORIENTED_EDGE('',*,*,#19,.T.);",
            "#22=(EDGE('',*,*) ORIENTED_EDGE('',#19,.T.));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex edge instances");

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(decoded.ir.model.edges.len(), 3);
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_advanced_face_uses_its_explicit_surface_carrier() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#28=PLANE('',#27);", "#28=CYLINDRICAL_SURFACE('',#27,5.);")
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#29=(FACE('',(#26)) FACE_SURFACE('',#28,.T.) ADVANCED_FACE());",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex advanced face");

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert!(decoded.ir.model.surfaces.iter().any(|surface| {
        surface.id.as_str() == "step:data:surface#28"
            && matches!(surface.geometry, SurfaceGeometry::Cylinder { .. })
    }));
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_vertex_point_instances_retain_their_point_carriers() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#6=VERTEX_POINT('',#3);",
            "#6=(VERTEX('',*) VERTEX_POINT('',#3));",
        )
        .replace(
            "#7=VERTEX_POINT('',#4);",
            "#7=(VERTEX('',*) VERTEX_POINT('',#4));",
        )
        .replace(
            "#8=VERTEX_POINT('',#5);",
            "#8=(VERTEX('',*) VERTEX_POINT('',#5));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex vertex points");

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(decoded.ir.model.vertices.len(), 3);
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_a_sheet_from_a_geometric_surface_set() {
    use cadmpeg_ir::topology::BodyKind;

    let bytes = include_bytes!("../tests/fixtures/ap242_geometric_set.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode geometric surface set");

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert!(result.ir.model.faces[0].loops.is_empty());
    assert_eq!(
        result.ir.model.faces[0].surface.as_str(),
        "step:data:surface#11"
    );
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn geometric_surface_representation_salvages_valid_sibling_sets() {
    let source = String::from_utf8(
        include_bytes!("../tests/fixtures/ap242_geometric_set.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#13=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#12),#2);",
        "#13=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#12,#99),#2);\n#99=UNSUPPORTED_SET('',());",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode geometric set with malformed sibling");

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(decoded.ir.model.faces.len(), 1);
    assert!(decoded
        .report
        .losses
        .iter()
        .any(|loss| { loss.message.contains("skipped non-set member #99") }));
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn geometric_bounded_surface_representation_reaches_its_product() {
    let source = String::from_utf8(
        include_bytes!("../tests/fixtures/ap242_geometric_set.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#20=PRODUCT('P','bounded part','',());\n#21=PRODUCT_DEFINITION_FORMATION('','',#20);\n#22=APPLICATION_CONTEXT('mechanical design');\n#23=PRODUCT_DEFINITION_CONTEXT('part definition',#22,'design');\n#24=PRODUCT_DEFINITION('part','',#21,#23);\n#25=PRODUCT_DEFINITION_SHAPE('','',#24);\n#26=SHAPE_DEFINITION_REPRESENTATION(#25,#13);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode product-bound bounded surface");

    assert_eq!(decoded.ir.model.product_definitions.len(), 1);
    assert_eq!(decoded.ir.model.product_definitions[0].bodies.len(), 1);
    assert_eq!(
        decoded.ir.model.product_definitions[0].bodies[0].as_str(),
        "step:data:body#13"
    );
}

#[test]
fn aliased_topology_root_reuses_the_committed_body_identity() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#70=SHELL_BASED_SURFACE_MODEL('',(#33));\n#71=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#70),#2);\n#72=PRODUCT('P','alias part','',());\n#73=PRODUCT_DEFINITION_FORMATION('','',#72);\n#74=APPLICATION_CONTEXT('mechanical design');\n#75=PRODUCT_DEFINITION_CONTEXT('part definition',#74,'design');\n#76=PRODUCT_DEFINITION('part','',#73,#75);\n#77=PRODUCT_DEFINITION_SHAPE('','',#76);\n#78=SHAPE_DEFINITION_REPRESENTATION(#77,#71);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode aliased topology root");

    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(decoded.ir.model.product_definitions[0].bodies.len(), 1);
    assert_eq!(
        decoded.ir.model.product_definitions[0].bodies[0].as_str(),
        "step:data:body#31"
    );
    assert!(!decoded
        .ir
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.0 == "step:data:shell_based_surface_model#70"));
}

#[test]
fn reused_shell_in_a_distinct_root_gets_a_new_owner_scope() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#70=SHELL_BASED_SURFACE_MODEL('',(#33,#71));\n#71=OPEN_SHELL('',(#29));\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode reused shell root");

    assert_eq!(
        decoded.ir.model.bodies.len(),
        3,
        "{:#?}",
        decoded.report.losses
    );
    assert_eq!(
        decoded
            .ir
            .model
            .shells
            .iter()
            .filter(|shell| shell.id.as_str().contains("root-70"))
            .count(),
        2
    );
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn reader_recovers_a_valid_solid_from_writer_output() {
    use cadmpeg_ir::topology::BodyKind;

    let source = unit_cube();
    let mut bytes = Vec::new();
    write_step(&source, &mut bytes, &StepWriteOptions::default()).unwrap();
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode generated cube STEP");

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.bodies[0].kind, BodyKind::Solid);
    assert_eq!(result.ir.model.faces.len(), 6);
    assert_eq!(result.ir.model.edges.len(), 12);
    assert_eq!(result.ir.model.vertices.len(), 8);
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn writer_round_trips_rigid_body_placements() {
    let mut ir = unit_cube();
    ir.model.bodies[0].transform = Some(cadmpeg_ir::transform::Transform {
        rows: [
            [0.0, -1.0, 0.0, 15.0],
            [1.0, 0.0, 0.0, 4.0],
            [0.0, 0.0, 1.0, 2.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    });
    let options = StepWriteOptions {
        unsupported: StepUnsupportedPolicy::Reject,
        ..StepWriteOptions::default()
    };
    let mut output = Vec::new();
    write_step(&ir, &mut output, &options).expect("write placed body");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode placed body");
    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(
        decoded.ir.model.bodies[0].transform,
        ir.model.bodies[0].transform
    );
}

#[test]
fn writer_declares_each_supported_target_schema_exactly() {
    for schema in [
        StepSchema::Ap203Edition1,
        StepSchema::Ap203Edition2,
        StepSchema::Ap214,
        StepSchema::Ap242Edition1,
        StepSchema::Ap242Edition2,
        StepSchema::Ap242Edition3,
    ] {
        let options = StepWriteOptions {
            schema,
            unsupported: StepUnsupportedPolicy::Reject,
            ..StepWriteOptions::default()
        };
        let mut bytes = Vec::new();
        write_step(&unit_cube(), &mut bytes, &options).expect("write target schema");
        let text = std::str::from_utf8(&bytes).expect("ASCII STEP output");
        assert!(text.contains(&format!("FILE_SCHEMA(('{}'));", schema.file_schema())));
        StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode target-schema output");
    }
}

#[test]
fn ap242_writer_round_trips_indexed_tessellation_and_exact_body_link() {
    let mut ir = unit_cube();
    ir.model
        .tessellations
        .push(cadmpeg_ir::tessellation::Tessellation {
            faces: Vec::new(),
            chordal_deflection: None,
            id: "mesh-0".into(),
            body: Some(ir.model.bodies[0].id.clone()),
            source_object: None,
            vertices: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            triangles: vec![[0, 1, 2], [2, 1, 0]],
            strip_lengths: Vec::new(),
            normals: vec![Vector3::new(0.0, 0.0, 1.0); 3],
            channels: Vec::new(),
        });
    let options = StepWriteOptions {
        schema: StepSchema::Ap242Edition3,
        ..StepWriteOptions::default()
    };
    let mut bytes = Vec::new();
    let report = write_step(&ir, &mut bytes, &options).expect("write AP242 tessellation");
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains("tessellation")));
    let text = String::from_utf8(bytes.clone()).expect("STEP text");
    assert_eq!(text.matches("TRIANGULATED_FACE(").count(), 1);

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP242 tessellation");
    assert_eq!(decoded.ir.model.tessellations.len(), 1);
    let mesh = &decoded.ir.model.tessellations[0];
    assert_eq!(mesh.vertices.len(), 3);
    assert_eq!(mesh.triangles, [[0, 1, 2], [2, 1, 0]]);
    assert_eq!(mesh.normals.len(), 3);
    assert!(mesh.body.is_some());
}

#[test]
fn step_color_assets_round_trip_names_and_tessellation_targets_strictly() {
    let cases: [(&[u8], StepSchema, &[&str]); 2] = [
        (
            include_bytes!("../tests/fixtures/ap214_sheet.p21"),
            StepSchema::Ap214,
            &["override red", "blue green"],
        ),
        (
            include_bytes!("../tests/fixtures/ap242_tessellation.p21"),
            StepSchema::Ap242Edition3,
            &["mesh green"],
        ),
    ];
    for (source, schema, expected_names) in cases {
        let ir = StepCodec::default()
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("decode styled STEP")
            .ir;
        let mut bytes = Vec::new();
        write_step(
            &ir,
            &mut bytes,
            &StepWriteOptions {
                schema,
                unsupported: StepUnsupportedPolicy::Reject,
                ..StepWriteOptions::default()
            },
        )
        .expect("strict styled STEP write");
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode written styled STEP");
        let names = decoded
            .ir
            .model
            .appearances
            .iter()
            .filter_map(|appearance| appearance.name.as_deref())
            .collect::<std::collections::BTreeSet<_>>();
        for expected in expected_names {
            assert!(names.contains(expected), "missing color name {expected}");
        }
        if expected_names == ["mesh green"] {
            assert!(decoded.ir.model.appearance_bindings.iter().any(|binding| {
                matches!(
                    binding.target,
                    cadmpeg_ir::appearance::AppearanceTarget::Tessellation(_)
                )
            }));
        }
    }
}

#[test]
fn writer_round_trips_product_body_ownership() {
    let mut ir = unit_cube();
    let product = cadmpeg_ir::ids::ProductDefinitionId("product-0".into());
    ir.model
        .product_definitions
        .push(cadmpeg_ir::products::ProductDefinition {
            id: product.clone(),
            kind: cadmpeg_ir::products::ProductDefinitionKind::Part,
            source_name: Some("Cube part".into()),
            label: Some("Cube part".into()),
            description: None,
            part_number: Some("PART-001".into()),
            bom_properties: std::collections::BTreeMap::default(),
            bodies: vec![ir.model.bodies[0].id.clone()],
            native_ref: None,
        });
    ir.model.occurrences.push(cadmpeg_ir::products::Occurrence {
        id: cadmpeg_ir::ids::OccurrenceId("root-0".into()),
        prototype: cadmpeg_ir::products::PrototypeReference::Local {
            definition: product,
        },
        parent: cadmpeg_ir::products::OccurrenceParent::Root,
        ordinal: 0,
        transform: cadmpeg_ir::transform::Transform::identity(),
        prototype_transform: cadmpeg_ir::transform::Transform::identity(),
        scale: [1.0; 3],
        name: Some("Cube root".into()),
        linked_subelements: Vec::new(),
        visible: None,
        element_component: None,
        claim_child: None,
        copy_on_change: None,
        copy_on_change_source: None,
        copy_on_change_group: None,
        copy_on_change_touched: None,
        link_transform: None,
        native_ref: None,
    });
    let options = StepWriteOptions {
        schema: StepSchema::Ap242Edition3,
        unsupported: StepUnsupportedPolicy::Reject,
        ..StepWriteOptions::default()
    };
    let mut output = Vec::new();
    write_step(&ir, &mut output, &options).expect("write product-owned body");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode product-owned body");
    assert_eq!(decoded.ir.model.product_definitions.len(), 1);
    assert_eq!(
        decoded.ir.model.product_definitions[0]
            .part_number
            .as_deref(),
        Some("PART-001")
    );
    assert_eq!(decoded.ir.model.product_definitions[0].bodies.len(), 1);
    assert_eq!(decoded.ir.model.occurrences.len(), 1);
}

#[test]
fn writer_round_trips_edge_based_wire_bodies() {
    let mut ir = unit_cube();
    let edge = ir.model.edges[0].clone();
    let curve = edge.curve.clone().expect("cube edge curve");
    ir.model.edges.retain(|candidate| candidate.id == edge.id);
    ir.model.curves.retain(|candidate| candidate.id == curve);
    ir.model
        .vertices
        .retain(|vertex| vertex.id == edge.start || vertex.id == edge.end);
    let point_ids = ir
        .model
        .vertices
        .iter()
        .map(|vertex| vertex.point.clone())
        .collect::<Vec<_>>();
    ir.model
        .points
        .retain(|point| point_ids.contains(&point.id));
    ir.model.coedges.clear();
    ir.model.loops.clear();
    ir.model.faces.clear();
    ir.model.surfaces.clear();
    ir.model.shells.truncate(1);
    ir.model.shells[0].faces.clear();
    ir.model.shells[0].wire_edges = vec![edge.id];
    ir.model.shells[0].free_vertices.clear();
    ir.model.regions.truncate(1);
    ir.model.regions[0].shells = vec![ir.model.shells[0].id.clone()];
    ir.model.bodies.truncate(1);
    ir.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Wire;
    ir.model.bodies[0].regions = vec![ir.model.regions[0].id.clone()];

    let mut output = Vec::new();
    write_step(&ir, &mut output, &StepWriteOptions::default()).expect("write wire body");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode wire body");
    assert_eq!(decoded.ir.model.bodies.len(), 1);
    assert_eq!(
        decoded.ir.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Wire
    );
    assert_eq!(decoded.ir.model.edges.len(), 1);
    assert_eq!(decoded.ir.model.shells[0].wire_edges.len(), 1);
    let validation = cadmpeg_ir::validate(&decoded.ir, decoded.report.losses);
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn writer_round_trips_standalone_points_and_curves() {
    let mut ir = unit_cube();
    ir.model.curves.truncate(1);
    ir.model.surfaces.clear();
    ir.model.bodies.clear();
    ir.model.regions.clear();
    ir.model.shells.clear();
    ir.model.faces.clear();
    ir.model.loops.clear();
    ir.model.coedges.clear();
    ir.model.edges.clear();
    ir.model.vertices.clear();

    let mut output = Vec::new();
    write_step(&ir, &mut output, &StepWriteOptions::default()).expect("write standalone geometry");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode standalone geometry");
    assert_eq!(decoded.ir.model.curves.len(), 1);
    assert_eq!(decoded.ir.model.points.len(), ir.model.points.len());
    assert!(decoded.ir.model.bodies.is_empty());
}

#[test]
fn decode_builds_product_occurrences_with_relative_placement() {
    use cadmpeg_ir::products::OccurrenceParent;

    let bytes = include_bytes!("../tests/fixtures/ap242_assembly.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP242 assembly");

    assert_eq!(result.ir.model.product_definitions.len(), 2);
    assert_eq!(result.ir.model.occurrences.len(), 2);
    let child = result
        .ir
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("Placed child"))
        .unwrap();
    assert!(matches!(child.parent, OccurrenceParent::Occurrence { .. }));
    assert_eq!(child.transform.rows[0][3], 25.0);
    assert_eq!(child.transform.rows[1][3], 0.0);
    assert_eq!(child.transform.rows[2][3], 0.0);
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let options = StepWriteOptions {
        schema: StepSchema::Ap242Edition3,
        ..StepWriteOptions::default()
    };
    let mut output = Vec::new();
    write_step(&result.ir, &mut output, &options).expect("write product graph");
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode written product graph");
    assert_eq!(roundtrip.ir.model.product_definitions.len(), 2);
    assert_eq!(roundtrip.ir.model.occurrences.len(), 2);
    let child = roundtrip
        .ir
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("Placed child"))
        .expect("round-tripped child occurrence");
    assert!(matches!(child.parent, OccurrenceParent::Occurrence { .. }));
    assert_eq!(child.transform.rows[0][3], 25.0);
}

#[test]
fn decode_builds_occurrence_placement_from_mapped_item() {
    let bytes = include_bytes!("../tests/fixtures/ap242_mapped_assembly.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode mapped-item assembly");

    let child = result
        .ir
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("Mapped child"))
        .unwrap();
    assert_eq!(child.transform.rows[0][3], 40.0);
    assert_eq!(child.transform.rows[1][3], 5.0);
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_transfers_ap242_one_based_tessellation_indices() {
    let bytes = include_bytes!("../tests/fixtures/ap242_tessellation.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP242 tessellation");

    assert_eq!(result.ir.model.tessellations.len(), 2);
    assert_eq!(result.ir.model.bodies.len(), 1);
    let mesh = &result.ir.model.tessellations[0];
    assert_eq!(mesh.vertices.len(), 3);
    assert_eq!(mesh.vertices[1].x, 10.0);
    assert_eq!(mesh.triangles, [[0, 1, 2]]);
    assert_eq!(mesh.normals.len(), 3);
    assert_eq!(
        mesh.body.as_ref().map(cadmpeg_ir::ids::BodyId::as_str),
        Some("step:data:body#38")
    );
    let complex = result
        .ir
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.ends_with("#7"))
        .unwrap();
    assert_eq!(complex.triangles, [[0, 1, 2], [2, 1, 3], [0, 1, 3]]);
    assert_eq!(complex.vertices[0], Point3::new(10.0, 10.0, 0.0));
    assert_eq!(complex.normals.len(), 4);
    assert_eq!(complex.normals[0].x, 1.0);
    assert!(result
        .ir
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Tessellation(_)
        )));
    assert!(result
        .report
        .notes
        .iter()
        .any(|note| note
            == "geometric validation surface area triangle sheet: expected 50, tessellation approximation 50"));
    assert!(result.report.notes.iter().any(|note| note.starts_with(
        "geometric validation centroid triangle centroid: expected (3.333333333333333,3.333333333333333,0), tessellation approximation distance"
    )));
    assert!(result.report.notes.iter().any(
        |note| note == "geometric validation volume open sheet volume: expected 0, tessellation approximation 0"
    ));
    assert!(!result.report.losses.iter().any(|loss| loss
        .message
        .contains("does not match transferred tessellation")));
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_transfers_ap242_semantic_pmi() {
    use cadmpeg_ir::pmi::{GeometricToleranceKind, PmiDefinition, PmiQuantity};

    let bytes = include_bytes!("../tests/fixtures/ap242_semantic_pmi.p21");
    let mut result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP242 semantic PMI");

    assert_eq!(result.ir.model.pmi.len(), 5);
    assert!(!result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("PLUS_MINUS_TOLERANCE #26")));
    let dimension = result
        .ir
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("width"))
        .unwrap();
    let PmiDefinition::Dimension {
        nominal,
        lower_deviation,
        upper_deviation,
        ref limits_and_fits,
        ..
    } = dimension.definition
    else {
        panic!("width is not a dimension")
    };
    assert_eq!(nominal.unwrap().value, 12.0);
    assert_eq!(lower_deviation.unwrap().value, -0.1);
    assert_eq!(upper_deviation.unwrap().value, 0.2);
    assert!(result.ir.model.pmi.iter().any(|annotation| matches!(
        annotation.definition,
        PmiDefinition::Dimension {
            dimension: cadmpeg_ir::pmi::DimensionKind::Diameter,
            ..
        }
    )));
    let fit = limits_and_fits.as_ref().expect("limits and fits");
    assert_eq!(fit.form_variance, "H");
    assert_eq!(fit.grade, "7");
    assert_eq!(fit.source, "ISO 286");
    let tolerance = result
        .ir
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("surface flatness"))
        .unwrap();
    let datum_system = result
        .ir
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("primary system"))
        .expect("datum system");
    assert!(matches!(
        &datum_system.definition,
        PmiDefinition::DatumSystem { references }
            if references.len() == 1
                && references[0].precedence == 1
                && references[0].modifiers == ["maximum_material_requirement", "distance:0.2"]
    ));
    assert!(matches!(
        tolerance.definition,
        PmiDefinition::GeometricTolerance {
            tolerance: GeometricToleranceKind::Flatness,
            magnitude: cadmpeg_ir::PmiValue {
                value: 0.05,
                quantity: PmiQuantity::Length,
            },
            datum_system: None,
        }
    ));
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    let semantic = dimension.id.clone();
    result.ir.model.pmi.push(cadmpeg_ir::PmiAnnotation {
        id: cadmpeg_ir::ids::PmiId("test:pmi:presentation".into()),
        name: Some("width note".into()),
        targets: Vec::new(),
        definition: PmiDefinition::Presentation {
            text: Some("12 mm".into()),
            placement: Some(cadmpeg_ir::transform::Transform::identity()),
            semantics: vec![semantic],
        },
    });
    let options = StepWriteOptions {
        schema: StepSchema::Ap242Edition3,
        ..StepWriteOptions::default()
    };
    let mut output = Vec::new();
    let report = write_step(&result.ir, &mut output, &options).expect("write semantic PMI");
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains("PMI annotation")));
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode written semantic PMI");
    assert_eq!(roundtrip.ir.model.pmi.len(), 6);
    assert!(roundtrip.ir.model.pmi.iter().any(|annotation| matches!(
        &annotation.definition,
        PmiDefinition::DatumSystem { references }
            if references.len() == 1
                && references[0].modifiers
                    == ["maximum_material_requirement", "distance:0.2"]
    )));
    assert!(roundtrip.ir.model.pmi.iter().any(|annotation| matches!(
        &annotation.definition,
        PmiDefinition::Presentation { semantics, .. } if semantics.len() == 1
    )));
    assert!(roundtrip.ir.model.pmi.iter().any(|annotation| matches!(
        annotation.definition,
        PmiDefinition::Dimension {
            nominal: Some(cadmpeg_ir::PmiValue {
                value: 12.0,
                quantity: PmiQuantity::Length,
            }),
            lower_deviation: Some(cadmpeg_ir::PmiValue { value: -0.1, .. }),
            upper_deviation: Some(cadmpeg_ir::PmiValue { value: 0.2, .. }),
            ..
        }
    )));
}

#[test]
fn decode_transfers_ap242_presentation_pmi() {
    use cadmpeg_ir::pmi::PmiDefinition;

    let bytes = include_bytes!("../tests/fixtures/ap242_presentation_pmi.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP242 presentation PMI");

    assert_eq!(result.ir.model.pmi.len(), 1);
    let PmiDefinition::Presentation {
        ref text,
        ref placement,
        ..
    } = result.ir.model.pmi[0].definition
    else {
        panic!("annotation occurrence is not presentation PMI")
    };
    assert_eq!(text.as_deref(), Some("inspect surface"));
    let transform = placement.as_ref().unwrap();
    assert_eq!(transform.rows[0][3], 10.0);
    assert_eq!(transform.rows[1][3], 20.0);
    assert_eq!(transform.rows[2][3], 30.0);
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let options = StepWriteOptions {
        schema: StepSchema::Ap242Edition3,
        ..StepWriteOptions::default()
    };
    let mut output = Vec::new();
    let report = write_step(&result.ir, &mut output, &options).expect("write presentation PMI");
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains("PMI annotation")));
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode written presentation PMI");
    assert_eq!(roundtrip.ir.model.pmi.len(), 1);
    assert!(matches!(
        &roundtrip.ir.model.pmi[0].definition,
        PmiDefinition::Presentation {
            text: Some(text),
            placement: Some(transform),
            ..
        } if text == "inspect surface"
            && transform.rows[0][3] == 10.0
            && transform.rows[1][3] == 20.0
            && transform.rows[2][3] == 30.0
    ));
}

fn export(ir: &CadIr) -> String {
    let mut buf = Vec::new();
    write_step(ir, &mut buf, &StepWriteOptions::default()).expect("write");
    String::from_utf8(buf).expect("utf8")
}

fn decode_inline(records: &str) -> cadmpeg_ir::codec::DecodeResult {
    let source = format!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('test'),'2;1');\nFILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');\nFILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));\nENDSEC;\nDATA;\n{records}\nENDSEC;\nEND-ISO-10303-21;\n"
    );
    StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode inline STEP")
}

#[test]
fn geometric_set_owns_catias_composite_trimmed_curve_chain() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(17.,23.,13.));
#2=CARTESIAN_POINT('',(21.8769469654,17.9785073637,13.));
#3=CARTESIAN_POINT('',(21.8769469654,28.0214926363,13.));
#4=DIRECTION('',(0.,0.,1.));
#5=AXIS2_PLACEMENT_3D('',#1,#4,$);
#6=CIRCLE('',#5,7.);
#7=TRIMMED_CURVE('',#6,(#2),(#3),.T.,.CARTESIAN.);
#8=COMPOSITE_CURVE_SEGMENT(.DISCONTINUOUS.,.T.,#7);
#9=COMPOSITE_CURVE('',(#8),.U.);
#10=GEOMETRIC_SET('NONE',(#9));
#11=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#10),#12);
#12=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );

    let composite = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "step:data:curve#9")
        .expect("composite curve");
    let source = composite
        .source_object
        .as_ref()
        .expect("geometric-set owner");
    assert_eq!(source.format, "step");
    assert_eq!(source.object_id, "#9");
    assert_eq!(source.name, None);

    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn deferred_curve_dependencies_resolve_independent_of_record_order() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=VECTOR('',#2,1.);
#4=LINE('',#1,#3);
#5=OFFSET_CURVE_3D('',#7,1.,.F.,#2);
#6=GEOMETRIC_SET('',(#5));
#7=OFFSET_CURVE_3D('',#4,2.,.F.,#2);
#8=SHAPE_REPRESENTATION('',(#6),#9);
#9=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );

    assert!(result
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve.id.as_str() == "step:data:curve#5"));
    assert!(result
        .ir
        .model
        .curves
        .iter()
        .any(|curve| curve.id.as_str() == "step:data:curve#7"));
    assert!(result.report.losses.iter().all(|loss| {
        !loss
            .message
            .contains("OFFSET_CURVE_3D #5 has no decoded basis curve")
    }));
}

#[test]
fn deferred_surface_dependencies_resolve_independent_of_record_order() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=PLANE('',#4);
#6=OFFSET_SURFACE('',#7,1.,.F.);
#7=OFFSET_SURFACE('',#5,2.,.F.);
#8=GEOMETRIC_SET('',(#6));
#9=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#8),#10);
#10=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );

    assert!(result
        .ir
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.as_str() == "step:data:surface#6"));
    assert!(result
        .ir
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.as_str() == "step:data:surface#7"));
    assert_eq!(result.ir.model.bodies.len(), 1);
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn conical_surface_accepts_a_finite_zero_half_angle() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=CONICAL_SURFACE('',#4,0.,0.);
#6=GEOMETRIC_SET('',(#5));
#7=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#6),#8);
#8=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );

    assert!(result.ir.model.surfaces.iter().any(|surface| {
        matches!(
            surface.geometry,
            cadmpeg_ir::geometry::SurfaceGeometry::Cone { half_angle, .. }
                if half_angle == 0.0
        )
    }));
    assert!(result.report.losses.iter().all(|loss| !loss
        .message
        .contains("CONICAL_SURFACE #5 has invalid geometry")));
}

#[test]
fn catia_cartesian_trim_points_resolve_on_nurbs_curve() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,1.,0.));
#3=CARTESIAN_POINT('',(2.,0.,0.));
#4=B_SPLINE_CURVE_WITH_KNOTS('',2,(#1,#2,#3),.UNSPECIFIED.,.F.,.U.,(3,3),(0.,2.),.UNSPECIFIED.);
#5=TRIMMED_CURVE('',#4,(#1),(#3),.T.,.CARTESIAN.);
#6=COMPOSITE_CURVE_SEGMENT(.DISCONTINUOUS.,.T.,#5);
#7=COMPOSITE_CURVE('',(#6),.U.);
#8=GEOMETRIC_SET('NONE',(#7));
#9=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#8),#10);
#10=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );

    assert_eq!(result.ir.model.curves.len(), 3);
    assert_eq!(result.ir.model.procedural_curves.len(), 1);
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn excessive_nurbs_degree_is_rejected_before_knot_allocation() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,0.,0.));
#3=B_SPLINE_CURVE_WITH_KNOTS('',4294967295,(#1,#2),.UNSPECIFIED.,.F.,.F.,(4294967298),(0.),.UNSPECIFIED.);",
    );
    assert!(result.ir.model.curves.is_empty());
}

#[test]
fn non_finite_tessellation_coordinates_are_rejected() {
    let result = decode_inline(
        "#1=COORDINATES_LIST('',1,((1E400,0.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,1,$,$,((1,1,1)));",
    );
    assert!(result.ir.model.tessellations.is_empty());
}

#[test]
fn mapped_representation_dag_is_memoized() {
    let depth = 32_u64;
    let mut records = String::from(
        "#1=APPLICATION_CONTEXT('');\n\
#2=PRODUCT('p','p','',());\n\
#3=PRODUCT_DEFINITION_FORMATION('','',#2);\n\
#4=PRODUCT_DEFINITION('','',#3,#1);\n\
#5=PRODUCT_DEFINITION_SHAPE('','',#4);\n\
#6=SHAPE_DEFINITION_REPRESENTATION(#5,#100);\n",
    );
    for level in 0..depth {
        let representation = 100 + level;
        let next = representation + 1;
        let map = 1_000 + level;
        let first = 2_000 + level * 2;
        let second = first + 1;
        write!(
            records,
            "#{representation}=SHAPE_REPRESENTATION('',(#{first},#{second}),$);\n\
#{map}=REPRESENTATION_MAP($,#{next});\n\
#{first}=MAPPED_ITEM('',#{map},$);\n\
#{second}=MAPPED_ITEM('',#{map},$);\n"
        )
        .expect("write mapped representation fixture");
    }
    write!(
        records,
        "#{}=SHAPE_REPRESENTATION('',(#9000),$);\n\
#9000=MANIFOLD_SOLID_BREP('',#9001);\n\
#9001=CLOSED_SHELL('',(#9002));\n\
#9002=ADVANCED_FACE('',(#9003),#9004,.T.);\n\
#9003=FACE_OUTER_BOUND('',#9005,.T.);\n\
#9005=VERTEX_LOOP('',#9006);\n\
#9006=VERTEX_POINT('',#9007);\n\
#9007=CARTESIAN_POINT('',(0.,0.,0.));\n\
#9004=PLANE('',#9008);\n\
#9008=AXIS2_PLACEMENT_3D('',#9007,$,$);",
        100 + depth
    )
    .expect("write terminal representation fixture");

    let result = decode_inline(&records);
    assert_eq!(result.ir.model.product_definitions.len(), 1);
    assert_eq!(result.ir.model.product_definitions[0].bodies.len(), 1);
    assert_eq!(
        result.ir.model.product_definitions[0].bodies[0].as_str(),
        "step:data:body#9000"
    );
}

#[test]
fn malformed_zero_partial_pmi_reference_is_non_panicking() {
    let result = decode_inline("#5=();\n#10=ANNOTATION_OCCURRENCE('',(),#5);");
    assert!(result.ir.model.pmi.len() <= 1);
}

#[test]
fn overriding_style_suppresses_the_base_binding() {
    let result = decode_inline(
        "#1=COLOUR_RGB('blue',0.,0.,1.);
#2=PRESENTATION_STYLE_ASSIGNMENT((#1));
#3=COLOUR_RGB('red',1.,0.,0.);
#4=PRESENTATION_STYLE_ASSIGNMENT((#3));
#10=STYLED_ITEM('',(#2),#20);
#11=OVER_RIDING_STYLED_ITEM('',(#4),#20,#10);
#20=SOURCE_ITEM();",
    );
    assert_eq!(result.ir.model.appearance_bindings.len(), 1);
    let binding = &result.ir.model.appearance_bindings[0];
    let appearance = result
        .ir
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.id == binding.appearance)
        .expect("overriding appearance");
    let color = appearance.base_color.expect("override color");
    assert_eq!((color.r, color.g, color.b), (1.0, 0.0, 0.0));
}

#[test]
fn null_style_branch_does_not_suppress_a_sibling_color() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=COLOUR_RGB('red',1.,0.,0.);
#3=PRESENTATION_STYLE_ASSIGNMENT((NULL_STYLE(.NULL.),#2));
#4=STYLED_ITEM('',(#3),#1);",
    );
    assert_eq!(result.ir.model.appearance_bindings.len(), 1);
}

#[test]
fn unresolved_lower_tolerance_does_not_shift_upper_deviation() {
    use cadmpeg_ir::pmi::PmiDefinition;

    let result = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#5=PRODUCT_DEFINITION_SHAPE('','',#99);
#6=SHAPE_ASPECT('feature','',#5,.T.);
#10=DIMENSIONAL_SIZE(#6,'width');
#16=UNRESOLVED_MEASURE();
#17=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.2),#1);
#18=TOLERANCE_VALUE(#16,#17);
#19=PLUS_MINUS_TOLERANCE(#18,#10);
#99=UNRESOLVED_PRODUCT();",
    );
    assert!(result.ir.model.pmi.iter().any(|annotation| matches!(
        annotation.definition,
        PmiDefinition::Dimension {
            lower_deviation: None,
            upper_deviation: Some(cadmpeg_ir::PmiValue { value, .. }),
            ..
        } if (value - 0.2).abs() < 1.0e-12
    )));
}

#[test]
fn typed_pmi_measure_uses_its_explicit_conversion_unit() {
    use cadmpeg_ir::pmi::PmiDefinition;

    let result = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));
#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#6=DATUM_FEATURE('feature','',#5,.T.);
#10=DIMENSIONAL_SIZE(#6,'width');
#30=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#1);
#31=(CONVERSION_BASED_UNIT('inch',#30) LENGTH_UNIT() NAMED_UNIT(*));
#13=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(5.0),#31);
#14=SHAPE_DIMENSION_REPRESENTATION('width value',(#13),#2);
#15=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#10,#14);
#99=UNRESOLVED_PRODUCT();",
    );
    assert!(result.ir.model.pmi.iter().any(|annotation| matches!(
        annotation.definition,
        PmiDefinition::Dimension {
            nominal: Some(cadmpeg_ir::PmiValue { value, .. }),
            ..
        } if (value - 127.0).abs() < 1.0e-12
    )));
}

#[test]
fn repeated_subassembly_instances_each_receive_the_subtree() {
    use cadmpeg_ir::products::{OccurrenceParent, PrototypeReference};

    let result = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('P','parent','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('parent','',#4,#5);
#7=PRODUCT('S','subassembly','',(#2));
#8=PRODUCT_DEFINITION_FORMATION('','',#7);
#9=PRODUCT_DEFINITION('subassembly','',#8,#5);
#10=PRODUCT('L','leaf','',(#2));
#11=PRODUCT_DEFINITION_FORMATION('','',#10);
#12=PRODUCT_DEFINITION('leaf','',#11,#5);
#20=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u1','sub one','',#6,#9,$);
#21=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u2','sub two','',#6,#9,$);
#22=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u3','leaf','',#9,#12,$);",
    );
    assert_eq!(result.ir.model.occurrences.len(), 5);
    let subassemblies = result
        .ir
        .model
        .occurrences
        .iter()
        .filter(|occurrence| {
            matches!(
                &occurrence.prototype,
                PrototypeReference::Local { definition }
                    if definition.as_str() == "step:product:product#7"
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(subassemblies.len(), 2);
    for subassembly in subassemblies {
        assert_eq!(
            result
                .ir
                .model
                .occurrences
                .iter()
                .filter(|occurrence| matches!(
                    &occurrence.parent,
                    OccurrenceParent::Occurrence { occurrence: parent }
                        if parent == &subassembly.id
                ))
                .count(),
            1
        );
    }
}

#[test]
fn ap203_specified_source_formations_build_occurrence_tree() {
    let result = decode_inline(
        "#1=APPLICATION_CONTEXT('configuration controlled design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('A','assembly','',(#2));
#4=PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE('','',#3,.NOT_KNOWN.);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('assembly','',#4,#5);
#7=PRODUCT('P','part','',(#2));
#8=PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE('','',#7,.NOT_KNOWN.);
#9=PRODUCT_DEFINITION('part','',#8,#5);
#10=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u1','part instance','',#6,#9,$);",
    );

    assert_eq!(result.ir.model.product_definitions.len(), 2);
    assert_eq!(result.ir.model.occurrences.len(), 2);
    assert!(result
        .ir
        .model
        .occurrences
        .iter()
        .any(|occurrence| matches!(
            &occurrence.prototype,
            cadmpeg_ir::products::PrototypeReference::Local { definition }
                if definition.as_str() == "step:product:product#7"
        )));
    assert!(!result
        .ir
        .native_unknowns("step")
        .unwrap()
        .iter()
        .any(|record| {
            record.id.0.contains("product_definition_formation")
                || record.id.0.contains("next_assembly_usage_occurrence")
        }));
}

#[test]
fn tessellation_geometry_sets_transfer_flag_and_invalid_pnindex_is_rejected() {
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../tests/fixtures/ap242_tessellation.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode tessellation fixture");
    assert!(result.report.geometry_transferred);
    assert!(result
        .ir
        .model
        .tessellations
        .iter()
        .any(|mesh| mesh.id == "step:tessellation:mesh#7" && mesh.body.is_none()));
    assert!(result.report.losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::ReferenceGraphNotClosed
            && loss.message.contains("mesh retained as detached")
    }));

    let malformed = decode_inline(
        "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,$,('bad'),((1,2,3)));",
    );
    assert!(malformed.ir.model.tessellations.is_empty());
    assert!(malformed
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("invalid pnindex")));
}

#[test]
fn shared_tessellation_item_is_not_assigned_to_an_arbitrary_body() {
    let source = String::from_utf8(
        include_bytes!("../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#80=WIRE_SHELL('',(#32));\n#81=SHELL_BASED_WIREFRAME_MODEL('',(#80));\n#82=TESSELLATED_SHELL('shared mesh',(#4),#80);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode shared tessellation item");
    let mesh = decoded
        .ir
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id == "step:tessellation:mesh#4")
        .expect("shared mesh");
    assert!(mesh.body.is_none());
    assert!(
        decoded.report.losses.iter().any(|loss| {
            loss.code == cadmpeg_ir::LossKind::ReferenceGraphNotClosed
                && loss.message.contains("multiple candidate bodies")
        }),
        "{:#?}",
        decoded.report.losses
    );
}

#[test]
fn malformed_complex_strip_does_not_discard_valid_strips() {
    let result = decode_inline(
        "#1=COORDINATES_LIST('',4,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.),(1.,1.,0.)));
#2=COMPLEX_TRIANGULATED_SURFACE_SET('',#1,4,$,$,((1,2),(1,2,3,4)),());",
    );
    assert_eq!(result.ir.model.tessellations.len(), 1);
    assert_eq!(result.ir.model.tessellations[0].triangles.len(), 2);
}

#[test]
fn ap203e1_does_not_emit_invisibility_entities() {
    let mut ir = unit_cube();
    ir.model.bodies[0].visible = Some(false);
    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        &StepWriteOptions {
            schema: StepSchema::Ap203Edition1,
            ..StepWriteOptions::default()
        },
    )
    .unwrap();
    assert!(!String::from_utf8(output).unwrap().contains("INVISIBILITY"));
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains("hidden body visibility")));
}

#[test]
fn rigid_transform_rejects_reflections() {
    assert!(!crate::is_rigid_transform(&[
        [-1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]));
}

#[test]
fn placement_reference_is_projected_and_angular_trims_use_context_units() {
    let result = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));
#3=PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(0.017453292519943295),#2);
#4=(CONVERSION_BASED_UNIT('degree',#3) NAMED_UNIT(*) PLANE_ANGLE_UNIT());
#5=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#4)) REPRESENTATION_CONTEXT('model','3D'));
#10=CARTESIAN_POINT('',(0.,0.,0.));
#11=DIRECTION('',(0.,0.,1.));
#12=DIRECTION('',(1.,0.,1.));
#13=AXIS2_PLACEMENT_3D('',#10,#11,#12);
#14=CIRCLE('',#13,2.);
#15=TRIMMED_CURVE('',#14,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(90.)),.T.,.PARAMETER.);
#16=GEOMETRIC_CURVE_SET('',(#15));
#17=SHAPE_REPRESENTATION('',(#16),#5);",
    );
    let circle = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#14")
        .expect("circle");
    let CurveGeometry::Circle {
        axis,
        ref_direction,
        ..
    } = circle.geometry
    else {
        panic!("decoded carrier is not a circle")
    };
    let dot = axis.x * ref_direction.x + axis.y * ref_direction.y + axis.z * ref_direction.z;
    assert!(dot.abs() < 1.0e-12);
    assert!(result
        .ir
        .model
        .procedural_curves
        .iter()
        .any(|curve| matches!(
            curve.definition,
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                parameter_range: [start, end],
                ..
            } if start.abs() < 1.0e-12 && (end - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12
        )));
}

#[test]
fn line_numeric_trim_uses_vector_magnitude_and_length_unit() {
    let result = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));
#10=CARTESIAN_POINT('',(0.,0.,0.));
#11=CARTESIAN_POINT('',(2.,0.,0.));
#12=DIRECTION('',(1.,0.,0.));
#13=VECTOR('',#12,2.);
#14=LINE('',#10,#13);
#15=TRIMMED_CURVE('',#14,(#11),(PARAMETER_VALUE(1.)),.T.,.UNSPECIFIED.);
#16=GEOMETRIC_CURVE_SET('',(#15));
#17=SHAPE_REPRESENTATION('',(#16),#2);",
    );
    assert!(result
        .ir
        .model
        .procedural_curves
        .iter()
        .any(|curve| matches!(
            curve.definition,
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                parameter_range: [start, end],
                ..
            } if (start - 2.0).abs() < 1.0e-12 && (end - 2.0).abs() < 1.0e-12
        )));
}

#[test]
fn unknown_recursive_curve_dependency_is_refused_without_panicking() {
    use cadmpeg_ir::geometry::{
        CompositeCurveSegment, CompositeCurveTransition, Curve, CurveGeometry,
    };

    let mut ir = CadIr::empty(Units::default());
    ir.model.curves.push(Curve {
        id: CurveId("unknown".into()),
        geometry: CurveGeometry::Unknown { record: None },
        source_object: None,
    });
    ir.model.curves.push(Curve {
        id: CurveId("composite".into()),
        geometry: CurveGeometry::Composite {
            segments: vec![CompositeCurveSegment {
                curve: CurveId("unknown".into()),
                same_sense: true,
                transition: CompositeCurveTransition::Continuous,
            }],
            self_intersect: Some(false),
        },
        source_object: None,
    });
    let output = export(&ir);
    assert!(!output.contains("COMPOSITE_CURVE("));
    let mut builder = crate::Builder::new(&ir, StepSchema::Ap242Edition3);
    assert!(builder.emit_curve("composite").is_none());
    assert!(builder.active_curves.is_empty());
    assert!(builder.emit_curve("composite").is_none());
    assert!(builder.active_curves.is_empty());
}

#[test]
fn standalone_geometry_uses_general_shape_representation() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.curves.push(Curve {
        id: CurveId("line".into()),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    let output = export(&ir);
    assert!(output.contains("SHAPE_REPRESENTATION('',"));
    assert!(!output.contains("ADVANCED_BREP_SHAPE_REPRESENTATION"));
}

#[test]
fn face_outer_bound_is_canonicalized_ahead_of_inner_bounds() {
    use cadmpeg_ir::ids::LoopId;
    use cadmpeg_ir::topology::Loop;

    let mut ir = unit_cube();
    let face = ir.model.faces[0].id.clone();
    let vertex = ir.model.vertices[0].id.clone();
    let inner = LoopId("zzzz:test:loop#inner".into());
    ir.model.loops.push(Loop {
        id: inner.clone(),
        face: face.clone(),
        boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Inner,
        coedges: Vec::new(),
        vertex_uses: vec![cadmpeg_ir::topology::VertexUse {
            vertex,
            after: None,
            pcurves: Vec::new(),
        }],
    });
    ir.model.faces[0].loops.push(inner);
    let output = export(&ir);
    let (exchange, diagnostics) = crate::parse::parse(output.as_bytes()).unwrap();
    assert!(diagnostics.is_empty());
    let (face_step, outer_bound, inner_bound, outer_loop) = exchange
        .records
        .iter()
        .find_map(|(&face_step, record)| {
            let partial = record.partials.first()?;
            if partial.name != "ADVANCED_FACE" {
                return None;
            }
            let crate::parse::Value::List(bounds) = partial.parameters.get(1)? else {
                return None;
            };
            if bounds.len() != 2 {
                return None;
            }
            let crate::parse::Value::Reference(first) = bounds[0] else {
                return None;
            };
            let crate::parse::Value::Reference(second) = bounds[1] else {
                return None;
            };
            let first_record = exchange.records.get(&first)?.partials.first()?;
            let second_record = exchange.records.get(&second)?.partials.first()?;
            let (outer, inner) = if first_record.name == "FACE_OUTER_BOUND" {
                (first, second)
            } else if second_record.name == "FACE_OUTER_BOUND" {
                (second, first)
            } else {
                return None;
            };
            let crate::parse::Value::Reference(outer_loop) = exchange.records.get(&outer)?.partials
                [0]
            .parameters
            .get(1)?
            else {
                return None;
            };
            Some((face_step, outer, inner, outer_loop))
        })
        .expect("face with outer and inner bounds");
    let ordered = format!("(#{outer_bound},#{inner_bound})");
    let reversed = format!("(#{inner_bound},#{outer_bound})");
    let reordered = output.replacen(&ordered, &reversed, 1);
    assert_ne!(reordered, output);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(reordered), &DecodeOptions::default())
        .expect("decode reversed face bounds");
    let face = decoded
        .ir
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == format!("step:data:face#{face_step}"))
        .expect("decoded face");
    assert_eq!(
        face.loops[0].as_str(),
        format!("step:data:loop#{outer_loop}-face-{face_step}")
    );
}

#[test]
fn failed_face_bounds_do_not_duplicate_the_shared_surface() {
    let mut ir = unit_cube();
    ir.model.faces[0].surface = ir.model.faces[1].surface.clone();
    ir.model.faces[0].loops.clear();
    let output = export(&ir);
    // Five face-owned surfaces remain after sharing, and the displaced carrier
    // is retained once as standalone construction geometry.
    assert_eq!(output.matches("= PLANE(").count(), 6);
}

#[test]
fn every_region_of_a_body_is_retained_as_a_shape_item() {
    let mut ir = unit_cube();
    let body = ir.model.bodies[0].id.clone();
    let mut region = ir.model.regions[0].clone();
    region.id.0 = "zzzz:test:region#second".into();
    ir.model.bodies[0].regions.push(region.id.clone());
    ir.model.regions.push(region);
    let mut builder = crate::Builder::new(&ir, StepSchema::Ap242Edition3);
    builder.build();
    assert_eq!(builder.body_item_refs[body.as_str()].len(), 2);
}

#[test]
fn ap242_dimension_kinds_emit_concrete_schema_entities() {
    use cadmpeg_ir::ids::PmiId;
    use cadmpeg_ir::pmi::{DimensionKind, GeometricToleranceKind, PmiDefinition};

    let mut ir = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../tests/fixtures/ap242_semantic_pmi.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode semantic PMI")
        .ir;
    let template = ir
        .model
        .pmi
        .iter()
        .find(|annotation| matches!(annotation.definition, PmiDefinition::Dimension { .. }))
        .cloned()
        .expect("dimension template");
    ir.model.pmi.clear();
    for (ordinal, kind) in [
        DimensionKind::Diameter,
        DimensionKind::Radius,
        DimensionKind::Location,
    ]
    .into_iter()
    .enumerate()
    {
        let mut annotation = template.clone();
        annotation.id = PmiId(format!("test:pmi:dimension#{ordinal}"));
        annotation.name = Some(format!("dimension {ordinal}"));
        let PmiDefinition::Dimension { dimension, .. } = &mut annotation.definition else {
            unreachable!()
        };
        *dimension = kind;
        ir.model.pmi.push(annotation);
    }
    let mut unsupported = template;
    unsupported.id = PmiId("test:pmi:tolerance#other".into());
    unsupported.definition = PmiDefinition::GeometricTolerance {
        tolerance: GeometricToleranceKind::Other("vendor_tolerance".into()),
        magnitude: cadmpeg_ir::PmiValue {
            value: 0.1,
            quantity: cadmpeg_ir::PmiQuantity::Length,
        },
        datum_system: None,
    };
    ir.model.pmi.push(unsupported);

    let mut output = Vec::new();
    let report = write_step(
        &ir,
        &mut output,
        &StepWriteOptions {
            schema: StepSchema::Ap242Edition3,
            ..StepWriteOptions::default()
        },
    )
    .expect("write dimensions");
    let text = String::from_utf8(output.clone()).unwrap();
    assert!(!text.contains("DIAMETER_SIZE"));
    assert!(!text.contains("RADIUS_SIZE"));
    assert!(!text.contains(" = GEOMETRIC_TOLERANCE("));
    assert!(text.contains(",'diameter')"));
    assert!(text.contains(",'radius')"));
    let (exchange, diagnostics) = crate::parse::parse(&output).unwrap();
    assert!(diagnostics.is_empty());
    let location = exchange
        .records
        .values()
        .find(|record| {
            record
                .partials
                .first()
                .is_some_and(|partial| partial.name == "DIMENSIONAL_LOCATION")
        })
        .expect("dimensional location");
    assert_eq!(location.partials[0].parameters.len(), 4);
    assert!(matches!(
        location.partials[0].parameters[0],
        crate::parse::Value::String(_)
    ));
    assert!(matches!(
        location.partials[0].parameters[1],
        crate::parse::Value::Omitted
    ));
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains("PMI annotation")));
}

#[test]
fn common_datum_compartment_round_trips_as_one_precedence() {
    use cadmpeg_ir::ids::PmiId;
    use cadmpeg_ir::pmi::{DatumReference, PmiDefinition};

    let mut ir = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../tests/fixtures/ap242_semantic_pmi.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode semantic PMI")
        .ir;
    let datum_a = ir
        .model
        .pmi
        .iter()
        .find(|annotation| matches!(annotation.definition, PmiDefinition::Datum { .. }))
        .cloned()
        .expect("datum A");
    let mut datum_b = datum_a.clone();
    datum_b.id = PmiId("test:model:pmi#datum-b".into());
    datum_b.definition = PmiDefinition::Datum {
        identification: "B".into(),
    };
    ir.model.pmi.push(datum_b.clone());
    let system = ir
        .model
        .pmi
        .iter_mut()
        .find(|annotation| matches!(annotation.definition, PmiDefinition::DatumSystem { .. }))
        .expect("datum system");
    let PmiDefinition::DatumSystem { references } = &mut system.definition else {
        unreachable!()
    };
    let modifiers = references[0].modifiers.clone();
    *references = vec![
        DatumReference {
            datum: datum_a.id,
            precedence: 1,
            common_group: Some(7),
            modifiers: modifiers.clone(),
        },
        DatumReference {
            datum: datum_b.id,
            precedence: 1,
            common_group: Some(7),
            modifiers: vec!["least_material_requirement".into()],
        },
    ];
    let validation = cadmpeg_ir::validate(&ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let mut output = Vec::new();
    write_step(
        &ir,
        &mut output,
        &StepWriteOptions {
            schema: StepSchema::Ap242Edition3,
            ..StepWriteOptions::default()
        },
    )
    .expect("write common datum");
    assert!(String::from_utf8_lossy(&output).contains("COMMON_DATUM_LIST(("));
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode common datum");
    assert!(roundtrip.ir.model.pmi.iter().any(|annotation| matches!(
        &annotation.definition,
        PmiDefinition::DatumSystem { references }
            if references.len() == 2
                && references.iter().all(|reference| reference.precedence == 1)
                && references.iter().all(|reference| reference.common_group == Some(1))
                && references[0].modifiers != references[1].modifiers
    )));
}

#[test]
fn rejected_step_write_detects_incomplete_datum_system() {
    use cadmpeg_ir::ids::PmiId;
    use cadmpeg_ir::pmi::PmiDefinition;

    let mut ir = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../tests/fixtures/ap242_semantic_pmi.p21")),
            &DecodeOptions::default(),
        )
        .unwrap()
        .ir;
    let system = ir
        .model
        .pmi
        .iter_mut()
        .find(|annotation| matches!(annotation.definition, PmiDefinition::DatumSystem { .. }))
        .unwrap();
    let PmiDefinition::DatumSystem { references } = &mut system.definition else {
        unreachable!()
    };
    references[0].datum = PmiId("test:model:pmi#missing".into());
    let mut output = Vec::new();
    assert!(matches!(
        write_step(
            &ir,
            &mut output,
            &StepWriteOptions {
                schema: StepSchema::Ap242Edition3,
                unsupported: StepUnsupportedPolicy::Reject,
                ..StepWriteOptions::default()
            }
        ),
        Err(StepError::Unsupported(_))
    ));
    assert!(output.is_empty());

    let system = ir
        .model
        .pmi
        .iter_mut()
        .find(|annotation| matches!(annotation.definition, PmiDefinition::DatumSystem { .. }))
        .unwrap();
    let PmiDefinition::DatumSystem { references } = &mut system.definition else {
        unreachable!()
    };
    references.clear();
    assert!(matches!(
        write_step(
            &ir,
            &mut output,
            &StepWriteOptions {
                schema: StepSchema::Ap242Edition3,
                unsupported: StepUnsupportedPolicy::Reject,
                ..StepWriteOptions::default()
            }
        ),
        Err(StepError::Unsupported(_))
    ));
    assert!(output.is_empty());
}

#[test]
fn presentation_reader_normalizes_invalid_layer_and_common_datum_inputs() {
    use cadmpeg_ir::pmi::PmiDefinition;
    use cadmpeg_ir::presentation::PresentationItem;

    let result = decode_inline(
        "#1=PRESENTATION_LAYER_ASSIGNMENT('','',());
#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#7=DATUM('',$,#5,.F.,'A');
#8=DATUM_SYSTEM('system','',#5,.F.,(#20));
#20=DATUM_REFERENCE_COMPARTMENT('',$,#5,.F.,COMMON_DATUM_LIST((#21)),());
#21=DATUM_REFERENCE_ELEMENT('',$,#5,.F.,#7,());
#30=PLUS_MINUS_TOLERANCE(#31,#32);
#31=UNKNOWN_LIMIT();
#32=UNKNOWN_CHARACTERISTIC();
#40=PRESENTATION_LAYER_ASSIGNMENT('inspection','',(#30));
#99=UNRESOLVED_PRODUCT();",
    );
    assert_eq!(result.ir.model.presentation_layers.len(), 1);
    assert!(matches!(
        result.ir.model.presentation_layers[0].items.as_slice(),
        [PresentationItem::Source { source_id }] if source_id == "#30"
    ));
    assert!(result.ir.model.pmi.iter().any(|annotation| matches!(
        &annotation.definition,
        PmiDefinition::DatumSystem { references }
            if references.len() == 1 && references[0].common_group.is_none()
    )));
    let validation = cadmpeg_ir::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

/// Emit a single surface carrier in isolation and return the DATA lines joined.
fn emit_surface_only(g: &SurfaceGeometry) -> String {
    let mut e = crate::writer::Emitter::new();
    crate::geometry::surface(&mut e, g);
    e.into_lines().join("\n")
}

/// Emit a single curve carrier in isolation and return the DATA lines joined.
fn emit_curve_only(g: &CurveGeometry) -> String {
    let mut e = crate::writer::Emitter::new();
    crate::geometry::curve(&mut e, g);
    e.into_lines().join("\n")
}

/// A one-face document whose single edge has no attributed curve, so the writer
/// must omit that edge and record a loss.
fn edgeless_doc() -> CadIr {
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::topology::{
        Body, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
    };
    let mut ir = CadIr::empty(Units::default());
    ir.model.points.push(Point {
        id: PointId("p0".into()),
        position: Point3::new(0.0, 0.0, 0.0),
        source_object: None,
    });
    ir.model.points.push(Point {
        id: PointId("p1".into()),
        position: Point3::new(1.0, 0.0, 0.0),
        source_object: None,
    });
    ir.model.vertices.push(Vertex {
        id: VertexId("v0".into()),
        point: PointId("p0".into()),
        tolerance: None,
    });
    ir.model.vertices.push(Vertex {
        id: VertexId("v1".into()),
        point: PointId("p1".into()),
        tolerance: None,
    });
    ir.model.edges.push(Edge {
        id: EdgeId("e0".into()),
        curve: None,
        start: VertexId("v0".into()),
        end: VertexId("v1".into()),
        param_range: None,
        tolerance: None,
    });
    ir.model.surfaces.push(Surface {
        id: SurfaceId("s0".into()),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    ir.model.coedges.push(Coedge {
        id: CoedgeId("ce0".into()),
        owner_loop: LoopId("lp0".into()),
        edge: EdgeId("e0".into()),
        next: CoedgeId("ce0".into()),
        previous: CoedgeId("ce0".into()),
        radial_next: CoedgeId("ce0".into()),
        sense: Sense::Forward,
        pcurves: Vec::new(),
        use_curve: None,
        use_curve_parameter_range: None,
    });
    ir.model.loops.push(Loop {
        id: LoopId("lp0".into()),
        face: FaceId("f0".into()),
        boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Outer,
        coedges: vec![CoedgeId("ce0".into())],
        vertex_uses: Vec::new(),
    });
    ir.model.faces.push(Face {
        id: FaceId("f0".into()),
        shell: ShellId("sh0".into()),
        surface: SurfaceId("s0".into()),
        sense: Sense::Forward,
        loops: vec![LoopId("lp0".into())],
        name: None,
        color: None,
        tolerance: None,
    });
    ir.model.shells.push(Shell {
        id: ShellId("sh0".into()),
        region: RegionId("l0".into()),
        faces: vec![FaceId("f0".into())],
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    ir.model.regions.push(Region {
        id: RegionId("l0".into()),
        body: BodyId("b0".into()),
        shells: vec![ShellId("sh0".into())],
    });
    ir.model.bodies.push(Body {
        id: BodyId("b0".into()),
        kind: cadmpeg_ir::topology::BodyKind::Solid,
        regions: vec![RegionId("l0".into())],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir
}

#[test]
fn cube_has_valid_part21_envelope() {
    let s = export(&unit_cube());
    assert!(s.starts_with("ISO-10303-21;\n"));
    assert!(s.contains("HEADER;"));
    assert!(s.contains("FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));"));
    assert!(s.contains("\nDATA;\n"));
    assert!(s.trim_end().ends_with("END-ISO-10303-21;"));
    // ENDSEC appears twice: once closing HEADER, once closing DATA.
    assert_eq!(s.matches("ENDSEC;").count(), 2);
}

#[test]
fn cube_emits_full_brep_hierarchy() {
    let s = export(&unit_cube());
    assert!(s.contains("MANIFOLD_SOLID_BREP"));
    assert!(s.contains("CLOSED_SHELL"));
    // Six planar faces, twelve unique edges, eight vertices.
    assert_eq!(s.matches("ADVANCED_FACE").count(), 6);
    assert_eq!(s.matches("= PLANE(").count(), 6);
    assert_eq!(s.matches("EDGE_CURVE").count(), 12);
    assert_eq!(s.matches("VERTEX_POINT").count(), 8);
    // 6 loops * 4 coedges = 24 oriented edges.
    assert_eq!(s.matches("ORIENTED_EDGE").count(), 24);
    assert_eq!(s.matches("= EDGE_LOOP(").count(), 6);
    assert_eq!(s.matches("FACE_OUTER_BOUND").count(), 6);
    // Every line edge carries a LINE curve.
    assert_eq!(s.matches("= LINE(").count(), 12);
}

#[test]
fn cube_product_and_context_boilerplate_present() {
    let s = export(&unit_cube());
    for kw in [
        "APPLICATION_CONTEXT",
        "APPLICATION_PROTOCOL_DEFINITION",
        "PRODUCT(",
        "PRODUCT_DEFINITION(",
        "PRODUCT_DEFINITION_SHAPE",
        "SHAPE_DEFINITION_REPRESENTATION",
        "ADVANCED_BREP_SHAPE_REPRESENTATION",
        "GEOMETRIC_REPRESENTATION_CONTEXT",
        "UNCERTAINTY_MEASURE_WITH_UNIT",
    ] {
        assert!(s.contains(kw), "missing {kw}");
    }
    // mm document → millimetre SI length unit.
    assert!(s.contains("SI_UNIT(.MILLI.,.METRE.)"));
}

#[test]
fn every_reference_resolves() {
    // Collect declared instance ids (#n = ...) and every #n referenced anywhere;
    // a valid Part 21 graph references only declared instances.
    let s = export(&unit_cube());
    let mut declared = std::collections::HashSet::new();
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix('#') {
            if let Some(eq) = rest.find(" =") {
                if let Ok(id) = rest[..eq].parse::<u64>() {
                    declared.insert(id);
                }
            }
        }
    }
    assert!(!declared.is_empty());
    // Scan referenced ids: '#' followed by digits, but skip the leading id of a
    // declaration line (handled by only scanning after the first '=').
    for line in s.lines() {
        let Some(eq) = line.find('=') else { continue };
        let body = &line[eq + 1..];
        let bytes = body.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'#' {
                let start = i + 1;
                let mut j = start;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j > start {
                    let id: u64 = body[start..j].parse().unwrap();
                    assert!(
                        declared.contains(&id),
                        "dangling reference #{id} in: {line}"
                    );
                }
                i = j;
            } else {
                i += 1;
            }
        }
    }
}

#[test]
fn reports_entity_counts_and_no_geometry_loss_for_cube() {
    let mut buf = Vec::new();
    let report = write_step(&unit_cube(), &mut buf, &StepWriteOptions::default()).unwrap();
    assert_eq!(report.census.total(), buf_line_count(&buf));
    assert_eq!(report.census.counts.get("ADVANCED_FACE"), Some(&6));
    assert_eq!(report.census.counts.get("VERTEX_POINT"), Some(&8));
    // The cube is fully representable: no error/blocking losses.
    assert_eq!(report.error_count(), 0);
}

fn buf_line_count(buf: &[u8]) -> usize {
    // Count DATA-section instance lines: those starting with '#'.
    String::from_utf8_lossy(buf)
        .lines()
        .filter(|l| l.starts_with('#'))
        .count()
}

/// A minimal single-cylinder-surface document exercising analytic emission and
/// interning of shared points/directions.
fn cylinder_surface_doc() -> CadIr {
    let mut ir = CadIr::empty(Units::default());
    ir.model.surfaces.push(Surface {
        id: SurfaceId("cyl".into()),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 5.0,
        },
        source_object: None,
    });
    ir
}

#[test]
fn analytic_surfaces_map_to_their_step_entities() {
    // Build one doc per analytic kind and check the keyword appears.
    let cases: Vec<(SurfaceGeometry, &str)> = vec![
        (
            SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 5.0,
            },
            "CYLINDRICAL_SURFACE",
        ),
        (
            SurfaceGeometry::Cone {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 2.0,
                ratio: 1.0,
                half_angle: 0.5,
            },
            "CONICAL_SURFACE",
        ),
        (
            SurfaceGeometry::Sphere {
                center: Point3::new(1.0, 2.0, 3.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 4.0,
            },
            "SPHERICAL_SURFACE",
        ),
        (
            SurfaceGeometry::Torus {
                center: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                major_radius: 3.0,
                minor_radius: 1.0,
            },
            "TOROIDAL_SURFACE",
        ),
    ];
    for (geom, kw) in cases {
        let mut ir = CadIr::empty(Units::default());
        ir.model.surfaces.push(Surface {
            id: SurfaceId("s".into()),
            geometry: geom,
            source_object: None,
        });
        // Surfaces alone aren't reachable from a shell, so they won't be emitted
        // by the topology walk; emit directly via the geometry module instead.
        let s = emit_surface_only(&ir.model.surfaces[0].geometry);
        assert!(s.contains(kw), "missing {kw} in {s}");
    }
}

#[test]
fn analytic_surface_placements_preserve_orientation() {
    let geometry = SurfaceGeometry::Sphere {
        center: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 1.0, 0.0),
        ref_direction: Vector3::new(0.0, 0.0, 1.0),
        radius: 4.0,
    };
    let s = emit_surface_only(&geometry);
    assert!(s.contains("DIRECTION('',(0.,1.,0.))"));
    assert!(s.contains("DIRECTION('',(0.,0.,1.))"));
}

#[test]
fn parabola_and_hyperbola_map_to_step_conics() {
    let parabola = emit_curve_only(&CurveGeometry::Parabola {
        vertex: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(0.0, 1.0, 0.0),
        focal_distance: 2.5,
    });
    assert!(parabola.contains("= PARABOLA("));
    assert!(parabola.contains(",2.5)"));

    let hyperbola = emit_curve_only(&CurveGeometry::Hyperbola {
        center: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(0.0, 1.0, 0.0),
        major_radius: 4.0,
        minor_radius: 1.5,
    });
    assert!(hyperbola.contains("= HYPERBOLA("));
    assert!(hyperbola.contains(",4.,1.5)"));
}

#[test]
fn nurbs_curve_non_rational_uses_with_knots() {
    let n = NurbsCurve {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
        ],
        weights: None,
        periodic: false,
    };
    let s = emit_curve_only(&CurveGeometry::Nurbs(n));
    assert!(s.contains("B_SPLINE_CURVE_WITH_KNOTS"));
    // Clamped end knots collapse to multiplicity 3.
    assert!(s.contains("(3,3)"), "knot multiplicities: {s}");
    assert!(!s.contains("RATIONAL"));
}

#[test]
fn nurbs_curve_rational_uses_complex_form() {
    let n = NurbsCurve {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
        ],
        weights: Some(vec![1.0, 0.5, 1.0]),
        periodic: false,
    };
    let s = emit_curve_only(&CurveGeometry::Nurbs(n));
    assert!(s.contains("RATIONAL_B_SPLINE_CURVE"));
    assert!(s.contains("BOUNDED_CURVE()"));
}

#[test]
fn nurbs_surface_grid_orientation_is_u_major() {
    let n = NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ],
        weights: None,
        u_periodic: false,
        v_periodic: false,
    };
    let s = emit_surface_only(&SurfaceGeometry::Nurbs(n));
    assert!(s.contains("B_SPLINE_SURFACE_WITH_KNOTS"));
}

#[test]
fn v1_document_uses_canonical_millimeter_unit() {
    let ir = unit_cube();
    assert_eq!(ir.units.length, LengthUnit::Millimeter);
    let s = export(&ir);
    assert!(s.contains("SI_UNIT(.MILLI.,.METRE.)"));
    assert!(!s.contains("CONVERSION_BASED_UNIT"));
}

#[test]
fn real_formatting_always_has_decimal_point() {
    // Coordinates like 10 must serialize as 10. (a Part 21 real), never 10.
    let s = export(&unit_cube());
    assert!(s.contains("10.")); // cube corner coordinate
    assert!(!s.contains("(10,")); // no bare integer coordinate
}

#[test]
fn edge_without_curve_is_reported_and_omitted() {
    let _ = cylinder_surface_doc(); // keep helper exercised
                                    // Build a tiny doc: one face on a plane, one loop, one coedge whose edge has
                                    // no curve. The edge should be omitted and a loss recorded.
    let ir = edgeless_doc();
    let mut buf = Vec::new();
    let report = write_step(&ir, &mut buf, &StepWriteOptions::default()).unwrap();
    let curve = Curve {
        id: CurveId("unused".into()),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    };
    let _ = curve; // silence unused import path
    assert!(report
        .losses
        .iter()
        .any(|l| l.message.contains("edge(s) have no typed 3D curve")));
    assert!(report.losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::TopologyNotTransferred
            && loss
                .message
                .contains("was omitted because it has no 3D curve")
    }));
}

#[test]
fn subds_tessellations_and_source_associations_are_reported_as_losses() {
    let source_object = cadmpeg_ir::SourceObjectAssociation {
        format: "test".into(),
        object_id: "object-0".into(),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let mut ir = unit_cube();
    ir.model.subds.push(cadmpeg_ir::SubdSurface {
        id: cadmpeg_ir::ids::SubdId("test:step:subd#0".into()),
        scheme: cadmpeg_ir::SubdScheme::CatmullClark,
        vertices: Vec::new(),
        edges: Vec::new(),
        faces: Vec::new(),
        source_object: Some(source_object.clone()),
    });
    ir.model
        .tessellations
        .push(cadmpeg_ir::tessellation::Tessellation {
            id: "test:step:tessellation#0".into(),
            body: None,
            faces: Vec::new(),
            chordal_deflection: None,
            source_object: Some(source_object),
            vertices: Vec::new(),
            triangles: Vec::new(),
            strip_lengths: Vec::new(),
            normals: Vec::new(),
            channels: Vec::new(),
        });

    let report = write_step(&ir, &mut Vec::new(), &StepWriteOptions::default()).unwrap();
    assert!(report.losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::LossCategory::Geometry
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss
                .message
                .contains("1 subdivision surface(s) were omitted")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::LossCategory::Geometry
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss
                .message
                .contains("1 tessellation(s) require an AP242 target")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::LossCategory::Metadata
            && loss
                .message
                .contains("2 source-object association(s) were not represented")
    }));
}

#[test]
fn face_on_unknown_surface_is_skipped_and_reported() {
    // Turn the cube's first face onto an unknown (opaque) surface. That face
    // cannot become an ADVANCED_FACE, so the writer must skip it and record one
    // aggregated, counted loss — the remaining five faces still export.
    let mut ir = unit_cube();
    let target = ir.model.faces[0].surface.0.clone();
    for s in &mut ir.model.surfaces {
        if s.id.0 == target {
            s.geometry = SurfaceGeometry::Unknown { record: None };
        }
    }
    let mut buf = Vec::new();
    let report = write_step(&ir, &mut buf, &StepWriteOptions::default()).unwrap();
    let s = String::from_utf8(buf).unwrap();

    assert_eq!(
        s.matches("ADVANCED_FACE").count(),
        5,
        "the unknown-surface face should be omitted"
    );
    let unknown_notes: Vec<_> = report
        .losses
        .iter()
        .filter(|l| l.message.contains("rest on an unknown"))
        .collect();
    assert_eq!(
        unknown_notes.len(),
        1,
        "loss must be aggregated into a single counted note, got: {:?}",
        report.losses
    );
    assert!(unknown_notes[0].message.contains("1 face(s)"));
    assert!(report.losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::TopologyNotTransferred
            && loss.message.contains("omitted face")
    }));
}

#[test]
fn writer_reports_each_enclosing_topology_reduction_and_strict_mode_rejects() {
    let mut outer_face = unit_cube();
    outer_face.model.faces[0].loops.clear();
    let report = write_step(&outer_face, &mut Vec::new(), &StepWriteOptions::default())
        .expect("report mode writes the surviving faces");
    assert!(report.losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::TopologyNotTransferred
            && loss.severity == cadmpeg_ir::Severity::Error
            && loss.message.contains("has no writable bounds")
    }));

    let mut inner_loop = unit_cube();
    inner_loop.model.faces[0]
        .loops
        .push(cadmpeg_ir::ids::LoopId(
            "step:data:loop#missing-inner".into(),
        ));
    let report = write_step(&inner_loop, &mut Vec::new(), &StepWriteOptions::default())
        .expect("report mode writes the surviving outer loop");
    assert!(report.losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::TopologyNotTransferred
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss.message.contains("has no writable topology")
    }));

    let mut missing_edge = unit_cube();
    missing_edge.model.coedges[0].edge = cadmpeg_ir::ids::EdgeId("step:data:edge#missing".into());
    let report = write_step(&missing_edge, &mut Vec::new(), &StepWriteOptions::default())
        .expect("report mode writes the surviving coedges");
    assert!(report.losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::TopologyNotTransferred
            && loss.message.contains("loop")
            && loss.message.contains("edge")
    }));

    let mut missing_void = unit_cube();
    missing_void.model.regions[0]
        .shells
        .push(cadmpeg_ir::ids::ShellId(
            "step:data:shell#missing-void".into(),
        ));
    let report = write_step(&missing_void, &mut Vec::new(), &StepWriteOptions::default())
        .expect("report mode writes the outer shell");
    assert!(report.losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::TopologyNotTransferred
            && loss.severity == cadmpeg_ir::Severity::Error
            && loss.message.contains("omitted void shell")
    }));

    let options = StepWriteOptions {
        unsupported: StepUnsupportedPolicy::Reject,
        ..StepWriteOptions::default()
    };
    assert!(matches!(
        write_step(&missing_void, &mut Vec::new(), &options),
        Err(StepError::Unsupported(_))
    ));
}

#[test]
fn unsupported_nested_and_polygonal_carriers_are_skipped_without_panicking() {
    let mut polygonal = unit_cube();
    let surface_id = polygonal.model.faces[0].surface.clone();
    polygonal
        .model
        .surfaces
        .iter_mut()
        .find(|surface| surface.id == surface_id)
        .unwrap()
        .geometry = SurfaceGeometry::Polygonal {
        vertices: Vec::new(),
        triangles: Vec::new(),
        chordal_deflection: 0.1,
    };
    let report = write_step(&polygonal, &mut Vec::new(), &StepWriteOptions::default())
        .expect("polygonal face is reported as an export loss");
    assert!(report.losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::LossCategory::Geometry
            && loss.message.contains("unknown or STEP-unsupported surface")
    }));

    let mut nested_unknown = unit_cube();
    let curve_id = nested_unknown.model.edges[0].curve.clone().unwrap();
    nested_unknown
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .unwrap()
        .geometry = CurveGeometry::Transformed {
        basis: Box::new(CurveGeometry::Unknown { record: None }),
        transform: cadmpeg_ir::transform::Transform::identity(),
    };
    let report = write_step(
        &nested_unknown,
        &mut Vec::new(),
        &StepWriteOptions::default(),
    )
    .expect("transformed unknown curve is reported as an export loss");
    assert!(report.losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::LossCategory::Geometry
            && loss.message.contains("STEP-unsupported transform")
    }));
}

#[test]
fn signed_analytic_radius_normalization_is_reported() {
    let mut ir = unit_cube();
    ir.model.surfaces[0].geometry = SurfaceGeometry::Sphere {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: -2.0,
    };

    let mut buf = Vec::new();
    let report = write_step(&ir, &mut buf, &StepWriteOptions::default()).unwrap();

    assert!(report.losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::LossCategory::Geometry
            && loss.message.contains("normalized to positive STEP radii")
    }));
}

#[test]
fn elliptical_cone_reduction_is_reported() {
    let mut ir = unit_cube();
    ir.model.surfaces[0].geometry = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
        ratio: 0.4,
        half_angle: 0.5,
    };

    let mut buf = Vec::new();
    let report = write_step(&ir, &mut buf, &StepWriteOptions::default()).unwrap();

    assert!(report.losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::LossCategory::Geometry
            && loss.message.contains("elliptical cone surface(s)")
    }));
}

#[test]
fn procedural_construction_reduction_is_reported() {
    let mut ir = unit_cube();
    ir.model
        .procedural_curves
        .push(cadmpeg_ir::geometry::ProceduralCurve {
            id: ProceduralCurveId("generated_int_cur".into()),
            curve: ir.model.curves[0].id.clone(),
            definition: cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection {
                context: cadmpeg_ir::geometry::IntcurveSupportContext {
                    sides: std::array::from_fn(|_| cadmpeg_ir::geometry::IntcurveSupportSide {
                        surface: None,
                        pcurve: None,
                        pcurve_parameter_range: None,
                    }),
                    parameter_range: [0.0, 1.0],
                    discontinuities: std::array::from_fn(|_| Vec::new()),
                },
                discontinuity_flag: false,
            },
            cache_fit_tolerance: Some(0.01),
        });

    let mut buf = Vec::new();
    let report = write_step(&ir, &mut buf, &StepWriteOptions::default()).unwrap();
    assert!(report.losses.iter().any(|loss| loss
        .message
        .contains("reduced to their solved STEP carriers")));
}

#[test]
fn source_native_record_reduction_is_reported() {
    let mut ir = unit_cube();
    ir.native.namespace_mut("f3d").arenas.insert(
        "asm_histories".into(),
        vec![cadmpeg_ir::NativeRecord::new(
            "asm-history-0",
            Default::default(),
        )],
    );
    ir.finalize();

    let mut buf = Vec::new();
    let report = write_step(&ir, &mut buf, &StepWriteOptions::default()).unwrap();
    assert!(report.losses.iter().any(|loss| loss
        .message
        .contains("source-native record(s) were not represented in STEP")));
}

#[test]
fn strict_writer_rejects_before_emitting_bytes() {
    let mut ir = unit_cube();
    ir.native.namespace_mut("f3d").arenas.insert(
        "asm_histories".into(),
        vec![cadmpeg_ir::NativeRecord::new(
            "asm-history-0",
            Default::default(),
        )],
    );
    ir.finalize();
    let options = StepWriteOptions {
        unsupported: StepUnsupportedPolicy::Reject,
        ..StepWriteOptions::default()
    };

    let mut bytes = Vec::new();
    let error = write_step(&ir, &mut bytes, &options).expect_err("strict rejection");
    assert!(matches!(error, StepError::Unsupported(_)));
    assert!(bytes.is_empty());
}

#[test]
fn strict_writer_refuses_retained_opaque_step_records_atomically() {
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../tests/fixtures/ap242_minimal.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode opaque STEP records");
    assert_eq!(decoded.ir.native_unknowns("step").unwrap().len(), 2);

    let mut bytes = Vec::new();
    let result = write_step(
        &decoded.ir,
        &mut bytes,
        &StepWriteOptions {
            schema: StepSchema::Ap242Edition3,
            unsupported: StepUnsupportedPolicy::Reject,
            ..StepWriteOptions::default()
        },
    );
    assert!(matches!(result, Err(StepError::Unsupported(_))));
    assert!(bytes.is_empty());
}

#[test]
fn hidden_body_geometry_and_visibility_round_trip() {
    let mut ir = unit_cube();
    ir.model.bodies[0].visible = Some(false);
    let mut buf = Vec::new();
    let report = write_step(&ir, &mut buf, &StepWriteOptions::default()).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.contains("MANIFOLD_SOLID_BREP"));
    assert!(s.contains("ADVANCED_FACE"));
    assert!(s.contains("INVISIBILITY"));
    assert!(report.losses.is_empty());
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(s.into_bytes()), &DecodeOptions::default())
        .expect("decode hidden body");
    assert_eq!(decoded.ir.model.bodies[0].visible, Some(false));

    let mut transformed = unit_cube();
    transformed.model.bodies[0].visible = Some(false);
    transformed.model.bodies[0].transform = Some(cadmpeg_ir::transform::Transform {
        rows: [
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    });
    let transformed_text = export(&transformed);
    assert!(transformed_text.contains("MAPPED_ITEM"));
    assert!(!transformed_text.contains("ADVANCED_BREP_SHAPE_REPRESENTATION"));
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(transformed_text),
            &DecodeOptions::default(),
        )
        .expect("decode hidden transformed body");
    assert_eq!(decoded.ir.model.bodies[0].visible, Some(false));

    // An explicitly visible body exports unchanged.
    let mut ir = unit_cube();
    ir.model.bodies[0].visible = Some(true);
    let s = export(&ir);
    assert!(s.contains("MANIFOLD_SOLID_BREP"));
}

#[test]
fn body_color_becomes_per_face_styled_item_presentation() {
    let mut ir = unit_cube();
    ir.model.bodies[0].color = Some(cadmpeg_ir::topology::Color {
        r: 0.25,
        g: 0.5,
        b: 0.75,
        a: 1.0,
    });
    let face_count = ir.model.faces.len();
    let s = export(&ir);
    assert!(s.contains("COLOUR_RGB('',0.25,0.5,0.75)"));
    assert!(s.contains("MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION"));
    // The body color is pushed down onto every face: one STYLED_ITEM per face,
    // each targeting an ADVANCED_FACE rather than the solid. OCCT/VTK viewers
    // (e.g. f3d) read colors only from faces, not MANIFOLD_SOLID_BREP.
    let styled: Vec<&str> = s.lines().filter(|l| l.contains("STYLED_ITEM")).collect();
    assert_eq!(styled.len(), face_count);
    let solid = s
        .lines()
        .find(|line| line.contains("MANIFOLD_SOLID_BREP"))
        .and_then(|line| line.split(" =").next())
        .unwrap()
        .to_string();
    for item in &styled {
        let target = item
            .rsplit_once(',')
            .map(|(_, tail)| tail.trim_end_matches(");").to_string())
            .unwrap();
        assert_ne!(target, solid, "body color must not style the solid");
        assert!(
            s.lines()
                .any(|line| line.starts_with(&format!("{target} = ADVANCED_FACE"))),
            "styled item must reference a face"
        );
    }
}

#[test]
fn face_appearance_binding_styles_the_advanced_face() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube();
    let face = ir.model.faces[0].id.clone();
    ir.model.appearances.push(Appearance {
        id: AppearanceId("test:appearance#black".to_string()),
        name: None,
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(cadmpeg_ir::topology::Color {
            r: 0.125,
            g: 0.125,
            b: 0.125,
            a: 1.0,
        }),
        properties: std::collections::BTreeMap::default(),
        textures: Vec::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:appearance-binding#face".to_string(),
        target: AppearanceTarget::Face(face),
        appearance: AppearanceId("test:appearance#black".to_string()),
        source_entity_id: None,
        object_type: None,
        channels: std::collections::BTreeMap::default(),
    });
    let s = export(&ir);
    assert!(s.contains("COLOUR_RGB('',0.125,0.125,0.125)"));
    let styled: Vec<&str> = s.lines().filter(|l| l.contains("STYLED_ITEM")).collect();
    assert_eq!(styled.len(), 1);
    // The styled item targets an ADVANCED_FACE instance.
    let target = styled[0]
        .rsplit_once(',')
        .map(|(_, tail)| tail.trim_end_matches(");").to_string())
        .unwrap();
    let face_line = s
        .lines()
        .find(|line| line.starts_with(&format!("{target} = ADVANCED_FACE")));
    assert!(face_line.is_some(), "styled item must reference a face");
}

/// The soccer-ball case: a body carries a base color and one face overrides it.
/// Every face must be styled (body color pushed down onto the faces that do not
/// override it), and the overriding face must carry its own color.
#[test]
fn face_override_wins_over_body_color_and_body_fills_the_rest() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube();
    let face_count = ir.model.faces.len();
    // White body base color.
    ir.model.bodies[0].color = Some(cadmpeg_ir::topology::Color {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    });
    // Black override on a single face, via an appearance binding.
    let face = ir.model.faces[0].id.clone();
    ir.model.appearances.push(Appearance {
        id: AppearanceId("test:appearance#black".to_string()),
        name: None,
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(cadmpeg_ir::topology::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        }),
        properties: std::collections::BTreeMap::default(),
        textures: Vec::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:appearance-binding#face".to_string(),
        target: AppearanceTarget::Face(face),
        appearance: AppearanceId("test:appearance#black".to_string()),
        source_entity_id: None,
        object_type: None,
        channels: std::collections::BTreeMap::default(),
    });

    let s = export(&ir);
    // Both colors are present, and every face is styled.
    assert!(s.contains("COLOUR_RGB('',1.,1.,1.)"));
    assert!(s.contains("COLOUR_RGB('',0.,0.,0.)"));
    let styled: Vec<&str> = s.lines().filter(|l| l.contains("STYLED_ITEM")).collect();
    assert_eq!(styled.len(), face_count);
    // Each color's style chain is emitted once and shared; grouping the styled
    // items by their style ref must yield exactly two groups sized 1 and
    // face_count - 1 (the lone override plus every inherited face).
    let mut per_style = std::collections::BTreeMap::<String, usize>::default();
    for item in &styled {
        // STYLED_ITEM('color',(#psa),#face)
        let psa = item
            .split_once(",(")
            .and_then(|(_, tail)| tail.split(')').next())
            .unwrap()
            .to_string();
        *per_style.entry(psa).or_default() += 1;
    }
    let mut counts: Vec<usize> = per_style.values().copied().collect();
    counts.sort_unstable();
    assert_eq!(counts, vec![1, face_count - 1]);
}

#[path = "integration_tests.rs"]
mod integration_tests;
