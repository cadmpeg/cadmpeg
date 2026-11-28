// SPDX-License-Identifier: Apache-2.0
//! Edit-and-patch engine: diff a neutral `CadIr` against a decoded baseline and
//! patch a retained F3D archive in place.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Write};

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};

use crate::decode;
use crate::loss::F3dLossCode;
use crate::writer::primitives::{f3d_native, validate_assembly_projection};
use cadmpeg_asm::nurbs::reader::LEN_TO_MM;
use cadmpeg_ir::codec::write::saved::adopt_configurations;
use cadmpeg_ir::report::loss::LossNote;
pub(crate) mod edits;
mod geometry;
pub(crate) mod records;
mod user_knots;
use edits::{
    validate_act_appearance_bindings, validate_act_entity_edits, validate_act_guid_edits,
    validate_act_link_edits, validate_act_registry_channel_edits, validate_body_color_edits,
    validate_body_member_edits, validate_body_native_key_edits, validate_body_transform_edits,
    validate_body_visibility_edits, validate_coedge_sense_edits, validate_configuration_edits,
    validate_construction_recipe_edits, validate_creation_timestamp_edits, validate_curve_edits,
    validate_design_type_edits, validate_edge_continuity_edits, validate_edge_ownership_edits,
    validate_edge_range_edits, validate_entity_header_edits, validate_face_color_edits,
    validate_face_sense_edits, validate_face_sidedness_edits, validate_history_state_edits,
    validate_lost_edge_edits, validate_material_assignment_appearances,
    validate_material_assignment_edits, validate_pcurve_edits, validate_persistent_reference_edits,
    validate_procedural_curve_edits, validate_procedural_surface_edits,
    validate_procedural_surface_fit_edits, validate_sketch_curve_edits,
    validate_sketch_point_edits, validate_sketch_relation_edits, validate_surface_edits,
    validate_tolerant_coedge_edits, validate_tolerant_edge_edits, validate_tolerant_vertex_edits,
    validate_transform_hint_edits, validate_vertex_ownership_edits, validate_wire_topology_edits,
    NurbsCurveEdit, NurbsSurfaceEdit, PatchNatives,
};
use geometry::patch_geometry;
use records::{
    patch_act_entities, patch_act_guids, patch_act_links, patch_body_members,
    patch_body_native_keys, patch_body_visibilities, patch_construction_recipes,
    patch_design_body_keys, patch_design_types, patch_edge_ownerships, patch_entity_headers,
    patch_history_states, patch_lost_edge_references, patch_material_assignments,
    patch_persistent_references, patch_sketch_curves, patch_sketch_points, patch_sketch_relations,
    patch_tolerant_coedge_parameters, patch_transform_hints, patch_wire_topologies,
};

