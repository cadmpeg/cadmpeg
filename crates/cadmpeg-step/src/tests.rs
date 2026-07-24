// SPDX-License-Identifier: Apache-2.0
//! Self-contained tests: IR documents are built in code (via the IR crate's
//! fixtures or inline), and expected STEP fragments are asserted inline. No test
//! depends on an external STEP consumer.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::{Codec, CodecEntry, Confidence, DecodeOptions};
use cadmpeg_ir::decode::InspectOptions;
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::{LengthUnit, Units};
use cadmpeg_ir::CadIr;
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
        anchors.push_str(&format!(
            "<a{index}>=(<a{}>,<a{}>);\n",
            index - 1,
            index - 1
        ));
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
        anchors.push_str(&format!(
            "<a{index}>=(<a{}>,<a{}>);\n",
            index - 1,
            index - 1
        ));
    }
    let records = (1..=8)
        .map(|id| format!("#{id}=ITEM(<a17>);"))
        .collect::<String>();
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'3;1');FILE_NAME('','','',(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;{anchors}ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;"
    );
    let error = crate::parse::parse(source.as_bytes()).unwrap_err();
    assert!(error.to_string().contains("expanded anchor"));
}

#[test]
fn parser_rejects_duplicate_complex_partial_names() {
    let source = b"ISO-10303-21;HEADER;ENDSEC;DATA;#1=(A()A());ENDSEC;END-ISO-10303-21;";
    assert!(crate::parse::parse(source).is_err());
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
            matches!(error, cadmpeg_ir::codec::CodecError::NotImplemented(message) if message == reason)
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
    let exchange = crate::parse::parse(bytes).expect("parse opaque signature payload");
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
        .arena_as::<cadmpeg_ir::unknown::UnknownRecord>("unknowns")
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
    let validation = cadmpeg_ir::validate::validate(&result.ir, result.report.losses.clone());
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
        [cadmpeg_ir::presentation::PresentationItem::Face { .. }]
    ));
    let validation = cadmpeg_ir::validate::validate(&result.ir, result.report.losses.clone());
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
    let validation = cadmpeg_ir::validate::validate(&result.ir, result.report.losses.clone());
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
    let validation = cadmpeg_ir::validate::validate(&result.ir, result.report.losses.clone());
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
    let validation = cadmpeg_ir::validate::validate(&result.ir, result.report.losses.clone());
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
    let product = cadmpeg_ir::ids::ProductId("product-0".into());
    ir.model.products.push(cadmpeg_ir::product::Product {
        id: product.clone(),
        product_id: "PART-001".into(),
        name: Some("Cube part".into()),
        bodies: vec![ir.model.bodies[0].id.clone()],
    });
    ir.model
        .product_occurrences
        .push(cadmpeg_ir::product::ProductOccurrence {
            id: cadmpeg_ir::ids::OccurrenceId("root-0".into()),
            product,
            parent: cadmpeg_ir::product::OccurrenceParent::Root,
            transform: cadmpeg_ir::transform::Transform::identity(),
            name: Some("Cube root".into()),
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
    assert_eq!(decoded.ir.model.products.len(), 1);
    assert_eq!(decoded.ir.model.products[0].product_id, "PART-001");
    assert_eq!(decoded.ir.model.products[0].bodies.len(), 1);
    assert_eq!(decoded.ir.model.product_occurrences.len(), 1);
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
    let validation = cadmpeg_ir::validate::validate(&decoded.ir, decoded.report.losses);
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
    use cadmpeg_ir::product::OccurrenceParent;

    let bytes = include_bytes!("../tests/fixtures/ap242_assembly.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP242 assembly");

    assert_eq!(result.ir.model.products.len(), 2);
    assert_eq!(result.ir.model.product_occurrences.len(), 2);
    let child = result
        .ir
        .model
        .product_occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("Placed child"))
        .unwrap();
    assert!(matches!(child.parent, OccurrenceParent::Occurrence { .. }));
    assert_eq!(child.transform.rows[0][3], 25.0);
    assert_eq!(child.transform.rows[1][3], 0.0);
    assert_eq!(child.transform.rows[2][3], 0.0);
    let validation = cadmpeg_ir::validate::validate(&result.ir, result.report.losses.clone());
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
    assert_eq!(roundtrip.ir.model.products.len(), 2);
    assert_eq!(roundtrip.ir.model.product_occurrences.len(), 2);
    let child = roundtrip
        .ir
        .model
        .product_occurrences
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
        .product_occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("Mapped child"))
        .unwrap();
    assert_eq!(child.transform.rows[0][3], 40.0);
    assert_eq!(child.transform.rows[1][3], 5.0);
    let validation = cadmpeg_ir::validate::validate(&result.ir, result.report.losses.clone());
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
        mesh.body.as_ref().map(|body| body.as_str()),
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
    let validation = cadmpeg_ir::validate::validate(&result.ir, result.report.losses.clone());
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
            magnitude: cadmpeg_ir::pmi::PmiValue {
                value: 0.05,
                quantity: PmiQuantity::Length,
            },
            datum_system: None,
        }
    ));
    let validation = cadmpeg_ir::validate::validate(&result.ir, result.report.losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    let semantic = dimension.id.clone();
    result.ir.model.pmi.push(cadmpeg_ir::pmi::PmiAnnotation {
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
            nominal: Some(cadmpeg_ir::pmi::PmiValue {
                value: 12.0,
                quantity: PmiQuantity::Length,
            }),
            lower_deviation: Some(cadmpeg_ir::pmi::PmiValue { value: -0.1, .. }),
            upper_deviation: Some(cadmpeg_ir::pmi::PmiValue { value: 0.2, .. }),
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
    let validation = cadmpeg_ir::validate::validate(&result.ir, result.report.losses.clone());
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
        records.push_str(&format!(
            "#{representation}=SHAPE_REPRESENTATION('',(#{first},#{second}),$);\n\
#{map}=REPRESENTATION_MAP($,#{next});\n\
#{first}=MAPPED_ITEM('',#{map},$);\n\
#{second}=MAPPED_ITEM('',#{map},$);\n"
        ));
    }
    records.push_str(&format!(
        "#{}=SHAPE_REPRESENTATION('',(#9000),$);\n#9000=MANIFOLD_SOLID_BREP('',#9001);\n#9001=CLOSED_SHELL('',());",
        100 + depth
    ));

    let result = decode_inline(&records);
    assert_eq!(result.ir.model.products.len(), 1);
    assert_eq!(result.ir.model.products[0].bodies.len(), 1);
    assert_eq!(
        result.ir.model.products[0].bodies[0].as_str(),
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
            upper_deviation: Some(cadmpeg_ir::pmi::PmiValue { value, .. }),
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
            nominal: Some(cadmpeg_ir::pmi::PmiValue { value, .. }),
            ..
        } if (value - 127.0).abs() < 1.0e-12
    )));
}

