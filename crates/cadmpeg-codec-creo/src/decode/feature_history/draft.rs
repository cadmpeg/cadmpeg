// SPDX-License-Identifier: Apache-2.0
//! Schema, thicken, datum, and sweep-admission feature definitions.

use super::super::holes::counterbore::{
    counterbore_axis_placement, counterbore_dimensions, counterbore_directed_placement,
};
use super::super::holes::drilled::{
    simple_drilled_hole_axis_placement, simple_drilled_hole_dimensions,
    simple_drilled_hole_envelope_spans, simple_drilled_hole_placement, simple_drilled_hole_recipe,
    stepped_hole_form,
};
use super::super::holes::placement::hole_placement;
use super::super::holes::sweep::{
    circular_sweep_feature_definition, circular_sweep_geometry, compact_simple_hole_cylinder_id,
    compact_simple_hole_geometry, extrusion_extent_and_direction, simple_hole_geometry,
};
use super::super::sketch::equations_coordinate::approximately_equal;
use super::super::sketch_ids::{feature_sketch_record_id_in_scan, model_sketch_id};
use super::super::sweep::extent::{
    generated_bounded_cylinder_extent, generated_nurbs_translation_extent,
    generated_rectilinear_plane_extent,
};
use super::super::sweep::planes::{
    feature_outline_planes, feature_plane_equations, generated_arc_cylinder_extent,
    generated_cap_plane_extent,
};
use super::super::uniqueness::{
    unique_feature_datum_plane, unique_feature_definition_for_transform,
    unique_feature_profile_ref, unique_feature_section_transform, unique_owned_feature_definition,
};
use super::axes::{
    feature_revolution_axis_for_transfer, model_feature_ids, section_profile_ref,
    unresolved_feature_profile_ref,
};
use super::knit::{
    draft_neutral_plane_selection, feature_result_surface_ids_by_feature,
    feature_surface_transitions, filled_surface_feature_definition, generated_surface_face_refs,
    knit_surface_feature_definition, thicken_plane_offset,
};
use super::named::{
    extrude_feature_definition_with_profile, named_or_referenced_feature_definition,
    reference_named_feature_definition, unresolved_extrude_extent,
};
use super::outputs::{
    feature_parameters, feature_reference_name, schema_operation_kind,
    section_definition_for_history_feature, sweep_output_kind, sweep_solid, CommaList,
};
use super::round::{
    chamfer_constant_distance, differing_positive_length_sets, round_constant_radius,
    round_observed_radii, round_placed_cylinder_radii,
};
use super::selections::feature_edge_selection;
use crate::container::ContainerScan;
use crate::decode::analytic::planes::{
    placed_plane_surfaces, placed_planes, reconciled_model_plane,
};
use crate::decode::sketch_transfer::recipe::{
    feature_revolution_extent, feature_schema_class, feature_section_sweep_semantics_conflict,
};
use crate::feature::schema::SchemaClass;
use crate::vecmath::normalize;
use crate::vecmath::{cross, dot};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::{
    features::{
        edge_treatments::{ChamferSpec, RadiusSpec},
        holes::{HoleBottom, HoleForm, HoleKind, HolePlacement},
        BooleanOp, EdgeSelection, ExtrudeExtent, FaceSelection,
        FeatureDefinition as IrFeatureDefinition, FeatureOperation as IrFeatureOperation,
        LinearTermination, PartialRevolveConstruction, RevolveConstruction, UnresolvedFamily,
    },
    scalar::Length,
};
use std::collections::{BTreeMap, BTreeSet};

/// The tolerance a feature definition's `local_sys` parameter frame is written to.
///
/// [`crate::placement::unique_complete_local_system`] yields twelve finite values, and every
/// reader of that record normalizes the columns it takes, so a column's length states nothing
/// and only the mutual orthogonality of the columns and the sign of their determinant remain.
/// This bound gates both: each pairwise dot against zero, and the determinant against one.
/// `placement.rs` reads the same record at the same value under `EPS_PLACEMENT_EXACT_GEOMETRY`.
///
/// `FeatureDatumPlaneFrame::new` and `FeatureCoordinateFrame::new` restate these conditions at
/// `1.0e-9`, in the one-sided determinant form used below, so this narrower bound is what
/// decides which matrices become a datum plane or a coordinate system.
const EPS_FEATURE_LOCAL_SYSTEM_ORTHOGONAL: f64 = 1.0e-12;

