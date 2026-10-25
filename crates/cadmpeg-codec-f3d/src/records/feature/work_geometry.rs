// SPDX-License-Identifier: Apache-2.0
//! Work axes, work points, work planes and the vertex recipes they resolve through.

use cadmpeg_ir::native::bytes::NativeBytes;

use cadmpeg_core::decode::u64_from_index;

use crate::records::dimensions::DesignRecipeReference;
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::references::DesignClassTag;
use cadmpeg_ir::scalar::FiniteReal;
use serde::{Deserialize, Serialize};

#[cfg(test)]
std::thread_local! {
    pub(crate) static WORK_GEOMETRY_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
struct WorkGeometryCloneProbe;

#[cfg(test)]
impl Clone for WorkGeometryCloneProbe {
    fn clone(&self) -> Self {
        WORK_GEOMETRY_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self
    }
}

cadmpeg_core::named_optional_field!(
    deserialize_carrier,
    Box<DesignWorkPointInputCarrierWire>,
    "carrier"
);
cadmpeg_core::named_optional_field!(deserialize_recipe_state_id, i64, "recipe_state_id");
cadmpeg_core::named_optional_field!(
    deserialize_resolved_vertex_slot,
    i64,
    "resolved_vertex_slot"
);
cadmpeg_core::named_optional_field!(deserialize_source, DesignWorkAxisSource, "source");
/// Source form for an exact solved `WorkAxis` construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) enum DesignWorkAxisSource {
    /// The axis carrier and two endpoint point carriers are cross-checked.
    TwoPoint {
        /// Ordered endpoint carrier record indices.
        point_record_indices: [u32; 2],
        /// Byte offsets of the first coordinate in each endpoint carrier of a
        /// located scope.
        #[serde(skip)]
        point_offsets: Option<[u64; 2]>,
    },
    /// A generation-specific carrier stores the axis directly and has one
    /// additional construction-support record in the enclosing scope.
    DirectCarrier {
        /// Indexed record carrying the origin and displacement values.
        carrier_record_index: u32,
        /// Enclosing scope's second ordered construction record.
        support_record_index: u32,
    },
}

/// Exact solved construction carried by a `WorkAxis` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignWorkAxisConstruction {
    /// First construction point in model centimetres.
    pub(crate) origin: [FiniteReal; 3],
    /// Displacement from the first construction point to the second, in centimetres.
    pub(crate) displacement: [FiniteReal; 3],
    /// Byte offset of the first origin coordinate of a located scope.
    #[serde(skip)]
    pub(crate) origin_offset: Option<u64>,
    /// Byte offset of the first displacement component of a located scope.
    #[serde(skip)]
    pub(crate) displacement_offset: Option<u64>,
    /// Native record form that supplied or corroborated the axis geometry.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_source"
    )]
    pub(crate) source: Option<DesignWorkAxisSource>,
}

/// One source-record reference used by a `WorkPoint` construction rule.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DesignWorkPointInputWire")]
pub(crate) struct DesignWorkPointInput {
    #[cfg(test)]
    clone_probe: WorkGeometryCloneProbe,
    /// Referenced Design record index.
    record_index: u32,
    /// Byte offset of the serialized reference target of a located scope.
    pub(crate) reference_offset: Option<u64>,
    /// Exact source carrier selected by this reference, when decoded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    carrier: Option<Box<DesignWorkPointInputCarrier>>,
}

impl DesignWorkPointInput {
    pub(crate) fn try_new(
        record_index: u32,
        reference_offset: Option<u64>,
        carrier: Option<Box<DesignWorkPointInputCarrier>>,
    ) -> Result<Self, String> {
        if matches!(
            carrier.as_deref(),
            Some(
                DesignWorkPointInputCarrier::WorkPlane { .. }
                    | DesignWorkPointInputCarrier::SketchPoint { .. }
            )
        ) && record_index.checked_add(4).is_none()
        {
            return Err("carrier record indices overflow input record_index".into());
        }
        Ok(Self {
            #[cfg(test)]
            clone_probe: WorkGeometryCloneProbe,
            record_index,
            reference_offset,
            carrier,
        })
    }
    pub(crate) fn record_index(&self) -> u32 {
        self.record_index
    }
    pub(crate) fn carrier(&self) -> Option<&DesignWorkPointInputCarrier> {
        self.carrier.as_deref()
    }
}

/// Serialized `DesignWorkPointInput` columns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignWorkPointInputWire {
    /// Referenced Design record index.
    record_index: u32,
    /// Exact source carrier selected by this reference, when decoded.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_carrier"
    )]
    carrier: Option<Box<DesignWorkPointInputCarrierWire>>,
}

impl TryFrom<DesignWorkPointInputWire> for DesignWorkPointInput {
    type Error = String;
    fn try_from(wire: DesignWorkPointInputWire) -> Result<Self, String> {
        let carrier = wire
            .carrier
            .map(|carrier| carrier.into_model(wire.record_index))
            .transpose()?
            .map(Box::new);
        Self::try_new(wire.record_index, None, carrier)
    }
}

impl From<DesignWorkPointInput> for DesignWorkPointInputWire {
    fn from(value: DesignWorkPointInput) -> Self {
        Self {
            record_index: value.record_index,
            carrier: value
                .carrier
                .map(|carrier| Box::new((*carrier).into_wire(value.record_index))),
        }
    }
}

