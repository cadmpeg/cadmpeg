// SPDX-License-Identifier: Apache-2.0

#[test]
fn tracking_identities_preserve_wire_and_reject_partial_locations() {
    let prefix = r#"{"wrapper_record_index":300,"wrapper_byte_offset":0,"wrapper_class_tag":"361","carrier_record_index":301,"carrier_byte_offset":33,"carrier_class_tag":"362","primary_identity":268,"primary_identity_offset":70,"selector":-1,"selector_offset":90,"kind":3,"kind_offset":94"#;
    let suffix =
        r#","following_record_index":302,"following_byte_offset":130,"following_class_tag":"363"}"#;
    for fields in ["", ",\"first_related_identity\":113,\"first_related_identity_offset\":110,\"second_related_identity\":119,\"second_related_identity_offset\":122"] {
        let wire = format!("{prefix}{fields}{suffix}");
        let value: crate::records::topology::DesignConstructionTrackingPath = serde_json::from_str(&wire).expect("tracking path");
        assert_eq!(serde_json::to_string(&value).expect("tracking wire"), wire);
    }
    for field in [
        "first_related_identity",
        "first_related_identity_offset",
        "second_related_identity",
        "second_related_identity_offset",
    ] {
        let error =
            serde_json::from_str::<crate::records::topology::DesignConstructionTrackingPath>(
                &format!("{prefix},\"{field}\":1{suffix}"),
            )
            .expect_err("partial identity location");
        assert!(error.to_string().contains(field));
    }
}

#[test]
fn body_recipe_selector_tail_preserves_wire_and_rejects_partial_locations() {
    let prefix = r#"{"id":"operand","scope_record_index":1,"scope_reference_ordinal":0,"record_index":2,"byte_offset":0,"class_tag":"365","asset_id":"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d","asset_id_offset":100,"context_id":"1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e","context_id_offset":150"#;
    let suffix = r#","references":[],"nested_record_index":5,"nested_record_index_offset":80,"recipe_id":"recipe","next_record_index":6,"next_byte_offset":240}"#;
    for fields in [
        "",
        ",\"selector_tail\":[7,0,0,0],\"selector_tail_offset\":220",
    ] {
        let wire = format!("{prefix}{fields}{suffix}");
        let value: crate::records::topology::DesignBodyRecipeOperand =
            serde_json::from_str(&wire).expect("body recipe operand");
        assert_eq!(
            serde_json::to_string(&value).expect("body recipe operand wire"),
            wire
        );
    }
    for fields in [
        ",\"selector_tail\":[7,0,0,0]",
        ",\"selector_tail_offset\":220",
    ] {
        let error = serde_json::from_str::<crate::records::topology::DesignBodyRecipeOperand>(
            &format!("{prefix}{fields}{suffix}"),
        )
        .expect_err("partial selector tail location");
        assert!(error.to_string().contains("selector_tail"));
    }
    for guid in [
        "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d",
        "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e",
    ] {
        let error = serde_json::from_str::<crate::records::topology::DesignBodyRecipeOperand>(
            &format!("{prefix}{suffix}").replace(guid, "not-a-guid"),
        )
        .expect_err("non-GUID body recipe identity")
        .to_string();
        assert!(error.contains("GUID"), "{error}");
    }
}

#[test]
fn loft_trailing_scope_reference_preserves_wire_and_rejects_partial_locations() {
    let prefix = r#"{"id":"carrier","scope_record_index":12,"scope_reference_ordinal":0,"record_index":20,"byte_offset":0,"class_tag":"322","owner_scope_record_index":12,"owner_scope_record_index_offset":20,"members":[22],"member_offsets":[30],"member_count":1,"member_count_offset":26,"opaque_index":1,"opaque_index_offset":34,"opaque_scalar":1.0,"opaque_scalar_offset":38,"repeated_opaque_index":1,"repeated_opaque_index_offset":46,"next_next_record_index":22,"next_next_reference_offset":50,"flags":[0,0],"flags_offset":59,"next_record_index":21,"next_reference_offset":61"#;
    let suffix = r#","paired_class_tag":"262","paired_byte_offset":98}"#;
    for fields in [
        "",
        ",\"trailing_scope_record_index\":12,\"trailing_scope_reference_offset\":88",
    ] {
        let wire = format!("{prefix}{fields}{suffix}");
        let value: crate::records::topology::DesignLoftLegacyBodyCarrier =
            serde_json::from_str(&wire).expect("loft carrier");
        assert_eq!(
            serde_json::to_string(&value).expect("loft carrier wire"),
            wire
        );
    }
    let error = serde_json::from_str::<crate::records::topology::DesignLoftLegacyBodyCarrier>(
        &format!("{prefix},\"trailing_scope_record_index\":13,\"trailing_scope_reference_offset\":88{suffix}"),
    ).expect_err("conflicting owning scope");
    assert!(error.to_string().contains("trailing_scope_record_index"));
    for field in [
        "trailing_scope_record_index",
        "trailing_scope_reference_offset",
    ] {
        let error = serde_json::from_str::<crate::records::topology::DesignLoftLegacyBodyCarrier>(
            &format!("{prefix},\"{field}\":12{suffix}"),
        )
        .expect_err("partial loft scope reference");
        assert!(error.to_string().contains(field));
    }
    let base: serde_json::Value = serde_json::from_str(&format!("{prefix}{suffix}")).unwrap();
    for (field, value) in [
        ("owner_scope_record_index", serde_json::json!(13)),
        ("repeated_opaque_index", serde_json::json!(2)),
        ("flags", serde_json::json!([0, 1])),
    ] {
        let mut invalid = base.clone();
        invalid[field] = value;
        assert!(
            serde_json::from_value::<crate::records::topology::DesignLoftLegacyBodyCarrier>(
                invalid
            )
            .expect_err("invalid derived field")
            .to_string()
            .contains(field)
        );
    }
    for ordinal in [0, 255, 256, u32::MAX] {
        let mut wire = base.clone();
        wire["opaque_index"] = ordinal.into();
        wire["repeated_opaque_index"] = ordinal.into();
        let parsed = serde_json::from_value::<crate::records::topology::DesignLoftLegacyBodyCarrier>(
            wire.clone(),
        );
        if ordinal == 255 {
            assert_eq!(
                serde_json::to_value(parsed.expect("maximum ordinal")).unwrap(),
                wire
            );
        } else {
            assert!(parsed
                .expect_err("invalid ordinal")
                .to_string()
                .contains("opaque_index"));
        }
    }
}

