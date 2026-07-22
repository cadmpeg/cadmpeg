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
use cadmpeg_ir::decode::{DecodeContext, View};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossCode, LossNote, Severity};
use cadmpeg_ir::units::{Tolerances, Units};
use cadmpeg_ir::unknown::UnknownRecord;

use crate::brep::{self, Brep};
use crate::container::{self, BrepFacts, ContainerScan};
use crate::{asm_header, materials, sab};

/// Decode a `.f3d` root view into a document and its loss report.
pub fn decode<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<DecodeResult, CodecError> {
    let scan = container::scan(ctx, root)?;

    if ctx.container_only() {
        let (ir, unknowns) = build_metadata_ir(&scan);
        let annotations = populate_annotations(&ir, &scan, &F3dNative::default(), None, &unknowns);
        let mut report = build_container_report(&scan, true);
        report_untransferred_assets(&scan, &mut report);
        return decode_result(
            ir,
            report,
            annotations,
            &unknowns,
            &preserve_source_image(&scan),
            cadmpeg_ir::SourceFidelity::default(),
        );
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
            let (mut ir, mut native, unknowns) = build_geometry_ir(&scan, &active, brep);
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
            native.design_body_members = crate::design::decode_body_members(&scan)?;
            native.design_configurations = crate::design::decode_configurations(&scan)?;
            let act = crate::act::decode(&scan)?;
            native.act_entities = act.entities;
            native.act_guids = act.guids;
            native.act_root_components = act.root_components;
            if !native.lost_edge_references.is_empty() {
                report.losses.push(
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
            let annotations = populate_annotations(
                &ir,
                &scan,
                &native,
                Some((&active.name, &annotation_records)),
                &unknowns,
            );
            report_untransferred_assets(&scan, &mut report);
            return decode_result(
                ir,
                report,
                annotations,
                &unknowns,
                &preserve_source_image(&scan),
                cadmpeg_ir::SourceFidelity::default(),
            );
        }
    }

    // No decodable SAB stream: use container metadata.
    let (mut ir, unknowns) = build_metadata_ir(&scan);
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
    native.design_body_members = crate::design::decode_body_members(&scan)?;
    native.design_configurations = crate::design::decode_configurations(&scan)?;
    let act = crate::act::decode(&scan)?;
    native.act_entities = act.entities;
    native.act_guids = act.guids;
    native.act_root_components = act.root_components;
    let decoded_materials = materials::decode(ctx, &scan)?;
    ir.model.appearances = decoded_materials.appearances;
    ir.model.appearance_bindings = decoded_materials.bindings;
    native.store(ir.native.namespace_mut("f3d"))?;
    let annotations = populate_annotations(&ir, &scan, &native, None, &unknowns);
    let mut report = build_container_report(&scan, false);
    report_untransferred_assets(&scan, &mut report);
    decode_result(
        ir,
        report,
        annotations,
        &unknowns,
        &preserve_source_image(&scan),
        cadmpeg_ir::SourceFidelity::default(),
    )
}

fn decode_result(
    mut ir: CadIr,
    report: DecodeReport,
    annotations: cadmpeg_ir::Annotations,
    unknowns: &[UnknownRecord],
    source_image: &UnknownRecord,
    mut source_fidelity: cadmpeg_ir::SourceFidelity,
) -> Result<DecodeResult, CodecError> {
    source_fidelity.annotations = annotations;
    source_fidelity.attach_native_unknown_records(&mut ir, "f3d", unknowns)?;
    source_fidelity.retain_unknown_records("f3d", std::slice::from_ref(source_image));
    ir.finalize();
    let hash = semantic_hash(&ir);
    if let Some(source) = &mut ir.source {
        source.attributes.insert("semantic_sha256".into(), hash);
    }
    Ok(DecodeResult::with_source_fidelity(
        ir,
        report,
        source_fidelity,
    ))
}

/// Reject strict decodes that omit mandatory, unreconstructable semantics.
/// Reports secondary archive assets that are not transferred.
fn report_untransferred_assets(scan: &ContainerScan, report: &mut DecodeReport) {
    use container::role;

    let mut seen = std::collections::BTreeSet::new();
    for entry in &scan.entries {
        if !seen.insert(entry.name.as_str()) {
            continue;
        }
        let role_label = container::classify(&entry.name);
        if matches!(role_label, role::PARAMESH | role::PREVIEW | role::IMAGE) {
            report
                .losses
                .push(untransferred_asset_loss(role_label, &entry.name));
        }
    }
}

/// Build the loss note for an untransferred secondary asset.
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

fn preserve_source_image(scan: &ContainerScan) -> UnknownRecord {
    let id = "f3d:file:source-image#0";
    UnknownRecord {
        id: UnknownId(id.into()),
        offset: 0,
        byte_len: scan.source_image.len() as u64,
        sha256: sha256_hex(scan.source_image),
        data: Some(scan.source_image.to_vec()),
        links: Vec::new(),
    }
}

pub(crate) fn semantic_hash(ir: &CadIr) -> String {
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
    ir: &CadIr,
    scan: &ContainerScan,
    native: &F3dNative,
    brep: Option<(&str, &[brep::AnnotationRecord])>,
    unknowns: &[UnknownRecord],
) -> cadmpeg_ir::Annotations {
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
            for unknown in unknowns {
                annotations
                    .note(&unknown.id.0, stream, unknown.offset)
                    .tag("opaque_brep");
            }
        }
    }
    annotations.build()
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
) -> (CadIr, F3dNative, Vec<UnknownRecord>) {
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
    (ir, native, brep.unknowns)
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

/// Build the loss report for a successful geometry decode.
fn build_geometry_report(scan: &ContainerScan, decoded: &Brep) -> DecodeReport {
    let s = &decoded.stats;
    let mut losses: Vec<LossNote> = Vec::new();

    if s.nurbs_surfaces > 0 {
        losses.push(LossNote {
            code: LossCode::ProceduralReduced,
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "{} spline surface record(s) were decoded into NURBS carriers from their inline \
                     cached B-spline block.",
                s.nurbs_surfaces
            ),
            provenance: None,
        });
    }
    if s.nurbs_curves > 0 {
        losses.push(LossNote {
            code: LossCode::ProceduralReduced,
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "{} procedural curve record(s) were decoded into NURBS carriers from their \
                     inline cached 3D B-spline block.",
                s.nurbs_curves
            ),
            provenance: None,
        });
    }
    if s.unknown_surface_faces > 0 {
        losses.push(LossNote {
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
        });
    }
    if s.procedural_curve_edges > 0 {
        losses.push(LossNote {
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
        });
    }
    if s.undecoded_pcurve_refs > 0 {
        losses.push(LossNote {
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
        });
    }
    if s.partial_procedural_supports > 0 {
        losses.push(
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
        losses.push(LossNote {
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
        });
    }
    losses.push(LossNote {
        code: LossCode::MaterialNotTransferred,
        category: LossCategory::Material,
        severity: Severity::Warning,
        message: "Materials/appearances (.protein assets, ACT/design assignments) were not \
                      transferred."
            .to_string(),
        provenance: None,
    });

    DecodeReport {
        format: "f3d".to_string(),
        container_only: false,
        geometry_transferred: true,
        coverage: std::collections::BTreeMap::new(),
        losses,
        notes: container::summarize(scan)
            .notes
            .into_iter()
            .filter(|note| !note.starts_with("container-level inspection only"))
            .collect(),
    }
}

