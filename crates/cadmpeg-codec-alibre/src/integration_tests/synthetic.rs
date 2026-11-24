// SPDX-License-Identifier: Apache-2.0
//! Assemblies whose `STEP_DATA` text the tests write, patched into the
//! compound container of a committed assembly.
//!
//! The container keeps its streams contiguous, so a text that is not longer
//! than the original stream replaces it in place and a space pads the rest.

use std::fmt::Write;

use cadmpeg_core::decode::{Companion, Companions};

use super::assembly::{count_loss, decode_with, named, swap, ASM_BOX, ASM_CYLINDER, ASM_TWO};
use crate::loss::AlibreLossCode;

/// The GUID of the document `asm_box`.
const BOX_GUID: &str = "58F88938-2D66-43CA-BFF8-892C49CE60A9";

const IDENTITY: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// One inclusion of a synthetic assembly. Lengths are centimetres.
#[derive(Clone)]
struct Inc {
    path: String,
    guid: String,
    axes: [[f64; 3]; 3],
    scale: f64,
    origin: [f64; 3],
    /// Whether the operator is null instead.
    no_operator: bool,
    /// The hidden flag of the own member; `None` for no own member.
    own: Option<bool>,
    /// The hidden flag of a member that names a component of a subassembly.
    nested: Option<bool>,
    configuration: String,
    /// Attributes after the fifteenth.
    extra: usize,
    anchored: bool,
}

impl Inc {
    fn new(path: &str, guid: &str) -> Self {
        Self {
            path: path.to_owned(),
            guid: guid.to_owned(),
            axes: IDENTITY,
            scale: 1.0,
            origin: [0.0; 3],
            no_operator: false,
            own: Some(false),
            nested: None,
            configuration: "Config<1>".to_owned(),
            extra: 0,
            anchored: false,
        }
    }
}

struct Config {
    name: String,
}

/// A synthetic assembly.
struct Synth {
    inclusions: Vec<Inc>,
    configurations: Vec<Config>,
    /// The index of the active configuration, if any.
    active: Option<usize>,
    /// Further entity lines.
    entities: Vec<String>,
    /// Inclusion numbers that the active configuration suppresses.
    suppressed: Vec<usize>,
    /// Lists an inclusion and a configuration that are not records of their
    /// kind.
    broken: bool,
}

impl Synth {
    fn new(inclusions: Vec<Inc>) -> Self {
        Self {
            inclusions,
            configurations: vec![Config {
                name: "Config<1>".to_owned(),
            }],
            active: Some(0),
            entities: Vec::new(),
            suppressed: Vec::new(),
            broken: false,
        }
    }

