// SPDX-License-Identifier: Apache-2.0
//! Wrap the format-independent ASM B-rep graph with Fusion-derived links,
//! blob-scoped id qualification, and Design body-map selector resolution.

use crate::records::{
    sketch_link_sense_is_unconstrained, CreationTimestamp, PersistentDesignLink,
    PersistentSubentityTag, SketchCurveLink,
};
use cadmpeg_asm::brep::attributes::attribute_key;
use cadmpeg_asm::brep::records::BodyNativeKey;
use cadmpeg_asm::brep::{
    collect_entity_adjacency, collect_owned_ids, decode_with_header, decode_with_purpose,
    remap_owned_ids, retain_root_entities, AsmBrep, DecodePurpose,
};
use cadmpeg_asm::ids::IdFormat;
use cadmpeg_asm::sab::Record;
use cadmpeg_core::decode::bounded_len;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::BodyId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// The ASM B-rep graph plus links derived from Fusion attribute records.
///
/// Kernel arenas are flattened at the top level for [`retain_root_entities`].
#[derive(Default, Serialize, Deserialize)]
pub struct Brep {
    /// The format-independent ASM graph.
    #[serde(flatten)]
    pub asm: AsmBrep,
    /// Typed sketch-curve provenance links.
    pub sketch_curve_links: Vec<SketchCurveLink>,
    /// Persistent design identifiers attached to solved entities.
    pub persistent_design_links: Vec<PersistentDesignLink>,
    /// Variable-width persistent tag groups attached to solved faces and edges.
    pub persistent_subentity_tags: Vec<PersistentSubentityTag>,
    /// Original authoring times attached to solved entities.
    pub creation_timestamps: Vec<CreationTimestamp>,
}

impl Brep {
    /// Wrap a decoded ASM graph and derive the Fusion attribute links.
    fn from_asm(asm: AsmBrep) -> Self {
        let sketch_curve_links = asm
            .attributes
            .iter()
            .filter_map(sketch_curve_link)
            .collect();
        let persistent_design_links = asm
            .attributes
            .iter()
            .flat_map(persistent_design_links)
            .collect();
        let persistent_subentity_tags = asm
            .attributes
            .iter()
            .flat_map(persistent_subentity_tags)
            .collect();
        let creation_timestamps = asm
            .attributes
            .iter()
            .filter_map(creation_timestamp)
            .collect();
        Self {
            asm,
            sketch_curve_links,
            persistent_design_links,
            persistent_subentity_tags,
            creation_timestamps,
        }
    }

    /// Map solved bodies to the selector used by this blob's Design body map.
    pub fn body_selectors(&self) -> HashMap<BodyId, u64> {
        let ordinal_mode = self
            .asm
            .body_native_keys
            .iter()
            .all(|body| body.asm_body_key.is_none());
        self.asm
            .body_native_keys
            .iter()
            .filter_map(|body| {
                let selector = if ordinal_mode {
                    Some(u64::from(body.body_ordinal))
                } else {
                    body.asm_body_key
                }?;
                Some((body.body.clone(), selector))
            })
            .collect()
    }

