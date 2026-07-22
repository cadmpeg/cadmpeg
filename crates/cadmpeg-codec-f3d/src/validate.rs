// SPDX-License-Identifier: Apache-2.0
//! Semantic validation of the Fusion `f3d` native namespace.
//!
//! [`validate_native`] loads the `f3d` native namespace from a decoded
//! [`CadIr`] and checks the settled byte frames and cross-record relationships
//! of every Fusion Design record family: body maps and bounds, parameter
//! scopes and their feature operands, sketch geometry and relations, dimension
//! loci, persistent identity links, and the ASM history graph. It returns the
//! [`Finding`] values in a fixed emission order; callers append them to the
//! generic IR validation report.

use crate::{design, history, ids, native, records};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::{Check, Finding, Severity};

/// Resolve the native design stream that owns a record `id`, defaulting to the
/// primary design stream when the id carries no stream qualifier.
fn design_stream(id: &str) -> &str {
    ids::native_stream(id).unwrap_or(ids::DEFAULT_STREAM)
}

/// Report whether a native `stream` scope contains the design `entry`, either
/// directly or through an `f3d:xref/` qualifier.
fn design_stream_contains_entry(stream: &str, entry: &str) -> bool {
    stream == ids::native_scope(entry)
        || stream
            .strip_prefix("f3d:xref/")
            .is_some_and(|qualified| qualified.ends_with(&format!("/{entry}")))
}

/// Report whether `value` is a canonical 36-character hyphenated GUID.
fn valid_design_guid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            matches!(index, 8 | 13 | 18 | 23) && byte == b'-'
                || !matches!(index, 8 | 13 | 18 | 23) && byte.is_ascii_hexdigit()
        })
}

use std::collections::{HashMap, HashSet};

