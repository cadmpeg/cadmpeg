// SPDX-License-Identifier: Apache-2.0
//! Self-contained tests: IR documents are built in code (via the IR crate's
//! fixtures or inline), and expected STEP fragments are asserted inline. No test
//! depends on an external STEP consumer.
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};
use cadmpeg_ir::eval::{
    model_curve_point_by_id, model_surface_partials_by_id, model_surface_point_by_id,
};
use cadmpeg_ir::index::ModelIndex;

use crate::ids::StepIdentity;
use cadmpeg_core::decode::{DecodeMode, InspectOptions};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;
use std::fmt::Write as _;
use std::io::Cursor;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::test_support::{decode_inline, export};
use crate::{write_step, StepCodec, StepSchema, StepUnsupportedPolicy, StepWriteOptions};

fn step_zip(entries: &[(&str, &[u8], CompressionMethod)]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for &(name, bytes, method) in entries {
        writer
            .start_file(
                name,
                SimpleFileOptions::default().compression_method(method),
            )
            .unwrap();
        std::io::Write::write_all(&mut writer, bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[test]
fn parser_allows_print_controls_only_outside_anchor_and_reference_sections() {
    let source = b"ISO-10303-\n21;\\N\\HEADER;FILE_DESCRIPTION(('te\\N\\st'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#\n1=ITEM();ENDSEC;END-ISO-10303-21;";
    crate::parse::parse(source).expect("print controls outside restricted sections");

    for section in [
        b"ANCHOR;\\N\\<a>=1;ENDSEC;".as_slice(),
        b"REFERENCE;\\N\\#1=<a>;ENDSEC;".as_slice(),
    ] {
        let source = [
            b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;".as_slice(),
            section,
            b"END-ISO-10303-21;".as_slice(),
        ]
        .concat();
        let error = crate::parse::parse(&source).expect_err("restricted print control");
        assert!(error.to_string().contains("print control directive"));
    }
}

#[test]
fn parser_rejects_excessive_parameter_nesting_without_recursing_unboundedly() {
    let nested = format!("{}1{}", "(".repeat(300), ")".repeat(300));
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM({nested});ENDSEC;END-ISO-10303-21;"
    );
    let error = crate::parse::parse(source.as_bytes()).unwrap_err();
    assert!(error.to_string().contains("nesting exceeds 256 levels"));
}

#[test]
fn parser_uses_the_decode_session_work_budget() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','','',(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_work_units = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("root fits the test policy");
    let error = crate::parse::parse_with_context(source, &ctx).expect_err("budget must refuse");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
    ));
}

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
fn parser_accounts_for_owned_value_storage() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(1);ENDSEC;END-ISO-10303-21;";
    let mut value_storage_limit = None;
    for max_retained_bytes in 1..=4096 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = max_retained_bytes;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits the test policy");
        let error = crate::parse::parse_with_context(source, &ctx)
            .expect_err("owned value storage must consume retained bytes");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if limit.context.operation == "step_parse_value_storage" {
            value_storage_limit = Some(limit);
            break;
        }
    }
    let limit = value_storage_limit.expect("value storage must have a retained-byte gate");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes
    );
    assert!(limit.additional > 0);
    assert!(limit.used <= limit.limit);
}

#[test]
fn parser_accounts_for_record_table_storage() {
    let records = (1..=64).fold(String::new(), |mut records, id| {
        write!(records, "#{id}=ITEM();").expect("write record fixture");
        records
    });
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;"
    );
    crate::parse::parse(source.as_bytes()).expect("record-table fixture must parse");
    let mut record_table_limit = None;
    for max_retained_bytes in (1..=131_072).step_by(64) {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = max_retained_bytes;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            source.as_bytes(),
            &arena,
            &policy,
        )
        .expect("root fits the test policy");
        let error = crate::parse::parse_with_context(source.as_bytes(), &ctx)
            .expect_err("record-table storage must consume retained bytes");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if limit.context.operation == "step_parse_record_table_storage" {
            record_table_limit = Some(limit);
            break;
        }
    }
    let limit = record_table_limit.expect("record-table storage must be charged");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes
    );
    assert!(limit.additional > 0);
    assert!(limit.used <= limit.limit);
}

#[test]
fn parser_accounts_for_anchor_tag_collection_storage() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<a>=1 {tag:2};ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    crate::parse::parse(source).expect("anchor-tag fixture must parse");
    let mut tag_limit = None;
    for max_retained_bytes in 1..=8192 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = max_retained_bytes;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits the test policy");
        let error = crate::parse::parse_with_context(source, &ctx)
            .expect_err("anchor-tag storage must consume retained bytes");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if limit.context.operation == "step_anchor_tag_storage" {
            tag_limit = Some(limit);
            break;
        }
    }
    let limit = tag_limit.expect("anchor-tag storage must have a retained-byte gate");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes
    );
    assert!(limit.additional > 0);
}

#[test]
fn anchor_materialization_uses_the_decode_session_budget() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<a>=(1,2,3,4,5,6,7,8);ENDSEC;DATA;#1=ITEM(<a>);ENDSEC;END-ISO-10303-21;";
    let mut materialization_limit = None;
    for max_work_units in 1..=1024 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_work_units = max_work_units;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits the test policy");
        let error = crate::parse::parse_with_context(source, &ctx)
            .expect_err("anchor materialization must consume shared work");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if limit.context.operation == "step_anchor_materialization" {
            materialization_limit = Some(limit);
            break;
        }
    }
    let limit = materialization_limit.expect("anchor materialization must have a budget gate");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert!(limit.additional > 0);
    assert!(limit.used <= limit.limit);
}

#[test]
fn local_reference_materialization_uses_the_decode_session_budget() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;3');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<a>=(1,2,3,4,5,6,7,8);ENDSEC;REFERENCE;@2=<#a>;ENDSEC;DATA;#1=ITEM(@2);ENDSEC;END-ISO-10303-21;";
    crate::parse::parse(source).expect("local-reference fixture must parse");
    let mut materialization_limit = None;
    for max_work_units in 1..=2048 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_work_units = max_work_units;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits the test policy");
        let error = crate::parse::parse_with_context(source, &ctx)
            .expect_err("local reference materialization must consume shared work");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if limit.context.operation == "step_reference_materialization" {
            materialization_limit = Some(limit);
            break;
        }
    }
    let limit = materialization_limit.expect("local references must have a budget gate");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert!(limit.additional > 0);
    assert!(limit.used <= limit.limit);
}

#[test]
fn parser_enforces_the_part21_header_contract() {
    let cases = [
        (
            "ISO-10303-21;HEADER;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "HEADER must begin with FILE_DESCRIPTION, FILE_NAME, and FILE_SCHEMA",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_SCHEMA(('AP242'));FILE_NAME('','',(''),(''),'','','');ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "HEADER must begin with FILE_DESCRIPTION, FILE_NAME, and FILE_SCHEMA",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','','','','','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "FILE_NAME has invalid parameters",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242','ap242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "FILE_SCHEMA has invalid or duplicate schema identifiers",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242','AP24\\X\\32'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "FILE_SCHEMA has invalid or duplicate schema identifiers",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(());ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "FILE_SCHEMA has invalid or duplicate schema identifiers",
        ),
    ];

    for (source, message) in cases {
        let error = crate::parse::parse(source.as_bytes()).expect_err("invalid header");
        assert!(
            error.to_string().contains(message),
            "expected {message:?}, got {error}"
        );
    }
}

#[test]
fn parser_validates_header_string_bounds_timestamps_and_schema_identifiers() {
    fn source(file_name: &str, schema: &str, extra: &str) -> String {
        format!(
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME({file_name});FILE_SCHEMA(({schema}));{extra}ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;"
        )
    }

    let valid = source(
        "'name','2026-02-28T23:59:59.123+02:30',('author'),('organization'),'preprocessor','',''",
        "'AP242 { 1 0 10303 442 3 1 4 }'",
        "SCHEMA_POPULATION((('part.step','2026-02-28T00:00:00Z','YWJjZA==')));",
    );
    crate::parse::parse(valid.as_bytes()).expect("valid header metadata");

    let signed_schema_oid = source(
        "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
        "' AUTOMOTIVE_DESIGN_CC2 { 1 2 10303 214 -1 1 5 4 } '",
        "",
    );
    crate::parse::parse(signed_schema_oid.as_bytes())
        .expect("schema object identifiers permit signed components");

    let invalid = [
        source(
            "'name','2026-02-30T23:59:59',('author'),('organization'),'preprocessor','',''",
            "'AP242'",
            "",
        ),
        source(
            "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
            "'AP242 { 1 invalid }'",
            "",
        ),
        source(
            "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
            "'AP242 { }'",
            "",
        ),
        source(
            "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
            "'AP242'",
            "SCHEMA_POPULATION((('part.step','2026-02-28T00:00:00Z','not*base64')));",
        ),
    ];
    for source in invalid {
        assert!(crate::parse::parse(source.as_bytes()).is_err());
    }

    let long_description = "x".repeat(257);
    let long_description_source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('{long_description}'),'4;2');FILE_NAME('name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;"
    );
    assert!(crate::parse::parse(long_description_source.as_bytes()).is_err());

    let long_schema = "A".repeat(1025);
    let long_schema_source = source(
        "'name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','',''",
        &format!("'{long_schema}'"),
        "",
    );
    assert!(crate::parse::parse(long_schema_source.as_bytes()).is_err());

    let malformed_data_schema = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('name','2026-02-28T23:59:59',('author'),('organization'),'preprocessor','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA('main',('AP242 { 1 invalid }'));#1=ITEM();ENDSEC;END-ISO-10303-21;";
    assert!(crate::parse::parse(malformed_data_schema.as_bytes()).is_err());
}

#[test]
fn parser_retains_unset_file_name_tail_metadata() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'',$,$);FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("unset producer metadata");
    assert!(matches!(
        exchange.header[1].parameters[6],
        crate::parse::Value::Omitted
    ));
}

#[test]
fn parser_allows_an_empty_data_population_in_edition_three() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;END-ISO-10303-21;";
    crate::parse::parse(source).expect("edition three permits no DATA section");
}

#[test]
fn parser_enforces_legacy_implementation_level_restrictions() {
    let cases = [
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;END-ISO-10303-21;",
            "historical implementation levels require one DATA section",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA('section');#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "2;1 forbids DATA section parameters",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "2;1 forbids ANCHOR and REFERENCE sections",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;YWJjZA==ENDSEC;",
            "2;1 forbids SIGNATURE sections",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'3;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "3;1 forbids ANCHOR and REFERENCE sections",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'3;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));SCHEMA_POPULATION(('all'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "3;1 forbids SCHEMA_POPULATION in HEADER",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'3;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;END-ISO-10303-21;",
            "historical implementation levels require one DATA section",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'3;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;YWJjZA==ENDSEC;",
            "3;1 forbids SIGNATURE sections",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'1;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "FILE_DESCRIPTION has an unsupported implementation level",
        ),
    ];

    for (source, message) in cases {
        let error = crate::parse::parse(source.as_bytes()).expect_err("invalid level");
        assert!(
            error.to_string().contains(message),
            "expected {message:?}, got {error}"
        );
    }
}

#[test]
fn parser_accepts_historical_implementation_level_spellings() {
    for level in ["1", "2", "2;1", "2;2", "3;1", "3;2"] {
        let source = format!(
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'{level}');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;"
        );
        crate::parse::parse(source.as_bytes()).expect("historical implementation level");
    }
}

#[test]
fn parser_enforces_edition_three_conformance_classes() {
    let cases = [
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<item>=#1;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "4;1 forbids ANCHOR and REFERENCE sections",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));SCHEMA_POPULATION((('part.step',$,$)));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "4;1 forbids SCHEMA_POPULATION in HEADER",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;YWJjZA==ENDSEC;",
            "4;1 forbids SIGNATURE sections",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;@10=<part.step#value>;ENDSEC;DATA;#1=ITEM(@10);ENDSEC;END-ISO-10303-21;",
            "this implementation level forbids value instances and EXPRESS constants",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(#PI);ENDSEC;END-ISO-10303-21;",
            "this implementation level forbids value instances and EXPRESS constants",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'3;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(<external>);ENDSEC;END-ISO-10303-21;",
            "resource values are only valid in edition-3 anchor items",
        ),
    ];

    for (source, message) in cases {
        let error = crate::parse::parse(source.as_bytes()).expect_err("invalid conformance class");
        assert!(
            error.to_string().contains(message),
            "expected {message:?}, got {error}"
        );
    }

    let class_three = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;3');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;@10=<part.step#value>;ENDSEC;DATA;#1=ITEM(@10,#PI);ENDSEC;END-ISO-10303-21;";
    crate::parse::parse(class_three).expect("class three value occurrences");
}

#[test]
fn parser_allows_multiple_schema_identifiers_at_legacy_level() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('CONFIG_CONTROL_DESIGN','GEOMETRIC_VALIDATION_PROPERTIES_MIM'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    crate::parse::parse(source).expect("2;1 permits multiple schema identifiers");
}

#[test]
fn parser_validates_optional_header_entities_and_data_section_targets() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));SCHEMA_POPULATION((('part.step',$,$)));FILE_POPULATION('AP242','INCLUDE_ALL_COMPATIBLE',('main'));SECTION_LANGUAGE('main','eng');SECTION_CONTEXT('main',('design'));!VENDOR(('metadata'));ENDSEC;DATA('main',('AP242'));#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid optional header entities");
    assert_eq!(exchange.data[0].records, vec![1]);
    assert_eq!(exchange.header[7].name, "!VENDOR");

    let invalid = [
        (
            "SCHEMA_POPULATION((('part.step',$)));",
            "SCHEMA_POPULATION has invalid parameters",
        ),
        (
            "FILE_POPULATION('AP214','INCLUDE_ALL_COMPATIBLE',('main'));",
            "FILE_POPULATION has invalid parameters",
        ),
        (
            "SECTION_LANGUAGE('missing','eng');",
            "header section reference names an unknown DATA section",
        ),
        (
            "SECTION_LANGUAGE('main','english');",
            "SECTION_LANGUAGE has invalid parameters",
        ),
        (
            "!VENDOR(('metadata'));SECTION_CONTEXT('main',('design'));",
            "built-in HEADER entities must precede user-defined entities",
        ),
    ];
    for (extra, message) in invalid {
        let source = format!(
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));{extra}ENDSEC;DATA('main',('AP242'));#1=ITEM();ENDSEC;END-ISO-10303-21;"
        );
        let error = crate::parse::parse(source.as_bytes()).expect_err("invalid header entity");
        assert!(
            error.to_string().contains(message),
            "expected {message:?}, got {error}"
        );
    }

    let legacy = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));SCHEMA_POPULATION((('part.step',$,$)));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    crate::parse::parse(legacy).expect("2;1 permits SCHEMA_POPULATION");
}

#[test]
fn parser_enforces_data_section_parameter_shape_and_multiplicity() {
    let cases = [
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;DATA;#2=ITEM();ENDSEC;END-ISO-10303-21;",
            "multiple DATA sections require section parameters",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;DATA('section',('AP242'));#2=ITEM();ENDSEC;END-ISO-10303-21;",
            "multiple DATA sections require section parameters",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA('section',('AP242'));#1=ITEM();ENDSEC;DATA('section',('AP242'));#2=ITEM();ENDSEC;END-ISO-10303-21;",
            "DATA section names must be unique",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA('section');#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "DATA section parameters must contain a name and one schema",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA('section',('AP214'));#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "DATA section schema is not listed in FILE_SCHEMA",
        ),
        (
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242','AP214'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
            "an unnamed DATA section requires one FILE_SCHEMA identifier",
        ),
    ];

    for (source, message) in cases {
        let error = crate::parse::parse(source.as_bytes()).expect_err("invalid DATA section");
        assert!(
            error.to_string().contains(message),
            "expected {message:?}, got {error}"
        );
    }

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242','AP214'));ENDSEC;DATA('section-1',('AP242'));#1=ITEM();ENDSEC;DATA('section-2',('AP214'));#2=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid named DATA sections");
    assert_eq!(exchange.data.len(), 2);
}

#[test]
fn parser_bounds_exponential_anchor_expansion() {
    let mut anchors = String::from("<a0>=(1,1);\n");
    for index in 1..40 {
        writeln!(anchors, "<a{index}>=(<a{}>,<a{}>);", index - 1, index - 1)
            .expect("write anchor fixture");
    }
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;{anchors}ENDSEC;DATA;#1=ITEM(<a39>);ENDSEC;END-ISO-10303-21;"
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
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;{anchors}ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;"
    );
    let error = crate::parse::parse(source.as_bytes()).unwrap_err();
    assert!(error.to_string().contains("expanded anchor"));
}

#[test]
fn parser_rejects_duplicate_complex_partial_names() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=(B()A()B());ENDSEC;END-ISO-10303-21;";
    let error = crate::parse::parse(source).expect_err("duplicate partial names must fail");
    assert!(matches!(
        error,
        crate::parse::ParseError::Syntax { message, .. }
            if message == "duplicate complex partial name"
    ));
}

#[test]
fn parser_accepts_external_instance_references_in_edition_three() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<external>=#100;ENDSEC;REFERENCE;#100=<part.step#root>;ENDSEC;DATA;#1=ITEM(<external>);ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) = crate::parse::parse(source).expect("external reference");

    assert!(diagnostics.is_empty());
    assert_eq!(exchange.references[0].name, "#100");
    assert_eq!(exchange.references[0].uri, "part.step#root");
    assert_eq!(
        exchange.anchors[0].value,
        crate::parse::Value::Reference(100)
    );
    assert_eq!(
        exchange.records[&1].partials[0].parameters,
        vec![crate::parse::Value::Reference(100)]
    );
}

#[test]
fn parser_resolves_local_entity_reference_anchors_before_schema_decoding() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<shape>=#2;ENDSEC;REFERENCE;#10=<#shape>;ENDSEC;DATA;#1=ITEM(#10);#2=TARGET();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) = crate::parse::parse(source).expect("local entity reference");

    assert!(diagnostics.is_empty());
    assert_eq!(
        exchange.records[&1].partials[0].parameters,
        vec![crate::parse::Value::Reference(2)]
    );
}

#[test]
fn parser_resolves_local_value_reference_anchors_and_nulls_invalid_targets() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;3');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<length>=3.;<shape>=#2;ENDSEC;REFERENCE;@10=<#length>;@11=<#shape>;#12=<missing>;#13=<external.step>;ENDSEC;DATA;#1=ITEM(@10,@11,#12,#13);#2=TARGET();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) = crate::parse::parse(source).expect("local value references");

    assert!(diagnostics.is_empty());
    assert_eq!(
        exchange.records[&1].partials[0].parameters,
        vec![
            crate::parse::Value::Real(3.0),
            crate::parse::Value::Omitted,
            crate::parse::Value::Omitted,
            crate::parse::Value::Omitted,
        ]
    );
}

#[test]
fn parser_checks_edition_three_syntax_before_local_reference_substitution() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<length>=3.;ENDSEC;REFERENCE;@10=<#length>;ENDSEC;DATA;#1=ITEM(@10);ENDSEC;END-ISO-10303-21;";
    let error = crate::parse::parse(source).expect_err("class-2 value occurrence");

    assert!(error
        .to_string()
        .contains("this implementation level forbids value instances"));
}

#[test]
fn parser_resolves_cyclic_local_references_to_null_values() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<cycle>=#10;ENDSEC;REFERENCE;#10=<#cycle>;ENDSEC;DATA;#1=ITEM(#10);ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) = crate::parse::parse(source).expect("cyclic reference");

    assert!(diagnostics.is_empty());
    assert_eq!(exchange.anchors[0].value, crate::parse::Value::Omitted);
    assert_eq!(
        exchange.records[&1].partials[0].parameters,
        vec![crate::parse::Value::Omitted]
    );
}

#[test]
fn parser_requires_numeric_reference_left_hand_sides() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;<external>=<part.step#root>;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let error = crate::parse::parse(source).expect_err("resource reference name");
    assert!(error.to_string().contains("expected reference name"));
}

#[test]
fn parser_accepts_value_instances_and_express_constants_in_edition_three() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;3');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<constant_entity>=#PI; <constant_value>=@E; <external_value>=@100;ENDSEC;REFERENCE;#200=<part.step#entity>;@100=<part.step#value>;ENDSEC;DATA;#1=ITEM(#PI,@E,@100,#200);ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) = crate::parse::parse(source).expect("edition-3 occurrences");

    assert!(diagnostics.is_empty());
    assert_eq!(
        exchange
            .references
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["#200", "@100"]
    );
    assert_eq!(
        exchange.anchors[0].value,
        crate::parse::Value::ConstantEntity("PI".into())
    );
    assert_eq!(
        exchange.anchors[1].value,
        crate::parse::Value::ConstantValue("E".into())
    );
    assert_eq!(
        exchange.records[&1].partials[0].parameters,
        vec![
            crate::parse::Value::ConstantEntity("PI".into()),
            crate::parse::Value::ConstantValue("E".into()),
            crate::parse::Value::ValueReference(100),
            crate::parse::Value::Reference(200),
        ]
    );
}

#[test]
fn parser_retains_anchor_tags_and_resolves_their_references() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;3');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<shape>=#1 {source:<part.step#shape>} {width:@100};ENDSEC;REFERENCE;@100=<part.step#width>;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) = crate::parse::parse(source).expect("anchor tags");

    assert!(diagnostics.is_empty());
    assert_eq!(exchange.anchors[0].name, "shape");
    assert_eq!(exchange.anchors[0].tags.len(), 2);
    assert_eq!(exchange.anchors[0].tags[0].name, "source");
    assert_eq!(
        exchange.anchors[0].tags[0].value,
        crate::parse::Value::Resource("part.step#shape".into())
    );
    assert_eq!(exchange.anchors[0].tags[1].name, "width");
    assert_eq!(
        exchange.anchors[0].tags[1].value,
        crate::parse::Value::ValueReference(100)
    );
}

#[test]
fn parser_enforces_anchor_name_and_item_grammar() {
    let cases = [
        ("<123>=1;", "anchor name must contain a non-digit character"),
        ("<>=1;", "anchor name must contain a non-digit character"),
        ("<a>=*;", "invalid anchor item"),
        ("<a>=TYPE(1);", "invalid anchor item"),
        ("<a>=1 {tag:TYPE(1)};", "invalid anchor tag item"),
    ];
    for (entry, message) in cases {
        let source = format!(
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;{entry}ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;"
        );
        let error = crate::parse::parse(source.as_bytes()).expect_err("invalid anchor entry");
        assert!(
            error.to_string().contains(message),
            "expected {message:?}, got {error}"
        );
    }

    let valid = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<a>=(1,(2));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    crate::parse::parse(valid).expect("nested anchor item");
}

#[test]
fn parser_rejects_unresolved_or_colliding_value_instances() {
    let unresolved = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;3');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(@100);ENDSEC;END-ISO-10303-21;";
    let error = crate::parse::parse(unresolved).expect_err("unresolved value instance");
    assert!(error
        .to_string()
        .contains("unresolved value instance reference"));

    let collision = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;3');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;@100=<part.step#value>;ENDSEC;DATA;#100=ITEM();ENDSEC;END-ISO-10303-21;";
    let error = crate::parse::parse(collision).expect_err("colliding value instance");
    assert!(error
        .to_string()
        .contains("external value instance collides with a DATA instance"));
}

