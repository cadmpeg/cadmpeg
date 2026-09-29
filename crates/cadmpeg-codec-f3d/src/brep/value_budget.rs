// SPDX-License-Identifier: Apache-2.0
//! Preflight charges for the dynamic value trees used by BREP graph operations.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::Serialize;
use std::io::{self, Write};

#[derive(Default)]
struct JsonShape {
    bytes: u64,
    items: u64,
    in_string: bool,
    escaped: bool,
    overflowed: bool,
}

impl Write for JsonShape {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let Ok(count) = u64::try_from(bytes.len()) else {
            self.overflowed = true;
            return Err(io::Error::other("BREP value byte count overflows"));
        };
        let Some(total) = self.bytes.checked_add(count) else {
            self.overflowed = true;
            return Err(io::Error::other("BREP value byte count overflows"));
        };
        self.bytes = total;
        for byte in bytes {
            if self.in_string {
                if self.escaped {
                    self.escaped = false;
                } else if *byte == b'\\' {
                    self.escaped = true;
                } else if *byte == b'"' {
                    self.in_string = false;
                }
            } else if *byte == b'"' {
                self.in_string = true;
            } else if matches!(*byte, b'{' | b'[' | b',' | b':') {
                let Some(total) = self.items.checked_add(1) else {
                    self.overflowed = true;
                    return Err(io::Error::other("BREP value item count overflows"));
                };
                self.items = total;
            }
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn reserve_projection<'a>(
    ctx: &'a DecodeContext<'_>,
    value: &impl Serialize,
    operation: &'static str,
) -> Result<cadmpeg_core::decode::ScopedReservation<'a>, CodecError> {
    let mut shape = JsonShape::default();
    if let Err(error) = serde_json::to_writer(&mut shape, value) {
        if shape.overflowed {
            return Err(ctx.refuse_codec_limit(operation, 0, u64::MAX));
        }
        return Err(CodecError::malformed(format_args!(
            "F3D BREP value preflight failed: {error}"
        )));
    }
    ctx.charge_collection_items(shape.items, operation)?;
    ctx.reserve_scoped(shape.bytes, operation)
}

#[cfg(test)]
pub(super) fn projection_items(value: &impl Serialize) -> u64 {
    let mut shape = JsonShape::default();
    serde_json::to_writer(&mut shape, value).expect("test value shape");
    shape.items
}

#[cfg(test)]
pub(super) fn projection_bytes(value: &impl Serialize) -> u64 {
    let mut shape = JsonShape::default();
    serde_json::to_writer(&mut shape, value).expect("test value shape");
    shape.bytes
}
