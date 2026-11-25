// SPDX-License-Identifier: Apache-2.0
//! Body-level visibility (`NoShow`), display naming, and container ownership
//! decoded from `CATPrtCont`, `MainDataStream`, and `CATSeeBodyCont`.
//!
//! In a nested `V5_CFV2` `.CATPart` container:
//! - The outer `CATPrtCont` stream stores a `7C05` entity table, a `7C08` object
//!   graph, a `7C0B` per-entity visualization property block, and a `7C02` UTF-8
//!   schema string catalog.
//! - Each `7C0B` entry corresponds positionally to one `7C05` entity record and
//!   encodes self-describing visualization properties keyed by catalog selectors.
//!   The `VPGlobal` inline property carries explicit `Show`/`NoShow` flags in
//!   byte 0: bit 0 (`0x01`) indicates an explicit show-attribute override, and
//!   bit 3 (`0x08`) marks `NoShow` (hidden) when set.
//! - The inner `MainDataStream` begins with a recursive `BagRep` (`0x62`) /
//!   `3DRep` (`0x95`) scene tree whose `BagRep` identifiers match `CATPrtCont`
//!   entity identities and whose `3DRep` identifiers match `CATSeeBodyCont`
//!   allocation ends in `CGMGeom`.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::ops::Range;

use cadmpeg_core::bytes::find_from;
use cadmpeg_core::decode::{index_from_u32, View};

use super::body_selection;
use super::{
    contiguous_descriptor_range, outer_container_declarations, parse_outer_stream_directory,
    parse_stream_directory, reconstruct_logical_stream, unique_largest_descriptor, ContainerScan,
    Descriptor, InnerDir, OuterContainerDeclaration, OUTER_MAGIC,
};
use crate::catalog;

/// Maximum recursive `BagRep` nesting depth admitted while parsing `MainDataStream`.
const MAX_BAGREP_DEPTH: usize = 64;

/// Maximum number of child entries processed in a single `BagRep` node.
const MAX_BAGREP_CHILDREN: usize = 65_536;

/// Maximum number of `7C05` entity records parsed in a `CATPrtCont` stream.
const MAX_PRTCONT_ENTITIES: usize = 262_144;

/// `VPGlobal` byte-0 bit indicating an explicit `Show`/`NoShow` attribute override.
const VPGLOBAL_SHOW_PRESENT_BIT: u8 = 0x01;

/// `VPGlobal` byte-0 bit indicating `NoShow` (hidden) when [`VPGLOBAL_SHOW_PRESENT_BIT`] is set.
const VPGLOBAL_NOSHOW_BIT: u8 = 0x08;

/// Structural container kind owning a CATIA V5 body in `CATPrtCont`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CatiaBodyContainerKind {
    /// Part Design solid body container (`MechanicalTool`, e.g. `PartBody`, `Body.N`).
    PartBody,
    /// Hybrid body container (`HybridBody`).
    HybridBody,
    /// Wireframe/surface construction set (`GSMTool` / `Open_body`, e.g. `Geometrical Set.N`).
    GeometricalSet,
    /// Standalone or other feature container.
    Other,
}

/// Decoded container ownership, display name, and visibility metadata for one
/// body representation in a `V5_CFV2` `.CATPart`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatiaBodyMetadata {
    /// Decoded display name from `CATPrtCont` (`MMAlias` or feature definition name).
    pub name: Option<String>,
    /// Decoded body-level visibility (`Some(true)` = shown, `Some(false)` = `NoShow`/hidden,
    /// `None` = absent/unresolved).
    pub visible: Option<bool>,
    /// Structural container kind (`PartBody`, `HybridBody`, `GeometricalSet`, `Other`).
    pub container_kind: CatiaBodyContainerKind,
    /// Raw `CATPrtCont` container class name (e.g. `"MechanicalTool"`, `"HybridBody"`, `"GSMTool"`).
    pub container_class: Option<String>,
    /// Owning `CATPrtCont` container entity identity, when resolved.
    pub container_entity_id: Option<u32>,
    /// Owning result feature `CATPrtCont` entity identity, when resolved.
    pub feature_entity_id: Option<u32>,
    /// `CGMGeom` / `CATSeeBodyCont` allocation end referenced by the `3DRep` node.
    pub cgm_allocation_end: Option<u32>,
    /// Logical byte range of the enclosing representation inside `MainDataStream`.
    pub rep_range: Range<usize>,
}

/// Decoded declaration of a body container directly from `CATPrtCont`,
/// including `NoShow` containers whose visual representation may be omitted
/// from `MainDataStream`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatiaPrtContBody {
    /// `CATPrtCont` entity identity.
    pub entity_id: u32,
    /// Structural container kind (`PartBody`, `HybridBody`, `GeometricalSet`).
    pub container_kind: CatiaBodyContainerKind,
    /// Raw `CATPrtCont` class name (`MechanicalTool`, `HybridBody`, `GSMTool`, `Open_body`).
    pub container_class: String,
    /// Decoded display name (`MMAlias` or non-generic subtype name), when present.
    pub name: Option<String>,
    /// Explicit `VPGlobal` visibility (`Some(true)` = `Show`, `Some(false)` = `NoShow`,
    /// `None` = no explicit `Show`/`NoShow` override in `VPGlobal`).
    pub explicit_visible: Option<bool>,
}

#[derive(Debug, Clone)]
struct PrtContEntity {
    entity_id: u32,
    class_name: Option<String>,
    subtype_name: Option<String>,
    alias: Option<String>,
    vp_global_visible: Option<bool>,
}

#[derive(Debug, Default)]
struct PrtContIndex {
    entities_by_id: HashMap<u32, PrtContEntity>,
    entities_ordered: Vec<PrtContEntity>,
    value_block_complete: bool,
}

#[derive(Debug, Clone, Copy)]
struct BagRepAncestor {
    entity_id: u32,
    vis_byte: u8,
}

#[derive(Debug, Clone)]
struct MdsRepNode {
    rep_range: Range<usize>,
    cgm_allocation_end: Option<u32>,
    ancestors: Vec<BagRepAncestor>,
}

