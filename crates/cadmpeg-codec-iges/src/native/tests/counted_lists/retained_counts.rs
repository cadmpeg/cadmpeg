use super::*;

fn salvage(bytes: &[u8]) -> cadmpeg_ir::codec::DecodeResult {
    IgesCodec
        .decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
        .unwrap()
}

fn overdeclared_site(
    entities: &[OwnedTestEntity],
    sequence: u32,
    arena: &str,
    declared_field: &str,
    declared: i64,
    list_field: &str,
) {
    let bytes = owned_test_file(entities);
    assert_overdeclared_contract(&bytes, sequence);

    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let record = native.arenas[arena]
        .iter()
        .find(|record| {
            record.fields()["source_entity"] == format!("iges:entity:directory#{sequence}")
        })
        .expect("native record");
    let fields = record.fields();
    assert_eq!(fields[declared_field], declared, "{arena} declared count");
    assert!(
        fields[list_field].as_array().unwrap().is_empty(),
        "{arena} {list_field} must not be read"
    );
}

fn only(entity_type: i64, form: i64, label: &str, parameters: &str) -> Vec<OwnedTestEntity> {
    vec![OwnedTestEntity {
        entity_type,
        form,
        label: label.into(),
        status: "00000200",
        parameters: parameters.into(),
    }]
}

fn point(label: &str) -> OwnedTestEntity {
    OwnedTestEntity {
        entity_type: 116,
        form: 0,
        label: label.into(),
        status: "00010100",
        parameters: "116,1,2,3;".into(),
    }
}

fn entity(entity_type: i64, form: i64, label: &str, parameters: &str) -> OwnedTestEntity {
    OwnedTestEntity {
        entity_type,
        form,
        label: label.into(),
        status: "00000200",
        parameters: parameters.into(),
    }
}

#[test]
fn decode_overdeclared_definition_levels_charge_the_loss_and_read_no_level() {
    overdeclared_site(
        &only(406, 1, "LEVELS", "406,3,10,20;"),
        1,
        "definition_levels",
        "declared_count",
        3,
        "levels",
    );
}

#[test]
fn decode_overdeclared_boolean_tree_charges_the_loss_and_reads_no_term() {
    overdeclared_site(
        &only(180, 0, "TREE", "180,5,-1,-3,1;"),
        1,
        "boolean_trees",
        "declared_length",
        5,
        "terms",
    );
}

#[test]
fn decode_overdeclared_subfigure_definition_charges_the_loss_and_reads_no_member() {
    overdeclared_site(
        &[point("MEMBER"), entity(308, 0, "SUB", "308,0,3HSUB,3,1;")],
        3,
        "subfigure_definitions",
        "declared_member_count",
        3,
        "members",
    );
}

#[test]
fn decode_overdeclared_group_charges_the_loss_and_reads_no_member() {
    overdeclared_site(
        &[point("MEMBER"), entity(402, 1, "GROUP", "402,3,1;")],
        3,
        "groups",
        "declared_member_count",
        3,
        "members",
    );
}

#[test]
fn decode_overdeclared_network_instance_charges_the_loss_and_reads_no_connect_point() {
    overdeclared_site(
        &only(420, 0, "NETINST", "420,0,0,0,0,1,1,1,0,2HR1,0,3,0;"),
        1,
        "network_instances",
        "declared_connect_point_count",
        3,
        "connect_points",
    );
}

#[test]
fn decode_overdeclared_solid_assembly_charges_the_loss_and_reads_no_item() {
    overdeclared_site(
        &only(184, 0, "ASSEMBLY", "184,2,1,3;"),
        1,
        "solid_assemblies",
        "declared_count",
        2,
        "items",
    );
}

fn manifold_solid_entities(
    parameters: impl Fn(u32, u32) -> String,
) -> (Vec<OwnedTestEntity>, u32, u32, u32) {
    let mut entities = Vec::new();
    let outer = append_tetrahedral_shell(&mut entities, "OUT", [0.0, 0.0, 0.0], 4.0);
    let void = append_tetrahedral_shell(&mut entities, "VOID", [0.5, 0.5, 0.5], 0.5);
    let solid = u32::try_from(entities.len() * 2 + 1).unwrap();
    let parameters = parameters(outer, void);
    entities.push(OwnedTestEntity {
        entity_type: 186,
        form: 0,
        label: "MSBO".into(),
        status: "00000000",
        parameters,
    });
    (entities, solid, outer, void)
}

