// SPDX-License-Identifier: Apache-2.0
//! The tagged values of a native body slice (`SolidCraft` `SCBREP05`).
//!
//! A slice is the eight-byte signature, then one tagged value. A tag byte
//! selects null (0), false (1), true (2), a signed (3) or unsigned (4)
//! 64-bit integer, a binary64 (5), a UTF-8 string (6), an array (7) or an
//! object (8). Numbers are eight little-endian bytes; strings and
//! collections carry a 32-bit little-endian length, and object keys are
//! length-prefixed UTF-8 in strictly increasing byte order. The reader
//! refuses an unknown tag, a non-finite number, a duplicate or unsorted key,
//! a value past the slice limits, and trailing bytes.

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use serde_json::{Map, Number, Value};

/// The signature of a native B-rep body slice.
pub(super) const SIGNATURE: &[u8; 8] = b"SCBREP05";
/// Deepest nesting of one slice.
const MAX_DEPTH: usize = 48;
/// Most values in one slice.
const MAX_VALUES: usize = 20_000_000;
/// Most entries in one collection.
const MAX_ITEMS: usize = 1_000_000;
/// Most bytes in one string.
const MAX_STRING: usize = 1 << 20;

/// Tag: null.
const TAG_NULL: u8 = 0;
/// Tag: false.
const TAG_FALSE: u8 = 1;
/// Tag: true.
const TAG_TRUE: u8 = 2;
/// Tag: signed 64-bit integer.
const TAG_SIGNED: u8 = 3;
/// Tag: unsigned 64-bit integer.
const TAG_UNSIGNED: u8 = 4;
/// Tag: binary64.
const TAG_FLOAT: u8 = 5;
/// Tag: UTF-8 string.
const TAG_STRING: u8 = 6;
/// Tag: array.
const TAG_ARRAY: u8 = 7;
/// Tag: object.
const TAG_OBJECT: u8 = 8;

/// Reads the tagged value of a body slice that starts with [`SIGNATURE`].
pub(super) fn read(slice: View<'_>) -> Result<Value, CodecError> {
    let mut view = slice;
    if view.take(SIGNATURE.len()) != Some(SIGNATURE.as_slice()) {
        return Err(CodecError::malformed(
            "body slice does not start with SCBREP05",
        ));
    }
    let mut reader = Reader { view, values: 0 };
    let value = reader.value(0)?;
    if !reader.view.is_empty() {
        return Err(CodecError::malformed(
            "body slice has bytes after its tagged value",
        ));
    }
    Ok(value)
}

struct Reader<'a> {
    view: View<'a>,
    values: usize,
}

fn truncated() -> CodecError {
    CodecError::malformed("body slice ends inside a tagged value")
}

