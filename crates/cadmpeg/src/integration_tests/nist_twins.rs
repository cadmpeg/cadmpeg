// SPDX-License-Identifier: Apache-2.0
//! Solid geometry agreement between the curated native and STEP designs.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::topology::BodyKind;
use cadmpeg_ir::{CadIr, DecodeOptions};
use cadmpeg_measure::compare::{compare, CompareOptions};
use cadmpeg_measure::report::{BodyMeasures, MeasureReport, ShellRole};
use cadmpeg_measure::value::{Quantity, Scalar};
use cadmpeg_measure::{MeasureInput, MeasureOptions, Measurer, Selection, SurfaceKind};

/// The same relative floor as `diff --geometry`, for document quantities.
const DESIGN_RELATIVE_FLOOR: f64 = 1.0e-9;
/// The same floor for sums of face areas within each surface family.
const FAMILY_AREA_RELATIVE_FLOOR: f64 = 1.0e-9;

struct Design {
    ir: CadIr,
    bodies: MeasureReport,
    families: BTreeMap<SurfaceKind, AreaSum>,
    confirmed_faces: usize,
    measured_faces: usize,
}

#[derive(Debug, Default)]
struct AreaSum {
    value: f64,
    error: f64,
    unavailable: Vec<&'static str>,
}

impl AreaSum {
    fn add(&mut self, area: &Quantity<Scalar>) {
        match area {
            Quantity::Measured(area) => {
                self.value += area.value().value.get();
                self.error += area.numeric_error().get() + area.model_uncertainty().get();
            }
            Quantity::Unavailable(area) => self.unavailable.push(area.reason().code()),
        }
    }
}

impl Design {
    fn measure(ir: CadIr) -> Self {
        let solids = ir
            .model
            .bodies
            .iter()
            .filter(|body| body.kind == BodyKind::Solid)
            .map(|body| body.id.clone())
            .collect::<Vec<_>>();
        assert!(!solids.is_empty(), "the design must transfer solid bodies");
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
        let measurer = Measurer::new(
            MeasureInput {
                ir: &ir,
                fidelity: None,
                geometry_transferred: true,
            },
            MeasureOptions::default(),
        );
        let mut selection = Selection {
            bodies: solids,
            ..Selection::default()
        };
        let bodies = measurer.measure(&ctx, &selection).expect("body measures");
        selection.all_faces = true;
        let faces = measurer.measure(&ctx, &selection).expect("face measures");
        ctx.finish_session().expect("measurement session");
        let mut families: BTreeMap<SurfaceKind, AreaSum> = BTreeMap::new();
        let mut confirmed_faces = 0;
        for face in faces.faces() {
            families
                .entry(*face.carrier().kind())
                .or_default()
                .add(face.measures().area());
            let orientation = face.orientation();
            if orientation.consistent() == &Some(true)
                && orientation.witness().as_ref().is_none_or(|w| *w.agrees())
            {
                confirmed_faces += 1;
            }
        }
        Self {
            ir,
            bodies,
            families,
            confirmed_faces,
            measured_faces: faces.faces().len(),
        }
    }

    fn closed_shells(&self) -> usize {
        self.bodies
            .bodies()
            .iter()
            .flat_map(BodyMeasures::shells)
            .filter(|shell| *shell.closed())
            .count()
    }

    fn complete(&self) -> bool {
        self.measured_faces > 0
            && self.confirmed_faces == self.measured_faces
            && self.bodies.bodies().iter().all(|body| {
                !body.shells().is_empty()
                    && body.shells().iter().all(|shell| {
                        *shell.closed()
                            && shell.volume().as_ref().is_some_and(|volume| {
                                volume.measured().is_some_and(|volume| {
                                    let signed = volume.value().value.get();
                                    match shell.role() {
                                        ShellRole::Outer => signed > 0.0,
                                        ShellRole::Void => signed < 0.0,
                                    }
                                })
                            })
                    })
                    && body.orientation().consistent() == &Some(true)
                    && body.orientation().unconfirmed().is_empty()
                    && body.orientation().conflict().is_empty()
                    && body
                        .volume()
                        .as_ref()
                        .is_some_and(|v| v.measured().is_some_and(|v| v.value().value.get() > 0.0))
                    && body.area().as_ref().is_some_and(|a| a.measured().is_some())
                    && body.bbox().measured().is_some()
            })
    }
}

