// SPDX-License-Identifier: Apache-2.0
//! Decode `TSplines.BlobParts/*.tsm` Form control cages.

use cadmpeg_core::decode::index_from_u32;

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};
use cadmpeg_ir::subd::{
    SubdEdge, SubdEdgeTag, SubdEdgeUse, SubdFace, SubdGripDirection, SubdGripWedge, SubdPlaneFrame,
    SubdRadialMapSelector, SubdRadialSymmetryMap, SubdScheme, SubdSecondaryGrip, SubdSurface,
    SubdSymmetry, SubdSymmetryKind, SubdVertex, SubdVertexGripLayout, SubdVertexTag,
};
use cadmpeg_ir::SourceObjectAssociation;

use crate::container::ContainerScan;
use crate::loss::F3dLossCode;

const ENTRY_MARKER: &str = "/TSplines.BlobParts/";
const CAGE_COORDINATE_SCALE: f64 = 10.0;
const FULL_CREASE_SHARPNESS: f64 = 1.0;
const EDGE_KNOT_MIRROR_RELATIVE_EPS: f64 = 1.0e-12;
const SYMMETRY_FRAME_EPS: f64 = 1.0e-9;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct HalfEdgeId(usize);

impl cadmpeg_core::decode::cost::DecodeCost for HalfEdgeId {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
}

#[derive(Clone, Copy)]
struct HalfEdge {
    next: HalfEdgeId,
    previous: HalfEdgeId,
    mate: HalfEdgeId,
    vertex: usize,
    face: Option<usize>,
}

impl HalfEdgeId {
    fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy)]
struct ParsedHalfEdge {
    next: usize,
    previous: usize,
    mate: usize,
    vertex: usize,
    face: i64,
}

fn compact_half_edges(
    ctx: &DecodeContext<'_>,
    name: &str,
    slots: &[Option<ParsedHalfEdge>],
    face_roots: &mut [Option<usize>],
    edge_roots: &mut [Option<usize>],
    vertex_roots: &mut [Option<(usize, SubdGripDirection)>],
) -> Result<Vec<HalfEdge>, CodecError> {
    let mut map = ctx.alloc_filled(slots.len(), None, "map T-spline half-edge slots")?;
    let mut dense = Vec::new();
    for (old, half) in ctx
        .admit_iter(slots, "compact T-spline half-edge slots")?
        .enumerate()
    {
        if let Some(half) = half {
            map[old] = Some(HalfEdgeId(dense.len()));
            ctx.push_vec(&mut dense, *half, "collect T-spline live half-edges")?;
        }
    }
    let remap = |index: usize| {
        map.get(index)
            .copied()
            .flatten()
            .ok_or_else(|| malformed(ctx, name, "half-edge names a deleted slot"))
    };
    let mut half_edges = Vec::new();
    for half in ctx.admit_iter(&dense, "resolve T-spline compact half-edges")? {
        let resolved = HalfEdge {
            next: remap(half.next)?,
            previous: remap(half.previous)?,
            mate: remap(half.mate)?,
            vertex: half.vertex,
            face: match half.face {
                -1 => None,
                face => Some(usize::try_from(face).map_err(|_| {
                    malformed(ctx, name, "half-edge face is negative or overflows")
                })?),
            },
        };
        ctx.push_vec(
            &mut half_edges,
            resolved,
            "collect T-spline compact half-edges",
        )?;
    }
    for root in face_roots.iter_mut().flatten() {
        *root = remap(*root)?.index();
    }
    for root in edge_roots.iter_mut().flatten() {
        *root = remap(*root)?.index();
    }
    for root in vertex_roots.iter_mut().flatten() {
        root.0 = remap(root.0)?.index();
    }
    Ok(half_edges)
}

struct UntypedRecords<'a>(&'a BTreeMap<String, usize>);

impl std::fmt::Display for UntypedRecords<'_> {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self.0.values().sum::<usize>();
        write!(
            out,
            "{count} T-spline record(s) were retained without typed semantics: "
        )?;
        for (index, (kind, count)) in self.0.iter().enumerate() {
            if index != 0 {
                out.write_str(", ")?;
            }
            write!(out, "{kind}={count}")?;
        }
        out.write_str(".")
    }
}

#[derive(Clone, Copy)]
struct GripPoint {
    point: Point3,
    weight: PositiveReal,
}

#[derive(Clone, Copy)]
enum GripVertexMarker {
    Primary(usize),
    Secondary(Option<usize>),
}

/// Decode every active-asset T-spline control cage in archive order.
///
/// A cage whose program is internally inconsistent degrades to an
/// error-severity loss note instead of failing the document decode; its
/// entry bytes remain retained in the container, and the serializer-backed
/// Form join leaves the affected Form on native retention.
pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<(Vec<SubdSurface>, Vec<cadmpeg_ir::report::loss::LossNote>), CodecError> {
    let Some(folder) = scan.design_asset_folder() else {
        return Ok((Vec::new(), Vec::new()));
    };
    let mut cages = Vec::new();
    let mut losses = Vec::new();
    for entry in ctx.admit_iter(&scan.entries, "scan T-spline container entries")? {
        let Some(extension) = std::path::Path::new(&entry.name).extension() else {
            continue;
        };
        let Ok(extension) = ctx.validate_utf8(
            extension.as_encoded_bytes(),
            "validate T-spline file extension",
        )?
        else {
            continue;
        };
        if !ctx.eq_ignore_ascii_case(extension, "tsm", "match T-spline file extension")? {
            continue;
        }
        let Some(relative) =
            ctx.strip_prefix(&entry.name, folder, "match T-spline asset folder")?
        else {
            continue;
        };
        if !ctx.starts_with(relative, ENTRY_MARKER, "match T-spline asset path")? {
            continue;
        }
        match parse(ctx, &entry.name, scan.entry_bytes(ctx, &entry.name)?) {
            Ok(parsed) => {
                if !parsed.unknown_record_kinds.is_empty() {
                    let message = ctx.format_retained(
                        format_args!("{}", UntypedRecords(&parsed.unknown_record_kinds)),
                        "describe untyped T-spline records",
                    )?;
                    ctx.push_vec(
                        &mut losses,
                        F3dLossCode::TsplineRecordUntyped.note(message),
                        "collect T-spline loss notes",
                    )?;
                }
                ctx.push_vec(&mut cages, parsed.surface, "collect T-spline cages")?;
            }
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                let message = ctx.format_retained(
                    format_args!("T-spline control cage not decoded: {error}"),
                    "describe undecoded T-spline cage",
                )?;
                ctx.push_vec(
                    &mut losses,
                    F3dLossCode::TsplineCageUndecoded.note(message),
                    "collect T-spline loss notes",
                )?;
            }
        }
    }
    Ok((cages, losses))
}

fn malformed(ctx: &DecodeContext<'_>, name: &str, message: impl std::fmt::Display) -> CodecError {
    match ctx.format_retained(
        format_args!("T-spline cage {name}: {message}"),
        "describe malformed T-spline cage",
    ) {
        Ok(text) => CodecError::Malformed(text),
        Err(refusal) => refusal,
    }
}

fn line_views<'ctx, 'text>(
    ctx: &'ctx DecodeContext<'_>,
    text: &'text str,
) -> Result<
    (
        cadmpeg_core::decode::ScopedReservation<'ctx>,
        Vec<&'text str>,
    ),
    CodecError,
> {
    let mut storage = ctx.reserve_scoped(0, "stage T-spline line views")?;
    let lines = storage.with_storage(|| -> Result<Vec<&'text str>, CodecError> {
        let bytes = text.as_bytes();
        let mut lines = Vec::new();
        let mut start = 0usize;
        for (end, byte) in ctx
            .admit_iter(bytes, "split T-spline text lines")?
            .enumerate()
        {
            if *byte != b'\n' {
                continue;
            }
            let line_end = if end > start && bytes[end - 1] == b'\r' {
                end - 1
            } else {
                end
            };
            ctx.push_vec(
                &mut lines,
                &text[start..line_end],
                "stage T-spline line views",
            )?;
            start = end + 1;
        }
        if start < text.len() {
            ctx.push_vec(&mut lines, &text[start..], "stage T-spline line views")?;
        }
        Ok(lines)
    })?;
    Ok((storage, lines))
}

fn ascii_field_views<'ctx, 'text>(
    ctx: &'ctx DecodeContext<'_>,
    line: &'text str,
) -> Result<
    (
        cadmpeg_core::decode::ScopedReservation<'ctx>,
        Vec<&'text str>,
    ),
    CodecError,
> {
    let mut storage = ctx.reserve_scoped(0, "stage T-spline record fields")?;
    let fields = storage.with_storage(|| -> Result<Vec<&'text str>, CodecError> {
        let bytes = line.as_bytes();
        let mut fields = Vec::new();
        let mut start = None;
        for (index, byte) in ctx
            .admit_iter(bytes, "split T-spline record fields")?
            .enumerate()
        {
            if byte.is_ascii_whitespace() {
                if let Some(start) = start.take() {
                    ctx.push_vec(
                        &mut fields,
                        &line[start..index],
                        "stage T-spline record fields",
                    )?;
                }
            } else if start.is_none() {
                start = Some(index);
            }
        }
        if let Some(start) = start {
            ctx.push_vec(&mut fields, &line[start..], "stage T-spline record fields")?;
        }
        Ok(fields)
    })?;
    Ok((storage, fields))
}

pub(crate) fn subd_id(
    ctx: &DecodeContext<'_>,
    name: &str,
    source_key: &str,
) -> Result<cadmpeg_ir::ids::SubdId, CodecError> {
    let key_text = ctx.copy_retained_text(source_key, "retain T-spline identity key")?;
    let key = cadmpeg_ir::ids::IdentityKey::try_new(key_text)
        .map_err(|error| malformed(ctx, name, format_args!("invalid subd identity: {error}")))?;
    let text = ctx.format_retained(
        format_args!("f3d:tspline:subd#{}", key.as_str()),
        "retain T-spline identity",
    )?;
    cadmpeg_ir::ids::SubdId::mint(text)
        .map_err(|error| malformed(ctx, name, format_args!("invalid subd identity: {error}")))
}

fn parse_int<T: cadmpeg_core::decode::text::TextScalar>(
    ctx: &DecodeContext<'_>,
    name: &str,
    value: Option<&str>,
    field: &str,
) -> Result<T, CodecError> {
    let Some(value) = value else {
        return Err(malformed(ctx, name, format_args!("invalid {field}")));
    };
    ctx.parse_text(value, "parse T-spline integer")?
        .map_err(|_| malformed(ctx, name, format_args!("invalid {field}")))
}

fn parse_f64(
    ctx: &DecodeContext<'_>,
    name: &str,
    value: Option<&str>,
    field: &str,
) -> Result<FiniteReal, CodecError> {
    let parsed = match value {
        Some(value) => ctx.parse_text::<f64>(value, "parse T-spline scalar")?.ok(),
        None => None,
    };
    parsed
        .and_then(FiniteReal::new)
        .ok_or_else(|| malformed(ctx, name, format_args!("invalid {field}")))
}

fn parse_direction(
    ctx: &DecodeContext<'_>,
    name: &str,
    value: Option<&str>,
) -> Result<SubdGripDirection, CodecError> {
    match value {
        Some("NORTH") => Ok(SubdGripDirection::North),
        Some("EAST") => Ok(SubdGripDirection::East),
        Some("SOUTH") => Ok(SubdGripDirection::South),
        Some("WEST") => Ok(SubdGripDirection::West),
        _ => Err(malformed(ctx, name, "invalid vertex direction")),
    }
}

/// Map each program slot to its IR index, or `None` for a deleted slot.
fn compact(ctx: &DecodeContext<'_>, live: &[bool]) -> Result<Vec<Option<u32>>, CodecError> {
    let mut next = 0u32;
    let mut compacted = Vec::new();
    for live in ctx.admit_iter(live, "compact T-spline slots")? {
        let index = if *live {
            let index = next;
            next = next
                .checked_add(1)
                .ok_or_else(|| CodecError::malformed("T-spline live slot count exceeds u32"))?;
            Some(index)
        } else {
            None
        };
        ctx.push_vec(&mut compacted, index, "compact T-spline slots")?;
    }
    Ok(compacted)
}

