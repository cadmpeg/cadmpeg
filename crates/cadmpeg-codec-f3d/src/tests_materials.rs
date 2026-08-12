// SPDX-License-Identifier: Apache-2.0
//! Materials-domain synthetic tests and fixtures.

use super::*;

#[test]
fn face_appearance_bindings_stay_unique_when_one_appearance_binds_many_faces() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceTarget};
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    use cadmpeg_ir::units::Units;

    // One appearance attribute GUID reaches every face carrying it, so the
    // assignment pair repeats across those faces. The face id has to enter the
    // binding id, or the arena holds colliding ids and fails both the global
    // identity check and the strict arena-order check.
    let face_guid = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
    let visual_family = "11111111-2222-3333-4444-555555555555";
    let visual_guid = "11111111-2222-3333-4444-555555555555_Post2015";
    let mut ir = cadmpeg_ir::CadIr::empty(Units::default());
    for face in ["face:1", "face:2", "face:3"] {
        ir.model.attributes.push(SourceAttribute {
            id: format!("attr:{face}").into(),
            target: AttributeTarget::Face(face.into()),
            name: "ATTRIB_CUSTOM-attrib".into(),
            values: vec![
                AttributeValue::String("NEUTRON_Material_attrib_def".into()),
                AttributeValue::String(face_guid.into()),
            ],
        });
    }
    let appearance = |id: &str, token: &str| Appearance {
        id: id.into(),
        name: None,
        asset_guid: None,
        library_id: None,
        visual_guid: Some(token.into()),
        physical_token: None,
        schema: None,
        category: None,
        base_color: None,
        properties: std::collections::BTreeMap::new(),
        textures: Vec::new(),
    };
    // Base record first so prefix-only selection cannot bind it.
    ir.model.appearances.extend([
        appearance("appearance:base", visual_family),
        appearance("appearance:revision", visual_guid),
    ]);

    crate::decode::resolve_face_appearance_bindings(
        &mut ir,
        &[crate::materials::FaceAppearanceAssignment {
            face_guid: face_guid.into(),
            visual_guid: visual_guid.into(),
            color: None,
        }],
    )
    .expect("unique face appearance");

    assert_eq!(ir.model.appearance_bindings.len(), 3);
    let ids = ir
        .model
        .appearance_bindings
        .iter()
        .map(|binding| binding.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), 3);
    assert!(ir
        .model
        .appearance_bindings
        .iter()
        .all(|binding| binding.appearance.as_str() == "appearance:revision"));
    let targets = ir
        .model
        .appearance_bindings
        .iter()
        .map(|binding| binding.target.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        targets,
        vec![
            AppearanceTarget::Face("face:1".into()),
            AppearanceTarget::Face("face:2".into()),
            AppearanceTarget::Face("face:3".into()),
        ]
    );

    // The decode path sorts by id after resolving; that sort is only a total
    // order over the arena when the ids are distinct.
    ir.model.appearance_bindings.sort_by(|a, b| a.id.cmp(&b.id));
    assert!(ir
        .model
        .appearance_bindings
        .windows(2)
        .all(|pair| pair[0].id < pair[1].id));

    ir.model.attributes[0].values.push(AttributeValue::String(
        "cccccccc-1111-2222-3333-dddddddddddd".into(),
    ));
    let error = crate::decode::resolve_face_appearance_bindings(
        &mut ir,
        &[crate::materials::FaceAppearanceAssignment {
            face_guid: face_guid.into(),
            visual_guid: visual_guid.into(),
            color: None,
        }],
    )
    .expect_err("a face material attribute with two GUID operands is ambiguous");
    assert!(error
        .to_string()
        .contains("exactly one lower-case face GUID"));
}

#[test]
fn legacy_face_assignment_color_precedes_appearance_base_but_not_brep_color() {
    use cadmpeg_ir::appearance::Appearance;
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    use cadmpeg_ir::topology::Color;

    let face_guid = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
    let visual_guid = "11111111-2222-3333-4444-555555555555_Post2015";
    let assignment_color = Color {
        r: 0.75,
        g: 0.25,
        b: 0.125,
        a: 1.0,
    };
    let explicit_color = Color {
        r: 0.1,
        g: 0.8,
        b: 0.2,
        a: 1.0,
    };
    let make_ir = || {
        let mut ir = cadmpeg_ir::examples::unit_cube();
        let face = ir.model.faces[0].id.clone();
        ir.model.attributes.push(SourceAttribute {
            id: "f3d:test:face-material".into(),
            target: AttributeTarget::Face(face),
            name: "ATTRIB_CUSTOM-attrib".into(),
            values: vec![
                AttributeValue::String("NEUTRON_Material_attrib_def".into()),
                AttributeValue::String(face_guid.into()),
            ],
        });
        ir.model.appearances.push(Appearance {
            id: "appearance:face".into(),
            name: None,
            asset_guid: None,
            library_id: None,
            visual_guid: Some(visual_guid.into()),
            physical_token: None,
            schema: None,
            category: None,
            base_color: Some(Color {
                r: 0.0,
                g: 0.0,
                b: 1.0,
                a: 1.0,
            }),
            properties: std::collections::BTreeMap::new(),
            textures: Vec::new(),
        });
        ir
    };
    let assignment = crate::materials::FaceAppearanceAssignment {
        face_guid: face_guid.into(),
        visual_guid: visual_guid.into(),
        color: Some(assignment_color),
    };

    let mut ir = make_ir();
    crate::decode::resolve_face_appearance_bindings(&mut ir, std::slice::from_ref(&assignment))
        .expect("legacy face assignment");
    assert_eq!(ir.model.faces[0].color, Some(assignment_color));
    assert_eq!(ir.model.appearance_bindings.len(), 1);

    let mut explicit = make_ir();
    explicit.model.faces[0].color = Some(explicit_color);
    crate::decode::resolve_face_appearance_bindings(
        &mut explicit,
        std::slice::from_ref(&assignment),
    )
    .expect("explicit face color");
    assert_eq!(explicit.model.faces[0].color, Some(explicit_color));

    let mut no_asset = make_ir();
    no_asset.model.appearances.clear();
    crate::decode::resolve_face_appearance_bindings(
        &mut no_asset,
        std::slice::from_ref(&assignment),
    )
    .expect("face color independent of appearance lookup");
    assert_eq!(no_asset.model.faces[0].color, Some(assignment_color));
    assert!(no_asset.model.appearance_bindings.is_empty());
}