fn load(relative: &str) -> Design {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/nist-pmi")
        .join(relative);
    let loaded = crate::loader::load_artifact(
        &cadmpeg_registry::InputCatalog::with_builtins(),
        &path,
        DecodeOptions::default(),
        None,
    )
    .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    assert!(loaded.decode_report().unwrap().geometry_transferred());
    Design::measure(loaded.ir)
}

fn diagonal(design: &Design) -> f64 {
    let bbox = design.bodies.document().bbox().as_ref().unwrap();
    bbox.measured().map_or(0.0, |bbox| {
        let low = bbox.value().min.get();
        let high = bbox.value().max.get();
        low.distance(high)
    })
}

fn agreement(native: &Design, step: &Design) -> Vec<String> {
    let mut failures = Vec::new();
    for (label, design) in [("native", native), ("STEP", step)] {
        println!(
            "{label}: confirmed orientation {}/{}; area {:?}; signed volume {:?}; bbox {:?}",
            design.confirmed_faces,
            design.measured_faces,
            design.bodies.document().area(),
            design.bodies.document().closed_volume(),
            design.bodies.document().bbox(),
        );
        if !design.complete() {
            failures.push(format!(
                "{label}: solid closure, signed volume or orientation incomplete"
            ));
        }
    }
    println!(
        "closed shells: {} / {}",
        native.closed_shells(),
        step.closed_shells()
    );
    if native.closed_shells() != step.closed_shells() {
        failures.push("closed-shell count".to_owned());
    }
    let comparison = compare(
        &native.ir,
        &native.bodies,
        &step.ir,
        &step.bodies,
        &CompareOptions::new(DESIGN_RELATIVE_FLOOR).unwrap(),
    )
    .expect("comparison allowance");
    for name in ["area", "closed_volume", "bbox"] {
        let rows: Vec<_> = comparison
            .document()
            .iter()
            .filter(|row| row.quantity() == name || row.quantity().starts_with(&format!("{name}.")))
            .collect();
        if rows.is_empty() || rows.iter().any(|row| row.agrees() != Some(true)) {
            failures.push(name.to_owned());
        }
        for row in rows {
            println!("{row:?}");
        }
    }
    let scale = diagonal(native).max(diagonal(step));
    let kinds: BTreeSet<_> = native.families.keys().chain(step.families.keys()).collect();
    for kind in kinds {
        let zero = AreaSum::default();
        let a = native.families.get(kind).unwrap_or(&zero);
        let b = step.families.get(kind).unwrap_or(&zero);
        let tolerance = a.error
            + b.error
            + FAMILY_AREA_RELATIVE_FLOOR * a.value.abs().max(b.value.abs()).max(scale.powi(2));
        println!(
            "area.{}: {a:?} / {b:?}; tolerance {tolerance}",
            kind.as_str()
        );
        if !a.unavailable.is_empty()
            || !b.unavailable.is_empty()
            || (a.value - b.value).abs() > tolerance
        {
            failures.push(format!("area.{}", kind.as_str()));
        }
    }
    for (label, design) in [("native", native), ("STEP", step)] {
        for body in design.bodies.bodies() {
            println!(
                "{label}: orientation {:?}; shells {:?}",
                body.orientation(),
                body.shells()
            );
        }
    }
    failures
}

fn twin(native: &str, step: &str) {
    let native = load(native);
    let step = load(&format!("step/{step}"));
    let failures = agreement(&native, &step);
    assert!(
        failures.is_empty(),
        "twin disagreements: {}",
        failures.join(", ")
    );
}

#[test]
#[cfg(feature = "sldprt")]
#[ignore = "FTC-06 has one unconfirmed face orientation; signed volume is unavailable"]
fn nist_twin_solidworks_ftc_06() {
    twin(
        "sldprt/nist_ftc_06_asme1_rd_sw1802.SLDPRT",
        "nist_ftc_06_asme1_ap242-e2.stp",
    );
}

#[test]
#[cfg(feature = "nx")]
fn nist_twin_nx_ctc_03() {
    twin(
        "nx/nist_ctc_03_asme1_nx1980_rc.prt",
        "nist_ctc_03_asme1_ap242-e2.stp",
    );
}

#[test]
#[cfg(feature = "catia")]
#[ignore = "FTC-07 has inconsistent orientations and boundary.pcurve-curve-mismatch; torus area differs from STEP"]
fn nist_twin_catia_ftc_07() {
    twin(
        "catia/nist_ftc_07_asme1_rd_ct5290_ra.CATPart",
        "nist_ftc_07_asme1_ap242-e2.stp",
    );
}

