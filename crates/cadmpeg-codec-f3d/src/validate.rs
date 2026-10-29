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

mod sheet_metal_settings;

use cadmpeg_core::convert::f64_from_index;

use crate::design::decode::scopes::extrude::is_class_296_two_sided_to_faces_scope;
use crate::layout::assembly_class_307_264_joint_origin_scope as class_307_joint_origin;
use crate::layout::legacy_class_397_symmetric_extrude_frame as class_397;
use crate::{design, history, ids, native, records};
use cadmpeg_core::decode::id_from_index;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::{
    check::{Check, Finding},
    Severity,
};

const EPS_VALIDATE_VALIDATE_PARAMETER_SCOPES_E10: f64 = 1.0e-10;
const EPS_VALIDATE_VALIDATE_PARAMETER_SCOPES_E8: f64 = 1.0e-8;

/// Resolve the native design stream that owns a record `id`, defaulting to the
/// primary design stream when the id carries no stream qualifier.
fn design_stream(id: &str) -> &str {
    ids::native_stream(id).unwrap_or(ids::DEFAULT_STREAM)
}

/// Report whether a native `stream` scope contains the design `entry`, either
/// directly or through an `f3d:xref/` qualifier.
fn design_stream_contains_entry(stream: &str, entry: &str) -> bool {
    ids::native_scope_matches(stream, entry)
}

/// Admit the empty reference table used by a legacy Combine tool operand.
fn body_recipe_reference_table_is_admitted(
    scope: Option<&records::feature::scope::DesignParameterScope>,
    operand: &records::topology::body_recipe::DesignBodyRecipeOperand,
) -> bool {
    !operand.references().is_empty()
        || matches!(
            operand.owner,
            records::topology::body_recipe::DesignOperandOwner::ScopeReference { .. }
        ) && scope.is_some_and(|scope| {
            scope.kind() == crate::records::feature::scope::DesignFeatureKind::Combine
                && scope.combine_operation().is_some_and(|operation| {
                    operation
                        .tools
                        .iter()
                        .any(|tool| tool.record_index == operand.record_index())
                })
        })
}

/// A rigid `As-built` alignment has no parameter owners, so its angle and
/// offset are zero.
fn rigid_alignment_scalars_hold(
    alignment: &records::feature::assembly::DesignAssemblyAlignment,
    rigid_as_built: bool,
) -> bool {
    !rigid_as_built
        || !alignment.owners.is_empty()
        || (alignment.angle() == 0.0 && alignment.offset() == [0.0; 3])
}

/// A rigid group whose operands were read keeps one member after its first
/// two for each locator its frame stores, each taking the locator reference
/// that follows the previous one, with the frame its wide locator carries. A
/// group whose operands were not read, and every other scope, has no further
/// members.
fn rigid_group_members_link(
    scope: &records::feature::scope::DesignParameterScope,
    alignment: &records::feature::assembly::DesignAssemblyAlignment,
    rigid_as_built: bool,
) -> bool {
    if rigid_as_built && alignment.form.is_some() {
        scope
            .assembly_frame()
            .and_then(records::feature::scope::generation::DesignAssemblyFrame::rigid_members)
            == alignment.additional_members.len().checked_add(2)
            && alignment.additional_members.iter().all(|member| {
                let records::feature::assembly::DesignAssemblyOperandQualifier::OccurrencePath {
                    path,
                } = &member.qualifier
                else {
                    return false;
                };
                valid_assembly_operand_path_link(scope, path)
                    && member.frame.reference_record_index != 0
            })
    } else {
        alignment.additional_members.is_empty()
    }
}

fn valid_assembly_operand_path_link(
    scope: &records::feature::scope::DesignParameterScope,
    path: &records::feature::assembly::DesignAssemblyOperandPath,
) -> bool {
    let link = path.link();
    let variable_reference = design::assembly::variable_reference_assembly_generation(
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
    );
    let wide = design::assembly::AssemblyScopeGeneration::of_scope(scope)
        .operand_frame_variant()
        .is_some_and(design::assembly::AssemblyOperandFrameVariant::is_wide_family)
        || design::assembly::scope_is_wide_as_built(scope);
    link.locator_record_index.checked_add(1) == Some(path.record_index)
        // A variable-reference or wide locator's path can continue over
        // further records before its wrapper.
        && if variable_reference || wide {
            link.locator_record_index
                .checked_add(2)
                .zip(link.locator_record_index.checked_add(65))
                .is_some_and(|(first, last)| (first..=last).contains(&link.wrapper_record_index))
        } else {
            link.locator_record_index.checked_add(2) == Some(link.wrapper_record_index)
        }
}

fn valid_class_363_operand_path_link(
    frame: &records::feature::assembly::DesignAssemblyOperandFrame,
    path: &records::feature::assembly::DesignAssemblyOperandPath,
) -> bool {
    let link = path.link();
    link.locator_class_tag.as_str() == "363"
        && link.wrapper_class_tag.as_str() == "388"
        && path.class_tag().as_str() == "386"
        && link.locator_record_index == frame.reference_record_index
}

fn valid_class_307_joint_origin_qualifier(
    native: &native::F3dNative,
    records_by_index: &HashMap<(&str, u32), &records::decal::DesignRecordHeader>,
    stream: &str,
    frame: &records::feature::assembly::DesignAssemblyOperandFrame,
    qualifier: &records::feature::assembly::DesignAssemblyOperandQualifier,
) -> bool {
    let records::feature::assembly::DesignAssemblyOperandQualifier::JointOrigin {
        scope_record_index,
        class_tag,
        paired_class_tag,
        ..
    } = qualifier
    else {
        return false;
    };
    frame.reference_record_index == *scope_record_index
        && class_tag.as_str() == "307"
        && paired_class_tag.as_str() == "264"
        && design_header_class_matches(
            records_by_index,
            stream,
            *scope_record_index,
            class_tag.as_str(),
        )
        && native
            .design_parameter_scopes
            .iter()
            .filter(|target_scope| {
                design_stream(&target_scope.id) == stream
                    && target_scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::JointOrigin
                    && target_scope.record_index == *scope_record_index
                    && target_scope.class_tag == *class_tag
                    && target_scope.paired_class_tag == *paired_class_tag
                    && joint_origin_frame_length(target_scope)
                        == Some(u64_from_index(class_307_joint_origin::LEN))
                    && target_scope.joint_origin_transform() == Some(frame.transform)
            })
            .count()
            == 1
}

/// The frame length a `JointOrigin` scope's generation names.
pub(super) fn joint_origin_frame_length(
    scope: &records::feature::scope::DesignParameterScope,
) -> Option<u64> {
    match scope.generation() {
        records::feature::scope::generation::DesignScopeGeneration::JointOrigin(frame) => {
            Some(frame.frame_length())
        }
        _ => None,
    }
}

/// The frame length a `Component Insert` scope's generation names.
pub(super) fn component_insert_frame_length(
    scope: &records::feature::scope::DesignParameterScope,
) -> Option<u64> {
    match scope.generation() {
        records::feature::scope::generation::DesignScopeGeneration::ComponentInsert(frame) => {
            Some(frame.frame_length())
        }
        _ => None,
    }
}

/// The scope delta of the placement matrix of a `Component Insert` frame
/// that stores one, by frame length, paired class and class.
pub(super) fn component_insert_matrix_delta(
    frame_length: u64,
    class_tag: &str,
    paired_class_tag: &str,
) -> Option<u64> {
    match (frame_length, paired_class_tag) {
        (385, "262") if class_tag == "283" => Some(46),
        (404, _) => Some(54),
        (399, "259") => Some(50),
        (381, "261") => Some(49),
        (395, "258") => Some(46),
        (389, "264") if class_tag == "414" => Some(50),
        _ => None,
    }
}

/// Whether a `Component Insert` frame holds the placement form its frame
/// stores: no matrix, a matrix in the scope alone, or a matrix that its
/// carrier also stores.
fn component_insert_frame_admits_placement(
    frame_length: Option<u64>,
    class_tag: &str,
    paired_class_tag: &str,
    placement: Option<&records::feature::assembly_features::DesignComponentInsertMatrix>,
) -> bool {
    let Some(frame_length) = frame_length else {
        return false;
    };
    match (frame_length, paired_class_tag, placement) {
        (261, "263", None) if class_tag == "296" => true,
        (261, "261", None) if class_tag == "410" => true,
        (261, "258", None) if class_tag == "426" => true,
        (261, "266", None) if class_tag == "434" => true,
        (257 | 261 | 267, "264", None) if class_tag == "414" => true,
        (257, "262", None) if class_tag == "283" => true,
        (385, "262", Some(matrix)) if class_tag == "283" => matrix.carrier.is_none(),
        (frame_length, paired, Some(matrix)) => {
            component_insert_matrix_delta(frame_length, class_tag, paired).is_some()
                && matrix.carrier.is_some()
        }
        _ => false,
    }
}

/// Whether the record header of `record_index` has the dynamic class
/// `class_tag`.
fn design_header_class_matches(
    records_by_index: &HashMap<(&str, u32), &records::decal::DesignRecordHeader>,
    stream: &str,
    record_index: u32,
    class_tag: &str,
) -> bool {
    records_by_index
        .get(&(stream, record_index))
        .is_some_and(|header| header.class_tag.as_str() == class_tag)
}

fn valid_axial_selector_identity(
    records_by_index: &HashMap<(&str, u32), &records::decal::DesignRecordHeader>,
    stream: &str,
    scope: &records::feature::scope::DesignParameterScope,
    selector: &records::feature::assembly::DesignAssemblyAxialSelectorIdentity,
) -> bool {
    let utf16_len = |value: &str| u64::try_from(value.encode_utf16().count()).ok();
    let Some(external_link_len) = utf16_len(&selector.external_link_name) else {
        return false;
    };
    let version_valid = selector.external_version.as_ref().is_none_or(|version| {
        utf16_len(version.version_urn.value.as_str())
            .is_some_and(|length| (1..=256).contains(&length))
    });
    let selector_pair_is_referenced = scope
        .reference_members()
        .values()
        .zip(scope.reference_members().values().skip(1))
        .filter(|(first, second)| {
            [**first, **second] == [selector.axis_record_index, selector.selector_record_index]
        })
        .count()
        == 1;
    let selector_records_are_unique = [selector.axis_record_index, selector.selector_record_index]
        .iter()
        .all(|record_index| {
            scope
                .reference_members()
                .values()
                .filter(|member| *member == record_index)
                .count()
                == 1
        });

    design_header_class_matches(
        records_by_index,
        stream,
        selector.axis_record_index,
        selector.axis_class_tag.as_str(),
    ) && design_header_class_matches(
        records_by_index,
        stream,
        selector.selector_record_index,
        selector.selector_class_tag.as_str(),
    ) && selector.axis_record_index.checked_add(3) == Some(selector.selector_record_index)
        && selector.selector_record_index.checked_add(3) == Some(selector.nested_record_index)
        && selector.selector_record_index.checked_add(5) == Some(selector.role_record_index)
        && version_valid
        && selector
            .external_asset_id
            .as_str()
            .eq_ignore_ascii_case(selector.selector_asset_id.as_str())
        && selector.occurrence_reference != 0
        && selector.external_object_reference != 0
        && (1..=256).contains(&external_link_len)
        && selector_pair_is_referenced
        && selector_records_are_unique
}

fn valid_axial_assembly_targets(
    native: &native::F3dNative,
    records_by_index: &HashMap<(&str, u32), &records::decal::DesignRecordHeader>,
    stream: &str,
    scope: &records::feature::scope::DesignParameterScope,
    frames: &[records::feature::assembly::DesignAssemblyOperandFrame; 2],
    targets: &[&records::feature::assembly::DesignAssemblyAxialOperandTarget; 2],
) -> bool {
    targets
        .iter()
        .copied()
        .zip(frames)
        .all(|(target, frame)| match target {
            records::feature::assembly::DesignAssemblyAxialOperandTarget::ComponentInsertOccurrence {
                component_insert_scope_record_index,
                construction_record_index,
                construction_class_tag,
                selectors,
                ..
            } => {
                let component_scopes = native
                    .design_parameter_scopes
                    .iter()
                    .filter(|target_scope| {
                        design_stream(&target_scope.id) == stream
                            && target_scope.kind()
                                == crate::records::feature::scope::DesignFeatureKind::ComponentInsert
                            && target_scope.record_index == *component_insert_scope_record_index
                            && target_scope.component_insert_construction().is_some_and(
                                |construction| {
                                    construction
                                        .neutron_role
                                        .eq_ignore_ascii_case(selectors[0].occurrence_role.as_str())
                                },
                            )
                    })
                    .count();
                frame.reference_record_index == *construction_record_index
                    && scope
                        .reference_members()
                        .values()
                        .filter(|record_index| **record_index == *construction_record_index)
                        .count()
                        == 1
                    && design_header_class_matches(
                        records_by_index,
                        stream,
                        *construction_record_index,
                        construction_class_tag.as_str(),
                    )
                    && valid_axial_selector_identity(records_by_index, stream, scope, &selectors[0])
                    && valid_axial_selector_identity(records_by_index, stream, scope, &selectors[1])
                    && selectors[0].selects_same_object(&selectors[1])
                    && selectors[0]
                        .occurrence_role
                        .as_str()
                        .eq_ignore_ascii_case(selectors[1].occurrence_role.as_str())
                    && component_scopes == 1
            }
            records::feature::assembly::DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin {
                scope_record_index,
            } => {
                frame.reference_record_index == *scope_record_index
                    && native
                        .design_parameter_scopes
                        .iter()
                        .filter(|target_scope| {
                            design_stream(&target_scope.id) == stream
                                && target_scope.kind()
                                    == crate::records::feature::scope::DesignFeatureKind::JointOrigin
                                && target_scope.record_index == *scope_record_index
                                && target_scope.joint_origin_transform() == Some(frame.transform)
                        })
                        .count()
                        == 1
            }
        })
}

use crate::records::topology::extrude_selection::DesignOperandRole;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

fn reload_native_arena<T: serde::de::DeserializeOwned>(
    decode: &DecodeContext<'_>,
    ir: &CadIr,
    name: &str,
) -> Result<Vec<T>, CodecError> {
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(Vec::new());
    };
    namespace
        .arena_as_for_decode(decode, name)
        .map_err(Into::into)
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
    /// headers they read. A decode below full depth stops before them, and a
    /// contained design-intent failure drops them, so in either case the
    /// stored scopes and owners carry no design-intent bindings.
    fn holds_design_intent(&self) -> bool {
        self.ir.scope.design
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
    let native = match native::F3dNative::load_inner(decode, namespace).map_err(CodecError::from) {
        Ok(native) => native,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => {
            return Ok(vec![Finding {
                related: Vec::new(),
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion native namespace does not match the expected arena shape".into(),
                entity: None,
            }]);
        }
    };
    validate_loaded(decode, ir, &native)
}

fn validate_loaded(
    decode: &DecodeContext<'_>,
    ir: &CadIr,
    native: &native::F3dNative,
) -> Result<Vec<Finding>, CodecError> {
    let ctx = Ctx::new(ir, native, decode);
    let mut findings = Vec::new();
    let mut expected_face_operands = reload_native_arena(decode, ir, "design_face_operands")?;
    let scope_histories = history::bind_scope_histories(
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        &native.asm_histories,
    );
    history::bind_face_operand_history_candidates(
        decode,
        &mut expected_face_operands,
        &native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.construction_recipes,
        history::BindingHistories::Shared(&native.asm_histories),
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
    validate_canvas_images(&ctx, &mut findings);
    validate_decal_images(&ctx, &mut findings);
    validate_mesh_features(&ctx, &mut findings);
    validate_component_occurrences(&ctx, &mut findings);
    validate_configurations(&ctx, &mut findings);
    validate_feature_timelines(&ctx, &mut findings)?;
    validate_parameter_scopes(&ctx, &mut findings)?;
    sheet_metal_settings::validate(&ctx, &mut findings);
    validate_extrude_selection_groups(&ctx, &mut findings);
    validate_construction_operand_groups(&ctx, &mut findings);
    validate_path_feature_operand_roles(&ctx, &mut findings);
    validate_extrude_parameter_operands(&ctx, &mut findings);
    let fillet_radius_group_records = validate_fillet_radius_groups(&ctx, &mut findings);
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
    validate_face_group_member_resolution(
        &mut findings,
        face_group_members,
        &face_operand_records,
        &native.design_entity_selection_operands,
    );
    validate_face_source_groups(&ctx, &mut findings);
    validate_sketch_placements(&ctx, &mut findings);
    validate_parameter_owners(&ctx, &mut findings);
    validate_parameter_companions(&ctx, &mut findings);
    validate_parameter_state_companions(&ctx, &mut findings);
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

/// Validate ACT record identity, table/group joins, ordered registries, and the
/// stored document-root discriminator.
fn validate_act(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    let native = ctx.native;
    let mut record_indices = HashSet::new();
    for entity in &native.act_entities {
        let stream = entity.stream();

        let unique_index = record_indices.insert((stream, entity.record_index()));
        if !unique_index {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ACT entity has an invalid identity, table membership, or change-group frame",
                Some(entity.id().to_owned()),
            );
        }
    }

    let mut guid_ordinals = std::collections::BTreeMap::<&str, (HashSet<u32>, &str)>::new();
    let mut guid_offsets = HashSet::new();
    for guid in &native.act_guids {
        let stream = guid.stream();

        guid_ordinals
            .entry(stream)
            .or_insert((HashSet::new(), guid.id().as_str()));
        let unique_ordinal = guid_ordinals
            .get_mut(stream)
            .ok_or_else(|| CodecError::malformed("F3D ACT GUID stream index missing"))?
            .0
            .insert(guid.ordinal);
        let unique_offset = guid
            .byte_offset()
            .is_none_or(|offset| guid_offsets.insert((stream, offset)));
        let valid = unique_offset && unique_ordinal;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ACT GUID-pool entry has an invalid identity, ordinal, offset, or GUID",
                Some(guid.id().to_owned()),
            );
        }
    }

    let mut table_reference_ordinals =
        std::collections::BTreeMap::<&str, (HashSet<u32>, &str)>::new();
    let mut table_reference_offsets = HashSet::new();
    for reference in &native.act_table_references {
        let stream = reference.stream();

        table_reference_ordinals
            .entry(stream)
            .or_insert((HashSet::new(), reference.id().as_str()));
        let unique_ordinal = table_reference_ordinals
            .get_mut(stream)
            .ok_or_else(|| CodecError::malformed("F3D ACT table stream index missing"))?
            .0
            .insert(reference.ordinal);
        let unique_offset = reference
            .byte_offset()
            .is_none_or(|offset| table_reference_offsets.insert((stream, offset)));
        let valid = unique_ordinal && unique_offset;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ACT table reference has an invalid identity, ordinal, or offset",
                Some(reference.id().to_owned()),
            );
        }
    }

    let mut registry_ordinals = std::collections::BTreeMap::<&str, (HashSet<u32>, &str)>::new();
    let mut registry_offsets = HashSet::new();
    let mut registry_names = HashSet::new();
    for channel in &native.act_registry_channels {
        let stream = channel.stream();

        registry_ordinals
            .entry(stream)
            .or_insert((HashSet::new(), channel.id().as_str()));
        let unique_ordinal = registry_ordinals
            .get_mut(stream)
            .ok_or_else(|| CodecError::malformed("F3D ACT registry stream index missing"))?
            .0
            .insert(channel.ordinal);
        let unique_offset = channel
            .byte_offset()
            .is_none_or(|offset| registry_offsets.insert((stream, offset)));
        let unique_name = registry_names.insert((stream, channel.name()));
        let valid = unique_offset && unique_name && unique_ordinal;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ACT channel-registry entry has an invalid identity, ordinal, offset, name, or GUID",
                Some(channel.id().to_owned()),
            );
        }
    }

    let mut root_counts = BTreeMap::<&str, (&str, usize)>::new();
    for root in &native.act_root_components {
        let stream = root.stream();

        root_counts
            .entry(stream)
            .or_insert((root.id().as_str(), 0))
            .1 += 1;
        let unique_record_index = record_indices.insert((stream, root.record_index));
        if !unique_record_index {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ACT root component has an invalid identity, frame, or tracked-entity reference",
                Some(root.id().to_owned()),
            );
        }
    }

    for (witness, count) in root_counts.into_values() {
        if count != 1 {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ACT stream does not have exactly one document-root component link",
                Some(witness.to_owned()),
            );
        }
    }
    for (ordinals, witness, family) in guid_ordinals
        .into_values()
        .map(|(ordinals, witness)| (ordinals, witness, "GUID pool"))
        .chain(
            table_reference_ordinals
                .into_values()
                .map(|(ordinals, witness)| (ordinals, witness, "table reference")),
        )
        .chain(
            registry_ordinals
                .into_values()
                .map(|(ordinals, witness)| (ordinals, witness, "channel registry")),
        )
    {
        let contiguous = id_from_index(ordinals.len()).is_some_and(|length| {
            ordinals
                .iter()
                .max()
                .and_then(|maximum| maximum.checked_add(1))
                == Some(length)
        });
        if !contiguous {
            let message = match family {
                "GUID pool" => "Fusion ACT GUID pool ordinals are not contiguous from zero",
                "table reference" => {
                    "Fusion ACT table reference ordinals are not contiguous from zero"
                }
                _ => "Fusion ACT channel registry ordinals are not contiguous from zero",
            };
            push_constant_finding(
                findings,
                Check::NativeLinks,
                message,
                Some(witness.to_owned()),
            );
        }
    }
    Ok(())
}

/// Validate unique configuration entries and a single authored table.
fn validate_configurations(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let mut entry_names = HashSet::new();
    for configuration in &ctx.native.design_configurations {
        let name = configuration.entry_name().as_str();
        if !entry_names.insert(name) {
            let id = configuration.id();
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design configuration entry name is duplicated",
                Some(id),
            );
        }
    }
    let mut nonempty_tables = ctx
        .native
        .design_configurations
        .iter()
        .filter(|configuration| !configuration.variants().is_empty());
    let first = nonempty_tables.next();
    if nonempty_tables.next().is_some() {
        let id = first.map(super::records::configuration::DesignConfiguration::id);
        push_constant_finding(
            findings,
            Check::NativeLinks,
            "Fusion Design configurations have no single authored table order",
            id,
        );
    }
}

/// Validate authored Design timeline order and its exact type and scope joins.
/// Scope history order is checked only in a document that holds the ASM
/// histories qualifying each history state.
fn validate_feature_timelines(ctx: &Ctx, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    let native = ctx.native;
    let mut timeline_ordinals = HashMap::<&str, u32>::new();
    let mut entity_type_counts = HashMap::<(&str, u64), usize>::new();
    let mut expected = BTreeMap::<(&str, u64), (String, u32, bool, &str)>::new();
    let mut design_types = native.design_types.iter().collect::<Vec<_>>();
    design_types.sort_by(|left, right| {
        (ids::native_stream(left.id()), left.class_tag.as_str())
            .cmp(&(ids::native_stream(right.id()), right.class_tag.as_str()))
    });
    for design_type in design_types {
        let Some(segment) = ids::design_segment(design_type.id()) else {
            continue;
        };
        let class_tag = Some(design_type.class_tag.as_str().to_owned());
        for entity_id in design_type.entities.values() {
            *entity_type_counts.entry((segment, *entity_id)).or_default() += 1;
        }
        if !design_type
            .type_guid
            .as_str()
            .eq_ignore_ascii_case(crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID)
        {
            continue;
        }

        let source_ordinal = timeline_ordinals.entry(segment).or_default();
        for entity_id in design_type.entities.values() {
            let valid_type =
                crate::design::decode::meta::is_supported_feature_timeline_type(design_type)
                    && class_tag.as_ref().is_some_and(|tag| {
                        records::references::DesignClassTag::try_from(tag.clone()).is_ok()
                    });
            let Some(class_tag) = class_tag.clone() else {
                continue;
            };

            if expected
                .insert(
                    (segment, *entity_id),
                    (
                        class_tag,
                        *source_ordinal,
                        valid_type,
                        design_type.id().as_str(),
                    ),
                )
                .is_some()
            {
                push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design feature-timeline type repeats an entity identity",
                    Some(design_type.id().to_owned()),
                );
            }
            *source_ordinal = source_ordinal.checked_add(1).ok_or_else(|| {
                CodecError::Malformed("F3D feature timeline source ordinal overflows".into())
            })?;
        }
    }

    let mut actual = native.design_feature_timelines.iter().collect::<Vec<_>>();
    actual.sort_by(|left, right| {
        (left.segment(), left.source_ordinal).cmp(&(right.segment(), right.source_ordinal))
    });
    let mut actual_records = HashSet::<(&str, u64)>::new();
    let mut item_records = HashSet::<(&str, u64)>::new();
    for timeline in actual {
        let segment = timeline.segment();
        let expected_type = expected.get(&(segment, timeline.record_index.get()));
        let unique_record = actual_records.insert((segment, timeline.record_index.get()));
        let record_valid =
            expected_type.is_some_and(|(class_tag, source_ordinal, valid_type, _)| {
                *valid_type
                    && timeline.class_tag.as_str() == class_tag
                    && timeline.source_ordinal == *source_ordinal
            }) && entity_type_counts.get(&(segment, timeline.record_index.get())) == Some(&1)
                && entity_type_counts.get(&(segment, timeline.context_record_index.get()))
                    == Some(&1)
                && unique_record;
        let mut items_valid = true;
        for item in timeline.frame().items().iter().map(|item| item.value) {
            items_valid &= entity_type_counts.get(&(segment, item)) == Some(&1)
                && item_records.insert((segment, item));
        }
        if !record_valid || !items_valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design feature timeline has an invalid typed frame",
                Some(timeline.id().to_owned()),
            );
        }
    }
    for ((segment, entity_id), (_, _, _, type_id)) in &expected {
        if !actual_records.contains(&(*segment, *entity_id)) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design feature-timeline type has no decoded record",
                Some((*type_id).to_owned()),
            );
        }
    }

    let mut scope_positions = HashMap::<&str, u64>::new();
    match crate::design::feature_project::authored_scope_ordinals_per_stream(
        &native.design_parameter_scopes,
        &native.design_feature_timelines,
    ) {
        Ok(authored) => {
            for scope in &native.design_parameter_scopes {
                let stream = design_stream(&scope.id);
                let Some(position) = authored.get(&(stream, scope.record_index)) else {
                    continue;
                };

                scope_positions.insert(scope.id.as_str(), *position);
            }
        }
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => {
            let entity = native
                .design_parameter_scopes
                .first()
                .map(|scope| scope.id.clone());
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design scopes have no complete authored order",
                entity,
            );
        }
    }

    if !ctx.holds_design_intent() {
        return Ok(());
    }
    let scope_history = crate::design::feature_project::ScopeHistoryGraph::new(
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        &native.design_component_naming_spaces,
        &native.asm_histories,
    );
    for scope in &native.design_parameter_scopes {
        let Some(position) = scope_positions.get(scope.id.as_str()).copied() else {
            continue;
        };
        match scope_history.predecessor(scope, |candidate| {
            scope_positions.contains_key(candidate.id.as_str())
        }) {
            Ok(crate::design::feature_project::ScopeHistoryPredecessor::Scope(predecessor)) => {
                if scope_positions
                    .get(predecessor.id.as_str())
                    .is_some_and(|predecessor| *predecessor >= position)
                {
                    push_constant_finding(
                        findings,
                        Check::NativeLinks,
                        "Fusion Design history edge runs forward in its feature timeline",
                        Some(scope.id.clone()),
                    );
                }
            }
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(_) => push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design scope history-state dependency is cyclic",
                Some(scope.id.clone()),
            ),
            Ok(
                crate::design::feature_project::ScopeHistoryPredecessor::None
                | crate::design::feature_project::ScopeHistoryPredecessor::Ambiguous,
            ) => {}
        }
    }
    Ok(())
}

fn mesh_record_offset_is(
    record: &records::mesh::DesignMeshRecordIdentity,
    relative: u64,
    offset: u64,
) -> bool {
    record.byte_offset().checked_add(relative) == Some(offset)
}

/// Validate complete `Base Mesh Feature` record graphs and their neutral links.
/// The neutral feature link is checked only in a document that holds features.
fn validate_mesh_features(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let mut feature_ids = HashSet::new();
    let mut scope_records = HashSet::new();
    let mut collection_records = HashSet::new();
    let mut body_records = HashSet::new();
    let mut entry_records = HashSet::new();
    let mut guid_records = HashSet::new();
    let mut wrapper_records = HashSet::new();
    let mut state_records = HashSet::new();
    let mut node_records = HashSet::new();
    let mut auxiliary_records = HashSet::new();
    let mut collection_owner_records = HashSet::new();
    let mut body_owner_records = HashMap::new();
    let mut texture_table_records = HashSet::new();
    let mut filename_records = HashMap::new();
    let mut projected_tessellations = HashSet::new();
    let asset_ids = ctx
        .ir
        .model
        .assets
        .iter()
        .map(|asset| &asset.id)
        .collect::<std::collections::HashSet<_>>();
    let tessellation_ids = ctx
        .ir
        .model
        .tessellations
        .iter()
        .map(|tessellation| tessellation.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    for feature in &ctx.native.design_mesh_features {
        let stream = design_stream(&feature.id);
        let scope = ctx
            .scopes_by_index
            .get(&(stream, feature.scope().record().record_index()));
        let mut valid = feature_ids.insert(feature.id.as_str())
            && scope_records.insert((stream, feature.scope().record().record_index()))
            && collection_records.insert((stream, feature.collection().record().record_index()))
            && texture_table_records
                .insert((stream, feature.texture_table.record().record_index()))
            && collection_owner_records
                .insert((stream, feature.collection_owner.record().record_index()))
            && mesh_record_offset_is(
                feature.collection_owner.record(),
                262,
                feature.collection_owner.backlink_offset(),
            )
            && scope.is_some_and(|scope| {
                scope.kind() == crate::records::feature::scope::DesignFeatureKind::BaseMeshFeature
            });

        let mut resources = feature.texture_table.resources().iter().collect::<Vec<_>>();
        resources.sort_by_key(|left| left.filename_ordinal);
        let mut resources_valid = true;
        for resource in &resources {
            let filename_key = (stream, resource.file.record().record_index());
            let filename_record_consistent = filename_records
                .get(&filename_key)
                .is_none_or(|record| *record == resource.file.record());

            filename_records
                .entry(filename_key)
                .or_insert(resource.file.record());
            resources_valid = filename_record_consistent && asset_ids.contains(&resource.asset);
            if !resources_valid {
                break;
            }
        }
        valid &= resources_valid;

        for body in feature.bodies() {
            let owner_key = (stream, body.owner_record.record_index());
            let owner_consistent = body_owner_records
                .get(&owner_key)
                .is_none_or(|record| *record == &body.owner_record);

            body_owner_records
                .entry(owner_key)
                .or_insert(&body.owner_record);
            let body_valid = body_records.insert((stream, body.placement.record().record_index()))
                && entry_records.insert((stream, body.entry.record().record_index()))
                && guid_records.insert((stream, body.guid.record().record_index()))
                && wrapper_records.insert((stream, body.wrapper_record.record_index()))
                && state_records.insert((stream, body.scene_state.record().record_index()))
                && node_records.insert((stream, body.scene_node.record_index()))
                && auxiliary_records.insert((stream, body.scene_auxiliary_record.record_index()))
                && owner_consistent
                && body.scene_node.frame_length() == 133;
            let projection_valid = if body_valid {
                match body.tessellation_id.as_deref() {
                    Some(id) => tessellation_ids.contains(id) && projected_tessellations.insert(id),
                    None => true,
                }
            } else {
                false
            };
            valid &= body_valid && projection_valid;
        }
        let projected = || {
            feature
                .bodies()
                .iter()
                .filter_map(|body| body.tessellation_id.as_deref())
        };
        if ctx.holds_design_intent() && projected().next().is_some() {
            valid &= scope.is_some_and(|scope| {
                ctx.ir.model.features.iter().any(|neutral| {
                    neutral.native_ref.as_deref() == Some(scope.id.as_str())
                        && matches!(
                            neutral.evaluation.definition(),
                            cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::MeshImport { tessellations })
                                if tessellations.iter().map(String::as_str).eq(projected())
                        )
                })
            });
        }
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design mesh feature has an invalid frame or object graph",
                Some(feature.id.clone()),
            );
        }
    }
}

