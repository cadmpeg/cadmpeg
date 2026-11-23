// SPDX-License-Identifier: Apache-2.0
//! Revolve projection and revolve axis binding.

use crate::design::decode::operands::entity_selections::entity_selection_matches_curve;
use crate::design::feature_project::sweep_loft::fixed_boolean_operation;
use crate::design::feature_project::unique_feature_match;
use crate::ids;
use crate::ids::native_stream;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::sketch_geometry::SketchCurveGeometry;
use crate::records::sketch_geometry::SketchCurveIdentity;
use crate::records::sketch_placement::DesignSketchPlacement;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::edge_identity::DesignEdgeOperand;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;

pub(in crate::design) fn project_fixed_revolve_with_entities(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    face_operands: &[crate::records::topology::face::DesignFaceOperand],
    (placements, curve_identities): (&[DesignSketchPlacement], &[SketchCurveIdentity]),
    design_types: &[crate::records::entity_header::SegmentType],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::extents::AngularTermination;
    use cadmpeg_ir::features::profiles::PlanarProfileRef;
    use cadmpeg_ir::features::revolve::{RevolutionAxis, RevolveConstruction, RevolveExtent};
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let crate::records::feature::scope::DesignScopePayload::Revolve(Some(
        crate::records::feature::path_features::DesignRevolveConstruction {
            operation,
            travel,
            second_side,
            solid,
            ..
        },
    )) = &scope.payload()
    else {
        return None;
    };
    let stream = native_stream(&scope.id)?;
    let mut profile = None;
    let mut axis_group = None;
    let mut targets = Vec::new();
    let mut body_count = 0;
    let mut group_count = 0;
    for group in construction_groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream) && group.scope_record_index == scope.record_index
    }) {
        group_count += 1;
        match group.role() {
            DesignOperandRole::PROFILE => {
                if profile.replace(group).is_some() {
                    return None;
                }
            }
            DesignOperandRole::ROLE_0X21 => {
                if axis_group.replace(group).is_some() {
                    return None;
                }
            }
            DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B => body_count += 1,
            DesignOperandRole::ROLE_0X5 => targets.push(group),
            _ => {}
        }
    }
    let profile = profile?;
    let axis_group = axis_group?;
    let expected_body_groups = usize::from(*operation != DesignExtrudeOperation::NewBody);
    let target_group = |record: &crate::records::identity::Located<u32>| {
        targets
            .iter()
            .find(|group| group.record_index == record.value && group.members().len() == 1)
            .copied()
    };
    let (target_extent, expected_target_groups) = match travel {
        crate::records::feature::path_features::DesignRevolveTravel::Angle(_) => (None, 0),
        crate::records::feature::path_features::DesignRevolveTravel::ToTargets(sides) => {
            let side_one = target_group(&sides.side_one)?;
            match &sides.side_two {
                None => (
                    Some(RevolveExtent::OneSided {
                        termination: AngularTermination::ToShape {
                            target: cadmpeg_ir::features::selections::FaceSelection::Native(
                                side_one.id.clone(),
                            ),
                        },
                    }),
                    1,
                ),
                // The frame stores no side for either target.
                Some(side_two) => {
                    target_group(side_two)?;
                    (
                        Some(RevolveExtent::TwoSided {
                            first: AngularTermination::Unresolved {},
                            second: AngularTermination::Unresolved {},
                        }),
                        2,
                    )
                }
            }
        }
    };
    if body_count != expected_body_groups
        || group_count != 2 + expected_body_groups + expected_target_groups
    {
        return None;
    }
    let [_] = profile.members() else {
        return None;
    };
    let [crate::records::identity::Located {
        value: axis_member, ..
    }] = axis_group.members()
    else {
        return None;
    };
    let mut matching = edge_operands.iter().filter(|operand| {
        native_stream(&operand.id) == Some(stream)
            && operand.scope_record_index == scope.record_index
            && operand.record_index() == *axis_member
    });
    let first = matching.next();
    let second = matching.next();
    let axis = if let (Some(axis_operand), None) = (first, second) {
        axis_operand.resolved_axis.map(|axis| RevolutionAxis {
            origin: axis.origin,
            direction: cadmpeg_ir::features::geometry::FeatureDirection3::from(axis.direction),
            reference: Some(cadmpeg_ir::features::profiles::PathRef::Native(
                axis_operand.id.clone(),
            )),
        })
    } else if first.is_none() {
        let resolved = resolve_sketch_axis_selection(
            scope,
            axis_group,
            *axis_member,
            entity_selection_operands,
            placements,
            curve_identities,
        )
        .or_else(|| {
            design_geometry_axis_selection(
                scope,
                axis_group,
                *axis_member,
                entity_selection_operands,
                placements,
                design_types,
            )
        });
        if resolved.is_none()
            && !unresolved_historical_face_axis_selection(
                scope,
                axis_group,
                *axis_member,
                entity_selection_operands,
            )
            && revolve_face_axis_operand(scope, axis_group, *axis_member, face_operands).is_none()
        {
            return None;
        }
        resolved
    } else {
        return None;
    };
    let revolve_profile: cadmpeg_ir::features::profiles::PlanarProfileRef =
        PlanarProfileRef::Native(profile.id.clone());
    let extent = match (travel, second_side) {
        (crate::records::feature::path_features::DesignRevolveTravel::ToTargets(_), _) => {
            target_extent?
        }
        (crate::records::feature::path_features::DesignRevolveTravel::Angle(angle), None) => {
            RevolveExtent::OneSided {
                termination: AngularTermination::Angle { angle: *angle },
            }
        }
        // Fusion's symmetric angle applies to each side; the neutral extent
        // states the total travel.
        (crate::records::feature::path_features::DesignRevolveTravel::Angle(angle), Some(side))
            if side.symmetric =>
        {
            RevolveExtent::Symmetric {
                termination: AngularTermination::Angle {
                    angle: cadmpeg_ir::scalar::PositiveAngle::new(2.0 * angle.get())?,
                },
            }
        }
        (crate::records::feature::path_features::DesignRevolveTravel::Angle(angle), Some(side)) => {
            RevolveExtent::TwoSided {
                first: AngularTermination::Angle { angle: *angle },
                second: AngularTermination::Angle { angle: side.angle },
            }
        }
    };
    Some(FeatureDefinition::Operation(FeatureOperation::Revolve {
        construction: match axis {
            Some(axis) => RevolveConstruction::Resolved {
                profile: revolve_profile,
                axis,
                extent,
                solid: *solid,
                face_maker: None,
                fuse_order: None,
                allow_multi_profile_faces: None,
            },
            None => RevolveConstruction::Unresolved(
                cadmpeg_ir::features::revolve::PartialRevolveConstruction::Axis {
                    profile: revolve_profile,
                    extent: Some(extent),
                    solid: *solid,
                    face_maker: None,
                    fuse_order: None,
                    allow_multi_profile_faces: None,
                },
            ),
        },
        op: fixed_boolean_operation(*operation),
    }))
}

