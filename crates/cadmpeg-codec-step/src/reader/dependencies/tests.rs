// SPDX-License-Identifier: Apache-2.0
//! STEP external-document dependency tests.

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
pub(crate) fn decode_reports_data_section_external_dependencies() {
    let bytes = include_bytes!("../../../tests/fixtures/ap242_external_documents.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode external document dependencies");

    assert!(result.report().notes.contains(
        &"external document SPEC-42 (Interface control drawing) from supplier vault".into()
    ));
    assert!(result
        .report()
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
fn standalone_relative_uri_is_retained_without_filesystem_resolution() {
    let bytes = include_bytes!("../../parse/tests/data/er01_standalone_relative_uri.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode standalone URI witness without a transport base");

    assert!(result
        .report()
        .notes
        .contains(&"external reference #10 -> parts/child.p21#target".into()));
    assert!(result
        .report()
        .notes
        .contains(&"external document doc-id (doc-name) from parts/document.p21#target".into()));

    let summary = StepCodec::default()
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("inspect standalone URI witness");
    let references = summary
        .entries
        .iter()
        .find(|entry| entry.name == "REFERENCE")
        .expect("reference inventory");
    assert_eq!(
        references.attributes["external_uris"],
        "parts/child.p21#target,#local_target"
    );
}

#[test]
fn resource_schemes_and_uuid_references_require_external_access() {
    let bytes = include_bytes!("../../parse/tests/data/er02_resource_access_witness.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode resource access witness without transport access");

    for note in [
        "external reference #10 -> https://example.invalid/part.p21#shape",
        "external reference #11 -> file:///definitely/not/a/real/part.p21#shape",
        "external reference #12 -> urn:uuid:123e4567-e89b-12d3-a456-426614174000#shape",
        "external reference #13 -> #123e4567-e89b-12d3-a456-426614174000",
    ] {
        assert!(
            result.report().notes.contains(&note.into()),
            "missing {note}"
        );
    }

    let summary = StepCodec::default()
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("inspect resource access witness");
    let references = summary
        .entries
        .iter()
        .find(|entry| entry.name == "REFERENCE")
        .expect("reference inventory");
    assert_eq!(
        references.attributes["external_uris"],
        "https://example.invalid/part.p21#shape,file:///definitely/not/a/real/part.p21#shape,urn:uuid:123e4567-e89b-12d3-a456-426614174000#shape,#123e4567-e89b-12d3-a456-426614174000"
    );
}

#[test]
fn decode_does_not_invoke_the_external_resource_resolver() {
    let bytes = include_bytes!("../../parse/tests/data/er02_resource_access_witness.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode must not access the URI resources");

    assert_eq!(
        result
            .report()
            .notes
            .iter()
            .filter(|note| note.starts_with("external reference "))
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec![
            "external reference #10 -> https://example.invalid/part.p21#shape",
            "external reference #11 -> file:///definitely/not/a/real/part.p21#shape",
            "external reference #12 -> urn:uuid:123e4567-e89b-12d3-a456-426614174000#shape",
            "external reference #13 -> #123e4567-e89b-12d3-a456-426614174000",
        ]
    );
}

#[derive(Debug, PartialEq)]
struct CallerEntityBinding {
    local_occurrence: u64,
    resource_uri: String,
    anchor_name: String,
    target_id: u64,
}

fn bind_entity_reference(
    root: &crate::parse::Exchange,
    target: &crate::parse::Exchange,
) -> Result<CallerEntityBinding, &'static str> {
    let reference = root
        .references
        .iter()
        .find(|reference| reference.name == "#10")
        .ok_or("missing root reference")?;
    let (resource_uri, anchor_name) = reference
        .uri
        .split_once('#')
        .ok_or("reference has no anchor fragment")?;
    if resource_uri.is_empty() || anchor_name.is_empty() {
        return Err("reference has an incomplete resource identity");
    }
    if crate::reader::schema_identifiers(root) != crate::reader::schema_identifiers(target) {
        return Err("resource schemas differ");
    }
    let root_units = unit_signatures(root);
    let target_units = unit_signatures(target);
    if root_units != target_units {
        return Err("resource units differ");
    }
    let root_context = context_signature(root).ok_or("root has no coordinate context")?;
    let target_context = context_signature(target).ok_or("target has no coordinate context")?;
    if root_context != target_context {
        return Err("resource coordinate contexts differ");
    }
    let anchor = target
        .anchors
        .iter()
        .find(|anchor| anchor.name == anchor_name)
        .ok_or("target anchor is missing")?;
    let crate::parse::Value::Reference(target_id) = anchor.value else {
        return Err("target anchor is not an entity instance");
    };
    let target_record = target
        .records
        .get(&target_id)
        .ok_or("target entity instance is missing")?;
    if !target_record
        .partials
        .iter()
        .any(|partial| partial.name == "CARTESIAN_POINT")
    {
        return Err("target entity is not the admitted AP242 anchor type");
    }
    Ok(CallerEntityBinding {
        local_occurrence: 10,
        resource_uri: resource_uri.to_owned(),
        anchor_name: anchor_name.to_owned(),
        target_id,
    })
}

fn unit_signatures(exchange: &crate::parse::Exchange) -> Vec<Vec<crate::parse::PartialRecord>> {
    exchange
        .records
        .values()
        .filter(|record| {
            record
                .partials
                .iter()
                .any(|partial| partial.name == "LENGTH_UNIT" || partial.name == "PLANE_ANGLE_UNIT")
        })
        .map(|record| record.partials.clone())
        .collect()
}

fn context_signature(
    exchange: &crate::parse::Exchange,
) -> Option<Vec<crate::parse::PartialRecord>> {
    exchange
        .records
        .values()
        .find(|record| {
            record
                .partials
                .iter()
                .any(|partial| partial.name == "GEOMETRIC_REPRESENTATION_CONTEXT")
        })
        .map(|record| record.partials.clone())
}

#[test]
fn caller_composition_binds_annex_j_style_target_after_resource_checks() {
    let root_bytes = include_bytes!("tests/data/er05_distributed_root.p21");
    let target_bytes = include_bytes!("tests/data/er05_distributed_subsidiary.p21");
    let (root, root_diagnostics) = crate::parse::parse(root_bytes).expect("parse distributed root");
    let (target, target_diagnostics) =
        crate::parse::parse(target_bytes).expect("parse distributed subsidiary");
    assert!(root_diagnostics.is_empty());
    assert!(target_diagnostics.is_empty());

    let binding = bind_entity_reference(&root, &target).expect("admit external entity binding");
    assert_eq!(
        binding,
        CallerEntityBinding {
            local_occurrence: 10,
            resource_uri: "https://example.invalid/er05/subsidiary.p21".into(),
            anchor_name: "remote_point".into(),
            target_id: 12,
        }
    );
    assert_eq!(
        root.records[&5].partials[0].parameters,
        vec![crate::parse::Value::Reference(10)]
    );
    assert_eq!(target.anchors[0].value, crate::parse::Value::Reference(12));
    assert_eq!(
        target.records[&11].partials[0].parameters,
        vec![crate::parse::Value::Reference(20)]
    );

    let root_result = StepCodec::default()
        .decode(&mut Cursor::new(root_bytes), &DecodeOptions::default())
        .expect("decode root without importing target");
    let target_result = StepCodec::default()
        .decode(&mut Cursor::new(target_bytes), &DecodeOptions::default())
        .expect("decode target independently");
    assert_eq!(root_result.ir().units, target_result.ir().units);
    assert!(root_result
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.id.as_str() == "step:data:point#4"));
    assert!(target_result
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.id.as_str() == "step:data:point#12"));
    assert!(!root_result
        .ir()
        .model
        .points
        .iter()
        .any(|point| point.id.as_str() == "step:data:point#12"));
    assert_eq!(
        root_result.ir().source.as_ref().unwrap().attributes["entity_instances"],
        "6"
    );
    assert_eq!(
        target_result.ir().source.as_ref().unwrap().attributes["entity_instances"],
        "6"
    );

    let mismatched_units = String::from_utf8(target_bytes.to_vec())
        .unwrap()
        .replace("SI_UNIT(.MILLI.,.METRE.)", "SI_UNIT(.CENTI.,.METRE.)");
    let (mismatched_units, _) =
        crate::parse::parse(mismatched_units.as_bytes()).expect("parse mismatched-unit target");
    assert_eq!(
        bind_entity_reference(&root, &mismatched_units),
        Err("resource units differ")
    );

    let mismatched_context = String::from_utf8(target_bytes.to_vec()).unwrap().replace(
        "REPRESENTATION_CONTEXT('model','3D')",
        "REPRESENTATION_CONTEXT('other','3D')",
    );
    let (mismatched_context, _) = crate::parse::parse(mismatched_context.as_bytes())
        .expect("parse mismatched-context target");
    assert_eq!(
        bind_entity_reference(&root, &mismatched_context),
        Err("resource coordinate contexts differ")
    );
}

