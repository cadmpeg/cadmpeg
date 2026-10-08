// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn circular_entity_selection_stops_after_second_match() {
    let definition = blank_definition();
    let table = FeatureEntityTable::new(42, 0, [204, 203, 200, 200].map(|class_id| FeatureEntityTableEntry {
        entity_id: 1,
        payload: crate::feature::entity::entry_payload(class_id, Some(2), None, None),
        prefixed: false,
        end_offset: 1,
        offset: 0,
    }).to_vec(), &Default::default(), 0);
    let short = [table.clone(), table.clone()];
    let mut long = short.to_vec();
    long.extend(std::iter::repeat_n(table, 64));
    let rows = crate::surface::unique_rows::UniqueIdRows::from_rows(Vec::new());
    let sources = PlacementSources {
        datums: &[], surface_rows: &rows, model_planes: &[], outline_planes: &[],
        plane_envelopes: &[], surface_parameters: &[], geometry_tables: &[], affected_ids: &[],
    };
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>, tables: &[FeatureEntityTable]| {
        super::super::circular_profile_aligned_origin(ctx, &definition, 42,
            super::super::SectionPlaneAxes {
                plane: SignedPlaneEquation { normal: [0.0, 0.0, 1.0], offset: 0.0 },
                u_axis: [1.0, 0.0, 0.0], v_axis: [0.0, 1.0, 0.0],
            }, &sources, tables)
    };
    crate::test_support::assert_work_boundaries(
        &["creo circular profile entity table selection"], |ctx| run(ctx, &short));
    let refusal = |tables: &[FeatureEntityTable]| crate::test_support::last_refusal_at(
        b"", cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo circular profile entity table selection", |ctx| run(ctx, tables));
    let cadmpeg_core::CodecError::ResourceLimit(short_refusal) = refusal(&short) else { panic!("work refusal") };
    let cadmpeg_core::CodecError::ResourceLimit(long_refusal) = refusal(&long) else { panic!("work refusal") };
    assert_eq!(short_refusal, long_refusal);
    assert_eq!(crate::decode::with_test_decode_ctx(|ctx| run(ctx, &long)).expect("ambiguous selection"), None);
}

#[test]
fn local_frame_selection_stops_after_second_complete_match() {
    let frame = FeatureParameterFrame {
        kind: FeatureParameterFrameKind::LocalSystem,
        body: Vec::new(),
        decoded_values: Some(finite_frame([0.0; 12])),
        offset: 1,
    };
    let mut short = blank_definition();
    short.parameter_frames = vec![frame.clone(), frame.clone()];
    let mut long = short.clone();
    long.parameter_frames.extend(std::iter::repeat_n(frame, 64));
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>, definition: &FeatureDefinition| {
        super::super::unique_complete_local_system(ctx, definition)
    };
    crate::test_support::assert_work_boundaries(
        &["creo complete local frame selection"], |ctx| run(ctx, &short));
    let refusal = |definition: &FeatureDefinition| crate::test_support::last_refusal_at(
        b"", cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo complete local frame selection", |ctx| run(ctx, definition));
    let cadmpeg_core::CodecError::ResourceLimit(short_refusal) = refusal(&short) else { panic!("work refusal") };
    let cadmpeg_core::CodecError::ResourceLimit(long_refusal) = refusal(&long) else { panic!("work refusal") };
    assert_eq!(short_refusal, long_refusal);
    assert_eq!(crate::decode::with_test_decode_ctx(|ctx| run(ctx, &long)).expect("ambiguous frame"), None);
}
