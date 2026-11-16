use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::schema::Schema;
use super::{sweep_with, SchemaSweepReport};

/// A positive number: a check the schema does not state.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(try_from = "f64")]
struct Positive(f64);

impl TryFrom<f64> for Positive {
    type Error = &'static str;
    fn try_from(value: f64) -> Result<Self, Self::Error> {
        (value > 0.0)
            .then_some(Self(value))
            .ok_or("must be positive")
    }
}

mod strict {
    use super::Positive;
    use serde::{Deserialize, Serialize};

    cadmpeg_core::named_optional_field!(deserialize_label, String, "label");

    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Root {
        thing: Thing,
    }

    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Thing {
        items: Vec<u32>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_label"
        )]
        label: Option<String>,
        shape: Shape,
    }

    #[derive(Serialize, Deserialize)]
    #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
    enum Shape {
        Circle { radius: Positive },
        Square { side: Positive },
    }
}

mod lax {
    use super::Positive;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Root {
        thing: Thing,
    }

    #[derive(Serialize, Deserialize)]
    struct Thing {
        #[serde(default)]
        items: Vec<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        shape: Shape,
    }

    #[derive(Serialize, Deserialize)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    enum Shape {
        Circle { radius: Positive },
        Square { side: Positive },
    }
}

fn schema(extra_shapes: &[Value], extra_defs: &[(&str, Value)]) -> Value {
    let variant = |kind: &str, member: &str, value: Value| {
        json!({
            "type": "object",
            "properties": {"kind": {"type": "string", "const": kind}, member: value},
            "additionalProperties": false,
            "required": ["kind", member],
        })
    };
    let mut shapes = vec![
        variant("circle", "radius", json!({"type": "number"})),
        variant("square", "side", json!({"type": "number"})),
    ];
    shapes.extend(extra_shapes.iter().cloned());
    let mut defs = json!({
        "Thing": {
            "type": "object",
            "properties": {
                "items": {"type": "array", "items": {"type": "integer", "minimum": 0}},
                "label": {"type": ["string", "null"], "default": null},
                "shape": {"$ref": "#/$defs/Shape"},
            },
            "additionalProperties": false,
            "required": ["items", "shape"],
        },
        "Shape": {"oneOf": shapes},
    });
    for (name, def) in extra_defs {
        defs[*name] = def.clone();
    }
    json!({
        "type": "object",
        "properties": {"thing": {"$ref": "#/$defs/Thing"}},
        "required": ["thing"],
        "$defs": defs,
    })
}

/// One document that reaches the circle and leaves the square to the build.
fn circle() -> Value {
    json!({"thing": {"items": [], "shape": {"kind": "circle", "radius": 1.0}}})
}

fn sweep<T: Serialize + for<'de> Deserialize<'de>>(schema: &Value) -> SchemaSweepReport {
    let read = |text: &str| serde_json::to_value(serde_json::from_str::<T>(text)?);
    sweep_with(schema, &[circle()], &circle(), &read)
}

#[test]
fn a_strict_reader_admits_one_spelling_at_every_position() {
    let report = sweep::<strict::Root>(&schema(&[], &[]));
    assert!(report.unreached.is_empty(), "{:?}", report.unreached);
    assert!(report.accepting.is_empty(), "{:?}", report.accepting);
    assert_eq!(report.definitions, 2);
    assert_eq!(report.definitions_reached, 2);
    assert_eq!(report.positions, 4);
    assert_eq!(report.built, 1);
}

#[test]
fn a_lax_reader_is_named_at_each_position_that_accepts() {
    let report = sweep::<lax::Root>(&schema(&[], &[]));
    assert!(report.unreached.is_empty(), "{:?}", report.unreached);
    let accepting = report
        .accepting
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        accepting,
        [
            "#/$defs/Shape/oneOf/0: the undeclared key `zz_bogus` is accepted",
            "#/$defs/Shape/oneOf/1: the undeclared key `zz_bogus` is accepted",
            "#/$defs/Thing: `items` left out",
            "#/$defs/Thing: `label` written null",
            "#/$defs/Thing: the undeclared key `zz_bogus` is accepted",
        ]
    );
}

#[test]
fn a_target_no_reader_accepts_is_reported_unreached() {
    let hexagon = json!({
        "type": "object",
        "properties": {"kind": {"type": "string", "const": "hexagon"}},
        "additionalProperties": false,
        "required": ["kind"],
    });
    let report = sweep::<strict::Root>(&schema(&[hexagon], &[]));
    assert_eq!(report.unreached, ["/$defs/Shape/oneOf/2"]);
}

#[test]
fn a_variant_that_holds_an_empty_type_has_no_target() {
    let never = json!({
        "type": "object",
        "properties": {
            "kind": {"type": "string", "const": "never"},
            "value": {"$ref": "#/$defs/Never"},
        },
        "additionalProperties": false,
        "required": ["kind", "value"],
    });
    let report = sweep::<strict::Root>(&schema(&[never], &[("Never", json!({"not": {}}))]));
    assert!(report.unreached.is_empty(), "{:?}", report.unreached);
    assert_eq!(report.uninhabited, ["/$defs/Never", "/$defs/Shape/oneOf/2"]);
}

#[test]
fn the_end_of_text_lookahead_matches_as_the_end_of_text() {
    let root = json!({});
    let schema = Schema::new(&root);
    let identity = r"^[^:#\s]+:[^:#\s]+#[^#\s]+(?![\s\S])";
    assert!(schema.pattern_matches(identity, "a:b#c"));
    assert!(!schema.pattern_matches(identity, "a:b#c d"));
    assert!(!schema.pattern_matches(identity, "a:b"));
}