fn require_end(
    ctx: &DecodeContext<'_>,
    name: &str,
    fields: &mut std::iter::Copied<
        cadmpeg_core::decode::scan::AdmittedIter<std::slice::Iter<'_, &str>>,
    >,
    record: &str,
) -> Result<(), CodecError> {
    if fields.next().is_some() {
        return Err(malformed(
            ctx,
            name,
            format_args!("{record} has trailing fields"),
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct ParsedCage {
    surface: SubdSurface,
    unknown_record_kinds: BTreeMap<String, usize>,
}

#[derive(Debug)]
struct DerivedGripConnectivity {
    vertex: usize,
    spoke_lengths: Vec<usize>,
    grip_indices: Vec<Option<usize>>,
}

#[derive(Clone, Copy)]
enum FanSlot {
    Phantom,
    Slot {
        half_edge: usize,
        face: Option<usize>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SymmetryMode {
    Correspondence,
    Radial,
}

#[derive(Debug)]
struct PartialSymmetryBlock {
    mode: SymmetryMode,
    plane: Option<[FiniteReal; 12]>,
    radial_segments: Option<std::num::NonZeroU32>,
    radial_sweep: Option<FiniteReal>,
    radial_maps: Vec<SubdRadialSymmetryMap>,
    record_kinds: BTreeSet<String>,
    face_forward: BTreeMap<usize, usize>,
    face_reverse: BTreeMap<usize, usize>,
    edge_forward: BTreeMap<usize, usize>,
    edge_reverse: BTreeMap<usize, usize>,
    vertex_forward: BTreeMap<usize, usize>,
    vertex_reverse: BTreeMap<usize, usize>,
}

impl PartialSymmetryBlock {
    fn new(mode: SymmetryMode) -> Self {
        Self {
            mode,
            plane: None,
            radial_segments: None,
            radial_sweep: None,
            radial_maps: Vec::new(),
            record_kinds: BTreeSet::new(),
            face_forward: BTreeMap::new(),
            face_reverse: BTreeMap::new(),
            edge_forward: BTreeMap::new(),
            edge_reverse: BTreeMap::new(),
            vertex_forward: BTreeMap::new(),
            vertex_reverse: BTreeMap::new(),
        }
    }
}

#[derive(Debug)]
struct SymmetryBlock {
    plane: [FiniteReal; 12],
    kind: SymmetryKind,
}

#[derive(Debug)]
enum SymmetryKind {
    Correspondence {
        face: BTreeMap<usize, usize>,
        edge: BTreeMap<usize, usize>,
        vertex: BTreeMap<usize, usize>,
    },
    Radial {
        segments: std::num::NonZeroU32,
        sweep: FiniteReal,
        maps: Vec<SubdRadialSymmetryMap>,
    },
}

fn parse_pairs(
    ctx: &DecodeContext<'_>,
    name: &str,
    fields: &mut std::iter::Copied<
        cadmpeg_core::decode::scan::AdmittedIter<std::slice::Iter<'_, &str>>,
    >,
    record: &str,
) -> Result<BTreeMap<usize, usize>, CodecError> {
    let mut values = Vec::new();
    for value in fields.by_ref() {
        ctx.push_vec(
            &mut values,
            parse_int::<usize>(ctx, name, Some(value), record)?,
            "read T-spline symmetry map values",
        )?;
    }
    if values.len() % 2 != 0 {
        return Err(malformed(
            ctx,
            name,
            format_args!("{record} has an unpaired index"),
        ));
    }
    let mut pairs = BTreeMap::new();
    let mut source = None;
    for (index, value) in ctx
        .admit_iter(&values, "pair T-spline symmetry indices")?
        .enumerate()
    {
        if index % 2 == 0 {
            source = Some(*value);
            continue;
        }
        let Some(source_index) = source.take() else {
            return Err(malformed(ctx, name, "symmetry map has an unpaired index"));
        };
        if ctx.contains_key_btree_map(
            &pairs,
            &source_index,
            "check duplicate T-spline symmetry source",
        )? {
            return Err(malformed(
                ctx,
                name,
                format_args!("{record} repeats a source index"),
            ));
        }
        ctx.insert_btree_map(
            &mut pairs,
            source_index,
            *value,
            "index T-spline symmetry map pairs",
        )?;
    }
    Ok(pairs)
}

fn parse_radial_pairs(
    ctx: &DecodeContext<'_>,
    name: &str,
    fields: &mut std::iter::Copied<
        cadmpeg_core::decode::scan::AdmittedIter<std::slice::Iter<'_, &str>>,
    >,
) -> Result<Vec<[u64; 2]>, CodecError> {
    let mut values = Vec::new();
    for value in fields.by_ref() {
        let index = ctx
            .parse_text::<u64>(value, "parse T-spline radial map index")?
            .map_err(|_| malformed(ctx, name, "invalid radial symmetry map index"))?;
        ctx.push_vec(&mut values, index, "read T-spline radial map values")?;
    }
    if values.len() % 2 != 0 {
        return Err(malformed(
            ctx,
            name,
            "radial symmetry map has an unpaired index",
        ));
    }
    let mut sources = BTreeSet::new();
    let mut pairs = Vec::new();
    let mut source = None;
    for (index, value) in ctx
        .admit_iter(&values, "pair T-spline radial map indices")?
        .enumerate()
    {
        if index % 2 == 0 {
            source = Some(*value);
            continue;
        }
        let Some(source_index) = source.take() else {
            return Err(malformed(
                ctx,
                name,
                "radial symmetry map has an unpaired index",
            ));
        };
        if ctx.contains_btree_set(
            &sources,
            &source_index,
            "check duplicate T-spline radial map source",
        )? {
            return Err(malformed(
                ctx,
                name,
                "radial symmetry map repeats a source index",
            ));
        }
        ctx.insert_btree_set(
            &mut sources,
            source_index,
            "index T-spline radial map sources",
        )?;
        ctx.push_vec(
            &mut pairs,
            [source_index, *value],
            "read T-spline radial map pairs",
        )?;
    }
    Ok(pairs)
}

fn validate_symmetry_map(
    ctx: &DecodeContext<'_>,
    name: &str,
    forward: &BTreeMap<usize, usize>,
    reverse: &BTreeMap<usize, usize>,
    slots: &[bool],
    element: &str,
) -> Result<(), CodecError> {
    for (&source, &target) in ctx.admit_iter(forward, "validate T-spline forward symmetry map")? {
        if !slots.get(source).copied().unwrap_or(false)
            || !slots.get(target).copied().unwrap_or(false)
        {
            return Err(malformed(
                ctx,
                name,
                format_args!("{element} symmetry map is inconsistent"),
            ));
        }
        if source != target
            && ctx.get_btree_map(reverse, &target, "check T-spline reverse symmetry map")?
                != Some(&source)
        {
            return Err(malformed(
                ctx,
                name,
                format_args!("{element} symmetry map is inconsistent"),
            ));
        }
    }
    for (&source, &target) in ctx.admit_iter(reverse, "validate T-spline reverse symmetry map")? {
        if !slots.get(source).copied().unwrap_or(false)
            || !slots.get(target).copied().unwrap_or(false)
        {
            return Err(malformed(
                ctx,
                name,
                format_args!("{element} symmetry map is inconsistent"),
            ));
        }
        if ctx.get_btree_map(forward, &target, "check T-spline forward symmetry map")?
            != Some(&source)
        {
            return Err(malformed(
                ctx,
                name,
                format_args!("{element} symmetry map is inconsistent"),
            ));
        }
    }
    Ok(())
}

fn symmetry_plane(
    ctx: &DecodeContext<'_>,
    name: &str,
    values: [FiniteReal; 12],
) -> Result<SubdPlaneFrame, CodecError> {
    let origin = Point3::new(
        values[0].get() * CAGE_COORDINATE_SCALE,
        values[1].get() * CAGE_COORDINATE_SCALE,
        values[2].get() * CAGE_COORDINATE_SCALE,
    );
    let first_axis = Vector3::new(values[4].get(), values[5].get(), values[6].get());
    let second_axis = Vector3::new(values[8].get(), values[9].get(), values[10].get());
    if (values[3].get() - 1.0).abs() > SYMMETRY_FRAME_EPS
        || values[7].get().abs() > SYMMETRY_FRAME_EPS
        || values[11].get().abs() > SYMMETRY_FRAME_EPS
    {
        return Err(malformed(
            ctx,
            name,
            "symmetry plane is not a homogeneous orthonormal frame",
        ));
    }
    SubdPlaneFrame::new(origin, first_axis, second_axis).map_err(|error| {
        malformed(
            ctx,
            name,
            format_args!("symmetry plane is not a homogeneous orthonormal frame: {error}"),
        )
    })
}

fn remap_symmetry_pairs(
    ctx: &DecodeContext<'_>,
    name: &str,
    map: &BTreeMap<usize, usize>,
    ir_indices: &[Option<u32>],
    element: &str,
) -> Result<Vec<[u32; 2]>, CodecError> {
    let mut remapped = Vec::new();
    for (&source, &target) in ctx.admit_iter(map, "remap T-spline symmetry pairs")? {
        let source = ir_indices.get(source).copied().flatten().ok_or_else(|| {
            malformed(
                ctx,
                name,
                format_args!("{element} symmetry source is deleted"),
            )
        })?;
        let target = ir_indices.get(target).copied().flatten().ok_or_else(|| {
            malformed(
                ctx,
                name,
                format_args!("{element} symmetry target is deleted"),
            )
        })?;
        ctx.push_vec(
            &mut remapped,
            [source, target],
            "remap T-spline symmetry pairs",
        )?;
    }
    Ok(remapped)
}

fn direction_offset(direction: SubdGripDirection) -> usize {
    match direction {
        SubdGripDirection::North => 0,
        SubdGripDirection::East => 1,
        SubdGripDirection::South => 2,
        SubdGripDirection::West => 3,
    }
}

fn build_fan(
    ctx: &DecodeContext<'_>,
    name: &str,
    vertex: usize,
    root: usize,
    half_edges: &[HalfEdge],
    face_live: &[bool],
) -> Result<Vec<FanSlot>, CodecError> {
    let root_id = HalfEdgeId(root);
    let root_half = &half_edges[root];
    if root_half.vertex != vertex {
        return Err(malformed(
            ctx,
            name,
            "vertex root does not terminate at its vertex",
        ));
    }

    let mut fan = Vec::new();
    let mut seen = BTreeSet::new();
    let mut current = root_id;
    loop {
        ctx.charge_work(1, "walk T-spline vertex fan")?;
        if ctx.contains_btree_set(&seen, &current, "detect repeated T-spline fan half-edge")? {
            if current == root_id {
                break;
            }
            return Err(malformed(
                ctx,
                name,
                "vertex half-edge fan repeats before its root",
            ));
        }
        ctx.insert_btree_set(&mut seen, current, "index T-spline fan half-edges")?;
        let half = &half_edges[current.index()];
        if half.vertex != vertex {
            return Err(malformed(
                ctx,
                name,
                "vertex half-edge fan leaves its terminal vertex",
            ));
        }
        if half
            .face
            .is_some_and(|face| !face_live.get(face).copied().unwrap_or(false))
        {
            return Err(malformed(
                ctx,
                name,
                "vertex half-edge fan names an invalid face",
            ));
        }
        ctx.push_vec(
            &mut fan,
            FanSlot::Slot {
                half_edge: current.index(),
                face: half.face,
            },
            "collect T-spline fan slots",
        )?;

        let next = &half_edges[half.next.index()];
        let mate = &half_edges[next.mate.index()];
        let Some(rotated) = (mate.vertex == vertex).then_some(next.mate) else {
            return Err(malformed(
                ctx,
                name,
                "vertex fan rotation leaves its terminal vertex",
            ));
        };
        current = rotated;
        if current == root_id {
            break;
        }
        if seen.len() >= half_edges.len() {
            return Err(malformed(ctx, name, "vertex half-edge fan does not close"));
        }
    }

    let mut gap = None;
    for (index, slot) in ctx
        .admit_iter(&fan, "validate T-spline fan gaps")?
        .enumerate()
    {
        if matches!(slot, FanSlot::Slot { face: None, .. }) && gap.replace(index).is_some() {
            return Err(malformed(
                ctx,
                name,
                "vertex half-edge fan has multiple boundary gaps",
            ));
        }
    }
    if let Some(gap) = gap {
        let phantom_count = if fan.len() < 4 { 4 - fan.len() } else { 0 };

        ctx.reserve_vec(&mut fan, phantom_count, "complete T-spline fan gaps")?;
        for _ in ctx.admit_iter(&(0..phantom_count), "complete T-spline fan gaps")? {
            let moved_slots = cadmpeg_core::decode::u64_from_index(fan.len() - gap - 1);
            let slot_bytes = cadmpeg_core::decode::u64_from_index(std::mem::size_of::<FanSlot>());
            let moved_bytes = moved_slots.checked_mul(slot_bytes).ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "move T-spline fan slots",
                    u64::MAX / slot_bytes,
                    moved_slots,
                )
            })?;
            let move_work = moved_bytes.checked_add(moved_slots).ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "move T-spline fan slots",
                    u64::MAX - moved_slots,
                    moved_bytes,
                )
            })?;
            ctx.charge_work(move_work, "move T-spline fan slots")?;
            fan.insert(gap + 1, FanSlot::Phantom);
        }
    }
    Ok(fan)
}

fn grip_block(
    ctx: &DecodeContext<'_>,
    name: &str,
    grip_points: &[Option<GripPoint>],
    indices: &[Option<usize>],
    cursor: &mut usize,
    count: usize,
) -> Result<Vec<Option<SubdSecondaryGrip>>, CodecError> {
    let end = cursor
        .checked_add(count)
        .ok_or_else(|| malformed(ctx, name, "derived-grip block arity overflows"))?;
    let values = indices.get(*cursor..end).ok_or_else(|| {
        malformed(
            ctx,
            name,
            "derived-grip run is shorter than its declared arity",
        )
    })?;
    *cursor = end;
    let mut grips = Vec::new();
    for &index in ctx.admit_iter(values, "materialize T-spline grip block")? {
        let grip = index
            .map(|index| {
                let point = grip_points.get(index).copied().flatten().ok_or_else(|| {
                    malformed(ctx, name, "derived-grip entry names a deleted grip")
                })?;
                SubdSecondaryGrip::from_parts(
                    u32::try_from(index)
                        .map_err(|_| malformed(ctx, name, "secondary grip index overflows IR"))?,
                    point.point,
                    point.weight,
                )
                .map_err(|error| malformed(ctx, name, error))
            })
            .transpose()?;
        ctx.push_vec(&mut grips, grip, "materialize T-spline grip block")?;
    }
    Ok(grips)
}

struct SecondaryLayoutContext<'a> {
    name: &'a str,
    vertex_roots: &'a [Option<(usize, SubdGripDirection)>],
    vertex_live: &'a [bool],
    vertex_ir: &'a [Option<u32>],
    face_live: &'a [bool],
    face_ir: &'a [Option<u32>],
    half_edges: &'a [HalfEdge],
    edge_by_half: &'a [Option<(u32, bool)>],
    grip_vertices: &'a [GripVertexMarker],
    grip_points: &'a [Option<GripPoint>],
}

