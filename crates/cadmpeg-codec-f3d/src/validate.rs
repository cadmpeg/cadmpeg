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

use crate::design::decode::scopes::extrude::is_class_296_two_sided_to_faces_layout;
use crate::design::decode::scopes::extrude::is_class_296_two_sided_to_faces_scope;
use crate::{design, history, ids, native, records};
mod dimensions;
mod scopes;

use cadmpeg_core::decode::id_from_index;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::{
    check::{Check, Finding},
    Severity,
};

/// Resolve the native design stream that owns a record `id`, defaulting to the
/// primary design stream when the id carries no stream qualifier.
fn design_stream(id: &str) -> &str {
    ids::native_stream(id).unwrap_or(ids::DEFAULT_STREAM)
}

/// Match a direct escaped entry or an xref scope ending in the entry.
fn design_stream_contains_entry(
    decode: &DecodeContext<'_>,
    stream: &str,
    entry: &str,
) -> Result<bool, CodecError> {
    const OP: &str = "match F3D design stream entry";
    if let Some(direct) = decode.strip_prefix(stream, "f3d:", OP)? {
        let mut cursor = 0usize;
        let matches = decode.all_by(
            entry.chars(),
            |character| {
                let mut utf8 = [0; 4];
                let bytes = character.encode_utf8(&mut utf8).as_bytes();
                let mut escaped = [0; 12];
                let length = if matches!(character, ':' | '#' | '%') || character.is_whitespace() {
                    const HEX: &[u8; 16] = b"0123456789ABCDEF";
                    // One Unicode scalar has at most four UTF-8 bytes.
                    for (index, byte) in bytes.iter().copied().enumerate() {
                        escaped[index * 3] = b'%';
                        escaped[index * 3 + 1] = HEX[usize::from(byte >> 4)];
                        escaped[index * 3 + 2] = HEX[usize::from(byte & 15)];
                    }
                    bytes.len() * 3
                } else {
                    escaped[..bytes.len()].copy_from_slice(bytes);
                    bytes.len()
                };
                let Some(end) = cursor.checked_add(length) else {
                    return Ok(false);
                };
                let Some(actual) = direct.as_bytes().get(cursor..end) else {
                    return Ok(false);
                };
                if actual != &escaped[..length] {
                    return Ok(false);
                }
                cursor = end;
                Ok(true)
            },
            OP,
        )?;
        if matches && cursor == direct.len() {
            return Ok(true);
        }
    }
    match decode.strip_prefix(stream, "f3d:xref/", OP)? {
        Some(qualified) => match decode.strip_suffix(qualified, entry, OP)? {
            Some(prefix) => decode.ends_with(prefix, "/", OP),
            None => Ok(false),
        },
        None => Ok(false),
    }
}

/// Admit the empty reference table used by a legacy Combine tool operand.
fn body_recipe_reference_table_is_admitted(
    decode: &DecodeContext<'_>,
    scope: Option<&records::feature::scope::DesignParameterScope>,
    operand: &records::topology::body_recipe::DesignBodyRecipeOperand,
) -> Result<bool, CodecError> {
    if !operand.references().is_empty() {
        return Ok(true);
    }
    if !matches!(
        operand.owner,
        records::topology::body_recipe::DesignOperandOwner::ScopeReference { .. }
    ) {
        return Ok(false);
    }
    let Some(scope) = scope else {
        return Ok(false);
    };
    if scope.kind() != crate::records::feature::scope::DesignFeatureKind::Combine {
        return Ok(false);
    }
    let Some(operation) = scope.combine_operation() else {
        return Ok(false);
    };
    Ok(operation.tools.first.record_index == operand.record_index()
        || decode.any_by(
            &operation.tools.additional,
            |tool| Ok(tool.record_index == operand.record_index()),
            "find F3D body recipe Combine tool",
        )?)
}

/// The reference at `ordinal` in storage order: plain values, then located ones.
fn reference_member_at<T, O>(
    references: &crate::records::identity::ReferenceRun<T, O>,
    ordinal: usize,
) -> Option<&T> {
    let (plain, located) = references.storage_slices();
    match ordinal.checked_sub(plain.len()) {
        None => plain.get(ordinal),
        Some(located_ordinal) => located
            .get(located_ordinal)
            .map(|reference| &reference.value),
    }
}

use crate::records::topology::extrude_selection::DesignOperandRole;
use std::collections::{HashMap, HashSet};

fn reload_native_arena<'ctx, T: serde::de::DeserializeOwned>(
    decode: &'ctx DecodeContext<'_>,
    ir: &CadIr,
    name: &str,
) -> Result<(cadmpeg_core::decode::ScopedReservation<'ctx>, Vec<T>), CodecError> {
    let (values, storage) = decode.with_scoped_storage("reload F3D validation records", || {
        let Some(namespace) = ir.native.namespace("f3d") else {
            return Ok(Vec::new());
        };
        namespace
            .arena_as_for_decode(decode, name)
            .map_err(CodecError::from)
    })?;
    Ok((storage, values))
}

/// Records of one arena grouped by key, each group in arena order.
type RecordGroups<'a, K, T> = HashMap<K, Vec<&'a T>>;

/// Group `records` by `key` under `storage`, keeping arena order within each
/// group. Records whose key is `None` are left out.
fn group_records<'a, K: Eq + std::hash::Hash + cadmpeg_core::decode::cost::DecodeCost, T>(
    decode: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    records: &'a [T],
    key: impl Fn(&'a T) -> Option<K>,
    operation: &'static str,
) -> Result<RecordGroups<'a, K, T>, CodecError> {
    let mut groups = HashMap::new();
    for record in decode.admit_iter(records, operation)? {
        let Some(key) = key(record) else {
            continue;
        };
        storage.with_storage(|| {
            decode.push_hash_group(&mut groups, key, record, operation, operation)
        })?;
    }
    Ok(groups)
}

/// A lookup set and the reservation held until its final validation consumer.
type ScopedRecordSet<'a, K> = (HashSet<K>, cadmpeg_core::decode::ScopedReservation<'a>);

/// GUID text of at most 38 bytes with ASCII letters lowercased, and its length.
type FoldedGuid = ([u8; 38], u8);

/// Fold `text` for an ASCII case-insensitive GUID key. Within the 38-byte
/// GUID bound, keys preserve ASCII case-insensitive equality and length.
/// A longer text has no key; the fold reads at most 38 bytes.
fn folded_guid(text: &str) -> Option<FoldedGuid> {
    let bytes = text.as_bytes();
    let length = u8::try_from(bytes.len())
        .ok()
        .filter(|length| *length <= 38)?;
    let mut folded = [0; 38];
    for (target, byte) in folded.iter_mut().zip(bytes) {
        *target = byte.to_ascii_lowercase();
    }
    Some((folded, length))
}

/// Read-only indexes over the loaded `f3d` native namespace, shared by the
/// per-family validators. The indexes derive from the namespace; grouped
/// records borrow it for the duration of [`validate_native_charged`]. Scoped
/// reservations hold index storage until the `Ctx` is released.
struct Ctx<'a, 'd> {
    decode: &'a DecodeContext<'d>,
    /// The decoded document, for model-side body, face, and edge identity.
    ir: &'a CadIr,
    /// The loaded native namespace.
    native: &'a native::F3dNative,
    /// Design record headers keyed by `(stream, record_index)`.
    records_by_index: HashMap<(&'a str, u32), &'a records::decal::DesignRecordHeader>,
    /// Byte offsets of Design record headers, keyed by stream.
    header_offsets: HashSet<(&'a str, u64)>,
    /// Construction recipes keyed by recipe id.
    recipes_by_id: HashMap<&'a str, &'a records::recipes::ConstructionRecipe>,
    /// Parameters keyed by `(stream, record_index)`.
    parameters_by_index: HashMap<(&'a str, u32), &'a records::parameters::DesignParameter>,
    /// Every parameter sharing a `(stream, record_index)`, in arena order.
    parameter_groups: RecordGroups<'a, (&'a str, u32), records::parameters::DesignParameter>,
    /// Parameter owners keyed by `(stream, record_index)`.
    owners_by_index: HashMap<(&'a str, u32), &'a records::parameters::DesignParameterOwner>,
    /// Every parameter owner sharing a `(stream, record_index)`, in arena order.
    owner_groups: RecordGroups<'a, (&'a str, u32), records::parameters::DesignParameterOwner>,
    /// Parameter companions keyed by `(stream, record_index)`.
    companions_by_index: HashMap<(&'a str, u32), &'a records::parameters::DesignParameterCompanion>,
    /// Parameter scopes keyed by `(stream, record_index)`.
    scopes_by_index: HashMap<(&'a str, u32), &'a records::feature::scope::DesignParameterScope>,
    /// Every parameter scope sharing a `(stream, record_index)`, in arena order.
    scope_groups: RecordGroups<'a, (&'a str, u32), records::feature::scope::DesignParameterScope>,
    /// Every parameter owner of a `(stream, scope_record_index)`, in arena order.
    scope_owner_groups: RecordGroups<'a, (&'a str, u32), records::parameters::DesignParameterOwner>,
    /// Every construction operand group sharing a `(stream, record_index)`, in arena order.
    operand_group_groups: RecordGroups<
        'a,
        (&'a str, u32),
        records::topology::construction::DesignConstructionOperandGroup,
    >,
    /// Construction operand groups keyed by their owning scope, in arena order.
    operand_groups_by_scope: RecordGroups<
        'a,
        (&'a str, u32),
        records::topology::construction::DesignConstructionOperandGroup,
    >,
    /// Every component occurrence sharing a `(stream, record_index)`, in arena order.
    occurrence_groups: RecordGroups<
        'a,
        (&'a str, u32),
        records::feature::assembly_features::DesignComponentOccurrence,
    >,
    /// Every component occurrence sharing a stream and case-folded occurrence
    /// GUID, in arena order.
    occurrence_guid_groups: RecordGroups<
        'a,
        (&'a str, FoldedGuid),
        records::feature::assembly_features::DesignComponentOccurrence,
    >,
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
    /// Unique ASM states by id; a repeated id keeps a tombstone.
    states_by_id: HashMap<i64, Option<&'a crate::history_records::AsmDeltaState>>,
    /// ASM history bound to each parameter scope with a history state.
    scope_histories: HashMap<String, String>,
    /// Storage of the grouped and record lookup indexes.
    _index_storage: cadmpeg_core::decode::ScopedReservation<'a>,
    /// Storage of the unique state index.
    _state_storage: cadmpeg_core::decode::ScopedReservation<'a>,
}

impl<'a, 'd> Ctx<'a, 'd> {
    fn push_constant_finding(
        &self,
        findings: &mut Vec<Finding>,
        check: Check,
        message: &'static str,
        entity: Option<String>,
    ) -> Result<(), CodecError> {
        self.decode.push_vec(
            findings,
            Finding {
                check,
                severity: Severity::Error,
                message: self
                    .decode
                    .copy_retained_text(message, "retain F3D validation finding message")?,
                entity,
            },
            "collect F3D native validation findings",
        )?;
        Ok(())
    }

    /// Every parameter scope at `(stream, record_index)`, in arena order.
    fn scope_group<'s>(
        &'s self,
        stream: &'s str,
        record_index: u32,
        operation: &'static str,
    ) -> Result<&'s [&'s records::feature::scope::DesignParameterScope], CodecError> {
        Ok(self
            .decode
            .get_hash_map(&self.scope_groups, &(stream, record_index), operation)?
            .map_or(&[], Vec::as_slice))
    }

    /// Every parameter owner at `(stream, record_index)`, in arena order.
    fn owner_group<'s>(
        &'s self,
        stream: &'s str,
        record_index: u32,
        operation: &'static str,
    ) -> Result<&'s [&'s records::parameters::DesignParameterOwner], CodecError> {
        Ok(self
            .decode
            .get_hash_map(&self.owner_groups, &(stream, record_index), operation)?
            .map_or(&[], Vec::as_slice))
    }

    /// Every parameter owner of the scope at `(stream, scope_record_index)`, in arena order.
    fn scope_owners<'s>(
        &'s self,
        stream: &'s str,
        scope_record_index: u32,
        operation: &'static str,
    ) -> Result<&'s [&'s records::parameters::DesignParameterOwner], CodecError> {
        Ok(self
            .decode
            .get_hash_map(
                &self.scope_owner_groups,
                &(stream, scope_record_index),
                operation,
            )?
            .map_or(&[], Vec::as_slice))
    }

    /// Every construction operand group at `(stream, record_index)`, in arena order.
    fn operand_group_records<'s>(
        &'s self,
        stream: &'s str,
        record_index: u32,
        operation: &'static str,
    ) -> Result<&'s [&'s records::topology::construction::DesignConstructionOperandGroup], CodecError>
    {
        Ok(self
            .decode
            .get_hash_map(
                &self.operand_group_groups,
                &(stream, record_index),
                operation,
            )?
            .map_or(&[], Vec::as_slice))
    }

    /// Every component occurrence at `(stream, record_index)`, in arena order.
    fn occurrence_records<'s>(
        &'s self,
        stream: &'s str,
        record_index: u32,
        operation: &'static str,
    ) -> Result<&'s [&'s records::feature::assembly_features::DesignComponentOccurrence], CodecError>
    {
        Ok(self
            .decode
            .get_hash_map(&self.occurrence_groups, &(stream, record_index), operation)?
            .map_or(&[], Vec::as_slice))
    }

    /// Every component occurrence in `stream` whose occurrence GUID equals
    /// `guid` without ASCII case, in arena order.
    fn occurrences_with_guid<'s>(
        &'s self,
        stream: &'s str,
        guid: &str,
        operation: &'static str,
    ) -> Result<&'s [&'s records::feature::assembly_features::DesignComponentOccurrence], CodecError>
    {
        let Some(guid) = folded_guid(guid) else {
            return Ok(&[]);
        };
        Ok(self
            .decode
            .get_hash_map(&self.occurrence_guid_groups, &(stream, guid), operation)?
            .map_or(&[], Vec::as_slice))
    }

    /// Every parameter at `(stream, record_index)`, in arena order.
    fn parameter_group<'s>(
        &'s self,
        stream: &'s str,
        record_index: u32,
        operation: &'static str,
    ) -> Result<&'s [&'s records::parameters::DesignParameter], CodecError> {
        Ok(self
            .decode
            .get_hash_map(&self.parameter_groups, &(stream, record_index), operation)?
            .map_or(&[], Vec::as_slice))
    }

    /// Build every shared index over `native` up front. All builds are pure and
    /// emit no findings, so their eager construction does not affect the
    /// observable finding order. A repeated key keeps its last record in the
    /// single-record indexes; the group indexes keep every record.
    fn new(
        ir: &'a CadIr,
        native: &'a native::F3dNative,
        decode: &'a DecodeContext<'d>,
    ) -> Result<Self, CodecError> {
        let mut storage = decode.reserve_scoped(0, "hold F3D validation indexes")?;
        let records_by_index = storage.with_storage(|| {
            decode.collect_hash_map(
                native
                    .design_record_headers
                    .iter()
                    .map(|record| ((design_stream(&record.id), record.record_index), record)),
                "index F3D design headers",
            )
        })?;
        let header_offsets = storage.with_storage(|| {
            decode.collect_hash_set(
                native
                    .design_record_headers
                    .iter()
                    .map(|record| (design_stream(&record.id), record.byte_offset)),
                "index F3D design header offsets",
            )
        })?;
        let recipes_by_id = storage.with_storage(|| {
            decode.collect_hash_map(
                native
                    .construction_recipes
                    .iter()
                    .map(|recipe| (recipe.id.as_str(), recipe)),
                "index F3D construction recipes",
            )
        })?;
        let parameters_by_index = storage.with_storage(|| {
            decode.collect_hash_map(
                native.design_parameters.iter().map(|parameter| {
                    (
                        (design_stream(&parameter.id), parameter.record_index),
                        parameter,
                    )
                }),
                "index F3D design parameters",
            )
        })?;
        let parameter_groups = group_records(
            decode,
            &mut storage,
            &native.design_parameters,
            |parameter| Some((design_stream(&parameter.id), parameter.record_index)),
            "group F3D design parameters",
        )?;
        let owners_by_index = storage.with_storage(|| {
            decode.collect_hash_map(
                native
                    .design_parameter_owners
                    .iter()
                    .map(|owner| ((design_stream(owner.id()), owner.record_index()), owner)),
                "index F3D parameter owners",
            )
        })?;
        let owner_groups = group_records(
            decode,
            &mut storage,
            &native.design_parameter_owners,
            |owner| Some((design_stream(owner.id()), owner.record_index())),
            "group F3D parameter owners",
        )?;
        let scope_owner_groups = group_records(
            decode,
            &mut storage,
            &native.design_parameter_owners,
            |owner| Some((design_stream(owner.id()), owner.scope_record_index())),
            "group F3D parameter owners by scope",
        )?;
        let companions_by_index = storage.with_storage(|| {
            decode.collect_hash_map(
                native.design_parameter_companions.iter().map(|companion| {
                    (
                        (design_stream(companion.id()), companion.record_index()),
                        companion,
                    )
                }),
                "index F3D parameter companions",
            )
        })?;
        let scopes_by_index = storage.with_storage(|| {
            decode.collect_hash_map(
                native
                    .design_parameter_scopes
                    .iter()
                    .map(|scope| ((design_stream(&scope.id), scope.record_index), scope)),
                "index F3D parameter scopes",
            )
        })?;
        let scope_groups = group_records(
            decode,
            &mut storage,
            &native.design_parameter_scopes,
            |scope| Some((design_stream(&scope.id), scope.record_index)),
            "group F3D parameter scopes",
        )?;
        let entities_by_suffix = storage.with_storage(|| {
            decode.collect_hash_map(
                native.design_entity_headers.iter().map(|entity| {
                    (
                        (design_stream(&entity.id), entity.entity_id.suffix()),
                        entity,
                    )
                }),
                "index F3D entity suffixes",
            )
        })?;
        let sketch_geometry_indices = storage.with_storage(|| {
            decode.collect_hash_set(
                native
                    .sketch_points
                    .iter()
                    .map(|point| (design_stream(&point.id), point.record_index))
                    .chain(
                        native
                            .sketch_curve_identities
                            .iter()
                            .map(|curve| (design_stream(&curve.id), curve.record_index)),
                    ),
                "index F3D sketch geometry",
            )
        })?;
        let mut placements_by_scope = HashMap::new();
        for placement in decode.admit_iter(
            &native.design_sketch_placements,
            "index F3D sketch placements",
        )? {
            let Some(scope_record_index) = placement.scope_record_index else {
                continue;
            };
            storage.with_storage(|| {
                decode.insert_hash_map(
                    &mut placements_by_scope,
                    (design_stream(&placement.id), scope_record_index),
                    placement,
                    "index F3D sketch placements",
                )
            })?;
        }
        let groups_by_index = storage.with_storage(|| {
            decode.collect_hash_map(
                native
                    .design_extrude_selection_groups
                    .iter()
                    .map(|group| ((design_stream(&group.id), group.record_index), group)),
                "index F3D extrude selection groups",
            )
        })?;
        let operand_groups_by_index = storage.with_storage(|| {
            decode.collect_hash_map(
                native
                    .design_construction_operand_groups
                    .iter()
                    .map(|group| ((design_stream(&group.id), group.record_index), group)),
                "index F3D construction operand groups",
            )
        })?;
        let operand_group_groups = group_records(
            decode,
            &mut storage,
            &native.design_construction_operand_groups,
            |group| Some((design_stream(&group.id), group.record_index)),
            "group F3D construction operand groups",
        )?;
        let operand_groups_by_scope = group_records(
            decode,
            &mut storage,
            &native.design_construction_operand_groups,
            |group| Some((design_stream(&group.id), group.scope_record_index)),
            "group F3D operand groups by scope",
        )?;
        let occurrence_groups = group_records(
            decode,
            &mut storage,
            &native.design_component_occurrences,
            |occurrence| Some((design_stream(&occurrence.id), occurrence.record_index)),
            "group F3D component occurrences",
        )?;
        let occurrence_guid_groups = group_records(
            decode,
            &mut storage,
            &native.design_component_occurrences,
            |occurrence| {
                Some((
                    design_stream(&occurrence.id),
                    folded_guid(occurrence.occurrence_guid.as_str())?,
                ))
            },
            "group F3D component occurrences by GUID",
        )?;
        let members_by_slot = storage.with_storage(|| {
            decode.collect_hash_map(
                native
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
                    }),
                "index F3D extrude selection members",
            )
        })?;
        let mut sketch_owner_ids = HashMap::new();
        for header in
            decode.admit_iter(&native.design_entity_headers, "index F3D sketch owner ids")?
        {
            if !header.in_sketch_module() {
                continue;
            }
            let Ok(suffix) = u32::try_from(header.entity_id.suffix()) else {
                continue;
            };
            storage.with_storage(|| {
                decode.insert_hash_map(
                    &mut sketch_owner_ids,
                    (design_stream(&header.id), suffix),
                    header.entity_id.as_str(),
                    "index F3D sketch owner ids",
                )
            })?;
        }
        let (states_by_id, state_storage) = decode.unique_index(
            decode
                .admit_iter(
                    &native.asm_histories,
                    "index F3D validation state histories",
                )?
                .flat_map(|history| history.states.iter().map(|state| (state.state_id, state))),
            "index F3D validation states",
        )?;
        let scope_histories = storage.with_storage(|| {
            history::bind_scope_histories(
                decode,
                &native.design_parameter_scopes,
                &native.design_body_bindings,
                &native.design_body_recipe_operands,
                &native.asm_histories,
            )
        })?;
        Ok(Ctx {
            decode,
            ir,
            native,
            records_by_index,
            header_offsets,
            recipes_by_id,
            parameters_by_index,
            parameter_groups,
            owners_by_index,
            owner_groups,
            companions_by_index,
            scopes_by_index,
            scope_groups,
            scope_owner_groups,
            operand_group_groups,
            operand_groups_by_scope,
            occurrence_groups,
            occurrence_guid_groups,
            entities_by_suffix,
            sketch_geometry_indices,
            placements_by_scope,
            groups_by_index,
            operand_groups_by_index,
            members_by_slot,
            sketch_owner_ids,
            scope_histories,
            states_by_id,
            _index_storage: storage,
            _state_storage: state_storage,
        })
    }
}

/// Validate native records using the source decode budget.
pub(crate) fn validate_native_charged(
    decode: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<Vec<Finding>, CodecError> {
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(Vec::new());
    };
    let native = match decode.with_scoped_storage("load F3D validation records", || {
        native::F3dNative::load_charged(decode, namespace)
    }) {
        Ok(native) => native,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => {
            let mut findings = Vec::new();
            decode.push_vec(
                &mut findings,
                Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: decode.copy_retained_text(
                        "Fusion native namespace does not match the expected arena shape",
                        "retain F3D validation finding message",
                    )?,
                    entity: None,
                },
                "collect F3D native validation findings",
            )?;
            return Ok(findings);
        }
    };
    validate_loaded(decode, ir, &native.0)
}

fn validate_loaded(
    decode: &DecodeContext<'_>,
    ir: &CadIr,
    native: &native::F3dNative,
) -> Result<Vec<Finding>, CodecError> {
    let ctx = Ctx::new(ir, native, decode)?;
    let mut findings = Vec::new();
    let (mut expected_face_operands_storage, mut expected_face_operands) =
        reload_native_arena(decode, ir, "design_face_operands")?;
    expected_face_operands_storage.with_storage(|| {
        history::bind_face_operand_history_candidates(
            decode,
            &mut expected_face_operands,
            &native.design_parameter_scopes,
            &native.design_construction_operand_groups,
            &native.construction_recipes,
            &native.asm_histories,
            &ctx.scope_histories,
        )
    })?;
    let mut face_group_members_storage =
        decode.reserve_scoped(0, "index F3D face group members")?;
    let mut decoded_profile_face_groups = HashSet::new();
    for operand in decode.admit_iter(
        &native.design_face_operands,
        "index F3D decoded profile face groups",
    )? {
        let Some(group_record_index) = operand.group_record_index() else {
            continue;
        };
        face_group_members_storage.with_storage(|| {
            decode.insert_hash_set(
                &mut decoded_profile_face_groups,
                (design_stream(&operand.id), group_record_index),
                "index F3D decoded profile face groups",
            )
        })?;
    }
    let mut face_group_members = std::collections::BTreeSet::new();
    for group in decode.admit_iter(
        &native.design_construction_operand_groups,
        "scan F3D face groups",
    )? {
        let selected = group.extrude_role().is_some_and(|role| {
            matches!(
                role,
                records::topology::extrude_selection::DesignExtrudeOperandRole::Faces(_)
            )
        }) || (group.extrude_role()
            == Some(records::topology::extrude_selection::DesignExtrudeOperandRole::Profile)
            && decode.contains_hash_set(
                &decoded_profile_face_groups,
                &(design_stream(&group.id), group.record_index),
                "find F3D profile face group",
            )?);
        if !selected {
            continue;
        }
        let native_stream = design_stream(&group.id);
        for member in decode.admit_iter(group.members(), "scan F3D face group members")? {
            face_group_members_storage.with_storage(|| {
                decode.insert_btree_set(
                    &mut face_group_members,
                    (native_stream, group.scope_record_index, member.value),
                    "index F3D face group members",
                )
            })?;
        }
    }
    validate_act(&ctx, &mut findings)?;
    validate_body_bindings(&ctx, &mut findings)?;
    validate_body_bounds(&ctx, &mut findings)?;
    validate_canvas_images(&ctx, &mut findings)?;
    validate_decal_images(&ctx, &mut findings)?;
    validate_mesh_features(&ctx, &mut findings)?;
    validate_component_occurrences(&ctx, &mut findings)?;
    validate_configurations(&ctx, &mut findings)?;
    validate_feature_timelines(&ctx, &mut findings)?;
    scopes::validate_parameter_scopes(&ctx, &mut findings)?;
    validate_extrude_selection_groups(&ctx, &mut findings)?;
    validate_construction_operand_groups(&ctx, &mut findings)?;
    validate_path_feature_operand_roles(&ctx, &mut findings)?;
    validate_extrude_parameter_operands(&ctx, &mut findings)?;
    let (fillet_radius_group_records, fillet_radius_group_records_storage) =
        validate_fillet_radius_groups(&ctx, &mut findings)?;
    validate_fillet_operand_groups(&ctx, &mut findings, &fillet_radius_group_records)?;
    drop((
        fillet_radius_group_records,
        fillet_radius_group_records_storage,
    ));
    let (operand_identity_groups, operand_identity_groups_storage) =
        validate_construction_operand_identities(&ctx, &mut findings)?;
    let (edge_identity_records, edge_identity_records_storage) =
        validate_edge_identity_operands(&ctx, &mut findings, &expected_face_operands)?;
    let (body_recipe_operand_records, body_recipe_operand_records_storage) =
        validate_body_recipe_operands(&ctx, &mut findings)?;
    let (edge_operand_records, edge_operand_records_storage) =
        validate_edge_operands(&ctx, &mut findings)?;
    let (edge_treatment_vertex_records, edge_treatment_vertex_records_storage) =
        validate_edge_treatment_vertex_operands(&ctx, &mut findings)?;
    validate_operand_group_carriers(
        &ctx,
        &mut findings,
        &operand_identity_groups,
        &edge_identity_records,
        &body_recipe_operand_records,
        &edge_operand_records,
        &edge_treatment_vertex_records,
    )?;
    validate_extrude_selection_members(&ctx, &mut findings)?;
    validate_entity_selection_operands(&ctx, &mut findings)?;
    validate_extrude_selection_group_members(&ctx, &mut findings)?;
    validate_edge_treatment_groups(
        &ctx,
        &mut findings,
        &edge_operand_records,
        &edge_identity_records,
        &edge_treatment_vertex_records,
    )?;
    drop((operand_identity_groups, operand_identity_groups_storage));
    drop((
        body_recipe_operand_records,
        body_recipe_operand_records_storage,
    ));
    let (face_operand_records, face_operand_records_storage) =
        validate_face_operands(&ctx, &mut findings, &expected_face_operands)?;
    validate_face_group_member_resolution(
        &ctx,
        &mut findings,
        &face_group_members,
        &face_operand_records,
        &native.design_entity_selection_operands,
    )?;
    drop((face_operand_records, face_operand_records_storage));
    drop((edge_identity_records, edge_identity_records_storage));
    drop((edge_operand_records, edge_operand_records_storage));
    drop((
        edge_treatment_vertex_records,
        edge_treatment_vertex_records_storage,
    ));
    validate_face_source_groups(&ctx, &mut findings)?;
    validate_sketch_placements(&ctx, &mut findings)?;
    validate_parameter_owners(&ctx, &mut findings)?;
    validate_parameter_companions(&ctx, &mut findings)?;
    let (dimension_recipe_ids, dimension_recipe_ids_storage) =
        dimensions::validate_dimension_recipe_records(&ctx, &mut findings)?;
    dimensions::validate_dimension_companion_recipes(&ctx, &mut findings, &dimension_recipe_ids)?;
    drop((dimension_recipe_ids, dimension_recipe_ids_storage));
    let (locus_pair_companions, locus_pair_companions_storage) =
        dimensions::validate_dimension_locus_pairs(&ctx, &mut findings)?;
    dimensions::validate_dimension_annotation_frames(&ctx, &mut findings)?;
    dimensions::validate_dimension_presentation_frames(&ctx, &mut findings)?;
    let (locus_group_companions, locus_group_companions_storage) =
        dimensions::validate_dimension_locus_groups(&ctx, &mut findings)?;
    dimensions::validate_dimension_null_locus_pairs(
        &ctx,
        &mut findings,
        &locus_pair_companions,
        &locus_group_companions,
    )?;
    drop((locus_pair_companions, locus_pair_companions_storage));
    drop((locus_group_companions, locus_group_companions_storage));
    validate_parameters(&ctx, &mut findings)?;
    validate_entity_headers(&ctx, &mut findings)?;
    validate_sketch_relations(&ctx, &mut findings)?;
    validate_sketch_geometry_identities(&ctx, &mut findings)?;
    validate_sketch_relation_owners(&ctx, &mut findings)?;
    validate_body_links(&ctx, &mut findings)?;
    validate_subentity_tags(&ctx, &mut findings)?;
    validate_history_graphs(&ctx, &mut findings)?;
    Ok(findings)
}

