// SPDX-License-Identifier: Apache-2.0
//! Byte-pattern search and printable-string extraction.

/// One byte of a search pattern: a fixed value or a `??` wildcard.
pub type PatternByte = Option<u8>;

/// Parses a hexadecimal search pattern.
///
/// Digits are read in pairs. `??` matches any byte. Whitespace between pairs is
/// ignored, so `4d 5a ?? 00` and `4d5a??00` are the same pattern.
///
/// # Errors
///
/// Returns a message when the pattern is empty, holds a character that is
/// neither a hexadecimal digit nor `?`, mixes a digit with `?` inside one pair,
/// or ends on a half byte.
pub fn parse_pattern(text: &str) -> Result<Vec<PatternByte>, String> {
    let chars: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    if chars.is_empty() {
        return Err(
            "empty pattern; expected hexadecimal byte pairs such as `4d5a??00`".to_string(),
        );
    }
    if !chars.len().is_multiple_of(2) {
        return Err(format!(
            "pattern `{text}` has {} hexadecimal digits; byte patterns need an even count",
            chars.len()
        ));
    }
    let mut pattern = Vec::with_capacity(chars.len() / 2);
    for pair in chars.chunks(2) {
        let (high, low) = (pair[0], pair[1]);
        match (high, low) {
            ('?', '?') => pattern.push(None),
            ('?', _) | (_, '?') => {
                return Err(format!(
                    "pattern `{text}`: `{high}{low}` mixes a wildcard with a digit; \
                     a wildcard covers a whole byte and is written `??`"
                ))
            }
            _ => {
                let high = hex_digit(high, text)?;
                let low = hex_digit(low, text)?;
                pattern.push(Some(high << 4 | low));
            }
        }
    }
    Ok(pattern)
}

fn hex_digit(c: char, text: &str) -> Result<u8, String> {
    c.to_digit(16)
        .map(|value| value as u8)
        .ok_or_else(|| format!("pattern `{text}`: `{c}` is not a hexadecimal digit or `?`"))
}

/// Encodes an ASCII search term, rejecting anything outside 7-bit ASCII.
///
/// # Errors
///
/// Returns a message when the term is empty or holds a non-ASCII character.
pub fn ascii_pattern(text: &str) -> Result<Vec<PatternByte>, String> {
    if text.is_empty() {
        return Err("empty ASCII search term".to_string());
    }
    if !text.is_ascii() {
        return Err(format!(
            "`{text}` is not ASCII; use --utf16le or --hex for other encodings"
        ));
    }
    Ok(text.bytes().map(Some).collect())
}

/// Encodes a search term as UTF-16LE code units.
///
/// # Errors
///
/// Returns a message when the term is empty.
pub fn utf16le_pattern(text: &str) -> Result<Vec<PatternByte>, String> {
    if text.is_empty() {
        return Err("empty UTF-16LE search term".to_string());
    }
    Ok(text
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .map(Some)
        .collect())
}

/// Returns every offset in `haystack` where `pattern` matches, in order.
///
/// `limit` caps the number of reported offsets; `None` reports all of them. The
/// match is byte exact except at wildcard positions.
pub fn find_all(haystack: &[u8], pattern: &[PatternByte], limit: Option<usize>) -> Vec<u64> {
    let mut hits = Vec::new();
    if pattern.is_empty() || haystack.len() < pattern.len() {
        return hits;
    }
    // Anchoring on a fixed byte lets `memchr` skip most of the file when the
    // pattern does not start with a wildcard.
    let anchor = pattern.iter().position(Option::is_some);
    let last_start = haystack.len() - pattern.len();
    let mut start = 0usize;
    while start <= last_start {
        let candidate = match anchor {
            Some(index) => {
                let byte = pattern[index].expect("anchor position holds a fixed byte");
                match memchr::memchr(byte, &haystack[start + index..]) {
                    Some(found) => start + found,
                    None => break,
                }
            }
            None => start,
        };
        if candidate > last_start {
            break;
        }
        if matches_at(haystack, candidate, pattern) {
            hits.push(candidate as u64);
            if limit.is_some_and(|max| hits.len() >= max) {
                break;
            }
        }
        start = candidate + 1;
    }
    hits
}

