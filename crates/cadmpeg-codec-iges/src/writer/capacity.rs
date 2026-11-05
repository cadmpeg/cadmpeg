// SPDX-License-Identifier: Apache-2.0
//! Fixed ASCII card field capacities.

/// Stored-field capacities of the Fixed ASCII card layout (#217 §5.0).
/// The payload row holds the 72 section-data bytes and the sequence row the
/// seven-column decimal field (`docs/formats/iges.md` §33).
pub(super) const CAPACITY_FIELDS: &[cadmpeg_ir::codec::write::CapacityField] = &[
    cadmpeg_ir::codec::write::CapacityField {
        field: "card.payload",
        limit: 72,
        source: "docs/formats/iges.md §33: card bytes 1 through 72 are section data",
        helper: "card_payload",
    },
    cadmpeg_ir::codec::write::CapacityField {
        field: "card.sequence",
        limit: 9_999_999,
        source: "docs/formats/iges.md §33: bytes 74 through 80 are the decimal sequence field",
        helper: "card_sequence",
    },
];

/// The section data fits the 72-byte card payload.
pub(super) fn card_payload(data: &[u8]) -> Result<(), cadmpeg_ir::codec::write::EncodeError> {
    let value = cadmpeg_core::decode::u64_from_index(data.len());
    let field = &CAPACITY_FIELDS[0];
    if value > field.limit {
        return Err(cadmpeg_ir::codec::write::EncodeError::TargetCapacity {
            field,
            limit: field.limit,
            value,
        });
    }
    Ok(())
}

/// The card sequence fits the seven-column decimal field.
pub(super) fn card_sequence(sequence: u32) -> Result<u32, cadmpeg_ir::codec::write::EncodeError> {
    let value = u64::from(sequence);
    let field = &CAPACITY_FIELDS[1];
    if value > field.limit {
        return Err(cadmpeg_ir::codec::write::EncodeError::TargetCapacity {
            field,
            limit: field.limit,
            value,
        });
    }
    Ok(sequence)
}
