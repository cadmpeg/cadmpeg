// SPDX-License-Identifier: Apache-2.0
//! The ordered feature tree of a document (`docs/formats/solidedge.md` §4),
//! projected as one timeline of features that state their operation family
//! and no operands.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_container::compound::CompoundSnapshot;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::container::{Container, Folder, OrderedKind, Rollback};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::names::CodecName;
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::report::loss::LossNote;

use crate::layout::{folder_object, part_feature_object};
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
    /// The navigator's last-feature identifier and the features in
    /// feature-tree order.
    Features { last: u32, listed: Vec<Listed> },
}

/// One listed feature: its object and what its class states of its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Listed {
    pub(crate) object: u32,
    pub(crate) kind: Kind,
    /// Whether the feature is suppressed; `None` for a class that states no
    /// state word and for an unreadable one.
    pub(crate) suppressed: Option<bool>,
}

/// The state-word bit of a part function that states it suppressed (§4.3).
const SUPPRESSED_BIT: u32 = 0x10;

/// The navigator's last-feature identifier and its listed object identifiers.
struct Entries {
    last: u32,
    objects: Vec<u32>,
}

/// What a feature object states of its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A part function and its function name; `None` where the stored block
    /// does not read.
    Function(Option<String>),
    /// A mirror copy or pattern.
    Group,
    /// A feature-tree folder and its member object identifiers; `None` where
    /// the member list does not read.
    Folder(Option<Vec<u32>>),
    /// An object of another class.
    Other(String),
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
            let last = entries.last;
            return listed(cluster, types, &entries.objects).map_or(Navigator::Unread, |listed| {
                Navigator::Features { last, listed }
            });
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

/// The last-feature identifier and the object identifiers of the navigator's
/// entries after the three base planes, in feature-tree order (§4.1).
fn entries(bytes: &[u8]) -> Option<Entries> {
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
    skip(bytes, &mut at, WORDS_BEFORE_COUNT - 4)?;
    let last = word(bytes, &mut at)?;
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
    matches!(bytes.len().checked_sub(at)?, 0 | 4).then_some(Entries {
        last,
        objects: features,
    })
}

/// The kind of each listed object. An identifier the cluster does not hold,
/// or one listed twice, makes the list unreadable.
fn listed(cluster: &Cluster<'_>, types: &TypeTable, entries: &[u32]) -> Option<Vec<Listed>> {
    let mut seen = BTreeSet::new();
    let mut features = Vec::new();
    for &id in entries {
        if !seen.insert(id) {
            return None;
        }
        let object = cluster.object(id)?;
        let class = types.class_name(object.form, object.flags)?;
        let kind = match class {
            PART_CLASS => Kind::Function(function_name(&object)),
            GROUP_CLASS => Kind::Group,
            FOLDER_CLASS => Kind::Folder(folder_members(&object)),
            other => Kind::Other(other.to_owned()),
        };
        let suppressed = (class == PART_CLASS)
            .then(|| View::u32_le_at(object.bytes, part_feature_object::STATE))
            .flatten()
            .map(|state| state & SUPPRESSED_BIT != 0);
        features.push(Listed {
            object: id,
            kind,
            suppressed,
        });
    }
    Some(features)
}

/// The member object identifiers of a `CLSID_JMFeatureGroup` object (§4.3): a
/// `u32` count and that many pairs of the `u32` value 1 and an identifier.
fn folder_members(object: &Object<'_>) -> Option<Vec<u32>> {
    let bytes = object.bytes;
    let count = usize::try_from(View::u32_le_at(bytes, folder_object::MEMBER_COUNT)?).ok()?;
    let mut at = folder_object::MEMBER_COUNT.checked_add(4)?;
    if count > bytes.len().checked_sub(at)? / 8 {
        return None;
    }
    let mut members = Vec::new();
    for _ in 0..count {
        if word(bytes, &mut at)? != 1 {
            return None;
        }
        members.push(word(bytes, &mut at)?);
    }
    Some(members)
}

/// The function name of a `CLSID_JMPart` object (§4.2): a counted block at
/// offset 132, then a counted ASCII name ending with one zero byte.
fn function_name(object: &Object<'_>) -> Option<String> {
    let bytes = object.bytes;
    let block = usize::try_from(View::u32_le_at(bytes, FUNCTION_BLOCK_AT)?).ok()?;
    let count_at = FUNCTION_BLOCK_AT.checked_add(4)?.checked_add(block)?;
    let count = usize::try_from(View::u32_le_at(bytes, count_at)?).ok()?;
    let start = count_at.checked_add(4)?;
    let (terminator, name) = bytes.get(start..start.checked_add(count)?)?.split_last()?;
    if *terminator != 0 || !name.is_ascii() {
        return None;
    }
    String::from_utf8(name.to_vec()).ok()
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
        "JMBaseTabFunction" | "JMFlangeFunction" => OperationFamily::SheetMetal,
        _ => return None,
    })
}

