// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::ids::Identity;
use cadmpeg_ir::presentation::{
    CameraState, PresentationDocument, PresentationId, PresentationState, PresentationStateKind,
    ViewPresentation,
};
use cadmpeg_ir::units::{FiniteVector, NonzeroVector};
use std::collections::BTreeMap;

#[test]
fn current_neutral_pose_and_name_regenerate_saved_view_without_source_bytes() {
    let mut ir = CadIr::empty();
    let camera_id: PresentationId = "test:model:presentation#camera".try_into().unwrap();
    let view_id: PresentationId = "test:model:presentation#view".try_into().unwrap();
    let mut camera = PresentationDocument::new(camera_id.clone());
    camera
        .set_states(vec![PresentationState {
            kind: PresentationStateKind::Camera(CameraState {
                position: FiniteVector::new([10.0, 20.0, 30.0]),
                orientation: NonzeroVector::new([0.0, 0.0, 1.0, std::f64::consts::FRAC_PI_2]),
                properties: BTreeMap::new(),
            }),
            order: 0,
            attributes: BTreeMap::new(),
            assets: Vec::new(),
        }])
        .unwrap();
    ir.model.presentation_documents.push(camera);
    ir.model.view_presentations.push(ViewPresentation {
        id: view_id.clone(),
        object: None,
        order: 0,
        expanded: None,
        visible: None,
        display_mode: Some("2".into()),
        selection_style: None,
        line_width: None,
        point_size: None,
        properties: BTreeMap::from([(
            cadmpeg_core::text::NonBlankString::new("name").unwrap(),
            "Edited view".into(),
        )]),
        native_ref: None,
    });
    let fields=serde_json::from_value(serde_json::json!({"list_kind":"named","list_index":0,"neutral_view":view_id,"viewport":{"neutral_camera":camera_id,"camera_valid":true,"frustum_valid":true,"port_valid":true,"projection":2,"frustum_mm":[-2,2,-1,1,0.1,100],"port":[0,1920,1080,0,0,1]}})).unwrap();
    ir.native.namespace_mut("rhino").arenas_mut().insert(
        "views".into(),
        vec![cadmpeg_ir::NativeRecord::new(
            Identity::new("rhino:document:view#saved").unwrap(),
            fields,
        )
        .unwrap()],
    );
    let exported = settings(&ir).unwrap();
    assert!(exported.losses.is_empty());
    assert_eq!(exported.regenerated.len(), 1);
    let mut settings = vec![crate::writer::units_record(0.01, 0.01).unwrap()];
    settings.extend(exported.settings);
    let archive = crate::test_support::test_dump::minimal_document(
        "80",
        &[
            crate::writer::table(TCODE_SETTINGS_TABLE, &settings).unwrap(),
            crate::writer::table(crate::writer::TCODE_OBJECT_TABLE, &[]).unwrap(),
        ],
    );
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&archive, &arena, &DecodePolicy::default()).unwrap();
    let scan = crate::container::scan(&ctx, &archive).unwrap();
    let mut decoded = CadIr::empty();
    crate::views::install(&ctx, &scan, &mut decoded).unwrap();
    let record = &decoded.native.namespace("rhino").unwrap().arenas()["views"][0];
    assert_eq!(record.field("name").unwrap(), "Edited view");
    let viewport = record.field("viewport").unwrap();
    assert_eq!(
        viewport["camera_location_mm"],
        serde_json::json!([10.0, 20.0, 30.0])
    );
    for (key, expected) in [
        ("camera_x_axis", [0.0, 1.0, 0.0]),
        ("camera_y_axis", [-1.0, 0.0, 0.0]),
        ("camera_direction", [0.0, 0.0, -1.0]),
    ] {
        for (actual, expected) in viewport[key].as_array().unwrap().iter().zip(expected) {
            assert!((actual.as_f64().unwrap() - expected).abs() < 1e-12);
        }
    }
}
