// SPDX-License-Identifier: Apache-2.0

use super::{detect, is_layer_name, member_name, open, package, DATA_ALIGNMENT};
use crate::test_support::with_root;

const LAYER: &[u8] = b"#usda 1.0\n";

#[test]
fn a_package_stores_its_root_layer_aligned() {
    for name in ["a.usda", "root.usda", "nested/directory/layer.usda"] {
        let bytes = package(&[(name, LAYER)]).expect("package");
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
    let bytes = package(&[("texture.png", LAYER)]).expect("package");
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
    let bytes = package(&[("root.usda", LAYER), ("textures/a.png", image)]).expect("package");
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
