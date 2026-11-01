// SPDX-License-Identifier: Apache-2.0
//! Semantic PMI of the NIST MBE PMI STEP specimens against the PMI that the
//! NIST CTC and FTC definition workbooks in `corpus/nist-pmi/definitions/`
//! specify for each test case.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::Path;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::pmi::{
    DimensionKind, DimensionTolerance, GeometricToleranceKind, PmiAnnotation, PmiDefinition,
};
use cadmpeg_ir::CadIr;

use crate::loss::StepLossCode;
use crate::StepCodec;

/// Allowance of a decoded PMI value against the value that NIST specifies.
const NIST_VALUE_TOLERANCE: f64 = 1.0e-9;

/// Millimetres per inch; NIST specifies CTC-05 in inches.
const MM_PER_INCH: f64 = 25.4;

fn decode_specimen(name: &str) -> DecodeResult {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/nist-pmi/step")
        .join(name);
    let bytes =
        std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap_or_else(|error| panic!("decode {name}: {error}"))
}

#[derive(Debug, Clone, PartialEq)]
struct Dimension {
    kind: DimensionKind,
    nominal: Option<f64>,
    /// Signed lower and upper deviations.
    deviations: Option<(f64, f64)>,
}

impl Dimension {
    fn new(kind: DimensionKind, nominal: Option<f64>, deviations: Option<(f64, f64)>) -> Self {
        Self {
            kind,
            nominal,
            deviations,
        }
    }

    fn agrees(&self, other: &Self) -> bool {
        self.kind == other.kind
            && option_agrees(self.nominal, other.nominal, values_agree)
            && option_agrees(self.deviations, other.deviations, |a, b| {
                values_agree(a.0, b.0) && values_agree(a.1, b.1)
            })
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Tolerance {
    kind: GeometricToleranceKind,
    magnitude: f64,
    /// Datum-reference compartments in precedence order; a common datum
    /// joins its identifications with `-`.
    datums: Vec<String>,
    modifiers: Vec<String>,
}

impl Tolerance {
    fn new(kind: GeometricToleranceKind, magnitude: f64, datums: &[&str]) -> Self {
        Self {
            kind,
            magnitude,
            datums: datums.iter().map(|datum| (*datum).to_string()).collect(),
            modifiers: Vec::new(),
        }
    }

    fn agrees(&self, other: &Self) -> bool {
        self.kind == other.kind
            && values_agree(self.magnitude, other.magnitude)
            && self.datums == other.datums
            && self.modifiers == other.modifiers
    }
}

fn values_agree(left: f64, right: f64) -> bool {
    (left - right).abs() <= NIST_VALUE_TOLERANCE
}

fn option_agrees<T: Copy>(left: Option<T>, right: Option<T>, agree: impl Fn(T, T) -> bool) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => agree(left, right),
        (None, None) => true,
        _ => false,
    }
}

fn annotation<'a>(ir: &'a CadIr, id: &cadmpeg_ir::ids::PmiId) -> &'a PmiAnnotation {
    ir.model
        .pmi
        .iter()
        .find(|annotation| &annotation.id == id)
        .unwrap_or_else(|| panic!("annotation {}", id.as_str()))
}

fn dimensions(ir: &CadIr) -> Vec<Dimension> {
    ir.model
        .pmi
        .iter()
        .filter_map(|annotation| match &annotation.definition {
            PmiDefinition::Dimension(dimension) => Some(Dimension::new(
                dimension.kind().clone(),
                dimension.nominal().map(|value| value.value.get()),
                dimension.tolerance().map(|tolerance| match tolerance {
                    DimensionTolerance::PlusMinus { lower, upper } => {
                        (lower.value.get(), upper.value.get())
                    }
                    other => panic!("NIST CTC tolerance form {other:?}"),
                }),
            )),
            _ => None,
        })
        .collect()
}

fn datum_compartments(ir: &CadIr, system: &cadmpeg_ir::ids::PmiId) -> Vec<String> {
    let PmiDefinition::DatumSystem { references } = &annotation(ir, system).definition else {
        panic!("{} is a datum system", system.as_str());
    };
    let mut compartments = BTreeMap::<u32, Vec<String>>::new();
    for reference in references.as_slice() {
        let PmiDefinition::Datum { identification } = &annotation(ir, &reference.datum).definition
        else {
            panic!("{} is a datum", reference.datum.as_str());
        };
        let mut label = identification.clone();
        for modifier in &reference.modifiers {
            label.push('(');
            label.push_str(modifier);
            label.push(')');
        }
        compartments
            .entry(reference.precedence.get())
            .or_default()
            .push(label);
    }
    compartments
        .into_values()
        .map(|compartment| compartment.join("-"))
        .collect()
}

