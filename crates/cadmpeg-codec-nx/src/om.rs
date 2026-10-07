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
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;

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
mod state_block;
use state_block::{operation_state_block_before_boundary, OperationStateBlock};
pub(crate) mod roll_forward;
pub(crate) mod state_link;
use roll_forward::{
    operation_state_group_at, operation_state_group_end_at, OperationStateGroupTable,
};
pub(crate) mod state_group;
use state_index::OperationStateIndex;
mod state_message_text;
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
    LocatedBinary64, PayloadScalarAtom, RepeatedScalar, ShiftedBinary32, ShiftedBinary64,
    ShiftedScalar,
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
pub(crate) struct EntityRecord<'a> {
    /// Absolute byte offset of the entity payload.
    pub(crate) offset: usize,
    /// Exactly bounded serialized entity payload.
    pub(crate) bytes: &'a [u8],
}

/// One NX object-model entity in a fixed-width object-id table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FixedEntityRecord<'a> {
    /// Object identifier and the absolute offset of its table word.
    pub(crate) object_id: (u32, u64),
    /// Absolute byte offset of the entity payload.
    pub(crate) offset: usize,
    /// Exactly bounded serialized entity payload.
    pub(crate) bytes: &'a [u8],
}

/// How one indexed section stores entity identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IndexedStore<'a> {
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
pub(crate) struct TypeDefinition<'a> {
    /// Absolute byte offset of the definition's length byte.
    pub(crate) offset: usize,
    /// Registered `UGS::` class name.
    pub(crate) name: &'a str,
    /// Complete registry bytes following the class name.
    pub(crate) registry_tail: &'a [u8],
}

/// One member declaration in an NX OM field registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FieldDefinition<'a> {
    /// Offset of the declaration length byte.
    pub(crate) offset: usize,
    /// Registered `m_` member name.
    pub(crate) name: &'a str,
    /// Complete registry bytes following the member name.
    pub(crate) registry_tail: &'a [u8],
}

/// One self-framed printable string value in an NX OM entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StringValue<'a> {
    /// Absolute byte offset of the `66 32 03` marker.
    pub(crate) offset: usize,
    /// Printable value bytes.
    pub(crate) value: PrintableString<&'a str>,
}

/// One canonical UUID in the compact NX OM string frame `03 26, text, 00`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UuidStringValue<'a> {
    /// Absolute byte offset of the `03 26` marker.
    pub(crate) offset: usize,
    /// Canonical lowercase UUID text.
    pub(crate) value: crate::canonical_uuid::CanonicalUuid<&'a str>,
}

/// One self-framed printable string in a surface-referenced payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SurfacePayloadString<'a> {
    /// Payload-relative offset of the `66 1b 03` marker.
    pub(crate) offset: usize,
    /// Exact non-empty string value.
    pub(crate) value: crate::payload_text::PayloadText<&'a str>,
}

/// Self-framed NX product/version marker in an OM store root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoreVersion<'a> {
    /// Absolute offset of the `04 01` marker.
    pub(crate) offset: usize,
    /// Exact printable product/version text, including the `NX ` prefix.
    pub(crate) value: ProductText<&'a str>,
}

/// Header of an internally pointed size-framed OM record area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecordAreaHeader<'a> {
    /// Absolute offset of the first control word.
    pub(crate) offset: usize,
    /// Three little-endian control words preceding the product record.
    pub(crate) control_words: [u32; 3],
    /// Product/version record following the control words.
    pub(crate) product: StoreVersion<'a>,
}

/// One RGB definition from an NX part color table.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ColorTableDefinition<'a> {
    /// Color name paired by table order.
    pub(crate) name: &'a str,
    /// Exact normalized components and their payload offsets.
    pub(crate) components: [(ColorComponent, usize); 3],
    /// Byte offset of the opening `05` marker.
    pub(crate) offset: usize,
}

/// Complete 216-entry NX part color table.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ColorTable<'a> {
    /// Byte offset of the counted name roster.
    pub(crate) offset: usize,
    /// Exact background components and their payload offsets.
    pub(crate) background: [(ColorComponent, usize); 3],
    /// Ordered definitions for color indices 1 through 216.
    pub(crate) definitions: [ColorTableDefinition<'a>; PALETTE_SIZE],
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

fn color_name_frame<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    offset: usize,
) -> Result<Option<(&'a str, usize)>, CodecError> {
    (|| {
        let byte_len = usize::from(bytes.get(offset).copied()?);
        if byte_len < 2 {
            return None;
        }
        let end = offset.checked_add(byte_len)?;
        let text = bytes.get(offset + 1..end)?.strip_suffix(&[0])?;
        if text.is_empty()
            || !propagate_resource!(ctx
                .admit_iter(text, "NX color name syntax")
                .map_err(CodecError::from))
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
        {
            return None;
        }
        Some(Ok((
            propagate_resource!(ctx.validate_utf8(text, "NX color name UTF-8 validation")).ok()?,
            byte_len,
        )))
    })()
    .transpose()
}

const COLOR_TABLE_NAME_HEADER: [u8; 4] = [0x02, 0x80, 0xd9, 0x01];
const COLOR_TABLE_DEFINITION_PREAMBLE: [u8; 21] = [
    0x02, 0x14, 0xff, 0x06, 0x00, 0xf0, 0x02, 0x80, 0x9d, 0x80, 0xc7, 0x00, 0xc0, 0x13, 0x0a, 0xc6,
    0x01, 0x80, 0xd9, 0x80, 0xc8,
];

// Validate a complete palette through borrowed names, component widths, and
// index tokens. The owning color vectors are materialized only after this
// self-framed candidate has passed every check.
fn color_table_end(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
) -> Result<Option<usize>, CodecError> {
    (|| {
        if bytes.get(start..start + COLOR_TABLE_NAME_HEADER.len()) != Some(&COLOR_TABLE_NAME_HEADER)
        {
            return None;
        }
        let mut at = start + COLOR_TABLE_NAME_HEADER.len();
        for ordinal in 0..=216 {
            let (name, width) = propagate_resource!(color_name_frame(ctx, bytes, at))?;
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
            if !propagate_resource!(ctx.equal(
                &(bytes.get(at..at + width)),
                &(Some(&token[..width])),
                "NX color table end equality"
            )) {
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
        Some(Ok(at))
    })()
    .transpose()
}

fn color_table_at<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    start: usize,
) -> Result<Option<ColorTable<'a>>, CodecError> {
    (|| {
        let mut at = start + COLOR_TABLE_NAME_HEADER.len();
        let mut names = [""; 217];
        for slot in &mut names {
            let (name, width) = propagate_resource!(color_name_frame(ctx, bytes, at))?;
            *slot = name;
            at += width;
        }
        at += COLOR_TABLE_DEFINITION_PREAMBLE.len();
        let background = color_components(bytes, &mut at)?;
        let mut definitions = std::array::from_fn(|_| ColorTableDefinition {
            name: "",
            components: background,
            offset: start,
        });
        for color_index in PaletteIndex::all() {
            let offset = at;
            at += 1 + color_index.definition_token().1 + 3;
            let components = color_components(bytes, &mut at)?;
            definitions[usize::from(color_index.value()) - 1] = ColorTableDefinition {
                name: names[usize::from(color_index.value())],
                components,
                offset,
            };
        }
        Some(Ok(ColorTable {
            offset: start,
            background,
            definitions,
        }))
    })()
    .transpose()
}

/// Decode every complete NX part color table in a bounded byte region.
pub(crate) fn color_tables<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
) -> Result<Vec<ColorTable<'a>>, CodecError> {
    let mut tables = Vec::new();
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan NX part color tables",
    )?;
    let mut start = 0;
    while start + COLOR_TABLE_NAME_HEADER.len() <= bytes.len() {
        if bytes.get(start..start + COLOR_TABLE_NAME_HEADER.len()) != Some(&COLOR_TABLE_NAME_HEADER)
        {
            start += 1;
            continue;
        }
        let Some(end) = color_table_end(ctx, bytes, start)? else {
            start += 1;
            continue;
        };
        if let Some(table) = color_table_at(ctx, bytes, start)? {
            ctx.reserve_vec(&mut tables, 1, "nx part color tables")?;
            tables.push(table);
        }
        start = end;
    }
    Ok(tables)
}

/// One exact shifted-IEEE scalar field in a reconstructed construction payload.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ConstructionPayloadScalarField {
    /// Payload-relative offset of the `50 59 66` marker.
    pub(crate) offset: usize,
    /// Serialized field discriminator following the marker.
    pub(crate) field_code: u8,
    /// Checked shifted-binary64 atom.
    pub(crate) scalar: ShiftedBinary64,
}

const SHIFTED_BINARY64_SCALAR_FRAME_LEN: usize = 13;

/// Decode exact `50 59 66, field_code, 00, shifted-f64` construction fields.
pub(crate) fn construction_payload_scalar_fields(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<ConstructionPayloadScalarField>, CodecError> {
    let mut fields = Vec::new();
    if let Some(range_end) = bytes.len().checked_sub(12) {
        for start in ctx.admit_iter(&(0..range_end), "scan NX construction scalars")? {
            if bytes.get(start..start + 3) != Some(b"PYf") || bytes.get(start + 4) != Some(&0x00) {
                continue;
            }
            let Some(scalar) = bytes
                .get(start + 5..start + SHIFTED_BINARY64_SCALAR_FRAME_LEN)
                .and_then(ShiftedBinary64::read)
            else {
                continue;
            };
            ctx.reserve_vec(&mut fields, 1, "NX construction scalar fields")?;
            fields.push(ConstructionPayloadScalarField {
                offset: start,
                field_code: bytes[start + 3],
                scalar,
            });
        }
    }
    Ok(fields)
}

/// Exact type-free named point record spanning consecutive store blocks.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OffsetStoreNamedPoint {
    /// Exact `Point<positive decimal>` name.
    pub(crate) name: String,
    /// Two checked scalar atoms and their frame offsets in block order.
    pub(crate) values: [LocatedBinary64; 2],
    /// Minimal number of consecutive blocks containing both scalar frames.
    pub(crate) block_count: usize,
}

/// Decode a named two-scalar point from a streaming block sequence.
pub(crate) fn offset_store_named_point<'a>(
    ctx: &DecodeContext<'_>,
    blocks: impl IntoIterator<Item = &'a [u8]>,
) -> Result<Option<OffsetStoreNamedPoint>, CodecError> {
    let mut bytes = Vec::new();
    let mut byte_reservation = None;
    let mut candidate = None;
    for (block_ordinal, block) in blocks.into_iter().enumerate() {
        // A later type-free name starts the next bounded data-block object.
        if !bytes.is_empty()
            && name_field::scan(ctx, block)?
                .first()
                .is_some_and(|name| name.code().is_none())
        {
            return Ok(candidate);
        }
        let length = bytes.len().checked_add(block.len()).ok_or_else(|| {
            ctx.refuse_codec_limit("NX named point block bytes", u64::MAX, u64::MAX)
        })?;
        drop(byte_reservation.take());
        byte_reservation = Some(ctx.reserve_scoped(
            cadmpeg_core::decode::u64_from_index(length),
            "NX named point block bytes",
        )?);
        ctx.reserve_vec(&mut bytes, block.len(), "NX named point block bytes")?;
        bytes.extend_from_slice(block);
        let names = name_field::scan(ctx, &bytes)?;
        let Some(name) = names.first() else {
            return Ok(None);
        };
        if name.code().is_some()
            || parse_positive_decimal_suffix(ctx, name.value(), "Point")?.is_none()
        {
            return Ok(None);
        }
        let next_name = ctx
            .admit_iter(&names, "NX named point following name lookup")?
            .find(|next| next.code().is_some() && next.offset() > name.offset());
        let interval_end = next_name.map_or(bytes.len(), name_field::NameField::offset);
        let mut scalars = construction_payload_scalar_fields(ctx, &bytes)?
            .into_iter()
            .filter(|scalar| {
                scalar.offset > name.offset()
                    && scalar
                        .offset
                        .checked_add(SHIFTED_BINARY64_SCALAR_FRAME_LEN)
                        .is_some_and(|end| end <= interval_end)
            });
        match (scalars.next(), scalars.next(), scalars.next()) {
            (Some(first_scalar), Some(second_scalar), None) => {
                if candidate.is_none() {
                    let value = name.value();
                    let mut owned = ctx.retained_string(value.len(), "NX named point name")?;
                    ctx.append_retained(&mut owned, value, "NX admitted text append")?;
                    candidate = Some(OffsetStoreNamedPoint {
                        name: owned,
                        values: [first_scalar, second_scalar].map(|field| LocatedBinary64 {
                            scalar: field.scalar,
                            offset: field.offset,
                        }),
                        block_count: block_ordinal + 1,
                    });
                }
            }
            (None, _, _) | (Some(_), None, _) => {}
            _ => return Ok(None),
        }
        if next_name.is_some() {
            return Ok(candidate);
        }
    }
    Ok(candidate)
}

fn parse_positive_decimal_suffix(
    ctx: &DecodeContext<'_>,
    value: &str,
    prefix: &str,
) -> Result<Option<u32>, CodecError> {
    let Some(suffix) = value.strip_prefix(prefix) else {
        return Ok(None);
    };
    if suffix.is_empty()
        || !ctx
            .admit_iter(suffix, "NX point ordinal digits")?
            .all(|ch| ch.is_ascii_digit())
    {
        return Ok(None);
    }
    let Ok(ordinal) = ctx.parse_text::<u32>(suffix, "NX point ordinal decimal parse")? else {
        return Ok(None);
    };
    Ok((ordinal != 0).then_some(ordinal))
}

/// Unit declared by an NX numeric-expression serialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExpressionUnit {
    /// Model length in millimeters as serialized by NX.
    Millimeter,
    /// Model length in inches as serialized by NX.
    Inch,
    /// Angular value in degrees as serialized by NX.
    Degree,
    /// Unit label without a neutral dimensional mapping.
    Native(String),
}
impl cadmpeg_core::decode::cost::DecodeCost for ExpressionUnit {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        match self {
            Self::Native(text) => (1_u8, text).decode_cost(ctx, operation),
            Self::Millimeter | Self::Inch | Self::Degree => Ok(1),
        }
    }
}

/// One numeric expression decoded from an exactly bounded OM entity.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NumericExpression<'a> {
    /// Persistent identity of the containing OM entity, when indexed.
    pub(crate) object_id: Option<u32>,
    /// Absolute byte offset of the expression text.
    pub(crate) offset: usize,
    /// NX parameter name.
    pub(crate) name: ParameterName<&'a str>,
    /// Declared native unit.
    pub(crate) unit: ExpressionUnit,
    /// Exact expression text following the serialized name separator.
    pub(crate) expression: &'a str,
}

impl NumericExpression<'_> {
    /// Finite value when the expression is context-free arithmetic.
    pub(crate) fn constant_value(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<FiniteReal>, CodecError> {
        evaluate_constant_expression(ctx, self.expression)
    }
}

/// One validated external entity-index/object-id-table pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexedSection<'a> {
    /// Self-anchored base used by every entity-index offset.
    pub(crate) base: usize,
    /// Absolute offset of the entity-index array.
    pub(crate) entity_index_offset: usize,
    /// Absolute offset of the object-id table or offset-only identity metadata.
    pub(crate) object_id_table_offset: usize,
    /// Length-framed class definitions preceding the entity index.
    pub(crate) types: Arc<[TypeDefinition<'a>]>,
    /// Length-framed member definitions preceding the entity index.
    pub(crate) fields: Arc<[FieldDefinition<'a>]>,
    /// Identity store used by this section.
    pub(crate) store: IndexedStore<'a>,
}

/// Internally pointed record-area bytes with their absolute offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RecordArea<'a> {
    /// Absolute offset of the record-area start.
    pub(crate) offset: usize,
    /// Exact record-area bytes, including the 12-byte control prefix.
    pub(crate) bytes: &'a [u8],
}

/// One size-framed NX object-model section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Section<'a> {
    /// Offset of the `ff ff ff ff` section signature.
    pub(crate) offset: usize,
    /// Complete section length including its 16-byte header.
    byte_len: usize,
    /// Class declarations in the section's contiguous type registry.
    pub(crate) types: Arc<[TypeDefinition<'a>]>,
    /// Member declarations in the section's field registry.
    pub(crate) fields: Arc<[FieldDefinition<'a>]>,
    /// Internally pointed record area, when the section carries one.
    pub(crate) record_area: Option<RecordArea<'a>>,
    /// Operation labels decoded while the section's record area is framed.
    cached_operation_labels: Arc<[OperationLabel<'a>]>,
}

/// A feature operation name in a size-framed feature-history record area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperationLabel<'a> {
    /// Complete operation header and its exact reference encodings.
    pub(crate) header: OperationHeader,
    /// Printable operation name without its terminating NUL.
    pub(crate) value: &'a str,
}

/// One unlabeled operation record bounded by validated operation headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UnlabeledOperationRecord<'a> {
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
pub(crate) struct OperationTerminalFrame {
    pub(crate) immediate_common_frame_offset: Option<usize>,
    pub(crate) frame: TerminalFrame<usize>,
}

/// One length-framed UTF-8 string in a bounded operation payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperationPayloadString<'a> {
    /// Absolute offset of the `04` marker.
    pub(crate) offset: usize,
    /// Exact non-empty string value.
    pub(crate) value: crate::payload_text::PayloadText<&'a str>,
}

/// Marker selecting a bounded operation text frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperationTextMarker {
    Text,
    String,
}

/// One length-framed UTF-8 text frame in a bounded operation payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperationPayloadTextFrame<'a> {
    /// Marker selecting the payload text-frame family.
    pub(crate) marker: OperationTextMarker,
    /// Absolute offset of the marker.
    pub(crate) offset: usize,
    /// Exact non-empty text value.
    pub(crate) value: crate::payload_text::PayloadText<&'a str>,
}

