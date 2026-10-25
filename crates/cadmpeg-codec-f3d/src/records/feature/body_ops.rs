// SPDX-License-Identifier: Apache-2.0
//! Whole-body operations: scale and copy-paste bodies.

use crate::records::identity::Sourced;
use crate::records::references::DesignClassTag;
use crate::records::serde_column::SliceColumn;
use crate::records::source_offsets::{OffsetVisitor, SourceOffsets};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(
    deserialize_center_position,
    [FiniteReal; 3],
    "center_position"
);
/// Fixed construction carried by a uniform body-scale scope.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignScaleOperationWire",
    into = "DesignScaleOperationWire"
)]
pub(crate) struct DesignScaleOperation {
    /// Counted construction group selecting the transformed bodies.
    pub(crate) body_group_record_index: u32,
    /// Native reference selecting the fixed scale center.
    pub(crate) center_record_index: u32,
    /// Explicit center position carried by legacy point-data centers, in source
    /// model centimetres.
    pub(crate) center_position: Option<Sourced<[FiniteReal; 3]>>,
    /// Uniform scale factor.
    pub(crate) uniform_factor: PositiveReal,
    /// Byte offset of `uniform_factor` of a located scope.
    pub(crate) uniform_factor_offset: Option<u64>,
}

#[derive(Serialize, Deserialize)]
struct DesignScaleOperationWire {
    /// Counted construction group selecting the transformed bodies.
    body_group_record_index: u32,
    /// Native reference selecting the fixed scale center.
    center_record_index: u32,
    /// Explicit center position carried by legacy point-data centers, in source
    /// model centimetres.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_center_position"
    )]
    center_position: Option<[FiniteReal; 3]>,
    /// Positive uniform scale factor.
    uniform_factor: f64,
}

impl From<DesignScaleOperation> for DesignScaleOperationWire {
    fn from(value: DesignScaleOperation) -> Self {
        Self {
            body_group_record_index: value.body_group_record_index,
            center_record_index: value.center_record_index,
            center_position: value.center_position.map(|center| center.value),
            uniform_factor: value.uniform_factor.get(),
        }
    }
}

impl TryFrom<DesignScaleOperationWire> for DesignScaleOperation {
    type Error = String;
    fn try_from(value: DesignScaleOperationWire) -> Result<Self, Self::Error> {
        Ok(Self {
            body_group_record_index: value.body_group_record_index,
            center_record_index: value.center_record_index,
            center_position: value.center_position.map(|value| Sourced {
                value,
                offset: None,
            }),
            uniform_factor: PositiveReal::new(value.uniform_factor)
                .ok_or("uniform_factor must be positive and finite")?,
            uniform_factor_offset: None,
        })
    }
}

impl SourceOffsets for DesignScaleOperation {
    fn visit_source_offsets(&mut self, offsets: &mut OffsetVisitor<'_>) {
        if let Some(center) = &mut self.center_position {
            offsets.sourced("center_position_offset", center);
        }
        offsets.offset("uniform_factor_offset", &mut self.uniform_factor_offset);
    }
}

/// Source and copied Design body identities carried by `CopyPasteBodies`.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignCopyPasteBodiesOperationWire")]
pub(crate) struct DesignCopyPasteBodiesOperation {
    bodies: Vec<DesignCopiedBody>,
    /// Counted body-selection group named by the scope prefix and reference table.
    pub(crate) body_group_record_index: u32,
    /// Dynamic class tag of the body group's primary header.
    pub(crate) body_group_class_tag: DesignClassTag,
    /// Byte offset of the body group's primary header of a located scope.
    body_group_byte_offset: Option<u64>,
    /// Indexed source-to-copy relation record named by the scope prefix.
    pub(crate) relation_record_index: u32,
    /// Dynamic class tag of the relation record's primary header.
    pub(crate) relation_class_tag: DesignClassTag,
    /// Byte offset of the relation record's primary header of a located
    /// scope.
    relation_byte_offset: Option<u64>,
}

