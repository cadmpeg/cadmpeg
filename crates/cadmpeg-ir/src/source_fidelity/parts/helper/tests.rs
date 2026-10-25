// SPDX-License-Identifier: Apache-2.0

use super::reserve_output;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

#[test]
fn output_reservations_grow_amortized_and_charge_retained_capacity_once() -> Result<(), CodecError>
{
    for staged in [false, true] {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"source", &arena, &DecodePolicy::service())?;
        let available = ctx.retained_bytes_available();
        let mut values = Vec::<u64>::new();
        let mut growths = 0;
        for value in 0..1024 {
            let capacity = values.capacity();
            let before = ctx.retained_bytes_available();
            reserve_output(&ctx, staged, &mut values, 1)?;
            if values.capacity() != capacity {
                growths += 1;
            } else {
                assert_eq!(ctx.retained_bytes_available(), before);
            }
            values.push(value);
        }
        assert!(
            growths <= 11,
            "1024 insertions require at most eleven doublings"
        );
        assert_eq!(values, (0..1024).collect::<Vec<_>>());
        let charged = available - ctx.retained_bytes_available();
        if staged {
            assert_eq!(charged, 0);
        } else {
            assert_eq!(
                charged,
                u64::try_from(values.capacity() * size_of::<u64>()).expect("bounded capacity")
            );
        }
    }
    Ok(())
}