pub(super) fn thicken_feature_definition(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Result<IrFeatureDefinition, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo thicken feature evidence")?;
    let transitions = scratch.with_storage(|| {
        feature_surface_transitions(
            ctx,
            feature_id,
            &scan.features.entity_tables,
            &scan.surfaces.rows,
        )
    })?;
    let faces = if let Some(transitions) = transitions.as_ref() {
        let mut source_ids = Vec::new();
        scratch.with_storage(|| {
            ctx.reserve_capacity(
                &mut source_ids,
                transitions.len(),
                "creo thicken source surface IDs",
            )
        })?;
        for &(source_id, _) in
            ctx.admit_iter(transitions, "creo thicken source surface references")?
        {
            scratch.with_storage(|| {
                ctx.push_vec(
                    &mut source_ids,
                    source_id,
                    "creo thicken source surface IDs",
                )
            })?;
        }
        let native = ctx.format_retained(
            format_args!(
                "creo:allfeatur:thicken_source_surfaces#{feature_id}:{}",
                CommaList(&source_ids)
            ),
            "creo thicken native selection",
        )?;
        let mut resolved = Vec::new();
        let mut all_faces_resolved = true;
        let mut source_iter = source_ids.iter();
        while let Some(surface_id) =
            ctx.next_charged(&mut source_iter, "creo thicken source face IDs")?
        {
            let Some(face) = ctx.find_by(
                &ir.model.faces,
                |candidate| {
                    Ok(crate::identity::matches_numbered_identity(
                        candidate.id.as_str(),
                        "creo:visibgeom:face#",
                        *surface_id,
                    ))
                },
                "creo thicken model face lookup",
            )?
            else {
                all_faces_resolved = false;
                break;
            };
            scratch.with_storage(|| {
                ctx.push_vec(
                    &mut resolved,
                    &face.id,
                    "creo thicken resolved face references",
                )
            })?;
        }
        if all_faces_resolved {
            let mut faces = Vec::new();
            for source in ctx.admit_iter(&resolved, "creo thicken resolved face copies")? {
                let face = source.try_clone_for_decode(ctx, "creo thicken face IDs")?;
                ctx.push_vec(&mut faces, face, "creo thicken face identities")?;
            }
            FaceSelection::Resolved { faces, native }
        } else {
            let available_features = scratch.with_storage(|| model_feature_ids(ctx, scan))?;
            let result_surface_ids = scratch.with_storage(|| {
                feature_result_surface_ids_by_feature(
                    ctx,
                    &scan.features.entity_tables,
                    &scan.surfaces.rows,
                )
            })?;
            if let Some(faces) = generated_surface_face_refs(
                ctx,
                &source_ids,
                &scan.surfaces.rows,
                &result_surface_ids,
                &available_features,
            )? {
                FaceSelection::generated(
                    faces,
                    ctx.copy_retained_text(&native, "creo thicken generated native selection")?,
                    ctx,
                )?
                .unwrap_or(FaceSelection::Native(native))
            } else {
                FaceSelection::Native(native)
            }
        }
    } else {
        FaceSelection::Unresolved
    };
    let offset = match transitions.as_deref() {
        Some(transitions) => {
            let planes = scratch.with_storage(|| placed_planes(ctx, scan))?;
            thicken_plane_offset(ctx, transitions, &planes, &scan.surfaces.rows)?
        }
        None => None,
    };
    Ok(IrFeatureDefinition::Operation(
        IrFeatureOperation::Thicken {
            faces,
            thickness: offset
                .and_then(|(magnitude, _)| cadmpeg_ir::scalar::PositiveLength::new(magnitude)),
            side: offset.map(|(_, side)| side),
        },
    ))
}

fn hole_face_selection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    surface_id: u32,
    result_surface_ids: &BTreeMap<u32, Vec<u32>>,
    available_features: &BTreeSet<cadmpeg_ir::features::FeatureId>,
) -> Result<FaceSelection, cadmpeg_core::CodecError> {
    let native = ctx.format_retained(
        format_args!("creo:visibgeom:surface#{surface_id}"),
        "creo hole native face selection",
    )?;
    let resolved = ctx.find_by(
        &ir.model.faces,
        |candidate| {
            Ok(crate::identity::matches_numbered_identity(
                candidate.id.as_str(),
                "creo:visibgeom:face#",
                surface_id,
            ))
        },
        "creo hole face lookup",
    )?;
    if let Some(source) = resolved {
        let face = source.id.try_clone_for_decode(ctx, "creo hole face IDs")?;
        let mut faces = Vec::new();
        ctx.reserve_vec(&mut faces, 1, "creo hole face identities")?;
        faces.push(face);
        return Ok(FaceSelection::Resolved { faces, native });
    }
    if crate::surface::unique_surface_row(&scan.surfaces.rows, surface_id)
        .is_some_and(|row| row.feature_id == feature_id)
    {
        return Ok(FaceSelection::Native(native));
    }
    if let Some(faces) = generated_surface_face_refs(
        ctx,
        &[surface_id],
        &scan.surfaces.rows,
        result_surface_ids,
        available_features,
    )? {
        return Ok(FaceSelection::generated(
            faces,
            ctx.copy_retained_text(&native, "creo hole generated native selection")?,
            ctx,
        )?
        .unwrap_or(FaceSelection::Native(native)));
    }
    Ok(FaceSelection::Native(native))
}

fn admitted_hole_placements(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: [Option<HolePlacement>; 3],
) -> Result<Vec<HolePlacement>, cadmpeg_core::CodecError> {
    let mut placements = Vec::new();
    for placement in candidates.into_iter().flatten() {
        ctx.reserve_vec(&mut placements, 1, "creo hole placements")?;
        placements.push(placement);
    }
    Ok(placements)
}

pub(super) fn linear_extrusion_extent_and_direction(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
) -> Result<Option<(ExtrudeExtent, [f64; 3])>, cadmpeg_core::CodecError> {
    let transforms = &scan.features.section_transforms;
    let unique_transform = match ctx.position_by(
        transforms,
        |transform| Ok(transform.feature_id == Some(feature_id)),
        "creo extrusion section transforms",
    )? {
        None => Some(None),
        Some(first) => (!ctx.any_by(
            &transforms[first + 1..],
            |transform| Ok(transform.feature_id == Some(feature_id)),
            "creo extrusion section transforms",
        )?)
        .then_some(Some(&transforms[first])),
    };
    let definition = match unique_transform {
        Some(Some(transform)) => {
            unique_feature_definition_for_transform(ctx, &scan.features.definitions, transform)?
        }
        Some(None) => unique_owned_feature_definition(ctx, &scan.features.definitions, feature_id)?,
        None => None,
    };
    let section = definition.and_then(|definition| definition.section_3d.as_ref());
    if let (Some(Some(transform)), Some(definition)) = (unique_transform, definition) {
        let mut extent =
            generated_arc_cylinder_extent(ctx, scan, ir, source_carriers, definition, transform)?;
        if extent.is_none() {
            if let Some(planes) =
                feature_plane_equations(ctx, scan, ir, source_carriers, feature_id)?
            {
                extent = extrusion_extent_and_direction(
                    transform.origin(),
                    transform.normal(),
                    ctx.admit_iter(&planes, "creo extrusion plane equations")?
                        .map(|plane| (plane.origin, plane.normal)),
                );
            }
        }
        if let Some(extent) = extent {
            return Ok(Some(extent));
        }
    }
    let mut extent = generated_cap_plane_extent(ctx, scan, ir, source_carriers, feature_id)?;
    if extent.is_none() {
        if let Some(transform) = unique_transform {
            extent = generated_bounded_cylinder_extent(
                ctx,
                scan,
                ir,
                source_carriers,
                feature_id,
                transform,
            )?;
        }
    }
    if extent.is_none() {
        if let Some(transform) = unique_transform {
            extent = generated_nurbs_translation_extent(
                ctx,
                scan,
                ir,
                source_carriers,
                feature_id,
                transform,
            )?;
        }
    }
    if extent.is_none() && matches!(unique_transform, Some(None)) {
        extent = generated_rectilinear_plane_extent(
            ctx,
            scan,
            ir,
            source_carriers,
            feature_id,
            section,
        )?;
    }
    Ok(extent)
}