#[test]
fn parser_rejects_edition_three_occurrences_in_historical_data() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(#PI);ENDSEC;END-ISO-10303-21;";
    let error = crate::parse::parse(source).expect_err("historical occurrence name");
    assert!(error
        .to_string()
        .contains("historical implementation levels forbid edition-3 occurrence names"));
}

#[test]
fn parser_resolves_anchor_before_repairing_omitted_entity_names() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<line_name>='anchored line';ENDSEC;DATA;#1=CARTESIAN_POINT('',(0.,0.,0.));#2=DIRECTION('',(1.,0.,0.));#3=VECTOR('',#2,1.);#4=LINE(<line_name>,#1,#3);ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) = crate::parse::parse(source).expect("anchored line name");

    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.kind != crate::parse::ParseDiagnosticKind::OmittedEntityName));
    assert_eq!(
        exchange.records[&4].partials[0].parameters,
        vec![
            crate::parse::Value::String(b"anchored line".to_vec()),
            crate::parse::Value::Reference(1),
            crate::parse::Value::Reference(3),
        ]
    );
}

#[test]
fn omitted_name_recovery_accounts_for_inserted_parameter_storage() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT((0.,0.,0.));ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) = crate::parse::parse(source).expect("recover omitted name");
    let parameters = &exchange.records[&1].partials[0].parameters;

    assert_eq!(parameters.len(), 2);
    assert_eq!(diagnostics.len(), 1);

    let mut recovery_limit = None;
    for max_retained_bytes in 1..=8192 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = max_retained_bytes;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)
                .expect("root fits the test policy");
        let error = crate::parse::parse_with_context(source, &ctx)
            .expect_err("recovered storage must consume retained bytes");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            continue;
        };
        if limit.context.operation == "step_omitted_name_recovery_storage" {
            recovery_limit = Some(limit);
            break;
        }
    }
    let limit = recovery_limit.expect("recovered name storage must have a budget gate");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes
    );
    assert!(limit.additional > 0);
}

#[test]
fn parser_retains_user_defined_entity_and_type_names() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=!VENDOR_ENTITY(!VENDOR_TYPE(#2));#2=KNOWN();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) = crate::parse::parse(source).expect("user-defined names");

    assert!(diagnostics.is_empty());
    assert_eq!(exchange.records[&1].partials[0].name, "!VENDOR_ENTITY");
    assert_eq!(
        exchange.records[&1].partials[0].parameters,
        vec![crate::parse::Value::Typed(
            "!VENDOR_TYPE".into(),
            Box::new(crate::parse::Value::Reference(2)),
        )]
    );
}

#[test]
fn parser_reports_recoverable_noncanonical_complex_partial_order() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=(NAMED_UNIT(#2)SOLID_ANGLE_UNIT()SI_UNIT($,.STERADIAN.));#2=DIMENSIONAL_EXPONENTS(0.,0.,0.,0.,0.,0.,0.);ENDSEC;END-ISO-10303-21;";
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
fn parser_recovers_omitted_geometry_name_without_shifting_context_fields() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT((0.,1.,2.));#2=GEOMETRIC_REPRESENTATION_CONTEXT(3);#3=MAPPED_ITEM(#1,#2);#4=SEAM_EDGE(*,*,#1,.T.,$);#5=SHAPE_REPRESENTATION((#1),$);#6=CLOSED_SHELL($,(#1));ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::parse::parse(source).expect("omitted geometry name is recoverable");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        crate::parse::ParseDiagnosticKind::OmittedEntityName
    );
    assert!(diagnostics[0]
        .message
        .contains("recovered 4 simple named carrier instance(s)"));
    assert_eq!(
        exchange.records[&1].partials[0].parameters,
        vec![
            crate::parse::Value::String(Vec::new()),
            crate::parse::Value::List(vec![
                crate::parse::Value::Real(0.0),
                crate::parse::Value::Real(1.0),
                crate::parse::Value::Real(2.0),
            ]),
        ]
    );
    assert_eq!(
        exchange.records[&2].partials[0].parameters,
        vec![crate::parse::Value::Integer(3)]
    );
    assert_eq!(
        exchange.records[&3].partials[0].parameters[0],
        crate::parse::Value::String(Vec::new())
    );
    assert_eq!(
        exchange.records[&4].partials[0].parameters[0],
        crate::parse::Value::String(Vec::new())
    );
    assert_eq!(
        exchange.records[&5].partials[0].parameters[0],
        crate::parse::Value::String(Vec::new())
    );
    assert_eq!(
        exchange.records[&6].partials[0].parameters,
        vec![
            crate::parse::Value::Omitted,
            crate::parse::Value::List(vec![crate::parse::Value::Reference(1)]),
        ]
    );
}

#[test]
fn decode_salvages_noncanonical_complex_partial_order_with_provenance() {
    let bytes = include_bytes!("../tests/fixtures/noncanonical_solid_angle.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("salvage mode accepts recoverable source order");
    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code
                == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::NoncanonicalSourceSyntax)
        })
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
    write_step(decoded.ir(), &mut output, &StepWriteOptions::default())
        .expect("export salvaged IR");

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
pub(crate) fn codec_detects_and_inspects_ap242_exchange_structure() {
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
fn codec_uses_the_first_schema_identifier_for_exact_edition_selection() {
    let cases = [
        (
            "'AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }','AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 4 1 4 }'",
            "edition 1",
        ),
        (
            "'AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 14 1 4 }'",
            "edition unspecified",
        ),
        (
            "'OTHER_SCHEMA { 1 0 10303 442 4 1 4 }'",
            "edition unspecified",
        ),
        (
            "'ap242_managed_model_based_3d_engineering_mim_lf { 1 0 10303 442 4 1 4 }'",
            "edition 3",
        ),
    ];

    for (identifiers, expected_edition) in cases {
        let first_identifier = identifiers.split(',').next().expect("first schema");
        let source = format!(
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(({identifiers}));ENDSEC;DATA('section',({first_identifier}));#1=ITEM();ENDSEC;END-ISO-10303-21;"
        );
        let summary = StepCodec::default()
            .inspect(
                &mut Cursor::new(source.as_bytes()),
                &InspectOptions::default(),
            )
            .expect("inspect schema identifiers");
        assert!(
            summary
                .notes
                .iter()
                .any(|note| note.ends_with(expected_edition)),
            "expected {expected_edition} in {:?}",
            summary.notes
        );
    }
}

#[test]
fn codec_detection_matches_part21_trivia_and_keyword_rules() {
    let source = b"/* preamble */\n  iso-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;ENDSEC;END-ISO-10303-21;";
    let codec = StepCodec::default();

    assert_eq!(codec.detect(source), Confidence::High);
    codec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("Part 21 leading trivia and case-insensitive magic");
}

#[test]
fn codec_refuses_out_of_envelope_encodings_by_name() {
    let codec = StepCodec::default();
    let cases: &[(&[u8], &str)] = &[
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
fn codec_decodes_step_zip_root_and_reports_archive_members() {
    let root = include_bytes!("../tests/fixtures/ap242_minimal.p21");
    let bytes = step_zip(&[
        ("ISO-10303.p21", root, CompressionMethod::Deflated),
        ("parts/child.p21", b"subsidiary", CompressionMethod::Stored),
        ("preview.bin", b"ancillary", CompressionMethod::Stored),
    ]);
    let codec = StepCodec::default();

    assert_eq!(codec.detect(&bytes), Confidence::Medium);
    let summary = codec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("inspect STEP ZIP");
    assert_eq!(summary.container_kind, "iso-10303-21-zip");
    assert_eq!(
        summary
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["ISO-10303.p21", "parts/child.p21", "preview.bin"]
    );
    assert_eq!(summary.entries[0].role, "root-exchange");
    assert_eq!(summary.entries[0].compression, "deflate");
    assert_eq!(summary.entries[1].role, "subsidiary-exchange");
    assert!(summary.entries[0].attributes["logical_sections"].contains("HEADER"));

    let result = codec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("decode STEP ZIP root");
    let source = result.ir().source.as_ref().expect("STEP source metadata");
    assert_eq!(source.format, "step");
    assert_eq!(source.attributes["container_kind"], "iso-10303-21-zip");
    assert_eq!(source.attributes["archive_root"], "ISO-10303.p21");
    assert_eq!(source.attributes["archive_entries"], "3");
    assert!(result
        .report()
        .notes
        .iter()
        .any(|note| note == "container root ISO-10303.p21; archive entries=3"));
}

#[test]
fn codec_resolves_root_references_relative_to_the_archive_directory() {
    let root = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('zip references'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;#10=<parts/child.p21#target>;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let bytes = step_zip(&[
        ("ISO-10303.p21", root, CompressionMethod::Stored),
        ("parts/child.p21", b"child", CompressionMethod::Stored),
    ]);
    let codec = StepCodec::default();
    let summary = codec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("inspect relative ZIP reference");
    assert!(summary
        .notes
        .iter()
        .any(|note| note == "internal resource #10 -> parts/child.p21#target"));

    let missing = step_zip(&[("ISO-10303.p21", root, CompressionMethod::Stored)]);
    assert!(matches!(
        codec.inspect(&mut Cursor::new(missing), &InspectOptions::default()),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));

    let escaping = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('zip references'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;#10=<../outside.p21#target>;ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let escaping = step_zip(&[("ISO-10303.p21", escaping, CompressionMethod::Stored)]);
    assert!(matches!(
        codec.inspect(&mut Cursor::new(escaping), &InspectOptions::default()),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn codec_rejects_step_zip_without_root_or_with_unsupported_layout() {
    let root = include_bytes!("../tests/fixtures/ap242_minimal.p21");
    let codec = StepCodec::default();

    let missing_root = step_zip(&[("part.p21", root, CompressionMethod::Stored)]);
    assert!(matches!(
        codec.inspect(&mut Cursor::new(missing_root), &InspectOptions::default()),
        Err(cadmpeg_core::CodecError::WrongFormat(_))
    ));

    let unsafe_path = step_zip(&[
        ("ISO-10303.p21", root, CompressionMethod::Stored),
        ("../outside.bin", b"unsafe", CompressionMethod::Stored),
    ]);
    assert!(matches!(
        codec.inspect(&mut Cursor::new(unsafe_path), &InspectOptions::default()),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));

    let zstd = step_zip(&[("ISO-10303.p21", root, CompressionMethod::Zstd)]);
    assert!(matches!(
        codec.inspect(&mut Cursor::new(zstd), &InspectOptions::default()),
        Err(cadmpeg_core::CodecError::NotImplemented(_))
    ));
}

#[test]
pub(crate) fn codec_inspects_edition3_sections_and_external_references() {
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
    assert_eq!(exchange.signatures.len(), 1);
    let signature = exchange.signatures[0].clone();
    assert!(bytes[signature.clone()]
        .windows(b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=".len())
        .any(|bytes| bytes == b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA="));
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode signature fixture");
    let unknowns = decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena");
    let signature_unknown = unknowns
        .iter()
        .find(|record| record.id.0 == "step:file:signature#0")
        .expect("retained signature");
    assert_eq!(
        decoded
            .source_fidelity()
            .retained_record(&signature_unknown.id.0)
            .and_then(|record| record.data.as_deref()),
        Some(&bytes[signature.clone()])
    );
    assert_eq!(
        exchange.records[&2].partials[0].parameters,
        vec![crate::parse::Value::Reference(1)]
    );
}

#[test]
fn parser_retains_multiple_signature_sections_after_exchange_terminator() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('signatures'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nENDSEC;SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nENDSEC;";
    let (exchange, diagnostics) = crate::parse::parse(source).expect("multiple signatures");

    assert!(diagnostics.is_empty());
    assert_eq!(exchange.signatures.len(), 2);
    assert!(source[exchange.signatures[0].clone()]
        .windows(b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=".len())
        .any(|bytes| bytes == b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA="));
    assert!(source[exchange.signatures[1].clone()]
        .windows(b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=".len())
        .any(|bytes| bytes == b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA="));
}

#[test]
fn parser_exposes_the_detached_signature_contract() {
    let source = b" /* leading trivia */ ISO-10303-21;HEADER;FILE_DESCRIPTION(('signature'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nENDSEC;";
    let (exchange, diagnostics) = crate::parse::parse(source).expect("signature contract");

    assert!(diagnostics.is_empty());
    let section = &exchange.signature_sections[0];
    let signed = &source[section.signed.clone()];
    assert!(signed.starts_with(b"ISO-10303-21;"));
    assert!(signed.ends_with(b"END-ISO-10303-21;"));
    assert_eq!(
        &source[section.payload.clone()],
        b"MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\n"
    );
    assert_eq!(section.cms.len(), 92);
    let signed_alphabet = section
        .signed_alphabet_bytes(source)
        .expect("signed source range");
    assert!(!signed_alphabet.contains(&b'\n'));
    assert!(signed_alphabet.ends_with(b"END-ISO-10303-21;"));
    assert_eq!(
        &source[section.span.clone()],
        b"SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nENDSEC;"
    );
}

#[test]
fn parser_ignores_controls_inside_signature_terminators() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('signature'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;MFoGCSqGSIb3DQEHAqBNMEsCAQExDTALBglghkgBZQMEAgEwCwYJKoZIhvcNAQcBMSowKAIBATAFMAACAQEwCwYJYIZIAWUDBAIBMA0GCSqGSIb3DQEBAQUABAA=\nEN\nDSEC;";
    let (exchange, _) = crate::parse::parse(source).expect("split signature terminator");
    assert_eq!(exchange.signatures.len(), 1);
}

#[test]
fn parser_rejects_invalid_signature_base64() {
    for (payload, expected_message) in [
        ("YWJjZA==!", "invalid SIGNATURE base64 padding"),
        ("YWJjZA==AAAA", "invalid SIGNATURE base64 padding"),
        ("YWJjZA=", "SIGNATURE base64 content has incomplete quantum"),
    ] {
        let source = format!(
            "ISO-10303-21;HEADER;FILE_DESCRIPTION(('signature'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;SIGNATURE;{payload}\nENDSEC;"
        );
        let error = crate::parse::parse(source.as_bytes()).expect_err("invalid signature");
        assert!(matches!(
            error,
            crate::parse::ParseError::Lex(crate::lex::LexError { message, .. })
                if message == expected_message
        ));
    }
}

#[test]
pub(crate) fn decode_preserves_named_opaque_records_with_exact_byte_spans() {
    let bytes = include_bytes!("../tests/fixtures/ap242_minimal.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode parsed STEP document");

    assert_eq!(result.ir().source.as_ref().unwrap().format, "step");
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
    assert!(!result.report().geometry_transferred);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("EXAMPLE_RECORD")));
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
    let bytes = include_bytes!("../tests/fixtures/ap242_semantic_pmi.p21");
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
fn unresolvable_length_unit_reports_an_error_loss() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('unresolvable length unit'),'2;1');FILE_NAME('unit','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=(LENGTH_UNIT() NAMED_UNIT(*));#2=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));#3=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#2)) REPRESENTATION_CONTEXT('model','3D'));#4=CARTESIAN_POINT('',(1.,2.,3.));#5=SHAPE_REPRESENTATION('',(#4),#3);ENDSEC;END-ISO-10303-21;";
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode bare named length unit");
    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.message
                .starts_with("the document length unit did not resolve")
        })
        .expect("unresolved length unit loss");
    assert_eq!(
        loss.code,
        cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::GeometryNotTransferred)
    );
    assert_eq!(loss.severity, cadmpeg_ir::Severity::Error);
    assert_eq!(
        loss.message,
        "the document length unit did not resolve; coordinates are unscaled and reported as millimetres"
    );
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
pub(crate) fn decode_transfers_placed_analytic_geometry_in_millimetres() {
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

    let bytes = include_bytes!("../tests/fixtures/ap242_geometry.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode typed STEP geometry");

    assert_eq!(result.ir().model.points.len(), 1);
    let placed = result
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id.0 == "step:data:point#3")
        .unwrap();
    assert_eq!(placed.position.x, 1.0);
    assert_eq!(placed.position.y, 2.0);
    assert_eq!(placed.position.z, 3.0);
    assert_eq!(result.ir().model.curves.len(), 9);
    assert!(result.ir().model.curves.iter().any(|curve| {
        curve.id.as_str() == "step:data:curve#45"
            && matches!(curve.geometry, CurveGeometry::Composite { .. })
    }));
    assert!(result.ir().model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Line { origin, direction }
            if origin.x == 1.0 && origin.y == 2.0 && origin.z == 3.0
                && direction.x == 0.0 && direction.y == 0.0 && direction.z == 1.0
    )));
    assert!(!result.report().losses.iter().any(|loss| loss
        .message
        .contains("GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #51")));
    assert!(result
        .ir()
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
    assert!(result.ir().model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Ellipse { major_radius, minor_radius, .. }
            if major_radius == 6.0 && minor_radius == 2.0
    )));
    assert!(result.ir().model.curves.iter().any(|curve| matches!(
        &curve.geometry,
        CurveGeometry::Nurbs(nurbs)
            if nurbs.degree == 2
                && nurbs.knots == [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
                && nurbs.weights.as_deref() == Some(&[1.0, 0.5, 1.0][..])
    )));
    assert_eq!(result.ir().model.surfaces.len(), 10);
    assert!(result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Curve(_)
        )));
    assert!(result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Surface(_)
        )));
    assert!(result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Point(_)
        )));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("STYLED_ITEM #43")));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("STYLED_ITEM #52")));
    assert_eq!(
        result
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| binding.source_entity_id.as_deref() == Some("#47"))
            .count(),
        2
    );
    assert!(result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            &binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Source { source_id } if source_id == "#6"
        )));
    assert!(result.ir().model.curves.iter().any(|curve| matches!(
        &curve.geometry,
        CurveGeometry::Nurbs(nurbs)
            if curve.id.as_str() == "step:data:curve#48"
                && nurbs.degree == 1
                && nurbs.knots == [0.0, 0.0, 1.0, 2.0, 2.0]
    )));
    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Plane { origin, normal, .. }
            if origin.x == 1.0 && origin.y == 2.0 && origin.z == 3.0 && normal.z == 1.0
    )));
    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
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
    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cylinder { radius, .. } if radius == 5.0
    )));
    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cone { radius, ratio, half_angle, .. }
            if radius == 5.0 && ratio == 1.0 && half_angle == 0.25
    )));
    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Sphere { radius, .. } if radius == 5.0
    )));
    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Torus { major_radius, minor_radius, .. }
            if major_radius == 8.0 && minor_radius == 2.0
    )));
    assert!(result.ir().model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Circle { center, radius, .. }
            if center.x == 1.0 && center.y == 2.0 && center.z == 3.0 && radius == 4.0
    )));
    assert!(result.report().geometry_transferred);
    assert_eq!(result.ir().model.procedural_curves.len(), 3);
    let cartesian_trim = result
        .ir()
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
        .ir()
        .model
        .procedural_curves
        .iter()
        .find_map(|curve| match &curve.definition {
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                source,
                parameter_range,
                ..
            } => Some((source, *parameter_range)),
            _ => None,
        })
        .expect("trimmed curve was not retained as a subset construction");
    assert_eq!(source.as_str(), "step:data:curve#8");
    assert_eq!(parameter_range, [0.0, std::f64::consts::FRAC_PI_2]);
    assert!(result
        .ir()
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
    assert_eq!(result.ir().model.procedural_surfaces.len(), 4);
    assert!(result
        .ir()
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
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| matches!(
            surface.definition,
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::LinearSweep { direction, .. }
                if direction.z == 2.0
        )));
    assert!(result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| matches!(
            surface.definition,
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::AxisRevolution { axis_direction, .. }
                if axis_direction.z == 1.0
        )));
    assert!(result
        .ir()
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
pub(crate) fn procedural_step_geometry_round_trips_as_native_entities() {
    let source = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../tests/fixtures/ap242_geometry.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode procedural geometry");

    let mut bytes = Vec::new();
    let report = write_step(
        source.ir(),
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
    assert_eq!(decoded.ir().model.procedural_curves.len(), 3);
    assert_eq!(decoded.ir().model.procedural_surfaces.len(), 4);

    let bounded = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../tests/fixtures/ap242_geometric_set.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode curve-bounded surface");
    let mut bytes = Vec::new();
    let report = write_step(bounded.ir(), &mut bytes, &StepWriteOptions::default())
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
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| matches!(
            surface.definition,
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::CurveBounded { .. }
        )));
    let mut rejected = Vec::new();
    assert!(write_step(
        bounded.ir(),
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
fn complex_swept_surfaces_decode_named_partials() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap242_geometry.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#23=SURFACE_OF_LINEAR_EXTRUSION('linear sweep',#8,#5);",
            "#23=(SURFACE() SURFACE_OF_LINEAR_EXTRUSION('linear sweep',#8,#5) SWEPT_SURFACE());",
        )
        .replace(
            "#25=SURFACE_OF_REVOLUTION('full revolution',#8,#24);",
            "#25=(SURFACE() SURFACE_OF_REVOLUTION('full revolution',#8,#24) SWEPT_SURFACE());",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex swept surfaces");

    assert!(decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| { surface.id.as_str() == "step:construction:swept_surface#23" }));
    assert!(decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| { surface.id.as_str() == "step:construction:swept_surface#25" }));
}

#[test]
pub(crate) fn decode_conical_apex_and_context_plane_angle_units() {
    let bytes = include_bytes!("../tests/fixtures/ap242_degree_cone.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode degree cone");

    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cone { radius, half_angle, .. }
            if radius == 0.0 && (half_angle - std::f64::consts::FRAC_PI_4).abs() < 1.0e-12
    )));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
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
pub(crate) fn decode_and_write_singular_vertex_loops() {
    let bytes = include_bytes!("../tests/fixtures/ap242_vertex_loop.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode vertex loops");
    assert_eq!(result.ir().model.loops.len(), 2);
    assert!(result
        .ir()
        .model
        .loops
        .iter()
        .all(|loop_| loop_.coedges.is_empty() && loop_.vertex_uses.len() == 1));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    let mut encoded = Vec::new();
    write_step(result.ir(), &mut encoded, &StepWriteOptions::default())
        .expect("write vertex loops");
    assert_eq!(
        String::from_utf8(encoded)
            .unwrap()
            .matches("VERTEX_LOOP")
            .count(),
        2
    );
}

#[test]
pub(crate) fn decode_resolves_conversion_units_and_linear_uncertainty() {
    let bytes = include_bytes!("../tests/fixtures/ap242_conversion_units.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode conversion-based units");

    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.ir().model.points[0].position.x, 50.8);
    assert!((result.ir().tolerances.linear - 0.0254).abs() < 1e-12);
}