fn build_secondary_layouts(
    ctx: &DecodeContext<'_>,
    context: &SecondaryLayoutContext<'_>,
    derived_grips: &[DerivedGripConnectivity],
) -> Result<Vec<Option<SubdVertexGripLayout>>, CodecError> {
    let SecondaryLayoutContext {
        name,
        vertex_roots,
        vertex_live,
        vertex_ir,
        face_live,
        face_ir,
        half_edges,
        edge_by_half,
        grip_vertices,
        grip_points,
    } = *context;
    let live_vertices = ctx
        .admit_iter(vertex_ir, "count T-spline live vertices")?
        .flatten()
        .count();
    let mut layouts =
        ctx.collect_indexed_vec(live_vertices, "f3d subd secondary layouts", |_| Ok(None))?;
    let mut has_cg = ctx.alloc_filled(
        vertex_live.len(),
        false,
        "f3d subd derived-grip ownership flags",
    )?;
    let mut secondary_counts =
        ctx.alloc_filled(vertex_live.len(), 0usize, "f3d subd secondary-grip counts")?;
    for marker in ctx.admit_iter(grip_vertices, "count T-spline secondary grips")? {
        if let GripVertexMarker::Secondary(Some(vertex)) = marker {
            *secondary_counts
                .get_mut(*vertex)
                .ok_or_else(|| malformed(ctx, name, "secondary grip vertex is out of range"))? += 1;
        }
    }

    for connectivity in ctx.admit_iter(derived_grips, "build T-spline secondary layouts")? {
        let vertex = connectivity.vertex;
        if !vertex_live.get(vertex).copied().unwrap_or(false) {
            return Err(malformed(ctx, name, "derived-grip vertex is out of range"));
        }
        if has_cg[vertex] {
            return Err(malformed(
                ctx,
                name,
                "vertex has more than one derived-grip record",
            ));
        }
        has_cg[vertex] = true;
        let (root, direction) = vertex_roots
            .get(vertex)
            .copied()
            .flatten()
            .ok_or_else(|| malformed(ctx, name, "derived-grip vertex has no root direction"))?;
        let fan = build_fan(ctx, name, vertex, root, half_edges, face_live)?;
        if connectivity.spoke_lengths.len() != fan.len() {
            return Err(malformed(
                ctx,
                name,
                "derived-grip wedge count does not match the completed vertex fan",
            ));
        }

        let offset = direction_offset(direction);
        let mut cursor = 0usize;
        let mut wedges = Vec::new();
        for (wedge, &spoke_count) in ctx
            .admit_iter(&connectivity.spoke_lengths, "project T-spline grip wedges")?
            .enumerate()
        {
            let sector_count = spoke_count
                .checked_mul(
                    connectivity.spoke_lengths[(wedge + 1) % connectivity.spoke_lengths.len()],
                )
                .ok_or_else(|| malformed(ctx, name, "derived-grip sector arity overflows"))?;
            let slot = fan[(wedge + offset) % fan.len()];
            let FanSlot::Slot { half_edge, face } = slot else {
                if spoke_count != 0 {
                    return Err(malformed(
                        ctx,
                        name,
                        "phantom wedge carries a nonzero spoke length",
                    ));
                }
                ctx.push_vec(
                    &mut wedges,
                    SubdGripWedge::Phantom {},
                    "project T-spline grip wedges",
                )?;
                continue;
            };
            let edge = Some(
                edge_by_half
                    .get(half_edge)
                    .copied()
                    .flatten()
                    .map(|(edge, _)| edge)
                    .ok_or_else(|| malformed(ctx, name, "fan half-edge has no owning edge"))?,
            );
            let sector_face =
                match face {
                    Some(face) => {
                        Some(face_ir.get(face).copied().flatten().ok_or_else(|| {
                            malformed(ctx, name, "fan sector names a deleted face")
                        })?)
                    }
                    None => None,
                };
            let spokes = grip_block(
                ctx,
                name,
                grip_points,
                &connectivity.grip_indices,
                &mut cursor,
                spoke_count,
            )?;
            let sectors = grip_block(
                ctx,
                name,
                grip_points,
                &connectivity.grip_indices,
                &mut cursor,
                sector_count,
            )?;
            ctx.push_vec(
                &mut wedges,
                SubdGripWedge::Slot {
                    edge,
                    sector_face,
                    spokes,
                    sectors,
                },
                "project T-spline grip wedges",
            )?;
        }
        if cursor != connectivity.grip_indices.len() {
            return Err(malformed(
                ctx,
                name,
                "derived-grip run has trailing entries",
            ));
        }
        let vertex_ir = vertex_ir[vertex]
            .ok_or_else(|| malformed(ctx, name, "derived-grip vertex is deleted"))?;
        layouts[index_from_u32(vertex_ir)] = Some(
            SubdVertexGripLayout::new(direction, wedges, ctx)?
                .map_err(|error| malformed(ctx, name, error))?,
        );
    }

    for (vertex, count) in ctx
        .admit_iter(
            &secondary_counts,
            "validate T-spline secondary-grip ownership",
        )?
        .enumerate()
    {
        if (*count != 0) != has_cg[vertex] {
            return Err(malformed(
                ctx,
                name,
                "secondary-grip ownership does not have exactly one derived-grip record",
            ));
        }
    }
    Ok(layouts)
}

enum EditorSelectionKind {
    Edges,
    Vertices,
    Grips,
}

impl EditorSelectionKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Edges => "100edges",
            Self::Vertices => "100verts",
            Self::Grips => "50000grip",
        }
    }
}

