// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};
use cadmpeg_ir::sketches::{SketchConstraintDefinition, SketchEntityId};
use cadmpeg_ir::Exactness;

use crate::container::{self, role, Layout};
use crate::surface::TorusRadius2Encoding;
use crate::test_support::*;
use crate::CreoCodec;

#[test]
fn scan_discovers_labeled_curve_prototypes() {
    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(b"crv_array\0crv_id\0\x07type\0\x08feat_id\0\x04");
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.curves.prototypes.len(), 1);
    assert_eq!(scan.curves.prototypes[0].id, 7);
    assert_eq!(scan.curves.prototypes[0].type_byte, 8);
    assert_eq!(scan.curves.prototypes[0].feature_id, Some(4));
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let records = &result.ir().native.namespace("creo").unwrap().arenas["curve_prototypes"];
    assert_eq!(records[0].fields()["curve_id"], 7);
    assert_eq!(records[0].fields()["type_byte"], 8);
    assert_eq!(records[0].fields()["generating_feature_id"], 4);
}

#[test]
fn scan_discovers_curve_halfedge_topology() {
    let mut payload = visibgeom_payload(0, 1);
    payload
        .extend_from_slice(b"topol_ref_data\0\x07\x08\x04\x01\xf6\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.curves.topology_rows.len(), 1);
    assert_eq!(scan.curves.topology_rows[0].faces, [10, 11]);
    assert_eq!(scan.curves.topology_rows[0].next_edges, [7, 7]);
    assert_eq!(scan.topology.half_edges.len(), 2);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let row = &result.ir().native.namespace("creo").unwrap().arenas["curve_topology_rows"][0];
    assert_eq!(row.fields()["curve_id"], 7);
    assert_eq!(row.fields()["type_byte"], 8);
    assert_eq!(row.fields()["feature_id"], 4);
    assert_eq!(row.fields()["directions"][0], 1);
    assert_eq!(row.fields()["directions"][1], 0xf6);
    assert_eq!(row.fields()["faces"][0], 10);
    assert_eq!(row.fields()["faces"][1], 11);
    assert_eq!(row.fields()["next_edges"][0], 7);
    assert_eq!(row.fields()["next_edges"][1], 7);
    assert_eq!(
        result.source_fidelity().annotations.provenance["creo:visibgeom:curve_topology#7"]
            .tag
            .as_deref(),
        Some("curve_topology_row")
    );
    let curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "creo:visibgeom:curve#7")
        .expect("retained unresolved curve carrier");
    assert!(matches!(
        curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Unknown { record: Some(_) }
    ));
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::RETAINED_UNKNOWN_VISIBLE_CURVE_ROW_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::UNTRANSFERRED_VISIBLE_CURVE_ROW_COUNT),
        1
    );
}

#[test]
fn repeated_curve_rows_receive_source_offset_native_keys() {
    let mut payload = visibgeom_payload(0, 2);
    payload.extend_from_slice(b"topol_ref_data\0");
    payload.extend_from_slice(b"\x07\x08\x04\x01\xf6\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    payload.extend_from_slice(b"\x07\x08\x04\x01\xf6\x0c\x0d\x07\x07\0\0\xe3\xe1\xe3");
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.curves.topology_rows.len(), 2);
    assert_eq!(
        scan.curves.topology_rows[0].id,
        scan.curves.topology_rows[1].id
    );
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let rows = &result
        .ir()
        .native
        .namespace("creo")
        .expect("native namespace")
        .arenas["curve_topology_rows"];
    assert_eq!(rows.len(), 2);
    for (native, source) in rows.iter().zip(&scan.curves.topology_rows) {
        assert_eq!(
            native.id(),
            format!("creo:visibgeom:curve_topology#7-{:020}", source.offset)
        );
    }
    assert_ne!(rows[0].id(), rows[1].id());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn scan_decodes_long_terminated_rows_in_each_curve_namespace() {
    let mut payload = b"crv_array\0topol_ref_data\0".to_vec();
    payload.extend_from_slice(b"\x07\x08\x04\x01\xf6\x0a\x0b\x07\x07\0\0\xe3");
    payload.extend_from_slice(b"\xe1\xf5\x05\xf6\xe3");
    payload.extend_from_slice(b"crv_array\0topol_ref_data\0");
    payload.extend_from_slice(b"\x08\x08\x05\x01\xf6\x0c\x0d\x08\x08\0\0\xe3");
    payload.extend_from_slice(b"\xe1\xf5\x05\xf6\xe3");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.curves.topology_rows.len(), 2);
    assert_eq!(scan.curves.topology_rows[0].id, 7);
    assert_eq!(scan.curves.topology_rows[0].faces, [10, 11]);
    assert_eq!(scan.curves.topology_rows[1].id, 8);
    assert_eq!(scan.curves.topology_rows[1].faces, [12, 13]);
}

