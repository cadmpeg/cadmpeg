// SPDX-License-Identifier: Apache-2.0
//! Frame NX object-model entities using external boundary and identity arrays.

pub(crate) mod journal_group;
pub(crate) mod state_journal;
use journal_group::JournalGroup;
pub(crate) mod state_counter;
use state_counter::StateCounterMap;
pub(crate) mod column_row;
pub(crate) mod compact_lane;
pub(crate) mod reference_value;
use reference_value::{DirectReference, LocatedReference, RecordReference, Tagged28};

pub(crate) mod csys_descriptor;
pub(crate) mod datum_csys;
pub(crate) mod datum_index;
pub(crate) mod datum_plane_header;
pub(crate) mod draft_identity;
pub(crate) mod draft_leading;
pub(crate) mod draft_references;
pub(crate) mod draft_terminal;
pub(crate) mod header_references;
pub(crate) mod operation_record;
pub(crate) mod plane_descriptor;
pub(crate) mod reference_index;
pub(crate) mod sketch_references;
use header_references::{HeaderReferences, OperationHeader};
use operation_record::{OperationBodyInput, OperationPayload, OperationRecord};
use sketch_references::SketchReferenceField;
pub(crate) mod block_construction;
pub(crate) mod body_write;
pub(crate) mod common_frame;
pub(crate) mod direct_reference;
use body_write::{BodyImageTag, BodyWriteFrame, BodyWriteIndex};
use common_frame::{CommonFrame, CommonFramePrefix, CommonFrameSuffix, TerminalFrame};
pub(crate) mod instances;
use reference_index::ReferenceIndexToken;
pub(crate) mod control_word;
use control_word::ControlWord24;

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU8;
use std::sync::Arc;

use crate::printable_string::PrintableString;
use cadmpeg_core::decode::{alloc_filled, View};

pub(crate) mod compact;
use compact::{CompactIndexAtom, LocatedCompactIndex, NullableCompactIndex};
pub(crate) mod color;
use color::{ColorComponent, PaletteIndex, BACKGROUND_NAME, PALETTE_SIZE};
pub(crate) mod body_scalar_triple;
pub(crate) mod branch_items;
pub(crate) mod discriminators;
pub(crate) mod extrude_32;
pub(crate) mod extrude_profile;
pub(crate) mod simple_hole_references;
pub(crate) mod surface_branches;
pub(crate) mod surface_envelope;
pub(crate) mod terminal_discriminator;
use branch_items::BranchItems;
pub(crate) mod binary64_pair;
pub(crate) mod name_field;
pub(crate) mod parameter_name;
pub(crate) mod scalar_pair;
use scalar_pair::{DatumPairForm, SketchPairForm};
pub(crate) mod scalar_run;
use scalar_run::FramedScalarRun;
pub(crate) mod sketch_scalar;
use sketch_scalar::{SketchMixedScalars, SketchScalarLaneForm, SketchScaledAtom};
pub(crate) mod fixed;
use fixed::{Q155Atom, Q155LaneFrame, Q155Marker, Q155};
pub(crate) mod nonempty;
pub(crate) mod state_index;
pub(crate) mod state_slot_lane;
pub(crate) mod state_slots;
pub(crate) mod state_status;
pub(crate) mod state_table;
pub(crate) mod state_tagged_value;
use state_table::OperationStateStatusTable;
pub(crate) mod state_block;
use state_block::{operation_state_block_before_boundary, OperationStateBlock};
pub(crate) mod roll_forward;
pub(crate) mod state_link;
use roll_forward::{
    operation_state_group_at, operation_state_group_end_at, OperationStateGroupTable,
};
pub(crate) mod state_group;
use state_index::OperationStateIndex;
pub(crate) mod state_message_text;
use nonempty::NonEmpty;
pub(crate) mod state_message;
use state_message::OperationStateMessage;
use state_tagged_value::StateTaggedValue;
pub(crate) mod counted_pattern_references;
pub(crate) mod delete_references;
pub(crate) mod fset_references;
pub(crate) mod pattern;
pub(crate) mod pattern_references;
pub(crate) mod projected_references;
use pattern::{PatternRow, PatternRows, PatternTerminal, PatternValue, PatternWideValues};
pub(crate) mod scalar;
pub(crate) mod swp104_state;
use scalar::{
    shifted_ieee_f64, LocatedBinary64, PayloadScalarAtom, RepeatedScalar, ShiftedBinary32,
    ShiftedBinary64, ShiftedScalar,
};
pub(crate) mod thru_curve_branches;
pub(crate) mod thru_curve_controls;
pub(crate) mod thru_curve_endings;
pub(crate) mod thru_curve_state;
use discriminators::DraftBinary32Branch;
use swp104_state::Swp104StateLane;
pub(crate) mod audit;
pub(crate) mod cache;
pub(crate) mod product;
pub(crate) mod registry;
use audit::{AuditRecord, AuditTrailRow};
pub(crate) mod control_leading_value;
use control_leading_value::ControlLeadingValue;
use product::{ProductRecord, ProductRecordForm, ProductText};
mod index_table;
use index_table::{DescendingU32Edges, FixedIndex, OffsetIndex};
use parameter_name::ParameterName;

/// One NX object-model entity payload without a fixed object-id table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityRecord<'a> {
    /// Absolute byte offset of the entity payload.
    pub offset: usize,
    /// Exactly bounded serialized entity payload.
    pub bytes: &'a [u8],
}

/// One NX object-model entity in a fixed-width object-id table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedEntityRecord<'a> {
    /// Object identifier and the absolute offset of its table word.
    pub object_id: (u32, u64),
    /// Absolute byte offset of the entity payload.
    pub offset: usize,
    /// Exactly bounded serialized entity payload.
    pub bytes: &'a [u8],
}

/// How one indexed section stores entity identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexedStore<'a> {
    /// Every record carries a fixed-width object id.
    Fixed {
        /// Entity records following the reserved zero-offset slot.
        records: Arc<[FixedEntityRecord<'a>]>,
    },
    /// Identity lives in the control block and column storage.
    OffsetOnly {
        /// Store-level control block bounded by slot zero.
        control: EntityRecord<'a>,
        /// Contiguous column-storage region after the control block.
        column_storage: &'a [u8],
        /// Entity records following the reserved zero-offset slot.
        records: Arc<[EntityRecord<'a>]>,
    },
}

/// One length-framed NX object-model class definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDefinition<'a> {
    /// Absolute byte offset of the definition's length byte.
    pub offset: usize,
    /// Registered `UGS::` class name.
    pub name: &'a str,
    /// Complete registry bytes following the class name.
    pub registry_tail: &'a [u8],
}

/// One member declaration in an NX OM field registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDefinition<'a> {
    /// Offset of the declaration length byte.
    pub offset: usize,
    /// Registered `m_` member name.
    pub name: &'a str,
    /// Complete registry bytes following the member name.
    pub registry_tail: &'a [u8],
}

/// One self-framed printable string value in an NX OM entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringValue<'a> {
    /// Absolute byte offset of the `66 32 03` marker.
    pub offset: usize,
    /// Printable value bytes.
    pub value: PrintableString<&'a str>,
}

/// One canonical UUID in the compact NX OM string frame `03 26, text, 00`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UuidStringValue<'a> {
    /// Absolute byte offset of the `03 26` marker.
    pub offset: usize,
    /// Canonical lowercase UUID text.
    pub value: crate::canonical_uuid::CanonicalUuid<&'a str>,
}

/// One self-framed printable string in a surface-referenced payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfacePayloadString<'a> {
    /// Payload-relative offset of the `66 1b 03` marker.
    pub offset: usize,
    /// Exact non-empty string value.
    pub value: crate::payload_text::PayloadText<&'a str>,
}

/// Self-framed NX product/version marker in an OM store root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreVersion<'a> {
    /// Absolute offset of the `04 01` marker.
    pub offset: usize,
    /// Exact printable product/version text, including the `NX ` prefix.
    pub value: ProductText<&'a str>,
}

/// Header of an internally pointed size-framed OM record area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordAreaHeader<'a> {
    /// Absolute offset of the first control word.
    pub offset: usize,
    /// Three little-endian control words preceding the product record.
    pub control_words: [u32; 3],
    /// Product/version record following the control words.
    pub product: StoreVersion<'a>,
}

/// One RGB definition from an NX part color table.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorTableDefinition<'a> {
    /// Color name paired by table order.
    pub name: &'a str,
    /// Exact normalized components and their payload offsets.
    pub components: [(ColorComponent, usize); 3],
    /// Byte offset of the opening `05` marker.
    pub offset: usize,
}

/// Complete 216-entry NX part color table.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorTable<'a> {
    /// Byte offset of the counted name roster.
    pub offset: usize,
    /// Exact background components and their payload offsets.
    pub background: [(ColorComponent, usize); 3],
    /// Ordered definitions for color indices 1 through 216.
    pub definitions: [ColorTableDefinition<'a>; PALETTE_SIZE],
}

fn color_components(bytes: &[u8], at: &mut usize) -> Option<[(ColorComponent, usize); 3]> {
    (0..3)
        .map(|_| {
            let offset = *at;
            let component = ColorComponent::read(bytes.get(offset..)?)?;
            *at += component.raw().len();
            Some((component, offset))
        })
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

fn color_name_frame(bytes: &[u8], offset: usize) -> Option<(&str, usize)> {
    let byte_len = usize::from(bytes.get(offset).copied()?);
    if byte_len < 2 {
        return None;
    }
    let end = offset.checked_add(byte_len)?;
    let text = bytes.get(offset + 1..end)?.strip_suffix(&[0])?;
    if text.is_empty()
        || !text
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    {
        return None;
    }
    Some((std::str::from_utf8(text).ok()?, byte_len))
}

const COLOR_TABLE_NAME_HEADER: [u8; 4] = [0x02, 0x80, 0xd9, 0x01];
const COLOR_TABLE_DEFINITION_PREAMBLE: [u8; 21] = [
    0x02, 0x14, 0xff, 0x06, 0x00, 0xf0, 0x02, 0x80, 0x9d, 0x80, 0xc7, 0x00, 0xc0, 0x13, 0x0a, 0xc6,
    0x01, 0x80, 0xd9, 0x80, 0xc8,
];

// Validate a complete palette through borrowed names, component widths, and
// index tokens. The owning color vectors are materialized only after this
// self-framed candidate has passed every check.
fn color_table_end(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start..start + COLOR_TABLE_NAME_HEADER.len()) != Some(&COLOR_TABLE_NAME_HEADER) {
        return None;
    }
    let mut at = start + COLOR_TABLE_NAME_HEADER.len();
    for ordinal in 0..=216 {
        let (name, width) = color_name_frame(bytes, at)?;
        if ordinal == 0 && name != BACKGROUND_NAME {
            return None;
        }
        at += width;
    }
    if bytes.get(at..at + COLOR_TABLE_DEFINITION_PREAMBLE.len())
        != Some(&COLOR_TABLE_DEFINITION_PREAMBLE)
    {
        return None;
    }
    at += COLOR_TABLE_DEFINITION_PREAMBLE.len();
    for _ in 0..3 {
        at += ColorComponent::read(bytes.get(at..)?)?.raw().len();
    }
    for color_index in PaletteIndex::all() {
        if bytes.get(at) != Some(&0x05) {
            return None;
        }
        at += 1;
        let (token, width) = color_index.definition_token();
        if bytes.get(at..at + width) != Some(&token[..width]) {
            return None;
        }
        at += width;
        if bytes.get(at..at + 3) != Some(&[0x01, 0x80, 0xc8]) {
            return None;
        }
        at += 3;
        for _ in 0..3 {
            at += ColorComponent::read(bytes.get(at..)?)?.raw().len();
        }
    }
    Some(at)
}

fn color_table_at(bytes: &[u8], start: usize) -> Option<ColorTable<'_>> {
    let mut at = start + COLOR_TABLE_NAME_HEADER.len();
    let mut names = Vec::with_capacity(217);
    for _ in 0..217 {
        let (name, width) = color_name_frame(bytes, at)?;
        names.push(name);
        at += width;
    }
    at += COLOR_TABLE_DEFINITION_PREAMBLE.len();

    let background = color_components(bytes, &mut at)?;
    let mut definitions = Vec::with_capacity(PALETTE_SIZE);
    for color_index in PaletteIndex::all() {
        let offset = at;
        at += 1 + color_index.definition_token().1 + 3;
        let components = color_components(bytes, &mut at)?;
        definitions.push(ColorTableDefinition {
            name: names[usize::from(color_index.value())],
            components,
            offset,
        });
    }
    Some(ColorTable {
        offset: start,
        background,
        definitions: definitions.try_into().ok()?,
    })
}

/// Decode every complete NX part color table in a bounded byte region.
pub fn color_tables(bytes: &[u8]) -> Vec<ColorTable<'_>> {
    let mut tables = Vec::new();
    let mut start = 0;
    while start + COLOR_TABLE_NAME_HEADER.len() <= bytes.len() {
        if bytes.get(start..start + COLOR_TABLE_NAME_HEADER.len()) != Some(&COLOR_TABLE_NAME_HEADER)
        {
            start += 1;
            continue;
        }
        let Some(end) = color_table_end(bytes, start) else {
            start += 1;
            continue;
        };
        if let Some(table) = color_table_at(bytes, start) {
            tables.push(table);
        }
        start = end;
    }
    tables
}

/// One exact shifted-IEEE scalar field in a reconstructed construction payload.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConstructionPayloadScalarField {
    /// Payload-relative offset of the `50 59 66` marker.
    pub offset: usize,
    /// Serialized field discriminator following the marker.
    pub field_code: u8,
    /// Checked shifted-binary64 atom.
    pub scalar: ShiftedBinary64,
}

const SHIFTED_BINARY64_SCALAR_FRAME_LEN: usize = 13;

/// Decode exact `50 59 66, field_code, 00, shifted-f64` construction fields.
pub fn construction_payload_scalar_fields(bytes: &[u8]) -> Vec<ConstructionPayloadScalarField> {
    let mut fields = Vec::new();
    for start in 0..bytes.len().saturating_sub(12) {
        if bytes.get(start..start + 3) != Some(b"PYf") || bytes.get(start + 4) != Some(&0x00) {
            continue;
        }
        let Some(scalar) = bytes
            .get(start + 5..start + SHIFTED_BINARY64_SCALAR_FRAME_LEN)
            .and_then(ShiftedBinary64::read)
        else {
            continue;
        };
        fields.push(ConstructionPayloadScalarField {
            offset: start,
            field_code: bytes[start + 3],
            scalar,
        });
    }
    fields
}

/// Exact type-free named point record spanning consecutive store blocks.
#[derive(Debug, Clone, PartialEq)]
pub struct OffsetStoreNamedPoint {
    /// Exact `Point<positive decimal>` name.
    pub name: String,
    /// Two checked scalar atoms and their frame offsets in block order.
    pub values: [LocatedBinary64; 2],
    /// Minimal number of consecutive blocks containing both scalar frames.
    pub block_count: usize,
}

/// Decode a named two-scalar point from a streaming block sequence.
pub(crate) fn offset_store_named_point<'a>(
    blocks: impl IntoIterator<Item = &'a [u8]>,
) -> Option<OffsetStoreNamedPoint> {
    let mut bytes = Vec::new();
    let mut candidate = None;
    for (block_ordinal, block) in blocks.into_iter().enumerate() {
        // A later type-free name starts the next bounded data-block object.
        if !bytes.is_empty()
            && name_field::scan(block)
                .first()
                .is_some_and(|name| name.code().is_none())
        {
            return candidate;
        }
        bytes.extend_from_slice(block);
        let names = name_field::scan(&bytes);
        let name = names.first()?;
        if name.code().is_some() || parse_positive_decimal_suffix(name.value(), "Point").is_none() {
            return None;
        }
        let next_name = names
            .iter()
            .find(|next| next.code().is_some() && next.offset() > name.offset());
        let interval_end = next_name.map_or(bytes.len(), name_field::NameField::offset);
        let scalars = construction_payload_scalar_fields(&bytes)
            .into_iter()
            .filter(|scalar| {
                scalar.offset > name.offset()
                    && scalar
                        .offset
                        .checked_add(SHIFTED_BINARY64_SCALAR_FRAME_LEN)
                        .is_some_and(|end| end <= interval_end)
            })
            .collect::<Vec<_>>();
        match scalars.as_slice() {
            [] | [_] => {}
            [first_scalar, second_scalar] => {
                candidate.get_or_insert_with(|| OffsetStoreNamedPoint {
                    name: name.value().to_string(),
                    values: [first_scalar, second_scalar].map(|field| LocatedBinary64 {
                        scalar: field.scalar,
                        offset: field.offset,
                    }),
                    block_count: block_ordinal + 1,
                });
            }
            _ => return None,
        }
        if next_name.is_some() {
            return candidate;
        }
    }
    candidate
}

fn parse_positive_decimal_suffix(value: &str, prefix: &str) -> Option<u32> {
    let suffix = value.strip_prefix(prefix)?;
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let ordinal = suffix.parse::<u32>().ok()?;
    (ordinal != 0).then_some(ordinal)
}

/// Unit declared by an NX numeric-expression serialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpressionUnit {
    /// Model length in millimeters as serialized by NX.
    Millimeter,
    /// Model length in inches as serialized by NX.
    Inch,
    /// Angular value in degrees as serialized by NX.
    Degree,
    /// Unit label without a neutral dimensional mapping.
    Native(String),
}

