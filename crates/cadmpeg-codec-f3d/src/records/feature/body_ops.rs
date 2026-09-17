// SPDX-License-Identifier: Apache-2.0
//! Whole-body operations: scale and copy-paste bodies.

use crate::records::identity::Located;
use crate::records::references::DesignClassTag;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(deserialize_center_position, [f64; 3], "center_position");
cadmpeg_core::named_optional_field!(
    deserialize_center_position_offset,
    u64,
    "center_position_offset"
);
/// Fixed construction carried by a uniform body-scale scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignScaleOperationWire",
    into = "DesignScaleOperationWire"
)]
pub struct DesignScaleOperation {
    /// Counted construction group selecting the transformed bodies.
    pub body_group_record_index: u32,
    /// Native reference selecting the fixed scale center.
    pub center_record_index: u32,
    /// Explicit center position carried by legacy point-data centers, in source
    /// model centimetres.
    pub center_position: Option<Located<[f64; 3]>>,
    /// Positive uniform scale factor.
    pub uniform_factor: f64,
    /// Byte offset of `uniform_factor`.
    pub uniform_factor_offset: u64,
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
    center_position: Option<[f64; 3]>,
    /// Byte offset of the explicit center position.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_center_position_offset"
    )]
    center_position_offset: Option<u64>,
    /// Positive uniform scale factor.
    uniform_factor: f64,
    /// Byte offset of `uniform_factor`.
    uniform_factor_offset: u64,
}

impl From<DesignScaleOperation> for DesignScaleOperationWire {
    fn from(value: DesignScaleOperation) -> Self {
        Self {
            body_group_record_index: value.body_group_record_index,
            center_record_index: value.center_record_index,
            center_position: value.center_position.map(|center| center.value),
            center_position_offset: value.center_position.map(|center| center.offset),
            uniform_factor: value.uniform_factor,
            uniform_factor_offset: value.uniform_factor_offset,
        }
    }
}

impl TryFrom<DesignScaleOperationWire> for DesignScaleOperation {
    type Error = String;
    fn try_from(value: DesignScaleOperationWire) -> Result<Self, Self::Error> {
        Ok(Self {
            body_group_record_index: value.body_group_record_index,
            center_record_index: value.center_record_index,
            center_position: Located::from_wire(
                value.center_position,
                value.center_position_offset,
                "center_position",
            )?,
            uniform_factor: value.uniform_factor,
            uniform_factor_offset: value.uniform_factor_offset,
        })
    }
}

/// Source and copied Design body identities carried by `CopyPasteBodies`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignCopyPasteBodiesOperationWire",
    into = "DesignCopyPasteBodiesOperationWire"
)]
pub struct DesignCopyPasteBodiesOperation {
    bodies: Vec<DesignCopiedBody>,
    /// Counted body-selection group named by the scope prefix and reference table.
    pub body_group_record_index: u32,
    /// Dynamic class tag of the body group's primary header.
    pub body_group_class_tag: DesignClassTag,
    /// Byte offset of the body group's primary header.
    body_group_byte_offset: u64,
    /// Indexed source-to-copy relation record named by the scope prefix.
    pub relation_record_index: u32,
    /// Dynamic class tag of the relation record's primary header.
    pub relation_class_tag: DesignClassTag,
    /// Byte offset of the relation record's primary header.
    relation_byte_offset: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesignCopiedBody {
    pub operand: Located<u32>,
    pub source: Located<u32>,
    pub copied: Located<u32>,
}

/// Source and copied Design body identities carried by `CopyPasteBodies`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignCopyPasteBodiesOperationWire {
    /// Counted body-selection group named by the scope prefix and reference table.
    body_group_record_index: u32,
    /// Dynamic class tag of the body group's primary header.
    body_group_class_tag: String,
    /// Byte offset of the body group's primary header.
    body_group_byte_offset: u64,
    /// Ordered body-operand records carried by the counted group.
    body_operand_record_indices: Vec<u32>,
    /// Byte offsets parallel to `body_operand_record_indices`.
    body_operand_record_offsets: Vec<u64>,
    /// Indexed source-to-copy relation record named by the scope prefix.
    relation_record_index: u32,
    /// Dynamic class tag of the relation record's primary header.
    relation_class_tag: String,
    /// Byte offset of the relation record's primary header.
    relation_byte_offset: u64,
    /// Source Design body entity suffixes in copy order.
    source_body_entity_suffixes: Vec<u32>,
    /// Byte offsets parallel to `source_body_entity_suffixes`.
    source_body_entity_suffix_offsets: Vec<u64>,
    /// Newly copied Design body entity suffixes parallel to the sources.
    copied_body_entity_suffixes: Vec<u32>,
    /// Byte offsets parallel to `copied_body_entity_suffixes`.
    copied_body_entity_suffix_offsets: Vec<u64>,
}

