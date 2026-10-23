// SPDX-License-Identifier: Apache-2.0
//! Package inspection: every member, its role, and its signature.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::CodecError;
use cadmpeg_ir::{ContainerKind, ContainerSummary};

use crate::package::{MemberRole, Package};

/// Entry attribute naming the member's leading signature.
const ATTRIBUTE_SIGNATURE: &str = "signature";
/// Entry attribute naming why a member could not be opened.
const ATTRIBUTE_OPEN_ERROR: &str = "open_error";
/// Signature attribute value of a member with no known signature.
const UNRECOGNIZED_SIGNATURE: &str = "unrecognized";

pub(crate) fn inspect<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
) -> Result<ContainerSummary, CodecError> {
    let package = Package::open(ctx, root)?;
    let structure = package.structure(ctx)?;
    let classification = crate::dialect::classify(structure);
    let mut entries = package.entries();
    for entry in &mut entries {
        let signature = match MemberRole::of(&entry.name) {
            MemberRole::Structure => {
                crate::pvs::generation(structure).map(crate::pvs::Generation::signature)
            }
            MemberRole::Geometry => match package.member(ctx, &entry.name) {
                Ok(member) => crate::ol::generation(member).map(crate::ol::Generation::signature),
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(error) => {
                    entry
                        .attributes
                        .insert(ATTRIBUTE_OPEN_ERROR.to_owned(), error.to_string());
                    continue;
                }
            },
            MemberRole::Other => continue,
        };
        entry.attributes.insert(
            ATTRIBUTE_SIGNATURE.to_owned(),
            signature.unwrap_or(UNRECOGNIZED_SIGNATURE).to_owned(),
        );
    }
    let losses = package
        .recovery_loss()
        .into_iter()
        .chain(classification.loss())
        .collect();
    let notes = vec![format!(
        "structure member {}; {} geometry members",
        package.structure_name(),
        package.geometry_names().len()
    )];
    Ok(ContainerSummary::classified(
        DialectLayers::of(classification.into_matched()),
        ContainerKind::Zip,
        entries,
        losses,
        notes,
    ))
}
