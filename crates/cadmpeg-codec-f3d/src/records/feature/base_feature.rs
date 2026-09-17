// SPDX-License-Identifier: Apache-2.0
//! Base features, the body references they carry and the results they state.

use crate::records::{identity::Located, mesh::DesignRelaxedGuidText};
use serde::{Deserialize, Serialize};
/// Encoded compact Base Feature mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DesignBaseFeatureCompactMode {
    Zero = 0,
    One = 1,
}

impl TryFrom<u8> for DesignBaseFeatureCompactMode {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Zero),
            1 => Ok(Self::One),
            _ => Err("mode must be 0 or 1"),
        }
    }
}

/// Layout of the legacy class-452/class-262 Base Feature envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesignBaseFeatureBodyReferenceForm {
    /// One output body with an encoded compact mode.
    CompactOneBody {
        mode: Located<DesignBaseFeatureCompactMode>,
        body: DesignLegacyBaseFeatureBody,
    },
    /// Two output bodies with no mode slot.
    ExpandedTwoBody {
        bodies: [DesignLegacyBaseFeatureBody; 2],
    },
}

/// One body in a legacy Base Feature envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesignLegacyBaseFeatureBody {
    pub entity: DesignBaseFeatureEntry<u32>,
    pub parameter_body: Located<u64>,
    pub auxiliary: Located<u64>,
}

impl DesignBaseFeatureBodyReferenceForm {
    fn bodies(&self) -> &[DesignLegacyBaseFeatureBody] {
        match self {
            Self::CompactOneBody { body, .. } => std::slice::from_ref(body),
            Self::ExpandedTwoBody { bodies } => bodies,
        }
    }
}

/// Typed construction data carried by a Fusion direct-modeling Base Feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignBaseFeatureConstructionWire",
    into = "DesignBaseFeatureConstructionWire"
)]
pub enum DesignBaseFeatureConstruction {
    /// Counted body, passive-reference, metadata, and result runs.
    ResultBodies {
        /// Ordered body, passive-reference, result, and optional repeated-field rows.
        bodies: DesignBaseFeatureResults,
        /// Shared passive-reference metadata record.
        metadata_record: u32,
        /// Byte offset of the shared metadata record.
        metadata_record_offset: u64,
        /// Variant-width source field following the metadata record.
        metadata_field: Vec<u8>,
    },
    /// Direct-modeling body-reference envelope used by the class-365/class-262 and
    /// class-377/class-259 forms.
    BodyBasedOnFaces {
        /// The single body suffix and the location shared by its reference views.
        body: Located<u32>,
        /// PM body-reference record named by the fixed envelope lane.
        parameter_body_record: u32,
        /// Byte offset of `parameter_body_record`.
        parameter_body_record_offset: u64,
        /// Auxiliary record named by the fixed envelope lane.
        auxiliary_record: u32,
        /// Byte offset of `auxiliary_record`.
        auxiliary_record_offset: u64,
        /// LP-UTF-16 GUID carried by the envelope.
        envelope_guid: DesignRelaxedGuidText,
        /// Byte offset of the first code unit of `envelope_guid`.
        envelope_guid_offset: u64,
        /// Byte offset of `tag_body_based_on_faces`.
        tag_body_based_on_faces_offset: u64,
    },
    /// Legacy body-reference envelope used by the class-452/class-262 forms.
    LegacyBodyBasedOnFaces {
        /// Compact one-body or expanded two-body source envelope form.
        form: DesignBaseFeatureBodyReferenceForm,
        /// Scope record repeated by the envelope's explicit scope-reference lane.
        scope_reference: u64,
        /// Byte offset of `scope_reference`.
        scope_reference_offset: u64,
        /// LP-UTF-16 GUID carried by the envelope.
        envelope_guid: DesignRelaxedGuidText,
        /// Byte offset of the first code unit of `envelope_guid`.
        envelope_guid_offset: u64,
        /// Byte offset of `tag_body_based_on_faces`.
        tag_body_based_on_faces_offset: u64,
    },
    /// Body snapshot form used by the class-314/class-259 scope pair.
    BodySnapshot {
        /// Ordered snapshot bodies with their source fields.
        bodies: Vec<DesignBaseFeatureEntry<u64>>,
        /// Three LP-UTF-16 source GUIDs carried by the snapshot envelope.
        related_guids: [DesignRelaxedGuidText; 3],
        /// Byte offsets of the first code unit of each related GUID.
        related_guid_offsets: [u64; 3],
        /// Indexed record carried by the snapshot linkage tail.
        linkage_record: u32,
        /// Byte offset of `linkage_record`.
        linkage_record_offset: u64,
        /// Auxiliary indexed record carried by the snapshot linkage tail.
        auxiliary_record: u32,
        /// Byte offset of `auxiliary_record`.
        auxiliary_record_offset: u64,
    },
}