/// One numeric expression decoded from an exactly bounded OM entity.
#[derive(Debug, Clone, PartialEq)]
pub struct NumericExpression<'a> {
    /// Persistent identity of the containing OM entity, when indexed.
    pub object_id: Option<u32>,
    /// Absolute byte offset of the expression text.
    pub offset: usize,
    /// NX parameter name.
    pub name: ParameterName<&'a str>,
    /// Declared native unit.
    pub unit: ExpressionUnit,
    /// Exact expression text following the serialized name separator.
    pub expression: &'a str,
}

impl NumericExpression<'_> {
    /// Finite value when the expression is context-free arithmetic.
    pub(crate) fn constant_value(&self) -> Option<f64> {
        evaluate_constant_expression(self.expression)
    }
}

/// One validated external entity-index/object-id-table pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedSection<'a> {
    /// Self-anchored base used by every entity-index offset.
    pub base: usize,
    /// Absolute offset of the entity-index array.
    pub entity_index_offset: usize,
    /// Absolute offset of the object-id table or offset-only identity metadata.
    pub object_id_table_offset: usize,
    /// Length-framed class definitions preceding the entity index.
    pub types: Arc<[TypeDefinition<'a>]>,
    /// Length-framed member definitions preceding the entity index.
    pub fields: Arc<[FieldDefinition<'a>]>,
    /// Identity store used by this section.
    pub store: IndexedStore<'a>,
}

/// Internally pointed record-area bytes with their absolute offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordArea<'a> {
    /// Absolute offset of the record-area start.
    pub offset: usize,
    /// Exact record-area bytes, including the 12-byte control prefix.
    pub bytes: &'a [u8],
}

/// One size-framed NX object-model section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section<'a> {
    /// Offset of the `ff ff ff ff` section signature.
    pub offset: usize,
    /// Complete section length including its 16-byte header.
    pub byte_len: usize,
    /// Class declarations in the section's contiguous type registry.
    pub types: Arc<[TypeDefinition<'a>]>,
    /// Member declarations in the section's field registry.
    pub fields: Arc<[FieldDefinition<'a>]>,
    /// Internally pointed record area, when the section carries one.
    pub record_area: Option<RecordArea<'a>>,
    /// Operation labels decoded while the section's record area is framed.
    cached_operation_labels: Arc<[OperationLabel<'a>]>,
}

/// A feature operation name in a size-framed feature-history record area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationLabel<'a> {
    /// Complete operation header and its exact reference encodings.
    pub header: OperationHeader,
    /// Printable operation name without its terminating NUL.
    pub value: &'a str,
}

/// One unlabeled operation record bounded by validated operation headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnlabeledOperationRecord<'a> {
    header: OperationHeader,
    bytes: &'a [u8],
}

impl<'a> UnlabeledOperationRecord<'a> {
    fn new(header: OperationHeader, bytes: &'a [u8]) -> Option<Self> {
        bytes.get(usize::from(header.byte_len())..)?;
        header.offset().checked_add(bytes.len())?;
        Some(Self { header, bytes })
    }

    pub(crate) fn header(self) -> OperationHeader {
        self.header
    }
    pub(crate) fn bytes(self) -> &'a [u8] {
        self.bytes
    }
    pub(crate) fn payload(self) -> &'a [u8] {
        &self.bytes[usize::from(self.header.byte_len())..]
    }
}

/// Terminal common-frame suffix with its independently matched preceding frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationTerminalFrame {
    pub immediate_common_frame_offset: Option<usize>,
    pub frame: TerminalFrame<usize>,
}

/// One length-framed UTF-8 string in a bounded operation payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationPayloadString<'a> {
    /// Absolute offset of the `04` marker.
    pub offset: usize,
    /// Exact non-empty string value.
    pub value: crate::payload_text::PayloadText<&'a str>,
}

/// Marker selecting a bounded operation text frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationTextMarker {
    Text,
    String,
}

/// One length-framed UTF-8 text frame in a bounded operation payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationPayloadTextFrame<'a> {
    /// Marker selecting the payload text-frame family.
    pub marker: OperationTextMarker,
    /// Absolute offset of the marker.
    pub offset: usize,
    /// Exact non-empty text value.
    pub value: crate::payload_text::PayloadText<&'a str>,
}

/// One canonical variable-width object index in an operation payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadObjectReference<T = ReferenceIndexToken, O = usize> {
    /// Absolute offset of the width marker.
    pub offset: O,
    /// Checked token retaining the exact marker and width.
    pub token: T,
}

/// One exact counted transform lane in a pattern operation payload.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternPayloadTransformLane {
    /// Absolute offset of the opening `01, count` field.
    pub offset: usize,
    /// Schema index framing every row in the lane.
    pub row_schema_index: NonZeroU8,
    pub rows: PatternRows<LocatedCompactIndex, usize>,
}

/// Exact counted instance-output lane in a multi-instance operation payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiInstanceOutputPayloadLane {
    /// Absolute offset of the opening `25 01, count` field.
    pub offset: usize,
    /// Complete selector groups and their trailing references.
    pub outputs: instances::MultiInstanceOutputs<usize>,
}

/// Count schema index with room for the three consecutive selector-row indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdenticalInstanceSchemaIndex(u8);

impl IdenticalInstanceSchemaIndex {
    pub fn new(value: u8) -> Option<Self> {
        value.checked_add(3).map(|_| Self(value))
    }

    pub fn value(self) -> u8 {
        self.0
    }

    pub fn row_indices(self) -> [u8; 3] {
        [self.0 + 1, self.0 + 2, self.0 + 3]
    }
}

/// Exact counted selector lane in an identical-instance output payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdenticalInstanceOutputPayloadLane {
    /// Absolute offset of the leading schema index.
    pub offset: usize,
    /// Schema index preceding the count field.
    pub leading_schema_index: u8,
    /// Schema index framing the serialized count.
    pub count_schema_index: IdenticalInstanceSchemaIndex,
    /// Ordered non-null compact selectors with their exact source tokens.
    pub selectors: compact::CountedIndexMembers<LocatedCompactIndex>,
}

/// Exact construction header in a point-feature payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointFeaturePayloadHeader {
    /// Construction object referenced by the header.
    pub reference: PayloadObjectReference,
    /// Serialized header mode.
    pub mode: discriminators::PointHeaderMode,
}

/// Exact six-scalar lane selected by a point-feature construction header.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointFeatureScalarLane {
    /// Six checked shifted-binary64 atoms in byte order.
    pub values: [ShiftedBinary64; 6],
    /// Start of the contiguous lane in the joined blocks.
    offset: usize,
}

impl PointFeatureScalarLane {
    pub fn value_offsets(&self) -> [usize; 6] {
        std::array::from_fn(|i| self.offset + i * 8)
    }
}

/// Exact leading construction branch in a `SWP104` payload.
#[derive(Debug, Clone, PartialEq)]
pub struct Swp104PayloadLeadingBranch {
    /// Nonzero construction discriminator at the payload start.
    pub discriminator: NonZeroU8,
    /// Four finite shifted-binary64 values in serialized order.
    pub scalars: [ShiftedBinary64; 4],
    /// Whether one zero byte precedes the branch mode.
    pub leading_zero: bool,
    /// Serialized nonzero branch mode.
    pub mode: NonZeroU8,
    /// Exact state lane preceding the terminal marker.
    pub state_lane: Swp104StateLane,
    /// Ordered nonterminal references.
    pub members: BranchItems<reference_index::PayloadIndexToken>,
    /// Terminal reference.
    pub terminal: reference_index::PayloadIndexToken,
}

impl Swp104PayloadLeadingBranch {
    pub(crate) fn byte_len(&self) -> usize {
        40 + usize::from(self.leading_zero)
            + self
                .members
                .as_slice()
                .iter()
                .map(|token| token.raw().len())
                .sum::<usize>()
            + self.state_lane.byte_len()
            + 3
            + self.terminal.raw().len()
            + 1
    }
}

/// Exact pair of scaled shifted-binary64 atoms in a reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchPayloadFixedPair {
    /// Payload-relative offset of the discriminator.
    pub offset: usize,
    /// Ordered values reconstructed from the `30` shifted-binary64 atoms and scaled by `1/4`.
    pub values: [SketchScaledAtom; 2],
    /// Exact discriminator and branch prefix selecting the pair layout.
    pub form: SketchPairForm,
}

impl SketchPayloadFixedPair {
    pub fn discriminator(&self) -> &'static [u8] {
        self.form.discriminator()
    }
    pub fn value_offsets(&self) -> [usize; 2] {
        let first = self.offset + self.discriminator().len();
        [first, first + 8 + self.form.separator_width()]
    }
}

/// Exact mixed scaled shifted-binary64 and shifted-binary32 pair in a sketch payload.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchPayloadMixedPair {
    /// Payload-relative offset of the discriminator.
    pub offset: usize,
    /// Exact scaled binary64 and binary32 atoms.
    pub scalars: SketchMixedScalars,
}

impl SketchPayloadMixedPair {
    pub fn value_offsets(&self) -> [usize; 2] {
        let first = self.offset + SketchPairForm::Legacy.discriminator().len();
        [first, first + 8 + 1]
    }
}

/// Exact pair of signed Q1.55 atoms following a datum-CSYS branch discriminator.
#[derive(Debug, Clone, PartialEq)]
pub struct DatumCsysPayloadFixedPair {
    /// Payload-relative offset of the discriminator.
    pub offset: usize,
    /// Ordered dimensionless Q1.55 values.
    pub values: [Q155; 2],
    /// Exact discriminator selecting the pair branch.
    pub form: DatumPairForm,
}

impl DatumCsysPayloadFixedPair {
    pub fn discriminator(&self) -> &'static [u8] {
        self.form.discriminator()
    }
    pub fn value_offsets(&self) -> [usize; 2] {
        let first = self.offset + self.discriminator().len();
        [first, first + 8 + 1]
    }
}

/// Fixed scalar header in one bounded extrusion payload.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExtrudePayloadHeader {
    /// Absolute offset of the first shifted-IEEE scalar.
    pub offset: usize,
    /// Ordered finite scalar values.
    pub scalars: [ShiftedBinary64; 2],
}

/// Four construction-block references carried by a `HOLE PACKAGE` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HolePackageConstructionGroupLane {
    /// Payload-relative offset of the fixed lane prefix.
    pub offset: usize,
    /// Compact selector preceding the repeated branch byte.
    pub selector: NonZeroU8,
    /// Branch byte repeated between the two reference pairs.
    pub branch: NonZeroU8,
    /// Ordered first and second construction-block pairs.
    pub references: [PayloadObjectReference; 4],
}

/// One wrapped member index in a branch-`11` operation body clause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationBodyMemberGroup {
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Ordered compact indices and their absolute source positions.
    pub members: Vec<LocatedCompactIndex>,
}

/// Exact continuation following a `TRIM BODY` branch-`11` member lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationBody11Continuation {
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Exact compact continuation index and its absolute source offset.
    pub continuation: LocatedCompactIndex,
    /// Exact required terminal reference and its absolute source offset.
    pub terminal: PayloadObjectReference,
}

/// Homogeneous checked references in an operation body lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationBodyReferenceLaneValues {
    CompactIndex(Vec<LocatedCompactIndex>),
    PayloadObjectIndex(Vec<PayloadObjectReference<reference_index::PayloadIndexToken>>),
}

/// Counted reference lane following an operation body scalar clause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationBodyReferenceLane {
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Branch discriminator following the body-reference terminator.
    pub branch: discriminators::OperationBodyReferenceBranch,
    /// Ordered non-null lane values with their encoding.
    pub values: OperationBodyReferenceLaneValues,
}

/// Self-framed NX parameter name in one bounded expression declaration record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpressionDeclarationName<'a> {
    /// Byte offset of the `04` marker within the containing byte range.
    pub offset: usize,
    /// Exact `p<decimal>[_qualifier]` name.
    pub name: ParameterName<&'a str, u32>,
    /// Independently framed numeric literal in the declaration record.
    pub literal: Option<&'a str>,
}

/// Primary body-object reference carried by one bounded operation record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationBodyReference {
    /// Absolute offset of the object-index token.
    pub offset: usize,
    /// Referenced body object index.
    pub object_index: reference_index::FeatureReferenceToken,
}

/// Object-index reference in one bounded offset-only OM data block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataBlockObjectReference {
    /// Byte offset of the object-index token within the containing byte range.
    pub offset: usize,
    /// Required feature index with its exact encoding.
    pub object_index: reference_index::FeatureReferenceToken,
}

/// Boolean operation kind stored after an operation label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOperationKind {
    /// Add tool bodies to the target.
    Unite,
    /// Remove tool bodies from the target.
    Subtract,
    /// Retain target/tool intersections.
    Intersect,
}

/// One feature-history Boolean with object-index operands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BooleanOperation {
    /// Absolute offset of the operation label tag.
    pub offset: usize,
    /// Boolean operation kind.
    pub kind: BooleanOperationKind,
    /// Target body reference and its exact source token.
    pub target: PayloadObjectReference,
    /// Ordered tool body references and their exact source tokens.
    pub tools: Vec<PayloadObjectReference>,
}

impl<'a> IndexedSection<'a> {
    /// Return the section base used by its external record offsets.
    pub const fn base_offset(&self) -> usize {
        self.base
    }

    /// Fixed-width object-id records, when this section is that store.
    pub fn as_fixed(&self) -> Option<&[FixedEntityRecord<'a>]> {
        match &self.store {
            IndexedStore::Fixed { records } => Some(records.as_ref()),
            IndexedStore::OffsetOnly { .. } => None,
        }
    }

    /// Control block, column storage, and records of an offset-only store.
    pub fn as_offset_only(&self) -> Option<(&EntityRecord<'a>, &'a [u8], &[EntityRecord<'a>])> {
        match &self.store {
            IndexedStore::OffsetOnly {
                control,
                column_storage,
                records,
            } => Some((control, column_storage, records.as_ref())),
            IndexedStore::Fixed { .. } => None,
        }
    }

    fn record_views(&self) -> Vec<(usize, &'a [u8], Option<u32>)> {
        match &self.store {
            IndexedStore::Fixed { records } => records
                .iter()
                .map(|record| (record.offset, record.bytes, Some(record.object_id.0)))
                .collect(),
            IndexedStore::OffsetOnly { records, .. } => records
                .iter()
                .map(|record| (record.offset, record.bytes, None))
                .collect(),
        }
    }

    /// Decode explicit numeric-expression text within bounded entity records.
    pub fn numeric_expressions(&self) -> Vec<NumericExpression<'a>> {
        self.numeric_expression_records()
            .into_iter()
            .map(|(_, expression)| expression)
            .collect()
    }

    /// Decode expressions together with their owning record ordinal.
    pub fn numeric_expression_records(&self) -> Vec<(usize, NumericExpression<'a>)> {
        let records = self.record_views();
        if !records.iter().any(|(_, bytes, _)| {
            bytes
                .windows(b"hostglobalvariables".len())
                .any(|window| window == b"hostglobalvariables")
        }) {
            return Vec::new();
        }
        records
            .into_iter()
            .enumerate()
            .filter_map(|(record_ordinal, (offset, bytes, object_id))| {
                numeric_expression_at(bytes, offset, object_id)
                    .map(|expression| (record_ordinal, expression))
            })
            .collect()
    }

    /// Decode every strictly framed printable string in each bounded record.
    pub fn string_values(&self) -> Vec<(usize, usize, Option<u32>, StringValue<'a>)> {
        self.record_views()
            .into_iter()
            .enumerate()
            .flat_map(|(record_ordinal, (offset, bytes, object_id))| {
                string_values(bytes, offset).into_iter().enumerate().map(
                    move |(value_ordinal, value)| (record_ordinal, value_ordinal, object_id, value),
                )
            })
            .collect()
    }

    /// Decode tagged cross-record references from every bounded record.
    // The tuple carries one coupled result; a separate alias would add no invariant.
    #[allow(clippy::type_complexity)]
    pub fn references(
        &self,
    ) -> Vec<(
        usize,
        usize,
        Option<u32>,
        LocatedReference<RecordReference<()>>,
    )> {
        let records = self.record_views();
        let record_count = records.len();
        records
            .into_iter()
            .enumerate()
            .flat_map(|(record_ordinal, (offset, bytes, object_id))| {
                let mut references = record_references(bytes, offset)
                    .into_iter()
                    .map(|reference| LocatedReference {
                        offset: reference.offset,
                        value: RecordReference::Direct(reference.value),
                    })
                    .collect::<Vec<_>>();
                references.extend(
                    counted_record_references(bytes, offset, record_count)
                        .into_iter()
                        .map(|reference| LocatedReference {
                            offset: reference.offset,
                            value: RecordReference::RecordOrdinal16 {
                                ordinal: reference.value,
                                target: (),
                            },
                        }),
                );
                references.sort_by_key(|reference| reference.offset);
                references
                    .into_iter()
                    .enumerate()
                    .map(move |(reference_ordinal, reference)| {
                        (record_ordinal, reference_ordinal, object_id, reference)
                    })
            })
            .collect()
    }
}