#[test]
// Fixture fields are appended from the bounded table of explicit test cases.
#[allow(clippy::format_push_string)]
fn construction_path_preserves_layout_wire_and_rejects_mixed_forms() {
    let prefix = r#"{"record_index":100,"byte_offset":0,"class_tag":"304","entity_ref":174,"entity_ref_offset":22"#;
    let suffix = r#","scope_record_index":90,"scope_record_index_offset":163,"nested_record_index":102,"nested_record_index_offset":174,"following_record_index":101,"following_byte_offset":190,"following_class_tag":"390"}"#;
    let fields = [
        (
            "transform",
            "[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]]",
        ),
        ("transform_offset", "33"),
        ("compact_variant", "false"),
    ];
    for mask in 0..8 {
        let mut wire = prefix.to_owned();
        for (index, (field, value)) in fields.iter().enumerate() {
            if mask & (1 << index) != 0 {
                wire.push_str(&format!(",\"{field}\":{value}"));
            }
        }
        wire.push_str(suffix);
        let result =
            serde_json::from_str::<crate::records::topology::DesignConstructionOperandPath>(&wire);
        if mask == 3 || mask == 4 {
            assert_eq!(
                serde_json::to_string(&result.expect("complete placement form"))
                    .expect("path wire"),
                wire
            );
        } else {
            let error = result.expect_err("invalid placement form").to_string();
            for (field, _) in fields {
                assert!(error.contains(field));
            }
        }
    }
}

#[test]
fn identity_wrapper_rows_preserve_wire_and_reject_unequal_arrays() {
    for count in 0..=2 {
        for offsets in 0..=2 {
            for tags in 0..=2 {
                let indices = ["[]", "[300]", "[300,305]"][count];
                let offsets_wire = ["[]", "[0]", "[0,24]"][offsets];
                let tags_wire = ["[]", "[\"384\"]", "[\"384\",\"289\"]"][tags];
                let wire = format!(
                    r#"{{"id":"identity#0","group_record_index":200,"wrapper_record_indices":{indices},"wrapper_byte_offsets":{offsets_wire},"wrapper_class_tags":{tags_wire},"following_record_index":310,"following_byte_offset":48,"following_class_tag":"304"}}"#
                );
                let parsed = serde_json::from_str::<
                    crate::records::topology::DesignConstructionOperandIdentity,
                >(&wire);
                if count == offsets && count == tags {
                    assert_eq!(
                        serde_json::to_string(&parsed.expect("complete rows"))
                            .expect("identity wire"),
                        wire
                    );
                } else {
                    let error = parsed.expect_err("unequal wrapper arrays").to_string();
                    assert!(error.contains("wrapper_record_indices"));
                    assert!(error.contains("wrapper_byte_offsets"));
                    assert!(error.contains("wrapper_class_tags"));
                }
            }
        }
    }
}

#[test]
fn extrude_selection_group_members_preserve_wire_and_reject_unequal_offsets() {
    let wire = r#"{"id":"group","scope_record_index":7,"scope_reference_ordinal":0,"record_index":9,"byte_offset":0,"class_tag":"277","member_count_offset":32,"members":[10,11],"member_offsets":[37,48],"opaque_index":1,"opaque_index_offset":58,"opaque_scalar":0.0,"opaque_scalar_offset":62,"variant":false,"paired_class_tag":"259","paired_byte_offset":111}"#;
    let group: crate::records::topology::DesignExtrudeSelectionGroup =
        serde_json::from_str(wire).expect("selection group");
    assert_eq!(serde_json::to_string(&group).expect("selection wire"), wire);
    for offsets in ["[]", "[37]", "[37,48,59]"] {
        let invalid = wire.replace(
            "\"member_offsets\":[37,48]",
            &format!("\"member_offsets\":{offsets}"),
        );
        let error =
            serde_json::from_str::<crate::records::topology::DesignExtrudeSelectionGroup>(&invalid)
                .expect_err("unequal member arrays")
                .to_string();
        assert!(error.contains("members"));
        assert!(error.contains("member_offsets"));
    }
}

