// SPDX-License-Identifier: Apache-2.0
//! Source-bounded Parameter record parsing with shared failure ownership.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};

use super::{
    OwnedParameterBytes, OwnedParseFailure, Ownership, ParameterAssembly, ParameterDefect,
    ParsedOwned, Token, TokenValue,
};
use crate::card::{CardScan, PhysicalLine, Section};
use crate::directory::DirectoryEntry;
use crate::global::ResolvedGlobal;

pub(crate) fn assemble(
    scan: &CardScan<'_>,
    directory: &[DirectoryEntry],
    excluded_directory: impl IntoIterator<Item = u32>,
    global: &ResolvedGlobal,
    ctx: &DecodeContext<'_>,
    output: &mut PartOutput,
) -> Result<ParameterAssembly, CodecError> {
    let source = scan
        .source_view()
        .ok_or_else(|| CodecError::malformed("IGES Parameter source view is absent"))?;
    ctx.capture_part_extent(source)?;
    let mut cards = Vec::new();
    for (_, card) in scan.section(Section::Parameter) {
        append_parameter_item(ctx, &mut cards, card)?;
    }
    if cards.is_empty() || !contiguous(source, &cards)? {
        return super::assemble(scan, directory, excluded_directory, global);
    }
    let extent = extent(source, &cards)?;
    let source_points = global
        .length_context()
        .filter(|_| !ctx.container_only())
        .map(|global| {
            crate::entities::geometry::point::SourcePointProjector::new(ctx, directory, global)
        })
        .transpose()?;
    let framing = envelopes(ctx, source, &cards, 72)?;
    let mut section_given_up = BTreeSet::new();
    let result = ctx.decode_part(
        output,
        extent,
        spec("iges.parameter_section", framing)?,
        |children| {
            let mut source_parts = ConflictParts::default();
            super::assemble_with(
                scan,
                directory,
                excluded_directory,
                None,
                global,
                |owned, lines, ownership| {
                    if !source_parts.initialized {
                        source_parts = ConflictParts::new(ctx, source, ownership, lines)?;
                        let unclaimed = ownership.iter().all(|owner| owner.cards.is_empty());
                        if source_parts.ambiguous() || unclaimed {
                            for owner in ownership
                                .iter()
                                .filter(|owner| unclaimed || !owner.cards.is_empty())
                            {
                                section_given_up.insert(owner.entry.sequence);
                            }
                            return Err(CodecError::malformed(if unclaimed {
                                "IGES Directory declarations and owner census locate no Parameter cards; only their section is bounded"
                            } else {
                                "IGES unequal Parameter claims overlap; only their section is bounded"
                            }));
                        }
                    }
                    let parsed = parse_owned(
                        ctx,
                        source,
                        children,
                        owned,
                        lines,
                        global,
                        &mut source_parts,
                        source_points.as_ref(),
                    )?;
                    Ok(parsed)
                },
            )
        },
    )?;
    Ok(result.unwrap_or_else(|| ParameterAssembly {
        records: Vec::new(),
        trailing_pointer_analysis: BTreeMap::new(),
        quarantined: Vec::new(),
        recoveries: crate::card::FramingRecoveries::default(),
        given_up: section_given_up,
    }))
}

