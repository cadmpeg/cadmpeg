// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::native::{RetainedXml, ValueRecord};
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding};
use cadmpeg_ir::presentation::{
    CameraState, PresentationDocument, PresentationState, PresentationStateKind,
};
use cadmpeg_ir::topology::{Body, Color};
use serde_json::json;

fn body() -> Body {
    serde_json::from_value(json!({"id":"authored:model:body#A","regions":[],"color":Color::from_rgba8(255,0,0,255),"visible":true})).expect("body")
}
fn appearance() -> Appearance {
    serde_json::from_value(
        json!({"id":"authored:model:appearance#Blue","base_color":Color::from_rgba8(0,0,255,255)}),
    )
    .expect("appearance")
}

#[test]
fn typed_gui_values_supply_content_instead_of_retained_xml() {
    let record = GuiPropertyRecord {
        id: "fcstd:native:gui-property#Visibility".into(),
        owner: "fcstd:native:gui-provider#A".into(),
        name: "Visibility".into(),
        type_name: "App::PropertyBool".into(),
        status: None,
        order: 0,
        values: vec![ValueRecord {
            order: 0,
            parent: None,
            tag: "Bool".into(),
            attributes: BTreeMap::from([("value".into(), "0".into())]),
            decoded_json: None,
            text: None,
            raw_xml: "<Bool value=\"contradicting-retained-blob\"/>".into(),
        }],
        side_entries: Vec::new(),
        xml: RetainedXml::from_source(
            "<Property name=\"Visibility\"><Bool value=\"1\"/></Property>",
            0,
        )
        .expect("span"),
    };
    let rendered = native_property(&record)
        .expect("typed property")
        .expect("inline");
    let document = roxmltree::Document::parse(&rendered).expect("XML");
    assert_eq!(
        document
            .descendants()
            .find(|node| node.has_tag_name("Bool"))
            .expect("value")
            .attribute("value"),
        Some("0")
    );
}

#[test]
fn explicit_appearance_binding_overrides_body_and_native_color() {
    let mut ir = CadIr::empty();
    let body = body();
    let appearance = appearance();
    let binding:AppearanceBinding=serde_json::from_value(json!({"id":"authored:model:appearance_binding#Blue","target":{"kind":"body","id":body.id},"appearance":appearance.id,"visible":false})).expect("binding");
    ir.model.bodies.push(body.clone());
    ir.model.appearances.push(appearance);
    ir.model.appearance_bindings.push(binding);
    let names = HashMap::from([(body.id.as_str().to_owned(), "Shape".into())]);
    let mut providers = BTreeMap::from([("Shape".into(), Provider::new("Shape".into()))]);
    providers.get_mut("Shape").expect("provider").set(
        "ShapeColor",
        "App::PropertyColor",
        "PropertyColor",
        0x00ff_00ff_u32,
    );
    let mut entries = BTreeMap::new();
    let mut losses = Vec::new();
    colors::write(
        &ir,
        &names,
        &names,
        &mut providers,
        &mut entries,
        &mut losses,
    )
    .expect("presentation");
    let document = roxmltree::Document::parse(&providers["Shape"].properties["ShapeColor"])
        .expect("color XML");
    assert_eq!(
        document
            .descendants()
            .find(|node| node.has_tag_name("PropertyColor"))
            .expect("color")
            .attribute("value"),
        Some("4278190335")
    );
    // D19: the red display colour and blue material colour are distinct facts.
    let material = roxmltree::Document::parse(&providers["Shape"].properties["ShapeMaterial"])
        .expect("material XML");
    assert_eq!(
        material
            .descendants()
            .find(|node| node.has_tag_name("PropertyMaterial"))
            .expect("material")
            .attribute("diffuseColor"),
        Some("65535")
    );
    assert!(providers["Shape"].properties["Visibility"].contains("value=\"0\""));
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "appearance.not-transferred");
    assert!(losses[0]
        .message
        .contains("authored:model:appearance_binding#Blue"));
    assert!(losses[0]
        .message
        .contains("independent visibility/channel gates"));
}

