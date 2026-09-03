// SPDX-License-Identifier: Apache-2.0
//! IGES Compressed ASCII expansion into the fixed-card parser's input model.
//!
//! The Compressed ASCII representation shares the Start, Global, and Terminate
//! records with Fixed ASCII. Its Data records carry inherited Directory Entry
//! fields followed by variable-length Parameter Data lines. Expansion derives
//! the four redundant Directory Entry fields and the fixed-card sequence
//! fields, then delegates all semantic work to the existing parser.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

const CARD_WIDTH: usize = 80;
const CARD_DATA_WIDTH: usize = 72;
const PARAMETER_DATA_WIDTH: usize = 64;
const MAX_SEQUENCE: u32 = 9_999_999;
const OMITTED_FIELDS: [usize; 4] = [2, 10, 11, 20];

#[derive(Debug, Clone)]
struct DataEntity {
    sequence: u32,
    fields: [Option<Vec<u8>>; 21],
    parameter_lines: Vec<Vec<u8>>,
}

#[derive(Debug)]
struct ParsedDirectoryRecord {
    sequence: u32,
    specs: Vec<(usize, Vec<u8>)>,
    next: usize,
}

#[derive(Debug, Clone, Copy)]
struct ParameterLexState {
    at_field_start: bool,
    hollerith_remaining: usize,
    count_start: Option<usize>,
}

impl Default for ParameterLexState {
    fn default() -> Self {
        Self {
            at_field_start: true,
            hollerith_remaining: 0,
            count_start: None,
        }
    }
}

fn malformed(message: impl Into<String>) -> CodecError {
    crate::error::malformed(format!("IGES Compressed ASCII: {}", message.into()))
}

fn split_lines<'a>(source: &'a [u8], ctx: &DecodeContext<'_>) -> Result<Vec<&'a [u8]>, CodecError> {
    if source.is_empty() {
        return Err(malformed("source is empty"));
    }
    let mut lines = Vec::new();
    let mut start = 0_usize;
    while start < source.len() {
        let relative_end = memchr::memchr2(b'\r', b'\n', &source[start..]);
        let (end, next) = match relative_end {
            Some(relative) => {
                let end = start
                    .checked_add(relative)
                    .ok_or_else(|| malformed("line offset overflows"))?;
                let ending =
                    usize::from(source[end] == b'\r' && source.get(end + 1) == Some(&b'\n'));
                (end, end + ending + 1)
            }
            None => (source.len(), source.len()),
        };
        ctx.charge_collection_items(1, "iges_compressed_ascii_lines")?;
        lines.push(&source[start..end]);
        start = next;
    }
    Ok(lines)
}

fn logical_global_stream(cards: &[&[u8]]) -> Result<Vec<u8>, CodecError> {
    let mut stream = Vec::new();
    let mut pending_digits = Vec::new();
    let mut hollerith_remaining = 0_usize;
    for card in cards {
        if card.len() != CARD_WIDTH {
            return Err(malformed("Start and Global records must be 80 columns"));
        }
        for byte in card[..CARD_DATA_WIDTH].iter().copied() {
            if hollerith_remaining > 0 {
                stream.push(byte);
                hollerith_remaining -= 1;
                continue;
            }
            if byte == b' ' {
                continue;
            }
            if byte.is_ascii_digit() {
                pending_digits.push(byte);
                continue;
            }
            if matches!(byte, b'H' | b'h') && !pending_digits.is_empty() {
                let count = std::str::from_utf8(&pending_digits)
                    .map_err(|_| malformed("Global Hollerith count is not ASCII"))?
                    .parse::<usize>()
                    .map_err(|_| malformed("Global Hollerith count is out of range"))?;
                stream.extend_from_slice(&pending_digits);
                stream.push(byte);
                pending_digits.clear();
                hollerith_remaining = count;
                continue;
            }
            stream.append(&mut pending_digits);
            stream.push(byte);
        }
    }
    stream.append(&mut pending_digits);
    if hollerith_remaining != 0 {
        return Err(malformed("Global Hollerith payload is truncated"));
    }
    Ok(stream)
}