#[cfg(test)]
thread_local! {
    static COPY_PASTE_BODIES_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignCopyPasteBodiesOperation {
    fn clone(&self) -> Self {
        #[cfg(test)]
        COPY_PASTE_BODIES_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            bodies: self.bodies.clone(),
            body_group_record_index: self.body_group_record_index,
            body_group_class_tag: self.body_group_class_tag.clone(),
            body_group_byte_offset: self.body_group_byte_offset,
            relation_record_index: self.relation_record_index,
            relation_class_tag: self.relation_class_tag.clone(),
            relation_byte_offset: self.relation_byte_offset,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DesignCopiedBody {
    pub(crate) operand: Sourced<u32>,
    pub(crate) source: Sourced<u32>,
    pub(crate) copied: Sourced<u32>,
}

/// Source and copied Design body identities carried by `CopyPasteBodies`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignCopyPasteBodiesOperationWire {
    /// Counted body-selection group named by the scope prefix and reference table.
    body_group_record_index: u32,
    /// Dynamic class tag of the body group's primary header.
    body_group_class_tag: String,
    /// Ordered body-operand records carried by the counted group.
    body_operand_record_indices: Vec<u32>,
    /// Indexed source-to-copy relation record named by the scope prefix.
    relation_record_index: u32,
    /// Dynamic class tag of the relation record's primary header.
    relation_class_tag: String,
    /// Source Design body entity suffixes in copy order.
    source_body_entity_suffixes: Vec<u32>,
    /// Newly copied Design body entity suffixes parallel to the sources.
    copied_body_entity_suffixes: Vec<u32>,
}

#[derive(Serialize)]
struct DesignCopyPasteBodiesOperationWireRef<'a> {
    body_group_record_index: u32,
    body_group_class_tag: &'a str,
    body_operand_record_indices: SliceColumn<'a, DesignCopiedBody, u32>,
    relation_record_index: u32,
    relation_class_tag: &'a str,
    source_body_entity_suffixes: SliceColumn<'a, DesignCopiedBody, u32>,
    copied_body_entity_suffixes: SliceColumn<'a, DesignCopiedBody, u32>,
}

impl Serialize for DesignCopyPasteBodiesOperation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let bodies = self.bodies.as_slice();
        DesignCopyPasteBodiesOperationWireRef {
            body_group_record_index: self.body_group_record_index,
            body_group_class_tag: self.body_group_class_tag.as_str(),
            body_operand_record_indices: SliceColumn::new(bodies, |body| body.operand.value),
            relation_record_index: self.relation_record_index,
            relation_class_tag: self.relation_class_tag.as_str(),
            source_body_entity_suffixes: SliceColumn::new(bodies, |body| body.source.value),
            copied_body_entity_suffixes: SliceColumn::new(bodies, |body| body.copied.value),
        }
        .serialize(serializer)
    }
}

pub(crate) struct CopyPasteRecordLocation {
    pub(crate) record_index: u32,
    pub(crate) class_tag: DesignClassTag,
    pub(crate) byte_offset: u64,
}

impl DesignCopyPasteBodiesOperation {
    /// Admit a decoded operation, whose body offsets follow the body group
    /// and relation record strides.
    pub(crate) fn try_new(
        bodies: Vec<DesignCopiedBody>,
        body_group: CopyPasteRecordLocation,
        relation: CopyPasteRecordLocation,
    ) -> Result<Self, CodecError> {
        let operation = Self::from_parts(
            bodies,
            body_group.record_index,
            body_group.class_tag,
            relation.record_index,
            relation.class_tag,
        )
        .map_err(CodecError::Malformed)?;
        let operation = Self {
            body_group_byte_offset: Some(body_group.byte_offset),
            relation_byte_offset: Some(relation.byte_offset),
            ..operation
        };
        if !operation.source_layout_valid() {
            return Err(CodecError::Malformed(
                "bodies operand, source, and copied offsets must follow their record strides"
                    .into(),
            ));
        }
        Ok(operation)
    }

    fn from_parts(
        bodies: Vec<DesignCopiedBody>,
        body_group_record_index: u32,
        body_group_class_tag: DesignClassTag,
        relation_record_index: u32,
        relation_class_tag: DesignClassTag,
    ) -> Result<Self, String> {
        if bodies.is_empty() {
            return Err("bodies must not be empty".into());
        }
        let mut suffixes = std::collections::HashSet::new();
        for body in &bodies {
            for suffix in [body.source.value, body.copied.value] {
                if !suffixes.insert(suffix) {
                    return Err("source and copied body suffixes must be pairwise distinct".into());
                }
            }
        }
        Ok(Self {
            bodies,
            body_group_record_index,
            body_group_class_tag,
            body_group_byte_offset: None,
            relation_record_index,
            relation_class_tag,
            relation_byte_offset: None,
        })
    }

    /// Whether the located body offsets follow their record strides: an
    /// operand reference every eleven bytes from the body group header at
    /// offset 26, and a source and a copied suffix every thirty bytes from the
    /// relation header at offsets 25 and 40.
    pub(crate) fn source_layout_valid(&self) -> bool {
        let (Some(body_group_byte_offset), Some(relation_byte_offset)) =
            (self.body_group_byte_offset, self.relation_byte_offset)
        else {
            return false;
        };
        self.bodies.iter().enumerate().all(|(ordinal, body)| {
            let ordinal = cadmpeg_core::decode::u64_from_index(ordinal);
            let operand_offset = ordinal
                .checked_mul(11)
                .and_then(|delta| body_group_byte_offset.checked_add(delta))
                .and_then(|offset| offset.checked_add(26));
            let source_offset = ordinal
                .checked_mul(30)
                .and_then(|delta| relation_byte_offset.checked_add(delta))
                .and_then(|offset| offset.checked_add(25));
            let copied_offset = source_offset.and_then(|offset| offset.checked_add(15));
            operand_offset.is_some()
                && body.operand.offset == operand_offset
                && source_offset.is_some()
                && body.source.offset == source_offset
                && copied_offset.is_some()
                && body.copied.offset == copied_offset
        })
    }
    pub(crate) fn bodies(&self) -> &[DesignCopiedBody] {
        &self.bodies
    }
    /// Byte offset of the body group's primary header of a located scope.
    #[cfg(test)]
    pub(crate) fn body_group_byte_offset(&self) -> Option<u64> {
        self.body_group_byte_offset
    }
    /// Byte offset of the relation record's primary header of a located
    /// scope.
    pub(crate) fn relation_byte_offset(&self) -> Option<u64> {
        self.relation_byte_offset
    }
}

