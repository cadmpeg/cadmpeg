// SPDX-License-Identifier: Apache-2.0
//! Datum-plane common headers and atomic construction references.

use super::{
    feature_input_blocks, unique_offset_data_block, visit_feature_history_operation_records,
    DatumPlaneBlockLane, FeatureIndexToken, FeatureResolvedIndexToken,
};
use crate::container::Container;
use crate::om::DatumPlanePayloadHeader;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// A retained common header with an optional decoded construction branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureDatumPlaneHeader {
    pub id: String,
    pub operation_label: String,
    header: DatumPlanePayloadHeader,
    branch: Option<ReferenceBranch>,
    pub source_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Branch<T> {
    Single { descriptor: T, object: T },
    Double { objects: [T; 2] },
}

impl<T> Branch<T> {
    fn references(&self, lane: DatumPlaneBlockLane) -> &[T] {
        match (self, lane) {
            (Self::Single { descriptor, .. }, DatumPlaneBlockLane::Descriptor) => {
                std::slice::from_ref(descriptor)
            }
            (Self::Single { object, .. }, DatumPlaneBlockLane::Object) => {
                std::slice::from_ref(object)
            }
            (Self::Double { .. }, DatumPlaneBlockLane::Descriptor) => &[],
            (Self::Double { objects }, DatumPlaneBlockLane::Object) => objects,
        }
    }