/// Validate ACT record identity, table/group joins, ordered registries, and the
/// stored document-root discriminator.
fn validate_act(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    let native = ctx.native;
    let mut stream_indexes_storage = ctx
        .decode
        .reserve_scoped(0, "index F3D ACT validation streams")?;
    let mut streams = std::collections::BTreeMap::<&str, &str>::new();
    let mut record_indices = HashSet::new();
    for entity in ctx
        .decode
        .admit_iter(&native.act_entities, "scan F3D act entities")?
    {
        let stream = entity.stream();
        if !ctx
            .decode
            .contains_key_btree_map(&streams, stream, "index F3D ACT streams")?
        {
            stream_indexes_storage.with_storage(|| {
                ctx.decode.insert_btree_map(
                    &mut streams,
                    stream,
                    entity.id().as_str(),
                    "index F3D ACT streams",
                )
            })?;
        }
        let unique_index = stream_indexes_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut record_indices,
                (stream, entity.record_index()),
                "index F3D ACT record indices",
            )
        })?;
        if !unique_index {
            ctx.push_constant_finding(findings, Check::NativeLinks,
                "Fusion ACT entity has an invalid identity, table membership, or change-group frame",
                Some(ctx.decode.copy_retained_text(entity.id(), "retain F3D validation entity")?))?;
        }
    }

    let mut guid_ordinals =
        std::collections::BTreeMap::<&str, (std::collections::BTreeSet<u32>, &str)>::new();
    let mut guid_offsets = HashSet::new();
    for guid in ctx
        .decode
        .admit_iter(&native.act_guids, "scan F3D act guids")?
    {
        let stream = guid.stream();
        if !ctx
            .decode
            .contains_key_btree_map(&streams, stream, "index F3D ACT streams")?
        {
            stream_indexes_storage.with_storage(|| {
                ctx.decode.insert_btree_map(
                    &mut streams,
                    stream,
                    guid.id().as_str(),
                    "index F3D ACT streams",
                )
            })?;
        }
        if !ctx.decode.contains_key_btree_map(
            &guid_ordinals,
            stream,
            "index F3D ACT GUID streams",
        )? {
            stream_indexes_storage.with_storage(|| {
                ctx.decode.insert_btree_map(
                    &mut guid_ordinals,
                    stream,
                    (std::collections::BTreeSet::new(), guid.id().as_str()),
                    "index F3D ACT GUID streams",
                )
            })?;
        }
        let unique_ordinal = stream_indexes_storage.with_storage(|| {
            ctx.decode.insert_btree_set(
                &mut ctx
                    .decode
                    .get_mut_btree_map(
                        &mut guid_ordinals,
                        stream,
                        "find F3D ACT GUID stream ordinals",
                    )?
                    .ok_or_else(|| CodecError::malformed("F3D ACT GUID stream index missing"))?
                    .0,
                guid.ordinal,
                "index F3D ACT GUID ordinals",
            )
        })?;
        let unique_offset = stream_indexes_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut guid_offsets,
                (stream, guid.byte_offset()),
                "index F3D ACT GUID offsets",
            )
        })?;
        let valid = unique_offset && unique_ordinal;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ACT GUID-pool entry has an invalid identity, ordinal, offset, or GUID",
                Some(
                    ctx.decode
                        .copy_retained_text(guid.id(), "retain F3D validation entity")?,
                ),
            )?;
        }
    }

    let mut table_reference_ordinals =
        std::collections::BTreeMap::<&str, (std::collections::BTreeSet<u32>, &str)>::new();
    let mut table_reference_offsets = HashSet::new();
    for reference in ctx.decode.admit_iter(
        &native.act_table_references,
        "scan F3D act table references",
    )? {
        let stream = reference.stream();
        if !ctx
            .decode
            .contains_key_btree_map(&streams, stream, "index F3D ACT streams")?
        {
            stream_indexes_storage.with_storage(|| {
                ctx.decode.insert_btree_map(
                    &mut streams,
                    stream,
                    reference.id().as_str(),
                    "index F3D ACT streams",
                )
            })?;
        }
        if !ctx.decode.contains_key_btree_map(
            &table_reference_ordinals,
            stream,
            "index F3D ACT table streams",
        )? {
            stream_indexes_storage.with_storage(|| {
                ctx.decode.insert_btree_map(
                    &mut table_reference_ordinals,
                    stream,
                    (std::collections::BTreeSet::new(), reference.id().as_str()),
                    "index F3D ACT table streams",
                )
            })?;
        }
        let unique_ordinal = stream_indexes_storage.with_storage(|| {
            ctx.decode.insert_btree_set(
                &mut ctx
                    .decode
                    .get_mut_btree_map(
                        &mut table_reference_ordinals,
                        stream,
                        "find F3D ACT table stream ordinals",
                    )?
                    .ok_or_else(|| CodecError::malformed("F3D ACT table stream index missing"))?
                    .0,
                reference.ordinal,
                "index F3D ACT table ordinals",
            )
        })?;
        let unique_offset = stream_indexes_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut table_reference_offsets,
                (stream, reference.byte_offset()),
                "index F3D ACT table offsets",
            )
        })?;
        let valid = unique_ordinal && unique_offset;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ACT table reference has an invalid identity, ordinal, or offset",
                Some(
                    ctx.decode
                        .copy_retained_text(reference.id(), "retain F3D validation entity")?,
                ),
            )?;
        }
    }

    let mut registry_ordinals =
        std::collections::BTreeMap::<&str, (std::collections::BTreeSet<u32>, &str)>::new();
    let mut registry_offsets = HashSet::new();
    let mut registry_names = HashSet::new();
    for channel in ctx.decode.admit_iter(
        &native.act_registry_channels,
        "scan F3D act registry channels",
    )? {
        let stream = channel.stream();
        if !ctx
            .decode
            .contains_key_btree_map(&streams, stream, "index F3D ACT streams")?
        {
            stream_indexes_storage.with_storage(|| {
                ctx.decode.insert_btree_map(
                    &mut streams,
                    stream,
                    channel.id().as_str(),
                    "index F3D ACT streams",
                )
            })?;
        }
        if !ctx.decode.contains_key_btree_map(
            &registry_ordinals,
            stream,
            "index F3D ACT registry streams",
        )? {
            stream_indexes_storage.with_storage(|| {
                ctx.decode.insert_btree_map(
                    &mut registry_ordinals,
                    stream,
                    (std::collections::BTreeSet::new(), channel.id().as_str()),
                    "index F3D ACT registry streams",
                )
            })?;
        }
        let unique_ordinal = stream_indexes_storage.with_storage(|| {
            ctx.decode.insert_btree_set(
                &mut ctx
                    .decode
                    .get_mut_btree_map(
                        &mut registry_ordinals,
                        stream,
                        "find F3D ACT registry stream ordinals",
                    )?
                    .ok_or_else(|| CodecError::malformed("F3D ACT registry stream index missing"))?
                    .0,
                channel.ordinal,
                "index F3D ACT registry ordinals",
            )
        })?;
        let unique_offset = stream_indexes_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut registry_offsets,
                (stream, channel.byte_offset()),
                "index F3D ACT registry offsets",
            )
        })?;
        let unique_name = stream_indexes_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut registry_names,
                (stream, channel.name()),
                "index F3D ACT registry names",
            )
        })?;
        let valid = unique_offset && unique_name && unique_ordinal;
        if !valid {
            ctx.push_constant_finding(findings, Check::NativeLinks,
                "Fusion ACT channel-registry entry has an invalid identity, ordinal, offset, name, or GUID",
                Some(ctx.decode.copy_retained_text(channel.id(), "retain F3D validation entity")?))?;
        }
    }

    let mut root_counts = HashMap::<&str, usize>::new();
    for root in ctx
        .decode
        .admit_iter(&native.act_root_components, "scan F3D act root components")?
    {
        let stream = root.stream();
        if !ctx
            .decode
            .contains_key_btree_map(&streams, stream, "index F3D ACT streams")?
        {
            stream_indexes_storage.with_storage(|| {
                ctx.decode.insert_btree_map(
                    &mut streams,
                    stream,
                    root.id().as_str(),
                    "index F3D ACT streams",
                )
            })?;
        }
        if !ctx
            .decode
            .contains_key_hash_map(&root_counts, stream, "index F3D ACT root counts")?
        {
            stream_indexes_storage.with_storage(|| {
                ctx.decode.insert_hash_map(
                    &mut root_counts,
                    stream,
                    Default::default(),
                    "index F3D ACT root counts",
                )
            })?;
        }
        let root_count = ctx
            .decode
            .get_mut_hash_map(&mut root_counts, stream, "index F3D ACT root counts")?
            .ok_or_else(|| CodecError::malformed("F3D validation default index missing"))?;
        *root_count = root_count.checked_add(1).ok_or_else(|| {
            ctx.decode
                .refuse_codec_limit("count F3D ACT roots", u64::MAX - 1, u64::MAX)
        })?;
        let unique_record_index = stream_indexes_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut record_indices,
                (stream, root.record_index),
                "index F3D ACT record indices",
            )
        })?;
        if !unique_record_index {
            ctx.push_constant_finding(findings, Check::NativeLinks,
                "Fusion ACT root component has an invalid identity, frame, or tracked-entity reference",
                Some(ctx.decode.copy_retained_text(root.id(), "retain F3D validation entity")?))?;
        }
    }

    for (stream, witness) in ctx
        .decode
        .admit_iter(&streams, "scan F3D ACT stream witnesses")?
        .map(|(stream, witness)| (*stream, *witness))
    {
        if ctx
            .decode
            .get_hash_map(&root_counts, stream, "find F3D ACT root count")?
            .copied()
            != Some(1)
        {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ACT stream does not have exactly one document-root component link",
                Some(
                    ctx.decode
                        .copy_retained_text(witness, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    for (ordinals, witness, family) in ctx
        .decode
        .admit_iter(&guid_ordinals, "scan F3D ACT GUID ordinal sets")?
        .map(|(_, (ordinals, witness))| (ordinals, *witness, "GUID pool"))
        .chain(
            ctx.decode
                .admit_iter(&table_reference_ordinals, "scan F3D ACT table ordinal sets")?
                .map(|(_, (ordinals, witness))| (ordinals, *witness, "table reference")),
        )
        .chain(
            ctx.decode
                .admit_iter(&registry_ordinals, "scan F3D ACT registry ordinal sets")?
                .map(|(_, (ordinals, witness))| (ordinals, *witness, "channel registry")),
        )
    {
        // Distinct ordinals are contiguous from zero exactly when the largest
        // is one less than their count.
        let contiguous = match id_from_index(ordinals.len()) {
            Some(length) => {
                ordinals.last().and_then(|maximum| maximum.checked_add(1)) == Some(length)
            }
            None => false,
        };
        if !contiguous {
            let message = match family {
                "GUID pool" => "Fusion ACT GUID pool ordinals are not contiguous from zero",
                "table reference" => {
                    "Fusion ACT table reference ordinals are not contiguous from zero"
                }
                _ => "Fusion ACT channel registry ordinals are not contiguous from zero",
            };
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                message,
                Some(
                    ctx.decode
                        .copy_retained_text(witness, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate unique configuration entries and a single authored table.
fn validate_configurations(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut entry_names_storage = ctx
        .decode
        .reserve_scoped(0, "index F3D configuration entry names")?;
    let mut entry_names = HashSet::new();
    for configuration in ctx.decode.admit_iter(
        &ctx.native.design_configurations,
        "scan F3D design configurations",
    )? {
        let name = configuration.entry_name().as_str();
        if !entry_names_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut entry_names,
                name,
                "index F3D configuration entry names",
            )
        })? {
            let id = {
                let decode = ctx.decode;
                configuration.id_charged(decode)?
            };
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design configuration entry name is duplicated",
                Some(id),
            )?;
        }
    }
    let configurations = &ctx.native.design_configurations;
    let first = ctx.decode.position_by(
        configurations,
        |configuration| Ok(!configuration.variants().is_empty()),
        "find F3D authored configuration table",
    )?;
    let Some(first) = first else {
        return Ok(());
    };
    if ctx.decode.any_by(
        &configurations[first + 1..],
        |configuration| Ok(!configuration.variants().is_empty()),
        "find F3D second authored configuration table",
    )? {
        let id = Some(configurations[first].id_charged(ctx.decode)?);
        ctx.push_constant_finding(
            findings,
            Check::NativeLinks,
            "Fusion Design configurations have no single authored table order",
            id,
        )?;
    }
    Ok(())
}

/// Validate authored Design timeline order and its exact type and scope joins.
fn validate_feature_timelines(ctx: &Ctx, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    let native = ctx.native;
    let mut timeline_indexes_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D feature timeline indexes")?;
    let mut type_ordinals = HashMap::<&str, u32>::new();
    let mut timeline_ordinals = HashMap::<&str, u32>::new();
    let mut entity_type_counts = HashMap::<(&str, u64), usize>::new();
    let mut expected = std::collections::BTreeMap::<(&str, u64), (String, u32, bool, &str)>::new();
    let mut design_types = timeline_indexes_storage.with_storage(|| {
        ctx.decode.collect_vec(
            native.design_types.iter().map(|value| {
                (
                    (
                        ids::native_stream(value.id()).unwrap_or_default(),
                        value.byte_offset,
                    ),
                    value,
                )
            }),
            "order F3D feature timeline types",
        )
    })?;
    ctx.decode.stable_sort_by_key(
        &mut design_types,
        |(key, _)| *key,
        Ord::cmp,
        "f3d feature timeline types sort",
    )?;
    for (_, design_type) in ctx
        .decode
        .admit_iter(&design_types, "scan F3D design types")?
    {
        let Some(meta_stream) = ids::native_stream(design_type.id()) else {
            continue;
        };
        let Some(segment) = ids::design_segment(design_type.id()) else {
            continue;
        };
        if !ctx.decode.contains_key_hash_map(
            &type_ordinals,
            meta_stream,
            "index F3D feature timeline type ordinals",
        )? {
            timeline_indexes_storage.with_storage(|| {
                ctx.decode.insert_hash_map(
                    &mut type_ordinals,
                    meta_stream,
                    Default::default(),
                    "index F3D feature timeline type ordinals",
                )
            })?;
        }
        let type_ordinal = ctx
            .decode
            .get_mut_hash_map(
                &mut type_ordinals,
                meta_stream,
                "index F3D feature timeline type ordinals",
            )?
            .ok_or_else(|| CodecError::malformed("F3D validation default index missing"))?;
        let class_tag = match type_ordinal.checked_add(256) {
            Some(tag) => Some(timeline_indexes_storage.with_storage(|| {
                ctx.decode
                    .format_retained(format_args!("{tag}"), "format F3D timeline class tag")
            })?),
            None => None,
        };
        *type_ordinal = type_ordinal.checked_add(1).ok_or_else(|| {
            CodecError::Malformed("F3D feature timeline type ordinal overflows".into())
        })?;
        let (entity_ids, located_entity_ids) = design_type.entities.storage_slices();
        for entity_id in ctx
            .decode
            .admit_iter(entity_ids, "scan F3D timeline entity IDs")?
            .chain(
                ctx.decode
                    .admit_iter(located_entity_ids, "scan F3D timeline located entity IDs")?
                    .map(|row| &row.value),
            )
        {
            if !ctx.decode.contains_key_hash_map(
                &entity_type_counts,
                &(segment, *entity_id),
                "index F3D feature timeline entity types",
            )? {
                timeline_indexes_storage.with_storage(|| {
                    ctx.decode.insert_hash_map(
                        &mut entity_type_counts,
                        (segment, *entity_id),
                        Default::default(),
                        "index F3D feature timeline entity types",
                    )
                })?;
            }
            let count = ctx
                .decode
                .get_mut_hash_map(
                    &mut entity_type_counts,
                    &(segment, *entity_id),
                    "index F3D feature timeline entity types",
                )?
                .ok_or_else(|| CodecError::malformed("F3D validation default index missing"))?;
            *count = count.checked_add(1).ok_or_else(|| {
                ctx.decode.refuse_codec_limit(
                    "count F3D feature timeline entity types",
                    u64::MAX - 1,
                    u64::MAX,
                )
            })?;
        }
        if folded_guid(design_type.type_guid.as_str())
            != folded_guid(crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID)
        {
            continue;
        }
        if !ctx.decode.contains_key_hash_map(
            &timeline_ordinals,
            segment,
            "index F3D feature timeline source ordinals",
        )? {
            timeline_indexes_storage.with_storage(|| {
                ctx.decode.insert_hash_map(
                    &mut timeline_ordinals,
                    segment,
                    Default::default(),
                    "index F3D feature timeline source ordinals",
                )
            })?;
        }
        let source_ordinal = ctx
            .decode
            .get_mut_hash_map(
                &mut timeline_ordinals,
                segment,
                "index F3D feature timeline source ordinals",
            )?
            .ok_or_else(|| CodecError::malformed("F3D validation default index missing"))?;
        let mut type_validity = None;
        for entity_id in ctx
            .decode
            .admit_iter(entity_ids, "scan F3D timeline entity IDs")?
            .chain(
                ctx.decode
                    .admit_iter(located_entity_ids, "scan F3D timeline located entity IDs")?
                    .map(|row| &row.value),
            )
        {
            let valid_type = match type_validity {
                Some(valid_type) => valid_type,
                None => {
                    let valid_type = crate::design::decode::meta::is_supported_feature_timeline_type(
                        design_type,
                    ) && match class_tag.as_ref() {
                        Some(tag) => {
                            let mut validation_storage = ctx
                                .decode
                                .reserve_scoped(0, "hold F3D timeline class tag validation")?;
                            records::references::DesignClassTag::try_from(
                                validation_storage.with_storage(|| {
                                    ctx.decode
                                        .copy_retained_text(tag, "copy F3D timeline type class tag")
                                })?,
                            )
                            .is_ok()
                        }
                        None => false,
                    };
                    type_validity = Some(valid_type);
                    valid_type
                }
            };
            let class_tag = match class_tag.as_ref() {
                Some(tag) => timeline_indexes_storage.with_storage(|| {
                    ctx.decode
                        .copy_retained_text(tag, "copy F3D expected timeline class tag")
                })?,
                None => continue,
            };
            if timeline_indexes_storage
                .with_storage(|| {
                    ctx.decode.insert_btree_map(
                        &mut expected,
                        (segment, *entity_id),
                        (
                            class_tag,
                            *source_ordinal,
                            valid_type,
                            design_type.id().as_str(),
                        ),
                        "index F3D expected feature timelines",
                    )
                })?
                .is_some()
            {
                ctx.push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design feature-timeline type repeats an entity identity",
                    Some(
                        ctx.decode
                            .copy_retained_text(design_type.id(), "retain F3D validation entity")?,
                    ),
                )?;
            }
            *source_ordinal = source_ordinal.checked_add(1).ok_or_else(|| {
                CodecError::Malformed("F3D feature timeline source ordinal overflows".into())
            })?;
        }
    }

    let mut actual = timeline_indexes_storage.with_storage(|| {
        ctx.decode.collect_vec(
            native.design_feature_timelines.iter(),
            "order F3D feature timeline records",
        )
    })?;
    ctx.decode.stable_sort_by_key(
        &mut actual,
        |value| (value.segment(), value.source_ordinal),
        Ord::cmp,
        "f3d feature timeline records sort",
    )?;
    let mut actual_records = HashSet::<(&str, u64)>::new();
    let mut item_records = HashSet::<(&str, u64)>::new();
    for timeline in ctx
        .decode
        .admit_iter(&actual, "scan F3D feature timeline records")?
    {
        let segment = timeline.segment();
        let expected_type = ctx.decode.get_btree_map(
            &expected,
            &(segment, timeline.record_index.get()),
            "find F3D expected timeline type",
        )?;
        let unique_record = timeline_indexes_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut actual_records,
                (segment, timeline.record_index.get()),
                "index F3D feature timeline record identities",
            )
        })?;
        let record_valid = match expected_type {
            Some((class_tag, source_ordinal, valid_type, _)) => {
                *valid_type
                    && (timeline.class_tag.as_str() == class_tag.as_str())
                    && timeline.source_ordinal == *source_ordinal
            }
            None => false,
        } && ctx.decode.get_hash_map(
            &entity_type_counts,
            &(segment, timeline.record_index.get()),
            "find F3D timeline record type count",
        )? == Some(&1)
            && ctx.decode.get_hash_map(
                &entity_type_counts,
                &(segment, timeline.context_record_index.get()),
                "find F3D timeline context type count",
            )? == Some(&1)
            && unique_record;
        let mut items_valid = true;
        for item in ctx
            .decode
            .admit_iter(timeline.frame().items(), "scan F3D timeline items")?
            .map(|item| item.value)
        {
            items_valid &= ctx.decode.get_hash_map(
                &entity_type_counts,
                &(segment, item),
                "find F3D timeline item type count",
            )? == Some(&1)
                && timeline_indexes_storage.with_storage(|| {
                    ctx.decode.insert_hash_set(
                        &mut item_records,
                        (segment, item),
                        "index F3D feature timeline item identities",
                    )
                })?;
        }
        if !record_valid || !items_valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design feature timeline has an invalid typed frame",
                Some(
                    ctx.decode
                        .copy_retained_text(timeline.id(), "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    for ((segment, entity_id), (_, _, _, type_id)) in ctx
        .decode
        .admit_iter(&expected, "scan F3D expected feature timelines")?
    {
        if !ctx.decode.contains_hash_set(
            &actual_records,
            &(*segment, *entity_id),
            "find F3D actual timeline record",
        )? {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design feature-timeline type has no decoded record",
                Some(
                    ctx.decode
                        .copy_retained_text(type_id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }

    let mut scope_positions = HashMap::<&str, u64>::new();
    match ctx
        .decode
        .with_scoped_storage("hold F3D authored scope ordinals", || {
            crate::design::feature_project::authored_scope_ordinals_per_stream(
                ctx.decode,
                &native.design_parameter_scopes,
                &native.design_feature_timelines,
            )
        }) {
        Ok((authored, _authored_storage)) => {
            for scope in ctx.decode.admit_iter(
                &native.design_parameter_scopes,
                "scan F3D design parameter scopes",
            )? {
                let stream = design_stream(&scope.id);
                let Some(position) = ctx.decode.get_hash_map(
                    &authored,
                    &(stream, scope.record_index),
                    "find F3D authored scope ordinal",
                )?
                else {
                    continue;
                };
                timeline_indexes_storage.with_storage(|| {
                    ctx.decode.insert_hash_map(
                        &mut scope_positions,
                        scope.id.as_str(),
                        *position,
                        "index F3D feature timeline scope positions",
                    )
                })?;
            }
        }
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => {
            let entity = native
                .design_parameter_scopes
                .first()
                .map(|scope| {
                    ctx.decode
                        .copy_retained_text(&scope.id, "retain F3D validation entity")
                })
                .transpose()?;
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design scopes have no complete authored order",
                entity,
            )?;
        }
    }

    let (scope_history, _scope_history_storage) =
        ctx.decode
            .with_scoped_storage("hold F3D timeline scope history", || {
                crate::design::feature_project::ScopeHistoryGraph::new(
                    ctx.decode,
                    &native.design_parameter_scopes,
                    &native.design_body_bindings,
                    &native.design_body_recipe_operands,
                    &native.design_component_naming_spaces,
                    &native.asm_histories,
                )
            })?;
    for scope in ctx.decode.admit_iter(
        &native.design_parameter_scopes,
        "scan F3D design parameter scopes",
    )? {
        let Some(position) = ctx
            .decode
            .get_hash_map(
                &scope_positions,
                scope.id.as_str(),
                "find F3D scope timeline position",
            )?
            .copied()
        else {
            continue;
        };
        match scope_history.predecessor(ctx.decode, scope, |candidate| {
            ctx.decode.contains_key_hash_map(
                &scope_positions,
                candidate.id.as_str(),
                "find F3D projected predecessor scope",
            )
        }) {
            Ok(crate::design::feature_project::ScopeHistoryPredecessor::Scope(predecessor)) => {
                if ctx
                    .decode
                    .get_hash_map(
                        &scope_positions,
                        predecessor.id.as_str(),
                        "find F3D predecessor timeline position",
                    )?
                    .is_some_and(|predecessor| *predecessor >= position)
                {
                    ctx.push_constant_finding(
                        findings,
                        Check::NativeLinks,
                        "Fusion Design history edge runs forward in its feature timeline",
                        Some(
                            ctx.decode
                                .copy_retained_text(&scope.id, "retain F3D validation entity")?,
                        ),
                    )?;
                }
            }
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(_) => ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design scope history-state dependency is cyclic",
                Some(
                    ctx.decode
                        .copy_retained_text(&scope.id, "retain F3D validation entity")?,
                ),
            )?,
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

/// Neutral features grouped by native reference, in model order.
type NeutralFeatureGroups<'a> = HashMap<&'a str, Vec<&'a cadmpeg_ir::features::Feature>>;

/// The neutral features whose native reference is `native_ref`, in model
/// order. The grouping is built on first use under `storage`.
fn neutral_features_with_native_ref<'g, 'a>(
    ctx: &Ctx<'a, '_>,
    groups: &'g mut Option<NeutralFeatureGroups<'a>>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    native_ref: &str,
) -> Result<&'g [&'a cadmpeg_ir::features::Feature], CodecError> {
    const OPERATION: &str = "group F3D neutral features by native reference";
    let groups = match groups {
        Some(groups) => groups,
        None => {
            let mut built = HashMap::new();
            for feature in ctx.decode.admit_iter(&ctx.ir.model.features, OPERATION)? {
                let Some(feature_ref) = feature.native_ref.as_deref() else {
                    continue;
                };
                storage.with_storage(|| {
                    ctx.decode.push_hash_group(
                        &mut built,
                        feature_ref,
                        feature,
                        OPERATION,
                        "collect F3D neutral feature group members",
                    )
                })?;
            }
            groups.insert(built)
        }
    };
    Ok(ctx
        .decode
        .get_hash_map(
            groups,
            native_ref,
            "find F3D neutral features by native reference",
        )?
        .map_or(&[], Vec::as_slice))
}

/// Validate complete `Base Mesh Feature` record graphs and their neutral links.
fn validate_mesh_features(ctx: &Ctx, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    if ctx.native.design_mesh_features.is_empty() {
        return Ok(());
    }
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
    let mut record_indexes_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D mesh record indexes")?;
    let mut body_owner_records = HashMap::new();
    let mut texture_table_records = HashSet::new();
    let mut filename_records = HashMap::new();
    let mut projected_tessellations = HashSet::new();
    let asset_ids = record_indexes_storage.with_storage(|| {
        ctx.decode.collect_hash_set(
            ctx.ir.model.assets.iter().map(|asset| &asset.id),
            "index F3D mesh asset IDs",
        )
    })?;
    let tessellation_ids = record_indexes_storage.with_storage(|| {
        ctx.decode.collect_hash_set(
            ctx.ir
                .model
                .tessellations
                .iter()
                .map(|tessellation| tessellation.id.as_str()),
            "index F3D mesh tessellation IDs",
        )
    })?;
    let mut neutral_features = None;
    for feature in ctx.decode.admit_iter(
        &ctx.native.design_mesh_features,
        "scan F3D design mesh features",
    )? {
        let stream = design_stream(&feature.id);
        let scope = ctx.decode.get_hash_map(
            &ctx.scopes_by_index,
            &(stream, feature.scope().record().record_index()),
            "find F3D mesh scope",
        )?;
        let mut valid = record_indexes_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut feature_ids,
                feature.id.as_str(),
                "index F3D mesh feature IDs",
            )
        })? && record_indexes_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut scope_records,
                (stream, feature.scope().record().record_index()),
                "index F3D mesh scope records",
            )
        })? && record_indexes_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut collection_records,
                (stream, feature.collection().record().record_index()),
                "index F3D mesh collection records",
            )
        })? && record_indexes_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut texture_table_records,
                (stream, feature.texture_table.record().record_index()),
                "index F3D mesh texture tables",
            )
        })? && record_indexes_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut collection_owner_records,
                (stream, feature.collection_owner.record().record_index()),
                "index F3D mesh collection owners",
            )
        })? && feature
            .scope()
            .record()
            .byte_offset()
            .checked_add(scope.map_or(0, |scope| scope.frame_length()))
            == Some(feature.scope().base_record().byte_offset())
            && mesh_record_offset_is(
                feature.collection_owner.record(),
                262,
                feature.collection_owner.backlink_offset(),
            )
            && match scope {
                Some(scope) => {
                    (scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::BaseMeshFeature)
                        && scope.byte_offset() == feature.scope().record().byte_offset()
                        && scope.paired_byte_offset() == feature.scope().base_record().byte_offset()
                }
                None => false,
            };

        let (mut resources, _resources_storage) =
            ctx.decode
                .with_scoped_storage("collect F3D mesh texture resources", || {
                    ctx.decode.collect_vec(
                        feature.texture_table.resources().iter(),
                        "collect F3D mesh texture resources",
                    )
                })?;
        ctx.decode.stable_sort_by(
            &mut resources,
            |value| &value.filename_ordinal,
            Ord::cmp,
            "f3d mesh texture resources sort",
        )?;
        let resources_valid = ctx.decode.all_by(
            &resources,
            |resource| {
                let filename_key = (stream, resource.file.record().record_index());
                let filename_record_consistent = match ctx.decode.get_hash_map(
                    &filename_records,
                    &filename_key,
                    "find F3D mesh filename record",
                )? {
                    Some(record) => *record == resource.file.record(),
                    None => {
                        record_indexes_storage.with_storage(|| {
                            ctx.decode.insert_hash_map(
                                &mut filename_records,
                                filename_key,
                                resource.file.record(),
                                "index F3D mesh filename records",
                            )
                        })?;
                        true
                    }
                };
                Ok(filename_record_consistent
                    && ctx.decode.contains_hash_set(
                        &asset_ids,
                        &resource.asset,
                        "find F3D mesh resource asset",
                    )?)
            },
            "scan F3D mesh texture resources",
        )?;
        valid &= resources_valid;

        for body in ctx
            .decode
            .admit_iter(feature.bodies(), "scan F3D mesh feature bodies")?
        {
            let owner_key = (stream, body.owner_record.record_index());
            let owner_consistent = match ctx.decode.get_hash_map(
                &body_owner_records,
                &owner_key,
                "find F3D mesh owner record",
            )? {
                Some(record) => *record == &body.owner_record,
                None => {
                    record_indexes_storage.with_storage(|| {
                        ctx.decode.insert_hash_map(
                            &mut body_owner_records,
                            owner_key,
                            &body.owner_record,
                            "index F3D mesh body owner records",
                        )
                    })?;
                    true
                }
            };
            let body_valid = record_indexes_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut body_records,
                    (stream, body.placement.record().record_index()),
                    "index F3D mesh body records",
                )
            })? && record_indexes_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut entry_records,
                    (stream, body.entry.record().record_index()),
                    "index F3D mesh entry records",
                )
            })? && record_indexes_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut guid_records,
                    (stream, body.guid.record().record_index()),
                    "index F3D mesh GUID records",
                )
            })? && record_indexes_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut wrapper_records,
                    (stream, body.wrapper_record.record_index()),
                    "index F3D mesh wrapper records",
                )
            })? && record_indexes_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut state_records,
                    (stream, body.scene_state.record().record_index()),
                    "index F3D mesh scene states",
                )
            })? && record_indexes_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut node_records,
                    (stream, body.scene_node.record_index()),
                    "index F3D mesh scene nodes",
                )
            })? && record_indexes_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut auxiliary_records,
                    (stream, body.scene_auxiliary_record.record_index()),
                    "index F3D mesh scene auxiliary records",
                )
            })? && owner_consistent
                && body.scene_node.frame_length() == 133;
            let projection_valid = if body_valid {
                match body.tessellation_id.as_deref() {
                    Some(id) => {
                        ctx.decode.contains_hash_set(
                            &tessellation_ids,
                            id,
                            "find F3D mesh tessellation",
                        )? && record_indexes_storage.with_storage(|| {
                            ctx.decode.insert_hash_set(
                                &mut projected_tessellations,
                                id,
                                "index F3D mesh projected tessellations",
                            )
                        })?
                    }
                    None => true,
                }
            } else {
                false
            };
            valid &= body_valid && projection_valid;
        }
        if ctx.decode.any_by(
            feature.bodies(),
            |body| Ok(body.tessellation_id.is_some()),
            "find F3D projected mesh body",
        )? {
            valid &= match scope {
                Some(scope) => ctx.decode.any_by(neutral_features_with_native_ref(ctx, &mut neutral_features, &mut record_indexes_storage, scope.id.as_str())?, |neutral| {
                    Ok(matches!(
                            neutral.evaluation.definition(),
                            cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::MeshImport { tessellations })
                                if {
                                    let mut expected = feature.bodies().iter();
                                    ctx.decode.all_by(tessellations.as_slice(), |id| {
                                        match ctx.decode.find_map(&mut expected,
    |body| Ok(body.tessellation_id.as_deref()),
    "scan F3D projected mesh bodies")? {
                                            Some(expected) => ctx.decode.equal(id.as_str(), expected, "compare F3D projected mesh tessellation IDs"),
                                            None => Ok(false),
                                        }
                                    }, "scan F3D neutral mesh tessellation IDs")?
                                        && ctx.decode.find_map(&mut expected,
    |body| Ok(body.tessellation_id.as_deref()),
    "scan F3D projected mesh bodies")?.is_none()
                                }
                        ))
                }, "find F3D neutral mesh feature")?,
                None => false,
            };
        }

        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design mesh feature has an invalid frame or object graph",
                Some(
                    ctx.decode
                        .copy_retained_text(&feature.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate Canvas scope and Design object joins.
fn validate_canvas_images(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    if native.design_canvas_images.is_empty() {
        return Ok(());
    }
    let mut storage = ctx
        .decode
        .reserve_scoped(0, "index F3D Canvas validation records")?;
    let mut scope_bindings = HashSet::new();
    let mut geometry_records = HashSet::new();
    let mut geometry_entities = HashSet::new();
    let mut component_entities = HashSet::new();
    for design_type in ctx
        .decode
        .admit_iter(&native.design_types, "scan F3D Canvas entity types")?
    {
        let (entities, operation) = match design_type.module.as_str() {
            records::entity_header::DESIGN_MODULE_BODY
            | records::entity_header::DESIGN_MODULE_GEOMETRY => {
                (&mut geometry_entities, "index F3D Canvas geometry entities")
            }
            records::entity_header::DESIGN_MODULE_FUSION
            | records::entity_header::DESIGN_MODULE_COMPONENT => (
                &mut component_entities,
                "index F3D Canvas component entities",
            ),
            _ => continue,
        };
        let segment = ids::design_segment(design_type.id());
        let (entity_values, located_entity_values) = design_type.entities.storage_slices();
        for suffix in ctx
            .decode
            .admit_iter(entity_values, "scan F3D Canvas entity type members")?
            .chain(
                ctx.decode
                    .admit_iter(
                        located_entity_values,
                        "scan F3D Canvas entity type located members",
                    )?
                    .map(|row| &row.value),
            )
        {
            storage.with_storage(|| {
                ctx.decode
                    .insert_hash_set(entities, (segment, *suffix), operation)
            })?;
        }
    }
    for image in ctx.decode.admit_iter(
        &native.design_canvas_images,
        "scan F3D design canvas images",
    )? {
        let native_stream = design_stream(&image.id);
        let design_segment = ids::design_segment(&image.id);
        let scope = ctx.decode.get_hash_map(
            &ctx.scopes_by_index,
            &(native_stream, image.scope_record_index),
            "find F3D Canvas scope",
        )?;
        let scope_valid = match scope {
            Some(scope) => {
                scope.kind() == crate::records::feature::scope::DesignFeatureKind::Canvas
            }
            None => false,
        };
        let scope_unique = if scope_valid {
            storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut scope_bindings,
                    (native_stream, image.scope_record_index),
                    "index F3D Canvas scopes",
                )
            })?
        } else {
            false
        };
        let geometry_unique = if scope_unique {
            storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut geometry_records,
                    (native_stream, image.geometry().record_index()),
                    "index F3D Canvas geometry records",
                )
            })?
        } else {
            false
        };
        let valid = scope_valid
            && scope_unique
            && geometry_unique
            && scope.is_some_and(|scope| scope.byte_offset() == image.scope_byte_offset())
            && ctx.decode.contains_hash_set(
                &geometry_entities,
                &(design_segment, u64::from(image.plane_entity_suffix)),
                "find F3D Canvas geometry entity",
            )?
            && ctx.decode.contains_hash_set(
                &component_entities,
                &(design_segment, u64::from(image.component_entity_suffix)),
                "find F3D Canvas component entity",
            )?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Canvas image has an invalid frame or Design object join",
                Some(
                    ctx.decode
                        .copy_retained_text(&image.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate Decal native and neutral object joins.
fn validate_decal_images(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    const TARGET_ROLE: DesignOperandRole = DesignOperandRole::BODIES_A;
    if ctx.native.design_decal_images.is_empty() {
        return Ok(());
    }
    let mut scope_bindings = HashSet::new();
    let mut asset_records = HashSet::new();
    let mut fusion_entities_storage = ctx
        .decode
        .reserve_scoped(0, "index F3D Decal fusion entities")?;
    let mut fusion_entities = HashSet::new();
    let mut operands_by_record = None;
    let mut neutral_features = None;
    for design_type in ctx.decode.admit_iter(
        &ctx.native.design_types,
        "scan F3D Decal fusion entities types",
    )? {
        if !matches!(
            design_type.module.as_str(),
            records::entity_header::DESIGN_MODULE_FUSION
        ) {
            continue;
        }
        let segment = ids::design_segment(design_type.id());
        let (entity_values, located_entity_values) = design_type.entities.storage_slices();
        for suffix in ctx
            .decode
            .admit_iter(entity_values, "scan F3D Decal fusion entities members")?
            .chain(
                ctx.decode
                    .admit_iter(
                        located_entity_values,
                        "scan F3D Decal fusion entities located members",
                    )?
                    .map(|row| &row.value),
            )
        {
            fusion_entities_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut fusion_entities,
                    (segment, *suffix),
                    "index F3D Decal fusion entities",
                )
            })?;
        }
    }
    for image in ctx.decode.admit_iter(
        &ctx.native.design_decal_images,
        "scan F3D design decal images",
    )? {
        let native_stream = design_stream(&image.id);
        let design_segment = ids::design_segment(&image.id);
        let scope = ctx.decode.get_hash_map(
            &ctx.scopes_by_index,
            &(native_stream, image.scope_record_index()),
            "find F3D Decal scope",
        )?;
        let group = ctx.decode.get_hash_map(
            &ctx.operand_groups_by_index,
            &(native_stream, image.target_group_record_index),
            "find F3D Decal target group",
        )?;
        let operand = match group
            .and_then(|group| group.members().first().map(|member| (group, member.value)))
        {
            Some((group, member)) => {
                let operands_by_record = match &mut operands_by_record {
                    Some(operands) => operands,
                    None => {
                        let mut operands = HashMap::new();
                        for operand in ctx.decode.admit_iter(
                            &ctx.native.design_body_recipe_operands,
                            "group F3D Decal operands",
                        )? {
                            fusion_entities_storage.with_storage(|| {
                                ctx.decode.push_hash_group(
                                    &mut operands,
                                    (design_stream(&operand.id), operand.record_index()),
                                    operand,
                                    "group F3D Decal operands",
                                    "collect F3D Decal operand group members",
                                )
                            })?;
                        }
                        operands_by_record.insert(operands)
                    }
                };
                let candidates = ctx
                    .decode
                    .get_hash_map(
                        operands_by_record,
                        &(native_stream, member),
                        "find F3D Decal operand records",
                    )?
                    .map_or(&[][..], Vec::as_slice);
                ctx.decode
                    .find_by(
                        candidates,
                        |operand| {
                            Ok(operand.scope_record_index == image.scope_record_index()
                                && operand.owner.group() == Some((group.record_index, 0)))
                        },
                        "find F3D Decal operand",
                    )?
                    .copied()
            }
            None => None,
        };
        let mut projected_faces_storage = ctx
            .decode
            .reserve_scoped(0, "collect F3D Decal projected faces")?;
        let projected =
            if image.mapping_mode == crate::records::decal::DesignDecalMappingMode::FitToFaces {
                if let Some(operand) = operand {
                    let mut faces = Vec::new();
                    for reference in ctx
                        .decode
                        .admit_iter(operand.references(), "scan F3D Decal projected references")?
                    {
                        for id in ctx.decode.admit_iter(
                            &reference.candidate_faces,
                            "scan F3D Decal projected faces",
                        )? {
                            projected_faces_storage.with_storage(|| {
                                let face = id.try_clone_for_decode(
                                    ctx.decode,
                                    "collect F3D Decal projected faces",
                                )?;
                                ctx.decode.push_vec(
                                    &mut faces,
                                    face,
                                    "collect F3D Decal projected faces",
                                )
                            })?;
                        }
                    }
                    ctx.decode.stable_sort_by(
                        &mut faces,
                        |value| value.as_str(),
                        Ord::cmp,
                        "f3d decal projected faces sort",
                    )?;
                    ctx.decode
                        .dedup_vec(&mut faces, "deduplicate F3D Decal projected faces")?;
                    (!faces.is_empty()).then_some((operand, faces))
                } else {
                    None
                }
            } else {
                None
            };
        let neutral_is_valid = match projected {
            None => true,
            Some((operand, expected_faces)) => match scope {
                None => false,
                Some(scope) => ctx.decode.any_by(
                    neutral_features_with_native_ref(
                        ctx,
                        &mut neutral_features,
                        &mut fusion_entities_storage,
                        scope.id.as_str(),
                    )?,
                    |feature| {
                        let cadmpeg_ir::features::FeatureDefinition::Operation(
                            cadmpeg_ir::features::FeatureOperation::Decal {
                                asset,
                                faces:
                                    cadmpeg_ir::features::FaceSelection::Resolved { faces, native },
                                mapping: cadmpeg_ir::features::DecalMapping::FitToFaces,
                                opacity: None,
                            },
                        ) = feature.evaluation.definition()
                        else {
                            return Ok(false);
                        };
                        Ok(ctx.decode.equal(
                            faces,
                            &expected_faces,
                            "compare F3D Decal projected face IDs",
                        )? && ctx.decode.equal(
                            native,
                            &operand.id,
                            "compare F3D Decal operand identity",
                        )? && ctx.decode.any_by(
                            &ctx.ir.model.assets,
                            |candidate| {
                                Ok(ctx.decode.equal(
                                    &candidate.id,
                                    asset,
                                    "compare F3D Decal asset identity",
                                )? && match &candidate.name {
                                    Some(name) => ctx.decode.equal(
                                        name.as_str(),
                                        image.asset.name(),
                                        "compare F3D Decal asset name",
                                    )?,
                                    None => false,
                                })
                            },
                            "find F3D Decal projected asset",
                        )?)
                    },
                    "find F3D Decal neutral feature",
                )?,
            },
        };
        drop(projected_faces_storage);
        let scope_valid = match scope {
            Some(scope) => scope.kind() == crate::records::feature::scope::DesignFeatureKind::Decal,
            None => false,
        };
        let scope_unique = if scope_valid {
            fusion_entities_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut scope_bindings,
                    (native_stream, image.scope_record_index()),
                    "index F3D Decal scopes",
                )
            })?
        } else {
            false
        };
        let asset_unique = if scope_unique {
            fusion_entities_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut asset_records,
                    (native_stream, image.asset.record_index()),
                    "index F3D Decal assets",
                )
            })?
        } else {
            false
        };
        let valid = scope_valid
            && scope_unique
            && asset_unique
            && scope.is_some_and(|scope| scope.byte_offset() == image.scope_byte_offset())
            && ctx.decode.contains_hash_set(
                &fusion_entities,
                &(design_segment, u64::from(image.asset.entity_suffix())),
                "find F3D Decal fusion entity",
            )?
            && group.is_some_and(|group| {
                group.scope_record_index == image.scope_record_index()
                    && group.role() == TARGET_ROLE
                    && group.members().len() == 1
            })
            && operand.is_some()
            && neutral_is_valid;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Decal image has an invalid frame or Design object join",
                Some(
                    ctx.decode
                        .copy_retained_text(&image.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate the ordered Design body-map binding entries and their pair runs.
fn validate_body_bindings(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let mut storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D body binding indexes")?;
    let mut binding_offsets = HashSet::new();
    let mut binding_groups =
        std::collections::BTreeMap::<(&str, u64), Vec<&records::bodies::DesignBodyBinding>>::new();
    for binding in ctx.decode.admit_iter(
        &native.design_body_bindings,
        "scan F3D design body bindings",
    )? {
        let native_stream = design_stream(binding.id());
        let resolved_valid = if let Some(body) = &binding.body {
            let mut source_keys_storage =
                ctx.decode.reserve_scoped(0, "hold F3D body source keys")?;
            let mut occurrence_keys = Vec::new();
            for key in ctx
                .decode
                .admit_iter(&native.body_native_keys, "select F3D body source keys")?
            {
                if ids::same_native_occurrence(
                    ctx.decode,
                    key.source_namespace.as_str(),
                    binding.id(),
                )? {
                    ctx.decode.push_scoped_vec(
                        &mut source_keys_storage,
                        &mut occurrence_keys,
                        key,
                        "collect F3D body source keys",
                    )?;
                }
            }
            let has_named_source = ctx.decode.any_by(
                &occurrence_keys,
                |key| match key.source_brep.as_deref() {
                    Some(source) => ctx.decode.equal(
                        source,
                        binding.blob_name(),
                        "compare F3D body source name",
                    ),
                    None => Ok(false),
                },
                "find F3D named body sources",
            )?;
            let mut source_keys = Vec::new();
            for key in ctx
                .decode
                .admit_iter(&occurrence_keys, "select F3D named body source keys")?
            {
                if if has_named_source {
                    match key.source_brep.as_deref() {
                        Some(source) => ctx.decode.equal(
                            source,
                            binding.blob_name(),
                            "compare F3D selected body source name",
                        )?,
                        None => false,
                    }
                } else {
                    key.source_brep.is_none()
                } {
                    ctx.decode.push_scoped_vec(
                        &mut source_keys_storage,
                        &mut source_keys,
                        *key,
                        "collect F3D body source keys",
                    )?;
                }
            }
            match crate::brep::resolve_body_selector(
                ctx.decode,
                source_keys.iter().copied(),
                binding.asm_body_key,
            ) {
                Ok(Some(resolved)) => {
                    ctx.decode
                        .equal(resolved, body, "compare F3D resolved body identity")?
                }
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Ok(None) | Err(_) => false,
            }
        } else {
            true
        };
        let valid = design_stream_contains_entry(ctx.decode, native_stream, binding.stream())?
            && resolved_valid
            && storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut binding_offsets,
                    (native_stream, binding.asm_body_key_offset()),
                    "index F3D body binding offsets",
                )
            })?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design body binding has an invalid ordered map entry",
                Some(
                    ctx.decode
                        .copy_retained_text(binding.id(), "retain F3D validation entity")?,
                ),
            )?;
        }
        storage.with_storage(|| {
            ctx.decode.push_btree_group(
                &mut binding_groups,
                (native_stream, binding.blob_name_offset()),
                binding,
                "index F3D body binding groups",
                "collect F3D body binding group members",
            )
        })?;
    }
    for (_, bindings) in ctx
        .decode
        .admit_iter(&mut binding_groups, "scan F3D body binding groups")?
    {
        ctx.decode.stable_sort_by_key(
            bindings,
            |value| value.pair_ordinal(),
            Ord::cmp,
            "f3d body binding pair run sort",
        )?;
        let complete = bindings
            .first()
            .is_some_and(|first| usize::try_from(first.pair_count()).ok() == Some(bindings.len()))
            && ctx.decode.all_by(
                bindings.iter().enumerate(),
                |(ordinal, binding)| {
                    Ok(
                        usize::try_from(binding.pair_ordinal()).ok() == Some(ordinal)
                            && binding.pair_count() == bindings[0].pair_count()
                            && ctx.decode.equal(
                                binding.blob_name(),
                                bindings[0].blob_name(),
                                "compare F3D body pair names",
                            )?
                            && ctx.decode.equal(
                                binding.stream(),
                                bindings[0].stream(),
                                "compare F3D body pair streams",
                            )?,
                    )
                },
                "check F3D ordered body pairs",
            )?;
        if !complete {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design body map has an incomplete ordered pair run",
                bindings
                    .first()
                    .map(|binding| {
                        ctx.decode
                            .copy_retained_text(binding.id(), "retain F3D validation entity")
                    })
                    .transpose()?,
            )?;
        }
    }
    Ok(())
}

