// SPDX-License-Identifier: Apache-2.0
//! Admit Sketch identity graphs only after proving their neutral content.

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;

use crate::design::feature_project::{
    project_parameter_design_with_edge_identities, ProjectInputs, ProjectedParameterDesign,
};
use crate::native::F3dNative;

pub(super) fn project(native: &F3dNative) -> Result<ProjectedParameterDesign, CodecError> {
    super::with_context(|ctx| {
        project_parameter_design_with_edge_identities(
            ctx,
            &ProjectInputs {
                native: &native.design_parameters,
                owners: &native.design_parameter_owners,
                scopes: &native.design_parameter_scopes,
                timelines: &native.design_feature_timelines,
                construction_groups: &[],
                fillet_radius_groups: &[],
                edge_operands: &[],
                edge_identity_operands: &[],
                edge_treatment_vertex_operands: &[],
                entity_selection_operands: &[],
                curve_identities: &native.sketch_curve_identities,
                face_operands: &[],
                body_recipe_operands: &[],
                legacy_loft_body_carriers: &[],
                placements: &native.design_sketch_placements,
                body_bindings: &[],
                component_naming_spaces: &[],
                histories: &[],
                face_boundaries: &[],
            },
        )
    })
}

pub(in crate::writer::synthesis) fn admit(
    target: &CadIr,
    native: &F3dNative,
) -> Result<bool, CodecError> {
    use crate::design::{design_feature_family, DesignFeatureFamily};
    if native.design_parameter_scopes.is_empty()
        || !native.design_feature_timelines.is_empty()
        || native.design_dimension_recipe_records.iter().any(|record| {
            !record.references.is_empty()
                || !record.matching_edge_operand_ids.is_empty()
                || record.program.is_empty()
                || i32::try_from(record.record_index).is_err()
        })
        || !target.model.spatial_sketches.is_empty()
        || target
            .model
            .parameters
            .iter()
            .any(|parameter| parameter.owner.is_none())
        || native
            .design_parameter_scopes
            .iter()
            .any(|scope| design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Sketch))
        || native
            .design_sketch_placements
            .iter()
            .any(|placement| placement.member_run_head() || placement.visibility.is_some())
    {
        return Ok(false);
    }
    let (sketches, entities) = super::with_context(|ctx| {
        crate::design::sketch_project::project_sketch_design(
            ctx,
            &native.design_sketch_placements,
            &native.sketch_points,
            &native.sketch_curve_identities,
            &native.sketch_relations,
            &native.sketch_texts,
            target.tolerances.linear.get(),
        )
    })?;
    if sketches != target.model.sketches || entities != target.model.sketch_entities {
        return Ok(false);
    }
    let mut projected = project(native)?;
    crate::design::feature_project::bind_sketch_feature_geometry(
        &mut projected.features,
        &native.design_parameter_scopes,
        &native.design_sketch_placements,
        &sketches,
        &[],
    )?;
    let mut stated = target.model.features.clone();
    // Evaluation invalidation belongs to the edited document. The generated
    // native graph cannot encode it; the writer reports that reduction.
    for feature in &mut stated {
        feature.needs_recompute = false;
    }
    if stated != projected.features {
        return Ok(false);
    }
    let mut constraints = constraints(native, &entities, target.tolerances.linear.get())?;
    constraints.sort_by(|left, right| left.id.cmp(&right.id));
    let mut stated_constraints = target.model.sketch_constraints.clone();
    stated_constraints.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(stated_constraints == constraints)
}

