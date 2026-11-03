// SPDX-License-Identifier: Apache-2.0

use cadmpeg_test_support::wire;

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::CreoLossCode;
use crate::CreoCodec;

#[test]
fn parallel_carriers_report_zero_solution_samples() {
    let mut payload = b"srf_array\0\xf8\x04".to_vec();
    for (surface, height) in [(1, 0.0), (2, 1.0), (3, 2.0), (4, 4.0)] {
        crate::test_support::push_generated_plane_row(
            &mut payload,
            surface,
            false,
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, height],
        );
    }
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\x06topol_ref_data\0");
    for (curve, faces, next) in [
        (10, [1, 2], [12, 13]),
        (11, [1, 3], [10, 15]),
        (12, [1, 4], [11, 14]),
        (13, [2, 3], [14, 11]),
        (14, [2, 4], [10, 15]),
        (15, [3, 4], [13, 12]),
    ] {
        crate::test_support::push_generated_topology_row(&mut payload, curve, faces, next);
    }
    let bytes = crate::test_support::build_prt("synthetic", &[("VisibGeom", payload)]);
    let result = CreoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("synthetic carrier decode");
    let coverage = wire::field_or_default::<std::collections::BTreeMap<String, usize>>(
        result.report(),
        "coverage",
    );
    assert!(coverage
        .get("brep_vertex_carrier_zero_candidate_count")
        .is_some_and(|count| *count > 0));
    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == CreoLossCode::BrepTransferIncomplete.kind())
        .expect("B-rep rejection loss");
    let (_, samples) = loss
        .message
        .split_once("rejection samples: ")
        .expect("carrier rejection samples");
    assert!(samples.contains("[faces:"));
    assert!(samples.contains(";carriers:"));
    assert!(samples.contains(";unique:0]"));
}

fn compact_plane_part(origin: &[u8]) -> Vec<u8> {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0, 0xe3]);
    payload.extend_from_slice(b"srf_prim_ptr(plane)\0\xe0\x02local_sys\0\xf9\x04\x03");
    for _ in 0..2 {
        payload.extend_from_slice(&[0x0f, 0x18, 0xe5]);
    }
    payload.push(0x0f);
    payload.extend_from_slice(origin);
    payload.extend_from_slice(b"\xe3crv_array\0\xf3\xf8\0");
    crate::test_support::build_prt("compact-plane", &[("ND:0:VisibGeom:0", payload)])
}

#[test]
fn compact_plane_decode_and_validation_use_the_same_finite_frame() {
    let bytes = compact_plane_part(&[0x2f, 0x00, 0x00, 0x2f, 0x10, 0x00, 0x2f, 0x20, 0x00]);
    let result = CreoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode compact plane");
    let plane = result
        .ir()
        .model
        .surfaces
        .iter()
        .find_map(|surface| {
            if surface.id.as_str() != "creo:visibgeom:surface#7" {
                return None;
            }
            match &surface.geometry {
                cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                    cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(plane),
                ) => Some(plane),
                _ => None,
            }
        })
        .expect("compact plane carrier");
    assert_eq!(<[f64; 3]>::from(plane.origin().get()), [2.0, 4.0, 8.0]);
    assert_eq!(
        <[f64; 3]>::from(*plane.frame().axis().as_raw()),
        [0.0, 0.0, 1.0]
    );
    assert_eq!(
        <[f64; 3]>::from(*plane.frame().reference().as_raw()),
        [1.0, 0.0, 0.0]
    );
    let encoded = serde_json::to_value(result.ir()).expect("serialize decoded frame");
    let roundtrip: cadmpeg_ir::document::CadIr =
        serde_json::from_value(encoded).expect("validate serialized frame");
    let report = cadmpeg_ir::validate_neutral(&roundtrip, result.report().losses.clone())
        .expect("validate decoded frame");
    assert!(report.is_ok(), "{report:?}");
}

#[test]
fn compact_plane_missing_origin_keeps_the_specific_loss() {
    let result = CreoCodec
        .decode(
            &mut Cursor::new(compact_plane_part(&[0x2f, 0x00])),
            &DecodeOptions::default(),
        )
        .expect("retain incomplete plane");
    assert!(!result.ir().model.surfaces.iter().any(|surface| {
        surface.id.as_str() == "creo:visibgeom:surface#7"
            && matches!(
                surface.geometry,
                cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                    cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(_)
                )
            )
    }));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == CreoLossCode::PlaneCompactFrameUnresolved.kind()));
}
