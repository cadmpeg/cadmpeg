// SPDX-License-Identifier: Apache-2.0
//! The native arena registry and the native dispositions declare the same
//! arenas (#231 W3, §2.3).

use std::collections::BTreeSet;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::{RhinoArchiveVersion, RhinoCodec};

fn fixtures() -> Vec<std::path::PathBuf> {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/fixtures");
    let mut fixtures = std::fs::read_dir(directory)
        .expect("golden fixtures")
        .map(|entry| entry.expect("fixture entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "3dm"))
        .filter(|path| {
            !path
                .file_stem()
                .is_some_and(|stem| stem.to_string_lossy().starts_with("reject_"))
        })
        .collect::<Vec<_>>();
    fixtures.sort();
    fixtures
}

#[test]
fn native_arena_registry_matches_native_dispositions_on_every_golden() {
    let registry = crate::native_registry::ARENAS
        .iter()
        .map(|arena| arena.to_string())
        .collect::<BTreeSet<_>>();
    let mut checked = 0;
    for path in fixtures() {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let decoded = RhinoCodec
            .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
            .expect("the golden decodes");
        let ir = decoded.ir();
        if RhinoCodec
            .plan(
                EncodeInput::new(ir, None),
                TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
            )
            .is_err()
        {
            continue;
        }
        let dispositions = super::super::native_dispositions(ir, RhinoArchiveVersion::V8)
            .expect("dispositions describe the document");
        let Some(namespace) = dispositions.namespaces.get("rhino") else {
            panic!("{}: the rhino namespace is declared", path.display());
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
            if registry.contains(arena) {
                continue;
            }
            assert!(
                ir.native
                    .namespace("rhino")
                    .and_then(|namespace| namespace.arenas().get(arena.as_str()))
                    .is_some_and(|records| !records.is_empty()),
                "{}: declared arena `{arena}` outside the registry is held by the document",
                path.display()
            );
        }
        checked += 1;
    }
    assert!(checked > 0, "at least one golden exercises the registry");
}
