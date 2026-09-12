// SPDX-License-Identifier: Apache-2.0
//! The `--layout` record mini-language.
//!
//! A layout is a comma-separated list of fields read back to back with no
//! implicit alignment or padding. Each field is one of:
//!
//! - `u8`, `i8` — one byte, no byte-order suffix is allowed.
//! - `u16le`, `i32be`, `f64le`, … — a wide scalar; the `le` or `be` suffix is
//!   mandatory.
//! - `bytesN` — `N` raw bytes, printed as hexadecimal.
//! - `padN` — `N` bytes skipped and not printed. Takes no name.
//!
//! Every field except `padN` accepts an optional `:name`. Unnamed fields are
//! called `f<index>` after their position in the spec.
//!
//! Example: `u32le:count,pad4,f64le:x,f64le:y,bytes4:tag`.

use std::num::NonZeroUsize;

use super::numeric::{Endian, ScalarType, ScalarValue};

/// Lowercase hexadecimal digits, indexed by nibble.
const HEX_DIGITS: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

/// Appends `byte` as two lowercase hexadecimal digits.
pub fn push_hex(out: &mut String, byte: u8) {
    out.push(HEX_DIGITS[usize::from(byte >> 4)]);
    out.push(HEX_DIGITS[usize::from(byte & 0x0f)]);
}


/// A parse failure in a layout spec.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LayoutError {
    /// The whole spec was empty or whitespace.
    #[error("empty layout; expected fields such as `u32le:count,f64le:x`")]
    EmptySpec,
    /// A comma-separated token was empty.
    #[error("layout field {index} is empty; remove the stray comma")]
    EmptyField {
        /// Zero-based position of the token in the spec.
        index: usize,
    },
    /// A token ended in `:` with no name after it.
    #[error("layout field {index} (`{token}`) has a `:` with no name after it")]
    EmptyName {
        /// Zero-based position of the token in the spec.
        index: usize,
        /// The offending token.
        token: String,
    },
    /// A field name held a second `:`.
    #[error("`{token}`: a field name cannot contain `:`")]
    NameHasColon {
        /// The offending token.
        token: String,
    },
    /// A wide scalar was written without an `le` or `be` suffix.
    #[error("`{token}`: `{base}` needs a byte-order suffix, as in `{base}le` or `{base}be`")]
    MissingByteOrder {
        /// The offending token.
        token: String,
        /// The scalar base name.
        base: String,
    },
    /// A one-byte scalar carried a byte-order suffix.
    #[error("`{token}`: `{base}` is one byte wide, so it takes no `{suffix}` suffix")]
    ForbiddenByteOrder {
        /// The offending token.
        token: String,
        /// The scalar base name.
        base: String,
        /// The rejected suffix.
        suffix: &'static str,
    },
    /// The type name matched nothing in the grammar.
    #[error(
        "`{token}`: unknown field type `{type_text}`; expected u8/i8, \
         u16/i16/u32/i32/u64/i64/f32/f64 with an `le` or `be` suffix, `bytesN`, or `padN`"
    )]
    UnknownType {
        /// The offending token.
        token: String,
        /// The unrecognised type text.
        type_text: String,
    },
    /// `bytes` or `pad` appeared with no byte count.
    #[error("`{token}`: `{keyword}` needs a byte count, as in `{keyword}4`")]
    MissingCount {
        /// The offending token.
        token: String,
        /// The keyword that needs a count.
        keyword: &'static str,
    },
    /// The byte count after `bytes` or `pad` was not a decimal number.
    #[error("`{token}`: `{digits}` is not a decimal byte count for `{keyword}`")]
    BadCount {
        /// The offending token.
        token: String,
        /// The keyword that needs a count.
        keyword: &'static str,
        /// The text that failed to parse.
        digits: String,
    },
    /// The byte count after `bytes` or `pad` was zero.
    #[error("`{token}`: a `{keyword}` field must cover at least one byte")]
    ZeroCount {
        /// The offending token.
        token: String,
        /// The keyword that needs a count.
        keyword: &'static str,
    },
    /// A `padN` field carried a name.
    #[error("`{token}`: a pad field is never printed, so it takes no name")]
    NamedPad {
        /// The offending token.
        token: String,
    },
    /// The accumulated record size overflowed a machine word.
    #[error("`{token}`: record size overflows a machine word")]
    SizeOverflow {
        /// The offending token.
        token: String,
    },
}