/// Validate Canvas scope and Design object joins.
fn validate_canvas_images(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let mut scope_bindings = HashSet::new();
    let mut geometry_records = HashSet::new();
    let geometry_entities = native
        .design_types
        .iter()
        .filter(|design_type| {
            matches!(
                design_type.module.as_str(),
                records::entity_header::DESIGN_MODULE_BODY
                    | records::entity_header::DESIGN_MODULE_GEOMETRY
            )
        })
        .flat_map(|design_type| {
            let design_segment = ids::design_segment(design_type.id());
            design_type
                .entities
                .values()
                .map(move |suffix| (design_segment, *suffix))
        })
        .collect::<std::collections::HashSet<_>>();
    let component_entities = native
        .design_types
        .iter()
        .filter(|design_type| {
            matches!(
                design_type.module.as_str(),
                records::entity_header::DESIGN_MODULE_FUSION
                    | records::entity_header::DESIGN_MODULE_COMPONENT
            )
        })
        .flat_map(|design_type| {
            let design_segment = ids::design_segment(design_type.id());
            design_type
                .entities
                .values()
                .map(move |suffix| (design_segment, *suffix))
        })
        .collect::<std::collections::HashSet<_>>();
    for image in &native.design_canvas_images {
        let native_stream = design_stream(&image.id);
        let design_segment = ids::design_segment(&image.id);
        let scope = ctx
            .scopes_by_index
            .get(&(native_stream, image.scope_record_index));
        let scope_valid = scope.is_some_and(|scope| {
            scope.kind() == crate::records::feature::scope::DesignFeatureKind::Canvas
        });
        let scope_unique = if scope_valid {
            scope_bindings.insert((native_stream, image.scope_record_index))
        } else {
            false
        };
        let geometry_unique = if scope_unique {
            geometry_records.insert((native_stream, image.geometry().record_index()))
        } else {
            false
        };
        let valid = scope_valid
            && scope_unique
            && geometry_unique
            && scope.is_some()
            && geometry_entities.contains(&(design_segment, u64::from(image.plane_entity_suffix)))
            && component_entities
                .contains(&(design_segment, u64::from(image.component_entity_suffix)));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Canvas image has an invalid frame or Design object join",
                Some(image.id.clone()),
            );
        }
    }
}

/// Validate Decal native and neutral object joins.
fn validate_decal_images(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    const TARGET_ROLE: DesignOperandRole = DesignOperandRole::BODIES_A;
    let mut scope_bindings = HashSet::new();
    let mut asset_records = HashSet::new();
    let fusion_entities = ctx
        .native
        .design_types
        .iter()
        .filter(|design_type| design_type.module == records::entity_header::DESIGN_MODULE_FUSION)
        .flat_map(|design_type| {
            let segment = ids::design_segment(design_type.id());
            design_type
                .entities
                .values()
                .map(move |suffix| (segment, *suffix))
        })
        .collect::<std::collections::HashSet<_>>();
    for image in &ctx.native.design_decal_images {
        let native_stream = design_stream(&image.id);
        let design_segment = ids::design_segment(&image.id);
        let scope = ctx
            .scopes_by_index
            .get(&(native_stream, image.scope_record_index()));
        let group = ctx
            .operand_groups_by_index
            .get(&(native_stream, image.target_group_record_index));
        let operand = group.and_then(|group| {
            let member = group.members().first()?.value;
            ctx.native
                .design_body_recipe_operands
                .iter()
                .find(|operand| {
                    design_stream(&operand.id) == native_stream
                        && operand.scope_record_index == image.scope_record_index()
                        && operand.record_index() == member
                        && operand.owner.group() == Some((group.record_index, 0))
                })
        });
        let projected =
            if image.mapping_mode == crate::records::decal::DesignDecalMappingMode::FitToFaces {
                if let Some(operand) = operand {
                    let mut faces = (operand
                        .references()
                        .iter()
                        .flat_map(|reference| reference.candidate_faces.iter()))
                    .cloned()
                    .collect::<Vec<_>>();
                    faces.sort_by(|left, right| left.as_str().cmp(right.as_str()));
                    faces.dedup();
                    (!faces.is_empty()).then_some((operand, faces))
                } else {
                    None
                }
            } else {
                None
            };
        let neutral_is_valid = projected.is_none_or(|(operand, expected_faces)| {
            scope.is_some_and(|scope| {
                ctx.ir.model.features.iter().any(|feature| {
                    feature.native_ref.as_deref() == Some(scope.id.as_str())
                        && matches!(
                            feature.evaluation.definition(),
                            cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Decal {
                                asset,
                                faces: cadmpeg_ir::features::FaceSelection::Resolved { faces, native },
                                mapping: cadmpeg_ir::features::DecalMapping::FitToFaces,
                                opacity: None,
                            }) if faces == &expected_faces
                                && native == &operand.id
                                && ctx.ir.model.assets.iter().any(|candidate| {
                                    candidate.id == *asset
                                        && candidate.name.as_ref().map(cadmpeg_core::text::NonBlankString::as_str) == Some(image.asset.name())
                                })
                        )
                })
            })
        });
        let scope_valid = scope.is_some_and(|scope| {
            scope.kind() == crate::records::feature::scope::DesignFeatureKind::Decal
        });
        let scope_unique = if scope_valid {
            scope_bindings.insert((native_stream, image.scope_record_index()))
        } else {
            false
        };
        let asset_unique = if scope_unique {
            asset_records.insert((native_stream, image.asset.record_index()))
        } else {
            false
        };
        let valid = scope_valid
            && scope_unique
            && asset_unique
            && scope.is_some()
            && fusion_entities.contains(&(design_segment, u64::from(image.asset.entity_suffix())))
            && group.is_some_and(|group| {
                group.scope_record_index == image.scope_record_index()
                    && group.role() == TARGET_ROLE
                    && group.members().len() == 1
            })
            && operand.is_some()
            && neutral_is_valid;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Decal image has an invalid frame or Design object join",
                Some(image.id.clone()),
            );
        }
    }
}

/// Validate the ordered Design body-map binding entries and their pair runs.
fn validate_body_bindings(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let mut binding_offsets = HashSet::new();
    let mut binding_ids = HashSet::new();
    let mut binding_groups =
        std::collections::HashMap::<(&str, u32), Vec<&records::bodies::DesignBodyBinding>>::new();
    for binding in &native.design_body_bindings {
        let native_stream = design_stream(binding.id());
        let resolved_valid = if let Some(body) = &binding.body {
            let has_named_source = native.body_native_keys.iter().any(|key| {
                ids::same_native_occurrence(key.source_namespace.as_str(), binding.id())
                    && key.source_brep.as_deref() == Some(binding.blob_name())
            });
            let source_keys = native.body_native_keys.iter().filter(|key| {
                ids::same_native_occurrence(key.source_namespace.as_str(), binding.id())
                    && if has_named_source {
                        key.source_brep.as_deref() == Some(binding.blob_name())
                    } else {
                        key.source_brep.is_none()
                    }
            });
            match crate::brep::resolve_body_selector(source_keys, binding.asm_body_key) {
                Ok(Some(resolved)) => resolved == body,
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Ok(None) | Err(_) => false,
            }
        } else {
            true
        };
        let valid = design_stream_contains_entry(native_stream, binding.stream())
            && resolved_valid
            && binding_ids.insert(binding.id())
            && binding
                .asm_body_key_offset()
                .is_none_or(|offset| binding_offsets.insert((native_stream, offset)));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design body binding has an invalid ordered map entry",
                Some(binding.id().to_owned()),
            );
        }
        binding_groups
            .entry((native_stream, binding.map_ordinal()))
            .or_default()
            .push(binding);
    }
    for assignment in &native.design_material_assignments {
        let valid = native
            .design_body_bindings
            .iter()
            .find(|binding| binding.id().as_str() == assignment.body_binding_id.as_str())
            .is_some_and(|binding| {
                design_stream(binding.id()) == design_stream(&assignment.id)
                    && binding.asm_body_key == assignment.asm_body_key
                    && binding.entity_suffix == assignment.entity_id.suffix()
            });
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design material assignment has an invalid body-map link",
                Some(assignment.id.clone()),
            );
        }
    }
    for bindings in binding_groups.values_mut() {
        bindings.sort_by_key(|left| left.pair_ordinal());
        let complete = bindings
            .first()
            .is_some_and(|first| usize::try_from(first.pair_count()).ok() == Some(bindings.len()))
            && bindings.iter().enumerate().all(|(ordinal, binding)| {
                usize::try_from(binding.pair_ordinal()).ok() == Some(ordinal)
                    && binding.pair_count() == bindings[0].pair_count()
                    && binding.blob_name() == bindings[0].blob_name()
                    && binding.stream() == bindings[0].stream()
            });
        if !complete {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design body map has an incomplete ordered pair run",
                bindings.first().map(|binding| binding.id().to_owned()),
            );
        }
    }
    Ok(())
}

/// Validate each Design body-bounds repeated record frame.
fn validate_body_bounds(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let mut bounded_bodies = HashSet::new();
    for bounds in &native.design_body_bounds {
        let native_stream = design_stream(bounds.id());
        let mut expected_bindings = native
            .design_body_bindings
            .iter()
            .filter(|binding| {
                design_stream_contains_entry(native_stream, binding.stream())
                    && binding.entity_suffix == bounds.entity_suffix()
            })
            .collect::<Vec<_>>();
        expected_bindings.sort_by_key(|left| (left.map_ordinal(), left.pair_ordinal()));
        let valid_frame = ctx
            .entities_by_suffix
            .get(&(native_stream, bounds.entity_suffix()))
            .is_some_and(|entity| {
                entity.module() == Some(records::entity_header::DESIGN_MODULE_BODY)
                    && entity.byte_offset.is_none_or(|offset| {
                        bounds
                            .entity_byte_offset()
                            .is_none_or(|bounds_offset| offset == bounds_offset)
                    })
            })
            && bounds.body_binding_ids().eq(expected_bindings
                .iter()
                .map(|binding| binding.id().as_str()));
        let valid = if valid_frame {
            bounded_bodies.insert((native_stream, bounds.entity_suffix()))
        } else {
            false
        };
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design body bounds have an invalid repeated record frame",
                Some(bounds.id().to_owned()),
            );
        }
    }
}

