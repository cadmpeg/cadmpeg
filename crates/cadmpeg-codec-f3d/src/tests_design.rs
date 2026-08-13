// SPDX-License-Identifier: Apache-2.0
//! Design-domain synthetic tests and fixtures.

use super::*;

#[test]
fn generated_source_less_rejects_act_without_segment_metadata() {
    use crate::records::ActEntity;

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let mut native = f3d_native_mut(&mut source_less);
    native.act_entities = vec![ActEntity {
        id: "generated:act-entity#0".into(),
        record_index: 7,
        table_record_index_offset: None,
        channel_record_index_offset: None,
        entity_id: "0_985".into(),
        table_entity_id_offset: None,
        channel_entity_id_offset: None,
        in_table: true,
        channel_class_tag: None,
        channels: Default::default(),
        channel_guid_offsets: Default::default(),
    }];
    drop(native);
    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("ACT generation without its record registry must fail atomically");
    assert!(error
        .to_string()
        .contains("requires a retained MetaStream record registry"));
}

#[test]
fn generated_source_less_writes_unassigned_protein_appearance() {
    use std::collections::BTreeMap;

    use cadmpeg_ir::appearance::Appearance;
    use cadmpeg_ir::ids::AppearanceId;
    use cadmpeg_ir::topology::Color;

    let visual_guid = "11111111-2222-3333-4444-555555555555";
    let appearance_id = AppearanceId("generated:appearance#0".into());
    let mut source_less = cadmpeg_ir::examples::unit_cube();
    source_less.model.appearances = vec![Appearance {
        id: appearance_id.clone(),
        name: Some("Prism-Generated".into()),
        asset_guid: Some(visual_guid.into()),
        library_id: None,
        visual_guid: Some(visual_guid.into()),
        physical_token: Some("PrismMaterial-Generated".into()),
        schema: Some("GenericSchema".into()),
        category: Some("Plastic/Generated".into()),
        base_color: Some(Color {
            r: 0.15,
            g: 0.35,
            b: 0.75,
            a: 1.0,
        }),
        properties: BTreeMap::from([
            ("reflectivity_at_0deg".into(), 0.25),
            ("refraction_index".into(), 1.5),
        ]),
        textures: Vec::new(),
    }];
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less Protein appearance encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less Protein appearance round trip");
    assert_eq!(round_trip.ir().model.appearances.len(), 1);
    let appearance = &round_trip.ir().model.appearances[0];
    assert_eq!(appearance.name.as_deref(), Some("Prism-Generated"));
    assert_eq!(appearance.visual_guid.as_deref(), Some(visual_guid));
    assert_eq!(appearance.schema.as_deref(), Some("GenericSchema"));
    assert_eq!(appearance.category.as_deref(), Some("Plastic/Generated"));
    assert_eq!(
        appearance.base_color,
        Some(Color {
            r: 0.15,
            g: 0.35,
            b: 0.75,
            a: 1.0,
        })
    );
    assert_eq!(
        appearance.properties.get("reflectivity_at_0deg"),
        Some(&0.25)
    );
    assert_eq!(appearance.properties.get("refraction_index"), Some(&1.5));
    assert!(round_trip.ir().model.appearance_bindings.is_empty());
    assert!(crate::validate::validate_native(round_trip.ir()).is_empty());
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new());
    assert!(
        validation.is_ok(),
        "validation findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_rejects_material_assignment_without_presentation_graph() {
    use crate::records::DesignMaterialAssignment;

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    f3d_native_mut(&mut source_less).design_material_assignments = vec![DesignMaterialAssignment {
        id: "generated:material-assignment#0".into(),
        asm_body_key: 42,
        asm_body_key_offset: 0,
        entity_suffix: 985,
        entity_suffix_offset: 0,
        entity_id: "0_985".into(),
        entity_id_offset: 0,
        visual_guid: "11111111-2222-3333-4444-555555555555".into(),
        visual_guid_offset: 0,
        physical_token: Some("PrismMaterial-Generated".into()),
        physical_token_offset: None,
        visual_preset: None,
        visual_preset_offset: None,
    }];

    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("an incomplete generated presentation graph must be refused");
    assert!(error
        .to_string()
        .contains("requires a typed body-presentation B-rep and scene graph"));
}

