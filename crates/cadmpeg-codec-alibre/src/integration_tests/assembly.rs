// SPDX-License-Identifier: Apache-2.0
//! Assembly decode over the committed `.AD_ASM` specimens and their parts.
//!
//! The expected placements are the design values, in millimetres: the parts
//! are a 20 x 30 x 10 box (x -10..10, y -15..15, z 0..10) and a cylinder of
//! radius 10 and height 15 (z 0..15), each at the origin of its own file.

use std::cell::Cell;
use std::io::Cursor;

use cadmpeg_core::decode::{
    Companion, Companions, DecodeArena, DecodeContext, DecodePolicy, InspectOptions,
};
use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions, DecodeResult};
use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::{
    JointOperands, Occurrence, ProductDefinition, ProductDefinitionKind, Storage,
};
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::CadIr;
use cadmpeg_measure::{MeasureInput, MeasureOptions, Measurer, Selection};

use crate::loss::AlibreLossCode;
use crate::AlibreCodec;

pub(crate) const ASM_BOX: &[u8] = include_bytes!("../../../../corpus/alibre/asm_box.AD_PRT");
pub(crate) const ASM_CYLINDER: &[u8] =
    include_bytes!("../../../../corpus/alibre/asm_cylinder.AD_PRT");
pub(crate) const ASM_TWO: &[u8] = include_bytes!("../../../../corpus/alibre/asm_two.AD_ASM");
pub(crate) const ASM_REPEAT: &[u8] = include_bytes!("../../../../corpus/alibre/asm_repeat.AD_ASM");
pub(crate) const ASM_NESTED: &[u8] = include_bytes!("../../../../corpus/alibre/asm_nested.AD_ASM");
pub(crate) const ASM_ROTATED: &[u8] =
    include_bytes!("../../../../corpus/alibre/asm_rotated.AD_ASM");
const TWO_TWIN: &[u8] = include_bytes!("../../../../corpus/alibre/asm_two.stp");
const REPEAT_TWIN: &[u8] = include_bytes!("../../../../corpus/alibre/asm_repeat.stp");
const NESTED_TWIN: &[u8] = include_bytes!("../../../../corpus/alibre/asm_nested.stp");
const ROTATED_TWIN: &[u8] = include_bytes!("../../../../corpus/alibre/asm_rotated.stp");

/// The assemblies, by name.
pub(crate) fn assembly_specimens() -> [(&'static str, &'static [u8]); 4] {
    [
        ("asm_two", ASM_TWO),
        ("asm_repeat", ASM_REPEAT),
        ("asm_nested", ASM_NESTED),
        ("asm_rotated", ASM_ROTATED),
    ]
}

/// Component files by name, each with its identity.
#[derive(Debug)]
pub(crate) struct Files(Vec<(&'static str, &'static [u8])>);

impl Files {
    /// The five committed files of the specimens.
    pub(crate) fn corpus() -> Self {
        Self(vec![
            ("asm_box.AD_PRT", ASM_BOX),
            ("asm_cylinder.AD_PRT", ASM_CYLINDER),
            ("asm_two.AD_ASM", ASM_TWO),
            ("asm_repeat.AD_ASM", ASM_REPEAT),
            ("asm_nested.AD_ASM", ASM_NESTED),
        ])
    }
}

impl Companions for Files {
    fn open(&self, _anchor: Option<&str>, path: &str, _max: u64) -> Result<Companion, String> {
        self.0
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(name, bytes)| Companion {
                id: (*name).to_owned(),
                bytes: bytes.to_vec(),
            })
            .ok_or_else(|| format!("{path}: not found"))
    }
}

/// Serves the assembly `asm_two` for every box request, each under a new
/// identity, so that an assembly nests below itself without a repeated file.
#[derive(Debug)]
struct Endless {
    served: Cell<usize>,
    /// `asm_two` stating the document GUID that the inclusions of the box
    /// reference.
    two: Vec<u8>,
}

