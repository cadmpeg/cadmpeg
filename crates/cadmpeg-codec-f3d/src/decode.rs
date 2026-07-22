// SPDX-License-Identifier: Apache-2.0
//! Assemble a `.f3d` archive into a [`CadIr`] document and [`DecodeReport`].
//!
//! [`crate::container`] scans the ZIP, reads ASM headers, finds the history
//! boundary, and selects the active B-rep. This module frames that B-rep with
//! [`crate::sab`], builds topology and geometry through [`crate::brep`], then
//! adds design, sketch, history, ACT, and appearance data.
//!
//! A framing failure or a stream without decoded geometry produces a
//! metadata-only document. The report marks geometry and topology as blocking,
//! and retained source data remains available for native replay.

use crate::native::F3dNative;
use cadmpeg_ir::annotations::AnnotationBuilder;
use cadmpeg_ir::codec::{CodecError, DecodeOptions, DecodeResult, ReadSeek};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::units::{Tolerances, Units};
use cadmpeg_ir::unknown::UnknownRecord;

use crate::brep::{self, Brep};
use crate::container::{self, BrepFacts, ContainerScan};
use crate::{asm_header, materials, sab};

fn unresolved_dimension_companion_count(native: &F3dNative) -> usize {
    use std::collections::{HashMap, HashSet};

    let parameters = native
        .design_parameters
        .iter()
        .map(|parameter| {
            (
                (
                    crate::ids::native_stream(&parameter.id).unwrap_or(crate::ids::DEFAULT_STREAM),
                    parameter.record_index,
                ),
                parameter.kind,
            )
        })
        .collect::<HashMap<_, _>>();
    let dimension_owners = native
        .design_parameter_owners
        .iter()
        .filter_map(|owner| {
            let stream = crate::ids::native_stream(&owner.id).unwrap_or(crate::ids::DEFAULT_STREAM);
            (parameters.get(&(stream, owner.parameter_record_index))
                == Some(&crate::records::DesignParameterKind::Dimension))
            .then_some((stream, owner.record_index))
        })
        .collect::<HashSet<_>>();
    let mut typed = HashSet::new();
    for pair in &native.design_dimension_locus_pairs {
        typed.insert((
            crate::ids::native_stream(&pair.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            pair.companion_record_index,
        ));
        typed.insert((
            crate::ids::native_stream(&pair.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            pair.governing_companion_record_index,
        ));
    }
    for frame in &native.design_dimension_annotation_frames {
        typed.insert((
            crate::ids::native_stream(&frame.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            frame.governing_companion_record_index,
        ));
    }
    for group in &native.design_dimension_locus_groups {
        typed.insert((
            crate::ids::native_stream(&group.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            group.companion_record_index,
        ));
    }
    for pair in &native.design_dimension_null_locus_pairs {
        typed.insert((
            crate::ids::native_stream(&pair.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            pair.companion_record_index,
        ));
        typed.insert((
            crate::ids::native_stream(&pair.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            pair.governing_companion_record_index,
        ));
    }
    for record in &native.design_dimension_recipe_records {
        typed.insert((
            crate::ids::native_stream(&record.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            record.companion_record_index,
        ));
    }
    native
        .design_parameter_companions
        .iter()
        .filter(|companion| {
            let stream =
                crate::ids::native_stream(&companion.id).unwrap_or(crate::ids::DEFAULT_STREAM);
            companion.payload_byte_length > 0
                && dimension_owners.contains(&(stream, companion.owner_record_index))
                && !typed.contains(&(stream, companion.record_index))
        })
        .count()
}

fn report_unresolved_dimension_companions(report: &mut DecodeReport, native: &F3dNative) {
    let count = unresolved_dimension_companion_count(native);
    if count != 0 {
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{count} payload-bearing Design dimension companion(s) were retained without a typed locus frame."
            ),
            provenance: None,
        });
    }
}

fn report_unresolved_configuration_rules(
    report: &mut DecodeReport,
    native: &F3dNative,
    ir: &CadIr,
) {
    let count = crate::design::configurations::unresolved_configuration_member_count(
        &native.design_configurations,
    );
    if count != 0 {
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{count} Design configuration JSON member(s) were retained without assigned neutral configuration semantics."
            ),
            provenance: None,
        });
    }
    let count = crate::design::configurations::unresolved_configuration_rule_count(
        &native.design_configurations,
        &ir.model.configurations,
    );
    if count != 0 {
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{count} nonempty Design configuration rule(s) were retained without an unambiguous neutral activation target."
            ),
            provenance: None,
        });
    }
    let count = crate::design::configurations::unresolved_configuration_parameter_override_count(
        &ir.model.configurations,
    );
    if count != 0 {
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{count} Design configuration parameter override(s) were retained without an unambiguous neutral parameter identity."
            ),
            provenance: None,
        });
    }
    let count = crate::design::configurations::unresolved_configuration_suppressed_feature_count(
        &ir.model.configurations,
    );
    if count != 0 {
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{count} Design configuration feature suppression(s) were retained without an unambiguous neutral feature identity."
            ),
            provenance: None,
        });
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct DesignProjectionGaps {
    unresolved_body_bindings: usize,
    native_features: usize,
    unprojected_feature_scopes: usize,
    unprojected_parameters: usize,
    untyped_parameter_units: usize,
    unresolved_expression_dependencies: usize,
    unprojected_history_dependencies: usize,
    ambiguous_history_dependencies: usize,
    native_constraints: usize,
    unprojected_sketch_placements: usize,
    unprojected_sketch_points: usize,
    unprojected_sketch_curves: usize,
    unprojected_sketch_surfaces: usize,
    unprojected_sketch_texts: usize,
    unprojected_sketch_relations: usize,
    unprojected_dimensions: usize,
    profile_selections: usize,
    face_selections: usize,
    body_selections: usize,
    partially_resolved_face_members: usize,
    native_edge_selections: usize,
    partially_resolved_edge_members: usize,
    unresolved_edge_selections: usize,
}

fn constraint_parameters(
    definition: &cadmpeg_ir::sketches::SketchConstraintDefinition,
) -> Vec<&cadmpeg_ir::features::ParameterId> {
    use cadmpeg_ir::sketches::SketchConstraintDefinition as Definition;

    match definition {
        Definition::Offset { parameter, .. } | Definition::Native { parameter, .. } => {
            parameter.iter().collect()
        }
        Definition::Distance { parameter, .. }
        | Definition::DistanceLoci { parameter, .. }
        | Definition::HorizontalDistance { parameter, .. }
        | Definition::VerticalDistance { parameter, .. }
        | Definition::RepeatedDistance { parameter, .. }
        | Definition::Angle { parameter, .. }
        | Definition::AngleToAxis { parameter, .. }
        | Definition::Radius { parameter, .. }
        | Definition::Diameter { parameter, .. } => vec![parameter],
        Definition::RectangularPattern { directions, .. } => directions
            .iter()
            .flat_map(|direction| {
                [
                    direction.span_parameter.as_ref(),
                    direction.count_parameter.as_ref(),
                ]
                .into_iter()
                .flatten()
            })
            .collect(),
        Definition::CircularPattern {
            angle_parameter,
            count_parameter,
            ..
        } => [angle_parameter.as_ref(), count_parameter.as_ref()]
            .into_iter()
            .flatten()
            .collect(),
        Definition::Coincident { .. }
        | Definition::Polygon { .. }
        | Definition::SplineGroup { .. }
        | Definition::TextFrame { .. }
        | Definition::TextPath { .. }
        | Definition::CoincidentLoci { .. }
        | Definition::Midpoint { .. }
        | Definition::Concentric { .. }
        | Definition::Collinear { .. }
        | Definition::Symmetric { .. }
        | Definition::Horizontal { .. }
        | Definition::HorizontalLoci { .. }
        | Definition::Vertical { .. }
        | Definition::VerticalLoci { .. }
        | Definition::Parallel { .. }
        | Definition::Perpendicular { .. }
        | Definition::Tangent { .. }
        | Definition::Curvature { .. }
        | Definition::Equal { .. }
        | Definition::Fixed { .. } => Vec::new(),
    }
}

fn design_projection_gaps(ir: &CadIr, native: &F3dNative) -> DesignProjectionGaps {
    use cadmpeg_ir::features::{BodySelection, EdgeSelection, Extent, ExtrudeStart, FaceSelection};
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};
    use cadmpeg_ir::sketches::SketchConstraintDefinition;
    use std::collections::{HashMap, HashSet};

    let source_lost_edge_reference_ids = native
        .lost_edge_references
        .iter()
        .map(|reference| reference.id.as_str())
        .collect::<HashSet<_>>();
    let projected_constraint_refs = ir
        .model
        .sketch_constraints
        .iter()
        .filter_map(|constraint| constraint.native_ref.as_deref())
        .chain(
            ir.model
                .spatial_sketch_constraints
                .iter()
                .filter_map(|constraint| constraint.native_ref.as_deref()),
        )
        .collect::<HashSet<_>>();
    let projected_sketch_refs = ir
        .model
        .sketches
        .iter()
        .filter_map(|sketch| sketch.native_ref.as_deref())
        .chain(
            ir.model
                .spatial_sketches
                .iter()
                .filter_map(|sketch| sketch.native_ref.as_deref()),
        )
        .collect::<HashSet<_>>();
    let projected_sketch_entity_refs = ir
        .model
        .sketch_entities
        .iter()
        .filter_map(|entity| entity.native_ref.as_deref())
        .chain(
            ir.model
                .spatial_sketch_entities
                .iter()
                .filter_map(|entity| entity.native_ref.as_deref()),
        )
        .collect::<HashSet<_>>();
    let projected_feature_refs = ir
        .model
        .features
        .iter()
        .filter_map(|feature| feature.native_ref.as_deref())
        .collect::<HashSet<_>>();
    let projected_parameter_refs = ir
        .model
        .parameters
        .iter()
        .filter_map(|parameter| parameter.native_ref.as_deref())
        .collect::<HashSet<_>>();
    let projected_features = ir
        .model
        .features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.as_deref()?, feature)))
        .collect::<HashMap<_, _>>();
    let mut state_scopes = HashMap::<(&str, i64), Vec<&str>>::new();
    for scope in &native.design_parameter_scopes {
        let (Some(stream), Some(state_id)) =
            (crate::ids::native_stream(&scope.id), scope.history_state_id)
        else {
            continue;
        };
        state_scopes
            .entry((stream, state_id))
            .or_default()
            .push(scope.id.as_str());
    }
    let mut unprojected_history_dependencies = 0;
    let mut ambiguous_history_dependencies = 0;
    for scope in &native.design_parameter_scopes {
        let (Some(stream), Some(previous_state_id), Some(feature)) = (
            crate::ids::native_stream(&scope.id),
            scope.previous_history_state_id,
            projected_features.get(scope.id.as_str()),
        ) else {
            continue;
        };
        let Some(predecessors) = state_scopes.get(&(stream, previous_state_id)) else {
            continue;
        };
        let [predecessor_ref] = predecessors.as_slice() else {
            ambiguous_history_dependencies += 1;
            continue;
        };
        let Some(predecessor) = projected_features.get(predecessor_ref) else {
            unprojected_history_dependencies += 1;
            continue;
        };
        if predecessor.id != feature.id && !feature.dependencies.contains(&predecessor.id) {
            unprojected_history_dependencies += 1;
        }
    }
    let projected_dimension_parameters =
        ir.model
            .sketch_constraints
            .iter()
            .flat_map(|constraint| constraint_parameters(&constraint.definition))
            .chain(
                ir.model.spatial_sketch_constraints.iter().filter_map(
                    |constraint| match &constraint.definition {
                        cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::Native {
                            parameter,
                            ..
                        } => parameter.as_ref(),
                        cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::ParallelLineDistance {
                            parameter,
                            ..
                        } => Some(parameter),
                        _ => None,
                    },
                ),
            )
            .cloned()
            .collect::<HashSet<_>>();

    let mut gaps = DesignProjectionGaps {
        unresolved_body_bindings: native
            .design_body_bindings
            .iter()
            .filter(|binding| binding.body.is_none())
            .count(),
        unprojected_history_dependencies,
        ambiguous_history_dependencies,
        unprojected_feature_scopes: native
            .design_parameter_scopes
            .iter()
            .filter(|scope| !projected_feature_refs.contains(scope.id.as_str()))
            .count(),
        unprojected_parameters: native
            .design_parameters
            .iter()
            .filter(|parameter| !projected_parameter_refs.contains(parameter.id.as_str()))
            .count(),
        untyped_parameter_units: crate::design::feature_project::untyped_parameter_unit_count(
            &native.design_parameters,
        ),
        unresolved_expression_dependencies:
            crate::design::dimensions::unresolved_parameter_expression_dependency_count(
                &native.design_parameters,
                &ir.model.parameters,
            ),
        native_constraints: ir
            .model
            .sketch_constraints
            .iter()
            .filter(|constraint| {
                matches!(
                    constraint.definition,
                    SketchConstraintDefinition::Native { .. }
                )
            })
            .count()
            + ir.model
                .spatial_sketch_constraints
                .iter()
                .filter(|constraint| {
                    matches!(
                        constraint.definition,
                        cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::Native { .. }
                    )
                })
                .count(),
        unprojected_sketch_placements: native
            .design_sketch_placements
            .iter()
            .filter(|placement| !projected_sketch_refs.contains(placement.id.as_str()))
            .count(),
        unprojected_sketch_points: native
            .sketch_points
            .iter()
            .filter(|point| {
                point.owner_reference.is_some()
                    && !projected_sketch_entity_refs.contains(point.id.as_str())
            })
            .count(),
        unprojected_sketch_curves: native
            .sketch_curve_identities
            .iter()
            .filter(|curve| {
                curve.owner_reference.is_some()
                    && !projected_sketch_entity_refs.contains(curve.id.as_str())
            })
            .count(),
        unprojected_sketch_surfaces: native
            .sketch_surfaces
            .iter()
            .filter(|surface| {
                surface.owner_reference.is_some()
                    && !projected_sketch_entity_refs.contains(surface.id.as_str())
            })
            .count(),
        unprojected_sketch_texts: native
            .sketch_texts
            .iter()
            .filter(|text| !projected_sketch_entity_refs.contains(text.id.as_str()))
            .count(),
        unprojected_sketch_relations: native
            .sketch_relations
            .iter()
            .filter(|relation| !projected_constraint_refs.contains(relation.id.as_str()))
            .count(),
        unprojected_dimensions: native
            .design_parameters
            .iter()
            .filter(|parameter| {
                parameter.kind == crate::records::DesignParameterKind::Dimension
                    && !projected_dimension_parameters
                        .contains(&crate::ids::neutral_parameter_id(parameter))
            })
            .count(),
        ..DesignProjectionGaps::default()
    };
    let mut edge_selection = |selection: &EdgeSelection| match selection {
        EdgeSelection::Native(_) => gaps.native_edge_selections += 1,
        EdgeSelection::Unresolved => gaps.unresolved_edge_selections += 1,
        EdgeSelection::HistoricalPartial { unresolved, .. } => {
            gaps.partially_resolved_edge_members += unresolved
                .iter()
                .filter(|id| !source_lost_edge_reference_ids.contains(id.as_str()))
                .count();
        }
        EdgeSelection::Edges(_)
        | EdgeSelection::Resolved { .. }
        | EdgeSelection::Historical { .. } => {}
    };
    let mut face_selection = |selection: &FaceSelection| match selection {
        FaceSelection::Native(_) | FaceSelection::Unresolved => gaps.face_selections += 1,
        FaceSelection::HistoricalPartial { unresolved, .. } => {
            gaps.partially_resolved_face_members += unresolved.len();
        }
        FaceSelection::Faces(_)
        | FaceSelection::Resolved { .. }
        | FaceSelection::Historical { .. } => {}
    };
    for feature in &ir.model.features {
        match &feature.definition {
            FeatureDefinition::Native { .. } => gaps.native_features += 1,
            FeatureDefinition::BaseFeature { bodies }
            | FeatureDefinition::InsertBodies { bodies } => {
                if matches!(bodies, BodySelection::Native(_) | BodySelection::Unresolved) {
                    gaps.body_selections += 1;
                }
            }
            FeatureDefinition::Extrude {
                profile,
                start,
                extent,
                ..
            } => {
                if matches!(
                    profile,
                    ProfileRef::Native(_) | ProfileRef::SketchSelection { .. }
                ) {
                    gaps.profile_selections += 1;
                }
                if let ExtrudeStart::FromFace { face, .. } = start {
                    face_selection(face);
                }
                if let Extent::ToFace { face, .. } = extent {
                    face_selection(face);
                }
            }
            FeatureDefinition::Fillet { groups } => {
                for group in groups {
                    edge_selection(&group.edges);
                }
            }
            FeatureDefinition::Chamfer { groups } => {
                for group in groups {
                    edge_selection(&group.edges);
                }
            }
            FeatureDefinition::MoveFace { faces, .. } => face_selection(faces),
            _ => {}
        }
    }
    gaps
}

