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

fn evidence<'a>(paths: impl IntoIterator<Item = &'a str>) -> bool {
    let mut sites = false;
    let mut roots = false;
    for path in paths {
        sites |= path.eq_ignore_ascii_case("JSitesList");
        roots |= path.eq_ignore_ascii_case("PSMroots");
    }
    sites && roots
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

#[cfg(test)]
mod tests {
    use super::evidence;

    #[test]
    fn identity_requires_both_root_paths() {
        assert!(evidence(["PSMroots", "JSitesList"]));
        assert!(evidence(["psmroots", "jsiteslist"]));
        for paths in [
            ["PSMroots", "Payload"],
            ["Payload", "JSitesList"],
            ["Nested/PSMroots", "JSitesList"],
            ["PSMroots", "Nested/JSitesList"],
        ] {
            assert!(!evidence(paths));
        }
    }
}