/// What one layout field reads out of the record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// A fixed-width number in a stated byte order.
    Scalar(ScalarType, Endian),
    /// A run of raw bytes rendered as hexadecimal.
    Bytes(NonZeroUsize),
}

impl FieldKind {
    /// Returns how many bytes the field consumes.
    pub const fn width(self) -> NonZeroUsize {
        match self {
            Self::Scalar(ty, _) => ty.width(),
            Self::Bytes(count) => count,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Field {
    Named { name: String, kind: FieldKind },
    Pad(NonZeroUsize),
}

impl Field {
    fn width(&self) -> NonZeroUsize {
        match self {
            Self::Named { kind, .. } => kind.width(),
            Self::Pad(count) => *count,
        }
    }
}

/// A parsed record layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// Fields in spec order, including `padN` runs.
    fields: Vec<Field>,
    /// Total record size, which every layout has at least one byte of.
    size: NonZeroUsize,
}

impl Layout {
    /// Parses a layout spec.
    ///
    /// # Errors
    ///
    /// Returns [`LayoutError`] for an empty spec, an empty or unknown field
    /// token, a missing or forbidden byte-order suffix, a zero or unparsable
    /// `bytesN`/`padN` count, a name on a `padN` field, or a record whose total
    /// size overflows `usize`.
    pub fn parse(spec: &str) -> Result<Self, LayoutError> {
        if spec.trim().is_empty() {
            return Err(LayoutError::EmptySpec);
        }
        let mut fields = Vec::new();
        let mut size: Option<NonZeroUsize> = None;
        for (index, raw_token) in spec.split(',').enumerate() {
            let token = raw_token.trim();
            if token.is_empty() {
                return Err(LayoutError::EmptyField { index });
            }
            let (type_text, name) = split_name(token, index)?;
            let field = if let Some(count) = type_text.strip_prefix("pad") {
                let count = parse_count(count, token, "pad")?;
                if name.is_some() {
                    return Err(LayoutError::NamedPad {
                        token: token.to_string(),
                    });
                }
                Field::Pad(count)
            } else {
                Field::Named {
                    kind: parse_kind(type_text, token)?,
                    name: name.unwrap_or_else(|| format!("f{index}")),
                }
            };
            let width = field.width();
            size = Some(match size {
                None => width,
                Some(total) => {
                    total
                        .checked_add(width.get())
                        .ok_or_else(|| LayoutError::SizeOverflow {
                            token: token.to_string(),
                        })?
                }
            });
            fields.push(field);
        }
        let Some(size) = size else {
            return Err(LayoutError::EmptySpec);
        };
        Ok(Self { fields, size })
    }

    /// Returns the total record size in bytes.
    pub const fn size(&self) -> NonZeroUsize {
        self.size
    }

    /// Splits `bytes` into whole records, dropping a trailing partial record.
    pub fn split<'a>(&'a self, bytes: &'a [u8]) -> impl Iterator<Item = Record<'a>> {
        bytes.chunks_exact(self.size.get()).map(|bytes| Record {
            layout: self,
            bytes,
        })
    }

    /// Returns the printable field names in layout order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.fields.iter().filter_map(|field| match field {
            Field::Named { name, .. } => Some(name.as_str()),
            Field::Pad(_) => None,
        })
    }

    fn fields_with_offsets(&self) -> impl Iterator<Item = (usize, &Field)> {
        self.fields.iter().scan(0, |offset, field| {
            let start = *offset;
            *offset += field.width().get();
            Some((start, field))
        })
    }
}

/// One record-sized window of a byte buffer, paired with the layout that spans it.
#[derive(Debug, Clone, Copy)]
pub struct Record<'a> {
    layout: &'a Layout,
    bytes: &'a [u8],
}