/// Validate feature parameter scopes and their paired feature-operation frames.
/// The links resolve into design-intent records, so a document without them
/// has nothing to check.
fn validate_parameter_scopes(ctx: &Ctx, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    if !ctx.holds_design_intent() {
        return Ok(());
    }
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let entities_by_suffix = &ctx.entities_by_suffix;
    let placements_by_scope = &ctx.placements_by_scope;
    let mut scope_indices = HashSet::new();
    for scope in &native.design_parameter_scopes {
        let native_stream = design_stream(&scope.id);
        let unique_index = scope_indices.insert((native_stream, scope.record_index));
        let entity_link = scope.sketch_entity().map(|binding| {
            entities_by_suffix
                .get(&(native_stream, binding.entity_id.suffix()))
                .is_some_and(|entity| entity.entity_id == binding.entity_id)
        });
        let valid_sketch_profile =
            |profile: &records::topology::sketch_profile::DesignSketchProfileOperand| {
                let header = records_by_index.get(&(native_stream, profile.record_index));
                let entity = entities_by_suffix.get(&(native_stream, profile.entity_id.suffix()));
                usize::try_from(profile.scope_reference_ordinal)
                    .ok()
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                    == Some(&profile.record_index)
                    && header.is_some_and(|header| {
                        source_positions_agree(header.byte_offset, profile.byte_offset())
                            && header.class_tag == profile.class_tag
                    })
                    && entity.is_some_and(|entity| {
                        entity.in_sketch_module() && entity.entity_id == profile.entity_id
                    })
                    && profile
                        .region_selection
                        .as_ref()
                        .is_none_or(|selection| profile.region_selection_is_admitted(selection))
            };
        let extrude_profile_link = scope.extrude_profile().is_none_or(valid_sketch_profile);
        let sweep_profile_link = scope.sweep_profile().is_none_or(valid_sketch_profile);
        let base_flange_profile_link = scope.base_flange_profile().is_none_or(valid_sketch_profile);
        let base_flange_link = match scope.base_flange_operation() {
            None => true,
            Some(operation) => {
                scope.reference_members().values().copied().eq([
                    operation.profile_group_record_index,
                    operation.profile_record_index,
                    operation.thickness_record_index,
                    operation.settings_record_index,
                ]) && scope.base_flange_profile().is_some_and(|profile| {
                    profile.record_index == operation.profile_record_index
                        && profile.scope_reference_ordinal == 1
                })
            }
        };
        let edge_flange_link = match scope.edge_flange_operation() {
            None => true,
            Some(operation) => {
                // The ordered reference table is in record-index order, so the
                // check is that every role names a distinct table entry and that
                // the entries no role claims are exactly the width owners.
                let edge_count = operation.selection.shape().edges().count();
                let claimed = operation
                    .selection
                    .shape()
                    .edges()
                    .flat_map(|edge| {
                        [
                            edge.wrapper,
                            edge.group_record_index.get(),
                            edge.operand_record_index(),
                            edge.aggregate_operand_record_index,
                        ]
                    })
                    .chain(operation.selection.shape().owner_indices().copied())
                    .chain(operation.auxiliary_reference_record_indices.iter().copied())
                    .chain([
                        operation.selection.aggregate_group_record_index(),
                        operation.height_owner_record_index,
                        operation.angle_owner_record_index,
                        operation.settings_record_index,
                    ])
                    .collect::<Vec<_>>();
                let mut claimed = claimed;
                if let records::feature::sheet_metal::DesignEdgeFlangeHeightExtent::ToObject {
                    target_group_record_index,
                    target_operand_record_index,
                    offset_owner_record_index,
                    ..
                } = operation.selection.shape().height()
                {
                    for index in [
                        target_group_record_index,
                        target_operand_record_index,
                        offset_owner_record_index,
                    ] {
                        claimed.push(index);
                    }
                }
                let unique_claimed = claimed
                    .iter()
                    .copied()
                    .collect::<std::collections::HashSet<_>>();
                edge_count > 0
                    && claimed.len() == scope.reference_members().len()
                    && unique_claimed.len() == claimed.len()
                    && claimed.iter().all(|index| {
                        scope
                            .reference_members()
                            .values()
                            .any(|value| value == index)
                    })
            }
        };
        let hem_link = match scope.hem_operation() {
            None => true,
            Some(operation) => {
                let mut claimed = vec![
                    operation.edge_wrapper_record_index,
                    operation.edge_group_record_index.get(),
                    operation.edge_operand_record_index(),
                    operation.aggregate_group_record_index.get(),
                    operation.aggregate_operand_record_index(),
                    operation.settings_record_index,
                ];
                match &operation.parameter_owners {
                    crate::records::feature::sheet_metal::DesignHemParameterOwners::GapLength {
                        gap_owner_record_index,
                        length_owner_record_index,
                    } => claimed.extend([*gap_owner_record_index, *length_owner_record_index]),
                    crate::records::feature::sheet_metal::DesignHemParameterOwners::RadiusAngle {
                        radius_owner_record_index,
                        angle_owner_record_index,
                    } => claimed.extend([*radius_owner_record_index, *angle_owner_record_index]),
                    crate::records::feature::sheet_metal::DesignHemParameterOwners::GapLengthRadius {
                        gap_owner_record_index,
                        length_owner_record_index,
                        radius_owner_record_index,
                    } => claimed.extend([
                        *gap_owner_record_index,
                        *length_owner_record_index,
                        *radius_owner_record_index,
                    ]),
                }
                claimed.iter().copied().collect::<HashSet<_>>().len() == claimed.len()
                    && claimed.len() == scope.reference_members().len()
                    && claimed.iter().all(|index| {
                        scope
                            .reference_members()
                            .values()
                            .any(|value| value == index)
                    })
            }
        };
        let copy_paste_link = match scope.copy_paste_bodies_operation() {
            None => true,
            Some(operation) => {
                let group_header =
                    records_by_index.get(&(native_stream, operation.body_group_record_index));
                let relation_header =
                    records_by_index.get(&(native_stream, operation.relation_record_index));
                scope.reference_members().values().next()
                    == Some(&operation.body_group_record_index)
                    && scope
                        .reference_members()
                        .values()
                        .skip(1)
                        .copied()
                        .eq(operation.bodies().iter().map(|body| body.operand.value))
                    && group_header
                        .is_some_and(|header| header.class_tag == operation.body_group_class_tag)
                    && relation_header
                        .is_some_and(|header| header.class_tag == operation.relation_class_tag)
                    && operation
                        .bodies()
                        .iter()
                        .map(|body| body.source.value)
                        .all(|suffix| {
                            native.design_body_bindings.iter().any(|binding| {
                                design_stream(binding.id()) == native_stream
                                    && binding.entity_suffix == u64::from(suffix)
                            })
                        })
                    && operation
                        .bodies()
                        .iter()
                        .map(|body| body.copied.value)
                        .all(|suffix| {
                            native.design_body_bindings.iter().any(|binding| {
                                design_stream(binding.id()) == native_stream
                                    && binding.entity_suffix == u64::from(suffix)
                                    && binding.body.is_some()
                            })
                        })
            }
        };
        let rectangular_pattern_link = match scope.rectangular_pattern_construction() {
            None => true,
            Some(construction) => {
                let instances_link = construction.instances().is_none_or(|instances| {
                    let active = [
                        (construction.u_count(), construction.u_extent()),
                        (construction.v_count(), construction.v_extent()),
                    ]
                    .into_iter()
                    .filter(|(count, _)| *count > 1)
                    .collect::<Vec<_>>();
                    let [(count, extent)] = active.as_slice() else {
                        return false;
                    };
                    let Ok(count) = usize::try_from(*count) else {
                        return false;
                    };
                    let Some(reference_end) = count.checked_add(5) else {
                        return false;
                    };
                    let expected_records = scope
                        .reference_members()
                        .values()
                        .next()
                        .into_iter()
                        .chain(
                            scope
                                .reference_members()
                                .values_in(6..reference_end)
                                .into_iter()
                                .flatten(),
                        )
                        .copied();
                    let Some(first) = instances
                        .frames()
                        .next()
                        .map(|frame| &frame.transform.value)
                    else {
                        return false;
                    };
                    let Some(last) = instances
                        .frames()
                        .next_back()
                        .map(|frame| &frame.transform.value)
                    else {
                        return false;
                    };
                    let delta = [
                        last[0][3] - first[0][3],
                        last[1][3] - first[1][3],
                        last[2][3] - first[2][3],
                    ];
                    let distance = cadmpeg_ir::math::Vector3::from(delta).norm();
                    let component_link =
                        valid_component_pattern_occurrences(native, native_stream, instances);
                    instances
                        .frames()
                        .map(|frame| frame.record_index)
                        .eq(expected_records)
                        && instances.instance_count() == count
                        && instances.frames().all(|frame| {
                            let transform = &frame.transform.value;
                            (0..3).all(|row| {
                                (0..3).all(|column| {
                                    (transform[row][column] - first[row][column]).abs()
                                        <= EPS_VALIDATE_VALIDATE_PARAMETER_SCOPES_E10
                                })
                            })
                        })
                        && (distance - extent.abs()).abs()
                            <= EPS_VALIDATE_VALIDATE_PARAMETER_SCOPES_E8
                        && instances.frames().enumerate().all(|(ordinal, frame)| {
                            let transform = &frame.transform.value;
                            let (Some(ordinal), Some(divisor)) =
                                (f64_from_index(ordinal), f64_from_index(count - 1))
                            else {
                                return false;
                            };
                            let fraction = ordinal / divisor;
                            (0..3).all(|axis| {
                                (transform[axis][3] - first[axis][3] - delta[axis] * fraction).abs()
                                    <= EPS_VALIDATE_VALIDATE_PARAMETER_SCOPES_E8
                            })
                        })
                        && instances.frames().all(|frame| {
                            records_by_index
                                .get(&(native_stream, frame.record_index))
                                .is_some_and(|header| {
                                    source_position_before(
                                        header.byte_offset,
                                        frame.transform.offset,
                                    )
                                })
                        })
                        && component_link
                });
                instances_link
                    && native
                        .design_parameter_owners
                        .iter()
                        .filter(|owner| {
                            design_stream(owner.id()) == native_stream
                                && owner.scope_record_index() == Some(scope.record_index)
                        })
                        .count()
                        == 4
                    && construction
                        .owner_record_indices
                        .iter()
                        .all(|record_index| {
                            scope
                                .reference_members()
                                .values()
                                .any(|value| value == record_index)
                                && records_by_index.contains_key(&(native_stream, *record_index))
                        })
                    && construction
                        .owner_record_indices
                        .iter()
                        .zip([
                            f64::from(construction.u_count()),
                            f64::from(construction.v_count()),
                            construction.u_extent(),
                            construction.v_extent(),
                        ])
                        .enumerate()
                        .all(|(ordinal, (record_index, value))| {
                            native.design_parameter_owners.iter().any(|owner| {
                                design_stream(owner.id()) == native_stream
                                    && owner.record_index() == *record_index
                                    && owner.scope_record_index() == Some(scope.record_index)
                                    && u32::try_from(ordinal).ok() == owner.local_ordinal()
                                    && owner.evaluated_value().get() == value
                            })
                        })
            }
        };
        let assembly_alignment_link = match scope.assembly_alignment() {
            None => true,
            Some(alignment) => {
                let values = if alignment.owners.len() == 2 {
                    vec![alignment.angle(), alignment.offset()[2]]
                } else if design::assembly::scope_is_placement_ordered_as_built(scope) {
                    vec![
                        alignment.offset()[0],
                        alignment.offset()[1],
                        alignment.offset()[2],
                        alignment.angle(),
                    ]
                } else {
                    vec![
                        alignment.angle(),
                        alignment.offset()[0],
                        alignment.offset()[1],
                        alignment.offset()[2],
                    ]
                };
                let generation = design::assembly::AssemblyScopeGeneration::of_scope(scope);
                let operand_frame_variant = generation.operand_frame_variant();
                let variable_reference = design::assembly::variable_reference_assembly_generation(
                    scope.class_tag.as_str(),
                    scope.paired_class_tag.as_str(),
                );
                let axial_frames = matches!(
                    operand_frame_variant,
                    Some(design::assembly::AssemblyOperandFrameVariant::Axial)
                );
                let wide_frames = matches!(
                    operand_frame_variant,
                    Some(design::assembly::AssemblyOperandFrameVariant::Wide)
                );
                let compact_wide_frames = matches!(
                    operand_frame_variant,
                    Some(design::assembly::AssemblyOperandFrameVariant::CompactWide)
                );
                let wide_as_built = design::assembly::scope_is_wide_as_built(scope);
                let rigid_as_built = design::assembly::scope_is_rigid_as_built(scope);
                let as_built_frames = scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::AsBuilt
                    && (scope.assembly_frame()
                        == Some(
                            records::feature::scope::generation::DesignAssemblyFrame::Frame399,
                        )
                        || wide_as_built);
                let as_built_421_generation =
                    design::assembly::scope_legacy_as_built_421_generation(scope);
                let as_built_421 = as_built_421_generation.is_some();
                let assembly_owner_count = native
                    .design_parameter_owners
                    .iter()
                    .filter(|owner| {
                        design_stream(owner.id()) == native_stream
                            && owner.scope_record_index() == Some(scope.record_index)
                    })
                    .count();
                let alignment_lane_bounds = generation.alignment_lane_bounds(assembly_owner_count);
                let operand_frames_link = if let Some(
                    records::feature::assembly::DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                        carriers,
                        ..
                    },
                ) = &alignment.form
                {
                    carriers
                        .references()
                        .into_iter()
                        .enumerate()
                        .all(|(ordinal, reference)| {
                            let reference_ordinal = ordinal * 2;
                            scope
                                .reference_members()
                                .values()
                                .nth(reference_ordinal)
                                .copied()
                                == Some(reference.value)
                                && records_by_index.contains_key(&(native_stream, reference.value))
                        })
                } else {
                    alignment.operand_frames().is_none_or(|frames| {
                        frames[0].reference_record_index != frames[1].reference_record_index
                            && frames.iter().all(|frame| {
                                if as_built_frames {
                                    frame.reference_record_index != 0
                                } else {
                                    records_by_index.contains_key(&(
                                        native_stream,
                                        frame.reference_record_index,
                                    ))
                                }
                            })
                    })
                };
                let solved_frame_link = alignment.solved_frame().is_none_or(|frame| {
                    let Some(generation) = as_built_421_generation else {
                        return false;
                    };
                    let Some(header) =
                        records_by_index.get(&(native_stream, frame.reference_record_index))
                    else {
                        return false;
                    };
                    as_built_421
                        && scope.reference_members().values().nth(8).copied()
                            == Some(frame.reference_record_index)
                        && header.class_tag.as_str() == generation.frame_class_tag()
                        && frame.class_tag == header.class_tag
                });
                let operand_qualifiers_link = match alignment.form.as_ref() {
                    Some(records::feature::assembly::DesignAssemblyAlignmentForm::Qualified(
                        operands,
                    )) => {
                        let frames = operands.each_ref().map(|operand| operand.frame.clone());
                        match (&operands[0].qualifier, &operands[1].qualifier) {
                            (records::feature::assembly::DesignAssemblyOperandQualifier::OccurrencePath { path: first },
                             records::feature::assembly::DesignAssemblyOperandQualifier::OccurrencePath { path: second }) => {
                                let paths = [first, second];

                            let class_363_carriers = paths
                                .iter()
                                .all(|path| path.link().locator_class_tag.as_str() == "363");
                            if class_363_carriers {
                                !axial_frames
                                    && paths[0].link().locator_record_index
                                        != paths[1].link().locator_record_index
                                    && paths.iter().zip(&frames).all(|(path, frame)| {
                                        valid_class_363_operand_path_link(frame, path)
                                    })
                            } else {
                                !axial_frames
                                    && paths.iter().all(|path| valid_assembly_operand_path_link(scope, path))
                                    && paths[0].link().locator_record_index
                                        != paths[1].link().locator_record_index
                                    && paths.iter().all(|path| {
                                        (path.admitted_by_type()
                                            || matches!(path.class_tag().as_str(), "294" | "299" | "307" | "329" | "330" | "386" | "390"))
                                            && !matches!(path.link().locator_class_tag.as_str(), "363" | "378")
                                            && (path.admitted_by_type()
                                                || matches!(
                                                path.class_tag().as_str(),
                                                "294" | "299" | "307" | "330" | "386"
                                            ) || path.occurrence_guids().first().is_some_and(
                                                |guid| {
                                                    native
                                                        .design_component_occurrences
                                                        .iter()
                                                        .filter(|occurrence| {
                                                            design_stream(&occurrence.id)
                                                                == native_stream
                                                                && occurrence
                                                                    .occurrence_guid.as_str()
                                                                    .eq_ignore_ascii_case(guid.as_str())
                                                        })
                                                        .count()
                                                        == 1
                                                },
                                            ))
                                    })
                            }

                            }
                            (records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget { target: first },
                             records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget { target: second }) => {
                                axial_frames && valid_axial_assembly_targets(native, records_by_index, native_stream, scope, &frames, &[first, second])
                            }
                            _ if variable_reference && operands.iter().any(|operand| matches!(operand.qualifier, records::feature::assembly::DesignAssemblyOperandQualifier::JointOrigin { .. })) => {
                                frames[0].reference_record_index != frames[1].reference_record_index
                                    && operands.iter().all(|operand| match &operand.qualifier {
                                        records::feature::assembly::DesignAssemblyOperandQualifier::OccurrencePath { path } => valid_class_363_operand_path_link(&operand.frame, path),
                                        qualifier @ records::feature::assembly::DesignAssemblyOperandQualifier::JointOrigin { .. } => valid_class_307_joint_origin_qualifier(native, records_by_index, native_stream, &operand.frame, qualifier),
                                        records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget { .. } => false,
                                    })
                            }
                            _ => false,
                        }
                    }
                    Some(records::feature::assembly::DesignAssemblyAlignmentForm::Frames {
                        ..
                    }) => axial_frames,
                    Some(records::feature::assembly::DesignAssemblyAlignmentForm::LimitsOnly {
                        ..
                    }) => as_built_421,
                    None
                    | Some(
                        records::feature::assembly::DesignAssemblyAlignmentForm::DatumEnvelope {
                            ..
                        }
                        | records::feature::assembly::DesignAssemblyAlignmentForm::SolvedOnly {
                            ..
                        }
                        | records::feature::assembly::DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                            ..
                        },
                    ) => true,
                };
                let joint_origin_envelope_link = alignment
                    .joint_origin_scope_record_index()
                    .is_none_or(|record_index| {
                        scope.class_tag.as_str() == "276"
                            && scope.paired_class_tag.as_str() == "258"
                            && scope.assembly_frame()
                                == Some(records::feature::scope::generation::DesignAssemblyFrame::Frame604)
                            && native.design_parameter_scopes.iter().any(|target| {
                                design_stream(&target.id) == native_stream
                                    && target.kind()
                                        == crate::records::feature::scope::DesignFeatureKind::JointOrigin
                                    && target.record_index == record_index
                            })
                    });
                let alignment_scalars_link = if let Some(generation) = as_built_421_generation {
                    match (alignment.limits(), alignment.owners.as_slice()) {
                        (Some(limits), [angle, offset_x, offset_y, offset_z]) => {
                            let alignment_lanes = [
                                (offset_x.value, alignment.offset()[0], 0_u32),
                                (offset_y.value, alignment.offset()[1], 1_u32),
                                (offset_z.value, alignment.offset()[2], 2_u32),
                                (angle.value, alignment.angle(), 3_u32),
                            ];
                            let limit_order = if generation.reverse_limit_order() {
                                [1, 0]
                            } else {
                                [0, 1]
                            };
                            let limit_lanes = limit_order.into_iter().zip([4_u32, 5]).map(
                                |(index, local_ordinal)| {
                                    (
                                        limits.owner_record_indices[index],
                                        [limits.minimum(), limits.maximum()][index],
                                        local_ordinal,
                                    )
                                },
                            );
                            limits.kind == generation.limit_kind()
                                && alignment_lanes.into_iter().chain(limit_lanes).all(
                                    |(record_index, value, local_ordinal)| {
                                        native.design_parameter_owners.iter().any(|owner| {
                                            design_stream(owner.id()) == native_stream
                                                && owner.record_index() == record_index
                                                && owner.scope_record_index()
                                                    == Some(scope.record_index)
                                                && owner.local_ordinal() == Some(local_ordinal)
                                                && owner.evaluated_value().get() == value
                                        })
                                    },
                                )
                        }
                        _ => false,
                    }
                } else {
                    rigid_alignment_scalars_hold(alignment, rigid_as_built)
                        && alignment_lane_bounds.is_some_and(|(alignment_start, alignment_end)| {
                            alignment_end.checked_sub(alignment_start).is_some_and(
                                |expected_offset| alignment.owners.len() == expected_offset,
                            ) && alignment.owners.iter().zip(&values).enumerate().all(
                                |(ordinal, (lane, value))| {
                                    native.design_parameter_owners.iter().any(|owner| {
                                        design_stream(owner.id()) == native_stream
                                            && owner.record_index() == lane.value
                                            && owner.scope_record_index()
                                                == Some(scope.record_index)
                                            && u32::try_from(alignment_start + ordinal).ok()
                                                == owner.local_ordinal()
                                            && owner.evaluated_value().get() == *value
                                    })
                                },
                            )
                        })
                };
                let alignment_reference_link = if let Some(generation) = as_built_421_generation {
                    alignment.limits().is_some_and(|limits| {
                        let limit_reference_indices = if generation.reverse_limit_order() {
                            [
                                limits.owner_record_indices[1],
                                limits.owner_record_indices[0],
                            ]
                        } else {
                            limits.owner_record_indices
                        };
                        alignment.owners.len() == 4
                            && scope
                                .reference_members()
                                .values()
                                .skip(4)
                                .take(4)
                                .copied()
                                .eq([
                                    alignment.owners[1].value,
                                    alignment.owners[2].value,
                                    alignment.owners[3].value,
                                    alignment.owners[0].value,
                                ])
                            && scope
                                .reference_members()
                                .values()
                                .skip(9)
                                .take(2)
                                .eq(limit_reference_indices.iter())
                    })
                } else if wide_frames || compact_wide_frames {
                    // The wide scope lists its four owners first.
                    scope
                        .reference_members()
                        .values()
                        .take(alignment.owners.len())
                        .eq(alignment.owners.iter().map(|owner| &owner.value))
                } else if design::assembly::variable_reference_assembly_generation(
                    scope.class_tag.as_str(),
                    scope.paired_class_tag.as_str(),
                ) {
                    (0..scope.reference_members().len())
                        .filter(|&start| {
                            scope
                                .reference_members()
                                .values_in(start..start + alignment.owners.len())
                                .is_some_and(|values| {
                                    values.eq(alignment.owners.iter().map(|owner| &owner.value))
                                })
                        })
                        .count()
                        == 1
                } else {
                    scope
                        .reference_members()
                        .values()
                        .rev()
                        .take(alignment.owners.len())
                        .eq(alignment.owners.iter().map(|owner| &owner.value).rev())
                };
                let additional_members_link =
                    rigid_group_members_link(scope, alignment, rigid_as_built);
                operand_frames_link
                    && solved_frame_link
                    && operand_qualifiers_link
                    && additional_members_link
                    && joint_origin_envelope_link
                    && alignment_reference_link
                    && alignment_scalars_link
            }
        };
        let component_insert_link = match scope.component_insert_construction() {
            None => true,
            Some(construction) => {
                let relation =
                    records_by_index.get(&(native_stream, construction.relation_record_index));
                let frame_matches_transform = component_insert_frame_admits_placement(
                    component_insert_frame_length(scope),
                    scope.class_tag.as_str(),
                    scope.paired_class_tag.as_str(),
                    construction.placement.as_ref(),
                );
                let role_valid = crate::bytes::is_guid_relaxed(&construction.neutron_role)
                    || (crate::bytes::is_guid_prefix(&construction.neutron_role)
                        && construction.neutron_role.as_bytes().get(36) == Some(&b'_')
                        && construction
                            .neutron_role
                            .get(37..)
                            .is_some_and(|suffix| suffix.starts_with("urn:")));
                scope
                    .reference_members()
                    .values()
                    .copied()
                    .eq([construction.relation_record_index])
                    && construction.carrier_record_index != construction.relation_record_index
                    && role_valid
                    && frame_matches_transform
                    && relation.is_some()
                    && (native.xref_references.is_empty()
                        || native.xref_references.iter().any(|reference| {
                            reference.neutron_role.as_str() == construction.neutron_role
                                && reference
                                    .transform
                                    .map(records::xref::XrefPlacementTransform::rows)
                                    == Some((*construction.transform()).into())
                        }))
            }
        };
        let copy_paste_component_link = match scope.copy_paste_component_operation() {
            None => true,
            Some(operation) => {
                let source = native
                    .design_component_occurrences
                    .iter()
                    .find(|occurrence| {
                        design_stream(&occurrence.id) == native_stream
                            && occurrence.record_index == operation.source_occurrence_record_index
                    });
                let copied = native
                    .design_component_occurrences
                    .iter()
                    .find(|occurrence| {
                        design_stream(&occurrence.id) == native_stream
                            && occurrence.record_index == operation.copied_occurrence_record_index
                    });
                matches!(
                    scope.generation(),
                    records::feature::scope::generation::DesignScopeGeneration::CopyPaste(_)
                ) && scope
                    .reference_members()
                    .values()
                    .copied()
                    .eq([operation.relation_record_index])
                    && operation.source_occurrence_record_index
                        != operation.copied_occurrence_record_index
                    && source.is_some_and(|source| {
                        source
                            .component_guid
                            .as_str()
                            .eq_ignore_ascii_case(operation.component_guid.as_str())
                            && source
                                .occurrence_guid
                                .as_str()
                                .eq_ignore_ascii_case(operation.source_occurrence_guid.as_str())
                            && source.transform().is_none()
                    })
                    && copied.is_some_and(|copied| {
                        copied
                            .component_guid
                            .as_str()
                            .eq_ignore_ascii_case(operation.component_guid.as_str())
                            && copied
                                .occurrence_guid
                                .as_str()
                                .eq_ignore_ascii_case(operation.copied_occurrence_guid.as_str())
                            && copied.transform().map(|frame| frame.value)
                                == Some(operation.copied_transform)
                    })
            }
        };
        let draft_link = match scope.draft_operation() {
            None => true,
            Some(operation) => {
                scope.reference_members().len() >= 6
                    && scope
                        .reference_members()
                        .values()
                        .any(|value| value == &operation.angle_record_index)
                    && scope
                        .reference_members()
                        .values()
                        .any(|value| value == &operation.opposite_angle_record_index)
                    && operation.angle_record_index != operation.opposite_angle_record_index
                    && records_by_index.contains_key(&(native_stream, operation.angle_record_index))
                    && records_by_index
                        .contains_key(&(native_stream, operation.opposite_angle_record_index))
            }
        };
        let combine_link = match scope.combine_operation() {
            None => true,
            Some(operation) => {
                let expected_selections = scope
                    .reference_members()
                    .values()
                    .skip(1)
                    .step_by(2)
                    .copied()
                    .collect::<HashSet<_>>();
                let selections = std::iter::once(operation.target_record_index)
                    .chain(operation.tools.iter().map(|tool| tool.record_index))
                    .collect::<Vec<_>>();
                let actual_selections = selections.iter().copied().collect::<HashSet<_>>();
                let valid_external =
                    |selection: &records::feature::combine::DesignCombineBodySelection| {
                        selection.external_identity.as_ref().is_none_or(|_| {
                            records_by_index.contains_key(&(native_stream, selection.record_index))
                        })
                    };
                let class_pair_form =
                    match (scope.class_tag.as_str(), scope.paired_class_tag.as_str()) {
                        ("387", "258") => {
                            Some(records::feature::combine::DesignCombineForm::Compact)
                        }
                        ("329", "261") => {
                            Some(records::feature::combine::DesignCombineForm::ExtendedReference)
                        }
                        _ => None,
                    };
                scope.reference_members().len() >= 4
                    && scope.reference_members().len().is_multiple_of(2)
                    && selections.len() == scope.reference_members().len() / 2
                    && actual_selections.len() == selections.len()
                    && actual_selections == expected_selections
                    && operation.tools.iter().all(valid_external)
                    && (operation.form == records::feature::combine::DesignCombineForm::Standard
                        || class_pair_form == Some(operation.form))
            }
        };
        let thread_link = match scope.thread_construction() {
            None => true,
            Some(construction) => {
                let expected_groups: Vec<_> = match construction.form {
                    records::feature::thread::DesignThreadForm::Standard
                    | records::feature::thread::DesignThreadForm::StandardLegacy => scope
                        .reference_members()
                        .values()
                        .next()
                        .copied()
                        .into_iter()
                        .collect::<Vec<_>>(),
                    records::feature::thread::DesignThreadForm::Compact(_)
                    | records::feature::thread::DesignThreadForm::CompactLegacy => scope
                        .reference_members()
                        .values()
                        .step_by(2)
                        .copied()
                        .collect::<Vec<_>>(),
                };
                scope.reference_members().len() >= 2
                    && scope.reference_members().len().is_multiple_of(2)
                    && match construction.form {
                        records::feature::thread::DesignThreadForm::StandardLegacy => {
                            scope.class_tag.as_str() == "334"
                                && scope.paired_class_tag.as_str() == "262"
                        }
                        records::feature::thread::DesignThreadForm::CompactLegacy => {
                            scope.class_tag.as_str() == "414"
                                && scope.paired_class_tag.as_str() == "263"
                        }
                        records::feature::thread::DesignThreadForm::Standard
                        | records::feature::thread::DesignThreadForm::Compact(_) => true,
                    }
                    && construction.face_group_record_indices == expected_groups
                    && match construction.form {
                        records::feature::thread::DesignThreadForm::Compact(Some(reference)) => {
                            records_by_index.contains_key(&(native_stream, reference.value.get()))
                        }
                        records::feature::thread::DesignThreadForm::Compact(None)
                        | records::feature::thread::DesignThreadForm::Standard
                        | records::feature::thread::DesignThreadForm::StandardLegacy
                        | records::feature::thread::DesignThreadForm::CompactLegacy => true,
                    }
                    && construction
                        .face_group_record_indices
                        .iter()
                        .enumerate()
                        .all(|(group_ordinal, record_index)| {
                            let compact_member = if matches!(
                                construction.form,
                                records::feature::thread::DesignThreadForm::Compact(_)
                                    | records::feature::thread::DesignThreadForm::CompactLegacy
                            ) {
                                let Some(reference_ordinal) = group_ordinal.checked_mul(2) else {
                                    return false;
                                };
                                let Some(member_ordinal) = reference_ordinal.checked_add(1) else {
                                    return false;
                                };
                                let Some(member_record_index) =
                                    scope.reference_members().values().nth(member_ordinal)
                                else {
                                    return false;
                                };
                                let Ok(scope_reference_ordinal) = u32::try_from(reference_ordinal)
                                else {
                                    return false;
                                };
                                Some((scope_reference_ordinal, *member_record_index))
                            } else {
                                None
                            };
                            let mut groups = native
                                .design_construction_operand_groups
                                .iter()
                                .filter(|group| {
                                    design_stream(&group.id) == native_stream
                                        && group.scope_record_index == scope.record_index
                                        && group.record_index == *record_index
                                        && group.role() == DesignOperandRole::ROLE_0X10
                                        && compact_member.is_none_or(
                                            |(scope_reference_ordinal, member_record_index)| {
                                                group.scope_reference_ordinal
                                                    == scope_reference_ordinal
                                                    && group
                                                        .members()
                                                        .iter()
                                                        .map(|member| member.value)
                                                        .eq([member_record_index])
                                            },
                                        )
                                });
                            groups.next().is_some() && groups.next().is_none()
                        })
            }
        };
        let joint_origin_link = scope.joint_origin_frame().is_none_or(|origin| {
            let transform = origin.joint_origin_transform;
            let inline = match (joint_origin_frame_length(scope), &origin.reference) {
                (Some(385), None) => true,
                (Some(336 | 347), Some(reference)) => scope
                    .reference_members()
                    .values()
                    .any(|value| value == &reference.joint_origin_reference),
                _ => false,
            };
            let assembly_operand = origin.reference.is_none()
                && native.design_parameter_scopes.iter().any(|assembly| {
                    design_stream(&assembly.id) == native_stream
                        && assembly.kind()
                            == crate::records::feature::scope::DesignFeatureKind::Assemble
                        && assembly.assembly_alignment().is_some_and(|alignment| {
                            alignment.operand_frames().is_some_and(|frames| {
                                frames.iter().any(|frame| {
                                    frame.reference_record_index == scope.record_index
                                        && frame.transform == transform
                                })
                            })
                        })
                });
            let single_operand_assembly = origin.reference.as_ref().is_some_and(|reference| {
                native.design_parameter_scopes.iter().any(|assembly| {
                    design_stream(&assembly.id) == native_stream
                        && assembly.kind()
                            == crate::records::feature::scope::DesignFeatureKind::Assemble
                        && assembly.class_tag.as_str() == "276"
                        && assembly.paired_class_tag.as_str() == "258"
                        && assembly.assembly_frame()
                            == Some(
                                records::feature::scope::generation::DesignAssemblyFrame::Frame604,
                            )
                        && assembly
                            .reference_members()
                            .values()
                            .any(|value| value == &reference.joint_origin_reference)
                })
            });
            inline || assembly_operand || single_operand_assembly
        });
        let work_point_link = valid_work_point_construction(ctx, scope, native_stream)?;
        let work_plane_link = valid_work_plane_construction(ctx, scope, native_stream);
        let valid =
            scope
                .generation()
                .admits_classes(scope.class_tag.as_str(), scope.paired_class_tag.as_str())
                && scope.extrude_prologue().is_none_or(|prologue| {
                    valid_extrude_prologue(native, scope, native_stream, prologue)
                })
                && match &scope.payload() {
                    records::feature::scope::DesignScopePayload::SurfaceStitch(operation) => {
                        scope.reference_members().len() >= 4
                            && scope.reference_members().len().is_multiple_of(2)
                            && scope.reference_members().values().rev().nth(1)
                                == Some(&operation.tolerance_record_index)
                            && scope.reference_members().values().next_back()
                                == Some(&operation.settings_record_index)
                    }
                    _ => true,
                }
                && match &scope.payload() {
                    records::feature::scope::DesignScopePayload::SurfaceRuled(operation) => {
                        scope.reference_members().values().next()
                            == Some(&operation.distance_owner_record_index)
                            && scope.reference_members().values().nth(1)
                                == Some(&operation.angle_owner_record_index)
                            && operation.distance_owner_record_index
                                != operation.angle_owner_record_index
                            && !operation.edge_group_record_indices.is_empty()
                            && operation
                                .edge_group_record_indices
                                .iter()
                                .all(|record_index| {
                                    scope
                                        .reference_members()
                                        .values()
                                        .any(|value| value == record_index)
                                }) && match operation.method {
                            records::feature::surface_ops::DesignRuledSurfaceMethod::Direction => {
                                operation.direction_entity_id.is_some()
                            }
                            records::feature::surface_ops::DesignRuledSurfaceMethod::Normal
                            | records::feature::surface_ops::DesignRuledSurfaceMethod::Tangent => {
                                operation.direction_entity_id.is_none()
                            }
                        }
                    }
                    _ => true,
                }
                && scope.reference_members().values().all(|record_index| {
                    records_by_index.contains_key(&(native_stream, *record_index))
                })
                && records_by_index.contains_key(&(native_stream, scope.record_index))
                && entity_link.unwrap_or(true)
                && extrude_profile_link
                && sweep_profile_link
                && base_flange_profile_link
                && base_flange_link
                && edge_flange_link
                && hem_link
                && copy_paste_link
                && rectangular_pattern_link
                && assembly_alignment_link
                && component_insert_link
                && copy_paste_component_link
                && draft_link
                && combine_link
                && thread_link
                && joint_origin_link
                && work_point_link
                && work_plane_link
                && (scope.sketch_entity().is_none()
                    || placements_by_scope.contains_key(&(native_stream, scope.record_index)))
                && unique_index;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design parameter scope has an invalid paired frame",
                Some(scope.id.clone()),
            );
        }
    }
    Ok(())
}

/// The extent an `Extrude` prologue states agrees with its travel-direction
/// and per-side discriminators, in the table its scope generation selects.
fn valid_extrude_prologue(
    native: &native::F3dNative,
    scope: &records::feature::scope::DesignParameterScope,
    native_stream: &str,
    prologue: records::feature::extrude::DesignExtrudePrologue,
) -> bool {
    use records::feature::extrude::{DesignExtrudeExtent as Extent, DesignExtrudePrologue};
    use records::feature::scope::generation::DesignExtrudeFrame as Frame;
    let frame = scope.generation().extrude();
    match prologue {
        DesignExtrudePrologue::LegacyDistance { .. } => true,
        DesignExtrudePrologue::ShiftedReferenceAware {
            direction_face_extend_values,
            side_extent_discriminators,
            extent,
            ..
        } => {
            let expected = match frame {
                Some(Frame::ShiftedToFaces538) => {
                    Some((13_usize, [2, 1], [2, 0], Extent::TwoSidedToFaces))
                }
                Some(Frame::ShiftedToFaces516) => {
                    Some((11_usize, [2, 1], [2, 0], Extent::TwoSidedToFaces))
                }
                Some(Frame::ShiftedThroughAll485) => {
                    Some((10_usize, [3, 0], [4, 4], Extent::SymmetricThroughAll))
                }
                _ => None,
            };
            expected.is_some_and(
                |(reference_member_count, direction_face_extend, side_extents, expected_extent)| {
                    scope.reference_members().len() == reference_member_count
                        && direction_face_extend_values == direction_face_extend
                        && side_extent_discriminators == side_extents
                        && extent == expected_extent
                },
            )
        }
        DesignExtrudePrologue::ReferenceAware {
            reference,
            direction_face_extend_values,
            side_extent_discriminators,
            first_side_target_ordinal,
            extent,
            ..
        } => {
            let prefix_valid = reference.is_none_or(|reference| {
                matches!(reference.trailing_zero_count, 7 | 8)
                    && scope
                        .reference_members()
                        .values()
                        .any(|value| value == &reference.record_index)
            });
            let target_ordinal_valid = first_side_target_ordinal.is_none_or(|target| {
                usize::try_from(target.scope_reference_ordinal)
                    .ok()
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal).copied())
                    .is_some_and(|record_index| {
                        let mut groups =
                            native
                                .design_construction_operand_groups
                                .iter()
                                .filter(|group| {
                                    design_stream(&group.id) == native_stream
                                        && group.scope_record_index == scope.record_index
                                        && group.record_index == record_index
                                        && group.scope_reference_ordinal
                                            == target.scope_reference_ordinal
                                        && group.role() == DesignOperandRole::ROLE_0X5
                                        && group.extrude_role().is_none()
                                });
                        groups.next().is_some() && groups.next().is_none()
                    })
            });
            let legacy_class_415_extent = frame == Some(Frame::Class415SymmetricDistance)
                && direction_face_extend_values == [3, 2]
                && side_extent_discriminators == [1, 1]
                && extent == Extent::SymmetricDistance;
            prefix_valid
                && matches!(direction_face_extend_values[0], 1..=3)
                && (standard_extrude_extent(
                    direction_face_extend_values[0],
                    side_extent_discriminators,
                    extent,
                ) || legacy_class_415_extent)
                && first_side_target_ordinal.is_none_or(|_| side_extent_discriminators[0] == 2)
                && target_ordinal_valid
        }
        DesignExtrudePrologue::LegacyShifted {
            operation_prefix_marker,
            direction_face_extend_values,
            side_extent_discriminators,
            extent,
            ..
        } => match frame {
            Some(Frame::Class397SymmetricDistance) => {
                operation_prefix_marker.is_none()
                    && direction_face_extend_values
                        == [class_397::DIRECTION_VALUE, class_397::FACE_EXTEND_VALUE]
                    && side_extent_discriminators == [1, 1]
                    && extent == Some(Extent::SymmetricDistance)
            }
            Some(Frame::Class296OneSidedToFace) => {
                direction_face_extend_values[0] == 1
                    && matches!(direction_face_extend_values[1], 1 | 2)
                    && side_extent_discriminators == [2, 0]
                    && extent == Some(Extent::OneSidedToFace)
            }
            Some(Frame::Class296SymmetricDistance) => {
                direction_face_extend_values == [3, 2]
                    && side_extent_discriminators == [1, 0]
                    && extent == Some(Extent::SymmetricDistance)
            }
            Some(Frame::Class296TwoSidedToFaces) => {
                direction_face_extend_values[0] == 2
                    && matches!(direction_face_extend_values[1], 1 | 2)
                    && side_extent_discriminators == [2, 0]
                    && extent == Some(Extent::TwoSidedToFaces)
            }
            Some(Frame::Class296LegacyOneSidedToFace) => {
                direction_face_extend_values == [1, 1]
                    && side_extent_discriminators == [2, 0]
                    && extent == Some(Extent::OneSidedToFace)
            }
            Some(Frame::Class296LegacyOneSidedDistance) => {
                direction_face_extend_values == [1, 2]
                    && side_extent_discriminators == [1, 0]
                    && extent == Some(Extent::OneSidedDistance)
            }
            // The general envelope selects its compact or field-shifted
            // layout by its frame offsets: its extent is one of either table.
            _ => {
                let compact = matches!(
                    (
                        direction_face_extend_values,
                        side_extent_discriminators,
                        extent
                    ),
                    ([1, _], [1, 0], Some(Extent::OneSidedDistance))
                        | ([3, _], [1, 0], Some(Extent::SymmetricDistance))
                        | ([2, 0], [1, 2], Some(Extent::TwoSidedDistanceToFace))
                );
                let standard = matches!(direction_face_extend_values[0], 1..=3)
                    && extent.is_some_and(|extent| {
                        extent != Extent::TwoSidedToFaces
                            && standard_extrude_extent(
                                direction_face_extend_values[0],
                                side_extent_discriminators,
                                extent,
                            )
                    });
                compact || standard
            }
        },
    }
}

/// The extents of the standard `Extrude` prologue table, by travel direction
/// and per-side discriminators.
fn standard_extrude_extent(
    direction: u32,
    side_extent_discriminators: [u32; 2],
    extent: records::feature::extrude::DesignExtrudeExtent,
) -> bool {
    use records::feature::extrude::DesignExtrudeExtent as Extent;
    matches!(
        (direction, side_extent_discriminators, extent),
        (1, [1, 0], Extent::OneSidedDistance)
            | (1, [2, 0], Extent::OneSidedToFace)
            | (1, [3, 0], Extent::OneSidedThroughNext)
            | (1, [4, 0], Extent::OneSidedThroughAll)
            | (2, [2, 0], Extent::TwoSidedToFaces)
            | (2, [1, 1], Extent::TwoSidedDistance)
            | (3, [1, 0], Extent::SymmetricDistance)
            | (3, [4, 4], Extent::SymmetricThroughAll)
    )
}

fn valid_work_point_construction(
    ctx: &Ctx,
    scope: &records::feature::scope::DesignParameterScope,
    native_stream: &str,
) -> Result<bool, CodecError> {
    let Some(construction) = scope.work_point_construction() else {
        return Ok(true);
    };
    let native = ctx.native;
    if !scope
        .reference_members()
        .values()
        .any(|value| value == &construction.point_record_index)
        || !ctx
            .records_by_index
            .contains_key(&(native_stream, construction.point_record_index))
    {
        return Ok(false);
    }

    construction
        .rule
        .inputs()
        .iter()
        .try_fold(true, |valid, input| {
            if !valid {
                return Ok(false);
            }
            let header = ctx
                .records_by_index
                .get(&(native_stream, input.record_index()));
            Ok(scope
                .reference_members()
                .values()
                .any(|value| value == &input.record_index())
                && header.is_some() && match input.carrier() {
                None => true,
                Some(
                    records::feature::work_geometry::DesignWorkPointInputCarrier::EdgeRecipe {
                        operand_id,
                    },
                ) => native.design_edge_operands.iter().any(|operand| {
                    operand.id == *operand_id
                        && design_stream(&operand.id) == native_stream
                        && operand.scope_record_index == scope.record_index
                        && operand.record_index() == input.record_index()
                }),
                Some(
                    records::feature::work_geometry::DesignWorkPointInputCarrier::VertexRecipe {
                        recipe: vertex,
                    },
                ) => valid_vertex_recipe(ctx, scope, native_stream, input.record_index(), vertex),
                Some(records::feature::work_geometry::DesignWorkPointInputCarrier::WorkPlane {
                    selection,
                }) => {
                    header.is_some_and(|header| header.class_tag == selection.class_tag)
                        && u32::try_from(selection.primary_identity)
                            .ok()
                            .and_then(|identity| identity.checked_add(1))
                            == Some(selection.work_plane_scope_record_index)
                        && native.design_parameter_scopes.iter().any(|plane| {
                            design_stream(&plane.id) == native_stream
                                && plane.kind()
                                    == crate::records::feature::scope::DesignFeatureKind::WorkPlane
                                && plane.record_index == selection.work_plane_scope_record_index
                        })
                }
                Some(
                    records::feature::work_geometry::DesignWorkPointInputCarrier::SketchPoint {
                        selection,
                    },
                ) => {
                    header.is_some_and(|header| header.class_tag == selection.class_tag)
                        && u32::try_from(selection.point_persistent_id).is_ok()
                        && !selection.point_native_id.trim().is_empty()
                        && native.sketch_points.iter().any(|point| {
                            point.id == selection.point_native_id
                                && design_stream(&point.id) == native_stream
                                && point.owner_reference == Some(selection.sketch_record_index)
                                && point.persistent_id() == Some(selection.point_persistent_id)
                        })
                }
            })
        })
}

fn valid_work_plane_construction(
    ctx: &Ctx,
    scope: &records::feature::scope::DesignParameterScope,
    native_stream: &str,
) -> bool {
    let Some(frame) = scope.work_plane_frame() else {
        return true;
    };
    let Some(construction) = &frame.work_plane_construction else {
        return true;
    };
    let placement_record_index = &construction.placement_record_index;
    let inputs = construction.inputs();
    let Some([placement, first, second, third, extra_offset]) =
        scope.reference_members().values_array()
    else {
        return false;
    };
    if !ctx
        .records_by_index
        .contains_key(&(native_stream, *placement_record_index))
    {
        return false;
    }
    let Some(owner) = ctx.native.design_parameter_owners.iter().find(|owner| {
        design_stream(owner.id()) == native_stream
            && owner.record_index() == *extra_offset
            && owner.scope_record_index() == Some(scope.record_index)
            && owner.evaluated_value().get() == 0.0
    }) else {
        return false;
    };

    placement == placement_record_index
        && [
            inputs[0].record_index(),
            inputs[1].record_index(),
            inputs[2].record_index(),
        ] == [*first, *second, *third]
        && scope.work_plane_reference() == Some(*extra_offset)
        && scope
            .work_plane_frame()
            .and_then(|frame| frame.reference.as_ref())
            .is_some()
        && inputs.iter().all(|input| {
            valid_vertex_recipe(ctx, scope, native_stream, input.record_index(), input)
        })
        && ctx.native.design_parameters.iter().any(|parameter| {
            design_stream(&parameter.id) == native_stream
                && parameter.record_index == owner.parameter_record_index()
                && parameter.owner_record_index() == Some(owner.record_index())
                && parameter.source_kind() == "ExtraOffset"
                && parameter.evaluated_value().get() == 0.0
        })
}

