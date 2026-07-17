// SPDX-License-Identifier: Apache-2.0
#![deny(clippy::disallowed_methods)]
//! Assemble a `.f3d` archive into a [`CadIr`] document and [`DecodeReport`].
//!
//! [`crate::container`] scans the ZIP, reads ASM headers, finds the history
//! boundary, and selects the active B-rep. This module frames that B-rep with
//! [`crate::sab`], builds topology and geometry through [`crate::brep`], then
//! adds design, sketch, history, ACT, and appearance data.
//!
//! A framing failure or a stream without decoded geometry produces a
//! metadata-only document. The report marks geometry and topology as blocking,
//! and retained source data remains available for native replay.

use crate::native::F3dNative;
use cadmpeg_ir::annotations::AnnotationBuilder;
use cadmpeg_ir::codec::{CodecError, DecodeResult};
use cadmpeg_ir::decode::{
    DecodeContext, DecodeMode, RecordDisposition, RecordKind, SourceLocation, SpaceId, View,
};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::report::{
    DecodeReport, LossCategory, LossCode, LossNote, ProfileVersions, Severity, StrictConsequence,
};
use cadmpeg_ir::units::{Tolerances, Units};
use cadmpeg_ir::unknown::UnknownRecord;

use crate::brep::{self, Brep};
use crate::container::{self, BrepFacts, ContainerScan};
use crate::{asm_header, fidelity, materials, sab};

