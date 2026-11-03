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
use cadmpeg_ir::source_fidelity::parts::PartOutput;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartSpec};
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::container::{self, Container, EntryContent};
use crate::loss::NxLossCode;
use crate::native::TypedNative;
use crate::parasolid::{self, Stream, StreamKind};

mod blend;
mod build;
mod document_projection;
pub(crate) mod emit;
pub(crate) mod feature_completeness;
mod geometry_work;
pub(crate) mod ids;
pub(crate) mod jpeg;
mod nurbs_fit;
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
    pub(crate) streams: Vec<Stream<'a>>,
    untyped_marks: std::cell::RefCell<Vec<(String, cadmpeg_ir::ArenaClasses)>>,
}

impl<'a> Scan<'a> {
    /// Build a scan over a parsed container and its located streams.
    pub(crate) fn new(container: Container<'a>, streams: Vec<Stream<'a>>) -> Self {
        Self {
            container,
            streams,
            untyped_marks: std::cell::RefCell::default(),
        }
    }
}

impl Scan<'_> {
    /// Mark one retained record untyped at its retention site (§1.3).
    pub(crate) fn note_untyped(
        &self,
        id: &str,
        classes: impl IntoIterator<Item = cadmpeg_ir::ArenaClass>,
    ) {
        let classes: cadmpeg_ir::ArenaClasses = classes.into_iter().collect();
        if classes.is_empty() {
            return;
        }
        let mut marks = self.untyped_marks.borrow_mut();
        if let Some((_, held)) = marks.iter_mut().find(|(marked, _)| marked == id) {
            held.extend(classes);
        } else {
            marks.push((id.to_owned(), classes));
        }
    }

    pub(crate) fn untyped_mark_count(&self) -> usize {
        self.untyped_marks.borrow().len()
    }

    pub(crate) fn truncate_untyped_marks(&self, len: usize) {
        self.untyped_marks.borrow_mut().truncate(len);
    }

    pub(crate) fn take_untyped_marks(&self) -> Vec<(String, cadmpeg_ir::ArenaClasses)> {
        std::mem::take(&mut self.untyped_marks.borrow_mut())
    }

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
        let container = container::scan_bytes(root.window())?;
        let streams = parasolid::extract_streams(ctx, root, &container)?;
        (container, streams)
    } else {
        let (container, part) = container::scan_legacy(ctx, root)?;
        let streams = parasolid::extract_legacy_streams(ctx, part)?;
        (container, streams)
    };
    Ok(Scan::new(container, streams))
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
    let (classification, notes) = crate::scan_notes::summarize(&scan)?;
    let (dialects, dialect_losses) = classification.into_report_parts();

    let mut admitted_entities = 0_u64;
    if ctx.container_only() {
        let (ir, annotations, unknowns, native_losses, outcome) =
            build_metadata_ir(ctx, &scan, &dialects)?;
        let mut body = build_container_body(&scan, dialect_losses, notes);
        body.transfer = cadmpeg_ir::report::decode::DecodeTransfer {
            scope: cadmpeg_ir::ContentScope::container(),
            geometry_transferred: false,
        };
        body.losses.extend(native_losses);
        report_untransferred_streams(&scan, &mut body, TypedNative::ContainerOnly);
        return decoded(
            ctx,
            &scan,
            ir,
            body,
            (annotations, unknowns),
            &mut admitted_entities,
            outcome,
        );
    }

    if let Some((ir, body, annotations, unknowns, outcome)) = try_decode_geometry(
        ctx,
        &scan,
        &dialects,
        &dialect_losses,
        &notes,
        &mut admitted_entities,
    )? {
        return decoded(
            ctx,
            &scan,
            ir,
            body,
            (annotations, unknowns),
            &mut admitted_entities,
            outcome,
        );
    }

    let (mut ir, annotations, unknowns, native_losses, outcome) =
        build_metadata_ir(ctx, &scan, &dialects)?;
    // Absence of a B-rep route does not erase independently decoded document
    // content (JT meshes, attributes, products, or design records).
    ir.scope = cadmpeg_ir::ContentScope {
        model: model_parts_located(&scan, &ir),
        design: outcome.design_held && design_parts_located(&scan, &ir),
    };
    let mut body = build_container_body(&scan, dialect_losses, notes);
    body.losses.extend(native_losses);
    report_untransferred_streams(&scan, &mut body, TypedNative::Available);
    decoded(
        ctx,
        &scan,
        ir,
        body,
        (annotations, unknowns),
        &mut admitted_entities,
        outcome,
    )
}