impl Endless {
    fn new() -> Self {
        Self {
            served: Cell::new(0),
            two: swap(
                ASM_TWO,
                "66949A14-F5DB-4970-BCD4-C5E4FD3F0BEA",
                "58F88938-2D66-43CA-BFF8-892C49CE60A9",
            ),
        }
    }
}

impl Companions for Endless {
    fn open(&self, _anchor: Option<&str>, path: &str, _max: u64) -> Result<Companion, String> {
        match path {
            "asm_box.AD_PRT" => {
                self.served.set(self.served.get() + 1);
                Ok(Companion {
                    id: format!("level-{}", self.served.get()),
                    bytes: self.two.clone(),
                })
            }
            "asm_cylinder.AD_PRT" => Ok(Companion {
                id: "cylinder".to_owned(),
                bytes: ASM_CYLINDER.to_vec(),
            }),
            _ => Err(format!("{path}: not found")),
        }
    }
}

/// Decodes `bytes` with the companion files `files`.
pub(crate) fn decode_with(bytes: &[u8], files: &dyn Companions) -> DecodeResult {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let ctx = ctx.with_companions(files);
    let result = AlibreCodec
        .decode_with_context(
            &ctx,
            root,
            &cadmpeg_ir::report::strictness::Strictness::default(),
        )
        .unwrap();
    ctx.finish_session().unwrap();
    result
}

fn decode_assembly(bytes: &[u8]) -> DecodeResult {
    decode_with(bytes, &Files::corpus())
}

/// `bytes` with every `from` replaced by `to` of the same length.
pub(super) fn swap(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
    assert_eq!(from.len(), to.len());
    let mut bytes = bytes.to_vec();
    let mut at = 0;
    while let Some(found) = find(&bytes[at..], from.as_bytes()) {
        let found = at + found;
        bytes[found..found + from.len()].copy_from_slice(to.as_bytes());
        at = found + to.len();
    }
    bytes
}

fn find(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    bytes
        .windows(needle.len())
        .position(|window| window == needle)
}

/// The occurrence of the assembly being decoded with `name`: a subassembly
/// can place a component of the same name, whose occurrence is not wanted.
pub(super) fn named<'a>(ir: &'a CadIr, name: &str) -> &'a Occurrence {
    let root = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .expect("a root definition");
    ir.model
        .occurrences
        .iter()
        .find(|occurrence| {
            occurrence.name.as_deref() == Some(name) && root.occurrences.contains(&occurrence.id)
        })
        .unwrap_or_else(|| panic!("no occurrence {name} in the root definition"))
}

pub(super) fn definition<'a>(ir: &'a CadIr, occurrence: &Occurrence) -> &'a ProductDefinition {
    ir.model
        .product_definitions
        .iter()
        .find(|definition| definition.id == occurrence.prototype)
        .unwrap()
}

fn translation(occurrence: &Occurrence) -> [f64; 3] {
    let rows = occurrence.transform.unwrap().affine_rows();
    [rows[0][3], rows[1][3], rows[2][3]]
}

fn assert_identity_rotation(occurrence: &Occurrence) {
    let rows = occurrence.transform.unwrap().affine_rows();
    for (row, values) in rows.iter().enumerate() {
        for (column, value) in values.iter().take(3).enumerate() {
            let expected = if row == column { 1.0 } else { 0.0 };
            assert!((value - expected).abs() < 1e-12, "{occurrence:?}");
        }
    }
}

fn assert_at(ir: &CadIr, name: &str, expected: [f64; 3]) {
    let occurrence = named(ir, name);
    assert_identity_rotation(occurrence);
    for (actual, expected) in translation(occurrence).into_iter().zip(expected) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "{name}: placed at {:?}, expected {expected:?}",
            translation(occurrence)
        );
    }
}

pub(super) fn assert_valid(result: &DecodeResult, name: &str) {
    let report = cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).unwrap();
    let errors: Vec<_> = report
        .findings
        .iter()
        .filter(|finding| finding.severity >= Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{name}: {errors:?}");
}