/// Decode a `.f3d` root view into a document and its loss report.
pub fn decode<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<DecodeResult, CodecError> {
    // The root `source` space each container entry's opaque payload is tiled in;
    // record tickets attribute their commit offsets against it (§6.2).
    let source_space = root.location().space;
    let scan = container::scan(ctx, root)?;

    if ctx.container_only() {
        let mut ir = build_metadata_ir(&scan)?;
        populate_annotations(&mut ir, &scan, &F3dNative::default(), None);
        preserve_source_image(&scan, &mut ir)?;
        let mut report = build_container_report(&scan, true);
        report.source_fidelity = Some(build_source_fidelity(&scan)?);
        account_records(
            ctx,
            source_space,
            &scan,
            &ir,
            &std::collections::BTreeMap::new(),
            &mut report,
        );
        return Ok(DecodeResult::new(ir, report));
    }

    // `try_decode_brep` returns `Some` after producing carriers or points.
    // A framed stream with no geometry uses the metadata-only path.
    if let Some(active) = container::select_active_brep(&scan).cloned() {
        if let Some((mut brep, mut report)) = try_decode_brep(&scan, &active)? {
            let decoded_materials = materials::decode_with_bodies(ctx, &scan, &brep.body_keys)?;
            let body_visibility = crate::design::decode_body_visibility(&scan, &active.name)?;
            let mut body_visibilities = Vec::new();
            for body in &mut brep.bodies {
                if let Some((asm_body_key, visibility)) =
                    brep.body_keys.get(&body.id).and_then(|key| {
                        body_visibility
                            .get(key)
                            .map(|visibility| (*key, visibility))
                    })
                {
                    body.visible = Some(visibility.visible);
                    body_visibilities.push(crate::records::BodyVisibility {
                        id: format!("f3d:{}:body-visibility#{}", visibility.stream, body.id),
                        body: body.id.clone(),
                        stream: visibility.stream.clone(),
                        byte_offset: visibility.byte_offset,
                        asm_body_key_offset: visibility.asm_body_key_offset,
                        asm_body_key,
                        entity_suffix: visibility.entity_suffix,
                        visible: visibility.visible,
                    });
                }
            }
            let annotation_records = std::mem::take(&mut brep.annotation_records);
            let (mut ir, mut native) = build_geometry_ir(&scan, &active, brep)?;
            native.body_visibilities = body_visibilities;
            if let Some(history) = decode_asm_history(&scan, &active)? {
                native.asm_histories.push(history);
            }
            native.construction_recipes = crate::design::decode_recipes(&scan)?;
            native.persistent_references = crate::design::decode_persistent_references(&scan)?;
            native.lost_edge_references = crate::design::decode_lost_edge_references(&scan)?;
            native.design_material_assignments =
                crate::materials::decode_design_assignments(&scan)?;
            native.design_objects = crate::design::decode_objects(&scan)?;
            native.design_entity_headers = crate::design::decode_entity_headers(&scan)?;
            native.design_record_headers =
                crate::design::decode_record_headers(&scan, &native.design_entity_headers)?;
            let sketch_relations = {
                crate::design::decode_sketch_relations(
                    &scan,
                    &native.design_record_headers,
                    &native.design_entity_headers,
                )?
            };
            native.sketch_relations = sketch_relations;
            extend_related_design_records(&scan, &mut native)?;
            native.sketch_points = crate::design::decode_sketch_points(&scan)?;
            native.sketch_curve_identities = crate::design::decode_sketch_curve_identities(&scan)?;
            native.design_body_members = crate::design::decode_body_members(ctx, &scan)?;
            native.design_configurations = crate::design::decode_configurations(&scan)?;
            let act = crate::act::decode(ctx, &scan)?;
            native.act_entities = act.entities;
            native.act_guids = act.guids;
            native.act_root_components = act.root_components;
            if !native.lost_edge_references.is_empty() {
                // Lost parametric edge references are an attribute concept the
                // decode cannot replay: route the omission through the Phase-4B
                // builder module so it is not a bare silent push.
                cadmpeg_ir::transfer::omit(
                    &mut report.losses,
                    LossNote {
                        code: LossCode::AttributesNotTransferred,
                        category: LossCategory::Attribute,
                        severity: Severity::Warning,
                        message: format!(
                            "{} source parametric edge reference(s) were marked EDGE_REFERENCE_LOST and cannot be replayed without repair.",
                            native.lost_edge_references.len()
                        ),
                        provenance: None,
                    },
                );
            }
            ir.model.appearances = decoded_materials.appearances;
            ir.model.appearance_bindings = decoded_materials.bindings;
            resolve_face_appearance_bindings(&mut ir, &decoded_materials.face_assignments);
            ir.model.appearance_bindings.sort_by(|a, b| a.id.cmp(&b.id));
            if !ir.model.appearances.is_empty() {
                if ir.model.appearance_bindings.is_empty() {
                    if let Some(loss) = report
                        .losses
                        .iter_mut()
                        .find(|loss| loss.category == LossCategory::Material)
                    {
                        loss.message = format!(
                            "{} Protein appearance asset(s) were decoded, but no topology assignment was resolved.",
                            ir.model.appearances.len()
                        );
                    }
                } else {
                    report
                        .losses
                        .retain(|loss| loss.category != LossCategory::Material);
                }
            }
            native.store(ir.native.namespace_mut("f3d"))?;
            populate_annotations(
                &mut ir,
                &scan,
                &native,
                Some((&active.name, &annotation_records)),
            );
            preserve_source_image(&scan, &mut ir)?;
            report.source_fidelity = Some(build_source_fidelity(&scan)?);
            account_records(
                ctx,
                source_space,
                &scan,
                &ir,
                &decoded_materials.appearance_origins,
                &mut report,
            );
            enforce_strict(ctx.mode(), &report)?;
            return Ok(DecodeResult::new(ir, report));
        }
    }

    // No decodable SAB stream: use container metadata.
    let mut ir = build_metadata_ir(&scan)?;
    let mut native = F3dNative::default();
    if let Some(active) = container::select_active_brep(&scan) {
        if let Some(history) = decode_asm_history(&scan, active)? {
            native.asm_histories.push(history);
        }
    }
    native.construction_recipes = crate::design::decode_recipes(&scan)?;
    native.persistent_references = crate::design::decode_persistent_references(&scan)?;
    native.lost_edge_references = crate::design::decode_lost_edge_references(&scan)?;
    native.design_material_assignments = crate::materials::decode_design_assignments(&scan)?;
    native.design_objects = crate::design::decode_objects(&scan)?;
    native.design_entity_headers = crate::design::decode_entity_headers(&scan)?;
    native.design_record_headers =
        crate::design::decode_record_headers(&scan, &native.design_entity_headers)?;
    let sketch_relations = {
        crate::design::decode_sketch_relations(
            &scan,
            &native.design_record_headers,
            &native.design_entity_headers,
        )?
    };
    native.sketch_relations = sketch_relations;
    extend_related_design_records(&scan, &mut native)?;
    native.sketch_points = crate::design::decode_sketch_points(&scan)?;
    native.sketch_curve_identities = crate::design::decode_sketch_curve_identities(&scan)?;
    native.design_body_members = crate::design::decode_body_members(ctx, &scan)?;
    native.design_configurations = crate::design::decode_configurations(&scan)?;
    let act = crate::act::decode(ctx, &scan)?;
    native.act_entities = act.entities;
    native.act_guids = act.guids;
    native.act_root_components = act.root_components;
    let decoded_materials = materials::decode(ctx, &scan)?;
    ir.model.appearances = decoded_materials.appearances;
    ir.model.appearance_bindings = decoded_materials.bindings;
    native.store(ir.native.namespace_mut("f3d"))?;
    populate_annotations(&mut ir, &scan, &native, None);
    preserve_source_image(&scan, &mut ir)?;
    let mut report = build_container_report(&scan, false);
    report.source_fidelity = Some(build_source_fidelity(&scan)?);
    account_records(
        ctx,
        source_space,
        &scan,
        &ir,
        &decoded_materials.appearance_origins,
        &mut report,
    );
    enforce_strict(ctx.mode(), &report)?;
    Ok(DecodeResult::new(ir, report))
}

