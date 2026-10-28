// SPDX-License-Identifier: Apache-2.0
//! Inward view/camera links and native-only saved-view emission.

use super::super::{encode_view, list, settings};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::write::NativeDisposition;
use cadmpeg_ir::presentation::{PresentationState, PresentationStateKind};
use cadmpeg_ir::units::{FiniteVector, NonzeroVector};
use cadmpeg_ir::{CadIr, NativeRecord};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

const EPS_CAMERA_AXES: f64 = 1e-12;

fn fields() -> Map<String, Value> {
    serde_json::from_value(json!({
        "list_kind":"named","list_index":0,"name":"Source view",
        "target_millimeters":[11,12,13],
        "show_construction_grid":false,"show_construction_axes":true,"show_world_axes":false,
        "legacy_display_mode":2,
        "window_position":{"maximized":false,"left":0.125,"right":0.875,"top":0.25,"bottom":0.75,"floating_viewport":1},
        "construction_plane":{"plane_origin_mm":[2,3,4],"plane_x_axis":[1,0,0],"plane_y_axis":[0,1,0],"plane_z_axis":[0,0,1],"plane_equation_mm":[0,0,1,-4],"grid_spacing_mm":5,"snap_spacing_mm":2.5,"grid_line_count":40,"thick_line_frequency":5,"name":"Source plane","depth_buffer":false},
        "viewport":{"camera_valid":true,"frustum_valid":true,"port_valid":true,"projection":2,
            "camera_location_mm":[1.25,-2.5,3.75],"camera_direction":[0,0,-1],"camera_up":[0,1,0],
            "camera_x_axis":[1,0,0],"camera_y_axis":[0,1,0],"camera_z_axis":[0,0,1],
            "frustum_mm":[-2,2,-1,1,0.1,100],"port":[0,1920,1080,0,0,1],
            "source_uuid":"11111111-2222-3333-4444-555555555555",
            "camera_up_locked":true,"camera_direction_locked":false,"camera_location_locked":true,
            "frustum_left_right_symmetric":true,"frustum_top_bottom_symmetric":false,
            "target_millimeters":[4,5,6],"camera_frame_valid":true,"view_scale":[1,2,3]},
        "attributes":null,"trace_image":null,"wallpaper":null,"parse_warnings":[]
    })).expect("complete current native view")
}

fn read(settings: &[Vec<u8>], project: bool) -> CadIr {
    let mut items = vec![crate::writer::units_record(0.01, 0.01).expect("millimetre units")];
    items.extend_from_slice(settings);
    let archive = crate::test_support::test_dump::minimal_document(
        "80",
        &[
            crate::writer::tests::table(crate::writer::TCODE_SETTINGS_TABLE, &items)
                .expect("settings table"),
            crate::writer::tests::table(crate::writer::TCODE_OBJECT_TABLE, &[])
                .expect("objects table"),
        ],
    );
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&archive, &arena, &policy).expect("decode context");
    let scan = crate::container::scan(&ctx, &archive).expect("scan saved view");
    let mut ir = CadIr::empty();
    let installed = crate::views::install(&ctx, &scan, &mut ir).expect("read saved-view grammar");
    assert!(installed.losses.is_empty(), "{:?}", installed.losses);
    assert!(
        installed.opaque_records.is_empty(),
        "{:?}",
        installed.opaque_records
    );
    if project {
        let before = serde_json::to_value(&ir.native).expect("native snapshot");
        let losses = crate::neutral_projection::project(&ctx, &scan, Vec::new(), &mut ir)
            .expect("project saved view");
        assert!(losses.is_empty(), "{losses:?}");
        assert_eq!(
            serde_json::to_value(&ir.native).expect("projected native"),
            before
        );
    }
    ir
}

fn source() -> CadIr {
    let encoded = encode_view(&fields(), None, None).expect("native saved-view source");
    read(&[list(0x2000_8036, &[encoded]).expect("named list")], true)
}

fn native(ir: &CadIr) -> &NativeRecord {
    let records = &ir.native.namespace("rhino").expect("namespace").arenas()["views"];
    assert_eq!(records.len(), 1);
    &records[0]
}