/// One canonical variable-width object index in an operation payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PayloadObjectReference<T = ReferenceIndexToken, O = usize> {
    /// Absolute offset of the width marker.
    pub(crate) offset: O,
    /// Checked token retaining the exact marker and width.
    pub(crate) token: T,
}

/// One exact counted transform lane in a pattern operation payload.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PatternPayloadTransformLane {
    /// Absolute offset of the opening `01, count` field.
    pub(crate) offset: usize,
    /// Schema index framing every row in the lane.
    pub(crate) row_schema_index: NonZeroU8,
    pub(crate) rows: PatternRows<LocatedCompactIndex, usize>,
}

/// Exact counted instance-output lane in a multi-instance operation payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MultiInstanceOutputPayloadLane {
    /// Absolute offset of the opening `25 01, count` field.
    pub(crate) offset: usize,
    /// Complete selector groups and their trailing references.
    pub(crate) outputs: instances::MultiInstanceOutputs<usize>,
}

/// Count schema index with room for the three consecutive selector-row indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IdenticalInstanceSchemaIndex(u8);

impl IdenticalInstanceSchemaIndex {
    pub(crate) fn new(value: u8) -> Option<Self> {
        value.checked_add(3).map(|_| Self(value))
    }

    pub(crate) fn value(self) -> u8 {
        self.0
    }

    pub(crate) fn row_indices(self) -> [u8; 3] {
        [self.0 + 1, self.0 + 2, self.0 + 3]
    }
}

/// Exact counted selector lane in an identical-instance output payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IdenticalInstanceOutputPayloadLane {
    /// Absolute offset of the leading schema index.
    pub(crate) offset: usize,
    /// Schema index preceding the count field.
    pub(crate) leading_schema_index: u8,
    /// Schema index framing the serialized count.
    pub(crate) count_schema_index: IdenticalInstanceSchemaIndex,
    /// Ordered non-null compact selectors with their exact source tokens.
    pub(crate) selectors: compact::CountedIndexMembers<LocatedCompactIndex>,
}

/// Exact construction header in a point-feature payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PointFeaturePayloadHeader {
    /// Construction object referenced by the header.
    pub(crate) reference: PayloadObjectReference,
    /// Serialized header mode.
    pub(crate) mode: discriminators::PointHeaderMode,
}

/// Exact six-scalar lane selected by a point-feature construction header.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PointFeatureScalarLane {
    /// Six checked shifted-binary64 atoms in byte order.
    pub(crate) values: [ShiftedBinary64; 6],
    /// Start of the contiguous lane in the joined blocks.
    offset: usize,
}

impl PointFeatureScalarLane {
    pub(crate) fn value_offsets(&self) -> [usize; 6] {
        std::array::from_fn(|i| self.offset + i * 8)
    }
}

/// Exact leading construction branch in a `SWP104` payload.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Swp104PayloadLeadingBranch {
    /// Nonzero construction discriminator at the payload start.
    pub(crate) discriminator: NonZeroU8,
    /// Four finite shifted-binary64 values in serialized order.
    pub(crate) scalars: [ShiftedBinary64; 4],
    /// Whether one zero byte precedes the branch mode.
    pub(crate) leading_zero: bool,
    /// Serialized nonzero branch mode.
    pub(crate) mode: NonZeroU8,
    /// Exact state lane preceding the terminal marker.
    pub(crate) state_lane: Swp104StateLane,
    /// Ordered nonterminal references.
    pub(crate) members: BranchItems<reference_index::PayloadIndexToken>,
    /// Terminal reference.
    pub(crate) terminal: reference_index::PayloadIndexToken,
}

impl Swp104PayloadLeadingBranch {
    pub(crate) fn byte_len(&self, ctx: &DecodeContext<'_>) -> Result<usize, CodecError> {
        let prefix = 40 + usize::from(self.leading_zero);
        let width = ctx
            .admit_iter(self.members.as_slice(), "NX SWP104 member token widths")?
            .try_fold(prefix, |width, token| width.checked_add(token.raw().len()))
            .and_then(|width| width.checked_add(self.state_lane.byte_len()))
            .and_then(|width| width.checked_add(3))
            .and_then(|width| width.checked_add(self.terminal.raw().len()))
            .and_then(|width| width.checked_add(1));
        width.ok_or_else(|| ctx.refuse_codec_limit("NX SWP104 branch extent", u64::MAX, u64::MAX))
    }
}

/// Exact pair of scaled shifted-binary64 atoms in a reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SketchPayloadFixedPair {
    /// Payload-relative offset of the discriminator.
    pub(crate) offset: usize,
    /// Ordered values reconstructed from the `30` shifted-binary64 atoms and scaled by `1/4`.
    pub(crate) values: [SketchScaledAtom; 2],
    /// Exact discriminator and branch prefix selecting the pair layout.
    pub(crate) form: SketchPairForm,
}

impl SketchPayloadFixedPair {
    fn discriminator(&self) -> &'static [u8] {
        self.form.discriminator()
    }
    pub(crate) fn value_offsets(&self) -> [usize; 2] {
        let first = self.offset + self.discriminator().len();
        [first, first + 8 + self.form.separator_width()]
    }
}

/// Exact mixed scaled shifted-binary64 and shifted-binary32 pair in a sketch payload.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SketchPayloadMixedPair {
    /// Payload-relative offset of the discriminator.
    pub(crate) offset: usize,
    /// Exact scaled binary64 and binary32 atoms.
    pub(crate) scalars: SketchMixedScalars,
}

impl SketchPayloadMixedPair {
    pub(crate) fn value_offsets(&self) -> [usize; 2] {
        let first = self.offset + SketchPairForm::Legacy.discriminator().len();
        [first, first + 8 + 1]
    }
}

/// Exact pair of signed Q1.55 atoms following a datum-CSYS branch discriminator.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DatumCsysPayloadFixedPair {
    /// Payload-relative offset of the discriminator.
    pub(crate) offset: usize,
    /// Ordered dimensionless Q1.55 values.
    pub(crate) values: [Q155; 2],
    /// Exact discriminator selecting the pair branch.
    pub(crate) form: DatumPairForm,
}

impl DatumCsysPayloadFixedPair {
    fn discriminator(&self) -> &'static [u8] {
        self.form.discriminator()
    }
    pub(crate) fn value_offsets(&self) -> [usize; 2] {
        let first = self.offset + self.discriminator().len();
        [first, first + 8 + 1]
    }
}

/// Fixed scalar header in one bounded extrusion payload.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ExtrudePayloadHeader {
    /// Absolute offset of the first shifted-IEEE scalar.
    pub(crate) offset: usize,
    /// Ordered finite scalar values.
    pub(crate) scalars: [ShiftedBinary64; 2],
}

/// Four construction-block references carried by a `HOLE PACKAGE` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HolePackageConstructionGroupLane {
    /// Payload-relative offset of the fixed lane prefix.
    pub(crate) offset: usize,
    /// Compact selector preceding the repeated branch byte.
    pub(crate) selector: NonZeroU8,
    /// Branch byte repeated between the two reference pairs.
    pub(crate) branch: NonZeroU8,
    /// Ordered first and second construction-block pairs.
    pub(crate) references: [PayloadObjectReference; 4],
}

/// One wrapped member index in a branch-`11` operation body clause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OperationBodyMemberGroup {
    /// Zero-based body-reference occurrence order.
    pub(crate) body_reference_ordinal: u32,
    /// Serialized body object index.
    pub(crate) body_object_index: u32,
    /// Ordered compact indices and their absolute source positions.
    pub(crate) members: Vec<LocatedCompactIndex>,
}

/// Exact continuation following a `TRIM BODY` branch-`11` member lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OperationBody11Continuation {
    /// Zero-based body-reference occurrence order.
    pub(crate) body_reference_ordinal: u32,
    /// Serialized body object index.
    pub(crate) body_object_index: u32,
    /// Exact compact continuation index and its absolute source offset.
    pub(crate) continuation: LocatedCompactIndex,
    /// Exact required terminal reference and its absolute source offset.
    pub(crate) terminal: PayloadObjectReference,
}

/// Homogeneous checked references in an operation body lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OperationBodyReferenceLaneValues {
    CompactIndex(Vec<LocatedCompactIndex>),
    PayloadObjectIndex(Vec<PayloadObjectReference<reference_index::PayloadIndexToken>>),
}

/// Counted reference lane following an operation body scalar clause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OperationBodyReferenceLane {
    /// Zero-based body-reference occurrence order.
    pub(crate) body_reference_ordinal: u32,
    /// Serialized body object index.
    pub(crate) body_object_index: u32,
    /// Branch discriminator following the body-reference terminator.
    pub(crate) branch: discriminators::OperationBodyReferenceBranch,
    /// Ordered non-null lane values with their encoding.
    pub(crate) values: OperationBodyReferenceLaneValues,
}

/// Self-framed NX parameter name in one bounded expression declaration record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExpressionDeclarationName<'a> {
    /// Byte offset of the `04` marker within the containing byte range.
    pub(crate) offset: usize,
    /// Exact `p<decimal>[_qualifier]` name.
    pub(crate) name: ParameterName<&'a str, u32>,
    /// Independently framed numeric literal in the declaration record.
    pub(crate) literal: Option<&'a str>,
}

/// Primary body-object reference carried by one bounded operation record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OperationBodyReference {
    /// Absolute offset of the object-index token.
    pub(crate) offset: usize,
    /// Referenced body object index.
    pub(crate) object_index: reference_index::FeatureReferenceToken,
}

/// Object-index reference in one bounded offset-only OM data block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DataBlockObjectReference {
    /// Byte offset of the object-index token within the containing byte range.
    pub(crate) offset: usize,
    /// Required feature index with its exact encoding.
    pub(crate) object_index: reference_index::FeatureReferenceToken,
}

/// Boolean operation kind stored after an operation label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BooleanOperationKind {
    /// Add tool bodies to the target.
    Unite,
    /// Remove tool bodies from the target.
    Subtract,
    /// Retain target/tool intersections.
    Intersect,
}

/// One feature-history Boolean with object-index operands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BooleanOperation {
    /// Absolute offset of the operation label tag.
    pub(crate) offset: usize,
    /// Boolean operation kind.
    pub(crate) kind: BooleanOperationKind,
    /// Target body reference and its exact source token.
    pub(crate) target: PayloadObjectReference,
    /// Ordered tool body references and their exact source tokens.
    pub(crate) tools: Vec<PayloadObjectReference>,
}

impl<'a> IndexedSection<'a> {
    /// Return the section base used by its external record offsets.
    pub(crate) const fn base_offset(&self) -> usize {
        self.base
    }

    /// Fixed-width object-id records, when this section is that store.
    pub(crate) fn as_fixed(&self) -> Option<&[FixedEntityRecord<'a>]> {
        match &self.store {
            IndexedStore::Fixed { records } => Some(records.as_ref()),
            IndexedStore::OffsetOnly { .. } => None,
        }
    }

