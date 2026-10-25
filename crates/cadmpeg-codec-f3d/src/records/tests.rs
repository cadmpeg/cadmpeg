// SPDX-License-Identifier: Apache-2.0

mod graphics;
mod parameter;
mod parameter_owner;
mod placement_matrix;

#[test]
fn parameter_discriminator_is_semantic_and_locations_are_not_document_content() {
    let base = serde_json::json!({"id":"parameter","class_tag":"123","record_index":1,"source_ordinal":0,"owner_record_index":2,"expression":"1","source_kind":"Distance","name":"d1","evaluated_value":1.0});
    for discriminator in [None, Some(0)] {
        let mut wire = base.clone();
        if let Some(value) = discriminator {
            wire["family_discriminator"] = serde_json::json!(value);
        }
        let parameter: crate::records::parameters::DesignParameter =
            serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(parameter).unwrap(), wire);
    }
    for field in [
        "family_discriminator_offset",
        "expression_offset",
        "name_offset",
        "evaluated_value_offset",
    ] {
        let mut invalid = base.clone();
        invalid[field] = serde_json::json!(0);
        assert!(
            serde_json::from_value::<crate::records::parameters::DesignParameter>(invalid)
                .unwrap_err()
                .to_string()
                .contains(field)
        );
    }
}

#[test]
fn selection_secondary_identities_preserve_wire_and_reject_partial_locations() {
    let fields = r#""record_index":2,"byte_offset":0,"class_tag":"365","asset_id":"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d","asset_id_offset":100,"context_id":"1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e","context_id_offset":150,"identity_record_index":5,"identity_record_offset":180,"primary_identity":183,"primary_identity_offset":209"#;
    let suffix = r#","next_record_index":6,"next_byte_offset":225}"#;
    for prefix in [
        "{",
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

#[test]
fn material_assignment_keeps_optional_tokens_independent_of_source_locations() {
    let base = serde_json::json!({"id":"f3d:native:material-assignment#0",
        "body_binding_id":"f3d:native:design-body-binding#0","asm_body_key":42,"entity_id":"0_985",
        "visual_guid":"11111111-2222-3333-4444-555555555555"});
    for field in ["physical_token", "visual_preset"] {
        for value in ["", "Prism-002"] {
            let mut wire = base.clone();
            wire[field] = value.into();
            let parsed: crate::records::references::DesignMaterialAssignment =
                serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(parsed).unwrap(), wire);
        }
        let mut invalid = base.clone();
        invalid[format!("{field}_offset")] = 50.into();
        assert!(
            serde_json::from_value::<crate::records::references::DesignMaterialAssignment>(invalid)
                .is_err()
        );
    }
    let parsed: crate::records::references::DesignMaterialAssignment =
        serde_json::from_value(base.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), base);
    let mut padded = base;
    padded["entity_id"] = "0_+00985".into();
    let parsed: crate::records::references::DesignMaterialAssignment =
        serde_json::from_value(padded.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), padded);
}

#[test]
fn recipe_design_id_is_usable_without_a_source_location() {
    for value in [None, Some(""), Some("301")] {
        let mut wire =
            serde_json::json!({"id":"recipe#0","kind":"body","recipe_index":0,"record_index":12});
        if let Some(value) = value {
            wire["design_id"] = serde_json::json!(value);
        }
        let recipe: crate::records::recipes::ConstructionRecipe =
            serde_json::from_value(wire.clone()).unwrap();
        assert!(recipe.byte_offset.is_none());
        assert!(recipe
            .design
            .as_ref()
            .is_none_or(|design| design.id.offset.is_none()));
        assert_eq!(serde_json::to_value(recipe).unwrap(), wire);
    }
}

#[test]
fn segment_base_guid_preserves_absent_empty_and_named_content() {
    for value in [None, Some(""), Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")] {
        let mut wire = serde_json::json!({"id":"f3d:native/MetaStream.dat:design-type#0","class_tag":"256","type_guid":"11111111-2222-3333-4444-555555555555","version":1,"module":"Fusion","entity_ids":[1]});
        if let Some(value) = value {
            wire["base_type_guid"] = serde_json::json!(value);
        }
        let record: crate::records::entity_header::SegmentType =
            serde_json::from_value(wire.clone()).unwrap();
        assert!(record.base_type_guid.offset().is_none());
        assert_eq!(
            record
                .base_type_guid
                .value()
                .map(super::mesh::DesignRelaxedGuidText::as_str),
            value.filter(|value| !value.is_empty())
        );
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
        wire["base_type_guid_offset"] = serde_json::json!(44);
        assert!(
            serde_json::from_value::<crate::records::entity_header::SegmentType>(wire).is_err()
        );
    }
}

#[test]
fn parameter_unit_is_optional_content_without_an_encoding_location() {
    for unit in [None, Some("mm")] {
        let mut wire = serde_json::json!({"id":"parameter","class_tag":"123","record_index":1,"source_ordinal":0,"owner_record_index":2,"expression":"1","source_kind":"Distance","name":"d1","evaluated_value":1.0});
        if let Some(unit) = unit {
            wire["unit"] = serde_json::json!(unit);
        }
        let parameter: crate::records::parameters::DesignParameter =
            serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(parameter).unwrap(), wire);
        wire["unit_offset"] = serde_json::json!(70);
        assert!(
            serde_json::from_value::<crate::records::parameters::DesignParameter>(wire).is_err()
        );
    }
}

#[test]
fn timeline_items_preserve_authored_order_without_offset_arrays() {
    let base = serde_json::json!({"id":"f3d:Design/BulkStream.dat:design-feature-timeline#200","class_tag":"256","record_index":35,"source_ordinal":0,"context_record_index":17,"item_record_indices":[102,101]});
    let timeline: crate::records::entity_header::DesignFeatureTimeline =
        serde_json::from_value(base.clone()).unwrap();
    assert_eq!(serde_json::to_value(timeline).unwrap(), base);
    for indices in [serde_json::json!([101, 101]), serde_json::json!([0, 101])] {
        let mut invalid = base.clone();
        invalid["item_record_indices"] = indices;
        assert!(
            serde_json::from_value::<crate::records::entity_header::DesignFeatureTimeline>(invalid)
                .is_err()
        );
    }
    let mut invalid = base;
    invalid["item_record_index_offsets"] = serde_json::json!([245, 256]);
    assert!(
        serde_json::from_value::<crate::records::entity_header::DesignFeatureTimeline>(invalid)
            .is_err()
    );
}

#[test]
fn annotation_return_members_preserve_content_and_reject_changed_multisets() {
    let wire = serde_json::json!({
        "id":"annotation", "governing_companion_record_index":2, "class_tag":"256", "record_index":3,
        "operands":[{"geometry_record_index":10,"role":1},{"geometry_record_index":11,"role":2}],
        "entity_genesis":0,"governing_owner_record_index":4,"return_members":[10,11],
        "paired_class_tag":"259","owner_reference":5
    });
    let frame: crate::records::dimensions::DesignDimensionAnnotationFrame =
        serde_json::from_value(wire.clone()).unwrap();
    assert!(frame.byte_offset().is_none());
    assert_eq!(serde_json::to_value(frame).unwrap(), wire);
    for members in [
        serde_json::json!([]),
        serde_json::json!([10]),
        serde_json::json!([10, 0]),
        serde_json::json!([10, 10]),
    ] {
        let mut invalid = wire.clone();
        invalid["return_members"] = members;
        assert!(
            serde_json::from_value::<crate::records::dimensions::DesignDimensionAnnotationFrame>(
                invalid
            )
            .is_err()
        );
    }
}

#[test]
fn dimension_locus_rows_preserve_return_order_without_derived_duplicates() {
    for state in [0, 32, 16384] {
        let wire = serde_json::json!({
            "id":"locus-group","companion_record_index":2,"class_tag":"256","record_index":3,
            "loci":[{"geometry_record_index":11,"role":0},{"geometry_record_index":10,"role":0}],
            "owner_reference":5,"owner_role":0,"state":state,"return_members":[10,11],
            "next_class_tag":"259","next_record_index":4
        });
        let group: crate::records::dimensions::DesignDimensionLocusGroup =
            serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(group.loci[0].geometry_record_index, 11);
        assert_eq!(group.loci[0].returned.value, 10);
        assert!(group.loci[0].returned.offset.is_none());
        assert_eq!(serde_json::to_value(group).unwrap(), wire);
        for field in ["loci", "return_members"] {
            let mut invalid = wire.clone();
            invalid[field].as_array_mut().unwrap().pop();
            assert!(
                serde_json::from_value::<crate::records::dimensions::DesignDimensionLocusGroup>(
                    invalid
                )
                .is_err()
            );
        }
        for field in [
            "constraint_kinds",
            "unknown_constraint_bits",
            "return_member_offsets",
            "byte_offset",
        ] {
            let mut invalid = wire.clone();
            invalid[field] = 0.into();
            assert!(
                serde_json::from_value::<crate::records::dimensions::DesignDimensionLocusGroup>(
                    invalid
                )
                .is_err()
            );
        }
    }
}

