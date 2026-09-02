// SPDX-License-Identifier: Apache-2.0
//! Materials-module unit tests and appearance suites.
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

use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::write::TargetRequest;
use std::io::{Cursor, Write};

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use zip::CompressionMethod;

use crate::bytes::lp_utf16_bytes;
use crate::loss::F3dLossCode;
use crate::test_support::*;
use crate::F3dCodec;

use super::{
    merge_definition_catalog_record, DefinitionCatalogRecord, RECORD_MARKER, STREAM_HEADER_LEN,
};

fn raw_body_map_pair(
    asm_key_offset: usize,
    entity_suffix: u64,
) -> crate::design::decode::body::BodyBinding {
    crate::design::decode::body::BodyBinding {
        blob_name: "BREP.synthetic.smbh".into(),
        blob_name_offset: asm_key_offset + 32,
        pair_count: 2,
        pair_ordinal: 0,
        asm_key: 7,
        asm_key_offset,
        entity_suffix,
        entity_suffix_offset: asm_key_offset + 8,
    }
}

fn resolved_body_binding(
    stream: &str,
    asm_key_offset: u64,
    entity_suffix: u64,
    blob_name: &str,
    body: &str,
) -> crate::records::DesignBodyBinding {
    crate::records::DesignBodyBinding {
        id: crate::ids::native_design_body_binding_id(stream, asm_key_offset),
        stream: stream.into(),
        pair_count: 1,
        pair_ordinal: 0,
        asm_body_key: 7,
        asm_body_key_offset: asm_key_offset,
        entity_suffix,
        entity_suffix_offset: asm_key_offset + 8,
        blob_name: blob_name.into(),
        blob_name_offset: asm_key_offset + 32,
        body: Some(cadmpeg_ir::ids::BodyId(body.into())),
    }
}

#[test]
fn definition_catalog_uses_page_boundaries_when_payload_contains_a_start_marker() {
    fn lp(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }

    let category: String = std::iter::repeat_n('x', 0x1_0080).collect();
    let mut logical = RECORD_MARKER.to_vec();
    lp(&mut logical, "GenericSchema");
    logical.push(0);
    lp(&mut logical, "Prism-001");
    lp(&mut logical, "Prism-001");
    logical.extend_from_slice(&2_u32.to_le_bytes());
    lp(&mut logical, &category);
    lp(&mut logical, "Default");
    lp(&mut logical, "Generated appearance");
    logical.extend_from_slice(&0_u32.to_le_bytes());
    logical.extend_from_slice(&1_u32.to_le_bytes());
    lp(&mut logical, "");

    let paged = super::page_logical(&logical).expect("page catalog record");
    let frames = cadmpeg_protein::record_frames(&paged).expect("frame catalog pages");
    let [frame] = frames.as_slice() else {
        panic!("marker-shaped length prefix must remain inside one logical record")
    };
    let decoded = super::decode_definition_catalog_record(&frame.bytes)
        .expect("decode framed definition record");
    assert_eq!(decoded.schema, "GenericSchema");
    assert_eq!(decoded.asset_id, "Prism-001");
    assert_eq!(decoded.category.as_deref(), Some(category.as_str()));
}

#[test]
fn definition_catalog_version_one_omits_category() {
    fn lp(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }

    let mut logical = RECORD_MARKER.to_vec();
    lp(&mut logical, "PrismOpaqueSchema");
    logical.push(1);
    lp(&mut logical, "Opaque(246,246,243)");
    lp(&mut logical, "EFD2D83C-576F-3A9B-8535-31523D8D8432");
    logical.extend_from_slice(&1_u32.to_le_bytes());
    lp(&mut logical, "Default");
    lp(&mut logical, "Prism opaque material.");
    logical.extend_from_slice(&2_u32.to_le_bytes());
    lp(&mut logical, "materials");
    lp(&mut logical, "opaque");
    logical.extend_from_slice(&0_u32.to_le_bytes());

    let decoded = super::decode_definition_catalog_record(&logical)
        .expect("decode version-one definition record");
    assert_eq!(decoded.schema, "PrismOpaqueSchema");
    assert_eq!(decoded.asset_id, "Opaque(246,246,243)");
    assert_eq!(decoded.category, None);
    assert_eq!(decoded.group.as_deref(), Some("Default"));
    assert_eq!(decoded.tags, ["materials", "opaque"]);
}