    /// Control block, column storage, and records of an offset-only store.
    pub(crate) fn as_offset_only(
        &self,
    ) -> Option<(&EntityRecord<'a>, &'a [u8], &[EntityRecord<'a>])> {
        match &self.store {
            IndexedStore::OffsetOnly {
                control,
                column_storage,
                records,
            } => Some((control, column_storage, records.as_ref())),
            IndexedStore::Fixed { .. } => None,
        }
    }

    /// Decode explicit numeric-expression text within bounded entity records.
    pub(crate) fn numeric_expressions(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<NumericExpression<'a>>, CodecError> {
        let records = self.numeric_expression_records(ctx)?;
        let mut expressions = Vec::new();
        for (_, expression) in records {
            ctx.reserve_vec(&mut expressions, 1, "nx indexed numeric expressions")?;
            expressions.push(expression);
        }
        Ok(expressions)
    }

    /// Decode expressions together with their owning record ordinal.
    pub(crate) fn numeric_expression_records(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<(usize, NumericExpression<'a>)>, CodecError> {
        let mut temporary = ctx.reserve_scoped(0, "nx indexed numeric record scan")?;
        let mut records: Vec<(usize, &'a [u8], Option<u32>)> = Vec::new();
        match &self.store {
            IndexedStore::Fixed { records: source } => {
                for record in ctx.admit_iter(source.as_ref(), "NX indexed numeric record scan")? {
                    ctx.reserve_scoped_vec(
                        &mut temporary,
                        &mut records,
                        1,
                        "nx indexed numeric record scan",
                    )?;
                    records.push((record.offset, record.bytes, Some(record.object_id.0)));
                }
            }
            IndexedStore::OffsetOnly {
                records: source, ..
            } => {
                for record in ctx.admit_iter(source.as_ref(), "NX indexed numeric record scan")? {
                    ctx.reserve_scoped_vec(
                        &mut temporary,
                        &mut records,
                        1,
                        "nx indexed numeric record scan",
                    )?;
                    records.push((record.offset, record.bytes, None));
                }
            }
        }
        let mut has_host_globals = false;
        for (_, bytes, _) in ctx.admit_iter(&records, "NX numeric expression owner records")? {
            if ctx
                .admit_iter(*bytes, "NX numeric expression host globals marker")?
                .enumerate()
                .skip(b"hostglobalvariables".len() - 1)
                .any(|(end, _)| {
                    &bytes[end + 1 - b"hostglobalvariables".len()..=end] == b"hostglobalvariables"
                })
            {
                has_host_globals = true;
                break;
            }
        }
        if !has_host_globals {
            return Ok(Vec::new());
        }
        let mut expressions = Vec::new();
        for (record_ordinal, (offset, bytes, object_id)) in ctx
            .admit_iter(&records, "NX numeric expression record traversal")?
            .copied()
            .enumerate()
        {
            if let Some(expression) = numeric_expression_at(ctx, bytes, offset, object_id)? {
                ctx.reserve_vec(&mut expressions, 1, "nx indexed numeric expression records")?;
                expressions.push((record_ordinal, expression));
            }
        }
        Ok(expressions)
    }
}

impl<'a> FixedEntityRecord<'a> {
    /// Decode every strictly framed printable string in this bounded record.
    pub(crate) fn string_values(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<StringValue<'a>>, CodecError> {
        string_values(ctx, self.bytes, self.offset)
    }

    /// Decode tagged references within this fixed-record table.
    pub(crate) fn references(
        &self,
        ctx: &DecodeContext<'_>,
        record_count: usize,
    ) -> Result<Vec<LocatedReference<RecordReference<()>>>, CodecError> {
        let direct = record_references(ctx, self.bytes, self.offset)?;
        let counted = counted_record_references(ctx, self.bytes, self.offset, record_count)?;
        let mut references = Vec::new();
        for reference in ctx
            .admit_iter(&direct, "NX direct fixed entity references")?
            .copied()
        {
            ctx.reserve_vec(&mut references, 1, "NX fixed entity references")?;
            references.push(LocatedReference {
                offset: reference.offset,
                value: RecordReference::Direct(reference.value),
            });
        }
        for reference in ctx
            .admit_iter(&counted, "NX counted fixed entity references")?
            .copied()
        {
            ctx.reserve_vec(&mut references, 1, "NX fixed entity references")?;
            references.push(LocatedReference {
                offset: reference.offset,
                value: RecordReference::RecordOrdinal16 {
                    ordinal: reference.value,
                    target: (),
                },
            });
        }
        ctx.stable_sort_by(
            &mut references,
            |value| &value.offset,
            Ord::cmp,
            "sort NX record references",
        )?;
        Ok(references)
    }
}

impl<'a> Section<'a> {
    fn record_area_parts(&self) -> Option<(usize, &'a [u8])> {
        self.record_area.map(|area| (area.offset, area.bytes))
    }

    /// Decode the validated record-area control and product header.
    pub(crate) fn record_area_header(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<RecordAreaHeader<'a>>, CodecError> {
        (|| {
            let (offset, bytes) = self.record_area_parts()?;
            let control_words = [
                View::u32_le_at(bytes, 0)?,
                View::u32_le_at(bytes, 4)?,
                View::u32_le_at(bytes, 8)?,
            ];
            let suffix = bytes.get(12..)?;
            let layout = match propagate_resource!(ProductRecord::read(
                ctx,
                suffix,
                ProductRecordForm::Modern
            )) {
                Some(layout) => layout,
                None => propagate_resource!(ProductRecord::read(
                    ctx,
                    suffix,
                    ProductRecordForm::LegacyFeature
                ))?,
            };
            Some(Ok(RecordAreaHeader {
                offset,
                control_words,
                product: StoreVersion {
                    offset: offset + 12,
                    value: layout.text(),
                },
            }))
        })()
        .transpose()
    }

    /// Decode strictly framed operation labels for parser/cache tests.
    #[cfg(test)]
    pub(crate) fn operation_labels(&self) -> Vec<OperationLabel<'a>> {
        self.cached_operation_labels.to_vec()
    }

    /// Decode fully framed Boolean operations from the pointed record area.
    pub(crate) fn boolean_operations(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<BooleanOperation>, CodecError> {
        let Some((base_offset, bytes)) = self.record_area_parts() else {
            return Ok(Vec::new());
        };
        boolean_operations_with_labels(ctx, bytes, base_offset, &self.cached_operation_labels)
    }

    /// Bound operation records and retain their ordinal in the complete label sequence.
    pub(crate) fn operation_records_with_label_ordinals(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<(usize, OperationRecord<'a>)>, CodecError> {
        let Some((base_offset, bytes)) = self.record_area_parts() else {
            return Ok(Vec::new());
        };
        operation_records_with_labels_and_ordinals(
            ctx,
            bytes,
            base_offset,
            &self.cached_operation_labels,
        )
    }

    /// Bound validated operation headers that have no complete label frame.
    pub(crate) fn unlabeled_operation_records_with_ordinals(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<(usize, UnlabeledOperationRecord<'a>)>, CodecError> {
        let Some((base_offset, bytes)) = self.record_area_parts() else {
            return Ok(Vec::new());
        };
        unlabeled_operation_records_with_ordinals(
            ctx,
            bytes,
            base_offset,
            &self.cached_operation_labels,
        )
    }

    /// Decode the bounded state-counter map of a feature-history record area.
    ///
    /// The section-role check is intentional. The same byte patterns occur in
    /// ordinary model-store payloads, where they do not carry operation state.
    pub(crate) fn operation_state_counter_map(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<StateCounterMap>, CodecError> {
        let is_feature_history = ctx
            .admit_iter(self.types.as_ref(), "NX section types role scan")?
            .any(|definition| definition.name == "UGS::FEATURE_RECORD")
            || ctx
                .admit_iter(self.fields.as_ref(), "NX section fields role scan")?
                .any(|definition| definition.name == "m_rollForwardStates");
        if !is_feature_history {
            return Ok(None);
        }
        let Some((base_offset, bytes)) = self.record_area_parts() else {
            return Ok(None);
        };
        StateCounterMap::read(ctx, bytes, base_offset)
    }

    /// Decode the field-declared `m_rollForwardStates` group table before the
    /// bounded operation-state counter map.
    pub(crate) fn operation_state_group_table(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<OperationStateGroupTable>, CodecError> {
        if !ctx
            .admit_iter(self.fields.as_ref(), "NX section fields role scan")?
            .any(|definition| definition.name == "m_rollForwardStates")
        {
            return Ok(None);
        }
        let Some(map) = self.operation_state_counter_map(ctx)? else {
            return Ok(None);
        };
        let Some((base_offset, bytes)) = self.record_area_parts() else {
            return Ok(None);
        };
        let Some(map_start) = map.offset().checked_sub(base_offset) else {
            return Ok(None);
        };
        operation_state_group_table_before_counter_map(ctx, bytes, map_start, base_offset)
    }

    /// Decode anchored state-journal groups from a feature-history record area.
    ///
    /// The journal prefix is selected by the record-area marker and its
    /// version-token run. After a complete group, only the exact `04 00`
    /// separator form may be skipped when it leads to another complete group.
    /// Any other byte stops the journal so later record-region data cannot
    /// become state.
    pub(crate) fn operation_state_journal_groups(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<Vec<JournalGroup<usize>>>, CodecError> {
        let is_feature_history = ctx
            .admit_iter(self.types.as_ref(), "NX section types role scan")?
            .any(|definition| definition.name == "UGS::FEATURE_RECORD");
        if !is_feature_history {
            return Ok(None);
        }
        let Some((base_offset, bytes, start, end)) = (|| {
            let (base_offset, bytes) = self.record_area_parts()?;
            let header = propagate_resource!(self.record_area_header(ctx))?;
            let product = header.product.offset.checked_sub(base_offset)?;
            let product_end = propagate_resource!(record_area_product_end(ctx, bytes, product))?;
            let start =
                propagate_resource!(operation_state_journal_start(ctx, bytes, product_end))?;
            let end = self
                .cached_operation_labels
                .first()
                .and_then(|label| label.header.offset().checked_sub(base_offset))
                .unwrap_or(bytes.len());
            Some(Ok((base_offset, bytes, start, end)))
        })()
        .transpose()?
        else {
            return Ok(None);
        };
        operation_state_journal_groups_before_boundary(ctx, bytes, start, end, base_offset)
    }

    fn operation_state_block(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<OperationStateBlock<'a>>, CodecError> {
        let Some(map) = self.operation_state_counter_map(ctx)? else {
            return Ok(None);
        };
        let Some((base_offset, bytes)) = self.record_area_parts() else {
            return Ok(None);
        };
        let Some((_, last_record)) = ctx
            .admit_iter(
                &self.operation_records_with_label_ordinals(ctx)?,
                "NX last operation record",
            )?
            .last()
            .copied()
        else {
            return Ok(None);
        };
        let start_offset = last_record.payload_offset();
        let Some(start) = start_offset.checked_sub(base_offset) else {
            return Ok(None);
        };
        let group = self.operation_state_group_table(ctx)?;
        let Some(terminal) = group
            .as_ref()
            .map_or(map.offset(), OperationStateGroupTable::offset)
            .checked_sub(base_offset)
        else {
            return Ok(None);
        };
        let mut ends = ctx.vector_storage(2, "NX operation state boundaries")?;
        if let Some(table) = &group {
            let Some(overlap_end) =
                terminal.checked_add(table.groups().first().opener().bytes().len())
            else {
                return Ok(None);
            };
            ends.push(overlap_end);
        }
        ends.push(terminal);
        for end in ctx
            .admit_iter(&ends, "NX operation state boundaries")?
            .copied()
        {
            if let Some(block) =
                operation_state_block_before_boundary(ctx, bytes, start, end, base_offset)?
            {
                return Ok(Some(block));
            }
        }
        Ok(None)
    }

    /// Decode the bounded per-object status lane after the operation records.
    pub(crate) fn operation_state_status_table(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<OperationStateStatusTable<'a>>, CodecError> {
        Ok(self
            .operation_state_block(ctx)?
            .map(|block| block.into_status_table(ctx))
            .transpose()?
            .flatten())
    }

    /// Decode the contiguous standalone message records immediately before
    /// the roll-forward table or counter-map boundary.
    pub(crate) fn operation_state_messages(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<Vec<OperationStateMessage<'a>>>, CodecError> {
        let Some(block) = self.operation_state_block(ctx)? else {
            return Ok(None);
        };
        block.into_messages(ctx)
    }

    /// Decode complete rows in an audit-trail record area.
    ///
    /// The registry role check prevents the same compact byte patterns in
    /// feature-history and model areas from being interpreted as audit data.
    /// Unknown bytes before, between, and after complete rows remain outside
    /// this typed view.
    pub(crate) fn audit_trail_rows(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<Vec<AuditTrailRow>>, CodecError> {
        let has_audit_marker = ctx
            .admit_iter(self.types.as_ref(), "NX section types role scan")?
            .any(|definition| definition.name == "UGS::OM::SaveAuditTrail");
        let has_specialized_marker = ctx
            .admit_iter(self.types.as_ref(), "NX section types role scan")?
            .any(|definition| {
                matches!(
                    definition.name,
                    "UGS::FEATURE_RECORD" | "UGS::EXP_expression" | "UGS::Solid::Topol"
                )
            });
        if !has_audit_marker || has_specialized_marker {
            return Ok(None);
        }
        let Some((base_offset, bytes, start)) = (|| {
            let (base_offset, bytes) = self.record_area_parts()?;
            let header = propagate_resource!(self.record_area_header(ctx))?;
            let product = header.product.offset.checked_sub(base_offset)?;
            let product_end = propagate_resource!(record_area_product_end(ctx, bytes, product))?;
            let tail = bytes.get(product_end..)?;
            let start = propagate_resource!(ctx
                .admit_iter(tail, "NX audit trail marker")
                .map_err(CodecError::from))
            .enumerate()
            .skip(1)
            .position(|(end, _)| tail[end - 1..end + 1] == [0x41, 0x00])?
            .checked_add(product_end + 2)?;
            Some(Ok((base_offset, bytes, start)))
        })()
        .transpose()?
        else {
            return Ok(None);
        };
        audit_trail_rows(ctx, bytes, start, bytes.len(), base_offset)
    }

    /// Decode unambiguous primary body references from bounded operation records.
    pub(crate) fn operation_body_references(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<(usize, OperationBodyReference)>, CodecError> {
        let mut references = Vec::new();
        for (ordinal, record) in self.operation_records_with_label_ordinals(ctx)? {
            if let Some(reference) = operation_body_reference(ctx, record.body_view())? {
                ctx.reserve_vec(&mut references, 1, "nx section operation body references")?;
                references.push((ordinal, reference));
            }
        }
        Ok(references)
    }
}

/// Decode complete feature-operation headers and their label frames.
fn operation_labels<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    base_offset: usize,
) -> Result<Vec<OperationLabel<'a>>, CodecError> {
    let mut labels = Vec::new();
    for header in ctx
        .admit_iter(
            &validated_operation_headers(ctx, bytes, base_offset)?,
            "NX operation label headers",
        )?
        .copied()
    {
        if let Some(label) = operation_label_at(ctx, bytes, base_offset, header)? {
            ctx.reserve_vec(&mut labels, 1, "nx operation labels")?;
            labels.push(label);
        }
    }
    Ok(labels)
}

fn validated_operation_headers(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    base_offset: usize,
) -> Result<Vec<OperationHeader>, CodecError> {
    const PREFIX: &[u8] = &[0x80, 0xcd, 0x01, 0x04, 0x01];
    const SCALAR_LEN: usize = 8;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan NX operation headers",
    )?;
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
        if ShiftedBinary64::read(raw_scalar).is_none()
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
        ctx.reserve_vec(&mut headers, 1, "nx operation headers")?;
        headers.push(header);
    }
    Ok(headers)
}

fn operation_label_at<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    base_offset: usize,
    header: OperationHeader,
) -> Result<Option<OperationLabel<'a>>, CodecError> {
    (|| -> Option<Result<OperationLabel<'a>, CodecError>> {
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
        if !propagate_resource!(ctx
            .admit_iter(name, "NX operation label text validation")
            .map_err(CodecError::from))
        .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
        {
            return None;
        }
        let Ok(value) =
            propagate_resource!(ctx.validate_utf8(name, "NX operation label UTF-8 validation"))
        else {
            return None;
        };
        Some(Ok(OperationLabel { header, value }))
    })()
    .transpose()
}

fn operation_records_with_labels_and_ordinals<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    base_offset: usize,
    labels: &[OperationLabel<'a>],
) -> Result<Vec<(usize, OperationRecord<'a>)>, CodecError> {
    let headers = validated_operation_headers(ctx, bytes, base_offset)?;
    let mut records = Vec::new();
    for (ordinal, header) in ctx
        .admit_iter(&headers, "NX bounded operation record headers")?
        .enumerate()
    {
        let record = (|| {
            let label = propagate_resource!(ctx
                .admit_iter(labels, "NX labeled operation header lookup")
                .map_err(CodecError::from))
            .find(|label| label.header.offset() == header.offset())?;
            let start = label.header.offset().checked_sub(base_offset)?;
            let end = headers
                .get(ordinal + 1)
                .map_or(bytes.len(), |next| next.offset() - base_offset);
            propagate_resource!(OperationRecord::new(ctx, bytes.get(start..end)?, *label)).map(Ok)
        })()
        .transpose()?;
        if let Some(record) = record {
            ctx.reserve_vec(&mut records, 1, "nx labeled operation records")?;
            records.push((ordinal, record));
        }
    }
    Ok(records)
}

fn unlabeled_operation_records_with_ordinals<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    base_offset: usize,
    labels: &[OperationLabel<'a>],
) -> Result<Vec<(usize, UnlabeledOperationRecord<'a>)>, CodecError> {
    let headers = validated_operation_headers(ctx, bytes, base_offset)?;
    let mut records = Vec::new();
    for (ordinal, header) in ctx
        .admit_iter(&headers, "NX bounded operation record headers")?
        .enumerate()
    {
        let record = (|| {
            if propagate_resource!(ctx
                .admit_iter(labels, "NX unlabeled operation header exclusion")
                .map_err(CodecError::from))
            .any(|label| label.header.offset() == header.offset())
            {
                return None;
            }
            let start = header.offset().checked_sub(base_offset)?;
            let end = headers
                .get(ordinal + 1)
                .map_or(bytes.len(), |next| next.offset() - base_offset);
            UnlabeledOperationRecord::new(*header, bytes.get(start..end)?).map(Ok)
        })()
        .transpose()?;
        if let Some(record) = record {
            ctx.reserve_vec(&mut records, 1, "nx unlabeled operation records")?;
            records.push((ordinal, record));
        }
    }
    Ok(records)
}

/// Decode ordered `03|04, length, text, 00` frames from one operation payload.
pub(crate) fn operation_payload_text_frames<'a>(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'a>,
) -> Result<Vec<OperationPayloadTextFrame<'a>>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(record.payload().len()),
        "scan NX operation payload text",
    )?;
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
        let Some(value) = ctx
            .validate_utf8(raw, "NX payload text UTF-8 validation")?
            .ok()
            .map(|value| {
                crate::payload_text::PayloadText::from_wire(ctx, value).map(|value| value.ok())
            })
            .transpose()?
            .flatten()
        else {
            at += 1;
            continue;
        };
        if declared < 3 || record.payload().get(end) != Some(&0) {
            at += 1;
            continue;
        }
        ctx.reserve_vec(&mut frames, 1, "nx operation payload text frames")?;
        frames.push(OperationPayloadTextFrame {
            marker,
            offset: record.payload_offset() + at,
            value,
        });
        at = end + 1;
    }
    Ok(frames)
}

/// Decode ordered `04, length, text, 00` strings from one operation payload.
pub(crate) fn operation_payload_strings<'a>(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'a>,
) -> Result<Vec<OperationPayloadString<'a>>, CodecError> {
    let mut strings = Vec::new();
    for frame in ctx.admit_iter(
        &operation_payload_text_frames(ctx, record)?,
        "NX operation payload string frames",
    )? {
        if frame.marker == OperationTextMarker::String {
            ctx.reserve_vec(&mut strings, 1, "nx operation payload strings")?;
            strings.push(OperationPayloadString {
                offset: frame.offset,
                value: frame.value,
            });
        }
    }
    Ok(strings)
}

/// Decode an exact nonempty duplicated shifted-binary64 lane before a hole template.
pub(crate) fn simple_hole_repeated_scalar_lane(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<Option<NonEmpty<RepeatedScalar<usize>>>, CodecError> {
    if record.name() != "SIMPLE HOLE" {
        return Ok(None);
    }
    let mut templates = operation_payload_strings(ctx, record)?
        .into_iter()
        .filter(|value| value.value.as_str().starts_with("Hole_"));
    let Some(template) = templates.next() else {
        return Ok(None);
    };
    if templates.next().is_some() {
        return Ok(None);
    }
    let Some(boundary) = template.offset.checked_sub(record.payload_offset()) else {
        return Ok(None);
    };
    let Some(prefix) = record.payload().get(..boundary) else {
        return Ok(None);
    };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(prefix.len()),
        "scan NX simple hole scalars",
    )?;
    let mut scalars = Vec::new();
    let mut at = 0usize;
    while at + 8 <= prefix.len() {
        if prefix[at] == 0x30 {
            if let Some(scalar) = ShiftedBinary64::read(&prefix[at..at + 8]) {
                ctx.reserve_vec(&mut scalars, 1, "nx simple hole scalar witnesses")?;
                scalars.push((scalar, record.payload_offset() + at));
                at += 8;
                continue;
            }
        }
        at += 1;
    }
    let half = scalars.len() / 2;
    if scalars.len() != half * 2 {
        return Ok(None);
    }
    let (first, second) = scalars.split_at(half);
    if ctx
        .admit_iter(first, "NX simple hole first scalar witnesses")?
        .zip(ctx.admit_iter(second, "NX simple hole second scalar witnesses")?)
        .any(|(left, right)| left.0 != right.0)
    {
        return Ok(None);
    }
    let mut repeated = Vec::new();
    for (left, right) in ctx
        .admit_iter(first, "NX simple hole first repeated scalars")?
        .zip(ctx.admit_iter(second, "NX simple hole second repeated scalars")?)
    {
        ctx.reserve_vec(&mut repeated, 1, "nx simple hole repeated scalars")?;
        repeated.push(RepeatedScalar {
            scalar: left.0,
            witness_offsets: [left.1, right.1],
        });
    }
    Ok(NonEmpty::from_admitted_vec(repeated))
}