/// Read-only indexes over the loaded `f3d` native namespace, shared by the
/// per-family validators. Every map is derived purely from the namespace and
/// borrows it for the duration of a [`validate_native`] call.
struct Ctx<'a> {
    /// The decoded document, for model-side body, face, and edge identity.
    ir: &'a CadIr,
    /// The loaded native namespace.
    native: &'a native::F3dNative,
    /// Design record indices keyed by `(stream, record_index)`.
    record_indices: HashSet<(&'a str, u32)>,
    /// Design record headers keyed by `(stream, record_index)`.
    records_by_index: HashMap<(&'a str, u32), &'a records::DesignRecordHeader>,
    /// Construction recipes keyed by recipe id.
    recipes_by_id: HashMap<&'a str, &'a records::ConstructionRecipe>,
    /// Parameters keyed by `(stream, record_index)`.
    parameters_by_index: HashMap<(&'a str, u32), &'a records::DesignParameter>,
    /// Parameter owners keyed by `(stream, record_index)`.
    owners_by_index: HashMap<(&'a str, u32), &'a records::DesignParameterOwner>,
    /// Parameter companions keyed by `(stream, record_index)`.
    companions_by_index: HashMap<(&'a str, u32), &'a records::DesignParameterCompanion>,
    /// Parameter scopes keyed by `(stream, record_index)`.
    scopes_by_index: HashMap<(&'a str, u32), &'a records::DesignParameterScope>,
    /// Entity headers keyed by `(stream, entity_suffix)`.
    entities_by_suffix: HashMap<(&'a str, u64), &'a records::DesignEntityHeader>,
    /// Sketch geometry record indices keyed by `(stream, record_index)`.
    sketch_geometry_indices: HashSet<(&'a str, u32)>,
    /// Sketch placements keyed by `(stream, scope_record_index)`.
    placements_by_scope: HashMap<(&'a str, u32), &'a records::DesignSketchPlacement>,
    /// Extrude selection groups keyed by `(stream, record_index)`.
    groups_by_index: HashMap<(&'a str, u32), &'a records::DesignExtrudeSelectionGroup>,
    /// Construction operand groups keyed by `(stream, record_index)`.
    operand_groups_by_index: HashMap<(&'a str, u32), &'a records::DesignConstructionOperandGroup>,
    /// Extrude selection members keyed by `(stream, group_record_index, ordinal)`.
    members_by_slot: HashMap<(&'a str, u32, u32), &'a records::DesignExtrudeSelectionMember>,
    /// Sketch owner entity suffixes keyed by `(stream, suffix)`.
    sketch_owners: HashSet<(&'a str, u32)>,
    /// Sketch owner entity ids keyed by `(stream, suffix)`.
    sketch_owner_ids: HashMap<(&'a str, u32), &'a str>,
}

impl<'a> Ctx<'a> {
    /// Build every shared index over `native` up front. All builds are pure and
    /// emit no findings, so their eager construction does not affect the
    /// observable finding order.
    fn new(ir: &'a CadIr, native: &'a native::F3dNative) -> Self {
        let record_indices = native
            .design_record_headers
            .iter()
            .map(|record| (design_stream(&record.id), record.record_index))
            .collect::<HashSet<_>>();
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
            .map(|owner| ((design_stream(&owner.id), owner.record_index), owner))
            .collect::<std::collections::HashMap<_, _>>();
        let companions_by_index = native
            .design_parameter_companions
            .iter()
            .map(|companion| {
                (
                    (design_stream(&companion.id), companion.record_index),
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
            .map(|entity| ((design_stream(&entity.id), entity.entity_suffix), entity))
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
            .collect::<HashSet<_>>();
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
        let sketch_owners = native
            .design_entity_headers
            .iter()
            .filter(|header| header.object_kind == Some(records::DesignObjectKind::Sketch))
            .filter_map(|header| {
                Some((
                    design_stream(&header.id),
                    u32::try_from(header.entity_suffix).ok()?,
                ))
            })
            .collect::<HashSet<_>>();
        let sketch_owner_ids = native
            .design_entity_headers
            .iter()
            .filter(|header| header.object_kind == Some(records::DesignObjectKind::Sketch))
            .filter_map(|header| {
                Some((
                    (
                        design_stream(&header.id),
                        u32::try_from(header.entity_suffix).ok()?,
                    ),
                    header.entity_id.as_str(),
                ))
            })
            .collect::<std::collections::HashMap<_, _>>();
        Ctx {
            ir,
            native,
            record_indices,
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
            sketch_owners,
            sketch_owner_ids,
        }
    }
}

/// Validate Fusion native design-record relationships and exact sketch frames.
pub fn validate_native(ir: &CadIr) -> Vec<Finding> {
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Vec::new();
    };
    if namespace.version != native::F3D_NATIVE_VERSION {
        let version = namespace.version;
        return vec![Finding {
            check: Check::Version,
            severity: Severity::Error,
            message: format!("unsupported Fusion native namespace version {version}"),
            entity: None,
        }];
    }
    let Ok(native) = native::F3dNative::load(namespace) else {
        return vec![Finding {
            check: Check::NativeLinks,
            severity: Severity::Error,
            message: "Fusion native namespace does not match schema version 1".into(),
            entity: None,
        }];
    };
    let native = &native;
    let ctx = Ctx::new(ir, native);
    let mut findings = Vec::new();
    let mut expected_face_operands = native.design_face_operands.clone();
    history::bind_face_operand_history_candidates(
        &mut expected_face_operands,
        &native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.asm_histories,
    );
    let decoded_profile_face_groups = native
        .design_face_operands
        .iter()
        .filter_map(|operand| Some((design_stream(&operand.id), operand.group_record_index?)))
        .collect::<HashSet<_>>();
    let face_group_members = native
        .design_construction_operand_groups
        .iter()
        .filter(|group| {
            group.extrude_role == Some(records::DesignExtrudeOperandRole::Faces)
                || (group.extrude_role == Some(records::DesignExtrudeOperandRole::Profile)
                    && decoded_profile_face_groups
                        .contains(&(design_stream(&group.id), group.record_index)))
        })
        .flat_map(|group| {
            let native_stream = design_stream(&group.id);
            group
                .members
                .iter()
                .map(move |member| (native_stream, group.scope_record_index, *member))
        })
        .collect::<HashSet<_>>();
    validate_body_bindings(&ctx, &mut findings);
    validate_body_bounds(&ctx, &mut findings);
    validate_parameter_scopes(&ctx, &mut findings);
    validate_extrude_selection_groups(&ctx, &mut findings);
    validate_construction_operand_groups(&ctx, &mut findings);
    validate_path_feature_operand_roles(&ctx, &mut findings);
    validate_extrude_parameter_operands(&ctx, &mut findings);
    let fillet_radius_group_records = validate_fillet_radius_groups(&ctx, &mut findings);
    validate_fillet_operand_groups(&ctx, &mut findings, &fillet_radius_group_records);
    let operand_identity_groups = validate_construction_operand_identities(&ctx, &mut findings);
    let edge_identity_records =
        validate_edge_identity_operands(&ctx, &mut findings, &expected_face_operands);
    validate_operand_group_identity_chains(
        &ctx,
        &mut findings,
        &operand_identity_groups,
        &edge_identity_records,
    );
    validate_extrude_selection_members(&ctx, &mut findings);
    validate_entity_selection_operands(&ctx, &mut findings);
    validate_extrude_selection_group_members(&ctx, &mut findings);
    let edge_operand_records = validate_edge_operands(&ctx, &mut findings);
    validate_edge_treatment_groups(
        &ctx,
        &mut findings,
        &edge_operand_records,
        &edge_identity_records,
    );
    let face_operand_records = validate_face_operands(&ctx, &mut findings, &expected_face_operands);
    validate_face_group_member_resolution(&mut findings, face_group_members, &face_operand_records);
    validate_sketch_placements(&ctx, &mut findings);
    validate_parameter_owners(&ctx, &mut findings);
    validate_parameter_companions(&ctx, &mut findings);
    let dimension_recipe_ids = validate_dimension_recipe_records(&ctx, &mut findings);
    validate_dimension_companion_recipes(&ctx, &mut findings, &dimension_recipe_ids);
    let locus_pair_companions = validate_dimension_locus_pairs(&ctx, &mut findings);
    validate_dimension_annotation_frames(&ctx, &mut findings);
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
    findings
}

/// Validate the ordered Design body-map binding entries and their pair runs.
fn validate_body_bindings(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let mut binding_offsets = HashSet::new();
    let mut binding_groups =
        std::collections::HashMap::<(&str, u64), Vec<&records::DesignBodyBinding>>::new();
    for binding in &native.design_body_bindings {
        let native_stream = design_stream(&binding.id);
        let valid = design_stream_contains_entry(native_stream, &binding.stream)
            && binding.pair_count > 0
            && binding.pair_ordinal < binding.pair_count
            && binding.entity_suffix_offset == binding.asm_body_key_offset.saturating_add(8)
            && binding.blob_name.starts_with("BREP.")
            && binding.blob_name_offset > binding.entity_suffix_offset
            && binding.body.as_ref().is_none_or(|body| {
                let has_named_source = native
                    .body_native_keys
                    .iter()
                    .any(|key| key.source_brep.as_deref() == Some(binding.blob_name.as_str()));
                let source_keys = native.body_native_keys.iter().filter(|key| {
                    if has_named_source {
                        key.source_brep.as_deref() == Some(binding.blob_name.as_str())
                    } else {
                        key.source_brep.is_none()
                    }
                });
                let ordinal_mode = source_keys.clone().all(|key| key.asm_body_key.is_none());
                source_keys.filter(|key| &key.body == body).any(|key| {
                    if ordinal_mode {
                        u64::from(key.body_ordinal) == binding.asm_body_key
                    } else {
                        key.asm_body_key == Some(binding.asm_body_key)
                    }
                })
            })
            && binding_offsets.insert((native_stream, binding.asm_body_key_offset));
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design body binding has an invalid ordered map entry".into(),
                entity: Some(binding.id.clone()),
            });
        }
        binding_groups
            .entry((native_stream, binding.blob_name_offset))
            .or_default()
            .push(binding);
    }
    for bindings in binding_groups.values_mut() {
        bindings.sort_by_key(|binding| binding.pair_ordinal);
        let complete = bindings
            .first()
            .is_some_and(|first| usize::try_from(first.pair_count).ok() == Some(bindings.len()))
            && bindings.iter().enumerate().all(|(ordinal, binding)| {
                usize::try_from(binding.pair_ordinal).ok() == Some(ordinal)
                    && binding.pair_count == bindings[0].pair_count
                    && binding.blob_name == bindings[0].blob_name
                    && binding.stream == bindings[0].stream
            });
        if !complete {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design body map has an incomplete ordered pair run".into(),
                entity: bindings.first().map(|binding| binding.id.clone()),
            });
        }
    }
}

/// Validate each Design body-bounds repeated record frame.
fn validate_body_bounds(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let entity_headers_by_suffix = native
        .design_entity_headers
        .iter()
        .map(|entity| ((design_stream(&entity.id), entity.entity_suffix), entity))
        .collect::<std::collections::HashMap<_, _>>();
    let mut bounded_bodies = HashSet::new();
    for bounds in &native.design_body_bounds {
        let native_stream = design_stream(&bounds.id);
        let expected_indices = u32::try_from(bounds.entity_suffix).ok().and_then(|index| {
            Some([
                index.checked_add(1)?,
                index.checked_add(2)?,
                index.checked_add(3)?,
            ])
        });
        let corners = [
            bounds.maximum.x,
            bounds.maximum.y,
            bounds.maximum.z,
            bounds.minimum.x,
            bounds.minimum.y,
            bounds.minimum.z,
        ];
        let mut expected_bindings = native
            .design_body_bindings
            .iter()
            .filter(|binding| {
                design_stream_contains_entry(native_stream, &binding.stream)
                    && binding.entity_suffix == bounds.entity_suffix
            })
            .collect::<Vec<_>>();
        expected_bindings.sort_by_key(|binding| binding.asm_body_key_offset);
        let expected_binding_ids = expected_bindings
            .into_iter()
            .map(|binding| binding.id.as_str())
            .collect::<Vec<_>>();
        let valid = entity_headers_by_suffix
            .get(&(native_stream, bounds.entity_suffix))
            .is_some_and(|entity| {
                entity.object_kind == Some(records::DesignObjectKind::Body)
                    && entity.byte_offset == bounds.entity_byte_offset
            })
            && expected_indices == Some(bounds.record_indices)
            && bounds.record_byte_offsets[0] < bounds.record_byte_offsets[1]
            && bounds.record_byte_offsets[1] < bounds.record_byte_offsets[2]
            && bounds
                .value_byte_offsets
                .iter()
                .zip(bounds.record_byte_offsets)
                .all(|(value, record)| *value > record)
            && bounds
                .body_binding_ids
                .iter()
                .map(String::as_str)
                .eq(expected_binding_ids)
            && corners.iter().all(|value| value.is_finite())
            && bounds.maximum.x >= bounds.minimum.x
            && bounds.maximum.y >= bounds.minimum.y
            && bounds.maximum.z >= bounds.minimum.z
            && (bounds.maximum.x > bounds.minimum.x
                || bounds.maximum.y > bounds.minimum.y
                || bounds.maximum.z > bounds.minimum.z)
            && bounded_bodies.insert((native_stream, bounds.entity_suffix));
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design body bounds have an invalid repeated record frame".into(),
                entity: Some(bounds.id.clone()),
            });
        }
    }
}

/// Validate feature parameter scopes and their paired feature-operation frames.
fn validate_parameter_scopes(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let record_indices = &ctx.record_indices;
    let records_by_index = &ctx.records_by_index;
    let entities_by_suffix = &ctx.entities_by_suffix;
    let placements_by_scope = &ctx.placements_by_scope;
    let mut scope_indices = HashSet::new();
    for scope in &native.design_parameter_scopes {
        let native_stream = design_stream(&scope.id);
        let unique_index = scope_indices.insert((native_stream, scope.record_index));
        let entity_link = match (
            scope.entity_id.as_deref(),
            scope.entity_suffix,
            scope.entity_reference_offset,
        ) {
            (None, None, None) => None,
            (Some(entity_id), Some(entity_suffix), Some(offset)) => Some(
                entities_by_suffix
                    .get(&(native_stream, entity_suffix))
                    .is_some_and(|entity| {
                        entity.entity_id == entity_id
                            && offset > scope.byte_offset
                            && offset < scope.paired_byte_offset
                    }),
            ),
            _ => Some(false),
        };
        let valid_sketch_profile = |profile: &records::DesignSketchProfileOperand| {
            let header = records_by_index.get(&(native_stream, profile.record_index));
            let entity = entities_by_suffix.get(&(native_stream, profile.entity_suffix));
            usize::try_from(profile.scope_reference_ordinal)
                .ok()
                .and_then(|ordinal| scope.reference_members.get(ordinal))
                == Some(&profile.record_index)
                && header.is_some_and(|header| {
                    header.byte_offset == profile.byte_offset
                        && header.class_tag == profile.class_tag
                })
                && entity.is_some_and(|entity| {
                    entity.object_kind == Some(records::DesignObjectKind::Sketch)
                        && entity.entity_id == profile.entity_id
                })
                && valid_design_guid(&profile.asset_id)
                && profile.asset_id_offset > profile.byte_offset
                && profile.entity_reference_offset > profile.asset_id_offset
                && profile.paired_byte_offset > profile.entity_reference_offset
                && profile.paired_class_tag.len() == 3
                && profile
                    .paired_class_tag
                    .bytes()
                    .all(|byte| byte.is_ascii_digit())
        };
        let is_extrude = design::design_feature_family(&scope.kind)
            == Some(design::DesignFeatureFamily::Extrude);
        let extrude_profile_link = scope
            .extrude_profile
            .as_ref()
            .is_none_or(|profile| is_extrude && valid_sketch_profile(profile));
        let is_base_flange = scope.kind == "BaseFlange";
        let base_flange_profile_link = scope
            .base_flange_profile
            .as_ref()
            .map_or(!is_base_flange, |profile| {
                is_base_flange && valid_sketch_profile(profile)
            });
        let base_flange_link = match (&scope.base_flange_operation, scope.kind.as_str()) {
            (None, kind) => kind != "BaseFlange",
            (Some(_), kind) if kind != "BaseFlange" => false,
            (Some(operation), _) => {
                scope.reference_members
                    == [
                        operation.profile_group_record_index,
                        operation.profile_record_index,
                        operation.thickness_record_index,
                        operation.settings_record_index,
                    ]
                    && scope.base_flange_profile.as_ref().is_some_and(|profile| {
                        profile.record_index == operation.profile_record_index
                            && profile.scope_reference_ordinal == 1
                    })
                    && operation.thickness.is_finite()
                    && operation.thickness > 0.0
                    && operation.thickness_offset == scope.byte_offset.saturating_add(123)
                    && operation.thickness_offset < scope.paired_byte_offset
            }
        };
        let edge_flange_link = match (&scope.edge_flange_operation, scope.kind.as_str()) {
            (None, _) => true,
            (Some(_), kind) if kind != "EdgeFlange" => false,
            (Some(operation), _) => {
                let edge_count = operation.edge_wrapper_record_indices.len();
                edge_count > 0
                    && operation.edge_group_record_indices.len() == edge_count
                    && operation.edge_operand_record_indices.len() == edge_count
                    && operation.aggregate_operand_record_indices.len() == edge_count
                    && scope.reference_members.len() == edge_count * 4 + 4
                    && (0..edge_count).all(|ordinal| {
                        scope.reference_members[ordinal * 3]
                            == operation.edge_wrapper_record_indices[ordinal]
                            && scope.reference_members[ordinal * 3 + 1]
                                == operation.edge_group_record_indices[ordinal]
                            && scope.reference_members[ordinal * 3 + 2]
                                == operation.edge_operand_record_indices[ordinal]
                    })
                    && scope.reference_members[edge_count * 3]
                        == operation.height_owner_record_index
                    && scope.reference_members[edge_count * 3 + 1]
                        == operation.angle_owner_record_index
                    && scope.reference_members[edge_count * 3 + 2]
                        == operation.aggregate_group_record_index
                    && scope.reference_members[edge_count * 3 + 3..edge_count * 4 + 3]
                        == operation.aggregate_operand_record_indices
                    && scope.reference_members.last() == Some(&operation.settings_record_index)
                    && operation.bend_radius.is_finite()
                    && operation.bend_radius > 0.0
                    && operation.bend_radius_offset > scope.byte_offset
                    && operation.bend_radius_offset < scope.paired_byte_offset
            }
        };
        let hem_link = match (&scope.hem_operation, scope.kind.as_str()) {
            (None, _) => true,
            (Some(_), kind) if kind != "Hem" => false,
            (Some(operation), _) => {
                scope.reference_members
                    == [
                        operation.gap_owner_record_index,
                        operation.length_owner_record_index,
                        operation.edge_wrapper_record_index,
                        operation.edge_group_record_index,
                        operation.edge_operand_record_index,
                        operation.aggregate_group_record_index,
                        operation.aggregate_operand_record_index,
                        operation.settings_record_index,
                    ]
                    && operation.bend_radius.is_finite()
                    && operation.bend_radius > 0.0
                    && operation.bend_radius_offset == scope.byte_offset.saturating_add(156)
                    && operation.bend_radius_offset < scope.paired_byte_offset
            }
        };
        let copy_paste_link = match (&scope.copy_paste_bodies_operation, scope.kind.as_str()) {
            (None, kind) => kind != "CopyPasteBodies",
            (Some(_), kind) if kind != "CopyPasteBodies" => false,
            (Some(operation), _) => {
                let body_count = operation.body_operand_record_indices.len();
                let group_header =
                    records_by_index.get(&(native_stream, operation.body_group_record_index));
                let relation_header =
                    records_by_index.get(&(native_stream, operation.relation_record_index));
                body_count > 0
                    && scope.reference_members.first() == Some(&operation.body_group_record_index)
                    && scope.reference_members[1..] == operation.body_operand_record_indices
                    && operation.body_operand_record_offsets.len() == body_count
                    && operation.body_operand_record_offsets.first()
                        == Some(&operation.body_group_byte_offset.saturating_add(26))
                    && operation
                        .body_operand_record_offsets
                        .windows(2)
                        .all(|pair| pair[1] == pair[0].saturating_add(11))
                    && operation.source_body_entity_suffixes.len() == body_count
                    && operation.source_body_entity_suffix_offsets.len() == body_count
                    && operation.copied_body_entity_suffixes.len() == body_count
                    && operation.copied_body_entity_suffix_offsets.len() == body_count
                    && operation.source_body_entity_suffix_offsets.first()
                        == Some(&operation.relation_byte_offset.saturating_add(25))
                    && operation
                        .source_body_entity_suffix_offsets
                        .iter()
                        .zip(&operation.copied_body_entity_suffix_offsets)
                        .all(|(source, copied)| *copied == source.saturating_add(15))
                    && operation
                        .source_body_entity_suffix_offsets
                        .windows(2)
                        .all(|pair| pair[1] == pair[0].saturating_add(30))
                    && operation
                        .source_body_entity_suffixes
                        .iter()
                        .chain(&operation.copied_body_entity_suffixes)
                        .copied()
                        .collect::<HashSet<_>>()
                        .len()
                        == body_count.saturating_mul(2)
                    && group_header.is_some_and(|header| {
                        header.byte_offset == operation.body_group_byte_offset
                            && header.class_tag == operation.body_group_class_tag
                    })
                    && relation_header.is_some_and(|header| {
                        header.byte_offset == operation.relation_byte_offset
                            && header.class_tag == operation.relation_class_tag
                    })
                    && operation.source_body_entity_suffixes.iter().all(|suffix| {
                        native.design_body_bindings.iter().any(|binding| {
                            design_stream(&binding.id) == native_stream
                                && binding.entity_suffix == u64::from(*suffix)
                        })
                    })
                    && operation.copied_body_entity_suffixes.iter().all(|suffix| {
                        native.design_body_bindings.iter().any(|binding| {
                            design_stream(&binding.id) == native_stream
                                && binding.entity_suffix == u64::from(*suffix)
                                && binding.body.is_some()
                        })
                    })
            }
        };
        let valid = scope.class_tag.len() == 3
            && scope.class_tag.bytes().all(|byte| byte.is_ascii_digit())
            && scope.paired_class_tag.len() == 3
            && scope
                .paired_class_tag
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            && !scope.kind.is_empty()
            && match (
                is_extrude,
                scope.extrude_operation,
                scope.extrude_operation_offset,
                scope.extrude_extent,
                scope.extrude_extent_offsets,
                scope.extrude_direction_reversed,
                scope.extrude_direction_reversed_offset,
                scope.extrude_start,
                scope.extrude_start_offset,
            ) {
                (
                    true,
                    Some(_),
                    Some(operation_offset),
                    Some(_),
                    Some(extent_offsets),
                    Some(_),
                    Some(direction_reversed_offset),
                    Some(_),
                    Some(start_offset),
                ) => {
                    operation_offset > scope.byte_offset
                        && extent_offsets
                            == [
                                operation_offset.saturating_add(4),
                                operation_offset.saturating_add(8),
                            ]
                        && start_offset == operation_offset.saturating_add(14)
                        && direction_reversed_offset == operation_offset.saturating_add(12)
                        && extent_offsets[1] < scope.reference_count_offset
                }
                (true, _, _, _, _, _, _, _, _) => false,
                (false, None, None, None, None, None, None, None, None) => true,
                _ => false,
            }
            && match (scope.kind.as_str(), scope.surface_stitch_operation.as_ref()) {
                ("SurfaceStitch", Some(operation)) => {
                    operation.gap_tolerance.is_finite()
                        && operation.gap_tolerance > 0.0
                        && operation.gap_tolerance_offset > scope.paired_byte_offset
                        && scope.reference_members.len() >= 4
                        && scope.reference_members.len().is_multiple_of(2)
                        && scope.reference_members[scope.reference_members.len() - 2]
                            == operation.tolerance_record_index
                        && scope.reference_members.last() == Some(&operation.settings_record_index)
                }
                ("SurfaceStitch", None) => false,
                (_, None) => true,
                (_, Some(_)) => false,
            }
            && scope.frame_length > 89
            && scope.paired_byte_offset == scope.byte_offset.saturating_add(scope.frame_length)
            && scope.kind_offset > scope.byte_offset
            && scope.kind_offset < scope.paired_byte_offset.saturating_sub(78)
            && scope.feature_ordinal > 0
            && scope.feature_ordinal_offset
                == scope
                    .paired_byte_offset
                    .saturating_sub(if scope.kind == "CopyPasteBodies" {
                        110
                    } else {
                        78
                    })
            && scope.history_state_id_offset == scope.kind_offset.saturating_sub(8)
            && scope.previous_history_state_id_offset
                == scope.feature_ordinal_offset.saturating_add(
                    if scope.kind == "CopyPasteBodies" {
                        53
                    } else {
                        31
                    },
                )
            && scope.history_state_id.is_some() == scope.previous_history_state_id.is_some()
            && scope.reference_count_offset > scope.byte_offset
            && scope.reference_count_offset < scope.kind_offset
            && !scope.reference_members.is_empty()
            && scope.reference_members.len() == scope.reference_member_offsets.len()
            && scope.reference_member_offsets.first()
                == Some(&scope.reference_count_offset.saturating_add(5))
            && scope
                .reference_member_offsets
                .windows(2)
                .all(|offsets| offsets[1] == offsets[0].saturating_add(11))
            && scope
                .reference_member_offsets
                .last()
                .is_some_and(|offset| offset.saturating_add(18) == scope.kind_offset)
            && scope.reference_member_offsets.iter().all(|offset| {
                *offset > scope.reference_count_offset && *offset < scope.kind_offset
            })
            && scope
                .reference_members
                .iter()
                .all(|record_index| record_indices.contains(&(native_stream, *record_index)))
            && record_indices.contains(&(native_stream, scope.record_index))
            && entity_link.unwrap_or(scope.kind != "Sketch")
            && extrude_profile_link
            && base_flange_profile_link
            && base_flange_link
            && edge_flange_link
            && hem_link
            && copy_paste_link
            && (scope.kind != "Sketch"
                || placements_by_scope.contains_key(&(native_stream, scope.record_index)))
            && unique_index;
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design parameter scope has an invalid paired frame".into(),
                entity: Some(scope.id.clone()),
            });
        }
    }
}

/// Validate Extrude selection groups and their counted member frames.
fn validate_extrude_selection_groups(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let record_indices = &ctx.record_indices;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let mut group_slots = HashSet::new();
    for group in &native.design_extrude_selection_groups {
        let native_stream = design_stream(&group.id);
        let scope = scopes_by_index.get(&(native_stream, group.scope_record_index));
        let header = records_by_index.get(&(native_stream, group.record_index));
        let valid = group.class_tag.len() == 3
            && group.class_tag.bytes().all(|byte| byte.is_ascii_digit())
            && group.paired_class_tag.len() == 3
            && group
                .paired_class_tag
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            && scope.is_some_and(|scope| {
                design::design_feature_family(&scope.kind)
                    == Some(design::DesignFeatureFamily::Extrude)
                    && usize::try_from(group.scope_reference_ordinal)
                        .ok()
                        .and_then(|ordinal| scope.reference_members.get(ordinal))
                        == Some(&group.record_index)
            })
            && header.is_some_and(|header| {
                header.byte_offset == group.byte_offset && header.class_tag == group.class_tag
            })
            && group.member_count_offset == group.byte_offset.saturating_add(32)
            && !group.members.is_empty()
            && group.members.len() == group.member_offsets.len()
            && group.members.iter().copied().collect::<HashSet<_>>().len() == group.members.len()
            && group.member_offsets.first() == Some(&group.member_count_offset.saturating_add(5))
            && group
                .member_offsets
                .windows(2)
                .all(|offsets| offsets[1] == offsets[0].saturating_add(11))
            && group.opaque_index != 0
            && group.opaque_index_offset
                == group.member_count_offset.saturating_add(4).saturating_add(
                    u64::try_from(group.members.len())
                        .unwrap_or(u64::MAX)
                        .saturating_mul(11),
                )
            && group.opaque_scalar.is_finite()
            && group.opaque_scalar_offset == group.opaque_index_offset.saturating_add(4)
            && group.paired_byte_offset == group.opaque_index_offset.saturating_add(53)
            && group
                .members
                .iter()
                .all(|member| record_indices.contains(&(native_stream, *member)))
            && group_slots.insert((
                native_stream,
                group.scope_record_index,
                group.scope_reference_ordinal,
            ));
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design Extrude selection group has an invalid counted frame"
                    .into(),
                entity: Some(group.id.clone()),
            });
        }
    }
}

/// Validate construction operand groups and their role discriminators.
fn validate_construction_operand_groups(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let record_indices = &ctx.record_indices;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let mut operand_group_slots = HashSet::new();
    for group in &native.design_construction_operand_groups {
        let native_stream = design_stream(&group.id);
        let scope = scopes_by_index.get(&(native_stream, group.scope_record_index));
        let header = records_by_index.get(&(native_stream, group.record_index));
        let member_run_length = u64::try_from(group.members.len())
            .unwrap_or(u64::MAX)
            .saturating_mul(11);
        let tail_offset = group
            .member_count_offset
            .saturating_add(4)
            .saturating_add(member_run_length);
        let valid = group.class_tag.len() == 3
            && group.class_tag.bytes().all(|byte| byte.is_ascii_digit())
            && group.paired_class_tag.len() == 3
            && group
                .paired_class_tag
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            && scope.is_some_and(|scope| {
                let role_is_valid = match design::design_feature_family(&scope.kind) {
                    Some(design::DesignFeatureFamily::Extrude) => match group.extrude_role {
                        Some(records::DesignExtrudeOperandRole::Bodies) => {
                            group.role == 0x0000_0008_0000_0000 && group.extrude_face_role.is_none()
                        }
                        Some(records::DesignExtrudeOperandRole::Profile) => {
                            group.role == 0x0000_0041_0000_0000
                                && group.extrude_face_role.is_none()
                                && scope.extrude_profile.as_ref().is_none_or(|profile| {
                                    group.members.first() == Some(&profile.record_index)
                                })
                        }
                        Some(records::DesignExtrudeOperandRole::Faces) => {
                            group.role == 0x0000_0011_0000_0000 && group.extrude_face_role.is_some()
                        }
                        None => false,
                    },
                    Some(
                        design::DesignFeatureFamily::Fillet | design::DesignFeatureFamily::Chamfer,
                    ) => group.extrude_role.is_none() && group.extrude_face_role.is_none(),
                    Some(design::DesignFeatureFamily::Coil) => {
                        group.role == 0x0000_0008_0000_0000
                            && group.extrude_role.is_none()
                            && group.extrude_face_role.is_none()
                    }
                    Some(design::DesignFeatureFamily::Move) => {
                        group.role == 0x0000_0004_0000_0000
                            && group.extrude_role.is_none()
                            && group.extrude_face_role.is_none()
                    }
                    Some(design::DesignFeatureFamily::OffsetFaces) => {
                        group.role == 0x0000_0010_0000_0000
                            && group.extrude_role.is_none()
                            && group.extrude_face_role.is_none()
                    }
                    Some(design::DesignFeatureFamily::Revolve) => {
                        matches!(group.role, 0x0000_0021_0000_0000 | 0x0000_0041_0000_0000)
                            && group.extrude_role.is_none()
                            && group.extrude_face_role.is_none()
                    }
                    Some(design::DesignFeatureFamily::Shell) => {
                        group.role == 0x0000_0010_0000_0000
                            && group.extrude_role.is_none()
                            && group.extrude_face_role.is_none()
                    }
                    Some(design::DesignFeatureFamily::Thicken) => {
                        matches!(group.role, 0x0000_0005_0000_0000 | 0x0000_0012_0000_0000)
                            && group.extrude_role.is_none()
                            && group.extrude_face_role.is_none()
                    }
                    Some(design::DesignFeatureFamily::Loft) => {
                        (scope.path_feature_construction.is_none()
                            || matches!(
                                group.role,
                                0x0000_0004_0000_0000
                                    | 0x0000_0005_0000_0000
                                    | 0x0000_0041_0000_0000
                                    | 0x0000_0043_0000_0000
                                    | 0x0000_0007_0000_0000
                            ))
                            && group.extrude_role.is_none()
                            && group.extrude_face_role.is_none()
                    }
                    Some(design::DesignFeatureFamily::Sweep) => {
                        (scope.path_feature_construction.is_none()
                            || matches!(group.role, 0x0000_0005_0000_0000 | 0x0000_0041_0000_0000))
                            && group.extrude_role.is_none()
                            && group.extrude_face_role.is_none()
                    }
                    Some(design::DesignFeatureFamily::SurfacePatch) => {
                        ((scope.frame_length == 339 && group.role == 0x0000_0041_0000_0000)
                            || (scope.frame_length != 339 && group.role == 0x0000_0004_0000_0000))
                            && group.extrude_role.is_none()
                            && group.extrude_face_role.is_none()
                    }
                    Some(design::DesignFeatureFamily::BoundaryFill) => {
                        matches!(group.role, 0x0000_0004_0000_0000 | 0x0000_0005_0000_0000)
                            && group.extrude_role.is_none()
                            && group.extrude_face_role.is_none()
                    }
                    Some(design::DesignFeatureFamily::Split) => {
                        group.role == 0x0000_0004_0000_0000
                            && group.extrude_role.is_none()
                            && group.extrude_face_role.is_none()
                    }
                    Some(design::DesignFeatureFamily::Scale) => {
                        group.role == 0x0000_0004_0000_0000
                            && group.extrude_role.is_none()
                            && group.extrude_face_role.is_none()
                    }
                    Some(_) => false,
                    None if scope.kind == "RemoveBody" => {
                        group.role == 0x0000_0004_0000_0000
                            && group.extrude_role.is_none()
                            && group.extrude_face_role.is_none()
                    }
                    None if scope.kind == "SurfaceStitch" => {
                        group.role == 0x0000_0005_0000_0000
                            && group.extrude_role.is_none()
                            && group.extrude_face_role.is_none()
                    }
                    None if scope.kind == "BaseFlange" => {
                        group.role == 0x0000_0041_0000_0000
                            && group.extrude_role.is_none()
                            && group.extrude_face_role.is_none()
                            && scope
                                .base_flange_profile
                                .as_ref()
                                .is_some_and(|profile| group.members == [profile.record_index])
                    }
                    None if matches!(scope.kind.as_str(), "EdgeFlange" | "Hem") => {
                        matches!(group.role, 0x0000_0008_0000_0000 | 0x0000_0043_0000_0000)
                            && group.extrude_role.is_none()
                            && group.extrude_face_role.is_none()
                    }
                    None => false,
                };
                (design::design_feature_family(&scope.kind).is_some()
                    || matches!(
                        scope.kind.as_str(),
                        "RemoveBody" | "SurfaceStitch" | "BaseFlange" | "EdgeFlange" | "Hem"
                    ))
                    && role_is_valid
                    && usize::try_from(group.scope_reference_ordinal)
                        .ok()
                        .and_then(|ordinal| scope.reference_members.get(ordinal))
                        == Some(&group.record_index)
                    && group
                        .members
                        .iter()
                        .all(|member| scope.reference_members.contains(member))
            })
            && header.is_some_and(|header| {
                header.byte_offset == group.byte_offset && header.class_tag == group.class_tag
            })
            && group.member_count_offset
                == group.byte_offset.saturating_add(
                    if scope.is_some_and(|scope| scope.kind == "SurfaceStitch") {
                        88
                    } else {
                        21
                    },
                )
            && !group.members.is_empty()
            && group.members.len() == group.member_offsets.len()
            && group.members.iter().copied().collect::<HashSet<_>>().len() == group.members.len()
            && group.member_offsets.first() == Some(&group.member_count_offset.saturating_add(5))
            && group
                .member_offsets
                .windows(2)
                .all(|offsets| offsets[1] == offsets[0].saturating_add(11))
            && group
                .members
                .iter()
                .all(|member| record_indices.contains(&(native_stream, *member)))
            && group.identity_record_offset == tail_offset.saturating_add(7)
            && group.role_offset == tail_offset.saturating_add(17)
            && group.opaque_index != 0
            && group.opaque_index_offset == tail_offset.saturating_add(35)
            && group.opaque_scalar.is_finite()
            && group.opaque_scalar_offset == tail_offset.saturating_add(39)
            && group.paired_byte_offset == tail_offset.saturating_add(88)
            && operand_group_slots.insert((
                native_stream,
                group.scope_record_index,
                group.scope_reference_ordinal,
            ));
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design construction operand group has an invalid counted frame"
                    .into(),
                entity: Some(group.id.clone()),
            });
        }
    }
}

