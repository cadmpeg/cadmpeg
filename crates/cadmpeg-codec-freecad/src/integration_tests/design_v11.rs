// SPDX-License-Identifier: Apache-2.0
//! The v11 design layer of every authored fixture.

use std::path::Path;

use super::*;
use cadmpeg_ir::report::check::Check;

#[test]
fn every_fixture_states_at_most_one_scope_that_passes_the_design_rules() {
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
    assert!(paths
        .iter()
        .any(|path| path.ends_with("design_history.FCStd")));
    for path in paths {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode fixture");
        let ir = decoded.ir();
        assert!(ir.design.scopes.len() <= 1, "{}", path.display());
        let findings = cadmpeg_ir::validate_neutral(ir, Vec::new())
            .expect("validate the neutral model")
            .findings
            .into_iter()
            .filter(|finding| finding.check == Check::Design)
            .collect::<Vec<_>>();
        assert!(findings.is_empty(), "{}: {findings:#?}", path.display());
    }
}
