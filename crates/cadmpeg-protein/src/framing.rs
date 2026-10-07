// SPDX-License-Identifier: Apache-2.0
//! Paged logical-record framing.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;

use crate::layout::{continuation_page, instance_stream_header, record_start_page, terminal_page};
use crate::{CONTINUATION_MARKER, PAGE_SIZE, RECORD_MARKER, STREAM_HEADER_LEN, TERMINAL_MARKER};

/// One exact logical record recovered from the `InstanceProperties` page
/// framing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordFrame {
    /// Byte offset in the dechunked logical stream.
    logical_offset: usize,
    /// Complete record bytes, including the opening marker.
    bytes: Vec<u8>,
}

impl RecordFrame {
    /// Returns the record offset in the dechunked logical stream.
    pub const fn logical_offset(&self) -> usize {
        self.logical_offset
    }

    /// Returns the complete record bytes, including the opening marker.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Temporary frames and the reservation that remains live while their bytes exist.
#[derive(Debug)]
pub struct RecordFrames<'ctx> {
    frames: Vec<RecordFrame>,
    _reservation: ScopedReservation<'ctx>,
}

impl RecordFrames<'_> {
    /// Borrows the framed records without releasing their storage reservation.
    pub fn frames(&self) -> &[RecordFrame] {
        &self.frames
    }
}

/// Split a paged `InstanceProperties` stream into caller-owned temporary
/// logical records.
///
/// The stream is a [`STREAM_HEADER_LEN`]-byte header followed by fixed
/// [`PAGE_SIZE`] pages. A page whose bytes 4..8 hold [`RECORD_MARKER`] opens a
/// record, [`CONTINUATION_MARKER`] extends it, and a page opening with
/// [`TERMINAL_MARKER`] closes it and carries the used byte count as a `u16` at
/// offset 4. Every record is returned with the opening marker restored so
/// record offsets match the on-page layout.
pub fn record_frames_admitted<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
) -> Result<RecordFrames<'ctx>, CodecError> {
    let mut reservation = ctx.reserve_scoped(0, "Protein temporary frames")?;
    let frames = frame_records(bytes, ctx, &mut reservation)?;
    Ok(RecordFrames {
        frames,
        _reservation: reservation,
    })
}