#[test]
fn construction_auxiliary_rows_preserve_wire_and_reject_unequal_offsets() {
    for fields in [
        "",
        r#","auxiliary_record_indices":[103,106],"auxiliary_record_offsets":[37,48]"#,
    ] {
        let wire = format!(
            r#"{{"member_count_offset":20{fields},"opaque_index":1,"opaque_index_offset":80,"opaque_scalar":0.0,"opaque_scalar_offset":84,"variant":false}}"#
        );
        let frame: crate::records::topology::DesignConstructionOperandGroupFrame =
            serde_json::from_str(&wire).expect("construction frame");
        assert_eq!(
            serde_json::to_string(&frame).expect("construction wire"),
            wire
        );
    }
    for fields in [
        r#", "auxiliary_record_indices":[103]"#,
        r#", "auxiliary_record_offsets":[37]"#,
        r#", "auxiliary_record_indices":[103,106],"auxiliary_record_offsets":[37]"#,
    ] {
        let wire = format!(
            r#"{{"member_count_offset":20{fields},"opaque_index":1,"opaque_index_offset":80,"opaque_scalar":0.0,"opaque_scalar_offset":84,"variant":false}}"#
        );
        let error = serde_json::from_str::<
            crate::records::topology::DesignConstructionOperandGroupFrame,
        >(&wire)
        .expect_err("unequal auxiliary arrays")
        .to_string();
        assert!(error.contains("auxiliary_record_indices"));
        assert!(error.contains("auxiliary_record_offsets"));
    }
}

#[test]
fn construction_trailing_rows_preserve_wire_and_reject_unequal_offsets() {
    for fields in [
        "",
        r#","trailing_record_indices":[300],"trailing_record_offsets":[1044]"#,
        r#","trailing_record_indices":[300,301],"trailing_record_offsets":[1044,1055]"#,
    ] {
        let wire = format!(
            r#"{{"member_count_offset":20{fields},"opaque_index":1,"opaque_index_offset":80,"opaque_scalar":0.0,"opaque_scalar_offset":84,"variant":false}}"#
        );
        let frame: crate::records::topology::DesignConstructionOperandGroupFrame =
            serde_json::from_str(&wire).expect("construction frame");
        assert_eq!(
            serde_json::to_string(&frame).expect("construction wire"),
            wire
        );
    }
    for fields in [
        r#","trailing_record_indices":[300]"#,
        r#","trailing_record_offsets":[1044]"#,
        r#","trailing_record_indices":[300,301],"trailing_record_offsets":[1044]"#,
    ] {
        let wire = format!(
            r#"{{"member_count_offset":20{fields},"opaque_index":1,"opaque_index_offset":80,"opaque_scalar":0.0,"opaque_scalar_offset":84,"variant":false}}"#
        );
        let error = serde_json::from_str::<
            crate::records::topology::DesignConstructionOperandGroupFrame,
        >(&wire)
        .expect_err("unequal trailing arrays")
        .to_string();
        assert!(error.contains("trailing_record_indices"));
        assert!(error.contains("trailing_record_offsets"));
    }
}

