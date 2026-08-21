// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use cadmpeg_ir::geometry::{CurveGeometry, PcurveGeometry, SurfaceGeometry};

use crate::test_support::*;

#[test]
fn nurbs_carriers_reject_nonfinite_millimeter_control_points() {
    let mut surface = bspline_partition_stream();
    let payload = surface
        .windows(4)
        .position(|window| window == [0, 125, 0, 21])
        .expect("surface payload");
    put_f64(&mut surface, payload + 97, f64::MAX);
    assert!(crate::nurbs::surfaces(&surface).is_empty());

    let mut curve = bspline_partition_stream();
    let payload = curve
        .windows(4)
        .position(|window| window == [0, 135, 0, 41])
        .expect("curve payload");
    put_f64(&mut curve, payload + 15, f64::MAX);
    assert!(crate::nurbs::curves(&curve).is_empty());

    let descriptor = curve
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("curve descriptor");
    put_ref(&mut curve, descriptor + 10, 2);
    put_f64(&mut curve, payload + 15, f64::MAX);
    put_f64(&mut curve, payload + 31, f64::MIN_POSITIVE);
    assert!(crate::nurbs::pcurves(&curve).is_empty());
}

#[test]
fn nurbs_periodicity_uses_logical_flags_not_knot_types() {
    let mut surface = bspline_partition_stream();
    let surface_descriptor = surface
        .windows(4)
        .position(|window| window == [0, 126, 0, 20])
        .expect("surface descriptor");
    surface[surface_descriptor + 4] = 1;
    surface[surface_descriptor + 5] = 0;
    surface[surface_descriptor + 18] = 2;
    surface[surface_descriptor + 19] = 3;
    let [surface] = crate::nurbs::surfaces(&surface)
        .try_into()
        .expect("one surface");
    let SurfaceGeometry::Nurbs(surface) = surface.geometry else {
        panic!("expected NURBS surface");
    };
    assert!(surface.u_periodic);
    assert!(!surface.v_periodic);

    let mut open_surface = bspline_partition_stream();
    let surface_descriptor = open_surface
        .windows(4)
        .position(|window| window == [0, 126, 0, 20])
        .expect("surface descriptor");
    open_surface[surface_descriptor + 4] = 0;
    open_surface[surface_descriptor + 18] = 6;
    let [open_surface] = crate::nurbs::surfaces(&open_surface)
        .try_into()
        .expect("one surface");
    let SurfaceGeometry::Nurbs(open_surface) = open_surface.geometry else {
        panic!("expected NURBS surface");
    };
    assert!(!open_surface.u_periodic);

    let mut curve = bspline_partition_stream();
    let curve_descriptor = curve
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("curve descriptor");
    curve[curve_descriptor + 16] = 2;
    curve[curve_descriptor + 17] = 1;
    let [curve] = crate::nurbs::curves(&curve).try_into().expect("one curve");
    let CurveGeometry::Nurbs(curve) = curve.geometry else {
        panic!("expected NURBS curve");
    };
    assert!(curve.periodic);

    let mut open_curve = bspline_partition_stream();
    let curve_descriptor = open_curve
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("curve descriptor");
    open_curve[curve_descriptor + 16] = 6;
    open_curve[curve_descriptor + 17] = 0;
    let [open_curve] = crate::nurbs::curves(&open_curve)
        .try_into()
        .expect("one curve");
    let CurveGeometry::Nurbs(open_curve) = open_curve.geometry else {
        panic!("expected NURBS curve");
    };
    assert!(!open_curve.periodic);

    let mut pcurve = bspline_partition_stream();
    let pcurve_descriptor = pcurve
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("pcurve descriptor");
    put_ref(&mut pcurve, pcurve_descriptor + 10, 2);
    pcurve[pcurve_descriptor + 16] = 6;
    pcurve[pcurve_descriptor + 17] = 0;
    let payload = pcurve
        .windows(4)
        .position(|window| window == [0, 135, 0, 41])
        .expect("pcurve payload");
    for (index, value) in [0.0, 0.0, 1.0, 0.02, 0.0, 1.0].into_iter().enumerate() {
        put_f64(&mut pcurve, payload + 15 + index * 8, value);
    }
    let [pcurve] = crate::nurbs::pcurves(&pcurve)
        .try_into()
        .expect("one pcurve");
    let PcurveGeometry::Nurbs { periodic, .. } = pcurve.geometry else {
        panic!("expected NURBS pcurve");
    };
    assert!(!periodic);
}