pub(super) fn unresolved_historical_face_axis_selection(
    scope: &DesignParameterScope,
    axis_group: &DesignConstructionOperandGroup,
    axis_member: u32,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) -> bool {
    let stream = native_stream(&scope.id);
    let selection = unique_feature_match(entity_selection_operands.iter().filter(|operand| {
        native_stream(&operand.id) == stream
            && operand.scope_record_index == scope.record_index
            && operand.group_record_index == axis_group.record_index
            && operand.group_member_ordinal == 0
            && operand.record_index() == axis_member
    }));
    selection.is_some_and(|selection| !selection.historical_face_candidates.is_empty())
}

pub(super) fn revolve_face_axis_operand<'a>(
    scope: &DesignParameterScope,
    axis_group: &DesignConstructionOperandGroup,
    axis_member: u32,
    face_operands: &'a [crate::records::topology::face::DesignFaceOperand],
) -> Option<&'a crate::records::topology::face::DesignFaceOperand> {
    let stream = native_stream(&scope.id);
    let operand = unique_feature_match(face_operands.iter().filter(|operand| {
        native_stream(&operand.id) == stream
            && operand.scope_record_index == scope.record_index
            && operand.group_record_index() == Some(axis_group.record_index)
            && operand.group_member_ordinal() == Some(0)
            && operand.record_index() == axis_member
    }))?;
    (crate::design::face_resolve::historical_face_operand_candidate_iter(operand)
        .next()
        .is_some())
    .then_some(operand)
}