#[test]
fn decode_overdeclared_manifold_solid_charges_the_loss_and_reads_no_void_shell() {
    let (entities, solid, _, _) =
        manifold_solid_entities(|outer, void| format!("186,{outer},1,2,{void},0;"));
    overdeclared_site(
        &entities,
        solid,
        "manifold_solids",
        "declared_void_count",
        2,
        "voids",
    );
}

#[test]
fn decode_overdeclared_units_data_charges_the_loss_and_reads_no_unit() {
    overdeclared_site(
        &only(316, 0, "UNITS", "316,3,2HIN,4HINCH,25.4,2HFT;"),
        1,
        "units_data",
        "declared_count",
        3,
        "units",
    );
}

#[test]
fn decode_overdeclared_leader_charges_the_loss_and_reads_no_segment() {
    overdeclared_site(
        &only(214, 1, "LEADER", "214,3,1.0,1.0,0.0,0.0,0.0,5.0,5.0;"),
        1,
        "annotations",
        "declared_segment_count",
        3,
        "segment_tails",
    );
}

#[test]
fn decode_overdeclared_line_font_pattern_reserves_the_hexadecimal_suffix() {
    let bytes = owned_test_file(&only(304, 2, "PATTERN", "304,3,1.0,2.0,2H0F;"));
    assert_overdeclared_contract(&bytes, 1);

    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let font = &native.arenas["line_fonts"][0];

    assert_eq!(font.fields()["segment_count"], 3);
    assert!(font.fields()["lengths"].as_array().unwrap().is_empty());
    assert!(font.fields()["hexadecimal_pattern"].is_null());
}

#[test]
fn decode_overdeclared_line_font_pattern_claims_no_length_as_its_suffix() {
    let bytes = owned_test_file(&only(304, 2, "PATTERN", "304,5,2HAB;"));
    assert_overdeclared_contract(&bytes, 1);

    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let font = &native.arenas["line_fonts"][0];

    assert_eq!(font.fields()["segment_count"], 5);
    assert!(font.fields()["lengths"].as_array().unwrap().is_empty());
    assert!(font.fields()["hexadecimal_pattern"].is_null());
}

#[test]
fn decode_overdeclared_copious_data_charges_the_loss_and_reads_no_tuple() {
    overdeclared_site(
        &[OwnedTestEntity {
            entity_type: 106,
            form: 1,
            label: "COPIOUS".into(),
            status: "00000000",
            parameters: "106,1,3,0.5,1,2,3;".into(),
        }],
        1,
        "copious_data",
        "declared_tuple_count",
        3,
        "tuples",
    );
}

#[test]
fn decode_overdeclared_external_reference_index_charges_the_loss_and_reads_no_entry() {
    for form in [2, 12] {
        overdeclared_site(
            &[
                point("TARGET"),
                entity(402, form, "INDEX", "402,3,1HA,1,1HB,1;"),
            ],
            3,
            "associativities",
            "declared_count",
            3,
            "entries",
        );
    }
}

#[test]
fn decode_overdeclared_segmented_visibility_charges_the_loss_and_reads_no_block() {
    overdeclared_site(
        &[
            entity(410, 0, "VIEW", "410,1;"),
            entity(402, 19, "SEGVIS", "402,2,1,0.5,1,0,0,0;"),
        ],
        3,
        "segmented_visibility",
        "declared_block_count",
        2,
        "blocks",
    );
}

#[test]
fn decode_overdeclared_view_list_charges_the_loss_and_reads_no_entity() {
    overdeclared_site(
        &[
            entity(410, 0, "VIEW", "410,1,1,0,0,0,0,0,0,1,3,0;"),
            entity(402, 6, "VIEWLST", "402,1,2,1,5;"),
            point("VISIBLE"),
        ],
        3,
        "associativities",
        "declared_visible_count",
        2,
        "visible_entities",
    );
}

