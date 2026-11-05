// SPDX-License-Identifier: Apache-2.0
//! The native arena registry and the synthesis native dispositions declare the
//! same arenas (#231 W3, #209 §2.3).

use std::collections::BTreeSet;
use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::SldprtCodec;

fn fixtures() -> Vec<(String, Vec<u8>)> {
    cadmpeg_test_support::golden::Harness::new(
        env!("CARGO_MANIFEST_DIR"),
        "sldprt",
        "UPDATE_GOLDEN=1 cargo test-fast golden",
    )
    .fixture_inputs()
}

#[test]
fn native_arena_registry_matches_native_dispositions_on_every_golden() {
    let registry = crate::native::SLDPRT_ARENA_NAMES
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<BTreeSet<_>>();
    let mut checked = 0;
    for (name, bytes) in fixtures() {
        let decoded = match SldprtCodec.decode(&mut Cursor::new(&bytes), &DecodeOptions::default()) {
            Ok(decoded) => decoded,
            Err(error) => {
                let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/golden/decode")
                    .join(format!("{name}.json"));
                let snapshot: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(golden).expect("decode golden"),
                )
                .expect("decode golden JSON");
                assert_eq!(snapshot["decode_error"].as_str(), Some(error.to_string().as_str()));
                continue;
            }
        };
        for fidelity in [None, Some(decoded.source_fidelity())] {
            let plan = SldprtCodec
                .plan(
                    EncodeInput::new(decoded.ir(), fidelity),
                    TargetRequest::Inherit,
                )
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            let Some(namespace) = plan.report().native_dispositions.namespaces.get("sldprt") else {
                panic!("{name}: the sldprt namespace is declared");
            };
            let declared = namespace.arenas.keys().cloned().collect::<BTreeSet<_>>();
            for arena in &registry {
                assert!(
                    declared.contains(arena),
                    "{name}: registry arena `{arena}` is declared"
                );
            }
            for arena in &declared {
                assert!(
                    registry.contains(arena),
                    "{name}: declared arena `{arena}` is outside the registry"
                );
            }
            checked += 1;
        }
    }
    assert!(checked > 0, "at least one golden exercises the registry");
}
