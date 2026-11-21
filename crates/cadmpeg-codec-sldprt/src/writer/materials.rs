// SPDX-License-Identifier: Apache-2.0
//! The `SWObjects` material records a semantic write generates.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::convert::truncate_f64_to_u8;
use cadmpeg_ir::appearance::AppearanceTarget;
use cadmpeg_ir::codec::write::capacity::{TextCapacity, TextUnit};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::schema::EntityKind;
use cadmpeg_ir::topology::Color;

use crate::loss::{NamedCount, SldprtLossCode};

/// A material record's name: a `CString` whose u8 count counts UTF-16 code
/// units.
pub(super) const MATERIAL_NAME: TextCapacity =
    TextCapacity::new("material name", TextUnit::Utf16CodeUnit, 255);

/// One material record and the appearance or body it is written from.
#[derive(Clone, Copy, Debug)]
pub(super) struct Material<'a> {
    pub(super) source: &'a str,
    pub(super) name: &'a str,
    pub(super) color: Color,
}

impl<'a> Material<'a> {
    fn record(&self) -> (&'a str, Color) {
        (self.name, self.color)
    }
}

/// Why no material record is written for an appearance or body.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum MaterialSkip {
    /// The appearance states no base colour.
    NoBaseColor,
    /// The part already has a body material; `SWObjects` holds one.
    SecondBodyMaterial,
}

impl MaterialSkip {
    const fn stated(self) -> &'static str {
        match self {
            Self::NoBaseColor => "state no base colour",
            Self::SecondBodyMaterial => {
                "state a second body material, and SWObjects holds one part material"
            }
        }
    }
}

/// The material records of a document, in write order, and the appearances
/// and bodies that give none.
#[derive(Debug, Default)]
pub(super) struct Materials<'a> {
    pub(super) records: Vec<Material<'a>>,
    pub(super) skipped: Vec<(&'a str, MaterialSkip)>,
}

impl<'a> Materials<'a> {
    /// The records as the material digest hashes them.
    pub(super) fn digest_view(&self) -> Vec<(&'a str, Color)> {
        self.records.iter().map(Material::record).collect()
    }
}

/// The part material from the body bindings, else from the body colours,
/// then one record per distinct `moVisualProperties_c` appearance.
pub(super) fn swobjects_materials(ir: &CadIr) -> Materials<'_> {
    let appearances = ir
        .model
        .appearances
        .iter()
        .map(|appearance| (&appearance.id, appearance))
        .collect::<HashMap<_, _>>();
    let mut materials = Materials::default();
    let mut body = None;
    for binding in &ir.model.appearance_bindings {
        let AppearanceTarget::Body(_) = &binding.target.entity else {
            continue;
        };
        let Some(appearance) = appearances.get(&binding.appearance) else {
            continue;
        };
        let Some(color) = appearance.base_color else {
            materials
                .skipped
                .push((appearance.id.as_str(), MaterialSkip::NoBaseColor));
            continue;
        };
        let material = Material {
            source: appearance.id.as_str(),
            name: appearance.name.as_deref().unwrap_or("Material"),
            color,
        };
        select_body_material(&mut body, material, &mut materials.skipped);
    }
    if body.is_none() {
        for part_body in &ir.geometry.bodies {
            let Some(color) = part_body.color else {
                continue;
            };
            let material = Material {
                source: part_body.id.as_str(),
                name: part_body.name.as_deref().unwrap_or("Material"),
                color,
            };
            select_body_material(&mut body, material, &mut materials.skipped);
        }
    }
    materials.records.extend(body);
    for appearance in &ir.model.appearances {
        if appearance.schema.as_deref() != Some("moVisualProperties_c") {
            continue;
        }
        let Some(color) = appearance.base_color else {
            materials
                .skipped
                .push((appearance.id.as_str(), MaterialSkip::NoBaseColor));
            continue;
        };
        let material = Material {
            source: appearance.id.as_str(),
            name: appearance.name.as_deref().unwrap_or("Material"),
            color,
        };
        if materials
            .records
            .iter()
            .all(|written| written.record() != material.record())
        {
            materials.records.push(material);
        }
    }
    materials
}

