// SPDX-License-Identifier: Apache-2.0
//! The ordered feature tree of a document (`docs/formats/solidedge.md` §4),
//! projected as one timeline of features that state their operation family
//! and no operands.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_container::compound::CompoundSnapshot;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::container::{Container, OrderedKind};
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::names::CodecName;
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::SolidEdgeLossCode;
use crate::psm::{Cluster, Object, SpaceMap, TypeTable, SPACE_MAP_STREAM, TYPE_TABLE_STREAM};

/// The class of a feature stored as a part function.
const PART_CLASS: &str = "CLSID_JMPart";
/// The class of a feature stored as a mirror copy or pattern.
const GROUP_CLASS: &str = "CLSID_JMGroup";
/// The class of a feature-tree folder.
const FOLDER_CLASS: &str = "CLSID_JMFeatureGroup";
/// The class of the object that owns the part's features.
const OWNER_CLASS: &str = "CLSID_JMPartOwner";

/// The navigator's class GUID in the GUID byte order of the type table:
/// the first three groups little-endian.
const NAVIGATOR_GUID: [u8; 16] = [
    0x50, 0x6D, 0xF6, 0x44, 0xD1, 0x4D, 0xCE, 0x11, 0xBF, 0x4E, 0x08, 0x00, 0x36, 0x01, 0xC7, 0x2A,
];
/// Object offset of the navigator GUID.
const NAVIGATOR_GUID_AT: usize = 12;
/// The bytes of each constant block of the navigator, before and after the
/// navigator name.
const BLOCK_BEFORE_NAME: usize = 147;
const BLOCK_AFTER_NAME: usize = 68;
/// The navigator name with its length word's count: the name and one zero
/// byte.
const NAVIGATOR_NAME: &[u8] = b"JMFeatureNavigatorPartModified\0";
/// The four constant words, the owner and the last feature, which precede the
/// entry count.
const WORDS_BEFORE_COUNT: usize = 24;
/// The entries that are the three base reference planes.
const BASE_PLANES: usize = 3;
/// Object offset of the `u32` byte count that precedes a part function's
/// stored block.
const FUNCTION_BLOCK_AT: usize = 132;

/// The result of reading the feature navigator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Navigator {
    /// No cluster-0 object is a part feature navigator.
    Absent,
    /// A navigator object exists and does not parse, or names an object the
    /// cluster does not hold.
    Unread,
    /// The features in feature-tree order.
    Features(Vec<Listed>),
}

/// One listed feature: its object and what its class states of its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Listed {
    pub(crate) object: u32,
    pub(crate) kind: Kind,
}

/// What a feature object states of its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A part function and its function name; `None` where the stored block
    /// does not read.
    Function(Option<String>),
    /// A mirror copy or pattern.
    Group,
    /// A feature-tree folder.
    Folder,
    /// An object of another class.
    Other(String),
    /// A listed object that the cluster does not hold or whose class the
    /// type table does not name.
    Unreadable,
}

/// The navigator of `cluster`, with the kind of each feature it lists.
pub(crate) fn navigator(cluster: &Cluster<'_>, types: &TypeTable) -> Navigator {
    let mut candidates = false;
    for object in cluster.objects() {
        if object
            .bytes
            .get(NAVIGATOR_GUID_AT..NAVIGATOR_GUID_AT + NAVIGATOR_GUID.len())
            != Some(&NAVIGATOR_GUID[..])
        {
            continue;
        }
        if !names_navigator(object.bytes) {
            continue;
        }
        candidates = true;
        if let Some(entries) = entries(object.bytes) {
            return listed(cluster, types, &entries).map_or(Navigator::Unread, Navigator::Features);
        }
    }
    if candidates {
        Navigator::Unread
    } else {
        Navigator::Absent
    }
}

/// Whether the cluster holds an object of a feature class.
pub(crate) fn has_feature_objects(cluster: &Cluster<'_>, types: &TypeTable) -> bool {
    cluster.objects().any(|object| {
        matches!(
            types.class_name(object.form, object.flags),
            Some(PART_CLASS | GROUP_CLASS | OWNER_CLASS)
        )
    })
}

/// The next little-endian `u32` of `bytes` at `*at`, advancing past it.
fn word(bytes: &[u8], at: &mut usize) -> Option<u32> {
    let value = View::u32_le_at(bytes, *at)?;
    *at = at.checked_add(4)?;
    Some(value)
}

/// Skip a `u32` count and that many `u32` identifiers.
fn skip_list(bytes: &[u8], at: &mut usize) -> Option<()> {
    let count = usize::try_from(word(bytes, at)?).ok()?;
    let size = count.checked_mul(4)?;
    if size > bytes.len().checked_sub(*at)? {
        return None;
    }
    *at = at.checked_add(size)?;
    Some(())
}

