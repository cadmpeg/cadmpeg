// SPDX-License-Identifier: Apache-2.0
//! Measured body geometry across an `FCStd` patch write, compared without
//! entity identities.

use std::io::Cursor;
use std::path::Path;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::report::export::WritePath;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_test_support::measured::{assert_measures_agree, measures_decide};

use crate::FcstdCodec;

/// The document label that the patch write stores.
const EDITED_LABEL: &str = "measured round trip";

/// Every corpus document that measures a comparable body keeps its
/// measured geometry when an edit of its document label forces the patch
/// writer to repack the archive.
#[test]
fn patched_corpus_documents_keep_measured_geometry() {
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/freecad_fcstd/fixtures");
    let mut paths: Vec<_> = std::fs::read_dir(&fixtures)
        .expect("FreeCAD corpus fixtures")
        .map(|entry| entry.expect("fixture entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "FCStd")
        })
        .collect();
    paths.sort();
    let mut measured = Vec::new();
    for path in paths {
        let label = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("fixture name")
            .to_owned();
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap_or_else(|_| panic!("{label}: the fixture does not decode"));
        if !measures_decide(&label, decoded.ir()) {
            continue;
        }
        let mut edited = decoded.ir().clone();
        FcstdCodec
            .set_property_value_attribute(
                &mut edited,
                crate::FcstdPropertyOwner::Document,
                "Label",
                0,
                "value",
                EDITED_LABEL,
            )
            .unwrap_or_else(|_| panic!("{label}: the document label is not editable"));
        let mut encoded = Vec::new();
        let plan = FcstdCodec
            .plan(EncodeInput::new(&edited, None), TargetRequest::Inherit)
            .unwrap_or_else(|_| panic!("{label}: the edit is not planned"));
        assert!(
            matches!(plan.report().write_path(), WritePath::Patched { .. }),
            "{label}: the label edit does not take the patch path"
        );
        plan.write_to(&mut encoded)
            .unwrap_or_else(|_| panic!("{label}: the patch write fails"));
        let round_trip = FcstdCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap_or_else(|_| panic!("{label}: the patched archive does not decode"));
        assert_eq!(
            round_trip.ir().model.bodies.len(),
            decoded.ir().model.bodies.len(),
            "{label}: the body count changes"
        );
        assert_measures_agree(&label, decoded.ir(), round_trip.ir());
        measured.push(label);
    }
    for expected in ["binary_exact_shape", "geometry_topology"] {
        assert!(
            measured.iter().any(|label| label == expected),
            "an exact-shape corpus document is not measured"
        );
    }
}