#[test]
fn face_colors_are_regenerated_as_complete_typed_binary_lists() {
    let mut ir = CadIr::empty();
    let body = body();
    let appearance = appearance();
    let binding:AppearanceBinding=serde_json::from_value(json!({"id":"authored:model:appearance_binding#Face","target":{"kind":"face","id":"authored:model:face#B"},"appearance":appearance.id})).expect("binding");
    ir.model.bodies.push(body.clone());
    ir.model.appearances.push(appearance);
    ir.model.appearance_bindings.push(binding);
    let names = HashMap::from([
        (body.id.as_str().to_owned(), "Shape".into()),
        ("authored:model:face#A".into(), "Shape".into()),
        ("authored:model:face#A:subelement".into(), "Face1".into()),
        ("authored:model:face#B".into(), "Shape".into()),
        ("authored:model:face#B:subelement".into(), "Face2".into()),
    ]);
    let body_names = HashMap::from([(body.id.as_str().to_owned(), "Shape".into())]);
    let mut providers = BTreeMap::new();
    let mut entries = BTreeMap::new();
    let mut losses = Vec::new();
    colors::write(
        &ir,
        &names,
        &body_names,
        &mut providers,
        &mut entries,
        &mut losses,
    )
    .expect("topology colors");
    assert_eq!(
        entries["Shape.DiffuseColor"],
        [
            2_u32.to_le_bytes(),
            0xff00_00ff_u32.to_le_bytes(),
            0x0000_ffff_u32.to_le_bytes()
        ]
        .concat()
    );
    assert!(providers["Shape"].properties["DiffuseColor"].contains("Shape.DiffuseColor"));
    assert!(losses.is_empty());
}

#[test]
fn camera_pose_overrides_settings_while_other_settings_remain_held() {
    let mut ir = CadIr::empty();
    let mut document = PresentationDocument::new(
        cadmpeg_ir::PresentationId::mint("authored:model:presentation#A").expect("id"),
    );
    let camera = CameraState {
        position: cadmpeg_ir::units::FiniteVector::new([10.0, 20.0, 30.0]),
        orientation: cadmpeg_ir::units::NonzeroVector::new([0.0, 1.0, 0.0, 2.0]),
        properties: BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("settings"),
            "PerspectiveCamera { position 1 2 3 orientation 0 0 1 0 heightAngle 0.5 }".into(),
        )]),
    };
    document
        .set_states(vec![PresentationState {
            kind: PresentationStateKind::Camera(camera),
            order: 0,
            attributes: BTreeMap::new(),
            assets: Vec::new(),
        }])
        .expect("states");
    ir.model.presentation_documents.push(document);
    let mut losses = Vec::new();
    let (_, _, state) = super::document::write(&ir, &mut losses).expect("document");
    let xml = roxmltree::Document::parse(&state).expect("state XML");
    let settings = xml.root_element().attribute("settings").expect("settings");
    assert!(settings.contains("position 10 20 30"));
    assert!(settings.contains("orientation 0 1 0 2"));
    assert!(settings.contains("heightAngle 0.5"));
    assert!(!settings.contains("position 1 2 3"));
    assert!(losses.is_empty());
}

#[test]
fn view_fields_win_over_duplicate_native_property_values_and_deletions() {
    let mut ir = CadIr::empty();
    let record = |name: &str, value: &str| GuiPropertyRecord {
        id: format!("fcstd:native:gui-property#{name}"),
        owner: "provider".into(),
        name: name.into(),
        type_name: "App::PropertyBool".into(),
        status: None,
        order: 0,
        values: vec![ValueRecord {
            order: 0,
            parent: None,
            tag: "Bool".into(),
            attributes: BTreeMap::from([("value".into(), value.into())]),
            decoded_json: None,
            text: None,
            raw_xml: "<Bool value=\"contradicting-retained-blob\"/>".into(),
        }],
        side_entries: Vec::new(),
        xml: RetainedXml::from_source("<Property/>", 0).expect("span"),
    };
    let mut provider = Provider::new("Shape".into());
    for property in [record("Visibility", "0"), record("DeletedSetting", "1")] {
        provider.properties.insert(
            property.name.clone(),
            native_property(&property).expect("typed").expect("inline"),
        );
        provider.native.insert(property.name.clone(), property);
    }
    ir.model.view_presentations.push(serde_json::from_value(json!({"id":"authored:model:presentation#View","object":"authored:model:body#A","order":0,"visible":true,"properties":{"Visibility":"0"}})).expect("view"));
    let names = HashMap::from([("authored:model:body#A".into(), "Shape".into())]);
    let mut providers = BTreeMap::from([("Shape".into(), provider)]);
    let mut losses = Vec::new();
    neutral_views(
        &ir,
        &names,
        &HashSet::from(["Shape"]),
        &mut providers,
        &mut losses,
    )
    .expect("neutral views");
    assert!(providers["Shape"].properties["Visibility"].contains("value=\"1\""));
    assert!(!providers["Shape"].properties.contains_key("DeletedSetting"));
    assert!(losses.is_empty());
}
