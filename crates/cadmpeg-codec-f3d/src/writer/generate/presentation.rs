// SPDX-License-Identifier: Apache-2.0
//! Shared Design type registration and browser-node identities for generated output.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use sha2::{Digest, Sha256};

use crate::design::body::{
    BODY_MAP_CARRIER_BASE_TYPE_GUID, BODY_MAP_CARRIER_TYPE_GUID, BODY_MAP_CARRIER_TYPE_VERSION,
};
use crate::design::presentation::{
    BROWSER_NODE_BASE_TYPE_GUID, BROWSER_NODE_TYPE_GUID, BROWSER_NODE_TYPE_VERSION,
};
use crate::records::{SegmentType, DESIGN_MODULE_BODY, DESIGN_MODULE_FUSION};

use super::attributes::{source_less_body_key, AttributeIndex};
use super::preconditions::DesignBindingsValidated;

/// One type-table row after generated record types have been registered.
pub(crate) struct GeneratedDesignType {
    pub type_guid: String,
    pub base_type_guid: Option<String>,
    pub version: u32,
    pub module: String,
    pub entity_ids: Vec<u64>,
}

impl From<&SegmentType> for GeneratedDesignType {
    fn from(value: &SegmentType) -> Self {
        Self {
            type_guid: value.type_guid.clone(),
            base_type_guid: value.base_type_guid.as_ref().map(|field| field.value.clone()),
            version: value.version,
            module: value.module.clone(),
            entity_ids: value.entity_ids.clone(),
        }
    }
}

/// One generated browser node joined to a body-map entity suffix.
pub(crate) struct GeneratedBrowserNode {
    pub entity_suffix: u64,
    pub node_guid: String,
    pub record_index: u32,
    pub visible: bool,
}

/// The common registry consumed by both generated Design streams.
///
/// Construction allocates every synthetic record identity and adds its type
/// membership once. The stream encoders therefore cannot derive different
/// class tags, record indices, or browser-node identities.
pub(crate) struct GeneratedDesignRegistry {
    pub types: Vec<GeneratedDesignType>,
    pub body_map: BTreeMap<u64, u64>,
    pub body_map_record_index: Option<u32>,
    pub body_map_class_tag: Option<String>,
    pub browser_nodes: Vec<GeneratedBrowserNode>,
    pub browser_node_class_tag: Option<String>,
}

impl GeneratedDesignRegistry {
    pub(crate) fn new(
        target: &CadIr,
        bindings: DesignBindingsValidated<'_>,
        attributes: &AttributeIndex<'_>,
    ) -> Result<Self, CodecError> {
        let native = bindings.native();
        let visibility_by_body = native
            .body_visibilities
            .iter()
            .map(|visibility| (visibility.body.as_str(), visibility))
            .collect::<BTreeMap<_, _>>();
        let mut body_map = BTreeMap::new();
        let mut suffixes = BTreeSet::new();
        let mut pending = Vec::new();

        for (ordinal, body) in target.model.bodies.iter().enumerate() {
            let Some(visible) = body.visible else {
                continue;
            };
            let metadata = visibility_by_body.get(body.id.as_str()).copied();
            let asm_body_key = match metadata {
                Some(metadata) => metadata.asm_body_key,
                None => u64::try_from(source_less_body_key(attributes, body, ordinal)?).map_err(
                    |_| CodecError::Malformed("source-less ASM body key is negative".into()),
                )?,
            };
            let entity_suffix = metadata.map_or(asm_body_key, |metadata| metadata.entity_suffix);
            if body_map.insert(asm_body_key, entity_suffix).is_some() {
                return Err(CodecError::malformed(format_args!(
                    "multiple source-less F3D bodies use ASM body key {asm_body_key}"
                )));
            }
            if !suffixes.insert(entity_suffix) {
                return Err(CodecError::malformed(format_args!(
                    "multiple source-less F3D bodies use Design entity {entity_suffix}"
                )));
            }
            pending.push((entity_suffix, body.id.as_str(), visible));
        }

        let mut used_record_indices = native
            .design_types
            .iter()
            .flat_map(|design_type| design_type.entity_ids.iter().copied())
            .filter_map(|entity| u32::try_from(entity).ok())
            .collect::<BTreeSet<_>>();
        used_record_indices.extend(
            native
                .design_record_headers
                .iter()
                .map(|header| header.record_index),
        );
        used_record_indices.extend(
            native
                .design_entity_headers
                .iter()
                .filter_map(|header| u32::try_from(header.entity_suffix).ok()),
        );
        used_record_indices.extend(
            body_map
                .values()
                .filter_map(|entity_suffix| u32::try_from(*entity_suffix).ok()),
        );
        let mut next_record_index = if body_map.is_empty() && pending.is_empty() {
            0
        } else {
            used_record_indices
                .last()
                .copied()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| {
                    CodecError::Malformed("F3D Design record index space is full".into())
                })?
        };