/// Orient partial-turn axes using the source sweep convention.
pub(crate) fn orient_revolve_axes(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    sketch_entities: &[cadmpeg_ir::sketches::SketchEntity],
    (linear_tolerance, angular_tolerance): (f64, f64),
) {
    use cadmpeg_ir::features::extents::AngularTermination;
    use cadmpeg_ir::features::profiles::PlanarProfileRef;
    use cadmpeg_ir::features::revolve::RevolveExtent;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    for feature in features {
        let reversed = scopes.iter().any(|scope| {
            feature.native_ref.as_deref() == Some(scope.id.as_str())
                && matches!(
                    scope.payload(),
                    crate::records::feature::scope::DesignScopePayload::Revolve(Some(
                        crate::records::feature::path_features::DesignRevolveConstruction {
                            reversed: true,
                            ..
                        }
                    ))
                )
        });
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Revolve { construction, .. }) =
                definition
            else {
                return;
            };
            let Some(axis) = construction.axis().cloned() else {
                return;
            };
            if matches!(
                construction.extent(),
                Some(RevolveExtent::OneSided {
                    termination: AngularTermination::Angle { angle },
                }) if *angle == cadmpeg_ir::scalar::PositiveAngle::FULL_TURN
            ) {
                return;
            }
            let oriented = construction.profile().and_then(|profile| {
                let (PlanarProfileRef::Sketch(sketch_id)
                | PlanarProfileRef::SketchProfiles {
                    sketch: sketch_id, ..
                }
                | PlanarProfileRef::SketchRegions {
                    sketch: sketch_id, ..
                }
                | PlanarProfileRef::SketchEntities {
                    sketch: sketch_id, ..
                }) = profile
                else {
                    return None;
                };
                sketches
                    .iter()
                    .find(|sketch| &sketch.id == sketch_id)
                    .and_then(|sketch| {
                        oriented_revolve_axis(
                            &axis,
                            sketch,
                            profile,
                            sketch_entities,
                            reversed,
                            (linear_tolerance, angular_tolerance),
                        )
                    })
            });
            construction.set_axis(oriented);
        });
    }
}

pub(super) fn selected_revolve_profile_entities<'a>(
    profile: &'a cadmpeg_ir::features::profiles::PlanarProfileRef,
    sketch: &'a cadmpeg_ir::sketches::Sketch,
) -> Option<Vec<&'a cadmpeg_ir::sketches::SketchEntityId>> {
    use cadmpeg_ir::features::profiles::{PlanarProfileRef, SketchProfileRegion};
    let mut entities = Vec::new();
    let mut add_loop = |index: u32| {
        let loop_ = sketch.profiles.get(usize::try_from(index).ok()?)?;
        entities.extend(loop_.iter().map(|use_| &use_.entity));
        Some(())
    };
    match profile {
        PlanarProfileRef::Sketch(_) => {
            entities.extend(sketch.profiles.iter().flatten().map(|use_| &use_.entity));
        }
        PlanarProfileRef::SketchProfiles { profiles, .. } => {
            for index in profiles {
                add_loop(*index)?;
            }
        }
        PlanarProfileRef::SketchRegions { regions, .. } => {
            for region in regions.iter() {
                if let SketchProfileRegion::Loops { loops } = region {
                    add_loop(loops.outer())?;
                    for index in loops.holes() {
                        add_loop(*index)?;
                    }
                }
            }
            for region in regions.iter() {
                if let SketchProfileRegion::Trimmed {
                    outer_boundary,
                    hole_boundaries,
                } = region
                {
                    entities.extend(
                        outer_boundary
                            .iter()
                            .chain(hole_boundaries.iter().flatten())
                            .map(|use_| &use_.entity),
                    );
                }
            }
        }
        PlanarProfileRef::SketchEntities {
            entities: selected, ..
        } => {
            entities.extend(selected.iter());
        }
        _ => return None,
    }
    Some(entities)
}