fn hollerith_at(bytes: &[u8], start: usize) -> Result<Option<(usize, usize)>, CodecError> {
    let mut cursor = start;
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    if cursor == start || !matches!(bytes.get(cursor), Some(b'H' | b'h')) {
        return Ok(None);
    }
    let count = std::str::from_utf8(&bytes[start..cursor])
        .map_err(|_| malformed("Global Hollerith count is not ASCII"))?
        .parse::<usize>()
        .map_err(|_| malformed("Global Hollerith count is out of range"))?;
    let payload_start = cursor
        .checked_add(1)
        .ok_or_else(|| malformed("Global Hollerith payload offset overflows"))?;
    let payload_end = payload_start
        .checked_add(count)
        .ok_or_else(|| malformed("Global Hollerith payload length overflows"))?;
    bytes
        .get(payload_start..payload_end)
        .ok_or_else(|| malformed("Global Hollerith payload is truncated"))?;
    Ok(Some((cursor + 1, payload_end)))
}

fn compressed_delimiters(cards: &[&[u8]]) -> Result<(u8, u8), CodecError> {
    let bytes = logical_global_stream(cards)?;
    let (parameter_delimiter, cursor) = if bytes.first() == Some(&b',') {
        (b',', 1)
    } else {
        let Some((header_end, payload_end)) = hollerith_at(&bytes, 0)? else {
            return Err(malformed("parameter delimiter is not a Hollerith string"));
        };
        let payload = &bytes[header_end..payload_end];
        if payload.len() != 1 || bytes.get(payload_end) != payload.first() {
            return Err(malformed("parameter delimiter is not a one-byte field"));
        }
        (payload[0], payload_end + 1)
    };
    let record_delimiter = if bytes.get(cursor) == Some(&parameter_delimiter) {
        b';'
    } else {
        let Some((header_end, payload_end)) = hollerith_at(&bytes, cursor)? else {
            return Err(malformed("record delimiter is not a Hollerith string"));
        };
        let payload = &bytes[header_end..payload_end];
        if payload.len() != 1 || bytes.get(payload_end) != Some(&parameter_delimiter) {
            return Err(malformed("record delimiter is not a one-byte field"));
        }
        payload[0]
    };
    if record_delimiter == b'@' {
        return Err(malformed(
            "record delimiter conflicts with the Directory field-specifier marker",
        ));
    }
    if parameter_delimiter == record_delimiter {
        return Err(malformed("parameter and record delimiters are equal"));
    }
    Ok((parameter_delimiter, record_delimiter))
}

fn parse_sequence(bytes: &[u8], start: usize, label: &str) -> Result<(u32, usize), CodecError> {
    let mut end = start;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    if end == start {
        return Err(malformed(format!(
            "{label} has no unsigned sequence number"
        )));
    }
    let value = std::str::from_utf8(&bytes[start..end])
        .map_err(|_| malformed(format!("{label} sequence is not ASCII")))?
        .parse::<u32>()
        .map_err(|_| malformed(format!("{label} sequence is out of range")))?;
    if value == 0 || value > MAX_SEQUENCE {
        return Err(malformed(format!(
            "{label} sequence is outside the seven-digit range"
        )));
    }
    Ok((value, end))
}

fn parse_field_specs(bytes: &[u8]) -> Result<Vec<(usize, Vec<u8>)>, CodecError> {
    let mut specs = Vec::new();
    let mut specified = [false; 21];
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        if bytes[cursor] != b'@' {
            return Err(malformed(
                "Directory field continuation does not begin with @",
            ));
        }
        cursor += 1;
        let field_start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        if field_start == cursor || bytes.get(cursor) != Some(&b'_') {
            return Err(malformed("Directory field specifier lacks an underscore"));
        }
        let field = std::str::from_utf8(&bytes[field_start..cursor])
            .map_err(|_| malformed("Directory field number is not ASCII"))?
            .parse::<usize>()
            .map_err(|_| malformed("Directory field number is out of range"))?;
        if !(1..=20).contains(&field) {
            return Err(malformed(format!(
                "Directory field number {field} is outside 1 through 20"
            )));
        }
        if OMITTED_FIELDS.contains(&field) {
            return Err(malformed(format!(
                "Directory field {field} is redundant in Compressed ASCII"
            )));
        }
        if specified[field] {
            return Err(malformed(format!(
                "Directory field {field} is specified more than once"
            )));
        }
        specified[field] = true;
        cursor += 1;
        let value_start = cursor;
        while bytes.get(cursor).is_some_and(|byte| *byte != b'@') {
            cursor += 1;
        }
        let value = bytes[value_start..cursor].to_vec();
        if value
            .iter()
            .any(|byte| !byte.is_ascii() || byte.is_ascii_control())
        {
            return Err(malformed(format!(
                "Directory field {field} contains a non-printable byte"
            )));
        }
        specs.push((field, value));
    }
    Ok(specs)
}

