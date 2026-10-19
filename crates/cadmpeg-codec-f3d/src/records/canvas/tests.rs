// SPDX-License-Identifier: Apache-2.0

#[test]
fn canvas_prologue_reconstructs_both_flags_and_fixed_zero_bytes() {
    for first_flag in [0, 1] {
        for visible in [0, 1] {
            let mut bytes = [0; 15];
            bytes[10] = first_flag;
            bytes[14] = visible;
            let prologue = crate::records::canvas::DesignCanvasPrologue::try_from(bytes)
                .expect("Canvas prologue");
            assert_eq!(prologue.first_flag, first_flag != 0);
            assert_eq!(prologue.visible(), visible != 0);
        }
    }
    for offset in 0..15 {
        let mut bytes = [0; 15];
        bytes[offset] = if matches!(offset, 10 | 14) { 2 } else { 1 };
        assert!(crate::records::canvas::DesignCanvasPrologue::try_from(bytes).is_err());
    }
}

#[test]
fn canvas_geometry_prologue_decodes_visibility_in_both_forms() {
    use crate::records::canvas::DesignCanvasPrologue;
    let mut expanded = [0; 15];
    expanded[14] = 1;
    assert!(DesignCanvasPrologue::try_from(expanded).is_ok());
    assert_eq!(
        DesignCanvasPrologue::try_from(expanded)
            .ok()
            .map(DesignCanvasPrologue::visible),
        Some(true)
    );

    expanded[14] = 0;
    assert_eq!(
        DesignCanvasPrologue::try_from(expanded)
            .ok()
            .map(DesignCanvasPrologue::visible),
        Some(false)
    );

    let mut compact = [0; 15];
    compact[10] = 1;
    assert!(DesignCanvasPrologue::try_from(compact).is_ok());
    assert_eq!(
        DesignCanvasPrologue::try_from(compact)
            .ok()
            .map(DesignCanvasPrologue::visible),
        Some(false)
    );

    compact[14] = 1;
    assert_eq!(
        DesignCanvasPrologue::try_from(compact)
            .ok()
            .map(DesignCanvasPrologue::visible),
        Some(true)
    );

    compact[11] = 1;
    assert!(DesignCanvasPrologue::try_from(compact).is_err());
}