#[test]
fn construction_member_rows_preserve_wire_and_reject_unequal_offsets() {
    for (members, offsets) in [("[]", "[]"), ("[10]", "[0]"), ("[10,11]", "[26,37]")] {
        let wire = format!(
            r#"{{"id":"group","scope_record_index":7,"scope_reference_ordinal":0,"record_index":9,"byte_offset":0,"class_tag":"277","members":{members},"member_offsets":{offsets},"frame":{{"member_count_offset":21,"opaque_index":1,"opaque_index_offset":80,"opaque_scalar":0.0,"opaque_scalar_offset":84,"variant":false}},"role":0,"role_offset":60,"paired_class_tag":"278","paired_byte_offset":100}}"#
        );
        let group: crate::records::topology::DesignConstructionOperandGroup =
            serde_json::from_str(&wire).expect("construction group");
        assert_eq!(
            serde_json::to_string(&group).expect("construction wire"),
            wire
        );
        let invalid = wire.replace(
            &format!("\"member_offsets\":{offsets}"),
            "\"member_offsets\":[1,2,3]",
        );
        let error =
            serde_json::from_str::<crate::records::topology::DesignConstructionOperandGroup>(
                &invalid,
            )
            .expect_err("unequal member arrays")
            .to_string();
        assert!(error.contains("members"));
        assert!(error.contains("member_offsets"));
        for (roles, valid) in [
            (r#""extrude_role":"bodies","#, true),
            (
                r#""extrude_role":"faces","extrude_face_role":"start","#,
                true,
            ),
            (r#""extrude_role":"faces","#, false),
            (r#""extrude_face_role":"start","#, false),
            (
                r#""extrude_role":"bodies","extrude_face_role":"start","#,
                false,
            ),
        ] {
            let role = if roles.contains("bodies") {
                crate::records::topology::DesignOperandRole::BODIES_A
            } else {
                crate::records::topology::DesignOperandRole::FACES
            };
            let tagged = wire.replace(r#""role":0,"#, &format!("\"role\":{},{roles}", role.raw()));
            let parsed = serde_json::from_str::<
                crate::records::topology::DesignConstructionOperandGroup,
            >(&tagged);
            if valid {
                assert_eq!(
                    serde_json::to_string(&parsed.expect("tagged construction group"))
                        .expect("tagged construction wire"),
                    tagged
                );
            } else {
                assert!(parsed
                    .expect_err("unpaired extrude role")
                    .to_string()
                    .contains("extrude_face_role"));
            }
        }
    }
}

#[test]
fn face_source_rows_preserve_wire_and_reject_unequal_offsets() {
    let prefix = r#"{"id":"face-source","scope_record_index":1,"carrier_reference_ordinal":0,"carrier_record_index":2,"carrier_byte_offset":0,"carrier_class_tag":"302","carrier_frame_length":80,"paired_record_index":3,"paired_byte_offset":80,"paired_class_tag":"303""#;
    let member = r#"{"record_index":100,"byte_offset":1000,"class_tag":"304","persistent_identity":{"local_id":1,"local_id_offset":1021,"asset_id":"AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE","asset_id_offset":1033,"context_id":"11111111-2222-4333-8444-555555555555","context_id_offset":1109,"tail_slot_present":false,"tail_slot_offset":1185,"next_record_index":101,"next_byte_offset":1190}}"#;
    for (members, offsets) in [
        ("[]".to_owned(), "[]"),
        (format!("[{member}]"), "[25]"),
        (format!("[{member},{member}]"), "[25,36]"),
    ] {
        let wire = format!(
            "{prefix},\"source_reference_offsets\":{offsets},\"source_members\":{members}}}"
        );
        let group: crate::records::topology::DesignFaceSourceGroup =
            serde_json::from_str(&wire).expect("Face source rows");
        assert_eq!(
            serde_json::to_string(&group).expect("Face source wire"),
            wire
        );
        let invalid = wire.replace(
            &format!("\"source_reference_offsets\":{offsets}"),
            "\"source_reference_offsets\":[25,36,47]",
        );
        let error =
            serde_json::from_str::<crate::records::topology::DesignFaceSourceGroup>(&invalid)
                .expect_err("unequal source arrays")
                .to_string();
        assert!(error.contains("source_members"));
        assert!(error.contains("source_reference_offsets"));
    }
}

#[test]
fn face_source_span_rejects_empty_reversed_and_conflicting_lengths() {
    let wire = r#"{"id":"face-source","scope_record_index":1,"carrier_reference_ordinal":0,"carrier_record_index":2,"carrier_byte_offset":10,"carrier_class_tag":"302","carrier_frame_length":80,"paired_record_index":3,"paired_byte_offset":90,"paired_class_tag":"303","source_reference_offsets":[],"source_members":[]}"#;
    for (field, value) in [
        ("paired_byte_offset", 10),
        ("paired_byte_offset", 9),
        ("carrier_frame_length", 79),
    ] {
        let mut invalid: serde_json::Value = serde_json::from_str(wire).expect("Face source wire");
        invalid[field] = value.into();
        let error =
            serde_json::from_value::<crate::records::topology::DesignFaceSourceGroup>(invalid)
                .expect_err("invalid carrier span")
                .to_string();
        assert!(error.contains(field));
    }
    let group: crate::records::topology::DesignFaceSourceGroup =
        serde_json::from_str(wire).expect("positive carrier span");
    assert_eq!(
        serde_json::to_string(&group).expect("Face source wire"),
        wire
    );
}

#[test]
fn historical_loop_wire_preserves_each_complete_binding_stage() {
    for count in [0_u32, 1, 3] {
        for stage in 0..4 {
            let mut wire = serde_json::json!({
                "loop_slot": 10,
                "coedge_slots": (0..count).map(|index| 20 + index).collect::<Vec<_>>(),
                "edge_slots": (0..count).map(|index| 30 + index).collect::<Vec<_>>()
            });
            if count != 0 {
                if stage >= 1 {
                    wire["vertex_slots"] =
                        serde_json::json!((0..count).map(|index| 40 + index).collect::<Vec<_>>());
                }
                if stage >= 2 {
                    wire["point_slots"] =
                        serde_json::json!((0..count).map(|index| 50 + index).collect::<Vec<_>>());
                }
                if stage >= 3 {
                    wire["positions"] = serde_json::json!((0..count)
                        .map(|index| cadmpeg_ir::math::Point3::new(f64::from(index), 0.0, 0.0))
                        .collect::<Vec<_>>());
                }
            }
            let context: crate::records::topology::DesignHistoricalFaceLoopContext =
                serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(context.boundary.coedges().count(), count as usize);
            assert_eq!(serde_json::to_value(&context).unwrap(), wire);
            for field in ["edge_slots", "vertex_slots", "point_slots", "positions"] {
                let mut invalid = wire.clone();
                let mut values = invalid
                    .get(field)
                    .and_then(serde_json::Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for _ in 0..=count {
                    values.push(if field == "positions" {
                        serde_json::to_value(cadmpeg_ir::math::Point3::new(9.0, 0.0, 0.0)).unwrap()
                    } else {
                        serde_json::json!(99)
                    });
                }
                invalid[field] = serde_json::Value::Array(values);
                assert!(serde_json::from_value::<
                    crate::records::topology::DesignHistoricalFaceLoopContext,
                >(invalid)
                .unwrap_err()
                .to_string()
                .contains(field));
            }
        }
    }
}

#[test]
fn face_operand_wire_derives_node_offsets() {
    for count in [0_u32, 1, 3] {
        let offsets: Vec<_> = (0..count).map(|index| 100 + index * 16).collect();
        let nodes: Vec<_> = offsets
            .iter()
            .map(|offset| {
                serde_json::json!({
                    "byte_offset": offset, "end_byte_offset": offset + 16,
                    "program": [-1, -1, 2, 7], "recipe_structure": null
                })
            })
            .collect();
        let base = serde_json::json!({
            "id": "face", "scope_record_index": 1, "scope_reference_ordinal": 0,
            "record_index": 2, "byte_offset": 10, "class_tag": "346",
            "paired_byte_offset": 20, "paired_class_tag": "262",
            "recipe_record_index": 3, "recipe_record_byte_offset": 30,
            "recipe_id": "recipe", "recipe_prefix_offset": 40, "recipe_prefix_bytes": "",
            "recipe_references": [], "recipe_kind": "bounded_face",
            "recipe_program_offset": 50, "recipe_program": [0, -1, 1],
            "recipe_node_offsets": offsets, "recipe_nodes": nodes,
            "next_record_index": 4, "next_byte_offset": 200
        });
        for grouped in [false, true] {
            let mut wire = base.clone();
            if grouped {
                wire["group_record_index"] = serde_json::json!(5);
                wire["group_member_ordinal"] = serde_json::json!(0);
            }
            for field in ["group_record_index", "group_member_ordinal"] {
                let mut invalid = wire.clone();
                invalid
                    .as_object_mut()
                    .unwrap()
                    .remove("group_record_index");
                invalid
                    .as_object_mut()
                    .unwrap()
                    .remove("group_member_ordinal");
                invalid[field] = serde_json::json!(5);
                let error =
                    serde_json::from_value::<crate::records::topology::DesignFaceOperand>(invalid)
                        .unwrap_err()
                        .to_string();
                assert!(error.contains("group_record_index"));
                assert!(error.contains("group_member_ordinal"));
            }
            let operand: crate::records::topology::DesignFaceOperand =
                serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(&operand).unwrap(), wire);
            let mut invalid = wire.clone();
            invalid["recipe_node_offsets"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!(999));
            assert!(
                serde_json::from_value::<crate::records::topology::DesignFaceOperand>(invalid)
                    .unwrap_err()
                    .to_string()
                    .contains("recipe_node_offsets")
            );
            if count != 0 {
                let mut invalid = wire;
                invalid["recipe_node_offsets"][0] = serde_json::json!(999);
                assert!(
                    serde_json::from_value::<crate::records::topology::DesignFaceOperand>(invalid)
                        .unwrap_err()
                        .to_string()
                        .contains("recipe_node_offsets")
                );
            }
        }
    }
}

#[test]
fn selector_context_wire_rejects_partial_clauses_and_derives_singleton() {
    let entry = crate::records::topology::DesignTopologyRecipeEntry {
        selector: 3,
        boundary_edge_count: std::num::NonZeroU32::new(4).unwrap(),
        topology_triplets: std::array::from_fn(|_| {
            crate::records::topology::DesignTopologyRecipeTriplet {
                outer: std::num::NonZeroU32::new(3).unwrap(),
                middle: 2,
                incident: Some(crate::records::topology::DesignTopologyIncident {
                    ordinal: 1,
                    side: crate::records::topology::DesignTopologyIncidentSide::Preceding,
                }),
            }
        }),
    };
    for edges in [vec![], vec![7], vec![7, 8]] {
        for count in [0, 1, 3] {
            let entries: Vec<_> = (0..count)
                .map(|index| (index % 2 == 0).then(|| entry.clone()))
                .collect();
            let slots: Vec<_> = (0..count)
                .map(|index| (index % 2 == 0).then(|| [vec![7, 8], vec![7]]))
                .collect();
            let mut wire = serde_json::json!({
                "selector": 3, "clause_entries": entries,
                "clause_triplet_edge_slots": slots,
                "incidence_matching_edge_slots": edges,
                "boundary_count_matching_edge_slots": [7, 8]
            });
            if edges.len() == 1 {
                wire["unique_incidence_edge_slot"] = serde_json::json!(7);
            }
            let context: crate::records::topology::DesignEdgeRecipeSelectorContext =
                serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(context.clauses.len(), count);
            assert_eq!(serde_json::to_value(&context).unwrap(), wire);
            let mut invalid = wire.clone();
            invalid["unique_incidence_edge_slot"] = serde_json::json!(9);
            assert!(serde_json::from_value::<
                crate::records::topology::DesignEdgeRecipeSelectorContext,
            >(invalid)
            .unwrap_err()
            .to_string()
            .contains("unique_incidence_edge_slot"));
            for field in ["clause_entries", "clause_triplet_edge_slots"] {
                let mut invalid = wire.clone();
                invalid[field]
                    .as_array_mut()
                    .unwrap()
                    .push(serde_json::Value::Null);
                assert!(serde_json::from_value::<
                    crate::records::topology::DesignEdgeRecipeSelectorContext,
                >(invalid)
                .unwrap_err()
                .to_string()
                .contains("clause_triplet_edge_slots"));
                if count != 0 {
                    let mut invalid = wire.clone();
                    invalid[field][0] = serde_json::Value::Null;
                    let error = serde_json::from_value::<
                        crate::records::topology::DesignEdgeRecipeSelectorContext,
                    >(invalid)
                    .unwrap_err()
                    .to_string();
                    assert!(error.contains("clause_entries"));
                    assert!(error.contains("clause_triplet_edge_slots"));
                }
            }
        }
    }
}

#[test]
fn edge_operand_wire_rejects_partial_resolved_axis() {
    let prefix = r#"{"id":"edge","scope_record_index":1,"scope_reference_ordinal":0,"record_index":2,"byte_offset":10,"class_tag":"346","paired_byte_offset":20,"paired_class_tag":"262","recipe_record_index":3,"recipe_record_byte_offset":30,"recipe_id":"recipe","recipe_prefix_offset":40,"recipe_prefix_bytes":"","recipe_references":[],"recipe_program_offset":50,"recipe_program":[]"#;
    let suffix = r#","next_record_index":4,"next_byte_offset":100}"#;
    let origin = serde_json::to_string(&cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)).unwrap();
    let direction = serde_json::to_string(&cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)).unwrap();
    for fields in [
        String::new(),
        format!(",\"resolved_axis_origin\":{origin},\"resolved_axis_direction\":{direction}"),
    ] {
        let wire = format!("{prefix}{fields}{suffix}");
        let operand: crate::records::topology::DesignEdgeOperand =
            serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&operand).unwrap(), wire);
    }
    for (field, value) in [
        ("resolved_axis_origin", origin),
        ("resolved_axis_direction", direction),
    ] {
        let invalid = format!("{prefix},\"{field}\":{value}{suffix}");
        let error = serde_json::from_str::<crate::records::topology::DesignEdgeOperand>(&invalid)
            .unwrap_err()
            .to_string();
        assert!(error.contains("resolved_axis_origin"));
        assert!(error.contains("resolved_axis_direction"));
    }
}