fn valid_vertex_recipe(
    ctx: &Ctx,
    scope: &records::feature::scope::DesignParameterScope,
    native_stream: &str,
    record_index: u32,
    vertex: &records::feature::work_geometry::DesignVertexRecipe,
) -> bool {
    let native = ctx.native;
    let header = ctx.records_by_index.get(&(native_stream, record_index));
    let recipe = ctx.recipes_by_id.get(vertex.recipe_id.as_str());
    let mut expected_references =
        design::decode::dimension_frames::decode_recipe_references(&vertex.recipe_prefix_bytes, 0);
    for reference in &mut expected_references {
        reference.selector_offset = None;
        reference.token_offset = None;
        reference.design_reference_offset = None;
        design::decode::dimension_frames::bind_recipe_reference_candidates(
            reference,
            &native.persistent_subentity_tags,
            Some(&scope.id),
        );
    }
    let program_byte_length = u64::try_from(vertex.recipe_program.len())
        .ok()
        .and_then(|length| length.checked_mul(4));
    let resolution_is_valid = match vertex.resolution {
        None => true,
        Some(resolution) => {
            let state_id = resolution.state_id;
            let vertex_slot = resolution.vertex_slot();
            let mut states = native
                .asm_histories
                .iter()
                .flat_map(|history| history.states().iter().map(move |state| (history, state)))
                .filter(|(_, state)| state.state_id == state_id);
            states.next().is_some_and(|(history, state)| {
                states.next().is_none()
                    && history.topology(state).map_or_else(
                        || history::projection_was_finalized(&native.asm_histories),
                        |topology| topology.vertices.contains(&vertex_slot),
                    )
            })
        }
    };
    vertex.record_index() == record_index
        && header.is_some_and(|header| header.class_tag == vertex.class_tag)
        && vertex
            .recipe_references
            .iter()
            .map(unlocated_recipe_reference)
            .eq(expected_references)
        && resolution_is_valid
        && recipe.is_some_and(|recipe| {
            design_stream(&recipe.id) == native_stream
                && recipe.kind == records::recipes::ConstructionRecipeKind::Vertex
        })
        && program_byte_length.is_some_and(|program_byte_length| program_byte_length != 0)
}

/// A recipe reference without its source offsets.
fn unlocated_recipe_reference(
    reference: &records::dimensions::DesignRecipeReference,
) -> records::dimensions::DesignRecipeReference {
    let mut reference = reference.clone();
    reference.selector_offset = None;
    reference.token_offset = None;
    reference.design_reference_offset = None;
    reference
}

fn validate_component_occurrences(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    struct LowerAscii<'a>(&'a str);
    impl std::fmt::Display for LowerAscii<'_> {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            for character in self.0.chars() {
                write!(formatter, "{}", character.to_ascii_lowercase())?;
            }
            Ok(())
        }
    }
    let mut identities = HashSet::new();
    let mut record_indices = HashSet::new();
    for occurrence in &ctx.native.design_component_occurrences {
        let stream = design_stream(&occurrence.id);
        let key = format!("{}", LowerAscii(occurrence.occurrence_guid.as_str()));
        let unique_identity = identities.insert((stream, key));
        let unique_record = record_indices.insert((stream, occurrence.record_index));
        // A class-256 carrier stores a placed frame only after its first
        // occurrence; every other carrier class can place ordinal one.
        let placement_admitted = match occurrence.placement() {
            records::feature::assembly_features::DesignComponentOccurrencePlacement::Base
            | records::feature::assembly_features::DesignComponentOccurrencePlacement::Identity {
                ..
            } => true,
            records::feature::assembly_features::DesignComponentOccurrencePlacement::Explicit {
                ordinal,
                ..
            } => occurrence.class_tag.as_str() != "256" || ordinal.get() > 1,
        };
        // The duplicated references must agree within one carrier, which
        // the decoder checks. The component-record reference names the
        // definition; carriers of one definition can store different
        // carrier GUIDs.
        for (holds, message) in [
            (
                unique_identity,
                "Fusion Design component occurrence GUID repeats in its Design stream",
            ),
            (
                unique_record,
                "Fusion Design component occurrence record index repeats in its Design stream",
            ),
            (
                placement_admitted,
                "Fusion Design class-256 component occurrence has a placed frame at ordinal one",
            ),
        ] {
            if !holds {
                push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    message,
                    Some(occurrence.id.clone()),
                );
            }
        }
    }
}

fn valid_component_pattern_occurrences(
    native: &native::F3dNative,
    stream: &str,
    instances: &records::feature::patterns::DesignRectangularPatternInstances,
) -> bool {
    let records::feature::patterns::DesignRectangularPatternInstances::Components {
        component_guid,
        seed,
        generated,
    } = instances
    else {
        return true;
    };
    native
        .design_component_occurrences
        .iter()
        .any(|occurrence| {
            design_stream(&occurrence.id) == stream
                && occurrence
                    .component_guid
                    .as_str()
                    .eq_ignore_ascii_case(component_guid.as_str())
                && occurrence
                    .occurrence_guid
                    .as_str()
                    .eq_ignore_ascii_case(seed.occurrence_guid.as_str())
                && matches!(
                    occurrence.placement(),
                    crate::records::feature::assembly_features::DesignComponentOccurrencePlacement::Base
                )
        })
        && generated.iter().enumerate().all(|(ordinal, row)| {
            native
                .design_component_occurrences
                .iter()
                .any(|occurrence| {
                    design_stream(&occurrence.id) == stream
                        && occurrence
                            .component_guid
                            .as_str()
                            .eq_ignore_ascii_case(component_guid.as_str())
                        && occurrence
                            .occurrence_guid
                            .as_str()
                            .eq_ignore_ascii_case(row.occurrence_guid.as_str())
                        && u32::try_from(ordinal).ok().and_then(|ordinal| ordinal.checked_add(2)) == Some(occurrence.occurrence_ordinal())
                        && occurrence.transform().map(|transform| transform.value)
                            == Some(row.instance.transform.value)
                })
        })
}

/// Validate Extrude selection groups and their counted member frames.
fn validate_extrude_selection_groups(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let mut group_slots = HashSet::new();
    for group in &native.design_extrude_selection_groups {
        let native_stream = design_stream(&group.id);
        let scope = scopes_by_index.get(&(native_stream, group.scope_record_index));
        let header = records_by_index.get(&(native_stream, group.record_index));
        let frame_valid = scope.is_some_and(|scope| {
            design::design_feature_family(&scope.kind())
                == Some(design::DesignFeatureFamily::Extrude)
                && usize::try_from(group.scope_reference_ordinal)
                    .ok()
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                    == Some(&group.record_index)
        }) && header.is_some_and(|header| {
            source_positions_agree(header.byte_offset, Some(group.byte_offset()))
                && header.class_tag == group.class_tag
        }) && group
            .members()
            .iter()
            .all(|member| records_by_index.contains_key(&(native_stream, member.value)));
        let valid = if frame_valid {
            group_slots.insert((
                native_stream,
                group.scope_record_index,
                group.scope_reference_ordinal,
            ))
        } else {
            false
        };
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Extrude selection group has an invalid counted frame",
                Some(group.id.clone()),
            );
        }
    }
}

/// Compare source positions only when actual scanner layout is present.
fn optional_source_offset_matches(base: Option<u64>, relative: u64, actual: Option<u64>) -> bool {
    match (base, actual) {
        (Some(base), Some(actual)) => base.checked_add(relative) == Some(actual),
        (None, None) => true,
        _ => false,
    }
}

/// Cross-record source layout is comparable only when both scanners held it.
fn source_positions_agree(left: Option<u64>, right: Option<u64>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left == right,
        _ => true,
    }
}
fn source_position_before(left: Option<u64>, right: Option<u64>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left < right,
        _ => true,
    }
}

/// Validate construction operand groups and their role discriminators.
fn validate_construction_operand_groups(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let mut operand_group_slots = HashSet::new();
    for group in &native.design_construction_operand_groups {
        let native_stream = design_stream(&group.id);
        let scope = scopes_by_index.get(&(native_stream, group.scope_record_index));
        let header = records_by_index.get(&(native_stream, group.record_index));
        let frame = &group.frame;
        let layout_valid = match (
            group.byte_offset,
            frame.member_count_offset,
            group.role_offset(),
            frame.opaque_scalar_offset(),
            group.paired_byte_offset,
        ) {
            (Some(start), Some(count), Some(role), Some(scalar), Some(paired)) => {
                let relative = if scope.is_some_and(|scope| {
                    scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfaceStitch
                        || (scope.kind()
                            == crate::records::feature::scope::DesignFeatureKind::SplitFace
                            && group.role() == DesignOperandRole::ROLE_0X21)
                        || (scope.kind()
                            == crate::records::feature::scope::DesignFeatureKind::Split
                            && matches!(
                                group.role(),
                                DesignOperandRole::ROLE_0X9 | DesignOperandRole::ROLE_0X21
                            ))
                }) {
                    88
                } else {
                    21
                };
                let member_run_end = group.members().last().map_or_else(
                    || count.checked_add(4),
                    |member| member.offset.and_then(|offset| offset.checked_add(10)),
                );
                start.checked_add(relative) == Some(count)
                    && group
                        .members()
                        .first()
                        .is_none_or(|member| count.checked_add(5) == member.offset)
                    && frame
                        .trailing_records()
                        .first()
                        .is_none_or(|record| role.checked_sub(10) == record.offset)
                    && member_run_end.is_some_and(|end| role >= end)
                    && scalar.checked_add(8).is_some_and(|end| paired > end)
            }
            (None, None, None, None, None) => {
                group.members().iter().all(|member| member.offset.is_none())
                    && frame
                        .auxiliary_records
                        .iter()
                        .all(|record| record.offset.is_none())
                    && frame
                        .trailing_records()
                        .iter()
                        .all(|record| record.offset.is_none())
            }
            _ => false,
        };
        let frame_valid = layout_valid
            && group.role().raw().trailing_zeros() >= 32
            && frame
                .auxiliary_records
                .iter()
                .map(|record| &record.value)
                .chain(frame.trailing_records().iter().map(|record| &record.value))
                .all(|record_index| records_by_index.contains_key(&(native_stream, *record_index)))
            && frame.trailing_transforms().iter().all(|transform| {
                frame
                    .trailing_records()
                    .iter()
                    .any(|record| record.value == transform.record_index())
                    && records_by_index
                        .get(&(native_stream, transform.record_index()))
                        .is_some_and(|header| {
                            transform.byte_offset().is_none_or(|offset| {
                                source_positions_agree(header.byte_offset, Some(offset))
                            }) && header.class_tag == transform.class_tag
                        })
                    && records_by_index
                        .get(&(native_stream, transform.following_record_index()))
                        .is_some_and(|header| {
                            transform.following_byte_offset().is_none_or(|offset| {
                                source_positions_agree(header.byte_offset, Some(offset))
                            }) && header.class_tag == transform.following_class_tag
                        })
            })
            && frame.trailing_dual_transforms().iter().all(|transform| {
                frame
                    .trailing_records()
                    .iter()
                    .any(|record| record.value == transform.record_index)
                    && records_by_index
                        .get(&(native_stream, transform.record_index))
                        .is_some_and(|header| {
                            transform.byte_offset.is_none_or(|offset| {
                                source_positions_agree(header.byte_offset, Some(offset))
                            }) && header.class_tag == transform.class_tag
                        })
                    && optional_source_offset_matches(
                        transform.byte_offset,
                        21,
                        transform.first_transform_offset,
                    )
                    && optional_source_offset_matches(
                        transform.byte_offset,
                        149,
                        transform.second_transform_offset,
                    )
            })
            && frame.trailing_flags().iter().all(|flag| {
                frame
                    .trailing_records()
                    .iter()
                    .any(|record| record.value == flag.record_index)
                    && records_by_index
                        .get(&(native_stream, flag.record_index))
                        .is_some_and(|header| {
                            flag.byte_offset.is_none_or(|offset| {
                                source_positions_agree(header.byte_offset, Some(offset))
                            }) && header.class_tag == flag.class_tag
                        })
                    && optional_source_offset_matches(flag.byte_offset, 22, flag.value_offset)
            })
            && frame.auxiliary_paths().iter().all(|path| {
                frame
                    .auxiliary_records
                    .iter()
                    .any(|record| record.value == path.record_index())
                    && records_by_index
                        .get(&(native_stream, path.record_index()))
                        .is_some_and(|header| {
                            path.byte_offset().is_none_or(|offset| {
                                source_positions_agree(header.byte_offset, Some(offset))
                            }) && header.class_tag == path.class_tag
                        })
                    && path.scope_record_index == group.scope_record_index
                    && records_by_index.contains_key(&(native_stream, path.nested_record_index()))
                    && records_by_index
                        .get(&(native_stream, path.following_record_index()))
                        .is_some_and(|header| {
                            path.following_byte_offset().is_none_or(|offset| {
                                source_positions_agree(header.byte_offset, Some(offset))
                            }) && header.class_tag == path.following_class_tag
                        })
            });
        let valid = scope.is_some_and(|scope| {
            let role_is_valid = match design::design_feature_family(&scope.kind()) {
                Some(design::DesignFeatureFamily::Extrude) => {
                    match group.operand_role {
                        records::topology::construction::DesignConstructionOperandRole::ExtrudeBodiesA
                        | records::topology::construction::DesignConstructionOperandRole::ExtrudeBodiesB => true,
                        records::topology::construction::DesignConstructionOperandRole::ExtrudeProfile => {
                            scope.extrude_profile().is_none_or(|profile| {
                                group.members().first().map(|member| &member.value)
                                    == Some(&profile.record_index)
                            })
                        }
                        records::topology::construction::DesignConstructionOperandRole::ExtrudeFaces {
                            encoding,
                            ..
                        } => match encoding {
                            records::topology::extrude_selection::DesignExtrudeFaceEncoding::Faces => true,
                            records::topology::extrude_selection::DesignExtrudeFaceEncoding::LegacyTermination => {
                                scope.extrude_prologue().and_then(
                                    records::feature::extrude::DesignExtrudePrologue::extent,
                                ) == Some(
                                    records::feature::extrude::DesignExtrudeExtent::OneSidedToFace,
                                ) || is_class_296_two_sided_to_faces_scope(scope)
                            }
                            records::topology::extrude_selection::DesignExtrudeFaceEncoding::SelectedStart => {
                                scope
                                    .extrude_prologue()
                                    .map(records::feature::extrude::DesignExtrudePrologue::start)
                                    == Some(records::feature::extrude::DesignExtrudeStart::FromFace)
                            }
                        },
                        records::topology::construction::DesignConstructionOperandRole::Other(role) => {
                            role == DesignOperandRole::ROLE_0X5
                        }
                    }
                }
                Some(
                    design::DesignFeatureFamily::Fillet | design::DesignFeatureFamily::Chamfer,
                ) => group.extrude_role().is_none(),
                Some(design::DesignFeatureFamily::Coil) => {
                    group.role()
                        == if scope.kind()
                            == crate::records::feature::scope::DesignFeatureKind::CoilPrimitive
                            && scope.reference_members().len() == 10
                            && scope.coil_long_form()
                        {
                            DesignOperandRole::BODIES_A
                        } else {
                            DesignOperandRole::BODIES_B
                        }
                        && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Move) => {
                    group.role() == DesignOperandRole::BODIES_A && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::OffsetFaces) => {
                    group.role() == DesignOperandRole::ROLE_0X10 && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Draft) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::ROLE_0X10 | DesignOperandRole::ROLE_0X21
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::ReplaceFace) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::ROLE_0X9 | DesignOperandRole::ROLE_0X10
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Revolve) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A
                            | DesignOperandRole::BODIES_B
                            | DesignOperandRole::ROLE_0X21
                            | DesignOperandRole::PROFILE
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Shell) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A | DesignOperandRole::ROLE_0X10
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Thicken) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::ROLE_0X5 | DesignOperandRole::ROLE_0X12
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Loft) => {
                    (!scope.has_path_construction()
                        || matches!(
                            group.role(),
                            DesignOperandRole::BODIES_A
                                | DesignOperandRole::ROLE_0X5
                                | DesignOperandRole::PROFILE
                                | DesignOperandRole::ROLE_0X43
                                | DesignOperandRole::ROLE_0X7
                        ))
                        && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Sweep) => {
                    (!scope.has_path_construction()
                        || matches!(
                            group.role(),
                            DesignOperandRole::BODIES_A
                                | DesignOperandRole::ROLE_0X5
                                | DesignOperandRole::FACES
                                | DesignOperandRole::PROFILE
                        ))
                        && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Pipe) => {
                    group.role() == DesignOperandRole::ROLE_0X5 && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::CircularPattern) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::RectangularPattern) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Mirror) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A
                            | DesignOperandRole::ROLE_0X5
                            | DesignOperandRole::BODIES_B
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SurfacePatch) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A | DesignOperandRole::PROFILE
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SurfaceOffset) => {
                    group.role() == DesignOperandRole::PROFILE && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SurfaceRuled) => {
                    group.role() == DesignOperandRole::BODIES_B
                        && group.extrude_role().is_none()
                        && scope.ruled_surface_operation().is_some_and(|operation| {
                            operation
                                .edge_group_record_indices
                                .contains(&group.record_index)
                        })
                }
                Some(design::DesignFeatureFamily::BoundaryFill) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A | DesignOperandRole::ROLE_0X5
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Hole) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A
                            | DesignOperandRole::BODIES_B
                            | DesignOperandRole::ROLE_0X5
                            | DesignOperandRole::ROLE_0X21
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SurfaceTrim) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A | DesignOperandRole::ROLE_0X21
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Split) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A
                            | DesignOperandRole::ROLE_0X9
                            | DesignOperandRole::ROLE_0X21
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Scale) => {
                    group.role() == DesignOperandRole::BODIES_A && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Thread) => {
                    group.role() == DesignOperandRole::ROLE_0X10
                        && group.extrude_role().is_none()
                        && scope.thread_construction().is_some_and(|construction| {
                            construction
                                .face_group_record_indices
                                .contains(&group.record_index)
                        })
                }
                Some(design::DesignFeatureFamily::SheetMetalEdgeFlange) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_B
                            | DesignOperandRole::ROLE_0X21
                            | DesignOperandRole::ROLE_0X43
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SheetMetalHem) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_B | DesignOperandRole::ROLE_0X43
                    ) && group.extrude_role().is_none()
                }
                Some(_) => false,
                None if scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::RemoveBody =>
                {
                    group.role() == DesignOperandRole::BODIES_A && group.extrude_role().is_none()
                }
                None if scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::SurfaceStitch =>
                {
                    group.role() == DesignOperandRole::ROLE_0X5 && group.extrude_role().is_none()
                }
                None if scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::SplitFace =>
                {
                    matches!(
                        group.role(),
                        DesignOperandRole::ROLE_0X10 | DesignOperandRole::ROLE_0X21
                    ) && group.extrude_role().is_none()
                }
                None if matches!(
                    scope.kind(),
                    crate::records::feature::scope::DesignFeatureKind::DeleteFace
                        | crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace
                ) =>
                {
                    group.role() == DesignOperandRole::ROLE_0X10 && group.extrude_role().is_none()
                }
                None if scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::Decal =>
                {
                    group.role() == DesignOperandRole::BODIES_A && group.extrude_role().is_none()
                }
                None if scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::BaseFlange =>
                {
                    group.role() == DesignOperandRole::PROFILE
                        && group.extrude_role().is_none()
                        && scope.base_flange_profile().as_ref().is_some_and(|profile| {
                            group
                                .members()
                                .iter()
                                .map(|member| member.value)
                                .eq([profile.record_index])
                        })
                }
                None if scope.kind() == crate::records::feature::scope::DesignFeatureKind::Hem => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_B | DesignOperandRole::ROLE_0X43
                    ) && group.extrude_role().is_none()
                }
                None => false,
            };
            (design::design_feature_family(&scope.kind()).is_some()
                || matches!(
                    scope.kind(),
                    crate::records::feature::scope::DesignFeatureKind::RemoveBody
                        | crate::records::feature::scope::DesignFeatureKind::SurfaceStitch
                        | crate::records::feature::scope::DesignFeatureKind::SplitFace
                        | crate::records::feature::scope::DesignFeatureKind::DeleteFace
                        | crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace
                        | crate::records::feature::scope::DesignFeatureKind::Decal
                        | crate::records::feature::scope::DesignFeatureKind::BaseFlange
                        | crate::records::feature::scope::DesignFeatureKind::EdgeFlange
                        | crate::records::feature::scope::DesignFeatureKind::Hem
                ))
                && role_is_valid
                && usize::try_from(group.scope_reference_ordinal)
                    .ok()
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                    == Some(&group.record_index)
                && group
                    .members()
                    .iter()
                    .map(|member| &member.value)
                    .all(|member| {
                        scope
                            .reference_members()
                            .values()
                            .any(|value| value == member)
                    })
        }) && header.is_some_and(|header| {
            group.byte_offset.is_none_or(|offset| source_positions_agree(header.byte_offset, Some(offset))) && header.class_tag == group.class_tag
        }) && frame_valid
            && !group.members().is_empty()
            && {
                let mut seen = HashSet::new();
                for member in group.members() {
                    seen.insert(member.value);
                }
                seen.len() == group.members().len()
            }
            && group
                .members()
                .iter()
                .map(|member| &member.value)
                .all(|member| records_by_index.contains_key(&(native_stream, *member)))
            && operand_group_slots.insert((native_stream, group.scope_record_index, group.scope_reference_ordinal));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design construction operand group has an invalid frame",
                Some(group.id.clone()),
            );
        }
    }
}

fn pipe_operand_roles_are_valid(
    operation: records::feature::extrude::DesignExtrudeOperation,
    groups: &[(DesignOperandRole, usize)],
) -> bool {
    let expected_bodies =
        usize::from(operation != records::feature::extrude::DesignExtrudeOperation::NewBody);
    groups.len() == 1 + expected_bodies
        && groups.iter().all(|(_, members)| *members > 0)
        && groups
            .iter()
            .filter(|(role, _)| *role == DesignOperandRole::ROLE_0X5)
            .count()
            == 1
        && groups
            .iter()
            .filter(|(role, _)| {
                matches!(
                    *role,
                    DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B
                )
            })
            .count()
            == expected_bodies
}

/// Validate path-feature operand roles against the scope construction.
///
/// The role grammar is shared with the fixed Loft projector. Keeping the
/// predicate independent of native byte offsets lets validation reject a
/// malformed role combination without rejecting a valid section/guide mix.
pub(crate) fn loft_operand_roles_are_valid(
    operation: records::feature::extrude::DesignExtrudeOperation,
    groups: &[(DesignOperandRole, usize)],
) -> bool {
    const BODY: DesignOperandRole = DesignOperandRole::BODIES_A;
    const SECTION: DesignOperandRole = DesignOperandRole::PROFILE;
    const FACE_SECTION: DesignOperandRole = DesignOperandRole::ROLE_0X43;
    const GUIDE: DesignOperandRole = DesignOperandRole::ROLE_0X5;
    const CENTERLINE: DesignOperandRole = DesignOperandRole::ROLE_0X7;

    let body_count = groups.iter().filter(|(role, _)| *role == BODY).count();
    let expected_body_count =
        usize::from(operation != records::feature::extrude::DesignExtrudeOperation::NewBody);
    if body_count != expected_body_count {
        return false;
    }
    let operands = || groups.iter().filter(|(role, _)| *role != BODY);
    let section_count = operands()
        .filter(|(role, _)| matches!(*role, SECTION | FACE_SECTION))
        .count();
    let guide_count = operands().filter(|(role, _)| *role == GUIDE).count();
    let centerline_count = operands().filter(|(role, _)| *role == CENTERLINE).count();

    if section_count >= 2 {
        let roles_are_known = operands()
            .all(|(role, _)| matches!(*role, SECTION | FACE_SECTION | GUIDE | CENTERLINE));
        return roles_are_known
            && centerline_count <= 1
            && !(guide_count > 0 && centerline_count > 0)
            && operands().count() == section_count + guide_count + centerline_count;
    }

    if operation != records::feature::extrude::DesignExtrudeOperation::NewBody {
        return false;
    }

    if section_count == 1 && operands().all(|(role, _)| matches!(*role, FACE_SECTION | GUIDE)) {
        let mut point_ordinals = operands()
            .enumerate()
            .filter(|(_, (role, member_count))| *role == GUIDE && *member_count == 1)
            .map(|(ordinal, _)| ordinal);
        let Some(point_ordinal) = point_ordinals.next() else {
            return false;
        };
        return point_ordinals.next().is_none()
            && (point_ordinal == 0 || point_ordinal + 1 == operands().count())
            && operands()
                .enumerate()
                .all(|(ordinal, (role, member_count))| {
                    ordinal == point_ordinal || *role != GUIDE || *member_count != 1
                });
    }

    if section_count == 0 && operands().count() >= 2 {
        let all_sections = operands().all(|(role, _)| *role == SECTION);
        let all_guides = operands().all(|(role, _)| *role == GUIDE);
        return all_sections || all_guides;
    }

    false
}

/// Validate the operand-group roles of path-feature scopes. The groups are
/// design-intent records, so a document without them has nothing to check.
fn validate_path_feature_operand_roles(ctx: &Ctx, findings: &mut Vec<Finding>) {
    if !ctx.holds_design_intent() {
        return;
    }
    let native = ctx.native;
    for scope in native
        .design_parameter_scopes
        .iter()
        .filter(|scope| scope.has_path_construction())
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
        let role_count = |role| groups.iter().filter(|group| group.role() == role).count();
        let group_roles = groups
            .iter()
            .map(|group| (group.role(), group.members().len()))
            .collect::<Vec<_>>();
        let valid = match &scope.payload() {
            records::feature::scope::DesignScopePayload::Revolve(Some(
                crate::records::feature::path_features::DesignRevolveConstruction {
                    operation,
                    angle_record_index,
                    opposite_angle,
                    ..
                },
            )) => {
                let body_count = role_count(DesignOperandRole::BODIES_A)
                    + role_count(DesignOperandRole::BODIES_B);
                let expected_body_count = usize::from(
                    *operation != records::feature::extrude::DesignExtrudeOperation::NewBody,
                );
                scope
                    .reference_members()
                    .values()
                    .any(|value| value == angle_record_index)
                    && opposite_angle.is_none_or(|located| {
                        scope
                            .reference_members()
                            .values()
                            .any(|value| value == &located.value)
                    })
                    && groups.len() == 2 + expected_body_count
                    && role_count(DesignOperandRole::ROLE_0X21) == 1
                    && role_count(DesignOperandRole::PROFILE) == 1
                    && body_count == expected_body_count
            }
            records::feature::scope::DesignScopePayload::Loft(Some(
                crate::records::feature::path_features::DesignLoftConstruction {
                    operation, ..
                },
            )) => loft_operand_roles_are_valid(*operation, &group_roles),
            records::feature::scope::DesignScopePayload::Sweep(Some(
                records::feature::scope::DesignSweepScope {
                    construction:
                        Some(records::feature::path_features::DesignSweepConstruction {
                            operation, ..
                        }),
                    ..
                },
            )) => {
                let path_count = role_count(DesignOperandRole::ROLE_0X5);
                let profile_count = role_count(DesignOperandRole::PROFILE);
                let guide_surface_count = role_count(DesignOperandRole::FACES);
                let guide_profile_frame = scope.sweep_profile().is_some_and(|profile| {
                    let profile_groups = || {
                        groups
                            .iter()
                            .filter(|group| group.role() == DesignOperandRole::PROFILE)
                    };
                    profile_groups()
                        .filter(|group| {
                            group
                                .members()
                                .iter()
                                .map(|member| member.value)
                                .eq([profile.record_index])
                        })
                        .count()
                        == 1
                        && profile_groups()
                            .filter(|group| {
                                !group
                                    .members()
                                    .iter()
                                    .map(|member| member.value)
                                    .eq([profile.record_index])
                            })
                            .filter(|group| {
                                !group.members().is_empty()
                                    && group.members().iter().map(|member| &member.value).all(
                                        |member| {
                                            native.design_entity_selection_operands.iter().any(
                                                |operand| {
                                                    design_stream(&operand.id) == native_stream
                                                        && operand.scope_record_index
                                                            == scope.record_index
                                                        && operand.group_record_index
                                                            == group.record_index
                                                        && operand.record_index() == *member
                                                },
                                            )
                                        },
                                    )
                            })
                            .count()
                            == 1
                });
                let common_roles =
                    (profile_count == 1 && guide_surface_count == 0 && matches!(path_count, 1 | 2))
                        || (profile_count == 2
                            && guide_surface_count == 1
                            && path_count == 1
                            && guide_profile_frame);
                common_roles
                    && match operation {
                        records::feature::extrude::DesignExtrudeOperation::NewBody => {
                            groups.len() == path_count + profile_count + guide_surface_count
                                && role_count(DesignOperandRole::BODIES_A) == 0
                        }
                        records::feature::extrude::DesignExtrudeOperation::Join
                        | records::feature::extrude::DesignExtrudeOperation::Cut
                        | records::feature::extrude::DesignExtrudeOperation::Intersect => {
                            guide_surface_count == 0
                                && groups.len() == path_count + 2
                                && role_count(DesignOperandRole::BODIES_A) == 1
                        }
                    }
            }
            records::feature::scope::DesignScopePayload::Pipe(Some(
                crate::records::feature::path_features::DesignPipeConstruction {
                    operation, ..
                },
            )) => pipe_operand_roles_are_valid(*operation, &group_roles),
            _ => false,
        };
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design path-feature operand roles conflict with its construction",
                Some(scope.id.clone()),
            );
        }
    }
}

