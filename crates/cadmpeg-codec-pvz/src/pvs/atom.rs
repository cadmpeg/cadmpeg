// SPDX-License-Identifier: Apache-2.0
//! ULEB128 values (§2.2), string atoms (§2.3), and the string pool (§3.5).

use std::rc::Rc;

use cadmpeg_core::decode::{index_from_u64, DecodeContext, View};
use cadmpeg_core::CodecError;

/// Payload bits one ULEB128 byte carries.
const ULEB_GROUP_BITS: u32 = 7;
/// Bit 7 of a ULEB128 byte: another byte follows.
const ULEB_CONTINUE: u8 = 0x80;
/// Bits of a ULEB128 value the reader holds.
const ULEB_VALUE_BITS: u32 = 64;
/// Smallest string-pool entry: a zero length.
const MIN_POOL_ENTRY_LEN: usize = 1;

/// Reads one ULEB128 value.
pub(super) fn uleb128(view: &mut View<'_>) -> Result<u64, CodecError> {
    let start = view.position();
    let mut value = 0_u64;
    let mut shift = 0_u32;
    loop {
        let byte = view.req_u8()?;
        let group = u64::from(byte & !ULEB_CONTINUE);
        let shifted = group
            .checked_shl(shift)
            .filter(|shifted| shift < ULEB_VALUE_BITS && shifted.checked_shr(shift) == Some(group));
        let Some(shifted) = shifted else {
            return Err(CodecError::malformed(format_args!(
                "ULEB128 value at offset {start} exceeds 64 bits"
            )));
        };
        value |= shifted;
        if byte & ULEB_CONTINUE == 0 {
            return Ok(value);
        }
        shift = shift.checked_add(ULEB_GROUP_BITS).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "ULEB128 value at offset {start} exceeds 64 bits"
            ))
        })?;
    }
}

/// Reads one ULEB128 value that counts or indexes in memory.
pub(super) fn uleb_index(view: &mut View<'_>) -> Result<usize, CodecError> {
    let start = view.position();
    let value = uleb128(view)?;
    index_from_u64(value).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "ULEB128 value {value} at offset {start} exceeds the address width"
        ))
    })
}

/// Reads `len` bytes of UTF-8 text.
fn utf8(view: &mut View<'_>, len: usize) -> Result<Rc<str>, CodecError> {
    let start = view.position();
    let bytes = view.req_take(len)?;
    std::str::from_utf8(bytes).map(Rc::from).map_err(|_| {
        CodecError::malformed(format_args!("string at offset {start} is not valid UTF-8"))
    })
}

/// The shared strings of one PVS member, in stored order. A reference to an
/// entry shares the entry; it does not copy it.
#[derive(Debug, Default)]
pub(crate) struct StringPool {
    entries: Vec<Rc<str>>,
}

