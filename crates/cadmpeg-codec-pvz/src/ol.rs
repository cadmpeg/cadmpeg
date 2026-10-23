// SPDX-License-Identifier: Apache-2.0
//! `.ol` geometry members (§4): the header, the stream directory, the
//! preamble block, and the streams the decoder reads.

mod attributes;
mod edges;
mod surface;
mod tlv;

use std::collections::BTreeMap;

use cadmpeg_core::decode::{index_from_u32, u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use crate::layout::{ol_directory_entry as entry, ol_header, ol_preamble_block as preamble, token};
use crate::loss::PvzLossCode;
use crate::read::{at, record};
pub(crate) use attributes::{Appearance, AppearanceTable};
pub(crate) use surface::Mesh;

/// Leading member bytes that carry the geometry-member signature.
const SIGNATURE_LEN: usize = 10;
/// Directory bytes before the entries: the `entry_count` field.
const DIRECTORY_COUNT_LEN: u64 = 4;

/// Geometry-member generation named by the member's leading signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Generation {
    /// Signature `DIV-OL08BL`.
    Ol08,
    /// Signature `DIV-OL09BL`.
    Ol09,
}

impl Generation {
    const ALL: [Self; 2] = [Self::Ol08, Self::Ol09];

    /// The ASCII signature at the start of the member.
    pub(crate) const fn signature(self) -> &'static str {
        match self {
            Self::Ol08 => "DIV-OL08BL",
            Self::Ol09 => "DIV-OL09BL",
        }
    }
}

/// The generation whose signature opens `member`, if any.
pub(crate) fn generation(member: View<'_>) -> Option<Generation> {
    let mut cursor = member;
    let leading = cursor.array::<SIGNATURE_LEN>()?;
    Generation::ALL
        .into_iter()
        .find(|generation| generation.signature().as_bytes() == leading)
}

/// The stored streams of one member, in directory order.
struct Container<'a> {
    /// Whether the 19-byte preamble block precedes the first stream (§4.3).
    preamble: bool,
    streams: Vec<(u32, View<'a>)>,
}

impl<'a> Container<'a> {
    /// Reads the header, the directory, and the preamble block (§4.1–§4.3).
    fn open(member: View<'a>) -> Result<Self, CodecError> {
        if generation(member).is_none() {
            return Err(CodecError::malformed(
                "geometry member opens with no known DIV-OL signature",
            ));
        }
        let mut view = member;
        let header = record(&mut view, ol_header::LEN)?;
        let version = at(header, ol_header::VERSION)?.req_u32_le()?;
        if version != ol_header::VERSION_VALUE {
            return Err(CodecError::malformed(format_args!(
                "geometry member states version {version}, not {}",
                ol_header::VERSION_VALUE
            )));
        }
        let directory_length = at(header, ol_header::DIRECTORY_LENGTH)?.req_u32_le()?;
        let declared = at(header, ol_header::ENTRY_COUNT)?.req_u32_le()?;
        let entry_len = u64_from_index(entry::LEN);
        if Some(u64::from(directory_length))
            != u64::from(declared)
                .checked_mul(entry_len)
                .and_then(|len| len.checked_add(DIRECTORY_COUNT_LEN))
        {
            return Err(CodecError::malformed(format_args!(
                "geometry directory length {directory_length} does not fit {declared} entries"
            )));
        }
        let count = view
            .counted(u64::from(declared), entry::LEN)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "geometry member declares {declared} directory entries in {} bytes",
                    view.remaining()
                ))
            })?;
        let mut entries = Vec::with_capacity(count.get());
        for _ in 0..count.get() {
            let row = record(&mut view, entry::LEN)?;
            let id = at(row, entry::STREAM_ID)?.req_u32_le()?;
            let offset = index_from_u32(at(row, entry::OFFSET)?.req_u32_le()?);
            entries.push((id, offset));
        }
        let Some(&(sentinel, end)) = entries.last() else {
            return Err(CodecError::malformed(
                "geometry directory has no end sentinel",
            ));
        };
        let length = member.window().len();
        if sentinel != token::STREAM_END_SENTINEL || end != length {
            return Err(CodecError::malformed(format_args!(
                "geometry directory ends with stream {sentinel:#010x} at offset {end}, not the \
                 end sentinel at the member length {length}"
            )));
        }
        let directory_end = view.read_len();
        let first = entries.first().map_or(end, |(_, offset)| *offset);
        let has_preamble = first.checked_sub(directory_end) == Some(preamble::LEN);
        if has_preamble {
            let block = record(&mut view, preamble::LEN)?;
            let tag = at(block, preamble::TAG)?.req_u32_le()?;
            let reserved =
                at(block, preamble::RESERVED)?.req_take(preamble::RESERVED_VALUE.len())?;
            if tag != preamble::TAG_VALUE || reserved != preamble::RESERVED_VALUE {
                return Err(CodecError::malformed(format_args!(
                    "geometry preamble block at offset {directory_end} has tag {tag:#010x}"
                )));
            }
        } else if first != directory_end {
            return Err(CodecError::malformed(format_args!(
                "first geometry stream starts at offset {first}, not at the directory end \
                 {directory_end}"
            )));
        }
        let mut streams = Vec::with_capacity(entries.len());
        let sentinel_index = entries.len() - 1;
        for (index, pair) in entries.windows(2).enumerate() {
            let ((id, start), (next_id, stop)) = (pair[0], pair[1]);
            if index + 1 < sentinel_index && next_id <= id {
                return Err(CodecError::malformed(format_args!(
                    "geometry directory lists stream {next_id:#010x} after {id:#010x}"
                )));
            }
            let payload = member
                .start()
                .checked_add(start)
                .zip(member.start().checked_add(stop))
                .and_then(|(start, stop)| member.child(start, stop))
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "geometry stream {id:#010x} spans offsets {start}..{stop}"
                    ))
                })?;
            streams.push((id, payload));
        }
        Ok(Self {
            preamble: has_preamble,
            streams,
        })
    }

    /// The stored payload of stream `id`.
    fn stream(&self, id: u32) -> Option<View<'a>> {
        self.streams
            .iter()
            .find(|(stream, _)| *stream == id)
            .map(|(_, payload)| *payload)
    }
}

