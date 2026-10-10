// SPDX-License-Identifier: Apache-2.0
//! Standalone topology follows IGES subordinate status.

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn dependent_curve_carriers_are_retained_without_standalone_wire_topology() {
    use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
    for (kind, parameters) in [
        (100, "100,0,0,0,1,0,0,1;"),
        (110, "110,2,3,0,4,3,0;"),
        (126, "126,1,1,0,0,1,0,0,0,1,1,1,1,0,0,0,1,0,0,0,1,0,0,1;"),
    ] {
        for status in ["00010000", "00030000"] {
            let bytes = owned_test_file(&[OwnedTestEntity {
                entity_type: kind,
                form: 0,
                label: "CURVE".into(),
                status,
                parameters: parameters.into(),
            }]);
            let decoded = crate::IgesCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .expect("dependent curve");
            assert_eq!(decoded.ir().model.curves.len(), 1);
            assert!(decoded.ir().model.bodies.is_empty());
            assert!(decoded.ir().model.edges.is_empty());
            assert!(decoded.ir().model.vertices.is_empty());
            assert!(decoded.ir().model.points.is_empty());
            let interval = decoded.ir().model.curves[0]
                .parameter_range
                .expect("finite carrier interval survives topology removal");
            let plan = crate::test_support::plan_at(crate::IgesVersion::V5_3, decoded.ir(), None)
                .expect("dependent carrier can be encoded");
            let mut written = Vec::new();
            plan.write_to(&mut written).unwrap();
            let recovered = crate::IgesCodec
                .decode(&mut Cursor::new(written), &DecodeOptions::default())
                .expect("dependent carrier round trip");
            assert!(recovered.ir().model.bodies.is_empty());
            assert_eq!(
                recovered.ir().model.curves[0].parameter_range,
                Some(interval)
            );
        }
    }
    let decoded = crate::test_support::decode(
        crate::test_support::test_procedural_surfaces::cacheless_line_tabulated_surface_file(),
    );
    assert!(
        !decoded.ir().model.curves.is_empty(),
        "surface support remains live"
    );
}

#[test]
fn consumed_boundary_carriers_keep_their_source_intervals() {
    let decoded = crate::test_support::decode(
        crate::test_support::test_surface_fixtures::trimmed_plane_file(),
    );
    assert_eq!(decoded.ir().model.curves.len(), 2);
    assert!(decoded
        .ir()
        .model
        .curves
        .iter()
        .all(|curve| curve.parameter_range
            == cadmpeg_ir::topology::IncreasingParameterInterval::new([0.0, 4.0])));
}

#[test]
fn logical_group_members_keep_their_physical_wire_geometry() {
    use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "MEMBER".into(),
            status: "00020000",
            parameters: "110,0,0,0,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 7,
            label: "GROUP".into(),
            status: "00000000",
            parameters: "402,1,1;".into(),
        },
    ]);
    let decoded = crate::IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("logically grouped wire");
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.edges.len(), 1);
    assert_eq!(decoded.ir().model.vertices.len(), 2);
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .expect("wire validation")
        .is_ok());
}
