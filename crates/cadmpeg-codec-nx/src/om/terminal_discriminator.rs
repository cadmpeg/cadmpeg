// SPDX-License-Identifier: Apache-2.0
//! Terminal operation discriminator with derived compact-token positions.

use super::compact::CompactIndexAtom;
use super::operation_record::OperationPayload;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OperationTerminalDiscriminator {
    origin: u64,
    type_indices: [CompactIndexAtom; 2],
    flags: [u8; 4],
    trailing_indices: Vec<CompactIndexAtom>,
}

impl OperationTerminalDiscriminator {
    pub(crate) fn new(
        origin: u64,
        type_indices: [CompactIndexAtom; 2],
        flags: [u8; 4],
        trailing_indices: Vec<CompactIndexAtom>,
    ) -> Result<Self, &'static str> {
        let start = origin
            .checked_add(17)
            .ok_or("source_offset: terminal discriminator overflows")?;
        type_indices
            .iter()
            .chain(&trailing_indices)
            .try_fold(start, |at, token| at.checked_add(token.raw().len() as u64))
            .ok_or("source_offset: terminal discriminator end overflows")?;
        Ok(Self {
            origin,
            type_indices,
            flags,
            trailing_indices,
        })
    }
    pub(crate) fn origin(&self) -> u64 {
        self.origin
    }
    pub(crate) fn flags(&self) -> [u8; 4] {
        self.flags
    }
    pub(crate) fn type_indices(&self) -> [(CompactIndexAtom, u64); 2] {
        let mut at = self.origin + 3;
        self.type_indices.map(|token| {
            let offset = at;
            at += token.raw().len() as u64;
            (token, offset)
        })
    }
    pub(crate) fn trailing_indices(&self) -> impl Iterator<Item = (CompactIndexAtom, u64)> + '_ {
        let mut at = self.origin
            + 16
            + self
                .type_indices
                .iter()
                .map(|token| token.raw().len() as u64)
                .sum::<u64>();
        self.trailing_indices.iter().map(move |token| {
            let offset = at;
            at += token.raw().len() as u64;
            (*token, offset)
        })
    }
    pub(crate) fn relocate(self, base: u64) -> Option<Self> {
        Self::new(
            base.checked_add(self.origin)?,
            self.type_indices,
            self.flags,
            self.trailing_indices,
        )
        .ok()
    }
}

/// Decode the unique terminal discriminator lane in a bounded operation payload.
pub(crate) fn operation_terminal_discriminator(
    record: OperationPayload<'_>,
) -> Option<OperationTerminalDiscriminator> {
    if record.payload().last() != Some(&0) {
        return None;
    }

    let decode = |start: usize| {
        if record.payload().get(start..start + 3) != Some(&[0x01, 0x01, 0x02]) {
            return None;
        }
        let mut at = start + 3;
        let first = CompactIndexAtom::read(record.payload().get(at..)?)?;
        at += first.raw().len();
        let second = CompactIndexAtom::read(record.payload().get(at..)?)?;
        at += second.raw().len();
        if record.payload().get(at..at + 4) != Some(&[0x01, 0x03, 0x02, 0x01]) {
            return None;
        }
        at += 4;
        let flags = record
            .payload()
            .get(at..at + 4)
            .and_then(|bytes| bytes.try_into().ok())?;
        at += 4;
        if record.payload().get(at..at + 5) != Some(&[0x00, 0x00, 0x00, 0x29, 0x29]) {
            return None;
        }
        at += 5;

        let trailing_end = record.payload().len() - 1;
        let trailing_bytes = record.payload().get(at..trailing_end)?;
        let mut scan = 0;
        let mut trailing_count = 0;
        while scan < trailing_bytes.len() {
            let token = CompactIndexAtom::read(trailing_bytes.get(scan..)?)?;
            scan += token.raw().len();
            trailing_count += 1;
        }

        // Candidates are scanned at every payload offset. Validate the bounded
        // trailing bytes before allocating their owned representation.
        let mut trailing_indices = Vec::with_capacity(trailing_count);
        let mut scan = 0;
        while scan < trailing_bytes.len() {
            let token = CompactIndexAtom::read(trailing_bytes.get(scan..)?)?;
            scan += token.raw().len();
            trailing_indices.push(token);
        }

        OperationTerminalDiscriminator::new(
            (record.payload_offset() + start) as u64,
            [first, second],
            flags,
            trailing_indices,
        )
        .ok()
    };

    let mut found = None;
    for start in 0..record.payload().len().saturating_sub(18) {
        let Some(lane) = decode(start) else {
            continue;
        };
        if found.is_some() {
            return None;
        }
        found = Some(lane);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_frame_preserves_empty_and_zero_valued_trailing_lanes() {
        let mut payload = vec![
            1, 1, 2, 0, 0x80, 0, 1, 3, 2, 1, 0, 255, 128, 1, 0, 0, 0, 0x29, 0x29, 0,
        ];
        let frame =
            operation_terminal_discriminator(OperationPayload::new(&payload, 100, "").unwrap())
                .unwrap();
        assert_eq!(
            frame
                .type_indices()
                .map(|(token, offset)| (token.value(), offset)),
            [(0, 103), (0, 104)]
        );
        assert_eq!(frame.flags(), [0, 255, 128, 1]);
        assert_eq!(frame.trailing_indices().count(), 0);
        assert!(frame.clone().relocate(u64::MAX - 120).is_some());
        assert!(frame.relocate(u64::MAX - 119).is_none());
        payload.pop();
        payload.extend([0, 0x90, 0, 0]);
        let frame =
            operation_terminal_discriminator(OperationPayload::new(&payload, 100, "").unwrap())
                .unwrap();
        assert_eq!(
            frame
                .trailing_indices()
                .map(|(token, offset)| (token.value(), offset))
                .collect::<Vec<_>>(),
            [(0, 119), (4096, 120)]
        );
        assert_eq!(
            frame
                .clone()
                .relocate(1000)
                .unwrap()
                .trailing_indices()
                .map(|(_, offset)| offset)
                .collect::<Vec<_>>(),
            [1119, 1120]
        );
        assert!(frame.clone().relocate(u64::MAX - 123).is_some());
        assert!(frame.relocate(u64::MAX - 122).is_none());
    }
}