#[test]
fn projection_preserves_all_view_and_camera_fields_without_native_backlinks() {
    let ir = source();
    let record = native(&ir);
    assert_eq!(ir.model.view_presentations.len(), 1);
    assert_eq!(ir.model.presentation_documents.len(), 1);
    assert_eq!(
        ir.model.view_presentations[0].native_ref.as_deref(),
        Some(record.id())
    );
    assert_eq!(
        ir.model.presentation_documents[0].native_ref.as_deref(),
        Some(record.id())
    );
    assert_eq!(record.field("neutral_view"), None);
    let viewport = record.field("viewport").expect("native viewport");
    assert!(viewport.get("neutral_camera").is_none());
    for name in [
        "camera_location_mm",
        "camera_direction",
        "camera_up",
        "camera_x_axis",
        "camera_y_axis",
        "camera_z_axis",
        "frustum_mm",
        "port",
        "target_millimeters",
        "view_scale",
    ] {
        let expected = &fields()["viewport"][name];
        let actual = viewport[name].as_array().expect("native coordinate array");
        for (actual, expected) in actual
            .iter()
            .zip(expected.as_array().expect("source coordinates"))
        {
            assert_eq!(actual.as_f64(), expected.as_f64(), "{name}");
        }
    }
    assert_eq!(record.field("name"), Some(json!("Source view")));
}

#[test]
fn current_inward_name_and_camera_pose_override_only_their_native_projection() {
    let mut ir = source();
    let source_native = serde_json::to_value(&ir.native).expect("source native");
    let id = native(&ir).id().to_owned();
    ir.model.view_presentations[0].properties.insert(
        cadmpeg_core::text::NonBlankString::new("name").expect("name property"),
        "Current view".into(),
    );
    let state = &ir.model.presentation_documents[0].states()[0];
    let PresentationStateKind::Camera(original) = &state.kind else {
        panic!("camera source")
    };
    let mut current = original.clone();
    current.position = FiniteVector::new([10.0, 20.0, 30.0]);
    current.orientation = NonzeroVector::new([0.0, 0.0, 1.0, std::f64::consts::FRAC_PI_2]);
    ir.model.presentation_documents[0]
        .set_states(vec![PresentationState {
            kind: PresentationStateKind::Camera(current),
            order: 0,
            attributes: BTreeMap::new(),
            assets: Vec::new(),
        }])
        .expect("current camera state");
    let result = settings(&ir).expect("current saved view");
    assert!(result.losses.is_empty(), "{:?}", result.losses);
    assert!(result.regenerated.contains(&id));
    assert!(!result.written.contains(&id));
    let output = read(&result.settings, false);
    let record = native(&output);
    assert_eq!(record.field("name"), Some(json!("Current view")));
    let viewport = record.field("viewport").expect("current viewport");
    assert_eq!(viewport["camera_location_mm"], json!([10.0, 20.0, 30.0]));
    for (name, expected) in [
        ("camera_x_axis", [0.0, 1.0, 0.0]),
        ("camera_y_axis", [-1.0, 0.0, 0.0]),
        ("camera_z_axis", [0.0, 0.0, 1.0]),
    ] {
        for (actual, expected) in viewport[name]
            .as_array()
            .expect("camera axis")
            .iter()
            .zip(expected)
        {
            assert!((actual.as_f64().expect("coordinate") - expected).abs() < EPS_CAMERA_AXES);
        }
    }
    let old = native(&ir).field("viewport").expect("source viewport");
    for name in [
        "projection",
        "frustum_mm",
        "port",
        "source_uuid",
        "camera_up_locked",
        "camera_direction_locked",
        "camera_location_locked",
        "frustum_left_right_symmetric",
        "frustum_top_bottom_symmetric",
        "target_millimeters",
        "camera_frame_valid",
        "view_scale",
    ] {
        assert_eq!(viewport[name], old[name], "{name}");
    }
    for name in [
        "construction_plane",
        "window_position",
        "target_millimeters",
    ] {
        assert_eq!(record.field(name), native(&ir).field(name));
    }
    assert_eq!(
        serde_json::to_value(&ir.native).expect("unchanged native"),
        source_native
    );
}

