use super::*;

#[test]
fn decode_resolves_legacy_text_node_font_pointer() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(legacy_text_node_font_pointer_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let text = result.ir().native.namespace("iges").unwrap().arenas["associativities"]
        .iter()
        .find(|value| value.fields()["kind"] == "legacy_text_node")
        .unwrap();
    assert_eq!(text.fields()["font_characteristic"], -1);
    assert_eq!(text.fields()["font_definition"], "iges:entity:directory#1");
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_preserves_recalculable_dimension_geometry_points() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(recalculable_dimension_associativity_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let associativity = result.ir().native.namespace("iges").unwrap().arenas["associativities"]
        .iter()
        .find(|value| value.fields()["kind"] == "recalculable_dimension")
        .unwrap();
    assert_eq!(
        associativity.fields()["dimension"],
        "iges:entity:directory#11"
    );
    assert_eq!(associativity.fields()["orientation_flag"], 4);
    assert_eq!(associativity.fields()["declared_geometry_count"], 2);
    assert_eq!(
        associativity.fields()["geometry"].as_array().unwrap().len(),
        2
    );
    assert_eq!(
        associativity.fields()["geometry"][0]["geometry"],
        "iges:entity:directory#7"
    );
    assert_eq!(associativity.fields()["geometry"][0]["location_flag"], 0);
    assert_eq!(
        associativity.fields()["geometry"][1]["geometry"],
        "iges:entity:directory#9"
    );
    assert_eq!(associativity.fields()["geometry"][1]["location_flag"], 1);
    assert_eq!(associativity.fields()["geometry"][1]["point"][0], 4.0);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_rejects_linear_dimension_orientation_eight() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(recalculable_dimension_associativity_file_with_orientation(
                8,
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn decode_types_fundamental_units_and_property_owner() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(units_data_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let units = &result.ir().native.namespace("iges").unwrap().arenas["units_data"][0];
    assert_eq!(units.fields()["units"].as_array().unwrap().len(), 3);
    assert_eq!(units.fields()["units"][0]["unit_type"][0], 76);
    assert_eq!(
        units.fields()["units"][0]["unit_value"],
        serde_json::json!([75, 78])
    );
    assert_eq!(units.fields()["units"][0]["scale_factor"], 1852.0);
    assert_eq!(
        units.fields()["units"][2]["scale_factor"],
        0.017_453_292_519_943_295
    );
    assert_eq!(units.fields()["owners"][0], "iges:entity:directory#1");
    let owner = &result.ir().native.namespace("iges").unwrap().arenas["entities"][0];
    assert_eq!(
        owner.fields()["property_links"][0],
        "iges:entity:directory#3"
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn definition_entities_require_independent_directory_entries() {
    for (entity_type, form, parameters, description) in [
        (302, 5001, "302,1,1,1,1,1;", "associativity definition"),
        (316, 0, "316,1,6HLENGTH,2HKN,1852;", "units data"),
    ] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                    entity_type,
                    form,
                    label: "DEFIN".into(),
                    status: "00010200",
                    parameters: parameters.into(),
                }])),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert!(
            result
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()),
            "invalid {description} Directory entry was projected: {:#?}",
            result.report().losses
        );
    }
}

