// SPDX-License-Identifier: Apache-2.0
//! Sweep and loft projection.

use crate::design::face_resolve::resolved_historical_face_group;
use crate::design::face_resolve::resolved_loft_edge_profile_group;
use crate::design::face_resolve::resolved_profile_face_group;

use crate::design::feature_project::resolved_loft_path;
use crate::design::feature_project::unique_feature_match;
use crate::design::feature_project::LoftPointSources;
use crate::design::feature_project::ProjectInputs;
use crate::ids::native_stream;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::edge_identity::DesignEdgeIdentityOperand;
use crate::records::topology::edge_identity::DesignEdgeOperand;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::face::DesignFaceOperand;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;
use std::collections::HashSet;

/// Solve each Loft point section whose group's one entity-selection member
/// names a planar sketch point by its Sketch record and persistent id.
pub(crate) fn bind_loft_sketch_point_sections(
    features: &mut [cadmpeg_ir::features::Feature],
    sources: &LoftPointSources<'_>,
) {
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, LoftPointSection, LoftSection,
    };

    for feature in features.iter_mut() {
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Loft { sections, .. }) = definition
            else {
                return;
            };
            for section in sections.iter_mut() {
                let LoftSection::Point(LoftPointSection::Native(native)) = section else {
                    continue;
                };
                if let Some(point) = loft_sketch_point(native.as_str(), sources) {
                    *section = LoftSection::Point(LoftPointSection::Point(point));
                }
            }
        });
    }
}

pub(super) fn loft_sketch_point(
    group_id: &str,
    sources: &LoftPointSources<'_>,
) -> Option<cadmpeg_ir::features::FinitePoint3> {
    fn only<T>(mut matches: impl Iterator<Item = T>) -> Option<T> {
        let first = matches.next()?;
        matches.next().is_none().then_some(first)
    }

    let group = only(sources.groups.iter().filter(|group| group.id == group_id))?;
    let stream = native_stream(&group.id)?;
    let (sketch_record, persistent_id) =
        loft_point_operand(group, sources.entity_operands)?.sketch_point()?;
    let sketch_record = u32::try_from(sketch_record).ok()?;
    let point = only(sources.sketch_points.iter().filter(|point| {
        native_stream(&point.id) == Some(stream)
            && point.owner_reference == Some(sketch_record)
            && point.persistent_id() == Some(persistent_id)
    }))?;
    let entity = only(
        sources
            .sketch_entities
            .iter()
            .filter(|entity| entity.native_ref.as_deref() == Some(point.id.as_str())),
    )?;
    let cadmpeg_ir::sketches::SketchGeometryDefinition::Point { position } =
        entity.geometry.definition()
    else {
        return None;
    };
    let sketch = only(
        sources
            .sketches
            .iter()
            .filter(|sketch| sketch.id == entity.sketch),
    )?;
    crate::design::geometry::sketch_point_to_model(sketch, position.get())
}

/// The one entity-selection operand that forms the single member of a
/// role-`0x21` Loft point group.
pub(super) fn loft_point_operand<'a>(
    group: &DesignConstructionOperandGroup,
    operands: &'a [crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) -> Option<&'a crate::records::topology::entity_selection::DesignEntitySelectionOperand> {
    let stream = native_stream(&group.id)?;
    let [member] = group.members() else {
        return None;
    };
    let mut matches = operands.iter().filter(|operand| {
        native_stream(&operand.id) == Some(stream)
            && operand.group_record_index == group.record_index
            && operand.group_member_ordinal == 0
    });
    let operand = matches.next()?;
    (group.role() == DesignOperandRole::ROLE_0X21
        && matches.next().is_none()
        && operand.record_index() == member.value)
        .then_some(operand)
}

