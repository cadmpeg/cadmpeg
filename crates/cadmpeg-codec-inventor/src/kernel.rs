// SPDX-License-Identifier: Apache-2.0
//! Typed Inventor kernel-carrier selection and envelope framing.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use cadmpeg_asm::brep::{decode_with_header, AsmBrep, DecodePurpose};
use cadmpeg_asm::kernel_header::BinaryHeader;
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
    pub(crate) segment_token: cadmpeg_ir::ids::IdentityKey,
    /// Length of the carrier window, which is never empty.
    pub(crate) carrier_len: std::num::NonZeroU64,
    pub(crate) record_ordinal: u32,
    pub(crate) segment_version_major: u8,
    pub(crate) family: KernelFamily,
    pub(crate) header_state: u32,
    pub(crate) header_kind: u16,
    pub(crate) header_value: u32,
    pub(crate) schema: u32,
    pub(crate) carrier_offset: u64,
    pub(crate) bytes: View<'a>,
    pub(crate) header: Result<Box<BinaryHeader>, String>,
    pub(crate) selected_key: u32,
    pub(crate) enabled: bool,
    pub(crate) delta_state: i32,
    pub(crate) history_reference: u32,
}

#[derive(Debug)]
pub(crate) enum ActiveCarrierState<'a> {
    NotApplicable,
    Selected(ActiveCarrier<'a>),
    Unavailable(String),
}

fn parse_kernel_header(
    ctx: &DecodeContext<'_>,
    family: KernelFamily,
    bytes: &[u8],
) -> Result<Result<BinaryHeader, String>, CodecError> {
    let (parsed, absent) = match family {
        KernelFamily::Asm => (
            asm_header::parse(ctx, bytes)?,
            "Inventor ASM carrier has no parseable header",
        ),
        KernelFamily::Acis => (
            acis_header::parse(ctx, bytes)?,
            "Inventor ACIS carrier has no parseable header",
        ),
    };
    match parsed {
        Some(header) => Ok(Ok(header)),
        None => {
            let mut detail = ctx.retained_string(
                absent.len(),
                "retain Inventor absent kernel header detail",
            )?;
            detail.push_str(absent);
            Ok(Err(detail))
        }
    }
}

pub(crate) fn decode_kernel_carrier(
    ctx: &DecodeContext<'_>,
    carrier: &ActiveCarrier<'_>,
    header: &BinaryHeader,
) -> Result<AsmBrep, CodecError> {
    let bytes = carrier.bytes.window();
    let (start, solved_limit) = match carrier.family {
        KernelFamily::Asm => (
            asm_header::record_stream_start_with_header(bytes, header).ok_or_else(|| {
                CodecError::Malformed("Inventor ASM carrier has no record stream".into())
            })?,
            asm_header::solved_record_limit_with_header(ctx, bytes, header)?,
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
                acis_header::solved_record_limit_with_header(ctx, bytes, header)?,
            )
        }
    };
    let width = header.width;
    let (records, records_storage) =
        ctx.with_scoped_storage("frame Inventor kernel carrier records", || {
            match solved_limit {
                Some(limit) => sab::frame(ctx, bytes, start, limit, width, None),
                None => sab::frame_history(ctx, bytes, start, bytes.len(), width, None),
            }
            .map_err(|failure| {
                failure.into_codec_error(ctx, |error| {
                    CodecError::malformed(format_args!(
                        "Inventor {} SAB framing failed: {error}",
                        carrier.family.label()
                    ))
                })
            })
        })?;
    let stream = (!records.is_empty())
        .then(|| {
            ctx.format_scoped(
                format_args!(
                    "RSeStorage/B{}:record:{}",
                    carrier.segment_token, carrier.record_ordinal
                ),
                "format Inventor kernel carrier stream name",
            )
        })
        .transpose()?;
    let brep = decode_with_header(
        ctx,
        &records,
        bytes,
        Some(&header.metadata),
        stream.as_ref().map_or("", |(name, _)| name.as_str()),
        cadmpeg_asm::asm_format!("inventor"),
        DecodePurpose::Model,
    )?;
    drop(stream);
    drop(records);
    drop(records_storage);
    Ok(brep)
}

