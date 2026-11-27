// SPDX-License-Identifier: Apache-2.0
//! `IronCAD` compound directory access.

use cadmpeg_container::compound::{
    CompoundClassId, CompoundEntry, CompoundPrefixProbe, CompoundSnapshot,
};
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Confidence;
use cadmpeg_ir::{ContainerKind, ContainerSummary};

use crate::dialect;
use crate::loss::IronCadLossCode;

/// Stream that holds the kernel record tree.
pub(crate) const BREP_DATA: &str = "Brep Data";

/// The stream that holds the part and assembly records.
pub(crate) const PUBLIC_ACCESS: &str = "Public Access";

/// Root streams that identify a scene, with [`BREP_DATA`].
const SHAPE_DATA: &str = "Shape Data";
pub(crate) const DOCUMENT_DATA: &str = "Document Data";

/// Root storage class of a scene.
pub(crate) const SCENE_CLASS: CompoundClassId = CompoundClassId::from_guid(
    0x0002_c52b,
    0x0000,
    0x0000,
    [0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
);

fn evidence<'a>(paths: impl IntoIterator<Item = &'a str>, class: CompoundClassId) -> bool {
    let (mut brep, mut shape, mut document) = (false, false, false);
    for path in paths {
        brep |= path.eq_ignore_ascii_case(BREP_DATA);
        shape |= path.eq_ignore_ascii_case(SHAPE_DATA);
        document |= path.eq_ignore_ascii_case(DOCUMENT_DATA);
    }
    brep && shape && document && class == SCENE_CLASS
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
            "IronCAD scene streams or root class are absent".into(),
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
        .map(|deviation| IronCadLossCode::ContainerNoncanonical.note(deviation.description()))
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
