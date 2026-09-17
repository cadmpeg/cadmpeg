// SPDX-License-Identifier: Apache-2.0
//! Face operands, face source groups and face recipe structures.

use super::body_recipe::DesignOperandGroup;
use super::construction::DesignConstructionPersistentIdentity;
use super::edge_recipe::DesignTopologyRecipeSide;
use super::historical_context::DesignHistoricalFaceSupportContext;
use crate::records::dimensions::DesignRecipeReference;
use crate::records::identity::Located;
use crate::records::identity::NonEmptyByteSpan;
use crate::records::recipes::ConstructionRecipeKind;
use crate::records::references::DesignClassTag;
use cadmpeg_ir::ids::FaceId;
use serde::Deserialize;
use serde::Serialize;

/// Face-selection operand owned by a parameter scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DesignFaceOperandWire", into = "DesignFaceOperandWire")]
pub struct DesignFaceOperand {
    frame: crate::records::frame_chain::RecordFrameChain,
    /// Globally unique deterministic identifier for this native operand.
    pub id: String,
    /// Owning parameter-scope record.
    pub scope_record_index: u32,
    /// Zero-based position in the scope's ordered reference table.
    pub scope_reference_ordinal: u32,
    /// Owning construction-operand group, absent for a direct scope operand.
    pub group: Option<DesignOperandGroup>,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: DesignClassTag,
    /// Byte offset of the same-index paired header.
    paired_byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    pub paired_class_tag: DesignClassTag,
    /// Byte offset of the recipe record's indexed header.
    recipe_record_byte_offset: u64,
    /// Native construction-recipe arena id.
    pub recipe_id: String,
    /// Complete recipe-specific prefix before the length-prefixed family name.
    pub recipe_prefix_bytes: Vec<u8>,
    /// Persistent Design selector/reference entries decoded from the prefix.
    pub recipe_references: Vec<DesignRecipeReference>,
    /// Exact face-recipe family.
    pub recipe_kind: ConstructionRecipeKind,
    /// Byte offset of the first i32 after the framed recipe-family name.
    pub recipe_program_offset: u64,
    /// Complete post-name i32 program ending at the next indexed record.
    pub recipe_program: Vec<i32>,
    /// Ordered nodes partitioning the program after its three-word header.
    pub recipe_nodes: Vec<DesignFaceRecipeNode>,
    /// Active solved faces carrying the recipe's persistent Design reference.
    pub candidate_faces: Vec<FaceId>,
    /// Candidate faces not explicitly named as topology context by a prefix
    /// selector carrying the recipe's own Design reference.
    pub unreferenced_candidate_faces: Vec<FaceId>,
    /// Faces named by a prefix operand carrying the recipe's own token and
    /// Design reference under a different native selector.
    pub alternate_selector_candidate_faces: Vec<FaceId>,
    /// Candidate faces present in the ASM topology immediately preceding the
    /// owning feature.
    pub preceding_candidate_faces: Vec<FaceId>,
    /// Preceding candidate faces deleted or updated by the owning feature's
    /// exact ASM state transition.
    pub changed_candidate_faces: Vec<FaceId>,
    /// Active candidates mapped through an invariant surface carrier to face
    /// owners in the immediately preceding historical topology.
    pub historical_support_contexts: Vec<DesignHistoricalFaceSupportContext>,
    /// Ordered stable historical face slots proven by the preceding topology
    /// or exact feature transition.
    pub resolved_face_slots: Vec<i64>,
    /// Current active-BREP face identity proven by a legacy Extrude recipe
    /// when no preceding historical slot exists.
    pub resolved_active_face: Option<FaceId>,
    /// Identity of the indexed record following the operand frame.
    pub next_record_index: u32,
    /// Byte offset of the indexed record following the operand frame.
    next_byte_offset: u64,
}

