// SPDX-License-Identifier: Apache-2.0
//! Identity framing for the pre-`7C05` design stream.

const CATALOG_OPEN: &[u8] = b"\xde\x04\xfe\xfe\x12CATCatalogManager";

/// One stored entity identity in a legacy identity run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyEntityIdentity {
    /// Offset of the `EA` identity delimiter.
    pub offset: usize,
    /// Little-endian identity following the delimiter.
    pub entity_id: u32,
}

/// A monotonically identified legacy run terminated by its schema catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyEntityRun {
    /// Offset of the fixed catalog opening production.
    pub catalog_offset: usize,
    /// Stored identities in source order.
    pub identities: Vec<LegacyEntityIdentity>,
}

/// Parse complete legacy identity runs terminated by the fixed schema-catalog opener.
#[must_use]
pub fn parse_runs(data: &[u8]) -> Vec<LegacyEntityRun> {
    memchr::memmem::find_iter(data, CATALOG_OPEN)
        .filter_map(|catalog_offset| parse_run_before(data, catalog_offset))
        .collect()
}

fn parse_run_before(data: &[u8], catalog_offset: usize) -> Option<LegacyEntityRun> {
    let mut identities = data[..catalog_offset]
        .windows(6)
        .enumerate()
        .filter_map(|(offset, bytes)| {
            if bytes[0] != 0xea || bytes[5] != 0x81 {
                return None;
            }
            let entity_id = u32::from_le_bytes(bytes[1..5].try_into().ok()?);
            (entity_id != 0).then_some(LegacyEntityIdentity { offset, entity_id })
        })
        .collect::<Vec<_>>();
    identities.last()?;
    let suffix_start = identities
        .windows(2)
        .rposition(|pair| pair[0].entity_id >= pair[1].entity_id)
        .map_or(0, |index| index + 1);
    identities.drain(..suffix_start);
    (identities.first()?.entity_id == 1).then_some(LegacyEntityRun {
        catalog_offset,
        identities,
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_runs, CATALOG_OPEN};

    fn identity(bytes: &mut Vec<u8>, entity_id: u32) {
        bytes.push(0xea);
        bytes.extend_from_slice(&entity_id.to_le_bytes());
        bytes.push(0x81);
        bytes.extend_from_slice(&[0xfd, 0x8c]);
    }

    #[test]
    fn parses_monotone_identity_suffix_before_legacy_catalog() {
        let mut bytes = vec![0xea, 9, 0, 0, 0, 0x81];
        identity(&mut bytes, 1);
        identity(&mut bytes, 4);
        identity(&mut bytes, 7);
        let catalog_offset = bytes.len();
        bytes.extend_from_slice(CATALOG_OPEN);

        let runs = parse_runs(&bytes);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].catalog_offset, catalog_offset);
        assert_eq!(
            runs[0]
                .identities
                .iter()
                .map(|identity| identity.entity_id)
                .collect::<Vec<_>>(),
            [1, 4, 7]
        );
    }

    #[test]
    fn rejects_suffix_that_does_not_begin_with_identity_one() {
        let mut bytes = Vec::new();
        identity(&mut bytes, 1);
        identity(&mut bytes, 4);
        identity(&mut bytes, 2);
        bytes.extend_from_slice(CATALOG_OPEN);

        assert!(parse_runs(&bytes).is_empty());
    }
}