pub(in super::super) fn schema_feature_definition(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
    schema_class: Option<SchemaClass>,
    kind: &str,
) -> Result<IrFeatureDefinition, cadmpeg_core::CodecError> {
    if numbered_feature_name_has_family(ctx, kind, "Fill")? {
        return filled_surface_feature_definition(ctx, scan, ir, feature_id);
    }
    if numbered_feature_name_has_family(ctx, kind, "Thicken")? {
        return thicken_feature_definition(ctx, scan, ir, feature_id);
    }
    if numbered_feature_name_has_family(ctx, kind, "Merge")? {
        return knit_surface_feature_definition(ctx, scan, feature_id);
    }
    if let Some(definition) = reference_named_feature_definition(ctx, kind)? {
        return Ok(definition);
    }
    if schema_class == Some(SchemaClass::Section) {
        let definition = match section_definition_for_history_feature(ctx, scan, feature_id)? {
            Some(definition) => match definition.section_3d.as_ref() {
                Some(section)
                    if unique_feature_section_transform(
                        ctx,
                        &scan.features.section_transforms,
                        definition.identity.id(),
                        section.offset,
                    )?
                    .is_some() =>
                {
                    Some(definition)
                }
                _ => None,
            },
            None => None,
        };
        let sketch = match definition {
            Some(definition) => match model_sketch_id(ctx, scan, definition)? {
                Some(sketch) => {
                    let sketch_found = ctx.any_by(
                        &ir.model.sketches,
                        |candidate| {
                            ctx.equal(
                                &candidate.id,
                                &sketch,
                                "creo schema sketch identity comparison",
                            )
                        },
                        "creo schema sketch model lookup",
                    )?;
                    sketch_found.then_some(sketch)
                }
                None => None,
            },
            None => None,
        };
        return Ok(IrFeatureDefinition::Operation(IrFeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(sketch),
        }));
    }
    if schema_class == Some(SchemaClass::Hole) {
        let stepped_form = stepped_hole_form(
            ctx,
            feature_id,
            &scan.features.entity_tables,
            &scan.surfaces.rows,
        )?;
        let stepped_dimensions = if stepped_form == Some(HoleForm::Counterbore) {
            counterbore_dimensions(ctx, scan, ir, feature_id)?
        } else {
            None
        };
        let stepped_directed = if stepped_form == Some(HoleForm::Counterbore) {
            counterbore_directed_placement(ctx, scan, ir, source_carriers, feature_id)?
        } else {
            None
        };
        let stepped_axis =
            if stepped_form == Some(HoleForm::Counterbore) && stepped_directed.is_none() {
                counterbore_axis_placement(ctx, scan, ir, feature_id)?
            } else {
                None
            };
        let drilled_recipe = simple_drilled_hole_recipe(
            ctx,
            feature_id,
            &scan.features.entity_tables,
            &scan.surfaces.rows,
        )?;
        let drilled_dimensions = if let Some(recipe) = drilled_recipe {
            simple_drilled_hole_dimensions(
                ctx,
                scan,
                simple_drilled_hole_envelope_spans(ctx, scan, recipe.table)?,
                recipe.dimension_family,
            )?
        } else {
            None
        };
        let drilled_placement = if let (Some(recipe), Some((diameter, _, depth))) =
            (drilled_recipe, drilled_dimensions)
        {
            simple_drilled_hole_placement(ctx, scan, recipe.table, diameter, depth)?
        } else {
            None
        };
        let placement = feature_outline_planes(ctx, scan, feature_id)?.and_then(hole_placement);
        let compact_cylinder_id = compact_simple_hole_cylinder_id(
            ctx,
            feature_id,
            &scan.features.entity_tables,
            &scan.surfaces.rows,
        )?;
        let mut solved = simple_hole_geometry(ctx, scan, feature_id)?;
        if solved.is_none() {
            solved = compact_simple_hole_geometry(ctx, scan, feature_id)?;
        }
        let simple_form = solved.is_some() || compact_cylinder_id.is_some();
        let mut face_evidence = ctx.reserve_scoped(0, "creo hole face selection evidence")?;
        let result_surface_ids = face_evidence.with_storage(|| {
            feature_result_surface_ids_by_feature(
                ctx,
                &scan.features.entity_tables,
                &scan.surfaces.rows,
            )
        })?;
        let available_features = face_evidence.with_storage(|| model_feature_ids(ctx, scan))?;
        let face_selection = |surface_id| {
            hole_face_selection(
                ctx,
                scan,
                ir,
                feature_id,
                surface_id,
                &result_surface_ids,
                &available_features,
            )
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
                                        (
                                            None,
                                            cadmpeg_ir::features::FinitePoint3::new(position),
                                            cadmpeg_ir::features::FeatureDirection3::new(direction),
                                            None,
                                            None,
                                            None,
                                        )
                                    },
                                )
                            },
                            |(entry_surface_id, direction, extent)| {
                                (
                                    Some(face_selection(entry_surface_id)),
                                    None,
                                    cadmpeg_ir::features::FeatureDirection3::new(Vector3::from(
                                        direction,
                                    )),
                                    None,
                                    Some(extent),
                                    None,
                                )
                            },
                        )
                    },
                    |crate::decode::holes::counterbore::CounterborePlacement {
                         face: entry_surface_id,
                         position,
                         direction,
                         extent,
                     }| {
                        (
                            entry_surface_id.map(face_selection),
                            cadmpeg_ir::features::FinitePoint3::new(position),
                            cadmpeg_ir::features::FeatureDirection3::new(direction),
                            None,
                            Some(extent),
                            None,
                        )
                    },
                )
            },
            |hole| {
                (
                    hole.entry_surface_id.map(face_selection),
                    Some(hole.geometry.origin()),
                    Some(cadmpeg_ir::features::FeatureDirection3::from(
                        *hole.geometry.frame().axis(),
                    )),
                    Length::new(2.0 * hole.geometry.radius().get()),
                    Some(hole.extent),
                    Some(HoleBottom::Flat),
                )
            },
        );
        let face = face.transpose()?;
        let drilled_dimensions =
            drilled_dimensions.filter(|(drilled_diameter, _, drilled_depth)| {
                !simple_form
                    && stepped_form.is_none()
                    && stepped_dimensions.is_none()
                    && diameter.as_ref().is_none_or(|diameter| {
                        let measured = FiniteReal::new(diameter.get());
                        let drilled = FiniteReal::new(*drilled_diameter);
                        measured
                            .zip(drilled)
                            .is_some_and(|(first, second)| approximately_equal(first, second))
                    })
                    && extent.as_ref().is_none_or(|extent| {
                        let LinearTermination::Blind { length } = extent else {
                            return false;
                        };
                        let measured = FiniteReal::new(length.get());
                        let drilled = FiniteReal::new(*drilled_depth);
                        measured
                            .zip(drilled)
                            .is_some_and(|(first, second)| approximately_equal(first, second))
                    })
            });
        let drilled_axis = if drilled_placement.is_none() {
            if let Some((recipe, (diameter, _, _))) = drilled_recipe.zip(drilled_dimensions) {
                simple_drilled_hole_axis_placement(ctx, scan, recipe.table, diameter)?
            } else {
                None
            }
        } else {
            None
        };
        let placement_candidates = [
            position
                .zip(direction)
                .map(|(position, direction)| HolePlacement::Directed {
                    position,
                    direction,
                }),
            stepped_axis,
            drilled_axis,
        ];
        let placements = admitted_hole_placements(ctx, placement_candidates)?;
        return Ok(IrFeatureDefinition::Operation(IrFeatureOperation::Hole {
            profile: None,
            profile_filter: None,
            face,
            direction: None,
            placements: (!placements.is_empty()).then_some(placements),
            shape: cadmpeg_ir::features::holes::HoleShape::new(
                cadmpeg_ir::features::holes::HoleConstruction::Form {
                    kind: match (
                        drilled_dimensions,
                        simple_form,
                        stepped_form,
                        stepped_dimensions,
                    ) {
                        (Some((_, drill_point_angle, _)), false, None, None) => {
                            cadmpeg_ir::scalar::InteriorAngle::new(drill_point_angle)
                                .map_or(HoleKind::Unresolved(None), |drill_point_angle| {
                                    HoleKind::SimpleDrilled { drill_point_angle }
                                })
                        }
                        (None, true, None, None) => HoleKind::Simple,
                        (_, _, Some(HoleForm::Counterbore), dimensions) if dimensions.is_some() => {
                            match (
                                dimensions.and_then(|(_, diameter, _)| {
                                    cadmpeg_ir::scalar::PositiveLength::new(diameter)
                                }),
                                dimensions.and_then(|(_, _, depth)| {
                                    cadmpeg_ir::scalar::PositiveLength::new(depth)
                                }),
                            ) {
                                (Some(diameter), Some(depth)) => {
                                    HoleKind::Counterbore { diameter, depth }
                                }
                                (Some(diameter), None) => HoleKind::PartialCounterbore(
                                    cadmpeg_ir::features::holes::PartialPair::First(diameter),
                                ),
                                (None, Some(depth)) => HoleKind::PartialCounterbore(
                                    cadmpeg_ir::features::holes::PartialPair::Second(depth),
                                ),
                                (None, None) => HoleKind::Unresolved(Some(HoleForm::Counterbore)),
                            }
                        }
                        (_, _, form, _) => HoleKind::Unresolved(form),
                    },
                    specification: None,
                },
                None,
                diameter
                    .or_else(|| {
                        drilled_dimensions.and_then(|(diameter, _, _)| Length::new(diameter))
                    })
                    .or_else(|| {
                        stepped_dimensions.and_then(|(diameter, _, _)| Length::new(diameter))
                    })
                    .and_then(|diameter| {
                        cadmpeg_ir::scalar::PositiveLength::try_from(diameter).ok()
                    }),
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,

            extent: extent.or_else(|| {
                drilled_dimensions
                    .and_then(|(_, _, depth)| cadmpeg_ir::scalar::NonZeroLength::new(depth))
                    .map(|length| LinearTermination::Blind { length })
            }),
            bottom,
            taper_angle: None,
            allow_multi_profile_faces: None,
        }));
    }
    if schema_class == Some(SchemaClass::Round) {
        let radius = match round_constant_radius(ctx, scan, ir, source_carriers, feature_id)?
            .and_then(cadmpeg_ir::scalar::PositiveLength::new)
        {
            Some(radius) => RadiusSpec::Constant { radius },
            None => {
                let mut scratch = ctx.reserve_scoped(0, "creo feature round radius evidence")?;
                let observed =
                    scratch.with_storage(|| round_observed_radii(ctx, scan, feature_id))?;
                let placed = scratch.with_storage(|| {
                    round_placed_cylinder_radii(ctx, scan, ir, source_carriers, feature_id)
                })?;
                if differing_positive_length_sets(ctx, &observed, &placed)? {
                    RadiusSpec::Unresolved {
                        form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Variable),
                    }
                } else {
                    RadiusSpec::Unresolved { form: None }
                }
            }
        };
        return Ok(IrFeatureDefinition::Operation(IrFeatureOperation::Fillet {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(
                cadmpeg_ir::features::edge_treatments::FilletGroup {
                    edges: feature_edge_selection(ctx, scan, ir, feature_id)?
                        .unwrap_or(EdgeSelection::Unresolved),
                    radius,
                    tangency_weight: None,
                },
            ),
        }));
    }
    if schema_class == Some(SchemaClass::Chamfer) {
        return Ok(IrFeatureDefinition::Operation(
            IrFeatureOperation::Chamfer {
                groups: cadmpeg_ir::features::NonEmptyMembers::one(
                    cadmpeg_ir::features::edge_treatments::ChamferGroup {
                        edges: feature_edge_selection(ctx, scan, ir, feature_id)?
                            .unwrap_or(EdgeSelection::Unresolved),
                        spec: chamfer_constant_distance(
                            ctx,
                            scan,
                            ir,
                            source_carriers,
                            feature_id,
                        )?
                        .and_then(cadmpeg_ir::scalar::PositiveLength::new)
                        .map_or_else(
                            || ChamferSpec::Unresolved { form: None },
                            |distance| ChamferSpec::Distance { distance },
                        ),
                    },
                ),
                flip_direction: false,
            },
        ));
    }
    if schema_class == Some(SchemaClass::Draft) {
        let neutral_plane = draft_neutral_plane_selection(ctx, scan, feature_id)?;
        let anchor = cadmpeg_ir::features::DraftAnchor::NeutralPlane {
            plane: neutral_plane,
            pull: None,
        };
        return Ok(IrFeatureDefinition::Operation(IrFeatureOperation::Draft {
            faces: FaceSelection::Unresolved,
            anchor,
            angle: None,
            outward: None,
        }));
    }
    let recipe = super::operations::feature_recipe(ctx, scan, feature_id)?;
    if schema_class == Some(SchemaClass::Protrusion)
        && !feature_section_sweep_semantics_conflict(ctx, scan, feature_id)?
        && section_sweep_allows_linear_extrusion(
            schema_class,
            recipe.map(crate::feature::operations::FeatureRecipe::kind),
        )
    {
        if let Some(sweep) = circular_sweep_geometry(ctx, scan, feature_id)? {
            let definition =
                unique_owned_feature_definition(ctx, &scan.features.definitions, feature_id)?
                    .filter(|definition| {
                        sweep
                            .section_definition_id
                            .is_none_or(|definition_id| definition_id == definition.identity.id())
                    });
            let profile = match definition {
                Some(definition) => section_profile_ref(
                    ctx,
                    ir,
                    feature_sketch_record_id_in_scan(ctx, scan, definition)?,
                )?,
                None => unresolved_feature_profile_ref(
                    ctx,
                    feature_id,
                    "creo unresolved section profile identity",
                )?,
            };
            let output_kind = sweep_output_kind(ctx, scan, ir, "extrusion", feature_id)?;
            return Ok(circular_sweep_feature_definition(
                profile,
                &sweep,
                section_sweep_boolean_operation(
                    recipe.map(crate::feature::operations::FeatureRecipe::effect),
                    kind,
                    output_kind.is_some(),
                    preceding_features_establish_body(ctx, ir)?,
                ),
                sweep_solid(output_kind),
            ));
        }
    }
    if recipe.map(crate::feature::operations::FeatureRecipe::kind)
        == Some(crate::feature::operations::FeatureRecipeKind::Revolve)
    {
        let extent = feature_revolution_extent(ctx, scan, feature_id)?;
        let profile = unique_feature_profile_ref(ctx, scan, ir, feature_id)?;
        let axis = feature_revolution_axis_for_transfer(
            ctx,
            scan,
            ir,
            source_carriers,
            feature_id,
            extent.as_ref(),
        )?;
        let output_kind = sweep_output_kind(ctx, scan, ir, "revolution", feature_id)?;
        let profile = profile.and_then(|profile| match profile {
            cadmpeg_ir::features::ProfileRef::Planar(profile) => Some(profile),
            _ => None,
        });
        let solid = sweep_solid(output_kind);
        return Ok(IrFeatureDefinition::Operation(
            IrFeatureOperation::Revolve {
                construction: match (profile, axis, extent) {
                    (None, axis, extent) => {
                        RevolveConstruction::Unresolved(PartialRevolveConstruction::Profile {
                            axis,
                            extent,
                            solid,
                            face_maker: None,
                            fuse_order: None,
                            allow_multi_profile_faces: None,
                        })
                    }
                    (Some(profile), None, extent) => {
                        RevolveConstruction::Unresolved(PartialRevolveConstruction::Axis {
                            profile,
                            extent,
                            solid,
                            face_maker: None,
                            fuse_order: None,
                            allow_multi_profile_faces: None,
                        })
                    }
                    (Some(profile), Some(axis), None) => {
                        RevolveConstruction::Unresolved(PartialRevolveConstruction::Extent {
                            profile,
                            axis,
                            solid,
                            face_maker: None,
                            fuse_order: None,
                            allow_multi_profile_faces: None,
                        })
                    }
                    (Some(profile), Some(axis), Some(extent)) => RevolveConstruction::Resolved {
                        profile,
                        axis,
                        extent,
                        solid,
                        face_maker: None,
                        fuse_order: None,
                        allow_multi_profile_faces: None,
                    },
                },
                op: section_sweep_boolean_operation(
                    recipe.map(crate::feature::operations::FeatureRecipe::effect),
                    kind,
                    output_kind.is_some(),
                    preceding_features_establish_body(ctx, ir)?,
                ),
            },
        ));
    }
    let recipe_kind = recipe.map(crate::feature::operations::FeatureRecipe::kind);
    if (!feature_section_sweep_semantics_conflict(ctx, scan, feature_id)?
        && section_sweep_allows_linear_extrusion(schema_class, recipe_kind))
        || feature_is_sheet_extrusion(ctx, scan, feature_id)?
    {
        let transforms = &scan.features.section_transforms;
        let unique_transform = match ctx.position_by(
            transforms,
            |transform| Ok(transform.feature_id == Some(feature_id)),
            "creo extrusion section transforms",
        )? {
            None => Some(None),
            Some(first) => (!ctx.any_by(
                &transforms[first + 1..],
                |transform| Ok(transform.feature_id == Some(feature_id)),
                "creo extrusion section transforms",
            )?)
            .then_some(Some(&transforms[first])),
        };
        let definition = match unique_transform {
            Some(Some(transform)) => {
                unique_feature_definition_for_transform(ctx, &scan.features.definitions, transform)?
            }
            Some(None) => {
                unique_owned_feature_definition(ctx, &scan.features.definitions, feature_id)?
            }
            None => None,
        };
        let profile = match definition {
            Some(definition) => Some(section_profile_ref(
                ctx,
                ir,
                feature_sketch_record_id_in_scan(ctx, scan, definition)?,
            )?),
            None => None,
        };
        let output_kind = sweep_output_kind(ctx, scan, ir, "extrusion", feature_id)?;
        let op = section_sweep_boolean_operation(
            recipe.map(crate::feature::operations::FeatureRecipe::effect),
            kind,
            output_kind.is_some(),
            preceding_features_establish_body(ctx, ir)?,
        );
        let extent_and_direction =
            linear_extrusion_extent_and_direction(ctx, scan, ir, source_carriers, feature_id)?;
        let construction = extent_and_direction
            .map(|(extent, direction)| (Some(Vector3::from(direction)), extent));
        let (direction, extent) = construction.unwrap_or((None, unresolved_extrude_extent()));
        let profile = match profile {
            Some(profile) => profile,
            None => unresolved_feature_profile_ref(
                ctx,
                feature_id,
                "creo unresolved section profile identity",
            )?,
        };
        return Ok(IrFeatureDefinition::Operation(
            IrFeatureOperation::Extrude {
                profile,
                direction: direction.map_or(
                    cadmpeg_ir::features::ExtrudeDirection::ProfileNormal {},
                    |vector| {
                        cadmpeg_ir::features::FeatureDirection3::new(vector).map_or(
                            cadmpeg_ir::features::ExtrudeDirection::Unresolved {},
                            |vector| cadmpeg_ir::features::ExtrudeDirection::Explicit {
                                vector,
                                source: None,
                            },
                        )
                    },
                ),
                start: cadmpeg_ir::features::ExtrudeStart::default(),
                extent,
                op,
                solid: sweep_solid(output_kind),
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            },
        ));
    }
    if schema_class == Some(SchemaClass::DatumPlane) {
        if let Some(datum) = unique_feature_datum_plane(ctx, &scan.planes.datums, feature_id)? {
            return Ok(datum_plane_feature_definition(&datum.plane()));
        }
        if ctx.any_by(
            &scan.planes.datums,
            |datum| Ok(datum.feature_id == feature_id),
            "creo datum plane feature records",
        )? {
            return Ok(IrFeatureDefinition::Operation(
                IrFeatureOperation::Unresolved {
                    family: UnresolvedFamily::DatumPlane,
                },
            ));
        }
        let rows = &*scan.surfaces.rows;
        if let Some(first) =
            ctx.position_by(
                rows,
                |row| {
                    Ok(row.feature_id == feature_id
                        && row.kind == crate::surface::SurfaceKind::Plane)
                },
                "creo schema datum surface rows",
            )?
        {
            let surface_id = rows[first].id;
            if ctx.any_by(
                &rows[first + 1..],
                |row| {
                    Ok(row.feature_id == feature_id
                        && row.kind == crate::surface::SurfaceKind::Plane
                        && row.id != surface_id)
                },
                "creo schema datum surface rows",
            )? || crate::surface::unique_surface_row(&scan.surfaces.rows, surface_id).is_none()
            {
                return Ok(IrFeatureDefinition::Operation(
                    IrFeatureOperation::Unresolved {
                        family: UnresolvedFamily::DatumPlane,
                    },
                ));
            }
            if let Some(definition) =
                reconciled_datum_plane_definition(ctx, scan, ir, source_carriers, surface_id)?
            {
                return Ok(definition);
            }
            return Ok(IrFeatureDefinition::Operation(
                IrFeatureOperation::Unresolved {
                    family: UnresolvedFamily::DatumPlane,
                },
            ));
        }
        if let Some(definition) = crate::decode::uniqueness::exactly_one_by(
            ctx,
            &scan.features.definitions,
            |definition| Ok(definition.identity.owner_feature_id() == Some(feature_id)),
            "creo schema datum feature definitions",
        )? {
            if let Some(values) = crate::placement::unique_complete_local_system(definition) {
                let values = values.get();
                let raw_normal = [values[6], values[7], values[8]];
                let raw_u_axis = [values[0], values[1], values[2]];
                if let (Some(normal), Some(u_axis)) = (normalize(raw_normal), normalize(raw_u_axis))
                {
                    if dot(normal, u_axis).abs() <= EPS_FEATURE_LOCAL_SYSTEM_ORTHOGONAL {
                        let origin = [values[9], values[10], values[11]];
                        if let Some(frame) = cadmpeg_ir::features::FeatureDatumPlaneFrame::new(
                            Point3::from(origin),
                            Vector3::from(normal),
                            Vector3::from(u_axis),
                        ) {
                            return Ok(IrFeatureDefinition::Operation(
                                IrFeatureOperation::DatumPlane { frame },
                            ));
                        }
                    }
                }
            }
        }
        return Ok(IrFeatureDefinition::Operation(
            IrFeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumPlane,
            },
        ));
    }
    if schema_class == Some(SchemaClass::SurfaceMerge) {
        return knit_surface_feature_definition(ctx, scan, feature_id);
    }
    if schema_class == Some(SchemaClass::CoordinateSystem) && kind == "PRT_CSYS_DEF" {
        if let Some(definition) = crate::decode::uniqueness::exactly_one_by(
            ctx,
            &scan.features.definitions,
            |definition| Ok(definition.identity.owner_feature_id() == Some(feature_id)),
            "creo coordinate system feature definitions",
        )? {
            if let Some(values) = crate::placement::unique_complete_local_system(definition) {
                let values = values.get();
                let x_axis = normalize([values[0], values[1], values[2]]);
                let y_axis = normalize([values[3], values[4], values[5]]);
                let z_axis = normalize([values[6], values[7], values[8]]);
                let origin = [values[9], values[10], values[11]];
                if let (Some(x_axis), Some(y_axis), Some(z_axis)) = (x_axis, y_axis, z_axis) {
                    let right_handed = dot(cross(x_axis, y_axis), z_axis)
                        >= 1.0 - EPS_FEATURE_LOCAL_SYSTEM_ORTHOGONAL;
                    let orthogonal = dot(x_axis, y_axis).abs()
                        <= EPS_FEATURE_LOCAL_SYSTEM_ORTHOGONAL
                        && dot(x_axis, z_axis).abs() <= EPS_FEATURE_LOCAL_SYSTEM_ORTHOGONAL
                        && dot(y_axis, z_axis).abs() <= EPS_FEATURE_LOCAL_SYSTEM_ORTHOGONAL;
                    if orthogonal && right_handed {
                        if let Some(frame) = cadmpeg_ir::features::FeatureCoordinateFrame::new(
                            Point3::from(origin),
                            Vector3::from(x_axis),
                            Vector3::from(y_axis),
                            Vector3::from(z_axis),
                        ) {
                            return Ok(IrFeatureDefinition::Operation(
                                IrFeatureOperation::DatumCoordinateSystem { frame },
                            ));
                        }
                    }
                }
            }
        }
        return Ok(IrFeatureDefinition::Operation(
            IrFeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumCoordinateSystem,
            },
        ));
    }
    if numbered_feature_name_has_family(ctx, kind, "Extrude")?
        && !feature_is_sheet_extrusion(ctx, scan, feature_id)?
    {
        let output_kind = sweep_output_kind(ctx, scan, ir, "extrusion", feature_id)?;
        let op = section_sweep_boolean_operation(
            recipe.map(crate::feature::operations::FeatureRecipe::effect),
            kind,
            output_kind.is_some(),
            preceding_features_establish_body(ctx, ir)?,
        );
        return extrude_feature_definition_with_profile(
            ctx,
            scan,
            ir,
            source_carriers,
            feature_id,
            op,
        );
    }
    if schema_class == Some(SchemaClass::Surface)
        && class_942_boundary_surface_entity_graph(
            ctx,
            feature_id,
            &scan.features.entity_tables,
            &scan.surfaces.rows,
        )?
    {
        return Ok(IrFeatureDefinition::Operation(
            IrFeatureOperation::Unresolved {
                family: UnresolvedFamily::BoundarySurface,
            },
        ));
    }
    if schema_class.and_then(schema_operation_kind).is_none() {
        if let Some(definition) = named_or_referenced_feature_definition(
            ctx,
            scan,
            ir,
            source_carriers,
            feature_id,
            kind,
        )? {
            return Ok(definition);
        }
        if let Some(definition) =
            unbounded_feature_plane_definition(ctx, scan, ir, source_carriers, feature_id)?
        {
            return Ok(definition);
        }
    }
    let kind = kind.into();
    let (text_storage, node_storage, parameters) = feature_parameters(ctx, scan, feature_id)?;
    text_storage.commit()?;
    let parameters = cadmpeg_core::text::named_entries_for_decode(
        ctx,
        format_args!("creo:model:feature#{feature_id}"),
        parameters,
    )?;
    drop(node_storage);
    Ok(IrFeatureDefinition::Operation(IrFeatureOperation::Native {
        kind,
        parameters,
    }))
}