/// Skip `count` bytes.
fn skip(bytes: &[u8], at: &mut usize, count: usize) -> Option<()> {
    let end = at.checked_add(count)?;
    if end > bytes.len() {
        return None;
    }
    *at = end;
    Some(())
}

/// Whether the part navigator name stands at its grammar position (§4.1).
/// An object with the navigator GUID and another name, such as an
/// assembly's, is not a part navigator.
fn names_navigator(bytes: &[u8]) -> bool {
    let mut at = NAVIGATOR_GUID_AT + NAVIGATOR_GUID.len();
    skip_list(bytes, &mut at)
        .and_then(|()| skip_list(bytes, &mut at))
        .and_then(|()| skip(bytes, &mut at, BLOCK_BEFORE_NAME))
        .and_then(|()| word(bytes, &mut at))
        .is_some_and(|length| {
            usize::try_from(length).is_ok_and(|length| length == NAVIGATOR_NAME.len())
                && bytes.get(at..at + NAVIGATOR_NAME.len()) == Some(NAVIGATOR_NAME)
        })
}

/// The object identifiers of the navigator's entries after the three base
/// planes, in feature-tree order (§4.1).
fn entries(bytes: &[u8]) -> Option<Vec<u32>> {
    let mut at = NAVIGATOR_GUID_AT.checked_add(NAVIGATOR_GUID.len())?;
    skip_list(bytes, &mut at)?;
    skip_list(bytes, &mut at)?;
    skip(bytes, &mut at, BLOCK_BEFORE_NAME)?;
    let name_len = usize::try_from(word(bytes, &mut at)?).ok()?;
    if name_len != NAVIGATOR_NAME.len()
        || bytes.get(at..at.checked_add(name_len)?)? != NAVIGATOR_NAME
    {
        return None;
    }
    skip(bytes, &mut at, name_len)?;
    skip(bytes, &mut at, BLOCK_AFTER_NAME)?;
    skip(bytes, &mut at, WORDS_BEFORE_COUNT)?;
    let count = usize::try_from(word(bytes, &mut at)?).ok()?;
    if count < BASE_PLANES {
        return None;
    }
    if count > bytes.len().checked_sub(at)? / 8 {
        return None;
    }
    let mut features = Vec::new();
    for index in 0..count {
        if word(bytes, &mut at)? != 0 {
            return None;
        }
        let object = word(bytes, &mut at)?;
        if index >= BASE_PLANES {
            features.push(object);
        }
    }
    // The final word, then at most one more word.
    word(bytes, &mut at)?;
    matches!(bytes.len().checked_sub(at)?, 0 | 4).then_some(features)
}

/// The kind of each listed object. An identifier listed twice makes the list
/// unreadable; an object that does not read is `Kind::Unreadable`.
fn listed(cluster: &Cluster<'_>, types: &TypeTable, entries: &[u32]) -> Option<Vec<Listed>> {
    let mut seen = BTreeSet::new();
    let mut features = Vec::new();
    for &id in entries {
        if !seen.insert(id) {
            return None;
        }
        let kind = match cluster.object(id) {
            Some(object) => match types.class_name(object.form, object.flags) {
                Some(PART_CLASS) => Kind::Function(function_name(&object)),
                Some(GROUP_CLASS) => Kind::Group,
                Some(FOLDER_CLASS) => Kind::Folder,
                Some(other) => Kind::Other(other.to_owned()),
                None => Kind::Unreadable,
            },
            None => Kind::Unreadable,
        };
        features.push(Listed { object: id, kind });
    }
    Some(features)
}

/// The function name of a `CLSID_JMPart` object (§4.2): a counted block at
/// offset 132, then a counted ASCII name ending with one zero byte.
fn function_name(object: &Object<'_>) -> Option<String> {
    function_span(object).map(|(name, _)| name)
}

/// The function name of a `CLSID_JMPart` object and the object offset after
/// its counted name, including the zero byte (§4.2).
pub(crate) fn function_span(object: &Object<'_>) -> Option<(String, usize)> {
    let bytes = object.bytes;
    let block = usize::try_from(View::u32_le_at(bytes, FUNCTION_BLOCK_AT)?).ok()?;
    let count_at = FUNCTION_BLOCK_AT.checked_add(4)?.checked_add(block)?;
    let count = usize::try_from(View::u32_le_at(bytes, count_at)?).ok()?;
    let start = count_at.checked_add(4)?;
    let end = start.checked_add(count)?;
    let (terminator, name) = bytes.get(start..end)?.split_last()?;
    if *terminator != 0 || !name.is_ascii() {
        return None;
    }
    Some((String::from_utf8(name.to_vec()).ok()?, end))
}

