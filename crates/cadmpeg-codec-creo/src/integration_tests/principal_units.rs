// SPDX-License-Identifier: Apache-2.0

use super::{assert_valid, decode};
use crate::test_support::{build_prt, push_generated_plane_row};

#[test]
fn binary_units_cgs_lengths_scale_exactly_once() {
    for (selector, expected, unit, scale) in [
        (51_u8, [1.0, 2.0, 4.0], "mmNs", None),
        (50_u8, [10.0, 20.0, 40.0], "CGS", Some("10")),
    ] {
        let mut visible = b"srf_array\0\xf8\x01".to_vec();
        push_generated_plane_row(
            &mut visible,
            7,
            false,
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 2.0, 4.0],
        );
        let mut basic = b"_principal_sys_units_id\0".to_vec();
        basic.push(selector);
        let decoded = decode(build_prt(
            "unit-control",
            &[("BasicData", basic), ("VisibGeom", visible)],
        ));
        assert_valid(&decoded);
        let value = serde_json::to_value(decoded.ir()).expect("CADIR serializes");
        let surface = value["model"]["surfaces"]
            .as_array()
            .expect("surfaces")
            .iter()
            .find(|row| row["id"] == "creo:visibgeom:surface#7")
            .expect("plane");
        assert_eq!(surface["geometry"]["kind"], "plane");
        assert_eq!(
            surface["geometry"]["origin"],
            serde_json::json!({"x":expected[0], "y":expected[1], "z":expected[2]})
        );
        assert_eq!(
            surface["geometry"]["normal"],
            serde_json::json!({"x":0.0,"y":0.0,"z":1.0})
        );
        assert_eq!(
            surface["geometry"]["u_axis"],
            serde_json::json!({"x":1.0,"y":0.0,"z":0.0})
        );
        let attributes = &decoded.ir().source.as_ref().expect("Source").attributes;
        assert_eq!(
            attributes.get("principal_unit").map(String::as_str),
            Some(unit)
        );
        assert_eq!(
            attributes.get("source_length_scale_mm").map(String::as_str),
            scale
        );
    }
}