/// Exact source carrier selected by one `WorkPoint` construction input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DesignWorkPointInputCarrier {
    /// Persistent edge recipe retained in the native edge-operand arena.
    EdgeRecipe {
        /// Native `DesignEdgeOperand` identifier.
        operand_id: String,
    },
    /// Persistent vertex recipe carried directly by this `WorkPoint` input.
    VertexRecipe {
        /// Exact vertex-recipe envelope.
        recipe: DesignVertexRecipe,
    },
    /// Persistent entity selection naming one `WorkPlane` scope.
    WorkPlane {
        /// Exact selection envelope and resolved `WorkPlane` scope.
        selection: DesignWorkPointPlaneSelection,
    },
    /// Direct persistent selection of one sketch point.
    SketchPoint {
        /// Exact selection envelope and resolved native sketch-point record.
        selection: DesignWorkPointSketchPointSelection,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DesignWorkPointInputCarrierWire {
    EdgeRecipe {
        operand_id: String,
    },
    VertexRecipe {
        recipe: DesignVertexRecipe,
    },
    WorkPlane {
        selection: DesignWorkPointPlaneSelectionWire,
    },
    SketchPoint {
        selection: DesignWorkPointSketchPointSelectionWire,
    },
}

impl DesignWorkPointInputCarrierWire {
    fn into_model(self, record_index: u32) -> Result<DesignWorkPointInputCarrier, String> {
        Ok(match self {
            Self::EdgeRecipe { operand_id } => {
                DesignWorkPointInputCarrier::EdgeRecipe { operand_id }
            }
            Self::VertexRecipe { recipe } => DesignWorkPointInputCarrier::VertexRecipe { recipe },
            Self::WorkPlane { selection } => DesignWorkPointInputCarrier::WorkPlane {
                selection: DesignWorkPointPlaneSelection::from_wire(record_index, selection)?,
            },
            Self::SketchPoint { selection } => DesignWorkPointInputCarrier::SketchPoint {
                selection: DesignWorkPointSketchPointSelection::from_wire(record_index, selection)?,
            },
        })
    }
}

impl DesignWorkPointInputCarrier {
    fn into_wire(self, record_index: u32) -> DesignWorkPointInputCarrierWire {
        match self {
            Self::EdgeRecipe { operand_id } => {
                DesignWorkPointInputCarrierWire::EdgeRecipe { operand_id }
            }
            Self::VertexRecipe { recipe } => {
                DesignWorkPointInputCarrierWire::VertexRecipe { recipe }
            }
            Self::WorkPlane { selection } => DesignWorkPointInputCarrierWire::WorkPlane {
                selection: selection.into_wire(record_index),
            },
            Self::SketchPoint { selection } => DesignWorkPointInputCarrierWire::SketchPoint {
                selection: selection.into_wire(record_index),
            },
        }
    }
}

/// Historical state and a nonnegative stable vertex slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DesignVertexResolution {
    /// Historical topology state against which the vertex recipe was evaluated.
    pub(crate) state_id: i64,
    vertex_slot: i64,
}

impl DesignVertexResolution {
    pub(crate) fn new(state_id: i64, vertex_slot: i64) -> Option<Self> {
        (vertex_slot >= 0).then_some(Self {
            state_id,
            vertex_slot,
        })
    }

    pub(crate) fn vertex_slot(self) -> i64 {
        self.vertex_slot
    }
}

/// Exact persistent `vertex_recipe_data` envelope.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DesignVertexRecipeWire")]
pub(crate) struct DesignVertexRecipe {
    #[cfg(test)]
    clone_probe: WorkGeometryCloneProbe,
    /// Indexed record that owns the vertex-recipe envelope.
    record_index: u32,
    /// Byte offset of the owning indexed-record header of a located scope.
    byte_offset: Option<u64>,
    /// Source per-file dynamic primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Byte offset of the same-index paired header of a located scope.
    paired_byte_offset: Option<u64>,
    /// Source per-file dynamic paired class tag.
    paired_class_tag: DesignClassTag,
    /// Byte offset of the vertex-recipe record header of a located scope.
    recipe_record_byte_offset: Option<u64>,
    /// Native construction-recipe arena id.
    pub(crate) recipe_id: String,
    /// Complete prefix before the length-prefixed recipe-family name.
    pub(crate) recipe_prefix_bytes: NativeBytes<Vec<u8>>,
    /// Persistent selector/reference entries decoded from the prefix.
    pub(crate) recipe_references: Vec<DesignRecipeReference>,
    /// Byte offset of the first post-name i32 of a located scope.
    pub(crate) recipe_program_offset: Option<u64>,
    /// Complete post-name i32 program.
    pub(crate) recipe_program: Vec<i32>,
    /// Historical state and proven stable vertex slot.
    pub(crate) resolution: Option<DesignVertexResolution>,
    /// Byte offset of the indexed record closing the envelope of a located
    /// scope.
    next_byte_offset: Option<u64>,
}

/// Indexed records the vertex-recipe envelope spans after its owner.
const VERTEX_RECIPE_RECORD_SPAN: u32 = 5;
/// Records between the owner and the vertex-recipe record.
const VERTEX_RECIPE_RECORD_DELTA: u32 = 3;
/// Bytes of the recipe record's indexed header before its prefix.
const VERTEX_RECIPE_PREFIX_DELTA: u64 = 11;

impl DesignVertexRecipe {
    /// Admit a decoded envelope, whose offsets increase and whose record
    /// indices follow the owner.
    pub(crate) fn try_new(draft: DesignVertexRecipeDraft) -> Result<Self, String> {
        if !(draft.byte_offset < draft.paired_byte_offset
            && draft.paired_byte_offset < draft.recipe_record_byte_offset
            && draft.recipe_record_byte_offset < draft.next_byte_offset)
        {
            return Err(
                "paired_byte_offset/recipe_record_byte_offset/next_byte_offset must increase"
                    .into(),
            );
        }
        let recipe_prefix_offset = draft
            .recipe_record_byte_offset
            .checked_add(VERTEX_RECIPE_PREFIX_DELTA)
            .ok_or("recipe_prefix_offset overflows")?;
        if recipe_prefix_offset != draft.recipe_prefix_offset {
            return Err("recipe_prefix_offset disagrees with frame layout".into());
        }
        let mut value = Self::from_fields(VertexRecipeFields {
            record_index: draft.record_index,
            class_tag: draft.class_tag,
            paired_class_tag: draft.paired_class_tag,
            recipe_record_index: draft.recipe_record_index,
            recipe_id: draft.recipe_id,
            recipe_prefix_bytes: draft.recipe_prefix_bytes,
            recipe_references: draft.recipe_references,
            recipe_program: draft.recipe_program,
            resolution: draft.resolution,
            next_record_index: draft.next_record_index,
        })?;
        value.byte_offset = Some(draft.byte_offset);
        value.paired_byte_offset = Some(draft.paired_byte_offset);
        value.recipe_record_byte_offset = Some(draft.recipe_record_byte_offset);
        value.recipe_program_offset = Some(draft.recipe_program_offset);
        value.next_byte_offset = Some(draft.next_byte_offset);
        Ok(value)
    }

    /// Admit the document fields of an envelope.
    fn from_fields(fields: VertexRecipeFields) -> Result<Self, String> {
        let VertexRecipeFields {
            record_index,
            class_tag,
            paired_class_tag,
            recipe_record_index,
            recipe_id,
            recipe_prefix_bytes,
            recipe_references,
            recipe_program,
            resolution,
            next_record_index,
        } = fields;
        record_index
            .checked_add(VERTEX_RECIPE_RECORD_SPAN)
            .ok_or("record_index frame extent overflows")?;
        let value = Self {
            #[cfg(test)]
            clone_probe: WorkGeometryCloneProbe,
            record_index,
            byte_offset: None,
            class_tag,
            paired_byte_offset: None,
            paired_class_tag,
            recipe_record_byte_offset: None,
            recipe_id,
            recipe_prefix_bytes: recipe_prefix_bytes.into(),
            recipe_references,
            recipe_program_offset: None,
            recipe_program,
            resolution,
            next_byte_offset: None,
        };
        if value.recipe_record_index() != recipe_record_index {
            return Err("recipe_record_index disagrees with frame layout".into());
        }
        if value.next_record_index() != next_record_index {
            return Err("next_record_index disagrees with frame layout".into());
        }
        Ok(value)
    }