    /// Resolve the Design selectors present for this blob. An exact native
    /// body key has precedence. A selector absent from the native-key domain
    /// selects the body with the same zero-based ordinal.
    pub(crate) fn body_selectors_for(
        &self,
        selectors: &HashSet<u64>,
    ) -> Result<HashMap<BodyId, u64>, cadmpeg_core::CodecError> {
        let body_keys = self.asm.body_native_keys.iter().collect::<Vec<_>>();
        let mut resolved = HashMap::new();
        for selector in selectors {
            let Some(body) = resolve_body_selector(&body_keys, *selector)? else {
                continue;
            };
            if let Some(previous) = resolved.insert(body.clone(), *selector) {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "F3D body {} is selected by both {previous} and {selector}",
                    body.0
                )));
            }
        }
        Ok(resolved)
    }

    /// Retain the connected entity graph rooted at the body-map keys selected
    /// for one BREP blob.
    pub fn retain_body_keys(
        &mut self,
        selected_keys: &HashSet<u64>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let annotations = std::mem::take(&mut self.asm.annotation_records);
        let mut value = serde_value::to_value(&*self).map_err(|error| {
            cadmpeg_core::CodecError::malformed(format_args!("BREP serialization failed: {error}"))
        })?;
        let mut owned = HashSet::new();
        collect_owned_ids(&value, &mut owned);
        let native_body_ids = self
            .asm
            .body_native_keys
            .iter()
            .map(|native| native.body.0.as_str())
            .collect::<HashSet<_>>();
        let mut roots = self
            .body_selectors_for(selected_keys)?
            .into_keys()
            .map(|body| body.0)
            .collect::<HashSet<_>>();
        // A Design body map selects native ASM body records. Neutral roots
        // projected from other saved top-level entities have no ASM body key
        // and remain part of the selected BREP blob.
        roots.extend(
            self.asm
                .bodies
                .iter()
                .filter(|body| !native_body_ids.contains(body.id.0.as_str()))
                .map(|body| body.id.0.clone()),
        );
        let mut adjacency = HashMap::<String, HashSet<String>>::new();
        collect_entity_adjacency(&value, &owned, &mut adjacency);
        let mut reachable = roots;
        let mut pending = reachable.iter().cloned().collect::<Vec<_>>();
        while let Some(id) = pending.pop() {
            for adjacent in adjacency.get(&id).into_iter().flatten() {
                if reachable.insert(adjacent.clone()) {
                    pending.push(adjacent.clone());
                }
            }
        }
        retain_root_entities(&mut value, &reachable);
        let mut retained: Self = crate::value_tree::from_value(value).map_err(|error| {
            cadmpeg_core::CodecError::malformed(format_args!(
                "retained BREP graph is invalid: {error}"
            ))
        })?;
        retained
            .asm
            .body_keys
            .retain(|body, _| reachable.contains(&body.0));
        retained.asm.annotation_records = annotations
            .into_iter()
            .filter(|annotation| reachable.contains(&annotation.id))
            .collect();
        *self = retained;
        Ok(())
    }

    /// Qualify every entity owned by this graph so several BREP blobs can
    /// coexist in one document model without record-index collisions.
    pub fn qualify_ids(
        &mut self,
        format: IdFormat<'_>,
        namespace: &str,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let annotations = std::mem::take(&mut self.asm.annotation_records);
        let mut value = serde_value::to_value(&*self).map_err(|error| {
            cadmpeg_core::CodecError::malformed(format_args!("BREP serialization failed: {error}"))
        })?;
        let mut owned = HashSet::new();
        collect_owned_ids(&value, &mut owned);
        let scheme_prefix = format!("{format}:");
        let replacements = owned
            .into_iter()
            .map(|id| {
                let replacement = format!(
                    "{format}:brep/{namespace}/{}",
                    id.strip_prefix(&scheme_prefix).unwrap_or(&id)
                );
                (id, replacement)
            })
            .collect::<HashMap<_, _>>();
        remap_owned_ids(&mut value, &replacements);
        let mut qualified: Self = crate::value_tree::from_value(value).map_err(|error| {
            cadmpeg_core::CodecError::malformed(format_args!("qualified BREP is invalid: {error}"))
        })?;
        qualified.asm.annotation_records = annotations
            .into_iter()
            .map(|mut annotation| {
                if let Some(id) = replacements.get(&annotation.id) {
                    annotation.id.clone_from(id);
                }
                annotation
            })
            .collect();
        *self = qualified;
        Ok(())
    }

    /// Append a disjoint, already-qualified BREP graph.
    pub fn append(&mut self, mut other: Self) {
        self.asm.append(other.asm);
        macro_rules! append_vecs {
            ($($field:ident),+ $(,)?) => {
                $(self.$field.append(&mut other.$field);)+
            };
        }
        append_vecs!(
            sketch_curve_links,
            persistent_design_links,
            persistent_subentity_tags,
            creation_timestamps,
        );
    }
}