#[test]
fn canvas_geometry_payload_preserves_source_float_bits() {
    let mut bytes = [0; 77];
    bytes[..4].copy_from_slice(&(-0.0_f32).to_le_bytes());
    for (offset, value) in [
        (5, -0.0_f64),
        (13, 2.5),
        (21, -3.5),
        (29, 1.0),
        (37, -0.0),
        (45, 0.0),
        (53, -0.0),
        (61, 0.0),
        (69, 1.0),
    ] {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let payload = crate::records::canvas::DesignCanvasGeometryPayload::try_from(bytes.as_slice())
        .expect("Canvas payload");
    let (opacity, frame) = payload.decoded();
    let origin = frame.origin().get();
    let u_axis = frame.u_axis();
    let v_axis = frame.v_axis();
    assert_eq!(opacity.to_bits(), (-0.0_f32).to_bits());
    assert_eq!(origin.x.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(origin.y, 25.0);
    assert_eq!(origin.z, -35.0);
    assert_eq!(u_axis.as_raw().y.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(v_axis.as_raw().x.to_bits(), (-0.0_f64).to_bits());
    assert!(crate::records::canvas::DesignCanvasGeometryPayload::try_from(&bytes[..76]).is_err());
}

fn canvas_wire_fixture() -> serde_json::Value {
    serde_json::json!({
        "id":"canvas","scope_record_index":103,"geometry_class_tag":"256","geometry_record_index":101,
        "geometry_first_flag":false,"visible":true,"paired_geometry_class_tag":"257",
        "boundary_segments":[[{"u":-2.0,"v":-1.0},{"u":3.0,"v":-1.0}],[{"u":-2.0,"v":4.0},{"u":3.0,"v":4.0}]],
        "plane_entity_suffix":200,"component_entity_suffix":201,"asset_class_tag":"258","asset_record_index":102,
        "asset_name":"image.png","label":"Canvas","opacity":0.75,"origin":{"x":10.0,"y":20.0,"z":30.0},
        "u_axis":{"x":1.0,"y":0.0,"z":0.0},"v_axis":{"x":0.0,"y":0.0,"z":1.0}
    })
}

fn canvas_image_fixture() -> super::DesignCanvasImage {
    let mut wire = canvas_wire_fixture();
    wire["id"] = serde_json::json!("f3d:native:canvas#0");
    serde_json::from_value(wire).unwrap()
}

#[test]
fn canvas_image_borrowed_wire_matches_owned_wire_bytes() {
    let image = canvas_image_fixture();
    let owned = super::DesignCanvasImageWire::from(image.clone());
    assert_eq!(
        serde_json::to_vec(&image).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn canvas_image_native_retained_limit_refuses_before_record_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let image = canvas_image_fixture();
    let arena_name = "design_canvas_images";
    let needed = serde_json::to_vec(&image).unwrap().len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    super::CANVAS_IMAGE_CLONE_COUNT.with(|count| count.set(0));
    let error = namespace
        .set_arena(&limited, arena_name, std::slice::from_ref(&image))
        .unwrap_err();
    super::CANVAS_IMAGE_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    namespace
        .set_arena(&service, arena_name, std::slice::from_ref(&image))
        .unwrap();
    assert_eq!(namespace.arenas()[arena_name].len(), 1);
}

#[test]
fn canvas_content_edits_are_independent_of_source_payloads() {
    let base = canvas_wire_fixture();
    for (field, replacement) in [
        ("visible", serde_json::json!(false)),
        ("opacity", serde_json::json!(0.5)),
        ("origin", serde_json::json!({"x":11.0,"y":20.0,"z":30.0})),
        ("geometry_first_flag", serde_json::json!(true)),
        ("label", serde_json::json!("A😀")),
        ("asset_name", serde_json::json!("图😀.png")),
    ] {
        let mut wire = base.clone();
        wire[field] = replacement;
        let image: super::DesignCanvasImage = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&image).unwrap(), wire);
        assert!(image.scope_byte_offset().is_none());
        assert!(image.geometry.byte_offset.is_none());
        let mut fidelity = cadmpeg_ir::SourceFidelity::default();
        image.retain_source(&mut fidelity);
        assert!(fidelity.metadata.is_empty());
    }
    for field in [
        "scope_reference_offset",
        "geometry_reference_offset",
        "geometry_byte_offset",
        "geometry_payload",
        "geometry_prologue",
        "geometry_frame_length",
        "boundary_coordinate_offsets",
    ] {
        let mut invalid = base.clone();
        invalid[field] = serde_json::json!(0);
        assert!(
            serde_json::from_value::<super::DesignCanvasImage>(invalid)
                .unwrap_err()
                .to_string()
                .contains(field)
        );
    }
}
#[test]
fn canvas_content_admission_checks_values_and_record_links() {
    let base = canvas_wire_fixture();
    for field in [
        "geometry_class_tag",
        "paired_geometry_class_tag",
        "asset_class_tag",
        "label",
        "asset_name",
    ] {
        let mut invalid = base.clone();
        invalid[field] = serde_json::json!("");
        assert!(
            serde_json::from_value::<super::DesignCanvasImage>(invalid).is_err(),
            "{field}"
        );
    }
    for (field, value) in [
        ("opacity", serde_json::json!(1.5)),
        ("u_axis", serde_json::json!({"x":0.0,"y":0.0,"z":0.0})),
        ("v_axis", serde_json::json!({"x":1.0,"y":0.0,"z":0.0})),
        ("asset_record_index", serde_json::json!(101)),
        (
            "boundary_segments",
            serde_json::json!([[{"u":-2.0,"v":-1.0},{"u":3.0,"v":-1.0}],[{"u":-2.0,"v":4.0},{"u":2.0,"v":4.0}]]),
        ),
    ] {
        let mut invalid = base.clone();
        invalid[field] = value;
        assert!(
            serde_json::from_value::<super::DesignCanvasImage>(invalid).is_err(),
            "{field}"
        );
    }
}
#[test]
fn source_canvas_admission_checks_actual_scope_envelopes_and_overflow() {
    for reference_at in [424, 428] {
        let content = canvas_image_fixture();
        let geometry = super::DesignCanvasGeometry::new(
            content.geometry.class_tags.clone(),
            content.geometry.record_index,
            100,
            content.geometry.label.clone(),
            content.geometry.prologue,
            content.geometry.boundary,
            content.geometry.payload.clone(),
        )
        .unwrap();
        let image = super::DesignCanvasImage::new(
            content.id,
            content.scope_record_index,
            reference_at,
            geometry,
            content.asset,
            content.plane_entity_suffix,
            content.component_entity_suffix,
        )
        .unwrap();
        assert_eq!(image.scope_byte_offset(), Some(402));
        let mut fidelity = cadmpeg_ir::SourceFidelity::default();
        image.retain_source(&mut fidelity);
        assert_eq!(
            fidelity.metadata[&format!("{}.geometry_byte_offset", image.id)],
            "100"
        );
        let restored: super::DesignCanvasImage =
            serde_json::from_value(serde_json::to_value(&image).unwrap()).unwrap();
        assert!(restored.scope_byte_offset().is_none());
        assert_eq!(
            serde_json::to_value(restored).unwrap(),
            serde_json::to_value(image).unwrap()
        );
    }
    let content = canvas_image_fixture();
    assert!(
        super::DesignCanvasGeometry::new(
            content.geometry.class_tags.clone(),
            content.geometry.record_index,
            u64::MAX,
            content.geometry.label,
            content.geometry.prologue,
            content.geometry.boundary,
            content.geometry.payload
        )
        .is_err()
    );
}

#[test]
fn canvas_geometry_payload_decodes_opacity_and_plane_frame() {
    use crate::records::canvas::DesignCanvasGeometryPayload;
    use cadmpeg_ir::math::{Point3, Vector3};
    let mut payload = [0; 77];
    payload[..4].copy_from_slice(&0.75f32.to_le_bytes());
    for (offset, value) in [
        (5, 1.0f64),
        (13, 2.0),
        (21, 3.0),
        (29, 1.0),
        (37, 0.0),
        (45, 0.0),
        (53, 0.0),
        (61, 0.0),
        (69, 1.0),
    ] {
        payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    assert_eq!(
        DesignCanvasGeometryPayload::try_from(payload.as_slice())
            .ok()
            .map(|payload| {
                let (opacity, frame) = payload.decoded();
                (
                    opacity,
                    frame.origin().get(),
                    *frame.u_axis().as_raw(),
                    *frame.v_axis().as_raw(),
                )
            }),
        Some((
            0.75,
            Point3::new(10.0, 20.0, 30.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ))
    );

    payload[4] = 1;
    assert!(DesignCanvasGeometryPayload::try_from(payload.as_slice()).is_err());
    payload[4] = 0;
    payload[53..61].copy_from_slice(&1.0f64.to_le_bytes());
    assert!(DesignCanvasGeometryPayload::try_from(payload.as_slice()).is_err());
    payload[53..61].copy_from_slice(&0.0f64.to_le_bytes());
    payload[29..37].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(DesignCanvasGeometryPayload::try_from(payload.as_slice()).is_err());
    payload[29..37].copy_from_slice(&1.0f64.to_le_bytes());
    payload[5..13].copy_from_slice(&1.0e308f64.to_le_bytes());
    assert!(DesignCanvasGeometryPayload::try_from(payload.as_slice()).is_err());
}

#[test]
fn canvas_bounds_preserve_segment_order_and_derive_extents() {
    use cadmpeg_ir::math::Point2;
    for (segments, mirroring) in [
        (
            [
                [Point2::new(3.0, 4.0), Point2::new(-2.0, 4.0)],
                [Point2::new(3.0, -1.0), Point2::new(-2.0, -1.0)],
            ],
            (true, true),
        ),
        (
            [
                [Point2::new(3.0, -1.0), Point2::new(3.0, 4.0)],
                [Point2::new(-2.0, -1.0), Point2::new(-2.0, 4.0)],
            ],
            (true, false),
        ),
    ] {
        let bounds =
            crate::records::canvas::DesignCanvasBounds::try_from(segments).expect("Canvas bounds");
        assert_eq!(bounds.segments(), segments);
        assert_eq!(bounds.mirroring(), mirroring);
        assert_eq!(
            bounds.extents(),
            [Point2::new(-2.0, -1.0), Point2::new(3.0, 4.0)]
        );
    }
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let segments = [
            [Point2::new(-2.0, -1.0), Point2::new(3.0, -1.0)],
            [Point2::new(-2.0, 4.0), Point2::new(invalid, 4.0)],
        ];
        assert!(crate::records::canvas::DesignCanvasBounds::try_from(segments).is_err());
    }
}

#[test]
fn canvas_bounds_decode_u_and_v_mirroring_from_endpoint_order() {
    use crate::records::canvas::DesignCanvasBounds;
    use cadmpeg_ir::math::Point2;
    assert_eq!(
        DesignCanvasBounds::try_from([
            [Point2::new(-2.0, -1.0), Point2::new(3.0, -1.0)],
            [Point2::new(-2.0, 4.0), Point2::new(3.0, 4.0)],
        ])
        .ok()
        .map(DesignCanvasBounds::mirroring),
        Some((false, false))
    );
    assert_eq!(
        DesignCanvasBounds::try_from([
            [Point2::new(3.0, -1.0), Point2::new(-2.0, -1.0)],
            [Point2::new(3.0, 4.0), Point2::new(-2.0, 4.0)],
        ])
        .ok()
        .map(DesignCanvasBounds::mirroring),
        Some((true, false))
    );
    assert_eq!(
        DesignCanvasBounds::try_from([
            [Point2::new(-2.0, 4.0), Point2::new(3.0, 4.0)],
            [Point2::new(-2.0, -1.0), Point2::new(3.0, -1.0)],
        ])
        .ok()
        .map(DesignCanvasBounds::mirroring),
        Some((false, true))
    );
    assert_eq!(
        DesignCanvasBounds::try_from([
            [Point2::new(3.0, 4.0), Point2::new(-2.0, 4.0)],
            [Point2::new(3.0, -1.0), Point2::new(-2.0, -1.0)],
        ])
        .ok()
        .map(DesignCanvasBounds::mirroring),
        Some((true, true))
    );
    assert_eq!(
        DesignCanvasBounds::try_from([
            [Point2::new(-2.0, 4.0), Point2::new(-2.0, -1.0)],
            [Point2::new(3.0, 4.0), Point2::new(3.0, -1.0)],
        ])
        .ok()
        .map(DesignCanvasBounds::mirroring),
        Some((false, true))
    );
    assert_eq!(
        DesignCanvasBounds::try_from([
            [Point2::new(3.0, -1.0), Point2::new(3.0, 4.0)],
            [Point2::new(-2.0, -1.0), Point2::new(-2.0, 4.0)],
        ])
        .ok()
        .map(DesignCanvasBounds::mirroring),
        Some((true, false))
    );
    assert_eq!(
        DesignCanvasBounds::try_from([
            [Point2::new(-2.0, -1.0), Point2::new(3.0, -1.0)],
            [
                Point2::new(f64::from_bits((-2.0f64).to_bits() + 4), 4.0),
                Point2::new(f64::from_bits(3.0f64.to_bits() + 4), 4.0),
            ],
        ])
        .ok()
        .map(DesignCanvasBounds::mirroring),
        Some((false, false))
    );
    assert_eq!(
        DesignCanvasBounds::try_from([
            [Point2::new(-2.0, -1.0), Point2::new(3.0, -1.0)],
            [Point2::new(-2.0, 4.0), Point2::new(2.0, 4.0)],
        ])
        .ok()
        .map(DesignCanvasBounds::mirroring),
        None
    );
}
