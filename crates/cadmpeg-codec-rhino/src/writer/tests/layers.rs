// SPDX-License-Identifier: Apache-2.0
//! Layers and appearance colours: the archive states each layer it can hold,
//! places each object on its layer, and states an appearance as the colour of
//! the object bound to it.

use std::collections::BTreeMap;

use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{AppearanceBindingId, AppearanceId, LayerId, PointId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};
use cadmpeg_ir::references::InstanceTarget;
use cadmpeg_ir::topology::{Color, Point};
use cadmpeg_ir::NativeRecord;

use crate::loss::RhinoLossCode;
use crate::RhinoArchiveVersion;

use super::{decoded, loss_naming, native_omission_naming, write_reported};

fn point(id: &str, x: f64) -> Point {
    Point::new(
        PointId::mint(id).expect("identity"),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(x, 0.0, 0.0))
            .expect("a finite position is a point"),
        None,
    )
}

/// A document of two points, the first listed on presentation layer `Walls`.
fn layered() -> CadIr {
    let mut ir = CadIr::empty();
    ir.geometry.points.push(point("cadir:model:point#on-walls", 1.0));
    ir.geometry.points.push(point("cadir:model:point#unlisted", 2.0));
    ir.model.presentation_layers.push(PresentationLayer {
        id: LayerId::mint("cadir:model:layer#walls").expect("identity"),
        name: "Walls".into(),
        description: None,
        visible: Some(false),
        items: vec![InstanceTarget::new(PresentationItem::Point {
            point: PointId::mint("cadir:model:point#on-walls").expect("identity"),
        })],
    });
    ir
}

/// The archive decoded back to an editable document.
fn editable(bytes: Vec<u8>) -> cadmpeg_test_support::EditableDecodeResult {
    use cadmpeg_ir::codec::Codec;
    cadmpeg_test_support::EditableDecodeResult::from(
        crate::RhinoCodec
            .decode(
                &mut std::io::Cursor::new(bytes),
                &cadmpeg_ir::codec::DecodeOptions::default(),
            )
            .expect("the written archive decodes"),
    )
}

fn native_layers(ir: &CadIr) -> &[NativeRecord] {
    ir.native
        .namespace("rhino")
        .and_then(|namespace| namespace.arenas().get("layers"))
        .map_or(&[], Vec::as_slice)
}

fn text(record: &NativeRecord, field: &str) -> Option<String> {
    record
        .field(field)
        .and_then(|value| value.as_str().map(str::to_owned))
}

/// The source layer reference of the decoded point at `x`.
fn layer_of_point_at(ir: &CadIr, x: f64) -> Option<String> {
    ir.geometry
        .points
        .iter()
        .find(|point| point.position().get().x == x)
        .expect("the point is written")
        .source_object
        .as_ref()
        .and_then(|source| source.layer.clone())
}

#[test]
fn a_presentation_layer_is_a_layer_record_holding_its_objects() {
    let (bytes, losses) = write_reported(&layered(), RhinoArchiveVersion::V8);
    let written = decoded(bytes);
    let layers = native_layers(&written);
    let names = layers
        .iter()
        .map(|record| text(record, "name"))
        .collect::<Vec<_>>();
    assert_eq!(names, [Some("Default".into()), Some("Walls".into())]);
    let walls = &layers[1];
    assert_eq!(walls.field("visible"), Some(serde_json::Value::Bool(false)));
    assert_eq!(layer_of_point_at(&written, 1.0), text(walls, "source_uuid"));
    assert_eq!(layer_of_point_at(&written, 2.0), text(&layers[0], "source_uuid"));
    assert!(
        losses
            .iter()
            .all(|loss| loss.code != RhinoLossCode::WriterLayerReduced.kind()),
        "{losses:#?}"
    );
}

/// Rhino to Rhino: the decoded layers are re-emitted (Rule W1) and each
/// object stays on its layer.
#[test]
fn decoded_layers_are_re_emitted_with_their_objects() {
    let (bytes, _) = write_reported(&layered(), RhinoArchiveVersion::V8);
    let first = decoded(bytes);
    let (bytes, losses) = write_reported(&first, RhinoArchiveVersion::V8);
    assert!(
        losses
            .iter()
            .all(|loss| loss.code.local_code() != "native.records-omitted"),
        "{losses:#?}"
    );
    let second = decoded(bytes);
    let source = |ir: &CadIr| {
        native_layers(ir)
            .iter()
            .map(|record| (text(record, "name"), text(record, "source_uuid")))
            .collect::<Vec<_>>()
    };
    assert_eq!(source(&second), source(&first));
    assert_eq!(
        layer_of_point_at(&second, 1.0),
        layer_of_point_at(&first, 1.0)
    );
}