#[test]
fn segment_entity_runs_keep_order_and_class_identity_without_source_offsets() {
    let base = serde_json::json!({
        "id":"f3d:native/MetaStream.dat:design-type#0", "class_tag":"256",
        "type_guid":"11111111-2222-3333-4444-555555555555", "version":1, "module":"Fusion", "entity_ids":[]
    });
    for ids in [
        serde_json::json!([]),
        serde_json::json!([10, 11]),
        serde_json::json!([11, 10]),
    ] {
        let mut wire = base.clone();
        wire["entity_ids"] = ids;
        let entry: crate::records::entity_header::SegmentType =
            serde_json::from_value(wire.clone()).unwrap();
        assert!(entry.byte_offset.is_none());
        assert_eq!(serde_json::to_value(entry).unwrap(), wire);
    }
    for field in [
        "byte_offset",
        "type_guid_offset",
        "version_offset",
        "entity_id_offsets",
    ] {
        let mut invalid = base.clone();
        invalid[field] = serde_json::json!([80]);
        assert!(
            serde_json::from_value::<crate::records::entity_header::SegmentType>(invalid).is_err()
        );
    }
}

#[test]
fn entity_header_keeps_absent_reference_list_distinct_from_present_empty_list() {
    let base =
        serde_json::json!({"id":"header","entity_id":"0_1","class_tag":"256","module":"MSketch"});
    for references in [
        None,
        Some(serde_json::json!([])),
        Some(serde_json::json!([34, 35])),
    ] {
        let mut wire = base.clone();
        if let Some(references) = references {
            wire["reference_indices"] = references;
        }
        let header: crate::records::entity_header::DesignEntityHeader =
            serde_json::from_value(wire.clone()).unwrap();
        assert!(header.byte_offset.is_none());
        assert_eq!(serde_json::to_value(header).unwrap(), wire);
    }
    let mut bound = base.clone();
    bound["record_reference"] = 33.into();
    bound["reference_indices"] = serde_json::json!([34, 35]);
    bound["member_indices"] = serde_json::json!([11]);
    let header: crate::records::entity_header::DesignEntityHeader =
        serde_json::from_value(bound.clone()).unwrap();
    assert_eq!(serde_json::to_value(header).unwrap(), bound);
    for field in [
        "declared_reference_count",
        "reference_offsets",
        "member_offsets",
        "record_reference_offset",
        "byte_offset",
        "optional_slot_present",
    ] {
        let mut invalid = base.clone();
        invalid[field] = 0.into();
        assert!(
            serde_json::from_value::<crate::records::entity_header::DesignEntityHeader>(invalid)
                .is_err()
        );
    }
    let mut orphan = base;
    orphan["record_reference"] = 33.into();
    assert!(
        serde_json::from_value::<crate::records::entity_header::DesignEntityHeader>(orphan)
            .is_err()
    );
}

#[test]
fn sketch_auxiliary_rows_keep_semantic_reference_order_without_location_runs() {
    let base = serde_json::json!({"id":"relation","record_index":1,"class_tag":"000",
        "owner_reference":1,"owner_entity_id":"owner","auxiliary_references":[],"members":[],
        "resolved_members":[],"state":0,"member_relation_ordinals":[],"entity_genesis":null,
        "pattern":null,"return_members":[],"resolved_return_members":[]});
    for values in [vec![], vec![2], vec![2, 3], vec![3, 2]] {
        let mut wire = base.clone();
        wire["auxiliary_references"] = serde_json::json!(values);
        let relation: crate::records::sketch_relations::SketchRelation =
            serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(
            relation
                .auxiliary_references()
                .values()
                .copied()
                .collect::<Vec<_>>(),
            values
        );
        assert!(relation.auxiliary_references().offsets().next().is_none());
        assert_eq!(serde_json::to_value(relation).unwrap(), wire);
    }
    let mut invalid = base;
    invalid["auxiliary_reference_offsets"] = serde_json::json!([0, 10]);
    assert!(
        serde_json::from_value::<crate::records::sketch_relations::SketchRelation>(invalid)
            .is_err()
    );
}

#[test]
fn sketch_nurbs_poles_preserve_wire_and_reject_partial_weights() {
    let source_refused = r#"{"kind":"nurbs","subtype_class_tag":"302","subtype_record_index":7,"degree":1,"fit_tolerance":0.125,"scalar_width":4,"knots":[0.0,0.0,1.0,1.0],"weights":[],"control_points":[{"x":2.0,"y":3.0,"z":4.0},{"x":5.0,"y":6.0,"z":7.0}]}"#;
    assert!(
        serde_json::from_str::<crate::records::sketch_geometry::SketchCurveGeometry>(
            source_refused
        )
        .unwrap_err()
        .to_string()
        .contains("scalar_width")
    );
    let base = source_refused.replace("\"scalar_width\":4", "\"scalar_width\":8");
    for weights in ["[]", "[1.0,0.5]"] {
        let expected = base.replace("\"weights\":[]", &format!("\"weights\":{weights}"));
        let curve: crate::records::sketch_geometry::SketchCurveGeometry =
            serde_json::from_str(&expected).unwrap();
        assert_eq!(serde_json::to_string(&curve).unwrap(), expected);
    }
    for weights in ["[1.0]", "[1.0,0.5,1.0]"] {
        let wire = base.replace("\"weights\":[]", &format!("\"weights\":{weights}"));
        assert!(
            serde_json::from_str::<crate::records::sketch_geometry::SketchCurveGeometry>(&wire)
                .unwrap_err()
                .to_string()
                .contains("weights")
        );
    }
    let empty = crate::records::sketch_geometry::SketchCurveGeometry::nurbs_from_parts(
        None,
        crate::records::references::DesignClassTag::try_from("302".to_owned()).unwrap(),
        7,
        crate::records::sketch_geometry::SketchNurbsGeometry::from_parts(
            1,
            0.125,
            8,
            vec![0.0, 1.0],
            crate::records::sketch_geometry::SketchNurbsPoles::Rational(Vec::new()),
        )
        .unwrap(),
    );
    let wire = serde_json::to_string(&empty).unwrap();
    assert_eq!(
        serde_json::from_str::<crate::records::sketch_geometry::SketchCurveGeometry>(&wire)
            .unwrap(),
        empty
    );
}

#[test]
fn native_sketch_nurbs_refuses_decreasing_knots_and_negative_weight() {
    let wire = serde_json::json!({
        "kind": "nurbs", "subtype_class_tag": "302", "subtype_record_index": 7,
        "degree": 1, "fit_tolerance": 0.125, "scalar_width": 8,
        "knots": [0.0, 0.0, 1.0, 1.0], "weights": [1.0, 1.0],
        "control_points": [
            {"x": 2.0, "y": 3.0, "z": 4.0},
            {"x": 5.0, "y": 6.0, "z": 7.0}
        ]
    });
    let mut decreasing = wire.clone();
    decreasing["knots"] = serde_json::json!([1.0, 0.0, 1.0, 1.0]);
    assert!(
        serde_json::from_value::<crate::records::sketch_geometry::SketchCurveGeometry>(decreasing)
            .unwrap_err()
            .to_string()
            .contains("nondecreasing")
    );
    let mut negative_weight = wire;
    negative_weight["weights"] = serde_json::json!([-1.0, 1.0]);
    assert!(
        serde_json::from_value::<crate::records::sketch_geometry::SketchCurveGeometry>(
            negative_weight
        )
        .unwrap_err()
        .to_string()
        .contains("positive and finite")
    );
}

#[test]
fn native_sketch_nurbs_refuses_wrong_cardinality_and_negative_fit_tolerance() {
    let wire = serde_json::json!({
        "kind": "nurbs", "subtype_class_tag": "302", "subtype_record_index": 7,
        "degree": 1, "fit_tolerance": 0.125, "scalar_width": 8,
        "knots": [0.0, 0.0, 1.0, 1.0], "weights": [],
        "control_points": [
            {"x": 2.0, "y": 3.0, "z": 4.0},
            {"x": 5.0, "y": 6.0, "z": 7.0}
        ]
    });
    let mut wrong_count = wire.clone();
    wrong_count["degree"] = serde_json::json!(2);
    assert!(
        serde_json::from_value::<crate::records::sketch_geometry::SketchCurveGeometry>(wrong_count)
            .unwrap_err()
            .to_string()
            .contains("knot count")
    );
    let mut negative_fit = wire;
    negative_fit["fit_tolerance"] = serde_json::json!(-0.125);
    assert!(
        serde_json::from_value::<crate::records::sketch_geometry::SketchCurveGeometry>(
            negative_fit
        )
        .unwrap_err()
        .to_string()
        .contains("fit_tolerance")
    );
}