/// Apply supported semantic edits to a retained F3D archive.
///
/// `source_image` must match the F3D source represented by `target`. The
/// function validates changed topology, geometry, design, sketch, history, and
/// appearance fields before patching records. Unsupported edits return
/// [`CodecError::NotImplemented`]. The configuration tables are written as
/// the configurations state them, and `losses` takes what they state
/// otherwise.
///
/// Completeness of the patch is checked by rebuilding the document from the
/// baseline plus the supported edits and comparing its machine-local content
/// digest to the target's. Both sides are computed here, in this process, from
/// this binary, which is the only setting in which
/// [`decode::document_local_sha256`] means anything; the comparison is bitwise
/// because the question is whether an unsupported field moved at all.
///
/// `restored` is the source image as the restore of a write read from CADIR
/// decoded it in `ctx`; the patch decodes the image itself without one.
pub(crate) fn write_semantic(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    target: &CadIr,
    source_image: &[u8],
    restored: Option<&super::RetainedArchive<'_>>,
    writer: &mut dyn Write,
    notes: &mut Vec<String>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    if !target.model.omitted_arenas_held() {
        return Err(CodecError::NotImplemented(
            "the document is edited since decode and was read from CADIR, which does not carry \
             the feature, parameter, configuration, and joint arenas the F3D patch compares \
             edits against; write it from the decoded source file"
                .into(),
        ));
    }
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let opened;
    let source = match restored {
        Some(source) => source,
        None => {
            opened = ctx.with_arena(&arena, |ctx| {
                super::RetainedArchive::read(ctx, source_image)
            })?;
            &opened
        }
    };
    let baseline = &source.decoded;
    let mut table_losses = Vec::new();
    if baseline
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::DesignIntentUndecoded.kind())
    {
        return Err(CodecError::NotImplemented(
            "F3D semantic patching refuses a source whose design intent was not decoded".into(),
        ));
    }
    let baseline_native = f3d_native(ctx, baseline.ir())?;
    let target_native = f3d_native(ctx, target)?;
    let mut tables = target_native
        .as_ref()
        .map(|native| crate::writer::generate::configuration_table::patched(target, native))
        .transpose()?;
    if let Some(tables) = &mut tables {
        table_losses.append(&mut tables.losses);
    }
    validate_assembly_projection(ctx, target, target_native.as_ref())?;
    let natives = PatchNatives {
        baseline: baseline_native.as_ref(),
        target: target_native.as_ref(),
    };
    let baseline_point_ids = baseline
        .ir()
        .geometry
        .points
        .iter()
        .map(|point| point.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let target_point_ids = target
        .geometry
        .points
        .iter()
        .map(|point| point.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if baseline_point_ids != target_point_ids {
        return Err(CodecError::NotImplemented(
            "F3D point regeneration requires the unchanged point-id set and finite coordinates"
                .into(),
        ));
    }
    let edited_curves =
        validate_curve_edits(&baseline.ir().geometry.curves, &target.geometry.curves)?;
    let nurbs_curve_edits = target
        .geometry
        .curves
        .iter()
        .filter_map(|curve| match curve.geometry.solved() {
            Some(SolvedCurveGeometry::Nurbs(nurbs))
                if edited_curves.contains(curve.id.as_str()) =>
            {
                let before = baseline
                    .ir()
                    .geometry
                    .curves
                    .iter()
                    .find(|before| before.id == curve.id)?;
                let Some(SolvedCurveGeometry::Nurbs(before)) = before.geometry.solved() else {
                    return None;
                };
                let mut nurbs = nurbs.clone();
                if let Some(shift) = native_subset_shift(target, &curve.id) {
                    nurbs
                        .edit_knots(|knots| knots.iter_mut().for_each(|knot| *knot += shift))
                        .ok()?;
                }
                Some((
                    curve.id.as_str().to_owned(),
                    NurbsCurveEdit {
                        curve: nurbs.clone(),
                        periodic: (before.periodic() != nurbs.periodic())
                            .then_some(nurbs.periodic()),
                    },
                ))
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let pcurve_edits = validate_pcurve_edits(&baseline.ir().geometry, &target.geometry)?;
    let edited_surfaces =
        validate_surface_edits(&baseline.ir().geometry.surfaces, &target.geometry.surfaces)?;
    let nurbs_surface_edits = target
        .geometry
        .surfaces
        .iter()
        .filter_map(|surface| match surface.geometry.solved() {
            Some(SolvedSurfaceGeometry::Nurbs(nurbs))
                if edited_surfaces.contains(surface.id.as_str()) =>
            {
                let before = baseline
                    .ir()
                    .geometry
                    .surfaces
                    .iter()
                    .find(|before| before.id == surface.id)?;
                let Some(SolvedSurfaceGeometry::Nurbs(before)) = before.geometry.solved() else {
                    return None;
                };
                Some((
                    surface.id.as_str().to_owned(),
                    NurbsSurfaceEdit {
                        surface: nurbs.clone(),
                        periodic: (before.u_periodic() != nurbs.u_periodic()
                            || before.v_periodic() != nurbs.v_periodic())
                        .then_some([nurbs.u_periodic(), nurbs.v_periodic()]),
                    },
                ))
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let extrusion_direction_edits = validate_procedural_surface_edits(baseline.ir(), target)?;
    let procedural_surface_fit_edits =
        validate_procedural_surface_fit_edits(baseline.ir(), target)?;
    let procedural_curve_edits = validate_procedural_curve_edits(
        &baseline.ir().geometry.procedural_curves,
        &target.geometry.procedural_curves,
    )?;
    let sketch_point_edits = validate_sketch_point_edits(natives)?;
    let sketch_curve_edits = validate_sketch_curve_edits(natives)?;
    let sketch_relation_edits = validate_sketch_relation_edits(natives)?;
    let persistent_reference_edits = validate_persistent_reference_edits(natives)?;
    let construction_recipe_edits = validate_construction_recipe_edits(natives)?;
    let body_member_edits = validate_body_member_edits(natives)?;
    let entity_header_edits = validate_entity_header_edits(natives)?;
    let design_type_edits = validate_design_type_edits(natives)?;
    let lost_edge_edits = validate_lost_edge_edits(natives)?;
    let material_assignment_edits = validate_material_assignment_edits(natives)?;
    let protein_appearance_edits =
        validate_material_assignment_appearances(natives, baseline.ir(), target)?;
    let mut act_guid_edits = validate_act_guid_edits(natives)?;
    for (stream, edits) in validate_act_registry_channel_edits(natives)? {
        act_guid_edits.entry(stream).or_default().extend(edits);
    }
    let act_link_edits = validate_act_link_edits(natives)?;
    let act_entity_edits = validate_act_entity_edits(natives)?;
    let baseline_act_table_references = natives
        .baseline
        .map(|native| &native.act_table_references[..])
        .unwrap_or_default();
    let target_act_table_references = natives
        .target
        .map(|native| &native.act_table_references[..])
        .unwrap_or_default();
    if baseline_act_table_references != target_act_table_references {
        return Err(CodecError::NotImplemented(
            "F3D ACT table-reference edits require regeneration of the complete table graph".into(),
        ));
    }
    let configuration_edits = validate_configuration_edits(
        baseline_native
            .as_ref()
            .map_or(&[][..], |native| native.design_configurations.as_slice()),
        tables
            .as_ref()
            .map_or(&[][..], |tables| tables.destination.documents.as_ref()),
    )?;
    validate_act_appearance_bindings(natives, baseline.ir(), target)?;
    let body_transform_edits =
        validate_body_transform_edits(&baseline.ir().geometry.bodies, &target.geometry.bodies)?;
    let body_visibility_edits = validate_body_visibility_edits(natives, baseline.ir(), target)?;
    let body_native_key_edits = validate_body_native_key_edits(natives)?;
    let transform_hint_edits = validate_transform_hint_edits(natives)?;
    let mut entity_color_edits =
        validate_body_color_edits(&baseline.ir().geometry.bodies, &target.geometry.bodies)?;
    entity_color_edits.extend(validate_face_color_edits(
        &baseline.ir().geometry.faces,
        &target.geometry.faces,
    )?);
    let mut edge_range_edits =
        validate_edge_range_edits(&baseline.ir().geometry.edges, &target.geometry.edges)?;
    // IR line-edge parameters are millimeter arc lengths; the native stream
    // stores centimeters. Conic parameters are angles in both.
    for (edge_id, range) in &mut edge_range_edits {
        let is_line = target
            .geometry
            .edges
            .iter()
            .find(|edge| edge.id.as_str() == edge_id)
            .and_then(|edge| edge.curve())
            .is_some_and(|curve_id| {
                target.geometry.curves.iter().any(|curve| {
                    curve.id == *curve_id
                        && matches!(
                            curve.geometry,
                            CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
                        )
                })
            });
        if is_line {
            range[0] /= LEN_TO_MM;
            range[1] /= LEN_TO_MM;
        }
        if let Some(shift) = target
            .geometry
            .edges
            .iter()
            .find(|edge| edge.id.as_str() == edge_id)
            .and_then(|edge| edge.curve())
            .and_then(|curve| native_subset_shift(target, curve))
        {
            range[0] += shift;
            range[1] += shift;
        }
    }
    let face_sense_edits = validate_face_sense_edits(natives, baseline.ir(), target)?;
    let coedge_sense_edits =
        validate_coedge_sense_edits(&baseline.ir().geometry.coedges, &target.geometry.coedges)?;
    let history_state_edits = validate_history_state_edits(natives)?;
    let creation_timestamp_edits = validate_creation_timestamp_edits(natives)?;
    let edge_continuity_edits = validate_edge_continuity_edits(natives)?;
    let edge_ownership_edits = validate_edge_ownership_edits(natives, target)?;
    let vertex_ownership_edits = validate_vertex_ownership_edits(natives, target)?;
    let face_sidedness_edits = validate_face_sidedness_edits(natives)?;
    let tolerant_edge_edits = validate_tolerant_edge_edits(natives, baseline.ir(), target)?;
    let tolerant_vertex_edits = validate_tolerant_vertex_edits(natives, baseline.ir(), target)?;
    let tolerant_coedge_edits = validate_tolerant_coedge_edits(natives, baseline.ir(), target)?;
    let wire_topology_edits = validate_wire_topology_edits(natives)?;
    let mut user_knot_edits = user_knots::edits(baseline.ir(), target)?;
    let mut supported_target = baseline.ir().clone();
    supported_target
        .geometry
        .subds
        .clone_from(&target.geometry.subds);
    supported_target
        .geometry
        .points
        .clone_from(&target.geometry.points);
    supported_target
        .geometry
        .curves
        .clone_from(&target.geometry.curves);
    supported_target
        .geometry
        .surfaces
        .clone_from(&target.geometry.surfaces);
    supported_target
        .geometry
        .pcurves
        .clone_from(&target.geometry.pcurves);
    for body in &mut supported_target.geometry.bodies {
        if let Some(candidate) = target
            .geometry
            .bodies
            .iter()
            .find(|candidate| candidate.id == body.id)
        {
            body.transform = candidate.transform;
            body.color = candidate.color;
            body.visible = candidate.visible;
        }
    }
    supported_target
        .geometry
        .edges
        .clone_from(&target.geometry.edges);
    supported_target
        .geometry
        .vertices
        .clone_from(&target.geometry.vertices);
    supported_target
        .geometry
        .faces
        .clone_from(&target.geometry.faces);
    supported_target
        .geometry
        .coedges
        .clone_from(&target.geometry.coedges);
    supported_target
        .model
        .appearance_bindings
        .clone_from(&target.model.appearance_bindings);
    supported_target
        .model
        .appearances
        .clone_from(&target.model.appearances);
    supported_target
        .geometry
        .procedural_surfaces
        .clone_from(&target.geometry.procedural_surfaces);
    supported_target
        .geometry
        .procedural_curves
        .clone_from(&target.geometry.procedural_curves);
    supported_target
        .model
        .configurations
        .clone_from(&target.model.configurations);
    adopt_configurations(&mut supported_target.design, &target.design);
    if let (Some(mut supported), Some(target_native)) =
        (baseline_native.clone(), target_native.as_ref())
    {
        supported
            .body_native_keys
            .clone_from(&target_native.body_native_keys);
        supported
            .sketch_points
            .clone_from(&target_native.sketch_points);
        supported
            .sketch_curve_identities
            .clone_from(&target_native.sketch_curve_identities);
        supported
            .sketch_relations
            .clone_from(&target_native.sketch_relations);
        supported
            .persistent_references
            .clone_from(&target_native.persistent_references);
        supported
            .construction_recipes
            .clone_from(&target_native.construction_recipes);
        supported
            .design_body_members
            .clone_from(&target_native.design_body_members);
        supported
            .design_configurations
            .clone_from(&target_native.design_configurations);
        supported
            .design_entity_headers
            .clone_from(&target_native.design_entity_headers);
        supported
            .design_types
            .clone_from(&target_native.design_types);
        supported
            .lost_edge_references
            .clone_from(&target_native.lost_edge_references);
        supported
            .design_material_assignments
            .clone_from(&target_native.design_material_assignments);
        supported.act_guids.clone_from(&target_native.act_guids);
        supported
            .act_registry_channels
            .clone_from(&target_native.act_registry_channels);
        supported
            .act_component_links
            .clone_from(&target_native.act_component_links);
        supported
            .act_entities
            .clone_from(&target_native.act_entities);
        supported
            .asm_histories
            .clone_from(&target_native.asm_histories);
        supported
            .creation_timestamps
            .clone_from(&target_native.creation_timestamps);
        supported
            .edge_continuities
            .clone_from(&target_native.edge_continuities);
        supported
            .edge_ownerships
            .clone_from(&target_native.edge_ownerships);
        supported
            .vertex_ownerships
            .clone_from(&target_native.vertex_ownerships);
        supported
            .face_sidedness
            .clone_from(&target_native.face_sidedness);
        supported
            .tolerant_coedge_parameters
            .clone_from(&target_native.tolerant_coedge_parameters);
        supported
            .tolerant_edge_tails
            .clone_from(&target_native.tolerant_edge_tails);
        supported
            .tolerant_vertex_tails
            .clone_from(&target_native.tolerant_vertex_tails);
        supported
            .transform_hints
            .clone_from(&target_native.transform_hints);
        supported
            .wire_topologies
            .clone_from(&target_native.wire_topologies);
        supported.store(
            ctx,
            supported_target
                .native
                .namespace_mut(cadmpeg_ir::codec_id!("f3d")),
        )?;
    }
    supported_target
        .model
        .adopt_omitted_arenas_baseline(&target.model);
    if decode::document_local_sha256(ctx, &supported_target)?
        != decode::document_local_sha256(ctx, target)?
    {
        return Err(CodecError::NotImplemented(
            "modified F3D IR contains edits beyond supported point, line, and plane carriers"
                .into(),
        ));
    }

    let active_brep = baseline
        .ir()
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("active_brep"))
        .ok_or_else(|| CodecError::Malformed("F3D baseline has no active BREP".into()))?;
    let positions = target
        .geometry
        .points
        .iter()
        .map(|point| (point.id.as_str().to_owned(), point.position().get()))
        .collect::<BTreeMap<_, _>>();
    let lines = target
        .geometry
        .curves
        .iter()
        .filter_map(|curve| match curve.geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
                let origin = line_curve.origin().get();
                let direction = *line_curve.direction().as_raw();
                edited_curves
                    .contains(curve.id.as_str())
                    .then(|| (curve.id.as_str().to_owned(), (origin, direction)))
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let conics = target
        .geometry
        .curves
        .iter()
        .filter_map(|curve| match curve.geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
                let center = circle_curve.center().get();
                let axis = *circle_curve.frame().axis().as_raw();
                let ref_direction = *circle_curve.frame().reference().as_raw();
                let radius = circle_curve.radius().get();
                edited_curves.contains(curve.id.as_str()).then(|| {
                    (
                        curve.id.as_str().to_owned(),
                        (center, axis, ref_direction, radius, radius),
                    )
                })
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
                let center = ellipse_curve.center().get();
                let axis = *ellipse_curve.frame().axis().as_raw();
                let major_direction = *ellipse_curve.frame().reference().as_raw();
                let major_radius = ellipse_curve.major_radius().get();
                let minor_radius = ellipse_curve.minor_radius().get();
                edited_curves.contains(curve.id.as_str()).then(|| {
                    (
                        curve.id.as_str().to_owned(),
                        (center, axis, major_direction, major_radius, minor_radius),
                    )
                })
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let degenerate_curves = target
        .geometry
        .curves
        .iter()
        .filter_map(|curve| match curve.geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(degenerate_curve)) => {
                let point = degenerate_curve.point().get();
                edited_curves
                    .contains(curve.id.as_str())
                    .then(|| (curve.id.as_str().to_owned(), point))
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let planes = target
        .geometry
        .surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
                let origin = plane_surface.origin().get();
                let normal = *plane_surface.frame().axis().as_raw();
                let u_axis = *plane_surface.frame().reference().as_raw();
                edited_surfaces
                    .contains(surface.id.as_str())
                    .then(|| (surface.id.as_str().to_owned(), (origin, normal, u_axis)))
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let spheres = target
        .geometry
        .surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
                let center = sphere_surface.center().get();
                let axis = *sphere_surface.frame().axis().as_raw();
                let ref_direction = *sphere_surface.frame().reference().as_raw();
                let radius = sphere_surface.radius().get();
                edited_surfaces.contains(surface.id.as_str()).then(|| {
                    (
                        surface.id.as_str().to_owned(),
                        (center, axis, ref_direction, radius),
                    )
                })
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let tori = target
        .geometry
        .surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
                let center = torus_surface.center().get();
                let axis = *torus_surface.frame().axis().as_raw();
                let ref_direction = *torus_surface.frame().reference().as_raw();
                let major_radius = torus_surface.major_radius().get();
                let minor_radius = torus_surface.minor_radius().get();
                edited_surfaces.contains(surface.id.as_str()).then(|| {
                    (
                        surface.id.as_str().to_owned(),
                        (center, axis, ref_direction, major_radius, minor_radius),
                    )
                })
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let cones = target
        .geometry
        .surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
                let origin = cylinder_surface.origin().get();
                let axis = *cylinder_surface.frame().axis().as_raw();
                let ref_direction = *cylinder_surface.frame().reference().as_raw();
                let radius = cylinder_surface.radius().get();
                edited_surfaces.contains(surface.id.as_str()).then(|| {
                    (
                        surface.id.as_str().to_owned(),
                        (origin, axis, ref_direction, radius, 1.0, 0.0),
                    )
                })
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
                let origin = cone_surface.origin().get();
                let axis = *cone_surface.frame().axis().as_raw();
                let ref_direction = *cone_surface.frame().reference().as_raw();
                let radius = cone_surface.radius().get();
                let ratio = cone_surface.ratio().get();
                let half_angle = cone_surface.half_angle().get();
                edited_surfaces.contains(surface.id.as_str()).then(|| {
                    (
                        surface.id.as_str().to_owned(),
                        (origin, axis, ref_direction, radius, ratio, half_angle),
                    )
                })
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();

    let mut archive = zip::ZipArchive::new(Cursor::new(source_image)).map_err(|error| {
        CodecError::malformed(format_args!("retained F3D ZIP is invalid: {error}"))
    })?;
    let output = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(output);
    let mut patched_protein_appearances = BTreeSet::new();
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).map_err(|error| {
            CodecError::malformed(format_args!("invalid F3D ZIP entry: {error}"))
        })?;
        let name = entry
            .name()
            .map_err(|error| {
                CodecError::malformed(format_args!("invalid ZIP entry name: {error}"))
            })?
            .into_owned();
        let compression = entry.compression();
        let options = crate::zip_write::file_options(compression);
        let expanded = source
            .scan
            .rewrite_member(&name, &entry, ctx.per_expand_allowance())?;
        if entry.is_dir() {
            if !expanded.is_empty() {
                return Err(CodecError::NotImplemented(format!(
                    "ZIP directory {name} carries a payload"
                )));
            }
            if compression == zip::CompressionMethod::ZSTD {
                zip.raw_copy_file(entry).map_err(|error| {
                    CodecError::malformed(format_args!("cannot copy ZIP directory {name}: {error}"))
                })?;
            } else {
                zip.add_directory(name, options).map_err(|error| {
                    CodecError::malformed(format_args!("cannot write F3D directory: {error}"))
                })?;
            }
            continue;
        }
        drop(entry);
        let mut bytes = expanded.to_vec();
        if let Some(cage) = user_knot_edits.remove(&name) {
            bytes = crate::tsm::user_knots::rewrite(ctx, &name, &bytes, &cage)?;
        }
        if let Some(configuration) = configuration_edits.get(&name) {
            bytes.clone_from(configuration);
        }
        if name == *active_brep {
            patch_geometry(
                &mut bytes,
                &geometry::GeometryEdits {
                    positions: &positions,
                    lines: &lines,
                    conics: &conics,
                    degenerate_curves: &degenerate_curves,
                    planes: &planes,
                    spheres: &spheres,
                    tori: &tori,
                    cones: &cones,
                    body_transforms: &body_transform_edits,
                    entity_colors: &entity_color_edits,
                    edge_ranges: &edge_range_edits,
                    face_senses: &face_sense_edits,
                    coedge_senses: &coedge_sense_edits,
                    procedural_surface_edits: &extrusion_direction_edits,
                    nurbs_surfaces: &nurbs_surface_edits,
                    nurbs_curves: &nurbs_curve_edits,
                    pcurves: &pcurve_edits,
                    procedural_curve_edits: &procedural_curve_edits,
                    procedural_surface_fits: &procedural_surface_fit_edits,
                    creation_timestamps: &creation_timestamp_edits,
                    edge_continuities: &edge_continuity_edits,
                    vertex_ownerships: &vertex_ownership_edits,
                    face_sidedness: &face_sidedness_edits,
                    tolerant_edges: &tolerant_edge_edits,
                    tolerant_vertices: &tolerant_vertex_edits,
                },
            )?;
            patch_transform_hints(&mut bytes, &transform_hint_edits)?;
            patch_tolerant_coedge_parameters(&mut bytes, &tolerant_coedge_edits)?;
            patch_wire_topologies(&mut bytes, &wire_topology_edits)?;
            patch_edge_ownerships(&mut bytes, &edge_ownership_edits)?;
            patch_body_native_keys(&mut bytes, &body_native_key_edits.asm)?;
            if let Some(edits) = history_state_edits.get(&name) {
                patch_history_states(&mut bytes, edits)?;
            }
        } else {
            if name.ends_with(".protein") && !protein_appearance_edits.is_empty() {
                let protein = source.scan.protein_source(ctx, &name)?;
                let (patched_bytes, patched_guids) = crate::materials::patch_protein_source(
                    &protein,
                    &protein_appearance_edits,
                    notes,
                    ctx,
                )?;
                bytes = patched_bytes;
                patched_protein_appearances.extend(patched_guids);
            }
            if let Some(edits) = sketch_point_edits.get(&name) {
                patch_sketch_points(&mut bytes, edits)?;
            }
            if let Some(edits) = sketch_curve_edits.get(&name) {
                patch_sketch_curves(&mut bytes, edits)?;
            }
            if let Some(edits) = sketch_relation_edits.get(&name) {
                patch_sketch_relations(&mut bytes, edits)?;
            }
            if let Some(edits) = persistent_reference_edits.get(&name) {
                patch_persistent_references(&mut bytes, edits)?;
            }
            if let Some(edits) = construction_recipe_edits.get(&name) {
                patch_construction_recipes(&mut bytes, edits)?;
            }
            if let Some(edits) = body_member_edits.get(&name) {
                patch_body_members(&mut bytes, edits)?;
            }
            if let Some(edits) = body_visibility_edits.get(&name) {
                patch_body_visibilities(&mut bytes, edits)?;
            }
            if let Some(edits) = body_native_key_edits.design.get(&name) {
                patch_design_body_keys(&mut bytes, edits)?;
            }
            if let Some(edits) = entity_header_edits.get(&name) {
                patch_entity_headers(&mut bytes, edits)?;
            }
            if let Some(edits) = design_type_edits.get(&name) {
                patch_design_types(&mut bytes, edits)?;
            }
            if let Some(edits) = lost_edge_edits.get(&name) {
                patch_lost_edge_references(&mut bytes, edits)?;
            }
            if let Some(edits) = material_assignment_edits.get(&name) {
                patch_material_assignments(&mut bytes, edits)?;
            }
            if let Some(edits) = act_guid_edits.get(&name) {
                patch_act_guids(&mut bytes, edits)?;
            }
            if let Some(edits) = act_link_edits.get(&name) {
                patch_act_links(&mut bytes, edits)?;
            }
            if let Some(edits) = act_entity_edits.get(&name) {
                patch_act_entities(&mut bytes, edits)?;
            }
        }
        if compression == zip::CompressionMethod::ZSTD {
            crate::zip_write::write_zstd_member(
                &mut zip,
                &mut archive,
                index,
                expanded,
                &bytes,
                notes,
            )?;
            continue;
        }
        zip.start_file(name, options).map_err(|error| {
            CodecError::malformed(format_args!("cannot write F3D entry: {error}"))
        })?;
        zip.write_all(&bytes)?;
    }
    if !user_knot_edits.is_empty() {
        return Err(CodecError::NotImplemented(
            "one or more edited T-spline surfaces have no writable TSM member".into(),
        ));
    }
    if patched_protein_appearances.len() != protein_appearance_edits.len() {
        return Err(CodecError::NotImplemented(
            "one or more edited F3D appearances have no writable Protein carrier".into(),
        ));
    }
    let output = zip
        .finish()
        .map_err(|error| CodecError::malformed(format_args!("cannot finish F3D ZIP: {error}")))?
        .into_inner();
    if let Some(tables) = tables {
        tables.destination.read_back(
            ctx.policy(),
            &output,
            target,
            &target.model.parameters,
            target_native
                .as_ref()
                .map_or(&[], |native| native.design_parameter_scopes.as_slice()),
            &mut table_losses,
        );
    }
    writer.write_all(&output)?;
    losses.extend(table_losses);
    Ok(())
}

/// The shift from a subset curve's local parameter onto its ACIS parameter,
/// which is the parameter of the curve it trims, negated on a reversed carrier.
fn native_subset_shift(ir: &CadIr, curve: &cadmpeg_ir::ids::CurveId) -> Option<f64> {
    ir.geometry.procedural_curves.iter().find_map(|procedural| {
        let cadmpeg_ir::geometry::procedural_curves::ProceduralCurveDefinition::Subset(subset) =
            procedural.definition()
        else {
            return None;
        };
        (ir.geometry.procedural_curve_owner(&procedural.id) == Some(curve)).then(|| {
            let [first, last] = subset.parameter_range().endpoints();
            if *subset.sense() {
                first
            } else {
                -last
            }
        })
    })
}

#[cfg(test)]
mod tests;