impl<'a> Section<'a> {
    fn record_area_parts(&self) -> Option<(usize, &'a [u8])> {
        self.record_area.map(|area| (area.offset, area.bytes))
    }

    /// Decode the validated record-area control and product header.
    pub fn record_area_header(&self) -> Option<RecordAreaHeader<'a>> {
        let (offset, bytes) = self.record_area_parts()?;
        let control_words = [
            View::u32_le_at(bytes, 0)?,
            View::u32_le_at(bytes, 4)?,
            View::u32_le_at(bytes, 8)?,
        ];
        let suffix = bytes.get(12..)?;
        let layout = ProductRecord::read(suffix, ProductRecordForm::Modern)
            .or_else(|| ProductRecord::read(suffix, ProductRecordForm::LegacyFeature))?;
        Some(RecordAreaHeader {
            offset,
            control_words,
            product: StoreVersion {
                offset: offset + 12,
                value: layout.text(),
            },
        })
    }

    /// Decode strictly framed operation labels for parser/cache tests.
    #[cfg(test)]
    pub fn operation_labels(&self) -> Vec<OperationLabel<'a>> {
        self.cached_operation_labels.to_vec()
    }

    /// Decode fully framed Boolean operations from the pointed record area.
    pub fn boolean_operations(&self) -> Vec<BooleanOperation> {
        let Some((base_offset, bytes)) = self.record_area_parts() else {
            return Vec::new();
        };
        boolean_operations_with_labels(bytes, base_offset, &self.cached_operation_labels)
    }

    /// Bound operation records and retain their ordinal in the complete label sequence.
    pub fn operation_records_with_label_ordinals(&self) -> Vec<(usize, OperationRecord<'a>)> {
        let Some((base_offset, bytes)) = self.record_area_parts() else {
            return Vec::new();
        };
        operation_records_with_labels_and_ordinals(
            bytes,
            base_offset,
            &self.cached_operation_labels,
        )
    }

    /// Bound validated operation headers that have no complete label frame.
    pub fn unlabeled_operation_records_with_ordinals(
        &self,
    ) -> Vec<(usize, UnlabeledOperationRecord<'a>)> {
        let Some((base_offset, bytes)) = self.record_area_parts() else {
            return Vec::new();
        };
        unlabeled_operation_records_with_ordinals(bytes, base_offset, &self.cached_operation_labels)
    }

    /// Decode the bounded state-counter map of a feature-history record area.
    ///
    /// The section-role check is intentional. The same byte patterns occur in
    /// ordinary model-store payloads, where they do not carry operation state.
    pub fn operation_state_counter_map(&self) -> Option<StateCounterMap> {
        let is_feature_history = self
            .types
            .iter()
            .any(|definition| definition.name == "UGS::FEATURE_RECORD")
            || self
                .fields
                .iter()
                .any(|definition| definition.name == "m_rollForwardStates");
        if !is_feature_history {
            return None;
        }
        let (base_offset, bytes) = self.record_area_parts()?;
        StateCounterMap::read(bytes, base_offset)
    }

    /// Decode the field-declared `m_rollForwardStates` group table before the
    /// bounded operation-state counter map.
    pub fn operation_state_group_table(&self) -> Option<OperationStateGroupTable> {
        if !self
            .fields
            .iter()
            .any(|definition| definition.name == "m_rollForwardStates")
        {
            return None;
        }
        let map = self.operation_state_counter_map()?;
        let (base_offset, bytes) = self.record_area_parts()?;
        let map_start = map.offset().checked_sub(base_offset)?;
        operation_state_group_table_before_counter_map(bytes, map_start, base_offset)
    }

    /// Decode anchored state-journal groups from a feature-history record area.
    ///
    /// The journal prefix is selected by the record-area marker and its
    /// version-token run. After a complete group, only the exact `04 00`
    /// separator form may be skipped when it leads to another complete group.
    /// Any other byte stops the journal so later record-region data cannot
    /// become state.
    pub fn operation_state_journal_groups(&self) -> Option<Vec<JournalGroup<usize>>> {
        let is_feature_history = self
            .types
            .iter()
            .any(|definition| definition.name == "UGS::FEATURE_RECORD");
        if !is_feature_history {
            return None;
        }
        let (base_offset, bytes) = self.record_area_parts()?;
        let header = self.record_area_header()?;
        let product = header.product.offset.checked_sub(base_offset)?;
        let product_end = record_area_product_end(bytes, product)?;
        let start = operation_state_journal_start(bytes, product_end)?;
        let end = self
            .cached_operation_labels
            .first()
            .and_then(|label| label.header.offset().checked_sub(base_offset))
            .unwrap_or(bytes.len());
        operation_state_journal_groups_before_boundary(bytes, start, end, base_offset)
    }

    fn operation_state_block(&self) -> Option<OperationStateBlock<'a>> {
        let map = self.operation_state_counter_map()?;
        let (base_offset, bytes) = self.record_area_parts()?;
        let (_, last_record) = self
            .operation_records_with_label_ordinals()
            .into_iter()
            .last()?;
        let start_offset = last_record.payload_offset();
        let start = start_offset.checked_sub(base_offset)?;
        let group = self.operation_state_group_table();
        let terminal = group
            .as_ref()
            .map_or(map.offset(), OperationStateGroupTable::offset)
            .checked_sub(base_offset)?;
        let mut ends = Vec::with_capacity(2);
        if let Some(table) = &group {
            let overlap_end =
                terminal.checked_add(table.groups().first().opener().bytes().len())?;
            ends.push(overlap_end);
        }
        ends.push(terminal);
        ends.into_iter()
            .find_map(|end| operation_state_block_before_boundary(bytes, start, end, base_offset))
    }

    /// Decode the bounded per-object status lane after the operation records.
    pub fn operation_state_status_table(&self) -> Option<OperationStateStatusTable<'a>> {
        self.operation_state_block()?.into_status_table()
    }

    /// Decode the contiguous standalone message records immediately before
    /// the roll-forward table or counter-map boundary.
    pub fn operation_state_messages(&self) -> Option<Vec<OperationStateMessage<'a>>> {
        self.operation_state_block()?.into_messages()
    }

    /// Decode complete rows in an audit-trail record area.
    ///
    /// The registry role check prevents the same compact byte patterns in
    /// feature-history and model areas from being interpreted as audit data.
    /// Unknown bytes before, between, and after complete rows remain outside
    /// this typed view.
    pub fn audit_trail_rows(&self) -> Option<Vec<AuditTrailRow>> {
        let has_audit_marker = self
            .types
            .iter()
            .any(|definition| definition.name == "UGS::OM::SaveAuditTrail");
        let has_specialized_marker = self.types.iter().any(|definition| {
            matches!(
                definition.name,
                "UGS::FEATURE_RECORD" | "UGS::EXP_expression" | "UGS::Solid::Topol"
            )
        });
        if !has_audit_marker || has_specialized_marker {
            return None;
        }
        let (base_offset, bytes) = self.record_area_parts()?;
        let header = self.record_area_header()?;
        let product = header.product.offset.checked_sub(base_offset)?;
        let product_end = record_area_product_end(bytes, product)?;
        let start = bytes
            .get(product_end..)?
            .windows(2)
            .position(|window| window == [0x41, 0x00])?
            .checked_add(product_end + 2)?;
        audit_trail_rows(bytes, start, bytes.len(), base_offset)
    }

    /// Decode unambiguous primary body references from bounded operation records.
    pub fn operation_body_references(&self) -> Vec<(usize, OperationBodyReference)> {
        self.operation_records_with_label_ordinals()
            .into_iter()
            .filter_map(|(ordinal, record)| {
                operation_body_reference(record.body_view()).map(|reference| (ordinal, reference))
            })
            .collect()
    }
}

/// Decode complete feature-operation headers and their label frames.
pub fn operation_labels(bytes: &[u8], base_offset: usize) -> Vec<OperationLabel<'_>> {
    validated_operation_headers(bytes, base_offset)
        .into_iter()
        .filter_map(|header| operation_label_at(bytes, base_offset, header))
        .collect()
}

fn validated_operation_headers(bytes: &[u8], base_offset: usize) -> Vec<OperationHeader> {
    const PREFIX: &[u8] = &[0x80, 0xcd, 0x01, 0x04, 0x01];
    const SCALAR_LEN: usize = 8;
    let mut headers = Vec::new();
    for marker in bytes
        .windows(PREFIX.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == PREFIX).then_some(offset))
    {
        let scalar_at = marker + PREFIX.len();
        let Some(raw_scalar) = bytes.get(scalar_at..scalar_at + SCALAR_LEN) else {
            continue;
        };
        if shifted_ieee_f64(raw_scalar).is_none()
            || bytes.get(scalar_at + SCALAR_LEN..scalar_at + SCALAR_LEN + 2) != Some(&[0xff, 0xff])
        {
            continue;
        }
        let Some(objects) = HeaderReferences::read(&bytes[scalar_at + SCALAR_LEN + 2..]) else {
            continue;
        };
        let Some(header) = base_offset
            .checked_add(marker)
            .and_then(|offset| OperationHeader::<usize>::new(offset, objects))
        else {
            continue;
        };
        headers.push(header);
    }
    headers
}

fn operation_label_at(
    bytes: &[u8],
    base_offset: usize,
    header: OperationHeader,
) -> Option<OperationLabel<'_>> {
    let at = header.end_offset().checked_sub(base_offset)?;
    if bytes.get(at) != Some(&0x03) {
        return None;
    }
    let length = bytes.get(at + 1).copied().map(usize::from)?;
    if length < 3 {
        return None;
    }
    let end = at.checked_add(length)?;
    if bytes.get(end) != Some(&0) {
        return None;
    }
    let name = bytes.get(at + 2..end)?;
    if !name
        .iter()
        .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    {
        return None;
    }
    let Ok(value) = std::str::from_utf8(name) else {
        return None;
    };
    Some(OperationLabel { header, value })
}

fn operation_records_with_labels_and_ordinals<'a>(
    bytes: &'a [u8],
    base_offset: usize,
    labels: &[OperationLabel<'a>],
) -> Vec<(usize, OperationRecord<'a>)> {
    let headers = validated_operation_headers(bytes, base_offset);
    headers
        .iter()
        .enumerate()
        .filter_map(|(ordinal, header)| {
            let label = labels
                .iter()
                .find(|label| label.header.offset() == header.offset())?;
            let start = label.header.offset().checked_sub(base_offset)?;
            let end = headers
                .get(ordinal + 1)
                .map_or(bytes.len(), |next| next.offset() - base_offset);
            Some((
                ordinal,
                OperationRecord::new(bytes.get(start..end)?, *label)?,
            ))
        })
        .collect()
}

fn unlabeled_operation_records_with_ordinals<'a>(
    bytes: &'a [u8],
    base_offset: usize,
    labels: &[OperationLabel<'a>],
) -> Vec<(usize, UnlabeledOperationRecord<'a>)> {
    let headers = validated_operation_headers(bytes, base_offset);
    headers
        .iter()
        .enumerate()
        .filter_map(|(ordinal, header)| {
            if labels
                .iter()
                .any(|label| label.header.offset() == header.offset())
            {
                return None;
            }
            let start = header.offset().checked_sub(base_offset)?;
            let end = headers
                .get(ordinal + 1)
                .map_or(bytes.len(), |next| next.offset() - base_offset);
            Some((
                ordinal,
                UnlabeledOperationRecord::new(*header, bytes.get(start..end)?)?,
            ))
        })
        .collect()
}

/// Decode ordered `03|04, length, text, 00` frames from one operation payload.
pub fn operation_payload_text_frames(
    record: OperationPayload<'_>,
) -> Vec<OperationPayloadTextFrame<'_>> {
    let mut frames = Vec::new();
    let mut at = 0usize;
    while at + 4 <= record.payload().len() {
        let marker = match record.payload()[at] {
            0x03 => OperationTextMarker::Text,
            0x04 => OperationTextMarker::String,
            _ => {
                at += 1;
                continue;
            }
        };
        let declared = usize::from(record.payload()[at + 1]);
        let Some(end) = at.checked_add(declared) else {
            at += 1;
            continue;
        };
        let Some(raw) = record.payload().get(at + 2..end) else {
            at += 1;
            continue;
        };
        let Some(value) = std::str::from_utf8(raw)
            .ok()
            .and_then(|value| crate::payload_text::PayloadText::new(value).ok())
        else {
            at += 1;
            continue;
        };
        if declared < 3 || record.payload().get(end) != Some(&0) {
            at += 1;
            continue;
        }
        frames.push(OperationPayloadTextFrame {
            marker,
            offset: record.payload_offset() + at,
            value,
        });
        at = end + 1;
    }
    frames
}

