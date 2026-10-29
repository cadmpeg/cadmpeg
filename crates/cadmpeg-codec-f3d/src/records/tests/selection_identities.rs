// SPDX-License-Identifier: Apache-2.0
//! Secondary identities of persistent selections.

#[test]
fn selection_secondary_identities_preserve_wire_and_reject_partial_locations() {
    entity_selection_secondary_identities();
    let fields = r#""record_index":2,"class_tag":"365","asset_id":"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d","context_id":"1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e","identity_record_index":5,"primary_identity":183"#;
    let suffix = r#","next_record_index":6}"#;
    for prefix in ["{"] {
        for identities in [
            "",
            ",\"secondary_identity\":249",
            ",\"secondary_identity\":249,\"curve_secondary_identity\":77",
        ] {
            let wire = format!("{prefix}{fields}{identities}{suffix}");
            let encoded = if prefix == "{" {
                let value: crate::records::feature::hole::DesignHoleFaceSelection =
                    serde_json::from_str(&wire).expect("hole selection");
                let encoded = serde_json::to_string(&value).expect("hole selection wire");
                let mut pointers = vec![
                    ("byte_offset", 0),
                    ("asset_id_offset", 100),
                    ("context_id_offset", 150),
                    ("identity_record_offset", 180),
                    ("primary_identity_offset", 201),
                    ("next_byte_offset", 209),
                ];
                if identities.contains("secondary_identity") {
                    pointers.push(("secondary_identity_offset", 217));
                }
                if identities.contains("curve_secondary_identity") {
                    pointers.push(("curve_secondary_identity_offset", 225));
                }
                encoded
            } else {
                let value: crate::records::topology::entity_selection::DesignEntitySelectionOperand = serde_json::from_str(&wire).expect("entity selection");
                serde_json::to_string(&value).expect("entity selection wire")
            };
            assert_eq!(encoded, wire);
        }
        for field in ["curve_secondary_identity"] {
            let wire = format!("{prefix}{fields},\"{field}\":1{suffix}");
            let error = if prefix == "{" {
                serde_json::from_str::<crate::records::feature::hole::DesignHoleFaceSelection>(
                    &wire,
                )
                .expect_err("partial hole selection identity")
                .to_string()
            } else {
                serde_json::from_str::<
                    crate::records::topology::entity_selection::DesignEntitySelectionOperand,
                >(&wire)
                .expect_err("partial entity selection identity")
                .to_string()
            };
            assert!(error.contains(field));
        }
        let wire = format!("{prefix}{fields},\"curve_secondary_identity\":77{suffix}");
        let error = if prefix == "{" {
            serde_json::from_str::<crate::records::feature::hole::DesignHoleFaceSelection>(&wire)
                .unwrap_err()
                .to_string()
        } else {
            serde_json::from_str::<
                crate::records::topology::entity_selection::DesignEntitySelectionOperand,
            >(&wire)
            .unwrap_err()
            .to_string()
        };
        assert!(error.contains("secondary_identity"));
        assert!(error.contains("curve_secondary_identity"));
        for guid in [
            "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d",
            "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e",
        ] {
            let wire = format!("{prefix}{fields}{suffix}").replace(guid, "not-a-guid");
            let error = if prefix == "{" {
                serde_json::from_str::<crate::records::feature::hole::DesignHoleFaceSelection>(
                    &wire,
                )
                .expect_err("non-GUID hole selection identity")
                .to_string()
            } else {
                serde_json::from_str::<
                    crate::records::topology::entity_selection::DesignEntitySelectionOperand,
                >(&wire)
                .expect_err("non-GUID entity selection identity")
                .to_string()
            };
            assert!(error.contains("GUID"), "{error}");
        }
    }
}

/// The entity-selection operand, whose wire carries its source offsets.
fn entity_selection_secondary_identities() {
    let fields = r#""record_index":2,"byte_offset":0,"class_tag":"365","asset_id":"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d","asset_id_offset":100,"context_id":"1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e","context_id_offset":150,"identity_record_index":5,"identity_record_offset":180,"primary_identity":183,"primary_identity_offset":209"#;
    let suffix = r#","next_record_index":6,"next_byte_offset":225}"#;
    for prefix in [
        "{\"id\":\"operand\",\"scope_record_index\":1,\"group_record_index\":2,\"group_member_ordinal\":0,",
    ] {
        for identities in [
            "",
            ",\"secondary_identity\":249,\"secondary_identity_offset\":217",
            ",\"secondary_identity\":249,\"secondary_identity_offset\":217,\"curve_secondary_identity\":77,\"curve_secondary_identity_offset\":201",
        ] {
            let wire = format!("{prefix}{fields}{identities}{suffix}");
            let wire = if identities.is_empty() {
                wire.replace(
                    "\"primary_identity_offset\":209",
                    "\"primary_identity_offset\":201",
                )
                .replace("\"next_byte_offset\":225", "\"next_byte_offset\":209")
            } else {
                wire
            };
            let encoded = if prefix == "{" {
                let value: crate::records::feature::hole::DesignHoleFaceSelection =
                    serde_json::from_str(&wire).expect("hole selection");
                serde_json::to_string(&value).expect("hole selection wire")
            } else {
                let value: crate::records::topology::entity_selection::DesignEntitySelectionOperand = serde_json::from_str(&wire).expect("entity selection");
                serde_json::to_string(&value).expect("entity selection wire")
            };
            assert_eq!(encoded, wire);
        }
        for field in [
            "secondary_identity",
            "secondary_identity_offset",
            "curve_secondary_identity",
            "curve_secondary_identity_offset",
        ] {
            let wire = format!("{prefix}{fields},\"{field}\":1{suffix}");
            let error = if prefix == "{" {
                serde_json::from_str::<crate::records::feature::hole::DesignHoleFaceSelection>(
                    &wire,
                )
                .expect_err("partial hole selection identity")
                .to_string()
            } else {
                serde_json::from_str::<
                    crate::records::topology::entity_selection::DesignEntitySelectionOperand,
                >(&wire)
                .expect_err("partial entity selection identity")
                .to_string()
            };
            assert!(error.contains(field));
        }
        let wire = format!(
            "{prefix}{fields},\"curve_secondary_identity\":77,\"curve_secondary_identity_offset\":201{suffix}"
        );
        let error = if prefix == "{" {
            serde_json::from_str::<crate::records::feature::hole::DesignHoleFaceSelection>(&wire)
                .unwrap_err()
                .to_string()
        } else {
            serde_json::from_str::<
                crate::records::topology::entity_selection::DesignEntitySelectionOperand,
            >(&wire)
            .unwrap_err()
            .to_string()
        };
        assert!(error.contains("secondary_identity"));
        assert!(error.contains("curve_secondary_identity"));
        for guid in [
            "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d",
            "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e",
        ] {
            let wire = format!("{prefix}{fields}{suffix}").replace(guid, "not-a-guid");
            let error = if prefix == "{" {
                serde_json::from_str::<crate::records::feature::hole::DesignHoleFaceSelection>(
                    &wire,
                )
                .expect_err("non-GUID hole selection identity")
                .to_string()
            } else {
                serde_json::from_str::<
                    crate::records::topology::entity_selection::DesignEntitySelectionOperand,
                >(&wire)
                .expect_err("non-GUID entity selection identity")
                .to_string()
            };
            assert!(error.contains("GUID"), "{error}");
        }
    }
}