/// One aligned body, passive reference, and result record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesignBaseFeatureResultBody {
    pub entity: DesignBaseFeatureEntry<u64>,
    pub reference: DesignBaseFeatureEntry<u32>,
    pub result: DesignBaseFeatureEntry<u32>,
}

/// Result-body runs with either no repeated fields or one field per body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesignBaseFeatureResults {
    WithoutRepeatedFields(Vec<DesignBaseFeatureResultBody>),
    WithRepeatedFields {
        first: (DesignBaseFeatureResultBody, [u8; 6]),
        rest: Vec<(DesignBaseFeatureResultBody, [u8; 6])>,
    },
}

impl DesignBaseFeatureResults {
    pub(crate) fn iter(&self) -> impl ExactSizeIterator<Item = &DesignBaseFeatureResultBody> {
        let count = match self {
            Self::WithoutRepeatedFields(bodies) => bodies.len(),
            Self::WithRepeatedFields { rest, .. } => 1 + rest.len(),
        };
        (0..count).map(move |index| match self {
            Self::WithoutRepeatedFields(bodies) => &bodies[index],
            Self::WithRepeatedFields { first, .. } if index == 0 => &first.0,
            Self::WithRepeatedFields { rest, .. } => &rest[index - 1].0,
        })
    }
}

/// One Base Feature reference value, its location, and its six-byte field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesignBaseFeatureEntry<T> {
    pub value: T,
    pub offset: u64,
    pub field: [u8; 6],
}

/// Wire form of the legacy class-452/class-262 Base Feature body-reference
/// envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DesignBaseFeatureBodyReferenceFormWire {
    /// One output body with 64-bit references in the legacy compact lanes.
    CompactOneBody,
    /// Two output bodies with counted 32-bit reference runs.
    ExpandedTwoBody,
}

