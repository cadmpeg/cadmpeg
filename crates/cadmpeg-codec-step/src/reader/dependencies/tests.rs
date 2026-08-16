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

#[test]
fn document_reference_source_is_metadata_not_a_part21_uri_base() {
    let result = decode_inline(
        "#1=DOCUMENT_TYPE('manual');
#2=DOCUMENT('DOC-1','Drawing','',#1);
#3=(APPLIED_DOCUMENT_REFERENCE() DOCUMENT_REFERENCE(#2,'../other.p21#drawing'));",
    );

    assert!(result
        .report()
        .notes
        .contains(&"external document DOC-1 (Drawing) from ../other.p21#drawing".into()));
    assert!(!result
        .report()
        .notes
        .iter()
        .any(|note| note.contains("internal resource")));
}

#[test]
fn external_resource_uris_are_reported_without_implicit_access() {
    let bytes = include_bytes!("../../parse/tests/data/er02_resource_access.p21");
    let codec = StepCodec::default();
    let result = codec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode ER-02 resource access witness");

    assert_eq!(
        result
            .report()
            .notes
            .iter()
            .filter(|note| note.starts_with("external reference "))
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec![
            "external reference #10 -> https://example.invalid/part.p21#target",
            "external reference #11 -> file:///var/lib/cadmpeg/part.p21#target",
            "external reference #12 -> urn:uuid:97c6e1f0-3544-11e5-a2cb-0800200c9a66#target",
            "external reference #13 -> #97c6e1f0-3544-11e5-a2cb-0800200c9a66",
        ]
    );

    let summary = codec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .expect("inspect ER-02 resource access witness");
    let references = summary
        .entries
        .iter()
        .find(|entry| entry.name == "REFERENCE")
        .expect("REFERENCE inventory");
    assert_eq!(references.attributes["external_count"], "4");
    assert_eq!(
        references.attributes["external_uris"],
        "https://example.invalid/part.p21#target,file:///var/lib/cadmpeg/part.p21#target,urn:uuid:97c6e1f0-3544-11e5-a2cb-0800200c9a66#target,#97c6e1f0-3544-11e5-a2cb-0800200c9a66"
    );
}