impl SourceOffsets for DesignCopyPasteBodiesOperation {
    fn visit_source_offsets(&mut self, offsets: &mut OffsetVisitor<'_>) {
        offsets.offset("body_group_byte_offset", &mut self.body_group_byte_offset);
        offsets.offset("relation_byte_offset", &mut self.relation_byte_offset);
        for (index, body) in self.bodies.iter_mut().enumerate() {
            offsets.sourced(
                &format!("body_operand_record_offsets.{index}"),
                &mut body.operand,
            );
            offsets.sourced(
                &format!("source_body_entity_suffix_offsets.{index}"),
                &mut body.source,
            );
            offsets.sourced(
                &format!("copied_body_entity_suffix_offsets.{index}"),
                &mut body.copied,
            );
        }
    }
}

impl TryFrom<DesignCopyPasteBodiesOperationWire> for DesignCopyPasteBodiesOperation {
    type Error = String;
    fn try_from(wire: DesignCopyPasteBodiesOperationWire) -> Result<Self, Self::Error> {
        let count = wire.body_operand_record_indices.len();
        if wire.source_body_entity_suffixes.len() != count {
            return Err(
                "source_body_entity_suffixes must match body_operand_record_indices".into(),
            );
        }
        if wire.copied_body_entity_suffixes.len() != count {
            return Err(
                "copied_body_entity_suffixes must match body_operand_record_indices".into(),
            );
        }
        let mut bodies = {
            let mut storage = Vec::new();
            storage.try_reserve_exact(count).map(|()| storage)
        }
        .map_err(|error| error.to_string())?;
        let unlocated = |value| Sourced {
            value,
            offset: None,
        };
        bodies.extend(
            wire.body_operand_record_indices
                .into_iter()
                .zip(wire.source_body_entity_suffixes)
                .zip(wire.copied_body_entity_suffixes)
                .map(|((operand, source), copied)| DesignCopiedBody {
                    operand: unlocated(operand),
                    source: unlocated(source),
                    copied: unlocated(copied),
                }),
        );
        Self::from_parts(
            bodies,
            wire.body_group_record_index,
            wire.body_group_class_tag.try_into()?,
            wire.relation_record_index,
            wire.relation_class_tag.try_into()?,
        )
    }
}

#[cfg(test)]
impl From<DesignCopyPasteBodiesOperation> for DesignCopyPasteBodiesOperationWire {
    fn from(value: DesignCopyPasteBodiesOperation) -> Self {
        Self {
            body_group_record_index: value.body_group_record_index,
            body_group_class_tag: value.body_group_class_tag.into(),
            relation_record_index: value.relation_record_index,
            relation_class_tag: value.relation_class_tag.into(),
            body_operand_record_indices: value
                .bodies
                .iter()
                .map(|body| body.operand.value)
                .collect(),
            source_body_entity_suffixes: value
                .bodies
                .iter()
                .map(|body| body.source.value)
                .collect(),
            copied_body_entity_suffixes: value
                .bodies
                .iter()
                .map(|body| body.copied.value)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn operation() -> DesignCopyPasteBodiesOperation {
        serde_json::from_str(r#"{"body_group_record_index":501,"body_group_class_tag":"264","body_operand_record_indices":[502,504],"relation_record_index":503,"relation_class_tag":"264","source_body_entity_suffixes":[11,13],"copied_body_entity_suffixes":[12,14]}"#).unwrap()
    }

    #[test]
    fn copy_paste_bodies_borrowed_wire_matches_owned_wire_bytes() {
        let operation = operation();
        let owned = DesignCopyPasteBodiesOperationWire::from(operation.clone());
        assert_eq!(
            serde_json::to_vec(&operation).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }

    #[test]
    fn copy_paste_bodies_native_retained_limit_refuses_before_clone() {
        #[derive(Serialize)]
        struct NestedRecord<'a> {
            id: &'static str,
            value: &'a DesignCopyPasteBodiesOperation,
        }
        let operation = operation();
        let record = NestedRecord {
            id: "f3d:native:copy-paste-bodies#0",
            value: &operation,
        };
        crate::test_support::native_test::assert_borrowed_native_retained_limit(
            &record,
            "design_parameter_scopes",
            || COPY_PASTE_BODIES_CLONE_COUNT.with(|count| count.set(0)),
            || COPY_PASTE_BODIES_CLONE_COUNT.with(std::cell::Cell::get),
        );
    }
}

mod identity_rewrite;