#[test]
fn historical_binding_wire_rejects_partial_identity_and_orphan_states() {
    fn check<T>(base: &serde_json::Value)
    where
        T: serde::de::DeserializeOwned + serde::Serialize + std::fmt::Debug,
    {
        for binding in [
            serde_json::json!({}),
            serde_json::json!({"historical_entity_kind": "loop", "historical_entity_ref": 42}),
            serde_json::json!({"historical_entity_kind": "loop", "historical_entity_ref": 42, "historical_state_ids": [2, 3]}),
        ] {
            let mut wire = base.clone();
            wire.as_object_mut()
                .unwrap()
                .extend(binding.as_object().unwrap().clone());
            let value: T = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(value).unwrap(), wire);
        }
        for binding in [
            serde_json::json!({"historical_entity_kind": "loop"}),
            serde_json::json!({"historical_entity_ref": 42}),
            serde_json::json!({"historical_state_ids": [2]}),
            serde_json::json!({"historical_entity_kind": "loop", "historical_state_ids": [2]}),
            serde_json::json!({"historical_entity_ref": 42, "historical_state_ids": [2]}),
        ] {
            let mut invalid = base.clone();
            invalid
                .as_object_mut()
                .unwrap()
                .extend(binding.as_object().unwrap().clone());
            let error = serde_json::from_value::<T>(invalid)
                .unwrap_err()
                .to_string();
            assert!(error.contains("historical_entity_kind"));
            assert!(error.contains("historical_entity_ref"));
            assert!(error.contains("historical_state_ids"));
        }
    }
    let mut member = serde_json::json!({
        "id": "member", "group_record_index": 1, "group_member_ordinal": 0,
        "record_index": 2, "byte_offset": 10, "class_tag": "346",
        "local_id": 17, "local_id_offset": 20,
        "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d", "asset_id_offset": 30,
        "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e", "context_id_offset": 40,
        "tail_slot_present": false, "tail_slot_offset": 0,
        "next_record_index": 3, "next_byte_offset": 50
    });
    check::<crate::records::topology::DesignExtrudeSelectionMember>(&member);
    for field in ["asset_id", "context_id"] {
        let mut invalid = member.clone();
        invalid[field] = serde_json::json!("asset");
        assert!(
            serde_json::from_value::<crate::records::topology::DesignExtrudeSelectionMember>(
                invalid
            )
            .expect_err("non-GUID selection identity")
            .to_string()
            .contains("GUID")
        );
    }
    for field in [
        "tail_slot_present",
        "tail_slot_offset",
        "next_record_index",
        "next_byte_offset",
    ] {
        member.as_object_mut().unwrap().remove(field);
    }
    member["scope_record_index"] = serde_json::json!(4);
    member["compact_layout"] = serde_json::json!(false);
    member["local_id_offset"] = serde_json::json!(34);
    check::<crate::records::topology::DesignEdgeIdentityOperand>(&member);
    for (compact, local_id_offset) in [(true, 32), (true, 33), (false, 34)] {
        let mut framed = member.clone();
        framed["compact_layout"] = compact.into();
        framed["local_id_offset"] = local_id_offset.into();
        let operand =
            serde_json::from_value::<crate::records::topology::DesignEdgeIdentityOperand>(framed)
                .expect("edge-identity prologue framing");
        assert_eq!(operand.local_id_offset(), local_id_offset);
        assert_eq!(operand.layout.is_compact(), compact);
    }
    for (compact, local_id_offset) in [(false, 32), (false, 33), (true, 34), (false, 20)] {
        let mut framed = member.clone();
        framed["compact_layout"] = compact.into();
        framed["local_id_offset"] = local_id_offset.into();
        assert!(
            serde_json::from_value::<crate::records::topology::DesignEdgeIdentityOperand>(framed)
                .is_err(),
            "compact_layout {compact} with local_id_offset {local_id_offset}"
        );
    }
    for field in ["asset_id", "context_id"] {
        let mut invalid = member.clone();
        invalid[field] = serde_json::json!("asset");
        assert!(
            serde_json::from_value::<crate::records::topology::DesignEdgeIdentityOperand>(invalid)
                .expect_err("non-GUID edge identity")
                .to_string()
                .contains("GUID")
        );
    }
}