pub(crate) fn select_active_carrier<'a>(
    ctx: &DecodeContext<'_>,
    segments: &[SegmentDescriptor<'a>],
    document_kind: &DocumentKind,
) -> Result<ActiveCarrierState<'a>, CodecError> {
    if !matches!(document_kind, DocumentKind::Part) {
        return Ok(ActiveCarrierState::NotApplicable);
    }
    let mut brep_count = 0_u64;
    let mut selected_segment = None;
    let mut segments = segments.iter();
    while let Some(segment) =
        ctx.next_charged(&mut segments, "scan Inventor kernel carrier segments")?
    {
        if matches!(segment.kind, SegmentKind::PmBRep) {
            brep_count = brep_count.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "Inventor kernel carrier segment count",
                    u64::MAX - 1,
                    u64::MAX,
                )
            })?;
            selected_segment = Some(segment);
        }
    }
    let Some(segment) = selected_segment.filter(|_| brep_count == 1) else {
        return unavailable(
            ctx,
            format_args!("part document has {brep_count} PmBRep segments; expected one"),
        );
    };
    let SegmentBulkState::Framed(bulk) = &segment.bulk else {
        let detail_text = "PmBRep bulk stream is unavailable";
        let mut detail =
            ctx.retained_string(detail_text.len(), "retain Inventor carrier unavailable detail")?;
        detail.push_str(detail_text);
        return Ok(ActiveCarrierState::Unavailable(detail));
    };
    let table = match &bulk.records {
        RecordFrameState::Framed(table) => table,
        RecordFrameState::Unavailable(_) => {
            let detail_text = "PmBRep record table is unavailable";
            let mut detail = ctx.retained_string(
                detail_text.len(),
                "retain Inventor carrier unavailable detail",
            )?;
            detail.push_str(detail_text);
            return Ok(ActiveCarrierState::Unavailable(detail));
        }
    };
    let mut carrier_count = 0_u64;
    let mut selected_record = None;
    let mut records = table.records.iter();
    while let Some(record) =
        ctx.next_charged(&mut records, "scan Inventor typed kernel carrier records")?
    {
        if record.type_id == KERNEL_RECORD_TYPE_ID {
            carrier_count = carrier_count.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "Inventor typed kernel carrier count",
                    u64::MAX - 1,
                    u64::MAX,
                )
            })?;
            selected_record = Some(record);
        }
    }
    let Some(record) = selected_record.filter(|_| carrier_count == 1) else {
        return unavailable(
            ctx,
            format_args!(
                "PmBRep contains {carrier_count} typed kernel-carrier records; expected one"
            ),
        );
    };
    let Some(version) = segment.registry.map(|join| join.version_major) else {
        let detail_text = "PmBRep segment version is unavailable from the registry";
        let mut detail =
            ctx.retained_string(detail_text.len(), "retain Inventor carrier unavailable detail")?;
        detail.push_str(detail_text);
        return Ok(ActiveCarrierState::Unavailable(detail));
    };
    match parse_carrier(
        ctx,
        record.payload,
        segment.pair.token.key(),
        record.ordinal,
        record.payload_offset,
        version,
    ) {
        Ok(carrier) => Ok(ActiveCarrierState::Selected(carrier)),
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(error) => {
            let detail = ctx.format_retained(
                format_args!("{error}"),
                "retain Inventor carrier unavailable detail",
            )?;
            Ok(ActiveCarrierState::Unavailable(detail))
        }
    }
}

fn unavailable<'a>(
    ctx: &DecodeContext<'_>,
    detail: std::fmt::Arguments<'_>,
) -> Result<ActiveCarrierState<'a>, CodecError> {
    Ok(ActiveCarrierState::Unavailable(ctx.format_retained(
        detail,
        "retain Inventor carrier unavailable detail",
    )?))
}

