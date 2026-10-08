//! Checked trim-handle partitions and primitive expansion.

use cadmpeg_core::decode::u64_from_index;

use std::sync::OnceLock;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::num::NonZeroUsize;

#[derive(Debug, Clone)]
pub(crate) struct TrimPacket {
    independent_count: usize,
    strip_lengths: Vec<usize>,
    fan_lengths: Vec<usize>,
    handles: Vec<u32>,
    triangles: OnceLock<Vec<[u32; 3]>>,
}

impl PartialEq for TrimPacket {
    fn eq(&self, other: &Self) -> bool {
        self.independent_count == other.independent_count
            && self.strip_lengths == other.strip_lengths
            && self.fan_lengths == other.fan_lengths
            && self.handles == other.handles
    }
}

/// A comparison reads the primitive count and every length and handle.
impl cadmpeg_core::decode::cost::DecodeCost for TrimPacket {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        let mut bytes = u64_from_index(std::mem::size_of::<usize>());
        for part in [
            self.strip_lengths.decode_cost(ctx, operation)?,
            self.fan_lengths.decode_cost(ctx, operation)?,
            self.handles.decode_cost(ctx, operation)?,
        ] {
            bytes = bytes
                .checked_add(part)
                .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        }
        Ok(bytes)
    }
}

impl TrimPacket {
    pub(crate) fn try_from(
        ctx: &DecodeContext<'_>,
        (independent_count, strip_lengths, fan_lengths, handles): (
            usize,
            Vec<usize>,
            Vec<usize>,
            Vec<u32>,
        ),
    ) -> Result<Option<Self>, CodecError> {
        let operation = "catia_trim_packet_partition";
        let mut expected = independent_count
            .checked_mul(3)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        for length in ctx
            .admit_iter(&strip_lengths, "catia_trim_packet_partition")?
            .chain(ctx.admit_iter(&fan_lengths, "catia_trim_packet_partition")?)
        {
            expected = expected
                .checked_add(*length)
                .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        }
        if expected != handles.len() {
            return Ok(None);
        }
        Ok(Some(Self {
            independent_count,
            strip_lengths,
            fan_lengths,
            handles,
            triangles: OnceLock::new(),
        }))
    }
}

impl TrimPacket {
    pub(crate) fn try_clone_with_context(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            independent_count: self.independent_count,
            strip_lengths: ctx.copy_slice(&self.strip_lengths, "catia_trim_clone_strip_lengths")?,
            fan_lengths: ctx.copy_slice(&self.fan_lengths, "catia_trim_clone_fan_lengths")?,
            handles: ctx.copy_slice(&self.handles, "catia_trim_clone_handles")?,
            triangles: OnceLock::new(),
        })
    }
    pub(crate) fn handles(&self) -> &[u32] {
        &self.handles
    }

    #[cfg(test)]
    pub(super) fn independent_count(&self) -> usize {
        self.independent_count
    }

    #[cfg(test)]
    pub(super) fn strip_lengths(&self) -> &[usize] {
        &self.strip_lengths
    }

    #[cfg(test)]
    pub(super) fn fan_lengths(&self) -> &[usize] {
        &self.fan_lengths
    }

    pub(crate) fn triangles(&self, ctx: &DecodeContext<'_>) -> Result<&[[u32; 3]], CodecError> {
        if let Some(triangles) = self.triangles.get() {
            return Ok(triangles);
        }
        let triangles = self.expand_triangles(ctx)?;
        match self.triangles.set(triangles) {
            Ok(()) | Err(_) => {
                self.triangles.get().map(Vec::as_slice).ok_or_else(|| {
                    ctx.refuse_codec_limit("catia_trim_triangles", u64::MAX, u64::MAX)
                })
            }
        }
    }

    fn expand_triangles(&self, ctx: &DecodeContext<'_>) -> Result<Vec<[u32; 3]>, CodecError> {
        let operation = "catia_trim_expansion_work";
        let triangle_count = ctx
            .admit_iter(&self.strip_lengths, operation)?
            .chain(ctx.admit_iter(&self.fan_lengths, operation)?)
            .try_fold(self.independent_count, |count, &length| {
                count.checked_add(match length {
                    0 | 1 => 0,
                    length => length - 2,
                })
            })
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        let mut triangles = Vec::new();
        ctx.reserve_vec(&mut triangles, triangle_count, "catia_trim_triangles")?;
        let (independent, mut remaining) = self.handles.split_at(3 * self.independent_count);
        let Some(triple_width) = NonZeroUsize::new(3) else {
            return Err(ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX));
        };
        for triple in ctx.admit_iter(independent, operation)?.chunks(triple_width) {
            let [first, second, third] = triple else {
                return Err(ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX));
            };
            triangles.push([*first, *second, *third]);
        }
        for &length in ctx.admit_iter(&self.strip_lengths, operation)? {
            let (strip, tail) = remaining.split_at(length);
            remaining = tail;
            for (index, triple) in ctx
                .admit_iter(strip, operation)?
                .windows(triple_width)
                .enumerate()
            {
                triangles.push(if index % 2 == 0 {
                    [triple[0], triple[1], triple[2]]
                } else {
                    [triple[1], triple[0], triple[2]]
                });
            }
        }
        for &length in ctx.admit_iter(&self.fan_lengths, operation)? {
            let (fan, tail) = remaining.split_at(length);
            remaining = tail;
            let Some(pair_width) = NonZeroUsize::new(2) else {
                return Err(ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX));
            };
            let Some((&center, rim)) = fan.split_first() else {
                continue;
            };
            for pair in ctx.admit_iter(rim, operation)?.windows(pair_width) {
                triangles.push([center, pair[0], pair[1]]);
            }
        }
        Ok(triangles)
    }
}