#[test]
fn duplicate_face_assignments_reject_conflicting_colors() {
    use cadmpeg_ir::topology::Color;

    let face_guid = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
    let visual_guid = "11111111-2222-3333-4444-555555555555_Post2015";
    let assignment = |r| crate::materials::FaceAppearanceAssignment {
        face_guid: face_guid.into(),
        visual_guid: visual_guid.into(),
        color: Some(Color {
            r,
            g: 0.25,
            b: 0.5,
            a: 1.0,
        }),
    };
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let error = crate::decode::resolve_face_appearance_bindings(
        &mut ir,
        &[assignment(0.25), assignment(0.75)],
    )
    .expect_err("one face material GUID cannot carry two neutral colors");
    assert!(error.to_string().contains("conflicting neutral colors"));
}

#[test]
fn decode_transfers_generated_protein_appearance() {
    let f3d = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let mut cur = Cursor::new(f3d);
    let result = F3dCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.appearances.len(), 1);
    let appearance = &result.ir().model.appearances[0];
    assert_eq!(appearance.name.as_deref(), Some("Prism-001"));
    assert_eq!(
        appearance.visual_guid.as_deref(),
        Some("11111111-2222-3333-4444-555555555555")
    );
    let color = appearance.base_color.expect("decoded diffuse color");
    assert_eq!((color.r, color.g, color.b), (0.1, 0.2, 0.3));
    assert_eq!(
        appearance.physical_token.as_deref(),
        Some("PrismMaterial-018")
    );
    assert_eq!(appearance.schema.as_deref(), Some("GenericSchema"));
    assert_eq!(
        appearance.category.as_deref(),
        Some("Plastic/Thermoplastic")
    );
    assert_eq!(result.ir().model.appearance_bindings.len(), 1);
    assert_eq!(f3d_native(result.ir()).act_entities.len(), 1);
    assert_eq!(f3d_native(result.ir()).act_entities[0].record_index, 7);
    assert_eq!(f3d_native(result.ir()).act_entities[0].entity_id, "0_985");
    assert_eq!(f3d_native(result.ir()).act_guids.len(), 1);
    assert_eq!(
        f3d_native(result.ir()).act_guids[0].guid,
        "eeeeeeee-1111-2222-3333-ffffffffffff"
    );
    assert_eq!(f3d_native(result.ir()).act_registry_channels.len(), 2);
    assert_eq!(f3d_native(result.ir()).act_table_references.len(), 1);
    assert_eq!(
        f3d_native(result.ir()).act_table_references[0].target_record,
        9
    );
    assert_eq!(
        f3d_native(result.ir()).act_registry_channels[0].name,
        "Appearance"
    );
    assert_eq!(
        f3d_native(result.ir()).act_registry_channels[1].name,
        "PhysicalMaterial"
    );
    assert!(f3d_native(result.ir()).act_entities[0].in_table);
    assert_eq!(f3d_native(result.ir()).act_root_components.len(), 1);
    assert_eq!(
        f3d_native(result.ir()).act_root_components[0].entity_id,
        "0_3"
    );
    assert_eq!(
        f3d_native(result.ir()).act_root_components[0].display_name,
        "(Unsaved)"
    );
    assert_eq!(
        f3d_native(result.ir()).act_root_components[0].instance_root_record,
        12
    );
    assert_eq!(
        f3d_native(result.ir()).act_root_components[0].tracked_entity_record,
        3
    );
    assert_eq!(
        f3d_native(result.ir()).act_root_components[0].components_root_record,
        7
    );
    assert_eq!(
        f3d_native(result.ir()).act_root_components[0].registry_flag,
        1
    );
    assert_eq!(
        f3d_native(result.ir()).act_entities[0]
            .channel_class_tag
            .as_deref(),
        Some("261")
    );
    assert_eq!(
        result.ir().model.appearance_bindings[0].appearance,
        appearance.id
    );
    assert!(matches!(
        &result.ir().model.appearance_bindings[0].target,
        cadmpeg_ir::appearance::AppearanceTarget::Body(body) if body == &result.ir().model.bodies[0].id
    ));
    assert_eq!(
        result.ir().model.appearance_bindings[0]
            .channels
            .get("Appearance")
            .map(String::as_str),
        Some("aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb")
    );
    assert_eq!(
        result.ir().model.appearance_bindings[0]
            .source_entity_id
            .as_deref(),
        Some("0_985")
    );
    assert_eq!(
        result.ir().model.appearance_bindings[0]
            .object_type
            .as_deref(),
        Some("Body")
    );
    assert_eq!(f3d_native(result.ir()).construction_recipes.len(), 1);
    assert_eq!(
        f3d_native(result.ir()).construction_recipes[0].kind,
        crate::records::ConstructionRecipeKind::Body
    );
    assert_eq!(
        f3d_native(result.ir()).construction_recipes[0]
            .design_id
            .as_deref(),
        Some("322")
    );
    assert_eq!(
        f3d_native(result.ir()).construction_recipes[0].record_index,
        123
    );
    assert_eq!(f3d_native(result.ir()).persistent_references.len(), 10);
    assert!(f3d_native(result.ir())
        .persistent_references
        .iter()
        .any(|reference| reference.value == 439));
    assert!(f3d_native(result.ir())
        .persistent_references
        .iter()
        .any(|reference| {
            reference.value == 440
                && reference.kind == crate::records::PersistentReferenceKind::CurvePrimary
        }));
    assert_eq!(f3d_native(result.ir()).lost_edge_references.len(), 1);
    assert_eq!(
        f3d_native(result.ir()).lost_edge_references[0].class_tag,
        "419"
    );
    assert_eq!(
        f3d_native(result.ir()).lost_edge_references[0].record_index,
        4645
    );
    assert_eq!(
        f3d_native(result.ir()).lost_edge_references[0].next_record_index,
        4646
    );
    assert!(result.report().losses.iter().any(|loss| loss
        .message
        .contains("source parametric edge reference(s) were marked")));
    assert_eq!(f3d_native(result.ir()).design_types.len(), 12);
    let sketch = f3d_native(result.ir())
        .design_types
        .iter()
        .find(|design_type| design_type.entity_ids.contains(&277))
        .cloned()
        .unwrap();
    assert_eq!(sketch.entity_ids, vec![277]);
    assert_eq!(sketch.version, 4);
    assert_eq!(f3d_native(result.ir()).design_entity_headers.len(), 2);
    let sketch_header = f3d_native(result.ir())
        .design_entity_headers
        .iter()
        .find(|header| header.entity_suffix == 277)
        .cloned()
        .expect("generated sketch entity header");
    assert_eq!(sketch_header.entity_id, "0_277");
    assert_eq!(sketch_header.class_tag, "257");
    assert!(sketch_header.optional_slot_present);
    assert_eq!(
        sketch_header.module.as_deref(),
        Some(crate::records::DESIGN_MODULE_SKETCH)
    );
    assert_eq!(sketch_header.record_reference, Some(584));
    assert_eq!(sketch_header.declared_reference_count, Some(2));
    assert_eq!(sketch_header.reference_indices, [33, 44]);
    assert_eq!(f3d_native(result.ir()).design_record_headers.len(), 6);
    let record_33 = f3d_native(result.ir())
        .design_record_headers
        .iter()
        .find(|record| record.record_index == 33)
        .cloned()
        .expect("record 33");
    assert_eq!(record_33.class_tag, "259");
    assert_eq!(f3d_native(result.ir()).sketch_relations.len(), 2);
    assert_eq!(
        f3d_native(result.ir()).sketch_relations[0].members,
        [100, 200]
    );
    assert_eq!(
        f3d_native(result.ir()).sketch_relations[0].return_members,
        [200, 100]
    );
    assert_eq!(
        f3d_native(result.ir()).sketch_relations[0].owner_reference,
        277
    );
    assert_eq!(
        f3d_native(result.ir()).sketch_relations[0].constraint_kinds,
        [crate::records::SketchConstraintKind::Parallel]
    );
    assert_eq!(
        f3d_native(result.ir()).sketch_relations[0].unknown_constraint_bits,
        0
    );
    assert!(f3d_native(result.ir()).sketch_relations[1]
        .auxiliary_references
        .is_empty());
    assert_eq!(
        f3d_native(result.ir()).sketch_relations[0].raw_bytes.len(),
        101
    );
    assert_eq!(f3d_native(result.ir()).sketch_points.len(), 5);
    let point_500 = f3d_native(result.ir())
        .sketch_points
        .iter()
        .find(|point| point.persistent_id == Some(500))
        .cloned()
        .expect("point 500");
    assert_eq!(point_500.coordinates.u, 12.5);
    assert_eq!(point_500.coordinates.v, -25.0);
    let point_600 = f3d_native(result.ir())
        .sketch_points
        .iter()
        .find(|point| point.persistent_id == Some(600))
        .cloned()
        .expect("point 600");
    assert_eq!(point_600.coordinates.u, -40.0);
    assert_eq!(point_600.entity_genesis, Some(9));
    assert_eq!(f3d_native(result.ir()).sketch_curve_identities.len(), 2);
    assert_eq!(
        f3d_native(result.ir()).sketch_curve_identities[0].primary_id,
        440
    );
    assert_eq!(
        f3d_native(result.ir()).sketch_curve_identities[0].secondary_id,
        0
    );
    assert_eq!(
        f3d_native(result.ir()).sketch_curve_identities[1].entity_genesis,
        Some(10)
    );
    assert!(matches!(
        f3d_native(result.ir()).sketch_curve_identities[0].geometry,
        Some(crate::records::SketchCurveGeometry::Arc { radius: 30.0, .. })
    ));
    assert!(matches!(
        &f3d_native(result.ir()).sketch_curve_identities[1].geometry,
        Some(crate::records::SketchCurveGeometry::Nurbs {
            carrier_reference: Some(42),
            degree: 2,
            weights,
            control_points,
            ..
        }) if weights.is_empty() && control_points.len() == 3
    ));
    assert_eq!(f3d_native(result.ir()).design_body_members.len(), 2);
    assert_eq!(
        f3d_native(result.ir()).design_body_members[0].entity_suffix,
        985
    );
    assert_eq!(
        f3d_native(result.ir()).design_body_members[1].entity_suffix,
        8422
    );
    assert!(f3d_native(result.ir())
        .design_body_members
        .iter()
        .all(|member| member.flags == 0));
    assert!(crate::validate::validate_native(result.ir()).is_empty());
}