#[test]
fn generated_source_less_rejects_collapsed_visibility_body_bindings() {
    let mut source_less = cadmpeg_ir::examples::unit_cube();
    source_less.model.bodies[0].visible = Some(false);
    let body = source_less.model.bodies[0].id.clone();
    f3d_native_mut(&mut source_less).body_visibilities = [985, 986]
        .into_iter()
        .enumerate()
        .map(|(ordinal, entity_suffix)| crate::records::BodyVisibility {
            id: format!("generated:body-visibility#{ordinal}"),
            body: body.clone(),
            stream: "generated/Design1/BulkStream.dat".into(),
            byte_offset: 0,
            asm_body_key_offset: 0,
            asm_body_key: 42,
            entity_suffix,
            visible: false,
        })
        .collect();

    let error = F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("conflicting body-map rows must not collapse");
    assert!(error
        .to_string()
        .contains("conflicts with the body-map key/suffix bijection"));
}

#[test]
fn generated_f3d_rewrites_native_sketch_point_coordinates() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected = update_f3d_native(&mut edited, |native| {
        let point = &mut native.sketch_points[0];
        point.coordinates.u += 12.5;
        point.coordinates.v -= 7.5;
        point.coordinates
    });

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("native sketch-point regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        f3d_native(round_trip.ir()).sketch_points[0].coordinates,
        expected
    );
}

#[test]
fn generated_f3d_rewrites_native_sketch_arc_geometry() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected = update_f3d_native(&mut edited, |native| {
        let curve = &mut native.sketch_curve_identities[0];
        let Some(crate::records::SketchCurveGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
            ..
        }) = &mut curve.geometry
        else {
            panic!("generated sketch curve must be an arc")
        };
        center.x += 20.0;
        *radius = 35.0;
        *start_angle = 0.25;
        *end_angle = 2.75;
        curve.geometry.clone()
    });

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("native sketch-arc regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        f3d_native(round_trip.ir()).sketch_curve_identities[0].geometry,
        expected
    );
}

#[test]
fn generated_f3d_rewrites_native_sketch_constraint_mask() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected_references = update_f3d_native(&mut edited, |native| {
        let relation = &mut native.sketch_relations[0];
        relation.state = 0x40;
        relation.constraint_kinds = vec![crate::records::SketchConstraintKind::Horizontal];
        relation.unknown_constraint_bits = 0;
        relation.members.reverse();
        for reference in &mut relation.auxiliary_references {
            *reference = reference.saturating_add(1);
        }
        relation.return_members.reverse();
        (
            relation.members.clone(),
            relation.auxiliary_references.clone(),
            relation.owner_reference,
            relation.return_members.clone(),
        )
    });

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("native sketch-constraint regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    let native = f3d_native(round_trip.ir());
    let relation = &native.sketch_relations[0];
    assert_eq!(relation.state, 0x40);
    assert_eq!(
        relation.constraint_kinds,
        [crate::records::SketchConstraintKind::Horizontal]
    );
    assert_eq!(relation.unknown_constraint_bits, 0);
    assert_eq!(relation.members, expected_references.0);
    assert_eq!(relation.auxiliary_references, expected_references.1);
    assert_eq!(relation.owner_reference, expected_references.2);
    assert_eq!(relation.return_members, expected_references.3);
}

#[test]
fn generated_f3d_rewrites_native_sketch_nurbs_values() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected = update_f3d_native(&mut edited, |native| {
        let curve = &mut native.sketch_curve_identities[1];
        let Some(crate::records::SketchCurveGeometry::Nurbs {
            fit_tolerance,
            control_points,
            ..
        }) = &mut curve.geometry
        else {
            panic!("generated sketch curve must be NURBS")
        };
        *fit_tolerance = 0.125;
        control_points[1].x += 15.0;
        control_points[1].y -= 5.0;
        curve.geometry.clone()
    });

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("native sketch-NURBS regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        f3d_native(round_trip.ir()).sketch_curve_identities[1].geometry,
        expected
    );
}

