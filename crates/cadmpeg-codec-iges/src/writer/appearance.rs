// SPDX-License-Identifier: Apache-2.0
//! Appearance facts preserved by emitted IGES surface colors.

use super::{
    display_color, finite, number, BodyColor, BodyPresentation, CadIr, Entity, EntityStatus,
    FiniteReal, IgesLossCode, LossNote,
};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn append_color_definitions(
    ir: &CadIr,
    entities: &mut Vec<Entity>,
    presentations: &mut BTreeMap<usize, BodyPresentation>,
) -> Result<(), CodecError> {
    let mut written = BTreeMap::new();
    let mut definition_presentations = Vec::new();
    for (owner, presentation) in presentations.iter_mut() {
        let Some(entity) = entities.get(*owner) else {
            continue;
        };
        let (color, appearance) = match presentation.color {
            BodyColor::Custom(color) => (
                color,
                bound_appearance(ir, entity, |candidate| candidate == color),
            ),
            BodyColor::Standard(number) if number > 0 => {
                let Some(appearance) = bound_appearance(
                    ir,
                    entity,
                    |candidate| matches!(display_color(Some(candidate), "appearance", &mut Vec::new()), BodyColor::Standard(value) if value == number),
                ) else {
                    continue;
                };
                let Some(color) = appearance.base_color else {
                    continue;
                };
                let shared_rgb = ir
                    .model
                    .appearances
                    .iter()
                    .filter(|other| other.base_color == Some(color))
                    .count()
                    > 1;
                if appearance.name.is_none() && !shared_rgb {
                    continue;
                }
                (color, Some(appearance))
            }
            _ => continue,
        };
        if let Some(index) = appearance
            .and_then(|appearance| written.get(appearance.id.as_str()))
            .copied()
        {
            presentation.color = BodyColor::Definition(index);
            continue;
        }
        let name = appearance
            .and_then(|appearance| appearance.name.as_deref())
            .filter(|name| {
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_graphic() || byte == b' ')
            })
            .map(|name| format!("{}H{name}", name.len()))
            .unwrap_or_default();
        let mut components = [color.r(), color.g(), color.b()]
            .map(|component| finite(f64::from(component) * 100.0, "color percentage"))
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        let entity_index = entities.len();
        let mut definition = Entity {
            structure: None,
            property_links: Vec::new(),
            association_links: Vec::new(),
            line_font: None,
            source_targets: Vec::new(),
            type_code: 314,
            form: 0,
            label: String::new(),
            status: EntityStatus::Definition,
            subscript: 0,
            parameter_body: format!(
                "{},{},{},{name};",
                number(components[0]),
                number(components[1]),
                number(components[2])
            )
            .into_bytes(),
            transform: None,
            transform_link: None,
        };
        if let Some(appearance) = appearance {
            if let Some(suffix) = appearance
                .id
                .as_str()
                .strip_prefix("iges:appearance:color#D")
            {
                let id = format!("iges:presentation:color#D{suffix}");
                if let Some(record) = ir
                    .native
                    .namespace("iges")
                    .and_then(|namespace| namespace.arenas().get("colors"))
                    .and_then(|records| records.iter().find(|record| record.id() == id))
                {
                    let channels = [color.r(), color.g(), color.b()];
                    for (index, key) in ["red_percent", "green_percent", "blue_percent"]
                        .into_iter()
                        .enumerate()
                    {
                        if let Some(value) = record
                            .field(key)
                            .and_then(|value| value.as_f64())
                            .filter(|value| {
                                cadmpeg_core::convert::f32_from_f64(*value / 100.0)
                                    == Some(channels[index])
                            })
                            .and_then(FiniteReal::new)
                        {
                            components[index] = value;
                        }
                    }
                    definition.parameter_body = format!(
                        "{},{},{},{name};",
                        number(components[0]),
                        number(components[1]),
                        number(components[2])
                    )
                    .into_bytes();
                    super::native_records::numeric::retain(ir, record, &mut definition);
                    if let Some(source) = record
                        .field("source_entity")
                        .and_then(|value| value.as_str().map(str::to_owned))
                    {
                        if let Some(directory) = ir
                            .native
                            .namespace("iges")
                            .and_then(|namespace| namespace.arenas().get("entities"))
                            .and_then(|records| records.iter().find(|entry| entry.id() == source))
                        {
                            if let Some(label) =
                                directory
                                    .field("label")
                                    .and_then(|value| {
                                        serde_json::from_value::<
                                            cadmpeg_ir::native::bytes::NativeBytes,
                                        >(value)
                                        .ok()
                                    })
                                    .and_then(|bytes| {
                                        String::from_utf8(bytes.as_ref().to_vec()).ok()
                                    })
                                    .filter(|label| {
                                        label.len() <= 8
                                            && label
                                                .bytes()
                                                .all(|byte| byte.is_ascii_graphic() || byte == b' ')
                                    })
                            {
                                definition.label = label.trim().to_owned();
                            }
                        }
                        definition.source_targets.push(source);
                    }
                    definition.source_targets.push(record.id().to_owned());
                    if let Some(number) = record
                        .field("fallback_color_number")
                        .and_then(|value| value.as_i64())
                        .filter(|number| (1..=8).contains(number))
                    {
                        definition_presentations.push((
                            entity_index,
                            BodyPresentation {
                                label: None,
                                name: None,
                                color: BodyColor::Standard(number),
                                visible: None,
                            },
                        ));
                    }
                }
            }
        }
        entities.push(definition);
        if let Some(appearance) = appearance {
            written.insert(appearance.id.as_str(), entity_index);
        }
        presentation.color = BodyColor::Definition(entity_index);
    }
    presentations.extend(definition_presentations);
    Ok(())
}