/// Validate each Design body-bounds repeated record frame.
fn validate_body_bounds(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    let native = ctx.native;
    if native.design_body_bounds.is_empty() {
        return Ok(());
    }
    let mut storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D body bounds indexes")?;
    let mut bindings_by_entity = HashMap::new();
    for binding in ctx.decode.admit_iter(
        &native.design_body_bindings,
        "group F3D body bounds bindings",
    )? {
        storage.with_storage(|| {
            ctx.decode.push_hash_group(
                &mut bindings_by_entity,
                binding.entity_suffix,
                binding,
                "group F3D body bounds bindings",
                "collect F3D body bounds binding group members",
            )
        })?;
    }
    let mut bounded_bodies = HashSet::new();
    for bounds in ctx
        .decode
        .admit_iter(&native.design_body_bounds, "scan F3D design body bounds")?
    {
        let native_stream = design_stream(bounds.id());
        let candidates = ctx
            .decode
            .get_hash_map(
                &bindings_by_entity,
                &bounds.entity_suffix(),
                "find F3D body bounds bindings",
            )?
            .map_or(&[][..], Vec::as_slice);
        let mut expected_storage = ctx
            .decode
            .reserve_scoped(0, "collect F3D expected body bounds bindings")?;
        let mut expected_bindings = Vec::new();
        for binding in ctx
            .decode
            .admit_iter(candidates, "collect F3D expected body bounds bindings")?
        {
            if design_stream_contains_entry(ctx.decode, native_stream, binding.stream())? {
                ctx.decode.push_scoped_vec(
                    &mut expected_storage,
                    &mut expected_bindings,
                    *binding,
                    "collect F3D expected body bounds bindings",
                )?;
            }
        }
        ctx.decode.stable_sort_by_key(
            &mut expected_bindings,
            |value| value.asm_body_key_offset(),
            Ord::cmp,
            "f3d body bounds binding sort",
        )?;
        let valid_frame = ctx
            .decode
            .get_hash_map(
                &ctx.entities_by_suffix,
                &(native_stream, bounds.entity_suffix()),
                "find F3D body bounds entity",
            )?
            .is_some_and(|entity| {
                entity.module() == Some(records::entity_header::DESIGN_MODULE_BODY)
                    && entity.byte_offset == bounds.entity_byte_offset()
            })
            && bounds.body_binding_ids().len() == expected_bindings.len()
            && ctx.decode.all_by(
                bounds.body_binding_ids().zip(&expected_bindings),
                |(id, binding)| {
                    ctx.decode.equal(
                        id,
                        binding.id().as_str(),
                        "compare F3D body bounds binding IDs",
                    )
                },
                "scan F3D body bounds binding IDs",
            )?;
        let valid = if valid_frame {
            storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut bounded_bodies,
                    (native_stream, bounds.entity_suffix()),
                    "index F3D bounded bodies",
                )
            })?
        } else {
            false
        };
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design body bounds have an invalid repeated record frame",
                Some(
                    ctx.decode
                        .copy_retained_text(bounds.id(), "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

fn valid_vertex_recipe(
    ctx: &Ctx,
    scope: &records::feature::scope::DesignParameterScope,
    native_stream: &str,
    record_index: u32,
    vertex: &records::feature::work_geometry::DesignVertexRecipe,
) -> Result<bool, CodecError> {
    let native = ctx.native;
    let header = ctx.decode.get_hash_map(
        &ctx.records_by_index,
        &(native_stream, record_index),
        "find F3D validation record index",
    )?;
    let recipe = ctx.decode.get_hash_map(
        &ctx.recipes_by_id,
        vertex.recipe_id.as_str(),
        "find F3D validation record index",
    )?;
    let mut expected_reference_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D vertex recipe expected references")?;
    let mut expected_references = expected_reference_storage.with_storage(|| {
        design::decode::dimension_frames::decode_recipe_references_charged(
            ctx.decode,
            &vertex.recipe_prefix_bytes,
            vertex.recipe_prefix_offset(),
        )
    })?;
    for reference in ctx.decode.admit_iter(
        &mut expected_references,
        "scan F3D expected recipe references",
    )? {
        expected_reference_storage.with_storage(|| {
            design::decode::dimension_frames::bind_recipe_reference_candidates_charged(
                ctx.decode,
                reference,
                &native.persistent_subentity_tags,
                Some(&scope.id),
            )
        })?;
    }
    let prefix_length = u64::try_from(vertex.recipe_prefix_bytes.len()).ok();
    let family_name_length = u64::try_from(design::construction_recipe_family_name_len(
        records::recipes::ConstructionRecipeKind::Vertex,
    ))
    .ok();
    let program_byte_length = u64::try_from(vertex.recipe_program.len())
        .ok()
        .and_then(|length| length.checked_mul(4));
    let resolution_is_valid = match vertex.resolution {
        None => true,
        Some(resolution) => {
            let state_id = resolution.state_id;
            let vertex_slot = resolution.vertex_slot();
            let states = ctx.decode.get_hash_map(
                &ctx.states_by_id,
                &state_id,
                "find F3D vertex recipe state",
            )?;
            match states {
                Some(Some(state)) => match state.topology() {
                    Some(topology) => ctx.decode.contains(
                        &topology.vertices,
                        &vertex_slot,
                        "find F3D vertex recipe historical vertex",
                    )?,
                    None => history::projection_was_finalized(ctx.decode, &native.asm_histories)?,
                },
                _ => false,
            }
        }
    };
    Ok(vertex.record_index() == record_index
        && (match header {
            Some(header) => {
                header.byte_offset == vertex.byte_offset() && (header.class_tag == vertex.class_tag)
            }
            None => false,
        })
        && prefix_length.is_some_and(|prefix_length| {
            vertex
                .recipe_prefix_offset()
                .checked_add(prefix_length)
                .zip(recipe.and_then(|recipe| recipe.byte_offset.checked_sub(4)))
                .is_some_and(|(prefix_end, recipe_prefix_end)| prefix_end == recipe_prefix_end)
        })
        && ctx.decode.equal(
            &vertex.recipe_references,
            &expected_references,
            "compare F3D vertex recipe references",
        )?
        && resolution_is_valid
        && match recipe {
            None => false,
            Some(recipe) => {
                ctx.decode.equal(
                    design_stream(&recipe.id),
                    native_stream,
                    "compare F3D vertex recipe stream",
                )? && recipe.kind == records::recipes::ConstructionRecipeKind::Vertex
                    && recipe.byte_offset > vertex.recipe_record_byte_offset()
                    && recipe.byte_offset < vertex.next_byte_offset()
                    && family_name_length.is_some_and(|family_name_length| {
                        recipe
                            .byte_offset
                            .checked_add(family_name_length)
                            .is_some_and(|expected_offset| {
                                vertex.recipe_program_offset == expected_offset
                            })
                    })
            }
        }
        && program_byte_length.is_some_and(|program_byte_length| {
            program_byte_length != 0
                && vertex
                    .recipe_program_offset
                    .checked_add(program_byte_length)
                    .is_some_and(|expected_offset| expected_offset == vertex.next_byte_offset())
        }))
}

fn validate_component_occurrences(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D component occurrences scratch")?;
    let mut identities = HashSet::new();
    let mut record_indices = HashSet::new();
    for occurrence in ctx.decode.admit_iter(
        &ctx.native.design_component_occurrences,
        "scan F3D design component occurrences",
    )? {
        let stream = design_stream(&occurrence.id);
        let key = folded_guid(occurrence.occurrence_guid.as_str());
        let unique_identity = scratch_storage.with_storage(|| {
            ctx.decode
                .insert_hash_set(&mut identities, (stream, key), "index F3D occurrence GUIDs")
        })?;
        let unique_record = if unique_identity {
            scratch_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut record_indices,
                    (stream, occurrence.record_index),
                    "index F3D occurrence record indices",
                )
            })?
        } else {
            false
        };
        let valid = unique_identity && unique_record && match occurrence.placement() {
            records::feature::assembly_features::DesignComponentOccurrencePlacement::Base => true,
            records::feature::assembly_features::DesignComponentOccurrencePlacement::Explicit {
                ordinal,
                ..
            } => occurrence.class_tag.as_str() == "327" || ordinal.get() > 1,
        };
        // The duplicated references must agree within one carrier, which
        // the decoder checks. The component GUID is the reusable-definition
        // identity; a different carrier-local component-record reference
        // does not contradict it.
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design component occurrence has an invalid fixed frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&occurrence.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate Extrude selection groups and their counted member frames.
fn validate_extrude_selection_groups(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D extrude selection groups scratch")?;
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let mut group_slots = HashSet::new();
    for group in ctx.decode.admit_iter(
        &native.design_extrude_selection_groups,
        "scan F3D design extrude selection groups",
    )? {
        let native_stream = design_stream(&group.id);
        let scope = ctx.decode.get_hash_map(
            scopes_by_index,
            &(native_stream, group.scope_record_index),
            "find F3D validation record index",
        )?;
        let header = ctx.decode.get_hash_map(
            records_by_index,
            &(native_stream, group.record_index),
            "find F3D validation record index",
        )?;
        let frame_valid = match scope {
            None => false,
            Some(scope) => {
                design::design_feature_family(&scope.kind())
                    == Some(design::DesignFeatureFamily::Extrude)
                    && match usize::try_from(group.scope_reference_ordinal) {
                        Ok(ordinal) => {
                            reference_member_at(scope.reference_members(), ordinal)
                                == Some(&group.record_index)
                        }
                        Err(_) => false,
                    }
            }
        } && match header {
            Some(header) => {
                header.byte_offset == group.byte_offset() && (header.class_tag == group.class_tag)
            }
            None => false,
        } && ctx.decode.all_by(
            group.members(),
            |member| {
                ctx.decode.contains_key_hash_map(
                    records_by_index,
                    &(native_stream, member.value),
                    "find F3D Extrude group member record",
                )
            },
            "validate F3D Extrude group member records",
        )?;
        let valid = if frame_valid {
            scratch_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut group_slots,
                    (
                        native_stream,
                        group.scope_record_index,
                        group.scope_reference_ordinal,
                    ),
                    "index F3D Extrude selection group slots",
                )
            })?
        } else {
            false
        };
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Extrude selection group has an invalid counted frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&group.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate construction operand groups and their role discriminators.
fn validate_construction_operand_groups(
    ctx: &Ctx,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D construction operand groups scratch")?;
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let mut operand_group_slots = HashSet::new();
    for group in ctx.decode.admit_iter(
        &native.design_construction_operand_groups,
        "scan F3D design construction operand groups",
    )? {
        let native_stream = design_stream(&group.id);
        let scope = ctx.decode.get_hash_map(
            scopes_by_index,
            &(native_stream, group.scope_record_index),
            "find F3D validation record index",
        )?;
        let header = ctx.decode.get_hash_map(
            records_by_index,
            &(native_stream, group.record_index),
            "find F3D validation record index",
        )?;
        let frame = &group.frame;
        let member_run_end = group.members().last().map_or_else(
            || frame.member_count_offset.checked_add(4),
            |member| member.offset.checked_add(10),
        );
        let frame_valid =
            group
                .byte_offset
                .checked_add(
                    if match scope {
                        None => false,
                        Some(scope) => {
                            (scope.kind()
                                == crate::records::feature::scope::DesignFeatureKind::SurfaceStitch)
                                || ((scope.kind()
                                    == crate::records::feature::scope::DesignFeatureKind::SplitFace)
                                    && group.role() == DesignOperandRole::ROLE_0X21)
                                || ((scope.kind()
                                    == crate::records::feature::scope::DesignFeatureKind::Split)
                                    && matches!(
                                        group.role(),
                                        DesignOperandRole::ROLE_0X9 | DesignOperandRole::ROLE_0X21
                                    ))
                        }
                    } {
                        88
                    } else {
                        21
                    },
                )
                .is_some_and(|expected_offset| frame.member_count_offset == expected_offset)
                && group.members().first().is_none_or(|member| {
                    frame
                        .member_count_offset
                        .checked_add(5)
                        .is_some_and(|expected_offset| member.offset == expected_offset)
                })
                && frame.trailing_records().first().is_none_or(|record| {
                    group
                        .role_offset()
                        .checked_sub(10)
                        .is_some_and(|expected_offset| record.offset == expected_offset)
                })
                && member_run_end.is_some_and(|end| group.role_offset() >= end)
                && group.role().raw().trailing_zeros() >= 32
                && frame
                    .opaque_scalar_offset()
                    .checked_add(8)
                    .is_some_and(|expected_offset| group.paired_byte_offset > expected_offset)
                && ctx.decode.all_by(
                    frame
                        .auxiliary_records
                        .iter()
                        .chain(frame.trailing_records().iter()),
                    |record| {
                        ctx.decode.contains_key_hash_map(
                            records_by_index,
                            &(native_stream, record.value),
                            "find F3D construction tail record",
                        )
                    },
                    "scan F3D construction auxiliary records",
                )?
                && ctx.decode.all_by(
                    frame.trailing_transforms(),
                    |transform| {
                        Ok(ctx.decode.any_by(
                            frame.trailing_records(),
                            |record| Ok(record.value == transform.record_index()),
                            "find F3D construction tail reference",
                        )? && match ctx.decode.get_hash_map(
                            records_by_index,
                            &(native_stream, transform.record_index()),
                            "find F3D construction tail record",
                        )? {
                            Some(header) => {
                                header.byte_offset == transform.byte_offset()
                                    && (header.class_tag == transform.class_tag)
                            }
                            None => false,
                        } && match ctx.decode.get_hash_map(
                            records_by_index,
                            &(native_stream, transform.following_record_index()),
                            "find F3D construction tail record",
                        )? {
                            Some(header) => {
                                header.byte_offset == transform.following_byte_offset()
                                    && (header.class_tag == transform.following_class_tag)
                            }
                            None => false,
                        })
                    },
                    "validate F3D construction trailing_transforms",
                )?
                && ctx.decode.all_by(
                    frame.trailing_dual_transforms(),
                    |transform| {
                        Ok(ctx.decode.any_by(
                            frame.trailing_records(),
                            |record| Ok(record.value == transform.record_index),
                            "find F3D construction tail reference",
                        )? && match ctx.decode.get_hash_map(
                            records_by_index,
                            &(native_stream, transform.record_index),
                            "find F3D construction tail record",
                        )? {
                            Some(header) => {
                                header.byte_offset == transform.byte_offset
                                    && (header.class_tag == transform.class_tag)
                            }
                            None => false,
                        } && transform.byte_offset.checked_add(21).is_some_and(
                            |expected_offset| transform.first_transform_offset == expected_offset,
                        ) && transform.byte_offset.checked_add(149).is_some_and(
                            |expected_offset| transform.second_transform_offset == expected_offset,
                        ))
                    },
                    "validate F3D construction trailing_dual_transforms",
                )?
                && ctx.decode.all_by(
                    frame.trailing_flags(),
                    |flag| {
                        Ok(ctx.decode.any_by(
                            frame.trailing_records(),
                            |record| Ok(record.value == flag.record_index),
                            "find F3D construction tail reference",
                        )? && match ctx.decode.get_hash_map(
                            records_by_index,
                            &(native_stream, flag.record_index),
                            "find F3D construction tail record",
                        )? {
                            Some(header) => {
                                header.byte_offset == flag.byte_offset
                                    && (header.class_tag == flag.class_tag)
                            }
                            None => false,
                        } && flag
                            .byte_offset
                            .checked_add(22)
                            .is_some_and(|expected_offset| flag.value_offset == expected_offset))
                    },
                    "validate F3D construction trailing_flags",
                )?
                && ctx.decode.all_by(
                    frame.auxiliary_paths(),
                    |path| {
                        Ok(ctx.decode.any_by(
                            &frame.auxiliary_records,
                            |record| Ok(record.value == path.record_index()),
                            "find F3D construction tail reference",
                        )? && match ctx.decode.get_hash_map(
                            records_by_index,
                            &(native_stream, path.record_index()),
                            "find F3D construction tail record",
                        )? {
                            Some(header) => {
                                header.byte_offset == path.byte_offset()
                                    && (header.class_tag == path.class_tag)
                            }
                            None => false,
                        } && path.scope_record_index == group.scope_record_index
                            && ctx.decode.contains_key_hash_map(
                                records_by_index,
                                &(native_stream, path.nested_record_index()),
                                "find F3D nested construction record",
                            )?
                            && match ctx.decode.get_hash_map(
                                records_by_index,
                                &(native_stream, path.following_record_index()),
                                "find F3D construction tail record",
                            )? {
                                Some(header) => {
                                    header.byte_offset == path.following_byte_offset()
                                        && (header.class_tag == path.following_class_tag)
                                }
                                None => false,
                            })
                    },
                    "validate F3D construction auxiliary_paths",
                )?;
        let valid = match scope {
            None => false,
            Some(scope) => {
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
                        == if (scope.kind() == crate::records::feature::scope::DesignFeatureKind::CoilPrimitive)
                            && scope.reference_members().len() == 10
                            && scope.coil_operation_offset() == scope.byte_offset().checked_add(22)
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
                        && match scope.ruled_surface_operation() {
                            Some(operation) => ctx.decode.contains(&operation.edge_group_record_indices, &group.record_index, "find F3D construction operand role group")?,
                            None => false,
                        }
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
                        DesignOperandRole::BODIES_A | DesignOperandRole::ROLE_0X5
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
                        && match scope.thread_construction() {
                            Some(construction) => ctx.decode.contains(&construction.face_group_record_indices, &group.record_index, "find F3D construction operand role group")?,
                            None => false,
                        }
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
                None if (scope.kind() == crate::records::feature::scope::DesignFeatureKind::RemoveBody) =>
                {
                    group.role() == DesignOperandRole::BODIES_A && group.extrude_role().is_none()
                }
                None if (scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfaceStitch) =>
                {
                    group.role() == DesignOperandRole::ROLE_0X5 && group.extrude_role().is_none()
                }
                None if (scope.kind() == crate::records::feature::scope::DesignFeatureKind::SplitFace) =>
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
                None if (scope.kind() == crate::records::feature::scope::DesignFeatureKind::Decal) =>
                {
                    group.role() == DesignOperandRole::BODIES_A && group.extrude_role().is_none()
                }
                None if (scope.kind() == crate::records::feature::scope::DesignFeatureKind::BaseFlange) =>
                {
                    group.role() == DesignOperandRole::PROFILE
                        && group.extrude_role().is_none()
                        && match scope.base_flange_profile().as_ref() {
                            Some(profile) => match group.members() {
                                [member] => ctx.decode.equal(&member.value, &profile.record_index, "compare F3D BaseFlange profile member")?,
                                _ => false,
                            },
                            None => false,
                        }
                }
                None if (scope.kind() == crate::records::feature::scope::DesignFeatureKind::Hem) => {
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
                    && match usize::try_from(group.scope_reference_ordinal) {
                        Ok(ordinal) => {
                            reference_member_at(scope.reference_members(), ordinal)
                                == Some(&group.record_index)
                        }
                        Err(_) => false,
                    }
                    && ctx.decode.all_by(
                        group.members(),
                        |member| {
                            let (plain, located) = scope.reference_members().storage_slices();
                            Ok(ctx.decode.contains(
                                plain,
                                &member.value,
                                "find F3D construction scope member",
                            )? || ctx.decode.any_by(
                                located,
                                |value| Ok(value.value == member.value),
                                "find F3D located construction scope member",
                            )?)
                        },
                        "validate F3D construction scope members",
                    )?
            }
        } && match header {
            Some(header) => {
                header.byte_offset == group.byte_offset && (header.class_tag == group.class_tag)
            }
            None => false,
        } && frame_valid
            && !group.members().is_empty()
            && {
                let mut seen_storage = ctx
                    .decode
                    .reserve_scoped(0, "hold F3D construction group member uniqueness")?;
                let mut seen = HashSet::new();
                for member in ctx
                    .decode
                    .admit_iter(group.members(), "scan F3D construction operand members")?
                {
                    seen_storage.with_storage(|| {
                        ctx.decode.insert_hash_set(
                            &mut seen,
                            member.value,
                            "index F3D construction operand group members",
                        )
                    })?;
                }
                seen.len() == group.members().len()
            }
            && ctx.decode.all_by(
                group.members(),
                |member| {
                    ctx.decode.contains_key_hash_map(
                        records_by_index,
                        &(native_stream, member.value),
                        "find F3D construction group member record",
                    )
                },
                "validate F3D construction group member records",
            )?
            && scratch_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut operand_group_slots,
                    (
                        native_stream,
                        group.scope_record_index,
                        group.scope_reference_ordinal,
                    ),
                    "index F3D construction operand group slots",
                )
            })?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design construction operand group has an invalid frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&group.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate path-feature operand roles against the scope construction.
