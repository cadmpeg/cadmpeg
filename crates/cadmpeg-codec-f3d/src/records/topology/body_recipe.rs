// SPDX-License-Identifier: Apache-2.0
//! Body-recipe operands, operand groups, owners and persistent references.

use crate::records::source_frame::SourceFrame;

use cadmpeg_core::decode::u64_from_index;

use crate::records::identity::Located;
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::references::DesignClassTag;
use cadmpeg_ir::ids::FaceId;
use serde::Deserialize;
use serde::Serialize;

/// Whole-body construction operand carrying a persistent body-recipe reference.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignBodyRecipeOperandWire")]
pub(crate) struct DesignBodyRecipeOperand {
    record_index: u32,
    frame: Option<crate::records::frame_chain::RecordFrameChain>,
    /// Globally unique deterministic identifier for this native operand.
    pub(crate) id: String,
    /// Owning feature scope record.
    pub(crate) scope_record_index: u32,
    /// Exact feature-scope ownership form.
    #[serde(flatten)]
    pub(crate) owner: DesignOperandOwner,
    /// Source per-file dynamic primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Asset UUID qualifying the persistent selection namespace.
    pub(crate) asset_id: DesignRelaxedGuidText,
    /// UUID of the selection context.
    pub(crate) context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    context_id_offset: Option<u64>,
    /// Raw four-byte selector-tail member after the fixed `u32 2`.
    ///
    /// Class `365` varies this member without a settled neutral meaning;
    /// class `367` stores `01 00 00 00`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selector_tail: Option<SourceFrame>,
    selector_tail_offset: Option<u64>,
    /// Counted persistent Design references carried by this operand.
    references: Vec<DesignBodyRecipeReference>,
    /// Body construction recipe contained by this operand record.
    pub(crate) recipe_id: String,
    /// Unique input-state face selected by this operand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) resolved_face_slot: Option<i64>,
    /// Exact ASM input state containing the resolved body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) resolved_body_state_id: Option<i64>,
    /// Unique input-state body containing every reference's candidate faces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) resolved_body_slot: Option<i64>,
    /// Complete boundary-face set of the resolved body in its input state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) resolved_body_face_slots: Vec<i64>,
    /// Byte offset of the indexed record immediately following this operand.
    next_byte_offset: Option<u64>,
}

#[cfg(test)]
thread_local! {
    static BODY_RECIPE_OPERAND_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignBodyRecipeOperand {
    fn clone(&self) -> Self {
        #[cfg(test)]
        BODY_RECIPE_OPERAND_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            record_index: self.record_index,
            frame: self.frame,
            id: self.id.clone(),
            scope_record_index: self.scope_record_index,
            owner: self.owner,
            class_tag: self.class_tag.clone(),
            asset_id: self.asset_id.clone(),
            context_id: self.context_id.clone(),
            context_id_offset: self.context_id_offset,
            selector_tail: self.selector_tail.clone(),
            selector_tail_offset: self.selector_tail_offset,
            references: self.references.clone(),
            recipe_id: self.recipe_id.clone(),
            resolved_face_slot: self.resolved_face_slot,
            resolved_body_state_id: self.resolved_body_state_id,
            resolved_body_slot: self.resolved_body_slot,
            resolved_body_face_slots: self.resolved_body_face_slots.clone(),
            next_byte_offset: self.next_byte_offset,
        }
    }
}

