// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]
use super::*;
use crate::loss::StepLossCode;

#[test]
fn byte_accounting_reports_an_unrecognized_suffix() {
    let input = include_bytes!("../../tests/fixtures/ap242_minimal.p21");
    let (mut exchange, _) = crate::parse::parse(input).expect("parse accounting fixture");
    let mut extended = input.to_vec();
    extended.push(0xc3);

    let accounting = byte_accounting(&extended, &exchange, &HashSet::new(), None)
        .expect("byte accounting allocation");

    assert_eq!(accounting.unclassified, 1);
    assert_eq!(
        accounting.structural + accounting.typed + accounting.opaque + accounting.unclassified,
        extended.len()
    );

    let result = decode_exchange_mode(
        &extended,
        cadmpeg_ir::codec::DecodeOptions::default(),
        &mut exchange,
        &[],
        true,
        None,
    )
    .expect("synthesized unknown record conversion")
    .0;
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::ByteAccountingUnclassified.kind()
            && loss.severity == cadmpeg_ir::Severity::Error
            && loss.message.contains("1 byte(s) unclassified")
    }));
}

#[test]
fn byte_accounting_claims_controls_inside_print_directives() {
    let input = b"1\\\x01N\x02\\2";
    let mut classes = vec![ByteClass::Unclassified; input.len()];

    claim_trivia(input, 1..input.len(), &mut classes);

    assert!(classes[1..6]
        .iter()
        .all(|class| *class == ByteClass::Structural));
}

#[test]
fn semantic_work_counts_nested_source_graph_nodes() {
    let simple = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let nested = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(((1,2),TYPE((3,4))));ENDSEC;END-ISO-10303-21;";
    let (simple_exchange, _) = crate::parse::parse(simple).expect("simple exchange");
    let (nested_exchange, _) = crate::parse::parse(nested).expect("nested exchange");

    assert!(semantic_input_work(&nested_exchange) > semantic_input_work(&simple_exchange));
}

#[test]
fn implicit_face_plane_work_scales_with_point_count() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=POLY_LOOP('',(#2,#3,#4,#5));#2=ITEM();#3=ITEM();#4=ITEM();#5=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("polygon exchange");

    assert_eq!(implicit_face_plane_work(&exchange), 4);
}

use std::fmt::Write as _;
use std::io::Cursor;

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::ids::CurveId;

use crate::test_support::decode_inline;
use crate::StepCodec;

#[test]
fn semantic_decode_uses_the_decode_session_work_budget() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let mut semantic_operation = None;
    for max_work_units in 1..=2048 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_work_units = max_work_units;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits the test policy");
        let error = crate::reader::decode(source, DecodeOptions::default(), &ctx)
            .expect_err("a small work budget must refuse one decode stage");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if !matches!(
            limit.context.operation,
            "step_lex_token"
                | "step_parse_record"
                | "step_parse_parameter"
                | "step_anchor_materialization"
                | "step_reference_materialization"
        ) {
            semantic_operation = Some(limit.context.operation);
            break;
        }
    }
    assert_eq!(semantic_operation, Some("step_geometry_decode"));
}

#[test]
fn semantic_decode_admits_ir_entities_at_stage_boundaries() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;#1=CARTESIAN_POINT('',(1.,2.,3.));#2=VERTEX_POINT('',#1);ENDSEC;END-ISO-10303-21;";
    let mut entity_limit = None;
    for max_entities in 1..=64 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_entities = max_entities;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits the test policy");
        let error = crate::reader::decode(source, DecodeOptions::default(), &ctx)
            .expect_err("a model entity must be admitted before the next semantic stage");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
            && limit.context.operation == "step_dependency_decode"
        {
            entity_limit = Some(limit);
            break;
        }
    }
    let limit = entity_limit.expect("IR entities must be charged at a semantic boundary");
    assert_eq!(limit.additional, 1);
    assert!(limit.used <= limit.limit);
}

