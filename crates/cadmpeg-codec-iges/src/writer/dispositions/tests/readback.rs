//! The facts a decode states, compared without record ids.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use cadmpeg_ir::codec::write::NativeDispositions;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::CadIr;
use serde_json::Value;

use crate::global::SourceField;
use crate::{IgesCodec, IgesVersion};

/// The geometry arenas whose records are facts an entity states.
const FACT_ARENAS: &[&str] = &[
    "points", "curves", "surfaces", "pcurves", "vertices", "edges", "coedges", "loops", "faces",
    "shells",
];

/// The geometry arenas a fact states through the records it names.
const NAMED_ARENAS: &[&str] = &[
    "points",
    "curves",
    "surfaces",
    "pcurves",
    "vertices",
    "edges",
    "coedges",
    "loops",
    "faces",
    "procedural_curves",
    "procedural_surfaces",
];

/// The relative tolerance of a number that a write and a decode restate.
const NUMBER_TOLERANCE: f64 = 1e-9;

/// The fields of a geometry record that state a native arena of their own,
/// with that arena.
pub(super) const ARENA_FIELDS: &[(&str, &str)] = &[("color", "display_attributes")];

/// The name a write's loss gives each Global text field, by field number.
const TEXT_FIELDS: &[(usize, &str)] = &[
    (3, "sending product id"),
    (4, "file name"),
    (12, "receiving product id"),
    (18, "file generation date"),
    (21, "author"),
    (22, "organization"),
    (25, "model modification date"),
    (26, "application protocol"),
];

/// A source decode, what its write at IGES 5.3 declares, and the decode of the
/// written file.
pub(super) struct Checked {
    pub(super) source: CadIr,
    pub(super) native: NativeDispositions,
    pub(super) output: CadIr,
    pub(super) output_global: Option<GlobalFacts>,
    /// The message of each loss the write charges.
    pub(super) losses: Vec<String>,
}

fn decode(file: &[u8]) -> Option<CadIr> {
    IgesCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .ok()
        .map(|decoded| decoded.ir().clone())
}

/// Decodes `file`, writes it, and decodes the written file. The error names
/// the step that refuses the source.
pub(super) fn write_and_read_back(file: &[u8]) -> Result<Checked, &'static str> {
    let source = decode(file).ok_or("decode")?;
    source.native.namespace("iges").ok_or("native namespace")?;
    let recovered = cadmpeg_ir::codec::write::edge_recovery::recover_edge_curves(&source)
        .map_err(|_| "edge recovery")?;
    let (synthesis, native) =
        crate::writer::target::synthesis_and_dispositions(&recovered.document, IgesVersion::V5_3)
            .map_err(|_| "write")?;
    let output = decode(&synthesis.bytes).expect("the written file decodes");
    let output_global = GlobalFacts::of(&output);
    Ok(Checked {
        source,
        native,
        output,
        output_global,
        losses: synthesis
            .losses
            .into_iter()
            .map(|loss| loss.message)
            .collect(),
    })
}

/// `file` with the entity at Directory `sequence` replaced by a null entity
/// (Type 0), or `None` when `file` holds no fixed-column Directory entry at
/// `sequence`.
pub(super) fn without_entity(file: &[u8], sequence: u32) -> Option<Vec<u8>> {
    let number = |columns: &[u8]| {
        std::str::from_utf8(columns)
            .ok()
            .and_then(|text| text.trim().parse::<u32>().ok())
    };
    let mut replaced = false;
    let mut first_parameter = true;
    let mut out = Vec::with_capacity(file.len());
    for line in file.split_inclusive(|byte| *byte == b'\n') {
        let mut line = line.to_vec();
        if line.len() >= 80 {
            let card = number(&line[73..80]);
            match line[72] {
                b'D' if card == Some(sequence)
                    || card.and_then(|card| card.checked_sub(1)) == Some(sequence) =>
                {
                    line[..8].copy_from_slice(b"       0");
                    replaced = true;
                }
                b'P' if number(&line[64..72]) == Some(sequence) => {
                    let text = if first_parameter { "0;" } else { "" };
                    line[..64].copy_from_slice(format!("{text:<64}").as_bytes());
                    first_parameter = false;
                }
                _ => {}
            }
        }
        out.extend_from_slice(&line);
    }
    replaced.then_some(out)
}

