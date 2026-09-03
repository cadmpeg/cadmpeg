// SPDX-License-Identifier: Apache-2.0
//! Offset argument parsing and fixed-width scalar reads.

use clap::{Args, ValueEnum};

use cadmpeg_core::bytes::assemble_u64_le;

/// Parses a byte count or file offset written in hexadecimal or decimal.
///
/// `0x`/`0X` selects hexadecimal, anything else is decimal. Underscores
/// separate digit groups and are ignored. The value is unsigned; a leading
/// sign is rejected so that a mistyped offset never silently wraps.
pub fn parse_offset(text: &str) -> Result<u64, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("expected a byte offset, found an empty value".to_string());
    }
    let (digits, radix) = match trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        Some(rest) => (rest, 16),
        None => (trimmed, 10),
    };
    let cleaned: String = digits.chars().filter(|c| *c != '_').collect();
    if cleaned.is_empty() {
        return Err(format!("{trimmed}: no digits after the radix prefix"));
    }
    if !cleaned.chars().all(|c| c.is_digit(radix)) {
        let kind = if radix == 16 {
            "hexadecimal"
        } else {
            "decimal"
        };
        return Err(format!("{trimmed}: not a {kind} byte offset"));
    }
    u64::from_str_radix(&cleaned, radix).map_err(|_| format!("{trimmed}: does not fit in 64 bits"))
}

/// Byte order applied to a multi-byte scalar read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endian {
    /// Least significant byte first.
    Little,
    /// Most significant byte first.
    Big,
}

impl Endian {
    /// Returns the two-letter suffix used in output and in layout specs.
    pub const fn suffix(self) -> &'static str {
        match self {
            Self::Little => "le",
            Self::Big => "be",
        }
    }
}

/// Mutually exclusive byte-order selection flags.
#[derive(Debug, Clone, Args)]
pub struct EndianArgs {
    /// Read little-endian. This is the default.
    #[arg(long, conflicts_with = "be")]
    le: bool,
    /// Read big-endian.
    #[arg(long)]
    be: bool,
}

impl EndianArgs {
    /// Returns the selected byte order, defaulting to little-endian.
    pub fn mode(&self) -> Endian {
        match (self.le, self.be) {
            (false, true) => Endian::Big,
            (_, false) => Endian::Little,
            (true, true) => unreachable!("clap rejects conflicting byte-order flags"),
        }
    }
}

/// Parses `--type`, teaching the right tool for non-scalar guesses.
///
/// Delegates to the `ScalarType` value enum — including its possible-values
/// help list — but catches the recurring text and hex-dump guesses with an
/// error that names the tool that does that job.
#[derive(Clone)]
pub struct ScalarTypeParser;

impl clap::builder::TypedValueParser for ScalarTypeParser {
    type Value = ScalarType;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<ScalarType, clap::Error> {
        let redirect = match value.to_str().map(str::to_ascii_lowercase).as_deref() {
            Some("ascii" | "string" | "str" | "text" | "utf8") => Some(
                "--type takes a fixed-width scalar; for text use `cadmpeg inspect strings`, \
                 or `cadmpeg inspect find --ascii TEXT` to locate it",
            ),
            Some("hex" | "bytes") => {
                Some("--type takes a fixed-width scalar; for a hex dump use `cadmpeg inspect hex`")
            }
            _ => None,
        };
        if let Some(message) = redirect {
            return Err(clap::Error::raw(
                clap::error::ErrorKind::InvalidValue,
                format!("{message}\n"),
            )
            .with_cmd(cmd));
        }
        clap::builder::EnumValueParser::<ScalarType>::new().parse_ref(cmd, arg, value)
    }

    fn possible_values(
        &self,
    ) -> Option<Box<dyn Iterator<Item = clap::builder::PossibleValue> + '_>> {
        Some(Box::new(
            clap::ValueEnum::value_variants()
                .iter()
                .filter_map(|variant: &ScalarType| clap::ValueEnum::to_possible_value(variant)),
        ))
    }
}

/// A fixed-width scalar that a decoder reads out of a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ScalarType {
    /// Unsigned 8-bit.
    U8,
    /// Signed 8-bit.
    I8,
    /// Unsigned 16-bit.
    U16,
    /// Signed 16-bit.
    I16,
    /// Unsigned 32-bit.
    U32,
    /// Signed 32-bit.
    I32,
    /// Unsigned 64-bit.
    U64,
    /// Signed 64-bit.
    I64,
    /// IEEE-754 binary32.
    F32,
    /// IEEE-754 binary64.
    F64,
}

