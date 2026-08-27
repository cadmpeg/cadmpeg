// SPDX-License-Identifier: Apache-2.0
//! Shared STEP Part 21 exchange builders for crate tests.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::CadIr;

use crate::{write_step, StepCodec, StepSchema, StepWriteOptions};

pub(crate) fn export(ir: &CadIr) -> String {
    let mut buf = Vec::new();
    write_step(
        ir,
        &mut buf,
        StepSchema::default(),
        &StepWriteOptions::default(),
    )
    .expect("write");
    String::from_utf8(buf).expect("utf8")
}

pub(crate) fn decode_inline(records: &str) -> cadmpeg_ir::codec::DecodeResult {
    let source = format!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('test'),'2;1');\nFILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');\nFILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));\nENDSEC;\nDATA;\n{records}\nENDSEC;\nEND-ISO-10303-21;\n"
    );
    StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode inline STEP")
}