/// The operation family of a function name. The families are the decoder's
/// reading of each function: a protrusion and a cutout extrude, a thin wall
/// shells, and a base tab and a flange are sheet-metal features.
fn function_family(function: &str) -> Option<OperationFamily> {
    Some(match function {
        "JMProtrusionFunction" | "JMCutoutFunction" => OperationFamily::Extrude,
        "JMRevolvedProtrusionFunction" => OperationFamily::Revolve,
        "JMMultiSweepProtrusionFunction" => OperationFamily::Sweep,
        "JMLoftedProtrusionFunction" => OperationFamily::Loft,
        "JMHoleFunction" => OperationFamily::Hole,
        "JMRoundFunction" => OperationFamily::Fillet,
        "JMChamferFunction" => OperationFamily::Chamfer,
        "JMThinwallAssocFunctionNew" => OperationFamily::Shell,
        "JMBaseTabFunction"
        | "JMFlangeFunction"
        | "JMMultiFlangeFunction"
        | "JMBaseContourFlangeFunction"
        | "JMHemAssocFunction"
        | "JMUnfoldFunction"
        | "JMRefoldFunction" => OperationFamily::SheetMetal,
        _ => return None,
    })
}

/// The operation of a listed feature, and the name that no family covers.
fn operation(kind: &Kind) -> Option<(FeatureOperation, Option<String>)> {
    let family = match kind {
        Kind::Group => Some(OperationFamily::Pattern),
        Kind::Function(Some(function)) => function_family(function),
        Kind::Function(None) | Kind::Other(_) => None,
        Kind::Folder | Kind::Unreadable => return None,
    };
    if let Some(family) = family {
        return Some((FeatureOperation::Unresolved { family }, None));
    }
    let name = match kind {
        Kind::Function(Some(function)) => function.as_str(),
        Kind::Function(None) => PART_CLASS,
        Kind::Other(class) => class.as_str(),
        Kind::Group | Kind::Folder | Kind::Unreadable => return None,
    };
    let native = FeatureOperation::Native {
        kind: CodecName::new(&cadmpeg_ir::codec_id!("solidedge"), name)?,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    };
    Some((native, Some(name.to_owned())))
}

pub(crate) fn feature_id(object: u32) -> FeatureId {
    FeatureId::compose(
        &cadmpeg_ir::identity_namespace!("solidedge", "model", "feature"),
        object,
    )
}

fn container_id() -> ContainerId {
    ContainerId::from(cadmpeg_ir::ids::Identity::compose(
        &cadmpeg_ir::identity_namespace!("solidedge", "model", "container"),
        cadmpeg_ir::identity_key!("timeline"),
    ))
}

/// The features of `listed` in order, with a loss for each kind no family
/// covers.
fn timeline(listed: &[Listed], losses: &mut Vec<LossNote>) -> Option<Vec<Feature>> {
    let mut features = Vec::new();
    let folders = listed
        .iter()
        .filter(|entry| entry.kind == Kind::Folder)
        .count();
    if folders > 0 {
        losses.push(SolidEdgeLossCode::FeatureFolderUnread.note(format!(
            "the feature tree has {folders} folder(s) whose members are not read; the features \
             are stated without folders"
        )));
    }
    for entry in listed.iter().filter(|entry| entry.kind != Kind::Folder) {
        if entry.kind == Kind::Unreadable {
            losses.push(SolidEdgeLossCode::FeatureStorageUnread.note(format!(
                "listed feature object {:#x} does not read; it is not stated",
                entry.object
            )));
            continue;
        }
        let (operation, unmapped) = operation(&entry.kind)?;
        if let Some(name) = unmapped {
            losses.push(SolidEdgeLossCode::FeatureKindUnknown.note(format!(
                "feature object {:#x} is of kind {name}, which has no operation family; it is \
                 stated as a native operation",
                entry.object
            )));
        }
        features.push(Feature {
            id: feature_id(entry.object),
            name: None,
            suppressed: None,
            definition: FeatureDefinition {
                operation,
                assembly: None,
                refine: None,
                fuzzy_tolerance: None,
            },
            placement: None,
            component: None,
            driven_by: None,
            native_ref: None,
        });
    }
    Some(features)
}

