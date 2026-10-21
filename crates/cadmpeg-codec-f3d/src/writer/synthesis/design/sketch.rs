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
            &entities,
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
                entities: &entities,
            },
            &native.design_dimension_presentation_frames,
            &[],
            target.tolerances.linear.get(),
        ),
    );
    constraints.sort_by(|left, right| left.id.cmp(&right.id));
    let mut stated_constraints = target.model.sketch_constraints.clone();
    stated_constraints.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(stated_constraints == constraints)
}

pub(in crate::writer::synthesis) fn retain_graph(native: &mut F3dNative, original: &F3dNative) {
    native.design_parameter_scopes = original.design_parameter_scopes.clone();
    native.design_sketch_placements = original.design_sketch_placements.clone();
    native.design_parameter_owners = original.design_parameter_owners.clone();
    native.design_parameter_companions = original
        .design_parameter_companions
        .iter()
        .map(crate::records::parameters::DesignParameterCompanion::without_payload)
        .collect();
    native.sketch_points = original.sketch_points.clone();
    native.sketch_curve_identities = original.sketch_curve_identities.clone();
    native.sketch_relations = original.sketch_relations.clone();
    native.sketch_texts = original.sketch_texts.clone();
}