/// Report appearance facts for which no equivalent Directory color was emitted.
pub(super) fn coverage(
    ir: &CadIr,
    entities: &[Entity],
    presentations: &BTreeMap<usize, BodyPresentation>,
) -> (
    cadmpeg_ir::codec::write::ArenaDisposition,
    cadmpeg_ir::codec::write::ArenaDisposition,
    Vec<LossNote>,
) {
    use cadmpeg_ir::appearance::AppearanceTarget;
    use cadmpeg_ir::codec::write::ArenaDisposition;
    let mut written_bindings = BTreeSet::new();
    let mut written_appearances = BTreeSet::new();
    for binding in &ir.model.appearance_bindings {
        let target = binding
            .target
            .typed_record_target(&ir.model)
            .unwrap_or_else(|| binding.target.clone());
        let (owner, object_type) = match &target {
            AppearanceTarget::Body(id) => (id.as_str(), Some("Body")),
            AppearanceTarget::Face(id) => (id.as_str(), Some("Face")),
            AppearanceTarget::Surface(id) => (id.as_str(), None),
            AppearanceTarget::Curve(id) => (id.as_str(), None),
            AppearanceTarget::Point(id) => (id.as_str(), None),
            _ => continue,
        };
        if binding.source_entity_id.is_some()
            || binding.object_type.as_deref() != object_type
            || binding.visible.is_some()
            || !binding.channels.is_empty()
            || ir
                .model
                .appearance_bindings
                .iter()
                .filter(|other| other.target == binding.target)
                .count()
                != 1
        {
            continue;
        }
        let Some(appearance) = ir
            .model
            .appearances
            .iter()
            .find(|appearance| appearance.id == binding.appearance)
        else {
            continue;
        };
        let Some(color) = appearance.base_color.filter(|color| color.a() == 1.0) else {
            continue;
        };
        if appearance.asset_guid.is_some()
            || appearance.library_id.is_some()
            || appearance.visual_guid.is_some()
            || appearance.physical_token.is_some()
            || appearance
                .schema
                .as_deref()
                .is_some_and(|schema| schema != "IGES color")
            || appearance.category.is_some()
            || !appearance.properties.is_empty()
            || !appearance.textures.is_empty()
        {
            continue;
        }
        let expected = display_color(Some(color), "surface", &mut Vec::new());
        let written = entities.iter().enumerate().any(|(index, entity)| {
            if !entity.source_targets.iter().any(|id| id == owner) {
                return false;
            }
            let Some(presentation) = presentations.get(&index) else {
                return false;
            };
            match (expected, presentation.color) {
                (BodyColor::Standard(left), BodyColor::Standard(right)) => {
                    left == right && appearance.name.is_none()
                }
                (_, BodyColor::Definition(index)) => {
                    let name = match appearance.name.as_deref() {
                        None => String::new(),
                        Some(name)
                            if !name.is_empty()
                                && name
                                    .bytes()
                                    .all(|byte| byte.is_ascii_graphic() || byte == b' ') =>
                        {
                            format!("{}H{name}", name.len())
                        }
                        Some(_) => return false,
                    };
                    entities.get(index).is_some_and(|definition| {
                        let Ok(body) = std::str::from_utf8(&definition.parameter_body) else {
                            return false;
                        };
                        let Some(body) = body.strip_suffix(';') else {
                            return false;
                        };
                        let mut parts = body.splitn(4, ',');
                        let values = std::array::from_fn::<_, 3, _>(|_| {
                            parts
                                .next()
                                .and_then(|value| {
                                    value.replace(['D', 'd'], "E").parse::<f64>().ok()
                                })
                                .and_then(|value| {
                                    cadmpeg_core::convert::f32_from_f64(value / 100.0)
                                })
                        });
                        definition.type_code == 314
                            && parts.next() == Some(name.as_str())
                            && values == [Some(color.r()), Some(color.g()), Some(color.b())]
                    })
                }
                _ => false,
            }
        });
        if written {
            written_bindings.insert(binding.id.as_str());
            written_appearances.insert(appearance.id.as_str());
        }
    }
    let disposition = |total, written| {
        if total == written {
            ArenaDisposition::Written
        } else if written == 0 {
            ArenaDisposition::Dropped
        } else {
            ArenaDisposition::Reported
        }
    };
    let binding_coverage = disposition(ir.model.appearance_bindings.len(), written_bindings.len());
    let appearance_coverage = disposition(ir.model.appearances.len(), written_appearances.len());
    let mut losses: Vec<_> = ir
        .model
        .appearance_bindings
        .iter()
        .filter(|binding| !written_bindings.contains(binding.id.as_str()))
        .map(|binding| {
            IgesLossCode::WriterArenaOmitted.note(format!(
                "IGES omits unsupported appearance_bindings record {}",
                binding.id
            ))
        })
        .chain(
            ir.model
                .appearances
                .iter()
                .filter(|appearance| !written_appearances.contains(appearance.id.as_str()))
                .map(|appearance| {
                    IgesLossCode::WriterArenaOmitted.note(format!(
                        "IGES omits unsupported appearances record {}",
                        appearance.id
                    ))
                }),
        )
        .collect();
    let mut seen = BTreeSet::new();
    for binding in &ir.model.appearance_bindings {
        if !seen.insert(binding.id.as_str()) {
            losses.push(IgesLossCode::WriterArenaOmitted.note(format!(
                "IGES omits duplicate appearance_bindings record {}",
                binding.id
            )));
        }
    }
    (appearance_coverage, binding_coverage, losses)
}