#[test]
fn implicit_face_plane_work_is_charged_before_plane_inference() {
    let point_references = (2..=17)
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(",");
    let point_records = (2..=17).fold(String::new(), |mut records, id| {
        writeln!(records, "#{id}=CARTESIAN_POINT('',({id}.,0.,0.));").expect("write point fixture");
        records
    });
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=POLY_LOOP('',({point_references}));{point_records}ENDSEC;END-ISO-10303-21;"
    );
    let mut plane_limit = None;
    for max_work_units in 1..=65_536 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_work_units = max_work_units;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            source.as_bytes(),
            &arena,
            &policy,
        )
        .expect("root fits the test policy");
        let error = crate::reader::decode(source.as_bytes(), DecodeOptions::default(), &ctx)
            .expect_err("bounded implicit-plane work must be refused at some budget");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if limit.context.operation == "step_implicit_face_plane" {
            plane_limit = Some(limit);
            break;
        }
    }
    let limit = plane_limit.expect("implicit face-plane work must have a stable budget gate");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(limit.additional, 16);
    assert!(limit.used <= limit.limit);
}

#[test]
pub(crate) fn decode_preserves_named_opaque_records_with_exact_byte_spans() {
    let bytes = include_bytes!("../../tests/fixtures/ap242_minimal.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode parsed STEP document");

    assert_eq!(result.ir().source.as_ref().unwrap().format(), "step");
    let unknowns = result.ir().native_unknowns("step").unwrap();
    assert_eq!(unknowns.len(), 2);
    assert_eq!(unknowns[0].id.0, "step:data:example_record#1");
    let retained = result
        .source_fidelity()
        .retained_record(&unknowns[0].id.0)
        .expect("opaque payload is retained in source fidelity");
    assert_eq!(
        retained.data.as_deref(),
        Some(&bytes[retained.offset as usize..(retained.offset + retained.byte_len) as usize])
    );
    assert!(unknowns[0]
        .links
        .contains(&"step:data:opaque_target#2".to_string()));
    assert!(!result.report().geometry_transferred());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("EXAMPLE_RECORD")));
}

#[test]
pub(crate) fn decode_retains_signature_opaque_without_verification_result() {
    let bytes = include_bytes!("../signature/tests/data/sg04_openssl_detached.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode signature witness");

    let signature = result
        .ir()
        .native_unknowns("step")
        .unwrap()
        .into_iter()
        .find(|record| record.id.0 == "step:file:signature#0")
        .expect("signature is retained as an opaque source record");
    let retained = result
        .source_fidelity()
        .retained_record(&signature.id.0)
        .expect("signature source fidelity");
    assert_eq!(
        retained.data.as_deref(),
        Some(&bytes[retained.offset as usize..(retained.offset + retained.byte_len) as usize])
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::OpaqueRecordPreserved.kind()
            && loss.message.contains("SIGNATURE")
    }));
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message.contains("signature valid")
            || loss.message.contains("signature invalid")
            || loss.message.contains("signature indeterminate")
    }));
}

#[test]
pub(crate) fn decode_user_defined_entities_as_named_opaque_records() {
    let bytes = include_bytes!("tests/data/ud01_user_defined_entity.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode user-defined entity witness");

    assert_eq!(result.ir().model.entity_count(), 0);
    let unknowns = result.ir().native_unknowns("step").unwrap();
    assert_eq!(unknowns.len(), 2);

    let target = unknowns
        .iter()
        .find(|record| record.id.0 == "step:data:!vendor_target#1")
        .expect("user-defined target record");
    assert!(target.links.is_empty());
    let target_source = result
        .source_fidelity()
        .retained_record(&target.id.0)
        .expect("retained user-defined target span");
    assert_eq!(
        target_source.data.as_deref(),
        Some(b"#1=!VENDOR_TARGET('target');".as_slice())
    );

    let entity = unknowns
        .iter()
        .find(|record| record.id.0 == "step:data:!vendor_entity#2")
        .expect("user-defined entity record");
    assert_eq!(entity.links, vec!["step:data:!vendor_target#1".to_string()]);
    let entity_source = result
        .source_fidelity()
        .retained_record(&entity.id.0)
        .expect("retained user-defined entity span");
    assert_eq!(
        entity_source.data.as_deref(),
        Some(b"#2=!VENDOR_ENTITY('vendor payload',#1,!VENDOR_TYPE((#1)));".as_slice())
    );

    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            .contains("!VENDOR_ENTITY instance(s) as named opaque STEP records")
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.findings.is_empty(), "{:#?}", validation.findings);
}

