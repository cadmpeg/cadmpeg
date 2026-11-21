// SPDX-License-Identifier: Apache-2.0
//! Texts cut to their field's width, and material records the writer skips.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_ir::appearance::Appearance;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::ids::AppearanceId;
use cadmpeg_ir::topology::Color;

use crate::writer::materials::{materials_payload, MATERIAL_NAME};
use crate::SldprtCodec;

#[test]
fn every_capacity_field_fits_its_u8_count() {
    for field in crate::writer::CAPACITY_FIELDS {
        let stored = if field.field == "unit name" {
            field.width * 2
        } else {
            field.width
        };
        assert!(u8::try_from(stored).is_ok(), "{field:?}");
    }
}

fn source_less_cube() -> cadmpeg_ir::CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    ir.geometry
        .faces
        .iter_mut()
        .for_each(|face| face.name = None);
    ir.geometry
        .edges
        .iter_mut()
        .for_each(|edge| edge.set_param_range(None));
    ir
}

fn steel() -> Color {
    Color::new(32.0 / 255.0, 64.0 / 255.0, 128.0 / 255.0, 1.0).unwrap()
}

fn material_appearance(id: &str, name: &str, color: Option<Color>) -> Appearance {
    Appearance {
        id: AppearanceId::mint(id).unwrap(),
        name: Some(name.into()),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: Some("moVisualProperties_c".into()),
        category: None,
        base_color: color,
        properties: BTreeMap::new(),
        textures: Vec::new(),
        native_ref: None,
    }
}

#[test]
fn an_overlong_material_name_is_cut_before_a_surrogate_pair() {
    let mut ir = source_less_cube();
    // 254 units, then a pair that would end at unit 256.
    let stem = "M".repeat(254);
    ir.geometry.bodies[0].name = Some(format!("{stem}\u{1f600}"));
    ir.geometry.bodies[0].color = Some(steel());
    let body = ir.geometry.bodies[0].id.as_str().to_owned();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let cut = plan
        .report()
        .losses
        .iter()
        .find(|loss| loss.code.local_code() == "value.unwritable")
        .unwrap();
    assert!(
        cut.message.contains(&format!(
            "{body} material name: 256 UTF-16 code units cut to 254; the field holds 255"
        )),
        "{}",
        cut.message
    );
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert!(decoded
        .ir()
        .model
        .appearances
        .iter()
        .any(|appearance| appearance.name.as_deref() == Some(stem.as_str())));
}

#[test]
fn a_material_without_colour_and_a_second_body_material_are_named() {
    let mut ir = source_less_cube();
    ir.geometry.bodies[0].color = Some(steel());
    ir.model.appearances.push(material_appearance(
        "synthetic:test:appearance#plain",
        "Plain",
        None,
    ));
    ir.model.appearances.push(material_appearance(
        "synthetic:test:appearance#brass",
        "Brass",
        Some(steel()),
    ));
    let payload = materials_payload(&ir);
    assert!(payload.cut.is_empty());
    let messages = payload
        .omitted
        .iter()
        .map(|note| {
            assert_eq!(
                note.code,
                crate::loss::SldprtLossCode::WriterMaterialsOmitted.kind()
            );
            note.message.clone()
        })
        .collect::<Vec<_>>();
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("state no base colour"));
    assert!(messages[0].contains("synthetic:test:appearance#plain"));
    // The body material and Brass are written; Plain is not.
    assert_eq!(
        payload
            .bytes
            .windows(b"moVisualProperties_c".len())
            .filter(|window| *window == b"moVisualProperties_c")
            .count(),
        2
    );
    assert!(MATERIAL_NAME.fit("Brass").unwritable_entry("x").is_none());
}

#[test]
fn a_second_distinct_body_colour_is_named_and_the_first_is_the_part_material() {
    let mut ir = source_less_cube();
    let first = ir.geometry.bodies[0].clone();
    let mut second = first.clone();
    second.id = cadmpeg_ir::ids::BodyId::mint("synthetic:test:body#second").unwrap();
    second.color = Some(Color::new(1.0, 0.0, 0.0, 1.0).unwrap());
    ir.geometry.bodies[0].color = Some(steel());
    ir.geometry.bodies.push(second);
    let materials = crate::writer::materials::swobjects_materials(&ir);
    assert_eq!(materials.records.len(), 1);
    assert_eq!(materials.records[0].source, first.id.as_str());
    assert_eq!(
        materials.skipped,
        [(
            "synthetic:test:body#second",
            crate::writer::materials::MaterialSkip::SecondBodyMaterial
        )]
    );
}

#[test]
fn an_overlong_generated_unit_name_is_cut() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    let id = "sldprt:metadata:source_linear_unit_name#0:0";
    ir.model
        .attributes
        .push(cadmpeg_ir::attributes::SourceAttribute {
            id: cadmpeg_ir::ids::AttributeId::mint(id).unwrap(),
            target: cadmpeg_ir::attributes::AttributeTarget::Document.into(),
            name: "source_linear_unit_name".into(),
            values: vec![Some(cadmpeg_ir::attributes::AttributeValue::String(
                "u".repeat(130),
            ))],
            native_ref: None,
        });
    let mut cut = Vec::new();
    let (objects, _) = crate::writer::metadata_payloads(
        &ir,
        0.001,
        false,
        &mut crate::writer::UnwrittenAttributes::default(),
        &mut cut,
    );
    assert_eq!(
        cut,
        [format!(
            "{id} unit name: 130 UTF-16 code units cut to 127; the field holds 127"
        )]
    );
    let marker = objects
        .windows(b"moLengthUserUnits_c".len())
        .position(|window| window == b"moLengthUserUnits_c")
        .unwrap()
        + b"moLengthUserUnits_c".len();
    assert_eq!(objects[marker + 3], 254);
}