/// Validate Extrude profile, operation, start, and extent operand agreement.
/// The body operands are design-intent records, so the operation is checked
/// against them only in a document that holds them.
fn validate_extrude_parameter_operands(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    for scope in native.design_parameter_scopes.iter().filter(|scope| {
        matches!(
            design::design_feature_family(&scope.kind()),
            Some(
                design::DesignFeatureFamily::Extrude
                    | design::DesignFeatureFamily::Fillet
                    | design::DesignFeatureFamily::Chamfer
            )
        )
    }) {
        let native_stream = design_stream(&scope.id);
        if design::design_feature_family(&scope.kind())
            == Some(design::DesignFeatureFamily::Extrude)
        {
            let mut profile_groups = native
                .design_construction_operand_groups
                .iter()
                .filter(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                        && group.extrude_role()
                            == Some(records::topology::extrude_selection::DesignExtrudeOperandRole::Profile)
                });
            let first_profile_group = profile_groups.next();
            let second_profile_group = profile_groups.next();
            let profile_matches_operand = scope.extrude_profile().is_none_or(|profile| {
                match (first_profile_group, second_profile_group) {
                    (None, _) => {
                        usize::try_from(profile.scope_reference_ordinal)
                            .ok()
                            .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                            == Some(&profile.record_index)
                    }
                    (Some(group), None) => {
                        group.members().first().map(|member| &member.value)
                            == Some(&profile.record_index)
                    }
                    (Some(_), Some(_)) => false,
                }
            });
            if !profile_matches_operand {
                push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design Extrude profile conflicts with its profile operand group",
                    Some(scope.id.clone()),
                );
            }
            let has_body_operands = native
                .design_construction_operand_groups
                .iter()
                .any(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                        && group.extrude_role()
                            == Some(records::topology::extrude_selection::DesignExtrudeOperandRole::Bodies)
                });
            let face_operand_group_count = native
                .design_construction_operand_groups
                .iter()
                .filter(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                        && group.extrude_role().is_some_and(|role| {
                            matches!(role, records::topology::extrude_selection::DesignExtrudeOperandRole::Faces(_))
                        })
                })
                .count();
            let target_shape_group_count = native
                .design_construction_operand_groups
                .iter()
                .filter(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                        && group.role() == DesignOperandRole::ROLE_0X5
                        && group.extrude_role().is_none()
                        && !group.members().is_empty()
                        && group
                            .members()
                            .iter()
                            .map(|member| &member.value)
                            .enumerate()
                            .all(|(ordinal, record_index)| {
                                id_from_index(ordinal).is_some_and(|ordinal| {
                                    native.design_body_recipe_operands.iter().any(|operand| {
                                        design_stream(&operand.id) == native_stream
                                            && operand.scope_record_index == scope.record_index
                                            && operand.owner.group()
                                                == Some((group.record_index, ordinal))
                                            && operand.record_index() == *record_index
                                    })
                                })
                            })
                })
                .count();
            let operation_matches_operands = match scope
                .extrude_prologue()
                .map(records::feature::extrude::DesignExtrudePrologue::operation)
            {
                Some(records::feature::extrude::DesignExtrudeOperation::NewBody) => {
                    !has_body_operands
                }
                Some(
                    records::feature::extrude::DesignExtrudeOperation::Join
                    | records::feature::extrude::DesignExtrudeOperation::Cut
                    | records::feature::extrude::DesignExtrudeOperation::Intersect,
                ) => has_body_operands,
                None => true,
            };
            if ctx.holds_design_intent() && !operation_matches_operands {
                push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design Extrude operation conflicts with its body operands",
                    Some(scope.id.clone()),
                );
            }
            let Some(prologue) = scope.extrude_prologue() else {
                continue;
            };
            let Some(extrude_extent) = prologue.extent() else {
                continue;
            };
            let parameter_kind_count = |source_kind: &str| {
                native
                    .design_parameter_owners
                    .iter()
                    .filter(|owner| {
                        design_stream(owner.id()) == native_stream
                            && owner.scope_record_index() == Some(scope.record_index)
                    })
                    .filter_map(|owner| {
                        parameters_by_index.get(&(native_stream, owner.parameter_record_index()))
                    })
                    .filter(|parameter| parameter.source_kind() == source_kind)
                    .count()
            };
            let parameter_kind_values = |source_kind: &'static str| {
                native
                    .design_parameter_owners
                    .iter()
                    .filter(|owner| {
                        design_stream(owner.id()) == native_stream
                            && owner.scope_record_index() == Some(scope.record_index)
                    })
                    .filter_map(|owner| {
                        parameters_by_index.get(&(native_stream, owner.parameter_record_index()))
                    })
                    .filter(move |parameter| parameter.source_kind() == source_kind)
                    .map(|parameter| parameter.evaluated_value().get())
            };
            let along_count = parameter_kind_count("AlongDistance");
            let against_count = parameter_kind_count("AgainstDistance");
            let fixed_lanes = scope.fixed_extrude_parameters();
            // A fixed lane is usually also the parameter owner of its source
            // kind; it adds a carrier only when it is not.
            let lane_count = |source_kind: &str,
                              lane: Option<
                &records::feature::fixed_parameters::DesignFixedExtrudeScalar,
            >| {
                let lane_is_owner = lane.is_some_and(|lane| {
                    native.design_parameter_owners.iter().any(|owner| {
                        design_stream(owner.id()) == native_stream
                            && owner.scope_record_index() == Some(scope.record_index)
                            && owner.record_index() == lane.record_index
                            && parameters_by_index
                                .get(&(native_stream, owner.parameter_record_index()))
                                .is_some_and(|parameter| parameter.source_kind() == source_kind)
                    })
                });
                parameter_kind_count(source_kind) + usize::from(lane.is_some() && !lane_is_owner)
            };
            let profile_offset_count = lane_count(
                "ProfileOffset",
                fixed_lanes.and_then(|fixed| fixed.profile_offset.as_ref()),
            );
            let side_one_offset_count = lane_count(
                "Side1Offset",
                fixed_lanes.and_then(|fixed| fixed.side_one_offset.as_ref()),
            );
            // A to-face Extrude's fixed lanes carry only its termination
            // offset and taper.
            let fixed_lanes_are_to_face_lanes = fixed_lanes.is_none_or(|fixed| {
                fixed.along_distance.is_none()
                    && fixed.against_distance.is_none()
                    && fixed.side_two_taper_angle.is_none()
            });
            let omitted_zero_side_one_offset =
                crate::design::face_resolve::extrude_omits_zero_side_one_offset(
                    scope,
                    &prologue,
                    side_one_offset_count,
                );
            let mut side_one_offsets = parameter_kind_values("Side1Offset");
            let side_one_offset_is_absent = match side_one_offsets.next() {
                None => true,
                Some(0.0) => side_one_offsets.next().is_none(),
                Some(_) => false,
            };
            let side_two_offset_count = parameter_kind_count("Side2Offset");
            let has_fixed_extrude_parameters = scope.fixed_extrude_parameters().is_some();
            let has_fixed_along = scope
                .fixed_extrude_parameters()
                .as_ref()
                .is_some_and(|fixed| fixed.along_distance.is_some());
            let fixed_along_uses_reversal = scope
                .fixed_extrude_parameters()
                .as_ref()
                .and_then(|fixed| fixed.along_distance.as_ref())
                .is_some_and(|distance| {
                    matches!(
                        distance,
                        records::feature::fixed_parameters::DesignFixedExtrudeDistance::DistanceConstruction(_)
                    )
                });
            let has_one_along_carrier = along_count <= 1 && (along_count == 1 || has_fixed_along);
            let has_fixed_against = scope
                .fixed_extrude_parameters()
                .as_ref()
                .is_some_and(|fixed| fixed.against_distance.is_some());
            let has_one_against_carrier =
                against_count <= 1 && (against_count == 1 || has_fixed_against);
            let class_296_two_faces_layout = scope.generation().extrude()
                == Some(records::feature::scope::generation::DesignExtrudeFrame::Class296TwoSidedToFaces);
            let extent_matches_operands = match extrude_extent {
                records::feature::extrude::DesignExtrudeExtent::OneSidedDistance => {
                    has_one_along_carrier
                        && against_count == 0
                        && side_one_offset_is_absent
                        && (!prologue.direction_reversed() || fixed_along_uses_reversal)
                }
                records::feature::extrude::DesignExtrudeExtent::OneSidedToFace => {
                    along_count == 0
                        && fixed_lanes_are_to_face_lanes
                        && against_count == 0
                        && if target_shape_group_count == 1 {
                            side_one_offset_is_absent
                        } else {
                            side_one_offset_count == 1 || omitted_zero_side_one_offset
                        }
                }
                records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces => {
                    along_count == 0
                        && !has_fixed_extrude_parameters
                        && against_count == 0
                        && side_one_offset_count == 1
                        && side_two_offset_count == 1
                        && (!prologue.direction_reversed() || class_296_two_faces_layout)
                }
                records::feature::extrude::DesignExtrudeExtent::TwoSidedDistance => {
                    has_one_along_carrier
                        && has_one_against_carrier
                        && side_one_offset_count == 0
                        && !prologue.direction_reversed()
                }
                records::feature::extrude::DesignExtrudeExtent::TwoSidedDistanceToFace => {
                    along_count == 1
                        && !has_fixed_extrude_parameters
                        && against_count == 0
                        && side_one_offset_count == 0
                        && side_two_offset_count == 1
                        && !prologue.direction_reversed()
                }
                records::feature::extrude::DesignExtrudeExtent::SymmetricDistance => {
                    has_one_along_carrier
                        && against_count == 0
                        && side_one_offset_is_absent
                        && !prologue.direction_reversed()
                }
                records::feature::extrude::DesignExtrudeExtent::SymmetricThroughAll => {
                    along_count == 0
                        && !has_fixed_extrude_parameters
                        && against_count == 0
                        && side_one_offset_is_absent
                        && !prologue.direction_reversed()
                }
                records::feature::extrude::DesignExtrudeExtent::OneSidedThroughNext
                | records::feature::extrude::DesignExtrudeExtent::OneSidedThroughAll => {
                    along_count == 0
                        && !has_fixed_extrude_parameters
                        && against_count == 0
                        && side_one_offset_is_absent
                }
            };
            let extrude_start = prologue.start();
            let start_matches_operands = match extrude_start {
                records::feature::extrude::DesignExtrudeStart::ProfilePlane => {
                    profile_offset_count == 0
                }
                records::feature::extrude::DesignExtrudeStart::OffsetProfilePlane
                | records::feature::extrude::DesignExtrudeStart::FromFace => {
                    profile_offset_count == 1
                }
            };
            let expected_face_group_count = usize::from(
                matches!(
                    extrude_extent,
                    records::feature::extrude::DesignExtrudeExtent::OneSidedToFace
                ) && target_shape_group_count == 0,
            ) + 2 * usize::from(matches!(
                extrude_extent,
                records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces
            )) + usize::from(matches!(
                extrude_extent,
                records::feature::extrude::DesignExtrudeExtent::TwoSidedDistanceToFace
            )) + usize::from(matches!(
                extrude_start,
                records::feature::extrude::DesignExtrudeStart::FromFace
            ));
            let mut face_groups = native.design_construction_operand_groups.iter().filter(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                        && group.extrude_role().is_some_and(|role| {
                            matches!(role, records::topology::extrude_selection::DesignExtrudeOperandRole::Faces(_))
                        })
                }).collect::<Vec<_>>();
            face_groups.sort_by(|left, right| {
                left.scope_reference_ordinal
                    .cmp(&right.scope_reference_ordinal)
            });
            let expected_face_roles = match (extrude_start, extrude_extent) {
                (
                    records::feature::extrude::DesignExtrudeStart::FromFace,
                    records::feature::extrude::DesignExtrudeExtent::OneSidedToFace,
                ) if target_shape_group_count == 0 => vec![
                    records::topology::extrude_selection::DesignExtrudeFaceRole::Start,
                    records::topology::extrude_selection::DesignExtrudeFaceRole::Termination,
                ],
                (records::feature::extrude::DesignExtrudeStart::FromFace, _) => {
                    vec![records::topology::extrude_selection::DesignExtrudeFaceRole::Start]
                }
                (_, records::feature::extrude::DesignExtrudeExtent::OneSidedToFace)
                    if target_shape_group_count == 0 =>
                {
                    vec![records::topology::extrude_selection::DesignExtrudeFaceRole::Termination]
                }
                (_, records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces) => vec![
                    records::topology::extrude_selection::DesignExtrudeFaceRole::Termination,
                    records::topology::extrude_selection::DesignExtrudeFaceRole::Termination,
                ],
                (_, records::feature::extrude::DesignExtrudeExtent::TwoSidedDistanceToFace) => {
                    vec![records::topology::extrude_selection::DesignExtrudeFaceRole::Termination]
                }
                _ => Vec::new(),
            };
            let invalid_face_groups_hide_extent_conflict =
                class_296_two_faces_layout && face_operand_group_count == 0;
            if !invalid_face_groups_hide_extent_conflict
                && (!extent_matches_operands
                    || !start_matches_operands
                    || face_operand_group_count != expected_face_group_count
                    || face_groups
                        .iter()
                        .map(|group| group.extrude_face_role())
                        .ne(expected_face_roles.iter().copied().map(Some)))
            {
                push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design Extrude start or extent conflicts with its parameters and face operands",
                    Some(scope.id.clone()),
                );
            }
        }
        if design::design_feature_family(&scope.kind()) == Some(design::DesignFeatureFamily::Sweep)
        {
            let mut profile_groups =
                native
                    .design_construction_operand_groups
                    .iter()
                    .filter(|group| {
                        design_stream(&group.id) == native_stream
                            && group.scope_record_index == scope.record_index
                            && group.role() == DesignOperandRole::PROFILE
                    });
            let profile_group = profile_groups.next();
            let profile_matches_operand = profile_groups.next().is_none()
                && scope.sweep_profile().is_none_or(|profile| {
                    profile_group.is_some_and(|group| {
                        group
                            .members()
                            .iter()
                            .map(|member| member.value)
                            .eq([profile.record_index])
                    })
                });
            if !profile_matches_operand {
                push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design Sweep profile conflicts with its profile operand group",
                    Some(scope.id.clone()),
                );
            }
        }
    }
}

/// Validate Fillet radius-law parameter assignments; returns the assigned groups.
fn validate_fillet_radius_groups<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> HashSet<(&'a str, u32)> {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let construction_groups_by_index = &ctx.operand_groups_by_index;
    let mut fillet_radius_group_records = HashSet::new();
    let mut fillet_radius_group_slots = HashSet::new();
    for assignment in &native.design_fillet_radius_groups {
        let native_stream = design_stream(&assignment.id);
        let scope = scopes_by_index.get(&(native_stream, assignment.scope_record_index));
        let group =
            construction_groups_by_index.get(&(native_stream, assignment.group_record_index));
        let assignment_parameter = |record_index: u32| {
            let parameter = *parameters_by_index.get(&(native_stream, record_index))?;
            let owner = *owners_by_index.get(&(native_stream, parameter.owner_record_index()?))?;
            (owner.scope_record_index() == Some(assignment.scope_record_index)
                && owner.parameter_record_index() == record_index)
                .then_some(parameter)
        };
        let tangency_weight = assignment
            .tangency_weight_parameter_record_index
            .and_then(&assignment_parameter);
        let is_fillet = |scope: &&records::feature::scope::DesignParameterScope| {
            design::design_feature_family(&scope.kind())
                == Some(design::DesignFeatureFamily::Fillet)
        };
        let valid = scope.is_some_and(is_fillet)
            && group.is_some_and(|group| {
                group.scope_record_index == assignment.scope_record_index
                    && group
                        .members()
                        .iter()
                        .map(|member| member.value)
                        .eq(assignment.edge_operand_record_indices.iter().copied())
            })
            && match &assignment.law {
                records::topology::fillet::DesignFilletRadiusLaw::Constant {
                    radius_parameter_record_index,
                } => {
                    assignment_parameter(*radius_parameter_record_index).is_some_and(|parameter| {
                        parameter.source_kind() == "Radius"
                            && parameter
                                .unit()
                                .map(|field| field.value.as_str())
                                .is_some_and(design::feature_project::design_length_unit)
                            && parameter.evaluated_value().get() > 0.0
                    })
                }
                records::topology::fillet::DesignFilletRadiusLaw::Chordal {
                    chord_length_parameter_record_index,
                } => assignment_parameter(*chord_length_parameter_record_index).is_some_and(
                    |parameter| {
                        parameter.source_kind() == "ChordLen"
                            && parameter
                                .unit()
                                .map(|field| field.value.as_str())
                                .is_some_and(design::feature_project::design_length_unit)
                            && parameter.evaluated_value().get() > 0.0
                    },
                ),
                records::topology::fillet::DesignFilletRadiusLaw::Asymmetric {
                    offset_one_parameter_record_index,
                    offset_two_parameter_record_index,
                } => [
                    (*offset_one_parameter_record_index, "EdgeOffset1"),
                    (*offset_two_parameter_record_index, "EdgeOffset2"),
                ]
                .into_iter()
                .all(|(record_index, kind)| {
                    assignment_parameter(record_index).is_some_and(|parameter| {
                        parameter.source_kind() == kind
                            && parameter
                                .unit()
                                .map(|field| field.value.as_str())
                                .is_some_and(design::feature_project::design_length_unit)
                            && parameter.evaluated_value().get() > 0.0
                    })
                }),
                records::topology::fillet::DesignFilletRadiusLaw::Variable {
                    start_radius_parameter_record_index,
                    end_radius_parameter_record_index,
                    middle: midpoint_records,
                } => {
                    let radius = |record_index: u32, kind: &str| {
                        assignment_parameter(record_index)
                            .filter(|parameter| {
                                parameter.source_kind() == kind
                                    && parameter
                                        .unit()
                                        .map(|field| field.value.as_str())
                                        .is_some_and(design::feature_project::design_length_unit)
                                    && parameter.evaluated_value().get() >= 0.0
                            })
                            .map(|parameter| parameter.evaluated_value().get())
                    };
                    let start = radius(*start_radius_parameter_record_index, "StartRadius");
                    let end = radius(*end_radius_parameter_record_index, "EndRadius");
                    let mut middle_positive = false;
                    let mut previous_position = None;
                    let mut middle_valid = true;
                    for row in midpoint_records {
                        let Some(middle_radius) =
                            radius(row.radius_parameter_record_index, "MidRadius")
                        else {
                            middle_valid = false;
                            break;
                        };
                        let Some(position) = assignment_parameter(row.parameter_record_index)
                            .filter(|parameter| {
                                parameter.source_kind() == "MidParams"
                                    && parameter.unit().is_none()
                                    && (0.0..1.0).contains(&parameter.evaluated_value().get())
                            })
                            .map(|parameter| parameter.evaluated_value().get())
                        else {
                            middle_valid = false;
                            break;
                        };
                        if previous_position.is_some_and(|previous| previous >= position) {
                            middle_valid = false;
                            break;
                        }
                        middle_positive |= middle_radius > 0.0;
                        previous_position = Some(position);
                    }
                    start.zip(end).is_some_and(|(start, end)| {
                        middle_valid && (start > 0.0 || end > 0.0 || middle_positive)
                    })
                }
            }
            && assignment
                .tangency_weight_parameter_record_index
                .is_none_or(|_| {
                    tangency_weight.is_some_and(|parameter| {
                        parameter.source_kind() == "TangencyWeight" && parameter.unit().is_none()
                    })
                })
            && fillet_radius_group_records.insert((native_stream, assignment.group_record_index))
            && fillet_radius_group_slots.insert((
                native_stream,
                assignment.scope_record_index,
                assignment.group_ordinal,
            ));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Fillet radius group has an invalid parameter assignment",
                Some(assignment.id.clone()),
            );
        }
    }
    fillet_radius_group_records
}

/// Report Fillet operand groups that carry no radius assignment.
fn validate_fillet_operand_groups<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    fillet_radius_group_records: &HashSet<(&'a str, u32)>,
) {
    let native = ctx.native;
    let scopes_by_index = &ctx.scopes_by_index;
    for group in &native.design_construction_operand_groups {
        let native_stream = design_stream(&group.id);
        let scope = scopes_by_index.get(&(native_stream, group.scope_record_index));
        let is_fillet = scope.is_some_and(|scope| {
            design::design_feature_family(&scope.kind())
                == Some(design::DesignFeatureFamily::Fillet)
        });
        let mut fixed_edge_group_count = 0;
        let mut is_fixed_edge_group = false;
        if let Some(scope) = scope {
            for candidate in &native.design_construction_operand_groups {
                if design_stream(&candidate.id) == native_stream
                    && candidate.scope_record_index == scope.record_index
                    && !candidate.members().is_empty()
                    && candidate
                        .members()
                        .iter()
                        .map(|member| &member.value)
                        .all(|member| {
                            native.design_edge_operands.iter().any(|operand| {
                                design_stream(&operand.id) == native_stream
                                    && operand.scope_record_index == scope.record_index
                                    && operand.record_index() == *member
                            })
                        })
                {
                    fixed_edge_group_count += 1;
                    is_fixed_edge_group |= candidate.record_index == group.record_index;
                }
            }
        }
        let has_radius_assignment =
            fillet_radius_group_records.contains(&(native_stream, group.record_index));
        let has_parameter_owner = native.design_parameter_owners.iter().any(|owner| {
            design_stream(owner.id()) == native_stream
                && owner.scope_record_index() == Some(group.scope_record_index)
        });
        let sole_compact_group_shape = scope.is_some_and(|scope| {
            native
                .design_construction_operand_groups
                .iter()
                .filter(|candidate| {
                    design_stream(&candidate.id) == native_stream
                        && candidate.scope_record_index == scope.record_index
                })
                .count()
                == 1
                && group
                    .members()
                    .iter()
                    .map(|member| &member.value)
                    .all(|member| {
                        native.design_edge_identity_operands.iter().any(|operand| {
                            design_stream(&operand.id) == native_stream
                                && operand.scope_record_index == scope.record_index
                                && operand.group_record_index == group.record_index
                                && operand.record_index() == *member
                        })
                    })
        });
        let full_round_group_shape = is_fillet
            && group.role() == DesignOperandRole::BODIES_A
            && !has_radius_assignment
            && !has_parameter_owner
            && scope.is_some_and(|scope| {
                native
                    .design_construction_operand_groups
                    .iter()
                    .filter(|candidate| {
                        design_stream(&candidate.id) == native_stream
                            && candidate.scope_record_index == scope.record_index
                    })
                    .count()
                    == 1
                    && group.members().len() == 1
                    && native.design_edge_operands.iter().all(|operand| {
                        design_stream(&operand.id) != native_stream
                            || operand.scope_record_index != scope.record_index
                            || operand.record_index() != group.members()[0].value
                    })
                    && native.design_face_operands.iter().any(|operand| {
                        design_stream(&operand.id) == native_stream
                            && operand.scope_record_index == scope.record_index
                            && operand.group_record_index() == Some(group.record_index)
                            && operand.group_member_ordinal() == Some(0)
                            && operand.record_index() == group.members()[0].value
                            && operand.recipe_kind
                                == records::recipes::ConstructionRecipeKind::BoundedFace
                    })
            });
        let valid_full_round_group = full_round_group_shape
            && !group.frame.variant
            && group.frame.trailing_records().len() == 1
            && group.frame.trailing_flags().len() == 1
            && group.frame.trailing_records()[0].value
                == group.frame.trailing_flags()[0].record_index
            && group.frame.trailing_flags()[0].value
            && native.design_face_operands.iter().any(|operand| {
                design_stream(&operand.id) == native_stream
                    && operand.scope_record_index == group.scope_record_index
                    && operand.group_record_index() == Some(group.record_index)
                    && operand.group_member_ordinal() == Some(0)
                    && operand.record_index() == group.members()[0].value
                    && !operand.resolved_face_slots.is_empty()
            });
        if full_round_group_shape {
            if !valid_full_round_group {
                push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design Fillet full-round face group is invalid",
                    Some(group.id.clone()),
                );
            }
            continue;
        }
        let has_fixed_assignment = scope
            .and_then(|scope| scope.fixed_fillet_parameters().map(|fixed| (scope, fixed)))
            .is_some_and(|(scope, fixed)| {
                native.design_parameter_owners.iter().all(|owner| {
                    design_stream(owner.id()) != native_stream
                        || owner.scope_record_index() != Some(scope.record_index)
                }) && fixed
                    .groups
                    .iter()
                    .flat_map(|group| {
                        group
                            .tangency_weight()
                            .into_iter()
                            .map(|tangency| tangency.record_index)
                            .chain(group.law().radii().map(|scalar| scalar.record_index))
                            .chain(
                                group
                                    .law()
                                    .intermediate()
                                    .iter()
                                    .map(|row| row.parameter.record_index),
                            )
                    })
                    .all(|record_index| {
                        scope
                            .reference_members()
                            .values()
                            .filter(|member| **member == record_index)
                            .count()
                            == 1
                    })
                    && ((fixed_edge_group_count == fixed.groups.len() && is_fixed_edge_group)
                        || (fixed.groups.len() == 1 && sole_compact_group_shape))
            });
        if is_fillet
            && (group.role() == DesignOperandRole::BODIES_B || sole_compact_group_shape)
            && !has_fixed_assignment
            && !has_radius_assignment
        {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Fillet operand group has no radius assignment",
                Some(group.id.clone()),
            );
        }
    }
}

/// Validate construction operand identity chains; returns identity-backed groups.
fn validate_construction_operand_identities<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> HashSet<(&'a str, u32)> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let operand_groups_by_index = &ctx.operand_groups_by_index;
    let mut operand_identity_groups = HashSet::new();
    let mut identity_ordinals = HashSet::new();
    for identity in &native.design_construction_operand_identities {
        let native_stream = design_stream(&identity.id);
        let group = operand_groups_by_index.get(&(native_stream, identity.group_record_index));
        let selected_profile = group
            .and_then(|group| scopes_by_index.get(&(native_stream, group.scope_record_index)))
            .and_then(|scope| scope.extrude_profile());
        let wrapper_shape = identity.wrappers().iter().all(|wrapper| {
            records_by_index
                .get(&(native_stream, wrapper.record_index))
                .is_some_and(|header| {
                    wrapper.byte_offset.is_none_or(|offset| {
                        source_positions_agree(header.byte_offset, Some(offset))
                    }) && header.class_tag == wrapper.class_tag
                })
        });
        let transform = group.and_then(|group| group.frame.trailing_transforms().first());
        let tracking_shape = identity.tracking_path().is_none_or(|path| {
            records_by_index
                .get(&(native_stream, path.wrapper_record_index()))
                .is_some_and(|header| {
                    path.wrapper_byte_offset().is_none_or(|offset| {
                        source_positions_agree(header.byte_offset, Some(offset))
                    }) && header.class_tag == path.wrapper_class_tag
                })
                && records_by_index
                    .get(&(native_stream, path.carrier_record_index()))
                    .is_some_and(|header| {
                        path.carrier_byte_offset().is_none_or(|offset| {
                            source_positions_agree(header.byte_offset, Some(offset))
                        }) && header.class_tag == path.carrier_class_tag
                    })
                && records_by_index
                    .get(&(native_stream, path.following_record_index()))
                    .is_some_and(|header| {
                        path.following_byte_offset().is_none_or(|offset| {
                            source_positions_agree(header.byte_offset, Some(offset))
                        }) && header.class_tag == path.following_class_tag
                    })
        });
        let chain_entry_shape = if let Some(path) = identity.tracking_path() {
            !identity.wrappers().is_empty()
                || (identity.wrappers().is_empty()
                    && transform.is_some_and(|transform| {
                        path.wrapper_record_index() == transform.following_record_index()
                            && source_positions_agree(
                                transform.following_byte_offset(),
                                path.wrapper_byte_offset(),
                            )
                            && path.wrapper_class_tag == transform.following_class_tag
                    }))
                || (identity.wrappers().is_empty()
                    && transform.is_none()
                    && group.is_some_and(|group| {
                        group
                            .frame
                            .trailing_records()
                            .first()
                            .map(|record| &record.value)
                            == Some(&path.wrapper_record_index())
                    }))
        } else {
            true
        };
        let following_shape =
            if identity.tracking_path().is_some() || !identity.wrappers().is_empty() {
                true
            } else {
                transform.is_some_and(|transform| {
                    identity.following_record_index() == transform.following_record_index()
                        && source_positions_agree(
                            transform.following_byte_offset(),
                            identity.following_byte_offset(),
                        )
                        && *identity.following_class_tag() == transform.following_class_tag
                })
            } && records_by_index
                .get(&(native_stream, identity.following_record_index()))
                .is_some_and(|header| {
                    identity.following_byte_offset().is_none_or(|offset| {
                        source_positions_agree(header.byte_offset, Some(offset))
                    }) && header.class_tag == *identity.following_class_tag()
                });
        let persistent_shape = identity.persistent_identity().is_none_or(|persistent| {
            selected_profile.is_none_or(|profile| profile.asset_id == persistent.asset_id)
                && (persistent.next_record_index != 0
                    || match (
                        identity.following_byte_offset(),
                        persistent.next_byte_offset(),
                    ) {
                        (Some(start), Some(end)) => {
                            start.checked_add(190) == Some(end)
                                && !records_by_index.values().any(|header| {
                                    design_stream(&header.id) == native_stream
                                        && header.byte_offset == Some(end)
                                })
                        }
                        _ => true,
                    })
                && records_by_index
                    .get(&(native_stream, persistent.next_record_index))
                    .is_none_or(|header| {
                        persistent.next_byte_offset().is_none_or(|offset| {
                            source_positions_agree(header.byte_offset, Some(offset))
                        })
                    })
        });
        let valid = identity_ordinals.insert((native_stream,identity.ordinal)) && group.is_some_and(|group| {
            let trailing = group
                .frame
                .trailing_records()
                .first()
                .map(|record| record.value);
            identity
                .wrappers()
                .first()
                .map(|wrapper| wrapper.record_index)
                .or_else(|| {
                    group
                        .frame
                        .trailing_transforms()
                        .first()
                        .map(super::records::topology::construction::DesignConstructionOperandTransform::record_index)
                })
                .or_else(|| {
                    identity
                        .tracking_path()
                        .map(super::records::topology::construction::DesignConstructionTrackingPath::wrapper_record_index)
                })
                == trailing
        }) && wrapper_shape
            && tracking_shape
            && chain_entry_shape
            && following_shape
            && persistent_shape
            && operand_identity_groups.insert((native_stream, identity.group_record_index));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design construction operand identity has an invalid nested frame",
                Some(identity.id.clone()),
            );
        }
    }
    operand_identity_groups
}

