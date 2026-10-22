// SPDX-License-Identifier: Apache-2.0
//! Plain text of an annotation rich-text string, as `ON_TextContent`
//! produces it: `SubstituteStackTags`, `ON_RtfParser` driving an
//! `ON_TextRunBuilder`, and `ON_TextContent::PlainTextWithFields`.

/// `ON_TextRun::RunType` values that reach the plain text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunType {
    None,
    Text,
    Newline,
    Paragraph,
    Field,
    Fontdef,
    Header,
}

#[derive(Debug, Clone)]
struct Run {
    kind: RunType,
    stacked: bool,
    codepoints: Vec<u32>,
}

impl Run {
    const EMPTY: Self = Self {
        kind: RunType::None,
        stacked: false,
        codepoints: Vec::new(),
    };
}

/// A construct whose plain text the source derives from state this reader
/// does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unflattened {
    /// A run holds a digit fraction. The dimension style's stack format then
    /// splits the run and changes the spacing.
    StyleDependentFraction,
    /// A `\'hh` byte sequence under a code page other than 1252.
    CodePage,
    /// A code point zero ends the source run string, and the source text
    /// iterator substitutes an unspecified value for a noncharacter.
    InvalidCodePoint,
}

/// The plain text of `rich`, or the construct that prevents an exact result.
pub(crate) fn plain_text(rich: &str) -> Result<String, Unflattened> {
    let substituted = substitute_stack_tags(&rich.chars().collect::<Vec<_>>());
    let mut parser = Parser {
        iterator: TextIterator::new(substituted),
        builder: Builder::new(),
        suspend_to_close: 0,
        in_real_rtf: false,
        code_page_changed: false,
    };
    parser.parse()?;
    parser.builder.failure.map_or(Ok(()), Err)?;
    plain_text_from_runs(&parser.builder.runs)
}

/// `SubstituteStackTags`: `%<...>%` becomes a field group and `[[a/b]]` a
/// stack group. The delimiter code that the Windows and Apple builds append
/// after `\stack` selects no text.
fn substitute_stack_tags(text: &[char]) -> Vec<char> {
    let at = |index: usize| text.get(index).copied().unwrap_or('\0');
    let mut output = Vec::with_capacity(text.len());
    let mut start = 0;
    let mut index = 0;
    let mut stacking = false;
    let mut in_field = false;
    let mut delimiter = '/';
    while index < text.len() && at(index) != '\0' {
        if !stacking && !in_field && at(index) == '%' && at(index + 1) == '<' {
            output.extend_from_slice(&text[start..index]);
            output.extend("{\\field %<".chars());
            index += 2;
            in_field = true;
            start = index;
        } else if in_field && at(index) == '>' && at(index + 1) == '%' {
            output.extend_from_slice(&text[start..index]);
            output.extend(">%}".chars());
            index += 2;
            start = index;
            in_field = false;
        } else if !in_field {
            if !stacking && at(index) == '[' && at(index + 1) == '[' {
                if index + 2 < text.len() && at(index + 2) == '[' {
                    index += 1;
                } else {
                    output.extend_from_slice(&text[start..index]);
                    output.extend("{\\stack".chars());
                    index += 2;
                    if matches!(at(index), '/' | '|') {
                        delimiter = at(index);
                        index += 1;
                    }
                    output.push(' ');
                    stacking = true;
                    start = index;
                }
            } else if at(index) == delimiter {
                output.extend_from_slice(&text[start..index]);
                output.push(delimiter);
                index += 1;
                start = index;
            } else if stacking && at(index) == ']' && at(index + 1) == ']' {
                output.extend_from_slice(&text[start..index]);
                output.push('}');
                index += 2;
                start = index;
                stacking = false;
            } else {
                index += 1;
            }
        } else {
            index += 1;
        }
    }
    output.extend_from_slice(&text[start..index.min(text.len())]);
    output
}

/// `ON_TextIterator`: `current` is the next code point to read, zero at the
/// end of the string.
struct TextIterator {
    text: Vec<char>,
    next_index: usize,
    current_index: usize,
    previous_index: usize,
    current: u32,
    previous: u32,
}

impl TextIterator {
    fn new(text: Vec<char>) -> Self {
        let mut iterator = Self {
            text,
            next_index: 0,
            current_index: 0,
            previous_index: 0,
            current: 0,
            previous: 0,
        };
        iterator.step();
        iterator
    }