#[test]
fn generated_f3d_rewrites_body_transform() {
    let source = f3d_with_smbh(&synthetic_geometry_with_transform_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    assert_eq!(f3d_native(decoded.ir()).transform_hints.len(), 1);
    assert!(!f3d_native(decoded.ir()).transform_hints[0].rotation);
    let (mut edited, _, fidelity) = decoded.into_parts();
    let transform = edited.model.bodies[0]
        .transform
        .as_mut()
        .expect("generated body transform");
    transform.rows[0][3] = 125.0;
    transform.rows[1][3] = -75.0;
    transform.rows[2][3] = 50.0;
    transform.rows[3][3] = 2.0;
    let expected = *transform;
    f3d_native_mut(&mut edited).transform_hints[0].reflection = true;
    f3d_native_mut(&mut edited).body_native_keys[0].asm_body_key = Some(84);

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("body-transform regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(round_trip.ir().model.bodies[0].transform, Some(expected));
    assert!(!f3d_native(round_trip.ir()).transform_hints[0].rotation);
    assert!(f3d_native(round_trip.ir()).transform_hints[0].reflection);
    assert_eq!(
        f3d_native(round_trip.ir()).body_native_keys[0].asm_body_key,
        Some(84)
    );
}

#[test]
fn body_key_edit_does_not_rewrite_ordinal_design_selector() {
    let body = cadmpeg_ir::ids::BodyId("f3d:brep:entity#1".into());
    let mut baseline = crate::native::F3dNative::default();
    baseline
        .body_native_keys
        .push(cadmpeg_asm::brep::records::BodyNativeKey {
            id: "f3d:asm:body-native-key#1".into(),
            body: body.clone(),
            record_index: 1,
            body_ordinal: 0,
            source_brep: Some("BREP.source.smb".into()),
            asm_body_key: Some(436),
        });
    baseline
        .body_visibilities
        .push(crate::records::BodyVisibility {
            id: "f3d:design:body-visibility#1".into(),
            body,
            stream: "Design1/BulkStream.dat".into(),
            byte_offset: 20,
            asm_body_key_offset: 40,
            asm_body_key: 0,
            entity_suffix: 1,
            visible: true,
        });
    let mut target = baseline.clone();
    target.body_native_keys[0].asm_body_key = Some(500);

    let edits = crate::writer::patch::edits::validate_body_native_key_edits(
        crate::writer::patch::edits::PatchNatives {
            baseline: Some(&baseline),
            target: Some(&target),
        },
    )
    .expect("body-key edit");

    assert_eq!(edits.asm.get(&1), Some(&500));
    assert!(edits.design.is_empty());
}

#[test]
fn generated_f3d_rewrites_design_recipe_and_persistent_reference() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated Design decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let mut native = f3d_native(&edited);
    let reference = native
        .persistent_references
        .iter_mut()
        .find(|reference| reference.value == 439)
        .expect("generated persistent reference");
    assert!(reference.byte_offset > 0);
    assert!(reference.value_offset > 0);
    reference.value = 9_001;
    let recipe = &mut native.construction_recipes[0];
    assert!(recipe.byte_offset > 0);
    assert!(recipe.record_index_offset.is_some());
    assert!(recipe.design_id_offset.is_some());
    recipe.record_index = 777;
    recipe.design_id = Some("333".into());
    let member = native
        .design_body_members
        .iter_mut()
        .find(|member| member.entity_suffix == 985)
        .expect("generated body member");
    assert!(member.byte_offset > 0);
    member.entity_suffix = 12_345;
    member.flags = 7;
    let header = native
        .design_entity_headers
        .iter_mut()
        .find(|header| header.in_sketch_module())
        .expect("generated sketch entity header");
    assert!(header.byte_offset > 0);
    assert!(header.record_reference_offset.is_some());
    assert_eq!(header.reference_offsets.len(), 2);
    header.record_reference = Some(585);
    header.reference_indices.swap(0, 1);
    let object = native
        .design_types
        .iter_mut()
        .find(|design_type| design_type.entity_ids == [33, 44])
        .expect("generated relation design type");
    assert!(object.byte_offset < object.version_offset);
    assert_eq!(object.entity_id_offsets.len(), 2);
    object.type_guid = "91111111-2222-3333-4444-555555555555".into();
    object.base_type_guid = Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeef".into());
    object.version = 9;
    let act_guid = native
        .act_guids
        .iter_mut()
        .find(|guid| guid.guid == "eeeeeeee-1111-2222-3333-ffffffffffff")
        .expect("generated standalone ACT GUID");
    assert!(act_guid.guid_offset > act_guid.byte_offset);
    act_guid.guid = "ffffffff-1111-2222-3333-444444444444".into();
    native.act_registry_channels[0].guid = "dddddddd-1111-2222-3333-eeeeeeeeeeee".into();
    let act_root = &mut native.act_root_components[0];
    act_root.instance_root_record = 71;
    act_root.components_root_record = 72;
    act_root.registry_flag = 0;
    act_root.entity_id = "1_3".into();
    act_root.display_name = "(Renamed)".into();
    let act_entity = &mut native.act_entities[0];
    assert!(act_entity.table_entity_id_offset.is_some());
    assert!(act_entity.channel_entity_id_offset.is_some());
    act_entity.channels.insert(
        "Appearance".into(),
        "dddddddd-1111-2222-3333-eeeeeeeeeeee".into(),
    );
    let binding = &mut edited.model.appearance_bindings[0];
    binding.channels.insert(
        "Appearance".into(),
        "dddddddd-1111-2222-3333-eeeeeeeeeeee".into(),
    );
    let lost_edge = &mut native.lost_edge_references[0];
    assert!(lost_edge.class_tag_offset > lost_edge.record_byte_offset);
    assert!(lost_edge.class_tag_offset < lost_edge.byte_offset);
    lost_edge.class_tag = "420".into();
    lost_edge.record_index = 4_700;
    let assignment = &mut native.design_material_assignments[0];
    assert!(assignment.entity_id_offset > 0);
    assert!(assignment.asm_body_key_offset > 0);
    assignment.physical_token = Some("PrismMaterial-019".into());
    assignment.visual_preset = Some("Prism-002".into());
    native.body_native_keys[0].asm_body_key = Some(84);
    edited.model.appearances[0].physical_token = Some("PrismMaterial-019".into());
    edited.model.appearances[0].base_color = Some(cadmpeg_ir::topology::Color {
        r: 0.8,
        g: 0.6,
        b: 0.4,
        a: 1.0,
    });
    edited.model.appearances[0]
        .properties
        .insert("reflectivity_at_0deg".into(), 0.7);
    edited.model.appearances[0]
        .properties
        .insert("refraction_index".into(), 1.8);
    assert_eq!(
        native.act_entities[0].entity_id,
        native.design_material_assignments[0].entity_id
    );
    native.store(edited.native.namespace_mut("f3d")).unwrap();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("persistent-reference regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated Design decode");
    assert_eq!(
        f3d_native(round_trip.ir()).design_material_assignments[0].asm_body_key,
        84
    );
    assert!(f3d_native(round_trip.ir())
        .persistent_references
        .iter()
        .any(|reference| reference.value == 9_001));
    assert_eq!(
        f3d_native(round_trip.ir()).construction_recipes[0].record_index,
        777
    );
    assert_eq!(
        f3d_native(round_trip.ir()).construction_recipes[0]
            .design_id
            .as_deref(),
        Some("333")
    );
    assert!(f3d_native(round_trip.ir())
        .design_body_members
        .iter()
        .any(|member| member.entity_suffix == 12_345 && member.flags == 7));
    let header = f3d_native(round_trip.ir())
        .design_entity_headers
        .iter()
        .find(|header| header.in_sketch_module())
        .cloned()
        .expect("round-trip sketch entity header");
    assert_eq!(header.entity_suffix, 277);
    assert_eq!(header.entity_id, "0_277");
    assert_eq!(header.record_reference, Some(585));
    assert_eq!(header.reference_indices, [44, 33]);
    let object = f3d_native(round_trip.ir())
        .design_types
        .iter()
        .find(|design_type| design_type.entity_ids == [33, 44])
        .cloned()
        .expect("round-trip relation design type");
    assert_eq!(object.entity_ids, [33, 44]);
    assert_eq!(object.type_guid, "91111111-2222-3333-4444-555555555555");
    assert_eq!(
        object.base_type_guid.as_deref(),
        Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeef")
    );
    assert_eq!(object.version, 9);
    assert!(f3d_native(round_trip.ir())
        .act_guids
        .iter()
        .any(|guid| guid.guid == "ffffffff-1111-2222-3333-444444444444"));
    let act_root = &f3d_native(round_trip.ir()).act_root_components[0];
    assert_eq!(act_root.record_index, 9);
    assert_eq!(act_root.instance_root_record, 71);
    assert_eq!(act_root.components_root_record, 72);
    assert_eq!(act_root.registry_flag, 0);
    assert_eq!(act_root.entity_id, "1_3");
    assert_eq!(act_root.display_name, "(Renamed)");
    assert_eq!(
        f3d_native(round_trip.ir()).act_registry_channels[0].guid,
        "dddddddd-1111-2222-3333-eeeeeeeeeeee"
    );
    let act_entity = &f3d_native(round_trip.ir()).act_entities[0];
    assert_eq!(act_entity.entity_id, "0_985");
    assert_eq!(
        act_entity.channels.get("Appearance").map(String::as_str),
        Some("dddddddd-1111-2222-3333-eeeeeeeeeeee")
    );
    let binding = &round_trip.ir().model.appearance_bindings[0];
    assert_eq!(binding.source_entity_id.as_deref(), Some("0_985"));
    assert_eq!(
        binding.channels.get("Appearance").map(String::as_str),
        Some("dddddddd-1111-2222-3333-eeeeeeeeeeee")
    );
    let lost_edge = &f3d_native(round_trip.ir()).lost_edge_references[0];
    assert_eq!(lost_edge.class_tag, "420");
    assert_eq!(lost_edge.record_index, 4_700);
    assert_eq!(
        f3d_native(round_trip.ir()).design_material_assignments[0].entity_id,
        "0_985"
    );
    assert_eq!(
        f3d_native(round_trip.ir()).design_material_assignments[0]
            .visual_preset
            .as_deref(),
        Some("Prism-002")
    );
    assert_eq!(
        round_trip.ir().model.appearances[0]
            .physical_token
            .as_deref(),
        Some("PrismMaterial-019")
    );
    assert_eq!(
        round_trip.ir().model.appearances[0].base_color,
        Some(cadmpeg_ir::topology::Color {
            r: 0.8,
            g: 0.6,
            b: 0.4,
            a: 1.0,
        })
    );
    assert_eq!(
        round_trip.ir().model.appearances[0]
            .properties
            .get("reflectivity_at_0deg"),
        Some(&0.7)
    );
    assert_eq!(
        round_trip.ir().model.appearances[0]
            .properties
            .get("refraction_index"),
        Some(&1.8)
    );
}

#[test]
fn generated_f3d_rejects_act_binding_divergence() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated ACT decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut edited, |native| {
        native.act_entities[0].channels.insert(
            "Appearance".into(),
            "dddddddd-1111-2222-3333-eeeeeeeeeeee".into(),
        );
    });

    let error = F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut Vec::new())
        .expect_err("divergent ACT and appearance binding must fail");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
}