type Extent = ([f64; 3], [f64; 3]);

/// The box of the bodies one occurrence places, in the instance space of the
/// occurrence.
fn occurrence_extent(ir: &CadIr, occurrence: &OccurrenceId) -> Option<Extent> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
    let input = MeasureInput {
        ir,
        fidelity: None,
        geometry_transferred: true,
    };
    let selection = Selection {
        occurrence: Some(occurrence.clone()),
        ..Selection::default()
    };
    let report = Measurer::new(input, MeasureOptions::default())
        .measure(&ctx, &selection)
        .unwrap();
    ctx.finish_session().unwrap();
    let mut extent: Option<Extent> = None;
    for body in report.bodies() {
        let measured = body.bbox().measured().unwrap().value();
        let low = [measured.min.x, measured.min.y, measured.min.z];
        let high = [measured.max.x, measured.max.y, measured.max.z];
        extent = Some(match extent {
            None => (low, high),
            Some((min, max)) => (
                [0, 1, 2].map(|axis| min[axis].min(low[axis])),
                [0, 1, 2].map(|axis| max[axis].max(high[axis])),
            ),
        });
    }
    extent
}

/// The box of the placed components: the union of the extents of the
/// occurrences of the top definition that `keep` admits.
fn assembled_extent(ir: &CadIr, keep: impl Fn(&Occurrence) -> bool) -> Extent {
    let placed: std::collections::BTreeSet<_> = ir
        .model
        .occurrences
        .iter()
        .map(|occurrence| &occurrence.prototype)
        .collect();
    let mut union: Option<Extent> = None;
    for definition in
        ir.model.product_definitions.iter().filter(|definition| {
            !placed.contains(&definition.id) && !definition.occurrences.is_empty()
        })
    {
        for id in &definition.occurrences {
            let occurrence = ir.model.occurrences.iter().find(|o| o.id == *id).unwrap();
            if !keep(occurrence) {
                continue;
            }
            let Some((low, high)) = occurrence_extent(ir, id) else {
                continue;
            };
            union = Some(match union {
                None => (low, high),
                Some((min, max)) => (
                    [0, 1, 2].map(|axis| min[axis].min(low[axis])),
                    [0, 1, 2].map(|axis| max[axis].max(high[axis])),
                ),
            });
        }
    }
    union.expect("an occurrence with geometry")
}

fn assert_extent(actual: Extent, expected: Extent, what: &str) {
    for axis in 0..3 {
        assert!(
            (actual.0[axis] - expected.0[axis]).abs() < 1e-6
                && (actual.1[axis] - expected.1[axis]).abs() < 1e-6,
            "{what}: {actual:?}, expected {expected:?}"
        );
    }
}

fn twin_extent(bytes: &[u8], keep: impl Fn(&Occurrence) -> bool) -> Extent {
    let result = cadmpeg_codec_step::StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assembled_extent(result.ir(), keep)
}

pub(super) fn loss_codes(result: &DecodeResult) -> Vec<cadmpeg_ir::report::loss::LossKind> {
    result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.clone())
        .collect()
}

pub(super) fn count_loss(result: &DecodeResult, code: AlibreLossCode) -> usize {
    loss_codes(result)
        .iter()
        .filter(|kind| **kind == code.kind())
        .count()
}