/// Decode `VPGlobal` byte-0 visibility bits.
///
/// Returns `Some(false)` for `NoShow`, `Some(true)` for explicit `Show`, and
/// `None` when byte 0 does not carry an explicit show-attribute override.
#[must_use]
pub(crate) fn decode_vp_global_visibility(inline_bytes: &[u8]) -> Option<bool> {
    let flags = *inline_bytes.first()?;
    if flags & VPGLOBAL_SHOW_PRESENT_BIT == 0 {
        return None;
    }
    Some(flags & VPGLOBAL_NOSHOW_BIT == 0)
}

/// Resolve per-population body metadata for standard FBB populations whose face
/// runs start at `fbb_starts` within `scan.main_data_stream()`.
pub(crate) fn resolve_standard_populations(
    scan: &ContainerScan<'_>,
    fbb_starts: &[usize],
) -> Vec<Option<CatiaBodyMetadata>> {
    if fbb_starts.is_empty() {
        return Vec::new();
    }
    let Some(mds) = scan.main_data_stream() else {
        return vec![None; fbb_starts.len()];
    };
    let reps = parse_mds_rep_tree(mds);
    if reps.is_empty() {
        return vec![None; fbb_starts.len()];
    }
    let prt_bytes = declared_outer_stream_bytes(
        &scan.data,
        scan.outer.as_ref(),
        &scan.outer_container_declarations,
        "CATPrtCont",
    );
    let prt_index = prt_bytes
        .as_deref()
        .and_then(parse_prtcont_index)
        .unwrap_or_default();
    let see_body_slots = declared_outer_stream_bytes(
        &scan.data,
        scan.outer.as_ref(),
        &scan.outer_container_declarations,
        "CATSeeBodyCont",
    )
    .as_deref()
    .and_then(body_selection::slots_from_bytes)
    .map(|slots| slots.into_iter().collect::<HashSet<usize>>());

    fbb_starts
        .iter()
        .map(|&fbb_start| {
            let mut matching = reps
                .iter()
                .filter(|rep| rep.rep_range.start <= fbb_start && fbb_start < rep.rep_range.end);
            let rep = matching.next()?;
            if matching.next().is_some() {
                return None;
            }
            Some(build_body_metadata(
                rep,
                &prt_index,
                see_body_slots.as_ref(),
            ))
        })
        .collect()
}

/// Decode body metadata (`name`, `visible`, and `PartBody`/`Geometrical Set`
/// container ownership) for every FBB face population in a complete `V5_CFV2`
/// `.CATPart` image.
#[must_use]
pub fn decode_v5_body_metadata(data: &[u8]) -> Vec<CatiaBodyMetadata> {
    if !data.starts_with(OUTER_MAGIC) {
        return Vec::new();
    }
    let outer = parse_outer_stream_directory(data);
    let declarations = outer
        .as_ref()
        .map(|dir| outer_container_declarations(data, dir))
        .unwrap_or_default();
    let Some(mds) = extract_main_data_stream(data) else {
        return Vec::new();
    };
    let reps = parse_mds_rep_tree(&mds);
    if reps.is_empty() {
        return Vec::new();
    }
    let prt_bytes = declared_outer_stream_bytes(data, outer.as_ref(), &declarations, "CATPrtCont");
    let prt_index = prt_bytes
        .as_deref()
        .and_then(parse_prtcont_index)
        .unwrap_or_default();
    let see_body_slots =
        declared_outer_stream_bytes(data, outer.as_ref(), &declarations, "CATSeeBodyCont")
            .as_deref()
            .and_then(body_selection::slots_from_bytes)
            .map(|slots| slots.into_iter().collect::<HashSet<usize>>());

    reps.iter()
        .filter(|rep| rep_contains_fbb_run(&mds, &rep.rep_range))
        .map(|rep| build_body_metadata(rep, &prt_index, see_body_slots.as_ref()))
        .collect()
}

/// Decode all `PartBody` (`MechanicalTool`), `HybridBody`, and `Geometrical Set`
/// (`GSMTool`/`Open_body`) container declarations from `CATPrtCont`, including
/// hidden `NoShow` containers whose representations are absent from
/// `MainDataStream`.
#[must_use]
pub fn decode_v5_prtcont_bodies(data: &[u8]) -> Vec<CatiaPrtContBody> {
    if !data.starts_with(OUTER_MAGIC) {
        return Vec::new();
    }
    let outer = parse_outer_stream_directory(data);
    let declarations = outer
        .as_ref()
        .map(|dir| outer_container_declarations(data, dir))
        .unwrap_or_default();
    let Some(prt_bytes) =
        declared_outer_stream_bytes(data, outer.as_ref(), &declarations, "CATPrtCont")
    else {
        return Vec::new();
    };
    let Some(prt_index) = parse_prtcont_index(&prt_bytes) else {
        return Vec::new();
    };
    prt_index
        .entities_ordered
        .into_iter()
        .filter_map(|entity| {
            let class_name = entity.class_name?;
            let container_kind = classify_container_class(&class_name);
            if container_kind == CatiaBodyContainerKind::Other {
                return None;
            }
            let name = entity
                .alias
                .or_else(|| entity.subtype_name.filter(|subtype| subtype != &class_name));
            Some(CatiaPrtContBody {
                entity_id: entity.entity_id,
                container_kind,
                container_class: class_name,
                name,
                explicit_visible: entity.vp_global_visible,
            })
        })
        .collect()
}

fn classify_container_class(class_name: &str) -> CatiaBodyContainerKind {
    match class_name {
        "MechanicalTool" => CatiaBodyContainerKind::PartBody,
        "HybridBody" => CatiaBodyContainerKind::HybridBody,
        "GSMTool" | "Open_body" => CatiaBodyContainerKind::GeometricalSet,
        _ => CatiaBodyContainerKind::Other,
    }
}