impl DesignBodyRecipeOperand {
    pub(crate) fn try_new(draft: DesignBodyRecipeOperandDraft) -> Result<Self, String> {
        if draft.context_id_offset <= draft.asset_id_offset
            || draft.context_id_offset >= draft.next_byte_offset
            || draft.next_byte_offset <= draft.nested_record_index_offset
            || draft.selector_tail.is_some_and(|tail| {
                tail.offset < draft.context_id_offset
                    || tail
                        .offset
                        .checked_add(4)
                        .is_none_or(|end| end > draft.next_byte_offset)
            })
        {
            return Err(
                "context_id_offset/selector_tail/next_byte_offset disagree with body frame".into(),
            );
        }
        for (ordinal, reference) in draft.references.iter().enumerate() {
            let offset = u64::try_from(ordinal)
                .ok()
                .and_then(|ordinal| ordinal.checked_mul(12))
                .and_then(|delta| draft.byte_offset.checked_add(25)?.checked_add(delta));
            if reference.design_reference == 0
                || offset != reference.design_reference_offset
                || offset.and_then(|offset| offset.checked_add(8)) != reference.form_offset
            {
                return Err("references contain an invalid identity or offset".into());
            }
        }
        let frame = crate::records::frame_chain::RecordFrameChain::try_new(
            draft.record_index,
            draft.byte_offset,
            4,
            u64::try_from(draft.references.len())
                .ok()
                .and_then(|count| count.checked_mul(12))
                .and_then(|bytes| bytes.checked_add(44))
                .ok_or("references length overflows")?,
        )?;
        let value = Self {
            record_index: draft.record_index,
            frame: Some(frame),
            id: draft.id,
            scope_record_index: draft.scope_record_index,
            owner: draft.owner,
            class_tag: draft.class_tag,
            asset_id: draft.asset_id,
            context_id: draft.context_id,
            context_id_offset: Some(draft.context_id_offset),
            selector_tail_offset: draft.selector_tail.map(|tail| tail.offset),
            selector_tail: draft.selector_tail.map(|tail| tail.value.to_vec().into()),
            references: draft.references,
            recipe_id: draft.recipe_id,
            resolved_face_slot: draft.resolved_face_slot,
            resolved_body_state_id: draft.resolved_body_state_id,
            resolved_body_slot: draft.resolved_body_slot,
            resolved_body_face_slots: draft.resolved_body_face_slots,
            next_byte_offset: Some(draft.next_byte_offset),
        };
        if value.nested_record_index() != draft.nested_record_index {
            return Err("nested_record_index disagrees with frame layout".into());
        }
        if value.next_record_index() != draft.next_record_index {
            return Err("next_record_index disagrees with frame layout".into());
        }
        if value.nested_record_index_offset() != Some(draft.nested_record_index_offset) {
            return Err("nested_record_index_offset disagrees with frame layout".into());
        }
        if value.asset_id_offset() != Some(draft.asset_id_offset) {
            return Err("asset_id_offset disagrees with frame layout".into());
        }
        Ok(value)
    }
    #[cfg(test)]
    pub(crate) fn into_draft(self) -> Result<DesignBodyRecipeOperandDraft, String> {
        let source = |offset: Option<u64>| {
            offset.ok_or_else(|| "body operand has no source layout".to_owned())
        };
        let record_index = self.record_index();
        let byte_offset = source(self.byte_offset())?;
        let nested_record_index = self.nested_record_index();
        let next_record_index = self.next_record_index();
        let nested_record_index_offset = source(self.nested_record_index_offset())?;
        let asset_id_offset = source(self.asset_id_offset())?;
        let selector_tail = self
            .selector_tail
            .as_ref()
            .map(|tail| {
                let value: [u8; 4] = tail
                    .data()
                    .ok_or("selector tail has no source bytes")?
                    .try_into()
                    .map_err(|_| "selector tail length must be four")?;
                Ok::<_, String>(Located {
                    value,
                    offset: source(self.selector_tail_offset)?,
                })
            })
            .transpose()?;
        Ok(DesignBodyRecipeOperandDraft {
            id: self.id,
            scope_record_index: self.scope_record_index,
            owner: self.owner,
            record_index,
            byte_offset,
            class_tag: self.class_tag,
            asset_id: self.asset_id,
            asset_id_offset,
            context_id: self.context_id,
            context_id_offset: source(self.context_id_offset)?,
            selector_tail,
            references: self.references,
            nested_record_index,
            nested_record_index_offset,
            recipe_id: self.recipe_id,
            resolved_face_slot: self.resolved_face_slot,
            resolved_body_state_id: self.resolved_body_state_id,
            resolved_body_slot: self.resolved_body_slot,
            resolved_body_face_slots: self.resolved_body_face_slots,
            next_record_index,
            next_byte_offset: source(self.next_byte_offset)?,
        })
    }
    pub(crate) fn record_index(&self) -> u32 {
        self.record_index
    }
    pub(crate) fn byte_offset(&self) -> Option<u64> {
        self.frame.map(|frame| frame.offset(0))
    }
    fn asset_id_offset(&self) -> Option<u64> {
        self.nested_record_index_offset()?.checked_add(18)
    }
    pub(crate) fn context_id_offset(&self) -> Option<u64> {
        self.context_id_offset
    }
    pub(crate) fn references(&self) -> &Vec<DesignBodyRecipeReference> {
        &self.references
    }
    pub(crate) fn nested_record_index(&self) -> u64 {
        u64::from(self.record_index + 3)
    }
    fn nested_record_index_offset(&self) -> Option<u64> {
        self.frame
            .map(|frame| frame.offset(26 + u64_from_index(self.references.len()) * 12))
    }
    pub(crate) fn next_record_index(&self) -> u32 {
        self.record_index + 4
    }
    pub(crate) fn next_byte_offset(&self) -> Option<u64> {
        self.next_byte_offset
    }