/// The axis directed so that a right-handed turn carries the selected profile
/// toward the sketch normal, or toward its reverse when `reversed`, or
/// `None` when the axis is not parallel to the sketch plane or the profile does
/// not lie on one side of the axis.
pub(super) fn oriented_revolve_axis(
    axis: &cadmpeg_ir::features::revolve::RevolutionAxis,
    sketch: &cadmpeg_ir::sketches::Sketch,
    profile: &cadmpeg_ir::features::profiles::PlanarProfileRef,
    sketch_entities: &[cadmpeg_ir::sketches::SketchEntity],
    reversed: bool,
    (linear_tolerance, angular_tolerance): (f64, f64),
) -> Option<cadmpeg_ir::features::revolve::RevolutionAxis> {
    let (origin, normal, u_axis) = sketch.resolved_placement()?;
    let normal = normal.get().unit()?;
    let u_axis = u_axis.get().unit()?;
    let v_axis = normal.cross(u_axis);
    let direction = axis.direction.get().unit()?;
    if direction.dot(normal).abs() > angular_tolerance {
        return None;
    }
    // In-plane and perpendicular to the axis: its sign names the profile side.
    let across = normal.cross(direction);
    let mut positive_side = None;
    for entity_id in selected_revolve_profile_entities(profile, sketch)? {
        let entity = sketch_entities
            .iter()
            .find(|entity| entity.id() == entity_id)?;
        for point in revolve_profile_sample_points(&entity.geometry) {
            let offset = origin
                .get()
                .translated(u_axis, point.u)
                .translated(v_axis, point.v)
                .vector_from(axis.origin.get())
                .dot(across);
            // A profile can touch its axis; only points off the axis name a side.
            if offset.abs() <= linear_tolerance {
                continue;
            }
            if *positive_side.get_or_insert(offset > 0.0) != (offset > 0.0) {
                return None;
            }
        }
    }
    Some(cadmpeg_ir::features::revolve::RevolutionAxis {
        direction: if positive_side? == reversed {
            // Reversing a zero component gives -0.0; keep it a plain zero.
            cadmpeg_ir::features::geometry::FeatureDirection3::from_unit_without_small_components(
                cadmpeg_ir::units::UnitVector3::normalized(axis.direction.reversed().get())?,
            )
        } else {
            axis.direction
        },
        ..axis.clone()
    })
}

/// Sketch points that lie on a profile curve or, for a closed conic, inside
/// it. A revolve profile does not cross its axis, so each of them names the
/// profile side. Other curve kinds contribute no point.
pub(super) fn revolve_profile_sample_points(
    geometry: &cadmpeg_ir::sketches::SketchGeometry,
) -> Vec<cadmpeg_ir::math::Point2> {
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::sketches::SketchGeometryDefinition as Definition;

    match geometry.definition().to_raw() {
        Definition::Line { start, end } => vec![start, end],
        Definition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => [start_angle, end_angle]
            .into_iter()
            .map(|angle| {
                Point2::new(
                    center.u + radius.get() * angle.get().cos(),
                    center.v + radius.get() * angle.get().sin(),
                )
            })
            .collect(),
        Definition::Circle { center, .. } => vec![center],
        Definition::Ellipse { center, bounds, .. } if bounds.is_none() => vec![center],
        _ => Vec::new(),
    }
}

