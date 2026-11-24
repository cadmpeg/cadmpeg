// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use super::*;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

fn edge(name: &str, geometry: Value) -> Edge {
    let record = serde_json::Map::from_iter([("geometry".into(), geometry)]);
    Edge::new(name.to_owned(), &Value::Object(record)).unwrap()
}

fn half_ring(ellipse: bool, from: f64, to: f64, reverse: bool) -> Ring {
    let geometry = if ellipse {
        json!({"kind":"ellipse","center":{"u":0.0,"v":0.0},"major_radius":2.0,
            "minor_radius":1.0,"major_angle":0.0,"bounds":[from,to]})
    } else {
        json!({"kind":"arc","center":{"u":0.0,"v":0.0},"radius":1.0,"start_angle":from,"end_angle":to})
    };
    let arc = edge("arc", geometry);
    let diameter = edge(
        "diameter",
        json!({"kind":"line", "start":{"u":arc.b[0],"v":arc.b[1]},
        "end":{"u":arc.a[0],"v":arc.a[1]}}),
    );
    let mut edges = vec![(arc, reverse), (diameter, reverse)];
    if reverse {
        edges.reverse();
    }
    Ring { edges }
}

#[test]
fn curved_endpoint_height_crossings_agree_with_lines_and_reversed_uses() {
    for ellipse in [false, true] {
        for reverse in [false, true] {
            let upper = half_ring(ellipse, 0.0, PI, reverse);
            assert!(!upper.contains([-3.0, 0.0]).unwrap());
            assert!(!upper.contains([3.0, 0.0]).unwrap());
            assert!(upper.contains([0.0, 0.5]).unwrap());
            assert!(!upper.contains([0.0, -0.5]).unwrap());
            let lower = half_ring(ellipse, PI, TAU, reverse);
            assert!(!lower.contains([-3.0, 0.0]).unwrap());
            assert!(lower.contains([0.0, -0.5]).unwrap());
            let left = half_ring(ellipse, FRAC_PI_2, 3.0 * FRAC_PI_2, reverse);
            for y in [-1.0, 1.0] {
                assert!(!left.contains([-3.0, y]).unwrap());
            }
            assert!(left.contains([-0.5, 0.0]).unwrap());
        }
    }
}

#[test]
fn full_carrier_seams_and_interior_tangencies_do_not_double_count() {
    for ellipse in [false, true] {
        let g = if ellipse {
            json!({"kind":"ellipse","center":{"u":0.0,"v":0.0},"major_radius":2.0,"minor_radius":1.0,"major_angle":0.0})
        } else {
            json!({"kind":"circle","center":{"u":0.0,"v":0.0},"radius":1.0})
        };
        let ring = Ring {
            edges: vec![(edge("closed", g), false)],
        };
        assert!(ring.contains([0.0, 0.0]).unwrap());
        assert!(!ring.contains([-3.0, 0.0]).unwrap());
        for y in [-1.0, 1.0] {
            assert!(!ring.contains([-3.0, y]).unwrap());
        }
    }
}

#[test]
fn rotated_ellipse_endpoints_near_extrema_keep_crossing_parity() {
    let angle = 0.3_f64;
    let phase = angle.cos().atan2(2.0 * angle.sin());
    for reverse in [false, true] {
        let arc = edge(
            "rotated",
            json!({"kind":"ellipse","center":{"u":0.0,"v":0.0},
            "major_radius":2.0,"minor_radius":1.0,"major_angle":angle,
            "bounds":[phase - 0.0001, phase + 3.0]}),
        );
        let heights = [arc.a[1], arc.b[1]];
        let chord = edge(
            "chord",
            json!({"kind":"line","start":{"u":arc.b[0],"v":arc.b[1]},
            "end":{"u":arc.a[0],"v":arc.a[1]}}),
        );
        let ring = Ring {
            edges: vec![(arc, reverse), (chord, reverse)],
        };
        for y in heights {
            assert!(!ring.contains([-6.0, y]).unwrap());
        }
    }
}

#[test]
fn isolation_certificates_accept_disjoint_arcs_and_keep_contacts_unproven() {
    let arc = |name, radius, from, to| {
        edge(
            name,
            json!({"kind":"arc","center":{"u":0.0,"v":0.0},
        "radius":radius,"start_angle":from,"end_angle":to}),
        )
    };
    let wide = arc("wide", 3.0, 0.0, 1.5 * PI);
    let gap = arc("gap", 3.0, 5.0 * PI / 3.0, 11.0 * PI / 6.0);
    let inner = arc("inner", 1.0, PI / 4.0, 3.0 * PI / 4.0);
    assert!(separated(&wide, &gap).unwrap());
    assert!(separated(&wide, &inner).unwrap());
    let touching = arc("touching", 3.0, 1.5 * PI, 11.0 * PI / 6.0);
    assert!(!separated(&wide, &touching).unwrap());
    let diameter = edge(
        "diameter",
        json!({"kind":"line","start":{"u":-4.0,"v":0.0},"end":{"u":4.0,"v":0.0}}),
    );
    assert!(!separated(&wide, &diameter).unwrap());
}

