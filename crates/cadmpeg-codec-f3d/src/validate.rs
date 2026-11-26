// SPDX-License-Identifier: Apache-2.0
//! Semantic validation of the Fusion `f3d` native namespace.
//!
//! [`validate_native_charged`] loads the `f3d` native namespace from a decoded
//! [`CadIr`] and checks the settled byte frames and cross-record relationships
//! of every Fusion Design record family: body maps and bounds, parameter
//! scopes and their feature operands, sketch geometry and relations, dimension
//! loci, persistent identity links, and the ASM history graph. It returns the
//! [`Finding`] values in a fixed emission order; callers append them to the
//! generic IR validation report.

use crate::{history, ids, native, records};
use assembly::validate_component_occurrences;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::check::{Check, Finding};
use cadmpeg_ir::report::Severity;
use construction_operands::{
    validate_construction_operand_groups, validate_construction_operand_identities,
};
use design_records::{
    validate_act, validate_body_bindings, validate_body_bounds, validate_configurations,
    validate_design_intent_failures, validate_feature_timelines, validate_mesh_features,
};
use dimensions::{
    validate_dimension_annotation_frames, validate_dimension_companion_recipes,
    validate_dimension_locus_groups, validate_dimension_locus_pairs,
    validate_dimension_null_locus_pairs, validate_dimension_presentation_frames,
    validate_dimension_recipe_records,
};
use edge_operands::{
    validate_edge_operands, validate_edge_treatment_groups, validate_edge_treatment_vertex_operands,
};
use extrude_selections::{
    validate_entity_selection_operands, validate_extrude_selection_group_members,
    validate_extrude_selection_groups, validate_extrude_selection_members,
};
use face_operands::{
    validate_face_group_member_resolution, validate_face_operands, validate_face_source_groups,
};
use fillets::{
    validate_fillet_operand_groups, validate_fillet_radius_groups, validate_fillet_sets,
};
use links::{validate_body_links, validate_history_graphs, validate_subentity_tags};
use operand_groups::{
    validate_body_recipe_operands, validate_edge_identity_operands, validate_operand_group_carriers,
};
use parameter_scopes::validate_parameter_scopes;
use parameters::{validate_parameter_companions, validate_parameter_owners, validate_parameters};
use path_features::validate_path_feature_operand_roles;
use sketches::{
    validate_entity_headers, validate_profile_curve_selection, validate_sketch_geometry_identities,
    validate_sketch_placements, validate_sketch_relation_owners, validate_sketch_relations,
};
use std::collections::{BTreeSet, HashMap, HashSet};
use xref::validate_xref_placement_sources;

mod assembly;
mod construction_operands;
mod design_records;
mod edge_operands;
mod extrude_selections;
mod face_operands;
mod fillets;
mod links;
mod operand_groups;
mod parameter_scopes;
mod parameters;
mod path_features;
mod sketches;
mod work_geometry;

mod body_ownership;
mod chamfer_sets;
mod dimensions;
mod emboss;
mod extrude_operands;
mod images;
mod path_pattern;
mod sheet_metal_settings;
mod sketch_scope;
mod xref;

const EPS_VALIDATE_VALIDATE_PARAMETER_SCOPES_E10: f64 = 1.0e-10;
const EPS_VALIDATE_VALIDATE_PARAMETER_SCOPES_E8: f64 = 1.0e-8;

/// Resolve the native design stream that owns a record `id`, defaulting to the
/// primary design stream when the id carries no stream qualifier.
fn design_stream(id: &str) -> &str {
    ids::native_stream(id).unwrap_or(ids::DEFAULT_STREAM)
}

/// Report whether a native `stream` scope contains the design `entry`, either
/// directly or below the definition key of an archive member.
fn design_stream_contains_entry(stream: &str, entry: &str) -> bool {
    ids::native_scope_matches(stream, entry)
}

/// The edge-recipe evidence that scope history binding reads from the
/// native namespace.
fn native_edge_recipe_faces(native: &native::F3dNative) -> history::edge_recipe_scope::Sources<'_> {
    history::edge_recipe_scope::Sources {
        operands: &native.design_edge_operands,
        recipes: &native.construction_recipes,
        tags: &native.persistent_subentity_tags,
    }
}