/// Decode ordered `04, length, text, 00` strings from one operation payload.
pub fn operation_payload_strings(record: OperationPayload<'_>) -> Vec<OperationPayloadString<'_>> {
    operation_payload_text_frames(record)
        .into_iter()
        .filter(|frame| frame.marker == OperationTextMarker::String)
        .map(|frame| OperationPayloadString {
            offset: frame.offset,
            value: frame.value,
        })
        .collect()
}

/// Decode an exact nonempty duplicated shifted-binary64 lane before a hole template.
pub fn simple_hole_repeated_scalar_lane(
    record: OperationPayload<'_>,
) -> Option<NonEmpty<RepeatedScalar<usize>>> {
    if record.name() != "SIMPLE HOLE" {
        return None;
    }
    let templates = operation_payload_strings(record)
        .into_iter()
        .filter(|value| value.value.as_str().starts_with("Hole_"))
        .collect::<Vec<_>>();
    let [template] = templates.as_slice() else {
        return None;
    };
    let boundary = template.offset.checked_sub(record.payload_offset())?;
    let prefix = record.payload().get(..boundary)?;
    let mut scalars = Vec::new();
    let mut at = 0usize;
    while at + 8 <= prefix.len() {
        if prefix[at] == 0x30 {
            if let Some(scalar) = ShiftedBinary64::read(&prefix[at..at + 8]) {
                scalars.push((scalar, record.payload_offset() + at));
                at += 8;
                continue;
            }
        }
        at += 1;
    }
    let half = scalars.len() / 2;
    if scalars.len() != half * 2 {
        return None;
    }
    let (first, second) = scalars.split_at(half);
    if first
        .iter()
        .zip(second)
        .any(|(left, right)| left.0 != right.0)
    {
        return None;
    }
    NonEmpty::new(
        first
            .iter()
            .zip(second)
            .map(|(left, right)| RepeatedScalar {
                scalar: left.0,
                witness_offsets: [left.1, right.1],
            }),
    )
}

/// Decode the unique four-block construction-group lane in a `HOLE PACKAGE` payload.
pub fn hole_package_construction_group_lane(
    record: OperationPayload<'_>,
) -> Option<HolePackageConstructionGroupLane> {
    const PREFIX: [u8; 5] = [0x00, 0x00, 0x01, 0x00, 0x00];
    const ZEROES: [u8; 4] = [0; 4];
    const SUFFIX: [u8; 3] = [0x00, 0x00, 0xff];
    if record.name() != "HOLE PACKAGE" {
        return None;
    }
    let mut candidate = None;
    for start in 0..record.payload().len().saturating_sub(PREFIX.len()) {
        if record.payload().get(start..start + PREFIX.len()) != Some(&PREFIX) {
            continue;
        }
        let Some(lane) = (|| {
            let selector = NonZeroU8::new(*record.payload().get(start + 5)?)?;
            let branch = NonZeroU8::new(*record.payload().get(start + 7)?)?;
            if record.payload().get(start + 6) != Some(&0)
                || record.payload().get(start + 8..start + 12) != Some(&ZEROES)
            {
                return None;
            }
            let mut at = start + 12;
            let references = std::array::from_fn::<_, 4, _>(|ordinal| {
                if ordinal == 2 {
                    if record.payload().get(at) != Some(&branch.get())
                        || record.payload().get(at + 1..at + 5) != Some(&ZEROES)
                    {
                        return None;
                    }
                    at += 5;
                }
                let reference_offset = at;
                let (object_index, width) = payload_object_index(record.payload().get(at..)?)?;
                at += width;
                Some(PayloadObjectReference {
                    offset: record.payload_offset() + reference_offset,
                    token: object_index,
                })
            });
            let [a, b, c, d] = references;
            let references = [a?, b?, c?, d?];
            if record.payload().get(at..at + SUFFIX.len()) != Some(&SUFFIX) {
                return None;
            }
            Some(HolePackageConstructionGroupLane {
                offset: start,
                selector,
                branch,
                references,
            })
        })() else {
            continue;
        };
        if candidate.is_some() {
            return None;
        }
        candidate = Some(lane);
    }
    candidate
}

/// Decode the unique counted reference field in a bounded `SKETCH` payload.
pub fn sketch_payload_references(record: OperationPayload<'_>) -> Option<SketchReferenceField> {
    if record.name() != "SKETCH" {
        return None;
    }
    unique_candidate(
        (0..record.payload().len().saturating_sub(3)).filter_map(|start| {
            if record.payload().get(start..start + 2) != Some(&[0x01, 0x00]) {
                return None;
            }
            SketchReferenceField::read(record, start)
        }),
    )
}

fn payload_object_index(bytes: &[u8]) -> Option<(ReferenceIndexToken, usize)> {
    let token = ReferenceIndexToken::read_payload(bytes)?;
    Some((token, token.raw().len()))
}

/// Decode the unique exactly counted transform lane in a bounded pattern payload.
pub fn pattern_payload_transform_lane(
    record: OperationPayload<'_>,
) -> Option<PatternPayloadTransformLane> {
    const FEATURE_PREFIX_TAIL: [u8; 3] = [0x01, 0x00, 0x00];
    const FEATURE_SCALAR_SUFFIX: [u8; 14] = [
        0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x03,
    ];
    const GEOMETRY_PREFIX_TAIL: [u8; 7] = [0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00];
    const GEOMETRY_SCALAR_SUFFIX: [u8; 10] =
        [0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x03];
    const ROW_TAIL: [u8; 5] = [0x00, 0x00, 0xff, 0x00, 0x00];
    let (prefix_tail, scalar_suffix) = match record.name() {
        "Pattern Feature" => (
            FEATURE_PREFIX_TAIL.as_slice(),
            FEATURE_SCALAR_SUFFIX.as_slice(),
        ),
        "Pattern Geometry" => (
            GEOMETRY_PREFIX_TAIL.as_slice(),
            GEOMETRY_SCALAR_SUFFIX.as_slice(),
        ),
        _ => return None,
    };
    let decode = |start: usize| {
        (record.payload().get(start) == Some(&0x01)).then_some(())?;
        let declared_count @ 2.. = *record.payload().get(start + 1)? else {
            return None;
        };
        let row_schema_index = NonZeroU8::new(*record.payload().get(start + 2)?)?;
        let mut at = start + 2;
        let mut rows = Vec::new();
        for ordinal in 1..declared_count {
            (record.payload().get(at) == Some(&row_schema_index.get())).then_some(())?;
            (record.payload().get(at + 1..at + 1 + prefix_tail.len()) == Some(prefix_tail))
                .then_some(())?;
            at += 1 + prefix_tail.len();
            let scalar = ShiftedScalar::read(record.payload().get(at..)?)?;
            let width = scalar.raw().len();
            let value = PatternValue {
                scalar,
                offset: record.payload_offset() + at,
            };
            at += width;
            (record.payload().get(at..at + scalar_suffix.len()) == Some(scalar_suffix))
                .then_some(())?;
            at += scalar_suffix.len();
            let selector_offset = at;
            let atom = CompactIndexAtom::read(record.payload().get(at..)?)?;
            let width = atom.raw().len();
            let selector = LocatedCompactIndex {
                atom,
                offset: record.payload_offset() + selector_offset,
            };
            rows.push(PatternRow {
                values: value,
                selector,
            });
            at += width;
            (record.payload().get(at) == Some(&0x01)).then_some(())?;
            (record.payload().get(at + 1) == Some(&ordinal)).then_some(())?;
            (record.payload().get(at + 2..at + 2 + ROW_TAIL.len()) == Some(&ROW_TAIL))
                .then_some(())?;
            at += 2 + ROW_TAIL.len();
        }
        let terminal_schema_index = row_schema_index.get() - 1;
        (record.payload().get(at) == Some(&terminal_schema_index)).then_some(())?;
        (record.payload().get(at + 1..at + 3) == Some(&[0x00, 0x00])).then_some(())?;
        (record.payload().get(at + 3) == Some(&0x01)).then_some(())?;
        Some(PatternPayloadTransformLane {
            offset: record.payload_offset() + start,
            row_schema_index,
            rows: PatternRows::Scalar(BranchItems::new(rows).ok()?),
        })
    };
    let decode_wide = |start: usize| {
        (record.name() == "Pattern Feature").then_some(())?;
        (record.payload().get(start) == Some(&0x01)).then_some(())?;
        let declared_count @ 2.. = *record.payload().get(start + 1)? else {
            return None;
        };
        let row_schema_index = NonZeroU8::new(*record.payload().get(start + 2)?)?;
        let mut at = start + 2;
        let mut rows = Vec::new();
        for ordinal in 1..declared_count {
            (record.payload().get(at) == Some(&row_schema_index.get())).then_some(())?;
            at += 1;
            let mut decode_value = |value_ordinal| {
                let value_offset = at;
                let scalar = ShiftedBinary64::read(record.payload().get(at..at + 8)?)?;
                let value = PatternValue {
                    scalar,
                    offset: record.payload_offset() + value_offset,
                };
                at += 8;
                if value_ordinal == 1 {
                    (record.payload().get(at..at + 2) == Some(&[0x00, 0x00])).then_some(())?;
                    at += 2;
                }
                Some(value)
            };
            let first = [
                decode_value(0)?,
                decode_value(1)?,
                decode_value(2)?,
                decode_value(3)?,
            ];
            (record.payload().get(at..at + 4) == Some(&[0x00; 4])).then_some(())?;
            at += 4;
            let terminal_value_offset = at;
            let scalar = PatternTerminal::read(record.payload().get(at..)?)?;
            let width = scalar.raw().len();
            let terminal = PatternValue {
                scalar,
                offset: record.payload_offset() + terminal_value_offset,
            };
            at += width;
            (record.payload().get(at..at + 7) == Some(&[0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x03]))
                .then_some(())?;
            at += 7;
            let selector_offset = at;
            let atom = CompactIndexAtom::read(record.payload().get(at..)?)?;
            let width = atom.raw().len();
            let selector = LocatedCompactIndex {
                atom,
                offset: record.payload_offset() + selector_offset,
            };
            rows.push(PatternRow {
                values: PatternWideValues { first, terminal },
                selector,
            });
            at += width;
            (record.payload().get(at) == Some(&0x01)).then_some(())?;
            (record.payload().get(at + 1) == Some(&ordinal)).then_some(())?;
            (record.payload().get(at + 2..at + 2 + ROW_TAIL.len()) == Some(&ROW_TAIL))
                .then_some(())?;
            at += 2 + ROW_TAIL.len();
        }
        let terminal_schema_index = row_schema_index.get() - 1;
        (record.payload().get(at) == Some(&terminal_schema_index)).then_some(())?;
        (record.payload().get(at + 1..at + 4) == Some(&[0x00, 0x00, 0x02])).then_some(())?;
        Some(PatternPayloadTransformLane {
            offset: record.payload_offset() + start,
            row_schema_index,
            rows: PatternRows::Wide(BranchItems::new(rows).ok()?),
        })
    };
    unique_candidate(
        (0..record.payload().len().saturating_sub(1))
            .filter_map(decode)
            .chain((0..record.payload().len().saturating_sub(1)).filter_map(decode_wide)),
    )
}

/// Decode the unique exactly counted instance-output lane in a bounded payload.
pub fn multi_instance_output_payload_lane(
    record: OperationPayload<'_>,
) -> Option<MultiInstanceOutputPayloadLane> {
    const ENVELOPE: [u8; 10] = [0x3a, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x25, 0x01];
    const ROW_PREFIX: [u8; 7] = [0x26, 0x27, 0x01, 0x02, 0x65, 0x01, 0x02];
    const ROW_ORDINAL_MARKER: u8 = 0x28;
    const REFERENCE_PREFIX: [u8; 2] = [0x00, 0x3b];

    if record.name() != "Multi Instance Output" {
        return None;
    }
    let decode = |start: usize| {
        (record.payload().get(start..start + ENVELOPE.len()) == Some(&ENVELOPE)).then_some(())?;
        let declared_count = *record.payload().get(start + ENVELOPE.len())?;
        (declared_count >= 2).then_some(())?;
        let mut instance_count = 0;
        let row_count = usize::from(declared_count - 1);
        let mut at = start + ENVELOPE.len() + 1;
        let mut rows = Vec::with_capacity(row_count);
        for expected_row_index in 2..=declared_count {
            (record.payload().get(at..at + ROW_PREFIX.len()) == Some(&ROW_PREFIX)).then_some(())?;
            at += ROW_PREFIX.len();
            let selector_offset = at;
            let atom = CompactIndexAtom::read(record.payload().get(at..)?)?;
            let width = atom.raw().len();
            let selector = LocatedCompactIndex {
                atom,
                offset: record.payload_offset() + selector_offset,
            };
            at += width;
            (record.payload().get(at) == Some(&ROW_ORDINAL_MARKER)).then_some(())?;
            let ordinal = *record.payload().get(at + 1)?;
            instance_count = instance_count.max(ordinal);
            (record.payload().get(at + 2) == Some(&expected_row_index)).then_some(())?;
            rows.push((selector, ordinal));
            at += 3;
        }
        (record.payload().get(at..at + REFERENCE_PREFIX.len()) == Some(&REFERENCE_PREFIX))
            .then_some(())?;
        at += REFERENCE_PREFIX.len();
        let mut trailing_references =
            Vec::with_capacity(usize::from(instance_count.saturating_sub(1)));
        for _ in 1..instance_count {
            let reference_offset = at;
            let object_index =
                reference_index::FeatureReferenceToken::read(record.payload().get(at..)?)?;
            let end = at + object_index.raw().len();
            trailing_references.push(PayloadObjectReference {
                offset: record.payload_offset() + reference_offset,
                token: object_index,
            });
            at = end;
        }
        (record.payload().get(at..at + 2) == Some(&[0x01, instance_count])).then_some(())?;
        Some(MultiInstanceOutputPayloadLane {
            offset: record.payload_offset() + start + 8,
            outputs: instances::MultiInstanceOutputs::new(rows, trailing_references).ok()?,
        })
    };
    unique_candidate((0..=record.payload().len().saturating_sub(ENVELOPE.len())).filter_map(decode))
}

/// Decode the unique exactly counted selector lane in an
/// `IDENTICAL INSTANCE OUTPUT` payload.
pub fn identical_instance_output_payload_lane(
    record: OperationPayload<'_>,
) -> Option<IdenticalInstanceOutputPayloadLane> {
    const ROW_MIDDLE: [u8; 2] = [0x01, 0x02];
    const SENTINEL: [u8; 7] = [0xe0, 0x7f, 0xff, 0xff, 0xff, 0x00, 0x00];

    if record.name() != "IDENTICAL INSTANCE OUTPUT" {
        return None;
    }
    let decode = |start: usize| {
        let leading_schema_index = *record.payload().get(start)?;
        let count_schema_index =
            IdenticalInstanceSchemaIndex::new(*record.payload().get(start + 1)?)?;
        (record.payload().get(start + 2) == Some(&0x01)).then_some(())?;
        let declared_count = *record.payload().get(start + 3)?;
        (declared_count >= 2).then_some(())?;
        let [first_schema_index, second_schema_index, third_schema_index] =
            count_schema_index.row_indices();
        let mut at = start + 4;
        let mut selectors = Vec::with_capacity(usize::from(declared_count - 1));
        for ordinal in 2..=declared_count {
            (record.payload().get(at) == Some(&first_schema_index)).then_some(())?;
            (record.payload().get(at + 1) == Some(&second_schema_index)).then_some(())?;
            (record.payload().get(at + 2..at + 4) == Some(&ROW_MIDDLE)).then_some(())?;
            (record.payload().get(at + 4) == Some(&third_schema_index)).then_some(())?;
            at += 5;
            let selector_offset = at;
            let atom = CompactIndexAtom::read(record.payload().get(at..)?)?;
            let width = atom.raw().len();
            selectors.push(LocatedCompactIndex {
                atom,
                offset: record.payload_offset() + selector_offset,
            });
            at += width;
            (record.payload().get(at) == Some(&0x00)).then_some(())?;
            (record.payload().get(at + 1) == Some(&ordinal)).then_some(())?;
            at += 2;
        }
        let terminal_count = declared_count.checked_add(1)?;
        (record.payload().get(at) == Some(&0x00)).then_some(())?;
        (record.payload().get(at + 1) == Some(&terminal_count)).then_some(())?;
        (record.payload().get(at + 2..at + 2 + SENTINEL.len()) == Some(&SENTINEL)).then_some(())?;
        Some(IdenticalInstanceOutputPayloadLane {
            offset: record.payload_offset() + start,
            leading_schema_index,
            count_schema_index,
            selectors: compact::CountedIndexMembers::new(selectors).ok()?,
        })
    };
    unique_candidate((0..record.payload().len().saturating_sub(3)).filter_map(decode))
}

/// Decode the exact leading construction header in a bounded `POINT` payload.
pub fn point_feature_payload_header(
    record: OperationPayload<'_>,
) -> Option<PointFeaturePayloadHeader> {
    const PREFIX: [u8; 7] = [0x72, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00];
    const REFERENCE_SUFFIX: [u8; 42] = [
        0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0d, 0x01, 0x02, 0x01, 0x00, 0x00, 0x00, 0x89,
        0x02, 0x01, 0x01, 0x01, 0x00, 0xa5, 0x57, 0x95, 0x01, 0x00, 0x00, 0xff,
    ];
    const MODE_SUFFIX: [u8; 20] = [
        0xc0, 0x1f, 0xff, 0xfd, 0x01, 0x00, 0x00, 0x01, 0x01, 0x01, 0x03, 0x02, 0x01, 0x01, 0x01,
        0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    if record.name() != "POINT" || record.payload().get(..PREFIX.len()) != Some(&PREFIX) {
        return None;
    }
    let mut at = PREFIX.len();
    let reference_offset = at;
    let (object_index, width) = payload_object_index(record.payload().get(at..)?)?;
    at += width;
    (record.payload().get(at..at + REFERENCE_SUFFIX.len()) == Some(&REFERENCE_SUFFIX))
        .then_some(())?;
    at += REFERENCE_SUFFIX.len();
    let mode = discriminators::PointHeaderMode::try_from(*record.payload().get(at)?).ok()?;
    at += 1;
    (record.payload().get(at..at + MODE_SUFFIX.len()) == Some(&MODE_SUFFIX)).then_some(())?;
    Some(PointFeaturePayloadHeader {
        reference: PayloadObjectReference {
            offset: record.payload_offset() + reference_offset,
            token: object_index,
        },
        mode,
    })
}

/// Decode the exact cross-block scalar lane selected by a `POINT` header target.
pub fn point_feature_scalar_lane(
    preceding_block: &[u8],
    target_block: &[u8],
) -> Option<PointFeatureScalarLane> {
    const SUFFIX: [u8; 19] = [
        0x00, 0x25, 0x25, 0x41, 0x00, 0x04, 0x01, 0x07, 0x01, 0xc0, 0x45, 0x10, 0x00, 0x80, 0x86,
        0x02, 0x00, 0x01, 0x00,
    ];
    let preceding_start = preceding_block.len().checked_sub(3)?;
    (target_block.get(45..64) == Some(&SUFFIX)).then_some(())?;
    let mut lane = Vec::with_capacity(48);
    lane.extend_from_slice(&preceding_block[preceding_start..]);
    lane.extend_from_slice(target_block.get(..45)?);
    let values = lane
        .chunks_exact(8)
        .map(ShiftedBinary64::read)
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    Some(PointFeatureScalarLane {
        values,
        offset: preceding_start,
    })
}

/// Decode the exact leading construction branch in a bounded `SWP104`
/// payload.
pub fn swp104_payload_leading_branch(
    record: OperationPayload<'_>,
) -> Option<Swp104PayloadLeadingBranch> {
    const HEADER: [u8; 4] = [0x00, 0x00, 0x01, 0x00];
    (record.name() == "SWP104").then_some(())?;
    let discriminator = NonZeroU8::new(*record.payload().first()?)?;
    (record.payload().get(1..5) == Some(&HEADER)).then_some(())?;

    let [a, b, c, d] = std::array::from_fn::<_, 4, _>(|i| {
        ShiftedBinary64::read(record.payload().get(5 + i * 8..13 + i * 8)?)
    });
    let scalars = [a?, b?, c?, d?];
    let mut at = 37;

    let leading_zero = record.payload().get(at) == Some(&0x00);
    at += usize::from(leading_zero);
    let mode = NonZeroU8::new(*record.payload().get(at)?)?;
    (*record.payload().get(at + 1)? == 0x01).then_some(())?;
    let declared_count @ 2.. = *record.payload().get(at + 2)? else {
        return None;
    };
    at += 3;
    let mut members = Vec::with_capacity(usize::from(declared_count) - 1);
    for _ in 1..declared_count {
        let object_index = reference_index::PayloadIndexToken::read(record.payload().get(at..)?)?;
        let width = object_index.raw().len();
        at += width;
        members.push(object_index);
    }

    let witnessed_count = if record.payload().get(at) == Some(&0x01) {
        let count @ 2.. = *record.payload().get(at + 1)? else {
            return None;
        };
        Some(count)
    } else {
        None
    };
    let state_len = if let Some(count) = witnessed_count {
        at += 2;
        usize::from(count) + 3
    } else {
        5
    };
    let state_lane = Swp104StateLane::from_parts(
        witnessed_count,
        record.payload().get(at..at + state_len)?.to_vec(),
    )
    .ok()?;
    at += state_len;
    (record.payload().get(at..at + 3) == Some(&[0xff, 0x01, 0x02])).then_some(())?;
    at += 3;
    let object_index = reference_index::PayloadIndexToken::read(record.payload().get(at..)?)?;
    let width = object_index.raw().len();
    at += width;
    let terminal = object_index;
    (*record.payload().get(at)? == 0x00).then_some(())?;

    Some(Swp104PayloadLeadingBranch {
        discriminator,
        scalars,
        leading_zero,
        mode,
        state_lane,
        members: BranchItems::new(members).ok()?,
        terminal,
    })
}

/// Decode the fixed two-scalar header in a bounded `EXTRUDE` payload.
pub fn extrude_payload_header(record: OperationPayload<'_>) -> Option<ExtrudePayloadHeader> {
    if record.name() != "EXTRUDE"
        || record.payload().get(..5) != Some(&[0x0f, 0x00, 0x00, 0x01, 0x00])
    {
        return None;
    }
    Some(ExtrudePayloadHeader {
        offset: record.payload_offset() + 5,
        scalars: [
            ShiftedBinary64::read(record.payload().get(5..13)?)?,
            ShiftedBinary64::read(record.payload().get(13..21)?)?,
        ],
    })
}

/// Decode wrapped member lanes following branch-`11` body scalar clauses.
pub fn operation_body_members(record: OperationBodyInput<'_>) -> Vec<OperationBodyMemberGroup> {
    operation_body_references(record)
        .into_iter()
        .enumerate()
        .filter_map(|(body_ordinal, reference)| {
            let token = reference.offset - record.offset();
            let end = token + reference.object_index.raw().len();
            if record.bytes().get(end..end + 2) != Some(&[0xff, 0x11]) {
                return None;
            }
            let mut at = end + 2;
            for _ in 0..3 {
                let atom = record.bytes().get(at..).and_then(PayloadScalarAtom::read)?;
                at += atom.raw().len();
            }
            if record.bytes().get(at) != Some(&0x01) {
                return None;
            }
            let count = record.bytes().get(at + 1).copied().map(usize::from)?;
            if count < 2 {
                return None;
            }
            at += 2;
            let mut members = Vec::with_capacity(count - 1);
            for _ in 0..count - 1 {
                if record.bytes().get(at) != Some(&0x2e) {
                    return None;
                }
                at += 1;
                let member_at = at;
                let atom = record.bytes().get(at..).and_then(CompactIndexAtom::read)?;
                at += atom.raw().len();
                if record.bytes().get(at) != Some(&0x00) {
                    return None;
                }
                at += 1;
                members.push(LocatedCompactIndex {
                    atom,
                    offset: record.offset() + member_at,
                });
            }
            Some(OperationBodyMemberGroup {
                body_reference_ordinal: body_ordinal as u32,
                body_object_index: reference.object_index.value(),
                members,
            })
        })
        .collect()
}

/// Decode exact continuations following `TRIM BODY` branch-`11` member lanes.
pub fn operation_body_11_continuations(
    record: OperationBodyInput<'_>,
) -> Vec<OperationBody11Continuation> {
    if record.name() != "TRIM BODY" {
        return Vec::new();
    }
    operation_body_references(record)
        .into_iter()
        .enumerate()
        .filter_map(|(body_ordinal, reference)| {
            let token = reference.offset - record.offset();
            let end = token + reference.object_index.raw().len();
            if record.bytes().get(end..end + 2) != Some(&[0xff, 0x11]) {
                return None;
            }
            let mut at = end + 2;
            for _ in 0..3 {
                let width = PayloadScalarAtom::read(record.bytes().get(at..)?)?
                    .raw()
                    .len();
                at += width;
            }
            if record.bytes().get(at) != Some(&0x01) {
                return None;
            }
            let member_count = usize::from(*record.bytes().get(at + 1)?);
            if member_count < 2 {
                return None;
            }
            at += 2;
            for _ in 0..member_count - 1 {
                if record.bytes().get(at) != Some(&0x2e) {
                    return None;
                }
                at += 1;
                let token = NullableCompactIndex::read(record.bytes(), at)?;
                token.atom?;
                at += token.raw().len();
                if record.bytes().get(at) != Some(&0x00) {
                    return None;
                }
                at += 1;
            }
            if record.bytes().get(at..at + 2) != Some(&[0x01, 0x02]) {
                return None;
            }
            at += 2;
            let mut continuation = LocatedCompactIndex::read(record.bytes(), at)?;
            at += continuation.atom.raw().len();
            continuation.offset += record.offset();
            if record.bytes().get(at..at + 3) != Some(&[0x00, 0x00, 0x01]) {
                return None;
            }
            at += 3;
            let terminal_at = at;
            let terminal_token = ReferenceIndexToken::read_feature(record.bytes().get(at..)?)?;
            let next = at + terminal_token.raw().len();
            if record.bytes().get(next..next + 2) != Some(&[0x00, 0x00]) {
                return None;
            }
            Some(OperationBody11Continuation {
                body_reference_ordinal: body_ordinal as u32,
                body_object_index: reference.object_index.value(),
                continuation,
                terminal: PayloadObjectReference {
                    token: terminal_token,
                    offset: record.offset() + terminal_at,
                },
            })
        })
        .collect()
}

/// Decode complete unwrapped counted reference lanes following body scalar clauses.
pub fn operation_body_reference_lanes(
    record: OperationBodyInput<'_>,
) -> Vec<OperationBodyReferenceLane> {
    operation_body_references(record)
        .into_iter()
        .enumerate()
        .filter_map(|(body_ordinal, reference)| {
            let token = reference.offset - record.offset();
            let end = token + reference.object_index.raw().len();
            let branch = discriminators::OperationBodyReferenceBranch::try_from(
                *record.bytes().get(end + 1)?,
            )
            .ok()?;
            let mut at = end + 2;
            for _ in 0..3 {
                let width = PayloadScalarAtom::read(record.bytes().get(at..)?)?
                    .raw()
                    .len();
                at += width;
            }
            if record.bytes().get(at) != Some(&0x01) {
                return None;
            }
            let count = usize::from(*record.bytes().get(at + 1)?);
            if count < 2 {
                return None;
            }
            at += 2;
            let compact =
                operation_body_reference_lane_values(record, at, count - 1, |bytes, offset| {
                    let atom = CompactIndexAtom::read(bytes)?;
                    let width = atom.raw().len();
                    Some((LocatedCompactIndex { atom, offset }, width))
                });
            let objects =
                operation_body_reference_lane_values(record, at, count - 1, |bytes, offset| {
                    let token = reference_index::PayloadIndexToken::read(bytes)?;
                    let width = token.raw().len();
                    Some((PayloadObjectReference { offset, token }, width))
                });
            let values = match (compact, objects) {
                (Some(values), None) => OperationBodyReferenceLaneValues::CompactIndex(values),
                (None, Some(values)) => {
                    OperationBodyReferenceLaneValues::PayloadObjectIndex(values)
                }
                _ => return None,
            };
            Some(OperationBodyReferenceLane {
                body_reference_ordinal: body_ordinal as u32,
                body_object_index: reference.object_index.value(),
                branch,
                values,
            })
        })
        .collect()
}

fn operation_body_reference_lane_values<T>(
    record: OperationBodyInput<'_>,
    mut at: usize,
    count: usize,
    read: impl Fn(&[u8], usize) -> Option<(T, usize)>,
) -> Option<Vec<T>> {
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let (value, width) = read(record.bytes().get(at..)?, record.offset() + at)?;
        at += width;
        values.push(value);
    }
    (record.bytes().get(at..at + 4) == Some(&[0x00, 0x00, 0x0b, 0x00])).then_some(values)
}

/// Decode one complete datum-plane descriptor block.
pub fn datum_plane_descriptor_block(bytes: &[u8]) -> Option<plane_descriptor::PlaneDescriptor> {
    plane_descriptor::PlaneDescriptor::read(bytes)
}

/// Decode every complete scalar-vector frame in a reconstructed sketch
/// payload.
pub fn sketch_payload_scalar_lanes(bytes: &[u8]) -> Vec<FramedScalarRun<SketchScalarLaneForm, ()>> {
    let mut lanes = [SketchScalarLaneForm::Form03, SketchScalarLaneForm::Form07]
        .into_iter()
        .flat_map(|form| {
            let discriminator = form.discriminator();
            bytes
                .windows(discriminator.len())
                .enumerate()
                .filter_map(move |(offset, window)| {
                    (window == discriminator).then_some(())?;
                    let mut at = offset + discriminator.len();
                    let mut values = Vec::new();
                    loop {
                        if bytes.get(at) == Some(&0x00) {
                            break;
                        }
                        let scalar = ShiftedScalar::read(bytes.get(at..)?)?;
                        at += scalar.raw().len();
                        values.push((scalar, ()));
                    }
                    FramedScalarRun::new(form, offset as u64, NonEmpty::new(values)?).ok()
                })
        })
        .collect::<Vec<_>>();
    lanes.sort_by_key(FramedScalarRun::offset);
    lanes
}

/// Decode every exactly framed scaled shifted-binary64 pair in a reconstructed sketch payload.
pub fn sketch_payload_fixed_pairs(bytes: &[u8]) -> Vec<SketchPayloadFixedPair> {
    let mut pairs = Vec::new();
    for form in SketchPairForm::ALL {
        let discriminator = form.discriminator();
        let separator_width = form.separator_width();
        for (offset, window) in bytes.windows(discriminator.len()).enumerate() {
            if window != discriminator {
                continue;
            }
            let first = offset + discriminator.len();
            let second = first + 8 + separator_width;
            if bytes.get(first) != Some(&0x30)
                || (separator_width != 0 && bytes.get(first + 8) != Some(&0x00))
                || bytes.get(second) != Some(&0x30)
            {
                continue;
            }
            let Some(first_value) = sketch_fixed_atom(bytes, first) else {
                continue;
            };
            let Some(second_value) = sketch_fixed_atom(bytes, second) else {
                continue;
            };
            pairs.push(SketchPayloadFixedPair {
                offset,
                values: [first_value, second_value],
                form,
            });
        }
    }
    pairs.sort_by_key(|pair| pair.offset);
    pairs
}

/// Decode every exactly framed mixed scaled shifted-binary64/binary32 pair in a sketch payload.
pub fn sketch_payload_mixed_pairs(bytes: &[u8]) -> Vec<SketchPayloadMixedPair> {
    let discriminator = SketchPairForm::Legacy.discriminator();
    let mut pairs = Vec::new();
    for (offset, window) in bytes.windows(discriminator.len()).enumerate() {
        if window != discriminator {
            continue;
        }
        let fixed_offset = offset + discriminator.len();
        let binary32_offset = fixed_offset + 9;
        if bytes.get(fixed_offset) != Some(&0x30) || bytes.get(fixed_offset + 8) != Some(&0x00) {
            continue;
        }
        let Some(binary32_raw_value): Option<[u8; 4]> = bytes
            .get(binary32_offset..binary32_offset + 4)
            .and_then(|raw| raw.try_into().ok())
        else {
            continue;
        };
        let Some(binary32_atom) = ShiftedBinary32::read(&binary32_raw_value) else {
            continue;
        };
        let Some(fixed) = sketch_fixed_atom(bytes, fixed_offset) else {
            continue;
        };
        pairs.push(SketchPayloadMixedPair {
            offset,
            scalars: SketchMixedScalars {
                fixed,
                binary32: binary32_atom,
            },
        });
    }
    pairs
}

fn sketch_fixed_atom(bytes: &[u8], offset: usize) -> Option<SketchScaledAtom> {
    Some(SketchScaledAtom::from_raw(
        bytes.get(offset + 1..offset + 8)?.try_into().ok()?,
    ))
}

/// Decode every exactly framed signed Q1.55 pair in a datum-CSYS payload.
pub fn datum_csys_payload_fixed_pairs(bytes: &[u8]) -> Vec<DatumCsysPayloadFixedPair> {
    let mut pairs = Vec::new();
    for form in DatumPairForm::ALL {
        let discriminator = form.discriminator();
        for (offset, window) in bytes.windows(discriminator.len()).enumerate() {
            if window != discriminator {
                continue;
            }
            let first = offset + discriminator.len();
            let second = first + 9;
            if bytes.get(first) != Some(&0x30)
                || bytes.get(first + 8) != Some(&0x00)
                || bytes.get(second) != Some(&0x30)
            {
                continue;
            }
            let Some(first_raw) = bytes
                .get(first + 1..first + 8)
                .and_then(|raw| raw.try_into().ok())
            else {
                continue;
            };
            let Some(second_raw) = bytes
                .get(second + 1..second + 8)
                .and_then(|raw| raw.try_into().ok())
            else {
                continue;
            };
            pairs.push(DatumCsysPayloadFixedPair {
                offset,
                values: [Q155::from_raw(first_raw), Q155::from_raw(second_raw)],
                form,
            });
        }
    }
    pairs.sort_by_key(|pair| pair.offset);
    pairs
}

/// Decode every complete signed Q1.55 lane in a reconstructed draft graph payload.
pub fn draft_construction_fixed_lanes(bytes: &[u8]) -> Vec<FramedScalarRun<Q155LaneFrame, ()>> {
    bytes
        .windows(Q155LaneFrame::DISCRIMINATOR.len())
        .enumerate()
        .filter_map(|(offset, window)| {
            (window == Q155LaneFrame::DISCRIMINATOR).then_some(())?;
            let mut at = offset + Q155LaneFrame::DISCRIMINATOR.len();
            let mut values = Vec::new();
            while let Some(marker) = bytes.get(at).copied().and_then(Q155Marker::read) {
                let raw = bytes.get(at + 1..at + 8)?.try_into().ok()?;
                values.push((
                    Q155Atom {
                        marker,
                        scalar: Q155::from_raw(raw),
                    },
                    (),
                ));
                at += 8;
            }
            if bytes.get(at) != Some(&0x00) {
                return None;
            }
            FramedScalarRun::new(Q155LaneFrame, offset as u64, NonEmpty::new(values)?).ok()
        })
        .collect()
}

/// Decode every complete shifted-binary32 lane in a reconstructed draft graph payload.
pub fn draft_construction_binary32_lanes(
    bytes: &[u8],
) -> Vec<FramedScalarRun<DraftBinary32Branch, ()>> {
    let mut lanes = [DraftBinary32Branch::Form04, DraftBinary32Branch::Form03]
        .into_iter()
        .flat_map(|branch| {
            let discriminator = branch.discriminator();
            bytes
                .windows(discriminator.len())
                .enumerate()
                .filter_map(move |(offset, window)| {
                    (window == discriminator).then_some(())?;
                    let mut at = offset + discriminator.len();
                    let mut values = Vec::new();
                    while matches!(bytes.get(at), Some(0x40..=0x5f | 0xc0..=0xdf)) {
                        let scalar = ShiftedBinary32::read(bytes.get(at..at + 4)?)?;
                        values.push((scalar, ()));
                        at += 4;
                    }
                    if bytes.get(at) != Some(&0x00) {
                        return None;
                    }
                    FramedScalarRun::new(branch, offset as u64, NonEmpty::new(values)?).ok()
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    lanes.sort_by_key(FramedScalarRun::offset);
    lanes
}

/// Decode a bounded datum-CSYS descriptor containing one unique maximal identity run.
pub fn datum_csys_descriptor_block(bytes: &[u8]) -> Option<csys_descriptor::CsysDescriptor> {
    csys_descriptor::CsysDescriptor::read(bytes)
}

/// Decode every complete identity frame in a reconstructed draft construction payload.
pub fn draft_construction_identity_frames(bytes: &[u8]) -> Vec<draft_identity::DraftIdentityFrame> {
    (0..bytes.len())
        .filter_map(|offset| draft_identity::DraftIdentityFrame::read(bytes, offset))
        .collect()
}

/// Decode compact object IDs followed by their complete frame discriminator.
pub fn data_block_object_frames(bytes: &[u8]) -> Vec<LocatedCompactIndex> {
    const DISCRIMINATOR: [u8; 18] = [
        0x00, 0x72, 0x01, 0xc0, 0x20, 0x02, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x01,
        0x02, 0x80, 0xa4,
    ];
    let mut references = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        let Some(atom) = CompactIndexAtom::read(&bytes[offset..]) else {
            offset += 1;
            continue;
        };
        let width = atom.raw().len();
        if bytes.get(offset + width..offset + width + DISCRIMINATOR.len()) != Some(&DISCRIMINATOR) {
            offset += 1;
            continue;
        }
        references.push(LocatedCompactIndex { atom, offset });
        offset += width + DISCRIMINATOR.len();
    }
    references
}

/// Decode the unique `04, length, p<decimal>[_qualifier], 00` declaration name.
pub fn expression_declaration_name(bytes: &[u8]) -> Option<ExpressionDeclarationName<'_>> {
    let mut declaration = None;
    let mut literal = None;
    let mut multiple_literals = false;
    for at in 0..bytes.len().saturating_sub(4) {
        if bytes[at] != 0x04 {
            continue;
        }
        let declared = usize::from(bytes[at + 1]);
        if declared < 4 {
            continue;
        }
        let Some(end) = at.checked_add(declared) else {
            continue;
        };
        let Some(raw) = bytes.get(at + 2..end) else {
            continue;
        };
        if bytes.get(end) != Some(&0) {
            continue;
        }
        let Ok(value) = std::str::from_utf8(raw) else {
            continue;
        };
        let Some(name) = ParameterName::<_, u32>::parse(value) else {
            if evaluate_constant_expression(value).is_some() && literal.replace(value).is_some() {
                multiple_literals = true;
            }
            continue;
        };
        if declaration.replace((at, name)).is_some() {
            return None;
        }
    }
    let (offset, name) = declaration?;
    let literal = (!multiple_literals).then_some(literal).flatten();
    Some(ExpressionDeclarationName {
        offset,
        name,
        literal,
    })
}

/// Decode the unique direct primary-body field in one operation.
pub fn operation_body_reference(record: OperationBodyInput<'_>) -> Option<OperationBodyReference> {
    unique_candidate(operation_body_reference_candidates(record))
}

fn operation_body_reference_candidates(
    record: OperationBodyInput<'_>,
) -> impl Iterator<Item = OperationBodyReference> + '_ {
    let mut cursor = 0usize;
    std::iter::from_fn(move || loop {
        let window_end = cursor.checked_add(3)?;
        let window = record.bytes().get(cursor..window_end)?;
        let marker = cursor;
        cursor += 1;
        let body_write = marker
            .checked_sub(record.payload_start())
            .and_then(|payload_marker| {
                operation_body_write_frame_at(
                    record.payload(),
                    record.payload_offset(),
                    payload_marker,
                )
            });
        if let Some(body_write) = body_write {
            cursor = cursor.max(body_write.end_offset() - record.offset());
            continue;
        }
        if window == [0x01, 0x02, 0x10] {
            let token = marker + 3;
            let Some(object_index) =
                reference_index::FeatureReferenceToken::read(&record.bytes()[token..])
            else {
                continue;
            };
            let end = token + object_index.raw().len();
            if record.bytes().get(end) != Some(&0xff) {
                continue;
            }
            return Some(OperationBodyReference {
                offset: record.offset() + token,
                object_index,
            });
        }
    })
}

/// Decode every ordered direct primary-body field in one operation.
pub fn operation_body_references(record: OperationBodyInput<'_>) -> Vec<OperationBodyReference> {
    operation_body_reference_candidates(record).collect()
}

/// Decode every exact nested `01 02 tag index 97 75 01 02 endpoint_tag index ff` frame.
///
/// Both indices are non-null and canonical. Endpoint tags `10`, `12`, and
/// `15` select the body-image field across the supported schema generations.
pub fn operation_body_write_frames(record: OperationPayload<'_>) -> Vec<BodyWriteFrame<usize>> {
    body_write_frames(record.payload(), record.payload_offset())
}

/// Decode body-write frames from one independently bounded unlabeled record.
pub fn unlabeled_operation_body_write_frames(
    record: UnlabeledOperationRecord<'_>,
) -> Vec<BodyWriteFrame<usize>> {
    body_write_frames(record.payload(), record.header().end_offset())
}

fn body_write_frames(payload: &[u8], payload_offset: usize) -> Vec<BodyWriteFrame<usize>> {
    let mut relations = Vec::new();
    for marker in payload
        .windows(2)
        .enumerate()
        .filter_map(|(offset, window)| (window == [0x01, 0x02]).then_some(offset))
    {
        if let Some(write) = operation_body_write_frame_at(payload, payload_offset, marker) {
            relations.push(write);
        }
    }
    relations
}

fn operation_body_write_frame_at(
    payload: &[u8],
    payload_offset: usize,
    marker: usize,
) -> Option<BodyWriteFrame<usize>> {
    let body_identity = *payload.get(marker + 2)?;
    let group_node = BodyWriteIndex::read(payload.get(marker + 3..)?)?;
    let first_end = marker + 3 + group_node.raw().len();
    (payload.get(first_end..first_end + 4) == Some(&[0x97, 0x75, 0x01, 0x02])).then_some(())?;
    let endpoint_tag = BodyImageTag::try_from(*payload.get(first_end + 4)?).ok()?;
    let image_at = first_end + 5;
    let body_image = BodyWriteIndex::read(payload.get(image_at..)?)?;
    (payload.get(image_at + body_image.raw().len()) == Some(&0xff)).then_some(())?;
    BodyWriteFrame::<usize>::new(
        body_identity,
        group_node,
        endpoint_tag,
        body_image,
        payload_offset.checked_add(marker)?,
    )
}

fn feature_object_index(bytes: &[u8], at: usize) -> Option<(Option<u32>, usize)> {
    let prefix = *bytes.get(at)?;
    match prefix {
        0x00..=0x7f => Some((Some(u32::from(prefix)), at + 1)),
        0x80..=0x8f => Some((
            Some(u32::from(prefix - 0x80) * 256 + u32::from(*bytes.get(at + 1)?)),
            at + 2,
        )),
        0x90 => Some((Some(u32::from(View::u16_be_at(bytes, at + 1)?)), at + 3)),
        0xff => Some((None, at + 1)),
        _ => None,
    }
}

/// Decode complete message records in one already bounded state region.
#[cfg(test)]
pub fn operation_state_messages(
    bytes: &[u8],
    base_offset: usize,
) -> Vec<OperationStateMessage<'_>> {
    let mut messages = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let Some(message) = OperationStateMessage::read(bytes, at, base_offset) else {
            at += 1;
            continue;
        };
        at = message.end_offset() - base_offset;
        messages.push(message);
    }
    messages
}