#[test]
fn decode_overdeclared_single_parent_charges_the_loss_and_reads_no_child() {
    overdeclared_site(
        &[point("CHILD"), entity(402, 9, "PARENT", "402,1,2,1,1;")],
        3,
        "associativities",
        "declared_child_count",
        2,
        "children",
    );
}

#[test]
fn decode_overdeclared_dimensioned_geometry_charges_the_loss_and_reads_no_geometry() {
    overdeclared_site(
        &[
            point("GEOM"),
            entity(202, 0, "DIM", "202,0,0,0,0,0,0,0,0,0;"),
            entity(402, 13, "DIMGEOM", "402,1,2,3,1;"),
        ],
        5,
        "associativities",
        "declared_geometry_count",
        2,
        "geometry",
    );
}

#[test]
fn decode_overdeclared_planar_charges_the_loss_and_reads_no_entity() {
    overdeclared_site(
        &[point("MEMBER"), entity(402, 16, "PLANAR", "402,1,2,0,1;")],
        3,
        "associativities",
        "declared_entity_count",
        2,
        "entities",
    );
}

#[test]
fn decode_overdeclared_recalculable_dimension_charges_the_loss_and_reads_no_tuple() {
    overdeclared_site(
        &[
            entity(202, 0, "DIM", "202,0,0,0,0,0,0,0,0,0;"),
            entity(402, 21, "RECALC", "402,1,2,1,0,0.0,1,0,0.0,0.0,0.0;"),
        ],
        3,
        "associativities",
        "declared_geometry_count",
        2,
        "geometry",
    );
}

#[test]
fn decode_overdeclared_attribute_definition_charges_the_loss_and_reads_no_attribute() {
    overdeclared_site(
        &only(322, 0, "ATTRDEF", "322,4HATTR,0,3,1,1,0,2,1,0;"),
        1,
        "attribute_table_definitions",
        "declared_attribute_count",
        3,
        "attributes",
    );
}

#[test]
fn decode_overdeclared_property_lists_charge_the_loss_and_read_no_value() {
    for (form, parameters, list) in [
        (12, "406,3,1HA,1HB;", "names"),
        (14, "406,3,1HA,1HB;", "values"),
        (24, "406,6,2,1,1HA,1,1HB;", "definitions"),
        (25, "406,5,1HX,3,1,2;", "levels"),
        (27, "406,6,1HN,3,1,5,1;", "values"),
        (34, "406,7,3,1,1,2,1;", "ranges"),
        (35, "406,7,3,1,1,2,1;", "ranges"),
    ] {
        let bytes = owned_test_file(&only(406, form, "PROP", parameters));
        assert_overdeclared_contract(&bytes, 1);

        let result = salvage(&bytes);
        let native = result.ir().native.namespace("iges").unwrap();
        let property = &native.arenas["properties"][0];
        assert!(
            property.fields()[list].as_array().unwrap().is_empty(),
            "form {form} {list} must not be read"
        );
    }
}

#[test]
fn decode_overdeclared_dimension_display_notes_charge_the_loss_and_read_no_note() {
    overdeclared_site(
        &only(
            406,
            30,
            "DIMDISP",
            "406,14,1,0,1,1HL,0,1.5707963267948966,0,0,0,0,0.0,2,1,1,2;",
        ),
        1,
        "properties",
        "declared_value_count",
        14,
        "supplemental_notes",
    );
}

#[test]
fn decode_overdeclared_flag_note_and_general_label_charge_the_loss_and_read_no_leader() {
    for (entity_type, parameters, sequence) in [
        (208, "208,0.0,0.0,0.0,0.0,1,2,3;", 5_u32),
        (210, "210,1,2,3;", 5),
    ] {
        let bytes = owned_test_file(&[
            entity(
                212,
                0,
                "NOTE",
                "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;",
            ),
            entity(214, 1, "LEADER", "214,1,1.0,1.0,0.0,0.0,0.0,5.0,5.0;"),
            entity(entity_type, 0, "ANNO", parameters),
        ]);
        assert_overdeclared_contract(&bytes, sequence);

        let result = salvage(&bytes);
        let native = result.ir().native.namespace("iges").unwrap();
        let annotation = native.arenas["annotations"]
            .iter()
            .find(|record| {
                record.fields()["source_entity"] == format!("iges:entity:directory#{sequence}")
            })
            .expect("annotation");
        assert!(
            annotation.fields()["leaders"]
                .as_array()
                .unwrap()
                .is_empty(),
            "type {entity_type} leaders must not be read"
        );
    }
}