#[test]
fn nurbs_accepts_encoded_cardinality_without_arbitrary_ceiling() {
    fn curve_stream(degree: u16, poles: u16) -> Vec<u8> {
        assert!(poles > degree);
        let distinct = usize::from(poles) + 1;
        let mut stream = Vec::new();

        let mut wrapper = record(134, 23);
        put_ref(&mut wrapper, 2, 50);
        wrapper[18] = b'+';
        put_ref(&mut wrapper, 19, 40);
        put_ref(&mut wrapper, 21, 41);
        stream.extend(wrapper);

        let mut descriptor = record(136, 27);
        put_ref(&mut descriptor, 2, 40);
        put_ref(&mut descriptor, 4, degree);
        put_ref(&mut descriptor, 8, poles);
        put_ref(&mut descriptor, 10, 3);
        put_ref(&mut descriptor, 14, distinct as u16);
        descriptor[16] = 2;
        descriptor[20] = 2;
        put_ref(&mut descriptor, 23, 42);
        put_ref(&mut descriptor, 25, 43);
        stream.extend(descriptor);

        let value_count = usize::from(poles) * 3;
        let mut payload = record(135, 15 + value_count * 8);
        put_ref(&mut payload, 2, 41);
        payload[9..13].copy_from_slice(&(value_count as u32).to_be_bytes());
        put_ref(&mut payload, 13, 1);
        for pole in 0..usize::from(poles) {
            let at = 15 + pole * 24;
            put_f64(&mut payload, at, pole as f64 * 0.01);
            put_f64(&mut payload, at + 8, 0.0);
            put_f64(&mut payload, at + 16, 0.0);
        }
        stream.extend(payload);

        let mut multiplicities = record(127, 8 + distinct * 2);
        multiplicities[4..6].copy_from_slice(&(distinct as u16).to_be_bytes());
        put_ref(&mut multiplicities, 6, 42);
        put_ref(&mut multiplicities, 8, degree + 1);
        for index in 1..distinct {
            put_ref(&mut multiplicities, 8 + index * 2, 1);
        }
        stream.extend(multiplicities);

        let mut knots = record(128, 8 + distinct * 8);
        knots[4..6].copy_from_slice(&(distinct as u16).to_be_bytes());
        put_ref(&mut knots, 6, 43);
        for index in 0..distinct {
            put_f64(&mut knots, 8 + index * 8, index as f64);
        }
        stream.extend(knots);
        stream
    }

    fn surface_stream(u_degree: u16, u_poles: u16, v_degree: u16, v_poles: u16) -> Vec<u8> {
        assert!(u_poles > u_degree && v_poles > v_degree);
        let u_distinct = usize::from(u_poles) + 1;
        let v_distinct = usize::from(v_poles) + 1;
        let poles = usize::from(u_poles) * usize::from(v_poles);
        let mut stream = Vec::new();

        let mut wrapper = record(124, 23);
        put_ref(&mut wrapper, 2, 10);
        wrapper[18] = b'+';
        put_ref(&mut wrapper, 19, 20);
        put_ref(&mut wrapper, 21, 21);
        stream.extend(wrapper);

        let mut descriptor = record(126, 48);
        put_ref(&mut descriptor, 2, 20);
        put_ref(&mut descriptor, 6, u_degree);
        put_ref(&mut descriptor, 8, v_degree);
        put_ref(&mut descriptor, 12, u_poles);
        put_ref(&mut descriptor, 16, v_poles);
        descriptor[18] = 2;
        descriptor[19] = 2;
        descriptor[20..24].copy_from_slice(&(u_distinct as u32).to_be_bytes());
        descriptor[24..28].copy_from_slice(&(v_distinct as u32).to_be_bytes());
        put_ref(&mut descriptor, 36, 30);
        put_ref(&mut descriptor, 38, 31);
        put_ref(&mut descriptor, 40, 32);
        put_ref(&mut descriptor, 42, 33);
        put_ref(&mut descriptor, 44, 125);
        put_ref(&mut descriptor, 46, 21);
        stream.extend(descriptor);

        let value_count = poles * 3;
        let mut payload = record(125, 97 + value_count * 8);
        put_ref(&mut payload, 2, 21);
        payload[90] = b'+';
        payload[91..95].copy_from_slice(&(value_count as u32).to_be_bytes());
        put_ref(&mut payload, 95, 1);
        for v in 0..usize::from(v_poles) {
            for u in 0..usize::from(u_poles) {
                let at = 97 + (v * usize::from(u_poles) + u) * 24;
                put_f64(&mut payload, at, u as f64 * 0.001);
                put_f64(&mut payload, at + 8, v as f64 * 0.001);
                put_f64(&mut payload, at + 16, 0.0);
            }
        }
        stream.extend(payload);

        for (reference, degree, distinct) in
            [(30, u_degree, u_distinct), (31, v_degree, v_distinct)]
        {
            let mut multiplicities = record(127, 8 + distinct * 2);
            multiplicities[4..6].copy_from_slice(&(distinct as u16).to_be_bytes());
            put_ref(&mut multiplicities, 6, reference);
            put_ref(&mut multiplicities, 8, degree + 1);
            for index in 1..distinct {
                put_ref(&mut multiplicities, 8 + index * 2, 1);
            }
            stream.extend(multiplicities);
        }
        for (reference, distinct) in [(32, u_distinct), (33, v_distinct)] {
            let mut knots = record(128, 8 + distinct * 8);
            knots[4..6].copy_from_slice(&(distinct as u16).to_be_bytes());
            put_ref(&mut knots, 6, reference);
            for index in 0..distinct {
                put_f64(&mut knots, 8 + index * 8, index as f64);
            }
            stream.extend(knots);
        }
        stream
    }

    let [high_degree] = crate::nurbs::curves(&curve_stream(11, 12))
        .try_into()
        .expect("one high-degree curve");
    let CurveGeometry::Nurbs(high_degree) = high_degree.geometry else {
        panic!("expected high-degree NURBS curve");
    };
    assert_eq!(high_degree.degree, 11);
    assert_eq!(high_degree.control_points.len(), 12);
    assert_eq!(high_degree.knots.len(), 24);

    let [wide_curve] = crate::nurbs::curves(&curve_stream(1, 5000))
        .try_into()
        .expect("one wide curve");
    let CurveGeometry::Nurbs(wide_curve) = wide_curve.geometry else {
        panic!("expected wide NURBS curve");
    };
    assert_eq!(wide_curve.control_points.len(), 5000);
    assert_eq!(wide_curve.knots.len(), 5002);

    let [wide_surface] = crate::nurbs::surfaces(&surface_stream(1, 2001, 1, 2))
        .try_into()
        .expect("one wide surface");
    let SurfaceGeometry::Nurbs(wide_surface) = wide_surface.geometry else {
        panic!("expected wide NURBS surface");
    };
    assert_eq!(wide_surface.control_points.len(), 4002);
    assert_eq!(wide_surface.u_knots.len(), 2003);
    assert_eq!(wide_surface.v_knots.len(), 4);

    let mut wide_curve_pole_count = curve_stream(1, 12);
    let curve_descriptor = wide_curve_pole_count
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("curve descriptor");
    wide_curve_pole_count[curve_descriptor + 6] = 1;
    assert!(crate::nurbs::curves(&wide_curve_pole_count).is_empty());

    let mut wide_curve_distinct_count = curve_stream(1, 12);
    wide_curve_distinct_count[curve_descriptor + 12] = 1;
    assert!(crate::nurbs::curves(&wide_curve_distinct_count).is_empty());

    let mut wide_surface_pole_count = surface_stream(1, 2, 1, 2);
    let surface_descriptor = wide_surface_pole_count
        .windows(4)
        .position(|window| window == [0, 126, 0, 20])
        .expect("surface descriptor");
    wide_surface_pole_count[surface_descriptor + 10] = 1;
    assert!(crate::nurbs::surfaces(&wide_surface_pole_count).is_empty());

    let mut wide_surface_distinct_count = surface_stream(1, 2, 1, 2);
    wide_surface_distinct_count[surface_descriptor + 20] = 1;
    assert!(crate::nurbs::surfaces(&wide_surface_distinct_count).is_empty());
}