        let body_map_record_index = (!body_map.is_empty())
            .then(|| allocate_record_index(&mut used_record_indices, &mut next_record_index))
            .transpose()?;
        pending.sort_by_key(|(entity_suffix, _, _)| *entity_suffix);
        let mut browser_nodes = Vec::with_capacity(pending.len());
        for (entity_suffix, body_id, visible) in pending {
            let record_index =
                allocate_record_index(&mut used_record_indices, &mut next_record_index)?;
            browser_nodes.push(GeneratedBrowserNode {
                entity_suffix,
                node_guid: deterministic_guid("browser-node", body_id),
                record_index,
                visible,
            });
        }

        let mut types = native
            .design_types
            .iter()
            .map(GeneratedDesignType::from)
            .collect::<Vec<_>>();
        let body_map_type = body_map_record_index
            .map(|record_index| {
                register_generated_type(
                    &mut types,
                    BODY_MAP_CARRIER_TYPE_GUID,
                    BODY_MAP_CARRIER_BASE_TYPE_GUID,
                    BODY_MAP_CARRIER_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    vec![u64::from(record_index)],
                )
            })
            .transpose()?;
        let browser_node_type = (!browser_nodes.is_empty())
            .then(|| {
                register_generated_type(
                    &mut types,
                    BROWSER_NODE_TYPE_GUID,
                    BROWSER_NODE_BASE_TYPE_GUID,
                    BROWSER_NODE_TYPE_VERSION,
                    DESIGN_MODULE_FUSION,
                    browser_nodes
                        .iter()
                        .map(|node| u64::from(node.record_index))
                        .collect(),
                )
            })
            .transpose()?;

        Ok(Self {
            body_map_class_tag: body_map_type.map(dynamic_class_tag).transpose()?,
            body_map_record_index,
            browser_node_class_tag: browser_node_type.map(dynamic_class_tag).transpose()?,
            types,
            body_map,
            browser_nodes,
        })
    }
}

fn allocate_record_index(used: &mut BTreeSet<u32>, next: &mut u32) -> Result<u32, CodecError> {
    while used.contains(next) {
        *next = next
            .checked_add(1)
            .ok_or_else(|| CodecError::Malformed("F3D Design record index space is full".into()))?;
    }
    let allocated = *next;
    used.insert(allocated);
    *next = next
        .checked_add(1)
        .ok_or_else(|| CodecError::Malformed("F3D Design record index space is full".into()))?;
    Ok(allocated)
}

fn register_generated_type(
    types: &mut Vec<GeneratedDesignType>,
    type_guid: &str,
    base_type_guid: &str,
    version: u32,
    module: &str,
    mut entity_ids: Vec<u64>,
) -> Result<usize, CodecError> {
    let matches = types
        .iter()
        .enumerate()
        .filter(|(_, design_type)| design_type.type_guid.eq_ignore_ascii_case(type_guid))
        .map(|(ordinal, _)| ordinal)
        .collect::<Vec<_>>();
    let ordinal = match matches.as_slice() {
        [] => {
            types.push(GeneratedDesignType {
                type_guid: type_guid.to_owned(),
                base_type_guid: Some(base_type_guid.to_owned()),
                version,
                module: module.to_owned(),
                entity_ids: Vec::new(),
            });
            types.len() - 1
        }
        [ordinal] => *ordinal,
        _ => {
            return Err(CodecError::malformed(format_args!(
                "F3D Design type registry repeats built-in type {type_guid}"
            )))
        }
    };
    let design_type = &mut types[ordinal];
    if design_type.version != version
        || design_type.module != module
        || design_type
            .base_type_guid
            .as_deref()
            .is_none_or(|base| !base.eq_ignore_ascii_case(base_type_guid))
    {
        return Err(CodecError::malformed(format_args!(
            "F3D Design type {type_guid} conflicts with its built-in registration"
        )));
    }
    entity_ids.sort_unstable();
    entity_ids.dedup();
    design_type.entity_ids = entity_ids;
    Ok(ordinal)
}

pub(crate) fn dynamic_class_tag(type_ordinal: usize) -> Result<String, CodecError> {
    let tag = u32::try_from(type_ordinal)
        .ok()
        .and_then(|ordinal| ordinal.checked_add(256))
        .filter(|tag| *tag <= 999)
        .ok_or_else(|| {
            CodecError::NotImplemented(
                "source-less F3D Design type registry exceeds three-digit class tags".into(),
            )
        })?;
    Ok(tag.to_string())
}

