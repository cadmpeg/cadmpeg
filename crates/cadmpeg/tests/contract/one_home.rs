// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::ArenaClass;

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
    let unreadable = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.to_string() == "decode/record.unreadable")
        .collect::<Vec<_>>();
    assert_eq!(unreadable.len(), 1, "{:?}", decoded.report().losses);
    assert_eq!(unreadable[0].severity, Severity::Error);
    assert!(unreadable[0]
        .message
        .starts_with("part step.simple_entity_instance "));
    let broken: &[u8] = b"#1=BROKEN(,);";
    let retained = decoded
        .source_fidelity()
        .retained_records()
        .values()
        .filter(|record| record.data() == Some(broken))
        .collect::<Vec<_>>();
    assert_eq!(retained.len(), 1);
    let offset = bytes
        .windows(broken.len())
        .position(|window| window == broken)
        .expect("the broken record occurs in the source");
    assert_eq!(
        retained[0].offset(),
        u64::try_from(offset).expect("the source offset fits u64")
    );
    assert!(retained[0].untyped().contains(&ArenaClass::Model));
}