impl ScalarType {
    /// Returns the encoded width in bytes.
    pub const fn width(self) -> usize {
        match self {
            Self::U8 | Self::I8 => 1,
            Self::U16 | Self::I16 => 2,
            Self::U32 | Self::I32 | Self::F32 => 4,
            Self::U64 | Self::I64 | Self::F64 => 8,
        }
    }

    /// Returns true when the encoding is one byte wide and byte order is moot.
    pub const fn is_single_byte(self) -> bool {
        self.width() == 1
    }

    /// Returns the spec name without a byte-order suffix.
    pub const fn base_name(self) -> &'static str {
        match self {
            Self::U8 => "u8",
            Self::I8 => "i8",
            Self::U16 => "u16",
            Self::I16 => "i16",
            Self::U32 => "u32",
            Self::I32 => "i32",
            Self::U64 => "u64",
            Self::I64 => "i64",
            Self::F32 => "f32",
            Self::F64 => "f64",
        }
    }

    /// Returns the display name, with a byte-order suffix for wide types.
    pub fn display_name(self, endian: Endian) -> String {
        if self.is_single_byte() {
            self.base_name().to_string()
        } else {
            format!("{}{}", self.base_name(), endian.suffix())
        }
    }

    /// Parses a base type name with no byte-order suffix.
    pub fn from_base_name(name: &str) -> Option<Self> {
        [
            Self::U8,
            Self::I8,
            Self::U16,
            Self::I16,
            Self::U32,
            Self::I32,
            Self::U64,
            Self::I64,
            Self::F32,
            Self::F64,
        ]
        .into_iter()
        .find(|candidate| candidate.base_name() == name)
    }

    /// Decodes one value from exactly [`ScalarType::width`] bytes.
    ///
    /// # Panics
    ///
    /// Panics when `bytes` is not exactly the encoded width. Callers slice the
    /// window before calling, so a mismatch is a programming error.
    pub fn read(self, bytes: &[u8], endian: Endian) -> ScalarValue {
        assert_eq!(
            bytes.len(),
            self.width(),
            "scalar read needs an exact slice"
        );
        let mut raw = [0u8; 8];
        raw[..bytes.len()].copy_from_slice(bytes);
        if endian == Endian::Big {
            raw[..bytes.len()].reverse();
        }
        let bits = assemble_u64_le(raw);
        match self {
            Self::U8 => ScalarValue::U8(bits as u8),
            Self::I8 => ScalarValue::I8(bits as i8),
            Self::U16 => ScalarValue::U16(bits as u16),
            Self::I16 => ScalarValue::I16(bits as i16),
            Self::U32 => ScalarValue::U32(bits as u32),
            Self::I32 => ScalarValue::I32(bits as i32),
            Self::U64 => ScalarValue::U64(bits),
            Self::I64 => ScalarValue::I64(bits as i64),
            Self::F32 => ScalarValue::F32(f32::from_bits(bits as u32)),
            Self::F64 => ScalarValue::F64(f64::from_bits(bits)),
        }
    }
}

/// A decoded scalar whose variant retains its exact encoded type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScalarValue {
    U8(u8),
    I8(i8),
    U16(u16),
    I16(i16),
    U32(u32),
    I32(i32),
    U64(u64),
    I64(i64),
    F32(f32),
    F64(f64),
}

impl ScalarValue {
    /// Renders the value the way a human reads it.
    pub fn decimal(self) -> String {
        match self {
            Self::U8(value) => value.to_string(),
            Self::I8(value) => value.to_string(),
            Self::U16(value) => value.to_string(),
            Self::I16(value) => value.to_string(),
            Self::U32(value) => value.to_string(),
            Self::I32(value) => value.to_string(),
            Self::U64(value) => value.to_string(),
            Self::I64(value) => value.to_string(),
            Self::F32(value) => format!("{:?}", f64::from(value)),
            Self::F64(value) => format!("{value:?}"),
        }
    }