#[test]
fn generated_act_native_validation_rejects_structural_drift() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated ACT decode");

    let mut wrong_root = decoded.ir().clone();
    update_f3d_native(&mut wrong_root, |native| {
        native.act_root_components[0].tracked_entity_record = 4;
    });
    assert!(crate::validate::validate_native(&wrong_root)
        .iter()
        .any(|finding| finding.message.contains("ACT root component")));

    let mut table_only = decoded.ir().clone();
    update_f3d_native(&mut table_only, |native| {
        let entity = &mut native.act_entities[0];
        entity.channel_class_tag = None;
        entity.channel_record_index_offset = None;
        entity.channel_entity_id_offset = None;
        entity.channels.clear();
        entity.channel_guid_offsets.clear();
    });
    assert!(crate::validate::validate_native(&table_only)
        .iter()
        .any(|finding| finding.message.contains("ACT entity")));

    let mut shifted_table_row = decoded.ir().clone();
    update_f3d_native(&mut shifted_table_row, |native| {
        native.act_entities[0].table_entity_id_offset = native.act_entities[0]
            .table_entity_id_offset
            .and_then(|offset| offset.checked_add(1));
    });
    assert!(crate::validate::validate_native(&shifted_table_row)
        .iter()
        .any(|finding| finding.message.contains("ACT entity")));

    let mut colliding_root = decoded.ir().clone();
    update_f3d_native(&mut colliding_root, |native| {
        native.act_root_components[0].record_index = native.act_entities[0].record_index;
    });
    assert!(crate::validate::validate_native(&colliding_root)
        .iter()
        .any(|finding| finding.message.contains("ACT root component")));

    let (mut wrong_registry, _, _) = decoded.into_parts();
    update_f3d_native(&mut wrong_registry, |native| {
        native.act_registry_channels[1].ordinal = 0;
    });
    assert!(crate::validate::validate_native(&wrong_registry)
        .iter()
        .any(|finding| finding.message.contains("ACT channel-registry entry")));

    let mut wrong_table_reference = wrong_registry;
    update_f3d_native(&mut wrong_table_reference, |native| {
        native.act_table_references[0].target_record_offset += 1;
    });
    assert!(crate::validate::validate_native(&wrong_table_reference)
        .iter()
        .any(|finding| finding.message.contains("ACT table reference")));
}