#[test]
fn decode_overdeclared_sectioned_area_charges_the_loss_and_reads_no_island() {
    overdeclared_site(
        &[
            entity(100, 0, "BOUND", "100,0.0,0.0,0.0,1.0,0.0,1.0,0.0;"),
            entity(230, 0, "SECTION", "230,1,0,0.0,0.0,0.0,0.0,0.0,2,1;"),
        ],
        3,
        "annotations",
        "declared_island_count",
        2,
        "islands",
    );
}

#[test]
fn decode_sectioned_area_retains_a_negative_declared_island_count() {
    let bytes = owned_test_file(&[
        entity(100, 0, "BOUND", "100,0.0,0.0,0.0,1.0,0.0,1.0,0.0;"),
        entity(230, 0, "SECTION", "230,1,0,0.0,0.0,0.0,0.0,0.0,-1,1;"),
    ]);
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let section = &native.arenas["annotations"][0];
    let fields = section.fields();

    assert_eq!(fields["declared_island_count"], -1);
    assert!(fields["islands"].as_array().unwrap().is_empty());
    assert_no_count_loss(&result);
}

fn assert_no_count_loss(result: &cadmpeg_ir::codec::DecodeResult) {
    assert_eq!(
        code_count(result.report(), IgesLossCode::ParameterCountOverdeclared),
        0,
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_units_data_reads_a_final_unit_present_in_part() {
    let bytes = owned_test_file(&only(316, 0, "UNITS", "316,2,2HIN,4HINCH,25.4,2HFT;"));
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let units = &native.arenas["units_data"][0];
    let fields = units.fields();
    let list = fields["units"].as_array().unwrap();

    assert_eq!(fields["declared_count"], 2);
    assert_eq!(list.len(), 2);
    assert!(list[1]["unit_value"].is_null());
    assert!(list[1]["scale_factor"].is_null());
    assert_no_count_loss(&result);
}

#[test]
fn decode_external_reference_index_reads_a_final_pair_present_in_part() {
    for form in [2, 12] {
        let bytes = owned_test_file(&[
            point("TARGET"),
            entity(402, form, "INDEX", "402,2,1HA,1,1HB;"),
        ]);
        let result = salvage(&bytes);
        let native = result.ir().native.namespace("iges").unwrap();
        let associativity = &native.arenas["associativities"][0];
        let fields = associativity.fields();
        let entries = fields["entries"].as_array().unwrap();

        assert_eq!(fields["declared_count"], 2);
        assert_eq!(entries.len(), 2);
        assert!(entries[1]["entity"].is_null());
        assert_no_count_loss(&result);
    }
}

#[test]
fn decode_segmented_visibility_reads_a_final_block_present_in_part() {
    let bytes = owned_test_file(&[
        entity(410, 0, "VIEW", "410,1;"),
        entity(402, 19, "SEGVIS", "402,2,1,0.5,1,0,0,0,1,0.75;"),
    ]);
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let visibility = &native.arenas["segmented_visibility"][0];
    let fields = visibility.fields();
    let blocks = fields["blocks"].as_array().unwrap();

    assert_eq!(fields["declared_block_count"], 2);
    assert_eq!(blocks.len(), 2);
    assert!(blocks[1]["display_flag"].is_null());
    assert_no_count_loss(&result);
}

// A negative declared count fails `usize::try_from`, so the counted tail is
// `Unreadable` and charges no overdeclaration loss. The retained
// `declared_*_count` field is then the only witness that the file declared
// anything at all. The sectioned-area twin of this test exercises the same
// path through `counted_tail_at`.
#[test]
fn decode_segmented_visibility_retains_a_negative_declared_block_count() {
    let bytes = owned_test_file(&[
        entity(410, 0, "VIEW", "410,1;"),
        entity(402, 19, "SEGVIS", "402,-1,1,0.5,1,0,0,0;"),
    ]);
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let visibility = &native.arenas["segmented_visibility"][0];
    let fields = visibility.fields();

    assert_eq!(fields["declared_block_count"], -1);
    assert!(fields["blocks"].as_array().unwrap().is_empty());
    assert_no_count_loss(&result);
}

#[test]
fn decode_view_visibility_reads_both_lists_and_retains_both_declared_counts() {
    let bytes = owned_test_file(&[
        entity(410, 0, "VIEW", "410,1;"),
        entity(402, 3, "VISIBLE", "402,1,1,1,5,0,0;"),
        point("SHOWN"),
    ]);
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let fields = native.arenas["view_visibility"][0].fields();
    let displays = fields["displays"].as_array().unwrap();
    let entities = fields["entities"].as_array().unwrap();

    assert_eq!(fields["declared_view_count"], 1);
    assert_eq!(fields["declared_entity_count"], 1);
    assert_eq!(displays.len(), 1);
    assert_eq!(displays[0]["view"], "iges:presentation:view#D1");
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0], "iges:entity:directory#5");
    assert_no_count_loss(&result);
}