pub(in super::super) fn datum_plane_feature_definition(
    datum: &crate::datum::DatumPlane,
) -> IrFeatureDefinition {
    let normal = datum.normal();
    cadmpeg_ir::features::FeatureDatumPlaneFrame::new(
        Point3::new(
            normal[0] * datum.offset(),
            normal[1] * datum.offset(),
            normal[2] * datum.offset(),
        ),
        Vector3::from(normal),
        cadmpeg_ir::geometry::derive_reference_direction(Vector3::from(normal)),
    )
    .map_or_else(
        || {
            IrFeatureDefinition::Operation(IrFeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumPlane,
            })
        },
        |frame| IrFeatureDefinition::Operation(IrFeatureOperation::DatumPlane { frame }),
    )
}

fn reconciled_datum_plane_definition(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    surface_id: u32,
) -> Result<Option<IrFeatureDefinition>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo datum plane evidence")?;
    let local_planes = scratch.with_storage(|| placed_planes(ctx, scan))?;
    let Some(plane) = reconciled_model_plane(ctx, &local_planes, ir, source_carriers, surface_id)?
    else {
        return Ok(None);
    };
    let normal = Vector3::from(plane.normal);
    let local_surfaces = scratch.with_storage(|| placed_plane_surfaces(ctx, scan))?;
    let local = ctx.get_btree_map(
        &local_surfaces,
        &surface_id,
        "creo datum local surface lookup",
    )?;
    let u_axis = if let Some(surface) = local {
        Vector3::from(surface.u_axis)
    } else {
        let surface = crate::decode::uniqueness::exactly_one_by(
            ctx,
            &ir.model.surfaces,
            |surface| {
                Ok(crate::identity::matches_numbered_identity(
                    surface.id.as_str(),
                    "creo:visibgeom:surface#",
                    surface_id,
                ))
            },
            "creo datum model surface lookup",
        )?;
        match surface.map(|surface| source_carriers.surface_geometry(surface)) {
            Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane))) => {
                *plane.frame().reference().as_raw()
            }
            _ => cadmpeg_ir::geometry::derive_reference_direction(normal),
        }
    };
    Ok(cadmpeg_ir::features::FeatureDatumPlaneFrame::new(
        Point3::from(plane.origin),
        normal,
        u_axis,
    )
    .map(|frame| IrFeatureDefinition::Operation(IrFeatureOperation::DatumPlane { frame })))
}