#[test]
fn parameter_source_preserves_semantic_family_and_checks_ownership() {
    for (source_kind, kind, owner) in [
        (
            "User Parameter",
            crate::records::parameters::DesignParameterKind::User,
            None,
        ),
        (
            "Linear Dimension-2",
            crate::records::parameters::DesignParameterKind::Dimension,
            Some(2),
        ),
        (
            "Distance",
            crate::records::parameters::DesignParameterKind::Feature,
            Some(2),
        ),
    ] {
        for discriminator in [0, 3, 4, 5, 6] {
            let mut wire = serde_json::json!({"id":"parameter","class_tag":"123","record_index":1,"family_discriminator":discriminator,"source_ordinal":0,"expression":"1","source_kind":source_kind,"name":"d1","evaluated_value":1.0});
            if let Some(owner) = owner {
                wire["owner_record_index"] = serde_json::json!(owner);
            }
            let parameter: crate::records::parameters::DesignParameter =
                serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(parameter.kind(), kind);
            assert_eq!(parameter.source_kind(), source_kind);
            assert_eq!(serde_json::to_value(parameter).unwrap(), wire);
            let mut wrong_owner = wire.clone();
            if owner.is_none() {
                wrong_owner["owner_record_index"] = serde_json::json!(2);
            } else {
                wrong_owner
                    .as_object_mut()
                    .unwrap()
                    .remove("owner_record_index");
            }
            assert!(
                serde_json::from_value::<crate::records::parameters::DesignParameter>(wrong_owner)
                    .is_err()
            );
            let mut bad_discriminator = wire.clone();
            bad_discriminator["family_discriminator"] = serde_json::json!(7);
            assert!(
                serde_json::from_value::<crate::records::parameters::DesignParameter>(
                    bad_discriminator
                )
                .is_err()
            );
            let mut no_discriminator = wire;
            no_discriminator
                .as_object_mut()
                .unwrap()
                .remove("family_discriminator");
            no_discriminator
                .as_object_mut()
                .unwrap()
                .remove("family_discriminator_offset");
            // Typed compact document parameters and compact owned parameters
            // both store no family discriminator.
            let parameter: crate::records::parameters::DesignParameter =
                serde_json::from_value(no_discriminator.clone()).unwrap();
            assert_eq!(serde_json::to_value(parameter).unwrap(), no_discriminator);
        }
    }
    assert!(
        crate::records::parameters::DesignParameterSource::new(String::new(), Some(2), None)
            .is_err()
    );
}

#[test]
fn construction_recipe_record_index_does_not_require_a_location() {
    for index in [None, Some(7)] {
        let mut wire = serde_json::json!({"id":"recipe","kind":"body","recipe_index":0});
        if let Some(index) = index {
            wire["record_index"] = serde_json::json!(index);
        }
        let recipe: crate::records::recipes::ConstructionRecipe =
            serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(recipe.record_index.map(|record| record.value), index);
        assert!(recipe
            .record_index
            .is_none_or(|record| record.offset.is_none()));
        assert_eq!(serde_json::to_value(recipe).unwrap(), wire);
        wire["record_index_offset"] = serde_json::json!(64);
        assert!(
            serde_json::from_value::<crate::records::recipes::ConstructionRecipe>(wire).is_err()
        );
    }
}

#[test]
fn construction_recipe_design_preserves_selection_and_rejects_orphan_selector() {
    let base = serde_json::json!({"id":"recipe","kind":"body","recipe_index":0,"record_index":7});
    let mut selected = base.clone();
    selected["design_id"] = serde_json::json!("301");
    selected["design_selector"] = serde_json::json!({"value":2});
    let recipe: crate::records::recipes::ConstructionRecipe =
        serde_json::from_value(selected.clone()).unwrap();
    assert_eq!(serde_json::to_value(recipe).unwrap(), selected);
    let mut orphan = base;
    orphan["design_selector"] = serde_json::json!({"value":2});
    assert!(serde_json::from_value::<crate::records::recipes::ConstructionRecipe>(orphan).is_err());
    selected["design_selector"]["byte_offset"] = serde_json::json!(15);
    assert!(
        serde_json::from_value::<crate::records::recipes::ConstructionRecipe>(selected).is_err()
    );
}

#[test]
fn companion_payload_absence_is_distinct_from_an_empty_payload() {
    let base = serde_json::json!({"id":"companion","class_tag":"123","record_index":3,"owner_record_index":2,"timestamp_micros":1});
    let unbound: crate::records::parameters::DesignParameterCompanion =
        serde_json::from_value(base.clone()).unwrap();
    assert!(unbound.payload().is_none());
    let mut empty = base.clone();
    empty["owned_recipe_ids"] = serde_json::json!([]);
    let bound: crate::records::parameters::DesignParameterCompanion =
        serde_json::from_value(empty.clone()).unwrap();
    let payload = bound.payload().unwrap();
    assert!(payload.owned_recipe_ids().is_empty());
    assert!(payload.byte_offset().is_none());
    assert!(payload.byte_length().is_none());
    assert_eq!(serde_json::to_value(bound).unwrap(), empty);
    for field in [
        "payload_byte_offset",
        "payload_byte_length",
        "timestamp_micros_offset",
    ] {
        let mut invalid = base.clone();
        invalid[field] = serde_json::json!(0);
        assert!(
            serde_json::from_value::<crate::records::parameters::DesignParameterCompanion>(invalid)
                .is_err()
        );
    }
}

#[test]
fn companion_timestamp_preserves_wire_and_rejects_zero() {
    use crate::records::parameters::{DesignCompanionPayload, DesignParameterCompanion};
    let source = DesignParameterCompanion::unbound(
        "companion".into(),
        0,
        "123".to_owned().try_into().unwrap(),
        3,
        2,
        std::num::NonZeroU64::new(1).unwrap(),
        42,
    )
    .bound(DesignCompanionPayload::new(58, 0, Vec::new()));
    let wire = r#"{"id":"companion","class_tag":"123","record_index":3,"owner_record_index":2,"timestamp_micros":1,"owned_recipe_ids":[]}"#;
    assert_eq!(serde_json::to_string(&source).unwrap(), wire);
    let companion: DesignParameterCompanion = serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&companion).unwrap(), wire);
    // The source positions are `record_layout` pointers of the companion
    // capture, not metadata.
    assert_eq!(
        source.source_offsets(),
        [
            ("byte_offset", Some(0)),
            ("timestamp_micros_offset", Some(42))
        ]
    );
    assert_eq!(
        source.payload_source_extent(),
        [
            ("payload_byte_offset", Some(58)),
            ("payload_byte_length", Some(0))
        ]
    );
    let native = crate::native::F3dNative {
        design_parameter_companions: vec![source.clone()],
        ..Default::default()
    };
    assert!(native
        .retained_fidelity()
        .expect("source fidelity")
        .metadata
        .is_empty());
    let legacy = wire.replace("timestamp_micros", "opaque_value");
    assert!(serde_json::from_str::<DesignParameterCompanion>(&legacy).is_err());
    for (invalid, field) in [
        (
            wire.replace("\"timestamp_micros\":1", "\"timestamp_micros\":0"),
            "timestamp_micros",
        ),
        (
            legacy.replace("\"opaque_value\":1", "\"opaque_value\":0"),
            "opaque_value",
        ),
    ] {
        let error = serde_json::from_str::<DesignParameterCompanion>(&invalid)
            .unwrap_err()
            .to_string();
        assert!(error.contains(field));
    }
}