#[test]
fn nurbs_carriers_reject_invalid_basis_cardinality() {
    let mut surface = bspline_partition_stream();
    let descriptor = surface
        .windows(4)
        .position(|window| window == [0, 126, 0, 20])
        .expect("surface descriptor");
    put_ref(&mut surface, descriptor + 6, 2);
    assert!(crate::nurbs::surfaces(&surface).is_empty());

    let mut curve = bspline_partition_stream();
    let descriptor = curve
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("curve descriptor");
    put_ref(&mut curve, descriptor + 4, 2);
    assert!(crate::nurbs::curves(&curve).is_empty());

    put_ref(&mut curve, descriptor + 10, 2);
    assert!(crate::nurbs::pcurves(&curve).is_empty());

    let mut short_knots = bspline_partition_stream();
    let multiplicities = short_knots
        .windows(12)
        .position(|record| record[..2] == [0, 127] && record[6..8] == 42u16.to_be_bytes())
        .expect("curve multiplicities");
    put_ref(&mut short_knots, multiplicities + 10, 1);
    assert!(crate::nurbs::curves(&short_knots).is_empty());
}

#[test]
fn nurbs_surface_rejects_mismatched_descriptor_payload_reference() {
    let mut stream = bspline_partition_stream();
    let descriptor = stream
        .windows(4)
        .position(|window| window == [0, 126, 0, 20])
        .expect("surface descriptor");
    put_ref(&mut stream, descriptor + 46, 22);
    assert!(crate::nurbs::surfaces(&stream).is_empty());
}

