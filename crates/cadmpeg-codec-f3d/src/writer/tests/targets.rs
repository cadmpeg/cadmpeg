// SPDX-License-Identifier: Apache-2.0
//! The write-target request reaching this encoder's `plan`.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{EncodeInput, Encoder, TargetRequest};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::units::Units;
use cadmpeg_ir::{FidelityResolution, RetainedSourceRecord, SourceFidelity};

use crate::{loss::F3dLossCode, F3dCodec};

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
        &F3dCodec,
        EncodeInput::new(&ir, None),
        TargetRequest::Explicit("f3d:nonesuch"),
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
    assert_eq!(format, "f3d");
    assert_eq!(requested.as_deref(), Some("f3d:nonesuch"));
    for target in Encoder::targets(&F3dCodec) {
        assert!(available.contains(target.id), "{available}");
    }
}

fn sourced_ir(dialect: &'static str) -> CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.source = Some(SourceMeta {
        format: "f3d".into(),
        dialect: Some(cadmpeg_core::dialect::DialectId::pinned(dialect)),
        ..SourceMeta::default()
    });
    ir
}

#[test]
fn explicit_transcode_declines_present_image_without_claiming_it_is_unavailable() {
    let ir = sourced_ir("f3d:f3z-multi-document");
    let data = b"present retained image".to_vec();
    let mut fidelity = SourceFidelity::default();
    fidelity.retained_records.push(RetainedSourceRecord {
        id: crate::ids::FILE_SOURCE_IMAGE_ID.into(),
        stream: "f3d".into(),
        offset: 0,
        byte_len: data.len() as u64,
        sha256: sha256_hex(&data),
        data: Some(data),
    });
    let plan = Encoder::plan(
        &F3dCodec,
        EncodeInput::new(&ir, Some(&fidelity)),
        TargetRequest::Explicit("f3d:manifest-3-2-0-0"),
    )
    .expect("explicit transcode plans");

    assert_eq!(plan.fidelity_resolution(), &FidelityResolution::NotConsumed);
    let displacement = plan
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::SourceDialectDisplaced.kind())
        .expect("the source dialect displacement is charged");
    assert!(displacement.message.contains("f3d:f3z-multi-document"));
    assert!(displacement.message.contains("f3d:manifest-3-2-0-0"));
    assert!(plan
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.code != "source.preserved-image-unavailable"));
}

#[test]
fn cross_format_write_has_no_dialect_displacement() {
    let mut ir = sourced_ir("step:ap242-edition-3");
    ir.source.as_mut().unwrap().format = "step".into();
    let plan = Encoder::plan(
        &F3dCodec,
        EncodeInput::new(&ir, None),
        TargetRequest::Explicit("f3d:manifest-3-2-0-0"),
    )
    .expect("cross-format synthesis plans");
    assert!(plan
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != F3dLossCode::SourceDialectDisplaced.kind()));
}

#[test]
fn inherit_with_missing_image_charges_preserved_image_unavailable() {
    let ir = sourced_ir("f3d:manifest-3-2-0-0");
    let plan = Encoder::plan(
        &F3dCodec,
        EncodeInput::new(&ir, None),
        TargetRequest::Inherit,
    )
    .expect("inherit synthesizes the catalog source dialect");

    assert_eq!(
        plan.fidelity_resolution(),
        &FidelityResolution::Degraded {
            reason: "preserved F3D source image is unavailable".into(),
        }
    );
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.code == "source.preserved-image-unavailable"));
}
