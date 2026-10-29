// SPDX-License-Identifier: Apache-2.0
use crate::test_support::test_curves_and_surfaces::point_file;

#[test]
fn public_fixed_ascii_decode_installs_global_value_captures_after_all_source_callbacks() {
    let decoded = crate::test_support::decode(point_file());
    assert_eq!(decoded.ir().model.points.len(), 1);
    let source = decoded.ir().source.as_ref().unwrap();
    assert_eq!(
        source.attributes.get("sender_product").map(String::as_str),
        Some("product")
    );
    assert_eq!(
        source
            .attributes
            .get("native_file_name")
            .map(String::as_str),
        Some("part.igs")
    );
    assert_eq!(
        source.attributes.get("native_units").map(String::as_str),
        Some("MM")
    );
}