/// A layer whose reference does not resolve in the written archive is
/// dropped by id, and an object on it moves to layer 0 with a loss.
#[test]
fn a_layer_naming_an_unwritten_material_is_omitted_with_its_objects_reported() {
    let (bytes, _) = write_reported(&layered(), RhinoArchiveVersion::V8);
    let mut source = editable(bytes);
    let walls;
    {
        let mut ir = source.ir_mut();
        let records = ir
            .native
            .namespace_mut("rhino")
            .arenas_mut()
            .get_mut("layers")
            .expect("decoded layers");
        let mut fields = records[1].fields().expect("layer fields");
        walls = records[1].id().to_owned();
        fields.insert("material_index".into(), serde_json::json!(0));
        records[1] = NativeRecord::new(
            cadmpeg_ir::ids::Identity::new(walls.clone()).expect("identity"),
            fields,
        )
        .expect("native record");
    }
    let (bytes, losses) = write_reported(source.ir(), RhinoArchiveVersion::V8);
    let message = native_omission_naming(&losses, &walls);
    assert!(message.contains("`layers`"), "{message}");
    let geometry = &source.ir().geometry;
    let point = &geometry
        .points
        .iter()
        .find(|point| point.position().get().x == 1.0)
        .expect("decoded point")
        .id;
    let vertex = &geometry
        .vertices
        .iter()
        .find(|vertex| &vertex.point == point)
        .expect("the point is a free vertex")
        .id;
    let shell = geometry
        .shells
        .iter()
        .find(|shell| shell.free_vertices().any(|free| free == vertex))
        .expect("the vertex is in a shell");
    let region = geometry
        .regions
        .iter()
        .find(|region| region.id == shell.region)
        .expect("the shell is in a region");
    loss_naming(
        &losses,
        RhinoLossCode::WriterObjectAttributesReduced,
        region.body.as_str(),
    );
    let written = decoded(bytes);
    assert_eq!(native_layers(&written).len(), 1);
    assert_eq!(written.geometry.points.len(), 2);
}

#[test]
fn a_child_layer_keeps_its_parent() {
    let (bytes, _) = write_reported(&layered(), RhinoArchiveVersion::V8);
    let mut source = editable(bytes);
    let parent;
    {
        let mut ir = source.ir_mut();
        let records = ir
            .native
            .namespace_mut("rhino")
            .arenas_mut()
            .get_mut("layers")
            .expect("decoded layers");
        parent = text(&records[0], "source_uuid").expect("default layer uuid");
        let mut fields = records[1].fields().expect("layer fields");
        let id = records[1].id().to_owned();
        fields.insert("parent_uuid".into(), serde_json::json!(parent));
        fields.insert("expanded".into(), serde_json::json!(true));
        records[1] = NativeRecord::new(
            cadmpeg_ir::ids::Identity::new(id).expect("identity"),
            fields,
        )
        .expect("native record");
    }
    let (bytes, losses) = write_reported(source.ir(), RhinoArchiveVersion::V8);
    assert!(
        losses
            .iter()
            .all(|loss| loss.code.local_code() != "native.records-omitted"),
        "{losses:#?}"
    );
    let written = decoded(bytes);
    let child = &native_layers(&written)[1];
    assert_eq!(text(child, "parent_uuid"), Some(parent));
    assert_eq!(child.field("expanded"), Some(serde_json::Value::Bool(true)));
}

fn appearance(id: &str, base_color: Option<Color>) -> Appearance {
    Appearance {
        id: AppearanceId::mint(id).expect("identity"),
        name: None,
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color,
        properties: BTreeMap::new(),
        textures: Vec::new(),
    }
}

fn binding(id: &str, appearance: &str, target: AppearanceTarget) -> AppearanceBinding {
    AppearanceBinding {
        id: AppearanceBindingId::mint(id).expect("identity"),
        target: InstanceTarget::new(target),
        appearance: AppearanceId::mint(appearance).expect("identity"),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: BTreeMap::new(),
    }
}

/// STEP to Rhino: the base colour of the appearance bound to an object is
/// that object's colour; an appearance the archive cannot state otherwise is
/// named.
#[test]
fn an_appearance_bound_to_an_object_is_its_colour() {
    let red = Color::new(1.0, 0.0, 0.0, 1.0).expect("colour");
    let mut ir = CadIr::empty();
    ir.geometry.points.push(point("cadir:model:point#red", 1.0));
    ir.model
        .appearances
        .push(appearance("cadir:model:appearance#red", Some(red)));
    ir.model.appearance_bindings.push(binding(
        "cadir:model:appearance-binding#red",
        "cadir:model:appearance#red",
        AppearanceTarget::Point(PointId::mint("cadir:model:point#red").expect("identity")),
    ));
    ir.model.appearance_bindings.push(binding(
        "cadir:model:appearance-binding#absent",
        "cadir:model:appearance#red",
        AppearanceTarget::Point(PointId::mint("cadir:model:point#absent").expect("identity")),
    ));
    let (bytes, losses) = write_reported(&ir, RhinoArchiveVersion::V8);
    loss_naming(
        &losses,
        RhinoLossCode::WriterAppearanceReduced,
        "cadir:model:appearance#red",
    );
    loss_naming(
        &losses,
        RhinoLossCode::WriterAppearanceReduced,
        "cadir:model:appearance-binding#absent",
    );
    assert!(losses
        .iter()
        .all(|loss| !loss.message.contains("cadir:model:appearance-binding#red")));
    let written = decoded(bytes);
    assert_eq!(
        written.geometry.points[0]
            .source_object
            .as_ref()
            .and_then(|source| source.color),
        Some(red)
    );
}