/// Read the feature order of the document and state it as one timeline on
/// `definition`. A stream that does not parse adds `feature.storage-unread`.
/// A document with feature objects and no navigator adds
/// `feature.navigator-absent`; one with no feature objects, such as an
/// assembly, adds nothing. Features state their family and no operands.
pub(crate) fn project<'a>(
    ctx: &DecodeContext<'a>,
    snapshot: &CompoundSnapshot<'a>,
    cluster: View<'a>,
    definition: &ProductDefinitionId,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let (Some(map), Some(types)) = (
        snapshot.stream(SPACE_MAP_STREAM),
        snapshot.stream(TYPE_TABLE_STREAM),
    ) else {
        return Ok(());
    };
    let streams = match (snapshot.open(ctx, map), snapshot.open(ctx, types)) {
        (Ok(map), Ok(types)) => (map, types),
        (Err(error @ CodecError::ResourceLimit(_)), _)
        | (_, Err(error @ CodecError::ResourceLimit(_))) => return Err(error),
        (Err(error), _) | (_, Err(error)) => {
            losses.push(SolidEdgeLossCode::FeatureStorageUnread.note(format!(
                "the PSM space map or type table could not be read: {error}; the feature order \
                 is not transferred"
            )));
            return Ok(());
        }
    };
    let (map, types) = streams;
    let (Some(map), Some(types)) = (
        SpaceMap::parse(map.window()),
        TypeTable::parse(types.window()),
    ) else {
        losses.push(SolidEdgeLossCode::FeatureStorageUnread.note(
            "the PSM space map or type table does not parse; the feature order is not transferred",
        ));
        return Ok(());
    };
    let Some(cluster) = Cluster::parse(cluster.window(), &map) else {
        losses.push(SolidEdgeLossCode::FeatureStorageUnread.note(
            "the PSM cluster does not match its space map; the feature order is not transferred",
        ));
        return Ok(());
    };
    let listed = match navigator(&cluster, &types) {
        Navigator::Features(listed) => listed,
        Navigator::Absent => {
            if has_feature_objects(&cluster, &types) {
                losses.push(SolidEdgeLossCode::FeatureNavigatorAbsent.note(
                    "the document has feature objects and no part feature navigator; the feature \
                     order is not transferred",
                ));
            }
            return Ok(());
        }
        Navigator::Unread => {
            losses.push(SolidEdgeLossCode::FeatureStorageUnread.note(
                "the feature navigator does not parse; the feature order is not transferred",
            ));
            return Ok(());
        }
    };
    let mut notes = Vec::new();
    let Some(features) = timeline(&listed, &mut notes) else {
        losses.push(
            SolidEdgeLossCode::FeatureStorageUnread.note(
                "a listed feature has no readable kind; the feature order is not transferred",
            ),
        );
        return Ok(());
    };
    if features.is_empty() {
        return Ok(());
    }
    losses.extend(notes);
    let id = container_id();
    let container = Container::Ordered {
        id: id.clone(),
        kind: OrderedKind::Timeline,
        members: features
            .iter()
            .map(|feature| Member::Feature(feature.id.clone()))
            .collect(),
        folders: Vec::new(),
        rollback: None,
        placement: None,
    };
    ir.design.scopes.push(DesignScope {
        definition: definition.clone(),
        roots: vec![id],
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: Vec::new(),
        unproduced: BTreeSet::new(),
    });
    ir.design.containers.push(container);
    ir.design.features.extend(features);
    crate::sketches::project(ctx, &cluster, &types, &listed, ir, losses)?;
    crate::extents::project(&cluster, &types, &listed, ir, losses);
    crate::sweeps::project(&cluster, &types, &listed, ir, losses);
    crate::sheet_metal::project_base_tabs(&cluster, &types, &listed, ir, losses);
    crate::sheet_metal::flange::project(&cluster, &types, &listed, ir, losses);
    crate::sheet_metal::hem::project(&cluster, &types, &listed, ir, losses);
    crate::sheet_metal::contour::project(&cluster, &types, &listed, ir, losses);
    crate::sheet_metal::unfold::project(&cluster, &types, &listed, ir, losses);
    crate::sheet_metal::corner::project(&cluster, &types, &listed, ir);
    crate::lofts::project(&cluster, &types, &listed, ir, losses);
    crate::dressup::project(&cluster, &types, &listed, ir, losses);
    crate::patterns::project(&cluster, &types, &listed, ir, losses);
    crate::sketches::name_untyped_profiles(&listed, ir);
    let pending = ir
        .design
        .features
        .iter()
        .filter(|feature| {
            matches!(
                feature.definition.operation,
                FeatureOperation::Unresolved { .. } | FeatureOperation::Native { .. }
            )
        })
        .count();
    if pending > 0 {
        losses.push(SolidEdgeLossCode::FeatureOperandsPending.note(format!(
            "the operands of {pending} feature(s) are not decoded; each states its operation \
             family only"
        )));
    }
    losses.push(SolidEdgeLossCode::FeatureRollbackUnread.note(
        "the rollback position and suppressed state of the features are not read; the timeline \
         states no rollback position",
    ));
    Ok(())
}

#[cfg(test)]
mod tests;