fn parse(ctx: &DecodeContext<'_>, name: &str, bytes: &[u8]) -> Result<ParsedCage, CodecError> {
    let text = ctx
        .validate_utf8(bytes, "validate T-spline text UTF-8")?
        .map_err(|error| malformed(ctx, name, format_args!("payload is not UTF-8: {error}")))?;
    let (_line_storage, lines) = line_views(ctx, text)?;
    let Some(header) = lines.first() else {
        return Err(malformed(ctx, name, "unsupported header"));
    };
    if !ctx.equal(header, &"#TS0200", "validate T-spline header")? {
        return Err(malformed(ctx, name, "unsupported header"));
    }

    // Every topology token occupies a slot in its own record order, and a bare
    // token is a deleted slot that occupies its index without defining an
    // element. Indices inside the program address slots, so slots are retained
    // through validation and compacted only when the IR cage is built.
    let mut face_roots: Vec<Option<usize>> = Vec::new();
    let mut edge_roots: Vec<Option<usize>> = Vec::new();
    let mut edge_knot_intervals: Vec<Option<PositiveReal>> = Vec::new();
    let mut edge_knot_records = Vec::new();
    let mut vertex_roots: Vec<Option<(usize, SubdGripDirection)>> = Vec::new();
    let mut vertex_live: Vec<bool> = Vec::new();
    let mut half_edges: Vec<Option<ParsedHalfEdge>> = Vec::new();
    let mut crease_edges = BTreeSet::new();
    let mut grip_vertices: Vec<GripVertexMarker> = Vec::new();
    let mut grip_points: Vec<Option<GripPoint>> = Vec::new();
    let mut in_grip_map = false;
    let mut declarations = BTreeSet::new();
    let mut end_conditions = None;
    let mut derived_grips = Vec::new();
    let mut selected_edges = BTreeSet::new();
    let mut selected_vertices = BTreeSet::new();
    let mut selected_grips = BTreeSet::new();
    let mut editor_declarations = BTreeSet::new();
    let mut symmetry_blocks = Vec::new();
    let mut current_symmetry: Option<PartialSymmetryBlock> = None;
    let mut terminal_declarations = BTreeSet::new();
    let mut unknown_record_kinds: BTreeMap<String, usize> = BTreeMap::new();
    for raw_line in ctx.admit_iter(&lines, "scan T-spline records")? {
        let line = ctx.trim_text(raw_line, "trim T-spline record")?;
        if line.is_empty() {
            continue;
        }
        let (_field_storage, field_views) = ascii_field_views(ctx, line)?;
        let mut fields = ctx
            .admit_iter(&field_views, "parse T-spline record fields")?
            .copied();
        let record_kind = fields.next();
        let selection = match record_kind {
            Some("100edges") => Some(EditorSelectionKind::Edges),
            Some("100verts") => Some(EditorSelectionKind::Vertices),
            Some("50000grip") => Some(EditorSelectionKind::Grips),
            _ => None,
        };
        if let Some(selection) = selection {
            let label = selection.as_str();
            if !matches!(selection, EditorSelectionKind::Grips)
                && !ctx.insert_btree_set(
                    &mut editor_declarations,
                    label,
                    "index T-spline editor selections",
                )?
            {
                return Err(malformed(
                    ctx,
                    name,
                    format_args!("duplicate {label} record"),
                ));
            }
            let mut values = BTreeSet::new();
            for value in fields {
                let index = parse_int::<usize>(ctx, name, Some(value), label)?;
                ctx.insert_btree_set(&mut values, index, "read T-spline editor selections")?;
            }
            match selection {
                EditorSelectionKind::Edges => selected_edges = values,
                EditorSelectionKind::Vertices => selected_vertices = values,
                EditorSelectionKind::Grips => {
                    for value in ctx.admit_iter(&values, "merge T-spline grip selections")? {
                        ctx.insert_btree_set(
                            &mut selected_grips,
                            *value,
                            "merge T-spline grip selections",
                        )?;
                    }
                }
            }
            continue;
        }
        match record_kind {
            Some("#TS0200") => require_end(ctx, name, &mut fields, "header")?,
            Some("degree") => {
                if parse_int::<usize>(ctx, name, fields.next(), "degree")? != 3 {
                    return Err(malformed(ctx, name, "unsupported degree"));
                }
                require_end(ctx, name, &mut fields, "degree declaration")?;
                ctx.insert_btree_set(&mut declarations, "degree", "index T-spline declarations")?;
            }
            Some(declaration @ ("cap-type" | "end-conditions" | "star-knot-rule")) => {
                let value = fields.next().ok_or_else(|| {
                    malformed(ctx, name, format_args!("missing {declaration} value"))
                })?;
                if declaration == "end-conditions" {
                    end_conditions = Some(value);
                }
                require_end(ctx, name, &mut fields, declaration)?;
                ctx.insert_btree_set(
                    &mut declarations,
                    declaration,
                    "index T-spline declarations",
                )?;
            }
            Some("star-smoothness") => {
                parse_f64(ctx, name, fields.next(), "star smoothness")?;
                require_end(ctx, name, &mut fields, "star-smoothness declaration")?;
                ctx.insert_btree_set(
                    &mut declarations,
                    "star-smoothness",
                    "index T-spline declarations",
                )?;
            }
            Some("units") => {
                if fields.next() != Some("1") || fields.next() != Some("meters") {
                    return Err(malformed(ctx, name, "unsupported units declaration"));
                }
                require_end(ctx, name, &mut fields, "units declaration")?;
                ctx.insert_btree_set(&mut declarations, "units", "index T-spline declarations")?;
            }
            Some("f") => match fields.next() {
                None => ctx.push_vec(&mut face_roots, None, "read T-spline face slots")?,
                root => {
                    ctx.push_vec(
                        &mut face_roots,
                        Some(parse_int::<usize>(ctx, name, root, "face root")?),
                        "read T-spline face slots",
                    )?;
                    parse_int::<i64>(ctx, name, fields.next(), "face flags")?;
                    require_end(ctx, name, &mut fields, "face")?;
                }
            },
            Some("e") => match fields.next() {
                None => {
                    ctx.push_vec(&mut edge_roots, None, "read T-spline edge slots")?;
                    ctx.push_vec(
                        &mut edge_knot_intervals,
                        None,
                        "read T-spline edge knot intervals",
                    )?;
                }
                root => {
                    ctx.push_vec(
                        &mut edge_roots,
                        Some(parse_int::<usize>(ctx, name, root, "edge root")?),
                        "read T-spline edge slots",
                    )?;
                    let knot_interval = parse_f64(ctx, name, fields.next(), "edge knot interval")?;
                    let knot_interval =
                        PositiveReal::new(knot_interval.get()).ok_or_else(|| {
                            malformed(ctx, name, "edge knot interval is not positive")
                        })?;
                    ctx.push_vec(
                        &mut edge_knot_intervals,
                        Some(knot_interval),
                        "read T-spline edge knot intervals",
                    )?;
                    require_end(ctx, name, &mut fields, "edge")?;
                }
            },
            Some("106ek") => {
                ctx.push_vec(
                    &mut edge_knot_records,
                    parse_f64(ctx, name, fields.next(), "106ek value")?,
                    "read T-spline edge knot records",
                )?;
                require_end(ctx, name, &mut fields, "106ek")?;
            }
            Some("v") => match fields.next() {
                None => {
                    ctx.push_vec(&mut vertex_live, false, "read T-spline vertex live flags")?;
                    ctx.push_vec(&mut vertex_roots, None, "read T-spline vertex roots")?;
                }
                root => {
                    let root = parse_int::<usize>(ctx, name, root, "vertex root")?;
                    let direction = parse_direction(ctx, name, fields.next())?;
                    require_end(ctx, name, &mut fields, "vertex")?;
                    ctx.push_vec(&mut vertex_live, true, "read T-spline vertex live flags")?;
                    ctx.push_vec(
                        &mut vertex_roots,
                        Some((root, direction)),
                        "read T-spline vertex roots",
                    )?;
                }
            },
            Some("l") => match fields.next() {
                None => ctx.push_vec(&mut half_edges, None, "read T-spline half-edge slots")?,
                next => {
                    let half = ParsedHalfEdge {
                        next: parse_int::<usize>(ctx, name, next, "half-edge next index")?,
                        previous: parse_int::<usize>(
                            ctx,
                            name,
                            fields.next(),
                            "half-edge previous index",
                        )?,
                        mate: parse_int::<usize>(ctx, name, fields.next(), "half-edge mate index")?,
                        vertex: parse_int::<usize>(
                            ctx,
                            name,
                            fields.next(),
                            "half-edge vertex index",
                        )?,
                        face: parse_int::<i64>(ctx, name, fields.next(), "half-edge face index")?,
                    };
                    parse_int::<i64>(ctx, name, fields.next(), "half-edge edge index")?;
                    parse_int::<i64>(ctx, name, fields.next(), "half-edge flags")?;
                    if fields.next().is_some() {
                        return Err(malformed(ctx, name, "half-edge has trailing fields"));
                    }
                    ctx.push_vec(&mut half_edges, Some(half), "read T-spline half-edge slots")?;
                }
            },
            Some("ec") => {
                ctx.insert_btree_set(
                    &mut crease_edges,
                    parse_int::<usize>(ctx, name, fields.next(), "crease edge index")?,
                    "read T-spline crease edges",
                )?;
                parse_int::<i64>(ctx, name, fields.next(), "crease flags")?;
                require_end(ctx, name, &mut fields, "crease")?;
            }
            Some("0m") => match fields.next() {
                Some("odd-grip-map") => {
                    require_end(ctx, name, &mut fields, "odd-grip-map declaration")?;
                    in_grip_map = true;
                }
                Some("gvp") if in_grip_map => {
                    ctx.push_vec(
                        &mut grip_vertices,
                        GripVertexMarker::Primary(parse_int::<usize>(
                            ctx,
                            name,
                            fields.next(),
                            "grip vertex index",
                        )?),
                        "read T-spline grip vertex markers",
                    )?;
                    require_end(ctx, name, &mut fields, "primary grip map")?;
                }
                Some("gv") if in_grip_map => {
                    let vertex =
                        parse_int::<i64>(ctx, name, fields.next(), "secondary grip vertex index")?;
                    if vertex < -1 {
                        return Err(malformed(ctx, name, "secondary grip vertex is below -1"));
                    }
                    ctx.push_vec(
                        &mut grip_vertices,
                        GripVertexMarker::Secondary(if vertex >= 0 {
                            Some(usize::try_from(vertex).map_err(|_| {
                                malformed(ctx, name, "secondary grip vertex exceeds address space")
                            })?)
                        } else {
                            None
                        }),
                        "read T-spline grip vertex markers",
                    )?;
                    require_end(ctx, name, &mut fields, "secondary grip map")?;
                }
                Some("cg") if in_grip_map => {
                    let vertex =
                        parse_int::<usize>(ctx, name, fields.next(), "derived-grip vertex")?;
                    let wedges =
                        parse_int::<usize>(ctx, name, fields.next(), "derived-grip wedge count")?;
                    let mut spoke_lengths = Vec::new();
                    let mut spoke_count_read = 0usize;
                    for value in fields.by_ref().take(wedges) {
                        spoke_count_read += 1;
                        let length = parse_int::<usize>(
                            ctx,
                            name,
                            Some(value),
                            "derived-grip spoke length",
                        )?;
                        ctx.push_vec(
                            &mut spoke_lengths,
                            length,
                            "read T-spline derived-grip spokes",
                        )?;
                    }
                    if spoke_count_read != wedges {
                        return Err(malformed(ctx, name, "invalid derived-grip spoke length"));
                    }
                    if spoke_lengths.is_empty() {
                        return Err(malformed(ctx, name, "derived-grip wedge count is zero"));
                    }
                    let grip_count = ctx
                        .admit_iter(&spoke_lengths, "count T-spline derived-grip entries")?
                        .enumerate()
                        .try_fold(0usize, |count, (wedge, &spoke_count)| {
                            let cross = spoke_count
                                .checked_mul(spoke_lengths[(wedge + 1) % spoke_lengths.len()])
                                .ok_or_else(|| {
                                    malformed(ctx, name, "derived-grip arity overflows")
                                })?;
                            count
                                .checked_add(spoke_count)
                                .and_then(|count| count.checked_add(cross))
                                .ok_or_else(|| malformed(ctx, name, "derived-grip arity overflows"))
                        })?;
                    let mut grip_indices = Vec::new();
                    let mut grip_count_read = 0usize;
                    for value in fields.by_ref().take(grip_count) {
                        grip_count_read += 1;
                        let index =
                            match parse_int::<i64>(ctx, name, Some(value), "derived-grip index")? {
                                -1 => None,
                                index => Some(usize::try_from(index).map_err(|_| {
                                    malformed(
                                        ctx,
                                        name,
                                        "derived-grip index is negative or overflows",
                                    )
                                })?),
                            };
                        ctx.push_vec(
                            &mut grip_indices,
                            index,
                            "read T-spline derived-grip indices",
                        )?;
                    }
                    if grip_count_read != grip_count {
                        return Err(malformed(ctx, name, "invalid derived-grip index"));
                    }
                    require_end(ctx, name, &mut fields, "derived-grip connectivity")?;
                    ctx.push_vec(
                        &mut derived_grips,
                        DerivedGripConnectivity {
                            vertex,
                            spoke_lengths,
                            grip_indices,
                        },
                        "read T-spline derived grips",
                    )?;
                }
                _ => return Err(malformed(ctx, name, "unknown odd-grip-map record")),
            },
            Some("0g") => match fields.next() {
                None => ctx.push_vec(&mut grip_points, None, "read T-spline grip points")?,
                x => {
                    let point = Point3::new(
                        parse_f64(ctx, name, x, "grip x")?.get() * CAGE_COORDINATE_SCALE,
                        parse_f64(ctx, name, fields.next(), "grip y")?.get()
                            * CAGE_COORDINATE_SCALE,
                        parse_f64(ctx, name, fields.next(), "grip z")?.get()
                            * CAGE_COORDINATE_SCALE,
                    );
                    let weight = parse_f64(ctx, name, fields.next(), "grip weight")?;
                    let weight = PositiveReal::new(weight.get())
                        .filter(|_| fields.next().is_none())
                        .ok_or_else(|| malformed(ctx, name, "grip weight is not positive"))?;
                    ctx.push_vec(
                        &mut grip_points,
                        Some(GripPoint { point, weight }),
                        "read T-spline grip points",
                    )?;
                }
            },
            Some("105sym") => {
                let mode = match parse_int::<i64>(ctx, name, fields.next(), "symmetry flags")? {
                    0 => SymmetryMode::Correspondence,
                    1 => SymmetryMode::Radial,
                    _ => return Err(malformed(ctx, name, "unsupported symmetry flags")),
                };
                require_end(ctx, name, &mut fields, "symmetry header")?;
                if let Some(block) = current_symmetry.replace(PartialSymmetryBlock::new(mode)) {
                    ctx.push_vec(&mut symmetry_blocks, block, "read T-spline symmetry blocks")?;
                }
            }
            Some("105plane") => {
                let block = current_symmetry
                    .as_mut()
                    .ok_or_else(|| malformed(ctx, name, "symmetry plane has no header"))?;
                let mut values = Vec::new();
                for value in fields {
                    ctx.push_vec(
                        &mut values,
                        parse_f64(ctx, name, Some(value), "symmetry plane coefficient")?,
                        "read T-spline symmetry plane coefficients",
                    )?;
                }
                let plane: [FiniteReal; 12] = values.try_into().map_err(|_| {
                    malformed(ctx, name, "symmetry plane must have 12 coefficients")
                })?;
                if block.plane.replace(plane).is_some() {
                    return Err(malformed(ctx, name, "duplicate symmetry plane"));
                }
            }
            Some("105a") => {
                let kind = fields
                    .next()
                    .ok_or_else(|| malformed(ctx, name, "missing symmetry map kind"))?;
                let pairs = parse_pairs(ctx, name, &mut fields, "symmetry map")?;
                let block = current_symmetry
                    .as_mut()
                    .ok_or_else(|| malformed(ctx, name, "symmetry map has no header"))?;
                if block.mode != SymmetryMode::Correspondence {
                    return Err(malformed(
                        ctx,
                        name,
                        "correspondence map belongs to a radial symmetry block",
                    ));
                }
                if ctx.contains_btree_set(
                    &block.record_kinds,
                    kind,
                    "check duplicate T-spline symmetry kind",
                )? {
                    return Err(malformed(
                        ctx,
                        name,
                        format_args!("duplicate {kind} symmetry map"),
                    ));
                }
                let kind_name = ctx.copy_retained_text(kind, "retain T-spline symmetry kind")?;
                ctx.insert_btree_set(
                    &mut block.record_kinds,
                    kind_name,
                    "index T-spline symmetry kinds",
                )?;
                let target = match kind {
                    "fr" => &mut block.face_forward,
                    "f" => &mut block.face_reverse,
                    "er" => &mut block.edge_forward,
                    "e" => &mut block.edge_reverse,
                    "vr" => &mut block.vertex_forward,
                    "v" => &mut block.vertex_reverse,
                    _ => return Err(malformed(ctx, name, "unknown symmetry map kind")),
                };
                *target = pairs;
            }
            Some("105r") => {
                let kind = fields
                    .next()
                    .ok_or_else(|| malformed(ctx, name, "missing radial symmetry record kind"))?;
                let block = current_symmetry
                    .as_mut()
                    .ok_or_else(|| malformed(ctx, name, "radial symmetry record has no header"))?;
                if block.mode != SymmetryMode::Radial {
                    return Err(malformed(
                        ctx,
                        name,
                        "radial symmetry record belongs to a correspondence block",
                    ));
                }
                if ctx.contains_btree_set(
                    &block.record_kinds,
                    kind,
                    "check duplicate T-spline symmetry kind",
                )? {
                    return Err(malformed(
                        ctx,
                        name,
                        format_args!("duplicate {kind} radial symmetry record"),
                    ));
                }
                let kind_name = ctx.copy_retained_text(kind, "retain T-spline symmetry kind")?;
                ctx.insert_btree_set(
                    &mut block.record_kinds,
                    kind_name,
                    "index T-spline symmetry kinds",
                )?;
                match kind {
                    "segments" => {
                        let segments = parse_int::<usize>(
                            ctx,
                            name,
                            fields.next(),
                            "radial symmetry segments",
                        )?;
                        block.radial_segments = Some(
                            std::num::NonZeroU32::new(u32::try_from(segments).map_err(|_| {
                                malformed(ctx, name, "radial symmetry segments exceed u32")
                            })?)
                            .ok_or_else(|| {
                                malformed(ctx, name, "radial symmetry segments is not positive")
                            })?,
                        );
                        require_end(ctx, name, &mut fields, "radial symmetry segments")?;
                    }
                    "sweep" => {
                        block.radial_sweep = Some(parse_f64(
                            ctx,
                            name,
                            fields.next(),
                            "radial symmetry sweep",
                        )?);
                        require_end(ctx, name, &mut fields, "radial symmetry sweep")?;
                    }
                    kind => {
                        let selector = match kind {
                            "ef" => SubdRadialMapSelector::Ef,
                            "er" => SubdRadialMapSelector::Er,
                            "ff" => SubdRadialMapSelector::Ff,
                            "fr" => SubdRadialMapSelector::Fr,
                            "vf" => SubdRadialMapSelector::Vf,
                            "vr" => SubdRadialMapSelector::Vr,
                            _ => {
                                return Err(malformed(
                                    ctx,
                                    name,
                                    format_args!("unknown radial symmetry record {kind}"),
                                ))
                            }
                        };
                        let pairs = parse_radial_pairs(ctx, name, &mut fields)?;
                        ctx.push_vec(
                            &mut block.radial_maps,
                            SubdRadialSymmetryMap { selector, pairs },
                            "read T-spline radial maps",
                        )?;
                    }
                }
            }
            Some(declaration @ ("tol" | "geom-tol")) => {
                let tolerance = parse_f64(ctx, name, fields.next(), declaration)?;
                if tolerance.get() <= 0.0 {
                    return Err(malformed(
                        ctx,
                        name,
                        format_args!("{declaration} is not positive"),
                    ));
                }
                require_end(ctx, name, &mut fields, declaration)?;
                if !ctx.insert_btree_set(
                    &mut terminal_declarations,
                    declaration,
                    "index T-spline terminal declarations",
                )? {
                    return Err(malformed(
                        ctx,
                        name,
                        format_args!("duplicate {declaration}"),
                    ));
                }
            }
            Some(declaration @ ("ver" | "behavior-version" | "compat-version")) => {
                if fields.next().is_none() {
                    return Err(malformed(
                        ctx,
                        name,
                        format_args!("missing {declaration} value"),
                    ));
                }
                require_end(ctx, name, &mut fields, declaration)?;
                if !ctx.insert_btree_set(
                    &mut terminal_declarations,
                    declaration,
                    "index T-spline terminal declarations",
                )? {
                    return Err(malformed(
                        ctx,
                        name,
                        format_args!("duplicate {declaration}"),
                    ));
                }
            }
            Some(kind) => {
                if let Some(count) = ctx.get_mut_btree_map(
                    &mut unknown_record_kinds,
                    kind,
                    "count T-spline unknown record kinds",
                )? {
                    *count = (*count).checked_add(1).ok_or_else(|| {
                        ctx.refuse_codec_limit(
                            "count T-spline unknown record kinds",
                            u64::MAX - 1,
                            u64::MAX,
                        )
                    })?;
                } else {
                    let key =
                        ctx.copy_retained_text(kind, "retain T-spline unknown record kind")?;
                    ctx.insert_btree_map(
                        &mut unknown_record_kinds,
                        key,
                        1,
                        "index T-spline unknown record kinds",
                    )?;
                }
            }
            None => {}
        }
    }
    if let Some(block) = current_symmetry {
        ctx.push_vec(&mut symmetry_blocks, block, "read T-spline symmetry blocks")?;
    }

    let edge_knot_mirror = !edge_knot_records.is_empty()
        && end_conditions == Some("SUBD_CREASES")
        && edge_knot_records.len() == edge_roots.len()
        && ctx.all_by(
            0..edge_knot_records.len().min(edge_knot_intervals.len()),
            |index| {
                let record = edge_knot_records[index];
                Ok(match edge_knot_intervals[index] {
                    Some(interval) => {
                        record.get() > 0.0
                            && (record.get() - interval.get()).abs()
                                <= EDGE_KNOT_MIRROR_RELATIVE_EPS
                                    * record.get().abs().max(interval.get().abs()).max(1.0)
                    }
                    None => record.get() == -1.0,
                })
            },
            "match T-spline mirrored edge knots",
        )?;
    if !edge_knot_records.is_empty() && !edge_knot_mirror {
        if let Some(count) = ctx.get_mut_btree_map(
            &mut unknown_record_kinds,
            "106ek",
            "count T-spline unknown record kinds",
        )? {
            *count = (*count)
                .checked_add(edge_knot_records.len())
                .ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "count T-spline unknown record kinds",
                        u64::MAX - 1,
                        u64::MAX,
                    )
                })?;
        } else {
            ctx.insert_btree_map(
                &mut unknown_record_kinds,
                ctx.copy_retained_text("106ek", "retain T-spline unknown record kind")?,
                edge_knot_records.len(),
                "index T-spline unknown record kinds",
            )?;
        }
    }

    let live_vertices = ctx
        .admit_iter(&vertex_live, "count T-spline live vertices")?
        .filter(|live| **live)
        .count();
    if declarations.len() != 6
        || !ctx.any_by(
            &face_roots,
            |value| Ok(Option::is_some(value)),
            "check T-spline face roots",
        )?
        || !ctx.any_by(
            &edge_roots,
            |value| Ok(Option::is_some(value)),
            "check T-spline edge roots",
        )?
        || live_vertices == 0
        || !ctx.any_by(
            &half_edges,
            |value| Ok(Option::is_some(value)),
            "check T-spline half-edges",
        )?
        || (!grip_vertices.is_empty() && grip_vertices.len() != grip_points.len())
    {
        return Err(malformed(ctx, name, "control cage is incomplete"));
    }
    let half_edges = compact_half_edges(
        ctx,
        name,
        &half_edges,
        &mut face_roots,
        &mut edge_roots,
        &mut vertex_roots,
    )?;

    let face_live = ctx.collect_vec(
        ctx.admit_iter(&face_roots, "index T-spline live faces")?
            .map(Option::is_some),
        "index T-spline live faces",
    )?;
    let edge_live = ctx.collect_vec(
        ctx.admit_iter(&edge_roots, "index T-spline live edges")?
            .map(Option::is_some),
        "index T-spline live edges",
    )?;
    for edge in ctx.admit_iter(&selected_edges, "validate T-spline edge selections")? {
        if !edge_live.get(*edge).copied().unwrap_or(false) {
            return Err(malformed(
                ctx,
                name,
                "selected edge is out of range or deleted",
            ));
        }
    }
    for vertex in ctx.admit_iter(&selected_vertices, "validate T-spline vertex selections")? {
        if !vertex_live.get(*vertex).copied().unwrap_or(false) {
            return Err(malformed(
                ctx,
                name,
                "selected vertex is out of range or deleted",
            ));
        }
    }
    for grip in ctx.admit_iter(&selected_grips, "validate T-spline grip selections")? {
        if !grip_points.get(*grip).is_some_and(Option::is_some) {
            return Err(malformed(
                ctx,
                name,
                "selected grip is out of range or deleted",
            ));
        }
    }
    let typed_symmetry_blocks = ctx.try_collect_vec(
        symmetry_blocks
            .into_iter()
            .map(|block| -> Result<_, CodecError> {
                let plane = block
                    .plane
                    .ok_or_else(|| malformed(ctx, name, "symmetry block has no plane"))?;
                let kind = match block.mode {
                    SymmetryMode::Correspondence => {
                        validate_symmetry_map(
                            ctx,
                            name,
                            &block.face_forward,
                            &block.face_reverse,
                            &face_live,
                            "face",
                        )?;
                        validate_symmetry_map(
                            ctx,
                            name,
                            &block.edge_forward,
                            &block.edge_reverse,
                            &edge_live,
                            "edge",
                        )?;
                        validate_symmetry_map(
                            ctx,
                            name,
                            &block.vertex_forward,
                            &block.vertex_reverse,
                            &vertex_live,
                            "vertex",
                        )?;
                        SymmetryKind::Correspondence {
                            face: block.face_forward,
                            edge: block.edge_forward,
                            vertex: block.vertex_forward,
                        }
                    }
                    SymmetryMode::Radial => {
                        let segments = block.radial_segments.ok_or_else(|| {
                            malformed(ctx, name, "radial symmetry block is missing segments")
                        })?;
                        let sweep = block.radial_sweep.ok_or_else(|| {
                            malformed(ctx, name, "radial symmetry block is missing sweep")
                        })?;
                        for selector in [
                            SubdRadialMapSelector::Ef,
                            SubdRadialMapSelector::Er,
                            SubdRadialMapSelector::Ff,
                            SubdRadialMapSelector::Fr,
                            SubdRadialMapSelector::Vf,
                            SubdRadialMapSelector::Vr,
                        ] {
                            if !ctx.any_by(
                                &block.radial_maps,
                                |map| Ok(map.selector == selector),
                                "find T-spline radial symmetry map",
                            )? {
                                return Err(malformed(
                                    ctx,
                                    name,
                                    format_args!(
                                        "radial symmetry block is missing {}",
                                        match selector {
                                            SubdRadialMapSelector::Ef => "ef",
                                            SubdRadialMapSelector::Er => "er",
                                            SubdRadialMapSelector::Ff => "ff",
                                            SubdRadialMapSelector::Fr => "fr",
                                            SubdRadialMapSelector::Vf => "vf",
                                            SubdRadialMapSelector::Vr => "vr",
                                        }
                                    ),
                                ));
                            }
                        }
                        SymmetryKind::Radial {
                            segments,
                            sweep,
                            maps: block.radial_maps,
                        }
                    }
                };
                Ok(SymmetryBlock { plane, kind })
            }),
        "type T-spline symmetry blocks",
    )?;
    let mut grip_owners =
        ctx.alloc_filled(grip_vertices.len(), None, "f3d subd secondary-grip owners")?;
    for connectivity in ctx.admit_iter(
        &derived_grips,
        "validate T-spline derived-grip connectivity",
    )? {
        if !vertex_live
            .get(connectivity.vertex)
            .copied()
            .unwrap_or(false)
        {
            return Err(malformed(
                ctx,
                name,
                "derived-grip connectivity is out of range",
            ));
        }
        for &index in ctx
            .admit_iter(
                &connectivity.grip_indices,
                "validate T-spline derived-grip entries",
            )?
            .flatten()
        {
            if !matches!(grip_vertices.get(index), Some(GripVertexMarker::Secondary(Some(vertex))) if *vertex == connectivity.vertex)
            {
                return Err(malformed(
                    ctx,
                    name,
                    "derived-grip entry is not a secondary grip of its vertex",
                ));
            }
            let owner_slot = grip_owners
                .get_mut(index)
                .ok_or_else(|| malformed(ctx, name, "derived-grip entry is out of range"))?;
            if owner_slot.replace(connectivity.vertex).is_some() {
                return Err(malformed(
                    ctx,
                    name,
                    "secondary grip is named more than once",
                ));
            }
        }
    }
    for (index, marker) in ctx
        .admit_iter(&grip_vertices, "validate T-spline secondary-grip owners")?
        .enumerate()
    {
        if let GripVertexMarker::Secondary(Some(vertex)) = marker {
            if grip_owners[index] != Some(*vertex) {
                return Err(malformed(
                    ctx,
                    name,
                    "secondary grip is not named exactly once by derived connectivity",
                ));
            }
        }
    }
    for (marker, point) in ctx
        .admit_iter(&grip_vertices, "validate T-spline grip point map")?
        .zip(ctx.admit_iter(&grip_points, "validate T-spline grip point values")?)
    {
        match marker {
            GripVertexMarker::Primary(vertex) | GripVertexMarker::Secondary(Some(vertex)) => {
                if point.is_none() || !vertex_live.get(*vertex).copied().unwrap_or(false) {
                    return Err(malformed(ctx, name, "grip vertex map is inconsistent"));
                }
            }
            GripVertexMarker::Secondary(None) if point.is_some() => {
                return Err(malformed(ctx, name, "deleted grip marker has a point"));
            }
            GripVertexMarker::Secondary(None) => {}
        }
    }
    for (index, half) in ctx
        .admit_iter(&half_edges, "validate T-spline half-edge topology")?
        .enumerate()
    {
        let id = HalfEdgeId(index);
        let mate = &half_edges[half.mate.index()];
        let next = &half_edges[half.next.index()];
        let previous = &half_edges[half.previous.index()];
        if mate.mate != id
            || next.previous != id
            || previous.next != id
            || !vertex_live.get(half.vertex).copied().unwrap_or(false)
        {
            return Err(malformed(ctx, name, "half-edge topology is inconsistent"));
        }
    }
    for (vertex, live) in ctx
        .admit_iter(&vertex_live, "validate T-spline vertex fans")?
        .copied()
        .enumerate()
    {
        if live {
            let (root, _) = vertex_roots
                .get(vertex)
                .copied()
                .flatten()
                .ok_or_else(|| malformed(ctx, name, "live vertex has no root direction"))?;
            build_fan(ctx, name, vertex, root, &half_edges, &face_live)?;
        }
    }

    // Slot indices address the program; IR indices address only populated slots.
    let vertex_ir = compact(ctx, &vertex_live)?;
    let edge_ir = compact(ctx, &edge_live)?;
    let face_ir = compact(ctx, &face_live)?;
    let symmetries = ctx.try_collect_vec(
        typed_symmetry_blocks
            .into_iter()
            .map(|block| -> Result<_, CodecError> {
                let plane = symmetry_plane(ctx, name, block.plane)?;
                let (kind, face_pairs, edge_pairs, vertex_pairs) = match block.kind {
                    SymmetryKind::Correspondence { face, edge, vertex } => (
                        SubdSymmetryKind::Correspondence {},
                        remap_symmetry_pairs(ctx, name, &face, &face_ir, "face")?,
                        remap_symmetry_pairs(ctx, name, &edge, &edge_ir, "edge")?,
                        remap_symmetry_pairs(ctx, name, &vertex, &vertex_ir, "vertex")?,
                    ),
                    SymmetryKind::Radial {
                        segments,
                        sweep,
                        maps,
                    } => (
                        SubdSymmetryKind::radial_from_parts(segments, sweep, maps, ctx)?
                            .map_err(|error| malformed(ctx, name, error))?,
                        Vec::new(),
                        Vec::new(),
                        Vec::new(),
                    ),
                };
                let symmetry =
                    SubdSymmetry::new(kind, plane, face_pairs, edge_pairs, vertex_pairs, ctx)?
                        .map_err(|error| malformed(ctx, name, error))?;
                Ok(symmetry)
            }),
        "project T-spline symmetries",
    )?;
    let edge_knot_intervals_ir = ctx.collect_vec(
        ctx.admit_iter(&edge_knot_intervals, "project T-spline edge knot intervals")?
            .copied()
            .flatten(),
        "project T-spline edge knot intervals",
    )?;
    let vertex_of = |slot: usize| {
        vertex_ir
            .get(slot)
            .copied()
            .flatten()
            .ok_or_else(|| malformed(ctx, name, "half-edge names a deleted vertex slot"))
    };

    let mut vertex_points = BTreeMap::new();
    if grip_vertices.is_empty() {
        if grip_points.len() != vertex_live.len() {
            return Err(malformed(
                ctx,
                name,
                "positional grip vertex map is incomplete",
            ));
        }
        for (slot, point) in ctx
            .admit_iter(&grip_points, "index positional T-spline grip points")?
            .enumerate()
        {
            if let (true, Some(point)) = (vertex_live[slot], point) {
                let vertex = vertex_of(slot)?;
                ctx.insert_btree_map(
                    &mut vertex_points,
                    vertex,
                    point.point,
                    "index T-spline vertex points",
                )?;
            }
        }
    } else {
        for (marker, point) in ctx
            .admit_iter(&grip_vertices, "index T-spline grip vertex points")?
            .zip(ctx.admit_iter(&grip_points, "index T-spline grip point values")?)
        {
            let (GripVertexMarker::Primary(slot), Some(point)) = (marker, point) else {
                continue;
            };
            let vertex = vertex_of(*slot)?;
            if ctx.contains_key_btree_map(
                &vertex_points,
                &vertex,
                "check T-spline primary grip vertex",
            )? {
                return Err(malformed(
                    ctx,
                    name,
                    "primary grip vertex map is inconsistent",
                ));
            }
            ctx.insert_btree_map(
                &mut vertex_points,
                vertex,
                point.point,
                "index T-spline vertex points",
            )?;
        }
    }
    if vertex_points.len() != live_vertices {
        return Err(malformed(
            ctx,
            name,
            "primary grip vertex map is incomplete",
        ));
    }

    let mut edge_by_half =
        ctx.alloc_filled(half_edges.len(), None, "f3d subd half-edge ownership")?;
    let mut edge_vertices = Vec::new();
    let mut edge = 0_u32;
    for root in ctx.admit_iter(&edge_roots, "index T-spline edge roots")? {
        let Some(root) = *root else {
            continue;
        };
        let current_edge = edge;
        edge = edge.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit(
                "count T-spline edges",
                u64::from(u32::MAX),
                u64::from(u32::MAX) + 1,
            )
        })?;
        let half = &half_edges[root];
        if edge_by_half[root].replace((current_edge, false)).is_some()
            || edge_by_half[half.mate.index()]
                .replace((current_edge, true))
                .is_some()
        {
            return Err(malformed(ctx, name, "edge roots reuse a half-edge"));
        }
        let mate = &half_edges[half.mate.index()];
        ctx.push_vec(
            &mut edge_vertices,
            [vertex_of(mate.vertex)?, vertex_of(half.vertex)?],
            "project T-spline edge vertices",
        )?;
    }
    if ctx.any_by(
        &edge_by_half,
        |value| Ok(Option::is_none(value)),
        "validate T-spline half-edge ownership",
    )? {
        return Err(malformed(
            ctx,
            name,
            "edge roots do not cover every half-edge",
        ));
    }
    if edge_knot_intervals_ir.len() != edge_vertices.len() {
        return Err(malformed(ctx, name, "edge knot interval map is incomplete"));
    }

    let mut secondary_layouts = build_secondary_layouts(
        ctx,
        &SecondaryLayoutContext {
            name,
            vertex_roots: &vertex_roots,
            vertex_live: &vertex_live,
            vertex_ir: &vertex_ir,
            face_live: &face_live,
            face_ir: &face_ir,
            half_edges: &half_edges,
            edge_by_half: &edge_by_half,
            grip_vertices: &grip_vertices,
            grip_points: &grip_points,
        },
        &derived_grips,
    )?;

    let mut faces = Vec::new();
    for (face_slot, start) in ctx
        .admit_iter(&face_roots, "project T-spline face rings")?
        .copied()
        .enumerate()
    {
        let Some(start) = start else { continue };
        let start_id = HalfEdgeId(start);
        let mut ring = Vec::new();
        let mut current = start_id;
        loop {
            ctx.charge_work(1, "walk T-spline face ring")?;
            let half = &half_edges[current.index()];
            if half.face != Some(face_slot) {
                return Err(malformed(
                    ctx,
                    name,
                    "face ring carries a different face index",
                ));
            }
            let (edge, reversed) = edge_by_half[current.index()]
                .ok_or_else(|| malformed(ctx, name, "face half-edge has no edge"))?;
            ctx.push_vec(
                &mut ring,
                SubdEdgeUse { edge, reversed },
                "project T-spline face ring",
            )?;
            current = half.next;
            if current == start_id {
                break;
            }
            if ring.len() > half_edges.len() {
                return Err(malformed(ctx, name, "face ring does not close"));
            }
        }
        ctx.push_vec(
            &mut faces,
            SubdFace::new(ring).map_err(|error| malformed(ctx, name, error))?,
            "project T-spline faces",
        )?;
    }

    let mut crease_incidence =
        ctx.alloc_filled(live_vertices, 0usize, "f3d subd crease incidence")?;
    for edge in ctx.admit_iter(&crease_edges, "index T-spline crease incidence")? {
        let vertices = edge_ir
            .get(*edge)
            .copied()
            .flatten()
            .and_then(|edge| edge_vertices.get(index_from_u32(edge)))
            .ok_or_else(|| malformed(ctx, name, "crease edge is out of range"))?;
        crease_incidence[index_from_u32(vertices[0])] += 1;
        crease_incidence[index_from_u32(vertices[1])] += 1;
    }
    let mut vertices = Vec::new();
    for index in ctx.admit_iter(&(0..live_vertices), "project T-spline vertices")? {
        let secondary_layout = secondary_layouts[index].take();
        let vertex_index = u32::try_from(index)
            .map_err(|_| malformed(ctx, name, "T-spline vertex index exceeds u32"))?;
        let point = ctx
            .get_btree_map(&vertex_points, &vertex_index, "find T-spline vertex point")?
            .copied()
            .ok_or_else(|| malformed(ctx, name, "T-spline vertex point is missing"))?;
        let crease_count = crease_incidence
            .get(index)
            .copied()
            .ok_or_else(|| malformed(ctx, name, "T-spline crease incidence is missing"))?;
        let vertex = SubdVertex::new(
            point,
            match crease_count {
                0 => SubdVertexTag::Smooth,
                1 => SubdVertexTag::Dart,
                2 => SubdVertexTag::Crease,
                _ => SubdVertexTag::Corner,
            },
            secondary_layout,
        )
        .map_err(|error| malformed(ctx, name, error))?;
        ctx.push_vec(&mut vertices, vertex, "project T-spline vertices")?;
    }
    let mut creased_edges = BTreeSet::new();
    for slot in ctx.admit_iter(&crease_edges, "index T-spline creased edges")? {
        if let Some(edge) = edge_ir.get(*slot).copied().flatten() {
            ctx.insert_btree_set(&mut creased_edges, edge, "index T-spline creased edges")?;
        }
    }
    let mut edges = Vec::new();
    for (index, vertices) in ctx
        .admit_iter(&edge_vertices, "project T-spline edges")?
        .copied()
        .enumerate()
    {
        let edge_index = u32::try_from(index)
            .map_err(|_| malformed(ctx, name, "T-spline edge index exceeds u32"))?;
        let crease =
            ctx.contains_btree_set(&creased_edges, &edge_index, "classify T-spline crease edge")?;
        let sharpness = if crease { FULL_CREASE_SHARPNESS } else { 0.0 };
        let edge = SubdEdge::from_parts(
            vertices,
            [sharpness; 2],
            if crease {
                SubdEdgeTag::Crease
            } else {
                SubdEdgeTag::Smooth
            },
            Some(edge_knot_intervals_ir[index]),
            [0.0, 0.0],
            ctx,
        )?
        .map_err(|error| malformed(ctx, name, error))?;
        ctx.push_vec(&mut edges, edge, "project T-spline edges")?;
    }
    let name_base = ctx
        .rsplit_once(name, "/", "find T-spline source name")?
        .map_or(name, |(_, base)| base);
    let source_key = ctx
        .strip_suffix(name_base, ".tsm", "remove T-spline file extension")?
        .unwrap_or(name);
    Ok(ParsedCage {
        surface: SubdSurface {
            id: subd_id(ctx, name, source_key)?,
            scheme: SubdScheme::CatmullClark,
            source_object: Some(SourceObjectAssociation {
                format: cadmpeg_ir::CodecFormat::F3d,
                object_id: cadmpeg_core::text::NonBlankString::for_decode(
                    ctx,
                    ctx.copy_retained_text(name, "retain T-spline source object ID")?,
                    "validate nonblank text",
                )?
                .ok_or_else(|| malformed(ctx, name, "source object_id must not be empty"))?,
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
            cage: cadmpeg_ir::subd::SubdCage::new(vertices, edges, faces, symmetries, ctx)?
                .map_err(|error| malformed(ctx, name, error))?,
        },
        unknown_record_kinds,
    })
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::subd;
    use cadmpeg_test_support::wire;

    use super::SubdGripWedge;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    const EPS_KNOT_INTERVAL: f64 = 1.0e-12;

    const QUAD_TOPOLOGY: &str = "degree 3\n\
cap-type G1CAPS\n\
star-smoothness 0\n\
units 1 meters\n\
end-conditions SUBD_CREASES\n\
star-knot-rule NURCCS\n\
f 0 0\n\
e 0 1\ne 2 1\ne 4 1\ne 6 1\n\
v 0 NORTH\nv 2 NORTH\nv 4 NORTH\nv 6 NORTH\n\
l 2 6 1 0 0 0 0\nl 7 3 0 3 -1 0 0\n\
l 4 0 3 1 0 0 0\nl 1 5 2 0 -1 0 0\n\
l 6 2 5 2 0 0 0\nl 3 7 4 1 -1 0 0\n\
l 0 4 7 3 0 0 0\nl 5 1 6 2 -1 0 0\n\
ec 0 0\nec 1 0\nec 2 0\nec 3 0\n";

    fn parse_cage(bytes: &[u8]) -> Result<super::ParsedCage, cadmpeg_core::CodecError> {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &DecodePolicy::default())
            .expect("test decode context");
        super::parse(&ctx, "synthetic.tsm", bytes)
    }

    fn compact_half_edge_limit(limit: u64) -> cadmpeg_core::CodecError {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        super::compact_half_edges(
            &ctx,
            "synthetic.tsm",
            &[Some(super::ParsedHalfEdge {
                next: 0,
                previous: 0,
                mate: 0,
                vertex: 0,
                face: -1,
            })],
            &mut [Some(0)],
            &mut [Some(0)],
            &mut [Some((0, super::SubdGripDirection::North))],
        )
        .err()
        .expect("half-edge compaction must refuse the configured limit")
    }

    #[test]
    fn tsm_half_edge_slot_map_refuses_collection_limit() {
        let error = compact_half_edge_limit(0);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "map T-spline half-edge slots")
        );
    }

    #[test]
    fn tsm_live_half_edge_list_refuses_collection_limit() {
        let error = compact_half_edge_limit(1);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "collect T-spline live half-edges")
        );
    }

    #[test]
    fn tsm_compact_half_edge_list_refuses_collection_limit() {
        let error = compact_half_edge_limit(2);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "collect T-spline compact half-edges")
        );
    }

    #[test]
    fn tsm_slot_compaction_refuses_collection_limit() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::compact(&ctx, &[true, false]).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "compact T-spline slots")
        );
    }

    fn parse_quad_limit(limit: u64) -> cadmpeg_core::CodecError {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0m odd-grip-map\n0m gvp 0\n0m gvp 1\n0m gvp 2\n0m gvp 3\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
        );
        let mut policy = DecodePolicy::service();
        // The ceiling counts topology slots, line and field views, and six declaration entries.
        let views = source.lines().count()
            + source
                .lines()
                .map(|line| line.split_ascii_whitespace().count())
                .sum::<usize>();
        let declarations = source
            .lines()
            .filter(|line| {
                matches!(
                    line.split_ascii_whitespace().next(),
                    Some(
                        "degree"
                            | "cap-type"
                            | "star-smoothness"
                            | "units"
                            | "end-conditions"
                            | "star-knot-rule"
                    )
                )
            })
            .count();
        policy.limits.max_collection_items = limit
            .checked_add(u64::try_from(views).unwrap())
            .unwrap()
            .checked_add(u64::try_from(declarations).unwrap())
            .unwrap();
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy).unwrap();
        super::parse(&ctx, "synthetic.tsm", source.as_bytes()).unwrap_err()
    }

    fn parse_small_limit(source: &str, items: u64, retained: u64) -> cadmpeg_core::CodecError {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = items;
        policy.limits.max_retained_bytes = retained;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy).unwrap();
        super::parse(&ctx, "synthetic.tsm", source.as_bytes()).unwrap_err()
    }

    #[test]
    fn tsm_loss_text_refuses_retained_limit() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 3;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = ctx
            .format_retained(
                format_args!("{}", "four"),
                "describe undecoded T-spline cage",
            )
            .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "describe undecoded T-spline cage")
        );
    }

    #[test]
    fn tsm_malformed_record_text_refuses_retained_limit() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let source = b"invalid header";
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy).unwrap();
        let error = super::parse(&ctx, "long-name.tsm", source).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "describe malformed T-spline cage")
        );
    }

    #[test]
    fn tsm_source_identity_key_refuses_retained_limit() {
        let source = quad_source();
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "retain T-spline identity key",
            |cap| Err::<(), cadmpeg_core::CodecError>(parse_small_limit(&source, u64::MAX, cap)),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "retain T-spline identity key")
        );
    }

    #[test]
    fn tsm_source_identity_refuses_retained_limit() {
        let source = quad_source();
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "retain T-spline identity",
            |cap| Err::<(), cadmpeg_core::CodecError>(parse_small_limit(&source, u64::MAX, cap)),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "retain T-spline identity")
        );
    }

    #[test]
    fn tsm_source_object_id_refuses_retained_limit() {
        let source = quad_source();
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "retain T-spline source object ID",
            |cap| Err::<(), cadmpeg_core::CodecError>(parse_small_limit(&source, u64::MAX, cap)),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "retain T-spline source object ID")
        );
    }

    #[test]
    fn tsm_charged_source_identity_matches_composed_identity() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let actual = super::subd_id(&ctx, "synthetic.tsm", "synthetic").unwrap();
        assert_eq!(actual.as_str(), "f3d:tspline:subd#synthetic");
    }

    #[test]
    fn tsm_cage_collection_refuses_item_limit() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut cages = Vec::new();
        let error = ctx
            .push_vec(&mut cages, 1_u8, "collect T-spline cages")
            .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "collect T-spline cages")
        );
    }

    #[test]
    fn tsm_loss_collection_refuses_item_limit() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut losses = Vec::new();
        let error = ctx
            .push_vec(&mut losses, 1_u8, "collect T-spline loss notes")
            .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "collect T-spline loss notes")
        );
    }

    fn quad_source() -> String {
        format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0m odd-grip-map\n0m gvp 0\n0m gvp 1\n0m gvp 2\n0m gvp 3\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
        )
    }

    fn derived_quad_source() -> String {
        format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0m odd-grip-map\n0m gvp 0\n0m gvp 1\n0m gvp 2\n0m gvp 3\n0m gv 0\n\
             0m cg 0 4 1 0 0 0 4\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n0g 0.5 0 0 1\n"
        )
    }

    #[test]
    #[cfg(target_pointer_width = "32")]
    fn secondary_grip_vertex_refuses_32_bit_wrap() {
        let source = derived_quad_source();
        parse_cage(source.as_bytes()).expect("fixture derived grip is valid");
        let invalid = source.replace("0m gv 0\n", "0m gv 4294967296\n");
        let error =
            parse_cage(invalid.as_bytes()).expect_err("secondary grip index exceeds address space");
        assert!(error
            .to_string()
            .contains("secondary grip vertex exceeds address space"));
    }

    fn symmetry_quad_source() -> String {
        format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0m odd-grip-map\n0m gvp 0\n0m gvp 1\n0m gvp 2\n0m gvp 3\n0m gv 0\n\
             0m cg 0 4 1 0 0 0 4\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n0g 0.5 0 0 1\n\
             100edges 0 2\n100verts 1\n50000grip 0\n50000grip 1\n\
             105sym 0\n105plane 0 2 0 1 0 1 0 0 0 0 1 0\n\
             105a fr 0 0\n105a er 0 0 1 2\n105a e 2 1\n\
             105a vr 0 1\n105a v 1 0\n\
             tol 0.00001\nver 6021\nbehavior-version 6.5.0\n"
        )
    }

    fn refusal_at_operation(source: &str, operation: &str) -> cadmpeg_core::CodecError {
        for limit in 0..512 {
            let error = parse_small_limit(source, limit, u64::MAX);
            if matches!(&error, cadmpeg_core::CodecError::ResourceLimit(refusal)
                if refusal.operation == operation)
            {
                return error;
            }
        }
        panic!("operation {operation} did not refuse a collection limit");
    }

    macro_rules! tsm_quad_projection_limit_test {
        ($name:ident, $source:expr, $operation:literal) => {
            #[test]
            fn $name() {
                let source = $source;
                let error = refusal_at_operation(&source, $operation);
                assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.operation == $operation));
            }
        };
    }

    tsm_quad_projection_limit_test!(
        tsm_edge_knot_projection_refuses_collection_limit,
        quad_source(),
        "project T-spline edge knot intervals"
    );
    tsm_quad_projection_limit_test!(
        tsm_vertex_point_index_refuses_collection_limit,
        quad_source(),
        "index T-spline vertex points"
    );
    tsm_quad_projection_limit_test!(
        tsm_edge_vertex_projection_refuses_collection_limit,
        quad_source(),
        "project T-spline edge vertices"
    );
    tsm_quad_projection_limit_test!(
        tsm_face_ring_refuses_collection_limit,
        quad_source(),
        "project T-spline face ring"
    );
    tsm_quad_projection_limit_test!(
        tsm_face_projection_refuses_collection_limit,
        quad_source(),
        "project T-spline faces"
    );
    tsm_quad_projection_limit_test!(
        tsm_vertex_projection_refuses_collection_limit,
        quad_source(),
        "project T-spline vertices"
    );

    #[test]
    fn tsm_vertex_projection_refuses_work_limit() {
        let source = quad_source();
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "project T-spline vertices",
            0,
            |ctx| super::parse(ctx, "synthetic.tsm", source.as_bytes()).map(|_| ()),
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                    && limit.operation == "project T-spline vertices"
        ));
    }

    tsm_quad_projection_limit_test!(
        tsm_creased_edge_index_refuses_collection_limit,
        quad_source(),
        "index T-spline creased edges"
    );
    tsm_quad_projection_limit_test!(
        tsm_edge_projection_refuses_collection_limit,
        quad_source(),
        "project T-spline edges"
    );
    tsm_quad_projection_limit_test!(
        tsm_derived_wedges_refuse_collection_limit,
        derived_quad_source(),
        "project T-spline grip wedges"
    );
    tsm_quad_projection_limit_test!(
        tsm_typed_symmetry_refuses_collection_limit,
        symmetry_quad_source(),
        "type T-spline symmetry blocks"
    );
    tsm_quad_projection_limit_test!(
        tsm_projected_symmetry_refuses_collection_limit,
        symmetry_quad_source(),
        "project T-spline symmetries"
    );

    #[test]
    fn tsm_typed_symmetry_source_refuses_work_limit() {
        let source = symmetry_quad_source();
        let cage = parse_cage(source.as_bytes()).expect("valid symmetry source");
        assert_eq!(
            wire::field_or_default::<Vec<subd::SubdSymmetry>>(&(cage.surface.cage), "symmetries")
                .len(),
            1
        );
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "type T-spline symmetry blocks",
            0,
            |ctx| super::parse(ctx, "synthetic.tsm", source.as_bytes()).map(|_| ()),
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                    && limit.operation == "type T-spline symmetry blocks"
        ));
    }

    #[test]
    fn tsm_projected_symmetry_source_refuses_work_limit() {
        let source = symmetry_quad_source();
        let cage = parse_cage(source.as_bytes()).expect("valid symmetry source");
        assert_eq!(
            wire::field_or_default::<Vec<subd::SubdSymmetry>>(&(cage.surface.cage), "symmetries")
                .len(),
            1
        );
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "project T-spline symmetries",
            0,
            |ctx| super::parse(ctx, "synthetic.tsm", source.as_bytes()).map(|_| ()),
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                    && limit.operation == "project T-spline symmetries"
        ));
    }

    fn fan_limit(items: u64) -> cadmpeg_core::CodecError {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = items;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let half_edges = [super::HalfEdge {
            next: super::HalfEdgeId(0),
            previous: super::HalfEdgeId(0),
            mate: super::HalfEdgeId(0),
            vertex: 0,
            face: None,
        }];
        super::build_fan(&ctx, "synthetic.tsm", 0, 0, &half_edges, &[])
            .err()
            .expect("fan must refuse the configured limit")
    }

    #[test]
    fn tsm_fan_seen_index_refuses_collection_limit() {
        let error = fan_limit(0);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index T-spline fan half-edges")
        );
    }

    #[test]
    fn tsm_fan_slots_refuse_collection_limit() {
        let error = fan_limit(1);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "collect T-spline fan slots")
        );
    }

    #[test]
    fn tsm_fan_phantoms_refuse_collection_limit() {
        let error = fan_limit(2);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "complete T-spline fan gaps")
        );
    }

    #[test]
    fn tsm_fan_gap_completion_refuses_work_limit() {
        let half_edges = [super::HalfEdge {
            next: super::HalfEdgeId(0),
            previous: super::HalfEdgeId(0),
            mate: super::HalfEdgeId(0),
            vertex: 0,
            face: None,
        }];
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "complete T-spline fan gaps",
            0,
            |ctx| super::build_fan(ctx, "synthetic.tsm", 0, 0, &half_edges, &[]).map(|_| ()),
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                    && limit.operation == "complete T-spline fan gaps"
        ));
    }

    #[test]
    fn tsm_fan_phantom_moves_preserve_work_refusal() {
        let half_edges = [super::HalfEdge {
            next: super::HalfEdgeId(0),
            previous: super::HalfEdgeId(0),
            mate: super::HalfEdgeId(0),
            vertex: 0,
            face: None,
        }];
        crate::test_support::with_decode_context(|ctx| {
            let fan = super::build_fan(ctx, "synthetic.tsm", 0, 0, &half_edges, &[]).unwrap();
            assert_eq!(fan.len(), 4);
            assert!(matches!(
                fan[0],
                super::FanSlot::Slot {
                    half_edge: 0,
                    face: None
                }
            ));
            assert!(fan[1..]
                .iter()
                .all(|slot| matches!(slot, super::FanSlot::Phantom)));
        });
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "move T-spline fan slots",
            0,
            |ctx| super::build_fan(ctx, "synthetic.tsm", 0, 0, &half_edges, &[]).map(|_| ()),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "move T-spline fan slots")
        );
    }

    #[test]
    fn tsm_grip_block_refuses_collection_limit() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::grip_block(&ctx, "synthetic.tsm", &[], &[None], &mut 0, 1).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "materialize T-spline grip block")
        );
    }

    #[test]
    fn tsm_symmetry_pair_remap_refuses_collection_limit() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let map = std::collections::BTreeMap::from([(0, 1)]);
        let error =
            super::remap_symmetry_pairs(&ctx, "synthetic.tsm", &map, &[Some(0), Some(1)], "face")
                .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "remap T-spline symmetry pairs")
        );
    }

    macro_rules! tsm_parse_collection_limit_test {
        ($name:ident, $source:literal, $operation:literal) => {
            #[test]
            fn $name() {
                // The ceiling includes line views, field views and preceding parser slots.
                let error = crate::test_support::resource_refusal_at(
                    cadmpeg_core::decode::ResourceDimension::CollectionItems,
                    $operation,
                    0,
                    |ctx| super::parse(ctx, "synthetic.tsm", $source.as_bytes()).map(|_| ()),
                );
                assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.operation == $operation), "{error:?}");
            }
        };
    }

    tsm_parse_collection_limit_test!(
        tsm_face_slot_refuses_collection_limit,
        "#TS0200\nf\n",
        "read T-spline face slots"
    );
    tsm_parse_collection_limit_test!(
        tsm_edge_slot_refuses_collection_limit,
        "#TS0200\ne\n",
        "read T-spline edge slots"
    );
    tsm_parse_collection_limit_test!(
        tsm_edge_knot_interval_refuses_collection_limit,
        "#TS0200\ne\n",
        "read T-spline edge knot intervals"
    );
    tsm_parse_collection_limit_test!(
        tsm_edge_knot_record_refuses_collection_limit,
        "#TS0200\n106ek 1\n",
        "read T-spline edge knot records"
    );
    tsm_parse_collection_limit_test!(
        tsm_vertex_flag_refuses_collection_limit,
        "#TS0200\nv\n",
        "read T-spline vertex live flags"
    );
    tsm_parse_collection_limit_test!(
        tsm_vertex_root_refuses_collection_limit,
        "#TS0200\nv\n",
        "read T-spline vertex roots"
    );
    tsm_parse_collection_limit_test!(
        tsm_half_edge_slot_refuses_collection_limit,
        "#TS0200\nl\n",
        "read T-spline half-edge slots"
    );
    tsm_parse_collection_limit_test!(
        tsm_grip_marker_refuses_collection_limit,
        "#TS0200\n0m odd-grip-map\n0m gvp 0\n",
        "read T-spline grip vertex markers"
    );
    tsm_parse_collection_limit_test!(
        tsm_grip_point_refuses_collection_limit,
        "#TS0200\n0g\n",
        "read T-spline grip points"
    );
    tsm_parse_collection_limit_test!(
        tsm_derived_spokes_refuses_collection_limit,
        "#TS0200\n0m odd-grip-map\n0m cg 0 1 0\n",
        "read T-spline derived-grip spokes"
    );
    tsm_parse_collection_limit_test!(
        tsm_derived_indices_refuses_collection_limit,
        "#TS0200\n0m odd-grip-map\n0m cg 0 1 1 0 0\n",
        "read T-spline derived-grip indices"
    );
    tsm_parse_collection_limit_test!(
        tsm_derived_grip_record_refuses_collection_limit,
        "#TS0200\n0m odd-grip-map\n0m cg 0 1 0\n",
        "read T-spline derived grips"
    );
    tsm_parse_collection_limit_test!(
        tsm_symmetry_block_refuses_collection_limit,
        "#TS0200\n105sym 0\n105sym 0\n",
        "read T-spline symmetry blocks"
    );
    tsm_parse_collection_limit_test!(
        tsm_symmetry_plane_coefficients_refuse_collection_limit,
        "#TS0200\n105sym 0\n105plane 0\n",
        "read T-spline symmetry plane coefficients"
    );
    tsm_parse_collection_limit_test!(
        tsm_editor_selection_refuses_collection_limit,
        "#TS0200\n100edges 1\n",
        "read T-spline editor selections"
    );
    tsm_parse_collection_limit_test!(
        tsm_grip_selection_merge_refuses_collection_limit,
        "#TS0200\n50000grip 1\n",
        "merge T-spline grip selections"
    );
    tsm_parse_collection_limit_test!(
        tsm_crease_edge_refuses_collection_limit,
        "#TS0200\nec 0 0\n",
        "read T-spline crease edges"
    );
    tsm_parse_collection_limit_test!(
        tsm_symmetry_map_values_refuse_collection_limit,
        "#TS0200\n105sym 0\n105a f 0 1\n",
        "read T-spline symmetry map values"
    );
    tsm_parse_collection_limit_test!(
        tsm_symmetry_map_pairs_refuse_collection_limit,
        "#TS0200\n105sym 0\n105a f 0 1\n",
        "index T-spline symmetry map pairs"
    );
    tsm_parse_collection_limit_test!(
        tsm_radial_map_values_refuse_collection_limit,
        "#TS0200\n105sym 1\n105r ef 0 1\n",
        "read T-spline radial map values"
    );
    tsm_parse_collection_limit_test!(
        tsm_radial_map_sources_refuse_collection_limit,
        "#TS0200\n105sym 1\n105r ef 0 1\n",
        "index T-spline radial map sources"
    );
    tsm_parse_collection_limit_test!(
        tsm_radial_map_pairs_refuse_collection_limit,
        "#TS0200\n105sym 1\n105r ef 0 1\n",
        "read T-spline radial map pairs"
    );
    tsm_parse_collection_limit_test!(
        tsm_radial_map_run_refuses_collection_limit,
        "#TS0200\n105sym 1\n105r ef 0 1\n",
        "read T-spline radial maps"
    );
    tsm_parse_collection_limit_test!(
        tsm_symmetry_kind_index_refuses_collection_limit,
        "#TS0200\n105sym 0\n105a f\n",
        "index T-spline symmetry kinds"
    );
    tsm_parse_collection_limit_test!(
        tsm_unknown_kind_index_refuses_collection_limit,
        "#TS0200\nzzz\n",
        "index T-spline unknown record kinds"
    );

    #[test]
    fn tsm_symmetry_kind_text_refuses_retained_limit() {
        let error = parse_small_limit("#TS0200\n105sym 0\n105a f\n", u64::MAX, 0);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "retain T-spline symmetry kind"),
            "{error:?}"
        );
    }

    #[test]
    fn tsm_unknown_kind_text_refuses_retained_limit() {
        let error = parse_small_limit("#TS0200\nzzz\n", u64::MAX, 0);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "retain T-spline unknown record kind"),
            "{error:?}"
        );
    }

    #[test]
    fn tsm_live_face_index_refuses_collection_limit() {
        let error = parse_quad_limit(61);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index T-spline live faces")
        );
    }

    #[test]
    fn tsm_live_edge_index_refuses_collection_limit() {
        let error = parse_quad_limit(62);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index T-spline live edges")
        );
    }

    #[test]
    fn tsm_vertex_slot_compaction_refuses_collection_limit() {
        let error = parse_quad_limit(94);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "compact T-spline slots"),
            "{error:?}"
        );
    }

    #[test]
    fn tsm_edge_slot_compaction_refuses_collection_limit() {
        let error = parse_quad_limit(98);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "compact T-spline slots")
        );
    }

    #[test]
    fn tsm_face_slot_compaction_refuses_collection_limit() {
        let error = parse_quad_limit(102);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "compact T-spline slots")
        );
    }

    #[test]
    fn parses_explicit_grip_map() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0m odd-grip-map\n0m gvp 0\n0m gvp 1\n0m gvp 2\n0m gvp 3\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("quad cage");
        assert_quad(&cage.surface);
    }

    #[test]
    fn parses_positional_grip_map() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("quad cage");
        assert_quad(&cage.surface);
    }

    #[test]
    fn counts_records_without_typed_semantics() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             vendor-extension 1 2 3\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("quad cage");
        assert_eq!(
            cage.unknown_record_kinds,
            std::collections::BTreeMap::from([("vendor-extension".to_string(), 1)])
        );
        assert_quad(&cage.surface);
    }

    #[test]
    fn parses_editor_metadata_and_derived_grip_connectivity() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0m odd-grip-map\n0m gvp 0\n0m gvp 1\n0m gvp 2\n0m gvp 3\n0m gv 0\n\
             0m cg 0 4 1 0 0 0 4\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n0g 0.5 0 0 1\n\
             100edges 0 2\n100verts 1\n50000grip 0\n50000grip 1\n\
             105sym 0\n105plane 0 2 0 1 0 1 0 0 0 0 1 0\n\
             105a fr 0 0\n105a er 0 0 1 2\n105a e 2 1\n\
             105a vr 0 1\n105a v 1 0\n\
             tol 0.00001\nver 6021\nbehavior-version 6.5.0\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("typed metadata");
        assert!(cage.unknown_record_kinds.is_empty());
        assert_quad(&cage.surface);
        let layout = wire::field_or_default::<Option<subd::SubdVertexGripLayout>>(
            &(wire::field::<Vec<subd::SubdVertex>>(&(cage.surface.cage), "vertices")[0]),
            "secondary_grips",
        )
        .expect("secondary grip layout");
        assert_eq!(
            wire::field::<subd::SubdGripDirection>(&(layout), "direction"),
            cadmpeg_ir::SubdGripDirection::North
        );
        assert_eq!(
            wire::field::<Vec<subd::SubdGripWedge>>(&(layout), "wedges").len(),
            4
        );
        let SubdGripWedge::Slot { spokes, .. } =
            &wire::field::<Vec<subd::SubdGripWedge>>(&(layout), "wedges")[0]
        else {
            panic!("first wedge is a fan slot");
        };
        assert_eq!(
            wire::field::<u32>(&(spokes[0].as_ref().unwrap()), "source_index"),
            4
        );
        assert!(
            wire::field::<Vec<subd::SubdGripWedge>>(&(layout), "wedges")[2..]
                .iter()
                .all(|wedge| matches!(wedge, SubdGripWedge::Phantom {}))
        );
        assert!(matches!(
            &wire::field::<Vec<subd::SubdGripWedge>>(&(layout), "wedges")[1],
            SubdGripWedge::Slot {
                sector_face: None,
                ..
            }
        ));

        assert_eq!(
            wire::field_or_default::<Vec<subd::SubdSymmetry>>(&(cage.surface.cage), "symmetries")
                .len(),
            1
        );
        let symmetry =
            &wire::field_or_default::<Vec<subd::SubdSymmetry>>(&(cage.surface.cage), "symmetries")
                [0];
        assert_eq!(
            wire::field::<subd::SubdSymmetryKind>(&(symmetry), "kind"),
            cadmpeg_ir::SubdSymmetryKind::Correspondence {}
        );
        assert_eq!(
            wire::field::<cadmpeg_ir::math::Point3>(&(symmetry.plane), "origin"),
            cadmpeg_ir::math::Point3::new(0.0, 20.0, 0.0)
        );
        assert_eq!(
            wire::field::<cadmpeg_ir::math::Vector3>(&(symmetry.plane), "first_axis"),
            cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
        );
        assert_eq!(
            wire::field::<cadmpeg_ir::math::Vector3>(&(symmetry.plane), "second_axis"),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
        );
        assert_eq!(
            wire::field_or_default::<Vec<[u32; 2]>>(&(symmetry), "face_pairs"),
            vec![[0, 0]]
        );
        assert_eq!(
            wire::field_or_default::<Vec<[u32; 2]>>(&(symmetry), "edge_pairs"),
            vec![[0, 0], [1, 2]]
        );
        assert_eq!(
            wire::field_or_default::<Vec<[u32; 2]>>(&(symmetry), "vertex_pairs"),
            vec![[0, 1]]
        );
    }

    #[test]
    fn rejects_secondary_grips_on_phantom_wedges() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0m odd-grip-map\n0m gvp 0\n0m gvp 1\n0m gvp 2\n0m gvp 3\n\
             0m gv 0\n0m gv 0\n\
             0m cg 0 4 1 0 1 0 4 5\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n\
             0g 0.5 0 0 1\n0g 0.6 0 0 1\n"
        );
        let error = parse_cage(source.as_bytes()).expect_err("phantom spoke");
        assert!(
            error
                .to_string()
                .contains("phantom wedge carries a nonzero spoke length"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn partitions_rectangular_sector_grids_with_product_arity() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0m odd-grip-map\n0m gvp 0\n0m gvp 1\n0m gvp 2\n0m gvp 3\n\
             0m gv 0\n0m gv 0\n0m gv 0\n0m gv 0\n0m gv 0\n\
             0m cg 0 4 2 1 0 0 4 5 6 7 8\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n\
             0g 0.1 0 0 1\n0g 0.2 0 0 1\n0g 0.3 0 0 1\n0g 0.4 0 0 1\n0g 0.5 0 0 1\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("rectangular sector grid");
        let layout = wire::field_or_default::<Option<subd::SubdVertexGripLayout>>(
            &(wire::field::<Vec<subd::SubdVertex>>(&(cage.surface.cage), "vertices")[0]),
            "secondary_grips",
        )
        .expect("secondary grip layout");
        let SubdGripWedge::Slot {
            spokes: first_spokes,
            sectors: first_sectors,
            ..
        } = &wire::field::<Vec<subd::SubdGripWedge>>(&(layout), "wedges")[0]
        else {
            panic!("first wedge is a fan slot");
        };
        let SubdGripWedge::Slot {
            spokes: second_spokes,
            sectors: second_sectors,
            ..
        } = &wire::field::<Vec<subd::SubdGripWedge>>(&(layout), "wedges")[1]
        else {
            panic!("second wedge is a fan slot");
        };
        assert_eq!(first_spokes.len(), 2);
        assert_eq!(first_sectors.len(), 2);
        assert_eq!(second_spokes.len(), 1);
        assert!(second_sectors.is_empty());
        assert_eq!(
            first_spokes
                .iter()
                .chain(first_sectors)
                .chain(second_spokes)
                .map(|grip| wire::field::<u32>(&(grip.as_ref().unwrap()), "source_index"))
                .collect::<Vec<_>>(),
            vec![4, 5, 6, 7, 8]
        );
    }

    #[test]
    fn maps_compass_words_to_north_anchored_offsets() {
        assert_eq!(
            super::direction_offset(cadmpeg_ir::SubdGripDirection::North),
            0
        );
        assert_eq!(
            super::direction_offset(cadmpeg_ir::SubdGripDirection::East),
            1
        );
        assert_eq!(
            super::direction_offset(cadmpeg_ir::SubdGripDirection::South),
            2
        );
        assert_eq!(
            super::direction_offset(cadmpeg_ir::SubdGripDirection::West),
            3
        );
    }

    #[test]
    fn transfers_knot_intervals_and_absolute_crease_sharpness() {
        let source = QUAD_TOPOLOGY
            .replace("e 0 1\n", "e 0 0.5\n")
            .replace("e 2 1\n", "e 2 0.25\n")
            .replace("e 4 1\n", "e 4 0.125\n")
            .replace("e 6 1\n", "e 6 0.0625\n");
        let cage = parse_cage(
            format!(
                "#TS0200\n{source}\
                 0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
            )
            .as_bytes(),
        )
        .expect("knot intervals");
        let expected = [0.5, 0.25, 0.125, 0.0625];
        for (edge, expected) in wire::field::<Vec<subd::SubdEdge>>(&(cage.surface.cage), "edges")
            .iter()
            .zip(expected)
        {
            let actual = wire::field_or_default::<Option<f64>>(&(edge), "knot_interval")
                .expect("knot interval");
            assert!((actual - expected).abs() < EPS_KNOT_INTERVAL);
            assert!(wire::field::<[f64; 2]>(&(edge), "sharpness")
                .iter()
                .all(|sharpness| (*sharpness - 1.0).abs() < EPS_KNOT_INTERVAL));
        }
    }

    #[test]
    fn validates_the_subd_creases_edge_knot_mirror() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}106ek 0.9999999999999\n106ek 1\n106ek 1\n106ek 1\n\
                     0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("edge-knot mirror");
        assert!(cage.unknown_record_kinds.is_empty());
        assert_eq!(
            wire::field::<Vec<subd::SubdEdge>>(&(cage.surface.cage), "edges").len(),
            4
        );
    }

    #[test]
    fn validates_deleted_slots_in_the_subd_creases_edge_knot_mirror() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}e\n106ek 1\n106ek 1\n106ek 1\n106ek 1\n106ek -1\n\
                     0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("deleted edge-knot slot");
        assert!(cage.unknown_record_kinds.is_empty());
        assert_eq!(
            wire::field::<Vec<subd::SubdEdge>>(&(cage.surface.cage), "edges").len(),
            4
        );
    }

    #[test]
    fn retains_the_unresolved_edge_knot_state_form() {
        let source = format!(
            "#TS0200\n{}106ek 0\n106ek 1\n106ek 0\n106ek 1\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n",
            QUAD_TOPOLOGY.replace(
                "end-conditions SUBD_CREASES",
                "end-conditions MULTIPLE_KNOTS"
            )
        );
        let cage = parse_cage(source.as_bytes()).expect("unresolved edge-knot state");
        assert_eq!(
            cage.unknown_record_kinds,
            std::collections::BTreeMap::from([("106ek".to_string(), 4)])
        );
    }

    #[test]
    fn parses_radial_symmetry_metadata() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n\
             105sym 1\n105plane 0 0 0 1 0 1 0 0 0 0 1 0\n\
             105r segments 4\n105r sweep 1\n\
             105r ef 0 1\n105r er 1 0\n105r ff 0 0\n105r fr 0 0\n\
             105r vf 0 1\n105r vr 1 0\n\
             tol 0.00001\nver 6021\nbehavior-version 6.5.0\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("radial symmetry metadata");
        assert!(cage.unknown_record_kinds.is_empty());
        assert_quad(&cage.surface);
        assert_eq!(
            wire::field_or_default::<Vec<subd::SubdSymmetry>>(&(cage.surface.cage), "symmetries")
                .len(),
            1
        );
        let symmetry =
            &wire::field_or_default::<Vec<subd::SubdSymmetry>>(&(cage.surface.cage), "symmetries")
                [0];
        let cadmpeg_ir::SubdSymmetryKind::Radial(radial) =
            wire::field::<subd::SubdSymmetryKind>(&(symmetry), "kind")
        else {
            panic!("radial symmetry kind");
        };
        let radial_maps =
            wire::field_or_default::<Vec<subd::SubdRadialSymmetryMap>>(&(radial), "radial_maps");
        assert_eq!(
            (
                wire::field::<std::num::NonZeroU32>(&(radial), "segments").get(),
                wire::field::<f64>(&(radial), "sweep")
            ),
            (4, 1.0)
        );
        assert_eq!(
            wire::field::<cadmpeg_ir::math::Point3>(&(symmetry.plane), "origin"),
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)
        );
        assert_eq!(
            wire::field::<cadmpeg_ir::math::Vector3>(&(symmetry.plane), "first_axis"),
            cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
        );
        assert_eq!(
            wire::field::<cadmpeg_ir::math::Vector3>(&(symmetry.plane), "second_axis"),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
        );
        assert!(wire::field_or_default::<Vec<[u32; 2]>>(&(symmetry), "face_pairs").is_empty());
        assert!(wire::field_or_default::<Vec<[u32; 2]>>(&(symmetry), "edge_pairs").is_empty());
        assert!(wire::field_or_default::<Vec<[u32; 2]>>(&(symmetry), "vertex_pairs").is_empty());
        assert_eq!(
            *radial_maps,
            vec![
                cadmpeg_ir::SubdRadialSymmetryMap {
                    selector: cadmpeg_ir::SubdRadialMapSelector::Ef,
                    pairs: vec![[0, 1]],
                },
                cadmpeg_ir::SubdRadialSymmetryMap {
                    selector: cadmpeg_ir::SubdRadialMapSelector::Er,
                    pairs: vec![[1, 0]],
                },
                cadmpeg_ir::SubdRadialSymmetryMap {
                    selector: cadmpeg_ir::SubdRadialMapSelector::Ff,
                    pairs: vec![[0, 0]],
                },
                cadmpeg_ir::SubdRadialSymmetryMap {
                    selector: cadmpeg_ir::SubdRadialMapSelector::Fr,
                    pairs: vec![[0, 0]],
                },
                cadmpeg_ir::SubdRadialSymmetryMap {
                    selector: cadmpeg_ir::SubdRadialMapSelector::Vf,
                    pairs: vec![[0, 1]],
                },
                cadmpeg_ir::SubdRadialSymmetryMap {
                    selector: cadmpeg_ir::SubdRadialMapSelector::Vr,
                    pairs: vec![[1, 0]],
                },
            ]
        );

        let unsupported = source.replace("105sym 1", "105sym 2");
        let error = parse_cage(unsupported.as_bytes()).expect_err("unsupported symmetry mode");
        assert!(
            error.to_string().contains("unsupported symmetry flags"),
            "unexpected error: {error}"
        );

        let missing = source.replace("105r vf 0 1\n", "");
        let error = parse_cage(missing.as_bytes()).expect_err("incomplete radial maps");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));

        let native_id = u64::MAX;
        let replacement = format!("105r ef {native_id} {native_id}\n");
        let native = source.replace("105r ef 0 1\n", &replacement);
        let cage = parse_cage(native.as_bytes()).expect("opaque radial native id");
        let cadmpeg_ir::SubdSymmetryKind::Radial(radial) = wire::field::<subd::SubdSymmetryKind>(
            &(wire::field_or_default::<Vec<subd::SubdSymmetry>>(
                &(cage.surface.cage),
                "symmetries",
            )[0]),
            "kind",
        ) else {
            panic!("radial symmetry kind");
        };
        let ef =
            wire::field_or_default::<Vec<subd::SubdRadialSymmetryMap>>(&(radial), "radial_maps")
                .into_iter()
                .find(|map| map.selector == cadmpeg_ir::SubdRadialMapSelector::Ef)
                .expect("ef radial map");
        assert_eq!(ef.pairs, vec![[native_id, native_id]]);
    }

    #[test]
    fn rejects_nonorthonormal_symmetry_plane() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n\
             105sym 0\n105plane 0 0 0 1 1 0 0 0 1 0 0 0\n\
             tol 0.00001\nver 6021\nbehavior-version 6.5.0\n"
        );
        let error = parse_cage(source.as_bytes()).expect_err("nonorthonormal symmetry plane");
        assert!(
            error
                .to_string()
                .contains("symmetry plane is not a homogeneous orthonormal frame"),
            "unexpected error: {error}"
        );
    }

    /// A bare topology token is a deleted slot: it consumes an index and
    /// defines no element. Appending one of each leaves the cage unchanged.
    #[test]
    fn deleted_slots_consume_an_index_without_defining_an_element() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}f\ne\nv\nl\n\
             0m odd-grip-map\n0m gvp 0\n0m gvp 1\n0m gvp 2\n0m gvp 3\n0m gv -1\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n0g\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("quad cage");
        assert_quad(&cage.surface);
    }

    /// Deleted slots renumber the IR: a leading deleted vertex and edge slot
    /// shift every program index by one without changing the emitted cage.
    #[test]
    fn deleted_slots_renumber_the_cage() {
        let shifted = QUAD_TOPOLOGY
            .replace("e 0 1\n", "e\ne 0 1\n")
            .replace("v 0 NORTH\n", "v\nv 0 NORTH\n")
            .replace(
                "ec 0 0\nec 1 0\nec 2 0\nec 3 0\n",
                "ec 1 0\nec 2 0\nec 3 0\nec 4 0\n",
            );
        let shifted = shifted.replace("l 2 6 1 0 0 0 0", "l 2 6 1 1 0 0 0");
        let shifted = shifted.replace("l 7 3 0 3 -1 0 0", "l 7 3 0 4 -1 0 0");
        let shifted = shifted.replace("l 4 0 3 1 0 0 0", "l 4 0 3 2 0 0 0");
        let shifted = shifted.replace("l 1 5 2 0 -1 0 0", "l 1 5 2 1 -1 0 0");
        let shifted = shifted.replace("l 6 2 5 2 0 0 0", "l 6 2 5 3 0 0 0");
        let shifted = shifted.replace("l 3 7 4 1 -1 0 0", "l 3 7 4 2 -1 0 0");
        let shifted = shifted.replace("l 0 4 7 3 0 0 0", "l 0 4 7 4 0 0 0");
        let shifted = shifted.replace("l 5 1 6 2 -1 0 0", "l 5 1 6 3 -1 0 0");
        let source = format!(
            "#TS0200\n{shifted}\
             0m odd-grip-map\n0m gv -1\n0m gvp 1\n0m gvp 2\n0m gvp 3\n0m gvp 4\n\
             0g\n0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("shifted quad cage");
        assert_quad(&cage.surface);
    }

    /// A populated half-edge may not name a deleted slot.
    #[test]
    fn a_half_edge_naming_a_deleted_slot_is_rejected() {
        let source = format!(
            "#TS0200\n{}\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n",
            QUAD_TOPOLOGY.replace("l 5 1 6 2 -1 0 0", "l")
        );
        let error = parse_cage(source.as_bytes()).expect_err("deleted mate");
        assert!(
            error.to_string().contains("names a deleted slot"),
            "unexpected error: {error}"
        );
    }

    fn assert_quad(cage: &subd::SubdSurface) {
        assert_eq!(
            wire::field::<Vec<subd::SubdVertex>>(&(cage.cage), "vertices").len(),
            4
        );
        assert_eq!(
            wire::field::<Vec<subd::SubdEdge>>(&(cage.cage), "edges").len(),
            4
        );
        assert_eq!(
            wire::field::<Vec<subd::SubdFace>>(&(cage.cage), "faces").len(),
            1
        );
        assert_eq!(
            wire::field::<Vec<subd::SubdVertex>>(&(cage.cage), "vertices")[1]
                .point()
                .x,
            10.0
        );
        assert!(wire::field::<Vec<subd::SubdEdgeUse>>(
            &(wire::field::<Vec<subd::SubdFace>>(&(cage.cage), "faces")[0]),
            "edges"
        )
        .iter()
        .all(|use_| !use_.reversed));
    }
}
