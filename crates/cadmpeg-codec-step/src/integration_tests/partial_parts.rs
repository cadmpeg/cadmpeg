// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::source_fidelity::parts::PartState;

use crate::{loss::StepLossCode, StepCodec};

#[test]
fn bounded_simple_entity_failure_uses_shared_home_and_preserves_later_neutral_geometry() {
    let defective = "#2=BROKEN(%,3);";
    let source = format!("ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT('',(1.,2.,3.));{defective}#3=CARTESIAN_POINT('later',(4.,5.,6.));#4=GEOMETRIC_SET('',(#1,#3));ENDSEC;END-ISO-10303-21;");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("independent points survive source damage");
    assert_eq!(
        decoded
            .ir()
            .model
            .points
            .iter()
            .map(|point| point.position().get())
            .collect::<Vec<_>>(),
        [Point3::new(1., 2., 3.), Point3::new(4., 5., 6.)]
    );
    let unknowns = decoded
        .ir()
        .native_unknowns("step")
        .expect("actual typed native stubs");
    let fidelity = decoded.source_fidelity();
    let homes = unknowns
        .iter()
        .filter(|stub| {
            fidelity
                .retained_record(stub.id.as_str())
                .and_then(|record| record.data())
                == Some(defective.as_bytes())
        })
        .collect::<Vec<_>>();
    assert_eq!(homes.len(), 1);
    let stub = homes[0];
    assert_eq!(fidelity.parts()[&stub.id].state(), &PartState::GivenUp {});
    assert!(fidelity.record_layout().contains_key(stub.id.as_str()));
    assert!(!unknowns
        .iter()
        .any(|record| record.id.as_str().starts_with("step:file:omitted#")));
    assert_eq!(
        decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        1
    );
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::ParseRecordOmitted.kind()));
}

#[test]
fn complex_and_cut_short_callbacks_preserve_actual_independent_point_geometry() {
    for (defective, continuation, point_count) in [
        ("#2=(A(1) A(2));", "#3=CARTESIAN_POINT('later',(4.,5.,6.));#5=GEOMETRIC_SET('',(#3));ENDSEC;END-ISO-10303-21;", 2),
        ("#2=ITEM(3 ", "ENDSEC;END-ISO-10303-21;", 1),
        ("#2=(A(1) ", "ENDSEC;END-ISO-10303-21;", 1),
        ("#2=ITEM('open;#3=CARTESIAN_POINT('',(9.,9.,9.));", "", 1),
        ("#2=(A(/* open;#3=CARTESIAN_POINT('',(9.,9.,9.));", "", 1),
    ] {
        let source = format!("ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT('',(1.,2.,3.));#4=GEOMETRIC_SET('',(#1));{defective}{continuation}");
        let decoded = StepCodec::default().decode(&mut Cursor::new(&source), &DecodeOptions::default()).expect("bounded malformed record preserves independent current geometry");
        let expected = [Point3::new(1., 2., 3.), Point3::new(4., 5., 6.)];
        assert_eq!(decoded.ir().model.points.iter().map(|point| point.position().get()).collect::<Vec<_>>(), expected[..point_count], "{defective}");
        let fidelity = decoded.source_fidelity();
        let unknowns = decoded.ir().native_unknowns("step").expect("actual native failure records");
        let homes = unknowns.iter().filter(|record| fidelity.retained_record(record.id.as_str()).and_then(|retained| retained.data()) == Some(defective.as_bytes())).collect::<Vec<_>>();
        assert_eq!(homes.len(), 1);
        let stub = homes[0];
        assert_eq!(fidelity.parts()[&stub.id].state(), &PartState::GivenUp {});
        assert!(fidelity.record_layout()[stub.id.as_str()].reads().is_empty());
        assert!(!unknowns.iter().any(|record| record.id.as_str().starts_with("step:file:omitted#")));
        assert_eq!(decoded.report().losses.iter().filter(|loss| loss.code.local_code() == "record.unreadable").count(), 1);
        assert!(!decoded.report().losses.iter().any(|loss| loss.code == StepLossCode::ParseRecordOmitted.kind()));
        assert_eq!(fidelity.metadata["bytes_unclassified"], "0");
    }
}
