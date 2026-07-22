// SPDX-License-Identifier: Apache-2.0
//! Exact physical-line and fixed-card framing.

use cadmpeg_ir::codec::Confidence;
use cadmpeg_ir::codec::{CodecError, ContainerEntry, ContainerSummary, ReadSeek};
use std::collections::BTreeMap;
use std::io::Read;

const CARD_WIDTH: usize = 80;
const MAX_SOURCE_BYTES: usize = 256 * 1024 * 1024;

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

    pub(crate) fn bytes(self) -> &'static [u8] {
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
}

impl PhysicalLine {
    pub(crate) fn line_ending(&self) -> &'static [u8] {
        self.ending.bytes()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CardScan {
    pub(crate) source: Vec<u8>,
    pub(crate) lines: Vec<PhysicalLine>,
}

fn take_line(input: &[u8]) -> Option<(&[u8], &[u8])> {
    let ending_at = input
        .iter()
        .position(|byte| matches!(byte, b'\r' | b'\n'))?;
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
    if line.len() != CARD_WIDTH || line.iter().any(|byte| !(b' '..=b'~').contains(byte)) {
        return None;
    }
    Some((*line.get(72)?, sequence(line)?))
}

pub(crate) fn detect_fixed_ascii(prefix: &[u8]) -> Confidence {
    let Some((first, rest)) = take_line(prefix) else {
        return Confidence::No;
    };
    let Some((second, _)) = take_line(rest) else {
        return Confidence::No;
    };
    if header(first) != Some((b'S', 1)) {
        return Confidence::No;
    }
    match header(second) {
        Some((b'S', 2) | (b'G', 1)) => Confidence::High,
        _ => Confidence::No,
    }
}

fn physical_lines(source: &[u8]) -> Result<Vec<PhysicalLine>, CodecError> {
    let mut lines = Vec::new();
    let mut start = 0_usize;
    let mut terminated = false;
    while start < source.len() {
        let relative_end = source[start..]
            .iter()
            .position(|byte| matches!(byte, b'\r' | b'\n'));
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
        let payload = source[start..payload_end].to_vec();
        let section = (!terminated && payload.len() == CARD_WIDTH)
            .then(|| payload.get(72).copied().and_then(Section::parse))
            .flatten();
        let sequence = (!terminated && payload.len() == CARD_WIDTH)
            .then(|| sequence(&payload))
            .flatten();
        lines.push(PhysicalLine {
            offset: u64::try_from(start)
                .map_err(|_| CodecError::Malformed("IGES source offset exceeds u64".into()))?,
            payload,
            ending,
            section,
            sequence,
        });
        terminated = terminated || section == Some(Section::Terminate);
        start = next;
    }
    Ok(lines)
}

fn validate_card_order(lines: &[PhysicalLine]) -> Result<(), CodecError> {
    let mut section = None;
    let mut expected_sequence = 1_u32;
    let mut terminated = false;
    for line in lines {
        if terminated {
            continue;
        }
        let Some(current) = line.section else {
            continue;
        };
        let current_sequence = line.sequence.ok_or_else(|| {
            CodecError::Malformed(format!(
                "IGES card at offset {} has an invalid sequence field",
                line.offset
            ))
        })?;
        if section != Some(current) {
            if section.is_some_and(|previous| current <= previous) {
                return Err(CodecError::Malformed(format!(
                    "IGES section {} is out of order",
                    current.name()
                )));
            }
            section = Some(current);
            expected_sequence = 1;
        }
        if current_sequence != expected_sequence {
            return Err(CodecError::Malformed(format!(
                "IGES {} sequence is {current_sequence}, expected {expected_sequence}",
                current.name()
            )));
        }
        expected_sequence = expected_sequence
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

fn validate_terminate_counts(lines: &[PhysicalLine]) -> Result<(), CodecError> {
    let terminate = lines
        .iter()
        .filter(|line| line.section == Some(Section::Terminate))
        .collect::<Vec<_>>();
    if terminate.len() != 1 {
        return Err(CodecError::Malformed(format!(
            "IGES Fixed ASCII has {} Terminate cards, expected 1",
            terminate.len()
        )));
    }
    let data = terminate[0].payload.get(..32).ok_or_else(|| {
        CodecError::Malformed("IGES Terminate card is shorter than 32 bytes".into())
    })?;
    let expected = [
        (b'S', Section::Start),
        (b'G', Section::Global),
        (b'D', Section::Directory),
        (b'P', Section::Parameter),
    ];
    for (field, (marker, section)) in data.chunks_exact(8).zip(expected) {
        let count = std::str::from_utf8(&field[1..])
            .ok()
            .map(str::trim)
            .filter(|text| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()));
        if field[0] != marker || count.is_none() {
            return Err(CodecError::Malformed(format!(
                "IGES Terminate field for {} is malformed",
                section.name()
            )));
        }
        let declared = count
            .and_then(|text| text.parse::<usize>().ok())
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "IGES Terminate count for {} is out of range",
                    section.name()
                ))
            })?;
        let actual = lines
            .iter()
            .filter(|line| line.section == Some(section))
            .count();
        if declared != actual {
            return Err(CodecError::Malformed(format!(
                "IGES Terminate count for {} is {declared}, actual {actual}",
                section.name()
            )));
        }
    }
    Ok(())
}

