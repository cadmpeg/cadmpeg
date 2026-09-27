// SPDX-License-Identifier: Apache-2.0
//! Persistent polyedge-reference construction decoding.
#![deny(clippy::disallowed_methods)]

use crate::loss::Diagnostics;
use std::io::{self, Write};
use std::ops::Range;

use cadmpeg_core::decode::{
    u64_from_index, DecodeContext, ResourceDimension, ResourceFailure, ResourceLimit, View,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::units::FiniteVector;
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};

use crate::mesh::MeshExpand;

use crate::chunks::{chunk_at, ArchiveVersion, FramingError};
use crate::objects::parse_class_wrapper;
use crate::wire::{ExactVec, Uuid};

const ANONYMOUS: u32 = 0x4000_8000;
const ITEM_CAP: usize = 1 << 20;

/// Lower bound on one polyedge segment's on-disk size (97-byte body layout
/// before the anonymous chunk header; see [`segment`]).
const MIN_SEGMENT_BYTES: usize = 97;
pub(crate) const CURVE_CLASS: Uuid = Uuid::from_canonical([
    0x39, 0xff, 0x3d, 0xd3, 0xfe, 0x0f, 0x48, 0x07, 0x9d, 0x59, 0x18, 0x5f, 0x0d, 0x73, 0xc0, 0xe4,
]);
const SEGMENT_CLASS: Uuid = Uuid::from_canonical([
    0x42, 0xf4, 0x7a, 0x87, 0x5b, 0x1b, 0x4e, 0x31, 0xab, 0x87, 0x46, 0x39, 0xd7, 0x83, 0x25, 0xd6,
]);

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Segment<R, D = [f64; 2]> {
    pub(crate) reference: R,
    pub(crate) reversed: bool,
    pub(crate) domain: D,
    pub(crate) proxy_domain: D,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EdgeDomains<D = [f64; 2]> {
    pub(crate) edge: D,
    pub(crate) trim: D,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PersistentReference {
    pub(crate) object_id: Uuid,
    component: [i32; 2],
    domains: EdgeDomains<FiniteVector<2>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HistoryReference {
    pub(crate) curve: crate::history::ObjectReference,
    pub(crate) sub_domain: [f64; 2],
    pub(crate) domains: Option<EdgeDomains>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PolyEdge<R, P = f64, D = [f64; 2]> {
    pub(crate) parameters: Vec<P>,
    pub(crate) segments: Vec<Segment<R, D>>,
}

pub(crate) type PersistentPolyEdge = PolyEdge<PersistentReference, FiniteReal, FiniteVector<2>>;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HistoryPolyEdge {
    pub(crate) polyedge: PolyEdge<HistoryReference>,
    pub(crate) evaluation_mode: i32,
}

fn refused(offset: usize, error: &CodecError) -> FramingError {
    match error {
        CodecError::ResourceLimit(limit) => FramingError::Resource(*limit),
        _ => FramingError::structural(offset, format!("polyedge allocation refused: {error}")),
    }
}

fn req_u8(view: &mut View<'_>) -> Result<u8, FramingError> {
    let offset = view.position();
    view.req_u8()
        .map_err(|_| FramingError::structural(offset, "polyedge record truncated"))
}

fn req_i32(view: &mut View<'_>) -> Result<i32, FramingError> {
    let offset = view.position();
    view.req_i32_le()
        .map_err(|_| FramingError::structural(offset, "polyedge record truncated"))
}

fn req_f64(view: &mut View<'_>) -> Result<f64, FramingError> {
    let offset = view.position();
    view.req_f64_le()
        .map_err(|_| FramingError::structural(offset, "polyedge record truncated"))
}

fn req_uuid(view: &mut View<'_>) -> Result<Uuid, FramingError> {
    let offset = view.position();
    let bytes = view
        .array()
        .ok_or_else(|| FramingError::structural(offset, "polyedge record truncated"))?;
    Ok(Uuid::from_wire(bytes))
}

fn req_bool(view: &mut View<'_>) -> Result<bool, FramingError> {
    let offset = view.position();
    match req_u8(view)? {
        0 => Ok(false),
        1 => Ok(true),
        value => Err(FramingError::structural(
            offset,
            format!("boolean value {value} is not 0 or 1"),
        )),
    }
}

/// Reads a committed 32-bit count and proves it against the remaining window as
/// a [`BoundedCount`] over `width`-byte elements under the codec-local
/// `ITEM_CAP`.
///
/// [`BoundedCount`]: cadmpeg_core::decode::BoundedCount
fn counted(
    view: &mut View<'_>,
    width: usize,
) -> Result<(usize, cadmpeg_core::decode::BoundedCount), FramingError> {
    let offset = view.position();
    let value = req_i32(view)?;
    let count = usize::try_from(value).map_err(|_| FramingError::Overflow { offset })?;
    if count > ITEM_CAP {
        return Err(FramingError::structural(
            offset,
            "polyedge count exceeds cap",
        ));
    }
    let bound = view.counted(count as u64, width).ok_or_else(|| {
        FramingError::structural(offset, "polyedge count exceeds remaining window")
    })?;
    Ok((count, bound))
}

fn interval(view: &mut View<'_>) -> Result<FiniteVector<2>, FramingError> {
    let offset = view.position();
    let value = [req_f64(view)?, req_f64(view)?];
    FiniteVector::new(value)
        .ok_or_else(|| FramingError::structural(offset, "polyedge interval is not finite"))
}

fn segment(
    root: View<'_>,
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
) -> Result<Segment<PersistentReference, FiniteVector<2>>, FramingError> {
    let chunk = chunk_at(data, range.start, range.end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short() {
        return Err(FramingError::structural(
            range.start,
            "invalid polyedge-segment framing",
        ));
    }
    let mut body = root
        .child(chunk.body().start, chunk.body().end)
        .ok_or_else(|| {
            FramingError::structural(chunk.body().start, "polyedge segment body out of range")
        })?;
    if req_i32(&mut body)? != 1 || req_i32(&mut body)? < 0 {
        return Err(FramingError::structural(
            chunk.body().start,
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
    let remaining = body.remaining();
    body.skip(remaining).ok_or_else(|| {
        FramingError::structural(body.position(), "polyedge segment suffix overruns body")
    })?;
    Ok(Segment {
        reference: PersistentReference {
            object_id,
            component,
            domains: EdgeDomains {
                edge: edge_domain,
                trim: trim_domain,
            },
        },
        reversed,
        domain,
        proxy_domain,
    })
}

pub(crate) fn decode(
    expand: MeshExpand<'_>,
    range: Range<usize>,
    archive: ArchiveVersion,
) -> Result<PersistentPolyEdge, FramingError> {
    let data = expand.data();
    let mut body = expand
        .root()
        .child(range.start, range.end)
        .ok_or_else(|| FramingError::structural(range.start, "polyedge body out of range"))?;

    let version = req_u8(&mut body)?;
    if version >> 4 != 1 {
        return Err(FramingError::structural(
            range.start,
            "unsupported polyedge-curve version",
        ));
    }
    let (segment_count, segment_bound) = counted(&mut body, MIN_SEGMENT_BYTES)?;
    req_i32(&mut body)?;
    req_i32(&mut body)?;
    body.skip(48)
        .ok_or_else(|| FramingError::structural(body.position(), "polyedge record truncated"))?;
    let (parameter_count, parameter_bound) = counted(&mut body, 8)?;

    let mut reserved =
        ExactVec::<FiniteReal>::new(expand.ctx(), parameter_bound, "Rhino polyedge parameters")
            .map_err(|error| refused(body.position(), &error))?;
    let mut previous: Option<FiniteReal> = None;
    for _ in 0..parameter_count {
        let offset = body.position();
        let value = req_f64(&mut body)?;
        let Some(value) = FiniteReal::new(value) else {
            return Err(FramingError::structural(
                offset,
                "invalid polyedge parameter",
            ));
        };
        if previous.is_some_and(|last| value.get() <= last.get()) {
            return Err(FramingError::structural(
                offset,
                "invalid polyedge parameter",
            ));
        }
        previous = Some(value);
        reserved
            .push(value)
            .map_err(|error| refused(body.position(), &error))?;
    }
    let parameters = reserved
        .finish()
        .map_err(|error| refused(body.position(), &error))?;

    let mut segments = ExactVec::<Segment<PersistentReference, FiniteVector<2>>>::new(
        expand.ctx(),
        segment_bound,
        "Rhino polyedge segments",
    )
    .map_err(|error| refused(body.position(), &error))?;
    for _ in 0..segment_count {
        let start = body.position();
        let wrapper = chunk_at(data, start, range.end, archive, false)?;
        let class = parse_class_wrapper(
            data,
            start..wrapper.next_offset(),
            archive,
            &mut Diagnostics::new(),
        )?;
        if class.class_uuid != SEGMENT_CLASS {
            return Err(FramingError::structural(
                start,
                "polyedge child is not a persistent segment",
            ));
        }
        segments
            .push(segment(
                expand.root(),
                data,
                class.class_data_range,
                archive,
            )?)
            .map_err(|error| refused(body.position(), &error))?;
        body.skip(wrapper.next_offset() - start).ok_or_else(|| {
            FramingError::structural(body.position(), "polyedge segment overruns body")
        })?;
    }
    let remaining = body.remaining();
    body.skip(remaining).ok_or_else(|| {
        FramingError::structural(body.position(), "polyedge suffix overruns body")
    })?;
    let segments = segments
        .finish()
        .map_err(|error| refused(body.position(), &error))?;
    Ok(PolyEdge {
        parameters,
        segments,
    })
}

const SEMANTIC_JSON_OPERATION: &str = "Rhino polyedge semantic JSON";

struct SemanticJson<'a>(&'a PersistentPolyEdge);

impl Serialize for SemanticJson<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("kind", "polyedge_reference")?;
        map.serialize_entry("parameters", &self.0.parameters)?;
        map.serialize_entry("segments", &SemanticSegments(&self.0.segments))?;
        map.end()
    }
}

struct SemanticSegments<'a>(&'a [Segment<PersistentReference, FiniteVector<2>>]);

impl Serialize for SemanticSegments<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for segment in self.0 {
            sequence.serialize_element(&SemanticSegment(segment))?;
        }
        sequence.end()
    }
}

struct SemanticSegment<'a>(&'a Segment<PersistentReference, FiniteVector<2>>);

impl Serialize for SemanticSegment<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(7))?;
        map.serialize_entry("component", &self.0.reference.component)?;
        map.serialize_entry("domain", &self.0.domain)?;
        map.serialize_entry("edge_domain", &self.0.reference.domains.edge)?;
        map.serialize_entry("object_id", &SemanticUuid(self.0.reference.object_id))?;
        map.serialize_entry("proxy_domain", &self.0.proxy_domain)?;
        map.serialize_entry("reversed", &self.0.reversed)?;
        map.serialize_entry("trim_domain", &self.0.reference.domains.trim)?;
        map.end()
    }
}