/// Typed construction data carried by a Fusion direct-modeling Base Feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// Untagged: required field sets are disjoint across variants.
#[serde(untagged)]
enum DesignBaseFeatureConstructionWire {
    /// Counted body, passive-reference, metadata, and result runs.
    ResultBodies {
        /// Ordered Design body entity suffixes exposed by the Base Feature.
        body_entity_suffixes: Vec<u64>,
        /// Byte offsets parallel to `body_entity_suffixes`.
        body_entity_suffix_offsets: Vec<u64>,
        /// Six-byte source fields parallel to `body_entity_suffixes`.
        body_entity_fields: Vec<[u8; 6]>,
        /// Ordered passive body-reference records parallel to the body suffixes.
        body_reference_records: Vec<u32>,
        /// Byte offsets parallel to `body_reference_records`.
        body_reference_record_offsets: Vec<u64>,
        /// Six-byte source fields parallel to `body_reference_records`.
        body_reference_fields: Vec<[u8; 6]>,
        /// Six-byte source fields in the repeated passive-reference run.
        repeated_reference_fields: Vec<[u8; 6]>,
        /// Shared passive-reference metadata record.
        metadata_record: u32,
        /// Byte offset of `metadata_record`.
        metadata_record_offset: u64,
        /// Variant-width source field following `metadata_record`.
        metadata_field: Vec<u8>,
        /// Ordered result-body join records parallel to the body suffixes.
        result_records: Vec<u32>,
        /// Byte offsets parallel to `result_records`.
        result_record_offsets: Vec<u64>,
        /// Six-byte source fields parallel to `result_records`.
        result_fields: Vec<[u8; 6]>,
    },
    /// Direct-modeling body-reference envelope used by the class-365/class-262 and
    /// class-377/class-259 forms.
    BodyBasedOnFaces {
        /// The Design body entity suffix exposed by the envelope.
        body_entity_suffixes: Vec<u64>,
        /// Byte offsets parallel to `body_entity_suffixes`.
        body_entity_suffix_offsets: Vec<u64>,
        /// Body suffixes used by history-to-BREP resolution for this form.
        body_reference_records: Vec<u32>,
        /// Byte offsets parallel to `body_reference_records`.
        body_reference_record_offsets: Vec<u64>,
        /// PM body-reference record named by the fixed envelope lane.
        parameter_body_record: u32,
        /// Byte offset of `parameter_body_record`.
        parameter_body_record_offset: u64,
        /// Auxiliary record named by the fixed envelope lane.
        auxiliary_record: u32,
        /// Byte offset of `auxiliary_record`.
        auxiliary_record_offset: u64,
        /// LP-UTF-16 GUID carried by the envelope.
        envelope_guid: DesignRelaxedGuidText,
        /// Byte offset of the first code unit of `envelope_guid`.
        envelope_guid_offset: u64,
        /// Stored body-source property value.
        tag_body_based_on_faces: bool,
        /// Byte offset of `tag_body_based_on_faces`.
        tag_body_based_on_faces_offset: u64,
    },
    /// Legacy body-reference envelope used by the class-452/class-262 forms.
    LegacyBodyBasedOnFaces {
        /// Compact one-body or expanded two-body source envelope form.
        form: DesignBaseFeatureBodyReferenceFormWire,
        /// Compact-form mode byte. The expanded form has no mode byte.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<u8>,
        /// Byte offset of the compact-form mode byte.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode_offset: Option<u64>,
        /// Ordered Design body entity suffixes exposed by the envelope.
        body_entity_suffixes: Vec<u64>,
        /// Byte offsets parallel to `body_entity_suffixes`.
        body_entity_suffix_offsets: Vec<u64>,
        /// Six-byte source fields parallel to `body_entity_suffixes`.
        body_entity_fields: Vec<[u8; 6]>,
        /// Body suffixes used by history-to-BREP resolution for this form.
        body_reference_records: Vec<u32>,
        /// Byte offsets parallel to `body_reference_records`.
        body_reference_record_offsets: Vec<u64>,
        /// Ordered PM body-reference records carried by the envelope.
        parameter_body_records: Vec<u64>,
        /// Byte offsets parallel to `parameter_body_records`.
        parameter_body_record_offsets: Vec<u64>,
        /// Ordered DM body-reference records carried by the envelope.
        auxiliary_records: Vec<u64>,
        /// Byte offsets parallel to `auxiliary_records`.
        auxiliary_record_offsets: Vec<u64>,
        /// Scope record repeated by the envelope's explicit scope-reference lane.
        scope_reference: u64,
        /// Byte offset of `scope_reference`.
        scope_reference_offset: u64,
        /// LP-UTF-16 GUID carried by the envelope.
        envelope_guid: DesignRelaxedGuidText,
        /// Byte offset of the first code unit of `envelope_guid`.
        envelope_guid_offset: u64,
        /// Stored body-source property value.
        tag_body_based_on_faces: bool,
        /// Byte offset of `tag_body_based_on_faces`.
        tag_body_based_on_faces_offset: u64,
    },
    /// Body snapshot form used by the class-314/class-259 scope pair.
    BodySnapshot {
        /// Ordered Design body entity suffixes exposed by the snapshot.
        body_entity_suffixes: Vec<u64>,
        /// Byte offsets parallel to `body_entity_suffixes`.
        body_entity_suffix_offsets: Vec<u64>,
        /// Six-byte source fields parallel to `body_entity_suffixes`.
        body_entity_fields: Vec<[u8; 6]>,
        /// Three LP-UTF-16 source GUIDs carried by the snapshot envelope.
        related_guids: [DesignRelaxedGuidText; 3],
        /// Byte offsets of the first code unit of each related GUID.
        related_guid_offsets: [u64; 3],
        /// Indexed record carried by the snapshot linkage tail.
        linkage_record: u32,
        /// Byte offset of `linkage_record`.
        linkage_record_offset: u64,
        /// Auxiliary indexed record carried by the snapshot linkage tail.
        auxiliary_record: u32,
        /// Byte offset of `auxiliary_record`.
        auxiliary_record_offset: u64,
    },
}

