// SPDX-License-Identifier: Apache-2.0
//! Notes shared by NX decode and inspection summaries.

use cadmpeg_core::bytes::assemble_u32_be;

use crate::decode::{self, Scan};
use crate::parasolid::StreamKind;

/// Classify a scan and build its inspection and decode notes.
pub(super) fn summarize(scan: &Scan) -> (crate::dialect::LayerClassification, Vec<String>) {
    let c = &scan.container;
    let (control_count, classified_control_count) = decode::offset_store_control_counts(c);
    let header_entry_count = c.entry_count(crate::container::Region::Header);
    let footer_entry_count = c.entry_count(crate::container::Region::Footer);
    let mut notes = match c.layout {
        crate::container::ContainerLayout::LegacyCfb { .. } => vec![format!(
            "legacy CFB container: {} directory entr{}",
            header_entry_count,
            if header_entry_count == 1 {
                "y"
            } else {
                "ies"
            },
        )],
        crate::container::ContainerLayout::Modern {
            file_tag,
            footer_offset,
            footer_fingerprint,
            ..
        } => vec![format!(
            "SPLMSSTR container: file tag {}, footer offset {}, {} HEADER and {} FOOTER directory entry/ies, fingerprint {:08x}",
            file_tag,
            footer_offset,
            header_entry_count,
            footer_entry_count,
            assemble_u32_be(footer_fingerprint),
        )],
    };
    notes.push(format!(
        "embedded streams: {} partition, {} deltas, {} plain (cached body), {} preview/non-Parasolid",
        scan.count(StreamKind::Partition),
        scan.count(StreamKind::Deltas),
        scan.count(StreamKind::Plain),
        scan.count(StreamKind::Preview),
    ));
    if control_count != 0 {
        notes.push(format!(
            "NX object model: {classified_control_count} of {control_count} bounded offset-store control block(s) have an admitted complete grammar"
        ));
    }
    let framed_om_sections = c.om_sections();
    if !framed_om_sections.is_empty() {
        let declarations = framed_om_sections
            .iter()
            .map(|(_, section)| section.types.len())
            .sum::<usize>();
        let fields = framed_om_sections
            .iter()
            .map(|(_, section)| section.fields.len())
            .sum::<usize>();
        notes.push(format!(
            "NX object model: {} size-framed section(s), {} class declaration(s), {} field declaration(s)",
            framed_om_sections.len(),
            declarations,
            fields
        ));
    }
    let om_sections = c.indexed_om_sections();
    if !om_sections.is_empty() {
        let entities = om_sections
            .iter()
            .filter_map(|(_, section)| section.as_fixed())
            .map(<[crate::om::FixedEntityRecord<'_>]>::len)
            .sum::<usize>();
        let blocks = om_sections
            .iter()
            .filter_map(|(_, section)| section.as_offset_only())
            .map(|(_control, _, records)| records.len() + 1)
            .sum::<usize>();
        if blocks == 0 {
            notes.push(format!(
                "NX object model: {} indexed section(s), {} bounded entity record(s)",
                om_sections.len(),
                entities
            ));
        } else {
            notes.push(format!(
                "NX object model: {} indexed section(s), {} ID-bounded entity record(s), {} offset-only data block(s)",
                om_sections.len(),
                entities,
                blocks
            ));
        }
    }
    if !scan.has_parasolid()
        && c.entries
            .iter()
            .any(|e| e.name.contains("ExternalReferences"))
    {
        notes.push(
            "no inline Parasolid geometry (assembly .prt: geometry in external child parts)"
                .to_string(),
        );
    }
    (crate::dialect::classify_layers(scan), notes)
}