#[test]
fn decode_selects_a_length_uncertainty_after_an_angular_measure() {
    let source = b"ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('mixed uncertainty'),'2;1');\nFILE_NAME('mixed-uncertainty','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');\nFILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));\nENDSEC;\nDATA;\n#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n#2=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#1);\n#3=(CONVERSION_BASED_UNIT('inch',#2) LENGTH_UNIT() NAMED_UNIT(*));\n#4=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n#5=UNCERTAINTY_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(0.5),#4,'angle_accuracy','');\n#6=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.002),#3,'distance_accuracy_value','');\n#7=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#5,#6)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#3,#4)) REPRESENTATION_CONTEXT('model','3D'));\n#8=CARTESIAN_POINT('two inches',(2.,0.,0.));\n#9=SHAPE_REPRESENTATION('construction points',(#8),#7);\nENDSEC;\nEND-ISO-10303-21;\n";
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode mixed uncertainty units");

    assert!((result.ir().tolerances.linear - 0.0508).abs() < 1e-12);
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::GeometryNotTransferred)
    }));
}

#[test]
fn decode_prefers_named_length_uncertainty_when_several_lengths_are_present() {
    let source = b"ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('named uncertainty'),'2;1');\nFILE_NAME('named-uncertainty','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');\nFILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));\nENDSEC;\nDATA;\n#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n#2=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.1),#1,'manufacturing_accuracy','');\n#3=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.2),#1,'distance_accuracy_value','');\n#4=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n#5=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#2,#3)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#4)) REPRESENTATION_CONTEXT('model','3D'));\n#6=CARTESIAN_POINT('point',(1.,0.,0.));\n#7=SHAPE_REPRESENTATION('construction points',(#6),#5);\nENDSEC;\nEND-ISO-10303-21;\n";
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode named uncertainty");

    assert!((result.ir().tolerances.linear - 0.2).abs() < 1e-12);
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::GeometryNotTransferred)
    }));
}

#[test]
fn decode_reports_ambiguous_length_uncertainty() {
    let source = b"ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('ambiguous uncertainty'),'2;1');\nFILE_NAME('ambiguous-uncertainty','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');\nFILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));\nENDSEC;\nDATA;\n#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n#2=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.1),#1,'first_accuracy','');\n#3=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.2),#1,'second_accuracy','');\n#4=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n#5=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#2,#3)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#4)) REPRESENTATION_CONTEXT('model','3D'));\n#6=CARTESIAN_POINT('point',(1.,0.,0.));\n#7=SHAPE_REPRESENTATION('construction points',(#6),#5);\nENDSEC;\nEND-ISO-10303-21;\n";
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode ambiguous uncertainty");

    assert_eq!(result.ir().tolerances.linear, 1e-6);
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::GeometryNotTransferred)
            && loss.severity == cadmpeg_ir::Severity::Warning
    }));
}

#[test]
fn decode_scales_geometry_by_its_representation_context() {
    let source = b"ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('per representation units'),'2;1');\nFILE_NAME('per-representation-units','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');\nFILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));\nENDSEC;\nDATA;\n#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n#2=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#1);\n#3=(CONVERSION_BASED_UNIT('inch',#2) LENGTH_UNIT() NAMED_UNIT(*));\n#4=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n#5=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#4)) REPRESENTATION_CONTEXT('metric','3D'));\n#6=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#3,#4)) REPRESENTATION_CONTEXT('inch','3D'));\n#7=CARTESIAN_POINT('metric point',(10.,0.,0.));\n#8=CARTESIAN_POINT('inch point',(1.,0.,0.));\n#9=SHAPE_REPRESENTATION('metric representation',(#7),#5);\n#10=SHAPE_REPRESENTATION('inch representation',(#8),#6);\nENDSEC;\nEND-ISO-10303-21;\n";
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode per-representation units");

    let metric = result
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id.as_str() == "step:data:point#7")
        .expect("metric point");
    let inch = result
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id.as_str() == "step:data:point#8")
        .expect("inch point");
    assert!((metric.position.x - 10.0).abs() < 1e-12);
    assert!((inch.position.x - 25.4).abs() < 1e-12);
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::GeometryNotTransferred)
    }));
}

#[test]
pub(crate) fn decode_builds_a_valid_connected_sheet_brep() {
    use cadmpeg_ir::topology::{BodyKind, Sense};

    let bytes = include_bytes!("../tests/fixtures/ap214_sheet.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP214 sheet");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir().model.regions.len(), 1);
    assert_eq!(result.ir().model.shells.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert!(result.ir().model.edges.iter().all(|edge| {
        edge.param_range
            .is_some_and(|[start, end]| start.is_finite() && end.is_finite() && start < end)
    }));
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.pcurves.len(), 1);
    assert_eq!(
        result
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
    assert!(matches!(
        result.ir().model.pcurves[0].geometry,
        cadmpeg_ir::geometry::PcurveGeometry::Line { origin, direction }
            if origin == cadmpeg_ir::math::Point2::new(0.0, 0.0)
                && direction == cadmpeg_ir::math::Point2::new(1.0, 0.0)
    ));
    assert!(result
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.sense == Sense::Forward));
    assert_eq!(result.ir().model.faces[0].sense, Sense::Reversed);
    assert!(result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Edge(_)
        )));
    assert_eq!(
        result.ir().model.faces[0].color,
        Some(cadmpeg_ir::topology::Color {
            r: 0.9,
            g: 0.1,
            b: 0.1,
            a: 1.0,
        })
    );
    assert_eq!(result.ir().model.presentation_layers.len(), 1);
    assert_eq!(
        result.ir().model.presentation_layers[0].name,
        "machined faces"
    );
    assert!(matches!(
        result.ir().model.presentation_layers[0].items.as_slice(),
        [cadmpeg_ir::PresentationItem::Face { .. }]
    ));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let mut output = Vec::new();
    let report = write_step(result.ir(), &mut output, &StepWriteOptions::default())
        .expect("write sheet pcurve");
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains("coedge pcurve(s) use unsupported")));
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode written pcurve");
    assert_eq!(roundtrip.ir().model.pcurves.len(), 1);
    assert_eq!(roundtrip.ir().model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(roundtrip.ir().model.presentation_layers.len(), 1);
    assert_eq!(
        roundtrip.ir().model.presentation_layers[0].name,
        "machined faces"
    );
    assert!(roundtrip
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Edge(_)
        )));
    assert_eq!(
        roundtrip
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
}

#[test]
fn linear_extrusion_surface_selects_endpoint_continuous_pcurve() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#28=PLANE('',#27);",
            "#69=VECTOR('',#9,1.);\n#28=SURFACE_OF_LINEAR_EXTRUSION('',#16,#69);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode linear-extrusion sheet");

    assert_eq!(decoded.ir().model.pcurves.len(), 1);
    assert_eq!(
        decoded
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
    let surface_id = SurfaceId("step:data:surface#28".into());
    let index = ModelIndex::new(decoded.ir());
    assert_eq!(
        model_surface_point_by_id(&index, &surface_id, 10.0, 0.0),
        Some(Point3::new(10.0, 0.0, 0.0))
    );
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::ReferenceGraphNotClosed)
            && loss.message.contains("curve #57")
            && loss.message.contains("no pcurve")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn normalized_linear_extrusion_pcurve_is_calibrated_to_surface_endpoints() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#28=PLANE('',#27);",
            "#69=VECTOR('',#9,1.);\n#70=CARTESIAN_POINT('',(0.,0.));\n#71=CARTESIAN_POINT('',(1.,0.));\n#28=SURFACE_OF_LINEAR_EXTRUSION('',#16,#69);",
        )
        .replace(
            "#54=LINE('',#51,#53);",
            "#54=B_SPLINE_CURVE_WITH_KNOTS('',1,(#70,#71),.UNSPECIFIED.,.F.,.F.,(2,2),(0.,1.),.PIECEWISE_BEZIER_KNOTS.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode normalized linear-extrusion pcurve");

    assert_eq!(decoded.ir().model.pcurves.len(), 1);
    assert_eq!(
        decoded
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
    let used_id = decoded
        .ir()
        .model
        .coedges
        .iter()
        .flat_map(|coedge| coedge.pcurves.iter())
        .next()
        .expect("calibrated linear-extrusion pcurve use")
        .pcurve
        .clone();
    assert!(used_id.as_str().starts_with("step:data:pcurve#56-use-"));
    let used = decoded
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id == used_id)
        .expect("calibrated linear-extrusion pcurve");
    assert!(matches!(
        &used.geometry,
        cadmpeg_ir::geometry::PcurveGeometry::Transformed {
            basis,
            transform,
        } if matches!(
            basis.as_ref(),
            cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
                degree: 1,
                control_points,
                ..
        } if control_points == &[Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)]
        ) && (transform.rows[0][0] - 10.0).abs() < 1.0e-12
            && transform.rows[1][1].abs() < 1.0e-12
    ));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::ReferenceGraphNotClosed)
            && loss.message.contains("curve #57")
            && loss.message.contains("no pcurve")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn linear_extrusion_surface_evaluates_a_nurbs_directrix() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#28=PLANE('',#27);",
            "#69=VECTOR('',#9,1.);\n#70=CARTESIAN_POINT('',(0.,0.,0.));\n#71=CARTESIAN_POINT('',(10.,0.,0.));\n#72=B_SPLINE_CURVE_WITH_KNOTS('',1,(#70,#71),.UNSPECIFIED.,.F.,.F.,(2,2),(0.,10.),.PIECEWISE_BEZIER_KNOTS.);\n#28=SURFACE_OF_LINEAR_EXTRUSION('',#72,#69);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode NURBS linear-extrusion sheet");

    let surface_id = SurfaceId("step:data:surface#28".into());
    let index = ModelIndex::new(decoded.ir());
    assert_eq!(
        model_surface_point_by_id(&index, &surface_id, 5.0, 0.0),
        Some(Point3::new(5.0, 0.0, 0.0))
    );
    let partials = model_surface_partials_by_id(&index, &surface_id, 5.0, 0.0)
        .expect("NURBS linear sweep partials");
    assert!((partials.du.x - 1.0).abs() < 1.0e-12);
    assert!(partials.du.y.abs() < 1.0e-12);
    assert!(partials.du.z.abs() < 1.0e-12);
    assert_eq!(partials.dv, Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(decoded.ir().model.pcurves.len(), 1);
    assert_eq!(
        decoded
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
}

#[test]
fn surface_of_revolution_selects_profile_parameter_pcurve() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#28=PLANE('',#27);",
            "#69=AXIS1_PLACEMENT('',#3,#9);\n#28=SURFACE_OF_REVOLUTION('',#16,#69);",
        )
        .replace("#52=DIRECTION('',(1.,0.));", "#52=DIRECTION('',(0.,1.));");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode surface of revolution sheet");

    let surface_id = SurfaceId("step:data:surface#28".into());
    let index = ModelIndex::new(decoded.ir());
    assert_eq!(
        model_surface_point_by_id(&index, &surface_id, 0.0, 10.0),
        Some(Point3::new(10.0, 0.0, 0.0))
    );
    assert_eq!(decoded.ir().model.pcurves.len(), 1);
    assert_eq!(
        decoded
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::ReferenceGraphNotClosed)
            && loss.message.contains("curve #57")
            && loss.message.contains("no pcurve")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn invalid_single_pcurve_is_omitted_instead_of_invalidating_topology() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#52=DIRECTION('',(1.,0.));", "#52=DIRECTION('',(0.,1.));");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode source with invalid pcurve");
    assert!(decoded
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.pcurves.is_empty()));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::PcurveOmitted)
            && loss.message.contains("one optional pcurve")
            && loss.message.contains("not continuous")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn surface_curve_retains_direct_surface_support() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
            "#57=SURFACE_CURVE('',#16,(#70),.CURVE_3D.);\n#70=PLANE('',#27);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode direct surface-curve support");

    let support = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#70")
        .expect("direct surface support carrier");
    assert_eq!(
        support
            .source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        Some("#57")
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn disconnected_source_shell_is_partitioned_into_connected_ir_shells() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#30=OPEN_SHELL('',(#29));",
            "#30=OPEN_SHELL('',(#29,#92));",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#70=CARTESIAN_POINT('',(20.,0.,0.));\n#71=CARTESIAN_POINT('',(30.,0.,0.));\n#72=CARTESIAN_POINT('',(20.,10.,0.));\n#73=VERTEX_POINT('',#70);\n#74=VERTEX_POINT('',#71);\n#75=VERTEX_POINT('',#72);\n#76=DIRECTION('',(0.,0.,1.));\n#77=DIRECTION('',(1.,0.,0.));\n#78=DIRECTION('',(-1.,1.,0.));\n#79=DIRECTION('',(0.,-1.,0.));\n#80=VECTOR('',#77,10.);\n#81=VECTOR('',#78,14.142135623730951);\n#82=VECTOR('',#79,10.);\n#83=LINE('',#70,#80);\n#84=LINE('',#71,#81);\n#85=LINE('',#72,#82);\n#86=EDGE_CURVE('',#73,#74,#83,.T.);\n#87=EDGE_CURVE('',#74,#75,#84,.T.);\n#88=EDGE_CURVE('',#75,#73,#85,.T.);\n#89=ORIENTED_EDGE('',*,*,#86,.T.);\n#90=ORIENTED_EDGE('',*,*,#87,.T.);\n#91=ORIENTED_EDGE('',*,*,#88,.T.);\n#93=EDGE_LOOP('',(#89,#90,#91));\n#94=FACE_OUTER_BOUND('',#93,.T.);\n#95=AXIS2_PLACEMENT_3D('',#70,#76,#77);\n#96=PLANE('',#95);\n#92=ADVANCED_FACE('',(#94),#96,.T.);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode disconnected source shell");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.regions.len(), 1);
    assert_eq!(decoded.ir().model.shells.len(), 2);
    assert_eq!(decoded.ir().model.faces.len(), 2);
    let source_loss = decoded
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.code
                == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::SourceTopologyInvalid)
        })
        .expect("source topology loss");
    assert!(source_loss.message.contains("OPEN_SHELL #30"));
    assert!(source_loss
        .message
        .contains("2 disconnected face components"));
    assert!(source_loss.provenance.is_some());
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn disconnected_brep_outer_shell_is_rejected_without_role_corruption() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#30=OPEN_SHELL('',(#29));",
            "#30=CLOSED_SHELL('',(#29,#92));",
        )
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=BREP_WITH_VOIDS('',#30,(#34));\n#34=CLOSED_SHELL('',(#29));",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#70=CARTESIAN_POINT('',(20.,0.,0.));\n#71=CARTESIAN_POINT('',(30.,0.,0.));\n#72=CARTESIAN_POINT('',(20.,10.,0.));\n#73=VERTEX_POINT('',#70);\n#74=VERTEX_POINT('',#71);\n#75=VERTEX_POINT('',#72);\n#76=DIRECTION('',(0.,0.,1.));\n#77=DIRECTION('',(1.,0.,0.));\n#78=DIRECTION('',(-1.,1.,0.));\n#79=DIRECTION('',(0.,-1.,0.));\n#80=VECTOR('',#77,10.);\n#81=VECTOR('',#78,14.142135623730951);\n#82=VECTOR('',#79,10.);\n#83=LINE('',#70,#80);\n#84=LINE('',#71,#81);\n#85=LINE('',#72,#82);\n#86=EDGE_CURVE('',#73,#74,#83,.T.);\n#87=EDGE_CURVE('',#74,#75,#84,.T.);\n#88=EDGE_CURVE('',#75,#73,#85,.T.);\n#89=ORIENTED_EDGE('',*,*,#86,.T.);\n#90=ORIENTED_EDGE('',*,*,#87,.T.);\n#91=ORIENTED_EDGE('',*,*,#88,.T.);\n#93=EDGE_LOOP('',(#89,#90,#91));\n#94=FACE_OUTER_BOUND('',#93,.T.);\n#95=AXIS2_PLACEMENT_3D('',#70,#76,#77);\n#96=PLANE('',#95);\n#92=ADVANCED_FACE('',(#94),#96,.T.);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode disconnected BREP outer shell");

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("STEP topology root #31 rejected: connected outer shell #30")
    }));
}

#[test]
pub(crate) fn decode_builds_a_valid_ap203_sheet_brep() {
    use cadmpeg_ir::topology::BodyKind;

    let bytes = include_bytes!("../tests/fixtures/ap203_sheet.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP203 sheet");

    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["schema"],
        "CONFIG_CONTROL_DESIGN"
    );
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert_eq!(result.ir().model.vertices.len(), 3);
    let composite = result
        .ir()
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
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| matches!(
            &surface.definition,
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::CurveBounded {
                support,
                boundaries,
                implicit_outer: false,
                ..
            } if support.as_str() == "step:data:surface#28"
                && boundaries.as_slice() == [cadmpeg_ir::ids::CurveId("step:data:curve#34".into())]
        )));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let mut encoded = Vec::new();
    write_step(result.ir(), &mut encoded, &StepWriteOptions::default())
        .expect("write composite curve graph");
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("decode written composite curve graph");
    assert!(roundtrip
        .ir()
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
        result.ir().model.bodies.len(),
        1,
        "{:#?}",
        result.report().losses
    );
    assert_eq!(result.ir().model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("does not resolve to a complete")));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
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
        result.ir().model.bodies.len(),
        1,
        "{:#?}",
        result.report().losses
    );
    assert_eq!(result.ir().model.bodies[0].kind, BodyKind::Solid);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert!(result
        .ir()
        .model
        .edges
        .iter()
        .all(|edge| edge.curve.is_none()));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
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

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    let surface = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#implicit-face-29")
        .expect("implicit face plane");
    let SurfaceGeometry::Plane {
        origin,
        normal,
        u_axis,
    } = &surface.geometry
    else {
        panic!("implicit face did not produce a plane");
    };
    assert_eq!(*normal, Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(*origin, Point3::new(10.0 / 3.0, 10.0 / 3.0, 0.0));
    assert_eq!(*u_axis, Vector3::new(1.0, 0.0, 0.0));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn implicit_face_plane_is_invariant_under_edge_ring_rotation() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#29=FACE('',(#26));",
        );
    let rotated = source.replace(
        "#25=EDGE_LOOP('',(#22,#23,#24));",
        "#25=EDGE_LOOP('',(#23,#24,#22));",
    );
    let first = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode base face");
    let second = StepCodec::default()
        .decode(&mut Cursor::new(rotated), &DecodeOptions::default())
        .expect("decode rotated base face");
    let first_surface = first
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#implicit-face-29")
        .expect("first implicit face plane");
    let second_surface = second
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#implicit-face-29")
        .expect("rotated implicit face plane");
    assert_eq!(first_surface.geometry, second_surface.geometry);
}

#[test]
fn non_planar_base_face_is_rejected_without_an_inferred_surface() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#25=EDGE_LOOP('',(#22,#23,#24));",
            "#25=POLY_LOOP('',(#3,#4,#5));",
        )
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#29=FACE('',(#26));",
        )
        .replace(
            "#25=POLY_LOOP('',(#3,#4,#5));",
            "#70=CARTESIAN_POINT('',(5.,5.,1.));\n#25=POLY_LOOP('',(#3,#4,#5,#70));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode non-planar base face");

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(!decoded
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.as_str() == "step:data:surface#implicit-face-29"));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::TopologyNotTransferred)
            && loss.severity == cadmpeg_ir::Severity::Error
    }));
}

#[test]
fn complex_outer_face_bound_uses_inherited_attributes() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#26=FACE_OUTER_BOUND('',#25,.T.);",
            "#26=(FACE_BOUND('',#25,.T.) FACE_OUTER_BOUND());",
        )
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#29=FACE('',(#26));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex face bound");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    let surface = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#implicit-face-29")
        .expect("implicit face plane");
    assert!(matches!(surface.geometry, SurfaceGeometry::Plane { .. }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn implicit_face_plane_uses_the_outer_loop_only() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#25=EDGE_LOOP('',(#22,#23,#24));",
            "#25=POLY_LOOP('',(#3,#4,#5));",
        )
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#70=CARTESIAN_POINT('',(2.,2.,0.));\n#71=CARTESIAN_POINT('',(2.,3.,0.));\n#72=CARTESIAN_POINT('',(3.,2.,0.));\n#73=POLY_LOOP('',(#70,#71,#72));\n#74=FACE_BOUND('',#73,.F.);\n#29=FACE('',(#74,#26));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode base face with a hole");

    let surface = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#implicit-face-29")
        .expect("implicit face plane");
    let SurfaceGeometry::Plane { normal, .. } = surface.geometry else {
        panic!("implicit face did not produce a plane");
    };
    assert_eq!(normal, Vector3::new(0.0, 0.0, 1.0));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn nearly_collinear_implicit_face_is_rejected_without_a_fabricated_plane() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#5=CARTESIAN_POINT('',(0.,10.,0.));",
            "#5=CARTESIAN_POINT('',(20.,0.0000000000002,0.));",
        )
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#29=FACE('',(#26));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode nearly collinear base face");

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded.ir().model.surfaces.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::TopologyNotTransferred)
            && loss.message.contains("implicit face plane")
    }));
}

#[test]
fn implicit_face_plane_keeps_base_orientation_across_oriented_face() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#25=EDGE_LOOP('',(#22,#23,#24));",
            "#25=POLY_LOOP('',(#3,#4,#5));",
        )
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#29=FACE('',(#26));",
        )
        .replace("#30=OPEN_SHELL('',(#29));", "#30=OPEN_SHELL('',(#34));")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));\n#34=ORIENTED_FACE('',#29,.F.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode oriented base face");

    let surface = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#implicit-face-34")
        .expect("implicit face plane");
    let SurfaceGeometry::Plane { normal, .. } = surface.geometry else {
        panic!("implicit face did not produce a plane");
    };
    assert_eq!(normal, Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(
        decoded.ir().model.faces[0].sense,
        cadmpeg_ir::topology::Sense::Forward
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
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

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    assert!(decoded
        .ir()
        .model
        .edges
        .iter()
        .all(|edge| edge.curve.is_none()));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::ReferenceGraphNotClosed)
            && loss
                .message
                .contains("edge #19 has no decoded surface or curve carrier")
    }));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::DecodeDiagnostic)
            && loss
                .message
                .contains("STEP edge #19 has no 3D curve carrier")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
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

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded.ir().model.vertices.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| loss
        .message
        .contains("VERTEX_POINT #6 has unresolved point carrier #3")));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn rejected_solid_root_reports_an_error_severity_loss() {
    let source =
        String::from_utf8(include_bytes!("../tests/fixtures/ap242_vertex_loop.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace("#10=VERTEX_POINT('',#8);", "#10=VERTEX_POINT('',#4);");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("salvage mode accepts a destroyed solid");

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::TopologyNotTransferred)
            && loss.severity == cadmpeg_ir::Severity::Error
    }));
}