#[test]
fn an_assembly_is_detected_and_inspected_as_its_own_dialect() {
    for (name, bytes) in assembly_specimens() {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default()).unwrap();
        assert_eq!(
            AlibreCodec.detect(&ctx, root).unwrap(),
            Confidence::High,
            "{name}"
        );
        ctx.finish_session().unwrap();
        let summary = AlibreCodec
            .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
            .unwrap();
        let dialects = summary.dialects().unwrap();
        assert_eq!(dialects.primary().dialect().as_str(), "alibre:cfb-assembly");
        let wire = cadmpeg_core::serde_erase::json::to_value(dialects).unwrap();
        assert!(
            wire["extra"].as_array().is_none_or(Vec::is_empty),
            "{name}: {wire}"
        );
        let roles: Vec<_> = summary
            .entries
            .iter()
            .map(|entry| (entry.name.as_str(), entry.role.as_str()))
            .collect();
        assert!(
            roles.contains(&("CONSTITUENTS", "external-references")),
            "{roles:?}"
        );
        assert!(roles.contains(&("STEP_DATA", "root-exchange")), "{roles:?}");
        assert!(!roles.iter().any(|(name, _)| *name == "SAB_DATA"));
    }
}

#[test]
fn two_parts_are_placed_side_by_side() {
    let result = decode_assembly(ASM_TWO);
    assert_valid(&result, "asm_two");
    let ir = result.ir();
    assert_eq!(ir.model.occurrences.len(), 2);
    assert_at(ir, "asm_box<1>", [0.0, 0.0, 0.0]);
    assert_at(ir, "asm_cylinder<1>", [40.0, 0.0, 0.0]);
    for occurrence in &ir.model.occurrences {
        assert_eq!(occurrence.suppressed, Some(false));
        assert_eq!(occurrence.visible, Some(true));
        assert!(matches!(
            definition(ir, occurrence).storage,
            Storage::File {
                source: Some(_),
                ..
            }
        ));
        assert_eq!(definition(ir, occurrence).bodies.len(), 1);
    }
    assert_eq!(ir.geometry.bodies.len(), 2);
    let root = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .unwrap();
    assert_eq!(root.kind, Some(ProductDefinitionKind::Assembly));
    assert_eq!(root.source_name.as_deref(), Some("asm_two"));
    assert_eq!(root.occurrences.len(), 2);
    assert!(root.bodies.is_empty());
    assert!(result.report().geometry_transferred());
    assert_eq!(
        count_loss(&result, AlibreLossCode::AssemblyMemberUnavailable),
        0
    );
}

#[test]
fn the_anchored_component_is_grounded() {
    let result = decode_assembly(ASM_TWO);
    let ir = result.ir();
    let [joint] = ir.model.assembly_joints.as_slice() else {
        panic!("one grounded joint, found {:?}", ir.model.assembly_joints);
    };
    let JointOperands::Grounded { connector, .. } = joint.operands() else {
        panic!("a grounded joint");
    };
    assert_eq!(
        connector.operand.container,
        cadmpeg_ir::products::OperandContainer::Occurrence {
            occurrence: named(ir, "asm_box<1>").id.clone(),
            occurrence_path: Vec::new(),
        }
    );
    let none = decode_assembly(ASM_REPEAT);
    assert!(none.ir().model.assembly_joints.is_empty());
}

#[test]
fn a_repeated_component_is_one_definition_with_two_occurrences() {
    let result = decode_assembly(ASM_REPEAT);
    assert_valid(&result, "asm_repeat");
    let ir = result.ir();
    assert_eq!(ir.model.occurrences.len(), 4);
    assert_at(ir, "asm_box<1>", [0.0, 0.0, 0.0]);
    assert_at(ir, "asm_box<2>", [0.0, 40.0, 10.0]);
    assert_at(ir, "asm_cylinder<1>", [-40.0, 0.0, 0.0]);
    assert_at(ir, "asm_cylinder<2>", [0.0, -40.0, 0.0]);
    assert_eq!(
        named(ir, "asm_box<1>").prototype,
        named(ir, "asm_box<2>").prototype
    );
    assert_eq!(
        named(ir, "asm_cylinder<1>").prototype,
        named(ir, "asm_cylinder<2>").prototype
    );
    assert_ne!(
        named(ir, "asm_box<1>").prototype,
        named(ir, "asm_cylinder<1>").prototype
    );
    // Two part definitions and one root: the box is decoded once.
    assert_eq!(ir.model.product_definitions.len(), 3);
    assert_eq!(ir.geometry.bodies.len(), 2);
}

