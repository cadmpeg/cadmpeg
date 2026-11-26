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

/// The messages of `messages` about arena `arena`.
fn of_arena<'a>(messages: &'a [String], arena: &str) -> Vec<&'a str> {
    messages
        .iter()
        .filter(|message| message.contains(&format!("arena `{arena}`")))
        .map(String::as_str)
        .collect()
}

/// Each analytic surface on no face, with its location at D1, its axis at D3
/// and its reference direction at D5, as its Form 1 Parameter Data names them.
#[test]
fn an_analytic_surface_on_no_face_is_listed_with_its_axis_and_location() {
    for (entity_type, parameters) in [
        (192, "192,1,3,2,5;"),
        (194, "194,1,3,2,30,5;"),
        (196, "196,1,2,3,5;"),
        (198, "198,1,3,4,1,5;"),
    ] {
        let messages = omitted(&owned_test_file(&[
            entity(116, "LOCATION", "00010000", "116,1,2,3,0;"),
            entity(123, "AXIS", "00010000", "123,0,0,1;"),
            entity(123, "REFDIR", "00010000", "123,1,0,0;"),
            OwnedTestEntity {
                form: 1,
                ..entity(entity_type, "SURFACE", "00000000", parameters)
            },
        ]));
        // The writer withholds Types 192 to 198 on no face, so the location
        // and both directions leave the file with the surface.
        let directions = "that are directions no written analytic surface references, which \
                          the semantic writer does not regenerate";
        let location = "that are points that locate only analytic surfaces the semantic \
                        writer withholds";
        let surface = "whose neutral surface the semantic writer withholds";
        assert_eq!(
            of_arena(&messages, "directions"),
            [format!(
                "the IGES write omits 2 native record(s) of `iges` arena `directions` \
                 {directions}: iges:native:direction#D3, iges:native:direction#D5"
            )],
            "Type {entity_type}"
        );
        assert_eq!(
            of_arena(&messages, "entities"),
            [format!(
                "the IGES write omits 4 native record(s) of `iges` arena `entities`: \
                 iges:entity:directory#1, iges:entity:directory#3, iges:entity:directory#5, \
                 iges:entity:directory#7; {directions}: iges:entity:directory#3, \
                 iges:entity:directory#5; {location}: iges:entity:directory#1; {surface}: \
                 iges:entity:directory#7"
            )],
            "Type {entity_type}"
        );
    }
}

#[test]
fn a_witness_line_is_listed_in_both_native_views() {
    let messages = omitted(&owned_test_file(&[OwnedTestEntity {
        form: 40,
        ..entity(106, "COPIOUS", "00000100", "106,1,3,0,0,0,1,0,2,0;")
    }]));
    let reason = "whose entity has no neutral geometry for the semantic writer to encode";
    assert_eq!(
        of_arena(&messages, "copious_data"),
        [format!(
            "the IGES write omits 1 native record(s) of `iges` arena `copious_data` {reason}: \
             iges:native:copious-data#D1"
        )]
    );
    assert_eq!(
        of_arena(&messages, "entities"),
        [format!(
            "the IGES write omits 1 native record(s) of `iges` arena `entities` {reason}: \
             iges:entity:directory#1"
        )]
    );
}

#[test]
fn a_transformation_no_written_record_uses_is_listed_in_both_native_views() {
    let messages = omitted(&owned_test_file(&[
        entity(124, "MOVE", "00000000", "124,1,0,0,10,0,1,0,0,0,0,1,0;"),
        entity(116, "POINT", "00000000", "116,1,2,3,0;"),
    ]));
    let reason = "that are transformations no written record uses, which the semantic writer \
                  does not regenerate";
    assert_eq!(
        of_arena(&messages, "transformations"),
        [format!(
            "the IGES write omits 1 native record(s) of `iges` arena `transformations` \
             {reason}: iges:native:transformation#D1"
        )]
    );
    assert_eq!(
        of_arena(&messages, "entities"),
        [format!(
            "the IGES write omits 1 native record(s) of `iges` arena `entities` {reason}: \
             iges:entity:directory#1"
        )]
    );
}