#[test]
fn strict_decode_rejects_a_destroyed_solid() {
    let source =
        String::from_utf8(include_bytes!("../tests/fixtures/ap242_vertex_loop.p21").to_vec())
            .expect("fixture is UTF-8")
            .replace("#10=VERTEX_POINT('',#8);", "#10=VERTEX_POINT('',#4);");
    let mut options = DecodeOptions::default();
    options.policy.mode = DecodeMode::Strict;

    let error = StepCodec::default()
        .decode(&mut Cursor::new(source), &options)
        .expect_err("strict mode rejects a destroyed solid");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
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
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);\n#34=ORIENTED_OPEN_SHELL('',*,#99,.T.);\n#99=UNSUPPORTED_SHELL('',());",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode sheet with one invalid shell");

    assert_eq!(
        decoded.ir().model.bodies.len(),
        1,
        "{:#?}",
        decoded.report().losses
    );
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("omitted 1 unresolved shell")));
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("shell carrier #34")));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
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
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);\n#34=OPEN_SHELL('',(#29));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode shared face shells");

    assert_eq!(
        decoded.ir().model.bodies.len(),
        2,
        "{:#?}",
        decoded.report().losses
    );
    assert_eq!(decoded.ir().model.faces.len(), 2);
    assert_eq!(
        decoded
            .ir()
            .model
            .faces
            .iter()
            .map(|face| face.id.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        2
    );
    assert!(decoded
        .ir()
        .model
        .faces
        .iter()
        .all(|face| face.color.is_some()));
    assert_eq!(decoded.ir().model.presentation_layers[0].items.len(), 2);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn brep_with_voids_scopes_edges_and_vertices_per_shell_after_shared_shell_use() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#30=OPEN_SHELL('',(#29));", "#30=CLOSED_SHELL('',(#29));")
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);\n#34=CLOSED_SHELL('',(#29));\n#70=BREP_WITH_VOIDS('',#30,(#34));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode scoped BREP with voids");

    assert!(decoded.ir().model.bodies.iter().any(|body| {
        body.id.as_str() == "step:data:body#70"
            && body.kind == cadmpeg_ir::topology::BodyKind::Solid
    }));
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("root #70 rejected")));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn first_brep_with_voids_scopes_all_shell_carriers() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#30=OPEN_SHELL('',(#29));", "#30=CLOSED_SHELL('',(#29));")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=BREP_WITH_VOIDS('',#30,(#34));\n#34=CLOSED_SHELL('',(#29));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode first BREP with voids");

    assert!(decoded
        .ir()
        .model
        .bodies
        .iter()
        .any(|body| body.id.as_str() == "step:data:body#31"));
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("root #31 rejected")));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn oriented_shell_reads_the_derived_cfs_faces_slot() {
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(oriented_closed_shell_source(true)),
            &DecodeOptions::default(),
        )
        .expect("decode specification-form oriented closed shell");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert!(decoded
        .ir()
        .model
        .bodies
        .iter()
        .any(|body| body.kind == cadmpeg_ir::topology::BodyKind::Solid));
    assert!(!decoded.report().losses.iter().any(|loss| loss.code
        == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::NoncanonicalSourceSyntax)));
}

#[test]
fn oriented_shell_without_the_derived_slot_is_read_and_reported() {
    let source = oriented_closed_shell_source(false);
    let record_offset = source
        .find("#33=ORIENTED_CLOSED_SHELL")
        .expect("oriented shell record");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode noncanonical oriented closed shell");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    let losses = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code
                == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::NoncanonicalSourceSyntax)
        })
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("ORIENTED_CLOSED_SHELL #33"));
    assert_eq!(
        losses[0]
            .provenance
            .as_ref()
            .expect("oriented shell provenance")
            .offset,
        record_offset as u64
    );
}

#[test]
fn strict_decode_rejects_an_oriented_shell_missing_its_derived_slot() {
    let mut options = DecodeOptions::default();
    options.policy.mode = DecodeMode::Strict;
    let error = StepCodec::default()
        .decode(
            &mut Cursor::new(oriented_closed_shell_source(false)),
            &options,
        )
        .expect_err("strict mode rejects a noncanonical oriented shell");

    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
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
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
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
        .ir()
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
fn unreferenced_curve_is_associated_as_free_geometry() {
    let decoded = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT($,.METRE.));
#2=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));
#3=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#2)) REPRESENTATION_CONTEXT('model','3D'));
#10=CARTESIAN_POINT('',(0.,0.,0.));
#11=DIRECTION('',(0.,0.,1.));
#12=DIRECTION('',(1.,0.,0.));
#13=AXIS2_PLACEMENT_3D('',#10,#11,#12);
#14=CIRCLE('',#13,2.);",
    );
    let curve = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#14")
        .expect("unreferenced circle");
    assert_eq!(
        curve
            .source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        Some("#14")
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn pcurve_trimmed_carrier_is_not_promoted_to_a_3d_curve() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#69=PCURVE('',#28,#70);\n#70=DEFINITIONAL_REPRESENTATION('',(#72),#50);\n#71=LINE('',#51,#53);\n#72=TRIMMED_CURVE('',#71,(0.),(1.),.T.,.PARAMETER.);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode pcurve trimmed carrier");

    assert!(decoded.ir().model.curves.iter().all(|curve| {
        curve.id.as_str() != "step:data:curve#71" && curve.id.as_str() != "step:data:curve#72"
    }));
    assert!(decoded.report().losses.iter().all(|loss| {
        !loss
            .message
            .contains("TRIMMED_CURVE #72 has invalid or unresolved basis/trim selectors")
    }));
}

#[test]
fn trimmed_curve_resolves_a_surface_curve_basis_carrier() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#70=TRIMMED_CURVE('',#57,(0.),(1.),.T.,.PARAMETER.);\n#71=GEOMETRIC_SET('',(#70));\n#72=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#71),#2);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode surface-curve trim");

    assert!(decoded.ir().model.curves.iter().any(|curve| {
        curve.id.as_str() == "step:data:curve#70"
            && matches!(curve.geometry, CurveGeometry::Line { .. })
    }));
    assert!(decoded.ir().model.procedural_curves.iter().any(|curve| {
        curve.curve.as_str() == "step:data:curve#70"
            && matches!(
                &curve.definition,
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset { source, .. }
                    if source.as_str() == "step:data:curve#16"
            )
    }));
    assert!(decoded.report().losses.iter().all(|loss| {
        !loss
            .message
            .contains("TRIMMED_CURVE #70 has invalid or unresolved basis/trim selectors")
    }));
}

#[test]
fn pcurve_trimmed_opposed_sense_has_an_ordered_parameter_range() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#53=VECTOR('',#52,1.);",
            "#53=VECTOR('',#52,10.);",
        )
        .replace(
            "#54=LINE('',#51,#53);",
            "#54=TRIMMED_CURVE('',#71,(PARAMETER_VALUE(1.)),(PARAMETER_VALUE(0.)),.F.,.PARAMETER.);\n#71=LINE('',#51,#53);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode opposed-sense pcurve trim");
    let pcurve = decoded
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == "step:data:pcurve#56")
        .expect("trimmed pcurve");
    assert!(matches!(
        &pcurve.geometry,
        cadmpeg_ir::geometry::PcurveGeometry::Trimmed {
            parameter_range: [start, end],
            ..
        } if *start == 0.0 && *end == 1.0
    ));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn pcurve_trimmed_stale_range_recovers_the_edge_use_interval() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#53=VECTOR('',#52,1.);", "#53=VECTOR('',#52,10.);")
        .replace(
            "#54=LINE('',#51,#53);",
            "#54=TRIMMED_CURVE('',#71,(PARAMETER_VALUE(-1.)),(PARAMETER_VALUE(2.)),.T.,.PARAMETER.);\n#71=LINE('',#51,#53);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode stale pcurve trim");
    let pcurve = cadmpeg_ir::ids::PcurveId("step:data:pcurve#56".into());
    let use_ = decoded
        .ir()
        .model
        .coedges
        .iter()
        .flat_map(|coedge| &coedge.pcurves)
        .find(|use_| use_.pcurve == pcurve)
        .expect("stale trimmed pcurve use");
    assert_eq!(use_.parameter_range, Some([0.0, 1.0]));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn a_protected_unowned_pcurve_stays_opaque() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
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
fn retention_reports_every_deleted_carrier_category() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
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
fn decode_maps_a_two_dimensional_polyline_to_a_pcurve_nurbs() {
    use cadmpeg_ir::geometry::PcurveGeometry;
    use cadmpeg_ir::math::Point2;

    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#52=DIRECTION('',(1.,0.));",
            "#52=CARTESIAN_POINT('',(1.,2.));",
        )
        .replace(
            "#4=CARTESIAN_POINT('',(10.,0.,0.));",
            "#4=CARTESIAN_POINT('',(3.,2.,0.));",
        )
        .replace("#53=VECTOR('',#52,1.);", "#53=CARTESIAN_POINT('',(3.,2.));")
        .replace("#54=LINE('',#51,#53);", "#54=POLYLINE('',(#51,#52,#53));");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode polyline pcurve");

    assert!(matches!(
        &decoded.ir().model.pcurves[0].geometry,
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
    assert_eq!(decoded.ir().model.bodies.len(), 1);
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
        .ir()
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
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn cylindrical_pcurve_coordinates_follow_surface_parameter_units() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("SI_UNIT(.MILLI.,.METRE.)", "SI_UNIT(.CENTI.,.METRE.)")
        .replace(
            "#4=CARTESIAN_POINT('',(10.,0.,0.));",
            "#4=CARTESIAN_POINT('',(1.,0.,0.));",
        )
        .replace("#13=VECTOR('',#10,10.);", "#13=VECTOR('',#10,1.);")
        .replace(
            "#27=AXIS2_PLACEMENT_3D('',#3,#9,#10);",
            "#70=CARTESIAN_POINT('',(0.,-1.,0.));\n#71=DIRECTION('',(1.,0.,0.));\n#72=DIRECTION('',(0.,1.,0.));\n#27=AXIS2_PLACEMENT_3D('',#70,#71,#72);",
        )
        .replace("#28=PLANE('',#27);", "#28=CYLINDRICAL_SURFACE('',#27,1.);")
        .replace("#52=DIRECTION('',(1.,0.));", "#52=DIRECTION('',(0.,1.));");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode cylindrical pcurve");

    let pcurve = decoded
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == "step:data:pcurve#56")
        .expect("cylindrical pcurve");
    assert!(matches!(
        pcurve.geometry,
        cadmpeg_ir::geometry::PcurveGeometry::Line { direction, .. }
            if direction.u.abs() < 1.0e-12 && (direction.v - 10.0).abs() < 1.0e-12
    ));
}

#[test]
fn degree_valued_cylindrical_pcurve_is_not_reinterpreted() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#3=CARTESIAN_POINT('',(0.,0.,0.));",
            "#3=CARTESIAN_POINT('',(-1.,0.,0.));",
        )
        .replace(
            "#4=CARTESIAN_POINT('',(10.,0.,0.));",
            "#4=CARTESIAN_POINT('',(-1.,10.,0.));",
        )
        .replace(
            "#10=DIRECTION('',(1.,0.,0.));",
            "#10=DIRECTION('',(0.,1.,0.));",
        )
        .replace(
            "#27=AXIS2_PLACEMENT_3D('',#3,#9,#10);",
            "#70=CARTESIAN_POINT('',(0.,0.,0.));\n#71=DIRECTION('',(0.,1.,0.));\n#72=DIRECTION('',(1.,0.,0.));\n#27=AXIS2_PLACEMENT_3D('',#70,#71,#72);",
        )
        .replace("#28=PLANE('',#27);", "#28=CYLINDRICAL_SURFACE('',#27,1.);")
        .replace(
            "#51=CARTESIAN_POINT('',(0.,0.));",
            "#51=CARTESIAN_POINT('',(180.,0.));",
        )
        .replace("#52=DIRECTION('',(1.,0.));", "#52=DIRECTION('',(0.,1.));")
        .replace("#53=VECTOR('',#52,1.);", "#53=VECTOR('',#52,10.);");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode degree-valued cylindrical pcurve");

    assert!(decoded
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.pcurves.is_empty()));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::PcurveOmitted)
            && loss.message.contains("curve #57")
            && loss.message.contains("pcurve is omitted")
    }));
}

#[test]
fn periodic_surface_pcurve_selection_seeds_line_parameter_branches() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#3=CARTESIAN_POINT('',(0.,0.,0.));",
            "#3=CARTESIAN_POINT('',(-1.,0.,0.));",
        )
        .replace(
            "#4=CARTESIAN_POINT('',(10.,0.,0.));",
            "#4=CARTESIAN_POINT('',(-0.5403023058681398,0.,0.8414709848078965));",
        )
        .replace("#16=LINE('',#3,#13);", "#16=CIRCLE('',#27,1.);")
        .replace(
            "#27=AXIS2_PLACEMENT_3D('',#3,#9,#10);",
            "#70=CARTESIAN_POINT('',(0.,0.,0.));\n#71=DIRECTION('',(0.,1.,0.));\n#72=DIRECTION('',(1.,0.,0.));\n#27=AXIS2_PLACEMENT_3D('',#70,#71,#72);",
        )
        .replace("#28=PLANE('',#27);", "#28=CYLINDRICAL_SURFACE('',#27,1.);");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode periodic cylindrical pcurve");

    assert!(decoded.ir().model.coedges.iter().any(|coedge| {
        coedge
            .pcurves
            .iter()
            .any(|use_| use_.pcurve.as_str() == "step:data:pcurve#56")
    }));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::ReferenceGraphNotClosed)
            && loss.message.contains("curve #57")
            && loss.message.contains("no pcurve")
    }));
}

#[test]
fn unsupported_optional_pcurve_does_not_discard_valid_topology() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#54=LINE('',#51,#53);", "#54=UNSUPPORTED_CURVE('',#51);");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode sheet with unsupported optional pcurve");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    assert!(decoded
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.pcurves.is_empty()));
    assert!(decoded.report().losses.iter().any(|loss| loss
        .message
        .contains("PCURVE #56 has no decoded surface or 2D curve")));
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("conflicts with decoded topology")));
}

#[test]
fn inconsistent_optional_pcurve_is_omitted_and_retained_as_source_data() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#51=CARTESIAN_POINT('',(0.,0.));",
            "#51=CARTESIAN_POINT('',(0.,1.));",
        )
        .replace(
            "#54=LINE('',#51,#53);",
            "#54=TRIMMED_CURVE('',#71,(0.),(10.),.T.,.PARAMETER.);",
        )
        .replace(
            "#68=STYLED_ITEM('',(#66),#19);",
            "#68=STYLED_ITEM('',(#66),#19);\n#71=LINE('',#51,#53);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode inconsistent optional pcurve");

    assert!(decoded
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.pcurves.is_empty()));
    assert!(
        decoded.report().losses.iter().any(|loss| {
            loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::PcurveOmitted)
                && loss.severity == cadmpeg_ir::Severity::Error
                && loss.message.contains("optional pcurve")
        }),
        "{:#?}",
        decoded.report().losses
    );
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.0 == "step:data:pcurve#56"));

    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
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

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    assert!(matches!(
        decoded
            .ir()
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
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == "step:data:surface#28")
            .map(|surface| &surface.geometry),
        Some(SurfaceGeometry::Unknown { record: Some(record) })
            if record.as_str() == "step:data:unsupported_surface#28"
    ));
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("conflicts with decoded topology")));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
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

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert!(matches!(
        decoded
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == "step:data:surface#28")
            .map(|surface| &surface.geometry),
        Some(SurfaceGeometry::Unknown { record: Some(record) })
            if record.as_str() == "step:data:unsupported_surface#28"
    ));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
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
fn failed_void_shell_does_not_commit_the_outer_brep() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=BREP_WITH_VOIDS('',#30,(#34));",
        )
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=OPEN_SHELL('',(#29));\n#34=OPEN_SHELL('',(#99));\n#99=UNSUPPORTED_FACE('',());",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode BREP with invalid void shell");

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.0 == "step:data:brep_with_voids#31"));
    assert!(decoded.report().losses.iter().any(|loss| loss
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

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.0 == "step:data:shell_based_surface_model#31"));
    assert!(decoded
        .report()
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

    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| loss
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

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert!(decoded.ir().model.coedges.iter().any(|coedge| {
        coedge
            .pcurves
            .iter()
            .any(|use_| use_.pcurve.as_str() == "step:data:pcurve#56")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn seam_edge_does_not_guess_an_unlisted_pcurve_reference() {
    let source = equivalent_seam_source()
        .replace(
            "#22=ORIENTED_EDGE('',*,*,#19,.T.);",
            "#22=SEAM_EDGE('',*,*,#19,.T.,#75);",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#72=CARTESIAN_POINT('',(0.,0.));\n#73=LINE('',#72,#53);\n#74=DEFINITIONAL_REPRESENTATION('',(#73),#50);\n#75=PCURVE('',#28,#74);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode seam edge with an unlisted pcurve");

    assert!(decoded.ir().model.coedges.iter().all(|coedge| {
        coedge
            .pcurves
            .iter()
            .all(|use_| use_.pcurve.as_str() != "step:data:pcurve#75")
    }));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::ReferenceGraphNotClosed)
            && loss.severity == cadmpeg_ir::Severity::Warning
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn seam_edge_rejects_an_explicit_pcurve_outside_its_curve() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#22=ORIENTED_EDGE('',*,*,#19,.T.);",
            "#22=SEAM_EDGE('',*,*,#19,.T.,#75);",
        )
        .replace(
            "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
            "#57=SEAM_CURVE('',#16,(#56),.PCURVE_S1.);",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#72=CARTESIAN_POINT('',(0.,0.));\n#73=LINE('',#72,#53);\n#74=DEFINITIONAL_REPRESENTATION('',(#73),#50);\n#75=PCURVE('',#28,#74);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode seam edge with an unlisted pcurve");

    assert!(decoded.ir().model.coedges.iter().all(|coedge| {
        coedge
            .pcurves
            .iter()
            .all(|use_| use_.pcurve.as_str() != "step:data:pcurve#75")
    }));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::ReferenceGraphNotClosed)
            && loss.message.contains("SEAM_EDGE #22")
            && loss.message.contains("belongs to its edge curve")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

fn equivalent_seam_source() -> String {
    String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
            "#57=SEAM_CURVE('',#16,(#56,#69),.PCURVE_S1.);",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#69=PCURVE('',#28,#70);\n#70=DEFINITIONAL_REPRESENTATION('',(#71),#50);\n#71=LINE('',#51,#53);\nENDSEC;\nEND-ISO-10303-21;",
        )
}

fn distinct_seam_source() -> String {
    equivalent_seam_source().replace(
        "#71=LINE('',#51,#53);",
        "#71=POLYLINE('',(#51,#72,#73));\n#72=CARTESIAN_POINT('',(5.,5.));\n#73=CARTESIAN_POINT('',(10.,0.));",
    )
}

fn seam_source_with_one_endpoint_continuous_candidate() -> String {
    equivalent_seam_source().replace(
        "#71=LINE('',#51,#53);",
        "#71=LINE('',#72,#53);\n#72=CARTESIAN_POINT('',(0.,5.));",
    )
}

#[test]
fn equivalent_seam_pcurve_candidates_select_one_carrier() {
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(equivalent_seam_source()),
            &DecodeOptions::default(),
        )
        .expect("decode equivalent seam pcurves");

    let coedge = decoded
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| !coedge.pcurves.is_empty())
        .expect("equivalent seam coedge");
    assert_eq!(coedge.pcurves.len(), 1);
    assert_eq!(coedge.pcurves[0].pcurve.as_str(), "step:data:pcurve#56");
    assert!(decoded.report().losses.iter().all(|loss| !loss
        .message
        .contains("no unique endpoint-continuous pcurve selects one")));

    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn distinct_tied_seam_pcurve_candidates_are_reported_not_guessed() {
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(distinct_seam_source()),
            &DecodeOptions::default(),
        )
        .expect("decode distinct tied seam pcurves");

    assert!(decoded
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.pcurves.is_empty()));
    let losses: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code
                == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::ReferenceGraphNotClosed)
        })
        .collect();
    assert_eq!(
        losses.len(),
        1,
        "unexpected losses: {:#?}",
        decoded.report().losses
    );
    assert_eq!(losses[0].severity, cadmpeg_ir::Severity::Warning);
    assert_eq!(
        losses[0].message,
        "curve #57 associates 2 pcurves with surface #28; no unique endpoint-continuous pcurve selects one, so the coedge has no pcurve"
    );
}

#[test]
fn endpoint_continuity_selects_the_unique_seam_pcurve_candidate() {
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(seam_source_with_one_endpoint_continuous_candidate()),
            &DecodeOptions::default(),
        )
        .expect("decode endpoint-continuous seam pcurve");

    let coedge = decoded
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| !coedge.pcurves.is_empty())
        .expect("seam coedge");
    assert_eq!(coedge.pcurves.len(), 1);
    assert_eq!(coedge.pcurves[0].pcurve.as_str(), "step:data:pcurve#56");
    assert!(decoded.report().losses.iter().all(|loss| !loss
        .message
        .contains("no unique endpoint-continuous pcurve selects one")));

    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn an_unambiguous_pcurve_still_binds() {
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!("../tests/fixtures/ap214_sheet.p21")),
            &DecodeOptions::default(),
        )
        .expect("decode unambiguous pcurve");

    assert!(decoded.ir().model.coedges.iter().any(|coedge| {
        coedge
            .pcurves
            .iter()
            .any(|use_| use_.pcurve.as_str() == "step:data:pcurve#56")
    }));
}

