// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::serde_erase::json;

#[test]
fn dimension_controls_preserve_sorted_serialization_and_replacement() {
    let mut entries = crate::presentation::DimensionControlEntries::default();
    entries
        .insert_with("z", || Ok(cadmpeg_core::json!(1)))
        .expect("first key admitted");
    entries
        .insert_with("a", || Ok(cadmpeg_core::json!(2)))
        .expect("second key admitted");
    entries
        .insert_with("z", || Ok(cadmpeg_core::json!(3)))
        .expect("duplicate key replaced");
    let controls = crate::presentation::DimensionStyleControls {
        controls: &entries,
        extra: None,
    };
    assert_eq!(
        json::to_string(&controls).expect("control serialization"),
        "{\"a\":2,\"z\":3}"
    );
}
