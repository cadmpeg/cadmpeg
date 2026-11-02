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

#[derive(Debug, Clone, Copy, PartialEq)]
enum Range {
    /// Signed lower and upper deviations from the nominal.
    Deviations(f64, f64),
    /// Lower and upper limit values.
    Limits(f64, f64),
}

impl Range {
    fn agrees(self, other: Self) -> bool {
        match (self, other) {
            (Self::Deviations(a, b), Self::Deviations(c, d))
            | (Self::Limits(a, b), Self::Limits(c, d)) => values_agree(a, c) && values_agree(b, d),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Dimension {
    kind: DimensionKind,
    nominal: Option<f64>,
    range: Option<Range>,
    modifiers: Vec<String>,
}

impl Dimension {
    fn new(kind: DimensionKind, nominal: Option<f64>, deviations: Option<(f64, f64)>) -> Self {
        Self {
            kind,
            nominal,
            range: deviations.map(|(lower, upper)| Range::Deviations(lower, upper)),
            modifiers: Vec::new(),
        }
    }

    fn limits(kind: DimensionKind, nominal: Option<f64>, lower: f64, upper: f64) -> Self {
        Self {
            kind,
            nominal,
            range: Some(Range::Limits(lower, upper)),
            modifiers: Vec::new(),
        }
    }

    fn modified(mut self, modifiers: &[&str]) -> Self {
        self.modifiers = modifiers.iter().map(|value| (*value).to_owned()).collect();
        self
    }

    fn agrees(&self, other: &Self) -> bool {
        self.kind == other.kind
            && option_agrees(self.nominal, other.nominal, values_agree)
            && option_agrees(self.range, other.range, Range::agrees)
            && self.modifiers == other.modifiers
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

    fn modified(mut self, modifiers: &[&str]) -> Self {
        self.modifiers = modifiers.iter().map(|value| (*value).to_owned()).collect();
        self
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
            PmiDefinition::Dimension(dimension) => Some(Dimension {
                kind: dimension.kind().clone(),
                modifiers: dimension.modifiers().to_vec(),
                nominal: dimension.nominal().map(|value| value.value.get()),
                range: dimension.tolerance().map(|tolerance| match tolerance {
                    DimensionTolerance::PlusMinus { lower, upper } => {
                        Range::Deviations(lower.value.get(), upper.value.get())
                    }
                    DimensionTolerance::Limits { lower, upper } => {
                        Range::Limits(lower.value.get(), upper.value.get())
                    }
                    other => panic!("NIST CTC tolerance form {other:?}"),
                }),
            }),
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

/// NIST CTC-01 rows 3-12 of `NIST-CTC-PMI-Definitions.xlsx`.
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
        // Row 7: ⌀34.8 - 35.2 & ⌀35.2/34.8. NIST states no nominal; the
        // specimen's `nominal value` item is 35.
        Dimension::limits(Diameter, Some(35.0), 34.8, 35.2),
        Dimension::limits(Diameter, Some(35.0), 34.8, 35.2),
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
/// specify depth values are dimensional locations. Row 41 also asserts its
/// nominal thread diameter, depth, and projected zone length.
#[test]
fn nist_ctc_04_semantic_pmi_matches_the_nist_definition() {
    use DimensionKind::{Angular, Diameter, Location};
    use GeometricToleranceKind::{Position, SurfaceProfile};

    let result = decode_specimen("nist_ctc_04_asme1_ap242-e1.stp");
    let ir = result.ir();

    let projected = ir
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.id.as_str() == "step:presentation:pmi#18891")
        .expect("projected position tolerance");
    let PmiDefinition::GeometricTolerance { constraints, .. } = &projected.definition else {
        panic!("geometric tolerance")
    };
    let [zone] = constraints.zones.as_slice() else {
        panic!("one projected zone")
    };
    assert_eq!(zone.form.as_str(), "cylindrical or circular");
    let [projection] = zone.projections.as_slice() else {
        panic!("one projection")
    };
    assert!((projection.length.get() - 50.0).abs() < NIST_VALUE_TOLERANCE);
    assert!(projection.offset.is_none());
    assert!(projection.boundaries.is_empty());

    // Row 34 adds D-H to the A, B, and C that the frames reference.
    assert_eq!(datums(ir), ["A", "B", "C", "D", "E", "F", "G", "H"]);

    let expected_dimensions = [
        Dimension::new(Location, Some(75.0), None).modified(&["theoretical"]),
        Dimension::new(Location, Some(20.0), Some((-0.2, 0.2))),
        Dimension::new(Location, Some(25.0), Some((-0.25, 0.25))),
        Dimension::new(Diameter, Some(12.0), None),
        // Row 33: ⌀10 basic.
        Dimension::new(Diameter, Some(10.0), None).modified(&["theoretical"]),
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
/// inches. Row 47 is a dimensional location with a statistical modifier.
/// Row 51 specifies `◎ | ⌀.030 | B`, while the specimen's
/// `CONCENTRICITY_TOLERANCE` references the datum system of datum A, so row 51
/// is not asserted. The specimen also holds two perpendicularity tolerances
/// that no CTC-05 row defines.
#[test]
fn nist_ctc_05_semantic_pmi_matches_the_nist_definition() {
    use GeometricToleranceKind::{CircularRunout, Roundness, Straightness, TotalRunout};

    let result = decode_specimen("nist_ctc_05_asme1_ap242-e1.stp");
    let ir = result.ir();

    assert_each_present(
        &[
            Dimension::new(
                DimensionKind::Location,
                Some(5.0 * MM_PER_INCH),
                Some((-0.008 * MM_PER_INCH, 0.008 * MM_PER_INCH)),
            )
            .modified(&["statistical"]),
            Dimension::new(DimensionKind::Location, Some(1.25 * MM_PER_INCH), None)
                .modified(&["theoretical"]),
            Dimension::new(DimensionKind::Location, Some(1.25 * MM_PER_INCH), None)
                .modified(&["theoretical"]),
            Dimension::new(DimensionKind::Location, Some(2.0 * MM_PER_INCH), None)
                .modified(&["theoretical"]),
            Dimension::new(DimensionKind::Location, Some(2.0 * MM_PER_INCH), None)
                .modified(&["theoretical"]),
        ],
        &dimensions(ir),
        Dimension::agrees,
    );

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

#[test]
fn nist_ctc_02_datum_material_conditions_preserve_the_serialized_pmi() {
    use GeometricToleranceKind::{LineProfile, Position, SurfaceProfile};
    let result = decode_specimen("nist_ctc_02_asme1_ap242-e2.stp");
    for (source_id, displacement) in [(56, 0.0), (57, 0.5), (58, 2.5)] {
        let annotation = result
            .ir()
            .model
            .pmi
            .iter()
            .find(|annotation| {
                annotation.id.as_str() == format!("step:presentation:pmi#{source_id}")
            })
            .expect("source tolerance");
        let PmiDefinition::GeometricTolerance { constraints, .. } = &annotation.definition else {
            panic!("geometric tolerance")
        };
        assert!(
            (constraints
                .displacement
                .expect("unequal displacement")
                .get()
                - displacement)
                .abs()
                < NIST_VALUE_TOLERANCE
        );
    }
    let expected = [
        Tolerance::new(LineProfile, 0.25, &["A", "B", "C"]),
        Tolerance::new(SurfaceProfile, 0.75, &["A", "B", "C"]),
        Tolerance::new(
            SurfaceProfile,
            0.5,
            &[
                "D",
                "E(maximum_material_requirement)",
                "F(maximum_material_requirement)",
            ],
        ),
        Tolerance::new(
            SurfaceProfile,
            0.5,
            &["D", "E(maximum_material_requirement)", "F"],
        ),
        Tolerance::new(
            SurfaceProfile,
            0.5,
            &["D", "E", "F(maximum_material_requirement)"],
        ),
        Tolerance::new(SurfaceProfile, 0.5, &["D", "E", "F"]),
        Tolerance::new(
            SurfaceProfile,
            0.8,
            &["G", "H(least_material_requirement)", "J"],
        ),
        Tolerance::new(
            SurfaceProfile,
            0.8,
            &[
                "G",
                "H(least_material_requirement)",
                "J(least_material_requirement)",
            ],
        ),
        Tolerance::new(
            SurfaceProfile,
            0.8,
            &["G", "H", "J(least_material_requirement)"],
        ),
        Tolerance::new(SurfaceProfile, 0.8, &["G", "H", "J"]),
        // Workbook row 21 specifies 1.2; source #184 references #355,
        // whose MEASURE_WITH_UNIT stores LENGTH_MEASURE(1.).
        Tolerance::new(Position, 1.0, &["A", "B", "C"]).modified(&["least_material_requirement"]),
        Tolerance::new(Position, 1.5, &["A", "B", "C"]).modified(&["maximum_material_requirement"]),
    ];
    assert_each_present(&expected, &tolerances(result.ir()), Tolerance::agrees);
}

#[test]
fn nist_ctc_03_inch_dimensions_and_tolerances_match_the_nist_definition() {
    use DimensionKind::{Diameter, Size};
    use GeometricToleranceKind::{
        Angularity, Flatness, Perpendicularity, Position, SurfaceProfile,
    };
    let result = decode_specimen("nist_ctc_03_asme1_ap242-e2.stp");
    let ir = result.ir();
    assert_eq!(datums(ir), ["A", "B", "C", "D", "E", "F"]);
    let expected_dimensions = [
        Dimension::new(DimensionKind::Location, Some(0.75 * MM_PER_INCH), None)
            .modified(&["auxiliary"]),
        Dimension::new(
            Diameter,
            Some(0.438 * MM_PER_INCH),
            Some((-0.005 * MM_PER_INCH, 0.005 * MM_PER_INCH)),
        ),
        Dimension::new(
            Diameter,
            Some(0.438 * MM_PER_INCH),
            Some((-0.005 * MM_PER_INCH, 0.005 * MM_PER_INCH)),
        ),
        Dimension::new(
            Diameter,
            Some(1.065 * MM_PER_INCH),
            Some((-0.003 * MM_PER_INCH, 0.003 * MM_PER_INCH)),
        ),
        Dimension::new(
            Diameter,
            Some(0.625 * MM_PER_INCH),
            Some((-0.005 * MM_PER_INCH, 0.005 * MM_PER_INCH)),
        ),
        Dimension::new(
            Size,
            Some(0.82 * MM_PER_INCH),
            Some((-0.06 * MM_PER_INCH, 0.06 * MM_PER_INCH)),
        ),
    ];
    assert_each_present(&expected_dimensions, &dimensions(ir), Dimension::agrees);
    let expected_tolerances = [
        Tolerance::new(SurfaceProfile, 0.01 * MM_PER_INCH, &[]),
        Tolerance::new(SurfaceProfile, 0.03 * MM_PER_INCH, &["A", "B", "C"]),
        Tolerance::new(SurfaceProfile, 0.06 * MM_PER_INCH, &["A", "B", "C"]),
        Tolerance::new(Perpendicularity, 0.01 * MM_PER_INCH, &["A"]),
        Tolerance::new(Perpendicularity, 0.01 * MM_PER_INCH, &["E"]),
        Tolerance::new(Position, 0.02 * MM_PER_INCH, &["A", "B"]),
        Tolerance::new(Position, 0.05 * MM_PER_INCH, &["D", "B", "C"])
            .modified(&["maximum_material_requirement"]),
        Tolerance::new(Angularity, 0.04 * MM_PER_INCH, &["A"]),
        Tolerance::new(Position, 0.03 * MM_PER_INCH, &["D", "B", "C"]),
        Tolerance::new(
            Position,
            0.05 * MM_PER_INCH,
            &[
                "A",
                "B(maximum_material_requirement)",
                "C(maximum_material_requirement)",
            ],
        )
        .modified(&["maximum_material_requirement"]),
        Tolerance::new(Position, 0.06 * MM_PER_INCH, &["D", "B", "C"]),
        Tolerance::new(Flatness, 0.005 * MM_PER_INCH, &[]),
    ];
    assert_each_present(&expected_tolerances, &tolerances(ir), Tolerance::agrees);
}

#[test]
fn nist_ftc_06_preserves_the_spherical_position_tolerance_zone() {
    let result = decode_specimen("nist_ftc_06_asme1_ap242-e2.stp");
    let dimensions = dimensions(result.ir());
    assert_each_present(
        &[
            Dimension::new(
                DimensionKind::SphericalDiameter,
                Some(1.25 * MM_PER_INCH),
                Some((-0.008 * MM_PER_INCH, 0.008 * MM_PER_INCH)),
            ),
            Dimension::new(
                DimensionKind::SphericalRadius,
                Some(0.5 * MM_PER_INCH),
                None,
            )
            .modified(&["auxiliary"]),
            Dimension::new(
                DimensionKind::Radius,
                Some(0.5 * MM_PER_INCH),
                Some((-0.02 * MM_PER_INCH, 0.02 * MM_PER_INCH)),
            )
            .modified(&["controlled radius"]),
            Dimension::new(
                DimensionKind::Diameter,
                Some(0.415 * MM_PER_INCH),
                Some((-0.008 * MM_PER_INCH, 0.008 * MM_PER_INCH)),
            ),
            Dimension::new(
                DimensionKind::Diameter,
                Some(0.625 * MM_PER_INCH),
                Some((-0.02 * MM_PER_INCH, 0.02 * MM_PER_INCH)),
            ),
            Dimension::new(
                DimensionKind::Diameter,
                Some(0.5625 * MM_PER_INCH),
                Some((-0.008 * MM_PER_INCH, 0.008 * MM_PER_INCH)),
            ),
            Dimension::new(
                DimensionKind::Diameter,
                Some(0.8125 * MM_PER_INCH),
                Some((-0.02 * MM_PER_INCH, 0.02 * MM_PER_INCH)),
            ),
            Dimension::new(
                DimensionKind::Diameter,
                Some(0.281 * MM_PER_INCH),
                Some((-0.008 * MM_PER_INCH, 0.008 * MM_PER_INCH)),
            ),
            Dimension::new(
                DimensionKind::Diameter,
                Some(0.4 * MM_PER_INCH),
                Some((-0.01 * MM_PER_INCH, 0.01 * MM_PER_INCH)),
            ),
        ],
        &dimensions,
        Dimension::agrees,
    );
    let tolerance = result
        .ir()
        .model
        .pmi
        .iter()
        .find(|annotation| annotation.id.as_str() == "step:presentation:pmi#135")
        .expect("spherical position tolerance");
    let PmiDefinition::GeometricTolerance {
        constraints,
        magnitude,
        tolerance: kind,
        ..
    } = &tolerance.definition
    else {
        panic!("geometric tolerance")
    };
    assert_eq!(*kind, GeometricToleranceKind::Position);
    assert!((magnitude.get().value.get() - 0.025 * MM_PER_INCH).abs() < NIST_VALUE_TOLERANCE);
    let [zone] = constraints.zones.as_slice() else {
        panic!("one spherical zone")
    };
    assert_eq!(zone.form.as_str(), "spherical");
}

#[test]
fn nist_ftc_07_profile_and_statistical_tolerances_match_the_nist_definition() {
    use GeometricToleranceKind::{Flatness, Parallelism, Position, SurfaceProfile};
    let result = decode_specimen("nist_ftc_07_asme1_ap242-e2.stp");
    let expected = [
        Tolerance::new(Parallelism, 0.03 * MM_PER_INCH, &["A"]),
        Tolerance::new(Position, 0.04 * MM_PER_INCH, &["A", "B", "C"])
            .modified(&["maximum_material_requirement"]),
        Tolerance::new(Position, 0.05 * MM_PER_INCH, &["A", "B", "C"])
            .modified(&["statistical_tolerance"]),
        Tolerance::new(Flatness, 0.01 * MM_PER_INCH, &[]),
        Tolerance::new(SurfaceProfile, 0.06 * MM_PER_INCH, &["A", "B", "C"]),
        Tolerance::new(SurfaceProfile, 0.04 * MM_PER_INCH, &["A", "B", "C"]),
        Tolerance::new(SurfaceProfile, 0.01 * MM_PER_INCH, &["A"]),
        Tolerance::new(SurfaceProfile, 0.07 * MM_PER_INCH, &["A", "B", "C"]),
        Tolerance::new(SurfaceProfile, 0.015 * MM_PER_INCH, &["A"]),
    ];
    assert_each_present(&expected, &tolerances(result.ir()), Tolerance::agrees);
    let flatness = result
        .ir()
        .model
        .pmi
        .iter()
        .find_map(|annotation| match &annotation.definition {
            PmiDefinition::GeometricTolerance {
                tolerance: Flatness,
                defined_unit,
                defined_area_unit,
                ..
            } => Some((defined_unit, defined_area_unit)),
            _ => None,
        })
        .expect("unit-basis flatness");
    assert!(
        (flatness.0.expect("unit diameter").value.get() - MM_PER_INCH).abs() < NIST_VALUE_TOLERANCE
    );
    assert_eq!(flatness.1.as_deref(), Some("circular"));
}

#[test]
fn nist_ftc_10_fit_and_zero_material_tolerances_match_the_nist_definition() {
    use GeometricToleranceKind::{Cylindricity, Position, Straightness, Symmetry};
    let result = decode_specimen("nist_ftc_10_asme1_ap242-e2.stp");
    let mut fits = 0;
    for annotation in &result.ir().model.pmi {
        if let PmiDefinition::Dimension(dimension) = &annotation.definition {
            if let Some(DimensionTolerance::Fit { fit }) = dimension.tolerance() {
                assert_eq!(dimension.kind(), &DimensionKind::Diameter);
                assert!(
                    (dimension.nominal().expect("fit nominal").value.get() - 3.5).abs()
                        < NIST_VALUE_TOLERANCE
                );
                assert_eq!(fit.form_variance, "G6");
                assert_eq!(fit.zone_variance, "hole");
                fits += 1;
            }
        }
    }
    assert_eq!(fits, 3);
    assert_each_present(&[
        Tolerance::new(Cylindricity, 0.1, &[]),
        Tolerance::new(Symmetry, 0.5, &["A", "J"]),
        Tolerance::new(Straightness, 0.2, &[]),
        Tolerance::new(Position, 0.0, &["A", "B", "C"]).modified(&["maximum_material_requirement"]),
        Tolerance::new(Position, 0.0, &["H", "K", "L"]).modified(&["least_material_requirement"]),
        Tolerance::new(Position, 0.8, &["D", "E(maximum_material_requirement)-F(maximum_material_requirement)-G(maximum_material_requirement)"]),
    ], &tolerances(result.ir()), Tolerance::agrees);
}

#[test]
fn every_nist_step_preserves_its_source_faces_and_dimensions() {
    use cadmpeg_ir::pmi::PmiDefinition;
    for name in [
        "nist_ctc_01_asme1_ap242-e1.stp",
        "nist_ctc_02_asme1_ap242-e2.stp",
        "nist_ctc_03_asme1_ap242-e2.stp",
        "nist_ctc_04_asme1_ap242-e1.stp",
        "nist_ctc_05_asme1_ap242-e1.stp",
        "nist_ftc_06_asme1_ap242-e2.stp",
        "nist_ftc_07_asme1_ap242-e2.stp",
        "nist_ftc_10_asme1_ap242-e2.stp",
        "nist_ftc_09_asme1_rd.stp",
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/nist-pmi/step")
            .join(name);
        let bytes = std::fs::read(path).expect("NIST source");
        let (exchange, _) = crate::test_support::with_service_context(&bytes, crate::parse::parse)
            .expect("parse NIST source");
        let result = decode_specimen(name);
        let faces = exchange
            .entities_any(&["ADVANCED_FACE", "FACE_SURFACE"])
            .count();
        assert!(faces > 0, "{name} contains no bounded faces");
        assert_eq!(
            result.ir().model.faces.len(),
            faces,
            "{name} lost source faces"
        );
        let source_dimensions = exchange
            .entities_any(&[
                "DIMENSIONAL_SIZE",
                "DIMENSIONAL_LOCATION",
                "ANGULAR_SIZE",
                "ANGULAR_LOCATION",
                "DIMENSIONAL_SIZE_WITH_DATUM_FEATURE",
                "DIMENSIONAL_LOCATION_WITH_DATUM_FEATURE",
            ])
            .count();
        let decoded_dimensions = result
            .ir()
            .model
            .pmi
            .iter()
            .filter(|annotation| matches!(annotation.definition, PmiDefinition::Dimension(_)))
            .count();
        assert_eq!(
            decoded_dimensions, source_dimensions,
            "{name} lost dimensional characteristics"
        );
    }
}