/// A geometry record with each record it names written in place of the name,
/// and no ids.
#[derive(Debug, Clone)]
pub(super) struct Fact {
    arena: String,
    value: Value,
    /// The id of the record and of each record written in its value.
    records: BTreeSet<String>,
}

impl Fact {
    /// The id of the record and of each record written in its value.
    pub(super) fn records(&self) -> impl Iterator<Item = &str> {
        self.records.iter().map(String::as_str)
    }
}

/// The facts of one decode, by record id, without the record fields
/// `removed`.
pub(super) struct Facts {
    facts: BTreeMap<String, Fact>,
    removed: Vec<&'static str>,
}

impl Facts {
    pub(super) fn of(ir: &CadIr, removed: &[&'static str]) -> Self {
        let geometry = serde_json::to_value(&ir.geometry).unwrap();
        let mut records = BTreeMap::new();
        for (arena, values) in geometry.as_object().unwrap() {
            for record in values.as_array().into_iter().flatten() {
                if let Some(id) = record.get("id").and_then(Value::as_str) {
                    records.insert(id.to_owned(), (arena.as_str(), record));
                }
            }
        }
        let mut facts = BTreeMap::new();
        for (id, (arena, record)) in &records {
            if FACT_ARENAS.contains(arena) {
                let mut inliner = Inliner {
                    records: &records,
                    removed,
                    named: vec![id.as_str()],
                    reached: BTreeSet::from([id.clone()]),
                };
                let value = inliner.stated(record);
                facts.insert(
                    id.clone(),
                    Fact {
                        arena: (*arena).to_owned(),
                        value,
                        records: inliner.reached,
                    },
                );
            }
        }
        Self {
            facts,
            removed: removed.to_vec(),
        }
    }

    /// The facts of this decode that the decode of `file` does not state
    /// with the same id and value. A file the decoder refuses states none.
    pub(super) fn changed_in(&self, file: &[u8]) -> BTreeMap<String, Fact> {
        let other =
            decode(file).map_or_else(BTreeMap::new, |ir| Self::of(&ir, &self.removed).facts);
        self.facts
            .iter()
            .filter(|(id, fact)| {
                other
                    .get(*id)
                    .is_none_or(|other| other.arena != fact.arena || other.value != fact.value)
            })
            .map(|(id, fact)| (id.clone(), fact.clone()))
            .collect()
    }

    /// The number of records of this decode that state `fact`.
    pub(super) fn count(&self, fact: &Fact) -> usize {
        self.facts
            .values()
            .filter(|own| own.arena == fact.arena && same(&own.value, &fact.value))
            .count()
    }

    /// The number of the records `ids` of this decode that state `fact`.
    pub(super) fn count_among<'a>(
        &self,
        fact: &Fact,
        ids: impl IntoIterator<Item = &'a String>,
    ) -> usize {
        ids.into_iter()
            .filter_map(|id| self.facts.get(id))
            .filter(|own| own.arena == fact.arena && same(&own.value, &fact.value))
            .count()
    }
}

/// Writes the records a geometry record names in place of their names.
struct Inliner<'a> {
    records: &'a BTreeMap<String, (&'a str, &'a Value)>,
    removed: &'a [&'a str],
    /// The records being written, outermost first, which a cycle names.
    named: Vec<&'a str>,
    /// Every record written.
    reached: BTreeSet<String>,
}

impl<'a> Inliner<'a> {
    fn stated(&mut self, value: &'a Value) -> Value {
        match value {
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .filter(|(key, _)| {
                        !matches!(key.as_str(), "id" | "source_object" | "tolerance")
                            && !self.removed.contains(&key.as_str())
                    })
                    .filter(|(key, _)| *key != "parameter_range" || !spans_its_knots(value))
                    .map(|(key, field)| (key.clone(), self.stated(field)))
                    .collect(),
            ),
            Value::Array(items) => {
                Value::Array(items.iter().map(|item| self.stated(item)).collect())
            }
            Value::String(text) => match self.records.get_key_value(text) {
                Some((id, (arena, record)))
                    if NAMED_ARENAS.contains(arena) && !self.named.contains(&id.as_str()) =>
                {
                    self.named.push(id);
                    self.reached.insert(id.clone());
                    let value = self.stated(record);
                    self.named.pop();
                    value
                }
                Some((_, (arena, _))) => Value::String(format!("<{arena}>")),
                None => value.clone(),
            },
            Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        }
    }
}