    /// The draft of a located envelope.
    #[cfg(test)]
    pub(crate) fn into_draft(self) -> Result<DesignVertexRecipeDraft, String> {
        let record_index = self.record_index();
        let recipe_record_index = self.recipe_record_index();
        let recipe_prefix_offset = self
            .recipe_prefix_offset()
            .ok_or("a vertex recipe read from the wire has no source offsets")?;
        let next_record_index = self.next_record_index();
        let missing = || "a vertex recipe read from the wire has no source offsets".to_owned();
        Ok(DesignVertexRecipeDraft {
            record_index,
            byte_offset: self.byte_offset.ok_or_else(missing)?,
            class_tag: self.class_tag,
            paired_byte_offset: self.paired_byte_offset.ok_or_else(missing)?,
            paired_class_tag: self.paired_class_tag,
            recipe_record_index,
            recipe_record_byte_offset: self.recipe_record_byte_offset.ok_or_else(missing)?,
            recipe_id: self.recipe_id,
            recipe_prefix_offset,
            recipe_prefix_bytes: self.recipe_prefix_bytes.into_inner(),
            recipe_references: self.recipe_references,
            recipe_program_offset: self.recipe_program_offset.ok_or_else(missing)?,
            recipe_program: self.recipe_program,
            resolution: self.resolution,
            next_record_index,
            next_byte_offset: self.next_byte_offset.ok_or_else(missing)?,
        })
    }
    pub(crate) fn record_index(&self) -> u32 {
        self.record_index
    }
    /// Byte offset of the owning indexed-record header of a located scope.
    pub(crate) fn byte_offset(&self) -> Option<u64> {
        self.byte_offset
    }
    pub(crate) fn recipe_record_index(&self) -> u32 {
        self.record_index + VERTEX_RECIPE_RECORD_DELTA
    }
    /// Byte offset of the vertex-recipe record header of a located scope.
    pub(crate) fn recipe_record_byte_offset(&self) -> Option<u64> {
        self.recipe_record_byte_offset
    }
    /// Byte offset of the recipe-specific prefix of a located scope.
    pub(crate) fn recipe_prefix_offset(&self) -> Option<u64> {
        self.recipe_record_byte_offset?
            .checked_add(VERTEX_RECIPE_PREFIX_DELTA)
    }
    pub(crate) fn next_record_index(&self) -> u32 {
        self.record_index + VERTEX_RECIPE_RECORD_SPAN
    }
    /// Byte offset of the indexed record closing the envelope of a located
    /// scope.
    pub(crate) fn next_byte_offset(&self) -> Option<u64> {
        self.next_byte_offset
    }
    /// Whether the located offsets increase from the owner header through the
    /// paired header and the recipe record to the closing record.
    pub(crate) fn source_layout_valid(&self) -> bool {
        match (
            self.byte_offset,
            self.paired_byte_offset,
            self.recipe_record_byte_offset,
            self.recipe_program_offset,
            self.next_byte_offset,
        ) {
            (Some(start), Some(paired), Some(recipe), Some(program), Some(next)) => {
                start < paired
                    && paired < recipe
                    && recipe < next
                    && recipe
                        .checked_add(VERTEX_RECIPE_PREFIX_DELTA)
                        .is_some_and(|prefix| prefix <= program && program < next)
            }
            _ => false,
        }
    }
}

impl crate::records::source_offsets::SourceOffsets for DesignVertexRecipe {
    fn visit_source_offsets(
        &mut self,
        offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
    ) {
        offsets.offset("byte_offset", &mut self.byte_offset);
        offsets.offset("paired_byte_offset", &mut self.paired_byte_offset);
        offsets.offset(
            "recipe_record_byte_offset",
            &mut self.recipe_record_byte_offset,
        );
        offsets.offset("recipe_program_offset", &mut self.recipe_program_offset);
        offsets.offset("next_byte_offset", &mut self.next_byte_offset);
        offsets.each("recipe_references", &mut self.recipe_references);
    }
}

/// The document fields of a vertex-recipe envelope.
struct VertexRecipeFields {
    record_index: u32,
    class_tag: DesignClassTag,
    paired_class_tag: DesignClassTag,
    recipe_record_index: u32,
    recipe_id: String,
    recipe_prefix_bytes: Vec<u8>,
    recipe_references: Vec<DesignRecipeReference>,
    recipe_program: Vec<i32>,
    resolution: Option<DesignVertexResolution>,
    next_record_index: u32,
}

/// Unadmitted `DesignVertexRecipe` fields.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignVertexRecipeDraft {
    /// Indexed record that owns the vertex-recipe envelope.
    pub(crate) record_index: u32,
    /// Byte offset of the owning indexed-record header.
    pub(crate) byte_offset: u64,
    /// Source per-file dynamic primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Byte offset of the same-index paired header.
    pub(crate) paired_byte_offset: u64,
    /// Source per-file dynamic paired class tag.
    pub(crate) paired_class_tag: DesignClassTag,
    /// Indexed record containing the vertex recipe.
    pub(crate) recipe_record_index: u32,
    /// Byte offset of the vertex-recipe record header.
    pub(crate) recipe_record_byte_offset: u64,
    /// Native construction-recipe arena id.
    pub(crate) recipe_id: String,
    /// Byte offset of the recipe-specific prefix after the indexed header.
    pub(crate) recipe_prefix_offset: u64,
    /// Complete prefix before the length-prefixed recipe-family name.
    pub(crate) recipe_prefix_bytes: Vec<u8>,
    /// Persistent selector/reference entries decoded from the prefix.
    pub(crate) recipe_references: Vec<DesignRecipeReference>,
    /// Byte offset of the first post-name i32.
    pub(crate) recipe_program_offset: u64,
    /// Complete post-name i32 program.
    pub(crate) recipe_program: Vec<i32>,
    /// Historical state and proven stable vertex slot.
    pub(crate) resolution: Option<DesignVertexResolution>,
    /// Identity of the indexed record closing the envelope.
    pub(crate) next_record_index: u32,
    /// Byte offset of the indexed record closing the envelope.
    pub(crate) next_byte_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct DesignVertexRecipeWire {
    /// Indexed record that owns the vertex-recipe envelope.
    record_index: u32,
    /// Source per-file dynamic primary class tag.
    class_tag: String,
    /// Source per-file dynamic paired class tag.
    paired_class_tag: String,
    /// Indexed record containing the vertex recipe.
    recipe_record_index: u32,
    /// Native construction-recipe arena id.
    recipe_id: String,
    /// Complete prefix before the length-prefixed recipe-family name.
    recipe_prefix_bytes: NativeBytes<Vec<u8>>,
    /// Persistent selector/reference entries decoded from the prefix.
    recipe_references: Vec<DesignRecipeReference>,
    /// Complete post-name i32 program.
    recipe_program: Vec<i32>,
    /// Historical topology state against which the vertex recipe was evaluated.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_recipe_state_id"
    )]
    recipe_state_id: Option<i64>,
    /// Stable vertex slot proven by the persistent face references and solved point.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_resolved_vertex_slot"
    )]
    resolved_vertex_slot: Option<i64>,
    /// Identity of the indexed record closing the envelope.
    next_record_index: u32,
}

