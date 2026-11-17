// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::EditableDecodeResult;

use crate::loss::StepLossCode;
use crate::StepCodec;

const MODULE_EDITION_2: &str = "ISO-10303-21;
HEADER;
FILE_DESCRIPTION((''),'2;1');
FILE_NAME('synthetic.step','2026-11-15T00:00:00',(''),(''),'','','');
FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 2 1 4 }'));
ENDSEC;
DATA;
#1=CARTESIAN_POINT('',(0.,0.,0.));
ENDSEC;
END-ISO-10303-21;
";

#[test]
fn module_edition_2_source_converts_as_ap242_edition_2() {
    let codec = StepCodec::default();
    let result = EditableDecodeResult::from(
        codec
            .decode(
                &mut Cursor::new(MODULE_EDITION_2.as_bytes()),
                &DecodeOptions::default(),
            )
            .expect("module edition 2 decode"),
    );
    let edition = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::SourceSchemaEditionAssumed.kind())
        .collect::<Vec<_>>();
    assert_eq!(edition.len(), 1);
    assert_eq!(
        edition[0].message,
        "FILE_SCHEMA object identifier { 1 0 10303 442 2 1 4 } names ISO/TS 10303-442:2018; this decode reads it as step:ap242-e2"
    );
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::SourceDialectUnverified.kind()));

    let plan = codec
        .plan(
            EncodeInput::new(result.ir(), Some(result.source_fidelity())),
            TargetRequest::Inherit,
        )
        .expect("same-format plan");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("same-format write");
    let text = String::from_utf8(bytes).expect("Part 21 text");
    assert!(text.contains(
        "FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 3 1 4 }'))"
    ));
}
