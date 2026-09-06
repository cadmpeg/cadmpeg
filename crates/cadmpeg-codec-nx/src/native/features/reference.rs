// SPDX-License-Identifier: Apache-2.0
//! A construction reference with its resolved target and source position.

use crate::om::reference_index::ReferenceIndexToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConstructionReference<B> {
    pub(crate) token: ReferenceIndexToken,
    pub(crate) data_block: B,
    pub(crate) source_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NullableConstructionReference {
    pub(crate) target: Option<(ReferenceIndexToken, Option<String>)>,
    pub(crate) source_offset: u64,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct DeleteReferenceFieldWire {
    id: String,
    operation_label: String,
    control: u8,
    object_indices: [Option<u32>; 5],
    raw_object_indices: [Vec<u8>; 5],
    data_blocks: [Option<String>; 5],
    source_offset: u64,
    object_index_source_offsets: [u64; 5],
}

impl From<super::FeatureDeleteReferenceField> for DeleteReferenceFieldWire {
    fn from(value: super::FeatureDeleteReferenceField) -> Self {
        Self {
            id: value.id,
            operation_label: value.operation_label,
            control: value.control,
            object_indices: value.references.each_ref().map(|slot| slot.target.as_ref().map(|target| target.0.value())),
            raw_object_indices: value.references.each_ref().map(|slot| slot.target.as_ref().map_or_else(|| vec![0xff], |target| target.0.raw().to_vec())),
            data_blocks: value.references.each_ref().map(|slot| slot.target.as_ref().and_then(|target| target.1.clone())),
            source_offset: value.source_offset,
            object_index_source_offsets: value.references.each_ref().map(|slot| slot.source_offset),
        }
    }
}

impl TryFrom<DeleteReferenceFieldWire> for super::FeatureDeleteReferenceField {
    type Error = String;

    fn try_from(wire: DeleteReferenceFieldWire) -> Result<Self, Self::Error> {
        let slots = std::array::from_fn::<_, 5, _>(|slot| {
            let target = match wire.object_indices[slot] {
                Some(index) => Some((
                    ReferenceIndexToken::from_wire(index, &wire.raw_object_indices[slot])
                        .map_err(|error| format!("object_indices[{slot}]: {error}"))?,
                    wire.data_blocks[slot].clone(),
                )),
                None => {
                    if wire.raw_object_indices[slot] != [0xff] {
                        return Err(format!("raw_object_indices[{slot}]: null reference requires ff"));
                    }
                    if wire.data_blocks[slot].is_some() {
                        return Err(format!("data_blocks[{slot}]: null reference cannot have a target"));
                    }
                    None
                }
            };
            Ok(NullableConstructionReference {
                target,
                source_offset: wire.object_index_source_offsets[slot],
            })
        });
        let [a, b, c, d, e] = slots;
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            control: wire.control,
            references: [a?, b?, c?, d?, e?],
            source_offset: wire.source_offset,
        })
    }
}