fn build_body_metadata(
    rep: &MdsRepNode,
    prt_index: &PrtContIndex,
    see_body_slots: Option<&HashSet<usize>>,
) -> CatiaBodyMetadata {
    let mut container_kind = CatiaBodyContainerKind::Other;
    let mut container_class: Option<String> = None;
    let mut container_entity_id: Option<u32> = None;
    let mut feature_entity_id: Option<u32> = None;
    let mut body_name: Option<String> = None;
    let mut explicit_vp_visible: Option<bool> = None;
    let mut any_mds_hidden = false;
    let mut all_mds_shown = !rep.ancestors.is_empty();

    for ancestor in &rep.ancestors {
        if ancestor.vis_byte == 0 {
            any_mds_hidden = true;
            all_mds_shown = false;
        } else if ancestor.vis_byte != 1 {
            all_mds_shown = false;
        }
        let Some(entity) = prt_index.entities_by_id.get(&ancestor.entity_id) else {
            continue;
        };
        if let Some(vis) = entity.vp_global_visible {
            if !vis {
                explicit_vp_visible = Some(false);
            } else if explicit_vp_visible.is_none() {
                explicit_vp_visible = Some(true);
            }
        }
        let Some(class_name) = entity.class_name.as_deref() else {
            continue;
        };
        let kind = classify_container_class(class_name);
        if kind != CatiaBodyContainerKind::Other {
            container_kind = kind;
            container_class = Some(class_name.to_owned());
            container_entity_id = Some(entity.entity_id);
            if let Some(alias) = entity.alias.as_deref().filter(|s| !s.is_empty()) {
                body_name = Some(alias.to_owned());
            } else if let Some(subtype) = entity
                .subtype_name
                .as_deref()
                .filter(|s| !s.is_empty() && *s != class_name)
            {
                body_name = Some(subtype.to_owned());
            }
        } else if class_name != "MechanicalPart" {
            feature_entity_id = Some(entity.entity_id);
            if class_name == "Solid" {
                if let Some(solid_name) = entity
                    .subtype_name
                    .as_deref()
                    .filter(|s| !s.is_empty() && *s != "Solid")
                {
                    let prefer_clean_solid = match body_name.as_deref() {
                        None => true,
                        Some(existing) => existing
                            .strip_prefix(solid_name)
                            .is_some_and(|suffix| suffix.starts_with("__CADM")),
                    };
                    if prefer_clean_solid {
                        body_name = Some(solid_name.to_owned());
                    }
                }
            } else if body_name.is_none() {
                if let Some(alias) = entity.alias.as_deref().filter(|s| !s.is_empty()) {
                    body_name = Some(alias.to_owned());
                }
            }
        }
    }

    let excluded_by_see_body = match (see_body_slots, rep.cgm_allocation_end) {
        (Some(slots), Some(slot)) => usize::try_from(slot)
            .ok()
            .is_some_and(|slot_usize| !slots.contains(&slot_usize)),
        _ => false,
    };

    let visible = if any_mds_hidden || explicit_vp_visible == Some(false) || excluded_by_see_body {
        Some(false)
    } else if explicit_vp_visible == Some(true)
        || (container_kind != CatiaBodyContainerKind::Other
            && prt_index.value_block_complete
            && all_mds_shown)
    {
        Some(true)
    } else {
        None
    };

    CatiaBodyMetadata {
        name: body_name,
        visible,
        container_kind,
        container_class,
        container_entity_id,
        feature_entity_id,
        cgm_allocation_end: rep.cgm_allocation_end,
        rep_range: rep.rep_range.clone(),
    }
}

fn declared_outer_stream_bytes<'a>(
    data: &'a [u8],
    outer: Option<&InnerDir>,
    declarations: &[OuterContainerDeclaration],
    class_name: &str,
) -> Option<Cow<'a, [u8]>> {
    let directory = outer?;
    let mut matching = declarations
        .iter()
        .filter(|declaration| declaration.class_name == class_name);
    let declaration = matching.next()?;
    if matching.next().is_some() {
        return None;
    }
    let descriptor = unique_descriptor_by_name(directory, &declaration.stream_name)?;
    if let Some(range) = contiguous_descriptor_range(descriptor, directory.inner, data.len()) {
        return Some(Cow::Borrowed(&data[range]));
    }
    let bytes = reconstruct_logical_stream(data, descriptor, directory.inner);
    (!bytes.is_empty()).then_some(Cow::Owned(bytes))
}

fn unique_descriptor_by_name<'a>(directory: &'a InnerDir, name: &str) -> Option<&'a Descriptor> {
    let mut descriptors = directory
        .descriptors
        .iter()
        .filter(|descriptor| descriptor.name == name);
    let descriptor = descriptors.next()?;
    descriptors.next().is_none().then_some(descriptor)
}

fn extract_main_data_stream(data: &[u8]) -> Option<Vec<u8>> {
    let inner_dir = parse_stream_directory(data)?;
    let descriptor = unique_largest_descriptor(
        inner_dir
            .descriptors
            .iter()
            .filter(|descriptor| descriptor.name == "MainDataStream"),
    )?;
    let bytes = reconstruct_logical_stream(data, descriptor, inner_dir.inner);
    (!bytes.is_empty()).then_some(bytes)
}

fn rep_contains_fbb_run(mds: &[u8], range: &Range<usize>) -> bool {
    let Some(segment) = mds.get(range.clone()) else {
        return false;
    };
    segment
        .windows(4)
        .any(|slice| matches!(slice[0], 0x30 | 0xb0) && slice[1..4] == [0x04, 0x04, 0xff])
}

/// Read a catalog entry selector (`0x32 <one_based:u32le>` or compact `byte >= 0x34`).
fn read_catalog_selector(bytes: &[u8], pos: usize) -> Option<(usize, usize)> {
    let lead = *bytes.get(pos)?;
    if lead == 0x32 {
        let one_based = View::u32_le_at(bytes, pos.checked_add(1)?)?;
        let zero_based = one_based.checked_sub(1)?;
        let next = pos.checked_add(5)?;
        if next > bytes.len() {
            return None;
        }
        return Some((index_from_u32(zero_based), next));
    }
    if lead >= 0x34 {
        return Some((usize::from(lead - 0x34), pos.checked_add(1)?));
    }
    None
}

/// Read an unsigned integer atom used inside `7C0B` value blocks
/// (`0x80 <u32le>`, `0x81..=0xD0`, or `0xD1..=0xE4 <lo>`).
fn read_vb_int(bytes: &[u8], pos: usize) -> Option<(u32, usize)> {
    let lead = *bytes.get(pos)?;
    if lead == 0x80 {
        let value = View::u32_le_at(bytes, pos.checked_add(1)?)?;
        let next = pos.checked_add(5)?;
        return (next <= bytes.len()).then_some((value, next));
    }
    if (0x81..=0xd0).contains(&lead) {
        return Some((u32::from(lead - 0x80), pos.checked_add(1)?));
    }
    if (0xd1..=0xe4).contains(&lead) {
        let low = *bytes.get(pos.checked_add(1)?)?;
        let high = u32::from(lead - 0xd1);
        let value = high
            .checked_mul(256)?
            .checked_add(u32::from(low))?
            .checked_add(1)?;
        return Some((value, pos.checked_add(2)?));
    }
    None
}