    fn text(&self) -> String {
        let mut out = String::from(
            "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('ALIBRE_SCHEMA'));\nENDSEC;\nDATA;\n",
        );
        let mut next = 10_u32;
        let mut id = || {
            next += 1;
            next
        };
        let product = id();
        writeln!(out, "#{product}=PRODUCT('p','synthetic','','',());").unwrap();
        let mut inclusion_ids = Vec::new();
        let mut references = std::collections::BTreeMap::new();
        let mut config_ids = Vec::new();
        for _ in &self.configurations {
            config_ids.push(id());
        }
        // Inclusion ids are reserved first so a configuration can list them.
        let reserved: Vec<u32> = self.inclusions.iter().map(|_| id()).collect();
        for (at, config) in self.configurations.iter().enumerate() {
            let suppressed = if at == 0 {
                self.suppressed
                    .iter()
                    .map(|index| format!("#{}", reserved[*index]))
                    .collect::<Vec<_>>()
                    .join(",")
            } else {
                String::new()
            };
            writeln!(
                out,
                "#{}=DESIGNASSEMBLYCONFIGURATION('{}',0,0,2,3326,1,-1,-1,({suppressed}),(),());",
                config_ids[at], config.name
            )
            .unwrap();
        }
        for (at, inc) in self.inclusions.iter().enumerate() {
            let reference = *references
                .entry((inc.path.clone(), inc.guid.clone()))
                .or_insert_with(|| {
                    let reference = id();
                    writeln!(
                        out,
                        "#{reference}=REPOSITORYVERSIONREFERENCE(1,'C','{}','{}','','',1,1,$);",
                        inc.path, inc.guid
                    )
                    .unwrap();
                    reference
                });
            let mut members = Vec::new();
            for (hidden, nested) in [(inc.own, false), (inc.nested, true)] {
                if let Some(hidden) = hidden {
                    let member = id();
                    let flag = if hidden { "T" } else { "F" };
                    let link = if nested {
                        "'A1B2C3D4-0000-4000-8000-000000000001'"
                    } else {
                        "$"
                    };
                    writeln!(
                        out,
                        "#{member}=DESIGNMEMBER($,$,0,.{flag}.,0,'0000000{at}-0000-4000-8000-00000000000{}',{link},$,0);",
                        u8::from(nested)
                    )
                    .unwrap();
                    members.push(format!("#{member}"));
                }
            }
            let operator = if inc.no_operator {
                "$".to_owned()
            } else {
                let mut vector = |value: [f64; 3], entity: &str| {
                    let vector = id();
                    writeln!(
                        out,
                        "#{vector}={entity}($,({:?},{:?},{:?}));",
                        value[0], value[1], value[2]
                    )
                    .unwrap();
                    vector
                };
                let x = vector(inc.axes[0], "DIRECTION");
                let y = vector(inc.axes[1], "DIRECTION");
                let z = vector(inc.axes[2], "DIRECTION");
                let origin = vector(inc.origin, "CARTESIAN_POINT");
                let operator = id();
                writeln!(
                    out,
                    "#{operator}=CARTESIAN_TRANSFORMATION_OPERATOR_3D($,$,$,#{x},#{y},#{origin},{:?},#{z});",
                    inc.scale
                )
                .unwrap();
                format!("#{operator}")
            };
            let tail = ",$".repeat(inc.extra);
            let anchored = if inc.anchored { "T" } else { "F" };
            writeln!(
                out,
                "#{}=DESIGNINCLUSION((),({}),'{}',#{reference},$,{operator},$,.{anchored}.,$,'0000000{at}-0000-4000-8000-0000000000FF',.F.,#{product},0,$,'{}'{tail});",
                reserved[at],
                members.join(","),
                inc.path,
                inc.configuration
            )
            .unwrap();
            inclusion_ids.push(reserved[at]);
        }
        for line in &self.entities {
            writeln!(out, "{line}").unwrap();
        }
        if self.broken {
            out.push_str("#8001=DESIGNINCLUSION(());\n#8002=PRODUCT('q','x','','',());\n");
            inclusion_ids.push(8001);
            config_ids.push(8002);
        }
        let list = |ids: &[u32]| {
            ids.iter()
                .map(|id| format!("#{id}"))
                .collect::<Vec<_>>()
                .join(",")
        };
        let active = self
            .active
            .map_or("$".to_owned(), |at| format!("#{}", config_ids[at]));
        let nulls = vec!["$"; 18].join(",");
        writeln!(
            out,
            "#9000=DESIGNASSEMBLY({nulls},#{product},$,({}),(),(),(),$,({}),{active});",
            list(&inclusion_ids),
            list(&config_ids)
        )
        .unwrap();
        out.push_str("ENDSEC;\nEND-ISO-10303-21;\r\n");
        out
    }

    /// The compound file of `ASM_TWO` with this text as its `STEP_DATA`.
    fn file(&self) -> Vec<u8> {
        replace_step(ASM_TWO, &self.text())
    }
}

/// The compound file `base` with `text` as its `STEP_DATA` stream.
fn replace_step(base: &[u8], text: &str) -> Vec<u8> {
    let start = find(base, b"ISO-10303-21;").expect("a STEP_DATA stream");
    let end_mark = b"END-ISO-10303-21;";
    let end = find(base, end_mark).expect("the end of the stream") + end_mark.len() + 2;
    assert!(
        text.len() <= end - start,
        "the text is {} octets",
        text.len()
    );
    let mut bytes = base.to_vec();
    bytes[start..end].fill(b' ');
    bytes[start..start + text.len()].copy_from_slice(text.as_bytes());
    bytes
}

fn find(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    bytes
        .windows(needle.len())
        .position(|window| window == needle)
}

