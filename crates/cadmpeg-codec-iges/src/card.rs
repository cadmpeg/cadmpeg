// SPDX-License-Identifier: Apache-2.0
//! Exact physical-line and fixed-card framing.

use cadmpeg_core::container::{ContainerRole, EntryStorage, VerbatimLabel};

use crate::error::malformed;
use crate::loss::IgesLossCode;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::codec::Confidence;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::ContainerSummary;
use cadmpeg_ir::SourceProvenance;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;

pub(crate) const CARD_WIDTH: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
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

/// A section a file can end in when it has no Terminate card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenSection {
    Start,
    Global,
    Directory,
    Parameter,
}

impl OpenSection {
    const fn from_section(section: Section) -> Option<Self> {
        match section {
            Section::Start => Some(Self::Start),
            Section::Global => Some(Self::Global),
            Section::Directory => Some(Self::Directory),
            Section::Parameter => Some(Self::Parameter),
            Section::Terminate => None,
        }
    }

    const fn ends_before_directory(self) -> bool {
        matches!(self, Self::Start | Self::Global)
    }

    const fn section(self) -> Section {
        match self {
            Self::Start => Section::Start,
            Self::Global => Section::Global,
            Self::Directory => Section::Directory,
            Self::Parameter => Section::Parameter,
        }
    }
}

/// How the Fixed ASCII scan ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceEnd {
    Terminated,
    EndsIn(OpenSection),
}

impl SourceEnd {
    pub(crate) const fn ends_before_directory(self) -> bool {
        match self {
            Self::Terminated => false,
            Self::EndsIn(section) => section.ends_before_directory(),
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
}

impl PhysicalLine {
    pub(crate) fn line_ending(&self) -> &'static [u8] {
        self.ending.bytes()
    }
}

#[derive(Debug, Clone)]
pub(crate) enum ScannedLine {
    Card {
        section: Section,
        sequence: u32,
        line: PhysicalLine,
    },
    Trailing(PhysicalLine),
    DroppedFinal(PhysicalLine),
}

impl ScannedLine {
    pub(crate) fn physical(&self) -> &PhysicalLine {
        match self {
            Self::Card { line, .. } | Self::Trailing(line) | Self::DroppedFinal(line) => line,
        }
    }
}

struct UnframedLine {
    line: PhysicalLine,
    section: Option<Section>,
    sequence: Option<u32>,
    fused_cards: Option<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct CardScan<'a> {
    source: &'a [u8],
    pub(crate) lines: Vec<ScannedLine>,
    pub(crate) recoveries: FramingRecoveries,
    pub(crate) source_end: SourceEnd,
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

/// The final physical line omitted from section framing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DroppedFinalCard {
    pub(crate) offset: u64,
    pub(crate) byte_length: u64,
}

/// The one framing record for a file that has no Terminate card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AbsentTerminate {
    pub(crate) section: OpenSection,
    pub(crate) file_end: u64,
    pub(crate) dropped_final_card: Option<DroppedFinalCard>,
}

struct DeclaredSequence(Option<u32>);

impl fmt::Display for DeclaredSequence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(sequence) => write!(formatter, "{sequence}"),
            None => formatter.write_str("no valid sequence"),
        }
    }
}

struct TrimmedLossyField<'a>(&'a [u8]);

impl fmt::Display for TrimmedLossyField<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut output = [0_u8; 24];
        let mut used = 0_usize;
        let mut remaining = self.0;
        loop {
            match std::str::from_utf8(remaining) {
                Ok(valid) => {
                    output[used..used + valid.len()].copy_from_slice(valid.as_bytes());
                    used += valid.len();
                    break;
                }
                Err(error) => {
                    let prefix = &remaining[..error.valid_up_to()];
                    output[used..used + prefix.len()].copy_from_slice(prefix);
                    used += prefix.len();
                    output[used..used + 3].copy_from_slice("�".as_bytes());
                    used += 3;
                    let invalid = error.error_len().unwrap_or(remaining.len() - prefix.len());
                    remaining = &remaining[prefix.len() + invalid..];
                }
            }
        }
        let text = std::str::from_utf8(&output[..used]).map_err(|_| fmt::Error)?;
        formatter.write_str(text.trim())
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
pub(crate) struct FramingRecoveries {
    records: BTreeMap<(Section, FramingDefect), FramingRecovery>,
    absent_terminate: Option<AbsentTerminate>,
}

