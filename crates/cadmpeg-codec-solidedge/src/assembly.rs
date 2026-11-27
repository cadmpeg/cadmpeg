// SPDX-License-Identifier: Apache-2.0
//! Assembly occurrences (`docs/formats/solidedge.md` §9): the placements of
//! the documents an assembly references.
//!
//! Each `CLSID_JAsmReference` object listed by the `References Collection
//! Object` becomes one occurrence of the external definition of its
//! `CLSID_JSite` object. The definition's path is the drive-absolute path of
//! the first file moniker of the site's link stream; CADIR states one path per
//! document, and the relative moniker is the path a reader resolves against the
//! assembly's own folder, which the decode does not know. A moniker that does
//! not read leaves the definition without a path. An occurrence that does not
//! read adds `assembly.occurrence-unread`, a site whose link does not read adds
//! `assembly.reference-unread`. The visibility table (§9.4) states each
//! occurrence's display state; the occurrence structure list (§9.4) states
//! each occurrence's display state and suppression: an occurrence it does not
//! list is suppressed, and a listed occurrence is hidden where its flags say
//! so. The table decides the display state where it reads, and the list where
//! it does not. A document where neither reads states no display state and no
//! suppression and adds `assembly.visibility-unread` once. The state word
//! states whether the occurrence is grounded, so it states no suppression. A
//! document with no references collection adds nothing.
//!
//! Stored lengths are metres and CADIR states millimetres. The stored rotation
//! `R` maps a row vector `p` to `p R + t`; CADIR applies a transform to a
//! column vector, so its linear rows are the columns of `R`.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{ExternalDocument, Occurrence, ProductDefinition, Storage};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::transform::Transform;

use crate::container::DocStreams;
use crate::extents::MM_PER_M;
use crate::layout::{
    assembly_reference, file_moniker, occurrence_structure_entry, occurrence_structure_header,
    references_collection,
};
use crate::loss::SolidEdgeLossCode;
use crate::psm::{Cluster, SpaceMap, TypeTable, SPACE_MAP_STREAM, TYPE_TABLE_STREAM};

/// The class of the object that lists the occurrences.
const COLLECTION_CLASS: &str = "References Collection Object";
/// The class of an occurrence.
const REFERENCE_CLASS: &str = "CLSID_JAsmReference";
/// The class of a referenced document's site.
const SITE_CLASS: &str = "CLSID_JSite";
/// Offset of the first file moniker in a site's link stream, after its 12-byte
/// header (§9.3).
const FIRST_MONIKER_AT: usize = 12;
/// The up-level counts of the absolute and the relative moniker (§9.3).
const ABSOLUTE_UP_LEVEL: u16 = 0;
const RELATIVE_UP_LEVEL: u16 = 1;
/// The word that follows a moniker path (§9.3).
const PATH_END: u16 = 0xFFFF;
/// The stream that lists the occurrences of an assembly (§9.4).
const STRUCTURE_STREAM: &str = "Preview/Structure/IOT";
/// The flag of a structure entry that marks a hidden occurrence (§9.4).
const HIDDEN_FLAG: u32 = 0x2;
/// The stream that lists the hidden occurrences (§9.4).
const VISIBILITY_STREAM: &str = "IOT/Default";
/// Offset of the record count of the visibility table, after its header (§9.4).
const VISIBILITY_COUNT_AT: usize = 104;
/// Length of one hidden-occurrence record (§9.4).
const VISIBILITY_RECORD_LEN: usize = 12;
/// Offset of the occurrence identifier within a record (§9.4).
const VISIBILITY_ID_AT: usize = 8;

/// Largest deviation of a rotation entry from an orthonormal matrix.
const EPS_ORTHO: f64 = 1e-6;
/// Largest deviation of a rotation's determinant from +1.
const EPS_DET: f64 = 1e-6;
/// Largest deviation of the stored trailing word from `1`.
const EPS_UNIT: f64 = 1e-9;

fn occurrence_id(object: u32) -> OccurrenceId {
    OccurrenceId::compose(
        &cadmpeg_ir::identity_namespace!("solidedge", "model", "occurrence"),
        object,
    )
}

fn site_definition(site: u32) -> ProductDefinitionId {
    ProductDefinitionId::compose(
        &cadmpeg_ir::identity_namespace!("solidedge", "model", "external-document"),
        site,
    )
}

/// The identifiers the references collection lists, and whether the list is
/// longer than the object's bytes.
struct Listing {
    declared: usize,
    ids: Vec<u32>,
}

