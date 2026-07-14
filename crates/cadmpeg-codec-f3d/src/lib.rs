// SPDX-License-Identifier: Apache-2.0
//! Read and write Autodesk Fusion `.f3d` archives.
//!
//! [`F3dCodec`] implements [`Codec`] and [`Encoder`]. Decoding produces a
//! [`CadIr`] document with B-rep topology, analytic and cached NURBS geometry,
//! body transforms, design and sketch records, construction history, and
//! appearances. Encoding replays an unchanged decoded archive byte for byte,
//! applies supported semantic edits to retained source data, or creates an
//! archive from the supported source-less profile.
//!
//! Support level: [L4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#support-ladder)
//! on the cadmpeg support ladder.
//!
//! # Decode
//!
//! ```no_run
//! use cadmpeg_codec_f3d::F3dCodec;
//! use cadmpeg_ir::{Codec, DecodeOptions};
//! use std::fs::File;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("part.f3d")?;
//! let result = F3dCodec.decode(&mut input, &DecodeOptions::default())?;
//! for loss in &result.report.losses {
//!     eprintln!("{:?}: {}", loss.severity, loss.message);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! [`Codec::inspect`] classifies the ZIP entries and reads ASM B-rep headers
//! without building geometry. `DecodeOptions::container_only` provides the
//! corresponding metadata-only `CadIr`.
//!
//! # Encode
//!
//! ```no_run
//! use cadmpeg_codec_f3d::F3dCodec;
//! use cadmpeg_ir::{Codec, DecodeOptions, Encoder};
//! use std::fs::File;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("part.f3d")?;
//! let mut result = F3dCodec.decode(&mut input, &DecodeOptions::default())?;
//! // Edit supported fields in result.ir.
//! let mut output = File::create("part-edited.f3d")?;
//! F3dCodec.encode(&result.ir, &mut output)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Data flow
//!
//! [`container`] selects the authoritative `.smbh` B-rep, or the first `.smb`
//! construction snapshot when no `.smbh` exists. [`sab`] frames its active
//! record slice. [`brep`] builds the topology chain from bodies through
//! vertices and points, while [`nurbs`] decodes cached spline carriers.
//! [`design`], [`history`], and [`materials`] populate source-native records and
//! appearance bindings.
//!
//! ASM model-space lengths become millimetres. Directions, ratios, angles,
//! knots, weights, and UV parameters retain their native scale.
//!
//! Inspect [`cadmpeg_ir::report::DecodeReport::losses`] before consuming a
//! decode. A stream that cannot produce geometry returns container metadata,
//! retained source data, and blocking geometry and topology losses. Referenced
//! carrier bytes needed for passthrough remain available as
//! [`cadmpeg_ir::unknown::UnknownRecord`] values.

mod act;
pub mod asm_header;
pub mod brep;
pub mod container;
pub mod decode;
pub mod design;
pub mod history;
mod history_records;
pub mod materials;
mod native;
pub mod nurbs;
pub mod records;
pub mod sab;
mod writer;

use cadmpeg_ir::codec::{
    Codec, CodecError, Confidence, ContainerSummary, DecodeOptions, DecodeResult, Encoder, ReadSeek,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::report::ExportReport;
use cadmpeg_ir::{Check, Finding, Severity};
use std::io::Write;

/// The ZIP local-file-header magic.
const ZIP_MAGIC: &[u8] = b"PK\x03\x04";

/// The Autodesk Fusion `.f3d` container codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct F3dCodec;

fn design_stream(id: &str) -> &str {
    design::native_stream(id).unwrap_or("f3d:design")
}

fn valid_design_guid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            matches!(index, 8 | 13 | 18 | 23) && byte == b'-'
                || !matches!(index, 8 | 13 | 18 | 23) && byte.is_ascii_hexdigit()
        })
}