/// The files `asm_box.AD_PRT` and `asm_cylinder.AD_PRT`, and any more.
#[derive(Debug)]
struct Beside {
    files: Vec<(String, Vec<u8>)>,
    asked: std::cell::RefCell<Vec<String>>,
}

impl Beside {
    fn parts() -> Self {
        Self::with(vec![
            ("asm_box.AD_PRT".to_owned(), ASM_BOX.to_vec()),
            ("asm_cylinder.AD_PRT".to_owned(), ASM_CYLINDER.to_vec()),
        ])
    }

    fn with(files: Vec<(String, Vec<u8>)>) -> Self {
        Self {
            files,
            asked: std::cell::RefCell::default(),
        }
    }
}

impl Companions for Beside {
    fn open(&self, _anchor: Option<&str>, path: &str, _max: u64) -> Result<Companion, String> {
        self.asked.borrow_mut().push(path.to_owned());
        self.files
            .iter()
            .find(|(name, _)| name == path)
            .map(|(name, bytes)| Companion {
                id: name.clone(),
                bytes: bytes.clone(),
            })
            .ok_or_else(|| format!("{path}: not found"))
    }
}

fn losses_of(result: &cadmpeg_ir::codec::DecodeResult, code: AlibreLossCode) -> Vec<String> {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == code.kind())
        .map(|loss| loss.message.clone())
        .collect()
}

fn decode_synth(synth: &Synth, files: &Beside) -> cadmpeg_ir::codec::DecodeResult {
    decode_with(&synth.file(), files)
}

#[test]
fn a_synthetic_assembly_decodes_like_the_specimens() {
    let mut synth = Synth::new(vec![Inc::new("D:/w/asm_box", BOX_GUID)]);
    synth.inclusions[0].origin = [1.0, 2.0, 3.0];
    let result = decode_synth(&synth, &Beside::parts());
    let occurrence = named(result.ir(), "asm_box<1>");
    let rows = occurrence.transform.unwrap().affine_rows();
    assert_eq!([rows[0][3], rows[1][3], rows[2][3]], [10.0, 20.0, 30.0]);
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    // One configuration of the default name loses nothing.
    for code in [
        AlibreLossCode::AssemblyConfigurationUnmapped,
        AlibreLossCode::AssemblyRecordUnreadable,
        AlibreLossCode::AssemblyPlacementUnreadable,
        AlibreLossCode::AssemblyInclusionUnmapped,
    ] {
        assert_eq!(count_loss(&result, code), 0, "{code:?}");
    }
}

#[test]
fn placements_that_are_not_rigid_are_reported_in_one_loss() {
    let r = std::f64::consts::FRAC_1_SQRT_2;
    let mut sheared = Inc::new("a/asm_box", BOX_GUID);
    sheared.axes = [[1.0, 0.0, 0.0], [r, r, 0.0], [0.0, 0.0, 1.0]];
    let mut mirrored = Inc::new("a/asm_box", BOX_GUID);
    mirrored.axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]];
    let mut scaled = Inc::new("a/asm_box", BOX_GUID);
    scaled.scale = 2.0;
    let mut bare = Inc::new("a/asm_box", BOX_GUID);
    bare.no_operator = true;
    let mut skewed_third = Inc::new("a/asm_box", BOX_GUID);
    skewed_third.axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, r, r]];
    let mut turned = Inc::new("a/asm_box", BOX_GUID);
    turned.axes = [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    let synth = Synth::new(vec![sheared, mirrored, scaled, bare, skewed_third, turned]);
    let result = decode_synth(&synth, &Beside::parts());
    let losses = losses_of(&result, AlibreLossCode::AssemblyPlacementUnreadable);
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert!(losses[0].starts_with("5 inclusion(s)"), "{}", losses[0]);
    for reason in ["orthogonal", "mirror", "scale 2", "no CARTESIAN", "z axis"] {
        assert!(losses[0].contains(reason), "{reason}: {}", losses[0]);
    }
    let occurrences = &result.ir().model.occurrences;
    assert_eq!(occurrences.len(), 6);
    assert_eq!(
        occurrences
            .iter()
            .filter(|occurrence| occurrence.transform.is_some())
            .count(),
        1
    );
    assert!(occurrences[5].transform.is_some());
}