#[test]
fn hidden_and_suppressed_state_follow_the_configuration() {
    let result = decode_assembly(ASM_REPEAT);
    let ir = result.ir();
    let state = |name: &str| {
        let occurrence = named(ir, name);
        (occurrence.visible, occurrence.suppressed)
    };
    assert_eq!(state("asm_box<1>"), (Some(true), Some(false)));
    assert_eq!(state("asm_box<2>"), (Some(true), Some(false)));
    assert_eq!(state("asm_cylinder<1>"), (Some(false), Some(false)));
    assert_eq!(state("asm_cylinder<2>"), (Some(true), Some(true)));
}

#[test]
fn a_subassembly_is_decoded_below_its_occurrence() {
    let result = decode_assembly(ASM_NESTED);
    assert_valid(&result, "asm_nested");
    let ir = result.ir();
    // The top assembly places two occurrences, the subassembly two more.
    assert_eq!(ir.model.occurrences.len(), 4);
    assert_at(ir, "asm_cylinder<1>", [0.0, 0.0, 0.0]);
    assert_at(ir, "asm_two<1>", [0.0, 0.0, 50.0]);
    let nested = definition(ir, named(ir, "asm_two<1>"));
    assert_eq!(nested.kind, Some(ProductDefinitionKind::Assembly));
    assert_eq!(nested.occurrences.len(), 2);
    assert!(matches!(
        nested.storage,
        Storage::File {
            source: Some(_),
            ..
        }
    ));
    let inner: Vec<_> = ir
        .model
        .occurrences
        .iter()
        .filter(|occurrence| nested.occurrences.contains(&occurrence.id))
        .collect();
    let box_inside = inner
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("asm_box<1>"))
        .unwrap();
    let cylinder_inside = inner
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("asm_cylinder<1>"))
        .unwrap();
    assert_eq!(translation(box_inside), [0.0, 0.0, 0.0]);
    assert_eq!(translation(cylinder_inside), [40.0, 0.0, 0.0]);
    // The cylinder of the top assembly and the cylinder of the subassembly
    // come from one file but are decoded in two scopes.
    assert_eq!(ir.geometry.bodies.len(), 3);
    assert!(result.report().geometry_transferred());
}

#[test]
fn the_assembled_boxes_match_the_step_exports() {
    // asm_two: the box and a cylinder 40 mm along x.
    let two = decode_assembly(ASM_TWO);
    let expected = ([-10.0, -15.0, 0.0], [50.0, 15.0, 15.0]);
    assert_extent(assembled_extent(two.ir(), |_| true), expected, "asm_two");
    assert_extent(twin_extent(TWO_TWIN, |_| true), expected, "asm_two twin");

    // asm_nested: the cylinder at the origin and asm_two lifted by 50 mm.
    let nested = decode_assembly(ASM_NESTED);
    let expected = ([-10.0, -15.0, 0.0], [50.0, 15.0, 65.0]);
    assert_extent(
        assembled_extent(nested.ir(), |_| true),
        expected,
        "asm_nested",
    );
    assert_extent(
        twin_extent(NESTED_TWIN, |_| true),
        expected,
        "asm_nested twin",
    );

    // asm_repeat: the export keeps the hidden cylinder and leaves out the
    // suppressed one.
    let repeat = decode_assembly(ASM_REPEAT);
    let expected = ([-50.0, -15.0, 0.0], [10.0, 55.0, 20.0]);
    assert_extent(
        assembled_extent(repeat.ir(), |occurrence| {
            occurrence.suppressed != Some(true)
        }),
        expected,
        "asm_repeat",
    );
    assert_extent(
        twin_extent(REPEAT_TWIN, |_| true),
        expected,
        "asm_repeat twin",
    );
}

