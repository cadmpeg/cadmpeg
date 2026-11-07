// SPDX-License-Identifier: Apache-2.0

use crate::SldprtCodec;
use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::writer_closure::{without_fidelity, Redecoded};
use std::io::Cursor;

#[test]
fn nested_circle_survives_source_less_write() {
    let source = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/fixtures/authored_nested_circle.sldprt"
    ))
    .unwrap();
    let options = DecodeOptions {
        depth: SldprtCodec.request(),
        ..DecodeOptions::default()
    };
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &options)
        .unwrap();
    let check = |bytes: &[u8]| -> Result<Redecoded, String> {
        let result = SldprtCodec
            .decode(&mut Cursor::new(bytes), &options)
            .map_err(|error| error.to_string())?;
        Ok(Redecoded {
            ir: serde_json::to_value(result.ir()).map_err(|error| error.to_string())?,
            loss_codes: result
                .report()
                .losses
                .iter()
                .map(|loss| loss.code.to_string())
                .collect(),
            members: Vec::new(),
        })
    };
    let findings = without_fidelity(&decoded, &SldprtCodec, &check);
    assert!(findings.is_empty(), "{}", findings.join("\n"));
}