#[test]
fn dimension_operands_preserve_null_and_required_index_wires() {
    use crate::records::dimensions::{
        DesignDimensionAnnotationOperand, DesignDimensionPresentationOperand,
    };
    for index in [0, 7, u32::MAX] {
        let wire = format!(r#"{{"geometry_record_index":{index},"role":3}}"#);
        let source = DesignDimensionAnnotationOperand {
            geometry_record_index: std::num::NonZeroU32::new(index),
            geometry_reference_offset: Some(25),
            role: 3,
            role_offset: Some(35),
        };
        assert_eq!(source.geometry_reference_offset, Some(25));
        assert_eq!(source.role_offset, Some(35));
        assert_eq!(serde_json::to_string(&source).unwrap(), wire);
        let operand: DesignDimensionAnnotationOperand = serde_json::from_str(&wire).unwrap();
        assert_eq!(
            operand.geometry_record_index,
            std::num::NonZeroU32::new(index)
        );
        assert_eq!(serde_json::to_string(&operand).unwrap(), wire);
        if index == 0 {
            let error = serde_json::from_str::<DesignDimensionPresentationOperand>(&wire)
                .unwrap_err()
                .to_string();
            assert!(error.contains("geometry_record_index"));
        } else {
            let source = DesignDimensionPresentationOperand {
                geometry_record_index: std::num::NonZeroU32::new(index).unwrap(),
                geometry_reference_offset: Some(25),
                role: 3,
                role_offset: Some(35),
            };
            assert_eq!(source.geometry_reference_offset, Some(25));
            assert_eq!(source.role_offset, Some(35));
            assert_eq!(serde_json::to_string(&source).unwrap(), wire);
            let operand: DesignDimensionPresentationOperand = serde_json::from_str(&wire).unwrap();
            assert_eq!(operand.geometry_record_index.get(), index);
            assert_eq!(serde_json::to_string(&operand).unwrap(), wire);
        }
    }
}

#[test]
fn sketch_entity_identity_derives_suffix_without_changing_its_spelling() {
    use crate::records::topology::sketch_profile::{
        DesignSketchProfileOperand, DesignSketchProfileOperandDraft,
    };

    for (name, suffix) in [
        ("Sketch_0", 0),
        ("Sketch_00017", 17),
        ("Sketch_+0017", 17),
        ("module_Sketch_42", 42),
        ("_18446744073709551615", u64::MAX),
    ] {
        let id = crate::records::identity::DesignEntityId::try_from(name.to_owned())
            .expect("valid entity ID");
        assert_eq!(id.as_str(), name);
        assert_eq!(id.suffix(), suffix);
        let wire = serde_json::json!({"entity_id": name, "entity_suffix": suffix, "entity_reference_offset": 20});
        let binding: crate::records::feature::scope::DesignSketchEntityBinding =
            serde_json::from_value(wire.clone()).expect("matching suffix");
        assert_eq!(serde_json::to_value(binding).unwrap(), wire);
        let mut mismatch = wire;
        mismatch["entity_suffix"] = (suffix ^ 1).into();
        let error = serde_json::from_value::<
            crate::records::feature::scope::DesignSketchEntityBinding,
        >(mismatch)
        .expect_err("contradictory suffix");
        assert!(error.to_string().contains("entity_suffix"));
    }
    for name in [
        "",
        "Sketch",
        "Sketch_",
        "Sketch_-1",
        "Sketch_++1",
        "Sketch_18446744073709551616",
    ] {
        assert!(
            crate::records::identity::DesignEntityId::try_from(name.to_owned())
                .expect_err("invalid entity ID")
                .contains("entity_id")
        );
    }

    let source = DesignSketchProfileOperand::try_new(DesignSketchProfileOperandDraft {
        scope_reference_ordinal: 0,
        record_index: 1,
        byte_offset: 10,
        class_tag: "300".to_owned().try_into().unwrap(),
        asset_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
            .to_owned()
            .try_into()
            .unwrap(),
        asset_id_offset: 20,
        entity_id: "Sketch_00017".to_owned().try_into().unwrap(),
        entity_reference_offset: 30,
        region_selection: None,
        paired_class_tag: "301".to_owned().try_into().unwrap(),
        paired_byte_offset: 40,
    })
    .expect("valid source profile ID");
    let mut fidelity = cadmpeg_ir::SourceFidelity::default();
    source.retain_source(&mut fidelity, "profile", "selection");
    for (field, offset) in [
        ("byte_offset", "10"),
        ("asset_id_offset", "20"),
        ("entity_reference_offset", "30"),
        ("paired_byte_offset", "40"),
    ] {
        assert_eq!(
            fidelity.metadata[&format!("profile.selection.{field}")],
            offset
        );
    }
    let wire = serde_json::json!({
        "scope_reference_ordinal": 0, "record_index": 1,
        "class_tag": "300", "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d",
        "entity_id": "Sketch_00017", "entity_suffix": 17,
        "paired_class_tag": "301"
    });
    assert_eq!(serde_json::to_value(&source).unwrap(), wire);
    let profile: DesignSketchProfileOperand =
        serde_json::from_value(wire.clone()).expect("valid profile ID");
    assert_eq!(serde_json::to_value(profile).unwrap(), wire);
    let mut mismatch = wire.clone();
    mismatch["entity_suffix"] = 18.into();
    assert!(serde_json::from_value::<
        crate::records::topology::sketch_profile::DesignSketchProfileOperand,
    >(mismatch)
    .expect_err("mismatched profile suffix")
    .to_string()
    .contains("entity_suffix"));
    let mut invalid_asset = wire;
    invalid_asset["asset_id"] = "asset".into();
    assert!(serde_json::from_value::<
        crate::records::topology::sketch_profile::DesignSketchProfileOperand,
    >(invalid_asset)
    .expect_err("non-GUID profile asset identity")
    .to_string()
    .contains("GUID"));
}

#[test]
fn sketch_placement_preserves_identity_wire() {
    let wire = serde_json::json!({
        "id": "placement", "entity_id": "Sketch_+0007",
        "class_tag": "300", "record_index": 1,
        "transform": [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0],
                      [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]],
        "paired_class_tag": "301"
    });
    let placement: crate::records::sketch_placement::DesignSketchPlacement =
        serde_json::from_value(wire.clone()).expect("matching identity");
    assert_eq!(placement.entity_id.suffix(), 7);
    assert_eq!(serde_json::to_value(placement).unwrap(), wire);
    for field in ["class_tag", "paired_class_tag"] {
        for invalid in ["", "12", "1234", "1a3", "１２３"] {
            let mut invalid_wire = wire.clone();
            invalid_wire[field] = invalid.into();
            assert!(serde_json::from_value::<
                crate::records::sketch_placement::DesignSketchPlacement,
            >(invalid_wire)
            .unwrap_err()
            .to_string()
            .contains(field));
        }
    }
}

#[test]
fn entity_header_identity_preserves_wire_spelling() {
    let wire = r#"{"id":"header","entity_id":"0_+001","class_tag":"256"}"#;
    let header: crate::records::entity_header::DesignEntityHeader =
        serde_json::from_str(wire).expect("matching identity");
    assert_eq!(header.entity_id.suffix(), 1);
    assert_eq!(serde_json::to_string(&header).unwrap(), wire);
    for class_tag in ["", "25", "25x", "2560", "٢٥٦"] {
        let mut invalid = serde_json::to_value(&header).unwrap();
        invalid["class_tag"] = class_tag.into();
        assert!(
            serde_json::from_value::<crate::records::entity_header::DesignEntityHeader>(invalid)
                .unwrap_err()
                .to_string()
                .contains("class_tag")
        );
    }
}

#[test]
fn sketch_visibility_retains_semantic_ordinal_and_validates_real_source_extents() {
    use crate::records::sketch_placement::DesignSketchVisibility;
    let wire = serde_json::json!({"stream_ordinal":1,"visible":false});
    let visibility: DesignSketchVisibility = serde_json::from_value(wire.clone()).unwrap();
    assert!(visibility.stream_ordinal_offset().is_none());
    assert!(visibility.visible_offset().is_none());
    assert_eq!(serde_json::to_value(visibility).unwrap(), wire);
    for field in ["stream_ordinal_offset", "visible_offset"] {
        let mut invalid = wire.clone();
        invalid[field] = 0.into();
        assert!(serde_json::from_value::<DesignSketchVisibility>(invalid).is_err());
    }
    let mut invalid = wire;
    invalid["stream_ordinal"] = 0.into();
    assert!(serde_json::from_value::<DesignSketchVisibility>(invalid).is_err());
    let last = DesignSketchVisibility::new(std::num::NonZeroU32::MIN, u64::MAX - 5, true).unwrap();
    assert_eq!(last.visible_offset(), Some(u64::MAX));
    assert!(DesignSketchVisibility::new(std::num::NonZeroU32::MIN, u64::MAX - 4, true).is_err());
}

#[test]
fn lost_edge_content_rejects_layout_keys_and_scanner_checks_actual_extent() {
    let wire = r#"{"id":"edge","class_tag":"419","record_index":299,"next_class_tag":"326","next_record_index":300}"#;
    let record: crate::records::references::LostEdgeReference = serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&record).unwrap(), wire);
    for field in [
        "class_tag_offset",
        "record_index_offset",
        "byte_offset",
        "next_byte_offset",
    ] {
        let mut invalid = serde_json::to_value(&record).unwrap();
        invalid[field] = serde_json::json!(0);
        assert!(
            serde_json::from_value::<crate::records::references::LostEdgeReference>(invalid)
                .unwrap_err()
                .to_string()
                .contains(field)
        );
    }
    let mut invalid = serde_json::to_value(&record).unwrap();
    invalid["record_byte_offset"] = serde_json::json!(u64::MAX - 47);
    assert!(
        serde_json::from_value::<crate::records::references::LostEdgeReference>(invalid)
            .unwrap_err()
            .to_string()
            .contains("record_byte_offset")
    );
    let last = crate::records::references::LostEdgeReference::new(
        "edge".into(),
        u64::MAX - 48,
        "419".into(),
        299,
        "326".into(),
        300,
    )
    .unwrap();
    assert_eq!(last.next_byte_offset(), Some(u64::MAX));
    assert!(crate::records::references::LostEdgeReference::new(
        "edge".into(),
        u64::MAX - 47,
        "419".into(),
        299,
        "326".into(),
        300
    )
    .is_err());
}

#[test]
fn lost_edge_reference_requires_three_digit_class_tags() {
    for invalid in ["", "12", "1234", "12a", "１２３"] {
        assert!(crate::records::references::LostEdgeReference::new(
            "edge".into(),
            0,
            invalid.into(),
            0,
            "000".into(),
            0
        )
        .unwrap_err()
        .contains("class_tag"));
        assert!(crate::records::references::LostEdgeReference::new(
            "edge".into(),
            0,
            "000".into(),
            0,
            invalid.into(),
            0
        )
        .unwrap_err()
        .contains("next_class_tag"));
        assert!(
            serde_json::from_value::<crate::records::references::DesignClassTag>(
                serde_json::json!(invalid)
            )
            .unwrap_err()
            .to_string()
            .contains("class_tag")
        );
    }
    for value in ["000", "019", "999"] {
        let tag: crate::records::references::DesignClassTag =
            serde_json::from_value(serde_json::json!(value)).unwrap();
        assert_eq!(tag.as_str(), value);
        assert_eq!(serde_json::to_value(tag).unwrap(), serde_json::json!(value));
    }
}