fn parse_directory_record(
    lines: &[&[u8]],
    start: usize,
    record_delimiter: u8,
) -> Result<ParsedDirectoryRecord, CodecError> {
    let first = lines
        .get(start)
        .ok_or_else(|| malformed("Data section ends before a Directory record"))?;
    if first.is_empty() || first[0] != b'D' {
        return Err(malformed("Data record does not begin with D"));
    }
    if first.len() > CARD_DATA_WIDTH {
        return Err(malformed("Directory field line exceeds 72 columns"));
    }
    let (sequence, cursor) = parse_sequence(first, 1, "Data record")?;
    let mut spec_bytes = first[cursor..].to_vec();
    let mut line_index = start;
    loop {
        if let Some(delimiter) = spec_bytes.iter().position(|byte| *byte == record_delimiter) {
            if spec_bytes[delimiter + 1..].iter().any(|byte| *byte != b' ') {
                return Err(malformed(
                    "Directory field record has data after its record delimiter",
                ));
            }
            spec_bytes.truncate(delimiter);
            return Ok(ParsedDirectoryRecord {
                sequence,
                specs: parse_field_specs(&spec_bytes)?,
                next: line_index + 1,
            });
        }
        line_index += 1;
        let continuation = lines
            .get(line_index)
            .ok_or_else(|| malformed("Directory field record has no record delimiter"))?;
        if continuation.len() > CARD_DATA_WIDTH {
            return Err(malformed("Directory field line exceeds 72 columns"));
        }
        if continuation.first() != Some(&b'@') {
            return Err(malformed(
                "Directory field continuation does not begin with @",
            ));
        }
        spec_bytes.extend_from_slice(continuation);
    }
}

fn apply_field_specs(
    previous: &[Option<Vec<u8>>; 21],
    specs: Vec<(usize, Vec<u8>)>,
    first: bool,
) -> Result<[Option<Vec<u8>>; 21], CodecError> {
    let mut fields = previous.clone();
    for (field, value) in specs {
        fields[field] = Some(value);
    }
    if first
        && (1..=20)
            .filter(|field| !OMITTED_FIELDS.contains(field))
            .any(|field| fields[field].is_none())
    {
        return Err(malformed(
            "the first Data record does not specify every non-redundant Directory field",
        ));
    }
    Ok(fields)
}

fn field_i64(fields: &[Option<Vec<u8>>; 21], field: usize, name: &str) -> Result<i64, CodecError> {
    let bytes = fields
        .get(field)
        .and_then(Option::as_deref)
        .ok_or_else(|| malformed(format!("Directory field {field} ({name}) is absent")))?;
    let text = std::str::from_utf8(bytes)
        .map_err(|_| malformed(format!("Directory field {field} ({name}) is not ASCII")))?
        .trim();
    if text.is_empty() {
        return Err(malformed(format!(
            "Directory field {field} ({name}) is blank"
        )));
    }
    text.parse::<i64>().map_err(|_| {
        malformed(format!(
            "Directory field {field} ({name}) is not a decimal integer"
        ))
    })
}

fn field_bytes(fields: &[Option<Vec<u8>>; 21], field: usize) -> Result<&[u8], CodecError> {
    fields
        .get(field)
        .and_then(Option::as_deref)
        .ok_or_else(|| malformed(format!("Directory field {field} is absent")))
}

fn fixed_field(field: usize, bytes: &[u8]) -> Result<[u8; 8], CodecError> {
    if bytes.len() > 8 {
        return Err(malformed(format!(
            "Directory field {field} exceeds eight columns"
        )));
    }
    let mut output = [b' '; 8];
    if matches!(field, 16 | 17) {
        output[..bytes.len()].copy_from_slice(bytes);
    } else {
        output[8 - bytes.len()..].copy_from_slice(bytes);
    }
    Ok(output)
}

fn fixed_number(value: i64) -> Result<[u8; 8], CodecError> {
    fixed_field(0, value.to_string().as_bytes())
}

fn sequence_field(marker: u8, sequence: u32) -> Result<[u8; 8], CodecError> {
    let mut output = [b' '; 8];
    output[0] = marker;
    let digits = sequence.to_string();
    if digits.len() > 7 {
        return Err(malformed("section sequence exceeds seven digits"));
    }
    output[8 - digits.len()..].copy_from_slice(digits.as_bytes());
    Ok(output)
}