#[test]
fn scan_bounds_curve_parameter_body_before_topology_suffix() {
    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(b"topol_ref_data\0\x07\x08\x04\x01\xf6");
    payload.extend_from_slice(&[0x0f, 0xe4, 0xf7, 0x81, 0x00]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0, 0xff]);
    payload.extend_from_slice(b"\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.curves.parameters.len(), 1);
    let parameters = &scan.curves.parameters[0];
    assert_eq!(parameters.curve_id, 7);
    assert_eq!(parameters.type_byte, 8);
    assert_eq!(parameters.scalar_values, vec![0.0, 1.0, 3.0]);
    assert_eq!(parameters.scalar_tokens[2].offset, 5);
    assert_eq!(parameters.scalar_tokens[2].length, 8);
    assert_eq!(parameters.scalar_tokens[2].raw[0], 0x46);
    assert_eq!(parameters.skipped_references, vec![256]);
    assert_eq!(parameters.references[0].entity_id, 256);
    assert_eq!(parameters.references[0].offset, 2);
    assert_eq!(parameters.references[0].length, 3);
    assert_eq!(parameters.opaque_spans.len(), 1);
    assert_eq!(parameters.opaque_spans[0].offset, 13);
    assert_eq!(parameters.opaque_spans[0].raw, [0xff]);
    assert_eq!(parameters.suffix, crate::curve::CurveSuffixStatus::Unique);
    assert_eq!(parameters.body.last(), Some(&0xff));
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let record = &result.ir().native.namespace("creo").unwrap().arenas["curve_parameters"][0];
    assert_eq!(record.fields()["curve_id"], 7);
    assert_eq!(record.fields()["type_byte"], 8);
    assert_eq!(
        record.fields()["body"].as_array().unwrap().len(),
        parameters.body.len()
    );
    assert_eq!(record.fields()["scalar_values"][2], 3.0);
    assert_eq!(record.fields()["scalar_tokens"][2]["offset"], 5);
    assert_eq!(record.fields()["scalar_tokens"][2]["raw"][0], 0x46);
    assert_eq!(record.fields()["skipped_references"][0], 256);
    assert_eq!(record.fields()["references"][0]["entity_id"], 256);
    assert_eq!(record.fields()["references"][0]["offset"], 2);
    assert_eq!(record.fields()["opaque_spans"][0]["offset"], 13);
    assert_eq!(record.fields()["opaque_spans"][0]["raw"][0], 0xff);
    assert_eq!(record.fields()["suffix"], "unique");
    assert!(record.fields()["suffix_candidate_count"].is_null());
    assert_eq!(
        result.source_fidelity().annotations.provenance["creo:visibgeom:curve_parameter#7"]
            .tag
            .as_deref(),
        Some("curve_parameter_record")
    );
}

