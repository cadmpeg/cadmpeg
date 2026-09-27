//! Checked trim-handle partitions and primitive expansion.

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
            strip_lengths: crate::resource::copy_retained_slice(
                ctx,
                &self.strip_lengths,
                "catia_trim_clone_strip_lengths",
            )?,
            fan_lengths: crate::resource::copy_retained_slice(
                ctx,
                &self.fan_lengths,
                "catia_trim_clone_fan_lengths",
            )?,
            handles: crate::resource::copy_retained_slice(
                ctx,
                &self.handles,
                "catia_trim_clone_handles",
            )?,
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
        let mut triangles = Vec::new();
        let (independent, mut remaining) = self.handles.split_at(3 * self.independent_count);
        for triple in independent.chunks_exact(3) {
            ctx.charge_retained(
                std::mem::size_of::<[u32; 3]>() as u64,
                "catia_trim_triangles",
            )?;
            crate::resource::push(
                ctx,
                &mut triangles,
                [triple[0], triple[1], triple[2]],
                "catia_trim_triangles",
            )?;
        }
        for &length in &self.strip_lengths {
            let (strip, tail) = remaining.split_at(length);
            remaining = tail;
            for index in 0..length.checked_sub(2).unwrap_or(0) {
                ctx.charge_retained(
                    std::mem::size_of::<[u32; 3]>() as u64,
                    "catia_trim_triangles",
                )?;
                crate::resource::push(
                    ctx,
                    &mut triangles,
                    if index % 2 == 0 {
                        [strip[index], strip[index + 1], strip[index + 2]]
                    } else {
                        [strip[index + 1], strip[index], strip[index + 2]]
                    },
                    "catia_trim_triangles",
                )?;
            }
        }
        for &length in &self.fan_lengths {
            let (fan, tail) = remaining.split_at(length);
            remaining = tail;
            for index in 1..length.checked_sub(1).unwrap_or(0) {
                ctx.charge_retained(
                    std::mem::size_of::<[u32; 3]>() as u64,
                    "catia_trim_triangles",
                )?;
                crate::resource::push(
                    ctx,
                    &mut triangles,
                    [fan[0], fan[index], fan[index + 1]],
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