#[test]
fn opaque_links_retain_typed_step_targets() {
    let result = decode_inline(
        "#1=EXAMPLE_RECORD('',#2);
        #2=LINE('typed target',#3,#5);
        #3=CARTESIAN_POINT('',(0.,0.,0.));
        #4=DIRECTION('',(1.,0.,0.));
        #5=VECTOR('',#4,1.);",
    );
    let unknowns = result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown records");
    assert_eq!(unknowns.len(), 1);
    assert_eq!(unknowns[0].links, vec!["step:data:curve#2".to_string()]);
}

#[test]
fn opaque_links_retain_fallback_carrier_targets() {
    let result = decode_inline(
        "#1=TRIMMED_CURVE('',#99,(0.),(1.),.T.,.PARAMETER.);
         #2=EXAMPLE_RECORD('',#1);
         #99=EXAMPLE_RECORD('missing basis');",
    );
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id.0 == "step:data:curve#1"));

    let unknowns = result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown records");
    let example = unknowns
        .iter()
        .find(|record| record.id.0 == "step:data:example_record#2")
        .expect("opaque record referencing fallback carrier");
    assert!(example.links.contains(&"step:data:curve#1".to_string()));

    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(!validation.findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::CarrierReachability
            && finding.entity.as_deref() == Some("step:data:curve#1")
    }));
}

#[test]
pub(crate) fn decode_accounts_for_every_part21_byte() {
    let bytes = include_bytes!("../../tests/fixtures/ap242_semantic_pmi.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode byte-accounting fixture");
    let attributes = &result.ir().source.as_ref().unwrap().attributes;
    let count = |name: &str| attributes[name].parse::<usize>().unwrap();

    assert!(count("bytes_structural") > 0);
    assert!(count("bytes_typed") > 0);
    assert_eq!(count("bytes_named_opaque"), 0);
    assert_eq!(count("bytes_unclassified"), 0);
    assert_eq!(
        count("bytes_structural")
            + count("bytes_typed")
            + count("bytes_named_opaque")
            + count("bytes_unclassified"),
        bytes.len()
    );
}

#[test]
fn every_repository_step_fixture_has_complete_byte_accounting() {
    let fixtures: &[(&str, &[u8])] = &[
        (
            "ap203_sheet",
            include_bytes!("../../tests/fixtures/ap203_sheet.p21"),
        ),
        (
            "ap214_sheet",
            include_bytes!("../../tests/fixtures/ap214_sheet.p21"),
        ),
        (
            "ap242_assembly",
            include_bytes!("../../tests/fixtures/ap242_assembly.p21"),
        ),
        (
            "ap242_conversion_units",
            include_bytes!("../../tests/fixtures/ap242_conversion_units.p21"),
        ),
        (
            "ap242_ed3_sections",
            include_bytes!("../../tests/fixtures/ap242_ed3_sections.p21"),
        ),
        (
            "ap242_degree_cone",
            include_bytes!("../../tests/fixtures/ap242_degree_cone.p21"),
        ),
        (
            "ap242_external_documents",
            include_bytes!("../../tests/fixtures/ap242_external_documents.p21"),
        ),
        (
            "ap242_geometry",
            include_bytes!("../../tests/fixtures/ap242_geometry.p21"),
        ),
        (
            "ap242_geometric_set",
            include_bytes!("../../tests/fixtures/ap242_geometric_set.p21"),
        ),
        (
            "ap242_mapped_assembly",
            include_bytes!("../../tests/fixtures/ap242_mapped_assembly.p21"),
        ),
        (
            "ap242_minimal",
            include_bytes!("../../tests/fixtures/ap242_minimal.p21"),
        ),
        (
            "ap242_presentation_pmi",
            include_bytes!("../../tests/fixtures/ap242_presentation_pmi.p21"),
        ),
        (
            "ap242_semantic_pmi",
            include_bytes!("../../tests/fixtures/ap242_semantic_pmi.p21"),
        ),
        (
            "ap242_tessellation",
            include_bytes!("../../tests/fixtures/ap242_tessellation.p21"),
        ),
        (
            "ap242_vertex_loop",
            include_bytes!("../../tests/fixtures/ap242_vertex_loop.p21"),
        ),
        (
            "complex_instance",
            include_bytes!("../../tests/fixtures/complex_instance.p21"),
        ),
        (
            "strings",
            include_bytes!("../../tests/fixtures/strings.p21"),
        ),
    ];
    for &(name, bytes) in fixtures {
        let result = StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let attributes = &result.ir().source.as_ref().unwrap().attributes;
        let count = |key: &str| attributes[key].parse::<usize>().unwrap();
        assert_eq!(count("bytes_unclassified"), 0, "{name}");
        assert_eq!(
            count("bytes_structural")
                + count("bytes_typed")
                + count("bytes_named_opaque")
                + count("bytes_unclassified"),
            bytes.len(),
            "{name}"
        );
    }
}