/// Find the unique current appearance assigned to this emitted carrier.
fn bound_appearance<'a>(
    ir: &'a CadIr,
    entity: &Entity,
    matches_color: impl Fn(cadmpeg_ir::topology::Color) -> bool,
) -> Option<&'a cadmpeg_ir::appearance::Appearance> {
    use cadmpeg_ir::appearance::AppearanceTarget;
    let mut matches = ir.model.appearance_bindings.iter().filter_map(|binding| {
        let target = binding
            .target
            .typed_record_target(&ir.model)
            .unwrap_or_else(|| binding.target.clone());
        let id = match &target {
            AppearanceTarget::Body(id) => id.as_str(),
            AppearanceTarget::Face(id) => id.as_str(),
            AppearanceTarget::Surface(id) => id.as_str(),
            AppearanceTarget::Curve(id) => id.as_str(),
            AppearanceTarget::Point(id) => id.as_str(),
            _ => return None,
        };
        if !entity.source_targets.iter().any(|target| target == id) {
            return None;
        }
        ir.model.appearances.iter().find(|appearance| {
            appearance.id == binding.appearance && appearance.base_color.is_some_and(&matches_color)
        })
    });
    let appearance = matches.next()?;
    if matches.any(|other| other.id != appearance.id) {
        return None;
    }
    Some(appearance)
}
