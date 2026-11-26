// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::combine::{
    DesignCombineExternalBodyIdentity, DesignCombineExternalBodyIdentityWire,
};
use cadmpeg_core::serde_erase::json;

fn external_wire(version: bool) -> serde_json::Value {
    let mut wire = cadmpeg_core::json!({
        "selector_asset_id":"11111111-1111-4111-8111-111111111111", "selector_asset_id_offset":44,
        "selector_context_id":"22222222-2222-4222-8222-222222222222", "selector_context_id_offset":120,
        "occurrence_reference":1, "occurrence_reference_offset":205,
        "external_body_reference":2, "external_body_reference_offset":220,
        "external_segment":3, "external_segment_offset":229,
        "external_asset_id":"11111111-1111-4111-8111-111111111111", "external_asset_id_offset":237,
        "external_link_name":"identity", "external_link_name_offset":314,
        "tail_values":[0,0], "tail_value_offsets":[337,349]
    });
    if version {
        wire["external_property_key"] = cadmpeg_core::json!("33333333-3333-4333-8333-333333333333");
        wire["external_property_key_offset"] = cadmpeg_core::json!(335);
        wire["external_version_urn"] = cadmpeg_core::json!("urn");
        wire["external_version_urn_offset"] = cadmpeg_core::json!(411);
        wire["tail_value_offsets"] = cadmpeg_core::json!([423, 435]);
    }
    wire
}

#[test]
fn external_combine_identity_admits_only_complete_offset_chains() {
    for version in [false, true] {
        let wire = external_wire(version);
        let admitted: DesignCombineExternalBodyIdentity = json::from_value(wire.clone()).unwrap();
        assert_eq!(json::to_value(admitted).unwrap(), wire);
        for field in [
            "selector_asset_id_offset",
            "selector_context_id_offset",
            "occurrence_reference_offset",
            "external_body_reference_offset",
            "external_segment_offset",
            "external_asset_id_offset",
            "external_link_name_offset",
            "external_property_key_offset",
            "external_version_urn_offset",
        ] {
            if let Some(offset) = wire.get(field).and_then(serde_json::Value::as_u64) {
                for invalid_offset in [offset + 1, u64::MAX] {
                    let mut invalid = wire.clone();
                    invalid[field] = cadmpeg_core::json!(invalid_offset);
                    let raw: DesignCombineExternalBodyIdentityWire =
                        json::from_value(invalid.clone()).unwrap();
                    assert!(
                        DesignCombineExternalBodyIdentity::try_from(raw).is_err(),
                        "{field}"
                    );
                    assert!(
                        json::from_value::<DesignCombineExternalBodyIdentity>(invalid).is_err(),
                        "{field}"
                    );
                }
            }
        }
        for lane in 0..2 {
            let mut invalid = wire.clone();
            invalid["tail_value_offsets"][lane] = cadmpeg_core::json!(0);
            assert!(json::from_value::<DesignCombineExternalBodyIdentity>(invalid).is_err());
        }
    }
}

#[test]
fn external_combine_identity_rejects_mismatched_assets_and_empty_content() {
    for (field, value) in [
        (
            "external_asset_id",
            cadmpeg_core::json!("22222222-2222-4222-8222-222222222222"),
        ),
        ("occurrence_reference", cadmpeg_core::json!(0)),
        ("external_body_reference", cadmpeg_core::json!(0)),
        ("external_link_name", cadmpeg_core::json!("")),
        ("external_version_urn", cadmpeg_core::json!("")),
    ] {
        let mut wire = external_wire(true);
        wire[field] = value;
        assert!(
            json::from_value::<DesignCombineExternalBodyIdentity>(wire).is_err(),
            "{field}"
        );
    }
}
