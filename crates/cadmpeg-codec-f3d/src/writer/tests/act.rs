// SPDX-License-Identifier: Apache-2.0
//! Writer-domain synthetic tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::native_test::{f3d_native_mut, update_f3d_native};
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_smbh_and_protein;
use crate::F3dCodec;

#[test]
fn generated_source_less_omits_act_without_segment_metadata_with_a_loss() {
    use crate::records::act::ActEntity;

    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let mut native = f3d_native_mut(&mut source_less);
    native.act_entities = vec![ActEntity::try_new(
        "f3d:generated:act-entity#7".into(),
        7,
        "0_985".into(),
        Some(crate::records::act::ActTableRow::new(0).unwrap()),
        crate::records::act::ActChannelGroup::try_new(
            100,
            Some(200),
            "261".to_owned().try_into().unwrap(),
            std::collections::BTreeMap::from([(
                "Appearance".into(),
                crate::records::identity::Located {
                    value: "11111111-2222-3333-4444-555555555555"
                        .to_owned()
                        .try_into()
                        .unwrap(),
                    offset: 120,
                },
            )]),
            None,
        )
        .unwrap(),
    )
    .unwrap()];
    drop(native);
    let (output, report) = super::source_less_analytic::synthesize_document(&source_less, None);
    super::source_less_analytic::assert_native_omission(&report);
    assert!(crate::test_support::native_test::f3d_native(output.ir())
        .act_entities
        .is_empty());
    assert_eq!(
        output.ir().model.bodies.len(),
        source_less.model.bodies.len()
    );
}

#[test]
fn generated_f3d_regenerates_neutral_content_when_act_binding_diverges() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated ACT decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut edited, |native| {
        native.act_entities[0]
            .set_channel_guid(
                "Appearance",
                String::from("dddddddd-1111-2222-3333-eeeeeeeeeeee")
                    .try_into()
                    .unwrap(),
            )
            .unwrap();
    });

    let (output, report) =
        super::source_less_analytic::synthesize_document(&edited, Some(&fidelity));
    super::source_less_analytic::assert_native_omission(&report);
    assert!(!crate::test_support::native_test::f3d_native(output.ir())
        .act_entities
        .is_empty());
    assert_eq!(output.ir().model.bodies.len(), edited.model.bodies.len());
    assert_eq!(
        output.ir().model.appearances[0].base_color,
        edited.model.appearances[0].base_color
    );
}

#[test]
fn generated_f3d_synthesizes_an_act_record_index_edit_without_metastream_edit() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated ACT decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut edited, |native| {
        native.act_component_links[0].record_index += 1;
    });

    let (output, report) =
        super::source_less_analytic::synthesize_document(&edited, Some(&fidelity));
    super::source_less_analytic::assert_native_omission(&report);
    assert!(!crate::test_support::native_test::f3d_native(output.ir())
        .act_entities
        .is_empty());
    assert_eq!(output.ir().model.bodies.len(), edited.model.bodies.len());
    assert_eq!(
        output.ir().model.appearances[0].base_color,
        edited.model.appearances[0].base_color
    );
}