#[test]
fn trailing_attributes_and_unusual_values_are_reported_not_refused() {
    let mut extra = Inc::new("a/asm_box", BOX_GUID);
    extra.extra = 2;
    let synth = Synth::new(vec![extra, Inc::new("a/asm_box", BOX_GUID)]);
    let result = decode_synth(&synth, &Beside::parts());
    let losses = losses_of(&result, AlibreLossCode::AssemblyInclusionUnmapped);
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert!(losses[0].starts_with("1 inclusion(s)"), "{}", losses[0]);
    assert!(losses[0].contains("[15, 16]"), "{}", losses[0]);
    assert_eq!(result.ir().model.occurrences.len(), 2);
}

#[test]
fn a_record_that_does_not_read_is_reported_and_the_rest_decodes() {
    let mut synth = Synth::new(vec![Inc::new("a/asm_box", BOX_GUID)]);
    synth.broken = true;
    let result = decode_synth(&synth, &Beside::parts());
    let losses = losses_of(&result, AlibreLossCode::AssemblyRecordUnreadable);
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert!(
        losses[0].starts_with("2 assembly record(s)"),
        "{}",
        losses[0]
    );
    assert_eq!(result.ir().model.occurrences.len(), 1);
    // The active configuration is the first, which still reads.
    assert_eq!(named(result.ir(), "asm_box<1>").suppressed, Some(false));
}

#[test]
fn a_hidden_nested_member_is_reported_once_for_all_inclusions() {
    let mut first = Inc::new("a/asm_box", BOX_GUID);
    first.nested = Some(true);
    let mut second = Inc::new("a/asm_box", BOX_GUID);
    second.nested = Some(true);
    let synth = Synth::new(vec![first, second, Inc::new("a/asm_box", BOX_GUID)]);
    let result = decode_synth(&synth, &Beside::parts());
    let losses = losses_of(&result, AlibreLossCode::AssemblyNestedStateUnread);
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert!(losses[0].starts_with("2 inclusion(s) hide 2 component(s)"));
}

#[test]
fn an_entity_of_an_unknown_name_is_reported_as_unread() {
    let mut synth = Synth::new(vec![Inc::new("a/asm_box", BOX_GUID)]);
    synth.entities.push("#8000=FROBNICATE(1);".to_owned());
    synth.entities.push("#8001=FROBNICATE(2);".to_owned());
    let result = decode_synth(&synth, &Beside::parts());
    let losses = losses_of(&result, AlibreLossCode::AssemblyEntitiesUnread);
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert!(losses[0].contains("FROBNICATE x2"), "{}", losses[0]);
}

#[test]
fn the_configuration_loss_names_only_what_is_lost() {
    // A component configuration other than the default.
    let mut other = Inc::new("a/asm_box", BOX_GUID);
    other.configuration = "Config<2>".to_owned();
    let synth = Synth::new(vec![other]);
    let result = decode_synth(&synth, &Beside::parts());
    let losses = losses_of(&result, AlibreLossCode::AssemblyConfigurationUnmapped);
    assert_eq!(losses.len(), 1);
    assert!(
        losses[0].starts_with("1 occurrence(s) name"),
        "{}",
        losses[0]
    );

    // Configurations beyond the active one, counted by the active index.
    let mut synth = Synth::new(vec![Inc::new("a/asm_box", BOX_GUID)]);
    synth.configurations.push(Config {
        name: "Config<2>".to_owned(),
    });
    synth.active = Some(1);
    let result = decode_synth(&synth, &Beside::parts());
    let losses = losses_of(&result, AlibreLossCode::AssemblyConfigurationUnmapped);
    assert_eq!(losses.len(), 1);
    assert!(
        losses[0].contains("1 assembly configuration(s) beyond the active one"),
        "{}",
        losses[0]
    );
}

#[test]
fn a_missing_active_configuration_is_reported() {
    let mut synth = Synth::new(vec![Inc::new("a/asm_box", BOX_GUID)]);
    synth.active = None;
    let result = decode_synth(&synth, &Beside::parts());
    let losses = losses_of(&result, AlibreLossCode::AssemblyRecordUnreadable);
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert!(losses[0].contains("active configuration"), "{}", losses[0]);
    assert_eq!(named(result.ir(), "asm_box<1>").suppressed, None);
}

