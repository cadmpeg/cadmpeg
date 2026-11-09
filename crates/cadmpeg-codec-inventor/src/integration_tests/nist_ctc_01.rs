// SPDX-License-Identifier: Apache-2.0
//! NIST CTC-01 dimension and hole-note expectations from the CTC definitions
//! workbook.

use std::io::Cursor;
use std::path::Path;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::pmi::{
    DimensionKind, DimensionTolerance, GeometricToleranceKind, PmiDefinition, PmiTarget,
};
use serde_json::Value;

use crate::InventorCodec;

const VALUE_TOLERANCE: f64 = 1.0e-9;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Expected {
    PlusMinus(f64, f64),
    Limits(f64, f64),
}

fn tolerance_matches(actual: Option<&DimensionTolerance>, expected: Expected) -> bool {
    let close = |left: f64, right: f64| (left - right).abs() < VALUE_TOLERANCE;
    match (actual, expected) {
        (Some(DimensionTolerance::PlusMinus { lower, upper }), Expected::PlusMinus(low, high)) => {
            close(lower.value.get(), low) && close(upper.value.get(), high)
        }
        (Some(DimensionTolerance::Limits { lower, upper }), Expected::Limits(low, high)) => {
            close(lower.value.get(), low) && close(upper.value.get(), high)
        }
        _ => false,
    }
}

/// The radius of the cylinder of a target face, or of the circle of a target
/// edge.
fn target_radius(model: &Value, target: &PmiTarget) -> Option<f64> {
    let find = |arena: &str, id: &str| {
        model[arena]
            .as_array()?
            .iter()
            .find(|entry| entry["id"] == id)
            .cloned()
    };
    let geometry = match target {
        PmiTarget::Face { face } => {
            let face = find("faces", face.as_str())?;
            find("surfaces", face["surface"].as_str()?)?["geometry"].clone()
        }
        PmiTarget::Edge { edge } => {
            let edge = find("edges", edge.as_str())?;
            find("curves", edge["carrier"]["curve"].as_str()?)?["geometry"].clone()
        }
        _ => return None,
    };
    geometry["radius"].as_f64()
}

#[test]
fn nist_ctc_01_transfers_dimension_tolerances_and_targets() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/nist-pmi/inventor/INV_nist_ctc_01_asme1_2021.ipt");
    let bytes = std::fs::read(path).expect("read Inventor specimen");
    let result = InventorCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode Inventor specimen");
    let model = serde_json::to_value(&result.ir().model).expect("serialize model");
    let half_degree = 0.5_f64.to_radians();
    let expected = [
        (
            DimensionKind::Diameter,
            25.0,
            Expected::PlusMinus(-0.15, 0.15),
            1,
        ),
        (
            DimensionKind::Diameter,
            35.0,
            Expected::Limits(34.8, 35.2),
            2,
        ),
        (
            DimensionKind::Diameter,
            20.0,
            Expected::PlusMinus(-0.05, 0.10),
            1,
        ),
        (
            DimensionKind::Diameter,
            20.0,
            Expected::PlusMinus(-0.10, 0.05),
            1,
        ),
        (
            DimensionKind::Diameter,
            35.0,
            Expected::PlusMinus(-0.2, 0.0),
            1,
        ),
        (
            DimensionKind::Diameter,
            35.0,
            Expected::PlusMinus(0.0, 0.2),
            1,
        ),
        (
            DimensionKind::Angular,
            60.0_f64.to_radians(),
            Expected::PlusMinus(-half_degree, half_degree),
            1,
        ),
    ];
    let dimensions: Vec<_> = result
        .ir()
        .model
        .pmi
        .iter()
        .filter_map(|pmi| match &pmi.definition {
            PmiDefinition::Dimension(dimension) => Some((pmi, dimension)),
            _ => None,
        })
        .collect();
    for (kind, nominal, tolerance, count) in expected {
        let matching: Vec<_> = dimensions
            .iter()
            .filter(|(_, dimension)| {
                *dimension.kind() == kind
                    && dimension
                        .nominal()
                        .is_some_and(|value| (value.value.get() - nominal).abs() < 1.0e-9)
                    && tolerance_matches(dimension.tolerance(), tolerance)
            })
            .collect();
        assert_eq!(matching.len(), count, "{kind:?} {nominal} {tolerance:?}");
        for (pmi, _) in matching {
            if kind == DimensionKind::Diameter {
                let [target] = pmi.targets.as_slice() else {
                    panic!("{} has {} targets", pmi.id.as_str(), pmi.targets.len());
                };
                let radius = target_radius(&model, target).expect("target radius");
                assert!(
                    (2.0 * radius - nominal).abs() < 1.0e-9,
                    "{}",
                    pmi.id.as_str()
                );
            } else {
                assert_eq!(pmi.targets.len(), 2, "{}", pmi.id.as_str());
            }
        }
    }
    let diameter_targets: std::collections::BTreeSet<_> = dimensions
        .iter()
        .filter(|(_, dimension)| *dimension.kind() == DimensionKind::Diameter)
        .flat_map(|(pmi, _)| pmi.targets.iter().map(|target| format!("{target:?}")))
        .collect();
    assert_eq!(diameter_targets.len(), 7);
    // Workbook row 10: the two position frames attach to two location
    // dimensions between plane pairs; the STEP twin gives each two faces and
    // no tolerance.
    let locations: Vec<_> = dimensions
        .iter()
        .filter(|(_, dimension)| *dimension.kind() == DimensionKind::Location)
        .collect();
    assert_eq!(locations.len(), 2);
    for (pmi, dimension) in locations {
        assert!(dimension.tolerance().is_none(), "{}", pmi.id.as_str());
        assert_eq!(pmi.targets.len(), 2, "{}", pmi.id.as_str());
    }
}