#[test]
fn generated_f3d_rejects_act_record_index_edit_without_metastream_edit() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated ACT decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut edited, |native| {
        native.act_root_components[0].record_index += 1;
    });

    let error = F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut Vec::new())
        .expect_err("an ACT record-index edit without its MetaStream index must fail");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::NotImplemented(message)
            if message.contains("ACT root edit changes fields")
    ));
}

#[test]
fn generated_f3d_rejects_material_assignment_divergence() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated material decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut edited, |native| {
        native.design_material_assignments[0].physical_token = Some("PrismMaterial-019".into());
    });

    let error = F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut Vec::new())
        .expect_err("divergent assignment and appearance must fail");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
}

#[test]
fn generated_f3d_rejects_partial_material_assignment_identity_edit() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated material decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut edited, |native| {
        let assignment = &mut native.design_material_assignments[0];
        assignment.entity_id = "0_986".into();
        assignment.entity_suffix = 986;
    });

    let error = F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut Vec::new())
        .expect_err("a partial presentation-graph identity edit must fail");
    assert!(error.to_string().contains(
        "requires synchronized body-presentation, browser-node, B-rep, and scene graphs"
    ));
}

#[test]
fn generated_f3d_rejects_invalid_or_structural_protein_property_edits() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated Protein decode");

    let mut invalid = decoded.ir().clone();
    invalid.model.appearances[0]
        .properties
        .insert("refraction_index".into(), 0.5);
    let error = F3dCodec
        .write_preserved_with_source_fidelity(&invalid, decoded.source_fidelity(), &mut Vec::new())
        .expect_err("out-of-range refraction must be refused");
    assert!(
        matches!(error, cadmpeg_core::CodecError::Malformed(message) if message.contains("refraction_index"))
    );

    let (mut structural, _, fidelity) = decoded.into_parts();
    structural.model.appearances[0]
        .properties
        .insert("unserialized_property".into(), 0.5);
    let error = F3dCodec
        .write_preserved_with_source_fidelity(&structural, &fidelity, &mut Vec::new())
        .expect_err("new Protein property must be refused");
    assert!(
        matches!(error, cadmpeg_core::CodecError::NotImplemented(message) if message.contains("unchanged property set"))
    );
}