fn parse_owned(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    output: &mut PartOutput,
    owned: &Ownership<'_>,
    lines: &BTreeMap<u32, &PhysicalLine>,
    global: &ResolvedGlobal,
    source_parts: &mut ConflictParts,
    source_points: Option<&crate::entities::geometry::point::SourcePointProjector<'_>>,
) -> Result<ParsedOwned, CodecError> {
    if owned.cards.is_empty() {
        return super::parse_legacy_owned(owned, lines, global);
    }
    let mut cards = Vec::new();
    for sequence in &owned.cards {
        let card = lines
            .get(sequence)
            .ok_or_else(|| CodecError::malformed("IGES owned Parameter card is absent"))?;
        append_parameter_item(ctx, &mut cards, *card)?;
    }
    if !contiguous(source, &cards)? {
        return super::parse_legacy_owned(owned, lines, global);
    }
    let extent = extent(source, &cards)?;
    if let Some(defect) = owned.quarantine {
        if defect != ParameterDefect::OwnershipConflict || !source_parts.located(extent) {
            return super::parse_legacy_owned(owned, lines, global);
        }
        if !source_parts.claim(extent) {
            return Ok(ParsedOwned::GivenUp);
        }
    }
    let mut inputs = Vec::new();
    let mut card_boundaries = Vec::new();
    for card in &cards {
        let start = absolute(source, card.offset)?;
        let payload = source
            .child(start, start + 64)
            .ok_or_else(|| CodecError::malformed("IGES Parameter payload exceeds its source"))?;
        append_parameter_item(ctx, &mut inputs, payload)?;
        let end = inputs
            .len()
            .checked_mul(64)
            .ok_or_else(|| CodecError::malformed("IGES Parameter stream length overflows"))?;
        append_parameter_item(ctx, &mut card_boundaries, end)?;
    }
    let stream = ctx.concat_named_views(
        &inputs,
        &format!(
            "iges/parameter/{}/{}/{}",
            ctx.capture_part_extent(source)?.stream(),
            extent.start(),
            extent.end()
        ),
    )?;
    let tokenize: super::FieldTokenizer = if owned.entry.entity_type == 116 && owned.entry.form == 0
    {
        super::tokenize_source_fields
    } else {
        super::tokenize_with_limits
    };
    let mut framing = envelopes(ctx, source, &cards, 64)?;
    if owned.entry.entity_type != 306 {
        match tokenize(
            stream.window(),
            &card_boundaries,
            global.parameter_delimiter,
            global.record_delimiter,
            global.global_table(),
            global.numeric_limits(),
        ) {
            Ok((_, end)) => {
                let logical = end - 1;
                let card = cards.get(logical / 64).ok_or_else(|| {
                    CodecError::malformed("IGES Parameter terminal exceeds its cards")
                })?;
                let terminal = absolute(source, card.offset)? + logical % 64;
                append_parameter_item(
                    ctx,
                    &mut framing,
                    u64_from_index(terminal)..u64_from_index(terminal + 1),
                )?;
            }
            Err(super::TokenizeFailure::Defect(_, _)) => {}
            Err(super::TokenizeFailure::Error(error)) => return Err(error),
        }
    }
    framing.sort_unstable_by_key(|range| (range.start, range.end));
    let id = format!("iges:entity:directory#{}", owned.entry.sequence);
    let parsed = ctx.decode_part(
        output,
        extent,
        spec("iges.parameter_record", framing)?,
        |staged| {
            if let Some(defect) = owned.quarantine {
                return Err(failure(
                    owned.entry.sequence,
                    OwnedParseFailure::Defect(defect, None),
                ));
            }
            ctx.record(&id, extent, || {
                ctx.retain_bytes(
                    u64_from_index(stream.window().len()),
                    "IGES Parameter syntax bytes",
                )?;
                let mut bytes =
                    ctx.alloc_filled(stream.window().len(), 0u8, "IGES Parameter syntax bytes")?;
                bytes.copy_from_slice(stream.window());
                let owned_bytes = OwnedParameterBytes {
                    bytes,
                    card_boundaries,
                };
                let mut record =
                    super::parse_owned_record_with_tokenizer(owned, owned_bytes, global, tokenize)
                        .map_err(|error| failure(owned.entry.sequence, error))?;
                if owned.entry.entity_type != 306 {
                    for token in record.tokens.iter_mut().skip(1) {
                        if owned.entry.entity_type == 116 && owned.entry.form == 0 {
                            read_point_field(
                                ctx,
                                stream,
                                token,
                                global,
                                owned.entry.sequence,
                                staged,
                            )?;
                        } else if let Some(token) = token {
                            read_token(ctx, stream, token, global, owned.entry.sequence)?;
                        }
                    }
                }
                if let Some(projector) = source_points {
                    projector.emit(ctx, owned.entry, &record, staged)?;
                }
                Ok(record)
            })
        },
    )?;
    Ok(parsed.map_or(ParsedOwned::GivenUp, ParsedOwned::Record))
}

#[derive(Default)]
struct ConflictParts {
    initialized: bool,
    ends: BTreeMap<usize, Option<usize>>,
    reached: BTreeSet<usize>,
}