#[test]
#[cfg(feature = "creo")]
#[ignore = "native CTC-01 has a 2 mm blind cut; total area differs from STEP"]
fn nist_twin_creo_ctc_01() {
    twin(
        "creo/nist_ctc_01_asme1_rd_cr10030.prt.5",
        "nist_ctc_01_asme1_ap242-e1.stp",
    );
}

#[test]
#[cfg(feature = "inventor")]
#[ignore = "CTC-01 total area differs; native box is shifted by 400 mm in X and 225 mm in Y"]
fn nist_twin_inventor_ctc_01() {
    twin(
        "inventor/INV_nist_ctc_01_asme1_2021.ipt",
        "nist_ctc_01_asme1_ap242-e1.stp",
    );
}

#[test]
fn nist_twin_check_accepts_equal_geometry_across_face_partitions() {
    let a = Design::measure(cadmpeg_ir::examples::unit_cube().unwrap());
    let split = cadmpeg_test_support::polyhedron::polyhedron(
        &[
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 10.0, 0.0],
            [0.0, 10.0, 0.0],
            [0.0, 0.0, 10.0],
            [10.0, 0.0, 10.0],
            [10.0, 10.0, 10.0],
            [0.0, 10.0, 10.0],
        ],
        &[
            &[0, 3, 2, 1],
            &[4, 5, 6],
            &[4, 6, 7],
            &[0, 1, 5, 4],
            &[1, 2, 6, 5],
            &[2, 3, 7, 6],
            &[3, 0, 4, 7],
        ],
    )
    .unwrap();
    let b = Design::measure(split);
    assert_ne!(a.measured_faces, b.measured_faces);
    assert!(agreement(&a, &b).is_empty());
}

#[test]
fn nist_twin_check_rejects_reversed_face_senses() {
    let cube = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut broken = cube.clone();
    for face in &mut broken.model.faces {
        face.sense = cadmpeg_ir::topology::Sense::Reversed;
    }
    let a = Design::measure(cube);
    let b = Design::measure(broken);
    assert!(!b.complete());
    assert!(!agreement(&a, &b).is_empty());

    let mut one_reversed = a.ir.clone();
    one_reversed.model.faces[0].sense = cadmpeg_ir::topology::Sense::Reversed;
    let b = Design::measure(one_reversed);
    assert!(!b.complete());
    assert!(!agreement(&a, &b).is_empty());

    let mut inward = a.ir.clone();
    for face in &mut inward.model.faces {
        face.sense = cadmpeg_ir::topology::Sense::Reversed;
    }
    for coedge in &mut inward.model.coedges {
        coedge.sense = match coedge.sense {
            cadmpeg_ir::topology::Sense::Forward => cadmpeg_ir::topology::Sense::Reversed,
            cadmpeg_ir::topology::Sense::Reversed => cadmpeg_ir::topology::Sense::Forward,
        };
    }
    for face_loop in &mut inward.model.loops {
        let mut coedges = face_loop.coedges().to_vec();
        coedges.reverse();
        face_loop.replace_ring(coedges, Vec::new()).unwrap();
    }
    let b = Design::measure(inward);
    assert_eq!(b.confirmed_faces, b.measured_faces);
    assert!(!b.complete());
    assert!(!agreement(&a, &b).is_empty());
}

#[test]
fn nist_twin_check_rejects_an_open_shell() {
    let cube = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut broken = cube.clone();
    let faces = broken.model.shells[0].faces()[1..].to_vec();
    broken.model.shells[0].replace_faces(faces).unwrap();
    let a = Design::measure(cube);
    let b = Design::measure(broken);
    assert_eq!(b.closed_shells(), 0);
    assert!(!agreement(&a, &b).is_empty());
}

#[test]
fn nist_twin_check_rejects_changed_dimensions_and_placement() {
    let cube = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut scaled = cube.clone();
    scaled.model.bodies[0].transform = cadmpeg_ir::transform::Transform::affine([
        [2.0, 0.0, 0.0, 0.0],
        [0.0, 2.0, 0.0, 0.0],
        [0.0, 0.0, 2.0, 0.0],
    ]);
    let a = Design::measure(cube.clone());
    let b = Design::measure(scaled);
    assert!(b.complete());
    let failures = agreement(&a, &b);
    for quantity in ["area", "closed_volume", "bbox", "area.plane"] {
        assert!(failures.iter().any(|failure| failure == quantity));
    }

    let mut translated = cube;
    translated.model.bodies[0].transform = cadmpeg_ir::transform::Transform::affine([
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ]);
    let b = Design::measure(translated);
    assert!(b.complete());
    assert_eq!(agreement(&a, &b), ["bbox"]);
}
