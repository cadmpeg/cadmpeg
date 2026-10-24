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
    let records = crate::source_records(&ir, &fidelity);
    let mut written = Vec::new();
    let dialect = crate::writer::write_semantic_with_records(
        &ir,
        &fidelity.annotations,
        &records,
        Some(&fidelity),
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
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
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
    ir
}

#[test]
fn explicit_cross_dialect_transcode_refuses_without_consuming_present_image() {
    let ir = sourced_ir("sldprt:sw-version-12000-plus");
    let data = b"present retained image".to_vec();
    let mut fidelity = SourceFidelity::default();
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
    let error = Encoder::plan(
        &SldprtCodec,
        EncodeInput::new(&ir, Some(&fidelity)),
        TargetRequest::Explicit("sldprt:unknown"),
    )
    .expect_err("cross-dialect nonconsuming route has no admitted Native framing");
    assert!(
        matches!(error, cadmpeg_core::CodecError::NotImplemented(ref detail)
        if detail.contains("validated retained local, central and EOCD signature tokens")),
        "{error}"
    );
}

#[test]
fn inherit_without_native_image_refuses_unproved_envelope_generation() {
    let ir = sourced_ir("sldprt:unknown");
    let error = Encoder::plan(
        &SldprtCodec,
        EncodeInput::new(&ir, None),
        TargetRequest::Inherit,
    )
    .expect_err("missing source image cannot supply Native framing");
    assert!(
        matches!(error, cadmpeg_core::CodecError::NotImplemented(ref detail)
        if detail.contains("validated retained local, central and EOCD signature tokens")),
        "{error}"
    );
}

mod fallback;
