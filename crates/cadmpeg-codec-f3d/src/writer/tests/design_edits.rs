// SPDX-License-Identifier: Apache-2.0
//! Writer-domain synthetic tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::F3dCodec;

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
    assert!(recipe.design_id.as_ref().and_then(|field| field.offset).is_some());
    recipe.record_index = 777;
    recipe.design_id.as_mut().expect("recipe id").value = "333".into();
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
        .find(|design_type| design_type.entities.values().copied().eq([33, 44]))
        .expect("generated relation design type");
    assert!(object.byte_offset < object.version_offset);
    let crate::records::ReferenceRun::Located(entities) = &object.entities else {
        panic!("parsed entity locations");
    };
    assert_eq!(entities.len(), 2);
    object.type_guid = "91111111-2222-3333-4444-555555555555".into();
    object.base_type_guid.as_mut().expect("base GUID").value = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeef".into();
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
    act_root.registry_flag = crate::records::ActRegistryFlag::Off;
    act_root.entity_id = "1_3".into();
    act_root.display_name = "(Renamed)".into();
    let act_entity = &mut native.act_entities[0];
    assert!(act_entity.table_entity_id_offset().is_some());
    assert!(act_entity.channel_entity_id_offset().is_some());
    act_entity.channel_group_mut().unwrap().channels.insert(
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
    assignment.physical_token.as_mut().expect("material field").value = "PrismMaterial-019".into();
    assignment.visual_preset.as_mut().expect("material field").value = "Prism-002".into();
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
    native
        .store(
            edited
                .native
                .namespace_mut("f3d", std::num::NonZeroU32::MIN),
        )
        .unwrap();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
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
            .design_id.as_ref().map(|field| field.value.as_str()),
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
        .find(|design_type| design_type.entities.values().copied().eq([33, 44]))
        .cloned()
        .expect("round-trip relation design type");
    assert_eq!(object.entities.values().copied().collect::<Vec<_>>(), [33, 44]);
    assert_eq!(object.type_guid, "91111111-2222-3333-4444-555555555555");
    assert_eq!(
        object.base_type_guid.as_ref().map(|field| field.value.as_str()),
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
    assert_eq!(act_root.registry_flag, crate::records::ActRegistryFlag::Off);
    assert_eq!(act_root.entity_id, "1_3");
    assert_eq!(act_root.display_name, "(Renamed)");
    assert_eq!(
        f3d_native(round_trip.ir()).act_registry_channels[0].guid,
        "dddddddd-1111-2222-3333-eeeeeeeeeeee"
    );
    let act_entity = &f3d_native(round_trip.ir()).act_entities[0];
    assert_eq!(act_entity.entity_id, "0_985");
    assert_eq!(
        act_entity.channels().get("Appearance").map(String::as_str),
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
            .as_ref().map(|field| field.value.as_str()),
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
