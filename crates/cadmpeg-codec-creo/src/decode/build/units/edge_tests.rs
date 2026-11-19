use super::*;
use cadmpeg_ir::features::{SheetMetalFlangeHeight, SheetMetalFlangeWidth};
use serde_json::json;

#[test]
fn edge_partial_lengths_scale_independently_and_keep_zero_radius() {
    let scale = PositiveReal::new(1000.0).expect("positive scale");
    for (wire, expected) in [
        (
            json!({"kind":"symmetric_per_edge","widths":[{"width":3.0},{}]}),
            json!({"kind":"symmetric_per_edge","widths":[{"width":3000.0},{}]}),
        ),
        (
            json!({"kind":"two_sides_per_edge","widths":[{"first":2.0},{"second":4.0}]}),
            json!({"kind":"two_sides_per_edge","widths":[{"first":2000.0},{"second":4000.0}]}),
        ),
        (
            json!({"kind":"two_offsets_per_edge","offsets":[{"first":0.0,"second":-7.5}]}),
            json!({"kind":"two_offsets_per_edge","offsets":[{"first":0.0,"second":-7500.0}]}),
        ),
    ] {
        let mut width: SheetMetalFlangeWidth = serde_json::from_value(wire).expect("partial width");
        scale_sheet_metal_flange_width(&mut width, scale).expect("scale widths");
        assert_eq!(
            serde_json::to_value(width).expect("scaled width wire"),
            expected
        );
    }
    for (wire, expected) in [
        (
            json!({"kind":"to_object","value":{"target":{"kind":"native","value":"test:plane"}}}),
            json!({"kind":"to_object","value":{"target":{"kind":"native","value":"test:plane"}}}),
        ),
        (
            json!({"kind":"to_object","value":{"offset":-2.0}}),
            json!({"kind":"to_object","value":{"offset":-2000.0}}),
        ),
    ] {
        let mut height: SheetMetalFlangeHeight =
            serde_json::from_value(wire).expect("partial height");
        scale_sheet_metal_flange_height(&mut height, scale).expect("scale height");
        assert_eq!(
            serde_json::to_value(height).expect("scaled height wire"),
            expected
        );
    }
    let mut radius = cadmpeg_ir::scalar::NonNegativeLength::new(0.0).expect("zero radius");
    scale_nonnegative_length(&mut radius, scale).expect("scale zero radius");
    assert_eq!(radius.get(), 0.0);
}
