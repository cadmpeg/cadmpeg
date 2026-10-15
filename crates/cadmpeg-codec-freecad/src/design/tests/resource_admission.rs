// SPDX-License-Identifier: Apache-2.0
//! Design feature and spreadsheet admission tests.

#[test]
fn design_boolean_scalar_keeps_case_insensitive_values_without_copy() {
    for (value, expected) in [("TrUe", true), ("FaLsE", false)] {
        let property = crate::native::PropertyRecord {
            id: "flag".into(),
            owner: "owner".into(),
            name: "Flag".into(),
            type_name: "App::PropertyBool".into(),
            family: crate::native::PropertyFamily::Unknown,
            status: None,
            body: crate::native::PropertyBody::Transient,
            order: 0,
            xml: crate::native::RetainedXml::from_text(
                format!("<Property><Bool value=\"{value}\"/></Property>"),
                0,
            )
            .expect("valid XML span"),
        };
        assert_eq!(
            super::super::bool_property(
                &cadmpeg_test_support::service_decode_context(),
                &[&property],
                "Flag"
            )
            .expect("admitted XML"),
            Some(expected)
        );
    }
}