pub(super) fn fixed_boolean_operation(
    operation: DesignExtrudeOperation,
) -> cadmpeg_ir::features::BooleanOp {
    match operation {
        DesignExtrudeOperation::Join => cadmpeg_ir::features::BooleanOp::Join,
        DesignExtrudeOperation::Cut => cadmpeg_ir::features::BooleanOp::Cut,
        DesignExtrudeOperation::Intersect => cadmpeg_ir::features::BooleanOp::Intersect,
        DesignExtrudeOperation::NewBody => cadmpeg_ir::features::BooleanOp::NewBody,
    }
}

pub(crate) struct LoftGroups<'a> {
    pub(crate) sections: Vec<&'a DesignConstructionOperandGroup>,
    pub(crate) rails: Vec<&'a DesignConstructionOperandGroup>,
}

pub(crate) fn loft_groups<'a>(
    construction: &crate::records::feature::path_features::DesignLoftConstruction,
    groups: &[&'a DesignConstructionOperandGroup],
) -> Result<LoftGroups<'a>, &'static str> {
    let group = |record_index: u32| {
        unique_feature_match(
            groups
                .iter()
                .copied()
                .filter(|group| group.record_index == record_index),
        )
    };
    let mut sections = Vec::new();
    for record in &construction.section_records {
        let section = record
            .checked_add(1)
            .and_then(group)
            .ok_or("a section-list entry names no construction-operand group of the scope")?;
        if !matches!(
            section.role(),
            DesignOperandRole::PROFILE
                | DesignOperandRole::ROLE_0X43
                | DesignOperandRole::ROLE_0X5
                | DesignOperandRole::ROLE_0X21
        ) {
            return Err("a section group has a role outside the section roles");
        }
        sections.push(section);
    }
    if sections.len() < 2 {
        return Err("the section list names fewer than two sections");
    }
    let reference_rail = construction
        .reference_target
        .map(u32::try_from)
        .transpose()
        .map_err(|_| "the offset-33 reference target is not a record index")?;
    let mut rails = Vec::new();
    for record in construction
        .rail_groups
        .iter()
        .copied()
        .chain(reference_rail)
    {
        let rail =
            group(record).ok_or("a rail entry names no construction-operand group of the scope")?;
        if !matches!(
            rail.role(),
            DesignOperandRole::ROLE_0X5 | DesignOperandRole::ROLE_0X7
        ) {
            return Err("a rail group has a role outside the rail roles");
        }
        rails.push(rail);
    }
    let named = sections
        .iter()
        .chain(&rails)
        .map(|group| group.record_index)
        .collect::<HashSet<_>>();
    if named.len() != sections.len() + rails.len() {
        return Err("two list entries name one group");
    }
    let bodies = groups
        .iter()
        .filter(|group| !named.contains(&group.record_index))
        .collect::<Vec<_>>();
    if bodies.len() != usize::from(construction.operation != DesignExtrudeOperation::NewBody)
        || bodies.iter().any(|group| {
            !matches!(
                group.role(),
                DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B
            )
        })
    {
        return Err(
            "the groups that neither list names are not the existing-body group of the operation",
        );
    }
    Ok(LoftGroups { sections, rails })
}

