// SPDX-License-Identifier: Apache-2.0
use super::reference_manifest;

#[test]
fn captured_manifest_admission_bounds_paths_and_nesting() {
    let segment = format!(
        "references/{}/{}/{}/",
        "1".repeat(24),
        "2".repeat(24),
        "3".repeat(24)
    );
    for depth in 1..=8 {
        assert_eq!(
            reference_manifest(&format!("{}manifest.json", segment.repeat(depth))),
            depth <= 7
        );
    }
    for path in [
        "manifest.json",
        "attachments/manifest.json",
        "references/short/short/short/manifest.json",
        "references/../manifest.json",
    ] {
        assert!(!reference_manifest(path));
    }
    assert!(!reference_manifest(&format!(
        "{segment}attachments/manifest.json"
    )));
}
