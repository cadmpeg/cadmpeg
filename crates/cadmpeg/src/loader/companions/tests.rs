// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::Companions;

use super::Directory;

#[test]
fn paths_resolve_against_the_file_that_names_them() {
    let directory = tempfile::tempdir().expect("directory");
    let nested = directory.path().join("parts");
    std::fs::create_dir(&nested).expect("nested directory");
    std::fs::write(directory.path().join("root.usda"), b"root").expect("root");
    std::fs::write(nested.join("bolt.usda"), b"bolt").expect("bolt");
    std::fs::write(directory.path().join("shared.usda"), b"shared").expect("shared");
    let source = Directory::beside(&directory.path().join("root.usda"));
    let bolt = source
        .open(None, "./parts/bolt.usda", 64)
        .expect("relative to the input");
    assert_eq!(bolt.bytes, b"bolt");
    let shared = source
        .open(Some(&bolt.id), "../shared.usda", 64)
        .expect("relative to the naming file");
    assert_eq!(shared.bytes, b"shared");
    let again = source
        .open(None, "shared.usda", 64)
        .expect("a second path to the same file");
    assert_eq!(again.id, shared.id);
}

#[test]
fn a_missing_or_oversized_file_is_unavailable() {
    let directory = tempfile::tempdir().expect("directory");
    std::fs::write(directory.path().join("root.usda"), b"root").expect("root");
    let source = Directory::beside(&directory.path().join("root.usda"));
    assert!(source.open(None, "absent.usda", 64).is_err());
    assert!(source.open(None, "root.usda", 3).is_err());
}

#[test]
fn a_located_file_has_the_identity_that_opening_it_gives() {
    let directory = tempfile::tempdir().expect("directory");
    std::fs::write(directory.path().join("root.usda"), b"root").expect("root");
    std::fs::write(directory.path().join("texture.png"), b"png").expect("texture");
    let source = Directory::beside(&directory.path().join("root.usda"));
    let located = source.locate(None, "./texture.png").expect("located");
    let opened = source.open(None, "texture.png", 64).expect("opened");
    assert_eq!(located, opened.id);
    assert!(source.locate(None, "absent.png").is_err());
}

#[test]
fn a_percent_encoded_path_names_the_decoded_file() {
    let directory = tempfile::tempdir().expect("directory");
    let library = directory.path().join("Material Library");
    std::fs::create_dir(&library).expect("library");
    std::fs::write(directory.path().join("root.usda"), b"root").expect("root");
    std::fs::write(library.join("physics.usda"), b"physics").expect("layer");
    let source = Directory::beside(&directory.path().join("root.usda"));
    let layer = source
        .open(None, "./Material%20Library/physics.usda", 64)
        .expect("the decoded path");
    assert_eq!(layer.bytes, b"physics");
    assert!(source
        .open(None, "./Material%2Library/physics.usda", 64)
        .is_err());
}