#[test]
fn repeated_subassembly_instances_each_receive_the_subtree() {
    use cadmpeg_ir::product::OccurrenceParent;

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
    assert_eq!(result.ir.model.product_occurrences.len(), 5);
    let subassemblies = result
        .ir
        .model
        .product_occurrences
        .iter()
        .filter(|occurrence| occurrence.product.as_str() == "step:product:product#7")
        .collect::<Vec<_>>();
    assert_eq!(subassemblies.len(), 2);
    for subassembly in subassemblies {
        assert_eq!(
            result
                .ir
                .model
                .product_occurrences
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

    assert_eq!(result.ir.model.products.len(), 2);
    assert_eq!(result.ir.model.product_occurrences.len(), 2);
    assert!(result
        .ir
        .model
        .product_occurrences
        .iter()
        .any(|occurrence| occurrence.product.as_str() == "step:product:product#7"));
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
    assert!(!crate::build::is_rigid_transform(&[
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
    let mut builder = crate::build::Builder::new(&ir, StepSchema::Ap242Edition3);
    assert!(builder.emit_curve("composite").is_none());
    assert!(builder.geom.active_curves.is_empty());
    assert!(builder.emit_curve("composite").is_none());
    assert!(builder.geom.active_curves.is_empty());
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
    let exchange = crate::parse::parse(output.as_bytes()).unwrap();
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
    let mut builder = crate::build::Builder::new(&ir, StepSchema::Ap242Edition3);
    builder.build();
    assert_eq!(builder.links.body_item_refs[body.as_str()].len(), 2);
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
        magnitude: cadmpeg_ir::pmi::PmiValue {
            value: 0.1,
            quantity: cadmpeg_ir::pmi::PmiQuantity::Length,
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
    let exchange = crate::parse::parse(&output).unwrap();
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
    let validation = cadmpeg_ir::validate::validate(&ir, Vec::new());
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
    let validation = cadmpeg_ir::validate::validate(&result.ir, result.report.losses.clone());
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
    assert_eq!(report.total_entities, buf_line_count(&buf));
    assert_eq!(report.entity_counts.get("ADVANCED_FACE"), Some(&6));
    assert_eq!(report.entity_counts.get("VERTEX_POINT"), Some(&8));
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
}

#[test]
fn subds_tessellations_and_source_associations_are_reported_as_losses() {
    let source_object = cadmpeg_ir::provenance::SourceObjectAssociation {
        format: "test".into(),
        object_id: "object-0".into(),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let mut ir = unit_cube();
    ir.model.subds.push(cadmpeg_ir::subd::SubdSurface {
        id: cadmpeg_ir::ids::SubdId("test:step:subd#0".into()),
        scheme: cadmpeg_ir::subd::SubdScheme::CatmullClark,
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
        loss.category == cadmpeg_ir::report::LossCategory::Geometry
            && loss.severity == cadmpeg_ir::report::Severity::Warning
            && loss
                .message
                .contains("1 subdivision surface(s) were omitted")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.category == cadmpeg_ir::report::LossCategory::Geometry
            && loss.severity == cadmpeg_ir::report::Severity::Warning
            && loss
                .message
                .contains("1 tessellation(s) require an AP242 target")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.category == cadmpeg_ir::report::LossCategory::Metadata
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
        loss.category == cadmpeg_ir::report::LossCategory::Geometry
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
        loss.category == cadmpeg_ir::report::LossCategory::Geometry
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
        loss.category == cadmpeg_ir::report::LossCategory::Geometry
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
        loss.category == cadmpeg_ir::report::LossCategory::Geometry
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
        vec![cadmpeg_ir::native::NativeRecord {
            id: "asm-history-0".into(),
            fields: Default::default(),
        }],
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
        vec![cadmpeg_ir::native::NativeRecord {
            id: "asm-history-0".into(),
            fields: Default::default(),
        }],
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
        properties: Default::default(),
        textures: Vec::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:appearance-binding#face".to_string(),
        target: AppearanceTarget::Face(face),
        appearance: AppearanceId("test:appearance#black".to_string()),
        source_entity_id: None,
        object_type: None,
        channels: Default::default(),
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
        properties: Default::default(),
        textures: Vec::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:appearance-binding#face".to_string(),
        target: AppearanceTarget::Face(face),
        appearance: AppearanceId("test:appearance#black".to_string()),
        source_entity_id: None,
        object_type: None,
        channels: Default::default(),
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
    let mut per_style: std::collections::BTreeMap<String, usize> = Default::default();
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

/// S0 golden net: SHA-256-pinned STEP writer output. The deep-modules refactor
/// moves code across module boundaries inside `cadmpeg-step`; a behavior-
/// preserving refactor needs an anchor independent of the internal structure
/// being changed. This freezes `write_step` output as sha256 digests pinned
/// in-source, so every later writer diff is a byte-exact regression gate rather
/// than a re-review.
///
/// Two surfaces are pinned. `WRITER_GOLDENS` freezes the two IR example fixtures
/// (`unit_cube`, `directed_subd_sum`) across all six `StepSchema` targets under
/// otherwise-default `StepWriteOptions` (empty timestamp is written as the fixed
/// `1970-01-01T00:00:00` epoch, so output is deterministic). `ROUNDTRIP_GOLDENS`
/// freezes every `tests/fixtures/*.p21` fixture decoded and re-written under
/// `Ap242Edition3` and `Ap214`; a fixture that cannot round-trip (decode
/// refuses, or a write refuses) pins the refusal string instead of a hash, which
/// freezes the success/refusal boundary as coverage.
///
/// Digests use `cadmpeg_ir::wire::hash::sha256_hex` (the workspace sha256 helper the
/// freecad golden harness uses) rather than a fresh `sha2` dev-dependency, so no
/// `Cargo.toml`/`Cargo.lock` change is needed. Regenerate the pinned tables with
/// `cargo test -p cadmpeg-step golden::print_goldens -- --ignored --nocapture`
/// and paste the printed rows back into the two `const` tables.
mod golden {
    use std::io::Cursor;

    use cadmpeg_ir::codec::{CodecEntry, DecodeOptions};
    use cadmpeg_ir::wire::hash::sha256_hex;
    use cadmpeg_ir::CadIr;

    use crate::{write_step, StepCodec, StepSchema, StepWriteOptions};

    /// The six application-protocol targets, paired with the label used as the
    /// table key. Covers `Ap203Edition1..=Ap242Edition3`.
    const SCHEMAS: [(&str, StepSchema); 6] = [
        ("Ap203Edition1", StepSchema::Ap203Edition1),
        ("Ap203Edition2", StepSchema::Ap203Edition2),
        ("Ap214", StepSchema::Ap214),
        ("Ap242Edition1", StepSchema::Ap242Edition1),
        ("Ap242Edition2", StepSchema::Ap242Edition2),
        ("Ap242Edition3", StepSchema::Ap242Edition3),
    ];

    /// Round-trip schema targets: the AP242 edition-3 long form and AP214.
    const ROUNDTRIP_SCHEMAS: [(&str, StepSchema); 2] = [
        ("Ap242Edition3", StepSchema::Ap242Edition3),
        ("Ap214", StepSchema::Ap214),
    ];

    /// Default writer options with only `schema` overridden. The empty default
    /// timestamp pins the header epoch, so digests are stable across runs.
    fn options(schema: StepSchema) -> StepWriteOptions {
        StepWriteOptions {
            schema,
            ..StepWriteOptions::default()
        }
    }

    /// The two IR example fixtures, paired with the label used as the table key.
    fn examples() -> [(&'static str, CadIr); 2] {
        [
            ("unit_cube", cadmpeg_ir::examples::unit_cube()),
            (
                "directed_subd_sum",
                cadmpeg_ir::examples::directed_subd_sum(),
            ),
        ]
    }

    /// Every `tests/fixtures/*.p21` fixture, embedded, paired with its stem.
    fn fixtures() -> Vec<(&'static str, &'static [u8])> {
        macro_rules! fixture {
            ($name:literal) => {
                (
                    $name,
                    include_bytes!(concat!("../tests/fixtures/", $name, ".p21")) as &'static [u8],
                )
            };
        }
        vec![
            fixture!("ap203_sheet"),
            fixture!("ap214_sheet"),
            fixture!("ap242_assembly"),
            fixture!("ap242_conversion_units"),
            fixture!("ap242_degree_cone"),
            fixture!("ap242_ed3_sections"),
            fixture!("ap242_external_documents"),
            fixture!("ap242_geometric_set"),
            fixture!("ap242_geometry"),
            fixture!("ap242_mapped_assembly"),
            fixture!("ap242_minimal"),
            fixture!("ap242_presentation_pmi"),
            fixture!("ap242_semantic_pmi"),
            fixture!("ap242_tessellation"),
            fixture!("ap242_vertex_loop"),
            fixture!("complex_instance"),
            fixture!("strings"),
        ]
    }

    /// sha256 of `write_step(ir, opts(schema))`. Under the default `Report`
    /// policy with a `Vec` sink, `write_step` cannot refuse, so an example golden
    /// is always a hash.
    fn writer_hash(ir: &CadIr, schema: StepSchema) -> String {
        let mut buf = Vec::new();
        write_step(ir, &mut buf, &options(schema))
            .expect("writer golden: write_step under Report policy with a Vec sink cannot fail");
        sha256_hex(&buf)
    }

    /// A round-trip outcome: either a sha256 of the re-written STEP bytes, or the
    /// decode/write refusal that stands in for it. A pinned refusal freezes the
    /// success/refusal boundary and counts as coverage.
    enum Outcome {
        Hash(String),
        Refusal(String),
    }

    impl Outcome {
        /// Compares a computed outcome against a pinned row.
        fn matches(&self, pin: &Pin) -> bool {
            match (self, pin) {
                (Outcome::Hash(a), Pin::Hash(b)) => a == b,
                (Outcome::Refusal(a), Pin::Refusal(b)) => a == b,
                _ => false,
            }
        }

        /// Renders a computed outcome as the `Pin` source literal it should pin to.
        fn as_pin_literal(&self) -> String {
            match self {
                Outcome::Hash(hash) => format!("Pin::Hash(\"{hash}\")"),
                Outcome::Refusal(message) => format!("Pin::Refusal({message:?})"),
            }
        }
    }

    /// A pinned round-trip row: a fixed hash or a fixed refusal string. Every
    /// current fixture round-trips under both schemas, so `ROUNDTRIP_GOLDENS`
    /// holds only `Hash` rows today; `Refusal` is retained because pinning a
    /// decode/write refusal is the designed way to freeze the success/refusal
    /// boundary, and `print_goldens` emits it verbatim if a fixture starts
    /// refusing.
    #[allow(dead_code)]
    enum Pin {
        Hash(&'static str),
        Refusal(&'static str),
    }

    impl Pin {
        fn describe(&self) -> String {
            match self {
                Pin::Hash(hash) => format!("Hash({hash})"),
                Pin::Refusal(message) => format!("Refusal({message})"),
            }
        }
    }

    /// Decodes `bytes` with `StepCodec`, then re-writes under `schema`. Decode or
    /// write failure becomes a pinned refusal.
    fn roundtrip_outcome(bytes: &[u8], schema: StepSchema) -> Outcome {
        let decoded =
            match StepCodec::default().decode(&mut Cursor::new(bytes), &DecodeOptions::default()) {
                Ok(result) => result,
                Err(err) => return Outcome::Refusal(format!("decode: {err}")),
            };
        let mut buf = Vec::new();
        match write_step(&decoded.ir, &mut buf, &options(schema)) {
            Ok(_) => Outcome::Hash(sha256_hex(&buf)),
            Err(err) => Outcome::Refusal(format!("write: {err}")),
        }
    }

    /// `(example, schema, sha256)`. 2 examples x 6 schemas = 12 rows.
    const WRITER_GOLDENS: &[(&str, &str, &str)] = &[
        (
            "unit_cube",
            "Ap203Edition1",
            "41fb7c0805b8dcf80c2c0f9621e3e43135870db45579272563055498ea75fafb",
        ),
        (
            "unit_cube",
            "Ap203Edition2",
            "7044a4af4816496d1dec2814a3ab131d532de414347b8cc6d3ebe3619a6e2d41",
        ),
        (
            "unit_cube",
            "Ap214",
            "a4443f1d97cdc9bb4651cbe046e70f8cf899de1980aaeeeeb16a6adecbea7e7d",
        ),
        (
            "unit_cube",
            "Ap242Edition1",
            "3e6180609d599f91d36ba6a248c271a868efa98b198d769cf75d4cd6f415c35f",
        ),
        (
            "unit_cube",
            "Ap242Edition2",
            "a4f4bd0c26045bf3685c7a3c657588d1650fff3f6862c79aef5e921dc1bbed0c",
        ),
        (
            "unit_cube",
            "Ap242Edition3",
            "007928e140281aa1f057901c28c685f67bf70601ac1d8eeb41404c75500a2a14",
        ),
        (
            "directed_subd_sum",
            "Ap203Edition1",
            "e16ed09b646d497d74f4f9b87f2abc2f69897b6b7362942cd984d2798e476b2a",
        ),
        (
            "directed_subd_sum",
            "Ap203Edition2",
            "9fe1e2bc775ad9875d85f020d5e81cf05056b9a9a0e0daed4874044fca0e25a8",
        ),
        (
            "directed_subd_sum",
            "Ap214",
            "9eb3590fc989eee82af4345cde4ffabbbc510bfc10e0879fabd9bb7a03849abb",
        ),
        (
            "directed_subd_sum",
            "Ap242Edition1",
            "553d717fe73dc6e7757b587187c2c8b91b6dfed0ce100243064686f926f70453",
        ),
        (
            "directed_subd_sum",
            "Ap242Edition2",
            "dc24c76c432a1764e8abd1e0330256f266fa8b8598cdd62bbbaa1f14094de91e",
        ),
        (
            "directed_subd_sum",
            "Ap242Edition3",
            "8f564ebb79d21a4ca349482128c95b5f53e816d1413a4d03a1f61f96fccf4e02",
        ),
    ];

    /// `(fixture, schema, pin)`. 17 fixtures x 2 schemas = 34 rows.
    const ROUNDTRIP_GOLDENS: &[(&str, &str, Pin)] = &[
        (
            "ap203_sheet",
            "Ap242Edition3",
            Pin::Hash("bc2b6db905a538aa009ce570bb5e25ec15a9f716afb9ef23a2e44884d608921c"),
        ),
        (
            "ap203_sheet",
            "Ap214",
            Pin::Hash("262733db93517ea047fc0fd2c455846f9dbb1d91809f38f9ff95510a808eb60a"),
        ),
        (
            "ap214_sheet",
            "Ap242Edition3",
            Pin::Hash("cd15a680903ef1d2031efef28a3261c762aaae8e508a925f0d1d247b80ef9312"),
        ),
        (
            "ap214_sheet",
            "Ap214",
            Pin::Hash("8ca5515e58ff53da0d8eb25bdabb1593122d5db85b57562ad64e385e3825aa16"),
        ),
        (
            "ap242_assembly",
            "Ap242Edition3",
            Pin::Hash("6c39aea7adb6f31a91e3ba40a3452920fd9659186723cea703a12b082ea448db"),
        ),
        (
            "ap242_assembly",
            "Ap214",
            Pin::Hash("0bc6646678bdc298e713a68d76b72ae0bf70d19cdc24650bcca8d67bc6fc34ba"),
        ),
        (
            "ap242_conversion_units",
            "Ap242Edition3",
            Pin::Hash("0f916b8c468b0fa312fc83c587b8ede9173785cb6cd55cee485d31552259f6c2"),
        ),
        (
            "ap242_conversion_units",
            "Ap214",
            Pin::Hash("d88626987d4667b6bdb5c1c2927406b94dab6fa95b320660d1e6187c8964e045"),
        ),
        (
            "ap242_degree_cone",
            "Ap242Edition3",
            Pin::Hash("443cb87a8b42437ee254121b43e1aeaf6b173519ccfaf0309ae7f6dbc0775056"),
        ),
        (
            "ap242_degree_cone",
            "Ap214",
            Pin::Hash("1436652fc776210992355d0e16ea962aed363a00c0d8f55447a7302bf78b4a07"),
        ),
        (
            "ap242_ed3_sections",
            "Ap242Edition3",
            Pin::Hash("e13df86a5c42928f02c07765d14d7be70b749941f794b025adc9b330b0dbe1c6"),
        ),
        (
            "ap242_ed3_sections",
            "Ap214",
            Pin::Hash("aeb71ec93a835d9d4dfeba55496f4a1297ac6d27df9e8db68bc5cce27c4cdbbc"),
        ),
        (
            "ap242_external_documents",
            "Ap242Edition3",
            Pin::Hash("44bd5837446ba4d2130193bb75ed4b3c3a02b7eb4fec175e2762e46829f707ea"),
        ),
        (
            "ap242_external_documents",
            "Ap214",
            Pin::Hash("2f9030385592bb5f892d40d5cd20a29afa64871e9c37c161238ac0fca05d053a"),
        ),
        (
            "ap242_geometric_set",
            "Ap242Edition3",
            Pin::Hash("c55f54e3aa8cbd32e703b756d7d8f758e106bad2da6dd8924e97fcbdb4b5d253"),
        ),
        (
            "ap242_geometric_set",
            "Ap214",
            Pin::Hash("97cc2e43c94b59ea9c1b28d0aa283bc252a71cd84de1839d5112b61294da30d5"),
        ),
        (
            "ap242_geometry",
            "Ap242Edition3",
            Pin::Hash("c20f42e2a26c9950c5d2d867968794dab8f681fc127ade6832ab0411bbee9d74"),
        ),
        (
            "ap242_geometry",
            "Ap214",
            Pin::Hash("9424c1de3c6984ffe03843993011056beefd6c91797b19c887e16e20c3e84f18"),
        ),
        (
            "ap242_mapped_assembly",
            "Ap242Edition3",
            Pin::Hash("780615c9429e279473cfa093946da505d35a3b2cb0f39261969ffc3b9e2f11e7"),
        ),
        (
            "ap242_mapped_assembly",
            "Ap214",
            Pin::Hash("fe15af32959595f507ce2f02093f01decf3108faf30510e37b8626b4887d165b"),
        ),
        (
            "ap242_minimal",
            "Ap242Edition3",
            Pin::Hash("44bd5837446ba4d2130193bb75ed4b3c3a02b7eb4fec175e2762e46829f707ea"),
        ),
        (
            "ap242_minimal",
            "Ap214",
            Pin::Hash("2f9030385592bb5f892d40d5cd20a29afa64871e9c37c161238ac0fca05d053a"),
        ),
        (
            "ap242_presentation_pmi",
            "Ap242Edition3",
            Pin::Hash("7b7fe445171424c4a93d23b1bd34f03e8ac64dd2cf071e1a7aeadf3b9001f3fa"),
        ),
        (
            "ap242_presentation_pmi",
            "Ap214",
            Pin::Hash("2f9030385592bb5f892d40d5cd20a29afa64871e9c37c161238ac0fca05d053a"),
        ),
        (
            "ap242_semantic_pmi",
            "Ap242Edition3",
            Pin::Hash("75fd6f1c55b7a82b4d2e308bf048df440d50a3272a002234b82d4d3534e7663d"),
        ),
        (
            "ap242_semantic_pmi",
            "Ap214",
            Pin::Hash("2f9030385592bb5f892d40d5cd20a29afa64871e9c37c161238ac0fca05d053a"),
        ),
        (
            "ap242_tessellation",
            "Ap242Edition3",
            Pin::Hash("bf2c4d3cf5498c8782b7fd41e83fd79be47a941a9908c9501f6ed026582429b4"),
        ),
        (
            "ap242_tessellation",
            "Ap214",
            Pin::Hash("2fc86fb810cd276ee64b01ba556c1315c093621b3a132a2061dfcf046763cbb9"),
        ),
        (
            "ap242_vertex_loop",
            "Ap242Edition3",
            Pin::Hash("33bb68af326308714d6708f0c2ae06f66f7e27b92a26df084319068ff02dd7e6"),
        ),
        (
            "ap242_vertex_loop",
            "Ap214",
            Pin::Hash("6172340a047123bdfa087bde94e80cb38af22ca3d534c5c45676cd0c91a32495"),
        ),
        (
            "complex_instance",
            "Ap242Edition3",
            Pin::Hash("887b2574d978a876af85956c6ddf34a28767b56bfbcdd08361f0e81f773ee6a4"),
        ),
        (
            "complex_instance",
            "Ap214",
            Pin::Hash("79f93adfa3955c0e8a961aa93f0f999162ea938cd9c4c8d4fcbc56ab1e93754d"),
        ),
        (
            "strings",
            "Ap242Edition3",
            Pin::Hash("44bd5837446ba4d2130193bb75ed4b3c3a02b7eb4fec175e2762e46829f707ea"),
        ),
        (
            "strings",
            "Ap214",
            Pin::Hash("2f9030385592bb5f892d40d5cd20a29afa64871e9c37c161238ac0fca05d053a"),
        ),
    ];

    fn writer_pin(example: &str, schema: &str) -> Option<&'static str> {
        WRITER_GOLDENS
            .iter()
            .find(|(e, s, _)| *e == example && *s == schema)
            .map(|(_, _, hash)| *hash)
    }

    fn roundtrip_pin(fixture: &str, schema: &str) -> Option<&'static Pin> {
        ROUNDTRIP_GOLDENS
            .iter()
            .find(|(f, s, _)| *f == fixture && *s == schema)
            .map(|(_, _, pin)| pin)
    }

    /// Every example x schema must hash to its pinned writer digest.
    #[test]
    fn writer_goldens_match() {
        assert_eq!(
            WRITER_GOLDENS.len(),
            examples().len() * SCHEMAS.len(),
            "writer golden table size must equal examples x schemas"
        );
        let mut failures = Vec::new();
        for (example, ir) in examples() {
            for (label, schema) in SCHEMAS {
                let actual = writer_hash(&ir, schema);
                match writer_pin(example, label) {
                    Some(expected) if expected == actual => {}
                    Some(expected) => failures.push(format!(
                        "writer `{example}` / `{label}`: pinned {expected} != actual {actual}"
                    )),
                    None => failures.push(format!("writer `{example}` / `{label}`: no pinned row")),
                }
            }
        }
        assert!(
            failures.is_empty(),
            "{} writer golden(s) drifted; regenerate with `cargo test -p cadmpeg-step golden::print_goldens -- --ignored --nocapture`:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    /// Every fixture x schema must match its pinned round-trip outcome (hash or
    /// refusal).
    #[test]
    fn roundtrip_goldens_match() {
        assert_eq!(
            ROUNDTRIP_GOLDENS.len(),
            fixtures().len() * ROUNDTRIP_SCHEMAS.len(),
            "round-trip golden table size must equal fixtures x schemas"
        );
        let mut failures = Vec::new();
        for (fixture, bytes) in fixtures() {
            for (label, schema) in ROUNDTRIP_SCHEMAS {
                let actual = roundtrip_outcome(bytes, schema);
                match roundtrip_pin(fixture, label) {
                    Some(pin) if actual.matches(pin) => {}
                    Some(pin) => failures.push(format!(
                        "roundtrip `{fixture}` / `{label}`: pinned {} != actual {}",
                        pin.describe(),
                        actual.as_pin_literal()
                    )),
                    None => {
                        failures.push(format!("roundtrip `{fixture}` / `{label}`: no pinned row"))
                    }
                }
            }
        }
        assert!(
            failures.is_empty(),
            "{} round-trip golden(s) drifted; regenerate with `cargo test -p cadmpeg-step golden::print_goldens -- --ignored --nocapture`:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    /// `write_step` output must be byte-identical across two runs with identical
    /// options. Covers the pure-writer path (example IR, no decode) and the
    /// decoded-fixture path (decode once, write twice) so the check isolates
    /// writer nondeterminism from decode nondeterminism.
    #[test]
    fn write_step_output_is_deterministic() {
        for (example, ir) in examples() {
            for (label, schema) in SCHEMAS {
                let mut first = Vec::new();
                let mut second = Vec::new();
                write_step(&ir, &mut first, &options(schema)).expect("first write");
                write_step(&ir, &mut second, &options(schema)).expect("second write");
                assert!(
                    first == second,
                    "writer `{example}` / `{label}`: write_step is nondeterministic across two runs"
                );
            }
        }
        for (fixture, bytes) in fixtures() {
            let Ok(decoded) =
                StepCodec::default().decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            else {
                continue;
            };
            for (label, schema) in ROUNDTRIP_SCHEMAS {
                let mut first = Vec::new();
                let mut second = Vec::new();
                if write_step(&decoded.ir, &mut first, &options(schema)).is_err() {
                    continue;
                }
                write_step(&decoded.ir, &mut second, &options(schema)).expect("second write");
                assert!(
                    first == second,
                    "roundtrip `{fixture}` / `{label}`: write_step is nondeterministic across two runs"
                );
            }
        }
    }

    /// Regeneration path (not run by default). Prints both pinned tables with
    /// freshly computed values, ready to paste into the `const` tables above.
    #[test]
    #[ignore = "regeneration helper; run with --ignored --nocapture"]
    fn print_goldens() {
        println!("\n// ==== WRITER_GOLDENS ====");
        for (example, ir) in examples() {
            for (label, schema) in SCHEMAS {
                println!(
                    "        (\"{example}\", \"{label}\", \"{}\"),",
                    writer_hash(&ir, schema)
                );
            }
        }
        println!("\n// ==== ROUNDTRIP_GOLDENS ====");
        for (fixture, bytes) in fixtures() {
            for (label, schema) in ROUNDTRIP_SCHEMAS {
                println!(
                    "        (\"{fixture}\", \"{label}\", {}),",
                    roundtrip_outcome(bytes, schema).as_pin_literal()
                );
            }
        }
        println!();
    }
}
