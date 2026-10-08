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
    let mut components = [None; 3];
    for slot in &mut components {
        let offset = *at;
        let component = ColorComponent::read(bytes.get(offset..)?)?;
        *at += component.raw().len();
        *slot = Some((component, offset));
    }
    let [Some(first), Some(second), Some(third)] = components else {
        return None;
    };
    Some([first, second, third])
}

fn color_name_frame<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    offset: usize,
) -> Result<Option<(&'a str, usize)>, CodecError> {
    let Some(byte_len) = bytes.get(offset).copied().map(usize::from) else {
        return Ok(None);
    };
    if byte_len < 2 {
        return Ok(None);
    }
    let Some(end) = offset.checked_add(byte_len) else {
        return Ok(None);
    };
    let Some(text) = bytes
        .get(offset + 1..end)
        .and_then(|text| text.strip_suffix(&[0]))
    else {
        return Ok(None);
    };
    if text.is_empty()
        || !ctx.all_by(
            text,
            |byte| Ok(byte.is_ascii_graphic() || *byte == b' '),
            "NX color name syntax",
        )?
    {
        return Ok(None);
    }
    let Ok(text) = ctx.validate_utf8(text, "NX color name UTF-8 validation")? else {
        return Ok(None);
    };
    Ok(Some((text, byte_len)))
}

const COLOR_TABLE_NAME_HEADER: [u8; 4] = [0x02, 0x80, 0xd9, 0x01];
const COLOR_TABLE_DEFINITION_PREAMBLE: [u8; 21] = [
    0x02, 0x14, 0xff, 0x06, 0x00, 0xf0, 0x02, 0x80, 0x9d, 0x80, 0xc7, 0x00, 0xc0, 0x13, 0x0a, 0xc6,
    0x01, 0x80, 0xd9, 0x80, 0xc8,
];

fn color_table_at<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    start: usize,
) -> Result<Option<(ColorTable<'a>, usize)>, CodecError> {
    if bytes.get(start..start + COLOR_TABLE_NAME_HEADER.len()) != Some(&COLOR_TABLE_NAME_HEADER) {
        return Ok(None);
    }
    let mut at = start + COLOR_TABLE_NAME_HEADER.len();
    let mut names = [""; 217];
    for (ordinal, slot) in names.iter_mut().enumerate() {
        let Some((name, width)) = color_name_frame(ctx, bytes, at)? else {
            return Ok(None);
        };
        if ordinal == 0 && name != BACKGROUND_NAME {
            return Ok(None);
        }
        *slot = name;
        at += width;
    }
    if bytes.get(at..at + COLOR_TABLE_DEFINITION_PREAMBLE.len())
        != Some(&COLOR_TABLE_DEFINITION_PREAMBLE)
    {
        return Ok(None);
    }
    at += COLOR_TABLE_DEFINITION_PREAMBLE.len();
    let Some(background) = color_components(bytes, &mut at) else {
        return Ok(None);
    };
    let mut definitions = std::array::from_fn(|_| ColorTableDefinition {
        name: "",
        components: background,
        offset: start,
    });
    for color_index in PaletteIndex::all() {
        let offset = at;
        if bytes.get(at) != Some(&5) {
            return Ok(None);
        }
        at += 1;
        let (token, width) = color_index.definition_token();
        if bytes.get(at..at + width) != Some(&token[..width]) {
            return Ok(None);
        }
        at += width;
        if bytes.get(at..at + 3) != Some(&[1, 0x80, 0xc8]) {
            return Ok(None);
        }
        at += 3;
        let Some(components) = color_components(bytes, &mut at) else {
            return Ok(None);
        };
        definitions[usize::from(color_index.value()) - 1] = ColorTableDefinition {
            name: names[usize::from(color_index.value())],
            components,
            offset,
        };
    }
    Ok(Some((
        ColorTable {
            offset: start,
            background,
            definitions,
        },
        at,
    )))
}