#[test]
fn nurbs_carriers_reject_duplicate_support_identities() {
    fn duplicate_record(stream: &mut Vec<u8>, tag: u8, xmt_offset: usize, xmt: u16, len: usize) {
        let start = stream
            .windows(len)
            .rposition(|record| {
                record[..2] == [0, tag] && record[xmt_offset..xmt_offset + 2] == xmt.to_be_bytes()
            })
            .expect("support record");
        let duplicate = stream[start..start + len].to_vec();
        stream.extend(duplicate);
    }

    for (tag, xmt_offset, xmt, len) in [
        (126, 2, 20, 48),
        (125, 2, 21, 193),
        (127, 6, 30, 12),
        (128, 6, 32, 24),
    ] {
        let mut stream = bspline_partition_stream();
        duplicate_record(&mut stream, tag, xmt_offset, xmt, len);
        assert!(
            crate::nurbs::surfaces(&stream).is_empty(),
            "duplicate type {tag}"
        );
    }

    for (tag, xmt_offset, xmt, len) in [
        (136, 2, 40, 27),
        (135, 2, 41, 63),
        (127, 6, 42, 12),
        (128, 6, 43, 24),
    ] {
        let mut stream = bspline_partition_stream();
        duplicate_record(&mut stream, tag, xmt_offset, xmt, len);
        assert!(
            crate::nurbs::curves(&stream).is_empty(),
            "duplicate type {tag}"
        );
    }
}

#[test]
fn nurbs_decodes_descriptors_at_the_stream_boundary() {
    fn move_record_to_end(stream: &mut Vec<u8>, tag: u8, xmt: u16, len: usize) {
        let start = stream
            .windows(len)
            .position(|record| record[..2] == [0, tag] && record[2..4] == xmt.to_be_bytes())
            .expect("descriptor record");
        let record = stream.drain(start..start + len).collect::<Vec<_>>();
        stream.extend(record);
    }

    let mut surface = bspline_partition_stream();
    move_record_to_end(&mut surface, 126, 20, 48);
    assert_eq!(crate::nurbs::surfaces(&surface).len(), 1);

    let mut curve = bspline_partition_stream();
    move_record_to_end(&mut curve, 136, 40, 27);
    assert_eq!(crate::nurbs::curves(&curve).len(), 1);
}

#[test]
fn nurbs_decodes_extended_xmt_arrays_payload_and_long_surface_descriptor() {
    let surfaces = crate::nurbs::surfaces(&extended_bspline_surface_stream());
    assert_eq!(surfaces.len(), 1);
    let SurfaceGeometry::Nurbs(surface) = &surfaces[0].geometry else {
        panic!("expected NURBS surface");
    };
    assert_eq!(surface.u_knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.v_knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.control_points.len(), 4);
    assert_eq!(surface.control_points[3].y, 20.0);
}

#[test]
fn nurbs_decodes_escaped_surface_payload_envelope() {
    let mut stream = bspline_partition_stream();
    let payload = stream
        .windows(4)
        .position(|window| window == [0, 125, 0, 21])
        .expect("surface payload");
    stream.insert(payload + 2, 0xff);

    let surfaces = crate::nurbs::surfaces(&stream);
    assert_eq!(surfaces.len(), 1);
    let SurfaceGeometry::Nurbs(surface) = &surfaces[0].geometry else {
        panic!("expected NURBS surface");
    };
    assert_eq!(surface.control_points.len(), 4);
    assert_eq!(surface.control_points[3].y, 20.0);
}