pub(crate) fn scan(reader: &mut dyn ReadSeek) -> Result<CardScan, CodecError> {
    let mut source = Vec::new();
    reader
        .take(u64::try_from(MAX_SOURCE_BYTES + 1).unwrap_or(u64::MAX))
        .read_to_end(&mut source)?;
    if source.len() > MAX_SOURCE_BYTES {
        return Err(CodecError::Malformed(format!(
            "IGES source exceeds {MAX_SOURCE_BYTES} byte limit"
        )));
    }
    if source.is_empty() {
        return Err(CodecError::WrongFormat("empty IGES source".into()));
    }
    let lines = physical_lines(&source)?;
    validate_card_order(&lines)?;
    validate_terminate_counts(&lines)?;
    Ok(CardScan { source, lines })
}

pub(crate) fn summarize(scan: &CardScan) -> ContainerSummary {
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
                let ending = match line.ending {
                    LineEnding::CrLf => 2,
                    LineEnding::Lf | LineEnding::Cr => 1,
                    LineEnding::None => 0,
                };
                size.saturating_add(u64::try_from(line.payload.len()).unwrap_or(u64::MAX) + ending)
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
            let ending = match line.ending {
                LineEnding::CrLf => 2,
                LineEnding::Lf | LineEnding::Cr => 1,
                LineEnding::None => 0,
            };
            size.saturating_add(u64::try_from(line.payload.len()).unwrap_or(u64::MAX) + ending)
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
    let noncanonical = scan
        .lines
        .iter()
        .take_while(|line| line.section != Some(Section::Terminate))
        .filter(|line| line.section.is_none())
        .collect::<Vec<_>>();
    if !noncanonical.is_empty() {
        let size = noncanonical.iter().fold(0_u64, |size, line| {
            size.saturating_add(
                u64::try_from(line.payload.len()).unwrap_or(u64::MAX)
                    + u64::try_from(line.line_ending().len()).unwrap_or(u64::MAX),
            )
        });
        entries.push(ContainerEntry {
            name: "noncanonical-physical-records".into(),
            role: "retained-opaque-records".into(),
            compression: "none".into(),
            compressed_size: size,
            uncompressed_size: size,
            attributes: BTreeMap::from([("records".into(), noncanonical.len().to_string())]),
        });
    }
    ContainerSummary {
        format: "iges".into(),
        container_kind: "fixed-ascii".into(),
        entries,
        notes: vec![format!("source_bytes={}", scan.source.len())],
    }
}
