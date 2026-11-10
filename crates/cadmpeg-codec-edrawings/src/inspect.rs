// SPDX-License-Identifier: Apache-2.0
//! Package inspection: every member and its role, and the model stream
//! wrapper.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::CodecError;
use cadmpeg_ir::{ContainerKind, ContainerSummary};

use crate::package::{MemberRole, Package};
use crate::stream::{read_wrapper, Wrapper};

/// Entry attribute naming the stream version of the model member.
const ATTRIBUTE_HSF_VERSION: &str = "hsf_version";
/// Entry attribute naming why the model member wrapper did not read.
const ATTRIBUTE_WRAPPER_ERROR: &str = "wrapper_error";

pub(crate) fn inspect<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
) -> Result<ContainerSummary, CodecError> {
    let package = Package::open(ctx, root)?;
    let model = package.model(ctx)?;
    let wrapper = match read_wrapper(model) {
        Ok(wrapper) => Ok(wrapper),
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(error) => Err(error),
    };
    let classification = crate::dialect::classify(wrapper.as_ref().ok().map(|w| &w.version));
    let mut entries = package.entries();
    for entry in &mut entries {
        if MemberRole::of(&entry.name) != MemberRole::Model {
            continue;
        }
        match &wrapper {
            Ok(wrapper) => {
                entry.attributes.insert(
                    ATTRIBUTE_HSF_VERSION.to_owned(),
                    wrapper.version.text().to_owned(),
                );
            }
            Err(error) => {
                entry
                    .attributes
                    .insert(ATTRIBUTE_WRAPPER_ERROR.to_owned(), error.to_string());
            }
        }
    }
    let losses = package
        .recovery_loss()
        .into_iter()
        .chain(classification.loss())
        .collect();
    let mut notes = vec![format!("{} members", package.member_count())];
    if let Ok(wrapper) = &wrapper {
        notes.push(describe(wrapper));
    }
    Ok(ContainerSummary::classified(
        DialectLayers::of(classification.into_matched()),
        ContainerKind::Zip,
        entries,
        losses,
        notes,
    ))
}

/// One line naming the wrapper records.
fn describe(wrapper: &Wrapper) -> String {
    let info = wrapper
        .info
        .map_or_else(|| "absent".to_owned(), |value| value.to_string());
    let bounds = wrapper.bounds.map_or_else(
        || "absent".to_owned(),
        |bounds| format!("{:?}..{:?}", bounds.min, bounds.max),
    );
    format!(
        "HSF V{}; info record {info}; bounds {bounds}; model stream at offset {}",
        wrapper.version.text(),
        wrapper.body_offset
    )
}