#[test]
fn unowned_pcurve_dependencies_are_retained_as_one_opaque_closure() {
    let source = String::from_utf8(include_bytes!("../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#69=PCURVE('',#28,#70);\n#70=DEFINITIONAL_REPRESENTATION('',(#71),#50);\n#71=LINE('',#51,#53);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode unowned pcurve");
    let unknowns = decoded
        .ir()
        .native_unknowns("step")
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
    assert!(decoded
        .source_fidelity()
        .retained_record(&line.id.0)
        .expect("unowned pcurve line payload is retained")
        .data
        .as_deref()
        .is_some_and(|data| data.starts_with(b"#71=LINE")));
    assert!(decoded
        .ir()
        .model
        .pcurves
        .iter()
        .all(|pcurve| pcurve.id.as_str() != "step:data:pcurve#69"));
}

#[test]
fn a_protected_unowned_pcurve_stays_opaque() {
    let source = String::from_utf8(include_bytes!("../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#19=EDGE_CURVE('',#6,#7,#57,.T.);",
            "#19=EDGE_CURVE('',#6,#7,#72,.T.);",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#69=PCURVE('',#28,#55);\n#72=TRIMMED_CURVE('',#16,(#69,0.),(#69,10.),.T.,.PARAMETER.);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode protected unowned pcurve");
    assert!(decoded
        .ir()
        .model
        .pcurves
        .iter()
        .all(|pcurve| { pcurve.id.as_str() != "step:data:pcurve#69" }));
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("protected_pcurves=1")));
    let unknowns = decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    assert!(unknowns
        .iter()
        .any(|record| record.id.0 == "step:data:pcurve#69"));
}

#[test]
fn failed_mandatory_point_root_remains_opaque_and_unbound() {
    let source = String::from_utf8(include_bytes!("../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#3=CARTESIAN_POINT('',(0.,0.,0.));",
            "#3=UNSUPPORTED_POINT('',(0.,0.,0.));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode source with unsupported mandatory vertex point");

    assert!(decoded.ir().model.bodies.is_empty());
    let unknowns = decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    assert!(unknowns
        .iter()
        .any(|record| record.id.0 == "step:data:unsupported_point#3"));
    assert!(unknowns
        .iter()
        .any(|record| record.id.0 == "step:data:shell_based_surface_model#31"));
    assert!(decoded.report().losses.iter().any(|loss| loss
        .message
        .contains("STEP topology root #31 rejected: vertex point #3")));
}

#[test]
fn unsupported_invisibility_relation_is_retained_as_opaque() {
    let decoded = decode_inline(
        "#1=INVISIBILITY((#2));
         #2=STYLED_ITEM('',(),#3);
         #3=GEOMETRIC_CURVE_SET('',());",
    );

    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.0 == "step:data:invisibility#1"));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("INVISIBILITY #1 targets unsupported item #2")
    }));
}