impl FramingRecoveries {
    pub(crate) fn record(
        &mut self,
        key: (Section, FramingDefect),
        position: usize,
        offset: u64,
        declared: fmt::Arguments<'_>,
        used: fmt::Arguments<'_>,
    ) -> Result<(), CodecError> {
        if let Some(recovery) = self.records.get_mut(&key) {
            recovery.count = recovery
                .count
                .checked_add(1)
                .ok_or_else(|| malformed("IGES framing recovery count overflows"))?;
            return Ok(());
        }
        let declared = declared.to_string();
        let used = used.to_string();
        self.records.insert(
            key,
            FramingRecovery {
                position,
                offset,
                declared,
                used,
                count: 1,
            },
        );
        Ok(())
    }

    pub(crate) fn set_absent_terminate(&mut self, absent: AbsentTerminate) {
        self.absent_terminate = Some(absent);
    }

    #[cfg(test)]
    pub(crate) fn absent_terminate(&self) -> Option<AbsentTerminate> {
        self.absent_terminate
    }

    pub(crate) fn merge(&mut self, other: Self) -> Result<(), CodecError> {
        for (key, recovery) in other.records {
            match self.records.get_mut(&key) {
                Some(held) => {
                    held.count = held
                        .count
                        .checked_add(recovery.count)
                        .ok_or_else(|| malformed("IGES framing recovery count overflows"))?;
                    if recovery.position < held.position {
                        held.position = recovery.position;
                        held.offset = recovery.offset;
                        held.declared = recovery.declared;
                        held.used = recovery.used;
                    }
                }
                None => {
                    self.records.insert(key, recovery);
                }
            }
        }
        Ok(())
    }

    pub(crate) fn notes(&self) -> Vec<LossNote> {
        let mut notes = Vec::new();
        for ((section, defect), recovery) in &self.records {
            let message = format!(
                "IGES {} section recovered {} from the card census: the first offending \
                 {} is at position {} in the section, which declared {}, and the decoder \
                 used {}; {} {} in this section required the same recovery",
                section.name(),
                defect.description(),
                defect.unit(),
                recovery.position,
                recovery.declared,
                recovery.used,
                recovery.count,
                defect.unit()
            );
            let tag = format!("{}:framing", section.name());
            let code = IgesLossCode::CardFramingRecovered;

            notes.push(
                code.note(message).with_provenance(
                    SourceProvenance::in_stream(
                        "iges",
                        cadmpeg_ir::stream_name!("iges"),
                        recovery.offset,
                    )
                    .with_tag(tag),
                ),
            );
        }
        if let Some(absent) = self.absent_terminate {
            let section = absent.section.section();
            let message = absent_terminate_message(absent);
            let tag = format!("{}:framing", section.name());
            let code = IgesLossCode::CardFramingRecovered;
            notes.push(
                code.note(message).with_provenance(
                    SourceProvenance::in_stream(
                        "iges",
                        cadmpeg_ir::stream_name!("iges"),
                        absent.file_end,
                    )
                    .with_tag(tag),
                ),
            );
        }
        notes
    }
}