pub(in crate::design) fn project_fixed_loft(
    scope: &DesignParameterScope,
    inputs: &ProjectInputs<'_>,
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, LoftGuidance, LoftPointSection, LoftSection,
        PlanarProfileRef, ProfileRef,
    };
    let &ProjectInputs {
        construction_groups,
        edge_operands,
        edge_identity_operands,
        face_operands,
        face_boundaries,
        ..
    } = inputs;

    let crate::records::feature::scope::DesignScopePayload::Loft(Some(construction)) =
        &scope.payload()
    else {
        return Ok(None);
    };
    if construction.section_records.is_empty() {
        return Ok(None);
    }
    let stream = or_none!(native_stream(&scope.id));
    let groups = construction_groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        })
        .collect::<Vec<_>>();
    let assigned = match loft_groups(construction, &groups) {
        Ok(assigned) => assigned,
        Err(reason) => {
            losses.push(
                crate::loss::F3dLossCode::FeatureLoftGroupsUnassigned.note(format!(
                    "Loft scope {}: {reason}, and the scope stays native.",
                    scope.id
                )),
            );
            return Ok(None);
        }
    };
    let mut sections = Vec::new();
    for group in &assigned.sections {
        let native = || ProfileRef::Planar(PlanarProfileRef::Native(group.id.clone()));
        let section = if group.role() == DesignOperandRole::ROLE_0X21 {
            LoftSection::Point(LoftPointSection::Native(or_none!(
                cadmpeg_core::text::NonBlankString::new(group.id.clone())
            )))
        } else if group.role() == DesignOperandRole::ROLE_0X5 {
            LoftSection::Profile(native())
        } else {
            let profile = match resolved_loft_edge_profile_group(
                scope,
                group,
                edge_operands,
                face_boundaries,
            )? {
                Some(profile) => Some(profile),
                None => resolved_profile_face_group(scope, group, face_operands)?,
            };
            LoftSection::Profile(profile.unwrap_or_else(native))
        };
        sections.push(section);
    }
    let mut rails = Vec::new();
    for group in &assigned.rails {
        rails.push(resolved_loft_path(
            group,
            construction_groups,
            edge_operands,
            edge_identity_operands,
            scope,
            ctx,
        )?);
    }
    let guidance = match (assigned.rails.as_slice(), rails.pop()) {
        ([rail], Some(path))
            if rail.role() == DesignOperandRole::ROLE_0X7
                && construction.reference_target.is_none() =>
        {
            losses.push(
                crate::loss::F3dLossCode::FeatureLoftCenterlineAssumed.note(format!(
                    "Loft scope {} projects the one role-0x7 group {} of its rail list as a centerline (DR-332).",
                    scope.id, rail.id
                )),
            );
            LoftGuidance::Centerline(path)
        }
        (_, path) => {
            rails.extend(path);
            LoftGuidance::Guides(rails)
        }
    };
    Ok(Some(FeatureDefinition::Operation(FeatureOperation::Loft {
        sections,
        guidance,
        op: fixed_boolean_operation(construction.operation),
        closed: false,
        solid: true,
        ruled: false,
        linearize: false,
        max_degree: None,
        allow_multi_profile_faces: None,
    })))
}