impl From<DesignVertexRecipe> for DesignVertexRecipeWire {
    fn from(value: DesignVertexRecipe) -> Self {
        let recipe_record_index = value.recipe_record_index();
        let next_record_index = value.next_record_index();
        Self {
            record_index: value.record_index,
            class_tag: value.class_tag.into(),
            paired_class_tag: value.paired_class_tag.into(),
            recipe_record_index,
            recipe_id: value.recipe_id,
            recipe_prefix_bytes: value.recipe_prefix_bytes,
            recipe_references: value.recipe_references,
            recipe_program: value.recipe_program,
            next_record_index,
            recipe_state_id: value.resolution.map(|resolution| resolution.state_id),
            resolved_vertex_slot: value.resolution.map(DesignVertexResolution::vertex_slot),
        }
    }
}

impl TryFrom<DesignVertexRecipeWire> for DesignVertexRecipe {
    type Error = String;

    fn try_from(value: DesignVertexRecipeWire) -> Result<Self, Self::Error> {
        let resolution = match (value.recipe_state_id, value.resolved_vertex_slot) {
            (None, None) => None,
            (Some(state_id), Some(vertex_slot)) => Some(
                DesignVertexResolution::new(state_id, vertex_slot)
                    .ok_or("resolved_vertex_slot must be nonnegative")?,
            ),
            _ => {
                return Err(
                    "recipe_state_id and resolved_vertex_slot must be present together".into(),
                )
            }
        };
        Self::from_fields(VertexRecipeFields {
            record_index: value.record_index,
            class_tag: value.class_tag.try_into()?,
            paired_class_tag: value.paired_class_tag.try_into()?,
            recipe_record_index: value.recipe_record_index,
            recipe_id: value.recipe_id,
            recipe_prefix_bytes: value.recipe_prefix_bytes.into_inner(),
            recipe_references: value.recipe_references,
            recipe_program: value.recipe_program,
            resolution,
            next_record_index: value.next_record_index,
        })
    }
}

/// Corner-vertex recipe carried as one member of an edge-treatment group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignEdgeTreatmentVertexOperand {
    /// Globally unique deterministic identifier for this group member.
    pub(crate) id: String,
    /// Owning edge-treatment scope record.
    pub(crate) scope_record_index: u32,
    /// Zero-based position in the scope reference table.
    pub(crate) scope_reference_ordinal: u32,
    /// Owning counted construction group.
    pub(crate) group_record_index: u32,
    /// Zero-based position in the group's member run.
    pub(crate) group_member_ordinal: u32,
    /// Exact persistent vertex-recipe envelope and resolved historical corner.
    pub(crate) recipe: DesignVertexRecipe,
}

impl crate::records::source_offsets::SourceOffsets for DesignEdgeTreatmentVertexOperand {
    fn visit_source_offsets(
        &mut self,
        offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
    ) {
        offsets.record("recipe", &mut self.recipe);
    }
}

/// Plane through three persistent B-rep vertices.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DesignWorkPlaneConstructionWire")]
pub(crate) struct DesignWorkPlaneConstruction {
    #[cfg(test)]
    clone_probe: WorkGeometryCloneProbe,
    /// Solved placement-frame record named by the scope.
    pub(crate) placement_record_index: u32,
    /// Persistent vertex inputs in source order.
    inputs: Box<[DesignVertexRecipe; 3]>,
}

impl DesignWorkPlaneConstruction {
    /// Admit an unresolved triple or three distinct vertices from one history state.
    pub(crate) fn try_new(
        placement_record_index: u32,
        inputs: Box<[DesignVertexRecipe; 3]>,
    ) -> Result<Self, String> {
        validate_three_point_resolutions(inputs.each_ref().map(|input| input.resolution))?;
        Ok(Self {
            #[cfg(test)]
            clone_probe: WorkGeometryCloneProbe,
            placement_record_index,
            inputs,
        })
    }

    /// Persistent vertex recipes in source order.
    pub(crate) fn inputs(&self) -> &[DesignVertexRecipe; 3] {
        &self.inputs
    }

    /// Candidate references without mutable access to vertex resolutions.
    pub(crate) fn recipe_references_mut(
        &mut self,
    ) -> impl Iterator<Item = &mut DesignRecipeReference> {
        self.inputs
            .iter_mut()
            .flat_map(|recipe| recipe.recipe_references.iter_mut())
    }

    /// Remove resolution for the complete vertex triple.
    pub(crate) fn clear_resolution(&mut self) {
        for input in self.inputs.iter_mut() {
            input.resolution = None;
        }
    }

    /// Commit three distinct vertices from one state, preserving the old value on failure.
    pub(crate) fn try_set_resolution(
        &mut self,
        resolution: [DesignVertexResolution; 3],
    ) -> Result<(), String> {
        validate_three_point_resolutions(resolution.map(Some))?;
        for (input, resolution) in self.inputs.iter_mut().zip(resolution) {
            input.resolution = Some(resolution);
        }
        Ok(())
    }
}

fn validate_three_point_resolutions(
    resolutions: [Option<DesignVertexResolution>; 3],
) -> Result<(), String> {
    match resolutions {
        [None, None, None] => Ok(()),
        [Some(first), Some(second), Some(third)] => {
            if first.state_id != second.state_id || first.state_id != third.state_id {
                return Err("recipe_state_id must match across all three inputs".into());
            }
            if first.vertex_slot() == second.vertex_slot()
                || first.vertex_slot() == third.vertex_slot()
                || second.vertex_slot() == third.vertex_slot()
            {
                return Err("resolved_vertex_slot must be distinct across all three inputs".into());
            }
            Ok(())
        }
        _ => Err(
            "inputs resolution must be absent for all three vertices or present for all three"
                .into(),
        ),
    }
}

