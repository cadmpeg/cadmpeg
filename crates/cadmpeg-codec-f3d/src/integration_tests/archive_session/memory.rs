// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn archive_session_deflated_members_preserve_decode_and_composition_equivalence() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let leaf = crate::test_support::zip_test::f3d_with_deflated_smbh(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
    );
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("leaf.f3d", XREF_ROLE)]);
    let bytes = crate::test_support::archive_session::deflated_archive(
        "root.f3d",
        &[
            ("root.f3d", &root),
            ("leaf.f3d", &leaf),
            ("sheet.f2d", b"drawing"),
        ],
    );
    let options = DecodeOptions::default();
    let expected = F3dCodec.decode(&mut std::io::Cursor::new(&bytes), &options);
    let composed =
        crate::test_support::archive_session::decode(&mut std::io::Cursor::new(&bytes), &options);
    crate::test_support::archive_session::assert_equal(&expected, &composed);
}