/// Decode a framed active slice into the IR B-rep graph.
///
/// `stream` names the source ZIP entry for provenance. Ids are minted as
/// `<format>:brep:entity#<record-index>`, unique across the `RecordTable`.
pub fn decode(records: &[Record], bytes: &[u8], stream: &str, format: IdFormat<'_>) -> Brep {
    Brep::from_asm(decode_with_purpose(
        records,
        bytes,
        stream,
        format,
        DecodePurpose::Model,
    ))
}

/// Decode a parsed text stream ([`cadmpeg_asm::sat`]) into the IR B-rep graph.
///
/// The text parser already typed the records and converted lengths into the
/// binary centimetre convention, so the shared decode path runs unchanged.
/// The header comes from the stream's ASCII header lines rather than a binary
/// header parse of `bytes`.
pub fn decode_text(
    stream: &cadmpeg_asm::sat::TextStream,
    bytes: &[u8],
    entry: &str,
    format: IdFormat<'_>,
) -> Brep {
    Brep::from_asm(decode_with_header(
        &stream.records,
        bytes,
        Some(stream.header.as_kernel_header()),
        entry,
        format,
        DecodePurpose::Model,
    ))
}

/// Decode only the topology and analytic measurements used to bind ASM
/// history. Free-form carrier shapes are not materialized because historical
/// binding consumes their stable record identities, not their control data.
pub(crate) fn decode_history_topology(
    records: &[Record],
    bytes: &[u8],
    format: IdFormat<'_>,
) -> Brep {
    Brep::from_asm(decode_with_purpose(
        records,
        bytes,
        "history",
        format,
        DecodePurpose::History,
    ))
}

/// Resolve one Design body selector within one BREP blob. Exact native keys
/// take precedence; an absent key falls back to the zero-based body ordinal.
pub(crate) fn resolve_body_selector(
    body_keys: &[&BodyNativeKey],
    selector: u64,
) -> Result<Option<BodyId>, cadmpeg_core::CodecError> {
    let direct = body_keys
        .iter()
        .filter(|body| body.asm_body_key == Some(selector))
        .map(|body| body.body.clone())
        .collect::<Vec<_>>();
    match direct.as_slice() {
        [body] => return Ok(Some(body.clone())),
        [] => {}
        _ => {
            return Err(cadmpeg_core::CodecError::malformed(format_args!(
                "F3D body selector {selector} matches multiple native body keys"
            )));
        }
    }
    let Some(ordinal) = u32::try_from(selector).ok() else {
        return Ok(None);
    };
    let ordinal = body_keys
        .iter()
        .filter(|body| body.body_ordinal == ordinal)
        .map(|body| body.body.clone())
        .collect::<Vec<_>>();
    match ordinal.as_slice() {
        [body] => Ok(Some(body.clone())),
        [] => Ok(None),
        _ => Err(cadmpeg_core::CodecError::malformed(format_args!(
            "F3D body selector {selector} matches multiple body ordinals"
        ))),
    }
}

/// The five members every `sketch_attrib_def` payload form writes.
struct SketchLinkPayload {
    sketch_curve_id: i64,
    ref_b: u64,
    sense: i64,
    role: i64,
    closure: i64,
}

