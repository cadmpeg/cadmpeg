// SPDX-License-Identifier: Apache-2.0
//! Terminal operation discriminator with derived compact-token positions.

use super::compact::CompactIndexAtom;
use super::operation_record::OperationPayload;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

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
    pub(crate) fn trailing_indices(
        &self,
    ) -> impl Iterator<Item = (CompactIndexAtom, u64)> + Clone + '_ {
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
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<Option<OperationTerminalDiscriminator>, CodecError> {
    if record.payload().last() != Some(&0) {
        return Ok(None);
    }
    ctx.charge_work(u64_from_index(record.payload().len()), "scan NX terminal discriminator")?;

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

        let origin = u64::try_from(record.payload_offset().checked_add(start)?).ok()?;
        let token_bytes = first.raw().len().checked_add(second.raw().len())?
            .checked_add(trailing_bytes.len())?;
        origin.checked_add(17)?.checked_add(u64_from_index(token_bytes))?;

        Some((origin, [first, second], flags, trailing_bytes, trailing_count))
    };

    let mut found = None;
    for start in 0..record.payload().len().saturating_sub(18) {
        if record.payload().get(start..start + 3) == Some(&[0x01, 0x01, 0x02]) {
            ctx.charge_work(u64_from_index(record.payload().len() - start), "scan NX terminal discriminator candidate")?;
        }
        let Some(candidate) = decode(start) else {
            continue;
        };
        if found.is_some() {
            return Ok(None);
        }
        found = Some(candidate);
    }
    let Some((origin, indices, flags, trailing_bytes, trailing_count)) = found else { return Ok(None); };
    let count = u64_from_index(trailing_count);
    let operation = "NX terminal discriminator trailing indices";
    ctx.charge_collection_items(count, operation)?;
    let bytes = count.checked_mul(u64_from_index(std::mem::size_of::<CompactIndexAtom>()))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, count))?;
    ctx.charge_retained(bytes, operation)?;
    let mut trailing_indices = Vec::new();
    trailing_indices.try_reserve_exact(trailing_count)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, count))?;
    let mut scan = 0;
    while scan < trailing_bytes.len() {
        let Some(tail) = trailing_bytes.get(scan..) else { return Ok(None); };
        let Some(token) = CompactIndexAtom::read(tail) else { return Ok(None); };
        scan += token.raw().len();
        trailing_indices.push(token);
    }
    Ok(OperationTerminalDiscriminator::new(origin, indices, flags, trailing_indices).ok())
}

#[cfg(test)]
mod tests {
    use super::super::operation_record::OperationPayload;
    use super::operation_terminal_discriminator;

    fn scan(record: OperationPayload<'_>) -> Option<super::OperationTerminalDiscriminator> {
        crate::test_support::with_decode_context(|ctx| operation_terminal_discriminator(ctx, record)).unwrap()
    }

    fn terminal_limit_error(policy: &cadmpeg_core::decode::DecodePolicy) -> cadmpeg_core::CodecError {
        let payload = b"\x01\x01\x02\x81\x5f\x80\xab\x01\x03\x02\x01\x01\x02\x01\x01\x00\x00\x00\x29\x29\x05\x80\xff\x00";
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(payload, &arena, policy).unwrap();
        let record = OperationPayload::new(payload, 200, "EXTRUDE").unwrap();
        operation_terminal_discriminator(&ctx, record).expect_err("terminal discriminator refusal")
    }

    #[test]
    fn om_terminal_discriminator_route_refuses_collection_limit() {
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        assert!(matches!(terminal_limit_error(&policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
    }

    #[test]
    fn om_terminal_discriminator_route_refuses_retained_limit() {
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;
        assert!(matches!(terminal_limit_error(&policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
    }

    #[test]
    fn om_terminal_discriminator_route_refuses_work_limit() {
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_work_units = 0;
        assert!(matches!(terminal_limit_error(&policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
    }

    #[test]
    fn terminal_frame_preserves_empty_and_zero_valued_trailing_lanes() {
        let mut payload = vec![
            1, 1, 2, 0, 0x80, 0, 1, 3, 2, 1, 0, 255, 128, 1, 0, 0, 0, 0x29, 0x29, 0,
        ];
        let frame = scan(OperationPayload::new(&payload, 100, "").unwrap()).unwrap();
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
        let frame = scan(OperationPayload::new(&payload, 100, "").unwrap()).unwrap();
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