impl TryFrom<DesignBaseFeatureConstructionWire> for DesignBaseFeatureConstruction {
    type Error = String;
    // Output cardinalities are bounded by already-materialized input vectors.
    #[allow(clippy::disallowed_methods)]
    fn try_from(wire: DesignBaseFeatureConstructionWire) -> Result<Self, Self::Error> {
        Ok(match wire {
            DesignBaseFeatureConstructionWire::ResultBodies {
                body_entity_suffixes,
                body_entity_suffix_offsets,
                body_entity_fields,
                body_reference_records,
                body_reference_record_offsets,
                body_reference_fields,
                repeated_reference_fields,
                metadata_record,
                metadata_record_offset,
                metadata_field,
                result_records,
                result_record_offsets,
                result_fields,
            } => {
                let count = body_entity_suffixes.len();
                for (field, len) in [
                    (
                        "body_entity_suffix_offsets",
                        body_entity_suffix_offsets.len(),
                    ),
                    ("body_entity_fields", body_entity_fields.len()),
                    ("body_reference_records", body_reference_records.len()),
                    (
                        "body_reference_record_offsets",
                        body_reference_record_offsets.len(),
                    ),
                    ("body_reference_fields", body_reference_fields.len()),
                    ("result_records", result_records.len()),
                    ("result_record_offsets", result_record_offsets.len()),
                    ("result_fields", result_fields.len()),
                ] {
                    if len != count {
                        return Err(format!(
                            "{field} must have the same length as body_entity_suffixes"
                        ));
                    }
                }
                if !repeated_reference_fields.is_empty() && repeated_reference_fields.len() != count
                {
                    return Err("repeated_reference_fields must be empty or have the same length as body_entity_suffixes".into());
                }
                let bodies = (0..count).map(|index| DesignBaseFeatureResultBody {
                    entity: DesignBaseFeatureEntry {
                        value: body_entity_suffixes[index],
                        offset: body_entity_suffix_offsets[index],
                        field: body_entity_fields[index],
                    },
                    reference: DesignBaseFeatureEntry {
                        value: body_reference_records[index],
                        offset: body_reference_record_offsets[index],
                        field: body_reference_fields[index],
                    },
                    result: DesignBaseFeatureEntry {
                        value: result_records[index],
                        offset: result_record_offsets[index],
                        field: result_fields[index],
                    },
                });
                let bodies = if repeated_reference_fields.is_empty() {
                    DesignBaseFeatureResults::WithoutRepeatedFields(bodies.collect())
                } else {
                    let mut repeated = bodies.zip(repeated_reference_fields);
                    match repeated.next() {
                        Some(first) => DesignBaseFeatureResults::WithRepeatedFields {
                            first,
                            rest: repeated.collect(),
                        },
                        None => {
                            return Err(
                                "repeated_reference_fields require body_entity_suffixes".into()
                            )
                        }
                    }
                };
                Self::ResultBodies {
                    bodies,
                    metadata_record,
                    metadata_record_offset,
                    metadata_field,
                }
            }
            DesignBaseFeatureConstructionWire::BodyBasedOnFaces {
                body_entity_suffixes,
                body_entity_suffix_offsets,
                body_reference_records,
                body_reference_record_offsets,
                parameter_body_record,
                parameter_body_record_offset,
                auxiliary_record,
                auxiliary_record_offset,
                envelope_guid,
                envelope_guid_offset,
                tag_body_based_on_faces,
                tag_body_based_on_faces_offset,
            } => {
                let ([suffix], [offset], [reference], [reference_offset]) = (
                    body_entity_suffixes.as_slice(),
                    body_entity_suffix_offsets.as_slice(),
                    body_reference_records.as_slice(),
                    body_reference_record_offsets.as_slice(),
                ) else {
                    return Err("body_entity_suffixes, body_entity_suffix_offsets, body_reference_records, and body_reference_record_offsets require one body".into());
                };
                if *suffix != u64::from(*reference) || offset != reference_offset {
                    return Err("body_reference_records and body_reference_record_offsets must match body_entity_suffixes and body_entity_suffix_offsets".into());
                }
                if !tag_body_based_on_faces {
                    return Err("tag_body_based_on_faces must be true for BodyBasedOnFaces".into());
                }
                Self::BodyBasedOnFaces {
                    body: Located {
                        value: *reference,
                        offset: *offset,
                    },
                    parameter_body_record,
                    parameter_body_record_offset,
                    auxiliary_record,
                    auxiliary_record_offset,
                    envelope_guid,
                    envelope_guid_offset,
                    tag_body_based_on_faces_offset,
                }
            }
            DesignBaseFeatureConstructionWire::LegacyBodyBasedOnFaces {
                form,
                mode,
                mode_offset,
                body_entity_suffixes,
                body_entity_suffix_offsets,
                body_entity_fields,
                body_reference_records,
                body_reference_record_offsets,
                parameter_body_records,
                parameter_body_record_offsets,
                auxiliary_records,
                auxiliary_record_offsets,
                scope_reference,
                scope_reference_offset,
                envelope_guid,
                envelope_guid_offset,
                tag_body_based_on_faces,
                tag_body_based_on_faces_offset,
            } => {
                let count = body_entity_suffixes.len();
                for (field, len) in [
                    (
                        "body_entity_suffix_offsets",
                        body_entity_suffix_offsets.len(),
                    ),
                    ("body_entity_fields", body_entity_fields.len()),
                    ("body_reference_records", body_reference_records.len()),
                    (
                        "body_reference_record_offsets",
                        body_reference_record_offsets.len(),
                    ),
                    ("parameter_body_records", parameter_body_records.len()),
                    (
                        "parameter_body_record_offsets",
                        parameter_body_record_offsets.len(),
                    ),
                    ("auxiliary_records", auxiliary_records.len()),
                    ("auxiliary_record_offsets", auxiliary_record_offsets.len()),
                ] {
                    if len != count {
                        return Err(format!(
                            "{field} must have the same length as body_entity_suffixes"
                        ));
                    }
                }
                if !tag_body_based_on_faces {
                    return Err(
                        "tag_body_based_on_faces must be true for LegacyBodyBasedOnFaces".into(),
                    );
                }
                let mut bodies = Vec::with_capacity(count);
                for index in 0..count {
                    if body_entity_suffixes[index] != u64::from(body_reference_records[index])
                        || body_entity_suffix_offsets[index] != body_reference_record_offsets[index]
                    {
                        return Err("body_reference_records and body_reference_record_offsets must match body_entity_suffixes and body_entity_suffix_offsets".into());
                    }
                    bodies.push(DesignLegacyBaseFeatureBody {
                        entity: DesignBaseFeatureEntry {
                            value: body_reference_records[index],
                            offset: body_entity_suffix_offsets[index],
                            field: body_entity_fields[index],
                        },
                        parameter_body: Located {
                            value: parameter_body_records[index],
                            offset: parameter_body_record_offsets[index],
                        },
                        auxiliary: Located {
                            value: auxiliary_records[index],
                            offset: auxiliary_record_offsets[index],
                        },
                    });
                }
                let form = match (form, mode, mode_offset, bodies.as_slice()) {
                    (DesignBaseFeatureBodyReferenceFormWire::CompactOneBody, Some(value), Some(offset), [body]) => DesignBaseFeatureBodyReferenceForm::CompactOneBody { mode: Located { value: DesignBaseFeatureCompactMode::try_from(value)?, offset }, body: *body },
                    (DesignBaseFeatureBodyReferenceFormWire::ExpandedTwoBody, None, None, [first, second]) => DesignBaseFeatureBodyReferenceForm::ExpandedTwoBody { bodies: [*first, *second] },
                    _ => return Err("form requires one body with mode and mode_offset for compact_one_body, or two bodies without mode for expanded_two_body".into()),
                };
                Self::LegacyBodyBasedOnFaces {
                    form,
                    scope_reference,
                    scope_reference_offset,
                    envelope_guid,
                    envelope_guid_offset,
                    tag_body_based_on_faces_offset,
                }
            }
            DesignBaseFeatureConstructionWire::BodySnapshot {
                body_entity_suffixes,
                body_entity_suffix_offsets,
                body_entity_fields,
                related_guids,
                related_guid_offsets,
                linkage_record,
                linkage_record_offset,
                auxiliary_record,
                auxiliary_record_offset,
            } => {
                if body_entity_suffixes.len() != body_entity_suffix_offsets.len()
                    || body_entity_suffixes.len() != body_entity_fields.len()
                {
                    return Err("body_entity_suffixes, body_entity_suffix_offsets, and body_entity_fields must have equal lengths".into());
                }
                let bodies = body_entity_suffixes
                    .into_iter()
                    .zip(body_entity_suffix_offsets)
                    .zip(body_entity_fields)
                    .map(|((value, offset), field)| DesignBaseFeatureEntry {
                        value,
                        offset,
                        field,
                    })
                    .collect();
                Self::BodySnapshot {
                    bodies,
                    related_guids,
                    related_guid_offsets,
                    linkage_record,
                    linkage_record_offset,
                    auxiliary_record,
                    auxiliary_record_offset,
                }
            }
        })
    }
}