#[test]
fn scan_resolves_section_scalar_cache_in_curve_rows() {
    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(b"topol_ref_data\0\x07\x08\x04\x01\xf6");
    payload.extend_from_slice(&[0x18, 0x00, 0xff]);
    payload.extend_from_slice(b"\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.curves.parameters.len(), 1);
    assert_eq!(scan.curves.parameters[0].scalar_values, vec![3.0]);
}

#[test]
fn scan_decodes_pcurve_endpoints_in_both_face_frames() {
    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(b"topol_ref_data\0\x07\x00\x04\x01\xf6");
    payload.extend_from_slice(&[0x0f, 0xe4]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x0f, 0xe4, 0x0f]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.push(0xe4);
    payload.extend_from_slice(b"\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.curves.pcurves.len(), 1);
    let pcurve = &scan.curves.pcurves[0];
    assert_eq!(pcurve.curve_id, 7);
    assert_eq!(pcurve.faces, [10, 11]);
    assert_eq!(pcurve.face_0_endpoints, [[0.0, 1.0], [1.0, 0.0]]);
    assert_eq!(pcurve.face_1_endpoints, [[3.0, 0.0], [3.0, 1.0]]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let records = &result.ir().native.namespace("creo").unwrap().arenas["pcurve_endpoints"];
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id(), "creo:visibgeom:pcurve_endpoints#7");
    assert_eq!(records[0].fields()["faces"][0], 10);
    assert_eq!(records[0].fields()["faces"][1], 11);
    assert_eq!(records[0].fields()["source_form"], "positional");

    let mut mismatched_topology = scan.curves.topology_rows.clone();
    mismatched_topology[0].type_byte = 1;
    assert!(
        crate::curve::pcurve_endpoints(&scan.curves.parameters, &mismatched_topology).is_empty()
    );
}

#[test]
fn scan_decodes_positive_dict_pcurve_slots() {
    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(b"topol_ref_data\0\x07\x00\x04\x01\xf6");
    payload.extend_from_slice(&[0x0f, 0xe4]);
    payload.extend_from_slice(&[0x98, 1, 2, 3, 4, 5, 6]);
    payload.extend_from_slice(&[0x0f, 0xe4, 0x2f, 0x43, 0]);
    payload.extend_from_slice(&[0x98, 1, 2, 3, 4, 5, 6]);
    payload.extend_from_slice(&[0x2f, 0x43, 0]);
    payload.extend_from_slice(b"\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");

    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));
    let expected = f64::from_be_bytes([0x40, 0x0d, 1, 2, 3, 4, 5, 6]);

    assert_eq!(scan.curves.parameters.len(), 1);
    assert_eq!(
        scan.curves.parameters[0].scalar_values,
        vec![0.0, 1.0, expected, 0.0, 1.0, 38.0, expected, 38.0]
    );
    assert_eq!(scan.curves.parameters[0].opaque_spans, Vec::new());
    assert_eq!(scan.curves.pcurves.len(), 1);
    assert_eq!(
        scan.curves.pcurves[0].face_0_endpoints,
        [[0.0, 1.0], [1.0, 38.0]]
    );
    assert_eq!(
        scan.curves.pcurves[0].face_1_endpoints,
        [[expected, 0.0], [expected, 38.0]]
    );
}

#[test]
fn scan_decodes_standalone_zero_slots_in_pcurve_endpoint_frames() {
    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(b"topol_ref_data\0\x07\x08\x04\x01\xf6");
    payload.extend_from_slice(&[0x12, 0xe4]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x12, 0xe4, 0x12]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0xe4]);
    payload.extend_from_slice(b"\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.curves.parameters.len(), 1);
    assert_eq!(scan.curves.parameters[0].scalar_tokens.len(), 5);
    assert_eq!(scan.curves.parameters[0].opaque_spans.len(), 3);
    assert!(scan.curves.parameters[0]
        .opaque_spans
        .iter()
        .all(|span| span.raw == [0x12]));
    assert_eq!(scan.curves.pcurves.len(), 1);
    assert_eq!(
        scan.curves.pcurves[0].face_0_endpoints,
        [[0.0, 1.0], [1.0, 0.0]]
    );
    assert_eq!(
        scan.curves.pcurves[0].face_1_endpoints,
        [[3.0, 0.0], [3.0, 1.0]]
    );
}

#[test]
fn scan_decodes_held_scalar_slots_in_pcurve_endpoint_frames() {
    let held_value = [0x29, 0xf6, 0x49];
    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(b"topol_ref_data\0\x07\x00\x04\x01\xf6");
    payload.extend_from_slice(&[0x0f, 0xd7, 0xe8, 0x03]);
    payload.extend_from_slice(&held_value);
    payload.extend_from_slice(&[0x1e, 0x0f, 0xe4, 0x0f, 0xe4, 0x0f, 0xe4]);
    payload.extend_from_slice(b"\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    let held = crate::psb::short_form_float(&held_value, 0)
        .expect("complete held scalar")
        .0;
    assert_eq!(scan.curves.pcurves.len(), 1);
    assert_eq!(
        scan.curves.pcurves[0].face_0_endpoints,
        [[0.0, held], [0.0, 1.0]]
    );
    assert_eq!(
        scan.curves.pcurves[0].face_1_endpoints,
        [[0.0, 1.0], [0.0, 1.0]]
    );
}

#[test]
fn scan_withholds_nine_slot_pcurve_endpoint_frames() {
    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(b"topol_ref_data\0\x07\x08\x04\x01\xf6");
    payload.extend_from_slice(&[0x12, 0xe4]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x12, 0xe4, 0x12]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0xe4, 0x12]);
    payload.extend_from_slice(b"\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert!(scan.curves.pcurves.is_empty());
}

