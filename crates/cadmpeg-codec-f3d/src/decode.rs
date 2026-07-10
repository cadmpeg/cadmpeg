// SPDX-License-Identifier: Apache-2.0
//! Decode a `.f3d` into an IR document, transferring the B-rep topology graph
//! and analytic geometry the SAB decoder understands and reporting the rest as
//! explicit loss.
//!
//! The container layer (ZIP entries, ASM header, `delta_state` boundary, active
//! BREP selection) is decoded by [`crate::container`]. This module frames the
//! active BREP's SAB record stream ([`crate::sab`]) and builds the IR B-rep
//! graph, analytic/cached NURBS carriers, pcurves, attributes, transforms, and
//! Protein/Design appearances ([`crate::brep`], [`crate::materials`]). Remaining
//! unsupported records are accounted for in the [`DecodeReport`]. When the stream is not a
//! decodable `BinaryFile8` SAB, or framing fails, decode falls back to the
//! container-metadata IR (active BREP preserved as an [`UnknownRecord`]) and
//! says so.

use cadmpeg_ir::annotations::AnnotationBuilder;
use cadmpeg_ir::codec::{CodecError, DecodeOptions, DecodeResult, ReadSeek};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::native::F3dNative;
use cadmpeg_ir::provenance::{EntityMeta, Exactness, Provenance};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::units::{Tolerances, Units};
use cadmpeg_ir::unknown::UnknownRecord;

use crate::brep::{self, Brep};
use crate::container::{self, BrepFacts, ContainerScan};
use crate::{asm_header, materials, sab};