struct RawEntityRecord<'a> {
    entity_id: u32,
    def_body: &'a [u8],
    value_body: &'a [u8],
}

fn parse_7c05_run(prt: &[u8]) -> Option<(Vec<RawEntityRecord<'_>>, usize)> {
    let mut pos = find_from(prt, &[0x7c, 0x05], 0)?;
    let mut records = Vec::new();
    while prt.get(pos..pos.checked_add(2)?) == Some(&[0x7c, 0x05]) {
        if records.len() >= MAX_PRTCONT_ENTITIES {
            return None;
        }
        let total_len = index_from_u32(View::u32_le_at(prt, pos.checked_add(2)?)?);
        if total_len < 6 {
            break;
        }
        let end = pos.checked_add(total_len)?;
        let record = prt.get(pos..end)?;
        let fallback_id = u32::try_from(records.len().checked_add(1)?).ok()?;
        let (entity_id, def_body, value_body) = if record.get(7..9) == Some(&[0x7c, 0x06]) {
            let def_len = index_from_u32(View::u32_le_at(record, 9)?);
            let def_end = 7_usize.checked_add(def_len)?;
            let def_body = record.get(13..def_end)?;
            let entity_id = extract_7c06_entity_id(def_body).unwrap_or(fallback_id);
            let value_body = if record.get(def_end..def_end.checked_add(2)?) == Some(&[0x7c, 0x07])
            {
                let val_len = index_from_u32(View::u32_le_at(record, def_end.checked_add(2)?)?);
                let val_start = def_end.checked_add(6)?;
                let val_end = def_end.checked_add(val_len)?;
                record.get(val_start..val_end).unwrap_or_default()
            } else {
                &[]
            };
            (entity_id, def_body, value_body)
        } else {
            let def_body = record.get(6..).unwrap_or_default();
            let entity_id = extract_inline_entity_id(record).unwrap_or(fallback_id);
            (entity_id, def_body, &[][..])
        };
        records.push(RawEntityRecord {
            entity_id,
            def_body,
            value_body,
        });
        pos = end;
    }
    (!records.is_empty()).then_some((records, pos))
}

fn extract_7c06_entity_id(def_body: &[u8]) -> Option<u32> {
    let (_, after_class) = read_catalog_selector(def_body, 2)?;
    let (_, after_subtype) = read_catalog_selector(def_body, after_class)?;
    if *def_body.get(after_subtype)? != 0xea {
        return None;
    }
    View::u32_le_at(def_body, after_subtype.checked_add(1)?)
}

fn extract_inline_entity_id(record: &[u8]) -> Option<u32> {
    let scan_end = record.len().min(24);
    let prefix = record.get(6..scan_end)?;
    let rel = prefix.iter().position(|&byte| byte == 0xea)?;
    let id_offset = 6_usize.checked_add(rel)?.checked_add(1)?;
    View::u32_le_at(record, id_offset)
}

fn parse_prtcont_index(prt: &[u8]) -> Option<PrtContIndex> {
    let (raw_entities, end_7c05) = parse_7c05_run(prt)?;
    let pos_7c08 = if prt.get(end_7c05..end_7c05.checked_add(2)?) == Some(&[0x7c, 0x08]) {
        end_7c05
    } else {
        let next = end_7c05.checked_add(1)?;
        if prt.get(next..next.checked_add(2)?) == Some(&[0x7c, 0x08]) {
            next
        } else {
            return None;
        }
    };
    let len_7c08 = index_from_u32(View::u32_le_at(prt, pos_7c08.checked_add(2)?)?);
    let pos_after_7c08 = pos_7c08.checked_add(len_7c08)?;

    let (vb_payload, cat_pos) = if prt.get(pos_after_7c08..pos_after_7c08.checked_add(2)?)
        == Some(&[0x7c, 0x0b])
    {
        let declared_len = index_from_u32(View::u32_le_at(prt, pos_after_7c08.checked_add(2)?)?);
        let term_pos = pos_after_7c08.checked_add(declared_len)?;
        if prt.get(term_pos) != Some(&0xfe) {
            return None;
        }
        let payload_start = pos_after_7c08.checked_add(6)?;
        let payload = prt.get(payload_start..term_pos)?;
        (Some(payload), term_pos.checked_add(1)?)
    } else {
        (None, pos_after_7c08)
    };

    let catalog_slice = prt.get(cat_pos..)?;
    let parsed_catalog = catalog::parse(catalog_slice)
        .catalogs
        .into_iter()
        .find(|cat| cat.pos == 0)?;
    let catalog_values: Vec<&str> = parsed_catalog
        .entries
        .iter()
        .map(|entry| entry.value.as_str())
        .collect();

    let vp_global_index = catalog_values.iter().position(|&value| value == "VPGlobal");
    let mm_alias_index = catalog_values.iter().position(|&value| value == "MMAlias");

    let parsed_vis = vb_payload.and_then(|payload| {
        let entries = parse_7c0b_visibility(payload, vp_global_index)?;
        (entries.len() == raw_entities.len()).then_some(entries)
    });
    let value_block_complete = parsed_vis.is_some();

    let mut index = PrtContIndex {
        entities_by_id: HashMap::with_capacity(raw_entities.len()),
        entities_ordered: Vec::with_capacity(raw_entities.len()),
        value_block_complete,
    };

    for (ordinal, raw) in raw_entities.iter().enumerate() {
        let (class_name, subtype_name) = decode_entity_class_pair(raw.def_body, &catalog_values);
        let alias = mm_alias_index
            .and_then(|mm_idx| extract_mm_alias(raw.value_body, mm_idx, &catalog_values));
        let vp_global_visible = parsed_vis
            .as_ref()
            .and_then(|entries| entries.get(ordinal).copied().flatten());
        let entity = PrtContEntity {
            entity_id: raw.entity_id,
            class_name,
            subtype_name,
            alias,
            vp_global_visible,
        };
        index
            .entities_by_id
            .insert(entity.entity_id, entity.clone());
        index.entities_ordered.push(entity);
    }

    Some(index)
}

