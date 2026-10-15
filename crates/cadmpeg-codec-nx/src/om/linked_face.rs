// SPDX-License-Identifier: Apache-2.0
//! Named WAVE cache prefix and exact bounded payload with an unassigned tail.

use super::core_reader::Reader;
use super::operation_record::OperationPayload;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::bytes::NativeBytes;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU8;

const CLASSES: [&str; 4] = [
    "UGS::ModlFeature::ExtractFaceParms",
    "UGS::CM_track_map",
    "UGS::ModlUtils::CachedWaveLinkData",
    "UGS::ModlUtils::CachedAttributes",
];
const SOURCE_KEYS: [&str; 9] = [
    "Name",
    "Type",
    "Parent_Part",
    "Last_Modified_By",
    "Modified_Date",
    "Handle",
    "PIO_Handle",
    "Defining_Feature",
    "WV_Part_Spec",
];
const WAVE_KEYS: [&str; 9] = [
    "Rules",
    "Parent_Part",
    "Description",
    "Interpart_Link_Type",
    "Assembly_Context",
    "Context_Time_Stamp",
    "Arrangement",
    "Cached_Referenced_Entities",
    "Cached_Referenced_Time_Stamps",
];
pub(crate) type CacheTables<'a> = [[&'a str; 9]; 3];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct LinkedFacePayload(
    #[serde(deserialize_with = "deserialize_payload")] NativeBytes<Vec<u8>>,
);

fn deserialize_payload<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<NativeBytes<Vec<u8>>, D::Error> {
    let bytes = NativeBytes::<Vec<u8>>::deserialize(d)?;
    read(&bytes).ok_or_else(|| {
        serde::de::Error::custom(
            "linked_face_payload: invalid named cache prefix in bounded payload",
        )
    })?;
    Ok(bytes)
}
impl LinkedFacePayload {
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }
    pub(crate) fn tables(&self) -> Option<CacheTables<'_>> {
        Some(read(&self.0)?.tables)
    }
}
struct CacheView<'a> {
    selectors: [u8; 4],
    tables: CacheTables<'a>,
}
fn selector(r: &mut Reader<'_>) -> Option<u8> {
    NonZeroU8::new(r.byte()?).map(NonZeroU8::get)
}
fn text<'a>(r: &mut Reader<'a>) -> Option<&'a str> {
    r.fixed(b"\x03")?;
    let length = usize::from(r.byte()?).checked_sub(2)?;
    let value = std::str::from_utf8(r.take(length)?).ok()?;
    (!value.contains('\0')).then_some(())?;
    r.fixed(b"\0")?;
    Some(value)
}
fn table<'a>(r: &mut Reader<'a>, class: u8, keys: [&str; 9]) -> Option<[&'a str; 9]> {
    r.fixed(&[class, 1, 10])?;
    for expected in keys {
        (text(r)? == expected).then_some(())?;
    }
    r.fixed(b"\x01\x0a")?;
    let mut values = [""; 9];
    for value in &mut values {
        *value = text(r)?;
    }
    Some(values)
}
fn read(bytes: &[u8]) -> Option<CacheView<'_>> {
    let mut r = Reader { bytes, at: 0 };
    let owner = selector(&mut r)?;
    r.fixed(b"\0\0\x01\0")?;
    for _ in 0..4 {
        r.fixed(b"\x01\0\0\0\0")?;
    }
    r.fixed(b"\0\0\0\0\0\0\x01\x02")?;
    let track = selector(&mut r)?;
    r.fixed(b"\xff")?;
    r.feature()?;
    r.fixed(b"\x01")?;
    let entries = r.byte()?.checked_sub(1).filter(|n| *n > 0)?;
    for _ in 0..entries {
        r.fixed(&[track, 255])?;
        r.feature()?;
    }
    r.fixed(b"\xff")?;
    let cache = selector(&mut r)?;
    for count in [2, 2, 4] {
        r.fixed(b"\x01\x03")?;
        for _ in 0..count {
            text(&mut r)?;
        }
    }
    r.fixed(b"\0\x01\x03")?;
    let table_class = NonZeroU8::new(*bytes.get(r.at)?)?.get();
    let body = table(&mut r, table_class, SOURCE_KEYS)?;
    let face = table(&mut r, table_class, SOURCE_KEYS)?;
    let wave = table(&mut r, table_class, WAVE_KEYS)?;
    text(&mut r)?.is_empty().then_some(())?;
    Some(CacheView {
        selectors: [owner, track, cache, table_class],
        tables: [body, face, wave],
    })
}

pub(crate) fn payload_for_decode(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
    classes: &[super::TypeDefinition<'_>],
) -> Result<Option<LinkedFacePayload>, CodecError> {
    if record.name() != "LINKED_FACE" {
        return Ok(None);
    }
    ctx.charge_work(
        u64_from_index(record.payload().len()),
        "NX linked-face named cache",
    )?;
    let Some(view) = read(record.payload()) else {
        return Ok(None);
    };
    if !view
        .selectors
        .into_iter()
        .zip(CLASSES)
        .all(|(index, expected)| {
            classes
                .get(usize::from(index) - 1)
                .is_some_and(|class| class.name == expected)
        })
    {
        return Ok(None);
    }
    Ok(Some(LinkedFacePayload(
        ctx.copy_slice(record.payload(), "NX linked-face payload and named cache")?
            .into(),
    )))
}

#[cfg(test)]
mod tests;
