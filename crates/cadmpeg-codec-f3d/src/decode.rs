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
                    crate::design::native_stream(&parameter.id).unwrap_or("f3d:design"),
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
            let stream = crate::design::native_stream(&owner.id).unwrap_or("f3d:design");
            (parameters.get(&(stream, owner.parameter_record_index))
                == Some(&crate::records::DesignParameterKind::Dimension))
            .then_some((stream, owner.record_index))
        })
        .collect::<HashSet<_>>();
    let mut typed = HashSet::new();
    for pair in &native.design_dimension_locus_pairs {
        typed.insert((
            crate::design::native_stream(&pair.id).unwrap_or("f3d:design"),
            pair.companion_record_index,
        ));
    }
    for group in &native.design_dimension_locus_groups {
        typed.insert((
            crate::design::native_stream(&group.id).unwrap_or("f3d:design"),
            group.companion_record_index,
        ));
    }
    for pair in &native.design_dimension_null_locus_pairs {
        typed.insert((
            crate::design::native_stream(&pair.id).unwrap_or("f3d:design"),
            pair.companion_record_index,
        ));
    }
    for record in &native.design_dimension_recipe_records {
        typed.insert((
            crate::design::native_stream(&record.id).unwrap_or("f3d:design"),
            record.companion_record_index,
        ));
    }
    native
        .design_parameter_companions
        .iter()
        .filter(|companion| {
            let stream = crate::design::native_stream(&companion.id).unwrap_or("f3d:design");
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
    let count = crate::design::unresolved_configuration_rule_count(
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
    let count =
        crate::design::unresolved_configuration_parameter_override_count(&ir.model.configurations);
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
    let count =
        crate::design::unresolved_configuration_suppressed_feature_count(&ir.model.configurations);
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
    native_features: usize,
    native_constraints: usize,
    profile_selections: usize,
    face_selections: usize,
    native_edge_selections: usize,
    unresolved_edge_selections: usize,
}

fn design_projection_gaps(ir: &CadIr) -> DesignProjectionGaps {
    use cadmpeg_ir::features::{EdgeSelection, Extent, ExtrudeStart, FaceSelection};
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};
    use cadmpeg_ir::sketches::SketchConstraintDefinition;

    let mut gaps = DesignProjectionGaps {
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
            .count(),
        ..DesignProjectionGaps::default()
    };
    let mut edge_selection = |selection: &EdgeSelection| match selection {
        EdgeSelection::Native(_) => gaps.native_edge_selections += 1,
        EdgeSelection::Unresolved => gaps.unresolved_edge_selections += 1,
        EdgeSelection::Edges(_)
        | EdgeSelection::Resolved { .. }
        | EdgeSelection::Historical { .. } => {}
    };
    for feature in &ir.model.features {
        if feature.suppressed {
            continue;
        }
        match &feature.definition {
            FeatureDefinition::Native { .. } => gaps.native_features += 1,
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
                if matches!(
                    start,
                    ExtrudeStart::FromFace {
                        face: FaceSelection::Native(_) | FaceSelection::Unresolved,
                        ..
                    }
                ) {
                    gaps.face_selections += 1;
                }
                if matches!(
                    extent,
                    Extent::ToFace {
                        face: FaceSelection::Native(_) | FaceSelection::Unresolved,
                        ..
                    }
                ) {
                    gaps.face_selections += 1;
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
            _ => {}
        }
    }
    gaps
}

fn report_design_projection_gaps(report: &mut DecodeReport, ir: &CadIr) {
    let gaps = design_projection_gaps(ir);
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
            "{} active feature scope(s) retain native operation semantics because no complete neutral feature definition was resolved.",
            gaps.native_features
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
        gaps.native_edge_selections,
        format!(
            "{} edge-treatment selection(s) retain native construction recipes because no neutral historical edge selection was resolved.",
            gaps.native_edge_selections
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

/// Decode a `.f3d` reader into a document and its loss report.
pub fn decode(
    reader: &mut dyn ReadSeek,
    options: &DecodeOptions,
) -> Result<DecodeResult, CodecError> {
    let scan = container::scan(reader)?;

    if options.container_only {
        let mut ir = build_metadata_ir(&scan)?;
        populate_annotations(&mut ir, &scan, &F3dNative::default(), None);
        preserve_source_image(&scan, &mut ir)?;
        let report = build_container_report(&scan, true);
        return Ok(DecodeResult::new(ir, report));
    }

    // `try_decode_brep` returns `Some` after producing carriers or points.
    // A framed stream with no geometry uses the metadata-only path.
    if let Some(active) = container::select_active_brep(&scan).cloned() {
        if let Some((mut brep, mut report)) = try_decode_brep(reader, &scan, &active)? {
            let decoded_materials = materials::decode_with_bodies(reader, &scan, &brep.body_keys)?;
            let body_visibility =
                crate::design::decode_body_visibility(reader, &scan, &active.name)?;
            let mut body_visibilities = Vec::new();
            for body in &mut brep.bodies {
                if let Some((asm_body_key, visibility)) =
                    brep.body_keys.get(&body.id).and_then(|key| {
                        body_visibility
                            .get(key)
                            .map(|visibility| (*key, visibility))
                    })
                {
                    body.visible = Some(visibility.visible);
                    body_visibilities.push(crate::records::BodyVisibility {
                        id: format!("f3d:design:body-visibility#{asm_body_key}"),
                        body: body.id.clone(),
                        stream: visibility.stream.clone(),
                        byte_offset: visibility.byte_offset,
                        asm_body_key_offset: visibility.asm_body_key_offset,
                        asm_body_key,
                        entity_suffix: visibility.entity_suffix,
                        visible: visibility.visible,
                    });
                }
            }
            let annotation_records = std::mem::take(&mut brep.annotation_records);
            let (mut ir, mut native) = build_geometry_ir(&scan, &active, brep)?;
            native.body_visibilities = body_visibilities;
            if let Some(history) = decode_asm_history(&scan, &active)? {
                native.asm_histories.push(history);
            }
            native.construction_recipes = crate::design::decode_recipes(reader, &scan)?;
            native.persistent_references =
                crate::design::decode_persistent_references(reader, &scan)?;
            native.lost_edge_references =
                crate::design::decode_lost_edge_references(reader, &scan)?;
            native.design_material_assignments =
                crate::materials::decode_design_assignments(reader, &scan)?;
            native.design_objects = crate::design::decode_objects(reader, &scan)?;
            native.design_parameters = crate::design::decode_parameters(reader, &scan)?;
            native.design_entity_headers = crate::design::decode_entity_headers(reader, &scan)?;
            native.design_record_headers =
                crate::design::decode_record_headers(reader, &scan, &native.design_entity_headers)?;
            let sketch_relations = {
                crate::design::decode_sketch_relations(
                    reader,
                    &scan,
                    &native.design_record_headers,
                    &native.design_entity_headers,
                )?
            };
            native.sketch_relations = sketch_relations;
            extend_related_design_records(reader, &scan, &mut native)?;
            native.sketch_points = crate::design::decode_sketch_points(reader, &scan)?;
            native.sketch_curve_identities =
                crate::design::decode_sketch_curve_identities(reader, &scan)?;
            crate::design::bind_sketch_graph(
                &native.design_entity_headers,
                &mut native.sketch_points,
                &mut native.sketch_curve_identities,
                &mut native.sketch_relations,
            )?;
            crate::design::bind_extrude_selection_geometry(
                &mut native.design_extrude_selection_members,
                &native.design_extrude_selection_groups,
                &native.design_parameter_scopes,
                &native.sketch_points,
                &native.sketch_curve_identities,
            );
            native.design_dimension_locus_pairs = crate::design::decode_dimension_locus_pairs(
                &scan,
                &native.design_parameters,
                &native.design_parameter_owners,
                &native.design_parameter_companions,
                &native.design_parameter_scopes,
                &native.design_record_headers,
                &native.sketch_points,
                &native.sketch_curve_identities,
            )?;
            native.design_dimension_locus_groups = crate::design::decode_dimension_locus_groups(
                &scan,
                &native.design_parameters,
                &native.design_parameter_owners,
                &native.design_parameter_companions,
                &native.design_parameter_scopes,
                &native.design_record_headers,
                &native.design_entity_headers,
                &native.sketch_points,
                &native.sketch_curve_identities,
            )?;
            native.design_dimension_null_locus_pairs =
                crate::design::decode_dimension_null_locus_pairs(
                    &scan,
                    &native.design_parameters,
                    &native.design_parameter_owners,
                    &native.design_parameter_companions,
                    &native.design_parameter_scopes,
                    &native.design_record_headers,
                    &native.design_sketch_placements,
                    &native.design_dimension_locus_pairs,
                    &native.design_dimension_locus_groups,
                    &native.sketch_points,
                    &native.sketch_curve_identities,
                )?;
            crate::design::remove_dimension_frame_relations(
                &mut native.sketch_relations,
                &native.design_dimension_locus_pairs,
                &native.design_dimension_locus_groups,
                &native.design_dimension_null_locus_pairs,
            );
            crate::design::bind_dimension_loci(
                &native.design_sketch_placements,
                &native.design_parameter_owners,
                &native.design_dimension_locus_pairs,
                &native.design_dimension_locus_groups,
                &native.design_dimension_null_locus_pairs,
                &mut native.sketch_points,
                &mut native.sketch_curve_identities,
            )?;
            native.design_body_members = crate::design::decode_body_members(reader, &scan)?;
            native.design_body_bindings = crate::design::decode_design_body_bindings(
                &scan,
                Some(&active.name),
                &native.body_native_keys,
            )?;
            native.design_body_bounds =
                crate::design::decode_body_bounds(&scan, &native.design_entity_headers)?;
            crate::design::bind_body_bounds(
                &mut native.design_body_bounds,
                &native.design_body_bindings,
            );
            native.design_configurations = crate::design::decode_configurations(&scan)?;
            ir.model.configurations =
                crate::design::project_configurations(&native.design_configurations);
            (ir.model.features, ir.model.parameters) = crate::design::project_parameter_design(
                &native.design_parameters,
                &native.design_parameter_owners,
                &native.design_parameter_scopes,
                &native.design_construction_operand_groups,
                &native.design_fillet_radius_groups,
                &native.design_edge_operands,
                &native.design_face_operands,
                &native.design_sketch_placements,
            );
            crate::design::bind_configuration_parameter_overrides(
                &mut ir.model.configurations,
                &ir.model.parameters,
            );
            crate::design::bind_configuration_suppressed_features(
                &mut ir.model.configurations,
                &ir.model.features,
            );
            ir.model.feature_input_topologies = crate::history::project_feature_input_topologies(
                &ir.model.features,
                &native.design_parameter_scopes,
                &native.asm_histories,
            );
            crate::history::bind_feature_outputs(
                &mut ir.model.features,
                &native.design_parameter_scopes,
                &native.asm_histories,
                &ir.model.bodies,
            );
            crate::history::bind_feature_face_selections(
                &mut ir.model.features,
                &native.design_parameter_scopes,
                &native.design_construction_operand_groups,
                &native.design_face_operands,
                &native.asm_histories,
            );
            (ir.model.sketches, ir.model.sketch_entities) = crate::design::project_sketch_design(
                &native.design_sketch_placements,
                &native.sketch_points,
                &native.sketch_curve_identities,
                ir.tolerances.linear,
            );
            crate::design::bind_extrude_profile_selections(
                &mut ir.model.features,
                &native.design_parameter_scopes,
                &native.design_extrude_selection_groups,
                &native.design_extrude_selection_members,
                &ir.model.sketches,
                crate::design::ExtrudeProfileResolution {
                    entities: &ir.model.sketch_entities,
                    histories: &native.asm_histories,
                    linear_tolerance: ir.tolerances.linear,
                },
            );
            crate::design::bind_extrude_start_planes(
                &mut ir.model.features,
                &ir.model.sketches,
                crate::design::ExtrudeStartPlaneResolution {
                    faces: &ir.model.faces,
                    surfaces: &ir.model.surfaces,
                    groups: &native.design_construction_operand_groups,
                    operands: &mut native.design_face_operands,
                    linear_tolerance: ir.tolerances.linear,
                    angular_tolerance: ir.tolerances.angular,
                },
            );
            ir.model.sketch_constraints = crate::design::project_sketch_constraints(
                &native.design_sketch_placements,
                &native.sketch_points,
                &native.sketch_curve_identities,
                &native.sketch_relations,
                &ir.model.sketch_entities,
            );
            ir.model
                .sketch_constraints
                .extend(crate::design::project_dimension_constraints(
                    &native.design_sketch_placements,
                    &native.design_parameters,
                    &native.design_parameter_owners,
                    &native.design_dimension_locus_pairs,
                    &native.design_dimension_locus_groups,
                    &native.design_dimension_null_locus_pairs,
                    &native.design_parameter_companions,
                    &native.design_dimension_recipe_records,
                    &native.sketch_points,
                    &native.sketch_curve_identities,
                    &ir.model.sketch_entities,
                ));
            ir.model
                .sketch_constraints
                .sort_by_key(|constraint| constraint.id.clone());
            let act = crate::act::decode(reader, &scan)?;
            native.act_entities = act.entities;
            native.act_guids = act.guids;
            native.act_root_components = act.root_components;
            report_unresolved_dimension_companions(&mut report, &native);
            report_unresolved_configuration_rules(&mut report, &native, &ir);
            report_design_projection_gaps(&mut report, &ir);
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
            ir.model.appearance_bindings.sort_by(|a, b| a.id.cmp(&b.id));
            if !ir.model.appearances.is_empty() {
                if ir.model.appearance_bindings.is_empty() {
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
            native.store(ir.native.namespace_mut("f3d"))?;
            populate_annotations(
                &mut ir,
                &scan,
                &native,
                Some((&active.name, &annotation_records)),
            );
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
    native.construction_recipes = crate::design::decode_recipes(reader, &scan)?;
    native.persistent_references = crate::design::decode_persistent_references(reader, &scan)?;
    native.lost_edge_references = crate::design::decode_lost_edge_references(reader, &scan)?;
    native.design_material_assignments =
        crate::materials::decode_design_assignments(reader, &scan)?;
    native.design_objects = crate::design::decode_objects(reader, &scan)?;
    native.design_parameters = crate::design::decode_parameters(reader, &scan)?;
    native.design_entity_headers = crate::design::decode_entity_headers(reader, &scan)?;
    native.design_record_headers =
        crate::design::decode_record_headers(reader, &scan, &native.design_entity_headers)?;
    let sketch_relations = {
        crate::design::decode_sketch_relations(
            reader,
            &scan,
            &native.design_record_headers,
            &native.design_entity_headers,
        )?
    };
    native.sketch_relations = sketch_relations;
    extend_related_design_records(reader, &scan, &mut native)?;
    native.sketch_points = crate::design::decode_sketch_points(reader, &scan)?;
    native.sketch_curve_identities = crate::design::decode_sketch_curve_identities(reader, &scan)?;
    crate::design::bind_sketch_graph(
        &native.design_entity_headers,
        &mut native.sketch_points,
        &mut native.sketch_curve_identities,
        &mut native.sketch_relations,
    )?;
    crate::design::bind_extrude_selection_geometry(
        &mut native.design_extrude_selection_members,
        &native.design_extrude_selection_groups,
        &native.design_parameter_scopes,
        &native.sketch_points,
        &native.sketch_curve_identities,
    );
    native.design_dimension_locus_pairs = crate::design::decode_dimension_locus_pairs(
        &scan,
        &native.design_parameters,
        &native.design_parameter_owners,
        &native.design_parameter_companions,
        &native.design_parameter_scopes,
        &native.design_record_headers,
        &native.sketch_points,
        &native.sketch_curve_identities,
    )?;
    native.design_dimension_locus_groups = crate::design::decode_dimension_locus_groups(
        &scan,
        &native.design_parameters,
        &native.design_parameter_owners,
        &native.design_parameter_companions,
        &native.design_parameter_scopes,
        &native.design_record_headers,
        &native.design_entity_headers,
        &native.sketch_points,
        &native.sketch_curve_identities,
    )?;
    native.design_dimension_null_locus_pairs = crate::design::decode_dimension_null_locus_pairs(
        &scan,
        &native.design_parameters,
        &native.design_parameter_owners,
        &native.design_parameter_companions,
        &native.design_parameter_scopes,
        &native.design_record_headers,
        &native.design_sketch_placements,
        &native.design_dimension_locus_pairs,
        &native.design_dimension_locus_groups,
        &native.sketch_points,
        &native.sketch_curve_identities,
    )?;
    crate::design::remove_dimension_frame_relations(
        &mut native.sketch_relations,
        &native.design_dimension_locus_pairs,
        &native.design_dimension_locus_groups,
        &native.design_dimension_null_locus_pairs,
    );
    crate::design::bind_dimension_loci(
        &native.design_sketch_placements,
        &native.design_parameter_owners,
        &native.design_dimension_locus_pairs,
        &native.design_dimension_locus_groups,
        &native.design_dimension_null_locus_pairs,
        &mut native.sketch_points,
        &mut native.sketch_curve_identities,
    )?;
    native.design_body_members = crate::design::decode_body_members(reader, &scan)?;
    native.design_body_bindings = crate::design::decode_design_body_bindings(
        &scan,
        container::select_active_brep(&scan).map(|entry| entry.name.as_str()),
        &native.body_native_keys,
    )?;
    native.design_body_bounds =
        crate::design::decode_body_bounds(&scan, &native.design_entity_headers)?;
    crate::design::bind_body_bounds(&mut native.design_body_bounds, &native.design_body_bindings);
    native.design_configurations = crate::design::decode_configurations(&scan)?;
    ir.model.configurations = crate::design::project_configurations(&native.design_configurations);
    (ir.model.features, ir.model.parameters) = crate::design::project_parameter_design(
        &native.design_parameters,
        &native.design_parameter_owners,
        &native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.design_fillet_radius_groups,
        &native.design_edge_operands,
        &native.design_face_operands,
        &native.design_sketch_placements,
    );
    crate::design::bind_configuration_parameter_overrides(
        &mut ir.model.configurations,
        &ir.model.parameters,
    );
    crate::design::bind_configuration_suppressed_features(
        &mut ir.model.configurations,
        &ir.model.features,
    );
    ir.model.feature_input_topologies = crate::history::project_feature_input_topologies(
        &ir.model.features,
        &native.design_parameter_scopes,
        &native.asm_histories,
    );
    crate::history::bind_feature_outputs(
        &mut ir.model.features,
        &native.design_parameter_scopes,
        &native.asm_histories,
        &ir.model.bodies,
    );
    crate::history::bind_feature_face_selections(
        &mut ir.model.features,
        &native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.design_face_operands,
        &native.asm_histories,
    );
    (ir.model.sketches, ir.model.sketch_entities) = crate::design::project_sketch_design(
        &native.design_sketch_placements,
        &native.sketch_points,
        &native.sketch_curve_identities,
        ir.tolerances.linear,
    );
    crate::design::bind_extrude_profile_selections(
        &mut ir.model.features,
        &native.design_parameter_scopes,
        &native.design_extrude_selection_groups,
        &native.design_extrude_selection_members,
        &ir.model.sketches,
        crate::design::ExtrudeProfileResolution {
            entities: &ir.model.sketch_entities,
            histories: &native.asm_histories,
            linear_tolerance: ir.tolerances.linear,
        },
    );
    crate::design::bind_extrude_start_planes(
        &mut ir.model.features,
        &ir.model.sketches,
        crate::design::ExtrudeStartPlaneResolution {
            faces: &ir.model.faces,
            surfaces: &ir.model.surfaces,
            groups: &native.design_construction_operand_groups,
            operands: &mut native.design_face_operands,
            linear_tolerance: ir.tolerances.linear,
            angular_tolerance: ir.tolerances.angular,
        },
    );
    ir.model.sketch_constraints = crate::design::project_sketch_constraints(
        &native.design_sketch_placements,
        &native.sketch_points,
        &native.sketch_curve_identities,
        &native.sketch_relations,
        &ir.model.sketch_entities,
    );
    ir.model
        .sketch_constraints
        .extend(crate::design::project_dimension_constraints(
            &native.design_sketch_placements,
            &native.design_parameters,
            &native.design_parameter_owners,
            &native.design_dimension_locus_pairs,
            &native.design_dimension_locus_groups,
            &native.design_dimension_null_locus_pairs,
            &native.design_parameter_companions,
            &native.design_dimension_recipe_records,
            &native.sketch_points,
            &native.sketch_curve_identities,
            &ir.model.sketch_entities,
        ));
    ir.model
        .sketch_constraints
        .sort_by_key(|constraint| constraint.id.clone());
    let act = crate::act::decode(reader, &scan)?;
    native.act_entities = act.entities;
    native.act_guids = act.guids;
    native.act_root_components = act.root_components;
    let decoded_materials = materials::decode(reader, &scan)?;
    ir.model.appearances = decoded_materials.appearances;
    ir.model.appearance_bindings = decoded_materials.bindings;
    native.store(ir.native.namespace_mut("f3d"))?;
    populate_annotations(&mut ir, &scan, &native, None);
    preserve_source_image(&scan, &mut ir)?;
    let mut report = build_container_report(&scan, false);
    report_unresolved_dimension_companions(&mut report, &native);
    Ok(DecodeResult::new(ir, report))
}

fn preserve_source_image(scan: &ContainerScan, ir: &mut CadIr) -> Result<(), CodecError> {
    let id = "f3d:file:source-image#0";
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
        .filter(|record| record.id.0 != "f3d:file:source-image#0")
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
    brep: Option<(&str, &[brep::AnnotationRecord])>,
) {
    let mut annotations = AnnotationBuilder::new();
    if let Some((stream_name, records)) = brep {
        let stream = annotations.stream(format!("f3d:{stream_name}"));
        for record in records {
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
            note(&crate::design::neutral_sketch_id(entity).0, "sketch");
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
                    &crate::design::neutral_sketch_constraint_id(&entity.id, entity.record_index).0,
                    "sketch_constraint",
                );
            }
        }
        for entity in &native.sketch_points {
            note(&entity.id, "sketch_point");
            if ir
                .model
                .sketch_entities
                .iter()
                .any(|projected| projected.native_ref.as_deref() == Some(entity.id.as_str()))
            {
                note(
                    &crate::design::neutral_sketch_point_id(&entity.id, entity.persistent_id).0,
                    "sketch_entity",
                );
            }
        }
        for entity in &native.sketch_curve_identities {
            note(&entity.id, "sketch_curve");
            if ir
                .model
                .sketch_entities
                .iter()
                .any(|projected| projected.native_ref.as_deref() == Some(entity.id.as_str()))
            {
                note(
                    &crate::design::neutral_sketch_curve_id(
                        &entity.id,
                        entity.primary_id,
                        entity.secondary_id,
                    )
                    .0,
                    "sketch_entity",
                );
            }
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
        .map(|entry| annotations.stream(format!("f3d:{}", entry.name)));
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
            let stream = annotations.stream(format!("f3d:{}", active.name));
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
            let scope = crate::design::native_stream(&relation.id)
                .unwrap_or("f3d:design")
                .to_owned();
            relation
                .members
                .iter()
                .chain(&relation.return_members)
                .map(move |record_index| (scope.clone(), *record_index))
        })
        .chain(native.design_parameters.iter().filter_map(|parameter| {
            Some((
                crate::design::native_stream(&parameter.id)?.to_owned(),
                parameter.owner_record_index?,
            ))
        }))
        .collect::<Vec<_>>();
    let existing = native
        .design_record_headers
        .iter()
        .filter_map(|record| {
            Some((
                crate::design::native_stream(&record.id)?.to_owned(),
                record.record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    native.design_record_headers.extend(
        crate::design::decode_related_record_headers(reader, scan, &indices)?
            .into_iter()
            .filter(|record| {
                crate::design::native_stream(&record.id).is_none_or(|scope| {
                    !existing.contains(&(scope.to_owned(), record.record_index))
                })
            }),
    );
    native
        .design_record_headers
        .sort_by_key(|record| record.id.clone());
    native.design_parameter_owners = crate::design::decode_parameter_owners(
        scan,
        &native.design_parameters,
        &native.design_record_headers,
    )?;
    let indices = native
        .design_parameter_owners
        .iter()
        .flat_map(|owner| {
            let scope = crate::design::native_stream(&owner.id)
                .unwrap_or("f3d:design")
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
                crate::design::native_stream(&record.id)?.to_owned(),
                record.record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    native.design_record_headers.extend(
        crate::design::decode_related_record_headers(reader, scan, &indices)?
            .into_iter()
            .filter(|record| {
                crate::design::native_stream(&record.id).is_none_or(|scope| {
                    !existing.contains(&(scope.to_owned(), record.record_index))
                })
            }),
    );
    native
        .design_record_headers
        .sort_by_key(|record| record.id.clone());
    native.design_parameter_companions = crate::design::decode_parameter_companions(
        scan,
        &native.design_parameter_owners,
        &native.design_record_headers,
    )?;
    native.design_parameter_scopes = crate::design::decode_parameter_scopes(
        scan,
        &native.design_parameter_owners,
        &native.design_record_headers,
        &native.design_entity_headers,
    )?;
    let indices = native
        .design_parameter_scopes
        .iter()
        .flat_map(|scope| {
            let stream = crate::design::native_stream(&scope.id)
                .unwrap_or("f3d:design")
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
                crate::design::native_stream(&record.id)?.to_owned(),
                record.record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    native.design_record_headers.extend(
        crate::design::decode_related_record_headers(reader, scan, &indices)?
            .into_iter()
            .filter(|record| {
                crate::design::native_stream(&record.id).is_none_or(|stream| {
                    !existing.contains(&(stream.to_owned(), record.record_index))
                })
            }),
    );
    native
        .design_record_headers
        .sort_by_key(|record| record.id.clone());
    crate::design::bind_extrude_profiles(
        scan,
        &mut native.design_parameter_scopes,
        &native.design_record_headers,
        &native.design_entity_headers,
    )?;
    native.design_construction_operand_groups = crate::design::decode_construction_operand_groups(
        scan,
        &native.design_parameter_scopes,
        &native.design_record_headers,
    )?;
    native.design_fillet_radius_groups = crate::design::decode_fillet_radius_groups(
        &native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.design_parameter_owners,
        &native.design_parameters,
    );
    native.design_extrude_selection_groups = crate::design::decode_extrude_selection_groups(
        scan,
        &native.design_parameter_scopes,
        &native.design_record_headers,
    )?;
    let mut indices = native
        .design_extrude_selection_groups
        .iter()
        .flat_map(|group| {
            let stream = crate::design::native_stream(&group.id)
                .unwrap_or("f3d:design")
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
                let stream = crate::design::native_stream(&group.id)
                    .unwrap_or("f3d:design")
                    .to_owned();
                std::iter::once((stream, group.identity_record_index))
            }),
    );
    let existing = native
        .design_record_headers
        .iter()
        .filter_map(|record| {
            Some((
                crate::design::native_stream(&record.id)?.to_owned(),
                record.record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    native.design_record_headers.extend(
        crate::design::decode_related_record_headers(reader, scan, &indices)?
            .into_iter()
            .filter(|record| {
                crate::design::native_stream(&record.id).is_none_or(|stream| {
                    !existing.contains(&(stream.to_owned(), record.record_index))
                })
            }),
    );
    native
        .design_record_headers
        .sort_by_key(|record| record.id.clone());
    native.design_construction_operand_identities =
        crate::design::decode_construction_operand_identities(
            scan,
            &native.design_construction_operand_groups,
            &native.design_record_headers,
        )?;
    crate::design::bind_lost_edge_groups(
        &mut native.design_construction_operand_groups,
        &native.design_construction_operand_identities,
        &native.lost_edge_references,
    )?;
    let indices = native
        .design_construction_operand_identities
        .iter()
        .flat_map(|identity| {
            let stream = crate::design::native_stream(&identity.id)
                .unwrap_or("f3d:design")
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
                crate::design::native_stream(&record.id)?.to_owned(),
                record.record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    native.design_record_headers.extend(
        crate::design::decode_related_record_headers(reader, scan, &indices)?
            .into_iter()
            .filter(|record| {
                crate::design::native_stream(&record.id).is_none_or(|stream| {
                    !existing.contains(&(stream.to_owned(), record.record_index))
                })
            }),
    );
    native
        .design_record_headers
        .sort_by_key(|record| record.id.clone());
    native.design_extrude_selection_members = crate::design::decode_extrude_selection_members(
        scan,
        &native.design_extrude_selection_groups,
        &native.design_record_headers,
    )?;
    crate::design::bind_extrude_selection_identities(
        &mut native.design_extrude_selection_members,
        &native.design_construction_operand_identities,
    );
    crate::history::bind_extrude_selection_history(
        &mut native.design_extrude_selection_members,
        &native.asm_histories,
    );
    native.design_edge_operands = crate::design::decode_edge_operands(
        scan,
        &native.design_parameter_scopes,
        &native.design_record_headers,
        &native.construction_recipes,
    )?;
    crate::design::bind_edge_operand_candidates(
        &mut native.design_edge_operands,
        &native.construction_recipes,
        &native.persistent_subentity_tags,
    );
    crate::history::bind_edge_operand_history_candidates(
        &mut native.design_edge_operands,
        &native.design_parameter_scopes,
        &native.asm_histories,
    );
    native.design_face_operands = crate::design::decode_face_operands(
        scan,
        &native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.design_record_headers,
        &native.construction_recipes,
    )?;
    crate::design::bind_face_operand_candidates(
        &mut native.design_face_operands,
        &native.construction_recipes,
        &native.persistent_subentity_tags,
    );
    crate::history::bind_face_operand_history_candidates(
        &mut native.design_face_operands,
        &native.design_parameter_scopes,
        &native.asm_histories,
    );
    native.design_sketch_placements =
        crate::design::decode_sketch_placements(scan, &native.design_parameter_scopes)?;
    let stream_lengths: std::collections::HashMap<String, usize> = scan
        .entries
        .iter()
        .filter(|entry| entry.role == container::role::BULKSTREAM && entry.name.contains("Design"))
        .map(|entry| {
            scan.entry_bytes(&entry.name)
                .map(|bytes| (format!("f3d:{}", entry.name), bytes.len()))
        })
        .collect::<Result<_, _>>()?;
    crate::design::bind_parameter_companion_payloads(
        &mut native.design_parameter_companions,
        &native.design_parameters,
        &native.design_parameter_owners,
        &native.design_parameter_scopes,
        &native.design_record_headers,
        &native.construction_recipes,
        &stream_lengths,
    );
    native.design_dimension_recipe_records = crate::design::decode_dimension_recipe_records(
        scan,
        &native.design_parameters,
        &native.design_parameter_owners,
        &native.design_parameter_companions,
        &native.construction_recipes,
    )?;
    crate::design::bind_dimension_recipe_reference_candidates(
        &mut native.design_dimension_recipe_records,
        &native.persistent_subentity_tags,
    );
    crate::design::bind_dimension_recipe_edge_operands(
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
) -> Result<Option<(Brep, DecodeReport)>, CodecError> {
    let width = active.header.as_ref().map_or(0, |h| h.width);
    if width != 4 && width != 8 {
        return Ok(None);
    }

    let bytes = scan.entry_bytes(&active.name)?;
    let Some(start) = asm_header::record_stream_start(bytes) else {
        return Ok(None);
    };
    let limit = active.delta_state_offset.unwrap_or(bytes.len());

    let records = match sab::frame(bytes, start, limit, usize::from(width)) {
        Ok(r) if !r.is_empty() => r,
        _ => return Ok(None),
    };

    let decoded = brep::decode(&records, bytes, &active.name);
    if decoded.surfaces.is_empty() && decoded.points.is_empty() && decoded.faces.is_empty() {
        return Ok(None);
    }
    let report = build_geometry_report(scan, &decoded);
    Ok(Some((decoded, report)))
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
                .is_some_and(|guid| guid.starts_with(&assignment.visual_guid))
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

#[cfg(test)]
mod tests {
    use super::{
        design_projection_gaps, unresolved_dimension_companion_count, DesignProjectionGaps,
    };
    use crate::native::F3dNative;
    use crate::records::{
        DesignDimensionLocusPair, DesignDimensionRecipeRecord, DesignParameter,
        DesignParameterCompanion, DesignParameterKind, DesignParameterOwner,
    };

    #[test]
    fn design_projection_gaps_count_each_retained_selection_family() {
        use cadmpeg_ir::sketches::{
            SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchId,
        };

        let mut ir = cadmpeg_ir::document::CadIr::empty(Default::default());
        ir.model.sketch_constraints.push(SketchConstraint {
            id: SketchConstraintId("constraint".into()),
            sketch: SketchId("sketch".into()),
            definition: SketchConstraintDefinition::Native {
                native_kind: "dimension".into(),
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

        assert_eq!(
            design_projection_gaps(&ir),
            DesignProjectionGaps {
                native_features: 1,
                native_constraints: 1,
                profile_selections: 1,
                face_selections: 1,
                native_edge_selections: 1,
                unresolved_edge_selections: 1,
            }
        );
    }

    #[test]
    fn payload_bearing_dimension_companion_requires_a_typed_dimension_frame() {
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
                companion_record_index: 30,
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
    }
}