// A chained two-list layout is refused jointly, so neither count has a
// defensible `present` figure and no `parameter.count-overdeclared` verdict
// can be charged for either list. The retained declared counts are the only
// witnesses, which is why the overrun tests below assert
// `assert_no_count_loss` instead of going through `overdeclared_site`.
#[test]
fn decode_view_visibility_retains_both_declared_counts_when_the_entity_list_overruns() {
    let bytes = owned_test_file(&[
        entity(410, 0, "VIEW", "410,1;"),
        entity(402, 3, "VISIBLE", "402,1,2,1,5;"),
        point("SHOWN"),
    ]);
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let fields = native.arenas["view_visibility"][0].fields();

    assert_eq!(fields["declared_view_count"], 1);
    assert_eq!(fields["declared_entity_count"], 2);
    assert!(fields["displays"].as_array().unwrap().is_empty());
    assert!(fields["entities"].as_array().unwrap().is_empty());
    assert_no_count_loss(&result);
}

#[test]
fn decode_view_visibility_retains_a_negative_declared_view_count() {
    let bytes = owned_test_file(&[entity(402, 4, "DISPLAY", "402,-1,0,0,0;")]);
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let fields = native.arenas["view_visibility"][0].fields();

    assert_eq!(fields["declared_view_count"], -1);
    assert_eq!(fields["declared_entity_count"], 0);
    assert!(fields["displays"].as_array().unwrap().is_empty());
    assert!(fields["entities"].as_array().unwrap().is_empty());
    assert_no_count_loss(&result);
}

#[test]
fn decode_drawing_reads_both_lists_and_retains_both_declared_counts() {
    let bytes = owned_test_file(&[
        entity(410, 0, "VIEW", "410,1;"),
        entity(404, 1, "DRAWING", "404,1,1,10,20,0.5,1,5,0,0;"),
        point("ANNOT"),
    ]);
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let fields = native.arenas["drawings"][0].fields();
    let views = fields["views"].as_array().unwrap();
    let annotations = fields["annotations"].as_array().unwrap();

    assert_eq!(fields["declared_view_count"], 1);
    assert_eq!(fields["declared_annotation_count"], 1);
    assert_eq!(views.len(), 1);
    assert_eq!(views[0]["view"], "iges:presentation:view#D1");
    assert!((views[0]["rotation"].as_f64().unwrap() - 0.5).abs() < 1.0e-12);
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0], "iges:entity:directory#5");
    assert_no_count_loss(&result);
}

