// SPDX-License-Identifier: Apache-2.0

use super::super::AttachSnapshot;
use cadmpeg_ir::document::CadIr;
use std::collections::BTreeSet;

fn unknown(id: &str) -> cadmpeg_ir::UnknownRecord {
    cadmpeg_ir::UnknownRecord::retained(
        cadmpeg_ir::ids::UnknownId::mint(id).expect("identity grammar"),
        0,
        vec![1],
        Vec::new(),
    )
}

#[test]
fn a_rolled_back_attachment_names_each_record_it_added_and_no_record_it_kept() {
    crate::test_support::with_decode_context(|ctx| {
        let mut ir = CadIr::empty();
        ir.native
            .namespace_mut(cadmpeg_ir::codec_id!("nx"))
            .set_arena(
                ctx,
                "kept",
                &[cadmpeg_core::json!({"id": "nx:native:kept#0"})],
            )
            .expect("the kept arena is admitted");
        let mut unknowns = vec![unknown("nx:container:kept#0")];
        let annotations = cadmpeg_ir::AnnotationBuilder::new();
        let snapshot = AttachSnapshot::capture(&mut ir, &annotations, &unknowns, &[]);

        ir.native
            .namespace_mut(cadmpeg_ir::codec_id!("nx"))
            .set_arena(
                ctx,
                "added",
                &[cadmpeg_core::json!({"id": "nx:native:added#0"})],
            )
            .expect("the added arena is admitted");
        unknowns.push(unknown("nx:container:added#0"));

        assert_eq!(
            snapshot.attached_records(&mut ir, &unknowns),
            BTreeSet::from([
                "nx:container:added#0".to_owned(),
                "nx:native:added#0".to_owned(),
            ])
        );
    });
}
