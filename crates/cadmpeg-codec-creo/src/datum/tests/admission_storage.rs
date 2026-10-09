// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn check_live_output_storage<T>(
    data: &[u8],
    parse: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>,
    check: impl Fn(&T) -> u64,
) {
    const CAP: u64 = 16 * 1024;
    for scoped in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = CAP;
        policy.limits.max_retained_bytes = CAP;
        let (ctx, _) = DecodeContext::from_root_bytes(data, &arena, &policy).expect("root");
        let parts = if scoped {
            let parts = ctx.with_scoped_storage("datum output parent", || parse(&ctx))
                .expect("scoped datum output");
            (parts.0, Some(parts.1))
        } else {
            (parse(&ctx).expect("retained datum output"), None)
        };
        let storage = parts.1;
        let value = parts.0;
        let expected_bytes = check(&value);
        let refusal = if scoped {
            ctx.reserve_scoped_limit(CAP + 1, "after datum scratch")
                .expect_err("probe live output only")
        } else {
            ctx.charge_retained_limit(CAP + 1, "after datum scratch")
                .expect_err("probe retained output only")
        };
        assert_eq!(refusal.dimension, if scoped {
            ResourceDimension::MaterializedBytes
        } else {
            ResourceDimension::RetainedBytes
        });
        assert_eq!((refusal.used, refusal.additional), (expected_bytes, CAP + 1));
        assert!(matches!(parse(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == refusal));
        assert_eq!(ctx.resource_refusal(), Some(refusal));
        drop(value);
        drop(storage);
    }
}

#[test]
fn datum_plane_scratch_drops_before_returned_records_last_use() {
    let mut data = b"srf_array\0\xf8\x01".to_vec();
    data.extend([4, 0x22, 1, 1, 1, 0]);
    data.extend([0x0f; 4]);
    for value in [2.0, 0.0, 3.0, -2.0, 0.0, -3.0] {
        if value == 0.0 { data.push(0x0f); } else { data.extend(ieee8(value)); }
    }
    check_live_output_storage(&data, |ctx| planes(ctx, &data), |records| {
        assert_eq!(records.as_slice(), [DatumPlaneRecord::new(4, 1,
            DatumPlane::new(Axis::Y, 0.0).expect("plane"), 0.0,
            [[Some(2.0), Some(3.0)], [Some(-2.0), Some(-3.0)]], 12)
            .expect("matching outline")]);
        u64::try_from(records.capacity() * std::mem::size_of::<DatumPlaneRecord>())
            .expect("output backing bytes")
    });
}

#[test]
fn datum_cylinder_scratch_drops_before_returned_records_last_use() {
    let mut data = b"srf_array\0\xf8\x01".to_vec();
    data.extend([8, 0x24, 3, 1, 1, 0]);
    data.extend([
        0x14, 0x2f, 0x10, 0x00, 0x2d, 0x1f, 0x6a, 0x7a, 0x29, 0x55, 0x38, 0x5e, 0x2f, 0x43, 0x00,
        0x48, 0x29, 0x00, 0x2f, 0x10, 0x00, 0x43, 0xe8, 0x00, 0x48, 0x27, 0x80, 0x2f, 0x43, 0x00,
        0x2a, 0xe8, 0x00,
    ]);
    check_live_output_storage(&data, |ctx| cylinders(ctx, &data), |records| {
        let [record] = records.as_slice() else { panic!("one cylinder"); };
        assert_eq!((record.id, record.feature_id, record.reversed, record.offset_in_payload),
            (8, 3, false, 12));
        assert_eq!(record.frame.frame().origin(), [-12.5, 4.0, 0.0]);
        assert_eq!(record.frame.frame().axis(), [0.0, 1.0, 0.0]);
        assert_eq!(record.frame.frame().ref_direction(), [1.0, 0.0, 0.0]);
        assert_eq!(record.frame.radius().get(), 0.75);
        assert_eq!(record.frame.length().map(PositiveLength::get), Some(34.0));
        u64::try_from(records.capacity() * std::mem::size_of::<super::super::DatumCylinder>())
            .expect("output backing bytes")
    });
}

#[test]
fn named_datum_scratch_drops_before_returned_record_last_use() {
    let mut data = b"\xe0\x01geom_id\0\x02\xe0\x01feat_id\0\x01outline\0\xf9\x02\x03".to_vec();
    for value in [0.0, 3.0, 4.0, 0.0, -3.0, -4.0] {
        if value == 0.0 { data.push(0x0f); } else { data.extend(ieee8(value)); }
    }
    check_live_output_storage(&data, |ctx| named_plane(ctx, &data), |record| {
        let record = record.as_ref().expect("named plane");
        assert_eq!((record.id, record.feature_id), (2, 1));
        assert_eq!(record.plane().normal(), [1.0, 0.0, 0.0]);
        assert_eq!(record.plane().offset(), 0.0);
        assert_eq!(record.corners(), [
            [Some(0.0), Some(3.0), Some(4.0)],
            [Some(0.0), Some(-3.0), Some(-4.0)],
        ]);
        0
    });
}