#[test]
fn sketch_placement_source_forms_conserve_transform_in_payload_free_document() {
    use crate::records::sketch_placement::{
        DesignSketchFrame, DesignSketchFrameForm as Form, DesignSketchPlacement,
        SketchPlacementMatrix,
    };
    let mut rows = crate::records::identity::IDENTITY_MATRIX;
    rows[0][3] = 12.0;
    let matrix = SketchPlacementMatrix::try_from(rows).unwrap();
    for (form, length, offset) in [
        (Form::ScopeCompact, 201, None),
        (Form::ScopeGenesisCompact, 213, None),
        (Form::ScopeLegacy305(matrix), 305, Some(148)),
        (Form::ScopeLegacy325(matrix), 325, Some(148)),
        (Form::ScopeExplicit(matrix), 329, Some(155)),
        (Form::ScopeGenesisExplicit(matrix), 341, Some(166)),
        (
            Form::MemberCompact {
                paired_byte_offset: 50,
            },
            34,
            None,
        ),
        (
            Form::MemberExplicit {
                paired_byte_offset: 50,
                transform: matrix,
            },
            162,
            Some(122),
        ),
    ] {
        let source = DesignSketchFrame::new(100, form).unwrap();
        let placement = DesignSketchPlacement {
            id: "f3d:native:sketch-placement#0".into(),
            scope_record_index: Some(1),
            entity_id: "Sketch_7".to_owned().try_into().unwrap(),
            visibility: None,
            class_tag: "300".to_owned().try_into().unwrap(),
            record_index: 1,
            paired_class_tag: "301".to_owned().try_into().unwrap(),
            frame: source,
        };
        assert_eq!(placement.frame_length(), Some(length));
        assert_eq!(placement.transform_offset(), offset);
        let wire = serde_json::to_value(&placement).unwrap();
        let document: DesignSketchPlacement = serde_json::from_value(wire.clone()).unwrap();
        let mut expected = *placement.transform();
        if placement.member_run_head() || length == 213 || length == 341 {
            for row in expected.iter_mut().take(3) {
                row[3] *= 10.0;
            }
        }
        assert_eq!(document.transform(), &expected);
        assert_eq!(document.member_run_head(), placement.member_run_head());
        assert!(document.byte_offset().is_none());
        assert_eq!(serde_json::to_value(document).unwrap(), wire);
        let mut invalid = wire;
        invalid["transform"][0][0] = 2.0.into();
        assert!(serde_json::from_value::<DesignSketchPlacement>(invalid).is_err());
    }
}

#[test]
fn sketch_placement_extent_and_matrix_are_checked_at_construction() {
    use crate::records::sketch_placement::{
        DesignSketchFrame, DesignSketchFrameForm, SketchPlacementMatrix,
    };
    assert!(
        DesignSketchFrame::new(u64::MAX - 200, DesignSketchFrameForm::ScopeCompact)
            .unwrap_err()
            .contains("byte_offset")
    );
    let end = DesignSketchFrame::new(u64::MAX - 201, DesignSketchFrameForm::ScopeCompact).unwrap();
    assert_eq!(end.paired_byte_offset(), Some(u64::MAX));
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut matrix = crate::records::identity::IDENTITY_MATRIX;
        matrix[0][3] = invalid;
        assert!(SketchPlacementMatrix::try_from(matrix)
            .unwrap_err()
            .contains("transform"));
    }
    let mut matrix = crate::records::identity::IDENTITY_MATRIX;
    matrix[3][0] = 1.0;
    assert!(SketchPlacementMatrix::try_from(matrix)
        .unwrap_err()
        .contains("transform"));
    let mut matrix = crate::records::identity::IDENTITY_MATRIX;
    matrix[0][1] = 1.0;
    matrix[1][1] = 0.0;
    assert!(SketchPlacementMatrix::try_from(matrix)
        .unwrap_err()
        .contains("transform"));
}

#[test]
fn timeline_frame_rejects_invalid_actual_source_spans() {
    use crate::records::entity_header::DesignTimelineFrame;
    use crate::records::identity::Located;
    let admit = |base, length, context, count, offsets: Vec<u64>| {
        DesignTimelineFrame::new(
            base,
            length,
            context,
            count,
            offsets
                .into_iter()
                .enumerate()
                .map(|(index, offset)| Located {
                    value: 101 + u64::try_from(index).unwrap(),
                    offset,
                })
                .collect(),
        )
    };
    assert!(admit(200, 100, 220, 240, vec![245, 278]).is_ok());
    for (length, context, count, offsets, diagnostic) in [
        (u64::MAX, 220, 240, vec![245, 278], "frame_length"),
        (100, 200, 240, vec![245, 278], "context_record_index_offset"),
        (100, 231, 240, vec![245, 278], "context_record_index_offset"),
        (
            100,
            u64::MAX,
            240,
            vec![245, 278],
            "context_record_index_offset",
        ),
        (100, 220, 297, vec![245, 278], "item_count_offset"),
        (100, 220, u64::MAX, vec![245, 278], "item_count_offset"),
        (100, 220, 240, vec![0, 0], "item_record_index_offsets"),
        (100, 220, 240, vec![244, 278], "item_record_index_offsets"),
        (100, 220, 240, vec![245, 255], "item_record_index_offsets"),
        (100, 220, 240, vec![245, 291], "item_record_index_offsets"),
        (
            100,
            220,
            240,
            vec![245, u64::MAX],
            "item_record_index_offsets",
        ),
    ] {
        let error = admit(200, length, context, count, offsets).unwrap_err();
        assert!(error.contains(diagnostic), "{error}");
    }
    let empty = DesignTimelineFrame::new(200, 44, 220, 240, Vec::new()).unwrap();
    assert!(empty.items().is_empty());
}

#[test]
fn sketch_relation_definition_preserves_masks_and_rejects_mismatched_payloads() {
    use crate::records::sketch_relations::{
        SketchPatternDefinition as Kind, SketchRelationDefinition as Definition,
    };
    let patterns = [
        (
            0x1000_0000,
            Some(Kind::Circular {
                angle_parameter: 2,
                count_parameter: 3,
                evaluated_angle: cadmpeg_ir::scalar::FiniteReal::new(1.5).unwrap(),
                evaluated_count: crate::records::sketch_relations::SketchPatternCount::try_from(2)
                    .unwrap(),
            }),
        ),
        (
            0x2000_0000,
            Some(Kind::Rectangular {
                directions: std::array::from_fn(|_| {
                    crate::records::sketch_relations::SketchPatternDirection {
                        count_parameter: 2,
                        distance_parameter: 3,
                        evaluated_count:
                            crate::records::sketch_relations::SketchPatternCount::try_from(2)
                                .unwrap(),
                        direction: [1.0, 0.0, 0.0].try_into().unwrap(),
                        evaluated_distance: cadmpeg_ir::scalar::FiniteReal::new(1.5).unwrap(),
                    }
                }),
            }),
        ),
        (0x100_0000_0000, Some(Kind::TextFrame { text_reference: 2 })),
        (
            0x200_0000_0000,
            Some(Kind::TextPath {
                text_reference: 2,
                glyph_transforms: Vec::new(),
            }),
        ),
    ];
    for (mask, kind) in &patterns {
        for unknown in [0, 0x4000, 1 << 63] {
            let definition = Definition::new(mask | unknown, kind.clone()).unwrap();
            assert_eq!(definition.state(), mask | unknown);
            assert_eq!(definition.pattern(), kind.as_ref());
        }
        assert!(Definition::new(*mask, None).is_err());
        assert!(Definition::new(0, kind.clone()).is_err());
        assert!(Definition::new(mask | 1, kind.clone()).is_err());
        for (other_mask, _) in &patterns {
            if other_mask != mask {
                assert!(Definition::new(*other_mask, kind.clone()).is_err());
            }
        }
    }
    for state in [0, 1, 0x11, 0x4000, 0x8000_0000, 0x20_0000_0000, 0x1000_0001] {
        assert_eq!(Definition::new(state, None).unwrap().state(), state);
    }
    let wire = r#"{"id":"relation","record_index":1,"class_tag":"000","owner_reference":1,"owner_entity_id":"owner","auxiliary_references":[],"rectangular_counted_reference_count":0,"members":[],"resolved_members":[],"state":1099511627776,"member_relation_ordinals":[],"entity_genesis":null,"pattern":{"kind":"text_frame","text_reference":2},"return_members":[],"resolved_return_members":[]}"#;
    let relation: crate::records::sketch_relations::SketchRelation =
        serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&relation).unwrap(), wire);
    let mut invalid: serde_json::Value = serde_json::from_str(wire).unwrap();
    invalid["pattern"] = serde_json::Value::Null;
    assert!(
        serde_json::from_value::<crate::records::sketch_relations::SketchRelation>(invalid)
            .unwrap_err()
            .to_string()
            .contains("pattern")
    );
}