#[test]
fn nist_ctc_01_transfers_feature_control_frames_and_datums() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/nist-pmi/inventor/INV_nist_ctc_01_asme1_2021.ipt");
    let bytes = std::fs::read(path).expect("read Inventor specimen");
    let result = InventorCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode Inventor specimen");
    let pmi = &result.ir().model.pmi;
    let mut datums: Vec<_> = pmi
        .iter()
        .filter_map(|annotation| match &annotation.definition {
            PmiDefinition::Datum { identification } => {
                assert_eq!(annotation.targets.len(), 1, "{}", annotation.id.as_str());
                Some(identification.as_str())
            }
            _ => None,
        })
        .collect();
    let labels: std::collections::HashMap<_, _> = pmi
        .iter()
        .filter_map(|annotation| match &annotation.definition {
            PmiDefinition::Datum { identification } => {
                Some((annotation.id.clone(), identification.as_str()))
            }
            _ => None,
        })
        .collect();
    datums.sort_unstable();
    assert_eq!(datums, ["A", "B", "C"]);
    // The workbook frames name their datums in the order A, B, C.
    for annotation in pmi {
        if let PmiDefinition::DatumSystem { references } = &annotation.definition {
            let order: Vec<_> = references
                .as_slice()
                .iter()
                .map(|reference| labels.get(&reference.datum).copied())
                .collect();
            let expected: Vec<_> = ["A", "B", "C"]
                .into_iter()
                .take(order.len())
                .map(Some)
                .collect();
            assert_eq!(order, expected, "{}", annotation.id.as_str());
        }
    }
    // Workbook rows 3 through 12: seven size dimensions, one angular and two
    // location dimensions, six frames, three datums, and five datum systems.
    assert_eq!(pmi.len(), 24);
    let datum_count = |system: &cadmpeg_ir::ids::PmiId| {
        pmi.iter()
            .find(|annotation| annotation.id == *system)
            .and_then(|annotation| match &annotation.definition {
                PmiDefinition::DatumSystem { references } => Some(references.as_slice().len()),
                _ => None,
            })
    };
    let expected = [
        (GeometricToleranceKind::Flatness, 0.2, 0, 1),
        (GeometricToleranceKind::Perpendicularity, 1.5, 1, 1),
        (GeometricToleranceKind::SurfaceProfile, 0.5, 1, 1),
        (GeometricToleranceKind::SurfaceProfile, 1.25, 3, 1),
        (GeometricToleranceKind::Position, 0.75, 3, 2),
    ];
    let frames: Vec<_> = pmi
        .iter()
        .filter_map(|annotation| match &annotation.definition {
            PmiDefinition::GeometricTolerance {
                tolerance,
                magnitude,
                datum_system,
                ..
            } => Some((
                annotation,
                tolerance,
                magnitude.get().value.get(),
                datum_system,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(frames.len(), 6);
    for (kind, magnitude, datums, count) in expected {
        let matching: Vec<_> = frames
            .iter()
            .filter(|(_, tolerance, value, system)| {
                **tolerance == kind
                    && (value - magnitude).abs() < VALUE_TOLERANCE
                    && system.as_ref().map_or(0, |id| datum_count(id).unwrap_or(0)) == datums
            })
            .collect();
        assert_eq!(matching.len(), count, "{kind:?} {magnitude}");
        assert!(matching
            .iter()
            .all(|(annotation, ..)| !annotation.targets.is_empty()));
    }
}
