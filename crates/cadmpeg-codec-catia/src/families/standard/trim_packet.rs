//! Checked trim-handle partitions and primitive expansion.

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TrimPacket {
    independent_count: usize,
    strip_lengths: Vec<usize>,
    fan_lengths: Vec<usize>,
    handles: Vec<u32>,
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
        })
    }
}

impl TrimPacket {
    pub(crate) fn handles(&self) -> &[u32] {
        &self.handles
    }

    #[cfg(test)]
    pub(crate) fn independent_count(&self) -> usize {
        self.independent_count
    }

    #[cfg(test)]
    pub(crate) fn strip_lengths(&self) -> &[usize] {
        &self.strip_lengths
    }

    #[cfg(test)]
    pub(crate) fn fan_lengths(&self) -> &[usize] {
        &self.fan_lengths
    }

    pub(crate) fn triangles(&self) -> Vec<[u32; 3]> {
        let mut triangles = Vec::new();
        let (independent, mut remaining) = self.handles.split_at(3 * self.independent_count);
        for triple in independent.chunks_exact(3) {
            triangles.push([triple[0], triple[1], triple[2]]);
        }
        for &length in &self.strip_lengths {
            let (strip, tail) = remaining.split_at(length);
            remaining = tail;
            for index in 0..length.saturating_sub(2) {
                triangles.push(if index % 2 == 0 {
                    [strip[index], strip[index + 1], strip[index + 2]]
                } else {
                    [strip[index + 1], strip[index], strip[index + 2]]
                });
            }
        }
        for &length in &self.fan_lengths {
            let (fan, tail) = remaining.split_at(length);
            remaining = tail;
            for index in 1..length.saturating_sub(1) {
                triangles.push([fan[0], fan[index], fan[index + 1]]);
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
    fn packet_preserves_independent_strip_and_fan_order() {
        let packet = TrimPacket::try_from((1, vec![4, 1], vec![4], (0..12).collect()))
            .expect("complete trim handle partition");
        assert_eq!(
            packet.triangles(),
            [[0, 1, 2], [3, 4, 5], [5, 4, 6], [8, 9, 10], [8, 10, 11]]
        );
    }
}
