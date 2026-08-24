// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::Codec;

use super::{point_file_with_version_flag, valid_global_fields};
use crate::test_support::point_file_with_global;
use crate::IgesCodec;

#[test]
fn inspect_reports_the_resolution_losses_it_charges_as_census_notes() {
    let mut fields = valid_global_fields();
    fields[11] = "7Hproduct".into();
    fields[16] = String::new();
    fields[22] = "6".into();
    let mut global = fields.join(",");
    global.push(';');

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(point_file_with_global(global.as_bytes())),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();

    assert!(summary.notes.contains(&"iges_version=4.0".into()));
    assert!(summary
        .notes
        .contains(&"loss.iges/source.dialect-unverified=1".into()));
    assert!(summary
        .notes
        .contains(&"loss.iges/presentation.line-weight-scale-unavailable=1".into()));
}

#[test]
fn inspect_reports_the_declared_version_flag_only_when_the_clamp_changes_it() {
    for (flag, version) in [("12", "5.3"), ("0", "2.0")] {
        let summary = IgesCodec
            .inspect(
                &mut Cursor::new(point_file_with_version_flag(flag)),
                &cadmpeg_core::decode::InspectOptions::default(),
            )
            .unwrap();
        assert!(
            summary.notes.contains(&format!("iges_version={version}")),
            "{flag}: {:#?}",
            summary.notes
        );
        assert!(
            summary.notes.contains(&format!("iges_version_flag={flag}")),
            "{flag}: {:#?}",
            summary.notes
        );
    }

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(point_file_with_version_flag("11")),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
    assert!(summary.notes.contains(&"iges_version=5.3".into()));
    assert!(
        !summary
            .notes
            .iter()
            .any(|note| note.starts_with("iges_version_flag=")),
        "{:#?}",
        summary.notes
    );
}