#[test]
fn generated_f3d_routes_appearance_edits_across_multiple_protein_assets() {
    let source = f3d_with_smbh_and_protein_guids(
        &synthetic_geometry_smbh(),
        &[
            "11111111-2222-3333-4444-555555555555",
            "99999999-2222-3333-4444-555555555555",
        ],
    );
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated multi-Protein decode");
    assert_eq!(decoded.ir().model.appearances.len(), 2);
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.appearances[0].base_color = Some(cadmpeg_ir::topology::Color {
        r: 0.2,
        g: 0.3,
        b: 0.4,
        a: 1.0,
    });
    edited.model.appearances[1].base_color = Some(cadmpeg_ir::topology::Color {
        r: 0.6,
        g: 0.7,
        b: 0.8,
        a: 1.0,
    });

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("multi-Protein appearance regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated multi-Protein decode");
    assert_eq!(round_trip.ir().model.appearances, edited.model.appearances);
}

#[test]
fn generated_f3d_rewrites_prism_scalar_properties() {
    let source = f3d_with_smbh_and_instance_properties(
        &synthetic_geometry_smbh(),
        &[
            generated_prism_instance_properties(
                "PrismOpaqueSchema",
                "11111111-2222-3333-4444-555555555555",
            ),
            generated_prism_instance_properties(
                "PrismTransparentSchema",
                "99999999-2222-3333-4444-555555555555",
            ),
        ],
    );
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated Prism decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let opaque = edited
        .model
        .appearances
        .iter_mut()
        .find(|appearance| appearance.schema.as_deref() == Some("PrismOpaqueSchema"))
        .expect("opaque appearance");
    opaque.properties.insert("surface_roughness".into(), 0.75);
    let transparent = edited
        .model
        .appearances
        .iter_mut()
        .find(|appearance| appearance.schema.as_deref() == Some("PrismTransparentSchema"))
        .expect("transparent appearance");
    transparent
        .properties
        .insert("refraction_index".into(), 2.25);

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("Prism scalar regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated Prism decode");
    assert!(round_trip.ir().model.appearances.iter().any(|appearance| {
        appearance.schema.as_deref() == Some("PrismOpaqueSchema")
            && appearance.properties.get("surface_roughness") == Some(&0.75)
    }));
    assert!(round_trip.ir().model.appearances.iter().any(|appearance| {
        appearance.schema.as_deref() == Some("PrismTransparentSchema")
            && appearance.properties.get("refraction_index") == Some(&2.25)
    }));
}