/// Read the payload following a `sketch_attrib_def` family name.
///
/// The three header integers are `1`, `1`, and a form selector. Form `3` writes
/// the members as one tagged ASCII field with a `0` between the sense and the
/// role, form `2` as six integers with a trailing `0`, and form `0` as the five
/// members alone. All three write the same five members in the same order, so
/// each yields one link.
fn sketch_link_payload(values: &[AttributeValue]) -> Option<SketchLinkPayload> {
    let [AttributeValue::Integer(1), AttributeValue::Integer(1), AttributeValue::Integer(form), payload @ ..] =
        values
    else {
        return None;
    };
    match (*form, payload) {
        (3, [AttributeValue::String(field)]) => {
            let fields = field.split_ascii_whitespace().collect::<Vec<_>>();
            let [sketch_curve_id, ref_b, sense, "0", role, closure] = fields[..] else {
                return None;
            };
            // `ref_b` reaches the full unsigned 64-bit range, so it is read
            // unsigned; every other member is signed.
            Some(SketchLinkPayload {
                sketch_curve_id: sketch_curve_id.parse().ok()?,
                ref_b: ref_b.parse().ok()?,
                sense: sense.parse().ok()?,
                role: role.parse().ok()?,
                closure: closure.parse().ok()?,
            })
        }
        (
            2,
            [AttributeValue::Integer(sketch_curve_id), AttributeValue::Integer(ref_b), AttributeValue::Integer(sense), AttributeValue::Integer(role), AttributeValue::Integer(closure), AttributeValue::Integer(0)],
        )
        | (
            0,
            [AttributeValue::Integer(sketch_curve_id), AttributeValue::Integer(ref_b), AttributeValue::Integer(sense), AttributeValue::Integer(role), AttributeValue::Integer(closure)],
        ) => Some(SketchLinkPayload {
            sketch_curve_id: *sketch_curve_id,
            ref_b: u64::try_from(*ref_b).ok()?,
            sense: *sense,
            role: *role,
            closure: *closure,
        }),
        _ => None,
    }
}

pub(crate) fn sketch_curve_link(attribute: &SourceAttribute) -> Option<SketchCurveLink> {
    let family = attribute.values.iter().position(
        |value| matches!(value, AttributeValue::String(name) if name == "sketch_attrib_def"),
    )?;
    let payload = sketch_link_payload(&attribute.values[family + 1..])?;
    Some(SketchCurveLink {
        id: format!("f3d:design:sketch-curve-link#{}", attribute_key(attribute)),
        target: attribute.target.clone(),
        sketch_curve_id: payload.sketch_curve_id,
        ref_b: payload.ref_b,
        sense: (!sketch_link_sense_is_unconstrained(payload.sense)).then_some(payload.sense),
        role: payload.role,
        closure: payload.closure,
    })
}