#[test]
fn ambiguous_pcurves_do_not_reject_the_body() {
    use cadmpeg_ir::topology::BodyKind;

    let source = distinct_seam_source()
        .replace("#30=OPEN_SHELL('',(#29));", "#30=CLOSED_SHELL('',(#29));")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=MANIFOLD_SOLID_BREP('',#30);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode solid with ambiguous seam pcurves");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Solid);
    assert!(decoded.report().losses.iter().any(|loss| loss.code
        == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::ReferenceGraphNotClosed)));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn omitted_geometry_names_preserve_intersection_curve_topology() {
    let mut source =
        String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
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
            loss.code
                == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::NoncanonicalSourceSyntax)
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

#[test]
fn intersection_curve_binds_its_basis_curve_and_pcurves() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#57=SURFACE_CURVE('',#16,(#56),.PCURVE_S1.);",
            "#57=INTERSECTION_CURVE('',#16,(#56),.PCURVE_S1.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode intersection curve");

    let edge = decoded
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.id.as_str() == "step:data:edge#19")
        .expect("intersection-curve edge");
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
    assert!(decoded.report().losses.iter().all(|loss| !loss
        .message
        .contains("surface-curve #57 has no resolvable basis")));
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

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert!(decoded
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.id.as_str() == "step:data:edge#19")
        .is_some_and(|edge| edge.curve.is_none()));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("STEP edge curve #19: surface-curve #57 has no resolvable basis")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
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

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert!(decoded.ir().model.edges.iter().any(|edge| {
        edge.id.as_str() == "step:data:edge#19"
            && edge
                .curve
                .as_ref()
                .is_some_and(|curve| curve.as_str() == "step:data:curve#18")
    }));
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .all(|record| record.id.0 != "step:data:subedge#19"));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
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

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert!(decoded
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.sense == cadmpeg_ir::topology::Sense::Reversed));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn nested_oriented_faces_compose_back_to_the_base_orientation() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#30=OPEN_SHELL('',(#29));", "#30=OPEN_SHELL('',(#35));")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));\n#34=ORIENTED_FACE('',#29,.F.);\n#35=ORIENTED_FACE('',#34,.F.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode nested oriented faces");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert_eq!(
        decoded.ir().model.faces[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert!(decoded
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.sense == cadmpeg_ir::topology::Sense::Forward));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_oriented_open_shell_preserves_shell_sense() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=(OPEN_SHELL('',(#29)) ORIENTED_OPEN_SHELL('',*,#30,.F.));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex oriented shell");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(
        decoded.ir().model.faces[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
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

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
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
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=WIRE_SHELL('',(#25));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode shell-based wireframe model");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Wire);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
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
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=VERTEX_SHELL('',#34);\n#34=VERTEX_LOOP('',#6);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode vertex shell");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Wire);
    assert_eq!(decoded.ir().model.shells[0].free_vertices.len(), 1);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
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
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=CONNECTED_EDGE_SUB_SET('',(#19,#20,#21),#34);\n#34=CONNECTED_EDGE_SET('',(#19,#20,#21));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode connected edge sub set");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Wire);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("has no resolvable parent")));
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.0 == "step:data:connected_edge_set#34"));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
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
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=CONNECTED_EDGE_SUB_SET('',(#19,#20,#21),#34);\n#34=UNSUPPORTED_SET('',());",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode subset with invalid parent");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("parent #34 does not resolve")));
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.0 == "step:data:connected_edge_sub_set#33"));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
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
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#34=CONNECTED_FACE_SUB_SET('',(#29),#30);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode connected face subset");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("CONNECTED_FACE_SUB_SET #34")));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
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
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#70=CONNECTED_EDGE_SET('',(#71,#72,#73));\n#71=ORIENTED_EDGE('',*,*,#19,.F.);\n#72=SEAM_EDGE('',*,*,#20,.T.,#56);\n#73=ORIENTED_EDGE('',*,*,#21,.T.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode direct oriented and seam edge members");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Wire);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    let reversed = decoded
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.id.as_str().starts_with("step:data:edge#71-"))
        .expect("oriented edge carrier");
    assert!(reversed.start.as_str().contains("vertex#7"));
    assert!(reversed.end.as_str().contains("vertex#6"));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
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
        .replace("#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);", "#33=OPEN_SHELL('',(#29));");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode shared wire model representations");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert!(!decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| { record.id.0 == "step:data:manifold_surface_shape_representation#71" }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_representation_items_reach_edge_based_wire_models() {
    use cadmpeg_ir::topology::BodyKind;

    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=EDGE_BASED_WIREFRAME_MODEL('',(#70));\n#70=CONNECTED_EDGE_SET('',(#19,#20,#21));\n#71=(MANIFOLD_SURFACE_SHAPE_REPRESENTATION() REPRESENTATION('',(#31),#2) SHAPE_REPRESENTATION());",
        )
        .replace("#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);", "#33=OPEN_SHELL('',(#29));");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex wire representation");

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.bodies[0].kind, BodyKind::Wire);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_shape_representation_is_typed_for_free_representation_items() {
    let decoded = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));
#3=CARTESIAN_POINT('free point',(1.,2.,3.));
#4=(REPRESENTATION('free shape',(#3),#2) SHAPE_REPRESENTATION());",
    );

    assert_eq!(decoded.ir().model.points.len(), 1);
    assert_eq!(
        decoded.ir().model.points[0]
            .source_object
            .as_ref()
            .and_then(|source| source.name.as_deref()),
        Some("free point")
    );
    assert!(!decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.0 == "step:data:shape_representation#4"));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
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

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
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

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert!(decoded.ir().model.surfaces.iter().any(|surface| {
        surface.id.as_str() == "step:data:surface#28"
            && matches!(surface.geometry, SurfaceGeometry::Cylinder { .. })
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn advanced_face_name_transfers_through_inherited_representation_item() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#29=ADVANCED_FACE('named face',(#26),#28,.T.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode named face");
    assert_eq!(
        decoded.ir().model.faces[0].name.as_deref(),
        Some("named face")
    );

    let mut output = Vec::new();
    write_step(decoded.ir(), &mut output, &StepWriteOptions::default()).expect("write named face");
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode written named face");
    assert_eq!(
        roundtrip.ir().model.faces[0].name.as_deref(),
        Some("named face")
    );
}

#[test]
fn complex_advanced_face_name_uses_representation_item_partial() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#29=ADVANCED_FACE('',(#26),#28,.T.);",
            "#29=(FACE('',(#26)) FACE_SURFACE('',#28,.T.) ADVANCED_FACE() REPRESENTATION_ITEM('complex named face'));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex named face");
    assert_eq!(
        decoded.ir().model.faces[0].name.as_deref(),
        Some("complex named face")
    );
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

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.vertices.len(), 3);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_geometry_instances_decode_named_partials() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#27=AXIS2_PLACEMENT_3D('',#3,#9,#10);",
            "#27=(AXIS2_PLACEMENT_3D('',#3,#9,#10) PLACEMENT());",
        )
        .replace(
            "#28=PLANE('',#27);",
            "#28=(GEOMETRIC_REPRESENTATION_ITEM() PLANE('',#27) SURFACE());",
        )
        .replace("#16=LINE('',#3,#13);", "#16=(CURVE() LINE('',#3,#13));")
        .replace("#54=LINE('',#51,#53);", "#54=(CURVE() LINE('',#51,#53));")
        .replace(
            "#56=PCURVE('',#28,#55);",
            "#56=(CURVE() PCURVE('',#28,#55));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex geometry instances");

    assert!(decoded.ir().model.curves.iter().any(|curve| {
        curve.id.as_str() == "step:data:curve#16"
            && matches!(curve.geometry, CurveGeometry::Line { .. })
    }));
    assert!(decoded.ir().model.surfaces.iter().any(|surface| {
        surface.id.as_str() == "step:data:surface#28"
            && matches!(surface.geometry, SurfaceGeometry::Plane { .. })
    }));
    assert_eq!(decoded.ir().model.pcurves.len(), 1);
    assert!(matches!(
        decoded.ir().model.pcurves[0].geometry,
        cadmpeg_ir::geometry::PcurveGeometry::Line { .. }
    ));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_points_and_directions_decode_named_partials() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#3=CARTESIAN_POINT('',(0.,0.,0.));",
            "#3=(CARTESIAN_POINT('',(0.,0.,0.)) GEOMETRIC_REPRESENTATION_ITEM() POINT(''));",
        )
        .replace(
            "#9=DIRECTION('',(0.,0.,1.));",
            "#9=(DIRECTION('',(0.,0.,1.)) GEOMETRIC_REPRESENTATION_ITEM());",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex points and directions");

    assert_eq!(decoded.ir().model.vertices.len(), 3);
    assert!(decoded.ir().model.surfaces.iter().any(|surface| {
        surface.id.as_str() == "step:data:surface#28"
            && matches!(surface.geometry, SurfaceGeometry::Plane { .. })
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_a_sheet_from_a_geometric_surface_set() {
    use cadmpeg_ir::topology::BodyKind;

    let bytes = include_bytes!("../tests/fixtures/ap242_geometric_set.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode geometric surface set");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert!(result.ir().model.faces[0].loops.is_empty());
    assert_eq!(
        result.ir().model.faces[0].surface.as_str(),
        "step:data:surface#11"
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_geometric_set_representation_uses_its_named_items() {
    use cadmpeg_ir::topology::BodyKind;

    let mut source = String::from_utf8(include_bytes!(
        "../tests/fixtures/ap242_geometric_set.p21"
    )
    .to_vec())
    .expect("fixture is UTF-8")
    .replace(
        "#12=GEOMETRIC_SET('',(#11));",
        "#12=GEOMETRIC_SET('',(#11,#14,#15));",
    )
    .replace(
        "#13=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#12),#2);",
        "#13=(GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION() REPRESENTATION('',(#12),#2) SHAPE_REPRESENTATION());",
    );
    let end = source.rfind("ENDSEC;").expect("STEP data section end");
    source.insert_str(
        end,
        "#14=(CIRCLE('',#6,5.) CURVE() GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('free circle'));\n#15=UNSUPPORTED_ITEM('');\n",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex geometric surface set");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(result.ir().model.faces.len(), 1);
    let free_circle = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#14")
        .expect("free complex representation member");
    assert_eq!(
        free_circle
            .source_object
            .as_ref()
            .and_then(|source| source.name.as_deref()),
        Some("free circle")
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains(
            "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION #13 omitted unsupported or unresolved member(s): #15",
        )
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn direct_boundary_curve_builds_a_curve_bounded_surface() {
    for boundary_type in ["BOUNDARY_CURVE", "OUTER_BOUNDARY_CURVE"] {
        let source = format!(
            "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n\
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));\n\
#3=CARTESIAN_POINT('',(0.,0.,0.));\n\
#4=DIRECTION('',(0.,0.,1.));\n\
#5=DIRECTION('',(1.,0.,0.));\n\
#6=AXIS2_PLACEMENT_3D('',#3,#4,#5);\n\
#7=CIRCLE('',#6,5.);\n\
#8=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#7);\n\
#9={boundary_type}('',(#8),.F.);\n\
#10=PLANE('',#6);\n\
#11=CURVE_BOUNDED_SURFACE('bounded',#10,(#9),.F.);\n\
#12=GEOMETRIC_SET('',(#11));\n\
#13=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#12),#2);\n",
        );
        let result = decode_inline(&source);

        let boundary = result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == "step:data:curve#9")
            .expect("boundary curve carrier");
        assert!(matches!(
            &boundary.geometry,
            CurveGeometry::Composite { segments, .. }
                if segments.len() == 1 && segments[0].curve.as_str() == "step:data:curve#7"
        ));

        let bounded = result
            .ir()
            .model
            .procedural_surfaces
            .iter()
            .find(|surface| surface.id.as_str() == "step:construction:curve_bounded_surface#11")
            .expect("curve-bounded surface");
        assert!(matches!(
            &bounded.definition,
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::CurveBounded { boundaries, .. }
                if boundaries == &[CurveId("step:data:curve#9".to_owned())]
        ));
        assert!(!result.report().losses.iter().any(|loss| {
            loss.message
                .contains("has invalid, cyclic, or unresolved segments")
        }));
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn complex_surface_curve_pcurve_is_retained_by_curve_bounded_surface() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap203_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#36=COMPOSITE_CURVE('nested edge',(#33),.F.);",
            "#36=(BOUNDED_CURVE() CURVE() GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('nested edge') SURFACE_CURVE(#16,(#44),.PCURVE_S1.));",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#38=(GEOMETRIC_REPRESENTATION_CONTEXT(2) PARAMETRIC_REPRESENTATION_CONTEXT() REPRESENTATION_CONTEXT('uv','2D'));\n#39=CARTESIAN_POINT('',(0.,0.));\n#40=DIRECTION('',(1.,0.));\n#41=VECTOR('',#40,1.);\n#42=LINE('',#39,#41);\n#43=DEFINITIONAL_REPRESENTATION('',(#42),#38);\n#44=PCURVE('',#28,#43);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex surface curve boundary");

    let (boundaries, boundary_pcurves) = decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find_map(|surface| match &surface.definition {
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::CurveBounded {
                boundaries,
                boundary_pcurves,
                ..
            } => Some((boundaries, boundary_pcurves)),
            _ => None,
        })
        .expect("curve-bounded surface");
    assert_eq!(
        boundaries,
        &[cadmpeg_ir::ids::CurveId("step:data:curve#34".into())]
    );
    assert!(
        decoded
            .ir()
            .model
            .pcurves
            .iter()
            .any(|pcurve| pcurve.id.as_str() == "step:data:pcurve#44"),
        "pcurves={:#?}, losses={:#?}",
        decoded.ir().model.pcurves,
        decoded.report().losses
    );
    assert_eq!(
        boundary_pcurves,
        &[cadmpeg_ir::ids::PcurveId("step:data:pcurve#44".into())]
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn free_surface_curve_keeps_its_three_dimensional_basis_reachable() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#80=CARTESIAN_POINT('',(20.,0.,0.));\n#81=DIRECTION('',(1.,0.,0.));\n#82=VECTOR('',#81,1.);\n#83=LINE('',#80,#82);\n#84=SURFACE_CURVE('',#83,(#56),.PCURVE_S1.);\n#85=GEOMETRIC_SET('free surface curve',(#84));\nENDSEC;\nEND-ISO-10303-21;",
        );
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(source.as_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode free surface curve");

    let basis = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "step:data:curve#83")
        .expect("surface-curve basis");
    assert_eq!(
        basis
            .source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        Some("#84")
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(!validation.findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::report::Check::CarrierReachability
            && finding.entity.as_deref() == Some("step:data:curve#83")
    }));
}

#[test]
fn rectangular_trimmed_surface_preserves_basis_ranges_and_senses() {
    let source = "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));\
#3=CARTESIAN_POINT('',(0.,0.,0.));\
#4=DIRECTION('',(0.,0.,1.));\
#5=DIRECTION('',(1.,0.,0.));\
#6=AXIS2_PLACEMENT_3D('',#3,#4,#5);\
#7=PLANE('',#6);\
#8=RECTANGULAR_TRIMMED_SURFACE('trim',#7,3.,1.,4.,2.,.F.,.F.);\
#9=GEOMETRIC_SET('',(#8));\
#10=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#9),#2);";
    let decoded = decode_inline(source);
    let trimmed = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#8")
        .expect("trimmed surface carrier");
    assert!(matches!(trimmed.geometry, SurfaceGeometry::Plane { .. }));
    let procedural = decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|surface| surface.surface.as_str() == "step:data:surface#8")
        .expect("trimmed surface construction");
    assert!(matches!(
        &procedural.definition,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Subset {
            support,
            parameter_ranges: [[3.0, 1.0], [4.0, 2.0]],
            u_sense: Some(false),
            v_sense: Some(false),
        } if support.as_str() == "step:data:surface#7"
    ));
    let index = ModelIndex::new(decoded.ir());
    let trimmed_id = SurfaceId("step:data:surface#8".into());
    assert_eq!(
        model_surface_point_by_id(&index, &trimmed_id, 0.0, 0.0),
        Some(Point3::new(3.0, 4.0, 0.0))
    );
    assert_eq!(
        model_surface_point_by_id(&index, &trimmed_id, 2.0, 2.0),
        Some(Point3::new(1.0, 2.0, 0.0))
    );
    let partials = model_surface_partials_by_id(&index, &trimmed_id, 1.0, 1.0)
        .expect("trimmed surface partials");
    assert_eq!(partials.du, Vector3::new(-1.0, 0.0, 0.0));
    assert_eq!(partials.dv, Vector3::new(0.0, -1.0, 0.0));

    let mut output = Vec::new();
    let report = write_step(decoded.ir(), &mut output, &StepWriteOptions::default())
        .expect("write trimmed surface");
    let text = String::from_utf8(output.clone()).expect("UTF-8 STEP");
    assert!(text.contains("RECTANGULAR_TRIMMED_SURFACE"));
    assert!(!report.losses.iter().any(|loss| {
        loss.message
            .contains("procedural surface definition(s) and")
    }));

    let round_trip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode trimmed surface");
    let round_trip = round_trip
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|surface| {
            matches!(
                &surface.definition,
                cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Subset {
                    parameter_ranges: [[3.0, 1.0], [4.0, 2.0]],
                    u_sense: Some(false),
                    v_sense: Some(false),
                    ..
                }
            )
        })
        .expect("round-trip trimmed surface construction");
    assert!(matches!(
        &round_trip.definition,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Subset {
            parameter_ranges: [[3.0, 1.0], [4.0, 2.0]],
            u_sense: Some(false),
            v_sense: Some(false),
            ..
        }
    ));
}

#[test]
fn rectangular_trimmed_surface_unwraps_cyclic_basis_parameters() {
    let source = "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=CYLINDRICAL_SURFACE('',#4,2.);
#6=RECTANGULAR_TRIMMED_SURFACE('trim',#5,5.5,.5,1.,3.,.T.,.T.);
#7=GEOMETRIC_SET('',(#6));
#8=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#7),#9);
#9=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));";
    let decoded = decode_inline(source);
    let construction = decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|surface| surface.surface.as_str() == "step:data:surface#6")
        .expect("cyclic trimmed surface construction");
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Subset {
        parameter_ranges,
        u_sense: Some(true),
        v_sense: Some(true),
        ..
    } = &construction.definition
    else {
        panic!(
            "unexpected cyclic trimmed definition: {:?}",
            construction.definition
        );
    };
    assert!((parameter_ranges[0][0] - 5.5).abs() < 1.0e-12);
    assert!((parameter_ranges[0][1] - (0.5 + std::f64::consts::TAU)).abs() < 1.0e-12);
    let index = ModelIndex::new(decoded.ir());
    let point = model_surface_point_by_id(
        &index,
        &SurfaceId("step:data:surface#6".into()),
        parameter_ranges[0][1] - parameter_ranges[0][0],
        1.0,
    )
    .expect("cyclic trimmed endpoint");
    assert!((point.x - 2.0 * 0.5_f64.cos()).abs() < 1.0e-12);
    assert!((point.y - 2.0 * 0.5_f64.sin()).abs() < 1.0e-12);
    assert!((point.z - 2.0).abs() < 1.0e-12);
}

#[test]
fn rectangular_trimmed_surface_keeps_topology_pcurves_in_local_uv_space() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#28=PLANE('',#27);",
            "#58=PLANE('',#27);\n#28=RECTANGULAR_TRIMMED_SURFACE('',#58,0.,10.,0.,10.,.T.,.T.);",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode trimmed sheet");
    let face = decoded.ir().model.faces.first().expect("trimmed face");
    assert_eq!(face.surface.as_str(), "step:data:surface#28");
    let construction = decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|surface| surface.surface == face.surface)
        .expect("trimmed face construction");
    assert!(matches!(
        &construction.definition,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Subset {
            support,
            parameter_ranges: [[0.0, 10.0], [0.0, 10.0]],
            u_sense: Some(true),
            v_sense: Some(true),
        } if support.as_str() == "step:data:surface#58"
    ));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
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

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| { loss.message.contains("skipped non-set member #99") }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn presentation_layer_expands_all_product_definition_views() {
    use cadmpeg_ir::presentation::PresentationItem;

    let result = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('P','Part','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('v1','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('design view','',#4,#5);
#7=PRODUCT_DEFINITION_FORMATION('v2','',#3);
#8=PRODUCT_DEFINITION('manufacturing view','',#7,#5);
#9=PRESENTATION_LAYER_ASSIGNMENT('definition views','',(#3));",
    );

    let layer = result
        .ir()
        .model
        .presentation_layers
        .first()
        .expect("product presentation layer");
    assert!(matches!(
        layer.items.as_slice(),
        [
            PresentationItem::Product { product: first },
            PresentationItem::Product { product: second },
        ] if first.as_str() == "step:product:product#3-definition-6"
            && second.as_str() == "step:product:product#3-definition-8"
    ));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
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

    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.product_definitions[0].bodies.len(), 1);
    assert_eq!(
        decoded.ir().model.product_definitions[0].bodies[0].as_str(),
        "step:data:body#31"
    );
    assert!(!decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| record.id.0 == "step:data:shell_based_surface_model#70"));
}

#[test]
fn topology_root_kind_preserves_distinct_body_kinds_for_shared_shells() {
    use cadmpeg_ir::topology::BodyKind;

    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#30=OPEN_SHELL('',(#29));", "#30=CLOSED_SHELL('',(#29));")
        .replace(
            "#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);",
            "#33=ORIENTED_CLOSED_SHELL('',*,#30,.F.);",
        )
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=SHELL_BASED_SURFACE_MODEL('',(#30));",
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#70=MANIFOLD_SOLID_BREP('',#30);\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode shared shell roots");

    assert_eq!(decoded.ir().model.bodies.len(), 2);
    assert!(decoded
        .ir()
        .model
        .bodies
        .iter()
        .any(|body| body.kind == BodyKind::Sheet));
    assert!(decoded
        .ir()
        .model
        .bodies
        .iter()
        .any(|body| body.kind == BodyKind::Solid));
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
        decoded.ir().model.bodies.len(),
        3,
        "{:#?}",
        decoded.report().losses
    );
    assert_eq!(
        decoded
            .ir()
            .model
            .shells
            .iter()
            .filter(|shell| shell.id.as_str().contains("root-70"))
            .count(),
        2
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn distinct_roots_with_shared_topology_get_owner_scopes() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#70=SHELL_BASED_SURFACE_MODEL('',(#71));\n#71=OPEN_SHELL('',(#29));\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode independent roots sharing topology");

    assert_eq!(
        decoded.ir().model.bodies.len(),
        2,
        "{:#?}",
        decoded.report().losses
    );
    assert!(decoded
        .ir()
        .model
        .edges
        .iter()
        .any(|edge| edge.id.as_str().contains("root-70")));
    assert!(decoded
        .ir()
        .model
        .edges
        .iter()
        .any(|edge| edge.id.as_str().contains("root-31")));
    assert!(decoded
        .ir()
        .model
        .vertices
        .iter()
        .any(|vertex| vertex.id.as_str().contains("root-70")));
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("conflicts with decoded topology")));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
pub(crate) fn reader_recovers_a_valid_solid_from_writer_output() {
    use cadmpeg_ir::topology::BodyKind;

    let source = unit_cube();
    let mut bytes = Vec::new();
    write_step(&source, &mut bytes, &StepWriteOptions::default()).unwrap();
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode generated cube STEP");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.bodies[0].kind, BodyKind::Solid);
    assert_eq!(result.ir().model.faces.len(), 6);
    assert_eq!(result.ir().model.edges.len(), 12);
    assert_eq!(result.ir().model.vertices.len(), 8);
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
pub(crate) fn step_color_assets_round_trip_names_and_tessellation_targets_strictly() {
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
            .into_parts()
            .0;
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
            .ir()
            .model
            .appearances
            .iter()
            .filter_map(|appearance| appearance.name.as_deref())
            .collect::<std::collections::BTreeSet<_>>();
        for expected in expected_names {
            assert!(names.contains(expected), "missing color name {expected}");
        }
        if expected_names == ["mesh green"] {
            assert!(decoded
                .ir()
                .model
                .appearance_bindings
                .iter()
                .any(|binding| {
                    matches!(
                        binding.target,
                        cadmpeg_ir::appearance::AppearanceTarget::Tessellation(_)
                    )
                }));
        }
    }
}

#[test]
fn mapped_presentation_does_not_report_body_placement_loss() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.));
#2=DIRECTION('',(1.,0.));
#3=AXIS2_PLACEMENT_2D('',#1,#2);
#4=REPRESENTATION_MAP(#3,#5);
#5=PRESENTATION_VIEW('',(),#6);
#6=REPRESENTATION_CONTEXT('','');
#7=CARTESIAN_POINT('',(10.,0.));
#8=AXIS2_PLACEMENT_2D('',#7,#2);
#9=MAPPED_ITEM('',#4,#8);",
    );
    assert!(!result.report().losses.iter().any(|loss| loss
        .message
        .contains("MAPPED_ITEM #9 has no resolved body placement")));
}

#[test]
fn presentation_layers_target_complex_tessellation_surface_sets() {
    let mut source = String::from_utf8(
        include_bytes!("../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#7=COMPLEX_TRIANGULATED_FACE('strip and fan',#6,4,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.)),$,(4,3,2,1),((1,2,3,4)),((1,2,4)));",
        "#7=COMPLEX_TRIANGULATED_SURFACE_SET('strip and fan',#6,4,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.)),(4,3,2,1),((1,2,3,4)),((1,2,4)));",
    );
    let end = source.rfind("ENDSEC;").expect("STEP data section end");
    source.insert_str(
        end,
        "#78=PRESENTATION_LAYER_ASSIGNMENT('mesh layer','',(#7));\n",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode layer target");

    let layer = decoded
        .ir()
        .model
        .presentation_layers
        .iter()
        .find(|layer| layer.name == "mesh layer")
        .expect("mesh presentation layer");
    assert!(layer.items.iter().any(|item| matches!(
        item,
        cadmpeg_ir::presentation::PresentationItem::Tessellation { tessellation }
            if tessellation.ends_with("#7")
    )));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);

    let mut output = Vec::new();
    let report = write_step(
        decoded.ir(),
        &mut output,
        &StepWriteOptions {
            schema: StepSchema::Ap242Edition3,
            ..StepWriteOptions::default()
        },
    )
    .expect("write tessellation layer");
    assert!(!report.losses.iter().any(|loss| {
        loss.message
            .contains("layer 'mesh layer' has 1 item(s) without a writable STEP carrier")
    }));
    let roundtrip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode tessellation layer");
    assert!(roundtrip
        .ir()
        .model
        .presentation_layers
        .iter()
        .any(|layer| {
            layer.name == "mesh layer"
                && layer.items.iter().any(|item| {
                    matches!(
                        item,
                        cadmpeg_ir::presentation::PresentationItem::Tessellation { .. }
                    )
                })
        }));
}

#[test]
fn reversed_step_ellipse_axes_are_canonicalized() {
    use cadmpeg_ir::geometry::CurveGeometry;

    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap242_geometry.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#10=ELLIPSE('',#6,6.,2.);", "#10=ELLIPSE('',#6,2.,6.);");
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(source.as_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode reversed ellipse");
    let ellipse = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#10")
        .expect("ellipse carrier");
    assert!(matches!(
        ellipse.geometry,
        CurveGeometry::Ellipse {
            major_radius,
            minor_radius,
            ..
        } if major_radius == 6.0 && minor_radius == 2.0
    ));
}

#[test]
fn reversed_step_ellipse_trim_preserves_source_parameterization() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=ELLIPSE('',#4,2.,6.);
#6=TRIMMED_CURVE('',#5,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(1.5707963267948966)),.T.,.PARAMETER.);
#7=GEOMETRIC_CURVE_SET('',(#6));
#8=SHAPE_REPRESENTATION('',(#7),$);",
    );
    let index = ModelIndex::new(result.ir());
    let start = model_curve_point_by_id(&index, &CurveId("step:data:curve#6".into()), 0.0)
        .expect("trimmed ellipse start");
    let end = model_curve_point_by_id(
        &index,
        &CurveId("step:data:curve#6".into()),
        std::f64::consts::FRAC_PI_2,
    )
    .expect("trimmed ellipse end");
    assert!((start.x - 2.0).abs() < 1.0e-12);
    assert!(start.y.abs() < 1.0e-12);
    assert!(end.x.abs() < 1.0e-12);
    assert!((end.y - 6.0).abs() < 1.0e-12);
    assert!(result.ir().model.procedural_curves.iter().any(|curve| {
        matches!(
            &curve.definition,
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                parameter_range: [start, end],
                ..
            } if curve.id.as_str() == "step:construction:trimmed_curve#6"
                && (*start + std::f64::consts::FRAC_PI_2).abs() < 1.0e-12
                && end.abs() < 1.0e-12
        )
    }));
}

#[test]
fn complex_presentation_annotation_inherits_text_and_placement() {
    use cadmpeg_ir::pmi::PmiDefinition;

    let source = String::from_utf8(
        include_bytes!("../tests/fixtures/ap242_presentation_pmi.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#7=TEXT_LITERAL('inspect surface',#6,'left',.RIGHT.,$);",
        "#7=(GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('') TEXT_LITERAL('inspect surface',#6,'left',.RIGHT.,$));",
    )
    .replace(
        "#8=ANNOTATION_TEXT_OCCURRENCE('surface note',(),#7);",
        "#8=(ANNOTATION_TEXT_OCCURRENCE() ANNOTATION_OCCURRENCE() DRAUGHTING_ANNOTATION_OCCURRENCE() GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('surface note') MAPPED_ITEM(#7,#7));",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex presentation PMI");

    assert_eq!(result.ir().model.pmi.len(), 1);
    let PmiDefinition::Presentation {
        ref text,
        ref placement,
        ..
    } = result.ir().model.pmi[0].definition
    else {
        panic!("complex annotation occurrence is not presentation PMI")
    };
    assert_eq!(
        result.ir().model.pmi[0].name.as_deref(),
        Some("surface note")
    );
    assert_eq!(text.as_deref(), Some("inspect surface"));
    let transform = placement.as_ref().expect("annotation placement");
    assert_eq!(transform.rows[0][3], 10.0);
    assert_eq!(transform.rows[1][3], 20.0);
    assert_eq!(transform.rows[2][3], 30.0);
}

#[test]
fn composite_presentation_text_does_not_depend_on_set_order() {
    use cadmpeg_ir::pmi::PmiDefinition;

    let result = decode_inline(
        "#1=TEXT_LITERAL('first',$,'left',.RIGHT.,$);
#2=TEXT_LITERAL('second',$,'left',.RIGHT.,$);
#3=COMPOSITE_TEXT('composite',(#1,#2));
#4=ANNOTATION_TEXT_OCCURRENCE('note',(),#3);",
    );

    let PmiDefinition::Presentation { ref text, .. } = result.ir().model.pmi[0].definition else {
        panic!("composite annotation is not presentation PMI")
    };
    assert!(text.is_none());
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::MetadataNotTransferred)
            && loss.message.contains("2 reachable text carriers")
    }));
    let unknowns = result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown records");
    for id in [1, 2, 3] {
        assert!(
            unknowns
                .iter()
                .any(|record| record.id.0.ends_with(&format!("#{id}"))),
            "ambiguous text carrier #{id} was not retained"
        );
    }
}