/// Resolve Revolve axes selected through history-qualified analytic faces.
pub(crate) fn bind_revolve_face_axes(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    construction_groups: &[DesignConstructionOperandGroup],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    face_operands: &[crate::records::topology::face::DesignFaceOperand],
    (faces, surfaces): (
        &[cadmpeg_ir::topology::Face],
        &[cadmpeg_ir::geometry::Surface],
    ),
    design_types: &[crate::records::entity_header::SegmentType],
) {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    for feature in features {
        let native_ref = feature.native_ref.as_deref();
        feature.evaluation.edit(|definition, _| 'feature_edit: {
            let FeatureDefinition::Operation(FeatureOperation::Revolve { construction, .. }) =
                definition
            else {
                break 'feature_edit;
            };
            if construction.axis().is_some() {
                break 'feature_edit;
            }
            let Some(scope) = native_ref
                .and_then(|native_ref| scopes.iter().find(|scope| scope.id == native_ref))
            else {
                break 'feature_edit;
            };
            let Some(stream) = native_stream(&scope.id) else {
                break 'feature_edit;
            };
            let Some(group) = unique_feature_match(construction_groups.iter().filter(|group| {
                native_stream(&group.id) == Some(stream)
                    && group.scope_record_index == scope.record_index
                    && group.role() == DesignOperandRole::ROLE_0X21
            })) else {
                break 'feature_edit;
            };
            let [crate::records::identity::Located { value: member, .. }] = group.members() else {
                break 'feature_edit;
            };
            let selection =
                unique_feature_match(entity_selection_operands.iter().filter(|operand| {
                    native_stream(&operand.id) == Some(stream)
                        && operand.scope_record_index == scope.record_index
                        && operand.group_record_index == group.record_index
                        && operand.group_member_ordinal == 0
                        && operand.record_index() == *member
                }));
            let entity_face_slot = match selection {
                Some(selection)
                    if crate::design::decode::meta::classify_origin_entity(
                        design_types,
                        stream,
                        selection.primary_identity,
                    ) == crate::design::decode::meta::OriginClassification::NotOrigin =>
                {
                    selection
                        .historical_face_candidates
                        .first()
                        .map(|candidate| candidate.face_slot)
                        .filter(|slot| {
                            selection
                                .historical_face_candidates
                                .iter()
                                .all(|candidate| candidate.face_slot == *slot)
                        })
                }
                _ => None,
            };
            let entity_axis = entity_face_slot.and_then(|face_slot| {
                analytic_axis_for_face(&ids::brep_face_id(face_slot), faces, surfaces)
            });
            let recipe_axis = revolve_face_axis_operand(scope, group, *member, face_operands)
                .and_then(|operand| {
                    let first_face =
                        crate::design::face_resolve::historical_face_operand_candidate_iter(
                            operand,
                        )
                        .min_by(|left, right| left.as_str().cmp(right.as_str()))?;
                    let first = analytic_axis_for_face(first_face, faces, surfaces)?;
                    crate::design::face_resolve::historical_face_operand_candidate_iter(operand)
                        .map(|face_id| analytic_axis_for_face(face_id, faces, surfaces))
                        .all(|axis| {
                            axis.is_some_and(|axis| {
                                crate::history::body_closures::same_axis_line(
                                    (first.origin.get(), first.direction.get()),
                                    (axis.origin.get(), axis.direction.get()),
                                )
                            })
                        })
                        .then_some(first)
                });
            let axis = match (entity_axis, recipe_axis) {
                (Some(entity), Some(recipe))
                    if crate::history::body_closures::same_axis_line(
                        (entity.origin.get(), entity.direction.get()),
                        (recipe.origin.get(), recipe.direction.get()),
                    ) =>
                {
                    Some(entity)
                }
                (Some(axis), None) | (None, Some(axis)) => Some(axis),
                _ => None,
            };
            construction.set_axis(axis.map(|mut axis| {
                axis.reference = Some(cadmpeg_ir::features::profiles::PathRef::Native(
                    group.id.clone(),
                ));
                axis
            }));
        });
    }
}

pub(super) fn analytic_axis_for_face(
    face_id: &cadmpeg_ir::ids::FaceId,
    faces: &[cadmpeg_ir::topology::Face],
    surfaces: &[cadmpeg_ir::geometry::Surface],
) -> Option<cadmpeg_ir::features::revolve::RevolutionAxis> {
    let face = unique_feature_match(faces.iter().filter(|face| &face.id == face_id))?;
    let surface =
        unique_feature_match(surfaces.iter().filter(|surface| surface.id == face.surface))?;
    analytic_surface_axis(&surface.geometry)
}

pub(super) fn analytic_surface_axis(
    geometry: &cadmpeg_ir::geometry::SurfaceGeometry,
) -> Option<cadmpeg_ir::features::revolve::RevolutionAxis> {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};

    let (origin, direction) = match geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
            (plane_surface.origin(), *plane_surface.frame().axis())
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
            (cylinder_surface.origin(), *cylinder_surface.frame().axis())
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
            (cone_surface.origin(), *cone_surface.frame().axis())
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
            (torus_surface.center(), *torus_surface.frame().axis())
        }
        _ => return None,
    };
    Some(cadmpeg_ir::features::revolve::RevolutionAxis {
        origin,
        direction: cadmpeg_ir::features::geometry::FeatureDirection3::from(direction),
        reference: None,
    })
}

