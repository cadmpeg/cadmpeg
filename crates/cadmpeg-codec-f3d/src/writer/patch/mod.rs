// SPDX-License-Identifier: Apache-2.0
//! Edit-and-patch engine: diff a neutral `CadIr` against a decoded baseline and
//! patch a retained F3D archive in place.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Write};

use cadmpeg_ir::codec::{Codec, CodecError, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use zip::write::SimpleFileOptions;

use crate::nurbs::reader::LEN_TO_MM;
use crate::writer::primitives::{f3d_native, validate_configuration_projection};
use crate::{decode, F3dCodec};
pub(crate) mod edits;
pub(crate) mod geometry;
pub(crate) mod records;
use edits::{
    validate_act_appearance_bindings, validate_act_entity_edits, validate_act_guid_edits,
    validate_act_root_edits, validate_body_color_edits, validate_body_member_edits,
    validate_body_native_key_edits, validate_body_transform_edits, validate_body_visibility_edits,
    validate_coedge_sense_edits, validate_configuration_edits, validate_construction_recipe_edits,
    validate_creation_timestamp_edits, validate_curve_edits, validate_design_object_edits,
    validate_edge_continuity_edits, validate_edge_ownership_edits, validate_edge_range_edits,
    validate_entity_header_edits, validate_face_color_edits, validate_face_sense_edits,
    validate_face_sidedness_edits, validate_history_state_edits, validate_lost_edge_edits,
    validate_material_assignment_appearances, validate_material_assignment_edits,
    validate_pcurve_edits, validate_persistent_reference_edits, validate_procedural_curve_edits,
    validate_procedural_surface_edits, validate_procedural_surface_fit_edits,
    validate_sketch_curve_edits, validate_sketch_point_edits, validate_sketch_relation_edits,
    validate_surface_edits, validate_tolerant_coedge_edits, validate_tolerant_edge_edits,
    validate_tolerant_vertex_edits, validate_transform_hint_edits, validate_vertex_ownership_edits,
    validate_wire_topology_edits, NurbsCurveEdit, NurbsSurfaceEdit,
};
use geometry::patch_geometry;
use records::{
    patch_act_entities, patch_act_guids, patch_act_roots, patch_body_members,
    patch_body_native_keys, patch_body_visibilities, patch_construction_recipes,
    patch_design_body_keys, patch_design_objects, patch_edge_ownerships, patch_entity_headers,
    patch_history_states, patch_lost_edge_references, patch_material_assignments,
    patch_persistent_references, patch_sketch_curves, patch_sketch_points, patch_sketch_relations,
    patch_tolerant_coedge_parameters, patch_transform_hints, patch_wire_topologies,
};

/// Apply supported semantic edits to a retained F3D archive.
///
/// `source_image` must match the F3D source represented by `target`. The
/// function validates changed topology, geometry, design, sketch, history, and
/// appearance fields before patching records. Unsupported edits return
/// [`CodecError::NotImplemented`].
pub fn write_semantic(
    target: &CadIr,
    source_image: &[u8],
    writer: &mut dyn Write,
) -> Result<(), CodecError> {
    if let Some(native) = f3d_native(target)? {
        validate_configuration_projection(target, &native)?;
    }
    let baseline = F3dCodec.decode(&mut Cursor::new(source_image), &DecodeOptions::default())?;
    let baseline_point_ids = baseline
        .ir
        .model
        .points
        .iter()
        .map(|point| point.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let target_point_ids = target
        .model
        .points
        .iter()
        .map(|point| point.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if baseline_point_ids != target_point_ids
        || target.model.points.iter().any(|point| {
            !point.position.x.is_finite()
                || !point.position.y.is_finite()
                || !point.position.z.is_finite()
        })
    {
        return Err(CodecError::NotImplemented(
            "F3D point regeneration requires the unchanged point-id set and finite coordinates"
                .into(),
        ));
    }
    let edited_curves = validate_curve_edits(&baseline.ir.model.curves, &target.model.curves)?;
    let nurbs_curve_edits = target
        .model
        .curves
        .iter()
        .filter_map(|curve| match &curve.geometry {
            CurveGeometry::Nurbs(nurbs) if edited_curves.contains(curve.id.as_str()) => {
                let before = baseline
                    .ir
                    .model
                    .curves
                    .iter()
                    .find(|before| before.id == curve.id)?;
                let CurveGeometry::Nurbs(before) = &before.geometry else {
                    return None;
                };
                Some((
                    curve.id.0.clone(),
                    NurbsCurveEdit {
                        curve: nurbs.clone(),
                        periodic: (before.periodic != nurbs.periodic).then_some(nurbs.periodic),
                    },
                ))
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let pcurve_edits = validate_pcurve_edits(&baseline.ir.model.pcurves, &target.model.pcurves)?;
    let edited_surfaces =
        validate_surface_edits(&baseline.ir.model.surfaces, &target.model.surfaces)?;
    let nurbs_surface_edits = target
        .model
        .surfaces
        .iter()
        .filter_map(|surface| match &surface.geometry {
            SurfaceGeometry::Nurbs(nurbs) if edited_surfaces.contains(surface.id.as_str()) => {
                let before = baseline
                    .ir
                    .model
                    .surfaces
                    .iter()
                    .find(|before| before.id == surface.id)?;
                let SurfaceGeometry::Nurbs(before) = &before.geometry else {
                    return None;
                };
                Some((
                    surface.id.0.clone(),
                    NurbsSurfaceEdit {
                        surface: nurbs.clone(),
                        periodic: (before.u_periodic != nurbs.u_periodic
                            || before.v_periodic != nurbs.v_periodic)
                            .then_some([nurbs.u_periodic, nurbs.v_periodic]),
                    },
                ))
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let extrusion_direction_edits = validate_procedural_surface_edits(&baseline.ir, target)?;
    let procedural_surface_fit_edits = validate_procedural_surface_fit_edits(&baseline.ir, target)?;
    let procedural_curve_edits = validate_procedural_curve_edits(
        &baseline.ir.model.procedural_curves,
        &target.model.procedural_curves,
    )?;
    let sketch_point_edits = validate_sketch_point_edits(&baseline.ir, target)?;
    let sketch_curve_edits = validate_sketch_curve_edits(&baseline.ir, target)?;
    let sketch_relation_edits = validate_sketch_relation_edits(&baseline.ir, target)?;
    let persistent_reference_edits = validate_persistent_reference_edits(&baseline.ir, target)?;
    let construction_recipe_edits = validate_construction_recipe_edits(&baseline.ir, target)?;
    let body_member_edits = validate_body_member_edits(&baseline.ir, target)?;
    let entity_header_edits = validate_entity_header_edits(&baseline.ir, target)?;
    let design_object_edits = validate_design_object_edits(&baseline.ir, target)?;
    let lost_edge_edits = validate_lost_edge_edits(&baseline.ir, target)?;
    let material_assignment_edits = validate_material_assignment_edits(&baseline.ir, target)?;
    let protein_appearance_edits = validate_material_assignment_appearances(&baseline.ir, target)?;
    let act_guid_edits = validate_act_guid_edits(&baseline.ir, target)?;
    let act_root_edits = validate_act_root_edits(&baseline.ir, target)?;
    let act_entity_edits = validate_act_entity_edits(&baseline.ir, target)?;
    let configuration_edits = validate_configuration_edits(&baseline.ir, target)?;
    validate_act_appearance_bindings(&baseline.ir, target)?;
    let body_transform_edits =
        validate_body_transform_edits(&baseline.ir.model.bodies, &target.model.bodies)?;
    let body_visibility_edits = validate_body_visibility_edits(&baseline.ir, target)?;
    let body_native_key_edits = validate_body_native_key_edits(&baseline.ir, target)?;
    let transform_hint_edits = validate_transform_hint_edits(&baseline.ir, target)?;
    let mut entity_color_edits =
        validate_body_color_edits(&baseline.ir.model.bodies, &target.model.bodies)?;
    entity_color_edits.extend(validate_face_color_edits(
        &baseline.ir.model.faces,
        &target.model.faces,
    )?);
    let mut edge_range_edits =
        validate_edge_range_edits(&baseline.ir.model.edges, &target.model.edges)?;
    // IR line-edge parameters are millimeter arc lengths; the native stream
    // stores centimeters. Conic parameters are angles in both.
    for (edge_id, range) in &mut edge_range_edits {
        let is_line = target
            .model
            .edges
            .iter()
            .find(|edge| edge.id.as_str() == edge_id)
            .and_then(|edge| edge.curve.as_ref())
            .is_some_and(|curve_id| {
                target.model.curves.iter().any(|curve| {
                    curve.id == *curve_id && matches!(curve.geometry, CurveGeometry::Line { .. })
                })
            });
        if is_line {
            range[0] /= LEN_TO_MM;
            range[1] /= LEN_TO_MM;
        }
    }
    let face_sense_edits = validate_face_sense_edits(&baseline.ir, target)?;
    let coedge_sense_edits =
        validate_coedge_sense_edits(&baseline.ir.model.coedges, &target.model.coedges)?;
    let history_state_edits = validate_history_state_edits(&baseline.ir, target)?;
    let creation_timestamp_edits = validate_creation_timestamp_edits(&baseline.ir, target)?;
    let edge_continuity_edits = validate_edge_continuity_edits(&baseline.ir, target)?;
    let edge_ownership_edits = validate_edge_ownership_edits(&baseline.ir, target)?;
    let vertex_ownership_edits = validate_vertex_ownership_edits(&baseline.ir, target)?;
    let face_sidedness_edits = validate_face_sidedness_edits(&baseline.ir, target)?;
    let tolerant_edge_edits = validate_tolerant_edge_edits(&baseline.ir, target)?;
    let tolerant_vertex_edits = validate_tolerant_vertex_edits(&baseline.ir, target)?;
    let tolerant_coedge_edits = validate_tolerant_coedge_edits(&baseline.ir, target)?;
    let wire_topology_edits = validate_wire_topology_edits(&baseline.ir, target)?;
    let mut supported_target = baseline.ir.clone();
    supported_target
        .model
        .points
        .clone_from(&target.model.points);
    supported_target
        .model
        .curves
        .clone_from(&target.model.curves);
    supported_target
        .model
        .surfaces
        .clone_from(&target.model.surfaces);
    supported_target
        .model
        .pcurves
        .clone_from(&target.model.pcurves);
    for body in &mut supported_target.model.bodies {
        if let Some(candidate) = target
            .model
            .bodies
            .iter()
            .find(|candidate| candidate.id == body.id)
        {
            body.transform = candidate.transform;
            body.color = candidate.color;
            body.visible = candidate.visible;
        }
    }
    supported_target.model.edges.clone_from(&target.model.edges);
    supported_target
        .model
        .vertices
        .clone_from(&target.model.vertices);
    supported_target.model.faces.clone_from(&target.model.faces);
    supported_target
        .model
        .coedges
        .clone_from(&target.model.coedges);
    supported_target
        .model
        .appearance_bindings
        .clone_from(&target.model.appearance_bindings);
    supported_target
        .model
        .appearances
        .clone_from(&target.model.appearances);
    supported_target
        .model
        .procedural_surfaces
        .clone_from(&target.model.procedural_surfaces);
    supported_target
        .model
        .procedural_curves
        .clone_from(&target.model.procedural_curves);
    supported_target
        .model
        .configurations
        .clone_from(&target.model.configurations);
    if let (Some(mut supported), Some(target_native)) =
        (f3d_native(&supported_target)?, f3d_native(target)?)
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
            .design_objects
            .clone_from(&target_native.design_objects);
        supported
            .lost_edge_references
            .clone_from(&target_native.lost_edge_references);
        supported
            .design_material_assignments
            .clone_from(&target_native.design_material_assignments);
        supported.act_guids.clone_from(&target_native.act_guids);
        supported
            .act_root_components
            .clone_from(&target_native.act_root_components);
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
        supported.store(supported_target.native.namespace_mut("f3d"))?;
    }
    if decode::semantic_hash(&supported_target) != decode::semantic_hash(target) {
        return Err(CodecError::NotImplemented(
            "modified F3D IR contains edits beyond supported point, line, and plane carriers"
                .into(),
        ));
    }

    let active_brep = baseline
        .ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("active_brep"))
        .ok_or_else(|| CodecError::Malformed("F3D baseline has no active BREP".into()))?;
    let positions = target
        .model
        .points
        .iter()
        .map(|point| (point.id.0.clone(), point.position))
        .collect::<BTreeMap<_, _>>();
    let lines = target
        .model
        .curves
        .iter()
        .filter_map(|curve| match curve.geometry {
            CurveGeometry::Line { origin, direction } => edited_curves
                .contains(curve.id.as_str())
                .then(|| (curve.id.0.clone(), (origin, direction))),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let conics = target
        .model
        .curves
        .iter()
        .filter_map(|curve| match curve.geometry {
            CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            } => edited_curves.contains(curve.id.as_str()).then(|| {
                (
                    curve.id.0.clone(),
                    (center, axis, ref_direction, radius, radius),
                )
            }),
            CurveGeometry::Ellipse {
                center,
                axis,
                major_direction,
                major_radius,
                minor_radius,
            } => edited_curves.contains(curve.id.as_str()).then(|| {
                (
                    curve.id.0.clone(),
                    (center, axis, major_direction, major_radius, minor_radius),
                )
            }),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let degenerate_curves = target
        .model
        .curves
        .iter()
        .filter_map(|curve| match curve.geometry {
            CurveGeometry::Degenerate { point } => edited_curves
                .contains(curve.id.as_str())
                .then(|| (curve.id.0.clone(), point)),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let planes = target
        .model
        .surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis,
            } => edited_surfaces
                .contains(surface.id.as_str())
                .then(|| (surface.id.0.clone(), (origin, normal, u_axis))),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let spheres = target
        .model
        .surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Sphere {
                center,
                axis,
                ref_direction,
                radius,
            } => edited_surfaces
                .contains(surface.id.as_str())
                .then(|| (surface.id.0.clone(), (center, axis, ref_direction, radius))),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let tori = target
        .model
        .surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Torus {
                center,
                axis,
                ref_direction,
                major_radius,
                minor_radius,
            } => edited_surfaces.contains(surface.id.as_str()).then(|| {
                (
                    surface.id.0.clone(),
                    (center, axis, ref_direction, major_radius, minor_radius),
                )
            }),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let cones = target
        .model
        .surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Cylinder {
                origin,
                axis,
                ref_direction,
                radius,
            } => edited_surfaces.contains(surface.id.as_str()).then(|| {
                (
                    surface.id.0.clone(),
                    (origin, axis, ref_direction, radius, 1.0, 0.0),
                )
            }),
            SurfaceGeometry::Cone {
                origin,
                axis,
                ref_direction,
                radius,
                ratio,
                half_angle,
            } => edited_surfaces.contains(surface.id.as_str()).then(|| {
                (
                    surface.id.0.clone(),
                    (origin, axis, ref_direction, radius, ratio, half_angle),
                )
            }),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();

    let mut archive = zip::ZipArchive::new(Cursor::new(source_image))
        .map_err(|error| CodecError::Malformed(format!("retained F3D ZIP is invalid: {error}")))?;
    let output = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(output);
    let mut patched_protein_appearances = BTreeSet::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| CodecError::Malformed(format!("invalid F3D ZIP entry: {error}")))?;
        let name = entry.name().to_owned();
        let options = SimpleFileOptions::default().compression_method(entry.compression());
        if entry.is_dir() {
            zip.add_directory(name, options).map_err(|error| {
                CodecError::Malformed(format!("cannot write F3D directory: {error}"))
            })?;
            continue;
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
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
                let (patched_bytes, patched_guids) =
                    crate::materials::patch_protein_appearances(&bytes, &protein_appearance_edits)?;
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
            if let Some(edits) = design_object_edits.get(&name) {
                patch_design_objects(&mut bytes, edits)?;
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
            if let Some(edits) = act_root_edits.get(&name) {
                patch_act_roots(&mut bytes, edits)?;
            }
            if let Some(edits) = act_entity_edits.get(&name) {
                patch_act_entities(&mut bytes, edits)?;
            }
        }
        zip.start_file(name, options)
            .map_err(|error| CodecError::Malformed(format!("cannot write F3D entry: {error}")))?;
        zip.write_all(&bytes)?;
    }
    if patched_protein_appearances.len() != protein_appearance_edits.len() {
        return Err(CodecError::NotImplemented(
            "one or more edited F3D appearances have no writable Protein carrier".into(),
        ));
    }
    let output = zip
        .finish()
        .map_err(|error| CodecError::Malformed(format!("cannot finish F3D ZIP: {error}")))?
        .into_inner();
    writer.write_all(&output)?;
    Ok(())
}
