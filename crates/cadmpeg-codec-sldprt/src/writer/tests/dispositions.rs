// SPDX-License-Identifier: Apache-2.0
//! The native arena registry and the synthesis native dispositions declare the
//! same arenas (#231 W3, #209 §2.3).

use std::collections::BTreeSet;
use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::SldprtCodec;

fn fixtures() -> Vec<std::path::PathBuf> {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/fixtures");
    let mut fixtures = std::fs::read_dir(directory)
        .expect("golden fixtures")
        .map(|entry| entry.expect("fixture entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "sldprt")
        })
        .collect::<Vec<_>>();
    fixtures.sort();
    fixtures
}

#[test]
fn native_arena_registry_matches_native_dispositions_on_every_golden() {
    let registry = crate::native::SLDPRT_ARENA_NAMES
        .iter()
        .map(|arena| arena.to_string())
        .collect::<BTreeSet<_>>();
    let mut checked = 0;
    for path in fixtures() {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        // Fixtures that do not decode under default options are decoder
        // §4.5 cases; they are not writer input and stay out of this check.
        let decoded = match SldprtCodec.decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        {
            Ok(decoded) => decoded,
            Err(_) => continue,
        };
        for fidelity in [None, Some(decoded.source_fidelity())] {
            let plan = match SldprtCodec.plan(
                EncodeInput::new(decoded.ir(), fidelity),
                TargetRequest::Inherit,
            ) {
                Ok(plan) => plan,
                Err(_) => continue,
            };
            let Some(namespace) = plan.report().native_dispositions.namespaces.get("sldprt") else {
                panic!("{}: the sldprt namespace is declared", path.display());
            };
            let declared = namespace.arenas.keys().cloned().collect::<BTreeSet<_>>();
            for arena in &registry {
                assert!(
                    declared.contains(arena),
                    "{}: registry arena `{arena}` is declared",
                    path.display()
                );
            }
            for arena in &declared {
                assert!(
                    registry.contains(arena),
                    "{}: declared arena `{arena}` is outside the registry",
                    path.display()
                );
            }
            checked += 1;
        }
    }
    assert!(checked > 0, "at least one golden exercises the registry");
}
