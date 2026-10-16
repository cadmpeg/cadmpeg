// SPDX-License-Identifier: Apache-2.0
fn encoded_scope() -> crate::records::feature::scope::DesignParameterScope {
    crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:asset/a %\u{2003}ç:parameter-scope#42",
        crate::records::feature::scope::DesignFeatureKind::try_from("Native:#% \u{a0}ç".to_owned())
            .unwrap(),
        42,
    )
}

#[test]
fn neutral_feature_id_preserves_bytes() {
    let scope = encoded_scope();
    assert_eq!(
        super::neutral_feature_id(&scope).unwrap(),
        crate::ids::neutral_feature_id(&scope)
    );
}