impl DesignFaceOperand {
    pub(crate) fn try_new(draft: DesignFaceOperandDraft) -> Result<Self, String> {
        if !(draft.byte_offset < draft.paired_byte_offset
            && draft.paired_byte_offset < draft.recipe_record_byte_offset
            && draft.recipe_record_byte_offset < draft.next_byte_offset)
        {
            return Err(
                "paired_byte_offset/recipe_record_byte_offset/next_byte_offset must increase"
                    .into(),
            );
        }
        draft
            .recipe_record_byte_offset
            .checked_add(11)
            .ok_or("recipe_prefix_offset overflows")?;
        let frame = crate::records::frame_chain::RecordFrameChain::try_new(
            draft.record_index,
            draft.byte_offset,
            3,
            0,
        )?;
        let value = Self {
            frame,
            id: draft.id,
            scope_record_index: draft.scope_record_index,
            scope_reference_ordinal: draft.scope_reference_ordinal,
            group: draft.group,
            class_tag: draft.class_tag,
            paired_byte_offset: draft.paired_byte_offset,
            paired_class_tag: draft.paired_class_tag,
            recipe_record_byte_offset: draft.recipe_record_byte_offset,
            recipe_id: draft.recipe_id,
            recipe_prefix_bytes: draft.recipe_prefix_bytes,
            recipe_references: draft.recipe_references,
            recipe_kind: draft.recipe_kind,
            recipe_program_offset: draft.recipe_program_offset,
            recipe_program: draft.recipe_program,
            recipe_nodes: draft.recipe_nodes,
            candidate_faces: draft.candidate_faces,
            unreferenced_candidate_faces: draft.unreferenced_candidate_faces,
            alternate_selector_candidate_faces: draft.alternate_selector_candidate_faces,
            preceding_candidate_faces: draft.preceding_candidate_faces,
            changed_candidate_faces: draft.changed_candidate_faces,
            historical_support_contexts: draft.historical_support_contexts,
            resolved_face_slots: draft.resolved_face_slots,
            resolved_active_face: draft.resolved_active_face,
            next_record_index: draft.next_record_index,
            next_byte_offset: draft.next_byte_offset,
        };
        if value.recipe_record_index() != draft.recipe_record_index {
            return Err("recipe_record_index disagrees with frame layout".into());
        }
        if value.recipe_prefix_offset() != draft.recipe_prefix_offset {
            return Err("recipe_prefix_offset disagrees with frame layout".into());
        }
        Ok(value)
    }
    pub(crate) fn into_draft(self) -> DesignFaceOperandDraft {
        let record_index = self.record_index();
        let byte_offset = self.byte_offset();
        let recipe_record_index = self.recipe_record_index();
        let recipe_prefix_offset = self.recipe_prefix_offset();
        DesignFaceOperandDraft {
            id: self.id,
            scope_record_index: self.scope_record_index,
            scope_reference_ordinal: self.scope_reference_ordinal,
            group: self.group,
            record_index,
            byte_offset,
            class_tag: self.class_tag,
            paired_byte_offset: self.paired_byte_offset,
            paired_class_tag: self.paired_class_tag,
            recipe_record_index,
            recipe_record_byte_offset: self.recipe_record_byte_offset,
            recipe_id: self.recipe_id,
            recipe_prefix_offset,
            recipe_prefix_bytes: self.recipe_prefix_bytes,
            recipe_references: self.recipe_references,
            recipe_kind: self.recipe_kind,
            recipe_program_offset: self.recipe_program_offset,
            recipe_program: self.recipe_program,
            recipe_nodes: self.recipe_nodes,
            candidate_faces: self.candidate_faces,
            unreferenced_candidate_faces: self.unreferenced_candidate_faces,
            alternate_selector_candidate_faces: self.alternate_selector_candidate_faces,
            preceding_candidate_faces: self.preceding_candidate_faces,
            changed_candidate_faces: self.changed_candidate_faces,
            historical_support_contexts: self.historical_support_contexts,
            resolved_face_slots: self.resolved_face_slots,
            resolved_active_face: self.resolved_active_face,
            next_record_index: self.next_record_index,
            next_byte_offset: self.next_byte_offset,
        }
    }
    pub(crate) fn record_index(&self) -> u32 {
        self.frame.index(0)
    }
    pub(crate) fn byte_offset(&self) -> u64 {
        self.frame.offset(0)
    }
    pub(crate) fn recipe_record_index(&self) -> u32 {
        self.frame.index(3)
    }
    pub(crate) fn recipe_record_byte_offset(&self) -> u64 {
        self.recipe_record_byte_offset
    }
    pub(crate) fn recipe_prefix_offset(&self) -> u64 {
        self.recipe_record_byte_offset + 11
    }
    pub(crate) fn next_byte_offset(&self) -> u64 {
        self.next_byte_offset
    }
}