pub(in crate::design) fn project_fixed_sweep(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    face_operands: &[DesignFaceOperand],
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        FaceSelection, FeatureDefinition, FeatureOperation, PlanarProfileRef, SweepGuideRail,
        SweepOrientation, SweepPathExtent,
    };
    use cadmpeg_ir::scalar::Angle;

    let Some((operation, values, profile, path, paths, guide_surfaces)) = (|| -> Option<_> {
        let crate::records::feature::scope::DesignScopePayload::Sweep(Some(
            crate::records::feature::scope::DesignSweepScope {
                construction:
                    Some(crate::records::feature::path_features::DesignSweepConstruction {
                        operation,
                        values,
                        ..
                    }),
                ..
            },
        )) = &scope.payload()
        else {
            return None;
        };
        let values: Vec<f64> = values.iter().map(|value| value.get()).collect();
        let stream = native_stream(&scope.id)?;
        let mut groups = Vec::new();
        for group in construction_groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
        }) {
            groups.push(group);
        }
        let mut profiles = Vec::new();
        let mut paths = Vec::new();
        let mut bodies = Vec::new();
        let mut guide_surfaces = Vec::new();
        for group in &groups {
            match group.role() {
                DesignOperandRole::PROFILE => {
                    profiles.push(*group);
                }
                DesignOperandRole::ROLE_0X5 => {
                    paths.push(*group);
                }
                DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B => {
                    bodies.push(*group);
                }
                DesignOperandRole::FACES => {
                    guide_surfaces.push(*group);
                }
                _ => {}
            }
        }
        paths.sort_by(|left, right| {
            left.scope_reference_ordinal
                .cmp(&right.scope_reference_ordinal)
        });
        let guide_surface_form = match guide_surfaces.as_slice() {
            [] => false,
            [_] => true,
            _ => return None,
        };
        let profile = if guide_surface_form {
            let sweep_profile = scope.sweep_profile()?;
            let mut carriers = profiles.iter().filter(|group| {
                group
                    .members()
                    .iter()
                    .map(|member| member.value)
                    .eq([sweep_profile.record_index])
            });
            let mut selections = profiles.iter().filter(|group| {
                !group
                    .members()
                    .iter()
                    .map(|member| member.value)
                    .eq([sweep_profile.record_index])
            });
            let (Some(_carrier), None, Some(selection), None) = (
                carriers.next(),
                carriers.next(),
                selections.next(),
                selections.next(),
            ) else {
                return None;
            };
            if selection.members().is_empty()
                || !selection
                    .members()
                    .iter()
                    .map(|member| &member.value)
                    .all(|member| {
                        entity_selection_operands.iter().any(|operand| {
                            native_stream(&operand.id) == Some(stream)
                                && operand.scope_record_index == scope.record_index
                                && operand.group_record_index == selection.record_index
                                && operand.record_index() == *member
                        })
                    })
            {
                return None;
            }
            *selection
        } else {
            let [profile] = profiles.as_slice() else {
                return None;
            };
            *profile
        };
        let ([path] | [path, _]) = paths.as_slice() else {
            return None;
        };
        let expected_group_count =
            profiles.len() + paths.len() + bodies.len() + guide_surfaces.len();
        let first_ordinal = groups
            .iter()
            .map(|group| group.scope_reference_ordinal)
            .min();
        let body_leads = bodies.iter().all(|body| {
            let ordinal = match body.role() {
                DesignOperandRole::BODIES_A => 0,
                _ => 7,
            };
            body.scope_reference_ordinal == ordinal && first_ordinal == Some(ordinal)
        });
        if bodies.len() > 1
            || !body_leads
            || groups.len() != expected_group_count
            || (*operation == DesignExtrudeOperation::NewBody && !bodies.is_empty())
            || (guide_surface_form && paths.len() != 1)
        {
            return None;
        }
        let valid_lanes = match values.as_slice() {
            [along, against] => {
                paths.len() == 1 && (0.0..=1.0).contains(along) && (0.0..=1.0).contains(against)
            }
            [along, against, along_rail, against_rail, _twist, _taper] => {
                [along, against, along_rail, against_rail]
                    .iter()
                    .all(|value| (0.0..=1.0).contains(*value))
                    && (paths.len() != 1 || (*along_rail == 1.0 && *against_rail == 1.0))
            }
            _ => false,
        };
        if !valid_lanes {
            return None;
        }
        Some((operation, values, profile, *path, paths, guide_surfaces))
    })() else {
        return Ok(None);
    };
    let path = resolved_loft_path(
        path,
        construction_groups,
        edge_operands,
        edge_identity_operands,
        scope,
        ctx,
    )?;
    let (along, against, guide_rail, twist, taper) = match values.as_slice() {
        [along, against] => (*along, *against, None, None, None),
        [along, against, along_rail, against_rail, twist, taper] => {
            let guide_extent = SweepPathExtent {
                along_fraction: or_none!(cadmpeg_ir::scalar::Fraction::new(*along_rail)),
                against_fraction: or_none!(cadmpeg_ir::scalar::Fraction::new(*against_rail)),
            };
            let guide_rail = if let Some(rail) = paths.get(1) {
                Some(SweepGuideRail {
                    path: resolved_loft_path(
                        rail,
                        construction_groups,
                        edge_operands,
                        edge_identity_operands,
                        scope,
                        ctx,
                    )?,
                    extent: guide_extent,
                })
            } else {
                None
            };
            let twist_angle = or_none!(Angle::new(*twist));
            let taper_angle = or_none!(Angle::new(*taper));
            (
                *along,
                *against,
                guide_rail,
                (*twist != 0.0).then_some(twist_angle),
                (*taper != 0.0).then_some(taper_angle),
            )
        }
        _ => return Ok(None),
    };
    let orientation = guide_surfaces
        .first()
        .map(|group| SweepOrientation::GuideSurface {
            faces: match resolved_historical_face_group(
                scope,
                scope.previous_history_state_id(),
                group,
                face_operands,
            ) {
                Some(selection) => selection,
                None => FaceSelection::Native(group.id.clone()),
            },
        });
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Sweep {
            shape: cadmpeg_ir::features::SweepShape::Solid {
                op: if *operation == DesignExtrudeOperation::NewBody {
                    cadmpeg_ir::features::SolidSweepOperation::NewBody
                } else {
                    or_none!(fixed_boolean_operation(*operation).try_into().ok())
                },
                section: cadmpeg_ir::features::SweepSection::Profile(
                    match resolved_profile_face_group(scope, profile, face_operands)? {
                        Some(cadmpeg_ir::features::ProfileRef::Planar(faces)) => faces,
                        _ => PlanarProfileRef::Native(profile.id.clone()),
                    },
                ),
                sections: Vec::new(),
            },

            path: Some(path),

            orientation,
            transition: None,
            transformation: None,
            path_tangent: false,
            linearize: false,
            twist,
            path_extent: Some(SweepPathExtent {
                along_fraction: or_none!(cadmpeg_ir::scalar::Fraction::new(along)),
                against_fraction: or_none!(cadmpeg_ir::scalar::Fraction::new(against)),
            }),
            guide_rail,
            taper,
            scale: None,
            allow_multi_profile_faces: None,
        },
    )))
}