pub(crate) fn persistent_design_links(attribute: &SourceAttribute) -> Vec<PersistentDesignLink> {
    let AttributeTarget::Body(_) = &attribute.target else {
        return Vec::new();
    };
    let Some(family) = attribute.values.iter().position(
        |value| matches!(value, AttributeValue::String(name) if name == "generic_tag_attrib_def"),
    ) else {
        return Vec::new();
    };
    let values = &attribute.values[family + 1..];
    let [AttributeValue::Integer(3), AttributeValue::Integer(3), AttributeValue::Integer(-1), AttributeValue::String(marker), AttributeValue::Integer(group_count), rest @ ..] =
        values
    else {
        return Vec::new();
    };
    if marker != "generic_tag_attrib_def " || *group_count < 0 {
        return Vec::new();
    }
    let Ok(group_count) = usize::try_from(*group_count) else {
        return Vec::new();
    };
    if rest.len() != group_count.saturating_mul(5) {
        return Vec::new();
    }
    let groups = rest
        .chunks_exact(5)
        .filter_map(|values| match values {
            [
                AttributeValue::Integer(entity_kind),
                AttributeValue::String(design_id),
                AttributeValue::Integer(design_reference),
                AttributeValue::Integer(0),
                AttributeValue::Integer(0),
            ] if !design_id.is_empty()
                && design_id.bytes().all(|byte| byte.is_ascii_digit()) =>
            {
                Some((*entity_kind, design_id.clone(), *design_reference))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if groups.len() != group_count {
        return Vec::new();
    }
    let groups = groups
        .into_iter()
        .filter(|(entity_kind, _, _)| *entity_kind == 3)
        .collect::<Vec<_>>();
    let last = groups.len().saturating_sub(1);
    groups
        .into_iter()
        .enumerate()
        .map(
            |(ordinal, (entity_kind, design_id, design_reference))| PersistentDesignLink {
                id: format!(
                    "f3d:design:persistent-design-link#{}:{ordinal}",
                    attribute_key(attribute)
                ),
                target: attribute.target.clone(),
                design_id,
                entity_kind,
                design_reference,
                ordinal: ordinal as u32,
                is_current: ordinal == last,
            },
        )
        .collect()
}

pub(crate) fn persistent_subentity_tags(
    attribute: &SourceAttribute,
) -> Vec<PersistentSubentityTag> {
    if !matches!(
        attribute.target,
        AttributeTarget::Face(_) | AttributeTarget::Edge(_)
    ) {
        return Vec::new();
    }
    let Some(family) = attribute.values.iter().position(
        |value| matches!(value, AttributeValue::String(name) if name == "generic_tag_attrib_def"),
    ) else {
        return Vec::new();
    };
    let values = &attribute.values[family + 1..];
    let [AttributeValue::Integer(3), AttributeValue::Integer(3), AttributeValue::Integer(-1), AttributeValue::String(marker), AttributeValue::Integer(group_count), rest @ ..] =
        values
    else {
        return Vec::new();
    };
    if marker != "generic_tag_attrib_def " || *group_count < 0 {
        return Vec::new();
    }
    let Ok(group_count) = usize::try_from(*group_count) else {
        return Vec::new();
    };
    // Each group consumes at least four leading attribute values from `rest`.
    let Some(group_count) = bounded_len(group_count as u64, 4, rest.len()) else {
        return Vec::new();
    };
    let mut position: usize = 0;
    let mut groups = Vec::with_capacity(group_count);
    for ordinal in 0..group_count {
        let Some(
            [AttributeValue::Integer(selector), AttributeValue::String(token), AttributeValue::Integer(0), AttributeValue::Integer(reference_count)],
        ) = rest.get(position..position.saturating_add(4))
        else {
            return Vec::new();
        };
        if token.is_empty() || *reference_count < 0 {
            return Vec::new();
        }
        let Ok(reference_count) = usize::try_from(*reference_count) else {
            return Vec::new();
        };
        let reference_start = position + 4;
        let reference_end = reference_start.saturating_add(reference_count);
        let Some(reference_values) = rest.get(reference_start..reference_end) else {
            return Vec::new();
        };
        let references = reference_values
            .iter()
            .map(|value| match value {
                AttributeValue::Integer(value) => Some(*value),
                _ => None,
            })
            .collect::<Option<Vec<_>>>();
        let Some(design_references) = references else {
            return Vec::new();
        };
        if !matches!(rest.get(reference_end), Some(AttributeValue::Integer(0))) {
            return Vec::new();
        }
        groups.push(PersistentSubentityTag {
            id: format!(
                "f3d:design:persistent-subentity-tag#{}:{ordinal}",
                attribute_key(attribute)
            ),
            target: attribute.target.clone(),
            selector: *selector,
            token: token.clone(),
            design_references,
            ordinal: ordinal as u32,
        });
        position = reference_end + 1;
    }
    if position != rest.len() {
        return Vec::new();
    }
    groups
}

pub(crate) fn creation_timestamp(attribute: &SourceAttribute) -> Option<CreationTimestamp> {
    let family = attribute.values.iter().position(
        |value| matches!(value, AttributeValue::String(name) if name == "Timestamp_attrib_def"),
    )?;
    let marker = attribute.values.get(family + 1)?;
    if !matches!(marker, AttributeValue::Integer(1)) {
        return None;
    }
    let AttributeValue::Float(unix_microseconds) = attribute.values.get(family + 2)? else {
        return None;
    };
    if !unix_microseconds.is_finite() {
        return None;
    }
    Some(CreationTimestamp {
        id: format!("f3d:design:creation-timestamp#{}", attribute_key(attribute)),
        target: attribute.target.clone(),
        record_index: attribute_key(attribute).parse().ok()?,
        unix_microseconds: *unix_microseconds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_asm::brep::AnnotationRecord;
    use cadmpeg_ir::ids::RegionId;
    use cadmpeg_ir::topology::{Body, BodyKind, Region};

    #[test]
    fn brep_qualification_rewrites_owned_ids_and_cross_references() {
        let body = BodyId("f3d:brep:entity#1".into());
        let region = RegionId("f3d:brep:entity#2".into());
        let mut brep = Brep {
            asm: AsmBrep {
                bodies: vec![Body {
                    id: body.clone(),
                    kind: BodyKind::default(),
                    regions: vec![region.clone()],
                    transform: None,
                    name: None,
                    color: None,
                    visible: None,
                }],
                regions: vec![Region {
                    id: region,
                    body: body.clone(),
                    shells: Vec::new(),
                }],
                body_keys: HashMap::from([(body.clone(), 7)]),
                body_native_keys: vec![BodyNativeKey {
                    id: "f3d:asm:body-native-key#1".into(),
                    body,
                    record_index: 1,
                    body_ordinal: 0,
                    source_brep: Some("BREP.source.smbh".into()),
                    asm_body_key: Some(7),
                }],
                annotation_records: vec![AnnotationRecord {
                    id: "f3d:brep:entity#1".into(),
                    stream: "asset/BREP.source.smbh".into(),
                    offset: 10,
                    tag: "body".into(),
                    derived_fields: Vec::new(),
                }],
                ..AsmBrep::default()
            },
            ..Brep::default()
        };

        brep.qualify_ids(crate::ids::ID_FORMAT, "source")
            .expect("qualify BREP");

        let qualified = BodyId("f3d:brep/source/brep:entity#1".into());
        assert_eq!(brep.asm.bodies[0].id, qualified);
        assert_eq!(brep.asm.regions[0].body, qualified);
        assert_eq!(brep.asm.body_native_keys[0].body, qualified);
        assert_eq!(brep.asm.body_keys.get(&qualified), Some(&7));
        assert_eq!(brep.asm.annotation_records[0].id, qualified.0);
        assert_eq!(
            brep.asm.body_native_keys[0].source_brep.as_deref(),
            Some("BREP.source.smbh")
        );
    }

    #[test]
    fn body_key_retention_keeps_only_the_selected_connected_graph() {
        let body = |index, region| Body {
            id: BodyId(format!("f3d:brep:entity#{index}")),
            kind: BodyKind::default(),
            regions: vec![RegionId(format!("f3d:brep:entity#{region}"))],
            transform: None,
            name: None,
            color: None,
            visible: None,
        };
        let native_key = |index, key| BodyNativeKey {
            id: format!("f3d:asm:body-native-key#{index}"),
            body: BodyId(format!("f3d:brep:entity#{index}")),
            record_index: index,
            body_ordinal: index - 1,
            source_brep: Some("BREP.source.smbh".into()),
            asm_body_key: Some(key),
        };
        let mut brep = Brep {
            asm: AsmBrep {
                bodies: vec![body(1, 2), body(3, 4)],
                regions: vec![
                    Region {
                        id: RegionId("f3d:brep:entity#2".into()),
                        body: BodyId("f3d:brep:entity#1".into()),
                        shells: Vec::new(),
                    },
                    Region {
                        id: RegionId("f3d:brep:entity#4".into()),
                        body: BodyId("f3d:brep:entity#3".into()),
                        shells: Vec::new(),
                    },
                ],
                body_keys: HashMap::from([
                    (BodyId("f3d:brep:entity#1".into()), 10),
                    (BodyId("f3d:brep:entity#3".into()), 20),
                ]),
                body_native_keys: vec![native_key(1, 10), native_key(3, 20)],
                ..AsmBrep::default()
            },
            ..Brep::default()
        };

        brep.retain_body_keys(&HashSet::from([20]))
            .expect("retain body graph");

        assert_eq!(brep.asm.bodies.len(), 1);
        assert_eq!(brep.asm.bodies[0].id.0, "f3d:brep:entity#3");
        assert_eq!(brep.asm.regions.len(), 1);
        assert_eq!(brep.asm.regions[0].id.0, "f3d:brep:entity#4");
        assert_eq!(brep.asm.body_native_keys.len(), 1);
        assert_eq!(brep.asm.body_keys.len(), 1);
    }

    #[test]
    fn body_key_retention_preserves_selectorless_neutral_roots() {
        let native_body = BodyId("f3d:brep:entity#1".into());
        let projected_body = BodyId("f3d:brep:saved-edge-body#5".into());
        let mut brep = Brep {
            asm: AsmBrep {
                bodies: vec![
                    Body {
                        id: native_body.clone(),
                        kind: BodyKind::Solid,
                        regions: Vec::new(),
                        transform: None,
                        name: None,
                        color: None,
                        visible: None,
                    },
                    Body {
                        id: projected_body.clone(),
                        kind: BodyKind::Wire,
                        regions: Vec::new(),
                        transform: None,
                        name: None,
                        color: None,
                        visible: None,
                    },
                ],
                body_keys: HashMap::from([(native_body.clone(), 10)]),
                body_native_keys: vec![BodyNativeKey {
                    id: "f3d:asm:body-native-key#1".into(),
                    body: native_body,
                    record_index: 1,
                    body_ordinal: 0,
                    source_brep: Some("BREP.source.smbh".into()),
                    asm_body_key: Some(10),
                }],
                ..AsmBrep::default()
            },
            ..Brep::default()
        };

        brep.retain_body_keys(&HashSet::from([10]))
            .expect("retain body graph");

        assert_eq!(brep.asm.bodies.len(), 2);
        assert!(brep.asm.bodies.iter().any(|body| body.id == projected_body));
    }

    #[test]
    fn body_selectors_use_ordinals_only_for_an_all_null_key_lane() {
        let native_key = |ordinal, key| BodyNativeKey {
            id: format!("f3d:asm:body-native-key#{ordinal}"),
            body: BodyId(format!("f3d:brep:entity#{ordinal}")),
            record_index: ordinal,
            body_ordinal: ordinal,
            source_brep: Some("BREP.source.smb".into()),
            asm_body_key: key,
        };
        let mut brep = Brep {
            asm: AsmBrep {
                body_native_keys: vec![native_key(0, None), native_key(1, None)],
                ..AsmBrep::default()
            },
            ..Brep::default()
        };

        assert_eq!(brep.body_selectors().len(), 2);
        assert_eq!(
            brep.body_selectors()[&BodyId("f3d:brep:entity#1".into())],
            1
        );

        brep.asm.body_native_keys[1].asm_body_key = Some(7);
        assert_eq!(
            brep.body_selectors(),
            HashMap::from([(BodyId("f3d:brep:entity#1".into()), 7)])
        );
    }

    #[test]
    fn design_body_selectors_prefer_exact_keys_then_fall_back_to_ordinals() {
        let native_key = |ordinal, key| BodyNativeKey {
            id: format!("f3d:asm:body-native-key#{ordinal}"),
            body: BodyId(format!("f3d:brep:entity#{ordinal}")),
            record_index: ordinal,
            body_ordinal: ordinal,
            source_brep: Some("BREP.source.smb".into()),
            asm_body_key: Some(key),
        };
        let mut brep = Brep {
            asm: AsmBrep {
                body_native_keys: vec![native_key(0, 1), native_key(1, 0)],
                ..AsmBrep::default()
            },
            ..Brep::default()
        };

        assert_eq!(
            brep.body_selectors_for(&HashSet::from([0])).unwrap(),
            HashMap::from([(BodyId("f3d:brep:entity#1".into()), 0)])
        );

        brep.asm.body_native_keys = vec![native_key(0, 436)];
        assert_eq!(
            brep.body_selectors_for(&HashSet::from([0])).unwrap(),
            HashMap::from([(BodyId("f3d:brep:entity#0".into()), 0)])
        );
    }
}