/// Decode every complete NX part color table in a bounded byte region.
pub(crate) fn color_tables<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
) -> Result<Vec<ColorTable<'a>>, CodecError> {
    let mut tables = Vec::new();
    let mut start = 0;
    while start + COLOR_TABLE_NAME_HEADER.len() <= bytes.len() {
        ctx.charge_work(1, "scan NX part color tables")?;
        if bytes.get(start..start + COLOR_TABLE_NAME_HEADER.len()) != Some(&COLOR_TABLE_NAME_HEADER)
        {
            start += 1;
            continue;
        }
        let Some((table, end)) = color_table_at(ctx, bytes, start)? else {
            start += 1;
            continue;
        };
        ctx.push_vec(&mut tables, table, "nx part color tables")?;
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
    let mut byte_reservation = ctx.reserve_scoped(0, "NX named point block bytes")?;
    let mut candidate = None;
    let mut blocks = blocks.into_iter();
    let mut block_count = 0;
    while let Some(block) = ctx.next_charged(&mut blocks, "NX named point block traversal")? {
        block_count += 1;
        let mut lookahead_storage = ctx.reserve_scoped(0, "NX named point lookahead storage")?;
        if !bytes.is_empty()
            && lookahead_storage
                .with_storage(|| name_field::scan(ctx, block))?
                .first()
                .is_some_and(|name| name.code().is_none())
        {
            break;
        }
        byte_reservation
            .with_storage(|| ctx.extend_vec(&mut bytes, block, "NX named point block bytes"))?;
        let mut name_storage = ctx.reserve_scoped(0, "NX named point name scan storage")?;
        let names = name_storage.with_storage(|| name_field::scan(ctx, &bytes))?;
        let Some(name) = names.first() else {
            return Ok(None);
        };
        if name.code().is_some()
            || parse_positive_decimal_suffix(ctx, name.value(), "Point")?.is_none()
        {
            return Ok(None);
        }
        let next_name = ctx.find_by(
            &names,
            |next| Ok(next.code().is_some() && next.offset() > name.offset()),
            "NX named point following name lookup",
        )?;
        let interval_end = next_name.map_or(bytes.len(), name_field::NameField::offset);
        let mut scalar_storage = ctx.reserve_scoped(0, "NX named point scalar scan storage")?;
        let scalars =
            scalar_storage.with_storage(|| construction_payload_scalar_fields(ctx, &bytes))?;
        let mut scalars = scalars.into_iter();
        let mut next_scalar = || {
            ctx.find_by(
                &mut scalars,
                |scalar| {
                    Ok(scalar.offset > name.offset()
                        && scalar
                            .offset
                            .checked_add(SHIFTED_BINARY64_SCALAR_FRAME_LEN)
                            .is_some_and(|end| end <= interval_end))
                },
                "NX named point scalar witness lookup",
            )
        };
        match (next_scalar()?, next_scalar()?, next_scalar()?) {
            (Some(first), Some(second), None) => {
                if candidate.is_none() {
                    candidate = Some((
                        name.value().len(),
                        [first, second].map(|field| LocatedBinary64 {
                            scalar: field.scalar,
                            offset: field.offset,
                        }),
                        block_count,
                    ));
                }
            }
            (None, _, _) | (Some(_), None, _) => {}
            _ => return Ok(None),
        }
        if next_name.is_some() {
            break;
        }
    }
    let Some((name_len, values, block_count)) = candidate else {
        return Ok(None);
    };
    // Leading name text starts after its marker and declared-length byte.
    let Some(raw) = bytes.get(2..2 + name_len) else {
        return Ok(None);
    };
    let Ok(name) = ctx.validate_utf8(raw, "NX named point final UTF-8 validation")? else {
        return Ok(None);
    };
    let name = ctx.copy_retained_text(name, "NX named point name")?;
    Ok(Some(OffsetStoreNamedPoint {
        name,
        values,
        block_count,
    }))
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
        || !ctx.all_by(
            suffix.bytes(),
            |byte| Ok(byte.is_ascii_digit()),
            "NX point ordinal digits",
        )?
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
        self.numeric_expression_output(ctx, |_, expression| expression)
    }

    /// Decode expressions together with their owning record ordinal.
    pub(crate) fn numeric_expression_records(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<(usize, NumericExpression<'a>)>, CodecError> {
        self.numeric_expression_output(ctx, |ordinal, expression| (ordinal, expression))
    }

    fn numeric_expression_output<T>(
        &self,
        ctx: &DecodeContext<'_>,
        mut project: impl FnMut(usize, NumericExpression<'a>) -> T,
    ) -> Result<Vec<T>, CodecError> {
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
        let marker = b"hostglobalvariables";
        let has_host_globals = ctx.any_by(
            &records,
            |(_, bytes, _)| {
                ctx.any_by(
                    bytes
                        .len()
                        .checked_sub(marker.len() - 1)
                        .map_or(0..0, |end| 0..end),
                    |at| Ok(&bytes[at..at + marker.len()] == marker),
                    "NX numeric expression host globals marker",
                )
            },
            "NX numeric expression owner records",
        )?;
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
                expressions.push(project(record_ordinal, expression));
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
        let mut storage = ctx.reserve_scoped(0, "NX fixed record reference scan storage")?;
        let direct = storage.with_storage(|| record_references(ctx, self.bytes, self.offset))?;
        let counted = storage.with_storage(|| {
            counted_record_references(ctx, self.bytes, self.offset, record_count)
        })?;
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
        let Some((offset, bytes)) = self.record_area_parts() else {
            return Ok(None);
        };
        let (Some(first), Some(second), Some(third)) = (
            View::u32_le_at(bytes, 0),
            View::u32_le_at(bytes, 4),
            View::u32_le_at(bytes, 8),
        ) else {
            return Ok(None);
        };
        let Some(suffix) = bytes.get(12..) else {
            return Ok(None);
        };
        let layout = match ProductRecord::read(ctx, suffix, ProductRecordForm::Modern)? {
            Some(layout) => layout,
            None => {
                let Some(layout) =
                    ProductRecord::read(ctx, suffix, ProductRecordForm::LegacyFeature)?
                else {
                    return Ok(None);
                };
                layout
            }
        };
        Ok(Some(RecordAreaHeader {
            offset,
            control_words: [first, second, third],
            product: StoreVersion {
                offset: offset + 12,
                value: layout.text(),
            },
        }))
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
        let is_feature_history = ctx.any_by(
            self.types.as_ref(),
            |definition| Ok(definition.name == "UGS::FEATURE_RECORD"),
            "NX section counter type role",
        )? || ctx.any_by(
            self.fields.as_ref(),
            |definition| Ok(definition.name == "m_rollForwardStates"),
            "NX section roll-forward field role",
        )?;
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
        if !ctx.any_by(
            self.fields.as_ref(),
            |definition| Ok(definition.name == "m_rollForwardStates"),
            "NX section group-table field role",
        )? {
            return Ok(None);
        }
        let mut map_storage =
            ctx.reserve_scoped(0, "NX roll-forward counter boundary workspace")?;
        let Some(map) = map_storage.with_storage(|| self.operation_state_counter_map(ctx))? else {
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
        let is_feature_history = ctx.any_by(
            self.types.as_ref(),
            |definition| Ok(definition.name == "UGS::FEATURE_RECORD"),
            "NX section journal type role",
        )?;
        if !is_feature_history {
            return Ok(None);
        }
        let Some((base_offset, bytes)) = self.record_area_parts() else {
            return Ok(None);
        };
        let Some(header) = self.record_area_header(ctx)? else {
            return Ok(None);
        };
        let Some(product) = header.product.offset.checked_sub(base_offset) else {
            return Ok(None);
        };
        let Some(product_end) = record_area_product_end(ctx, bytes, product)? else {
            return Ok(None);
        };
        let Some(start) = operation_state_journal_start(ctx, bytes, product_end)? else {
            return Ok(None);
        };
        let end = self
            .cached_operation_labels
            .first()
            .and_then(|label| label.header.offset().checked_sub(base_offset))
            .unwrap_or(bytes.len());
        operation_state_journal_groups_before_boundary(ctx, bytes, start, end, base_offset)
    }

    fn operation_state_block<'ctx>(
        &self,
        ctx: &'ctx DecodeContext<'_>,
    ) -> Result<Option<OperationStateBlock<'a, 'ctx>>, CodecError> {
        let mut map_storage = ctx.reserve_scoped(0, "NX state block counter boundary workspace")?;
        let Some(map) = map_storage.with_storage(|| self.operation_state_counter_map(ctx))? else {
            return Ok(None);
        };
        let Some((base_offset, bytes)) = self.record_area_parts() else {
            return Ok(None);
        };
        let mut record_storage = ctx.reserve_scoped(0, "NX state block record storage")?;
        let records =
            record_storage.with_storage(|| self.operation_records_with_label_ordinals(ctx))?;
        let Some((_, last_record)) = records.last().copied() else {
            return Ok(None);
        };
        let start_offset = last_record.payload_offset();
        let Some(start) = start_offset.checked_sub(base_offset) else {
            return Ok(None);
        };
        let mut group_storage = ctx.reserve_scoped(0, "NX state block group boundary workspace")?;
        let group = group_storage.with_storage(|| self.operation_state_group_table(ctx))?;
        let Some(terminal) = group
            .as_ref()
            .map_or(map.offset(), OperationStateGroupTable::offset)
            .checked_sub(base_offset)
        else {
            return Ok(None);
        };
        let overlap_end = if let Some(table) = &group {
            let Some(end) = terminal.checked_add(table.groups().first().opener().bytes().len())
            else {
                return Ok(None);
            };
            Some(end)
        } else {
            None
        };
        for end in overlap_end.into_iter().chain(std::iter::once(terminal)) {
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
        let mut block_storage = ctx.reserve_scoped(0, "NX state message block workspace")?;
        let Some(block) = block_storage.with_storage(|| self.operation_state_block(ctx))? else {
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
        let has_audit_marker = ctx.any_by(
            self.types.as_ref(),
            |definition| Ok(definition.name == "UGS::OM::SaveAuditTrail"),
            "NX audit registry role",
        )?;
        let has_specialized_marker = ctx.any_by(
            self.types.as_ref(),
            |definition| {
                Ok(matches!(
                    definition.name,
                    "UGS::FEATURE_RECORD" | "UGS::EXP_expression" | "UGS::Solid::Topol"
                ))
            },
            "NX specialized registry role",
        )?;
        if !has_audit_marker || has_specialized_marker {
            return Ok(None);
        }
        let Some((base_offset, bytes)) = self.record_area_parts() else {
            return Ok(None);
        };
        let Some(header) = self.record_area_header(ctx)? else {
            return Ok(None);
        };
        let Some(product) = header.product.offset.checked_sub(base_offset) else {
            return Ok(None);
        };
        let Some(product_end) = record_area_product_end(ctx, bytes, product)? else {
            return Ok(None);
        };
        let Some(tail) = bytes.get(product_end..) else {
            return Ok(None);
        };
        let Some(relative) = ctx.position_by(
            1..tail.len(),
            |end| Ok(tail[end - 1..=end] == [0x41, 0x00]),
            "NX audit trail marker",
        )?
        else {
            return Ok(None);
        };
        let Some(start) = relative.checked_add(product_end + 2) else {
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
        let mut storage = ctx.reserve_scoped(0, "NX operation body record storage")?;
        let records = storage.with_storage(|| self.operation_records_with_label_ordinals(ctx))?;
        for (ordinal, record) in ctx.admit_iter(records, "NX operation body record traversal")? {
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
    let mut storage = ctx.reserve_scoped(0, "NX operation label header storage")?;
    let headers = storage.with_storage(|| validated_operation_headers(ctx, bytes, base_offset))?;
    for header in ctx
        .admit_iter(&headers, "NX operation label headers")?
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
    let mut headers = Vec::new();
    for marker in ctx.admit_iter(
        bytes
            .len()
            .checked_sub(PREFIX.len() - 1)
            .map_or(0..0, |end| 0..end),
        "scan NX operation headers",
    )? {
        if &bytes[marker..marker + PREFIX.len()] != PREFIX {
            continue;
        }
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
    let Some(at) = header.end_offset().checked_sub(base_offset) else {
        return Ok(None);
    };
    if bytes.get(at) != Some(&3) {
        return Ok(None);
    }
    let Some(length @ 3..) = bytes.get(at + 1).copied().map(usize::from) else {
        return Ok(None);
    };
    let Some(end) = at.checked_add(length) else {
        return Ok(None);
    };
    if bytes.get(end) != Some(&0) {
        return Ok(None);
    }
    let Some(name) = bytes.get(at + 2..end) else {
        return Ok(None);
    };
    if !ctx.all_by(
        name,
        |byte| Ok(byte.is_ascii_graphic() || *byte == b' '),
        "NX operation label text validation",
    )? {
        return Ok(None);
    }
    let Ok(value) = ctx.validate_utf8(name, "NX operation label UTF-8 validation")? else {
        return Ok(None);
    };
    Ok(Some(OperationLabel { header, value }))
}

fn operation_label_index<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    labels: &[OperationLabel<'a>],
) -> Result<
    (
        BTreeMap<usize, OperationLabel<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    let mut index = BTreeMap::new();
    let mut storage = ctx.reserve_scoped(0, "NX operation label index storage")?;
    for label in ctx.admit_iter(labels, "NX operation label index entries")? {
        storage.with_storage(|| {
            ctx.entry_btree_map(
                &mut index,
                label.header.offset(),
                "NX operation label index insertion",
            )?
            .or_insert(*label);
            Ok::<_, CodecError>(())
        })?;
    }
    Ok((index, storage))
}

fn operation_records_with_labels_and_ordinals<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    base_offset: usize,
    labels: &[OperationLabel<'a>],
) -> Result<Vec<(usize, OperationRecord<'a>)>, CodecError> {
    let mut header_storage = ctx.reserve_scoped(0, "NX labeled operation header storage")?;
    let headers =
        header_storage.with_storage(|| validated_operation_headers(ctx, bytes, base_offset))?;
    let (labels, _label_storage) = operation_label_index(ctx, labels)?;
    let mut records = Vec::new();
    for (ordinal, header) in ctx
        .admit_iter(&headers, "NX labeled operation record headers")?
        .enumerate()
    {
        let Some(label) = ctx.get_btree_map(
            &labels,
            &header.offset(),
            "NX labeled operation header lookup",
        )?
        else {
            continue;
        };
        let Some(start) = label.header.offset().checked_sub(base_offset) else {
            continue;
        };
        let end = headers
            .get(ordinal + 1)
            .map_or(bytes.len(), |next| next.offset() - base_offset);
        let Some(record_bytes) = bytes.get(start..end) else {
            continue;
        };
        if let Some(record) = OperationRecord::new(ctx, record_bytes, *label)? {
            ctx.push_vec(
                &mut records,
                (ordinal, record),
                "NX labeled operation records",
            )?;
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
    let mut header_storage = ctx.reserve_scoped(0, "NX unlabeled operation header storage")?;
    let headers =
        header_storage.with_storage(|| validated_operation_headers(ctx, bytes, base_offset))?;
    let (labels, _label_storage) = operation_label_index(ctx, labels)?;
    let mut records = Vec::new();
    for (ordinal, header) in ctx
        .admit_iter(&headers, "NX unlabeled operation record headers")?
        .enumerate()
    {
        if ctx.contains_key_btree_map(
            &labels,
            &header.offset(),
            "NX unlabeled operation header exclusion",
        )? {
            continue;
        }
        let Some(start) = header.offset().checked_sub(base_offset) else {
            continue;
        };
        let end = headers
            .get(ordinal + 1)
            .map_or(bytes.len(), |next| next.offset() - base_offset);
        let Some(record_bytes) = bytes.get(start..end) else {
            continue;
        };
        if let Some(record) = UnlabeledOperationRecord::new(*header, record_bytes) {
            ctx.push_vec(
                &mut records,
                (ordinal, record),
                "NX unlabeled operation records",
            )?;
        }
    }
    Ok(records)
}

/// Decode ordered `03|04, length, text, 00` frames from one operation payload.
pub(crate) fn operation_payload_text_frames<'a>(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'a>,
) -> Result<Vec<OperationPayloadTextFrame<'a>>, CodecError> {
    let mut frames = Vec::new();
    let mut at = 0usize;
    while at + 4 <= record.payload().len() {
        ctx.charge_work(1, "scan NX operation payload text")?;
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
        let Ok(text) = ctx.validate_utf8(raw, "NX payload text UTF-8 validation")? else {
            at += 1;
            continue;
        };
        let Ok(value) = crate::payload_text::PayloadText::from_wire(ctx, text)? else {
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
    let mut storage = ctx.reserve_scoped(0, "NX payload string frame storage")?;
    let frames = storage.with_storage(|| operation_payload_text_frames(ctx, record))?;
    for frame in ctx.admit_iter(&frames, "NX operation payload string frames")? {
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
    let mut template_storage = ctx.reserve_scoped(0, "NX hole template frame storage")?;
    let templates = template_storage.with_storage(|| operation_payload_strings(ctx, record))?;
    let mut templates = templates.into_iter();
    let Some(template) = ctx.find_by(
        &mut templates,
        |value| Ok(value.value.as_str().starts_with("Hole_")),
        "NX hole template lookup",
    )?
    else {
        return Ok(None);
    };
    if ctx
        .find_by(
            &mut templates,
            |value| Ok(value.value.as_str().starts_with("Hole_")),
            "NX hole template uniqueness",
        )?
        .is_some()
    {
        return Ok(None);
    }
    let Some(boundary) = template.offset.checked_sub(record.payload_offset()) else {
        return Ok(None);
    };
    let Some(prefix) = record.payload().get(..boundary) else {
        return Ok(None);
    };
    let mut scalar_storage = ctx.reserve_scoped(0, "NX hole scalar witness storage")?;
    let mut scalars = Vec::new();
    let mut at = 0usize;
    while at + 8 <= prefix.len() {
        ctx.charge_work(1, "scan NX simple hole scalars")?;
        if prefix[at] == 0x30 {
            if let Some(scalar) = ShiftedBinary64::read(&prefix[at..at + 8]) {
                scalar_storage.with_storage(|| {
                    ctx.reserve_vec(&mut scalars, 1, "NX simple hole scalar witnesses")
                })?;
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
    if ctx.any_by(
        first.iter().zip(second),
        |(left, right)| Ok(left.0 != right.0),
        "NX simple hole scalar witness equality",
    )? {
        return Ok(None);
    }
    let mut repeated = Vec::new();
    for (left, right) in ctx
        .admit_iter(first, "NX simple hole repeated scalars")?
        .zip(second)
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
    const PREFIX: [u8; 5] = [0, 0, 1, 0, 0];
    const ZEROES: [u8; 4] = [0; 4];
    const SUFFIX: [u8; 3] = [0, 0, 0xff];
    if record.name() != "HOLE PACKAGE" {
        return Ok(None);
    }
    let Some(last) = record.payload().len().checked_sub(PREFIX.len()) else {
        return Ok(None);
    };
    unique_payload_candidate(
        ctx,
        0..last,
        |start| {
            if record.payload().get(start..start + PREFIX.len()) != Some(&PREFIX) {
                return Ok(None);
            }
            let Some(selector) = record
                .payload()
                .get(start + 5)
                .copied()
                .and_then(NonZeroU8::new)
            else {
                return Ok(None);
            };
            let Some(branch) = record
                .payload()
                .get(start + 7)
                .copied()
                .and_then(NonZeroU8::new)
            else {
                return Ok(None);
            };
            if record.payload().get(start + 6) != Some(&0)
                || record.payload().get(start + 8..start + 12) != Some(&ZEROES)
            {
                return Ok(None);
            }
            let mut at = start + 12;
            let mut references = [const { None }; 4];
            for (ordinal, slot) in references.iter_mut().enumerate() {
                if ordinal == 2 {
                    if record.payload().get(at) != Some(&branch.get())
                        || record.payload().get(at + 1..at + 5) != Some(&ZEROES)
                    {
                        return Ok(None);
                    }
                    at += 5;
                }
                let reference_offset = at;
                let Some((object_index, width)) =
                    record.payload().get(at..).and_then(payload_object_index)
                else {
                    return Ok(None);
                };
                at += width;
                *slot = Some(PayloadObjectReference {
                    offset: record.payload_offset() + reference_offset,
                    token: object_index,
                });
            }
            let [Some(a), Some(b), Some(c), Some(d)] = references else {
                return Ok(None);
            };
            if record.payload().get(at..at + SUFFIX.len()) != Some(&SUFFIX) {
                return Ok(None);
            }
            Ok(Some(HolePackageConstructionGroupLane {
                offset: start,
                selector,
                branch,
                references: [a, b, c, d],
            }))
        },
        "NX hole package group candidate search",
        "NX hole group candidate storage",
    )
}

/// Retain one complete parse only after a second complete parse is excluded.
fn unique_payload_candidate<T, I: IntoIterator>(
    ctx: &DecodeContext<'_>,
    source: I,
    mut decode: impl FnMut(I::Item) -> Result<Option<T>, CodecError>,
    scan: &'static str,
    storage_operation: &'static str,
) -> Result<Option<T>, CodecError> {
    let mut source = source.into_iter();
    let first = ctx.find_map(
        &mut source,
        |input| {
            let mut storage = ctx.reserve_scoped(0, storage_operation)?;
            let candidate = storage.with_storage(|| decode(input))?;
            Ok(candidate.map(|candidate| (candidate, storage)))
        },
        scan,
    )?;
    let Some((candidate, storage)) = first else {
        return Ok(None);
    };
    if ctx
        .find_map(
            &mut source,
            |input| {
                let mut storage = ctx.reserve_scoped(0, storage_operation)?;
                Ok(storage.with_storage(|| decode(input))?.map(|_| ()))
            },
            scan,
        )?
        .is_some()
    {
        return Ok(None);
    }
    storage.commit()?;
    Ok(Some(candidate))
}

/// Decode the unique counted reference field in a bounded `SKETCH` payload.
pub(crate) fn sketch_payload_references<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<
    (
        Option<SketchReferenceField>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    let mut storage = ctx.reserve_scoped(0, "NX sketch reference scratch")?;
    if record.name() != "SKETCH" {
        return Ok((None, storage));
    }
    let Some(end) = record.payload().len().checked_sub(3) else {
        return Ok((None, storage));
    };
    let Some(shape) = unique_payload_candidate(
        ctx,
        0..end,
        |start| {
            if record.payload().get(start..start + 2) != Some(&[0x01, 0x00]) {
                return Ok(None);
            }
            SketchReferenceField::read(ctx, record, start)
        },
        "scan NX sketch reference fields",
        "NX sketch reference candidate storage",
    )?
    else {
        return Ok((None, storage));
    };
    let field = storage.with_storage(|| shape.materialize(ctx))?;
    Ok((Some(field), storage))
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
    enum RowValues {
        Scalar(PatternValue<ShiftedScalar, usize>),
        Wide(PatternWideValues<usize>),
    }
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
    let Some(end) = record.payload().len().checked_sub(1) else {
        return Ok(None);
    };
    unique_payload_candidate(
        ctx,
        (0..end).map(|start| (start, false)).chain(
            (0..if record.name() == "Pattern Feature" {
                end
            } else {
                0
            })
                .map(|start| (start, true)),
        ),
        |(start, wide)| {
            if wide && record.name() != "Pattern Feature" {
                return Ok(None);
            }
            if record.payload().get(start) != Some(&0x01) {
                return Ok(None);
            }
            let Some(declared_count @ 2..) = record.payload().get(start + 1).copied() else {
                return Ok(None);
            };
            let Some(row_schema_index) = record
                .payload()
                .get(start + 2)
                .copied()
                .and_then(NonZeroU8::new)
            else {
                return Ok(None);
            };
            let mut at = start + 2;
            let mut scalar_rows = Vec::new();
            let mut wide_rows = Vec::new();
            let mut ordinals = 1..declared_count;
            while !ordinals.is_empty() {
                let Some(ordinal) =
                    ctx.next_charged(&mut ordinals, "NX pattern transform row traversal")?
                else {
                    break;
                };
                if record.payload().get(at) != Some(&row_schema_index.get()) {
                    return Ok(None);
                }
                let values = if wide {
                    at += 1;
                    let mut decode_value = |value_ordinal| {
                        let offset = record.payload_offset() + at;
                        let scalar = ShiftedBinary64::read(record.payload().get(at..at + 8)?)?;
                        at += 8;
                        if value_ordinal == 1 {
                            if record.payload().get(at..at + 2) != Some(&[0, 0]) {
                                return None;
                            }
                            at += 2;
                        }
                        Some(PatternValue { scalar, offset })
                    };
                    let Some(first) = decode_value(0) else {
                        return Ok(None);
                    };
                    let Some(second) = decode_value(1) else {
                        return Ok(None);
                    };
                    let Some(third) = decode_value(2) else {
                        return Ok(None);
                    };
                    let Some(fourth) = decode_value(3) else {
                        return Ok(None);
                    };
                    let first = [first, second, third, fourth];
                    if record.payload().get(at..at + 4) != Some(&[0; 4]) {
                        return Ok(None);
                    }
                    at += 4;
                    let Some(scalar) = record.payload().get(at..).and_then(PatternTerminal::read)
                    else {
                        return Ok(None);
                    };
                    let terminal = PatternValue {
                        scalar,
                        offset: record.payload_offset() + at,
                    };
                    at += scalar.raw().len();
                    if record.payload().get(at..at + 7) != Some(&[0, 0, 0, 0, 1, 1, 3]) {
                        return Ok(None);
                    }
                    at += 7;
                    RowValues::Wide(PatternWideValues { first, terminal })
                } else {
                    if record.payload().get(at + 1..at + 1 + prefix_tail.len()) != Some(prefix_tail)
                    {
                        return Ok(None);
                    }
                    at += 1 + prefix_tail.len();
                    let Some(scalar) = record.payload().get(at..).and_then(ShiftedScalar::read)
                    else {
                        return Ok(None);
                    };
                    let value = PatternValue {
                        scalar,
                        offset: record.payload_offset() + at,
                    };
                    at += scalar.raw().len();
                    if record.payload().get(at..at + scalar_suffix.len()) != Some(scalar_suffix) {
                        return Ok(None);
                    }
                    at += scalar_suffix.len();
                    RowValues::Scalar(value)
                };
                let Some(atom) = record.payload().get(at..).and_then(CompactIndexAtom::read) else {
                    return Ok(None);
                };
                let selector = LocatedCompactIndex {
                    atom,
                    offset: record.payload_offset() + at,
                };
                at += atom.raw().len();
                if record.payload().get(at) != Some(&1)
                    || record.payload().get(at + 1) != Some(&ordinal)
                    || record.payload().get(at + 2..at + 2 + ROW_TAIL.len()) != Some(&ROW_TAIL)
                {
                    return Ok(None);
                }
                at += 2 + ROW_TAIL.len();
                match values {
                    RowValues::Scalar(values) => ctx.push_vec(
                        &mut scalar_rows,
                        PatternRow { values, selector },
                        "NX pattern scalar transform rows",
                    )?,
                    RowValues::Wide(values) => ctx.push_vec(
                        &mut wide_rows,
                        PatternRow { values, selector },
                        "NX pattern wide transform rows",
                    )?,
                }
            }
            if record.payload().get(at) != Some(&(row_schema_index.get() - 1)) {
                return Ok(None);
            }
            let rows = if wide {
                if record.payload().get(at + 1..at + 4) != Some(&[0, 0, 2]) {
                    return Ok(None);
                }
                let Ok(rows) = BranchItems::new(wide_rows) else {
                    return Ok(None);
                };
                PatternRows::Wide(rows)
            } else {
                if record.payload().get(at + 1..at + 3) != Some(&[0, 0])
                    || record.payload().get(at + 3) != Some(&1)
                {
                    return Ok(None);
                }
                let Ok(rows) = BranchItems::new(scalar_rows) else {
                    return Ok(None);
                };
                PatternRows::Scalar(rows)
            };
            Ok(Some(PatternPayloadTransformLane {
                offset: record.payload_offset() + start,
                row_schema_index,
                rows,
            }))
        },
        "scan NX pattern transform lanes",
        "NX pattern transform candidate storage",
    )
}

/// Decode the unique exactly counted instance-output lane in a bounded payload.
pub(crate) fn multi_instance_output_payload_lane(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<Option<MultiInstanceOutputPayloadLane>, CodecError> {
    const ENVELOPE: [u8; 10] = [0x3a, 0, 0, 1, 0, 0, 0, 0, 0x25, 1];
    const ROW_PREFIX: [u8; 7] = [0x26, 0x27, 1, 2, 0x65, 1, 2];
    const ROW_ORDINAL_MARKER: u8 = 0x28;
    const REFERENCE_PREFIX: [u8; 2] = [0, 0x3b];
    if record.name() != "Multi Instance Output" {
        return Ok(None);
    }
    let Some(end) = record.payload().len().checked_sub(ENVELOPE.len()) else {
        return Ok(None);
    };
    unique_payload_candidate(
        ctx,
        0..=end,
        |start| {
            if record.payload().get(start..start + ENVELOPE.len()) != Some(&ENVELOPE) {
                return Ok(None);
            }
            let Some(declared_count @ 2..) = record.payload().get(start + ENVELOPE.len()).copied()
            else {
                return Ok(None);
            };
            let mut instance_count = 0;
            let mut at = start + ENVELOPE.len() + 1;
            let mut row_storage = ctx.reserve_scoped(0, "NX multi-instance row workspace")?;
            let mut rows = Vec::new();
            let mut indices = 2..=declared_count;
            while !indices.is_empty() {
                let Some(expected) =
                    ctx.next_charged(&mut indices, "NX multi-instance selector row traversal")?
                else {
                    break;
                };
                if record.payload().get(at..at + ROW_PREFIX.len()) != Some(&ROW_PREFIX) {
                    return Ok(None);
                }
                at += ROW_PREFIX.len();
                let Some(atom) = record.payload().get(at..).and_then(CompactIndexAtom::read) else {
                    return Ok(None);
                };
                let selector = LocatedCompactIndex {
                    atom,
                    offset: record.payload_offset() + at,
                };
                at += atom.raw().len();
                if record.payload().get(at) != Some(&ROW_ORDINAL_MARKER) {
                    return Ok(None);
                }
                let Some(ordinal) = record.payload().get(at + 1).copied() else {
                    return Ok(None);
                };
                instance_count = instance_count.max(ordinal);
                if record.payload().get(at + 2) != Some(&expected) {
                    return Ok(None);
                }
                row_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut rows,
                        (selector, ordinal),
                        "NX multi-instance selector rows",
                    )
                })?;
                at += 3;
            }
            if record.payload().get(at..at + REFERENCE_PREFIX.len()) != Some(&REFERENCE_PREFIX) {
                return Ok(None);
            }
            at += REFERENCE_PREFIX.len();
            let mut references = Vec::new();
            let mut instances = 1..instance_count;
            while !instances.is_empty() {
                ctx.next_charged(
                    &mut instances,
                    "NX multi-instance trailing reference traversal",
                )?;
                let Some(token) = record
                    .payload()
                    .get(at..)
                    .and_then(reference_index::FeatureReferenceToken::read)
                else {
                    return Ok(None);
                };
                ctx.push_vec(
                    &mut references,
                    PayloadObjectReference {
                        offset: record.payload_offset() + at,
                        token,
                    },
                    "NX multi-instance trailing references",
                )?;
                at += token.raw().len();
            }
            if record.payload().get(at..at + 2) != Some(&[1, instance_count]) {
                return Ok(None);
            }
            let Some(outputs) =
                instances::MultiInstanceOutputs::new_charged(ctx, rows, references)?
            else {
                return Ok(None);
            };
            Ok(Some(MultiInstanceOutputPayloadLane {
                offset: record.payload_offset() + start + 8,
                outputs,
            }))
        },
        "scan NX multi-instance output lanes",
        "NX multi-instance candidate storage",
    )
}

/// Decode the unique exactly counted selector lane in an
/// `IDENTICAL INSTANCE OUTPUT` payload.
pub(crate) fn identical_instance_output_payload_lane(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<Option<IdenticalInstanceOutputPayloadLane>, CodecError> {
    const ROW_MIDDLE: [u8; 2] = [1, 2];
    const SENTINEL: [u8; 7] = [0xe0, 0x7f, 0xff, 0xff, 0xff, 0, 0];
    if record.name() != "IDENTICAL INSTANCE OUTPUT" {
        return Ok(None);
    }
    let Some(end) = record.payload().len().checked_sub(3) else {
        return Ok(None);
    };
    unique_payload_candidate(
        ctx,
        0..end,
        |start| {
            let Some(leading_schema_index) = record.payload().get(start).copied() else {
                return Ok(None);
            };
            let Some(count_schema_index) = record
                .payload()
                .get(start + 1)
                .copied()
                .and_then(IdenticalInstanceSchemaIndex::new)
            else {
                return Ok(None);
            };
            if record.payload().get(start + 2) != Some(&1) {
                return Ok(None);
            }
            let Some(declared_count @ 2..) = record.payload().get(start + 3).copied() else {
                return Ok(None);
            };
            let Some(terminal_count) = declared_count.checked_add(1) else {
                return Ok(None);
            };
            let [first, second, third] = count_schema_index.row_indices();
            let mut at = start + 4;
            let mut selectors = Vec::new();
            let mut ordinals = 2..=declared_count;
            while !ordinals.is_empty() {
                let Some(ordinal) = ctx.next_charged(
                    &mut ordinals,
                    "NX identical-instance selector row traversal",
                )?
                else {
                    break;
                };
                if record.payload().get(at) != Some(&first)
                    || record.payload().get(at + 1) != Some(&second)
                    || record.payload().get(at + 2..at + 4) != Some(&ROW_MIDDLE)
                    || record.payload().get(at + 4) != Some(&third)
                {
                    return Ok(None);
                }
                at += 5;
                let Some(atom) = record.payload().get(at..).and_then(CompactIndexAtom::read) else {
                    return Ok(None);
                };
                let selector = LocatedCompactIndex {
                    atom,
                    offset: record.payload_offset() + at,
                };
                at += atom.raw().len();
                if record.payload().get(at) != Some(&0)
                    || record.payload().get(at + 1) != Some(&ordinal)
                {
                    return Ok(None);
                }
                at += 2;
                ctx.push_vec(&mut selectors, selector, "NX identical-instance selectors")?;
            }
            if record.payload().get(at) != Some(&0)
                || record.payload().get(at + 1) != Some(&terminal_count)
                || record.payload().get(at + 2..at + 2 + SENTINEL.len()) != Some(&SENTINEL)
            {
                return Ok(None);
            }
            let Ok(selectors) = compact::CountedIndexMembers::new(selectors) else {
                return Ok(None);
            };
            Ok(Some(IdenticalInstanceOutputPayloadLane {
                offset: record.payload_offset() + start,
                leading_schema_index,
                count_schema_index,
                selectors,
            }))
        },
        "scan NX identical-instance selectors",
        "NX identical-instance candidate storage",
    )
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
    let mut storage = ctx.reserve_scoped(0, "NX SWP104 candidate storage")?;
    let len = usize::from(declared_count) - 1;
    let operation = "NX SWP104 members";
    let mut members = storage.with_storage(|| ctx.collection_vec(len, operation))?;
    let mut indices = 1..declared_count;
    while !indices.is_empty() {
        ctx.next_charged(&mut indices, "scan NX SWP104 leading branch")?;
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
            Some(bytes) => {
                storage.with_storage(|| ctx.copy_retained(bytes, "NX SWP104 state lane"))?
            }
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

    storage.commit()?;
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

fn operation_body_member_start(
    record: OperationBodyInput<'_>,
    reference: &OperationBodyReference,
) -> Option<(usize, usize)> {
    let token = reference.offset - record.offset();
    let end = token + reference.object_index.raw().len();
    let mut at = end + 2;
    for _ in 0..3 {
        at += PayloadScalarAtom::read(record.bytes().get(at..)?)?
            .raw()
            .len();
    }
    if record.bytes().get(at) != Some(&1) {
        return None;
    }
    let count = usize::from(*record.bytes().get(at + 1)?);
    if count < 2 {
        return None;
    }
    Some((at + 2, count - 1))
}

/// Decode wrapped member lanes following branch-`11` body scalar clauses.
pub(crate) fn operation_body_members(
    ctx: &DecodeContext<'_>,
    record: OperationBodyInput<'_>,
) -> Result<Vec<OperationBodyMemberGroup>, CodecError> {
    let mut groups = Vec::new();
    for (body_ordinal, reference) in operation_body_reference_candidates(ctx, record)?.enumerate() {
        let token = reference.offset - record.offset();
        let end = token + reference.object_index.raw().len();
        if record.bytes().get(end..end + 2) != Some(&[0xff, 0x11]) {
            continue;
        }
        let Some((mut at, count)) = operation_body_member_start(record, &reference) else {
            continue;
        };
        let mut storage = ctx.reserve_scoped(0, "NX body member candidate storage")?;
        let mut members = Vec::new();
        let mut rows = 0..count;
        let mut complete = true;
        while !rows.is_empty() {
            ctx.next_charged(&mut rows, "NX operation body members row traversal")?;
            if record.bytes().get(at) != Some(&0x2e) {
                complete = false;
                break;
            }
            at += 1;
            let member_at = at;
            let Some(atom) = record.bytes().get(at..).and_then(CompactIndexAtom::read) else {
                complete = false;
                break;
            };
            at += atom.raw().len();
            if record.bytes().get(at) != Some(&0) {
                complete = false;
                break;
            }
            at += 1;
            storage.with_storage(|| {
                ctx.push_vec(
                    &mut members,
                    LocatedCompactIndex {
                        atom,
                        offset: record.offset() + member_at,
                    },
                    "NX operation body members",
                )
            })?;
        }
        if !complete {
            continue;
        }
        let Some(body_reference_ordinal) = u32::try_from(body_ordinal).ok() else {
            continue;
        };
        storage.commit()?;
        ctx.push_vec(
            &mut groups,
            OperationBodyMemberGroup {
                body_reference_ordinal,
                body_object_index: reference.object_index.value(),
                members,
            },
            "NX operation body member groups",
        )?;
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
    let mut continuations = Vec::new();
    for (body_ordinal, reference) in operation_body_reference_candidates(ctx, record)?.enumerate() {
        let token = reference.offset - record.offset();
        let end = token + reference.object_index.raw().len();
        if record.bytes().get(end..end + 2) != Some(&[0xff, 0x11]) {
            continue;
        }
        let Some((mut at, count)) = operation_body_member_start(record, &reference) else {
            continue;
        };
        let mut rows = 0..count;
        let mut complete = true;
        while !rows.is_empty() {
            ctx.next_charged(
                &mut rows,
                "NX operation body 11 continuations row validation",
            )?;
            if record.bytes().get(at) != Some(&0x2e) {
                complete = false;
                break;
            }
            at += 1;
            let Some(token) =
                NullableCompactIndex::read(record.bytes(), at).filter(|token| token.atom.is_some())
            else {
                complete = false;
                break;
            };
            at += token.raw().len();
            if record.bytes().get(at) != Some(&0) {
                complete = false;
                break;
            }
            at += 1;
        }
        if !complete || record.bytes().get(at..at + 2) != Some(&[1, 2]) {
            continue;
        }
        at += 2;
        let Some(mut continuation) = LocatedCompactIndex::read(record.bytes(), at) else {
            continue;
        };
        at += continuation.atom.raw().len();
        continuation.offset += record.offset();
        if record.bytes().get(at..at + 3) != Some(&[0, 0, 1]) {
            continue;
        }
        at += 3;
        let Some(terminal_token) = record
            .bytes()
            .get(at..)
            .and_then(ReferenceIndexToken::read_feature)
        else {
            continue;
        };
        let next = at + terminal_token.raw().len();
        if record.bytes().get(next..next + 2) != Some(&[0, 0]) {
            continue;
        }
        let Some(body_reference_ordinal) = u32::try_from(body_ordinal).ok() else {
            continue;
        };
        ctx.push_vec(
            &mut continuations,
            OperationBody11Continuation {
                body_reference_ordinal,
                body_object_index: reference.object_index.value(),
                continuation,
                terminal: PayloadObjectReference {
                    token: terminal_token,
                    offset: record.offset() + at,
                },
            },
            "NX operation body continuations",
        )?;
    }
    Ok(continuations)
}

/// Decode complete unwrapped counted reference lanes following body scalar clauses.
pub(crate) fn operation_body_reference_lanes(
    ctx: &DecodeContext<'_>,
    record: OperationBodyInput<'_>,
) -> Result<Vec<OperationBodyReferenceLane>, CodecError> {
    let mut lanes = Vec::new();
    for (body_ordinal, reference) in operation_body_reference_candidates(ctx, record)?.enumerate() {
        let token = reference.offset - record.offset();
        let end = token + reference.object_index.raw().len();
        let Some(branch) =
            record.bytes().get(end + 1).copied().and_then(|value| {
                discriminators::OperationBodyReferenceBranch::try_from(value).ok()
            })
        else {
            continue;
        };
        let Some((at, count)) = operation_body_member_start(record, &reference) else {
            continue;
        };
        let mut compact_storage =
            ctx.reserve_scoped(0, "NX compact body lane candidate storage")?;
        let compact = compact_storage.with_storage(|| {
            operation_body_reference_lane_values(ctx, record, at, count, |bytes, offset| {
                let atom = CompactIndexAtom::read(bytes)?;
                Some((LocatedCompactIndex { atom, offset }, atom.raw().len()))
            })
        })?;
        let mut object_storage = ctx.reserve_scoped(0, "NX object body lane candidate storage")?;
        let objects = object_storage.with_storage(|| {
            operation_body_reference_lane_values(ctx, record, at, count, |bytes, offset| {
                let token = reference_index::PayloadIndexToken::read(bytes)?;
                Some((PayloadObjectReference { offset, token }, token.raw().len()))
            })
        })?;
        let Some(body_reference_ordinal) = u32::try_from(body_ordinal).ok() else {
            continue;
        };
        let values = match (compact, objects) {
            (Some(values), None) => {
                compact_storage.commit()?;
                OperationBodyReferenceLaneValues::CompactIndex(values)
            }
            (None, Some(values)) => {
                object_storage.commit()?;
                OperationBodyReferenceLaneValues::PayloadObjectIndex(values)
            }
            _ => continue,
        };
        ctx.push_vec(
            &mut lanes,
            OperationBodyReferenceLane {
                body_reference_ordinal,
                body_object_index: reference.object_index.value(),
                branch,
                values,
            },
            "NX operation body reference lanes",
        )?;
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
    let mut rows = 0..count;
    while !rows.is_empty() {
        ctx.next_charged(
            &mut rows,
            "NX operation body reference lane values range traversal",
        )?;
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
    let mut lanes = Vec::new();
    for form in [SketchScalarLaneForm::Form03, SketchScalarLaneForm::Form07] {
        let discriminator = form.discriminator();
        for offset in ctx.admit_iter(
            bytes
                .len()
                .checked_sub(discriminator.len() - 1)
                .map_or(0..0, |end| 0..end),
            "scan NX sketch scalar lanes",
        )? {
            let window = &bytes[offset..offset + discriminator.len()];
            if window != discriminator {
                continue;
            }
            let mut at = offset + discriminator.len();
            let mut candidate_storage =
                ctx.reserve_scoped(0, "NX scalar lane candidate storage")?;
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
                candidate_storage
                    .with_storage(|| ctx.reserve_vec(&mut values, 1, "NX sketch scalar atoms"))?;
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
            candidate_storage.commit()?;
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
    let mut pairs = Vec::new();
    for form in SketchPairForm::ALL {
        let discriminator = form.discriminator();
        let separator_width = form.separator_width();
        for offset in ctx.admit_iter(
            bytes
                .len()
                .checked_sub(discriminator.len() - 1)
                .map_or(0..0, |end| 0..end),
            "scan NX sketch fixed pairs",
        )? {
            let window = &bytes[offset..offset + discriminator.len()];
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
    let discriminator = SketchPairForm::Legacy.discriminator();
    let mut pairs = Vec::new();
    for offset in ctx.admit_iter(
        bytes
            .len()
            .checked_sub(discriminator.len() - 1)
            .map_or(0..0, |end| 0..end),
        "scan NX sketch mixed pairs",
    )? {
        let window = &bytes[offset..offset + discriminator.len()];
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
    let mut pairs = Vec::new();
    for form in DatumPairForm::ALL {
        let discriminator = form.discriminator();
        for offset in ctx.admit_iter(
            bytes
                .len()
                .checked_sub(discriminator.len() - 1)
                .map_or(0..0, |end| 0..end),
            "scan NX datum CSYS pairs",
        )? {
            let window = &bytes[offset..offset + discriminator.len()];
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
    let mut lanes = Vec::new();
    for offset in ctx.admit_iter(
        bytes
            .len()
            .checked_sub(Q155LaneFrame::DISCRIMINATOR.len() - 1)
            .map_or(0..0, |end| 0..end),
        "scan NX draft fixed lanes",
    )? {
        let window = &bytes[offset..offset + Q155LaneFrame::DISCRIMINATOR.len()];
        if window != Q155LaneFrame::DISCRIMINATOR {
            continue;
        }
        let mut at = offset + Q155LaneFrame::DISCRIMINATOR.len();
        let mut candidate_storage = ctx.reserve_scoped(0, "NX scalar lane candidate storage")?;
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
            candidate_storage
                .with_storage(|| ctx.reserve_vec(&mut values, 1, "NX draft fixed atoms"))?;
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
        candidate_storage.commit()?;
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
    let mut lanes = Vec::new();
    for branch in [DraftBinary32Branch::Form04, DraftBinary32Branch::Form03] {
        let discriminator = branch.discriminator();
        for offset in ctx.admit_iter(
            bytes
                .len()
                .checked_sub(discriminator.len() - 1)
                .map_or(0..0, |end| 0..end),
            "scan NX draft binary32 lanes",
        )? {
            let window = &bytes[offset..offset + discriminator.len()];
            if window != discriminator {
                continue;
            }
            let mut at = offset + discriminator.len();
            let mut candidate_storage =
                ctx.reserve_scoped(0, "NX scalar lane candidate storage")?;
            let mut values = Vec::new();
            let complete = loop {
                ctx.charge_work(1, "scan NX draft binary32 atoms")?;
                if !matches!(bytes.get(at), Some(0x40..=0x5f | 0xc0..=0xdf)) {
                    break bytes.get(at) == Some(&0x00);
                }
                let Some(scalar) = bytes.get(at..at + 4).and_then(ShiftedBinary32::read) else {
                    break false;
                };
                candidate_storage
                    .with_storage(|| ctx.reserve_vec(&mut values, 1, "NX draft binary32 atoms"))?;
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
            candidate_storage.commit()?;
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
    let mut frames = Vec::new();
    for offset in ctx.admit_iter(0..bytes.len(), "scan NX draft identity frames")? {
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
    let mut references = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        ctx.charge_work(1, "scan NX data-block object frames")?;
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
        let mut candidates = 0..range_end;
        while !candidates.is_empty() {
            let Some(at) =
                ctx.next_charged(&mut candidates, "NX expression declaration candidate scan")?
            else {
                break;
            };
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

fn operation_body_reference_at(
    record: OperationBodyInput<'_>,
    marker: usize,
    cursor: &mut usize,
) -> Option<OperationBodyReference> {
    if marker < *cursor {
        return None;
    }
    *cursor = marker + 1;
    let body_write = marker.checked_sub(record.payload_start()).and_then(|at| {
        operation_body_write_frame_at(record.payload(), record.payload_offset(), at)
    });
    if let Some(write) = body_write {
        *cursor = (*cursor).max(write.end_offset() - record.offset());
        return None;
    }
    if record.bytes().get(marker..marker + 3) != Some(&[1, 2, 0x10]) {
        return None;
    }
    let token = marker + 3;
    let object_index = reference_index::FeatureReferenceToken::read(&record.bytes()[token..])?;
    if record.bytes().get(token + object_index.raw().len()) != Some(&0xff) {
        return None;
    }
    Some(OperationBodyReference {
        offset: record.offset() + token,
        object_index,
    })
}

/// Decode the unique direct primary-body field in one operation.
pub(crate) fn operation_body_reference(
    ctx: &DecodeContext<'_>,
    record: OperationBodyInput<'_>,
) -> Result<Option<OperationBodyReference>, CodecError> {
    let mut starts = record
        .bytes()
        .len()
        .checked_sub(2)
        .map_or(0..0, |end| 0..end);
    let mut cursor = 0;
    let first = ctx.find_map(
        &mut starts,
        |marker| Ok(operation_body_reference_at(record, marker, &mut cursor)),
        "NX unique body reference search",
    )?;
    if first.is_none() {
        return Ok(None);
    }
    if ctx
        .find_map(
            &mut starts,
            |marker| Ok(operation_body_reference_at(record, marker, &mut cursor)),
            "NX body reference uniqueness",
        )?
        .is_some()
    {
        return Ok(None);
    }
    Ok(first)
}

fn operation_body_reference_candidates<'record>(
    ctx: &DecodeContext<'_>,
    record: OperationBodyInput<'record>,
) -> Result<impl Iterator<Item = OperationBodyReference> + 'record, CodecError> {
    let mut cursor = 0;
    Ok(ctx
        .admit_iter(
            record
                .bytes()
                .len()
                .checked_sub(2)
                .map_or(0..0, |end| 0..end),
            "NX operation body reference traversal",
        )?
        .filter_map(move |marker| operation_body_reference_at(record, marker, &mut cursor)))
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
    let mut relations = Vec::new();
    for marker in ctx.admit_iter(
        payload.len().checked_sub(1).map_or(0..0, |end| 0..end),
        "scan NX body-write frames",
    )? {
        if payload[marker..marker + 2] != [1, 2] {
            continue;
        }
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
    let mut scratch = ctx.reserve_scoped(0, "NX operation-state group workspace")?;
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
            scratch.with_storage(|| {
                ctx.push_vec(
                    &mut candidates,
                    (at, end),
                    "NX operation-state group candidates",
                )
            })?;
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

    let mut predecessors = scratch.with_storage(|| {
        ctx.alloc_filled(
            candidates.len(),
            None,
            "NX operation-state group predecessors",
        )
    })?;
    let mut best_by_end = BTreeMap::<usize, GroupPath>::new();
    for (candidate_index, (start, end)) in ctx
        .admit_iter(&candidates, "NX operation-state group path candidates")?
        .enumerate()
    {
        let previous = ctx
            .get_btree_map(
                &best_by_end,
                start,
                "NX preceding operation-state group path",
            )?
            .copied();
        let path = GroupPath {
            last_candidate: candidate_index,
            length: previous.map_or(1, |path| path.length + 1),
            first_start: previous.map_or(*start, |path| path.first_start),
        };
        predecessors[candidate_index] = previous.map(|path| path.last_candidate);
        let replace = ctx
            .get_btree_map(
                &best_by_end,
                end,
                "NX operation-state group path replacement",
            )?
            .is_none_or(|current| {
                path.length > current.length
                    || (path.length == current.length && path.first_start < current.first_start)
            });
        if replace {
            scratch.with_storage(|| {
                ctx.insert_btree_map(
                    &mut best_by_end,
                    *end,
                    path,
                    "NX operation-state group path insertion",
                )
            })?;
        }
    }

    let trailing_start =
        if map_start >= 2 && bytes.get(map_start - 2..map_start) == Some(&[0x01, 0x01]) {
            map_start - 2
        } else {
            map_start
        };
    let Some(terminal) = ctx
        .get_btree_map(
            &best_by_end,
            &trailing_start,
            "NX terminal operation-state group path",
        )?
        .copied()
    else {
        return Ok(None);
    };
    let mut path = scratch
        .with_storage(|| ctx.collection_vec(terminal.length, "NX operation-state group path"))?;
    let mut candidate = Some(terminal.last_candidate);
    while let Some(candidate_index) = candidate {
        ctx.charge_work(1, "NX operation-state predecessor traversal")?;
        path.push(candidate_index);
        candidate = predecessors[candidate_index];
    }
    ctx.reverse(&mut path, "NX operation-state group path reversal")?;
    let Some(&last) = path.last() else {
        return Ok(None);
    };
    let mut group_storage = ctx.reserve_scoped(0, "NX selected group candidate storage")?;
    let mut groups = group_storage
        .with_storage(|| ctx.collection_vec(path.len(), "NX operation-state groups"))?;
    for candidate in ctx
        .admit_iter(&path, "NX selected state group path traversal")?
        .copied()
    {
        let Some(group) = group_storage.with_storage(|| {
            operation_state_group_at(ctx, bytes, candidates[candidate].0, map_start, base_offset)
        })?
        else {
            return Ok(None);
        };
        groups.push(group);
    }
    let Some(trailing) = bytes.get(candidates[last].1..map_start) else {
        return Ok(None);
    };
    let table = OperationStateGroupTable::new(ctx, groups, trailing)?;
    if table.is_some() {
        group_storage.commit()?;
    }
    Ok(table)
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
    let mut storage = ctx.reserve_scoped(0, "NX audit row candidate storage")?;
    let mut rows = Vec::new();
    let mut at = start;
    let mut previous_ordinal = None;
    while at < end {
        ctx.charge_work(1, "scan NX audit-trail rows")?;
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
        storage.with_storage(|| ctx.reserve_vec(&mut rows, 1, "NX audit-trail rows"))?;
        rows.push(row);
    }
    storage.commit()?;
    Ok(Some(rows))
}

fn operation_state_journal_start(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    product_end: usize,
) -> Result<Option<usize>, CodecError> {
    let Some(tail) = bytes.get(product_end..) else {
        return Ok(None);
    };
    let Some(relative) = ctx.position_by(
        1..tail.len(),
        |end| Ok(tail[end - 1..=end] == [0x41, 0x00]),
        "NX state journal marker",
    )?
    else {
        return Ok(None);
    };
    let Some(marker) = relative.checked_add(product_end) else {
        return Ok(None);
    };
    let Some(mut at) = marker.checked_add(2) else {
        return Ok(None);
    };
    let mut count_tokens = 0usize;
    let mut saw_repeated_token = false;
    loop {
        ctx.charge_work(1, "NX journal version token traversal")?;
        if bytes
            .get(at..at + 3)
            .is_some_and(|token| token[0] == 3 && token[1] == 5)
        {
            count_tokens += 1;
            at += 3;
        } else if bytes.get(at..at + 4) == Some(&[3, 3, 2, 0]) {
            saw_repeated_token = true;
            at += 4;
        } else {
            break;
        }
    }
    if bytes.get(at) == Some(&0) {
        at += 1;
    }
    loop {
        ctx.charge_work(1, "NX journal prefix separator traversal")?;
        if bytes.get(at..at + 2) != Some(&[4, 0]) {
            break;
        }
        at += 2;
    }
    Ok((count_tokens > 0 && (saw_repeated_token || count_tokens >= 2)).then_some(at))
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
    let mut storage = ctx.reserve_scoped(0, "NX journal candidate storage")?;
    let mut groups = Vec::new();
    let mut at = start;
    let mut previous_ordinal = None;
    loop {
        ctx.charge_work(1, "scan NX state-journal groups")?;
        let group =
            storage.with_storage(|| JournalGroup::read(ctx, bytes, at, end, base_offset))?;
        let group = match group {
            Some(group) => group,
            None => {
                let mut next = at;
                loop {
                    ctx.charge_work(1, "NX journal group separator traversal")?;
                    if bytes.get(next..next + 2) != Some(&[4, 0]) {
                        break;
                    }
                    next += 2;
                }
                if next == at {
                    break;
                }
                let Some(group) = storage
                    .with_storage(|| JournalGroup::read(ctx, bytes, next, end, base_offset))?
                else {
                    break;
                };
                group
            }
        };
        let mut initial = group.rows().initial().iter();
        while initial.len() > 0 {
            let Some(row) = ctx.next_charged(&mut initial, "NX journal initial rows")? else {
                break;
            };
            let ordinal = row.ordinal().value();
            if previous_ordinal.is_some_and(|previous| ordinal <= previous) {
                return Ok(None);
            }
            previous_ordinal = Some(ordinal);
        }
        let ordinal = group.rows().last().ordinal().value();
        if previous_ordinal.is_some_and(|previous| ordinal <= previous) {
            return Ok(None);
        }
        previous_ordinal = Some(ordinal);
        let Some(next) = group.end_offset().checked_sub(base_offset) else {
            return Ok(None);
        };
        at = next;
        storage.with_storage(|| ctx.push_vec(&mut groups, group, "NX state-journal groups"))?;
    }
    if groups.is_empty() {
        return Ok(None);
    }
    storage.commit()?;
    Ok(Some(groups))
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
    let decode = |start: usize, marker| -> Result<Option<CommonFrame<usize>>, CodecError> {
        if marker == [1, 1, 1] && record.name() != "DELETE" {
            return Ok(None);
        }
        let Some(bytes) = record.payload().get(start..) else {
            return Ok(None);
        };
        let Some(prefix) = CommonFramePrefix::read(bytes, marker) else {
            return Ok(None);
        };
        let state_at = prefix.byte_len();
        let Some(state) = bytes
            .get(state_at..state_at + 8)
            .and_then(|state| state.try_into().ok())
        else {
            return Ok(None);
        };
        let Some(tail) = bytes.get(state_at + 8..) else {
            return Ok(None);
        };
        let Some(suffix) = CommonFrameSuffix::read(tail) else {
            return Ok(None);
        };
        let Some(offset) = record.payload_offset().checked_add(start) else {
            return Ok(None);
        };
        Ok(CommonFrame::<usize>::new(prefix, state, suffix, offset))
    };
    let mut frames = Vec::new();
    for start in ctx.admit_iter(&(0..record.payload().len()), "scan NX common frames")? {
        if let Some(frame) = decode(start, [1, 3, 2])? {
            ctx.reserve_vec(&mut frames, 1, "nx common frames")?;
            frames.push(frame);
        }
        if let Some(frame) = decode(start, [1, 1, 1])? {
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
    let mut storage = ctx.reserve_scoped(0, "NX terminal common frame storage")?;
    let common_frames = storage.with_storage(|| operation_common_frames(ctx, record))?;
    let mut candidate = None;
    for start in (0..terminator).rev().take(9) {
        let Some(suffix) = CommonFrameSuffix::read(&record.payload()[start..]) else {
            continue;
        };
        if start + suffix.byte_len() != record.payload().len() {
            continue;
        }
        let Some(offset) = record.payload_offset().checked_add(start) else {
            continue;
        };
        let Some(frame) = TerminalFrame::<usize>::new(suffix, offset) else {
            continue;
        };
        let immediate_common_frame_offset = ctx
            .find_by(
                &common_frames,
                |common| {
                    Ok(common.local_ordinal_offset() == frame.offset()
                        && common.end_offset() == frame.end_offset())
                },
                "NX terminal common frame lookup",
            )?
            .map(CommonFrame::<usize>::offset);
        if candidate.is_some() {
            return Ok(None);
        }
        candidate = Some(OperationTerminalFrame {
            immediate_common_frame_offset,
            frame,
        });
    }
    Ok(candidate)
}

/// Decode ordered `04 00, object_index, 02 0b` references from one bounded block.
pub(crate) fn data_block_object_references(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<DataBlockObjectReference>, CodecError> {
    let mut references = Vec::new();
    let mut at = 0usize;
    while at + 5 <= bytes.len() {
        ctx.charge_work(1, "scan NX data-block object references")?;
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
    let mut operations = Vec::new();
    for label in ctx
        .admit_iter(labels, "scan NX Boolean operation labels")?
        .copied()
    {
        let kind = match label.value {
            "UNITE" => BooleanOperationKind::Unite,
            "SUBTRACT" => BooleanOperationKind::Subtract,
            "INTERSECT" => BooleanOperationKind::Intersect,
            _ => continue,
        };
        let Some(at) = label.header.end_offset().checked_sub(base_offset) else {
            continue;
        };
        let Some(label_width) = bytes.get(at + 1).copied().map(usize::from) else {
            continue;
        };
        let Some(label_end) = at
            .checked_add(label_width)
            .and_then(|end| end.checked_add(1))
        else {
            continue;
        };
        if bytes.get(label_end..label_end + BODY_HEADER.len()) != Some(BODY_HEADER) {
            continue;
        }
        let mut target_storage = ctx.reserve_scoped(0, "NX Boolean target workspace")?;
        let Some((targets, next)) = target_storage.with_storage(|| {
            counted_feature_object_indices(ctx, bytes, base_offset, label_end + BODY_HEADER.len())
        })?
        else {
            continue;
        };
        if targets.len() != 1 || bytes.get(next) != Some(&0) {
            continue;
        }
        let mut tool_storage = ctx.reserve_scoped(0, "NX Boolean tool candidate storage")?;
        let Some((tools, end)) = tool_storage
            .with_storage(|| counted_feature_object_indices(ctx, bytes, base_offset, next + 1))?
        else {
            continue;
        };
        if tools.is_empty() || bytes.get(end) != Some(&0) {
            continue;
        }
        let Some(target) = targets.into_iter().next() else {
            continue;
        };
        tool_storage.commit()?;
        ctx.push_vec(
            &mut operations,
            BooleanOperation {
                offset: label.header.end_offset(),
                kind,
                target,
                tools,
            },
            "NX Boolean operations",
        )?;
    }
    Ok(operations)
}

fn counted_feature_object_indices(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    base_offset: usize,
    at: usize,
) -> Result<Option<(Vec<PayloadObjectReference>, usize)>, CodecError> {
    if bytes.get(at) != Some(&1) {
        return Ok(None);
    }
    let Some(count) = bytes
        .get(at + 1)
        .and_then(|value| usize::from(*value).checked_sub(1))
    else {
        return Ok(None);
    };
    let mut cursor = at + 2;
    let mut storage = ctx.reserve_scoped(0, "NX counted feature reference candidate storage")?;
    let mut values = Vec::new();
    let mut indices = 0..count;
    while !indices.is_empty() {
        ctx.next_charged(
            &mut indices,
            "NX counted feature object indices range traversal",
        )?;
        let Some(value) = bytes
            .get(cursor..)
            .and_then(ReferenceIndexToken::read_feature)
        else {
            return Ok(None);
        };
        storage.with_storage(|| {
            ctx.push_vec(
                &mut values,
                PayloadObjectReference {
                    offset: base_offset + cursor,
                    token: value,
                },
                "NX Boolean references",
            )
        })?;
        cursor += value.raw().len();
    }
    storage.commit()?;
    Ok(Some((values, cursor)))
}

/// Decode count-framed runs of same-section record references.
pub(crate) fn counted_record_references(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    base_offset: usize,
    record_count: usize,
) -> Result<Vec<LocatedReference<u16>>, CodecError> {
    let mut references = Vec::new();
    let mut at = 0usize;
    while at + 5 <= bytes.len() {
        ctx.charge_work(1, "scan NX counted record references")?;
        if bytes[at] != 1 || bytes[at + 1] < 2 {
            at += 1;
            continue;
        }
        let count = usize::from(bytes[at + 1] - 1);
        let Some(end) = at
            .checked_add(2 + count * 3)
            .filter(|end| *end <= bytes.len())
        else {
            at += 1;
            continue;
        };
        let mut storage = ctx.reserve_scoped(0, "NX counted record candidate storage")?;
        let mut candidate = Vec::new();
        let mut indices = 0..count;
        let mut complete = true;
        while !indices.is_empty() {
            let Some(index) = ctx.next_charged(&mut indices, "NX counted record tag validation")?
            else {
                break;
            };
            let token = at + 2 + index * 3;
            if bytes[token] != 0x90 {
                complete = false;
                break;
            }
            let Some(value) = View::u16_be_at(bytes, token + 1)
                .filter(|value| usize::from(*value) < record_count)
            else {
                complete = false;
                break;
            };
            storage.with_storage(|| {
                ctx.push_vec(
                    &mut candidate,
                    LocatedReference {
                        offset: base_offset + token,
                        value,
                    },
                    "NX counted record candidate references",
                )
            })?;
        }
        if !complete {
            at += 1;
            continue;
        }
        for reference in
            ctx.admit_iter(candidate, "NX counted record references output traversal")?
        {
            ctx.push_vec(&mut references, reference, "NX counted record references")?;
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
    let mut storage = ctx.reserve_scoped(0, "NX direct record reference storage")?;
    let parsed = storage.with_storage(|| references(ctx, bytes, base_offset))?;
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
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        ctx.charge_work(1, "scan NX tagged references")?;
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
    let mut values = Vec::new();
    for offset in ctx.admit_iter(
        bytes
            .len()
            .checked_sub(MARKER.len() - 1)
            .map_or(0..0, |end| 0..end),
        "scan NX printable strings",
    )? {
        if &bytes[offset..offset + MARKER.len()] != MARKER {
            continue;
        }
        let Some(text_len) = bytes
            .get(offset + 3)
            .and_then(|value| usize::from(*value).checked_sub(2))
        else {
            continue;
        };
        let Some(start) = offset.checked_add(4) else {
            continue;
        };
        let Some(end) = start.checked_add(text_len) else {
            continue;
        };
        let Some(raw) = bytes.get(start..end) else {
            continue;
        };
        if bytes.get(end) != Some(&0) {
            continue;
        }
        let Ok(text) = ctx.validate_utf8(raw, "NX printable string UTF-8 validation")? else {
            continue;
        };
        let Ok(value) = PrintableString::from_wire(ctx, text)? else {
            continue;
        };
        ctx.push_vec(
            &mut values,
            StringValue {
                offset: base_offset + offset,
                value,
            },
            "NX printable strings",
        )?;
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
    let mut values = Vec::new();
    for offset in ctx.admit_iter(
        bytes
            .len()
            .checked_sub(MARKER.len() - 1)
            .map_or(0..0, |end| 0..end),
        "scan NX UUID strings",
    )? {
        if &bytes[offset..offset + MARKER.len()] != MARKER {
            continue;
        }
        let Some(start) = offset.checked_add(MARKER.len()) else {
            continue;
        };
        let Some(end) = start.checked_add(TEXT_LEN) else {
            continue;
        };
        let Some(raw) = bytes.get(start..end) else {
            continue;
        };
        let Ok(text) = std::str::from_utf8(raw) else {
            continue;
        };
        let Ok(value) = crate::canonical_uuid::CanonicalUuid::new(text) else {
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
    let mut strings = Vec::new();
    for offset in ctx.admit_iter(
        bytes
            .len()
            .checked_sub(MARKER.len() - 1)
            .map_or(0..0, |end| 0..end),
        "scan NX surface payload strings",
    )? {
        if &bytes[offset..offset + MARKER.len()] != MARKER {
            continue;
        }
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
        let Ok(text) = ctx.validate_utf8(raw, "parse NX surface payload string")? else {
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
    let marker = b"hostglobalvariables";
    if !ctx.any_by(
        bytes
            .len()
            .checked_sub(marker.len() - 1)
            .map_or(0..0, |end| 0..end),
        |start| Ok(&bytes[start..start + marker.len()] == marker),
        "NX numeric expression table marker",
    )? {
        return Ok(Vec::new());
    }
    let mut expressions = Vec::new();
    let prefix = b"(Number [";
    for offset in ctx.admit_iter(
        bytes
            .len()
            .checked_sub(prefix.len() - 1)
            .map_or(0..0, |end| 0..end),
        "NX numeric expression marker traversal",
    )? {
        if &bytes[offset..offset + prefix.len()] != prefix {
            continue;
        }
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
        let Some(relative) = ctx.position_by(
            tail.len().checked_sub(3).map_or(0..0, |end| 0..end),
            |start| Ok(tail[start..start + 4] == [0xff; 4]),
            "nx framed OM section scan",
        )?
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
    let Some(limit) = section_end.checked_sub(3) else {
        return Ok(None);
    };
    unique_payload_candidate(
        ctx,
        schema_start..limit,
        |at| {
            let Some(relative) =
                View::u32_le_at(bytes, at).and_then(|value| usize::try_from(value).ok())
            else {
                return Ok(None);
            };
            let Some(target) = section_offset.checked_add(relative) else {
                return Ok(None);
            };
            let Some(after) = at.checked_add(4) else {
                return Ok(None);
            };
            let Some(last) = target.checked_add(15) else {
                return Ok(None);
            };
            if target < after || last > section_end {
                return Ok(None);
            }
            let Some(suffix) = target
                .checked_add(12)
                .and_then(|start| bytes.get(start..section_end))
            else {
                return Ok(None);
            };
            Ok(ProductRecord::read(ctx, suffix, ProductRecordForm::Modern)?.map(|_| (target, at)))
        },
        "NX record area pointer search",
        "NX record area pointer candidate storage",
    )
}

fn legacy_feature_record_area_pointer(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    section_offset: usize,
    schema_start: usize,
    section_end: usize,
    types: &[TypeDefinition<'_>],
) -> Result<Option<(usize, usize)>, CodecError> {
    if !ctx.any_by(
        types,
        |definition| Ok(definition.name == "UGS::FEATURE_RECORD"),
        "NX legacy feature role scan",
    )? {
        return Ok(None);
    }
    let Some(limit) = section_end.checked_sub(4) else {
        return Ok(None);
    };
    unique_payload_candidate(
        ctx,
        schema_start..limit,
        |at| {
            if bytes.get(at) != Some(&1) {
                return Ok(None);
            }
            let Some(relative) =
                View::u32_le_at(bytes, at + 1).and_then(|value| usize::try_from(value).ok())
            else {
                return Ok(None);
            };
            let Some(target) = section_offset
                .checked_add(relative)
                .and_then(|target| target.checked_add(1))
            else {
                return Ok(None);
            };
            let Some(after) = at.checked_add(5) else {
                return Ok(None);
            };
            let Some(last) = target.checked_add(12) else {
                return Ok(None);
            };
            if target < after
                || last > section_end
                || View::u32_le_at(bytes, target).is_none()
                || View::u32_le_at(bytes, target + 4).is_none()
                || View::u32_le_at(bytes, target + 8).is_none()
            {
                return Ok(None);
            }
            let Some(suffix) = bytes.get(target + 12..section_end) else {
                return Ok(None);
            };
            Ok(
                ProductRecord::read(ctx, suffix, ProductRecordForm::LegacyFeature)?
                    .map(|_| (target, at)),
            )
        },
        "NX legacy record area pointer search",
        "NX legacy record area pointer candidate storage",
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

fn product_record_range_at(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    offset: usize,
) -> Result<Option<ProductRecordRange>, CodecError> {
    let Some(suffix) = bytes.get(offset..) else {
        return Ok(None);
    };
    let Some(layout) = ProductRecord::read(ctx, suffix, ProductRecordForm::Modern)? else {
        return Ok(None);
    };
    Ok(offset
        .checked_add(layout.byte_len())
        .map(|end| ProductRecordRange { start: offset, end }))
}

fn record_area_product_end(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    offset: usize,
) -> Result<Option<usize>, CodecError> {
    let Some(suffix) = bytes.get(offset..) else {
        return Ok(None);
    };
    let layout = match ProductRecord::read(ctx, suffix, ProductRecordForm::Modern)? {
        Some(layout) => layout,
        None => {
            let Some(layout) = ProductRecord::read(ctx, suffix, ProductRecordForm::LegacyFeature)?
            else {
                return Ok(None);
            };
            layout
        }
    };
    Ok(offset.checked_add(layout.byte_len()))
}

/// Count validated product records fully contained in `[lower, upper]`.
///
/// A validated product record cannot contain another validated product-record
/// start: its text is restricted to printable ASCII and spaces, while a record
/// start begins with a non-printable byte. The discovery pass therefore orders
/// both starts and ends. This makes the containment query a pair of binary
/// searches instead of a scan over every product record for every candidate.
fn product_record_count_within(
    ctx: &DecodeContext<'_>,
    ranges: &[ProductRecordRange],
    lower: usize,
    upper: usize,
) -> Result<usize, CodecError> {
    let first = ctx.partition_point(
        ranges,
        |range| Ok(range.start < lower),
        "NX product record lower bound",
    )?;
    let end = ctx.partition_point(
        ranges,
        |range| Ok(range.end <= upper),
        "NX product record upper bound",
    )?;
    Ok(ranges
        .get(first..end)
        .map_or(0, <[ProductRecordRange]>::len))
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
    ctx.retain_vec(
        &mut candidates,
        |candidate| {
            if candidate.source().len() <= furthest_end {
                return Ok(false);
            }
            furthest_end = candidate.source().len();
            Ok(true)
        },
        "NX indexed section containment",
    )?;
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
    let mut temporary = ctx.reserve_scoped(0, "nx indexed OM candidate scan")?;
    let mut candidates = Vec::new();
    let mut seen_record_starts = BTreeSet::new();
    let mut product_record_ranges = Vec::new();
    for offset in ctx.admit_iter(0..bytes.len(), "NX indexed product record discovery")? {
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
        for table in ctx.admit_iter(&(0..range_end), "NX fixed index table traversal")? {
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
                FixedIndex::new(ctx, &descending_u32_edges, index_start, count, base, table)?
            else {
                continue;
            };
            if !temporary.with_storage(|| {
                ctx.insert_btree_set(
                    &mut seen_record_starts,
                    table_end,
                    "NX fixed record start insertion",
                )
            })? {
                continue;
            }
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
        for count_offset in ctx.admit_iter(&(8..range_end), "NX offset index table traversal")? {
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
                product_record_count_within(ctx, &product_record_ranges, first, second)?
                    .checked_add(product_record_count_within(
                        ctx,
                        &product_record_ranges,
                        second,
                        third,
                    )?)
                    .ok_or_else(|| {
                        CodecError::Malformed("NX product record count overflow".into())
                    })?;
            if product_record_count != 1 {
                continue;
            }
            let Some(index) = OffsetIndex::new(
                ctx,
                &descending_u32_edges,
                index_start,
                offset_count,
                count_offset,
            )?
            else {
                continue;
            };
            if !temporary.with_storage(|| {
                ctx.insert_btree_set(
                    &mut seen_record_starts,
                    second,
                    "NX offset record start insertion",
                )
            })? {
                continue;
            }
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
    for candidate in ctx.admit_iter(
        select_outer_indexed_candidates(ctx, candidates)?,
        "NX selected indexed section traversal",
    )? {
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
    ctx.find_map(
        0..last,
        |at| {
            let Some(product) = ProductRecord::read(ctx, &bytes[at..], ProductRecordForm::Modern)?
            else {
                return Ok(None);
            };
            Ok(base_offset.checked_add(at).map(|offset| StoreVersion {
                offset,
                value: product.text(),
            }))
        },
        "NX store version marker search",
    )
}

/// Decode the zero-prefixed offset-store control form as ordered 24-bit values.
///
/// Each word is serialized `00, value:u24 LE`. The complete form is atomic.
fn offset_store_control_values<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
) -> Result<
    Option<(
        NonEmpty<ControlWord24>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    )>,
    CodecError,
> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return Ok(None);
    }
    let mut storage = ctx.reserve_scoped(0, "NX zero-prefixed control candidate storage")?;
    let mut values = Vec::new();
    let mut words = 0..bytes.len() / 4;
    while !words.is_empty() {
        let Some(index) =
            ctx.next_charged(&mut words, "NX zero-prefixed control word traversal")?
        else {
            break;
        };
        let word = &bytes[index * 4..index * 4 + 4];
        if word[0] != 0 {
            return Ok(None);
        }
        storage.with_storage(|| {
            ctx.push_vec(
                &mut values,
                ControlWord24::new([word[1], word[2], word[3]]),
                "NX zero-prefixed control values",
            )
        })?;
    }
    Ok(NonEmpty::from_admitted_vec(values).map(|values| (values, storage)))
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
    if ctx.any_by(
        0..count,
        |index| Ok(value_at(index).is_none()),
        "nx offset-store control validation",
    )? {
        return Ok(None);
    }
    let mut scratch = ctx.reserve_scoped(0, "NX offset-store class workspace")?;
    let mut suffix_minima = scratch
        .with_storage(|| ctx.alloc_filled(count, u32::MAX, "NX offset-store suffix minima"))?;
    for index in ctx
        .admit_iter(&(0..count - 1), "NX offset-store suffix minimum traversal")?
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
    let mut indices = 0..count - 1;
    while !indices.is_empty() {
        let Some(index) = ctx.next_charged(&mut indices, "nx offset-store identity scan")? else {
            break;
        };
        let minimum = suffix_minima[index];
        let Some(identity) = value_at(index) else {
            return Ok(None);
        };
        if !scratch.with_storage(|| {
            ctx.insert_btree_set(
                &mut identities,
                identity,
                "NX offset-store class identity insertion",
            )
        })? {
            break;
        }
        maximum_identity = maximum_identity.max(identity);
        if maximum_identity < minimum && boundary.replace(index + 1).is_some() {
            return Ok(None);
        }
    }
    let Some(boundary) = boundary else {
        return Ok(None);
    };
    let mut ordinals = ctx.collection_vec(boundary, "nx offset-store class ordinals")?;
    for index in ctx.admit_iter(&(0..boundary), "NX offset-store class ordinal projection")? {
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

fn offset_store_product_anchored_form<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    control: &[u8],
    first_record: &[u8],
) -> Result<
    Option<(
        OffsetStoreControlForm,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    )>,
    CodecError,
> {
    let source = (0..control.len())
        .map(|offset| (&control[offset..], offset))
        .chain(
            (0..first_record.len()).map(|offset| (&first_record[offset..], control.len() + offset)),
        );
    let Some(product_offset) = unique_payload_candidate(
        ctx,
        source,
        |(suffix, offset)| {
            Ok(ProductRecord::read(ctx, suffix, ProductRecordForm::Modern)?.map(|_| offset))
        },
        "NX control product anchor search",
        "NX control product anchor candidate storage",
    )?
    else {
        return Ok(None);
    };
    let leading_width = product_offset % 4;
    if product_offset >= control.len() {
        let Some(control_array_bytes) = control.len().checked_sub(leading_width) else {
            return Ok(None);
        };
        if control_array_bytes.is_multiple_of(4) {
            return Ok(None);
        }
    }
    let leading_value = if leading_width == 0 {
        None
    } else {
        let Some(value) =
            ControlLeadingValue::read(leading_width, control.iter().chain(first_record).copied())
        else {
            return Ok(None);
        };
        Some(value)
    };
    let mut storage = ctx.reserve_scoped(0, "NX product-anchored control candidate storage")?;
    let mut values = Vec::new();
    let mut words = 0..(product_offset - leading_width) / 4;
    while !words.is_empty() {
        let Some(index) =
            ctx.next_charged(&mut words, "NX product-anchored control word traversal")?
        else {
            break;
        };
        let Some(value) = joined_control_u32_le(control, first_record, leading_width + index * 4)
        else {
            return Ok(None);
        };
        storage.with_storage(|| {
            ctx.push_vec(&mut values, value, "NX product-anchored control values")
        })?;
    }
    let Some(values) = NonEmpty::from_admitted_vec(values) else {
        return Ok(None);
    };
    Ok(Some((
        OffsetStoreControlForm::ProductAnchored {
            leading_value,
            values,
        },
        storage,
    )))
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
        (Some((values, zero_storage)), None) => {
            zero_storage.commit()?;
            Ok(Some(OffsetStoreControlForm::ZeroPrefixed { values }))
        }
        (_, Some((form, product_storage))) => {
            product_storage.commit()?;
            Ok(Some(form))
        }
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
    let Some(relative) = ctx.position_by(
        bytes
            .len()
            .checked_sub(PREFIX.len() - 1)
            .map_or(0..0, |end| 0..end),
        |start| Ok(&bytes[start..start + PREFIX.len()] == PREFIX),
        "NX numeric expression prefix",
    )?
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
    let Some((unit, rest)) = ctx.split_once(text, "]) ", "NX numeric expression unit separator")?
    else {
        return Ok(None);
    };
    let Some((name, value_tail)) =
        ctx.split_once(rest, ": ", "NX numeric expression name separator")?
    else {
        return Ok(None);
    };
    if name.is_empty()
        || !ctx.all_by(
            name.bytes(),
            |byte| Ok(byte.is_ascii_alphanumeric() || byte == b'_'),
            "NX numeric expression name",
        )?
    {
        return Ok(None);
    }
    let Some((value_text, comment)) =
        ctx.split_once(value_tail, "; ", "NX numeric expression comment separator")?
    else {
        return Ok(None);
    };
    if !comment.is_empty() && !numeric_expression_comment_is_valid(ctx, comment)? {
        return Ok(None);
    }
    let name = ParameterName::from_wire(ctx, name)?;
    let Some(unit) = crate::om_tokens::unit_for(ctx, unit)? else {
        return Ok(None);
    };
    Ok(Some(NumericExpression {
        object_id,
        offset: base_offset + relative,
        name,
        unit,
        expression: value_text,
    }))
}

fn numeric_expression_comment_is_valid(
    ctx: &DecodeContext<'_>,
    comment: &str,
) -> Result<bool, CodecError> {
    Ok(comment.starts_with("//")
        && ctx.all_by(
            comment.chars(),
            |ch| Ok(ch.is_ascii_graphic() || matches!(ch, ' ' | '\t' | '\r' | '\n')),
            "NX numeric expression comment",
        )?)
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
        at: usize,
        values: Vec<FiniteReal>,
        values_reservation: cadmpeg_core::decode::ScopedReservation<'b>,
        operators: Vec<Operator>,
        operators_reservation: cadmpeg_core::decode::ScopedReservation<'b>,
        expect_operand: bool,
    }

    impl Parser<'_, '_, '_> {
        fn push_value(&mut self, value: FiniteReal) -> Result<(), CodecError> {
            self.values_reservation.with_storage(|| {
                self.ctx
                    .push_vec(&mut self.values, value, "NX expression value stack")
            })
        }
        fn push_operator(&mut self, operator: Operator) -> Result<(), CodecError> {
            self.operators_reservation.with_storage(|| {
                self.ctx.push_vec(
                    &mut self.operators,
                    operator,
                    "NX expression operator stack",
                )
            })
        }
        fn spaces(&mut self) -> Result<(), CodecError> {
            let skipped = self
                .ctx
                .position_by(
                    self.at..self.bytes.len(),
                    |at| Ok(!self.bytes[at].is_ascii_whitespace()),
                    "NX constant expression whitespace",
                )?
                .unwrap_or(self.bytes.len() - self.at);
            self.at += skipped;
            Ok(())
        }
        fn number(&mut self) -> Result<Option<FiniteReal>, CodecError> {
            let start = self.at;
            let digits = self
                .ctx
                .position_by(
                    self.at..self.bytes.len(),
                    |at| Ok(!self.bytes[at].is_ascii_digit() && self.bytes[at] != b'.'),
                    "NX constant expression mantissa",
                )?
                .unwrap_or(self.bytes.len() - self.at);
            self.at += digits;
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
                let exponent = self
                    .ctx
                    .position_by(
                        self.at..self.bytes.len(),
                        |at| Ok(!self.bytes[at].is_ascii_digit()),
                        "NX constant expression exponent",
                    )?
                    .unwrap_or(self.bytes.len() - self.at);
                self.at += exponent;
                if exponent == 0 {
                    return Ok(None);
                }
            }
            if self.at == start {
                return Ok(None);
            }
            let Ok(text) = self.ctx.validate_utf8(
                &self.bytes[start..self.at],
                "NX constant expression UTF-8 validation",
            )?
            else {
                return Ok(None);
            };
            let Ok(value) = self
                .ctx
                .parse_text::<f64>(text, "NX constant expression decimal parse")?
            else {
                return Ok(None);
            };
            Ok(FiniteReal::new(value))
        }

        fn apply_top(&mut self) -> Result<Option<()>, CodecError> {
            let Some(operator) = self.operators.pop() else {
                return Ok(None);
            };
            let value = match operator {
                Operator::OpenParen => return Ok(None),
                Operator::Unary(operator) => {
                    let Some(value) = self.values.pop() else {
                        return Ok(None);
                    };
                    match operator {
                        b'+' => value,
                        b'-' => value.negated(),
                        _ => return Ok(None),
                    }
                }
                Operator::Binary(operator) => {
                    let Some(right) = self.values.pop() else {
                        return Ok(None);
                    };
                    let Some(left) = self.values.pop() else {
                        return Ok(None);
                    };
                    let (left, right) = (left.get(), right.get());
                    let raw = match operator {
                        b'+' => left + right,
                        b'-' => left - right,
                        b'*' => left * right,
                        b'/' => left / right,
                        b'^' => left.powf(right),
                        _ => return Ok(None),
                    };
                    {
                        let Some(value) = FiniteReal::new(raw) else {
                            return Ok(None);
                        };
                        value
                    }
                }
            };
            self.push_value(value)?;
            Ok(Some(()))
        }

        fn push_binary(&mut self, operator: u8) -> Result<Option<()>, CodecError> {
            let incoming = Operator::Binary(operator);
            loop {
                self.ctx
                    .charge_work(1, "NX constant expression operator application")?;
                let Some(top) = self.operators.last() else {
                    break;
                };
                if matches!(top, Operator::OpenParen)
                    || top.precedence() < incoming.precedence()
                    || (top.precedence() == incoming.precedence()
                        && incoming.is_right_associative())
                {
                    break;
                }
                if self.apply_top()?.is_none() {
                    return Ok(None);
                }
            }
            self.push_operator(incoming)?;
            self.expect_operand = true;
            Ok(Some(()))
        }

        fn close_group(&mut self) -> Result<Option<()>, CodecError> {
            loop {
                self.ctx
                    .charge_work(1, "NX constant expression operator application")?;
                let Some(operator) = self.operators.last() else {
                    break;
                };
                if matches!(operator, Operator::OpenParen) {
                    break;
                }
                if self.apply_top()?.is_none() {
                    return Ok(None);
                }
            }
            Ok(matches!(self.operators.pop(), Some(Operator::OpenParen)).then_some(()))
        }

        fn parse(mut self) -> Result<Option<FiniteReal>, CodecError> {
            while self.at < self.bytes.len() {
                self.spaces()?;
                if self.at == self.bytes.len() {
                    break;
                }
                self.ctx.charge_work(1, "NX numeric expression scan")?;
                let byte = self.bytes[self.at];
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
                            let Some(value) = self.number()? else {
                                return Ok(None);
                            };
                            self.push_value(value)?;
                            self.expect_operand = false;
                        }
                    }
                } else {
                    match byte {
                        b'+' | b'-' | b'*' | b'/' | b'^' => {
                            if self.push_binary(byte)?.is_none() {
                                return Ok(None);
                            }
                            self.at += 1;
                        }
                        b')' => {
                            if self.close_group()?.is_none() {
                                return Ok(None);
                            }
                            self.at += 1;
                        }
                        _ => return Ok(None),
                    }
                }
            }
            if self.expect_operand {
                return Ok(None);
            }
            while !self.operators.is_empty() {
                self.ctx
                    .charge_work(1, "NX constant expression operator application")?;
                let Some(operator) = self.operators.last() else {
                    break;
                };
                if matches!(operator, Operator::OpenParen) {
                    return Ok(None);
                }
                if self.apply_top()?.is_none() {
                    return Ok(None);
                }
            }
            Ok((self.values.len() == 1).then(|| self.values[0]))
        }
    }

    let values_reservation = ctx.reserve_scoped(0, "NX expression value stack")?;
    let operators_reservation = ctx.reserve_scoped(0, "NX expression operator stack")?;
    Parser {
        bytes: text.as_bytes(),
        ctx,
        at: 0,
        values: Vec::new(),
        values_reservation,
        operators: Vec::new(),
        operators_reservation,
        expect_operand: true,
    }
    .parse()
}

#[cfg(test)]
mod tests;