#[test]
fn presentation_graph_search_does_not_hide_unmodeled_tessellated_carriers() {
    let source = String::from_utf8(
        include_bytes!("../tests/fixtures/ap242_presentation_pmi.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#7=TEXT_LITERAL('inspect surface',#6,'left',.RIGHT.,$);",
        "#7=(GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('') TEXT_LITERAL('inspect surface',#6,'left',.RIGHT.,$));",
    )
    .replace(
        "#8=ANNOTATION_TEXT_OCCURRENCE('surface note',(),#7);",
        "#8=(TESSELLATED_GEOMETRIC_SET((#11)) ANNOTATION_TEXT_OCCURRENCE() ANNOTATION_OCCURRENCE() DRAUGHTING_ANNOTATION_OCCURRENCE() GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('surface note') MAPPED_ITEM(#7,#7));",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#11=(GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('tess carrier') TESSELLATED_GEOMETRIC_SET((#12)) TESSELLATED_ITEM());\n#12=TESSELLATED_CURVE_SET('tess curve',#13,((1,2)));\n#13=COORDINATES_LIST('',((0.,0.,0.),(1.,0.,0.)));\nENDSEC;\nEND-ISO-10303-21;",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode presentation graph with tessellated carrier");

    let unknowns = result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown records");
    for id in [11, 12, 13] {
        assert!(
            unknowns
                .iter()
                .any(|record| record.id.0.ends_with(&format!("#{id}"))),
            "tessellated carrier #{id} was consumed without a neutral representation"
        );
    }
}

#[test]
fn annotation_plane_keeps_its_neutral_plane_reachable() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            "#69=AXIS2_PLACEMENT_3D('',#3,#9,#10);\n#70=PLANE('',#69);\n#71=ANNOTATION_PLANE('',(),#70,());\nENDSEC;\nEND-ISO-10303-21;",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode annotation plane");
    let plane = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#70")
        .expect("annotation plane surface");
    assert_eq!(
        plane
            .source_object
            .as_ref()
            .map(|association| association.object_id.as_str()),
        Some("#71")
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(!validation.findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::CarrierReachability
            && finding.entity.as_deref() == Some("step:data:surface#70")
    }));
}

#[test]
fn styled_free_curve_is_a_reachable_source_carrier() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
         #2=CARTESIAN_POINT('',(1.,0.,0.));
         #7=POLYLINE('',(#1,#2));
         #10=STYLED_ITEM('',(),#7);",
    );
    let curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "step:data:curve#7")
        .expect("styled polyline carrier");
    assert_eq!(
        curve
            .source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        Some("#10")
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(!validation.findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::CarrierReachability
            && finding.entity.as_deref() == Some("step:data:curve#7")
    }));
}

#[test]
fn complex_tessellated_face_keeps_exact_support_surface_reachable() {
    let source = String::from_utf8(
        include_bytes!("../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#7=COMPLEX_TRIANGULATED_FACE('strip and fan',#6,4,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.)),$,(4,3,2,1),((1,2,3,4)),((1,2,4)));",
        "#7=COMPLEX_TRIANGULATED_FACE('strip and fan',#6,4,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.)),#79,(4,3,2,1),((1,2,3,4)),((1,2,4)));",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#79=PLANE('exact support',#34);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(source.as_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode complex tessellated support");
    let support = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.0 == "step:data:surface#79")
        .expect("exact support surface");
    assert_eq!(
        support
            .source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        Some("#7")
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(!validation.findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::CarrierReachability
            && finding.entity.as_deref() == Some("step:data:surface#79")
    }));
}

fn oriented_closed_shell_source(derived_slot: bool) -> String {
    let oriented_shell = if derived_slot {
        "#33=ORIENTED_CLOSED_SHELL('',*,#30,.F.);"
    } else {
        "#33=ORIENTED_CLOSED_SHELL('',#30,.F.);"
    };
    String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace("#30=OPEN_SHELL('',(#29));", "#30=CLOSED_SHELL('',(#29));")
        .replace("#33=ORIENTED_OPEN_SHELL('',*,#30,.F.);", oriented_shell)
        .replace(
            "#31=SHELL_BASED_SURFACE_MODEL('',(#33));",
            "#31=BREP_WITH_VOIDS('',#33,());",
        )
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
        .ir()
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

    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
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
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id.as_str() == "step:data:curve#5"));
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id.as_str() == "step:data:curve#7"));
    assert!(result.report().losses.iter().all(|loss| {
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
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.as_str() == "step:data:surface#6"));
    assert!(result
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.as_str() == "step:data:surface#7"));
    assert_eq!(result.ir().model.bodies.len(), 1);
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
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

    assert!(result.ir().model.surfaces.iter().any(|surface| {
        matches!(
            surface.geometry,
            cadmpeg_ir::geometry::SurfaceGeometry::Cone { half_angle, .. }
                if half_angle == 0.0
        )
    }));
    assert!(result.report().losses.iter().all(|loss| !loss
        .message
        .contains("CONICAL_SURFACE #5 has invalid geometry")));
}

#[test]
fn catia_cartesian_trim_points_resolve_on_nurbs_curve() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,1.,0.));
#3=CARTESIAN_POINT('',(2.,0.,0.));
#4=B_SPLINE_CURVE_WITH_KNOTS('',2,(#1,#2,#3),.UNSPECIFIED.,.U.,.U.,(3,3),(0.,2.),.UNSPECIFIED.);
#5=TRIMMED_CURVE('',#4,(#1),(#3),.T.,.CARTESIAN.);
#6=COMPOSITE_CURVE_SEGMENT(.DISCONTINUOUS.,.T.,#5);
#7=COMPOSITE_CURVE('',(#6),.U.);
#8=GEOMETRIC_SET('NONE',(#7));
#9=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#8),#10);
#10=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );

    assert_eq!(result.ir().model.curves.len(), 3);
    assert_eq!(result.ir().model.procedural_curves.len(), 1);
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::DecodeDiagnostic)
            && loss.message.contains("UNKNOWN periodicity")
            && loss.message.contains("#4")
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn defaulted_spline_curve_subtypes_derive_knot_vectors() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,1.,0.));
#3=CARTESIAN_POINT('',(2.,0.,0.));
#4=QUASI_UNIFORM_CURVE('quasi',2,(#1,#2,#3),.UNSPECIFIED.,.F.,.F.);
#5=UNIFORM_CURVE('uniform',1,(#1,#2,#3),.UNSPECIFIED.,.F.,.F.);
#6=BEZIER_CURVE('bezier',2,(#1,#2,#3),.UNSPECIFIED.,.F.,.F.);
#7=(BOUNDED_CURVE() B_SPLINE_CURVE(2,(#1,#2,#3),.UNSPECIFIED.,.F.,.F.) QUASI_UNIFORM_CURVE() RATIONAL_B_SPLINE_CURVE((1.,.5,1.)) CURVE() GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('rational'));
#8=GEOMETRIC_SET('',(#4,#5,#6,#7));
#9=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#8),#10);
#10=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );

    let nurbs = |id: &str| {
        result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == id)
            .and_then(|curve| match &curve.geometry {
                CurveGeometry::Nurbs(nurbs) => Some(nurbs),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing NURBS curve {id}"))
    };
    assert_eq!(
        nurbs("step:data:curve#4").knots,
        [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
    );
    assert_eq!(nurbs("step:data:curve#5").knots, [-1.0, 0.0, 1.0, 2.0, 3.0]);
    assert_eq!(
        nurbs("step:data:curve#6").knots,
        [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
    );
    let rational = nurbs("step:data:curve#7");
    assert_eq!(rational.knots, [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(rational.weights.as_deref(), Some(&[1.0, 0.5, 1.0][..]));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn defaulted_spline_surface_subtypes_derive_axis_knot_vectors() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,0.,0.));
#3=CARTESIAN_POINT('',(2.,0.,0.));
#4=CARTESIAN_POINT('',(0.,1.,0.));
#5=CARTESIAN_POINT('',(1.,1.,0.));
#6=CARTESIAN_POINT('',(2.,1.,0.));
#10=QUASI_UNIFORM_SURFACE('quasi',1,1,((#1,#2,#3),(#4,#5,#6)),.UNSPECIFIED.,.F.,.F.,.F.);
#11=UNIFORM_SURFACE('uniform',1,2,((#1,#2,#3),(#4,#5,#6)),.UNSPECIFIED.,.F.,.F.,.F.);
#12=BEZIER_SURFACE('bezier',1,2,((#1,#2,#3),(#4,#5,#6)),.UNSPECIFIED.,.F.,.F.,.F.);
#13=GEOMETRIC_SET('',(#10,#11,#12));
#14=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#13),#15);
#15=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );

    let nurbs = |id: &str| {
        result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == id)
            .and_then(|surface| match &surface.geometry {
                SurfaceGeometry::Nurbs(nurbs) => Some(nurbs),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing NURBS surface {id}"))
    };
    assert_eq!(nurbs("step:data:surface#10").u_knots, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(
        nurbs("step:data:surface#10").v_knots,
        [0.0, 0.0, 1.0, 2.0, 2.0]
    );
    assert_eq!(nurbs("step:data:surface#11").u_knots, [-1.0, 0.0, 1.0, 2.0]);
    assert_eq!(
        nurbs("step:data:surface#11").v_knots,
        [-2.0, -1.0, 0.0, 1.0, 2.0, 3.0]
    );
    assert_eq!(nurbs("step:data:surface#12").u_knots, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(
        nurbs("step:data:surface#12").v_knots,
        [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_rational_quasi_uniform_surface_decodes_with_weight_grid() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,0.,0.));
#3=CARTESIAN_POINT('',(0.,1.,0.));
#4=CARTESIAN_POINT('',(1.,1.,0.));
#5=CARTESIAN_POINT('',(0.,2.,0.));
#6=CARTESIAN_POINT('',(1.,2.,0.));
#7=(BOUNDED_SURFACE() B_SPLINE_SURFACE(2,1,((#1,#2),(#3,#4),(#5,#6)),.UNSPECIFIED.,.F.,.F.,.F.) QUASI_UNIFORM_SURFACE() RATIONAL_B_SPLINE_SURFACE(((1.,.5),(1.,.5),(1.,1.))) SURFACE());
#8=GEOMETRIC_SET('',(#7));
#9=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#8),#10);
#10=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#7")
        .expect("complex rational surface");
    let SurfaceGeometry::Nurbs(nurbs) = &surface.geometry else {
        panic!("complex rational surface is not NURBS")
    };
    assert_eq!(nurbs.u_knots, [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(nurbs.v_knots, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(
        nurbs.weights.as_deref(),
        Some(&[1.0, 0.5, 1.0, 0.5, 1.0, 1.0][..])
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn quasi_uniform_pcurve_is_decoded_from_its_2d_representation() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#51=CARTESIAN_POINT('',(0.,0.));\n#52=DIRECTION('',(1.,0.));",
            "#51=CARTESIAN_POINT('',(0.,0.));\n#52=DIRECTION('',(1.,0.));\n#58=CARTESIAN_POINT('',(10.,0.));",
        )
        .replace(
            "#54=LINE('',#51,#53);",
            "#54=QUASI_UNIFORM_CURVE('',1,(#51,#58),.UNSPECIFIED.,.F.,.F.);",
        )
        .replace(
            "#55=DEFINITIONAL_REPRESENTATION('',(#54),#50);",
            "#55=(DEFINITIONAL_REPRESENTATION()REPRESENTATION('',(#54),#50)SHAPE_REPRESENTATION());",
        );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode quasi-uniform pcurve");

    assert!(result.ir().model.pcurves.iter().any(|pcurve| {
        matches!(
            &pcurve.geometry,
            cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
                degree: 1,
                knots,
                control_points,
                weights: None,
                periodic: false,
            } if knots == &[0.0, 0.0, 1.0, 1.0] && control_points.len() == 2
        )
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn excessive_nurbs_degree_is_rejected_before_knot_allocation() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,0.,0.));
#3=B_SPLINE_CURVE_WITH_KNOTS('',4294967295,(#1,#2),.UNSPECIFIED.,.F.,.F.,(4294967298),(0.),.UNSPECIFIED.);",
    );
    assert!(result.ir().model.curves.is_empty());
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
    assert_eq!(result.ir().model.product_definitions.len(), 1);
    assert_eq!(result.ir().model.product_definitions[0].bodies.len(), 1);
    assert_eq!(
        result.ir().model.product_definitions[0].bodies[0].as_str(),
        "step:data:body#9000"
    );
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
    assert_eq!(result.ir().model.appearance_bindings.len(), 1);
    let binding = &result.ir().model.appearance_bindings[0];
    let appearance = result
        .ir()
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.id == binding.appearance)
        .expect("overriding appearance");
    let color = appearance.base_color.expect("override color");
    assert_eq!((color.r, color.g, color.b), (1.0, 0.0, 0.0));
}

#[test]
fn independent_face_styles_keep_bindings_without_source_order_scalar_color() {
    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#68=STYLED_ITEM('',(#66),#19);",
            "#68=STYLED_ITEM('',(#66),#19);\n#69=COLOUR_RGB('independent blue',0.,0.,1.);\n#70=FILL_AREA_STYLE_COLOUR('',#69);\n#71=FILL_AREA_STYLE('',(#70));\n#72=SURFACE_STYLE_FILL_AREA(#71);\n#73=SURFACE_SIDE_STYLE('',(#72));\n#74=SURFACE_STYLE_USAGE(.BOTH.,#73);\n#75=PRESENTATION_STYLE_ASSIGNMENT((#74));\n#76=STYLED_ITEM('',(#75),#29);",
        );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode independent face styles");

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == "step:data:face#29")
        .expect("styled face");
    assert!(face.color.is_none());
    assert_eq!(
        result
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| {
                matches!(
                    &binding.target,
                    cadmpeg_ir::appearance::AppearanceTarget::Face(face)
                        if face.as_str() == "step:data:face#29"
                )
            })
            .count(),
        2
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::MetadataNotTransferred)
            && loss.message.contains("#47")
            && loss.message.contains("#76")
            && loss.message.contains("scalar color omitted")
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn presentation_records_retain_non_color_geometry_owners() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,0.,0.));
#3=POLYLINE('styled curve',(#1,#2));
#4=CURVE_STYLE('line style',$,$,$);
#5=PRESENTATION_STYLE_ASSIGNMENT((#4));
#6=STYLED_ITEM('',(#5),#3);
#7=DIRECTION('',(0.,0.,1.));
#8=AXIS2_PLACEMENT_3D('',#1,#7,$);
#9=PLANE('annotation support',#8);
#10=ANNOTATION_PLANE('annotation plane',(),#9,());",
    );

    let curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "step:data:curve#3")
        .expect("styled curve");
    assert_eq!(
        curve
            .source_object
            .as_ref()
            .expect("styled curve owner")
            .object_id,
        "#6"
    );
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.0 == "step:data:surface#9")
        .expect("annotation support surface");
    assert_eq!(
        surface
            .source_object
            .as_ref()
            .expect("annotation plane owner")
            .object_id,
        "#10"
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_styled_item_decodes_color_and_owns_its_curve() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,0.,0.));
#3=POLYLINE('styled curve',(#1,#2));
#4=COLOUR_RGB('red',1.,0.,0.);
#5=PRESENTATION_STYLE_ASSIGNMENT((#4));
#6=(ANNOTATION_CURVE_OCCURRENCE() STYLED_ITEM((#5),#3));
#7=COLOUR_RGB('blue',0.,0.,1.);
#8=PRESENTATION_STYLE_ASSIGNMENT((#7));
#9=(ANNOTATION_CURVE_OCCURRENCE() OVER_RIDING_STYLED_ITEM((#8),#3,#6));",
    );

    let curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "step:data:curve#3")
        .expect("complex styled curve");
    assert_eq!(
        curve
            .source_object
            .as_ref()
            .expect("complex styled curve owner")
            .object_id,
        "#6"
    );
    assert!(result.ir().model.appearance_bindings.iter().any(|binding| {
        matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Curve(ref curve)
                if curve.as_str() == "step:data:curve#3"
        )
    }));
    assert_eq!(result.ir().model.appearance_bindings.len(), 1);
    let appearance = result
        .ir()
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.id == result.ir().model.appearance_bindings[0].appearance)
        .expect("overriding appearance");
    assert_eq!(appearance.name.as_deref(), Some("blue"));
    assert!(result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .all(|record| record.id.0 != "step:data:styled_item#6"));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_colour_rgb_inherits_name_and_components() {
    let result = decode_inline(
        "#1=(COLOUR_RGB(1.,0.,0.) COLOUR_SPECIFICATION('red') COLOUR());
#2=PRESENTATION_STYLE_ASSIGNMENT((#1));
#3=STYLED_ITEM('',(#2),#4);
#4=SOURCE_ITEM();",
    );
    assert_eq!(result.ir().model.appearance_bindings.len(), 1);
    let appearance = result
        .ir()
        .model
        .appearances
        .first()
        .expect("complex RGB appearance");
    assert_eq!(appearance.name.as_deref(), Some("red"));
    assert_eq!(appearance.base_color.unwrap().r, 1.0);
}