#[test]
fn decode_binds_revision_suffixed_protein_visual_guid() {
    let visual = "11111111-2222-3333-4444-555555555555_Post2015_Post2015";
    let f3d = f3d_with_smbh_and_protein_guids(&synthetic_geometry_smbh(), &[visual]);
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .expect("revision-suffixed Protein decode");

    assert_eq!(result.ir().model.appearances.len(), 1);
    assert_eq!(
        result.ir().model.appearances[0].visual_guid.as_deref(),
        Some(visual)
    );
    assert_eq!(result.ir().model.appearance_bindings.len(), 1);
    assert_eq!(
        result.ir().model.appearance_bindings[0].appearance,
        result.ir().model.appearances[0].id
    );
}

#[test]
fn decode_transfers_generated_custom_attribute() {
    let f3d = f3d_with_smbh(&synthetic_geometry_with_attribute_smbh());
    let mut cur = Cursor::new(f3d);
    let result = F3dCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.attributes.len(), 2);
    let attribute = result
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| {
            attribute.values.iter().any(|value| {
                matches!(
                    value,
                    cadmpeg_ir::attributes::AttributeValue::String(text)
                        if text == "generic_tag_attrib_def"
                )
            })
        })
        .expect("generic tag attribute");
    assert_eq!(attribute.name, "ATTRIB_CUSTOM-attrib");
    assert!(matches!(
        &attribute.target,
        cadmpeg_ir::attributes::AttributeTarget::Body(body) if body == &result.ir().model.bodies[0].id
    ));
    assert!(attribute.values.iter().any(|value| matches!(
        value,
        cadmpeg_ir::attributes::AttributeValue::String(text) if text == "322"
    )));
    assert_eq!(f3d_native(result.ir()).persistent_design_links.len(), 2);
    assert_eq!(
        f3d_native(result.ir()).persistent_design_links[1].design_id,
        "322"
    );
    assert_eq!(
        f3d_native(result.ir()).persistent_design_links[1].design_reference,
        7
    );
    assert!(!f3d_native(result.ir()).persistent_design_links[0].is_current);
    assert!(f3d_native(result.ir()).persistent_design_links[1].is_current);
    assert!(attribute.values.iter().any(|value| matches!(
        value,
        cadmpeg_ir::attributes::AttributeValue::String(text) if text == "900"
    )));
    assert_eq!(f3d_native(result.ir()).creation_timestamps.len(), 1);
    assert_eq!(
        f3d_native(result.ir()).creation_timestamps[0].unix_microseconds,
        1_579_392_000_000_007.0
    );
}

