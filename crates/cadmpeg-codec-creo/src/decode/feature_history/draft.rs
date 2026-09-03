// SPDX-License-Identifier: Apache-2.0
//! Schema, thicken, datum, and sweep-admission feature definitions.

use super::super::analytic::{
    cross, dot, placed_plane_surfaces, placed_planes, reconciled_model_plane,
};
use super::super::holes::{
    circular_sweep_feature_definition, circular_sweep_geometry, compact_simple_hole_cylinder_id,
    compact_simple_hole_geometry, counterbore_axis_placement, counterbore_dimensions,
    counterbore_directed_placement, extrusion_extent_and_direction, hole_placement,
    simple_drilled_hole_axis_placement, simple_drilled_hole_dimensions,
    simple_drilled_hole_envelope_spans, simple_drilled_hole_placement, simple_drilled_hole_recipe,
    simple_hole_geometry, stepped_hole_form,
};
use super::super::sketch::{approximately_equal, normalized};
use super::super::sketch_ids::{feature_sketch_record_id_in_scan, model_sketch_id};
use super::super::sketch_transfer::{
    feature_recipe, feature_recipe_effect, feature_revolution_extent, feature_schema_class,
    feature_section_sweep_semantics_conflict,
};
use super::super::sweep::{
    feature_outline_planes, feature_plane_equations, generated_arc_cylinder_extent,
    generated_bounded_cylinder_extent, generated_cap_plane_extent,
    generated_nurbs_translation_extent, generated_rectilinear_plane_extent,
};
use super::super::uniqueness::{
    unique_feature_datum_plane, unique_feature_definition_for_transform,
    unique_feature_profile_ref, unique_feature_section_transform, unique_owned_feature_definition,
};
use super::{
    chamfer_constant_distance, differing_positive_lengths, draft_neutral_plane_selection,
    extrude_feature_definition_with_profile, feature_edge_selection, feature_parameters,
    feature_reference_name, feature_result_surface_ids_by_feature,
    feature_revolution_axis_for_transfer, feature_surface_transitions,
    filled_surface_feature_definition, generated_surface_face_refs,
    knit_surface_feature_definition, model_feature_ids, named_or_referenced_feature_definition,
    reference_named_feature_definition, round_constant_radius, round_observed_radii,
    round_placed_cylinder_radii, schema_operation_kind, section_definition_for_history_feature,
    section_profile_ref, sweep_output_kind, sweep_solid, thicken_plane_offset,
    unresolved_extrude_extent,
};
use crate::container::ContainerScan;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    Angle, BooleanOp, ChamferSpec, EdgeSelection, ExtrudeExtent, FaceSelection,
    FeatureDefinition as IrFeatureDefinition, HoleBottom, HoleForm, HoleKind, Length, ProfileRef,
    RadiusForm, RadiusSpec, RevolutionConstruction, Termination,
};
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::ids::{FaceId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{BTreeMap, BTreeSet};

const EPS_FRAME_ORTHONORMAL: f64 = 1.0e-12;

pub(in super::super) fn thicken_feature_definition(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> IrFeatureDefinition {
    let transitions = feature_surface_transitions(
        feature_id,
        &scan.features.entity_tables,
        &scan.surfaces.rows,
    );
    let faces = transitions
        .as_ref()
        .map_or(FaceSelection::Unresolved, |transitions| {
            let source_ids = transitions
                .iter()
                .map(|(source_id, _)| *source_id)
                .collect::<Vec<_>>();
            let available_features = model_feature_ids(scan);
            let result_surface_ids = feature_result_surface_ids_by_feature(
                &scan.features.entity_tables,
                &scan.surfaces.rows,
            );
            let native = format!(
                "creo:allfeatur:thicken_source_surfaces#{feature_id}:{}",
                source_ids
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            );
            let faces = source_ids
                .iter()
                .map(|surface_id| FaceId(format!("creo:visibgeom:face#{surface_id}")))
                .collect::<Vec<_>>();
            if faces
                .iter()
                .all(|face| ir.model.faces.iter().any(|candidate| candidate.id == *face))
            {
                FaceSelection::Resolved { faces, native }
            } else if let Some(faces) = generated_surface_face_refs(
                &source_ids,
                &scan.surfaces.rows,
                &result_surface_ids,
                &available_features,
            ) {
                FaceSelection::Generated { faces, native }
            } else {
                FaceSelection::Native(native)
            }
        });
    let offset = transitions.as_deref().and_then(|transitions| {
        thicken_plane_offset(transitions, &placed_planes(scan), &scan.surfaces.rows)
    });
    IrFeatureDefinition::Thicken {
        faces,
        thickness: offset.map(|(magnitude, _)| Length(magnitude)),
        side: offset.map(|(_, side)| side),
    }
}

pub(in super::super) fn linear_extrusion_extent_and_direction(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<(ExtrudeExtent, [f64; 3])> {
    let transforms = scan
        .features
        .section_transforms
        .iter()
        .filter(|transform| transform.feature_id == Some(feature_id))
        .collect::<Vec<_>>();
    let definition = match transforms.as_slice() {
        [transform] => {
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        }
        [] => unique_owned_feature_definition(&scan.features.definitions, feature_id),
        _ => None,
    };
    let section = definition.and_then(|definition| definition.section_3d.as_ref());
    let unique_transform = match transforms.as_slice() {
        [] => Some(None),
        [transform] => Some(Some(*transform)),
        _ => None,
    };
    if let ([transform], Some(definition)) = (transforms.as_slice(), definition) {
        if let Some(extent) = generated_arc_cylinder_extent(scan, ir, definition, transform)
            .or_else(|| {
                feature_plane_equations(scan, ir, feature_id).and_then(|planes| {
                    extrusion_extent_and_direction(transform.origin, transform.normal, planes)
                })
            })
        {
            return Some(extent);
        }
    }
    generated_cap_plane_extent(scan, ir, feature_id)
        .or_else(|| {
            unique_transform.and_then(|transform| {
                generated_bounded_cylinder_extent(scan, ir, feature_id, transform)
            })
        })
        .or_else(|| {
            unique_transform.and_then(|transform| {
                generated_nurbs_translation_extent(scan, ir, feature_id, transform)
            })
        })
        .or_else(|| {
            (transforms.is_empty())
                .then_some(())
                .and_then(|()| generated_rectilinear_plane_extent(scan, ir, feature_id, section))
        })
}

pub(in super::super) fn schema_feature_definition(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    schema_class: u32,
    kind: &str,
) -> IrFeatureDefinition {
    if numbered_feature_name_has_family(kind, "Fill") {
        return filled_surface_feature_definition(scan, ir, feature_id);
    }
    if numbered_feature_name_has_family(kind, "Thicken") {
        return thicken_feature_definition(scan, ir, feature_id);
    }
    if numbered_feature_name_has_family(kind, "Merge") {
        return knit_surface_feature_definition(scan, feature_id);
    }
    if let Some(definition) = reference_named_feature_definition(kind) {
        return definition;
    }
    if schema_class == 926 {
        let sketch =
            section_definition_for_history_feature(scan, feature_id).and_then(|definition| {
                let section = definition.section_3d.as_ref()?;
                unique_feature_section_transform(
                    &scan.features.section_transforms,
                    definition.id,
                    section.offset,
                )?;
                let sketch = model_sketch_id(scan, definition);
                ir.model
                    .sketches
                    .iter()
                    .any(|candidate| candidate.id == sketch)
                    .then_some(sketch)
            });
        return IrFeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::default(),
            sketch,
        };
    }
    if schema_class == 911 {
        let stepped_form = stepped_hole_form(
            feature_id,
            &scan.features.entity_tables,
            &scan.surfaces.rows,
        );
        let stepped_dimensions = (stepped_form == Some(HoleForm::Counterbore))
            .then(|| counterbore_dimensions(scan, ir, feature_id))
            .flatten();
        let stepped_directed = (stepped_form == Some(HoleForm::Counterbore))
            .then(|| counterbore_directed_placement(scan, ir, feature_id))
            .flatten();
        let stepped_axis = (stepped_form == Some(HoleForm::Counterbore)
            && stepped_directed.is_none())
        .then(|| counterbore_axis_placement(scan, ir, feature_id))
        .flatten();
        let drilled_recipe = simple_drilled_hole_recipe(
            feature_id,
            &scan.features.entity_tables,
            &scan.surfaces.rows,
        );
        let drilled_dimensions = drilled_recipe.and_then(|recipe| {
            simple_drilled_hole_dimensions(
                scan,
                simple_drilled_hole_envelope_spans(scan, recipe.table),
                recipe.dimension_family,
            )
        });
        let drilled_placement =
            drilled_recipe
                .zip(drilled_dimensions)
                .and_then(|(recipe, (diameter, _, depth))| {
                    simple_drilled_hole_placement(scan, recipe.table, diameter, depth)
                });
        let placement = feature_outline_planes(scan, feature_id).and_then(hole_placement);
        let compact_cylinder_id = compact_simple_hole_cylinder_id(
            feature_id,
            &scan.features.entity_tables,
            &scan.surfaces.rows,
        );
        let solved = simple_hole_geometry(scan, feature_id)
            .or_else(|| compact_simple_hole_geometry(scan, feature_id));
        let simple_form = solved.is_some() || compact_cylinder_id.is_some();
        let result_surface_ids = feature_result_surface_ids_by_feature(
            &scan.features.entity_tables,
            &scan.surfaces.rows,
        );
        let available_features = model_feature_ids(scan);
        let face_selection = |surface_id| {
            let native = format!("creo:visibgeom:surface#{surface_id}");
            let face = FaceId(format!("creo:visibgeom:face#{surface_id}"));
            if ir.model.faces.iter().any(|candidate| candidate.id == face) {
                FaceSelection::Resolved {
                    faces: vec![face],
                    native,
                }
            } else if crate::surface::unique_surface_row(&scan.surfaces.rows, surface_id)
                .is_some_and(|row| row.feature_id == feature_id)
            {
                FaceSelection::Native(native)
            } else if let Some(faces) = generated_surface_face_refs(
                &[surface_id],
                &scan.surfaces.rows,
                &result_surface_ids,
                &available_features,
            ) {
                FaceSelection::Generated { faces, native }
            } else {
                FaceSelection::Native(native)
            }
        };
        let (face, position, direction, diameter, extent, bottom) = solved.map_or_else(
            || {
                stepped_directed.map_or_else(
                    || {
                        placement.map_or_else(
                            || {
                                drilled_placement.map_or(
                                    (None, None, None, None, None, None),
                                    |(position, direction)| {
                                        (None, Some(position), Some(direction), None, None, None)
                                    },
                                )
                            },
                            |(entry_surface_id, direction, extent)| {
                                (
                                    Some(face_selection(entry_surface_id)),
                                    None,
                                    Some(Vector3::new(direction[0], direction[1], direction[2])),
                                    None,
                                    Some(extent),
                                    None,
                                )
                            },
                        )
                    },
                    |(entry_surface_id, position, direction, extent)| {
                        (
                            entry_surface_id.map(face_selection),
                            Some(position),
                            Some(direction),
                            None,
                            Some(extent),
                            None,
                        )
                    },
                )
            },
            |hole| {
                let SurfaceGeometry::Cylinder { origin, radius, .. } = hole.geometry else {
                    unreachable!("simple hole helper returns a cylinder")
                };
                (
                    hole.entry_surface_id.map(face_selection),
                    Some(origin),
                    Some(Vector3::new(
                        hole.direction[0],
                        hole.direction[1],
                        hole.direction[2],
                    )),
                    Some(Length(2.0 * radius)),
                    Some(hole.extent),
                    Some(HoleBottom::Flat),
                )
            },
        );
        let drilled_dimensions =
            drilled_dimensions.filter(|(drilled_diameter, _, drilled_depth)| {
                !simple_form
                    && stepped_form.is_none()
                    && stepped_dimensions.is_none()
                    && diameter
                        .as_ref()
                        .is_none_or(|diameter| approximately_equal(diameter.0, *drilled_diameter))
                    && extent.as_ref().is_none_or(|extent| {
                        matches!(extent, Termination::Blind { length }
                        if approximately_equal(length.0, *drilled_depth))
                    })
            });
        let drilled_axis = (drilled_placement.is_none())
            .then(|| {
                let (recipe, (diameter, _, _)) = drilled_recipe.zip(drilled_dimensions)?;
                simple_drilled_hole_axis_placement(scan, recipe.table, diameter)
            })
            .flatten();
        return IrFeatureDefinition::Hole {
            profile: None,
            profile_filter: None,
            face,
            position,
            direction,
            placements: stepped_axis.into_iter().chain(drilled_axis).collect(),
            kind: match (
                drilled_dimensions,
                simple_form,
                stepped_form,
                stepped_dimensions,
            ) {
                (Some((_, drill_point_angle, _)), false, None, None) => HoleKind::SimpleDrilled {
                    drill_point_angle: Angle(drill_point_angle),
                },
                (None, true, None, None) => HoleKind::Simple,
                (None, false, Some(HoleForm::Counterbore), Some((_, diameter, depth))) => {
                    HoleKind::Counterbore {
                        diameter: Length(diameter),
                        depth: Length(depth),
                    }
                }
                (_, _, Some(HoleForm::Counterbore), dimensions) if dimensions.is_some() => {
                    HoleKind::PartialCounterbore {
                        diameter: dimensions.map(|(_, diameter, _)| Length(diameter)),
                        depth: dimensions.map(|(_, _, depth)| Length(depth)),
                    }
                }
                (_, _, form, _) => HoleKind::Unresolved(form),
            },
            exit_kind: None,
            diameter: diameter
                .or_else(|| drilled_dimensions.map(|(diameter, _, _)| Length(diameter)))
                .or_else(|| stepped_dimensions.map(|(diameter, _, _)| Length(diameter))),
            extent: extent.or_else(|| {
                drilled_dimensions.map(|(_, _, depth)| Termination::Blind {
                    length: Length(depth),
                })
            }),
            bottom,
            taper_angle: None,
            specification: None,
            allow_multi_profile_faces: None,
        };
    }
    if schema_class == 913 {
        let mut observed_radii = round_observed_radii(scan, feature_id);
        observed_radii.extend(round_placed_cylinder_radii(scan, ir, feature_id));
        let radius = round_constant_radius(scan, ir, feature_id).map_or_else(
            || RadiusSpec::Unresolved {
                form: differing_positive_lengths(&observed_radii).then_some(RadiusForm::Variable),
            },
            |radius| RadiusSpec::Constant {
                radius: Length(radius),
            },
        );
        return IrFeatureDefinition::Fillet {
            groups: vec![cadmpeg_ir::features::FilletGroup {
                edges: feature_edge_selection(scan, ir, feature_id)
                    .unwrap_or(EdgeSelection::Unresolved),
                radius,
                tangency_weight: None,
            }],
        };
    }
    if schema_class == 914 {
        return IrFeatureDefinition::Chamfer {
            groups: vec![cadmpeg_ir::features::ChamferGroup {
                edges: feature_edge_selection(scan, ir, feature_id)
                    .unwrap_or(EdgeSelection::Unresolved),
                spec: chamfer_constant_distance(scan, ir, feature_id).map_or_else(
                    || ChamferSpec::Unresolved { form: None },
                    |distance| ChamferSpec::Distance {
                        distance: Length(distance),
                    },
                ),
            }],
            flip_direction: false,
        };
    }
    if schema_class == 927 {
        let neutral_plane = draft_neutral_plane_selection(scan, feature_id);
        let anchor = cadmpeg_ir::features::DraftAnchor::NeutralPlane {
            plane: neutral_plane,
            pull: None,
        };
        return IrFeatureDefinition::Draft {
            faces: FaceSelection::Unresolved,
            anchor,
            angle: None,
            outward: None,
        };
    }
    if schema_class == 917
        && !feature_section_sweep_semantics_conflict(scan, feature_id)
        && section_sweep_allows_linear_extrusion(schema_class, feature_recipe(scan, feature_id))
    {
        if let Some(sweep) = circular_sweep_geometry(scan, feature_id) {
            let definition =
                unique_owned_feature_definition(&scan.features.definitions, feature_id).filter(
                    |definition| {
                        sweep
                            .section_definition_id
                            .is_none_or(|definition_id| definition_id == definition.id)
                    },
                );
            let profile = definition.map_or_else(
                || ProfileRef::Unresolved(format!("creo:model:feature#{feature_id}")),
                |definition| {
                    section_profile_ref(ir, feature_sketch_record_id_in_scan(scan, definition))
                },
            );
            let output_kind = sweep_output_kind(scan, ir, "extrusion", feature_id);
            return circular_sweep_feature_definition(
                profile,
                &sweep,
                section_sweep_boolean_operation(
                    feature_recipe_effect(scan, feature_id),
                    kind,
                    output_kind.is_some(),
                    preceding_features_establish_body(ir),
                ),
                sweep_solid(output_kind),
            );
        }
    }
    if feature_recipe(scan, feature_id) == Some(crate::feature::FeatureRecipeKind::Revolve) {
        let extent = feature_revolution_extent(scan, feature_id);
        let profile = unique_feature_profile_ref(scan, ir, feature_id);
        let axis = feature_revolution_axis_for_transfer(scan, ir, feature_id, extent.as_ref());
        let output_kind = sweep_output_kind(scan, ir, "revolution", feature_id);
        return IrFeatureDefinition::Revolve {
            construction: RevolutionConstruction {
                profile,
                axis,
                extent,
                axis_reference: None,
                solid: sweep_solid(output_kind),
                face_maker_class: None,
                fuse_order: None,
                allow_multi_profile_faces: None,
            },
            op: section_sweep_boolean_operation(
                feature_recipe_effect(scan, feature_id),
                kind,
                output_kind.is_some(),
                preceding_features_establish_body(ir),
            ),
        };
    }
    let recipe = feature_recipe(scan, feature_id);
    if (!feature_section_sweep_semantics_conflict(scan, feature_id)
        && section_sweep_allows_linear_extrusion(schema_class, recipe))
        || feature_is_sheet_extrusion(scan, feature_id)
    {
        let transforms = scan
            .features
            .section_transforms
            .iter()
            .filter(|transform| transform.feature_id == Some(feature_id))
            .collect::<Vec<_>>();
        let definition = match transforms.as_slice() {
            [transform] => {
                unique_feature_definition_for_transform(&scan.features.definitions, transform)
            }
            [] => unique_owned_feature_definition(&scan.features.definitions, feature_id),
            _ => None,
        };
        let profile = definition.map(|definition| {
            section_profile_ref(ir, feature_sketch_record_id_in_scan(scan, definition))
        });
        let output_kind = sweep_output_kind(scan, ir, "extrusion", feature_id);
        let op = section_sweep_boolean_operation(
            feature_recipe_effect(scan, feature_id),
            kind,
            output_kind.is_some(),
            preceding_features_establish_body(ir),
        );
        let extent_and_direction = linear_extrusion_extent_and_direction(scan, ir, feature_id);
        let construction = extent_and_direction.map(|(extent, direction)| {
            (
                Some(Vector3::new(direction[0], direction[1], direction[2])),
                extent,
            )
        });
        let (direction, extent) = construction.unwrap_or((None, unresolved_extrude_extent()));
        let profile = profile
            .unwrap_or_else(|| ProfileRef::Unresolved(format!("creo:model:feature#{feature_id}")));
        return IrFeatureDefinition::Extrude {
            profile,
            direction: direction.map_or(
                cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
                cadmpeg_ir::features::ExtrudeDirection::Explicit,
            ),
            start: cadmpeg_ir::features::ExtrudeStart::default(),
            extent,
            op,
            direction_source: None,
            solid: sweep_solid(output_kind),
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        };
    }
    if schema_class == 923 {
        if let Some(datum) = unique_feature_datum_plane(&scan.planes.datums, feature_id) {
            return datum_plane_feature_definition(datum);
        }
        if scan
            .planes
            .datums
            .iter()
            .any(|datum| datum.feature_id == feature_id)
        {
            return IrFeatureDefinition::DatumPlaneUnresolved;
        }
        let plane_ids = scan
            .surfaces
            .rows
            .iter()
            .filter(|row| {
                row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
            })
            .map(|row| row.id)
            .collect::<BTreeSet<_>>();
        let plane_ids = plane_ids.into_iter().collect::<Vec<_>>();
        if plane_ids.len() > 1 {
            return IrFeatureDefinition::DatumPlaneUnresolved;
        }
        if let [surface_id] = plane_ids.as_slice() {
            if crate::surface::unique_surface_row(&scan.surfaces.rows, *surface_id).is_none() {
                return IrFeatureDefinition::DatumPlaneUnresolved;
            }
            if let Some(definition) = reconciled_datum_plane_definition(scan, ir, *surface_id) {
                return definition;
            }
            return IrFeatureDefinition::DatumPlaneUnresolved;
        }
        let definitions = scan
            .features
            .definitions
            .iter()
            .filter(|definition| definition.owner_feature_id == Some(feature_id))
            .collect::<Vec<_>>();
        if let [definition] = definitions.as_slice() {
            if let Some(values) = crate::placement::unique_complete_local_system(definition) {
                let raw_normal: [f64; 3] = values[6..9].try_into().expect("three values");
                let raw_u_axis: [f64; 3] = values[0..3].try_into().expect("three values");
                if let (Some(normal), Some(u_axis)) =
                    (normalized(raw_normal), normalized(raw_u_axis))
                {
                    if dot(normal, u_axis).abs() <= EPS_FRAME_ORTHONORMAL {
                        let origin: [f64; 3] = values[9..12].try_into().expect("three values");
                        return IrFeatureDefinition::DatumPlane {
                            origin: Point3::new(origin[0], origin[1], origin[2]),
                            normal: Vector3::new(normal[0], normal[1], normal[2]),
                            u_axis: Vector3::new(u_axis[0], u_axis[1], u_axis[2]),
                        };
                    }
                }
            }
        }
        return IrFeatureDefinition::DatumPlaneUnresolved;
    }
    if schema_class == 946 {
        return knit_surface_feature_definition(scan, feature_id);
    }
    if schema_class == 979 && kind == "PRT_CSYS_DEF" {
        let definitions = scan
            .features
            .definitions
            .iter()
            .filter(|definition| definition.owner_feature_id == Some(feature_id))
            .collect::<Vec<_>>();
        if let [definition] = definitions.as_slice() {
            if let Some(values) = crate::placement::unique_complete_local_system(definition) {
                let x_axis = normalized(values[0..3].try_into().expect("three values"));
                let y_axis = normalized(values[3..6].try_into().expect("three values"));
                let z_axis = normalized(values[6..9].try_into().expect("three values"));
                let origin: [f64; 3] = values[9..12].try_into().expect("three values");
                if let (Some(x_axis), Some(y_axis), Some(z_axis)) = (x_axis, y_axis, z_axis) {
                    let right_handed =
                        dot(cross(x_axis, y_axis), z_axis) >= 1.0 - EPS_FRAME_ORTHONORMAL;
                    let orthogonal = dot(x_axis, y_axis).abs() <= EPS_FRAME_ORTHONORMAL
                        && dot(x_axis, z_axis).abs() <= EPS_FRAME_ORTHONORMAL
                        && dot(y_axis, z_axis).abs() <= EPS_FRAME_ORTHONORMAL;
                    if origin.into_iter().all(f64::is_finite) && orthogonal && right_handed {
                        return IrFeatureDefinition::DatumCoordinateSystem {
                            origin: Point3::new(origin[0], origin[1], origin[2]),
                            x_axis: Vector3::new(x_axis[0], x_axis[1], x_axis[2]),
                            y_axis: Vector3::new(y_axis[0], y_axis[1], y_axis[2]),
                            z_axis: Vector3::new(z_axis[0], z_axis[1], z_axis[2]),
                        };
                    }
                }
            }
        }
        return IrFeatureDefinition::DatumCoordinateSystemUnresolved;
    }
    if numbered_feature_name_has_family(kind, "Extrude")
        && !feature_is_sheet_extrusion(scan, feature_id)
    {
        let output_kind = sweep_output_kind(scan, ir, "extrusion", feature_id);
        let op = section_sweep_boolean_operation(
            feature_recipe_effect(scan, feature_id),
            kind,
            output_kind.is_some(),
            preceding_features_establish_body(ir),
        );
        return extrude_feature_definition_with_profile(scan, ir, feature_id, op);
    }
    if schema_class == 942
        && class_942_boundary_surface_entity_graph(
            feature_id,
            &scan.features.entity_tables,
            &scan.surfaces.rows,
        )
    {
        return IrFeatureDefinition::BoundarySurfaceUnresolved;
    }
    if schema_operation_kind(schema_class).is_none() {
        if let Some(definition) = named_or_referenced_feature_definition(scan, ir, feature_id, kind)
        {
            return definition;
        }
        if let Some(definition) = unbounded_feature_plane_definition(scan, ir, feature_id) {
            return definition;
        }
    }
    IrFeatureDefinition::Native {
        kind: kind.to_string(),
        parameters: feature_parameters(scan, feature_id),
        properties: BTreeMap::new(),
    }
}

pub(in super::super) fn datum_plane_feature_definition(
    datum: &crate::datum::DatumPlane,
) -> IrFeatureDefinition {
    IrFeatureDefinition::DatumPlane {
        origin: Point3::new(
            datum.normal[0] * datum.offset,
            datum.normal[1] * datum.offset,
            datum.normal[2] * datum.offset,
        ),
        normal: Vector3::new(datum.normal[0], datum.normal[1], datum.normal[2]),
        u_axis: cadmpeg_ir::geometry::derive_reference_direction(Vector3::new(
            datum.normal[0],
            datum.normal[1],
            datum.normal[2],
        )),
    }
}

fn reconciled_datum_plane_definition(
    scan: &ContainerScan,
    ir: &CadIr,
    surface_id: u32,
) -> Option<IrFeatureDefinition> {
    let plane = reconciled_model_plane(&placed_planes(scan), ir, surface_id)?;
    let normal = Vector3::new(plane.normal[0], plane.normal[1], plane.normal[2]);
    let u_axis = placed_plane_surfaces(scan)
        .get(&surface_id)
        .map(|(_, u_axis, _)| Vector3::new(u_axis[0], u_axis[1], u_axis[2]))
        .or_else(|| {
            let model_id = SurfaceId(format!("creo:visibgeom:surface#{surface_id}"));
            let surfaces = ir
                .model
                .surfaces
                .iter()
                .filter(|surface| surface.id == model_id)
                .collect::<Vec<_>>();
            let [surface] = surfaces.as_slice() else {
                return None;
            };
            match &surface.geometry {
                SurfaceGeometry::Plane { u_axis, .. } => Some(*u_axis),
                _ => None,
            }
        })
        .unwrap_or_else(|| cadmpeg_ir::geometry::derive_reference_direction(normal));
    Some(IrFeatureDefinition::DatumPlane {
        origin: Point3::new(plane.origin[0], plane.origin[1], plane.origin[2]),
        normal,
        u_axis,
    })
}

pub(in super::super) fn unbounded_feature_plane_definition(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<IrFeatureDefinition> {
    let rows = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
        })
        .collect::<Vec<_>>();
    let [row] = rows.as_slice() else {
        return None;
    };
    (row.boundary_type == 1
        && row.next_surface == 0
        && crate::surface::unique_surface_row(&scan.surfaces.rows, row.id) == Some(*row))
    .then_some(())?;
    reconciled_datum_plane_definition(scan, ir, row.id)
}

pub(in super::super) fn numbered_feature_name_has_family(name: &str, family: &str) -> bool {
    name.strip_prefix(family)
        .and_then(|suffix| suffix.strip_prefix(' '))
        .is_some_and(|ordinal| {
            !ordinal.is_empty() && ordinal.bytes().all(|byte| byte.is_ascii_digit())
        })
}

pub(in super::super) fn section_sweep_allows_linear_extrusion(
    schema_class: u32,
    recipe: Option<crate::feature::FeatureRecipeKind>,
) -> bool {
    recipe == Some(crate::feature::FeatureRecipeKind::Extrude)
        || (matches!(schema_class, 916 | 917)
            && recipe != Some(crate::feature::FeatureRecipeKind::Revolve))
}

pub(in super::super) fn feature_is_sheet_extrusion(scan: &ContainerScan, feature_id: u32) -> bool {
    feature_schema_class(scan, feature_id) == Some(942)
        && feature_reference_name(scan, feature_id)
            .is_some_and(|name| numbered_feature_name_has_family(name, "Extrude"))
}

pub(in super::super) fn feature_allows_linear_extrusion(
    scan: &ContainerScan,
    feature_id: u32,
) -> bool {
    (!feature_section_sweep_semantics_conflict(scan, feature_id)
        && feature_schema_class(scan, feature_id).is_some_and(|schema_class| {
            section_sweep_allows_linear_extrusion(schema_class, feature_recipe(scan, feature_id))
        }))
        || feature_is_sheet_extrusion(scan, feature_id)
}

pub(in super::super) fn feature_allows_additive_linear_extrusion(
    scan: &ContainerScan,
    feature_id: u32,
) -> bool {
    !feature_section_sweep_semantics_conflict(scan, feature_id)
        && feature_schema_class(scan, feature_id) == Some(917)
        && section_sweep_allows_linear_extrusion(917, feature_recipe(scan, feature_id))
        && feature_recipe_effect(scan, feature_id)
            .is_none_or(|effect| effect == crate::feature::FeatureRecipeEffect::Protrude)
}

pub(in super::super) fn preceding_features_establish_body(ir: &CadIr) -> bool {
    ir.model.features.iter().any(|feature| {
        feature.suppressed != Some(true)
            && (!feature.outputs.is_empty()
                || matches!(
                    feature.definition,
                    IrFeatureDefinition::Extrude {
                        op: BooleanOp::NewBody,
                        ..
                    } | IrFeatureDefinition::Revolve {
                        op: BooleanOp::NewBody,
                        ..
                    }
                ))
    })
}

pub(in super::super) fn section_sweep_boolean_operation(
    recipe_effect: Option<crate::feature::FeatureRecipeEffect>,
    kind: &str,
    has_evaluated_body: bool,
    prior_body: bool,
) -> BooleanOp {
    match recipe_effect {
        Some(crate::feature::FeatureRecipeEffect::Protrude) if prior_body => BooleanOp::Join,
        Some(crate::feature::FeatureRecipeEffect::Protrude) => BooleanOp::NewBody,
        Some(crate::feature::FeatureRecipeEffect::Cut) => BooleanOp::Cut,
        None if kind == "Protrusion" && prior_body => BooleanOp::Join,
        None if kind == "Protrusion" => BooleanOp::NewBody,
        None if kind == "Cut" => BooleanOp::Cut,
        None if has_evaluated_body => BooleanOp::NewBody,
        _ => BooleanOp::Unresolved,
    }
}

pub(in super::super) fn class_942_boundary_surface_entity_graph(
    feature_id: u32,
    tables: &[crate::feature::FeatureEntityTable],
    surface_rows: &[crate::surface::SurfaceRow],
) -> bool {
    let mut generated_surfaces = surface_rows
        .iter()
        .filter(|row| row.feature_id == feature_id);
    let Some(surface) = generated_surfaces.next() else {
        return false;
    };
    if generated_surfaces.next().is_some() || surface.kind != crate::surface::SurfaceKind::Extrusion
    {
        return false;
    }
    let owned = tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .collect::<Vec<_>>();
    let unique_table = |class_id| {
        let mut matches = owned
            .iter()
            .copied()
            .filter(|table| table.table_class_id == class_id);
        let table = matches.next()?;
        matches.next().is_none().then_some(table)
    };
    let Some(generated) = unique_table(29) else {
        return false;
    };
    let Some(topology) = unique_table(94) else {
        return false;
    };
    let Some(owner) = unique_table(67) else {
        return false;
    };
    let Some(output) = unique_table(100) else {
        return false;
    };
    let [owner_entry] = owner.entries.as_slice() else {
        return false;
    };
    matches!(
        generated.entries.as_slice(),
        [entry]
            if entry.class_id == 200
                && entry.entity_id == surface.id
                && entry.source_entity_id == Some(0)
                && generated.surface_ids.as_slice() == [surface.id]
    ) && topology
        .entries
        .iter()
        .map(|entry| entry.class_id)
        .eq([221, 222, 220, 220])
        && owner_entry.class_id == 200
        && owner_entry.source_entity_id == Some(feature_id)
        && matches!(
            output.entries.as_slice(),
            [entry]
                if entry.entity_id == owner_entry.entity_id
                    && entry.class_id == surface.id
        )
}

#[cfg(test)]
mod tests;