#[test]
fn isolation_projections_keep_roundoff_near_translated_contacts_unproven() {
    let origin = 1_000_000_000.0;
    let arc = edge(
        "translated",
        json!({"kind":"arc","center":{"u":origin,"v":origin},"radius":2.0,"start_angle":PI/4.0,"end_angle":3.0*PI/4.0}),
    );
    for (gap, proven) in [(0.0, false), (0.000_001, false), (0.001, true)] {
        let line = edge(
            "line",
            json!({"kind":"line","start":{"u":origin-1.0,"v":origin+2.0+gap},"end":{"u":origin+1.0,"v":origin+2.0+gap}}),
        );
        assert_eq!(separated(&arc, &line).unwrap(), proven);
    }
}

fn frame_model() -> Model {
    stored_frame_model(false)
}

fn stored_frame_model(clockwise: bool) -> Model {
    let mut model = cadmpeg_ir::CadIr::empty().model;
    let corners = if clockwise {
        [[-4.0, -1.0], [-4.0, 1.0], [-2.0, 1.0], [-2.0, -1.0]]
    } else {
        [[-4.0, -1.0], [-2.0, -1.0], [-2.0, 1.0], [-4.0, 1.0]]
    };
    let mut geometries: Vec<Value> = (0..4)
        .map(|i| {
            let [su, sv] = corners[i];
            let [eu, ev] = corners[(i + 1) % 4];
            json!({"kind":"line","start":{"u":su,"v":sv},"end":{"u":eu,"v":ev}})
        })
        .collect();
    geometries.push(json!({"kind":"circle","center":{"u":-3.25,"v":0.0},"radius":0.25}));
    let half = half_ring(false, 0.0, PI, false);
    geometries.extend(half.edges.into_iter().map(|(e, _)| e.geometry));
    for (i, g) in geometries.into_iter().enumerate() {
        model.sketch_entities.push(
            serde_json::from_value(json!({"id":format!("synthetic:model:sketch-entity#{i}"),
            "sketch":"synthetic:model:sketch#frame","construction":false,"endpoint_refs":[],"geometry":g}))
            .unwrap(),
        );
    }
    model
}

#[test]
fn unrelated_semicircle_does_not_enclose_the_rectangle_hole_witness() {
    let model = frame_model();
    let witness = edge(
        "synthetic:model:sketch-entity#4",
        json!({"kind":"circle","center":{"u":-3.25,"v":0.0},"radius":0.25}),
    );
    assert_eq!(witness.a, [-3.0, 0.0]);
    let region = exterior_region(
        "synthetic:model:sketch#frame",
        &[witness],
        &model,
        &mut Recovery::default(),
    )
    .unwrap();
    assert_eq!(region["outer_boundary"].as_array().unwrap().len(), 4);
    assert_eq!(region["hole_boundaries"].as_array().unwrap().len(), 1);
}

#[test]
fn long_escaped_ids_exhaust_bytes_before_boundary_expansion() {
    let mut model = frame_model();
    let mut wire = serde_json::to_value(&model.sketch_entities[0]).unwrap();
    wire["id"] = json!(format!(
        "synthetic:model:sketch-entity#{}",
        "quoted\"".repeat(512)
    ));
    model.sketch_entities[0] = serde_json::from_value(wire).unwrap();
    let witness = edge(
        "synthetic:model:sketch-entity#4",
        json!({"kind":"circle","center":{"u":-3.25,"v":0.0},"radius":0.25}),
    );
    let mut recovery = Recovery::default();
    recovery.budget.bytes = 1024;
    assert_eq!(
        exterior_region(
            "synthetic:model:sketch#frame",
            &[witness],
            &model,
            &mut recovery
        )
        .unwrap_err(),
        BUDGET_LOSS
    );
    assert_eq!(recovery.budget.bytes, 0);
    assert!(recovery.budget.output > 0);
    assert_eq!(recovery.check().unwrap_err(), BUDGET_LOSS);
    let work = recovery.budget.work;
    assert_eq!(recovery.budget.work(1).unwrap_err(), BUDGET_LOSS);
    assert_eq!(recovery.budget.work, work);
}