pub(super) fn spatial_sketch_entity_endpoints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entity: &cadmpeg_ir::sketches::SpatialSketchEntity,
) -> Result<Option<[Point3; 2]>, CodecError> {
    use cadmpeg_ir::sketches::SpatialSketchGeometryDefinition;

    match entity.geometry.definition() {
        SpatialSketchGeometryDefinition::Line { start, end } => Ok(Some([start.get(), end.get()])),
        SpatialSketchGeometryDefinition::Arc {
            center,
            normal,
            reference_direction,
            radius,
            start_angle,
            end_angle,
        } => {
            let normal = normal.as_raw();
            let reference_direction = reference_direction.as_raw();
            let transverse = normal.cross(*reference_direction);
            let at = |angle: f64| {
                center.translated(
                    reference_direction.scale(angle.cos()) + transverse.scale(angle.sin()),
                    radius.get(),
                )
            };
            Ok(Some([at(start_angle.get()), at(end_angle.get())]))
        }
        SpatialSketchGeometryDefinition::Nurbs { curve } if !curve.periodic() => {
            let Ok(degree) = usize::try_from(curve.degree()) else {
                return Ok(None);
            };
            let start = curve.knots()[degree];
            let end = curve.knots()[curve.pole_count()];
            let Some(first) =
                cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::nurbs_curve_point_at(ctx, curve, start),
                )?)?
            else {
                return Ok(None);
            };
            let Some(last) =
                cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::nurbs_curve_point_at(ctx, curve, end),
                )?)?
            else {
                return Ok(None);
            };
            Ok(Some([first.get(), last.get()]))
        }
        _ => Ok(None),
    }
}

