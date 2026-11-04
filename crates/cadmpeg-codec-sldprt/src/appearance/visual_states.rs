// SPDX-License-Identifier: Apache-2.0
//! Scoped native appearance ownership.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::ids::{BodyId, FaceId};

use super::AppearanceDefinition;
use crate::brep::attrib::NativeIdentity;
use crate::brep::feature_source::FeatureSourceId;
use crate::container::{ContainerScan, Section};
use crate::layout::{
    visual_component_body_prefix as body_prefix, visual_states_feature_appearance_prefix as record,
};
use crate::records::{FeatureHistory, FeatureInputLane};

#[derive(Clone, Copy)]
pub(crate) struct Inputs<'a> {
    pub(crate) model: &'a Model,
    pub(crate) histories: &'a [FeatureHistory],
    pub(crate) lanes: &'a [FeatureInputLane],
    pub(crate) faces: &'a [NativeIdentity<FaceId>],
    pub(crate) bodies: &'a [NativeIdentity<BodyId>],
}

#[derive(Clone)]
pub(crate) struct Choice {
    pub(crate) rank: u8,
    pub(crate) ordinal: usize,
    pub(crate) definition: Option<AppearanceDefinition>,
}

#[derive(Default)]
pub(crate) struct Plan {
    pub(crate) faces: BTreeMap<FaceId, Choice>,
    pub(crate) feature_sources: BTreeSet<FeatureSourceId>,
    pub(crate) unresolved: usize,
    pub(crate) unresolved_details: Vec<super::UnresolvedAppearanceBinding>,
}

fn merge(choices: &mut BTreeMap<FaceId, Choice>, face: FaceId, incoming: Choice) {
    match choices.entry(face) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(incoming);
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            let previous = entry.get_mut();
            if incoming.rank > previous.rank {
                *previous = incoming;
            } else if incoming.rank == previous.rank
                && previous.definition.as_ref().map(|d| d.color)
                    != incoming.definition.as_ref().map(|d| d.color)
            {
                previous.definition = None;
            }
        }
    }
}

fn framed_string(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: usize,
) -> Result<Option<(String, usize)>, CodecError> {
    let Some(marker_end) = at.checked_add(3) else {
        return Ok(None);
    };
    if bytes.get(at..marker_end) != Some(&[0xff, 0xfe, 0xff]) {
        return Ok(None);
    }
    let Some(count) = bytes.get(at + 3).map(|n| usize::from(*n)) else {
        return Ok(None);
    };
    let end = at + 4 + count * 2;
    let Some(raw) = bytes.get(at + 4..end) else {
        return Ok(None);
    };
    match ctx.utf16le_text(raw, count, false, "decode native appearance state key") {
        Ok(value) => Ok(Some((value, end))),
        Err(CodecError::Malformed(_)) => Ok(None),
        Err(error) => Err(error),
    }
}

fn selected_bodies(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    model: &Model,
) -> Result<Option<BTreeSet<BodyId>>, CodecError> {
    let classes = crate::tessellation::class_intervals(bytes);
    let mut keys = Vec::new();
    for class in classes
        .iter()
        .filter(|class| class.name == "visStateKeyData_c")
    {
        let part = &bytes[class.content.start()..class.content.end()];
        for (at, marker) in part.windows(3).enumerate() {
            if marker != [0xff, 0xfe, 0xff] {
                continue;
            }
            if let Some((key, _)) = framed_string(ctx, part, at)? {
                if !key.is_empty() {
                    keys.push(key);
                }
            }
        }
    }
    let [key] = keys.as_slice() else {
        return Ok(None);
    };
    if !key
        .split_once('_')
        .is_some_and(|(configuration, display)| !configuration.is_empty() && !display.is_empty())
    {
        return Ok(None);
    }
    let matches: Vec<_> = model
        .configurations
        .iter()
        .filter(|configuration| {
            configuration.name.as_deref().is_some_and(|name| {
                [format!("{name}_"), format!("<{name}>_")]
                    .iter()
                    .any(|prefix| {
                        key.strip_prefix(prefix)
                            .is_some_and(|display| !display.is_empty())
                    })
            })
        })
        .collect();
    let configuration = match matches.as_slice() {
        [configuration] => *configuration,
        _ => return Ok(None),
    };
    if !configuration.active {
        return Ok(None);
    }
    Ok(configuration
        .bodies
        .as_deref()
        .map(|bodies| bodies.iter().cloned().collect()))
}