/// Validate path-feature operand roles against the scope construction.
fn validate_path_feature_operand_roles(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    for scope in native.design_parameter_scopes.iter().filter(|scope| {
        matches!(
            design::design_feature_family(&scope.kind),
            Some(
                design::DesignFeatureFamily::Revolve
                    | design::DesignFeatureFamily::Loft
                    | design::DesignFeatureFamily::Sweep
            )
        ) && scope.path_feature_construction.is_some()
    }) {
        let native_stream = design_stream(&scope.id);
        let groups = native
            .design_construction_operand_groups
            .iter()
            .filter(|group| {
                design_stream(&group.id) == native_stream
                    && group.scope_record_index == scope.record_index
            })
            .collect::<Vec<_>>();
        let role_count = |role| groups.iter().filter(|group| group.role == role).count();
        let singleton_role_5_ordinals = groups
            .iter()
            .enumerate()
            .filter(|(_, group)| group.role == 0x0000_0005_0000_0000 && group.members.len() == 1)
            .map(|(ordinal, _)| ordinal)
            .collect::<Vec<_>>();
        let valid = match scope.path_feature_construction.as_ref() {
            Some(records::DesignPathFeatureConstruction::Revolve { .. }) => {
                groups.len() == 2
                    && role_count(0x0000_0021_0000_0000) == 1
                    && role_count(0x0000_0041_0000_0000) == 1
            }
            Some(records::DesignPathFeatureConstruction::Loft { operation, .. }) => match operation
            {
                records::DesignExtrudeOperation::Join => {
                    groups.len() == 3
                        && role_count(0x0000_0004_0000_0000) == 1
                        && role_count(0x0000_0041_0000_0000) == 2
                }
                records::DesignExtrudeOperation::NewBody => {
                    (groups.len() >= 2
                        && (role_count(0x0000_0005_0000_0000) == groups.len()
                            || role_count(0x0000_0041_0000_0000) == groups.len()))
                        || (groups.len() >= 2
                            && role_count(0x0000_0043_0000_0000) == 2
                            && ((role_count(0x0000_0005_0000_0000) == groups.len() - 2
                                && role_count(0x0000_0007_0000_0000) == 0)
                                || (groups.len() == 3
                                    && role_count(0x0000_0005_0000_0000) == 0
                                    && role_count(0x0000_0007_0000_0000) == 1)))
                        || (role_count(0x0000_0043_0000_0000) == 1
                            && role_count(0x0000_0005_0000_0000) == groups.len() - 1
                            && (singleton_role_5_ordinals.as_slice() == [0]
                                || singleton_role_5_ordinals.as_slice() == [groups.len() - 1]))
                }
                _ => false,
            },
            Some(records::DesignPathFeatureConstruction::Sweep { .. }) => {
                groups.len() == 2
                    && role_count(0x0000_0041_0000_0000) == 1
                    && role_count(0x0000_0005_0000_0000) == 1
            }
            None => false,
        };
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design path-feature operand roles conflict with its construction"
                    .into(),
                entity: Some(scope.id.clone()),
            });
        }
    }
}

/// Validate Extrude profile, operation, start, and extent operand agreement.
fn validate_extrude_parameter_operands(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    for scope in native.design_parameter_scopes.iter().filter(|scope| {
        matches!(
            design::design_feature_family(&scope.kind),
            Some(
                design::DesignFeatureFamily::Extrude
                    | design::DesignFeatureFamily::Fillet
                    | design::DesignFeatureFamily::Chamfer
            )
        )
    }) {
        let native_stream = design_stream(&scope.id);
        if design::design_feature_family(&scope.kind) == Some(design::DesignFeatureFamily::Extrude)
        {
            let mut profile_groups =
                native
                    .design_construction_operand_groups
                    .iter()
                    .filter(|group| {
                        design_stream(&group.id) == native_stream
                            && group.scope_record_index == scope.record_index
                            && group.extrude_role
                                == Some(records::DesignExtrudeOperandRole::Profile)
                    });
            let profile_group = profile_groups.next();
            let profile_matches_operand = profile_groups.next().is_none()
                && scope.extrude_profile.as_ref().is_none_or(|profile| {
                    profile_group
                        .is_some_and(|group| group.members.first() == Some(&profile.record_index))
                });
            if !profile_matches_operand {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message:
                        "Fusion Design Extrude profile conflicts with its profile operand group"
                            .into(),
                    entity: Some(scope.id.clone()),
                });
            }
            let has_body_operands = native
                .design_construction_operand_groups
                .iter()
                .any(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                        && group.extrude_role == Some(records::DesignExtrudeOperandRole::Bodies)
                });
            let face_operand_group_count = native
                .design_construction_operand_groups
                .iter()
                .filter(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                        && group.extrude_role == Some(records::DesignExtrudeOperandRole::Faces)
                })
                .count();
            let operation_matches_operands = match scope.extrude_operation {
                Some(records::DesignExtrudeOperation::NewBody) => !has_body_operands,
                Some(
                    records::DesignExtrudeOperation::Join
                    | records::DesignExtrudeOperation::Cut
                    | records::DesignExtrudeOperation::Intersect,
                ) => has_body_operands,
                None => false,
            };
            if !operation_matches_operands {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: "Fusion Design Extrude operation conflicts with its body operands"
                        .into(),
                    entity: Some(scope.id.clone()),
                });
            }
            let parameter_kind_count = |source_kind: &str| {
                native
                    .design_parameter_owners
                    .iter()
                    .filter(|owner| {
                        design_stream(&owner.id) == native_stream
                            && owner.scope_record_index == scope.record_index
                    })
                    .filter_map(|owner| {
                        parameters_by_index.get(&(native_stream, owner.parameter_record_index))
                    })
                    .filter(|parameter| parameter.source_kind == source_kind)
                    .count()
            };
            let along_count = parameter_kind_count("AlongDistance");
            let against_count = parameter_kind_count("AgainstDistance");
            let profile_offset_count = parameter_kind_count("ProfileOffset");
            let side_one_offset_count = parameter_kind_count("Side1Offset");
            let has_fixed_extrude_parameters = scope.fixed_extrude_parameters.is_some();
            let has_one_along_carrier =
                along_count <= 1 && (along_count == 1 || has_fixed_extrude_parameters);
            let extent_matches_operands = match scope.extrude_extent {
                Some(records::DesignExtrudeExtent::OneSidedDistance) => {
                    has_one_along_carrier
                        && against_count == 0
                        && side_one_offset_count == 0
                        && scope.extrude_direction_reversed == Some(false)
                }
                Some(records::DesignExtrudeExtent::OneSidedToFace) => {
                    along_count == 0
                        && !has_fixed_extrude_parameters
                        && against_count == 0
                        && side_one_offset_count == 1
                }
                Some(records::DesignExtrudeExtent::TwoSidedDistance) => {
                    along_count == 1
                        && !has_fixed_extrude_parameters
                        && against_count == 1
                        && side_one_offset_count == 0
                        && scope.extrude_direction_reversed == Some(false)
                }
                None => false,
            };
            let start_matches_operands = match scope.extrude_start {
                Some(records::DesignExtrudeStart::ProfilePlane) => profile_offset_count == 0,
                Some(
                    records::DesignExtrudeStart::OffsetProfilePlane
                    | records::DesignExtrudeStart::FromFace,
                ) => profile_offset_count == 1,
                None => false,
            };
            let expected_face_group_count = usize::from(matches!(
                scope.extrude_extent,
                Some(records::DesignExtrudeExtent::OneSidedToFace)
            )) + usize::from(matches!(
                scope.extrude_start,
                Some(records::DesignExtrudeStart::FromFace)
            ));
            let mut face_groups = native
                .design_construction_operand_groups
                .iter()
                .filter(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                        && group.extrude_role == Some(records::DesignExtrudeOperandRole::Faces)
                })
                .collect::<Vec<_>>();
            face_groups.sort_by_key(|group| group.scope_reference_ordinal);
            let expected_face_roles = match (scope.extrude_start, scope.extrude_extent) {
                (
                    Some(records::DesignExtrudeStart::FromFace),
                    Some(records::DesignExtrudeExtent::OneSidedToFace),
                ) => vec![
                    records::DesignExtrudeFaceRole::Start,
                    records::DesignExtrudeFaceRole::Termination,
                ],
                (Some(records::DesignExtrudeStart::FromFace), _) => {
                    vec![records::DesignExtrudeFaceRole::Start]
                }
                (_, Some(records::DesignExtrudeExtent::OneSidedToFace)) => {
                    vec![records::DesignExtrudeFaceRole::Termination]
                }
                _ => Vec::new(),
            };
            if !extent_matches_operands
                || !start_matches_operands
                || face_operand_group_count != expected_face_group_count
                || face_groups
                    .iter()
                    .map(|group| group.extrude_face_role)
                    .ne(expected_face_roles.iter().copied().map(Some))
            {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: "Fusion Design Extrude start or extent conflicts with its parameters and face operands"
                        .into(),
                    entity: Some(scope.id.clone()),
                });
            }
        }
    }
}