#[test]
fn generated_f3d_rewrites_body_rgb_color() {
    let source = f3d_with_smbh(&synthetic_geometry_with_body_color_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected = cadmpeg_ir::topology::Color {
        r: 0.7,
        g: 0.4,
        b: 0.2,
        a: 1.0,
    };
    edited.model.bodies[0].color = Some(expected);

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("body-color regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(round_trip.ir().model.bodies[0].color, Some(expected));
}

#[test]
fn generated_f3d_rewrites_the_winning_truecolor_attribute() {
    let source = f3d_with_smbh(&synthetic_geometry_with_body_truecolor_chain_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated truecolor F3D decode");
    assert_eq!(
        decoded.ir().model.bodies[0].color,
        Some(cadmpeg_ir::topology::Color {
            r: 32.0 / 255.0,
            g: 64.0 / 255.0,
            b: 96.0 / 255.0,
            a: 1.0,
        })
    );
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected = cadmpeg_ir::topology::Color {
        r: 64.0 / 255.0,
        g: 128.0 / 255.0,
        b: 192.0 / 255.0,
        a: 1.0,
    };
    edited.model.bodies[0].color = Some(expected);

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("truecolor regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated truecolor decode");
    assert_eq!(round_trip.ir().model.bodies[0].color, Some(expected));
}

#[test]
fn generated_f3d_rewrites_fixed_width_decimal_color_text() {
    let source = f3d_with_smbh(&synthetic_geometry_with_body_decimal_color_chain_smbh(
        "04227264",
    ));
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated decimal-color F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected = cadmpeg_ir::topology::Color {
        r: 1.0 / 255.0,
        g: 2.0 / 255.0,
        b: 3.0 / 255.0,
        a: 1.0,
    };
    edited.model.bodies[0].color = Some(expected);

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("decimal-color regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated decimal-color decode");
    assert_eq!(round_trip.ir().model.bodies[0].color, Some(expected));
}

#[test]
fn generated_f3d_rejects_lossy_truecolor_edit() {
    let source = f3d_with_smbh(&synthetic_geometry_with_body_truecolor_chain_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated truecolor F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.bodies[0].color = Some(cadmpeg_ir::topology::Color {
        r: 0.5,
        g: 64.0 / 255.0,
        b: 96.0 / 255.0,
        a: 1.0,
    });

    let error = F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut Vec::new())
        .expect_err("nonrepresentable truecolor edit must be rejected");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
}

#[test]
fn generated_f3d_rejects_decimal_color_text_growth() {
    let source = f3d_with_smbh(&synthetic_geometry_with_body_decimal_color_chain_smbh(
        "255",
    ));
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated decimal-color F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.bodies[0].color = Some(cadmpeg_ir::topology::Color {
        r: 1.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    });

    let error = F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut Vec::new())
        .expect_err("wider decimal-color text must be rejected");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
}

#[test]
fn generated_f3d_rewrites_face_rgb_color_and_sense() {
    let source = f3d_with_smbh(&synthetic_geometry_with_face_color_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected = cadmpeg_ir::topology::Color {
        r: 0.6,
        g: 0.3,
        b: 0.9,
        a: 1.0,
    };
    edited.model.faces[0].color = Some(expected);
    edited.model.faces[0].sense = cadmpeg_ir::topology::Sense::Reversed;

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("face-color regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(round_trip.ir().model.faces[0].color, Some(expected));
    assert_eq!(
        round_trip.ir().model.faces[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
}

#[test]
fn generated_f3d_rewrites_edge_parameter_range() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.edges[0].param_range = Some([-2.5, 4.75]);

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("edge-range regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        round_trip.ir().model.edges[0].param_range,
        Some([-2.5, 4.75])
    );
}

#[test]
fn generated_f3d_rewrites_edge_native_metadata() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let owner = edited.model.coedges[0].id.clone();
    {
        let mut native = f3d_native_mut(&mut edited);
        native.edge_continuities[0].continuity = "tangent".into();
        native.edge_continuities[0].sense = cadmpeg_ir::topology::Sense::Reversed;
        native.edge_ownerships[0].owner_coedge = Some(owner.clone());
    }

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("edge-continuity regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        f3d_native(round_trip.ir()).edge_continuities[0].continuity,
        "tangent"
    );
    assert_eq!(
        f3d_native(round_trip.ir()).edge_continuities[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert_eq!(
        f3d_native(round_trip.ir()).edge_ownerships[0].owner_coedge,
        Some(owner)
    );
}

#[test]
fn generated_f3d_rewrites_vertex_ownership() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let replacement = edited.model.edges[1].id.clone();
    {
        let mut native = f3d_native_mut(&mut edited);
        native.vertex_ownerships[1].owning_edge = replacement.clone();
        native.vertex_ownerships[1].endpoint_index = 0;
    }

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("vertex-ownership regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    let ownership = &f3d_native(round_trip.ir()).vertex_ownerships[1];
    assert_eq!(ownership.owning_edge, replacement);
    assert_eq!(ownership.endpoint_index, 0);
}

#[test]
fn generated_f3d_rewrites_face_and_coedge_sense() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.faces[0].sense = cadmpeg_ir::topology::Sense::Reversed;
    edited.model.coedges[0].sense = cadmpeg_ir::topology::Sense::Reversed;

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("orientation regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        round_trip.ir().model.faces[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert_eq!(
        round_trip.ir().model.coedges[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
}