/// The exact-geometry stream of one member (§8.1), expanded.
pub(crate) struct ExactStream<'a> {
    /// `0x01000003` (`f32` scalars) or `0x01000004` (`f64` scalars).
    pub(crate) stream_id: u32,
    /// The expanded TLV records of the stream, from its stream index record
    /// to its end.
    pub(crate) payload: View<'a>,
}

/// Expands the exact-geometry stream of `member`, if it has one.
pub(crate) fn exact_stream<'a>(
    ctx: &DecodeContext<'a>,
    member: View<'_>,
) -> Result<Option<ExactStream<'a>>, CodecError> {
    let container = Container::open(member)?;
    let mut exact = container.streams.iter().filter(|(id, _)| is_exact(*id));
    let Some(&(stream_id, stored)) = exact.next() else {
        return Ok(None);
    };
    if exact.next().is_some() {
        return Err(CodecError::malformed(
            "geometry member carries more than one exact-geometry stream",
        ));
    }
    Ok(Some(ExactStream {
        stream_id,
        payload: crate::gzip::expand(ctx, stored)?,
    }))
}

fn is_exact(id: u32) -> bool {
    id == token::STREAM_EXACT_F32 || id == token::STREAM_EXACT_F64
}

/// The decoded display content of one geometry member.
#[derive(Debug)]
pub(crate) struct MemberGeometry {
    pub(crate) appearances: AppearanceTable,
    /// Surface tessellation, one mesh per appearance and vertex layout.
    pub(crate) meshes: Vec<Mesh>,
    /// Whether the member carries an exact-geometry stream.
    pub(crate) has_exact: bool,
    pub(crate) residue: MemberResidue,
}

