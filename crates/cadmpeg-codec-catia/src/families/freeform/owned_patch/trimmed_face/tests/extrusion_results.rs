// SPDX-License-Identifier: Apache-2.0
use super::super::*;
use crate::test_support::test_a5a8::a5_pcurve_stream_with_support_and_uv;
use crate::wire::bytes::AllocationReferenceEncoding;
use crate::wire::records::consolidated_records;
use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurfaceAxis};

fn frame(bytes: &mut Vec<u8>, class: u8, prefix: &[u8], scalars: &[f64]) {
    let mut payload = prefix.to_vec();
    for value in scalars {
        payload.extend(value.to_le_bytes());
    }
    bytes.extend([
        0xb2,
        3,
        class,
        u8::try_from(payload.len()).expect("short frame"),
        5,
    ]);
    bytes.extend(payload);
}
fn stream() -> Vec<u8> {
    let mut bytes = vec![0xa5, 3, 0x34, 0, 0, 0, 0, 5];
    frame(&mut bytes, 0x31, &[5], &[0.0, -1.0, -1.0, 2.0, 2.0]);
    let mut bridge = vec![5, 9];
    bridge.extend(0.0_f64.to_le_bytes());
    bridge.push(1);
    frame(&mut bytes, 0x30, &bridge, &[-1.0, 2.0, -1.0, 2.0]);
    for support in [1, 2] {
        let mut curve = a5_pcurve_stream_with_support_and_uv(support, [-1.0, 2.0], [0.0, 0.0]);
        let knots = 8 + 1 + 4;
        curve[knots..knots + 8].copy_from_slice(&(-1.0_f64).to_le_bytes());
        curve[knots + 8..knots + 16].copy_from_slice(&2.0_f64.to_le_bytes());
        bytes.extend(curve);
    }
    frame(
        &mut bytes,
        0x23,
        &[0x82, 9, 5, 0x0a, 0x87, 0x0d],
        &[0.0, 1.0, 1e-6, 0.0, 1.0, 1.0, 0.0, 1e-6],
    );
    let mut offset = vec![5];
    for value in [0.0_f64, 1.0] {
        offset.extend(value.to_le_bytes());
    }
    offset.push(5);
    frame(&mut bytes, 0x14, &offset, &[0.25, 0.0, 0.0, 1.0, -0.2, 0.8]);
    let mut extrusion = vec![5];
    for value in [0.0_f64, 0.0, 1.0, 0.0, 2.0, 1.0, 0.0, 0.0, 0.0] {
        extrusion.extend(value.to_le_bytes());
    }
    extrusion.extend([5, 5]);
    frame(&mut bytes, 0x2c, &extrusion, &[]);
    let mut bridge = vec![5, 25];
    bridge.extend(0.0_f64.to_le_bytes());
    bridge.push(0x21);
    frame(&mut bytes, 0x30, &bridge, &[-0.2, 0.8, 0.0, 2.0]);
    let mut ring = vec![0x89];
    ring.extend([5; 9]);
    ring.extend([0x84, 0x41, 0, 0, 0x0d]);
    for value in [0.0_f64, 0.2, 1.0, 1.8] {
        ring.extend(value.to_le_bytes());
    }
    ring.push(1);
    for value in [-2.0_f32, 3.0, -2.0, 3.0, -2.0, 3.0] {
        ring.extend(value.to_le_bytes());
    }
    frame(&mut bytes, 0x62, &ring, &[]);
    bytes
}
fn run(expand: bool, extend_base: bool) {
    let mut bytes = stream();
    if extend_base {
        let records = consolidated_records(&bytes);
        let cache = records[1].payload().expect("offset cache").start;
        bytes[cache + 1..cache + 9].copy_from_slice(&0.1_f64.to_le_bytes());
        let bridge = records[2].payload().expect("base bridge").start;
        bytes[bridge + 2..bridge + 10].copy_from_slice(&0.1_f64.to_le_bytes());
        bytes[bridge + 11..bridge + 19].copy_from_slice(&0.2_f64.to_le_bytes());
        bytes[bridge + 19..bridge + 27].copy_from_slice(&0.8_f64.to_le_bytes());
    }
    let records = consolidated_records(&bytes);
    assert_eq!(records.len(), 10);
    let axis = || NurbsSurfaceAxis::new(1, vec![-1.0, -1.0, 2.0, 2.0], false);
    let surface = NurbsSurface::new(
        axis(),
        axis(),
        NurbsPoleGrid::Polynomial {
            rows: [-1.0, 2.0]
                .map(|u| [-1.0, 2.0].map(|v| Point3::new(u, v, 0.0)).to_vec())
                .to_vec(),
        },
        false,
    )
    .expect("cached planar tensor");
    let tensors = BTreeMap::from([(0, surface)]);
    let source = Source {
        bytes: &bytes,
        records: &records,
        tensors: &tensors,
        physical: Some,
        resolve: |index: usize, reference: AllocationReference| {
            let value = usize::try_from(reference.value).ok()?;
            match reference.encoding {
                AllocationReferenceEncoding::BackwardDistance => index.checked_sub(value),
                AllocationReferenceEncoding::OwnedChild => index.checked_add(1)?.checked_add(value),
                _ => None,
            }
        },
    };
    let ir = CadIr::empty();
    let carriers = BTreeMap::new();
    let mut notes = LaneRefusals::new();
    let carrier = if expand {
        source.extrusion_ring_carrier(&ir, 8, 9, [[-0.2, 0.8], [0.0, 2.0]], &carriers, &mut notes)
    } else {
        source.carrier(&ir, 8, &carriers, &mut notes)
    }
    .expect("decoded extrusion result");
    assert_eq!(
        carrier.bounds(),
        Some([[-0.2, if expand { 1.0 } else { 0.8 }], [0.0, 2.0]])
    );
    for u in [-0.2_f64, -0.1, 0.3, if expand { 1.0 } else { 0.8 }] {
        for v in [0.0, 2.0] {
            let p = carrier.evaluate([u, v]).expect("retained surface point");
            assert!(
                p.distance(Point3::new(
                    u + 0.2,
                    -0.25,
                    v + if extend_base { 0.1 } else { 0.0 }
                )) < 1e-9
            );
        }
    }
    let notes = notes.take_notes();
    assert_eq!(notes.len(), 1);
    assert_eq!(
        notes[0].code.local_code(),
        "geometry.parameter-map-accuracy-inferred"
    );
    assert!(notes[0].message.contains("class-14"));
    assert!(notes[0]
        .message
        .contains(&format!("base chart: {extend_base}")));
}
#[test]
fn serialized_extrusion_result_maps_into_its_native_directrix() {
    run(false, false);
}
#[test]
fn serialized_ring_expansion_keeps_the_original_chart_sides() {
    run(true, false);
}

#[test]
fn serialized_extrusion_reports_use_of_the_extended_base_chart() {
    run(false, true);
}