/// Validate Fillet radius-law parameter assignments; returns the assigned groups.
fn validate_fillet_radius_groups<'a>(
    ctx: &Ctx<'a>,
    findings: &mut Vec<Finding>,
) -> HashSet<(&'a str, u32)> {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let construction_groups_by_index = native
        .design_construction_operand_groups
        .iter()
        .map(|group| ((design_stream(&group.id), group.record_index), group))
        .collect::<std::collections::HashMap<_, _>>();
    let mut fillet_radius_group_records = HashSet::new();
    let mut fillet_radius_group_slots = HashSet::new();
    for assignment in &native.design_fillet_radius_groups {
        let native_stream = design_stream(&assignment.id);
        let scope = scopes_by_index.get(&(native_stream, assignment.scope_record_index));
        let group =
            construction_groups_by_index.get(&(native_stream, assignment.group_record_index));
        let assignment_parameter = |record_index: u32| {
            let parameter = *parameters_by_index.get(&(native_stream, record_index))?;
            let owner = *owners_by_index.get(&(native_stream, parameter.owner_record_index?))?;
            (owner.scope_record_index == assignment.scope_record_index
                && owner.parameter_record_index == record_index)
                .then_some(parameter)
        };
        let tangency_weight = assignment
            .tangency_weight_parameter_record_index
            .and_then(&assignment_parameter);
        let valid = scope.is_some_and(|scope| matches!(scope.kind.as_str(), "Fillet" | "Congé"))
            && group.is_some_and(|group| {
                group.scope_record_index == assignment.scope_record_index
                    && group.members == assignment.edge_operand_record_indices
            })
            && match &assignment.law {
                records::DesignFilletRadiusLaw::Constant {
                    radius_parameter_record_index,
                } => {
                    assignment_parameter(*radius_parameter_record_index).is_some_and(|parameter| {
                        parameter.source_kind == "Radius"
                            && parameter
                                .unit
                                .as_deref()
                                .is_some_and(design::feature_project::design_length_unit)
                            && parameter.evaluated_value > 0.0
                            && parameter.evaluated_value.is_finite()
                    })
                }
                records::DesignFilletRadiusLaw::Chordal {
                    chord_length_parameter_record_index,
                } => assignment_parameter(*chord_length_parameter_record_index).is_some_and(
                    |parameter| {
                        parameter.source_kind == "ChordLen"
                            && parameter
                                .unit
                                .as_deref()
                                .is_some_and(design::feature_project::design_length_unit)
                            && parameter.evaluated_value > 0.0
                            && parameter.evaluated_value.is_finite()
                    },
                ),
                records::DesignFilletRadiusLaw::Variable {
                    start_radius_parameter_record_index,
                    end_radius_parameter_record_index,
                    middle_radius_parameter_record_indices,
                    middle_parameter_record_indices,
                } => {
                    let radius = |record_index: u32, kind: &str| {
                        assignment_parameter(record_index)
                            .filter(|parameter| {
                                parameter.source_kind == kind
                                    && parameter
                                        .unit
                                        .as_deref()
                                        .is_some_and(design::feature_project::design_length_unit)
                                    && parameter.evaluated_value.is_finite()
                                    && parameter.evaluated_value >= 0.0
                            })
                            .map(|parameter| parameter.evaluated_value)
                    };
                    let start = radius(*start_radius_parameter_record_index, "StartRadius");
                    let end = radius(*end_radius_parameter_record_index, "EndRadius");
                    let middle = middle_radius_parameter_record_indices
                        .iter()
                        .map(|record_index| radius(*record_index, "MidRadius"))
                        .collect::<Option<Vec<_>>>();
                    let positions = middle_parameter_record_indices
                        .iter()
                        .map(|record_index| {
                            assignment_parameter(*record_index)
                                .filter(|parameter| {
                                    parameter.source_kind == "MidParams"
                                        && parameter.unit.is_none()
                                        && parameter.evaluated_value.is_finite()
                                        && (0.0..1.0).contains(&parameter.evaluated_value)
                                })
                                .map(|parameter| parameter.evaluated_value)
                        })
                        .collect::<Option<Vec<_>>>();
                    start.zip(end).zip(middle).zip(positions).is_some_and(
                        |(((start, end), middle), positions)| {
                            middle.len() == positions.len()
                                && (start > 0.0 || end > 0.0 || middle.iter().any(|r| *r > 0.0))
                                && positions.windows(2).all(|pair| pair[0] < pair[1])
                        },
                    )
                }
            }
            && assignment
                .tangency_weight_parameter_record_index
                .is_none_or(|_| {
                    tangency_weight.is_some_and(|parameter| {
                        parameter.source_kind == "TangencyWeight"
                            && parameter.unit.is_none()
                            && parameter.evaluated_value.is_finite()
                    })
                })
            && fillet_radius_group_records.insert((native_stream, assignment.group_record_index))
            && fillet_radius_group_slots.insert((
                native_stream,
                assignment.scope_record_index,
                assignment.group_ordinal,
            ));
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design Fillet radius group has an invalid parameter assignment"
                    .into(),
                entity: Some(assignment.id.clone()),
            });
        }
    }
    fillet_radius_group_records
}

/// Report Fillet operand groups that carry no radius assignment.
fn validate_fillet_operand_groups<'a>(
    ctx: &Ctx<'a>,
    findings: &mut Vec<Finding>,
    fillet_radius_group_records: &HashSet<(&'a str, u32)>,
) {
    let native = ctx.native;
    let scopes_by_index = &ctx.scopes_by_index;
    for group in &native.design_construction_operand_groups {
        let native_stream = design_stream(&group.id);
        let scope = scopes_by_index.get(&(native_stream, group.scope_record_index));
        let is_fillet =
            scope.is_some_and(|scope| matches!(scope.kind.as_str(), "Fillet" | "Congé"));
        let has_fixed_assignment = scope
            .and_then(|scope| {
                scope
                    .fixed_fillet_parameters
                    .as_ref()
                    .map(|fixed| (scope, fixed))
            })
            .is_some_and(|(scope, fixed)| {
                let radius_count = fixed.radii.len();
                let intermediate_count = fixed.intermediate_parameters.len();
                let valid_law_shape = (radius_count == 1 && intermediate_count == 0)
                    || (radius_count >= 2 && radius_count == intermediate_count.saturating_add(2));
                fixed.tangency_weight.is_finite()
                    && fixed.tangency_weight > 0.0
                    && valid_law_shape
                    && fixed
                        .radii
                        .iter()
                        .all(|radius| radius.is_finite() && *radius >= 0.0)
                    && fixed.radii.iter().any(|radius| *radius > 0.0)
                    && fixed.radius_record_indexes.len() == radius_count
                    && fixed.radius_offsets.len() == radius_count
                    && fixed.intermediate_parameter_record_indexes.len() == intermediate_count
                    && fixed.intermediate_parameter_offsets.len() == intermediate_count
                    && fixed
                        .intermediate_parameters
                        .iter()
                        .all(|parameter| parameter.is_finite() && (0.0..1.0).contains(parameter))
                    && fixed
                        .intermediate_parameters
                        .windows(2)
                        .all(|parameters| parameters[0] < parameters[1])
                    && native.design_parameter_owners.iter().all(|owner| {
                        design_stream(&owner.id) != native_stream
                            || owner.scope_record_index != scope.record_index
                    })
                    && std::iter::once(fixed.tangency_weight_record_index)
                        .chain(fixed.radius_record_indexes.iter().copied())
                        .chain(fixed.intermediate_parameter_record_indexes.iter().copied())
                        .all(|record_index| {
                            scope
                                .reference_members
                                .iter()
                                .filter(|member| **member == record_index)
                                .count()
                                == 1
                        })
                    && native
                        .design_construction_operand_groups
                        .iter()
                        .filter(|candidate| {
                            design_stream(&candidate.id) == native_stream
                                && candidate.scope_record_index == scope.record_index
                        })
                        .count()
                        == 1
            });
        if is_fillet
            && !has_fixed_assignment
            && !fillet_radius_group_records.contains(&(native_stream, group.record_index))
        {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design Fillet operand group has no radius assignment".into(),
                entity: Some(group.id.clone()),
            });
        }
    }
}

/// Validate construction operand identity chains; returns identity-backed groups.
fn validate_construction_operand_identities<'a>(
    ctx: &Ctx<'a>,
    findings: &mut Vec<Finding>,
) -> HashSet<(&'a str, u32)> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let operand_groups_by_index = &ctx.operand_groups_by_index;
    let mut operand_identity_groups = HashSet::new();
    for identity in &native.design_construction_operand_identities {
        let native_stream = design_stream(&identity.id);
        let group = operand_groups_by_index.get(&(native_stream, identity.group_record_index));
        let selected_profile = group
            .and_then(|group| scopes_by_index.get(&(native_stream, group.scope_record_index)))
            .and_then(|scope| scope.extrude_profile.as_ref());
        let wrapper_shape = !identity.wrapper_record_indices.is_empty()
            && identity.wrapper_record_indices.len() == identity.wrapper_byte_offsets.len()
            && identity.wrapper_record_indices.len() == identity.wrapper_class_tags.len()
            && identity
                .wrapper_record_indices
                .iter()
                .copied()
                .collect::<HashSet<_>>()
                .len()
                == identity.wrapper_record_indices.len()
            && identity
                .wrapper_byte_offsets
                .windows(2)
                .all(|offsets| offsets[1] == offsets[0].saturating_add(24))
            && identity
                .wrapper_record_indices
                .iter()
                .zip(&identity.wrapper_byte_offsets)
                .zip(&identity.wrapper_class_tags)
                .all(|((&record_index, &byte_offset), class_tag)| {
                    class_tag.len() == 3
                        && class_tag.bytes().all(|byte| byte.is_ascii_digit())
                        && records_by_index
                            .get(&(native_stream, record_index))
                            .is_some_and(|header| {
                                header.byte_offset == byte_offset && header.class_tag == *class_tag
                            })
                });
        let following_shape = identity.following_class_tag.len() == 3
            && identity
                .following_class_tag
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            && identity
                .wrapper_byte_offsets
                .last()
                .is_some_and(|offset| identity.following_byte_offset == offset.saturating_add(24))
            && records_by_index
                .get(&(native_stream, identity.following_record_index))
                .is_some_and(|header| {
                    header.byte_offset == identity.following_byte_offset
                        && header.class_tag == identity.following_class_tag
                });
        let persistent_shape = identity
            .persistent_identity
            .as_ref()
            .is_none_or(|persistent| {
                persistent.local_id_offset == identity.following_byte_offset.saturating_add(21)
                    && persistent.asset_id_offset
                        == identity.following_byte_offset.saturating_add(33)
                    && persistent.context_id_offset > persistent.asset_id_offset
                    && valid_design_guid(&persistent.asset_id)
                    && valid_design_guid(&persistent.context_id)
                    && selected_profile
                        .is_none_or(|profile| profile.asset_id == persistent.asset_id)
                    && persistent.next_byte_offset
                        == identity.following_byte_offset.saturating_add(190)
                    && persistent.next_record_index != 0
            });
        let valid = group.is_some_and(|group| {
            identity.wrapper_record_indices.first() == Some(&group.identity_record_index)
        }) && wrapper_shape
            && following_shape
            && persistent_shape
            && operand_identity_groups.insert((native_stream, identity.group_record_index));
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design construction operand identity has an invalid nested frame"
                    .into(),
                entity: Some(identity.id.clone()),
            });
        }
    }
    operand_identity_groups
}

/// Validate edge identity operands; returns their backing record set.
fn validate_edge_identity_operands<'a>(
    ctx: &Ctx<'a>,
    findings: &mut Vec<Finding>,
    expected_face_operands: &[records::DesignFaceOperand],
) -> HashSet<(&'a str, u32)> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let operand_groups_by_index = &ctx.operand_groups_by_index;
    let mut expected_edge_identity_operands = native.design_edge_identity_operands.clone();
    history::bind_edge_identity_history(
        &mut expected_edge_identity_operands,
        &native.design_construction_operand_identities,
        &native.design_parameter_scopes,
        &native.asm_histories,
    );
    history::bind_edge_identity_bounded_face_rules(
        &mut expected_edge_identity_operands,
        expected_face_operands,
    );
    let expected_edge_identity_operands = expected_edge_identity_operands
        .iter()
        .map(|operand| (operand.id.as_str(), operand))
        .collect::<HashMap<_, _>>();
    let mut edge_identity_slots = HashSet::new();
    let mut edge_identity_records = HashSet::new();
    for operand in &native.design_edge_identity_operands {
        let native_stream = design_stream(&operand.id);
        let scope = scopes_by_index.get(&(native_stream, operand.scope_record_index));
        let group = operand_groups_by_index.get(&(native_stream, operand.group_record_index));
        let header = records_by_index.get(&(native_stream, operand.record_index));
        let valid = operand.class_tag.len() == 3
            && operand.class_tag.bytes().all(|byte| byte.is_ascii_digit())
            && scope.is_some_and(|scope| {
                matches!(
                    design::design_feature_family(&scope.kind),
                    Some(
                        design::DesignFeatureFamily::Fillet | design::DesignFeatureFamily::Chamfer
                    )
                )
            })
            && group.is_some_and(|group| {
                group.scope_record_index == operand.scope_record_index
                    && usize::try_from(operand.group_member_ordinal)
                        .ok()
                        .and_then(|ordinal| group.members.get(ordinal))
                        == Some(&operand.record_index)
            })
            && header.is_some_and(|header| {
                header.byte_offset == operand.byte_offset && header.class_tag == operand.class_tag
            })
            && operand.local_id_offset == operand.byte_offset.saturating_add(24)
            && operand.asset_id_offset == operand.byte_offset.saturating_add(42)
            && operand.context_id_offset == operand.asset_id_offset.saturating_add(76)
            && valid_design_guid(&operand.asset_id)
            && valid_design_guid(&operand.context_id)
            && expected_edge_identity_operands.get(operand.id.as_str()) == Some(&operand)
            && edge_identity_slots.insert((
                native_stream,
                operand.group_record_index,
                operand.group_member_ordinal,
            ))
            && edge_identity_records.insert((native_stream, operand.record_index));
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design edge identity operand has an invalid fixed frame".into(),
                entity: Some(operand.id.clone()),
            });
        }
    }
    edge_identity_records
}

/// Report operand groups lacking an identity chain.
fn validate_operand_group_identity_chains<'a>(
    ctx: &Ctx<'a>,
    findings: &mut Vec<Finding>,
    operand_identity_groups: &HashSet<(&'a str, u32)>,
    edge_identity_records: &HashSet<(&'a str, u32)>,
) {
    let native = ctx.native;
    for group in &native.design_construction_operand_groups {
        let native_stream = design_stream(&group.id);
        let mut identity_members = native
            .design_edge_identity_operands
            .iter()
            .filter(|operand| {
                design_stream(&operand.id) == native_stream
                    && operand.scope_record_index == group.scope_record_index
                    && operand.group_record_index == group.record_index
            })
            .collect::<Vec<_>>();
        identity_members.sort_by_key(|operand| operand.group_member_ordinal);
        let has_exact_identity_members = identity_members.len() == group.members.len()
            && identity_members
                .iter()
                .enumerate()
                .all(|(ordinal, operand)| {
                    usize::try_from(operand.group_member_ordinal) == Ok(ordinal)
                        && group.members.get(ordinal) == Some(&operand.record_index)
                        && edge_identity_records.contains(&(native_stream, operand.record_index))
                });
        if !operand_identity_groups.contains(&(native_stream, group.record_index))
            && !has_exact_identity_members
        {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design construction operand group has no identity chain".into(),
                entity: Some(group.id.clone()),
            });
        }
    }
}

/// Validate Extrude selection members against their resolved sketch geometry.
fn validate_extrude_selection_members(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let groups_by_index = &ctx.groups_by_index;
    let mut member_slots = HashSet::new();
    let mut member_records = HashSet::new();
    for member in &native.design_extrude_selection_members {
        let native_stream = design_stream(&member.id);
        let group = groups_by_index.get(&(native_stream, member.group_record_index));
        let header = records_by_index.get(&(native_stream, member.record_index));
        let selected_profile = group
            .and_then(|group| scopes_by_index.get(&(native_stream, group.scope_record_index)))
            .and_then(|scope| scope.extrude_profile.as_ref());
        let selected_sketch =
            selected_profile.and_then(|profile| u32::try_from(profile.entity_suffix).ok());
        let point_targets = native.sketch_points.iter().filter_map(|point| {
            (selected_sketch.is_some()
                && design_stream(&point.id) == native_stream
                && point.owner_reference == selected_sketch
                && point.persistent_id == member.local_id)
                .then_some(records::SketchRelationOperand::Point {
                    record_index: point.record_index,
                    persistent_id: point.persistent_id,
                })
        });
        let curve_targets = native.sketch_curve_identities.iter().filter_map(|curve| {
            (selected_sketch.is_some()
                && design_stream(&curve.id) == native_stream
                && curve.owner_reference == selected_sketch
                && (curve.primary_id == member.local_id
                    || curve.secondary_id != 0 && curve.secondary_id == member.local_id))
                .then_some(records::SketchRelationOperand::Curve {
                    record_index: curve.record_index,
                    primary_id: curve.primary_id,
                    secondary_id: curve.secondary_id,
                })
        });
        let targets = point_targets.chain(curve_targets).collect::<Vec<_>>();
        let expected_target = match targets.as_slice() {
            [target] => Some(target.clone()),
            _ => None,
        };
        let mut expected_identities = native
            .design_construction_operand_identities
            .iter()
            .filter(|identity| {
                design_stream(&identity.id) == native_stream
                    && identity.following_record_index == member.record_index
                    && identity.following_byte_offset == member.byte_offset
                    && identity
                        .persistent_identity
                        .as_ref()
                        .is_some_and(|persistent| {
                            persistent.local_id == member.local_id
                                && persistent.asset_id == member.asset_id
                                && persistent.context_id == member.context_id
                        })
            })
            .collect::<Vec<_>>();
        expected_identities.sort_by_key(|identity| identity.wrapper_byte_offsets.first().copied());
        let expected_identity_ids = expected_identities
            .into_iter()
            .map(|identity| identity.id.as_str())
            .collect::<Vec<_>>();
        let expected_history =
            history::historical_selection_identity_kind(&native.asm_histories, member.local_id);
        let history_matches = expected_history.as_ref().map(|(kind, _, _)| *kind)
            == member.historical_entity_kind
            && expected_history
                .as_ref()
                .map(|(_, entity_ref, _)| *entity_ref)
                == member.historical_entity_ref
            && expected_history
                .as_ref()
                .map(|(_, _, states)| states.as_slice())
                .unwrap_or_default()
                == member.historical_state_ids.as_slice();
        let valid = member.class_tag.len() == 3
            && member.class_tag.bytes().all(|byte| byte.is_ascii_digit())
            && group.is_some_and(|group| {
                usize::try_from(member.group_member_ordinal)
                    .ok()
                    .and_then(|ordinal| group.members.get(ordinal))
                    == Some(&member.record_index)
            })
            && header.is_some_and(|header| {
                header.byte_offset == member.byte_offset && header.class_tag == member.class_tag
            })
            && member.local_id_offset == member.byte_offset.saturating_add(21)
            && member.asset_id_offset == member.byte_offset.saturating_add(33)
            && member.context_id_offset > member.asset_id_offset
            && valid_design_guid(&member.asset_id)
            && valid_design_guid(&member.context_id)
            && selected_profile.is_none_or(|profile| profile.asset_id == member.asset_id)
            && member.resolved_geometry == expected_target
            && member
                .operand_identity_ids
                .iter()
                .map(String::as_str)
                .eq(expected_identity_ids)
            && history_matches
            && member.next_byte_offset == member.byte_offset.saturating_add(190)
            && member.next_record_index != 0
            && member_slots.insert((
                native_stream,
                member.group_record_index,
                member.group_member_ordinal,
            ))
            && member_records.insert((native_stream, member.record_index));
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design Extrude selection member has an invalid fixed frame".into(),
                entity: Some(member.id.clone()),
            });
        }
    }
}