/// Build an RFC 9562 version-8 UUID from a domain-separated stable identity.
fn deterministic_guid(domain: &str, identity: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"cadmpeg:f3d:generated-design:v1\0");
    digest.update(domain.as_bytes());
    digest.update(b"\0");
    digest.update(identity.as_bytes());
    let mut bytes: [u8; 16] = digest.finalize()[..16]
        .try_into()
        .expect("SHA-256 always contains sixteen prefix bytes");
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}

#[cfg(test)]
mod tests {
    use super::{deterministic_guid, GeneratedDesignRegistry};

    fn body_map_type(entity_ids: Vec<u64>) -> crate::records::SegmentType {
        crate::records::SegmentType {
            id: "synthetic:design-type#body-map".into(),
            byte_offset: 0,
            type_guid: crate::design::body::BODY_MAP_CARRIER_TYPE_GUID.into(),
            type_guid_offset: 0,
            base_type_guid: Some(crate::records::RecordedValue { value: crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID.into(), offset: Some(0) }),
            version: crate::design::body::BODY_MAP_CARRIER_TYPE_VERSION,
            version_offset: 0,
            module: crate::records::DESIGN_MODULE_BODY.into(),
            entity_id_offsets: vec![0; entity_ids.len()],
            entity_ids,
        }
    }

    fn browser_node_type(entity_ids: Vec<u64>) -> crate::records::SegmentType {
        crate::records::SegmentType {
            id: "synthetic:design-type#browser-node".into(),
            byte_offset: 0,
            type_guid: crate::design::presentation::BROWSER_NODE_TYPE_GUID.into(),
            type_guid_offset: 0,
            base_type_guid: Some(crate::records::RecordedValue { value: crate::design::presentation::BROWSER_NODE_BASE_TYPE_GUID.into(), offset: Some(0) }),
            version: crate::design::presentation::BROWSER_NODE_TYPE_VERSION,
            version_offset: 0,
            module: crate::records::DESIGN_MODULE_FUSION.into(),
            entity_id_offsets: vec![0; entity_ids.len()],
            entity_ids,
        }
    }

