// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::CodecFormat;
use cadmpeg_test_support::contract::parts::check_one_home;

#[test]
fn step_malformed_terminated_statement_has_one_home_and_keeps_the_real_peer() {
    let bytes = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('one-home recovery'),'2;1');FILE_NAME('one-home','2026-10-22T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=BROKEN(,);#2=CARTESIAN_POINT('peer',(4.,5.,6.));#3=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#4=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));#5=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#3,#4)) REPRESENTATION_CONTEXT('peer','3D'));#6=SHAPE_REPRESENTATION('point',(#2),#5);ENDSEC;END-ISO-10303-21;";
    let decoded = cadmpeg_codec_step::StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("independently terminated bad statement is local");
    assert_eq!(decoded.ir().model.points.len(), 1);
    assert_eq!(decoded.ir().model.points[0].position().get().x, 4.0);
    assert_eq!(decoded.ir().model.points[0].position().get().y, 5.0);
    assert_eq!(decoded.ir().model.points[0].position().get().z, 6.0);
    let given_up = decoded
        .source_fidelity()
        .parts()
        .values()
        .filter(|part| {
            matches!(
                part.state(),
                cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(given_up.len(), 1);
    assert_eq!(given_up[0].level().as_str(), "step.simple_entity_instance");
    check_one_home(
        CodecFormat::Step,
        &decoded.ir().native,
        decoded.source_fidelity(),
        &decoded.report().losses,
        "file",
        |stream, range| {
            if stream != "file" {
                return None;
            }
            bytes.get(usize::try_from(range.start).ok()?..usize::try_from(range.end).ok()?)
        },
    )
    .expect("exact helper home, retained image, layouts and unique loss");
}