#[test]
fn sketch_relation_runs_reject_partial_resolution_and_preserve_atomic_binding() {
    use crate::records::sketch_relations::{
        SketchRelationMembers, SketchRelationOperand, SketchRelationReturnMembers,
    };
    let unresolved = SketchRelationMembers::from_indices([(1, 25, 3), (2, 40, 5)]).unwrap();
    let mut resolved = unresolved.clone();
    resolved.resolve(|record_index| SketchRelationOperand::Record { record_index });
    for (row, (index, offset, ordinal)) in resolved.iter().zip([(1, 25, 3), (2, 40, 5)]) {
        assert_eq!(row.reference.record_index(), index);
        assert_eq!(
            row.reference.resolved(),
            Some(&SketchRelationOperand::Record {
                record_index: index
            })
        );
        assert_eq!(
            (row.offset, row.relation_ordinal),
            (Some(offset), Some(ordinal))
        );
    }
    assert!(
        SketchRelationMembers::try_from(vec![unresolved[0].clone(), resolved[1].clone()]).is_err()
    );
    assert!(
        SketchRelationMembers::try_from(vec![resolved[0].clone(), unresolved[1].clone()]).is_err()
    );
    let unresolved_return = SketchRelationReturnMembers::from_indices([(2, 60), (1, 75)]).unwrap();
    let mut resolved_return = unresolved_return.clone();
    resolved_return.resolve(|record_index| SketchRelationOperand::Record { record_index });
    assert!(SketchRelationReturnMembers::try_from(vec![
        unresolved_return[0].clone(),
        resolved_return[1].clone()
    ])
    .is_err());
    assert!(SketchRelationReturnMembers::try_from(vec![
        resolved_return[0].clone(),
        unresolved_return[1].clone()
    ])
    .is_err());
    let mut interrupted = unresolved.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        interrupted.resolve(|record_index| {
            assert_ne!(record_index, 2, "interrupt the second resolution");
            SketchRelationOperand::Record { record_index }
        });
    }));
    assert!(result.is_err());
    assert_eq!(interrupted, unresolved);
    let mut interrupted_return = unresolved_return.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        interrupted_return.resolve(|record_index| {
            assert_ne!(record_index, 1, "interrupt the second resolution");
            SketchRelationOperand::Record { record_index }
        });
    }));
    assert!(result.is_err());
    assert_eq!(interrupted_return, unresolved_return);
}

#[test]
fn glyph_transform_preserves_finite_matrix_wire_without_an_affine_restriction() {
    for wire in [
        "[[1.0,0.0,0.0,0.5],[0.0,1.0,0.0,-0.0],[0.0,0.0,1.0,-2.5],[0.0,0.0,0.0,1.0]]",
        "[[0.0,0.0,0.0,0.0],[0.0,0.0,0.0,0.0],[0.0,0.0,0.0,0.0],[0.0,0.0,0.0,1.0]]",
        "[[1.0,0.0,0.0,0.5],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,-2.5],[0.0,0.0,0.0,2.0]]",
    ] {
        let transform: crate::records::sketch_relations::SketchGlyphTransform =
            serde_json::from_str(wire).unwrap();
        assert_eq!(serde_json::to_string(&transform).unwrap(), wire);
        assert_eq!(
            crate::records::sketch_relations::SketchGlyphTransform::try_from(transform.rows())
                .unwrap(),
            transform
        );
    }
    for ordinal in 0..16 {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut rows = [[0.0; 4]; 4];
            rows[ordinal / 4][ordinal % 4] = value;
            assert!(
                crate::records::sketch_relations::SketchGlyphTransform::try_from(rows)
                    .unwrap_err()
                    .to_string()
                    .contains("glyph_transforms")
            );
        }
    }
}

#[test]
fn sketch_text_layout_preserves_flat_wire_pairs_and_independent_references() {
    use crate::records::sketch_geometry::SketchText;

    let prefix = r#"{"id":"text","record_index":1,"owner_reference":2,"class_tag":"000","class_version":4,"text":"text","font_family":"Arial","font_weight":400,"height":10.0"#;
    let frame = serde_json::to_string(&crate::records::source_frame::SourceFrame::from(
        Vec::<u8>::new(),
    ))
    .unwrap();
    let color = r#","color":{"r":0.0,"g":0.0,"b":0.0,"a":1.0}"#;
    let placement = r#","anchor":{"u":2.0,"v":3.0},"rotation":0.5"#;
    let alignment = r#","horizontal_alignment":3,"vertical_alignment":7"#;
    let txt = format!("{prefix}{color}{placement},\"frame\":{frame}}}");
    let decoded: SketchText = serde_json::from_str(&txt).expect("txt placement");
    assert_eq!(serde_json::to_string(&decoded).unwrap(), txt);
    let mut source = decoded.clone();
    source.byte_offset = Some(0);
    source.raw_bytes = Vec::<u8>::new().into();
    assert_eq!(source.byte_offset, Some(0));
    assert_eq!(serde_json::to_string(&source).unwrap(), txt);
    for placement in ["", placement] {
        for alignment in ["", alignment] {
            for first_reference in ["", ",\"first_reference\":0", ",\"first_reference\":12"] {
                for second_reference in ["", ",\"second_reference\":0", ",\"second_reference\":13"]
                {
                    let wire = format!(
                        "{prefix},\"width_factor\":1.0{color}{placement}{alignment}{first_reference}{second_reference},\"frame\":{frame}}}"
                    );
                    let decoded: SketchText =
                        serde_json::from_str(&wire).expect("textex member pairs");
                    assert_eq!(serde_json::to_string(&decoded).unwrap(), wire);
                }
            }
        }
    }
}

#[test]
fn sketch_text_layout_rejects_partial_placement_and_alignment() {
    use crate::records::sketch_geometry::SketchText;

    let base = serde_json::json!({
        "id": "text", "record_index": 1, "owner_reference": 2,
        "class_tag": "000", "class_version": 4,
        "text": "text", "font_family": "Arial", "font_weight": 400,
        "height": 10.0, "width_factor": 1.0,
        "color": {"r": 0.0, "g": 0.0, "b": 0.0, "a": 1.0},
        "frame": serde_json::to_value(crate::records::source_frame::SourceFrame::from(Vec::<u8>::new())).unwrap()
    });
    for (field, value) in [
        ("anchor", serde_json::json!({"u": 2.0, "v": 3.0})),
        ("rotation", serde_json::json!(0.5)),
        ("horizontal_alignment", serde_json::json!(3)),
        ("vertical_alignment", serde_json::json!(7)),
    ] {
        let mut wire = base.clone();
        wire[field] = value;
        let error = serde_json::from_value::<SketchText>(wire).unwrap_err();
        assert!(error.to_string().contains(field), "{error}");
    }
}

#[test]
fn sketch_point_flags_preserve_numeric_wire_and_reject_non_boolean_values() {
    use crate::records::sketch_geometry::SketchPoint;
    use serde_json::json;

    for (form, count) in [
        (json!({"kind": "version0"}), 1),
        (json!({"kind": "version8"}), 7),
        (json!({"kind": "version10"}), 7),
        (
            json!({"kind": "version10_inline_typed", "trailing_reference": 3}),
            7,
        ),
        (
            json!({"kind": "version11", "padded_paired_reference": false,
                "companion_prefix_present_zero": false}),
            8,
        ),
        (
            json!({"kind": "version11_inline_typed", "trailing_reference": 3,
                "companion_prefix_present_zero": false}),
            8,
        ),
    ] {
        let mut base = json!({
            "id": "point", "record_index": 1, "class_tag": "000",
            "byte_offset": 0, "coordinate_offset": 1, "record_form": form,
            "paired_reference": 2, "coordinates": {"u": 2.0, "v": 3.0}, "depth": 0.0
        });
        base["companion"] = json!({"incident_curves": []});
        if count > 1 {
            base["persistent_id"] = json!(4);
            base["closure"] = json!({"selector": 0, "state": 0});
        }
        let point: SketchPoint = serde_json::from_value(base.clone()).expect("omitted zero flags");
        assert_eq!(serde_json::to_value(point).unwrap(), base);
        for prefix_present_zero in [false, true] {
            let mut wire = base.clone();
            wire["companion"] = json!({"incident_curves": [7, 2]});
            if count == 8 {
                wire["record_form"]["companion_prefix_present_zero"] = json!(prefix_present_zero);
            } else if prefix_present_zero {
                continue;
            }
            let point =
                serde_json::from_value::<SketchPoint>(wire.clone()).expect("companion wire");
            let companion = point.companion();
            assert_eq!(companion.prefix_present_zero, prefix_present_zero);
            assert_eq!(companion.incident_curves, [7, 2]);
            assert_eq!(serde_json::to_value(point).unwrap(), wire);
        }
        for depth in [-7.5, 2.5] {
            let mut wire = base.clone();
            wire["depth"] = json!(depth);
            let decoded = serde_json::from_value::<SketchPoint>(wire.clone());
            if count > 1 {
                let point = decoded.expect("three-dimensional point form");
                assert_eq!(point.depth(), depth);
                assert_eq!(serde_json::to_value(point).unwrap(), wire);
            } else {
                let error = decoded.unwrap_err();
                assert!(error.to_string().contains("depth"), "{error}");
            }
        }
        for genesis in [0, 9, u64::MAX] {
            let mut wire = base.clone();
            wire["entity_genesis"] = json!(genesis);
            let decoded = serde_json::from_value::<SketchPoint>(wire.clone());
            if count == 8 {
                let point = decoded.expect("version-11 genesis");
                assert_eq!(point.entity_genesis(), Some(genesis));
                assert_eq!(serde_json::to_value(point).unwrap(), wire);
            } else {
                let error = decoded.unwrap_err();
                assert!(error.to_string().contains("entity_genesis"), "{error}");
            }
        }
        for lane in 0..8 {
            for value in [1, 2, 255] {
                let mut wire = base.clone();
                let mut flags = [0; 8];
                flags[lane] = value;
                wire["flags"] = json!(cadmpeg_ir::native::bytes::NativeBytes::from(flags));
                let decoded = serde_json::from_value::<SketchPoint>(wire.clone());
                if lane < count && value == 1 {
                    assert_eq!(
                        serde_json::to_value(decoded.expect("boolean flag")).unwrap(),
                        wire
                    );
                } else {
                    let error = decoded.unwrap_err();
                    assert!(error.to_string().contains("flags"), "{error}");
                }
            }
        }
    }
}