///
/// The role grammar is shared with the fixed Loft projector. Keeping the
/// predicate independent of native byte offsets lets validation reject a
/// malformed role combination without rejecting a valid section/guide mix.
pub(crate) fn loft_operand_roles_are_valid(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operation: records::feature::extrude::DesignExtrudeOperation,
    groups: &[(DesignOperandRole, usize)],
) -> Result<bool, CodecError> {
    const BODY: DesignOperandRole = DesignOperandRole::BODIES_A;
    const SECTION: DesignOperandRole = DesignOperandRole::PROFILE;
    const FACE_SECTION: DesignOperandRole = DesignOperandRole::ROLE_0X43;
    const GUIDE: DesignOperandRole = DesignOperandRole::ROLE_0X5;
    const CENTERLINE: DesignOperandRole = DesignOperandRole::ROLE_0X7;

    #[derive(Default)]
    struct Counts {
        bodies: usize,
        sections: usize,
        face_sections: usize,
        guides: usize,
        centerlines: usize,
        point_guides: usize,
        point_ordinal: usize,
    }
    let counts = decode
        .fold(
            groups,
            (Counts::default(), 0usize),
            |(mut counts, ordinal), (role, members)| {
                match *role {
                    BODY => counts.bodies += 1,
                    SECTION => counts.sections += 1,
                    FACE_SECTION => counts.face_sections += 1,
                    GUIDE => {
                        if *members == 1 {
                            counts.point_guides += 1;
                            counts.point_ordinal = ordinal - counts.bodies;
                        }
                        counts.guides += 1;
                    }
                    CENTERLINE => counts.centerlines += 1,
                    _ => {}
                }
                Ok((counts, ordinal + 1))
            },
            "count F3D Loft body roles",
        )?
        .0;
    let expected_bodies =
        usize::from(operation != records::feature::extrude::DesignExtrudeOperation::NewBody);
    if counts.bodies != expected_bodies {
        return Ok(false);
    }
    let operands = groups.len() - counts.bodies;
    let sections = counts.sections + counts.face_sections;
    if sections >= 2 {
        return Ok(operands == sections + counts.guides + counts.centerlines
            && counts.centerlines <= 1
            && !(counts.guides > 0 && counts.centerlines > 0));
    }
    if operation != records::feature::extrude::DesignExtrudeOperation::NewBody {
        return Ok(false);
    }
    if sections == 1 && operands == counts.face_sections + counts.guides {
        return Ok(counts.point_guides == 1
            && (counts.point_ordinal == 0 || counts.point_ordinal + 1 == operands));
    }
    Ok(
        sections == 0
            && operands >= 2
            && (operands == counts.sections || operands == counts.guides),
    )
}

