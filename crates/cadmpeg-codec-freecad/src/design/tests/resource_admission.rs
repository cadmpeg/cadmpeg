// SPDX-License-Identifier: Apache-2.0
//! Design feature and spreadsheet admission tests.

#[test]
fn design_boolean_scalar_keeps_case_insensitive_values_without_copy() {
    for (value, expected) in [("TrUe", true), ("FaLsE", false)] {
        let property = crate::test_support::source_fidelity::PropertyFixture::new(
            crate::native::PropertyRecord {
                dynamic: None,
                id: "flag".into(),
                owner: "owner".into(),
                name: "Flag".into(),
                type_name: "App::PropertyBool".into(),
                family: crate::native::PropertyFamily::Unknown,
                status: None,
                body: Some(crate::native::PropertyBody::Transient),
                order: 0,
            },
            format!("<Property><Bool value=\"{value}\"/></Property>"),
        );
        crate::test_support::source_fidelity::with_property_reader(
            &[],
            &[&property],
            &cadmpeg_core::decode::DecodePolicy::service(),
            |ctx| {
                assert_eq!(
                    super::super::bool_property(ctx, &[&property], "Flag").expect("admitted XML"),
                    Some(expected)
                );
            },
        );
    }
}