/// The listing of the first references collection of `cluster`, or `None`
/// where the cluster holds none.
fn listing(cluster: &Cluster<'_>, types: &TypeTable) -> Option<Listing> {
    let object = cluster
        .objects()
        .find(|object| types.class_name(object.form, object.flags) == Some(COLLECTION_CLASS))?;
    let bytes = object.bytes;
    let declared = View::u32_le_at(bytes, references_collection::COUNT)
        .and_then(|count| usize::try_from(count).ok())
        .unwrap_or(0);
    let first = references_collection::COUNT
        .checked_add(4)
        .and_then(|at| at.checked_add(declared.div_ceil(8)));
    let mut ids = Vec::new();
    if let Some(first) = first {
        let room = bytes.len().checked_sub(first).map_or(0, |rest| rest / 4);
        for index in 0..declared.min(room) {
            let Some(id) = index
                .checked_mul(4)
                .and_then(|offset| first.checked_add(offset))
                .and_then(|at| View::u32_le_at(bytes, at))
            else {
                break;
            };
            ids.push(id);
        }
    }
    Some(Listing { declared, ids })
}

/// One occurrence as stored.
struct Stored {
    site: u32,
    /// The placement, or the reason it is not stated.
    transform: Result<Transform, String>,
}

fn reals<const N: usize>(bytes: &[u8], at: usize) -> Option<[f64; N]> {
    let mut values = [0.0; N];
    for (index, value) in values.iter_mut().enumerate() {
        let offset = index.checked_mul(8)?.checked_add(at)?;
        *value = View::f64_le_at(bytes, offset).filter(|real| real.is_finite())?;
    }
    Some(values)
}

/// Whether `r` is orthonormal with determinant +1.
fn proper_rotation(r: &[[f64; 3]; 3]) -> bool {
    for (i, row_i) in r.iter().enumerate() {
        for (j, row_j) in r.iter().enumerate() {
            let dot: f64 = row_i.iter().zip(row_j).map(|(a, b)| a * b).sum();
            let want = if i == j { 1.0 } else { 0.0 };
            if (dot - want).abs() > EPS_ORTHO {
                return false;
            }
        }
    }
    let cross = [
        r[1][1] * r[2][2] - r[1][2] * r[2][1],
        r[1][2] * r[2][0] - r[1][0] * r[2][2],
        r[1][0] * r[2][1] - r[1][1] * r[2][0],
    ];
    let det: f64 = r[0].iter().zip(&cross).map(|(a, b)| a * b).sum();
    (det - 1.0).abs() <= EPS_DET
}

fn placement(bytes: &[u8]) -> Result<Transform, String> {
    let (Some(flat), Some(t), Some([unit])) = (
        reals::<9>(bytes, assembly_reference::ROTATION),
        reals::<3>(bytes, assembly_reference::TRANSLATION),
        reals::<1>(bytes, assembly_reference::UNIT),
    ) else {
        return Err("the rotation or translation holds a value that is not finite".into());
    };
    if (unit - 1.0).abs() > EPS_UNIT {
        return Err("the word after the translation is not 1".into());
    }
    let r = [
        [flat[0], flat[1], flat[2]],
        [flat[3], flat[4], flat[5]],
        [flat[6], flat[7], flat[8]],
    ];
    if !proper_rotation(&r) {
        return Err("the rotation is not orthonormal with determinant +1".into());
    }
    let rows = std::array::from_fn(|j| [r[0][j], r[1][j], r[2][j], t[j] * MM_PER_M]);
    Transform::affine(rows).ok_or_else(|| "the placement is not finite".into())
}

fn read_occurrence(cluster: &Cluster<'_>, types: &TypeTable, id: u32) -> Result<Stored, String> {
    let object = cluster
        .object(id)
        .ok_or_else(|| "the object is not in the cluster".to_owned())?;
    match types.class_name(object.form, object.flags) {
        Some(REFERENCE_CLASS) => {}
        Some(other) => return Err(format!("the object is of class {other}")),
        None => return Err("the object has no class".into()),
    }
    let bytes = object.bytes;
    if bytes.len() < assembly_reference::LEN {
        return Err(format!(
            "the object has {} bytes, fewer than the {} of an occurrence",
            bytes.len(),
            assembly_reference::LEN
        ));
    }
    let site = View::u32_le_at(bytes, assembly_reference::SITE)
        .ok_or_else(|| "the site identifier is unreadable".to_owned())?;
    let site_object = cluster
        .object(site)
        .ok_or_else(|| format!("the site object {site:#x} is not in the cluster"))?;
    if types.class_name(site_object.form, site_object.flags) != Some(SITE_CLASS) {
        return Err(format!("the object {site:#x} is not a {SITE_CLASS}"));
    }
    Ok(Stored {
        site,
        transform: placement(bytes),
    })
}