fn validate_path_feature_operand_roles(
    ctx: &Ctx,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    for scope in ctx
        .decode
        .admit_iter(
            &native.design_parameter_scopes,
            "scan F3D path feature scopes",
        )?
        .filter(|scope| scope.has_path_construction())
    {
        let native_stream = design_stream(&scope.id);
        let mut group_storage = ctx
            .decode
            .reserve_scoped(0, "collect F3D path-feature operand groups")?;
        let mut groups = Vec::new();
        for group in ctx.decode.admit_iter(
            ctx.decode
                .get_hash_map(
                    &ctx.operand_groups_by_scope,
                    &(native_stream, scope.record_index),
                    "find F3D path-feature scope operand groups",
                )?
                .map_or(&[][..], Vec::as_slice),
            "scan F3D path-feature operand groups",
        )? {
            group_storage.with_storage(|| {
                ctx.decode.push_vec(
                    &mut groups,
                    *group,
                    "collect F3D path-feature operand groups",
                )
            })?;
        }
        let role_count = |role| {
            Ok::<_, CodecError>(
                ctx.decode
                    .admit_iter(&groups, "count F3D path operand roles")?
                    .filter(|group| group.role() == role)
                    .count(),
            )
        };
        let group_roles =
            ctx.decode
                .with_scoped_storage("collect F3D path-feature operand roles", || {
                    ctx.decode.collect_vec(
                        ctx.decode
                            .admit_iter(&groups, "scan F3D path operand role groups")?
                            .map(|group| (group.role(), group.members().len())),
                        "collect F3D path-feature operand roles",
                    )
                })?;
        let valid = match &scope.payload() {
            records::feature::scope::DesignScopePayload::Revolve(Some(
                crate::records::feature::path_features::DesignRevolveConstruction {
                    operation,
                    angle_record_index,
                    opposite_angle,
                    ..
                },
            )) => {
                let body_count = role_count(DesignOperandRole::BODIES_A)?
                    .checked_add(role_count(DesignOperandRole::BODIES_B)?)
                    .ok_or_else(|| {
                        ctx.decode.refuse_codec_limit(
                            "count F3D Revolve body roles",
                            u64::MAX - 1,
                            u64::MAX,
                        )
                    })?;
                let expected_body_count = usize::from(
                    *operation != records::feature::extrude::DesignExtrudeOperation::NewBody,
                );
                ctx.decode.any_by(
                    scope.reference_members().storage_slices().0.iter().chain(
                        scope
                            .reference_members()
                            .storage_slices()
                            .1
                            .iter()
                            .map(|row| &row.value),
                    ),
                    |value| Ok(*value == *angle_record_index),
                    "scan F3D Revolve angle references",
                )? && (match opposite_angle {
                    None => true,
                    Some(located) => ctx.decode.any_by(
                        scope.reference_members().storage_slices().0.iter().chain(
                            scope
                                .reference_members()
                                .storage_slices()
                                .1
                                .iter()
                                .map(|row| &row.value),
                        ),
                        |value| Ok(*value == located.value),
                        "scan F3D Revolve opposite angle references",
                    )?,
                }) && groups.len() == 2 + expected_body_count
                    && role_count(DesignOperandRole::ROLE_0X21)? == 1
                    && role_count(DesignOperandRole::PROFILE)? == 1
                    && body_count == expected_body_count
            }
            records::feature::scope::DesignScopePayload::Loft(Some(
                crate::records::feature::path_features::DesignLoftConstruction {
                    operation, ..
                },
            )) => loft_operand_roles_are_valid(ctx.decode, *operation, &group_roles.0)?,
            records::feature::scope::DesignScopePayload::Sweep(Some(
                records::feature::scope::DesignSweepScope {
                    construction:
                        Some(records::feature::path_features::DesignSweepConstruction {
                            operation, ..
                        }),
                    ..
                },
            )) => {
                let path_count = role_count(DesignOperandRole::ROLE_0X5)?;
                let profile_count = role_count(DesignOperandRole::PROFILE)?;
                let guide_surface_count = role_count(DesignOperandRole::FACES)?;
                let guide_profile_frame = match scope.sweep_profile() {
                    None => false,
                    Some(profile) => {
                        let profile_groups = || {
                            Ok::<_, CodecError>(
                                ctx.decode
                                    .admit_iter(&groups, "scan F3D Sweep profile groups")?
                                    .filter(|group| group.role() == DesignOperandRole::PROFILE),
                            )
                        };
                        profile_groups()?
                            .filter(|group| {
                                group
                                    .members()
                                    .iter()
                                    .map(|member| member.value)
                                    .eq([profile.record_index])
                            })
                            .count()
                            == 1
                            && profile_groups()?
                                .filter(|group| {
                                    !group
                                        .members()
                                        .iter()
                                        .map(|member| member.value)
                                        .eq([profile.record_index])
                                })
                                .try_fold(0usize, |count, group| {
                                    let selected = !group.members().is_empty()
                                        && ctx.decode.all_by(
                                            group.members(),
                                            |member| {
                                                ctx.decode.any_by(
                                                    &native.design_entity_selection_operands,
                                                    |operand| {
                                                        Ok(ctx.decode.equal(
                                                            design_stream(&operand.id),
                                                            native_stream,
                                                            "compare F3D Sweep selection streams",
                                                        )? && operand.scope_record_index
                                                            == scope.record_index
                                                            && operand.group_record_index
                                                                == group.record_index
                                                            && operand.record_index()
                                                                == member.value)
                                                    },
                                                    "find F3D Sweep member selection",
                                                )
                                            },
                                            "validate F3D Sweep profile members",
                                        )?;
                                    count.checked_add(usize::from(selected)).ok_or_else(|| {
                                        ctx.decode.refuse_codec_limit(
                                            "count F3D Sweep selected profiles",
                                            u64::MAX - 1,
                                            u64::MAX,
                                        )
                                    })
                                })?
                                == 1
                    }
                };
                let common_roles =
                    (profile_count == 1 && guide_surface_count == 0 && matches!(path_count, 1 | 2))
                        || (profile_count == 2
                            && guide_surface_count == 1
                            && path_count == 1
                            && guide_profile_frame);
                common_roles
                    && match operation {
                        records::feature::extrude::DesignExtrudeOperation::NewBody => {
                            groups.len()
                                == path_count
                                    .checked_add(profile_count)
                                    .and_then(|count| count.checked_add(guide_surface_count))
                                    .ok_or_else(|| {
                                        ctx.decode.refuse_codec_limit(
                                            "count F3D Sweep operand roles",
                                            u64::MAX - 1,
                                            u64::MAX,
                                        )
                                    })?
                                && role_count(DesignOperandRole::BODIES_A)? == 0
                        }
                        records::feature::extrude::DesignExtrudeOperation::Join
                        | records::feature::extrude::DesignExtrudeOperation::Cut
                        | records::feature::extrude::DesignExtrudeOperation::Intersect => {
                            guide_surface_count == 0
                                && groups.len()
                                    == path_count.checked_add(2).ok_or_else(|| {
                                        ctx.decode.refuse_codec_limit(
                                            "count F3D Sweep operand roles",
                                            u64::MAX - 1,
                                            u64::MAX,
                                        )
                                    })?
                                && role_count(DesignOperandRole::BODIES_A)? == 1
                        }
                    }
            }
            records::feature::scope::DesignScopePayload::Pipe(Some(
                crate::records::feature::path_features::DesignPipeConstruction {
                    operation, ..
                },
            )) => {
                *operation == records::feature::extrude::DesignExtrudeOperation::NewBody
                    && groups.len() == 1
                    && role_count(DesignOperandRole::ROLE_0X5)? == 1
                    && !groups[0].members().is_empty()
            }
            _ => false,
        };
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design path-feature operand roles conflict with its construction",
                Some(
                    ctx.decode
                        .copy_retained_text(&scope.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate Extrude profile, operation, start, and extent operand agreement.
fn validate_extrude_parameter_operands(
    ctx: &Ctx,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    const PARAMETER_KINDS: [&str; 5] = [
        "AlongDistance",
        "AgainstDistance",
        "ProfileOffset",
        "Side1Offset",
        "Side2Offset",
    ];
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    for scope in ctx
        .decode
        .admit_iter(
            &native.design_parameter_scopes,
            "scan F3D feature parameter scopes",
        )?
        .filter(|scope| {
            matches!(
                design::design_feature_family(&scope.kind()),
                Some(
                    design::DesignFeatureFamily::Extrude
                        | design::DesignFeatureFamily::Fillet
                        | design::DesignFeatureFamily::Chamfer
                )
            )
        })
    {
        let native_stream = design_stream(&scope.id);
        if design::design_feature_family(&scope.kind())
            == Some(design::DesignFeatureFamily::Extrude)
        {
            let scope_groups = ctx
                .decode
                .get_hash_map(
                    &ctx.operand_groups_by_scope,
                    &(native_stream, scope.record_index),
                    "find F3D Extrude scope operand groups",
                )?
                .map_or(&[][..], Vec::as_slice);
            let scope_owners = ctx.scope_owners(
                native_stream,
                scope.record_index,
                "find F3D Extrude scope parameter owners",
            )?;
            let mut first_profile_group = None;
            let mut second_profile_group = None;
            ctx.decode.any_by(
                scope_groups,
                |&group| {
                    if group.extrude_role()
                        == Some(
                            records::topology::extrude_selection::DesignExtrudeOperandRole::Profile,
                        )
                    {
                        if first_profile_group.is_none() {
                            first_profile_group = Some(group);
                        } else {
                            second_profile_group = Some(group);
                            return Ok(true);
                        }
                    }

                    Ok(false)
                },
                "find F3D Extrude profile groups",
            )?;
            let profile_matches_operand = scope
                .extrude_profile()
                .map(|profile| -> Result<bool, CodecError> {
                    Ok(match (first_profile_group, second_profile_group) {
                        (None, _) => match usize::try_from(profile.scope_reference_ordinal).ok() {
                            None => false,
                            Some(ordinal) => {
                                reference_member_at(scope.reference_members(), ordinal)
                                    == Some(&profile.record_index)
                            }
                        },
                        (Some(group), None) => {
                            group.members().first().map(|member| &member.value)
                                == Some(&profile.record_index)
                        }
                        (Some(_), Some(_)) => false,
                    })
                })
                .transpose()?
                .unwrap_or(true);
            if !profile_matches_operand {
                ctx.push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design Extrude profile conflicts with its profile operand group",
                    Some(
                        ctx.decode
                            .copy_retained_text(&scope.id, "retain F3D validation entity")?,
                    ),
                )?;
            }
            let has_body_operands = ctx.decode.any_by(
                scope_groups,
                |group| {
                    Ok(group.extrude_role()
                        == Some(
                            records::topology::extrude_selection::DesignExtrudeOperandRole::Bodies,
                        ))
                },
                "find F3D Extrude body operand group",
            )?;
            let face_operand_group_count = ctx
                .decode
                .admit_iter(scope_groups, "count F3D Extrude face operand groups")?
                .try_fold(0usize, |count, group| {
                    let matches = group.extrude_role().is_some_and(|role| {
                        matches!(
                            role,
                            records::topology::extrude_selection::DesignExtrudeOperandRole::Faces(
                                _
                            )
                        )
                    });
                    count.checked_add(usize::from(matches)).ok_or_else(|| {
                        ctx.decode.refuse_codec_limit(
                            "count F3D Extrude face operand groups",
                            u64::MAX - 1,
                            u64::MAX,
                        )
                    })
                })?;
            let target_shape_group_count = ctx
                .decode
                .admit_iter(scope_groups, "count F3D Extrude target shape groups")?
                .try_fold(0usize, |count, group| -> Result<usize, CodecError> {
                    let matches = group.role() == DesignOperandRole::ROLE_0X5
                        && group.extrude_role().is_none()
                        && !group.members().is_empty()
                        && {
                            ctx.decode.all_by(
                                group.members().iter().enumerate(),
                                |(ordinal, member)| {
                                    let Some(ordinal) = id_from_index(ordinal) else {
                                        return Ok(false);
                                    };
                                    if !ctx.decode.any_by(
                                        &native.design_body_recipe_operands,
                                        |operand| {
                                            Ok(ctx.decode.equal(
                                                design_stream(&operand.id),
                                                native_stream,
                                                "compare F3D Extrude target recipe streams",
                                            )? && operand.scope_record_index
                                                == scope.record_index
                                                && operand.owner.group()
                                                    == Some((group.record_index, ordinal))
                                                && operand.record_index() == member.value)
                                        },
                                        "find F3D Extrude target body recipe",
                                    )? {
                                        return Ok(false);
                                    }

                                    Ok(true)
                                },
                                "validate F3D Extrude target shape members",
                            )?
                        };
                    count.checked_add(usize::from(matches)).ok_or_else(|| {
                        ctx.decode.refuse_codec_limit(
                            "count F3D Extrude target shape groups",
                            u64::MAX - 1,
                            u64::MAX,
                        )
                    })
                })?;
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
            if !operation_matches_operands {
                ctx.push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design Extrude operation conflicts with its body operands",
                    Some(
                        ctx.decode
                            .copy_retained_text(&scope.id, "retain F3D validation entity")?,
                    ),
                )?;
            }
            let Some(prologue) = scope.extrude_prologue() else {
                continue;
            };
            let Some(extrude_extent) = prologue.extent() else {
                continue;
            };

            let (counts, side_one_values) = ctx.decode.fold(
                scope_owners,
                ([0usize; 5], [None; 2]),
                |(mut counts, mut side_one_values), owner| {
                    if let Some(parameter) = ctx.decode.get_hash_map(
                        parameters_by_index,
                        &(native_stream, owner.parameter_record_index()),
                        "find F3D Extrude owner parameter",
                    )? {
                        for (index, kind) in PARAMETER_KINDS.into_iter().enumerate() {
                            if ctx.decode.equal(
                                parameter.source_kind(),
                                kind,
                                "compare F3D Extrude parameter kinds",
                            )? {
                                counts[index] += 1;
                                if index == 3 {
                                    if side_one_values[0].is_none() {
                                        side_one_values[0] =
                                            Some(parameter.evaluated_value().get());
                                    } else if side_one_values[1].is_none() {
                                        side_one_values[1] =
                                            Some(parameter.evaluated_value().get());
                                    }
                                }
                                break;
                            }
                        }
                    }
                    Ok((counts, side_one_values))
                },
                "count F3D Extrude parameter kinds",
            )?;
            let [along_count, against_count, profile_offset_count, side_one_offset_count, side_two_offset_count] =
                counts;
            let omitted_zero_side_one_offset =
                crate::design::face_resolve::extrude_omits_zero_side_one_offset(
                    scope,
                    &prologue,
                    side_one_offset_count,
                );
            let side_one_offset_is_absent = match side_one_values[0] {
                None => true,
                Some(0.0) => side_one_values[1].is_none(),
                Some(_) => false,
            };
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
            let class_296_two_faces_layout = scope
                .reference_count_offset()
                .checked_sub(scope.byte_offset())
                .is_some_and(|reference_count_offset| {
                    is_class_296_two_sided_to_faces_layout(
                        scope.class_tag.as_str(),
                        scope.paired_class_tag.as_str(),
                        scope.frame_length(),
                        reference_count_offset,
                        scope.reference_members().len(),
                    )
                });
            let extent_matches_operands = match extrude_extent {
                records::feature::extrude::DesignExtrudeExtent::OneSidedDistance => {
                    has_one_along_carrier
                        && against_count == 0
                        && side_one_offset_is_absent
                        && (!prologue.direction_reversed() || fixed_along_uses_reversal)
                }
                records::feature::extrude::DesignExtrudeExtent::OneSidedToFace => {
                    along_count == 0
                        && !has_fixed_extrude_parameters
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
                    along_count == 1
                        && !has_fixed_extrude_parameters
                        && against_count == 1
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
            let mut face_groups_storage = ctx
                .decode
                .reserve_scoped(0, "hold F3D Extrude face operand groups")?;
            let mut face_groups = face_groups_storage.with_storage(|| -> Result<Vec<_>, CodecError> {
                ctx.decode.admit_iter(scope_groups, "scan F3D Extrude face operand groups")?.try_fold(Vec::new(), |mut groups, group| {
                    if  group.extrude_role().is_some_and(|role| matches!(role, records::topology::extrude_selection::DesignExtrudeOperandRole::Faces(_)))
                    { ctx.decode.push_vec(&mut groups, group, "collect F3D Extrude face operand groups")?; }
                    Ok(groups)
                })
            })?;
            ctx.decode.stable_sort_by(
                &mut face_groups,
                |value| &value.scope_reference_ordinal,
                Ord::cmp,
                "f3d extrude face operand groups sort",
            )?;
            let expected_face_roles: &[_] = match (extrude_start, extrude_extent) {
                (
                    records::feature::extrude::DesignExtrudeStart::FromFace,
                    records::feature::extrude::DesignExtrudeExtent::OneSidedToFace,
                ) if target_shape_group_count == 0 => &[
                    records::topology::extrude_selection::DesignExtrudeFaceRole::Start,
                    records::topology::extrude_selection::DesignExtrudeFaceRole::Termination,
                ],
                (records::feature::extrude::DesignExtrudeStart::FromFace, _) => {
                    &[records::topology::extrude_selection::DesignExtrudeFaceRole::Start]
                }
                (_, records::feature::extrude::DesignExtrudeExtent::OneSidedToFace)
                    if target_shape_group_count == 0 =>
                {
                    &[records::topology::extrude_selection::DesignExtrudeFaceRole::Termination]
                }
                (_, records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces) => &[
                    records::topology::extrude_selection::DesignExtrudeFaceRole::Termination,
                    records::topology::extrude_selection::DesignExtrudeFaceRole::Termination,
                ],
                (_, records::feature::extrude::DesignExtrudeExtent::TwoSidedDistanceToFace) => {
                    &[records::topology::extrude_selection::DesignExtrudeFaceRole::Termination]
                }
                _ => &[],
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
                ctx.push_constant_finding(findings, Check::NativeLinks,
                    "Fusion Design Extrude start or extent conflicts with its parameters and face operands",
                    Some(ctx.decode.copy_retained_text(&scope.id, "retain F3D validation entity")?))?;
            }
        }
        if design::design_feature_family(&scope.kind()) == Some(design::DesignFeatureFamily::Sweep)
        {
            let scope_groups = ctx
                .decode
                .get_hash_map(
                    &ctx.operand_groups_by_scope,
                    &(native_stream, scope.record_index),
                    "find F3D Extrude scope operand groups",
                )?
                .map_or(&[][..], Vec::as_slice);
            let mut first_profile_group = None;
            let mut second_profile_group = None;
            ctx.decode.any_by(
                scope_groups,
                |&group| {
                    if group.role() == DesignOperandRole::PROFILE {
                        if first_profile_group.is_none() {
                            first_profile_group = Some(group);
                        } else {
                            second_profile_group = Some(group);
                            return Ok(true);
                        }
                    }

                    Ok(false)
                },
                "find F3D Sweep profile groups",
            )?;
            let profile_group = first_profile_group;
            let profile_matches_operand = second_profile_group.is_none()
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
                ctx.push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design Sweep profile conflicts with its profile operand group",
                    Some(
                        ctx.decode
                            .copy_retained_text(&scope.id, "retain F3D validation entity")?,
                    ),
                )?;
            }
        }
    }
    Ok(())
}

/// Validate Fillet radius-law parameter assignments; returns the assigned groups.
fn validate_fillet_radius_groups<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<ScopedRecordSet<'a, (&'a str, u32)>, CodecError> {
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D fillet radius groups scratch")?;
    let mut set_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D fillet radius groups result")?;
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let construction_groups_by_index = &ctx.operand_groups_by_index;
    let mut fillet_radius_group_records = HashSet::new();
    let mut fillet_radius_group_slots = HashSet::new();
    for assignment in ctx.decode.admit_iter(
        &native.design_fillet_radius_groups,
        "scan F3D design fillet radius groups",
    )? {
        let native_stream = design_stream(&assignment.id);
        let scope = ctx.decode.get_hash_map(
            scopes_by_index,
            &(native_stream, assignment.scope_record_index),
            "find F3D validation record index",
        )?;
        let group = ctx.decode.get_hash_map(
            construction_groups_by_index,
            &(native_stream, assignment.group_record_index),
            "find F3D validation linked record",
        )?;
        let assignment_parameter = |record_index: u32| -> Result<
            Option<&records::parameters::DesignParameter>,
            CodecError,
        > {
            let Some(&parameter) = ctx.decode.get_hash_map(
                parameters_by_index,
                &(native_stream, record_index),
                "find F3D Fillet assignment parameter",
            )?
            else {
                return Ok(None);
            };
            let Some(owner_record_index) = parameter.owner_record_index() else {
                return Ok(None);
            };
            let Some(&owner) = ctx.decode.get_hash_map(
                owners_by_index,
                &(native_stream, owner_record_index),
                "find F3D Fillet assignment parameter owner",
            )?
            else {
                return Ok(None);
            };
            Ok((owner.scope_record_index() == assignment.scope_record_index
                && owner.parameter_record_index() == record_index)
                .then_some(parameter))
        };
        let tangency_weight = assignment
            .tangency_weight_parameter_record_index
            .map(&assignment_parameter)
            .transpose()?
            .flatten();
        let is_fillet = |scope: &&records::feature::scope::DesignParameterScope| {
            design::design_feature_family(&scope.kind())
                == Some(design::DesignFeatureFamily::Fillet)
        };
        let valid = scope.is_some_and(is_fillet)
            && match group {
                Some(group) => {
                    group.scope_record_index == assignment.scope_record_index
                        && group.members().len() == assignment.edge_operand_record_indices.len()
                        && ctx.decode.all_by(
                            group
                                .members()
                                .iter()
                                .zip(&assignment.edge_operand_record_indices),
                            |(actual, expected)| Ok(actual.value == *expected),
                            "compare F3D Fillet edge operand indices",
                        )?
                }
                None => false,
            }
            && match &assignment.law {
                records::topology::fillet::DesignFilletRadiusLaw::Constant {
                    radius_parameter_record_index,
                } => match assignment_parameter(*radius_parameter_record_index)? {
                    None => false,
                    Some(parameter) => {
                        ctx.decode.equal(
                            parameter.source_kind(),
                            "Radius",
                            "compare F3D Fillet radius parameter kind",
                        )? && parameter
                            .unit()
                            .map(|field| field.value.as_str())
                            .is_some_and(design::feature_project::design_length_unit)
                            && parameter.evaluated_value().get() > 0.0
                    }
                },
                records::topology::fillet::DesignFilletRadiusLaw::Chordal {
                    chord_length_parameter_record_index,
                } => match assignment_parameter(*chord_length_parameter_record_index)? {
                    None => false,
                    Some(parameter) => {
                        ctx.decode.equal(
                            parameter.source_kind(),
                            "ChordLen",
                            "compare F3D Fillet parameter kind",
                        )? && parameter
                            .unit()
                            .map(|field| field.value.as_str())
                            .is_some_and(design::feature_project::design_length_unit)
                            && parameter.evaluated_value().get() > 0.0
                    }
                },
                records::topology::fillet::DesignFilletRadiusLaw::Asymmetric {
                    offset_one_parameter_record_index,
                    offset_two_parameter_record_index,
                } => [
                    (*offset_one_parameter_record_index, "EdgeOffset1"),
                    (*offset_two_parameter_record_index, "EdgeOffset2"),
                ]
                .into_iter()
                .try_fold(
                    true,
                    |valid, (record_index, kind)| -> Result<bool, CodecError> {
                        if !valid {
                            return Ok(false);
                        }
                        Ok(match assignment_parameter(record_index)? {
                            None => false,
                            Some(parameter) => {
                                ctx.decode.equal(
                                    parameter.source_kind(),
                                    kind,
                                    "compare F3D Fillet parameter kind",
                                )? && parameter
                                    .unit()
                                    .map(|field| field.value.as_str())
                                    .is_some_and(design::feature_project::design_length_unit)
                                    && parameter.evaluated_value().get() > 0.0
                            }
                        })
                    },
                )?,
                records::topology::fillet::DesignFilletRadiusLaw::Variable {
                    start_radius_parameter_record_index,
                    end_radius_parameter_record_index,
                    middle: midpoint_records,
                } => {
                    let radius =
                        |record_index: u32, kind: &str| -> Result<Option<f64>, CodecError> {
                            let Some(parameter) = assignment_parameter(record_index)? else {
                                return Ok(None);
                            };
                            Ok((ctx.decode.equal(
                                parameter.source_kind(),
                                kind,
                                "compare F3D Fillet radius parameter kind",
                            )? && parameter
                                .unit()
                                .map(|field| field.value.as_str())
                                .is_some_and(design::feature_project::design_length_unit)
                                && parameter.evaluated_value().get() >= 0.0)
                                .then_some(parameter.evaluated_value().get()))
                        };
                    let start = radius(*start_radius_parameter_record_index, "StartRadius")?;
                    let end = radius(*end_radius_parameter_record_index, "EndRadius")?;
                    let mut middle_positive = false;
                    let mut previous_position = None;
                    let middle_valid = ctx.decode.all_by(
                        midpoint_records.iter(),
                        |row| {
                            let Some(middle_radius) =
                                radius(row.radius_parameter_record_index, "MidRadius")?
                            else {
                                return Ok(false);
                            };
                            let position = match assignment_parameter(row.parameter_record_index)? {
                                None => None,
                                Some(parameter) => (ctx.decode.equal(
                                    parameter.source_kind(),
                                    "MidParams",
                                    "compare F3D midpoint parameter kind",
                                )? && parameter.unit().is_none()
                                    && (0.0..1.0).contains(&parameter.evaluated_value().get()))
                                .then_some(parameter.evaluated_value().get()),
                            };
                            let Some(position) = position else {
                                return Ok(false);
                            };
                            if previous_position.is_some_and(|previous| previous >= position) {
                                return Ok(false);
                            }
                            middle_positive |= middle_radius > 0.0;
                            previous_position = Some(position);

                            Ok(true)
                        },
                        "scan F3D midpoint records",
                    )?;
                    start.zip(end).is_some_and(|(start, end)| {
                        middle_valid && (start > 0.0 || end > 0.0 || middle_positive)
                    })
                }
            }
            && (assignment.tangency_weight_parameter_record_index.is_none()
                || match tangency_weight {
                    None => false,
                    Some(parameter) => {
                        parameter.source_kind() == "TangencyWeight" && parameter.unit().is_none()
                    }
                })
            && set_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut fillet_radius_group_records,
                    (native_stream, assignment.group_record_index),
                    "index F3D Fillet radius group records",
                )
            })?
            && scratch_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut fillet_radius_group_slots,
                    (
                        native_stream,
                        assignment.scope_record_index,
                        assignment.group_ordinal,
                    ),
                    "index F3D Fillet radius group slots",
                )
            })?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Fillet radius group has an invalid parameter assignment",
                Some(
                    ctx.decode
                        .copy_retained_text(&assignment.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok((fillet_radius_group_records, set_storage))
}

/// Report Fillet operand groups that carry no radius assignment.
fn validate_fillet_operand_groups<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    fillet_radius_group_records: &HashSet<(&'a str, u32)>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let scopes_by_index = &ctx.scopes_by_index;
    for group in ctx.decode.admit_iter(
        &native.design_construction_operand_groups,
        "scan F3D design construction operand groups",
    )? {
        let native_stream = design_stream(&group.id);
        let scope = ctx.decode.get_hash_map(
            scopes_by_index,
            &(native_stream, group.scope_record_index),
            "find F3D validation record index",
        )?;
        let is_fillet = scope.is_some_and(|scope| {
            design::design_feature_family(&scope.kind())
                == Some(design::DesignFeatureFamily::Fillet)
        });
        let mut fixed_edge_group_count = 0usize;
        let mut is_fixed_edge_group = false;
        if let Some(scope) = scope {
            for candidate in ctx.decode.admit_iter(
                ctx.decode
                    .get_hash_map(
                        &ctx.operand_groups_by_scope,
                        &(native_stream, scope.record_index),
                        "find F3D Fillet scope operand groups",
                    )?
                    .map_or(&[][..], Vec::as_slice),
                "scan F3D design construction operand groups",
            )? {
                if !candidate.members().is_empty()
                    && ctx.decode.all_by(
                        candidate.members(),
                        |member| {
                            let member = &member.value;

                            ctx.decode.any_by(
                                &native.design_edge_operands,
                                |operand| {
                                    Ok(ctx.decode.equal(
                                        design_stream(&operand.id),
                                        native_stream,
                                        "compare F3D Fillet operand streams",
                                    )? && operand.scope_record_index == scope.record_index
                                        && operand.record_index() == *member)
                                },
                                "find F3D fixed Fillet edge operand",
                            )
                        },
                        "validate F3D fixed Fillet group members",
                    )?
                {
                    fixed_edge_group_count =
                        fixed_edge_group_count.checked_add(1).ok_or_else(|| {
                            ctx.decode.refuse_codec_limit(
                                "count F3D fixed Fillet edge groups",
                                u64::MAX - 1,
                                u64::MAX,
                            )
                        })?;
                    is_fixed_edge_group |= candidate.record_index == group.record_index;
                }
            }
        }
        let has_radius_assignment = ctx.decode.contains_hash_set(
            fillet_radius_group_records,
            &(native_stream, group.record_index),
            "find F3D Fillet radius assignment",
        )?;
        let has_parameter_owner = !ctx
            .scope_owners(
                native_stream,
                group.scope_record_index,
                "find F3D Fillet parameter owner",
            )?
            .is_empty();
        let sole_compact_group_shape = match scope {
            None => false,
            Some(scope) => {
                ctx.decode
                    .get_hash_map(
                        &ctx.operand_groups_by_scope,
                        &(native_stream, scope.record_index),
                        "count F3D Fillet scope operand groups",
                    )?
                    .map_or(0, Vec::len)
                    == 1
                    && ctx.decode.all_by(
                        group.members(),
                        |member| {
                            ctx.decode.any_by(
                                &native.design_edge_identity_operands,
                                |operand| {
                                    Ok(ctx.decode.equal(
                                        design_stream(&operand.id),
                                        native_stream,
                                        "compare F3D Fillet operand streams",
                                    )? && operand.scope_record_index == scope.record_index
                                        && operand.group_record_index == group.record_index
                                        && operand.record_index() == member.value)
                                },
                                "find F3D compact Fillet edge identity operand",
                            )
                        },
                        "validate F3D compact Fillet group members",
                    )?
            }
        };
        let full_round_group_shape = is_fillet
            && group.role() == DesignOperandRole::BODIES_A
            && !has_radius_assignment
            && !has_parameter_owner
            && match scope {
                None => false,
                Some(scope) => {
                    ctx.decode
                        .get_hash_map(
                            &ctx.operand_groups_by_scope,
                            &(native_stream, scope.record_index),
                            "count F3D Fillet scope operand groups",
                        )?
                        .map_or(0, Vec::len)
                        == 1
                        && group.members().len() == 1
                        && ctx.decode.all_by(
                            &native.design_edge_operands,
                            |operand| {
                                Ok(!ctx.decode.equal(
                                    design_stream(&operand.id),
                                    native_stream,
                                    "compare F3D Fillet operand streams",
                                )? || operand.scope_record_index != scope.record_index
                                    || operand.record_index() != group.members()[0].value)
                            },
                            "exclude F3D full-round Fillet edge operand",
                        )?
                        && ctx.decode.any_by(
                            &native.design_face_operands,
                            |operand| {
                                Ok(ctx.decode.equal(
                                    design_stream(&operand.id),
                                    native_stream,
                                    "compare F3D Fillet operand streams",
                                )? && operand.scope_record_index == scope.record_index
                                    && operand.group_record_index() == Some(group.record_index)
                                    && operand.group_member_ordinal() == Some(0)
                                    && operand.record_index() == group.members()[0].value
                                    && operand.recipe_kind
                                        == records::recipes::ConstructionRecipeKind::BoundedFace)
                            },
                            "find F3D full-round Fillet bounded face operand",
                        )?
                }
            };
        let valid_full_round_group = full_round_group_shape
            && !group.frame.variant
            && group.frame.trailing_records().len() == 1
            && group.frame.trailing_flags().len() == 1
            && group.frame.trailing_records()[0].value
                == group.frame.trailing_flags()[0].record_index
            && group.frame.trailing_flags()[0].value
            && ctx.decode.any_by(
                &native.design_face_operands,
                |operand| {
                    Ok(ctx.decode.equal(
                        design_stream(&operand.id),
                        native_stream,
                        "compare F3D Fillet operand streams",
                    )? && operand.scope_record_index == group.scope_record_index
                        && operand.group_record_index() == Some(group.record_index)
                        && operand.group_member_ordinal() == Some(0)
                        && operand.record_index() == group.members()[0].value
                        && !operand.resolved_face_slots.is_empty())
                },
                "find F3D full-round Fillet resolved face operand",
            )?;
        if full_round_group_shape {
            if !valid_full_round_group {
                ctx.push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design Fillet full-round face group is invalid",
                    Some(
                        ctx.decode
                            .copy_retained_text(&group.id, "retain F3D validation entity")?,
                    ),
                )?;
            }
            continue;
        }
        let has_fixed_assignment = match scope
            .and_then(|scope| scope.fixed_fillet_parameters().map(|fixed| (scope, fixed)))
        {
            None => false,
            Some((scope, fixed)) => {
                let unique_reference = |record_index: u32| -> Result<bool, CodecError> {
                    let (plain, located) = scope.reference_members().storage_slices();
                    Ok(ctx
                        .decode
                        .admit_iter(plain, "count F3D fixed Fillet scope references")?
                        .chain(
                            ctx.decode
                                .admit_iter(
                                    located,
                                    "count F3D located fixed Fillet scope references",
                                )?
                                .map(|member| &member.value),
                        )
                        .filter(|member| **member == record_index)
                        .count()
                        == 1)
                };
                ctx.scope_owners(native_stream, scope.record_index, "exclude F3D fixed Fillet parameter owners")?.is_empty()
                && ctx.decode.all_by(&fixed.groups, |fixed_group| {
                    if let Some(tangency) = fixed_group.tangency_weight() {
                        if !unique_reference(tangency.record_index)? { return Ok(false); }
                    }
                    let radii_valid = match fixed_group.law() {
                        records::feature::fixed_parameters::DesignFixedFilletLaw::Constant(radius) => unique_reference(radius.record_index)?,
                        records::feature::fixed_parameters::DesignFixedFilletLaw::Variable { start, end, intermediate } => {
                            unique_reference(start.record_index)? && unique_reference(end.record_index)?
                                && ctx.decode.all_by(intermediate, |row| unique_reference(row.radius.record_index), "validate F3D fixed Fillet intermediate radius references")?
                        }
                    };
                    Ok(radii_valid && ctx.decode.all_by(fixed_group.law().intermediate(), |row| unique_reference(row.parameter.record_index), "validate F3D fixed Fillet intermediate parameter references")?)
                }, "validate F3D fixed Fillet radius groups")?
                && ((fixed_edge_group_count == fixed.groups.len() && is_fixed_edge_group)
                    || (fixed.groups.len() == 1 && sole_compact_group_shape))
            }
        };
        if is_fillet
            && (group.role() == DesignOperandRole::BODIES_B || sole_compact_group_shape)
            && !has_fixed_assignment
            && !has_radius_assignment
        {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Fillet operand group has no radius assignment",
                Some(
                    ctx.decode
                        .copy_retained_text(&group.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate construction operand identity chains; returns identity-backed groups.
fn validate_construction_operand_identities<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<ScopedRecordSet<'a, (&'a str, u32)>, CodecError> {
    let mut set_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D construction operand identities result")?;
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let operand_groups_by_index = &ctx.operand_groups_by_index;
    let mut operand_identity_groups = HashSet::new();
    for identity in ctx.decode.admit_iter(
        &native.design_construction_operand_identities,
        "scan F3D design construction operand identities",
    )? {
        let native_stream = design_stream(&identity.id);
        let group = ctx.decode.get_hash_map(
            operand_groups_by_index,
            &(native_stream, identity.group_record_index),
            "find F3D validation record index",
        )?;
        let selected_scope = match group {
            Some(group) => ctx.decode.get_hash_map(
                scopes_by_index,
                &(native_stream, group.scope_record_index),
                "find F3D selected profile scope",
            )?,
            None => None,
        };
        let selected_profile = selected_scope.and_then(|scope| scope.extrude_profile());
        let wrapper_shape = ctx.decode.all_by(
            identity.wrappers(),
            |wrapper| {
                Ok(
                    match ctx.decode.get_hash_map(
                        records_by_index,
                        &(native_stream, wrapper.record_index),
                        "find F3D construction identity header",
                    )? {
                        Some(header) => {
                            header.byte_offset == wrapper.byte_offset
                                && (header.class_tag == wrapper.class_tag)
                        }
                        None => false,
                    },
                )
            },
            "validate F3D construction identity wrappers",
        )?;
        let transform = group.and_then(|group| group.frame.trailing_transforms().first());
        let tracking_shape = match identity.tracking_path() {
            None => true,
            Some(path) => {
                (match ctx.decode.get_hash_map(
                    records_by_index,
                    &(native_stream, path.wrapper_record_index()),
                    "find F3D construction identity header",
                )? {
                    Some(header) => {
                        header.byte_offset == path.wrapper_byte_offset()
                            && (header.class_tag == path.wrapper_class_tag)
                    }
                    None => false,
                }) && (match ctx.decode.get_hash_map(
                    records_by_index,
                    &(native_stream, path.carrier_record_index()),
                    "find F3D construction identity header",
                )? {
                    Some(header) => {
                        header.byte_offset == path.carrier_byte_offset()
                            && (header.class_tag == path.carrier_class_tag)
                    }
                    None => false,
                }) && (match ctx.decode.get_hash_map(
                    records_by_index,
                    &(native_stream, path.following_record_index()),
                    "find F3D construction identity header",
                )? {
                    Some(header) => {
                        header.byte_offset == path.following_byte_offset()
                            && (header.class_tag == path.following_class_tag)
                    }
                    None => false,
                })
            }
        };
        let chain_entry_shape = if let Some(path) = identity.tracking_path() {
            !identity.wrappers().is_empty()
                || (identity.wrappers().is_empty()
                    && match transform {
                        None => false,
                        Some(transform) => {
                            path.wrapper_record_index() == transform.following_record_index()
                                && path.wrapper_byte_offset() == transform.following_byte_offset()
                                && (path.wrapper_class_tag == transform.following_class_tag)
                        }
                    })
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
            (if identity.tracking_path().is_some() || !identity.wrappers().is_empty() {
                true
            } else {
                match transform {
                    None => false,
                    Some(transform) => {
                        identity.following_record_index() == transform.following_record_index()
                            && identity.following_byte_offset() == transform.following_byte_offset()
                            && (identity.following_class_tag() == &transform.following_class_tag)
                    }
                }
            }) && (match ctx.decode.get_hash_map(
                records_by_index,
                &(native_stream, identity.following_record_index()),
                "find F3D construction identity header",
            )? {
                Some(header) => {
                    header.byte_offset == identity.following_byte_offset()
                        && (&header.class_tag == identity.following_class_tag())
                }
                None => false,
            });
        let persistent_shape = match identity.persistent_identity() {
            None => true,
            Some(persistent) => {
                (match selected_profile {
                    None => true,
                    Some(profile) => profile.asset_id == persistent.asset_id,
                }) && (if persistent.next_record_index != 0 {
                    true
                } else if identity.following_byte_offset().checked_add(190)
                    != Some(persistent.next_byte_offset())
                {
                    false
                } else {
                    !ctx.decode.contains_hash_set(
                        &ctx.header_offsets,
                        &(native_stream, persistent.next_byte_offset()),
                        "find F3D construction persistent terminal header",
                    )?
                }) && ctx
                    .decode
                    .get_hash_map(
                        records_by_index,
                        &(native_stream, persistent.next_record_index),
                        "find F3D construction persistent next record",
                    )?
                    // The header arena indexes records named by Design entity
                    // reference lists. A nested identity can terminate at a
                    // structurally parsed record that no entity names, so the
                    // arena is not an exhaustive index of terminal records.
                    .is_none_or(|header| header.byte_offset == persistent.next_byte_offset())
            }
        };
        let valid = group.is_some_and(|group| {
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
            && set_storage.with_storage(|| { ctx.decode.insert_hash_set(&mut operand_identity_groups, (native_stream, identity.group_record_index), "index F3D construction operand identity groups") })?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design construction operand identity has an invalid nested frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&identity.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok((operand_identity_groups, set_storage))
}

/// Validate edge identity operands; returns their backing record set.
fn validate_edge_identity_operands<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    expected_face_operands: &[records::topology::face::DesignFaceOperand],
) -> Result<ScopedRecordSet<'a, (&'a str, u32)>, CodecError> {
    let decode = ctx.decode;
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D edge identity operands scratch")?;
    let mut set_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D edge identity operands result")?;
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let operand_groups_by_index = &ctx.operand_groups_by_index;
    let (mut expected_edge_identity_operands_storage, mut expected_edge_identity_operands) =
        reload_native_arena(decode, ctx.ir, "design_edge_identity_operands")?;
    expected_edge_identity_operands_storage.with_storage(|| {
        history::selection::bind_edge_identity_history(
            decode,
            &mut expected_edge_identity_operands,
            &native.design_construction_operand_identities,
            &native.design_parameter_scopes,
            &native.asm_histories,
            &ctx.scope_histories,
        )
    })?;
    expected_edge_identity_operands_storage.with_storage(|| {
        history::selection::bind_edge_identity_bounded_face_rules(
            decode,
            &mut expected_edge_identity_operands,
            expected_face_operands,
        )
    })?;
    let expected_edge_identity_operands = scratch_storage.with_storage(|| {
        decode.collect_hash_map(
            expected_edge_identity_operands
                .iter()
                .map(|operand| (operand.id.as_str(), operand)),
            "index F3D expected edge identity operands",
        )
    })?;
    let mut edge_identity_slots = HashSet::new();
    let mut edge_identity_records = HashSet::new();
    for operand in ctx.decode.admit_iter(
        &native.design_edge_identity_operands,
        "scan F3D design edge identity operands",
    )? {
        let native_stream = design_stream(&operand.id);
        let scope = ctx.decode.get_hash_map(
            scopes_by_index,
            &(native_stream, operand.scope_record_index),
            "find F3D validation record index",
        )?;
        let group = ctx.decode.get_hash_map(
            operand_groups_by_index,
            &(native_stream, operand.group_record_index),
            "find F3D validation record index",
        )?;
        let header = ctx.decode.get_hash_map(
            records_by_index,
            &(native_stream, operand.record_index()),
            "find F3D validation record index",
        )?;
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
        }) && (match header {
            Some(header) => {
                header.byte_offset == operand.byte_offset()
                    && (header.class_tag == operand.class_tag)
            }
            None => false,
        }) && match ctx.decode.get_hash_map(
            &expected_edge_identity_operands,
            operand.id.as_str(),
            "find F3D expected operand",
        )? {
            Some(expected) => ctx.decode.equal(
                *expected,
                operand,
                "compare F3D expected edge identity operand",
            )?,
            None => false,
        } && scratch_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut edge_identity_slots,
                (
                    native_stream,
                    operand.group_record_index,
                    operand.group_member_ordinal,
                ),
                "index F3D edge identity slots",
            )
        })? && set_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut edge_identity_records,
                (native_stream, operand.record_index()),
                "index F3D edge identity records",
            )
        })?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design edge identity operand has an invalid fixed frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&operand.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok((edge_identity_records, set_storage))
}