#[test]
fn cached_topology_still_charges_cumulative_output_and_work() {
    for output_limited in [false, true] {
        let model = frame_model();
        let witness = edge(
            "synthetic:model:sketch-entity#4",
            json!({"kind":"circle","center":{"u":-3.25,"v":0.0},"radius":0.25}),
        );
        let mut recovery = Recovery::default();
        if output_limited {
            recovery.budget.output = 10;
        } else {
            recovery.budget.work = 200;
        }
        exterior_region(
            "synthetic:model:sketch#frame",
            std::slice::from_ref(&witness),
            &model,
            &mut recovery,
        )
        .unwrap();
        let after_first = recovery.budget.work;
        exterior_region(
            "synthetic:model:sketch#frame",
            std::slice::from_ref(&witness),
            &model,
            &mut recovery,
        )
        .unwrap();
        assert_eq!(recovery.loops.len(), 1);
        if !output_limited {
            eprintln!(
                "Cached recovery: first {} work units, next {} work units",
                200 - after_first,
                after_first - recovery.budget.work
            );
        }
        if !output_limited {
            assert!(after_first - recovery.budget.work < 200 - after_first);
        }
        let mut refused = false;
        for _ in 0..20 {
            if let Err(reason) = exterior_region(
                "synthetic:model:sketch#frame",
                std::slice::from_ref(&witness),
                &model,
                &mut recovery,
            ) {
                assert_eq!(reason, BUDGET_LOSS);
                refused = true;
                break;
            }
        }
        assert!(refused);
    }
}

/// Twice the signed area of a boundary made of line uses and one full circle.
fn signed_area(model: &Model, boundary: &Value) -> f64 {
    let uses = boundary.as_array().unwrap();
    let find = |id: &Value| {
        model
            .sketch_entities
            .iter()
            .map(|e| serde_json::to_value(e).unwrap())
            .find(|e| e["id"] == *id)
            .map(|e| e["geometry"].clone())
            .unwrap()
    };
    if let [one] = uses.as_slice() {
        let radius = find(&one["entity"])["radius"].as_f64().unwrap();
        let sign = if one["reversed"].as_bool().unwrap() {
            -1.0
        } else {
            1.0
        };
        return sign * 2.0 * PI * radius * radius;
    }
    uses.iter()
        .map(|u| {
            let g = find(&u["entity"]);
            let point = |key: &str| [g[key]["u"].as_f64().unwrap(), g[key]["v"].as_f64().unwrap()];
            let (a, b) = if u["reversed"].as_bool().unwrap() {
                (point("end"), point("start"))
            } else {
                (point("start"), point("end"))
            };
            a[0] * b[1] - a[1] * b[0]
        })
        .sum()
}

#[test]
fn exterior_region_orients_the_outer_ring_counterclockwise_and_the_hole_clockwise() {
    for clockwise in [false, true] {
        let model = stored_frame_model(clockwise);
        let witness = edge(
            "synthetic:model:sketch-entity#4",
            json!({"kind":"circle","center":{"u":-3.25,"v":0.0},"radius":0.25}),
        );
        let region = exterior_region(
            "synthetic:model:sketch#frame",
            &[witness],
            &model,
            &mut Recovery::default(),
        )
        .unwrap();
        let outer = signed_area(&model, &region["outer_boundary"]);
        assert!((outer - 8.0).abs() < 1e-9, "outer {outer}");
        let hole = signed_area(&model, &region["hole_boundaries"][0]);
        assert!(hole < 0.0, "hole {hole}");
        assert_eq!(region["hole_boundaries"][0][0]["reversed"], json!(true));
    }
}

#[test]
fn oriented_returns_the_requested_sense_from_either_stored_sense() {
    for stored_reverse in [false, true] {
        let ring = half_ring(false, 0.0, PI, stored_reverse);
        let stored_ccw = ring.counterclockwise().unwrap();
        for want in [true, false] {
            let uses = ring.oriented(want).unwrap();
            let rebuilt = Ring {
                edges: uses
                    .iter()
                    .map(|u| {
                        let (e, _) = ring
                            .edges
                            .iter()
                            .find(|(e, _)| u["entity"].as_str() == Some(e.id.as_str()))
                            .unwrap();
                        (
                            edge(&e.id, e.geometry.clone()),
                            u["reversed"].as_bool().unwrap(),
                        )
                    })
                    .collect(),
            };
            assert_eq!(rebuilt.counterclockwise().unwrap(), want);
            if stored_ccw == want {
                assert_eq!(uses, ring.boundary());
            } else {
                let flipped: Vec<_> = ring.boundary().into_iter().rev().collect();
                assert_eq!(uses.len(), flipped.len());
                for (got, old) in uses.iter().zip(&flipped) {
                    assert_eq!(got["entity"], old["entity"]);
                    assert_ne!(got["reversed"], old["reversed"]);
                }
            }
        }
    }
}
