// SPDX-License-Identifier: Apache-2.0
//! Typed Inventor kernel-carrier selection and envelope framing.

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use crate::rse::{
    DocumentKind, RecordFrameState, SegmentBulkState, SegmentDescriptor, SegmentKind,
};

const KERNEL_RECORD_TYPE_ID: [u8; 16] = [
    0x5c, 0x59, 0x45, 0xf6, 0xd5, 0x11, 0x33, 0x13, 0x10, 0x00, 0x60, 0xa6, 0xbb, 0xa6, 0x47, 0xb5,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KernelFamily {
    Asm,
    Acis,
}

impl KernelFamily {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Asm => "asm",
            Self::Acis => "acis",
        }
    }
}

#[derive(Debug)]
pub(crate) struct ActiveCarrier<'a> {
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) segment_version_major: u8,
    pub(crate) family: KernelFamily,
    pub(crate) header_state: u32,
    pub(crate) header_kind: u16,
    pub(crate) header_value: u32,
    pub(crate) schema: u32,
    pub(crate) carrier_offset: u64,
    pub(crate) bytes: View<'a>,
    pub(crate) selected_key: u32,
    pub(crate) enabled: bool,
    pub(crate) delta_state: i32,
    pub(crate) history_reference: u32,
}

#[derive(Debug)]
pub(crate) enum ActiveCarrierState<'a> {
    NotApplicable,
    NotExpanded,
    Selected(ActiveCarrier<'a>),
    Unavailable(String),
}

pub(crate) fn select_active_carrier<'a>(
    segments: &[SegmentDescriptor<'a>],
    document_kind: &DocumentKind,
) -> ActiveCarrierState<'a> {
    if !matches!(document_kind, DocumentKind::Part) {
        return ActiveCarrierState::NotApplicable;
    }
    let brep_segments = segments
        .iter()
        .filter(|segment| matches!(segment.kind, SegmentKind::PmBRep))
        .collect::<Vec<_>>();
    let [segment] = brep_segments.as_slice() else {
        return ActiveCarrierState::Unavailable(format!(
            "part document has {} PmBRep segments; expected one",
            brep_segments.len()
        ));
    };
    let SegmentBulkState::Framed(bulk) = &segment.bulk else {
        return ActiveCarrierState::Unavailable("PmBRep bulk stream is unavailable".into());
    };
    let table = match &bulk.records {
        RecordFrameState::NotExpanded => return ActiveCarrierState::NotExpanded,
        RecordFrameState::Framed(table) => table,
        RecordFrameState::Unavailable(_) => {
            return ActiveCarrierState::Unavailable("PmBRep record table is unavailable".into());
        }
    };
    let carriers = table
        .records
        .iter()
        .filter(|record| record.type_id == KERNEL_RECORD_TYPE_ID)
        .collect::<Vec<_>>();
    let [record] = carriers.as_slice() else {
        return ActiveCarrierState::Unavailable(format!(
            "PmBRep contains {} typed kernel-carrier records; expected one",
            carriers.len()
        ));
    };
    let Some(version) = segment.registry_version_major else {
        return ActiveCarrierState::Unavailable(
            "PmBRep segment version is unavailable from the registry".into(),
        );
    };
    match parse_carrier(
        record.payload,
        segment.pair.token.as_str(),
        record.ordinal,
        record.payload_offset,
        version,
    ) {
        Ok(carrier) => ActiveCarrierState::Selected(carrier),
        Err(error) => ActiveCarrierState::Unavailable(error.to_string()),
    }
}

