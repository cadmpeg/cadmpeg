// SPDX-License-Identifier: Apache-2.0
//! Hole projection.

use crate::design::face_resolve::design_angle;
use crate::design::face_resolve::resolved_direct_face_selection;
use crate::design::feature_project::design_positive_length;
use crate::design::feature_project::single_operand_group;
use crate::design::feature_project::unique_feature_match;
use crate::design::feature_project::DESIGN_LENGTH_TO_MILLIMETRE;
use crate::ids::native_stream;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameter;

use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::face::DesignFaceOperand;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;
use std::collections::HashSet;

/// Bind the sketch points that place a sketch-point `Hole` as its profile
/// after the sketch arenas have been projected. Every location names one
/// planar sketch point, and all of them belong to one sketch.
pub(crate) fn bind_hole_sketch_point_profiles(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    sketch_entities: &[cadmpeg_ir::sketches::SketchEntity],
) {
    use crate::records::feature::work_geometry::DesignWorkPointInputCarrier;
    use cadmpeg_ir::features::holes::HoleProfileFilter;
    use cadmpeg_ir::features::profiles::PlanarProfileRef;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    for feature in features.iter_mut() {
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Hole {
                profile: profile @ None,
                profile_filter,
                ..
            }) = definition
            else {
                return;
            };
            let Some(construction) = feature
                .native_ref
                .as_deref()
                .and_then(|native_ref| scopes.iter().find(|scope| scope.id == native_ref))
                .and_then(DesignParameterScope::hole_construction)
            else {
                return;
            };
            if !construction.placed_by_sketch_points() || construction.entry_points().is_none() {
                return;
            }
            let Some(points) = construction
                .locations
                .iter()
                .map(|location| {
                    let DesignWorkPointInputCarrier::SketchPoint { selection } =
                        location.vertex_input.as_ref()?.carrier()?
                    else {
                        return None;
                    };
                    let mut matches = sketch_entities.iter().filter(|entity| {
                        entity.native_ref.as_deref() == Some(selection.point_native_id.as_str())
                    });
                    match (matches.next(), matches.next()) {
                        (Some(entity), None) => Some(entity),
                        _ => None,
                    }
                })
                .collect::<Option<Vec<_>>>()
            else {
                return;
            };
            let Some(sketch) = points.first().map(|entity| &entity.sketch) else {
                return;
            };
            if points.iter().any(|entity| &entity.sketch != sketch) {
                return;
            }
            let Ok(selected) = PlanarProfileRef::sketch_entities(
                sketch.clone(),
                points.iter().map(|entity| entity.id().clone()).collect(),
            ) else {
                return;
            };
            *profile = Some(selected);
            *profile_filter = Some(HoleProfileFilter::Points);
        });
    }
}

/// Tap type of a simple Hole.
pub(super) const HOLE_TAP_SIMPLE: u32 = 0;

/// Tap type of a clearance Hole.
pub(super) const HOLE_TAP_CLEARANCE: u32 = 1;

/// Tap type of a tapped Hole.
pub(super) const HOLE_TAP_TAPPED: u32 = 2;

/// Tap type of a taper-tapped Hole.
pub(super) const HOLE_TAP_TAPER_TAPPED: u32 = 3;

/// The loss a Hole with tap type `tap_type` reports.
pub(in crate::design) fn hole_tap_type_loss(
    scope_id: &str,
    tap_type: u32,
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    match tap_type {
        HOLE_TAP_SIMPLE | HOLE_TAP_CLEARANCE => None,
        HOLE_TAP_TAPPED => Some(crate::loss::F3dLossCode::HoleTappedDiameterStored.note(format!(
            "Tapped Hole {scope_id} keeps its stored HoleDiameter; Fusion models the bore diameter that its thread size gives."
        ))),
        HOLE_TAP_TAPER_TAPPED => Some(crate::loss::F3dLossCode::HoleTaperTappedBoreCylindrical.note(format!(
            "Taper-tapped Hole {scope_id} keeps a cylindrical bore of its stored HoleDiameter; Fusion models the tapered bore that its thread size gives."
        ))),
        other => Some(crate::loss::F3dLossCode::HoleExtentLeadingWordAssumed.note(format!(
            "Hole {scope_id} assumes its scalar geometry is independent of tap type {other} (DR-106)."
        ))),
    }
}

