// SPDX-License-Identifier: Apache-2.0
//! Build IR and diagnostics from an NX native container.
//!
//! [`scan`] parses the container and inflates its embedded streams. [`decode`]
//! converts supported analytic and NURBS carriers to millimetres, resolves
//! supported topology, preserves each Parasolid stream as an unknown record, and
//! returns a decode body describing incomplete transfer; the sealed wrapper stamps
//! the classification onto the report. Partition and deltas streams are both
//! decoded; callers must use the report to account for unresolved active-face
//! selection and other loss.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::StreamHandle;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::container::{self, Container, EntryContent};
use crate::loss::NxLossCode;
use crate::native::TypedNative;
use crate::parasolid::{self, Stream, StreamKind};

mod blend;
mod build;
pub(crate) mod emit;
pub(crate) mod feature_completeness;
mod geometry_work;
pub(crate) mod ids;
pub(crate) mod jpeg;
mod offset;
pub(crate) mod pcurves;
pub(crate) mod report;
mod support_uv;

use build::try_decode_geometry;
use emit::{source_meta, unknown_stream};

const MISSING_TOLERANCE: f64 = -31_415_800_000_000.0;
/// Parsed container data shared by inspection and entity decoding.
pub(crate) struct Scan<'a> {
    /// Parsed SPLMSSTR container.
    pub(crate) container: Container<'a>,
    /// Located and inflated Parasolid or preview streams.
    pub(crate) streams: Vec<Stream>,
}

impl Scan<'_> {
    /// Count streams with the requested classification.
    pub(super) fn count(&self, kind: StreamKind) -> usize {
        self.streams.iter().filter(|s| s.kind() == kind).count()
    }

    /// Return whether the file contains an inline Parasolid stream.
    ///
    /// NX assemblies may contain only references to external child parts.
    pub(super) fn has_parasolid(&self) -> bool {
        self.streams.iter().any(|s| s.kind().is_parasolid())
    }
}

/// Parse the SPLMSSTR container and inflate streams in its canonical part entry.
pub(crate) fn scan<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Scan<'a>, CodecError> {
    let (container, streams) = if container::looks_like_nx(root.window()) {
        let container = container::scan_bytes(ctx, root.window())?;
        let streams = parasolid::extract_streams(ctx, root, &container)?;
        (container, streams)
    } else {
        let (container, part) = container::scan_legacy(ctx, root)?;
        let streams = parasolid::extract_legacy_streams(ctx, part)?;
        (container, streams)
    };
    Ok(Scan { container, streams })
}

/// Decode an NX `.prt` into IR and a loss report.
///
/// When [`DecodeContext::container_only`] is set, the returned IR contains source
/// metadata and preserved streams but no typed entities. Otherwise the decoder
/// emits supported geometry and resolvable topology. A valid container can
/// decode successfully with no geometry, including an assembly whose geometry
/// resides in external child parts.
pub(crate) fn decode<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Decoded, CodecError> {
    let scan = scan(ctx, root)?;
    let (classification, notes) = crate::scan_notes::summarize(ctx, &scan)?;
    let (dialects, dialect_losses) = classification.into_report_parts();

    let mut admitted_entities = 0_u64;
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(scan.streams.len()),
        "admit NX streams",
    )?;
    if ctx.container_only() {
        let (ir, annotations, unknowns, native_losses) =
            build_metadata_ir(ctx, root, &scan, &dialects)?;
        let mut body = build_container_body(ctx, &scan, dialect_losses, notes)?;
        ctx.extend_vec(&mut body.losses, native_losses, "nx decode losses")?;
        report_untransferred_streams(ctx, &scan, &mut body, TypedNative::ContainerOnly)?;
        return decoded(ctx, ir, body, annotations, unknowns, &mut admitted_entities);
    }

    if let Some((ir, body, annotations, unknowns)) = try_decode_geometry(
        ctx,
        root,
        &scan,
        &dialects,
        &dialect_losses,
        &notes,
        &mut admitted_entities,
    )? {
        return decoded(ctx, ir, body, annotations, unknowns, &mut admitted_entities);
    }

    let (ir, annotations, unknowns, native_losses) =
        build_metadata_ir(ctx, root, &scan, &dialects)?;
    let mut body = build_container_body(ctx, &scan, dialect_losses, notes)?;
    ctx.extend_vec(&mut body.losses, native_losses, "nx decode losses")?;
    report_untransferred_streams(ctx, &scan, &mut body, TypedNative::Available)?;
    decoded(ctx, ir, body, annotations, unknowns, &mut admitted_entities)
}

