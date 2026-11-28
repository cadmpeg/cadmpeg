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
/// Stream holding the component references of an assembly.
pub(crate) const CONSTITUENTS_STREAM: &str = "CONSTITUENTS";
const PROPERTIES_STREAM: &str = "PROP_DATA";
const VERSION_STREAM: &str = "VERSION";

/// The kind of document a compound file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DocumentKind {
    /// A part: the `SAB_DATA` and `STEP_DATA` streams.
    Part,
    /// An assembly: the `CONSTITUENTS` and `STEP_DATA` streams and no `SAB_DATA`.
    Assembly,
}

fn has_stream(paths: &[String], name: &str) -> bool {
    paths.iter().any(|path| path.eq_ignore_ascii_case(name))
}

/// The document kind that directory paths carry the streams of.
pub(crate) fn document_kind(paths: &[String]) -> Option<DocumentKind> {
    if !has_stream(paths, STEP_STREAM) {
        return None;
    }
    if has_stream(paths, SAB_STREAM) {
        Some(DocumentKind::Part)
    } else if has_stream(paths, CONSTITUENTS_STREAM) {
        Some(DocumentKind::Assembly)
    } else {
        None
    }
}

/// Whether directory paths carry the streams of a part or of an assembly.
pub(crate) fn has_alibre_evidence(paths: &[String]) -> bool {
    document_kind(paths).is_some()
}

/// One parsed Alibre compound container.
pub(crate) struct AlibreContainer<'a> {
    snapshot: CompoundSnapshot<'a>,
    kind: DocumentKind,
}

impl<'a> AlibreContainer<'a> {
    pub(crate) fn open(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Self, CodecError> {
        let snapshot = CompoundSnapshot::new(ctx, root)?;
        let paths: Vec<String> = snapshot
            .entries()
            .iter()
            .map(|entry| entry.path().to_owned())
            .collect();
        let Some(kind) = document_kind(&paths) else {
            return Err(CodecError::WrongFormat(
                "not an Alibre document: the compound file lacks the SAB_DATA and STEP_DATA streams of a part and the CONSTITUENTS and STEP_DATA streams of an assembly"
                    .into(),
            ));
        };
        Ok(Self { snapshot, kind })
    }

    /// Whether the container holds a part or an assembly.
    pub(crate) fn kind(&self) -> DocumentKind {
        self.kind
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

    /// The document GUID that the `VERSION` stream states, in upper case: the
    /// first GUID after the first bar of the stream. `None` when the stream is
    /// absent or states none.
    pub(crate) fn document_guid(&self, ctx: &DecodeContext<'a>) -> Option<String> {
        let view = self.stream(ctx, VERSION_STREAM).ok()?;
        version_document_guid(view.window())
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

    /// The loss naming the streams of an assembly whose content the IR does
    /// not carry: every stream but `STEP_DATA`, which the assembly reader
    /// decodes.
    pub(crate) fn undecoded_assembly_streams_loss(&self) -> Option<LossNote> {
        let names: Vec<&str> = self
            .snapshot
            .entries()
            .iter()
            .filter_map(|entry| match entry {
                CompoundEntry::Stream(stream) => Some(stream.path()),
                CompoundEntry::Storage(_) => None,
            })
            .filter(|name| !name.eq_ignore_ascii_case(STEP_STREAM))
            .collect();
        (!names.is_empty()).then(|| {
            AlibreLossCode::SourceStreamsUndecoded.note(format!(
                "streams without a decoder: {}; the component references of CONSTITUENTS repeat those of STEP_DATA",
                names.join(", ")
            ))
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

/// The GUID after the first bar of the text of a `VERSION` stream.
fn version_document_guid(bytes: &[u8]) -> Option<String> {
    let bar = bytes.iter().position(|byte| *byte == b'|')?;
    let candidate = bytes.get(bar + 1..bar + 1 + 36)?;
    let shaped = candidate.iter().enumerate().all(|(at, byte)| match at {
        8 | 13 | 18 | 23 => *byte == b'-',
        _ => byte.is_ascii_hexdigit(),
    });
    shaped.then(|| {
        candidate
            .to_ascii_uppercase()
            .iter()
            .map(|b| char::from(*b))
            .collect()
    })
}

fn classify(entry: &CompoundEntry) -> ContainerRole {
    let path = entry.path();
    if path.eq_ignore_ascii_case(SAB_STREAM) {
        return ContainerRole::AcisBinary;
    }
    if path.eq_ignore_ascii_case(STEP_STREAM) {
        return ContainerRole::RootExchange;
    }
    if path.eq_ignore_ascii_case(CONSTITUENTS_STREAM) {
        return ContainerRole::ExternalReferences;
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
    use super::{has_alibre_evidence, version_document_guid};

    #[test]
    fn the_document_guid_follows_the_first_bar_of_the_version_text() {
        let text = b"\x00\xa5<Repository Guid=\"0\" Name=\"C\"/>:/p/asm_box|58f88938-2D66-43CA-BFF8-892C49CE60A9||1|1";
        assert_eq!(
            version_document_guid(text).as_deref(),
            Some("58F88938-2D66-43CA-BFF8-892C49CE60A9")
        );
        assert_eq!(version_document_guid(b"no bar here"), None);
        assert_eq!(
            version_document_guid(b"a|not-a-guid-not-a-guid-not-a-guid-xx"),
            None
        );
        assert_eq!(version_document_guid(b"a|58F88938-2D66"), None);
    }

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
    fn an_assembly_has_the_component_stream_and_no_body_stream() {
        assert!(has_alibre_evidence(&paths(&[
            "CONSTITUENTS",
            "PROP_DATA",
            "STEP_DATA"
        ])));
        assert_eq!(
            super::document_kind(&paths(&["constituents", "step_data"])),
            Some(super::DocumentKind::Assembly)
        );
        assert_eq!(
            super::document_kind(&paths(&["CONSTITUENTS", "SAB_DATA", "STEP_DATA"])),
            Some(super::DocumentKind::Part)
        );
    }

    #[test]
    fn other_compound_files_are_not_claimed() {
        assert!(!has_alibre_evidence(&paths(&["CONSTITUENTS", "PROP_DATA"])));
        assert!(!has_alibre_evidence(&paths(&["STEP_DATA", "PROP_DATA"])));
        assert!(!has_alibre_evidence(&paths(&[
            "RSeStorage",
            "RSeStorage/RSeSegInfo"
        ])));
        assert!(!has_alibre_evidence(&paths(&["SAB_DATA"])));
    }
}