fn faces_of_bodies(model: &Model, bodies: &BTreeSet<BodyId>) -> BTreeSet<FaceId> {
    let regions: BTreeSet<_> = model
        .bodies
        .iter()
        .filter(|body| bodies.contains(&body.id))
        .flat_map(|body| &body.regions)
        .collect();
    let shells: BTreeSet<_> = model
        .regions
        .iter()
        .filter(|region| regions.contains(&region.id))
        .flat_map(|region| &region.shells)
        .collect();
    model
        .faces
        .iter()
        .filter(|face| shells.contains(&face.shell))
        .map(|face| face.id.clone())
        .collect()
}

fn body_identity(bytes: &[u8]) -> Option<u32> {
    let bytes = bytes.get(..body_prefix::LEN)?;
    let token = View::u16_le_at(bytes, body_prefix::CLASS_TOKEN)?;
    let identity = View::u32_le_at(bytes, body_prefix::IDENTITY)?;
    (token > 0x8000
        && token != u16::MAX
        && bytes.get(
            body_prefix::ZERO_PADDING
                ..body_prefix::ZERO_PADDING + body_prefix::ZERO_PADDING_VALUE.len(),
        ) == Some(&body_prefix::ZERO_PADDING_VALUE)
        && identity != 0
        && identity != u32::MAX
        && View::u32_le_at(bytes, body_prefix::IDENTITY_COPY) == Some(identity))
    .then_some(identity)
}

fn record_definition(
    section: Section<'_>,
    bytes: &[u8],
    at: usize,
) -> Option<AppearanceDefinition> {
    let value = bytes.get(at..at.checked_add(record::LEN)?)?;
    if View::u32_le_at(value, record::SELECTOR_ONE_A) != Some(record::SELECTOR_ONE_A_VALUE)
        || View::u32_le_at(value, record::SELECTOR_ONE_B) != Some(record::SELECTOR_ONE_B_VALUE)
        || View::u32_le_at(value, record::SELECTOR_TWO) != Some(record::SELECTOR_TWO_VALUE)
        || value.get(record::INSTANCE_PREFIX..record::MARKER)
            != Some(&record::INSTANCE_PREFIX_VALUE)
        || value.get(record::MARKER..record::PACKED_COLOR) != Some(&record::MARKER_VALUE)
    {
        return None;
    }
    let packed = View::u32_le_at(value, record::PACKED_COLOR)?;
    (packed >> 24 == 0).then(|| AppearanceDefinition {
        name: "SolidWorks scoped appearance".into(),
        color: super::packed_rgb(packed),
        packed_hex: format!("{packed:08x}"),
        source_name: section.source_stream().clone(),
        record_offset: at,
    })
}

#[derive(Clone)]
struct FeatureFaces {
    resolved: BTreeSet<FaceId>,
    uncertain: BTreeSet<FaceId>,
}

fn feature_faces(inputs: &Inputs<'_>, source: u32, timestamp: u32) -> Option<FeatureFaces> {
    let candidates: Vec<_> = inputs
        .histories
        .iter()
        .flat_map(|history| &history.features)
        .filter(|feature| feature.source_value() == Some(source))
        .collect();
    let [feature] = candidates.as_slice() else {
        return None;
    };
    let mut model_features = inputs
        .model
        .features
        .iter()
        .filter(|candidate| candidate.native_ref.as_deref() == Some(feature.id.as_str()));
    model_features.next()?;
    if model_features.next().is_some() {
        return None;
    }
    let mut objects = Vec::new();
    for lane in inputs.lanes {
        if let Some(name) =
            crate::resolved_features::scalars::FeatureObjectNames::new(&lane.names).of(feature)
        {
            objects.push((lane, name));
        }
    }
    let [(lane, name)] = objects.as_slice() else {
        return None;
    };
    let start = usize::try_from(name.offset).ok()?;
    let names = crate::resolved_features::scalars::FeatureObjectNames::new(&lane.names);
    let end = crate::resolved_features::scalars::FeatureObjectStarts::new(inputs.histories, &names)
        .end(name.offset, lane.native_payload.len());
    let object = lane.native_payload.get(start..end)?;
    let faces: BTreeSet<_> = inputs
        .faces
        .iter()
        .filter(|identity| {
            identity.values.get(1) == Some(&source) && identity.values.get(2) == Some(&timestamp)
        })
        .map(|identity| identity.target.clone())
        .collect();
    let mut uncertain_sources = BTreeSet::new();
    let mut unsupported_path = false;
    for class in crate::tessellation::class_intervals(object)
        .into_iter()
        .filter(|class| class.name == "moCompFace_c")
    {
        let part = &object[..class.content.end()];
        let mut paths = 0;
        let Some(last_marker) = class.content.end().checked_sub(15) else {
            continue;
        };
        for at in class.content.start()..last_marker {
            if part.get(at..at + 16)
                != Some(&crate::resolved_features::selections::COMPACT_EDGE_VECTOR_MARKER)
            {
                continue;
            }
            if let Some(components) =
                crate::resolved_features::selections::component_vector_path_at(part, at)
            {
                paths += 1;
                for component in components {
                    if let Some(pair) = View::u32_le_at(component.type_signature.as_ref(), 4)
                        .zip(View::u32_le_at(component.type_signature.as_ref(), 8))
                    {
                        uncertain_sources.insert(pair);
                    }
                }
            } else {
                unsupported_path = true;
            }
        }
        unsupported_path |= paths == 0;
    }
    let uncertain = inputs
        .faces
        .iter()
        .filter(|identity| {
            !faces.contains(&identity.target)
                && (unsupported_path
                    || identity
                        .values
                        .get(1)
                        .zip(identity.values.get(2))
                        .is_some_and(|(source, timestamp)| {
                            uncertain_sources.contains(&(*source, *timestamp))
                        }))
        })
        .map(|identity| identity.target.clone())
        .collect();
    (!faces.is_empty()).then_some(FeatureFaces {
        resolved: faces,
        uncertain,
    })
}