impl From<DesignBaseFeatureConstruction> for DesignBaseFeatureConstructionWire {
    // Output cardinalities are bounded by already-materialized input vectors.
    #[allow(clippy::disallowed_methods)]
    fn from(value: DesignBaseFeatureConstruction) -> Self {
        match value {
            DesignBaseFeatureConstruction::ResultBodies {
                bodies,
                metadata_record,
                metadata_record_offset,
                metadata_field,
            } => {
                let body_entity_suffixes = bodies.iter().map(|body| body.entity.value).collect();
                let body_entity_suffix_offsets =
                    bodies.iter().map(|body| body.entity.offset).collect();
                let body_entity_fields = bodies.iter().map(|body| body.entity.field).collect();
                let body_reference_records =
                    bodies.iter().map(|body| body.reference.value).collect();
                let body_reference_record_offsets =
                    bodies.iter().map(|body| body.reference.offset).collect();
                let body_reference_fields =
                    bodies.iter().map(|body| body.reference.field).collect();
                let result_records = bodies.iter().map(|body| body.result.value).collect();
                let result_record_offsets = bodies.iter().map(|body| body.result.offset).collect();
                let result_fields = bodies.iter().map(|body| body.result.field).collect();
                let repeated_reference_fields = match bodies {
                    DesignBaseFeatureResults::WithoutRepeatedFields(_) => Vec::new(),
                    DesignBaseFeatureResults::WithRepeatedFields { first, rest } => {
                        std::iter::once(first.1)
                            .chain(rest.into_iter().map(|(_, field)| field))
                            .collect()
                    }
                };
                Self::ResultBodies {
                    body_entity_suffixes,
                    body_entity_suffix_offsets,
                    body_entity_fields,
                    body_reference_records,
                    body_reference_record_offsets,
                    body_reference_fields,
                    repeated_reference_fields,
                    metadata_record,
                    metadata_record_offset,
                    metadata_field,
                    result_records,
                    result_record_offsets,
                    result_fields,
                }
            }
            DesignBaseFeatureConstruction::BodyBasedOnFaces {
                body,
                parameter_body_record,
                parameter_body_record_offset,
                auxiliary_record,
                auxiliary_record_offset,
                envelope_guid,
                envelope_guid_offset,
                tag_body_based_on_faces_offset,
            } => Self::BodyBasedOnFaces {
                body_entity_suffixes: vec![u64::from(body.value)],
                body_entity_suffix_offsets: vec![body.offset],
                body_reference_records: vec![body.value],
                body_reference_record_offsets: vec![body.offset],
                tag_body_based_on_faces: true,
                parameter_body_record,
                parameter_body_record_offset,
                auxiliary_record,
                auxiliary_record_offset,
                envelope_guid,
                envelope_guid_offset,
                tag_body_based_on_faces_offset,
            },
            DesignBaseFeatureConstruction::LegacyBodyBasedOnFaces {
                form,
                scope_reference,
                scope_reference_offset,
                envelope_guid,
                envelope_guid_offset,
                tag_body_based_on_faces_offset,
            } => {
                let bodies = form.bodies();
                let body_entity_suffixes = bodies
                    .iter()
                    .map(|body| u64::from(body.entity.value))
                    .collect();
                let body_entity_suffix_offsets =
                    bodies.iter().map(|body| body.entity.offset).collect();
                let body_entity_fields = bodies.iter().map(|body| body.entity.field).collect();
                let body_reference_records = bodies.iter().map(|body| body.entity.value).collect();
                let body_reference_record_offsets =
                    bodies.iter().map(|body| body.entity.offset).collect();
                let parameter_body_records = bodies
                    .iter()
                    .map(|body| body.parameter_body.value)
                    .collect();
                let parameter_body_record_offsets = bodies
                    .iter()
                    .map(|body| body.parameter_body.offset)
                    .collect();
                let auxiliary_records = bodies.iter().map(|body| body.auxiliary.value).collect();
                let auxiliary_record_offsets =
                    bodies.iter().map(|body| body.auxiliary.offset).collect();
                let (form, mode, mode_offset) = match form {
                    DesignBaseFeatureBodyReferenceForm::CompactOneBody { mode, .. } => (
                        DesignBaseFeatureBodyReferenceFormWire::CompactOneBody,
                        Some(mode.value as u8),
                        Some(mode.offset),
                    ),
                    DesignBaseFeatureBodyReferenceForm::ExpandedTwoBody { .. } => (
                        DesignBaseFeatureBodyReferenceFormWire::ExpandedTwoBody,
                        None,
                        None,
                    ),
                };
                Self::LegacyBodyBasedOnFaces {
                    form,
                    mode,
                    mode_offset,
                    body_entity_suffixes,
                    body_entity_suffix_offsets,
                    body_entity_fields,
                    body_reference_records,
                    body_reference_record_offsets,
                    parameter_body_records,
                    parameter_body_record_offsets,
                    auxiliary_records,
                    auxiliary_record_offsets,
                    scope_reference,
                    scope_reference_offset,
                    envelope_guid,
                    envelope_guid_offset,
                    tag_body_based_on_faces: true,
                    tag_body_based_on_faces_offset,
                }
            }
            DesignBaseFeatureConstruction::BodySnapshot {
                bodies,
                related_guids,
                related_guid_offsets,
                linkage_record,
                linkage_record_offset,
                auxiliary_record,
                auxiliary_record_offset,
            } => {
                let mut body_entity_suffixes = Vec::with_capacity(bodies.len());
                let mut body_entity_suffix_offsets = Vec::with_capacity(bodies.len());
                let mut body_entity_fields = Vec::with_capacity(bodies.len());
                for body in bodies {
                    body_entity_suffixes.push(body.value);
                    body_entity_suffix_offsets.push(body.offset);
                    body_entity_fields.push(body.field);
                }
                Self::BodySnapshot {
                    body_entity_suffixes,
                    body_entity_suffix_offsets,
                    body_entity_fields,
                    related_guids,
                    related_guid_offsets,
                    linkage_record,
                    linkage_record_offset,
                    auxiliary_record,
                    auxiliary_record_offset,
                }
            }
        }
    }
}