fn operation_state_group_table_before_counter_map(
    bytes: &[u8],
    map_start: usize,
    base_offset: usize,
) -> Option<OperationStateGroupTable> {
    #[derive(Clone, Copy)]
    struct GroupPath {
        last_candidate: usize,
        length: usize,
        first_start: usize,
    }

    if map_start > bytes.len() {
        return None;
    }
    let mut candidates = Vec::new();
    for at in 0..map_start.saturating_sub(2) {
        if !matches!(bytes.get(at..at + 2), Some([0x01, 0x00 | 0x01])) {
            continue;
        }
        let Some(end) = operation_state_group_end_at(bytes, at, map_start, base_offset) else {
            continue;
        };
        candidates.push((at, end));
    }
    candidates.sort_by_key(|(start, end)| (*end, *start));

    let mut predecessors = cadmpeg_core::decode::alloc_filled(
        candidates.len(),
        None,
        "nx operation-state group predecessors",
    )
    .ok()?;
    let mut best_by_end = BTreeMap::<usize, GroupPath>::new();
    for (candidate_index, (start, end)) in candidates.iter().enumerate() {
        let previous = best_by_end.get(start).copied();
        let path = GroupPath {
            last_candidate: candidate_index,
            length: previous.map_or(1, |path| path.length + 1),
            first_start: previous.map_or(*start, |path| path.first_start),
        };
        predecessors[candidate_index] = previous.map(|path| path.last_candidate);
        let replace = best_by_end.get(end).is_none_or(|current| {
            path.length > current.length
                || (path.length == current.length && path.first_start < current.first_start)
        });
        if replace {
            best_by_end.insert(*end, path);
        }
    }

    let trailing_start =
        if map_start >= 2 && bytes.get(map_start - 2..map_start) == Some(&[0x01, 0x01]) {
            map_start - 2
        } else {
            map_start
        };
    let terminal = best_by_end.get(&trailing_start).copied()?;
    let mut path = Vec::with_capacity(terminal.length);
    let mut candidate = Some(terminal.last_candidate);
    while let Some(candidate_index) = candidate {
        path.push(candidate_index);
        candidate = predecessors[candidate_index];
    }
    path.reverse();
    let last = *path.last()?;
    let groups = path
        .into_iter()
        .map(|candidate| {
            operation_state_group_at(bytes, candidates[candidate].0, map_start, base_offset)
        })
        .collect::<Option<Vec<_>>>()?;
    OperationStateGroupTable::new(groups, bytes.get(candidates[last].1..map_start)?)
}