    fn try_map<U>(&self, mut map: impl FnMut(&T) -> Option<U>) -> Option<Branch<U>> {
        Some(match self {
            Self::Single { descriptor, object } => Branch::Single {
                descriptor: map(descriptor)?,
                object: map(object)?,
            },
            Self::Double {
                objects: [first, second],
            } => Branch::Double {
                objects: [map(first)?, map(second)?],
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReferenceBranch {
    Unresolved(Branch<FeatureIndexToken>),
    Resolved(Branch<FeatureResolvedIndexToken>),
}

impl FeatureDatumPlaneHeader {
    pub(super) fn resolved_references(
        &self,
        lane: DatumPlaneBlockLane,
    ) -> &[FeatureResolvedIndexToken] {
        match &self.branch {
            Some(ReferenceBranch::Resolved(branch)) => branch.references(lane),
            Some(ReferenceBranch::Unresolved(_)) | None => &[],
        }
    }
}

/// Decode common datum-plane payload headers from feature-history records.
pub(crate) fn feature_datum_plane_headers(container: &Container) -> Vec<FeatureDatumPlaneHeader> {
    let indexed = container.indexed_om_sections();
    let inputs = feature_input_blocks(container);
    let mut headers = Vec::new();
    visit_feature_history_operation_records(
        container,
        |_section, section_key, entry_offset, operation_ordinal, record| {
            let Some(header) = crate::om::datum_plane_payload_header(record) else {
                return;
            };
            let branch = crate::om::datum_plane_descriptor_reference_branch(record)
                .map(|branch| Branch::Single {
                    descriptor: FeatureIndexToken {
                        value: branch.descriptor_index,
                        raw: branch.raw_descriptor_index,
                        source_offset: entry_offset + branch.descriptor_offset as u64,
                    },
                    object: FeatureIndexToken {
                        value: branch.object_index,
                        raw: branch.raw_object_index,
                        source_offset: entry_offset + branch.object_offset as u64,
                    },
                })
                .or_else(|| {
                    crate::om::datum_plane_double_reference_branch(record).map(|branch| {
                        Branch::Double {
                            objects: branch.references.map(|reference| FeatureIndexToken {
                                value: reference.object_index,
                                raw: reference.raw_object_index,
                                source_offset: entry_offset + reference.offset as u64,
                            }),
                        }
                    })
                });
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            let input_prefixes = inputs
                .iter()
                .filter(|input| input.operation_label == operation_label)
                .filter_map(|input| {
                    input
                        .data_block
                        .rsplit_once(":block#")
                        .map(|(prefix, _)| prefix)
                })
                .collect::<BTreeSet<_>>();
            let branch = branch.map(|branch| {
                let resolved = (input_prefixes.len() == 1).then_some(()).and_then(|()| {
                    let input_prefix = *input_prefixes.iter().next()?;
                    branch.try_map(|token| {
                        let data_block = unique_offset_data_block(&indexed, token.value)?;
                        data_block
                            .rsplit_once(":block#")
                            .is_some_and(|(prefix, _)| prefix == input_prefix)
                            .then(|| FeatureResolvedIndexToken {
                                token: token.clone(),
                                data_block,
                            })
                    })
                });
                resolved.map_or_else(
                    || ReferenceBranch::Unresolved(branch),
                    ReferenceBranch::Resolved,
                )
            });
            headers.push(FeatureDatumPlaneHeader {
                id: format!(
                    "nx:feature-history:datum-plane-header#{section_key}-{operation_ordinal:010}"
                ),
                operation_label,
                header,
                branch,
                source_offset: entry_offset + record.payload_offset as u64,
            });
        },
    );
    headers
}

#[derive(Serialize, Deserialize)]
struct HeaderWire {
    /// Globally unique header identity.
    id: String,
    /// Owning `DATUM_PLANE` operation label.
    operation_label: String,
    /// Payload control byte.
    control: u8,
    /// Declared construction count.
    declared_count: u8,
    /// Tag selecting the following construction branch.
    branch_tag: u8,
    /// Ordered compact descriptor indices carried by the selected branch.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    descriptor_indices: Vec<u32>,
    /// Exact compact descriptor-index tokens in branch order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    raw_descriptor_indices: Vec<Vec<u8>>,
    /// Ordered canonical object indices carried by the selected branch.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    object_indices: Vec<u32>,
    /// Exact canonical object-index tokens in branch order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    raw_object_indices: Vec<Vec<u8>>,
    /// Atomically resolved same-store descriptor blocks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    descriptor_data_blocks: Vec<String>,
    /// Atomically resolved same-store canonical object blocks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    object_data_blocks: Vec<String>,
    /// Absolute offsets of compact descriptor indices.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    descriptor_source_offsets: Vec<u64>,
    /// Absolute offsets of canonical object-index markers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    object_source_offsets: Vec<u64>,
    /// Absolute offset of the payload control byte.
    source_offset: u64,
}

impl Serialize for FeatureDatumPlaneHeader {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = HeaderWire {
            id: self.id.clone(),
            operation_label: self.operation_label.clone(),
            control: self.header.control,
            declared_count: self.header.declared_count,
            branch_tag: self.header.branch_tag,
            descriptor_indices: Vec::new(),
            raw_descriptor_indices: Vec::new(),
            object_indices: Vec::new(),
            raw_object_indices: Vec::new(),
            descriptor_data_blocks: Vec::new(),
            object_data_blocks: Vec::new(),
            descriptor_source_offsets: Vec::new(),
            object_source_offsets: Vec::new(),
            source_offset: self.source_offset,
        };
        for lane in [DatumPlaneBlockLane::Descriptor, DatumPlaneBlockLane::Object] {
            let (indices, raw, offsets, blocks) = match lane {
                DatumPlaneBlockLane::Descriptor => (
                    &mut wire.descriptor_indices,
                    &mut wire.raw_descriptor_indices,
                    &mut wire.descriptor_source_offsets,
                    &mut wire.descriptor_data_blocks,
                ),
                DatumPlaneBlockLane::Object => (
                    &mut wire.object_indices,
                    &mut wire.raw_object_indices,
                    &mut wire.object_source_offsets,
                    &mut wire.object_data_blocks,
                ),
            };
            match &self.branch {
                Some(ReferenceBranch::Unresolved(branch)) => {
                    for token in branch.references(lane) {
                        indices.push(token.value);
                        raw.push(token.raw.clone());
                        offsets.push(token.source_offset);
                    }
                }
                Some(ReferenceBranch::Resolved(branch)) => {
                    for reference in branch.references(lane) {
                        indices.push(reference.token.value);
                        raw.push(reference.token.raw.clone());
                        offsets.push(reference.token.source_offset);
                        blocks.push(reference.data_block.clone());
                    }
                }
                None => {}
            }
        }
        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for FeatureDatumPlaneHeader {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = HeaderWire::deserialize(deserializer)?;
        let tokens = |indices: Vec<u32>, raw: Vec<Vec<u8>>, offsets: Vec<u64>| {
            if indices.len() != raw.len() || indices.len() != offsets.len() {
                return Err(serde::de::Error::custom(
                    "datum-plane token columns differ in length",
                ));
            }
            Ok(indices
                .into_iter()
                .zip(raw)
                .zip(offsets)
                .map(|((value, raw), source_offset)| FeatureIndexToken {
                    value,
                    raw,
                    source_offset,
                })
                .collect::<Vec<_>>())
        };
        let descriptors = tokens(
            wire.descriptor_indices,
            wire.raw_descriptor_indices,
            wire.descriptor_source_offsets,
        )?;
        let objects = tokens(
            wire.object_indices,
            wire.raw_object_indices,
            wire.object_source_offsets,
        )?;
        let branch = match (descriptors.as_slice(), objects.as_slice()) {
            ([], []) => None,
            ([descriptor], [object])
                if matches!(
                    (wire.declared_count, wire.branch_tag),
                    (2, 0x1b | 0x23) | (3, 0x28)
                ) =>
            {
                Some(Branch::Single {
                    descriptor: descriptor.clone(),
                    object: object.clone(),
                })
            }
            ([], [first, second])
                if matches!((wire.declared_count, wire.branch_tag), (2 | 3, 0x29)) =>
            {
                Some(Branch::Double {
                    objects: [first.clone(), second.clone()],
                })
            }
            _ => {
                return Err(serde::de::Error::custom(
                    "datum-plane references do not match the construction branch",
                ))
            }
        };
        let branch = if wire.descriptor_data_blocks.is_empty() && wire.object_data_blocks.is_empty()
        {
            branch.map(ReferenceBranch::Unresolved)
        } else {
            match branch {
                Some(Branch::Single { descriptor, object }) => {
                    let [descriptor_block]: [String; 1] =
                        wire.descriptor_data_blocks.try_into().map_err(|_| {
                            serde::de::Error::custom(
                                "single datum-plane branch requires one resolved descriptor",
                            )
                        })?;
                    let [object_block]: [String; 1] =
                        wire.object_data_blocks.try_into().map_err(|_| {
                            serde::de::Error::custom(
                                "single datum-plane branch requires one resolved object",
                            )
                        })?;
                    Some(ReferenceBranch::Resolved(Branch::Single {
                        descriptor: FeatureResolvedIndexToken {
                            token: descriptor,
                            data_block: descriptor_block,
                        },
                        object: FeatureResolvedIndexToken {
                            token: object,
                            data_block: object_block,
                        },
                    }))
                }
                Some(Branch::Double {
                    objects: [first, second],
                }) if wire.descriptor_data_blocks.is_empty() => {
                    let [first_block, second_block]: [String; 2] =
                        wire.object_data_blocks.try_into().map_err(|_| {
                            serde::de::Error::custom(
                                "double datum-plane branch requires two resolved objects",
                            )
                        })?;
                    Some(ReferenceBranch::Resolved(Branch::Double {
                        objects: [
                            FeatureResolvedIndexToken {
                                token: first,
                                data_block: first_block,
                            },
                            FeatureResolvedIndexToken {
                                token: second,
                                data_block: second_block,
                            },
                        ],
                    }))
                }
                Some(Branch::Double { .. }) | None => {
                    return Err(serde::de::Error::custom(
                        "datum-plane blocks have no matching references",
                    ))
                }
            }
        };
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            header: DatumPlanePayloadHeader {
                control: wire.control,
                declared_count: wire.declared_count,
                branch_tag: wire.branch_tag,
            },
            branch,
            source_offset: wire.source_offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::FeatureDatumPlaneHeader;

    #[test]
    fn header_only_and_resolved_branches_preserve_column_wire() {
        for json in [
            r#"{"id":"header","operation_label":"operation","control":1,"declared_count":2,"branch_tag":255,"source_offset":10}"#,
            r#"{"id":"header","operation_label":"operation","control":1,"declared_count":2,"branch_tag":27,"source_offset":10}"#,
            r#"{"id":"header","operation_label":"operation","control":1,"declared_count":2,"branch_tag":27,"descriptor_indices":[3],"raw_descriptor_indices":[[3]],"object_indices":[4],"raw_object_indices":[[4]],"descriptor_data_blocks":["descriptor"],"object_data_blocks":["object"],"descriptor_source_offsets":[20],"object_source_offsets":[21],"source_offset":10}"#,
            r#"{"id":"header","operation_label":"operation","control":1,"declared_count":3,"branch_tag":41,"object_indices":[3,4],"raw_object_indices":[[3],[4]],"object_data_blocks":["first","second"],"object_source_offsets":[20,21],"source_offset":10}"#,
            r#"{"id":"header","operation_label":"operation","control":1,"declared_count":3,"branch_tag":40,"descriptor_indices":[3],"raw_descriptor_indices":[[3]],"object_indices":[4],"raw_object_indices":[[4]],"descriptor_source_offsets":[20],"object_source_offsets":[21],"source_offset":10}"#,
        ] {
            let header: FeatureDatumPlaneHeader = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&header).unwrap(), json);
        }
    }

    #[test]
    fn wire_requires_complete_branch_tokens_and_atomic_resolution() {
        let json = r#"{"id":"header","operation_label":"operation","control":1,"declared_count":2,"branch_tag":27,"descriptor_indices":[3],"raw_descriptor_indices":[[3]],"object_indices":[4],"raw_object_indices":[[4]],"descriptor_data_blocks":["descriptor"],"object_data_blocks":["object"],"descriptor_source_offsets":[20],"object_source_offsets":[21],"source_offset":10}"#;
        for column in [
            "descriptor_indices",
            "raw_descriptor_indices",
            "object_indices",
            "raw_object_indices",
            "descriptor_data_blocks",
            "object_data_blocks",
            "descriptor_source_offsets",
            "object_source_offsets",
        ] {
            let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
            wire[column].as_array_mut().unwrap().clear();
            assert!(
                serde_json::from_value::<FeatureDatumPlaneHeader>(wire).is_err(),
                "{column}"
            );
        }
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire["branch_tag"] = 41.into();
        assert!(serde_json::from_value::<FeatureDatumPlaneHeader>(wire).is_err());
    }
}