fn report_design_projection_gaps(report: &mut DecodeReport, ir: &CadIr, native: &F3dNative) {
    let gaps = design_projection_gaps(ir, native);
    if gaps.unresolved_body_bindings != 0 {
        report.losses.push(LossNote {
            category: LossCategory::Topology,
            severity: Severity::Warning,
            message: format!(
                "{} Design body-map pair(s) do not resolve to a body in the named BREP blob.",
                gaps.unresolved_body_bindings
            ),
            provenance: None,
        });
    }
    let mut push = |count: usize, message: String| {
        if count != 0 {
            report.losses.push(LossNote {
                category: LossCategory::Other,
                severity: Severity::Warning,
                message,
                provenance: None,
            });
        }
    };
    push(
        gaps.native_features,
        format!(
            "{} feature scope(s) retain native operation semantics because no complete neutral feature definition was resolved.",
            gaps.native_features
        ),
    );
    push(
        gaps.unprojected_feature_scopes,
        format!(
            "{} decoded feature scope(s) have no neutral construction-history feature.",
            gaps.unprojected_feature_scopes
        ),
    );
    push(
        gaps.unprojected_parameters,
        format!(
            "{} decoded Design parameter(s) have no neutral parameter.",
            gaps.unprojected_parameters
        ),
    );
    push(
        gaps.untyped_parameter_units,
        format!(
            "{} decoded Design parameter(s) retain unit tokens without a settled neutral quantity kind.",
            gaps.untyped_parameter_units
        ),
    );
    push(
        gaps.unresolved_expression_dependencies,
        format!(
            "{} decoded parameter expression symbol(s) name same-stream parameters without a neutral dependency edge.",
            gaps.unresolved_expression_dependencies
        ),
    );
    push(
        gaps.unprojected_history_dependencies,
        format!(
            "{} feature history-state dependency link(s) were not projected into neutral construction history.",
            gaps.unprojected_history_dependencies
        ),
    );
    push(
        gaps.ambiguous_history_dependencies,
        format!(
            "{} feature history-state dependency link(s) have multiple source scopes for the preceding state identity.",
            gaps.ambiguous_history_dependencies
        ),
    );
    push(
        gaps.native_constraints,
        format!(
            "{} sketch constraint(s) retain native operands because no unique neutral relation was resolved.",
            gaps.native_constraints
        ),
    );
    push(
        gaps.unprojected_sketch_placements,
        format!(
            "{} decoded Sketch placement(s) have no neutral sketch.",
            gaps.unprojected_sketch_placements
        ),
    );
    push(
        gaps.unprojected_sketch_points,
        format!(
            "{} decoded sketch point(s) have no neutral sketch entity.",
            gaps.unprojected_sketch_points
        ),
    );
    push(
        gaps.unprojected_sketch_curves,
        format!(
            "{} decoded sketch curve(s) have no neutral sketch entity.",
            gaps.unprojected_sketch_curves
        ),
    );
    push(
        gaps.unprojected_sketch_surfaces,
        format!(
            "{} decoded sketch surface(s) have no neutral spatial sketch entity.",
            gaps.unprojected_sketch_surfaces
        ),
    );
    push(
        gaps.unprojected_sketch_texts,
        format!(
            "{} decoded sketch text record(s) have no neutral sketch entity.",
            gaps.unprojected_sketch_texts
        ),
    );
    push(
        gaps.unprojected_sketch_relations,
        format!(
            "{} decoded sketch relation(s) have no neutral constraint.",
            gaps.unprojected_sketch_relations
        ),
    );
    push(
        gaps.unprojected_dimensions,
        format!(
            "{} Design dimension parameter(s) have no parameter-backed neutral or native sketch constraint.",
            gaps.unprojected_dimensions
        ),
    );
    push(
        gaps.profile_selections,
        format!(
            "{} feature profile selection(s) retain native selection identities because no unique neutral profile was resolved.",
            gaps.profile_selections
        ),
    );
    push(
        gaps.face_selections,
        format!(
            "{} feature face selection(s) retain native candidates because no unique topological face was resolved.",
            gaps.face_selections
        ),
    );
    push(
        gaps.body_selections,
        format!(
            "{} feature body selection(s) retain native identities because no unique solved body was resolved.",
            gaps.body_selections
        ),
    );
    push(
        gaps.partially_resolved_face_members,
        format!(
            "{} feature face operand(s) remain unresolved inside state-bound historical selections.",
            gaps.partially_resolved_face_members
        ),
    );
    push(
        gaps.native_edge_selections,
        format!(
            "{} edge-treatment selection(s) retain native construction recipes because no neutral historical edge selection was resolved.",
            gaps.native_edge_selections
        ),
    );
    push(
        gaps.partially_resolved_edge_members,
        format!(
            "{} edge-treatment operand(s) remain unresolved inside state-bound historical selections.",
            gaps.partially_resolved_edge_members
        ),
    );
    push(
        gaps.unresolved_edge_selections,
        format!(
            "{} edge-treatment selection(s) are unresolved because their source edge references were lost.",
            gaps.unresolved_edge_selections
        ),
    );
}

fn model_brep_candidates(
    scan: &ContainerScan,
    bindings: &[crate::records::DesignBodyBinding],
) -> Vec<BrepFacts> {
    let referenced = bindings
        .iter()
        .map(|binding| binding.blob_name.as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut candidates = scan
        .breps
        .iter()
        .filter(|brep| {
            brep.name
                .rsplit('/')
                .next()
                .is_some_and(|basename| referenced.contains(basename))
        })
        .cloned()
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        candidates.extend(container::select_active_brep(scan).cloned());
    } else if let Some(active) = container::select_active_brep(scan) {
        if let Some(position) = candidates
            .iter()
            .position(|candidate| candidate.name == active.name)
        {
            candidates.swap(0, position);
        }
    }
    candidates
}

fn brep_identity_namespace(entry: &str) -> Option<&str> {
    entry.rsplit('/').next()?.strip_prefix("BREP.")
}