fn decode_entity_class_pair(
    def_body: &[u8],
    catalog_values: &[&str],
) -> (Option<String>, Option<String>) {
    let Some((sel0, next)) = read_catalog_selector(def_body, 2) else {
        return (None, None);
    };
    let class_name = catalog_values.get(sel0).map(|&s| s.to_owned());
    let subtype_name = read_catalog_selector(def_body, next)
        .and_then(|(sel1, _)| catalog_values.get(sel1).map(|&s| s.to_owned()));
    (class_name, subtype_name)
}

fn extract_mm_alias(
    value_body: &[u8],
    mm_alias_index: usize,
    catalog_values: &[&str],
) -> Option<String> {
    let mut tag_buf = [0_u8; 5];
    let tag: &[u8] = if let Some(compact) = mm_alias_index
        .checked_add(0x34)
        .and_then(|v| u8::try_from(v).ok())
    {
        tag_buf[0] = compact;
        &tag_buf[..1]
    } else {
        let one_based = u32::try_from(mm_alias_index.checked_add(1)?).ok()?;
        let le = one_based.to_le_bytes();
        tag_buf[0] = 0x32;
        tag_buf[1..5].copy_from_slice(&le);
        &tag_buf[..5]
    };
    let tag_pos = find_from(value_body, tag, 0)?;
    let search_start = tag_pos.checked_add(tag.len())?;
    let sep_pos = find_from(value_body, &[0x37], search_start)?;
    let sel_pos = sep_pos.checked_add(1)?;
    if value_body.get(sel_pos) == Some(&0xfe) {
        return None;
    }
    let (alias_idx, _) = read_catalog_selector(value_body, sel_pos)?;
    catalog_values.get(alias_idx).map(|&s| s.to_owned())
}

/// Parse the complete `7C0B` visualization block into one `Option<bool>`
/// (`VPGlobal` explicit visibility) per `7C05` entity record.
///
/// Returns `None` if any byte fails to match the self-describing `7C0B` grammar.
fn parse_7c0b_visibility(
    payload: &[u8],
    vp_global_index: Option<usize>,
) -> Option<Vec<Option<bool>>> {
    let mut at = 0;
    let mut entities = Vec::new();
    while at < payload.len() {
        let (count_plus_1, next) = read_vb_int(payload, at)?;
        let prop_count = usize::try_from(count_plus_1.checked_sub(1)?).ok()?;
        at = next;
        let mut entity_vis: Option<bool> = None;
        for _ in 0..prop_count {
            let (key_idx, after_key) = read_catalog_selector(payload, at)?;
            at = after_key;
            let (inline_slice, after_val) = skip_7c0b_value(payload, at)?;
            at = after_val;
            if vp_global_index == Some(key_idx) {
                if let Some(inline) = inline_slice {
                    if let Some(vis) = decode_vp_global_visibility(inline) {
                        entity_vis = Some(vis);
                    }
                }
            }
        }
        entities.push(entity_vis);
    }
    (at == payload.len()).then_some(entities)
}

/// Advance over one self-describing property value in a `7C0B` visualization block.
///
/// Returns `(Some(inline_bytes), next_offset)` for an `8E <E8..=EF> 84 <bytes>`
/// inline packet, and `(None, next_offset)` for all other value forms.
fn skip_7c0b_value(payload: &[u8], pos: usize) -> Option<(Option<&[u8]>, usize)> {
    let lead = *payload.get(pos)?;
    match lead {
        0x81 => Some((None, pos.checked_add(1)?)),
        0x83..=0x86 => {
            let (_, next) = read_vb_int(payload, pos.checked_add(1)?)?;
            Some((None, next))
        }
        0x87 => {
            let sub = *payload.get(pos.checked_add(1)?)?;
            match sub {
                0xe6 => {
                    let next = pos.checked_add(10)?;
                    (next <= payload.len()).then_some((None, next))
                }
                0xe7..=0xe9 => Some((None, pos.checked_add(2)?)),
                _ => None,
            }
        }
        0x8c | 0x8f => {
            let (_, next) = read_catalog_selector(payload, pos.checked_add(1)?)?;
            Some((None, next))
        }
        0x8e => {
            let code = *payload.get(pos.checked_add(1)?)?;
            if (0xe8..=0xef).contains(&code) && payload.get(pos.checked_add(2)?) == Some(&0x84) {
                let len = usize::from(code - 0xe7);
                let start = pos.checked_add(3)?;
                let end = start.checked_add(len)?;
                let slice = payload.get(start..end)?;
                return Some((Some(slice), end));
            }
            if code == 0xee {
                let next = pos.checked_add(10)?;
                return (next <= payload.len()).then_some((None, next));
            }
            if code == 0xe5 {
                let len = index_from_u32(View::u32_le_at(payload, pos.checked_add(2)?)?);
                let next = pos.checked_add(6)?.checked_add(len)?;
                return (next <= payload.len()).then_some((None, next));
            }
            None
        }
        0xe5 => {
            let len = index_from_u32(View::u32_le_at(payload, pos.checked_add(1)?)?);
            let next = pos.checked_add(5)?.checked_add(len)?;
            (next <= payload.len()).then_some((None, next))
        }
        _ => None,
    }
}

/// Parse the recursive `BagRep` (`0x62`) / `3DRep` (`0x95`) / auxiliary (`0x64`/`0x78`)
/// scene tree at the start of `MainDataStream`.
fn parse_mds_rep_tree(mds: &[u8]) -> Vec<MdsRepNode> {
    if mds.len() < 20 || mds.get(0..2) != Some(&[0xa3, 0xff]) {
        return Vec::new();
    }
    let mut reps = Vec::new();
    let mut stack = Vec::new();
    let _parsed_end = parse_bagrep_node(mds, 6, 0, &mut stack, &mut reps);
    reps
}

