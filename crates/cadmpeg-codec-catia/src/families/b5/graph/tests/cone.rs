use crate::families::b5::graph::{parse_offset_surface, parse_surface, B5Record, B5Surface};
use std::collections::{BTreeMap, HashMap};

#[test]
fn cone_native_and_neutral_charts_preserve_reference_radius_and_signed_slant() {
    const EPS_CONE_CHART: f64 = 1.0e-12;
    let angle = 0.25_f64;
    let u = 0.8_f64;
    let v = -1.2_f64;
    for (handedness, shift) in [(-1.0_f64, 0.0_f64), (1.0, 0.0), (-1.0, 1.25), (1.0, 1.25)] {
        let values = [
            1.0,
            2.0,
            3.0,
            1.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            handedness,
            angle,
            4.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
            -2.0,
            8.0,
            3.0,
            1.0,
            shift,
            -3.0 * std::f64::consts::FRAC_PI_4,
            5.0 * std::f64::consts::FRAC_PI_4,
        ];
        let mut payload = vec![0x80];
        for value in values {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        let surface = parse_surface(&B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x29,
            object_id: 7,
            payload,
        })
        .expect("a finite cone with a negative slant start");
        let B5Surface::Cone {
            surface: Some(neutral),
            ..
        } = &surface
        else {
            panic!("the active interval has positive radii");
        };
        let radius = 4.0 + v * angle.cos();
        let expected = [
            1.0 + radius * (u / 3.0).cos(),
            2.0 + radius * (u / 3.0).sin(),
            3.0 + handedness * v * angle.sin(),
        ];
        crate::test_support::with_service_context(|ctx| {
            let native = crate::families::b5::graph::lift_pcurve_endpoints(
                ctx,
                &surface,
                &BTreeMap::new(),
                [[u, v + shift]; 2],
            )
            .expect("bounded lift")
            .expect("finite endpoints")[0];
            let uv = cadmpeg_ir::math::Point2::new(handedness * u / 3.0, (v + 2.0) * angle.sin());
            let neutral = cadmpeg_ir::eval::decode::surface_point_solved(
                ctx,
                &cadmpeg_ir::geometry::SolvedSurfaceGeometry::Cone(*neutral),
                uv.u,
                uv.v,
            )
            .expect("finite neutral cone evaluation");
            for point in [native, neutral] {
                let actual: [f64; 3] = point.get().into();
                assert!(actual
                    .into_iter()
                    .zip(expected)
                    .all(|(a, b)| (a - b).abs() < EPS_CONE_CHART));
            }
        });
    }
}