#[test]
fn type316_scale_is_scoped_to_the_property_owner() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(units_data_scope_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let units = &native.arenas["units_data"][0];
    assert_eq!(
        units.fields()["owners"],
        serde_json::json!(["iges:entity:directory#1"])
    );
    let unowned = native.arenas["entities"]
        .iter()
        .find(|entity| entity.fields()["directory_sequence"] == 5)
        .unwrap();
    assert!(unowned.fields()["property_links"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn decode_preserves_ordered_solid_assembly_member_placements() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(solid_assembly_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let assemblies = &result.ir().native.namespace("iges").unwrap().arenas["solid_assemblies"];
    assert_eq!(assemblies.len(), 1);
    let assembly_fields = assemblies[0].fields();
    let items = assembly_fields["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["item"], "iges:entity:directory#1");
    assert!(items[0]["transformation"].is_null());
    assert_eq!(items[1]["item"], "iges:entity:directory#3");
    assert_eq!(items[1]["transformation"], "iges:native:transformation#D5");
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_rejects_cyclic_solid_assembly_definitions() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 158,
            form: 0,
            label: "SPHERE".into(),
            status: "00000000",
            parameters: "158,1,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 184,
            form: 0,
            label: "ASSEMBL1".into(),
            status: "00000200",
            parameters: "184,2,1,5,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 184,
            form: 0,
            label: "ASSEMBL2".into(),
            status: "00000200",
            parameters: "184,2,1,3,0,0;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        result.ir().native.namespace("iges").unwrap().arenas["solid_assemblies"].len(),
        2
    );
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.message.contains(
                "solid-assembly use flag, form, members, transforms, or acyclicity is invalid"
            ))
            .count(),
        2
    );
}

#[test]
fn decode_preserves_nested_subfigure_definitions_and_instances() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(nested_subfigure_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let definitions = &native.arenas["subfigure_definitions"];
    assert_eq!(definitions.len(), 2);
    let parent = definitions
        .iter()
        .find(|definition| definition.id() == "iges:product:subfigure-definition#D7")
        .unwrap();
    assert_eq!(parent.fields()["depth"], 1);
    assert_eq!(parent.fields()["members"][0], "iges:entity:directory#5");
    let instances = &native.arenas["subfigure_instances"];
    assert_eq!(instances.len(), 2);
    let child = instances
        .iter()
        .find(|instance| instance.id() == "iges:product:subfigure-instance#D5")
        .unwrap();
    assert_eq!(
        child.fields()["definition"],
        "iges:product:subfigure-definition#D3"
    );
    assert_eq!(child.fields()["translation"][0], 1.0);
    assert_eq!(child.fields()["scale"], 0.5);
    let occurrences = &native.arenas["product_occurrences"];
    assert_eq!(occurrences.len(), 3);
    let nested = occurrences
        .iter()
        .find(|occurrence| occurrence.id() == "iges:product:occurrence#9/5")
        .unwrap();
    assert_eq!(nested.fields()["root"], false);
    assert_eq!(
        nested.fields()["instance_path"][0],
        "iges:entity:directory#9"
    );
    assert_eq!(
        nested.fields()["instance_path"][1],
        "iges:entity:directory#5"
    );
    assert_eq!(nested.fields()["world_transform"][0][0], 1.0);
    assert_eq!(nested.fields()["world_transform"][0][3], 12.0);
    assert_eq!(nested.fields()["world_transform"][1][3], 24.0);
    assert_eq!(nested.fields()["world_transform"][2][3], 36.0);
    let leaf = occurrences
        .iter()
        .find(|occurrence| occurrence.id() == "iges:product:occurrence#9/5/D1")
        .unwrap();
    assert_eq!(leaf.fields()["root"], false);
    assert_eq!(leaf.fields()["member"], "iges:entity:directory#1");
    assert_eq!(leaf.fields()["neutral_links"][0], "iges:model:curve#D1");
    assert_eq!(
        leaf.fields()["world_transform"],
        nested.fields()["world_transform"]
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn v5_applies_definition_transformations_to_subfigure_occurrences() {
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    for (definition_type, instance_type, definition_arena) in [
        (308, 408, "subfigure_definitions"),
        (320, 420, "network_definitions"),
    ] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(transformed_subfigure_definition_file(
                    definition_type,
                    instance_type,
                    global_v5,
                    0,
                    0,
                    1,
                )),
                &DecodeOptions::default(),
            )
            .unwrap();
        let native = result.ir().native.namespace("iges").unwrap();
        assert_eq!(native.arenas[definition_arena].len(), 1);
        assert_eq!(
            native.arenas[definition_arena][0].fields()["transformation"],
            "iges:native:transformation#D1"
        );
        let root = native.arenas["product_occurrences"]
            .iter()
            .find(|occurrence| occurrence.id() == "iges:product:occurrence#7")
            .unwrap();
        assert_eq!(root.fields()["root"], true);
        assert_eq!(root.fields()["world_transform"][0][3], 10.0);
        assert_eq!(root.fields()["world_transform"][1][3], 20.0);
        assert_eq!(root.fields()["world_transform"][2][3], 30.0);
        let leaf = native.arenas["product_occurrences"]
            .iter()
            .find(|occurrence| occurrence.id() == "iges:product:occurrence#7/D3")
            .unwrap();
        assert_eq!(leaf.fields()["root"], false);
        assert_eq!(leaf.fields()["world_transform"][0][3], 10.0);
        assert_eq!(leaf.fields()["world_transform"][1][3], 20.0);
        assert_eq!(leaf.fields()["world_transform"][2][3], 30.0);
        assert!(
            !result
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()),
            "{:#?}",
            result.report().losses
        );
    }
}