    pub(crate) fn retain_source(
        &self,
        fidelity: &mut cadmpeg_ir::SourceFidelity,
    ) -> Result<(), cadmpeg_core::CodecError> {
        for (field, offset) in [
            ("byte_offset", self.byte_offset()),
            ("asset_id_offset", self.asset_id_offset()),
            ("context_id_offset", self.context_id_offset()),
            ("selector_tail_offset", self.selector_tail_offset),
            (
                "nested_record_index_offset",
                self.nested_record_index_offset(),
            ),
            ("next_byte_offset", self.next_byte_offset()),
        ] {
            if let Some(offset) = offset {
                fidelity
                    .metadata
                    .insert(format!("{}.{field}", self.id), offset.to_string());
            }
        }
        for (index, reference) in self.references.iter().enumerate() {
            for (field, offset) in [
                ("design_reference_offset", reference.design_reference_offset),
                ("form_offset", reference.form_offset),
            ] {
                if let Some(offset) = offset {
                    fidelity.metadata.insert(
                        format!("{}.references.{index}.{field}", self.id),
                        offset.to_string(),
                    );
                }
            }
        }
        if let Some(tail) = &self.selector_tail {
            let stream = crate::ids::native_stream(&self.id)
                .and_then(|stream| stream.strip_prefix(crate::ids::SCHEME_PREFIX))
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "body operand has no source stream identity",
                    )
                })?;
            // Only the fixed class-367 syntax has a fully known encoding. A
            // variable selector tail remains untyped source content, not an
            // authored native semantic field.
            let untyped =
                if self.class_tag.as_str() == "367" && tail.data() == Some(&[1, 0, 0, 0][..]) {
                    cadmpeg_ir::ArenaClasses::default()
                } else {
                    [cadmpeg_ir::document::ArenaClass::Design].into()
                };
            tail.retain(
                fidelity,
                &format!("{}:selector-tail", self.id),
                stream,
                self.selector_tail_offset,
                untyped,
            )?;
        }
        Ok(())
    }
}

impl Serialize for DesignBodyRecipeOperand {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            id: &'a str,
            scope_record_index: u32,
            #[serde(flatten)]
            owner: DesignOperandOwner,
            record_index: u32,
            class_tag: &'a str,
            asset_id: &'a str,
            context_id: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            selector_tail: Option<&'a SourceFrame>,
            references: &'a [DesignBodyRecipeReference],
            nested_record_index: u64,
            recipe_id: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            resolved_face_slot: Option<i64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            resolved_body_state_id: Option<i64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            resolved_body_slot: Option<i64>,
            #[serde(skip_serializing_if = "<[i64]>::is_empty")]
            resolved_body_face_slots: &'a [i64],
            next_record_index: u32,
        }
        WireRef {
            id: &self.id,
            scope_record_index: self.scope_record_index,
            owner: self.owner,
            record_index: self.record_index(),
            class_tag: self.class_tag.as_str(),
            asset_id: self.asset_id.as_str(),
            context_id: self.context_id.as_str(),
            selector_tail: self.selector_tail.as_ref(),
            references: &self.references,
            nested_record_index: self.nested_record_index(),
            recipe_id: &self.recipe_id,
            resolved_face_slot: self.resolved_face_slot,
            resolved_body_state_id: self.resolved_body_state_id,
            resolved_body_slot: self.resolved_body_slot,
            resolved_body_face_slots: &self.resolved_body_face_slots,
            next_record_index: self.next_record_index(),
        }
        .serialize(serializer)
    }
}