/// Decode a complete bounded `m_rollForwardStates` group table.
#[cfg(test)]
pub fn operation_state_group_table(
    bytes: &[u8],
    start: usize,
    end: usize,
    base_offset: usize,
) -> Option<OperationStateGroupTable> {
    if start >= end || end > bytes.len() {
        return None;
    }
    let mut groups = Vec::new();
    let mut at = start;
    let mut trailing_start = end;
    while at < end {
        let Some(group) = operation_state_group_at(bytes, at, end, base_offset) else {
            if bytes.get(at..end) == Some(&[0x01, 0x01]) {
                trailing_start = at;
                at = end;
                break;
            }
            return None;
        };
        at = group.end_offset().checked_sub(base_offset)?;
        groups.push(group);
    }
    if at != end {
        return None;
    }
    OperationStateGroupTable::new(groups, bytes.get(trailing_start..end)?)
}

fn audit_trail_row_at(
    bytes: &[u8],
    at: usize,
    end: usize,
    base_offset: usize,
) -> Option<AuditTrailRow> {
    let bytes = bytes.get(..end)?;
    if bytes.get(at) != Some(&0x04) {
        return None;
    }
    let ordinal = OperationStateIndex::read_at(bytes, at.checked_add(1)?, base_offset)?.token()?;
    let mut cursor = at.checked_add(1 + ordinal.raw().len())?;
    if bytes.get(cursor) != Some(&0x13) {
        return None;
    }
    cursor += 1;

    let frame_selector = if bytes
        .get(cursor..cursor + 4)
        .is_some_and(|frame| frame[0] == 0x04 && frame[1] == 0x05 && frame[3] == 0x00)
    {
        let selector = *bytes.get(cursor + 2)?;
        cursor += 4;
        Some(selector)
    } else {
        None
    };

    if bytes.get(cursor) != Some(&0xe0) {
        return None;
    }
    let timestamp = View::u32_be_at(bytes, cursor + 1)?;
    cursor = cursor.checked_add(5)?;
    let value = StateTaggedValue::read_at(bytes, cursor)?;
    AuditTrailRow::new(
        base_offset,
        at,
        AuditRecord {
            ordinal,
            frame_selector,
            timestamp,
            value,
        },
    )
}

/// Decode complete audit-trail rows from a bounded record-area suffix.
///
/// A row scan is admitted only when its ordinal tokens are strictly increasing
/// in source order. This rejects a coincidental inner match instead of
/// assigning a second interpretation to a row sequence. Bytes that do not
/// complete the row grammar are left untyped.
pub fn audit_trail_rows(
    bytes: &[u8],
    start: usize,
    end: usize,
    base_offset: usize,
) -> Option<Vec<AuditTrailRow>> {
    if start >= end || end > bytes.len() {
        return None;
    }
    let mut rows = Vec::new();
    let mut at = start;
    let mut previous_ordinal = None;
    while at < end {
        let Some(row) = audit_trail_row_at(bytes, at, end, base_offset) else {
            at += 1;
            continue;
        };
        let ordinal = row.record().ordinal.value();
        if previous_ordinal.is_some_and(|previous| ordinal <= previous) {
            return None;
        }
        previous_ordinal = Some(ordinal);
        at = row.local_end();
        rows.push(row);
    }
    Some(rows)
}

fn operation_state_journal_start(bytes: &[u8], product_end: usize) -> Option<usize> {
    let marker = bytes
        .get(product_end..)?
        .windows(2)
        .position(|window| window == [0x41, 0x00])?
        .checked_add(product_end)?;
    let mut at = marker.checked_add(2)?;
    let mut count_tokens = 0usize;
    let mut saw_repeated_token = false;
    loop {
        if bytes
            .get(at..at + 3)
            .is_some_and(|token| token[0] == 0x03 && token[1] == 0x05)
        {
            count_tokens += 1;
            at += 3;
        } else if bytes.get(at..at + 4) == Some(&[0x03, 0x03, 0x02, 0x00]) {
            saw_repeated_token = true;
            at += 4;
        } else {
            break;
        }
    }
    if bytes.get(at) == Some(&0x00) {
        at += 1;
    }
    while bytes.get(at..at + 2) == Some(&[0x04, 0x00]) {
        at += 2;
    }
    (count_tokens > 0 && (saw_repeated_token || count_tokens >= 2)).then_some(at)
}

fn operation_state_journal_groups_before_boundary(
    bytes: &[u8],
    start: usize,
    end: usize,
    base_offset: usize,
) -> Option<Vec<JournalGroup<usize>>> {
    if start >= end || end > bytes.len() {
        return None;
    }
    let mut groups = Vec::new();
    let mut at = start;
    let mut previous_ordinal = None;
    loop {
        let Some(group) = JournalGroup::read(bytes, at, end, base_offset) else {
            let mut next = at;
            while bytes.get(next..next + 2) == Some(&[0x04, 0x00]) {
                next += 2;
            }
            if next == at || JournalGroup::read(bytes, next, end, base_offset).is_none() {
                break;
            }
            at = next;
            continue;
        };
        for row in group.rows().iter() {
            let ordinal = row.ordinal().value();
            if previous_ordinal.is_some_and(|previous| ordinal <= previous) {
                return None;
            }
            previous_ordinal = Some(ordinal);
        }
        at = group.end_offset().checked_sub(base_offset)?;
        groups.push(group);
    }
    (!groups.is_empty()).then_some(groups)
}

/// Decode a complete bounded state journal.
#[cfg(test)]
pub fn operation_state_journal(
    bytes: &[u8],
    start: usize,
    end: usize,
    base_offset: usize,
) -> Option<Vec<JournalGroup<usize>>> {
    if start >= end || end > bytes.len() {
        return None;
    }
    let mut groups = Vec::new();
    let mut at = start;
    while at < end {
        let group = JournalGroup::read(bytes, at, end, base_offset)?;
        at = group.end_offset().checked_sub(base_offset)?;
        groups.push(group);
    }
    (!groups.is_empty() && at == end).then_some(groups)
}

/// Return one decoded candidate only when the scan produced exactly one.
///
/// A number of OM fields are discovered by probing every byte in an already
/// bounded payload. Materializing all successful probes lets overlapping
/// malformed framing turn a linear scan into an allocation proportional to
/// the number of hits. The parser needs only the uniqueness decision, so keep
/// the first candidate and reject as soon as a second one is complete.
fn unique_candidate<T>(candidates: impl IntoIterator<Item = T>) -> Option<T> {
    let mut candidate = None;
    for next in candidates {
        if candidate.is_some() {
            return None;
        }
        candidate = Some(next);
    }
    candidate
}

/// Decode every exact common frame in one bounded operation payload.
pub fn operation_common_frames(record: OperationPayload<'_>) -> Vec<CommonFrame<usize>> {
    let decode = |start: usize, marker| {
        if marker == [1, 1, 1] && record.name() != "DELETE" {
            return None;
        }
        let bytes = record.payload().get(start..)?;
        let prefix = CommonFramePrefix::read(bytes, marker)?;
        let state_at = prefix.byte_len();
        let state = bytes.get(state_at..state_at + 8)?.try_into().ok()?;
        let suffix = CommonFrameSuffix::read(bytes.get(state_at + 8..)?)?;
        CommonFrame::<usize>::new(
            prefix,
            state,
            suffix,
            record.payload_offset().checked_add(start)?,
        )
    };
    let mut frames = Vec::new();
    for start in 0..record.payload().len() {
        if let Some(frame) = decode(start, [1, 3, 2]) {
            frames.push(frame);
        }
        if let Some(frame) = decode(start, [1, 1, 1]) {
            frames.push(frame);
        }
    }
    frames.sort_by_key(CommonFrame::<usize>::offset);
    frames
}

/// Decode the unique terminal common-frame suffix and its exact immediate common frame.
pub fn operation_terminal_frame(record: OperationPayload<'_>) -> Option<OperationTerminalFrame> {
    let terminator = record.payload().len().checked_sub(1)?;
    (record.payload().get(terminator) == Some(&0)).then_some(())?;
    let common_frames = operation_common_frames(record);
    unique_candidate(
        (terminator.saturating_sub(9)..terminator).filter_map(|start| {
            let suffix = CommonFrameSuffix::read(record.payload().get(start..)?)?;
            (start + suffix.byte_len() == record.payload().len()).then_some(())?;
            let frame =
                TerminalFrame::<usize>::new(suffix, record.payload_offset().checked_add(start)?)?;
            let immediate_common_frame_offset = common_frames
                .iter()
                .find(|common| {
                    common.local_ordinal_offset() == frame.offset()
                        && common.end_offset() == frame.end_offset()
                })
                .map(CommonFrame::<usize>::offset);
            Some(OperationTerminalFrame {
                immediate_common_frame_offset,
                frame,
            })
        }),
    )
}

/// Decode ordered `04 00, object_index, 02 0b` references from one bounded block.
pub fn data_block_object_references(bytes: &[u8]) -> Vec<DataBlockObjectReference> {
    let mut references = Vec::new();
    let mut at = 0usize;
    while at + 5 <= bytes.len() {
        if bytes.get(at..at + 2) != Some(&[0x04, 0x00]) {
            at += 1;
            continue;
        }
        let token = at + 2;
        let Some(object_index) = reference_index::FeatureReferenceToken::read(&bytes[token..])
        else {
            at += 1;
            continue;
        };
        let end = token + object_index.raw().len();
        if bytes.get(end..end + 2) != Some(&[0x02, 0x0b]) {
            at += 1;
            continue;
        }
        references.push(DataBlockObjectReference {
            offset: token,
            object_index,
        });
        at = end + 2;
    }
    references
}