#[test]
fn nurbs_coalesces_equivalent_surface_descriptor_representations() {
    let mut stream = bspline_partition_stream();
    let mut descriptor = record(126, 49);
    put_ref(&mut descriptor, 2, 20);
    put_ref(&mut descriptor, 6, 1);
    put_ref(&mut descriptor, 8, 1);
    put_ref(&mut descriptor, 12, 2);
    put_ref(&mut descriptor, 16, 2);
    descriptor[18] = 5;
    descriptor[19] = 5;
    descriptor[20..24].copy_from_slice(&2u32.to_be_bytes());
    descriptor[24..28].copy_from_slice(&2u32.to_be_bytes());
    let mut at = 34;
    for reference in [9, 30, 31, 32, 33] {
        put_ref(&mut descriptor, at, reference);
        at += 2;
        descriptor[at] = 0;
        at += 1;
    }
    stream.extend(descriptor);

    let surfaces = crate::nurbs::surfaces(&stream);
    assert_eq!(surfaces.len(), 1);
    let SurfaceGeometry::Nurbs(surface) = &surfaces[0].geometry else {
        panic!("expected NURBS surface");
    };
    assert_eq!(surface.control_points.len(), 4);
}

#[test]
fn nurbs_coalesces_equivalent_curve_descriptor_representations() {
    let mut stream = bspline_partition_stream();
    let mut descriptor = record(136, 30);
    put_ref(&mut descriptor, 2, 40);
    put_ref(&mut descriptor, 4, 1);
    put_ref(&mut descriptor, 8, 2);
    put_ref(&mut descriptor, 10, 3);
    put_ref(&mut descriptor, 14, 2);
    descriptor[16] = 5;
    descriptor[20] = 1;
    let mut at = 21;
    for reference in [41, 42, 43] {
        put_ref(&mut descriptor, at, reference);
        at += 2;
        descriptor[at] = 0;
        at += 1;
    }
    stream.extend(descriptor);

    let curves = crate::nurbs::curves(&stream);
    assert_eq!(curves.len(), 1);
    let CurveGeometry::Nurbs(curve) = &curves[0].geometry else {
        panic!("expected NURBS curve");
    };
    assert_eq!(curve.control_points.len(), 2);
}

#[test]
fn nurbs_decodes_escaped_curve_descriptor_and_payload_count() {
    let mut stream = bspline_partition_stream();
    let descriptor = stream
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("curve descriptor");
    stream.insert(descriptor + 2, 0xff);
    let payload = stream
        .windows(4)
        .position(|window| window == [0, 135, 0, 41])
        .expect("curve payload");
    stream.insert(payload + 2, 0xff);
    stream.insert(payload + 10, 0xff);

    let curves = crate::nurbs::curves(&stream);
    assert_eq!(curves.len(), 1);
    let CurveGeometry::Nurbs(curve) = &curves[0].geometry else {
        panic!("expected NURBS curve");
    };
    assert_eq!(curve.control_points.len(), 2);
    assert_eq!(curve.control_points[1].x, 20.0);
}

#[test]
fn nurbs_compact_curve_descriptor_survives_a_status_prefix_collision() {
    let mut stream = bspline_partition_stream();
    let descriptor = stream
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("curve descriptor");
    stream[descriptor + 17..descriptor + 21].copy_from_slice(&[0, 0, 0, 1]);

    assert_eq!(crate::nurbs::curves(&stream).len(), 1);
}

#[test]
fn nurbs_decodes_dimension_four_rational_curve() {
    let mut stream = bspline_partition_stream();
    let descriptor = stream
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("curve descriptor");
    put_ref(&mut stream, descriptor + 10, 4);

    let payload = stream
        .windows(4)
        .position(|window| window == [0, 135, 0, 41])
        .expect("curve payload");
    let old_payload_len = 15 + 6 * 8;
    let mut rational_payload = record(135, 15 + 8 * 8);
    put_ref(&mut rational_payload, 2, 41);
    rational_payload[9..13].copy_from_slice(&8u32.to_be_bytes());
    for (index, value) in [0.0, 0.0, 0.0, 1.0, 0.04, 0.0, 0.0, 2.0]
        .into_iter()
        .enumerate()
    {
        put_f64(&mut rational_payload, 15 + index * 8, value);
    }
    stream.splice(payload..payload + old_payload_len, rational_payload);

    let curves = crate::nurbs::curves(&stream);
    assert_eq!(curves.len(), 1);
    let CurveGeometry::Nurbs(curve) = &curves[0].geometry else {
        panic!("expected NURBS curve");
    };
    assert_eq!(curve.weights.as_deref(), Some([1.0, 2.0].as_slice()));
    assert_eq!(curve.control_points[1].x, 20.0);
}