#[test]
fn a_hostile_component_path_is_not_looked_up() {
    let hostile = [
        "x/%2e%2e%2fsecret%2fpart",
        "%2fhome%2fu%2fother",
        "a/..",
        "a/.",
        "",
        "C:evil",
        "a/b\u{0}c",
        "a/b\nc",
        "a/ab%",
    ];
    let synth = Synth::new(
        hostile
            .iter()
            .map(|path| Inc::new(path, BOX_GUID))
            .collect(),
    );
    let files = Beside::parts();
    let result = decode_synth(&synth, &files);
    assert!(files.asked.borrow().is_empty(), "{:?}", files.asked);
    assert_eq!(result.ir().model.occurrences.len(), hostile.len());
    // One loss for each distinct name.
    assert_eq!(
        count_loss(&result, AlibreLossCode::AssemblyMemberUnavailable),
        hostile.len()
    );
    assert!(result.ir().geometry.bodies.is_empty());
}

#[test]
fn a_file_of_another_document_is_not_merged_under_the_name() {
    let other = swap(ASM_BOX, BOX_GUID, "00000000-AAAA-BBBB-CCCC-000000000000");
    // The same file name, two documents: the file beside the assembly is the
    // second document.
    let synth = Synth::new(vec![
        Inc::new("a/asm_box", BOX_GUID),
        Inc::new("b/asm_box", "00000000-AAAA-BBBB-CCCC-000000000000"),
    ]);
    let files = Beside::with(vec![("asm_box.AD_PRT".to_owned(), other)]);
    let result = decode_synth(&synth, &files);
    let ir = result.ir();
    assert_eq!(ir.model.occurrences.len(), 2);
    let first = named(ir, "asm_box<1>");
    let second = named(ir, "asm_box<2>");
    assert_ne!(first.prototype, second.prototype);
    assert_eq!(
        losses_of(&result, AlibreLossCode::AssemblyMemberDocumentMismatch).len(),
        1
    );
    assert_eq!(ir.geometry.bodies.len(), 1);
    // The file matches the second inclusion only.
    assert!(matches!(
        super::assembly::definition(ir, second).storage,
        cadmpeg_ir::products::Storage::File { .. }
    ));
    assert!(matches!(
        super::assembly::definition(ir, first).storage,
        cadmpeg_ir::products::Storage::External(_)
    ));
}

#[test]
fn a_component_that_names_no_document_is_taken_as_found() {
    let synth = Synth::new(vec![Inc::new("a/asm_box", "")]);
    let result = decode_synth(&synth, &Beside::parts());
    assert_eq!(
        count_loss(&result, AlibreLossCode::AssemblyMemberDocumentMismatch),
        0
    );
    assert_eq!(result.ir().geometry.bodies.len(), 1);
}

#[test]
fn the_unavailable_reason_names_the_attempts() {
    let synth = Synth::new(vec![Inc::new("a/asm_box", BOX_GUID)]);
    let files = Beside::with(Vec::new());
    let result = decode_synth(&synth, &files);
    let losses = losses_of(&result, AlibreLossCode::AssemblyMemberUnavailable);
    assert_eq!(losses.len(), 1);
    assert!(
        losses[0].contains("not found") && losses[0].contains("AD_PRT, AD_ASM, ad_prt, ad_asm"),
        "{}",
        losses[0]
    );
    assert_eq!(files.asked.borrow().len(), 4);
}

#[test]
fn an_assembly_that_includes_its_own_file_is_cut_at_once() {
    // The companion named asm_box is the assembly that requests it.
    let synth = Synth::new(vec![Inc::new("a/asm_box", BOX_GUID)]);
    let bytes = synth.file();
    let files = Beside::with(vec![("asm_box.AD_PRT".to_owned(), bytes.clone())]);
    let result = decode_with(&bytes, &files);
    assert_eq!(count_loss(&result, AlibreLossCode::AssemblyMemberCycle), 1);
}
