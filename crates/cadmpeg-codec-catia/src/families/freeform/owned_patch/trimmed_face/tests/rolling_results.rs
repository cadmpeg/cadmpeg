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
fn with_carrier<T>(
    bytes: &[u8],
    index: usize,
    cached: bool,
    run: impl FnOnce(Carrier<'_>) -> T,
) -> Option<T> {
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
    let mut ir = CadIr::empty();
    let mut carriers = BTreeMap::new();
    if cached {
        use cadmpeg_ir::geometry::{ProceduralSurface, Surface, SurfaceGeometry};
        use cadmpeg_ir::ids::{ProceduralSurfaceId, SurfaceId};
        let native_bytes = jet(1.0, 0.0);
        let native_records = consolidated_records(&native_bytes);
        let definition = a5_owned_rolling_curves(&native_bytes, &native_records)
            .next()?
            .definition()?;
        let construction = ProceduralSurfaceId::mint("catia:test:cached-result").expect("ID");
        ir.geometry.procedural_surfaces.push(ProceduralSurface::new(
            construction.clone(),
            definition,
            None,
        ));
        ir.geometry.surfaces.push(Surface {
            id: SurfaceId::mint("catia:test:cached-support").expect("ID"),
            geometry: SurfaceGeometry::Procedural {
                construction,
                cache: None,
            },
            source_object: None,
            native_ref: None,
        });
        carriers.insert(records[1].byte_offset(), 0);
    }
    source
        .carrier(&ir, index, &carriers, &mut LaneRefusals::new())
        .map(run)
}
fn result_stream(bounds: [[f64; 2]; 2]) -> Vec<u8> {
    let mut bytes = jet(0.75, 0.0);
    bytes.extend(jet(1.0, 0.0));
    construction(&mut bytes, 9, 0.75);
    let mut p = vec![9, 5];
    p.extend(0.25_f64.to_le_bytes());
    p.push(0x19);
    for f in bounds.into_iter().flatten() {
        p.extend(f.to_le_bytes());
    }
    frame(&mut bytes, 0x30, &p);
    bytes
}

#[test]
fn cropped_rolling_results_keep_the_native_transverse_parameterization() {
    for cached in [false, true] {
        for v in [[0.0, 2.0], [0.0, 1.5], [0.5, 1.5], [1.0, 1.5]] {
            let bytes = result_stream([[0.2, 0.8], v]);
            with_carrier(&bytes, 3, cached, |carrier| {
                assert_eq!(carrier.bounds(), Some([[0.0, 1.0], [0.0, 2.0]]));
                for u in [0.0, 0.5, 1.0] {
                    let point = carrier
                        .evaluate(carrier.point([u, 1.0]))
                        .expect("native midpoint");
                    let r = std::f64::consts::FRAC_1_SQRT_2;
                    assert!(point.distance(Point3::new(r, r, u)) < 1e-10);
                }
            })
            .expect("cropped result construction");
        }
    }
}

#[test]
fn rolling_result_windows_do_not_reject_native_supports() {
    for cached in [false, true] {
        for bounds in [
            [[-0.1, 1.0], [0.0, 1.0]],
            [[0.0, 1.0], [-0.1, 1.5]],
            [[0.0, 1.0], [0.0, 2.1]],
            [[0.0, 1.0], [0.0, f64::from_bits(2.0_f64.to_bits() + 1)]],
        ] {
            let bytes = result_stream(bounds);
            with_carrier(&bytes, 3, cached, |carrier| {
                assert_eq!(carrier.bounds(), Some([[0.0, 1.0], [0.0, 2.0]]));
                let endpoint = carrier
                    .evaluate(carrier.point([1.0, 2.0]))
                    .expect("native endpoint");
                assert!(endpoint.distance(Point3::new(0.0, 1.0, 1.0)) < 1e-10);
            })
            .expect("native support survives result window");
        }
    }
}

#[test]
fn cached_rolling_result_survives_unavailable_native_sites() {
    let mut bytes = result_stream([[0.0, 1.0], [0.0, 2.0]]);
    let records = consolidated_records(&bytes);
    let rail = records[1].payload().expect("jet payload").start + 4 + 16;
    bytes[rail..rail + 8].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(
        a5_owned_rolling_curves(&bytes, &consolidated_records(&bytes))
            .all(|j| j.pos != records[1].byte_offset())
    );
    with_carrier(&bytes, 3, true, |carrier| {
        assert_eq!(carrier.bounds(), Some([[0.0, 1.0], [0.0, 2.0]]));
    })
    .expect("cached support remains available");
}