#[test]
fn variable_fillet_midpoints_preserve_wire_and_reject_unpaired_records() {
    let wire = r#"{"kind":"variable","start_radius_parameter_record_index":51,"end_radius_parameter_record_index":61,"middle_radius_parameter_record_indices":[71],"middle_parameter_record_indices":[81]}"#;
    let law: crate::records::topology::DesignFilletRadiusLaw = serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&law).unwrap(), wire);
    for invalid in [wire.replace("[71]", "[]"), wire.replace("[81]", "[]")] {
        let error =
            serde_json::from_str::<crate::records::topology::DesignFilletRadiusLaw>(&invalid)
                .unwrap_err()
                .to_string();
        assert!(error.contains("middle_radius_parameter_record_indices"));
        assert!(error.contains("middle_parameter_record_indices"));
    }
}

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
                    let member: crate::records::topology::DesignSketchProfileRegionMember =
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
        let error =
            serde_json::from_str::<crate::records::topology::DesignSketchProfileRegionMember>(
                &wire(kind, 1, [0, 0, 0, 0, 1, 1, 0, 0]),
            )
            .expect_err("fixed kind");
        assert!(error.to_string().contains("kind"));
    }
    for identity in [0, u64::from(u32::MAX) + 1, u64::MAX] {
        let error =
            serde_json::from_str::<crate::records::topology::DesignSketchProfileRegionMember>(
                &wire(3, identity, [0, 0, 0, 0, 1, 1, 0, 0]),
            )
            .expect_err("nonzero u32 identity");
        assert!(error.to_string().contains("curve_primary_id"));
    }
    for index in 0..8 {
        let mut words = [0, 0, 0, 0, 1, 1, 0, 0];
        words[index] = 3;
        let error =
            serde_json::from_str::<crate::records::topology::DesignSketchProfileRegionMember>(
                &wire(3, 1, words),
            )
            .expect_err("invalid incidence word");
        assert!(error.to_string().contains("incidence_words"));
    }
}

