// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]

mod assembly;
mod assembly_variable_reference;
mod combine;
mod component_pattern;
mod copy_paste_bodies;
mod derived_instance;
mod fixed_kind_operations;
mod fixed_kind_tail;
mod flange;
mod history_admission;
mod legacy_class_397;
mod legacy_frames;
mod legacy_work_planes;
mod named_empty_label;
mod named_variable_tail;
mod scale;
mod surfaces;
mod thicken;
mod thread;

mod fixed_kind_path_operations;
mod fixed_kind_tail_operations;

/// A header that can open a parameter scope.
struct ScopeCandidateHeader {
    record_index: u32,
    class_tag: crate::records::references::DesignClassTag,
    byte_offset: u64,
}

/// Every header of `bytes` that can open a parameter scope: each header except
/// the last that carries its record index, in the decoder's frame order.
fn scope_candidate_headers(bytes: &[u8]) -> Vec<ScopeCandidateHeader> {
    let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut headers = Vec::new();
    for (record_index, offsets) in records.records(&ctx).unwrap() {
        for start in &offsets[..offsets.len().saturating_sub(1)] {
            let header =
                crate::design::decode::sketch::indexed_record_header_at(bytes, *start).unwrap();
            headers.push(ScopeCandidateHeader {
                record_index,
                class_tag: crate::records::references::DesignClassTag::try_from(
                    String::from_utf8(header.class_tag.to_vec()).unwrap(),
                )
                .unwrap(),
                byte_offset: cadmpeg_core::decode::u64_from_index(*start),
            });
        }
    }
    headers
}
