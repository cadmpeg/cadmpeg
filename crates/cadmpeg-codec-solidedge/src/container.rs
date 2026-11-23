// SPDX-License-Identifier: Apache-2.0
//! Solid Edge compound directory access.

use cadmpeg_container::compound::{CompoundEntry, CompoundPrefixProbe, CompoundSnapshot};
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

fn evidence<'a>(paths: impl IntoIterator<Item = &'a str>) -> bool {
    let (mut sites, mut roots, mut info, mut publish) = (false, false, false, false);
    for path in paths {
        sites |= path.eq_ignore_ascii_case(SITES_LIST);
        roots |= path.eq_ignore_ascii_case(PSM_ROOTS);
        info |= path.eq_ignore_ascii_case(FAMILY_INFO);
        publish |= path.eq_ignore_ascii_case(FAMILY_PUBLISH);
    }
    (sites && roots) || (info && publish)
}

pub(crate) fn detect(ctx: &DecodeContext<'_>, prefix: View<'_>) -> Result<Confidence, CodecError> {
    Ok(match CompoundPrefixProbe::inspect(ctx, prefix)? {
        CompoundPrefixProbe::DirectoryEvidence(paths)
            if evidence(paths.iter().map(String::as_str)) =>
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
    if !evidence(snapshot.entries().iter().map(CompoundEntry::path)) {
        return Err(CodecError::WrongFormat(
            "Solid Edge directory signature is absent".into(),
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