#[test]
fn components_that_are_not_available_stay_external() {
    let result = decode_with(ASM_REPEAT, &Files(Vec::new()));
    assert_valid(&result, "asm_repeat without files");
    let ir = result.ir();
    assert_eq!(ir.model.occurrences.len(), 4);
    assert_at(ir, "asm_cylinder<2>", [0.0, -40.0, 0.0]);
    assert!(ir.geometry.bodies.is_empty());
    assert!(!result.report().geometry_transferred());
    // One loss per distinct component, not per occurrence.
    assert_eq!(
        count_loss(&result, AlibreLossCode::AssemblyMemberUnavailable),
        2
    );
    let box_definition = definition(ir, named(ir, "asm_box<1>"));
    let Storage::External(document) = &box_definition.storage else {
        panic!("an external definition");
    };
    assert!(document
        .path
        .as_ref()
        .is_some_and(|path| path.as_str().ends_with("/asm_box")));
    assert!(document.document_id.is_some());
}

#[test]
fn a_component_that_does_not_decode_stays_external() {
    let files = Files(vec![
        ("asm_box.AD_PRT", ASM_BOX),
        ("asm_cylinder.AD_PRT", &ASM_TWO[..64]),
    ]);
    let result = decode_with(ASM_TWO, &files);
    assert_valid(&result, "asm_two with a truncated component");
    assert_eq!(
        count_loss(&result, AlibreLossCode::AssemblyMemberUndecoded),
        1,
        "{:?}",
        result.report().losses
    );
    assert_eq!(result.ir().geometry.bodies.len(), 1);
}

#[test]
fn an_assembly_that_places_itself_is_cut_at_the_cycle() {
    // The file of the box request is the assembly that requests it.
    #[derive(Debug)]
    struct Loop;
    impl Companions for Loop {
        fn open(&self, _: Option<&str>, path: &str, _: u64) -> Result<Companion, String> {
            match path {
                "asm_box.AD_PRT" => Ok(Companion {
                    id: "loop".to_owned(),
                    bytes: ASM_TWO.to_vec(),
                }),
                "asm_cylinder.AD_PRT" => Ok(Companion {
                    id: "cylinder".to_owned(),
                    bytes: ASM_CYLINDER.to_vec(),
                }),
                _ => Err(format!("{path}: not found")),
            }
        }
    }
    let result = decode_with(ASM_TWO, &Loop);
    assert_valid(&result, "asm_two loop");
    assert_eq!(count_loss(&result, AlibreLossCode::AssemblyMemberCycle), 1);
}

#[test]
fn nesting_is_cut_at_the_limit() {
    // The root is not asm_two, whose bytes every box request serves: a
    // component equal to the root would be cut as a cycle at once.
    let result = decode_with(ASM_REPEAT, &Endless::new());
    assert_valid(&result, "asm_repeat endless");
    let truncated: Vec<_> = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == AlibreLossCode::AssemblyExpansionTruncated.kind())
        .map(|loss| loss.message.clone())
        .collect();
    // The box and the cylinder of the deepest assembly are both past the limit.
    assert_eq!(truncated.len(), 2, "{truncated:#?}");
}

#[test]
fn the_decode_is_deterministic_and_keeps_its_streams_in_the_loss_report() {
    let first = decode_assembly(ASM_TWO);
    let second = decode_assembly(ASM_TWO);
    assert_eq!(first.ir(), second.ir());
    assert!(loss_codes(&first).contains(&AlibreLossCode::SourceStreamsUndecoded.kind()));
    // The reader maps everything the specimens state: one configuration of
    // the default name leaves nothing to report.
    for code in [
        AlibreLossCode::AssemblyConfigurationUnmapped,
        AlibreLossCode::AssemblyRecordUnreadable,
        AlibreLossCode::AssemblyEntitiesUnread,
        AlibreLossCode::AssemblyInclusionUnmapped,
        AlibreLossCode::AssemblyPlacementUnreadable,
        AlibreLossCode::AssemblyNestedStateUnread,
    ] {
        assert_eq!(count_loss(&first, code), 0, "{code:?}");
    }
}