impl<'a> Record<'a> {
    /// Returns the printable fields in layout order, dropping `padN` runs.
    pub fn fields(&self) -> impl Iterator<Item = DecodedField<'a>> {
        let bytes = self.bytes;
        self.layout
            .fields_with_offsets()
            .filter_map(move |(offset, field)| {
                let Field::Named { name, kind } = field else {
                    return None;
                };
                // `bytes` spans the whole layout: `split` is the only mint.
                let value = match *kind {
                    FieldKind::Scalar(ty, endian) => {
                        let width = ty.width().get();
                        let mut raw = [0u8; ScalarType::MAX_WIDTH];
                        raw[..width].copy_from_slice(&bytes[offset..offset + width]);
                        DecodedValue::Scalar {
                            value: ty.window_of(&raw).read(endian),
                            endian,
                        }
                    }
                    FieldKind::Bytes(count) => {
                        DecodedValue::Bytes(RawBytes::new(bytes, offset, count))
                    }
                };
                Some(DecodedField {
                    name,
                    offset,
                    value,
                })
            })
    }
}

/// A run of raw bytes covering at least one byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawBytes<'a>(&'a [u8]);

impl<'a> RawBytes<'a> {
    /// Returns the run of `count` bytes of `bytes` that starts at `offset`.
    ///
    /// The count is the field's own width, so the run covers at least one byte
    /// and the length is read back off the slice.
    pub fn new(bytes: &'a [u8], offset: usize, count: NonZeroUsize) -> Self {
        Self(&bytes[offset..offset + count.get()])
    }

    /// Returns the bytes of the run.
    pub const fn as_slice(self) -> &'a [u8] {
        self.0
    }

    /// Returns how many bytes the run covers.
    pub fn len(self) -> NonZeroUsize {
        NonZeroUsize::new(self.0.len()).expect("a raw byte run covers at least one byte")
    }
}

/// The reading one decoded field carries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DecodedValue<'a> {
    /// A number decoded in the byte order the layout states.
    Scalar {
        /// The decoded value, which names its own encoded type.
        value: ScalarValue,
        /// The byte order the value was decoded in.
        endian: Endian,
    },
    /// A run of raw bytes with no numeric reading.
    Bytes(RawBytes<'a>),
}

/// One field of a decoded record, ready to print.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecodedField<'a> {
    name: &'a str,
    offset: usize,
    value: DecodedValue<'a>,
}

impl<'a> DecodedField<'a> {
    /// Returns the field name.
    pub const fn name(&self) -> &'a str {
        self.name
    }

    /// Returns the byte offset from the start of the record.
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// Returns the reading the field carries.
    pub const fn value(&self) -> DecodedValue<'a> {
        self.value
    }

    /// Returns the type name including any byte-order suffix.
    pub fn type_name(&self) -> String {
        match self.value {
            DecodedValue::Scalar { value, endian } => value.ty().display_name(endian),
            DecodedValue::Bytes(raw) => format!("bytes{}", raw.len()),
        }
    }

    /// Returns the hexadecimal rendering of the encoded bytes.
    pub fn hex(&self) -> String {
        match self.value {
            DecodedValue::Scalar { value, .. } => value.hex(),
            DecodedValue::Bytes(raw) => hex_bytes(raw.as_slice()),
        }
    }
}

fn split_name(token: &str, index: usize) -> Result<(&str, Option<String>), LayoutError> {
    match token.split_once(':') {
        None => Ok((token, None)),
        Some((type_text, name)) => {
            let name = name.trim();
            if name.is_empty() {
                return Err(LayoutError::EmptyName {
                    index,
                    token: token.to_string(),
                });
            }
            if name.contains(':') {
                return Err(LayoutError::NameHasColon {
                    token: token.to_string(),
                });
            }
            Ok((type_text.trim(), Some(name.to_string())))
        }
    }
}

fn parse_kind(type_text: &str, token: &str) -> Result<FieldKind, LayoutError> {
    if let Some(count) = type_text.strip_prefix("bytes") {
        return parse_count(count, token, "bytes").map(FieldKind::Bytes);
    }
    if let Some(ty) = ScalarType::from_base_name(type_text) {
        if ty.is_single_byte() {
            return Ok(FieldKind::Scalar(ty, Endian::Le));
        }
        return Err(LayoutError::MissingByteOrder {
            token: token.to_string(),
            base: type_text.to_string(),
        });
    }
    for (suffix, endian) in [("le", Endian::Le), ("be", Endian::Be)] {
        let Some(base) = type_text.strip_suffix(suffix) else {
            continue;
        };
        let Some(ty) = ScalarType::from_base_name(base) else {
            continue;
        };
        if ty.is_single_byte() {
            return Err(LayoutError::ForbiddenByteOrder {
                token: token.to_string(),
                base: base.to_string(),
                suffix,
            });
        }
        return Ok(FieldKind::Scalar(ty, endian));
    }
    Err(LayoutError::UnknownType {
        token: token.to_string(),
        type_text: type_text.to_string(),
    })
}

