// SPDX-License-Identifier: Apache-2.0
//! Solid Edge compound directory access.

use cadmpeg_container::compound::{
    CompoundClassId, CompoundEntry, CompoundPrefixProbe, CompoundSnapshot, CompoundStreamEntry,
};
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Confidence;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::{ContainerKind, ContainerSummary};

use std::collections::BTreeSet;

use crate::dialect;
use crate::family;
use crate::loss::SolidEdgeLossCode;

/// Root entries that identify a Solid Edge document.
const SITES_LIST: &str = "JSitesList";
const PSM_ROOTS: &str = "PSMroots";
/// Root entries that identify a family-of-assemblies document.
const FAMILY_INFO: &str = "FamilyInfo";
const FAMILY_PUBLISH: &str = "FOAPublishData";

/// Root storage classes of part, sheet-metal, and assembly documents.
const DOCUMENT_CLASSES: [CompoundClassId; 3] = [
    CompoundClassId::from_guid(
        0x23c5_2e80,
        0x4698,
        0x11ce,
        [0xb3, 0x07, 0x08, 0x00, 0x36, 0x3a, 0x1e, 0x02],
    ),
    CompoundClassId::from_guid(
        0xdd85_22e0,
        0x2375,
        0x11d0,
        [0xac, 0x05, 0x08, 0x00, 0x36, 0xfd, 0x18, 0x02],
    ),
    CompoundClassId::from_guid(
        0x00c6_bf00,
        0x483b,
        0x11ce,
        [0x95, 0x1a, 0x08, 0x00, 0x36, 0x01, 0xbe, 0x52],
    ),
];
/// Root storage class of a family-of-assemblies document.
const FAMILY_CLASS: CompoundClassId = CompoundClassId::from_guid(
    0x04d6_13a0,
    0xa322,
    0x40b5,
    [0xa2, 0xa4, 0x36, 0xca, 0x0d, 0xe6, 0xf5, 0xd9],
);

fn evidence<'a>(paths: impl IntoIterator<Item = &'a str>, class: CompoundClassId) -> bool {
    let (mut sites, mut roots, mut info, mut publish) = (false, false, false, false);
    for path in paths {
        sites |= path.eq_ignore_ascii_case(SITES_LIST);
        roots |= path.eq_ignore_ascii_case(PSM_ROOTS);
        info |= path.eq_ignore_ascii_case(FAMILY_INFO);
        publish |= path.eq_ignore_ascii_case(FAMILY_PUBLISH);
    }
    (sites && roots && DOCUMENT_CLASSES.contains(&class))
        || (info && publish && class == FAMILY_CLASS)
}

pub(crate) fn detect(ctx: &DecodeContext<'_>, prefix: View<'_>) -> Result<Confidence, CodecError> {
    Ok(match CompoundPrefixProbe::inspect(ctx, prefix)? {
        CompoundPrefixProbe::DirectoryEvidence { paths, root_class }
            if evidence(paths.iter().map(String::as_str), root_class) =>
        {
            Confidence::High
        }
        _ => Confidence::No,
    })
}

pub(crate) fn open<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
) -> Result<CompoundSnapshot<'a>, CodecError> {
    let snapshot = CompoundSnapshot::new(ctx, root)?;
    if !evidence(
        snapshot.entries().iter().map(CompoundEntry::path),
        snapshot.root_class(),
    ) {
        return Err(CodecError::WrongFormat(
            "Solid Edge directory signature or root class is absent".into(),
        ));
    }
    Ok(snapshot)
}

/// The storage whose streams make up the document: the root of an ordinary
/// document, the active member storage of a family of assemblies.
pub(crate) struct DocStreams<'s, 'a> {
    snapshot: &'s CompoundSnapshot<'a>,
    /// The storage path with its trailing separator; empty at the root. `None`
    /// when the family names no member storage.
    prefix: Option<String>,
    /// The identifiers of the occurrence objects the active family member
    /// lists; `None` places every occurrence of the storage.
    listed: Option<BTreeSet<u32>>,
}

impl<'s, 'a> DocStreams<'s, 'a> {
    /// Select the member of `snapshot`. A family document with no readable
    /// active member adds `family.member-unread` and states no streams.
    pub(crate) fn select(
        ctx: &DecodeContext<'a>,
        snapshot: &'s CompoundSnapshot<'a>,
        losses: &mut Vec<LossNote>,
    ) -> Result<Self, CodecError> {
        if snapshot.stream(SITES_LIST).is_some() {
            return Ok(Self {
                snapshot,
                prefix: Some(String::new()),
                listed: None,
            });
        }
        let name = match snapshot.stream(FAMILY_INFO) {
            Some(entry) => family::active_name(snapshot.open(ctx, entry)?.window()),
            None => None,
        };
        let prefix = name
            .clone()
            .filter(|name| !name.contains('/'))
            .map(|name| format!("{name}/"))
            .filter(|prefix| {
                snapshot.stream(&format!("{prefix}{SITES_LIST}")).is_some()
                    && snapshot.stream(&format!("{prefix}{PSM_ROOTS}")).is_some()
            });
        if prefix.is_none() {
            losses.push(SolidEdgeLossCode::FamilyMemberUnread.note(
                "the family of assemblies names no member storage that holds the document \
                 streams; no member is read",
            ));
        }
        let mut listed = None;
        if let (Some(name), Some(_)) = (&name, &prefix) {
            let table = match snapshot.stream(family::MEMBERS_PATH) {
                Some(entry) => {
                    family::listed_occurrences(snapshot.open(ctx, entry)?.window(), name)
                }
                None => None,
            };
            if table.is_none() {
                losses.push(SolidEdgeLossCode::FamilyMemberUnread.note(
                    "the member table has no readable record for the active member; every \
                     occurrence of the member storage is kept",
                ));
            }
            listed = table;
        }
        Ok(Self {
            snapshot,
            prefix,
            listed,
        })
    }

    /// Whether the active member places the occurrence object `id`.
    pub(crate) fn lists(&self, id: u32) -> bool {
        self.listed
            .as_ref()
            .is_none_or(|listed| listed.contains(&id))
    }

    /// The stream `path` of the member.
    pub(crate) fn stream(&self, path: &str) -> Option<&'s CompoundStreamEntry> {
        let prefix = self.prefix.as_deref()?;
        self.snapshot.stream(&format!("{prefix}{path}"))
    }

    /// Open a stream of the snapshot.
    pub(crate) fn open(
        &self,
        ctx: &DecodeContext<'a>,
        entry: &CompoundStreamEntry,
    ) -> Result<View<'a>, CodecError> {
        self.snapshot.open(ctx, entry)
    }
}

pub(crate) fn summary(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
) -> Result<ContainerSummary, CodecError> {
    let snapshot = open(ctx, root)?;
    let losses = snapshot
        .deviations()
        .map(|deviation| SolidEdgeLossCode::ContainerNoncanonical.note(deviation.description()))
        .collect();
    Ok(ContainerSummary::classified(
        dialect::layers(),
        ContainerKind::Cfb,
        snapshot.container_entries(|entry| match entry {
            CompoundEntry::Storage(_) => ContainerRole::Storage,
            CompoundEntry::Stream(_) => ContainerRole::Stream,
        }),
        losses,
        Vec::new(),
    ))
}