/// Reject a decode in strict mode when the report carries a loss whose code
/// removes mandatory, unreconstructable semantics (§10 Phase 4).
///
/// Phase 4 requires strict mode to refuse a decode that could only be completed
/// by dropping semantics the target model treats as mandatory, rather than
/// silently returning a partial model. The decision keys on
/// [`LossCode::strict_consequence`] together with severity: a
/// [`Reject`](StrictConsequence::Reject) code (untransferred geometry or
/// topology) carried at [`Severity::Blocking`] marks mandatory, unreconstructable
/// semantics strict mode cannot tolerate — this is the f3d metadata-only
/// fallback, where the active B-rep stream was not a decodable SAB and no typed
/// geometry or topology was produced. The same Reject code at a lower severity is
/// an accountable partial loss over content that *was* transferred (faces on
/// undecoded spline surfaces standing beside a decoded topology graph); it,
/// accountable approximations ([`ProceduralReduced`](LossCode::ProceduralReduced)),
/// retained passthrough, and operator-requested omissions
/// [`Tolerate`](StrictConsequence::Tolerate) and pass through. Salvage mode never
/// rejects; it returns the same report with the loss code recorded, so every
/// strict rejection has a salvage counterpart that names the loss.
///
/// The refusal is surfaced as [`CodecError::Malformed`] with a `strict:` prefix:
/// the error taxonomy (§3.1) has no dedicated semantic-refusal decode variant,
/// and `Malformed` is the codebase's established spelling for a strict-mode
/// refusal (a classified error the stage-2 salvage/strict oracles accept).
/// Container-only decode never reaches here — the caller returns before entity
/// decode — so an operator-requested skip is never a strict rejection.
fn enforce_strict(mode: DecodeMode, report: &DecodeReport) -> Result<(), CodecError> {
    if mode != DecodeMode::Strict {
        return Ok(());
    }
    let mut codes: Vec<&'static str> = report
        .losses
        .iter()
        .filter(|loss| {
            loss.severity == Severity::Blocking
                && loss.code.strict_consequence() == StrictConsequence::Reject
        })
        .map(|loss| loss.code.as_str())
        .collect();
    if codes.is_empty() {
        return Ok(());
    }
    codes.sort_unstable();
    codes.dedup();
    Err(CodecError::Malformed(format!(
        "strict: mandatory semantics could not be represented (loss codes: {})",
        codes.join(", ")
    )))
}

/// Issue and resolve one record ticket per container entry the decode walked
/// (§6.2). The container entry is f3d's L1 commit boundary — the same unit the
/// L1 fidelity ledger tiles as one opaque payload span — so issuance instruments
/// the §3.3 boundary the codec already crosses rather than adding a separate
/// bookkeeping pass. Duplicate archive paths collapse to one ticket, matching
/// the ledger's derived-space dedup.
///
/// Each entry resolves at the point its outcome is decided:
/// - the active B-rep stream resolves [`RecordDisposition::Typed`] against its
///   emitted geometry entities when geometry transferred, or
///   [`RecordDisposition::Dropped`] against the blocking geometry loss when the
///   stream fell back to container metadata;
/// - a `.protein` appearance archive resolves `Typed` against its appearance
///   entities when appearances transferred, or `Dropped` against the material
///   loss otherwise;
/// - a secondary tessellation, preview, or image asset the codec does not
///   transfer resolves `Dropped` against a per-entry loss note appended here, so
///   the skip is accounted and never silent;
/// - every other entry — inactive B-rep snapshots and design/history/ACT streams
///   retained into the native namespace, plus pure container framing — resolves
///   [`RecordDisposition::Structural`]: its bytes are conservation-accounted by
///   the L1 ledger and it contributes no format-neutral model entity.
fn account_records(
    ctx: &DecodeContext,
    source_space: SpaceId,
    scan: &ContainerScan,
    ir: &CadIr,
    appearance_origins: &std::collections::BTreeMap<String, String>,
    report: &mut DecodeReport,
) {
    use container::role;

    let offsets: std::collections::BTreeMap<&str, u64> = scan
        .layout
        .iter()
        .map(|entry| (entry.name.as_str(), entry.compressed.start))
        .collect();
    let active = container::select_active_brep(scan).map(|brep| brep.name.clone());
    let mut seen = std::collections::BTreeSet::new();
    let mut material_note_taken = false;

    for entry in &scan.entries {
        if !seen.insert(entry.name.as_str()) {
            continue;
        }
        let role_label = container::classify(&entry.name);
        // Every admitted entry carries a data offset (`admit_entry` refuses those
        // without one), so it is always tiled into `layout`; a miss is a scan
        // invariant break, surfaced in debug builds rather than mislocated to 0.
        debug_assert!(
            offsets.contains_key(entry.name.as_str()),
            "container entry {} absent from scan layout",
            entry.name
        );
        let location = SourceLocation {
            space: source_space,
            offset: offsets.get(entry.name.as_str()).copied().unwrap_or(0),
        };
        let ticket = ctx.commit_record(location, RecordKind(role_label));

        let disposition = match role_label {
            role::BREP_SMBH | role::BREP_SMB => {
                if active.as_deref() == Some(entry.name.as_str()) {
                    if report.geometry_transferred {
                        let outputs = brep_outputs(ir);
                        if outputs.is_empty() {
                            RecordDisposition::Structural
                        } else {
                            RecordDisposition::Typed { outputs }
                        }
                    } else if let Some(loss) = find_loss(report, LossCategory::Geometry, |loss| {
                        loss.severity == Severity::Blocking
                    }) {
                        RecordDisposition::Dropped { loss }
                    } else {
                        RecordDisposition::Structural
                    }
                } else {
                    // An inactive construction snapshot is retained verbatim in
                    // the source image; its bytes are byte-accounted and it emits
                    // no format-neutral model entity.
                    RecordDisposition::Structural
                }
            }
            role::PROTEIN => {
                // Attribute only the appearances this archive decoded, keyed by the
                // per-entry origin map, so multiple `.protein` archives never each
                // claim the flattened union (§6.2). An archive that decoded nothing
                // resolves via its material loss, not a borrowed transfer.
                let outputs: Vec<String> = ir
                    .model
                    .appearances
                    .iter()
                    .filter(|appearance| {
                        appearance_origins.get(&appearance.id.0).map(String::as_str)
                            == Some(entry.name.as_str())
                    })
                    .map(|appearance| appearance.id.0.clone())
                    .collect();
                if !outputs.is_empty() {
                    RecordDisposition::Typed { outputs }
                } else if !material_note_taken {
                    match find_loss(report, LossCategory::Material, |_| true) {
                        Some(loss) => {
                            material_note_taken = true;
                            RecordDisposition::Dropped { loss }
                        }
                        None => RecordDisposition::Structural,
                    }
                } else {
                    RecordDisposition::Structural
                }
            }
            role::PARAMESH | role::PREVIEW | role::IMAGE => {
                // A walked secondary asset the codec does not transfer: an
                // omission that drains through the Phase-4B builder module so its
                // report entry cannot be skipped, then backs its `Dropped`
                // disposition (§6.2).
                let loss = untransferred_asset_loss(role_label, &entry.name);
                cadmpeg_ir::transfer::omit(&mut report.losses, loss.clone());
                RecordDisposition::Dropped { loss }
            }
            _ => RecordDisposition::Structural,
        };
        ctx.resolve(ticket, disposition);
    }
}