fn build_metadata_ir(scan: &ContainerScan) -> (CadIr, Vec<UnknownRecord>) {
    let mut ir = CadIr::empty(Units::default());
    let mut unknowns = Vec::new();

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

        unknowns.push(UnknownRecord {
            id: UnknownId(format!("f3d:{}:unknown#0", brep.name)),
            offset: 0,
            byte_len: brep.uncompressed_len,
            sha256: brep.sha256.clone(),
            data: None,
            links: Vec::new(),
        });
    }

    ir.source = Some(SourceMeta {
        format: "f3d".to_string(),
        attributes,
    });
    (ir, unknowns)
}

fn build_container_report(scan: &ContainerScan, container_only: bool) -> DecodeReport {
    let summary = container::summarize(scan);
    let brep_count = scan.breps.len();

    let mut losses: Vec<LossNote> = Vec::new();
    losses.push(LossNote {
        code: LossCode::GeometryNotTransferred,
        category: LossCategory::Geometry,
        severity: Severity::Blocking,
        message: format!(
            "ASM BREP geometry was not transferred: the active stream is not a decodable \
                 BinaryFile4/BinaryFile8 SAB (or its framing failed). {brep_count} BREP stream(s) \
                 were located, but no surfaces, curves, or points were produced."
        ),
        provenance: None,
    });
    losses.push(LossNote {
        code: LossCode::TopologyNotTransferred,
        category: LossCategory::Topology,
        severity: Severity::Blocking,
        message: "B-rep topology graph (body/region/shell/face/loop/coedge/edge/vertex) was \
                      not built for this stream."
            .to_string(),
        provenance: None,
    });
    losses.push(LossNote {
        code: LossCode::MaterialNotTransferred,
        category: LossCategory::Material,
        severity: Severity::Warning,
        message: "Materials/appearances (.protein assets, ACT/design assignments) were not \
                      transferred."
            .to_string(),
        provenance: None,
    });

    if container::select_active_brep(scan).is_none() {
        losses.push(LossNote {
            code: LossCode::MissingGeometryStream,
            category: LossCategory::Geometry,
            severity: Severity::Error,
            message: "no ASM BREP stream (.smb/.smbh) was found in the container".to_string(),
            provenance: None,
        });
    }

    DecodeReport {
        format: "f3d".to_string(),
        container_only,
        geometry_transferred: false,
        coverage: std::collections::BTreeMap::new(),
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
