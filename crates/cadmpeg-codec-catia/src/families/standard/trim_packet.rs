//! Checked trim-handle partitions and primitive expansion.

use cadmpeg_core::decode::u64_from_index;

use std::sync::OnceLock;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

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

impl TryFrom<(usize, Vec<usize>, Vec<usize>, Vec<u32>)> for TrimPacket {
    type Error = &'static str;

    fn try_from(
        (independent_count, strip_lengths, fan_lengths, handles): (
            usize,
            Vec<usize>,
            Vec<usize>,
            Vec<u32>,
        ),
    ) -> Result<Self, Self::Error> {
        let expected = independent_count.checked_mul(3).and_then(|count| {
            strip_lengths
                .iter()
                .chain(&fan_lengths)
                .try_fold(count, |count, length| count.checked_add(*length))
        });
        if expected != Some(handles.len()) {
            return Err("trim packet partition does not consume handles");
        }
        Ok(Self {
            independent_count,
            strip_lengths,
            fan_lengths,
            handles,
            triangles: OnceLock::new(),
        })
    }
}

impl TrimPacket {
    pub(crate) fn try_clone_with_context(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            independent_count: self.independent_count,
            strip_lengths: ctx
                .copy_retained_slice(&self.strip_lengths, "catia_trim_clone_strip_lengths")?,
            fan_lengths: ctx
                .copy_retained_slice(&self.fan_lengths, "catia_trim_clone_fan_lengths")?,
            handles: ctx.copy_retained_slice(&self.handles, "catia_trim_clone_handles")?,
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
        let lengths = self
            .strip_lengths
            .len()
            .checked_add(self.fan_lengths.len())
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        ctx.charge_work(u64_from_index(lengths), operation)?;
        let triangle_count = self
            .strip_lengths
            .iter()
            .chain(&self.fan_lengths)
            .try_fold(self.independent_count, |count, &length| {
                count.checked_add(match length {
                    0 | 1 => 0,
                    length => length - 2,
                })
            })
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        let work = u64_from_index(triangle_count)
            .checked_mul(3)
            .and_then(|work| work.checked_add(u64_from_index(self.handles.len())))
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        ctx.charge_work(work, operation)?;
        let mut triangles = Vec::new();
        let (independent, mut remaining) = self.handles.split_at(3 * self.independent_count);
        for triple in independent.chunks_exact(3) {
            ctx.charge_retained(
                u64_from_index(std::mem::size_of::<[u32; 3]>()),
                "catia_trim_triangles",
            )?;
            ctx.push_vec(
                &mut triangles,
                [triple[0], triple[1], triple[2]],
                "catia_trim_triangles",
            )?;
        }
        for &length in &self.strip_lengths {
            let (strip, tail) = remaining.split_at(length);
            remaining = tail;
            for (index, triple) in strip.windows(3).enumerate() {
                ctx.charge_retained(
                    u64_from_index(std::mem::size_of::<[u32; 3]>()),
                    "catia_trim_triangles",
                )?;
                ctx.push_vec(
                    &mut triangles,
                    if index % 2 == 0 {
                        [triple[0], triple[1], triple[2]]
                    } else {
                        [triple[1], triple[0], triple[2]]
                    },
                    "catia_trim_triangles",
                )?;
            }
        }
        for &length in &self.fan_lengths {
            let (fan, tail) = remaining.split_at(length);
            remaining = tail;
            let Some((&center, rim)) = fan.split_first() else {
                continue;
            };
            for pair in rim.windows(2) {
                ctx.charge_retained(
                    u64_from_index(std::mem::size_of::<[u32; 3]>()),
                    "catia_trim_triangles",
                )?;
                ctx.push_vec(
                    &mut triangles,
                    [center, pair[0], pair[1]],
                    "catia_trim_triangles",
                )?;
            }
        }
        Ok(triangles)
    }
}

#[cfg(test)]
mod tests {
    use super::TrimPacket;
    use cadmpeg_core::CodecError;

    #[test]
    fn trim_expansion_refuses_work_before_cache_installation() {
        let packet =
            TrimPacket::try_from((1, vec![], vec![], vec![0, 1, 2])).expect("complete partition");
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
        assert!(TrimPacket::try_from((usize::MAX, vec![], vec![], vec![])).is_err());
        assert!(TrimPacket::try_from((0, vec![usize::MAX], vec![1], vec![])).is_err());
        assert!(TrimPacket::try_from((1, vec![], vec![], vec![0, 1])).is_err());
        assert!(TrimPacket::try_from((0, vec![1], vec![], vec![0, 1])).is_err());
    }

    #[test]
    fn packet_reuses_expansion_without_changing_equality() {
        let packet = TrimPacket::try_from((1, vec![4], vec![4], (0..11).collect()))
            .expect("complete trim handle partition");
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
        let packet = TrimPacket::try_from((1, vec![4, 1], vec![4], (0..12).collect()))
            .expect("complete trim handle partition");
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
        let packet = TrimPacket::try_from((1, vec![4], vec![4], (0..11).collect()))
            .expect("complete trim handle partition");
        let refusal = crate::test_support::with_collection_limit(4, |ctx| packet.triangles(ctx))
            .expect_err("five triangles exceed four admitted items");
        assert!(matches!(refusal, CodecError::ResourceLimit(limit)
            if limit.operation == "catia_trim_triangles"));
        assert!(packet.triangles.get().is_none());
    }
}