#[test]
fn missing_last_view_and_camera_counterparts_reemit_the_complete_native_saved_view() {
    let mut ir = source();
    let id = native(&ir).id().to_owned();
    let before = native(&ir).fields().expect("source native fields");
    ir.model.view_presentations.clear();
    ir.model.presentation_documents.clear();
    assert!(ir.scope.model);
    let result = settings(&ir).expect("native W1 view");
    assert!(result.losses.is_empty(), "{:?}", result.losses);
    assert!(result.regenerated.is_empty());
    assert!(result.written.contains(&id));
    let output = read(&result.settings, false);
    assert_eq!(
        native(&output).fields().expect("written native fields"),
        before
    );
    assert_eq!(
        native(&ir).fields().expect("untouched native fields"),
        before
    );
    let dispositions = crate::writer::native_dispositions(&ir).expect("native dispositions");
    assert_eq!(
        dispositions.record_disposition("rhino", "views", &id),
        NativeDisposition::Written
    );
}

#[test]
fn removing_only_one_counterpart_preserves_the_other_current_projection_and_stored_fields() {
    let original = source();
    let mut without_camera = original.clone();
    without_camera.model.presentation_documents.clear();
    without_camera.model.view_presentations[0]
        .properties
        .insert(
            cadmpeg_core::text::NonBlankString::new("name").expect("name property"),
            "Only current view".into(),
        );
    let result = settings(&without_camera).expect("view without current camera");
    assert!(result.losses.is_empty(), "{:?}", result.losses);
    let output = read(&result.settings, false);
    assert_eq!(
        native(&output).field("name"),
        Some(json!("Only current view"))
    );
    assert_eq!(
        native(&output).field("viewport"),
        native(&original).field("viewport")
    );
    let mut without_view = original.clone();
    without_view.model.view_presentations.clear();
    let result = settings(&without_view).expect("camera without current view");
    assert!(result.losses.is_empty(), "{:?}", result.losses);
    let output = read(&result.settings, false);
    assert_eq!(native(&output).field("name"), Some(json!("Source view")));
    assert_eq!(
        native(&output).field("viewport"),
        native(&original).field("viewport")
    );
}

#[test]
fn ambiguous_current_camera_links_are_reported_instead_of_selecting_one() {
    let mut ir = source();
    let id = native(&ir).id().to_owned();
    let mut duplicate = ir.model.presentation_documents[0].clone();
    duplicate.id = "test:model:presentation#second-camera"
        .try_into()
        .expect("second camera identity");
    ir.model.presentation_documents.push(duplicate);
    let result = settings(&ir).expect("ambiguous current cameras");
    assert!(result.losses.iter().any(|loss| loss.message.contains(&id)
        && loss
            .message
            .contains("more than one current view or camera")));
    assert!(!result.written.contains(&id));
    assert!(!result.regenerated.contains(&id));
}

#[test]
fn unrepresentable_optional_viewport_field_combination_is_reported_without_default_filling() {
    let mut ir = source();
    let record = native(&ir);
    let id = record.id().to_owned();
    let mut fields = record.fields().expect("source view fields");
    fields.get_mut("viewport").expect("viewport")["target_millimeters"] = Value::Null;
    let changed = NativeRecord::new(
        id.as_str().try_into().expect("native identity"),
        fields.clone(),
    )
    .expect("current native field edit");
    ir.native
        .namespace_mut("rhino")
        .arenas_mut()
        .insert("views".into(), vec![changed]);
    ir.model.view_presentations.clear();
    ir.model.presentation_documents.clear();
    let result = settings(&ir).expect("reported optional field conflict");
    assert!(result.settings.is_empty());
    assert!(result.losses.iter().any(|loss| loss.message.contains(&id)
        && loss.message.contains("no complete saved-view encoding")));
    assert!(!result.written.contains(&id));
    assert!(!result.regenerated.contains(&id));
    assert_eq!(native(&ir).fields().expect("native fields kept"), fields);
    let dispositions = crate::writer::native_dispositions(&ir).expect("native dispositions");
    assert_eq!(
        dispositions.record_disposition("rhino", "views", &id),
        NativeDisposition::Dropped
    );
}