pub(in crate::writer::synthesis) fn retain_graph(native: &mut F3dNative, original: &F3dNative) {
    native.design_parameter_scopes = original.design_parameter_scopes.clone();
    native.design_sketch_placements = original.design_sketch_placements.clone();
    native.design_parameter_owners = original.design_parameter_owners.clone();
    native.design_dimension_recipe_records = original.design_dimension_recipe_records.clone();
    native.design_parameter_companions = original
        .design_parameter_companions
        .iter()
        .map(crate::records::parameters::DesignParameterCompanion::without_payload)
        .collect();
    native.sketch_points = original.sketch_points.clone();
    native.sketch_curve_identities = original
        .sketch_curve_identities
        .iter()
        .filter(|curve| {
            // An ownerless auxiliary curve has no neutral Sketch entity. Omit it
            // only when no retained Sketch carrier names it; a linked unsupported
            // curve keeps the graph out of the writable profile instead.
            curve.owner_reference.is_some()
                || original.sketch_points.iter().any(|point| {
                    point
                        .companion()
                        .incident_curves
                        .contains(&curve.record_index)
                })
                || original.sketch_relations.iter().any(|relation| {
                    relation
                        .all_member_indices()
                        .chain(relation.auxiliary_references().values().copied())
                        .any(|index| index == curve.record_index)
                })
                || original.design_entity_headers.iter().any(|header| {
                    header
                        .reference_values()
                        .chain(header.member_values())
                        .any(|index| *index == curve.record_index)
                })
                || original.sketch_curve_identities.iter().any(|other| {
                    matches!(&other.geometry,
                        Some(crate::records::sketch_geometry::SketchCurveGeometry::Nurbs { carrier_reference: Some(reference), .. })
                            if *reference == u64::from(curve.record_index))
                })
                || original.design_parameter_scopes.iter().any(|scope| {
                    scope
                        .reference_members()
                        .values()
                        .any(|index| *index == curve.record_index)
                })
        })
        .cloned()
        .collect();
    native.sketch_relations = original.sketch_relations.clone();
    native.sketch_texts = original.sketch_texts.clone();
}

fn constraints(
    native: &F3dNative,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
) -> Result<Vec<cadmpeg_ir::sketches::SketchConstraint>, CodecError> {
    let mut constraints = super::with_context(|ctx| {
        crate::design::constraints::project_sketch_constraints(
            ctx,
            &native.design_sketch_placements,
            &native.design_parameters,
            (
                &native.sketch_points,
                &native.sketch_curve_identities,
                &native.sketch_texts,
            ),
            &native.sketch_relations,
            entities,
        )
    })?;
    constraints.extend(
        crate::design::dimensions::project_dimension_constraints_with_presentations(
            &crate::design::dimensions::DimensionConstraintInputs {
                placements: &native.design_sketch_placements,
                parameters: &native.design_parameters,
                owners: &native.design_parameter_owners,
                pairs: &native.design_dimension_locus_pairs,
                groups: &native.design_dimension_locus_groups,
                annotation_frames: &native.design_dimension_annotation_frames,
                null_pairs: &native.design_dimension_null_locus_pairs,
                companions: &native.design_parameter_companions,
                recipe_records: &native.design_dimension_recipe_records,
                points: &native.sketch_points,
                curves: &native.sketch_curve_identities,
                entities,
            },
            &native.design_dimension_presentation_frames,
            &[],
            tolerance,
        ),
    );
    Ok(constraints)
}

pub(in crate::writer::synthesis) fn reduce_constraint_fields(
    target: &mut CadIr,
    native: &F3dNative,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    if native.design_parameter_scopes.is_empty() {
        return Ok(());
    }
    let projected = constraints(
        native,
        &target.model.sketch_entities,
        target.tolerances.linear.get(),
    )?;
    for stated in &mut target.model.sketch_constraints {
        let Some(written) = projected.iter().find(|written| written.id == stated.id) else {
            losses.push(
                crate::loss::F3dLossCode::WriterDesignRecordsOmitted.note(format!(
                    "Sketch constraint {} has no exact regenerated dimensional definition",
                    stated.id
                )),
            );
            continue;
        };
        if stated.definition != written.definition {
            use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Definition;
            let mut entity_only = false;
            let detail = match (stated.definition.kind(), written.definition.kind()) {
                (
                    Definition::Native { entities: before, .. },
                    Definition::Native { entities: after, .. },
                ) if !before.is_empty() && after.is_empty() => {
                    let mut restored = written.definition.clone();
                    restored
                        .edit(|definition| {
                            if let Definition::Native { entities, .. } = definition {
                                entities.clone_from(before);
                            }
                        })
                        .map_err(|error| CodecError::Malformed(error.into()))?;
                    entity_only = restored == stated.definition;
                    let lost = before
                        .iter()
                        .map(|entity| entity.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(
                        "ordered entity links reduce from {} to {}; omitted links [{}]",
                        before.len(), after.len(), lost
                    )
                }
                _ => "the target's dimensional definition cannot retain every canonical constraint field".into(),
            };
            losses.push(
                crate::loss::F3dLossCode::WriterDesignRecordsOmitted
                    .note(format!("Sketch constraint {}: {detail}", stated.id)),
            );
            if entity_only {
                // S-entity: the actual target schema retains Native kind, the
                // parameter and its real recipe operand with unresolved loci.
                // Its nonempty operand admits the empty-link definition.
                stated.definition = written.definition.clone();
            }
        }
    }
    Ok(())
}