impl Reader<'_> {
    fn length(&mut self, limit: usize, what: &str) -> Result<usize, CodecError> {
        let length = self.view.u32_le().ok_or_else(truncated)?;
        let length = usize::try_from(length)
            .map_err(|_| CodecError::malformed(format_args!("body slice {what} length")))?;
        // Every entry takes at least one byte, so a count past the bytes left
        // is refused before anything is allocated for it.
        if length > limit || length > self.view.remaining() {
            return Err(CodecError::malformed(format_args!(
                "body slice {what} of {length} entries exceeds its limit or the slice"
            )));
        }
        Ok(length)
    }

    fn string(&mut self) -> Result<String, CodecError> {
        let length = self.length(MAX_STRING, "string")?;
        let bytes = self.view.take(length).ok_or_else(truncated)?;
        String::from_utf8(bytes.to_vec())
            .map_err(|_| CodecError::malformed("body slice string is not UTF-8"))
    }

    fn value(&mut self, depth: usize) -> Result<Value, CodecError> {
        self.values = self
            .values
            .checked_add(1)
            .filter(|count| *count <= MAX_VALUES)
            .ok_or_else(|| CodecError::malformed("body slice holds too many values"))?;
        let tag = self.view.u8().ok_or_else(truncated)?;
        Ok(match tag {
            TAG_NULL => Value::Null,
            TAG_FALSE => Value::Bool(false),
            TAG_TRUE => Value::Bool(true),
            TAG_SIGNED => Value::Number(self.view.i64_le().ok_or_else(truncated)?.into()),
            TAG_UNSIGNED => Value::Number(self.view.u64_le().ok_or_else(truncated)?.into()),
            TAG_FLOAT => {
                let number = self.view.f64_le().ok_or_else(truncated)?;
                Value::Number(
                    Number::from_f64(number)
                        .ok_or_else(|| CodecError::malformed("body slice number is not finite"))?,
                )
            }
            TAG_STRING => Value::String(self.string()?),
            TAG_ARRAY | TAG_OBJECT => {
                if depth >= MAX_DEPTH {
                    return Err(CodecError::malformed("body slice nests too deeply"));
                }
                if tag == TAG_ARRAY {
                    let count = self.length(MAX_ITEMS, "array")?;
                    let mut items = Vec::with_capacity(count);
                    for _ in 0..count {
                        items.push(self.value(depth + 1)?);
                    }
                    Value::Array(items)
                } else {
                    let count = self.length(MAX_ITEMS, "object")?;
                    let mut members = Map::new();
                    let mut last: Option<String> = None;
                    for _ in 0..count {
                        let key = self.string()?;
                        if last.as_ref().is_some_and(|last| *last >= key) {
                            return Err(CodecError::malformed(
                                "body slice object keys are duplicate or unsorted",
                            ));
                        }
                        let value = self.value(depth + 1)?;
                        last = Some(key.clone());
                        members.insert(key, value);
                    }
                    Value::Object(members)
                }
            }
            other => {
                return Err(CodecError::malformed(format_args!(
                    "body slice tag {other} is unknown"
                )))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slice(body: &[u8]) -> Vec<u8> {
        let mut bytes = SIGNATURE.to_vec();
        bytes.extend_from_slice(body);
        bytes
    }

    fn key(name: &str) -> Vec<u8> {
        let mut bytes = u32::try_from(name.len()).unwrap().to_le_bytes().to_vec();
        bytes.extend_from_slice(name.as_bytes());
        bytes
    }

    #[test]
    fn an_object_of_every_scalar_reads_back() {
        let mut body = vec![TAG_OBJECT, 4, 0, 0, 0];
        body.extend(key("a"));
        body.push(TAG_FLOAT);
        body.extend(1.5_f64.to_le_bytes());
        body.extend(key("b"));
        body.extend([TAG_ARRAY, 2, 0, 0, 0, TAG_TRUE, TAG_NULL]);
        body.extend(key("c"));
        body.push(TAG_SIGNED);
        body.extend((-3_i64).to_le_bytes());
        body.extend(key("d"));
        body.push(TAG_STRING);
        body.extend(key("x"));
        let bytes = slice(&body);
        let value = read(View::over_retained(&bytes)).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"a": 1.5, "b": [true, null], "c": -3, "d": "x"})
        );
    }

    #[test]
    fn malformed_slices_are_refused() {
        let unsorted = {
            let mut body = vec![TAG_OBJECT, 2, 0, 0, 0];
            body.extend(key("b"));
            body.push(TAG_NULL);
            body.extend(key("a"));
            body.push(TAG_NULL);
            body
        };
        let mut nan = vec![TAG_FLOAT];
        nan.extend(f64::NAN.to_le_bytes());
        let cases: [Vec<u8>; 5] = [
            vec![9],
            vec![TAG_NULL, TAG_NULL],
            vec![TAG_ARRAY, 0xff, 0xff, 0xff, 0x7f],
            unsorted,
            nan,
        ];
        for body in cases {
            let bytes = slice(&body);
            assert!(read(View::over_retained(&bytes)).is_err(), "{body:?}");
        }
        assert!(read(View::over_retained(b"SCMESH01\x00")).is_err());
    }

    #[test]
    fn nesting_past_the_limit_is_refused() {
        let mut body = [TAG_ARRAY, 1, 0, 0, 0].repeat(MAX_DEPTH + 1);
        body.push(TAG_NULL);
        let bytes = slice(&body);
        assert!(read(View::over_retained(&bytes)).is_err());
    }
}