    /// Renders the encoded bit pattern, zero-padded to the type width.
    ///
    /// Signed values print their two's-complement pattern and floats print
    /// their IEEE-754 bits, so the text always matches what is in the file.
    pub fn hex(self) -> String {
        let bits = match self {
            Self::U8(value) => u64::from(value),
            Self::I8(value) => u64::from(value as u8),
            Self::U16(value) => u64::from(value),
            Self::I16(value) => u64::from(value as u16),
            Self::U32(value) => u64::from(value),
            Self::I32(value) => u64::from(value as u32),
            Self::U64(value) => value,
            Self::I64(value) => value as u64,
            Self::F32(value) => u64::from(value.to_bits()),
            Self::F64(value) => value.to_bits(),
        };
        let digits = match self {
            Self::U8(_) | Self::I8(_) => 2,
            Self::U16(_) | Self::I16(_) => 4,
            Self::U32(_) | Self::I32(_) | Self::F32(_) => 8,
            Self::U64(_) | Self::I64(_) | Self::F64(_) => 16,
        };
        format!("0x{bits:0digits$x}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_offset_reads_both_radixes() {
        assert_eq!(parse_offset("0"), Ok(0));
        assert_eq!(parse_offset("4096"), Ok(4096));
        assert_eq!(parse_offset("0x1000"), Ok(4096));
        assert_eq!(parse_offset("0X1000"), Ok(4096));
        assert_eq!(parse_offset("  0x1_0000 "), Ok(65536));
        assert_eq!(parse_offset("1_000"), Ok(1000));
    }

    #[test]
    fn parse_offset_rejects_garbage() {
        for bad in ["", "   ", "0x", "-1", "+1", "12g", "0xzz", "1.5", "0b101"] {
            assert!(parse_offset(bad).is_err(), "{bad} should not parse");
        }
    }

    #[test]
    fn parse_offset_rejects_overflow() {
        assert_eq!(parse_offset("0xffffffffffffffff"), Ok(u64::MAX));
        assert!(parse_offset("0x1_0000_0000_0000_0000").is_err());
    }

    #[test]
    fn unsigned_reads_follow_byte_order() {
        // 0x0102 big-endian is 258; the same bytes little-endian are 0x0201.
        let bytes = [0x01, 0x02];
        assert_eq!(
            ScalarType::U16.read(&bytes, Endian::Big),
            ScalarValue::U16(258)
        );
        assert_eq!(
            ScalarType::U16.read(&bytes, Endian::Little),
            ScalarValue::U16(513)
        );
    }

    #[test]
    fn signed_reads_sign_extend_at_each_width() {
        assert_eq!(
            ScalarType::I8.read(&[0xff], Endian::Little),
            ScalarValue::I8(-1)
        );
        assert_eq!(
            ScalarType::I16.read(&[0x00, 0x80], Endian::Little),
            ScalarValue::I16(-32768)
        );
        assert_eq!(
            ScalarType::I32.read(&[0xff, 0xff, 0xff, 0xff], Endian::Big),
            ScalarValue::I32(-1)
        );
        assert_eq!(
            ScalarType::I64.read(&[0, 0, 0, 0, 0, 0, 0, 0x80], Endian::Little),
            ScalarValue::I64(i64::MIN)
        );
    }

    #[test]
    fn float_reads_match_hand_built_bit_patterns() {
        // 1.5f64 is sign 0, exponent 0x3ff, mantissa 0x8000000000000.
        let one_point_five = 0x3ff8_0000_0000_0000u64.to_le_bytes();
        assert_eq!(
            ScalarType::F64.read(&one_point_five, Endian::Little),
            ScalarValue::F64(1.5)
        );
        // -2.0f32 is sign 1, exponent 0x80, mantissa 0.
        let minus_two = 0xc000_0000u32.to_be_bytes();
        assert_eq!(
            ScalarType::F32.read(&minus_two, Endian::Big),
            ScalarValue::F32(-2.0)
        );
    }

    #[test]
    fn hex_rendering_pads_to_the_type_width() {
        assert_eq!(ScalarValue::U32(1).hex(), "0x00000001");
        assert_eq!(ScalarValue::I16(-1).hex(), "0xffff");
        assert_eq!(ScalarValue::I64(-1).hex(), "0xffffffffffffffff");
        assert_eq!(ScalarValue::F64(1.5).hex(), "0x3ff8000000000000");
        assert_eq!(ScalarValue::F32(-2.0).hex(), "0xc0000000");
    }

    #[test]
    fn display_names_carry_a_suffix_only_for_wide_types() {
        assert_eq!(ScalarType::U8.display_name(Endian::Big), "u8");
        assert_eq!(ScalarType::F64.display_name(Endian::Big), "f64be");
    }
}
