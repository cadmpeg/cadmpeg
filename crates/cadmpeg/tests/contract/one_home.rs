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

// The §4.5 one-home contract over every damaged input (#215 rule 1): every
// part a `record.unreadable` loss names has exactly one `unknowns` stub in
// the part-id family whose retained bytes equal the part extent, and no
// other loss names a span inside it. The stub identity formula itself is
// covered by the part helper's own unit tests; this module checks the
// triangle loss span, stub and retained bytes without recomputing the digest.
mod damaged_one_home {
    const PART_FAMILY: &str = ":source:part#";

    fn same_stream(record_stream: &str, span_stream: Option<&str>) -> bool {
        record_stream == span_stream.unwrap_or("")
    }

    fn check_format(format: &str) {
        let inputs = super::super::support::damaged_fixtures(format);
        let mut findings = Vec::new();
        if inputs.is_empty() {
            findings.push(format!(
                "{format}: no damaged input covers the one-home rule"
            ));
        }
        for fixture in inputs.iter().filter(|fixture| fixture.damaged) {
            let decoded = match super::super::support::decode(format, &fixture.bytes) {
                Ok(decoded) => decoded,
                Err(error) => {
                    findings.push(format!(
                        "{format}: {} refuses its damaged input: {error}",
                        fixture.path.display()
                    ));
                    continue;
                }
            };
            let document = super::super::support::document(&decoded);
            let mut stubs = Vec::new();
            if let Some(native) = document
                .get("native")
                .and_then(serde_json::Value::as_object)
            {
                for namespace in native.values() {
                    if let Some(rows) = namespace
                        .get("unknowns")
                        .and_then(serde_json::Value::as_array)
                    {
                        for row in rows {
                            if let Some(id) = row.get("id").and_then(serde_json::Value::as_str) {
                                if id.contains(PART_FAMILY) {
                                    stubs.push(id.to_owned());
                                }
                            }
                        }
                    }
                }
            }
            let unreadable: Vec<_> = decoded
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code.local_code() == super::super::support::UNREADABLE)
                .collect();
            if unreadable.len() != stubs.len() {
                findings.push(format!(
                    "{format}: {} names {} given-up part(s) but holds {} part stub(s)",
                    fixture.path.display(),
                    unreadable.len(),
                    stubs.len()
                ));
            }
            let unique: std::collections::BTreeSet<&str> =
                stubs.iter().map(String::as_str).collect();
            if unique.len() != stubs.len() {
                findings.push(format!(
                    "{format}: {} holds duplicate part stubs",
                    fixture.path.display()
                ));
            }
            for loss in &unreadable {
                let Some(span) = loss.provenance.as_ref() else {
                    continue;
                };
                let length = span.byte_len().unwrap_or(0);
                let partner =
                    decoded
                        .source_fidelity()
                        .retained_records()
                        .iter()
                        .find(|(_, record)| {
                            same_stream(record.stream(), span.stream_name())
                                && record.offset() == span.offset
                                && record.byte_len() == length
                        });
                let Some((id, retained)) = partner else {
                    findings.push(format!(
                        "{format}: {}: no retained record matches the part at offset {}",
                        fixture.path.display(),
                        span.offset
                    ));
                    continue;
                };
                if !id.as_str().contains(PART_FAMILY)
                    || !stubs.iter().any(|stub| stub == id.as_str())
                {
                    findings.push(format!(
                        "{format}: {}: the retained record at offset {} is not the part stub",
                        fixture.path.display(),
                        span.offset
                    ));
                }
                if span.stream_name().is_none() {
                    let end = span.offset.saturating_add(length) as usize;
                    let start = span.offset as usize;
                    if end <= fixture.bytes.len() {
                        match retained.data() {
                            Some(data) if data == &fixture.bytes[start..end] => {}
                            _ => findings.push(format!(
                                "{format}: {}: retained bytes differ from the part extent at offset {}",
                                fixture.path.display(),
                                span.offset
                            )),
                        }
                    }
                }
                for other in &decoded.report().losses {
                    if other.code.local_code() == super::super::support::UNREADABLE {
                        continue;
                    }
                    if let Some(provenance) = other.provenance.as_ref() {
                        if same_stream(provenance.stream_name().unwrap_or(""), span.stream_name())
                            && provenance.byte_len().is_some_and(|other_length| {
                                provenance.offset < span.offset + length
                                    && span.offset < provenance.offset + other_length
                            })
                        {
                            findings.push(format!(
                                "{format}: {}: {} names a span inside the given-up part at offset {}",
                                fixture.path.display(),
                                other.code.local_code(),
                                span.offset
                            ));
                        }
                    }
                }
            }
        }
        super::super::support::assert_no_findings(&findings);
    }

    macro_rules! damaged_one_home {
        ($($module:ident => $format:literal, $test:ident);* $(;)?) => {
            $(#[cfg(feature = $format)]
            mod $module {
                #[test]
                fn $test() {
                    super::check_format($format);
                }
            })*
        };
    }

    damaged_one_home! {
        iges_damaged_one_home => "iges", iges_damaged_parts_have_one_home;
        step_damaged_one_home => "step", step_damaged_parts_have_one_home;
        freecad_damaged_one_home => "fcstd", freecad_damaged_parts_have_one_home;
    }
}