/// Unadmitted `DesignFaceOperand` fields.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignFaceOperandDraft {
    /// Globally unique deterministic identifier for this native operand.
    pub id: String,
    /// Owning parameter-scope record.
    pub scope_record_index: u32,
    /// Zero-based position in the scope's ordered reference table.
    pub scope_reference_ordinal: u32,
    /// Owning construction-operand group, absent for a direct scope operand.
    pub group: Option<DesignOperandGroup>,
    /// Primary indexed-record identity named by a face operand group.
    pub record_index: u32,
    /// Byte offset of the primary indexed-record header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub class_tag: DesignClassTag,
    /// Byte offset of the same-index paired header.
    pub paired_byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    pub paired_class_tag: DesignClassTag,
    /// Indexed record containing the face regeneration recipe.
    pub recipe_record_index: u32,
    /// Byte offset of the recipe record's indexed header.
    pub recipe_record_byte_offset: u64,
    /// Native construction-recipe arena id.
    pub recipe_id: String,
    /// Byte offset of the recipe-specific prefix after the indexed header.
    pub recipe_prefix_offset: u64,
    /// Complete recipe-specific prefix before the length-prefixed family name.
    pub recipe_prefix_bytes: Vec<u8>,
    /// Persistent Design selector/reference entries decoded from the prefix.
    pub recipe_references: Vec<DesignRecipeReference>,
    /// Exact face-recipe family.
    pub recipe_kind: ConstructionRecipeKind,
    /// Byte offset of the first i32 after the framed recipe-family name.
    pub recipe_program_offset: u64,
    /// Complete post-name i32 program ending at the next indexed record.
    pub recipe_program: Vec<i32>,
    /// Ordered nodes partitioning the program after its three-word header.
    pub recipe_nodes: Vec<DesignFaceRecipeNode>,
    /// Active solved faces carrying the recipe's persistent Design reference.
    pub candidate_faces: Vec<FaceId>,
    /// Candidate faces not explicitly named as topology context by a prefix
    /// selector carrying the recipe's own Design reference.
    pub unreferenced_candidate_faces: Vec<FaceId>,
    /// Faces named by a prefix operand carrying the recipe's own token and
    /// Design reference under a different native selector.
    pub alternate_selector_candidate_faces: Vec<FaceId>,
    /// Candidate faces present in the ASM topology immediately preceding the
    /// owning feature.
    pub preceding_candidate_faces: Vec<FaceId>,
    /// Preceding candidate faces deleted or updated by the owning feature's
    /// exact ASM state transition.
    pub changed_candidate_faces: Vec<FaceId>,
    /// Active candidates mapped through an invariant surface carrier to face
    /// owners in the immediately preceding historical topology.
    pub historical_support_contexts: Vec<DesignHistoricalFaceSupportContext>,
    /// Ordered stable historical face slots proven by the preceding topology
    /// or exact feature transition.
    pub resolved_face_slots: Vec<i64>,
    /// Current active-BREP face identity proven by a legacy Extrude recipe
    /// when no preceding historical slot exists.
    pub resolved_active_face: Option<FaceId>,
    /// Identity of the indexed record following the operand frame.
    pub next_record_index: u32,
    /// Byte offset of the indexed record following the operand frame.
    pub next_byte_offset: u64,
}