#[test]
fn resource_metadata_and_uri_spellings_do_not_create_cache_identity() {
    let bytes = include_bytes!("tests/data/er04_cache_identity.p21");
    let (exchange, diagnostics) = crate::parse::parse(bytes).expect("parse cache witness");
    assert!(diagnostics.is_empty());
    let population = exchange
        .header
        .iter()
        .find(|record| record.name == "SCHEMA_POPULATION")
        .expect("schema population header");
    let crate::parse::Value::List(entries) = &population.parameters[0] else {
        panic!("schema population entries");
    };
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries[0],
        crate::parse::Value::List(vec![
            crate::parse::Value::String(b"https://example.invalid/model.p21".to_vec()),
            crate::parse::Value::String(b"2026-08-16T00:00:00".to_vec()),
            crate::parse::Value::Omitted,
        ])
    );
    assert_eq!(
        entries[1],
        crate::parse::Value::List(vec![
            crate::parse::Value::String(b"https://example.invalid/model.p21".to_vec()),
            crate::parse::Value::String(b"2026-08-17T00:00:00".to_vec()),
            crate::parse::Value::Omitted,
        ])
    );

    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode cache witness without resource access");
    assert!(result
        .report()
        .notes
        .contains(&"external reference #10 -> https://example.invalid/model.p21#shape".into()));
    assert!(result
        .report()
        .notes
        .contains(&"external reference #11 -> https://example.invalid/./model.p21#shape".into()));
    let source = result.ir().source.as_ref().expect("STEP source metadata");
    assert!(!source.attributes.keys().any(|key| key.contains("cache")));

    let summary = StepCodec::default()
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("inspect cache witness");
    let references = summary
        .entries
        .iter()
        .find(|entry| entry.name == "REFERENCE")
        .expect("reference inventory");
    assert_eq!(
        references.attributes["external_uris"],
        "https://example.invalid/model.p21#shape,https://example.invalid/./model.p21#shape"
    );
}