/// Validate whole-body recipe operands; returns their backing record set.
fn validate_body_recipe_operands<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<ScopedRecordSet<'a, (&'a str, u32)>, CodecError> {
    let decode = ctx.decode;
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D body recipe operands scratch")?;
    let mut set_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D body recipe operands result")?;
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let operand_groups_by_index = &ctx.operand_groups_by_index;
    let recipes_by_id = &ctx.recipes_by_id;
    let (mut expected_operands_storage, mut expected_operands) =
        reload_native_arena(decode, ctx.ir, "design_body_recipe_operands")?;
    if let Err(error) = expected_operands_storage.with_storage(|| {
        design::decode::operands::bind_body_recipe_operand_candidates(
            ctx.decode,
            &mut expected_operands,
            &native.construction_recipes,
            &native.persistent_subentity_tags,
            &native.design_parameter_scopes,
        )
    }) {
        if matches!(error, CodecError::ResourceLimit(_)) {
            return Err(error);
        }
        let message = ctx.decode.format_retained(
            format_args!("Fusion Design body-recipe candidate binding failed: {error}"),
            "retain F3D body-recipe candidate binding finding",
        )?;
        ctx.decode.push_vec(
            findings,
            Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message,
                entity: None,
            },
            "collect F3D native validation findings",
        )?;
    }
    expected_operands_storage.with_storage(|| {
        history::bind_body_recipe_operand_history_candidates(
            decode,
            &mut expected_operands,
            &native.construction_recipes,
            &native.design_parameter_scopes,
            &native.asm_histories,
        )
    })?;
    let expected_operands = scratch_storage.with_storage(|| {
        decode.collect_hash_map(
            expected_operands
                .iter()
                .map(|operand| (operand.id.as_str(), operand)),
            "index F3D expected body recipe operands",
        )
    })?;
    let mut member_slots = HashSet::new();
    let mut operand_records = HashSet::new();
    for operand in ctx.decode.admit_iter(
        &native.design_body_recipe_operands,
        "scan F3D design body recipe operands",
    )? {
        let native_stream = design_stream(&operand.id);
        let scope = ctx.decode.get_hash_map(
            scopes_by_index,
            &(native_stream, operand.scope_record_index),
            "find F3D validation record index",
        )?;
        let header = ctx.decode.get_hash_map(
            records_by_index,
            &(native_stream, operand.record_index()),
            "find F3D validation record index",
        )?;
        let recipe = ctx.decode.get_hash_map(
            recipes_by_id,
            operand.recipe_id.as_str(),
            "find F3D validation record index",
        )?;
        let valid_owner = match scope {
            None => false,
            Some(scope) => match operand.owner {
                records::topology::body_recipe::DesignOperandOwner::Group {
                    group_record_index,
                    group_member_ordinal,
                } => ctx
                    .decode
                    .get_hash_map(
                        operand_groups_by_index,
                        &(native_stream, group_record_index),
                        "find F3D body recipe owner group",
                    )?
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
                    ((scope.kind() == crate::records::feature::scope::DesignFeatureKind::Hole)
                        || (!scope_reference_ordinal.is_multiple_of(2)
                            && (match scope.combine_operation() {
                                None => false,
                                Some(operation) => {
                                    operation.target_record_index == operand.record_index()
                                        || operation.tools.first.record_index
                                            == operand.record_index()
                                        || ctx.decode.any_by(
                                            &operation.tools.additional,
                                            |tool| Ok(tool.record_index == operand.record_index()),
                                            "find F3D body recipe combine tool",
                                        )?
                                }
                            })))
                        && match usize::try_from(scope_reference_ordinal) {
                            Err(_) => false,
                            Ok(ordinal) => {
                                reference_member_at(scope.reference_members(), ordinal)
                                    == Some(&operand.record_index())
                            }
                        }
                }
            },
        };
        let valid = valid_owner
            && (match header {
                Some(header) => {
                    header.byte_offset == operand.byte_offset()
                        && (header.class_tag == operand.class_tag)
                }
                None => false,
            })
            && body_recipe_reference_table_is_admitted(decode, scope.copied(), operand)?
            && (match recipe {
                None => false,
                Some(recipe) => {
                    let selector_is_valid = recipe.design.as_ref().is_some_and(|design| {
                        let design_id = &design.id;
                        let design_id_offset = design_id.offset;
                        let Some(selector) = design.selector else {
                            return false;
                        };
                        u64::try_from(design_id.value.len())
                            .ok()
                            .is_some_and(|length| {
                                let selector_follows_id = design_id_offset.checked_add(length)
                                    == Some(selector.byte_offset);
                                let prefix_frame = selector.byte_offset.checked_add(20)
                                    == Some(recipe.byte_offset);
                                let body_suffix_frame = recipe
                                    .byte_offset
                                    .checked_add(u64_from_index(b"body_recipe_data".len()))
                                    .and_then(|offset| offset.checked_add(12))
                                    == Some(design_id_offset)
                                    && selector.value == operand.next_record_index();
                                selector_follows_id
                                    && selector.value != 0
                                    && (prefix_frame || body_suffix_frame)
                            })
                    });
                    ctx.decode.equal(
                        design_stream(&recipe.id),
                        native_stream,
                        "compare F3D body recipe streams",
                    )? && recipe.kind == records::recipes::ConstructionRecipeKind::Body
                        && recipe.byte_offset > operand.context_id_offset()
                        && recipe.byte_offset < operand.next_byte_offset()
                        && selector_is_valid
                }
            })
            && match ctx.decode.get_hash_map(
                &expected_operands,
                operand.id.as_str(),
                "find F3D expected operand",
            )? {
                Some(expected) => ctx.decode.equal(
                    *expected,
                    operand,
                    "compare F3D expected body recipe operand",
                )?,
                None => false,
            }
            && scratch_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut member_slots,
                    (native_stream, operand.scope_record_index, operand.owner),
                    "index F3D body recipe member slots",
                )
            })?
            && set_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut operand_records,
                    (native_stream, operand.record_index()),
                    "index F3D body recipe records",
                )
            })?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design body recipe operand has an invalid nested frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&operand.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok((operand_records, set_storage))
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
) -> Result<(), CodecError> {
    let native = ctx.native;
    if native.design_construction_operand_groups.is_empty() {
        return Ok(());
    }
    let mut index_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D operand carrier indexes")?;
    let identity_groups = group_records(
        ctx.decode,
        &mut index_storage,
        &native.design_edge_identity_operands,
        |operand| {
            Some((
                design_stream(&operand.id),
                operand.scope_record_index,
                operand.group_record_index,
            ))
        },
        "index F3D identity carrier groups",
    )?;
    let entity_groups = group_records(
        ctx.decode,
        &mut index_storage,
        &native.design_entity_selection_operands,
        |operand| {
            Some((
                design_stream(&operand.id),
                operand.scope_record_index,
                operand.group_record_index,
            ))
        },
        "index F3D entity selection carrier groups",
    )?;
    let face_groups = group_records(
        ctx.decode,
        &mut index_storage,
        &native.design_face_operands,
        |operand| {
            Some((
                design_stream(&operand.id),
                operand.scope_record_index,
                operand.group_record_index()?,
            ))
        },
        "index F3D face carrier groups",
    )?;
    let body_recipe_groups = group_records(
        ctx.decode,
        &mut index_storage,
        &native.design_body_recipe_operands,
        |operand| {
            Some((
                design_stream(&operand.id),
                operand.scope_record_index,
                operand.owner.group()?.0,
            ))
        },
        "index F3D body recipe carrier groups",
    )?;
    for group in ctx.decode.admit_iter(
        &native.design_construction_operand_groups,
        "scan F3D design construction operand groups",
    )? {
        let native_stream = design_stream(&group.id);
        let key = (native_stream, group.scope_record_index, group.record_index);
        let identity_candidates = ctx
            .decode
            .get_hash_map(&identity_groups, &key, "find F3D identity carrier group")?
            .map_or(&[][..], Vec::as_slice);
        let (mut identity_members, _identity_storage) =
            ctx.decode
                .with_scoped_storage("hold F3D operand group identity members", || {
                    ctx.decode.collect_vec(
                        identity_candidates.iter().copied(),
                        "collect F3D operand group identity members",
                    )
                })?;
        ctx.decode.stable_sort_by(
            &mut identity_members,
            |value| &value.group_member_ordinal,
            Ord::cmp,
            "f3d operand group identity members sort",
        )?;
        let has_exact_identity_members = !group.members().is_empty()
            && identity_members.len() == group.members().len()
            && ctx.decode.all_by(
                identity_members.iter().enumerate(),
                |(ordinal, operand)| -> Result<bool, CodecError> {
                    Ok(usize::try_from(operand.group_member_ordinal) == Ok(ordinal)
                        && group.members().get(ordinal).map(|member| &member.value)
                            == Some(&operand.record_index())
                        && ctx.decode.contains_hash_set(
                            edge_identity_records,
                            &(native_stream, operand.record_index()),
                            "find F3D edge identity record",
                        )?)
                },
                "validate F3D operand group identity members",
            )?;
        let has_exact_entity_selection_members = !group.members().is_empty()
            && ctx.decode.all_by(
                group.members().iter().enumerate(),
                |(ordinal, member)| -> Result<bool, CodecError> {
                    let Some(ordinal) = id_from_index(ordinal) else {
                        return Ok(false);
                    };
                    ctx.decode.any_by(
                        ctx.decode
                            .get_hash_map(
                                &entity_groups,
                                &key,
                                "find F3D entity selection carrier group",
                            )?
                            .map_or(&[][..], Vec::as_slice),
                        |operand| {
                            Ok(operand.group_member_ordinal == ordinal
                                && operand.record_index() == member.value)
                        },
                        "find F3D operand group entity_selection carrier",
                    )
                },
                "validate F3D operand group entity_selection members",
            )?;
        let has_exact_face_members = !group.members().is_empty()
            && ctx.decode.all_by(
                group.members().iter().enumerate(),
                |(ordinal, member)| -> Result<bool, CodecError> {
                    let Some(ordinal) = id_from_index(ordinal) else {
                        return Ok(false);
                    };
                    ctx.decode.any_by(
                        ctx.decode
                            .get_hash_map(&face_groups, &key, "find F3D face carrier group")?
                            .map_or(&[][..], Vec::as_slice),
                        |operand| {
                            Ok(operand.group_member_ordinal() == Some(ordinal)
                                && operand.record_index() == member.value)
                        },
                        "find F3D operand group face carrier",
                    )
                },
                "validate F3D operand group face members",
            )?;
        let has_exact_body_recipe_members = !group.members().is_empty()
            && ctx.decode.all_by(
                group.members().iter().enumerate(),
                |(ordinal, member)| -> Result<bool, CodecError> {
                    let Some(ordinal) = id_from_index(ordinal) else {
                        return Ok(false);
                    };
                    ctx.decode.any_by(
                        ctx.decode
                            .get_hash_map(
                                &body_recipe_groups,
                                &key,
                                "find F3D body recipe carrier group",
                            )?
                            .map_or(&[][..], Vec::as_slice),
                        |operand| {
                            Ok(operand.owner.group() == Some((group.record_index, ordinal))
                                && operand.record_index() == member.value
                                && ctx.decode.contains_hash_set(
                                    body_recipe_operand_records,
                                    &(native_stream, operand.record_index()),
                                    "find F3D body recipe operand record",
                                )?)
                        },
                        "find F3D operand group body_recipe carrier",
                    )
                },
                "validate F3D operand group body_recipe members",
            )?;
        let has_exact_topology_recipe_members = !group.members().is_empty()
            && ctx.decode.all_by(
                group.members(),
                |member| {
                    Ok(ctx.decode.contains_hash_set(
                        edge_operand_records,
                        &(native_stream, member.value),
                        "find F3D edge operand record",
                    )? || ctx.decode.contains_hash_set(
                        edge_treatment_vertex_records,
                        &(native_stream, member.value),
                        "find F3D edge treatment vertex record",
                    )?)
                },
                "validate F3D topology recipe members",
            )?;
        let has_exact_sketch_profile_member = group.members().len() == 1
            && match ctx.decode.get_hash_map(
                &ctx.scopes_by_index,
                &(native_stream, group.scope_record_index),
                "find F3D operand group profile scope",
            )? {
                Some(scope) => scope
                    .extrude_profile()
                    .or(scope.sweep_profile())
                    .or(scope.base_flange_profile())
                    .is_some_and(|profile| group.members()[0].value == profile.record_index),
                None => false,
            };
        let has_exact_group_members = !group.members().is_empty()
            && ctx.decode.all_by(
                group.members(),
                |record| {
                    ctx.decode.any_by(
                        ctx.operand_group_records(
                            native_stream,
                            record.value,
                            "find F3D nested operand group records",
                        )?,
                        |member| {
                            Ok(member.scope_record_index == group.scope_record_index
                                && member.scope_reference_ordinal > group.scope_reference_ordinal)
                        },
                        "find F3D nested operand group",
                    )
                },
                "validate F3D nested operand group members",
            )?;
        let has_exact_trailing_carrier = group.frame.trailing_records().is_empty()
            || ctx.decode.contains_hash_set(
                operand_identity_groups,
                &(native_stream, group.record_index),
                "find F3D operand identity group",
            )?
            || (group.frame.trailing_transforms().len()
                + group.frame.trailing_dual_transforms().len()
                + group.frame.trailing_flags().len()
                == group.frame.trailing_records().len()
                && ctx.decode.all_by(
                    group.frame.trailing_records(),
                    |record| {
                        Ok(ctx.decode.any_by(
                            group.frame.trailing_transforms(),
                            |transform| Ok(transform.record_index() == record.value),
                            "find F3D trailing transform",
                        )? || ctx.decode.any_by(
                            group.frame.trailing_dual_transforms(),
                            |transform| Ok(transform.record_index == record.value),
                            "find F3D trailing dual transform",
                        )? || ctx.decode.any_by(
                            group.frame.trailing_flags(),
                            |flag| Ok(flag.record_index == record.value),
                            "find F3D trailing flag",
                        )?)
                    },
                    "validate F3D trailing operand carriers",
                )?);
        let has_exact_member_carrier = ctx.decode.contains_hash_set(
            operand_identity_groups,
            &(native_stream, group.record_index),
            "find F3D operand identity group",
        )? || has_exact_identity_members
            || has_exact_entity_selection_members
            || has_exact_face_members
            || has_exact_body_recipe_members
            || has_exact_topology_recipe_members
            || has_exact_sketch_profile_member
            || has_exact_group_members;
        if !has_exact_member_carrier {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design construction operand group has no exact typed member",
                Some(
                    ctx.decode
                        .copy_retained_text(&group.id, "retain F3D validation entity")?,
                ),
            )?;
        }
        if !has_exact_trailing_carrier {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design construction operand group has no exact trailing carrier",
                Some(
                    ctx.decode
                        .copy_retained_text(&group.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate Extrude selection members against their resolved sketch geometry.
fn validate_extrude_selection_members(
    ctx: &Ctx,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D extrude selection members scratch")?;
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    if native.design_extrude_selection_members.is_empty() {
        return Ok(());
    }
    let mut target_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D Extrude selection target entries")?;
    let mut target_entries = Vec::new();
    for point in ctx.decode.admit_iter(
        &native.sketch_points,
        "scan F3D Extrude selection point targets",
    )? {
        if let (Some(owner), Some(local_id)) = (point.owner_reference, point.persistent_id()) {
            target_storage.with_storage(|| {
                ctx.decode.push_vec(
                    &mut target_entries,
                    (
                        (design_stream(&point.id), owner, local_id),
                        records::sketch_relations::SketchRelationOperand::Point {
                            record_index: point.record_index,
                            persistent_id: point.persistent_id(),
                        },
                    ),
                    "collect F3D Extrude selection targets",
                )
            })?;
        }
    }
    for curve in ctx.decode.admit_iter(
        &native.sketch_curve_identities,
        "scan F3D Extrude selection curve targets",
    )? {
        if let Some(owner) = curve.owner_reference {
            let target = records::sketch_relations::SketchRelationOperand::Curve {
                record_index: curve.record_index,
                primary_id: curve.primary_id.get(),
                secondary_id: curve.secondary_id,
            };
            target_storage.with_storage(|| {
                ctx.decode.push_vec(
                    &mut target_entries,
                    (
                        (design_stream(&curve.id), owner, curve.primary_id.get()),
                        target.clone(),
                    ),
                    "collect F3D Extrude selection targets",
                )
            })?;
            if curve.secondary_id != 0 && curve.secondary_id != curve.primary_id.get() {
                target_storage.with_storage(|| {
                    ctx.decode.push_vec(
                        &mut target_entries,
                        (
                            (design_stream(&curve.id), owner, curve.secondary_id),
                            target,
                        ),
                        "collect F3D Extrude selection targets",
                    )
                })?;
            }
        }
    }
    let (targets, _targets_storage) = ctx
        .decode
        .unique_index(target_entries, "index F3D Extrude selection targets")?;
    drop(target_storage);
    let identities_by_member = group_records(
        ctx.decode,
        &mut scratch_storage,
        &native.design_construction_operand_identities,
        |identity| {
            Some((
                design_stream(&identity.id),
                identity.following_record_index(),
                identity.following_byte_offset(),
            ))
        },
        "index F3D Extrude selection identities",
    )?;
    let historical_candidates_retained =
        history::projection_was_finalized(ctx.decode, &native.asm_histories)?;
    let scopes_by_index = &ctx.scopes_by_index;
    let groups_by_index = &ctx.groups_by_index;
    let mut member_slots = HashSet::new();
    let mut member_records = HashSet::new();
    for member in ctx.decode.admit_iter(
        &native.design_extrude_selection_members,
        "scan F3D design extrude selection members",
    )? {
        let native_stream = design_stream(&member.id);
        let group = ctx.decode.get_hash_map(
            groups_by_index,
            &(native_stream, member.group_record_index),
            "find F3D validation record index",
        )?;
        let header = ctx.decode.get_hash_map(
            records_by_index,
            &(native_stream, member.record_index()),
            "find F3D validation record index",
        )?;
        let selected_scope = match group {
            Some(group) => ctx.decode.get_hash_map(
                scopes_by_index,
                &(native_stream, group.scope_record_index),
                "find F3D selected profile scope",
            )?,
            None => None,
        };
        let selected_profile = selected_scope.and_then(|scope| scope.extrude_profile());
        let selected_sketch =
            selected_profile.and_then(|profile| u32::try_from(profile.entity_id.suffix()).ok());
        let expected_target = match selected_sketch {
            Some(sketch) => ctx
                .decode
                .get_hash_map(
                    &targets,
                    &(native_stream, sketch, member.local_id),
                    "find F3D Extrude selection target",
                )?
                .and_then(Option::as_ref)
                .cloned(),
            None => None,
        };
        let mut expected_identities_storage = ctx
            .decode
            .reserve_scoped(0, "collect F3D Extrude selection identities")?;
        let mut expected_identities = Vec::new();
        let identity_candidates = ctx
            .decode
            .get_hash_map(
                &identities_by_member,
                &(native_stream, member.record_index(), member.byte_offset()),
                "find F3D Extrude selection identity candidates",
            )?
            .map_or(&[][..], Vec::as_slice);
        for &identity in ctx
            .decode
            .admit_iter(identity_candidates, "scan F3D Extrude selection identities")?
        {
            let Some(persistent) = identity.persistent_identity() else {
                continue;
            };
            if persistent.local_id != member.local_id
                || (persistent.asset_id != member.asset_id)
                || (persistent.context_id != member.context_id)
            {
                continue;
            }
            ctx.decode.push_scoped_vec(
                &mut expected_identities_storage,
                &mut expected_identities,
                identity,
                "collect F3D Extrude selection identities",
            )?;
        }
        ctx.decode.stable_sort_by_key(
            &mut expected_identities,
            |value| value.wrappers().first().map(|wrapper| wrapper.byte_offset),
            Ord::cmp,
            "f3d extrude selection identities sort",
        )?;
        let (expected_history, _expected_history_storage) = ctx.decode.with_scoped_storage(
            "hold F3D expected Extrude selection history",
            || {
                history::selection::historical_extrude_selection_identity_kind(
                    ctx.decode,
                    member,
                    &native.design_component_naming_spaces,
                    &native.design_body_bindings,
                    &native.asm_histories,
                )
            },
        )?;
        let history_matches = if historical_candidates_retained {
            if let Some(binding) = member.historical.as_ref() {
                let (state_ids, _state_ids_storage) = ctx.decode.with_scoped_storage(
                    "hold F3D Extrude selection history states",
                    || {
                        ctx.decode.collect_hash_set(
                            binding.state_ids.iter().copied(),
                            "index F3D Extrude selection history states",
                        )
                    },
                )?;
                state_ids.len() == binding.state_ids.len()
                    && ctx.decode.all_by(
                        &binding.state_ids,
                        |state_id| {
                            Ok(ctx
                                .decode
                                .get_hash_map(
                                    &ctx.states_by_id,
                                    state_id,
                                    "find F3D Extrude selection history state",
                                )?
                                .is_some())
                        },
                        "validate F3D Extrude selection history states",
                    )?
            } else {
                true
            }
        } else {
            ctx.decode.equal(
                &expected_history
                    .as_ref()
                    .map(|(kind, entity_ref, states)| (*kind, *entity_ref, states.as_slice())),
                &member.historical.as_ref().map(|binding| {
                    (
                        binding.kind,
                        binding.entity_ref,
                        binding.state_ids.as_slice(),
                    )
                }),
                "compare F3D Extrude selection history identities",
            )?
        };
        let terminal_next = if member.next_record_index != 0 {
            false
        } else {
            !ctx.decode.contains_hash_set(
                &ctx.header_offsets,
                &(native_stream, member.next_byte_offset()),
                "find F3D Extrude selection next header",
            )?
        };
        let valid = group.is_some_and(|group| {
            usize::try_from(member.group_member_ordinal)
                .ok()
                .and_then(|ordinal| group.members().get(ordinal))
                .map(|reference| reference.value)
                == Some(member.record_index())
        }) && (match header {
            Some(header) => {
                header.byte_offset == member.byte_offset() && (header.class_tag == member.class_tag)
            }
            None => false,
        }) && (match selected_profile {
            Some(profile) => profile.asset_id == member.asset_id,
            None => true,
        }) && member.resolved_geometry == expected_target
            && member.operand_identity_ids.len() == expected_identities.len()
            && ctx.decode.all_by(
                member.operand_identity_ids.iter().zip(&expected_identities),
                |(actual, expected)| {
                    ctx.decode.equal(
                        actual.as_str(),
                        expected.id.as_str(),
                        "compare F3D Extrude selection identity ids",
                    )
                },
                "scan F3D Extrude selection identity ids",
            )?
            && history_matches
            && (member.next_record_index != 0 || terminal_next)
            && scratch_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut member_slots,
                    (
                        native_stream,
                        member.group_record_index,
                        member.group_member_ordinal,
                    ),
                    "index F3D Extrude selection member slots",
                )
            })?
            && scratch_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut member_records,
                    (native_stream, member.record_index()),
                    "index F3D Extrude selection member records",
                )
            })?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Extrude selection member has an invalid fixed frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&member.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate entity-selection operand nested frames.
fn validate_entity_selection_operands(
    ctx: &Ctx,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D entity selection operands scratch")?;
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let operand_groups_by_index = &ctx.operand_groups_by_index;
    let mut entity_selection_slots = HashSet::new();
    for operand in ctx.decode.admit_iter(
        &native.design_entity_selection_operands,
        "scan F3D design entity selection operands",
    )? {
        let native_stream = design_stream(&operand.id);
        let group = ctx.decode.get_hash_map(
            operand_groups_by_index,
            &(native_stream, operand.group_record_index),
            "find F3D validation record index",
        )?;
        let header = ctx.decode.get_hash_map(
            records_by_index,
            &(native_stream, operand.record_index()),
            "find F3D validation record index",
        )?;
        let valid = group.is_some_and(|group| {
            group.scope_record_index == operand.scope_record_index
                && usize::try_from(operand.group_member_ordinal)
                    .ok()
                    .and_then(|ordinal| group.members().get(ordinal).map(|member| &member.value))
                    == Some(&operand.record_index())
        }) && (match header {
            Some(header) => {
                header.byte_offset == operand.byte_offset()
                    && (&header.class_tag == operand.class_tag())
            }
            None => false,
        }) && scratch_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut entity_selection_slots,
                (
                    native_stream,
                    operand.group_record_index,
                    operand.group_member_ordinal,
                ),
                "index F3D entity selection slots",
            )
        })?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design entity-selection operand has an invalid nested frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&operand.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Report Extrude selection groups with missing or inconsistent members.
fn validate_extrude_selection_group_members(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let members_by_slot = &ctx.members_by_slot;
    for group in ctx.decode.admit_iter(
        &native.design_extrude_selection_groups,
        "scan F3D design extrude selection groups",
    )? {
        let native_stream = design_stream(&group.id);
        let complete = ctx.decode.all_by(
            group.members().iter().enumerate(),
            |(index, _)| {
                let Ok(ordinal) = u32::try_from(index) else {
                    return Ok(false);
                };
                let Some(member) = ctx.decode.get_hash_map(
                    members_by_slot,
                    &(native_stream, group.record_index, ordinal),
                    "find F3D Extrude selection member",
                )?
                else {
                    return Ok(false);
                };
                let next = usize::try_from(ordinal)
                    .ok()
                    .and_then(|ordinal| ordinal.checked_add(1))
                    .and_then(|next_index| group.members().get(next_index));
                if let Some(next_record_index) = next {
                    let Some(next_ordinal) = ordinal.checked_add(1) else {
                        return Ok(false);
                    };
                    let next_member = ctx.decode.get_hash_map(
                        members_by_slot,
                        &(native_stream, group.record_index, next_ordinal),
                        "find F3D next Extrude selection member",
                    )?;
                    if member.next_record_index != next_record_index.value
                        || !next_member.is_some_and(|next_member| {
                            member.next_byte_offset() == next_member.byte_offset()
                        })
                    {
                        return Ok(false);
                    }
                }

                Ok(true)
            },
            "validate F3D Extrude selection member frames",
        )?;
        let context_id = ctx
            .decode
            .get_hash_map(
                members_by_slot,
                &(native_stream, group.record_index, 0),
                "find F3D first group member",
            )?
            .map(|member| member.context_id.as_str());
        let context_consistent = if let Some(context_id) = context_id {
            let consistent = ctx.decode.all_by(
                group.members().iter().enumerate(),
                |(index, _)| {
                    let Ok(ordinal) = u32::try_from(index) else {
                        return Ok(false);
                    };
                    let member = ctx.decode.get_hash_map(
                        members_by_slot,
                        &(native_stream, group.record_index, ordinal),
                        "find F3D Extrude selection context",
                    )?;
                    let Some(member) = member else {
                        return Ok(false);
                    };
                    if member.context_id.as_str() != context_id {
                        return Ok(false);
                    }

                    Ok(true)
                },
                "validate F3D Extrude selection member contexts",
            )?;
            consistent
        } else {
            false
        };
        if !(complete && context_consistent) {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Extrude selection group has missing members",
                Some(
                    ctx.decode
                        .copy_retained_text(&group.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
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
    decode: &DecodeContext<'_>,
    actual: &[records::dimensions::DesignRecipeReference],
    expected: &[records::dimensions::DesignRecipeReference],
    ignore_derived_candidates: bool,
) -> Result<bool, CodecError> {
    if !ignore_derived_candidates {
        return decode.equal(actual, expected, "compare F3D recipe references");
    }
    if actual.len() != expected.len() {
        return Ok(false);
    }
    decode.all_by(
        actual.iter().zip(expected),
        |(actual, expected)| {
            Ok(actual.selector == expected.selector
                && actual.selector_offset == expected.selector_offset
                && decode.equal(
                    &actual.token,
                    &expected.token,
                    "compare F3D recipe reference token",
                )?
                && actual.token_offset == expected.token_offset
                && actual.design_reference == expected.design_reference
                && actual.design_reference_offset == expected.design_reference_offset)
        },
        "scan F3D actual recipe reference frames",
    )
}

/// Validate edge operands and their recipe frames; returns their record set.
fn validate_edge_operands<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<ScopedRecordSet<'a, (&'a str, u32)>, CodecError> {
    let decode = ctx.decode;
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D edge operands scratch")?;
    let mut set_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D edge operands result")?;
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let recipes_by_id = &ctx.recipes_by_id;
    let scopes_by_index = &ctx.scopes_by_index;
    let historical_candidates_retained =
        history::projection_was_finalized(decode, &native.asm_histories)?;
    let mut edge_operand_slots = HashSet::new();
    let mut edge_operand_records = HashSet::new();
    let (mut expected_edge_operands_storage, mut expected_edge_operands) =
        reload_native_arena(decode, ctx.ir, "design_edge_operands")?;
    expected_edge_operands_storage.with_storage(|| {
        history::bind_edge_operand_history_candidates(
            decode,
            &mut expected_edge_operands,
            &native.design_parameter_scopes,
            &native.construction_recipes,
            &native.asm_histories,
            &ctx.scope_histories,
        )
    })?;
    let expected_edge_operands = scratch_storage.with_storage(|| {
        decode.collect_hash_map(
            expected_edge_operands
                .iter()
                .map(|operand| (operand.id.as_str(), operand)),
            "index F3D expected edge operands",
        )
    })?;
    for operand in ctx.decode.admit_iter(
        &native.design_edge_operands,
        "scan F3D design edge operands",
    )? {
        let native_stream = design_stream(&operand.id);
        let scope = ctx.decode.get_hash_map(
            scopes_by_index,
            &(native_stream, operand.scope_record_index),
            "find F3D validation record index",
        )?;
        let header = ctx.decode.get_hash_map(
            records_by_index,
            &(native_stream, operand.record_index()),
            "find F3D validation record index",
        )?;
        let recipe = ctx.decode.get_hash_map(
            recipes_by_id,
            operand.recipe_id.as_str(),
            "find F3D validation record index",
        )?;
        let design_reference = recipe
            .and_then(|recipe| recipe.record_index)
            .map(|record_index| i64::from(record_index.value))
            .filter(|value| *value >= 0);
        let _expected_faces_storage;
        let expected_faces;
        (expected_faces, _expected_faces_storage) = ctx.decode.with_scoped_storage(
            "F3D validation expected edge operand candidate faces",
            || match design_reference {
                Some(design_reference) => design::decode::operands::edge_operand_candidate_faces(
                    ctx.decode,
                    design_reference,
                    &native.persistent_subentity_tags,
                    Some(&operand.id),
                ),
                None => Ok(Vec::new()),
            },
        )?;
        let (mut expected_references, mut expected_references_storage) = ctx
            .decode
            .with_scoped_storage("hold F3D validation recipe references", || {
                design::decode::dimension_frames::decode_recipe_references_charged(
                    ctx.decode,
                    &operand.recipe_prefix_bytes,
                    operand.recipe_prefix_offset(),
                )
            })?;
        if !historical_candidates_retained {
            for reference in ctx.decode.admit_iter(
                &mut expected_references,
                "scan F3D expected recipe references",
            )? {
                expected_references_storage.with_storage(|| {
                    design::decode::dimension_frames::bind_recipe_reference_candidates_charged(
                        ctx.decode,
                        reference,
                        &native.persistent_subentity_tags,
                        Some(&operand.id),
                    )
                })?;
            }
        }
        let (
            expected_surface_patch_recipe_structure,
            _expected_surface_patch_recipe_structure_storage,
        ) = ctx
            .decode
            .with_scoped_storage("hold F3D expected surface patch recipe", || {
                Ok::<_, CodecError>(match scope {
                    Some(scope)
                        if (scope.kind()
                            == crate::records::feature::scope::DesignFeatureKind::SurfacePatch) =>
                    {
                        design::decode::operands::surface_patch_recipe_structure_with_context(
                            ctx.decode,
                            &operand.recipe_program,
                            operand.recipe_references.len(),
                        )?
                    }
                    _ => None,
                })
            })?;
        let groups = ctx
            .decode
            .get_hash_map(
                &ctx.operand_groups_by_scope,
                &(native_stream, operand.scope_record_index),
                "find F3D terminal edge scope groups",
            )?
            .map_or(&[][..], Vec::as_slice);
        let terminal_group_member = ctx.decode.any_by(
            groups,
            |group| {
                Ok(group.members().last().map(|member| &member.value)
                    == Some(&operand.record_index()))
            },
            "find F3D terminal edge operand group",
        )?;
        let valid = match scope {
            Some(scope) => {
                design::decode::operands::has_edge_recipe_operands(&scope.kind())
                    && match usize::try_from(operand.scope_reference_ordinal) {
                        Ok(ordinal) => {
                            reference_member_at(scope.reference_members(), ordinal)
                                == Some(&operand.record_index())
                        }
                        Err(_) => false,
                    }
            }
            None => false,
        } && match header {
            Some(header) => {
                header.byte_offset == operand.byte_offset()
                    && (header.class_tag == operand.class_tag)
            }
            None => false,
        } && (operand
            .record_index()
            .checked_add(scope.map_or(4, |scope| {
                design::decode::operands::edge_recipe_terminal_delta(&scope.kind())
            }))
            .is_some_and(|expected_offset| operand.next_record_index == expected_offset)
            || terminal_group_member)
            && operand
                .recipe_prefix_offset()
                .checked_add(u64_from_index(operand.recipe_prefix_bytes.len()))
                .zip(recipe.and_then(|recipe| recipe.byte_offset.checked_sub(4)))
                .is_some_and(|(prefix_end, recipe_prefix_end)| prefix_end == recipe_prefix_end)
            && recipe_reference_frames_match(
                decode,
                &operand.recipe_references,
                &expected_references,
                historical_candidates_retained,
            )?
            && match recipe {
                Some(recipe) => {
                    ctx.decode.equal(
                        design_stream(&recipe.id),
                        native_stream,
                        "compare F3D edge recipe streams",
                    )? && recipe.kind == records::recipes::ConstructionRecipeKind::Edge
                        && recipe.byte_offset > operand.recipe_record_byte_offset()
                        && recipe.byte_offset < operand.next_byte_offset()
                }
                None => false,
            }
            && {
                let (expected_structure, _expected_structure_storage) = ctx
                    .decode
                    .with_scoped_storage("hold F3D expected recipe structure", || {
                        design::decode::operands::edge_recipe_structure_with_context(
                            ctx.decode,
                            &operand.recipe_program,
                        )
                    })?;
                ctx.decode.equal(
                    &expected_structure,
                    &operand.recipe_structure,
                    "compare F3D edge recipe structures",
                )?
            }
            && ctx.decode.equal(
                &expected_surface_patch_recipe_structure,
                &operand.surface_patch_recipe_structure,
                "compare F3D SurfacePatch recipe structures",
            )?
            && (historical_candidates_retained
                || ctx.decode.equal(
                    &expected_faces,
                    &operand.candidate_faces,
                    "compare F3D edge operand candidate faces",
                )?)
            && match ctx.decode.get_hash_map(
                &expected_edge_operands,
                operand.id.as_str(),
                "find F3D expected operand",
            )? {
                Some(expected) => {
                    ctx.decode
                        .equal(*expected, operand, "compare F3D expected edge operand")?
                }
                None => false,
            };
        let valid = valid
            && scratch_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut edge_operand_slots,
                    (
                        native_stream,
                        operand.scope_record_index,
                        operand.scope_reference_ordinal,
                    ),
                    "index F3D edge operand slots",
                )
            })?
            && set_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut edge_operand_records,
                    (native_stream, operand.record_index()),
                    "index F3D edge operand records",
                )
            })?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design edge operand has an invalid scope or recipe frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&operand.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok((edge_operand_records, set_storage))
}

fn validate_edge_treatment_vertex_operands<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<ScopedRecordSet<'a, (&'a str, u32)>, CodecError> {
    let decode = ctx.decode;
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D edge treatment vertex operands scratch")?;
    let mut set_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D edge treatment vertex operands result")?;
    let native = ctx.native;
    let (mut expected_storage, mut expected) =
        reload_native_arena::<records::feature::work_geometry::DesignEdgeTreatmentVertexOperand>(
            decode,
            ctx.ir,
            "design_edge_treatment_vertex_operands",
        )?;
    for operand in ctx
        .decode
        .admit_iter(&mut expected, "scan F3D expected vertex operands")?
    {
        for reference in ctx.decode.admit_iter(
            &mut operand.recipe.recipe_references,
            "scan F3D expected vertex recipe references",
        )? {
            expected_storage.with_storage(|| {
                design::decode::dimension_frames::bind_recipe_reference_candidates_charged(
                    ctx.decode,
                    reference,
                    &native.persistent_subentity_tags,
                    Some(&operand.id),
                )
            })?;
        }
    }
    expected_storage.with_storage(|| {
        history::bind_edge_treatment_vertex_history(
            decode,
            &mut expected,
            &native.design_parameter_scopes,
            &native.asm_histories,
            &ctx.scope_histories,
        )
    })?;
    let expected = scratch_storage.with_storage(|| {
        decode.collect_hash_map(
            expected
                .iter()
                .map(|operand| (operand.id.as_str(), operand)),
            "index F3D expected edge treatment vertex operands",
        )
    })?;
    let mut records = HashSet::new();
    for operand in ctx.decode.admit_iter(
        &native.design_edge_treatment_vertex_operands,
        "scan F3D design edge treatment vertex operands",
    )? {
        let stream = design_stream(&operand.id);
        let scope = ctx.decode.get_hash_map(
            &ctx.scopes_by_index,
            &(stream, operand.scope_record_index),
            "find F3D validation linked record",
        )?;
        let groups = ctx.operand_group_records(
            stream,
            operand.group_record_index,
            "find F3D edge-treatment vertex group records",
        )?;
        let group_position = ctx.decode.position_by(
            groups,
            |group| Ok(group.scope_record_index == operand.scope_record_index),
            "find F3D edge-treatment vertex groups",
        )?;
        let group = group_position.map(|position| groups[position]);
        let (expected_id, expected_id_storage) = ctx.decode.with_scoped_storage(
            "hold F3D expected edge-treatment vertex identity",
            || {
                crate::ids::native_scoped_id(
                    decode,
                    stream,
                    "edge-treatment-vertex-operand",
                    operand.recipe.byte_offset(),
                )
            },
        )?;
        let id_valid = ctx.decode.equal(
            &operand.id,
            &expected_id,
            "compare F3D edge-treatment vertex operand ids",
        )?;
        drop((expected_id, expected_id_storage));
        let scope_valid = if id_valid {
            match scope {
                Some(scope) => {
                    design::decode::operands::has_edge_recipe_operands(&scope.kind())
                        && match usize::try_from(operand.scope_reference_ordinal) {
                            Ok(ordinal) => {
                                reference_member_at(scope.reference_members(), ordinal)
                                    == Some(&operand.recipe.record_index())
                            }
                            Err(_) => false,
                        }
                }
                None => false,
            }
        } else {
            false
        };
        let group_member_valid = if id_valid && scope_valid {
            match group {
                Some(group) => {
                    usize::try_from(operand.group_member_ordinal)
                        .ok()
                        .and_then(|ordinal| {
                            group.members().get(ordinal).map(|member| &member.value)
                        })
                        == Some(&operand.recipe.record_index())
                }
                None => false,
            }
        } else {
            false
        };
        let unique_group = if id_valid && scope_valid && group_member_valid {
            match group_position {
                Some(position) => !ctx.decode.any_by(
                    &groups[position + 1..],
                    |group| Ok(group.scope_record_index == operand.scope_record_index),
                    "find F3D edge-treatment vertex groups",
                )?,
                None => false,
            }
        } else {
            false
        };
        let valid = id_valid
            && scope_valid
            && group_member_valid
            && unique_group
            && match ctx.decode.get_hash_map(
                &expected,
                operand.id.as_str(),
                "find F3D expected operand",
            )? {
                Some(expected) => ctx.decode.equal(
                    *expected,
                    operand,
                    "compare F3D expected edge-treatment vertex operand",
                )?,
                None => false,
            };
        let valid = valid
            && set_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut records,
                    (stream, operand.recipe.record_index()),
                    "index F3D edge treatment vertex records",
                )
            })?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion edge-treatment vertex operand has an invalid group or recipe frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&operand.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok((records, set_storage))
}