/// The operation of a listed feature, and the name that no family covers.
fn operation(kind: &Kind) -> Option<(FeatureOperation, Option<String>)> {
    let family = match kind {
        Kind::Group => Some(OperationFamily::Pattern),
        Kind::Function(Some(function)) => function_family(function),
        Kind::Function(None) | Kind::Other(_) => None,
        Kind::Folder(_) => return None,
    };
    if let Some(family) = family {
        return Some((FeatureOperation::Unresolved { family }, None));
    }
    let name = match kind {
        Kind::Function(Some(function)) => function.as_str(),
        Kind::Function(None) => PART_CLASS,
        Kind::Other(class) => class.as_str(),
        Kind::Group | Kind::Folder(_) => return None,
    };
    let native = FeatureOperation::Native {
        kind: CodecName::new(&cadmpeg_ir::codec_id!("solidedge"), name)?,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    };
    Some((native, Some(name.to_owned())))
}

fn feature_id(object: u32) -> FeatureId {
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

/// The features of a timeline and the folders over them.
struct Timeline {
    features: Vec<Feature>,
    folders: Vec<Folder>,
}

/// The index run of a folder's members among the timeline features: every
/// member is a timeline feature and the indices are consecutive ascending.
fn folder_run(members: &[u32], index: &BTreeMap<u32, u32>) -> Option<Folder> {
    let mut first = None;
    let mut previous: Option<u32> = None;
    for member in members {
        let at = *index.get(member)?;
        if let Some(previous) = previous {
            if previous.checked_add(1)? != at {
                return None;
            }
        } else {
            first = Some(at);
        }
        previous = Some(at);
    }
    Some(Folder {
        name: None,
        first: first?,
        last: previous?,
    })
}

/// The features of `listed` in order, the folders over them, and a loss for
/// each kind no family covers, each folder that does not state a run of
/// features and each feature with no readable suppression state.
fn timeline(listed: &[Listed], losses: &mut Vec<LossNote>) -> Option<Timeline> {
    let mut features = Vec::new();
    let mut index = BTreeMap::new();
    let mut unread_state = 0_usize;
    for entry in listed
        .iter()
        .filter(|entry| !matches!(entry.kind, Kind::Folder(_)))
    {
        let (operation, unmapped) = operation(&entry.kind)?;
        if let Some(name) = unmapped {
            losses.push(SolidEdgeLossCode::FeatureKindUnknown.note(format!(
                "feature object {:#x} is of kind {name}, which has no operation family; it is \
                 stated as a native operation",
                entry.object
            )));
        }
        if entry.suppressed.is_none() {
            unread_state += 1;
        }
        index.insert(entry.object, u32::try_from(features.len()).ok()?);
        features.push(Feature {
            id: feature_id(entry.object),
            name: None,
            suppressed: entry.suppressed.map(Dimension::Literal),
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
    let mut folders = Vec::new();
    let mut any_folder = false;
    for entry in listed {
        let Kind::Folder(members) = &entry.kind else {
            continue;
        };
        any_folder = true;
        match members
            .as_deref()
            .and_then(|members| folder_run(members, &index))
        {
            Some(folder) => folders.push(folder),
            None => losses.push(SolidEdgeLossCode::FeatureFolderUnread.note(format!(
                "folder object {:#x} does not list a run of consecutive features; it is not stated",
                entry.object
            ))),
        }
    }
    if any_folder {
        losses.push(SolidEdgeLossCode::FeatureFolderNameUnread.note("folder names are not read"));
    }
    if unread_state > 0 {
        losses.push(SolidEdgeLossCode::FeatureSuppressionUnread.note(format!(
            "the suppressed state of {unread_state} feature(s) is not read"
        )));
    }
    Some(Timeline { features, folders })
}

/// The rollback position that the navigator's last-feature identifier states
/// over the timeline `members`. An identifier that names no member states no
/// position and adds `feature.rollback-unread`.
fn rollback(
    last: u32,
    objects: &[u32],
    members: &[Member],
    losses: &mut Vec<LossNote>,
) -> Option<Rollback> {
    match objects.iter().position(|object| *object == last) {
        Some(at) => match at.checked_add(1).and_then(|next| members.get(next)) {
            Some(next) => Some(Rollback::Before(next.clone())),
            None => Some(Rollback::End),
        },
        None => {
            losses.push(SolidEdgeLossCode::FeatureRollbackUnread.note(format!(
                "the navigator's last feature {last:#x} is not a listed feature; the timeline \
                 states no rollback position"
            )));
            None
        }
    }
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
    let map = snapshot.open(ctx, map)?;
    let types = snapshot.open(ctx, types)?;
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
    let (last, listed) = match navigator(&cluster, &types) {
        Navigator::Features { last, listed } => (last, listed),
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
    let Some(Timeline { features, folders }) = timeline(&listed, &mut notes) else {
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
    losses.push(SolidEdgeLossCode::FeatureOperandsPending.note(
        "the operands of the features are not decoded; each feature states its operation family \
         only",
    ));
    let members: Vec<Member> = features
        .iter()
        .map(|feature| Member::Feature(feature.id.clone()))
        .collect();
    let objects: Vec<u32> = listed
        .iter()
        .filter(|entry| !matches!(entry.kind, Kind::Folder(_)))
        .map(|entry| entry.object)
        .collect();
    let rollback = rollback(last, &objects, &members, losses);
    let id = container_id();
    let container = Container::Ordered {
        id: id.clone(),
        kind: OrderedKind::Timeline,
        members,
        folders,
        rollback,
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
    Ok(())
}

#[cfg(test)]
mod tests;