#[test]
fn source_less_tolerant_vertex_retains_custom_attribute_ownership() {
    use cadmpeg_ir::attributes::AttributeTarget;

    let mut source = cadmpeg_ir::examples::unit_cube();
    source.source = None;
    source.set_native_unknowns("f3d", &[]).unwrap();
    let vertex = source.model.vertices[0].id.clone();
    source.model.vertices[0].tolerance = Some(0.025);
    f3d_native_mut(&mut source).creation_timestamps = vec![crate::records::CreationTimestamp {
        id: "f3d:asm:creation-timestamp#generated".into(),
        target: AttributeTarget::Vertex(vertex),
        record_index: 0,
        unix_microseconds: 1_579_392_000_000_037.0,
    }];

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less tolerant vertex encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less tolerant vertex decode");

    let tolerant_vertex = round_trip
        .ir()
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.tolerance == Some(0.025))
        .expect("tolerant vertex");
    let attribute = round_trip
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| {
            attribute.name == "ATTRIB_CUSTOM-attrib"
                && attribute.target == AttributeTarget::Vertex(tolerant_vertex.id.clone())
        })
        .expect("tolerant vertex attribute");
    assert_eq!(
        attribute.target,
        AttributeTarget::Vertex(tolerant_vertex.id.clone())
    );
    assert_eq!(
        f3d_native(round_trip.ir()).creation_timestamps[0].unix_microseconds,
        1_579_392_000_000_037.0
    );
}

#[test]
fn generated_f3d_rewrites_creation_timestamp() {
    let source = f3d_with_smbh(&synthetic_geometry_with_attribute_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated timestamp decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected = 1_704_067_200_000_009.0;
    update_f3d_native(&mut edited, |native| {
        assert_eq!(native.creation_timestamps[0].record_index, 20);
        native.creation_timestamps[0].unix_microseconds = expected;
    });

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("timestamp regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated timestamp decode");
    assert_eq!(
        f3d_native(round_trip.ir()).creation_timestamps[0].unix_microseconds,
        expected
    );
}

#[test]
fn decode_transfers_generated_sketch_curve_link() {
    let f3d = f3d_with_smbh(&synthetic_geometry_with_sketch_link_smbh(
        SketchLinkForm::Tagged("113 0 1 0 2 3"),
    ));
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    let link = f3d_native(result.ir())
        .sketch_curve_links
        .first()
        .cloned()
        .unwrap();
    assert_eq!(
        link.target,
        cadmpeg_ir::attributes::AttributeTarget::Coedge(cadmpeg_ir::ids::CoedgeId(
            "f3d:brep:entity#7".into()
        ))
    );
    assert_eq!(link.sketch_curve_id, 113);
    assert_eq!(link.sense, Some(1));
    assert_eq!((link.role, link.closure), (2, 3));
}

/// The one sketch-curve link a synthetic archive carries under `form`.
pub(super) fn decoded_sketch_link(
    form: SketchLinkForm<'_>,
) -> Option<crate::records::SketchCurveLink> {
    let f3d = f3d_with_smbh(&synthetic_geometry_with_sketch_link_smbh(form));
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();
    f3d_native(result.ir()).sketch_curve_links.first().cloned()
}

#[test]
fn a_sketch_link_keeps_the_second_tuple_member_the_source_writes() {
    let link = decoded_sketch_link(SketchLinkForm::Tagged("113 4550 1 0 2 3"))
        .expect("a non-zero second member does not refuse the link");
    assert_eq!((link.sketch_curve_id, link.ref_b), (113, 4550));
    assert_eq!((link.sense, link.role, link.closure), (Some(1), 2, 3));
    // The member reaches the full unsigned 64-bit range, so it does not fit the
    // signed reading the other members take.
    let link = decoded_sketch_link(SketchLinkForm::Tagged("113 18446744073709551615 1 0 2 3"))
        .expect("a second member above i64::MAX does not refuse the link");
    assert_eq!(link.ref_b, u64::MAX);
}

#[test]
fn a_sketch_link_decodes_in_every_payload_form() {
    // Form 2 writes the five members as integers with a trailing `0`; form 0
    // writes them with no trailing member at all.
    for form in [
        SketchLinkForm::Integers(2, &[113, 4550, 1, 2, 3, 0]),
        SketchLinkForm::Integers(0, &[113, 4550, 1, 2, 3]),
    ] {
        let link = decoded_sketch_link(form).expect("integer-form sketch link");
        assert_eq!((link.sketch_curve_id, link.ref_b), (113, 4550));
        assert_eq!((link.sense, link.role, link.closure), (Some(1), 2, 3));
    }
    // An integer form spells the unconstrained sense as the signed `-1` of the
    // same 32-bit pattern the tagged field spells as `4294967295`.
    assert_eq!(
        decoded_sketch_link(SketchLinkForm::Integers(2, &[113, 0, -1, 2, 3, 0]))
            .expect("integer-form sketch link")
            .sense,
        None
    );
    assert!(decoded_sketch_link(SketchLinkForm::Integers(2, &[113, 0, 1, 2, 3])).is_none());
    assert!(decoded_sketch_link(SketchLinkForm::Integers(0, &[113, 0, 1, 2, 3, 0])).is_none());
}

#[test]
fn an_unconstrained_sketch_link_sense_round_trips_in_its_source_spelling() {
    use crate::records::SketchCurveLink;

    let f3d = f3d_with_smbh(&synthetic_geometry_with_sketch_link_smbh(
        SketchLinkForm::Tagged("113 0 4294967295 0 2 3"),
    ));
    let decoded = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        f3d_native(decoded.ir()).sketch_curve_links[0].sense,
        None,
        "4294967295 is the disabled sense, not a stored one"
    );

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let coedge = source_less.model.coedges[0].id.clone();
    f3d_native_mut(&mut source_less).sketch_curve_links = vec![SketchCurveLink {
        id: "generated:sketch-curve-link#0".into(),
        target: cadmpeg_ir::attributes::AttributeTarget::Coedge(coedge),
        sketch_curve_id: 113,
        ref_b: 0,
        sense: None,
        role: 2,
        closure: 3,
    }];
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("source-less sketch-link encode");
    assert!(
        encoded
            .windows(b"113 0 4294967295 0 2 3".len())
            .any(|window| window == b"113 0 4294967295 0 2 3"),
        "the writer must re-emit the disabled sense in its source spelling"
    );
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less sketch-link round trip");
    let link = &f3d_native(round_trip.ir()).sketch_curve_links[0];
    assert_eq!((link.sketch_curve_id, link.sense), (113, None));
    assert_eq!((link.role, link.closure), (2, 3));
}

