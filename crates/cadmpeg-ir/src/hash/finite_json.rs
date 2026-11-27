// SPDX-License-Identifier: Apache-2.0
//! Canonical and value JSON writing over `cadmpeg_core::finite_json`.
//!
//! `serde_json` writes `NaN` and `±inf` as `null`, so a document carrying a
//! non-finite float digests as if it carried `null` and the digest is not total
//! over what the IR can hold. The adapter in `cadmpeg_core::finite_json` wraps
//! a JSON serializer, refuses a non-finite `f64` or `f32`, and delegates every
//! other method unchanged.
//!
//! Readers of a whole IR value that need `serde_json`'s own float spelling or
//! the names of the structs it holds take the same walk through
//! [`write_serde_json`], so each IR type compiles one JSON serializer.
//! [`to_json_value`] takes it too, so a `serde_json::Value` of an IR type
//! reuses this serializer's code instead of compiling every IR type against
//! `serde_json`'s value serializer as well.

use cadmpeg_core::finite_json::{self as core_json, FiniteJsonError, JsonFloats, JsonStructs};
use serde::ser::{Error as _, Serialize};

pub(crate) mod admission;
#[cfg(test)]
mod tests;
use admission::CanonicalAdmission;

/// Canonical JSON could not be written.
#[derive(Debug, thiserror::Error)]
pub enum CanonicalJsonError {
    /// The caller's resource policy refused canonical ordering.
    #[error(transparent)]
    Resource(#[from] cadmpeg_core::CodecError),
    /// A float on the value is not finite, so canonical JSON cannot state it.
    #[error("canonical JSON holds no non-finite float: {value}")]
    NonFinite {
        /// The refused float.
        value: f64,
    },
    /// The value does not serialize as canonical JSON.
    #[error("canonical JSON serialization: {0}")]
    Serialize(serde_json::Error),
    /// The document nests past a depth bound of a CADIR document (§2.5).
    #[error("the document {} {at}", .bound.describe())]
    PastBound {
        /// The bound the document passes.
        bound: crate::bounds::Bound,
        /// Where the document passes it: the record, or the field outside
        /// every record.
        at: String,
    },
}

impl From<FiniteJsonError> for CanonicalJsonError {
    fn from(error: FiniteJsonError) -> Self {
        match error {
            FiniteJsonError::Resource(limit) => Self::Resource(limit.into()),
            FiniteJsonError::NonFinite(value) => Self::NonFinite { value },
            FiniteJsonError::Serialize(error) => Self::Serialize(error),
            FiniteJsonError::Write(error) => Self::Serialize(serde_json::Error::io(error)),
        }
    }
}

/// Writes `value` as canonical pretty JSON, refusing a non-finite float.
///
/// The bytes are the ones `serde_json::to_writer_pretty` produces. The float
/// refusal is the adapter's, not `serde_json`'s: `serde_json` writes a
/// non-finite float as `null`.
pub(super) fn write_canonical_json<T: Serialize + ?Sized>(
    admission: CanonicalAdmission<'_, '_>,
    writer: &mut dyn std::io::Write,
    value: &T,
) -> Result<(), CanonicalJsonError> {
    write_json(admission, JsonFloats::Canonical, None, writer, value)
}

/// Writes an already-erased value as canonical pretty JSON, refusing a
/// non-finite float without adding a second erasure layer.
pub(crate) fn write_canonical_json_erased(
    admission: CanonicalAdmission<'_, '_>,
    writer: &mut dyn std::io::Write,
    write: &dyn Fn(
        cadmpeg_core::serde_erase::SerAdapter<'_>,
    ) -> Result<(), cadmpeg_core::serde_erase::SerError>,
) -> Result<(), CanonicalJsonError> {
    write_json_erased(admission, JsonFloats::Canonical, None, writer, write)
}

/// Writes an already-erased value as pretty JSON with `serde_json` float
/// semantics (negative zero normalized to zero, non-finite floats written as
/// `null`) without adding a second erasure layer.
pub(crate) fn write_serde_json_erased(
    admission: CanonicalAdmission<'_, '_>,
    writer: &mut dyn std::io::Write,
    write: &dyn Fn(
        cadmpeg_core::serde_erase::SerAdapter<'_>,
    ) -> Result<(), cadmpeg_core::serde_erase::SerError>,
) -> Result<(), CanonicalJsonError> {
    write_json_erased(admission, JsonFloats::SerdeJson, None, writer, write)
}

/// Receives the structs and maps a [`write_serde_json`] walk writes, in
/// writing order.
pub(crate) trait Structs {
    /// A struct named `name` begins: its JSON text follows.
    fn begin(&self, name: &'static str);
    /// The innermost struct that began has ended: its JSON text precedes.
    fn end(&self);
    /// A map begins: its JSON text follows.
    fn map(&self) {}
}

struct StructsBridge<'a>(&'a dyn Structs);

impl JsonStructs for StructsBridge<'_> {
    fn begin(&self, name: &'static str) {
        self.0.begin(name);
    }

    fn end(&self) {
        self.0.end();
    }

    fn map(&self) {
        self.0.map();
    }
}