/// Validate Fusion native design-record relationships and exact sketch frames.
pub fn validate_native(ir: &CadIr) -> Vec<Finding> {
    use std::collections::HashSet;

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
    let mut findings = Vec::new();
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
    let entity_headers_by_suffix = native
        .design_entity_headers
        .iter()
        .map(|entity| ((design_stream(&entity.id), entity.entity_suffix), entity))
        .collect::<std::collections::HashMap<_, _>>();
    let body_native_keys = native
        .body_native_keys
        .iter()
        .filter_map(|key| Some(((key.body.clone(), key.asm_body_key?), key)))
        .collect::<std::collections::HashMap<_, _>>();
    let mut binding_offsets = HashSet::new();
    let mut binding_groups =
        std::collections::HashMap::<(&str, u64), Vec<&records::DesignBodyBinding>>::new();
    for binding in &native.design_body_bindings {
        let native_stream = design_stream(&binding.id);
        let valid = native_stream == format!("f3d:{}", binding.stream)
            && binding.pair_count > 0
            && binding.pair_ordinal < binding.pair_count
            && binding.entity_suffix_offset == binding.asm_body_key_offset.saturating_add(8)
            && binding.blob_name.starts_with("BREP.")
            && binding.blob_name_offset > binding.entity_suffix_offset
            && binding.body.as_ref().is_none_or(|body| {
                body_native_keys.contains_key(&(body.clone(), binding.asm_body_key))
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
                native_stream == format!("f3d:{}", binding.stream)
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
    let recipes_by_id = native
        .construction_recipes
        .iter()
        .map(|recipe| (recipe.id.as_str(), recipe))
        .collect::<std::collections::HashMap<_, _>>();
    let mut parameter_indices = HashSet::new();
    let mut parameter_ordinals = HashSet::new();
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
        .map(|placement| {
            (
                (design_stream(&placement.id), placement.scope_record_index),
                placement,
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    let asm_state_ids = native
        .asm_histories
        .iter()
        .flat_map(|history| &history.states)
        .map(|state| state.state_id)
        .collect::<HashSet<_>>();
    let mut scope_indices = HashSet::new();
    let mut scope_ordinals = HashSet::new();
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
        let extrude_profile_link = match (&scope.extrude_profile, scope.kind.as_str()) {
            (None, "Extrude") => false,
            (None, _) => true,
            (Some(_), kind) if kind != "Extrude" => false,
            (Some(profile), _) => {
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
                scope.kind.as_str(),
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
                    "Extrude",
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
                ("Extrude", _, _, _, _, _, _, _, _) => false,
                (_, None, None, None, None, None, None, None, None) => true,
                _ => false,
            }
            && scope.frame_length > 89
            && scope.paired_byte_offset == scope.byte_offset.saturating_add(scope.frame_length)
            && scope.kind_offset > scope.byte_offset
            && scope.kind_offset < scope.paired_byte_offset.saturating_sub(78)
            && scope.feature_ordinal > 0
            && scope.feature_ordinal_offset == scope.paired_byte_offset.saturating_sub(78)
            && scope.history_state_id_offset == scope.kind_offset.saturating_sub(8)
            && scope.previous_history_state_id_offset
                == scope.feature_ordinal_offset.saturating_add(31)
            && scope.history_state_id.is_some() == scope.previous_history_state_id.is_some()
            && scope
                .history_state_id
                .is_none_or(|state_id| asm_state_ids.contains(&state_id))
            && scope
                .previous_history_state_id
                .is_none_or(|state_id| asm_state_ids.contains(&state_id))
            && scope_ordinals.insert((native_stream, scope.kind.as_str(), scope.feature_ordinal))
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
    let groups_by_index = native
        .design_extrude_selection_groups
        .iter()
        .map(|group| ((design_stream(&group.id), group.record_index), group))
        .collect::<std::collections::HashMap<_, _>>();
    let mut group_scopes = HashSet::new();
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
                scope.kind == "Extrude"
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
            ))
            && group_scopes.insert((native_stream, group.scope_record_index));
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
    for scope in native
        .design_parameter_scopes
        .iter()
        .filter(|scope| scope.kind == "Extrude")
    {
        let native_stream = design_stream(&scope.id);
        if !group_scopes.contains(&(native_stream, scope.record_index)) {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design Extrude scope has no counted selection group".into(),
                entity: Some(scope.id.clone()),
            });
        }
    }
    let mut operand_group_slots = HashSet::new();
    let mut operand_group_scopes = HashSet::new();
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
                let role_is_valid = match scope.kind.as_str() {
                    "Extrude" => match group.extrude_role {
                        Some(records::DesignExtrudeOperandRole::Bodies) => {
                            group.role == 0x0000_0008_0000_0000 && group.extrude_face_role.is_none()
                        }
                        Some(records::DesignExtrudeOperandRole::Profile) => {
                            group.role == 0x0000_0041_0000_0000
                                && group.extrude_face_role.is_none()
                                && scope.extrude_profile.as_ref().is_some_and(|profile| {
                                    group.members.as_slice() == [profile.record_index]
                                })
                        }
                        Some(records::DesignExtrudeOperandRole::Faces) => {
                            group.role == 0x0000_0011_0000_0000 && group.extrude_face_role.is_some()
                        }
                        None => false,
                    },
                    "Fillet" | "Chamfer" => {
                        group.extrude_role.is_none() && group.extrude_face_role.is_none()
                    }
                    _ => false,
                };
                matches!(scope.kind.as_str(), "Extrude" | "Fillet" | "Chamfer")
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
            && group.member_count_offset == group.byte_offset.saturating_add(21)
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
        if valid {
            operand_group_scopes.insert((native_stream, group.scope_record_index));
        } else {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design construction operand group has an invalid counted frame"
                    .into(),
                entity: Some(group.id.clone()),
            });
        }
    }
    for scope in native
        .design_parameter_scopes
        .iter()
        .filter(|scope| matches!(scope.kind.as_str(), "Extrude" | "Fillet" | "Chamfer"))
    {
        let native_stream = design_stream(&scope.id);
        if !operand_group_scopes.contains(&(native_stream, scope.record_index)) {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design feature scope has no counted operand group".into(),
                entity: Some(scope.id.clone()),
            });
        }
        if scope.kind == "Extrude" {
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
            let extent_matches_operands = match scope.extrude_extent {
                Some(records::DesignExtrudeExtent::OneSidedDistance) => {
                    along_count == 1
                        && against_count == 0
                        && side_one_offset_count == 0
                        && scope.extrude_direction_reversed == Some(false)
                }
                Some(records::DesignExtrudeExtent::OneSidedToFace) => {
                    along_count == 0 && against_count == 0 && side_one_offset_count == 1
                }
                Some(records::DesignExtrudeExtent::TwoSidedDistance) => {
                    along_count == 1
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
        let radius =
            parameters_by_index.get(&(native_stream, assignment.radius_parameter_record_index));
        let tangency_weight = assignment
            .tangency_weight_parameter_record_index
            .and_then(|record_index| parameters_by_index.get(&(native_stream, record_index)));
        let valid = scope.is_some_and(|scope| scope.kind == "Fillet")
            && group.is_some_and(|group| {
                group.scope_record_index == assignment.scope_record_index
                    && group.members == assignment.edge_operand_record_indices
            })
            && radius.is_some_and(|parameter| {
                parameter.source_kind == "Radius"
                    && parameter.unit.as_deref() == Some("mm")
                    && parameter.evaluated_value > 0.0
                    && parameter.evaluated_value.is_finite()
            })
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
    for group in &native.design_construction_operand_groups {
        let native_stream = design_stream(&group.id);
        let is_fillet = scopes_by_index
            .get(&(native_stream, group.scope_record_index))
            .is_some_and(|scope| scope.kind == "Fillet");
        if is_fillet && !fillet_radius_group_records.contains(&(native_stream, group.record_index))
        {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design Fillet operand group has no radius assignment".into(),
                entity: Some(group.id.clone()),
            });
        }
    }
    let operand_groups_by_index = native
        .design_construction_operand_groups
        .iter()
        .map(|group| ((design_stream(&group.id), group.record_index), group))
        .collect::<std::collections::HashMap<_, _>>();
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
    for group in &native.design_construction_operand_groups {
        let native_stream = design_stream(&group.id);
        if !operand_identity_groups.contains(&(native_stream, group.record_index)) {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design construction operand group has no identity chain".into(),
                entity: Some(group.id.clone()),
            });
        }
    }
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
            && selected_profile.is_some_and(|profile| profile.asset_id == member.asset_id)
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
    let mut edge_operand_slots = HashSet::new();
    let mut edge_operand_records = HashSet::new();
    let mut edge_operand_scopes = HashSet::new();
    for operand in &native.design_edge_operands {
        let native_stream = design_stream(&operand.id);
        let scope = scopes_by_index.get(&(native_stream, operand.scope_record_index));
        let header = records_by_index.get(&(native_stream, operand.record_index));
        let recipe = recipes_by_id.get(operand.recipe_id.as_str());
        let valid = operand.class_tag.len() == 3
            && operand.class_tag.bytes().all(|byte| byte.is_ascii_digit())
            && operand.paired_class_tag.len() == 3
            && operand
                .paired_class_tag
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            && scope.is_some_and(|scope| {
                matches!(scope.kind.as_str(), "Fillet" | "Chamfer")
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
            && recipe.is_some_and(|recipe| {
                design_stream(&recipe.id) == native_stream
                    && recipe.kind == crate::records::ConstructionRecipeKind::Edge
                    && recipe.byte_offset > operand.recipe_record_byte_offset
                    && recipe.byte_offset < operand.next_byte_offset
            })
            && edge_operand_slots.insert((
                native_stream,
                operand.scope_record_index,
                operand.scope_reference_ordinal,
            ))
            && edge_operand_records.insert((native_stream, operand.record_index));
        if valid {
            edge_operand_scopes.insert((native_stream, operand.scope_record_index));
        } else {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design edge operand has an invalid scope or recipe frame".into(),
                entity: Some(operand.id.clone()),
            });
        }
    }
    for scope in native
        .design_parameter_scopes
        .iter()
        .filter(|scope| matches!(scope.kind.as_str(), "Fillet" | "Chamfer"))
    {
        let native_stream = design_stream(&scope.id);
        if !edge_operand_scopes.contains(&(native_stream, scope.record_index)) {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design edge-treatment scope has no edge recipe operand".into(),
                entity: Some(scope.id.clone()),
            });
        }
    }
    let face_group_members = native
        .design_construction_operand_groups
        .iter()
        .filter(|group| group.extrude_role == Some(records::DesignExtrudeOperandRole::Faces))
        .flat_map(|group| {
            let native_stream = design_stream(&group.id);
            group
                .members
                .iter()
                .map(move |member| (native_stream, group.scope_record_index, *member))
        })
        .collect::<HashSet<_>>();
    let mut face_operand_records = HashSet::new();
    for operand in &native.design_face_operands {
        let native_stream = design_stream(&operand.id);
        let scope = scopes_by_index.get(&(native_stream, operand.scope_record_index));
        let header = records_by_index.get(&(native_stream, operand.record_index));
        let recipe = recipes_by_id.get(operand.recipe_id.as_str());
        let mut expected_faces = recipe
            .and_then(|recipe| recipe.design_id.as_deref())
            .and_then(|value| value.parse::<i64>().ok())
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
        let valid = operand.class_tag.len() == 3
            && operand.class_tag.bytes().all(|byte| byte.is_ascii_digit())
            && operand.paired_class_tag.len() == 3
            && operand
                .paired_class_tag
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            && scope.is_some_and(|scope| {
                scope.kind == "Extrude"
                    && usize::try_from(operand.scope_reference_ordinal)
                        .ok()
                        .and_then(|ordinal| scope.reference_members.get(ordinal))
                        == Some(&operand.record_index)
            })
            && face_group_members.contains(&(
                native_stream,
                operand.scope_record_index,
                operand.record_index,
            ))
            && header.is_some_and(|header| {
                header.byte_offset == operand.byte_offset && header.class_tag == operand.class_tag
            })
            && operand.paired_byte_offset > operand.byte_offset
            && operand.recipe_record_index == operand.record_index.saturating_add(3)
            && operand.recipe_record_byte_offset > operand.paired_byte_offset
            && operand.next_byte_offset > operand.recipe_record_byte_offset
            && matches!(
                operand.recipe_kind,
                records::ConstructionRecipeKind::Face
                    | records::ConstructionRecipeKind::BoundedFace
            )
            && operand.recipe_program.len() >= 3
            && operand.recipe_program.get(0..2) == Some(&[0, -1])
            && usize::try_from(operand.recipe_program[2]).ok()
                == Some(operand.recipe_node_offsets.len())
            && operand.recipe_node_offsets == expected_node_offsets
            && operand.recipe_nodes.len() == expected_nodes.len()
            && operand
                .recipe_nodes
                .iter()
                .zip(expected_nodes)
                .all(|(node, (start, end))| {
                    node.byte_offset == start
                        && node.end_byte_offset == end
                        && node.program.get(0..3) == Some(&[-1, -1, 2])
                        && u64::try_from(node.program.len()).ok().is_some_and(|words| {
                            start.saturating_add(words.saturating_mul(4)) == end
                        })
                })
            && operand
                .recipe_nodes
                .iter()
                .flat_map(|node| node.program.iter().copied())
                .eq(operand.recipe_program.iter().copied().skip(3))
            && operand.recipe_node_offsets.first()
                == Some(&operand.recipe_program_offset.saturating_add(12))
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
    for member in face_group_members {
        if !face_operand_records.contains(&member) {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion Design Extrude face group has an unresolved recipe operand".into(),
                entity: None,
            });
        }
    }
    let mut placement_records = HashSet::new();
    let mut placement_scopes = HashSet::new();
    for placement in &native.design_sketch_placements {
        let native_stream = design_stream(&placement.id);
        let unique_record = placement_records.insert((native_stream, placement.record_index));
        let unique_scope = placement_scopes.insert((native_stream, placement.scope_record_index));
        let scope = scopes_by_index.get(&(native_stream, placement.scope_record_index));
        let compact = placement.frame_length == 201
            && placement.transform_offset.is_none()
            && placement.transform
                == [
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ];
        let explicit = placement.frame_length == 329
            && placement.transform_offset == Some(placement.byte_offset.saturating_add(55));
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
            && placement.paired_byte_offset
                == placement.byte_offset.saturating_add(placement.frame_length)
            && (compact || explicit)
            && design::valid_sketch_transform(&placement.transform)
            && scope.is_some_and(|scope| {
                scope.kind == "Sketch"
                    && scope.entity_id.as_deref() == Some(placement.entity_id.as_str())
                    && scope.entity_suffix == Some(placement.entity_suffix)
            })
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
            && owner.evaluated_value_offset == owner.byte_offset + 40
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
        let mut decoded_references =
            design::decode_dimension_recipe_references(&record.prefix_bytes, record.prefix_offset);
        for reference in &mut decoded_references {
            reference.candidate_faces = design::dimension_recipe_candidate_faces(
                reference,
                &native.persistent_subentity_tags,
            );
            reference.candidate_edges = design::dimension_recipe_candidate_edges(
                reference,
                &native.persistent_subentity_tags,
            );
        }
        let valid = record.class_tag.len() == 3
            && record.class_tag.bytes().all(|byte| byte.is_ascii_digit())
            && record.frame_length >= 11
            && !record.prefix_bytes.is_empty()
            && decoded_references == record.references
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
        let valid = pair.class_tag.len() == 3
            && pair.class_tag.bytes().all(|byte| byte.is_ascii_digit())
            && pair.paired_class_tag.len() == 3
            && pair
                .paired_class_tag
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            && companion_contains_frame
            && dimension_companion
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
        let (expected_kinds, expected_unknown) = design::decode_constraint_kinds(group.state);
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
            && group.unknown_constraint_bits == expected_unknown
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
            && parameter.prefix_value == design::design_parameter_prefix(&parameter.source_kind)
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
    }
    let sketch_owners = native
        .design_entity_headers
        .iter()
        .filter(|header| header.object_kind == Some(records::DesignObjectKind::Sketch))
        .map(|header| (design_stream(&header.id), header.entity_suffix as u32))
        .collect::<HashSet<_>>();
    let sketch_owner_ids = native
        .design_entity_headers
        .iter()
        .filter(|header| header.object_kind == Some(records::DesignObjectKind::Sketch))
        .map(|header| {
            (
                (design_stream(&header.id), header.entity_suffix as u32),
                header.entity_id.as_str(),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    for relation in &native.sketch_relations {
        let native_stream = design_stream(&relation.id);
        let (constraint_kinds, unknown_constraint_bits) =
            design::decode_constraint_kinds(relation.state);
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
    for point in &native.sketch_points {
        if !point.coordinates.u.is_finite() || !point.coordinates.v.is_finite() {
            findings.push(Finding {
                check: Check::Bounds,
                severity: Severity::Error,
                message: "Fusion sketch point contains a non-finite coordinate".into(),
                entity: Some(point.id.clone()),
            });
        }
    }
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
        .collect::<std::collections::HashMap<_, _>>();
    let mut relation_owners = std::collections::HashMap::new();
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
    for pair in &native.design_dimension_locus_pairs {
        let native_stream = design_stream(&pair.id);
        let owner = companions_by_index
            .get(&(native_stream, pair.companion_record_index))
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
            .get(&(native_stream, pair.companion_record_index))
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
    let body_ids = ir
        .model
        .bodies
        .iter()
        .map(|body| &body.id)
        .collect::<HashSet<_>>();
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
    findings
}

impl F3dCodec {
    /// Write a decoded F3D document, replaying its source bytes when its
    /// semantic content is unchanged.
    ///
    /// Supported edits regenerate affected records within the retained archive.
    /// The method returns [`CodecError::NotImplemented`] when `ir` has no F3D
    /// semantic baseline or retained source image.
    pub fn write_preserved(&self, ir: &CadIr, writer: &mut dyn Write) -> Result<(), CodecError> {
        let expected = ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("semantic_sha256"))
            .ok_or_else(|| CodecError::NotImplemented("IR has no F3D semantic baseline".into()))?;
        let unknowns = ir.native_unknowns("f3d")?;
        let record = unknowns
            .iter()
            .find(|record| record.id.0 == "f3d:file:source-image#0")
            .ok_or_else(|| {
                CodecError::NotImplemented("IR has no retained F3D source image".into())
            })?;
        let data = record.data.as_ref().ok_or_else(|| {
            CodecError::Malformed("retained F3D source image has no bytes".into())
        })?;
        let hash = sha256_hex(data);
        if data.len() as u64 != record.byte_len || hash != record.sha256 {
            return Err(CodecError::Malformed(
                "retained F3D source image failed integrity validation".into(),
            ));
        }
        if decode::semantic_hash(ir) != *expected {
            return writer::write_semantic(ir, data, writer);
        }
        writer.write_all(data)?;
        Ok(())
    }
}

impl Codec for F3dCodec {
    fn id(&self) -> &'static str {
        "f3d"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        if !prefix.starts_with(ZIP_MAGIC) {
            return Confidence::No;
        }
        // A ZIP alone is a weak signal (many formats are ZIPs). An f3d marker
        // string in the prefix — entry names are stored in cleartext in ZIP
        // local headers — makes it conclusive.
        if container::DETECT_MARKERS
            .iter()
            .any(|m| contains_subslice(prefix, m))
        {
            Confidence::High
        } else {
            Confidence::Low
        }
    }

    fn inspect(&self, reader: &mut dyn ReadSeek) -> Result<ContainerSummary, CodecError> {
        let scan = container::scan(reader)?;
        Ok(container::summarize(&scan))
    }

    fn decode(
        &self,
        reader: &mut dyn ReadSeek,
        options: &DecodeOptions,
    ) -> Result<DecodeResult, CodecError> {
        decode::decode(reader, options)
    }
}

impl Encoder for F3dCodec {
    fn id(&self) -> &'static str {
        "f3d"
    }

    fn encode(&self, ir: &CadIr, writer: &mut dyn Write) -> Result<ExportReport, CodecError> {
        let replay = ir
            .native_unknowns("f3d")?
            .into_iter()
            .any(|record| record.id.0 == "f3d:file:source-image#0");
        if replay {
            self.write_preserved(ir, writer)?;
        } else {
            writer::write_new(ir, writer)?;
        }
        let validation = cadmpeg_ir::validate(ir, Vec::new());
        let total_entities = validation.entity_counts.values().sum();
        Ok(ExportReport {
            format: "f3d".into(),
            entity_counts: validation.entity_counts,
            total_entities,
            losses: Vec::new(),
            notes: vec![
                if replay {
                    "preserved source container replayed verbatim"
                } else {
                    "source container regenerated from IR"
                }
                .into(),
                "entity counts are derived from the IR".into(),
            ],
        })
    }
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[cfg(test)]
mod tests;
