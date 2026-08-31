// SPDX-License-Identifier: Apache-2.0
//! The write-target request reaching this encoder's `plan`.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions, EncodeInput, Encoder, TargetRequest};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::units::Units;
use cadmpeg_ir::{FidelityResolution, RetainedSourceRecord, SourceFidelity};
use std::io::Cursor;

use crate::test_support::{make_block, sldprt_with_body_and_history, triangle_body};
use crate::{dialect::SldprtDialect, loss::SldprtLossCode, SldprtCodec};

#[test]
fn first_solidworks_envelope_selects_the_written_dialect() {
    let sections = [
        (
            "Contents/Features",
            br#"<?xml version="1.0"?><swSolidWorks swVersion="11000"/>"#.as_slice(),
        ),
        (
            "Contents/SolidWorks",
            br#"<?xml version="1.0"?><swSolidWorks swVersion="34000"/>"#.as_slice(),
        ),
    ];
    let declaration =
        crate::container::first_solidworks_envelope(sections.iter().map(|(_, payload)| *payload))
            .and_then(|envelope| envelope.sw_version);
    let dialect = crate::dialect::SldprtDialect::from_declaration(declaration.as_deref());

    assert_eq!(dialect, crate::dialect::SldprtDialect::SwVersionPre12000);
}

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
        SldprtDialect::classify(None),
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

/// An explicit target this writer does not produce is refused by `plan` itself,
/// with the catalog in the message.
///
/// The check runs before any synthesis, so an empty document is enough: what is
/// under test is that the request reaches the encoder at all. This writer has a
/// single dialect, which is exactly why the refusal must exist — every other id
/// is a claim it cannot honour.
#[test]
fn plan_refuses_an_explicit_target_outside_the_catalog() {
    let ir = CadIr::empty(Units::default());
    let error = Encoder::plan(
        &SldprtCodec,
        EncodeInput::new(&ir, None),
        TargetRequest::Explicit("sldprt:nonesuch"),
    )
    .err()
    .expect("an id outside the catalog is refused");

    let CodecError::UnsupportedTarget(refusal) = &error else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(refusal.format(), "sldprt");
    assert_eq!(refusal.requested(), Some("sldprt:nonesuch"));
    for target in Encoder::targets(&SldprtCodec) {
        assert!(
            refusal
                .available()
                .iter()
                .any(|available| available == target),
            "{:?}",
            refusal.available()
        );
    }
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
        matched,
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

    assert_eq!(plan.fidelity_resolution(), &FidelityResolution::NotConsumed);
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

    assert_eq!(
        plan.fidelity_resolution(),
        &FidelityResolution::Degraded {
            reason: "preserved SLDPRT source image is unavailable".into(),
        }
    );
    let unavailable = plan
        .report()
        .losses
        .iter()
        .find(|loss| loss.code.code == "source.preserved-image-unavailable")
        .expect("missing sidecar charges image unavailability");
    assert!(unavailable.message.contains("regenerated from IR"));
}