fn parse_bagrep_node(
    mds: &[u8],
    at: usize,
    depth: usize,
    stack: &mut Vec<BagRepAncestor>,
    reps: &mut Vec<MdsRepNode>,
) -> Option<usize> {
    if depth >= MAX_BAGREP_DEPTH {
        return None;
    }
    let search_end = at.checked_add(8)?.min(mds.len());
    let window = mds.get(at..search_end)?;
    let rel = find_from(window, &[0x04, 0xfd, 0xff], 0)?;
    let marker_pos = at.checked_add(rel)?;
    if marker_pos < 2 {
        return None;
    }
    let mut cursor = marker_pos.checked_add(3)?;
    let has_sphere = View::u32_be_at(mds, cursor)?;
    if has_sphere > 1 {
        return None;
    }
    cursor = cursor.checked_add(4)?.checked_add(21)?;
    let ident = View::u32_le_at(mds, cursor)?;
    let kind = mds.get(cursor.checked_add(4)?..cursor.checked_add(6)?)?;
    if kind != [0xff, 0x03] && kind != [0xff, 0xff] {
        return None;
    }
    let vis_byte = *mds.get(cursor.checked_add(6)?)?;
    let child_count = index_from_u32(View::u32_le_at(mds, cursor.checked_add(10)?)?);
    if child_count > MAX_BAGREP_CHILDREN {
        return None;
    }
    let mut cur = cursor.checked_add(14)?;
    stack.push(BagRepAncestor {
        entity_id: ident,
        vis_byte,
    });
    for _ in 0..child_count {
        if mds.get(cur) == Some(&0x01)
            && matches!(mds.get(cur.checked_add(1)?), Some(&0x00 | &0x01))
        {
            cur = cur.checked_add(1)?;
        }
        let lead = *mds.get(cur)?;
        if lead != 0x00 && lead != 0x01 {
            stack.pop();
            return None;
        }
        let op = *mds.get(cur.checked_add(1)?)?;
        match op {
            0x62 => {
                cur = match parse_bagrep_node(mds, cur.checked_add(2)?, depth + 1, stack, reps) {
                    Some(next) => next,
                    None => {
                        stack.pop();
                        return None;
                    }
                };
            }
            0x95 => {
                let Some((rstart, rend)) = parse_rep95_span(mds, cur) else {
                    stack.pop();
                    return None;
                };
                let cgm_allocation_end = extract_rep_allocation_end(mds, rstart, rend);
                reps.push(MdsRepNode {
                    rep_range: rstart..rend,
                    cgm_allocation_end,
                    ancestors: stack.clone(),
                });
                cur = rend;
            }
            0x64 | 0x78 => {
                if mds.get(cur.checked_add(2)?) != Some(&0xff) {
                    stack.pop();
                    return None;
                }
                let raw_len = View::u32_le_at(mds, cur.checked_add(3)?)?;
                let abs_u32 = if raw_len >= 0x8000_0000 {
                    u32::try_from(0x1_0000_0000_u64.checked_sub(u64::from(raw_len))?).ok()?
                } else {
                    raw_len
                };
                let abs_len = index_from_u32(abs_u32);
                let rstart = cur.checked_add(7)?;
                let rend = cur.checked_add(1)?.checked_add(abs_len)?;
                if rend > mds.len() || rend < rstart {
                    stack.pop();
                    return None;
                }
                let cgm_allocation_end = extract_rep_allocation_end(mds, rstart, rend);
                if cgm_allocation_end.is_some() {
                    reps.push(MdsRepNode {
                        rep_range: rstart..rend,
                        cgm_allocation_end,
                        ancestors: stack.clone(),
                    });
                }
                cur = rend;
            }
            _ => {
                stack.pop();
                return None;
            }
        }
    }
    stack.pop();
    Some(cur)
}

fn parse_rep95_span(mds: &[u8], cur: usize) -> Option<(usize, usize)> {
    let size_lead = *mds.get(cur.checked_add(2)?)?;
    let (rstart, mut rend) = if size_lead == 0xff {
        let len = index_from_u32(View::u32_le_at(mds, cur.checked_add(3)?)?);
        let rstart = cur.checked_add(7)?;
        let rend = cur.checked_add(3)?.checked_add(len)?;
        (rstart, rend)
    } else {
        let len = usize::from(size_lead);
        let rstart = cur.checked_add(3)?;
        let rend = rstart.checked_add(len)?;
        (rstart, rend)
    };
    if rend > mds.len() || rend < rstart {
        return None;
    }
    if size_lead == 0xff && rend > rstart && mds.get(rend.checked_sub(1)?) == Some(&0xff) {
        rend = rend.checked_add(4)?;
        if rend > mds.len() {
            return None;
        }
    }
    Some((rstart, rend))
}