#[test]
fn definition_catalog_version_zero_omits_category_and_group() {
    fn lp(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }

    let mut logical = RECORD_MARKER.to_vec();
    lp(&mut logical, "UnifiedBitmapSchema");
    logical.push(0);
    lp(&mut logical, "Metal-045_metal_pattern_shader");
    lp(&mut logical, "Metal-045_metal_pattern_shader");
    logical.extend_from_slice(&0_u32.to_le_bytes());
    lp(&mut logical, "Unified Bitmap.");
    logical.extend_from_slice(&2_u32.to_le_bytes());
    lp(&mut logical, "maps");
    lp(&mut logical, "misc");
    logical.extend_from_slice(&1_u32.to_le_bytes());
    lp(&mut logical, "Maps/UnifiedBitmap/UnifiedBitmap.png");

    let decoded = super::decode_definition_catalog_record(&logical)
        .expect("decode version-zero definition record");
    assert_eq!(decoded.category, None);
    assert_eq!(decoded.group, None);
    assert_eq!(decoded.description, "Unified Bitmap.");
}

#[test]
fn definition_catalog_version_three_adds_subgroup() {
    fn lp(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }

    let mut logical = RECORD_MARKER.to_vec();
    lp(&mut logical, "GenericSchema");
    logical.push(0);
    lp(&mut logical, "InvGen-063");
    lp(&mut logical, "InvGen-063");
    logical.extend_from_slice(&3_u32.to_le_bytes());
    for value in ["Metal", "Default", "Miscellaneous", "Generic material."] {
        lp(&mut logical, value);
    }
    logical.extend_from_slice(&0_u32.to_le_bytes());
    logical.extend_from_slice(&0_u32.to_le_bytes());

    let decoded = super::decode_definition_catalog_record(&logical)
        .expect("decode version-three definition record");
    assert_eq!(decoded.category.as_deref(), Some("Metal"));
    assert_eq!(decoded.group.as_deref(), Some("Default"));
    assert_eq!(decoded.subgroup.as_deref(), Some("Miscellaneous"));
    assert_eq!(decoded.description, "Generic material.");
}

#[test]
fn definition_catalog_uses_asset_and_schema_identity() {
    fn definition(asset: &str, category: &str) -> DefinitionCatalogRecord {
        DefinitionCatalogRecord {
            schema: "PrismMetalSchema".into(),
            asset_id: asset.into(),
            base_asset_id: asset.into(),
            category: Some(category.into()),
            group: Some("Default".into()),
            subgroup: None,
            description: "Steel - satin".into(),
            tags: vec!["Metal".into(), "Steel".into()],
            preview_paths: vec!["Mats/PrismMetal/Presets/t_Prism-256.png".into()],
        }
    }

    let mut definitions = std::collections::HashMap::new();
    merge_definition_catalog_record(&mut definitions, definition("Prism-256", "Metal/Steel"));
    merge_definition_catalog_record(&mut definitions, definition("Prism-256", "Metal/Steel"));
    assert_eq!(definitions.len(), 1);

    let mut alternate_description = definition("Prism-256", "Metal/Steel");
    alternate_description.description = "CCAF1000-E7D9-2CF1-9BA1-B9224CFEBAF6".into();
    merge_definition_catalog_record(&mut definitions, alternate_description);

    merge_definition_catalog_record(&mut definitions, definition("Prism-256", "Metal/Stainless"));
    let key = ("Prism-256".to_owned(), "PrismMetalSchema".to_owned());
    assert_eq!(definitions[&key].category, None);

    let mut second_schema = definition("Prism-256", "Metal/Steel");
    second_schema.schema = "GenericSchema".into();
    merge_definition_catalog_record(&mut definitions, second_schema);
    assert_eq!(definitions.len(), 2);

    let mut alternate_base = definition("Prism-256", "Metal/Steel");
    alternate_base.base_asset_id = "another-base".into();
    merge_definition_catalog_record(&mut definitions, alternate_base);
    assert_eq!(definitions.len(), 2);
}