#[test]
fn cone_offset_uses_signed_normal_distance_and_generator_chart_translation() {
    let angle = 1.1_f64;
    let distance = -0.7_f64;
    let translation = 2.3_f64;
    let source_shift = 0.9_f64;
    for handedness in [-1.0_f64, 1.0] {
        let cone = |id, axial, radius, range: [f64; 2], shift| {
            let values = [
                0.0,
                0.0,
                handedness * axial,
                1.0,
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
                0.0,
                0.0,
                handedness,
                angle,
                radius,
                -std::f64::consts::FRAC_PI_2,
                std::f64::consts::FRAC_PI_2,
                range[0],
                range[1],
                3.0,
                1.0,
                shift,
                -std::f64::consts::PI,
                std::f64::consts::PI,
            ];
            let mut payload = vec![0x80];
            for value in values {
                payload.extend_from_slice(&value.to_le_bytes());
            }
            parse_surface(&B5Record {
                offset: 0,
                family: 0xb5,
                class: 0x29,
                object_id: id,
                payload,
            })
            .expect("the cone has finite positive active radii")
        };
        let source = cone(3, 0.0, 20.0, [3.0, 9.0], source_shift);
        let carrier = cone(
            2,
            translation * angle.sin() - handedness * distance * angle.cos(),
            20.0 + translation * angle.cos() + handedness * distance * angle.sin(),
            [3.0 - translation, 9.0 - translation],
            source_shift + translation,
        );
        let mut payload = vec![0x82, 0x82, 0x83];
        payload.extend_from_slice(&distance.to_le_bytes());
        payload.push(0x11);
        for value in [-1.5, 1.5, 3.0 + source_shift, 9.0 + source_shift] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x30,
            object_id: 9,
            payload,
        };
        let surfaces = BTreeMap::from([(2, carrier.clone()), (3, source.clone())]);
        let result = parse_offset_surface(&record, &surfaces, &BTreeMap::new(), &HashMap::new())
            .expect("signed normal offset with shifted local charts");
        assert_eq!(
            result.carrier_kind,
            crate::families::b5::graph::B5OffsetCarrierKind::Cone
        );
        let bounds = result.parameter_bounds;
        assert!(
            !crate::families::b5::graph::cone_offset_construction_agrees(
                &source, &source, 1e-200, bounds,
            )
        );
        assert!(
            !crate::families::b5::graph::cone_offset_construction_agrees(
                &source, &carrier, -distance, bounds
            )
        );
        let mut wrong_shift = carrier.clone();
        let B5Surface::Cone { slant_shift, .. } = &mut wrong_shift else {
            unreachable!();
        };
        *slant_shift = crate::test_support::test_b5::finite(source_shift + translation + 0.01);
        assert!(
            !crate::families::b5::graph::cone_offset_construction_agrees(
                &source,
                &wrong_shift,
                distance,
                bounds
            )
        );
        let wrong_bounds = [
            bounds[0],
            crate::test_support::test_b5::increasing([3.0 + source_shift, 9.01 + source_shift]),
        ];
        assert!(
            !crate::families::b5::graph::cone_offset_construction_agrees(
                &source,
                &carrier,
                distance,
                wrong_bounds
            )
        );
        let mut wrong_frame = carrier;
        let B5Surface::Cone { direction_y, .. } = &mut wrong_frame else {
            unreachable!();
        };
        *direction_y = direction_y.reversed();
        assert!(
            !crate::families::b5::graph::cone_offset_construction_agrees(
                &source,
                &wrong_frame,
                distance,
                bounds
            )
        );
    }
}

#[test]
fn cone_offset_gate_matches_the_native_negative_axis_normal() {
    let source_values = [
        -243.199_999_999_999_96,
        93.709_812_562_329_09,
        15.700_000_000_000_003,
        1.0,
        0.0,
        0.0,
        -0.0,
        1.0,
        0.0,
        -0.0,
        0.0,
        -1.0,
        1.483_529_864_195_180_2,
        19.000_000_000_000_01,
        0.500_000_000_000_000_7,
        std::f64::consts::PI,
        0.0,
        61.122_110_572_506_76,
        19.000_000_000_000_01,
        1.0,
        0.0,
        -1.320_796_326_794_896_1,
        4.962_388_980_384_69,
    ];
    let result_values = [
        -243.199_999_999_999_96,
        93.709_812_562_329_09,
        108.790_314_339_615_12,
        1.0,
        0.0,
        0.0,
        -0.0,
        1.0,
        0.0,
        -0.0,
        0.0,
        -1.0,
        1.483_529_864_195_180_2,
        17.380_481_755_250_685,
        -1.320_796_326_794_896_1,
        4.962_388_980_384_69,
        0.0,
        146.603_122_942_962_98,
        19.000_000_000_000_01,
        1.0,
        -92.877_227_904_333_08,
        -1.320_796_326_794_896_3,
        4.962_388_980_384_69,
    ];
    let cone = |values: [f64; 23], id| {
        let mut payload = vec![0x80];
        for value in values {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        parse_surface(&B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x29,
            object_id: id,
            payload,
        })
        .expect("the complete native cone frame")
    };
    let source = cone(source_values, 3);
    let carrier = cone(result_values, 2);
    let bounds = [
        crate::test_support::test_b5::increasing([9.500_000_000_000_018, 34.595_130_209_103_06]),
        crate::test_support::test_b5::increasing([-92.877_227_904_333_08, 53.725_895_038_629_89]),
    ];
    assert!(crate::families::b5::graph::cone_offset_construction_agrees(
        &source, &carrier, -6.5, bounds
    ));
    assert!(
        !crate::families::b5::graph::cone_offset_construction_agrees(
            &source, &carrier, 6.5, bounds
        )
    );
}