/// Model entity ids attributable to the active B-rep stream, for a `Typed`
/// disposition. Bodies are the natural output; a stream that produced only loose
/// geometry (surfaces, faces, points, or curves — the non-empty guarantee
/// [`try_decode_brep`] enforces) names one representative carrier instead.
fn brep_outputs(ir: &CadIr) -> Vec<String> {
    if !ir.model.bodies.is_empty() {
        return ir
            .model
            .bodies
            .iter()
            .map(|body| body.id.0.clone())
            .collect();
    }
    [
        ir.model.surfaces.first().map(|s| s.id.0.clone()),
        ir.model.faces.first().map(|f| f.id.0.clone()),
        ir.model.points.first().map(|p| p.id.0.clone()),
        ir.model.curves.first().map(|c| c.id.0.clone()),
    ]
    .into_iter()
    .flatten()
    .take(1)
    .collect()
}

/// The first report loss in `category` satisfying `pred`, cloned for a `Dropped`
/// disposition whose accountability rests on that already-emitted note.
fn find_loss(
    report: &DecodeReport,
    category: LossCategory,
    pred: impl Fn(&LossNote) -> bool,
) -> Option<LossNote> {
    report
        .losses
        .iter()
        .find(|loss| loss.category == category && pred(loss))
        .cloned()
}

/// A distinct per-entry loss note for a secondary asset the codec walked but did
/// not transfer, so its `Dropped` ticket consumes its own report entry (§6.2).
fn untransferred_asset_loss(role_label: &str, name: &str) -> LossNote {
    use container::role;

    let (category, message) = match role_label {
        role::PARAMESH => (
            LossCategory::Geometry,
            format!(
                "secondary tessellated mesh asset `{name}` (.paramesh) was not transferred; \
                 it is a derived preview, not the exact B-rep source."
            ),
        ),
        role::IMAGE => (
            LossCategory::Material,
            format!("appearance/decal image asset `{name}` was not transferred."),
        ),
        _ => (
            LossCategory::Other,
            format!("preview/thumbnail asset `{name}` was not transferred."),
        ),
    };
    LossNote {
        code: LossCode::AssetNotTransferred,
        category,
        severity: Severity::Info,
        message,
        provenance: None,
    }
}

/// Build the validated L1 container-accounting ledger for the scanned archive.
///
/// A tiling defect surfaces as a `Malformed` decode failure rather than a
/// silently absent proof, so an accounting-enabled report never ships an
/// inconsistent ledger through its `source_fidelity` slot.
fn build_source_fidelity(
    scan: &ContainerScan<'_>,
) -> Result<cadmpeg_ir::source_fidelity::SourceFidelity, CodecError> {
    fidelity::build_validated_ledger(scan).map_err(|e| {
        CodecError::Malformed(format!("f3d source-fidelity ledger is not a level: {e}"))
    })
}

fn preserve_source_image(scan: &ContainerScan, ir: &mut CadIr) -> Result<(), CodecError> {
    let id = "f3d:file:source-image#0";
    ir.push_native_unknown(
        "f3d",
        UnknownRecord {
            id: UnknownId(id.into()),
            offset: 0,
            byte_len: scan.source_image.len() as u64,
            sha256: sha256_hex(scan.source_image),
            data: Some(scan.source_image.to_vec()),
            links: Vec::new(),
        },
    )?;
    ir.finalize();
    let hash = semantic_hash(ir);
    if let Some(source) = &mut ir.source {
        source.attributes.insert("semantic_sha256".into(), hash);
    }
    Ok(())
}

pub(crate) fn semantic_hash(ir: &CadIr) -> String {
    // Normalize with a field-by-field clone so the retained source image (the
    // largest single payload) is filtered out instead of copied and dropped.
    let mut normalized = ir.clone();
    normalized.finalize();
    normalized.source = ir.source.as_ref().map(|source| {
        let mut source = source.clone();
        source.attributes.remove("semantic_sha256");
        source
    });
    let unknowns = ir
        .native_unknowns("f3d")
        .unwrap_or_default()
        .into_iter()
        .filter(|record| record.id.0 != "f3d:file:source-image#0")
        .collect::<Vec<_>>();
    normalized
        .set_native_unknowns("f3d", &unknowns)
        .expect("F3D unknown records serialize");
    sha256_hex(
        normalized
            .to_canonical_json()
            .expect("CadIr serialization")
            .as_bytes(),
    )
}