#[test]
fn empty_reference_runs_have_one_representation() {
    use crate::records::identity::{Located, ReferenceRun};

    let empty = ReferenceRun::<u32>::unlocated(Vec::new());
    assert_eq!(empty, ReferenceRun::located(Vec::new()));
    assert_eq!(empty.located_rows(), Some([].as_slice()));
    assert_eq!(
        ReferenceRun::<u32>::from_columns(Vec::new(), Vec::new(), "field").expect("empty columns"),
        ReferenceRun::located(Vec::new())
    );

    let unlocated = ReferenceRun::unlocated(vec![7u32]);
    assert!(unlocated.located_rows().is_none());
    assert_ne!(
        unlocated,
        ReferenceRun::located(vec![Located {
            value: 7u32,
            offset: 0
        }])
    );

    let header = crate::records::entity_header::DesignEntityHeader {
        id: "header".into(),
        byte_offset: Some(10),
        entity_id: crate::records::identity::DesignEntityId::from_parts("Sketch", 7),
        class_tag: "256".to_owned().try_into().unwrap(),
        optional_slot_present: Some(false),
        registration: crate::records::entity_header::DesignEntityRegistration::new(
            Some("MSketch".into()),
            Some(crate::records::entity_header::SketchHeaderReferences {
                record_reference: None,
                record_reference_offset: Some(20),
                references: Vec::new(),
            }),
            ReferenceRun::unlocated(Vec::new()),
        )
        .unwrap(),
    };
    let wire = r#"{"id":"header","entity_id":"Sketch_7","class_tag":"256","module":"MSketch","reference_indices":[]}"#;
    assert_eq!(serde_json::to_string(&header).unwrap(), wire);
    assert!(header.content_eq(
        &serde_json::from_str::<crate::records::entity_header::DesignEntityHeader>(wire).unwrap()
    ));
    let mut fidelity = cadmpeg_ir::SourceFidelity::default();
    header.retain_source(&mut fidelity);
    assert_eq!(fidelity.metadata["header.byte_offset"], "10");
    assert_eq!(fidelity.metadata["header.optional_slot_present"], "false");
    assert_eq!(fidelity.metadata["header.record_reference_offset"], "20");
}

#[test]
fn dimension_locus_pairs_preserve_both_semantic_forms_and_reject_a_stray_opaque_index() {
    let base = serde_json::json!({
        "id":"f3d:test:dimension-locus-pair#0", "companion_record_index":1,
        "governing_companion_record_index":2, "class_tag":"274", "record_index":3,
        "first_geometry_record_index":0, "first_role":0,
        "second_geometry_record_index":41, "second_role":1, "paired_class_tag":"273"
    });
    for (opaque, first, valid) in [
        (Some(4), 40, true),
        (None, 0, true),
        (Some(4), 0, false),
        (None, 40, false),
    ] {
        let mut wire = base.clone();
        wire["first_geometry_record_index"] = first.into();
        if let Some(opaque) = opaque {
            wire["opaque_index"] = opaque.into();
        }
        let parsed = serde_json::from_value::<crate::records::dimensions::DesignDimensionLocusPair>(
            wire.clone(),
        );
        if valid {
            let pair = parsed.unwrap();
            assert!(pair.byte_offset().is_none());
            assert_eq!(serde_json::to_value(pair).unwrap(), wire);
        } else {
            assert!(parsed.is_err(), "{wire}");
        }
    }
    for field in [
        "byte_offset",
        "frame_length",
        "opaque_index_offset",
        "first_role_offset",
        "paired_byte_offset",
    ] {
        let mut wire = base.clone();
        wire[field] = 0.into();
        assert!(
            serde_json::from_value::<crate::records::dimensions::DesignDimensionLocusPair>(wire)
                .is_err()
        );
    }
}

#[test]
fn null_locus_arena_preserves_base_wire_fields_and_order() {
    let entry = r#"{"id":"f3d:test:dimension-locus-pair#0","companion_record_index":1,"governing_companion_record_index":2,"class_tag":"274","record_index":3,"null_role":0,"geometry_record_index":41,"geometry_role":1,"paired_class_tag":"273"}"#;
    let mut wire = serde_json::to_value(crate::native::F3dNative::default()).unwrap();
    wire["design_dimension_null_locus_pairs"] =
        serde_json::from_str(&format!("[{entry}]")).unwrap();
    let native: crate::native::F3dNative = serde_json::from_value(wire).unwrap();
    let encoded = serde_json::to_string(&native).unwrap();
    assert!(encoded.contains(&format!(r#""design_dimension_null_locus_pairs":[{entry}]"#)));
    let decoded: crate::native::F3dNative = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, native);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            &mut namespace,
        )
        .unwrap();
    let arena: Vec<crate::records::dimension_null_locus_wire::Wire> = namespace
        .arena_as("design_dimension_null_locus_pairs")
        .unwrap();
    assert_eq!(serde_json::to_string(&arena).unwrap(), format!("[{entry}]"));
    assert_eq!(
        crate::test_support::with_decode_policy(
            &cadmpeg_core::decode::DecodePolicy::default(),
            |ctx| crate::native::F3dNative::load(ctx, &namespace)
        )
        .unwrap(),
        native
    );
}

#[test]
fn segment_type_guid_preserves_relaxed_text_and_rejects_invalid_text() {
    for guid in ["g".repeat(36), "_".repeat(38), "bad".into()] {
        let wire = format!(
            r#"{{"id":"f3d:native/MetaStream.dat:design-type#0","class_tag":"256","type_guid":"{guid}","version":1,"module":"Fusion","entity_ids":[]}}"#
        );
        let decoded = serde_json::from_str::<crate::records::entity_header::SegmentType>(&wire);
        if guid == "bad" {
            assert!(decoded.is_err());
        } else {
            assert_eq!(serde_json::to_string(&decoded.unwrap()).unwrap(), wire);
        }
    }
}

#[test]
fn segment_base_guid_rejects_invalid_text() {
    let wire = r#"{"id":"f3d:native/MetaStream.dat:design-type#0","class_tag":"256","type_guid":"11111111-2222-3333-4444-555555555555","base_type_guid":"invalid","version":1,"module":"Fusion","entity_ids":[]}"#;
    let error =
        serde_json::from_str::<crate::records::entity_header::SegmentType>(wire).unwrap_err();
    assert!(error.to_string().contains("base_type_guid"));
}

#[test]
fn material_assignment_sidecar_rejects_invalid_visual_guid() {
    let wire = serde_json::json!({
        "id": "material#0", "body_binding_id":"f3d:native:design-body-binding#0",
        "asm_body_key": 42, "entity_id": "0_985", "visual_guid": "not-a-guid"
    });
    assert!(serde_json::from_value::<super::references::DesignMaterialAssignment>(wire).is_err());
}

#[test]
fn visual_token_wire_preserves_revision_spelling() {
    for text in [
        "abcdef01-2222-3333-4444-555555555555",
        "ABCDEF01-2222-3333-4444-555555555555_Post2015_Post2015",
    ] {
        let wire = serde_json::json!(text);
        let token: super::references::DesignVisualToken =
            serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(token).unwrap(), wire);
    }
    for text in [
        "",
        "not-a-guid",
        "abcdef01-2222-3333-4444-555555555555_post2015",
    ] {
        assert!(
            serde_json::from_value::<super::references::DesignVisualToken>(serde_json::json!(text))
                .is_err()
        );
    }
}

#[test]
fn sketch_link_sidecar_rejects_present_sentinels() {
    for sense in [-1, super::sketch_links::SKETCH_LINK_SENSE_UNCONSTRAINED] {
        let wire = serde_json::json!({"id": "link", "target": {"kind": "document"},
            "sketch_curve_id": 1, "ref_b": 0, "sense": sense, "role": 0, "closure": 0});
        assert!(serde_json::from_value::<super::sketch_links::SketchCurveLink>(wire).is_err());
    }
}

#[test]
fn sketch_link_sidecar_preserves_all_non_sentinel_senses() {
    for sense in [i64::MIN, -2, 0, 1, 2, i64::MAX] {
        let wire = serde_json::json!({"id": "link", "target": {"kind": "document"},
            "sketch_curve_id": 1, "ref_b": 0, "sense": sense, "role": 0, "closure": 0});
        let record: super::sketch_links::SketchCurveLink =
            serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
    }
}

#[test]
fn sketch_pattern_count_enforces_both_bounds() {
    for value in [0, 100_001, u32::MAX] {
        assert!(super::sketch_relations::SketchPatternCount::try_from(value).is_err());
        let error =
            serde_json::from_value::<super::sketch_relations::SketchPatternCount>(value.into())
                .unwrap_err();
        assert!(error.to_string().contains("evaluated_count"));
    }
    for value in [1, 100_000] {
        let count = super::sketch_relations::SketchPatternCount::try_from(value).unwrap();
        assert_eq!(count.get(), value);
        assert_eq!(
            serde_json::to_value(count).unwrap(),
            serde_json::json!(value)
        );
    }
}