    fn step(&mut self) {
        if let Some(&next) = self.text.get(self.next_index) {
            self.previous_index = self.current_index;
            self.current_index = self.next_index;
            self.next_index += 1;
            self.previous = self.current;
            self.current = u32::from(next);
        } else {
            self.previous = self.current;
            self.current = 0;
        }
    }

    /// The next code point, or `None` at a zero code point.
    fn read(&mut self) -> Option<u32> {
        let value = self.current;
        self.step();
        (value != 0).then_some(value)
    }

    fn back(&mut self) {
        self.next_index = self.current_index;
        self.current_index = self.previous_index;
        self.current = self.previous;
    }

    fn at_backslash_tic(&self) -> bool {
        self.current == u32::from('\\') && self.text.get(self.next_index) == Some(&'\'')
    }

    /// `ReadCharValue`: one `\'hh` byte.
    fn read_char_value(&mut self) -> Option<u8> {
        if self.read()? != u32::from('\\') || self.read()? != u32::from('\'') {
            return None;
        }
        let high = char::from_u32(self.read()?)?.to_digit(16)?;
        let low = char::from_u32(self.read()?)?.to_digit(16)?;
        u8::try_from(high * 16 + low).ok()
    }
}

/// `ON_DimStyle::stack_format::None` run building of `ON_TextRunBuilder`,
/// limited to the state that selects run text.
struct Builder {
    runs: Vec<Run>,
    current: Run,
    codepoints: Vec<u32>,
    level: i32,
    font_table_level: i32,
    reading_font_definition: bool,
    pending_high_surrogate: Option<u16>,
    failure: Option<Unflattened>,
}

const NO_FONT_TABLE_LEVEL: i32 = 10_000;
const REPLACEMENT_CHARACTER: u32 = 0xfffd;

impl Builder {
    fn new() -> Self {
        Self {
            runs: Vec::new(),
            current: Run::EMPTY,
            codepoints: Vec::new(),
            level: 0,
            font_table_level: NO_FONT_TABLE_LEVEL,
            reading_font_definition: false,
            pending_high_surrogate: None,
            failure: None,
        }
    }

    fn fail(&mut self, failure: Unflattened) {
        self.failure.get_or_insert(failure);
    }

    fn reading_font_table(&self) -> bool {
        self.level >= self.font_table_level
    }

    fn append_code_point(&mut self, code_point: u32) {
        if self.codepoints.is_empty() && !self.current.stacked {
            self.current = Run::EMPTY;
        }
        self.push_code_point(code_point);
    }

    /// The base-builder append that bypasses the run reset.
    fn push_code_point(&mut self, code_point: u32) {
        if code_point == 0 || !valid_code_point(code_point) {
            self.fail(Unflattened::InvalidCodePoint);
        }
        self.codepoints.push(code_point);
    }

    fn flush_text(&mut self) {
        if self.codepoints.first().is_some_and(|first| *first != 0) {
            self.current.codepoints = std::mem::take(&mut self.codepoints);
            self.current.kind = if self.reading_font_definition {
                RunType::Fontdef
            } else {
                RunType::Text
            };
        } else {
            self.codepoints.clear();
        }
    }

    fn finish_current_run(&mut self) {
        if matches!(
            self.current.kind,
            RunType::Text | RunType::Field | RunType::Newline | RunType::Paragraph
        ) {
            if has_fraction(&self.current.codepoints) {
                self.fail(Unflattened::StyleDependentFraction);
            }
            self.runs
                .push(std::mem::replace(&mut self.current, Run::EMPTY));
        }
    }

    fn format_change(&mut self) {
        if !self.codepoints.is_empty() {
            self.flush_text();
            self.finish_current_run();
        }
    }

    fn group_begin(&mut self) {
        self.flush_text();
        self.finish_current_run();
        self.level += 1;
        self.current = Run::EMPTY;
    }

    fn group_end(&mut self) {
        self.flush_text();
        self.finish_current_run();
        self.current = Run::EMPTY;
        if self.level <= self.font_table_level {
            self.font_table_level = NO_FONT_TABLE_LEVEL;
        }
        self.level -= 1;
    }

    fn run_begin(&mut self) {
        self.flush_text();
        self.finish_current_run();
        self.current = Run::EMPTY;
    }