struct SemanticUuid(Uuid);

impl Serialize for SemanticUuid {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

struct SemanticJsonWriter<'a, 'b> {
    ctx: &'a DecodeContext<'b>,
    bytes: Vec<u8>,
    refusal: Option<CodecError>,
}

impl Write for SemanticJsonWriter<'_, '_> {
    fn write(&mut self, chunk: &[u8]) -> io::Result<usize> {
        let result = self
            .ctx
            .charge_retained(u64_from_index(chunk.len()), SEMANTIC_JSON_OPERATION)
            .and_then(|()| {
                self.bytes.try_reserve(chunk.len()).map_err(|_| {
                    CodecError::ResourceLimit(ResourceLimit {
                        dimension: ResourceDimension::RetainedBytes,
                        reason: ResourceFailure::AllocationFailed,
                        limit: u64::MAX,
                        used: 0,
                        additional: u64_from_index(chunk.len()),
                        operation: SEMANTIC_JSON_OPERATION,
                    })
                })
            });
        if let Err(error) = result {
            self.refusal = Some(error);
            return Err(io::ErrorKind::Other.into());
        }
        self.bytes.extend_from_slice(chunk);
        Ok(chunk.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn semantic_json(
    ctx: &DecodeContext<'_>,
    polyedge: &PersistentPolyEdge,
) -> Result<Option<String>, CodecError> {
    let mut writer = SemanticJsonWriter {
        ctx,
        bytes: Vec::new(),
        refusal: None,
    };
    let serialized = serde_json::to_writer(&mut writer, &SemanticJson(polyedge));
    if let Some(refusal) = writer.refusal {
        return Err(refusal);
    }
    Ok(serialized
        .ok()
        .and_then(|()| String::from_utf8(writer.bytes).ok()))
}

#[cfg(test)]
pub(crate) mod tests;
