// SPDX-License-Identifier: Apache-2.0
use super::super::*;
use crate::test_support::test_a5a8::a5_freeform_curve_stream;
use crate::wire::bytes::AllocationReferenceEncoding;
use crate::wire::records::consolidated_records;

fn jet(radius: f64, second_rail_error: f64) -> Vec<u8> {
    let mut bytes = a5_freeform_curve_stream();
    for site in 0_u16..2 {
        let start = 8 + 4 + 16 + usize::from(site) * 80;
        for (lane, value) in [
            (0, radius),
            (4, radius + second_rail_error),
            (2, f64::from(site)),
            (5, f64::from(site)),
            (8, f64::from(site)),
        ] {
            bytes[start + lane * 8..start + (lane + 1) * 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}
fn frame(bytes: &mut Vec<u8>, class: u8, payload: &[u8]) {
    bytes.extend([
        0xb2,
        3,
        class,
        u8::try_from(payload.len()).expect("short payload"),
        5,
    ]);
    bytes.extend(payload);
}
fn construction(bytes: &mut Vec<u8>, reference: u8, radius: f64) {
    let mut p = vec![0x85, reference, 5, 5, 5, 5, 0x11, 5];
    p.extend(radius.to_le_bytes());
    p.extend([3, 5]);
    p.extend([0; 8]);
    p.extend([1, 5]);
    frame(bytes, 0x37, &p);
}
fn with_carrier<T>(bytes: &[u8], index: usize, run: impl FnOnce(Carrier<'_>) -> T) -> Option<T> {
    let records = consolidated_records(bytes);
    let source = Source {
        bytes,
        records: &records,
        resolve: |index: usize, reference: AllocationReference| {
            (reference.encoding == AllocationReferenceEncoding::BackwardDistance)
                .then(|| index.checked_sub(usize::try_from(reference.value).ok()?))
                .flatten()
        },
        physical: Some,
        tensors: &BTreeMap::new(),
    };
    source
        .carrier(
            &CadIr::empty(),
            index,
            &BTreeMap::new(),
            &mut LaneRefusals::new(),
        )
        .map(run)
}
#[test]
fn cropped_rolling_results_keep_the_native_transverse_parameterization() {
    for v in [[0.0, 2.0], [0.0, 1.5], [0.5, 1.5], [1.0, 1.5]] {
        let mut bytes = jet(0.75, 0.0);
        bytes.extend(jet(1.0, 0.0));
        construction(&mut bytes, 9, 0.75);
        let mut p = vec![9, 5];
        p.extend(0.25_f64.to_le_bytes());
        p.push(0x19);
        for f in [0.0_f64, 1.0, v[0], v[1]] {
            p.extend(f.to_le_bytes());
        }
        frame(&mut bytes, 0x30, &p);
        with_carrier(&bytes, 3, |carrier| {
            assert_eq!(carrier.bounds(), Some([[0.0, 1.0], v]));
            let point = carrier
                .evaluate(carrier.point([0.5, 1.0]))
                .expect("native midpoint");
            let r = std::f64::consts::FRAC_1_SQRT_2;
            assert!(point.distance(Point3::new(r, r, 0.5)) < 1e-10);
        })
        .expect("cropped result construction");
    }
}
#[test]
fn rolling_result_bounds_stay_inside_the_native_chart() {
    let bytes = jet(1.0, 0.0);
    let records = consolidated_records(&bytes);
    let native = a5_owned_rolling_curves(&bytes, &records)
        .next()
        .expect("jet");
    for bounds in [
        [[0.0, 1.0], [-0.1, 1.5]],
        [[0.0, 1.0], [0.0, 2.1]],
        [[0.0, 1.0], [1.0, 0.5]],
        [[-0.1, 1.0], [0.0, 1.0]],
    ] {
        assert!(
            Carrier::rolling_ball_at_radius(native.clone(), 1.0, native.chart_v_end)
                .expect("carrier")
                .with_result_bounds(bounds)
                .is_none()
        );
    }
}
#[test]
fn construction_radius_corrections_reach_the_carrier_and_report_the_shift() {
    for (error, accepted) in [(0.000_001, true), (0.000_01, false)] {
        let mut bytes = jet(1.0, error);
        construction(&mut bytes, 5, 1.0);
        let result = with_carrier(&bytes, 1, |carrier| {
            let point = carrier
                .evaluate(carrier.point([0.5, 1.0]))
                .expect("corrected section");
            let r = std::f64::consts::FRAC_1_SQRT_2;
            assert!(point.distance(Point3::new(r, r, 0.5)) < 1e-10);
            let Some(super::super::super::SheetSurfaceFit::RollingBall { max_rail_shift_mm }) =
                carrier.surface_fit()
            else {
                panic!("rolling fit");
            };
            assert!((max_rail_shift_mm - error).abs() < 1e-10);
        });
        assert_eq!(result.is_some(), accepted);
    }
}
