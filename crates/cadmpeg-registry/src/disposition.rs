// SPDX-License-Identifier: Apache-2.0
//! Public read/write capability types and TOML cell conversion.

use std::fmt;

use serde::Deserialize;

/// A capability-registry cell whose word is outside its vocabulary.
#[derive(Debug, thiserror::Error)]
#[error("{word:?} is not a {column} disposition; expected one of: {expected}", expected = .column.vocabulary())]
pub struct UnknownDisposition {
    /// The registry column the word came from, `read` or `write`.
    column: Column,
    /// The word the registry carried.
    word: String,
}

#[derive(Debug, Clone, Copy)]
enum Column {
    Read,
    Write,
}

impl Column {
    const fn vocabulary(self) -> &'static str {
        match self {
            Self::Read => ReadDisposition::VOCABULARY,
            Self::Write => WriteDisposition::VOCABULARY,
        }
    }
}

impl fmt::Display for Column {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Read => "read",
            Self::Write => "write",
        })
    }
}

/// A capability ladder level from zero through nine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "u8")]
pub struct LadderLevel(u8);

/// A level outside the capability ladder.
#[derive(Debug, thiserror::Error)]
#[error("level {0} is outside 0..=9")]
pub struct InvalidLadderLevel(u8);

impl TryFrom<u8> for LadderLevel {
    type Error = InvalidLadderLevel;

    fn try_from(level: u8) -> Result<Self, Self::Error> {
        if level <= 9 {
            Ok(Self(level))
        } else {
            Err(InvalidLadderLevel(level))
        }
    }
}

impl fmt::Display for LadderLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// What cadmpeg does when it reads a dialect.
///
/// The `read` column of `docs/dialect-support.toml`, verbatim. The column is
/// three refusal-and-recovery states plus one ladder score, and they are not
/// points on one scale: `detected` is "recognized, with no fixture witnessing
/// a decode", which is a statement about evidence, while `refused` is a
/// statement about the codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub enum ReadDisposition {
    /// A `docs/format-support.md` ladder score, `L0` through `L9`.
    Level(LadderLevel),
    /// Classified, with no fixture witnessing a decode. The floor for an
    /// unwitnessed dialect.
    Detected,
    /// The codec refuses the file, by `Admission::Refused` or by a
    /// `CodecError` raised before any report exists.
    Refused,
    /// Parsed with a strategy some other row declares, which is
    /// `Admission::Unverified`, or with no declared strategy at all, which
    /// is `Admission::Residual`.
    UnclassifiedRecovered,
}

impl ReadDisposition {
    /// The vocabulary, for a refusal message.
    const VOCABULARY: &'static str = "L0..L9, detected, refused, unclassified-recovered";
}

impl fmt::Display for ReadDisposition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Level(level) => write!(f, "L{level}"),
            Self::Detected => f.write_str("detected"),
            Self::Refused => f.write_str("refused"),
            Self::UnclassifiedRecovered => f.write_str("unclassified-recovered"),
        }
    }
}

impl TryFrom<String> for ReadDisposition {
    type Error = UnknownDisposition;

    fn try_from(word: String) -> Result<Self, Self::Error> {
        match word.as_str() {
            "detected" => return Ok(Self::Detected),
            "refused" => return Ok(Self::Refused),
            "unclassified-recovered" => return Ok(Self::UnclassifiedRecovered),
            _ => {}
        }
        if let Some(level) = word
            .strip_prefix('L')
            .and_then(|rest| rest.parse::<u8>().ok())
            .and_then(|level| LadderLevel::try_from(level).ok())
        {
            return Ok(Self::Level(level));
        }
        Err(UnknownDisposition {
            column: Column::Read,
            word,
        })
    }
}

/// What cadmpeg does when it writes a dialect.
///
/// The `write` column of `docs/dialect-support.toml`, verbatim. Synthesis and
/// preservation are different capabilities and the column never conflates
/// them: `verified` and `emitted` grade a `TargetDescriptor` this build can
/// synthesize, `preserved` records that a same-dialect re-export replays a
/// retained baseline instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub enum WriteDisposition {
    /// Synthesized, with checked-in golden artifacts pinning the bytes.
    Verified,
    /// Synthesized, with no golden pinning the bytes.
    Emitted,
    /// Not synthesized. A same-dialect re-export replays the retained source.
    Preserved,
    /// Not written at all.
    None,
}

impl WriteDisposition {
    /// The vocabulary, for a refusal message.
    const VOCABULARY: &'static str = "verified, emitted, preserved, none";
}

impl fmt::Display for WriteDisposition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Verified => "verified",
            Self::Emitted => "emitted",
            Self::Preserved => "preserved",
            Self::None => "none",
        })
    }
}

impl TryFrom<String> for WriteDisposition {
    type Error = UnknownDisposition;

    fn try_from(word: String) -> Result<Self, Self::Error> {
        match word.as_str() {
            "verified" => Ok(Self::Verified),
            "emitted" => Ok(Self::Emitted),
            "preserved" => Ok(Self::Preserved),
            "none" => Ok(Self::None),
            _ => Err(UnknownDisposition {
                column: Column::Write,
                word,
            }),
        }
    }
}

/// What cadmpeg declares it does with one dialect, read and write.
///
/// Declared, not observed: this is the static fact a file-open dialog needs
/// before it opens anything. What a particular run did is
/// `cadmpeg_core::dialect::Admission`, and no preflight can report it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Disposition {
    /// The `read` cell.
    pub read: ReadDisposition,
    /// The `write` cell.
    pub write: WriteDisposition,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_disposition_vocabulary_is_closed() {
        for word in ["L0", "L9", "detected", "refused", "unclassified-recovered"] {
            let read = ReadDisposition::try_from(word.to_owned()).expect("a declared word parses");
            assert_eq!(read.to_string(), word);
        }
        for word in ["verified", "emitted", "preserved", "none"] {
            let write =
                WriteDisposition::try_from(word.to_owned()).expect("a declared word parses");
            assert_eq!(write.to_string(), word);
        }
        assert!(ReadDisposition::try_from("L10".to_owned()).is_err());
        assert!(ReadDisposition::try_from("L".to_owned()).is_err());
        assert!(ReadDisposition::try_from("verified".to_owned()).is_err());
        assert!(WriteDisposition::try_from("L4".to_owned()).is_err());
    }
}