/// Validate edge identity operands; returns their backing record set.
fn validate_edge_identity_operands<'a>(
    decode: &DecodeContext<'_>,
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    expected_face_operands: &[records::topology::face::DesignFaceOperand],
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let operand_groups_by_index = &ctx.operand_groups_by_index;
    let mut expected_edge_identity_operands =
        reload_native_arena(decode, ctx.ir, "design_edge_identity_operands")?;
    let scope_histories = history::bind_scope_histories(
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        &native.asm_histories,
    );
    history::selection::bind_edge_identity_history(
        &mut expected_edge_identity_operands,
        &native.design_construction_operand_identities,
        &native.design_parameter_scopes,
        history::BindingHistories::Shared(&native.asm_histories),
        &scope_histories,
        &HashSet::new(),
    );
    history::selection::bind_edge_identity_bounded_face_rules(
        &mut expected_edge_identity_operands,
        expected_face_operands,
    );
    let expected_edge_identity_operands = expected_edge_identity_operands
        .iter()
        .map(|operand| (operand.id.as_str(), operand))
        .collect::<std::collections::HashMap<_, _>>();
    let mut edge_identity_slots = HashSet::new();
    let mut edge_identity_records = HashSet::new();
    for operand in &native.design_edge_identity_operands {
        let native_stream = design_stream(&operand.id);
        let scope = scopes_by_index.get(&(native_stream, operand.scope_record_index));
        let group = operand_groups_by_index.get(&(native_stream, operand.group_record_index));
        let header = records_by_index.get(&(native_stream, operand.record_index()));
        let valid = scope.is_some_and(|scope| {
            matches!(
                design::design_feature_family(&scope.kind()),
                Some(design::DesignFeatureFamily::Fillet | design::DesignFeatureFamily::Chamfer)
            )
        }) && group.is_some_and(|group| {
            group.scope_record_index == operand.scope_record_index
                && usize::try_from(operand.group_member_ordinal)
                    .ok()
                    .and_then(|ordinal| group.members().get(ordinal).map(|member| &member.value))
                    == Some(&operand.record_index())
        }) && header.is_some_and(|header| {
            source_positions_agree(header.byte_offset, operand.byte_offset())
                && header.class_tag == operand.class_tag
        }) && expected_edge_identity_operands
            .get(operand.id.as_str())
            .is_some_and(|expected| operand.same_content(expected))
            && edge_identity_slots.insert((
                native_stream,
                operand.group_record_index,
                operand.group_member_ordinal,
            ))
            && edge_identity_records.insert((native_stream, operand.record_index()));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design edge identity operand has an invalid fixed frame",
                Some(operand.id.clone()),
            );
        }
    }
    Ok(edge_identity_records)
}

/// Validate whole-body recipe operands; returns their backing record set.
fn validate_body_recipe_operands<'a>(
    decode: &DecodeContext<'_>,
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let operand_groups_by_index = &ctx.operand_groups_by_index;
    let recipes_by_id = &ctx.recipes_by_id;
    let mut expected_operands = reload_native_arena(decode, ctx.ir, "design_body_recipe_operands")?;
    design::decode::operands::bind_body_recipe_operand_candidates(
        &mut expected_operands,
        &native.construction_recipes,
        &native.persistent_subentity_tags,
        &native.design_parameter_scopes,
    );
    history::bind_body_recipe_operand_history_candidates(
        &mut expected_operands,
        &native.construction_recipes,
        &native.design_parameter_scopes,
        history::BindingHistories::Shared(&native.asm_histories),
    )?;
    let expected_operands = expected_operands
        .iter()
        .map(|operand| (operand.id.as_str(), operand))
        .collect::<std::collections::HashMap<_, _>>();
    let mut member_slots = HashSet::new();
    let mut operand_records = HashSet::new();
    for operand in &native.design_body_recipe_operands {
        let native_stream = design_stream(&operand.id);
        let scope = scopes_by_index.get(&(native_stream, operand.scope_record_index));
        let header = records_by_index.get(&(native_stream, operand.record_index()));
        let recipe = recipes_by_id.get(operand.recipe_id.as_str());
        let valid_owner = scope.is_some_and(|scope| match operand.owner {
            records::topology::body_recipe::DesignOperandOwner::Group {
                group_record_index,
                group_member_ordinal,
            } => operand_groups_by_index
                .get(&(native_stream, group_record_index))
                .is_some_and(|group| {
                    group.scope_record_index == operand.scope_record_index
                        && usize::try_from(group_member_ordinal)
                            .ok()
                            .and_then(|ordinal| {
                                group.members().get(ordinal).map(|member| &member.value)
                            })
                            == Some(&operand.record_index())
                }),
            records::topology::body_recipe::DesignOperandOwner::ScopeReference {
                scope_reference_ordinal,
            } => {
                (scope.kind() == crate::records::feature::scope::DesignFeatureKind::Hole
                    || (!scope_reference_ordinal.is_multiple_of(2)
                        && scope.combine_operation().is_some_and(|operation| {
                            operation.target_record_index == operand.record_index()
                                || operation
                                    .tools
                                    .iter()
                                    .any(|tool| tool.record_index == operand.record_index())
                        })))
                    && usize::try_from(scope_reference_ordinal)
                        .ok()
                        .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                        == Some(&operand.record_index())
            }
        });
        let valid = valid_owner
            && header.is_some_and(|header| {
                source_positions_agree(header.byte_offset, operand.byte_offset())
                    && header.class_tag == operand.class_tag
            })
            && body_recipe_reference_table_is_admitted(scope.copied(), operand)
            && recipe.is_some_and(|recipe| {
                let selector_is_valid = recipe.design.as_ref().is_some_and(|design| {
                    let Some(selector) = design.selector else {
                        return false;
                    };
                    selector.value != 0
                        && match (design.id.offset, selector.byte_offset, recipe.byte_offset) {
                            (Some(design_at), Some(selector_at), Some(recipe_at)) => {
                                u64::try_from(design.id.value.len())
                                    .ok()
                                    .is_some_and(|length| {
                                        let follows =
                                            design_at.checked_add(length) == Some(selector_at);
                                        let prefix = selector_at.checked_add(20) == Some(recipe_at);
                                        let suffix = recipe_at
                                            .checked_add(u64_from_index(b"body_recipe_data".len()))
                                            .and_then(|offset| offset.checked_add(12))
                                            == Some(design_at)
                                            && selector.value == operand.next_record_index();
                                        follows && (prefix || suffix)
                                    })
                            }
                            (None, None, None) => true,
                            _ => true, // The other typed record may have been authored independently.
                        }
                });
                design_stream(&recipe.id) == native_stream
                    && recipe.kind == records::recipes::ConstructionRecipeKind::Body
                    && source_position_before(operand.context_id_offset(), recipe.byte_offset)
                    && source_position_before(recipe.byte_offset, operand.next_byte_offset())
                    && selector_is_valid
            })
            && expected_operands.get(operand.id.as_str()) == Some(&operand)
            && member_slots.insert((native_stream, operand.scope_record_index, operand.owner))
            && operand_records.insert((native_stream, operand.record_index()));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design body recipe operand has an invalid nested frame",
                Some(operand.id.clone()),
            );
        }
    }
    Ok(operand_records)
}

/// Report operand groups lacking a typed member carrier.
fn validate_operand_group_carriers<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    operand_identity_groups: &HashSet<(&'a str, u32)>,
    edge_identity_records: &HashSet<(&'a str, u32)>,
    body_recipe_operand_records: &HashSet<(&'a str, u32)>,
    edge_operand_records: &HashSet<(&'a str, u32)>,
    edge_treatment_vertex_records: &HashSet<(&'a str, u32)>,
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
        identity_members.sort_by_key(|left| left.group_member_ordinal);
        let has_exact_identity_members = !group.members().is_empty()
            && identity_members.len() == group.members().len()
            && identity_members
                .iter()
                .enumerate()
                .all(|(ordinal, operand)| {
                    usize::try_from(operand.group_member_ordinal) == Ok(ordinal)
                        && group.members().get(ordinal).map(|member| &member.value)
                            == Some(&operand.record_index())
                        && edge_identity_records.contains(&(native_stream, operand.record_index()))
                });
        let has_exact_entity_selection_members = !group.members().is_empty()
            && group
                .members()
                .iter()
                .map(|member| &member.value)
                .enumerate()
                .all(|(ordinal, record_index)| {
                    id_from_index(ordinal).is_some_and(|ordinal| {
                        native
                            .design_entity_selection_operands
                            .iter()
                            .any(|operand| {
                                design_stream(&operand.id) == native_stream
                                    && operand.scope_record_index == group.scope_record_index
                                    && operand.group_record_index == group.record_index
                                    && operand.group_member_ordinal == ordinal
                                    && operand.record_index() == *record_index
                            })
                    })
                });
        let has_exact_face_members = !group.members().is_empty()
            && group
                .members()
                .iter()
                .map(|member| &member.value)
                .enumerate()
                .all(|(ordinal, record_index)| {
                    id_from_index(ordinal).is_some_and(|ordinal| {
                        native.design_face_operands.iter().any(|operand| {
                            design_stream(&operand.id) == native_stream
                                && operand.scope_record_index == group.scope_record_index
                                && operand.group_record_index() == Some(group.record_index)
                                && operand.group_member_ordinal() == Some(ordinal)
                                && operand.record_index() == *record_index
                        })
                    })
                });
        let has_exact_body_recipe_members = !group.members().is_empty()
            && group
                .members()
                .iter()
                .map(|member| &member.value)
                .enumerate()
                .all(|(ordinal, record_index)| {
                    id_from_index(ordinal).is_some_and(|ordinal| {
                        native.design_body_recipe_operands.iter().any(|operand| {
                            design_stream(&operand.id) == native_stream
                                && operand.scope_record_index == group.scope_record_index
                                && operand.owner.group() == Some((group.record_index, ordinal))
                                && operand.record_index() == *record_index
                                && body_recipe_operand_records
                                    .contains(&(native_stream, operand.record_index()))
                        })
                    })
                });
        let has_exact_topology_recipe_members = !group.members().is_empty()
            && group
                .members()
                .iter()
                .map(|member| &member.value)
                .all(|record_index| {
                    edge_operand_records.contains(&(native_stream, *record_index))
                        || edge_treatment_vertex_records.contains(&(native_stream, *record_index))
                });
        let has_exact_sketch_profile_member = group.members().len() == 1
            && ctx
                .scopes_by_index
                .get(&(native_stream, group.scope_record_index))
                .is_some_and(|scope| {
                    scope
                        .extrude_profile()
                        .or(scope.sweep_profile())
                        .or(scope.base_flange_profile())
                        .is_some_and(|profile| {
                            group
                                .members()
                                .iter()
                                .map(|member| member.value)
                                .eq([profile.record_index])
                        })
                });
        let has_exact_group_members = !group.members().is_empty()
            && group
                .members()
                .iter()
                .map(|member| &member.value)
                .all(|record_index| {
                    native
                        .design_construction_operand_groups
                        .iter()
                        .any(|member| {
                            design_stream(&member.id) == native_stream
                                && member.scope_record_index == group.scope_record_index
                                && member.scope_reference_ordinal > group.scope_reference_ordinal
                                && member.record_index == *record_index
                        })
                });
        let has_exact_trailing_carrier = group.frame.trailing_records().is_empty()
            || operand_identity_groups.contains(&(native_stream, group.record_index))
            || (group.frame.trailing_transforms().len()
                + group.frame.trailing_dual_transforms().len()
                + group.frame.trailing_flags().len()
                == group.frame.trailing_records().len()
                && group
                    .frame
                    .trailing_records()
                    .iter()
                    .map(|record| &record.value)
                    .all(|record_index| {
                        group
                            .frame
                            .trailing_transforms()
                            .iter()
                            .any(|transform| transform.record_index() == *record_index)
                            || group
                                .frame
                                .trailing_dual_transforms()
                                .iter()
                                .any(|transform| transform.record_index == *record_index)
                            || group
                                .frame
                                .trailing_flags()
                                .iter()
                                .any(|flag| flag.record_index == *record_index)
                    }));
        let has_exact_member_carrier = operand_identity_groups
            .contains(&(native_stream, group.record_index))
            || has_exact_identity_members
            || has_exact_entity_selection_members
            || has_exact_face_members
            || has_exact_body_recipe_members
            || has_exact_topology_recipe_members
            || has_exact_sketch_profile_member
            || has_exact_group_members;
        if !has_exact_member_carrier {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design construction operand group has no exact typed member",
                Some(group.id.clone()),
            );
        }
        if !has_exact_trailing_carrier {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design construction operand group has no exact trailing carrier",
                Some(group.id.clone()),
            );
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
        let header = records_by_index.get(&(native_stream, member.record_index()));
        let selected_profile = group
            .and_then(|group| scopes_by_index.get(&(native_stream, group.scope_record_index)))
            .and_then(|scope| scope.extrude_profile());
        let selected_sketch =
            selected_profile.and_then(|profile| u32::try_from(profile.entity_id.suffix()).ok());
        let point_targets = native.sketch_points.iter().filter_map(|point| {
            (selected_sketch.is_some()
                && design_stream(&point.id) == native_stream
                && point.owner_reference == selected_sketch
                && point.persistent_id() == Some(member.local_id))
            .then_some(records::sketch_relations::SketchRelationOperand::Point {
                record_index: point.record_index,
                persistent_id: point.persistent_id(),
            })
        });
        let curve_targets = native.sketch_curve_identities.iter().filter_map(|curve| {
            (selected_sketch.is_some()
                && design_stream(&curve.id) == native_stream
                && curve.owner_reference == selected_sketch
                && (curve.primary_id.get() == member.local_id
                    || curve.secondary_id != 0 && curve.secondary_id == member.local_id))
                .then_some(records::sketch_relations::SketchRelationOperand::Curve {
                    record_index: curve.record_index,
                    primary_id: curve.primary_id.get(),
                    secondary_id: curve.secondary_id,
                })
        });
        let mut targets = point_targets.chain(curve_targets);
        let first_target = targets.next();
        let expected_target = if targets.next().is_none() {
            first_target
        } else {
            None
        };
        let mut expected_identities = native
            .design_construction_operand_identities
            .iter()
            .filter(|identity| {
                design_stream(&identity.id) == native_stream
                    && identity.following_record_index() == member.record_index()
                    && source_positions_agree(
                        identity.following_byte_offset(),
                        Some(member.byte_offset()),
                    )
                    && identity.persistent_identity().is_some_and(|persistent| {
                        persistent.local_id == member.local_id
                            && persistent.asset_id == member.asset_id
                            && persistent.context_id == member.context_id
                    })
            })
            .collect::<Vec<_>>();
        expected_identities.sort_by_key(|identity| identity.ordinal);

        let expected_history = history::selection::historical_extrude_selection_identity_kind(
            member,
            &native.design_component_naming_spaces,
            &native.design_body_bindings,
            &native.asm_histories,
        );
        let history_matches = if history::projection_was_finalized(&native.asm_histories) {
            if let Some(binding) = member.historical.as_ref() {
                binding
                    .state_ids
                    .iter()
                    .copied()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    == binding.state_ids.len()
                    && binding.state_ids.iter().all(|state_id| {
                        native
                            .asm_histories
                            .iter()
                            .flat_map(crate::history_records::AsmHistory::states)
                            .any(|state| state.state_id == *state_id)
                    })
            } else {
                true
            }
        } else {
            expected_history
                .as_ref()
                .map(|(kind, entity_ref, states)| (*kind, *entity_ref, states.as_slice()))
                == member.historical.as_ref().map(|binding| {
                    (
                        binding.kind,
                        binding.entity_ref,
                        binding.state_ids.as_slice(),
                    )
                })
        };
        let terminal_next = member.next_record_index == 0
            && !records_by_index.values().any(|header| {
                design_stream(&header.id) == native_stream
                    && header.byte_offset == Some(member.next_byte_offset())
            });
        let valid = group.is_some_and(|group| {
            usize::try_from(member.group_member_ordinal)
                .ok()
                .and_then(|ordinal| group.members().get(ordinal))
                .map(|reference| reference.value)
                == Some(member.record_index())
        }) && header.is_some_and(|header| {
            source_positions_agree(header.byte_offset, Some(member.byte_offset()))
                && header.class_tag == member.class_tag
        }) && selected_profile
            .is_none_or(|profile| profile.asset_id == member.asset_id)
            && member.resolved_geometry == expected_target
            && member
                .operand_identity_ids
                .iter()
                .map(String::as_str)
                .eq(expected_identities
                    .iter()
                    .map(|identity| identity.id.as_str()))
            && history_matches
            && (member.next_record_index != 0 || terminal_next)
            && member_slots.insert((
                native_stream,
                member.group_record_index,
                member.group_member_ordinal,
            ))
            && member_records.insert((native_stream, member.record_index()));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Extrude selection member has an invalid fixed frame",
                Some(member.id.clone()),
            );
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
        let header = records_by_index.get(&(native_stream, operand.record_index()));
        let valid = group.is_some_and(|group| {
            group.scope_record_index == operand.scope_record_index
                && usize::try_from(operand.group_member_ordinal)
                    .ok()
                    .and_then(|ordinal| group.members().get(ordinal).map(|member| &member.value))
                    == Some(&operand.record_index())
        }) && header.is_some_and(|header| {
            source_positions_agree(header.byte_offset, Some(operand.byte_offset()))
                && header.class_tag == *operand.class_tag()
        }) && entity_selection_slots.insert((
            native_stream,
            operand.group_record_index,
            operand.group_member_ordinal,
        ));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design entity-selection operand has an invalid nested frame",
                Some(operand.id.clone()),
            );
        }
    }
}

/// Report Extrude selection groups with missing or inconsistent members.
fn validate_extrude_selection_group_members(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let members_by_slot = &ctx.members_by_slot;
    for group in &native.design_extrude_selection_groups {
        let native_stream = design_stream(&group.id);
        let complete = (0..group.members().len()).all(|ordinal| {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                return false;
            };
            let Some(member) = members_by_slot.get(&(native_stream, group.record_index, ordinal))
            else {
                return false;
            };
            let next = usize::try_from(ordinal)
                .ok()
                .and_then(|ordinal| group.members().get(ordinal + 1));
            next.is_none_or(|next_record_index| {
                let Some(next_ordinal) = ordinal.checked_add(1) else {
                    return false;
                };
                let next_member =
                    members_by_slot.get(&(native_stream, group.record_index, next_ordinal));
                member.next_record_index == next_record_index.value
                    && next_member.is_some_and(|next_member| {
                        member.next_byte_offset() == next_member.byte_offset()
                    })
            })
        });
        let context_id = members_by_slot
            .get(&(native_stream, group.record_index, 0))
            .map(|member| member.context_id.as_str());
        let context_consistent = context_id.is_some_and(|context_id| {
            (0..group.members().len()).all(|ordinal| {
                u32::try_from(ordinal)
                    .ok()
                    .and_then(|ordinal| {
                        members_by_slot.get(&(native_stream, group.record_index, ordinal))
                    })
                    .is_some_and(|member| member.context_id.as_str() == context_id)
            })
        });
        if !(complete && context_consistent) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Extrude selection group has missing members",
                Some(group.id.clone()),
            );
        }
    }
}

/// Bytes from a face recipe header to the recipe program, by recipe kind.
///
/// `None` states a recipe kind that carries no face operand: it states no
/// program offset at all.
fn recipe_program_operand_length(kind: records::recipes::ConstructionRecipeKind) -> Option<u64> {
    match kind {
        records::recipes::ConstructionRecipeKind::Face => Some(16),
        records::recipes::ConstructionRecipeKind::BoundedFace => Some(24),
        records::recipes::ConstructionRecipeKind::Body
        | records::recipes::ConstructionRecipeKind::Edge
        | records::recipes::ConstructionRecipeKind::Vertex => None,
    }
}

fn recipe_reference_frames_match(
    actual: &[records::dimensions::DesignRecipeReference],
    expected: &[records::dimensions::DesignRecipeReference],
    ignore_derived_candidates: bool,
) -> bool {
    if !ignore_derived_candidates {
        return actual == expected;
    }
    actual.len() == expected.len()
        && actual.iter().zip(expected).all(|(actual, expected)| {
            actual.selector == expected.selector
                && actual.selector_offset == expected.selector_offset
                && actual.token == expected.token
                && actual.token_offset == expected.token_offset
                && actual.design_reference == expected.design_reference
                && actual.design_reference_offset == expected.design_reference_offset
        })
}

/// Validate edge operands and their recipe frames; returns their record set.
fn validate_edge_operands<'a>(
    decode: &DecodeContext<'_>,
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let recipes_by_id = &ctx.recipes_by_id;
    let scopes_by_index = &ctx.scopes_by_index;
    let historical_candidates_retained = history::projection_was_finalized(&native.asm_histories);
    let mut edge_operand_slots = HashSet::new();
    let mut edge_operand_records = HashSet::new();
    let mut expected_edge_operands = reload_native_arena(decode, ctx.ir, "design_edge_operands")?;
    let scope_histories = history::bind_scope_histories(
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        &native.asm_histories,
    );
    let expected_face_boundaries = history::bind_edge_operand_history_candidates(
        &mut expected_edge_operands,
        &native.design_parameter_scopes,
        &native.construction_recipes,
        history::BindingHistories::Shared(&native.asm_histories),
        &scope_histories,
    )?;
    validate_face_boundaries(
        ctx,
        findings,
        (!historical_candidates_retained).then_some(expected_face_boundaries.as_slice()),
    );
    let expected_edge_operands = expected_edge_operands
        .iter()
        .map(|operand| (operand.id.as_str(), operand))
        .collect::<std::collections::HashMap<_, _>>();
    for operand in &native.design_edge_operands {
        let native_stream = design_stream(&operand.id);
        let scope = scopes_by_index.get(&(native_stream, operand.scope_record_index));
        let header = records_by_index.get(&(native_stream, operand.record_index()));
        let recipe = recipes_by_id.get(operand.recipe_id.as_str());
        let design_reference = recipe
            .and_then(|recipe| recipe.record_index)
            .map(|record_index| i64::from(record_index.value))
            .filter(|value| *value >= 0);
        let expected_faces = match design_reference {
            Some(design_reference) => design::decode::operands::edge_operand_candidate_faces(
                design_reference,
                &native.persistent_subentity_tags,
                Some(&operand.id),
            ),
            None => Vec::new(),
        };
        let mut expected_references = design::decode::dimension_frames::decode_recipe_references(
            &operand.recipe_prefix_bytes,
            operand.recipe_prefix_offset(),
        );
        if !historical_candidates_retained {
            for reference in &mut expected_references {
                design::decode::dimension_frames::bind_recipe_reference_candidates(
                    reference,
                    &native.persistent_subentity_tags,
                    Some(&operand.id),
                );
            }
        }
        let expected_surface_patch_recipe_structure = scope
            .filter(|scope| {
                scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfacePatch
            })
            .and_then(|_| {
                design::decode::operands::surface_patch_recipe_structure(
                    &operand.recipe_program,
                    operand.recipe_references.len(),
                )
            });
        let terminal_group_member = native
            .design_construction_operand_groups
            .iter()
            .any(|group| {
                design_stream(&group.id) == native_stream
                    && group.scope_record_index == operand.scope_record_index
                    && group.members().last().map(|member| &member.value)
                        == Some(&operand.record_index())
            });
        let valid = scope.is_some_and(|scope| {
            design::decode::operands::has_edge_recipe_operands(&scope.kind())
                && usize::try_from(operand.scope_reference_ordinal)
                    .ok()
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                    == Some(&operand.record_index())
        }) && header.is_some_and(|header| {
            source_positions_agree(header.byte_offset, Some(operand.byte_offset()))
                && header.class_tag == operand.class_tag
        }) && (operand
            .record_index()
            .checked_add(scope.map_or(4, |scope| {
                design::decode::operands::edge_recipe_terminal_delta(&scope.kind())
            }))
            .is_some_and(|expected_offset| operand.next_record_index == expected_offset)
            || terminal_group_member)
            && (recipe.is_some_and(|recipe| recipe.byte_offset.is_none())
                || operand
                    .recipe_prefix_offset()
                    .checked_add(u64_from_index(operand.recipe_prefix_bytes.len()))
                    .zip(recipe.and_then(|recipe| {
                        recipe.byte_offset.and_then(|offset| offset.checked_sub(4))
                    }))
                    .is_some_and(|(prefix_end, recipe_prefix_end)| {
                        prefix_end == recipe_prefix_end
                    }))
            && recipe_reference_frames_match(
                &operand.recipe_references,
                &expected_references,
                historical_candidates_retained,
            )
            && recipe.is_some_and(|recipe| {
                design_stream(&recipe.id) == native_stream
                    && recipe.kind == crate::records::recipes::ConstructionRecipeKind::Edge
                    && recipe.byte_offset.is_none_or(|offset| {
                        offset > operand.recipe_record_byte_offset()
                            && offset < operand.next_byte_offset()
                    })
            })
            && design::decode::operands::edge_recipe_structure(
                ctx.decode,
                &operand.recipe_program,
            )? == operand.recipe_structure
            && expected_surface_patch_recipe_structure == operand.surface_patch_recipe_structure
            && (historical_candidates_retained || expected_faces == operand.candidate_faces)
            && expected_edge_operands.get(operand.id.as_str()) == Some(&operand);
        let valid = valid
            && edge_operand_slots.insert((
                native_stream,
                operand.scope_record_index,
                operand.scope_reference_ordinal,
            ))
            && edge_operand_records.insert((native_stream, operand.record_index()));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design edge operand has an invalid scope or recipe frame",
                Some(operand.id.clone()),
            );
        }
    }
    Ok(edge_operand_records)
}

/// Validate `asm_face_boundaries` and every edge-operand reference to them.
///
/// Each boundary belongs to one delta state, carries the identity derived from
/// that state and its face slot, and is named by at least one reference
/// context. Each context names result-state boundaries of its result faces and
/// preceding-state boundaries of its preceding and support faces, in face
/// order. `expected` holds the boundaries re-derived from retained historical
/// topology, when the projection still retains it.
fn validate_face_boundaries<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    expected: Option<&[records::topology::historical_context::AsmFaceBoundary]>,
) {
    use records::topology::historical_context::AsmFaceBoundary;
    let native = ctx.native;
    let mut states = HashMap::new();
    for history in &native.asm_histories {
        for state in history.states() {
            states.insert(state.id.as_str(), state);
        }
    }
    let mut referenced = HashSet::new();
    for operand in &native.design_edge_operands {
        let stream = design_stream(&operand.id);
        let result_state_id = ctx
            .scopes_by_index
            .get(&(stream, operand.scope_record_index))
            .and_then(|scope| scope.history_state_id());
        let mut result_parent = None;
        let mut preceding_parent = None;
        let mut valid = true;
        let mut check =
            |ids: &'a [String], slots: &[i64], parent: &mut Option<&'a str>, state_id| {
                let mut remaining = slots.iter();
                for id in ids {
                    referenced.insert(id.as_str());
                    let boundary = AsmFaceBoundary::find(&native.asm_face_boundaries, id);
                    let state = boundary.and_then(|boundary| states.get(boundary.parent.as_str()));
                    valid &= boundary.zip(state).is_some_and(|(boundary, state)| {
                        Some(state.state_id) == state_id
                            && *parent.get_or_insert(boundary.parent.as_str()) == boundary.parent
                            && remaining.any(|slot| *slot == boundary.face_slot)
                    });
                }
            };
        for context in &operand.recipe_reference_contexts {
            let slots = |faces: &[cadmpeg_ir::ids::FaceId]| {
                faces
                    .iter()
                    .filter_map(|face| history::stable_ref(face.as_str()))
                    .collect::<Vec<_>>()
            };
            check(
                &context.result_face_boundary_ids,
                &slots(&context.result_faces),
                &mut result_parent,
                result_state_id,
            );
            check(
                &context.preceding_face_boundary_ids,
                &slots(&context.preceding_faces),
                &mut preceding_parent,
                operand.recipe_state_id,
            );
            check(
                &context.preceding_support_face_boundary_ids,
                &context.preceding_support_face_slots,
                &mut preceding_parent,
                operand.recipe_state_id,
            );
        }
        valid &= result_parent
            .zip(preceding_parent)
            .is_none_or(|(result, preceding)| {
                states
                    .get(result)
                    .zip(states.get(preceding))
                    .is_some_and(|(result, preceding)| result.parent == preceding.parent)
            });
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design edge operand names an invalid historical face boundary",
                Some(operand.id.clone()),
            );
        }
    }
    let expected = expected.map(|expected| {
        expected
            .iter()
            .map(|boundary| (boundary.id.as_str(), boundary))
            .collect::<HashMap<_, _>>()
    });
    for boundary in &native.asm_face_boundaries {
        let valid = states.get(boundary.parent.as_str()).is_some_and(|state| {
            state.byte_offset.is_none_or(|offset| {
                AsmFaceBoundary::identity(&state.id, offset, boundary.face_slot).as_deref()
                    == Some(boundary.id.as_str())
            })
        }) && referenced.contains(boundary.id.as_str())
            && expected
                .as_ref()
                .is_none_or(|expected| expected.get(boundary.id.as_str()) == Some(&boundary));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ASM face boundary has an invalid state, identity, or reference",
                Some(boundary.id.clone()),
            );
        }
    }
}

fn validate_edge_treatment_vertex_operands<'a>(
    decode: &DecodeContext<'_>,
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let native = ctx.native;
    let mut expected = reload_native_arena::<
        records::feature::work_geometry::DesignEdgeTreatmentVertexOperand,
    >(decode, ctx.ir, "design_edge_treatment_vertex_operands")?;
    for operand in &mut expected {
        for reference in &mut operand.recipe.recipe_references {
            design::decode::dimension_frames::bind_recipe_reference_candidates(
                reference,
                &native.persistent_subentity_tags,
                Some(&operand.id),
            );
        }
    }
    let scope_histories = history::bind_scope_histories(
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        &native.asm_histories,
    );
    history::bind_edge_treatment_vertex_history(
        &mut expected,
        &native.design_parameter_scopes,
        &native.asm_histories,
        &scope_histories,
    )?;
    let expected = expected
        .iter()
        .map(|operand| (operand.id.as_str(), operand))
        .collect::<std::collections::HashMap<_, _>>();
    let mut records = HashSet::new();
    for operand in &native.design_edge_treatment_vertex_operands {
        let stream = design_stream(&operand.id);
        let scope = ctx
            .scopes_by_index
            .get(&(stream, operand.scope_record_index));
        let mut groups = native
            .design_construction_operand_groups
            .iter()
            .filter(|group| {
                design_stream(&group.id) == stream
                    && group.scope_record_index == operand.scope_record_index
                    && group.record_index == operand.group_record_index
            });
        let group = groups.next();
        let valid = scope.is_some_and(|scope| {
            design::decode::operands::has_edge_recipe_operands(&scope.kind())
                && usize::try_from(operand.scope_reference_ordinal)
                    .ok()
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                    == Some(&operand.recipe.record_index())
        }) && group.is_some_and(|group| {
            usize::try_from(operand.group_member_ordinal)
                .ok()
                .and_then(|ordinal| group.members().get(ordinal).map(|member| &member.value))
                == Some(&operand.recipe.record_index())
        }) && groups.next().is_none()
            && expected.get(operand.id.as_str()) == Some(&operand);
        let valid = valid && records.insert((stream, operand.recipe.record_index()));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion edge-treatment vertex operand has an invalid group or recipe frame",
                Some(operand.id.clone()),
            );
        }
    }
    Ok(records)
}