fn parse_count(
    digits: &str,
    token: &str,
    keyword: &'static str,
) -> Result<NonZeroUsize, LayoutError> {
    if digits.is_empty() {
        return Err(LayoutError::MissingCount {
            token: token.to_string(),
            keyword,
        });
    }
    let count: usize = digits.parse().map_err(|_| LayoutError::BadCount {
        token: token.to_string(),
        keyword,
        digits: digits.to_string(),
    })?;
    NonZeroUsize::new(count).ok_or_else(|| LayoutError::ZeroCount {
        token: token.to_string(),
        keyword,
    })
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 3);
    for (index, byte) in bytes.iter().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        push_hex(&mut out, *byte);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_does_not_widen_printed_name_column() {
        let layout = Layout::parse("u8:x,pad4,u8:y").unwrap();
        assert_eq!(layout.names().map(str::len).max(), Some(1));
        assert_eq!(layout.names().collect::<Vec<_>>(), ["x", "y"]);
    }

    fn kinds(spec: &str) -> Vec<FieldKind> {
        Layout::parse(spec)
            .unwrap()
            .fields
            .into_iter()
            .filter_map(|field| match field {
                Field::Named { kind, .. } => Some(kind),
                Field::Pad(_) => None,
            })
            .collect()
    }

    #[test]
    fn parses_the_documented_example() {
        let layout = Layout::parse("u32le:count,pad4,f64le:x,f64le:y,bytes4:tag").unwrap();
        assert_eq!(layout.size().get(), 4 + 4 + 8 + 8 + 4);
        let names: Vec<&str> = layout.names().collect();
        assert_eq!(names, ["count", "x", "y", "tag"]);
        let offsets: Vec<usize> = layout
            .fields_with_offsets()
            .map(|(offset, _)| offset)
            .collect();
        assert_eq!(offsets, [0, 4, 8, 16, 24]);
    }

    #[test]
    fn accepts_every_scalar_width_and_both_byte_orders() {
        assert_eq!(
            kinds("u8,i8"),
            [
                FieldKind::Scalar(ScalarType::U8, Endian::Le),
                FieldKind::Scalar(ScalarType::I8, Endian::Le),
            ]
        );
        assert_eq!(
            kinds("u16be,i64le,f32be,f64le"),
            [
                FieldKind::Scalar(ScalarType::U16, Endian::Be),
                FieldKind::Scalar(ScalarType::I64, Endian::Le),
                FieldKind::Scalar(ScalarType::F32, Endian::Be),
                FieldKind::Scalar(ScalarType::F64, Endian::Le),
            ]
        );
    }

    #[test]
    fn tolerates_whitespace_around_tokens_and_names() {
        let layout = Layout::parse(" u32le : count , f64be:x ").unwrap();
        assert_eq!(layout.size().get(), 12);
        assert_eq!(layout.names().collect::<Vec<_>>(), ["count", "x"]);
    }

    #[track_caller]
    fn error(spec: &str) -> LayoutError {
        Layout::parse(spec).expect_err("spec should not parse")
    }

    #[test]
    fn rejects_empty_specs_and_stray_commas() {
        assert_eq!(error(""), LayoutError::EmptySpec);
        assert_eq!(error("   "), LayoutError::EmptySpec);
        assert_eq!(error(","), LayoutError::EmptyField { index: 0 });
        assert_eq!(error(",u32le"), LayoutError::EmptyField { index: 0 });
        assert_eq!(error("u32le,"), LayoutError::EmptyField { index: 1 });
        assert_eq!(error("u32le,,f64le"), LayoutError::EmptyField { index: 1 });
    }

    #[test]
    fn rejects_a_colon_with_no_name() {
        assert_eq!(
            error("u32le:"),
            LayoutError::EmptyName {
                index: 0,
                token: "u32le:".to_string(),
            }
        );
        assert_eq!(
            error("f64le,u32le: "),
            LayoutError::EmptyName {
                index: 1,
                token: "u32le:".to_string(),
            }
        );
        assert_eq!(
            error("u32le:a:b"),
            LayoutError::NameHasColon {
                token: "u32le:a:b".to_string(),
            }
        );
    }

    #[test]
    fn rejects_missing_or_forbidden_byte_order_suffixes() {
        assert_eq!(
            error("u32"),
            LayoutError::MissingByteOrder {
                token: "u32".to_string(),
                base: "u32".to_string(),
            }
        );
        assert_eq!(
            error("u8le"),
            LayoutError::ForbiddenByteOrder {
                token: "u8le".to_string(),
                base: "u8".to_string(),
                suffix: "le",
            }
        );
        assert_eq!(
            error("i8be"),
            LayoutError::ForbiddenByteOrder {
                token: "i8be".to_string(),
                base: "i8".to_string(),
                suffix: "be",
            }
        );
    }

    #[test]
    fn rejects_unknown_types() {
        for bad in ["u24le", "float64", "u32me"] {
            assert!(
                matches!(error(bad), LayoutError::UnknownType { .. }),
                "{bad:?} should be an unknown type, got {:?}",
                error(bad)
            );
        }
    }

    #[test]
    fn rejects_bad_bytes_and_pad_counts() {
        assert_eq!(
            error("bytes"),
            LayoutError::MissingCount {
                token: "bytes".to_string(),
                keyword: "bytes",
            }
        );
        assert_eq!(
            error("bytes0"),
            LayoutError::ZeroCount {
                token: "bytes0".to_string(),
                keyword: "bytes",
            }
        );
        assert_eq!(
            error("pad0"),
            LayoutError::ZeroCount {
                token: "pad0".to_string(),
                keyword: "pad",
            }
        );
        assert_eq!(
            error("padx"),
            LayoutError::BadCount {
                token: "padx".to_string(),
                keyword: "pad",
                digits: "x".to_string(),
            }
        );
        assert!(matches!(error("bytes-1"), LayoutError::BadCount { .. }));
    }

    #[test]
    fn rejects_a_named_pad() {
        assert_eq!(
            error("pad4:reserved"),
            LayoutError::NamedPad {
                token: "pad4:reserved".to_string(),
            }
        );
    }

    #[test]
    fn decode_reads_hand_built_bytes_and_drops_pad() {
        let layout = Layout::parse("u32le:count,pad2,i16be:delta,bytes2:tag").unwrap();
        assert_eq!(layout.size().get(), 10);
        // count = 0x0000002a = 42, pad, delta = 0xfffe = -2, tag = ab cd.
        let record = [0x2a, 0x00, 0x00, 0x00, 0xee, 0xee, 0xff, 0xfe, 0xab, 0xcd];
        let records: Vec<_> = layout.split(&record).collect();
        assert_eq!(records.len(), 1);
        let decoded: Vec<_> = records[0].fields().collect();
        assert_eq!(decoded.len(), 3);
        assert_eq!(decoded[0].name(), "count");
        assert_eq!(decoded[0].type_name(), "u32le");
        assert_eq!(
            decoded[0].value(),
            DecodedValue::Scalar {
                value: ScalarValue::U32(42),
                endian: Endian::Le,
            }
        );
        assert_eq!(decoded[0].hex(), "0x0000002a");
        assert_eq!(decoded[1].name(), "delta");
        assert_eq!(
            decoded[1].value(),
            DecodedValue::Scalar {
                value: ScalarValue::I16(-2),
                endian: Endian::Be,
            }
        );
        assert_eq!(decoded[1].hex(), "0xfffe");
        assert_eq!(decoded[1].offset(), 6);
        assert_eq!(decoded[2].type_name(), "bytes2");
        assert_eq!(decoded[2].hex(), "ab cd");
        let DecodedValue::Bytes(raw) = decoded[2].value() else {
            panic!("the tag field is a raw byte run");
        };
        assert_eq!(raw.as_slice(), &[0xab, 0xcd]);
        assert_eq!(raw.len().get(), 2);
    }

    #[test]
    fn split_drops_a_trailing_partial_record() {
        let layout = Layout::parse("u16le:a").unwrap();
        let starts: Vec<usize> = layout
            .split(&[1, 2, 3, 4, 5])
            .map(|record| record.fields().next().expect("one named field").offset())
            .collect();
        assert_eq!(starts, [0, 0]);
        assert_eq!(layout.split(&[1]).count(), 0);
    }
}