/// How one decode route left the design class, with its given-up parts.
pub(crate) struct RouteOutcome {
    /// False only when the class-level attachment failure gave up design.
    pub(crate) design_held: bool,
    /// Staged part give-ups: retained bytes, stub records, and losses.
    pub(crate) parts: PartOutput,
}

/// Native arenas whose records carry design content: feature history,
/// expressions, and configurations.
fn is_design_native_arena(name: &str) -> bool {
    name.starts_with("feature_")
        || name.starts_with("expression")
        || name.starts_with("configuration")
}

/// Whether one container entry can hold model content.
fn entry_holds_model(content: crate::container::EntryContent) -> bool {
    use crate::container::EntryContent::*;
    matches!(
        content,
        ActiveBodyIndex
            | FastLoadStructure
            | FastLoadJt
            | DisplayJt
            | ExternalReferences
            | MaterialTexture
            | PartAttributes
            | AssetCatalog
            | NamedOpaqueStream
    )
}

/// Whether one container entry can hold design content.
fn entry_holds_design(content: crate::container::EntryContent) -> bool {
    use crate::container::EntryContent::*;
    matches!(content, PartPayload | Arrangements | NamedOpaqueStream)
}

/// Located model parts (§1.1): kernel streams, display and fast-load
/// structure, assembly references, and any other retained native record.
fn model_parts_located(scan: &Scan<'_>, ir: &CadIr) -> bool {
    scan.has_parasolid()
        || ir
            .model
            .populated_arena(cadmpeg_ir::ArenaClass::Model)
            .is_some()
        || scan
            .container
            .entries
            .iter()
            .any(|entry| entry_holds_model(entry.content()))
        || ir.native.namespace("nx").is_some_and(|namespace| {
            namespace
                .arenas()
                .iter()
                .any(|(arena, records)| !records.is_empty() && !is_design_native_arena(arena))
        })
}

/// Located design parts (§1.1): object-model sections and the arrangement
/// entries, or any retained design record.
fn design_parts_located(scan: &Scan<'_>, ir: &CadIr) -> bool {
    !scan.container.indexed_om_sections().is_empty()
        || ir
            .model
            .populated_arena(cadmpeg_ir::ArenaClass::Design)
            .is_some()
        || scan
            .container
            .entries
            .iter()
            .any(|entry| entry_holds_design(entry.content()))
        || ir.native.namespace("nx").is_some_and(|namespace| {
            namespace
                .arenas()
                .iter()
                .any(|(arena, records)| !records.is_empty() && is_design_native_arena(arena))
        })
}