/// Resolve a Revolve axis selection that names a Design Geometry origin axis.
///
/// The type table identifies the origin axis, and no Sketch placement in the
/// stream owns its identity. A body edge or face axis is a recipe operand, so
/// an ASM topology identity with the same value is a different entity.
pub(super) fn design_geometry_axis_selection(
    scope: &DesignParameterScope,
    axis_group: &DesignConstructionOperandGroup,
    axis_member: u32,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    placements: &[DesignSketchPlacement],
    design_types: &[crate::records::entity_header::SegmentType],
) -> Option<cadmpeg_ir::features::revolve::RevolutionAxis> {
    let stream = native_stream(&scope.id)?;
    let selection = unique_feature_match(entity_selection_operands.iter().filter(|operand| {
        native_stream(&operand.id) == Some(stream)
            && operand.scope_record_index == scope.record_index
            && operand.group_record_index == axis_group.record_index
            && operand.group_member_ordinal == 0
            && operand.record_index() == axis_member
    }))?;
    if placements.iter().any(|placement| {
        native_stream(&placement.id) == Some(stream)
            && placement.entity_id.suffix() == selection.primary_identity
    }) {
        return None;
    }
    let crate::design::decode::meta::OriginClassification::Origin(
        crate::design::decode::meta::OriginEntity::Axis(ordinal),
    ) = crate::design::decode::meta::classify_origin_entity(
        design_types,
        stream,
        selection.primary_identity,
    )
    else {
        return None;
    };
    let direction = match ordinal {
        0 => cadmpeg_ir::units::UnitVector3::X_AXIS,
        1 => cadmpeg_ir::units::UnitVector3::Y_AXIS,
        2 => cadmpeg_ir::units::UnitVector3::Z_AXIS,
        _ => return None,
    };
    Some(cadmpeg_ir::features::revolve::RevolutionAxis {
        origin: cadmpeg_ir::features::geometry::FinitePoint3::ZERO,
        direction: cadmpeg_ir::features::geometry::FeatureDirection3::from(direction),
        reference: Some(cadmpeg_ir::features::profiles::PathRef::Native(
            selection.id.clone(),
        )),
    })
}

pub(super) fn resolve_sketch_axis_selection(
    scope: &DesignParameterScope,
    axis_group: &DesignConstructionOperandGroup,
    axis_member: u32,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    placements: &[DesignSketchPlacement],
    curve_identities: &[SketchCurveIdentity],
) -> Option<cadmpeg_ir::features::revolve::RevolutionAxis> {
    let stream = native_stream(&scope.id)?;
    let selection = unique_feature_match(entity_selection_operands.iter().filter(|operand| {
        native_stream(&operand.id) == Some(stream)
            && operand.scope_record_index == scope.record_index
            && operand.group_record_index == axis_group.record_index
            && operand.group_member_ordinal == 0
            && operand.record_index() == axis_member
    }))?;
    let owner_reference = u32::try_from(selection.primary_identity).ok()?;
    let placement = unique_feature_match(placements.iter().filter(|placement| {
        native_stream(&placement.id) == Some(stream)
            && placement.entity_id.suffix() == selection.primary_identity
    }))?;
    let curve = unique_feature_match(curve_identities.iter().filter(|curve| {
        native_stream(&curve.id) == Some(stream)
            && curve.owner_reference == Some(owner_reference)
            && entity_selection_matches_curve(selection, curve)
    }))?;
    let SketchCurveGeometry::Line {
        start, direction, ..
    } = curve.geometry.as_ref()?
    else {
        return None;
    };
    let start = start.as_raw();
    let direction = direction.as_raw();
    let origin_scale = crate::design::face_resolve::placement_origin_scale(placement);
    let origin = Point3::new(
        placement.transform()[0][0] * start.x
            + placement.transform()[0][1] * start.y
            + placement.transform()[0][2] * start.z
            + placement.transform()[0][3] * origin_scale,
        placement.transform()[1][0] * start.x
            + placement.transform()[1][1] * start.y
            + placement.transform()[1][2] * start.z
            + placement.transform()[1][3] * origin_scale,
        placement.transform()[2][0] * start.x
            + placement.transform()[2][1] * start.y
            + placement.transform()[2][2] * start.z
            + placement.transform()[2][3] * origin_scale,
    );
    let direction = Vector3::new(
        placement.transform()[0][0] * direction.x
            + placement.transform()[0][1] * direction.y
            + placement.transform()[0][2] * direction.z,
        placement.transform()[1][0] * direction.x
            + placement.transform()[1][1] * direction.y
            + placement.transform()[1][2] * direction.z,
        placement.transform()[2][0] * direction.x
            + placement.transform()[2][1] * direction.y
            + placement.transform()[2][2] * direction.z,
    );
    Some(cadmpeg_ir::features::revolve::RevolutionAxis {
        origin: cadmpeg_ir::features::geometry::FinitePoint3::new(origin)?,
        direction: cadmpeg_ir::features::geometry::FeatureDirection3::from(
            cadmpeg_ir::units::UnitVector3::normalized_by_reciprocal(direction)?,
        ),
        reference: Some(cadmpeg_ir::features::profiles::PathRef::Native(
            selection.id.clone(),
        )),
    })
}
