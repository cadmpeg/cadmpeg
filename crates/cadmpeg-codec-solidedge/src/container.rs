// SPDX-License-Identifier: Apache-2.0
//! Solid Edge compound directory access.

use cadmpeg_container::compound::{
    CompoundClassId, CompoundEntry, CompoundPrefixProbe, CompoundSnapshot,
};
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Confidence;
use cadmpeg_ir::{ContainerKind, ContainerSummary};

use crate::dialect;
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