/// Validate entity-selection operand nested frames.
fn validate_entity_selection_operands(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let operand_groups_by_index = &ctx.operand_groups_by_index;
    let mut entity_selection_slots = HashSet::new();
    for operand in &native.design_entity_selection_operands {
        let native_stream = design_stream(&operand.id);
        let group = operand_groups_by_index.get(&(native_stream, operand.group_record_index));
        let header = records_by_index.get(&(native_stream, operand.record_index));
        let valid = operand.class_tag.len() == 3
            && operand.class_tag.bytes().all(|byte| byte.is_ascii_digit())
            && group.is_some_and(|group| {
                group.scope_record_index == operand.scope_record_index
                    && usize::try_from(operand.group_member_ordinal)
                        .ok()
                        .and_then(|ordinal| group.members.get(ordinal))
                        == Some(&operand.record_index)
            })
            && header.is_some_and(|header| {
                header.byte_offset == operand.byte_offset && header.class_tag == operand.class_tag
            })
            && valid_design_guid(&operand.asset_id)
            && valid_design_guid(&operand.context_id)
            && operand.identity_record_index == operand.record_index.saturating_add(3)
            && operand.primary_identity_offset == operand.identity_record_offset.saturating_add(29)
            && operand.secondary_identity_offset
                == operand.identity_record_offset.saturating_add(37)
            && operand.next_record_index == operand.record_index.saturating_add(4)
            && operand.next_byte_offset == operand.identity_record_offset.saturating_add(45)
            && entity_selection_slots.insert((
                native_stream,
                operand.group_record_index,
                operand.group_member_ordinal,
            ));
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design entity-selection operand has an invalid nested frame"
                    .into(),
                entity: Some(operand.id.clone()),
            });
        }
    }
}

/// Report Extrude selection groups with missing or inconsistent members.
fn validate_extrude_selection_group_members(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let members_by_slot = &ctx.members_by_slot;
    for group in &native.design_extrude_selection_groups {
        let native_stream = design_stream(&group.id);
        let complete = (0..group.members.len()).all(|ordinal| {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                return false;
            };
            let Some(member) = members_by_slot.get(&(native_stream, group.record_index, ordinal))
            else {
                return false;
            };
            let next = usize::try_from(ordinal)
                .ok()
                .and_then(|ordinal| group.members.get(ordinal + 1));
            next.is_none_or(|next_record_index| {
                let next_member = members_by_slot.get(&(
                    native_stream,
                    group.record_index,
                    ordinal.saturating_add(1),
                ));
                member.next_record_index == *next_record_index
                    && next_member.is_some_and(|next_member| {
                        member.next_byte_offset == next_member.byte_offset
                    })
            })
        });
        let context_id = members_by_slot
            .get(&(native_stream, group.record_index, 0))
            .map(|member| member.context_id.as_str());
        let context_consistent = context_id.is_some_and(|context_id| {
            (0..group.members.len()).all(|ordinal| {
                u32::try_from(ordinal)
                    .ok()
                    .and_then(|ordinal| {
                        members_by_slot.get(&(native_stream, group.record_index, ordinal))
                    })
                    .is_some_and(|member| member.context_id == context_id)
            })
        });
        if !(complete && context_consistent) {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design Extrude selection group has missing members".into(),
                entity: Some(group.id.clone()),
            });
        }
    }
}

/// Validate edge operands and their recipe frames; returns their record set.
fn validate_edge_operands<'a>(
    ctx: &Ctx<'a>,
    findings: &mut Vec<Finding>,
) -> HashSet<(&'a str, u32)> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let recipes_by_id = &ctx.recipes_by_id;
    let scopes_by_index = &ctx.scopes_by_index;
    let mut edge_operand_slots = HashSet::new();
    let mut edge_operand_records = HashSet::new();
    let mut expected_edge_operands = native.design_edge_operands.clone();
    history::bind_edge_operand_history_candidates(
        &mut expected_edge_operands,
        &native.design_parameter_scopes,
        &native.asm_histories,
    );
    let expected_edge_operands = expected_edge_operands
        .iter()
        .map(|operand| (operand.id.as_str(), operand))
        .collect::<HashMap<_, _>>();
    for operand in &native.design_edge_operands {
        let native_stream = design_stream(&operand.id);
        let scope = scopes_by_index.get(&(native_stream, operand.scope_record_index));
        let header = records_by_index.get(&(native_stream, operand.record_index));
        let recipe = recipes_by_id.get(operand.recipe_id.as_str());
        let expected_faces = recipe
            .map(|recipe| i64::from(recipe.record_index))
            .filter(|value| *value >= 0)
            .map(|design_reference| {
                design::decode::operands::edge_operand_candidate_faces(
                    design_reference,
                    &native.persistent_subentity_tags,
                )
            })
            .unwrap_or_default();
        let mut expected_references = design::decode::dimension_frames::decode_recipe_references(
            &operand.recipe_prefix_bytes,
            operand.recipe_prefix_offset,
        );
        for reference in &mut expected_references {
            design::decode::dimension_frames::bind_recipe_reference_candidates(
                reference,
                &native.persistent_subentity_tags,
            );
        }
        let valid = operand.class_tag.len() == 3
            && operand.class_tag.bytes().all(|byte| byte.is_ascii_digit())
            && operand.paired_class_tag.len() == 3
            && operand
                .paired_class_tag
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            && scope.is_some_and(|scope| {
                (matches!(
                    design::design_feature_family(&scope.kind),
                    Some(
                        design::DesignFeatureFamily::Fillet
                            | design::DesignFeatureFamily::Chamfer
                            | design::DesignFeatureFamily::Revolve
                            | design::DesignFeatureFamily::Loft
                    )
                ) || matches!(scope.kind.as_str(), "EdgeFlange" | "Hem"))
                    && usize::try_from(operand.scope_reference_ordinal)
                        .ok()
                        .and_then(|ordinal| scope.reference_members.get(ordinal))
                        == Some(&operand.record_index)
            })
            && header.is_some_and(|header| {
                header.byte_offset == operand.byte_offset && header.class_tag == operand.class_tag
            })
            && operand.paired_byte_offset > operand.byte_offset
            && operand.recipe_record_index == operand.record_index.saturating_add(3)
            && operand.recipe_record_byte_offset > operand.paired_byte_offset
            && operand.next_byte_offset > operand.recipe_record_byte_offset
            && operand.recipe_prefix_offset == operand.recipe_record_byte_offset.saturating_add(11)
            && operand
                .recipe_prefix_offset
                .saturating_add(operand.recipe_prefix_bytes.len() as u64)
                == recipe.map_or(u64::MAX, |recipe| recipe.byte_offset.saturating_sub(4))
            && operand.recipe_references == expected_references
            && recipe.is_some_and(|recipe| {
                design_stream(&recipe.id) == native_stream
                    && recipe.kind == crate::records::ConstructionRecipeKind::Edge
                    && recipe.byte_offset > operand.recipe_record_byte_offset
                    && recipe.byte_offset < operand.next_byte_offset
            })
            && design::decode::operands::edge_recipe_structure(&operand.recipe_program)
                == operand.recipe_structure
            && expected_faces == operand.candidate_faces
            && expected_edge_operands.get(operand.id.as_str()) == Some(&operand)
            && edge_operand_slots.insert((
                native_stream,
                operand.scope_record_index,
                operand.scope_reference_ordinal,
            ))
            && edge_operand_records.insert((native_stream, operand.record_index));
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design edge operand has an invalid scope or recipe frame".into(),
                entity: Some(operand.id.clone()),
            });
        }
    }
    edge_operand_records
}

/// Report Fillet/Chamfer edge groups with incomplete selection operands.
fn validate_edge_treatment_groups<'a>(
    ctx: &Ctx<'a>,
    findings: &mut Vec<Finding>,
    edge_operand_records: &HashSet<(&'a str, u32)>,
    edge_identity_records: &HashSet<(&'a str, u32)>,
) {
    let native = ctx.native;
    for scope in native
        .design_parameter_scopes
        .iter()
        .filter(|scope| matches!(scope.kind.as_str(), "Fillet" | "Chamfer"))
    {
        let native_stream = design_stream(&scope.id);
        let groups = native
            .design_construction_operand_groups
            .iter()
            .filter(|group| {
                design_stream(&group.id) == native_stream
                    && group.scope_record_index == scope.record_index
            })
            .collect::<Vec<_>>();
        let complete = !groups.is_empty()
            && groups.iter().all(|group| {
                let recipe_backed = group
                    .members
                    .iter()
                    .all(|member| edge_operand_records.contains(&(native_stream, *member)));
                let identity_backed = group
                    .members
                    .iter()
                    .all(|member| edge_identity_records.contains(&(native_stream, *member)));
                recipe_backed || identity_backed
            });
        if !complete {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design edge-treatment group has incomplete selection operands"
                    .into(),
                entity: Some(scope.id.clone()),
            });
        }
    }
}