fn extract_rep_allocation_end(mds: &[u8], rstart: usize, rend: usize) -> Option<u32> {
    let search_end = rstart.checked_add(10)?.min(rend);
    let window = mds.get(rstart..search_end)?;
    let rel = find_from(window, &[0x04, 0xfd, 0xff], 0)?;
    let marker_pos = rstart.checked_add(rel)?;
    let cursor = marker_pos.checked_add(3)?;
    let has_sphere = View::u32_be_at(mds, cursor)?;
    if has_sphere > 1 {
        return None;
    }
    let ident_pos = cursor.checked_add(25)?;
    if ident_pos.checked_add(4)? > rend {
        return None;
    }
    View::u32_le_at(mds, ident_pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_catalog_entry(buf: &mut Vec<u8>, text: &str) {
        let bytes = text.as_bytes();
        if bytes.len() < 255 {
            buf.push(u8::try_from(bytes.len() + 1).expect("short entry"));
        } else {
            buf.push(0);
            buf.extend(u32::try_from(bytes.len()).expect("u32").to_le_bytes());
        }
        buf.extend_from_slice(bytes);
    }

    fn build_7c02_catalog(entries: &[&str]) -> Vec<u8> {
        let mut body = Vec::new();
        let count_plus_1 = entries.len() + 1;
        assert!(count_plus_1 <= 0x50);
        body.push(u8::try_from(0x80 + count_plus_1).expect("compact count"));
        for entry in entries {
            push_catalog_entry(&mut body, entry);
        }
        let total_len = u32::try_from(6 + body.len()).expect("u32");
        let mut out = vec![0x7c, 0x02];
        out.extend(total_len.to_le_bytes());
        out.extend(body);
        out
    }

    fn build_7c05_entity(
        entity_id: u32,
        class_idx: usize,
        subtype_idx: usize,
        mm_alias: Option<(usize, usize)>,
    ) -> Vec<u8> {
        let mut def_body = vec![0x00, 0x00];
        def_body.push(u8::try_from(class_idx + 0x34).expect("compact selector"));
        def_body.push(u8::try_from(subtype_idx + 0x34).expect("compact selector"));
        def_body.push(0xea);
        def_body.extend(entity_id.to_le_bytes());
        let def_len = u32::try_from(6 + def_body.len()).expect("u32");
        let mut def_frame = vec![0x7c, 0x06];
        def_frame.extend(def_len.to_le_bytes());
        def_frame.extend(def_body);

        let mut val_body = Vec::new();
        if let Some((mm_idx, alias_idx)) = mm_alias {
            val_body.push(u8::try_from(mm_idx + 0x34).expect("compact selector"));
            val_body.push(0x37);
            val_body.push(u8::try_from(alias_idx + 0x34).expect("compact selector"));
            val_body.extend([0xfe, 0xfe]);
        } else {
            val_body.extend([0xfe, 0xfe]);
        }
        let val_len = u32::try_from(6 + val_body.len()).expect("u32");
        let mut val_frame = vec![0x7c, 0x07];
        val_frame.extend(val_len.to_le_bytes());
        val_frame.extend(val_body);

        let total_len = u32::try_from(7 + def_frame.len() + val_frame.len()).expect("u32");
        let mut rec = vec![0x7c, 0x05];
        rec.extend(total_len.to_le_bytes());
        rec.push(0x00);
        rec.extend(def_frame);
        rec.extend(val_frame);
        rec
    }

    fn build_bagrep_header(ident: u32, flag: u8, vis_byte: u8, child_count: u32) -> Vec<u8> {
        let mut out = vec![0x00, flag, 0x04, 0x04, 0xfd, 0xff];
        out.extend(1_u32.to_be_bytes());
        out.extend([0_u8; 21]);
        out.extend(ident.to_le_bytes());
        out.extend([0xff, 0x03]);
        out.extend([vis_byte, 0x00, 0x00, 0xff]);
        out.extend(child_count.to_le_bytes());
        out
    }

    fn build_rep95_leaf(cgm_slot: u32, fbb_count: usize) -> Vec<u8> {
        let mut inner = vec![0x00, 0x30, 0x04, 0x04, 0xfd, 0xff];
        inner.extend(1_u32.to_be_bytes());
        inner.extend([0_u8; 21]);
        inner.extend(cgm_slot.to_le_bytes());
        inner.extend([0xff, 0xff, 0x02, 0x00]);
        for _ in 0..fbb_count {
            inner.extend([0x30, 0x04, 0x04, 0xff, 0xff, 0x80, 0x80, 0x80]);
        }
        inner.push(0x00); // non-0xff trailer count byte
        let rlen = u32::try_from(4 + inner.len()).expect("u32");
        let mut out = vec![0x00, 0x95, 0xff];
        out.extend(rlen.to_le_bytes());
        out.extend(inner);
        out
    }

    #[test]
    fn vp_global_visibility_bits_distinguish_show_noshow_and_unset() {
        assert_eq!(decode_vp_global_visibility(&[0x01, 0x00]), Some(true));
        assert_eq!(decode_vp_global_visibility(&[0x09, 0x00]), Some(false));
        assert_eq!(decode_vp_global_visibility(&[0xc5, 0xff, 0x04]), Some(true));
        assert_eq!(
            decode_vp_global_visibility(&[0xcd, 0xff, 0x00]),
            Some(false)
        );
        assert_eq!(decode_vp_global_visibility(&[0xc4, 0xff, 0x00]), None);
        assert_eq!(decode_vp_global_visibility(&[]), None);
    }

    #[test]
    fn synthetic_prtcont_and_mds_decode_names_visibility_and_container_kinds() {
        // Catalog entries: 0..4 standard prefix, 4..=14 body classes/names/VPGlobal
        let catalog_entries = [
            "CATCatalogManager",
            "catalogManager",
            "catalogLinks",
            "",
            "MechanicalPart",
            "Part1",
            "MechanicalTool",
            "HybridBody",
            "GSMTool",
            "Open_body",
            "Solid",
            "MMAlias",
            "VPGlobal",
            "CutterBody",
            "ConstructionSet",
            "MainPartBody",
            "MainPartBody__CADM1",
        ];
        // Build CATPrtCont with 5 entities:
        // eid=1: MechanicalPart / Part1
        // eid=2: HybridBody (alias="MainPartBody__CADM1"), default shown
        // eid=3: Solid / MainPartBody (clean solid name overrides __CADM1)
        // eid=4: MechanicalTool (alias="CutterBody"), VPGlobal = 0x09 0x00 (NoShow)
        // eid=5: GSMTool / Open_body (alias="ConstructionSet"), VPGlobal = 0xCD 0xFF 0x00 (NoShow)
        let mut prt = Vec::new();
        prt.extend(build_7c05_entity(1, 4, 5, None));
        prt.extend(build_7c05_entity(2, 7, 7, Some((11, 16))));
        prt.extend(build_7c05_entity(3, 10, 15, None));
        prt.extend(build_7c05_entity(4, 6, 6, Some((11, 13))));
        prt.extend(build_7c05_entity(5, 8, 9, Some((11, 14))));
        prt.push(0xde);
        // Empty 7C08 frame (length 6)
        prt.extend([0x7c, 0x08, 6, 0, 0, 0]);
        // 7C0B value block with 5 entity entries:
        // eid=1: A(1) (0 props)
        // eid=2: A(1) (0 props -> default shown)
        // eid=3: A(1) (0 props)
        // eid=4: A(2) VPGlobal(idx 12 -> 0x40) 8E E9 84 09 00 (NoShow)
        // eid=5: A(2) VPGlobal(idx 12 -> 0x40) 8E EA 84 CD FF 00 (NoShow)
        let mut vb_payload = vec![0x81, 0x81, 0x81];
        vb_payload.extend([0x82, 0x40, 0x8e, 0xe9, 0x84, 0x09, 0x00]);
        vb_payload.extend([0x82, 0x40, 0x8e, 0xea, 0x84, 0xcd, 0xff, 0x00]);
        let vb_decl_len = u32::try_from(6 + vb_payload.len()).expect("u32");
        prt.extend([0x7c, 0x0b]);
        prt.extend(vb_decl_len.to_le_bytes());
        prt.extend(vb_payload);
        prt.push(0xfe);
        prt.extend(build_7c02_catalog(&catalog_entries));

        let prt_index = parse_prtcont_index(&prt).expect("parse synthetic CATPrtCont");
        assert!(prt_index.value_block_complete);
        assert_eq!(prt_index.entities_ordered.len(), 5);
        assert_eq!(prt_index.entities_by_id[&4].vp_global_visible, Some(false));
        assert_eq!(prt_index.entities_by_id[&5].vp_global_visible, Some(false));

        // Build synthetic MainDataStream with root (eid=1, 3 children):
        // Child 0: BagRep(eid=2) -> BagRep(eid=3) -> Rep95(slot=100, 2 FBB faces)
        // Child 1: BagRep(eid=4) -> Rep95(slot=200, 1 FBB face)
        // Child 2: BagRep(eid=5) -> Rep95(slot=300, 1 FBB face)
        let mut body = Vec::new();
        body.extend(build_bagrep_header(1, 0x00, 1, 3));
        // Child 0
        body.extend([0x00, 0x62]);
        body.extend(build_bagrep_header(2, 0x30, 1, 1));
        body.extend([0x00, 0x62]);
        body.extend(build_bagrep_header(3, 0x30, 1, 1));
        body.extend(build_rep95_leaf(100, 2));
        // Child 1
        body.extend([0x00, 0x62]);
        body.extend(build_bagrep_header(4, 0x30, 1, 1));
        body.extend(build_rep95_leaf(200, 1));
        // Child 2
        body.extend([0x00, 0x62]);
        body.extend(build_bagrep_header(5, 0x20, 1, 1));
        body.extend(build_rep95_leaf(300, 1));
        body.extend(1.0_f32.to_le_bytes());

        let mut mds = vec![0xa3, 0xff];
        let total = u32::try_from(4 + body.len()).expect("u32");
        mds.extend(total.to_le_bytes());
        mds.extend(body);

        let reps = parse_mds_rep_tree(&mds);
        assert_eq!(reps.len(), 3);
        let see_slots: HashSet<usize> = [100, 200, 300].into_iter().collect();
        let meta: Vec<CatiaBodyMetadata> = reps
            .iter()
            .map(|rep| build_body_metadata(rep, &prt_index, Some(&see_slots)))
            .collect();

        assert_eq!(meta[0].name.as_deref(), Some("MainPartBody"));
        assert_eq!(meta[0].visible, Some(true));
        assert_eq!(meta[0].container_kind, CatiaBodyContainerKind::HybridBody);

        assert_eq!(meta[1].name.as_deref(), Some("CutterBody"));
        assert_eq!(meta[1].visible, Some(false));
        assert_eq!(meta[1].container_kind, CatiaBodyContainerKind::PartBody);

        assert_eq!(meta[2].name.as_deref(), Some("ConstructionSet"));
        assert_eq!(meta[2].visible, Some(false));
        assert_eq!(
            meta[2].container_kind,
            CatiaBodyContainerKind::GeometricalSet
        );
    }

    #[test]
    fn see_body_exclusion_and_mds_vis_zero_mark_body_hidden() {
        let rep_excluded = MdsRepNode {
            rep_range: 10..40,
            cgm_allocation_end: Some(999),
            ancestors: vec![BagRepAncestor {
                entity_id: 2,
                vis_byte: 1,
            }],
        };
        let see_slots: HashSet<usize> = [100, 200].into_iter().collect();
        let meta = build_body_metadata(&rep_excluded, &PrtContIndex::default(), Some(&see_slots));
        assert_eq!(meta.visible, Some(false));

        let rep_mds_hidden = MdsRepNode {
            rep_range: 10..40,
            cgm_allocation_end: Some(100),
            ancestors: vec![BagRepAncestor {
                entity_id: 2,
                vis_byte: 0,
            }],
        };
        let meta_hidden =
            build_body_metadata(&rep_mds_hidden, &PrtContIndex::default(), Some(&see_slots));
        assert_eq!(meta_hidden.visible, Some(false));

        let rep_unknown = MdsRepNode {
            rep_range: 10..40,
            cgm_allocation_end: Some(100),
            ancestors: vec![BagRepAncestor {
                entity_id: 2,
                vis_byte: 1,
            }],
        };
        let meta_unknown =
            build_body_metadata(&rep_unknown, &PrtContIndex::default(), Some(&see_slots));
        assert_eq!(meta_unknown.visible, None);
    }

    #[test]
    fn value_block_parser_handles_all_self_describing_forms_and_rejects_truncation() {
        // Entity 0: 0 props (0x81)
        // Entity 1: 6 props (0x87):
        //   - wide selector 0x32 (1 -> idx 0), flag 0x81
        //   - compact selector 0x35 (idx 1), escaped int 0x83 0x80 ff ff ff ff
        //   - compact selector 0x36 (idx 2), f64 zero 0x87 0xe8
        //   - compact selector 0x37 (idx 3), string ref 0x8f 0x34
        //   - compact selector 0x38 (idx 4), raw 8-byte 0x8e 0xee <8B>
        //   - wide selector 0x32 (10 -> idx 9 = VPGlobal), inline 0x8e 0xe9 0x84 0x09 0x00
        let mut payload = vec![0x81, 0x87];
        payload.extend([0x32, 1, 0, 0, 0, 0x81]);
        payload.extend([0x35, 0x83, 0x80, 0xff, 0xff, 0xff, 0xff]);
        payload.extend([0x36, 0x87, 0xe8]);
        payload.extend([0x37, 0x8f, 0x34]);
        payload.extend([0x38, 0x8e, 0xee, 1, 2, 3, 4, 5, 6, 7, 8]);
        payload.extend([0x32, 10, 0, 0, 0, 0x8e, 0xe9, 0x84, 0x09, 0x00]);

        let parsed = parse_7c0b_visibility(&payload, Some(9)).expect("parse all value forms");
        assert_eq!(parsed, vec![None, Some(false)]);

        // Truncated payload must return None cleanly.
        assert!(parse_7c0b_visibility(&payload[..payload.len() - 1], Some(9)).is_none());
    }
}
