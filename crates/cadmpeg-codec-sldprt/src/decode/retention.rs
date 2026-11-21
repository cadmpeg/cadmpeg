// SPDX-License-Identifier: Apache-2.0
//! Decode-local record of the source records kept without decoding.
//!
//! Marks are noted where a route keeps a record's content out of every typed
//! arena, then applied to the records the decode retained.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::ArenaClass;
use cadmpeg_core::CodecError;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{Annotations, SourceFidelity};

#[derive(Default)]
pub(super) struct Retention {
    records: BTreeMap<String, BTreeSet<ArenaClass>>,
}

impl Retention {
    pub(super) fn record(&mut self, id: &str, class: ArenaClass) {
        self.records.entry(id.to_owned()).or_default().insert(class);
    }

    /// Mark every record no parser identified the content family of.
    pub(super) fn opaque_records(&mut self, records: &[UnknownRecord], annotations: &Annotations) {
        for record in records {
            if self.records.contains_key(record.id().as_str()) {
                continue;
            }
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
            // Either class may occur in a payload of unknown family.
            self.record(record.id().as_str(), ArenaClass::Model);
            self.record(record.id().as_str(), ArenaClass::Design);
        }
    }

    pub(super) fn apply(self, fidelity: &mut SourceFidelity) -> Result<(), CodecError> {
        for (id, classes) in self.records {
            if fidelity.retained_records().contains_key(id.as_str()) {
                fidelity.mark_untyped(&id, classes)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Retention;
    use cadmpeg_core::decode::ArenaClass;
    use cadmpeg_ir::hash::digest::Sha256Digest;
    use cadmpeg_ir::ids::UnknownId;
    use cadmpeg_ir::{Annotations, RetainedSourceRecord, SourceFidelity};

    #[test]
    fn marks_go_only_to_records_the_decode_retained() {
        let mut fidelity = SourceFidelity::with_annotations(
            Annotations::default(),
            Sha256Digest::digest(b"sldprt retention test"),
        );
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
        retention.record("sldprt:file:block#29", ArenaClass::Model);
        retention.apply(&mut fidelity).unwrap();
        assert_eq!(
            fidelity.retained_records()["sldprt:file:block#17"]
                .untyped()
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            [ArenaClass::Design]
        );
        assert!(fidelity.retained_records()["sldprt:file:source-image#0"]
            .untyped()
            .is_empty());
        assert_eq!(
            fidelity.unrepresented().iter().copied().collect::<Vec<_>>(),
            [ArenaClass::Design]
        );
    }
}