fn populate_annotations(
    ir: &mut CadIr,
    scan: &ContainerScan,
    native: &F3dNative,
    brep: Option<(&str, &[brep::AnnotationRecord])>,
) {
    let mut annotations = AnnotationBuilder::new();
    if let Some((stream_name, records)) = brep {
        let stream = annotations.stream(format!("f3d:{stream_name}"));
        for record in records {
            annotations
                .note(&record.id, stream, record.offset)
                .tag(&record.tag);
            for field in &record.derived_fields {
                annotations.derived(&record.id, *field);
            }
        }
    }

    let native_stream = annotations.stream("f3d:native");
    let mut note = |id: &str, tag: &str| {
        let offset = trailing_offset(id);
        annotations.note(id, native_stream, offset).tag(tag);
    };
    {
        for entity in &native.construction_recipes {
            note(&entity.id, "construction_recipe");
        }
        for entity in &native.persistent_references {
            note(&entity.id, "persistent_reference");
        }
        for entity in &native.lost_edge_references {
            note(&entity.id, "EDGE_REFERENCE_LOST");
        }
        for entity in &native.design_objects {
            note(&entity.id, "design_object");
        }
        for entity in &native.design_entity_headers {
            note(&entity.id, "design_entity_header");
        }
        for entity in &native.design_record_headers {
            note(&entity.id, "design_record_header");
        }
        for entity in &native.design_body_members {
            note(&entity.id, "BodiesRoot");
        }
        for entity in &native.design_material_assignments {
            note(&entity.id, "material_assignment");
        }
        for entity in &native.sketch_relations {
            note(&entity.id, "sketch_relation");
        }
        for entity in &native.sketch_points {
            note(&entity.id, "sketch_point");
        }
        for entity in &native.sketch_curve_identities {
            note(&entity.id, "sketch_curve");
        }
        for entity in &native.sketch_curve_links {
            note(&entity.id, "sketch_curve_link");
        }
        for entity in &native.persistent_design_links {
            note(&entity.id, "persistent_design_link");
        }
        for entity in &native.act_entities {
            note(&entity.id, "ACTEntity");
        }
        for entity in &native.act_guids {
            note(&entity.id, "ACTGuid");
        }
        for entity in &native.act_root_components {
            note(&entity.id, "ACTRootComponent");
        }
        for history in &native.asm_histories {
            note(&history.id, "history_stream");
            for state in &history.states {
                note(&state.id, "delta_state");
                for board in &state.bulletin_boards {
                    note(&board.id, "BulletinBoard");
                    for change in &board.changes {
                        note(&change.id, "entity_change");
                    }
                }
                for record in &state.records {
                    note(&record.id, &record.name);
                }
            }
        }
    }

    let appearance_stream = scan
        .entries
        .iter()
        .find(|entry| entry.role == container::role::PROTEIN)
        .map(|entry| annotations.stream(format!("f3d:{}", entry.name)));
    if let Some(stream) = appearance_stream {
        for appearance in &ir.model.appearances {
            annotations
                .note(&appearance.id.0, stream, 0)
                .tag(appearance.schema.as_deref().unwrap_or("appearance"));
        }
    }
    for binding in &ir.model.appearance_bindings {
        annotations
            .note(&binding.id, native_stream, 0)
            .tag("appearance_binding");
    }
    if brep.is_none() {
        if let Some(active) = container::select_active_brep(scan) {
            let stream = annotations.stream(format!("f3d:{}", active.name));
            for unknown in ir.native_unknowns("f3d").unwrap_or_default() {
                annotations
                    .note(&unknown.id.0, stream, unknown.offset)
                    .tag("opaque_brep");
            }
        }
    }
    ir.annotations = annotations.build();
}

fn trailing_offset(id: &str) -> u64 {
    id.rsplit(':')
        .find_map(|part| part.parse::<u64>().ok())
        .unwrap_or(0)
}

fn decode_asm_history(
    scan: &ContainerScan,
    active: &BrepFacts,
) -> Result<Option<crate::history_records::AsmHistory>, CodecError> {
    let width = active.header.as_ref().map_or(8, |h| usize::from(h.width));
    let bytes = scan.entry_bytes(&active.name)?;
    Ok(crate::history::decode(bytes, &active.name, width))
}

fn extend_related_design_records(
    scan: &ContainerScan,
    native: &mut F3dNative,
) -> Result<(), CodecError> {
    let indices = native
        .sketch_relations
        .iter()
        .flat_map(|relation| relation.members.iter().chain(&relation.return_members))
        .copied()
        .collect::<Vec<_>>();
    let existing = native
        .design_record_headers
        .iter()
        .map(|record| record.record_index)
        .collect::<std::collections::HashSet<_>>();
    native.design_record_headers.extend(
        crate::design::decode_related_record_headers(scan, &indices)?
            .into_iter()
            .filter(|record| !existing.contains(&record.record_index)),
    );
    native
        .design_record_headers
        .sort_by_key(|record| record.record_index);
    Ok(())
}

