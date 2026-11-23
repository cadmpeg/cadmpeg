// SPDX-License-Identifier: Apache-2.0
//! The write-target request reaching this encoder's `plan`.

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::{RetainedSourceRecord, SourceFidelity};
use std::io::Cursor;

use crate::test_support::container::make_block;
use crate::test_support::history::sldprt_with_body_and_history;
use crate::test_support::parasolid::triangle_body;
use crate::{dialect::SldprtDialect, SldprtCodec};

#[test]
fn semantic_writer_reclassifies_the_final_retained_envelope() {
    let mut source = sldprt_with_body_and_history(&triangle_body());
    source.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<?xml version="1.0"?><swSolidWorks swVersion="13100"><swModel swName="part" swConfigurationName="Default"/></swSolidWorks>"#,
    ));
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(crate::test_support::container::native_envelope_fixture(
                &source,
            )),
            &DecodeOptions::default(),
        )
        .expect("versioned part decodes");
    let (mut ir, _, fidelity) = decoded.into_parts();
    let attributes = ir
        .source
        .take()
        .expect("decode classifies the source")
        .attributes;
    ir.source = Some(SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(SldprtDialect::classify(None)),
        attributes,
    ));
    let records = crate::source_records(&ir, &fidelity).expect("source records join");
    let mut written = Vec::new();
    let dialect = crate::writer::write_semantic_with_records(
        &ir,
        &fidelity.annotations,
        &records,
        &[],
        &mut written,
    )
    .expect("semantic write succeeds")
    .dialect;
    let redecoded = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .expect("written part decodes");
    let redecoded_dialect = redecoded
        .report()
        .dialects()
        .expect("written part classifies")
        .primary()
        .dialect();

    assert_eq!(dialect, SldprtDialect::SwVersion12000Plus.id());
    assert_eq!(&dialect, redecoded_dialect);
}

fn sourced_ir(dialect: &'static str) -> CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    ir.geometry.bodies[0].name = None;
    ir.geometry
        .faces
        .iter_mut()
        .for_each(|face| face.name = None);
    ir.geometry
        .edges
        .iter_mut()
        .for_each(|edge| edge.set_param_range(None));
    let matched = if dialect == "sldprt:unknown" {
        crate::dialect::SldprtDialect::classify(None)
    } else {
        cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect::DialectId::parse(dialect).expect("test id has dialect grammar"),
        )
    };
    ir.source = Some(SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(matched),
        std::collections::BTreeMap::new(),
    ));
    ir.state_root_definition(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        crate::decode::root_definition_id(),
        cadmpeg_ir::products::ExternalDocument::default(),
    )
    .expect("one root definition");
    ir
}

#[test]
fn explicit_cross_dialect_transcode_synthesizes_without_consuming_present_image() {
    let ir = sourced_ir("sldprt:sw-version-12000-plus");
    let data = b"present retained image".to_vec();
    let mut fidelity = SourceFidelity::new(cadmpeg_ir::hash::digest::Sha256Digest::digest(&data));
    let source_image_id: cadmpeg_ir::ids::UnknownId = crate::SOURCE_IMAGE_ID
        .to_owned()
        .try_into()
        .expect("source image identity");
    let record = RetainedSourceRecord::from_bytes(
        "sldprt",
        0,
        cadmpeg_ir::source_fidelity::RetainedBytes::Inline { data },
    )
    .expect("source image extent");
    fidelity
        .insert_retained_record(source_image_id, record)
        .expect("source image identity is unique");
    assert!(fidelity
        .retained_record(crate::SOURCE_IMAGE_ID)
        .unwrap()
        .data()
        .is_some());
    let plan = Encoder::plan(
        &SldprtCodec,
        EncodeInput::new(&ir, Some(&fidelity)),
        TargetRequest::Explicit("sldprt:unknown"),
    )
    .expect("the cross-dialect route synthesizes its Native framing");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "record.residual-unavailable"));
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    assert_ne!(&written[..4], &[0; 4], "file_id is nonzero");
    assert_eq!(&written[4..8], &[0, 0, 0, 4]);
}

#[test]
fn inherit_without_native_image_synthesizes_the_envelope() {
    let ir = sourced_ir("sldprt:unknown");
    let plan = Encoder::plan(
        &SldprtCodec,
        EncodeInput::new(&ir, None),
        TargetRequest::Inherit,
    )
    .expect("a missing source image leaves the envelope to synthesis");
    assert!(plan.report().losses.iter().any(|loss| {
        loss.code == crate::loss::SldprtLossCode::SourcePreservedImageUnavailable.kind()
    }));
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().geometry.faces.len(), ir.geometry.faces.len());
}