/// Decode a `.f3d` reader into a document and its loss report.
pub fn decode(
    reader: &mut dyn ReadSeek,
    options: &DecodeOptions,
) -> Result<DecodeResult, CodecError> {
    let scan = container::scan(reader)?;

    if crate::f3z::is_f3z(&scan) {
        return crate::f3z::decode(&scan, options);
    }

    if options.container_only {
        let mut ir = build_metadata_ir(&scan)?;
        annotate_docstruct(&mut ir, &scan);
        populate_annotations(&mut ir, &scan, &F3dNative::default(), None);
        preserve_source_image(&scan, &mut ir)?;
        let mut report = build_container_report(&scan, true);
        if let Ok(Some(table)) = crate::xref::decode(&scan) {
            apply_assembly_classification(&mut report, &scan, &table);
        }
        return Ok(DecodeResult::new(ir, report));
    }

    let unbound_body_bindings =
        crate::design::decode::body::decode_design_body_bindings(&scan, None, &[])?;
    let model_breps = model_brep_candidates(&scan, &unbound_body_bindings);

    // Every Design body-map pair names its owning BREP blob. Decode the
    // complete referenced set; a document-level model is not confined to one
    // arbitrary `.smbh` entry.
    if let Some(model_brep) = model_breps.first().cloned() {
        let active = container::select_active_brep(&scan)
            .cloned()
            .unwrap_or(model_brep);
        let qualify_ids = model_breps.len() > 1;
        let mut brep = Brep::default();
        let mut body_visibilities = Vec::new();
        let mut decoded_brep_count = 0usize;
        let all_body_visibility = crate::design::decode::body::decode_all_body_visibility(&scan)?;
        let mut selected_body_keys =
            std::collections::HashMap::<String, std::collections::HashSet<u64>>::new();
        for binding in &unbound_body_bindings {
            selected_body_keys
                .entry(binding.blob_name.clone())
                .or_default()
                .insert(binding.asm_body_key);
        }
        for candidate in &model_breps {
            let Some(mut part) = try_decode_brep(reader, &scan, candidate)? else {
                continue;
            };
            let blob_name = candidate.name.rsplit('/').next().unwrap_or(&candidate.name);
            if let Some(keys) = selected_body_keys.get(blob_name) {
                part.retain_body_keys(keys)?;
            }
            let mut body_selectors = part.body_selectors();
            for body in &mut part.bodies {
                if let Some(visibility) = body_selectors.get(&body.id).and_then(|selector| {
                    all_body_visibility.get(&(blob_name.to_owned(), *selector))
                }) {
                    body.visible = Some(visibility.visible);
                }
            }
            if qualify_ids {
                let namespace = brep_identity_namespace(&candidate.name).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "BREP entry has no stable blob identity: {}",
                        candidate.name
                    ))
                })?;
                part.qualify_ids(namespace)?;
                body_selectors = part.body_selectors();
            }
            for body in &part.bodies {
                if let Some((body_selector, visibility)) =
                    body_selectors.get(&body.id).and_then(|selector| {
                        all_body_visibility
                            .get(&(blob_name.to_owned(), *selector))
                            .map(|visibility| (*selector, visibility))
                    })
                {
                    body_visibilities.push(crate::records::BodyVisibility {
                        id: format!("f3d:{}:body-visibility#{body_selector}", candidate.name),
                        body: body.id.clone(),
                        stream: visibility.stream.clone(),
                        byte_offset: visibility.byte_offset,
                        asm_body_key_offset: visibility.asm_body_key_offset,
                        asm_body_key: body_selector,
                        entity_suffix: visibility.entity_suffix,
                        visible: visibility.visible,
                    });
                }
            }
            brep.append(part);
            decoded_brep_count += 1;
        }
        if decoded_brep_count != 0 {
            let mut report = build_geometry_report(&scan, &brep);
            if decoded_brep_count != model_breps.len() {
                report.losses.push(LossNote {
                    category: LossCategory::Geometry,
                    severity: Severity::Warning,
                    message: format!(
                        "{} Design-referenced BREP blob(s) could not be decoded.",
                        model_breps.len() - decoded_brep_count
                    ),
                    provenance: None,
                });
            }
            let decoded_materials = materials::decode_with_bodies(reader, &scan, &brep.body_keys)?;
            let annotation_records = std::mem::take(&mut brep.annotation_records);
            let (mut ir, mut native) = build_geometry_ir(&scan, &active, brep)?;
            ir.model.subds = crate::tsm::decode(&scan)?;
            native.body_visibilities = body_visibilities;
            if let Some(history) = decode_asm_history(&scan, &active)? {
                native.asm_histories.push(history);
            }
            native.construction_recipes =
                crate::design::decode::parameters::decode_recipes(reader, &scan)?;
            native.persistent_references =
                crate::design::decode::sketch::decode_persistent_references(reader, &scan)?;
            native.lost_edge_references =
                crate::design::decode::sketch::decode_lost_edge_references(reader, &scan)?;
            native.design_material_assignments =
                crate::materials::decode_design_assignments(reader, &scan)?;
            native.design_objects = crate::design::decode::sketch::decode_objects(reader, &scan)?;
            native.design_parameters =
                crate::design::decode::parameters::decode_parameters(reader, &scan)?;
            native.design_entity_headers =
                crate::design::decode::sketch::decode_entity_headers(reader, &scan)?;
            native.design_record_headers = crate::design::decode::sketch::decode_record_headers(
                reader,
                &scan,
                &native.design_entity_headers,
            )?;
            let sketch_relations = {
                crate::design::decode::sketch::decode_sketch_relations(
                    reader,
                    &scan,
                    &native.design_record_headers,
                    &native.design_entity_headers,
                )?
            };
            native.sketch_relations = sketch_relations;
            extend_related_design_records(reader, &scan, &mut native)?;
            native.sketch_points =
                crate::design::decode::sketch::decode_sketch_points(reader, &scan)?;
            native.sketch_texts = crate::design::decode::sketch::decode_sketch_texts(&scan)?;
            native.sketch_curve_identities =
                crate::design::decode::sketch::decode_sketch_curve_identities(reader, &scan)?;
            native.sketch_surfaces = crate::design::decode::sketch::decode_sketch_surfaces(&scan)?;
            crate::design::decode::sketch::bind_sketch_graph(
                &native.design_entity_headers,
                &mut native.sketch_points,
                &mut native.sketch_curve_identities,
                &mut native.sketch_surfaces,
                &mut native.sketch_relations,
            )?;
            crate::design::decode::operands::bind_extrude_selection_geometry(
                &mut native.design_extrude_selection_members,
                &native.design_extrude_selection_groups,
                &native.design_parameter_scopes,
                &native.sketch_points,
                &native.sketch_curve_identities,
            );
            let dimension_inputs = crate::design::decode::dimension_frames::DimensionDecodeInputs {
                scan: &scan,
                parameters: &native.design_parameters,
                owners: &native.design_parameter_owners,
                companions: &native.design_parameter_companions,
                scopes: &native.design_parameter_scopes,
                headers: &native.design_record_headers,
                points: &native.sketch_points,
                curves: &native.sketch_curve_identities,
            };
            native.design_dimension_locus_pairs =
                crate::design::decode::dimension_frames::decode_dimension_locus_pairs(
                    &dimension_inputs,
                )?;
            native.design_dimension_annotation_frames =
                crate::design::decode::dimension_frames::decode_dimension_annotation_frames(
                    &dimension_inputs,
                    &native.design_entity_headers,
                )?;
            native.design_dimension_locus_groups =
                crate::design::decode::dimension_frames::decode_dimension_locus_groups(
                    &dimension_inputs,
                    &native.design_entity_headers,
                )?;
            native.design_dimension_null_locus_pairs =
                crate::design::decode::dimension_frames::decode_dimension_null_locus_pairs(
                    &dimension_inputs,
                    &native.design_dimension_locus_pairs,
                    &native.design_dimension_locus_groups,
                )?;
            crate::design::dimensions::remove_dimension_frame_relations(
                &mut native.sketch_relations,
                &native.design_dimension_locus_pairs,
                &native.design_dimension_locus_groups,
                &native.design_dimension_null_locus_pairs,
            );
            crate::design::dimensions::bind_dimension_loci(
                &native.design_sketch_placements,
                &native.design_parameter_owners,
                &native.design_dimension_locus_pairs,
                &native.design_dimension_locus_groups,
                &native.design_dimension_annotation_frames,
                &native.design_dimension_null_locus_pairs,
                &mut native.sketch_points,
                &mut native.sketch_curve_identities,
            )?;
            native.design_body_members =
                crate::design::decode::body::decode_body_members(reader, &scan)?;
            native.design_body_bindings = crate::design::decode::body::decode_design_body_bindings(
                &scan,
                Some(&active.name),
                &native.body_native_keys,
            )?;
            native.design_body_bounds = crate::design::decode::body::decode_body_bounds(
                &scan,
                &native.design_entity_headers,
            )?;
            crate::design::decode::body::bind_body_bounds(
                &mut native.design_body_bounds,
                &native.design_body_bindings,
            );
            native.design_configurations =
                crate::design::configurations::decode_configurations(&scan)?;
            ir.model.configurations = crate::design::configurations::project_configurations(
                &native.design_configurations,
            );
            (ir.model.features, ir.model.parameters) =
                crate::design::feature_project::project_parameter_design_with_edge_identities(
                    &crate::design::feature_project::ProjectInputs {
                        native: &native.design_parameters,
                        owners: &native.design_parameter_owners,
                        scopes: &native.design_parameter_scopes,
                        construction_groups: &native.design_construction_operand_groups,
                        fillet_radius_groups: &native.design_fillet_radius_groups,
                        edge_operands: &native.design_edge_operands,
                        edge_identity_operands: &native.design_edge_identity_operands,
                        face_operands: &native.design_face_operands,
                        placements: &native.design_sketch_placements,
                        body_bindings: &native.design_body_bindings,
                    },
                );
            crate::design::feature_project::bind_form_cages(
                &scan,
                &native.design_parameter_scopes,
                &native.design_record_headers,
                &mut ir.model.features,
                &ir.model.subds,
            )?;
            crate::design::configurations::bind_configuration_parameter_overrides(
                &mut ir.model.configurations,
                &ir.model.parameters,
            );
            crate::design::configurations::bind_configuration_suppressed_features(
                &mut ir.model.configurations,
                &ir.model.features,
            );
            ir.model.feature_input_topologies = crate::history::project_feature_input_topologies(
                &ir.model.features,
                &native.design_parameter_scopes,
                &native.asm_histories,
                &native.design_edge_operands,
            );
            crate::history::bind_feature_outputs(
                &mut ir.model.features,
                &native.design_parameter_scopes,
                &native.asm_histories,
                &ir.model.bodies,
            );
            crate::history::bind_feature_body_selections(
                &mut ir.model.features,
                &native.design_parameter_scopes,
                &native.design_construction_operand_groups,
                &native.design_body_recipe_operands,
                &native.asm_histories,
            );
            crate::history::bind_feature_face_selections(
                &mut ir.model.features,
                &native.design_parameter_scopes,
                &native.design_construction_operand_groups,
                &native.design_face_operands,
                &native.design_body_recipe_operands,
                &native.asm_histories,
            );
            crate::history::bind_feature_path_selections(
                &mut ir.model.features,
                &native.design_parameter_scopes,
                &native.design_construction_operand_groups,
                &native.design_entity_selection_operands,
            );
            (ir.model.sketches, ir.model.sketch_entities) =
                crate::design::sketch_project::project_sketch_design(
                    &native.design_sketch_placements,
                    &native.sketch_points,
                    &native.sketch_curve_identities,
                    &native.sketch_texts,
                    ir.tolerances.linear,
                );
            (ir.model.spatial_sketches, ir.model.spatial_sketch_entities) =
                crate::design::sketch_project::project_spatial_sketch_design(
                    &native.design_sketch_placements,
                    &native.sketch_points,
                    &native.sketch_curve_identities,
                    &native.sketch_surfaces,
                    &native.sketch_relations,
                    ir.tolerances.linear,
                );
            crate::design::decode::operands::bind_loft_sketch_selections(
                &scan,
                &native.design_construction_operand_groups,
                &native.design_record_headers,
                &crate::design::decode::operands::LoftSketchResolution {
                    entities: &native.design_entity_headers,
                    entity_selection_operands: &native.design_entity_selection_operands,
                    placements: &native.design_sketch_placements,
                    curve_identities: &native.sketch_curve_identities,
                    spatial_sketches: &ir.model.spatial_sketches,
                },
                &mut ir.model.features,
            )?;
            crate::design::feature_project::bind_sketch_feature_geometry(
                &mut ir.model.features,
                &native.design_parameter_scopes,
                &native.design_sketch_placements,
                &ir.model.sketches,
                &ir.model.spatial_sketches,
            );
            ir.model.spatial_sketch_constraints =
                crate::design::sketch_project::project_spatial_sketch_constraints(
                    &native.design_sketch_placements,
                    &native.sketch_relations,
                    &native.sketch_points,
                    &native.sketch_curve_identities,
                    &native.sketch_surfaces,
                    &ir.model.spatial_sketch_entities,
                );
            crate::design::profile_select::bind_extrude_profile_selections(
                &mut ir.model.features,
                &native.design_parameter_scopes,
                &native.design_extrude_selection_groups,
                &native.design_extrude_selection_members,
                &ir.model.sketches,
                crate::design::profile_select::ExtrudeProfileResolution {
                    entities: &ir.model.sketch_entities,
                    spatial_sketches: &ir.model.spatial_sketches,
                    spatial_entities: &ir.model.spatial_sketch_entities,
                    histories: &native.asm_histories,
                    linear_tolerance: ir.tolerances.linear,
                },
            );
            crate::design::face_resolve::bind_extrude_start_planes(
                &mut ir.model.features,
                &ir.model.sketches,
                &mut crate::design::face_resolve::ExtrudeStartPlaneResolution {
                    faces: &ir.model.faces,
                    surfaces: &ir.model.surfaces,
                    groups: &native.design_construction_operand_groups,
                    operands: &mut native.design_face_operands,
                    linear_tolerance: ir.tolerances.linear,
                    angular_tolerance: ir.tolerances.angular,
                },
            );
            ir.model.sketch_constraints = crate::design::constraints::project_sketch_constraints(
                &native.design_sketch_placements,
                &native.design_parameters,
                &native.sketch_points,
                &native.sketch_curve_identities,
                &native.sketch_texts,
                &native.sketch_relations,
                &ir.model.sketch_entities,
            );
            let constraint_inputs = crate::design::dimensions::DimensionConstraintInputs {
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
                entities: &ir.model.sketch_entities,
            };
            ir.model.sketch_constraints.extend(
                crate::design::dimensions::project_dimension_constraints(
                    &constraint_inputs,
                    &ir.model.spatial_sketches,
                ),
            );
            ir.model.spatial_sketch_constraints.extend(
                crate::design::dimensions::project_spatial_dimension_constraints(
                    &constraint_inputs,
                    &ir.model.spatial_sketches,
                    &ir.model.spatial_sketch_entities,
                ),
            );
            crate::design::dimensions::bind_offset_dimension_parameters(
                &mut ir.model.sketch_constraints,
                &native.design_parameters,
            );
            ir.model
                .sketch_constraints
                .sort_by_key(|constraint| constraint.id.clone());
            ir.model
                .spatial_sketch_constraints
                .sort_by_key(|constraint| constraint.id.clone());
            let act = crate::act::decode(reader, &scan)?;
            native.act_entities = act.entities;
            native.act_guids = act.guids;
            native.act_root_components = act.root_components;
            report_unresolved_dimension_companions(&mut report, &native);
            report_unresolved_configuration_rules(&mut report, &native, &ir);
            report_design_projection_gaps(&mut report, &ir, &native);
            if !native.lost_edge_references.is_empty() {
                report.losses.push(LossNote {
                    category: LossCategory::Attribute,
                    severity: Severity::Warning,
                    message: format!(
                        "{} source parametric edge reference(s) were marked EDGE_REFERENCE_LOST and cannot be replayed without repair.",
                        native.lost_edge_references.len()
                    ),
                    provenance: None,
                });
            }
            ir.model.appearances = decoded_materials.appearances;
            ir.model.appearance_bindings = decoded_materials.bindings;
            resolve_face_appearance_bindings(&mut ir, &decoded_materials.face_assignments);
            apply_appearance_base_colors(&mut ir);
            ir.model.appearance_bindings.sort_by(|a, b| a.id.cmp(&b.id));
            if !ir.model.appearances.is_empty() {
                if decoded_materials.has_topology_assignments
                    && ir.model.appearance_bindings.is_empty()
                {
                    if let Some(loss) = report
                        .losses
                        .iter_mut()
                        .find(|loss| loss.category == LossCategory::Material)
                    {
                        loss.message = format!(
                            "{} Protein appearance asset(s) were decoded, but no topology assignment was resolved.",
                            ir.model.appearances.len()
                        );
                    }
                } else {
                    report
                        .losses
                        .retain(|loss| loss.category != LossCategory::Material);
                }
            }
            annotate_docstruct(&mut ir, &scan);
            match crate::xref::decode(&scan) {
                Ok(Some(table)) => {
                    native.xref_designs = table.designs;
                    native.xref_references = table.references;
                }
                Ok(None) => {}
                Err(error) => report.losses.push(xref_parse_loss(&error)),
            }
            native.store(ir.native.namespace_mut("f3d"))?;
            populate_annotations(&mut ir, &scan, &native, Some(&annotation_records));
            preserve_source_image(&scan, &mut ir)?;
            return Ok(DecodeResult::new(ir, report));
        }
    }

    // No decodable SAB stream: use container metadata.
    let mut ir = build_metadata_ir(&scan)?;
    let mut native = F3dNative::default();
    if let Some(active) = container::select_active_brep(&scan) {
        if let Some(history) = decode_asm_history(&scan, active)? {
            native.asm_histories.push(history);
        }
    }
    native.construction_recipes = crate::design::decode::parameters::decode_recipes(reader, &scan)?;
    native.persistent_references =
        crate::design::decode::sketch::decode_persistent_references(reader, &scan)?;
    native.lost_edge_references =
        crate::design::decode::sketch::decode_lost_edge_references(reader, &scan)?;
    native.design_material_assignments =
        crate::materials::decode_design_assignments(reader, &scan)?;
    native.design_objects = crate::design::decode::sketch::decode_objects(reader, &scan)?;
    native.design_parameters = crate::design::decode::parameters::decode_parameters(reader, &scan)?;
    native.design_entity_headers =
        crate::design::decode::sketch::decode_entity_headers(reader, &scan)?;
    native.design_record_headers = crate::design::decode::sketch::decode_record_headers(
        reader,
        &scan,
        &native.design_entity_headers,
    )?;
    let sketch_relations = {
        crate::design::decode::sketch::decode_sketch_relations(
            reader,
            &scan,
            &native.design_record_headers,
            &native.design_entity_headers,
        )?
    };
    native.sketch_relations = sketch_relations;
    extend_related_design_records(reader, &scan, &mut native)?;
    native.sketch_points = crate::design::decode::sketch::decode_sketch_points(reader, &scan)?;
    native.sketch_texts = crate::design::decode::sketch::decode_sketch_texts(&scan)?;
    native.sketch_curve_identities =
        crate::design::decode::sketch::decode_sketch_curve_identities(reader, &scan)?;
    native.sketch_surfaces = crate::design::decode::sketch::decode_sketch_surfaces(&scan)?;
    crate::design::decode::sketch::bind_sketch_graph(
        &native.design_entity_headers,
        &mut native.sketch_points,
        &mut native.sketch_curve_identities,
        &mut native.sketch_surfaces,
        &mut native.sketch_relations,
    )?;
    crate::design::decode::operands::bind_extrude_selection_geometry(
        &mut native.design_extrude_selection_members,
        &native.design_extrude_selection_groups,
        &native.design_parameter_scopes,
        &native.sketch_points,
        &native.sketch_curve_identities,
    );
    let dimension_inputs = crate::design::decode::dimension_frames::DimensionDecodeInputs {
        scan: &scan,
        parameters: &native.design_parameters,
        owners: &native.design_parameter_owners,
        companions: &native.design_parameter_companions,
        scopes: &native.design_parameter_scopes,
        headers: &native.design_record_headers,
        points: &native.sketch_points,
        curves: &native.sketch_curve_identities,
    };
    native.design_dimension_locus_pairs =
        crate::design::decode::dimension_frames::decode_dimension_locus_pairs(&dimension_inputs)?;
    native.design_dimension_annotation_frames =
        crate::design::decode::dimension_frames::decode_dimension_annotation_frames(
            &dimension_inputs,
            &native.design_entity_headers,
        )?;
    native.design_dimension_locus_groups =
        crate::design::decode::dimension_frames::decode_dimension_locus_groups(
            &dimension_inputs,
            &native.design_entity_headers,
        )?;
    native.design_dimension_null_locus_pairs =
        crate::design::decode::dimension_frames::decode_dimension_null_locus_pairs(
            &dimension_inputs,
            &native.design_dimension_locus_pairs,
            &native.design_dimension_locus_groups,
        )?;
    crate::design::dimensions::remove_dimension_frame_relations(
        &mut native.sketch_relations,
        &native.design_dimension_locus_pairs,
        &native.design_dimension_locus_groups,
        &native.design_dimension_null_locus_pairs,
    );
    crate::design::dimensions::bind_dimension_loci(
        &native.design_sketch_placements,
        &native.design_parameter_owners,
        &native.design_dimension_locus_pairs,
        &native.design_dimension_locus_groups,
        &native.design_dimension_annotation_frames,
        &native.design_dimension_null_locus_pairs,
        &mut native.sketch_points,
        &mut native.sketch_curve_identities,
    )?;
    native.design_body_members = crate::design::decode::body::decode_body_members(reader, &scan)?;
    native.design_body_bindings = crate::design::decode::body::decode_design_body_bindings(
        &scan,
        container::select_active_brep(&scan).map(|entry| entry.name.as_str()),
        &native.body_native_keys,
    )?;
    native.design_body_bounds =
        crate::design::decode::body::decode_body_bounds(&scan, &native.design_entity_headers)?;
    crate::design::decode::body::bind_body_bounds(
        &mut native.design_body_bounds,
        &native.design_body_bindings,
    );
    native.design_configurations = crate::design::configurations::decode_configurations(&scan)?;
    ir.model.configurations =
        crate::design::configurations::project_configurations(&native.design_configurations);
    (ir.model.features, ir.model.parameters) =
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            &crate::design::feature_project::ProjectInputs {
                native: &native.design_parameters,
                owners: &native.design_parameter_owners,
                scopes: &native.design_parameter_scopes,
                construction_groups: &native.design_construction_operand_groups,
                fillet_radius_groups: &native.design_fillet_radius_groups,
                edge_operands: &native.design_edge_operands,
                edge_identity_operands: &native.design_edge_identity_operands,
                face_operands: &native.design_face_operands,
                placements: &native.design_sketch_placements,
                body_bindings: &native.design_body_bindings,
            },
        );
    crate::design::feature_project::bind_form_cages(
        &scan,
        &native.design_parameter_scopes,
        &native.design_record_headers,
        &mut ir.model.features,
        &ir.model.subds,
    )?;
    crate::design::configurations::bind_configuration_parameter_overrides(
        &mut ir.model.configurations,
        &ir.model.parameters,
    );
    crate::design::configurations::bind_configuration_suppressed_features(
        &mut ir.model.configurations,
        &ir.model.features,
    );
    ir.model.feature_input_topologies = crate::history::project_feature_input_topologies(
        &ir.model.features,
        &native.design_parameter_scopes,
        &native.asm_histories,
        &native.design_edge_operands,
    );
    crate::history::bind_feature_outputs(
        &mut ir.model.features,
        &native.design_parameter_scopes,
        &native.asm_histories,
        &ir.model.bodies,
    );
    crate::history::bind_feature_body_selections(
        &mut ir.model.features,
        &native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.design_body_recipe_operands,
        &native.asm_histories,
    );
    crate::history::bind_feature_face_selections(
        &mut ir.model.features,
        &native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.design_face_operands,
        &native.design_body_recipe_operands,
        &native.asm_histories,
    );
    crate::history::bind_feature_path_selections(
        &mut ir.model.features,
        &native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.design_entity_selection_operands,
    );
    (ir.model.sketches, ir.model.sketch_entities) =
        crate::design::sketch_project::project_sketch_design(
            &native.design_sketch_placements,
            &native.sketch_points,
            &native.sketch_curve_identities,
            &native.sketch_texts,
            ir.tolerances.linear,
        );
    (ir.model.spatial_sketches, ir.model.spatial_sketch_entities) =
        crate::design::sketch_project::project_spatial_sketch_design(
            &native.design_sketch_placements,
            &native.sketch_points,
            &native.sketch_curve_identities,
            &native.sketch_surfaces,
            &native.sketch_relations,
            ir.tolerances.linear,
        );
    crate::design::decode::operands::bind_loft_sketch_selections(
        &scan,
        &native.design_construction_operand_groups,
        &native.design_record_headers,
        &crate::design::decode::operands::LoftSketchResolution {
            entities: &native.design_entity_headers,
            entity_selection_operands: &native.design_entity_selection_operands,
            placements: &native.design_sketch_placements,
            curve_identities: &native.sketch_curve_identities,
            spatial_sketches: &ir.model.spatial_sketches,
        },
        &mut ir.model.features,
    )?;
    crate::design::feature_project::bind_sketch_feature_geometry(
        &mut ir.model.features,
        &native.design_parameter_scopes,
        &native.design_sketch_placements,
        &ir.model.sketches,
        &ir.model.spatial_sketches,
    );
    ir.model.spatial_sketch_constraints =
        crate::design::sketch_project::project_spatial_sketch_constraints(
            &native.design_sketch_placements,
            &native.sketch_relations,
            &native.sketch_points,
            &native.sketch_curve_identities,
            &native.sketch_surfaces,
            &ir.model.spatial_sketch_entities,
        );
    crate::design::profile_select::bind_extrude_profile_selections(
        &mut ir.model.features,
        &native.design_parameter_scopes,
        &native.design_extrude_selection_groups,
        &native.design_extrude_selection_members,
        &ir.model.sketches,
        crate::design::profile_select::ExtrudeProfileResolution {
            entities: &ir.model.sketch_entities,
            spatial_sketches: &ir.model.spatial_sketches,
            spatial_entities: &ir.model.spatial_sketch_entities,
            histories: &native.asm_histories,
            linear_tolerance: ir.tolerances.linear,
        },
    );
    crate::design::face_resolve::bind_extrude_start_planes(
        &mut ir.model.features,
        &ir.model.sketches,
        &mut crate::design::face_resolve::ExtrudeStartPlaneResolution {
            faces: &ir.model.faces,
            surfaces: &ir.model.surfaces,
            groups: &native.design_construction_operand_groups,
            operands: &mut native.design_face_operands,
            linear_tolerance: ir.tolerances.linear,
            angular_tolerance: ir.tolerances.angular,
        },
    );
    ir.model.sketch_constraints = crate::design::constraints::project_sketch_constraints(
        &native.design_sketch_placements,
        &native.design_parameters,
        &native.sketch_points,
        &native.sketch_curve_identities,
        &native.sketch_texts,
        &native.sketch_relations,
        &ir.model.sketch_entities,
    );
    let constraint_inputs = crate::design::dimensions::DimensionConstraintInputs {
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
        entities: &ir.model.sketch_entities,
    };
    ir.model
        .sketch_constraints
        .extend(crate::design::dimensions::project_dimension_constraints(
            &constraint_inputs,
            &ir.model.spatial_sketches,
        ));
    ir.model.spatial_sketch_constraints.extend(
        crate::design::dimensions::project_spatial_dimension_constraints(
            &constraint_inputs,
            &ir.model.spatial_sketches,
            &ir.model.spatial_sketch_entities,
        ),
    );
    crate::design::dimensions::bind_offset_dimension_parameters(
        &mut ir.model.sketch_constraints,
        &native.design_parameters,
    );
    ir.model
        .sketch_constraints
        .sort_by_key(|constraint| constraint.id.clone());
    ir.model
        .spatial_sketch_constraints
        .sort_by_key(|constraint| constraint.id.clone());
    let act = crate::act::decode(reader, &scan)?;
    native.act_entities = act.entities;
    native.act_guids = act.guids;
    native.act_root_components = act.root_components;
    let decoded_materials = materials::decode(reader, &scan)?;
    ir.model.appearances = decoded_materials.appearances;
    ir.model.appearance_bindings = decoded_materials.bindings;
    annotate_docstruct(&mut ir, &scan);
    let xref_table = crate::xref::decode(&scan);
    if let Ok(Some(table)) = &xref_table {
        native.xref_designs.clone_from(&table.designs);
        native.xref_references.clone_from(&table.references);
    }
    native.store(ir.native.namespace_mut("f3d"))?;
    populate_annotations(&mut ir, &scan, &native, None);
    preserve_source_image(&scan, &mut ir)?;
    let mut report = build_container_report(&scan, false);
    report_unresolved_dimension_companions(&mut report, &native);
    match &xref_table {
        Ok(Some(table)) => apply_assembly_classification(&mut report, &scan, table),
        Ok(None) => {}
        Err(error) => report.losses.push(xref_parse_loss(error)),
    }
    Ok(DecodeResult::new(ir, report))
}