/// Unadmitted `DesignBodyRecipeOperand` fields.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignBodyRecipeOperandDraft {
    /// Globally unique deterministic identifier for this native operand.
    pub(crate) id: String,
    /// Owning feature scope record.
    pub(crate) scope_record_index: u32,
    /// Exact feature-scope ownership form.
    pub(crate) owner: DesignOperandOwner,
    /// Primary indexed-record identity.
    pub(crate) record_index: u32,
    /// Primary indexed-header byte offset.
    pub(crate) byte_offset: u64,
    /// Source per-file dynamic primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Asset UUID qualifying the persistent selection namespace.
    pub(crate) asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    pub(crate) asset_id_offset: u64,
    /// UUID of the selection context.
    pub(crate) context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub(crate) context_id_offset: u64,
    /// Raw four-byte selector-tail member after the fixed `u32 2`.
    ///
    /// Class `365` varies this member without a settled neutral meaning;
    /// class `367` stores `01 00 00 00`.
    pub(crate) selector_tail: Option<Located<[u8; 4]>>,
    /// Counted persistent Design references carried by this operand.
    pub(crate) references: Vec<DesignBodyRecipeReference>,
    /// Tagged nested record reference following the Design reference.
    pub(crate) nested_record_index: u64,
    /// Byte offset of `nested_record_index`.
    pub(crate) nested_record_index_offset: u64,
    /// Body construction recipe contained by this operand record.
    pub(crate) recipe_id: String,
    /// Unique input-state face selected by this operand.
    pub(crate) resolved_face_slot: Option<i64>,
    /// Exact ASM input state containing the resolved body.
    pub(crate) resolved_body_state_id: Option<i64>,
    /// Unique input-state body containing every reference's candidate faces.
    pub(crate) resolved_body_slot: Option<i64>,
    /// Complete boundary-face set of the resolved body in its input state.
    pub(crate) resolved_body_face_slots: Vec<i64>,
    /// Identity of the indexed record immediately following this operand.
    pub(crate) next_record_index: u32,
    /// Byte offset of the indexed record immediately following this operand.
    pub(crate) next_byte_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignBodyRecipeOperandWire {
    pub(crate) id: String,
    pub(crate) scope_record_index: u32,
    #[serde(flatten)]
    pub(crate) owner: DesignOperandOwner,
    pub(crate) record_index: u32,
    pub(crate) class_tag: DesignClassTag,
    pub(crate) asset_id: DesignRelaxedGuidText,
    pub(crate) context_id: DesignRelaxedGuidText,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_selector_tail"
    )]
    pub(crate) selector_tail: Option<SourceFrame>,
    pub(crate) references: Vec<DesignBodyRecipeReference>,
    pub(crate) nested_record_index: u64,
    pub(crate) recipe_id: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_resolved_face_slot"
    )]
    pub(crate) resolved_face_slot: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_resolved_body_state_id"
    )]
    pub(crate) resolved_body_state_id: Option<i64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_resolved_body_slot"
    )]
    pub(crate) resolved_body_slot: Option<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) resolved_body_face_slots: Vec<i64>,
    pub(crate) next_record_index: u32,
}
impl TryFrom<DesignBodyRecipeOperandWire> for DesignBodyRecipeOperand {
    type Error = String;
    fn try_from(wire: DesignBodyRecipeOperandWire) -> Result<Self, Self::Error> {
        let next_record_index = wire
            .record_index
            .checked_add(4)
            .ok_or("body operand record indices overflow")?;
        if u64::from(next_record_index - 1) != wire.nested_record_index
            || next_record_index != wire.next_record_index
        {
            return Err(
                "body operand nested/next record indices disagree with logical record chain".into(),
            );
        }
        if wire
            .references
            .iter()
            .any(|reference| reference.design_reference == 0)
        {
            return Err("body operand reference identity is zero".into());
        }
        if wire
            .selector_tail
            .as_ref()
            .is_some_and(|tail| tail.byte_len() != 4)
        {
            return Err("selector_tail evidence length must be four".into());
        }
        Ok(Self {
            record_index: wire.record_index,
            frame: None,
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            owner: wire.owner,
            class_tag: wire.class_tag,
            asset_id: wire.asset_id,
            context_id: wire.context_id,
            context_id_offset: None,
            selector_tail: wire.selector_tail,
            selector_tail_offset: None,
            references: wire.references,
            recipe_id: wire.recipe_id,
            resolved_face_slot: wire.resolved_face_slot,
            resolved_body_state_id: wire.resolved_body_state_id,
            resolved_body_slot: wire.resolved_body_slot,
            resolved_body_face_slots: wire.resolved_body_face_slots,
            next_byte_offset: None,
        })
    }
}
#[cfg(test)]
impl From<DesignBodyRecipeOperand> for DesignBodyRecipeOperandWire {
    fn from(value: DesignBodyRecipeOperand) -> Self {
        Self {
            record_index: value.record_index(),
            nested_record_index: value.nested_record_index(),
            next_record_index: value.next_record_index(),
            id: value.id,
            scope_record_index: value.scope_record_index,
            owner: value.owner,
            class_tag: value.class_tag,
            asset_id: value.asset_id,
            context_id: value.context_id,
            selector_tail: value.selector_tail,
            references: value.references,
            recipe_id: value.recipe_id,
            resolved_face_slot: value.resolved_face_slot,
            resolved_body_state_id: value.resolved_body_state_id,
            resolved_body_slot: value.resolved_body_slot,
            resolved_body_face_slots: value.resolved_body_face_slots,
        }
    }
}

