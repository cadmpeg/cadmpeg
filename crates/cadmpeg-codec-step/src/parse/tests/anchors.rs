// SPDX-License-Identifier: Apache-2.0
//! Part 21 anchor, local-reference, and value-instance tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]
#![allow(unused_imports)]

use std::fmt::Write as _;
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeMode, InspectOptions};
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};
use cadmpeg_ir::eval::{
    model_curve_point_by_id, model_surface_partials_by_id, model_surface_point_by_id, pcurve_uv,
};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, SurfaceId};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::{LengthUnit, Units};
use cadmpeg_ir::CadIr;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::ids::StepIdentity;
use crate::test_support::{decode_inline, export};
use crate::{
    write_step, StepCodec, StepError, StepSchema, StepUnsupportedPolicy, StepWriteOptions,
};

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
fn standalone_relative_reference_has_no_implicit_transport_base() {
    let source = include_bytes!("data/er01_standalone_relative_uri.p21");
    let (exchange, diagnostics) = crate::parse::parse(source).expect("standalone URI witness");

    assert!(diagnostics.is_empty());
    assert_eq!(exchange.references[0].uri, "parts/child.p21#target");
    assert_eq!(
        exchange.records[&1].partials[0].parameters,
        vec![
            crate::parse::Value::Reference(10),
            crate::parse::Value::Reference(2),
        ]
    );
    assert_eq!(
        exchange.records[&4].partials[0].parameters,
        vec![
            crate::parse::Value::Reference(3),
            crate::parse::Value::String(b"parts/document.p21#target".to_vec()),
        ]
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
fn parser_resolves_anchor_and_reference_chain_before_name_recovery() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;3');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;<named>='anchored line';<number>=2.;ENDSEC;REFERENCE;@10=<#named>;@11=<#number>;@12=<#missing>;ENDSEC;DATA;#1=LINE('literal',#6,#7);#2=LINE(<named>,#6,#7);#3=LINE(@10,#6,#7);#4=LINE(@11,#6,#7);#5=LINE(@12,#6,#7);#6=KNOWN();#7=KNOWN();ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) = crate::parse::parse(source).expect("resolve name branches");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        crate::parse::ParseDiagnosticKind::OmittedEntityName
    );
    assert_eq!(
        exchange.records[&1].partials[0].parameters[0],
        crate::parse::Value::String(b"literal".to_vec())
    );
    assert_eq!(
        exchange.records[&2].partials[0].parameters[0],
        crate::parse::Value::String(b"anchored line".to_vec())
    );
    assert_eq!(
        exchange.records[&3].partials[0].parameters[0],
        crate::parse::Value::String(b"anchored line".to_vec())
    );
    assert_eq!(
        exchange.records[&4].partials[0].parameters[0],
        crate::parse::Value::String(Vec::new())
    );
    assert_eq!(
        exchange.records[&5].partials[0].parameters[0],
        crate::parse::Value::Omitted
    );
}
