// SPDX-License-Identifier: Apache-2.0
//! Sketch-profile operands and profile region members.

use serde_json::json;

#[test]
fn profile_region_member_preserves_fixed_words_and_closed_incidence_values() {
    let wire = |kind: u32, identity: u64, words: [u32; 8]| {
        format!("{{\"kind\":{kind},\"kind_offset\":40,\"curve_primary_id\":{identity},\"curve_primary_id_offset\":44,\"incidence_words\":{},\"incidence_words_offset\":48}}", serde_json::to_string(&words).expect("incidence words"))
    };
    for identity in [1, u64::from(u32::MAX)] {
        for flag in [0, 1] {
            for first in [1, 2] {
                for second in [1, 2] {
                    let json = wire(3, identity, [0, 0, 0, flag, first, second, 0, 0]);
                    let member: super::DesignSketchProfileRegionMember =
                        serde_json::from_str(&json).expect("region member");
                    assert_eq!(
                        serde_json::to_string(&member).expect("region member wire"),
                        json
                    );
                }
            }
        }
    }
    for kind in [0, 1, 2, 4, u32::MAX] {
        let error = serde_json::from_str::<super::DesignSketchProfileRegionMember>(&wire(
            kind,
            1,
            [0, 0, 0, 0, 1, 1, 0, 0],
        ))
        .expect_err("fixed kind");
        assert!(error.to_string().contains("kind"));
    }
    for identity in [0, u64::from(u32::MAX) + 1, u64::MAX] {
        let error = serde_json::from_str::<super::DesignSketchProfileRegionMember>(&wire(
            3,
            identity,
            [0, 0, 0, 0, 1, 1, 0, 0],
        ))
        .expect_err("nonzero u32 identity");
        assert!(error.to_string().contains("curve_primary_id"));
    }
    for index in 0..8 {
        let mut words = [0, 0, 0, 0, 1, 1, 0, 0];
        words[index] = 3;
        let error =
            serde_json::from_str::<super::DesignSketchProfileRegionMember>(&wire(3, 1, words))
                .expect_err("invalid incidence word");
        assert!(error.to_string().contains("incidence_words"));
    }
}

#[test]
fn sketch_profile_offsets_must_follow_their_predecessors() {
    use super::DesignSketchProfileOperand;
    let wire = json!({
        "scope_reference_ordinal": 0, "record_index": 7, "byte_offset": 0,
        "class_tag": "300", "paired_class_tag": "301", "paired_byte_offset": 160,
        "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d", "asset_id_offset": 32,
        "entity_id": "Sketch_1", "entity_suffix": 1, "entity_reference_offset": 80
    });
    let admitted: DesignSketchProfileOperand = serde_json::from_value(wire.clone()).unwrap();
    let mut draft = admitted.into_draft();
    draft.byte_offset = draft.asset_id_offset;
    assert!(DesignSketchProfileOperand::try_new(draft).is_err());
    for (field, preceding) in [
        ("asset_id_offset", 0),
        ("entity_reference_offset", 32),
        ("paired_byte_offset", 80),
    ] {
        let mut invalid = wire.clone();
        invalid[field] = preceding.into();
        assert!(serde_json::from_value::<DesignSketchProfileOperand>(invalid).is_err());
    }
}
