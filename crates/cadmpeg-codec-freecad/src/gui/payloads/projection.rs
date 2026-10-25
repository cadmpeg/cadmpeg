// SPDX-License-Identifier: Apache-2.0
//! Neutral GUI lists select the current typed interpretation of their member.

use super::PayloadProperty;
use crate::native::entry_contents::EntryContents;
use crate::native::EntryRecord;
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, HashMap};

pub(crate) fn matches(
    contents: &EntryContents,
    kind: &str,
    values: &[crate::native::ValueRecord],
) -> bool {
    match (kind, contents) {
        ("App::PropertyColorList", EntryContents::Color { .. })
        | ("App::PropertyFloatList", EntryContents::Float { .. })
        | ("App::PropertyVectorList", EntryContents::Vector { .. })
        | ("App::PropertyPlacementList", EntryContents::Placement { .. })
        | ("Part::PropertyFilletEdges", EntryContents::Fillet { .. })
        | ("Mesh::PropertyMaterial", EntryContents::MeshMaterial { .. }) => true,
        ("App::PropertyMaterialList", EntryContents::Material { version, .. }) => {
            values
                .iter()
                .find(|value| value.tag == "MaterialList")
                .and_then(|value| value.attributes.get("version"))
                .map_or(Some(0), |version| version.parse::<u32>().ok())
                == Some(*version)
        }
        (kind, EntryContents::F32 { values }) => {
            super::auxiliary::width(kind)
                == Some(match values {
                    crate::native::entry_contents::F32Values::Scalar(_) => 1,
                    crate::native::entry_contents::F32Values::Vector(_) => 3,
                    crate::native::entry_contents::F32Values::Curvature(_) => 8,
                })
        }
        _ => false,
    }
}
pub(crate) fn for_property<'a>(
    entries: &'a [EntryRecord],
    property: PayloadProperty<'_>,
) -> Vec<(&'a EntryRecord, &'a EntryContents)> {
    entries
        .iter()
        .filter(|entry| {
            property
                .side_entries
                .iter()
                .any(|name| name == entry.name())
                && property.values.iter().any(|value| {
                    value
                        .attributes
                        .get("file")
                        .is_some_and(|file| file == entry.name())
                })
        })
        .flat_map(|entry| {
            entry
                .contents()
                .iter()
                .filter(|contents| matches(contents, property.type_name, property.values))
                .map(move |contents| (entry, contents))
        })
        .collect()
}
pub(in crate::gui) struct MaterialList {
    pub(in crate::gui) entry: String,
    pub(in crate::gui) version: u32,
    pub(in crate::gui) count: usize,
    pub(in crate::gui) materials: BTreeMap<usize, super::super::GuiMaterial>,
}
pub(in crate::gui) fn material_lists(
    ir: &cadmpeg_ir::CadIr,
    graph: &super::super::Graph,
    alpha: bool,
) -> Result<HashMap<String, MaterialList>, CodecError> {
    let entries = ir
        .native
        .namespace("fcstd")
        .map(|namespace| namespace.arena_as::<EntryRecord>("entries"))
        .transpose()?
        .unwrap_or_default();
    let mut lists = HashMap::new();
    for property in graph
        .properties
        .iter()
        .filter(|property| property.type_name == "App::PropertyMaterialList")
    {
        for (entry, contents) in for_property(&entries, property.into()) {
            let EntryContents::Material { version, values } = contents else {
                continue;
            };
            let mut materials = BTreeMap::new();
            for (index, slot) in values.iter().enumerate() {
                let Some(fixed) = &slot.fixed else { continue };
                materials.insert(
                    index,
                    super::super::GuiMaterial {
                        ambient: super::super::convert_packed_alpha(fixed.ambient, alpha),
                        diffuse: super::super::convert_packed_alpha(fixed.diffuse, alpha),
                        specular: super::super::convert_packed_alpha(fixed.specular, alpha),
                        emissive: super::super::convert_packed_alpha(fixed.emissive, alpha),
                        shininess: fixed.shininess,
                        transparency: fixed.transparency,
                        image: slot.image.clone(),
                        image_path: slot.image_path.clone(),
                        uuid: slot.uuid.clone(),
                    },
                );
            }
            lists.insert(
                property.id.clone(),
                MaterialList {
                    entry: entry.id().to_owned(),
                    version: *version,
                    count: values.len(),
                    materials,
                },
            );
        }
    }
    Ok(lists)
}
