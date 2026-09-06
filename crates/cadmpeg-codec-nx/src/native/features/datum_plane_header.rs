// SPDX-License-Identifier: Apache-2.0
//! Datum-plane common headers and atomic construction references.

use super::reference::ConstructionReference;
use super::{
    feature_input_blocks, unique_offset_data_block, visit_feature_history_operation_records,
    DatumPlaneBlockLane,
};
use crate::container::Container;
use crate::om::compact::CompactIndexAtom;
use crate::om::reference_index::PayloadIndexToken;
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
enum Branch<B> {
    Single {
        descriptor: ConstructionReference<B, CompactIndexAtom>,
        object: ConstructionReference<B, PayloadIndexToken>,
    },
    Double {
        objects: [ConstructionReference<B, PayloadIndexToken>; 2],
    },
}

impl<B> Branch<B> {
    fn descriptor(&self) -> Option<&ConstructionReference<B, CompactIndexAtom>> {
        match self {
            Self::Single { descriptor, .. } => Some(descriptor),
            Self::Double { .. } => None,
        }
    }

    fn objects(&self) -> &[ConstructionReference<B, PayloadIndexToken>] {
        match self {
            Self::Single { object, .. } => std::slice::from_ref(object),
            Self::Double { objects } => objects,
        }
    }

    fn write_wire(&self, wire: &mut HeaderWire, block: impl Fn(&B) -> Option<String>) {
        if let Some(reference) = self.descriptor() {
            wire.descriptor_indices.push(reference.token.value());
            wire.raw_descriptor_indices
                .push(reference.token.raw().to_vec());
            wire.descriptor_source_offsets.push(reference.source_offset);
            wire.descriptor_data_blocks
                .extend(block(&reference.data_block));
        }
        for reference in self.objects() {
            wire.object_indices.push(reference.token.value());
            wire.raw_object_indices.push(reference.token.raw().to_vec());
            wire.object_source_offsets.push(reference.source_offset);
            wire.object_data_blocks.extend(block(&reference.data_block));
        }
    }
}