fn frame_records(
    bytes: &[u8],
    ctx: &DecodeContext<'_>,
    scope: &mut ScopedReservation<'_>,
) -> Result<Vec<RecordFrame>, CodecError> {
    if bytes.len() < STREAM_HEADER_LEN + PAGE_SIZE {
        return Err(CodecError::Malformed(
            "Protein page stream is shorter than its header and one page".into(),
        ));
    }
    if View::u32_le_at(bytes, instance_stream_header::DECLARED_SIZE)
        .map(cadmpeg_core::decode::index_from_u32)
        != Some(PAGE_SIZE)
    {
        return Err(CodecError::Malformed(
            "Protein declared page size is invalid".into(),
        ));
    }
    if !(bytes.len() - STREAM_HEADER_LEN).is_multiple_of(PAGE_SIZE) {
        return Err(CodecError::Malformed(
            "Protein page stream has a partial trailing page".into(),
        ));
    }
    let mut records: Vec<RecordFrame> = Vec::new();
    let mut current = false;
    let mut logical_offset = 0usize;
    // Each page costs one step of fixed marker tests; its payload copy is
    // charged by the copy itself.
    let mut pages = bytes[STREAM_HEADER_LEN..].chunks_exact(PAGE_SIZE);
    while let Some(page) = ctx.next_charged(&mut pages, "Protein page framing scan")? {
        let (payload, terminal) = if page.get(record_start_page::MARKER..record_start_page::BODY)
            == Some(RECORD_MARKER)
        {
            if current {
                let record = records.last().ok_or_else(|| {
                    CodecError::Malformed("Protein page has no record owner".into())
                })?;
                logical_offset =
                    logical_offset
                        .checked_add(record.bytes.len())
                        .ok_or_else(|| {
                            CodecError::Malformed(
                                "Protein record-start logical offset overflow".into(),
                            )
                        })?;
            }
            current = false;
            (&page[record_start_page::BODY..], false)
        } else if page.get(continuation_page::MARKER..continuation_page::BODY)
            == Some(CONTINUATION_MARKER)
        {
            if !current {
                return Err(CodecError::Malformed(
                    "Protein continuation page has no open record".into(),
                ));
            }
            (&page[continuation_page::BODY..], false)
        } else if page.get(terminal_page::MARKER..terminal_page::USED) == Some(TERMINAL_MARKER) {
            let used =
                usize::from(View::u16_le_at(page, terminal_page::USED).ok_or_else(|| {
                    CodecError::Malformed("Protein terminal used-byte count is truncated".into())
                })?);
            let payload = page
                .get(terminal_page::BODY..terminal_page::BODY + used)
                .ok_or_else(|| {
                    CodecError::Malformed("Protein terminal payload is truncated".into())
                })?;
            (payload, true)
        } else {
            return Err(CodecError::Malformed(
                "Protein page marker is unknown".into(),
            ));
        };
        if !current {
            let frame = RecordFrame {
                logical_offset,
                bytes: Vec::new(),
            };
            ctx.push_scoped_vec(scope, &mut records, frame, "Protein logical record frame")?;
            let frame = records
                .last_mut()
                .ok_or_else(|| CodecError::Malformed("Protein page has no record owner".into()))?;
            scope.with_storage(|| {
                ctx.extend_from_slice(
                    &mut frame.bytes,
                    RECORD_MARKER,
                    "Protein copied record range",
                )
            })?;
            current = true;
        }
        let frame = records
            .last_mut()
            .ok_or_else(|| CodecError::Malformed("Protein page has no record owner".into()))?;
        scope.with_storage(|| {
            ctx.extend_from_slice(&mut frame.bytes, payload, "Protein copied record range")
        })?;
        if terminal {
            logical_offset = logical_offset
                .checked_add(frame.bytes.len())
                .ok_or_else(|| {
                    CodecError::Malformed("Protein terminal logical offset overflow".into())
                })?;
            current = false;
        }
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    #[test]
    fn new_start_page_closes_the_open_record() {
        let mut bytes = vec![0_u8; crate::STREAM_HEADER_LEN + 2 * crate::PAGE_SIZE];
        let size_offset = crate::layout::instance_stream_header::DECLARED_SIZE;
        bytes[size_offset..size_offset + 4].copy_from_slice(
            &u32::try_from(crate::PAGE_SIZE)
                .expect("page size")
                .to_le_bytes(),
        );
        for (index, value) in [1_u8, 2].into_iter().enumerate() {
            let start = crate::STREAM_HEADER_LEN + index * crate::PAGE_SIZE;
            let page = &mut bytes[start..start + crate::PAGE_SIZE];
            let marker = crate::layout::record_start_page::MARKER;
            let body = crate::layout::record_start_page::BODY;
            page[marker..body].copy_from_slice(crate::RECORD_MARKER);
            page[body..].fill(value);
        }
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &bytes,
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .expect("root");
        let records = super::record_frames_admitted(&ctx, &bytes).expect("two start pages");
        let frames = records.frames();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].logical_offset(), 0);
        let body_len = crate::PAGE_SIZE - crate::layout::record_start_page::BODY;
        assert_eq!(
            frames[1].logical_offset(),
            crate::RECORD_MARKER.len() + body_len
        );
        assert_eq!(
            frames[0].bytes(),
            [
                crate::RECORD_MARKER,
                &[1_u8; crate::PAGE_SIZE - crate::layout::record_start_page::BODY]
            ]
            .concat()
        );
        assert_eq!(
            frames[1].bytes(),
            [
                crate::RECORD_MARKER,
                &[2_u8; crate::PAGE_SIZE - crate::layout::record_start_page::BODY]
            ]
            .concat()
        );
    }

    #[test]
    fn temporary_frames_release_their_live_storage() {
        let mut bytes = vec![0_u8; crate::STREAM_HEADER_LEN + crate::PAGE_SIZE];
        let offset = crate::layout::instance_stream_header::DECLARED_SIZE;
        bytes[offset..offset + 4].copy_from_slice(
            &u32::try_from(crate::PAGE_SIZE)
                .expect("page size")
                .to_le_bytes(),
        );
        let page = &mut bytes[crate::STREAM_HEADER_LEN..];
        page[..4].copy_from_slice(crate::TERMINAL_MARKER);
        page[4..6].copy_from_slice(&5_u16.to_le_bytes());
        let body = crate::layout::terminal_page::BODY;
        page[body..body + 5].copy_from_slice(b"frame");
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        // The frame vector has four slots. Its byte vector starts with eight
        // slots and doubles to sixteen for the marker and five-byte payload.
        let live = 4 * std::mem::size_of::<super::RecordFrame>() + 16;
        // Reallocation overlaps the old eight-byte vector with its new storage.
        let peak = live + 8;
        policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(peak);
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("root");
        let frames = super::record_frames_admitted(&ctx, &bytes)
            .expect("temporary frames use no retained allowance");
        assert_eq!(
            frames.frames()[0].bytes(),
            [crate::RECORD_MARKER, b"frame"].concat()
        );
        drop(frames);
        let reservation = ctx
            .reserve_scoped(
                cadmpeg_core::decode::u64_from_index(peak),
                "reuse frame storage",
            )
            .expect("frame owner released all scoped storage");
        drop(reservation);
        let frames =
            super::record_frames_admitted(&ctx, &bytes).expect("frame storage can be reused");
        let overlap = ctx
            .reserve_scoped(8, "unused growth overlap allowance")
            .expect("growth overlap is released while frames remain live");
        assert!(
            matches!(ctx.reserve_scoped(1, "probe live frame storage"), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
        );
        drop(overlap);
        drop(frames);
    }

    #[test]
    fn valid_terminal_page_refuses_zero_work_before_scanning() {
        let mut bytes = vec![0_u8; crate::STREAM_HEADER_LEN + crate::PAGE_SIZE];
        let offset = crate::layout::instance_stream_header::DECLARED_SIZE;
        bytes[offset..offset + 4].copy_from_slice(
            &u32::try_from(crate::PAGE_SIZE)
                .expect("page size")
                .to_le_bytes(),
        );
        bytes[crate::STREAM_HEADER_LEN..crate::STREAM_HEADER_LEN + crate::TERMINAL_MARKER.len()]
            .copy_from_slice(crate::TERMINAL_MARKER);
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (service, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &bytes,
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .expect("root");
        let frames = super::record_frames_admitted(&service, &bytes).expect("valid terminal page");
        assert_eq!(frames.frames().len(), 1);
        assert_eq!(frames.frames()[0].bytes(), crate::RECORD_MARKER);
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("root");
        let error =
            super::record_frames_admitted(&ctx, &bytes).expect_err("page work must be admitted");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == "Protein page framing scan")
        );
    }

    #[test]
    fn page_framing_admits_work_before_scanning_pages() {
        let mut bytes = vec![0_u8; crate::STREAM_HEADER_LEN + crate::PAGE_SIZE];
        let offset = crate::layout::instance_stream_header::DECLARED_SIZE;
        bytes[offset..offset + 4].copy_from_slice(
            &u32::try_from(crate::PAGE_SIZE)
                .expect("page size")
                .to_le_bytes(),
        );
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("root");
        let error = super::record_frames_admitted(&ctx, &bytes)
            .expect_err("scan must be admitted before marker checks");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
        );
    }
}