/// Frame and decode the active BREP's SAB stream. Returns `None` when the stream
/// is not a decodable `BinaryFile4`/`BinaryFile8` SAB, or frames but yields no
/// geometry (leaving the caller to fall back to the container-metadata IR).
fn try_decode_brep(
    scan: &ContainerScan,
    active: &BrepFacts,
) -> Result<Option<(Brep, DecodeReport)>, CodecError> {
    let width = active.header.as_ref().map_or(0, |h| h.width);
    if width != 4 && width != 8 {
        return Ok(None);
    }

    let bytes = scan.entry_bytes(&active.name)?;
    let Some(start) = asm_header::record_stream_start(bytes) else {
        return Ok(None);
    };
    let limit = active.delta_state_offset.unwrap_or(bytes.len());

    let records = match sab::frame(bytes, start, limit, usize::from(width)) {
        Ok(r) if !r.is_empty() => r,
        _ => return Ok(None),
    };

    let decoded = brep::decode(&records, bytes, &active.name);
    if decoded.surfaces.is_empty() && decoded.points.is_empty() && decoded.faces.is_empty() {
        return Ok(None);
    }
    let report = build_geometry_report(scan, &decoded);
    Ok(Some((decoded, report)))
}

/// Assemble the IR document from the decoded B-rep graph.
fn build_geometry_ir(
    scan: &ContainerScan,
    active: &BrepFacts,
    brep: Brep,
) -> Result<(CadIr, F3dNative), CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let (source, tolerances) = source_and_tolerances(scan, active);
    ir.source = Some(source);
    ir.tolerances = tolerances;

    ir.model.bodies = brep.bodies;
    ir.model.regions = brep.regions;
    ir.model.shells = brep.shells;
    ir.model.faces = brep.faces;
    ir.model.loops = brep.loops;
    ir.model.coedges = brep.coedges;
    ir.model.edges = brep.edges;
    ir.model.vertices = brep.vertices;
    ir.model.points = brep.points;
    ir.model.surfaces = brep.surfaces;
    ir.model.curves = brep.curves;
    ir.model.pcurves = brep.pcurves;
    ir.model.procedural_surfaces = brep.procedural_surfaces;
    ir.model.procedural_curves = brep.procedural_curves;
    let native = F3dNative {
        body_native_keys: brep.body_native_keys,
        sketch_curve_links: brep.sketch_curve_links,
        persistent_design_links: brep.persistent_design_links,
        edge_continuities: brep.edge_continuities,
        edge_ownerships: brep.edge_ownerships,
        vertex_ownerships: brep.vertex_ownerships,
        face_sidedness: brep.face_sidedness,
        tolerant_vertex_tails: brep.tolerant_vertex_tails,
        tolerant_coedge_parameters: brep.tolerant_coedge_parameters,
        wire_topologies: brep.wire_topologies,
        transform_hints: brep.transform_hints,
        creation_timestamps: brep.creation_timestamps,
        ..F3dNative::default()
    };
    ir.model.attributes = brep.attributes;
    ir.set_native_unknowns("f3d", &brep.unknowns)?;
    Ok((ir, native))
}

/// Source metadata attributes and kernel tolerances from the active BREP header.
fn source_and_tolerances(scan: &ContainerScan, active: &BrepFacts) -> (SourceMeta, Tolerances) {
    let mut attributes = std::collections::BTreeMap::new();
    if let Some(folder) = &scan.asset_folder {
        attributes.insert("asset_folder".to_string(), folder.clone());
    }
    attributes.insert(
        "zip_entry_count".to_string(),
        scan.entries.len().to_string(),
    );
    attributes.insert("active_brep".to_string(), active.name.clone());
    attributes.insert("active_brep_sha256".to_string(), active.sha256.clone());
    if let Some(off) = active.delta_state_offset {
        attributes.insert("active_slice_len".to_string(), off.to_string());
    }

    let mut tolerances = Tolerances::default();
    if let Some(h) = &active.header {
        if let Some(pf) = &h.product_family {
            attributes.insert("product_family".to_string(), pf.clone());
        }
        if let Some(pv) = &h.product_version {
            attributes.insert("product_version".to_string(), pv.clone());
        }
        if let Some(sd) = &h.save_date {
            attributes.insert("save_date".to_string(), sd.clone());
        }
        if let (Some(resabs), Some(resnor)) = (h.linear, h.angular) {
            tolerances = Tolerances {
                linear: resabs,
                angular: resnor,
            };
        }
    }

    (
        SourceMeta {
            format: "f3d".to_string(),
            attributes,
        },
        tolerances,
    )
}