/// Closed profiles of one spatial sketch, or `None` when the chain search
/// exhausts `budget`.
pub(in crate::design) fn closed_spatial_sketch_profiles(
    ctx: &DecodeContext<'_>,
    entities: &[&cadmpeg_ir::sketches::SpatialSketchEntity],
    tolerance: f64,
    budget: &cadmpeg_core::decode::WorkBudget,
) -> Result<Option<Vec<cadmpeg_ir::sketches::SpatialSketchProfile>>, CodecError> {
    use cadmpeg_ir::sketches::{
        SpatialSketchEntityUse, SpatialSketchGeometryDefinition, SpatialSketchProfile,
    };

    if !tolerance.is_finite() || tolerance <= 0.0 {
        return Ok(Some(Vec::new()));
    }
    let mut profiles = Vec::new();
    let mut edges = Vec::new();
    for entity in entities
        .iter()
        .copied()
        .filter(|entity| !entity.construction)
    {
        match entity.geometry.definition() {
            SpatialSketchGeometryDefinition::Circle {
                center,
                normal,
                reference_direction,
                ..
            } => {
                let boundary = vec![SpatialSketchEntityUse {
                    entity: entity.id().clone(),
                    reversed: false,
                }];
                if let Ok(profile) = SpatialSketchProfile::try_new(
                    center.get(),
                    *normal.as_raw(),
                    *reference_direction.as_raw(),
                    boundary,
                ) {
                    profiles.push(profile);
                }
            }
            _ => {
                if let Some(ends) = spatial_sketch_entity_endpoints(ctx, entity)? {
                    edges.push((entity, ends));
                }
            }
        }
    }
    let close = |a: Point3, b: Point3| (a.x - b.x).hypot(a.y - b.y).hypot(a.z - b.z) <= tolerance;
    let mut unused = HashSet::new();
    for index in 0..edges.len() {
        unused.insert(index);
    }
    loop {
        if !budget.charge_by(unused.len()) {
            return Ok(None);
        }
        let Some(&first) = unused
            .iter()
            .min_by(|a, b| edges[**a].0.id().cmp(edges[**b].0.id()))
        else {
            break;
        };
        unused.remove(&first);
        let mut uses = Vec::new();
        uses.push((first, false));
        let start = edges[first].1[0];
        let mut end = edges[first].1[1];
        while !close(end, start) {
            if !budget.charge_by(unused.len()) {
                return Ok(None);
            }
            let mut candidate = None;
            let mut ambiguous = false;
            for &index in &unused {
                let [candidate_start, candidate_end] = edges[index].1;
                let match_ = if close(end, candidate_start) {
                    Some((index, false))
                } else if close(end, candidate_end) {
                    Some((index, true))
                } else {
                    None
                };
                if let Some(match_) = match_ {
                    if candidate.replace(match_).is_some() {
                        ambiguous = true;
                        break;
                    }
                }
            }
            if ambiguous {
                break;
            }
            let Some(next) = candidate else {
                break;
            };
            unused.remove(&next.0);
            uses.push(next);
            end = if next.1 {
                edges[next.0].1[0]
            } else {
                edges[next.0].1[1]
            };
        }
        if !budget.charge_by(edges.len()) {
            return Ok(None);
        }
        let start_degree = edges
            .iter()
            .filter(|(_, [edge_start, edge_end])| {
                close(*edge_start, start) || close(*edge_end, start)
            })
            .count();
        if !close(end, start) || uses.len() < 3 || start_degree != 2 {
            continue;
        }
        let point_for = |(index, reversed): &(usize, bool)| edges[*index].1[usize::from(*reversed)];
        let origin = point_for(&uses[0]);
        let mut normal = Vector3::new(0.0, 0.0, 0.0);
        for pair in uses[1..].windows(2) {
            let a = point_for(&pair[0]).vector_from(origin);
            let b = point_for(&pair[1]).vector_from(origin);
            normal = normal + a.cross(b);
        }
        let normal_length = normal.norm();
        let first_end = edges[uses[0].0].1[1];
        let u = first_end.vector_from(origin);
        let u_length = u.norm();
        if normal_length <= tolerance || u_length <= tolerance {
            continue;
        }
        normal = normal.scale(1.0 / normal_length);
        let u_axis = u.scale(1.0 / u_length);
        if uses
            .iter()
            .any(|use_| point_for(use_).vector_from(origin).dot(normal).abs() > tolerance)
        {
            continue;
        }
        let mut boundary = Vec::new();
        for (index, reversed) in uses {
            boundary.push(SpatialSketchEntityUse {
                entity: edges[index].0.id().clone(),
                reversed,
            });
        }
        if let Ok(profile) = SpatialSketchProfile::try_new(origin, normal, u_axis, boundary) {
            profiles.push(profile);
        }
    }
    profiles.sort_by(|a, b| a.boundary()[0].entity.cmp(&b.boundary()[0].entity));
    Ok(Some(profiles))
}
