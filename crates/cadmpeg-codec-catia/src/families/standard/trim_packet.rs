//! Checked trim-handle partitions and primitive expansion.

use std::sync::{Arc, OnceLock};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

#[derive(Debug, Clone)]
pub(crate) struct TrimPacket {
    inner: Arc<TrimPacketData>,
}

#[derive(Debug)]
struct TrimPacketData {
    independent_count: usize,
    strip_lengths: Vec<usize>,
    fan_lengths: Vec<usize>,
    handles: Vec<u32>,
    triangles: OnceLock<Vec<[u32; 3]>>,
}

impl PartialEq for TrimPacket {
    fn eq(&self, other: &Self) -> bool {
        self.inner.independent_count == other.inner.independent_count
            && self.inner.strip_lengths == other.inner.strip_lengths
            && self.inner.fan_lengths == other.inner.fan_lengths
            && self.inner.handles == other.inner.handles
    }
}

impl TryFrom<(usize, Vec<usize>, Vec<usize>, Vec<u32>)> for TrimPacketData {
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

#[cfg(test)]
impl TryFrom<(usize, Vec<usize>, Vec<usize>, Vec<u32>)> for TrimPacket {
    type Error = &'static str;

    fn try_from(lanes: (usize, Vec<usize>, Vec<usize>, Vec<u32>)) -> Result<Self, Self::Error> {
        Ok(Self {
            inner: Arc::new(TrimPacketData::try_from(lanes)?),
        })
    }
}

impl TrimPacket {
    pub(super) fn from_lanes(
        independent_count: usize,
        strip_lengths: Vec<usize>,
        fan_lengths: Vec<usize>,
        handles: Vec<u32>,
    ) -> Result<Option<Self>, CodecError> {
        let Ok(inner) =
            TrimPacketData::try_from((independent_count, strip_lengths, fan_lengths, handles))
        else {
            return Ok(None);
        };

        Ok(Some(Self {
            inner: Arc::new(inner),
        }))
    }
    pub(crate) fn try_clone_with_context(&self) -> Result<Self, CodecError> {
        Ok(self.clone())
    }

    pub(crate) fn handles(&self) -> &[u32] {
        &self.inner.handles
    }

    #[cfg(test)]
    pub(super) fn independent_count(&self) -> usize {
        self.inner.independent_count
    }

    #[cfg(test)]
    pub(super) fn strip_lengths(&self) -> &[usize] {
        &self.inner.strip_lengths
    }

    #[cfg(test)]
    pub(super) fn fan_lengths(&self) -> &[usize] {
        &self.inner.fan_lengths
    }

    pub(crate) fn triangles(&self, ctx: &DecodeContext<'_>) -> Result<&[[u32; 3]], CodecError> {
        if let Some(triangles) = self.inner.triangles.get() {
            return Ok(triangles);
        }
        let triangles = self.expand_triangles()?;
        match self.inner.triangles.set(triangles) {
            Ok(()) | Err(_) => self
                .inner
                .triangles
                .get()
                .map(Vec::as_slice)
                .ok_or_else(|| ctx.refuse_codec_limit("catia_trim_triangles", u64::MAX, u64::MAX)),
        }
    }

    fn expand_triangles(&self) -> Result<Vec<[u32; 3]>, CodecError> {
        let mut triangles = Vec::new();
        let (independent, mut remaining) = self
            .inner
            .handles
            .split_at(3 * self.inner.independent_count);
        for triple in independent.chunks_exact(3) {
            triangles.push([triple[0], triple[1], triple[2]]);
        }
        for &length in &self.inner.strip_lengths {
            let (strip, tail) = remaining.split_at(length);
            remaining = tail;
            for (index, triple) in strip.windows(3).enumerate() {
                triangles.push(if index % 2 == 0 {
                    [triple[0], triple[1], triple[2]]
                } else {
                    [triple[1], triple[0], triple[2]]
                });
            }
        }
        for &length in &self.inner.fan_lengths {
            let (fan, tail) = remaining.split_at(length);
            remaining = tail;
            let Some((&center, rim)) = fan.split_first() else {
                continue;
            };
            for pair in rim.windows(2) {
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
        assert!(packet.inner.triangles.get().is_none());
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
        assert!(packet.inner.triangles.get().is_none());
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
        assert!(packet.inner.triangles.get().is_some());
        assert!(cold.inner.triangles.get().is_some());
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
        assert!(packet.inner.triangles.get().is_none());
    }
    #[test]
    fn cloned_trim_packet_shares_handles_and_expansion() {
        let packet =
            TrimPacket::try_from((1, vec![], vec![], vec![1, 2, 3])).expect("complete partition");
        crate::test_support::with_service_context(|ctx| packet.triangles(ctx).map(|_| ()))
            .expect("initial expansion");
        let cloned =
            crate::test_support::with_collection_limit(0, |ctx| packet.try_clone_with_context())
                .expect("sharing immutable lanes allocates no collection items");
        assert!(std::ptr::eq(packet.handles(), cloned.handles()));
        crate::test_support::with_collection_limit(0, |ctx| {
            assert!(std::ptr::eq(
                packet.triangles(ctx).expect("cached original"),
                cloned.triangles(ctx).expect("shared cache")
            ));
        });
    }
}