#[test]
fn decode_mixed_analytic_and_unknown_faces_sharing_an_edge() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let f3d = f3d_with_smbh(&synthetic_mixed_smbh());
    let mut cur = Cursor::new(f3d);
    let result = F3dCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert!(result.report().geometry_transferred);
    // Two faces (one plane, one spline), sharing one edge; five edges total.
    assert_eq!(result.ir().model.faces.len(), 2);
    assert_eq!(result.ir().model.edges.len(), 5);
    assert_eq!(result.ir().model.vertices.len(), 4);
    assert_eq!(result.ir().model.coedges.len(), 6);

    // Exactly one analytic (plane) and one unknown surface.
    let planes = result
        .ir()
        .model
        .surfaces
        .iter()
        .filter(|s| matches!(s.geometry, SurfaceGeometry::Plane { .. }))
        .count();
    let unknowns = result
        .ir()
        .model
        .surfaces
        .iter()
        .filter(|s| matches!(s.geometry, SurfaceGeometry::Unknown { .. }))
        .count();
    assert_eq!((planes, unknowns), (1, 1));

    // The shared edge is used by two mutually-referencing coedges of opposite
    // sense (the manifold invariant), which coedge-pairing validation enforces.
    let paired = result
        .ir()
        .model
        .coedges
        .iter()
        .filter(|c| c.radial_next != c.id)
        .count();
    assert_eq!(paired, 2);

    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
    assert_eq!(result.ir().model.surfaces.len(), 2);
}

#[test]
fn body_visibility_maps_asm_keys_through_member_nodes() {
    fn lp_utf16(out: &mut Vec<u8>, value: &str) {
        let units: Vec<u16> = value.encode_utf16().collect();
        out.extend_from_slice(&(units.len() as u32).to_le_bytes());
        for unit in units {
            out.extend_from_slice(&unit.to_le_bytes());
        }
    }

    let mut bulk = Vec::new();
    let mut primary_records = vec![(899u64, 0u64)];
    // Typed body-map record: indexed header, ten zero bytes, pair count,
    // (ASM key, member) pairs, the 12-byte tail, then the blob name.
    bulk.extend_from_slice(&3u32.to_le_bytes());
    bulk.extend_from_slice(b"256");
    bulk.extend_from_slice(&899u32.to_le_bytes());
    bulk.extend_from_slice(&[0; crate::design::body::GENERATED_BODY_MAP_ZERO_PREFIX_LEN]);
    bulk.extend_from_slice(&2u32.to_le_bytes());
    for (key, member) in [(3u64, 269u64), (6, 533)] {
        bulk.extend_from_slice(&key.to_le_bytes());
        bulk.extend_from_slice(&member.to_le_bytes());
    }
    bulk.extend_from_slice(&1793u64.to_le_bytes());
    bulk.extend_from_slice(&0u32.to_le_bytes());
    lp_utf16(&mut bulk, "BREP.synthetic.smbh");
    // Typed browser-node records: indexed header, ten-byte base payload,
    // GUID, hidden flag, `01 01` marker, and member id.
    for (record_index, guid, hidden, member) in [
        (900u32, "b412e170-dc0c-4932-b699-43fc72cc8b13", 0u8, 269u64),
        (901, "d4b1078c-43bf-4f6d-a50a-963f94273901", 1, 533),
    ] {
        primary_records.push((
            u64::from(record_index),
            u64::try_from(bulk.len()).expect("synthetic BulkStream offset"),
        ));
        bulk.extend_from_slice(&3u32.to_le_bytes());
        bulk.extend_from_slice(b"257");
        bulk.extend_from_slice(&record_index.to_le_bytes());
        bulk.extend_from_slice(&[0; 10]);
        lp_utf16(&mut bulk, guid);
        bulk.push(hidden);
        bulk.extend_from_slice(&[0x01, 0x01]);
        bulk.extend_from_slice(&member.to_le_bytes());
    }

    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&bulk).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&design_metastream_with_records(
        &[
            (
                crate::design::body::BODY_MAP_CARRIER_TYPE_GUID,
                crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID,
                crate::design::body::BODY_MAP_CARRIER_TYPE_VERSION,
                crate::records::DESIGN_MODULE_BODY,
                &[899],
            ),
            (
                crate::design::presentation::BROWSER_NODE_TYPE_GUID,
                crate::design::presentation::BROWSER_NODE_BASE_TYPE_GUID,
                crate::design::presentation::BROWSER_NODE_TYPE_VERSION,
                crate::records::DESIGN_MODULE_FUSION,
                &[900, 901],
            ),
        ],
        &primary_records,
    ))
    .unwrap();
    let bytes = zip.finish().unwrap().into_inner();

    with_scan(&bytes, |scan| {
        let visibility = crate::design::decode::body::decode_all_body_visibility(scan).unwrap();
        assert_eq!(
            visibility
                .get(&("BREP.synthetic.smbh".into(), 3))
                .map(|item| item.visible),
            Some(true),
            "flag 0 decodes visible"
        );
        assert_eq!(
            visibility
                .get(&("BREP.synthetic.smbh".into(), 6))
                .map(|item| item.visible),
            Some(false),
            "flag 1 decodes hidden"
        );

        assert!(!visibility.contains_key(&("BREP.other.smbh".into(), 3)));
    });
}

