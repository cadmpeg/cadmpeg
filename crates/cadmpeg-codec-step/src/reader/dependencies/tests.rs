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
