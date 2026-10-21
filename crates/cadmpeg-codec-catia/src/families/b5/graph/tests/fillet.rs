use super::super::{
    parse_supported_surface, supported_surface_parameters_match_carrier, B5Record,
    B5SupportedSurfaceParameters, B5Surface,
};

#[test]
fn fillet_control_domains_and_positive_zero_are_required() {
    let mut payload = vec![0x85, 0x82, 0x83, 0x84, 0x85, 0x86, 0x09, 0x05];
    payload.extend_from_slice(&2.5f64.to_le_bytes());
    payload.extend_from_slice(&[0x03, 0x05]);
    payload.extend_from_slice(&0.0f64.to_le_bytes());
    payload.extend_from_slice(&[0x01, 0x05]);
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x37,
        object_id: 7,
        payload,
    };
    for (slot, invalid) in [
        (0, 0x15),
        (1, 0x01),
        (2, 0x01),
        (3, 0x01),
        (4, 0x09),
        (5, 0x81),
    ] {
        let mut invalid_control = record.clone();
        let offsets = [6, 7, 16, 17, 26, 27];
        invalid_control.payload[offsets[slot]] = invalid;
        assert!(
            parse_supported_surface(&invalid_control).is_none(),
            "control {slot}"
        );
    }
    let mut negative_zero = record.clone();
    negative_zero.payload[18..26].copy_from_slice(&(-0.0f64).to_le_bytes());
    assert!(parse_supported_surface(&negative_zero).is_none());
}

#[test]
fn fillet_sphere_closure_matches_contact_order_and_latitude() {
    let radius = 1e-200_f64;
    let sphere = |carrier_radius| B5Surface::Sphere {
        center: crate::test_support::test_b5::point([0.0; 3]),
        frame: crate::test_support::test_b5::frame([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        direction_y: crate::test_support::test_b5::unit([0.0, 1.0, 0.0]),
        radius: crate::test_support::test_b5::positive_length(1.0),
        azimuth_range: crate::test_support::test_b5::increasing([0.0, 1.0]),
        latitude_range: crate::test_support::test_b5::increasing([
            -1.0,
            std::f64::consts::FRAC_PI_2,
        ]),
        construction_radius: carrier_radius,
        chart_origin: crate::test_support::test_b5::finite(0.0),
    };

    for (order, closure, lower) in [
        (0x01, 0x11, -1.0),
        (0x05, 0x09, -1.0),
        (0x01, 0x81, -std::f64::consts::FRAC_PI_2),
        (0x05, 0x81, -std::f64::consts::FRAC_PI_2),
    ] {
        let mut sphere = sphere(crate::test_support::test_b5::positive_length(radius));
        if let B5Surface::Sphere { latitude_range, .. } = &mut sphere {
            *latitude_range =
                crate::test_support::test_b5::increasing([lower, std::f64::consts::FRAC_PI_2]);
        }
        let valid = B5SupportedSurfaceParameters::Radius {
            controls: [0x0d, 0x05, 0x03, 0x05, order, closure],
            construction_radius: crate::test_support::test_b5::positive_length(radius),
        };
        assert!(supported_surface_parameters_match_carrier(&valid, &sphere));
        let mut invalid = valid;
        if let B5SupportedSurfaceParameters::Radius { controls, .. } = &mut invalid {
            controls[5] = 0x05;
        }
        assert!(!supported_surface_parameters_match_carrier(
            &invalid, &sphere
        ));
    }
}