/// Decode the unique four-block construction-group lane in a `HOLE PACKAGE` payload.
pub(crate) fn hole_package_construction_group_lane(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<Option<HolePackageConstructionGroupLane>, cadmpeg_core::CodecError> {
    let parsed: Option<Result<_, cadmpeg_core::CodecError>> = (|| {
        const PREFIX: [u8; 5] = [0x00, 0x00, 0x01, 0x00, 0x00];
        const ZEROES: [u8; 4] = [0; 4];
        const SUFFIX: [u8; 3] = [0x00, 0x00, 0xff];
        if record.name() != "HOLE PACKAGE" {
            return None;
        }
        let mut candidate = None;
        if let Some(last) = record.payload().len().checked_sub(PREFIX.len()) {
            for start in propagate_resource!(ctx
                .admit_iter(&(0..last), "NX hole package group candidate search")
                .map_err(CodecError::from))
            {
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
                        let (object_index, width) =
                            payload_object_index(record.payload().get(at..)?)?;
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
        }
        candidate.map(Ok)
    })();
    parsed.transpose()
}

/// Decode the unique counted reference field in a bounded `SKETCH` payload.
pub(crate) fn sketch_payload_references(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<Option<SketchReferenceField>, CodecError> {
    if record.name() != "SKETCH" {
        return Ok(None);
    }
    let mut failure = None;
    let field = {
        let Some(candidate_end_0) = record.payload().len().checked_sub(3) else {
            return Ok(None);
        };
        let mut candidates = ctx
            .admit_iter(&(0..candidate_end_0), "scan NX sketch reference fields")?
            .filter_map(|start| {
                if failure.is_some() {
                    return None;
                }
                if record.payload().get(start..start + 2) != Some(&[0x01, 0x00]) {
                    return None;
                }
                match SketchReferenceField::read(ctx, record, start) {
                    Ok(field) => field,
                    Err(error) => {
                        failure = Some(error);
                        None
                    }
                }
            });
        let first = candidates.next();
        let second = candidates.next();
        if second.is_none() {
            first
        } else {
            None
        }
    };
    if let Some(error) = failure {
        return Err(error);
    }
    field.map(|shape| shape.materialize(ctx)).transpose()
}

fn payload_object_index(bytes: &[u8]) -> Option<(ReferenceIndexToken, usize)> {
    let token = ReferenceIndexToken::read_payload(bytes)?;
    Some((token, token.raw().len()))
}

/// Decode the unique exactly counted transform lane in a bounded pattern payload.
pub(crate) fn pattern_payload_transform_lane(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<Option<PatternPayloadTransformLane>, CodecError> {
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
        _ => return Ok(None),
    };
    let failure = std::cell::RefCell::new(None);
    let decode = |start: usize| {
        if failure.borrow().is_some() {
            return None;
        }
        (record.payload().get(start) == Some(&0x01)).then_some(())?;
        let declared_count @ 2.. = *record.payload().get(start + 1)? else {
            return None;
        };
        let row_schema_index = NonZeroU8::new(*record.payload().get(start + 2)?)?;
        let mut at = start + 2;
        let mut rows = Vec::new();
        for ordinal in match ctx.admit_iter(
            &(1..declared_count),
            "NX pattern payload transform lane row traversal",
        ) {
            Ok(rows) => rows,
            Err(error) => {
                *failure.borrow_mut() = Some(error.into());
                return None;
            }
        } {
            (record.payload().get(at) == Some(&row_schema_index.get())).then_some(())?;
            (match ctx.equal(
                &record.payload().get(at + 1..at + 1 + prefix_tail.len()),
                &Some(prefix_tail),
                "NX pattern transform framing equality",
            ) {
                Ok(matches) => matches,
                Err(error) => {
                    *failure.borrow_mut() = Some(error.into());
                    return None;
                }
            })
            .then_some(())?;
            at += 1 + prefix_tail.len();
            let scalar = ShiftedScalar::read(record.payload().get(at..)?)?;
            let width = scalar.raw().len();
            let value = PatternValue {
                scalar,
                offset: record.payload_offset() + at,
            };
            at += width;
            (match ctx.equal(
                &record.payload().get(at..at + scalar_suffix.len()),
                &Some(scalar_suffix),
                "NX pattern transform framing equality",
            ) {
                Ok(matches) => matches,
                Err(error) => {
                    *failure.borrow_mut() = Some(error.into());
                    return None;
                }
            })
            .then_some(())?;
            at += scalar_suffix.len();
            let selector_offset = at;
            let atom = CompactIndexAtom::read(record.payload().get(at..)?)?;
            let width = atom.raw().len();
            let selector = LocatedCompactIndex {
                atom,
                offset: record.payload_offset() + selector_offset,
            };
            if let Err(error) = ctx.reserve_vec(&mut rows, 1, "nx pattern transform rows") {
                *failure.borrow_mut() = Some(error);
                return None;
            }
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
        if failure.borrow().is_some() {
            return None;
        }
        (record.name() == "Pattern Feature").then_some(())?;
        (record.payload().get(start) == Some(&0x01)).then_some(())?;
        let declared_count @ 2.. = *record.payload().get(start + 1)? else {
            return None;
        };
        let row_schema_index = NonZeroU8::new(*record.payload().get(start + 2)?)?;
        let mut at = start + 2;
        let mut rows = Vec::new();
        for ordinal in match ctx.admit_iter(
            &(1..declared_count),
            "NX pattern payload transform lane row traversal",
        ) {
            Ok(rows) => rows,
            Err(error) => {
                *failure.borrow_mut() = Some(error.into());
                return None;
            }
        } {
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
            if let Err(error) = ctx.reserve_vec(&mut rows, 1, "nx pattern wide transform rows") {
                *failure.borrow_mut() = Some(error);
                return None;
            }
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
    let candidate = {
        let Some(candidate_end_0) = record.payload().len().checked_sub(1) else {
            return Ok(None);
        };
        let Some(candidate_end_1) = record.payload().len().checked_sub(1) else {
            return Ok(None);
        };
        let mut candidates = ctx
            .admit_iter(&(0..candidate_end_0), "scan NX pattern transform lanes")?
            .filter_map(decode)
            .chain(
                ctx.admit_iter(&(0..candidate_end_1), "scan NX pattern transform lanes")?
                    .filter_map(decode_wide),
            );
        let first = candidates.next();
        let second = candidates.next();
        if second.is_none() {
            first
        } else {
            None
        }
    };
    if let Some(error) = failure.into_inner() {
        return Err(error);
    }
    Ok(candidate)
}

/// Decode the unique exactly counted instance-output lane in a bounded payload.
pub(crate) fn multi_instance_output_payload_lane(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<Option<MultiInstanceOutputPayloadLane>, CodecError> {
    const ENVELOPE: [u8; 10] = [0x3a, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x25, 0x01];
    const ROW_PREFIX: [u8; 7] = [0x26, 0x27, 0x01, 0x02, 0x65, 0x01, 0x02];
    const ROW_ORDINAL_MARKER: u8 = 0x28;
    const REFERENCE_PREFIX: [u8; 2] = [0x00, 0x3b];

    if record.name() != "Multi Instance Output" {
        return Ok(None);
    }
    let mut failure = None;
    let mut decode = |start: usize| {
        if failure.is_some() {
            return None;
        }
        (record.payload().get(start..start + ENVELOPE.len()) == Some(&ENVELOPE)).then_some(())?;
        let declared_count = *record.payload().get(start + ENVELOPE.len())?;
        (declared_count >= 2).then_some(())?;
        let mut instance_count = 0;
        let mut at = start + ENVELOPE.len() + 1;
        let mut rows = Vec::new();
        for expected_row_index in match ctx.admit_iter(
            &(2..=declared_count),
            "NX multi instance output payload lane row traversal",
        ) {
            Ok(rows) => rows,
            Err(error) => {
                failure = Some(error.into());
                return None;
            }
        } {
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
            if let Err(error) = ctx.reserve_vec(&mut rows, 1, "nx multi-instance selector rows") {
                failure = Some(error);
                return None;
            }
            rows.push((selector, ordinal));
            at += 3;
        }
        (record.payload().get(at..at + REFERENCE_PREFIX.len()) == Some(&REFERENCE_PREFIX))
            .then_some(())?;
        at += REFERENCE_PREFIX.len();
        // The row ordinals state the instance count. One trailing reference
        // follows for every instance after the first, so the ordinal domain
        // states how many references the lane carries; `MultiInstanceOutputs`
        // refuses a lane whose reference count does not cover every instance.
        let trailing_instances = 1..instance_count;
        let mut trailing_references = Vec::new();
        for _ in match ctx.admit_iter(
            &(trailing_instances),
            "NX multi instance output payload lane row traversal",
        ) {
            Ok(rows) => rows,
            Err(error) => {
                failure = Some(error.into());
                return None;
            }
        } {
            let reference_offset = at;
            let object_index =
                reference_index::FeatureReferenceToken::read(record.payload().get(at..)?)?;
            let end = at + object_index.raw().len();
            if let Err(error) = ctx.reserve_vec(
                &mut trailing_references,
                1,
                "nx multi-instance trailing references",
            ) {
                failure = Some(error);
                return None;
            }
            trailing_references.push(PayloadObjectReference {
                offset: record.payload_offset() + reference_offset,
                token: object_index,
            });
            at = end;
        }
        (record.payload().get(at..at + 2) == Some(&[0x01, instance_count])).then_some(())?;
        let outputs =
            match instances::MultiInstanceOutputs::new_charged(ctx, rows, trailing_references) {
                Ok(Some(outputs)) => outputs,
                Ok(None) => return None,
                Err(error) => {
                    failure = Some(error);
                    return None;
                }
            };
        Some(MultiInstanceOutputPayloadLane {
            offset: record.payload_offset() + start + 8,
            outputs,
        })
    };
    let candidate = {
        let Some(candidate_end_0) = record.payload().len().checked_sub(ENVELOPE.len()) else {
            return Ok(None);
        };
        let mut candidates = ctx
            .admit_iter(
                &(0..=candidate_end_0),
                "scan NX multi-instance output lanes",
            )?
            .filter_map(&mut decode);
        let first = candidates.next();
        let second = candidates.next();
        if second.is_none() {
            first
        } else {
            None
        }
    };
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(candidate)
}

/// Decode the unique exactly counted selector lane in an
/// `IDENTICAL INSTANCE OUTPUT` payload.
pub(crate) fn identical_instance_output_payload_lane(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<Option<IdenticalInstanceOutputPayloadLane>, CodecError> {
    const ROW_MIDDLE: [u8; 2] = [0x01, 0x02];
    const SENTINEL: [u8; 7] = [0xe0, 0x7f, 0xff, 0xff, 0xff, 0x00, 0x00];

    if record.name() != "IDENTICAL INSTANCE OUTPUT" {
        return Ok(None);
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
        for ordinal in propagate_resource!(ctx
            .admit_iter(
                &(2..=declared_count),
                "NX identical instance output payload lane row validation"
            )
            .map_err(CodecError::from))
        {
            (record.payload().get(at) == Some(&first_schema_index)).then_some(())?;
            (record.payload().get(at + 1) == Some(&second_schema_index)).then_some(())?;
            (record.payload().get(at + 2..at + 4) == Some(&ROW_MIDDLE)).then_some(())?;
            (record.payload().get(at + 4) == Some(&third_schema_index)).then_some(())?;
            at += 5;
            let atom = CompactIndexAtom::read(record.payload().get(at..)?)?;
            let width = atom.raw().len();
            at += width;
            (record.payload().get(at) == Some(&0x00)).then_some(())?;
            (record.payload().get(at + 1) == Some(&ordinal)).then_some(())?;
            at += 2;
        }
        let terminal_count = declared_count.checked_add(1)?;
        (record.payload().get(at) == Some(&0x00)).then_some(())?;
        (record.payload().get(at + 1) == Some(&terminal_count)).then_some(())?;
        (record.payload().get(at + 2..at + 2 + SENTINEL.len()) == Some(&SENTINEL)).then_some(())?;
        (Some((
            start,
            leading_schema_index,
            count_schema_index,
            declared_count,
        )))
        .map(Ok)
    };
    let Some((start, leading_schema_index, count_schema_index, declared_count)) = ({
        let Some(candidate_end_0) = record.payload().len().checked_sub(3) else {
            return Ok(None);
        };
        let mut candidates = ctx
            .admit_iter(
                &(0..candidate_end_0),
                "scan NX identical-instance selectors",
            )?
            .filter_map(decode);
        let first = candidates.next().transpose()?;
        let second = candidates.next().transpose()?;
        if second.is_none() {
            first
        } else {
            None
        }
    }) else {
        return Ok(None);
    };
    let selector_count = usize::from(declared_count - 1);
    let operation = "NX identical-instance selectors";
    let mut selectors = ctx.collection_vec(selector_count, operation)?;
    let mut at = start + 4;
    for _ in ctx.admit_iter(
        &(2..=declared_count),
        "NX identical instance output payload lane range traversal",
    )? {
        at += 5;
        let Some(atom) = record.payload().get(at..).and_then(CompactIndexAtom::read) else {
            return Ok(None);
        };
        selectors.push(LocatedCompactIndex {
            atom,
            offset: record.payload_offset() + at,
        });
        at += atom.raw().len() + 2;
    }
    Ok(compact::CountedIndexMembers::new(selectors)
        .ok()
        .map(|selectors| IdenticalInstanceOutputPayloadLane {
            offset: record.payload_offset() + start,
            leading_schema_index,
            count_schema_index,
            selectors,
        }))
}

/// Decode the exact leading construction header in a bounded `POINT` payload.
pub(crate) fn point_feature_payload_header(
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
pub(crate) fn point_feature_scalar_lane(
    preceding_block: &[u8],
    target_block: &[u8],
) -> Option<PointFeatureScalarLane> {
    const SUFFIX: [u8; 19] = [
        0x00, 0x25, 0x25, 0x41, 0x00, 0x04, 0x01, 0x07, 0x01, 0xc0, 0x45, 0x10, 0x00, 0x80, 0x86,
        0x02, 0x00, 0x01, 0x00,
    ];
    let preceding_start = preceding_block.len().checked_sub(3)?;
    if target_block.get(45..64) != Some(&SUFFIX) {
        return None;
    }
    let target = target_block.get(..45)?;
    let mut lane = [0; 48];
    lane[..3].copy_from_slice(&preceding_block[preceding_start..]);
    lane[3..].copy_from_slice(target);
    let first = ShiftedBinary64::read(&lane[..8])?;
    let mut values = [first; 6];
    for (value, bytes) in values.iter_mut().zip(lane.chunks_exact(8)) {
        let scalar = ShiftedBinary64::read(bytes)?;
        *value = scalar;
    }
    Some(PointFeatureScalarLane {
        values,
        offset: preceding_start,
    })
}

/// Decode the exact leading construction branch in a bounded `SWP104`
/// payload.
pub(crate) fn swp104_payload_leading_branch(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<Option<Swp104PayloadLeadingBranch>, CodecError> {
    const HEADER: [u8; 4] = [0x00, 0x00, 0x01, 0x00];
    if record.name() != "SWP104" {
        return Ok(None);
    }
    let Some((discriminator, scalars, leading_zero, mode, declared_count, mut at)) = (|| {
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
        Some((
            discriminator,
            scalars,
            leading_zero,
            mode,
            declared_count,
            at,
        ))
    })() else {
        return Ok(None);
    };
    let len = usize::from(declared_count) - 1;
    let operation = "NX SWP104 members";
    let mut members = ctx.collection_vec(len, operation)?;
    for _ in ctx.admit_iter(&(1..declared_count), "scan NX SWP104 leading branch")? {
        let Some(object_index) = record
            .payload()
            .get(at..)
            .and_then(reference_index::PayloadIndexToken::read)
        else {
            return Ok(None);
        };
        let width = object_index.raw().len();
        at += width;
        members.push(object_index);
    }

    let witnessed_count = if record.payload().get(at) == Some(&0x01) {
        let Some(count @ 2..) = record.payload().get(at + 1).copied() else {
            return Ok(None);
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
        match record.payload().get(at..at + state_len) {
            Some(bytes) => ctx.copy_retained(bytes, "NX SWP104 state lane")?,
            None => return Ok(None),
        },
    )
    .ok();
    let Some(state_lane) = state_lane else {
        return Ok(None);
    };
    at += state_len;
    if record.payload().get(at..at + 3) != Some(&[0xff, 0x01, 0x02]) {
        return Ok(None);
    }
    at += 3;
    let Some(object_index) = record
        .payload()
        .get(at..)
        .and_then(reference_index::PayloadIndexToken::read)
    else {
        return Ok(None);
    };
    let width = object_index.raw().len();
    at += width;
    let terminal = object_index;
    if record.payload().get(at) != Some(&0x00) {
        return Ok(None);
    }

    Ok(Some(Swp104PayloadLeadingBranch {
        discriminator,
        scalars,
        leading_zero,
        mode,
        state_lane,
        members: match BranchItems::new(members) {
            Ok(members) => members,
            Err(_) => return Ok(None),
        },
        terminal,
    }))
}

/// Decode the fixed two-scalar header in a bounded `EXTRUDE` payload.
pub(crate) fn extrude_payload_header(record: OperationPayload<'_>) -> Option<ExtrudePayloadHeader> {
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
pub(crate) fn operation_body_members(
    ctx: &DecodeContext<'_>,
    record: OperationBodyInput<'_>,
) -> Result<Vec<OperationBodyMemberGroup>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(record.bytes().len()),
        "scan NX operation body members",
    )?;
    let mut groups = Vec::new();
    for (body_ordinal, reference) in operation_body_reference_candidates(ctx, record)?.enumerate() {
        let mut failure = None;
        let group = (|| {
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
            let mut members = Vec::new();
            for _ in
                match ctx.admit_iter(&(0..count - 1), "NX operation body members row traversal") {
                    Ok(rows) => rows,
                    Err(error) => {
                        failure = Some(error.into());
                        return None;
                    }
                }
            {
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
                if let Err(error) = ctx.reserve_vec(&mut members, 1, "NX operation body members") {
                    failure = Some(error);
                    return None;
                }
                members.push(LocatedCompactIndex {
                    atom,
                    offset: record.offset() + member_at,
                });
            }
            Some(OperationBodyMemberGroup {
                body_reference_ordinal: u32::try_from(body_ordinal).ok()?,
                body_object_index: reference.object_index.value(),
                members,
            })
        })();
        if let Some(error) = failure {
            return Err(error);
        }
        if let Some(group) = group {
            ctx.reserve_vec(&mut groups, 1, "NX operation body member groups")?;
            groups.push(group);
        }
    }
    Ok(groups)
}

/// Decode exact continuations following `TRIM BODY` branch-`11` member lanes.
pub(crate) fn operation_body_11_continuations(
    ctx: &DecodeContext<'_>,
    record: OperationBodyInput<'_>,
) -> Result<Vec<OperationBody11Continuation>, CodecError> {
    if record.name() != "TRIM BODY" {
        return Ok(Vec::new());
    }
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(record.bytes().len()),
        "scan NX operation body continuations",
    )?;
    let mut continuations = Vec::new();
    for (body_ordinal, reference) in operation_body_reference_candidates(ctx, record)?.enumerate() {
        let continuation = (|| {
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
            for _ in propagate_resource!(ctx
                .admit_iter(
                    &(0..member_count - 1),
                    "NX operation body 11 continuations row validation"
                )
                .map_err(CodecError::from))
            {
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
            (Some(OperationBody11Continuation {
                body_reference_ordinal: u32::try_from(body_ordinal).ok()?,
                body_object_index: reference.object_index.value(),
                continuation,
                terminal: PayloadObjectReference {
                    token: terminal_token,
                    offset: record.offset() + terminal_at,
                },
            }))
            .map(Ok)
        })()
        .transpose()?;
        if let Some(continuation) = continuation {
            ctx.reserve_vec(&mut continuations, 1, "NX operation body continuations")?;
            continuations.push(continuation);
        }
    }
    Ok(continuations)
}

/// Decode complete unwrapped counted reference lanes following body scalar clauses.
pub(crate) fn operation_body_reference_lanes(
    ctx: &DecodeContext<'_>,
    record: OperationBodyInput<'_>,
) -> Result<Vec<OperationBodyReferenceLane>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(record.bytes().len()),
        "scan NX operation body reference lanes",
    )?;
    let mut lanes = Vec::new();
    for (body_ordinal, reference) in operation_body_reference_candidates(ctx, record)?.enumerate() {
        let mut failure = None;
        let lane = (|| {
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
            let compact = match operation_body_reference_lane_values(
                ctx,
                record,
                at,
                count - 1,
                |bytes, offset| {
                    let atom = CompactIndexAtom::read(bytes)?;
                    let width = atom.raw().len();
                    Some((LocatedCompactIndex { atom, offset }, width))
                },
            ) {
                Ok(values) => values,
                Err(error) => {
                    failure = Some(error);
                    return None;
                }
            };
            let objects = match operation_body_reference_lane_values(
                ctx,
                record,
                at,
                count - 1,
                |bytes, offset| {
                    let token = reference_index::PayloadIndexToken::read(bytes)?;
                    let width = token.raw().len();
                    Some((PayloadObjectReference { offset, token }, width))
                },
            ) {
                Ok(values) => values,
                Err(error) => {
                    failure = Some(error);
                    return None;
                }
            };
            let values = match (compact, objects) {
                (Some(values), None) => OperationBodyReferenceLaneValues::CompactIndex(values),
                (None, Some(values)) => {
                    OperationBodyReferenceLaneValues::PayloadObjectIndex(values)
                }
                _ => return None,
            };
            Some(OperationBodyReferenceLane {
                body_reference_ordinal: u32::try_from(body_ordinal).ok()?,
                body_object_index: reference.object_index.value(),
                branch,
                values,
            })
        })();
        if let Some(error) = failure {
            return Err(error);
        }
        if let Some(lane) = lane {
            ctx.reserve_vec(&mut lanes, 1, "NX operation body reference lanes")?;
            lanes.push(lane);
        }
    }
    Ok(lanes)
}

fn operation_body_reference_lane_values<T>(
    ctx: &DecodeContext<'_>,
    record: OperationBodyInput<'_>,
    mut at: usize,
    count: usize,
    read: impl Fn(&[u8], usize) -> Option<(T, usize)>,
) -> Result<Option<Vec<T>>, CodecError> {
    let mut values = Vec::new();
    for _ in ctx.admit_iter(
        &(0..count),
        "NX operation body reference lane values range traversal",
    )? {
        let Some((value, width)) = record
            .bytes()
            .get(at..)
            .and_then(|bytes| read(bytes, record.offset() + at))
        else {
            return Ok(None);
        };
        at += width;
        ctx.reserve_vec(&mut values, 1, "NX operation body lane values")?;
        values.push(value);
    }
    Ok((record.bytes().get(at..at + 4) == Some(&[0x00, 0x00, 0x0b, 0x00])).then_some(values))
}

/// Decode one complete datum-plane descriptor block.
pub(crate) fn datum_plane_descriptor_block(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<plane_descriptor::PlaneDescriptor>, CodecError> {
    plane_descriptor::PlaneDescriptor::from_bytes(ctx, bytes)
}

/// Decode every complete scalar-vector frame in a reconstructed sketch
/// payload.
pub(crate) fn sketch_payload_scalar_lanes(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<FramedScalarRun<SketchScalarLaneForm, ()>>, CodecError> {
    let work = cadmpeg_core::decode::u64_from_index(bytes.len())
        .checked_mul(2)
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "scan NX sketch scalar lanes",
                0,
                cadmpeg_core::decode::u64_from_index(bytes.len()),
            )
        })?;
    ctx.charge_work(work, "scan NX sketch scalar lanes")?;
    let mut lanes = Vec::new();
    for form in [SketchScalarLaneForm::Form03, SketchScalarLaneForm::Form07] {
        let discriminator = form.discriminator();
        for (offset, window) in bytes.windows(discriminator.len()).enumerate() {
            if !ctx.equal_bytes(
                window,
                discriminator,
                "NX sketch scalar lane discriminator equality",
            )? {
                continue;
            }
            let mut at = offset + discriminator.len();
            let mut values = Vec::new();
            let complete = loop {
                ctx.charge_work(1, "scan NX sketch scalar atoms")?;
                if bytes.get(at) == Some(&0x00) {
                    break true;
                }
                let Some(scalar) = bytes.get(at..).and_then(ShiftedScalar::read) else {
                    break false;
                };
                at += scalar.raw().len();
                ctx.reserve_vec(&mut values, 1, "NX sketch scalar atoms")?;
                values.push((scalar, ()));
            };
            if !complete {
                continue;
            }
            let Some(values) = NonEmpty::from_admitted_vec(values) else {
                continue;
            };
            let Ok(lane) = FramedScalarRun::from_wire(
                ctx,
                form,
                cadmpeg_core::decode::u64_from_index(offset),
                values,
            )?
            else {
                continue;
            };
            ctx.reserve_vec(&mut lanes, 1, "NX sketch scalar lanes")?;
            lanes.push(lane);
        }
    }
    ctx.stable_sort_by_key(
        &mut lanes,
        FramedScalarRun::offset,
        Ord::cmp,
        "sort NX sketch payload scalar lanes",
    )?;
    Ok(lanes)
}

/// Decode every exactly framed scaled shifted-binary64 pair in a reconstructed sketch payload.
pub(crate) fn sketch_payload_fixed_pairs(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<SketchPayloadFixedPair>, CodecError> {
    let work = cadmpeg_core::decode::u64_from_index(bytes.len())
        .checked_mul(cadmpeg_core::decode::u64_from_index(
            SketchPairForm::ALL.len(),
        ))
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "scan NX sketch fixed pairs",
                0,
                cadmpeg_core::decode::u64_from_index(bytes.len()),
            )
        })?;
    ctx.charge_work(work, "scan NX sketch fixed pairs")?;
    let mut pairs = Vec::new();
    for form in SketchPairForm::ALL {
        let discriminator = form.discriminator();
        let separator_width = form.separator_width();
        for (offset, window) in bytes.windows(discriminator.len()).enumerate() {
            if !ctx.equal_bytes(
                window,
                discriminator,
                "NX sketch fixed pair discriminator equality",
            )? {
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
            ctx.reserve_vec(&mut pairs, 1, "NX sketch fixed pairs")?;
            pairs.push(SketchPayloadFixedPair {
                offset,
                values: [first_value, second_value],
                form,
            });
        }
    }
    ctx.stable_sort_by(
        &mut pairs,
        |value| &value.offset,
        Ord::cmp,
        "sort NX sketch payload pairs",
    )?;
    Ok(pairs)
}

/// Decode every exactly framed mixed scaled shifted-binary64/binary32 pair in a sketch payload.
pub(crate) fn sketch_payload_mixed_pairs(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<SketchPayloadMixedPair>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan NX sketch mixed pairs",
    )?;
    let discriminator = SketchPairForm::Legacy.discriminator();
    let mut pairs = Vec::new();
    for (offset, window) in bytes.windows(discriminator.len()).enumerate() {
        if !ctx.equal_bytes(
            window,
            discriminator,
            "NX sketch mixed pair discriminator equality",
        )? {
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
        ctx.reserve_vec(&mut pairs, 1, "NX sketch mixed pairs")?;
        pairs.push(SketchPayloadMixedPair {
            offset,
            scalars: SketchMixedScalars {
                fixed,
                binary32: binary32_atom,
            },
        });
    }
    Ok(pairs)
}

fn sketch_fixed_atom(bytes: &[u8], offset: usize) -> Option<SketchScaledAtom> {
    Some(SketchScaledAtom::from_raw(
        bytes.get(offset + 1..offset + 8)?.try_into().ok()?,
    ))
}

/// Decode every exactly framed signed Q1.55 pair in a datum-CSYS payload.
pub(crate) fn datum_csys_payload_fixed_pairs(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<DatumCsysPayloadFixedPair>, CodecError> {
    let work = cadmpeg_core::decode::u64_from_index(bytes.len())
        .checked_mul(cadmpeg_core::decode::u64_from_index(
            DatumPairForm::ALL.len(),
        ))
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "scan NX datum CSYS pairs",
                0,
                cadmpeg_core::decode::u64_from_index(bytes.len()),
            )
        })?;
    ctx.charge_work(work, "scan NX datum CSYS pairs")?;
    let mut pairs = Vec::new();
    for form in DatumPairForm::ALL {
        let discriminator = form.discriminator();
        for (offset, window) in bytes.windows(discriminator.len()).enumerate() {
            if !ctx.equal_bytes(
                window,
                discriminator,
                "NX datum CSYS pair discriminator equality",
            )? {
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
            let (Some(first_value), Some(second_value)) =
                (Q155::from_raw(first_raw), Q155::from_raw(second_raw))
            else {
                continue;
            };
            ctx.reserve_vec(&mut pairs, 1, "NX datum CSYS pairs")?;
            pairs.push(DatumCsysPayloadFixedPair {
                offset,
                values: [first_value, second_value],
                form,
            });
        }
    }
    ctx.stable_sort_by(
        &mut pairs,
        |value| &value.offset,
        Ord::cmp,
        "sort NX datum csys payload pairs",
    )?;
    Ok(pairs)
}

/// Decode every complete signed Q1.55 lane in a reconstructed draft graph payload.
pub(crate) fn draft_construction_fixed_lanes(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<FramedScalarRun<Q155LaneFrame, ()>>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan NX draft fixed lanes",
    )?;
    let mut lanes = Vec::new();
    for (offset, window) in bytes
        .windows(Q155LaneFrame::DISCRIMINATOR.len())
        .enumerate()
    {
        if window != Q155LaneFrame::DISCRIMINATOR {
            continue;
        }
        let mut at = offset + Q155LaneFrame::DISCRIMINATOR.len();
        let mut values = Vec::new();
        let complete = loop {
            ctx.charge_work(1, "scan NX draft fixed atoms")?;
            let Some(marker) = bytes.get(at).copied().and_then(Q155Marker::read) else {
                break bytes.get(at) == Some(&0x00);
            };
            let Some(raw) = bytes
                .get(at + 1..at + 8)
                .and_then(|raw| raw.try_into().ok())
            else {
                break false;
            };
            let Some(scalar) = Q155::from_raw(raw) else {
                break false;
            };
            ctx.reserve_vec(&mut values, 1, "NX draft fixed atoms")?;
            values.push((Q155Atom { marker, scalar }, ()));
            at += 8;
        };
        if !complete {
            continue;
        }
        let Some(values) = NonEmpty::from_admitted_vec(values) else {
            continue;
        };
        let Ok(lane) = FramedScalarRun::from_wire(
            ctx,
            Q155LaneFrame,
            cadmpeg_core::decode::u64_from_index(offset),
            values,
        )?
        else {
            continue;
        };
        ctx.reserve_vec(&mut lanes, 1, "NX draft fixed lanes")?;
        lanes.push(lane);
    }
    Ok(lanes)
}

/// Decode every complete shifted-binary32 lane in a reconstructed draft graph payload.
pub(crate) fn draft_construction_binary32_lanes(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<FramedScalarRun<DraftBinary32Branch, ()>>, CodecError> {
    let work = cadmpeg_core::decode::u64_from_index(bytes.len())
        .checked_mul(2)
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "scan NX draft binary32 lanes",
                0,
                cadmpeg_core::decode::u64_from_index(bytes.len()),
            )
        })?;
    ctx.charge_work(work, "scan NX draft binary32 lanes")?;
    let mut lanes = Vec::new();
    for branch in [DraftBinary32Branch::Form04, DraftBinary32Branch::Form03] {
        let discriminator = branch.discriminator();
        for (offset, window) in bytes.windows(discriminator.len()).enumerate() {
            if window != discriminator {
                continue;
            }
            let mut at = offset + discriminator.len();
            let mut values = Vec::new();
            let complete = loop {
                ctx.charge_work(1, "scan NX draft binary32 atoms")?;
                if !matches!(bytes.get(at), Some(0x40..=0x5f | 0xc0..=0xdf)) {
                    break bytes.get(at) == Some(&0x00);
                }
                let Some(scalar) = bytes.get(at..at + 4).and_then(ShiftedBinary32::read) else {
                    break false;
                };
                ctx.reserve_vec(&mut values, 1, "NX draft binary32 atoms")?;
                values.push((scalar, ()));
                at += 4;
            };
            if !complete {
                continue;
            }
            let Some(values) = NonEmpty::from_admitted_vec(values) else {
                continue;
            };
            let Ok(lane) = FramedScalarRun::from_wire(
                ctx,
                branch,
                cadmpeg_core::decode::u64_from_index(offset),
                values,
            )?
            else {
                continue;
            };
            ctx.reserve_vec(&mut lanes, 1, "NX draft binary32 lanes")?;
            lanes.push(lane);
        }
    }
    ctx.stable_sort_by_key(
        &mut lanes,
        FramedScalarRun::offset,
        Ord::cmp,
        "sort NX draft construction lanes",
    )?;
    Ok(lanes)
}

/// Decode a bounded datum-CSYS descriptor containing one unique maximal identity run.
pub(crate) fn datum_csys_descriptor_block(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<csys_descriptor::CsysDescriptor>, CodecError> {
    csys_descriptor::CsysDescriptor::read_charged(ctx, bytes)
}

/// Decode every complete identity frame in a reconstructed draft construction payload.
pub(crate) fn draft_construction_identity_frames(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<draft_identity::DraftIdentityFrame>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan NX draft identity frames",
    )?;
    let mut frames = Vec::new();
    for offset in 0..bytes.len() {
        if let Some(frame) = draft_identity::DraftIdentityFrame::read(ctx, bytes, offset)? {
            ctx.reserve_vec(&mut frames, 1, "NX draft identity frames")?;
            frames.push(frame);
        }
    }
    Ok(frames)
}

/// Decode compact object IDs followed by their complete frame discriminator.
pub(crate) fn data_block_object_frames(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<LocatedCompactIndex>, CodecError> {
    const DISCRIMINATOR: [u8; 18] = [
        0x00, 0x72, 0x01, 0xc0, 0x20, 0x02, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x01,
        0x02, 0x80, 0xa4,
    ];
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan NX data-block object frames",
    )?;
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
        ctx.reserve_vec(&mut references, 1, "NX data-block object frames")?;
        references.push(LocatedCompactIndex { atom, offset });
        offset += width + DISCRIMINATOR.len();
    }
    Ok(references)
}

/// Decode the unique `04, length, p<decimal>[_qualifier], 00` declaration name.
pub(crate) fn expression_declaration_name<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
) -> Result<Option<ExpressionDeclarationName<'a>>, CodecError> {
    let mut declaration = None;
    let mut literal = None;
    let mut multiple_literals = false;
    if let Some(range_end) = bytes.len().checked_sub(4) {
        for at in ctx.admit_iter(
            &(0..range_end),
            "NX expression declaration name range traversal",
        )? {
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
            let Ok(value) = ctx.validate_utf8(raw, "NX expression parameter UTF-8 validation")?
            else {
                continue;
            };
            let Some(name) = ParameterName::<_, u32>::parse_wire(ctx, value)? else {
                if evaluate_constant_expression(ctx, value)?.is_some()
                    && literal.replace(value).is_some()
                {
                    multiple_literals = true;
                }
                continue;
            };
            if declaration.replace((at, name)).is_some() {
                return Ok(None);
            }
        }
    }
    let Some((offset, name)) = declaration else {
        return Ok(None);
    };
    let literal = (!multiple_literals).then_some(literal).flatten();
    Ok(Some(ExpressionDeclarationName {
        offset,
        name,
        literal,
    }))
}