fn absent_terminate_message(absent: AbsentTerminate) -> String {
    let ended = format!(
        "IGES Fixed ASCII file ends in the {} section at byte offset {} and has no Terminate card",
        absent.section.section().name(),
        absent.file_end
    );
    match absent.dropped_final_card {
        Some(dropped) => format!(
            "{ended}; the final physical line at byte offset {} has {} bytes",
            dropped.offset, dropped.byte_length
        ),
        None => ended,
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
    if !matches!(header(first), Some((b'S', 1..)))
        || !matches!(header(second), Some((b'S' | b'G', 1..)))
    {
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

fn exactly_one_start_card(bytes: &[u8]) -> bool {
    let payload = match memchr::memchr2(b'\r', b'\n', bytes) {
        Some(at) => {
            let ending_len =
                usize::from(bytes[at] == b'\r' && bytes.get(at + 1) == Some(&b'\n')) + 1;
            if at.checked_add(ending_len) != Some(bytes.len()) {
                return false;
            }
            &bytes[..at]
        }
        None => bytes,
    };
    payload.len() == CARD_WIDTH && matches!(header(payload), Some((b'S', 1..)))
}

/// Whole-file Fixed ASCII for a read that reached end of file.
///
/// One Start card, or that card plus one final physical line shorter than 80
/// bytes. A second physical line of 80 bytes stays with the two-card rule.
pub(crate) fn whole_file_fixed_ascii(bytes: &[u8]) -> bool {
    if exactly_one_start_card(bytes) {
        return true;
    }
    let Some(at) = memchr::memchr2(b'\r', b'\n', bytes) else {
        return false;
    };
    let ending_len = usize::from(bytes[at] == b'\r' && bytes.get(at + 1) == Some(&b'\n')) + 1;
    let Some(rest) = at.checked_add(ending_len).and_then(|end| bytes.get(end..)) else {
        return false;
    };
    let first = &bytes[..at];
    if first.len() != CARD_WIDTH || !matches!(header(first), Some((b'S', 1..))) {
        return false;
    }
    let Some(second_at) = memchr::memchr2(b'\r', b'\n', rest) else {
        return !rest.is_empty() && rest.len() < CARD_WIDTH;
    };
    let second_ending =
        usize::from(rest[second_at] == b'\r' && rest.get(second_at + 1) == Some(&b'\n')) + 1;
    second_at > 0
        && second_at < CARD_WIDTH
        && second_at.checked_add(second_ending) == Some(rest.len())
}

pub(crate) fn detect_fixed_ascii(prefix: &[u8]) -> Confidence {
    let Some((first, rest)) = take_line(prefix) else {
        return detect_card_stride(prefix);
    };
    if !matches!(header(first), Some((b'S', 1..))) {
        return Confidence::No;
    }
    let Some(second) = second_card_image(first, rest) else {
        return Confidence::No;
    };
    match header(second) {
        Some((b'S' | b'G', 1..)) => Confidence::High,
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

fn physical_lines(source: &[u8]) -> Result<Vec<UnframedLine>, CodecError> {
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
        let payload_width = payload_end
            .checked_sub(start)
            .ok_or_else(|| CodecError::Malformed("IGES line offset exceeds payload".into()))?;
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
            let card_end = card_start
                .checked_add(CARD_WIDTH)
                .ok_or_else(|| CodecError::Malformed("IGES card offset overflow".into()))?
                .min(payload_end);

            let payload = source[card_start..card_end].to_vec();
            let marked = !terminated && payload.len() == CARD_WIDTH;
            let section = marked.then(|| marker(&payload)).flatten();
            let sequence = marked.then(|| sequence(&payload)).flatten();
            let card_ending = if card_end == payload_end {
                ending
            } else {
                LineEnding::None
            };

            lines.push(UnframedLine {
                line: PhysicalLine {
                    offset: cadmpeg_core::decode::u64_from_index(card_start),
                    payload,
                    ending: card_ending,
                },
                section,
                sequence,
                fused_cards: (cards > 1 && index == 0).then_some(cards),
            });
            terminated = terminated || section == Some(Section::Terminate);
            card_start = card_end;
        }
        if card_start != payload_end {
            let payload = source[card_start..payload_end].to_vec();

            lines.push(UnframedLine {
                line: PhysicalLine {
                    offset: cadmpeg_core::decode::u64_from_index(card_start),
                    payload,
                    ending,
                },
                section: None,
                sequence: None,
                fused_cards: None,
            });
        }
        start = next;
    }
    Ok(lines)
}

fn dropped_final_card(line: &UnframedLine) -> Result<DroppedFinalCard, CodecError> {
    let payload = u64_from_index(line.line.payload.len());
    let ending = u64_from_index(line.line.ending.bytes().len());
    let byte_length = payload
        .checked_add(ending)
        .ok_or_else(|| malformed("IGES dropped card length overflows"))?;
    Ok(DroppedFinalCard {
        offset: line.line.offset,
        byte_length,
    })
}

/// Order the sections and make each card's position inside its section its
/// sequence, recording every declaration the position replaced.
fn frame_sections(
    mut lines: Vec<UnframedLine>,
    recoveries: &mut FramingRecoveries,
    file_end: u64,
) -> Result<(Vec<ScannedLine>, SourceEnd), CodecError> {
    let has_terminate = lines
        .iter()
        .any(|line| line.section == Some(Section::Terminate));
    let dropped = if has_terminate {
        None
    } else if lines.last().is_some_and(|line| line.section.is_none()) {
        lines.pop()
    } else {
        None
    };
    let mut scanned = Vec::new();

    let mut section = None;
    let mut position = 1_usize;
    let mut terminated = false;
    for raw in lines {
        if terminated {
            scanned.push(ScannedLine::Trailing(raw.line));
            continue;
        }
        let line = &raw.line;
        let current = raw.section.ok_or_else(|| {
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
        if let Some(count) = raw.fused_cards {
            let bytes = count
                .checked_mul(CARD_WIDTH)
                .ok_or_else(|| malformed("IGES fused card byte count overflows"))?;
            recoveries.record(
                (current, FramingDefect::CardBoundary),
                position,
                line.offset,
                format_args!("one physical line of {bytes} bytes"),
                format_args!("{count} 80-column cards"),
            )?;
        }
        if raw.sequence != Some(recovered) {
            recoveries.record(
                (current, FramingDefect::Sequence),
                position,
                line.offset,
                format_args!("{}", DeclaredSequence(raw.sequence)),
                format_args!("{recovered}"),
            )?;
        }
        position = position
            .checked_add(1)
            .ok_or_else(|| CodecError::Malformed("IGES section sequence overflow".into()))?;
        terminated = current == Section::Terminate;
        scanned.push(ScannedLine::Card {
            section: current,
            sequence: recovered,
            line: raw.line,
        });
    }
    if !matches!(
        scanned.first(),
        Some(ScannedLine::Card {
            section: Section::Start,
            ..
        })
    ) {
        return Err(CodecError::Malformed(
            "IGES Fixed ASCII requires a Start section".into(),
        ));
    }
    if terminated {
        return Ok((scanned, SourceEnd::Terminated));
    }
    let last = match scanned.last() {
        Some(ScannedLine::Card { section, .. }) => *section,
        Some(ScannedLine::Trailing(_) | ScannedLine::DroppedFinal(_)) | None => {
            return Err(CodecError::Malformed(
                "IGES Fixed ASCII requires a Start section".into(),
            ));
        }
    };
    let open = OpenSection::from_section(last)
        .ok_or_else(|| CodecError::Malformed("IGES Fixed ASCII requires a Start section".into()))?;
    let dropped_card = dropped.as_ref().map(dropped_final_card).transpose()?;
    recoveries.set_absent_terminate(AbsentTerminate {
        section: open,
        file_end,
        dropped_final_card: dropped_card,
    });
    if let Some(line) = dropped {
        scanned.push(ScannedLine::DroppedFinal(line.line));
    }
    Ok((scanned, SourceEnd::EndsIn(open)))
}

/// Replace each Terminate count that disagrees with the card census.
fn terminate_counts(
    lines: &[ScannedLine],
    recoveries: &mut FramingRecoveries,
) -> Result<(), CodecError> {
    let Some(terminate) = lines.iter().find_map(|line| match line {
        ScannedLine::Card {
            section: Section::Terminate,
            line,
            ..
        } => Some(line),
        _ => None,
    }) else {
        return Ok(());
    };
    let Some(data) = terminate.payload.get(..32) else {
        return Ok(());
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
            .filter(|line| matches!(line, ScannedLine::Card { section: current, .. } if *current == section))
            .count();
        if declared != Some(census) {
            recoveries.record(
                (Section::Terminate, FramingDefect::TerminateCount),
                1,
                terminate.offset,
                format_args!("{} count {}", section.name(), TrimmedLossyField(field)),
                format_args!("{} count {census}", section.name()),
            )?;
        }
    }
    Ok(())
}

pub(crate) fn scan(source: &[u8]) -> Result<CardScan<'_>, CodecError> {
    if source.is_empty() {
        return Err(CodecError::WrongFormat("empty IGES source".into()));
    }
    let lines = physical_lines(source)?;
    let mut recoveries = FramingRecoveries::default();
    let file_end = u64_from_index(source.len());
    let (lines, source_end) = frame_sections(lines, &mut recoveries, file_end)?;
    terminate_counts(&lines, &mut recoveries)?;
    Ok(CardScan {
        source,
        lines,
        recoveries,
        source_end,
    })
}

struct EndingSummary([usize; 4]);

impl fmt::Display for EndingSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for (name, count) in ["cr", "crlf", "lf", "none"].into_iter().zip(self.0) {
            if count == 0 {
                continue;
            }
            if !first {
                formatter.write_str(",")?;
            }
            write!(formatter, "{name}:{count}")?;
            first = false;
        }
        Ok(())
    }
}

pub(crate) fn summarize(
    scan: &CardScan<'_>,
    primary: cadmpeg_core::dialect::DialectMatch,
) -> Result<ContainerSummary, CodecError> {
    let sections = [
        Section::Start,
        Section::Global,
        Section::Directory,
        Section::Parameter,
        Section::Terminate,
    ];
    let mut entries = Vec::new();
    for section in sections {
        let mut line_count = 0_usize;
        let mut size = 0_u64;
        let mut endings = [0_usize; 4];
        for (_, line) in scan.section(section) {
            line_count += 1;
            size = size
                .checked_add(u64_from_index(
                    line.payload.len() + line.ending.bytes().len(),
                ))
                .ok_or_else(|| malformed("IGES card summary section size overflows"))?;
            let index = match line.ending {
                LineEnding::Cr => 0,
                LineEnding::CrLf => 1,
                LineEnding::Lf => 2,
                LineEnding::None => 3,
            };
            endings[index] += 1;
        }
        if line_count == 0 {
            continue;
        }

        let attributes = BTreeMap::from([
            ("cards".to_owned(), line_count.to_string()),
            (
                "line_endings".to_owned(),
                EndingSummary(endings).to_string(),
            ),
        ]);
        entries.push(ContainerEntry {
            name: section.name().to_owned(),
            role: ContainerRole::Section,
            storage: EntryStorage::verbatim(VerbatimLabel::None, size),
            attributes,
        });
    }

    let terminate_index = scan.lines.iter().position(|line| {
        matches!(
            line,
            ScannedLine::Card {
                section: Section::Terminate,
                ..
            }
        )
    });
    let post_terminate = terminate_index
        .and_then(|index| scan.lines.get(index + 1..))
        .unwrap_or_default();
    if !post_terminate.is_empty() {
        let mut size = 0_u64;
        for line in post_terminate {
            let line = line.physical();
            size = size
                .checked_add(u64_from_index(
                    line.payload.len() + line.ending.bytes().len(),
                ))
                .ok_or_else(|| malformed("IGES card summary trailing size overflows"))?;
        }

        let attributes = BTreeMap::from([("records".to_owned(), post_terminate.len().to_string())]);
        entries.push(ContainerEntry {
            name: "post-terminate".to_owned(),
            role: ContainerRole::RetainedTrailingRecords,
            storage: EntryStorage::verbatim(VerbatimLabel::None, size),
            attributes,
        });
    }
    let notes = vec![format!("source_bytes={}", scan.source.len())];
    Ok(ContainerSummary::classified(
        cadmpeg_core::dialect::DialectLayers::of(primary),
        cadmpeg_ir::ContainerKind::FixedAscii,
        entries,
        Vec::new(),
        notes,
    ))
}

impl CardScan<'_> {
    pub(crate) fn section(&self, section: Section) -> impl Iterator<Item = (u32, &PhysicalLine)> {
        self.lines.iter().filter_map(move |line| match line {
            ScannedLine::Card {
                section: current,
                sequence,
                line,
            } if *current == section => Some((*sequence, line)),
            ScannedLine::Card { .. } | ScannedLine::Trailing(_) | ScannedLine::DroppedFinal(_) => {
                None
            }
        })
    }

    pub(crate) fn post_terminate_count(&self) -> usize {
        self.lines
            .iter()
            .position(|line| {
                matches!(
                    line,
                    ScannedLine::Card {
                        section: Section::Terminate,
                        ..
                    }
                )
            })
            .map_or(0, |index| self.lines.len() - (index + 1))
    }
}

#[cfg(test)]
mod quarantine_tests;
#[cfg(test)]
mod tests;