fn parse_carrier<'a>(
    ctx: &DecodeContext<'_>,
    payload: View<'a>,
    segment_token: &cadmpeg_ir::ids::IdentityKey,
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
    // The carrier window is what the record holds between its header and its
    // footer. Admitting its length here is what gives every reader a nonzero
    // length instead of a check at the point of use.
    let carrier_len = cadmpeg_core::decode::u64_from_index(carrier.window().len());
    let Some(carrier_len) = std::num::NonZeroU64::new(carrier_len) else {
        return Err(CodecError::Malformed(
            "Inventor kernel-carrier record holds no carrier bytes".into(),
        ));
    };
    let header = match parse_kernel_header(ctx, family, carrier.window())? {
        Ok(header) => Ok(Box::new(header)),
        Err(detail) => Err(detail),
    };
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
        segment_token: segment_token
            .try_clone_for_decode(ctx, "retain Inventor selected carrier token")?,
        carrier_len,
        record_ordinal,
        segment_version_major,
        family,
        header_state,
        header_kind,
        header_value,
        schema,
        carrier_offset: record_payload_offset
            + cadmpeg_core::decode::u64_from_index(carrier_header::LEN),
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
    use cadmpeg_asm::brep::AsmBrep;
    use cadmpeg_asm::kernel_header::{BinaryHeader, KernelHeader, RefWidth};
    use cadmpeg_container::compound::CompoundSnapshot;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    use super::{
        decode_kernel_carrier, parse_carrier, select_active_carrier, ActiveCarrier,
        ActiveCarrierState, KernelFamily,
    };
    use crate::rse::{DocumentKind, RseInventory};
    use crate::test_support::test_fixtures::acis_sphere_kernel_stream;
    use cadmpeg_core::decode::View;
    use cadmpeg_core::CodecError;

    fn header_copy_fixture() -> BinaryHeader {
        BinaryHeader {
            width: RefWidth::Eight,
            metadata: KernelHeader {
                save_format_version: Some(700),
                entity_count: Some(12),
                flags: Some(3),
                product_family: Some("family".to_owned()),
                product_version: Some("version".to_owned()),
                save_date: Some("date".to_owned()),
                scale: Some(2.0),
                linear: Some(0.125),
                angular: Some(0.25),
            },
        }
    }

    fn empty_asm_carrier_for_header(header: &BinaryHeader) -> Vec<u8> {
        let mut stream = match header.width {
            RefWidth::Four => {
                let mut stream = b"ASM BinaryFile4".to_vec();
                stream.extend_from_slice(
                    &header.metadata.save_format_version.unwrap_or_default().to_le_bytes(),
                );
                stream.extend_from_slice(&0_u32.to_le_bytes());
                stream.extend_from_slice(
                    &u32::try_from(header.metadata.entity_count.unwrap_or_default())
                        .expect("fixture entity count fits u32")
                        .to_le_bytes(),
                );
                stream.extend_from_slice(
                    &u32::try_from(header.metadata.flags.unwrap_or_default())
                        .expect("fixture flags fit u32")
                        .to_le_bytes(),
                );
                stream
            }
            RefWidth::Eight => {
                let mut stream = b"ASM BinaryFile8".to_vec();
                stream.extend_from_slice(
                    &header.metadata.save_format_version.unwrap_or_default().to_le_bytes(),
                );
                stream.extend_from_slice(&[0; 12]);
                stream.extend_from_slice(
                    &header.metadata.entity_count.unwrap_or_default().to_le_bytes(),
                );
                stream.extend_from_slice(&header.metadata.flags.unwrap_or_default().to_le_bytes());
                stream
            }
        };
        for value in [
            header.metadata.product_family.as_deref().unwrap_or(""),
            header.metadata.product_version.as_deref().unwrap_or(""),
            header.metadata.save_date.as_deref().unwrap_or(""),
        ] {
            stream.extend_from_slice(&[
                0x07,
                u8::try_from(value.len()).expect("fixture string fits u8"),
            ]);
            stream.extend_from_slice(value.as_bytes());
        }
        for value in [
            header.metadata.scale.unwrap_or_default(),
            header.metadata.linear.unwrap_or_default(),
            header.metadata.angular.unwrap_or_default(),
        ] {
            stream.push(0x06);
            stream.extend_from_slice(&value.to_le_bytes());
        }
        carrier_fixture(&stream, 23)
    }

    #[test]
    fn decoded_kernel_fits_17_work_units_without_header_copy() {
        let header = header_copy_fixture();
        let original_header = header.clone();
        let bytes = empty_asm_carrier_for_header(&header);
        let arena = DecodeArena::new();
        let (service, view) = DecodeContext::from_root_bytes(
            &bytes,
            &arena,
            &DecodePolicy::service(),
        )
        .expect("service context");
        let carrier = parse_carrier(&service, view, &cadmpeg_ir::identity_key!("token"), 7, 100, 23)
            .expect("carrier parses");
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 17;
        policy.limits.max_work_units = 17;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("limited context");
        let decoded = decode_kernel_carrier(&ctx, &carrier, &header)
            .expect("empty ASM carrier decodes with both allowances");
        assert!(decoded.bodies.is_empty());
        assert_eq!(header, original_header);
        ctx.charge_retained(17, "retain remaining kernel header allowance")
            .expect("all 17 header bytes remain available after decode");
        let retained_refusal = ctx
            .charge_retained(1, "probe kernel retained allowance")
            .expect_err("the retained allowance is exhausted by the probe");
        let CodecError::ResourceLimit(retained_refusal) = retained_refusal else {
            panic!("retained probe must hit its configured limit");
        };
        assert_eq!(retained_refusal.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(retained_refusal.used, 17);
        assert!(matches!(
            ctx.finish_session(),
            Err(CodecError::ResourceLimit(limit)) if limit == retained_refusal
        ));

        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 17;
        policy.limits.max_work_units = 17;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("independent work context");
        let decoded = decode_kernel_carrier(&ctx, &carrier, &header)
            .expect("empty ASM carrier decodes with both allowances");
        assert!(decoded.bodies.is_empty());
        assert_eq!(header, original_header);
        let work_refusal = ctx
            .charge_work(18, "probe kernel work allowance")
            .expect_err("the terminal work probe exceeds the remaining allowance");
        let CodecError::ResourceLimit(work_refusal) = work_refusal else {
            panic!("work probe must hit its configured limit");
        };
        assert_eq!(work_refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!(work_refusal.limit, 17);
        assert!(work_refusal.used > 0);
        assert!(work_refusal.used <= 17);
        assert!(matches!(
            ctx.finish_session(),
            Err(CodecError::ResourceLimit(limit)) if limit == work_refusal
        ));
    }

    #[test]
    fn decoded_kernel_leaves_zero_and_sixteen_byte_header_allowances_available() {
        let header = header_copy_fixture();
        let original_header = header.clone();
        let bytes = empty_asm_carrier_for_header(&header);
        for retained in [0, 16] {
            let arena = DecodeArena::new();
            let (service, view) = DecodeContext::from_root_bytes(
                &bytes,
                &arena,
                &DecodePolicy::service(),
            )
            .expect("service context");
            let carrier =
                parse_carrier(&service, view, &cadmpeg_ir::identity_key!("token"), 7, 100, 23)
                    .expect("carrier parses");
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = retained;
            let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("limited context");
            let decoded = decode_kernel_carrier(&ctx, &carrier, &header)
                .expect("empty ASM carrier decodes without retaining header text");
            assert!(decoded.bodies.is_empty());
            assert_eq!(header, original_header);
            ctx.charge_retained(retained, "retain remaining kernel header allowance")
                .expect("decode leaves the full configured allowance available");
            let refusal = ctx
                .charge_retained(1, "probe kernel retained allowance")
                .expect_err("the retained probe exceeds the configured allowance");
            let CodecError::ResourceLimit(refusal) = refusal else {
                panic!("retained probe must hit its configured limit");
            };
            assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(refusal.used, retained);
            assert!(matches!(
                ctx.finish_session(),
                Err(CodecError::ResourceLimit(limit)) if limit == refusal
            ));
        }
    }

    #[test]
    fn zero_work_decode_refuses_empty_asm_reachable_face_probe() {
        let header = header_copy_fixture();
        let original_header = header.clone();
        let bytes = empty_asm_carrier_for_header(&header);
        let arena = DecodeArena::new();
        let (service, view) = DecodeContext::from_root_bytes(
            &bytes,
            &arena,
            &DecodePolicy::service(),
        )
        .expect("service context");
        let carrier = parse_carrier(&service, view, &cadmpeg_ir::identity_key!("token"), 7, 100, 23)
            .expect("carrier parses");
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("zero-work context");
        let refusal = decode_kernel_carrier(&ctx, &carrier, &header)
            .err()
            .expect("empty ASM reachability still probes the iterator end");
        let CodecError::ResourceLimit(refusal) = refusal else {
            panic!("zero work must refuse the first charged ASM operation");
        };
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!(refusal.operation, "ASM reachable face walk");
        assert_eq!(refusal.used, 0);
        assert_eq!(refusal.additional, 1);
        assert!(matches!(
            ctx.finish_session(),
            Err(CodecError::ResourceLimit(limit)) if limit == refusal
        ));
        assert_eq!(header, original_header);

        let (service, _) = DecodeContext::from_root_bytes(
            &bytes,
            &arena,
            &DecodePolicy::service(),
        )
        .expect("service context");
        let decoded = decode_kernel_carrier(&service, &carrier, &header)
            .expect("empty ASM carrier decodes under the service work budget");
        assert!(decoded.bodies.is_empty());
        assert_eq!(header, original_header);
    }

    #[test]
    fn zero_work_refusal_preserves_absent_and_empty_kernel_header_strings() {
        let mut header = header_copy_fixture();
        header.metadata.product_family = None;
        header.metadata.product_version = Some(String::new());
        header.metadata.save_date = None;
        let original_header = header.clone();
        let bytes = empty_asm_carrier_for_header(&header);
        let arena = DecodeArena::new();
        let (service, view) = DecodeContext::from_root_bytes(
            &bytes,
            &arena,
            &DecodePolicy::service(),
        )
        .expect("service context");
        let carrier = parse_carrier(&service, view, &cadmpeg_ir::identity_key!("token"), 7, 100, 23)
            .expect("carrier parses");
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("zero-retained-and-work context");
        let work_refusal = decode_kernel_carrier(&ctx, &carrier, &header)
            .err()
            .expect("zero work refuses the empty ASM reachability probe");
        let CodecError::ResourceLimit(work_refusal) = work_refusal else {
            panic!("zero work must refuse the first charged ASM operation");
        };
        assert_eq!(work_refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!(work_refusal.operation, "ASM reachable face walk");
        assert_eq!(work_refusal.used, 0);
        assert_eq!(work_refusal.additional, 1);
        assert!(matches!(
            ctx.finish_session(),
            Err(CodecError::ResourceLimit(limit)) if limit == work_refusal
        ));
        assert_eq!(header.metadata, original_header.metadata);

        let mut retained_policy = DecodePolicy::service();
        retained_policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &retained_policy)
            .expect("independent retained context");
        let decoded = decode_kernel_carrier(&ctx, &carrier, &header)
            .expect("empty ASM carrier decodes with the service work budget");
        assert!(decoded.bodies.is_empty());
        assert_eq!(header.metadata, original_header.metadata);
        let retained_refusal = ctx
            .charge_retained(1, "probe kernel retained allowance")
            .expect_err("zero retained allowance refuses the probe");
        let CodecError::ResourceLimit(retained_refusal) = retained_refusal else {
            panic!("retained probe must hit its configured limit");
        };
        assert_eq!(retained_refusal.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(retained_refusal.used, 0);
        assert!(matches!(
            ctx.finish_session(),
            Err(CodecError::ResourceLimit(limit)) if limit == retained_refusal
        ));
    }

    #[test]
    fn parsed_kernel_header_refuses_retained_limit_before_product_copy() {
        let bytes = carrier_fixture(&empty_asm_fixture(), 23);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes =
            cadmpeg_core::decode::u64_from_index("Inventor".len()) - 1;
        let (limited, view) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
        assert!(matches!(
            parse_carrier(&limited, view, &cadmpeg_ir::identity_key!("token"), 7, 100, 23),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain kernel header product string"
        ));
        let (service, view) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("service context");
        assert!(parse_carrier(
            &service,
            view,
            &cadmpeg_ir::identity_key!("token"),
            7,
            100,
            23
        )
        .is_ok());
    }

    #[test]
    fn parsed_kernel_header_box_needs_no_collection_slot() {
        let bytes = carrier_fixture(&empty_asm_fixture(), 23);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (limited, view) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
        let carrier = parse_carrier(
            &limited,
            view,
            &cadmpeg_ir::identity_key!("token"),
            7,
            100,
            23,
        )
        .expect("a fixed header box has no collection slots");
        assert_eq!(
            carrier
                .header
                .expect("parsed header")
                .metadata
                .save_format_version,
            Some(700)
        );
        // The parsed header is one fixed record; zero collection entries were stored.
        assert!(matches!(limited.charge_collection_items(1, "probe"),
            Err(CodecError::ResourceLimit(limit)) if limit.used == 0));
    }

    #[test]
    fn absent_kernel_header_detail_refuses_retained_limit_before_copy() {
        let bytes = carrier_fixture(b"ASM BinaryFile9", 23);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (limited, view) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
        assert!(matches!(
            parse_carrier(&limited, view, &cadmpeg_ir::identity_key!("token"), 7, 100, 23),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain Inventor absent kernel header detail"
        ));
        let (service, view) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("service context");
        let carrier = parse_carrier(
            &service,
            view,
            &cadmpeg_ir::identity_key!("token"),
            7,
            100,
            23,
        )
        .expect("carrier selection retains absent header");
        assert_eq!(
            carrier
                .header
                .expect_err("invalid width has no parsed header"),
            "Inventor ASM carrier has no parseable header"
        );
    }

    #[test]
    fn active_carrier_scans_refuse_work_limit_before_record_search() {
        let bytes = crate::test_support::test_fixtures::primary_envelope_fixture();
        let arena = DecodeArena::new();
        let (setup_ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("carrier fixture context");
        let snapshot = CompoundSnapshot::new(&setup_ctx, root).expect("compound fixture");
        let inventory = RseInventory::build(&setup_ctx, &snapshot).expect("RSe fixture");
        assert!(matches!(
            select_active_carrier(&setup_ctx, &inventory.segments, &DocumentKind::Part)
                .expect("service admission"),
            ActiveCarrierState::Selected(_)
        ));
        let record_count = inventory
            .segments
            .iter()
            .find_map(|segment| {
                if !matches!(&segment.kind, crate::rse::SegmentKind::PmBRep) {
                    return None;
                }
                let crate::rse::SegmentBulkState::Framed(bulk) = &segment.bulk else {
                    return None;
                };
                let crate::rse::RecordFrameState::Framed(table) = &bulk.records else {
                    return None;
                };
                Some(table.records.len())
            })
            .expect("fixture has a framed PmBRep record table");
        let segment_work = cadmpeg_core::decode::u64_from_index(inventory.segments.len());
        let record_work = cadmpeg_core::decode::u64_from_index(record_count);

        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (limited_ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
        let refusal = select_active_carrier(
            &limited_ctx,
            &inventory.segments,
            &DocumentKind::Part,
        )
        .err()
        .expect("zero work refuses the first segment iterator step");
        let CodecError::ResourceLimit(refusal) = refusal else {
            panic!("segment scan refusal must be a resource limit");
        };
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!(refusal.operation, "scan Inventor kernel carrier segments");
        assert_eq!(refusal.used, 0);
        assert_eq!(refusal.additional, 1);
        assert!(matches!(
            limited_ctx.finish_session(),
            Err(CodecError::ResourceLimit(limit)) if limit == refusal
        ));

        policy.limits.max_work_units = segment_work;
        let (limited_ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
        let refusal = select_active_carrier(
            &limited_ctx,
            &inventory.segments,
            &DocumentKind::Part,
        )
        .err()
        .expect("the segment iterator end probe exceeds the exact n-unit cap");
        let CodecError::ResourceLimit(refusal) = refusal else {
            panic!("segment end probe refusal must be a resource limit");
        };
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!(refusal.operation, "scan Inventor kernel carrier segments");
        assert_eq!(refusal.used, segment_work);
        assert_eq!(refusal.additional, 1);
        assert!(matches!(
            limited_ctx.finish_session(),
            Err(CodecError::ResourceLimit(limit)) if limit == refusal
        ));

        let record_scan_limit = segment_work
            .checked_add(1)
            .expect("fixture segment count leaves room for the end probe");
        policy.limits.max_work_units = record_scan_limit;
        let (limited_ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
        let refusal = select_active_carrier(
            &limited_ctx,
            &inventory.segments,
            &DocumentKind::Part,
        )
        .err()
        .expect("record scan refuses before its first iterator step");
        let CodecError::ResourceLimit(refusal) = refusal else {
            panic!("record scan refusal must be a resource limit");
        };
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!(refusal.operation, "scan Inventor typed kernel carrier records");
        assert_eq!(refusal.used, record_scan_limit);
        assert_eq!(refusal.additional, 1);
        assert!(matches!(
            limited_ctx.finish_session(),
            Err(CodecError::ResourceLimit(limit)) if limit == refusal
        ));

        let record_end_limit = record_scan_limit
            .checked_add(record_work)
            .expect("fixture record count fits the work budget");
        policy.limits.max_work_units = record_end_limit;
        let (limited_ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
        let refusal = select_active_carrier(
            &limited_ctx,
            &inventory.segments,
            &DocumentKind::Part,
        )
        .err()
        .expect("the record iterator end probe exceeds its exact count cap");
        let CodecError::ResourceLimit(refusal) = refusal else {
            panic!("record end probe refusal must be a resource limit");
        };
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!(refusal.operation, "scan Inventor typed kernel carrier records");
        assert_eq!(refusal.used, record_end_limit);
        assert_eq!(refusal.additional, 1);
        assert!(matches!(
            limited_ctx.finish_session(),
            Err(CodecError::ResourceLimit(limit)) if limit == refusal
        ));
    }

    #[test]
    fn selected_carrier_token_refuses_retained_limit_before_copy() {
        let bytes = crate::test_support::test_fixtures::primary_envelope_fixture();
        let arena = DecodeArena::new();
        let (setup_ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("carrier fixture context");
        let snapshot = CompoundSnapshot::new(&setup_ctx, root).expect("compound fixture");
        let inventory = RseInventory::build(&setup_ctx, &snapshot).expect("RSe fixture");
        let ActiveCarrierState::Selected(carrier) =
            select_active_carrier(&setup_ctx, &inventory.segments, &DocumentKind::Part)
                .expect("service admission")
        else {
            panic!("selected carrier fixture");
        };
        let mut policy = DecodePolicy::service();
        let header = carrier.header.as_ref().expect("parsed carrier header");
        let header_strings = [
            &header.metadata.product_family,
            &header.metadata.product_version,
            &header.metadata.save_date,
        ]
        .into_iter()
        .flatten()
        .map(String::len)
        .sum::<usize>();
        policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(
            header_strings + carrier.segment_token.as_str().len() - 1,
        );
        let (limited_ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
        assert!(matches!(
            select_active_carrier(&limited_ctx, &inventory.segments, &DocumentKind::Part),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain Inventor selected carrier token"
        ));
    }

    fn decode_test_carrier(
        ctx: &DecodeContext<'_>,
        carrier: &ActiveCarrier<'_>,
    ) -> Result<AsmBrep, CodecError> {
        let header = carrier
            .header
            .as_ref()
            .map_err(|detail| CodecError::Malformed(detail.clone()))?;
        decode_kernel_carrier(ctx, carrier, header)
    }

    #[test]
    fn typed_carrier_envelope_selects_family_and_exact_footer() {
        let bytes = carrier_fixture(b"ASM BinaryFile4 synthetic", 18);
        with_view(&bytes, |ctx, view| {
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
            let carrier = parse_carrier(ctx, view, &cadmpeg_ir::identity_key!("token"), 7, 100, 18)
                .expect("carrier parses");
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
        with_view(&malformed, |ctx, view| {
            assert!(
                parse_carrier(ctx, view, &cadmpeg_ir::identity_key!("token"), 7, 100, 18).is_err()
            );
        });
    }

    #[test]
    fn asm_carrier_uses_header_boundary_and_shared_decoder() {
        let asm = empty_asm_fixture();
        let bytes = carrier_fixture(&asm, 23);
        let arena = DecodeArena::new();
        let (ctx, view) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic carrier fits policy");
        let carrier = parse_carrier(&ctx, view, &cadmpeg_ir::identity_key!("token"), 7, 100, 23)
            .expect("carrier parses");
        let header = carrier.header.as_ref().expect("parsed carrier header");
        let decoded = decode_test_carrier(&ctx, &carrier).expect("ASM carrier decodes");

        assert_eq!(header.width.bytes(), 4);
        assert_eq!(header.metadata.save_format_version, Some(700));
        assert_eq!(
            header.metadata.product_family.as_deref(),
            Some("Inventor")
        );
        assert!(decoded.bodies.is_empty());
        assert!(decoded.unknowns.is_empty());
    }

    #[test]
    fn acis_carrier_uses_32_bit_header_and_shared_decoder() {
        let acis = empty_acis_fixture();
        let bytes = carrier_fixture(&acis, 17);
        let arena = DecodeArena::new();
        let (ctx, view) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic carrier fits policy");
        let carrier = parse_carrier(&ctx, view, &cadmpeg_ir::identity_key!("token"), 7, 100, 17)
            .expect("carrier parses");
        let header = carrier.header.as_ref().expect("parsed carrier header");
        let decoded = decode_test_carrier(&ctx, &carrier).expect("ACIS carrier decodes");

        assert_eq!(carrier.family, KernelFamily::Acis);
        assert_eq!(header.width.bytes(), 4);
        assert_eq!(header.metadata.save_format_version, Some(21_800));
        assert_eq!(
            header.metadata.product_family.as_deref(),
            Some("Inventor")
        );
        assert!(decoded.bodies.is_empty());
        assert!(decoded.unknowns.is_empty());
    }

    #[test]
    fn decoded_kernel_metadata_stays_on_the_original_header() {
        let bytes = carrier_fixture(&empty_asm_fixture(), 23);
        let arena = DecodeArena::new();
        let (service, view) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("service context");
        let carrier = parse_carrier(
            &service,
            view,
            &cadmpeg_ir::identity_key!("token"),
            7,
            100,
            23,
        )
        .expect("carrier parses");
        let header = carrier.header.as_ref().expect("header");
        let mut policy = DecodePolicy::service();
        // The carrier owns 8 bytes for family, 8 for version, and 10 for date.
        // Decode borrows that header and retains no additional header bytes.
        policy.limits.max_retained_bytes = 8 + 8 + 10;
        let (limited, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
        let decoded = decode_kernel_carrier(&limited, &carrier, header)
            .expect("borrowed metadata needs no retained copy");
        assert!(decoded.bodies.is_empty());
        assert_eq!(header.metadata.product_family.as_deref(), Some("Inventor"));
        limited
            .charge_retained(8 + 8 + 10, "retain the remaining kernel header allowance")
            .expect("the original 26-byte allowance remains after decode");
        assert!(matches!(limited.charge_retained(1, "probe kernel retained allowance"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes && limit.used == 26));
        assert!(decode_test_carrier(&service, &carrier).is_ok());
    }

    #[test]
    fn empty_kernel_stream_leaves_the_original_retained_allowance_available() {
        let bytes = carrier_fixture(&empty_asm_fixture(), 23);
        let arena = DecodeArena::new();
        let (service, view) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("service context");
        let carrier = parse_carrier(
            &service,
            view,
            &cadmpeg_ir::identity_key!("token"),
            7,
            100,
            23,
        )
        .expect("carrier parses");
        let mut policy = DecodePolicy::service();
        // The carrier owns 8 bytes for family, 8 for version, and 10 for date.
        // Decode retains no additional header bytes, leaving all 25 available.
        policy.limits.max_retained_bytes = 8 + 8 + 10 - 1;
        let (limited, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
        assert!(decode_test_carrier(&limited, &carrier).is_ok());
        limited
            .charge_retained(8 + 8 + 10 - 1, "retain the remaining kernel header allowance")
            .expect("the original 25-byte allowance remains after decode");
        assert!(matches!(limited.charge_retained(1, "probe kernel retained allowance"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes && limit.used == 25));
        assert!(decode_test_carrier(&service, &carrier).is_ok());
    }

    #[test]
    fn empty_kernel_skips_stream_name_formatting() {
        let bytes = carrier_fixture(&empty_asm_fixture(), 23);
        let arena = DecodeArena::new();
        let (service, view) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("service context");
        let carrier = parse_carrier(
            &service,
            view,
            &cadmpeg_ir::identity_key!("token"),
            7,
            100,
            23,
        )
        .expect("carrier parses");
        let mut policy = DecodePolicy::service();
        // Empty framing and absent annotations need zero temporary stream bytes.
        policy.limits.max_materialized_bytes = 0;
        let (limited, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        let decoded =
            decode_test_carrier(&limited, &carrier).expect("empty framing needs no stream name");
        assert!(decoded.annotation_records.is_empty());
        assert!(matches!(limited.reserve_scoped(u64::MAX, "probe"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::MaterializedBytes && limit.used == 0));
        assert!(decode_test_carrier(&service, &carrier).is_ok());
    }

    #[test]
    fn decoded_kernel_uses_original_header_text_without_retained_copy() {
        let bytes = carrier_fixture(&empty_asm_fixture(), 23);
        let arena = DecodeArena::new();
        let (service, view) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("service context");
        let carrier = parse_carrier(
            &service,
            view,
            &cadmpeg_ir::identity_key!("token"),
            7,
            100,
            23,
        )
        .expect("carrier parses");
        let header = carrier.header.as_ref().expect("parsed kernel header");
        let mut policy = DecodePolicy::service();
        // The carrier owns 8 + 8 + 10 bytes of header text. Empty ASM framing
        // uses scoped storage, so the 26-byte allowance remains untouched.
        policy.limits.max_retained_bytes = 8 + 8 + 10;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        let decoded =
            decode_test_carrier(&ctx, &carrier).expect("header text is borrowed from the carrier");
        assert_eq!(
            header.metadata.product_family.as_deref(),
            Some("Inventor")
        );
        assert_eq!(
            header.metadata.product_version.as_deref(),
            Some("ASM test")
        );
        assert_eq!(
            header.metadata.save_date.as_deref(),
            Some("2000-01-01")
        );
        assert!(decoded.bodies.is_empty());
        ctx.charge_retained(8 + 8 + 10, "retain the remaining kernel header allowance")
            .expect("the original 26-byte allowance remains after decode");
        assert!(matches!(ctx.charge_retained(1, "probe kernel retained allowance"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes && limit.used == 26));
    }

    #[test]
    fn decoded_kernel_releases_frame_and_stream_storage() {
        let bytes = carrier_fixture(&acis_sphere_kernel_stream(21_800), 17);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, view) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        let carrier = parse_carrier(&ctx, view, &cadmpeg_ir::identity_key!("token"), 7, 100, 17)
            .expect("carrier parses");
        let decoded = decode_test_carrier(&ctx, &carrier).expect("sphere decodes");
        assert_eq!(decoded.bodies.len(), 1);
        assert!(
            matches!(ctx.reserve_scoped(u64::MAX, "released temporary kernel data"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::MaterializedBytes && limit.used == 0)
        );
    }

    #[test]
    fn decoded_kernel_header_leaves_the_original_short_allowance_available() {
        let bytes = carrier_fixture(&empty_asm_fixture(), 23);
        let arena = DecodeArena::new();
        let (service, view) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("service context");
        let carrier = parse_carrier(
            &service,
            view,
            &cadmpeg_ir::identity_key!("token"),
            7,
            100,
            23,
        )
        .expect("carrier parses");
        let header = carrier.header.as_ref().expect("parsed kernel header");
        let mut policy = DecodePolicy::service();
        // The carrier owns 8 bytes for family. Decode retains no additional
        // header bytes, leaving all 7 configured bytes available.
        policy.limits.max_retained_bytes =
            cadmpeg_core::decode::u64_from_index("Inventor".len()) - 1;
        let (limited, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
        let decoded = decode_kernel_carrier(&limited, &carrier, header)
            .expect("borrowed header needs no retained allowance");
        assert!(decoded.bodies.is_empty());
        assert_eq!(header.metadata.product_family.as_deref(), Some("Inventor"));
        limited
            .charge_retained(
                cadmpeg_core::decode::u64_from_index("Inventor".len()) - 1,
                "retain the remaining kernel header allowance",
            )
            .expect("the original 7-byte allowance remains after decode");
        assert!(matches!(limited.charge_retained(1, "probe kernel retained allowance"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes && limit.used == 7));
        assert!(decode_test_carrier(&service, &carrier).is_ok());
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
            let carrier =
                parse_carrier(&ctx, view, &cadmpeg_ir::identity_key!("token"), 7, 100, 17)
                    .expect("carrier parses");
            assert_eq!(carrier.family, KernelFamily::Acis);
            let header = carrier.header.as_ref().expect("parsed carrier header");
            assert_eq!(header.width.bytes(), 4);
            assert_eq!(
                header.metadata.save_format_version,
                Some(save_format_version)
            );
            assert_eq!(header.metadata.product_family.as_deref(), Some("Inventor"));
            decode_test_carrier(&ctx, &carrier).expect("ACIS carrier decodes")
        };

        let verified = decode(21_800);
        let unverified = decode(70_000);

        assert_eq!(unverified.bodies.len(), 1);
        assert_eq!(unverified.faces.len(), 1);
        assert_eq!(
            unverified.surfaces.len(),
            verified.surfaces.len(),
            "the substituted grammar read the same carriers"
        );
        assert!(unverified.unknowns.is_empty());
    }

    #[test]
    fn pre_15_segment_version_attempts_the_nearest_footer_and_reads_the_same_records() {
        let decode = |segment_version_major| {
            let bytes = carrier_fixture(&acis_sphere_kernel_stream(21_800), segment_version_major);
            let arena = DecodeArena::new();
            let (ctx, view) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                    .expect("synthetic carrier fits policy");
            let carrier = parse_carrier(
                &ctx,
                view,
                &cadmpeg_ir::identity_key!("token"),
                7,
                100,
                segment_version_major,
            )
            .expect("nearest footer frames");
            decode_test_carrier(&ctx, &carrier).expect("ACIS carrier decodes")
        };

        let in_band = decode(15);
        let recovered = decode(14);
        assert_eq!(recovered.bodies.len(), in_band.bodies.len());
        assert_eq!(recovered.faces.len(), in_band.faces.len());
        assert_eq!(recovered.surfaces.len(), in_band.surfaces.len());
    }

    #[test]
    fn pre_15_segment_version_over_garbage_is_malformed() {
        let bytes = carrier_fixture(b"not a kernel carrier", 14);
        with_view(&bytes, |ctx, view| {
            assert!(matches!(
                parse_carrier(ctx, view, &cadmpeg_ir::identity_key!("token"), 7, 100, 14),
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
        let carrier = parse_carrier(&ctx, view, &cadmpeg_ir::identity_key!("token"), 7, 100, 17)
            .expect("carrier parses");
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
            bytes.push(u8::try_from(value.len()).expect("fixture value fits u8"));
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
            bytes.push(u8::try_from(value.len()).expect("fixture value fits u8"));
            bytes.extend_from_slice(value.as_bytes());
        }
        for value in [1.0_f64, 1.0e-6, 1.0e-10] {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    fn with_view(bytes: &[u8], test: impl FnOnce(&DecodeContext<'_>, View<'_>)) {
        let arena = DecodeArena::new();
        let (ctx, view) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default())
            .expect("synthetic carrier fits policy");
        test(&ctx, view);
    }
}