/// Decode the unique direct primary-body field in one operation.
pub(crate) fn operation_body_reference(
    ctx: &DecodeContext<'_>,
    record: OperationBodyInput<'_>,
) -> Result<Option<OperationBodyReference>, CodecError> {
    let mut candidates = operation_body_reference_candidates(ctx, record)?;
    let first = candidates.next();
    Ok(if candidates.next().is_some() {
        None
    } else {
        first
    })
}

fn operation_body_reference_candidates<'record>(
    ctx: &DecodeContext<'_>,
    record: OperationBodyInput<'record>,
) -> Result<impl Iterator<Item = OperationBodyReference> + 'record, CodecError> {
    let width = std::num::NonZeroUsize::new(3)
        .ok_or_else(|| CodecError::Malformed("zero body reference window width".into()))?;
    let mut cursor = 0usize;
    Ok(ctx
        .admit_iter(record.bytes(), "NX operation body reference traversal")?
        .windows(width)
        .enumerate()
        .filter_map(move |(marker, window)| {
            if marker < cursor {
                return None;
            }
            cursor = marker + 1;
            let body_write =
                marker
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
                return None;
            }
            if window != [0x01, 0x02, 0x10] {
                return None;
            }
            let token = marker + 3;
            let object_index =
                reference_index::FeatureReferenceToken::read(&record.bytes()[token..])?;
            let end = token + object_index.raw().len();
            if record.bytes().get(end) != Some(&0xff) {
                return None;
            }
            Some(OperationBodyReference {
                offset: record.offset() + token,
                object_index,
            })
        }))
}