/// Decode a `.f3d` reader into an IR + report.
pub fn decode(
    reader: &mut dyn ReadSeek,
    options: &DecodeOptions,
) -> Result<DecodeResult, CodecError> {
    let scan = container::scan(reader)?;

    if options.container_only {
        let mut ir = build_metadata_ir(&scan);
        finalize_dual_write(&mut ir);
        let report = build_container_report(&scan, true);
        return Ok(DecodeResult { ir, report });
    }

    // Attempt a real geometry decode of the active BREP. `try_decode_brep`
    // yields `Some` only when it actually produced carriers/points; a stream
    // that frames but carries no geometry falls through to the honest metadata
    // path rather than reporting an empty graph as a geometry transfer.
    if let Some(active) = container::select_active_brep(&scan).cloned() {
        if let Some((brep, mut report)) = try_decode_brep(reader, &scan, &active)? {
            let decoded_materials = materials::decode_with_bodies(reader, &scan, &brep.body_keys)?;
            let mut ir = build_geometry_ir(&scan, &active, brep);
            if let Some(history) = decode_asm_history(reader, &active)? {
                ir.asm_histories.push(history);
            }
            ir.construction_recipes = crate::design::decode_recipes(reader, &scan)?;
            ir.persistent_references = crate::design::decode_persistent_references(reader, &scan)?;
            ir.lost_edge_references = crate::design::decode_lost_edge_references(reader, &scan)?;
            ir.design_objects = crate::design::decode_objects(reader, &scan)?;
            ir.design_entity_headers = crate::design::decode_entity_headers(reader, &scan)?;
            ir.design_record_headers =
                crate::design::decode_record_headers(reader, &scan, &ir.design_entity_headers)?;
            ir.sketch_relations =
                crate::design::decode_sketch_relations(reader, &scan, &ir.design_record_headers)?;
            extend_related_design_records(reader, &scan, &mut ir)?;
            ir.sketch_points = crate::design::decode_sketch_points(reader, &scan)?;
            ir.sketch_curve_identities =
                crate::design::decode_sketch_curve_identities(reader, &scan)?;
            ir.design_body_members = crate::design::decode_body_members(reader, &scan)?;
            let act = crate::act::decode(reader, &scan)?;
            ir.act_entities = act.entities;
            ir.act_guids = act.guids;
            ir.act_root_components = act.root_components;
            if !ir.lost_edge_references.is_empty() {
                report.losses.push(LossNote {
                    category: LossCategory::Attribute,
                    severity: Severity::Warning,
                    message: format!(
                        "{} source parametric edge reference(s) were marked EDGE_REFERENCE_LOST and cannot be replayed without repair.",
                        ir.lost_edge_references.len()
                    ),
                    provenance: None,
                });
            }
            ir.appearances = decoded_materials.appearances;
            ir.appearance_bindings = decoded_materials.bindings;
            if !ir.appearances.is_empty() {
                if ir.appearance_bindings.is_empty() {
                    if let Some(loss) = report
                        .losses
                        .iter_mut()
                        .find(|loss| loss.category == LossCategory::Material)
                    {
                        loss.message = format!(
                            "{} Protein appearance asset(s) were decoded, but no topology assignment was resolved.",
                            ir.appearances.len()
                        );
                    }
                } else {
                    report
                        .losses
                        .retain(|loss| loss.category != LossCategory::Material);
                }
            }
            finalize_dual_write(&mut ir);
            return Ok(DecodeResult { ir, report });
        }
    }

    // No decodable SAB stream: honest container-metadata fallback.
    let mut ir = build_metadata_ir(&scan);
    if let Some(active) = container::select_active_brep(&scan) {
        if let Some(history) = decode_asm_history(reader, active)? {
            ir.asm_histories.push(history);
        }
    }
    ir.construction_recipes = crate::design::decode_recipes(reader, &scan)?;
    ir.persistent_references = crate::design::decode_persistent_references(reader, &scan)?;
    ir.lost_edge_references = crate::design::decode_lost_edge_references(reader, &scan)?;
    ir.design_objects = crate::design::decode_objects(reader, &scan)?;
    ir.design_entity_headers = crate::design::decode_entity_headers(reader, &scan)?;
    ir.design_record_headers =
        crate::design::decode_record_headers(reader, &scan, &ir.design_entity_headers)?;
    ir.sketch_relations =
        crate::design::decode_sketch_relations(reader, &scan, &ir.design_record_headers)?;
    extend_related_design_records(reader, &scan, &mut ir)?;
    ir.sketch_points = crate::design::decode_sketch_points(reader, &scan)?;
    ir.sketch_curve_identities = crate::design::decode_sketch_curve_identities(reader, &scan)?;
    ir.design_body_members = crate::design::decode_body_members(reader, &scan)?;
    let act = crate::act::decode(reader, &scan)?;
    ir.act_entities = act.entities;
    ir.act_guids = act.guids;
    ir.act_root_components = act.root_components;
    let decoded_materials = materials::decode(reader, &scan)?;
    ir.appearances = decoded_materials.appearances;
    ir.appearance_bindings = decoded_materials.bindings;
    let report = build_container_report(&scan, false);
    finalize_dual_write(&mut ir);
    Ok(DecodeResult { ir, report })
}