/// Face-selection operand owned by a parameter scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignFaceOperandWire {
    /// Globally unique deterministic identifier for this native operand.
    id: String,
    /// Owning parameter-scope record.
    scope_record_index: u32,
    /// Zero-based position in the scope's ordered reference table.
    scope_reference_ordinal: u32,
    /// Owning construction-operand group, absent for a direct scope operand.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_group_record_index"
    )]
    group_record_index: Option<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_group_member_ordinal"
    )]
    group_member_ordinal: Option<u32>,
    /// Primary indexed-record identity named by a face operand group.
    record_index: u32,
    /// Byte offset of the primary indexed-record header.
    byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    class_tag: String,
    /// Byte offset of the same-index paired header.
    paired_byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    paired_class_tag: String,
    /// Indexed record containing the face regeneration recipe.
    recipe_record_index: u32,
    /// Byte offset of the recipe record's indexed header.
    recipe_record_byte_offset: u64,
    /// Native construction-recipe arena id.
    recipe_id: String,
    /// Byte offset of the recipe-specific prefix after the indexed header.
    recipe_prefix_offset: u64,
    /// Complete recipe-specific prefix before the length-prefixed family name.
    #[serde(with = "cadmpeg_ir::bytes")]
    recipe_prefix_bytes: Vec<u8>,
    /// Persistent Design selector/reference entries decoded from the prefix.
    recipe_references: Vec<DesignRecipeReference>,
    /// Exact face-recipe family.
    recipe_kind: ConstructionRecipeKind,
    /// Byte offset of the first i32 after the framed recipe-family name.
    recipe_program_offset: u64,
    /// Complete post-name i32 program ending at the next indexed record.
    recipe_program: Vec<i32>,
    /// Byte offsets of the `[-1, -1, 2]` node openers declared by the program.
    recipe_node_offsets: Vec<u64>,
    /// Ordered nodes partitioning the program after its three-word header.
    recipe_nodes: Vec<DesignFaceRecipeNode>,
    /// Active solved faces carrying the recipe's persistent Design reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    candidate_faces: Vec<FaceId>,
    /// Candidate faces not explicitly named as topology context by a prefix
    /// selector carrying the recipe's own Design reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    unreferenced_candidate_faces: Vec<FaceId>,
    /// Faces named by a prefix operand carrying the recipe's own token and
    /// Design reference under a different native selector.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    alternate_selector_candidate_faces: Vec<FaceId>,
    /// Candidate faces present in the ASM topology immediately preceding the
    /// owning feature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    preceding_candidate_faces: Vec<FaceId>,
    /// Preceding candidate faces deleted or updated by the owning feature's
    /// exact ASM state transition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    changed_candidate_faces: Vec<FaceId>,
    /// Active candidates mapped through an invariant surface carrier to face
    /// owners in the immediately preceding historical topology.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    historical_support_contexts: Vec<DesignHistoricalFaceSupportContext>,
    /// Ordered stable historical face slots proven by the preceding topology
    /// or exact feature transition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    resolved_face_slots: Vec<i64>,
    /// Current active-BREP face identity proven by a legacy Extrude recipe
    /// when no preceding historical slot exists.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_resolved_active_face"
    )]
    resolved_active_face: Option<FaceId>,
    /// Identity of the indexed record following the operand frame.
    next_record_index: u32,
    /// Byte offset of the indexed record following the operand frame.
    next_byte_offset: u64,
}

impl TryFrom<DesignFaceOperandWire> for DesignFaceOperand {
    type Error = String;
    fn try_from(wire: DesignFaceOperandWire) -> Result<Self, Self::Error> {
        if !wire
            .recipe_node_offsets
            .iter()
            .copied()
            .eq(wire.recipe_nodes.iter().map(|node| node.byte_offset))
        {
            return Err("recipe_node_offsets must match recipe_nodes byte_offset values".into());
        }
        let group = match (wire.group_record_index, wire.group_member_ordinal) {
            (None, None) => None,
            (Some(group_record_index), Some(group_member_ordinal)) => Some(DesignOperandGroup {
                group_record_index,
                group_member_ordinal,
            }),
            _ => {
                return Err(
                    "group_record_index and group_member_ordinal must occur together".into(),
                )
            }
        };
        Self::try_new(DesignFaceOperandDraft {
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            scope_reference_ordinal: wire.scope_reference_ordinal,
            group,
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            paired_byte_offset: wire.paired_byte_offset,
            paired_class_tag: wire.paired_class_tag.try_into()?,
            recipe_record_index: wire.recipe_record_index,
            recipe_record_byte_offset: wire.recipe_record_byte_offset,
            recipe_id: wire.recipe_id,
            recipe_prefix_offset: wire.recipe_prefix_offset,
            recipe_prefix_bytes: wire.recipe_prefix_bytes,
            recipe_references: wire.recipe_references,
            recipe_kind: wire.recipe_kind,
            recipe_program_offset: wire.recipe_program_offset,
            recipe_program: wire.recipe_program,
            recipe_nodes: wire.recipe_nodes,
            candidate_faces: wire.candidate_faces,
            unreferenced_candidate_faces: wire.unreferenced_candidate_faces,
            alternate_selector_candidate_faces: wire.alternate_selector_candidate_faces,
            preceding_candidate_faces: wire.preceding_candidate_faces,
            changed_candidate_faces: wire.changed_candidate_faces,
            historical_support_contexts: wire.historical_support_contexts,
            resolved_face_slots: wire.resolved_face_slots,
            resolved_active_face: wire.resolved_active_face,
            next_record_index: wire.next_record_index,
            next_byte_offset: wire.next_byte_offset,
        })
    }
}