fn reload_native_arena<T: serde::de::DeserializeOwned>(
    decode: &DecodeContext<'_>,
    ir: &CadIr,
    name: &str,
) -> Result<Vec<T>, CodecError> {
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(Vec::new());
    };
    if !ir.fragment {
        return namespace
            .arena_as_for_decode(decode, name)
            .map_err(Into::into);
    }
    let mut records = Vec::new();
    for record in namespace.arena_iter_as_for_decode(decode, name) {
        match record {
            Ok(record) => records.push(record),
            Err(error) => {
                let error = CodecError::from(error);
                if matches!(error, CodecError::ResourceLimit(_)) {
                    return Err(error);
                }
            }
        }
    }
    Ok(records)
}

/// Read-only indexes over the loaded `f3d` native namespace, shared by the
/// per-family validators. Every map is derived purely from the namespace and
/// borrows it for the duration of a [`validate_native`] call.
struct Ctx<'a, 'd> {
    decode: &'a DecodeContext<'d>,
    /// The decoded document, for model-side body, face, and edge identity.
    ir: &'a CadIr,
    /// The loaded native namespace.
    native: &'a native::F3dNative,
    /// Design record headers keyed by `(stream, record_index)`.
    records_by_index: HashMap<(&'a str, u32), &'a records::decal::DesignRecordHeader>,
    /// Construction recipes keyed by recipe id.
    recipes_by_id: HashMap<&'a str, &'a records::recipes::ConstructionRecipe>,
    /// Parameters keyed by `(stream, record_index)`.
    parameters_by_index: HashMap<(&'a str, u32), &'a records::parameters::DesignParameter>,
    /// Parameter owners keyed by `(stream, record_index)`.
    owners_by_index: HashMap<(&'a str, u32), &'a records::parameters::DesignParameterOwner>,
    /// Parameter companions keyed by `(stream, record_index)`.
    companions_by_index: HashMap<(&'a str, u32), &'a records::parameters::DesignParameterCompanion>,
    /// Parameter scopes keyed by `(stream, record_index)`.
    scopes_by_index: HashMap<(&'a str, u32), &'a records::feature::scope::DesignParameterScope>,
    /// Entity headers keyed by `(stream, entity_suffix)`.
    entities_by_suffix: HashMap<(&'a str, u64), &'a records::entity_header::DesignEntityHeader>,
    /// Sketch geometry record indices keyed by `(stream, record_index)`.
    sketch_geometry_indices: HashSet<(&'a str, u32)>,
    /// Sketch placements keyed by `(stream, scope_record_index)`.
    placements_by_scope:
        HashMap<(&'a str, u32), &'a records::sketch_placement::DesignSketchPlacement>,
    /// Extrude selection groups keyed by `(stream, record_index)`.
    groups_by_index: HashMap<
        (&'a str, u32),
        &'a records::topology::extrude_selection::DesignExtrudeSelectionGroup,
    >,
    /// Construction operand groups keyed by `(stream, record_index)`.
    operand_groups_by_index: HashMap<
        (&'a str, u32),
        &'a records::topology::construction::DesignConstructionOperandGroup,
    >,
    /// Extrude selection members keyed by `(stream, group_record_index, ordinal)`.
    members_by_slot: HashMap<
        (&'a str, u32, u32),
        &'a records::topology::extrude_selection::DesignExtrudeSelectionMember,
    >,
    /// Sketch owner entity ids keyed by `(stream, suffix)`.
    sketch_owner_ids: HashMap<(&'a str, u32), &'a str>,
}

fn push_constant_finding(
    findings: &mut Vec<Finding>,
    check: Check,
    message: &'static str,
    entity: Option<String>,
) {
    findings.push(Finding {
        related: Vec::new(),
        check,
        rule: cadmpeg_ir::rule_name!("f3d/validate.native-constant"),
        severity: Severity::Error,
        message: message.into(),
        entity,
    });
}

impl<'a, 'd> Ctx<'a, 'd> {
    /// Build every shared index over `native` up front. All builds are pure and
    /// emit no findings, so their eager construction does not affect the
    /// observable finding order.
    fn new(ir: &'a CadIr, native: &'a native::F3dNative, decode: &'a DecodeContext<'d>) -> Self {
        let records_by_index = native
            .design_record_headers
            .iter()
            .map(|record| ((design_stream(&record.id), record.record_index), record))
            .collect::<std::collections::HashMap<_, _>>();
        let recipes_by_id = native
            .construction_recipes
            .iter()
            .map(|recipe| (recipe.id.as_str(), recipe))
            .collect::<std::collections::HashMap<_, _>>();
        let parameters_by_index = native
            .design_parameters
            .iter()
            .map(|parameter| {
                (
                    (design_stream(&parameter.id), parameter.record_index),
                    parameter,
                )
            })
            .collect::<std::collections::HashMap<_, _>>();
        let owners_by_index = native
            .design_parameter_owners
            .iter()
            .map(|owner| ((design_stream(owner.id()), owner.record_index()), owner))
            .collect::<std::collections::HashMap<_, _>>();
        let companions_by_index = native
            .design_parameter_companions
            .iter()
            .map(|companion| {
                (
                    (design_stream(companion.id()), companion.record_index()),
                    companion,
                )
            })
            .collect::<std::collections::HashMap<_, _>>();
        let scopes_by_index = native
            .design_parameter_scopes
            .iter()
            .map(|scope| ((design_stream(&scope.id), scope.record_index), scope))
            .collect::<std::collections::HashMap<_, _>>();
        let entities_by_suffix = native
            .design_entity_headers
            .iter()
            .map(|entity| {
                (
                    (design_stream(&entity.id), entity.entity_id.suffix()),
                    entity,
                )
            })
            .collect::<std::collections::HashMap<_, _>>();
        let sketch_geometry_indices = native
            .sketch_points
            .iter()
            .map(|point| (design_stream(&point.id), point.record_index))
            .chain(
                native
                    .sketch_curve_identities
                    .iter()
                    .map(|curve| (design_stream(&curve.id), curve.record_index)),
            )
            .collect::<std::collections::HashSet<_>>();
        let placements_by_scope = native
            .design_sketch_placements
            .iter()
            .filter_map(|placement| {
                Some((
                    (design_stream(&placement.id), placement.scope_record_index?),
                    placement,
                ))
            })
            .collect::<std::collections::HashMap<_, _>>();
        let groups_by_index = native
            .design_extrude_selection_groups
            .iter()
            .map(|group| ((design_stream(&group.id), group.record_index), group))
            .collect::<std::collections::HashMap<_, _>>();
        let operand_groups_by_index = native
            .design_construction_operand_groups
            .iter()
            .map(|group| ((design_stream(&group.id), group.record_index), group))
            .collect::<std::collections::HashMap<_, _>>();
        let members_by_slot = native
            .design_extrude_selection_members
            .iter()
            .map(|member| {
                (
                    (
                        design_stream(&member.id),
                        member.group_record_index,
                        member.group_member_ordinal,
                    ),
                    member,
                )
            })
            .collect::<std::collections::HashMap<_, _>>();
        let sketch_owner_ids = native
            .design_entity_headers
            .iter()
            .filter(|header| header.in_sketch_module())
            .filter_map(|header| {
                Some((
                    (
                        design_stream(&header.id),
                        u32::try_from(header.entity_id.suffix()).ok()?,
                    ),
                    header.entity_id.as_str(),
                ))
            })
            .collect::<std::collections::HashMap<_, _>>();
        Ctx {
            decode,
            ir,
            native,
            records_by_index,
            recipes_by_id,
            parameters_by_index,
            owners_by_index,
            companions_by_index,
            scopes_by_index,
            entities_by_suffix,
            sketch_geometry_indices,
            placements_by_scope,
            groups_by_index,
            operand_groups_by_index,
            members_by_slot,
            sketch_owner_ids,
        }
    }

    /// Whether the document holds the design-intent records and the record
    /// headers they read. A decode whose scope excludes design stops before
    /// them, and a contained design-intent failure drops them, so in either
    /// case the stored scopes and owners carry no design-intent bindings.
    fn holds_design_intent(&self) -> bool {
        self.ir.scope().design && self.native.design_intent_failures.is_empty()
    }

    /// Whether the neutral features that design-intent records project to
    /// are held: a document read from CADIR JSON does not hold them.
    fn holds_neutral_design(&self) -> bool {
        self.holds_design_intent() && self.ir.model.omitted_arenas_held()
    }
}

/// Validate the `f3d` native namespace of `ir` against its decoded IR.
pub(crate) fn validate_native(
    decode: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<Vec<Finding>, CodecError> {
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(Vec::new());
    };
    let loaded = if ir.fragment {
        native::F3dNative::load_fragment(decode, namespace)
    } else {
        native::F3dNative::load_inner(decode, namespace).map(|native| (native, Vec::new()))
    };
    let (native, skipped) = match loaded.map_err(CodecError::from) {
        Ok(native) => native,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => {
            return Ok(vec![Finding {
                rule: cadmpeg_ir::rule_name!("f3d/validate.fusion-native-namespace-does-not-match"),
                related: Vec::new(),
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion native namespace does not match the expected arena shape".into(),
                entity: None,
            }]);
        }
    };
    let mut findings = validate_loaded(decode, ir, &native)?;
    findings.extend(skipped.into_iter().map(|reason| Finding {
        rule: cadmpeg_ir::rule_name!("f3d/validate.skipped-native-arena-row"),
        check: Check::FragmentSkipped,
        severity: Severity::Info,
        entity: None,
        related: Vec::new(),
        message: format!("skipped native arena row: {reason}"),
    }));
    Ok(findings)
}

fn validate_loaded(
    decode: &DecodeContext<'_>,
    ir: &CadIr,
    native: &native::F3dNative,
) -> Result<Vec<Finding>, CodecError> {
    let ctx = Ctx::new(ir, native, decode);
    let mut findings = crate::scene::validate(&native.display_poses, &ir.model);
    let mut expected_face_operands = reload_native_arena(decode, ir, "design_face_operands")?;
    let scope_histories = history::scope_histories::bind_scope_histories(
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        native_edge_recipe_faces(native),
        &native.asm_histories,
        &mut Vec::new(),
    );
    // discarded-value: the decoder reported these losses when it bound the operands.
    let _ = history::face_operand_history::bind_face_operand_history_candidates(
        decode,
        &mut expected_face_operands,
        &native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.construction_recipes,
        history::binding_histories::BindingHistories::Shared(&native.asm_histories),
        &scope_histories,
    )?;
    let decoded_profile_face_groups = native
        .design_face_operands
        .iter()
        .filter_map(|operand| Some((design_stream(&operand.id), operand.group_record_index()?)))
        .collect::<std::collections::HashSet<_>>();
    let face_group_members = native
        .design_construction_operand_groups
        .iter()
        .filter(|group| {
            group.extrude_role().is_some_and(|role| {
                matches!(
                    role,
                    records::topology::extrude_selection::DesignExtrudeOperandRole::Faces(_)
                )
            }) || (group.extrude_role()
                == Some(records::topology::extrude_selection::DesignExtrudeOperandRole::Profile)
                && decoded_profile_face_groups
                    .contains(&(design_stream(&group.id), group.record_index)))
        })
        .flat_map(|group| {
            let native_stream = design_stream(&group.id);
            group
                .members()
                .iter()
                .map(|member| &member.value)
                .map(move |member| (native_stream, group.scope_record_index, *member))
        })
        .collect::<BTreeSet<_>>();
    validate_act(&ctx, &mut findings)?;
    validate_body_bindings(&ctx, &mut findings)?;
    validate_body_bounds(&ctx, &mut findings);
    images::validate_canvas_images(&ctx, &mut findings);
    images::validate_decal_images(&ctx, &mut findings);
    validate_mesh_features(&ctx, &mut findings);
    validate_component_occurrences(&ctx, &mut findings);
    validate_xref_placement_sources(&ctx, &mut findings)?;
    validate_configurations(&ctx, &mut findings);
    validate_design_intent_failures(&ctx, &mut findings);
    validate_feature_timelines(&ctx, &mut findings)?;
    validate_parameter_scopes(&ctx, &mut findings)?;
    emboss::validate(&ctx, &mut findings);
    sheet_metal_settings::validate(&ctx, &mut findings);
    path_pattern::validate(&ctx, &mut findings);
    validate_extrude_selection_groups(&ctx, &mut findings);
    validate_construction_operand_groups(&ctx, &mut findings);
    validate_path_feature_operand_roles(&ctx, &mut findings);
    extrude_operands::validate_extrude_parameter_operands(&ctx, &mut findings);
    let mut fillet_radius_group_records = validate_fillet_sets(&ctx, &mut findings);
    chamfer_sets::validate(&ctx, &mut findings);
    fillet_radius_group_records.extend(validate_fillet_radius_groups(&ctx, &mut findings));
    validate_fillet_operand_groups(&ctx, &mut findings, &fillet_radius_group_records);
    let operand_identity_groups = validate_construction_operand_identities(&ctx, &mut findings);
    let edge_identity_records =
        validate_edge_identity_operands(decode, &ctx, &mut findings, &expected_face_operands)?;
    let body_recipe_operand_records = validate_body_recipe_operands(decode, &ctx, &mut findings)?;
    let edge_operand_records = validate_edge_operands(decode, &ctx, &mut findings)?;
    let edge_treatment_vertex_records =
        validate_edge_treatment_vertex_operands(decode, &ctx, &mut findings)?;
    validate_operand_group_carriers(
        &ctx,
        &mut findings,
        &operand_identity_groups,
        &edge_identity_records,
        &body_recipe_operand_records,
        &edge_operand_records,
        &edge_treatment_vertex_records,
    );
    validate_extrude_selection_members(&ctx, &mut findings);
    validate_entity_selection_operands(&ctx, &mut findings);
    for selection in &native.design_profile_curve_selections {
        validate_profile_curve_selection(selection, &mut findings);
    }
    validate_extrude_selection_group_members(&ctx, &mut findings);
    validate_edge_treatment_groups(
        &ctx,
        &mut findings,
        &edge_operand_records,
        &edge_identity_records,
        &edge_treatment_vertex_records,
    );
    let face_operand_records =
        validate_face_operands(&ctx, &mut findings, &expected_face_operands)?;
    // A one-member termination group whose member is not a face operand is
    // resolved by the construction operand identity that the group owns.
    let identity_group_members = native
        .design_construction_operand_groups
        .iter()
        .filter(|group| {
            let stream = design_stream(&group.id);
            group.members().len() == 1
                && group.extrude_role().is_some_and(|role| {
                    matches!(
                        role,
                        records::topology::extrude_selection::DesignExtrudeOperandRole::Faces(_)
                    )
                })
                && native
                    .design_construction_operand_identities
                    .iter()
                    .any(|identity| {
                        design_stream(&identity.id) == stream
                            && identity.group_record_index == group.record_index
                    })
        })
        .flat_map(|group| {
            let stream = design_stream(&group.id);
            group
                .members()
                .iter()
                .map(move |member| (stream, group.scope_record_index, member.value))
        })
        .collect::<HashSet<_>>();
    validate_face_group_member_resolution(
        &mut findings,
        face_group_members,
        &face_operand_records,
        &identity_group_members,
        &native.design_entity_selection_operands,
        &native.design_parameter_scopes,
    );
    validate_face_source_groups(&ctx, &mut findings);
    validate_sketch_placements(&ctx, &mut findings);
    validate_parameter_owners(&ctx, &mut findings);
    validate_parameter_companions(&ctx, &mut findings);
    let dimension_recipe_ids = validate_dimension_recipe_records(&ctx, &mut findings);
    validate_dimension_companion_recipes(&ctx, &mut findings, &dimension_recipe_ids);
    let locus_pair_companions = validate_dimension_locus_pairs(&ctx, &mut findings);
    validate_dimension_annotation_frames(&ctx, &mut findings);
    validate_dimension_presentation_frames(&ctx, &mut findings);
    let locus_group_companions = validate_dimension_locus_groups(&ctx, &mut findings);
    validate_dimension_null_locus_pairs(
        &ctx,
        &mut findings,
        &locus_pair_companions,
        &locus_group_companions,
    );
    validate_parameters(&ctx, &mut findings);
    validate_entity_headers(&ctx, &mut findings);
    validate_sketch_relations(&ctx, &mut findings);
    validate_sketch_geometry_identities(&ctx, &mut findings);
    validate_sketch_relation_owners(&ctx, &mut findings);
    validate_body_links(&ctx, &mut findings);
    validate_subentity_tags(&ctx, &mut findings);
    validate_history_graphs(&ctx, &mut findings);
    Ok(findings)
}

#[cfg(test)]
mod tests;