/// Wire form of [`DesignWorkPlaneConstruction`].
///
/// The single variant is deliberate. It stamps `"kind": "three_point"` into
/// the native JSON, keeping room for the other `WorkPlane` constructions
/// Fusion authors, and — unlike an internally tagged struct, which ignores
/// the tag field — it rejects any other `kind` on deserialization.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DesignWorkPlaneConstructionWire {
    ThreePoint {
        placement_record_index: u32,
        inputs: Box<[DesignVertexRecipe; 3]>,
    },
}

impl From<DesignWorkPlaneConstruction> for DesignWorkPlaneConstructionWire {
    fn from(value: DesignWorkPlaneConstruction) -> Self {
        Self::ThreePoint {
            placement_record_index: value.placement_record_index,
            inputs: value.inputs,
        }
    }
}

impl TryFrom<DesignWorkPlaneConstructionWire> for DesignWorkPlaneConstruction {
    type Error = String;
    fn try_from(value: DesignWorkPlaneConstructionWire) -> Result<Self, Self::Error> {
        let DesignWorkPlaneConstructionWire::ThreePoint {
            placement_record_index,
            inputs,
        } = value;
        Self::try_new(placement_record_index, inputs)
    }
}

/// Exact persistent entity selection naming one `WorkPlane` scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignWorkPointPlaneSelection {
    /// Source per-file dynamic primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Asset UUID qualifying the selection namespace.
    asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset identifier's UTF-16LE code units of a located
    /// scope.
    asset_id_offset: Option<u64>,
    /// UUID of the selection context.
    context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units of a located
    /// scope.
    context_id_offset: Option<u64>,
    /// Byte offset of the nested identity record of a located scope.
    identity_record_offset: Option<u64>,
    /// Serialized primary identity immediately preceding the `WorkPlane` scope.
    pub(crate) primary_identity: u64,
    /// Selected `WorkPlane` scope record index.
    pub(crate) work_plane_scope_record_index: u32,
}

/// Bytes from the plane-selection identity record to its primary identity.
const PLANE_SELECTION_PRIMARY_IDENTITY: u64 = 21;
/// Bytes from the plane-selection identity record to the closing record.
const PLANE_SELECTION_IDENTITY_LEN: u64 = 29;

impl DesignWorkPointPlaneSelection {
    /// Admit a decoded selection.
    pub(crate) fn try_new(
        record_index: u32,
        draft: DesignWorkPointPlaneSelectionDraft,
    ) -> Result<Self, String> {
        let mut value = Self::from_wire(
            record_index,
            DesignWorkPointPlaneSelectionWire {
                class_tag: draft.class_tag,
                asset_id: draft.asset_id,
                context_id: draft.context_id,
                identity_record_index: draft.identity_record_index,
                primary_identity: draft.primary_identity,
                work_plane_scope_record_index: draft.work_plane_scope_record_index,
                next_record_index: draft.next_record_index,
            },
        )?;
        draft
            .identity_record_offset
            .checked_add(PLANE_SELECTION_IDENTITY_LEN)
            .ok_or("identity_record_offset overflows")?;
        if !(draft.asset_id_offset < draft.context_id_offset
            && draft.context_id_offset < draft.identity_record_offset)
        {
            return Err(
                "asset_id_offset/context_id_offset/identity_record_offset must increase".into(),
            );
        }
        value.asset_id_offset = Some(draft.asset_id_offset);
        value.context_id_offset = Some(draft.context_id_offset);
        value.identity_record_offset = Some(draft.identity_record_offset);
        if value.next_byte_offset() != Some(draft.next_byte_offset) {
            return Err("next_byte_offset disagrees with frame layout".into());
        }
        if value.primary_identity_offset() != Some(draft.primary_identity_offset) {
            return Err("primary_identity_offset disagrees with frame layout".into());
        }
        Ok(value)
    }
    fn from_wire(
        record_index: u32,
        wire: DesignWorkPointPlaneSelectionWire,
    ) -> Result<Self, String> {
        if record_index.checked_add(3) != Some(wire.identity_record_index) {
            return Err("carrier.identity_record_index must follow record_index by 3".into());
        }
        if record_index.checked_add(4) != Some(wire.next_record_index) {
            return Err("carrier.next_record_index must follow record_index by 4".into());
        }
        Ok(Self {
            class_tag: wire.class_tag,
            asset_id: wire.asset_id,
            asset_id_offset: None,
            context_id: wire.context_id,
            context_id_offset: None,
            identity_record_offset: None,
            primary_identity: wire.primary_identity,
            work_plane_scope_record_index: wire.work_plane_scope_record_index,
        })
    }
    fn into_wire(self, record_index: u32) -> DesignWorkPointPlaneSelectionWire {
        DesignWorkPointPlaneSelectionWire {
            class_tag: self.class_tag,
            asset_id: self.asset_id,
            context_id: self.context_id,
            identity_record_index: record_index + 3,
            primary_identity: self.primary_identity,
            work_plane_scope_record_index: self.work_plane_scope_record_index,
            next_record_index: record_index + 4,
        }
    }
    fn primary_identity_offset(&self) -> Option<u64> {
        self.identity_record_offset?
            .checked_add(PLANE_SELECTION_PRIMARY_IDENTITY)
    }
    fn next_byte_offset(&self) -> Option<u64> {
        self.identity_record_offset?
            .checked_add(PLANE_SELECTION_IDENTITY_LEN)
    }
    /// Whether the located offsets increase from the asset identifier
    /// through the context identifier to the identity record.
    pub(crate) fn source_layout_valid(&self) -> bool {
        match (
            self.asset_id_offset,
            self.context_id_offset,
            self.identity_record_offset,
        ) {
            (Some(asset), Some(context), Some(identity)) => {
                asset < context
                    && context < identity
                    && identity.checked_add(PLANE_SELECTION_IDENTITY_LEN).is_some()
            }
            _ => false,
        }
    }
}

impl crate::records::source_offsets::SourceOffsets for DesignWorkPointPlaneSelection {
    fn visit_source_offsets(
        &mut self,
        offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
    ) {
        offsets.offset("asset_id_offset", &mut self.asset_id_offset);
        offsets.offset("context_id_offset", &mut self.context_id_offset);
        offsets.offset("identity_record_offset", &mut self.identity_record_offset);
    }
}

/// Document fields of a `DesignWorkPointPlaneSelection`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignWorkPointPlaneSelectionWire {
    /// Source per-file dynamic primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Asset UUID qualifying the selection namespace.
    pub(crate) asset_id: DesignRelaxedGuidText,
    /// UUID of the selection context.
    pub(crate) context_id: DesignRelaxedGuidText,
    /// Nested indexed record carrying the persistent identity.
    pub(crate) identity_record_index: u32,
    /// Serialized primary identity immediately preceding the `WorkPlane` scope.
    pub(crate) primary_identity: u64,
    /// Selected `WorkPlane` scope record index.
    pub(crate) work_plane_scope_record_index: u32,
    /// Identity of the indexed record closing the selection envelope.
    pub(crate) next_record_index: u32,
}

