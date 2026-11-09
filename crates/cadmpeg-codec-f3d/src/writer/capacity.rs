// SPDX-License-Identifier: Apache-2.0
//! Stored-field capacities of the F3D writer (#217 §5.0).

use cadmpeg_ir::codec::write::{CapacityField, EncodeError};

/// Format capacity rows: each stored field whose width bounds the write.
pub(crate) const CAPACITY_FIELDS: &[CapacityField] = &[CapacityField {
    field: "asm.face.asm_face_key",
    limit: 9_223_372_036_854_775_807,
    source: "docs/formats/asm.md §5: face chunk[1] stores the asm_face_key as an i64",
    helper: "asm_face_key",
}];

/// The face key as its signed 64-bit face field stores it.
pub(crate) fn asm_face_key(key: u64) -> Result<i64, EncodeError> {
    let field = &CAPACITY_FIELDS[0];
    i64::try_from(key).map_err(|_| EncodeError::TargetCapacity {
        field,
        limit: field.limit,
        value: key,
    })
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::codec::write::EncodeError;

    #[test]
    fn the_face_key_helper_admits_its_limit_and_refuses_the_next_value() {
        let limit = super::CAPACITY_FIELDS[0].limit;
        assert_eq!(
            super::asm_face_key(limit).ok().map(i64::cast_unsigned),
            Some(limit)
        );
        assert!(matches!(
            super::asm_face_key(limit + 1),
            Err(EncodeError::TargetCapacity { field, limit: refused, value })
                if field == &super::CAPACITY_FIELDS[0] && refused == limit && value == limit + 1
        ));
    }
}