impl Branch<()> {
    fn resolve(&self, mut block: impl FnMut(u32) -> Option<String>) -> Option<Branch<String>> {
        let resolve_object = |reference: &ConstructionReference<(), PayloadIndexToken>,
                              data_block| {
            ConstructionReference {
                token: reference.token,
                data_block,
                source_offset: reference.source_offset,
            }
        };
        Some(match self {
            Self::Single { descriptor, object } => Branch::Single {
                descriptor: ConstructionReference {
                    token: descriptor.token,
                    data_block: block(descriptor.token.value())?,
                    source_offset: descriptor.source_offset,
                },
                object: resolve_object(object, block(object.token.value())?),
            },
            Self::Double {
                objects: [first, second],
            } => Branch::Double {
                objects: [
                    resolve_object(first, block(first.token.value())?),
                    resolve_object(second, block(second.token.value())?),
                ],
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReferenceBranch {
    Unresolved(Branch<()>),
    Resolved(Branch<String>),
}

impl FeatureDatumPlaneHeader {
    pub(super) fn resolved_data_blocks(
        &self,
        lane: DatumPlaneBlockLane,
    ) -> impl Iterator<Item = &String> {
        let blocks = match (&self.branch, lane) {
            (
                Some(ReferenceBranch::Resolved(Branch::Single { descriptor, .. })),
                DatumPlaneBlockLane::Descriptor,
            ) => [Some(&descriptor.data_block), None],
            (
                Some(ReferenceBranch::Resolved(Branch::Single { object, .. })),
                DatumPlaneBlockLane::Object,
            ) => [Some(&object.data_block), None],
            (
                Some(ReferenceBranch::Resolved(Branch::Double {
                    objects: [first, second],
                })),
                DatumPlaneBlockLane::Object,
            ) => [Some(&first.data_block), Some(&second.data_block)],
            (
                Some(ReferenceBranch::Resolved(Branch::Double { .. })),
                DatumPlaneBlockLane::Descriptor,
            )
            | (Some(ReferenceBranch::Unresolved(_)) | None, _) => [None, None],
        };
        blocks.into_iter().flatten()
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
            let Some(header) = crate::om::datum_plane_payload_header(record.payload_view()) else {
                return;
            };
            let object = |reference: crate::om::PayloadObjectReference<PayloadIndexToken>| {
                ConstructionReference {
                    token: reference.token,
                    data_block: (),
                    source_offset: entry_offset + reference.offset as u64,
                }
            };
            let branch = crate::om::datum_plane_descriptor_reference_branch(record.payload_view())
                .map(|branch| Branch::Single {
                    descriptor: ConstructionReference {
                        token: branch.descriptor.atom,
                        data_block: (),
                        source_offset: entry_offset + branch.descriptor.offset as u64,
                    },
                    object: object(branch.object),
                })
                .or_else(|| {
                    crate::om::datum_plane_double_reference_branch(record.payload_view()).map(
                        |branch| Branch::Double {
                            objects: branch.references.map(object),
                        },
                    )
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
                    branch.resolve(|index| {
                        let data_block = unique_offset_data_block(&indexed, index)?;
                        data_block
                            .rsplit_once(":block#")
                            .is_some_and(|(prefix, _)| prefix == input_prefix)
                            .then_some(data_block)
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
                source_offset: entry_offset + record.payload_offset() as u64,
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
        match &self.branch {
            Some(ReferenceBranch::Unresolved(branch)) => branch.write_wire(&mut wire, |()| None),
            Some(ReferenceBranch::Resolved(branch)) => {
                branch.write_wire(&mut wire, |block| Some(block.clone()));
            }
            None => {}
        }
        wire.serialize(serializer)
    }
}

fn wire_tokens<T>(
    indices: Vec<u32>,
    raw: Vec<Vec<u8>>,
    offsets: Vec<u64>,
    field: &str,
    read: impl Fn(u32, &[u8]) -> Result<T, &'static str>,
) -> Result<Vec<ConstructionReference<(), T>>, String> {
    if indices.len() != raw.len() || indices.len() != offsets.len() {
        return Err(format!("{field}: token columns differ in length"));
    }
    indices
        .into_iter()
        .zip(raw)
        .zip(offsets)
        .enumerate()
        .map(|(slot, ((value, raw), source_offset))| {
            Ok(ConstructionReference {
                token: read(value, &raw).map_err(|error| format!("{field}[{slot}]: {error}"))?,
                data_block: (),
                source_offset,
            })
        })
        .collect()
}

impl<'de> Deserialize<'de> for FeatureDatumPlaneHeader {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = HeaderWire::deserialize(deserializer)?;
        let descriptors = wire_tokens(
            wire.descriptor_indices,
            wire.raw_descriptor_indices,
            wire.descriptor_source_offsets,
            "descriptor_indices",
            CompactIndexAtom::from_wire,
        )
        .map_err(serde::de::Error::custom)?;
        let objects = wire_tokens(
            wire.object_indices,
            wire.raw_object_indices,
            wire.object_source_offsets,
            "object_indices",
            PayloadIndexToken::from_wire,
        )
        .map_err(serde::de::Error::custom)?;
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
                    "datum-plane references do not match declared_count/branch_tag",
                ))
            }
        };
        let branch = if wire.descriptor_data_blocks.is_empty() && wire.object_data_blocks.is_empty()
        {
            branch.map(ReferenceBranch::Unresolved)
        } else {
            let Some(branch) = branch else {
                return Err(serde::de::Error::custom(
                    "data_blocks have no matching references",
                ));
            };
            if wire.descriptor_data_blocks.len() != usize::from(branch.descriptor().is_some())
                || wire.object_data_blocks.len() != branch.objects().len()
            {
                return Err(serde::de::Error::custom(
                    "descriptor_data_blocks/object_data_blocks require complete branch resolution",
                ));
            }
            let mut blocks = wire
                .descriptor_data_blocks
                .into_iter()
                .chain(wire.object_data_blocks);
            Some(ReferenceBranch::Resolved(
                branch.resolve(|_| blocks.next()).ok_or_else(|| {
                    serde::de::Error::custom("data_blocks: incomplete branch resolution")
                })?,
            ))
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
            r#"{"id":"header","operation_label":"operation","control":1,"declared_count":2,"branch_tag":27,"descriptor_indices":[3],"raw_descriptor_indices":[[3]],"object_indices":[4],"raw_object_indices":[[240,4]],"descriptor_data_blocks":["descriptor"],"object_data_blocks":["object"],"descriptor_source_offsets":[20],"object_source_offsets":[21],"source_offset":10}"#,
            r#"{"id":"header","operation_label":"operation","control":1,"declared_count":3,"branch_tag":41,"object_indices":[3,4],"raw_object_indices":[[240,3],[240,4]],"object_data_blocks":["first","second"],"object_source_offsets":[20,21],"source_offset":10}"#,
            r#"{"id":"header","operation_label":"operation","control":1,"declared_count":3,"branch_tag":40,"descriptor_indices":[3],"raw_descriptor_indices":[[3]],"object_indices":[4],"raw_object_indices":[[240,4]],"descriptor_source_offsets":[20],"object_source_offsets":[21],"source_offset":10}"#,
        ] {
            let header: FeatureDatumPlaneHeader = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&header).unwrap(), json);
        }
    }

    #[test]
    fn wire_requires_complete_branch_tokens_and_atomic_resolution() {
        let json = r#"{"id":"header","operation_label":"operation","control":1,"declared_count":2,"branch_tag":27,"descriptor_indices":[3],"raw_descriptor_indices":[[3]],"object_indices":[4],"raw_object_indices":[[240,4]],"descriptor_data_blocks":["descriptor"],"object_data_blocks":["object"],"descriptor_source_offsets":[20],"object_source_offsets":[21],"source_offset":10}"#;
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
    #[test]
    fn branch_tokens_preserve_distinct_descriptor_and_object_grammars() {
        let json = r#"{"id":"header","operation_label":"operation","control":1,"declared_count":2,"branch_tag":27,"descriptor_indices":[4096],"raw_descriptor_indices":[[144,0]],"object_indices":[256],"raw_object_indices":[[241,1,0]],"descriptor_data_blocks":["descriptor"],"object_data_blocks":["object"],"descriptor_source_offsets":[20],"object_source_offsets":[22],"source_offset":10}"#;
        let header: FeatureDatumPlaneHeader = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&header).unwrap(), json);
        assert_eq!(
            header
                .resolved_data_blocks(super::DatumPlaneBlockLane::Descriptor)
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["descriptor"]
        );
        assert_eq!(
            header
                .resolved_data_blocks(super::DatumPlaneBlockLane::Object)
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["object"]
        );
        for (column, raw) in [
            ("raw_descriptor_indices", vec![255]),
            ("raw_descriptor_indices", vec![144]),
            ("raw_descriptor_indices", vec![144, 0, 0]),
            ("raw_descriptor_indices", vec![1]),
            ("raw_object_indices", vec![144, 1, 0]),
            ("raw_object_indices", vec![241, 1]),
            ("raw_object_indices", vec![241, 1, 0, 0]),
            ("raw_object_indices", vec![240, 1]),
        ] {
            let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
            wire[column][0] = serde_json::json!(raw);
            let error = serde_json::from_value::<FeatureDatumPlaneHeader>(wire).unwrap_err();
            let field = if column == "raw_descriptor_indices" {
                "descriptor_indices"
            } else {
                "object_indices"
            };
            assert!(error.to_string().contains(field));
        }
    }
}