fn finalize_dual_write(ir: &mut CadIr) {
    ir.native.f3d = Some(F3dNative {
        act_entities: ir.act_entities.clone(),
        act_guids: ir.act_guids.clone(),
        act_root_components: ir.act_root_components.clone(),
        design_objects: ir.design_objects.clone(),
        design_entity_headers: ir.design_entity_headers.clone(),
        design_record_headers: ir.design_record_headers.clone(),
        design_body_members: ir.design_body_members.clone(),
        construction_recipes: ir.construction_recipes.clone(),
        persistent_design_links: ir.persistent_design_links.clone(),
        persistent_references: ir.persistent_references.clone(),
        sketch_curve_links: ir.sketch_curve_links.clone(),
        sketch_relations: ir.sketch_relations.clone(),
        sketch_points: ir.sketch_points.clone(),
        sketch_curve_identities: ir.sketch_curve_identities.clone(),
        lost_edge_references: ir.lost_edge_references.clone(),
        asm_histories: ir.asm_histories.clone(),
        ..F3dNative::default()
    });

    let mut annotations = AnnotationBuilder::new();
    macro_rules! annotate {
        ($arena:expr, $key:expr) => {
            for entity in $arena {
                let key = ($key)(entity);
                let stream = annotations.stream(entity.meta.provenance.stream.clone());
                let note = annotations.note(key.clone(), stream, entity.meta.provenance.offset);
                if let Some(tag) = &entity.meta.provenance.tag {
                    note.tag(tag.clone());
                }
                annotations.exactness(key, entity.meta.exactness);
            }
        };
    }

    annotate!(&ir.bodies, |e: &cadmpeg_ir::topology::Body| e.id.0.clone());
    annotate!(&ir.lumps, |e: &cadmpeg_ir::topology::Lump| e.id.0.clone());
    annotate!(&ir.shells, |e: &cadmpeg_ir::topology::Shell| e.id.0.clone());
    annotate!(&ir.faces, |e: &cadmpeg_ir::topology::Face| e.id.0.clone());
    annotate!(&ir.loops, |e: &cadmpeg_ir::topology::Loop| e.id.0.clone());
    annotate!(&ir.coedges, |e: &cadmpeg_ir::topology::Coedge| e
        .id
        .0
        .clone());
    annotate!(&ir.edges, |e: &cadmpeg_ir::topology::Edge| e.id.0.clone());
    annotate!(&ir.vertices, |e: &cadmpeg_ir::topology::Vertex| e
        .id
        .0
        .clone());
    annotate!(&ir.points, |e: &cadmpeg_ir::topology::Point| e.id.0.clone());
    annotate!(&ir.surfaces, |e: &cadmpeg_ir::geometry::Surface| e
        .id
        .0
        .clone());
    annotate!(&ir.curves, |e: &cadmpeg_ir::geometry::Curve| e.id.0.clone());
    annotate!(&ir.pcurves, |e: &cadmpeg_ir::geometry::Pcurve| e
        .id
        .0
        .clone());
    annotate!(
        &ir.surface_parameterizations,
        |e: &cadmpeg_ir::geometry::SurfaceParameterization| e.surface.0.clone()
    );
    annotate!(
        &ir.procedural_surfaces,
        |e: &cadmpeg_ir::geometry::ProceduralSurface| e.surface.0.clone()
    );
    annotate!(
        &ir.procedural_curves,
        |e: &cadmpeg_ir::geometry::ProceduralCurve| e.curve.0.clone()
    );
    annotate!(
        &ir.sketch_curve_links,
        |e: &cadmpeg_ir::design::SketchCurveLink| {
            format!("{}:{}", e.coedge.0, e.sketch_curve_id)
        }
    );
    annotate!(
        &ir.persistent_design_links,
        |e: &cadmpeg_ir::design::PersistentDesignLink| {
            format!("{:?}:{}:{}", e.target, e.design_id, e.ordinal)
        }
    );
    annotate!(
        &ir.construction_recipes,
        |e: &cadmpeg_ir::design::ConstructionRecipe| {
            format!("{:?}:{:?}:{}", e.kind, e.design_id, e.recipe_index)
        }
    );
    annotate!(
        &ir.persistent_references,
        |e: &cadmpeg_ir::design::PersistentReference| {
            format!("{:?}:{}", e.kind, e.meta.provenance.offset)
        }
    );
    annotate!(
        &ir.lost_edge_references,
        |e: &cadmpeg_ir::design::LostEdgeReference| {
            format!("{}:{}", e.class_tag, e.record_index)
        }
    );
    annotate!(
        &ir.design_objects,
        |e: &cadmpeg_ir::design::DesignObject| e.self_guid.clone()
    );
    annotate!(
        &ir.design_entity_headers,
        |e: &cadmpeg_ir::design::DesignEntityHeader| { format!("{}:{}", e.class_tag, e.entity_id) }
    );
    annotate!(
        &ir.design_record_headers,
        |e: &cadmpeg_ir::design::DesignRecordHeader| {
            format!("{}:{}", e.record_index, e.class_tag)
        }
    );
    annotate!(
        &ir.sketch_relations,
        |e: &cadmpeg_ir::design::SketchRelation| e.record_index.to_string()
    );
    annotate!(&ir.sketch_points, |e: &cadmpeg_ir::design::SketchPoint| e
        .record_index
        .to_string());
    annotate!(
        &ir.sketch_curve_identities,
        |e: &cadmpeg_ir::design::SketchCurveIdentity| e.record_index.to_string()
    );
    annotate!(
        &ir.design_body_members,
        |e: &cadmpeg_ir::design::DesignBodyMember| e.entity_suffix.to_string()
    );
    annotate!(&ir.act_entities, |e: &cadmpeg_ir::design::ActEntity| {
        format!("{}:{}", e.record_index, e.entity_id)
    });
    annotate!(&ir.act_guids, |e: &cadmpeg_ir::design::ActGuid| {
        format!("{}:{}", e.ordinal, e.guid)
    });
    annotate!(
        &ir.act_root_components,
        |e: &cadmpeg_ir::design::ActRootComponent| {
            format!("{}:{}", e.record_index, e.entity_id)
        }
    );
    annotate!(&ir.asm_histories, |e: &cadmpeg_ir::history::AsmHistory| {
        format!("{}:{}", e.meta.provenance.stream, e.meta.provenance.offset)
    });
    annotate!(&ir.appearances, |e: &cadmpeg_ir::appearance::Appearance| e
        .id
        .0
        .clone());
    annotate!(
        &ir.appearance_bindings,
        |e: &cadmpeg_ir::appearance::AppearanceBinding| {
            format!("{:?}:{}", e.target, e.appearance.0)
        }
    );
    annotate!(
        &ir.attributes,
        |e: &cadmpeg_ir::attributes::SourceAttribute| e.id.0.clone()
    );
    annotate!(&ir.unknowns, |e: &cadmpeg_ir::unknown::UnknownRecord| e
        .id
        .0
        .clone());

    for edge in &ir.edges {
        if edge.param_range.is_some()
            && edge
                .curve
                .as_ref()
                .and_then(|curve| ir.curves.iter().find(|candidate| candidate.id == *curve))
                .is_some_and(|curve| {
                    matches!(
                        curve.geometry,
                        cadmpeg_ir::geometry::CurveGeometry::Circle { .. }
                            | cadmpeg_ir::geometry::CurveGeometry::Ellipse { .. }
                    )
                })
        {
            annotations.derived(&edge.id.0, "param_range");
        }
    }
    for surface in &ir.surfaces {
        let source_frame = ir
            .surface_parameterizations
            .iter()
            .any(|frame| frame.surface == surface.id);
        if !source_frame {
            match surface.geometry {
                cadmpeg_ir::geometry::SurfaceGeometry::Plane {
                    u_axis: Some(_), ..
                } => {
                    annotations.derived(&surface.id.0, "geometry.u_axis");
                }
                cadmpeg_ir::geometry::SurfaceGeometry::Cylinder {
                    ref_direction: Some(_),
                    ..
                }
                | cadmpeg_ir::geometry::SurfaceGeometry::Cone {
                    ref_direction: Some(_),
                    ..
                }
                | cadmpeg_ir::geometry::SurfaceGeometry::Sphere {
                    ref_direction: Some(_),
                    ..
                }
                | cadmpeg_ir::geometry::SurfaceGeometry::Torus {
                    ref_direction: Some(_),
                    ..
                } => {
                    annotations.derived(&surface.id.0, "geometry.ref_direction");
                }
                _ => {}
            }
        }
    }
    ir.annotations = annotations.build();
}