fn decoded(
    ctx: &DecodeContext<'_>,
    mut ir: CadIr,
    body: DecodeBody,
    annotations: cadmpeg_ir::Annotations,
    unknowns: Vec<UnknownRecord>,
    admitted_entities: &mut u64,
) -> Result<Decoded, CodecError> {
    ctx.admit_entities(
        cadmpeg_core::decode::u64_from_index(ir.model.entity_count()),
        admitted_entities,
        "admit NX entities",
    )?;
    let mut source_fidelity = cadmpeg_ir::SourceFidelity::with_annotations(annotations);
    source_fidelity.attach_native_unknown_records(&mut ir, "nx", unknowns, ctx)?;
    Ok(Decoded {
        ir,
        body,
        source_fidelity,
    })
}

fn report_untransferred_streams(
    ctx: &DecodeContext<'_>,
    scan: &Scan,
    body: &mut DecodeBody,
    typed_native: TypedNative,
) -> Result<(), CodecError> {
    let (control_count, classified_control_count) =
        offset_store_control_counts(ctx, &scan.container)?;
    if classified_control_count != control_count {
        charge_loss_code(ctx, NxLossCode::OffsetStoreControlUntyped)?;
        ctx.push_vec(&mut body.losses, NxLossCode::OffsetStoreControlUntyped.note(ctx.format_retained(format_args!(
                "{} of {control_count} bounded offset-store control block(s) have no admitted complete grammar.",
                control_count - classified_control_count
            ), "nx offset control loss text")?), "nx decode losses")?;
    }
    for entry in &scan.container.entries {
        let content = entry.content();
        if content.retains_opaque_payload()
            && !(typed_native == TypedNative::Available
                && content == EntryContent::SaveToggleInfo
                && crate::native::toggle::has_complete_saved_toggle_stream(&scan.container))
        {
            charge_loss_code(ctx, NxLossCode::ContainerStreamOpaque)?;
            ctx.push_vec(&mut body.losses, NxLossCode::ContainerStreamOpaque.note(ctx.format_retained(format_args!(
                    "Named container stream {} is classified as {} and retained byte-exact; its field semantics are not completely typed.",
                    entry.name,
                    content.label()
                ), "nx opaque stream loss text")?), "nx decode losses")?;
        }
    }
    for (index, stream) in scan.streams.iter().enumerate() {
        if !stream.kind().is_parasolid() {
            charge_loss_code(ctx, NxLossCode::NonParasolidStreamOmitted)?;
            ctx.push_vec(
                &mut body.losses,
                NxLossCode::NonParasolidStreamOmitted.note(ctx.format_retained(
                    format_args!(
                        "Non-Parasolid {} stream #{index} was classified but not transferred.",
                        stream.kind().label()
                    ),
                    "nx omitted stream loss text",
                )?),
                "nx decode losses",
            )?;
        }
    }
    Ok(())
}

fn charge_loss_code(ctx: &DecodeContext<'_>, code: NxLossCode) -> Result<(), CodecError> {
    let bytes = "nx"
        .len()
        .checked_add(code.code().len())
        .ok_or_else(|| ctx.refuse_codec_limit("nx loss code text", 0, u64::MAX))?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(bytes),
        "nx loss code text",
    )
}