/// Report Fillet/Chamfer edge groups with incomplete selection operands.
fn validate_edge_treatment_groups<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    edge_operand_records: &HashSet<(&'a str, u32)>,
    edge_identity_records: &HashSet<(&'a str, u32)>,
    edge_treatment_vertex_records: &HashSet<(&'a str, u32)>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    for scope in ctx
        .decode
        .admit_iter(
            &native.design_parameter_scopes,
            "scan F3D feature parameter scopes",
        )?
        .filter(|scope| {
            matches!(
                scope.kind(),
                crate::records::feature::scope::DesignFeatureKind::Fillet
                    | crate::records::feature::scope::DesignFeatureKind::Chamfer
            )
        })
    {
        let native_stream = design_stream(&scope.id);
        let groups = ctx
            .decode
            .get_hash_map(
                &ctx.operand_groups_by_scope,
                &(native_stream, scope.record_index),
                "find F3D edge-treatment scope groups",
            )?
            .map_or(&[][..], Vec::as_slice);
        let complete = !groups.is_empty()
            && ctx.decode.all_by(
                groups,
                |group| {
                    let recipe_backed = ctx.decode.all_by(
                        group.members(),
                        |member| {
                            Ok(ctx.decode.contains_hash_set(
                                edge_operand_records,
                                &(native_stream, member.value),
                                "find F3D edge-treatment recipe member",
                            )? || ctx.decode.contains_hash_set(
                                edge_treatment_vertex_records,
                                &(native_stream, member.value),
                                "find F3D edge-treatment vertex member",
                            )?)
                        },
                        "validate F3D edge-treatment recipe members",
                    )?;
                    let identity_backed = ctx.decode.all_by(
                        group.members(),
                        |member| {
                            ctx.decode.contains_hash_set(
                                edge_identity_records,
                                &(native_stream, member.value),
                                "find F3D edge-treatment identity member",
                            )
                        },
                        "validate F3D edge-treatment identity members",
                    )?;
                    Ok(recipe_backed || identity_backed)
                },
                "scan F3D edge-treatment groups",
            )?;
        if !complete {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design edge-treatment group has incomplete selection operands",
                Some(
                    ctx.decode
                        .copy_retained_text(&scope.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate face operands and their recipe frames; returns their record set.
fn validate_face_operands<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    expected_face_operands: &[records::topology::face::DesignFaceOperand],
) -> Result<ScopedRecordSet<'a, (&'a str, u32, u32)>, CodecError> {
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D face operands scratch")?;
    let mut set_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D face operands result")?;
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let recipes_by_id = &ctx.recipes_by_id;
    let scopes_by_index = &ctx.scopes_by_index;
    let historical_candidates_retained =
        history::projection_was_finalized(ctx.decode, &native.asm_histories)?;
    let face_groups_by_index = &ctx.operand_groups_by_index;
    let expected_face_operands = scratch_storage.with_storage(|| {
        ctx.decode.collect_hash_map(
            expected_face_operands
                .iter()
                .map(|operand| (operand.id.as_str(), operand)),
            "index F3D expected face operands",
        )
    })?;
    let mut face_operand_records = HashSet::new();
    for operand in ctx.decode.admit_iter(
        &native.design_face_operands,
        "scan F3D design face operands",
    )? {
        let native_stream = design_stream(&operand.id);
        let scope = ctx.decode.get_hash_map(
            scopes_by_index,
            &(native_stream, operand.scope_record_index),
            "find F3D validation record index",
        )?;
        let header = ctx.decode.get_hash_map(
            records_by_index,
            &(native_stream, operand.record_index()),
            "find F3D validation record index",
        )?;
        let recipe = ctx.decode.get_hash_map(
            recipes_by_id,
            operand.recipe_id.as_str(),
            "find F3D validation record index",
        )?;
        let mut expected_faces_storage = ctx
            .decode
            .reserve_scoped(0, "collect F3D expected operand faces")?;
        let mut expected_faces = Vec::new();
        if let Some(design_reference) = recipe
            .and_then(|recipe| recipe.record_index)
            .map(|record_index| i64::from(record_index.value))
            .filter(|value| *value >= 0)
        {
            for tag in ctx.decode.admit_iter(
                &native.persistent_subentity_tags,
                "scan F3D expected operand face tags",
            )? {
                if !crate::ids::same_native_occurrence(ctx.decode, &tag.id, &operand.id)?
                    || !ctx.decode.contains(
                        &tag.design_references,
                        &design_reference,
                        "find F3D expected operand face reference",
                    )?
                {
                    continue;
                }
                if let cadmpeg_ir::attributes::AttributeTarget::Face(id) = &tag.target {
                    ctx.decode.push_scoped_vec(
                        &mut expected_faces_storage,
                        &mut expected_faces,
                        id,
                        "collect F3D expected operand faces",
                    )?;
                }
            }
        }
        ctx.decode.stable_sort_by(
            &mut expected_faces,
            |value| value.as_str(),
            Ord::cmp,
            "f3d face operand expected faces sort",
        )?;
        ctx.decode.dedup_vec(
            &mut expected_faces,
            "deduplicate F3D expected operand faces",
        )?;
        let (mut expected_references, mut expected_references_storage) = ctx
            .decode
            .with_scoped_storage("hold F3D validation recipe references", || {
                design::decode::dimension_frames::decode_recipe_references_charged(
                    ctx.decode,
                    &operand.recipe_prefix_bytes,
                    operand.recipe_prefix_offset(),
                )
            })?;
        if !historical_candidates_retained {
            for reference in ctx.decode.admit_iter(
                &mut expected_references,
                "scan F3D expected recipe references",
            )? {
                expected_references_storage.with_storage(|| {
                    design::decode::dimension_frames::bind_recipe_reference_candidates_charged(
                        ctx.decode,
                        reference,
                        &native.persistent_subentity_tags,
                        Some(&operand.id),
                    )
                })?;
            }
        }
        let recipe_design_reference = recipe
            .and_then(|recipe| recipe.record_index)
            .map(|record_index| i64::from(record_index.value))
            .filter(|value| *value >= 0);
        let mut referenced_faces_storage = ctx
            .decode
            .reserve_scoped(0, "index F3D referenced operand faces")?;
        let mut referenced_faces = HashSet::new();
        for reference in ctx
            .decode
            .admit_iter(&expected_references, "scan F3D referenced face operands")?
        {
            if Some(reference.design_reference) != recipe_design_reference {
                continue;
            }
            for face in ctx.decode.admit_iter(
                &reference.candidate_faces,
                "scan F3D referenced operand faces",
            )? {
                referenced_faces_storage.with_storage(|| {
                    ctx.decode.insert_hash_set(
                        &mut referenced_faces,
                        face,
                        "index F3D referenced operand faces",
                    )
                })?;
            }
        }
        let mut expected_unreferenced_faces_storage = ctx
            .decode
            .reserve_scoped(0, "collect F3D unreferenced operand faces")?;
        let mut expected_unreferenced_faces = Vec::new();
        for face in ctx
            .decode
            .admit_iter(&expected_faces, "scan F3D unreferenced operand faces")?
        {
            if !ctx.decode.contains_hash_set(
                &referenced_faces,
                face,
                "find F3D referenced operand face",
            )? {
                ctx.decode.push_scoped_vec(
                    &mut expected_unreferenced_faces_storage,
                    &mut expected_unreferenced_faces,
                    *face,
                    "collect F3D unreferenced operand faces",
                )?;
            }
        }
        let mut expected_alternate_selector_faces_storage = ctx
            .decode
            .reserve_scoped(0, "collect F3D alternate selector operand faces")?;
        let mut expected_alternate_selector_faces = Vec::new();
        for reference in ctx.decode.admit_iter(
            &expected_references,
            "scan F3D alternate selector face operands",
        )? {
            if Some(reference.design_reference) != recipe_design_reference {
                continue;
            }
            for face in ctx.decode.admit_iter(
                &reference.alternate_selector_faces,
                "scan F3D alternate selector operand faces",
            )? {
                ctx.decode.push_scoped_vec(
                    &mut expected_alternate_selector_faces_storage,
                    &mut expected_alternate_selector_faces,
                    face,
                    "collect F3D alternate selector operand faces",
                )?;
            }
        }
        ctx.decode.stable_sort_by(
            &mut expected_alternate_selector_faces,
            |value| value.as_str(),
            Ord::cmp,
            "f3d face operand alternate selector faces sort",
        )?;
        ctx.decode.dedup_vec(
            &mut expected_alternate_selector_faces,
            "deduplicate F3D alternate selector operand faces",
        )?;
        let Some(recipe_window) = std::num::NonZeroUsize::new(3) else {
            return Err(CodecError::Malformed(
                "F3D face recipe window is empty".into(),
            ));
        };
        let mut expected_node_offsets_storage = ctx
            .decode
            .reserve_scoped(0, "collect F3D face recipe node offsets")?;
        let mut expected_node_offsets = Vec::new();
        for (index, values) in ctx
            .decode
            .admit_iter(&operand.recipe_program, "scan F3D face recipe node windows")?
            .windows(recipe_window)
            .enumerate()
        {
            if values != [-1, -1, 2].as_slice() {
                continue;
            }
            let offset = u64_from_index(index)
                .checked_mul(4)
                .and_then(|delta| operand.recipe_program_offset.checked_add(delta))
                .ok_or_else(|| {
                    CodecError::Malformed("F3D face recipe node offset overflows".into())
                })?;
            ctx.decode.push_scoped_vec(
                &mut expected_node_offsets_storage,
                &mut expected_node_offsets,
                offset,
                "collect F3D face recipe node offsets",
            )?;
        }
        let (expected_nodes, _expected_nodes_storage) =
            ctx.decode
                .with_scoped_storage("hold F3D expected face recipe nodes", || {
                    ctx.decode.collect_vec(
                        expected_node_offsets.iter().copied().zip(
                            expected_node_offsets
                                .iter()
                                .copied()
                                .skip(1)
                                .chain(std::iter::once(operand.next_byte_offset())),
                        ),
                        "collect F3D face recipe nodes",
                    )
                })?;
        let valid_program =
            match design::decode::operands::face_recipe_program_kind(&operand.recipe_program) {
                Some(design::decode::operands::FaceRecipeProgramKind::Terminal) => {
                    operand.recipe_nodes.is_empty()
                }
                Some(design::decode::operands::FaceRecipeProgramKind::Counted { .. }) => {
                    operand.recipe_nodes.len() == expected_nodes.len()
                        && ctx.decode.all_by(
                            operand.recipe_nodes.iter().zip(expected_nodes.iter()),
                            |(node, (start, end))| -> Result<bool, CodecError> {
                                if node.byte_offset != *start
                                    || node.end_byte_offset != *end
                                    || node.program.get(0..3) != Some(&[-1, -1, 2])
                                {
                                    return Ok(false);
                                }
                                let (expected_structure, _expected_structure_storage) =
                                ctx.decode.with_scoped_storage(
                                    "hold F3D expected face recipe structure",
                                    || {
                                        Ok::<_, CodecError>(node.program.get(3..).map(|program| {
        design::decode::operands::face_recipe_structure_with_context(ctx.decode, program)
    }).transpose()?.flatten())
                                    },
                                )?;
                                Ok(ctx.decode.equal(
                                    &node.recipe_structure,
                                    &expected_structure,
                                    "compare F3D face recipe structures",
                                )? && u64_from_index(node.program.len())
                                    .checked_mul(4)
                                    .and_then(|delta| (*start).checked_add(delta))
                                    .is_some_and(|expected_offset| expected_offset == *end))
                            },
                            "scan F3D face recipe nodes",
                        )?
                        && match operand.recipe_nodes.first() {
                            None => true,
                            Some(first_node) => match first_node
                                .byte_offset
                                .checked_sub(operand.recipe_program_offset)
                                .and_then(|byte_offset| usize::try_from(byte_offset / 4).ok())
                            {
                                None => false,
                                Some(first_node_index) => {
                                    if first_node_index > operand.recipe_program.len() {
                                        false
                                    } else {
                                        let mut program_offset = first_node_index;
                                        let programs_match = ctx.decode.all_by(
                                            operand.recipe_nodes.iter(),
                                            |node| {
                                                let Some(end_offset) =
                                                    program_offset.checked_add(node.program.len())
                                                else {
                                                    return Err(ctx.decode.refuse_codec_limit(
                                                        "compare F3D face recipe node programs",
                                                        u64::MAX,
                                                        u64::MAX,
                                                    ));
                                                };
                                                let Some(expected_program) = operand
                                                    .recipe_program
                                                    .get(program_offset..end_offset)
                                                else {
                                                    return Ok(false);
                                                };
                                                if !ctx.decode.equal(
                                                    node.program.as_slice(),
                                                    expected_program,
                                                    "compare F3D face recipe node programs",
                                                )? {
                                                    return Ok(false);
                                                }
                                                program_offset = end_offset;

                                                Ok(true)
                                            },
                                            "scan F3D face recipe node programs",
                                        )?;
                                        programs_match
                                            && program_offset == operand.recipe_program.len()
                                    }
                                }
                            },
                        }
                }
                None => false,
            };
        let expected_history = ctx.decode.get_hash_map(
            &expected_face_operands,
            operand.id.as_str(),
            "find F3D validation record index",
        )?;
        let valid = (match scope {
            None => false,
            Some(scope) => {
                let family = design::design_feature_family(&scope.kind());
                match (operand.group_record_index(), operand.group_member_ordinal()) {
                    (Some(group_record_index), Some(group_member_ordinal)) => {
                        let group = ctx
                            .decode
                            .get_hash_map(
                                face_groups_by_index,
                                &(native_stream, group_record_index),
                                "find F3D face operand group",
                            )?
                            .copied();
                        let exact_group_member = match group {
                            Some(group) => {
                                group.scope_record_index == operand.scope_record_index
                                    && match usize::try_from(operand.scope_reference_ordinal) {
                                        Ok(ordinal) => {
                                            reference_member_at(scope.reference_members(), ordinal)
                                                == Some(&group_record_index)
                                        }
                                        Err(_) => false,
                                    }
                                    && usize::try_from(group_member_ordinal).ok().and_then(
                                        |ordinal| {
                                            group.members().get(ordinal).map(|member| &member.value)
                                        },
                                    ) == Some(&operand.record_index())
                            }
                            None => false,
                        };
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
                                let thread_group_valid = match group {
                                    Some(group)
                                        if group.role() == DesignOperandRole::ROLE_0X10 =>
                                    {
                                        match scope.thread_construction() {
                                            Some(construction) => ctx.decode.contains(
                                                &construction.face_group_record_indices,
                                                &group.record_index,
                                                "find F3D thread face operand group",
                                            )?,
                                            None => false,
                                        }
                                    }
                                    _ => false,
                                };
                                thread_group_valid && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            Some(
                                design::DesignFeatureFamily::Fillet
                                | design::DesignFeatureFamily::Chamfer,
                            ) => {
                                operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                                    && ctx.decode.any_by(
                                        &native.design_edge_identity_operands,
                                        |identity| {
                                            Ok(ctx.decode.equal(
                                                design_stream(&identity.id),
                                                native_stream,
                                                "compare F3D face identity streams",
                                            )? && identity.scope_record_index
                                                == operand.scope_record_index
                                                && identity.group_record_index == group_record_index
                                                && identity.group_member_ordinal == group_member_ordinal
                                                && identity.record_index() == operand.record_index()
                                                && (identity.class_tag == operand.class_tag))
                                        },
                                        "find F3D face edge identity",
                                    )?
                            }
                            None if (scope.kind() == crate::records::feature::scope::DesignFeatureKind::SplitFace) =>
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
                        let direct_member = match usize::try_from(operand.scope_reference_ordinal) {
                            Ok(ordinal) => {
                                reference_member_at(scope.reference_members(), ordinal)
                                    == Some(&operand.record_index())
                            }
                            Err(_) => false,
                        };
                        direct_member && match family {
                            Some(
                                design::DesignFeatureFamily::OffsetFaces
                                | design::DesignFeatureFamily::Shell
                                | design::DesignFeatureFamily::Thicken,
                            ) => true,
                            Some(design::DesignFeatureFamily::Split) => {
                                operand.scope_reference_ordinal == 1
                            }
                            Some(design::DesignFeatureFamily::Hole) => {
                                operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            Some(design::DesignFeatureFamily::Assemble)
                                if (scope.kind()
                                    == crate::records::feature::scope::DesignFeatureKind::AsBuilt)
                                    && design::assembly::legacy_as_built_421_generation(
                                        scope.frame_length(),
                                        scope.class_tag.as_str(),
                                        scope.paired_class_tag.as_str(),
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
            }
        }) && match header {
            Some(header) => {
                header.byte_offset == operand.byte_offset()
                    && (header.class_tag == operand.class_tag)
            }
            None => false,
        } && operand
            .recipe_prefix_offset()
            .checked_add(u64_from_index(operand.recipe_prefix_bytes.len()))
            .zip(recipe.and_then(|recipe| recipe.byte_offset.checked_sub(4)))
            .is_some_and(|(prefix_end, recipe_prefix_end)| prefix_end == recipe_prefix_end)
            && recipe_reference_frames_match(
                ctx.decode,
                &operand.recipe_references,
                &expected_references,
                historical_candidates_retained,
            )?
            && valid_program
            && recipe_program_operand_length(operand.recipe_kind).is_some_and(|operand_length| {
                recipe.is_some_and(|recipe| {
                    recipe
                        .byte_offset
                        .checked_add(operand_length)
                        .is_some_and(|expected_offset| {
                            operand.recipe_program_offset == expected_offset
                        })
                })
            })
            && u64_from_index(operand.recipe_program.len())
                .checked_mul(4)
                .and_then(|delta| operand.recipe_program_offset.checked_add(delta))
                .is_some_and(|expected_offset| operand.next_byte_offset() == expected_offset)
            && (historical_candidates_retained
                || face_ids_match_refs(ctx.decode, &operand.candidate_faces, &expected_faces)?)
            && (historical_candidates_retained
                || face_ids_match_refs(
                    ctx.decode,
                    &operand.unreferenced_candidate_faces,
                    &expected_unreferenced_faces,
                )?)
            && (historical_candidates_retained
                || face_ids_match_refs(
                    ctx.decode,
                    &operand.alternate_selector_candidate_faces,
                    &expected_alternate_selector_faces,
                )?)
            && match expected_history {
                Some(expected) => {
                    ctx.decode.equal(
                        &operand.preceding_candidate_faces,
                        &expected.preceding_candidate_faces,
                        "compare F3D preceding face candidates",
                    )? && ctx.decode.equal(
                        &operand.changed_candidate_faces,
                        &expected.changed_candidate_faces,
                        "compare F3D changed face candidates",
                    )? && ctx.decode.equal(
                        &operand.historical_support_contexts,
                        &expected.historical_support_contexts,
                        "compare F3D historical face support contexts",
                    )?
                }
                None => false,
            }
            && match recipe {
                Some(recipe) => {
                    ctx.decode.equal(
                        design_stream(&recipe.id),
                        native_stream,
                        "compare F3D face recipe streams",
                    )? && recipe.kind == operand.recipe_kind
                        && recipe.byte_offset > operand.recipe_record_byte_offset()
                        && recipe.byte_offset < operand.next_byte_offset()
                }
                None => false,
            };
        let valid = valid
            && set_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut face_operand_records,
                    (
                        native_stream,
                        operand.scope_record_index,
                        operand.record_index(),
                    ),
                    "index F3D face operand records",
                )
            })?;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design face operand has an invalid scope or recipe frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&operand.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok((face_operand_records, set_storage))
}

fn face_ids_match_refs(
    decode: &DecodeContext<'_>,
    actual: &[cadmpeg_ir::ids::FaceId],
    expected: &[&cadmpeg_ir::ids::FaceId],
) -> Result<bool, CodecError> {
    if actual.len() != expected.len() {
        return Ok(false);
    }
    decode.all_by(
        actual.iter().zip(expected),
        |(actual, expected)| decode.equal(actual, *expected, "compare F3D face identity"),
        "scan F3D actual face identities",
    )
}

/// Report face-group members with no resolved recipe operand.
fn validate_face_group_member_resolution(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
    face_group_members: &std::collections::BTreeSet<(&str, u32, u32)>,
    face_operand_records: &HashSet<(&str, u32, u32)>,
    entity_selection_operands: &[records::topology::entity_selection::DesignEntitySelectionOperand],
) -> Result<(), CodecError> {
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D face group member resolution scratch")?;
    let entity_selection_records = scratch_storage.with_storage(|| {
        ctx.decode.collect_hash_set(
            entity_selection_operands.iter().map(|operand| {
                (
                    design_stream(&operand.id),
                    operand.scope_record_index,
                    operand.record_index(),
                )
            }),
            "index F3D face group entity selections",
        )
    })?;
    for member in ctx
        .decode
        .admit_iter(face_group_members, "scan F3D face group members")?
        .copied()
    {
        if !ctx.decode.contains_hash_set(
            face_operand_records,
            &member,
            "find F3D face group operand",
        )? && !ctx.decode.contains_hash_set(
            &entity_selection_records,
            &member,
            "find F3D face group entity selection",
        )? {
            let entity = {
                let decode = ctx.decode;
                decode.format_retained(
                    format_args!(
                        "{}:design-face-group-member#{}:{}",
                        member.0, member.1, member.2
                    ),
                    "retain F3D face group member identity",
                )?
            };
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Extrude face group has an unresolved recipe operand",
                Some(entity),
            )?;
        }
    }
    Ok(())
}

/// Validate retained Face source carriers and their persistent identities.
fn validate_face_source_groups(ctx: &Ctx, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D face source groups scratch")?;
    let native = ctx.native;
    let mut carrier_records = HashSet::new();
    for group in ctx.decode.admit_iter(
        &native.design_face_source_groups,
        "scan F3D design face source groups",
    )? {
        let native_stream = design_stream(&group.id);
        let scope = ctx.decode.get_hash_map(
            &ctx.scopes_by_index,
            &(native_stream, group.scope_record_index),
            "find F3D validation linked record",
        )?;
        let carrier_header = ctx.decode.get_hash_map(
            &ctx.records_by_index,
            &(native_stream, group.carrier_record_index),
            "find F3D validation linked record",
        )?;
        let paired_header = ctx.decode.get_hash_map(
            &ctx.records_by_index,
            &(native_stream, group.paired_record_index),
            "find F3D validation linked record",
        )?;
        let carrier_ordinal = usize::try_from(group.carrier_reference_ordinal).ok();
        let source_spec = design::decode::operands::face_source_carrier_spec(
            group.carrier_class_tag.as_str(),
            group.paired_class_tag.as_str(),
        );
        let scope_links_valid = match scope {
            Some(scope)
                if (scope.kind() == crate::records::feature::scope::DesignFeatureKind::Face) =>
            {
                match carrier_ordinal {
                    Some(ordinal) => {
                        reference_member_at(scope.reference_members(), ordinal)
                            == Some(&group.carrier_record_index)
                            && match ordinal.checked_add(1) {
                                Some(ordinal) => {
                                    reference_member_at(scope.reference_members(), ordinal)
                                        == Some(&group.paired_record_index)
                                }
                                None => false,
                            }
                    }
                    None => false,
                }
            }
            _ => false,
        };
        let headers_valid = match carrier_header {
            Some(header) => {
                header.byte_offset == group.carrier_span.start()
                    && (header.class_tag == group.carrier_class_tag)
            }
            None => false,
        } && match paired_header {
            Some(header) => {
                header.byte_offset == group.carrier_span.end()
                    && (header.class_tag == group.paired_class_tag)
            }
            None => false,
        };
        let source_offsets_valid = match source_spec {
            Some(layout) => match u64::try_from(layout.source_reference_offset) {
                Err(_) => false,
                Ok(source_reference_offset) => ctx.decode.all_by(
                    group.source_members.iter().enumerate(),
                    |(ordinal, member)| {
                        let Ok(ordinal) = u64::try_from(ordinal) else {
                            return Ok(false);
                        };
                        if group
                            .carrier_span
                            .start()
                            .checked_add(source_reference_offset)
                            .and_then(|offset| offset.checked_add(ordinal.checked_mul(11)?))
                            != Some(member.offset)
                        {
                            return Ok(false);
                        }

                        Ok(true)
                    },
                    "validate F3D face-source member offsets",
                )?,
            },
            None => false,
        };
        let mut source_records = HashSet::new();
        let mut source_records_storage = ctx
            .decode
            .reserve_scoped(0, "hold F3D Face source member records")?;
        let source_members_valid = if let Some(layout) = source_spec {
            if group.source_members.len() == layout.source_count {
                ctx.decode.all_by(
                    group.source_members.iter().map(|member| &member.value),
                    |member| {
                        let unique_record = source_records_storage.with_storage(|| {
                            ctx.decode.insert_hash_set(
                                &mut source_records,
                                member.record_index,
                                "index F3D Face source member records",
                            )
                        })?;
                        let persistent = &member.persistent_identity;
                        let local_id_offset = member.byte_offset.checked_add(21);
                        let asset_id_offset = member.byte_offset.checked_add(33);
                        let member_valid = unique_record
                            && member.byte_offset > group.carrier_span.start()
                            && local_id_offset == Some(persistent.local_id_offset())
                            && asset_id_offset == Some(persistent.asset_id_offset())
                            && persistent.context_id_offset() > persistent.asset_id_offset()
                            && persistent.tail_slot_offset() > persistent.context_id_offset()
                            && persistent.next_byte_offset() > member.byte_offset;
                        if !member_valid {
                            return Ok(false);
                        }

                        Ok(true)
                    },
                    "scan F3D face source members",
                )?
            } else {
                false
            }
        } else {
            false
        };
        let valid = scratch_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut carrier_records,
                (native_stream, group.carrier_record_index),
                "index F3D Face source carriers",
            )
        })? && scope_links_valid
            && headers_valid
            && source_offsets_valid
            && source_members_valid;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Face source carrier has invalid links or offsets",
                Some(
                    ctx.decode
                        .copy_retained_text(&group.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate sketch placement frames and their scope links.
fn validate_sketch_placements(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D sketch placements scratch")?;
    let native = ctx.native;
    let scopes_by_index = &ctx.scopes_by_index;
    let mut placement_records = HashSet::new();
    let mut placement_scopes = HashSet::new();
    let mut visibility_offsets = HashSet::new();
    let mut visibility_ordinals = std::collections::BTreeSet::new();
    for placement in ctx.decode.admit_iter(
        &native.design_sketch_placements,
        "scan F3D design sketch placements",
    )? {
        let native_stream = design_stream(&placement.id);
        let unique_record = scratch_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut placement_records,
                (native_stream, placement.record_index),
                "index F3D sketch placement records",
            )
        })?;
        let unique_scope = match placement.scope_record_index {
            Some(index) => scratch_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut placement_scopes,
                    (native_stream, index),
                    "index F3D sketch placement scopes",
                )
            })?,
            None => true,
        };
        let scope = match placement.scope_record_index {
            Some(index) => ctx.decode.get_hash_map(
                scopes_by_index,
                &(native_stream, index),
                "find F3D placement scope",
            )?,
            None => None,
        };
        let visibility_valid = if let Some(visibility) = placement.visibility.as_ref() {
            let header_valid = ctx
                .decode
                .get_hash_map(
                    &ctx.entities_by_suffix,
                    &(native_stream, placement.entity_id.suffix()),
                    "find F3D sketch visibility header",
                )?
                .is_some_and(|entity| visibility.stream_ordinal_offset() > entity.byte_offset);
            if header_valid
                && scratch_storage.with_storage(|| {
                    ctx.decode.insert_btree_set(
                        &mut visibility_ordinals,
                        (native_stream, visibility.stream_ordinal.get()),
                        "index F3D sketch visibility ordinals",
                    )
                })?
            {
                scratch_storage.with_storage(|| {
                    ctx.decode.insert_hash_set(
                        &mut visibility_offsets,
                        (native_stream, visibility.visible_offset()),
                        "index F3D sketch visibility offsets",
                    )
                })?
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
            match scope {
                Some(scope)
                    if design::design_feature_family(&scope.kind())
                        == Some(design::DesignFeatureFamily::Sketch) =>
                {
                    match scope.sketch_entity() {
                        Some(binding) => ctx.decode.equal(
                            &binding.entity_id,
                            &placement.entity_id,
                            "compare F3D sketch placement entity identities",
                        )?,
                        None => false,
                    }
                }
                _ => false,
            }
        };
        let valid = scope_valid && unique_record && unique_scope && visibility_valid;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design sketch placement has an invalid frame or scope link",
                Some(
                    ctx.decode
                        .copy_retained_text(&placement.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    let mut visibility_ranges_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D visibility ordinal ranges")?;
    let mut visibility_ordinal_ranges = std::collections::BTreeMap::<&str, (usize, u32)>::new();
    for (stream, ordinal) in ctx
        .decode
        .admit_iter(&visibility_ordinals, "scan F3D visibility ordinals")?
        .copied()
    {
        if !ctx.decode.contains_key_btree_map(
            &visibility_ordinal_ranges,
            stream,
            "index F3D visibility stream ordinals",
        )? {
            visibility_ranges_storage.with_storage(|| {
                ctx.decode.insert_btree_map(
                    &mut visibility_ordinal_ranges,
                    stream,
                    Default::default(),
                    "index F3D sketch visibility ordinal ranges",
                )
            })?;
        }
        let (count, maximum) = ctx
            .decode
            .get_mut_btree_map(
                &mut visibility_ordinal_ranges,
                stream,
                "index F3D visibility stream ordinals",
            )?
            .ok_or_else(|| CodecError::malformed("F3D validation default index missing"))?;
        *count = count.checked_add(1).ok_or_else(|| {
            ctx.decode.refuse_codec_limit(
                "count F3D visibility stream ordinals",
                u64::MAX - 1,
                u64::MAX,
            )
        })?;
        *maximum = (*maximum).max(ordinal);
    }
    for (stream, (count, maximum)) in ctx
        .decode
        .admit_iter(
            &visibility_ordinal_ranges,
            "scan F3D visibility ordinal ranges",
        )?
        .map(|(stream, range)| (*stream, *range))
    {
        if usize::try_from(maximum).ok() != Some(count) {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design sketch Geometry member ordinals are not contiguous",
                Some(
                    ctx.decode
                        .copy_retained_text(stream, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate parameter owner frames and their indexed parameter links.
fn validate_parameter_owners(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D parameter owners scratch")?;
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let parameters_by_index = &ctx.parameters_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let mut owner_indices = HashSet::new();
    let mut owner_local_ordinals = HashSet::new();
    for owner in ctx.decode.admit_iter(
        &native.design_parameter_owners,
        "scan F3D design parameter owners",
    )? {
        let native_stream = design_stream(owner.id());
        let unique_index = scratch_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut owner_indices,
                (native_stream, owner.record_index()),
                "index F3D parameter owners",
            )
        })?;
        let parameter = ctx.decode.get_hash_map(
            parameters_by_index,
            &(native_stream, owner.parameter_record_index()),
            "find F3D validation record index",
        )?;
        let legacy_68_frame = owner.frame_length() == 68;
        let frame_layout = !matches!(owner.frame_length(), 68 | 88)
            || parameter.is_some_and(|parameter| {
                owner.evaluated_value_offset() == parameter.evaluated_value_offset()
            });
        let scope_resolves = legacy_68_frame
            || ctx.decode.contains_key_hash_map(
                records_by_index,
                &(native_stream, owner.scope_record_index()),
                "find F3D parameter owner scope",
            )?;
        let unique_local_ordinal = if legacy_68_frame {
            true
        } else {
            scratch_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut owner_local_ordinals,
                    (
                        native_stream,
                        owner.scope_record_index(),
                        owner.local_ordinal(),
                    ),
                    "index F3D parameter owner local ordinals",
                )
            })?
        };
        let valid = frame_layout
            && scope_resolves
            && ctx.decode.contains_key_hash_map(
                records_by_index,
                &(native_stream, owner.parameter_record_index()),
                "find F3D parameter owner record",
            )?
            && ctx.decode.contains_key_hash_map(
                records_by_index,
                &(native_stream, owner.companion_record_index()),
                "find F3D parameter companion record",
            )?
            && ctx
                .decode
                .get_hash_map(
                    companions_by_index,
                    &(native_stream, owner.companion_record_index()),
                    "find F3D parameter companion owner",
                )?
                .is_some_and(|companion| companion.owner_record_index() == owner.record_index())
            && parameter.is_some_and(|parameter| {
                parameter.owner_record_index() == Some(owner.record_index())
                    && parameter.evaluated_value().get().to_bits()
                        == owner.evaluated_value().get().to_bits()
            })
            && unique_index
            && unique_local_ordinal;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design parameter owner has an invalid frame or indexed link",
                Some(
                    ctx.decode
                        .copy_retained_text(owner.id(), "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate parameter companion prefixes and owned recipe runs.
fn validate_parameter_companions(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D parameter companions scratch")?;
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let mut companion_indices = HashSet::new();
    let mut companion_owners = HashSet::new();
    for companion in ctx.decode.admit_iter(
        &native.design_parameter_companions,
        "scan F3D design parameter companions",
    )? {
        let native_stream = design_stream(companion.id());
        let payload = companion.payload();
        let payload_end =
            payload.and_then(|payload| payload.byte_offset().checked_add(payload.byte_length()));
        let mut expected_recipes_storage = ctx
            .decode
            .reserve_scoped(0, "collect F3D companion expected recipes")?;
        let mut expected_recipes = Vec::new();
        for recipe in ctx.decode.admit_iter(
            &native.construction_recipes,
            "scan F3D companion expected recipes",
        )? {
            if ctx.decode.equal(
                design_stream(&recipe.id),
                native_stream,
                "compare F3D companion recipe streams",
            )? && payload.is_some_and(|payload| {
                payload_end.is_some_and(|end| {
                    recipe.byte_offset >= payload.byte_offset() && recipe.byte_offset < end
                })
            }) {
                ctx.decode.push_scoped_vec(
                    &mut expected_recipes_storage,
                    &mut expected_recipes,
                    recipe,
                    "collect F3D companion expected recipes",
                )?;
            }
        }
        ctx.decode.stable_sort_by(
            &mut expected_recipes,
            |value| &value.byte_offset,
            Ord::cmp,
            "f3d parameter companion recipes sort",
        )?;
        let unique_index = scratch_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut companion_indices,
                (native_stream, companion.record_index()),
                "index F3D parameter companions",
            )
        })?;
        let unique_owner = scratch_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut companion_owners,
                (native_stream, companion.owner_record_index()),
                "index F3D companion owners",
            )
        })?;
        let owner = ctx.decode.get_hash_map(
            owners_by_index,
            &(native_stream, companion.owner_record_index()),
            "find F3D validation record index",
        )?;
        let valid = companion
            .byte_offset()
            .checked_add(42)
            .is_some_and(|expected_offset| companion.timestamp_micros_offset() == expected_offset)
            && payload.is_none_or(|payload| {
                companion
                    .byte_offset()
                    .checked_add(58)
                    .is_some_and(|expected_offset| payload.byte_offset() == expected_offset)
            })
            && (payload.is_none() || payload_end.is_some())
            && match payload {
                Some(payload) => {
                    payload.owned_recipe_ids().len() == expected_recipes.len()
                        && ctx.decode.all_by(
                            payload.owned_recipe_ids().iter().zip(&expected_recipes),
                            |(actual, expected)| {
                                ctx.decode.equal(
                                    actual.as_str(),
                                    expected.id.as_str(),
                                    "compare F3D parameter companion recipe ids",
                                )
                            },
                            "scan F3D parameter companion recipe ids",
                        )?
                }
                None => expected_recipes.is_empty(),
            }
            && ctx.decode.contains_key_hash_map(
                records_by_index,
                &(native_stream, companion.record_index()),
                "find F3D parameter companion record",
            )?
            && owner
                .is_some_and(|owner| owner.companion_record_index() == companion.record_index())
            && unique_index
            && unique_owner;
        if !valid {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design parameter companion has an invalid prefix or owner link",
                Some(
                    ctx.decode
                        .copy_retained_text(companion.id(), "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate parameter record identity uniqueness.
fn validate_parameters(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    let native = ctx.native;
    let mut parameter_indices_storage = ctx
        .decode
        .reserve_scoped(0, "index F3D validation parameters")?;
    let mut parameter_indices = HashSet::new();
    for parameter in ctx
        .decode
        .admit_iter(&native.design_parameters, "scan F3D design parameters")?
    {
        let native_stream = design_stream(&parameter.id);
        let key = (native_stream, parameter.record_index);
        if !parameter_indices_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut parameter_indices,
                key,
                "index F3D validation parameters",
            )
        })? {
            let id = {
                let decode = ctx.decode;
                decode.format_retained(
                    format_args!("{}", parameter.id),
                    "retain F3D parameter finding entity",
                )?
            };
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design parameter has an invalid frame, family discriminator, or owner",
                Some(id),
            )?;
        }
    }
    Ok(())
}

/// Validate design entity reference runs and suffix uniqueness.
fn validate_entity_headers(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let mut entity_suffixes_storage = ctx
        .decode
        .reserve_scoped(0, "index F3D design entity suffixes")?;
    let mut entity_suffixes = HashSet::new();
    for header in ctx.decode.admit_iter(
        &native.design_entity_headers,
        "scan F3D design entity headers",
    )? {
        let native_stream = design_stream(&header.id);
        let references_resolve = match header.sketch_references() {
            Some(references) => ctx.decode.all_by(
                &references.references,
                |reference| {
                    ctx.decode.contains_key_hash_map(
                        records_by_index,
                        &(native_stream, reference.value),
                        "find F3D entity reference target",
                    )
                },
                "validate F3D entity reference run",
            )?,
            None => true,
        };
        if !references_resolve {
            ctx.push_constant_finding(
                findings,
                Check::ReferentialIntegrity,
                "Fusion design entity has an invalid reference run",
                Some(ctx.decode.copy_retained_text(
                    header.entity_id.as_str(),
                    "retain F3D validation entity",
                )?),
            )?;
        }
        let key = (native_stream, header.entity_id.suffix());
        if !entity_suffixes_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut entity_suffixes,
                key,
                "index F3D design entity suffixes",
            )
        })? {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design entity suffix is duplicated within its stream",
                Some(ctx.decode.copy_retained_text(
                    header.entity_id.as_str(),
                    "retain F3D validation entity",
                )?),
            )?;
        }
    }
    Ok(())
}