fn decode_asm_history(
    reader: &mut dyn ReadSeek,
    active: &BrepFacts,
) -> Result<Option<cadmpeg_ir::history::AsmHistory>, CodecError> {
    let bytes = container::decompress_entry(reader, &active.name)?;
    Ok(crate::history::decode(&bytes, &active.name))
}

fn extend_related_design_records(
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
    ir: &mut CadIr,
) -> Result<(), CodecError> {
    let indices = ir
        .sketch_relations
        .iter()
        .flat_map(|relation| relation.members.iter().chain(&relation.return_members))
        .copied()
        .collect::<Vec<_>>();
    let existing = ir
        .design_record_headers
        .iter()
        .map(|record| record.record_index)
        .collect::<std::collections::HashSet<_>>();
    ir.design_record_headers.extend(
        crate::design::decode_related_record_headers(reader, scan, &indices)?
            .into_iter()
            .filter(|record| !existing.contains(&record.record_index)),
    );
    ir.design_record_headers
        .sort_by_key(|record| record.meta.provenance.offset);
    Ok(())
}

/// Frame and decode the active BREP's SAB stream. Returns `None` when the stream
/// is not a decodable `BinaryFile8` SAB, or frames but yields no geometry
/// (leaving the caller to fall back to the container-metadata IR).
fn try_decode_brep(
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
    active: &BrepFacts,
) -> Result<Option<(Brep, DecodeReport)>, CodecError> {
    // Only the documented BinaryFile8 record layout is decoded.
    let width = active.header.as_ref().map_or(0, |h| h.width);
    if width != 8 {
        return Ok(None);
    }

    let bytes = container::decompress_entry(reader, &active.name)?;
    let Some(start) = asm_header::record_stream_start(&bytes) else {
        return Ok(None);
    };
    let limit = active.delta_state_offset.unwrap_or(bytes.len());

    let records = match sab::frame(&bytes, start, limit, 8) {
        Ok(r) if !r.is_empty() => r,
        _ => return Ok(None),
    };

    let decoded = brep::decode(&records, &bytes, &active.name);
    if decoded.surfaces.is_empty() && decoded.points.is_empty() && decoded.faces.is_empty() {
        return Ok(None);
    }
    let report = build_geometry_report(scan, &decoded);
    Ok(Some((decoded, report)))
}