/// Validate face operands and their recipe frames; returns their record set.
fn validate_face_operands<'a>(
    ctx: &Ctx<'a>,
    findings: &mut Vec<Finding>,
    expected_face_operands: &[records::DesignFaceOperand],
) -> HashSet<(&'a str, u32, u32)> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let recipes_by_id = &ctx.recipes_by_id;
    let scopes_by_index = &ctx.scopes_by_index;
    let face_groups_by_index = native
        .design_construction_operand_groups
        .iter()
        .map(|group| ((design_stream(&group.id), group.record_index), group))
        .collect::<HashMap<_, _>>();
    let expected_face_operands = expected_face_operands
        .iter()
        .map(|operand| (operand.id.as_str(), operand))
        .collect::<HashMap<_, _>>();
    let mut face_operand_records = HashSet::new();
    for operand in &native.design_face_operands {
        let native_stream = design_stream(&operand.id);
        let scope = scopes_by_index.get(&(native_stream, operand.scope_record_index));
        let header = records_by_index.get(&(native_stream, operand.record_index));
        let recipe = recipes_by_id.get(operand.recipe_id.as_str());
        let mut expected_faces = recipe
            .map(|recipe| i64::from(recipe.record_index))
            .filter(|value| *value >= 0)
            .map(|design_reference| {
                native
                    .persistent_subentity_tags
                    .iter()
                    .filter(|tag| tag.design_references.contains(&design_reference))
                    .filter_map(|tag| match &tag.target {
                        cadmpeg_ir::attributes::AttributeTarget::Face(id) => Some(id.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        expected_faces.sort_by(|left, right| left.0.cmp(&right.0));
        expected_faces.dedup();
        let mut expected_references = design::decode::dimension_frames::decode_recipe_references(
            &operand.recipe_prefix_bytes,
            operand.recipe_prefix_offset,
        );
        for reference in &mut expected_references {
            design::decode::dimension_frames::bind_recipe_reference_candidates(
                reference,
                &native.persistent_subentity_tags,
            );
        }
        let recipe_design_reference = recipe
            .map(|recipe| i64::from(recipe.record_index))
            .filter(|value| *value >= 0);
        let referenced_faces = expected_references
            .iter()
            .filter(|reference| Some(reference.design_reference) == recipe_design_reference)
            .flat_map(|reference| &reference.candidate_faces)
            .collect::<HashSet<_>>();
        let expected_unreferenced_faces = expected_faces
            .iter()
            .filter(|face| !referenced_faces.contains(face))
            .cloned()
            .collect::<Vec<_>>();
        let mut expected_alternate_selector_faces = expected_references
            .iter()
            .filter(|reference| Some(reference.design_reference) == recipe_design_reference)
            .flat_map(|reference| &reference.alternate_selector_faces)
            .cloned()
            .collect::<Vec<_>>();
        expected_alternate_selector_faces.sort_by(|left, right| left.0.cmp(&right.0));
        expected_alternate_selector_faces.dedup();
        let expected_node_offsets = operand
            .recipe_program
            .windows(3)
            .enumerate()
            .filter(|(_, values)| *values == [-1, -1, 2])
            .map(|(index, _)| {
                operand
                    .recipe_program_offset
                    .saturating_add(u64::try_from(index).unwrap_or(u64::MAX).saturating_mul(4))
            })
            .collect::<Vec<_>>();
        let expected_nodes = expected_node_offsets
            .iter()
            .copied()
            .zip(
                expected_node_offsets
                    .iter()
                    .copied()
                    .skip(1)
                    .chain(std::iter::once(operand.next_byte_offset)),
            )
            .collect::<Vec<_>>();
        let valid_program =
            match design::decode::operands::face_recipe_program_kind(&operand.recipe_program) {
                Some(design::decode::operands::FaceRecipeProgramKind::Terminal) => {
                    operand.recipe_node_offsets.is_empty() && operand.recipe_nodes.is_empty()
                }
                Some(design::decode::operands::FaceRecipeProgramKind::Counted { .. }) => {
                    operand.recipe_node_offsets == expected_node_offsets
                        && operand.recipe_nodes.len() == expected_nodes.len()
                        && operand.recipe_nodes.iter().zip(expected_nodes).all(
                            |(node, (start, end))| {
                                node.byte_offset == start
                                    && node.end_byte_offset == end
                                    && node.program.get(0..3) == Some(&[-1, -1, 2])
                                    && node.recipe_structure
                                        == node.program.get(3..).and_then(
                                            design::decode::operands::face_recipe_structure,
                                        )
                                    && u64::try_from(node.program.len()).ok().is_some_and(|words| {
                                        start.saturating_add(words.saturating_mul(4)) == end
                                    })
                            },
                        )
                        && (if operand.recipe_nodes.is_empty() {
                            operand.recipe_node_offsets.is_empty()
                        } else {
                            operand
                                .recipe_nodes
                                .iter()
                                .flat_map(|node| node.program.iter().copied())
                                .eq(operand.recipe_program.iter().copied().skip(3))
                                && operand.recipe_node_offsets.first()
                                    == Some(&operand.recipe_program_offset.saturating_add(12))
                        })
                }
                None => false,
            };
        let expected_history = expected_face_operands.get(operand.id.as_str());
        let valid = operand.class_tag.len() == 3
            && operand.class_tag.bytes().all(|byte| byte.is_ascii_digit())
            && operand.paired_class_tag.len() == 3
            && operand
                .paired_class_tag
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            && scope.is_some_and(|scope| {
                let family = design::design_feature_family(&scope.kind);
                match (operand.group_record_index, operand.group_member_ordinal) {
                    (Some(group_record_index), Some(group_member_ordinal)) => {
                        let group = face_groups_by_index
                            .get(&(native_stream, group_record_index))
                            .copied();
                        let exact_group_member = group.is_some_and(|group| {
                            group.scope_record_index == operand.scope_record_index
                                && usize::try_from(operand.scope_reference_ordinal)
                                    .ok()
                                    .and_then(|ordinal| scope.reference_members.get(ordinal))
                                    == Some(&group_record_index)
                                && usize::try_from(group_member_ordinal)
                                    .ok()
                                    .and_then(|ordinal| group.members.get(ordinal))
                                    == Some(&operand.record_index)
                        });
                        exact_group_member
                            && match family {
                                Some(
                                    design::DesignFeatureFamily::Extrude
                                    | design::DesignFeatureFamily::OffsetFaces
                                    | design::DesignFeatureFamily::Shell
                                    | design::DesignFeatureFamily::Thicken
                                    | design::DesignFeatureFamily::Split,
                                ) => true,
                                Some(design::DesignFeatureFamily::Loft) => {
                                    group.is_some_and(|group| {
                                        matches!(
                                            group.role,
                                            0x0000_0041_0000_0000 | 0x0000_0043_0000_0000
                                        )
                                    }) && operand.recipe_kind
                                        == records::ConstructionRecipeKind::BoundedFace
                                }
                                Some(
                                    design::DesignFeatureFamily::Fillet
                                    | design::DesignFeatureFamily::Chamfer,
                                ) => {
                                    operand.recipe_kind
                                        == records::ConstructionRecipeKind::BoundedFace
                                        && native.design_edge_identity_operands.iter().any(
                                            |identity| {
                                                design_stream(&identity.id) == native_stream
                                                    && identity.scope_record_index
                                                        == operand.scope_record_index
                                                    && identity.group_record_index
                                                        == group_record_index
                                                    && identity.group_member_ordinal
                                                        == group_member_ordinal
                                                    && identity.record_index == operand.record_index
                                                    && identity.class_tag == operand.class_tag
                                                    && identity.local_id
                                                        == u64::from(operand.recipe_record_index)
                                            },
                                        )
                                }
                                _ => false,
                            }
                    }
                    (None, None) => {
                        let direct_member = usize::try_from(operand.scope_reference_ordinal)
                            .ok()
                            .and_then(|ordinal| scope.reference_members.get(ordinal))
                            == Some(&operand.record_index);
                        direct_member
                            && match family {
                                Some(
                                    design::DesignFeatureFamily::OffsetFaces
                                    | design::DesignFeatureFamily::Shell
                                    | design::DesignFeatureFamily::Thicken,
                                ) => true,
                                Some(design::DesignFeatureFamily::Split) => {
                                    operand.scope_reference_ordinal == 1
                                }
                                _ => false,
                            }
                    }
                    _ => false,
                }
            })
            && header.is_some_and(|header| {
                header.byte_offset == operand.byte_offset && header.class_tag == operand.class_tag
            })
            && operand.paired_byte_offset > operand.byte_offset
            && operand.recipe_record_index == operand.record_index.saturating_add(3)
            && operand.recipe_record_byte_offset > operand.paired_byte_offset
            && operand.next_byte_offset > operand.recipe_record_byte_offset
            && operand.recipe_prefix_offset == operand.recipe_record_byte_offset.saturating_add(11)
            && operand
                .recipe_prefix_offset
                .saturating_add(operand.recipe_prefix_bytes.len() as u64)
                == recipe.map_or(u64::MAX, |recipe| recipe.byte_offset.saturating_sub(4))
            && operand.recipe_references == expected_references
            && matches!(
                operand.recipe_kind,
                records::ConstructionRecipeKind::Face
                    | records::ConstructionRecipeKind::BoundedFace
            )
            && valid_program
            && operand.recipe_program_offset
                == recipe.map_or(u64::MAX, |recipe| {
                    recipe
                        .byte_offset
                        .saturating_add(match operand.recipe_kind {
                            records::ConstructionRecipeKind::Face => 16,
                            records::ConstructionRecipeKind::BoundedFace => 24,
                            _ => u64::MAX,
                        })
                })
            && operand.next_byte_offset
                == operand.recipe_program_offset.saturating_add(
                    u64::try_from(operand.recipe_program.len())
                        .unwrap_or(u64::MAX)
                        .saturating_mul(4),
                )
            && operand.candidate_faces == expected_faces
            && operand.unreferenced_candidate_faces == expected_unreferenced_faces
            && operand.alternate_selector_candidate_faces == expected_alternate_selector_faces
            && expected_history.is_some_and(|expected| {
                operand.preceding_candidate_faces == expected.preceding_candidate_faces
                    && operand.changed_candidate_faces == expected.changed_candidate_faces
                    && operand.historical_support_contexts == expected.historical_support_contexts
            })
            && recipe.is_some_and(|recipe| {
                design_stream(&recipe.id) == native_stream
                    && recipe.kind == operand.recipe_kind
                    && recipe.byte_offset > operand.recipe_record_byte_offset
                    && recipe.byte_offset < operand.next_byte_offset
            })
            && face_operand_records.insert((
                native_stream,
                operand.scope_record_index,
                operand.record_index,
            ));
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design face operand has an invalid scope or recipe frame".into(),
                entity: Some(operand.id.clone()),
            });
        }
    }
    face_operand_records
}

/// Report face-group members with no resolved recipe operand.
fn validate_face_group_member_resolution(
    findings: &mut Vec<Finding>,
    face_group_members: HashSet<(&str, u32, u32)>,
    face_operand_records: &HashSet<(&str, u32, u32)>,
) {
    for member in face_group_members {
        if !face_operand_records.contains(&member) {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design Extrude face group has an unresolved recipe operand".into(),
                entity: Some(format!(
                    "{}:design-face-group-member#{}:{}",
                    member.0, member.1, member.2
                )),
            });
        }
    }
}

/// Validate sketch placement frames and their scope links.
fn validate_sketch_placements(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let scopes_by_index = &ctx.scopes_by_index;
    let mut placement_records = HashSet::new();
    let mut placement_scopes = HashSet::new();
    for placement in &native.design_sketch_placements {
        let native_stream = design_stream(&placement.id);
        let unique_record = placement_records.insert((native_stream, placement.record_index));
        let unique_scope = placement
            .scope_record_index
            .is_none_or(|index| placement_scopes.insert((native_stream, index)));
        let scope = placement
            .scope_record_index
            .and_then(|index| scopes_by_index.get(&(native_stream, index)));
        let identity = placement.transform
            == [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ];
        let compact =
            placement.frame_length == 201 && placement.transform_offset.is_none() && identity;
        let explicit = placement.frame_length == 329
            && placement.transform_offset == Some(placement.byte_offset.saturating_add(55));
        let genesis_compact =
            placement.frame_length == 213 && placement.transform_offset.is_none() && identity;
        let genesis_explicit = placement.frame_length == 341
            && placement.transform_offset == Some(placement.byte_offset.saturating_add(66));
        let member_run_head = (placement.frame_length == 34
            && placement.transform_offset.is_none()
            && identity)
            || (placement.frame_length == 162
                && placement.transform_offset == Some(placement.byte_offset.saturating_add(22)));
        let frame_valid = if placement.member_run_head {
            // The paired member-run record precedes the head record; the
            // frame length covers the head record alone.
            member_run_head
                && scope.is_none_or(|scope| {
                    design::design_feature_family(&scope.kind)
                        == Some(design::DesignFeatureFamily::Sketch)
                })
        } else {
            placement.paired_byte_offset
                == placement.byte_offset.saturating_add(placement.frame_length)
                && (compact || explicit || genesis_compact || genesis_explicit)
                && scope.is_some_and(|scope| {
                    design::design_feature_family(&scope.kind)
                        == Some(design::DesignFeatureFamily::Sketch)
                        && scope.entity_id.as_deref() == Some(placement.entity_id.as_str())
                        && scope.entity_suffix == Some(placement.entity_suffix)
                })
        };
        let valid = placement.class_tag.len() == 3
            && placement
                .class_tag
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            && placement.paired_class_tag.len() == 3
            && placement
                .paired_class_tag
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            && frame_valid
            && design::decode::sketch::valid_sketch_transform(&placement.transform)
            && unique_record
            && unique_scope;
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design sketch placement has an invalid frame or scope link".into(),
                entity: Some(placement.id.clone()),
            });
        }
    }
}

/// Validate parameter owner frames and their indexed parameter links.
fn validate_parameter_owners(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let record_indices = &ctx.record_indices;
    let parameters_by_index = &ctx.parameters_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let mut owner_indices = HashSet::new();
    let mut owner_ordinals = HashSet::new();
    let mut owner_local_ordinals = HashSet::new();
    for owner in &native.design_parameter_owners {
        let native_stream = design_stream(&owner.id);
        let unique_index = owner_indices.insert((native_stream, owner.record_index));
        let unique_ordinal = owner_ordinals.insert((native_stream, owner.owned_ordinal));
        let unique_local_ordinal = owner_local_ordinals.insert((
            native_stream,
            owner.scope_record_index,
            owner.local_ordinal,
        ));
        let parameter = parameters_by_index.get(&(native_stream, owner.parameter_record_index));
        let owner_first = owner.parameter_record_index == owner.record_index.saturating_add(1)
            && owner.companion_record_index == owner.record_index.saturating_add(2);
        let parameter_first = owner.record_index == owner.parameter_record_index.saturating_add(1)
            && owner.companion_record_index == owner.record_index.saturating_add(1);
        let valid = owner.class_tag.len() == 3
            && owner.class_tag.bytes().all(|byte| byte.is_ascii_digit())
            && owner.variant <= 1
            && owner.evaluated_value.is_finite()
            && matches!(
                owner.evaluated_value_offset.checked_sub(owner.byte_offset),
                Some(40 | 41)
            )
            && (owner_first || parameter_first)
            && scopes_by_index.contains_key(&(native_stream, owner.scope_record_index))
            && record_indices.contains(&(native_stream, owner.parameter_record_index))
            && record_indices.contains(&(native_stream, owner.companion_record_index))
            && companions_by_index
                .get(&(native_stream, owner.companion_record_index))
                .is_some_and(|companion| companion.owner_record_index == owner.record_index)
            && parameter.is_some_and(|parameter| {
                parameter.owner_record_index == Some(owner.record_index)
                    && parameter.evaluated_value.to_bits() == owner.evaluated_value.to_bits()
            })
            && unique_index
            && unique_ordinal
            && unique_local_ordinal;
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design parameter owner has an invalid frame or indexed link"
                    .into(),
                entity: Some(owner.id.clone()),
            });
        }
    }
}

/// Validate parameter companion prefixes and owned recipe runs.
fn validate_parameter_companions(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let record_indices = &ctx.record_indices;
    let owners_by_index = &ctx.owners_by_index;
    let mut companion_indices = HashSet::new();
    let mut companion_owners = HashSet::new();
    for companion in &native.design_parameter_companions {
        let native_stream = design_stream(&companion.id);
        let payload_end = companion
            .payload_byte_offset
            .checked_add(companion.payload_byte_length);
        let mut expected_recipes = native
            .construction_recipes
            .iter()
            .filter(|recipe| {
                design_stream(&recipe.id) == native_stream
                    && payload_end.is_some_and(|end| {
                        recipe.byte_offset >= companion.payload_byte_offset
                            && recipe.byte_offset < end
                    })
            })
            .collect::<Vec<_>>();
        expected_recipes.sort_by_key(|recipe| recipe.byte_offset);
        let expected_recipe_ids = expected_recipes
            .into_iter()
            .map(|recipe| recipe.id.as_str())
            .collect::<Vec<_>>();
        let unique_index = companion_indices.insert((native_stream, companion.record_index));
        let unique_owner = companion_owners.insert((native_stream, companion.owner_record_index));
        let owner = owners_by_index.get(&(native_stream, companion.owner_record_index));
        let valid = companion.class_tag.len() == 3
            && companion
                .class_tag
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            && companion.timestamp_micros != 0
            && companion.timestamp_micros_offset == companion.byte_offset.saturating_add(42)
            && companion.payload_byte_offset == companion.byte_offset.saturating_add(58)
            && payload_end.is_some()
            && companion
                .owned_recipe_ids
                .iter()
                .map(String::as_str)
                .eq(expected_recipe_ids)
            && record_indices.contains(&(native_stream, companion.record_index))
            && owner.is_some_and(|owner| owner.companion_record_index == companion.record_index)
            && unique_index
            && unique_owner;
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design parameter companion has an invalid prefix or owner link"
                    .into(),
                entity: Some(companion.id.clone()),
            });
        }
    }
}

/// Validate dimension recipe records; returns the owned recipe ids.
fn validate_dimension_recipe_records<'a>(
    ctx: &Ctx<'a>,
    findings: &mut Vec<Finding>,
) -> HashSet<(&'a str, &'a str)> {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let mut dimension_recipe_ids = HashSet::new();
    for record in &native.design_dimension_recipe_records {
        let native_stream = design_stream(&record.id);
        let companion = companions_by_index.get(&(native_stream, record.companion_record_index));
        let dimension_companion = companion.is_some_and(|companion| {
            owners_by_index
                .get(&(native_stream, companion.owner_record_index))
                .and_then(|owner| {
                    parameters_by_index.get(&(native_stream, owner.parameter_record_index))
                })
                .is_some_and(|parameter| parameter.kind == records::DesignParameterKind::Dimension)
        });
        let recipe = native
            .construction_recipes
            .iter()
            .find(|recipe| recipe.id == record.recipe_id);
        let companion_order_matches = companion.is_some_and(|companion| {
            usize::try_from(record.recipe_ordinal)
                .ok()
                .and_then(|ordinal| companion.owned_recipe_ids.get(ordinal))
                == Some(&record.recipe_id)
        });
        let frame_end = record.byte_offset.checked_add(record.frame_length);
        let prefix_end = record
            .prefix_offset
            .checked_add(record.prefix_bytes.len() as u64);
        let program_end = record
            .program_offset
            .checked_add((record.program.len() as u64).saturating_mul(4));
        let mut decoded_references = design::decode::dimension_frames::decode_recipe_references(
            &record.prefix_bytes,
            record.prefix_offset,
        );
        for reference in &mut decoded_references {
            design::decode::dimension_frames::bind_recipe_reference_candidates(
                reference,
                &native.persistent_subentity_tags,
            );
        }
        let valid = record.class_tag.len() == 3
            && record.class_tag.bytes().all(|byte| byte.is_ascii_digit())
            && record.frame_length >= 11
            && !record.prefix_bytes.is_empty()
            && decoded_references == record.references
            && design::decode::dimension_frames::dimension_recipe_matching_edge_operand_ids(
                record,
                &native.design_edge_operands,
            ) == record.matching_edge_operand_ids
            && record.prefix_offset == record.byte_offset.saturating_add(11)
            && !record.program.is_empty()
            && record.program_offset >= record.byte_offset.saturating_add(11)
            && program_end == frame_end
            && dimension_companion
            && companion_order_matches
            && recipe.is_some_and(|recipe| {
                design_stream(&recipe.id) == native_stream
                    && recipe.byte_offset >= record.byte_offset.saturating_add(11)
                    && frame_end.is_some_and(|end| recipe.byte_offset < end)
                    && prefix_end == recipe.byte_offset.checked_sub(4)
                    && record.program_offset
                        == recipe.byte_offset.saturating_add(
                            design::construction_recipe_family_name_len(recipe.kind) as u64,
                        )
            })
            && dimension_recipe_ids.insert((native_stream, record.recipe_id.as_str()));
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design dimension recipe has an invalid indexed-record owner"
                    .into(),
                entity: Some(record.id.clone()),
            });
        }
    }
    dimension_recipe_ids
}

/// Report dimension companions owning an unresolved construction recipe.
fn validate_dimension_companion_recipes<'a>(
    ctx: &Ctx<'a>,
    findings: &mut Vec<Finding>,
    dimension_recipe_ids: &HashSet<(&'a str, &'a str)>,
) {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    for companion in &native.design_parameter_companions {
        let native_stream = design_stream(&companion.id);
        let dimension_companion = owners_by_index
            .get(&(native_stream, companion.owner_record_index))
            .and_then(|owner| {
                parameters_by_index.get(&(native_stream, owner.parameter_record_index))
            })
            .is_some_and(|parameter| parameter.kind == records::DesignParameterKind::Dimension);
        if dimension_companion
            && companion.owned_recipe_ids.iter().any(|recipe_id| {
                !dimension_recipe_ids.contains(&(native_stream, recipe_id.as_str()))
            })
        {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design dimension companion has an unowned construction recipe"
                    .into(),
                entity: Some(companion.id.clone()),
            });
        }
    }
}

