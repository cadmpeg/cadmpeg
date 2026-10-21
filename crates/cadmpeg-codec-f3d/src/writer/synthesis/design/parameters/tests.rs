// SPDX-License-Identifier: Apache-2.0
//! Missing evaluated caches use current literals, never stale cache properties.

use cadmpeg_ir::features::{DesignParameter, Expression};

fn parameter(expression: &str) -> DesignParameter {
    DesignParameter {
        id: "authored:model:parameter#width".try_into().unwrap(),
        owner: None,
        ordinal: 0,
        name: "width".into(),
        expression: Expression::text(expression),
        driving: true,
        value: None,
        display: None,
        pmi: None,
        properties: [
            (cadmpeg_core::nonblank_literal!("unit"), "cm".into()),
            (
                cadmpeg_core::nonblank_literal!("evaluated_scalar"),
                "1.2".into(),
            ),
        ]
        .into(),
        native_ref: None,
    }
}

#[test]
fn absent_cache_literal_uses_current_expression_and_canonical_unit_conversion() {
    let parameter = parameter("24 mm");
    let (unit, value) = super::scalar(&parameter, Some("cm")).unwrap();
    assert_eq!(unit.as_deref(), Some("cm"));
    assert_eq!(value, 2.4);
    assert!(parameter.value.is_none());
    assert_eq!(parameter.properties["evaluated_scalar"], "1.2");
}

#[test]
fn absent_cache_nonliteral_never_reads_stale_evaluated_scalar() {
    for text in ["other + 1 mm", "sin(2)", "not-a-number"] {
        let parameter = parameter(text);
        assert!(super::scalar(&parameter, Some("cm")).is_none());
        assert!(parameter.value.is_none());
    }
}
