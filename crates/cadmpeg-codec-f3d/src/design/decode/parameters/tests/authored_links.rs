// SPDX-License-Identifier: Apache-2.0
//! Newly authored owner and companion frames satisfy their source grammar.

use crate::records::parameters::{DesignParameterCompanion, DesignParameterOwner};

#[test]
fn authored_owner_and_companion_keep_semantic_links_without_source_locations() {
    for variant in [None, Some(0), Some(1)] {
        let mut owner_content = serde_json::json!({
            "id": "f3d:design:design-parameter-owner#1",
            "class_tag": "268",
            "record_index": 1,
            "scope_record_index": 40,
            "local_ordinal": 7,
            "evaluated_value": -2.5,
            "parameter_record_index": 2,
            "owned_ordinal": 3,
            "companion_record_index": 3
        });
        if let Some(variant) = variant {
            owner_content["variant"] = variant.into();
        }
        let owner: DesignParameterOwner = serde_json::from_value(owner_content).unwrap();
        let companion: DesignParameterCompanion = serde_json::from_value(serde_json::json!({
            "id": "f3d:design:design-parameter-companion#3",
            "class_tag": "269",
            "record_index": 3,
            "owner_record_index": 1,
            "timestamp_micros": 123456
        }))
        .unwrap();
        assert_eq!(owner.byte_offset(), None);
        assert_eq!(companion.byte_offset(), None);
        let mut bytes = Vec::new();
        crate::writer::generate::parameter_links::encode(
            &mut bytes,
            std::slice::from_ref(&owner),
            std::slice::from_ref(&companion),
        )
        .unwrap();
        let owner_length = if variant.is_some() { 104 } else { 103 };
        let actual = super::parse_parameter_owner(&bytes[..owner_length]).unwrap();
        assert_eq!(actual.record_index, owner.record_index());
        assert_eq!(
            actual.scope_record_index,
            owner.scope_record_index().unwrap()
        );
        assert_eq!(actual.local_ordinal, owner.local_ordinal().unwrap());
        assert_eq!(
            actual.parameter_record_index,
            owner.parameter_record_index()
        );
        assert_eq!(
            actual.companion_record_index,
            owner.companion_record_index()
        );
        assert_eq!(actual.owned_ordinal, owner.owned_ordinal());
        assert_eq!(actual.evaluated_value, owner.evaluated_value());
        assert_eq!(actual.variant, owner.variant());
        let companion_start = owner_length + 11;
        let actual = super::parse_parameter_companion(&bytes[companion_start..]).unwrap();
        assert_eq!(actual.class_tag, *companion.class_tag());
        assert_eq!(actual.record_index, companion.record_index());
        assert_eq!(actual.owner_record_index, companion.owner_record_index());
        assert_eq!(actual.timestamp_micros, companion.timestamp_micros());
    }
}

#[test]
fn authored_legacy_owner_preserves_absent_scope_and_local_slots() {
    for (tag, scope, length) in [("268", None, 68), ("284", Some(40), 88)] {
        let mut content = serde_json::json!({
            "id": "f3d:design:design-parameter-owner#1",
            "class_tag": tag,
            "record_index": 1,
            "evaluated_value": -2.5,
            "parameter_record_index": 2,
            "owned_ordinal": 3,
            "companion_record_index": 3
        });
        if let Some(scope) = scope {
            content["scope_record_index"] = scope.into();
        }
        let owner: DesignParameterOwner = serde_json::from_value(content).unwrap();
        // Legacy owners refer to the parameter's scalar outside their own
        // frame. Here that actual source scalar is the first eight bytes.
        let mut bytes = (-2.5_f64).to_le_bytes().to_vec();
        crate::writer::generate::parameter_links::encode(
            &mut bytes,
            std::slice::from_ref(&owner),
            &[],
        )
        .unwrap();
        let cache = crate::records::identity::Located {
            value: cadmpeg_core::decode::View::f64_le_at(&bytes, 0).unwrap(),
            offset: 0,
        };
        let frame = &bytes[8..8 + length];
        let actual = super::parse_legacy_parameter_owner_68(frame, cache, 8)
            .or_else(|| super::parse_legacy_parameter_owner_88(frame, cache, 8))
            .unwrap()
            .into_record("Design/BulkStream.dat", 8)
            .unwrap();
        assert_eq!(actual.scope_record_index(), scope);
        assert_eq!(actual.local_ordinal(), None);
        assert_eq!(actual.evaluated_value(), owner.evaluated_value());
        assert_eq!(
            actual.parameter_record_index(),
            owner.parameter_record_index()
        );
        assert_eq!(
            actual.companion_record_index(),
            owner.companion_record_index()
        );
        assert_eq!(actual.owned_ordinal(), owner.owned_ordinal());
    }
}