#[test]
fn v5_preserves_label_display_links_on_subfigure_definitions() {
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    for (definition_type, instance_type, definition_arena) in [
        (308, 408, "subfigure_definitions"),
        (320, 420, "network_definitions"),
    ] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(transformed_subfigure_definition_file(
                    definition_type,
                    instance_type,
                    global_v5,
                    0,
                    9,
                    0,
                )),
                &DecodeOptions::default(),
            )
            .unwrap();
        let native = result.ir().native.namespace("iges").unwrap();
        assert_eq!(
            native.arenas[definition_arena][0].fields()["label_display"],
            "iges:structure:associativity#D9"
        );
        assert!(
            !result
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()),
            "{:#?}",
            result.report().losses
        );
    }
}

#[test]
fn rejects_wrong_subfigure_definition_pointers_in_v4_and_v5_profiles() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    for (version, global) in [("V4", &global_v4[..]), ("V5", &global_v5[..])] {
        for (definition_type, instance_type, message) in [
            (
                308,
                408,
                "subfigure definition fields or nesting depth is invalid",
            ),
            (
                320,
                420,
                "network definition fields or nesting depth is invalid",
            ),
        ] {
            for (label_display, definition_transform, pointer_kind) in
                [(3, 0, "label display"), (0, 3, "transformation")]
            {
                let result = IgesCodec
                    .decode(
                        &mut Cursor::new(transformed_subfigure_definition_file(
                            definition_type,
                            instance_type,
                            global,
                            i64::from(version == "V4"),
                            label_display,
                            definition_transform,
                        )),
                        &DecodeOptions::default(),
                    )
                    .unwrap();
                assert!(
                    result.report().losses.iter().any(|loss| {
                        loss.code == IgesLossCode::EntityNotProjected.kind()
                            && loss.message.contains(message)
                    }),
                    "{version} {definition_type} wrong {pointer_kind}: {:#?}",
                    result.report().losses
                );
                assert!(result.ir().native.namespace("iges").unwrap().arenas
                    ["product_occurrences"]
                    .is_empty());
            }
        }
    }
}

#[test]
fn v4_preserves_label_display_links_on_subfigure_definitions() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    for (definition_type, instance_type, definition_arena) in [
        (308, 408, "subfigure_definitions"),
        (320, 420, "network_definitions"),
    ] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(transformed_subfigure_definition_file(
                    definition_type,
                    instance_type,
                    global_v4,
                    1,
                    9,
                    0,
                )),
                &DecodeOptions::default(),
            )
            .unwrap();
        let native = result.ir().native.namespace("iges").unwrap();
        assert_eq!(native.arenas[definition_arena].len(), 1);
        assert!(
            native.arenas[definition_arena][0].fields()["label_display"]
                == "iges:structure:associativity#D9",
            "{:#?}",
            result.report().losses
        );
        assert!(
            !native.arenas["product_occurrences"].is_empty(),
            "{:#?}",
            result.report().losses
        );
        assert!(
            result
                .report()
                .losses
                .iter()
                .all(|loss| { loss.code != IgesLossCode::EntityNotProjected.kind() }),
            "{:#?}",
            result.report().losses
        );
    }
}

#[test]
fn v4_applies_definition_transformations_to_subfigure_occurrences() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    for (definition_type, instance_type, definition_arena) in [
        (308, 408, "subfigure_definitions"),
        (320, 420, "network_definitions"),
    ] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(transformed_subfigure_definition_file(
                    definition_type,
                    instance_type,
                    global_v4,
                    1,
                    0,
                    1,
                )),
                &DecodeOptions::default(),
            )
            .unwrap();
        let native = result.ir().native.namespace("iges").unwrap();
        assert_eq!(native.arenas[definition_arena].len(), 1);
        assert_eq!(
            native.arenas[definition_arena][0].fields()["transformation"],
            "iges:native:transformation#D1"
        );
        let root = native.arenas["product_occurrences"]
            .iter()
            .find(|occurrence| occurrence.id() == "iges:product:occurrence#7")
            .unwrap();
        assert_eq!(root.fields()["root"], true);
        assert_eq!(root.fields()["world_transform"][0][3], 10.0);
        assert_eq!(root.fields()["world_transform"][1][3], 20.0);
        assert_eq!(root.fields()["world_transform"][2][3], 30.0);
        assert!(
            !result
                .report()
                .losses
                .iter()
                .any(|loss| { loss.code == IgesLossCode::EntityNotProjected.kind() }),
            "{:#?}",
            result.report().losses
        );
    }
}