    fn run_end(&mut self) {
        self.run_begin();
        if self.level <= self.font_table_level {
            self.font_table_level = NO_FONT_TABLE_LEVEL;
        }
    }

    fn finish_font_definition(&mut self) {
        if !self.reading_font_definition {
            return;
        }
        if !self.codepoints.is_empty() {
            self.flush_text();
        }
        if self.current.kind == RunType::Fontdef && self.level == self.font_table_level {
            self.current = Run::EMPTY;
        }
        self.reading_font_definition = false;
    }

    /// `ON_TextBuilder::UniDecimal`.
    fn unicode_decimal(&mut self, value: &str) {
        let Some(number) = leading_integer(value) else {
            return;
        };
        let unit = if let Ok(signed) = i16::try_from(number) {
            u16::from_ne_bytes(signed.to_ne_bytes())
        } else if let Ok(unsigned) = u16::try_from(number) {
            unsigned
        } else {
            if self.pending_high_surrogate.is_none() {
                if let Ok(code_point) = u32::try_from(number) {
                    if code_point > 0xffff && valid_code_point(code_point) {
                        self.append_code_point(code_point);
                    }
                }
            }
            return;
        };
        let code_point = match (unit, self.pending_high_surrogate) {
            (0xd800..=0xdbff, Some(_)) => {
                self.append_code_point(REPLACEMENT_CHARACTER);
                REPLACEMENT_CHARACTER
            }
            (0xd800..=0xdbff, None) => {
                self.pending_high_surrogate = Some(unit);
                return;
            }
            (0xdc00..=0xdfff, Some(high)) => char::decode_utf16([high, unit])
                .next()
                .and_then(Result::ok)
                .map(u32::from)
                .filter(|code_point| valid_code_point(*code_point))
                .unwrap_or(REPLACEMENT_CHARACTER),
            (0xdc00..=0xdfff, None) => REPLACEMENT_CHARACTER,
            (_, pending) => {
                if pending.is_some() {
                    self.push_code_point(REPLACEMENT_CHARACTER);
                }
                if valid_code_point(u32::from(unit)) {
                    u32::from(unit)
                } else {
                    REPLACEMENT_CHARACTER
                }
            }
        };
        self.append_code_point(code_point);
        self.pending_high_surrogate = None;
    }
}

/// `ON_IsValidUnicodeCodePoint`: no surrogate, no value above U+10FFFF, and
/// none of the last two code points of planes 0, 15, and 16.
fn valid_code_point(code_point: u32) -> bool {
    char::from_u32(code_point).is_some()
        && !matches!(
            code_point,
            0xfffe | 0xffff | 0xf_fffe | 0xf_ffff | 0x10_fffe | 0x10_ffff
        )
}

/// `ON_wString::ToNumber` on the tag value: an optional minus sign and the
/// decimal digits that follow it.
fn leading_integer(value: &str) -> Option<i64> {
    let (negative, digits) = value
        .strip_prefix('-')
        .map_or((false, value), |rest| (true, rest));
    let end = digits
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(digits.len());
    let magnitude: u64 = digits[..end].parse().ok()?;
    if negative {
        0_i64.checked_sub_unsigned(magnitude)
    } else {
        i64::try_from(magnitude).ok()
    }
}

/// `FindFrac` succeeds at least once in the run text.
fn has_fraction(codepoints: &[u32]) -> bool {
    let digit = |code_point: u32| (u32::from('0')..=u32::from('9')).contains(&code_point);
    let length = codepoints
        .iter()
        .position(|code_point| *code_point == 0)
        .unwrap_or(codepoints.len());
    codepoints[..length]
        .windows(3)
        .any(|window| window[1] == u32::from('/') && digit(window[0]) && digit(window[2]))
}

/// `ON_MSSBCP_0x80_0xFF_Unicode(1252)` for bytes 0x80 through 0x9F; bytes
/// 0xA0 through 0xFF map to the same code point.
const CP1252_0X80_0X9F: [u32; 32] = [
    0x20ac, 0x81, 0x201a, 0x0192, 0x201e, 0x2026, 0x2020, 0x2021, 0x02c6, 0x2030, 0x0160, 0x2039,
    0x0152, 0x8d, 0x017d, 0x8f, 0x90, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014,
    0x02dc, 0x2122, 0x0161, 0x203a, 0x0153, 0x9d, 0x017e, 0x0178,
];

