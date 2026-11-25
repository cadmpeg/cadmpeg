// SPDX-License-Identifier: Apache-2.0

use super::{detect, is_layer_name, member_name, open, package, DATA_ALIGNMENT};
use crate::test_support::with_root;

const LAYER: &[u8] = b"#usda 1.0\n";

#[test]
fn a_package_stores_its_root_layer_aligned() {
    for name in ["a.usda", "root.usda", "nested/directory/layer.usda"] {
        let bytes = package(&[(name, LAYER)]).0;
        with_root(&bytes, |ctx, root| {
            assert!(detect(ctx, root.window()).expect("detect"));
            let package = open(ctx, root).expect("open");
            assert_eq!(package.root_name, name);
            assert_eq!(package.root.window(), LAYER);
            assert_eq!(package.root.start() % DATA_ALIGNMENT, 0);
        });
    }
}

#[test]
fn a_package_whose_first_member_is_no_layer_is_refused() {
    let bytes = package(&[("texture.png", LAYER)]).0;
    with_root(&bytes, |ctx, root| {
        assert!(!detect(ctx, root.window()).expect("detect"));
        assert!(open(ctx, root).is_err());
    });
}

#[test]
fn layer_names_have_a_layer_extension() {
    assert!(is_layer_name("a.usdc"));
    assert!(is_layer_name("A.USD"));
    assert!(!is_layer_name("a.usdz"));
}

#[test]
fn relative_assets_name_members() {
    assert_eq!(
        member_name("root.usda", "./parts/a.usdc").as_deref(),
        Some("parts/a.usdc")
    );
    assert_eq!(
        member_name("dir/root.usdc", "../tex.png").as_deref(),
        Some("tex.png")
    );
    assert_eq!(member_name("root.usda", "../outside.usd"), None);
    assert_eq!(member_name("root.usda", "/abs.usd"), None);
    assert_eq!(member_name("root.usda", "http://host/a.usd"), None);
}

#[test]
fn every_member_of_a_package_is_stored_aligned() {
    let image: &[u8] = b"\x89PNG image";
    let bytes = package(&[("root.usda", LAYER), ("textures/a.png", image)]).0;
    with_root(&bytes, |ctx, root| {
        let package = open(ctx, root).expect("open");
        assert_eq!(package.root.window(), LAYER);
        let member = package
            .snapshot
            .open(ctx, "textures/a.png")
            .expect("member");
        assert_eq!(member.window(), image);
        assert_eq!(member.start() % DATA_ALIGNMENT, 0);
    });
}

/// A member whose name is longer than a ZIP name field states is left out
/// and named, and the members around it are stored.
#[test]
fn a_member_past_the_archive_limits_is_left_out() {
    let image: &[u8] = b"\x89PNG image";
    let long = format!("textures/{}.png", "a".repeat(usize::from(u16::MAX)));
    let (bytes, left_out) = package(&[
        ("root.usda", LAYER),
        (&long, image),
        ("textures/b.png", image),
    ]);
    assert_eq!(left_out, [long]);
    with_root(&bytes, |ctx, root| {
        let package = open(ctx, root).expect("open");
        assert_eq!(package.root.window(), LAYER);
        let member = package
            .snapshot
            .open(ctx, "textures/b.png")
            .expect("member");
        assert_eq!(member.window(), image);
        assert_eq!(member.start() % DATA_ALIGNMENT, 0);
    });
}

/// A root layer past the archive limits is not left out: the package holds
/// the empty layer as its root, and `export:output.empty` names the root
/// layer's size.
#[test]
fn a_root_layer_past_the_archive_limits_leaves_an_empty_package() {
    let root = "#usda 1.0\ndef Xform \"Root\"\n{\n    def Mesh \"Plate\"\n    {\n    }\n}\n";
    let limit = super::EMPTY_LAYER.len();
    let (bytes, losses) = super::package_layer_by(
        root.as_bytes(),
        &[("textures/a.png".to_owned(), b"\x89PNG image".to_vec())],
        |name, length, end, central, count| {
            (name != super::ROOT_MEMBER || length <= limit)
                .then(|| super::admit(name, length, end, central, count))
                .flatten()
        },
    );
    let [note] = losses.as_slice() else {
        panic!("one loss: {losses:#?}");
    };
    assert_eq!(note.code.to_string(), "export/output.empty");
    assert!(
        note.message.contains(&root.len().to_string()),
        "{}",
        note.message
    );
    with_root(&bytes, |ctx, archive| {
        let package = open(ctx, archive).expect("open");
        assert_eq!(package.root_name, super::ROOT_MEMBER);
        assert_eq!(package.root.window(), super::EMPTY_LAYER);
    });
    let decoded = crate::test_support::decode_bytes(&bytes);
    assert!(decoded.ir().model.product_definitions.len() <= 1);
    assert!(decoded.ir().model.tessellations.is_empty());
}

/// A texture member past the archive limits is left out with the asset path
/// that names it; the other member and its asset path are kept.
#[test]
fn a_texture_member_left_out_takes_its_asset_path() {
    let long = format!("textures/{}.png", "a".repeat(usize::from(u16::MAX)));
    let image = b"\x89PNG image".to_vec();
    let root = format!(
        "#usda 1.0\ndef Shader \"Long\"\n{{\n    asset inputs:file = @{long}@ (\n        colorSpace = \"sRGB\"\n    )\n    token inputs:wrapS = \"repeat\"\n}}\ndef Shader \"Short\"\n{{\n    asset inputs:file = @textures/b.png@\n}}\n"
    );
    let (bytes, losses) = super::package_layer(
        root.as_bytes(),
        &[
            (long.clone(), image.clone()),
            ("textures/b.png".to_owned(), image.clone()),
        ],
    );
    let [note] = losses.as_slice() else {
        panic!("one loss: {losses:#?}");
    };
    assert_eq!(note.code.to_string(), "export/value.unwritable");
    assert!(note.message.contains(&long));
    with_root(&bytes, |ctx, archive| {
        let package = open(ctx, archive).expect("open");
        assert_eq!(
            package.root.window(),
            "#usda 1.0\ndef Shader \"Long\"\n{\n    token inputs:wrapS = \"repeat\"\n}\ndef Shader \"Short\"\n{\n    asset inputs:file = @textures/b.png@\n}\n"
                .as_bytes()
        );
        let member = package
            .snapshot
            .open(ctx, "textures/b.png")
            .expect("member");
        assert_eq!(member.window(), image.as_slice());
    });
    crate::test_support::decode_bytes(&bytes);
}