#[test]
fn a_loss_of_a_subassembly_names_its_path_once() {
    let result = decode_assembly(ASM_NESTED);
    let messages: Vec<_> = result
        .report()
        .losses
        .iter()
        .map(|loss| loss.message.as_str())
        .collect();
    assert!(
        messages
            .iter()
            .any(|message| message.starts_with("component asm_two/asm_box: ")),
        "{messages:#?}"
    );
    assert!(
        !messages
            .iter()
            .any(|message| message.contains("component asm_two: component ")),
        "{messages:#?}"
    );
}

/// `WorldTransform.Array()` of each occurrence of `asm_rotated`, from the
/// Alibre API: row-major 4 x 4 with the translation in the last row, so row
/// `i` is the image of local axis `i` and `point' = point * M`, in mm.
const ROTATED_BOX_1: [[f64; 4]; 4] = [
    [0.0, 1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [40.0, 0.0, 0.0, 1.0],
];
const ROTATED_CYLINDER_1: [[f64; 4]; 4] = [
    [0.0, 0.0, -1.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 40.0, 0.0, 1.0],
];
const ROTATED_BOX_2: [[f64; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 0.866_025, 0.5, 0.0],
    [0.0, -0.5, 0.866_025, 0.0],
    [0.0, 0.0, 40.0, 1.0],
];

/// Asserts the rotation to 1e-6 and the translation to 1e-6 mm. The IR
/// transform maps a column vector, so its column `c` is the image of local
/// axis `c`, which is row `c` of the API array.
fn assert_world(ir: &CadIr, name: &str, array: [[f64; 4]; 4]) {
    let rows = named(ir, name).transform.unwrap().affine_rows();
    for (r, row) in rows.iter().enumerate() {
        for c in 0..3 {
            assert!(
                (row[c] - array[c][r]).abs() < 1e-6,
                "{name}: rotation [{r}][{c}] is {}, the API states {}",
                row[c],
                array[c][r]
            );
        }
        assert!(
            (row[3] - array[3][r]).abs() < 1e-6,
            "{name}: translation {r} is {}, the API states {}",
            row[3],
            array[3][r]
        );
    }
}

#[test]
fn rotated_occurrences_carry_the_world_transform_of_the_api() {
    let result = decode_assembly(ASM_ROTATED);
    assert_valid(&result, "asm_rotated");
    let ir = result.ir();
    assert_eq!(ir.model.occurrences.len(), 3);
    assert_world(ir, "asm_box<1>", ROTATED_BOX_1);
    assert_world(ir, "asm_cylinder<1>", ROTATED_CYLINDER_1);
    assert_world(ir, "asm_box<2>", ROTATED_BOX_2);
    // The anchored box carries its rotated frame on its grounded joint.
    let [joint] = ir.model.assembly_joints.as_slice() else {
        panic!("one grounded joint, found {:?}", ir.model.assembly_joints);
    };
    let JointOperands::Grounded { connector, .. } = joint.operands() else {
        panic!("a grounded joint");
    };
    assert_eq!(Some(connector.frame), named(ir, "asm_box<1>").transform);
    assert_eq!(
        count_loss(&result, AlibreLossCode::AssemblyPlacementUnreadable),
        0
    );
}

#[test]
fn the_rotated_assembly_matches_the_step_export() {
    let result = decode_assembly(ASM_ROTATED);
    let actual = assembled_extent(result.ir(), |_| true);
    let twin = twin_extent(ROTATED_TWIN, |_| true);
    assert_extent(actual, twin, "asm_rotated against its twin");
    // The first box alone: x 25..55, y -10..10, z 0..10.
    let first = named(result.ir(), "asm_box<1>").id.clone();
    let alone = occurrence_extent(result.ir(), &first).expect("the box has geometry");
    assert_extent(
        alone,
        ([25.0, -10.0, 0.0], [55.0, 10.0, 10.0]),
        "asm_box<1>",
    );
}
