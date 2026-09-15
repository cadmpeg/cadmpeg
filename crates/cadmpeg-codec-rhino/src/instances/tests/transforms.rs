// SPDX-License-Identifier: Apache-2.0
use crate::chunks::ArchiveVersion;
use crate::loss::RhinoLossCode;
use crate::test_support::test_dump as bytes;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::transform::Transform;
use std::io::Cursor;

fn object(kind: i64, class: [u8; 16], id: [u8; 16], payload: &[u8]) -> Vec<u8> {
    let archive = ArchiveVersion::V8;
    let mut attributes = bytes::tagged_attributes(&[], 0);
    attributes[1..17].copy_from_slice(&id);
    let body = [
        bytes::short_chunk(archive, 0x8200_0071, kind),
        bytes::class_wrapper(archive, class, payload),
        bytes::crc_chunk(archive, 0x0200_8072, &attributes),
        bytes::short_chunk(archive, 0x8200_007f, 0),
    ]
    .concat();
    bytes::nested_crc_chunk(archive, 0x2000_8070, &body)
}

fn document(transforms: &[[[f64; 4]; 4]]) -> Vec<u8> {
    let archive = ArchiveVersion::V8;
    let definition = bytes::definition_record(
        archive,
        &bytes::v6_definition_payload(archive, [0x51; 16], &[[0x61; 16]], 1, false, false),
    );
    let mut objects = vec![object(
        1,
        bytes::POINT_CLASS,
        [0x61; 16],
        &bytes::point_payload([1.0, 2.0, 3.0]),
    )];
    for (index, rows) in transforms.iter().enumerate() {
        objects.push(object(
            0x1000,
            bytes::INSTANCE_REFERENCE_CLASS,
            [0x70 + u8::try_from(index).unwrap(); 16],
            &bytes::instance_reference_payload([0x51; 16], *rows),
        ));
    }
    bytes::minimal_document(
        "80",
        &[
            bytes::table(archive, 0x1000_0014, &[]),
            bytes::table(archive, 0x1000_0015, &[bytes::units_record(archive, 2)]),
            bytes::table(archive, 0x1000_0021, &[definition]),
            bytes::table(archive, 0x1000_0013, &objects),
        ],
    )
}

#[test]
fn extreme_finite_instance_transforms_keep_native_occurrences_and_geometry() {
    for exponent in [-800, -600, 0, 600, 800] {
        let scale = 2.0_f64.powi(exponent);
        let rows = [
            [scale, 0.0, 0.0, 0.0],
            [0.0, scale, 0.0, 0.0],
            [0.0, 0.0, scale, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let source = document(&[rows]);
        let decoded = crate::RhinoCodec
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .unwrap();
        assert!(
            !decoded.report().losses.iter().any(|loss| [
                RhinoLossCode::ProductOccurrenceDropped.kind(),
                RhinoLossCode::IntegrityFailure.kind()
            ]
            .contains(&loss.code)),
            "exponent {exponent}: {:?}",
            decoded.report().losses
        );
        let reread = CadIr::from_json(&serde_json::to_string(decoded.ir()).unwrap()).unwrap();
        for ir in [decoded.ir(), &reread] {
            assert_eq!(ir.model.points.len(), 1, "exponent {exponent}");
            assert_eq!(
                ir.model.points[0].position,
                Point3::new(scale, 2.0 * scale, 3.0 * scale)
            );
            let native = serde_json::to_value(ir.native.namespace("rhino").unwrap()).unwrap();
            let occurrences = native["product_occurrences"].as_array().unwrap();
            assert_eq!(occurrences.len(), 1);
            assert_eq!(occurrences[0]["transform_units"], "millimeter");
            assert_eq!(
                occurrences[0]["transform"],
                serde_json::to_value(rows).unwrap()
            );
        }
    }
}

#[test]
fn invalid_instance_inverse_is_located_and_later_reference_survives() {
    for invalid in [
        [
            [0.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        [
            [f64::from_bits(1), 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        [
            [0.5, 0.0, 0.0, f64::MAX],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    ] {
        let source = document(&[invalid, Transform::identity().rows()]);
        let scan = crate::container::scan_owned(source.clone()).unwrap();
        let offset = scan.objects[1].range().start as u64;
        let decoded = crate::RhinoCodec
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .unwrap();
        let losses: Vec<_> = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == RhinoLossCode::ProductOccurrenceDropped.kind())
            .collect();
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].provenance.as_ref().unwrap().offset, offset);
        assert!(!decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == RhinoLossCode::IntegrityFailure.kind()));
        let ir = CadIr::from_json(&serde_json::to_string(decoded.ir()).unwrap()).unwrap();
        assert_eq!(ir.model.points.len(), 1);
        assert_eq!(ir.model.points[0].position, Point3::new(1.0, 2.0, 3.0));
        let native = serde_json::to_value(ir.native.namespace("rhino").unwrap()).unwrap();
        assert_eq!(native["product_occurrences"].as_array().unwrap().len(), 1);
        assert_eq!(
            native["product_occurrences"][0]["source_uuid"],
            "71717171-7171-7171-7171-717171717171"
        );
    }
}
