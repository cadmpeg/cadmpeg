// SPDX-License-Identifier: Apache-2.0
//! Current geometry written through compatible IGES native records.

use serde_json::json;

#[test]
fn copious_linear_path_uses_current_curve_points() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    use std::io::Cursor;
    let harness =
        cadmpeg_test_support::golden::Harness::new(env!("CARGO_MANIFEST_DIR"), "igs", "unused");
    let (_, bytes) = harness
        .fixture_inputs()
        .into_iter()
        .find(|(name, _)| name == "copious_linear_path")
        .unwrap();
    let decoded = crate::IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let original = crate::writer::synthesize(decoded.ir(), crate::IgesVersion::V5_3).unwrap();
    assert_eq!(original.counts.get("106_copious_data"), Some(&1));
    let original_after = crate::IgesCodec
        .decode(&mut Cursor::new(original.bytes), &DecodeOptions::default())
        .unwrap();
    let before_record = &decoded.ir().native.namespace("iges").unwrap().arenas()["copious_data"][0];
    let after_record = &original_after
        .ir()
        .native
        .namespace("iges")
        .unwrap()
        .arenas()["copious_data"][0];
    assert_eq!(
        before_record.fields().unwrap(),
        after_record.fields().unwrap()
    );
    let mut ir = decoded.ir().clone();
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) = &mut ir.model.curves[0].geometry
    else {
        panic!("linear path projects to NURBS");
    };
    curve
        .try_map_control_points(|index, point| {
            let mut point = point.get();
            if index == 1 {
                point.y = 0.5;
            }
            cadmpeg_ir::features::FinitePoint3::new(point).ok_or(())
        })
        .unwrap();
    let output = crate::writer::synthesize(&ir, crate::IgesVersion::V5_3).unwrap();
    assert_eq!(output.counts.get("106_copious_data"), Some(&1));
    assert!(output
        .regenerated_native
        .contains("iges:native:copious-data#D1"));
    let after = crate::IgesCodec
        .decode(&mut Cursor::new(output.bytes), &DecodeOptions::default())
        .unwrap();
    let record = &after.ir().native.namespace("iges").unwrap().arenas()["copious_data"][0];
    assert_eq!(record.field("tuples").unwrap()[1][1], json!(0.5));
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) =
        &after.ir().model.curves[0].geometry
    else {
        panic!("written path projects to NURBS");
    };
    assert_eq!(curve.control_points()[1].get().y, 0.5);
}

#[test]
fn copious_linear_path_retains_typed_vectors() {
    use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 106,
        form: 13,
        label: "PATH".into(),
        status: "00000000",
        parameters: "106,3,2,0,0,0,1,0,0,2,0,0,0,1,0;".into(),
    }]);
    let decoded = crate::IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let before = &decoded.ir().native.namespace("iges").unwrap().arenas()["copious_data"][0];
    let output = crate::writer::synthesize(decoded.ir(), crate::IgesVersion::V5_3).unwrap();
    assert_eq!(output.counts.get("106_copious_data"), Some(&1));
    let after = crate::IgesCodec
        .decode(&mut Cursor::new(output.bytes), &DecodeOptions::default())
        .unwrap();
    let record = &after.ir().native.namespace("iges").unwrap().arenas()["copious_data"][0];
    assert_eq!(record.field("tuples"), before.field("tuples"));
}

#[test]
fn canonical_type_108_plane_retains_its_directory_fields() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;
    let harness =
        cadmpeg_test_support::golden::Harness::new(env!("CARGO_MANIFEST_DIR"), "igs", "unused");
    let (_, bytes) = harness
        .fixture_inputs()
        .into_iter()
        .find(|(name, _)| name == "plane")
        .unwrap();
    let decoded = crate::IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let original = &decoded.ir().native.namespace("iges").unwrap().arenas()["entities"][0];
    let output = crate::writer::synthesize(decoded.ir(), crate::IgesVersion::V5_3).unwrap();
    assert_eq!(output.counts.get("108_plane"), Some(&1));
    let after = crate::IgesCodec
        .decode(&mut Cursor::new(output.bytes), &DecodeOptions::default())
        .unwrap();
    let regenerated = &after.ir().native.namespace("iges").unwrap().arenas()["entities"][0];
    assert_eq!(original.fields().unwrap(), regenerated.fields().unwrap());
    assert_eq!(
        decoded.ir().model.surfaces[0].geometry,
        after.ir().model.surfaces[0].geometry
    );
}