#[test]
fn complex_surface_targets_use_surface_style_domain() {
    let result = decode_inline(
        "#1=COLOUR_RGB('red',1.,0.,0.);
#2=COLOUR_RGB('blue',0.,0.,1.);
#3=CURVE_STYLE('',#1,$,$);
#4=SURFACE_STYLE_RENDERING(#2,$,$,$,$,$);
#5=PRESENTATION_STYLE_ASSIGNMENT((#3,#4));
#6=STYLED_ITEM('',(#5),#7);
#7=(ADVANCED_FACE() FACE_SURFACE());",
    );
    assert_eq!(result.ir().model.appearances.len(), 1);
    assert_eq!(
        result.ir().model.appearances[0].base_color,
        Some(cadmpeg_ir::topology::Color {
            r: 0.0,
            g: 0.0,
            b: 1.0,
            a: 1.0,
        })
    );
}

#[test]
fn surface_style_usage_prefers_positive_side_over_set_order() {
    let result = decode_inline(
        "#1=COLOUR_RGB('negative',1.,0.,0.);
#2=SURFACE_STYLE_RENDERING(#1,$,$,$,$,$);
#3=SURFACE_SIDE_STYLE('',(#2));
#4=SURFACE_STYLE_USAGE(.NEGATIVE.,#3);
#5=COLOUR_RGB('positive',0.,1.,0.);
#6=SURFACE_STYLE_RENDERING(#5,$,$,$,$,$);
#7=SURFACE_SIDE_STYLE('',(#6));
#8=SURFACE_STYLE_USAGE(.POSITIVE.,#7);
#9=PRESENTATION_STYLE_ASSIGNMENT((#4,#8));
#10=STYLED_ITEM('',(#9),#11);
#11=(ADVANCED_FACE() FACE_SURFACE());",
    );
    assert_eq!(result.ir().model.appearances.len(), 1);
    assert_eq!(
        result.ir().model.appearances[0].base_color,
        Some(cadmpeg_ir::topology::Color {
            r: 0.0,
            g: 1.0,
            b: 0.0,
            a: 1.0,
        })
    );
}

#[test]
fn curve_targets_use_curve_style_domain() {
    let result = decode_inline(
        "#1=COLOUR_RGB('surface',1.,0.,0.);
#2=SURFACE_STYLE_RENDERING(#1,$,$,$,$,$);
#3=COLOUR_RGB('curve',0.,1.,0.);
#4=CURVE_STYLE('',#3,$,$);
#5=PRESENTATION_STYLE_ASSIGNMENT((#2,#4));
#6=STYLED_ITEM('',(#5),#7);
#7=POLYLINE('styled curve',());",
    );
    assert_eq!(result.ir().model.appearances.len(), 1);
    assert_eq!(
        result.ir().model.appearances[0].base_color,
        Some(cadmpeg_ir::topology::Color {
            r: 0.0,
            g: 1.0,
            b: 0.0,
            a: 1.0,
        })
    );
}

#[test]
fn point_targets_use_point_style_domain() {
    let result = decode_inline(
        "#1=COLOUR_RGB('surface',1.,0.,0.);
#2=SURFACE_STYLE_RENDERING(#1,$,$,$,$,$);
#3=COLOUR_RGB('point',0.,1.,0.);
#4=POINT_STYLE('',.DOT.,POSITIVE_LENGTH_MEASURE(1.),#3);
#5=PRESENTATION_STYLE_ASSIGNMENT((#2,#4));
#6=STYLED_ITEM('',(#5),#7);
#7=CARTESIAN_POINT('styled point',(0.,0.,0.));",
    );
    assert_eq!(result.ir().model.appearances.len(), 1);
    assert_eq!(
        result.ir().model.appearances[0].base_color,
        Some(cadmpeg_ir::topology::Color {
            r: 0.0,
            g: 1.0,
            b: 0.0,
            a: 1.0,
        })
    );
}

#[test]
fn null_style_branch_does_not_suppress_a_sibling_color() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=COLOUR_RGB('red',1.,0.,0.);
#3=PRESENTATION_STYLE_ASSIGNMENT((NULL_STYLE(.NULL.),#2));
#4=STYLED_ITEM('',(#3),#1);",
    );
    assert_eq!(result.ir().model.appearance_bindings.len(), 1);
}

#[test]
fn complex_null_style_inherited_partial_suppresses_false_color_warning() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=(PRESENTATION_STYLE_ASSIGNMENT(()) STYLE_ASSIGNMENT((NULL_STYLE(.NULL.))));
#3=STYLED_ITEM('',(#2),#1);",
    );
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("STYLED_ITEM #3 has no resolved surface color")
    }));
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
        .ir()
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
        .ir()
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
    assert!(result.report().losses.iter().all(|loss| {
        !loss
            .message
            .contains("LINE #14 parameter scale did not resolve")
    }));
}

#[test]
fn omitted_placement_reference_uses_the_first_projected_axis() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.6,0.8,0.));
#3=AXIS2_PLACEMENT_3D('',#1,#2,$);
#4=CIRCLE('',#3,2.);
#5=GEOMETRIC_CURVE_SET('',(#4));
#6=SHAPE_REPRESENTATION('',(#5),$);",
    );
    let circle = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#4")
        .expect("circle");
    let CurveGeometry::Circle { ref_direction, .. } = circle.geometry else {
        panic!("decoded carrier is not a circle");
    };
    assert!((ref_direction.x - 0.8).abs() < 1.0e-12);
    assert!((ref_direction.y + 0.6).abs() < 1.0e-12);
    assert!(ref_direction.z.abs() < 1.0e-12);
}

#[test]
fn near_parallel_omitted_reference_uses_a_stable_projected_axis() {
    let result = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));
#3=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#2)) REPRESENTATION_CONTEXT('model','3D'));
#10=CARTESIAN_POINT('',(0.,0.,0.));
#11=DIRECTION('',(-1.,0.0000000612905015206,0.0000000692801624183));
#12=AXIS2_PLACEMENT_3D('',#10,#11,$);
#13=CIRCLE('',#12,2.);
#14=GEOMETRIC_CURVE_SET('',(#13));
#15=SHAPE_REPRESENTATION('',(#14),#3);",
    );
    let circle = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "step:data:curve#13")
        .expect("circle");
    let CurveGeometry::Circle {
        axis,
        ref_direction,
        ..
    } = circle.geometry
    else {
        panic!("decoded carrier is not a circle");
    };
    let dot = axis.x * ref_direction.x + axis.y * ref_direction.y + axis.z * ref_direction.z;
    assert!(ref_direction.y > 0.999_999_999);
    assert!(dot.abs() < 1.0e-12);
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn parallel_axis_reference_direction_is_reported_and_inferred() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(0.,0.,2.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=CIRCLE('',#4,2.);
#6=GEOMETRIC_CURVE_SET('',(#5));
#7=SHAPE_REPRESENTATION('',(#6),$);",
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::CarrierAxisInferred)
            && loss.message.contains("AXIS2_PLACEMENT_3D #4")
    }));
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
        .ir()
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
fn trimmed_curve_replica_keeps_parent_parameterization_for_both_selectors() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=DIRECTION('',(0.,1.,0.));
#4=DIRECTION('',(0.,0.,1.));
#5=VECTOR('',#2,2.);
#6=LINE('',#1,#5);
#7=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#2,#3,#1,3.,#4);
#8=CURVE_REPLICA('',#6,#7);
#9=TRIMMED_CURVE('',#8,(PARAMETER_VALUE(1.)),(PARAMETER_VALUE(2.)),.T.,.PARAMETER.);
#10=CARTESIAN_POINT('',(6.,0.,0.));
#11=CARTESIAN_POINT('',(12.,0.,0.));
#12=TRIMMED_CURVE('',#8,(#10),(#11),.T.,.CARTESIAN.);
#13=GEOMETRIC_CURVE_SET('',(#9,#12));
#14=SHAPE_REPRESENTATION('',(#13),$);",
    );

    for (curve_id, expected) in [("#9", [2.0, 4.0]), ("#12", [2.0, 4.0])] {
        let construction_id =
            StepIdentity::construction("trimmed_curve", curve_id.trim_start_matches('#'));
        assert!(result.ir().model.procedural_curves.iter().any(|curve| {
            curve.id.as_str() == construction_id
                && matches!(
                    curve.definition,
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                        parameter_range,
                        ..
                    } if parameter_range == expected
                )
        }));
    }

    assert!(result.ir().model.procedural_curves.iter().any(|curve| {
        matches!(
            &curve.definition,
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Replica { source, .. }
                if curve.curve.as_str() == "step:data:curve#8"
                    && source.as_str() == "step:data:curve#6"
        )
    }));
    let index = ModelIndex::new(result.ir());
    assert_eq!(
        model_curve_point_by_id(&index, &CurveId("step:data:curve#9".into()), 0.0,),
        Some(Point3::new(6.0, 0.0, 0.0))
    );
    assert_eq!(
        model_curve_point_by_id(&index, &CurveId("step:data:curve#9".into()), 2.0,),
        Some(Point3::new(12.0, 0.0, 0.0))
    );

    let mut output = Vec::new();
    write_step(result.ir(), &mut output, &StepWriteOptions::default())
        .expect("write trimmed replica");
    let text = String::from_utf8(output.clone()).expect("STEP output is UTF-8");
    assert!(text.contains("CURVE_REPLICA"));
    assert!(text.contains("TRIMMED_CURVE"));
    let round_trip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode trimmed replica");
    assert!(round_trip.ir().model.procedural_curves.iter().any(|curve| {
        matches!(
            &curve.definition,
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Replica { source, .. }
                if source.as_str().starts_with("step:data:curve#")
        )
    }));
}

#[test]
fn trimmed_curve_prefers_the_parameter_value_under_parameter_master() {
    let result = decode_inline(
        "#20=CARTESIAN_POINT('',(0.,0.,0.));
#21=DIRECTION('',(0.,0.,1.));
#22=DIRECTION('',(1.,0.,0.));
#23=AXIS2_PLACEMENT_3D('',#20,#21,#22);
#24=CIRCLE('',#23,1.);
#30=CARTESIAN_POINT('',(1.,0.,0.));
#31=CARTESIAN_POINT('',(0.,-1.,0.));
#40=TRIMMED_CURVE('',#24,(#30,PARAMETER_VALUE(0.)), (#31,PARAMETER_VALUE(4.712388980)),.T.,.PARAMETER.);
#41=GEOMETRIC_CURVE_SET('',(#40));
#42=SHAPE_REPRESENTATION('',(#41),$);",
    );
    let parameter_range = result
        .ir()
        .model
        .procedural_curves
        .iter()
        .find_map(|curve| match &curve.definition {
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                parameter_range, ..
            } if curve.id.as_str() == "step:construction:trimmed_curve#40" => {
                Some(*parameter_range)
            }
            _ => None,
        })
        .expect("parameter-master trimmed curve");
    assert_eq!(parameter_range[0], 0.0);
    assert!((parameter_range[1] - 3.0 * std::f64::consts::PI / 2.0).abs() < 1.0e-9);
    assert!(result.report().losses.iter().all(|loss| {
        !loss
            .message
            .contains("fell back to a Cartesian trim selector")
    }));
}

#[test]
fn trimmed_curve_prefers_the_point_under_cartesian_master() {
    let result = decode_inline(
        "#20=CARTESIAN_POINT('',(0.,0.,0.));
#21=DIRECTION('',(0.,0.,1.));
#22=DIRECTION('',(1.,0.,0.));
#23=AXIS2_PLACEMENT_3D('',#20,#21,#22);
#24=CIRCLE('',#23,1.);
#30=CARTESIAN_POINT('',(1.,0.,0.));
#31=CARTESIAN_POINT('',(0.,-1.,0.));
#40=TRIMMED_CURVE('',#24,(#30,PARAMETER_VALUE(0.)), (#31,PARAMETER_VALUE(4.712388980)),.T.,.CARTESIAN.);
#41=GEOMETRIC_CURVE_SET('',(#40));
#42=SHAPE_REPRESENTATION('',(#41),$);",
    );
    let parameter_range = result
        .ir()
        .model
        .procedural_curves
        .iter()
        .find_map(|curve| match &curve.definition {
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                parameter_range, ..
            } if curve.id.as_str() == "step:construction:trimmed_curve#40" => {
                Some(*parameter_range)
            }
            _ => None,
        })
        .expect("Cartesian-master trimmed curve");
    assert_eq!(parameter_range[0], 0.0);
    assert!((parameter_range[1] - 3.0 * std::f64::consts::PI / 2.0).abs() < 1.0e-12);
    assert!(result.report().losses.iter().all(|loss| {
        !loss
            .message
            .contains("fell back to a parameter trim selector")
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn trimmed_curve_opposed_sense_retains_the_periodic_branch() {
    let result = decode_inline(
        "#20=CARTESIAN_POINT('',(0.,0.,0.));
#21=DIRECTION('',(0.,0.,1.));
#22=DIRECTION('',(1.,0.,0.));
#23=AXIS2_PLACEMENT_3D('',#20,#21,#22);
#24=CIRCLE('',#23,1.);
#40=TRIMMED_CURVE('',#24,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(1.5707963267948966)),.F.,.PARAMETER.);
#41=GEOMETRIC_CURVE_SET('',(#40));
#42=SHAPE_REPRESENTATION('',(#41),$);",
    );
    let parameter_range = result
        .ir()
        .model
        .procedural_curves
        .iter()
        .find_map(|curve| match &curve.definition {
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                parameter_range, ..
            } if curve.id.as_str() == "step:construction:trimmed_curve#40" => {
                Some(*parameter_range)
            }
            _ => None,
        })
        .expect("opposed-sense trimmed curve");
    assert!((parameter_range[0] - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12);
    assert!((parameter_range[1] - std::f64::consts::TAU).abs() < 1.0e-12);
    assert!(result.ir().model.procedural_curves.iter().any(|curve| {
        matches!(
            &curve.definition,
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset { sense, .. }
                if curve.id.as_str() == "step:construction:trimmed_curve#40" && !sense
        )
    }));
    let mut output = Vec::new();
    write_step(result.ir(), &mut output, &StepWriteOptions::default())
        .expect("write opposed-sense trimmed curve");
    let text = String::from_utf8(output).expect("STEP output is UTF-8");
    assert!(text.contains(".F.,.PARAMETER."));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn trimmed_curve_forward_sense_wraps_a_closed_basis() {
    let result = decode_inline(
        "#20=CARTESIAN_POINT('',(0.,0.,0.));
#21=DIRECTION('',(0.,0.,1.));
#22=DIRECTION('',(1.,0.,0.));
#23=AXIS2_PLACEMENT_3D('',#20,#21,#22);
#24=CIRCLE('',#23,1.);
#40=TRIMMED_CURVE('',#24,(PARAMETER_VALUE(5.)),(PARAMETER_VALUE(1.)),.T.,.PARAMETER.);
#41=GEOMETRIC_CURVE_SET('',(#40));
#42=SHAPE_REPRESENTATION('',(#41),$);",
    );
    let parameter_range = result
        .ir()
        .model
        .procedural_curves
        .iter()
        .find_map(|curve| match &curve.definition {
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                parameter_range, ..
            } if curve.id.as_str() == "step:construction:trimmed_curve#40" => {
                Some(*parameter_range)
            }
            _ => None,
        })
        .expect("forward trimmed curve");
    assert!((parameter_range[0] - 5.0).abs() < 1.0e-12);
    assert!((parameter_range[1] - (1.0 + std::f64::consts::TAU)).abs() < 1.0e-12);
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn trimmed_curve_reports_a_fallback_when_the_preferred_form_is_absent() {
    let result = decode_inline(
        "#20=CARTESIAN_POINT('',(0.,0.,0.));
#21=DIRECTION('',(0.,0.,1.));
#22=DIRECTION('',(1.,0.,0.));
#23=AXIS2_PLACEMENT_3D('',#20,#21,#22);
#24=CIRCLE('',#23,1.);
#30=CARTESIAN_POINT('',(1.,0.,0.));
#31=CARTESIAN_POINT('',(0.,-1.,0.));
#40=TRIMMED_CURVE('',#24,(#30), (#31,PARAMETER_VALUE(4.712388980)),.T.,.PARAMETER.);
#41=GEOMETRIC_CURVE_SET('',(#40));
#42=SHAPE_REPRESENTATION('',(#41),$);",
    );
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| {
                loss.code
                    == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::DecodeDiagnostic)
                    && loss.message.contains("TRIMMED_CURVE #40")
                    && loss.message.contains("Cartesian trim selector")
            })
            .count(),
        1
    );
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
fn advanced_brep_representation_reuses_its_committed_solid_body() {
    let source = export(&unit_cube()).replace(
        "ADVANCED_BREP_SHAPE_REPRESENTATION",
        "ADVANCED_BREP_REPRESENTATION",
    );
    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode advanced B-rep representation");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert!(!result
        .ir()
        .native_unknowns("step")
        .unwrap()
        .iter()
        .any(|record| record.id.0.contains("advanced_brep_representation")));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn advanced_brep_mapped_representation_reuses_its_committed_solid_body() {
    let mut source = export(&unit_cube()).replace(
        "ADVANCED_BREP_SHAPE_REPRESENTATION",
        "ADVANCED_BREP_REPRESENTATION",
    );
    let representation_line = source
        .lines()
        .find(|line| line.contains("ADVANCED_BREP_REPRESENTATION("))
        .expect("written advanced B-rep representation");
    let representation = representation_line
        .split_once('=')
        .and_then(|(id, _)| id.trim().strip_prefix('#'))
        .and_then(|id| id.parse::<u64>().ok())
        .expect("advanced B-rep representation id");
    let context = representation_line
        .split_once('=')
        .and_then(|(_, record)| record.strip_suffix(';'))
        .and_then(|record| record.strip_suffix(')'))
        .and_then(|record| record.rsplit_once(','))
        .map(|(_, context)| context.trim())
        .expect("advanced B-rep representation context");
    let next_id = source
        .lines()
        .filter_map(|line| {
            line.strip_prefix('#')
                .and_then(|line| line.split_once('='))
                .and_then(|(id, _)| id.trim().parse::<u64>().ok())
        })
        .max()
        .expect("written STEP entity")
        + 1;
    let map = next_id;
    let mapped_item = next_id + 1;
    let mapped_representation = next_id + 2;
    let records = format!(
        "#{map}=REPRESENTATION_MAP($,#{representation});\n\
#{mapped_item}=MAPPED_ITEM('',#{map},$);\n\
#{mapped_representation}=(ADVANCED_BREP_REPRESENTATION() REPRESENTATION('mapped',(#{mapped_item}),{context}) REPRESENTATION_ITEM('mapped'));\n"
    );
    let end = source.rfind("ENDSEC;").expect("STEP data section end");
    source.insert_str(end, &records);

    let result = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode mapped advanced B-rep representation");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("ADVANCED_BREP_REPRESENTATION instance(s) as named opaque STEP records")
    }));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
pub(crate) fn face_outer_bound_is_canonicalized_ahead_of_inner_bounds() {
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
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == StepIdentity::data("face", face_step))
        .expect("decoded face");
    assert_eq!(
        face.loops[0].as_str(),
        StepIdentity::data("loop", format!("{outer_loop}-face-{face_step}"))
    );
}

#[test]
fn duplicate_face_outer_bounds_are_reported_without_inventing_inner_roles() {
    use cadmpeg_ir::ids::LoopId;
    use cadmpeg_ir::topology::Loop;

    let mut ir = unit_cube();
    let face = ir.model.faces[0].id.clone();
    let duplicate = LoopId("synthetic:test:loop#duplicate-outer".into());
    ir.model.loops.push(Loop {
        id: duplicate.clone(),
        face: face.clone(),
        boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Outer,
        coedges: Vec::new(),
        vertex_uses: vec![cadmpeg_ir::topology::VertexUse {
            vertex: ir.model.vertices[0].id.clone(),
            after: None,
            pcurves: Vec::new(),
        }],
    });
    ir.model.faces[0].loops.push(duplicate);

    let output = export(&ir);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode duplicate outer bounds");
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == cadmpeg_ir::LossKind::shared(cadmpeg_ir::LossTaxonomy::SourceTopologyInvalid)
            && loss.message.contains("violates the STEP face-bound rule")
            && loss
                .message
                .contains("marking the remaining 1 roles unspecified")
    }));
    let face = &decoded.ir().model.faces[0];
    let roles = face
        .loops
        .iter()
        .map(|id| {
            decoded
                .ir()
                .model
                .loops
                .iter()
                .find(|loop_| loop_.id == *id)
                .expect("decoded face loop")
                .boundary_role
        })
        .collect::<Vec<_>>();
    assert_eq!(
        roles
            .iter()
            .filter(|role| **role == cadmpeg_ir::topology::LoopBoundaryRole::Outer)
            .count(),
        1
    );
    assert_eq!(
        roles
            .iter()
            .filter(|role| **role == cadmpeg_ir::topology::LoopBoundaryRole::Unspecified)
            .count(),
        1
    );
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
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
pub(crate) fn every_region_of_a_body_is_retained_as_a_shape_item() {
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
fn body_layers_and_visibility_cover_every_region_shape_item() {
    use cadmpeg_ir::ids::LayerId;
    use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};

    let mut ir = unit_cube();
    let body = ir.model.bodies[0].id.clone();
    let mut region = ir.model.regions[0].clone();
    region.id.0 = "zzzz:test:region#second".into();
    ir.model.bodies[0].regions.push(region.id.clone());
    ir.model.regions.push(region);
    ir.model.bodies[0].visible = Some(false);
    ir.model.presentation_layers.push(PresentationLayer {
        id: LayerId("test:layer#body".into()),
        name: "all body regions".into(),
        description: None,
        items: vec![PresentationItem::Body { body }],
    });

    let mut bytes = Vec::new();
    write_step(&ir, &mut bytes, &StepWriteOptions::default()).expect("write body presentation");
    let (exchange, diagnostics) = crate::parse::parse(&bytes).expect("parse body presentation");
    assert!(diagnostics.is_empty());
    let layer = exchange
        .records
        .values()
        .find(|record| {
            record
                .partials
                .iter()
                .any(|partial| partial.name == "PRESENTATION_LAYER_ASSIGNMENT")
        })
        .expect("body presentation layer");
    let layer_partial = layer
        .partials
        .iter()
        .find(|partial| partial.name == "PRESENTATION_LAYER_ASSIGNMENT")
        .unwrap();
    let crate::parse::Value::List(layer_items) = &layer_partial.parameters[2] else {
        panic!("layer items are not an aggregate")
    };
    assert_eq!(layer_items.len(), 2);
    let visibility = exchange
        .records
        .values()
        .find(|record| {
            record
                .partials
                .iter()
                .any(|partial| partial.name == "INVISIBILITY")
        })
        .expect("body invisibility");
    let visibility_partial = visibility
        .partials
        .iter()
        .find(|partial| partial.name == "INVISIBILITY")
        .unwrap();
    let crate::parse::Value::List(hidden_items) = &visibility_partial.parameters[0] else {
        panic!("visibility items are not an aggregate")
    };
    assert_eq!(hidden_items.len(), 2);
}