fn decoded(
    ctx: &DecodeContext<'_>,
    scan: &Scan<'_>,
    mut ir: CadIr,
    mut body: DecodeBody,
    source: (cadmpeg_ir::Annotations, Vec<UnknownRecord>),
    admitted_entities: &mut u64,
    outcome: RouteOutcome,
) -> Result<Decoded, CodecError> {
    let (annotations, unknowns) = source;
    let RouteOutcome { design_held, parts } = outcome;
    let (_staged_model, staged_native, staged_fidelity, staged_losses) = parts.into_parts();
    // Lanes write the live document; the helper's stages stay empty. Only
    // give-ups (retained bytes, stub records, losses) arrive here.
    body.losses.extend(staged_losses);
    for (format, mut namespace) in staged_native.0 {
        debug_assert_eq!(format, "nx", "NX parts stage only the NX namespace");
        let destination = ir.native.namespace_mut("nx");
        for (arena, mut records) in std::mem::take(namespace.arenas_mut()) {
            destination
                .arenas_mut()
                .entry(arena)
                .or_default()
                .append(&mut records);
        }
    }
    if ctx.container_only() {
        ir.scope = cadmpeg_ir::ContentScope::container();
    } else {
        document_projection::project(ctx, &mut ir, &mut body.losses)?;
        ir.scope.model = body.transfer.geometry_transferred || model_parts_located(scan, &ir);
        ir.scope.design = design_held && design_parts_located(scan, &ir);
    }
    body.transfer.scope = ir.scope;
    ctx.admit_entities(
        cadmpeg_core::decode::u64_from_index(ir.model.entity_count()),
        admitted_entities,
        "admit NX entities",
    )?;
    let mut source_fidelity = cadmpeg_ir::SourceFidelity::for_decode(ctx, annotations);
    source_fidelity.append(staged_fidelity)?;
    source_fidelity.attach_native_unknown_records(&mut ir, "nx", unknowns, ctx)?;
    for (id, classes) in scan.take_untyped_marks() {
        source_fidelity.mark_untyped(&id, classes)?;
    }
    Ok(Decoded {
        ir,
        body,
        source_fidelity,
    })
}

fn report_untransferred_streams(scan: &Scan, body: &mut DecodeBody, typed_native: TypedNative) {
    let (control_count, classified_control_count) = offset_store_control_counts(&scan.container);
    if classified_control_count != control_count {
        body.losses.push(NxLossCode::OffsetStoreControlUntyped.note(
            format!(
                "{} of {control_count} bounded offset-store control block(s) have no admitted complete grammar.",
                control_count - classified_control_count
            ),
        ));
    }
    for entry in &scan.container.entries {
        let content = entry.content();
        if content.retains_opaque_payload()
            && !(typed_native == TypedNative::Available
                && content == EntryContent::SaveToggleInfo
                && crate::native::toggle::has_complete_saved_toggle_stream(&scan.container))
        {
            body.losses.push(NxLossCode::ContainerStreamOpaque.note(
                format!(
                    "Named container stream {} is classified as {} and retained byte-exact; its field semantics are not completely typed.",
                    entry.name,
                    content.label()
                ),
            ));
        }
    }
    for entry in scan.container.bounded_indexed_om_entries() {
        body.losses.push(NxLossCode::SourceScanBounded.note(format!(
            "The indexed object-model section search in {} reached its work bound of {}; \
             its remaining sections were skipped.",
            entry.name,
            crate::om::MAX_INDEXED_SECTION_REGISTRY_WORK,
        )));
    }
    for (index, stream) in scan.streams.iter().enumerate() {
        if !stream.kind().is_parasolid() {
            body.losses
                .push(NxLossCode::NonParasolidStreamOmitted.note(format!(
                    "Non-Parasolid {} stream #{index} was classified but not transferred.",
                    stream.kind().label()
                )));
        }
    }
}

pub(super) fn offset_store_control_counts(container: &Container) -> (usize, usize) {
    let mut total = 0;
    let mut classified = 0;
    for (_, section) in container.indexed_om_sections() {
        let Some((control, _, records)) = section.as_offset_only() else {
            continue;
        };
        total += 1;
        if crate::om::offset_store_control_form(
            control.bytes,
            records.first().map(|record| record.bytes),
        )
        .is_some()
        {
            classified += 1;
        }
    }
    (total, classified)
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
    scan: &Scan,
    dialects: &DialectLayers,
) -> Result<
    (
        CadIr,
        cadmpeg_ir::Annotations,
        Vec<UnknownRecord>,
        Vec<LossNote>,
        RouteOutcome,
    ),
    CodecError,
