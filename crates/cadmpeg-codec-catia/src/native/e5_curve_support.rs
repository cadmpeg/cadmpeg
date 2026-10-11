// SPDX-License-Identifier: Apache-2.0
//! Complete E5 correspondence programs indexed by physical source record.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use crate::families::e5::support::{self, E5CurveSupport};

/// A source support record and its admitted reference/co-facet program.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CatiaE5CurveSupportRecord {
    pub(crate) id: String,
    /// Absolute record offset in the source file.
    byte_offset: u64,
    /// Persistent E5 record identity.
    record_id: u32,
    /// Ordered facets, independent tolerance fields and complete map program.
    support: E5CurveSupport,
}

pub(super) fn derive(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<CatiaE5CurveSupportRecord>, CodecError> {
    ctx.charge_work(u64_from_index(bytes.len()), "catia_e5_native_support_scan")?;
    let mut output = Vec::new();
    for span in crate::container::all_e5_record_spans(bytes) {
        let frame = &bytes[span.clone()];
        let class = frame[3];
        if !matches!(class, 0xc0 | 0xc1) {
            continue;
        }
        let Some(support) = support::parse(ctx, class, &frame[13..])? else {
            continue;
        };
        let record_id = View::u32_le_at(frame, 9)
            .ok_or_else(|| CodecError::malformed("E5 support frame has no record identity"))?;
        ctx.push_vec(
            &mut output,
            CatiaE5CurveSupportRecord {
                id: ctx.format_retained(
                    format_args!("catia:e5:curve-support#{}", span.start),
                    "catia_native_e5_curve_support_id",
                )?,
                byte_offset: u64_from_index(span.start),
                record_id,
                support,
            },
            "catia_native_e5_curve_supports",
        )?;
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::derive;

    fn boundary_frames() -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut payload = vec![0x81, 0x81, 0x81, 0, 0];
        for value in [0.0_f64, 1.0, 0.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.extend_from_slice(&0_u32.to_le_bytes());
        for _ in 0..2 {
            crate::test_support::test_e5::append_e5_record(&mut bytes, 0xc0, 7, &payload);
        }
        bytes
    }

    #[test]
    fn physical_records_retain_distinct_identities_and_round_trip() {
        let records =
            crate::test_support::with_service_context(|ctx| derive(ctx, &boundary_frames()))
                .expect("service budget");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].record_id, 7);
        assert_eq!(records[1].record_id, 7);
        assert_ne!(records[0].id, records[1].id);
        assert_eq!(records[0].byte_offset, 0);
        assert_eq!(records[1].byte_offset, 46);
        let wire = serde_json::to_value(&records).expect("store programs");
        assert_eq!(
            serde_json::from_value::<Vec<super::CatiaE5CurveSupportRecord>>(wire)
                .expect("load programs"),
            records
        );
    }

    #[test]
    fn native_support_records_refuse_before_arena_growth() {
        let result =
            crate::test_support::with_collection_limit(0, |ctx| derive(ctx, &boundary_frames()));
        assert!(
            matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_native_e5_curve_supports")
        );
    }
}
