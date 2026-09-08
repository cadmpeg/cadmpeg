// SPDX-License-Identifier: Apache-2.0
//! The write-target request reaching this encoder's `plan`.

use cadmpeg_ir::codec::write::{EncodeInput, Encoder, TargetRequest};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::{FidelityResolution, RetainedSourceRecord, SourceFidelity};

use crate::{loss::F3dLossCode, F3dCodec};

fn sourced_ir(dialect: &'static str) -> CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.source = Some(SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect::DialectId::pinned(dialect),
        )),
        std::collections::BTreeMap::new(),
    ));
    ir
}

#[test]
fn explicit_transcode_declines_present_image_without_claiming_it_is_unavailable() {
    let ir = sourced_ir("f3d:f3z-multi-document");
    let data = b"present retained image".to_vec();
    let mut fidelity = SourceFidelity::default();
    fidelity
        .retained_records
        .push(RetainedSourceRecord::retained(
            crate::ids::FILE_SOURCE_IMAGE_ID,
            "f3d",
            0,
            data,
        ));
    let plan = Encoder::plan(
        &F3dCodec,
        EncodeInput::new(&ir, Some(&fidelity)),
        TargetRequest::Explicit("f3d:manifest-3-2-0-0"),
    )
    .expect("explicit transcode plans");

    assert_eq!(&plan.report().fidelity(), &FidelityResolution::NotConsumed);
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
        .all(|loss| loss.code.local_code() != "source.preserved-image-unavailable"));
}

#[test]
fn cross_format_write_has_no_dialect_displacement() {
    let ir = sourced_ir("step:ap242-edition-3");
    let fidelity = SourceFidelity::default();
    let plan = Encoder::plan(
        &F3dCodec,
        EncodeInput::new(&ir, Some(&fidelity)),
        TargetRequest::Explicit("f3d:manifest-3-2-0-0"),
    )
    .expect("cross-format synthesis plans");
    assert_eq!(&plan.report().fidelity(), &FidelityResolution::NotConsumed);
    assert!(plan.report().losses.iter().all(|loss| loss.code
        != F3dLossCode::SourceDialectDisplaced.kind()
        && loss.code != F3dLossCode::SourcePreservedImageUnavailable.kind()));
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

    // No fidelity was offered, so the sealed wrapper owns this state and
    // stamps `NotProvided`; the missing-image fact survives as the typed loss.
    assert_eq!(&plan.report().fidelity(), &FidelityResolution::NotProvided);
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "source.preserved-image-unavailable"));
}