/// Record the `Properties.dat` docstruct declaration on the source metadata.
fn annotate_docstruct(ir: &mut CadIr, scan: &ContainerScan) {
    let Some(docstruct) = crate::xref::docstruct(scan) else {
        return;
    };
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert("docstruct_type".into(), docstruct.doc_type);
        source
            .attributes
            .insert("docstruct_subtype".into(), docstruct.subtype);
    }
}

/// A warning for a present but unparseable `RedirectionsStream.dat`.
fn xref_parse_loss(error: &CodecError) -> LossNote {
    LossNote {
        category: LossCategory::Metadata,
        severity: Severity::Warning,
        message: format!("external-reference table was not decoded: {error}"),
        provenance: None,
    }
}

/// Reclassify a BREP-less assembly document: its model is the placement of
/// its XREF targets, so producing no geometry is not a loss
/// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
fn apply_assembly_classification(
    report: &mut DecodeReport,
    scan: &ContainerScan,
    table: &crate::xref::XrefTable,
) {
    if !crate::xref::is_assembly(scan, Some(table)) {
        return;
    }
    report.losses.retain(|loss| {
        !(loss.severity >= Severity::Error
            && matches!(
                loss.category,
                LossCategory::Geometry | LossCategory::Topology
            ))
    });
    report.losses.push(LossNote {
        category: LossCategory::Geometry,
        severity: Severity::Info,
        message: format!(
            "assembly document: geometry is defined by {} external reference(s); decode the \
             containing .f3z archive to resolve them",
            table.references.len()
        ),
        provenance: None,
    });
    for reference in &table.references {
        let note = match crate::xref::design_for(table, reference) {
            Some(design) => format!(
                "xref {}: {} -> {} (lineage {}, version {}, neutronRole {})",
                reference.ordinal,
                design.display_name,
                design.target_file_name,
                design.lineage_urn,
                design.version_urn,
                reference.neutron_role
            ),
            None => format!(
                "xref {}: -> {} (neutronRole {})",
                reference.ordinal, reference.relative_path, reference.neutron_role
            ),
        };
        report.notes.push(note);
    }
}

fn preserve_source_image(scan: &ContainerScan, ir: &mut CadIr) -> Result<(), CodecError> {
    let id = crate::ids::FILE_SOURCE_IMAGE_ID;
    ir.push_native_unknown(
        "f3d",
        UnknownRecord {
            id: UnknownId(id.into()),
            offset: 0,
            byte_len: scan.source_image.len() as u64,
            sha256: sha256_hex(&scan.source_image),
            data: Some(scan.source_image.clone()),
            links: Vec::new(),
        },
    )?;
    ir.finalize();
    let hash = semantic_hash(ir);
    if let Some(source) = &mut ir.source {
        source.attributes.insert("semantic_sha256".into(), hash);
    }
    Ok(())
}