#[test]
fn signed_resource_digest_and_timestamp_are_retained_without_cache_identity() {
    let bytes = include_bytes!("tests/data/er04_cache_identity_signed.p21");
    let signed_resource = include_bytes!("../../signature/tests/data/sg04_openssl_detached.p21");
    let (exchange, diagnostics) = crate::parse::parse(bytes).expect("parse signed cache witness");
    assert!(diagnostics.is_empty());
    let signed_exchange = crate::parse::parse(signed_resource)
        .expect("parse signed resource")
        .0;
    assert_eq!(signed_exchange.signature_sections.len(), 1);

    let population = exchange
        .header
        .iter()
        .find(|record| record.name == "SCHEMA_POPULATION")
        .expect("signed schema population header");
    let crate::parse::Value::List(entries) = &population.parameters[0] else {
        panic!("signed schema population entries");
    };
    assert_eq!(entries.len(), 2);
    for (entry, (address, timestamp)) in entries.iter().zip([
        ("signature/sg04_openssl_detached.p21", "2026-08-16T00:00:00"),
        (
            "signature/./sg04_openssl_detached.p21",
            "2026-08-17T00:00:00",
        ),
    ]) {
        assert_eq!(
            entry,
            &crate::parse::Value::List(vec![
                crate::parse::Value::String(address.as_bytes().to_vec()),
                crate::parse::Value::String(timestamp.as_bytes().to_vec()),
                crate::parse::Value::String(
                    b"PVXS8diN1zTOTu9AEL6T+aJH7u5ckF7wVCROqLXlIDA=".to_vec(),
                ),
            ])
        );
    }

    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode signed cache witness without resource access");
    let source = result.ir().source.as_ref().expect("STEP source metadata");
    assert!(!source.attributes.keys().any(|key| key.contains("cache")));
    assert!(result
        .report()
        .notes
        .iter()
        .all(|note| !note.starts_with("external resource")));
}

#[test]
fn complex_document_dependency_records_use_inherited_fields() {
    let result = decode_inline(
        "#1=DOCUMENT_TYPE('digital');
#2=(DOCUMENT('SPEC-42','Interface control drawing','',#1) DOCUMENT_FILE());
#3=(APPLIED_DOCUMENT_REFERENCE() DOCUMENT_REFERENCE(#2,'supplier vault'));",
    );

    assert!(result.report().notes.contains(
        &"external document SPEC-42 (Interface control drawing) from supplier vault".into()
    ));
    assert!(!result
        .ir()
        .native_unknowns("step")
        .expect("STEP unknown arena")
        .iter()
        .any(|record| {
            record.id.0 == "step:data:document#2"
                || record.id.0 == "step:data:document_file#2"
                || record.id.0 == "step:data:applied_document_reference#3"
                || record.id.0 == "step:data:document_reference#3"
        }));
}
