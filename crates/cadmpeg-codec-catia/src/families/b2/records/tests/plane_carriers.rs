use crate::families::b2::records::{b2_plane_carriers, b2_plane_geometry, B2PlaneCarrierPayload};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};

fn stream(selector: u8, values: &[f64], width: u8, flag: u8) -> Vec<u8> {
    let payload_len = u8::try_from(2 + 8 * values.len()).expect("bounded fixture lane");
    let mut bytes = vec![0xb1 + width, flag, 0x27, payload_len, 0x05];
    bytes.extend(std::iter::repeat_n(0, usize::from(width - 1)));
    bytes.extend([0xb4, selector]);
    for value in values {
        bytes.extend(value.to_le_bytes());
    }
    bytes
}

#[test]
fn five_plane_layouts_preserve_points_directions_and_bounds_at_every_header_width() {
    let cases: &[(u8, &[f64], [f64; 3])] = &[
        (0xec, &[10.0, 20.0, -2.0, 5.0, -2.0, 3.0], [1.0, 0.0, 0.0]),
        (
            0xe4,
            &[10.0, 20.0, -1.0, -2.0, 5.0, -2.0, 3.0],
            [-1.0, 0.0, 0.0],
        ),
        (
            0xc4,
            &[10.0, 20.0, 0.0, -1.0, -2.0, 5.0, -2.0, 3.0],
            [0.0, -1.0, 0.0],
        ),
        (0xdc, &[10.0, 20.0, -2.0, 5.0, -2.0, 3.0], [0.0, 1.0, 0.0]),
        (
            0xd4,
            &[10.0, 20.0, -1.0, -2.0, 5.0, -2.0, 3.0],
            [0.0, -1.0, 0.0],
        ),
    ];
    for &(selector, values, expected_direction) in cases {
        for width in 1..=3 {
            for flag in [0x03, 0x13, 0x83] {
                let bytes = stream(selector, values, width, flag);
                let carriers = b2_plane_carriers(&bytes);
                let [carrier] = carriers.as_slice() else {
                    panic!("one complete plane chart")
                };
                assert_eq!(carrier.end, bytes.len());
                assert_eq!(u8::from(carrier.width), width);
                assert_eq!(u8::from(carrier.flag), flag);
                assert_eq!(carrier.payload.selector(), selector);
                assert!(matches!(&carrier.payload, B2PlaneCarrierPayload::Plane {
                    point, direction, u_range, v_range, ..
                } if *point == [10.0, 20.0] && direction.get() == expected_direction
                    && u_range.endpoints() == [-2.0, 5.0] && v_range.endpoints() == [-2.0, 3.0]));
                let Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane))) =
                    b2_plane_geometry(carrier)
                else {
                    panic!("known plane geometry")
                };
                assert_eq!(
                    *plane.origin(),
                    cadmpeg_ir::math::Point3::new(10.0, 20.0, 0.0)
                );
                assert_eq!(
                    <[f64; 3]>::from(*plane.frame().reference().as_raw()),
                    expected_direction
                );
                assert_eq!(
                    <[f64; 3]>::from(*plane.frame().binormal().as_raw()),
                    [0.0, 0.0, 1.0]
                );
            }
        }
    }
}

#[test]
fn plane_layouts_require_unit_directions_and_ordered_parameter_rectangles() {
    for (selector, values) in [
        (0xec, vec![10.0, 20.0, 5.0, -2.0, -2.0, 3.0]),
        (0xdc, vec![10.0, 20.0, -2.0, 5.0, 3.0, -2.0]),
        (0xe4, vec![10.0, 20.0, 2.0, -2.0, 5.0, -2.0, 3.0]),
        (0xd4, vec![10.0, 20.0, 0.0, -2.0, 5.0, -2.0, 3.0]),
        (0xc4, vec![10.0, 20.0, 1.0, 1.0, -2.0, 5.0, -2.0, 3.0]),
        (0xe4, vec![10.0, 20.0, 1.0, 0.0, -2.0, 5.0, -2.0, 3.0]),
    ] {
        assert!(b2_plane_carriers(&stream(selector, &values, 1, 3)).is_empty());
    }
}