pub(super) fn hole_extent_loss(scope: &DesignParameterScope) -> cadmpeg_ir::report::loss::LossNote {
    crate::loss::F3dLossCode::HoleExtentUnproven.note(format!(
        "Hole {} retains its native extent because its controls, depth or face operands do not define an admitted extent.", scope.id
    ))
}

/// The face that ends an Extents To Hole. Its sole
/// role-`0x0000000500000000` group has one member. When that member is the
/// one direct `face` recipe operand of the scope at its record and resolves
/// one face of the preceding state, that face ends the hole.
/// Otherwise the group stays the native selection.
pub(super) fn hole_extent_face(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::selections::FaceSelection> {
    let group = single_operand_group(groups, scope, DesignOperandRole::ROLE_0X5)?;
    let [member] = group.members() else {
        return None;
    };
    let stream = native_stream(&scope.id);
    let resolved = unique_feature_match(face_operands.iter().filter(|operand| {
        native_stream(&operand.id) == stream
            && operand.scope_record_index == scope.record_index
            && operand.record_index() == member.value
    }))
    .filter(|operand| {
        operand.group_record_index().is_none()
            && operand.recipe_kind == crate::records::recipes::ConstructionRecipeKind::Face
    })
    .and_then(|operand| match operand.resolved_face_slots.as_slice() {
        [face] => crate::design::face_resolve::historical_face_selection_with_native(
            scope,
            scope.previous_history_state_id()?,
            vec![*face],
            group.id.clone(),
        ),
        _ => None,
    });
    Some(resolved.unwrap_or_else(|| {
        cadmpeg_ir::features::selections::FaceSelection::Native(group.id.clone())
    }))
}

pub(super) fn hole_termination(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
) -> Option<cadmpeg_ir::features::extents::LinearTermination> {
    use cadmpeg_ir::features::extents::LinearTermination;
    let extent = scope.hole_extent()?;
    let mut depths = parameters
        .iter()
        .filter(|(_, p)| p.source_kind() == "HoleDepth");
    let depth = depths.next();
    if depths.next().is_some() {
        return None;
    }
    match extent.extent_control {
        Some(0) if extent.saved_non_distance_depth.is_some() && depth.is_none() => {
            Some(LinearTermination::ToFace {
                face: hole_extent_face(scope, groups, face_operands)?,
                offset: None,
            })
        }
        Some(0) | None => Some(LinearTermination::Blind {
            length: design_positive_length(depth?.1)?.into(),
        }),
        Some(1)
            if extent.saved_non_distance_depth.is_some()
                && depth.is_none()
                && hole_start_faces_only(scope, face_operands)? =>
        {
            Some(LinearTermination::ThroughAll {})
        }
        _ => None,
    }
}

/// Whether the face operands of a Hole scope name at most its start face: no
/// operand (a sketch-point Hole); a direct `face` or `bounded_face` recipe
/// followed by a `bounded_face` recipe; or, on a face-point carrier, a `face`
/// or `bounded_face` recipe followed by three `bounded_face` recipes at
/// consecutive ordinals. Each operand is bound to its scope reference entry.
/// A further face operand, or a direct persistent face selection on a carrier
/// that sketch points do not place, can name an extent face, so it refuses the
/// form.
pub(super) fn hole_start_faces_only(
    scope: &DesignParameterScope,
    face_operands: &[DesignFaceOperand],
) -> Option<bool> {
    use crate::records::recipes::ConstructionRecipeKind;
    let stream = native_stream(&scope.id)?;
    if scope.hole_construction().is_some_and(|construction| {
        construction.face_selection.is_some() && !construction.placed_by_sketch_points()
    }) {
        return Some(false);
    }
    let mut faces: Vec<_> = face_operands
        .iter()
        .filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == scope.record_index
        })
        .collect();
    faces.sort_by_key(|operand| operand.scope_reference_ordinal);
    let bound = |operand: &DesignFaceOperand| {
        operand.group_record_index().is_none()
            && usize::try_from(operand.scope_reference_ordinal)
                .ok()
                .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                == Some(&operand.record_index())
    };
    let start_run = |faces: &[&DesignFaceOperand]| {
        faces.split_first().is_some_and(|(first, rest)| {
            matches!(
                first.recipe_kind,
                ConstructionRecipeKind::Face | ConstructionRecipeKind::BoundedFace
            ) && rest
                .iter()
                .all(|operand| operand.recipe_kind == ConstructionRecipeKind::BoundedFace)
        }) && faces.iter().all(|operand| bound(operand))
    };
    Some(match faces.as_slice() {
        [] => true,
        [first, second] => {
            first.scope_reference_ordinal < second.scope_reference_ordinal && start_run(&faces)
        }
        [first, .., last] if faces.len() == 4 => {
            scope.hole_construction().is_some_and(
                crate::records::feature::hole::DesignHoleConstruction::placed_at_face_point,
            ) && last
                .scope_reference_ordinal
                .checked_sub(first.scope_reference_ordinal)
                == Some(3)
                && start_run(&faces)
        }
        _ => false,
    })
}

