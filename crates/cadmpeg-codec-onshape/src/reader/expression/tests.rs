// SPDX-License-Identifier: Apache-2.0

#[test]
fn variable_arithmetic_refuses_forward_references_wrong_units_and_excessive_nesting() {
    use super::{evaluate, Dimension};
    let model = cadmpeg_ir::document::Model::default();
    for source in ["#missing", "1 mm + 2 deg", "1 mm / 0", "sin(2 mm)"] {
        assert!(
            evaluate(
                source,
                Dimension::Length,
                &model,
                &std::collections::BTreeSet::new()
            )
            .is_err(),
            "{source}"
        );
    }
    let nested = format!("{}1 mm{}", "(".repeat(100), ")".repeat(100));
    assert!(evaluate(
        &nested,
        Dimension::Length,
        &model,
        &std::collections::BTreeSet::new()
    )
    .is_err());
    assert_eq!(
        evaluate(
            "2 * (3 mm + 1 cm)",
            Dimension::Length,
            &model,
            &std::collections::BTreeSet::new()
        )
        .unwrap()
        .value,
        26.0
    );
}