fn cp1252(byte: u8) -> u32 {
    match byte {
        0x80..=0x9f => CP1252_0X80_0X9F[usize::from(byte - 0x80)],
        _ => u32::from(byte),
    }
}

struct Parser {
    iterator: TextIterator,
    builder: Builder,
    suspend_to_close: i32,
    in_real_rtf: bool,
    code_page_changed: bool,
}

impl Parser {
    /// `ON_RtfParser::Parse`.
    fn parse(&mut self) -> Result<(), Unflattened> {
        loop {
            if self.suspend_to_close <= 0 && self.iterator.at_backslash_tic() {
                self.parse_code_page_bytes()?;
                continue;
            }
            let Some(code_point) = self.iterator.read() else {
                break;
            };
            if self.suspend_to_close > 0 {
                match char::from_u32(code_point) {
                    Some('}') => self.suspend_to_close -= 1,
                    Some('{') => self.suspend_to_close += 1,
                    _ => {}
                }
                if self.suspend_to_close == 0 {
                    self.iterator.back();
                }
                continue;
            }
            match char::from_u32(code_point) {
                Some('\\') => self.parse_escape(),
                Some('\n' | '\u{2028}') => {
                    if !self.in_real_rtf {
                        self.builder.flush_text();
                        self.builder.group_end();
                        self.builder.current.kind = RunType::Paragraph;
                        self.builder.group_begin();
                    }
                }
                Some('\u{a0}' | '\u{202f}' | '\u{200b}') => {
                    self.builder.append_code_point(u32::from(' '));
                }
                Some('\r' | '\t') => {}
                Some('{') => self.builder.group_begin(),
                Some('}') => self.builder.group_end(),
                Some(';') if self.builder.reading_font_definition => {
                    self.builder.finish_font_definition();
                }
                _ => self.builder.append_code_point(code_point),
            }
        }
        self.builder.flush_text();
        self.builder.group_end();
        Ok(())
    }

    /// `Internal_ParseMBCSString` under the default code page 1252.
    fn parse_code_page_bytes(&mut self) -> Result<(), Unflattened> {
        if self.code_page_changed {
            return Err(Unflattened::CodePage);
        }
        let mut bytes = Vec::new();
        let mut parsed = true;
        while self.iterator.at_backslash_tic() {
            match self.iterator.read_char_value() {
                Some(byte) => bytes.push(byte),
                None => {
                    parsed = false;
                    break;
                }
            }
        }
        if bytes.first().is_some_and(|first| *first != 0) {
            if bytes.contains(&0) {
                return Err(Unflattened::InvalidCodePoint);
            }
            for byte in bytes {
                self.builder.append_code_point(cp1252(byte));
            }
        }
        if !parsed {
            self.builder.append_code_point(REPLACEMENT_CHARACTER);
        }
        Ok(())
    }

    /// The character after a backslash.
    fn parse_escape(&mut self) {
        let Some(code_point) = self.iterator.read() else {
            return;
        };
        match char::from_u32(code_point) {
            Some('\\' | '{' | '}') => self.builder.append_code_point(code_point),
            Some('\n' | '\r' | '\u{2028}' | '\u{2029}') => {
                self.builder.flush_text();
                self.builder.run_end();
                self.builder.current.kind = RunType::Paragraph;
                self.builder.run_begin();
            }
            Some('~') => self.builder.append_code_point(u32::from(' ')),
            Some('_') => self.builder.append_code_point(u32::from('-')),
            Some('-' | '|' | ':' | '\'') => {}
            Some('*') => self.read_optional_tag(),
            _ => {
                self.iterator.back();
                self.read_tag();
            }
        }
    }

    /// `ReadOptionalTag`, after `\*`.
    fn read_optional_tag(&mut self) {
        while let Some(code_point) = self.iterator.read() {
            if code_point == u32::from('\\') {
                if !self.read_tag() && self.suspend_to_close == 0 {
                    self.suspend_to_close = 1;
                }
            } else {
                if code_point == u32::from('{') || code_point == u32::from('}') {
                    self.iterator.back();
                }
                break;
            }
        }
    }