pub(super) fn hole_placements(
    scope: &DesignParameterScope,
) -> Option<Vec<cadmpeg_ir::features::holes::HolePlacement>> {
    let construction = scope.hole_construction()?;
    let control = scope
        .hole_extent()
        .and_then(|extent| extent.direction_control.as_ref())
        .map(|control| control.value);
    let [x, y, z] = match control {
        Some(control) => construction.drilling_direction(control)?,
        None => construction
            .direction
            .map(cadmpeg_ir::scalar::FiniteReal::get),
    };
    let direction = cadmpeg_ir::features::geometry::FeatureDirection3::new(Vector3::new(x, y, z))?;
    construction
        .entry_points()?
        .into_iter()
        .map(|point| {
            let position = cadmpeg_ir::features::geometry::FinitePoint3::new(Point3::new(
                point[0].get() * DESIGN_LENGTH_TO_MILLIMETRE,
                point[1].get() * DESIGN_LENGTH_TO_MILLIMETRE,
                point[2].get() * DESIGN_LENGTH_TO_MILLIMETRE,
            ))?;
            Some(match control {
                Some(_) => cadmpeg_ir::features::holes::HolePlacement::Directed {
                    position,
                    direction,
                },
                None => cadmpeg_ir::features::holes::HolePlacement::Axis {
                    origin: position,
                    axis: direction,
                },
            })
        })
        .collect()
}