/// Unadmitted `DesignWorkPointPlaneSelection` fields.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignWorkPointPlaneSelectionDraft {
    /// Source per-file dynamic primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Asset UUID qualifying the selection namespace.
    pub(crate) asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset identifier's UTF-16LE code units.
    pub(crate) asset_id_offset: u64,
    /// UUID of the selection context.
    pub(crate) context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub(crate) context_id_offset: u64,
    /// Nested indexed record carrying the persistent identity.
    pub(crate) identity_record_index: u32,
    /// Byte offset of the nested identity record.
    pub(crate) identity_record_offset: u64,
    /// Serialized primary identity immediately preceding the `WorkPlane` scope.
    pub(crate) primary_identity: u64,
    /// Byte offset of the primary identity.
    pub(crate) primary_identity_offset: u64,
    /// Selected `WorkPlane` scope record index.
    pub(crate) work_plane_scope_record_index: u32,
    /// Identity of the indexed record closing the selection envelope.
    pub(crate) next_record_index: u32,
    /// Byte offset of the indexed record closing the selection envelope.
    pub(crate) next_byte_offset: u64,
}

/// Exact persistent entity selection naming one sketch point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignWorkPointSketchPointSelection {
    /// Source per-file dynamic primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Asset UUID qualifying the selection namespace.
    asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset identifier's UTF-16LE code units of a located
    /// scope.
    asset_id_offset: Option<u64>,
    /// UUID of the selection context.
    context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units of a located
    /// scope.
    context_id_offset: Option<u64>,
    /// Byte offset of the nested identity record of a located scope.
    identity_record_offset: Option<u64>,
    /// Record identity of the owning Sketch entity.
    pub(crate) sketch_record_index: u32,
    /// Persistent identity of the selected sketch point.
    pub(crate) point_persistent_id: u64,
    /// Native id of the decoded sketch-point record selected by this frame.
    pub(crate) point_native_id: String,
}

impl DesignWorkPointSketchPointSelection {
    /// Admit a decoded selection.
    pub(crate) fn try_new(
        record_index: u32,
        draft: DesignWorkPointSketchPointSelectionDraft,
    ) -> Result<Self, String> {
        let mut value = Self::from_wire(
            record_index,
            DesignWorkPointSketchPointSelectionWire {
                class_tag: draft.class_tag,
                asset_id: draft.asset_id,
                context_id: draft.context_id,
                identity_record_index: draft.identity_record_index,
                sketch_record_index: draft.sketch_record_index,
                point_persistent_id: draft.point_persistent_id,
                point_native_id: draft.point_native_id,
                next_record_index: draft.next_record_index,
            },
        )?;
        draft
            .identity_record_offset
            .checked_add(u64_from_index(
                crate::layout::work_point_sketch_point_identity::LEN,
            ))
            .ok_or("identity_record_offset overflows")?;
        if !(draft.asset_id_offset < draft.context_id_offset
            && draft.context_id_offset < draft.identity_record_offset)
        {
            return Err(
                "asset_id_offset/context_id_offset/identity_record_offset must increase".into(),
            );
        }
        value.asset_id_offset = Some(draft.asset_id_offset);
        value.context_id_offset = Some(draft.context_id_offset);
        value.identity_record_offset = Some(draft.identity_record_offset);
        if value.next_byte_offset() != Some(draft.next_byte_offset) {
            return Err("next_byte_offset disagrees with frame layout".into());
        }
        if value.sketch_record_index_offset() != Some(draft.sketch_record_index_offset) {
            return Err("sketch_record_index_offset disagrees with frame layout".into());
        }
        if value.point_persistent_id_offset() != Some(draft.point_persistent_id_offset) {
            return Err("point_persistent_id_offset disagrees with frame layout".into());
        }
        Ok(value)
    }
    fn from_wire(
        record_index: u32,
        wire: DesignWorkPointSketchPointSelectionWire,
    ) -> Result<Self, String> {
        if record_index.checked_add(3) != Some(wire.identity_record_index)
            || record_index.checked_add(4) != Some(wire.next_record_index)
        {
            return Err("carrier identity/next_record_index disagree with input frame".into());
        }
        Ok(Self {
            class_tag: wire.class_tag,
            asset_id: wire.asset_id,
            asset_id_offset: None,
            context_id: wire.context_id,
            context_id_offset: None,
            identity_record_offset: None,
            sketch_record_index: wire.sketch_record_index,
            point_persistent_id: wire.point_persistent_id,
            point_native_id: wire.point_native_id,
        })
    }
    fn into_wire(self, record_index: u32) -> DesignWorkPointSketchPointSelectionWire {
        DesignWorkPointSketchPointSelectionWire {
            class_tag: self.class_tag,
            asset_id: self.asset_id,
            context_id: self.context_id,
            identity_record_index: record_index + 3,
            sketch_record_index: self.sketch_record_index,
            point_persistent_id: self.point_persistent_id,
            point_native_id: self.point_native_id,
            next_record_index: record_index + 4,
        }
    }
    fn sketch_record_index_offset(&self) -> Option<u64> {
        self.identity_record_offset?.checked_add(u64_from_index(
            crate::layout::work_point_sketch_point_identity::SKETCH_RECORD_INDEX,
        ))
    }
    fn point_persistent_id_offset(&self) -> Option<u64> {
        self.identity_record_offset?.checked_add(u64_from_index(
            crate::layout::work_point_sketch_point_identity::POINT_PERSISTENT_ID,
        ))
    }
    fn next_byte_offset(&self) -> Option<u64> {
        self.identity_record_offset?.checked_add(u64_from_index(
            crate::layout::work_point_sketch_point_identity::LEN,
        ))
    }
    /// Whether the located offsets increase from the asset identifier
    /// through the context identifier to the identity record.
    pub(crate) fn source_layout_valid(&self) -> bool {
        match (
            self.asset_id_offset,
            self.context_id_offset,
            self.identity_record_offset,
        ) {
            (Some(asset), Some(context), Some(identity)) => {
                asset < context && context < identity && self.next_byte_offset().is_some()
            }
            _ => false,
        }
    }
}

impl crate::records::source_offsets::SourceOffsets for DesignWorkPointSketchPointSelection {
    fn visit_source_offsets(
        &mut self,
        offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
    ) {
        offsets.offset("asset_id_offset", &mut self.asset_id_offset);
        offsets.offset("context_id_offset", &mut self.context_id_offset);
        offsets.offset("identity_record_offset", &mut self.identity_record_offset);
    }
}

