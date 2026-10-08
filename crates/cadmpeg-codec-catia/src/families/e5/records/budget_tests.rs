// SPDX-License-Identifier: Apache-2.0
//! NURBS pole rejection work.

#[test]
fn e5_invalid_first_pole_does_not_charge_unvisited_rows_or_columns() {
    for (rows, columns) in [(1, 1), (10_000, 1), (1, 10_000)] {
        let mut visits = 0;
        let result = crate::test_support::with_work_limit(2, |ctx| {
            super::nurbs_pole_rows(ctx, rows, columns, |_| {
                visits += 1;
                None::<[f64; 3]>
            })
        })
        .expect("one row visit and one pole visit");
        assert!(result.is_none());
        assert_eq!(visits, 1);
    }
}


#[test]
fn e5_analytic_surfaces_do_not_read_past_declared_payloads() {
    use crate::test_support::test_e5::{e5_circle_stream, e5_torus_stream};

    let mut cone = e5_torus_stream();
    cone[3] = 0xca;
    cone[110..118].copy_from_slice(&std::f64::consts::FRAC_PI_4.to_le_bytes());
    cone[118..126].copy_from_slice(&2.0_f64.to_le_bytes());
    cone.resize(174, 0);
    cone[158..166].copy_from_slice(&1.0_f64.to_le_bytes());
    cone[166..174].copy_from_slice(&1.0_f64.to_le_bytes());
    cone[5..7].copy_from_slice(&161_u16.to_le_bytes());
    // Each complete record is a carrier. Every shortened header leaves the
    // same bytes after its declared frame, where they cannot supply geometry.
    for mut bytes in [e5_circle_stream(), cone, e5_torus_stream()] {
        let size = usize::from(u16::from_le_bytes([bytes[5], bytes[6]]));
        let complete = crate::test_support::with_service_context(|ctx| {
            super::e5_surfaces(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
        })
        .expect("complete carrier budget");
        assert_eq!(complete.len(), 1);
        for payload_size in 0..size {
            bytes[5..7].copy_from_slice(
                &u16::try_from(payload_size).expect("fixture payload size").to_le_bytes(),
            );
            let frame = super::E5Frame::at(&bytes, 0).expect("short frame is structurally readable");
            // The carrier may omit an opaque suffix, but it cannot read a
            // scalar which is only present beyond the declared payload.
            let standalone = &bytes[..frame.end()];
            let bounded = crate::test_support::with_service_context(|ctx| {
                super::e5_surfaces(ctx, standalone, &mut crate::nurbs::LaneRefusals::new())
            })
            .expect("short carrier budget");
            let with_trailing_bytes = crate::test_support::with_service_context(|ctx| {
                super::e5_surfaces(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
            })
            .expect("trailing source budget");
            assert_eq!(with_trailing_bytes.len(), bounded.len(), "class {} size {payload_size}", bytes[3]);
        }
    }
}

#[test]
fn e5_bounded_carrier_transfer_keeps_original_resource_refusal() {
    let bytes = crate::test_support::test_e5::e5_torus_stream();
    let refusal = crate::test_support::with_collection_limit(0, |ctx| {
        let error = super::e5_surfaces(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
            .expect_err("carrier output growth refuses");
        let cadmpeg_core::CodecError::ResourceLimit(first) = error else {
            panic!("typed refusal");
        };
        assert_eq!(first.operation, "catia_e5_surfaces");
        assert_eq!(ctx.resource_refusal(), Some(first));
        assert!(matches!(
            super::e5_surfaces(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new()),
            Err(cadmpeg_core::CodecError::ResourceLimit(next)) if next == first
        ));
        Ok::<_, cadmpeg_core::CodecError>(())
    });
    refusal.expect("refusal assertions complete");
}
