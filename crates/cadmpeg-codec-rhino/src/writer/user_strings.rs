// SPDX-License-Identifier: Apache-2.0
//! Object attribute user strings authored from neutral string attributes.

use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
use cadmpeg_ir::document::CadIr;

use super::{crc_chunk, crc_chunk_with_direct, long_chunk, utf16};

const TCODE_OBJECT_RECORD_ATTRIBUTES_USERDATA: u32 = 0x0200_0073;
const TCODE_ANONYMOUS_CHUNK: u32 = 0x4000_8000;
const TCODE_OPENNURBS_CLASS_USERDATA: u32 = 0x0002_7ffd;
const TCODE_OPENNURBS_CLASS_USERDATA_HEADER: u32 = 0x0002_fff9;
const USERDATA_ARCHIVE_VERSION: i32 = 60;

/// The neutral record id an attribute targets, for targets an object holds.
fn target_id(target: &AttributeTarget) -> Option<&str> {
    match target {
        AttributeTarget::Body(id) => Some(id.as_str()),
        AttributeTarget::Point(id) => Some(id.as_str()),
        AttributeTarget::Curve(id) => Some(id.as_str()),
        AttributeTarget::Surface(id) => Some(id.as_str()),
        AttributeTarget::Tessellation(id) => Some(id.as_str()),
        AttributeTarget::Record { id, .. } => Some(id.as_str()),
        _ => None,
    }
}

/// The user strings of one object: every single-string attribute that targets
/// a record in `records`, in arena order. The first `name` attribute of each
/// record whose value is the written object `name` is the object name itself.
pub(super) fn object_user_strings(
    ir: &CadIr,
    records: &[&str],
    name: Option<&str>,
) -> Vec<(String, String)> {
    let mut named = Vec::new();
    let mut strings: Vec<(String, String)> = Vec::new();
    for attribute in &ir.model.attributes {
        let Some(record) = target_id(&attribute.target).filter(|id| records.contains(id)) else {
            continue;
        };
        let [Some(AttributeValue::String(value))] = attribute.values.as_slice() else {
            continue;
        };
        if attribute.name == "name" && Some(value.as_str()) == name && !named.contains(&record) {
            named.push(record);
            continue;
        }
        let entry = (attribute.name.clone(), value.clone());
        if !strings.contains(&entry) {
            strings.push(entry);
        }
    }
    strings
}

fn anonymous_chunk(
    fields: &[u8],
    children: &[std::ops::Range<usize>],
) -> Result<Vec<u8>, crate::writer::WriteError> {
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend(0_i32.to_le_bytes());
    body.extend(fields);
    let children = children
        .iter()
        .map(|range| range.start + 8..range.end + 8)
        .collect::<Vec<_>>();
    let ranges = crate::chunks::direct_checksum_ranges(&(0..body.len()), &children)
        .map_err(|error| crate::writer::WriteError::Omit(error.to_string()))?;
    let mut direct = Vec::new();
    for range in &ranges {
        direct.extend_from_slice(&body[range]);
    }
    crc_chunk_with_direct(TCODE_ANONYMOUS_CHUNK, &body, &direct)
}

fn user_string_list(strings: &[(String, String)]) -> Result<Vec<u8>, crate::writer::WriteError> {
    let count = i32::try_from(strings.len())
        .map_err(|_| crate::writer::WriteError::Omit("user string count exceeds i32".into()))?;
    let mut fields = count.to_le_bytes().to_vec();
    let mut entries = Vec::new();
    for (key, value) in strings {
        let mut entry = utf16(key)?;
        entry.extend(utf16(value)?);
        let start = fields.len();
        fields.extend(anonymous_chunk(&entry, &[])?);
        entries.push(start..fields.len());
    }
    let payload = crc_chunk(TCODE_ANONYMOUS_CHUNK, &anonymous_chunk(&fields, &entries)?)?;
    let class = crate::objects::USER_STRING_LIST.to_wire();
    let identity: Vec<u8> = (0..16)
        .flat_map(|index| if index % 5 == 0 { 1.0_f64 } else { 0.0 }.to_le_bytes())
        .collect();
    let header_body = [
        class.to_vec(),
        class.to_vec(),
        0_i32.to_le_bytes().to_vec(),
        identity,
        [0; 16].to_vec(),
        vec![0],
        USERDATA_ARCHIVE_VERSION.to_le_bytes().to_vec(),
        0_u32.to_le_bytes().to_vec(),
    ]
    .concat();
    let header = crc_chunk(TCODE_OPENNURBS_CLASS_USERDATA_HEADER, &header_body)?;
    let header_len = header.len();
    let mut wrapper = vec![0x22];
    wrapper.extend(header);
    wrapper.extend(payload);
    let children = [1..1 + header_len, 1 + header_len..wrapper.len()];
    let ranges = crate::chunks::direct_checksum_ranges(&(0..wrapper.len()), &children)
        .map_err(|error| crate::writer::WriteError::Omit(error.to_string()))?;
    let mut hasher = crc32fast::Hasher::new();
    for range in &ranges {
        hasher.update(&wrapper[range]);
    }
    let checksum = hasher.finalize();
    wrapper.extend(checksum.to_le_bytes());
    long_chunk(TCODE_OPENNURBS_CLASS_USERDATA, &wrapper)
}

/// The attributes userdata chunk that carries `strings`, absent when empty.
pub(super) fn attributes_userdata(
    strings: &[(String, String)],
) -> Result<Option<Vec<u8>>, crate::writer::WriteError> {
    if strings.is_empty() {
        return Ok(None);
    }
    Ok(Some(long_chunk(
        TCODE_OBJECT_RECORD_ATTRIBUTES_USERDATA,
        &user_string_list(strings)?,
    )?))
}