#[test]
fn sketch_identity_sidecar_rejects_zero() {
    let point = serde_json::json!({"id": "point", "record_index": 1, "class_tag": "000",
        "byte_offset": 0, "coordinate_offset": 1, "record_form": {"kind": "version8"},
        "paired_reference": 2, "coordinates": {"u": 2.0, "v": 3.0}, "depth": 0.0,
        "persistent_id": 0, "closure": {"selector": 0, "state": 0},
        "companion": {"incident_curves": []}});
    assert!(
        serde_json::from_value::<super::sketch_geometry::SketchPoint>(point)
            .unwrap_err()
            .to_string()
            .contains("persistent_id")
    );
    let curve = serde_json::json!({"id": "curve", "record_index": 1, "class_tag": "000",
        "primary_id": 0, "secondary_id": 0});
    assert!(serde_json::from_value::<super::sketch_geometry::SketchCurveIdentity>(curve).is_err());
    let surface = serde_json::json!({"id": "surface", "record_index": 1, "class_tag": "000",
        "byte_offset": 0, "persistent_id": 0, "u_degree": 0, "v_degree": 0,
        "u_knots": [], "v_knots": [], "control_points": []});
    assert!(serde_json::from_value::<super::sketch_geometry::SketchSurface>(surface).is_err());
}

#[test]
fn sketch_point_admission_and_edits_keep_finite_coordinates() {
    let draft = super::sketch_geometry::SketchPointDraft {
        id: "point".into(),
        record_index: 1,
        owner_reference: None,
        class_tag: "000".to_owned().try_into().unwrap(),
        byte_offset: Some(0),
        coordinate_offset: 0,
        companion: crate::records::sketch_geometry::SketchPointCompanion {
            incident_curves: Vec::new(),
        },
        record_form: super::sketch_geometry::SketchPointRecordForm::version11(
            1,
            super::sketch_geometry::SketchPointClosure::Selector0State0,
            None,
            -2.0,
        ),
        paired_reference: 2,
        coordinates: cadmpeg_ir::math::Point2::new(1.0, -1.0),
    };
    let original = super::sketch_geometry::SketchPoint::try_from(draft.clone()).unwrap();
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for coordinates in [
            cadmpeg_ir::math::Point2::new(value, 0.0),
            cadmpeg_ir::math::Point2::new(0.0, value),
        ] {
            let mut invalid = draft.clone();
            invalid.coordinates = coordinates;
            assert!(super::sketch_geometry::SketchPoint::try_from(invalid).is_err());
            let mut point = original.clone();
            assert!(point.try_set_coordinates(coordinates).is_err());
            assert_eq!(point, original);
        }
        let mut form = draft.record_form.clone();
        let super::sketch_geometry::SketchPointRecordForm::Version11 { depth, .. } = &mut form
        else {
            panic!("version 11");
        };
        *depth = value;
        let mut invalid = draft.clone();
        invalid.record_form = form.clone();
        assert!(super::sketch_geometry::SketchPoint::try_from(invalid).is_err());
        let mut point = original.clone();
        assert!(point.try_set_record_form(form).is_err());
        assert_eq!(point, original);
        let mut wire = super::sketch_geometry::SketchPointSerde::from(original.clone());
        wire.depth = value;
        assert!(super::sketch_geometry::SketchPoint::try_from(wire).is_err());
    }
}

#[test]
fn sketch_point_requires_a_distinct_companion_on_every_route() {
    let mut wire = serde_json::json!({"id": "point", "record_index": 1, "class_tag": "000",
        "byte_offset": 0, "coordinate_offset": 1, "record_form": {"kind": "version8"},
        "paired_reference": 2, "coordinates": {"u": 2.0, "v": 3.0}, "depth": 0.0,
        "persistent_id": 1, "closure": {"selector": 0, "state": 0},
        "companion": {"incident_curves": []}});
    let original: super::sketch_geometry::SketchPoint =
        serde_json::from_value(wire.clone()).unwrap();
    assert!(original.companion().incident_curves.is_empty());
    assert_eq!(serde_json::to_value(&original).unwrap(), wire);
    wire["companion"]["incident_curves"] = serde_json::json!([7, 7]);
    assert!(
        serde_json::from_value::<super::sketch_geometry::SketchPoint>(wire.clone())
            .unwrap_err()
            .to_string()
            .contains("incident_curves")
    );
    wire.as_object_mut().unwrap().remove("companion");
    assert!(
        serde_json::from_value::<super::sketch_geometry::SketchPoint>(wire)
            .unwrap_err()
            .to_string()
            .contains("companion")
    );
    let duplicate = super::sketch_geometry::SketchPointCompanion {
        incident_curves: vec![7, 7],
    };
    let draft = super::sketch_geometry::SketchPointDraft {
        id: "point".into(),
        record_index: 1,
        owner_reference: None,
        class_tag: "000".to_owned().try_into().unwrap(),
        byte_offset: Some(0),
        coordinate_offset: 0,
        record_form: original.record_form().clone().into_raw(),
        paired_reference: 2,
        coordinates: original.coordinates(),
        companion: duplicate.clone(),
    };
    assert!(super::sketch_geometry::SketchPoint::try_from(draft).is_err());
    let mut point = original.clone();
    assert!(point.try_set_companion(duplicate).is_err());
    assert_eq!(point, original);
}

#[test]
fn body_bounds_admit_only_ordered_finite_cache_frames() {
    let wire = serde_json::json!({"id":"f3d:Design/BulkStream.dat:design-body-bounds#0","entity_suffix":10,
        "record_indices":[11,12,13],"maximum":{"x":1.0,"y":0.0,"z":0.0},"minimum":{"x":0.0,"y":0.0,"z":0.0}});
    let record: super::bodies::DesignBodyBounds = serde_json::from_value(wire.clone()).unwrap();
    assert!(record.entity_byte_offset().is_none());
    assert_eq!(serde_json::to_value(&record).unwrap(), wire);
    for (field, value) in [
        ("entity_suffix", serde_json::json!(u64::MAX)),
        ("entity_suffix", serde_json::json!(u32::MAX)),
        ("record_indices", serde_json::json!([11, 12, 14])),
        ("maximum", serde_json::json!({"x":-1.0,"y":0.0,"z":0.0})),
        ("maximum", serde_json::json!({"x":0.0,"y":0.0,"z":0.0})),
    ] {
        let mut invalid = wire.clone();
        invalid[field] = value;
        assert!(
            serde_json::from_value::<super::bodies::DesignBodyBounds>(invalid).is_err(),
            "{field}"
        );
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut invalid = super::bodies::DesignBodyBoundsContent::from(record.clone());
        invalid.maximum.x = value;
        assert!(super::bodies::DesignBodyBounds::try_from(invalid).is_err());
    }
    let mut source = wire;
    source["entity_byte_offset"] = 0.into();
    source["record_byte_offsets"] = serde_json::json!([20, 40, 60]);
    source["value_byte_offsets"] = serde_json::json!([21, 41, 61]);
    let admit = |value| {
        super::bodies::DesignBodyBounds::try_from(
            serde_json::from_value::<super::bodies::DesignBodyBoundsWire>(value).unwrap(),
        )
    };
    assert_eq!(admit(source.clone()).unwrap().entity_byte_offset(), Some(0));
    for (field, value) in [
        ("record_byte_offsets", serde_json::json!([20, 20, 60])),
        ("value_byte_offsets", serde_json::json!([20, 41, 61])),
    ] {
        let mut invalid = source.clone();
        invalid[field] = value;
        assert!(admit(invalid).is_err(), "{field}");
    }
}

#[test]
fn body_binding_admits_semantic_map_order_but_checks_actual_source_pair_locations() {
    let wire = serde_json::json!({"id":"f3d:Design/BulkStream.dat:design-body-binding#10","stream":"Design/BulkStream.dat",
        "map_ordinal":0,"pair_count":1,"pair_ordinal":0,"asm_body_key":0,"entity_suffix":0,"blob_name":"BREP."});
    let record: super::bodies::DesignBodyBinding = serde_json::from_value(wire.clone()).unwrap();
    assert!(record.asm_body_key_offset().is_none());
    assert_eq!(serde_json::to_value(record).unwrap(), wire);
    for (field, value) in [
        ("pair_count", serde_json::json!(0)),
        ("pair_ordinal", serde_json::json!(1)),
        ("blob_name", serde_json::json!("body.smbh")),
    ] {
        let mut invalid = wire.clone();
        invalid[field] = value;
        assert!(
            serde_json::from_value::<super::bodies::DesignBodyBinding>(invalid).is_err(),
            "{field}"
        );
    }
    let mut source = wire;
    source["asm_body_key_offset"] = 10.into();
    source["entity_suffix_offset"] = 18.into();
    source["blob_name_offset"] = 19.into();
    let admit = |value| {
        super::bodies::DesignBodyBinding::try_from(
            serde_json::from_value::<super::bodies::DesignBodyBindingWire>(value).unwrap(),
        )
    };
    assert_eq!(
        admit(source.clone()).unwrap().asm_body_key_offset(),
        Some(10)
    );
    for (field, value) in [
        ("asm_body_key_offset", u64::MAX),
        ("entity_suffix_offset", 19),
        ("blob_name_offset", 18),
    ] {
        let mut invalid = source.clone();
        invalid[field] = value.into();
        assert!(admit(invalid).is_err(), "{field}");
    }
    for field in [
        "asm_body_key_offset",
        "entity_suffix_offset",
        "blob_name_offset",
    ] {
        source[field] = u64::MAX.into();
    }
    assert!(admit(source).is_err());
}

mod act_entities;

mod native_ids;

mod persistent_text;

mod locus_frames;