#[test]
fn decode_omits_occurrence_with_malformed_placement_and_reports_it() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(malformed_occurrence_placement_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();

    assert_eq!(native.arenas["subfigure_instances"].len(), 1);
    assert!(native.arenas["product_occurrences"].is_empty());
    let expansion = &native.arenas["product_occurrence_expansion"][0];
    assert_eq!(expansion.fields()["truncated"], true);
    assert_eq!(expansion.fields()["issues"][0], "malformed_placement");
    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == IgesLossCode::OccurrencePlacementMalformed.kind())
        .expect("malformed placement loss");
    assert_eq!(
        loss.provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref()),
        Some("directory_entry:D5")
    );
}

#[test]
fn decode_bounds_product_occurrence_expansion_with_a_named_loss() {
    let result = crate::reader::decode_with_test_occurrence_limits(
        &occurrence_limit_file(),
        DecodeOptions::default(),
        100,
        crate::native::MAX_PRODUCT_OCCURRENCE_DEPTH,
    )
    .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();

    assert_eq!(native.arenas["product_occurrences"].len(), 100);
    let expansion = &native.arenas["product_occurrence_expansion"][0];
    assert_eq!(expansion.fields()["output_limit"], 100);
    assert_eq!(expansion.fields()["depth_limit"], 64);
    assert_eq!(expansion.fields()["emitted"], 100);
    assert_eq!(expansion.fields()["truncated"], true);
    assert_eq!(expansion.fields()["issues"][0], "output_limit");
    assert!(result.report().losses.iter().any(|loss| {
        loss.message == "IGES product occurrence expansion reached its configured output limit"
    }));
    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == IgesLossCode::OccurrenceExpansionOutputTruncated.kind())
        .unwrap();
    assert_eq!(
        loss.provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref()),
        Some("directory_entry:D203")
    );
}

#[test]
fn decode_reports_product_occurrence_depth_truncation() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(occurrence_depth_limit_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();

    assert_eq!(native.arenas["product_occurrences"].len(), 64);
    let expansion = &native.arenas["product_occurrence_expansion"][0];
    assert_eq!(
        expansion.fields()["output_limit"],
        crate::native::MAX_PRODUCT_OCCURRENCES
    );
    assert_eq!(expansion.fields()["depth_limit"], 64);
    assert_eq!(expansion.fields()["emitted"], 64);
    assert_eq!(expansion.fields()["truncated"], true);
    assert_eq!(expansion.fields()["issues"][0], "depth_limit");
    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            == "IGES product occurrence expansion reached its configured nesting-depth limit"
    }));
    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == IgesLossCode::OccurrenceExpansionDepthTruncated.kind())
        .unwrap();
    assert_eq!(
        loss.provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref()),
        Some("directory_entry:D259")
    );
}

#[test]
fn decode_applies_the_session_recursion_limit_to_product_occurrences() {
    let mut options = DecodeOptions::default();
    options.policy.limits.max_recursion_depth = 1;
    let error = IgesCodec
        .decode(&mut Cursor::new(nested_subfigure_file()), &options)
        .unwrap_err();

    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RecursionDepth
                && limit.context.operation == "iges_product_occurrence"
    ));
}