impl DesignBaseFeatureConstruction {
    /// Return the body suffixes in source order for any Base Feature form.
    pub(crate) fn body_entity_suffixes(&self) -> impl ExactSizeIterator<Item = u64> + '_ {
        let count = match self {
            Self::BodySnapshot { bodies, .. } => bodies.len(),
            Self::BodyBasedOnFaces { .. } => 1,
            Self::ResultBodies { bodies, .. } => bodies.iter().len(),
            Self::LegacyBodyBasedOnFaces { form, .. } => form.bodies().len(),
        };
        (0..count).map(move |index| match self {
            Self::BodySnapshot { bodies, .. } => bodies[index].value,
            Self::BodyBasedOnFaces { body, .. } => u64::from(body.value),
            Self::ResultBodies { bodies, .. } => match bodies {
                DesignBaseFeatureResults::WithoutRepeatedFields(bodies) => {
                    bodies[index].entity.value
                }
                DesignBaseFeatureResults::WithRepeatedFields { first, .. } if index == 0 => {
                    first.0.entity.value
                }
                DesignBaseFeatureResults::WithRepeatedFields { rest, .. } => {
                    rest[index - 1].0.entity.value
                }
            },
            Self::LegacyBodyBasedOnFaces { form, .. } => {
                u64::from(form.bodies()[index].entity.value)
            }
        })
    }

    /// Return passive body-reference records for forms that carry them.
    pub(crate) fn body_reference_records(&self) -> impl Iterator<Item = u32> + '_ {
        let (results, legacy, single) = match self {
            Self::ResultBodies { bodies, .. } => (Some(bodies), &[][..], None),
            Self::LegacyBodyBasedOnFaces { form, .. } => (None, form.bodies(), None),
            Self::BodyBasedOnFaces { body, .. } => (None, &[][..], Some(body.value)),
            Self::BodySnapshot { .. } => (None, &[][..], None),
        };
        results
            .into_iter()
            .flat_map(DesignBaseFeatureResults::iter)
            .map(|body| body.reference.value)
            .chain(legacy.iter().map(|body| body.entity.value))
            .chain(single)
    }
}