/// Decode every ordered direct primary-body field in one operation.
pub(crate) fn operation_body_references(
    ctx: &DecodeContext<'_>,
    record: OperationBodyInput<'_>,
) -> Result<Vec<OperationBodyReference>, CodecError> {
    let mut references = Vec::new();
    for reference in operation_body_reference_candidates(ctx, record)? {
        ctx.reserve_vec(&mut references, 1, "NX operation body references")?;
        references.push(reference);
    }
    Ok(references)
}

/// Decode every exact nested `01 02 tag index 97 75 01 02 endpoint_tag index ff` frame.
///
/// Both indices are non-null and canonical. Endpoint tags `10`, `12`, and
/// `15` select the body-image field across the supported schema generations.
pub(crate) fn operation_body_write_frames(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<Vec<BodyWriteFrame<usize>>, CodecError> {
    body_write_frames(ctx, record.payload(), record.payload_offset())
}

/// Decode body-write frames from one independently bounded unlabeled record.
pub(crate) fn unlabeled_operation_body_write_frames(
    ctx: &DecodeContext<'_>,
    record: UnlabeledOperationRecord<'_>,
) -> Result<Vec<BodyWriteFrame<usize>>, CodecError> {
    body_write_frames(ctx, record.payload(), record.header().end_offset())
}

fn body_write_frames(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    payload_offset: usize,
) -> Result<Vec<BodyWriteFrame<usize>>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(payload.len()),
        "scan NX body-write frames",
    )?;
    let mut relations = Vec::new();
    for marker in payload
        .windows(2)
        .enumerate()
        .filter_map(|(offset, window)| (window == [0x01, 0x02]).then_some(offset))
    {
        if let Some(write) = operation_body_write_frame_at(payload, payload_offset, marker) {
            ctx.reserve_vec(&mut relations, 1, "nx body-write frames")?;
            relations.push(write);
        }
    }
    Ok(relations)
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
fn operation_state_messages(bytes: &[u8], base_offset: usize) -> Vec<OperationStateMessage<'_>> {
    let mut messages = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let Some(message) = crate::test_support::with_decode_context(|ctx| {
            OperationStateMessage::read(ctx, bytes, at, base_offset)
        })
        .unwrap() else {
            at += 1;
            continue;
        };
        at = message.end_offset() - base_offset;
        messages.push(message);
    }
    messages
}

fn operation_state_group_table_before_counter_map(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    map_start: usize,
    base_offset: usize,
) -> Result<Option<OperationStateGroupTable>, CodecError> {
    #[derive(Clone, Copy)]
    struct GroupPath {
        last_candidate: usize,
        length: usize,
        first_start: usize,
    }

    if map_start > bytes.len() {
        return Ok(None);
    }
    let mut candidates = Vec::new();
    if let Some(range_end) = map_start.checked_sub(2) {
        for at in ctx.admit_iter(&(0..range_end), "nx operation-state group scan")? {
            if !matches!(bytes.get(at..at + 2), Some([0x01, 0x00 | 0x01])) {
                continue;
            }
            let Some(end) = operation_state_group_end_at(ctx, bytes, at, map_start, base_offset)?
            else {
                continue;
            };
            ctx.charge_collection_items(1, "nx operation-state group candidates")?;
            ctx.reserve_capacity(&mut candidates, 1, "nx operation-state group candidates")?;
            candidates.push((at, end));
        }
    }
    ctx.stable_sort_by_key(
        &mut candidates,
        |value| {
            let (left_start, left_end) = value;
            (*left_end, *left_start)
        },
        Ord::cmp,
        "sort NX operation state group candidates",
    )?;

    let predecessor_bytes = candidates
        .len()
        .checked_mul(std::mem::size_of::<Option<usize>>())
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "nx operation-state group predecessors",
                0,
                cadmpeg_core::decode::u64_from_index(candidates.len()),
            )
        })?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(predecessor_bytes),
        "nx operation-state group predecessors",
    )?;
    let mut predecessors = ctx.alloc_filled(
        candidates.len(),
        None,
        "nx operation-state group predecessors",
    )?;
    let mut best_by_end = BTreeMap::<usize, GroupPath>::new();
    for (candidate_index, (start, end)) in candidates.iter().enumerate() {
        ctx.charge_work(1, "nx operation-state group paths")?;
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
            if !best_by_end.contains_key(end) {
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(usize, GroupPath)>()),
                    "nx operation-state group paths",
                )?;
            }
            ctx.insert_btree_map(
                &mut best_by_end,
                *end,
                path,
                "nx operation-state group paths",
            )?;
        }
    }

    let trailing_start =
        if map_start >= 2 && bytes.get(map_start - 2..map_start) == Some(&[0x01, 0x01]) {
            map_start - 2
        } else {
            map_start
        };
    let Some(terminal) = best_by_end.get(&trailing_start).copied() else {
        return Ok(None);
    };
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(terminal.length),
        "nx operation-state group path",
    )?;
    let mut path = Vec::new();
    ctx.reserve_capacity(&mut path, terminal.length, "nx operation-state group path")?;
    let mut candidate = Some(terminal.last_candidate);
    while let Some(candidate_index) = candidate {
        path.push(candidate_index);
        candidate = predecessors[candidate_index];
    }
    ctx.reverse(&mut path, "NX operation-state group path reversal")?;
    let Some(&last) = path.last() else {
        return Ok(None);
    };
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(path.len()),
        "nx operation-state groups",
    )?;
    let mut groups = Vec::new();
    ctx.reserve_capacity(&mut groups, path.len(), "nx operation-state groups")?;
    for candidate in ctx
        .admit_iter(&path, "NX selected state group path traversal")?
        .copied()
    {
        let Some(group) =
            operation_state_group_at(ctx, bytes, candidates[candidate].0, map_start, base_offset)?
        else {
            return Ok(None);
        };
        groups.push(group);
    }
    let Some(trailing) = bytes.get(candidates[last].1..map_start) else {
        return Ok(None);
    };
    OperationStateGroupTable::new(ctx, groups, trailing)
}

/// Decode a complete bounded `m_rollForwardStates` group table.
#[cfg(test)]
fn operation_state_group_table(
    bytes: &[u8],
    start: usize,
    end: usize,
    base_offset: usize,
) -> Option<OperationStateGroupTable> {
    crate::test_support::with_decode_context_over(
        bytes,
        |_| {},
        |ctx| {
            if start >= end || end > bytes.len() {
                return None;
            }
            let mut groups = Vec::new();
            let mut at = start;
            let mut trailing_start = end;
            while at < end {
                let Some(group) = operation_state_group_at(ctx, bytes, at, end, base_offset)
                    .expect("test group allocation is admitted")
                else {
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
            OperationStateGroupTable::new(ctx, groups, bytes.get(trailing_start..end)?).unwrap()
        },
    )
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
fn audit_trail_rows(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    end: usize,
    base_offset: usize,
) -> Result<Option<Vec<AuditTrailRow>>, CodecError> {
    if start >= end || end > bytes.len() {
        return Ok(None);
    }
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(end - start),
        "scan NX audit-trail rows",
    )?;
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
            return Ok(None);
        }
        previous_ordinal = Some(ordinal);
        at = row.local_end();
        ctx.reserve_vec(&mut rows, 1, "NX audit-trail rows")?;
        rows.push(row);
    }
    Ok(Some(rows))
}

fn operation_state_journal_start(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    product_end: usize,
) -> Result<Option<usize>, CodecError> {
    (|| {
        let tail = bytes.get(product_end..)?;
        let marker = propagate_resource!(ctx
            .admit_iter(tail, "NX state journal marker")
            .map_err(CodecError::from))
        .enumerate()
        .skip(1)
        .position(|(end, _)| tail[end - 1..end + 1] == [0x41, 0x00])?
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
        (count_tokens > 0 && (saw_repeated_token || count_tokens >= 2)).then_some(Ok(at))
    })()
    .transpose()
}

fn operation_state_journal_groups_before_boundary(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    end: usize,
    base_offset: usize,
) -> Result<Option<Vec<JournalGroup<usize>>>, CodecError> {
    if start >= end || end > bytes.len() {
        return Ok(None);
    }
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(end - start),
        "scan NX state-journal groups",
    )?;
    let mut groups = Vec::new();
    let mut at = start;
    let mut previous_ordinal = None;
    loop {
        let Some(group) = JournalGroup::read(ctx, bytes, at, end, base_offset)? else {
            let mut next = at;
            while bytes.get(next..next + 2) == Some(&[0x04, 0x00]) {
                next += 2;
            }
            if next == at || JournalGroup::read(ctx, bytes, next, end, base_offset)?.is_none() {
                break;
            }
            at = next;
            continue;
        };
        for row in ctx
            .admit_iter(group.rows().initial(), "NX journal initial rows")?
            .chain(ctx.admit_iter(
                std::slice::from_ref(group.rows().last()),
                "NX journal final row",
            )?)
        {
            let ordinal = row.ordinal().value();
            if previous_ordinal.is_some_and(|previous| ordinal <= previous) {
                return Ok(None);
            }
            previous_ordinal = Some(ordinal);
        }
        let Some(next) = group.end_offset().checked_sub(base_offset) else {
            return Ok(None);
        };
        at = next;
        ctx.reserve_vec(&mut groups, 1, "NX state-journal groups")?;
        groups.push(group);
    }
    Ok((!groups.is_empty()).then_some(groups))
}

/// Decode a complete bounded state journal.
#[cfg(test)]
fn operation_state_journal(
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
        let group = crate::test_support::with_decode_context(|ctx| {
            JournalGroup::read(ctx, bytes, at, end, base_offset)
        })
        .ok()??;
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
#[cfg(test)]
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
pub(crate) fn operation_common_frames(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<Vec<CommonFrame<usize>>, CodecError> {
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
    for start in ctx.admit_iter(&(0..record.payload().len()), "scan NX common frames")? {
        if let Some(frame) = decode(start, [1, 3, 2]) {
            ctx.reserve_vec(&mut frames, 1, "nx common frames")?;
            frames.push(frame);
        }
        if let Some(frame) = decode(start, [1, 1, 1]) {
            ctx.reserve_vec(&mut frames, 1, "nx common frames")?;
            frames.push(frame);
        }
    }
    ctx.stable_sort_by_key(
        &mut frames,
        CommonFrame::<usize>::offset,
        Ord::cmp,
        "sort NX common frames",
    )?;
    Ok(frames)
}

/// Decode the unique terminal common-frame suffix and its exact immediate common frame.
pub(crate) fn operation_terminal_frame(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<Option<OperationTerminalFrame>, CodecError> {
    let Some(terminator) = record.payload().len().checked_sub(1) else {
        return Ok(None);
    };
    if record.payload().get(terminator) != Some(&0) {
        return Ok(None);
    }
    let common_frames = operation_common_frames(ctx, record)?;
    let mut candidate = None;
    for start in ctx
        .admit_iter(
            &(0..terminator),
            "NX operation terminal frame range traversal",
        )?
        .rev()
        .take(9)
    {
        let parsed = (|| {
            let suffix = CommonFrameSuffix::read(record.payload().get(start..)?)?;
            (start + suffix.byte_len() == record.payload().len()).then_some(())?;
            let frame =
                TerminalFrame::<usize>::new(suffix, record.payload_offset().checked_add(start)?)?;
            let immediate_common_frame_offset = propagate_resource!(ctx
                .admit_iter(&common_frames, "NX terminal common frame lookup")
                .map_err(CodecError::from))
            .find(|common| {
                common.local_ordinal_offset() == frame.offset()
                    && common.end_offset() == frame.end_offset()
            })
            .map(CommonFrame::<usize>::offset);
            Some(Ok(OperationTerminalFrame {
                immediate_common_frame_offset,
                frame,
            }))
        })()
        .transpose()?;
        if let Some(parsed) = parsed {
            if candidate.is_some() {
                return Ok(None);
            }
            candidate = Some(parsed);
        }
    }
    Ok(candidate)
}

/// Decode ordered `04 00, object_index, 02 0b` references from one bounded block.
pub(crate) fn data_block_object_references(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<DataBlockObjectReference>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan NX data-block object references",
    )?;
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
        ctx.reserve_vec(&mut references, 1, "nx data-block object references")?;
        references.push(DataBlockObjectReference {
            offset: token,
            object_index,
        });
        at = end + 2;
    }
    Ok(references)
}

fn boolean_operations_with_labels(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    base_offset: usize,
    labels: &[OperationLabel<'_>],
) -> Result<Vec<BooleanOperation>, CodecError> {
    const BODY_HEADER: &[u8] = &[
        0x31, 0x00, 0x00, 0x01, 0x00, 0x14, 0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b, 0x03,
        0x00, 0x00, 0xe0, 0x7f, 0xff, 0xff, 0xff, 0x01, 0x01,
    ];
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(labels.len()),
        "scan NX Boolean operation labels",
    )?;
    let mut operations = Vec::new();
    for label in labels.iter().copied() {
        let mut failure = None;
        let operation = (|| {
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
            let (targets, next) = match counted_feature_object_indices(
                ctx,
                bytes,
                base_offset,
                label_end + BODY_HEADER.len(),
            ) {
                Ok(Some(values)) => values,
                Ok(None) => return None,
                Err(error) => {
                    failure = Some(error);
                    return None;
                }
            };
            if targets.len() != 1 || bytes.get(next) != Some(&0) {
                return None;
            }
            let (tools, end) =
                match counted_feature_object_indices(ctx, bytes, base_offset, next + 1) {
                    Ok(Some(values)) => values,
                    Ok(None) => return None,
                    Err(error) => {
                        failure = Some(error);
                        return None;
                    }
                };
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
        })();
        if let Some(error) = failure {
            return Err(error);
        }
        if let Some(operation) = operation {
            ctx.reserve_vec(&mut operations, 1, "NX Boolean operations")?;
            operations.push(operation);
        }
    }
    Ok(operations)
}

fn counted_feature_object_indices(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    base_offset: usize,
    at: usize,
) -> Result<Option<(Vec<PayloadObjectReference>, usize)>, CodecError> {
    if bytes.get(at) != Some(&0x01) {
        return Ok(None);
    }
    let Some(count) = bytes
        .get(at + 1)
        .and_then(|value| usize::from(*value).checked_sub(1))
    else {
        return Ok(None);
    };
    let values_start = at + 2;
    let mut scan_cursor = values_start;
    for _ in ctx.admit_iter(
        &(0..count),
        "NX counted feature object indices range traversal",
    )? {
        let Some((Some(_), next)) = feature_object_index(bytes, scan_cursor) else {
            return Ok(None);
        };
        scan_cursor = next;
    }

    let mut cursor = values_start;
    let mut values = Vec::new();
    for _ in ctx.admit_iter(
        &(0..count),
        "NX counted feature object indices range traversal",
    )? {
        let Some(value) = bytes
            .get(cursor..)
            .and_then(ReferenceIndexToken::read_feature)
        else {
            return Ok(None);
        };
        let next = cursor + value.raw().len();
        ctx.reserve_vec(&mut values, 1, "NX Boolean references")?;
        values.push(PayloadObjectReference {
            offset: base_offset + cursor,
            token: value,
        });
        cursor = next;
    }
    Ok(Some((values, cursor)))
}

/// Decode count-framed runs of same-section record references.
pub(crate) fn counted_record_references(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    base_offset: usize,
    record_count: usize,
) -> Result<Vec<LocatedReference<u16>>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan NX counted record references",
    )?;
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
        if end > bytes.len()
            || ctx
                .admit_iter(&(0..count), "NX counted record tag validation")?
                .any(|index| bytes[at + 2 + index * 3] != 0x90)
        {
            at += 1;
            continue;
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(count),
            "validate NX counted record references",
        )?;
        if (0..count).any(|index| {
            let token = at + 2 + index * 3;
            View::u16_be_at(bytes, token + 1).is_none_or(|value| usize::from(value) >= record_count)
        }) {
            at += 1;
            continue;
        }
        for index in ctx.admit_iter(&(0..count), "NX counted record references range traversal")? {
            let token = at + 2 + index * 3;
            let Some(value) = View::u16_be_at(bytes, token + 1) else {
                break;
            };
            ctx.reserve_vec(&mut references, 1, "NX counted record references")?;
            references.push(LocatedReference {
                offset: base_offset + token,
                value,
            });
        }
        at = end;
    }
    Ok(references)
}

/// Decode self-identifying persistent handles and exact adjacent handle pairs.
fn record_references(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    base_offset: usize,
) -> Result<Vec<LocatedReference<DirectReference>>, CodecError> {
    let parsed = references(ctx, bytes, base_offset)?;
    let mut out = Vec::new();
    for reference in ctx
        .admit_iter(&parsed, "NX persistent record references")?
        .copied()
        .filter(|reference| matches!(reference.value, DirectReference::PersistentHandle(_)))
    {
        ctx.reserve_vec(&mut out, 1, "NX record references")?;
        out.push(reference);
    }
    for (index, tagged) in ctx
        .admit_iter(&parsed, "NX paired record references")?
        .enumerate()
        .skip(1)
    {
        let persistent = &parsed[index - 1];
        let adjacent = persistent
            .offset
            .checked_add(5)
            .is_some_and(|offset| tagged.offset == offset);
        if matches!(persistent.value, DirectReference::PersistentHandle(_))
            && matches!(tagged.value, DirectReference::Tagged28(_))
            && adjacent
        {
            ctx.reserve_vec(&mut out, 1, "NX record references")?;
            out.push(*tagged);
        }
    }
    ctx.stable_sort_by(
        &mut out,
        |value| &value.offset,
        Ord::cmp,
        "sort NX direct references",
    )?;
    Ok(out)
}

