// SPDX-License-Identifier: Apache-2.0
//! Every document-reachable shape a live `FreeCAD` decode produces refuses an
//! unknown key.
//!
//! The committed `FreeCAD` decode goldens store `native` as an elision marker,
//! so their `ir` does not read back as a `CadIr` and the workspace-wide sweep
//! in `cadmpeg-ir` skips them. This sweep decodes the charter fixtures instead
//! and runs the same walk over the live document, which reaches the sketch,
//! annotation, assembly, and product shapes no other codec's goldens carry.

use std::collections::BTreeSet;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use cadmpeg_codec_freecad::FcstdCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::unknown_keys::accepting_shapes;

/// Extension of the committed fixture inputs.
const FIXTURE_EXTENSION: &str = "FCStd";

#[test]
fn every_decoded_shape_refuses_an_unknown_key() {
    let fixtures = fixture_inputs();
    assert!(
        !fixtures.is_empty(),
        "the sweep found no fixtures to decode"
    );
    let mut accepting = BTreeSet::new();
    let mut swept = 0_usize;
    for path in fixtures {
        let bytes = std::fs::read(&path).expect("read fixture");
        let result = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode fixture");
        let ir = serde_json::to_value(result.ir()).expect("serialize ir");
        let (shapes, count) = accepting_shapes(&ir);
        assert!(count > 0, "a decoded document reads back as a document");
        swept += count;
        accepting.extend(shapes);
    }
    assert!(swept > 0, "the sweep walked {swept} shapes");
    assert!(
        accepting.is_empty(),
        "{} decoded shapes accept an unknown key:\n{}",
        accepting.len(),
        accepting.into_iter().collect::<Vec<_>>().join("\n")
    );
}

/// The charter fixtures this codec's goldens decode, in directory order.
fn fixture_inputs() -> Vec<PathBuf> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the crate manifest sits two levels below the repository root")
        .join("corpus/freecad_fcstd/fixtures");
    let mut found = Vec::new();
    let entries = std::fs::read_dir(&directory).expect("read the fixture directory");
    for entry in entries {
        let file = entry.expect("fixture entry").path();
        if file
            .extension()
            .is_some_and(|extension| extension == FIXTURE_EXTENSION)
        {
            found.push(file);
        }
    }
    found.sort();
    found
}