#[test]
fn material_owner_rejects_more_than_one_pair_for_its_entity_suffix() {
    let body_map = [raw_body_map_pair(25, 100), raw_body_map_pair(41, 100)];
    let Err(error) = super::unique_body_map_pair(&body_map, 100, "material assignment") else {
        panic!("one Design entity must not select two map pairs")
    };
    assert!(error
        .to_string()
        .contains("matches multiple body-map pairs"));
}

#[test]
fn equal_keys_in_different_brep_namespaces_resolve_by_exact_map_pair() {
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let first = resolved_body_binding(
        stream,
        25,
        100,
        "BREP.first.smbh",
        "f3d:brep/first/brep:entity#1",
    );
    let second_body = cadmpeg_ir::ids::BodyId("f3d:brep/second/brep:entity#1".into());
    let second = resolved_body_binding(stream, 125, 200, "BREP.second.smbh", &second_body.0);
    let owner = crate::ids::native_scoped_id(stream, "material-assignment", 500);
    let visual_guid = "11111111-2222-3333-4444-555555555555";
    let appearance = cadmpeg_ir::appearance::Appearance {
        id: cadmpeg_ir::ids::AppearanceId("f3d:appearance#second".into()),
        name: None,
        asset_guid: Some(visual_guid.into()),
        library_id: None,
        visual_guid: Some(visual_guid.into()),
        physical_token: None,
        schema: None,
        category: None,
        base_color: None,
        properties: std::collections::BTreeMap::new(),
        textures: Vec::new(),
    };
    let assignment = crate::records::DesignMaterialAssignment {
        id: owner,
        asm_body_key: 7,
        asm_body_key_offset: 125,
        entity_suffix: 200,
        entity_suffix_offset: 133,
        entity_id: "0_200".into(),
        entity_id_offset: 500,
        visual_guid: visual_guid.into(),
        visual_guid_offset: 600,
        physical_token: None,
        physical_token_offset: None,
        visual_preset: None,
        visual_preset_offset: None,
    };
    let projected = super::bind_bodies(
        &[appearance],
        &[assignment],
        &std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
        &[first, second],
    )
    .expect("blob-qualified material binding");
    let [binding] = projected.as_slice() else {
        panic!("one appearance binding expected")
    };
    assert_eq!(
        binding.target,
        cadmpeg_ir::appearance::AppearanceTarget::Body(second_body)
    );
}

#[test]
fn presetless_assignment_matches_only_its_visual_guid() {
    let appearance_guid = "11111111-2222-3333-4444-555555555555";
    let mut appearance = cadmpeg_ir::appearance::Appearance {
        id: cadmpeg_ir::ids::AppearanceId("f3d:appearance#catalog".into()),
        name: None,
        asset_guid: Some(appearance_guid.into()),
        library_id: None,
        visual_guid: Some(appearance_guid.into()),
        physical_token: None,
        schema: None,
        category: None,
        base_color: None,
        properties: std::collections::BTreeMap::new(),
        textures: Vec::new(),
    };
    let mut assignment = crate::records::DesignMaterialAssignment {
        id: "f3d:design:material-assignment#1".into(),
        asm_body_key: 7,
        asm_body_key_offset: 25,
        entity_suffix: 100,
        entity_suffix_offset: 33,
        entity_id: "0_100".into(),
        entity_id_offset: 500,
        visual_guid: "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE".into(),
        visual_guid_offset: 600,
        physical_token: None,
        physical_token_offset: None,
        visual_preset: None,
        visual_preset_offset: None,
    };

    assert!(
        super::appearance_for_assignment(std::slice::from_ref(&appearance), &assignment)
            .expect("valid preset-less assignment")
            .is_none()
    );

    assignment.visual_guid = appearance_guid.into();
    assert!(
        super::appearance_for_assignment(std::slice::from_ref(&appearance), &assignment)
            .expect("exact visual-token assignment")
            .is_some()
    );

    assignment.visual_guid = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE".into();
    assignment.visual_preset = Some("Prism-017".into());
    appearance.name = Some("Prism-017".into());
    assert!(
        super::appearance_for_assignment(std::slice::from_ref(&appearance), &assignment)
            .expect("present preset-name fallback")
            .is_some()
    );
}