#[cfg(test)]
mod tests {
    use super::TrimPacket;
    use cadmpeg_core::CodecError;

    fn packet(args: (usize, Vec<usize>, Vec<usize>, Vec<u32>)) -> TrimPacket {
        crate::test_support::with_service_context(|ctx| TrimPacket::try_from(ctx, args))
            .expect("service resource budget")
            .expect("complete trim handle partition")
    }

    #[test]
    fn trim_expansion_refuses_work_before_cache_installation() {
        let packet = packet((1, vec![], vec![], vec![0, 1, 2]));
        crate::test_support::with_work_limit(0, |ctx| {
            let CodecError::ResourceLimit(limit) = packet
                .triangles(ctx)
                .expect_err("triangle work must be admitted")
            else {
                panic!("resource refusal required")
            };
            assert_eq!(limit.operation, "catia_trim_expansion_work");
            assert_eq!(ctx.resource_refusal(), Some(limit));
        });
        assert!(packet.triangles.get().is_none());
    }

    #[test]
    fn packet_rejects_overflow_and_incomplete_partitions() {
        for args in [
            (usize::MAX, vec![], vec![], vec![]),
            (0, vec![usize::MAX], vec![1], vec![]),
        ] {
            crate::test_support::with_service_context(|ctx| {
                let CodecError::ResourceLimit(limit) = TrimPacket::try_from(ctx, args)
                    .expect_err("overflow must refuse as a resource limit")
                else {
                    panic!("resource refusal required")
                };
                assert_eq!(limit.operation, "catia_trim_packet_partition");
                assert_eq!(ctx.resource_refusal(), Some(limit));
            });
        }
        for args in [
            (1, vec![], vec![], vec![0, 1]),
            (0, vec![1], vec![], vec![0, 1]),
        ] {
            let result =
                crate::test_support::with_service_context(|ctx| TrimPacket::try_from(ctx, args));
            assert!(result.expect("service resource budget").is_none());
        }
    }

    #[test]
    fn trim_partition_sources_propagate_caller_work_refusals() {
        // One strip-length visit precedes one fan-length visit.
        for cap in [0, 1] {
            crate::test_support::with_work_limit(cap, |ctx| {
                let CodecError::ResourceLimit(limit) =
                    TrimPacket::try_from(ctx, (0, vec![1], vec![1], vec![10, 11]))
                        .expect_err("both partition sources require admission")
                else {
                    panic!("resource refusal required")
                };
                assert_eq!(limit.operation, "catia_trim_packet_partition");
                assert_eq!(ctx.resource_refusal(), Some(limit));
            });
        }
    }

    #[test]
    fn packet_reuses_expansion_without_changing_equality() {
        let packet = packet((1, vec![4], vec![4], (0..11).collect()));
        let cold = packet.clone();
        assert!(packet.triangles.get().is_none());
        crate::test_support::with_service_context(|ctx| {
            let first = packet.triangles(ctx).expect("service resource budget");
            assert_eq!(first.len(), 5);
            assert!(std::ptr::eq(
                first,
                packet.triangles(ctx).expect("cached triangles")
            ));
            assert!(std::ptr::eq(
                first,
                packet.triangles(ctx).expect("cached triangles")
            ));
        });
        assert!(packet.triangles.get().is_some());
        assert!(cold.triangles.get().is_none());
        assert_eq!(packet, cold);
    }

    #[test]
    fn packet_preserves_independent_strip_and_fan_order() {
        let packet = packet((1, vec![4, 1], vec![4], (0..12).collect()));
        assert_eq!(
            crate::test_support::with_service_context(|ctx| packet
                .triangles(ctx)
                .expect("service resource budget")
                .to_vec()),
            [[0, 1, 2], [3, 4, 5], [5, 4, 6], [8, 9, 10], [8, 10, 11]]
        );
    }

    #[test]
    fn lazy_trim_triangle_expansion_refuses_before_retained_growth() {
        let packet = packet((1, vec![4], vec![4], (0..11).collect()));
        let refusal = crate::test_support::with_collection_limit(4, |ctx| packet.triangles(ctx))
            .expect_err("five triangles exceed four admitted items");
        assert!(matches!(refusal, CodecError::ResourceLimit(limit)
            if limit.operation == "catia_trim_triangles"));
        assert!(packet.triangles.get().is_none());
    }
}