#[test]
fn decode_does_not_infer_roots_from_malformed_definition_members() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(malformed_occurrence_definition_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();

    assert_eq!(native.arenas["subfigure_definitions"].len(), 3);
    assert_eq!(native.arenas["subfigure_instances"].len(), 1);
    assert!(native.arenas["product_occurrences"].is_empty());
    let expansion = &native.arenas["product_occurrence_expansion"][0];
    assert_eq!(expansion.fields()["truncated"], true);
    assert_eq!(expansion.fields()["issues"][0], "malformed_definition");
    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == IgesLossCode::OccurrenceRootInferenceBlocked.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 2);
    let tags = losses
        .iter()
        .map(|loss| {
            loss.provenance
                .as_ref()
                .and_then(|provenance| provenance.tag.as_deref())
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(tags, ["directory_entry:D5", "directory_entry:D7"]);
    let dangling = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#7")
        .unwrap();
    assert_eq!(dangling.fields()["references"][0]["resolution"], "dangling");
    assert!(native.arenas["subfigure_definitions"]
        .iter()
        .find(|definition| definition.id() == "iges:product:subfigure-definition#D7")
        .unwrap()
        .fields()["members"][0]
        .is_null());
}

#[test]
fn decode_does_not_infer_roots_from_malformed_network_definition_members() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(malformed_network_occurrence_definition_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();

    assert_eq!(native.arenas["network_definitions"].len(), 1);
    assert_eq!(native.arenas["network_instances"].len(), 1);
    assert!(native.arenas["product_occurrences"].is_empty());
    let expansion = &native.arenas["product_occurrence_expansion"][0];
    assert_eq!(expansion.fields()["truncated"], true);
    assert_eq!(expansion.fields()["issues"][0], "malformed_definition");
    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == IgesLossCode::OccurrenceRootInferenceBlocked.kind())
        .unwrap();
    assert_eq!(
        loss.provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref()),
        Some("directory_entry:D1")
    );
    assert!(native.arenas["network_definitions"][0].fields()["members"][0].is_null());
}

#[test]
fn decode_rejects_non_decreasing_subfigure_nesting_depth() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(invalid_subfigure_depth_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result.report().losses.iter().any(|loss| loss
        .message
        .contains("subfigure definition fields or nesting depth is invalid")));
    assert_eq!(
        result.ir().native.namespace("iges").unwrap().arenas["subfigure_definitions"].len(),
        2
    );
}

#[test]
fn decode_omits_occurrences_for_rejected_structure_entities() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(invalid_top_level_occurrence_structure_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();

    assert_eq!(native.arenas["subfigure_definitions"].len(), 1);
    assert_eq!(native.arenas["subfigure_instances"].len(), 2);
    assert_eq!(native.arenas["network_definitions"].len(), 1);
    assert_eq!(native.arenas["network_instances"].len(), 1);
    assert!(native.arenas["product_occurrences"].is_empty());
    let expansion = &native.arenas["product_occurrence_expansion"][0];
    assert_eq!(expansion.fields()["emitted"], 0);
    assert_eq!(expansion.fields()["truncated"], false);
    assert!(expansion.fields()["issues"].as_array().unwrap().is_empty());
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == IgesLossCode::EntityNotProjected.kind())
            .count(),
        4
    );
}

