// SPDX-License-Identifier: Apache-2.0
//! Typed Inventor kernel-carrier selection and envelope framing.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use cadmpeg_asm::brep::{decode_with_header, AsmBrep, DecodePurpose};
use cadmpeg_asm::ids::IdFormat;
use cadmpeg_asm::kernel_header::KernelHeader;
use cadmpeg_asm::sab;
use cadmpeg_asm::{acis_header, asm_header};

use crate::layout::kernel_carrier_header as carrier_header;
use crate::rse::{
    DocumentKind, RecordFrameState, SegmentBulkState, SegmentDescriptor, SegmentKind,
};

const KERNEL_RECORD_TYPE_ID: [u8; 16] = [
    0x5c, 0x59, 0x45, 0xf6, 0xd5, 0x11, 0x33, 0x13, 0x10, 0x00, 0x60, 0xa6, 0xbb, 0xa6, 0x47, 0xb5,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
    pub(crate) header: Result<Box<KernelHeader>, String>,
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

pub(crate) struct DecodedKernelCarrier {
    pub(crate) header: KernelHeader,
    pub(crate) brep: AsmBrep,
}

fn parse_kernel_header(family: KernelFamily, bytes: &[u8]) -> Result<KernelHeader, String> {
    match family {
        KernelFamily::Asm => asm_header::parse(bytes)
            .ok_or_else(|| "Inventor ASM carrier has no parseable header".into()),
        KernelFamily::Acis => acis_header::parse(bytes)
            .ok_or_else(|| "Inventor ACIS carrier has no parseable header".into()),
    }
}

pub(crate) fn decode_kernel_carrier(
    ctx: &DecodeContext<'_>,
    carrier: &ActiveCarrier<'_>,
    header: &KernelHeader,
) -> Result<DecodedKernelCarrier, CodecError> {
    let bytes = carrier.bytes.window();
    let (start, solved_limit) = match carrier.family {
        KernelFamily::Asm => (
            asm_header::record_stream_start_with_header(bytes, header).ok_or_else(|| {
                CodecError::Malformed("Inventor ASM carrier has no record stream".into())
            })?,
            asm_header::solved_record_limit_with_header(bytes, header),
        ),
        KernelFamily::Acis => {
            // Every save-format band frames and decodes the same way. The band
            // moves the carrier's `acis:` admission and its
            // source.kernel-dialect-unverified mark (`dialect::kernel_layer`), never
            // whether the records are read.
            (
                acis_header::record_stream_start_with_header(bytes, header).ok_or_else(|| {
                    CodecError::Malformed("Inventor ACIS carrier has no record stream".into())
                })?,
                acis_header::solved_record_limit_with_header(bytes, header),
            )
        }
    };
    let width = usize::from(header.width);
    let records = match solved_limit {
        Some(limit) => sab::frame(bytes, start, limit, width),
        None => sab::frame_history(bytes, start, bytes.len(), width),
    }
    .map_err(|error| {
        CodecError::malformed(format_args!(
            "Inventor {} SAB framing failed: {error}",
            carrier.family.label()
        ))
    })?;
    ctx.charge_collection_items(records.len() as u64, "frame Inventor kernel records")?;
    let stream = format!(
        "RSeStorage/B{}:record:{}",
        carrier.segment_token, carrier.record_ordinal
    );
    let brep = decode_with_header(
        &records,
        bytes,
        Some(header.clone()),
        &stream,
        IdFormat("inventor"),
        DecodePurpose::Model,
    );
    Ok(DecodedKernelCarrier {
        header: header.clone(),
        brep,
    })
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
        0..=22 => 17,
        23..=u8::MAX => 18,
    };
    if bytes.len() < carrier_header::LEN + footer_len {
        return Err(CodecError::Malformed(
            "truncated Inventor kernel-carrier record".into(),
        ));
    }
    let header_state = read_u32(bytes, carrier_header::HEADER_STATE, "carrier header state")?;
    let header_kind = read_u16(bytes, carrier_header::HEADER_KIND, "carrier header kind")?;
    let header_value = read_u32(bytes, carrier_header::HEADER_VALUE, "carrier header value")?;
    let schema = read_u32(bytes, carrier_header::SCHEMA, "carrier schema")?;
    let carrier_end = bytes.len() - footer_len;
    let family = if bytes[carrier_header::LEN..carrier_end].starts_with(b"ASM BinaryFile") {
        KernelFamily::Asm
    } else if bytes[carrier_header::LEN..carrier_end].starts_with(b"ACIS BinaryFile") {
        KernelFamily::Acis
    } else {
        return Err(CodecError::Malformed(
            "typed Inventor kernel carrier has no ASM or ACIS signature at its payload start"
                .into(),
        ));
    };
    let carrier = payload
        .child(
            payload.start() + carrier_header::LEN,
            payload.start() + carrier_end,
        )
        .ok_or_else(|| CodecError::Malformed("Inventor kernel-carrier range is invalid".into()))?;
    let header = parse_kernel_header(family, carrier.window()).map(Box::new);
    let mut offset = carrier_end;
    let selected_key = read_u32(bytes, offset, "carrier selected key")?;
    offset += 4;
    let enabled = match bytes[offset] {
        0 => false,
        1 => true,
        value => {
            return Err(CodecError::malformed(format_args!(
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
        carrier_offset: record_payload_offset + carrier_header::LEN as u64,
        bytes: carrier,
        header,
        selected_key,
        enabled,
        delta_state,
        history_reference,
    })
}

fn read_u16(bytes: &[u8], offset: usize, name: &str) -> Result<u16, CodecError> {
    View::u16_le_at(bytes, offset)
        .ok_or_else(|| CodecError::malformed(format_args!("truncated Inventor {name}")))
}

fn read_u32(bytes: &[u8], offset: usize, name: &str) -> Result<u32, CodecError> {
    View::u32_le_at(bytes, offset)
        .ok_or_else(|| CodecError::malformed(format_args!("truncated Inventor {name}")))
}

fn read_i32(bytes: &[u8], offset: usize, name: &str) -> Result<i32, CodecError> {
    View::i32_le_at(bytes, offset)
        .ok_or_else(|| CodecError::malformed(format_args!("truncated Inventor {name}")))
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    use super::*;
    use crate::test_support::acis_sphere_kernel_stream;

    fn decode_test_carrier(
        ctx: &DecodeContext<'_>,
        carrier: &ActiveCarrier<'_>,
    ) -> Result<DecodedKernelCarrier, CodecError> {
        let header = carrier
            .header
            .as_ref()
            .map_err(|detail| CodecError::Malformed(detail.clone()))?;
        decode_kernel_carrier(ctx, carrier, header)
    }

    #[test]
    fn typed_carrier_envelope_selects_family_and_exact_footer() {
        let bytes = carrier_fixture(b"ASM BinaryFile4 synthetic", 18);
        with_view(&bytes, |view| {
            assert_eq!(
                u32::from_le_bytes(bytes[0..4].try_into().expect("planted header state")),
                1
            );
            assert_eq!(
                u16::from_le_bytes(bytes[4..6].try_into().expect("planted header kind")),
                2
            );
            assert_eq!(
                u32::from_le_bytes(bytes[6..10].try_into().expect("planted header value")),
                3
            );
            assert_eq!(
                u32::from_le_bytes(bytes[10..14].try_into().expect("planted schema")),
                4
            );
            let carrier = parse_carrier(view, "token", 7, 100, 18).expect("carrier parses");
            assert_eq!(carrier.header_state, 1);
            assert_eq!(carrier.header_kind, 2);
            assert_eq!(carrier.header_value, 3);
            assert_eq!(carrier.schema, 4);
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

    #[test]
    fn asm_carrier_uses_header_boundary_and_shared_decoder() {
        let asm = empty_asm_fixture();
        let bytes = carrier_fixture(&asm, 23);
        let arena = DecodeArena::new();
        let (ctx, view) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic carrier fits policy");
        let carrier = parse_carrier(view, "token", 7, 100, 23).expect("carrier parses");
        let decoded = decode_test_carrier(&ctx, &carrier).expect("ASM carrier decodes");

        assert_eq!(decoded.header.width, 4);
        assert_eq!(decoded.header.save_format_version, Some(700));
        assert_eq!(decoded.header.product_family.as_deref(), Some("Inventor"));
        assert!(decoded.brep.bodies.is_empty());
        assert!(decoded.brep.unknowns.is_empty());
    }

    #[test]
    fn acis_carrier_uses_32_bit_header_and_shared_decoder() {
        let acis = empty_acis_fixture();
        let bytes = carrier_fixture(&acis, 17);
        let arena = DecodeArena::new();
        let (ctx, view) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic carrier fits policy");
        let carrier = parse_carrier(view, "token", 7, 100, 17).expect("carrier parses");
        let decoded = decode_test_carrier(&ctx, &carrier).expect("ACIS carrier decodes");

        assert_eq!(carrier.family, KernelFamily::Acis);
        assert_eq!(decoded.header.width, 4);
        assert_eq!(decoded.header.save_format_version, Some(21_800));
        assert_eq!(decoded.header.product_family.as_deref(), Some("Inventor"));
        assert!(decoded.brep.bodies.is_empty());
        assert!(decoded.brep.unknowns.is_empty());
    }

    #[test]
    fn an_acis_carrier_outside_the_verified_band_reads_the_same_records() {
        // The save format bands the label the decode carries, never whether the
        // carrier is read. Proved on records, not on an empty stream: the same
        // sphere body decodes at 70000 as at 21800.
        let decode = |save_format_version: u32| {
            let bytes = carrier_fixture(&acis_sphere_kernel_stream(save_format_version), 17);
            let arena = DecodeArena::new();
            let (ctx, view) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                    .expect("synthetic carrier fits policy");
            let carrier = parse_carrier(view, "token", 7, 100, 17).expect("carrier parses");
            assert_eq!(carrier.family, KernelFamily::Acis);
            decode_test_carrier(&ctx, &carrier).expect("ACIS carrier decodes")
        };

        let verified = decode(21_800);
        let unverified = decode(70_000);

        assert_eq!(unverified.header.width, 4);
        assert_eq!(unverified.header.save_format_version, Some(70_000));
        assert_eq!(
            unverified.header.product_family.as_deref(),
            Some("Inventor")
        );
        assert_eq!(unverified.brep.bodies.len(), 1);
        assert_eq!(unverified.brep.faces.len(), 1);
        assert_eq!(
            unverified.brep.surfaces.len(),
            verified.brep.surfaces.len(),
            "the substituted grammar read the same carriers"
        );
        assert!(unverified.brep.unknowns.is_empty());
    }

    #[test]
    fn pre_15_segment_version_attempts_the_nearest_footer_and_reads_the_same_records() {
        let decode = |segment_version_major| {
            let bytes = carrier_fixture(&acis_sphere_kernel_stream(21_800), segment_version_major);
            let arena = DecodeArena::new();
            let (ctx, view) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                    .expect("synthetic carrier fits policy");
            let carrier = parse_carrier(view, "token", 7, 100, segment_version_major)
                .expect("nearest footer frames");
            decode_test_carrier(&ctx, &carrier).expect("ACIS carrier decodes")
        };

        let in_band = decode(15);
        let recovered = decode(14);
        assert_eq!(recovered.brep.bodies.len(), in_band.brep.bodies.len());
        assert_eq!(recovered.brep.faces.len(), in_band.brep.faces.len());
        assert_eq!(recovered.brep.surfaces.len(), in_band.brep.surfaces.len());
    }

    #[test]
    fn pre_15_segment_version_over_garbage_is_malformed() {
        let bytes = carrier_fixture(b"not a kernel carrier", 14);
        with_view(&bytes, |view| {
            assert!(matches!(
                parse_carrier(view, "token", 7, 100, 14),
                Err(CodecError::Malformed(_))
            ));
        });
    }

    #[test]
    fn a_carrier_whose_header_does_not_parse_is_still_refused() {
        // Structural refusal stands: the magic matched and the fixed header did
        // not read, so there is nothing to frame.
        let mut acis = b"ACIS BinaryFile".to_vec();
        acis.extend_from_slice(&70_000_u32.to_le_bytes());
        let bytes = carrier_fixture(&acis, 17);
        let arena = DecodeArena::new();
        let (ctx, view) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic carrier fits policy");
        let carrier = parse_carrier(view, "token", 7, 100, 17).expect("carrier parses");
        assert!(matches!(
            decode_test_carrier(&ctx, &carrier),
            Err(CodecError::Malformed(_))
        ));
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

    fn empty_asm_fixture() -> Vec<u8> {
        let mut bytes = b"ASM BinaryFile4".to_vec();
        bytes.extend_from_slice(&700_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        for value in ["Inventor", "ASM test", "2000-01-01"] {
            bytes.push(0x07);
            bytes.push(value.len() as u8);
            bytes.extend_from_slice(value.as_bytes());
        }
        for value in [1.0_f64, 1.0e-6, 1.0e-10] {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    fn empty_acis_fixture() -> Vec<u8> {
        acis_fixture(21_800)
    }

    /// The same carrier at one save format, so a band no `acis:` row verifies
    /// can be read beside a verified one.
    fn acis_fixture(save_format_version: u32) -> Vec<u8> {
        let mut bytes = b"ACIS BinaryFile".to_vec();
        for value in [save_format_version, 0, 0, 0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in ["Inventor", "ASM 218 test", "2000-01-01"] {
            bytes.push(0x07);
            bytes.push(value.len() as u8);
            bytes.extend_from_slice(value.as_bytes());
        }
        for value in [1.0_f64, 1.0e-6, 1.0e-10] {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    fn with_view(bytes: &[u8], test: impl FnOnce(View<'_>)) {
        let arena = DecodeArena::new();
        let (_, view) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default())
            .expect("synthetic carrier fits policy");
        test(view);
    }
}