/// The path of the file moniker at `at` in `bytes`, and the offset after it.
fn moniker(bytes: &[u8], at: usize, up_level: u16) -> Option<(NonBlankString, usize)> {
    let end_of_clsid = at.checked_add(file_moniker::CLSID_VALUE.len())?;
    if bytes.get(at..end_of_clsid)? != file_moniker::CLSID_VALUE {
        return None;
    }
    if View::u16_le_at(bytes, at.checked_add(file_moniker::UP_LEVEL)?)? != up_level {
        return None;
    }
    let length = usize::try_from(View::u32_le_at(
        bytes,
        at.checked_add(file_moniker::PATH_LEN)?,
    )?)
    .ok()?;
    let start = at.checked_add(file_moniker::LEN)?;
    let end = start.checked_add(length)?;
    let (terminator, text) = bytes.get(start..end)?.split_last()?;
    if *terminator != 0 || text.contains(&0) || !text.is_ascii() {
        return None;
    }
    if View::u16_le_at(bytes, end)? != PATH_END {
        return None;
    }
    let path = NonBlankString::new(String::from_utf8(text.to_vec()).ok()?)?;
    Some((path, end.checked_add(2)?))
}

/// The document path of a site's link stream: the first moniker's path, or
/// the second's where the first does not read. The second moniker is the next
/// occurrence of the moniker CLSID after the first one's end.
fn link_path(bytes: &[u8]) -> Option<NonBlankString> {
    let from = match moniker(bytes, FIRST_MONIKER_AT, ABSOLUTE_UP_LEVEL) {
        Some((path, _)) => return Some(path),
        None => FIRST_MONIKER_AT.checked_add(file_moniker::CLSID_VALUE.len())?,
    };
    let found = bytes
        .get(from..)?
        .windows(file_moniker::CLSID_VALUE.len())
        .position(|window| window == file_moniker::CLSID_VALUE)?;
    moniker(bytes, from.checked_add(found)?, RELATIVE_UP_LEVEL).map(|(path, _)| path)
}

/// The flags of each entry of an occurrence structure list (§9.4) by occurrence
/// object, or `None` where the stream is not 20 + 20·`n` bytes long for its
/// stated count `n`.
fn structure_flags(bytes: &[u8]) -> Option<BTreeMap<u32, u32>> {
    let count =
        usize::try_from(View::u32_le_at(bytes, occurrence_structure_header::COUNT)?).ok()?;
    let body = count.checked_mul(occurrence_structure_entry::LEN)?;
    if occurrence_structure_header::LEN.checked_add(body)? != bytes.len() {
        return None;
    }
    let mut flags = BTreeMap::new();
    for index in 0..count {
        let at = index
            .checked_mul(occurrence_structure_entry::LEN)?
            .checked_add(occurrence_structure_header::LEN)?;
        let object = View::u32_le_at(bytes, at.checked_add(occurrence_structure_entry::OBJECT)?)?;
        let entry = View::u32_le_at(bytes, at.checked_add(occurrence_structure_entry::FLAGS)?)?;
        if flags.insert(object, entry).is_some() {
            return None;
        }
    }
    Some(flags)
}

/// The object identifiers of the hidden occurrences, or `None` where the
/// visibility table is absent or shorter than its count states.
fn hidden_occurrences<'a>(
    ctx: &DecodeContext<'a>,
    snapshot: &DocStreams<'_, 'a>,
) -> Option<BTreeSet<u32>> {
    let entry = snapshot.stream(VISIBILITY_STREAM)?;
    let view = snapshot.open(ctx, entry).ok()?;
    let bytes = view.window();
    let count = usize::try_from(View::u32_le_at(bytes, VISIBILITY_COUNT_AT)?).ok()?;
    let first = VISIBILITY_COUNT_AT.checked_add(4)?;
    let end = count
        .checked_mul(VISIBILITY_RECORD_LEN)
        .and_then(|len| first.checked_add(len))?;
    if bytes.len() < end {
        return None;
    }
    let mut hidden = BTreeSet::new();
    for index in 0..count {
        let at = index
            .checked_mul(VISIBILITY_RECORD_LEN)?
            .checked_add(first)?
            .checked_add(VISIBILITY_ID_AT)?;
        hidden.insert(View::u32_le_at(bytes, at)?);
    }
    Some(hidden)
}

fn site_stream(site: u32) -> String {
    format!("JSite{site}/\u{1}Ole")
}