/// Validate dimension locus pairs; returns their companion set.
fn validate_dimension_locus_pairs<'a>(
    ctx: &Ctx<'a>,
    findings: &mut Vec<Finding>,
) -> HashSet<(&'a str, u32)> {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let mut locus_pair_indices = HashSet::new();
    let mut locus_pair_companions = HashSet::new();
    for pair in &native.design_dimension_locus_pairs {
        let native_stream = design_stream(&pair.id);
        let unique_index = locus_pair_indices.insert((native_stream, pair.record_index));
        let unique_companion =
            locus_pair_companions.insert((native_stream, pair.companion_record_index));
        let companion = companions_by_index.get(&(native_stream, pair.companion_record_index));
        let companion_contains_frame = companion.is_some_and(|companion| {
            pair.byte_offset >= companion.byte_offset.saturating_add(58)
                && !native.design_parameter_owners.iter().any(|owner| {
                    design_stream(&owner.id) == native_stream
                        && owner.byte_offset > companion.byte_offset
                        && owner.byte_offset <= pair.byte_offset
                })
        });
        let dimension_companion = companion.is_some_and(|companion| {
            owners_by_index
                .get(&(native_stream, companion.owner_record_index))
                .and_then(|owner| {
                    parameters_by_index.get(&(native_stream, owner.parameter_record_index))
                })
                .is_some_and(|parameter| parameter.kind == records::DesignParameterKind::Dimension)
        });
        let governs_following_dimension =
            design::decode::dimension_frames::following_dimension_companion_record_index(
                &pair.id,
                pair.paired_byte_offset,
                &native.design_parameter_owners,
                &native.design_parameters,
            ) == Some(pair.governing_companion_record_index);
        let valid = pair.class_tag.len() == 3
            && pair.class_tag.bytes().all(|byte| byte.is_ascii_digit())
            && pair.paired_class_tag.len() == 3
            && pair
                .paired_class_tag
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            && companion_contains_frame
            && dimension_companion
            && governs_following_dimension
            && pair.frame_length > 69
            && pair.paired_byte_offset == pair.byte_offset.saturating_add(pair.frame_length)
            && pair.opaque_index_offset == pair.byte_offset.saturating_add(35)
            && pair.first_geometry_reference_offset == pair.byte_offset.saturating_add(40)
            && pair.first_role_offset == pair.byte_offset.saturating_add(50)
            && pair.second_geometry_reference_offset == pair.byte_offset.saturating_add(55)
            && pair.second_role_offset == pair.byte_offset.saturating_add(65)
            && sketch_geometry_indices.contains(&(native_stream, pair.first_geometry_record_index))
            && sketch_geometry_indices
                .contains(&(native_stream, pair.second_geometry_record_index))
            && unique_index
            && unique_companion;
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design dimension locus pair has an invalid frame or geometry link"
                    .into(),
                entity: Some(pair.id.clone()),
            });
        }
    }
    locus_pair_companions
}

/// Validate dimension annotation frames and their operand runs.
fn validate_dimension_annotation_frames(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let entities_by_suffix = &ctx.entities_by_suffix;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let mut annotation_frame_indices = HashSet::new();
    for frame in &native.design_dimension_annotation_frames {
        let native_stream = design_stream(&frame.id);
        let unique_index = annotation_frame_indices.insert((native_stream, frame.record_index));
        let governing_owner = owners_by_index
            .get(&(native_stream, frame.governing_owner_record_index))
            .copied();
        let physical_interval_valid = match frame.companion_record_index {
            Some(record_index) => companions_by_index
                .get(&(native_stream, record_index))
                .is_some_and(|companion| {
                    frame.byte_offset >= companion.byte_offset.saturating_add(58)
                        && frame.paired_byte_offset
                            < companion
                                .byte_offset
                                .saturating_add(58)
                                .saturating_add(companion.payload_byte_length)
                }),
            None => governing_owner.is_some_and(|owner| {
                scopes_by_index
                    .get(&(native_stream, owner.scope_record_index))
                    .is_some_and(|scope| frame.byte_offset >= scope.byte_offset)
                    && native
                        .design_parameter_owners
                        .iter()
                        .filter(|candidate| {
                            design_stream(&candidate.id) == native_stream
                                && candidate.scope_record_index == owner.scope_record_index
                        })
                        .filter_map(|candidate| {
                            companions_by_index
                                .get(&(native_stream, candidate.companion_record_index))
                                .map(|companion| companion.byte_offset)
                        })
                        .min()
                        .is_some_and(|end| frame.paired_byte_offset < end)
            }),
        };
        let governing_link_valid = governing_owner.is_some_and(|owner| {
            owner.companion_record_index == frame.governing_companion_record_index
                && parameters_by_index
                    .get(&(native_stream, owner.parameter_record_index))
                    .is_some_and(|parameter| {
                        parameter.kind == records::DesignParameterKind::Dimension
                    })
        });
        let operand_start = frame.byte_offset.saturating_add(24);
        let operands_valid = !frame.operands.is_empty()
            && frame.operands.iter().enumerate().all(|(ordinal, operand)| {
                let start = operand_start.saturating_add((ordinal as u64).saturating_mul(15));
                operand.geometry_reference_offset == start.saturating_add(1)
                    && operand.role_offset == start.saturating_add(11)
                    && (operand.geometry_record_index == 0
                        || sketch_geometry_indices
                            .contains(&(native_stream, operand.geometry_record_index)))
            });
        let returns_start = frame.governing_owner_reference_offset.saturating_add(15);
        let returns_valid = frame.return_members.len() == frame.return_member_offsets.len()
            && frame
                .return_members
                .iter()
                .zip(&frame.return_member_offsets)
                .enumerate()
                .all(|(ordinal, (record_index, offset))| {
                    *offset == returns_start.saturating_add((ordinal as u64).saturating_mul(11))
                        && sketch_geometry_indices.contains(&(native_stream, *record_index))
                });
        let mut operand_members = frame
            .operands
            .iter()
            .filter_map(|operand| {
                (operand.geometry_record_index != 0).then_some(operand.geometry_record_index)
            })
            .collect::<Vec<_>>();
        let mut return_members = frame.return_members.clone();
        operand_members.sort_unstable();
        return_members.sort_unstable();
        let owner_is_sketch = entities_by_suffix
            .get(&(native_stream, u64::from(frame.owner_reference)))
            .is_some_and(|entity| entity.object_kind == Some(records::DesignObjectKind::Sketch));
        let valid = frame.class_tag.len() == 3
            && frame.class_tag.bytes().all(|byte| byte.is_ascii_digit())
            && frame.paired_class_tag.len() == 3
            && frame
                .paired_class_tag
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            && unique_index
            && physical_interval_valid
            && governing_link_valid
            && operands_valid
            && frame.annotation_byte_offset
                == operand_start
                    .saturating_add((frame.operands.len() as u64).saturating_mul(15))
                    .saturating_add(57)
            && frame.governing_owner_reference_offset
                == frame
                    .annotation_byte_offset
                    .saturating_add(frame.annotation_bytes.len() as u64)
                    .saturating_add(1)
            && returns_valid
            && operand_members == return_members
            && frame.paired_byte_offset == frame.byte_offset.saturating_add(frame.frame_length)
            && frame.owner_reference_offset == frame.paired_byte_offset.saturating_add(20)
            && owner_is_sketch;
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design dimension annotation frame has invalid links or offsets"
                    .into(),
                entity: Some(frame.id.clone()),
            });
        }
    }
}

/// Validate dimension locus groups; returns their companion set.
fn validate_dimension_locus_groups<'a>(
    ctx: &Ctx<'a>,
    findings: &mut Vec<Finding>,
) -> HashSet<(&'a str, u32)> {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let entities_by_suffix = &ctx.entities_by_suffix;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let mut locus_group_indices = HashSet::new();
    let mut locus_group_companions = HashSet::new();
    for group in &native.design_dimension_locus_groups {
        let native_stream = design_stream(&group.id);
        let unique_index = locus_group_indices.insert((native_stream, group.record_index));
        locus_group_companions.insert((native_stream, group.companion_record_index));
        let companion = companions_by_index.get(&(native_stream, group.companion_record_index));
        let companion_contains_frame = companion.is_some_and(|companion| {
            group.byte_offset >= companion.byte_offset.saturating_add(58)
                && !native.design_parameter_owners.iter().any(|owner| {
                    design_stream(&owner.id) == native_stream
                        && owner.byte_offset > companion.byte_offset
                        && owner.byte_offset <= group.byte_offset
                })
        });
        let dimension_companion = companion.is_some_and(|companion| {
            owners_by_index
                .get(&(native_stream, companion.owner_record_index))
                .and_then(|owner| {
                    parameters_by_index.get(&(native_stream, owner.parameter_record_index))
                })
                .is_some_and(|parameter| parameter.kind == records::DesignParameterKind::Dimension)
        });
        let count = group.loci.len();
        let loci_start = group.byte_offset.saturating_add(24);
        let loci_offsets_valid = group.loci.iter().enumerate().all(|(ordinal, locus)| {
            let start = loci_start.saturating_add((ordinal as u64).saturating_mul(15));
            locus.geometry_reference_offset == start.saturating_add(1)
                && locus.role_offset == start.saturating_add(11)
                && sketch_geometry_indices.contains(&(native_stream, locus.geometry_record_index))
        });
        let owner_start = loci_start.saturating_add((count as u64).saturating_mul(15));
        let returns_start = owner_start.saturating_add(24);
        let returns_valid = group.return_members.len() == count
            && group.return_member_offsets.len() == count
            && group
                .return_members
                .iter()
                .zip(&group.return_member_offsets)
                .enumerate()
                .all(|(ordinal, (record_index, offset))| {
                    *offset
                        == returns_start
                            .saturating_add((ordinal as u64).saturating_mul(11))
                            .saturating_add(1)
                        && sketch_geometry_indices.contains(&(native_stream, *record_index))
                });
        let mut locus_members = group
            .loci
            .iter()
            .map(|locus| locus.geometry_record_index)
            .collect::<Vec<_>>();
        let mut return_members = group.return_members.clone();
        locus_members.sort_unstable();
        return_members.sort_unstable();
        let (expected_kinds, expected_unknown) =
            design::decode::sketch::decode_constraint_kinds(u64::from(group.state));
        let owner_is_sketch = entities_by_suffix
            .get(&(native_stream, u64::from(group.owner_reference)))
            .is_some_and(|entity| entity.object_kind == Some(records::DesignObjectKind::Sketch));
        let frame_does_not_overlap = native.design_dimension_locus_groups.iter().all(|other| {
            design_stream(&other.id) != native_stream
                || other.companion_record_index != group.companion_record_index
                || other.record_index == group.record_index
                || group.next_byte_offset <= other.byte_offset
                || other.next_byte_offset <= group.byte_offset
        });
        let valid = group.class_tag.len() == 3
            && group.class_tag.bytes().all(|byte| byte.is_ascii_digit())
            && group.next_class_tag.len() == 3
            && group
                .next_class_tag
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            && companion_contains_frame
            && dimension_companion
            && (1..=64).contains(&count)
            && loci_offsets_valid
            && group.owner_reference_offset == owner_start.saturating_add(2)
            && group.owner_role_offset == owner_start.saturating_add(12)
            && group.state_offset == owner_start.saturating_add(16)
            && owner_is_sketch
            && returns_valid
            && locus_members == return_members
            && group.constraint_kinds == expected_kinds
            && u64::from(group.unknown_constraint_bits) == expected_unknown
            && group.next_byte_offset
                == returns_start
                    .saturating_add((count as u64).saturating_mul(11))
                    .saturating_add(1)
            && group.frame_length == group.next_byte_offset.saturating_sub(group.byte_offset)
            && unique_index
            && frame_does_not_overlap;
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design dimension locus group has an invalid counted frame or geometry link"
                    .into(),
                entity: Some(group.id.clone()),
            });
        }
    }
    locus_group_companions
}

/// Validate null-locus dimension pairs against typed companions.
fn validate_dimension_null_locus_pairs<'a>(
    ctx: &Ctx<'a>,
    findings: &mut Vec<Finding>,
    locus_pair_companions: &HashSet<(&'a str, u32)>,
    locus_group_companions: &HashSet<(&'a str, u32)>,
) {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let mut null_locus_pair_indices = HashSet::new();
    let mut null_locus_pair_companions = HashSet::new();
    for pair in &native.design_dimension_null_locus_pairs {
        let native_stream = design_stream(&pair.id);
        let unique_index = null_locus_pair_indices.insert((native_stream, pair.record_index));
        let unique_companion =
            null_locus_pair_companions.insert((native_stream, pair.companion_record_index));
        let companion = companions_by_index.get(&(native_stream, pair.companion_record_index));
        let companion_contains_frame = companion.is_some_and(|companion| {
            pair.byte_offset >= companion.byte_offset.saturating_add(58)
                && !native.design_parameter_owners.iter().any(|owner| {
                    design_stream(&owner.id) == native_stream
                        && owner.byte_offset > companion.byte_offset
                        && owner.byte_offset <= pair.byte_offset
                })
        });
        let dimension_companion = companion.is_some_and(|companion| {
            owners_by_index
                .get(&(native_stream, companion.owner_record_index))
                .and_then(|owner| {
                    parameters_by_index.get(&(native_stream, owner.parameter_record_index))
                })
                .is_some_and(|parameter| parameter.kind == records::DesignParameterKind::Dimension)
        });
        let governs_following_dimension =
            design::decode::dimension_frames::following_dimension_companion_record_index(
                &pair.id,
                pair.paired_byte_offset,
                &native.design_parameter_owners,
                &native.design_parameters,
            ) == Some(pair.governing_companion_record_index);
        let companion_has_typed_frame = locus_pair_companions
            .contains(&(native_stream, pair.companion_record_index))
            || locus_group_companions.contains(&(native_stream, pair.companion_record_index));
        let valid = pair.class_tag.len() == 3
            && pair.class_tag.bytes().all(|byte| byte.is_ascii_digit())
            && pair.paired_class_tag.len() == 3
            && pair
                .paired_class_tag
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            && companion_contains_frame
            && dimension_companion
            && governs_following_dimension
            && !companion_has_typed_frame
            && pair.frame_length > 54
            && pair.paired_byte_offset == pair.byte_offset.saturating_add(pair.frame_length)
            && pair.null_reference_offset == pair.byte_offset.saturating_add(25)
            && pair.null_role_offset == pair.byte_offset.saturating_add(35)
            && pair.geometry_reference_offset == pair.byte_offset.saturating_add(40)
            && pair.geometry_role_offset == pair.byte_offset.saturating_add(50)
            && sketch_geometry_indices.contains(&(native_stream, pair.geometry_record_index))
            && unique_index
            && unique_companion;
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message:
                    "Fusion Design null-locus dimension pair has an invalid frame or geometry link"
                        .into(),
                entity: Some(pair.id.clone()),
            });
        }
    }
}