fn matches_at(haystack: &[u8], at: usize, pattern: &[PatternByte]) -> bool {
    pattern
        .iter()
        .zip(&haystack[at..at + pattern.len()])
        .all(|(expected, actual)| expected.is_none_or(|byte| byte == *actual))
}

/// Concrete character encoding of one extracted string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringEncoding {
    /// A run of printable single-byte ASCII.
    Ascii,
    /// A run of printable ASCII widened to UTF-16LE code units.
    Utf16le,
}

impl StringEncoding {
    /// Returns the label printed next to each extracted string.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ascii => "ascii",
            Self::Utf16le => "utf16le",
        }
    }
}

/// Character encodings selected for one string-extraction scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum StringScan {
    /// Runs of printable single-byte ASCII.
    Ascii,
    /// Runs of printable ASCII widened to UTF-16LE code units.
    Utf16le,
    /// Both of the above, merged and sorted by offset.
    Both,
}

/// One printable run found in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundString {
    /// Offset of the first byte of the run.
    pub offset: u64,
    /// Encoding the run was read in.
    pub encoding: StringEncoding,
    /// The decoded text.
    pub text: String,
}

/// Extracts printable runs of at least `min_len` characters.
///
/// Runs are maximal: a longer run is never reported as several shorter ones. For
/// [`StringScan::Both`] the ASCII and UTF-16LE passes run independently and
/// the results are sorted by offset, so a UTF-16LE run is also visible as the
/// ASCII characters interleaved with its zero bytes only when those characters
/// themselves form a long enough ASCII run.
pub fn extract_strings(bytes: &[u8], min_len: usize, encoding: StringScan) -> Vec<FoundString> {
    let min_len = min_len.max(1);
    let mut found = match encoding {
        StringScan::Ascii => ascii_runs(bytes, min_len),
        StringScan::Utf16le => utf16le_runs(bytes, min_len),
        StringScan::Both => {
            let mut all = ascii_runs(bytes, min_len);
            all.extend(utf16le_runs(bytes, min_len));
            all
        }
    };
    found.sort_by_key(|item| (item.offset, item.text.len()));
    found
}

const fn is_printable(byte: u8) -> bool {
    byte.is_ascii_graphic() || byte == b' ' || byte == b'\t'
}

fn ascii_runs(bytes: &[u8], min_len: usize) -> Vec<FoundString> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut run = String::new();
    for (index, byte) in bytes.iter().enumerate() {
        if is_printable(*byte) {
            if run.is_empty() {
                start = index;
            }
            run.push(*byte as char);
        } else if long_enough(&run, min_len) {
            out.push(FoundString {
                offset: start as u64,
                encoding: StringEncoding::Ascii,
                text: std::mem::take(&mut run),
            });
        } else {
            run.clear();
        }
    }
    if long_enough(&run, min_len) {
        out.push(FoundString {
            offset: start as u64,
            encoding: StringEncoding::Ascii,
            text: run,
        });
    }
    out
}

fn utf16le_runs(bytes: &[u8], min_len: usize) -> Vec<FoundString> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut run = String::new();
    let mut index = 0usize;
    while index + 1 < bytes.len() {
        if is_printable(bytes[index]) && bytes[index + 1] == 0 {
            if run.is_empty() {
                start = index;
            }
            run.push(bytes[index] as char);
            index += 2;
            continue;
        }
        if long_enough(&run, min_len) {
            out.push(FoundString {
                offset: start as u64,
                encoding: StringEncoding::Utf16le,
                text: std::mem::take(&mut run),
            });
        } else {
            run.clear();
        }
        index += 1;
    }
    if long_enough(&run, min_len) {
        out.push(FoundString {
            offset: start as u64,
            encoding: StringEncoding::Utf16le,
            text: run,
        });
    }
    out
}

/// Reports whether the pending run is long enough to emit.
fn long_enough(run: &str, min_len: usize) -> bool {
    run.chars().count() >= min_len
}