fn tolerances(ir: &CadIr) -> Vec<Tolerance> {
    ir.model
        .pmi
        .iter()
        .filter_map(|annotation| match &annotation.definition {
            PmiDefinition::GeometricTolerance {
                tolerance,
                magnitude,
                datum_system,
                modifiers,
                ..
            } => Some(Tolerance {
                kind: tolerance.clone(),
                magnitude: magnitude.get().value.get(),
                datums: datum_system
                    .as_ref()
                    .map(|system| datum_compartments(ir, system))
                    .unwrap_or_default(),
                modifiers: modifiers.clone(),
            }),
            _ => None,
        })
        .collect()
}

fn datums(ir: &CadIr) -> Vec<String> {
    let mut datums = ir
        .model
        .pmi
        .iter()
        .filter_map(|annotation| match &annotation.definition {
            PmiDefinition::Datum { identification } => Some(identification.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    datums.sort();
    datums
}

/// Matches each expected item to a distinct decoded item and returns the
/// decoded items that no expectation uses.
fn assert_each_present<T: std::fmt::Debug + Clone>(
    expected: &[T],
    decoded: &[T],
    agrees: impl Fn(&T, &T) -> bool,
) -> Vec<T> {
    let mut unused = decoded.to_vec();
    for item in expected {
        let index = unused
            .iter()
            .position(|candidate| agrees(item, candidate))
            .unwrap_or_else(|| {
                panic!("NIST expects {item:?}; unmatched decoded items: {unused:#?}")
            });
        unused.remove(index);
    }
    unused
}

/// NIST CTC-01 rows 3-12 of `NIST-CTC-PMI-Definitions.xlsx`. The limit
/// dimension of row 7 keeps its STEP nominal and loses its limits (open item
/// AP-19 in `docs/formats/step-open-items.md`).
#[test]
fn nist_ctc_01_semantic_pmi_matches_the_nist_definition() {
    use DimensionKind::{Angular, Diameter};
    use GeometricToleranceKind::{Flatness, Perpendicularity, Position, SurfaceProfile};

    let result = decode_specimen("nist_ctc_01_asme1_ap242-e1.stp");
    let ir = result.ir();

    assert_eq!(datums(ir), ["A", "B", "C"]);

    let expected_dimensions = [
        // Row 5: ⌀20 +0.05/-0.10 & ⌀20 +0.10/-0.05.
        Dimension::new(Diameter, Some(20.0), Some((-0.10, 0.05))),
        Dimension::new(Diameter, Some(20.0), Some((-0.05, 0.10))),
        // Row 6: ⌀25 ±0.15.
        Dimension::new(Diameter, Some(25.0), Some((-0.15, 0.15))),
        // Row 7: ⌀34.8 - 35.2 & ⌀35.2/34.8.
        Dimension::new(Diameter, Some(35.0), None),
        Dimension::new(Diameter, Some(35.0), None),
        // Row 8: ⌀35 0/-0.2 & ⌀35 +0.2/0.
        Dimension::new(Diameter, Some(35.0), Some((-0.2, 0.0))),
        Dimension::new(Diameter, Some(35.0), Some((0.0, 0.2))),
        // Row 9: 60° ±0.5°.
        Dimension::new(
            Angular,
            Some(60.0_f64.to_radians()),
            Some(((-0.5_f64).to_radians(), 0.5_f64.to_radians())),
        ),
        // Row 10: two dimensions without a dimension value.
        Dimension::new(Diameter, None, None),
        Dimension::new(Diameter, None, None),
    ];
    let unmatched = assert_each_present(&expected_dimensions, &dimensions(ir), Dimension::agrees);
    // The specimen adds two valueless `DIMENSIONAL_LOCATION` records that no
    // NIST row defines.
    assert!(
        unmatched
            .iter()
            .all(|dimension| dimension.kind == DimensionKind::Location),
        "dimensions outside the NIST definition: {unmatched:#?}"
    );
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == StepLossCode::DimensionalLimitsOmitted.kind())
            .count(),
        2
    );

    let expected_tolerances = [
        // Row 3: ⌓ | 1.25 | A | B | C, applied between A and B.
        Tolerance::new(SurfaceProfile, 1.25, &["A", "B", "C"]),
        // Row 4: ⌓ | 0.5 | A, applied all around.
        Tolerance::new(SurfaceProfile, 0.5, &["A"]),
        // Row 10: ⌖ | 0.75 | A | B | C, two examples.
        Tolerance::new(Position, 0.75, &["A", "B", "C"]),
        Tolerance::new(Position, 0.75, &["A", "B", "C"]),
        // Row 11: ▱ | 0.2.
        Tolerance::new(Flatness, 0.2, &[]),
        // Row 12: ⏊ | 1.5 | A.
        Tolerance::new(Perpendicularity, 1.5, &["A"]),
    ];
    let unmatched = assert_each_present(&expected_tolerances, &tolerances(ir), Tolerance::agrees);
    assert!(
        unmatched.is_empty(),
        "tolerances outside the NIST definition: {unmatched:#?}"
    );
}

/// NIST CTC-04 rows 33-42 of `NIST-CTC-PMI-Definitions.xlsx`. The rows that
/// specify a depth, a thread, or a dimension without a size or location
/// designation are not asserted. CADIR carries no projected tolerance zone, so
/// row 41 asserts the position tolerance without its projection.
#[test]
fn nist_ctc_04_semantic_pmi_matches_the_nist_definition() {
    use DimensionKind::{Angular, Diameter};
    use GeometricToleranceKind::{Position, SurfaceProfile};

    let result = decode_specimen("nist_ctc_04_asme1_ap242-e1.stp");
    let ir = result.ir();

    // Row 34 adds D-H to the A, B, and C that the frames reference.
    assert_eq!(datums(ir), ["A", "B", "C", "D", "E", "F", "G", "H"]);

    let expected_dimensions = [
        // Row 33: ⌀10 basic.
        Dimension::new(Diameter, Some(10.0), None),
        // Row 36: 30X ⌵∅20 ±0.3 X 90° ±1°.
        Dimension::new(Diameter, Some(20.0), Some((-0.3, 0.3))),
        Dimension::new(
            Angular,
            Some(90.0_f64.to_radians()),
            Some(((-1.0_f64).to_radians(), 1.0_f64.to_radians())),
        ),
        // Row 37: 30X ⌀14 ±0.1.
        Dimension::new(Diameter, Some(14.0), Some((-0.1, 0.1))),
        // Row 38: 4X ⌴⌀20 ±0.2.
        Dimension::new(Diameter, Some(20.0), Some((-0.2, 0.2))),
        // Row 42: 6X ⌀6.65 ±0.12.
        Dimension::new(Diameter, Some(6.65), Some((-0.12, 0.12))),
    ];
    assert_each_present(&expected_dimensions, &dimensions(ir), Dimension::agrees);

    let expected_tolerances = [
        // Row 35: ⌓ | 0.5 | A | B | C.
        Tolerance::new(SurfaceProfile, 0.5, &["A", "B", "C"]),
        // Row 39: composite ⌓ | 2 | D | G | H over 0.2 | D.
        Tolerance::new(SurfaceProfile, 2.0, &["D", "G", "H"]),
        Tolerance::new(SurfaceProfile, 0.2, &["D"]),
        // Row 40: ⌖ | ⌀ 0.35 | A | B | C.
        Tolerance::new(Position, 0.35, &["A", "B", "C"]),
        // Row 41: ⌖ | ⌀0.75Ⓟ50 | A | B | C.
        Tolerance::new(Position, 0.75, &["A", "B", "C"]),
        // Row 42: composite ⌖ | ⌀1.5 | D | E | F over ⌀0.3 | D | E.
        Tolerance::new(Position, 1.5, &["D", "E", "F"]),
        Tolerance::new(Position, 0.3, &["D", "E"]),
    ];
    let unmatched = assert_each_present(&expected_tolerances, &tolerances(ir), Tolerance::agrees);
    assert!(
        unmatched.is_empty(),
        "tolerances outside the NIST definition: {unmatched:#?}"
    );
}

/// NIST CTC-05 rows 43-52 of `NIST-CTC-PMI-Definitions.xlsx`, converted from
/// inches. Row 47 gives no size or location designation and is not asserted.
/// Row 51 specifies `◎ | ⌀.030 | B`, while the specimen's
/// `CONCENTRICITY_TOLERANCE` references the datum system of datum A, so row 51
/// is not asserted. The specimen also holds two perpendicularity tolerances
/// that no CTC-05 row defines.
#[test]
fn nist_ctc_05_semantic_pmi_matches_the_nist_definition() {
    use GeometricToleranceKind::{CircularRunout, Roundness, Straightness, TotalRunout};

    let result = decode_specimen("nist_ctc_05_asme1_ap242-e1.stp");
    let ir = result.ir();

    // Rows 43-46: A, A-B, B, and the datum targets C1 and D1.
    assert_eq!(datums(ir), ["A", "B", "C", "D"]);

    let expected_tolerances = [
        // Row 48: − | .005.
        Tolerance::new(Straightness, 0.005 * MM_PER_INCH, &[]),
        // Row 49: ⌰ | .002 | A and ⌰ | .015 | B.
        Tolerance::new(TotalRunout, 0.002 * MM_PER_INCH, &["A"]),
        Tolerance::new(TotalRunout, 0.015 * MM_PER_INCH, &["B"]),
        // Row 50: ○ | .002.
        Tolerance::new(Roundness, 0.002 * MM_PER_INCH, &[]),
        // Row 52: ↗ | .035 | A-B and ↗ | .025 | A-B.
        Tolerance::new(CircularRunout, 0.035 * MM_PER_INCH, &["A-B"]),
        Tolerance::new(CircularRunout, 0.025 * MM_PER_INCH, &["A-B"]),
    ];
    assert_each_present(&expected_tolerances, &tolerances(ir), Tolerance::agrees);
}