#[test]
fn protein_revision_suffix_distinguishes_visual_record_identity() {
    assert!(!crate::materials::visual_tokens_match(
        "7DD7765D-CA8C-4A38-B156-B3B4916E0C17_Post2015_Post2015",
        "7dd7765d-ca8c-4a38-b156-b3b4916e0c17",
    ));
    assert!(crate::materials::visual_tokens_match(
        "7DD7765D-CA8C-4A38-B156-B3B4916E0C17_Post2015",
        "7dd7765d-ca8c-4a38-b156-b3b4916e0c17_Post2015",
    ));
    assert!(!crate::materials::visual_tokens_match(
        "not-a-guid_Post2015",
        "not-a-guid",
    ));
}

#[test]
fn browser_body_appearance_joins_through_browser_node_guid() {
    let mut bytes = vec![0u8; 8];
    let records = [
        (
            "1b5e92d0-eade-40d5-ab4d-35af2eb411b4",
            "674E6024-4294-4322-B572-A88F64F0DA77_Post2015_Post2015",
            37_251u64,
        ),
        (
            "a349885b-a9b6-4b79-a9c9-7976717ee6be",
            "4218E352-E25F-423E-8DCD-527E5148C2F6_Post2015_Post2015",
            37_441u64,
        ),
    ];
    for (node_guid, visual, entity_suffix) in records {
        for value in [
            "e966e81d-2581-4d41-821d-839938974425",
            node_guid,
            "DE897CF7-F483-4D31-A2D8-41671FE36D3D",
            "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C",
            "PrismMaterial-018",
            "ba2d3026-32c4-4584-b0e1-a738e387fa35",
            visual,
            "BA5EE55E-9982-449B-9D66-9F036540E140",
            "Prism-090",
        ] {
            bytes.extend(lp_utf16_bytes(value));
        }
        bytes.extend(lp_utf16_bytes(node_guid));
        bytes.push(0);
        bytes.extend([0x01, 0x01]);
        bytes.extend(entity_suffix.to_le_bytes());
    }

    assert_eq!(
        crate::materials::browser_body_appearances(&bytes),
        [
            (37_251, records[0].1.to_string()),
            (37_441, records[1].1.to_string()),
        ]
    );
    assert!(
        crate::materials::face_appearance_assignments(&bytes).is_empty(),
        "a body-owned visual marker is not also a face assignment"
    );
}

#[test]
fn legacy_face_appearance_assignment_decodes_both_variable_width_forms() {
    let face_guid = "cd92d0f6-5b31-4bbf-84ae-4611f435537e";
    let visual_guid = "F0EF16AD-4AD3-4D25-9AA8-ECF48936A48F_Post2015";
    let first = legacy_face_appearance_entry(
        face_guid,
        [0.25, 0.5, 0.75, 1.0],
        visual_guid,
        0,
        None,
        "Prism-042",
    );
    let second = legacy_face_appearance_entry(
        "e6c14fe2-6c11-4a22-8ccc-c10fba912345",
        [0.75, 0.25, 0.5, 1.0],
        "A1C44310-E91B-4B59-B527-18265C123456_Post2015",
        1,
        Some("X"),
        "PrismOpaque",
    );
    assert_ne!(first.len(), second.len());

    let mut bytes = vec![0u8; 8];
    bytes.extend(first);
    bytes.extend(second);
    let out = crate::materials::face_appearance_assignments(&bytes);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].face_guid, face_guid);
    assert_eq!(out[0].visual_guid, visual_guid);
    assert_eq!(
        out[0].color,
        Some(cadmpeg_ir::topology::Color {
            r: 0.25,
            g: 0.5,
            b: 0.75,
            a: 1.0,
        })
    );
    assert_eq!(
        out[1].color,
        Some(cadmpeg_ir::topology::Color {
            r: 0.75,
            g: 0.25,
            b: 0.5,
            a: 1.0,
        })
    );
}

#[test]
fn face_appearance_assignment_rejects_entity_id_and_uppercase_targets() {
    for target in [
        "0_985",
        "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C",
        "c1eea57c-3f56-45fc-b8cb-a9ec46a9994C",
    ] {
        let bytes = legacy_face_appearance_entry(
            target,
            [0.25, 0.5, 0.75, 1.0],
            "F0EF16AD-4AD3-4D25-9AA8-ECF48936A48F_Post2015",
            1,
            None,
            "PrismOpaque",
        );
        assert!(crate::materials::face_appearance_assignments(&bytes).is_empty());
    }
}