/// Document fields of a `DesignWorkPointSketchPointSelection`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignWorkPointSketchPointSelectionWire {
    /// Source per-file dynamic primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Asset UUID qualifying the selection namespace.
    pub(crate) asset_id: DesignRelaxedGuidText,
    /// UUID of the selection context.
    pub(crate) context_id: DesignRelaxedGuidText,
    /// Nested indexed record carrying the persistent identity.
    pub(crate) identity_record_index: u32,
    /// Record identity of the owning Sketch entity.
    pub(crate) sketch_record_index: u32,
    /// Persistent identity of the selected sketch point.
    pub(crate) point_persistent_id: u64,
    /// Native id of the decoded sketch-point record selected by this frame.
    pub(crate) point_native_id: String,
    /// Identity of the indexed record closing the selection envelope.
    pub(crate) next_record_index: u32,
}

/// Unadmitted `DesignWorkPointSketchPointSelection` fields.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignWorkPointSketchPointSelectionDraft {
    /// Source per-file dynamic primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Asset UUID qualifying the selection namespace.
    pub(crate) asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset identifier's UTF-16LE code units.
    pub(crate) asset_id_offset: u64,
    /// UUID of the selection context.
    pub(crate) context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub(crate) context_id_offset: u64,
    /// Nested indexed record carrying the persistent identity.
    pub(crate) identity_record_index: u32,
    /// Byte offset of the nested identity record.
    pub(crate) identity_record_offset: u64,
    /// Record identity of the owning Sketch entity.
    pub(crate) sketch_record_index: u32,
    /// Byte offset of the Sketch entity identity.
    pub(crate) sketch_record_index_offset: u64,
    /// Persistent identity of the selected sketch point.
    pub(crate) point_persistent_id: u64,
    /// Byte offset of the sketch-point identity.
    pub(crate) point_persistent_id_offset: u64,
    /// Native id of the decoded sketch-point record selected by this frame.
    pub(crate) point_native_id: String,
    /// Identity of the indexed record closing the selection envelope.
    pub(crate) next_record_index: u32,
    /// Byte offset of the indexed record closing the selection envelope.
    pub(crate) next_byte_offset: u64,
}

/// Construction rule whose input arity and decoded carrier roles agree.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DesignWorkPointRuleForm")]
pub(crate) struct DesignWorkPointRule {
    form: DesignWorkPointRuleForm,
}

impl Serialize for DesignWorkPointRule {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.form.serialize(serializer)
    }
}

impl DesignWorkPointRule {
    pub(crate) fn from_serialized(
        reference_type: u32,
        inputs: Vec<DesignWorkPointInput>,
    ) -> Result<Self, String> {
        let form = match (reference_type, inputs.len()) {
            (5, 1) => DesignWorkPointRuleForm::CircleCenter {
                input: take_one_work_point_input(inputs)?,
            },
            (7, 2) => DesignWorkPointRuleForm::TwoEdgeIntersection {
                inputs: take_work_point_inputs(inputs)?,
            },
            (8, 3) => DesignWorkPointRuleForm::ThreePlaneIntersection {
                inputs: take_work_point_inputs(inputs)?,
            },
            (10, 1) => DesignWorkPointRuleForm::Vertex {
                input: take_one_work_point_input(inputs)?,
            },
            (14, 2) => DesignWorkPointRuleForm::EdgePlaneIntersection {
                inputs: take_work_point_inputs(inputs)?,
            },
            (20, 1) => DesignWorkPointRuleForm::DistanceOnEdge {
                input: take_one_work_point_input(inputs)?,
            },
            _ => DesignWorkPointRuleForm::Native {
                reference_type,
                inputs,
            },
        };
        let is_edge = |input: &DesignWorkPointInput| {
            input.carrier.as_deref().is_none_or(|carrier| {
                matches!(carrier, DesignWorkPointInputCarrier::EdgeRecipe { .. })
            })
        };
        let is_vertex = |input: &DesignWorkPointInput| {
            input.carrier.as_deref().is_none_or(|carrier| {
                matches!(
                    carrier,
                    DesignWorkPointInputCarrier::VertexRecipe { .. }
                        | DesignWorkPointInputCarrier::SketchPoint { .. }
                )
            })
        };
        let is_plane = |input: &DesignWorkPointInput| {
            input.carrier.as_deref().is_none_or(|carrier| {
                matches!(carrier, DesignWorkPointInputCarrier::WorkPlane { .. })
            })
        };
        let compatible = match &form {
            DesignWorkPointRuleForm::CircleCenter { input }
            | DesignWorkPointRuleForm::DistanceOnEdge { input } => is_edge(input),
            DesignWorkPointRuleForm::TwoEdgeIntersection { inputs } => inputs.iter().all(is_edge),
            DesignWorkPointRuleForm::ThreePlaneIntersection { inputs } => {
                inputs.iter().all(is_plane)
            }
            DesignWorkPointRuleForm::Vertex { input } => is_vertex(input),
            DesignWorkPointRuleForm::EdgePlaneIntersection { inputs } => {
                is_edge(&inputs[0]) && is_plane(&inputs[1])
            }
            DesignWorkPointRuleForm::Native { .. } => true,
        };
        if !compatible {
            return Err(
                "WorkPoint rule input carrier conflicts with its reference_type role".into(),
            );
        }
        Ok(Self { form })
    }

    pub(crate) fn form(&self) -> &DesignWorkPointRuleForm {
        &self.form
    }

    pub(crate) fn reference_type(&self) -> u32 {
        self.form.reference_type()
    }

    pub(crate) fn inputs(&self) -> &[DesignWorkPointInput] {
        self.form.inputs()
    }

    pub(crate) fn vertex_recipes_mut(&mut self) -> impl Iterator<Item = &mut DesignVertexRecipe> {
        self.form.inputs_mut().iter_mut().filter_map(|input| {
            match input.carrier.as_deref_mut()? {
                DesignWorkPointInputCarrier::VertexRecipe { recipe } => Some(recipe),
                _ => None,
            }
        })
    }
}

fn take_work_point_inputs<const N: usize>(
    inputs: Vec<DesignWorkPointInput>,
) -> Result<[DesignWorkPointInput; N], String> {
    inputs
        .try_into()
        .map_err(|_| "WorkPoint input arity changed during admission".into())
}

fn take_one_work_point_input(
    inputs: Vec<DesignWorkPointInput>,
) -> Result<DesignWorkPointInput, String> {
    let [input] = take_work_point_inputs(inputs)?;
    Ok(input)
}