pub(in super::super) fn unbounded_feature_plane_definition(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
) -> Result<Option<IrFeatureDefinition>, cadmpeg_core::CodecError> {
    let Some(row) = crate::decode::uniqueness::exactly_one_by(
        ctx,
        &*scan.surfaces.rows,
        |row| Ok(row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane),
        "creo unbounded feature plane rows",
    )?
    else {
        return Ok(None);
    };
    if !(row.boundary_type == crate::surface::BoundaryType::Code01
        && row.next_surface == 0
        && crate::surface::unique_surface_row(&scan.surfaces.rows, row.id) == Some(row))
    {
        return Ok(None);
    }
    reconciled_datum_plane_definition(ctx, scan, ir, source_carriers, row.id)
}

pub(in super::super) fn numbered_feature_name_has_family(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    name: &str,
    family: &str,
) -> Result<bool, cadmpeg_core::CodecError> {
    let Some(ordinal) = name
        .strip_prefix(family)
        .and_then(|suffix| suffix.strip_prefix(' '))
    else {
        return Ok(false);
    };
    Ok(!ordinal.is_empty()
        && ctx.all_by(
            ordinal.as_bytes(),
            |byte| Ok(byte.is_ascii_digit()),
            "creo numbered feature ordinal bytes",
        )?)
}