/// Validate sketch relation owners and byte frames.
fn validate_sketch_relations(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let sketch_owner_ids = &ctx.sketch_owner_ids;
    for relation in ctx
        .decode
        .admit_iter(&native.sketch_relations, "scan F3D sketch relations")?
    {
        let native_stream = design_stream(&relation.id);
        let owner_matches = matches!(
            (
                ctx.decode.get_hash_map(sketch_owner_ids, &(native_stream, relation.owner_reference), "find F3D sketch relation owner")?,
                relation.owner_entity_id.as_ref().map(cadmpeg_core::text::NonBlankString::as_str),
            ),
            (Some(expected), Some(actual)) if ctx.decode.equal(*expected, actual, "compare F3D sketch relation owner identities")?
        );
        if !owner_matches {
            ctx.push_constant_finding(
                findings,
                Check::ReferentialIntegrity,
                "Fusion sketch relation has an invalid owner or byte frame",
                Some(
                    ctx.decode
                        .copy_retained_text(&relation.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

/// Validate sketch point, curve, and surface persistent identities.
fn validate_sketch_geometry_identities(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let mut sketch_point_storage = ctx
        .decode
        .reserve_scoped(0, "index F3D sketch point identities")?;
    let mut sketch_geometry_storage = ctx
        .decode
        .reserve_scoped(0, "index F3D sketch geometry records")?;
    let mut sketch_point_identities = HashSet::new();
    let mut sketch_geometry_records = HashSet::new();
    // An unresolved owner is not one shared sketch. Enforce uniqueness only
    // when the owning sketch reference is known.
    for point in ctx
        .decode
        .admit_iter(&native.sketch_points, "scan F3D sketch points")?
    {
        let duplicate = if let (Some(persistent_id), Some(owner_reference)) =
            (point.persistent_id(), point.owner_reference)
        {
            !sketch_point_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut sketch_point_identities,
                    (design_stream(&point.id), owner_reference, persistent_id),
                    "index F3D sketch point identities",
                )
            })?
        } else {
            false
        };
        if duplicate {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch point has an invalid persistent identity",
                Some(
                    ctx.decode
                        .copy_retained_text(&point.id, "retain F3D validation entity")?,
                ),
            )?;
        }
        if !sketch_geometry_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut sketch_geometry_records,
                (design_stream(&point.id), point.record_index),
                "index F3D sketch geometry records",
            )
        })? {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch geometry aliases another typed indexed record",
                Some(
                    ctx.decode
                        .copy_retained_text(&point.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    let mut sketch_curve_storage = ctx
        .decode
        .reserve_scoped(0, "index F3D sketch curve identities")?;
    let mut sketch_curve_identities = HashSet::new();
    for curve in ctx.decode.admit_iter(
        &native.sketch_curve_identities,
        "scan F3D sketch curve identities",
    )? {
        let duplicate = if let Some(owner_reference) = curve.owner_reference {
            !sketch_curve_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut sketch_curve_identities,
                    (
                        design_stream(&curve.id),
                        owner_reference,
                        curve.primary_id.get(),
                        curve.secondary_id,
                    ),
                    "index F3D sketch curve identities",
                )
            })?
        } else {
            false
        };
        if duplicate {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch curve has an invalid persistent identity",
                Some(
                    ctx.decode
                        .copy_retained_text(&curve.id, "retain F3D validation entity")?,
                ),
            )?;
        }
        if !sketch_geometry_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut sketch_geometry_records,
                (design_stream(&curve.id), curve.record_index),
                "index F3D sketch geometry records",
            )
        })? {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch geometry aliases another typed indexed record",
                Some(
                    ctx.decode
                        .copy_retained_text(&curve.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    let mut sketch_surface_storage = ctx
        .decode
        .reserve_scoped(0, "index F3D sketch surface identities")?;
    let mut sketch_surface_identities = HashSet::new();
    for surface in ctx
        .decode
        .admit_iter(&native.sketch_surfaces, "scan F3D sketch surfaces")?
    {
        let duplicate = if let Some(owner_reference) = surface.owner_reference {
            !sketch_surface_storage.with_storage(|| {
                ctx.decode.insert_hash_set(
                    &mut sketch_surface_identities,
                    (
                        design_stream(&surface.id),
                        owner_reference,
                        surface.persistent_id.get(),
                    ),
                    "index F3D sketch surface identities",
                )
            })?
        } else {
            false
        };
        if duplicate {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch surface has an invalid persistent identity",
                Some(
                    ctx.decode
                        .copy_retained_text(&surface.id, "retain F3D validation entity")?,
                ),
            )?;
        }
        if !sketch_geometry_storage.with_storage(|| {
            ctx.decode.insert_hash_set(
                &mut sketch_geometry_records,
                (design_stream(&surface.id), surface.record_index),
                "index F3D sketch geometry records",
            )
        })? {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch geometry aliases another typed indexed record",
                Some(
                    ctx.decode
                        .copy_retained_text(&surface.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

fn emit_sketch_relation_finding(
    decode: &DecodeContext<'_>,
    findings: &mut Vec<Finding>,
    entity: &str,
    message: &'static str,
) -> Result<(), CodecError> {
    decode.reserve_vec(findings, 1, "collect F3D sketch owner findings")?;
    let entity = decode.copy_retained_text(entity, "retain F3D sketch owner finding ID")?;
    findings.push(Finding {
        check: Check::NativeLinks,
        severity: Severity::Error,
        message: decode.copy_retained_text(message, "retain F3D sketch owner finding message")?,
        entity: Some(entity),
    });
    Ok(())
}

fn validate_sketch_relation_owners(
    ctx: &Ctx,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let decode = ctx.decode;
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D sketch relation owners scratch")?;
    let native = ctx.native;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let placements_by_scope = &ctx.placements_by_scope;
    let sketch_owner_ids = &ctx.sketch_owner_ids;
    let typed_sketch_records = scratch_storage.with_storage(|| {
        decode.collect_hash_set(
            decode
                .admit_iter(
                    &native.sketch_points,
                    "admit source for index F3D typed sketch records",
                )?
                .map(|point| (design_stream(&point.id), point.record_index))
                .chain(
                    decode
                        .admit_iter(
                            &native.sketch_curve_identities,
                            "admit source for index F3D typed sketch records",
                        )?
                        .map(|curve| (design_stream(&curve.id), curve.record_index)),
                )
                .chain(
                    decode
                        .admit_iter(
                            &native.sketch_surfaces,
                            "admit source for index F3D typed sketch records",
                        )?
                        .map(|surface| (design_stream(&surface.id), surface.record_index)),
                ),
            "index F3D typed sketch records",
        )
    })?;
    let sketch_operands = scratch_storage.with_storage(|| {
        decode.collect_hash_map(
            decode
                .admit_iter(
                    &native.sketch_points,
                    "admit source for index F3D sketch operands",
                )?
                .map(|point| {
                    (
                        (design_stream(&point.id), point.record_index),
                        records::sketch_relations::SketchRelationOperand::Point {
                            record_index: point.record_index,
                            persistent_id: point.persistent_id(),
                        },
                    )
                })
                .chain(
                    decode
                        .admit_iter(
                            &native.sketch_curve_identities,
                            "admit source for index F3D sketch operands",
                        )?
                        .map(|curve| {
                            (
                                (design_stream(&curve.id), curve.record_index),
                                records::sketch_relations::SketchRelationOperand::Curve {
                                    record_index: curve.record_index,
                                    primary_id: curve.primary_id.get(),
                                    secondary_id: curve.secondary_id,
                                },
                            )
                        }),
                )
                .chain(
                    decode
                        .admit_iter(
                            &native.sketch_surfaces,
                            "admit source for index F3D sketch operands",
                        )?
                        .map(|surface| {
                            (
                                (design_stream(&surface.id), surface.record_index),
                                records::sketch_relations::SketchRelationOperand::Surface {
                                    record_index: surface.record_index,
                                    persistent_id: surface.persistent_id.get(),
                                },
                            )
                        }),
                ),
            "index F3D sketch operands",
        )
    })?;
    let mut relation_owners = std::collections::HashMap::new();
    for (id, record_index, owner_reference) in decode
        .admit_iter(&native.sketch_points, "scan F3D sketch point owners")?
        .map(|point| (&point.id, point.record_index, point.owner_reference))
        .chain(
            decode
                .admit_iter(
                    &native.sketch_curve_identities,
                    "scan F3D sketch curve owners",
                )?
                .map(|curve| (&curve.id, curve.record_index, curve.owner_reference)),
        )
        .chain(
            decode
                .admit_iter(&native.sketch_surfaces, "scan F3D sketch surface owners")?
                .map(|surface| (&surface.id, surface.record_index, surface.owner_reference)),
        )
    {
        let Some(owner_reference) = owner_reference else {
            continue;
        };
        let native_stream = design_stream(id);
        if decode.contains_key_hash_map(
            sketch_owner_ids,
            &(native_stream, owner_reference),
            "find F3D sketch owner identity",
        )? {
            scratch_storage.with_storage(|| {
                decode.insert_hash_map(
                    &mut relation_owners,
                    (native_stream, record_index),
                    owner_reference,
                    "index F3D sketch relation owners",
                )
            })?;
        }
    }
    for relation in ctx
        .decode
        .admit_iter(&native.sketch_relations, "scan F3D sketch relations")?
    {
        let native_stream = design_stream(&relation.id);
        let agrees = |reference: &records::sketch_relations::SketchRelationReference| -> Result<bool, CodecError> {
            let record_index = reference.record_index();
            Ok(match decode.get_hash_map(&sketch_operands, &(native_stream, record_index), "find F3D sketch relation operand")? {
                Some(expected) => reference.resolved() == Some(expected),
                None => {
                    reference.resolved()
                        == Some(&records::sketch_relations::SketchRelationOperand::Record {
                            record_index,
                        })
                }
            })
        };
        if !decode.all_by(
            relation.members().iter(),
            |member| agrees(&member.reference),
            "validate F3D sketch relation members",
        )? || !decode.all_by(
            relation.return_members().iter(),
            |member| agrees(&member.reference),
            "validate F3D sketch relation return members",
        )? {
            emit_sketch_relation_finding(
                decode,
                findings,
                &relation.id,
                "Fusion sketch relation typed operands disagree with its indexed references",
            )?;
        }
        for member in decode
            .admit_iter(&relation.members()[..], "scan F3D sketch relation members")?
            .map(|member| member.reference.record_index())
            .chain(
                decode
                    .admit_iter(
                        &relation.return_members()[..],
                        "scan F3D sketch relation return members",
                    )?
                    .map(|member| member.reference.record_index()),
            )
        {
            if !decode.contains_hash_set(
                &typed_sketch_records,
                &(native_stream, member),
                "find F3D relation typed sketch record",
            )? {
                continue;
            }
            if scratch_storage
                .with_storage(|| {
                    decode.insert_hash_map(
                        &mut relation_owners,
                        (native_stream, member),
                        relation.owner_reference,
                        "index F3D sketch relation owners",
                    )
                })?
                .is_some_and(|owner| owner != relation.owner_reference)
            {
                emit_sketch_relation_finding(
                    decode,
                    findings,
                    &relation.id,
                    "Fusion sketch member belongs to multiple sketch owners",
                )?;
            }
        }
    }
    for entity in decode
        .admit_iter(
            &native.design_entity_headers,
            "scan F3D sketch entity headers",
        )?
        .filter(|entity| entity.in_sketch_module())
    {
        let native_stream = design_stream(&entity.id);
        let Ok(owner) = u32::try_from(entity.entity_id.suffix()) else {
            continue;
        };
        let Some(members) = entity.sketch_members() else {
            continue;
        };
        let (values, located) = members.storage_slices();
        for member in decode
            .admit_iter(values, "scan F3D sketch entity members")?
            .chain(
                decode
                    .admit_iter(located, "scan F3D sketch entity located members")?
                    .map(|row| &row.value),
            )
        {
            if !decode.contains_hash_set(
                &typed_sketch_records,
                &(native_stream, *member),
                "find F3D entity typed sketch record",
            )? {
                continue;
            }
            if scratch_storage
                .with_storage(|| {
                    decode.insert_hash_map(
                        &mut relation_owners,
                        (native_stream, *member),
                        owner,
                        "index F3D sketch relation owners",
                    )
                })?
                .is_some_and(|existing| existing != owner)
            {
                emit_sketch_relation_finding(
                    decode,
                    findings,
                    &entity.id,
                    "Fusion sketch member belongs to multiple sketch owners",
                )?;
            }
        }
    }
    for pair in ctx.decode.admit_iter(
        &native.design_dimension_locus_pairs[..],
        "scan F3D design dimension locus pairs",
    )? {
        let native_stream = design_stream(&pair.id);
        let owner = match decode.get_hash_map(
            companions_by_index,
            &(native_stream, pair.governing_companion_record_index),
            "find F3D sketch relation dimension companion",
        )? {
            Some(companion) => match decode.get_hash_map(
                owners_by_index,
                &(native_stream, companion.owner_record_index()),
                "find F3D sketch relation parameter owner",
            )? {
                Some(parameter_owner) => decode
                    .get_hash_map(
                        placements_by_scope,
                        &(native_stream, parameter_owner.scope_record_index()),
                        "find F3D sketch relation scope placement",
                    )?
                    .and_then(|placement| u32::try_from(placement.entity_id.suffix()).ok()),
                None => None,
            },
            None => None,
        };
        let Some(owner) = owner else {
            continue;
        };
        for member in [
            pair.loci()[0].geometry_index(),
            pair.loci()[1].geometry_index(),
        ] {
            if scratch_storage
                .with_storage(|| {
                    decode.insert_hash_map(
                        &mut relation_owners,
                        (native_stream, member),
                        owner,
                        "index F3D sketch relation owners",
                    )
                })?
                .is_some_and(|existing| existing != owner)
            {
                emit_sketch_relation_finding(
                    decode,
                    findings,
                    &pair.id,
                    "Fusion sketch member belongs to multiple sketch owners",
                )?;
            }
        }
    }
    for group in ctx.decode.admit_iter(
        &native.design_dimension_locus_groups,
        "scan F3D design dimension locus groups",
    )? {
        let native_stream = design_stream(&group.id);
        for member in decode
            .admit_iter(&group.loci, "scan F3D locus geometry members")?
            .map(|locus| locus.geometry_record_index)
            .chain(
                decode
                    .admit_iter(&group.loci, "scan F3D locus return members")?
                    .map(|locus| locus.returned.value),
            )
        {
            if scratch_storage
                .with_storage(|| {
                    decode.insert_hash_map(
                        &mut relation_owners,
                        (native_stream, member),
                        group.owner_reference,
                        "index F3D sketch relation owners",
                    )
                })?
                .is_some_and(|existing| existing != group.owner_reference)
            {
                emit_sketch_relation_finding(
                    decode,
                    findings,
                    &group.id,
                    "Fusion sketch member belongs to multiple sketch owners",
                )?;
            }
        }
    }
    for pair in ctx.decode.admit_iter(
        &native.design_dimension_null_locus_pairs[..],
        "scan F3D design dimension null locus pairs",
    )? {
        let native_stream = design_stream(&pair.id);
        let owner = match decode.get_hash_map(
            companions_by_index,
            &(native_stream, pair.governing_companion_record_index),
            "find F3D sketch relation dimension companion",
        )? {
            Some(companion) => match decode.get_hash_map(
                owners_by_index,
                &(native_stream, companion.owner_record_index()),
                "find F3D sketch relation parameter owner",
            )? {
                Some(parameter_owner) => decode
                    .get_hash_map(
                        placements_by_scope,
                        &(native_stream, parameter_owner.scope_record_index()),
                        "find F3D sketch relation scope placement",
                    )?
                    .and_then(|placement| u32::try_from(placement.entity_id.suffix()).ok()),
                None => None,
            },
            None => None,
        };
        let Some(owner) = owner else {
            continue;
        };
        if scratch_storage
            .with_storage(|| {
                decode.insert_hash_map(
                    &mut relation_owners,
                    (native_stream, pair.loci()[1].geometry_index()),
                    owner,
                    "index F3D sketch relation owners",
                )
            })?
            .is_some_and(|existing| existing != owner)
        {
            emit_sketch_relation_finding(
                decode,
                findings,
                &pair.id,
                "Fusion sketch member belongs to multiple sketch owners",
            )?;
        }
    }
    for (id, record_index, owner_reference) in decode
        .admit_iter(&native.sketch_points, "scan F3D sketch point owners")?
        .map(|point| (&point.id, point.record_index, point.owner_reference))
        .chain(
            decode
                .admit_iter(
                    &native.sketch_curve_identities,
                    "scan F3D sketch curve owners",
                )?
                .map(|curve| (&curve.id, curve.record_index, curve.owner_reference)),
        )
    {
        if decode
            .get_hash_map(
                &relation_owners,
                &(design_stream(id), record_index),
                "find F3D sketch geometry relation owner",
            )?
            .copied()
            != owner_reference
        {
            emit_sketch_relation_finding(
                decode,
                findings,
                id,
                "Fusion sketch geometry owner disagrees with its relation graph",
            )?;
        }
    }
    Ok(())
}

/// Validate persistent body links and their history ordering.
fn validate_body_links(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D body links scratch")?;
    let native = ctx.native;
    let ir = ctx.ir;
    let body_ids = scratch_storage.with_storage(|| {
        ctx.decode.collect_hash_set(
            ir.model.bodies.iter().map(|body| &body.id),
            "index F3D persistent body targets",
        )
    })?;
    let mut body_links: std::collections::BTreeMap<_, Vec<_>> = std::collections::BTreeMap::new();
    for link in ctx.decode.admit_iter(
        &native.persistent_design_links,
        "scan F3D persistent design links",
    )? {
        let target_key = match &link.target {
            cadmpeg_ir::attributes::AttributeTarget::Body(id)
                if ctx.decode.contains_hash_set(
                    &body_ids,
                    id,
                    "find F3D persistent body target",
                )? =>
            {
                Some(id)
            }
            _ => None,
        };
        let Some(target_key) = target_key else {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion persistent body link has an invalid target or group payload",
                Some(
                    ctx.decode
                        .copy_retained_text(&link.id, "retain F3D validation entity")?,
                ),
            )?;
            continue;
        };
        scratch_storage.with_storage(|| {
            ctx.decode.push_btree_group(
                &mut body_links,
                target_key,
                link,
                "index F3D persistent body link groups",
                "collect F3D persistent body link members",
            )
        })?;
    }
    for (_, links) in ctx
        .decode
        .admit_iter(&mut body_links, "scan F3D persistent body link groups")?
    {
        ctx.decode.stable_sort_by(
            links,
            |value| &value.ordinal,
            Ord::cmp,
            "f3d body links sort",
        )?;
        if ctx.decode.any_by(
            links.iter().enumerate(),
            |(ordinal, link)| Ok(u32::try_from(ordinal) != Ok(link.ordinal)),
            "validate F3D persistent body link ordering",
        )? {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion persistent body links have noncanonical history ordering",
                links
                    .first()
                    .map(|link| {
                        ctx.decode
                            .copy_retained_text(&link.id, "retain F3D validation entity")
                    })
                    .transpose()?,
            )?;
        }
    }
    Ok(())
}

/// Validate persistent subentity tags and their group ordering.
fn validate_subentity_tags(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut scratch_storage = ctx
        .decode
        .reserve_scoped(0, "hold F3D subentity tags scratch")?;
    let native = ctx.native;
    let ir = ctx.ir;
    let face_ids = scratch_storage.with_storage(|| {
        ctx.decode.collect_hash_set(
            ir.model.faces.iter().map(|face| &face.id),
            "index F3D persistent face targets",
        )
    })?;
    let edge_ids = scratch_storage.with_storage(|| {
        ctx.decode.collect_hash_set(
            ir.model.edges.iter().map(|edge| &edge.id),
            "index F3D persistent edge targets",
        )
    })?;
    let mut subentity_tags = std::collections::BTreeMap::new();
    for tag in ctx.decode.admit_iter(
        &native.persistent_subentity_tags,
        "scan F3D persistent subentity tags",
    )? {
        let Some(target_key) = (match &tag.target {
            cadmpeg_ir::attributes::AttributeTarget::Face(id)
                if ctx.decode.contains_hash_set(
                    &face_ids,
                    id,
                    "find F3D persistent face target",
                )? =>
            {
                Some((1_u8, id.as_str()))
            }
            cadmpeg_ir::attributes::AttributeTarget::Edge(id)
                if ctx.decode.contains_hash_set(
                    &edge_ids,
                    id,
                    "find F3D persistent edge target",
                )? =>
            {
                Some((0_u8, id.as_str()))
            }
            _ => None,
        }) else {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion persistent subentity tag has an invalid target or group payload",
                Some(
                    ctx.decode
                        .copy_retained_text(&tag.id, "retain F3D validation entity")?,
                ),
            )?;
            continue;
        };
        scratch_storage.with_storage(|| {
            ctx.decode.push_btree_group(
                &mut subentity_tags,
                target_key,
                tag,
                "index F3D persistent subentity tag groups",
                "collect F3D persistent subentity tag members",
            )
        })?;
    }
    for (_, tags) in ctx.decode.admit_iter(
        &mut subentity_tags,
        "scan F3D persistent subentity tag groups",
    )? {
        ctx.decode.stable_sort_by(
            tags,
            |value| &value.ordinal,
            Ord::cmp,
            "f3d subentity tags sort",
        )?;
        if ctx.decode.any_by(
            tags.iter().enumerate(),
            |(ordinal, tag)| Ok(u32::try_from(ordinal) != Ok(tag.ordinal)),
            "validate F3D persistent subentity tag ordering",
        )? {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion persistent subentity tags have noncanonical group ordering",
                tags.first()
                    .map(|tag| {
                        ctx.decode
                            .copy_retained_text(&tag.id, "retain F3D validation entity")
                    })
                    .transpose()?,
            )?;
        }
    }
    Ok(())
}

/// Validate each ASM history graph as a coherent state chain.
fn validate_history_graphs(ctx: &Ctx, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    let decode = ctx.decode;
    let native = ctx.native;
    for history in ctx
        .decode
        .admit_iter(&native.asm_histories, "scan F3D asm histories")?
    {
        let coherent = history::graph_is_coherent_charged(decode, history)?;
        if !coherent {
            ctx.push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ASM history graph is not a coherent doubly linked state chain",
                Some(
                    ctx.decode
                        .copy_retained_text(&history.id, "retain F3D validation entity")?,
                ),
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