/// Escapes a decoded string for single-line output.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_patterns_with_and_without_wildcards() {
        assert_eq!(parse_pattern("4d5a"), Ok(vec![Some(0x4d), Some(0x5a)]));
        assert_eq!(parse_pattern("4d 5a"), Ok(vec![Some(0x4d), Some(0x5a)]));
        assert_eq!(
            parse_pattern("4d??00"),
            Ok(vec![Some(0x4d), None, Some(0x00)])
        );
        assert_eq!(parse_pattern("AB"), Ok(vec![Some(0xab)]));
        assert_eq!(parse_pattern("????"), Ok(vec![None, None]));
    }

    #[test]
    fn rejects_malformed_patterns() {
        for bad in ["", "   ", "4", "4d5", "zz", "4?", "?d", "4d 5"] {
            assert!(parse_pattern(bad).is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn encodes_text_search_terms() {
        assert_eq!(ascii_pattern("Hi"), Ok(vec![Some(b'H'), Some(b'i')]));
        assert_eq!(
            utf16le_pattern("Hi"),
            Ok(vec![Some(b'H'), Some(0), Some(b'i'), Some(0)])
        );
        assert!(ascii_pattern("").is_err());
        assert!(ascii_pattern("é").is_err());
        assert!(utf16le_pattern("").is_err());
    }

    #[test]
    fn finds_every_occurrence_including_overlaps() {
        let haystack = b"aaaa";
        let pattern = ascii_pattern("aa").unwrap();
        assert_eq!(find_all(haystack, &pattern, None), vec![0, 1, 2]);
    }

    #[test]
    fn honours_wildcards_and_the_result_limit() {
        // Hand-built: "ax1" at 0, "ay1" at 3, "az1" at 6.
        let haystack = b"ax1ay1az1";
        let pattern = parse_pattern("61??31").unwrap();
        assert_eq!(find_all(haystack, &pattern, None), vec![0, 3, 6]);
        assert_eq!(find_all(haystack, &pattern, Some(2)), vec![0, 3]);
    }

    #[test]
    fn matches_a_leading_wildcard_pattern() {
        let haystack = &[0x00, 0x11, 0x22, 0x11, 0x22];
        let pattern = parse_pattern("??1122").unwrap();
        assert_eq!(find_all(haystack, &pattern, None), vec![0, 2]);
    }

    #[test]
    fn reports_nothing_when_the_pattern_is_longer_than_the_file() {
        assert_eq!(
            find_all(b"ab", &parse_pattern("616263").unwrap(), None),
            Vec::<u64>::new()
        );
    }

    #[test]
    fn extracts_maximal_ascii_runs_over_the_minimum_length() {
        let bytes = b"\x00abcd\x00ef\x00longer-name\x00";
        let found = extract_strings(bytes, 4, StringScan::Ascii);
        let texts: Vec<&str> = found.iter().map(|item| item.text.as_str()).collect();
        assert_eq!(texts, ["abcd", "longer-name"]);
        assert_eq!(found[0].offset, 1);
        assert_eq!(found[1].offset, 9);
    }

    #[test]
    fn extracts_utf16le_runs() {
        let mut bytes = vec![0xffu8];
        for c in "Part1".bytes() {
            bytes.push(c);
            bytes.push(0);
        }
        bytes.push(0xff);
        let found = extract_strings(&bytes, 4, StringScan::Utf16le);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "Part1");
        assert_eq!(found[0].offset, 1);
    }

    #[test]
    fn both_encodings_are_merged_and_sorted_by_offset() {
        let mut bytes = b"header".to_vec();
        bytes.push(0xff);
        for c in "wide".bytes() {
            bytes.push(c);
            bytes.push(0);
        }
        let found = extract_strings(&bytes, 4, StringScan::Both);
        let pairs: Vec<(u64, &str)> = found
            .iter()
            .map(|item| (item.offset, item.text.as_str()))
            .collect();
        assert_eq!(pairs, [(0, "header"), (7, "wide")]);
    }

    #[test]
    fn escapes_quotes_backslashes_and_tabs() {
        assert_eq!(escape("a\"b\\c\td"), "a\\\"b\\\\c\\td");
    }
}