#[test]
fn topology_recipe_derived_ordinals_preserve_wire_and_reject_conflicts() {
    for field in ["incident_edge_ordinal", "incident_side"] {
        let mut wire = serde_json::json!({"outer":3,"middle":2,"vertex_ordinal":2,"incident_edge_ordinal":1,"incident_side":"preceding"});
        wire.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<crate::records::topology::DesignTopologyRecipeTriplet>(wire)
                .unwrap_err()
                .to_string()
                .contains(field)
        );
    }
    for (outer, vertex) in [(1_u32, 0_u32), (4, 3), (u32::MAX, u32::MAX - 1)] {
        let wire = format!(r#"{{"outer":{outer},"middle":-1,"vertex_ordinal":{vertex}}}"#);
        let triplet: crate::records::topology::DesignTopologyRecipeTriplet =
            serde_json::from_str(&wire).unwrap();
        assert_eq!(triplet.vertex_ordinal(), vertex);
        assert_eq!(serde_json::to_string(&triplet).unwrap(), wire);
        let mut invalid = serde_json::to_value(&triplet).unwrap();
        invalid["vertex_ordinal"] = serde_json::json!(outer);
        assert!(
            serde_json::from_value::<crate::records::topology::DesignTopologyRecipeTriplet>(
                invalid
            )
            .unwrap_err()
            .to_string()
            .contains("vertex_ordinal")
        );
    }
    let wire = r#"{"selector":0,"boundary_edge_count":4,"topology_triplets":[{"outer":3,"middle":2,"vertex_ordinal":2,"incident_edge_ordinal":1,"incident_side":"preceding"},{"outer":3,"middle":2,"vertex_ordinal":2,"incident_edge_ordinal":1,"incident_side":"preceding"}],"common_incident_edge_ordinal":1}"#;
    let entry: crate::records::topology::DesignTopologyRecipeEntry =
        serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&entry).unwrap(), wire);
    let mut invalid = serde_json::to_value(entry).unwrap();
    invalid["common_incident_edge_ordinal"] = serde_json::json!(2);
    assert!(
        serde_json::from_value::<crate::records::topology::DesignTopologyRecipeEntry>(invalid)
            .unwrap_err()
            .to_string()
            .contains("common_incident_edge_ordinal")
    );
}