#[test]
fn retention_reports_every_deleted_carrier_category() {
    let source = String::from_utf8(include_bytes!("../../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#69=PCURVE('',#78,#70);\n#70=DEFINITIONAL_REPRESENTATION('',(#71,#84),#50);\n#71=LINE('',#51,#53);\n#74=CARTESIAN_POINT('',(20.,20.,0.));\n#75=DIRECTION('',(0.,0.,1.));\n#76=DIRECTION('',(1.,0.,0.));\n#77=AXIS2_PLACEMENT_3D('',#74,#75,#76);\n#78=PLANE('',#77);\n#79=DIRECTION('',(1.,0.,0.));\n#80=VECTOR('',#79,1.);\n#83=LINE('',#74,#80);\n#84=OPAQUE_REFERENCE(#83,#86);\n#86=POLY_LOOP('',(#74));\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode carrier retention fixture");
    let message = decoded
        .report()
        .losses
        .iter()
        .find(|loss| loss.message.contains("unowned STEP carrier retention"))
        .map(|loss| loss.message.as_str())
        .expect("carrier retention report");
    for category in ["deleted pcurves=1", "points=1", "curves=1", "surfaces=1"] {
        assert!(message.contains(category), "missing {category}: {message}");
    }
    assert!(decoded
        .ir()
        .model
        .pcurves
        .iter()
        .all(|pcurve| pcurve.id.as_str() != "step:data:pcurve#69"));
    assert!(decoded
        .ir()
        .model
        .curves
        .iter()
        .all(|curve| curve.id.as_str() != "step:data:curve#83"));
    assert!(decoded
        .ir()
        .model
        .surfaces
        .iter()
        .all(|surface| surface.id.as_str() != "step:data:surface#78"));
    assert!(decoded
        .ir()
        .model
        .points
        .iter()
        .all(|point| point.id.as_str() != "step:data:point#74"));
}

#[test]
fn decode_charges_one_loss_for_an_out_of_range_schema_object_identifier() {
    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AUTOMOTIVE_DESIGN_CC2 { 1 2 10303 214 -1 1 5 4 }'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("an out-of-range component does not refuse the file");

    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::SchemaObjectIdentifierOutOfRange.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].severity, cadmpeg_ir::Severity::Warning);
    assert_eq!(
        losses[0].message,
        "FILE_SCHEMA identifier AUTOMOTIVE_DESIGN_CC2 has an out-of-range object identifier component -1; the object identifier is not admitted"
    );
    let provenance = losses[0].provenance.as_ref().expect("source provenance");
    assert_eq!(provenance.format, "step");
    assert_eq!(
        provenance.offset,
        source.find("FILE_SCHEMA").unwrap() as u64
    );
    assert_eq!(provenance.tag.as_deref(), Some("schema_identifier"));
    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["schema"],
        "AUTOMOTIVE_DESIGN_CC2 { 1 2 10303 214 -1 1 5 4 }"
    );
}

#[test]
fn decode_does_not_charge_a_loss_for_a_valid_schema_object_identifier() {
    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AUTOMOTIVE_DESIGN_CC2 { 1 2 10303 214 1 1 5 4 }'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("valid schema object identifier");
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::SchemaObjectIdentifierOutOfRange.kind()));
}

#[test]
fn decode_reports_the_substituted_grammar_for_an_unknown_implementation_level() {
    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'1;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("the framed implementation-level declaration is recoverable");

    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::ImplementationLevelUnverified.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("1;1"));
    assert!(losses[0].message.contains("4;3 grammar"));
    let provenance = losses[0].provenance.as_ref().expect("source provenance");
    assert_eq!(
        provenance.offset,
        source.find("FILE_DESCRIPTION").unwrap() as u64
    );
    assert_eq!(provenance.tag.as_deref(), Some("implementation_level"));
}

#[test]
fn decode_salvages_noncanonical_complex_partial_order_with_provenance() {
    let bytes = include_bytes!("../../tests/fixtures/noncanonical_solid_angle.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("salvage mode accepts recoverable source order");
    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::ParseNoncanonicalSyntax.kind())
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
    assert_eq!(result.ir().native_unknowns("step").unwrap().len(), 0);
    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["bytes_named_opaque"],
        "0"
    );
}

#[test]
fn strict_decode_rejects_noncanonical_complex_partial_order() {
    let bytes = include_bytes!("../../tests/fixtures/noncanonical_solid_angle.p21");
    let mut options = DecodeOptions::default();
    options.policy.mode = DecodeMode::Strict;
    let error = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &options)
        .expect_err("strict mode rejects noncanonical source order");

    assert!(matches!(
        error,
        cadmpeg_ir::codec::DecodeFailure::StrictRejected { .. }
    ));
}