impl ConflictParts {
    fn new(
        ctx: &DecodeContext<'_>,
        source: View<'_>,
        ownership: &[Ownership<'_>],
        lines: &BTreeMap<u32, &PhysicalLine>,
    ) -> Result<Self, CodecError> {
        let mut ends = BTreeMap::new();
        for owned in ownership {
            if owned.quarantine != Some(ParameterDefect::OwnershipConflict)
                || owned.cards.is_empty()
            {
                continue;
            }
            let mut cards = Vec::new();
            for sequence in &owned.cards {
                let card = lines.get(sequence).ok_or_else(|| {
                    CodecError::malformed("IGES conflicted Parameter card is absent")
                })?;
                append_parameter_item(ctx, &mut cards, *card)?;
            }
            if !contiguous(source, &cards)? {
                continue;
            }
            let extent = extent(source, &cards)?;
            if let Some(end) = ends.get_mut(&extent.start()) {
                if *end != Some(extent.end()) {
                    *end = None;
                }
            } else {
                ends.insert(extent.start(), Some(extent.end()));
            }
        }
        Ok(Self {
            initialized: true,
            ends,
            reached: BTreeSet::new(),
        })
    }

    fn ambiguous(&self) -> bool {
        let mut previous_end = None;
        for (start, end) in &self.ends {
            let Some(end) = end else {
                return true;
            };
            if previous_end.is_some_and(|previous| previous > *start) {
                return true;
            }
            previous_end = Some(*end);
        }
        false
    }

    fn located(&self, extent: View<'_>) -> bool {
        self.ends.get(&extent.start()) == Some(&Some(extent.end()))
    }

    fn claim(&mut self, extent: View<'_>) -> bool {
        self.reached.insert(extent.start())
    }
}

fn read_point_field(
    ctx: &DecodeContext<'_>,
    stream: View<'_>,
    slot: &mut Option<Token>,
    global: &ResolvedGlobal,
    sequence: u32,
    output: &mut PartOutput,
) -> Result<(), CodecError> {
    let Some(mut token) = slot.take() else {
        return Ok(());
    };
    let end = token
        .span
        .end
        .checked_add(1)
        .ok_or_else(|| CodecError::malformed("IGES coordinate field extent overflows"))?;
    let extent = stream
        .child(token.span.start, end)
        .ok_or_else(|| CodecError::malformed("IGES coordinate field exceeds its source"))?;
    let mut framing = Vec::new();
    if let TokenValue::String(value) = &token.value {
        let header_end = token
            .span
            .end
            .checked_sub(value.as_ref().len())
            .ok_or_else(|| {
                CodecError::malformed("IGES coordinate Hollerith header exceeds its field")
            })?;
        append_parameter_item(
            ctx,
            &mut framing,
            u64_from_index(token.span.start)..u64_from_index(header_end),
        )?;
    }
    append_parameter_item(
        ctx,
        &mut framing,
        u64_from_index(token.span.end)..u64_from_index(end),
    )?;
    *slot = ctx.decode_part(
        output,
        extent,
        spec("iges.parameter_field", framing)?,
        |_| {
            if matches!(token.value, TokenValue::Unreadable(_)) {
                let bytes = stream.read_slice(token.span.clone()).ok_or_else(|| {
                    CodecError::malformed("IGES coordinate source token is unreadable")
                })?;
                if matches!(
                    token.value,
                    TokenValue::Unreadable(super::ParameterDefect::NumericCrossesCard)
                ) {
                    return Err(failure(
                        sequence,
                        OwnedParseFailure::Defect(
                            super::ParameterDefect::NumericCrossesCard,
                            Some(token.span.start),
                        ),
                    ));
                }
                let value =
                    super::numeric_with_limits(bytes, 0..bytes.len(), global.numeric_limits())
                        .map_err(|error| failure(sequence, error.into()))?;
                token.value = value.value;
            } else {
                read_token(ctx, stream, &mut token, global, sequence)?;
            }
            Ok(token)
        },
    )?;
    Ok(())
}

fn read_token(
    ctx: &DecodeContext<'_>,
    stream: View<'_>,
    token: &mut Token,
    global: &ResolvedGlobal,
    sequence: u32,
) -> Result<(), CodecError> {
    match &token.value {
        TokenValue::Integer(_) | TokenValue::Real(_) => {
            let bytes = stream
                .read_slice(token.span.clone())
                .ok_or_else(|| CodecError::malformed("IGES numeric source token is unreadable"))?;
            let parsed = super::numeric_with_limits(bytes, 0..bytes.len(), global.numeric_limits())
                .map_err(|error| failure(sequence, error.into()))?;
            token.value = parsed.value;
        }
        TokenValue::String(value) => {
            let start = token
                .span
                .end
                .checked_sub(value.as_ref().len())
                .ok_or_else(|| CodecError::malformed("IGES string payload range overflows"))?;
            let bytes = stream
                .read_slice(start..token.span.end)
                .ok_or_else(|| CodecError::malformed("IGES string source payload is unreadable"))?;
            ctx.retain_bytes(u64_from_index(bytes.len()), "IGES typed Parameter string")?;
            let mut value = ctx.alloc_filled(bytes.len(), 0u8, "IGES typed Parameter string")?;
            value.copy_from_slice(bytes);
            token.value = TokenValue::String(value.into());
        }
        TokenValue::Omitted | TokenValue::Unreadable(_) => {}
    }
    Ok(())
}

fn failure(sequence: u32, error: OwnedParseFailure) -> CodecError {
    match error {
        OwnedParseFailure::Error(error) => error,
        OwnedParseFailure::Defect(defect, offset) => CodecError::malformed(format!(
            "IGES Parameter Data D{sequence}, logical offset {offset:?}: {} ({})",
            defect.describe(),
            defect.key()
        )),
    }
}

fn contiguous(source: View<'_>, cards: &[&PhysicalLine]) -> Result<bool, CodecError> {
    for pair in cards.windows(2) {
        if physical_end(source, pair[0])? != absolute(source, pair[1].offset)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn extent<'a>(source: View<'a>, cards: &[&PhysicalLine]) -> Result<View<'a>, CodecError> {
    let first = cards
        .first()
        .ok_or_else(|| CodecError::malformed("IGES Parameter part has no cards"))?;
    let last = cards
        .last()
        .ok_or_else(|| CodecError::malformed("IGES Parameter part has no final card"))?;
    source
        .child(absolute(source, first.offset)?, physical_end(source, last)?)
        .ok_or_else(|| CodecError::malformed("IGES Parameter part exceeds its source"))
}

fn absolute(source: View<'_>, offset: u64) -> Result<usize, CodecError> {
    usize::try_from(offset)
        .ok()
        .and_then(|offset| source.start().checked_add(offset))
        .ok_or_else(|| CodecError::malformed("IGES Parameter source offset overflows"))
}

fn physical_end(source: View<'_>, card: &PhysicalLine) -> Result<usize, CodecError> {
    absolute(source, card.offset)?
        .checked_add(card.payload.len())
        .and_then(|end| end.checked_add(card.line_ending().len()))
        .ok_or_else(|| CodecError::malformed("IGES Parameter card extent overflows"))
}

fn envelopes(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    cards: &[&PhysicalLine],
    payload_columns: usize,
) -> Result<Vec<Range<u64>>, CodecError> {
    let mut framing = Vec::new();
    for card in cards {
        let start = absolute(source, card.offset)? + payload_columns;
        append_parameter_item(
            ctx,
            &mut framing,
            u64_from_index(start)..u64_from_index(physical_end(source, card)?),
        )?;
    }
    Ok(framing)
}

fn spec(name: &str, framing: Vec<Range<u64>>) -> Result<PartSpec, CodecError> {
    let level = PartLevel::parse(name).ok_or_else(|| {
        CodecError::malformed("IGES Parameter part level is absent from Appendix C")
    })?;
    Ok(PartSpec::new(level, framing))
}

/// Push parser working storage. It is not retained bytes
/// (`docs/decode-resource-caps.md`, "Retained bytes"); the input length bounds
/// it.
fn append_parameter_item<T>(
    ctx: &DecodeContext<'_>,
    out: &mut Vec<T>,
    value: T,
) -> Result<(), CodecError> {
    let bytes = u64_from_index(size_of::<T>());
    out.try_reserve(1).map_err(|_| {
        ResourceLimit::allocation_failed(
            ResourceDimension::RetainedBytes,
            ctx.policy().limits.max_retained_bytes,
            bytes,
            "IGES Parameter parser storage",
        )
    })?;
    out.push(value);
    Ok(())
}

#[cfg(test)]
mod tests;
