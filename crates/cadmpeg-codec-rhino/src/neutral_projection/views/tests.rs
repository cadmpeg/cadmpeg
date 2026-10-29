// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::ids::Identity;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::NativeRecord;
use serde_json::{Map, Value};

fn camera(fields: &Map<String, Value>) -> Option<CameraProjection> {
    fn vector(fields: &Map<String, Value>, key: &str) -> Option<[FiniteReal; 3]> {
        let values = fields.get(key)?.as_array()?;
        (values.len() == 3).then_some(())?;
        Some([
            FiniteReal::new(values[0].as_f64()?)?,
            FiniteReal::new(values[1].as_f64()?)?,
            FiniteReal::new(values[2].as_f64()?)?,
        ])
    }
    Some(CameraProjection {
        location: vector(fields, "camera_location_mm")?,
        direction: vector(fields, "camera_direction")?,
        up: vector(fields, "camera_up")?,
        x_axis: vector(fields, "camera_x_axis")?,
        y_axis: vector(fields, "camera_y_axis")?,
        z_axis: vector(fields, "camera_z_axis")?,
    })
}

fn orientation(fields: &Map<String, Value>) -> Option<NonzeroVector<4>> {
    super::orientation(&camera(fields)?)
}

fn project(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<Vec<LossNote>, CodecError> {
    let sources = ir.native.namespace("rhino")
        .and_then(|namespace| namespace.arenas().get("views"))
        .into_iter().flatten().map(|record| {
            let bool_field = |key| record.field(key).and_then(|value| value.as_bool())
                .expect("original unit input contains the declared view flag");
            ViewProjection {
                native_id: record.id().to_owned(),
                name: record.field("name").and_then(|value| value.as_str().map(str::to_owned))
                    .expect("original unit input contains the declared view name"),
                guides: crate::views::ViewGuides {
                    construction_grid: bool_field("show_construction_grid"),
                    construction_axes: bool_field("show_construction_axes"),
                    world_axes: bool_field("show_world_axes"),
                },
                legacy_display_mode: record.field("legacy_display_mode").and_then(|value| value.as_i64()),
                modern_display_mode: record.field("attributes").and_then(|value| value.as_object()
                    .and_then(|fields| fields.get("display"))
                    .and_then(|value| value.as_str().map(str::to_owned))),
                has_attributes: record.field("attributes").is_some_and(|value| value.is_object()),
                camera: record.field("viewport").and_then(|value| value.as_object().map(|fields|
                    camera(fields).expect("original unit input contains six finite three-coordinate camera vectors"))),
            }
        }).collect();
    super::project(ctx, sources, ir)
}

fn fields() -> Map<String, Value> {
    serde_json::from_value(serde_json::json!({
        "list_kind":"active","list_index":0,"name":"Perspective",
        "show_construction_grid":false,"show_construction_axes":true,"show_world_axes":true,
        "legacy_display_mode":2,
        "viewport":{"camera_valid":true,"frustum_valid":true,"port_valid":true,"projection":2,
            "camera_location_mm":[1,2,3],"camera_direction":[0,0,-1],"camera_up":[0,1,0],
            "camera_x_axis":[1,0,0],"camera_y_axis":[0,1,0],"camera_z_axis":[0,0,1],
            "frustum_mm":[-2,2,-1,1,0.1,100],"port":[0,1920,1080,0,0,1]}
    }))
    .unwrap()
}

fn install(ir: &mut CadIr, ordinal: usize, fields: Map<String, Value>) {
    ir.native
        .namespace_mut("rhino")
        .arenas_mut()
        .entry("views".into())
        .or_default()
        .push(
            NativeRecord::new(
                Identity::new(format!("rhino:document:view#active-{ordinal:04}")).unwrap(),
                fields,
            )
            .unwrap(),
        );
}

#[test]
fn admitted_source_camera_and_view_name_have_one_neutral_home() {
    let mut ir = CadIr::empty();
    install(&mut ir, 0, fields());
    let losses = project(&cadmpeg_test_support::service_decode_context(), &mut ir).unwrap();
    assert!(losses.is_empty());
    assert_eq!(ir.model.view_presentations.len(), 1);
    let view = &ir.model.view_presentations[0];
    assert_eq!(
        view.properties
            .iter()
            .find(|(key, _)| key.as_str() == "name")
            .unwrap()
            .1,
        "Perspective"
    );
    let document = &ir.model.presentation_documents[0];
    let PresentationStateKind::Camera(camera) = &document.states()[0].kind else {
        panic!("camera")
    };
    assert_eq!(camera.position.unwrap().get(), [1.0, 2.0, 3.0]);
    assert_eq!(camera.orientation.unwrap().get(), [0.0, 0.0, 1.0, 0.0]);
    let record = &ir.native.namespace("rhino").unwrap().arenas()["views"][0];
    assert_eq!(record.field("name").unwrap(), "Perspective");
    let viewport = record.field("viewport").unwrap();
    for name in [
        "camera_location_mm",
        "camera_direction",
        "camera_up",
        "camera_x_axis",
        "camera_y_axis",
        "camera_z_axis",
    ] {
        // #213 keeps the exact typed source camera fields.
        assert_eq!(viewport[name], fields()["viewport"][name]);
    }
    assert_eq!(view.native_ref.as_deref(), Some(record.id()));
    assert_eq!(document.native_ref.as_deref(), Some(record.id()));
    assert_eq!(record.field("neutral_view"), None);
    assert!(viewport.get("neutral_camera").is_none());
}

#[test]
fn multiple_active_views_do_not_invent_one_document_active_view() {
    let mut ir = CadIr::empty();
    install(&mut ir, 0, fields());
    install(&mut ir, 1, fields());
    project(&cadmpeg_test_support::service_decode_context(), &mut ir).unwrap();
    assert_eq!(ir.model.view_presentations.len(), 2);
    assert_eq!(ir.model.presentation_documents.len(), 2);
    assert!(ir
        .model
        .presentation_documents
        .iter()
        .all(|document| document.active_view.is_none()));
}

#[test]
fn inconsistent_source_camera_frame_is_retained_and_reported_without_repair() {
    let mut ir = CadIr::empty();
    let mut source = fields();
    source["viewport"]["camera_up"] = serde_json::json!([1, 0, 0]);
    install(&mut ir, 0, source);
    let losses = project(&cadmpeg_test_support::service_decode_context(), &mut ir).unwrap();
    assert_eq!(losses.len(), 1);
    assert!(ir.model.presentation_documents.is_empty());
    assert_eq!(
        ir.native.namespace("rhino").unwrap().arenas()["views"][0]
            .field("viewport")
            .unwrap()["camera_up"],
        serde_json::json!([1, 0, 0])
    );
}

#[test]
fn half_turn_camera_frame_has_a_finite_axis_angle() {
    let mut source = fields()["viewport"].as_object().unwrap().clone();
    source.insert("camera_x_axis".into(), serde_json::json!([-1, 0, 0]));
    source.insert("camera_y_axis".into(), serde_json::json!([0, -1, 0]));
    source.insert("camera_up".into(), serde_json::json!([0, -1, 0]));
    let [x, y, z, angle] = orientation(&source).unwrap().get();
    assert_eq!([x, y, z], [0.0, 0.0, 1.0]);
    assert!((angle - std::f64::consts::PI).abs() < 1e-12);
}