/// Assemble the IR document from the decoded B-rep graph.
fn build_geometry_ir(scan: &ContainerScan, active: &BrepFacts, brep: Brep) -> CadIr {
    let mut ir = CadIr::empty(Units::default());
    let (source, tolerances) = source_and_tolerances(scan, active);
    ir.source = Some(source);
    ir.tolerances = tolerances;

    ir.bodies = brep.bodies;
    ir.lumps = brep.lumps;
    ir.shells = brep.shells;
    ir.faces = brep.faces;
    ir.loops = brep.loops;
    ir.coedges = brep.coedges;
    ir.edges = brep.edges;
    ir.vertices = brep.vertices;
    ir.points = brep.points;
    ir.surfaces = brep.surfaces;
    ir.curves = brep.curves;
    ir.pcurves = brep.pcurves;
    ir.procedural_surfaces = brep.procedural_surfaces;
    ir.procedural_curves = brep.procedural_curves;
    ir.surface_parameterizations = brep.surface_parameterizations;
    ir.sketch_curve_links = brep.sketch_curve_links;
    ir.persistent_design_links = brep.persistent_design_links;
    ir.attributes = brep.attributes;
    ir.unknowns = brep.unknowns;
    ir
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
        if let (Some(resabs), Some(resnor)) = (h.resabs, h.resnor) {
            tolerances = Tolerances { resabs, resnor };
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
fn build_geometry_report(scan: &ContainerScan, decoded: &Brep) -> DecodeReport {
    let s = &decoded.stats;
    let mut losses = Vec::new();

    if s.nurbs_surfaces > 0 {
        losses.push(LossNote {
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
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "{} procedural curve record(s) were decoded into NURBS carriers from their inline \
                 cached 3D B-spline block.",
                s.nurbs_curves
            ),
            provenance: None,
        });
    }
    if s.unknown_surface_faces > 0 {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{} face(s) rest on spline/procedural surfaces whose shape was not decoded into a \
                 typed carrier (no inline cached B-spline block — the cache is reached through a \
                 subtype reference, or the record is a procedural form this codec does not \
                 evaluate); the face, its loops, and trims are emitted with an unknown-geometry \
                 surface linking to the preserved record bytes. Topology is transferred; the \
                 underlying surface shape is not.",
                s.unknown_surface_faces
            ),
            provenance: None,
        });
    }
    if s.procedural_curve_edges > 0 {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{} edge(s) reference a procedural intcurve/spline 3D curve with no decodable inline \
                 B-spline cache; the edge was emitted with its vertices and parameter range but no \
                 attributed curve carrier.",
                s.procedural_curve_edges
            ),
            provenance: None,
        });
    }
    if s.undecoded_pcurve_refs > 0 {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{} coedge(s) carry an explicit UV pcurve reference whose carrier could not be \
                 decoded; those coedges were emitted without a pcurve.",
                s.undecoded_pcurve_refs
            ),
            provenance: None,
        });
    }
    if s.partial_procedural_supports > 0 {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{} rolling-ball blend definition(s) retain their signed radius and solved cache, but only one of two native supports resolved.",
                s.partial_procedural_supports
            ),
            provenance: None,
        });
    }
    if s.other_records > 0 {
        losses.push(LossNote {
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
        losses,
        notes: container::summarize(scan)
            .notes
            .into_iter()
            .filter(|note| !note.starts_with("container-level inspection only"))
            .collect(),
    }
}

fn build_metadata_ir(scan: &ContainerScan) -> CadIr {
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
            if let (Some(resabs), Some(resnor)) = (h.resabs, h.resnor) {
                ir.tolerances = Tolerances { resabs, resnor };
            }
        }

        ir.unknowns.push(UnknownRecord {
            id: UnknownId(format!("f3d:{}", brep.name)),
            offset: 0,
            byte_len: brep.uncompressed_len,
            sha256: brep.sha256.clone(),
            data: None,
            links: Vec::new(),
            meta: EntityMeta {
                provenance: Provenance {
                    format: "f3d".to_string(),
                    stream: brep.name.clone(),
                    offset: 0,
                    tag: Some("asm_brep_stream".to_string()),
                },
                exactness: Exactness::Unknown,
            },
        });
    }

    ir.source = Some(SourceMeta {
        format: "f3d".to_string(),
        attributes,
    });
    ir
}

fn build_container_report(scan: &ContainerScan, container_only: bool) -> DecodeReport {
    let summary = container::summarize(scan);
    let brep_count = scan.breps.len();

    let mut losses = vec![
        LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Blocking,
            message: format!(
                "ASM BREP geometry was not transferred: the active stream is not a decodable \
                 BinaryFile8 SAB (or its framing failed). {brep_count} BREP stream(s) were located \
                 and their headers read, but no surfaces, curves, or points were produced."
            ),
            provenance: None,
        },
        LossNote {
            category: LossCategory::Topology,
            severity: Severity::Blocking,
            message: "B-rep topology graph (body/lump/shell/face/loop/coedge/edge/vertex) was not \
                      built for this stream."
                .to_string(),
            provenance: None,
        },
        LossNote {
            category: LossCategory::Material,
            severity: Severity::Warning,
            message: "Materials/appearances (.protein assets, ACT/design assignments) were not \
                      transferred."
                .to_string(),
            provenance: None,
        },
    ];

    if container::select_active_brep(scan).is_none() {
        losses.push(LossNote {
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
        losses,
        notes: summary.notes,
    }
}