/// Validate parameter records, family discriminators, and owner shape.
fn validate_parameters(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let owners_by_index = &ctx.owners_by_index;
    let mut parameter_indices = HashSet::new();
    let mut parameter_ordinals = HashSet::new();
    for parameter in &native.design_parameters {
        let native_stream = design_stream(&parameter.id);
        let unique_index = parameter_indices.insert((native_stream, parameter.record_index));
        let unique_ordinal = parameter_ordinals.insert((native_stream, parameter.source_ordinal));
        let expected_kind = if parameter.source_kind == "User Parameter" {
            records::DesignParameterKind::User
        } else if parameter.source_kind.contains("Dimension") {
            records::DesignParameterKind::Dimension
        } else {
            records::DesignParameterKind::Feature
        };
        let owner_shape_valid = match parameter.kind {
            records::DesignParameterKind::User => parameter.owner_record_index.is_none(),
            records::DesignParameterKind::Dimension | records::DesignParameterKind::Feature => {
                parameter
                    .owner_record_index
                    .is_some_and(|owner| owners_by_index.contains_key(&(native_stream, owner)))
            }
        };
        let offsets_ordered = parameter.byte_offset < parameter.expression_offset
            && parameter.prefix_value_offset == parameter.byte_offset.saturating_add(22)
            && parameter.prefix_value_offset < parameter.expression_offset
            && parameter.expression_offset < parameter.source_kind_offset
            && parameter.source_kind_offset
                < parameter.unit_offset.unwrap_or(parameter.name_offset)
            && parameter
                .unit_offset
                .is_none_or(|offset| offset < parameter.name_offset)
            && parameter.name_offset < parameter.evaluated_value_offset;
        let valid = parameter.class_tag.len() == 3
            && parameter
                .class_tag
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            && !parameter.expression.is_empty()
            && !parameter.source_kind.is_empty()
            && !parameter.name.is_empty()
            && parameter.unit.as_ref().is_none_or(|unit| !unit.is_empty())
            && parameter.unit.is_some() == parameter.unit_offset.is_some()
            && parameter.evaluated_value.is_finite()
            && design::decode::parameters::valid_design_parameter_prefix(
                parameter.prefix_value,
                &parameter.source_kind,
            )
            && parameter.kind == expected_kind
            && owner_shape_valid
            && offsets_ordered
            && unique_index
            && unique_ordinal;
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message:
                    "Fusion Design parameter has an invalid frame, family discriminator, or owner"
                        .into(),
                entity: Some(parameter.id.clone()),
            });
        }
    }
}

/// Validate design entity reference runs and suffix uniqueness.
fn validate_entity_headers(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let record_indices = &ctx.record_indices;
    let mut entity_suffixes = HashSet::new();
    for header in &native.design_entity_headers {
        let native_stream = design_stream(&header.id);
        let count_matches = header
            .declared_reference_count
            .is_none_or(|count| count as usize == header.reference_indices.len());
        let references_resolve = header
            .reference_indices
            .iter()
            .all(|index| record_indices.contains(&(native_stream, *index)));
        if !count_matches || !references_resolve {
            findings.push(Finding {
                check: Check::ReferentialIntegrity,
                severity: Severity::Error,
                message: "Fusion design entity has an invalid reference run".into(),
                entity: Some(header.entity_id.clone()),
            });
        }
        if !entity_suffixes.insert((native_stream, header.entity_suffix)) {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design entity suffix is duplicated within its stream".into(),
                entity: Some(header.entity_id.clone()),
            });
        }
    }
}

/// Validate sketch relation owners and byte frames.
fn validate_sketch_relations(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let sketch_owners = &ctx.sketch_owners;
    let sketch_owner_ids = &ctx.sketch_owner_ids;
    for relation in &native.sketch_relations {
        let native_stream = design_stream(&relation.id);
        let (constraint_kinds, unknown_constraint_bits) =
            design::decode::sketch::decode_constraint_kinds(relation.state);
        let offsets_fit = relation
            .member_offsets
            .iter()
            .chain(&relation.auxiliary_reference_offsets)
            .chain(std::iter::once(&relation.owner_reference_offset))
            .chain(&relation.return_member_offsets)
            .all(|offset| {
                usize::try_from(*offset)
                    .ok()
                    .and_then(|offset| offset.checked_add(4))
                    .is_some_and(|end| end <= relation.raw_bytes.len())
            });
        let valid = sketch_owners.contains(&(native_stream, relation.owner_reference))
            && sketch_owner_ids
                .get(&(native_stream, relation.owner_reference))
                .copied()
                == Some(relation.owner_entity_id.as_str())
            && relation.raw_bytes.len() >= 24
            && relation.members.len() == relation.member_offsets.len()
            && relation.auxiliary_references.len() == relation.auxiliary_reference_offsets.len()
            && relation.return_members.len() == relation.return_member_offsets.len()
            && offsets_fit
            && relation.unknown_constraint_bits == unknown_constraint_bits
            && relation.constraint_kinds == constraint_kinds;
        if !valid {
            findings.push(Finding {
                check: Check::ReferentialIntegrity,
                severity: Severity::Error,
                message: "Fusion sketch relation has an invalid owner or byte frame".into(),
                entity: Some(relation.id.clone()),
            });
        }
    }
}

/// Validate sketch point, curve, and surface persistent identities.
fn validate_sketch_geometry_identities(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let mut sketch_point_identities = HashSet::new();
    let mut sketch_geometry_records = HashSet::new();
    for point in &native.sketch_points {
        if !point.coordinates.u.is_finite() || !point.coordinates.v.is_finite() {
            findings.push(Finding {
                check: Check::Bounds,
                severity: Severity::Error,
                message: "Fusion sketch point contains a non-finite coordinate".into(),
                entity: Some(point.id.clone()),
            });
        }
        if point.persistent_id == 0
            || !sketch_point_identities.insert((
                design_stream(&point.id),
                point.owner_reference,
                point.persistent_id,
            ))
        {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion sketch point has an invalid persistent identity".into(),
                entity: Some(point.id.clone()),
            });
        }
        if !sketch_geometry_records.insert((design_stream(&point.id), point.record_index)) {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion sketch geometry aliases another typed indexed record".into(),
                entity: Some(point.id.clone()),
            });
        }
    }
    let mut sketch_curve_identities = HashSet::new();
    for curve in &native.sketch_curve_identities {
        if curve.primary_id == 0
            || !sketch_curve_identities.insert((
                design_stream(&curve.id),
                curve.owner_reference,
                curve.primary_id,
                curve.secondary_id,
            ))
        {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion sketch curve has an invalid persistent identity".into(),
                entity: Some(curve.id.clone()),
            });
        }
        if !sketch_geometry_records.insert((design_stream(&curve.id), curve.record_index)) {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion sketch geometry aliases another typed indexed record".into(),
                entity: Some(curve.id.clone()),
            });
        }
    }
    let mut sketch_surface_identities = HashSet::new();
    for surface in &native.sketch_surfaces {
        if surface.persistent_id == 0
            || !sketch_surface_identities.insert((
                design_stream(&surface.id),
                surface.owner_reference,
                surface.persistent_id,
            ))
        {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion sketch surface has an invalid persistent identity".into(),
                entity: Some(surface.id.clone()),
            });
        }
        if !sketch_geometry_records.insert((design_stream(&surface.id), surface.record_index)) {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion sketch geometry aliases another typed indexed record".into(),
                entity: Some(surface.id.clone()),
            });
        }
    }
}

/// Validate the sketch ownership graph across relations, dimensions, and loci.
fn validate_sketch_relation_owners(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let placements_by_scope = &ctx.placements_by_scope;
    let sketch_owners = &ctx.sketch_owners;
    let typed_sketch_records = native
        .sketch_points
        .iter()
        .map(|point| (design_stream(&point.id), point.record_index))
        .chain(
            native
                .sketch_curve_identities
                .iter()
                .map(|curve| (design_stream(&curve.id), curve.record_index)),
        )
        .chain(
            native
                .sketch_surfaces
                .iter()
                .map(|surface| (design_stream(&surface.id), surface.record_index)),
        )
        .collect::<std::collections::HashSet<_>>();
    let sketch_operands = native
        .sketch_points
        .iter()
        .map(|point| {
            (
                (design_stream(&point.id), point.record_index),
                records::SketchRelationOperand::Point {
                    record_index: point.record_index,
                    persistent_id: point.persistent_id,
                },
            )
        })
        .chain(native.sketch_curve_identities.iter().map(|curve| {
            (
                (design_stream(&curve.id), curve.record_index),
                records::SketchRelationOperand::Curve {
                    record_index: curve.record_index,
                    primary_id: curve.primary_id,
                    secondary_id: curve.secondary_id,
                },
            )
        }))
        .chain(native.sketch_surfaces.iter().map(|surface| {
            (
                (design_stream(&surface.id), surface.record_index),
                records::SketchRelationOperand::Surface {
                    record_index: surface.record_index,
                    persistent_id: surface.persistent_id,
                },
            )
        }))
        .collect::<std::collections::HashMap<_, _>>();
    let mut relation_owners = std::collections::HashMap::new();
    for (id, record_index, owner_reference) in native
        .sketch_points
        .iter()
        .map(|point| (&point.id, point.record_index, point.owner_reference))
        .chain(
            native
                .sketch_curve_identities
                .iter()
                .map(|curve| (&curve.id, curve.record_index, curve.owner_reference)),
        )
        .chain(
            native
                .sketch_surfaces
                .iter()
                .map(|surface| (&surface.id, surface.record_index, surface.owner_reference)),
        )
    {
        let Some(owner_reference) = owner_reference else {
            continue;
        };
        let native_stream = design_stream(id);
        if sketch_owners.contains(&(native_stream, owner_reference)) {
            relation_owners.insert((native_stream, record_index), owner_reference);
        }
    }
    for relation in &native.sketch_relations {
        let native_stream = design_stream(&relation.id);
        let resolve = |indices: &[u32]| {
            indices
                .iter()
                .map(|record_index| {
                    sketch_operands
                        .get(&(native_stream, *record_index))
                        .cloned()
                        .unwrap_or(records::SketchRelationOperand::Record {
                            record_index: *record_index,
                        })
                })
                .collect::<Vec<_>>()
        };
        if relation.resolved_members != resolve(&relation.members)
            || relation.resolved_return_members != resolve(&relation.return_members)
        {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message:
                    "Fusion sketch relation typed operands disagree with its indexed references"
                        .into(),
                entity: Some(relation.id.clone()),
            });
        }
        for member in relation.members.iter().chain(&relation.return_members) {
            if !typed_sketch_records.contains(&(native_stream, *member)) {
                continue;
            }
            if relation_owners
                .insert((native_stream, *member), relation.owner_reference)
                .is_some_and(|owner| owner != relation.owner_reference)
            {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: "Fusion sketch member belongs to multiple sketch owners".into(),
                    entity: Some(relation.id.clone()),
                });
            }
        }
    }
    for entity in native
        .design_entity_headers
        .iter()
        .filter(|entity| entity.object_kind == Some(records::DesignObjectKind::Sketch))
    {
        let native_stream = design_stream(&entity.id);
        let Ok(owner) = u32::try_from(entity.entity_suffix) else {
            continue;
        };
        for member in &entity.member_indices {
            if !typed_sketch_records.contains(&(native_stream, *member)) {
                continue;
            }
            if relation_owners
                .insert((native_stream, *member), owner)
                .is_some_and(|existing| existing != owner)
            {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: "Fusion sketch member belongs to multiple sketch owners".into(),
                    entity: Some(entity.id.clone()),
                });
            }
        }
    }
    for pair in &native.design_dimension_locus_pairs {
        let native_stream = design_stream(&pair.id);
        let owner = companions_by_index
            .get(&(native_stream, pair.governing_companion_record_index))
            .and_then(|companion| {
                owners_by_index.get(&(native_stream, companion.owner_record_index))
            })
            .and_then(|parameter_owner| {
                placements_by_scope.get(&(native_stream, parameter_owner.scope_record_index))
            })
            .and_then(|placement| u32::try_from(placement.entity_suffix).ok());
        let Some(owner) = owner else {
            continue;
        };
        for member in [
            pair.first_geometry_record_index,
            pair.second_geometry_record_index,
        ] {
            if relation_owners
                .insert((native_stream, member), owner)
                .is_some_and(|existing| existing != owner)
            {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: "Fusion sketch member belongs to multiple sketch owners".into(),
                    entity: Some(pair.id.clone()),
                });
            }
        }
    }
    for group in &native.design_dimension_locus_groups {
        let native_stream = design_stream(&group.id);
        for member in group
            .loci
            .iter()
            .map(|locus| locus.geometry_record_index)
            .chain(group.return_members.iter().copied())
        {
            if relation_owners
                .insert((native_stream, member), group.owner_reference)
                .is_some_and(|existing| existing != group.owner_reference)
            {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: "Fusion sketch member belongs to multiple sketch owners".into(),
                    entity: Some(group.id.clone()),
                });
            }
        }
    }
    for pair in &native.design_dimension_null_locus_pairs {
        let native_stream = design_stream(&pair.id);
        let owner = companions_by_index
            .get(&(native_stream, pair.governing_companion_record_index))
            .and_then(|companion| {
                owners_by_index.get(&(native_stream, companion.owner_record_index))
            })
            .and_then(|parameter_owner| {
                placements_by_scope.get(&(native_stream, parameter_owner.scope_record_index))
            })
            .and_then(|placement| u32::try_from(placement.entity_suffix).ok());
        let Some(owner) = owner else {
            continue;
        };
        if relation_owners
            .insert((native_stream, pair.geometry_record_index), owner)
            .is_some_and(|existing| existing != owner)
        {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion sketch member belongs to multiple sketch owners".into(),
                entity: Some(pair.id.clone()),
            });
        }
    }
    for (id, record_index, owner_reference) in native
        .sketch_points
        .iter()
        .map(|point| (&point.id, point.record_index, point.owner_reference))
        .chain(
            native
                .sketch_curve_identities
                .iter()
                .map(|curve| (&curve.id, curve.record_index, curve.owner_reference)),
        )
    {
        if relation_owners
            .get(&(design_stream(id), record_index))
            .copied()
            != owner_reference
        {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion sketch geometry owner disagrees with its relation graph".into(),
                entity: Some(id.clone()),
            });
        }
    }
}

/// Validate persistent body links and their history ordering.
fn validate_body_links(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let ir = ctx.ir;
    let body_ids = ir
        .model
        .bodies
        .iter()
        .map(|body| &body.id)
        .collect::<HashSet<_>>();
    let mut body_links = std::collections::BTreeMap::new();
    for link in &native.persistent_design_links {
        let target_key = match &link.target {
            cadmpeg_ir::attributes::AttributeTarget::Body(id) if body_ids.contains(id) => {
                Some(id.0.clone())
            }
            _ => None,
        };
        let valid = target_key.is_some()
            && link.entity_kind == 3
            && !link.design_id.is_empty()
            && link.design_id.bytes().all(|byte| byte.is_ascii_digit());
        if !valid {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion persistent body link has an invalid target or group payload"
                    .into(),
                entity: Some(link.id.clone()),
            });
            continue;
        }
        body_links
            .entry(target_key.expect("validated body target"))
            .or_insert_with(Vec::new)
            .push(link);
    }
    for links in body_links.values_mut() {
        links.sort_by_key(|link| link.ordinal);
        if links.iter().enumerate().any(|(ordinal, link)| {
            link.ordinal != ordinal as u32 || link.is_current != (ordinal + 1 == links.len())
        }) {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion persistent body links have noncanonical history ordering".into(),
                entity: links.first().map(|link| link.id.clone()),
            });
        }
    }
}

/// Validate persistent subentity tags and their group ordering.
fn validate_subentity_tags(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let ir = ctx.ir;
    let face_ids = ir
        .model
        .faces
        .iter()
        .map(|face| &face.id)
        .collect::<HashSet<_>>();
    let edge_ids = ir
        .model
        .edges
        .iter()
        .map(|edge| &edge.id)
        .collect::<HashSet<_>>();
    let mut subentity_tags = std::collections::BTreeMap::new();
    for tag in &native.persistent_subentity_tags {
        let target_key = match &tag.target {
            cadmpeg_ir::attributes::AttributeTarget::Face(id) if face_ids.contains(id) => {
                Some(format!("face:{}", id.0))
            }
            cadmpeg_ir::attributes::AttributeTarget::Edge(id) if edge_ids.contains(id) => {
                Some(format!("edge:{}", id.0))
            }
            _ => None,
        };
        if target_key.is_none() || tag.token.is_empty() || tag.design_references.is_empty() {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion persistent subentity tag has an invalid target or group payload"
                    .into(),
                entity: Some(tag.id.clone()),
            });
            continue;
        }
        subentity_tags
            .entry(target_key.expect("validated subentity target"))
            .or_insert_with(Vec::new)
            .push(tag);
    }
    for tags in subentity_tags.values_mut() {
        tags.sort_by_key(|tag| tag.ordinal);
        if tags
            .iter()
            .enumerate()
            .any(|(ordinal, tag)| tag.ordinal != ordinal as u32)
        {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion persistent subentity tags have noncanonical group ordering".into(),
                entity: tags.first().map(|tag| tag.id.clone()),
            });
        }
    }
}

/// Validate each ASM history graph as a coherent state chain.
fn validate_history_graphs(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    for history in &native.asm_histories {
        if !history::graph_is_coherent(history) {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion ASM history graph is not a coherent doubly linked state chain"
                    .into(),
                entity: Some(history.id.clone()),
            });
        }
    }
}