/// Decodes the attribute, surface tessellation, and edge tessellation
/// streams of one member. A failed stream is recorded in the residue; only a
/// failed container refuses the member.
pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    member: View<'_>,
) -> Result<MemberGeometry, CodecError> {
    let container = Container::open(member)?;
    let mut residue = MemberResidue {
        preambles: usize::from(container.preamble),
        ..MemberResidue::default()
    };
    for (id, _) in &container.streams {
        let known = [
            token::STREAM_ATTRIBUTES,
            token::STREAM_SURFACE_TESSELLATION,
            token::STREAM_EDGE_TESSELLATION,
        ]
        .contains(id)
            || is_exact(*id);
        if !known {
            residue.unknown_streams.push(*id);
        }
    }
    let appearances = match container.stream(token::STREAM_ATTRIBUTES) {
        Some(stored) => read_stream(ctx, stored, &mut residue, "attribute", |stream, local| {
            attributes::read(stream, token::STREAM_ATTRIBUTES, local)
        })?
        .unwrap_or_default(),
        None => {
            residue
                .failed_streams
                .push("the attribute stream is absent".to_owned());
            AppearanceTable::default()
        }
    };
    let meshes = match container.stream(token::STREAM_SURFACE_TESSELLATION) {
        Some(stored) => read_stream(
            ctx,
            stored,
            &mut residue,
            "surface tessellation",
            |stream, local| {
                surface::read(
                    stream,
                    token::STREAM_SURFACE_TESSELLATION,
                    &appearances,
                    local,
                )
            },
        )?
        .unwrap_or_default(),
        None => Vec::new(),
    };
    if let Some(stored) = container.stream(token::STREAM_EDGE_TESSELLATION) {
        let polylines = read_stream(
            ctx,
            stored,
            &mut residue,
            "edge tessellation",
            |stream, local| edges::read(stream, token::STREAM_EDGE_TESSELLATION, local),
        )?;
        for polyline in polylines.into_iter().flatten() {
            residue.edge_polylines += 1;
            residue.edge_points += polyline.points.len();
            residue.edge_segments += polyline.segments.len();
        }
    }
    Ok(MemberGeometry {
        appearances,
        meshes,
        has_exact: container.streams.iter().any(|(id, _)| is_exact(*id)),
        residue,
    })
}