pub(super) fn project_hole(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::extents::LinearTermination;
    use cadmpeg_ir::features::holes::{HoleBottom, HoleForm, HoleKind, PartialPair};
    use cadmpeg_ir::features::selections::FaceSelection;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::Hole {
        return Ok(None);
    }
    let extent = hole_termination(scope, parameters, groups, face_operands).unwrap_or_else(|| {
        losses.push(hole_extent_loss(scope));
        LinearTermination::Unresolved {}
    });
    let parameter = |source_kind: &str| {
        let mut matches = parameters
            .iter()
            .filter(|(_, parameter)| parameter.source_kind() == source_kind)
            .map(|(_, parameter)| *parameter);
        let parameter = matches.next()?;
        matches.next().is_none().then_some(parameter)
    };
    let depth = match &extent {
        LinearTermination::Blind { length } => Some(length.get()),
        _ => None,
    };
    let diameter = parameter("HoleDiameter").and_then(design_positive_length);
    let tip_angle = parameter("TipAngle")
        .and_then(design_angle)
        .filter(|angle| angle.get() > 0.0 && angle.get() <= std::f64::consts::PI);
    let drill_point_angle =
        tip_angle.and_then(|angle| cadmpeg_ir::scalar::InteriorAngle::try_from(angle).ok());
    let bottom = tip_angle.map(|_| {
        drill_point_angle.map_or(HoleBottom::Flat, |included_angle| HoleBottom::Angled {
            included_angle,
            depth_to_tip: false,
        })
    });
    let has_role = |roles: &[&str]| {
        parameters
            .iter()
            .any(|(_, parameter)| roles.contains(&parameter.source_kind_str()))
    };
    let counterbore_parameters = has_role(&["CBDiameter", "CBDepth"]);
    let countersink_parameters = has_role(&["CSDiameter", "CSAngle"]);
    let hole_type = scope
        .hole_extent()
        .and_then(|extent| extent.hole_type.as_ref())
        .map(|hole_type| hole_type.value);
    let (counterbore, countersink) = match hole_type {
        Some(0) => (false, false),
        Some(1) => (true, false),
        Some(2) => (false, true),
        _ => (counterbore_parameters, countersink_parameters),
    };
    let type_association = match hole_type {
        Some(0) => !counterbore_parameters && !countersink_parameters,
        Some(1) => !countersink_parameters,
        Some(2) => !counterbore_parameters,
        _ => scope
            .hole_extent()
            .is_none_or(|extent| extent.leading_word.is_none()),
    };
    let kind = match (counterbore, countersink) {
        (false, false) if diameter.is_none() && tip_angle.is_none() && hole_type.is_none() => {
            HoleKind::Unresolved(None)
        }
        (false, false) => drill_point_angle.map_or(HoleKind::Simple, |drill_point_angle| {
            HoleKind::SimpleDrilled { drill_point_angle }
        }),
        (true, false) => match (
            parameter("CBDiameter").and_then(design_positive_length),
            parameter("CBDepth").and_then(design_positive_length),
        ) {
            (Some(entry_diameter), Some(entry_depth))
                if diameter.is_some_and(|bore| entry_diameter.get() > bore.get())
                    && depth.is_none_or(|depth| entry_depth.get() <= depth) =>
            {
                drill_point_angle.map_or(
                    HoleKind::Counterbore {
                        diameter: entry_diameter,
                        depth: entry_depth,
                    },
                    |drill_point_angle| HoleKind::CounterboreDrilled {
                        diameter: entry_diameter,
                        depth: entry_depth,
                        drill_point_angle,
                    },
                )
            }
            (Some(diameter), None) => HoleKind::PartialCounterbore(PartialPair::First(diameter)),
            (None, Some(depth)) => HoleKind::PartialCounterbore(PartialPair::Second(depth)),
            _ => HoleKind::Unresolved(Some(HoleForm::Counterbore)),
        },
        (false, true) => match (
            parameter("CSDiameter").and_then(design_positive_length),
            parameter("CSAngle")
                .and_then(design_angle)
                .and_then(|angle| cadmpeg_ir::scalar::InteriorAngle::try_from(angle).ok()),
        ) {
            (Some(entry_diameter), Some(angle))
                if diameter.is_some_and(|bore| entry_diameter.get() > bore.get()) =>
            {
                HoleKind::Countersink {
                    diameter: entry_diameter,
                    angle,
                }
            }
            (Some(diameter), None) => HoleKind::PartialCountersink(PartialPair::First(diameter)),
            (None, Some(angle)) => HoleKind::PartialCountersink(PartialPair::Second(angle)),
            _ => HoleKind::Unresolved(Some(HoleForm::Countersink)),
        },
        (true, true) => HoleKind::Unresolved(None),
    };
    if diameter.is_none() || bottom.is_none() || kind.is_unresolved() {
        losses.push(crate::loss::F3dLossCode::HoleShapePartialAssumed.note(format!(
            "Hole {} assumes its admitted dimensions can be retained independently while its complete entry or bottom form remains unresolved (DR-470).", scope.id
        )));
    }
    let reference_dimensions = has_role(&["RefDim1", "RefDim2"]);
    let edge_groups = groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == native_stream(&scope.id)
                && group.scope_record_index == scope.record_index
                && group.role() == DesignOperandRole::ROLE_0X21
        })
        .count();
    let reference_constraints = match (parameter("RefDim1"), parameter("RefDim2")) {
        (None, None) => !reference_dimensions && edge_groups == 0,
        (Some(first), None) => edge_groups == 1 && design_positive_length(first).is_some(),
        (Some(first), Some(second)) => {
            edge_groups == 2
                && design_positive_length(first).is_some()
                && design_positive_length(second).is_some()
        }
        (None, Some(_)) => false,
    };
    let mut roles = HashSet::new();
    let mut names = HashSet::new();
    let independent_parameters = parameters.iter().any(|(_, parameter)| {
        !matches!(
            parameter.source_kind_str(),
            "HoleDepth"
                | "HoleDiameter"
                | "TipAngle"
                | "CBDiameter"
                | "CBDepth"
                | "CSDiameter"
                | "CSAngle"
                | "RefDim1"
                | "RefDim2"
        ) || !roles.insert(parameter.source_kind_str())
            || !names.insert(parameter.name())
    });
    if matches!(&extent, LinearTermination::Unresolved {})
        || independent_parameters
        || !reference_constraints
        || !type_association
    {
        losses.push(crate::loss::F3dLossCode::HoleParametersIndependentAssumed.note(format!(
            "Hole {} projects its admitted dimensions independently of unresolved extent, entry-type or parameter associations; its encoded entry type governs the active treatment and all parameter values and expressions are retained (DR-470).", scope.id
        )));
    }
    let specification = hole_clearance_specification(scope);
    if let Some(extent) = scope.hole_extent() {
        if let Some(loss) = extent
            .leading_word
            .as_ref()
            .and_then(|word| hole_tap_type_loss(&scope.id, word.value))
        {
            losses.push(loss);
        }
        if specification.is_some() {
            losses.push(crate::loss::F3dLossCode::HoleClearanceFieldsAssumed.note(format!(
                "Hole {} sets the modeled, cosmetic, hand and depth fields of its clearance specification to modeled, cosmetic off, right hand and hole depth, which its sizing block does not store; its stored diameter gives the bore without comparison to the standard size, and its fastener name remains a source property.", scope.id
            )));
        } else if extent.standard.is_some() {
            losses.push(crate::loss::F3dLossCode::HoleStandardSizingAssumed.note(format!(
                "Hole {} assumes its scalar geometry is independent of its native standard sizing names, which remain source properties (DR-470).", scope.id
            )));
        }
    }
    let face = match resolved_direct_face_selection(scope, face_operands)? {
        Some(face) => face,
        None => FaceSelection::Native(scope.id.clone()),
    };
    let placements = hole_placements(scope);
    if reference_dimensions {
        losses.push(crate::loss::F3dLossCode::HoleReferenceDimensionsNative.note(format!(
            "Hole {} assumes its carrier point gives the reference-edge-distance placement; its source parameters and references retain the distance constraints (DR-254).", scope.id
        )));
    }
    if placements.as_ref().is_some_and(|placements| {
        placements.iter().any(|placement| {
            matches!(
                placement,
                cadmpeg_ir::features::holes::HolePlacement::Axis { .. }
            )
        })
    }) {
        losses.push(crate::loss::F3dLossCode::HoleCarrierAxisAssumed.note(format!(
            "Hole {} reads its carrier normal as an unoriented drill axis because its direction control is unavailable (DR-471).", scope.id
        )));
    }
    if placements.is_none()
        && scope.hole_construction().is_some_and(
            crate::records::feature::hole::DesignHoleConstruction::face_point_unselected,
        )
    {
        losses.push(crate::loss::F3dLossCode::HolePlacementPointUnselected.note(format!(
            "Hole {} retains its native placement because its face-point carrier and its point-data records lie at different points and no admitted rule selects the entry point (DR-1061).", scope.id
        )));
    } else if placements.is_none() {
        losses.push(crate::loss::F3dLossCode::HolePlacementUnproven.note(format!(
            "Hole {} retains its native placement because its carrier and point-data records do not define an admitted placement.", scope.id
        )));
    }
    Ok(Some(FeatureDefinition::Operation(FeatureOperation::Hole {
        profile: None,
        profile_filter: None,
        face: Some(face),
        direction: None,
        placements,
        shape: cadmpeg_ir::features::holes::HoleShape::new(
            cadmpeg_ir::features::holes::HoleConstruction::Form {
                kind,
                specification: specification.map(Box::new),
            },
            None,
            diameter,
        )
        .map_err(CodecError::malformed)?,

        extent: Some(extent),
        bottom,
        taper_angle: None,
        allow_multi_profile_faces: None,
    })))
}

/// The clearance specification of a clearance Hole (tap type `1`) with standard sizing names.
pub(super) fn hole_clearance_specification(
    scope: &DesignParameterScope,
) -> Option<cadmpeg_ir::features::holes::HoleSpecification> {
    let extent = scope.hole_extent()?;
    if extent.leading_word.as_ref()?.value != HOLE_TAP_CLEARANCE {
        return None;
    }
    let names = extent.standard.as_ref()?;
    let named = |name: &str| (!name.trim().is_empty()).then(|| name.to_owned());
    Some(cadmpeg_ir::features::holes::HoleSpecification::Clearance {
        standard: cadmpeg_core::text::NonBlankString::new(names.standard.clone())?,
        designation: named(&names.size),
        fit: named(&names.fit),
        modeled: true,
        cosmetic: false,
        hand: cadmpeg_ir::features::holes::ThreadHand::Right,
        depth: cadmpeg_ir::features::holes::HoleThreadDepth::HoleDepth,
        clearance: None,
    })
}