    fn node_guids_for_order(reverse: bool) -> std::collections::BTreeMap<u64, String> {
        let mut target = cadmpeg_ir::examples::unit_cube();
        let mut first = target.model.bodies[0].clone();
        first.id =
            cadmpeg_ir::ids::BodyId::mint("synthetic:stable-body:a").expect("identity grammar");
        first.visible = Some(false);
        let mut second = first.clone();
        second.id =
            cadmpeg_ir::ids::BodyId::mint("synthetic:stable-body:b").expect("identity grammar");
        second.visible = Some(true);
        target.model.bodies = if reverse {
            vec![second.clone(), first.clone()]
        } else {
            vec![first.clone(), second.clone()]
        };

        let native = crate::native::F3dNative {
            body_native_keys: vec![
                cadmpeg_asm::brep::records::BodyNativeKey {
                    source_namespace:
                        cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                            cadmpeg_asm::ids::IdFormat("generated"),
                        ),
                    body: first.id.clone(),
                    record_index: 1,
                    body_ordinal: 0,
                    source_brep: None,
                    asm_body_key: Some(11),
                },
                cadmpeg_asm::brep::records::BodyNativeKey {
                    source_namespace:
                        cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                            cadmpeg_asm::ids::IdFormat("generated"),
                        ),
                    body: second.id.clone(),
                    record_index: 2,
                    body_ordinal: 1,
                    source_brep: None,
                    asm_body_key: Some(22),
                },
            ],
            body_visibilities: vec![
                crate::records::BodyVisibility {
                    id: "generated:visibility#a".into(),
                    body: first.id,
                    stream: "generated/Design1/BulkStream.dat".into(),
                    byte_offset: 0,
                    asm_body_key_offset: 0,
                    asm_body_key: 11,
                    entity_suffix: 101,
                    visible: false,
                },
                crate::records::BodyVisibility {
                    id: "generated:visibility#b".into(),
                    body: second.id,
                    stream: "generated/Design1/BulkStream.dat".into(),
                    byte_offset: 0,
                    asm_body_key_offset: 0,
                    asm_body_key: 22,
                    entity_suffix: 202,
                    visible: true,
                },
            ],
            ..Default::default()
        };
        let attributes = super::AttributeIndex::new(&target, &native)
            .expect("unambiguous appearance assignments");
        let bindings = super::super::preconditions::validate_source_less_design_bindings(&native)
            .expect("synthetic body bindings");
        GeneratedDesignRegistry::new(&target, bindings, &attributes)
            .expect("generated Design registry")
            .browser_nodes
            .into_iter()
            .map(|node| (node.entity_suffix, node.node_guid))
            .collect()
    }

    #[test]
    fn generated_guid_is_stable_domain_separated_uuid_v8() {
        let first = deterministic_guid("browser-node", "body:stable");
        assert_eq!(first, deterministic_guid("browser-node", "body:stable"));
        assert_ne!(
            first,
            deterministic_guid("physical-material", "body:stable")
        );
        assert_eq!(first.len(), 36);
        assert_eq!(first.as_bytes()[14], b'8');
        assert!(matches!(first.as_bytes()[19], b'8' | b'9' | b'A' | b'B'));
        assert!(crate::bytes::is_guid_relaxed(&first));
    }

    #[test]
    fn browser_node_guid_does_not_depend_on_neutral_body_order() {
        assert_eq!(node_guids_for_order(false), node_guids_for_order(true));
    }

    #[test]
    fn generated_presentation_records_do_not_alias_their_body_entity() {
        let mut target = cadmpeg_ir::examples::unit_cube();
        target.model.bodies[0].visible = Some(true);
        let native = crate::native::F3dNative::default();
        let attributes = super::AttributeIndex::new(&target, &native)
            .expect("unambiguous appearance assignments");
        let bindings = super::super::preconditions::validate_source_less_design_bindings(&native)
            .expect("empty Design bindings");
        let registry = GeneratedDesignRegistry::new(&target, bindings, &attributes)
            .expect("generated Design registry");
        let [node] = registry.browser_nodes.as_slice() else {
            panic!("one visible body must generate one browser node")
        };
        let body_map = registry
            .body_map_record_index
            .expect("one visible body must generate one body map");
        assert_ne!(u64::from(node.record_index), node.entity_suffix);
        assert_ne!(u64::from(body_map), node.entity_suffix);
        assert_ne!(body_map, node.record_index);
    }

    #[test]
    fn generated_body_map_replaces_stale_type_membership() {
        let mut target = cadmpeg_ir::examples::unit_cube();
        target.model.bodies[0].visible = Some(true);
        let native = crate::native::F3dNative {
            design_types: vec![body_map_type(vec![17])],
            ..Default::default()
        };
        let attributes = super::AttributeIndex::new(&target, &native)
            .expect("unambiguous appearance assignments");
        let bindings = super::super::preconditions::validate_source_less_design_bindings(&native)
            .expect("empty Design bindings");
        let registry = GeneratedDesignRegistry::new(&target, bindings, &attributes)
            .expect("generated Design registry");
        let record_index = registry.body_map_record_index.expect("generated body map");
        let body_map_type = registry
            .types
            .iter()
            .find(|design_type| {
                design_type
                    .type_guid
                    .eq_ignore_ascii_case(crate::design::body::BODY_MAP_CARRIER_TYPE_GUID)
            })
            .expect("body-map type registration");
        assert_eq!(body_map_type.entity_ids, [u64::from(record_index)]);
        assert!(!body_map_type.entity_ids.contains(&17));
    }

    #[test]
    fn generated_browser_nodes_replace_stale_type_membership() {
        let mut target = cadmpeg_ir::examples::unit_cube();
        target.model.bodies[0].visible = Some(true);
        let native = crate::native::F3dNative {
            design_types: vec![browser_node_type(vec![17])],
            ..Default::default()
        };
        let attributes = super::AttributeIndex::new(&target, &native)
            .expect("unambiguous appearance assignments");
        let bindings = super::super::preconditions::validate_source_less_design_bindings(&native)
            .expect("empty Design bindings");
        let registry = GeneratedDesignRegistry::new(&target, bindings, &attributes)
            .expect("generated Design registry");
        let [node] = registry.browser_nodes.as_slice() else {
            panic!("one visible body must generate one browser node")
        };
        let node_type = registry
            .types
            .iter()
            .find(|design_type| {
                design_type
                    .type_guid
                    .eq_ignore_ascii_case(crate::design::presentation::BROWSER_NODE_TYPE_GUID)
            })
            .expect("browser-node type registration");
        assert_eq!(node_type.entity_ids, [u64::from(node.record_index)]);
        assert!(!node_type.entity_ids.contains(&17));
    }

    #[test]
    fn full_record_index_space_is_irrelevant_without_generated_nodes() {
        let target = cadmpeg_ir::examples::unit_cube();
        let native = crate::native::F3dNative {
            design_types: vec![browser_node_type(vec![u64::from(u32::MAX)])],
            ..Default::default()
        };
        let attributes = super::AttributeIndex::new(&target, &native)
            .expect("unambiguous appearance assignments");
        let bindings = super::super::preconditions::validate_source_less_design_bindings(&native)
            .expect("empty Design bindings");
        let registry = GeneratedDesignRegistry::new(&target, bindings, &attributes)
            .expect("no browser-node allocation is needed");
        assert!(registry.browser_nodes.is_empty());
    }
}