fn append_card(
    output: &mut Vec<u8>,
    data: &[u8],
    marker: u8,
    sequence: u32,
) -> Result<(), CodecError> {
    if data.len() != CARD_DATA_WIDTH {
        return Err(malformed("generated card data does not occupy 72 columns"));
    }
    output.extend_from_slice(data);
    output.extend_from_slice(&sequence_field(marker, sequence)?);
    output.push(b'\n');
    Ok(())
}

fn append_source_card(output: &mut Vec<u8>, line: &[u8], section: u8) -> Result<(), CodecError> {
    if line.len() != CARD_WIDTH || line.get(72) != Some(&section) {
        return Err(malformed(
            "Start or Global record is not a fixed 80-column card",
        ));
    }
    output.extend_from_slice(line);
    output.push(b'\n');
    Ok(())
}

fn append_directory_cards(
    output: &mut Vec<u8>,
    entity: &DataEntity,
    parameter_start: u32,
) -> Result<(), CodecError> {
    let entity_type = field_bytes(&entity.fields, 1)?;
    let first_fields = [
        fixed_field(1, entity_type)?,
        fixed_number(i64::from(parameter_start))?,
        fixed_field(3, field_bytes(&entity.fields, 3)?)?,
        fixed_field(4, field_bytes(&entity.fields, 4)?)?,
        fixed_field(5, field_bytes(&entity.fields, 5)?)?,
        fixed_field(6, field_bytes(&entity.fields, 6)?)?,
        fixed_field(7, field_bytes(&entity.fields, 7)?)?,
        fixed_field(8, field_bytes(&entity.fields, 8)?)?,
        fixed_field(9, field_bytes(&entity.fields, 9)?)?,
    ];
    let second_fields = [
        fixed_field(11, entity_type)?,
        fixed_field(12, field_bytes(&entity.fields, 12)?)?,
        fixed_field(13, field_bytes(&entity.fields, 13)?)?,
        fixed_field(14, field_bytes(&entity.fields, 14)?)?,
        fixed_field(15, field_bytes(&entity.fields, 15)?)?,
        fixed_field(16, field_bytes(&entity.fields, 16)?)?,
        fixed_field(17, field_bytes(&entity.fields, 17)?)?,
        fixed_field(18, field_bytes(&entity.fields, 18)?)?,
        fixed_field(19, field_bytes(&entity.fields, 19)?)?,
    ];
    let mut first = [b' '; CARD_DATA_WIDTH];
    let mut second = [b' '; CARD_DATA_WIDTH];
    for (index, field) in first_fields.into_iter().enumerate() {
        first[index * 8..(index + 1) * 8].copy_from_slice(&field);
    }
    for (index, field) in second_fields.into_iter().enumerate() {
        second[index * 8..(index + 1) * 8].copy_from_slice(&field);
    }
    append_card(output, &first, b'D', entity.sequence)?;
    let even_sequence = entity
        .sequence
        .checked_add(1)
        .ok_or_else(|| malformed("Directory sequence overflows"))?;
    append_card(output, &second, b'D', even_sequence)
}

fn parameter_record_terminator(
    line: &[u8],
    state: &mut ParameterLexState,
    parameter_delimiter: u8,
    record_delimiter: u8,
) -> bool {
    for byte in line.iter().copied() {
        if state.hollerith_remaining > 0 {
            state.hollerith_remaining -= 1;
            continue;
        }
        if state.at_field_start {
            if byte == b' ' {
                continue;
            }
            if byte.is_ascii_digit() {
                state.count_start = Some(
                    state
                        .count_start
                        .unwrap_or_default()
                        .saturating_mul(10)
                        .saturating_add(usize::from(byte - b'0')),
                );
                continue;
            }
            if matches!(byte, b'H' | b'h') {
                let Some(count) = state.count_start.take() else {
                    state.at_field_start = false;
                    continue;
                };
                state.hollerith_remaining = count;
                state.at_field_start = false;
                continue;
            }
            state.count_start = None;
            state.at_field_start = false;
        }
        if byte == parameter_delimiter {
            state.at_field_start = true;
            state.count_start = None;
        } else if byte == record_delimiter {
            return true;
        }
    }
    false
}