#[test]
fn surface_patch_recipe_requires_two_clauses_and_preserves_root_wire() {
    let clause = r#"{"fields":[[0],[0],[2,0],[0,0],[0],[0,0]],"face_reference_ordinals":[0,0],"edge_reference_ordinals":[0,0],"payload_entry_count":0,"entries":[]}"#;
    let wire = format!(r#"{{"root":2,"clauses":[{clause},{clause}]}}"#);
    let structure: crate::records::topology::DesignSurfacePatchRecipeStructure =
        serde_json::from_str(&wire).unwrap();
    assert_eq!(serde_json::to_string(&structure).unwrap(), wire);
    let invalid_root = wire.replace("\"root\":2", "\"root\":1");
    assert!(
        serde_json::from_str::<crate::records::topology::DesignSurfacePatchRecipeStructure>(
            &invalid_root
        )
        .unwrap_err()
        .to_string()
        .contains("root")
    );
    for clauses in [
        String::new(),
        clause.to_owned(),
        format!("{clause},{clause},{clause}"),
    ] {
        let invalid = format!(r#"{{"root":2,"clauses":[{clauses}]}}"#);
        assert!(
            serde_json::from_str::<crate::records::topology::DesignSurfacePatchRecipeStructure>(
                &invalid
            )
            .unwrap_err()
            .to_string()
            .contains("clauses")
        );
    }
}

#[test]
fn face_recipe_postlude_derives_delimiters_and_rejects_other_programs() {
    let side = r#"{"field_count":2,"header_value":0,"scalars":[0],"payload_prefix":[0],"payload_entry_count":0,"entries":[]}"#;
    let prefix = format!(r#"{{"root":0,"prelude":[1,2],"sides":[{side},{side}]"#);
    for value in [i32::MIN, -1, 0, 4, i32::MAX] {
        let wire = format!(r#"{prefix},"postlude":[-1,{value},-1,0,0,-1]}}"#);
        let structure: crate::records::topology::DesignFaceRecipeStructure =
            serde_json::from_str(&wire).unwrap();
        assert_eq!(structure.postlude_value, Some(value));
        assert_eq!(serde_json::to_string(&structure).unwrap(), wire);
    }
    let omitted = format!("{prefix}}}");
    for wire in [omitted.clone(), format!(r#"{prefix},"postlude":[]}}"#)] {
        let structure: crate::records::topology::DesignFaceRecipeStructure =
            serde_json::from_str(&wire).unwrap();
        assert_eq!(structure.postlude_value, None);
        assert_eq!(serde_json::to_string(&structure).unwrap(), omitted);
    }
    for postlude in [
        "[-1]",
        "[-1,4,-1,0,0]",
        "[0,4,-1,0,0,-1]",
        "[-1,4,-1,0,1,-1]",
    ] {
        let wire = format!(r#"{prefix},"postlude":{postlude}}}"#);
        assert!(
            serde_json::from_str::<crate::records::topology::DesignFaceRecipeStructure>(&wire)
                .unwrap_err()
                .to_string()
                .contains("postlude")
        );
    }
}

#[test]
fn construction_group_wire_requires_source_and_extrude_roles_to_agree() {
    use crate::records::topology::{DesignConstructionOperandGroup, DesignOperandRole};
    let wire = serde_json::json!({
        "id": "group", "scope_record_index": 1, "scope_reference_ordinal": 0,
        "record_index": 2, "byte_offset": 10, "class_tag": "256",
        "members": [], "member_offsets": [],
        "frame": {"member_count_offset": 20, "opaque_index": 1,
            "opaque_index_offset": 30, "opaque_scalar": 0.0,
            "opaque_scalar_offset": 34, "variant": false},
        "role": DesignOperandRole::PROFILE.raw(), "extrude_role": "profile",
        "role_offset": 40, "paired_class_tag": "257", "paired_byte_offset": 50
    });
    let group: DesignConstructionOperandGroup = serde_json::from_value(wire.clone()).unwrap();
    let encoded = serde_json::to_value(&group).unwrap();
    assert_eq!(encoded["role"], wire["role"]);
    assert_eq!(encoded["extrude_role"], wire["extrude_role"]);
    assert_eq!(
        serde_json::from_value::<DesignConstructionOperandGroup>(encoded).unwrap(),
        group
    );
    for (role, extrude_role, face_role) in [
        (DesignOperandRole::PROFILE, "bodies", None),
        (DesignOperandRole::BODIES_A, "profile", None),
        (DesignOperandRole::PROFILE, "faces", Some("start")),
        (DesignOperandRole::FACES, "faces", None),
        (DesignOperandRole::PROFILE, "profile", Some("termination")),
    ] {
        let mut invalid = wire.clone();
        invalid["role"] = serde_json::json!(role.raw());
        invalid["extrude_role"] = serde_json::json!(extrude_role);
        if let Some(face_role) = face_role {
            invalid["extrude_face_role"] = serde_json::json!(face_role);
        }
        let error = serde_json::from_value::<DesignConstructionOperandGroup>(invalid).unwrap_err();
        assert!(error.to_string().contains("role"));
    }
}

#[test]
fn recipe_sidecar_rejects_disagreeing_counts() {
    let side = serde_json::json!({"field_count": 3, "header_value": 0,
        "scalars": [0], "payload_prefix": [0], "payload_entry_count": 0, "entries": []});
    assert!(serde_json::from_value::<super::DesignTopologyRecipeSide>(side).is_err());
    let side = serde_json::json!({"field_count": 2, "header_value": 0,
        "scalars": [0], "payload_prefix": [0], "payload_entry_count": 1, "entries": []});
    assert!(serde_json::from_value::<super::DesignTopologyRecipeSide>(side).is_err());
    let clause = serde_json::json!({"fields": [], "face_reference_ordinals": [0, 0],
        "edge_reference_ordinals": [0, 0], "payload_entry_count": 1, "entries": []});
    assert!(serde_json::from_value::<super::DesignSurfacePatchRecipeClause>(clause).is_err());
}

#[test]
fn recipe_sidecar_derives_counts_without_changing_wire() {
    let side = serde_json::json!({"field_count": 2, "header_value": 0,
        "scalars": [0], "payload_prefix": [0], "payload_entry_count": 0, "entries": []});
    let record: super::DesignTopologyRecipeSide = serde_json::from_value(side.clone()).unwrap();
    assert_eq!(record.field_count(), 2);
    assert_eq!(serde_json::to_value(record).unwrap(), side);
}
