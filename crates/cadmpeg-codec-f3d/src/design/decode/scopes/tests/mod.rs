// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]

mod assembly;
mod assembly_variable_reference;
mod chamfer_sets;
mod combine;
mod component_pattern;
mod copy_paste_bodies;
mod derived_instance;
mod draft_flags;
mod extended_thread;
mod fillet_sets;
mod fixed_extrude_lanes;
mod fixed_kind_operations;
mod fixed_kind_tail;
mod fixed_scalar_frames;
mod flange;
mod history_admission;
mod legacy_class_397;
mod legacy_frames;
mod legacy_work_planes;
mod move_reference_forms;
mod move_transform;
mod named_delete_face;
mod named_empty_label;
mod named_variable_tail;
mod offset_faces;
mod point_direction_work_axes;
mod referenced_revolve;
mod remove_body_named_tail;
mod scale;
mod shell_wall_slots;
mod surface_offset_groups;
mod surfaces;
mod thicken;
mod thread;
mod to_object_revolve;
mod typed_references;
mod unstitch;

fn read_work_axis(
    bytes: &[u8],
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> crate::records::feature::work_axes::DesignWorkAxisConstruction {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, source) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let mut parts = Vec::new();
    let evidence = super::work_axes::decode_work_axis(
        &ctx,
        source,
        &crate::design::test_support::indexed_record_offsets_for_test(bytes),
        scope,
        "test/BulkStream.dat",
        super::work_axes::WorkAxisRegistrations {
            entities: &std::collections::HashMap::new(),
            classes: &std::collections::HashMap::new(),
        },
        &mut parts,
    )
    .unwrap();
    assert!(parts.is_empty());
    evidence
}

mod fixed_kind_path_operations;
mod fixed_kind_tail_operations;

fn move_transform_type(guid: &str, version: u32) -> crate::records::entity_header::SegmentType {
    crate::records::entity_header::SegmentType::try_new(
        "f3d:Design/MetaStream.dat:design-type#0".into(),
        crate::design::test_support::design_type(guid, None, version, "Fusion", vec![]),
    )
    .unwrap()
}

fn move_transform_types<'a>(
    row: &'a crate::records::entity_header::SegmentType,
    tags: &[u32],
) -> std::collections::HashMap<u32, &'a crate::records::entity_header::SegmentType> {
    tags.iter().map(|&tag| (tag, row)).collect()
}
