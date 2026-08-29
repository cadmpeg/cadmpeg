// SPDX-License-Identifier: Apache-2.0
//! Exact physical-line and fixed-card framing.

use crate::loss::IgesLossCode;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::{CodecError, ContainerEntry, ContainerSummary};
use cadmpeg_ir::codec::Confidence;
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::SourceProvenance;
use std::collections::BTreeMap;

const CARD_WIDTH: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Section {
    Start,
    Global,
    Directory,
    Parameter,
    Terminate,
}

impl Section {
    fn parse(marker: u8) -> Option<Self> {
        match marker {
            b'S' => Some(Self::Start),
            b'G' => Some(Self::Global),
            b'D' => Some(Self::Directory),
            b'P' => Some(Self::Parameter),
            b'T' => Some(Self::Terminate),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Global => "global",
            Self::Directory => "directory-entry",
            Self::Parameter => "parameter-data",
            Self::Terminate => "terminate",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineEnding {
    Lf,
    CrLf,
    Cr,
    None,
}

impl LineEnding {
    fn name(self) -> &'static str {
        match self {
            Self::Lf => "lf",
            Self::CrLf => "crlf",
            Self::Cr => "cr",
            Self::None => "none",
        }
    }

    fn bytes(self) -> &'static [u8] {
        match self {
            Self::Lf => b"\n",
            Self::CrLf => b"\r\n",
            Self::Cr => b"\r",
            Self::None => b"",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PhysicalLine {
    pub(crate) offset: u64,
    pub(crate) payload: Vec<u8>,
    ending: LineEnding,
    pub(crate) section: Option<Section>,
    pub(crate) sequence: Option<u32>,
    fused_cards: Option<usize>,
}

impl PhysicalLine {
    pub(crate) fn line_ending(&self) -> &'static [u8] {
        self.ending.bytes()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CardScan<'a> {
    pub(crate) source: &'a [u8],
    pub(crate) lines: Vec<PhysicalLine>,
    pub(crate) recoveries: FramingRecoveries,
}

/// One class of card-framing declaration the decoder took from the census.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FramingDefect {
    CardBoundary,
    Sequence,
    ParameterOwner,
    UnclaimedParameterCard,
    TerminateCount,
}

impl FramingDefect {
    fn description(self) -> &'static str {
        match self {
            Self::CardBoundary => "a card boundary",
            Self::Sequence => "a card sequence",
            Self::ParameterOwner => "a Parameter Data card owner",
            Self::UnclaimedParameterCard => "an unclaimed Parameter Data card",
            Self::TerminateCount => "a declared Terminate count",
        }
    }

    fn unit(self) -> &'static str {
        match self {
            Self::TerminateCount => "declaration",
            Self::CardBoundary
            | Self::Sequence
            | Self::ParameterOwner
            | Self::UnclaimedParameterCard => "card",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FramingRecovery {
    position: usize,
    offset: u64,
    declared: String,
    used: String,
    count: usize,
}

/// Recovered framing declarations, at most one per section and defect class.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FramingRecoveries(BTreeMap<(Section, FramingDefect), FramingRecovery>);

impl FramingRecoveries {
    pub(crate) fn record(
        &mut self,
        section: Section,
        defect: FramingDefect,
        position: usize,
        offset: u64,
        declared: impl Into<String>,
        used: impl Into<String>,
    ) {
        self.0
            .entry((section, defect))
            .and_modify(|recovery| recovery.count = recovery.count.saturating_add(1))
            .or_insert_with(|| FramingRecovery {
                position,
                offset,
                declared: declared.into(),
                used: used.into(),
                count: 1,
            });
    }

    pub(crate) fn merge(&mut self, other: Self) {
        for (key, recovery) in other.0 {
            match self.0.get_mut(&key) {
                Some(held) => {
                    held.count = held.count.saturating_add(recovery.count);
                    if recovery.position < held.position {
                        held.position = recovery.position;
                        held.offset = recovery.offset;
                        held.declared = recovery.declared;
                        held.used = recovery.used;
                    }
                }
                None => {
                    self.0.insert(key, recovery);
                }
            }
        }
    }

    pub(crate) fn notes(&self) -> Vec<LossNote> {
        self.0
            .iter()
            .map(|((section, defect), recovery)| {
                IgesLossCode::CardFramingRecovered
                    .note(format!(
                        "IGES {} section recovered {} from the card census: the first offending {} is at position {} in the section, which declared {}, and the decoder used {}; {} {} in this section required the same recovery",
                        section.name(),
                        defect.description(),
                        defect.unit(),
                        recovery.position,
                        recovery.declared,
                        recovery.used,
                        recovery.count,
                        defect.unit(),
                    ))
                    .with_provenance(SourceProvenance {
                        format: "iges".into(),
                        stream: "iges".into(),
                        offset: recovery.offset,
                        tag: Some(format!("{}:framing", section.name())),
                    })
            })
            .collect()
    }
}

fn take_line(input: &[u8]) -> Option<(&[u8], &[u8])> {
    let ending_at = memchr::memchr2(b'\r', b'\n', input)?;
    let ending_len =
        usize::from(input[ending_at] == b'\r' && input.get(ending_at + 1) == Some(&b'\n')) + 1;
    Some((&input[..ending_at], &input[ending_at + ending_len..]))
}

fn sequence(card: &[u8]) -> Option<u32> {
    let field = card.get(73..80)?;
    let first_digit = field.iter().position(|byte| *byte != b' ')?;
    let digits = &field[first_digit..];
    if digits.iter().any(|byte| !byte.is_ascii_digit()) {
        return None;
    }
    let mut value = 0_u32;
    for digit in digits.iter().copied() {
        value = value
            .checked_mul(10)?
            .checked_add(u32::from(digit - b'0'))?;
    }
    (value > 0).then_some(value)
}

fn header(line: &[u8]) -> Option<(u8, u32)> {
    let card = line.get(..CARD_WIDTH)?;
    let marker = *card.get(72)?;
    Section::parse(marker)?;
    Some((marker, sequence(card)?))
}

fn marker(card: &[u8]) -> Option<Section> {
    card.get(72).copied().and_then(Section::parse)
}

/// Confidence that a terminator-free stream is a sequence of 80-column cards.
///
/// [IGES 5.3 §2.2](https://paulbourke.net/dataformats/iges/IGES.pdf) makes the
/// line terminator a media convention, so a stride of marked card images is a
/// Fixed ASCII file even with no terminator in it.
fn detect_card_stride(prefix: &[u8]) -> Confidence {
    let mut cards = prefix.chunks_exact(CARD_WIDTH);
    let (Some(first), Some(second)) = (cards.next(), cards.next()) else {
        return Confidence::No;
    };
    if header(first) != Some((b'S', 1)) || !matches!(header(second), Some((b'S', 2) | (b'G', 1))) {
        return Confidence::No;
    }
    if cards.any(|card| marker(card).is_none()) {
        return Confidence::No;
    }
    Confidence::High
}

/// The second card image of a stream whose first line is `first`.
///
/// [IGES 5.3 §2.2](https://paulbourke.net/dataformats/iges/IGES.pdf) makes the
/// line terminator a media convention that separates card images, so the second
/// card image is the second card of the first line when that line divides into
/// cards, and the first card of the next line otherwise.
fn second_card_image<'a>(first: &'a [u8], rest: &'a [u8]) -> Option<&'a [u8]> {
    if fused_card_count(first).is_some_and(|count| count > 1) {
        return first.get(CARD_WIDTH..CARD_WIDTH * 2);
    }
    take_line(rest).map(|(second, _)| second)
}

pub(crate) fn detect_fixed_ascii(prefix: &[u8]) -> Confidence {
    let Some((first, rest)) = take_line(prefix) else {
        return detect_card_stride(prefix);
    };
    if header(first) != Some((b'S', 1)) {
        return Confidence::No;
    }
    let Some(second) = second_card_image(first, rest) else {
        return Confidence::No;
    };
    match header(second) {
        Some((b'S', 2) | (b'G', 1)) => Confidence::High,
        _ => Confidence::No,
    }
}

/// The card count of a pre-Terminate line whose payload divides into cards.
fn fused_card_count(payload: &[u8]) -> Option<usize> {
    let count = payload
        .len()
        .is_multiple_of(CARD_WIDTH)
        .then_some(payload.len() / CARD_WIDTH)?;
    payload
        .chunks_exact(CARD_WIDTH)
        .all(|card| marker(card).is_some())
        .then_some(count)
}

fn physical_lines(
    source: &[u8],
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Vec<PhysicalLine>, CodecError> {
    let mut lines = Vec::new();
    let mut start = 0_usize;
    let mut terminated = false;
    while start < source.len() {
        let relative_end = memchr::memchr2(b'\r', b'\n', &source[start..]);
        let (payload_end, ending, next) = match relative_end {
            Some(relative) => {
                let end = start
                    .checked_add(relative)
                    .ok_or_else(|| CodecError::Malformed("IGES line offset overflow".into()))?;
                if source[end] == b'\r' && source.get(end + 1) == Some(&b'\n') {
                    (end, LineEnding::CrLf, end + 2)
                } else if source[end] == b'\r' {
                    (end, LineEnding::Cr, end + 1)
                } else {
                    (end, LineEnding::Lf, end + 1)
                }
            }
            None => (source.len(), LineEnding::None, source.len()),
        };
        let payload_width = payload_end.saturating_sub(start);
        let cards = if payload_width > CARD_WIDTH && !terminated {
            let fixed_end = start
                .checked_add(CARD_WIDTH)
                .ok_or_else(|| CodecError::Malformed("IGES line offset overflow".into()))?;
            let fixed = source.get(start..fixed_end).ok_or_else(|| {
                CodecError::Malformed("IGES fixed card exceeds the source image".into())
            })?;
            if marker(fixed) == Some(Section::Terminate) {
                1
            } else {
                fused_card_count(&source[start..payload_end]).ok_or_else(|| {
                    CodecError::Malformed(
                        "IGES Fixed ASCII physical line exceeds 80 bytes before Terminate".into(),
                    )
                })?
            }
        } else {
            1
        };
        let mut card_start = start;
        for index in 0..cards {
            let card_end = card_start.saturating_add(CARD_WIDTH).min(payload_end);
            let payload = source[card_start..card_end].to_vec();
            let marked = !terminated && payload.len() == CARD_WIDTH;
            let section = marked.then(|| marker(&payload)).flatten();
            let sequence = marked.then(|| sequence(&payload)).flatten();
            let card_ending = if card_end == payload_end {
                ending
            } else {
                LineEnding::None
            };
            charge_line(ctx)?;
            lines.push(PhysicalLine {
                offset: u64::try_from(card_start)
                    .map_err(|_| CodecError::Malformed("IGES source offset exceeds u64".into()))?,
                payload,
                ending: card_ending,
                section,
                sequence,
                fused_cards: (cards > 1 && index == 0).then_some(cards),
            });
            terminated = terminated || section == Some(Section::Terminate);
            card_start = card_end;
        }
        if card_start != payload_end {
            charge_line(ctx)?;
            lines.push(PhysicalLine {
                offset: u64::try_from(card_start)
                    .map_err(|_| CodecError::Malformed("IGES source offset exceeds u64".into()))?,
                payload: source[card_start..payload_end].to_vec(),
                ending,
                section: None,
                sequence: None,
                fused_cards: None,
            });
        }
        start = next;
    }
    Ok(lines)
}

/// Order the sections and make each card's position inside its section its
/// sequence, recording every declaration the position replaced.
fn frame_sections(
    lines: &mut [PhysicalLine],
    recoveries: &mut FramingRecoveries,
) -> Result<(), CodecError> {
    let mut section = None;
    let mut position = 1_usize;
    let mut terminated = false;
    for line in lines.iter_mut() {
        if terminated {
            continue;
        }
        let current = line.section.ok_or_else(|| {
            crate::error::malformed(format!(
                "IGES physical line at offset {} is unsequenced before Terminate",
                line.offset
            ))
        })?;
        if section != Some(current) {
            if section.is_some_and(|previous| current <= previous) {
                return Err(CodecError::malformed(format_args!(
                    "IGES section {} is out of order",
                    current.name()
                )));
            }
            section = Some(current);
            position = 1;
        }
        let recovered = u32::try_from(position)
            .map_err(|_| CodecError::Malformed("IGES section sequence overflow".into()))?;
        if let Some(count) = line.fused_cards {
            recoveries.record(
                current,
                FramingDefect::CardBoundary,
                position,
                line.offset,
                format!(
                    "one physical line of {} bytes",
                    count.saturating_mul(CARD_WIDTH)
                ),
                format!("{count} 80-column cards"),
            );
        }
        if line.sequence != Some(recovered) {
            recoveries.record(
                current,
                FramingDefect::Sequence,
                position,
                line.offset,
                line.sequence
                    .map_or_else(|| "no valid sequence".to_owned(), |value| value.to_string()),
                recovered.to_string(),
            );
        }
        line.sequence = Some(recovered);
        position = position
            .checked_add(1)
            .ok_or_else(|| CodecError::Malformed("IGES section sequence overflow".into()))?;
        terminated = current == Section::Terminate;
    }
    if lines.first().and_then(|line| line.section) != Some(Section::Start) || !terminated {
        return Err(CodecError::Malformed(
            "IGES Fixed ASCII requires Start through Terminate sections".into(),
        ));
    }
    Ok(())
}

/// Replace each Terminate count that disagrees with the card census.
fn terminate_counts(lines: &[PhysicalLine], recoveries: &mut FramingRecoveries) {
    let Some(terminate) = lines
        .iter()
        .find(|line| line.section == Some(Section::Terminate))
    else {
        return;
    };
    let Some(data) = terminate.payload.get(..32) else {
        return;
    };
    let expected = [
        (b'S', Section::Start),
        (b'G', Section::Global),
        (b'D', Section::Directory),
        (b'P', Section::Parameter),
    ];
    for (field, (marker, section)) in data.chunks_exact(8).zip(expected) {
        let declared = (field[0] == marker)
            .then(|| std::str::from_utf8(&field[1..]).ok().map(str::trim))
            .flatten()
            .filter(|text| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|text| text.parse::<usize>().ok());
        let census = lines
            .iter()
            .filter(|line| line.section == Some(section))
            .count();
        if declared != Some(census) {
            recoveries.record(
                Section::Terminate,
                FramingDefect::TerminateCount,
                1,
                terminate.offset,
                format!(
                    "{} count {}",
                    section.name(),
                    String::from_utf8_lossy(field).trim()
                ),
                format!("{} count {census}", section.name()),
            );
        }
    }
}

#[cfg(test)]
pub(crate) fn scan(source: &[u8]) -> Result<CardScan<'_>, CodecError> {
    scan_with_context(source, None)
}

pub(crate) fn scan_with_context<'a>(
    source: &'a [u8],
    ctx: Option<&DecodeContext<'_>>,
) -> Result<CardScan<'a>, CodecError> {
    if source.is_empty() {
        return Err(CodecError::WrongFormat("empty IGES source".into()));
    }
    let mut lines = physical_lines(source, ctx)?;
    let mut recoveries = FramingRecoveries::default();
    frame_sections(&mut lines, &mut recoveries)?;
    terminate_counts(&lines, &mut recoveries);
    Ok(CardScan {
        source,
        lines,
        recoveries,
    })
}

fn charge_line(ctx: Option<&DecodeContext<'_>>) -> Result<(), CodecError> {
    ctx.map_or(Ok(()), |ctx| ctx.charge_collection_items(1, "iges_cards"))
}

pub(crate) fn summarize(
    scan: &CardScan<'_>,
    primary: cadmpeg_core::dialect::DialectMatch,
) -> ContainerSummary {
    let sections = [
        Section::Start,
        Section::Global,
        Section::Directory,
        Section::Parameter,
        Section::Terminate,
    ];
    let mut entries = sections
        .into_iter()
        .filter_map(|section| {
            let lines = scan
                .lines
                .iter()
                .filter(|line| line.section == Some(section))
                .collect::<Vec<_>>();
            if lines.is_empty() {
                return None;
            }
            let mut endings = BTreeMap::<&str, usize>::new();
            for line in &lines {
                *endings.entry(line.ending.name()).or_default() += 1;
            }
            let mut attributes = BTreeMap::new();
            attributes.insert("cards".into(), lines.len().to_string());
            attributes.insert(
                "line_endings".into(),
                endings
                    .into_iter()
                    .map(|(name, count)| format!("{name}:{count}"))
                    .collect::<Vec<_>>()
                    .join(","),
            );
            let size = lines.iter().fold(0_u64, |size, line| {
                size.saturating_add(
                    u64::try_from(line.payload.len() + line.ending.bytes().len())
                        .unwrap_or(u64::MAX),
                )
            });
            Some(ContainerEntry {
                name: section.name().into(),
                role: "section".into(),
                compression: "none".into(),
                compressed_size: size,
                uncompressed_size: size,
                attributes,
            })
        })
        .collect::<Vec<_>>();
    let terminate_index = scan
        .lines
        .iter()
        .position(|line| line.section == Some(Section::Terminate));
    let post_terminate = terminate_index
        .and_then(|index| scan.lines.get(index + 1..))
        .unwrap_or_default();
    if !post_terminate.is_empty() {
        let size = post_terminate.iter().fold(0_u64, |size, line| {
            size.saturating_add(
                u64::try_from(line.payload.len() + line.ending.bytes().len()).unwrap_or(u64::MAX),
            )
        });
        entries.push(ContainerEntry {
            name: "post-terminate".into(),
            role: "retained-trailing-records".into(),
            compression: "none".into(),
            compressed_size: size,
            uncompressed_size: size,
            attributes: BTreeMap::from([("records".into(), post_terminate.len().to_string())]),
        });
    }
    ContainerSummary::classified(
        cadmpeg_core::dialect::DialectLayers::of(primary),
        "fixed-ascii",
        entries,
        vec![format!("source_bytes={}", scan.source.len())],
    )
}

impl CardScan<'_> {
    pub(crate) fn post_terminate_count(&self) -> usize {
        self.lines
            .iter()
            .position(|line| line.section == Some(Section::Terminate))
            .map_or(0, |index| self.lines.len().saturating_sub(index + 1))
    }
}

#[cfg(test)]
mod quarantine_tests;
#[cfg(test)]
mod tests;