/// Report Fillet/Chamfer edge groups with incomplete selection operands. The
/// groups are design-intent records, so a document without them has nothing
/// to check.
fn validate_edge_treatment_groups<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    edge_operand_records: &HashSet<(&'a str, u32)>,
    edge_identity_records: &HashSet<(&'a str, u32)>,
    edge_treatment_vertex_records: &HashSet<(&'a str, u32)>,
) {
    if !ctx.holds_design_intent() {
        return;
    }
    let native = ctx.native;
    for scope in native.design_parameter_scopes.iter().filter(|scope| {
        matches!(
            scope.kind(),
            crate::records::feature::scope::DesignFeatureKind::Fillet
                | crate::records::feature::scope::DesignFeatureKind::Chamfer
        )
    }) {
        let native_stream = design_stream(&scope.id);
        let mut groups = native
            .design_construction_operand_groups
            .iter()
            .filter(|group| {
                design_stream(&group.id) == native_stream
                    && group.scope_record_index == scope.record_index
            })
            .peekable();
        let complete = groups.peek().is_some()
            && groups.all(|group| {
                let recipe_backed =
                    group
                        .members()
                        .iter()
                        .map(|member| &member.value)
                        .all(|member| {
                            edge_operand_records.contains(&(native_stream, *member))
                                || edge_treatment_vertex_records.contains(&(native_stream, *member))
                        });
                let identity_backed = group
                    .members()
                    .iter()
                    .map(|member| &member.value)
                    .all(|member| edge_identity_records.contains(&(native_stream, *member)));
                recipe_backed || identity_backed
            });
        if !complete {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design edge-treatment group has incomplete selection operands",
                Some(scope.id.clone()),
            );
        }
    }
}

/// Validate face operands and their recipe frames; returns their record set.
fn validate_face_operands<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    expected_face_operands: &[records::topology::face::DesignFaceOperand],
) -> Result<HashSet<(&'a str, u32, u32)>, CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let recipes_by_id = &ctx.recipes_by_id;
    let scopes_by_index = &ctx.scopes_by_index;
    let historical_candidates_retained = history::projection_was_finalized(&native.asm_histories);
    let face_groups_by_index = native
        .design_construction_operand_groups
        .iter()
        .map(|group| ((design_stream(&group.id), group.record_index), group))
        .collect::<std::collections::HashMap<_, _>>();
    let expected_face_operands = expected_face_operands
        .iter()
        .map(|operand| (operand.id.as_str(), operand))
        .collect::<std::collections::HashMap<_, _>>();
    let mut face_operand_records = HashSet::new();
    for operand in &native.design_face_operands {
        let native_stream = design_stream(&operand.id);
        let scope = scopes_by_index.get(&(native_stream, operand.scope_record_index));
        let header = records_by_index.get(&(native_stream, operand.record_index()));
        let recipe = recipes_by_id.get(operand.recipe_id.as_str());
        let mut expected_faces = recipe
            .and_then(|recipe| recipe.record_index)
            .map(|record_index| i64::from(record_index.value))
            .filter(|value| *value >= 0)
            .into_iter()
            .flat_map(|design_reference| {
                native
                    .persistent_subentity_tags
                    .iter()
                    .filter(move |tag| {
                        crate::ids::same_native_occurrence(&tag.id, &operand.id)
                            && tag.design_references.contains(&design_reference)
                    })
                    .filter_map(|tag| match &tag.target {
                        cadmpeg_ir::attributes::AttributeTarget::Face(id) => Some(id),
                        _ => None,
                    })
            })
            .collect::<Vec<_>>();
        expected_faces.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        expected_faces.dedup();
        let mut expected_references = design::decode::dimension_frames::decode_recipe_references(
            &operand.recipe_prefix_bytes,
            operand.recipe_prefix_offset(),
        );
        if !historical_candidates_retained {
            for reference in &mut expected_references {
                design::decode::dimension_frames::bind_recipe_reference_candidates(
                    reference,
                    &native.persistent_subentity_tags,
                    Some(&operand.id),
                );
            }
        }
        let recipe_design_reference = recipe
            .and_then(|recipe| recipe.record_index)
            .map(|record_index| i64::from(record_index.value))
            .filter(|value| *value >= 0);
        let referenced_faces = expected_references
            .iter()
            .filter(|reference| Some(reference.design_reference) == recipe_design_reference)
            .flat_map(|reference| &reference.candidate_faces)
            .collect::<std::collections::HashSet<_>>();
        let expected_unreferenced_faces = expected_faces
            .iter()
            .copied()
            .filter(|face| !referenced_faces.contains(face))
            .collect::<Vec<_>>();
        let mut expected_alternate_selector_faces = expected_references
            .iter()
            .filter(|reference| Some(reference.design_reference) == recipe_design_reference)
            .flat_map(|reference| &reference.alternate_selector_faces)
            .collect::<Vec<_>>();
        expected_alternate_selector_faces.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        expected_alternate_selector_faces.dedup();
        let expected_node_offsets = operand
            .recipe_program
            .windows(3)
            .enumerate()
            .filter(|(_, values)| *values == [-1, -1, 2])
            .map(|(index, _)| {
                u64_from_index(index)
                    .checked_mul(4)
                    .and_then(|delta| operand.recipe_program_offset.checked_add(delta))
                    .ok_or_else(|| {
                        CodecError::Malformed("F3D face recipe node offset overflows".into())
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let expected_nodes = expected_node_offsets
            .iter()
            .copied()
            .zip(
                expected_node_offsets
                    .iter()
                    .copied()
                    .skip(1)
                    .chain(std::iter::once(operand.next_byte_offset())),
            )
            .collect::<Vec<_>>();
        let valid_program =
            match design::decode::operands::face_recipe_program_kind(&operand.recipe_program) {
                Some(design::decode::operands::FaceRecipeProgramKind::Terminal) => {
                    operand.recipe_nodes.is_empty()
                }
                Some(design::decode::operands::FaceRecipeProgramKind::Counted { .. }) => {
                    operand.recipe_nodes.len() == expected_nodes.len()
                        && operand.recipe_nodes.iter().zip(expected_nodes).try_fold(
                            true,
                            |valid, (node, (start, end))| -> Result<bool, CodecError> {
                                if !valid {
                                    return Ok(false);
                                }
                                Ok(node.byte_offset == start
                                    && node.end_byte_offset == end
                                    && node.program.get(0..3) == Some(&[-1, -1, 2])
                                    && node.recipe_structure
                                        == node
                                            .program
                                            .get(3..)
                                            .map(|program| {
                                                design::decode::operands::face_recipe_structure(
                                                    ctx.decode, program,
                                                )
                                            })
                                            .transpose()?
                                            .flatten()
                                    && u64_from_index(node.program.len())
                                        .checked_mul(4)
                                        .and_then(|delta| start.checked_add(delta))
                                        .is_some_and(|expected_offset| expected_offset == end))
                            },
                        )?
                        && operand.recipe_nodes.first().is_none_or(|first_node| {
                            first_node
                                .byte_offset
                                .checked_sub(operand.recipe_program_offset)
                                .and_then(|byte_offset| usize::try_from(byte_offset / 4).ok())
                                .is_some_and(|first_node_index| {
                                    operand
                                        .recipe_nodes
                                        .iter()
                                        .flat_map(|node| node.program.iter().copied())
                                        .eq(operand
                                            .recipe_program
                                            .iter()
                                            .copied()
                                            .skip(first_node_index))
                                })
                        })
                }
                None => false,
            };
        let expected_history = expected_face_operands.get(operand.id.as_str());
        let valid = scope.is_some_and(|scope| {
            let family = design::design_feature_family(&scope.kind());
            match (operand.group_record_index(), operand.group_member_ordinal()) {
                (Some(group_record_index), Some(group_member_ordinal)) => {
                    let group = face_groups_by_index
                        .get(&(native_stream, group_record_index))
                        .copied();
                    let exact_group_member = group.is_some_and(|group| {
                        group.scope_record_index == operand.scope_record_index
                            && usize::try_from(operand.scope_reference_ordinal)
                                .ok()
                                .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                                == Some(&group_record_index)
                            && usize::try_from(group_member_ordinal)
                                .ok()
                                .and_then(|ordinal| {
                                    group.members().get(ordinal).map(|member| &member.value)
                                })
                                == Some(&operand.record_index())
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
                            Some(design::DesignFeatureFamily::ReplaceFace) => {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::ROLE_0X10
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            Some(design::DesignFeatureFamily::Loft) => {
                                group.is_some_and(|group| {
                                    matches!(
                                        group.role(),
                                        DesignOperandRole::PROFILE | DesignOperandRole::ROLE_0X43
                                    )
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            Some(design::DesignFeatureFamily::Sweep) => {
                                group.is_some_and(|group| group.role() == DesignOperandRole::FACES)
                                    && operand.recipe_kind
                                        == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            Some(design::DesignFeatureFamily::SurfaceOffset) => {
                                group
                                    .is_some_and(|group| group.role() == DesignOperandRole::PROFILE)
                                    && operand.recipe_kind
                                        == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            Some(design::DesignFeatureFamily::Draft) => {
                                group.is_some_and(|group| match group.role() {
                                    DesignOperandRole::ROLE_0X10 => {
                                        operand.recipe_kind
                                            == records::recipes::ConstructionRecipeKind::BoundedFace
                                    }
                                    DesignOperandRole::ROLE_0X21 => {
                                        operand.recipe_kind
                                            == records::recipes::ConstructionRecipeKind::Face
                                    }
                                    _ => false,
                                })
                            }
                            Some(design::DesignFeatureFamily::Revolve) => {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::ROLE_0X21
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::Face
                            }
                            Some(design::DesignFeatureFamily::CircularPattern) => {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::BODIES_B
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::Face
                            }
                            Some(design::DesignFeatureFamily::Mirror) => {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::BODIES_B
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::Face
                            }
                            Some(design::DesignFeatureFamily::Thread) => {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::ROLE_0X10
                                        && scope.thread_construction().is_some_and(|construction| {
                                            construction
                                                .face_group_record_indices
                                                .contains(&group.record_index)
                                        })
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            Some(
                                design::DesignFeatureFamily::Fillet
                                | design::DesignFeatureFamily::Chamfer,
                            ) => {
                                operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                                    && native.design_edge_identity_operands.iter().any(|identity| {
                                        design_stream(&identity.id) == native_stream
                                            && identity.scope_record_index
                                                == operand.scope_record_index
                                            && identity.group_record_index == group_record_index
                                            && identity.group_member_ordinal == group_member_ordinal
                                            && identity.record_index() == operand.record_index()
                                            && identity.class_tag == operand.class_tag
                                    })
                            }
                            None if scope.kind()
                                == crate::records::feature::scope::DesignFeatureKind::SplitFace =>
                            {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::ROLE_0X10
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            None if matches!(
                                scope.kind(),
                                crate::records::feature::scope::DesignFeatureKind::DeleteFace
                                    | crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace
                            ) =>
                            {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::ROLE_0X10
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            _ => false,
                        }
                }
                (None, None) => {
                    let direct_member = usize::try_from(operand.scope_reference_ordinal)
                        .ok()
                        .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                        == Some(&operand.record_index());
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
                            Some(design::DesignFeatureFamily::Hole) => matches!(
                                operand.recipe_kind,
                                records::recipes::ConstructionRecipeKind::BoundedFace
                                    | records::recipes::ConstructionRecipeKind::Face
                            ),
                            Some(design::DesignFeatureFamily::Assemble)
                                if scope.kind()
                                    == crate::records::feature::scope::DesignFeatureKind::AsBuilt
                                    && design::assembly::scope_legacy_as_built_421_generation(
                                        scope,
                                    )
                                    .is_some() =>
                            {
                                matches!(
                                    (operand.scope_reference_ordinal, operand.recipe_kind),
                                    (1, records::recipes::ConstructionRecipeKind::BoundedFace)
                                        | (3, records::recipes::ConstructionRecipeKind::Face)
                                )
                            }
                            _ => false,
                        }
                }
                _ => false,
            }
        }) && header.is_some_and(|header| {
            source_positions_agree(header.byte_offset, Some(operand.byte_offset())) && header.class_tag == operand.class_tag
        }) && (recipe.is_some_and(|recipe|recipe.byte_offset.is_none()) || operand
            .recipe_prefix_offset().checked_add(u64_from_index(operand.recipe_prefix_bytes.len())).zip(recipe.and_then(|recipe| recipe.byte_offset.and_then(|offset| offset.checked_sub(4)))).is_some_and(|(prefix_end, recipe_prefix_end)| prefix_end == recipe_prefix_end))
            && recipe_reference_frames_match(
                &operand.recipe_references,
                &expected_references,
                historical_candidates_retained,
            )
            && valid_program
            && recipe_program_operand_length(operand.recipe_kind).is_some_and(|operand_length| {
                recipe.is_some_and(|recipe| {
                    recipe.byte_offset.is_none_or(|offset|offset.checked_add(operand_length).is_some_and(|expected_offset| operand.recipe_program_offset == expected_offset))
                })
            })
            && u64_from_index(operand.recipe_program.len()).checked_mul(4).and_then(|delta| operand
                    .recipe_program_offset.checked_add(delta)).is_some_and(|expected_offset| operand.next_byte_offset() == expected_offset)
            && (historical_candidates_retained
                || face_ids_match_refs(&operand.candidate_faces, &expected_faces))
            && (historical_candidates_retained
                || face_ids_match_refs(&operand.unreferenced_candidate_faces, &expected_unreferenced_faces))
            && (historical_candidates_retained
                || face_ids_match_refs(&operand.alternate_selector_candidate_faces, &expected_alternate_selector_faces))
            && expected_history.is_some_and(|expected| {
                operand.preceding_candidate_faces == expected.preceding_candidate_faces
                    && operand.changed_candidate_faces == expected.changed_candidate_faces
                    && operand.historical_support_contexts == expected.historical_support_contexts
            })
            && recipe.is_some_and(|recipe| {
                design_stream(&recipe.id) == native_stream
                    && recipe.kind == operand.recipe_kind
                    && recipe.byte_offset.is_none_or(|offset| offset > operand.recipe_record_byte_offset() && offset < operand.next_byte_offset())
            });
        let valid = valid
            && face_operand_records.insert((
                native_stream,
                operand.scope_record_index,
                operand.record_index(),
            ));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design face operand has an invalid scope or recipe frame",
                Some(operand.id.clone()),
            );
        }
    }
    Ok(face_operand_records)
}

fn face_ids_match_refs(
    actual: &[cadmpeg_ir::ids::FaceId],
    expected: &[&cadmpeg_ir::ids::FaceId],
) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual == *expected)
}

/// Report face-group members with no resolved recipe operand.
fn validate_face_group_member_resolution(
    findings: &mut Vec<Finding>,
    face_group_members: BTreeSet<(&str, u32, u32)>,
    face_operand_records: &HashSet<(&str, u32, u32)>,
    entity_selection_operands: &[records::topology::entity_selection::DesignEntitySelectionOperand],
) {
    let entity_selection_records = entity_selection_operands
        .iter()
        .map(|operand| {
            (
                design_stream(&operand.id),
                operand.scope_record_index,
                operand.record_index(),
            )
        })
        .collect::<std::collections::HashSet<_>>();
    for member in face_group_members {
        if !face_operand_records.contains(&member) && !entity_selection_records.contains(&member) {
            let entity = {
                format!(
                    "{}:design-face-group-member#{}:{}",
                    member.0, member.1, member.2
                )
            };
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Extrude face group has an unresolved recipe operand",
                Some(entity),
            );
        }
    }
}

/// Validate retained Face source carriers and their persistent identities.
fn validate_face_source_groups(ctx: &Ctx, findings: &mut Vec<Finding>) {
    use design::decode::operands::{
        face_source_carrier_spec, face_source_profile_pair_count, face_source_record_is_registered,
        FaceSourceType,
    };
    let native = ctx.native;
    let mut carrier_records = HashSet::new();
    for group in &native.design_face_source_groups {
        let native_stream = design_stream(&group.id);
        let scope = ctx
            .scopes_by_index
            .get(&(native_stream, group.scope_record_index));
        let carrier_header = ctx
            .records_by_index
            .get(&(native_stream, group.carrier_record_index));
        let paired_header = ctx
            .records_by_index
            .get(&(native_stream, group.paired_record_index));
        let carrier_ordinal = usize::try_from(group.carrier_reference_ordinal).ok();
        let source_spec = face_source_carrier_spec(group.source_members.len());
        let registered = |record, class: &str, role| {
            face_source_record_is_registered(&native.design_types, &group.id, record, class, role)
        };
        let scope_links_valid = scope.is_some_and(|scope| {
            scope.kind() == crate::records::feature::scope::DesignFeatureKind::Face
                && registered(
                    scope.record_index,
                    scope.class_tag.as_str(),
                    FaceSourceType::Scope,
                )
                && face_source_profile_pair_count(&native.design_types, scope, |record| {
                    ctx.records_by_index
                        .get(&(native_stream, record))
                        .map(|header| header.class_tag.as_str())
                }) == Some(group.source_members.len())
                && carrier_ordinal
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                    == Some(&group.carrier_record_index)
                && carrier_ordinal
                    .and_then(|ordinal| ordinal.checked_add(1))
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                    == Some(&group.paired_record_index)
        });
        let headers_valid = carrier_header.is_some_and(|header| {
            source_positions_agree(header.byte_offset, Some(group.carrier_span.start()))
                && header.class_tag == group.carrier_class_tag
                && registered(
                    header.record_index,
                    header.class_tag.as_str(),
                    FaceSourceType::Carrier,
                )
        }) && paired_header.is_some_and(|header| {
            source_positions_agree(header.byte_offset, Some(group.carrier_span.end()))
                && header.class_tag == group.paired_class_tag
                && registered(
                    header.record_index,
                    header.class_tag.as_str(),
                    FaceSourceType::Settings,
                )
        });
        let source_offsets_valid = source_spec.is_some_and(|layout| {
            let Ok(source_reference_offset) = u64::try_from(layout.source_reference_offset) else {
                return false;
            };
            if layout
                .scalar_offset
                .checked_add(crate::layout::sheet_metal_region_carrier_scalar_lane::LEN)
                .is_none_or(|length| {
                    u64::try_from(length).map_or(true, |length| {
                        group
                            .carrier_span
                            .start()
                            .checked_add(length)
                            .is_none_or(|end| end > group.carrier_span.end())
                    })
                })
            {
                return false;
            }
            group
                .source_members
                .iter()
                .enumerate()
                .all(|(ordinal, member)| {
                    let Ok(ordinal) = u64::try_from(ordinal) else {
                        return false;
                    };
                    group
                        .carrier_span
                        .start()
                        .checked_add(source_reference_offset)
                        .and_then(|offset| {
                            offset.checked_add(
                                ordinal.checked_mul(
                                    u64::try_from(
                                        crate::layout::sheet_metal_region_identity_reference::LEN,
                                    )
                                    .ok()?,
                                )?,
                            )
                        })
                        == Some(member.offset)
                })
        });
        let mut source_records = HashSet::new();
        let source_members_valid = if let Some(layout) = source_spec {
            if group.source_members.len() == layout.source_count {
                let mut valid = true;
                for member in group.source_members.iter().map(|member| &member.value) {
                    let unique_record = source_records.insert(member.record_index);
                    let persistent = &member.persistent_identity;
                    let local_id_offset = member.byte_offset.checked_add(21);
                    let asset_id_offset = member.byte_offset.checked_add(33);
                    let member_valid = unique_record
                        && registered(
                            member.record_index,
                            member.class_tag.as_str(),
                            FaceSourceType::Identity,
                        )
                        && member.byte_offset > group.carrier_span.start()
                        && source_positions_agree(local_id_offset, persistent.local_id_offset())
                        && source_positions_agree(asset_id_offset, persistent.asset_id_offset())
                        && source_position_before(
                            persistent.asset_id_offset(),
                            persistent.context_id_offset(),
                        )
                        && source_position_before(
                            persistent.context_id_offset(),
                            persistent.tail_slot_offset(),
                        )
                        && source_position_before(
                            Some(member.byte_offset),
                            persistent.next_byte_offset(),
                        );
                    if !member_valid {
                        valid = false;
                        break;
                    }
                }
                valid
            } else {
                false
            }
        } else {
            false
        };
        let valid = carrier_records.insert((native_stream, group.carrier_record_index))
            && scope_links_valid
            && headers_valid
            && source_offsets_valid
            && source_members_valid;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Face source carrier has invalid links or offsets",
                Some(group.id.clone()),
            );
        }
    }
}

/// Validate sketch placement frames and their scope links.
fn validate_sketch_placements(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let scopes_by_index = &ctx.scopes_by_index;
    let mut placement_records = HashSet::new();
    let mut placement_scopes = HashSet::new();
    let mut visibility_offsets = HashSet::new();
    let mut visibility_ordinals = HashSet::new();
    let mut ordinals_by_component = BTreeMap::<(&str, Option<u64>), Vec<(u64, u32)>>::new();
    // Sketch Geometry member ordinals count within a component: the one whose
    // entity cluster, starting at its root entity, holds the sketch entity.
    let mut component_roots = HashMap::<&str, Vec<u64>>::new();
    for space in &native.design_component_naming_spaces {
        component_roots
            .entry(design_stream(&space.id))
            .or_default()
            .push(space.component_record_index);
    }
    for roots in component_roots.values_mut() {
        roots.sort_unstable();
    }
    let component_of = |stream: &str, entity: u64| {
        component_roots.get(stream).and_then(|roots| {
            roots
                .partition_point(|root| *root <= entity)
                .checked_sub(1)
                .and_then(|index| roots.get(index).copied())
        })
    };
    for placement in &native.design_sketch_placements {
        let native_stream = design_stream(&placement.id);
        let component = component_of(native_stream, placement.entity_id.suffix());
        let unique_record = placement_records.insert((native_stream, placement.record_index));
        let unique_scope = match placement.scope_record_index {
            Some(index) => placement_scopes.insert((native_stream, index)),
            None => true,
        };
        let scope = placement
            .scope_record_index
            .and_then(|index| scopes_by_index.get(&(native_stream, index)));
        let visibility_valid = if let Some(visibility) = placement.visibility.as_ref() {
            let header_valid = ctx
                .entities_by_suffix
                .get(&(native_stream, placement.entity_id.suffix()))
                .is_some_and(|entity| {
                    entity.byte_offset.is_none_or(|offset| {
                        visibility
                            .stream_ordinal_offset()
                            .is_none_or(|ordinal_offset| ordinal_offset > offset)
                    })
                });
            if header_valid
                && visibility_ordinals.insert((
                    native_stream,
                    component,
                    visibility.stream_ordinal.get(),
                ))
            {
                ordinals_by_component
                    .entry((native_stream, component))
                    .or_default()
                    .push((
                        placement.entity_id.suffix(),
                        visibility.stream_ordinal.get(),
                    ));
                visibility
                    .visible_offset()
                    .is_none_or(|offset| visibility_offsets.insert((native_stream, offset)))
            } else {
                false
            }
        } else {
            true
        };
        let scope_valid = if placement.member_run_head() {
            scope.is_none_or(|scope| {
                design::design_feature_family(&scope.kind())
                    == Some(design::DesignFeatureFamily::Sketch)
            })
        } else {
            scope.is_some_and(|scope| {
                design::design_feature_family(&scope.kind())
                    == Some(design::DesignFeatureFamily::Sketch)
                    && scope
                        .sketch_entity()
                        .is_some_and(|binding| binding.entity_id == placement.entity_id)
            })
        };
        let valid = scope_valid && unique_record && unique_scope && visibility_valid;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design sketch placement has an invalid frame or scope link",
                Some(placement.id.clone()),
            );
        }
    }
    for ((stream, _), mut ordinals) in ordinals_by_component {
        ordinals.sort_unstable();
        if ordinals.windows(2).any(|pair| pair[0].1 >= pair[1].1) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design sketch Geometry member ordinals do not increase with sketch entity order",
                Some(stream.to_owned()),
            );
        }
    }
}

/// Validate parameter owner frames and their indexed parameter links. The
/// companion links resolve into design-intent records, so a document without
/// them has nothing to check.
fn validate_parameter_owners(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    if !ctx.holds_design_intent() {
        return;
    }
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let parameters_by_index = &ctx.parameters_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let mut owner_indices = HashSet::new();
    let mut owner_local_ordinals = HashSet::new();
    for owner in &native.design_parameter_owners {
        let native_stream = design_stream(owner.id());
        let unique_index = owner_indices.insert((native_stream, owner.record_index()));
        let parameter = parameters_by_index.get(&(native_stream, owner.parameter_record_index()));
        let scope_resolves = owner
            .scope_record_index()
            .is_none_or(|scope| records_by_index.contains_key(&(native_stream, scope)));
        let unique_local_ordinal = match (owner.scope_record_index(), owner.local_ordinal()) {
            (Some(scope), Some(ordinal)) => {
                owner_local_ordinals.insert((native_stream, scope, ordinal))
            }
            (_, None) => true,
            (None, Some(_)) => false,
        };
        let valid = scope_resolves
            && records_by_index.contains_key(&(native_stream, owner.parameter_record_index()))
            && records_by_index.contains_key(&(native_stream, owner.companion_record_index()))
            && companions_by_index
                .get(&(native_stream, owner.companion_record_index()))
                .is_some_and(|companion| companion.owner_record_index() == owner.record_index())
            && parameter.is_some_and(|parameter| {
                parameter.owner_record_index() == Some(owner.record_index())
                    && parameter.evaluated_value().get().to_bits()
                        == owner.evaluated_value().get().to_bits()
            })
            && unique_index
            && unique_local_ordinal;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design parameter owner has an invalid frame or indexed link",
                Some(owner.id().to_owned()),
            );
        }
    }
}

/// Validate parameter companion prefixes and owned recipe runs.
/// A kept parameter state and its kept companion name each other: the
/// companion's local reference to the state stores the state's entity index.
fn validate_parameter_state_companions(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    for state in &native.design_parameter_states {
        let Some(companion) = native
            .design_parameter_state_companions
            .iter()
            .find(|companion| companion.id == state.companion)
        else {
            continue;
        };
        if companion.state_record_index != u64::from(state.record_index) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion parameter state companion does not reference its state's entity index",
                Some(companion.id.clone()),
            );
        }
    }
}

fn validate_parameter_companions(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let mut companion_indices = HashSet::new();
    let mut companion_owners = HashSet::new();
    for companion in &native.design_parameter_companions {
        let native_stream = design_stream(companion.id());
        let payload = companion.payload();
        let payload_interval =
            payload.and_then(|payload| payload.byte_offset().zip(payload.byte_length()));
        let source_payload_valid = payload_interval.is_none_or(|(start, length)| {
            let Some(end) = start.checked_add(length) else {
                return false;
            };
            let mut expected = native
                .construction_recipes
                .iter()
                .filter(|recipe| {
                    design_stream(&recipe.id) == native_stream
                        && recipe
                            .byte_offset
                            .is_some_and(|offset| offset >= start && offset < end)
                })
                .collect::<Vec<_>>();
            expected.sort_by_key(|recipe| recipe.byte_offset);
            payload.is_some_and(|payload| {
                payload
                    .owned_recipe_ids()
                    .iter()
                    .map(String::as_str)
                    .eq(expected.iter().map(|recipe| recipe.id.as_str()))
            })
        });
        let mut recipe_ids = HashSet::new();
        let recipe_links_valid = payload.is_none_or(|payload| {
            payload.owned_recipe_ids().iter().all(|id| {
                recipe_ids.insert(id.as_str())
                    && native.construction_recipes.iter().any(|recipe| {
                        recipe.id == *id && design_stream(&recipe.id) == native_stream
                    })
            })
        });
        let unique_index = companion_indices.insert((native_stream, companion.record_index()));
        let unique_owner = companion_owners.insert((native_stream, companion.owner_record_index()));
        let owner = owners_by_index.get(&(native_stream, companion.owner_record_index()));
        let valid = source_payload_valid
            && recipe_links_valid
            && records_by_index.contains_key(&(native_stream, companion.record_index()))
            && owner
                .is_some_and(|owner| owner.companion_record_index() == companion.record_index())
            && unique_index
            && unique_owner;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design parameter companion has an invalid prefix or owner link",
                Some(companion.id().to_owned()),
            );
        }
    }
}

/// Validate dimension recipe records; returns the owned recipe ids.
fn validate_dimension_recipe_records<'a>(
    ctx: &Ctx<'a, '_>,
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
                .get(&(native_stream, companion.owner_record_index()))
                .and_then(|owner| {
                    parameters_by_index.get(&(native_stream, owner.parameter_record_index()))
                })
                .is_some_and(|parameter| {
                    parameter.kind() == records::parameters::DesignParameterKind::Dimension
                })
        });
        let recipe = native
            .construction_recipes
            .iter()
            .find(|recipe| recipe.id == record.recipe_id);
        let companion_order_matches = companion.is_some_and(|companion| {
            usize::try_from(record.recipe_ordinal)
                .ok()
                .and_then(|ordinal| {
                    companion
                        .payload()
                        .and_then(|payload| payload.owned_recipe_ids().get(ordinal))
                })
                == Some(&record.recipe_id)
        });
        // Frame admission is meaningful only while a decoder has the real
        // source frame. CADIR admits the typed recipe and references directly.
        let layout_valid = match (
            record.byte_offset,
            record.frame_length,
            record.prefix_offset,
            record.program_offset,
            record.prefix_bytes.data(),
        ) {
            (
                Some(byte_offset),
                Some(frame_length),
                Some(prefix_offset),
                Some(program_offset),
                Some(prefix),
            ) => {
                let frame_end = byte_offset.checked_add(frame_length);
                let prefix_end = prefix_offset.checked_add(u64_from_index(prefix.len()));
                let program_end = u64_from_index(record.program.len())
                    .checked_mul(4)
                    .and_then(|length| program_offset.checked_add(length));
                let mut references = design::decode::dimension_frames::decode_recipe_references(
                    prefix,
                    prefix_offset,
                );
                for reference in &mut references {
                    design::decode::dimension_frames::bind_recipe_reference_candidates(
                        reference,
                        &native.persistent_subentity_tags,
                        Some(&record.id),
                    );
                }
                frame_length >= 11
                    && !prefix.is_empty()
                    && references == record.references
                    && byte_offset.checked_add(11) == Some(prefix_offset)
                    && byte_offset
                        .checked_add(11)
                        .is_some_and(|start| program_offset >= start)
                    && program_end == frame_end
                    && recipe.is_some_and(|recipe| {
                        recipe.byte_offset.is_none_or(|offset| {
                            offset >= prefix_offset
                                && frame_end.is_some_and(|end| offset < end)
                                && prefix_end == offset.checked_sub(4)
                                && offset.checked_add(u64_from_index(
                                    design::construction_recipe_family_name_len(recipe.kind),
                                )) == Some(program_offset)
                        })
                    })
            }
            (None, None, None, None, None) => true,
            _ => false,
        };
        let edge_operands_match =
            design::decode::dimension_frames::dimension_recipe_matching_edge_operand_ids(
                record,
                &native.design_edge_operands,
            ) == record.matching_edge_operand_ids;
        let valid = layout_valid
            && !record.program.is_empty()
            && edge_operands_match
            && dimension_companion
            && companion_order_matches
            && recipe.is_some_and(|recipe| {
                design_stream(&recipe.id) == native_stream && recipe.kind == record.recipe_kind
            })
            && dimension_recipe_ids.insert((native_stream, record.recipe_id.as_str()));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension recipe has an invalid indexed-record owner",
                Some(record.id.clone()),
            );
        }
    }
    dimension_recipe_ids
}