impl TryFrom<DesignWorkPointRuleForm> for DesignWorkPointRule {
    type Error = String;

    fn try_from(value: DesignWorkPointRuleForm) -> Result<Self, Self::Error> {
        let canonical = Self::from_serialized(value.reference_type(), value.inputs().to_vec())?;
        if std::mem::discriminant(&value) != std::mem::discriminant(canonical.form()) {
            return Err(
                "WorkPoint native rule reference_type and input arity identify a supported rule"
                    .into(),
            );
        }
        Ok(canonical)
    }
}

/// Construction rule and exact input arity carried by a `WorkPoint` point-data record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum DesignWorkPointRuleForm {
    /// Center of one selected circular edge.
    CircleCenter {
        /// Selected circular-edge carrier.
        input: DesignWorkPointInput,
    },
    /// Intersection of two selected edges.
    TwoEdgeIntersection {
        /// Selected edge carriers in source order.
        inputs: [DesignWorkPointInput; 2],
    },
    /// Intersection of three selected planes.
    ThreePlaneIntersection {
        /// Selected plane carriers in source order.
        inputs: [DesignWorkPointInput; 3],
    },
    /// One selected B-rep vertex.
    Vertex {
        /// Selected vertex carrier.
        input: DesignWorkPointInput,
    },
    /// Intersection of one selected edge and one selected plane, in source order.
    EdgePlaneIntersection {
        /// Edge and plane carriers in serialized order.
        inputs: [DesignWorkPointInput; 2],
    },
    /// Point at a specified distance along one selected edge.
    DistanceOnEdge {
        /// Selected edge carrier.
        input: DesignWorkPointInput,
    },
    /// Source rule code and inputs retained without a normalized rule variant.
    Native {
        /// Serialized `refType` value.
        reference_type: u32,
        /// Counted input-reference run in source order.
        inputs: Vec<DesignWorkPointInput>,
    },
}

impl DesignWorkPointRuleForm {
    /// Return the serialized `refType` value.
    fn reference_type(&self) -> u32 {
        match self {
            Self::CircleCenter { .. } => 5,
            Self::TwoEdgeIntersection { .. } => 7,
            Self::ThreePlaneIntersection { .. } => 8,
            Self::Vertex { .. } => 10,
            Self::EdgePlaneIntersection { .. } => 14,
            Self::DistanceOnEdge { .. } => 20,
            Self::Native { reference_type, .. } => *reference_type,
        }
    }

    /// Return the source input references in serialized order.
    fn inputs(&self) -> &[DesignWorkPointInput] {
        match self {
            Self::CircleCenter { input }
            | Self::Vertex { input }
            | Self::DistanceOnEdge { input } => std::slice::from_ref(input),
            Self::TwoEdgeIntersection { inputs } | Self::EdgePlaneIntersection { inputs } => inputs,
            Self::ThreePlaneIntersection { inputs } => inputs,
            Self::Native { inputs, .. } => inputs,
        }
    }

    fn inputs_mut(&mut self) -> &mut [DesignWorkPointInput] {
        match self {
            Self::CircleCenter { input }
            | Self::Vertex { input }
            | Self::DistanceOnEdge { input } => std::slice::from_mut(input),
            Self::TwoEdgeIntersection { inputs } | Self::EdgePlaneIntersection { inputs } => inputs,
            Self::ThreePlaneIntersection { inputs } => inputs,
            Self::Native { inputs, .. } => inputs,
        }
    }
}

/// Exact solved construction carried by a `WorkPoint` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignWorkPointConstruction {
    /// Point-data record selected by the scope.
    pub(crate) point_record_index: u32,
    /// Byte offset of the point-data record header of a located scope.
    #[serde(skip)]
    pub(crate) point_record_byte_offset: Option<u64>,
    /// Solved point in source model centimetres.
    pub(crate) position: [FiniteReal; 3],
    /// Byte offset of the first position coordinate of a located scope.
    #[serde(skip)]
    pub(crate) position_offset: Option<u64>,
    /// Typed construction rule and its source inputs.
    pub(crate) rule: DesignWorkPointRule,
    /// Byte offset of the serialized `refType` value of a located scope.
    #[serde(skip)]
    pub(crate) reference_type_offset: Option<u64>,
}

impl crate::records::source_offsets::SourceOffsets for DesignWorkPointConstruction {
    fn visit_source_offsets(
        &mut self,
        offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
    ) {
        offsets.offset(
            "point_record_byte_offset",
            &mut self.point_record_byte_offset,
        );
        offsets.offset("position_offset", &mut self.position_offset);
        offsets.offset("reference_type_offset", &mut self.reference_type_offset);
        for (index, input) in self.rule.form.inputs_mut().iter_mut().enumerate() {
            offsets.nested(&format!("rule.inputs.{index}"), |offsets| {
                input.visit_source_offsets(offsets);
            });
        }
    }
}

impl crate::records::source_offsets::SourceOffsets for DesignWorkPointInput {
    fn visit_source_offsets(
        &mut self,
        offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
    ) {
        offsets.offset("reference_offset", &mut self.reference_offset);
        if let Some(carrier) = self.carrier.as_deref_mut() {
            match carrier {
                DesignWorkPointInputCarrier::EdgeRecipe { .. } => {}
                DesignWorkPointInputCarrier::VertexRecipe { recipe } => {
                    offsets.record("carrier.recipe", recipe);
                }
                DesignWorkPointInputCarrier::WorkPlane { selection } => {
                    offsets.record("carrier.selection", selection);
                }
                DesignWorkPointInputCarrier::SketchPoint { selection } => {
                    offsets.record("carrier.selection", selection);
                }
            }
        }
    }
}

impl crate::records::source_offsets::SourceOffsets for DesignWorkAxisConstruction {
    fn visit_source_offsets(
        &mut self,
        offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
    ) {
        offsets.offset("origin_offset", &mut self.origin_offset);
        offsets.offset("displacement_offset", &mut self.displacement_offset);
        if let Some(DesignWorkAxisSource::TwoPoint { point_offsets, .. }) = &mut self.source {
            offsets.offsets("source.TwoPoint.point_offsets", point_offsets);
        }
    }
}

impl crate::records::source_offsets::SourceOffsets for DesignWorkPlaneConstruction {
    fn visit_source_offsets(
        &mut self,
        offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
    ) {
        offsets.each("inputs", &mut self.inputs[..]);
    }
}

impl DesignWorkPointInput {
    pub(crate) fn try_set_carrier(
        &mut self,
        carrier: Option<Box<DesignWorkPointInputCarrier>>,
    ) -> Result<(), String> {
        *self = Self::try_new(self.record_index, self.reference_offset, carrier)?;
        Ok(())
    }
}

mod serialize;

mod identity_rewrite;