#[test]
fn complete_visual_token_selects_one_revision_record() {
    let base_token = "11111111-2222-3333-4444-555555555555";
    let revised_token = "11111111-2222-3333-4444-555555555555_Post2015";
    let appearance = |id: &str, token: &str| cadmpeg_ir::appearance::Appearance {
        id: cadmpeg_ir::ids::AppearanceId(id.into()),
        name: None,
        asset_guid: Some(token.into()),
        library_id: None,
        visual_guid: Some(token.into()),
        physical_token: None,
        schema: None,
        category: None,
        base_color: None,
        properties: std::collections::BTreeMap::new(),
        textures: Vec::new(),
    };
    let appearances = [
        appearance("f3d:appearance#base", base_token),
        appearance("f3d:appearance#revised", revised_token),
    ];

    let selected = super::appearance_for_visual_token(&appearances, revised_token, None)
        .expect("unique complete visual token")
        .expect("revised appearance exists");
    assert_eq!(selected.id.as_str(), "f3d:appearance#revised");

    let duplicates = [
        appearance("f3d:appearance#first", revised_token),
        appearance("f3d:appearance#second", revised_token),
    ];
    assert!(matches!(
        super::appearance_for_visual_token(&duplicates, revised_token, None),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn visual_preset_fallback_requires_one_record() {
    let appearance = |id: &str| cadmpeg_ir::appearance::Appearance {
        id: cadmpeg_ir::ids::AppearanceId(id.into()),
        name: Some("Prism-017".into()),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: None,
        properties: std::collections::BTreeMap::new(),
        textures: Vec::new(),
    };
    let appearances = [
        appearance("f3d:appearance#first"),
        appearance("f3d:appearance#second"),
    ];

    assert!(matches!(
        super::appearance_for_visual_token(
            &appearances,
            "11111111-2222-3333-4444-555555555555",
            Some("Prism-017"),
        ),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn generic_connection_delta_rejects_unknown_and_truncated_forms() {
    let mut record = vec![0; 120];
    record[102] = 2;
    assert_eq!(super::generic_connection_delta(&record, 0), None);

    record[102] = 1;
    record[104..108].copy_from_slice(&1u32.to_le_bytes());
    record[108..112].copy_from_slice(&16u32.to_le_bytes());
    assert_eq!(super::generic_connection_delta(&record, 0), None);
}

fn distance_record(unit: u32, value: f64) -> cadmpeg_protein::DecodedRecord {
    cadmpeg_protein::DecodedRecord {
        ordinal: 0,
        logical_offset: 0,
        schema: "TestSchema".into(),
        guid: String::new(),
        base: String::new(),
        asset_lib_id: String::new(),
        properties: std::collections::BTreeMap::from([(
            "test_Depth".to_owned(),
            cadmpeg_protein::DecodedProperty {
                value_offset: 0,
                value: cadmpeg_protein::PropertyValue::Distance { unit, value },
                connections: Vec::new(),
            },
        )]),
    }
}

#[test]
fn decoded_color_requires_finite_normalized_channels() {
    assert!(super::decoded_color([0.0, 0.25, 0.5, 1.0]).is_some());
    for invalid in [f64::NAN, f64::INFINITY, -0.01, 1.01] {
        assert!(super::decoded_color([invalid, 0.25, 0.5, 1.0]).is_none());
    }
}

/// The three length tags of the Distance quantity class each convert to
/// the IR's millimetres. `0x200e` is millimetre, not centimetre.
#[test]
fn distance_tags_convert_to_millimetres() {
    for (unit, value, expected) in [(0x2016, 1.0, 25.4), (0x200e, 0.5, 0.5), (0x200d, 0.5, 5.0)] {
        let record = distance_record(unit, value);
        assert_eq!(
            super::distance_property(&record, "Depth"),
            Ok(Some(expected))
        );
    }
}

#[test]
fn schema_primary_colour_wins_over_rival_colour_members() {
    for (schema, primary_id) in [
        ("GenericSchema", "generic_diffuse"),
        ("MetalSchema", "metal_color"),
        ("MetallicPaintSchema", "metallicpaint_base_color"),
        ("PlasticVinylSchema", "plasticvinyl_color"),
        ("PrismLayeredSchema", "layered_diffuse"),
        ("PrismMetalSchema", "metal_f0"),
        ("PrismOpaqueSchema", "opaque_albedo"),
        ("PrismTransparentSchema", "transparent_color"),
        ("PrismWoodSchema", "surface_albedo"),
    ] {
        let mut properties = std::collections::BTreeMap::from([
            color_property("common_Tint_color", [0.75, 0.75, 0.75, 1.0]),
            color_property("surface_albedo", [0.5, 0.5, 0.5, 1.0]),
            (
                "common_Tint_toggle".to_owned(),
                cadmpeg_protein::DecodedProperty {
                    value_offset: 0,
                    value: cadmpeg_protein::PropertyValue::Boolean(false),
                    connections: Vec::new(),
                },
            ),
        ]);
        properties.insert(
            primary_id.to_owned(),
            cadmpeg_protein::DecodedProperty {
                value_offset: 0,
                value: cadmpeg_protein::PropertyValue::Color([0.125, 0.25, 0.375, 1.0]),
                connections: Vec::new(),
            },
        );
        let record = appearance_record(schema, properties);
        assert_eq!(
            super::appearance_base_color(&record).map(|color| color.g),
            Some(0.25),
            "{schema} selects {primary_id}"
        );
    }
}

#[test]
fn enabled_common_tint_replaces_the_schema_primary_colour() {
    let mut properties = std::collections::BTreeMap::from([
        color_property("opaque_albedo", [0.125, 0.25, 0.375, 1.0]),
        color_property("surface_albedo", [0.5, 0.5, 0.5, 1.0]),
        color_property("common_Tint_color", [0.75, 0.625, 0.5, 1.0]),
    ]);
    properties.insert(
        "common_Tint_toggle".to_owned(),
        cadmpeg_protein::DecodedProperty {
            value_offset: 0,
            value: cadmpeg_protein::PropertyValue::Boolean(true),
            connections: Vec::new(),
        },
    );
    let record = appearance_record("PrismOpaqueSchema", properties);
    assert_eq!(
        super::appearance_base_color(&record).map(|color| color.g),
        Some(0.625)
    );
}

fn color_property(id: &str, color: [f64; 4]) -> (String, cadmpeg_protein::DecodedProperty) {
    (
        id.to_owned(),
        cadmpeg_protein::DecodedProperty {
            value_offset: 0,
            value: cadmpeg_protein::PropertyValue::Color(color),
            connections: Vec::new(),
        },
    )
}

fn appearance_record(
    schema: &str,
    properties: std::collections::BTreeMap<String, cadmpeg_protein::DecodedProperty>,
) -> cadmpeg_protein::DecodedRecord {
    cadmpeg_protein::DecodedRecord {
        ordinal: 0,
        logical_offset: 0,
        schema: schema.to_owned(),
        guid: "11111111-2222-3333-4444-555555555555".to_owned(),
        base: "Prism-001".to_owned(),
        asset_lib_id: String::new(),
        properties,
    }
}

fn texture_record(guid: &str, path: &str) -> cadmpeg_protein::DecodedRecord {
    cadmpeg_protein::DecodedRecord {
        ordinal: 0,
        logical_offset: 0,
        schema: "UnifiedBitmapSchema".to_owned(),
        guid: guid.to_owned(),
        base: "Texture-001".to_owned(),
        asset_lib_id: String::new(),
        properties: std::collections::BTreeMap::from([(
            "unifiedbitmap_Bitmap".to_owned(),
            cadmpeg_protein::DecodedProperty {
                value_offset: 0,
                value: cadmpeg_protein::PropertyValue::TextureUri(vec![path.to_owned()]),
                connections: Vec::new(),
            },
        )]),
    }
}

fn appearance_connected_to(texture_guid: &str) -> cadmpeg_protein::DecodedRecord {
    appearance_record(
        "GenericSchema",
        std::collections::BTreeMap::from([(
            "generic_diffuse".to_owned(),
            cadmpeg_protein::DecodedProperty {
                value_offset: 0,
                value: cadmpeg_protein::PropertyValue::Color([0.25, 0.5, 0.75, 1.0]),
                connections: vec![texture_guid.to_owned()],
            },
        )]),
    )
}

#[test]
fn equivalent_duplicate_texture_guids_bind_once() {
    let guid = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
    let texture = texture_record(guid, "textures/albedo.png");
    let (appearances, untyped_count) = super::appearances_from_schema_records(&[
        appearance_connected_to(guid),
        texture.clone(),
        texture,
    ])
    .expect("equivalent texture records deduplicate");

    assert_eq!(untyped_count, 0);
    assert_eq!(appearances.len(), 1);
    assert_eq!(appearances[0].textures.len(), 1);
    assert_eq!(appearances[0].textures[0].paths, ["textures/albedo.png"]);
}

#[test]
fn conflicting_duplicate_texture_guids_reject_in_both_orders() {
    let guid = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
    let first = texture_record(guid, "textures/first.png");
    let second = texture_record(guid, "textures/second.png");
    for textures in [[first.clone(), second.clone()], [second, first]] {
        let error = super::appearances_from_schema_records(&[
            appearance_connected_to(guid),
            textures[0].clone(),
            textures[1].clone(),
        ])
        .expect_err("one texture GUID cannot select conflicting payloads");

        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    }
}

/// A Distance whose tag names a quantity other than length has no
/// millimetre reading and must not be silently taken as one.
#[test]
fn a_non_length_distance_tag_yields_no_value() {
    let record = distance_record(0x0002_1008, 1.0);
    assert_eq!(super::distance_property(&record, "Depth"), Err(0x0002_1008));
}

#[test]
fn unknown_texture_distance_unit_omits_typed_texture_and_counts_loss() {
    let guid = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
    let mut texture = texture_record(guid, "textures/albedo.png");
    texture.properties.insert(
        "unifiedbitmap_RealWorldScaleX".into(),
        cadmpeg_protein::DecodedProperty {
            value_offset: 0,
            value: cadmpeg_protein::PropertyValue::Distance {
                unit: 0x0002_1008,
                value: 3.0,
            },
            connections: Vec::new(),
        },
    );

    let (appearances, untyped_count) =
        super::appearances_from_schema_records(&[appearance_connected_to(guid), texture])
            .expect("unknown unit is retained as a typed-projection loss");

    assert_eq!(untyped_count, 1);
    assert!(appearances[0].textures.is_empty());
}

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
fn decode_rejects_invalid_instance_property_page_framing() {
    let mut properties = generated_instance_properties_for("11111111-2222-3333-4444-555555555555");
    properties[STREAM_HEADER_LEN + 4] ^= 1;
    let f3d = f3d_with_smbh_and_instance_properties(&synthetic_geometry_smbh(), &[properties]);

    let error = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .expect_err("invalid Protein page framing must reject material decode");

    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))
    ));
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
        .plan(EncodeInput::new(&source, None), TargetRequest::Inherit)
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

    assert!(result.report().geometry_transferred());
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
fn browser_body_appearance_scan_rejects_binary_utf16_length_candidates() {
    let mut bytes = vec![0u8; 8];
    for _ in 0..32 {
        bytes.extend_from_slice(&256u32.to_le_bytes());
        bytes.extend(std::iter::repeat_n(0, 256 * 2));
    }

    assert!(super::lp_utf16_strings(&bytes).is_empty());
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

/// A report carrying the unconditional appearance loss that
/// `build_container_report` and `build_geometry_report` state before appearance
/// decoding runs.
fn appearance_loss_report() -> cadmpeg_ir::codec::DecodeBody {
    cadmpeg_ir::codec::DecodeBody {
        geometry_transferred: false,
        coverage: std::collections::BTreeMap::new(),
        losses: vec![F3dLossCode::MaterialNotTransferred.note(
            "Materials/appearances (.protein assets, ACT/design assignments) were not \
             transferred.",
        )],
        notes: Vec::new(),
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
    }
}

fn opaque_appearance(guid: &str) -> cadmpeg_ir::appearance::Appearance {
    cadmpeg_ir::appearance::Appearance {
        id: cadmpeg_ir::ids::AppearanceId(format!("f3d:design:appearance#{guid}")),
        name: Some("Prism-Opaque".to_owned()),
        asset_guid: Some(guid.to_owned()),
        library_id: None,
        visual_guid: Some(guid.to_owned()),
        physical_token: None,
        schema: Some("PrismOpaqueSchema".to_owned()),
        category: None,
        base_color: Some(cadmpeg_ir::topology::Color {
            r: 0.5,
            g: 0.5,
            b: 0.5,
            a: 1.0,
        }),
        properties: std::collections::BTreeMap::new(),
        textures: Vec::new(),
    }
}

fn material_losses(report: &cadmpeg_ir::codec::DecodeBody) -> Vec<&str> {
    report
        .losses
        .iter()
        .filter(|loss| loss.code.category() == cadmpeg_ir::report::LossCategory::Material)
        .map(|loss| loss.message.as_str())
        .collect()
}

#[test]
fn appearance_loss_stands_when_no_asset_decodes() {
    let ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut report = appearance_loss_report();
    crate::decode::reconcile_appearance_loss(&mut report, &ir, false);
    assert_eq!(material_losses(&report).len(), 1);
}

#[test]
fn appearance_loss_clears_when_an_unassigned_catalog_transfers() {
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.appearances = vec![opaque_appearance("2F0E19C1-0000-4000-8000-000000000001")];
    let mut report = appearance_loss_report();
    crate::decode::reconcile_appearance_loss(&mut report, &ir, false);
    assert!(material_losses(&report).is_empty());
}

#[test]
fn appearance_loss_counts_assets_whose_assignment_is_unresolved() {
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.appearances = vec![
        opaque_appearance("2F0E19C1-0000-4000-8000-000000000001"),
        opaque_appearance("2F0E19C1-0000-4000-8000-000000000002"),
    ];
    let mut report = appearance_loss_report();
    crate::decode::reconcile_appearance_loss(&mut report, &ir, true);
    let messages = material_losses(&report);
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("2 Protein appearance asset(s)"));
}

#[test]
fn appearance_loss_clears_when_an_assignment_resolves() {
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    let appearance = opaque_appearance("2F0E19C1-0000-4000-8000-000000000001");
    ir.model.appearance_bindings = vec![cadmpeg_ir::appearance::AppearanceBinding {
        id: "f3d:appearance:body#0_1:2F0E19C1-0000-4000-8000-000000000001".to_owned(),
        target: cadmpeg_ir::appearance::AppearanceTarget::Body(cadmpeg_ir::ids::BodyId(
            "f3d:brep/a.smbh/brep:entity#1".to_owned(),
        )),
        appearance: appearance.id.clone(),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::new(),
    }];
    ir.model.appearances = vec![appearance];
    let mut report = appearance_loss_report();
    crate::decode::reconcile_appearance_loss(&mut report, &ir, true);
    assert!(material_losses(&report).is_empty());
}