pub(super) fn offset_store_control_counts(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<(usize, usize), CodecError> {
    let mut total = 0;
    let mut classified = 0;
    for (_, section) in container.indexed_om_sections(ctx)? {
        let Some((control, _, records)) = section.as_offset_only() else {
            continue;
        };
        total += 1;
        if crate::om::offset_store_control_form(
            ctx,
            control.bytes,
            records.first().map(|record| record.bytes),
        )?
        .is_some()
        {
            classified += 1;
        }
    }
    Ok((total, classified))
}

/// Aggregate carrier counts across the decoded streams, for reporting.
#[derive(Debug, Default)]
struct Counts {
    points: usize,
    planes: usize,
    cylinders: usize,
    cones: usize,
    spheres: usize,
    tori: usize,
    nurbs_surfaces: usize,
    offset_surfaces: usize,
    blend_surfaces: usize,
    lines: usize,
    circles: usize,
    ellipses: usize,
    nurbs_curves: usize,
    intersection_curves: usize,
    intersection_rejections: crate::intersection::RejectionCounts,
}

impl Counts {
    fn surfaces(&self) -> usize {
        self.planes
            + self.cylinders
            + self.cones
            + self.spheres
            + self.tori
            + self.nurbs_surfaces
            + self.offset_surfaces
            + self.blend_surfaces
    }
    fn curves(&self) -> usize {
        self.lines + self.circles + self.ellipses + self.nurbs_curves + self.intersection_curves
    }
}

fn build_metadata_ir(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    scan: &Scan,
    dialects: &DialectLayers,
) -> Result<
    (
        CadIr,
        cadmpeg_ir::Annotations,
        Vec<UnknownRecord>,
        Vec<LossNote>,
    ),
    CodecError,
> {
    let unknown_count = scan
        .streams
        .iter()
        .filter(|stream| stream.kind().is_parasolid())
        .count();
    let mut unknowns = ctx.collection_vec(unknown_count, "nx metadata unknown streams")?;
    let mut ir = CadIr::decoded(source_meta(ctx, scan, dialects)?);
    let mut annotations = AnnotationBuilder::new();
    let mut losses = Vec::new();
    let source_stream = StreamHandle::new_for_decode(ctx, cadmpeg_ir::stream_name!("nx:container"), "allocate annotation stream handle")?;
    for (si, stream) in scan.streams.iter().enumerate() {
        if stream.kind().is_parasolid() {
            let unknown = unknown_stream(ctx, si, stream)?;
            annotations.note_for_decode(ctx, unknown.id().as_str(), &source_stream, cadmpeg_core::decode::u64_from_index(stream.file_offset), Some(stream.kind().label()))?;
            annotations.exactness_for_decode(ctx, unknown.id().as_str(), Exactness::Derived)?;
            unknowns.push(unknown);
        }
    }
    if ctx.container_only() {
        crate::native::attach_container_layer(
            ctx,
            &mut ir,
            scan,
            &mut annotations,
            &mut unknowns,
            TypedNative::ContainerOnly,
        )?;
    } else {
        let mut parsed = crate::native::substrate::ParsedStreams::parse(ctx, scan)?;
        let model = crate::native::model::NativeModel::extract(
            ctx,
            root,
            &scan.container,
            &scan.streams,
            &mut parsed,
            None,
        )?;
        crate::native::attach_annotations(
            ctx,
            &mut ir,
            &model,
            scan,
            &mut annotations,
            &mut unknowns,
            &mut losses,
        )?;
    }
    Ok((ir, annotations.build(), unknowns, losses))
}

fn build_container_body(
    ctx: &DecodeContext<'_>,
    scan: &Scan,
    dialect_losses: Vec<LossNote>,
    notes: Vec<String>,
) -> Result<DecodeBody, CodecError> {
    let assembly = scan
        .container
        .entries
        .iter()
        .any(|e| e.name.contains("ExternalReferences"))
        && !scan.has_parasolid();

    let loss = if assembly {
        charge_loss_code(ctx, NxLossCode::AssemblyComponentsExternal)?;
        NxLossCode::AssemblyComponentsExternal.note(
            "No inline Parasolid geometry: this is an assembly .prt. Component geometry \
                      lives in external child .prt files named in EXTREFSTREAM, and the assembled \
                      solid's inputs (child partitions + constraint solve) are absent from this \
                      file. This is an external-dependency boundary, not a decode gap.",
        )
    } else {
        charge_loss_code(ctx, NxLossCode::GeometryNotTransferred)?;
        NxLossCode::GeometryNotTransferred.note(
            "No B-rep geometry was transferred: no gate-passing analytic carrier was found \
                      in the embedded Parasolid streams (they may hold only B-spline/procedural \
                      geometry this codec does not yet type). The streams are preserved verbatim as \
                      unknown passthrough records.",
        )
    };

    let mut body = DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(false),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: Vec::new(),
        notes,
        transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
    };
    ctx.push_vec(&mut body.losses, loss, "nx decode losses")?;
    ctx.extend_vec(&mut body.losses, dialect_losses, "nx decode losses")?;
    Ok(body)
}

#[cfg(test)]
mod tests;