#[test]
pub(crate) fn presentation_reader_normalizes_invalid_layer_and_common_datum_inputs() {
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
    assert_eq!(result.ir().model.presentation_layers.len(), 1);
    assert!(matches!(
        result.ir().model.presentation_layers[0].items.as_slice(),
        [PresentationItem::Source { source_id }] if source_id == "#30"
    ));
    assert!(result.ir().model.pmi.iter().any(|annotation| matches!(
        &annotation.definition,
        PmiDefinition::DatumSystem { references }
            if references.len() == 1 && references[0].common_group.is_none()
    )));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn presentation_reader_resolves_complex_datum_reference_inheritance() {
    use cadmpeg_ir::pmi::PmiDefinition;

    let result = decode_inline(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#5=PRODUCT_DEFINITION_SHAPE('PMI shape','',#99);
#7=DATUM('',$,#5,.F.,'A');
#8=DATUM_SYSTEM('system','',#5,.F.,(#20));
#20=(DATUM_REFERENCE_COMPARTMENT() GENERAL_DATUM_REFERENCE(COMMON_DATUM_LIST((#21)),(#22)) SHAPE_ASPECT('','',#5,.F.));
#21=(DATUM_REFERENCE_ELEMENT() GENERAL_DATUM_REFERENCE(#7,()) SHAPE_ASPECT('','',#5,.F.));
#22=DATUM_REFERENCE_MODIFIER_WITH_VALUE(.DISTANCE.,#23);
#23=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.2),#1);
#99=UNRESOLVED_PRODUCT();",
    );
    let system = result
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("system"))
        .expect("complex datum system");
    assert!(matches!(
        &system.definition,
        PmiDefinition::DatumSystem { references }
            if references.len() == 1
                && references[0].datum.as_str() == "step:presentation:pmi#7"
                && references[0].common_group.is_none()
                && references[0].modifiers == ["distance:0.2"]
    ));
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| { !loss.message.contains("DATUM_REFERENCE_COMPARTMENT #20") }));
}

#[test]
fn transformed_curves_and_surfaces_round_trip_through_step_replicas() {
    let transform = Transform {
        rows: [
            [0.0, -2.0, 0.0, 10.0],
            [2.0, 0.0, 0.0, 20.0],
            [0.0, 0.0, 2.0, 30.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };
    let curve_geometry = CurveGeometry::Transformed {
        basis: Box::new(CurveGeometry::Line {
            origin: Point3::new(1.0, 2.0, 3.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        }),
        transform,
    };
    let surface_geometry = SurfaceGeometry::Transformed {
        basis: Box::new(SurfaceGeometry::Plane {
            origin: Point3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        }),
        transform,
    };
    let mut source = CadIr::empty(Units::default());
    source.model.curves.push(Curve {
        id: CurveId("transformed-curve".into()),
        geometry: curve_geometry.clone(),
        source_object: None,
    });
    source.model.surfaces.push(Surface {
        id: SurfaceId("transformed-surface".into()),
        geometry: surface_geometry.clone(),
        source_object: None,
    });

    let mut output = Vec::new();
    write_step(&source, &mut output, &StepWriteOptions::default()).expect("write replicas");
    let text = String::from_utf8(output.clone()).expect("STEP output is UTF-8");
    assert!(text.contains("CURVE_REPLICA"));
    assert!(text.contains("SURFACE_REPLICA"));
    assert!(text.contains("CARTESIAN_TRANSFORMATION_OPERATOR_3D"));
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode replicas");
    assert!(decoded
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.geometry == curve_geometry));
    assert!(decoded
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.geometry == surface_geometry));
}

#[test]
fn forward_replica_dependencies_resolve_to_nested_transforms() {
    let decoded = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=DIRECTION('',(0.,1.,0.));
#4=DIRECTION('',(0.,0.,1.));
#5=VECTOR('',#2,1.);
#6=LINE('',#1,#5);
#7=CARTESIAN_POINT('',(10.,20.,30.));
#8=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#2,#3,#7,2.,#4);
#9=CURVE_REPLICA('',#10,#8);
#10=CURVE_REPLICA('',#6,#8);
#11=AXIS2_PLACEMENT_3D('',#1,#4,#2);
#12=PLANE('',#11);
#13=SURFACE_REPLICA('',#14,#8);
#14=SURFACE_REPLICA('',#12,#8);",
    );
    let transform = Transform {
        rows: [
            [2.0, 0.0, 0.0, 10.0],
            [0.0, 2.0, 0.0, 20.0],
            [0.0, 0.0, 2.0, 30.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };
    let base_curve = CurveGeometry::Line {
        origin: Point3::new(0.0, 0.0, 0.0),
        direction: Vector3::new(1.0, 0.0, 0.0),
    };
    let expected_curve = CurveGeometry::Transformed {
        basis: Box::new(CurveGeometry::Transformed {
            basis: Box::new(base_curve),
            transform,
        }),
        transform,
    };
    let expected_surface = SurfaceGeometry::Transformed {
        basis: Box::new(SurfaceGeometry::Transformed {
            basis: Box::new(SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            }),
            transform,
        }),
        transform,
    };
    assert!(decoded
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id.as_str() == "step:data:curve#9" && curve.geometry == expected_curve));
    assert_eq!(
        decoded
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == "step:data:curve#6")
            .and_then(|curve| curve.source_object.as_ref())
            .map(|source| source.object_id.as_str()),
        Some("#10")
    );
    assert!(decoded
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.as_str() == "step:data:surface#13"
            && surface.geometry == expected_surface));
    assert_eq!(
        decoded
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == "step:data:surface#12")
            .and_then(|surface| surface.source_object.as_ref())
            .map(|source| source.object_id.as_str()),
        Some("#14")
    );
}

#[test]
fn cartesian_transformation_operator_derives_optional_axes() {
    let decoded = decode_inline(
        "#1=CARTESIAN_POINT('',(10.,20.,30.));
#2=CARTESIAN_TRANSFORMATION_OPERATOR_3D('', $,$,#1,$,$);
#3=CARTESIAN_POINT('',(0.,0.,0.));
#4=DIRECTION('',(1.,0.,0.));
#5=VECTOR('',#4,1.);
#6=LINE('',#3,#5);
#7=CURVE_REPLICA('',#6,#2);
#8=DIRECTION('',(1.,1.,0.));
#9=DIRECTION('',(0.,0.,1.));
#10=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#8,$,#3,2.,#9);
#11=CURVE_REPLICA('',#6,#10);
#12=GEOMETRIC_CURVE_SET('',(#7,#11));
#13=SHAPE_REPRESENTATION('',(#12),$);",
    );

    let transform_for = |id: &str| {
        decoded
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == id)
            .and_then(|curve| match &curve.geometry {
                CurveGeometry::Transformed { transform, .. } => Some(*transform),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing transformed curve {id}"))
    };
    let assert_rows = |actual: Transform, expected: [[f64; 4]; 4]| {
        for (row, values) in expected.iter().enumerate() {
            for (column, expected) in values.iter().enumerate() {
                assert!(
                    (actual.rows[row][column] - expected).abs() < 1.0e-12,
                    "matrix coefficient [{row}][{column}] was {}, expected {expected}",
                    actual.rows[row][column]
                );
            }
        }
    };

    assert_rows(
        transform_for("step:data:curve#7"),
        [
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 20.0],
            [0.0, 0.0, 1.0, 30.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    );
    let root_two = 2.0_f64.sqrt();
    assert_rows(
        transform_for("step:data:curve#11"),
        [
            [root_two, -root_two, 0.0, 0.0],
            [root_two, root_two, 0.0, 0.0],
            [0.0, 0.0, 2.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    );
}

#[test]
fn pcurve_replica_derives_orthogonal_two_dimensional_axes() {
    use cadmpeg_ir::geometry::PcurveGeometry;

    let source = String::from_utf8(include_bytes!("../tests/fixtures/ap214_sheet.p21").to_vec())
        .expect("fixture is UTF-8")
        .replace(
            "#55=DEFINITIONAL_REPRESENTATION('',(#54),#50);",
            "#55=DEFINITIONAL_REPRESENTATION('',(#73),#50);\n#71=DIRECTION('',(1.,1.));\n#72=CARTESIAN_TRANSFORMATION_OPERATOR_2D('',#71,$,#51,1.);\n#73=CURVE_REPLICA('',#54,#72);",
        )
        .replace(
            "#4=CARTESIAN_POINT('',(10.,0.,0.));",
            "#4=CARTESIAN_POINT('',(0.7071067811865476,0.7071067811865476,0.));",
        );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode pcurve replica");
    let pcurve = decoded
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.as_str() == "step:data:pcurve#56")
        .expect("replica pcurve");
    let PcurveGeometry::Transformed { transform, .. } = &pcurve.geometry else {
        panic!("pcurve replica lost its transformation")
    };
    let root_two = 2.0_f64.sqrt();
    assert!((transform.rows[0][0] - 1.0 / root_two).abs() < 1.0e-12);
    assert!((transform.rows[0][1] + 1.0 / root_two).abs() < 1.0e-12);
    assert!((transform.rows[1][0] - 1.0 / root_two).abs() < 1.0e-12);
    assert!((transform.rows[1][1] - 1.0 / root_two).abs() < 1.0e-12);
}

#[test]
fn surface_replica_dependencies_resolve_before_trimmed_surfaces() {
    let decoded = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=DIRECTION('',(0.,1.,0.));
#4=DIRECTION('',(0.,0.,1.));
#5=AXIS2_PLACEMENT_3D('',#1,#4,#2);
#6=PLANE('',#5);
#7=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#2,#3,#1,2.,#4);
#8=SURFACE_REPLICA('',#9,#7);
#9=SURFACE_REPLICA('',#6,#7);
#10=RECTANGULAR_TRIMMED_SURFACE('',#8,0.,1.,0.,1.,.T.,.T.);
#11=GEOMETRIC_SET('',(#10));
#12=SHAPE_REPRESENTATION('',(#11),#13);
#13=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );

    assert!(decoded.ir().model.surfaces.iter().any(|surface| {
        surface.id.as_str() == "step:data:surface#10"
            && matches!(surface.geometry, SurfaceGeometry::Transformed { .. })
    }));
    assert!(decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| {
            surface.surface.as_str() == "step:data:surface#10"
                && matches!(
                    &surface.definition,
                    cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Subset {
                        support,
                        parameter_ranges: [[0.0, 1.0], [0.0, 1.0]],
                        u_sense: Some(true),
                        v_sense: Some(true),
                    } if support.as_str() == "step:data:surface#8"
                )
        }));
    assert!(decoded.report().losses.iter().all(|loss| {
        !loss
            .message
            .contains("RECTANGULAR_TRIMMED_SURFACE #10 has invalid or unresolved")
    }));

    assert!(decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| {
            matches!(
                &surface.definition,
                cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Replica { source, .. }
                    if surface.surface.as_str() == "step:data:surface#8"
                        && source.as_str() == "step:data:surface#9"
            )
        }));
    let index = ModelIndex::new(decoded.ir());
    assert_eq!(
        model_surface_point_by_id(&index, &SurfaceId("step:data:surface#10".into()), 0.0, 0.0,),
        Some(Point3::new(0.0, 0.0, 0.0))
    );
    assert_eq!(
        model_surface_point_by_id(&index, &SurfaceId("step:data:surface#10".into()), 1.0, 1.0,),
        Some(Point3::new(4.0, 4.0, 0.0))
    );

    let mut output = Vec::new();
    write_step(decoded.ir(), &mut output, &StepWriteOptions::default())
        .expect("write trimmed surface replica");
    let text = String::from_utf8(output.clone()).expect("STEP output is UTF-8");
    assert!(text.contains("SURFACE_REPLICA"));
    assert!(text.contains("RECTANGULAR_TRIMMED_SURFACE"));
    let round_trip = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode trimmed surface replica");
    assert!(round_trip
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| {
            matches!(
                &surface.definition,
                cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Replica { source, .. }
                    if source.as_str().starts_with("step:data:surface#")
            )
        }));
}

#[test]
fn long_forward_curve_replica_chain_resolves_with_a_worklist() {
    let mut records = String::from(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=DIRECTION('',(0.,1.,0.));
#4=DIRECTION('',(0.,0.,1.));
#5=VECTOR('',#2,1.);
#6=LINE('',#1,#5);
#7=CARTESIAN_POINT('',(10.,20.,30.));
#8=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#2,#3,#7,2.,#4);
",
    );
    for id in 9..=264 {
        writeln!(records, "#{id}=CURVE_REPLICA('',#{},#8);", id + 1).expect("append curve replica");
    }
    records.push_str("#265=CURVE_REPLICA('',#6,#8);");

    let decoded = decode_inline(&records);
    assert!(decoded.ir().model.curves.iter().any(|curve| {
        curve.id.as_str() == "step:data:curve#9"
            && matches!(curve.geometry, CurveGeometry::Transformed { .. })
    }));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("CURVE_REPLICA #9 has invalid or unresolved parent/operator")
    }));
}

#[test]
fn long_forward_offset_surface_chain_resolves_with_a_worklist() {
    let mut records = String::from(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=PLANE('',#4);
",
    );
    for id in 6..=261 {
        writeln!(records, "#{id}=OFFSET_SURFACE('',#{},1.,.F.);", id + 1)
            .expect("append offset surface");
    }
    records.push_str("#262=OFFSET_SURFACE('',#5,1.,.F.);\n#263=GEOMETRIC_SET('',(#6));");

    let decoded = decode_inline(&records);
    assert!(decoded
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.as_str() == "step:data:surface#6"));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.message
            .contains("OFFSET_SURFACE #6 has invalid or unresolved support parameters")
    }));
}

#[test]
fn replicas_retain_bounded_parent_relations() {
    let decoded = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=DIRECTION('',(0.,1.,0.));
#4=DIRECTION('',(0.,0.,1.));
#5=VECTOR('',#2,1.);
#6=LINE('',#1,#5);
#7=TRIMMED_CURVE('',#6,(PARAMETER_VALUE(1.)),(PARAMETER_VALUE(2.)),.T.,.PARAMETER.);
#8=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#2,#3,#1,3.,#4);
#9=CURVE_REPLICA('',#7,#8);
#10=AXIS2_PLACEMENT_3D('',#1,#4,#2);
#11=PLANE('',#10);
#12=RECTANGULAR_TRIMMED_SURFACE('',#11,1.,2.,3.,4.,.T.,.T.);
#13=SURFACE_REPLICA('',#12,#8);
#14=GEOMETRIC_SET('',(#9,#13));
#15=SHAPE_REPRESENTATION('',(#14),#16);
#16=(GEOMETRIC_REPRESENTATION_CONTEXT(3) REPRESENTATION_CONTEXT('',''));",
    );

    assert!(decoded.ir().model.procedural_curves.iter().any(|curve| {
        matches!(
            &curve.definition,
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Replica { source, .. }
                if curve.curve.as_str() == "step:data:curve#9"
                    && source.as_str() == "step:data:curve#7"
        )
    }));
    assert!(decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| {
            matches!(
                &surface.definition,
                cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Replica { source, .. }
                    if surface.surface.as_str() == "step:data:surface#13"
                        && source.as_str() == "step:data:surface#12"
            )
        }));
    let index = ModelIndex::new(decoded.ir());
    assert_eq!(
        model_curve_point_by_id(&index, &CurveId("step:data:curve#9".into()), 0.0,),
        Some(Point3::new(3.0, 0.0, 0.0))
    );
    assert_eq!(
        model_surface_point_by_id(&index, &SurfaceId("step:data:surface#13".into()), 0.0, 0.0,),
        Some(Point3::new(3.0, 9.0, 0.0))
    );

    let mut output = Vec::new();
    write_step(decoded.ir(), &mut output, &StepWriteOptions::default())
        .expect("write replicas of bounded parents");
    let text = String::from_utf8(output).expect("STEP output is UTF-8");
    assert!(text.contains("CURVE_REPLICA"));
    assert!(text.contains("SURFACE_REPLICA"));
    assert!(text.contains("TRIMMED_CURVE"));
    assert!(text.contains("RECTANGULAR_TRIMMED_SURFACE"));
}

#[test]
pub(crate) fn hidden_body_geometry_and_visibility_round_trip() {
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
    assert_eq!(decoded.ir().model.bodies[0].visible, Some(false));

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
    assert_eq!(decoded.ir().model.bodies[0].visible, Some(false));

    // An explicitly visible body exports unchanged.
    let mut ir = unit_cube();
    ir.model.bodies[0].visible = Some(true);
    let s = export(&ir);
    assert!(s.contains("MANIFOLD_SOLID_BREP"));
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
pub(crate) fn body_color_becomes_per_face_styled_item_presentation() {
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
pub(crate) fn face_appearance_binding_styles_the_advanced_face() {
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

#[test]
fn vertex_appearance_binding_styles_the_vertex_point() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube();
    let vertex = ir.model.vertices[0].id.clone();
    ir.model.appearances.push(Appearance {
        id: AppearanceId("test:appearance#vertex".to_string()),
        name: Some("vertex green".to_string()),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(cadmpeg_ir::topology::Color {
            r: 0.125,
            g: 0.75,
            b: 0.25,
            a: 1.0,
        }),
        properties: std::collections::BTreeMap::default(),
        textures: Vec::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:appearance-binding#vertex".to_string(),
        target: AppearanceTarget::Vertex(vertex),
        appearance: AppearanceId("test:appearance#vertex".to_string()),
        source_entity_id: None,
        object_type: None,
        channels: std::collections::BTreeMap::default(),
    });

    let text = export(&ir);
    assert!(text.contains("POINT_STYLE"));
    assert!(text.contains("COLOUR_RGB('vertex green',0.125,0.75,0.25)"));

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(text), &DecodeOptions::default())
        .expect("decode vertex appearance");
    assert!(decoded
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| {
            matches!(
                &binding.target,
                AppearanceTarget::Vertex(vertex) if vertex.as_str().starts_with("step:data:vertex#")
            )
        }));
}

#[test]
fn point_presentation_layer_writes_the_cartesian_point_carrier() {
    use cadmpeg_ir::ids::LayerId;
    use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};

    let mut ir = unit_cube();
    let point = ir.model.points[0].id.clone();
    ir.model.presentation_layers.push(PresentationLayer {
        id: LayerId("test:layer#point".to_string()),
        name: "point layer".to_string(),
        description: Some("standalone points".to_string()),
        items: vec![PresentationItem::Point { point }],
    });

    let mut bytes = Vec::new();
    let report =
        write_step(&ir, &mut bytes, &StepWriteOptions::default()).expect("write point layer");
    assert!(!report.losses.iter().any(|loss| {
        loss.message
            .contains("layer 'point layer' has 1 item(s) without a writable STEP carrier")
    }));
    let text = String::from_utf8(bytes).expect("STEP is UTF-8");
    assert!(text.contains("PRESENTATION_LAYER_ASSIGNMENT('point layer','standalone points',"));
}

#[test]
fn presentation_layer_round_trips_product_occurrence_and_pmi_items() {
    use cadmpeg_ir::ids::{LayerId, OccurrenceId, PmiId, ProductDefinitionId};
    use cadmpeg_ir::pmi::{PmiAnnotation, PmiDefinition};
    use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};
    use cadmpeg_ir::products::{Occurrence, OccurrenceParent, ProductDefinition};

    let mut ir = unit_cube();
    let body = ir.model.bodies[0].id.clone();
    let parent_product = ProductDefinitionId("test:product#parent".into());
    let child_product = ProductDefinitionId("test:product#child".into());
    ir.model.product_definitions.extend([
        ProductDefinition {
            id: parent_product.clone(),
            kind: cadmpeg_ir::products::ProductDefinitionKind::Part,
            source_name: Some("Parent assembly".into()),
            label: Some("Parent assembly".into()),
            description: None,
            part_number: None,
            bom_properties: std::collections::BTreeMap::default(),
            bodies: Vec::new(),
            native_ref: None,
        },
        ProductDefinition {
            id: child_product.clone(),
            kind: cadmpeg_ir::products::ProductDefinitionKind::Part,
            source_name: Some("Child part".into()),
            label: Some("Child part".into()),
            description: None,
            part_number: None,
            bom_properties: std::collections::BTreeMap::default(),
            bodies: vec![body],
            native_ref: None,
        },
    ]);
    let root = OccurrenceId("test:occurrence#root".into());
    let child = OccurrenceId("test:occurrence#child".into());
    ir.model.occurrences.extend([
        Occurrence {
            id: root.clone(),
            prototype: cadmpeg_ir::products::PrototypeReference::Local {
                definition: parent_product.clone(),
            },
            parent: OccurrenceParent::Root,
            ordinal: 0,
            transform: Transform::identity(),
            prototype_transform: Transform::identity(),
            scale: [1.0; 3],
            name: Some("Root assembly".into()),
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
        },
        Occurrence {
            id: child.clone(),
            prototype: cadmpeg_ir::products::PrototypeReference::Local {
                definition: child_product,
            },
            parent: OccurrenceParent::Occurrence { occurrence: root },
            ordinal: 0,
            transform: Transform {
                rows: [
                    [1.0, 0.0, 0.0, 25.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
            },
            prototype_transform: Transform::identity(),
            scale: [1.0; 3],
            name: Some("Child occurrence".into()),
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
        },
    ]);
    let annotation = PmiId("test:pmi#note".into());
    ir.model.pmi.push(PmiAnnotation {
        id: annotation.clone(),
        name: Some("inspection note".into()),
        targets: Vec::new(),
        definition: PmiDefinition::Presentation {
            text: Some("inspect this assembly".into()),
            placement: Some(Transform::identity()),
            semantics: Vec::new(),
        },
    });
    ir.model.presentation_layers.push(PresentationLayer {
        id: LayerId("test:layer#mixed".into()),
        name: "mixed layer".into(),
        description: None,
        items: vec![
            PresentationItem::Product {
                product: parent_product,
            },
            PresentationItem::Occurrence { occurrence: child },
            PresentationItem::Pmi { annotation },
        ],
    });

    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        &StepWriteOptions {
            schema: StepSchema::Ap242Edition3,
            unsupported: StepUnsupportedPolicy::Reject,
            ..StepWriteOptions::default()
        },
    )
    .expect("write mixed presentation layer");
    assert!(!report.losses.iter().any(|loss| {
        loss.message
            .contains("layer 'mixed layer' has 3 item(s) without a writable STEP carrier")
    }));

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode mixed presentation layer");
    let layer = decoded
        .ir()
        .model
        .presentation_layers
        .iter()
        .find(|layer| layer.name == "mixed layer")
        .expect("mixed presentation layer");
    assert_eq!(layer.items.len(), 3);
    assert!(matches!(layer.items[0], PresentationItem::Product { .. }));
    assert!(matches!(
        layer.items[1],
        PresentationItem::Occurrence { .. }
    ));
    assert!(matches!(layer.items[2], PresentationItem::Pmi { .. }));
}

/// The soccer-ball case: a body carries a base color and one face overrides it.
/// Every face must be styled (body color pushed down onto the faces that do not
/// override it), and the overriding face must carry its own color.
#[test]
pub(crate) fn face_override_wins_over_body_color_and_body_fills_the_rest() {
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