impl DesignCopyPasteBodiesOperation {
    pub(crate) fn try_new(
        bodies: Vec<DesignCopiedBody>,
        body_group_record_index: u32,
        body_group_class_tag: DesignClassTag,
        body_group_byte_offset: u64,
        relation_record_index: u32,
        relation_class_tag: DesignClassTag,
        relation_byte_offset: u64,
    ) -> Result<Self, String> {
        if bodies.is_empty() {
            return Err("bodies must not be empty".into());
        }
        let mut suffixes = std::collections::HashSet::new();
        let mut operand_offset = body_group_byte_offset.saturating_add(26);
        let mut source_offset = relation_byte_offset.saturating_add(25);
        for body in &bodies {
            if !suffixes.insert(body.source.value) || !suffixes.insert(body.copied.value) {
                return Err("source and copied body suffixes must be pairwise distinct".into());
            }
            if body.operand.offset != operand_offset
                || body.source.offset != source_offset
                || body.copied.offset != source_offset.saturating_add(15)
            {
                return Err(
                    "bodies operand, source, and copied offsets must follow their record strides"
                        .into(),
                );
            }
            operand_offset = operand_offset.saturating_add(11);
            source_offset = source_offset.saturating_add(30);
        }
        Ok(Self {
            bodies,
            body_group_record_index,
            body_group_class_tag,
            body_group_byte_offset,
            relation_record_index,
            relation_class_tag,
            relation_byte_offset,
        })
    }
    pub(crate) fn bodies(&self) -> &[DesignCopiedBody] {
        &self.bodies
    }
    pub(crate) fn body_group_byte_offset(&self) -> u64 {
        self.body_group_byte_offset
    }
    pub(crate) fn relation_byte_offset(&self) -> u64 {
        self.relation_byte_offset
    }
}

impl TryFrom<DesignCopyPasteBodiesOperationWire> for DesignCopyPasteBodiesOperation {
    type Error = String;
    fn try_from(wire: DesignCopyPasteBodiesOperationWire) -> Result<Self, Self::Error> {
        let count = wire.body_operand_record_indices.len();
        if wire.body_operand_record_offsets.len() != count {
            return Err(
                "body_operand_record_offsets must match body_operand_record_indices".into(),
            );
        }
        if wire.source_body_entity_suffixes.len() != count {
            return Err(
                "source_body_entity_suffixes must match body_operand_record_indices".into(),
            );
        }
        if wire.source_body_entity_suffix_offsets.len() != count {
            return Err(
                "source_body_entity_suffix_offsets must match body_operand_record_indices".into(),
            );
        }
        if wire.copied_body_entity_suffixes.len() != count {
            return Err(
                "copied_body_entity_suffixes must match body_operand_record_indices".into(),
            );
        }
        if wire.copied_body_entity_suffix_offsets.len() != count {
            return Err(
                "copied_body_entity_suffix_offsets must match body_operand_record_indices".into(),
            );
        }
        let bodies = wire
            .body_operand_record_indices
            .into_iter()
            .zip(wire.body_operand_record_offsets)
            .zip(
                wire.source_body_entity_suffixes
                    .into_iter()
                    .zip(wire.source_body_entity_suffix_offsets),
            )
            .zip(
                wire.copied_body_entity_suffixes
                    .into_iter()
                    .zip(wire.copied_body_entity_suffix_offsets),
            )
            .map(
                |(((value, offset), (source, source_offset)), (copied, copied_offset))| {
                    DesignCopiedBody {
                        operand: Located { value, offset },
                        source: Located {
                            value: source,
                            offset: source_offset,
                        },
                        copied: Located {
                            value: copied,
                            offset: copied_offset,
                        },
                    }
                },
            )
            .collect();
        Self::try_new(
            bodies,
            wire.body_group_record_index,
            wire.body_group_class_tag.try_into()?,
            wire.body_group_byte_offset,
            wire.relation_record_index,
            wire.relation_class_tag.try_into()?,
            wire.relation_byte_offset,
        )
    }
}

impl From<DesignCopyPasteBodiesOperation> for DesignCopyPasteBodiesOperationWire {
    fn from(value: DesignCopyPasteBodiesOperation) -> Self {
        Self {
            body_group_record_index: value.body_group_record_index,
            body_group_class_tag: value.body_group_class_tag.into(),
            body_group_byte_offset: value.body_group_byte_offset,
            relation_record_index: value.relation_record_index,
            relation_class_tag: value.relation_class_tag.into(),
            relation_byte_offset: value.relation_byte_offset,
            body_operand_record_indices: value
                .bodies
                .iter()
                .map(|body| body.operand.value)
                .collect(),
            body_operand_record_offsets: value
                .bodies
                .iter()
                .map(|body| body.operand.offset)
                .collect(),
            source_body_entity_suffixes: value
                .bodies
                .iter()
                .map(|body| body.source.value)
                .collect(),
            source_body_entity_suffix_offsets: value
                .bodies
                .iter()
                .map(|body| body.source.offset)
                .collect(),
            copied_body_entity_suffixes: value
                .bodies
                .iter()
                .map(|body| body.copied.value)
                .collect(),
            copied_body_entity_suffix_offsets: value
                .bodies
                .iter()
                .map(|body| body.copied.offset)
                .collect(),
        }
    }
}