/// Report dimension companions owning an unresolved construction recipe.
fn validate_dimension_companion_recipes<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    dimension_recipe_ids: &HashSet<(&'a str, &'a str)>,
) {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    for companion in &native.design_parameter_companions {
        let native_stream = design_stream(companion.id());
        let dimension_companion = owners_by_index
            .get(&(native_stream, companion.owner_record_index()))
            .and_then(|owner| {
                parameters_by_index.get(&(native_stream, owner.parameter_record_index()))
            })
            .is_some_and(|parameter| {
                parameter.kind() == records::parameters::DesignParameterKind::Dimension
            });
        if dimension_companion
            && companion.payload().is_some_and(|payload| {
                payload.owned_recipe_ids().iter().any(|recipe_id| {
                    !dimension_recipe_ids.contains(&(native_stream, recipe_id.as_str()))
                })
            })
        {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension companion has an unowned construction recipe",
                Some(companion.id().to_owned()),
            );
        }
    }
}

/// Resolve a dimension companion from document records, independently of source layout.
fn is_dimension_companion(ctx: &Ctx<'_, '_>, stream: &str, index: u32) -> bool {
    ctx.companions_by_index
        .get(&(stream, index))
        .and_then(|companion| {
            ctx.owners_by_index
                .get(&(stream, companion.owner_record_index()))
        })
        .and_then(|owner| {
            ctx.parameters_by_index
                .get(&(stream, owner.parameter_record_index()))
        })
        .is_some_and(|parameter| {
            parameter.kind() == records::parameters::DesignParameterKind::Dimension
        })
}
fn companion_source_contains(
    ctx: &Ctx<'_, '_>,
    stream: &str,
    index: u32,
    frame_start: Option<u64>,
) -> bool {
    ctx.companions_by_index
        .get(&(stream, index))
        .is_some_and(|companion| match (companion.byte_offset(), frame_start) {
            (Some(start), Some(at)) => {
                companion
                    .payload_byte_offset()
                    .is_some_and(|minimum| at >= minimum)
                    && !ctx.native.design_parameter_owners.iter().any(|owner| {
                        design_stream(owner.id()) == stream
                            && owner
                                .byte_offset()
                                .is_some_and(|offset| offset > start && offset <= at)
                    })
            }
            _ => true,
        })
}
fn governing_dimension_companion_valid(
    ctx: &Ctx<'_, '_>,
    stream: &str,
    index: u32,
    source_id: &str,
    source_end: Option<u64>,
) -> bool {
    is_dimension_companion(ctx, stream, index)
        && source_end.is_none_or(|end| {
            design::decode::dimension_frames::following_dimension_companion_record_index(
                source_id,
                end,
                &ctx.native.design_parameter_owners,
                &ctx.native.design_parameters,
            ) == Some(index)
        })
}
/// Validate dimension locus pairs; source framing and semantic links are separate.
fn validate_dimension_locus_pairs<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> HashSet<(&'a str, u32)> {
    let mut indices = HashSet::new();
    let mut companions = HashSet::new();
    for pair in &ctx.native.design_dimension_locus_pairs {
        let stream = design_stream(&pair.id);
        let valid = indices.insert((stream, pair.record_index))
            && companions.insert((stream, pair.companion_record_index))
            && companion_source_contains(
                ctx,
                stream,
                pair.companion_record_index,
                pair.byte_offset(),
            )
            && is_dimension_companion(ctx, stream, pair.companion_record_index)
            && governing_dimension_companion_valid(
                ctx,
                stream,
                pair.governing_companion_record_index,
                &pair.id,
                pair.paired_byte_offset(),
            )
            && pair.loci().iter().all(|locus| {
                ctx.sketch_geometry_indices
                    .contains(&(stream, locus.geometry_index()))
            });
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension locus pair has an invalid frame or geometry link",
                Some(pair.id.clone()),
            );
        }
    }
    companions
}
/// Validate annotation ownership, nullable operands, and actual source windows when held.
fn validate_dimension_annotation_frames(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let mut indices = HashSet::new();
    for frame in &ctx.native.design_dimension_annotation_frames {
        let stream = design_stream(&frame.id);
        let owner = ctx
            .owners_by_index
            .get(&(stream, frame.governing_owner_record_index));
        let interval = match frame.companion_record_index {
            Some(index) => {
                companion_source_contains(ctx, stream, index, frame.byte_offset())
                    && ctx
                        .companions_by_index
                        .get(&(stream, index))
                        .is_some_and(|companion| {
                            companion.payload().is_some_and(|payload| {
                                match (
                                    companion.payload_byte_offset(),
                                    payload.byte_length(),
                                    frame.paired_byte_offset(),
                                ) {
                                    (Some(start), Some(length), Some(frame_end)) => {
                                        start.checked_add(length).is_some_and(|end| frame_end < end)
                                    }
                                    _ => true,
                                }
                            })
                        })
            }
            None => owner
                .and_then(|owner| owner.scope_record_index())
                .and_then(|scope| ctx.scopes_by_index.get(&(stream, scope)))
                .is_some_and(|scope| {
                    let scope_start = frame.byte_offset().is_none_or(|start| {
                        scope
                            .byte_offset()
                            .is_none_or(|scope_start| start >= scope_start)
                    });
                    let before_companions = frame.paired_byte_offset().is_none_or(|frame_end| {
                        ctx.native
                            .design_parameter_owners
                            .iter()
                            .filter(|candidate| {
                                design_stream(candidate.id()) == stream
                                    && candidate.scope_record_index()
                                        == owner.and_then(|owner| owner.scope_record_index())
                            })
                            .filter_map(|candidate| {
                                ctx.companions_by_index
                                    .get(&(stream, candidate.companion_record_index()))
                                    .and_then(|companion| companion.byte_offset())
                            })
                            .min()
                            .is_some_and(|end| frame_end < end)
                    });
                    scope_start && before_companions
                }),
        };
        let governing = owner.is_some_and(|owner| {
            owner.companion_record_index() == frame.governing_companion_record_index
                && is_dimension_companion(ctx, stream, frame.governing_companion_record_index)
        });
        let operands = frame.operands().iter().all(|operand| {
            operand
                .geometry_record_index
                .is_none_or(|index| ctx.sketch_geometry_indices.contains(&(stream, index.get())))
        });
        let sketch = ctx
            .entities_by_suffix
            .get(&(stream, u64::from(frame.owner_reference)))
            .is_some_and(|entity| entity.in_sketch_module());
        if !indices.insert((stream, frame.record_index))
            || !interval
            || !governing
            || !operands
            || !sketch
        {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension annotation frame has invalid links or offsets",
                Some(frame.id.clone()),
            );
        }
    }
}
/// Validate direct presentation semantics; physical ordering is source-only evidence.
fn validate_dimension_presentation_frames(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let mut indices = HashSet::new();
    let scopes = ctx
        .native
        .design_sketch_placements
        .iter()
        .filter_map(|placement| {
            Some((
                (design_stream(&placement.id), placement.entity_id.suffix()),
                placement.scope_record_index?,
            ))
        })
        .collect::<HashMap<_, _>>();
    for frame in &ctx.native.design_dimension_presentation_frames {
        let stream = design_stream(&frame.id);
        let owner = ctx
            .owners_by_index
            .get(&(stream, frame.governing_owner_record_index));
        let owner_link = owner.is_some_and(|owner| {
            owner.parameter_record_index() == frame.governing_parameter_record_index
                && owner.companion_record_index() == frame.governing_companion_record_index
                && is_dimension_companion(ctx, stream, frame.governing_companion_record_index)
                && ctx
                    .companions_by_index
                    .get(&(stream, frame.governing_companion_record_index))
                    .is_some_and(|companion| companion.owner_record_index() == owner.record_index())
        });
        let nearest = frame.paired_byte_offset.is_none_or(|end| {
            ctx.native
                .design_parameter_owners
                .iter()
                .filter(|candidate| {
                    design_stream(candidate.id()) == stream
                        && scopes
                            .get(&(stream, u64::from(frame.owner_reference)))
                            .is_some_and(|scope| candidate.scope_record_index() == Some(*scope))
                        && candidate.byte_offset().is_some_and(|offset| offset > end)
                        && ctx
                            .parameters_by_index
                            .get(&(stream, candidate.parameter_record_index()))
                            .is_some_and(|parameter| {
                                parameter.kind()
                                    == records::parameters::DesignParameterKind::Dimension
                            })
                })
                .min_by_key(|candidate| candidate.byte_offset())
                .is_some_and(|candidate| {
                    candidate.record_index() == frame.governing_owner_record_index
                })
        });
        let operands = !frame.operands.is_empty()
            && frame.operands.iter().all(|operand| {
                ctx.sketch_geometry_indices
                    .contains(&(stream, operand.geometry_record_index.get()))
            });
        let layout = match (
            frame.byte_offset,
            frame.frame_length,
            frame.presentation_byte_offset,
            frame.paired_byte_offset,
            frame.owner_reference_offset,
        ) {
            (None, None, None, None, None) => true,
            (Some(start), Some(length), Some(presentation), Some(end), Some(owner_at)) => {
                end > start
                    && start.checked_add(length) == Some(end)
                    && start.checked_add(24).and_then(|at| {
                        u64_from_index(frame.operands.len())
                            .checked_mul(15)
                            .and_then(|delta| at.checked_add(delta))
                    }) == Some(presentation)
                    && frame.presentation_bytes.as_ref().is_some_and(|bytes| {
                        presentation.checked_add(bytes.byte_len()) == Some(end)
                    })
                    && end.checked_add(20) == Some(owner_at)
                    && frame.operands.iter().enumerate().all(|(ordinal, operand)| {
                        let at = u64_from_index(ordinal).checked_mul(15).and_then(|delta| {
                            start
                                .checked_add(24)
                                .and_then(|start| start.checked_add(delta))
                        });
                        at.and_then(|at| at.checked_add(1)) == operand.geometry_reference_offset
                            && at.and_then(|at| at.checked_add(11)) == operand.role_offset
                    })
            }
            _ => false,
        };
        let sketch = ctx
            .entities_by_suffix
            .get(&(stream, u64::from(frame.owner_reference)))
            .is_some_and(|entity| entity.in_sketch_module());
        if !indices.insert((stream, frame.record_index))
            || !layout
            || !owner_link
            || !nearest
            || !operands
            || !sketch
        {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension presentation frame has invalid links or offsets",
                Some(frame.id.clone()),
            );
        }
    }
}
/// Validate counted-locus semantic references and, independently, actual source framing.
fn validate_dimension_locus_groups<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> HashSet<(&'a str, u32)> {
    let mut indices = HashSet::new();
    let mut companions = HashSet::new();
    for group in &ctx.native.design_dimension_locus_groups {
        let stream = design_stream(&group.id);
        companions.insert((stream, group.companion_record_index));
        let count = group.loci.len();
        let references = group.loci.iter().all(|locus| {
            ctx.sketch_geometry_indices
                .contains(&(stream, locus.geometry_record_index))
                && ctx
                    .sketch_geometry_indices
                    .contains(&(stream, locus.returned.value))
        });
        let mut source_members = group
            .loci
            .iter()
            .map(|locus| locus.geometry_record_index)
            .collect::<Vec<_>>();
        source_members.sort_unstable_by(Ord::cmp);
        let mut return_members = group
            .loci
            .iter()
            .map(|locus| locus.returned.value)
            .collect::<Vec<_>>();
        return_members.sort_unstable_by(Ord::cmp);
        let layout = match (
            group.byte_offset,
            group.frame_length,
            group.owner_reference_offset,
            group.owner_role_offset,
            group.state_offset,
            group.next_byte_offset,
        ) {
            (None, None, None, None, None, None) => group.loci.iter().all(|locus| {
                locus.geometry_reference_offset.is_none()
                    && locus.role_offset.is_none()
                    && locus.returned.offset.is_none()
            }),
            (
                Some(start),
                Some(length),
                Some(owner_at),
                Some(role_at),
                Some(state_at),
                Some(end),
            ) => {
                let loci_at = start.checked_add(24);
                let owner_start = u64_from_index(count)
                    .checked_mul(15)
                    .and_then(|delta| loci_at.and_then(|at| at.checked_add(delta)));
                let returns_at = owner_start.and_then(|at| at.checked_add(24));
                start.checked_add(length) == Some(end)
                    && owner_start.and_then(|at| at.checked_add(2)) == Some(owner_at)
                    && owner_start.and_then(|at| at.checked_add(12)) == Some(role_at)
                    && owner_start.and_then(|at| at.checked_add(16)) == Some(state_at)
                    && u64_from_index(count)
                        .checked_mul(11)
                        .and_then(|delta| returns_at.and_then(|at| at.checked_add(delta)))
                        .and_then(|at| at.checked_add(1))
                        == Some(end)
                    && group.loci.iter().enumerate().all(|(ordinal, locus)| {
                        let at = u64_from_index(ordinal)
                            .checked_mul(15)
                            .and_then(|delta| loci_at.and_then(|at| at.checked_add(delta)));
                        let returned = u64_from_index(ordinal)
                            .checked_mul(11)
                            .and_then(|delta| returns_at.and_then(|at| at.checked_add(delta)))
                            .and_then(|at| at.checked_add(1));
                        at.and_then(|at| at.checked_add(1)) == locus.geometry_reference_offset
                            && at.and_then(|at| at.checked_add(11)) == locus.role_offset
                            && returned == locus.returned.offset
                    })
            }
            _ => false,
        };
        let overlap = ctx
            .native
            .design_dimension_locus_groups
            .iter()
            .all(|other| {
                design_stream(&other.id) != stream
                    || other.companion_record_index != group.companion_record_index
                    || other.record_index == group.record_index
                    || match (
                        group.byte_offset,
                        group.next_byte_offset,
                        other.byte_offset,
                        other.next_byte_offset,
                    ) {
                        (Some(start), Some(end), Some(other_start), Some(other_end)) => {
                            end <= other_start || other_end <= start
                        }
                        _ => true,
                    }
            });
        let sketch = ctx
            .entities_by_suffix
            .get(&(stream, u64::from(group.owner_reference)))
            .is_some_and(|entity| entity.in_sketch_module());
        let valid = indices.insert((stream, group.record_index))
            && companion_source_contains(
                ctx,
                stream,
                group.companion_record_index,
                group.byte_offset,
            )
            && is_dimension_companion(ctx, stream, group.companion_record_index)
            && (1..=64).contains(&count)
            && references
            && source_members == return_members
            && layout
            && overlap
            && sketch;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension locus group has an invalid counted frame or geometry link",
                Some(group.id.clone()),
            );
        }
    }
    companions
}
/// Validate null-locus dimension pairs against document-held typed companions.
fn validate_dimension_null_locus_pairs<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    locus_pairs: &HashSet<(&'a str, u32)>,
    locus_groups: &HashSet<(&'a str, u32)>,
) {
    let mut indices = HashSet::new();
    let mut companions = HashSet::new();
    for pair in &ctx.native.design_dimension_null_locus_pairs {
        let stream = design_stream(&pair.id);
        let companion = (stream, pair.companion_record_index);
        let valid = indices.insert((stream, pair.record_index))
            && companions.insert(companion)
            && companion_source_contains(
                ctx,
                stream,
                pair.companion_record_index,
                pair.byte_offset(),
            )
            && is_dimension_companion(ctx, stream, pair.companion_record_index)
            && governing_dimension_companion_valid(
                ctx,
                stream,
                pair.governing_companion_record_index,
                &pair.id,
                pair.paired_byte_offset(),
            )
            && !locus_pairs.contains(&companion)
            && !locus_groups.contains(&companion)
            && ctx
                .sketch_geometry_indices
                .contains(&(stream, pair.loci()[1].geometry_index()));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design null-locus dimension pair has an invalid frame or geometry link",
                Some(pair.id.clone()),
            );
        }
    }
}

/// Validate parameter record identity uniqueness.
fn validate_parameters(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let mut parameter_indices = HashSet::new();
    for parameter in &native.design_parameters {
        let native_stream = design_stream(&parameter.id);
        if !parameter_indices.insert((native_stream, parameter.record_index)) {
            let id = parameter.id.clone();
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design parameter has an invalid frame, family discriminator, or owner",
                Some(id),
            );
        }
    }
}

/// Validate design entity reference runs and suffix uniqueness.
fn validate_entity_headers(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let mut entity_suffixes = HashSet::new();
    for header in &native.design_entity_headers {
        let native_stream = design_stream(&header.id);
        let references_resolve = header
            .reference_values()
            .all(|index| records_by_index.contains_key(&(native_stream, *index)));
        if !references_resolve {
            push_constant_finding(
                findings,
                Check::ReferentialIntegrity,
                "Fusion design entity has an invalid reference run",
                Some(header.entity_id.as_str().to_owned()),
            );
        }
        let key = (native_stream, header.entity_id.suffix());
        if !entity_suffixes.insert(key) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design entity suffix is duplicated within its stream",
                Some(header.entity_id.as_str().to_owned()),
            );
        }
    }
}

/// Validate sketch relation owners and byte frames.
fn validate_sketch_relations(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let sketch_owner_ids = &ctx.sketch_owner_ids;
    for relation in &native.sketch_relations {
        let native_stream = design_stream(&relation.id);
        let owner_matches = matches!(
            (
                sketch_owner_ids.get(&(native_stream, relation.owner_reference)),
                relation.owner_entity_id.as_ref().map(cadmpeg_core::text::NonBlankString::as_str),
            ),
            (Some(expected), Some(actual)) if *expected == actual
        );
        if !owner_matches {
            push_constant_finding(
                findings,
                Check::ReferentialIntegrity,
                "Fusion sketch relation has an invalid owner or byte frame",
                Some(relation.id.clone()),
            );
        }
    }
}

/// Validate sketch point and surface persistent identities and the typed
/// record of every sketch point, curve, and surface.
fn validate_sketch_geometry_identities(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let mut sketch_point_identities = HashSet::new();
    let mut sketch_geometry_records = HashSet::new();
    // An unresolved owner is not one shared sketch. Enforce uniqueness only
    // when the owning sketch reference is known.
    for point in &native.sketch_points {
        let duplicate = if let (Some(persistent_id), Some(owner_reference)) =
            (point.persistent_id(), point.owner_reference)
        {
            !sketch_point_identities.insert((
                design_stream(&point.id),
                owner_reference,
                persistent_id,
            ))
        } else {
            false
        };
        if duplicate {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch point has an invalid persistent identity",
                Some(point.id.clone()),
            );
        }
        if !sketch_geometry_records.insert((design_stream(&point.id), point.record_index)) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch geometry aliases another typed indexed record",
                Some(point.id.clone()),
            );
        }
    }
    // Distinct curve records of one Sketch can carry one identity pair, so
    // the pair is not checked for uniqueness.
    for curve in &native.sketch_curve_identities {
        if !sketch_geometry_records.insert((design_stream(&curve.id), curve.record_index)) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch geometry aliases another typed indexed record",
                Some(curve.id.clone()),
            );
        }
    }
    let mut sketch_surface_identities = HashSet::new();
    for surface in &native.sketch_surfaces {
        let duplicate = if let Some(owner_reference) = surface.owner_reference {
            !sketch_surface_identities.insert((
                design_stream(&surface.id),
                owner_reference,
                surface.persistent_id.get(),
            ))
        } else {
            false
        };
        if duplicate {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch surface has an invalid persistent identity",
                Some(surface.id.clone()),
            );
        }
        if !sketch_geometry_records.insert((design_stream(&surface.id), surface.record_index)) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch geometry aliases another typed indexed record",
                Some(surface.id.clone()),
            );
        }
    }
}

fn emit_sketch_relation_finding(findings: &mut Vec<Finding>, entity: &str, message: &'static str) {
    let entity = {
        let mut copied = String::new();
        copied.push_str(entity);
        copied
    };
    findings.push(Finding {
        related: Vec::new(),
        check: Check::NativeLinks,
        severity: Severity::Error,
        message: message.into(),
        entity: Some(entity),
    });
}

fn validate_sketch_relation_owners(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let placements_by_scope = &ctx.placements_by_scope;
    let sketch_owner_ids = &ctx.sketch_owner_ids;
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
                records::sketch_relations::SketchRelationOperand::Point {
                    record_index: point.record_index,
                    persistent_id: point.persistent_id(),
                },
            )
        })
        .chain(native.sketch_curve_identities.iter().map(|curve| {
            (
                (design_stream(&curve.id), curve.record_index),
                records::sketch_relations::SketchRelationOperand::Curve {
                    record_index: curve.record_index,
                    primary_id: curve.primary_id.get(),
                    secondary_id: curve.secondary_id,
                },
            )
        }))
        .chain(native.sketch_surfaces.iter().map(|surface| {
            (
                (design_stream(&surface.id), surface.record_index),
                records::sketch_relations::SketchRelationOperand::Surface {
                    record_index: surface.record_index,
                    persistent_id: surface.persistent_id.get(),
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
        if sketch_owner_ids.contains_key(&(native_stream, owner_reference)) {
            relation_owners.insert((native_stream, record_index), owner_reference);
        }
    }
    for relation in &native.sketch_relations {
        let native_stream = design_stream(&relation.id);
        let agrees = |reference: &records::sketch_relations::SketchRelationReference| {
            let record_index = reference.record_index();
            match sketch_operands.get(&(native_stream, record_index)) {
                Some(expected) => reference.resolved() == Some(expected),
                None => {
                    reference.resolved()
                        == Some(&records::sketch_relations::SketchRelationOperand::Record {
                            record_index,
                        })
                }
            }
        };
        if !relation
            .members()
            .iter()
            .all(|member| agrees(&member.reference))
            || !relation
                .return_members()
                .iter()
                .all(|member| agrees(&member.reference))
        {
            emit_sketch_relation_finding(
                findings,
                &relation.id,
                "Fusion sketch relation typed operands disagree with its indexed references",
            );
        }
        for member in relation.all_member_indices() {
            if !typed_sketch_records.contains(&(native_stream, member)) {
                continue;
            }
            if relation_owners
                .insert((native_stream, member), relation.owner_reference)
                .is_some_and(|owner| owner != relation.owner_reference)
            {
                emit_sketch_relation_finding(
                    findings,
                    &relation.id,
                    "Fusion sketch member belongs to multiple sketch owners",
                );
            }
        }
    }
    for entity in native
        .design_entity_headers
        .iter()
        .filter(|entity| entity.in_sketch_module())
    {
        let native_stream = design_stream(&entity.id);
        let Ok(owner) = u32::try_from(entity.entity_id.suffix()) else {
            continue;
        };
        for member in entity.member_values() {
            if !typed_sketch_records.contains(&(native_stream, *member)) {
                continue;
            }
            if relation_owners
                .insert((native_stream, *member), owner)
                .is_some_and(|existing| existing != owner)
            {
                emit_sketch_relation_finding(
                    findings,
                    &entity.id,
                    "Fusion sketch member belongs to multiple sketch owners",
                );
            }
        }
    }
    for pair in &native.design_dimension_locus_pairs {
        let native_stream = design_stream(&pair.id);
        let owner = companions_by_index
            .get(&(native_stream, pair.governing_companion_record_index))
            .and_then(|companion| {
                owners_by_index.get(&(native_stream, companion.owner_record_index()))
            })
            .and_then(|parameter_owner| {
                parameter_owner
                    .scope_record_index()
                    .and_then(|scope| placements_by_scope.get(&(native_stream, scope)))
            })
            .and_then(|placement| u32::try_from(placement.entity_id.suffix()).ok());
        let Some(owner) = owner else {
            continue;
        };
        for member in [
            pair.loci()[0].geometry_index(),
            pair.loci()[1].geometry_index(),
        ] {
            if relation_owners
                .insert((native_stream, member), owner)
                .is_some_and(|existing| existing != owner)
            {
                emit_sketch_relation_finding(
                    findings,
                    &pair.id,
                    "Fusion sketch member belongs to multiple sketch owners",
                );
            }
        }
    }
    for group in &native.design_dimension_locus_groups {
        let native_stream = design_stream(&group.id);
        for member in group
            .loci
            .iter()
            .map(|locus| locus.geometry_record_index)
            .chain(group.loci.iter().map(|locus| locus.returned.value))
        {
            if relation_owners
                .insert((native_stream, member), group.owner_reference)
                .is_some_and(|existing| existing != group.owner_reference)
            {
                emit_sketch_relation_finding(
                    findings,
                    &group.id,
                    "Fusion sketch member belongs to multiple sketch owners",
                );
            }
        }
    }
    for pair in &native.design_dimension_null_locus_pairs {
        let native_stream = design_stream(&pair.id);
        let owner = companions_by_index
            .get(&(native_stream, pair.governing_companion_record_index))
            .and_then(|companion| {
                owners_by_index.get(&(native_stream, companion.owner_record_index()))
            })
            .and_then(|parameter_owner| {
                parameter_owner
                    .scope_record_index()
                    .and_then(|scope| placements_by_scope.get(&(native_stream, scope)))
            })
            .and_then(|placement| u32::try_from(placement.entity_id.suffix()).ok());
        let Some(owner) = owner else {
            continue;
        };
        if relation_owners
            .insert((native_stream, pair.loci()[1].geometry_index()), owner)
            .is_some_and(|existing| existing != owner)
        {
            emit_sketch_relation_finding(
                findings,
                &pair.id,
                "Fusion sketch member belongs to multiple sketch owners",
            );
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
            emit_sketch_relation_finding(
                findings,
                id,
                "Fusion sketch geometry owner disagrees with its relation graph",
            );
        }
    }
}

/// Validate persistent body links and their history ordering.
fn validate_body_links(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let ir = ctx.ir;
    let body_ids = ir
        .model
        .bodies
        .iter()
        .map(|body| &body.id)
        .collect::<std::collections::HashSet<_>>();
    let mut body_links: std::collections::BTreeMap<_, Vec<_>> = std::collections::BTreeMap::new();
    for link in &native.persistent_design_links {
        let target_key = match &link.target {
            cadmpeg_ir::attributes::AttributeTarget::Body(id) if body_ids.contains(id) => Some(id),
            _ => None,
        };
        let Some(target_key) = target_key else {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion persistent body link has an invalid target or group payload",
                Some(link.id.clone()),
            );
            continue;
        };
        body_links.entry(target_key).or_default().push(link);
    }
    for links in body_links.values_mut() {
        links.sort_by_key(|left| left.ordinal);
        if links
            .iter()
            .enumerate()
            .any(|(ordinal, link)| u32::try_from(ordinal) != Ok(link.ordinal))
        {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion persistent body links have noncanonical history ordering",
                links.first().map(|link| link.id.clone()),
            );
        }
    }
}

/// Validate persistent subentity tags and their group ordering.
fn validate_subentity_tags(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let ir = ctx.ir;
    let face_ids = ir
        .model
        .faces
        .iter()
        .map(|face| &face.id)
        .collect::<std::collections::HashSet<_>>();
    let edge_ids = ir
        .model
        .edges
        .iter()
        .map(|edge| &edge.id)
        .collect::<std::collections::HashSet<_>>();
    let mut subentity_tags: std::collections::BTreeMap<_, Vec<_>> =
        std::collections::BTreeMap::new();
    for tag in &native.persistent_subentity_tags {
        let Some(target_key) = (match &tag.target {
            cadmpeg_ir::attributes::AttributeTarget::Face(id) if face_ids.contains(id) => {
                Some((1_u8, id.as_str()))
            }
            cadmpeg_ir::attributes::AttributeTarget::Edge(id) if edge_ids.contains(id) => {
                Some((0_u8, id.as_str()))
            }
            _ => None,
        }) else {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion persistent subentity tag has an invalid target or group payload",
                Some(tag.id.clone()),
            );
            continue;
        };
        subentity_tags.entry(target_key).or_default().push(tag);
    }
    for tags in subentity_tags.values_mut() {
        tags.sort_by_key(|left| left.ordinal);
        if tags
            .iter()
            .enumerate()
            .any(|(ordinal, tag)| u32::try_from(ordinal) != Ok(tag.ordinal))
        {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion persistent subentity tags have noncanonical group ordering",
                tags.first().map(|tag| tag.id.clone()),
            );
        }
    }
}

/// Validate each ASM history graph as a coherent state chain.
fn validate_history_graphs(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    for history in &native.asm_histories {
        if let Err(error) = history.check_transitions() {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: format!("Fusion ASM history transition is invalid: {error:?}"),
                entity: Some(history.id.clone()),
                related: Vec::new(),
            });
        }
        let coherent = history::graph_is_coherent(history);
        if !coherent {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ASM history graph is not a coherent doubly linked state chain",
                Some(history.id.clone()),
            );
        }
    }
}

#[cfg(test)]
mod tests;