#[test]
fn scan_withholds_pcurve_endpoints_with_unclaimed_body_bytes() {
    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(b"topol_ref_data\0\x07\x00\x04\x01\xf6");
    payload.extend([0x0f; 8]);
    payload.push(0xff);
    payload.extend_from_slice(b"\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.curves.parameters.len(), 1);
    assert_eq!(scan.curves.parameters[0].scalar_tokens.len(), 8);
    assert_eq!(scan.curves.parameters[0].opaque_spans[0].raw, [0xff]);
    assert!(scan.curves.pcurves.is_empty());
}

#[test]
fn scan_decodes_fc_curve_world_coordinate_lane() {
    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(b"topol_ref_data\0\x07\x09\x04\x01\xf6\xfc\x08");
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x2d, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x46, 0, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x2d, 0, 0, 0, 0, 0, 0, 0, 0xff]);
    payload.extend_from_slice(b"\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.curves.fc_coordinates.len(), 1);
    let coordinates = &scan.curves.fc_coordinates[0];
    assert_eq!(coordinates.curve_id, 7);
    assert_eq!(coordinates.subtype, 8);
    assert_eq!(coordinates.body, scan.curves.parameters[0].body);
    assert_eq!(coordinates.values_mm, vec![3.0, -3.0, 2.0, -2.0]);
    assert_eq!(coordinates.tokens[0].offset, 2);
    assert_eq!(coordinates.tokens[0].length, 8);
    assert_eq!(coordinates.tokens[0].raw, [0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    assert_eq!(coordinates.tokens[1].offset, 10);
    assert_eq!(coordinates.opaque_spans[0].offset, 0);
    assert_eq!(coordinates.opaque_spans[0].raw, [0xfc, 0x08]);
    assert_eq!(coordinates.opaque_spans[1].raw, [0xff]);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let records = &result.ir().native.namespace("creo").unwrap().arenas["fc_curve_coordinates"];
    assert_eq!(records[0].fields()["curve_id"], 7);
    assert_eq!(records[0].fields()["values_mm"][1], -3.0);
    assert_eq!(records[0].fields()["tokens"][1]["offset"], 10);
    assert_eq!(records[0].fields()["tokens"][1]["length"], 8);
    assert_eq!(records[0].fields()["opaque_spans"][1]["raw"][0], 0xff);
}

#[test]
fn scan_validates_fc05_circle_from_record_points() {
    fn world(payload: &mut Vec<u8>, value: f64) {
        let raw = value.to_be_bytes();
        payload.push(match raw[0] {
            0x40 => 0x46,
            0xc0 => 0x2d,
            _ => panic!("generated FC05 value must use a world-token exponent"),
        });
        payload.extend_from_slice(&raw[1..]);
    }

    let mut payload = visibgeom_payload(0, 1);
    payload.extend_from_slice(b"topol_ref_data\0\x07\x09\x04\x01\xf6\xfc\x05");
    for [x, z, t, y] in [
        [4.0, 3.0, 2.0, 2.0],
        [3.0, 4.0, 2.0 + std::f64::consts::FRAC_PI_2, 2.0],
        [2.0, 3.0, 2.0 + std::f64::consts::PI, 2.0],
        [3.0, 2.0, 2.0 + 3.0 * std::f64::consts::FRAC_PI_2, 2.0],
    ] {
        world(&mut payload, x);
        world(&mut payload, z);
        world(&mut payload, t);
        world(&mut payload, y);
    }
    payload.push(0xff);
    payload.extend_from_slice(b"\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.curves.fc05_circles.len(), 1);
    let circle = &scan.curves.fc05_circles[0];
    assert_eq!(circle.curve_id, 7);
    assert_eq!(circle.center_row_frame, [3.0, 3.0]);
    assert_eq!(circle.radius_mm, 1.0);
    assert_eq!(circle.cap_ordinate_row_frame, Some(2.0));
    assert_eq!(circle.point_count, 4);
    assert_eq!(circle.max_residual, 0.0);
    assert!(circle.angle_parameter_consistent);
    assert_eq!(circle.parameter_sign, Some(1));
    let direction = circle
        .reference_direction_row_frame
        .expect("unique parameter-zero direction");
    assert!((direction[0] - (-2.0_f64).cos()).abs() < 1e-12);
    assert!((direction[1] - (-2.0_f64).sin()).abs() < 1e-12);
    let mut unknown_parameter = scan.curves.parameters[0].clone();
    unknown_parameter.body.splice(114..122, [0x39, 0x29, 0x00]);
    let carriers = crate::curve::fc05_circles(&[unknown_parameter]);
    let [carrier] = carriers.as_slice() else {
        panic!("circle geometry is independent of an unresolved parameter token");
    };
    assert_eq!(carrier.center_row_frame, [3.0, 3.0]);
    assert_eq!(carrier.radius_mm, 1.0);
    assert!(!carrier.angle_parameter_consistent);
    assert_eq!(carrier.parameter_sign, None);
    assert_eq!(carrier.reference_direction_row_frame, None);
    assert_eq!(carrier.sample_direction_row_frame, [1.0, 0.0]);
    let mut trailing = scan.curves.parameters[0].clone();
    trailing.body.push(0xfe);
    assert!(crate::curve::fc05_circles(&[trailing]).is_empty());
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let records = &result.ir().native.namespace("creo").unwrap().arenas["fc05_circles"];
    assert_eq!(records[0].fields()["curve_id"], 7);
    assert_eq!(records[0].fields()["radius_mm"], 1.0);
    assert_eq!(records[0].fields()["sample_direction_row_frame"][0], 1.0);
    assert_eq!(records[0].fields()["parameter_sign"], 1);
}

#[test]
fn scan_decodes_labeled_prototype_pcurve_uvs() {
    let mut payload = visibgeom_payload(0, 0);
    payload.extend_from_slice(b"crv_id\0\x2c type\0\x00 crv_pnt_arr\0\xf9\x02\x04");
    payload.extend_from_slice(&[0x12, 0xe4]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x12, 0xe4, 0x12]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0xe4]);
    payload.extend_from_slice(b"topol_ref_data\0");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.curves.prototype_pcurves.len(), 1);
    let prototype = &scan.curves.prototype_pcurves[0];
    assert_eq!(prototype.curve_id, 44);
    assert_eq!(prototype.face_0_endpoints, [[0.0, 1.0], [1.0, 0.0]]);
    assert_eq!(prototype.face_1_endpoints, [[3.0, 0.0], [3.0, 1.0]]);
}

