// SPDX-License-Identifier: Apache-2.0
//! Decode-local ownership of source records whose content was not projected.
//!
//! These flags are recorded at projection cuts. Loss-code classification is an
//! independent audit and never supplies this production state.

use std::collections::BTreeMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::{ArenaClass, ArenaClasses};
use cadmpeg_ir::{Annotations, SourceFidelity};

#[derive(Default)]
pub(super) struct Retention {
    records: BTreeMap<String, ArenaClasses>,
    source: ArenaClasses,
    pub(super) pmi_pointers: BTreeMap<String, BTreeMap<String, u64>>,
}

impl Retention {
    pub(super) fn install_layouts(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        fidelity: &mut SourceFidelity,
    ) -> Result<(), CodecError> {
        crate::pmi::layout::install(ctx, std::mem::take(&mut self.pmi_pointers), fidelity)
    }

    pub(super) fn source(&mut self, class: ArenaClass) {
        self.source.insert(class);
    }

    pub(super) fn record(&mut self, id: &str, class: ArenaClass) {
        self.records.entry(id.to_owned()).or_default().insert(class);
    }

    /// Localize opaque source framing before its bytes leave the decoder.
    pub(super) fn opaque_records(
        &mut self,
        records: &[cadmpeg_ir::unknown::UnknownRecord],
        annotations: &Annotations,
    ) -> Vec<String> {
        let mut opaque = Vec::new();
        for record in records {
            let provenance = annotations.provenance.get(record.id().as_str());
            let stream = provenance.map(|provenance| provenance.stream().to_ascii_lowercase());
            let typed_stream = stream.as_deref().is_some_and(|stream| {
                [
                    "keywords",
                    "resolvedfeatures",
                    "swobjects",
                    "displaylists",
                    "units",
                    "pmisemanticdatadb",
                    "solidworks",
                ]
                .iter()
                .any(|family| stream.contains(family))
            });
            let tag = provenance.and_then(|provenance| provenance.tag.as_deref());
            let typed_payload = matches!(
                tag,
                Some(
                    "parasolid"
                        | "parasolid_stream"
                        | "ole2"
                        | "png-preview"
                        | "bmp-thumbnail"
                        | "tessellation"
                        | "sw-objects"
                        | "displaylist_tessellation"
                )
            );
            if typed_stream || typed_payload {
                continue;
            }
            // No parser identified this payload's content family. Either class
            // may occur there; recording both prevents treating framing as a
            // typed representation of its payload.
            self.record(record.id().as_str(), ArenaClass::Model);
            self.record(record.id().as_str(), ArenaClass::Design);
            opaque.push(record.id().as_str().to_owned());
        }
        opaque
    }

    pub(super) fn apply(mut self, fidelity: &mut SourceFidelity) -> Result<(), CodecError> {
        for (id, classes) in self.records {
            if fidelity.retained_records().contains_key(id.as_str()) {
                fidelity.mark_untyped(&id, classes)?;
            } else {
                // Metadata routes can retain only the enclosing source image.
                self.source.extend(classes);
            }
        }
        if !self.source.is_empty() {
            fidelity.mark_untyped("sldprt:file:source-image#0", self.source)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Retention;
    use cadmpeg_ir::document::ArenaClass;
    use cadmpeg_ir::ids::UnknownId;
    use cadmpeg_ir::{RetainedSourceRecord, SourceFidelity};

    #[test]
    fn untyped_flags_follow_the_owned_record_and_union_across_sidecar_roundtrip() {
        let mut fidelity = SourceFidelity::default();
        for id in ["sldprt:file:source-image#0", "sldprt:file:block#17"] {
            fidelity
                .insert_retained_record(
                    UnknownId::mint(id).unwrap(),
                    RetainedSourceRecord::whole("source", vec![1, 2, 3]),
                )
                .unwrap();
        }
        let mut retention = Retention::default();
        retention.record("sldprt:file:block#17", ArenaClass::Design);
        // A record not retained separately belongs to the enclosing source.
        retention.record("sldprt:file:block#29", ArenaClass::Model);
        retention.apply(&mut fidelity).unwrap();
        assert!(fidelity.retained_records()["sldprt:file:block#17"]
            .untyped()
            .contains(&ArenaClass::Design));
        assert!(fidelity.retained_records()["sldprt:file:source-image#0"]
            .untyped()
            .contains(&ArenaClass::Model));
        let encoded = serde_json::to_value(&fidelity).unwrap();
        let mut parsed: SourceFidelity = serde_json::from_value(encoded).unwrap();
        assert_eq!(parsed.unrepresented(), fidelity.unrepresented());
        parsed.remove_retained_record("sldprt:file:block#17");
        assert!(!parsed.unrepresented().contains(&ArenaClass::Design));
        assert!(parsed.unrepresented().contains(&ArenaClass::Model));
    }
}
