// SPDX-License-Identifier: Apache-2.0
//! The write-target request reaching this encoder's `plan`.

use cadmpeg_ir::codec::write::{EncodeInput, Encoder, TargetRequest};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::{FidelityResolution, RetainedSourceRecord, SourceFidelity};
use std::io::Cursor;

use crate::test_support::{make_block, sldprt_with_body_and_history, triangle_body};
use crate::{dialect::SldprtDialect, loss::SldprtLossCode, SldprtCodec};

#[test]
fn semantic_writer_reclassifies_the_final_retained_envelope() {
    let mut source = sldprt_with_body_and_history(&triangle_body());
    source.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<?xml version="1.0"?><swSolidWorks swVersion="13100"><swModel swName="part" swConfigurationName="Default"/></swSolidWorks>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
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
        &mut written,
    )
    .expect("semantic write succeeds");
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
    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.bodies[0].name = None;
    ir.model.faces.iter_mut().for_each(|face| face.name = None);
    ir.model
        .edges
        .iter_mut()
        .for_each(|edge| edge.param_range = None);
    let matched = if dialect == "sldprt:unknown" {
        crate::dialect::SldprtDialect::classify(None)
    } else {
        cadmpeg_core::dialect::DialectMatch::admitted(cadmpeg_core::dialect::DialectId::pinned(
            dialect,
        ))
    };
    ir.source = Some(SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(matched),
        std::collections::BTreeMap::new(),
    ));
    ir
}

#[test]
fn explicit_transcode_declines_present_image_without_claiming_it_is_unavailable() {
    let ir = sourced_ir("sldprt:sw-version-12000-plus");
    let data = b"present retained image".to_vec();
    let mut fidelity = SourceFidelity::default();
    fidelity.retained_records.push(RetainedSourceRecord {
        id: crate::SOURCE_IMAGE_ID.into(),
        stream: "sldprt".into(),
        offset: 0,
        byte_len: data.len() as u64,
        sha256: sha256_hex(&data),
        data: Some(data),
    });
    let plan = Encoder::plan(
        &SldprtCodec,
        EncodeInput::new(&ir, Some(&fidelity)),
        TargetRequest::Explicit("sldprt:unknown"),
    )
    .expect("explicit transcode plans");

    assert_eq!(&plan.report().fidelity, &FidelityResolution::NotConsumed);
    assert_eq!(plan.report().write_path, cadmpeg_ir::WritePath::Synthesized);
    let displacement = plan
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == SldprtLossCode::SourceDialectDisplaced.kind())
        .expect("the source dialect displacement is charged");
    assert!(displacement
        .message
        .contains("sldprt:sw-version-12000-plus"));
    assert!(displacement.message.contains("sldprt:unknown"));
    assert!(plan
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.code != "source.preserved-image-unavailable"));
    assert!(plan
        .report()
        .notes
        .iter()
        .any(|note| note == "source container regenerated from IR"));
}

#[test]
fn inherit_with_missing_image_charges_preserved_image_unavailable() {
    let ir = sourced_ir("sldprt:unknown");
    let plan = Encoder::plan(
        &SldprtCodec,
        EncodeInput::new(&ir, None),
        TargetRequest::Inherit,
    )
    .expect("inherit synthesizes the catalog source dialect");

    // No fidelity was provided, so the sealed wrapper resolves the report to
    // `NotProvided`; the image-missing reason survives as the typed loss below.
    assert_eq!(&plan.report().fidelity, &FidelityResolution::NotProvided);
    let unavailable = plan
        .report()
        .losses
        .iter()
        .find(|loss| loss.code.code == "source.preserved-image-unavailable")
        .expect("missing sidecar charges image unavailability");
    assert!(unavailable.message.contains("regenerated from IR"));
}