#[test]
fn scan_withholds_non_exact_labeled_prototype_pcurve_arrays() {
    for tail in [
        vec![0xff],
        vec![0xe4, 0x12],
        vec![0x18, 0xe7, 0x04, 0x2f, 0x08, 0x00, 0xe4, 0x18],
    ] {
        let mut payload = visibgeom_payload(0, 0);
        payload.extend_from_slice(b"crv_id\0\x2c type\0\x00 crv_pnt_arr\0\xf9\x02\x04");
        payload.extend_from_slice(&[0x12, 0xe4]);
        payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
        payload.extend_from_slice(&[0x12, 0xe4, 0x12]);
        payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
        payload.extend_from_slice(&tail);
        payload.extend_from_slice(b"topol_ref_data\0");
        let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

        assert!(scan.curves.prototype_pcurves.is_empty());
    }
}

#[test]
fn scan_withholds_displaced_labeled_prototype_pcurve_wrapper() {
    let mut payload = visibgeom_payload(0, 0);
    payload.extend_from_slice(b"crv_id\0\x2c type\0\x00 crv_pnt_arr\0junk\xf9\x02\x04");
    payload.extend_from_slice(&[0x12, 0xe4]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x12, 0xe4, 0x12]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0xe4]);
    payload.extend_from_slice(b"topol_ref_data\0");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert!(scan.curves.prototype_pcurves.is_empty());
}

