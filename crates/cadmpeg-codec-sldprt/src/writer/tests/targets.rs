// SPDX-License-Identifier: Apache-2.0
//! The write-target request reaching this encoder's `plan`.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{EncodeInput, Encoder, TargetRequest};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::units::Units;
use cadmpeg_ir::{FidelityResolution, RetainedSourceRecord, SourceFidelity};

use crate::{loss::SldprtLossCode, SldprtCodec};

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

    let CodecError::UnsupportedTarget {
        format,
        requested,
        available,
        ..
    } = &error
    else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(format, "sldprt");
    assert_eq!(requested.as_deref(), Some("sldprt:nonesuch"));
    for target in Encoder::targets(&SldprtCodec) {
        assert!(available.contains(target.id), "{available}");
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
    ir.source = Some(SourceMeta {
        format: "sldprt".into(),
        dialect: Some(cadmpeg_core::dialect::DialectId::pinned(dialect)),
        ..SourceMeta::default()
    });
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