/// Construction-operand group record and member position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct DesignOperandGroup {
    /// Owning construction-operand group record.
    pub(crate) group_record_index: u32,
    /// Zero-based position in the group's ordered member run.
    pub(crate) group_member_ordinal: u32,
}

/// Exact owner of a construction operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(untagged)]
pub(crate) enum DesignOperandOwner {
    /// Operand named by a counted construction-operand group.
    Group {
        /// Owning construction-operand group record.
        group_record_index: u32,
        /// Zero-based position in the group's ordered member run.
        group_member_ordinal: u32,
    },
    /// Standalone operand named directly by the feature scope reference table.
    ScopeReference {
        /// Zero-based position in the scope's ordered reference table.
        scope_reference_ordinal: u32,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DesignOperandOwnerFields {
    #[serde(default, deserialize_with = "deserialize_owner_group_record_index")]
    group_record_index: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_owner_group_member_ordinal")]
    group_member_ordinal: Option<u32>,
    #[serde(
        default,
        deserialize_with = "deserialize_owner_scope_reference_ordinal"
    )]
    scope_reference_ordinal: Option<u32>,
}

impl<'de> Deserialize<'de> for DesignOperandOwner {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let fields = DesignOperandOwnerFields::deserialize(deserializer)?;
        match (
            fields.group_record_index,
            fields.group_member_ordinal,
            fields.scope_reference_ordinal,
        ) {
            (Some(group_record_index), Some(group_member_ordinal), None) => Ok(Self::Group {
                group_record_index,
                group_member_ordinal,
            }),
            (None, None, Some(scope_reference_ordinal)) => Ok(Self::ScopeReference {
                scope_reference_ordinal,
            }),
            _ => Err(serde::de::Error::custom(
                "construction operand owner requires exactly one scope reference or complete group membership",
            )),
        }
    }
}