/// Decode tagged references wholly contained in `bytes`.
pub(crate) fn references(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    base_offset: usize,
) -> Result<Vec<LocatedReference<DirectReference>>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan NX tagged references",
    )?;
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        if bytes[at] == 0xe0 {
            if let Some(value) = View::u32_be_at(bytes, at + 1) {
                ctx.reserve_vec(&mut out, 1, "NX tagged references")?;
                out.push(LocatedReference {
                    offset: base_offset + at,
                    value: DirectReference::PersistentHandle(value),
                });
                at += 5;
                continue;
            }
        } else if bytes[at] & 0xf0 == 0xc0 {
            if let Some(value) = View::u32_be_at(bytes, at) {
                ctx.reserve_vec(&mut out, 1, "NX tagged references")?;
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
    Ok(out)
}

/// Decode `66 32 03` printable-string values wholly contained in `bytes`.
pub(crate) fn string_values<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    base_offset: usize,
) -> Result<Vec<StringValue<'a>>, CodecError> {
    const MARKER: &[u8] = &[0x66, 0x32, 0x03];
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan NX printable strings",
    )?;
    let mut values = Vec::new();
    for (offset, _) in bytes
        .windows(MARKER.len())
        .enumerate()
        .filter(|(_, window)| *window == MARKER)
    {
        let value = (|| {
            let declared = usize::from(*bytes.get(offset + 3)?);
            let text_len = declared.checked_sub(2)?;
            let start = offset.checked_add(4)?;
            let end = start.checked_add(text_len)?;
            let raw = bytes.get(start..end)?;
            (bytes.get(end) == Some(&0)).then_some(())?;
            let value = propagate_resource!(PrintableString::from_wire(
                ctx,
                propagate_resource!(ctx.validate_utf8(raw, "NX printable string UTF-8 validation"))
                    .ok()?
            ))
            .ok()?;
            Some(Ok(StringValue {
                offset: base_offset + offset,
                value,
            }))
        })()
        .transpose()?;
        if let Some(value) = value {
            ctx.reserve_vec(&mut values, 1, "NX printable strings")?;
            values.push(value);
        }
    }
    Ok(values)
}

/// Decode complete `03 26, canonical UUID text, 00` values in `bytes`.
pub(crate) fn uuid_string_values<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    base_offset: usize,
) -> Result<Vec<UuidStringValue<'a>>, CodecError> {
    const MARKER: &[u8] = &[0x03, 0x26];
    const TEXT_LEN: usize = 36;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan NX UUID strings",
    )?;
    let mut values = Vec::new();
    for (offset, _) in bytes
        .windows(MARKER.len())
        .enumerate()
        .filter(|(_, window)| *window == MARKER)
    {
        let Some(start) = offset.checked_add(MARKER.len()) else {
            continue;
        };
        let Some(end) = start.checked_add(TEXT_LEN) else {
            continue;
        };
        let Some(raw) = bytes.get(start..end) else {
            continue;
        };
        let Ok(text) = ctx.validate_utf8(raw, "NX UUID text UTF-8 validation")? else {
            continue;
        };
        let Ok(value) = crate::canonical_uuid::CanonicalUuid::from_wire(ctx, text)? else {
            continue;
        };
        if bytes.get(end) != Some(&0) {
            continue;
        }
        let Some(absolute_offset) = base_offset.checked_add(offset) else {
            continue;
        };
        ctx.reserve_vec(&mut values, 1, "nx UUID strings")?;
        values.push(UuidStringValue {
            offset: absolute_offset,
            value,
        });
    }
    Ok(values)
}

#[cfg(test)]
mod uuid_string_value_tests {
    use super::uuid_string_values;

    #[test]
    fn decodes_only_complete_canonical_uuid_frames() {
        crate::test_support::with_decode_context(|ctx| {
            let mut bytes = b"prefix\x03\x2601234567-89ab-cdef-0123-456789abcdef\0suffix".to_vec();
            let values = uuid_string_values(ctx, &bytes, 100).unwrap();
            assert_eq!(values.len(), 1);
            assert_eq!(values[0].offset, 106);
            assert_eq!(
                values[0].value.as_str(),
                "01234567-89ab-cdef-0123-456789abcdef"
            );

            bytes[6 + 2 + 9] = b'A';
            assert!(uuid_string_values(ctx, &bytes, 0).unwrap().is_empty());
            assert!(crate::canonical_uuid::CanonicalUuid::new(
                "01234567-89ab-cdef-0123-456789abcde"
            )
            .is_err());
            assert!(crate::canonical_uuid::CanonicalUuid::new(
                "01234567-89ab-cdef-0123_456789abcdef"
            )
            .is_err());
            assert!(crate::canonical_uuid::CanonicalUuid::new(
                "01234567-89ab-cdef-0123-456789abcdeg"
            )
            .is_err());
        });
    }

    #[test]
    fn uuid_frames_refuse_collection_limit() {
        let bytes = b"\x03\x2601234567-89ab-cdef-0123-456789abcdef\0";

        crate::test_support::with_decode_context_over(
            bytes,
            |policy| {
                policy.limits.max_collection_items = 0;
            },
            |ctx| {
                let error = uuid_string_values(ctx, bytes, 0).unwrap_err();
                assert!(
                    matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
                );
            },
        );
    }

    #[test]
    fn uuid_frames_refuse_retained_limit() {
        let bytes = b"\x03\x2601234567-89ab-cdef-0123-456789abcdef\0";

        crate::test_support::with_decode_context_over(
            bytes,
            |policy| {
                policy.limits.max_retained_bytes = 0;
            },
            |ctx| {
                let error = uuid_string_values(ctx, bytes, 0).unwrap_err();
                assert!(
                    matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
                );
            },
        );
    }

    #[test]
    fn uuid_frames_refuse_work_limit() {
        let bytes = b"\x03\x2601234567-89ab-cdef-0123-456789abcdef\0";

        crate::test_support::with_decode_context_over(
            bytes,
            |policy| {
                policy.limits.max_work_units = 0;
            },
            |ctx| {
                let error = uuid_string_values(ctx, bytes, 0).unwrap_err();
                assert!(
                    matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
                );
            },
        );
    }

    #[test]
    fn rejects_truncated_or_unterminated_uuid_frames() {
        crate::test_support::with_decode_context(|ctx| {
            let frame = b"\x03\x2601234567-89ab-cdef-0123-456789abcdef\0";
            assert!(uuid_string_values(ctx, &frame[..frame.len() - 1], 0)
                .unwrap()
                .is_empty());
            let mut unterminated = frame.to_vec();
            *unterminated.last_mut().expect("nonempty frame") = 1;
            assert!(uuid_string_values(ctx, &unterminated, 0)
                .unwrap()
                .is_empty());
        });
    }
}

/// Decode `66 1b 03, byte-length, printable UTF-8, 00` values in `bytes`.
pub(crate) fn surface_payload_strings<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
) -> Result<Vec<SurfacePayloadString<'a>>, CodecError> {
    const MARKER: &[u8] = &[0x66, 0x1b, 0x03];
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "scan NX surface payload strings",
    )?;
    let mut strings = Vec::new();
    for (offset, _) in bytes
        .windows(MARKER.len())
        .enumerate()
        .filter(|(_, window)| *window == MARKER)
    {
        let Some(text_len) = bytes.get(offset + MARKER.len()).copied().map(usize::from) else {
            continue;
        };
        let Some(start) = offset.checked_add(MARKER.len() + 1) else {
            continue;
        };
        let Some(end) = start.checked_add(text_len) else {
            continue;
        };
        let Some(raw) = bytes.get(start..end) else {
            continue;
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(raw.len()),
            "parse NX surface payload string",
        )?;
        let Ok(text) = std::str::from_utf8(raw) else {
            continue;
        };
        let Ok(value) = crate::payload_text::PayloadText::from_wire(ctx, text)? else {
            continue;
        };
        if bytes.get(end) != Some(&0) {
            continue;
        }
        let value = SurfacePayloadString { offset, value };
        ctx.reserve_vec(&mut strings, 1, "nx surface payload strings")?;
        strings.push(value);
    }
    Ok(strings)
}

/// Decode every strictly length-framed numeric expression in an OM payload.
///
/// The `hostglobalvariables` marker identifies the owning table. Individual
/// records are self-framed as `handle, 04, length, text, 00`, so expression
/// decoding does not depend on an object-id table having the same cardinality
/// as an external entity-index array.
pub(crate) fn numeric_expressions<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
) -> Result<Vec<NumericExpression<'a>>, CodecError> {
    if !ctx
        .admit_iter(bytes, "NX numeric expression table marker")?
        .enumerate()
        .skip(b"hostglobalvariables".len() - 1)
        .any(|(end, _)| {
            &bytes[end + 1 - b"hostglobalvariables".len()..end + 1] == b"hostglobalvariables"
        })
    {
        return Ok(Vec::new());
    }
    let mut expressions = Vec::new();
    for (end, _) in ctx
        .admit_iter(bytes, "NX numeric expression marker traversal")?
        .enumerate()
        .skip(b"(Number [".len() - 1)
        .filter(|(end, _)| &bytes[end + 1 - b"(Number [".len()..end + 1] == b"(Number [")
    {
        let offset = end + 1 - b"(Number [".len();
        let Some(start) = offset.checked_sub(3) else {
            continue;
        };
        let Some(expression) = numeric_expression_at(ctx, &bytes[start..], start, None)? else {
            continue;
        };
        ctx.reserve_vec(&mut expressions, 1, "nx numeric expressions")?;
        expressions.push(expression);
    }
    Ok(expressions)
}

/// Locate independently size-framed OM sections and their type registries.
pub(crate) fn sections<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
) -> Result<Vec<Section<'a>>, CodecError> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 16 <= bytes.len() {
        let tail = &bytes[at..];
        let Some(relative) = ctx
            .admit_iter(tail, "nx framed OM section scan")?
            .enumerate()
            .skip(3)
            .position(|(end, _)| tail[end - 3..end + 1] == [0xff; 4])
        else {
            break;
        };
        let offset = at + relative;
        let Some(payload_len) =
            View::u32_be_at(bytes, offset + 8).map(cadmpeg_core::decode::index_from_u32)
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
        let type_registry = registry::type_registry(ctx, bytes, offset + 16, end)?;
        let types = type_registry.definitions;
        let field_start = type_registry.field_start;
        let record_area_pointer =
            match section_record_area_pointer(ctx, bytes, offset, field_start, end)? {
                Some(pointer) => Some(pointer),
                None => legacy_feature_record_area_pointer(
                    ctx,
                    bytes,
                    offset,
                    field_start,
                    end,
                    &types,
                )?,
            };
        let (fields, record_area_offset) =
            if let Some((record_area_offset, pointer_offset)) = record_area_pointer {
                (
                    registry::all_field_definitions(ctx, bytes, field_start, pointer_offset)?,
                    Some(record_area_offset),
                )
            } else {
                (
                    registry::field_definitions(ctx, bytes, field_start, end)?,
                    None,
                )
            };
        let record_area = record_area_offset.map(|start| RecordArea {
            offset: start,
            bytes: &bytes[start..end],
        });
        let cached_operation_labels = match record_area {
            Some(area) => operation_labels(ctx, area.bytes, area.offset)?,
            None => Vec::new(),
        };
        ctx.reserve_vec(&mut out, 1, "nx framed OM sections")?;
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
    Ok(out)
}

fn section_record_area_pointer(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    section_offset: usize,
    schema_start: usize,
    section_end: usize,
) -> Result<Option<(usize, usize)>, CodecError> {
    (|| {
        let mut matches = (schema_start..section_end.checked_sub(3)?).filter_map(|at| {
            let relative = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
            let target = section_offset.checked_add(relative)?;
            (target >= at.checked_add(4)? && target.checked_add(15)? <= section_end)
                .then_some(())?;
            propagate_resource!(ProductRecord::read(
                ctx,
                bytes.get(target.checked_add(12)?..section_end)?,
                ProductRecordForm::Modern,
            ))
            .is_some()
            .then_some(Ok((target, at)))
        });
        let first = propagate_resource!(matches.next().transpose())?;
        propagate_resource!(matches.next().transpose())
            .is_none()
            .then_some(Ok(first))
    })()
    .transpose()
}

fn legacy_feature_record_area_pointer(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    section_offset: usize,
    schema_start: usize,
    section_end: usize,
    types: &[TypeDefinition<'_>],
) -> Result<Option<(usize, usize)>, CodecError> {
    (|| {
        if !propagate_resource!(ctx
            .admit_iter(types, "NX legacy feature role scan")
            .map_err(CodecError::from))
        .any(|definition| definition.name == "UGS::FEATURE_RECORD")
        {
            return None;
        }
        let mut candidates = (schema_start..section_end.checked_sub(4)?).filter_map(|at| {
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
            propagate_resource!(ProductRecord::read(
                ctx,
                bytes.get(target + 12..section_end)?,
                ProductRecordForm::LegacyFeature,
            ))?;
            Some(Ok((target, at)))
        });
        let first = propagate_resource!(candidates.next().transpose())?;
        propagate_resource!(candidates.next().transpose())
            .is_none()
            .then_some(Ok(first))
    })()
    .transpose()
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

fn product_record_range_at(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    offset: usize,
) -> Result<Option<ProductRecordRange>, CodecError> {
    (|| {
        let suffix = bytes.get(offset..)?;
        let layout =
            propagate_resource!(ProductRecord::read(ctx, suffix, ProductRecordForm::Modern))?;
        Some(Ok(ProductRecordRange {
            start: offset,
            end: offset.checked_add(layout.byte_len())?,
        }))
    })()
    .transpose()
}

fn record_area_product_end(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    offset: usize,
) -> Result<Option<usize>, CodecError> {
    (|| {
        let suffix = bytes.get(offset..)?;
        let layout = match propagate_resource!(ProductRecord::read(
            ctx,
            suffix,
            ProductRecordForm::Modern
        )) {
            Some(layout) => layout,
            None => propagate_resource!(ProductRecord::read(
                ctx,
                suffix,
                ProductRecordForm::LegacyFeature
            ))?,
        };
        offset.checked_add(layout.byte_len()).map(Ok)
    })()
    .transpose()
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
    let Some(contained) = ranges.get(first..end) else {
        return 0;
    };
    contained.len()
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
fn select_outer_indexed_candidates<'a>(
    ctx: &DecodeContext<'_>,
    mut candidates: Vec<IndexedCandidate<'a>>,
) -> Result<Vec<IndexedCandidate<'a>>, CodecError> {
    ctx.stable_sort_by_key(
        &mut candidates,
        |value| (value.start(), std::cmp::Reverse(value.source().len())),
        Ord::cmp,
        "nx indexed OM candidates outer sort",
    )?;
    let mut furthest_end = 0;
    candidates.retain(|candidate| {
        if candidate.source().len() <= furthest_end {
            return false;
        }
        furthest_end = candidate.source().len();
        true
    });
    ctx.stable_sort_by(
        &mut candidates,
        |value| &value.discovery_order,
        Ord::cmp,
        "nx indexed OM candidates discovery sort",
    )?;
    Ok(candidates)
}

fn materialize_indexed_candidate<'a>(
    ctx: &DecodeContext<'_>,
    candidate: IndexedCandidate<'a>,
) -> Result<IndexedSection<'a>, CodecError> {
    let bytes = candidate.source();
    let entity_index_offset = candidate.start();
    let base = match &candidate.kind {
        IndexedCandidateKind::Fixed(index) => index.base(),
        IndexedCandidateKind::OffsetOnly(_) => 0,
    };
    let type_registry = registry::type_registry(ctx, bytes, base, entity_index_offset)?;
    let fields = registry::all_field_definitions(
        ctx,
        bytes,
        type_registry.field_start,
        entity_index_offset,
    )?;
    let (object_id_table_offset, store) = match candidate.kind {
        IndexedCandidateKind::Fixed(index) => (
            index.object_id_table_offset(),
            IndexedStore::Fixed {
                records: {
                    let mut records = Vec::new();
                    for record in index.records(ctx)? {
                        ctx.reserve_vec(&mut records, 1, "nx fixed OM records")?;
                        records.push(record);
                    }
                    records.into()
                },
            },
        ),
        IndexedCandidateKind::OffsetOnly(index) => {
            let control = index.control();
            (
                control.offset,
                IndexedStore::OffsetOnly {
                    control,
                    column_storage: index.column_storage(),
                    records: {
                        let mut records = Vec::new();
                        for record in index.records(ctx)? {
                            ctx.reserve_vec(&mut records, 1, "nx offset OM records")?;
                            records.push(record);
                        }
                        records.into()
                    },
                },
            )
        }
    };
    Ok(IndexedSection {
        base,
        entity_index_offset,
        object_id_table_offset,
        types: type_registry.definitions.into(),
        fields: fields.into(),
        store,
    })
}