/// Whether the curve `value` states as its parameter range the knot domain
/// of its B-spline, which is the range the curve has without one.
fn spans_its_knots(value: &Value) -> bool {
    let knots = value
        .pointer("/geometry/value/knots")
        .and_then(Value::as_array);
    let range = value.get("parameter_range").and_then(Value::as_array);
    match (knots, range) {
        (Some(knots), Some(range)) => {
            knots.first().is_some_and(|first| same(first, &range[0]))
                && knots.last().is_some_and(|last| same(last, &range[1]))
        }
        _ => false,
    }
}

/// Whether two stated values agree, each number within the restatement
/// tolerance.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
            (a - b).abs() <= NUMBER_TOLERANCE * a.abs().max(b.abs()).max(1.0)
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, a)| b.get(key).is_some_and(|b| same(a, b)))
        }
        _ => a == b,
    }
}

/// The Global facts a decode states: the text fields by number when stated,
/// the model scale and unit, the millimetre width of each line weight, the
/// minimum resolution in millimetres, and the drafting standard.
#[derive(Debug)]
pub(super) struct GlobalFacts {
    text: BTreeMap<usize, SourceField>,
    model_scale: Option<f64>,
    units_flag: Option<i64>,
    units_name: Option<String>,
    line_widths_mm: Option<Vec<f64>>,
    minimum_resolution_mm: Option<f64>,
    drafting_standard: i64,
}

impl GlobalFacts {
    pub(super) fn of(ir: &CadIr) -> Option<Self> {
        let bytes = crate::writer::source_global::source_global_bytes(ir)?;
        let fields = crate::global::source_fields(&bytes)?;
        let resolved = crate::global::resolved(&bytes)?;
        let text = TEXT_FIELDS
            .iter()
            .filter_map(|(number, _)| match fields.get(number - 1)? {
                SourceField::Omitted => None,
                field => Some((*number, field.clone())),
            })
            .collect();
        Some(Self {
            text,
            model_scale: resolved
                .model_scale()
                .map(cadmpeg_ir::scalar::PositiveReal::get),
            units_flag: resolved.units_flag(),
            units_name: resolved.units_name().map(str::to_owned),
            line_widths_mm: resolved.length_context().map(|context| {
                (1..)
                    .map_while(|number| context.line_weight_mm(number))
                    .collect()
            }),
            minimum_resolution_mm: resolved
                .length_context()
                .map(|context| context.minimum_resolution_mm()),
            drafting_standard: resolved.drafting_standard(),
        })
    }

    /// The names of the fields whose source facts `output` does not state.
    /// A line width names either line-weight field.
    pub(super) fn unrestated_in(&self, output: &Self) -> Vec<&'static [&'static str]> {
        let numbers_agree = |source: &[f64], output: &[f64]| {
            source.len() == output.len()
                && source.iter().zip(output).all(|(source, output)| {
                    (source - output).abs()
                        <= NUMBER_TOLERANCE * source.abs().max(output.abs()).max(1.0)
                })
        };
        let optional_agree = |source: Option<&[f64]>, output: Option<&[f64]>| match (source, output)
        {
            (None, _) => true,
            (Some(source), Some(output)) => numbers_agree(source, output),
            (Some(_), None) => false,
        };
        let mut unrestated = TEXT_FIELDS
            .iter()
            .filter(|(number, _)| {
                self.text
                    .get(number)
                    .is_some_and(|field| output.text.get(number) != Some(field))
            })
            .map(|(_, name)| std::slice::from_ref(name))
            .collect::<Vec<_>>();
        let facts: [(bool, &'static [&'static str]); 6] = [
            (
                self.model_scale == output.model_scale,
                &["model space scale"],
            ),
            (self.units_flag == output.units_flag, &["units flag"]),
            (self.units_name == output.units_name, &["units name"]),
            (
                optional_agree(
                    self.line_widths_mm.as_deref(),
                    output.line_widths_mm.as_deref(),
                ),
                &["maximum line width", "maximum line weight gradations"],
            ),
            (
                optional_agree(
                    self.minimum_resolution_mm
                        .as_ref()
                        .map(std::slice::from_ref),
                    output
                        .minimum_resolution_mm
                        .as_ref()
                        .map(std::slice::from_ref),
                ),
                &["minimum resolution"],
            ),
            (
                self.drafting_standard == output.drafting_standard,
                &["drafting standard"],
            ),
        ];
        unrestated.extend(
            facts
                .into_iter()
                .filter(|(agree, _)| !agree)
                .map(|(_, names)| names),
        );
        unrestated
    }
}