#[test]
fn native_only_inconsistent_camera_vectors_are_reemitted_without_repair() {
    let mut source = fields();
    source["viewport"]["camera_up"] = json!([1, 0, 0]);
    let encoded = encode_view(&source, None, None).expect("stored inconsistent camera frame");
    let mut ir = read(
        &[list(0x2000_8036, &[encoded]).expect("source named view")],
        false,
    );
    let before = native(&ir).fields().expect("actual source native");
    ir.model.view_presentations.clear();
    ir.model.presentation_documents.clear();
    let result = settings(&ir).expect("native-only camera frame");
    assert!(result.losses.is_empty(), "{:?}", result.losses);
    let output = read(&result.settings, false);
    assert_eq!(
        native(&output).fields().expect("unaltered camera frame"),
        before
    );
    assert_eq!(
        native(&output).field("viewport").expect("written viewport")["camera_up"],
        json!([1.0, 0.0, 0.0])
    );
}

#[test]
fn native_view_scale_without_a_frame_field_is_not_silently_dropped_as_written() {
    let mut ir = source();
    let record = native(&ir);
    let id = record.id().to_owned();
    let mut fields = record.fields().expect("source view fields");
    fields.get_mut("viewport").expect("viewport")["camera_frame_valid"] = Value::Null;
    assert!(fields["viewport"]["target_millimeters"].is_array());
    assert_eq!(fields["viewport"]["view_scale"], json!([1.0, 2.0, 3.0]));
    let changed = NativeRecord::new(
        id.as_str().try_into().expect("native identity"),
        fields.clone(),
    )
    .expect("held scale and absent frame field");
    ir.native
        .namespace_mut("rhino")
        .arenas_mut()
        .insert("views".into(), vec![changed]);
    ir.model.view_presentations.clear();
    ir.model.presentation_documents.clear();
    let result = settings(&ir).expect("reported viewport field conflict");
    assert!(result.settings.is_empty());
    assert!(result.losses.iter().any(|loss| loss.message.contains(&id)
        && loss.message.contains("no complete saved-view encoding")));
    assert!(!result.written.contains(&id));
    assert!(!result.regenerated.contains(&id));
    assert_eq!(
        native(&ir)
            .fields()
            .expect("all current native fields kept"),
        fields
    );
    let dispositions = crate::writer::native_dispositions(&ir).expect("native dispositions");
    assert_eq!(
        dispositions.record_disposition("rhino", "views", &id),
        NativeDisposition::Dropped
    );
}

#[test]
fn present_nil_viewport_uuid_is_not_claimed_written_as_an_absent_source_uuid() {
    let mut ir = source();
    let record = native(&ir);
    let id = record.id().to_owned();
    let mut fields = record.fields().expect("genuine source view fields");
    fields.get_mut("viewport").expect("source viewport")["source_uuid"] =
        json!("00000000-0000-0000-0000-000000000000");
    let changed = NativeRecord::new(
        id.as_str().try_into().expect("native identity"),
        fields.clone(),
    )
    .expect("present nil source uuid");
    ir.native
        .namespace_mut("rhino")
        .arenas_mut()
        .insert("views".into(), vec![changed]);
    ir.model.view_presentations.clear();
    ir.model.presentation_documents.clear();
    assert!(ir.scope.model);
    let result = settings(&ir).expect("reported unrepresentable nil presence");
    assert!(result.settings.is_empty());
    assert!(result.losses.iter().any(|loss| loss.message == format!("{id}: present nil viewport source_uuid has no complete saved-view encoding that retains its presence")));
    assert!(!result.written.contains(&id));
    assert!(!result.regenerated.contains(&id));
    assert_eq!(
        native(&ir)
            .fields()
            .expect("native fields retained exactly"),
        fields
    );
    let dispositions = crate::writer::native_dispositions(&ir).expect("native dispositions");
    assert_eq!(
        dispositions.record_disposition("rhino", "views", &id),
        NativeDisposition::Dropped
    );
}
