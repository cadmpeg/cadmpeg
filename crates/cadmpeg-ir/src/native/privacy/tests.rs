// SPDX-License-Identifier: Apache-2.0
use super::*;

#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Shape {
    Line { length: f64 },
    Circle { radius: f64 },
}

#[derive(serde::Deserialize)]
struct Record {
    id: String,
    name: String,
    shape: Shape,
}

#[test]
fn repro_native_privacy_classifies_internal_enum_and_raw_text() {
    let json =
        r#"{"id":"synthetic:s:record#1","name":"line","shape":{"kind":"line","length":3.0}}"#;
    let fields = collect(20, || {
        let record: Record = serde_json::from_str(json).unwrap();
        assert_eq!(record.id, "synthetic:s:record#1");
        assert_eq!(record.name, "line");
        match record.shape {
            Shape::Line { length } => assert_eq!(length, 3.0),
            Shape::Circle { radius } => assert_eq!(radius, 3.0),
        }
        observe::<Record>(&record.id, json);
        Ok(())
    })
    .unwrap();
    assert_eq!(
        fields["synthetic:s:record#1"],
        BTreeSet::from(["/shape/kind".into()])
    );
}

#[test]
fn repro_native_privacy_limits_work_and_restores_collector() {
    let json =
        r#"{"id":"synthetic:s:record#1","name":"secret","shape":{"kind":"line","length":3.0}}"#;
    assert!(collect(0, || {
        observe::<Record>("synthetic:s:record#1", json);
        Ok(())
    })
    .is_err());
    assert!(collect(20, || {
        observe::<Record>("synthetic:s:record#1", json);
        Ok(())
    })
    .is_ok());
    assert!(collect::<()>(20, || Err(CodecError::InvalidInput("refusal".into()))).is_err());
    assert!(collect(20, || {
        observe::<Record>("synthetic:s:record#1", json);
        Ok(())
    })
    .is_ok());
}

#[derive(serde::Deserialize)]
#[serde(try_from = "SemanticWire")]
struct SemanticRecord;

#[derive(serde::Deserialize)]
struct SemanticWire {
    source_kind: String,
    kind: String,
    name: String,
}

impl TryFrom<SemanticWire> for SemanticRecord {
    type Error = &'static str;

    fn try_from(wire: SemanticWire) -> Result<Self, Self::Error> {
        if wire.source_kind != "Dimension" || wire.kind != "dimension" {
            return Err("kind disagrees with source_kind");
        }
        if wire.name.is_empty() {
            return Err("name is empty");
        }
        Ok(Self)
    }
}

#[test]
fn repro_native_privacy_preserves_custom_admission_tokens() {
    let json = r#"{"source_kind":"Dimension","kind":"dimension","name":"private-member.f3d"}"#;
    let fields = collect(20, || {
        observe::<SemanticRecord>("synthetic:s:record#1", json);
        Ok(())
    })
    .unwrap();
    assert_eq!(
        fields["synthetic:s:record#1"],
        BTreeSet::from(["/source_kind".into(), "/kind".into()])
    );
}