/// Make `material` the part material, or skip it when another one is.
fn select_body_material<'a>(
    body: &mut Option<Material<'a>>,
    material: Material<'a>,
    skipped: &mut Vec<(&'a str, MaterialSkip)>,
) {
    match body {
        Some(current) if current.record() != material.record() => {
            skipped.push((material.source, MaterialSkip::SecondBodyMaterial));
        }
        Some(_) => {}
        None => *body = Some(material),
    }
}

/// The generated `SWObjects` material member and its losses.
pub(super) struct MaterialsPayload {
    pub(super) bytes: Vec<u8>,
    /// `export:value.unwritable` entries for names cut to their field.
    pub(super) cut: Vec<String>,
    /// One `WriterMaterialsOmitted` note per skip reason.
    pub(super) omitted: Vec<LossNote>,
}

pub(super) fn materials_payload(ir: &CadIr) -> MaterialsPayload {
    let materials = swobjects_materials(ir);
    let mut bytes = Vec::new();
    let mut cut = Vec::new();
    for material in &materials.records {
        let name = MATERIAL_NAME.fit(material.name);
        cut.extend(name.unwritable_entry(material.source));
        bytes.extend(material_payload(name.text, material.color));
    }
    let mut grouped = BTreeMap::<MaterialSkip, NamedCount>::new();
    for (source, reason) in &materials.skipped {
        grouped.entry(*reason).or_default().push(source);
    }
    let omitted = grouped
        .into_iter()
        .map(|(reason, named)| {
            SldprtLossCode::WriterMaterialsOmitted.note(format!(
                "{} appearance or body record(s) {}, so the SLDPRT semantic exporter wrote no \
                 SWObjects material for them: {named}",
                named.count(),
                reason.stated()
            ))
        })
        .collect();
    MaterialsPayload {
        bytes,
        cut,
        omitted,
    }
}

/// One `moVisualProperties_c` record. `name` fits [`MATERIAL_NAME`].
fn material_payload(name: &str, color: Color) -> Vec<u8> {
    let length = u8::try_from(name.encode_utf16().count())
        .expect("MATERIAL_NAME.width fits the u8 count (writer::tests::capacity)");
    let mut out = b"moVisualProperties_c".to_vec();
    let component = |value: f64| {
        let component = truncate_f64_to_u8((value * 255.0).round());
        debug_assert!(component.is_some(), "Color components lie in [0, 1]");
        component.unwrap_or_default()
    };
    out.extend_from_slice(&[
        component(color.r()),
        component(color.g()),
        component(color.b()),
        0,
    ]);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0x00c0_c0c0u32.to_le_bytes());
    out.extend_from_slice(&[0xff, 0xfe, 0xff, 0x00]);
    out.extend_from_slice(&[0xff, 0xfe, 0xff, length]);
    for unit in name.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
}

/// `export:edit.dropped` for a write that keeps the retained `SWObjects`
/// materials while the document's materials differ from them: each material
/// record added, removed or changed against the `baseline` decode, or every
/// record when there is no baseline.
pub(super) fn retained_materials_kept(ir: &CadIr, baseline: Option<&CadIr>) -> Option<LossNote> {
    let current = swobjects_materials(ir);
    let changed = match baseline {
        Some(baseline) => {
            let original = swobjects_materials(baseline);
            let records = |materials: &Materials<'_>| {
                materials
                    .records
                    .iter()
                    .map(|material| {
                        (
                            material.source.to_owned(),
                            material.name.to_owned(),
                            material.color,
                        )
                    })
                    .collect::<Vec<_>>()
            };
            let (before, after) = (records(&original), records(&current));
            before
                .iter()
                .filter(|record| !after.contains(record))
                .chain(after.iter().filter(|record| !before.contains(record)))
                .map(|(source, ..)| source.clone())
                .collect::<BTreeSet<_>>()
        }
        None => current
            .records
            .iter()
            .map(|material| material.source.to_owned())
            .collect(),
    };
    let named = if changed.is_empty() {
        BTreeSet::from(["SWObjects".to_owned()])
    } else {
        changed
    };
    cadmpeg_ir::codec::write::edit_dropped(
        EntityKind::Appearance,
        "SLDPRT",
        if baseline.is_none() {
            "keeps the retained SWObjects materials and writes no material edit; with no source \
             decode it cannot tell which record changed"
        } else {
            "keeps the retained SWObjects materials and writes no material edit"
        },
        named,
    )
}