#[test]
fn decode_drawing_retains_both_declared_counts_when_the_annotation_list_overruns() {
    let bytes = owned_test_file(&[
        entity(410, 0, "VIEW", "410,1;"),
        entity(404, 0, "DRAWING", "404,1,1,10,20,3,5;"),
        point("ANNOT"),
    ]);
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let fields = native.arenas["drawings"][0].fields();

    assert_eq!(fields["declared_view_count"], 1);
    assert_eq!(fields["declared_annotation_count"], 3);
    assert!(fields["views"].as_array().unwrap().is_empty());
    assert!(fields["annotations"].as_array().unwrap().is_empty());
    assert_no_count_loss(&result);
}

#[test]
fn decode_drawing_with_a_negative_declared_view_count_locates_no_annotation_count() {
    let bytes = owned_test_file(&[
        entity(410, 0, "VIEW", "410,1;"),
        entity(404, 0, "DRAWING", "404,-1,1,10,20,1,5,0,0;"),
        point("ANNOT"),
    ]);
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let fields = native.arenas["drawings"][0].fields();

    assert_eq!(fields["declared_view_count"], -1);
    assert!(fields["declared_annotation_count"].is_null());
    assert!(fields["views"].as_array().unwrap().is_empty());
    assert!(fields["annotations"].as_array().unwrap().is_empty());
    assert_no_count_loss(&result);
}

#[test]
fn decode_general_symbol_retains_both_declared_counts_when_the_leader_list_overruns() {
    let bytes = owned_test_file(&[
        entity(
            212,
            0,
            "NOTE",
            "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HS;",
        ),
        entity(100, 0, "GEOMETRY", "100,0,0,0,1,0,1,0;"),
        entity(228, 0, "SYMBOL", "228,1,1,3,2,5;"),
    ]);
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let symbol = native.arenas["annotations"]
        .iter()
        .find(|record| record.fields()["kind"] == "general_symbol")
        .expect("general symbol");
    let fields = symbol.fields();

    assert_eq!(fields["declared_geometry_count"], 1);
    assert_eq!(fields["declared_leader_count"], 2);
    assert!(fields["geometry"].as_array().unwrap().is_empty());
    assert!(fields["leaders"].as_array().unwrap().is_empty());
    assert_no_count_loss(&result);
}

#[test]
fn decode_manifold_solid_reads_its_shell_uses_and_resolves_both_closed_shells() {
    let (bytes, solid, outer, void) = explicit_void_solid_file();
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let solids = &native.arenas["manifold_solids"];
    assert_eq!(solids.len(), 1);
    let fields = solids[0].fields();
    let voids = fields["voids"].as_array().unwrap();

    assert_eq!(solids[0].id(), format!("iges:solid:manifold-brep#D{solid}"));
    assert_eq!(
        fields["source_entity"],
        format!("iges:entity:directory#{solid}")
    );
    assert_eq!(fields["shell"], format!("iges:entity:directory#{outer}"));
    assert_eq!(fields["shell_orientation"], 1);
    assert_eq!(fields["declared_void_count"], 1);
    assert!(fields["transformation"].is_null());
    assert_eq!(voids.len(), 1);
    assert_eq!(voids[0]["shell"], format!("iges:entity:directory#{void}"));
    assert_eq!(voids[0]["orientation"], 0);
    assert_no_count_loss(&result);
}

// §4.49 gives VOF no default, so a final pair the record delimiter cuts
// short keeps a null orientation instead of an invented flag. The trailing
// partial pair is admitted by the `div_ceil` branch of
// `items_before_default_tail_at`, the same path the segmented-visibility
// twin exercises.
#[test]
fn decode_manifold_solid_reads_a_final_void_shell_use_present_in_part() {
    let (entities, solid, _, void) =
        manifold_solid_entities(|outer, void| format!("186,{outer},1,1,{void};"));
    let result = salvage(&owned_test_file(&entities));
    let native = result.ir().native.namespace("iges").unwrap();
    let fields = native.arenas["manifold_solids"][0].fields();
    let voids = fields["voids"].as_array().unwrap();

    assert_eq!(
        fields["source_entity"],
        format!("iges:entity:directory#{solid}")
    );
    assert_eq!(fields["declared_void_count"], 1);
    assert_eq!(voids.len(), 1);
    assert_eq!(voids[0]["shell"], format!("iges:entity:directory#{void}"));
    assert!(voids[0]["orientation"].is_null());
    assert_no_count_loss(&result);
}