/// State the occurrences of an assembly on the root definition.
pub(crate) fn project<'a>(
    ctx: &DecodeContext<'a>,
    snapshot: &DocStreams<'_, 'a>,
    cluster: View<'a>,
    root: &ProductDefinitionId,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let (Some(map), Some(types)) = (
        snapshot.stream(SPACE_MAP_STREAM),
        snapshot.stream(TYPE_TABLE_STREAM),
    ) else {
        return Ok(());
    };
    let map = snapshot.open(ctx, map)?;
    let types = snapshot.open(ctx, types)?;
    let (Some(map), Some(types)) = (
        SpaceMap::parse(map.window()),
        TypeTable::parse(types.window()),
    ) else {
        return Ok(());
    };
    let Some(cluster) = Cluster::parse(cluster.window(), &map) else {
        return Ok(());
    };
    let Some(listing) = listing(&cluster, &types) else {
        return Ok(());
    };
    if listing.ids.len() < listing.declared {
        losses.push(SolidEdgeLossCode::AssemblyOccurrenceUnread.note(format!(
            "the references collection states {} occurrence(s) and holds the identifiers of {}; \
             the others are not transferred",
            listing.declared,
            listing.ids.len()
        )));
    }
    let hidden = hidden_occurrences(ctx, snapshot);
    let mut definitions: BTreeMap<u32, ProductDefinitionId> = BTreeMap::new();
    let mut placed = BTreeSet::new();
    let mut new_definitions = Vec::new();
    let mut occurrences = Vec::new();
    for id in listing.ids.into_iter().filter(|id| snapshot.lists(*id)) {
        if !placed.insert(id) {
            losses.push(SolidEdgeLossCode::AssemblyOccurrenceUnread.note(format!(
                "occurrence object {id:#x} is listed twice; the second entry is not transferred"
            )));
            continue;
        }
        let stored = match read_occurrence(&cluster, &types, id) {
            Ok(stored) => stored,
            Err(reason) => {
                losses.push(SolidEdgeLossCode::AssemblyOccurrenceUnread.note(format!(
                    "occurrence object {id:#x} does not read: {reason}; it is not transferred"
                )));
                continue;
            }
        };
        ctx.charge_entities(1, "project Solid Edge occurrence")?;
        let prototype = match definitions.get(&stored.site) {
            Some(prototype) => prototype.clone(),
            None => {
                ctx.charge_entities(1, "project Solid Edge referenced document")?;
                let link = snapshot
                    .stream(&site_stream(stored.site))
                    .and_then(|entry| snapshot.open(ctx, entry).ok())
                    .and_then(|view| link_path(view.window()));
                if link.is_none() {
                    losses.push(SolidEdgeLossCode::AssemblyReferenceUnread.note(format!(
                        "the link of site object {:#x} does not read; its occurrences are kept \
                         without a document path",
                        stored.site
                    )));
                }
                let prototype = site_definition(stored.site);
                new_definitions.push(ProductDefinition::new(
                    prototype.clone(),
                    Storage::External(ExternalDocument {
                        path: link,
                        ..ExternalDocument::default()
                    }),
                ));
                definitions.insert(stored.site, prototype.clone());
                prototype
            }
        };
        let transform = match stored.transform {
            Ok(transform) => Some(transform),
            Err(reason) => {
                losses.push(SolidEdgeLossCode::AssemblyOccurrenceUnread.note(format!(
                    "the placement of occurrence object {id:#x} does not read: {reason}; the \
                     occurrence states no placement"
                )));
                None
            }
        };
        let occurrence = Occurrence::new(occurrence_id(id), prototype, transform);
        occurrences.push((id, occurrence));
    }
    if occurrences.is_empty() {
        return Ok(());
    }
    let structure = snapshot
        .stream(STRUCTURE_STREAM)
        .and_then(|entry| snapshot.open(ctx, entry).ok())
        .and_then(|view| structure_flags(view.window()));
    if let Some(flags) = &structure {
        for (id, occurrence) in &mut occurrences {
            let entry = flags.get(id);
            occurrence.suppressed = Some(entry.is_none() || snapshot.excludes(*id));
            occurrence.visible = entry.map(|bits| bits & HIDDEN_FLAG == 0);
        }
    }
    if let Some(hidden) = &hidden {
        for (id, occurrence) in &mut occurrences {
            occurrence.visible = Some(!hidden.contains(id));
        }
    }
    if hidden.is_none() && structure.is_none() {
        losses.push(SolidEdgeLossCode::AssemblyVisibilityUnread.note(
            "neither the visibility table nor the occurrence structure list of the assembly \
             reads; the occurrences state no visibility or suppression",
        ));
    }
    for (id, occurrence) in &mut occurrences {
        if snapshot.excludes(*id) {
            occurrence.suppressed = Some(true);
        }
    }
    let occurrences: Vec<Occurrence> = occurrences
        .into_iter()
        .map(|(_, occurrence)| occurrence)
        .collect();
    if let Some(owner) = ir
        .model
        .product_definitions
        .iter_mut()
        .find(|definition| definition.id == *root)
    {
        owner
            .occurrences
            .extend(occurrences.iter().map(|occurrence| occurrence.id.clone()));
    }
    ir.model.product_definitions.extend(new_definitions);
    ir.model.occurrences.extend(occurrences);
    Ok(())
}

#[cfg(test)]
mod tests;
