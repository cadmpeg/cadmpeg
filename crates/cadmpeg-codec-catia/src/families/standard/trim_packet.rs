//! Checked trim-handle partitions and primitive expansion.

use std::sync::{Arc, OnceLock};

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
    ) -> Option<Self> {
        let Ok(inner) =
            TrimPacketData::try_from((independent_count, strip_lengths, fan_lengths, handles))
        else {
            return None;
        };

        Some(Self {
            inner: Arc::new(inner),
        })
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

    pub(crate) fn triangles(&self) -> &[[u32; 3]] {
        self.inner.triangles.get_or_init(|| self.expand_triangles())
    }

    fn expand_triangles(&self) -> Vec<[u32; 3]> {
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
        triangles
    }
}

#[cfg(test)]
mod tests {
    use super::TrimPacket;

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
        let first = packet.triangles();
        assert_eq!(first.len(), 5);
        assert!(std::ptr::eq(first, packet.triangles()));
        assert!(std::ptr::eq(first, packet.triangles()));
        assert!(packet.inner.triangles.get().is_some());
        assert!(cold.inner.triangles.get().is_some());
        assert_eq!(packet, cold);
    }

    #[test]
    fn packet_preserves_independent_strip_and_fan_order() {
        let packet = TrimPacket::try_from((1, vec![4, 1], vec![4], (0..12).collect()))
            .expect("complete trim handle partition");
        assert_eq!(
            packet.triangles().to_vec(),
            [[0, 1, 2], [3, 4, 5], [5, 4, 6], [8, 9, 10], [8, 10, 11]]
        );
    }

    #[test]
    fn cloned_trim_packet_shares_handles_and_expansion() {
        let packet =
            TrimPacket::try_from((1, vec![], vec![], vec![1, 2, 3])).expect("complete partition");
        packet.triangles();
        let cloned = packet.clone();
        assert!(std::ptr::eq(packet.handles(), cloned.handles()));
        assert!(std::ptr::eq(packet.triangles(), cloned.triangles()));
    }
}