/// Loss report for a successful geometry decode.
///
/// Every note is constructed through the shared platform helpers (doc §6.2,
/// §10 Phase 4B): a concept the decode did not carry into typed IR goes through
/// [`cadmpeg_ir::transfer::omit`] so the omission cannot be reached without
/// recording its note, while a reduction that survives approximately
/// (spline/procedural forms solved into cached NURBS carriers) or an
/// informational census goes through [`cadmpeg_ir::transfer::reduce`]. Both
/// resolve through the platform [`Builder`](cadmpeg_ir::transfer::Builder) into
/// the report's loss channel. This function routes every note through those
/// helpers rather than the bare `losses.push` spelling; the guarantee is that
/// the platform builder is the one construction path used on the geometry path,
/// not a type error against `Vec::push` (the platform `Builder` cannot ban the
/// method). A direct push would compile — review keeps it out.
fn build_geometry_report(scan: &ContainerScan, decoded: &Brep) -> DecodeReport {
    use cadmpeg_ir::transfer::{omit, reduce};

    let s = &decoded.stats;
    let mut losses: Vec<LossNote> = Vec::new();

    if s.nurbs_surfaces > 0 {
        reduce(
            &mut losses,
            LossNote {
                code: LossCode::ProceduralReduced,
                category: LossCategory::Geometry,
                severity: Severity::Info,
                message: format!(
                    "{} spline surface record(s) were decoded into NURBS carriers from their inline \
                     cached B-spline block.",
                    s.nurbs_surfaces
                ),
                provenance: None,
            },
        );
    }
    if s.nurbs_curves > 0 {
        reduce(
            &mut losses,
            LossNote {
                code: LossCode::ProceduralReduced,
                category: LossCategory::Geometry,
                severity: Severity::Info,
                message: format!(
                    "{} procedural curve record(s) were decoded into NURBS carriers from their \
                     inline cached 3D B-spline block.",
                    s.nurbs_curves
                ),
                provenance: None,
            },
        );
    }
    if s.unknown_surface_faces > 0 {
        // Unsupported-concept-to-omission boundary: the underlying surface shape
        // was not decoded into a typed carrier. Topology transfers; the shape
        // does not, so the omitted value does not exist.
        omit(
            &mut losses,
            LossNote {
                code: LossCode::GeometryNotTransferred,
                category: LossCategory::Geometry,
                severity: Severity::Warning,
                message: format!(
                    "{} face(s) rest on spline/procedural surfaces whose shape was not decoded into \
                     a typed carrier (no inline cached B-spline block — the cache is reached \
                     through a subtype reference, or the record is a procedural form this codec \
                     does not evaluate); the face, its loops, and trims are emitted with an \
                     unknown-geometry surface linking to the preserved record bytes. Topology is \
                     transferred; the underlying surface shape is not.",
                    s.unknown_surface_faces
                ),
                provenance: None,
            },
        );
    }
    if s.procedural_curve_edges > 0 {
        omit(
            &mut losses,
            LossNote {
                code: LossCode::GeometryNotTransferred,
                category: LossCategory::Geometry,
                severity: Severity::Warning,
                message: format!(
                    "{} edge(s) reference a procedural intcurve/spline 3D curve with no decodable \
                     inline B-spline cache; the edge was emitted with its vertices and parameter \
                     range but no attributed curve carrier.",
                    s.procedural_curve_edges
                ),
                provenance: None,
            },
        );
    }
    if s.undecoded_pcurve_refs > 0 {
        omit(
            &mut losses,
            LossNote {
                code: LossCode::PcurveOmitted,
                category: LossCategory::Geometry,
                severity: Severity::Warning,
                message: format!(
                    "{} coedge(s) carry an explicit UV pcurve reference with no decodable 2D \
                     carrier on the face surface's parameterization; those coedges were emitted \
                     without a pcurve.",
                    s.undecoded_pcurve_refs
                ),
                provenance: None,
            },
        );
    }
    if s.partial_procedural_supports > 0 {
        reduce(
            &mut losses,
            LossNote {
                code: LossCode::CarrierSummary,
                category: LossCategory::Geometry,
                severity: Severity::Warning,
                message: format!(
                    "{} rolling-ball blend definition(s) retain their signed radius and solved cache, but only one of two native supports resolved.",
                    s.partial_procedural_supports
                ),
                provenance: None,
            },
        );
    }
    if s.other_records > 0 {
        omit(
            &mut losses,
            LossNote {
                code: LossCode::AttributesNotTransferred,
                category: LossCategory::Attribute,
                severity: Severity::Warning,
                message: format!(
                    "{} active-slice application/refinement record(s) were not transferred: {}.",
                    s.other_records,
                    s.other_record_kinds
                        .iter()
                        .map(|(name, count)| format!("{name}={count}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                provenance: None,
            },
        );
    }
    omit(
        &mut losses,
        LossNote {
            code: LossCode::MaterialNotTransferred,
            category: LossCategory::Material,
            severity: Severity::Warning,
            message: "Materials/appearances (.protein assets, ACT/design assignments) were not \
                      transferred."
                .to_string(),
            provenance: None,
        },
    );

    DecodeReport {
        retention_degraded: false,
        profile_versions: ProfileVersions::default(),
        source_fidelity: None,
        format: "f3d".to_string(),
        container_only: false,
        geometry_transferred: true,
        losses,
        notes: container::summarize(scan)
            .notes
            .into_iter()
            .filter(|note| !note.starts_with("container-level inspection only"))
            .collect(),
    }
}

fn build_metadata_ir(scan: &ContainerScan) -> Result<CadIr, CodecError> {
    let mut ir = CadIr::empty(Units::default());

    let mut attributes = std::collections::BTreeMap::new();
    if let Some(folder) = &scan.asset_folder {
        attributes.insert("asset_folder".to_string(), folder.clone());
    }
    attributes.insert(
        "zip_entry_count".to_string(),
        scan.entries.len().to_string(),
    );

    if let Some(brep) = container::select_active_brep(scan) {
        attributes.insert("active_brep".to_string(), brep.name.clone());
        attributes.insert("active_brep_sha256".to_string(), brep.sha256.clone());
        if let Some(off) = brep.delta_state_offset {
            attributes.insert("active_slice_len".to_string(), off.to_string());
        }
        if let Some(h) = &brep.header {
            if let Some(pf) = &h.product_family {
                attributes.insert("product_family".to_string(), pf.clone());
            }
            if let Some(pv) = &h.product_version {
                attributes.insert("product_version".to_string(), pv.clone());
            }
            if let Some(sd) = &h.save_date {
                attributes.insert("save_date".to_string(), sd.clone());
            }
            if let (Some(resabs), Some(resnor)) = (h.linear, h.angular) {
                ir.tolerances = Tolerances {
                    linear: resabs,
                    angular: resnor,
                };
            }
        }

        ir.push_native_unknown(
            "f3d",
            UnknownRecord {
                id: UnknownId(format!("f3d:{}:unknown#0", brep.name)),
                offset: 0,
                byte_len: brep.uncompressed_len,
                sha256: brep.sha256.clone(),
                data: None,
                links: Vec::new(),
            },
        )?;
    }

    ir.source = Some(SourceMeta {
        format: "f3d".to_string(),
        attributes,
    });
    Ok(ir)
}

fn build_container_report(scan: &ContainerScan, container_only: bool) -> DecodeReport {
    let summary = container::summarize(scan);
    let brep_count = scan.breps.len();

    // The metadata-only fallback: geometry, topology, and materials are all
    // concepts this decode did not carry into typed IR. Each resolves through
    // the Phase-4B builder module as an omission (§10) so no drop is silent;
    // retained source bytes remain available for native replay.
    let mut losses: Vec<LossNote> = Vec::new();
    cadmpeg_ir::transfer::omit(
        &mut losses,
        LossNote {
            code: LossCode::GeometryNotTransferred,
            category: LossCategory::Geometry,
            severity: Severity::Blocking,
            message: format!(
                "ASM BREP geometry was not transferred: the active stream is not a decodable \
                 BinaryFile4/BinaryFile8 SAB (or its framing failed). {brep_count} BREP stream(s) \
                 were located, but no surfaces, curves, or points were produced."
            ),
            provenance: None,
        },
    );
    cadmpeg_ir::transfer::omit(
        &mut losses,
        LossNote {
            code: LossCode::TopologyNotTransferred,
            category: LossCategory::Topology,
            severity: Severity::Blocking,
            message: "B-rep topology graph (body/region/shell/face/loop/coedge/edge/vertex) was \
                      not built for this stream."
                .to_string(),
            provenance: None,
        },
    );
    cadmpeg_ir::transfer::omit(
        &mut losses,
        LossNote {
            code: LossCode::MaterialNotTransferred,
            category: LossCategory::Material,
            severity: Severity::Warning,
            message: "Materials/appearances (.protein assets, ACT/design assignments) were not \
                      transferred."
                .to_string(),
            provenance: None,
        },
    );

    if container::select_active_brep(scan).is_none() {
        cadmpeg_ir::transfer::omit(
            &mut losses,
            LossNote {
                code: LossCode::MissingGeometryStream,
                category: LossCategory::Geometry,
                severity: Severity::Error,
                message: "no ASM BREP stream (.smb/.smbh) was found in the container".to_string(),
                provenance: None,
            },
        );
    }

    DecodeReport {
        retention_degraded: false,
        profile_versions: ProfileVersions::default(),
        source_fidelity: None,
        format: "f3d".to_string(),
        container_only,
        geometry_transferred: false,
        losses,
        notes: summary.notes,
    }
}

/// Join per-face appearance assignments to BREP faces through the face GUID
/// carried by each face's `NEUTRON_Material_attrib_def` attribute
/// ([spec §8.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#82-materials)).
fn resolve_face_appearance_bindings(
    ir: &mut CadIr,
    face_assignments: &[materials::FaceAppearanceAssignment],
) {
    use cadmpeg_ir::appearance::{AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};

    if face_assignments.is_empty() {
        return;
    }
    let mut faces_by_guid: std::collections::HashMap<&str, Vec<cadmpeg_ir::ids::FaceId>> =
        std::collections::HashMap::new();
    for attribute in &ir.model.attributes {
        let AttributeTarget::Face(face) = &attribute.target else {
            continue;
        };
        let strings: Vec<&str> = attribute
            .values
            .iter()
            .filter_map(|value| match value {
                AttributeValue::String(value) => Some(value.as_str()),
                _ => None,
            })
            .collect();
        if !strings.contains(&"NEUTRON_Material_attrib_def") {
            continue;
        }
        for value in strings {
            if value.len() == 36 && value.matches('-').count() == 4 {
                faces_by_guid.entry(value).or_default().push(face.clone());
            }
        }
    }
    let mut bound_targets = ir
        .model
        .appearance_bindings
        .iter()
        .map(|binding| binding.target.clone())
        .collect::<std::collections::HashSet<_>>();
    for assignment in face_assignments {
        let Some(faces) = faces_by_guid.get(assignment.face_guid.as_str()) else {
            continue;
        };
        let Some(appearance) = ir.model.appearances.iter().find(|appearance| {
            appearance
                .visual_guid
                .as_deref()
                .is_some_and(|guid| guid.starts_with(&assignment.visual_guid))
        }) else {
            continue;
        };
        for face in faces {
            let target = AppearanceTarget::Face(face.clone());
            if !bound_targets.insert(target.clone()) {
                continue;
            }
            ir.model.appearance_bindings.push(AppearanceBinding {
                id: format!(
                    "f3d:appearance:face#{}:{}",
                    assignment.face_guid, assignment.visual_guid
                ),
                target,
                appearance: appearance.id.clone(),
                source_entity_id: None,
                object_type: None,
                channels: std::collections::BTreeMap::new(),
            });
        }
    }
}