/// Locate validated NX OM entity-index/object-id-table pairs.
///
/// A candidate is accepted only when the arrays are adjacent, the index is
/// monotone, its first offset is zero, its second offset self-anchors the first
/// entity exactly at the end of the object-id table, and that entity carries the
/// NX root marker.
pub(crate) fn indexed_sections<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
) -> Result<Vec<IndexedSection<'a>>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "nx indexed OM section scan",
    )?;
    let mut temporary = ctx.reserve_scoped(0, "nx indexed OM candidate scan")?;
    let mut candidates = Vec::new();
    let mut seen_record_starts = BTreeSet::new();
    let mut product_record_ranges = Vec::new();
    for offset in 0..bytes.len() {
        if let Some(range) = product_record_range_at(ctx, bytes, offset)? {
            ctx.reserve_scoped_vec(
                &mut temporary,
                &mut product_record_ranges,
                1,
                "nx product record ranges",
            )?;
            product_record_ranges.push(range);
        }
    }
    let descending_u32_edges = DescendingU32Edges::new(ctx, &mut temporary, bytes)?;
    if let Some(range_end) = bytes.len().checked_sub(4) {
        for table in ctx.admit_iter(&(0..range_end), "NX indexed sections range traversal")? {
            let Some(count) =
                View::u32_le_at(bytes, table).map(cadmpeg_core::decode::index_from_u32)
            else {
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
            let Some(_) = product_record_range_at(ctx, bytes, table_end)? else {
                continue;
            };
            if View::u32_le_at(bytes, index_start) != Some(0) {
                continue;
            }
            let Some(first) =
                View::u32_le_at(bytes, index_start + 4).map(cadmpeg_core::decode::index_from_u32)
            else {
                continue;
            };
            let Some(base) = table_end.checked_sub(first) else {
                continue;
            };
            let Some(index) =
                FixedIndex::new(&descending_u32_edges, index_start, count, base, table)
            else {
                continue;
            };
            if seen_record_starts.contains(&table_end) {
                continue;
            }
            temporary.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                usize,
            >()))?;
            ctx.insert_btree_set(
                &mut seen_record_starts,
                table_end,
                "nx OM seen record starts",
            )?;
            ctx.reserve_scoped_vec(
                &mut temporary,
                &mut candidates,
                1,
                "nx indexed OM candidates",
            )?;
            candidates.push(IndexedCandidate {
                discovery_order: candidates.len(),
                kind: IndexedCandidateKind::Fixed(index),
            });
        }
    }
    if let Some(range_end) = bytes.len().checked_sub(4) {
        for count_offset in
            ctx.admit_iter(&(8..range_end), "NX indexed sections range traversal")?
        {
            let Some(record_count) =
                View::u32_le_at(bytes, count_offset).map(cadmpeg_core::decode::index_from_u32)
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
            let Some(first) =
                View::u32_le_at(bytes, index_start).map(cadmpeg_core::decode::index_from_u32)
            else {
                continue;
            };
            let Some(second) =
                View::u32_le_at(bytes, index_start + 4).map(cadmpeg_core::decode::index_from_u32)
            else {
                continue;
            };
            let Some(third) =
                View::u32_le_at(bytes, index_start + 8).map(cadmpeg_core::decode::index_from_u32)
            else {
                continue;
            };
            let Some(last) =
                View::u32_le_at(bytes, count_offset - 4).map(cadmpeg_core::decode::index_from_u32)
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
                product_record_count_within(&product_record_ranges, first, second)
                    .checked_add(product_record_count_within(
                        &product_record_ranges,
                        second,
                        third,
                    ))
                    .ok_or_else(|| {
                        CodecError::Malformed("NX product record count overflow".into())
                    })?;
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
            if seen_record_starts.contains(&second) {
                continue;
            }
            temporary.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                usize,
            >()))?;
            ctx.insert_btree_set(&mut seen_record_starts, second, "nx OM seen record starts")?;
            ctx.reserve_scoped_vec(
                &mut temporary,
                &mut candidates,
                1,
                "nx indexed OM candidates",
            )?;
            candidates.push(IndexedCandidate {
                discovery_order: candidates.len(),
                kind: IndexedCandidateKind::OffsetOnly(index),
            });
        }
    }
    let mut sections = Vec::new();
    for candidate in select_outer_indexed_candidates(ctx, candidates)? {
        let section = materialize_indexed_candidate(ctx, candidate)?;
        ctx.reserve_vec(&mut sections, 1, "nx indexed OM sections")?;
        sections.push(section);
    }
    Ok(sections)
}

/// Decode the first self-framed NX product/version marker in `bytes`.
pub(crate) fn store_version<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    base_offset: usize,
) -> Result<Option<StoreVersion<'a>>, CodecError> {
    let Some(last) = bytes.len().checked_sub(3) else {
        return Ok(None);
    };
    Ok(ctx
        .admit_iter(&(0..last), "NX store version marker search")?
        .find_map(|at| {
            let product = propagate_resource!(ProductRecord::read(
                ctx,
                &bytes[at..],
                ProductRecordForm::Modern
            ))?;
            Some(Ok(StoreVersion {
                offset: base_offset.checked_add(at)?,
                value: product.text(),
            }))
        })
        .transpose()?)
}

/// Decode the zero-prefixed offset-store control form as ordered 24-bit values.
///
/// Each word is serialized `00, value:u24 LE`. The complete form is atomic.
fn offset_store_control_values(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<NonEmpty<ControlWord24>>, CodecError> {
    if !bytes.len().is_multiple_of(4) {
        return Ok(None);
    }
    let Some(values) = NonEmpty::new_charged(
        ctx,
        bytes
            .chunks_exact(4)
            .map(|word| (word[0] == 0).then(|| ControlWord24::new([word[1], word[2], word[3]]))),
    )?
    else {
        return Ok(None);
    };
    values.transpose_charged(ctx)
}

/// Decode the distinct leading class-registry identities in an offset-store
/// control block.
///
/// The registry may omit declarations that this decoder cannot type, so its
/// retained declaration count is not an ordinal bound. The class lane is
/// instead the unique nonempty prefix whose identities are distinct and all
/// smaller than every following metadata value.
pub(crate) fn offset_store_control_class_ordinals(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<Vec<u32>>, cadmpeg_core::CodecError> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return Ok(None);
    }
    let value_at = |index: usize| {
        let start = index.checked_mul(4)?;
        let end = start.checked_add(4)?;
        let word = bytes.get(start..end)?;
        (word[0] == 0).then(|| ControlWord24::new([word[1], word[2], word[3]]).value())
    };
    let count = bytes.len() / 4;
    let count_u64 = cadmpeg_core::decode::u64_from_index(count);
    if ctx
        .admit_iter(&(0..count), "nx offset-store control validation")?
        .any(|index| value_at(index).is_none())
    {
        return Ok(None);
    }
    let scratch_bytes = count_u64
        .checked_mul(4)
        .ok_or_else(|| ctx.refuse_codec_limit("nx offset-store suffix minima", 0, count_u64))?;
    let _suffix_reservation = ctx.reserve_scoped(scratch_bytes, "nx offset-store suffix minima")?;
    let mut suffix_minima = ctx.alloc_filled(count, u32::MAX, "nx offset-store suffix minima")?;
    for index in ctx
        .admit_iter(
            &(0..count - 1),
            "NX offset store control class ordinals range traversal",
        )?
        .rev()
    {
        let Some(next) = value_at(index + 1) else {
            return Ok(None);
        };
        suffix_minima[index] = suffix_minima[index + 1].min(next);
    }
    let mut identities = BTreeSet::new();
    let mut maximum_identity = 0;
    let mut boundary = None;
    for (index, minimum) in ctx
        .admit_iter(&suffix_minima[..count - 1], "nx offset-store identity scan")?
        .enumerate()
    {
        let Some(identity) = value_at(index) else {
            return Ok(None);
        };
        if identities.contains(&identity) {
            break;
        }
        ctx.insert_btree_set(
            &mut identities,
            identity,
            "nx offset-store class identities",
        )?;
        maximum_identity = maximum_identity.max(identity);
        if maximum_identity < *minimum && boundary.replace(index + 1).is_some() {
            return Ok(None);
        }
    }
    let Some(boundary) = boundary else {
        return Ok(None);
    };
    let mut ordinals = ctx.collection_vec(boundary, "nx offset-store class ordinals")?;
    for index in ctx.admit_iter(
        &(0..boundary),
        "NX offset store control class ordinals range traversal",
    )? {
        let Some(identity) = value_at(index) else {
            return Ok(None);
        };
        ordinals.push(identity);
    }
    Ok(Some(ordinals))
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
    ctx: &DecodeContext<'_>,
    control: &[u8],
    first_record: &[u8],
) -> Result<Option<OffsetStoreControlForm>, CodecError> {
    let Some((product_offset, leading_width, leading_value)) = (|| {
        let mut product_offsets = (0..control.len())
            .filter_map(|offset| {
                propagate_resource!(ProductRecord::read(
                    ctx,
                    &control[offset..],
                    ProductRecordForm::Modern
                ))?;
                Some(Ok(offset))
            })
            .chain((0..first_record.len()).filter_map(|offset| {
                propagate_resource!(ProductRecord::read(
                    ctx,
                    &first_record[offset..],
                    ProductRecordForm::Modern
                ))?;
                Some(Ok(control.len() + offset))
            }));
        let product_offset = propagate_resource!(product_offsets.next().transpose())?;
        if propagate_resource!(product_offsets.next().transpose()).is_some() {
            return None;
        }
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
        Some(Ok((product_offset, leading_width, leading_value)))
    })()
    .transpose()?
    else {
        return Ok(None);
    };
    let Some(values) = NonEmpty::new_charged(
        ctx,
        (0..(product_offset - leading_width) / 4)
            .map(|index| joined_control_u32_le(control, first_record, leading_width + index * 4)),
    )?
    else {
        return Ok(None);
    };
    let Some(values) = values.transpose_charged(ctx)? else {
        return Ok(None);
    };
    Ok(Some(OffsetStoreControlForm::ProductAnchored {
        leading_value,
        values,
    }))
}

/// One complete admitted offset-only store control-block form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OffsetStoreControlForm {
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
pub(crate) fn offset_store_control_form(
    ctx: &DecodeContext<'_>,
    control: &[u8],
    first_record: Option<&[u8]>,
) -> Result<Option<OffsetStoreControlForm>, CodecError> {
    let zero = offset_store_control_values(ctx, control)?;
    let product =
        offset_store_product_anchored_form(ctx, control, first_record.unwrap_or_default())?;
    match (zero, product) {
        (Some(values), None) => Ok(Some(OffsetStoreControlForm::ZeroPrefixed { values })),
        (_, Some(form)) => Ok(Some(form)),
        (None, None) => Ok(None),
    }
}

fn numeric_expression_at<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    base_offset: usize,
    object_id: Option<u32>,
) -> Result<Option<NumericExpression<'a>>, CodecError> {
    const PREFIX: &[u8] = b"(Number [";
    let Some(relative) = ctx
        .admit_iter(bytes, "NX numeric expression prefix")?
        .enumerate()
        .skip(PREFIX.len() - 1)
        .position(|(end, _)| &bytes[end + 1 - PREFIX.len()..=end] == PREFIX)
    else {
        return Ok(None);
    };
    if relative < 3 || bytes.get(relative - 2) != Some(&0x04) {
        return Ok(None);
    }
    let Some(declared) = bytes.get(relative - 1).copied().map(usize::from) else {
        return Ok(None);
    };
    let Some(text_len) = declared.checked_sub(2) else {
        return Ok(None);
    };
    let Some(text_end) = relative.checked_add(text_len) else {
        return Ok(None);
    };
    if bytes.get(text_end) != Some(&0) {
        return Ok(None);
    }
    let Some(raw) = bytes.get(relative..text_end) else {
        return Ok(None);
    };
    let Ok(text) = ctx.validate_utf8(raw, "NX numeric expression UTF-8 validation")? else {
        return Ok(None);
    };
    let Some(text) = text.strip_prefix("(Number [") else {
        return Ok(None);
    };
    let Some((unit, rest)) = text.split_once("]) ") else {
        return Ok(None);
    };
    let Some(unit) = crate::om_tokens::unit_for(ctx, unit)? else {
        return Ok(None);
    };
    let Some((name, value_tail)) = rest.split_once(": ") else {
        return Ok(None);
    };
    if name.is_empty()
        || !ctx
            .admit_iter(name, "NX numeric expression name")?
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return Ok(None);
    }
    let Some((value_text, comment)) = value_tail.split_once("; ") else {
        return Ok(None);
    };
    if !comment.is_empty() && !numeric_expression_comment_is_valid(ctx, comment)? {
        return Ok(None);
    }
    Ok(Some(NumericExpression {
        object_id,
        offset: base_offset + relative,
        name: ParameterName::from_wire(ctx, name)?,
        unit,
        expression: value_text,
    }))
}

fn numeric_expression_comment_is_valid(
    ctx: &DecodeContext<'_>,
    comment: &str,
) -> Result<bool, CodecError> {
    Ok(comment.starts_with("//")
        && ctx
            .admit_iter(comment, "NX numeric expression comment")?
            .all(|ch| ch.is_ascii_graphic() || matches!(ch, ' ' | '\t' | '\r' | '\n')))
}

/// Evaluate the context-free arithmetic subset of NX numeric formulas.
/// Names and function calls fail; they need the parameter graph.
pub(crate) fn evaluate_constant_expression(
    ctx: &DecodeContext<'_>,
    text: &str,
) -> Result<Option<FiniteReal>, CodecError> {
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

    struct Parser<'a, 'b, 'c> {
        bytes: &'a [u8],
        ctx: &'b DecodeContext<'c>,
        failure: Option<CodecError>,
        at: usize,
        values: Vec<FiniteReal>,
        values_reservation: cadmpeg_core::decode::ScopedReservation<'b>,
        charged_values_len: usize,
        operators: Vec<Operator>,
        operators_reservation: cadmpeg_core::decode::ScopedReservation<'b>,
        charged_operators_len: usize,
        expect_operand: bool,
    }

    impl Parser<'_, '_, '_> {
        fn push_value(&mut self, value: FiniteReal) -> Option<()> {
            let charged = (|| -> Result<(), CodecError> {
                if self.values.len() >= self.charged_values_len {
                    self.values_reservation
                        .grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                            FiniteReal,
                        >(
                        )))?;
                    self.charged_values_len =
                        self.charged_values_len.checked_add(1).ok_or_else(|| {
                            self.ctx
                                .refuse_codec_limit("NX expression value stack", 0, u64::MAX)
                        })?;
                }
                self.ctx
                    .reserve_vec(&mut self.values, 1, "NX expression value stack")
            })();
            if let Err(error) = charged {
                self.failure = Some(error);
                return None;
            }
            self.values.push(value);
            Some(())
        }

        fn push_operator(&mut self, operator: Operator) -> Option<()> {
            let charged = (|| -> Result<(), CodecError> {
                if self.operators.len() >= self.charged_operators_len {
                    self.operators_reservation
                        .grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                            Operator,
                        >(
                        )))?;
                    self.charged_operators_len =
                        self.charged_operators_len.checked_add(1).ok_or_else(|| {
                            self.ctx
                                .refuse_codec_limit("NX expression operator stack", 0, u64::MAX)
                        })?;
                }
                self.ctx
                    .reserve_vec(&mut self.operators, 1, "NX expression operator stack")
            })();
            if let Err(error) = charged {
                self.failure = Some(error);
                return None;
            }
            self.operators.push(operator);
            Some(())
        }
        fn spaces(&mut self) {
            while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
                self.at += 1;
            }
        }

        fn number(&mut self) -> Option<FiniteReal> {
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
            let validated = match self.ctx.validate_utf8(
                &self.bytes[start..self.at],
                "NX constant expression UTF-8 validation",
            ) {
                Ok(validated) => validated,
                Err(error) => {
                    self.failure = Some(error);
                    return None;
                }
            };
            let text = validated.ok()?;
            let parsed = match self
                .ctx
                .parse_text::<f64>(text, "NX constant expression decimal parse")
            {
                Ok(parsed) => parsed,
                Err(error) => {
                    self.failure = Some(error);
                    return None;
                }
            };
            let value = parsed.ok()?;
            FiniteReal::new(value)
        }

        fn apply_top(&mut self) -> Option<()> {
            let operator = self.operators.pop()?;
            let value = match operator {
                Operator::OpenParen => return None,
                Operator::Unary(operator) => {
                    let value = self.values.pop()?;
                    match operator {
                        b'+' => value,
                        b'-' => value.negated(),
                        _ => return None,
                    }
                }
                Operator::Binary(operator) => {
                    let right = self.values.pop()?;
                    let left = self.values.pop()?;
                    let (left, right) = (left.get(), right.get());
                    let raw = match operator {
                        b'+' => left + right,
                        b'-' => left - right,
                        b'*' => left * right,
                        b'/' => left / right,
                        b'^' => left.powf(right),
                        _ => return None,
                    };
                    FiniteReal::new(raw)?
                }
            };
            self.push_value(value)?;
            Some(())
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
            self.push_operator(incoming)?;
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

        fn parse(&mut self) -> Option<FiniteReal> {
            while self.at < self.bytes.len() {
                self.spaces();
                if self.at == self.bytes.len() {
                    break;
                }
                let byte = *self.bytes.get(self.at)?;
                if self.expect_operand {
                    match byte {
                        b'+' | b'-' => {
                            self.push_operator(Operator::Unary(byte))?;
                            self.at += 1;
                        }
                        b'(' => {
                            self.push_operator(Operator::OpenParen)?;
                            self.at += 1;
                        }
                        _ => {
                            let value = self.number()?;
                            self.push_value(value)?;
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

        fn parse_with_refusal(mut self) -> Result<Option<FiniteReal>, CodecError> {
            let value = self.parse();
            if let Some(error) = self.failure {
                return Err(error);
            }
            Ok(value)
        }
    }

    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(text.len()),
        "NX numeric expression scan",
    )?;
    let values_reservation = ctx.reserve_scoped(0, "NX expression value stack")?;
    let operators_reservation = ctx.reserve_scoped(0, "NX expression operator stack")?;
    Parser {
        bytes: text.as_bytes(),
        ctx,
        failure: None,
        at: 0,
        values: Vec::new(),
        values_reservation,
        charged_values_len: 0,
        operators: Vec::new(),
        operators_reservation,
        charged_operators_len: 0,
        expect_operand: true,
    }
    .parse_with_refusal()
}

#[cfg(test)]
mod tests;