fn parse_carrier<'a>(
    payload: View<'a>,
    segment_token: &str,
    record_ordinal: u32,
    record_payload_offset: u64,
    segment_version_major: u8,
) -> Result<ActiveCarrier<'a>, CodecError> {
    let bytes = payload.window();
    let footer_len = match segment_version_major {
        15..=22 => 17,
        23..=u8::MAX => 18,
        value => {
            return Err(CodecError::NotImplemented(format!(
                "Inventor kernel-carrier envelope for segment version {value} is not implemented"
            )));
        }
    };
    if bytes.len() < 14 + footer_len {
        return Err(CodecError::Malformed(
            "truncated Inventor kernel-carrier record".into(),
        ));
    }
    let header_state = read_u32(bytes, 0, "carrier header state")?;
    let header_kind = read_u16(bytes, 4, "carrier header kind")?;
    let header_value = read_u32(bytes, 6, "carrier header value")?;
    let schema = read_u32(bytes, 10, "carrier schema")?;
    let carrier_end = bytes.len() - footer_len;
    let family = if bytes[14..carrier_end].starts_with(b"ASM BinaryFile") {
        KernelFamily::Asm
    } else if bytes[14..carrier_end].starts_with(b"ACIS BinaryFile") {
        KernelFamily::Acis
    } else {
        return Err(CodecError::Malformed(
            "typed Inventor kernel carrier has no ASM or ACIS signature at its payload start"
                .into(),
        ));
    };
    let carrier = payload
        .child(payload.start() + 14, payload.start() + carrier_end)
        .ok_or_else(|| CodecError::Malformed("Inventor kernel-carrier range is invalid".into()))?;
    let mut offset = carrier_end;
    let selected_key = read_u32(bytes, offset, "carrier selected key")?;
    offset += 4;
    let enabled = match bytes[offset] {
        0 => false,
        1 => true,
        value => {
            return Err(CodecError::Malformed(format!(
                "Inventor kernel-carrier enabled flag is {value}"
            )));
        }
    };
    offset += 1;
    let delta_state = read_i32(bytes, offset, "carrier delta state")?;
    offset += 4;
    if segment_version_major >= 23 {
        if bytes[offset] != 0 {
            return Err(CodecError::Malformed(
                "Inventor kernel-carrier versioned padding is nonzero".into(),
            ));
        }
        offset += 1;
    }
    let history_reference = read_u32(bytes, offset, "carrier history reference")?;
    offset += 4;
    let terminator = read_u32(bytes, offset, "carrier terminator")?;
    offset += 4;
    if terminator != u32::MAX || offset != bytes.len() {
        return Err(CodecError::Malformed(
            "Inventor kernel-carrier footer is not exactly exhausted".into(),
        ));
    }
    Ok(ActiveCarrier {
        segment_token: segment_token.into(),
        record_ordinal,
        segment_version_major,
        family,
        header_state,
        header_kind,
        header_value,
        schema,
        carrier_offset: record_payload_offset + 14,
        bytes: carrier,
        selected_key,
        enabled,
        delta_state,
        history_reference,
    })
}

fn read_u16(bytes: &[u8], offset: usize, name: &str) -> Result<u16, CodecError> {
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..offset.saturating_add(2))
            .ok_or_else(|| CodecError::Malformed(format!("truncated Inventor {name}")))?
            .try_into()
            .expect("two-byte slice"),
    ))
}

fn read_u32(bytes: &[u8], offset: usize, name: &str) -> Result<u32, CodecError> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset.saturating_add(4))
            .ok_or_else(|| CodecError::Malformed(format!("truncated Inventor {name}")))?
            .try_into()
            .expect("four-byte slice"),
    ))
}

fn read_i32(bytes: &[u8], offset: usize, name: &str) -> Result<i32, CodecError> {
    Ok(i32::from_le_bytes(
        bytes
            .get(offset..offset.saturating_add(4))
            .ok_or_else(|| CodecError::Malformed(format!("truncated Inventor {name}")))?
            .try_into()
            .expect("four-byte slice"),
    ))
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    use super::*;

    #[test]
    fn typed_carrier_envelope_selects_family_and_exact_footer() {
        let bytes = carrier_fixture(b"ASM BinaryFile4 synthetic", 18);
        with_view(&bytes, |view| {
            let carrier = parse_carrier(view, "token", 7, 100, 18).expect("carrier parses");
            assert_eq!(carrier.family, KernelFamily::Asm);
            assert_eq!(carrier.bytes.window(), b"ASM BinaryFile4 synthetic");
            assert_eq!(carrier.record_ordinal, 7);
            assert_eq!(carrier.carrier_offset, 114);
        });
        let mut malformed = bytes;
        *malformed.last_mut().expect("footer byte") = 0;
        with_view(&malformed, |view| {
            assert!(parse_carrier(view, "token", 7, 100, 18).is_err());
        });
    }

    fn carrier_fixture(carrier: &[u8], version: u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(&4_u32.to_le_bytes());
        bytes.extend_from_slice(carrier);
        bytes.extend_from_slice(&5_u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&(-1_i32).to_le_bytes());
        if version >= 23 {
            bytes.push(0);
        }
        bytes.extend_from_slice(&6_u32.to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes
    }

    fn with_view(bytes: &[u8], test: impl FnOnce(View<'_>)) {
        let arena = DecodeArena::new();
        let (_, view) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default())
            .expect("synthetic carrier fits policy");
        test(view);
    }
}