#[test]
fn strict_decode_rejects_omitted_entity_name_recovery() {
    // The schema is a declared dialect so that the refusal this test asserts is
    // the omitted-name one. A bare 'AP242' declares no registry row and would
    // refuse first on `source.dialect-unverified`.
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));ENDSEC;DATA;#1=CARTESIAN_POINT((0.,0.,0.));ENDSEC;END-ISO-10303-21;";
    let mut options = DecodeOptions::default();
    options.policy.mode = DecodeMode::Strict;
    let error = StepCodec::default()
        .decode(&mut Cursor::new(source), &options)
        .expect_err("strict mode rejects omitted-name recovery");

    assert!(matches!(
        error,
        cadmpeg_ir::codec::DecodeFailure::StrictRejected { .. }
    ));
    assert!(error.to_string().contains("parse.noncanonical-syntax"));
}

#[test]
fn omitted_geometry_names_preserve_intersection_curve_topology() {
    let mut source =
        String::from_utf8(include_bytes!("../../tests/fixtures/ap214_sheet.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace(
                "#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));",
                "#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#69)) REPRESENTATION_CONTEXT('model','3D'));",
            )
            .replace(
                "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
                "#57=INTERSECTION_CURVE(#16,(#56),.PCURVE_S1.);",
            )
            .replace(
                "ENDSEC;\nEND-ISO-10303-21;",
                "#69=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\nENDSEC;\nEND-ISO-10303-21;",
            );
    for (id, entity) in [
        ("3", "CARTESIAN_POINT"),
        ("4", "CARTESIAN_POINT"),
        ("5", "CARTESIAN_POINT"),
        ("6", "VERTEX_POINT"),
        ("7", "VERTEX_POINT"),
        ("8", "VERTEX_POINT"),
        ("9", "DIRECTION"),
        ("10", "DIRECTION"),
        ("11", "DIRECTION"),
        ("12", "DIRECTION"),
        ("13", "VECTOR"),
        ("14", "VECTOR"),
        ("15", "VECTOR"),
        ("16", "LINE"),
        ("17", "LINE"),
        ("18", "LINE"),
        ("19", "EDGE_CURVE"),
        ("20", "EDGE_CURVE"),
        ("21", "EDGE_CURVE"),
        ("22", "ORIENTED_EDGE"),
        ("23", "ORIENTED_EDGE"),
        ("24", "ORIENTED_EDGE"),
        ("25", "EDGE_LOOP"),
        ("26", "FACE_OUTER_BOUND"),
        ("27", "AXIS2_PLACEMENT_3D"),
        ("28", "PLANE"),
        ("29", "ADVANCED_FACE"),
        ("30", "OPEN_SHELL"),
        ("31", "SHELL_BASED_SURFACE_MODEL"),
        ("33", "ORIENTED_OPEN_SHELL"),
        ("51", "CARTESIAN_POINT"),
        ("52", "DIRECTION"),
        ("53", "VECTOR"),
        ("54", "LINE"),
        ("55", "DEFINITIONAL_REPRESENTATION"),
        ("56", "PCURVE"),
    ] {
        let named = format!("#{id}={entity}('',");
        let unnamed = format!("#{id}={entity}(");
        let previous_len = source.len();
        source = source.replace(&named, &unnamed);
        assert!(
            source.len() < previous_len,
            "fixture record #{id} was not converted to omitted-name syntax"
        );
    }

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode omitted-name intersection curve");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    let edge = decoded
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.id.as_str() == "step:data:edge#19")
        .expect("omitted-name intersection edge");
    assert_eq!(
        edge.curve.as_ref().map(CurveId::as_str),
        Some("step:data:curve#16")
    );
    assert!(decoded.ir().model.coedges.iter().any(|coedge| {
        coedge
            .pcurves
            .iter()
            .any(|use_| use_.pcurve.as_str() == "step:data:pcurve#56")
    }));
    let name_loss = decoded
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.code == StepLossCode::ParseNoncanonicalSyntax.kind()
                && loss
                    .message
                    .contains("recovered 37 simple named carrier instance(s)")
        })
        .expect("omitted-name recovery loss");
    assert_eq!(
        name_loss
            .provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref()),
        Some("entity_name")
    );
    assert!(decoded.report().losses.iter().all(|loss| {
        !loss
            .message
            .contains("INTERSECTION_CURVE #57 has no decoded 3D curve")
    }));

    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
