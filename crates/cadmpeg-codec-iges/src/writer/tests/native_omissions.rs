// SPDX-License-Identifier: Apache-2.0
//! The semantic writer lists every native record it leaves out by id (§3.4,
//! Rule W3).
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
use crate::{IgesCodec, IgesVersion};

fn entity(
    entity_type: i64,
    label: &str,
    status: &'static str,
    parameters: &str,
) -> OwnedTestEntity {
    OwnedTestEntity {
        entity_type,
        form: 0,
        label: label.into(),
        status,
        parameters: parameters.into(),
    }
}

/// The `native.records-omitted` messages of a semantic write of `file`.
fn omitted(file: &[u8]) -> Vec<String> {
    let decoded = IgesCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let plan = IgesCodec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit(IgesVersion::V5_3.descriptor().id.as_str()),
        )
        .unwrap();
    plan.report()
        .losses
        .iter()
        .filter(|loss| loss.code.to_string() == "export/native.records-omitted")
        .map(|loss| loss.message.clone())
        .collect()
}

#[test]
fn a_direction_no_written_surface_references_is_listed_in_both_native_views() {
    let messages = omitted(&owned_test_file(&[
        entity(116, "LOCATION", "00010000", "116,1,2,3,0;"),
        entity(123, "AXIS", "00010000", "123,0,0,1;"),
        entity(123, "STRAY", "00000000", "123,1,0,0;"),
        entity(190, "PLANE", "00000000", "190,1,3;"),
    ]));
    let reason = "that are directions no written analytic surface references, which the \
                  semantic writer does not regenerate";
    // The axis the plane references is regenerated; only the stray direction
    // is listed, in its `directions` record and its Type 123 entity.
    assert_eq!(
        messages
            .iter()
            .filter(|message| message.contains("arena `directions`")
                || message.contains("arena `entities`"))
            .collect::<Vec<_>>(),
        [
            &format!(
                "the IGES write omits 1 native record(s) of `iges` arena `directions` {reason}: \
                 iges:native:direction#D5"
            ),
            &format!(
                "the IGES write omits 1 native record(s) of `iges` arena `entities` {reason}: \
                 iges:entity:directory#5"
            ),
        ]
    );
}

#[test]
fn native_topology_without_neutral_faces_is_listed_by_id() {
    let messages = omitted(&owned_test_file(&[
        entity(116, "POINT", "00000000", "116,1,2,3,0;"),
        entity(502, "VERTICES", "00010001", "502,1,0,0,0;"),
    ]));
    assert!(
        messages.contains(
            &"the IGES write omits 1 native record(s) of `iges` arena `entities` that are \
              native topology, which has no neutral projection, so the semantic writer omits \
              it: iges:entity:directory#3"
                .to_owned()
        ),
        "{messages:#?}"
    );
}