#[test]
fn decode_does_not_promote_subfigure_instance_in_rejected_definition() {
    let rejected = IgesCodec
        .decode(
            &mut Cursor::new(rejected_containing_subfigure_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = rejected.ir().native.namespace("iges").unwrap();
    assert!(native.arenas["product_occurrences"].is_empty());
    let expansion = &native.arenas["product_occurrence_expansion"][0];
    assert_eq!(expansion.fields()["emitted"], 0);
    assert_eq!(expansion.fields()["truncated"], false);
    assert!(expansion.fields()["issues"].as_array().unwrap().is_empty());

    let admitted = IgesCodec
        .decode(
            &mut Cursor::new(admitted_containing_subfigure_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(
        admitted.ir().native.namespace("iges").unwrap().arenas["product_occurrences"].len(),
        2
    );

    let container_only = IgesCodec
        .decode(
            &mut Cursor::new(rejected_containing_subfigure_file()),
            &DecodeOptions {
                container_only: true,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    assert_eq!(
        container_only.ir().native.namespace("iges").unwrap().arenas["product_occurrences"].len(),
        2
    );
}

#[test]
fn decode_does_not_promote_network_instance_in_rejected_definition() {
    let rejected = IgesCodec
        .decode(
            &mut Cursor::new(rejected_containing_network_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = rejected.ir().native.namespace("iges").unwrap();
    assert!(native.arenas["product_occurrences"].is_empty());
    let expansion = &native.arenas["product_occurrence_expansion"][0];
    assert_eq!(expansion.fields()["emitted"], 0);
    assert_eq!(expansion.fields()["truncated"], false);
    assert!(expansion.fields()["issues"].as_array().unwrap().is_empty());

    let admitted = IgesCodec
        .decode(
            &mut Cursor::new(admitted_containing_network_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(
        admitted.ir().native.namespace("iges").unwrap().arenas["product_occurrences"].len(),
        2
    );

    let container_only = IgesCodec
        .decode(
            &mut Cursor::new(rejected_containing_network_file()),
            &DecodeOptions {
                container_only: true,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    assert_eq!(
        container_only.ir().native.namespace("iges").unwrap().arenas["product_occurrences"].len(),
        2
    );
}

#[test]
fn container_only_preserves_raw_occurrence_expansion_without_structure_admission() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(invalid_top_level_occurrence_structure_file()),
            &DecodeOptions {
                container_only: true,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();

    assert!(!result.report().geometry_transferred);
    assert_eq!(native.arenas["product_occurrences"].len(), 3);
    let expansion = &native.arenas["product_occurrence_expansion"][0];
    assert_eq!(expansion.fields()["emitted"], 3);
    assert_eq!(expansion.fields()["truncated"], false);
    assert!(expansion.fields()["issues"].as_array().unwrap().is_empty());
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn decode_preserves_network_definition_and_anisotropic_instance() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(network_subfigure_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let definition = &native.arenas["network_definitions"][0];
    assert_eq!(definition.id(), "iges:product:network-definition#D1");
    assert_eq!(definition.fields()["type_flag"], 1);
    assert_eq!(definition.fields()["declared_connect_point_count"], 2);
    assert_eq!(
        definition.fields()["connect_points"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let instance = &native.arenas["network_instances"][0];
    assert_eq!(
        instance.fields()["definition"],
        "iges:product:network-definition#D1"
    );
    assert_eq!(instance.fields()["translation"][2], 3.0);
    assert_eq!(instance.fields()["scale"][0], 2.0);
    assert!(instance.fields()["scale"][1].is_null());
    assert!(instance.fields()["scale"][2].is_null());
    assert!(instance.fields()["type_flag"].is_null());
    let occurrence = &native.arenas["product_occurrences"][0];
    assert_eq!(occurrence.fields()["world_transform"][0][0], 2.0);
    assert_eq!(occurrence.fields()["world_transform"][1][1], 2.0);
    assert_eq!(occurrence.fields()["world_transform"][2][2], 2.0);
    assert_eq!(occurrence.fields()["world_transform"][0][3], 1.0);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn network_connectivity_uses_versioned_null_pointer_rules() {
    let definition = [Some(1_u32)];
    let instance = [None];
    assert!(network_connectivity_valid(
        &definition,
        &instance,
        Dialect::V5_0
    ));
    assert!(!network_connectivity_valid(
        &definition,
        &instance,
        Dialect::V4_0
    ));
    assert!(network_connectivity_valid(
        &definition,
        &[Some(3)],
        Dialect::V4_0
    ));
    assert!(!network_connectivity_valid(&definition, &[], Dialect::V5_0));
    assert!(!network_connectivity_valid(
        &[None],
        &[Some(3)],
        Dialect::V5_0
    ));
    assert!(network_connectivity_valid(&[None], &[None], Dialect::V5_0));
}

#[test]
fn subfigure_definition_directory_fields_use_the_v4_table_rules() {
    let entry = |line_font, subordinate, use_flag, hierarchy| DirectoryEntry {
        source_offset: 0,
        sequence: 1,
        entity_type: 308,
        parameter_start: 0,
        structure: 0,
        line_font,
        level: 0,
        view: 0,
        transform: 0,
        label_display: 0,
        status: Status {
            blank: 0,
            subordinate,
            use_flag,
            hierarchy,
        },
        line_weight: 0,
        color: 0,
        parameter_line_count: 0,
        form: 0,
        reserved: [[b' '; 8]; 2],
        label: [b' '; 8],
        subscript: 0,
    };

    assert!(!subfigure_definition_directory_fields_valid(
        &entry(0, 0, 2, 0),
        Dialect::V4_0
    ));
    assert!(subfigure_definition_directory_fields_valid(
        &entry(1, 0, 2, 0),
        Dialect::V4_0
    ));
    assert!(subfigure_definition_directory_fields_valid(
        &entry(0, 0, 2, 1),
        Dialect::V4_0
    ));
    assert!(!subfigure_definition_directory_fields_valid(
        &entry(1, 1, 2, 0),
        Dialect::V4_0
    ));
    assert!(!subfigure_definition_directory_fields_valid(
        &entry(1, 0, 1, 0),
        Dialect::V4_0
    ));
    assert!(subfigure_definition_directory_fields_valid(
        &entry(0, 3, 2, 0),
        Dialect::V5_0
    ));
}

#[test]
fn attribute_list_type_meaning_uses_versioned_ranges() {
    for (dialect, value, expected) in [
        (Dialect::V4_0, 0, Some("property-entity-defined")),
        (Dialect::V4_0, 5, Some("other-application-area")),
        (Dialect::V4_0, 5000, Some("other-application-area")),
        (Dialect::V4_0, 5001, Some("user-defined")),
        (Dialect::V4_0, 9999, Some("user-defined")),
        (Dialect::V4_0, 10_000, None),
        (Dialect::V5_0, 0, Some("type406-form15-defined")),
        (Dialect::V5_0, 5, Some("electrical-lep-manufacturing")),
        (Dialect::V5_0, 6, Some("other-application-area")),
        (Dialect::V5_0, 5000, Some("other-application-area")),
        (Dialect::V5_0, 5001, Some("implementor-defined")),
        (Dialect::V5_0, 9999, Some("implementor-defined")),
        (Dialect::V5_0, 10_000, None),
    ] {
        assert_eq!(
            crate::entities::structure::attribute_list_type_meaning(value, dialect),
            expected
        );
    }
}

fn network_null_connect_point_file(global: &[u8]) -> Vec<u8> {
    owned_test_file_with_global(
        &[
            OwnedTestEntity {
                entity_type: 132,
                form: 0,
                label: "DEFPIN".into(),
                status: "00000400",
                parameters: "132,0,0,0,0,1,1,2HP1,0,3HPIN,0,1,1,0,3;".into(),
            },
            OwnedTestEntity {
                entity_type: 320,
                form: 0,
                label: "NETWORK".into(),
                status: "00000200",
                parameters: "320,0,3HNET,0,1,2HR1,0,1,1;".into(),
            },
            OwnedTestEntity {
                entity_type: 420,
                form: 0,
                label: "NETINST".into(),
                status: "00000000",
                parameters: "420,3,10,20,30,1,,,1,2HU1,0,1,0;".into(),
            },
        ],
        global,
    )
}

#[test]
fn network_null_instance_connect_point_is_v5_only() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";

    let v4 = IgesCodec
        .decode(
            &mut Cursor::new(network_null_connect_point_file(global_v4)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(v4.report().losses.iter().any(|loss| {
        loss.message
            .contains("network instance definition or count is invalid")
    }));

    let v5 = IgesCodec
        .decode(
            &mut Cursor::new(network_null_connect_point_file(global_v5)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(!v5.report().losses.iter().any(|loss| {
        loss.message
            .contains("network instance definition or count is invalid")
    }));
    assert_eq!(
        v5.ir().native.namespace("iges").unwrap().arenas["network_instances"].len(),
        1
    );
}

#[test]
fn decode_rejects_wrong_typed_network_instance_type_flag() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(wrong_typed_network_instance_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(
        result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_rejects_wrong_typed_network_definition_type_flag() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(wrong_typed_network_definition_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn decode_preserves_owned_network_connect_points() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(connected_network_subfigure_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let points = &native.arenas["connect_points"];
    assert_eq!(points.len(), 2);
    assert_eq!(points[0].fields()["type_flag"], 101);
    assert_eq!(points[0].fields()["function_identifier"][0], 80);
    assert_eq!(points[0].fields()["function_identifier"][1], 49);
    assert_eq!(points[0].fields()["owner"], "iges:entity:directory#3");
    assert_eq!(points[1].fields()["position"][2], 3.0);
    assert_eq!(points[1].fields()["owner"], "iges:entity:directory#7");
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn iges_4_0_rejects_a_post_4_0_connect_point_type_flag() {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let bytes = owned_test_file_with_global(
        &[OwnedTestEntity {
            entity_type: 132,
            form: 0,
            label: "SIGNALPT".into(),
            status: "00000400",
            parameters: "132,0,0,0,0,101,1,2HP1,0,3HPIN,0,1,1,0,0,1,7,0;".into(),
        }],
        global,
    );

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn connect_point_function_code_extension_is_v5_only() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let entity = [OwnedTestEntity {
        entity_type: 132,
        form: 0,
        label: "SIGNALPT".into(),
        status: "00000400",
        parameters: "132,0,0,0,0,1,1,2HP1,0,3HPIN,0,1,6,0,0;".into(),
    }];

    let v4 = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_global(&entity, global_v4)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(v4
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));

    let v5 = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_global(&entity, global_v5)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(!v5
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}