#[test]
fn decode_manifold_solid_retains_a_negative_declared_void_count() {
    let (entities, _, outer, _) = manifold_solid_entities(|outer, _| format!("186,{outer},1,-1;"));
    let result = salvage(&owned_test_file(&entities));
    let native = result.ir().native.namespace("iges").unwrap();
    let fields = native.arenas["manifold_solids"][0].fields();

    assert_eq!(fields["declared_void_count"], -1);
    assert!(fields["voids"].as_array().unwrap().is_empty());
    assert_eq!(fields["shell"], format!("iges:entity:directory#{outer}"));
    assert_eq!(fields["shell_orientation"], 1);
    assert_no_count_loss(&result);
}

#[test]
fn decode_manifold_solid_leaves_an_open_shell_pointer_unresolved() {
    let (mut entities, closed_solid, outer, _) =
        manifold_solid_entities(|outer, _| format!("186,{outer},1,0;"));
    entities.push(entity(514, 2, "OPEN", "514,0;"));
    let open_shell = u32::try_from(entities.len() * 2 - 1).unwrap();
    let open_solid = u32::try_from(entities.len() * 2 + 1).unwrap();
    entities.push(entity(186, 0, "OPENSLD", &format!("186,{open_shell},1,0;")));
    let result = salvage(&owned_test_file(&entities));
    let native = result.ir().native.namespace("iges").unwrap();
    let solids = &native.arenas["manifold_solids"];
    assert_eq!(solids.len(), 2);
    let closed = solids[0].fields();
    let rejected = solids[1].fields();

    assert_eq!(
        closed["source_entity"],
        format!("iges:entity:directory#{closed_solid}")
    );
    assert_eq!(closed["shell"], format!("iges:entity:directory#{outer}"));
    assert_eq!(
        rejected["source_entity"],
        format!("iges:entity:directory#{open_solid}")
    );
    assert!(rejected["shell"].is_null());
    assert_eq!(rejected["declared_void_count"], 0);
    assert!(rejected["voids"].as_array().unwrap().is_empty());
    assert_eq!(
        code_count(result.report(), IgesLossCode::PointerUnresolved),
        1
    );
    assert_no_count_loss(&result);
}

#[test]
fn decode_text_score_reads_a_final_range_present_in_part() {
    let bytes = owned_test_file(&only(406, 34, "UNDER", "406,7,2,1,1,2,1,1;"));
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let property = &native.arenas["properties"][0];
    let fields = property.fields();
    let ranges = fields["ranges"].as_array().unwrap();

    assert_eq!(ranges.len(), 2);
    assert!(ranges[1]["last_character"].is_null());
    assert_no_count_loss(&result);
}

#[test]
fn decode_leader_reads_a_final_segment_present_in_part() {
    let bytes = owned_test_file(&only(
        214,
        1,
        "LEADER",
        "214,2,1.0,1.0,0.0,0.0,0.0,5.0,5.0,7.0;",
    ));
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let leader = &native.arenas["annotations"][0];
    let fields = leader.fields();
    let tails = fields["segment_tails"].as_array().unwrap();

    assert_eq!(fields["declared_segment_count"], 2);
    assert_eq!(tails.len(), 2);
    assert_eq!(tails[1][0], 7.0);
    assert!(tails[1][1].is_null());
    assert_no_count_loss(&result);
}

#[test]
fn decode_copious_data_reads_a_final_tuple_present_in_part() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 106,
        form: 1,
        label: "COPIOUS".into(),
        status: "00000000",
        parameters: "106,1,2,0.5,1,2,3;".into(),
    }]);
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let copious = &native.arenas["copious_data"][0];
    let fields = copious.fields();
    let tuples = fields["tuples"].as_array().unwrap();

    assert_eq!(fields["declared_tuple_count"], 2);
    assert_eq!(tuples.len(), 2);
    assert_eq!(tuples[1][0], 3.0);
    assert!(tuples[1][1].is_null());
    assert_no_count_loss(&result);
}

