// SPDX-License-Identifier: Apache-2.0
//! Persistent polyedge-reference construction decoding.
//!
//! Migrated module (doc section 10 Phase 2). The record body and every child
//! segment are read through a bounded [`View`] over a codec-local
//! [`DecodeContext`]: the count-framed parameter table and the segment list are
//! sized by [`View::counted`] physical-plausibility proofs and reserved through
//! [`DecodeContext::exact_vec`], committed scalar reads use the `req_*` mirror,
//! and per-element `work` is charged against the platform budget. The residual
//! framing boundary is `chunk_at`/`parse_class_wrapper`, which return typed
//! ranges (not byte slices) that locate each child record's window before the
//! `View` takes over; there is no `View::window()` egress. The module carries
//! the graduated `deny(clippy::disallowed_methods)`.
#![deny(clippy::disallowed_methods)]

use std::ops::Range;

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::decode::{DecodeArena, DecodeContext, DecodePolicy, View};

use crate::chunks::{chunk_at, ArchiveVersion, FramingError};
use crate::objects::parse_class_wrapper;
use crate::wire::Uuid;

const ANONYMOUS: u32 = 0x4000_8000;
const ITEM_CAP: usize = 1 << 20;
pub(crate) const CURVE_CLASS: Uuid = Uuid::from_canonical([
    0x39, 0xff, 0x3d, 0xd3, 0xfe, 0x0f, 0x48, 0x07, 0x9d, 0x59, 0x18, 0x5f, 0x0d, 0x73, 0xc0, 0xe4,
]);
const SEGMENT_CLASS: Uuid = Uuid::from_canonical([
    0x42, 0xf4, 0x7a, 0x87, 0x5b, 0x1b, 0x4e, 0x31, 0xab, 0x87, 0x46, 0x39, 0xd7, 0x83, 0x25, 0xd6,
]);

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Segment {
    pub(crate) object_id: Uuid,
    pub(crate) component: [i32; 2],
    pub(crate) edge_domain: [f64; 2],
    pub(crate) trim_domain: [f64; 2],
    pub(crate) reversed: bool,
    pub(crate) domain: [f64; 2],
    pub(crate) proxy_domain: [f64; 2],
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PolyEdge {
    pub(crate) parameters: Vec<f64>,
    pub(crate) segments: Vec<Segment>,
}

fn malformed(offset: usize, message: impl Into<String>) -> FramingError {
    FramingError::Structural {
        offset,
        message: message.into(),
    }
}

/// Maps a budget refusal from the platform context onto the module's framing
/// error. The codec-local `ITEM_CAP` bounds each count well below any default
/// budget ceiling, so a refusal here means the reserved size was implausible for
/// the surrounding window.
fn refused(offset: usize, error: &CodecError) -> FramingError {
    malformed(offset, format!("polyedge allocation refused: {error}"))
}

/// Reads one committed `u8` through the `req_*` mirror.
fn req_u8(view: &mut View<'_>) -> Result<u8, FramingError> {
    let offset = view.position();
    view.req_u8()
        .map_err(|_| malformed(offset, "polyedge record truncated"))
}

/// Reads one committed `i32` through the `req_*` mirror.
fn req_i32(view: &mut View<'_>) -> Result<i32, FramingError> {
    let offset = view.position();
    view.req_i32_le()
        .map_err(|_| malformed(offset, "polyedge record truncated"))
}

/// Reads one committed `f64` through the `req_*` mirror.
fn req_f64(view: &mut View<'_>) -> Result<f64, FramingError> {
    let offset = view.position();
    view.req_f64_le()
        .map_err(|_| malformed(offset, "polyedge record truncated"))
}

/// Reads a committed 16-byte UUID through the `req_*` mirror.
fn req_uuid(view: &mut View<'_>) -> Result<Uuid, FramingError> {
    let offset = view.position();
    let bytes = view
        .req_take(16)
        .map_err(|_| malformed(offset, "polyedge record truncated"))?;
    Ok(Uuid::from_wire(bytes.try_into().expect("length checked")))
}

/// Reads an archive boolean encoded as one byte through the `req_*` mirror.
fn req_bool(view: &mut View<'_>) -> Result<bool, FramingError> {
    let offset = view.position();
    match req_u8(view)? {
        0 => Ok(false),
        1 => Ok(true),
        value => Err(malformed(
            offset,
            format!("boolean value {value} is not 0 or 1"),
        )),
    }
}

/// Reads a committed 32-bit count and proves it against the remaining window as
/// a [`BoundedCount`] over `width`-byte elements under the codec-local
/// `ITEM_CAP`.
///
/// [`BoundedCount`]: cadmpeg_ir::decode::BoundedCount
fn counted(
    view: &mut View<'_>,
    width: usize,
) -> Result<(usize, cadmpeg_ir::decode::BoundedCount), FramingError> {
    let offset = view.position();
    let value = req_i32(view)?;
    let count = usize::try_from(value).map_err(|_| FramingError::Overflow { offset })?;
    if count > ITEM_CAP {
        return Err(malformed(offset, "polyedge count exceeds cap"));
    }
    let bound = view
        .counted(count as u64, width)
        .ok_or_else(|| malformed(offset, "polyedge count exceeds remaining window"))?;
    Ok((count, bound))
}

fn interval(view: &mut View<'_>) -> Result<[f64; 2], FramingError> {
    let offset = view.position();
    let value = [req_f64(view)?, req_f64(view)?];
    if value.iter().all(|value| value.is_finite()) {
        Ok(value)
    } else {
        Err(malformed(offset, "polyedge interval is not finite"))
    }
}

fn segment(
    root: View<'_>,
    ctx: &DecodeContext<'_>,
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
) -> Result<Segment, FramingError> {
    let chunk = chunk_at(data, range.start, range.end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short || chunk.next_offset != range.end {
        return Err(malformed(range.start, "invalid polyedge-segment framing"));
    }
    let mut body = root
        .child(chunk.body.start, chunk.body.end)
        .ok_or_else(|| malformed(chunk.body.start, "polyedge segment body out of range"))?;
    // A polyedge segment is a fixed field layout (no count-framed collection);
    // charge one work unit per committed scalar field read.
    ctx.charge_work(13, "polyedge_segment", Some(body.location()))
        .map_err(|error| refused(chunk.body.start, &error))?;
    if req_i32(&mut body)? != 1 || req_i32(&mut body)? != 0 {
        return Err(malformed(
            chunk.body.start,
            "unsupported polyedge-segment version",
        ));
    }
    let object_id = req_uuid(&mut body)?;
    let component = [req_i32(&mut body)?, req_i32(&mut body)?];
    let edge_domain = interval(&mut body)?;
    let trim_domain = interval(&mut body)?;
    let reversed = req_bool(&mut body)?;
    let domain = interval(&mut body)?;
    let proxy_domain = interval(&mut body)?;
    if body.remaining() != 0 {
        return Err(malformed(
            body.position(),
            "polyedge segment has trailing bytes",
        ));
    }
    Ok(Segment {
        object_id,
        component,
        edge_domain,
        trim_domain,
        reversed,
        domain,
        proxy_domain,
    })
}

pub(crate) fn decode(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
) -> Result<PolyEdge, FramingError> {
    // Read the record body through a bounded `View` on a codec-local context.
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(data, &arena, &policy)
        .map_err(|error| refused(range.start, &error))?;
    let mut body = root
        .child(range.start, range.end)
        .ok_or_else(|| malformed(range.start, "polyedge body out of range"))?;

    let version = req_u8(&mut body)?;
    if version >> 4 != 1 {
        return Err(malformed(range.start, "unsupported polyedge-curve version"));
    }
    let (segment_count, segment_bound) = counted(&mut body, 1)?;
    if segment_count == 0 {
        return Err(malformed(body.position(), "polyedge curve has no segments"));
    }
    req_i32(&mut body)?;
    req_i32(&mut body)?;
    body.skip(48)
        .ok_or_else(|| malformed(body.position(), "polyedge record truncated"))?;
    let (parameter_count, parameter_bound) = counted(&mut body, 8)?;
    if parameter_count != segment_count + 1 {
        return Err(malformed(
            body.position(),
            "polyedge parameter count mismatch",
        ));
    }

    // Count-framed parameter table: reserve exactly `parameter_count` scalars.
    ctx.charge_work(
        parameter_count as u64,
        "polyedge_parameters",
        Some(body.location()),
    )
    .map_err(|error| refused(body.position(), &error))?;
    let mut reserved = ctx
        .exact_vec::<f64>(parameter_bound)
        .map_err(|error| refused(body.position(), &error))?;
    let mut previous: Option<f64> = None;
    for _ in 0..parameter_count {
        let offset = body.position();
        let value = req_f64(&mut body)?;
        if !value.is_finite() || previous.is_some_and(|last| value <= last) {
            return Err(malformed(offset, "invalid polyedge parameter"));
        }
        previous = Some(value);
        reserved
            .push(value)
            .map_err(|error| refused(body.position(), &error))?;
    }
    let parameters = reserved
        .finish_exact()
        .map_err(|error| refused(body.position(), &error))?;

    // Count-framed segment list: reserve exactly `segment_count` segments; each
    // child record is located by `chunk_at`/`parse_class_wrapper` framing and
    // read through a child `View`.
    let mut segments = ctx
        .exact_vec::<Segment>(segment_bound)
        .map_err(|error| refused(body.position(), &error))?;
    for _ in 0..segment_count {
        let start = body.position();
        let wrapper = chunk_at(data, start, range.end, archive, false)?;
        let class =
            parse_class_wrapper(data, start..wrapper.next_offset, archive, &mut Vec::new())?;
        if class.class_uuid != SEGMENT_CLASS {
            return Err(malformed(
                start,
                "polyedge child is not a persistent segment",
            ));
        }
        segments
            .push(segment(root, &ctx, data, class.class_data_range, archive)?)
            .map_err(|error| refused(body.position(), &error))?;
        body.skip(wrapper.next_offset - start)
            .ok_or_else(|| malformed(body.position(), "polyedge segment overruns body"))?;
    }
    if body.remaining() != 0 {
        return Err(malformed(
            body.position(),
            "polyedge curve has trailing bytes",
        ));
    }
    let segments = segments
        .finish_exact()
        .map_err(|error| refused(body.position(), &error))?;
    Ok(PolyEdge {
        parameters,
        segments,
    })
}

pub(crate) fn semantic_json(polyedge: &PolyEdge) -> Option<String> {
    let segments = polyedge
        .segments
        .iter()
        .map(|segment| {
            serde_json::json!({
                "object_id": segment.object_id.to_string(),
                "component": segment.component,
                "edge_domain": segment.edge_domain,
                "trim_domain": segment.trim_domain,
                "reversed": segment.reversed,
                "domain": segment.domain,
                "proxy_domain": segment.proxy_domain,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&serde_json::json!({
        "kind": "polyedge_reference",
        "parameters": polyedge.parameters,
        "segments": segments,
    }))
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive_test_support::{class_wrapper, crc_chunk};

    fn polyedge_payload() -> Vec<u8> {
        let mut segment = 1_i32.to_le_bytes().to_vec();
        segment.extend(0_i32.to_le_bytes());
        segment.extend([0_u8; 15]);
        segment.push(9);
        segment.extend(2_i32.to_le_bytes());
        segment.extend(17_i32.to_le_bytes());
        for value in [0.0_f64, 4.0, 1.0, 3.0] {
            segment.extend(value.to_le_bytes());
        }
        segment.push(1);
        for value in [10.0_f64, 20.0, 2.0, 6.0] {
            segment.extend(value.to_le_bytes());
        }
        let segment = crc_chunk(ANONYMOUS, &segment);
        let segment_class = [
            0x87, 0x7a, 0xf4, 0x42, 0x1b, 0x5b, 0x31, 0x4e, 0xab, 0x87, 0x46, 0x39, 0xd7, 0x83,
            0x25, 0xd6,
        ];

        let mut payload = vec![0x10];
        payload.extend(1_i32.to_le_bytes());
        payload.extend(0_i32.to_le_bytes());
        payload.extend(0_i32.to_le_bytes());
        payload.extend([0_u8; 48]);
        payload.extend(2_i32.to_le_bytes());
        payload.extend(0.0_f64.to_le_bytes());
        payload.extend(10.0_f64.to_le_bytes());
        payload.extend(class_wrapper(segment_class, &segment));
        payload
    }

    #[test]
    fn decodes_persistent_polyedge_segment_construction() {
        let payload = polyedge_payload();
        let decoded = decode(&payload, 0..payload.len(), ArchiveVersion::V8).unwrap();
        assert_eq!(decoded.parameters, [0.0, 10.0]);
        assert_eq!(decoded.segments[0].component, [2, 17]);
        assert_eq!(decoded.segments[0].edge_domain, [0.0, 4.0]);
        assert_eq!(decoded.segments[0].trim_domain, [1.0, 3.0]);
        assert!(decoded.segments[0].reversed);
        assert_eq!(decoded.segments[0].domain, [10.0, 20.0]);
        assert_eq!(decoded.segments[0].proxy_domain, [2.0, 6.0]);
    }

    #[test]
    fn truncating_the_segment_child_is_rejected_at_the_record_boundary() {
        // Drop the trailing bytes of the child segment record so the
        // count-framed segment loop runs past the body's proven window.
        let mut payload = polyedge_payload();
        payload.truncate(payload.len() - 16);
        assert!(decode(&payload, 0..payload.len(), ArchiveVersion::V8).is_err());
    }
}