impl StringPool {
    /// Reads the string-pool payload: a count, then that many
    /// length-prefixed strings that fill the payload exactly.
    pub(crate) fn read(ctx: &DecodeContext<'_>, mut payload: View<'_>) -> Result<Self, CodecError> {
        let declared = uleb128(&mut payload)?;
        let count = payload
            .counted(declared, MIN_POOL_ENTRY_LEN)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "string pool declares {declared} entries in {} bytes",
                    payload.remaining()
                ))
            })?;
        let mut entries = ctx.alloc_filled(count.get(), Rc::from(""), "PVZ string pool")?;
        for entry in &mut entries {
            let len = uleb_index(&mut payload)?;
            *entry = utf8(&mut payload, len)?;
        }
        if !payload.is_empty() {
            return Err(CodecError::malformed(format_args!(
                "string pool leaves {} bytes after its last entry",
                payload.remaining()
            )));
        }
        Ok(Self { entries })
    }

    /// Reads one string atom, resolving a reference against this pool.
    pub(crate) fn atom(&self, view: &mut View<'_>) -> Result<Rc<str>, CodecError> {
        let start = view.position();
        let code = uleb128(view)?;
        if code % 2 == 0 {
            let len = index_from_u64(code / 2).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "inline atom at offset {start} is longer than the address width"
                ))
            })?;
            return utf8(view, len);
        }
        index_from_u64(code / 2)
            .and_then(|index| self.entries.get(index))
            .cloned()
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "atom at offset {start} names string-pool entry {} of {}",
                    code / 2,
                    self.entries.len()
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use cadmpeg_core::decode::View;

    use super::{uleb128, StringPool};
    use crate::test_support::pvs::{inline_atom, pool_atom, string_pool_payload, uleb};

    #[test]
    fn uleb128_reads_seven_bits_per_byte_low_group_first() {
        for (bytes, value) in [
            (&[0x00][..], 0),
            (&[0x7f], 127),
            (&[0x80, 0x01], 128),
            (&[0xe5, 0x8e, 0x26], 624_485),
            (
                &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01],
                u64::MAX,
            ),
        ] {
            assert_eq!(uleb128(&mut View::over_retained(bytes)).unwrap(), value);
            assert_eq!(uleb(value), bytes);
        }
    }

    #[test]
    fn overlong_or_truncated_uleb128_is_refused() {
        for bytes in [
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02][..],
            &[0x80; 11],
            &[0x80],
        ] {
            assert!(
                uleb128(&mut View::over_retained(bytes)).is_err(),
                "{bytes:02x?}"
            );
        }
    }

    #[test]
    fn even_codes_are_inline_text_and_odd_codes_name_pool_entries() {
        let pool = StringPool::read(
            &cadmpeg_test_support::service_decode_context(),
            View::over_retained(&string_pool_payload(&["TOP.ASM", "", "BRACKET.PRT"])),
        )
        .unwrap();

        let mut bytes = inline_atom("wing");
        bytes.extend(inline_atom(""));
        bytes.extend(pool_atom(2));
        bytes.extend(pool_atom(0));
        let mut view = View::over_retained(&bytes);
        let atoms: Vec<String> = (0..4)
            .map(|_| pool.atom(&mut view).unwrap().to_string())
            .collect();
        assert_eq!(atoms, ["wing", "", "BRACKET.PRT", "TOP.ASM"]);
        assert!(view.is_empty());

        assert!(pool.atom(&mut View::over_retained(&pool_atom(3))).is_err());
        assert!(StringPool::default()
            .atom(&mut View::over_retained(&pool_atom(0)))
            .is_err());
        assert!(pool
            .atom(&mut View::over_retained(&[0x04, 0xff, 0xfe]))
            .is_err());
    }

    #[test]
    fn every_reference_to_a_pool_entry_shares_the_entry() {
        let long = "x".repeat(60_000);
        let pool = StringPool::read(
            &cadmpeg_test_support::service_decode_context(),
            View::over_retained(&string_pool_payload(&[long.as_str()])),
        )
        .unwrap();
        let references = 1000;
        let bytes: Vec<u8> = (0..references).flat_map(|_| pool_atom(0)).collect();
        let mut view = View::over_retained(&bytes);
        let first = pool.atom(&mut view).unwrap();
        assert_eq!(&*first, long);
        for _ in 1..references {
            assert!(Rc::ptr_eq(&first, &pool.atom(&mut view).unwrap()));
        }
        assert!(view.is_empty());
    }

    #[test]
    fn a_pool_that_does_not_fill_its_payload_is_refused() {
        let mut payload = string_pool_payload(&["A"]);
        payload.push(0);
        assert!(StringPool::read(
            &cadmpeg_test_support::service_decode_context(),
            View::over_retained(&payload)
        )
        .is_err());
        assert!(StringPool::read(
            &cadmpeg_test_support::service_decode_context(),
            View::over_retained(&[0x05, 0x00])
        )
        .is_err());
    }
}