#[test]
fn decode_line_font_pattern_holds_only_complete_lengths_before_its_suffix() {
    let bytes = owned_test_file(&only(304, 2, "PATTERN", "304,2,1.0,2.0,2H0F;"));
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let font = &native.arenas["line_fonts"][0];
    let fields = font.fields();

    assert_eq!(fields["segment_count"], 2);
    assert_eq!(fields["lengths"].as_array().unwrap().len(), 2);
    assert!(fields["hexadecimal_pattern"].is_array());
    assert_no_count_loss(&result);
}

#[test]
fn decode_charges_one_count_loss_per_entry_in_directory_sequence_order() {
    let bytes = owned_test_file(&[
        entity(406, 1, "LEVELS", "406,3,10,20;"),
        entity(316, 0, "UNITS", "316,3,2HIN,4HINCH,25.4,2HFT;"),
        entity(
            212,
            0,
            "NOTE",
            "212,2,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;",
        ),
    ]);
    let result = salvage(&bytes);
    let charged = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == IgesLossCode::ParameterCountOverdeclared.kind())
        .map(|loss| {
            loss.provenance
                .as_ref()
                .and_then(|source| source.tag.clone())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();

    assert_eq!(
        charged,
        [
            "directory_entry:D1".to_owned(),
            "directory_entry:D3".to_owned(),
            "directory_entry:D5".to_owned()
        ]
    );
}

#[test]
fn decode_charges_the_count_loss_once_for_an_entry_with_two_counted_lists() {
    let bytes = owned_test_file(&only(
        406,
        30,
        "DIMDISP",
        "406,14,1,0,1,1HL,0,1.5707963267948966,0,0,0,0,0.0,2,1,1,2;",
    ));
    assert_overdeclared_contract(&bytes, 1);
}

#[test]
fn decode_attribute_definition_holds_its_count_while_the_nested_triple_stays_empty() {
    let bytes = owned_test_file(&only(322, 0, "ATTRDEF", "322,4HATTR,0,1,1,1;"));
    let result = salvage(&bytes);
    let native = result.ir().native.namespace("iges").unwrap();
    let definition = &native.arenas["attribute_table_definitions"][0];

    assert_eq!(definition.fields()["declared_attribute_count"], 1);
    assert!(definition.fields()["attributes"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_no_count_loss(&result);
}

#[test]
fn decode_attribute_instance_rows_clamp_to_the_values_the_record_holds() {
    for (parameters, rows) in [
        ("422,2,8,4HIRON,9,5HBRASS;", 2),
        ("422,1,8,4HIRON,9,5HBRASS;", 1),
        ("422,0,8,4HIRON;", 0),
        ("422,3,8,4HIRON,9,5HBRASS;", 0),
        ("422,2,8,4HIRON,9;", 0),
        ("422,,8,4HIRON;", 0),
        ("422,-3,8,4HIRON;", 0),
        ("422,9223372036854775807,8,4HIRON;", 0),
    ] {
        let entities = [
            OwnedTestEntity {
                entity_type: 322,
                form: 0,
                label: "ATTRDEF".into(),
                status: "00000000",
                parameters: "322,4HMETA,1,2,10,1,1,11,3,1;".into(),
            },
            OwnedTestEntity {
                entity_type: 422,
                form: 1,
                label: "ATTRTAB".into(),
                status: "00000000",
                parameters: parameters.into(),
            },
        ];
        let bytes = owned_test_file_with_structures(&entities, &[(3, -1)]);
        let result = salvage(&bytes);
        let native = result.ir().native.namespace("iges").unwrap();
        let instance = &native.arenas["attribute_table_instances"][0];
        let fields = instance.fields();

        assert_eq!(
            fields["definition"], "iges:product:attribute-definition#D1",
            "{parameters}"
        );
        let read = fields["rows"].as_array().unwrap();
        assert_eq!(read.len(), rows, "{parameters}");
        assert!(
            read.iter().all(|row| row.as_array().unwrap().len() == 2),
            "{parameters}"
        );
    }
}