> {
    let mut unknowns = Vec::new();
    let mut ir = CadIr::decoded(source_meta(scan, dialects, None)?);
    let mut annotations = AnnotationBuilder::new();
    let mut losses = Vec::new();
    let source_stream = StreamHandle::new(cadmpeg_ir::stream_name!("nx:container"));
    for (si, stream) in scan.streams.iter().enumerate() {
        let unknown = unknown_stream(ctx, si, stream)?;
        annotations.note(
            unknown.id().as_str(),
            &source_stream,
            cadmpeg_core::decode::u64_from_index(stream.file_offset),
            Some(stream.kind().label()),
        );
        annotations.exactness(unknown.id().as_str(), Exactness::Derived);
        scan.note_untyped(unknown.id().as_str(), [cadmpeg_ir::ArenaClass::Model]);
        unknowns.push(unknown);
    }
    let mut parts = PartOutput::new(cadmpeg_ir::CodecFormat::Nx, ctx);
    let mut outcome = if ctx.container_only() {
        crate::native::attach_container_layer(
            ctx,
            &mut ir,
            scan,
            &mut annotations,
            &mut unknowns,
            TypedNative::ContainerOnly,
        )?;
        RouteOutcome {
            design_held: true,
            parts,
        }
    } else {
        let held = match (|| {
            let mut parsed = crate::native::substrate::ParsedStreams::parse(scan)?;
            let model = crate::native::model::NativeModel::extract(
                ctx,
                &scan.container,
                &scan.streams,
                &mut parsed,
                None,
            )?;
            let held = crate::native::attach_annotations(
                ctx,
                &mut ir,
                &model,
                scan,
                &mut annotations,
                &mut unknowns,
                &mut losses,
                &mut parts,
            )?;
            crate::native::retain_deltas_gaps(ctx, scan, &model, &mut unknowns)?;
            Ok(held)
        })() {
            Ok(held) => held,
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                ir.scope.design = false;
                losses.push(NxLossCode::NativeAttachmentFailed.note(format!(
                    "Native object-model extraction failed: {error}. Decoded streams are kept. \
                     No attachment record was located, so the design class is not delivered."
                )));
                false
            }
        };
        RouteOutcome {
            design_held: held,
            parts,
        }
    };
    give_up_cut_tails(ctx, scan, &mut outcome.parts)?;
    Ok((ir, annotations.build(), unknowns, losses, outcome))
}

/// Give up every container entry tail the cut source cannot supply: the
/// entry's end is missing, so the rest of its stream is one part (§4.5).
fn give_up_cut_tails(
    ctx: &DecodeContext<'_>,
    scan: &Scan<'_>,
    parts: &mut PartOutput,
) -> Result<(), CodecError> {
    for (start, end) in scan.container.cut_tails() {
        let (Ok(start), Ok(end)) = (usize::try_from(start), usize::try_from(end)) else {
            continue;
        };
        let Some(extent) = scan.container.data.view().child(start, end) else {
            continue;
        };
        let location = extent.location_at(start);
        ctx.decode_part(parts, extent, PartSpec::new("nx.cut_tail"), |_| {
            Err::<(), CodecError>(CodecError::truncated(location, "NX cut container tail"))
        })?;
    }
    Ok(())
}

fn build_container_body(
    scan: &Scan,
    dialect_losses: Vec<LossNote>,
    notes: Vec<String>,
) -> cadmpeg_ir::codec::DecodeBody {
    let assembly = scan.container.has_external_references() && !scan.has_parasolid();

    let loss = if assembly {
        NxLossCode::AssemblyComponentsExternal.note({
            "No inline Parasolid geometry: this is an assembly .prt. Component geometry \
                      lives in external child .prt files named in EXTREFSTREAM, and the assembled \
                      solid's inputs (child partitions + constraint solve) are absent from this \
                      file. This is an external-dependency boundary, not a decode gap."
        })
    } else {
        NxLossCode::GeometryNotTransferred.note(
            { "No B-rep geometry was transferred: no gate-passing analytic carrier was found \
                      in the embedded Parasolid streams (they may hold only B-spline/procedural \
                      geometry this codec does not yet type). The streams are preserved verbatim as \
                      unknown passthrough records." },
        )
    };

    let mut body = DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer {
            scope: cadmpeg_ir::ContentScope::full(),
            geometry_transferred: false,
        },
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: Vec::new(),
        notes,
    };
    body.losses.push(loss);
    body.losses.extend(dialect_losses);
    body
}

#[cfg(test)]
mod tests;
