// SPDX-License-Identifier: Apache-2.0
//! Indexed records whose primary headers the decoder retains.

use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayload};
use crate::records::feature::thread::{DesignThreadConstruction, DesignThreadForm};

/// A reference-bearing compact Thread trailer names an indexed record outside
/// the scope reference table. Its primary header is retained with the scope
/// references, so the trailer reference resolves.
#[test]
fn compact_thread_trailer_record_is_a_related_record() {
    let construction = DesignThreadConstruction {
        form: DesignThreadForm::Compact(Some(crate::records::identity::Located {
            value: std::num::NonZeroU32::new(2075).unwrap(),
            offset: 144,
        })),
        designation_offset: 38,
        designation: cadmpeg_core::text::NonBlankString::new("M3x0.5").unwrap(),
        nominal_size: crate::records::feature::thread::DesignThreadNominalSize::try_from(
            "3.0".to_owned(),
        )
        .unwrap(),
        profile: cadmpeg_core::text::NonBlankString::new("ISO Metric profile").unwrap(),
        pitch: cadmpeg_ir::scalar::PositiveReal::new(0.05).unwrap(),
        face_group_record_indices: vec![988],
        diameters: crate::records::feature::thread::DesignThreadDiameters::new(
            0.3086, 0.2529, 0.2725,
        )
        .unwrap(),
    };
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#100",
        DesignFeatureKind::Thread,
        987,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 200;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![988, 989]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
            draft.payload = DesignScopePayload::Thread(Some(construction));
        })
        .unwrap();
    let indices = super::super::scope_related_indices(&[scope]);
    let stream = "f3d:Design/BulkStream.dat".to_owned();
    assert_eq!(
        indices,
        vec![(stream.clone(), 988), (stream.clone(), 989), (stream, 2075)]
    );
}