#[test]
fn scan_withholds_duplicate_labeled_prototype_pcurve_arrays() {
    let mut payload = visibgeom_payload(0, 0);
    payload.extend_from_slice(b"crv_id\0\x2c type\0\x00 crv_pnt_arr\0\xf9\x02\x04");
    payload.extend([0x0f; 8]);
    payload.extend_from_slice(b"crv_pnt_arr\0\xf9\x02\x04");
    payload.extend([0x0f; 8]);
    payload.extend_from_slice(b"topol_ref_data\0");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert!(scan.curves.prototype_pcurves.is_empty());
}

#[test]
fn scan_decodes_and_binds_labeled_prototype_topology() {
    let mut payload = visibgeom_payload(0, 0);
    payload.extend_from_slice(b"crv_id\0\x2c type\0\x00");
    payload.extend_from_slice(b"crv_hdr_geom_ptr[0]\0\x0a crv_hdr_geom_ptr[1]\0\x0b");
    payload.extend_from_slice(b"next_crv_hdr_ptr[0]\0\x2c next_crv_hdr_ptr[1]\0\x2c");
    payload.extend_from_slice(b"crv_pnt_arr\0\xf9\x02\x04");
    payload.extend_from_slice(&[0x0f, 0xe4]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0x0f, 0xe4, 0x0f]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    payload.push(0xe4);
    payload.extend_from_slice(b"topol_ref_data\0");
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes(data.clone());

    assert_eq!(scan.curves.prototype_topology.len(), 1);
    assert_eq!(scan.curves.prototype_topology[0].curve_id, 44);
    assert_eq!(scan.curves.prototype_topology[0].faces, [10, 11]);
    assert_eq!(scan.curves.prototype_topology[0].next_edges, [44, 44]);
    assert_eq!(scan.curves.bound_prototype_pcurves.len(), 1);
    assert_eq!(scan.curves.bound_prototype_pcurves[0].faces, [10, 11]);
    assert_eq!(
        scan.curves.bound_prototype_pcurves[0].face_0_endpoints,
        [[0.0, 1.0], [1.0, 0.0]]
    );
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let namespace = result.ir().native.namespace("creo").unwrap();
    assert_eq!(
        namespace.arenas["prototype_pcurves"][0].fields()["curve_id"],
        44
    );
    assert_eq!(
        namespace.arenas["curve_prototype_topology"][0].fields()["faces"][1],
        11
    );
}

#[test]
fn scan_withholds_duplicate_labeled_prototype_topology_fields() {
    let mut payload = visibgeom_payload(0, 0);
    payload.extend_from_slice(b"crv_id\0\x2c type\0\x00");
    payload.extend_from_slice(
        b"crv_hdr_geom_ptr[0]\0\x0a crv_hdr_geom_ptr[0]\0\x0a \
          crv_hdr_geom_ptr[1]\0\x0b next_crv_hdr_ptr[0]\0\x2c next_crv_hdr_ptr[1]\0\x2c",
    );
    payload.extend_from_slice(b"topol_ref_data\0");
    let scan = container::scan_bytes(build_prt("c", &[("VisibGeom", payload)]));

    assert!(scan.curves.prototype_topology.is_empty());
}

#[test]
fn prototype_pcurve_binding_requires_unique_native_identity() {
    let pcurve = crate::curve::PrototypePcurveEndpoints {
        curve_id: 44,
        face_0_endpoints: [[0.0, 1.0], [1.0, 0.0]],
        face_1_endpoints: [[3.0, 0.0], [3.0, 1.0]],
        offset: 10,
    };
    let topology = crate::curve::CurvePrototypeTopology {
        curve_id: 44,
        faces: [10, 11],
        next_edges: [44, 44],
        offset: 20,
    };

    assert!(crate::curve::bind_prototype_pcurves(
        &[pcurve.clone(), pcurve.clone()],
        std::slice::from_ref(&topology),
    )
    .is_empty());
    assert!(crate::curve::bind_prototype_pcurves(
        std::slice::from_ref(&pcurve),
        &[topology.clone(), topology],
    )
    .is_empty());
}
