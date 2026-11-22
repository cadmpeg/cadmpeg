// SPDX-License-Identifier: Apache-2.0
//! The box pass against the full pass on the measurement fixtures.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::CadIr;
use cadmpeg_measure::test_support::box_pass::{agreements, Agreement};

fn decode(path: &Path) -> CadIr {
    let mut bytes = Cursor::new(std::fs::read(path).expect("read the fixture"));
    let options = DecodeOptions::default();
    let decoded = match path.extension().and_then(|extension| extension.to_str()) {
        Some("p21") => cadmpeg_codec_step::StepCodec::default().decode(&mut bytes, &options),
        Some("igs") => cadmpeg_codec_iges::IgesCodec.decode(&mut bytes, &options),
        _ => panic!("{} is no STEP or IGES fixture", path.display()),
    };
    decoded.expect("the fixture decodes").ir().clone()
}

fn fixtures() -> Vec<PathBuf> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/measure");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(directory)
        .expect("list the fixtures")
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension != "md"))
        .collect();
    paths.sort();
    paths
}

/// Every body of every measurement fixture has the full pass's box in the
/// box pass, or a difference the box pass states.
#[test]
fn the_box_pass_differs_from_the_full_pass_only_where_it_says_so() {
    let paths = fixtures();
    assert_eq!(paths.len(), 8);
    for path in paths {
        let bodies = agreements(&decode(&path)).expect("both passes complete");
        assert!(!bodies.is_empty(), "{}", path.display());
        for body in bodies {
            assert_ne!(
                body.agreement,
                Agreement::Disagree,
                "{} {}",
                path.display(),
                body.body
            );
        }
    }
}

/// The plane whose outer boundary is the plane itself, with one hole: the
/// face is unbounded, so neither pass gives a box, rather than the box of
/// the hole.
#[test]
fn the_box_pass_gives_no_box_for_a_plane_with_an_implicit_outer_boundary() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/measure/measure_implicit_outer_plane.igs");
    let ir = decode(&path);
    let agreement: Vec<Agreement> = agreements(&ir)
        .expect("both passes complete")
        .into_iter()
        .map(|body| body.agreement)
        .collect();
    assert_eq!(agreement, [Agreement::Unboxed]);
    let boxes = cadmpeg_measure::body_boxes(&ir).expect("the box pass completes");
    let [body_box] = boxes.as_slice() else {
        panic!("one body box");
    };
    let missing = body_box.bbox().unavailable().expect("no box");
    assert!(
        missing.message().ends_with("boundary.unbounded-face"),
        "{}",
        missing.message()
    );
}