/// Writes `value` as the pretty JSON `serde_json::to_writer_pretty` produces,
/// admitting each value through `admission`.
///
/// Floats are spelled as `serde_json` spells them, except that negative zero
/// is written as zero (§2.5): a non-finite float is `null`. `structs` receives each struct the
/// walk writes.
///
/// # Errors
///
/// [`CanonicalJsonError::Resource`] when `admission` refuses a value, and
/// [`CanonicalJsonError::Serialize`] when the value or the writer fails.
pub(crate) fn write_serde_json<T: Serialize + ?Sized>(
    admission: CanonicalAdmission<'_, '_>,
    structs: Option<&dyn Structs>,
    writer: &mut dyn std::io::Write,
    value: &T,
) -> Result<(), CanonicalJsonError> {
    write_json(admission, JsonFloats::SerdeJson, structs, writer, value)
}

fn write_json<T: Serialize + ?Sized>(
    admission: CanonicalAdmission<'_, '_>,
    floats: JsonFloats,
    structs: Option<&dyn Structs>,
    writer: &mut dyn std::io::Write,
    value: &T,
) -> Result<(), CanonicalJsonError> {
    write_json_erased(admission, floats, structs, writer, &|serializer| {
        value.serialize(serializer)
    })
}

fn write_json_erased(
    admission: CanonicalAdmission<'_, '_>,
    floats: JsonFloats,
    structs: Option<&dyn Structs>,
    writer: &mut dyn std::io::Write,
    write: &dyn Fn(
        cadmpeg_core::serde_erase::SerAdapter<'_>,
    ) -> Result<(), cadmpeg_core::serde_erase::SerError>,
) -> Result<(), CanonicalJsonError> {
    let bridge = structs.map(StructsBridge);
    let hooks: Option<&dyn JsonStructs> = bridge.as_ref().map(|item| {
        let dyn_ref: &dyn JsonStructs = item;
        dyn_ref
    });
    core_json::write_pretty_json_erased(admission.context(), floats, hooks, writer, write)
        .map_err(CanonicalJsonError::from)
}

/// Renders `value` as canonical pretty JSON text, refusing a non-finite float.
///
/// This is the one route every production write of an IR document takes: the
/// CADIR encoder, `CadIr::to_canonical_json`, and the decode sidecar. A
/// document written any other way would spell a non-finite float `null`.
pub fn to_canonical_json_string<T: Serialize + ?Sized>(
    value: &T,
) -> Result<String, CanonicalJsonError> {
    let mut bytes = Vec::new();
    write_canonical_json(CanonicalAdmission::unlimited(), &mut bytes, value)?;
    String::from_utf8(bytes)
        .map_err(|error| CanonicalJsonError::Serialize(serde_json::Error::custom(error)))
}

/// Renders `value` as canonical JSON text with no whitespace between tokens:
/// the text `serde_json::to_string` writes, with canonical floats. A
/// non-finite float is refused.
pub(crate) fn to_compact_canonical_json_string<T: Serialize + ?Sized>(
    value: &T,
) -> Result<String, CanonicalJsonError> {
    let mut compact = Compact::default();
    write_canonical_json(CanonicalAdmission::unlimited(), &mut compact, value)?;
    String::from_utf8(compact.bytes)
        .map_err(|error| CanonicalJsonError::Serialize(serde_json::Error::custom(error)))
}

/// Keeps the pretty JSON written to it without the spaces and line breaks
/// between tokens.
#[derive(Default)]
struct Compact {
    bytes: Vec<u8>,
    in_string: bool,
    escaped: bool,
}

impl std::io::Write for Compact {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        for &byte in buf {
            if self.in_string {
                if self.escaped {
                    self.escaped = false;
                } else if byte == b'\\' {
                    self.escaped = true;
                } else if byte == b'"' {
                    self.in_string = false;
                }
            } else if byte == b' ' || byte == b'\n' {
                continue;
            } else if byte == b'"' {
                self.in_string = true;
            }
            self.bytes.push(byte);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Returns the `serde_json::Value` that `serde_json::to_value` returns for
/// `value`.
///
/// The value is written as JSON text through the canonical writer's
/// serializer and read back. Floats pass as `serde_json::to_value` stores
/// them, except that negative zero is zero (§2.5): a non-finite float becomes
/// `null` and an `f32` widens to `f64`. An object key is plain text, also a key that
/// spells `serde_json`'s `RawValue` token.
///
/// # Errors
///
/// Returns the error of a `Serialize` implementation that fails, or that of
/// a map key that is not a string.
pub fn to_json_value<T: Serialize + ?Sized>(
    value: &T,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut bytes = Vec::new();
    write_json_value_bytes(&mut bytes, value)?;
    let text = String::from_utf8(bytes).map_err(serde_json::Error::custom)?;
    crate::native::replay::value(&text)
}

/// Writes `value` into `bytes` as JSON text with `to_json_value` float
/// semantics, reusing the canonical serializer's monomorphizations.
pub(crate) fn write_json_value_bytes<T: Serialize + ?Sized>(
    bytes: &mut Vec<u8>,
    value: &T,
) -> Result<(), serde_json::Error> {
    write_json(
        CanonicalAdmission::unlimited(),
        JsonFloats::Value,
        None,
        bytes,
        value,
    )
    .map_err(|error| match error {
        CanonicalJsonError::Serialize(error) => error,
        error => serde_json::Error::custom(error),
    })
}
