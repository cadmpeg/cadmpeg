// SPDX-License-Identifier: Apache-2.0
//! Alibre compound-container classification and stream access.

use cadmpeg_container::compound::{CompoundEntry, CompoundSnapshot};
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::ContainerSummary;

use crate::dialect::KernelStream;
use crate::loss::AlibreLossCode;

/// Stream holding the part body as an ACIS binary stream.
pub(crate) const SAB_STREAM: &str = "SAB_DATA";
/// Stream holding the Alibre feature model as STEP text.
pub(crate) const STEP_STREAM: &str = "STEP_DATA";
const PROPERTIES_STREAM: &str = "PROP_DATA";
const VERSION_STREAM: &str = "VERSION";

/// Whether directory paths carry both body streams of a part.
pub(crate) fn has_alibre_evidence(paths: &[String]) -> bool {
    [SAB_STREAM, STEP_STREAM]
        .iter()
        .all(|name| paths.iter().any(|path| path.eq_ignore_ascii_case(name)))
}

/// One parsed Alibre compound container.
pub(crate) struct AlibreContainer<'a> {
    snapshot: CompoundSnapshot<'a>,
}

impl<'a> AlibreContainer<'a> {
    pub(crate) fn open(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Self, CodecError> {
        let snapshot = CompoundSnapshot::new(ctx, root)?;
        let paths: Vec<String> = snapshot
            .entries()
            .iter()
            .map(|entry| entry.path().to_owned())
            .collect();
        if !has_alibre_evidence(&paths) {
            return Err(CodecError::WrongFormat(
                "not an Alibre part: the compound file lacks the SAB_DATA and STEP_DATA streams"
                    .into(),
            ));
        }
        Ok(Self { snapshot })
    }

    /// Opens one named stream of the container.
    pub(crate) fn stream(
        &self,
        ctx: &DecodeContext<'a>,
        name: &str,
    ) -> Result<View<'a>, CodecError> {
        let entry = self.snapshot.stream(name).ok_or_else(|| {
            CodecError::malformed(format_args!("Alibre part has no {name} stream"))
        })?;
        self.snapshot.open(ctx, entry)
    }

    /// One loss per noncanonical encoding the compound reader admitted.
    pub(crate) fn deviation_losses(&self) -> Vec<LossNote> {
        self.snapshot
            .deviations()
            .map(|deviation| AlibreLossCode::ContainerNoncanonical.note(deviation.description()))
            .collect()
    }

    /// The loss naming the streams whose content the IR does not carry.
    pub(crate) fn undecoded_streams_loss(&self) -> Option<LossNote> {
        let names: Vec<&str> = self
            .snapshot
            .entries()
            .iter()
            .filter_map(|entry| match entry {
                CompoundEntry::Stream(stream) => Some(stream.path()),
                CompoundEntry::Storage(_) => None,
            })
            .filter(|name| {
                !name.eq_ignore_ascii_case(SAB_STREAM) && !name.eq_ignore_ascii_case(STEP_STREAM)
            })
            .collect();
        (!names.is_empty()).then(|| {
            AlibreLossCode::SourceStreamsUndecoded
                .note(format!("streams without a decoder: {}", names.join(", ")))
        })
    }

    pub(crate) fn summary(
        &self,
        classification: crate::dialect::Classification,
        kernel: Option<&KernelStream>,
    ) -> Result<ContainerSummary, CodecError> {
        let mut entries = self.snapshot.container_entries(classify);
        if let Some(kernel) = kernel {
            let metadata = &kernel.header.metadata;
            if let Some(entry) = entries
                .iter_mut()
                .find(|entry| entry.name.eq_ignore_ascii_case(SAB_STREAM))
            {
                insert_attribute(entry, "kernel_family", "acis");
                for (key, value) in [
                    (
                        "acis_save_format_version",
                        metadata.save_format_version.map(|value| value.to_string()),
                    ),
                    ("product_family", metadata.product_family.clone()),
                    ("product_version", metadata.product_version.clone()),
                    (
                        "header_scale",
                        metadata.scale.map(|value| value.to_string()),
                    ),
                ] {
                    if let Some(value) = value {
                        insert_attribute(entry, key, value);
                    }
                }
            }
        }
        let mut losses = self.deviation_losses();
        losses.extend(classification.kernel_loss());
        let notes = vec![format!(
            "CFB v{} with {} streams",
            self.snapshot.major_version(),
            entries
                .iter()
                .filter(|entry| !entry.name.is_empty())
                .count()
        )];
        Ok(ContainerSummary::classified(
            classification.layers()?,
            cadmpeg_ir::ContainerKind::Cfb,
            entries,
            losses,
            notes,
        ))
    }
}

fn insert_attribute(entry: &mut ContainerEntry, key: &str, value: impl std::fmt::Display) {
    entry.attributes.insert(key.to_owned(), value.to_string());
}

fn classify(entry: &CompoundEntry) -> ContainerRole {
    let path = entry.path();
    if path.eq_ignore_ascii_case(SAB_STREAM) {
        return ContainerRole::AcisBinary;
    }
    if path.eq_ignore_ascii_case(STEP_STREAM) {
        return ContainerRole::RootExchange;
    }
    if path.eq_ignore_ascii_case(PROPERTIES_STREAM) {
        return ContainerRole::Properties;
    }
    if path.eq_ignore_ascii_case(VERSION_STREAM) {
        return ContainerRole::Metadata;
    }
    match entry {
        CompoundEntry::Storage(_) => ContainerRole::Storage,
        CompoundEntry::Stream(_) => ContainerRole::Stream,
    }
}

#[cfg(test)]
mod tests {
    use super::has_alibre_evidence;

    fn paths(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn a_part_has_both_body_streams() {
        assert!(has_alibre_evidence(&paths(&[
            "PROP_DATA",
            "SAB_DATA",
            "STEP_DATA"
        ])));
        assert!(has_alibre_evidence(&paths(&["sab_data", "step_data"])));
    }

    #[test]
    fn other_compound_files_are_not_claimed() {
        assert!(!has_alibre_evidence(&paths(&["STEP_DATA", "CONSTITUENTS"])));
        assert!(!has_alibre_evidence(&paths(&[
            "RSeStorage",
            "RSeStorage/RSeSegInfo"
        ])));
        assert!(!has_alibre_evidence(&paths(&["SAB_DATA"])));
    }
}