fn parse_data_entity(
    lines: &[&[u8]],
    start: usize,
    previous: &[Option<Vec<u8>>; 21],
    first: bool,
    parameter_delimiter: u8,
    record_delimiter: u8,
) -> Result<(DataEntity, usize), CodecError> {
    let directory = parse_directory_record(lines, start, record_delimiter)?;
    let fields = apply_field_specs(previous, directory.specs, first)?;
    let sequence = directory.sequence;
    let mut cursor = directory.next;
    let entity_type = field_i64(&fields, 1, "entity type")?;
    let line_count = field_i64(&fields, 14, "Parameter Data line count")?;
    let line_count = usize::try_from(line_count)
        .map_err(|_| malformed("Parameter Data line count is negative or out of range"))?;
    let mut parameter_lines = Vec::new();
    if line_count == 0 {
        if entity_type != 0 {
            return Err(malformed("a non-null entity has zero Parameter Data lines"));
        }
    } else {
        let end = cursor
            .checked_add(line_count)
            .ok_or_else(|| malformed("Parameter Data line range overflows"))?;
        let source_lines = lines
            .get(cursor..end)
            .ok_or_else(|| malformed("Parameter Data lines end before the declared count"))?;
        let mut state = ParameterLexState::default();
        let mut terminated = false;
        for line in source_lines {
            if line.len() > PARAMETER_DATA_WIDTH {
                return Err(malformed("Parameter Data line exceeds 64 columns"));
            }
            if !terminated
                && parameter_record_terminator(
                    line,
                    &mut state,
                    parameter_delimiter,
                    record_delimiter,
                )
            {
                terminated = true;
            }
            parameter_lines.push((*line).to_vec());
        }
        if !terminated {
            return Err(malformed(
                "Parameter Data record has no record delimiter in its declared lines",
            ));
        }
        cursor = end;
    }
    Ok((
        DataEntity {
            sequence,
            fields,
            parameter_lines,
        },
        cursor,
    ))
}

fn append_parameter_cards(
    output: &mut Vec<u8>,
    entity: &DataEntity,
    first_parameter_sequence: u32,
) -> Result<(), CodecError> {
    for (index, line) in entity.parameter_lines.iter().enumerate() {
        let sequence = first_parameter_sequence
            .checked_add(
                u32::try_from(index)
                    .map_err(|_| malformed("Parameter Data sequence index overflows"))?,
            )
            .ok_or_else(|| malformed("Parameter Data sequence overflows"))?;
        let mut data = [b' '; CARD_DATA_WIDTH];
        data[..line.len()].copy_from_slice(line);
        let owner = fixed_number(i64::from(entity.sequence))?;
        data[64..72].copy_from_slice(&owner);
        append_card(output, &data, b'P', sequence)?;
    }
    Ok(())
}

fn append_terminate(
    output: &mut Vec<u8>,
    start_count: usize,
    global_count: usize,
    directory_count: usize,
    parameter_count: usize,
) -> Result<(), CodecError> {
    let counts = [
        (b'S', start_count),
        (b'G', global_count),
        (b'D', directory_count),
        (b'P', parameter_count),
    ];
    let mut data = [b' '; CARD_DATA_WIDTH];
    for (index, (marker, count)) in counts.into_iter().enumerate() {
        let count = u32::try_from(count).map_err(|_| malformed("section count is out of range"))?;
        let field = sequence_field(marker, count)?;
        data[index * 8..(index + 1) * 8].copy_from_slice(&field);
    }
    append_card(output, &data, b'T', 1)
}

fn charge_normalization(ctx: &DecodeContext<'_>, bytes: usize) -> Result<(), CodecError> {
    ctx.charge_work(
        u64::try_from(bytes).unwrap_or(u64::MAX),
        "iges_compressed_ascii_normalization",
    )
}