pub(in super::super) fn section_sweep_allows_linear_extrusion(
    schema_class: Option<SchemaClass>,
    recipe: Option<crate::feature::operations::FeatureRecipeKind>,
) -> bool {
    recipe == Some(crate::feature::operations::FeatureRecipeKind::Extrude)
        || (matches!(
            schema_class,
            Some(SchemaClass::Cut | SchemaClass::Protrusion)
        ) && recipe != Some(crate::feature::operations::FeatureRecipeKind::Revolve))
}

pub(in super::super) fn feature_is_sheet_extrusion(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<bool, cadmpeg_core::CodecError> {
    if feature_schema_class(ctx, scan, feature_id)? != Some(SchemaClass::Surface) {
        return Ok(false);
    }
    let Some(name) = feature_reference_name(ctx, scan, feature_id)? else {
        return Ok(false);
    };
    let Ok(name) = ctx.validate_utf8(name, "creo sheet extrusion feature name UTF-8")? else {
        return Ok(false);
    };
    numbered_feature_name_has_family(ctx, name, "Extrude")
}

pub(in super::super) fn feature_allows_linear_extrusion(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<bool, cadmpeg_core::CodecError> {
    let schema_class = feature_schema_class(ctx, scan, feature_id)?;
    if !feature_section_sweep_semantics_conflict(ctx, scan, feature_id)? {
        if let Some(schema_class) = schema_class {
            let recipe = super::operations::feature_recipe(ctx, scan, feature_id)?;
            if section_sweep_allows_linear_extrusion(
                Some(schema_class),
                recipe.map(crate::feature::operations::FeatureRecipe::kind),
            ) {
                return Ok(true);
            }
        }
    }
    feature_is_sheet_extrusion(ctx, scan, feature_id)
}

pub(in super::super) fn feature_allows_additive_linear_extrusion(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<bool, cadmpeg_core::CodecError> {
    if feature_section_sweep_semantics_conflict(ctx, scan, feature_id)?
        || feature_schema_class(ctx, scan, feature_id)? != Some(SchemaClass::Protrusion)
    {
        return Ok(false);
    }
    let recipe = super::operations::feature_recipe(ctx, scan, feature_id)?;
    Ok(section_sweep_allows_linear_extrusion(
        Some(SchemaClass::Protrusion),
        recipe.map(crate::feature::operations::FeatureRecipe::kind),
    ) && recipe
        .map(crate::feature::operations::FeatureRecipe::effect)
        .is_none_or(|effect| effect == crate::feature::operations::FeatureRecipeEffect::Protrude))
}

pub(in super::super) fn preceding_features_establish_body(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
) -> Result<bool, cadmpeg_core::CodecError> {
    ctx.any_by(
        &ir.model.features,
        |feature| {
            Ok(feature.suppressed != Some(true)
                && (!feature.evaluation.outputs().is_empty()
                    || matches!(
                        feature.evaluation.definition(),
                        IrFeatureDefinition::Operation(
                            IrFeatureOperation::Extrude {
                                op: BooleanOp::NewBody,
                                ..
                            } | IrFeatureOperation::Revolve {
                                op: BooleanOp::NewBody,
                                ..
                            }
                        )
                    )))
        },
        "creo prior feature body lookup",
    )
}

pub(in super::super) fn section_sweep_boolean_operation(
    recipe_effect: Option<crate::feature::operations::FeatureRecipeEffect>,
    kind: &str,
    has_evaluated_body: bool,
    prior_body: bool,
) -> BooleanOp {
    match recipe_effect {
        Some(crate::feature::operations::FeatureRecipeEffect::Protrude) if prior_body => {
            BooleanOp::Join
        }
        Some(crate::feature::operations::FeatureRecipeEffect::Protrude) => BooleanOp::NewBody,
        Some(crate::feature::operations::FeatureRecipeEffect::Cut) => BooleanOp::Cut,
        None if kind == "Protrusion" && prior_body => BooleanOp::Join,
        None if kind == "Protrusion" => BooleanOp::NewBody,
        None if kind == "Cut" => BooleanOp::Cut,
        None if has_evaluated_body => BooleanOp::NewBody,
        _ => BooleanOp::Unresolved,
    }
}

pub(in super::super) fn class_942_boundary_surface_entity_graph(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature_id: u32,
    tables: &[crate::feature::entity::FeatureEntityTable],
    surface_rows: &[crate::surface::SurfaceRow],
) -> Result<bool, cadmpeg_core::CodecError> {
    let Some(surface) = crate::decode::uniqueness::exactly_one_by(
        ctx,
        surface_rows,
        |row| Ok(row.feature_id == feature_id),
        "creo boundary surface generated rows",
    )?
    else {
        return Ok(false);
    };
    if !matches!(surface.kind, crate::surface::SurfaceKind::Extrusion(_)) {
        return Ok(false);
    }
    let mut selected = [None; 4];
    let mut rows = tables.iter();
    while let Some(table) = ctx.next_charged(&mut rows, "creo boundary surface entity tables")? {
        if table.feature_id != feature_id {
            continue;
        }
        let slot = match table.table_class_id {
            29 => 0,
            94 => 1,
            67 => 2,
            100 => 3,
            _ => continue,
        };
        if selected[slot].replace(table).is_some() {
            return Ok(false);
        }
    }
    let [Some(generated), Some(topology), Some(owner), Some(output)] = selected else {
        return Ok(false);
    };
    let [owner_entry] = owner.entries.as_slice() else {
        return Ok(false);
    };
    Ok(matches!(
        generated.entries.as_slice(),
        [entry]
            if entry.entity_id == surface.id
                && entry.source_entity_id() == Some(0)
                && generated.surface_ids_iter().eq([surface.id])
    ) && topology
        .entries
        .iter()
        .map(crate::feature::entity::FeatureEntityTableEntry::class_id)
        .eq([221, 222, 220, 220])
        && owner_entry.source_entity_id() == Some(feature_id)
        && matches!(
            output.entries.as_slice(),
            [entry]
                if entry.entity_id == owner_entry.entity_id
                    && entry.class_id() == surface.id
        ))
}

#[cfg(test)]
mod tests;