impl From<DesignFaceOperand> for DesignFaceOperandWire {
    fn from(operand: DesignFaceOperand) -> Self {
        let operand = operand.into_draft();
        let recipe_node_offsets = operand
            .recipe_nodes
            .iter()
            .map(|node| node.byte_offset)
            .collect();
        Self {
            id: operand.id,
            scope_record_index: operand.scope_record_index,
            scope_reference_ordinal: operand.scope_reference_ordinal,
            group_record_index: operand.group.map(|group| group.group_record_index),
            group_member_ordinal: operand.group.map(|group| group.group_member_ordinal),
            record_index: operand.record_index,
            byte_offset: operand.byte_offset,
            class_tag: operand.class_tag.into(),
            paired_byte_offset: operand.paired_byte_offset,
            paired_class_tag: operand.paired_class_tag.into(),
            recipe_record_index: operand.recipe_record_index,
            recipe_record_byte_offset: operand.recipe_record_byte_offset,
            recipe_id: operand.recipe_id,
            recipe_prefix_offset: operand.recipe_prefix_offset,
            recipe_prefix_bytes: operand.recipe_prefix_bytes,
            recipe_references: operand.recipe_references,
            recipe_kind: operand.recipe_kind,
            recipe_program_offset: operand.recipe_program_offset,
            recipe_program: operand.recipe_program,
            recipe_node_offsets,
            recipe_nodes: operand.recipe_nodes,
            candidate_faces: operand.candidate_faces,
            unreferenced_candidate_faces: operand.unreferenced_candidate_faces,
            alternate_selector_candidate_faces: operand.alternate_selector_candidate_faces,
            preceding_candidate_faces: operand.preceding_candidate_faces,
            changed_candidate_faces: operand.changed_candidate_faces,
            historical_support_contexts: operand.historical_support_contexts,
            resolved_face_slots: operand.resolved_face_slots,
            resolved_active_face: operand.resolved_active_face,
            next_record_index: operand.next_record_index,
            next_byte_offset: operand.next_byte_offset,
        }
    }
}

impl DesignFaceOperand {
    pub(crate) fn group_record_index(&self) -> Option<u32> {
        self.group.map(|group| group.group_record_index)
    }

    pub(crate) fn group_member_ordinal(&self) -> Option<u32> {
        self.group.map(|group| group.group_member_ordinal)
    }
}

/// Native source-shape carrier owned by a `Face` parameter scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignFaceSourceGroupWire",
    into = "DesignFaceSourceGroupWire"
)]
pub struct DesignFaceSourceGroup {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Owning `Face` parameter-scope record.
    pub scope_record_index: u32,
    /// Zero-based position of the source carrier in the scope reference table.
    pub carrier_reference_ordinal: u32,
    /// Indexed record carrying the ordered source-shape references.
    pub carrier_record_index: u32,
    /// Source interval from the carrier header to its paired header.
    pub carrier_span: NonEmptyByteSpan,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    pub carrier_class_tag: DesignClassTag,
    /// Indexed record paired with the source carrier.
    pub paired_record_index: u32,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    pub paired_class_tag: DesignClassTag,
    /// Ordered persistent source-shape identities.
    pub source_members: Vec<Located<DesignFaceSourceMember>>,
}

/// Native source-shape carrier owned by a `Face` parameter scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignFaceSourceGroupWire {
    /// Globally unique deterministic identifier for this native record.
    id: String,
    /// Owning `Face` parameter-scope record.
    scope_record_index: u32,
    /// Zero-based position of the source carrier in the scope reference table.
    carrier_reference_ordinal: u32,
    /// Indexed record carrying the ordered source-shape references.
    carrier_record_index: u32,
    /// Byte offset of the source carrier's indexed header.
    carrier_byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    carrier_class_tag: String,
    /// Bytes from the carrier header to its paired carrier header.
    carrier_frame_length: u64,
    /// Indexed record paired with the source carrier.
    paired_record_index: u32,
    /// Byte offset of the paired carrier's indexed header.
    paired_byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII paired class tag.
    paired_class_tag: String,
    /// Absolute byte offsets of the marked source-reference slots.
    source_reference_offsets: Vec<u64>,
    /// Ordered persistent source-shape identities.
    source_members: Vec<DesignFaceSourceMember>,
}