fn boolean_operations_with_labels(
    bytes: &[u8],
    base_offset: usize,
    labels: &[OperationLabel<'_>],
) -> Vec<BooleanOperation> {
    const BODY_HEADER: &[u8] = &[
        0x31, 0x00, 0x00, 0x01, 0x00, 0x14, 0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b, 0x03,
        0x00, 0x00, 0xe0, 0x7f, 0xff, 0xff, 0xff, 0x01, 0x01,
    ];
    labels
        .iter()
        .copied()
        .filter_map(|label| {
            let kind = match label.value {
                "UNITE" => BooleanOperationKind::Unite,
                "SUBTRACT" => BooleanOperationKind::Subtract,
                "INTERSECT" => BooleanOperationKind::Intersect,
                _ => return None,
            };
            let at = label.header.end_offset().checked_sub(base_offset)?;
            let label_end = at.checked_add(usize::from(*bytes.get(at + 1)?))? + 1;
            if bytes.get(label_end..label_end + BODY_HEADER.len()) != Some(BODY_HEADER) {
                return None;
            }
            let (targets, next) =
                counted_feature_object_indices(bytes, base_offset, label_end + BODY_HEADER.len())?;
            if targets.len() != 1 || bytes.get(next) != Some(&0) {
                return None;
            }
            let (tools, end) = counted_feature_object_indices(bytes, base_offset, next + 1)?;
            if tools.is_empty() || bytes.get(end) != Some(&0) {
                return None;
            }
            let target = targets.into_iter().next()?;
            Some(BooleanOperation {
                offset: label.header.end_offset(),
                kind,
                target,
                tools,
            })
        })
        .collect()
}

fn counted_feature_object_indices(
    bytes: &[u8],
    base_offset: usize,
    at: usize,
) -> Option<(Vec<PayloadObjectReference>, usize)> {
    if bytes.get(at) != Some(&0x01) {
        return None;
    }
    let count = usize::from(*bytes.get(at + 1)?).checked_sub(1)?;
    let values_start = at + 2;
    let mut scan_cursor = values_start;
    for _ in 0..count {
        let (value, next) = feature_object_index(bytes, scan_cursor)?;
        value?;
        scan_cursor = next;
    }

    let mut cursor = values_start;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let value = ReferenceIndexToken::read_feature(bytes.get(cursor..)?)?;
        let next = cursor + value.raw().len();
        values.push(PayloadObjectReference {
            offset: base_offset + cursor,
            token: value,
        });
        cursor = next;
    }
    Some((values, cursor))
}

/// Decode count-framed runs of same-section record references.
pub fn counted_record_references(
    bytes: &[u8],
    base_offset: usize,
    record_count: usize,
) -> Vec<LocatedReference<u16>> {
    let mut references = Vec::new();
    let mut at = 0usize;
    while at + 5 <= bytes.len() {
        if bytes[at] != 0x01 || bytes[at + 1] < 2 {
            at += 1;
            continue;
        }
        let count = usize::from(bytes[at + 1] - 1);
        let Some(end) = at.checked_add(2 + count * 3) else {
            at += 1;
            continue;
        };
        if end > bytes.len() || (0..count).any(|index| bytes[at + 2 + index * 3] != 0x90) {
            at += 1;
            continue;
        }
        let run = (0..count)
            .map(|index| {
                let token = at + 2 + index * 3;
                let value = View::u16_be_at(bytes, token + 1)?;
                (usize::from(value) < record_count).then_some(LocatedReference {
                    offset: base_offset + token,
                    value,
                })
            })
            .collect::<Option<Vec<_>>>();
        if let Some(run) = run {
            references.extend(run);
            at = end;
        } else {
            at += 1;
        }
    }
    references
}

/// Decode self-identifying persistent handles and exact adjacent handle pairs.
pub fn record_references(
    bytes: &[u8],
    base_offset: usize,
) -> Vec<LocatedReference<DirectReference>> {
    let parsed = references(bytes, base_offset);
    let mut out = parsed
        .iter()
        .copied()
        .filter(|reference| matches!(reference.value, DirectReference::PersistentHandle(_)))
        .collect::<Vec<_>>();
    out.extend(
        parsed
            .iter()
            .zip(parsed.iter().skip(1))
            .filter_map(|(persistent, tagged)| {
                let adjacent = persistent
                    .offset
                    .checked_add(5)
                    .is_some_and(|offset| tagged.offset == offset);
                (matches!(persistent.value, DirectReference::PersistentHandle(_))
                    && matches!(tagged.value, DirectReference::Tagged28(_))
                    && adjacent)
                    .then_some(*tagged)
            }),
    );
    out.sort_by_key(|reference| reference.offset);
    out
}

/// Decode tagged references wholly contained in `bytes`.
pub fn references(bytes: &[u8], base_offset: usize) -> Vec<LocatedReference<DirectReference>> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        if bytes[at] == 0xe0 {
            if let Some(value) = View::u32_be_at(bytes, at + 1) {
                out.push(LocatedReference {
                    offset: base_offset + at,
                    value: DirectReference::PersistentHandle(value),
                });
                at += 5;
                continue;
            }
        } else if bytes[at] & 0xf0 == 0xc0 {
            if let Some(value) = View::u32_be_at(bytes, at) {
                out.push(LocatedReference {
                    offset: base_offset + at,
                    value: DirectReference::Tagged28(Tagged28::from_word(value)),
                });
                at += 4;
                continue;
            }
        }
        at += 1;
    }
    out
}

/// Decode `66 32 03` printable-string values wholly contained in `bytes`.
pub fn string_values(bytes: &[u8], base_offset: usize) -> Vec<StringValue<'_>> {
    const MARKER: &[u8] = &[0x66, 0x32, 0x03];
    bytes
        .windows(MARKER.len())
        .enumerate()
        .filter(|(_, window)| *window == MARKER)
        .filter_map(|(offset, _)| {
            let declared = usize::from(*bytes.get(offset + 3)?);
            let text_len = declared.checked_sub(2)?;
            let start = offset.checked_add(4)?;
            let end = start.checked_add(text_len)?;
            let raw = bytes.get(start..end)?;
            (bytes.get(end) == Some(&0)).then_some(())?;
            let value = PrintableString::new(std::str::from_utf8(raw).ok()?).ok()?;
            Some(StringValue {
                offset: base_offset + offset,
                value,
            })
        })
        .collect()
}

/// Decode complete `03 26, canonical UUID text, 00` values in `bytes`.
pub fn uuid_string_values(bytes: &[u8], base_offset: usize) -> Vec<UuidStringValue<'_>> {
    const MARKER: &[u8] = &[0x03, 0x26];
    const TEXT_LEN: usize = 36;
    bytes
        .windows(MARKER.len())
        .enumerate()
        .filter(|(_, window)| *window == MARKER)
        .filter_map(|(offset, _)| {
            let start = offset.checked_add(MARKER.len())?;
            let end = start.checked_add(TEXT_LEN)?;
            let raw = bytes.get(start..end)?;
            let value =
                crate::canonical_uuid::CanonicalUuid::new(std::str::from_utf8(raw).ok()?).ok()?;
            (bytes.get(end) == Some(&0)).then_some(UuidStringValue {
                offset: base_offset + offset,
                value,
            })
        })
        .collect()
}

#[cfg(test)]
mod uuid_string_value_tests {
    use super::*;

    #[test]
    fn decodes_only_complete_canonical_uuid_frames() {
        let mut bytes = b"prefix\x03\x2601234567-89ab-cdef-0123-456789abcdef\0suffix".to_vec();
        let values = uuid_string_values(&bytes, 100);
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].offset, 106);
        assert_eq!(
            values[0].value.as_str(),
            "01234567-89ab-cdef-0123-456789abcdef"
        );

        bytes[6 + 2 + 9] = b'A';
        assert!(uuid_string_values(&bytes, 0).is_empty());
        assert!(
            crate::canonical_uuid::CanonicalUuid::new("01234567-89ab-cdef-0123-456789abcde")
                .is_err()
        );
        assert!(
            crate::canonical_uuid::CanonicalUuid::new("01234567-89ab-cdef-0123_456789abcdef")
                .is_err()
        );
        assert!(
            crate::canonical_uuid::CanonicalUuid::new("01234567-89ab-cdef-0123-456789abcdeg")
                .is_err()
        );
    }

    #[test]
    fn rejects_truncated_or_unterminated_uuid_frames() {
        let frame = b"\x03\x2601234567-89ab-cdef-0123-456789abcdef\0";
        assert!(uuid_string_values(&frame[..frame.len() - 1], 0).is_empty());
        let mut unterminated = frame.to_vec();
        *unterminated.last_mut().expect("nonempty frame") = 1;
        assert!(uuid_string_values(&unterminated, 0).is_empty());
    }
}

/// Decode `66 1b 03, byte-length, printable UTF-8, 00` values in `bytes`.
pub fn surface_payload_strings(bytes: &[u8]) -> Vec<SurfacePayloadString<'_>> {
    const MARKER: &[u8] = &[0x66, 0x1b, 0x03];
    bytes
        .windows(MARKER.len())
        .enumerate()
        .filter(|(_, window)| *window == MARKER)
        .filter_map(|(offset, _)| {
            let text_len = usize::from(*bytes.get(offset + MARKER.len())?);
            let start = offset.checked_add(MARKER.len() + 1)?;
            let end = start.checked_add(text_len)?;
            let raw = bytes.get(start..end)?;
            let value =
                crate::payload_text::PayloadText::new(std::str::from_utf8(raw).ok()?).ok()?;
            (bytes.get(end) == Some(&0)).then_some(SurfacePayloadString { offset, value })
        })
        .collect()
}

/// Decode every strictly length-framed numeric expression in an OM payload.
///
/// The `hostglobalvariables` marker identifies the owning table. Individual
/// records are self-framed as `handle, 04, length, text, 00`, so expression
/// decoding does not depend on an object-id table having the same cardinality
/// as an external entity-index array.
pub fn numeric_expressions(bytes: &[u8]) -> Vec<NumericExpression<'_>> {
    if !bytes
        .windows(b"hostglobalvariables".len())
        .any(|window| window == b"hostglobalvariables")
    {
        return Vec::new();
    }
    bytes
        .windows(b"(Number [".len())
        .enumerate()
        .filter(|(_, window)| *window == b"(Number [")
        .filter_map(|(offset, _)| {
            numeric_expression_at(
                &bytes[offset.saturating_sub(3)..],
                offset.saturating_sub(3),
                None,
            )
        })
        .collect()
}

/// Locate independently size-framed OM sections and their type registries.
pub fn sections(bytes: &[u8]) -> Vec<Section<'_>> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 16 <= bytes.len() {
        let Some(relative) = bytes[at..]
            .windows(4)
            .position(|window| window == [0xff; 4])
        else {
            break;
        };
        let offset = at + relative;
        let Some(payload_len) = View::u32_be_at(bytes, offset + 8).map(|value| value as usize)
        else {
            break;
        };
        let standard_end = offset
            .checked_add(16)
            .and_then(|header_end| header_end.checked_add(payload_len));
        let compact_terminal_end = offset
            .checked_add(12)
            .and_then(|header_end| header_end.checked_add(payload_len))
            .filter(|end| *end == bytes.len());
        let Some(end) = standard_end
            .filter(|end| *end <= bytes.len())
            .or(compact_terminal_end)
        else {
            at = offset + 4;
            continue;
        };
        if bytes.get(offset + 12..offset + 14) != Some(b"OM") || end > bytes.len() {
            at = offset + 4;
            continue;
        }
        let type_registry = registry::type_registry(bytes, offset + 16, end);
        let types = type_registry.definitions;
        let field_start = type_registry.field_start;
        let record_area_pointer = section_record_area_pointer(bytes, offset, field_start, end)
            .or_else(|| {
                legacy_feature_record_area_pointer(bytes, offset, field_start, end, &types)
            });
        let (fields, record_area_offset) =
            if let Some((record_area_offset, pointer_offset)) = record_area_pointer {
                (
                    registry::all_field_definitions(bytes, field_start, pointer_offset),
                    Some(record_area_offset),
                )
            } else {
                (registry::field_definitions(bytes, field_start, end), None)
            };
        let record_area = record_area_offset.map(|start| RecordArea {
            offset: start,
            bytes: &bytes[start..end],
        });
        let cached_operation_labels =
            record_area.map_or_else(Vec::new, |area| operation_labels(area.bytes, area.offset));
        out.push(Section {
            offset,
            byte_len: end - offset,
            types: types.into(),
            fields: fields.into(),
            record_area,
            cached_operation_labels: cached_operation_labels.into(),
        });
        at = end;
    }
    out
}

fn section_record_area_pointer(
    bytes: &[u8],
    section_offset: usize,
    schema_start: usize,
    section_end: usize,
) -> Option<(usize, usize)> {
    let mut matches = (schema_start..section_end.saturating_sub(3)).filter_map(|at| {
        let relative = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
        let target = section_offset.checked_add(relative)?;
        (target >= at.checked_add(4)? && target.checked_add(15)? <= section_end).then_some(())?;
        ProductRecord::read(
            bytes.get(target.checked_add(12)?..section_end)?,
            ProductRecordForm::Modern,
        )
        .is_some()
        .then_some((target, at))
    });
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

fn legacy_feature_record_area_pointer(
    bytes: &[u8],
    section_offset: usize,
    schema_start: usize,
    section_end: usize,
    types: &[TypeDefinition<'_>],
) -> Option<(usize, usize)> {
    if !types
        .iter()
        .any(|definition| definition.name == "UGS::FEATURE_RECORD")
    {
        return None;
    }
    unique_candidate(
        (schema_start..section_end.saturating_sub(4)).filter_map(|at| {
            if bytes.get(at) != Some(&0x01) {
                return None;
            }
            let relative = usize::try_from(View::u32_le_at(bytes, at + 1)?).ok()?;
            let target = section_offset.checked_add(relative)?.checked_add(1)?;
            (target >= at.checked_add(5)? && target.checked_add(12)? <= section_end)
                .then_some(())?;
            View::u32_le_at(bytes, target)?;
            View::u32_le_at(bytes, target + 4)?;
            View::u32_le_at(bytes, target + 8)?;
            ProductRecord::read(
                bytes.get(target + 12..section_end)?,
                ProductRecordForm::LegacyFeature,
            )?;
            Some((target, at))
        }),
    )
}

#[derive(Debug, Clone, Copy)]
struct ProductRecordRange {
    start: usize,
    end: usize,
}

#[derive(Debug, Clone)]
enum IndexedCandidateKind<'a> {
    Fixed(FixedIndex<'a>),
    OffsetOnly(OffsetIndex<'a>),
}

#[derive(Debug, Clone)]
struct IndexedCandidate<'a> {
    discovery_order: usize,
    kind: IndexedCandidateKind<'a>,
}

impl<'a> IndexedCandidate<'a> {
    fn start(&self) -> usize {
        match &self.kind {
            IndexedCandidateKind::Fixed(index) => index.index_start(),
            IndexedCandidateKind::OffsetOnly(index) => index.index_start(),
        }
    }

    fn source(&self) -> &'a [u8] {
        match &self.kind {
            IndexedCandidateKind::Fixed(index) => index.source(),
            IndexedCandidateKind::OffsetOnly(index) => index.source(),
        }
    }
}

fn product_record_range_at(bytes: &[u8], offset: usize) -> Option<ProductRecordRange> {
    let suffix = bytes.get(offset..)?;
    let layout = ProductRecord::read(suffix, ProductRecordForm::Modern)?;
    Some(ProductRecordRange {
        start: offset,
        end: offset.checked_add(layout.byte_len())?,
    })
}

fn record_area_product_end(bytes: &[u8], offset: usize) -> Option<usize> {
    let suffix = bytes.get(offset..)?;
    let layout = ProductRecord::read(suffix, ProductRecordForm::Modern)
        .or_else(|| ProductRecord::read(suffix, ProductRecordForm::LegacyFeature))?;
    offset.checked_add(layout.byte_len())
}

/// Count validated product records fully contained in `[lower, upper]`.
///
/// A validated product record cannot contain another validated product-record
/// start: its text is restricted to printable ASCII and spaces, while a record
/// start begins with a non-printable byte. The discovery pass therefore orders
/// both starts and ends. This makes the containment query a pair of binary
/// searches instead of a scan over every product record for every candidate.
fn product_record_count_within(ranges: &[ProductRecordRange], lower: usize, upper: usize) -> usize {
    let first = ranges.partition_point(|range| range.start < lower);
    let end = ranges.partition_point(|range| range.end <= upper);
    end.saturating_sub(first)
}

/// Select outer indexed interpretations before materializing section records.
///
/// The entity-index and object-id arrays are the physical ownership envelope
/// of an indexed section. A second valid table wholly inside that envelope is
/// serialized record content, not another section. Partial overlap remains in
/// the result because neither candidate owns the other candidate's bytes.
///
/// Sorting by start and then descending end makes the furthest end of all
/// admitted candidates sufficient to recognize every nested candidate. This
/// keeps the admission pass linear after sorting and, more importantly, keeps
/// rejected candidates as layout metadata rather than allocated records.
fn select_outer_indexed_candidates(
    mut candidates: Vec<IndexedCandidate<'_>>,
) -> Vec<IndexedCandidate<'_>> {
    candidates.sort_by(|left, right| {
        left.start()
            .cmp(&right.start())
            .then_with(|| right.source().len().cmp(&left.source().len()))
    });
    let mut admitted = Vec::with_capacity(candidates.len());
    let mut furthest_end = 0;
    for candidate in candidates {
        if candidate.source().len() <= furthest_end {
            continue;
        }
        furthest_end = candidate.source().len();
        admitted.push(candidate);
    }
    admitted.sort_by_key(|candidate| candidate.discovery_order);
    admitted
}

