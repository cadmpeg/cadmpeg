//! The facts a decode states, compared without record ids.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use cadmpeg_ir::codec::write::NativeDispositions;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::CadIr;
use serde_json::Value;

use crate::global::SourceField;
use crate::writer::dispositions::appearances::AppearanceDrops;
use crate::{IgesCodec, IgesVersion};

/// The geometry arenas whose records are facts an entity states.
const FACT_ARENAS: &[&str] = &[
    "points", "curves", "surfaces", "pcurves", "vertices", "edges", "coedges", "loops", "faces",
    "shells", "bodies",
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

/// The fields of a geometry record that the Directory display fields of the
/// entity it is decoded from state.
const DISPLAY_FIELDS: &[&str] = &["color", "visible"];

/// The value that stands for a stated value another disposition charges. It
/// agrees with every value, and with a field the other side lacks.
const CHARGED: &str = "<charged>";

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
    /// The appearances and appearance bindings the write does not state.
    pub(super) appearances: AppearanceDrops,
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
    let (synthesis, native, appearances) = crate::test_support::with_policy_context(
        &[],
        &cadmpeg_core::decode::DecodePolicy {
            limits: cadmpeg_core::decode::ResourceLimits::desktop(),
        },
        |ctx| {
            let recovered =
                cadmpeg_ir::codec::write::edge_recovery::recover_edge_curves(&source, ctx)
                    .map_err(|_| "edge recovery")?;
            let (synthesis, native) = crate::writer::target::synthesis_and_dispositions(
                ctx,
                &recovered.document,
                IgesVersion::V5_3,
            )
            .map_err(|_| "write")?;
            let appearances = AppearanceDrops::of(&recovered.document, &synthesis);
            Ok::<_, &'static str>((synthesis, native, appearances))
        },
    )?;
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
        appearances,
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

/// The Directory sequence of the entity the decoder derives the geometry
/// record `id` from: the last `D{n}` piece of its key.
pub(super) fn decoded_from(id: &str) -> Option<u32> {
    id.split_once('#')?
        .1
        .rsplit([':', '-'])
        .find_map(|piece| piece.strip_prefix('D')?.parse().ok())
}

/// The source records whose stated values a write's other dispositions
/// charge.
#[derive(Default)]
pub(super) struct Charged {
    /// The sequences of the Dropped entities. A reference to a record decoded
    /// from one of them is that entity's loss.
    pub(super) entities: BTreeSet<u32>,
    /// The sequences of the entities whose display attribute record is
    /// Dropped. The colour and blank status of a record decoded from one of
    /// them is that record's loss.
    pub(super) displays: BTreeSet<u32>,
}

/// A geometry record with each record it names written in place of the name,
/// and no ids.
#[derive(Debug, Clone)]
pub(super) struct Fact {
    arena: String,
    /// The record as the decode states it.
    stated: Value,
    /// The record with each value another disposition charges replaced by
    /// [`CHARGED`].
    compared: Value,
}

/// Which value of a fact a count compares.
#[derive(Clone, Copy)]
pub(super) enum Compared {
    /// The value with each value another disposition charges replaced: what
    /// a Regenerated entity states.
    Regenerated,
    /// The value as the decode states it: what a Dropped entity loses.
    Dropped,
}

impl Fact {
    fn value(&self, compared: Compared) -> &Value {
        match compared {
            Compared::Regenerated => &self.compared,
            Compared::Dropped => &self.stated,
        }
    }
}

/// The facts of one decode, by record id.
pub(super) struct Facts {
    facts: BTreeMap<String, Fact>,
}

impl Facts {
    pub(super) fn of(ir: &CadIr, charged: &Charged) -> Self {
        let geometry = cadmpeg_core::serde_erase::json::to_value(&ir.geometry).unwrap();
        let records = geometry_records(&geometry);
        let no_charge = Charged::default();
        let mut facts = BTreeMap::new();
        for (id, (arena, record)) in &records {
            if FACT_ARENAS.contains(arena) {
                let state = |charged: &Charged| {
                    Inliner {
                        records: &records,
                        charged,
                        named: vec![id.as_str()],
                    }
                    .stated(record)
                };
                facts.insert(
                    id.clone(),
                    Fact {
                        arena: (*arena).to_owned(),
                        stated: state(&no_charge),
                        compared: state(charged),
                    },
                );
            }
        }
        Self { facts }
    }

    /// The facts of this decode that the decode of `file` does not state
    /// with the same id and value. A file the decoder refuses states none.
    pub(super) fn changed_in(&self, file: &[u8]) -> BTreeMap<String, Fact> {
        let other =
            decode(file).map_or_else(BTreeMap::new, |ir| Self::of(&ir, &Charged::default()).facts);
        self.facts
            .iter()
            .filter(|(id, fact)| {
                other
                    .get(*id)
                    .is_none_or(|other| other.arena != fact.arena || other.stated != fact.stated)
            })
            .map(|(id, fact)| (id.clone(), fact.clone()))
            .collect()
    }

    /// The number of records of this decode that state `fact`.
    pub(super) fn count(&self, fact: &Fact, compared: Compared) -> usize {
        self.facts
            .values()
            .filter(|own| {
                own.arena == fact.arena && same(own.value(compared), fact.value(compared))
            })
            .count()
    }

    /// The number of the records `ids` of this decode that state `fact`.
    pub(super) fn count_among<'a>(
        &self,
        fact: &Fact,
        ids: impl IntoIterator<Item = &'a String>,
    ) -> usize {
        let compared = Compared::Regenerated;
        ids.into_iter()
            .filter_map(|id| self.facts.get(id))
            .filter(|own| {
                own.arena == fact.arena && same(own.value(compared), fact.value(compared))
            })
            .count()
    }
}