    /// `ReadTag`: a control word with an optional numeric value. Returns
    /// whether the tag is known.
    fn read_tag(&mut self) -> bool {
        const MAXIMUM: usize = 63;
        let mut name = String::new();
        let mut value = String::new();
        let mut in_name = true;
        while let Some(code_point) = self.iterator.read() {
            let character = char::from_u32(code_point).unwrap_or('\u{fffd}');
            if in_name && character.is_ascii_alphabetic() && name.len() < MAXIMUM {
                name.push(character);
            } else if (character.is_ascii_digit()
                || character == '.'
                || (character == '-' && value.is_empty()))
                && value.len() < MAXIMUM
            {
                in_name = false;
                value.push(character);
            } else {
                if !name.eq_ignore_ascii_case("u") {
                    self.builder.format_change();
                }
                let known = self.process_tag(&name, &value);
                if matches!(character, '\\' | '{' | '}') {
                    self.iterator.back();
                }
                return known;
            }
        }
        false
    }

    /// `ProcessTag`, limited to the tags that select run text.
    fn process_tag(&mut self, name: &str, value: &str) -> bool {
        let tag = name.to_ascii_lowercase();
        let has_value = !value.is_empty();
        match tag.as_str() {
            "rtf" => {
                self.in_real_rtf = true;
                self.builder.current.kind = RunType::Header;
            }
            "fonttbl" => self.builder.font_table_level = self.builder.level,
            "f" if has_value => {
                if leading_integer(value).is_some_and(|index| index >= 0)
                    && self.builder.reading_font_table()
                {
                    self.builder.reading_font_definition = true;
                }
            }
            "fcharset" if has_value => {
                if leading_integer(value) != Some(0) {
                    self.code_page_changed = true;
                }
            }
            "cpg" if has_value => {
                if leading_integer(value) != Some(1252) {
                    self.code_page_changed = true;
                }
            }
            "deff" | "fs" | "red" | "green" | "blue" | "cf" | "cb" if has_value => {}
            "deff" | "f" | "fs" | "fcharset" | "cpg" | "red" | "green" | "blue" | "cf" | "cb" => {
                return false
            }
            "line" => self.builder.current.kind = RunType::Newline,
            "par" => self.builder.current.kind = RunType::Paragraph,
            "tab" => {
                for _ in 0..8 {
                    self.builder.append_code_point(u32::from(' '));
                }
            }
            "stack" => {
                self.builder.current.kind = RunType::Text;
                self.builder.current.stacked = true;
            }
            "stnone" => self.builder.current.stacked = false,
            "field" => self.builder.current.kind = RunType::Field,
            "u" => self.builder.unicode_decimal(value),
            "lquote" => self.builder.push_code_point(0x2018),
            "rquote" => self.builder.push_code_point(0x2019),
            "ldblquote" => self.builder.push_code_point(0x201c),
            "rdblquote" => self.builder.push_code_point(0x201d),
            "bullet" => self.builder.push_code_point(0x2022),
            "endash" => self.builder.push_code_point(0x2013),
            "emdash" => self.builder.push_code_point(0x2014),
            "pard" | "sect" | "b" | "i" | "ul" | "ulnone" | "strike" | "super" | "sub"
            | "nosupersub" | "colortbl" | "stackheight" | "uc" | "upr" | "ud" => {}
            _ => return false,
        }
        true
    }
}

/// `ON_TextContent::Internal_GetPlainText` without field evaluation or
/// wrapping.
fn plain_text_from_runs(runs: &[Run]) -> Result<String, Unflattened> {
    let mut line_count = 0;
    let mut multiline = false;
    for run in runs {
        if run.kind == RunType::Text && line_count > 0 {
            multiline = true;
        } else if matches!(run.kind, RunType::Newline | RunType::Paragraph) {
            line_count += 1;
        }
    }
    let mut text = String::new();
    for (index, run) in runs.iter().enumerate() {
        match run.kind {
            RunType::Text | RunType::Field => {
                let mut run_text = String::new();
                for code_point in &run.codepoints {
                    if *code_point == 0 {
                        break;
                    }
                    run_text
                        .push(char::from_u32(*code_point).ok_or(Unflattened::InvalidCodePoint)?);
                }
                if index > 0 && run_text.contains('/') {
                    text.push(' ');
                }
                text.push_str(&run_text);
            }
            RunType::Newline | RunType::Paragraph if multiline => text.push_str("\r\n"),
            _ => {}
        }
    }
    Ok(text)
}

#[cfg(test)]
mod tests;