impl TryFrom<DesignFaceSourceGroupWire> for DesignFaceSourceGroup {
    type Error = String;
    fn try_from(wire: DesignFaceSourceGroupWire) -> Result<Self, Self::Error> {
        if wire.source_members.len() != wire.source_reference_offsets.len() {
            return Err(
                "source_members and source_reference_offsets must have equal lengths".into(),
            );
        }
        let carrier_span = NonEmptyByteSpan::new(wire.carrier_byte_offset, wire.paired_byte_offset)
            .ok_or("paired_byte_offset must follow carrier_byte_offset")?;
        if wire.carrier_frame_length != carrier_span.byte_len() {
            return Err("carrier_frame_length must match the carrier byte span".into());
        }
        Ok(Self {
            carrier_span,
            source_members: wire
                .source_members
                .into_iter()
                .zip(wire.source_reference_offsets)
                .map(|(value, offset)| Located { value, offset })
                .collect(),
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            carrier_reference_ordinal: wire.carrier_reference_ordinal,
            carrier_record_index: wire.carrier_record_index,
            carrier_class_tag: wire.carrier_class_tag.try_into()?,
            paired_record_index: wire.paired_record_index,
            paired_class_tag: wire.paired_class_tag.try_into()?,
        })
    }
}

impl From<DesignFaceSourceGroup> for DesignFaceSourceGroupWire {
    fn from(group: DesignFaceSourceGroup) -> Self {
        let (source_members, source_reference_offsets) = group
            .source_members
            .into_iter()
            .map(|member| (member.value, member.offset))
            .unzip();
        Self {
            source_members,
            source_reference_offsets,
            id: group.id,
            scope_record_index: group.scope_record_index,
            carrier_reference_ordinal: group.carrier_reference_ordinal,
            carrier_record_index: group.carrier_record_index,
            carrier_byte_offset: group.carrier_span.start(),
            carrier_class_tag: group.carrier_class_tag.into(),
            carrier_frame_length: group.carrier_span.byte_len(),
            paired_record_index: group.paired_record_index,
            paired_byte_offset: group.carrier_span.end(),
            paired_class_tag: group.paired_class_tag.into(),
        }
    }
}

/// Persistent source-shape identity named by a `Face` source carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignFaceSourceMember {
    /// Indexed record named by the carrier's source-reference slot.
    pub record_index: u32,
    /// Byte offset of the persistent-identity record's indexed header.
    pub byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII identity class tag.
    pub class_tag: DesignClassTag,
    /// Fixed persistent identity carried by the source record.
    pub persistent_identity: DesignConstructionPersistentIdentity,
}

/// One length-delimited node in a face regeneration recipe program.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignFaceRecipeNode {
    /// Byte offset of the node's `[-1, -1, 2]` opener.
    pub byte_offset: u64,
    /// Exclusive byte offset of the next node or the operand's following record.
    pub end_byte_offset: u64,
    /// Complete node words, including the three-word opener.
    pub program: Vec<i32>,
    /// Shared two-side topology recipe structure following the node opener.
    pub recipe_structure: Option<DesignFaceRecipeStructure>,
}

/// Structured topology program following a face-recipe node opener.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignFaceRecipeStructure {
    /// Scalar before the prelude delimiters.
    pub root: i32,
    /// Two scalar prelude runs before the first side clause.
    pub prelude: [i32; 2],
    /// Two ordered topology side clauses.
    pub sides: [DesignTopologyRecipeSide; 2],
    /// Scalar carried by the optional `[-1, value, -1, 0, 0, -1]` postlude.
    #[serde(
        default,
        rename = "postlude",
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_face_recipe_postlude",
        deserialize_with = "deserialize_face_recipe_postlude"
    )]
    pub postlude_value: Option<i32>,
}

// The wire adapter receives the optional field by reference, including its absence.
// Serde passes the field by reference to this wire adapter.
#[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)]
fn serialize_face_recipe_postlude<S: serde::Serializer>(
    value: &Option<i32>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(value) => [-1, *value, -1, 0, 0, -1].as_slice().serialize(serializer),
        None => <[i32]>::serialize(&[], serializer),
    }
}

fn deserialize_face_recipe_postlude<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<i32>, D::Error> {
    let words = Vec::<i32>::deserialize(deserializer)?;
    match words.as_slice() {
        [] => Ok(None),
        [-1, value, -1, 0, 0, -1] => Ok(Some(*value)),
        _ => Err(serde::de::Error::custom(
            "postlude must be empty or [-1, value, -1, 0, 0, -1]",
        )),
    }
}

cadmpeg_core::named_optional_field!(deserialize_group_record_index, u32, "group_record_index");

cadmpeg_core::named_optional_field!(
    deserialize_group_member_ordinal,
    u32,
    "group_member_ordinal"
);

cadmpeg_core::named_optional_field!(
    deserialize_resolved_active_face,
    FaceId,
    "resolved_active_face"
);

#[cfg(test)]
mod tests;
