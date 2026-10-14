// SPDX-License-Identifier: Apache-2.0

#[test]
fn dimension_controls_preserve_sorted_serialization_and_replacement() {
    let mut entries = crate::presentation::DimensionControlEntries::default();
    entries
        .insert_with("z", || Ok(serde_json::json!(1)))
        .expect("first key admitted");
    entries
        .insert_with("a", || Ok(serde_json::json!(2)))
        .expect("second key admitted");
    entries
        .insert_with("z", || Ok(serde_json::json!(3)))
        .expect("duplicate key replaced");
    let controls = crate::presentation::DimensionStyleControls {
        controls: &entries,
        extra: None,
    };
    assert_eq!(
        serde_json::to_string(&controls).expect("control serialization"),
        "{\"a\":2,\"z\":3}"
    );
}
