// SPDX-License-Identifier: Apache-2.0
//! Unrepresented-content accounting charges every check it owns.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::export::write_step;
use crate::loss::StepLossCode;
use crate::{StepCodec, StepSchema, StepWriteOptions};

/// Charging dropped PMI does not end the accounting: content checked after
/// PMI on a target without semantic PMI is still charged.
#[test]
fn pmi_dropped_by_schema_does_not_skip_later_losses() {
    let mut ir = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!(
                "../../tests/fixtures/ap242_semantic_pmi.p21"
            )),
            &DecodeOptions::default(),
        )
        .expect("decode semantic PMI")
        .into_parts()
        .0;
    ir.native.namespace_mut("f3d").arenas_mut().insert(
        "asm_histories".into(),
        vec![cadmpeg_ir::NativeRecord::new(
            cadmpeg_ir::ids::Identity::new("f3d:test:asm-history#0").expect("valid identity"),
            serde_json::Map::default(),
        )
        .expect("valid native identity")],
    );

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report-mode STEP write");
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::PmiAnnotationNotWritten.kind()));
    assert!(
        report
            .losses
            .iter()
            .any(|loss| loss.message.contains("source-native record(s)")),
        "{:?}",
        report.losses
    );
}