/// Expands and reads one stream. A resource refusal stays fatal; any other
/// failure is recorded, and only a stream that reads whole adds its residue.
fn read_stream<T>(
    ctx: &DecodeContext<'_>,
    stored: View<'_>,
    residue: &mut MemberResidue,
    label: &str,
    read: impl FnOnce(View<'_>, &mut MemberResidue) -> Result<T, CodecError>,
) -> Result<Option<T>, CodecError> {
    let mut local = MemberResidue::default();
    match crate::gzip::expand(ctx, stored).and_then(|stream| read(stream, &mut local)) {
        Ok(value) => {
            residue.merge(&local);
            Ok(Some(value))
        }
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(error) => {
            residue
                .failed_streams
                .push(format!("the {label} stream was not decoded: {error}"));
            Ok(None)
        }
    }
}

/// Records of one stream that the decoder does not transfer, by tag.
#[derive(Debug, Default)]
pub(crate) struct StreamTally(BTreeMap<u32, usize>);

impl StreamTally {
    /// Counts one record of `tag`.
    pub(crate) fn count(&mut self, tag: u32) {
        *self.0.entry(tag).or_default() += 1;
    }

    fn absorb(&mut self, other: &Self) {
        for (tag, count) in &other.0 {
            *self.0.entry(*tag).or_default() += count;
        }
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn describe(&self) -> String {
        self.0
            .iter()
            .map(|(tag, count)| format!("{tag:#x} x{count}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Geometry-member content the model does not carry, counted for loss notes.
#[derive(Debug, Default)]
pub(crate) struct MemberResidue {
    pub(crate) attribute_records: StreamTally,
    pub(crate) surface_records: StreamTally,
    pub(crate) edge_records: StreamTally,
    pub(crate) appearance_orphan_values: usize,
    pub(crate) appearance_redefinitions: usize,
    pub(crate) appearance_unresolved: usize,
    pub(crate) appearance_tails: usize,
    pub(crate) patch_headers: usize,
    pub(crate) texture_vertices: usize,
    pub(crate) edge_index_tables: usize,
    edge_polylines: usize,
    edge_points: usize,
    edge_segments: usize,
    preambles: usize,
    unknown_streams: Vec<u32>,
    /// Streams that were not decoded, with the reason.
    failed_streams: Vec<String>,
}

impl MemberResidue {
    /// Adds the counts of `other`.
    fn merge(&mut self, other: &Self) {
        self.attribute_records.absorb(&other.attribute_records);
        self.surface_records.absorb(&other.surface_records);
        self.edge_records.absorb(&other.edge_records);
        self.appearance_orphan_values += other.appearance_orphan_values;
        self.appearance_redefinitions += other.appearance_redefinitions;
        self.appearance_unresolved += other.appearance_unresolved;
        self.appearance_tails += other.appearance_tails;
        self.patch_headers += other.patch_headers;
        self.texture_vertices += other.texture_vertices;
        self.edge_index_tables += other.edge_index_tables;
        self.edge_polylines += other.edge_polylines;
        self.edge_points += other.edge_points;
        self.edge_segments += other.edge_segments;
        self.preambles += other.preambles;
    }
}

/// The residue of every geometry member of a package, for package-level
/// loss notes.
#[derive(Debug, Default)]
pub(crate) struct PackageResidue {
    total: MemberResidue,
    members: usize,
    /// `(member, reason)` for each stream that was not decoded.
    failures: Vec<(String, String)>,
    /// Members with unknown stream identities.
    unknown_streams: Vec<(String, Vec<u32>)>,
}

impl PackageResidue {
    /// Adds the residue of member `name`.
    pub(crate) fn absorb(&mut self, name: &str, residue: &MemberResidue) {
        self.members += 1;
        self.total.merge(residue);
        for reason in &residue.failed_streams {
            self.failures.push((name.to_owned(), reason.clone()));
        }
        if !residue.unknown_streams.is_empty() {
            self.unknown_streams
                .push((name.to_owned(), residue.unknown_streams.clone()));
        }
    }

    /// One loss note per kind of geometry-member content the model does not
    /// carry.
    pub(crate) fn losses(&self) -> Vec<LossNote> {
        let total = &self.total;
        let mut losses = Vec::new();
        for (member, reason) in &self.failures {
            losses.push(
                PvzLossCode::GeometryStreamUndecoded
                    .note(format!("geometry member {member:?}: {reason}")),
            );
        }
        for (member, ids) in &self.unknown_streams {
            let ids = ids
                .iter()
                .map(|id| format!("{id:#010x}"))
                .collect::<Vec<_>>()
                .join(", ");
            losses.push(PvzLossCode::GeometryStreamUndecoded.note(format!(
                "geometry member {member:?} carries streams of undefined identity {ids} (PVZ-28)"
            )));
        }
        if !total.attribute_records.is_empty() {
            losses.push(PvzLossCode::AttributeRecordsUndecoded.note(format!(
                "attribute-stream records outside the appearance records were not transferred: \
                 {} (feature identifiers, layers, and per-face attributes; PVZ-13)",
                total.attribute_records.describe()
            )));
        }
        let appearance_defects = total.appearance_orphan_values
            + total.appearance_redefinitions
            + total.appearance_unresolved;
        if appearance_defects > 0 {
            losses.push(PvzLossCode::AttributeAppearanceUnresolved.note(format!(
                "{} appearance values without a following reference, {} repeated definitions \
                 of one base index, and {} patch appearances with no defined appearance were \
                 not transferred",
                total.appearance_orphan_values,
                total.appearance_redefinitions,
                total.appearance_unresolved
            )));
        }
        if total.appearance_tails > 0 {
            losses.push(PvzLossCode::AttributeAppearanceTail.note(format!(
                "{} appearance references carry a 17-byte tail whose meaning is an open item \
                 (PVZ-12); the tails were not transferred",
                total.appearance_tails
            )));
        }
        if total.patch_headers > 0 {
            losses.push(PvzLossCode::TessellationPatchFields.note(format!(
                "{} tessellation patch headers carry face_ref, flag, and refs fields whose \
                 meaning is an open item (PVZ-14); the fields were not transferred",
                total.patch_headers
            )));
        }
        if !total.surface_records.is_empty() {
            losses.push(PvzLossCode::TessellationRecordsUndecoded.note(format!(
                "surface-tessellation records outside the patch, vertex, and triangle records \
                 were not transferred: {} (PVZ-15)",
                total.surface_records.describe()
            )));
        }
        if total.texture_vertices > 0 {
            losses.push(PvzLossCode::TessellationTextureCoordinates.note(format!(
                "{} tessellation vertices carry texture coordinates; the texture binding is an \
                 open item (PVZ-17), so the coordinates were not transferred",
                total.texture_vertices
            )));
        }
        if total.edge_polylines > 0 || total.edge_index_tables > 0 || !total.edge_records.is_empty()
        {
            losses.push(PvzLossCode::TessellationEdgesNotTransferred.note(format!(
                "edge tessellation was decoded but not transferred: {} edge polylines with {} \
                 points and {} segments, {} edge index tables, and other records {}; the model \
                 carries no edge display polylines (PVZ-16)",
                total.edge_polylines,
                total.edge_points,
                total.edge_segments,
                total.edge_index_tables,
                if total.edge_records.is_empty() {
                    "none".to_owned()
                } else {
                    total.edge_records.describe()
                }
            )));
        }
        if total.preambles > 0 {
            losses.push(PvzLossCode::GeometryPreambleUndecoded.note(format!(
                "{} of {} geometry members carry the 19-byte preamble block whose fields are an \
                 open item (PVZ-11); the block was not transferred",
                total.preambles, self.members
            )));
        }
        losses
    }
}

#[cfg(test)]
mod tests;