/// Each record of the serialized geometry `geometry`, with its arena, by id.
fn geometry_records(geometry: &Value) -> BTreeMap<String, (&str, &Value)> {
    let mut records = BTreeMap::new();
    for (arena, values) in geometry.as_object().unwrap() {
        for record in values.as_array().into_iter().flatten() {
            if let Some(id) = record.get("id").and_then(Value::as_str) {
                records.insert(id.to_owned(), (arena.as_str(), record));
            }
        }
    }
    records
}

/// Writes the records a geometry record names in place of their names.
struct Inliner<'a> {
    records: &'a BTreeMap<String, (&'a str, &'a Value)>,
    charged: &'a Charged,
    /// The records being written, outermost first, which a cycle names.
    named: Vec<&'a str>,
}

impl<'a> Inliner<'a> {
    /// `value` with each geometry record id in it written as the record.
    fn target(&mut self, value: &'a Value) -> Value {
        match value {
            Value::String(text) => match self.records.get_key_value(text) {
                Some((id, (_, record))) => {
                    self.named.push(id);
                    let value = self.stated(record);
                    self.named.pop();
                    value
                }
                None => value.clone(),
            },
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(key, field)| (key.clone(), self.target(field)))
                    .collect(),
            ),
            Value::Array(items) => {
                Value::Array(items.iter().map(|item| self.target(item)).collect())
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        }
    }

    fn stated(&mut self, value: &'a Value) -> Value {
        match value {
            Value::Object(fields) => {
                let display_charged = fields
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(decoded_from)
                    .is_some_and(|sequence| self.charged.displays.contains(&sequence));
                Value::Object(
                    fields
                        .iter()
                        .filter(|(key, _)| {
                            !matches!(key.as_str(), "id" | "source_object" | "tolerance")
                        })
                        .filter(|(key, _)| *key != "parameter_range" || !spans_its_knots(value))
                        .map(|(key, field)| {
                            let value = if display_charged && DISPLAY_FIELDS.contains(&key.as_str())
                            {
                                Value::String(CHARGED.to_owned())
                            } else {
                                self.stated(field)
                            };
                            (key.clone(), value)
                        })
                        .collect(),
                )
            }
            Value::Array(items) => {
                Value::Array(items.iter().map(|item| self.stated(item)).collect())
            }
            Value::String(text) => match self.records.get_key_value(text) {
                Some(_)
                    if decoded_from(text)
                        .is_some_and(|sequence| self.charged.entities.contains(&sequence)) =>
                {
                    Value::String(CHARGED.to_owned())
                }
                Some((id, (arena, record)))
                    if NAMED_ARENAS.contains(arena) && !self.named.contains(&id.as_str()) =>
                {
                    self.named.push(id);
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

/// The value of each display attribute record of a decode, by id, with its
/// colour as red, green, and blue fractions, and no ids.
pub(super) fn display_values(ir: &CadIr) -> BTreeMap<String, Value> {
    let namespace = ir.native.namespace("iges").unwrap();
    let arena = |name: &str| {
        namespace
            .arenas()
            .get(name)
            .into_iter()
            .flatten()
            .map(|record| (record.id().to_owned(), record.fields().unwrap()))
            .collect::<BTreeMap<_, _>>()
    };
    let colors = arena("colors");
    arena("display_attributes")
        .into_iter()
        .map(|(id, mut fields)| {
            let number = fields.get("color_number").and_then(Value::as_i64);
            let color = match number {
                Some(0) => Value::Null,
                Some(standard @ 1..) => crate::entities::presentation::standard_color(standard)
                    .map_or_else(
                        || Value::String(format!("standard colour {standard}")),
                        |color| cadmpeg_core::json!([color.r(), color.g(), color.b()]),
                    ),
                _ => fields
                    .get("color_definition")
                    .and_then(Value::as_str)
                    .and_then(|definition| colors.get(definition))
                    .map_or(Value::String("unresolved".to_owned()), |definition| {
                        let percent = |field: &str| {
                            definition
                                .get(field)
                                .and_then(Value::as_f64)
                                .map(|p| p / 100.0)
                        };
                        cadmpeg_core::json!([
                            percent("red_percent"),
                            percent("green_percent"),
                            percent("blue_percent")
                        ])
                    }),
            };
            for key in ["id", "source_entity", "color_number", "color_definition"] {
                fields.remove(key);
            }
            fields.insert("color".to_owned(), color);
            (id, Value::Object(fields))
        })
        .collect()
}

/// The value of each colour definition record of a decode, by id, with no
/// ids.
pub(super) fn color_values(ir: &CadIr) -> BTreeMap<String, Value> {
    ir.native
        .namespace("iges")
        .unwrap()
        .arenas()
        .get("colors")
        .into_iter()
        .flatten()
        .map(|record| {
            let mut fields = record.fields().unwrap();
            fields.remove("id");
            fields.remove("source_entity");
            (record.id().to_owned(), Value::Object(fields))
        })
        .collect()
}

/// The value of each appearance of a decode, by id, with no ids.
pub(super) fn appearance_values(ir: &CadIr) -> BTreeMap<String, Value> {
    ir.model
        .appearances
        .iter()
        .map(|appearance| {
            let mut value = cadmpeg_core::serde_erase::json::to_value(appearance).unwrap();
            let fields = value.as_object_mut().unwrap();
            fields.remove("id");
            fields.remove("native_ref");
            (appearance.id.as_str().to_owned(), value)
        })
        .collect()
}

/// The value of each appearance binding of a decode, by id: the colour of
/// its appearance in place of the appearance, and the geometry record it
/// binds written in place of its id, with the values `charged` charges
/// replaced, and no ids.
pub(super) fn binding_values(ir: &CadIr, charged: &Charged) -> BTreeMap<String, Value> {
    let geometry = cadmpeg_core::serde_erase::json::to_value(&ir.geometry).unwrap();
    let records = geometry_records(&geometry);
    ir.model
        .appearance_bindings
        .iter()
        .map(|binding| {
            let mut value = cadmpeg_core::serde_erase::json::to_value(binding).unwrap();
            let fields = value.as_object_mut().unwrap();
            fields.remove("id");
            fields.remove("native_ref");
            let color = ir
                .model
                .appearances
                .iter()
                .find(|appearance| appearance.id == binding.appearance)
                .and_then(|appearance| appearance.base_color);
            fields.insert(
                "appearance".to_owned(),
                cadmpeg_core::serde_erase::json::to_value(color).unwrap(),
            );
            let target = fields.remove("target").unwrap_or(Value::Null);
            let mut inliner = Inliner {
                records: &records,
                charged,
                named: Vec::new(),
            };
            fields.insert("target".to_owned(), inliner.target(&target));
            (binding.id.as_str().to_owned(), value)
        })
        .collect()
}

/// The number of `values` that agree with `value`.
pub(super) fn tally<'a>(values: impl Iterator<Item = &'a Value>, value: &Value) -> usize {
    values.filter(|other| same(other, value)).count()
}

/// Whether `values` holds a value that agrees with `value`.
pub(super) fn holds<'a>(mut values: impl Iterator<Item = &'a Value>, value: &Value) -> bool {
    values.any(|other| same(other, value))
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
/// tolerance. [`CHARGED`] agrees with every value.
fn same(a: &Value, b: &Value) -> bool {
    let charged = |value: &Value| value.as_str() == Some(CHARGED);
    if charged(a) || charged(b) {
        return true;
    }
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
            (a - b).abs() <= NUMBER_TOLERANCE * a.abs().max(b.abs()).max(1.0)
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.iter()
                .all(|(key, a)| b.get(key).map_or(charged(a), |b| same(a, b)))
                && b.iter().all(|(key, b)| a.contains_key(key) || charged(b))
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