pub(crate) fn semantic_hash(ir: &CadIr) -> String {
    // Normalize with a field-by-field clone so the retained source image (the
    // largest single payload) is filtered out instead of copied and dropped.
    let mut normalized = ir.clone();
    normalized.finalize();
    normalized.source = ir.source.as_ref().map(|source| {
        let mut source = source.clone();
        source.attributes.remove("semantic_sha256");
        source
    });
    let unknowns = ir
        .native_unknowns("f3d")
        .unwrap_or_default()
        .into_iter()
        .filter(|record| record.id.0 != crate::ids::FILE_SOURCE_IMAGE_ID)
        .collect::<Vec<_>>();
    normalized
        .set_native_unknowns("f3d", &unknowns)
        .expect("F3D unknown records serialize");
    sha256_hex(
        normalized
            .to_canonical_json()
            .expect("CadIr serialization")
            .as_bytes(),
    )
}

fn populate_annotations(
    ir: &mut CadIr,
    scan: &ContainerScan,
    native: &F3dNative,
    brep: Option<&[brep::AnnotationRecord]>,
) {
    let mut annotations = AnnotationBuilder::new();
    if let Some(records) = brep {
        for record in records {
            let stream = annotations.stream(crate::ids::native_scope(&record.stream));
            annotations
                .note(&record.id, stream, record.offset)
                .tag(&record.tag);
            for field in &record.derived_fields {
                annotations.derived(&record.id, *field);
            }
        }
    }

    let native_stream = annotations.stream("f3d:native");
    let mut note = |id: &str, tag: &str| {
        let offset = trailing_offset(id);
        annotations.note(id, native_stream, offset).tag(tag);
    };
    {
        for entity in &native.construction_recipes {
            note(&entity.id, "construction_recipe");
        }
        for entity in &native.persistent_references {
            note(&entity.id, "persistent_reference");
        }
        for entity in &native.lost_edge_references {
            note(&entity.id, "EDGE_REFERENCE_LOST");
        }
        for entity in &native.design_objects {
            note(&entity.id, "design_object");
        }
        for entity in &native.design_parameters {
            note(&entity.id, "design_parameter");
        }
        for entity in &native.design_parameter_companions {
            note(&entity.id, "design_parameter_companion");
        }
        for entity in &native.design_dimension_locus_pairs {
            note(&entity.id, "design_dimension_locus_pair");
            if let Some(projected) = ir
                .model
                .sketch_constraints
                .iter()
                .find(|projected| projected.native_ref.as_deref() == Some(entity.id.as_str()))
            {
                note(&projected.id.0, "sketch_constraint");
            }
        }
        for entity in &native.design_dimension_annotation_frames {
            note(&entity.id, "design_dimension_annotation_frame");
            if let Some(projected) = ir
                .model
                .sketch_constraints
                .iter()
                .find(|projected| projected.native_ref.as_deref() == Some(entity.id.as_str()))
            {
                note(&projected.id.0, "sketch_constraint");
            }
        }
        for entity in &native.design_dimension_locus_groups {
            note(&entity.id, "design_dimension_locus_group");
            if let Some(projected) = ir
                .model
                .sketch_constraints
                .iter()
                .find(|projected| projected.native_ref.as_deref() == Some(entity.id.as_str()))
            {
                note(&projected.id.0, "sketch_constraint");
            }
        }
        for entity in &native.design_dimension_null_locus_pairs {
            note(&entity.id, "design_dimension_null_locus_pair");
            if let Some(projected) = ir
                .model
                .sketch_constraints
                .iter()
                .find(|projected| projected.native_ref.as_deref() == Some(entity.id.as_str()))
            {
                note(&projected.id.0, "sketch_constraint");
            }
        }
        for entity in &native.design_parameter_owners {
            note(&entity.id, "design_parameter_owner");
        }
        for entity in &native.design_parameter_scopes {
            note(&entity.id, "design_parameter_scope");
        }
        for entity in &native.design_edge_operands {
            note(&entity.id, "design_edge_operand");
        }
        for entity in &native.design_face_operands {
            note(&entity.id, "design_face_operand");
        }
        for entity in &native.design_sketch_placements {
            note(&entity.id, "design_sketch_placement");
            let planar = crate::ids::neutral_sketch_id(entity);
            if ir.model.sketches.iter().any(|sketch| sketch.id == planar) {
                note(&planar.0, "sketch");
            }
            let spatial = crate::ids::neutral_spatial_sketch_id(entity);
            if ir
                .model
                .spatial_sketches
                .iter()
                .any(|sketch| sketch.id == spatial)
            {
                note(&spatial.0, "spatial_sketch");
            }
        }
        for entity in &native.design_entity_headers {
            note(&entity.id, "design_entity_header");
        }
        for entity in &native.design_record_headers {
            note(&entity.id, "design_record_header");
        }
        for entity in &native.design_body_members {
            note(&entity.id, "BodiesRoot");
        }
        for entity in &native.design_material_assignments {
            note(&entity.id, "material_assignment");
        }
        for entity in &native.sketch_relations {
            note(&entity.id, "sketch_relation");
            if ir
                .model
                .sketch_constraints
                .iter()
                .any(|projected| projected.native_ref.as_deref() == Some(entity.id.as_str()))
            {
                note(
                    &crate::ids::neutral_sketch_constraint_id(&entity.id, entity.record_index).0,
                    "sketch_constraint",
                );
            }
        }
        for entity in &native.sketch_points {
            note(&entity.id, "sketch_point");
            if let Some(projected) = ir
                .model
                .sketch_entities
                .iter()
                .find(|projected| projected.native_ref.as_deref() == Some(entity.id.as_str()))
            {
                note(&projected.id.0, "sketch_entity");
            }
        }
        for entity in &native.sketch_curve_identities {
            note(&entity.id, "sketch_curve");
            if let Some(projected) = ir
                .model
                .sketch_entities
                .iter()
                .find(|projected| projected.native_ref.as_deref() == Some(entity.id.as_str()))
            {
                note(&projected.id.0, "sketch_entity");
            }
        }
        for entity in &native.sketch_surfaces {
            note(&entity.id, "sketch_surface");
        }
        for entity in &native.sketch_curve_links {
            note(&entity.id, "sketch_curve_link");
        }
        for entity in &native.persistent_design_links {
            note(&entity.id, "persistent_design_link");
        }
        for entity in &native.persistent_subentity_tags {
            note(&entity.id, "persistent_subentity_tag");
        }
        for entity in &native.act_entities {
            note(&entity.id, "ACTEntity");
        }
        for entity in &native.act_guids {
            note(&entity.id, "ACTGuid");
        }
        for entity in &native.act_root_components {
            note(&entity.id, "ACTRootComponent");
        }
        for history in &native.asm_histories {
            note(&history.id, "history_stream");
            for state in &history.states {
                note(&state.id, "delta_state");
                for board in &state.bulletin_boards {
                    note(&board.id, "BulletinBoard");
                    for change in &board.changes {
                        note(&change.id, "entity_change");
                    }
                }
                for record in &state.records {
                    note(&record.id, &record.name);
                }
            }
        }
    }

    let appearance_stream = scan
        .entries
        .iter()
        .find(|entry| entry.role == container::role::PROTEIN)
        .map(|entry| annotations.stream(crate::ids::native_scope(&entry.name)));
    if let Some(stream) = appearance_stream {
        for appearance in &ir.model.appearances {
            annotations
                .note(&appearance.id.0, stream, 0)
                .tag(appearance.schema.as_deref().unwrap_or("appearance"));
        }
    }
    for binding in &ir.model.appearance_bindings {
        annotations
            .note(&binding.id, native_stream, 0)
            .tag("appearance_binding");
    }
    if brep.is_none() {
        if let Some(active) = container::select_active_brep(scan) {
            let stream = annotations.stream(crate::ids::native_scope(&active.name));
            for unknown in ir.native_unknowns("f3d").unwrap_or_default() {
                annotations
                    .note(&unknown.id.0, stream, unknown.offset)
                    .tag("opaque_brep");
            }
        }
    }
    ir.annotations = annotations.build();
}

fn trailing_offset(id: &str) -> u64 {
    id.rsplit(':')
        .find_map(|part| part.parse::<u64>().ok())
        .unwrap_or(0)
}

fn decode_asm_history(
    scan: &ContainerScan,
    active: &BrepFacts,
) -> Result<Option<crate::history_records::AsmHistory>, CodecError> {
    let width = active.header.as_ref().map_or(8, |h| usize::from(h.width));
    let bytes = scan.entry_bytes(&active.name)?;
    Ok(crate::history::decode(bytes, &active.name, width))
}