#[test]
fn legacy_face_appearance_assignment_rejects_partial_and_malformed_envelopes() {
    let face_guid = "cd92d0f6-5b31-4bbf-84ae-4611f435537e";
    let visual_guid = "F0EF16AD-4AD3-4D25-9AA8-ECF48936A48F_Post2015";
    let mut partial = lp_utf16_bytes(face_guid);
    partial.extend(lp_utf16_bytes(visual_guid));
    partial.extend(lp_utf16_bytes("BA5EE55E-9982-449B-9D66-9F036540E140"));
    assert!(crate::materials::face_appearance_assignments(&partial).is_empty());

    let mut malformed = legacy_face_appearance_entry(
        face_guid,
        [0.25, 0.5, 0.75, 1.0],
        visual_guid,
        1,
        None,
        "PrismOpaque",
    );
    let carrier_at = lp_utf16_bytes(face_guid).len() + 4 * size_of::<f32>();
    malformed[carrier_at + 2] = 1;
    assert!(crate::materials::face_appearance_assignments(&malformed).is_empty());
}

pub(super) fn legacy_face_appearance_entry(
    face_guid: &str,
    color: [f32; 4],
    visual_guid: &str,
    selector_kind: u8,
    display_name: Option<&str>,
    selector: &str,
) -> Vec<u8> {
    let mut bytes = lp_utf16_bytes(face_guid);
    for component in color {
        bytes.extend(component.to_le_bytes());
    }
    bytes.extend([1, 1]);
    bytes.extend([0; 9]);
    bytes.push(selector_kind);
    bytes.extend(lp_utf16_bytes(visual_guid));
    bytes.extend(lp_utf16_bytes("BA5EE55E-9982-449B-9D66-9F036540E140"));
    if let Some(display_name) = display_name {
        bytes.extend(lp_utf16_bytes(display_name));
    } else {
        bytes.extend(0_u32.to_le_bytes());
    }
    bytes.extend(lp_utf16_bytes(selector));
    bytes.extend(0_f32.to_le_bytes());
    bytes.extend(1_f32.to_le_bytes());
    bytes
}

#[test]
fn modern_face_appearance_assignment_uses_second_framed_lowercase_guid() {
    let unrelated_guid = "11111111-1111-1111-1111-111111111111";
    let first_guid = "22222222-2222-2222-2222-222222222222";
    let face_guid = "33333333-3333-3333-3333-333333333333";
    let visual_guid = "F0EF16AD-4AD3-4D25-9AA8-ECF48936A48F_Post2015";
    let mut bytes = lp_utf16_bytes(unrelated_guid);
    bytes.extend(lp_utf16_bytes(first_guid));
    bytes.extend([0xa5; 8]);
    bytes.extend([0; 8]);
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend([1, 1, 0, 0, 0]);
    bytes.extend(lp_utf16_bytes(face_guid));
    bytes.extend([0; 12]);
    bytes.extend(1_f32.to_le_bytes());
    bytes.extend([1, 1]);
    bytes.extend([0; 10]);
    for value in [visual_guid, "08861000-1D69-CF2A-C082-CBD98E7E5D7F"] {
        bytes.extend(lp_utf16_bytes(value));
    }
    bytes.extend(0_u32.to_le_bytes());
    bytes.extend(lp_utf16_bytes("005E1000-55CE-AFB6-81A1-36E3EF077C5F"));
    let out = crate::materials::face_appearance_assignments(&bytes);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].face_guid, face_guid);
    assert_eq!(out[0].visual_guid, visual_guid);
    assert_eq!(out[0].color, None);

    let mut malformed = bytes;
    let first_gap_at = lp_utf16_bytes(unrelated_guid).len() + lp_utf16_bytes(first_guid).len();
    malformed[first_gap_at + 8] = 1;
    assert!(crate::materials::face_appearance_assignments(&malformed).is_empty());
}

#[test]
fn modern_face_appearance_assignment_requires_the_first_guid_carrier() {
    let mut bytes = vec![0u8; 8];
    for value in [
        "22222222-2222-2222-2222-222222222222",
        "F0EF16AD-4AD3-4D25-9AA8-ECF48936A48F_Post2015",
        "08861000-1D69-CF2A-C082-CBD98E7E5D7F",
    ] {
        bytes.extend(lp_utf16_bytes(value));
    }
    bytes.extend(0_u32.to_le_bytes());
    bytes.extend(lp_utf16_bytes("005E1000-55CE-AFB6-81A1-36E3EF077C5F"));
    assert!(crate::materials::face_appearance_assignments(&bytes).is_empty());
}

#[test]
fn modern_body_appearance_is_not_a_face_assignment() {
    let mut bytes = vec![0u8; 8];
    for value in [
        "11111111-1111-1111-1111-111111111111",
        "PrismMaterial-018",
        "F0EF16AD-4AD3-4D25-9AA8-ECF48936A48F_Post2015",
        "08861000-1D69-CF2A-C082-CBD98E7E5D7F",
    ] {
        bytes.extend(lp_utf16_bytes(value));
    }
    bytes.extend(0_u32.to_le_bytes());
    bytes.extend(lp_utf16_bytes("005E1000-55CE-AFB6-81A1-36E3EF077C5F"));
    assert!(crate::materials::face_appearance_assignments(&bytes).is_empty());
}