cadmpeg_core::named_optional_field!(
    deserialize_owner_group_record_index,
    u32,
    "group_record_index"
);
cadmpeg_core::named_optional_field!(
    deserialize_owner_group_member_ordinal,
    u32,
    "group_member_ordinal"
);
cadmpeg_core::named_optional_field!(
    deserialize_owner_scope_reference_ordinal,
    u32,
    "scope_reference_ordinal"
);

impl DesignOperandOwner {
    /// Return the construction-group record and member position, when grouped.
    pub(crate) const fn group(self) -> Option<(u32, u32)> {
        match self {
            Self::Group {
                group_record_index,
                group_member_ordinal,
            } => Some((group_record_index, group_member_ordinal)),
            Self::ScopeReference { .. } => None,
        }
    }
}

/// One counted persistent reference inside a whole-body recipe operand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignBodyRecipeReference {
    /// Persistent Design reference.
    pub(crate) design_reference: u64,
    /// Byte offset of `design_reference`.
    #[serde(skip)]
    pub(crate) design_reference_offset: Option<u64>,
    /// Reference-local serialized form discriminator.
    pub(crate) form: u32,
    /// Byte offset of `form`.
    #[serde(skip)]
    pub(crate) form_offset: Option<u64>,
    /// Solved faces carrying this reference, ordered by face identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) candidate_faces: Vec<cadmpeg_ir::ids::FaceId>,
    /// Candidate faces present in the owning feature's input topology.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) preceding_candidate_faces: Vec<cadmpeg_ir::ids::FaceId>,
    /// Input-state bodies containing at least one candidate face.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) preceding_body_slots: Vec<i64>,
}

/// Stable ASM entity family named by a Design persistent identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AsmHistoricalEntityKind {
    /// Body topology slot.
    Body,
    /// Region topology slot.
    Region,
    /// Shell topology slot.
    Shell,
    /// Face topology slot.
    Face,
    /// Loop topology slot.
    Loop,
    /// Coedge topology slot.
    Coedge,
    /// Edge topology slot.
    Edge,
    /// Vertex topology slot.
    Vertex,
    /// Point carrier slot.
    Point,
    /// Surface carrier slot.
    Surface,
    /// Curve carrier slot.
    Curve,
    /// Parametric-curve carrier slot.
    Pcurve,
}

/// Mutable binding evidence for one fixed body-recipe reference.
pub(crate) struct DesignBodyRecipeReferenceBindings<'a> {
    pub(crate) design_reference: u64,
    pub(crate) form: u32,
    pub(crate) candidate_faces: &'a mut Vec<FaceId>,
    pub(crate) preceding_candidate_faces: &'a mut Vec<FaceId>,
    pub(crate) preceding_body_slots: &'a mut Vec<i64>,
}

impl DesignBodyRecipeOperand {
    pub(crate) fn reference_bindings_mut(
        &mut self,
    ) -> impl Iterator<Item = DesignBodyRecipeReferenceBindings<'_>> {
        self.references
            .iter_mut()
            .map(|reference| DesignBodyRecipeReferenceBindings {
                design_reference: reference.design_reference,
                form: reference.form,
                candidate_faces: &mut reference.candidate_faces,
                preceding_candidate_faces: &mut reference.preceding_candidate_faces,
                preceding_body_slots: &mut reference.preceding_body_slots,
            })
    }
}

cadmpeg_core::named_optional_field!(deserialize_selector_tail, SourceFrame, "selector_tail");

cadmpeg_core::named_optional_field!(deserialize_resolved_face_slot, i64, "resolved_face_slot");

cadmpeg_core::named_optional_field!(
    deserialize_resolved_body_state_id,
    i64,
    "resolved_body_state_id"
);

cadmpeg_core::named_optional_field!(deserialize_resolved_body_slot, i64, "resolved_body_slot");

#[cfg(test)]
mod tests;

mod identity_rewrite;