/// Expand one Compressed ASCII source into the fixed-card input consumed by
/// the IGES section, Directory, and Parameter parsers.
pub(crate) fn normalize(source: &[u8], ctx: &DecodeContext<'_>) -> Result<Vec<u8>, CodecError> {
    let lines = split_lines(source, ctx)?;
    let flag = lines
        .first()
        .copied()
        .ok_or_else(|| malformed("flag record is missing"))?;
    if flag.len() != CARD_WIDTH || flag.get(72) != Some(&b'C') {
        return Err(malformed(
            "Compressed ASCII flag is not an 80-column C record",
        ));
    }

    let mut cursor = 1_usize;
    let start_begin = cursor;
    while lines
        .get(cursor)
        .is_some_and(|line| line.len() == CARD_WIDTH && line.get(72) == Some(&b'S'))
    {
        cursor += 1;
    }
    if cursor == start_begin {
        return Err(malformed("Start section is missing after the flag record"));
    }
    let global_begin = cursor;
    while lines
        .get(cursor)
        .is_some_and(|line| line.len() == CARD_WIDTH && line.get(72) == Some(&b'G'))
    {
        cursor += 1;
    }
    if cursor == global_begin {
        return Err(malformed("Global section is missing"));
    }
    let data_begin = cursor;
    let terminate_index = lines
        .get(data_begin..)
        .and_then(|tail| {
            tail.iter()
                .position(|line| line.len() == CARD_WIDTH && line.get(72) == Some(&b'T'))
        })
        .map(|index| data_begin + index)
        .ok_or_else(|| malformed("Terminate section is missing"))?;

    let global_cards = lines[global_begin..data_begin].to_vec();
    let (parameter_delimiter, record_delimiter) = compressed_delimiters(&global_cards)?;
    let mut previous = std::array::from_fn(|_| None);
    let mut entities = Vec::new();
    let mut data_cursor = data_begin;
    let mut expected_sequence = 1_u32;
    while data_cursor < terminate_index {
        let (entity, next) = parse_data_entity(
            &lines,
            data_cursor,
            &previous,
            entities.is_empty(),
            parameter_delimiter,
            record_delimiter,
        )?;
        if entity.sequence != expected_sequence || entity.sequence % 2 == 0 {
            return Err(malformed(format!(
                "Data record sequence {} is not the next odd Directory sequence {}",
                entity.sequence, expected_sequence
            )));
        }
        expected_sequence = expected_sequence
            .checked_add(2)
            .ok_or_else(|| malformed("Directory sequence overflows"))?;
        previous.clone_from(&entity.fields);
        entities.push(entity);
        data_cursor = next;
    }

    let parameter_count = entities
        .iter()
        .map(|entity| entity.parameter_lines.len())
        .sum::<usize>();
    let directory_count = entities
        .len()
        .checked_mul(2)
        .ok_or_else(|| malformed("Directory section count overflows"))?;
    let output_estimate = start_begin
        .checked_add(global_cards.len())
        .and_then(|count| count.checked_add(directory_count))
        .and_then(|count| count.checked_add(parameter_count))
        .and_then(|count| count.checked_add(1))
        .and_then(|count| count.checked_mul(CARD_WIDTH + 1))
        .and_then(|size| size.checked_add(source.len()))
        .ok_or_else(|| malformed("normalized source size overflows"))?;
    charge_normalization(ctx, source.len().saturating_add(output_estimate))?;

    let mut output = Vec::with_capacity(output_estimate);
    for line in &lines[start_begin..global_begin] {
        append_source_card(&mut output, line, b'S')?;
    }
    for line in &lines[global_begin..data_begin] {
        append_source_card(&mut output, line, b'G')?;
    }
    let mut parameter_starts = Vec::with_capacity(entities.len());
    let mut parameter_sequence = 1_u32;
    for entity in &entities {
        let parameter_start = if entity.parameter_lines.is_empty() {
            0
        } else {
            parameter_sequence
        };
        parameter_starts.push(parameter_start);
        parameter_sequence = parameter_sequence
            .checked_add(
                u32::try_from(entity.parameter_lines.len())
                    .map_err(|_| malformed("Parameter Data section count overflows"))?,
            )
            .ok_or_else(|| malformed("Parameter Data sequence overflows"))?;
    }
    for (entity, parameter_start) in entities.iter().zip(parameter_starts) {
        append_directory_cards(&mut output, entity, parameter_start)?;
    }
    let mut parameter_sequence = 1_u32;
    for entity in &entities {
        append_parameter_cards(&mut output, entity, parameter_sequence)?;
        parameter_sequence = parameter_sequence
            .checked_add(
                u32::try_from(entity.parameter_lines.len())
                    .map_err(|_| malformed("Parameter Data section count overflows"))?,
            )
            .ok_or_else(|| malformed("Parameter Data sequence overflows"))?;
    }
    append_terminate(
        &mut output,
        start_begin,
        global_cards.len(),
        directory_count,
        parameter_count,
    )?;
    for line in lines.get(terminate_index + 1..).unwrap_or_default() {
        output.extend_from_slice(line);
        output.push(b'\n');
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