pub(crate) fn resolve(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    inputs: Inputs<'_>,
) -> Result<Plan, CodecError> {
    let mut plan = Plan::default();
    let eligible_pairs: BTreeSet<_> = inputs
        .faces
        .iter()
        .filter_map(|identity| Some((*identity.values.get(1)?, *identity.values.get(2)?)))
        .collect();
    let mut feature_cache = BTreeMap::<(u32, u32), Option<FeatureFaces>>::new();
    for section in scan.sections().filter(|section| {
        matches!(
            section.name(),
            Some("ThirdPtyStore/VisualStates" | "ThirdPtyStore/VisualStates__ZLB")
        )
    }) {
        let bytes = section.payload();
        let classes = crate::tessellation::class_intervals(bytes);
        let Some(bodies) = selected_bodies(ctx, bytes, inputs.model)? else {
            plan.unresolved += 1;
            plan.unresolved_details
                .push(super::UnresolvedAppearanceBinding {
                    owner_class: "VisualStates".to_owned(),
                    identity: section
                        .name()
                        .unwrap_or("ThirdPtyStore/VisualStates")
                        .to_owned(),
                    appearance_index: 0,
                    reason: "display-state scope does not resolve to active configuration bodies"
                        .to_owned(),
                    source_offset: 0,
                    packed_hex: None,
                });
            continue;
        };
        let scope = faces_of_bodies(inputs.model, &bodies);
        for (index, class) in classes.iter().enumerate() {
            if class.name == "moVisualProperties_c" {
                let previous: Vec<_> = classes[..index]
                    .iter()
                    .rev()
                    .take(4)
                    .map(|class| class.name.as_str())
                    .collect();
                let chain = previous.starts_with(&[
                    "visStateVisualPropertiesAttribute_c",
                    "moUnitComponent_c",
                    "moCompHolder_c",
                ]) || previous.starts_with(&[
                    "visStateVisualStateAttribute_c",
                    "visStateVisualPropertiesAttribute_c",
                    "moUnitComponent_c",
                    "moCompHolder_c",
                ]) || previous.starts_with(&[
                    "moCompHolder_c",
                    "visStateVisualStateAttribute_c",
                    "visStateVisualPropertiesAttribute_c",
                    "moUnitComponent_c",
                ]);
                let attribute = classes[..index]
                    .iter()
                    .rev()
                    .find(|class| class.name == "visStateVisualPropertiesAttribute_c");
                let attribute_valid = attribute
                    .and_then(|class| bytes.get(class.content.start()..class.content.end()))
                    .is_some_and(|payload| {
                        payload == [1, 0, 0, 0] || payload == [1, 0, 0, 0, 0, 0]
                    });
                if chain && attribute_valid {
                    if let Some(definition) = super::definition_at(
                        ctx,
                        section,
                        class.content.start(),
                        class.class_offset,
                    )? {
                        for face in &scope {
                            merge(
                                &mut plan.faces,
                                face.clone(),
                                Choice {
                                    rank: 1,
                                    ordinal: section.ordinal(),
                                    definition: Some(definition.clone()),
                                },
                            );
                        }
                    }
                }
            }
            let rank = match class.name.as_str() {
                "moCompHolder_c" => 1,
                "moCompSolidBody_c" => 2,
                "moCompFeature_c" => 3,
                "moCompFace_c" => 4,
                name if name.ends_with("SurfIdRep_c") => 4,
                _ => continue,
            };
            let Some(last_record) = class.content.end().checked_sub(record::LEN - 1) else {
                continue;
            };
            for at in class.content.start()..last_record {
                let Some(definition) = record_definition(section, bytes, at) else {
                    continue;
                };
                let targets = match rank {
                    1 => Some(scope.clone()),
                    2 => body_identity(&bytes[class.content.start()..class.content.end()])
                        .and_then(|value| {
                            let candidates: BTreeSet<_> = inputs
                                .bodies
                                .iter()
                                .filter(|identity| {
                                    identity.values.as_slice() == [value]
                                        && bodies.contains(&identity.target)
                                })
                                .map(|identity| identity.target.clone())
                                .collect();
                            (candidates.len() == 1)
                                .then(|| faces_of_bodies(inputs.model, &candidates))
                        }),
                    3 => {
                        let source = View::u32_le_at(bytes, at + record::FEATURE_SOURCE_ID);
                        let timestamp = View::u32_le_at(bytes, at + record::FEATURE_TIMESTAMP);
                        source.zip(timestamp).and_then(|(source, timestamp)| {
                            if !eligible_pairs.contains(&(source, timestamp)) {
                                return None;
                            }
                            let faces = feature_cache
                                .entry((source, timestamp))
                                .or_insert_with(|| feature_faces(&inputs, source, timestamp))
                                .clone()?;
                            if !faces.uncertain.is_empty() {
                                plan.unresolved += 1;
                                plan.unresolved_details.push(
                                    super::UnresolvedAppearanceBinding {
                                        owner_class: "moCompFace_c".to_owned(),
                                        identity: format!("{source}:{timestamp}"),
                                        appearance_index: 0,
                                        reason:
                                            "feature faces contain uncertain members outside the resolved set"
                                                .to_owned(),
                                        source_offset: at,
                                        packed_hex: Some(format!(
                                            "{packed:08x}",
                                            packed = View::u32_le_at(
                                                bytes,
                                                at + record::PACKED_COLOR
                                            )
                                            .unwrap_or(0)
                                        )),
                                    },
                                );
                            }
                            for face in faces.uncertain.intersection(&scope) {
                                merge(
                                    &mut plan.faces,
                                    face.clone(),
                                    Choice {
                                        rank: 3,
                                        ordinal: section.ordinal(),
                                        definition: None,
                                    },
                                );
                            }
                            if let Ok(source) = FeatureSourceId::try_from(source) {
                                plan.feature_sources.insert(source);
                            }
                            Some(faces.resolved)
                        })
                    }
                    _ => None,
                };
                if let Some(targets) = targets {
                    for target in targets.intersection(&scope) {
                        merge(
                            &mut plan.faces,
                            target.clone(),
                            Choice {
                                rank,
                                ordinal: section.ordinal(),
                                definition: Some(definition.clone()),
                            },
                        );
                    }
                } else {
                    plan.unresolved += 1;
                    plan.unresolved_details
                        .push(super::UnresolvedAppearanceBinding {
                            owner_class: class.name.clone(),
                            identity: match rank {
                                2 => "body".to_owned(),
                                3 => View::u32_le_at(bytes, at + record::FEATURE_SOURCE_ID)
                                    .map(|source| source.to_string())
                                    .unwrap_or_default(),
                                _ => "face".to_owned(),
                            },
                            appearance_index: 0,
                            reason: "appearance target does not resolve to in-scope faces"
                                .to_owned(),
                            source_offset: at,
                            packed_hex: View::u32_le_at(bytes, at + record::PACKED_COLOR)
                                .map(|packed| format!("{packed:08x}")),
                        });
                    for target in &scope {
                        merge(
                            &mut plan.faces,
                            target.clone(),
                            Choice {
                                rank,
                                ordinal: section.ordinal(),
                                definition: None,
                            },
                        );
                    }
                }
            }
        }
    }
    plan.unresolved += plan
        .faces
        .values()
        .filter(|choice| choice.definition.is_none())
        .count();
    Ok(plan)
}

#[cfg(test)]
mod tests;