fn extend_related_design_records(
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
    native: &mut F3dNative,
) -> Result<(), CodecError> {
    let indices = native
        .sketch_relations
        .iter()
        .flat_map(|relation| {
            let scope = crate::ids::native_stream(&relation.id)
                .unwrap_or(crate::ids::DEFAULT_STREAM)
                .to_owned();
            relation
                .members
                .iter()
                .chain(&relation.return_members)
                .map(move |record_index| (scope.clone(), *record_index))
        })
        .chain(native.design_parameters.iter().filter_map(|parameter| {
            Some((
                crate::ids::native_stream(&parameter.id)?.to_owned(),
                parameter.owner_record_index?,
            ))
        }))
        .collect::<Vec<_>>();
    let existing = native
        .design_record_headers
        .iter()
        .filter_map(|record| {
            Some((
                crate::ids::native_stream(&record.id)?.to_owned(),
                record.record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    native.design_record_headers.extend(
        crate::design::decode::sketch::decode_related_record_headers(reader, scan, &indices)?
            .into_iter()
            .filter(|record| {
                crate::ids::native_stream(&record.id).is_none_or(|scope| {
                    !existing.contains(&(scope.to_owned(), record.record_index))
                })
            }),
    );
    native
        .design_record_headers
        .sort_by_key(|record| record.id.clone());
    native.design_parameter_owners = crate::design::decode::parameters::decode_parameter_owners(
        scan,
        &native.design_parameters,
        &native.design_record_headers,
    )?;
    let indices = native
        .design_parameter_owners
        .iter()
        .flat_map(|owner| {
            let scope = crate::ids::native_stream(&owner.id)
                .unwrap_or(crate::ids::DEFAULT_STREAM)
                .to_owned();
            [
                owner.scope_record_index,
                owner.parameter_record_index,
                owner.companion_record_index,
            ]
            .map(|record_index| (scope.clone(), record_index))
        })
        .collect::<Vec<_>>();
    let existing = native
        .design_record_headers
        .iter()
        .filter_map(|record| {
            Some((
                crate::ids::native_stream(&record.id)?.to_owned(),
                record.record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    native.design_record_headers.extend(
        crate::design::decode::sketch::decode_related_record_headers(reader, scan, &indices)?
            .into_iter()
            .filter(|record| {
                crate::ids::native_stream(&record.id).is_none_or(|scope| {
                    !existing.contains(&(scope.to_owned(), record.record_index))
                })
            }),
    );
    native
        .design_record_headers
        .sort_by_key(|record| record.id.clone());
    native.design_parameter_companions =
        crate::design::decode::parameters::decode_parameter_companions(
            scan,
            &native.design_parameter_owners,
            &native.design_record_headers,
        )?;
    native.design_parameter_scopes = crate::design::decode::scopes::decode_parameter_scopes(
        scan,
        &native.design_entity_headers,
    )?;
    crate::design::decode::operands::disambiguate_fixed_fillet_parameters(
        &mut native.design_parameter_scopes,
        &native.design_parameter_owners,
    );
    let mut existing = native
        .design_record_headers
        .iter()
        .filter_map(|record| {
            Some((
                crate::ids::native_stream(&record.id)?.to_owned(),
                record.record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    for scope in &native.design_parameter_scopes {
        let Some(stream) = crate::ids::native_stream(&scope.id) else {
            continue;
        };
        if existing.insert((stream.to_owned(), scope.record_index)) {
            native
                .design_record_headers
                .push(crate::records::DesignRecordHeader {
                    id: format!("{stream}:design-record-header#{}", scope.byte_offset),
                    record_index: scope.record_index,
                    class_tag: scope.class_tag.clone(),
                    byte_offset: scope.byte_offset,
                });
        }
        if let Some(operation) = &scope.copy_paste_bodies_operation {
            if existing.insert((stream.to_owned(), operation.relation_record_index)) {
                native
                    .design_record_headers
                    .push(crate::records::DesignRecordHeader {
                        id: format!(
                            "{stream}:design-record-header#{}",
                            operation.relation_byte_offset
                        ),
                        record_index: operation.relation_record_index,
                        class_tag: operation.relation_class_tag.clone(),
                        byte_offset: operation.relation_byte_offset,
                    });
            }
        }
    }
    let indices = native
        .design_parameter_scopes
        .iter()
        .flat_map(|scope| {
            let stream = crate::ids::native_stream(&scope.id)
                .unwrap_or(crate::ids::DEFAULT_STREAM)
                .to_owned();
            scope
                .reference_members
                .iter()
                .map(move |record_index| (stream.clone(), *record_index))
        })
        .collect::<Vec<_>>();
    let existing = native
        .design_record_headers
        .iter()
        .filter_map(|record| {
            Some((
                crate::ids::native_stream(&record.id)?.to_owned(),
                record.record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    native.design_record_headers.extend(
        crate::design::decode::sketch::decode_related_record_headers(reader, scan, &indices)?
            .into_iter()
            .filter(|record| {
                crate::ids::native_stream(&record.id).is_none_or(|stream| {
                    !existing.contains(&(stream.to_owned(), record.record_index))
                })
            }),
    );
    native
        .design_record_headers
        .sort_by_key(|record| record.id.clone());
    crate::design::decode::operands::bind_sketch_profiles(
        scan,
        &mut native.design_parameter_scopes,
        &native.design_record_headers,
        &native.design_entity_headers,
    )?;
    native.design_construction_operand_groups =
        crate::design::decode::operands::decode_construction_operand_groups(
            scan,
            &native.design_parameter_scopes,
            &native.design_record_headers,
        )?;
    native.design_extrude_selection_groups =
        crate::design::decode::operands::decode_extrude_selection_groups(
            scan,
            &native.design_parameter_scopes,
            &native.design_record_headers,
        )?;
    let mut indices = native
        .design_extrude_selection_groups
        .iter()
        .flat_map(|group| {
            let stream = crate::ids::native_stream(&group.id)
                .unwrap_or(crate::ids::DEFAULT_STREAM)
                .to_owned();
            group
                .members
                .iter()
                .map(move |record_index| (stream.clone(), *record_index))
        })
        .collect::<Vec<_>>();
    indices.extend(
        native
            .design_construction_operand_groups
            .iter()
            .flat_map(|group| {
                let stream = crate::ids::native_stream(&group.id)
                    .unwrap_or(crate::ids::DEFAULT_STREAM)
                    .to_owned();
                std::iter::once((stream, group.identity_record_index))
            }),
    );
    let existing = native
        .design_record_headers
        .iter()
        .filter_map(|record| {
            Some((
                crate::ids::native_stream(&record.id)?.to_owned(),
                record.record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    native.design_record_headers.extend(
        crate::design::decode::sketch::decode_related_record_headers(reader, scan, &indices)?
            .into_iter()
            .filter(|record| {
                crate::ids::native_stream(&record.id).is_none_or(|stream| {
                    !existing.contains(&(stream.to_owned(), record.record_index))
                })
            }),
    );
    native
        .design_record_headers
        .sort_by_key(|record| record.id.clone());
    native.design_construction_operand_identities =
        crate::design::decode::operands::decode_construction_operand_identities(
            scan,
            &native.design_construction_operand_groups,
            &native.design_record_headers,
        )?;
    let scopes = native
        .design_parameter_scopes
        .iter()
        .filter_map(|scope| {
            Some((
                (
                    crate::ids::native_stream(&scope.id)?.to_owned(),
                    scope.record_index,
                ),
                scope.kind.as_str(),
            ))
        })
        .collect::<std::collections::HashMap<_, _>>();
    let identified_groups = native
        .design_construction_operand_identities
        .iter()
        .filter_map(|identity| {
            Some((
                crate::ids::native_stream(&identity.id)?.to_owned(),
                identity.group_record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    native.design_edge_identity_operands =
        crate::design::decode::operands::decode_edge_identity_operands(
            scan,
            &native.design_parameter_scopes,
            &native.design_construction_operand_groups,
            &native.design_record_headers,
        )?;
    let identity_member_groups = native
        .design_edge_identity_operands
        .iter()
        .filter_map(|operand| {
            Some((
                crate::ids::native_stream(&operand.id)?.to_owned(),
                operand.group_record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    native.design_construction_operand_groups.retain(|group| {
        let Some(stream) = crate::ids::native_stream(&group.id) else {
            return true;
        };
        let kind = scopes
            .get(&(stream.to_owned(), group.scope_record_index))
            .copied();
        !matches!(kind, Some("Congé" | "Chanfrein"))
            || identified_groups.contains(&(stream.to_owned(), group.record_index))
            || identity_member_groups.contains(&(stream.to_owned(), group.record_index))
    });
    native.design_fillet_radius_groups =
        crate::design::decode::operands::decode_fillet_radius_groups(
            &native.design_parameter_scopes,
            &native.design_construction_operand_groups,
            &native.design_parameter_owners,
            &native.design_parameters,
        );
    crate::design::decode::operands::bind_lost_edge_groups(
        &mut native.design_construction_operand_groups,
        &native.design_construction_operand_identities,
        &native.lost_edge_references,
    )?;
    let indices = native
        .design_construction_operand_identities
        .iter()
        .flat_map(|identity| {
            let stream = crate::ids::native_stream(&identity.id)
                .unwrap_or(crate::ids::DEFAULT_STREAM)
                .to_owned();
            identity
                .wrapper_record_indices
                .iter()
                .copied()
                .chain(std::iter::once(identity.following_record_index))
                .map(move |record_index| (stream.clone(), record_index))
        })
        .collect::<Vec<_>>();
    let existing = native
        .design_record_headers
        .iter()
        .filter_map(|record| {
            Some((
                crate::ids::native_stream(&record.id)?.to_owned(),
                record.record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    native.design_record_headers.extend(
        crate::design::decode::sketch::decode_related_record_headers(reader, scan, &indices)?
            .into_iter()
            .filter(|record| {
                crate::ids::native_stream(&record.id).is_none_or(|stream| {
                    !existing.contains(&(stream.to_owned(), record.record_index))
                })
            }),
    );
    native
        .design_record_headers
        .sort_by_key(|record| record.id.clone());
    native.design_extrude_selection_members =
        crate::design::decode::operands::decode_extrude_selection_members(
            scan,
            &native.design_extrude_selection_groups,
            &native.design_record_headers,
        )?;
    native.design_entity_selection_operands =
        crate::design::decode::operands::decode_entity_selection_operands(
            scan,
            &native.design_construction_operand_groups,
            &native.design_record_headers,
        )?;
    crate::history::bind_entity_selection_history(
        &mut native.design_entity_selection_operands,
        &native.design_parameter_scopes,
        &native.asm_histories,
    );
    native.design_body_recipe_operands =
        crate::design::decode::operands::decode_body_recipe_operands(
            scan,
            &native.design_construction_operand_groups,
            &native.design_record_headers,
            &native.construction_recipes,
        )?;
    crate::design::decode::operands::bind_body_recipe_operand_candidates(
        &mut native.design_body_recipe_operands,
        &native.persistent_subentity_tags,
    );
    crate::history::bind_body_recipe_operand_history_candidates(
        &mut native.design_body_recipe_operands,
        &native.design_parameter_scopes,
        &native.asm_histories,
    );
    crate::design::decode::operands::bind_extrude_selection_identities(
        &mut native.design_extrude_selection_members,
        &native.design_construction_operand_identities,
    );
    crate::history::bind_extrude_selection_history(
        &mut native.design_extrude_selection_members,
        &native.asm_histories,
    );
    crate::history::bind_edge_identity_history(
        &mut native.design_edge_identity_operands,
        &native.design_construction_operand_identities,
        &native.design_parameter_scopes,
        &native.asm_histories,
    );
    native.design_edge_operands = crate::design::decode::operands::decode_edge_operands(
        scan,
        &native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.design_record_headers,
        &native.construction_recipes,
    )?;
    crate::design::decode::operands::bind_edge_operand_candidates(
        &mut native.design_edge_operands,
        &native.construction_recipes,
        &native.persistent_subentity_tags,
    );
    crate::history::bind_edge_operand_history_candidates(
        &mut native.design_edge_operands,
        &native.design_parameter_scopes,
        &native.asm_histories,
    );
    native.design_face_operands = crate::design::decode::operands::decode_face_operands(
        scan,
        &native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.design_record_headers,
        &native.construction_recipes,
    )?;
    crate::design::decode::operands::bind_face_operand_candidates(
        &mut native.design_face_operands,
        &native.construction_recipes,
        &native.persistent_subentity_tags,
    );
    crate::history::bind_face_operand_history_candidates(
        &mut native.design_face_operands,
        &native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.asm_histories,
    );
    crate::history::bind_edge_identity_bounded_face_rules(
        &mut native.design_edge_identity_operands,
        &native.design_face_operands,
    );
    native.design_sketch_placements = crate::design::decode::sketch::decode_sketch_placements(
        scan,
        &native.design_parameter_scopes,
        &native.design_entity_headers,
    )?;
    let stream_lengths: std::collections::HashMap<String, usize> = scan
        .entries
        .iter()
        .filter(|entry| entry.role == container::role::BULKSTREAM && entry.name.contains("Design"))
        .map(|entry| {
            scan.entry_bytes(&entry.name)
                .map(|bytes| (crate::ids::native_scope(&entry.name), bytes.len()))
        })
        .collect::<Result<_, _>>()?;
    crate::design::decode::parameters::bind_parameter_companion_payloads(
        &mut native.design_parameter_companions,
        &native.design_parameters,
        &native.design_parameter_owners,
        &native.design_parameter_scopes,
        &native.design_record_headers,
        &native.construction_recipes,
        &stream_lengths,
    );
    native.design_dimension_recipe_records =
        crate::design::decode::dimension_frames::decode_dimension_recipe_records(
            scan,
            &native.design_parameters,
            &native.design_parameter_owners,
            &native.design_parameter_companions,
            &native.construction_recipes,
        )?;
    crate::design::decode::dimension_frames::bind_dimension_recipe_reference_candidates(
        &mut native.design_dimension_recipe_records,
        &native.persistent_subentity_tags,
    );
    crate::design::decode::dimension_frames::bind_dimension_recipe_edge_operands(
        &mut native.design_dimension_recipe_records,
        &native.design_edge_operands,
    );
    Ok(())
}

/// Frame and decode the active BREP's SAB stream. Returns `None` when the stream
/// is not a decodable `BinaryFile4`/`BinaryFile8` SAB, or frames but yields no
/// geometry (leaving the caller to fall back to the container-metadata IR).
fn try_decode_brep(
    _reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
    active: &BrepFacts,
) -> Result<Option<Brep>, CodecError> {
    let width = active.header.as_ref().map_or(0, |h| h.width);
    if width != 4 && width != 8 {
        return Ok(None);
    }

    let bytes = scan.entry_bytes(&active.name)?;
    let Some(start) = asm_header::record_stream_start(bytes) else {
        return Ok(None);
    };
    // A stream without a delta-state boundary is history-less: its final
    // `End-of-ASM-data` record ends at EOF without the `0x11` terminator, so
    // it needs the EOF-tolerant framer used for the history partition.
    let framed = match active.delta_state_offset {
        Some(limit) => sab::frame(bytes, start, limit, usize::from(width)),
        None => sab::frame_history(bytes, start, bytes.len(), usize::from(width)),
    };
    let records = match framed {
        Ok(r) if !r.is_empty() => r,
        _ => return Ok(None),
    };

    let decoded = brep::decode(&records, bytes, &active.name);
    if decoded.surfaces.is_empty() && decoded.points.is_empty() && decoded.faces.is_empty() {
        return Ok(None);
    }
    Ok(Some(decoded))
}

/// Assemble the IR document from the decoded B-rep graph.
fn build_geometry_ir(
    scan: &ContainerScan,
    active: &BrepFacts,
    brep: Brep,
) -> Result<(CadIr, F3dNative), CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let (source, tolerances) = source_and_tolerances(scan, active);
    ir.source = Some(source);
    ir.tolerances = tolerances;

    ir.model.bodies = brep.bodies;
    ir.model.regions = brep.regions;
    ir.model.shells = brep.shells;
    ir.model.faces = brep.faces;
    ir.model.loops = brep.loops;
    ir.model.coedges = brep.coedges;
    ir.model.edges = brep.edges;
    ir.model.vertices = brep.vertices;
    ir.model.points = brep.points;
    ir.model.surfaces = brep.surfaces;
    ir.model.curves = brep.curves;
    ir.model.pcurves = brep.pcurves;
    ir.model.procedural_surfaces = brep.procedural_surfaces;
    ir.model.procedural_curves = brep.procedural_curves;
    let native = F3dNative {
        body_native_keys: brep.body_native_keys,
        sketch_curve_links: brep.sketch_curve_links,
        persistent_design_links: brep.persistent_design_links,
        persistent_subentity_tags: brep.persistent_subentity_tags,
        edge_continuities: brep.edge_continuities,
        edge_ownerships: brep.edge_ownerships,
        vertex_ownerships: brep.vertex_ownerships,
        face_sidedness: brep.face_sidedness,
        tolerant_vertex_tails: brep.tolerant_vertex_tails,
        tolerant_edge_tails: brep.tolerant_edge_tails,
        tolerant_coedge_parameters: brep.tolerant_coedge_parameters,
        mesh_surface_sentinels: brep.mesh_surface_sentinels,
        wire_topologies: brep.wire_topologies,
        transform_hints: brep.transform_hints,
        creation_timestamps: brep.creation_timestamps,
        ..F3dNative::default()
    };
    ir.model.attributes = brep.attributes;
    ir.set_native_unknowns("f3d", &brep.unknowns)?;
    Ok((ir, native))
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
        if let (Some(resabs), Some(resnor)) = (h.linear, h.angular) {
            tolerances = Tolerances {
                linear: resabs,
                angular: resnor,
            };
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
fn format_kind_counts(counts: &std::collections::BTreeMap<String, usize>) -> String {
    counts
        .iter()
        .map(|(name, count)| format!("{name}={count}"))
        .collect::<Vec<_>>()
        .join(", ")
}

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
    if s.missing_face_surfaces > 0 {
        losses.push(LossNote {
            category: LossCategory::Topology,
            severity: Severity::Warning,
            message: format!(
                "{} face(s) were omitted because their required surface reference was null or dangling. Reference conditions: {}.",
                s.missing_face_surfaces,
                format_kind_counts(&s.missing_face_surface_kinds)
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
                 underlying surface shape is not. Native kinds: {}.",
                s.unknown_surface_faces,
                format_kind_counts(&s.unknown_surface_kinds)
            ),
            provenance: None,
        });
    }
    if s.mesh_surface_faces > 0 {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!(
                "{} face(s) use zero-payload mesh_surface sentinels. Their exact surfaces are absent by definition; the emitted unknown surface preserves that distinction from tessellation attributes.",
                s.mesh_surface_faces
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
                 attributed curve carrier. Native kinds: {}.",
                s.procedural_curve_edges,
                format_kind_counts(&s.procedural_curve_kinds)
            ),
            provenance: None,
        });
    }
    if s.undecoded_pcurve_refs > 0 {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{} coedge(s) carry an explicit UV pcurve reference with no decodable 2D \
                 carrier on the face surface's parameterization; those coedges were emitted \
                 without a pcurve. Native kinds: {}.",
                s.undecoded_pcurve_refs,
                format_kind_counts(&s.undecoded_pcurve_kinds)
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

fn build_metadata_ir(scan: &ContainerScan) -> Result<CadIr, CodecError> {
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
            if let (Some(resabs), Some(resnor)) = (h.linear, h.angular) {
                ir.tolerances = Tolerances {
                    linear: resabs,
                    angular: resnor,
                };
            }
        }

        ir.push_native_unknown(
            "f3d",
            UnknownRecord {
                id: UnknownId(format!("f3d:{}:unknown#0", brep.name)),
                offset: 0,
                byte_len: brep.uncompressed_len,
                sha256: brep.sha256.clone(),
                data: None,
                links: Vec::new(),
            },
        )?;
    }

    ir.source = Some(SourceMeta {
        format: "f3d".to_string(),
        attributes,
    });
    Ok(ir)
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
                 BinaryFile4/BinaryFile8 SAB (or its framing failed). {brep_count} BREP stream(s) \
                 were located, but no surfaces, curves, or points were produced."
            ),
            provenance: None,
        },
        LossNote {
            category: LossCategory::Topology,
            severity: Severity::Blocking,
            message:
                "B-rep topology graph (body/region/shell/face/loop/coedge/edge/vertex) was not \
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

/// Join per-face appearance assignments to BREP faces through the face GUID
/// carried by each face's `NEUTRON_Material_attrib_def` attribute
/// ([spec §8.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#82-materials)).
fn resolve_face_appearance_bindings(
    ir: &mut CadIr,
    face_assignments: &[materials::FaceAppearanceAssignment],
) {
    use cadmpeg_ir::appearance::{AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};

    if face_assignments.is_empty() {
        return;
    }
    let mut faces_by_guid: std::collections::HashMap<&str, Vec<cadmpeg_ir::ids::FaceId>> =
        std::collections::HashMap::new();
    for attribute in &ir.model.attributes {
        let AttributeTarget::Face(face) = &attribute.target else {
            continue;
        };
        let strings: Vec<&str> = attribute
            .values
            .iter()
            .filter_map(|value| match value {
                AttributeValue::String(value) => Some(value.as_str()),
                _ => None,
            })
            .collect();
        if !strings.contains(&"NEUTRON_Material_attrib_def") {
            continue;
        }
        for value in strings {
            if value.len() == 36 && value.matches('-').count() == 4 {
                faces_by_guid.entry(value).or_default().push(face.clone());
            }
        }
    }
    let mut bound_targets = ir
        .model
        .appearance_bindings
        .iter()
        .map(|binding| binding.target.clone())
        .collect::<std::collections::HashSet<_>>();
    for assignment in face_assignments {
        let Some(faces) = faces_by_guid.get(assignment.face_guid.as_str()) else {
            continue;
        };
        let Some(appearance) = ir.model.appearances.iter().find(|appearance| {
            appearance
                .visual_guid
                .as_deref()
                .is_some_and(|guid| materials::visual_guid_matches(guid, &assignment.visual_guid))
        }) else {
            continue;
        };
        for face in faces {
            let target = AppearanceTarget::Face(face.clone());
            if !bound_targets.insert(target.clone()) {
                continue;
            }
            ir.model.appearance_bindings.push(AppearanceBinding {
                id: format!(
                    "f3d:appearance:face#{}:{}",
                    assignment.face_guid, assignment.visual_guid
                ),
                target,
                appearance: appearance.id.clone(),
                source_entity_id: None,
                object_type: None,
                channels: std::collections::BTreeMap::new(),
            });
        }
    }
}

/// Fill absent explicit topology colors from uniquely bound appearance assets.
/// Native RGB/truecolor attributes remain authoritative on the same target.
fn apply_appearance_base_colors(ir: &mut CadIr) {
    use cadmpeg_ir::appearance::AppearanceTarget;

    let colors = ir
        .model
        .appearances
        .iter()
        .filter_map(|appearance| Some((appearance.id.clone(), appearance.base_color?)))
        .collect::<std::collections::HashMap<_, _>>();
    let mut targets = std::collections::HashMap::new();
    let mut ambiguous = std::collections::HashSet::new();
    for binding in &ir.model.appearance_bindings {
        let Some(color) = colors.get(&binding.appearance).copied() else {
            continue;
        };
        if targets.insert(binding.target.clone(), color).is_some() {
            ambiguous.insert(binding.target.clone());
        }
    }
    for body in &mut ir.model.bodies {
        let target = AppearanceTarget::Body(body.id.clone());
        if body.color.is_none() && !ambiguous.contains(&target) {
            body.color = targets.get(&target).copied();
        }
    }
    for face in &mut ir.model.faces {
        let target = AppearanceTarget::Face(face.id.clone());
        if face.color.is_none() && !ambiguous.contains(&target) {
            face.color = targets.get(&target).copied();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_appearance_base_colors, design_projection_gaps, unresolved_dimension_companion_count,
        DesignProjectionGaps,
    };
    use crate::native::F3dNative;
    use crate::records::{
        DesignBodyBinding, DesignDimensionLocusPair, DesignDimensionNullLocusPair,
        DesignDimensionRecipeRecord, DesignParameter, DesignParameterCompanion,
        DesignParameterKind, DesignParameterOwner, DesignParameterScope, DesignSketchPlacement,
        LostEdgeReference, SketchCurveIdentity, SketchPoint, SketchRelation,
    };

    #[test]
    fn design_projection_gaps_count_unresolved_body_map_pairs() {
        let ir = cadmpeg_ir::document::CadIr::empty(Default::default());
        let mut native = F3dNative::default();
        native.design_body_bindings.push(DesignBodyBinding {
            id: "f3d:design:body-binding#0".into(),
            stream: "Design/BulkStream.dat".into(),
            pair_count: 1,
            pair_ordinal: 0,
            asm_body_key: 0,
            asm_body_key_offset: 0,
            entity_suffix: 1,
            entity_suffix_offset: 8,
            blob_name: "BREP.snapshot.smb".into(),
            blob_name_offset: 16,
            body: None,
        });

        assert_eq!(
            design_projection_gaps(&ir, &native).unresolved_body_bindings,
            1
        );
    }

    #[test]
    fn design_projection_gaps_count_each_retained_selection_family() {
        use cadmpeg_ir::math::{Point2, Point3, Vector3};
        use cadmpeg_ir::sketches::{
            Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
            SketchEntityId, SketchGeometry, SketchId,
        };

        let mut ir = cadmpeg_ir::document::CadIr::empty(Default::default());
        ir.model.sketch_constraints.push(SketchConstraint {
            id: SketchConstraintId("constraint".into()),
            sketch: SketchId("sketch".into()),
            definition: SketchConstraintDefinition::Native {
                native_kind: "dimension".into(),
                native_state: None,
                entities: Vec::new(),
                parameter: None,
                operands: Vec::new(),
            },
            native_ref: Some("native:constraint".into()),
        });
        ir.model.features.push(
            serde_json::from_value(serde_json::json!({
                "id": "extrude",
                "ordinal": 0,
                "definition": {
                    "definition": "extrude",
                    "profile": {
                        "kind": "sketch_selection",
                        "value": {"sketch": "sketch", "selections": ["native:profile"]}
                    },
                    "start": {"kind": "profile_plane"},
                    "extent": {
                        "kind": "to_face",
                        "face": {"kind": "native", "value": "native:face"}
                    },
                    "op": "cut"
                }
            }))
            .expect("Extrude feature"),
        );
        ir.model.features.push(
            serde_json::from_value(serde_json::json!({
                "id": "fillet",
                "ordinal": 1,
                "definition": {
                    "definition": "fillet",
                    "groups": [
                        {
                            "edges": {"kind": "native", "value": "native:edges"},
                            "radius": {"kind": "constant", "radius": 1.0}
                        },
                        {
                            "edges": {"kind": "unresolved"},
                            "radius": {"kind": "constant", "radius": 2.0}
                        },
                        {
                            "edges": {
                                "kind": "historical_partial",
                                "value": {
                                    "state": "history-input",
                                    "edges": [],
                                    "unresolved": [
                                        "native:edge-operand#1",
                                        "f3d:test:lost-edge-reference#2"
                                    ],
                                    "native": "native:partial-edges"
                                }
                            },
                            "radius": {"kind": "constant", "radius": 3.0}
                        }
                    ]
                }
            }))
            .expect("Fillet feature"),
        );
        ir.model.features.push(
            serde_json::from_value(serde_json::json!({
                "id": "suppressed-fillet",
                "ordinal": 2,
                "suppressed": true,
                "definition": {
                    "definition": "fillet",
                    "groups": [{
                        "edges": {"kind": "native", "value": "native:suppressed-edges"},
                        "radius": {"kind": "constant", "radius": 3.0}
                    }]
                }
            }))
            .expect("suppressed Fillet feature"),
        );
        ir.model.features.push(
            serde_json::from_value(serde_json::json!({
                "id": "native-feature",
                "ordinal": 3,
                "definition": {
                    "definition": "native",
                    "kind": "unsupported",
                    "parameters": {},
                    "properties": {}
                }
            }))
            .expect("native feature"),
        );

        let mut native = F3dNative::default();
        native.design_sketch_placements.push(DesignSketchPlacement {
            member_run_head: false,
            id: "native:sketch-placement".into(),
            scope_record_index: Some(10),
            entity_id: "Sketch_1".into(),
            entity_suffix: 1,
            byte_offset: 0,
            class_tag: "000".into(),
            record_index: 10,
            frame_length: 1,
            transform: [[0.0; 4]; 4],
            transform_offset: None,
            paired_class_tag: "001".into(),
            paired_byte_offset: 1,
        });
        native.sketch_points.push(SketchPoint {
            id: "native:sketch-point".into(),
            record_index: 11,
            owner_reference: Some(1),
            class_tag: "000".into(),
            byte_offset: 0,
            coordinate_offset: 0,
            entity_genesis: None,
            persistent_id: 1,
            paired_reference: 0,
            coordinates: Point2::new(0.0, 0.0),
            raw_bytes: Vec::new(),
        });
        native.sketch_curve_identities.push(SketchCurveIdentity {
            id: "native:sketch-curve".into(),
            record_index: 12,
            owner_reference: Some(1),
            class_tag: "000".into(),
            byte_offset: 0,
            geometry_offset: 0,
            entity_genesis: None,
            primary_id: 1,
            secondary_id: 2,
            geometry: None,
        });
        native.lost_edge_references.push(LostEdgeReference {
            id: "f3d:test:lost-edge-reference#2".into(),
            record_byte_offset: 0,
            class_tag_offset: 0,
            class_tag: "000".into(),
            record_index: 0,
            record_index_offset: 0,
            byte_offset: 0,
            next_byte_offset: 1,
            next_class_tag: "001".into(),
            next_record_index: 1,
        });
        native.sketch_relations.push(SketchRelation {
            id: "native:unprojected-relation".into(),
            record_index: 1,
            class_tag: "000".into(),
            byte_offset: 0,
            state_offset: 0,
            owner_reference: 1,
            owner_entity_id: "0_1".into(),
            auxiliary_references: Vec::new(),
            auxiliary_reference_offsets: Vec::new(),
            members: Vec::new(),
            resolved_members: Vec::new(),
            member_offsets: Vec::new(),
            owner_reference_offset: 0,
            state: 0,
            constraint_kinds: Vec::new(),
            unknown_constraint_bits: 0,
            member_roles: Vec::new(),
            entity_genesis: None,
            pattern: None,
            return_members: Vec::new(),
            resolved_return_members: Vec::new(),
            return_member_offsets: Vec::new(),
            raw_bytes: Vec::new(),
        });
        native.design_parameters.push(DesignParameter {
            id: "f3d:test:design-parameter#2".into(),
            byte_offset: 0,
            class_tag: "000".into(),
            record_index: 2,
            prefix_value: 0,
            prefix_value_offset: 0,
            source_ordinal: 2,
            owner_record_index: Some(3),
            expression: "1 mm".into(),
            expression_offset: 0,
            source_kind: "Linear Dimension-2".into(),
            source_kind_offset: 0,
            kind: DesignParameterKind::Dimension,
            unit: Some("native-unit".into()),
            unit_offset: Some(0),
            name: "d2".into(),
            name_offset: 0,
            evaluated_value: 0.1,
            evaluated_value_offset: 0,
        });
        native.design_parameter_scopes.push(DesignParameterScope {
            id: "native:unprojected-scope".into(),
            byte_offset: 0,
            class_tag: "000".into(),
            record_index: 3,
            frame_length: 1,
            kind: "Unsupported".into(),
            kind_offset: 0,
            extrude_operation: None,
            extrude_operation_offset: None,
            extrude_extent: None,
            extrude_extent_offsets: None,
            extrude_direction_reversed: None,
            extrude_direction_reversed_offset: None,
            extrude_start: None,
            extrude_start_offset: None,
            coil_operation: None,
            coil_operation_offset: None,
            coil_extent: None,
            coil_extent_offset: None,
            coil_section: None,
            coil_section_offset: None,
            coil_section_placement: None,
            coil_section_placement_offset: None,
            coil_clockwise: None,
            coil_clockwise_offset: None,
            feature_ordinal: 1,
            feature_ordinal_offset: 0,
            history_state_id: None,
            history_state_id_offset: 0,
            previous_history_state_id: None,
            previous_history_state_id_offset: 0,
            reference_count_offset: 0,
            reference_members: Vec::new(),
            reference_member_offsets: Vec::new(),
            solid_primitive: None,
            direct_face_operation: None,
            move_operation: None,
            scale_operation: None,
            surface_stitch_operation: None,
            base_flange_operation: None,
            edge_flange_operation: None,
            hem_operation: None,
            fixed_extrude_parameters: None,
            fixed_fillet_parameters: None,
            fixed_chamfer_parameters: None,
            path_feature_construction: None,
            copy_paste_bodies_operation: None,
            base_feature_construction: None,
            work_plane_transform: None,
            work_plane_transform_offset: None,
            work_plane_reference: None,
            work_plane_reference_offset: None,
            work_point_position: None,
            work_point_position_offset: None,
            extrude_profile: None,
            base_flange_profile: None,
            entity_id: None,
            entity_suffix: None,
            entity_reference_offset: None,
            paired_class_tag: "001".into(),
            paired_byte_offset: 1,
        });
        assert_eq!(
            design_projection_gaps(&ir, &native),
            DesignProjectionGaps {
                unresolved_body_bindings: 0,
                native_features: 1,
                unprojected_feature_scopes: 1,
                unprojected_parameters: 1,
                untyped_parameter_units: 1,
                unresolved_expression_dependencies: 0,
                unprojected_history_dependencies: 0,
                ambiguous_history_dependencies: 0,
                native_constraints: 1,
                unprojected_sketch_placements: 1,
                unprojected_sketch_points: 1,
                unprojected_sketch_curves: 1,
                unprojected_sketch_surfaces: 0,
                unprojected_sketch_texts: 0,
                unprojected_sketch_relations: 1,
                unprojected_dimensions: 1,
                profile_selections: 1,
                face_selections: 1,
                body_selections: 0,
                partially_resolved_face_members: 0,
                native_edge_selections: 2,
                partially_resolved_edge_members: 1,
                unresolved_edge_selections: 1,
            }
        );

        native.sketch_points[0].owner_reference = None;
        native.sketch_curve_identities[0].owner_reference = None;
        let ownerless = design_projection_gaps(&ir, &native);
        assert_eq!(ownerless.unprojected_sketch_points, 0);
        assert_eq!(ownerless.unprojected_sketch_curves, 0);
        native.sketch_points[0].owner_reference = Some(1);
        native.sketch_curve_identities[0].owner_reference = Some(1);

        ir.model.sketches.push(Sketch {
            id: SketchId("sketch".into()),
            name: None,
            configuration: None,
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
            profiles: Vec::new(),
            native_ref: Some("native:sketch-placement".into()),
        });
        for (id, native_ref) in [
            ("point", "native:sketch-point"),
            ("curve", "native:sketch-curve"),
        ] {
            ir.model.sketch_entities.push(SketchEntity {
                id: SketchEntityId(id.into()),
                sketch: SketchId("sketch".into()),
                construction: false,
                native_ref: Some(native_ref.into()),
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Point {
                    position: Point2::new(0.0, 0.0),
                },
            });
        }
        let gaps = design_projection_gaps(&ir, &native);
        assert_eq!(gaps.unprojected_sketch_placements, 0);
        assert_eq!(gaps.unprojected_sketch_points, 0);
        assert_eq!(gaps.unprojected_sketch_curves, 0);

        ir.model.parameters.push(
            serde_json::from_value(serde_json::json!({
                "id": "parameter-2",
                "ordinal": 2,
                "name": "d2",
                "expression": "1 mm",
                "native_ref": "f3d:test:design-parameter#2"
            }))
            .expect("Design parameter"),
        );
        assert_eq!(
            design_projection_gaps(&ir, &native).unprojected_parameters,
            0
        );
    }

    #[test]
    fn design_projection_gaps_require_unique_scope_state_dependencies() {
        let scope = |record_index, current, previous| DesignParameterScope {
            id: format!("f3d:native:scope#{record_index}"),
            byte_offset: u64::from(record_index),
            class_tag: "000".into(),
            record_index,
            frame_length: 1,
            kind: "Unsupported".into(),
            kind_offset: 0,
            extrude_operation: None,
            extrude_operation_offset: None,
            extrude_extent: None,
            extrude_extent_offsets: None,
            extrude_direction_reversed: None,
            extrude_direction_reversed_offset: None,
            extrude_start: None,
            extrude_start_offset: None,
            coil_operation: None,
            coil_operation_offset: None,
            coil_extent: None,
            coil_extent_offset: None,
            coil_section: None,
            coil_section_offset: None,
            coil_section_placement: None,
            coil_section_placement_offset: None,
            coil_clockwise: None,
            coil_clockwise_offset: None,
            feature_ordinal: record_index,
            feature_ordinal_offset: 0,
            history_state_id: current,
            history_state_id_offset: 0,
            previous_history_state_id: previous,
            previous_history_state_id_offset: 0,
            reference_count_offset: 0,
            reference_members: Vec::new(),
            reference_member_offsets: Vec::new(),
            solid_primitive: None,
            direct_face_operation: None,
            move_operation: None,
            scale_operation: None,
            surface_stitch_operation: None,
            base_flange_operation: None,
            edge_flange_operation: None,
            hem_operation: None,
            fixed_extrude_parameters: None,
            fixed_fillet_parameters: None,
            fixed_chamfer_parameters: None,
            path_feature_construction: None,
            copy_paste_bodies_operation: None,
            base_feature_construction: None,
            work_plane_transform: None,
            work_plane_transform_offset: None,
            work_plane_reference: None,
            work_plane_reference_offset: None,
            work_point_position: None,
            work_point_position_offset: None,
            extrude_profile: None,
            base_flange_profile: None,
            entity_id: None,
            entity_suffix: None,
            entity_reference_offset: None,
            paired_class_tag: "001".into(),
            paired_byte_offset: u64::from(record_index) + 1,
        };
        let mut native = F3dNative::default();
        native.design_parameter_scopes = vec![
            scope(1, Some(10), None),
            scope(2, Some(11), Some(10)),
            scope(3, Some(20), None),
            scope(4, Some(20), None),
            scope(5, Some(21), Some(20)),
        ];
        let mut ir = cadmpeg_ir::document::CadIr::empty(Default::default());
        ir.model.features = native
            .design_parameter_scopes
            .iter()
            .map(|scope| {
                serde_json::from_value(serde_json::json!({
                    "id": format!("feature-{}", scope.record_index),
                    "ordinal": scope.record_index,
                    "definition": {
                        "definition": "native",
                        "kind": "Unsupported",
                        "parameters": {},
                        "properties": {}
                    },
                    "native_ref": scope.id
                }))
                .expect("native feature")
            })
            .collect();

        let gaps = design_projection_gaps(&ir, &native);
        assert_eq!(gaps.unprojected_feature_scopes, 0);
        assert_eq!(gaps.unprojected_history_dependencies, 1);
        assert_eq!(gaps.ambiguous_history_dependencies, 1);

        let predecessor = ir.model.features[0].id.clone();
        ir.model.features[1].dependencies.push(predecessor);
        let gaps = design_projection_gaps(&ir, &native);
        assert_eq!(gaps.unprojected_history_dependencies, 0);
        assert_eq!(gaps.ambiguous_history_dependencies, 1);
    }

    #[test]
    fn payload_bearing_dimension_companion_uses_the_governing_dimension_frame() {
        let stream = "f3d:test/BulkStream.dat";
        let mut native = F3dNative::default();
        native.design_parameters.push(DesignParameter {
            id: format!("{stream}:design-parameter#10"),
            byte_offset: 0,
            class_tag: "305".into(),
            record_index: 10,
            prefix_value: 0,
            prefix_value_offset: 22,
            source_ordinal: 0,
            owner_record_index: Some(20),
            expression: "5 mm".into(),
            expression_offset: 40,
            source_kind: "Linear Dimension-2".into(),
            source_kind_offset: 60,
            kind: DesignParameterKind::Dimension,
            unit: Some("mm".into()),
            unit_offset: Some(90),
            name: "d1".into(),
            name_offset: 100,
            evaluated_value: 0.5,
            evaluated_value_offset: 110,
        });
        native.design_parameter_owners.push(DesignParameterOwner {
            id: format!("{stream}:design-parameter-owner#20"),
            byte_offset: 120,
            class_tag: "292".into(),
            record_index: 20,
            scope_record_index: 1,
            local_ordinal: 0,
            evaluated_value: 0.5,
            evaluated_value_offset: 160,
            parameter_record_index: 10,
            owned_ordinal: 0,
            variant: 0,
            companion_record_index: 30,
        });
        native
            .design_parameter_companions
            .push(DesignParameterCompanion {
                id: format!("{stream}:design-parameter-companion#30"),
                byte_offset: 220,
                class_tag: "408".into(),
                record_index: 30,
                owner_record_index: 20,
                timestamp_micros: 1,
                timestamp_micros_offset: 262,
                payload_byte_offset: 278,
                payload_byte_length: 100,
                owned_recipe_ids: Vec::new(),
            });
        assert_eq!(unresolved_dimension_companion_count(&native), 1);

        let mut recipe_backed = native.clone();
        recipe_backed
            .design_dimension_recipe_records
            .push(DesignDimensionRecipeRecord {
                id: format!("{stream}:design-dimension-recipe-record#31"),
                companion_record_index: 30,
                recipe_ordinal: 0,
                recipe_id: format!("{stream}:construction-recipe#31"),
                byte_offset: 278,
                class_tag: "423".into(),
                record_index: 31,
                frame_length: 100,
                prefix_offset: 300,
                prefix_bytes: Vec::new(),
                references: Vec::new(),
                program_offset: 320,
                program: vec![-1],
                matching_edge_operand_ids: Vec::new(),
            });
        assert_eq!(unresolved_dimension_companion_count(&recipe_backed), 0);

        native
            .design_dimension_locus_pairs
            .push(DesignDimensionLocusPair {
                id: format!("{stream}:design-dimension-locus-pair#278"),
                companion_record_index: 99,
                governing_companion_record_index: 30,
                byte_offset: 278,
                class_tag: "423".into(),
                record_index: 31,
                frame_length: 100,
                opaque_index: 0,
                opaque_index_offset: 300,
                first_geometry_record_index: 40,
                first_geometry_reference_offset: 305,
                first_role: 1,
                first_role_offset: 315,
                second_geometry_record_index: 41,
                second_geometry_reference_offset: 320,
                second_role: 2,
                second_role_offset: 330,
                paired_class_tag: "259".into(),
                paired_byte_offset: 378,
            });
        assert_eq!(unresolved_dimension_companion_count(&native), 0);
        native.design_dimension_locus_pairs[0].companion_record_index = 30;
        native.design_dimension_locus_pairs[0].governing_companion_record_index = 99;
        assert_eq!(unresolved_dimension_companion_count(&native), 0);

        native.design_dimension_locus_pairs.clear();
        native
            .design_dimension_null_locus_pairs
            .push(DesignDimensionNullLocusPair {
                id: format!("{stream}:design-dimension-null-locus-pair#278"),
                companion_record_index: 99,
                governing_companion_record_index: 30,
                byte_offset: 278,
                class_tag: "423".into(),
                record_index: 31,
                frame_length: 100,
                null_reference_offset: 300,
                null_role: 14,
                null_role_offset: 305,
                geometry_record_index: 40,
                geometry_reference_offset: 310,
                geometry_role: 3,
                geometry_role_offset: 320,
                paired_class_tag: "259".into(),
                paired_byte_offset: 378,
            });
        assert_eq!(unresolved_dimension_companion_count(&native), 0);
        native.design_dimension_null_locus_pairs[0].companion_record_index = 30;
        native.design_dimension_null_locus_pairs[0].governing_companion_record_index = 99;
        assert_eq!(unresolved_dimension_companion_count(&native), 0);
    }

    #[test]
    fn appearance_base_colors_fill_only_uncolored_unambiguous_targets() {
        use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
        use cadmpeg_ir::ids::AppearanceId;
        use cadmpeg_ir::topology::Color;

        let mut ir = cadmpeg_ir::examples::unit_cube();
        let body = ir.model.bodies[0].id.clone();
        let first_face = ir.model.faces[0].id.clone();
        let second_face = ir.model.faces[1].id.clone();
        let direct = Color {
            r: 0.9,
            g: 0.8,
            b: 0.7,
            a: 1.0,
        };
        let material = Color {
            r: 0.1,
            g: 0.2,
            b: 0.3,
            a: 1.0,
        };
        ir.model.bodies[0].color = Some(direct);
        ir.model.appearances.push(Appearance {
            id: AppearanceId("f3d:appearance#material".into()),
            name: None,
            asset_guid: None,
            visual_guid: None,
            physical_token: None,
            schema: None,
            category: None,
            base_color: Some(material),
            properties: Default::default(),
            textures: Vec::new(),
        });
        let binding = |id: &str, target| AppearanceBinding {
            id: id.into(),
            target,
            appearance: AppearanceId("f3d:appearance#material".into()),
            source_entity_id: None,
            object_type: None,
            channels: Default::default(),
        };
        ir.model.appearance_bindings = vec![
            binding("body", AppearanceTarget::Body(body)),
            binding("face", AppearanceTarget::Face(first_face)),
            binding("ambiguous-a", AppearanceTarget::Face(second_face.clone())),
            binding("ambiguous-b", AppearanceTarget::Face(second_face)),
        ];

        apply_appearance_base_colors(&mut ir);
        assert_eq!(ir.model.bodies[0].color, Some(direct));
        assert_eq!(ir.model.faces[0].color, Some(material));
        assert_eq!(ir.model.faces[1].color, None);
    }
}