fn materialize_indexed_candidate(candidate: IndexedCandidate<'_>) -> IndexedSection<'_> {
    let bytes = candidate.source();
    let entity_index_offset = candidate.start();
    let base = match &candidate.kind {
        IndexedCandidateKind::Fixed(index) => index.base(),
        IndexedCandidateKind::OffsetOnly(_) => 0,
    };
    let type_registry = registry::type_registry(bytes, base, entity_index_offset);
    let fields =
        registry::all_field_definitions(bytes, type_registry.field_start, entity_index_offset);
    let (object_id_table_offset, store) = match candidate.kind {
        IndexedCandidateKind::Fixed(index) => (
            index.object_id_table_offset(),
            IndexedStore::Fixed {
                records: index.records().collect::<Vec<_>>().into(),
            },
        ),
        IndexedCandidateKind::OffsetOnly(index) => {
            let control = index.control();
            (
                control.offset,
                IndexedStore::OffsetOnly {
                    control,
                    column_storage: index.column_storage(),
                    records: index.records().collect::<Vec<_>>().into(),
                },
            )
        }
    };
    IndexedSection {
        base,
        entity_index_offset,
        object_id_table_offset,
        types: type_registry.definitions.into(),
        fields: fields.into(),
        store,
    }
}

/// Locate validated NX OM entity-index/object-id-table pairs.
///
/// A candidate is accepted only when the arrays are adjacent, the index is
/// monotone, its first offset is zero, its second offset self-anchors the first
/// entity exactly at the end of the object-id table, and that entity carries the
/// NX root marker.
pub fn indexed_sections(bytes: &[u8]) -> Vec<IndexedSection<'_>> {
    let mut candidates = Vec::new();
    let mut seen_record_starts = BTreeSet::new();
    let product_record_ranges = (0..bytes.len())
        .filter_map(|offset| product_record_range_at(bytes, offset))
        .collect::<Vec<_>>();
    let descending_u32_edges = DescendingU32Edges::new(bytes);
    for table in 0..bytes.len().saturating_sub(4) {
        let Some(count) = View::u32_le_at(bytes, table).map(|value| value as usize) else {
            continue;
        };
        if !(2..=100_000).contains(&count) {
            continue;
        }
        let Some(index_len) = count.checked_add(1).and_then(|n| n.checked_mul(4)) else {
            continue;
        };
        let Some(index_start) = table.checked_sub(index_len) else {
            continue;
        };
        let Some(table_end) = count
            .checked_mul(4)
            .and_then(|length| table.checked_add(4 + length))
        else {
            continue;
        };
        let Some(_) = product_record_range_at(bytes, table_end) else {
            continue;
        };
        if View::u32_le_at(bytes, index_start) != Some(0) {
            continue;
        }
        let Some(first) = View::u32_le_at(bytes, index_start + 4).map(|value| value as usize)
        else {
            continue;
        };
        let Some(base) = table_end.checked_sub(first) else {
            continue;
        };
        let Some(index) = FixedIndex::new(&descending_u32_edges, index_start, count, base, table)
        else {
            continue;
        };
        if !seen_record_starts.insert(table_end) {
            continue;
        }
        candidates.push(IndexedCandidate {
            discovery_order: candidates.len(),
            kind: IndexedCandidateKind::Fixed(index),
        });
    }
    for count_offset in 8..bytes.len().saturating_sub(4) {
        let Some(record_count) = View::u32_le_at(bytes, count_offset).map(|value| value as usize)
        else {
            continue;
        };
        if !(2..=100_000).contains(&record_count) {
            continue;
        }
        let offset_count = record_count + 2;
        let Some(index_len) = offset_count.checked_mul(4) else {
            continue;
        };
        let Some(index_start) = count_offset.checked_sub(index_len) else {
            continue;
        };
        let Some(first) = View::u32_le_at(bytes, index_start).map(|value| value as usize) else {
            continue;
        };
        let Some(second) = View::u32_le_at(bytes, index_start + 4).map(|value| value as usize)
        else {
            continue;
        };
        let Some(third) = View::u32_le_at(bytes, index_start + 8).map(|value| value as usize)
        else {
            continue;
        };
        let Some(last) = View::u32_le_at(bytes, count_offset - 4).map(|value| value as usize)
        else {
            continue;
        };
        if first < count_offset + 4
            || first >= second
            || second > third
            || third > last
            || last > bytes.len()
        {
            continue;
        }
        // The first two data ranges contain the sole product marker. Reject
        // random count words before allocating and walking the full offset
        // table; malformed payloads commonly satisfy the cheap monotonicity
        // checks while carrying no self-framed NX record at all.
        let product_record_count =
            product_record_count_within(&product_record_ranges, first, second).saturating_add(
                product_record_count_within(&product_record_ranges, second, third),
            );
        if product_record_count != 1 {
            continue;
        }
        let Some(index) = OffsetIndex::new(
            &descending_u32_edges,
            index_start,
            offset_count,
            count_offset,
        ) else {
            continue;
        };
        if !seen_record_starts.insert(second) {
            continue;
        }
        candidates.push(IndexedCandidate {
            discovery_order: candidates.len(),
            kind: IndexedCandidateKind::OffsetOnly(index),
        });
    }
    select_outer_indexed_candidates(candidates)
        .into_iter()
        .map(materialize_indexed_candidate)
        .collect()
}

/// Decode the first self-framed NX product/version marker in `bytes`.
pub fn store_version(bytes: &[u8], base_offset: usize) -> Option<StoreVersion<'_>> {
    (0..bytes.len().saturating_sub(3)).find_map(|at| {
        let product = ProductRecord::read(&bytes[at..], ProductRecordForm::Modern)?;
        Some(StoreVersion {
            offset: base_offset.checked_add(at)?,
            value: product.text(),
        })
    })
}

/// Decode the zero-prefixed offset-store control form as ordered 24-bit values.
///
/// Each word is serialized `00, value:u24 LE`. The complete form is atomic.
pub fn offset_store_control_values(bytes: &[u8]) -> Option<NonEmpty<ControlWord24>> {
    bytes.len().is_multiple_of(4).then_some(())?;
    NonEmpty::new(
        bytes
            .chunks_exact(4)
            .map(|word| (word[0] == 0).then(|| ControlWord24::new([word[1], word[2], word[3]]))),
    )?
    .transpose()
}

/// Decode the distinct leading class-registry identities in an offset-store
/// control block.
///
/// The registry may omit declarations that this decoder cannot type, so its
/// retained declaration count is not an ordinal bound. The class lane is
/// instead the unique nonempty prefix whose identities are distinct and all
/// smaller than every following metadata value.
pub fn offset_store_control_class_ordinals(bytes: &[u8]) -> Option<Vec<u32>> {
    let values = offset_store_control_values(bytes)?
        .into_iter()
        .map(ControlWord24::value)
        .collect::<Vec<_>>();
    let mut suffix_minima =
        alloc_filled(values.len(), u32::MAX, "nx offset-store suffix minima").ok()?;
    for index in (0..values.len().saturating_sub(1)).rev() {
        suffix_minima[index] = suffix_minima[index + 1].min(values[index + 1]);
    }
    let mut identities = BTreeSet::new();
    let mut maximum_identity = 0;
    let mut boundary = None;
    for index in 0..values.len().saturating_sub(1) {
        let identity = values[index];
        if !identities.insert(identity) {
            break;
        }
        maximum_identity = maximum_identity.max(identity);
        if maximum_identity < suffix_minima[index] && boundary.replace(index + 1).is_some() {
            return None;
        }
    }
    let boundary = boundary?;
    Some(values[..boundary].to_vec())
}

fn joined_control_byte(control: &[u8], first_record: &[u8], offset: usize) -> Option<u8> {
    if offset < control.len() {
        control.get(offset).copied()
    } else {
        first_record.get(offset - control.len()).copied()
    }
}

fn joined_control_u32_le(control: &[u8], first_record: &[u8], offset: usize) -> Option<u32> {
    Some(
        u32::from(joined_control_byte(control, first_record, offset)?)
            | (u32::from(joined_control_byte(control, first_record, offset + 1)?) << 8)
            | (u32::from(joined_control_byte(control, first_record, offset + 2)?) << 16)
            | (u32::from(joined_control_byte(control, first_record, offset + 3)?) << 24),
    )
}

fn offset_store_product_anchored_form(
    control: &[u8],
    first_record: &[u8],
) -> Option<OffsetStoreControlForm> {
    let product_offset = unique_candidate(
        (0..control.len())
            .filter(|offset| {
                ProductRecord::read(&control[*offset..], ProductRecordForm::Modern).is_some()
            })
            .chain(
                (0..first_record.len())
                    .filter(|offset| {
                        ProductRecord::read(&first_record[*offset..], ProductRecordForm::Modern)
                            .is_some()
                    })
                    .map(|offset| control.len() + offset),
            ),
    )?;
    let leading_width = product_offset % 4;
    if product_offset >= control.len() {
        let control_array_bytes = control.len().checked_sub(leading_width)?;
        (!control_array_bytes.is_multiple_of(4)).then_some(())?;
    }
    let leading_value = if leading_width == 0 {
        None
    } else {
        Some(ControlLeadingValue::read(
            leading_width,
            control.iter().chain(first_record).copied(),
        )?)
    };
    let values = NonEmpty::new(
        (0..(product_offset - leading_width) / 4)
            .map(|index| joined_control_u32_le(control, first_record, leading_width + index * 4)),
    )?
    .transpose()?;
    Some(OffsetStoreControlForm::ProductAnchored {
        leading_value,
        values,
    })
}

/// One complete admitted offset-only store control-block form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OffsetStoreControlForm {
    /// Complete `00 + value:u24 LE` word array.
    ZeroPrefixed {
        /// Ordered values decoded from the complete control block.
        values: NonEmpty<ControlWord24>,
    },
    /// Compact leading value and aligned `u32 LE` array preceding one
    /// self-framed product record.
    ProductAnchored {
        /// Width and value of the compact leading little-endian integer.
        leading_value: Option<ControlLeadingValue>,
        /// Ordered values preceding the product record.
        values: NonEmpty<u32>,
    },
}

/// Classify one complete offset-only store control lane atomically.
///
/// Product-anchored storage may cross the physical boundary between the
/// control block and the first column block. Exactly one admitted grammar must
/// accept the complete control envelope.
pub fn offset_store_control_form(
    control: &[u8],
    first_record: Option<&[u8]>,
) -> Option<OffsetStoreControlForm> {
    let zero = offset_store_control_values(control);
    let product = offset_store_product_anchored_form(control, first_record.unwrap_or_default());
    match (zero, product) {
        (Some(values), None) => Some(OffsetStoreControlForm::ZeroPrefixed { values }),
        (_, Some(form)) => Some(form),
        (None, None) => None,
    }
}

fn numeric_expression_at(
    bytes: &[u8],
    base_offset: usize,
    object_id: Option<u32>,
) -> Option<NumericExpression<'_>> {
    const PREFIX: &[u8] = b"(Number [";
    let relative = bytes
        .windows(PREFIX.len())
        .position(|window| window == PREFIX)?;
    if relative < 3 || bytes.get(relative - 2) != Some(&0x04) {
        return None;
    }
    let declared = usize::from(*bytes.get(relative - 1)?);
    let text_len = declared.checked_sub(2)?;
    let text_end = relative.checked_add(text_len)?;
    (bytes.get(text_end) == Some(&0)).then_some(())?;
    let text = std::str::from_utf8(bytes.get(relative..text_end)?).ok()?;
    let text = text.strip_prefix("(Number [")?;
    let (unit, rest) = text.split_once("]) ")?;
    let unit = crate::om_tokens::unit_for(unit)?;
    let (name, value_tail) = rest.split_once(": ")?;
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return None;
    }
    let (value_text, comment) = value_tail.split_once("; ")?;
    if !comment.is_empty() && !numeric_expression_comment_is_valid(comment) {
        return None;
    }
    Some(NumericExpression {
        object_id,
        offset: base_offset + relative,
        name: ParameterName::new(name),
        unit,
        expression: value_text,
    })
}

fn numeric_expression_comment_is_valid(comment: &str) -> bool {
    comment.starts_with("//")
        && comment
            .bytes()
            .all(|byte| byte.is_ascii_graphic() || matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
}

/// Evaluate the context-free arithmetic subset of NX numeric formulas.
/// Names and function calls fail; they need the parameter graph.
pub(crate) fn evaluate_constant_expression(text: &str) -> Option<f64> {
    // This is the expression grammar's explicit operator stack. Do not turn
    // nested parentheses or unary signs back into recursive descent: formula
    // text is untrusted input, and a valid bounded record must not consume the
    // decoder stack merely because its nesting is deep.
    #[derive(Debug, Clone, Copy)]
    enum Operator {
        OpenParen,
        Unary(u8),
        Binary(u8),
    }

    impl Operator {
        fn precedence(self) -> u8 {
            match self {
                Self::OpenParen => 0,
                Self::Binary(b'+' | b'-') => 1,
                Self::Binary(b'*' | b'/') => 2,
                Self::Unary(_) => 3,
                Self::Binary(b'^') => 4,
                Self::Binary(_) => 0,
            }
        }

        fn is_right_associative(self) -> bool {
            matches!(self, Self::Unary(_) | Self::Binary(b'^'))
        }
    }

    struct Parser<'a> {
        bytes: &'a [u8],
        at: usize,
        values: Vec<f64>,
        operators: Vec<Operator>,
        expect_operand: bool,
    }

    impl Parser<'_> {
        fn spaces(&mut self) {
            while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
                self.at += 1;
            }
        }

        fn number(&mut self) -> Option<f64> {
            self.spaces();
            let start = self.at;
            while self
                .bytes
                .get(self.at)
                .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'.')
            {
                self.at += 1;
            }
            if self
                .bytes
                .get(self.at)
                .is_some_and(|byte| matches!(byte, b'e' | b'E'))
            {
                self.at += 1;
                if self
                    .bytes
                    .get(self.at)
                    .is_some_and(|byte| matches!(byte, b'+' | b'-'))
                {
                    self.at += 1;
                }
                let exponent = self.at;
                while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                    self.at += 1;
                }
                (self.at > exponent).then_some(())?;
            }
            (self.at > start).then_some(())?;
            let value: f64 = std::str::from_utf8(&self.bytes[start..self.at])
                .ok()?
                .parse()
                .ok()?;
            value.is_finite().then_some(value)
        }

        fn apply_top(&mut self) -> Option<()> {
            let operator = self.operators.pop()?;
            let value = match operator {
                Operator::OpenParen => return None,
                Operator::Unary(operator) => {
                    let value = self.values.pop()?;
                    match operator {
                        b'+' => value,
                        b'-' => -value,
                        _ => return None,
                    }
                }
                Operator::Binary(operator) => {
                    let right = self.values.pop()?;
                    let left = self.values.pop()?;
                    match operator {
                        b'+' => left + right,
                        b'-' => left - right,
                        b'*' => left * right,
                        b'/' => left / right,
                        b'^' => left.powf(right),
                        _ => return None,
                    }
                }
            };
            value.is_finite().then(|| self.values.push(value))
        }

        fn push_binary(&mut self, operator: u8) -> Option<()> {
            let incoming = Operator::Binary(operator);
            while self.operators.last().is_some_and(|top| {
                !matches!(top, Operator::OpenParen)
                    && (top.precedence() > incoming.precedence()
                        || (top.precedence() == incoming.precedence()
                            && !incoming.is_right_associative()))
            }) {
                self.apply_top()?;
            }
            self.operators.push(incoming);
            self.expect_operand = true;
            Some(())
        }

        fn close_group(&mut self) -> Option<()> {
            while self
                .operators
                .last()
                .is_some_and(|operator| !matches!(operator, Operator::OpenParen))
            {
                self.apply_top()?;
            }
            matches!(self.operators.pop(), Some(Operator::OpenParen)).then_some(())
        }

        fn parse(mut self) -> Option<f64> {
            while self.at < self.bytes.len() {
                self.spaces();
                if self.at == self.bytes.len() {
                    break;
                }
                let byte = *self.bytes.get(self.at)?;
                if self.expect_operand {
                    match byte {
                        b'+' | b'-' => {
                            self.operators.push(Operator::Unary(byte));
                            self.at += 1;
                        }
                        b'(' => {
                            self.operators.push(Operator::OpenParen);
                            self.at += 1;
                        }
                        _ => {
                            let value = self.number()?;
                            self.values.push(value);
                            self.expect_operand = false;
                        }
                    }
                } else {
                    match byte {
                        b'+' | b'-' | b'*' | b'/' | b'^' => {
                            self.push_binary(byte)?;
                            self.at += 1;
                        }
                        b')' => {
                            self.close_group()?;
                            self.at += 1;
                        }
                        _ => return None,
                    }
                }
            }
            if self.expect_operand {
                return None;
            }
            while let Some(operator) = self.operators.last() {
                if matches!(operator, Operator::OpenParen) {
                    return None;
                }
                self.apply_top()?;
            }
            (self.values.len() == 1).then(|| self.values[0])
        }
    }

    Parser {
        bytes: text.as_bytes(),
        at: 0,
        values: Vec::new(),
        operators: Vec::new(),
        expect_operand: true,
    }
    .parse()
}

#[cfg(test)]
mod tests;
