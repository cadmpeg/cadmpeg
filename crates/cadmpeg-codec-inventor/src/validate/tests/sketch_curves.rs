// SPDX-License-Identifier: Apache-2.0

use super::{empty_data, sketch_point, sketch_records, TestResult};
use crate::pmdc::{PmDcListMetadata, PmDcReference, PmDcReferenceList};
use crate::sketch::PmDcSketchEntityKind;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::{Angle, FiniteReal, Length, PositiveReal};
use cadmpeg_ir::sketches::{
    EllipseRadii, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};

#[test]
fn neutral_ellipse_validation_uses_the_closed_source_gate() -> TestResult {
    for state in [0, 1, 2, u8::MAX] {
        for ends in [[0, 0], [2, 2], [0, 2]] {
            let mut data = empty_data();
            let mut source = sketch_point(
                22,
                0,
                serde_json::json!({"state": null, "associations": null}),
            )?;
            crate::test_support::sketch_curves::edit_entity(&mut source, |payload| {
                payload.kind = PmDcSketchEntityKind::Ellipse {
                    points: PmDcReferenceList::new(
                        2,
                        Some(PmDcListMetadata::U32([2, 0])),
                        ends.map(PmDcReference::from_packed).to_vec(),
                    )
                    .expect("ellipse ends"),
                    auxiliary: Vec::new(),
                    center: PmDcReference::from_packed(2),
                    major_direction: [FiniteReal::ONE, FiniteReal::ZERO],
                    major_radius: PositiveReal::new(3.0).expect("major radius"),
                    minor_radius: PositiveReal::new(2.0).expect("minor radius"),
                    state,
                };
            });
            source.identity.type_id =
                crate::record_identity::RecordTypeId::from_bytes(source.kind.record_type_id());
            let native = source.id();
            data.pm_dc_sketch_entities.push(source);
            data.pm_dc_sketch_entities.push(sketch_point(
                22,
                1,
                serde_json::json!({"state": null, "associations": null}),
            )?);
            sketch_records(&mut data)?;
            let mut ir = cadmpeg_ir::CadIr::empty();
            ir.model.sketch_entities.push(
                SketchEntity::new(
                    SketchEntityId::mint("inventor:design:sketch-entity#generated-0")?,
                    SketchId::mint("inventor:design:sketch#generated-3")?,
                    SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
                        center: Point2::new(0.0, 0.0),
                        major_angle: Angle::new(0.0).ok_or("angle")?,
                        radii: EllipseRadii {
                            major_radius: Length::new(30.0).ok_or("major length")?,
                            minor_radius: Length::new(20.0).ok_or("minor length")?,
                        },
                        bounds: None,
                    })?,
                )
                .with_native_ref(Some(native)),
            );
            let mut findings = Vec::new();
            super::super::validate_sketches(&data, &ir, &mut findings);
            assert_eq!(findings.is_empty(), state == 1 && ends == [0, 0]);
            for finding in &findings {
                assert!(finding.message.contains("closed-ellipse source form"));
            }
            ir.model.sketch_entities[0].geometry =
                SketchGeometry::native(cadmpeg_core::nonblank_literal!("ellipse"));
            findings.clear();
            super::super::validate_sketches(&data, &ir, &mut findings);
            assert!(findings.is_empty());
        }
    }
    Ok(())
}

#[test]
fn neutral_spline_validation_recomputes_the_endpoint_bound_form() -> TestResult {
    for pole_count in [4, 5, 8] {
        let sources = crate::test_support::sketch_curves::open_cubic_entities_with(pole_count, 3.0);
        let index = cadmpeg_core::decode::unique_index(sources.iter().map(|source| {
            (
                (
                    source.identity.segment_token.as_str(),
                    source.identity.record_ordinal,
                ),
                source,
            )
        }));
        let geometry = crate::sketch::project_geometry(&sources[2], &index)
            .ok_or("synthetic neutral cubic")?;
        let neutral = SketchEntity::new(
            SketchEntityId::mint("inventor:design:sketch-entity#generated-7")?,
            SketchId::mint("inventor:design:sketch#generated-2")?,
            geometry,
        )
        .with_native_ref(Some(sources[2].id()))
        .with_endpoint_refs(vec![
            "inventor:pmdc:sketch-entity#generated-3".to_owned(),
            "inventor:pmdc:sketch-entity#generated-4".to_owned(),
        ]);
        for mode in 0..8 {
            let mut data = empty_data();
            data.pm_dc_sketch_entities = sources.clone();
            match mode {
                0 => {}
                1 | 2 => {
                    let mut wire = serde_json::to_value(&data.pm_dc_sketch_entities[2])?;
                    if mode == 1 {
                        wire["save_version_major"] = serde_json::json!(25);
                    } else {
                        wire["entity_flags"] = serde_json::json!(0);
                    }
                    data.pm_dc_sketch_entities[2] = serde_json::from_value(wire)?;
                }
                3 => {
                    crate::test_support::sketch_curves::edit_entity(
                        &mut data.pm_dc_sketch_entities[0],
                        |payload| {
                            let PmDcSketchEntityKind::Point { position, .. } = &mut payload.kind
                            else {
                                panic!("endpoint");
                            };
                            position[0] = FiniteReal::ONE;
                        },
                    );
                }
                4 => crate::test_support::sketch_curves::edit_entity(
                    &mut data.pm_dc_sketch_entities[0],
                    |payload| payload.sketch = PmDcReference::from_packed(4),
                ),
                5 => {
                    let wire = serde_json::to_value(&data.pm_dc_sketch_entities[2])?;
                    let mut changed = wire;
                    changed["kind"]["data"]["secondary"]["poles"][1][0] = serde_json::json!(1.0);
                    data.pm_dc_sketch_entities[2] = serde_json::from_value(changed)?;
                }
                _ => {}
            }
            sketch_records(&mut data)?;
            data.records.push(serde_json::from_value(serde_json::json!({
                "id": "inventor:rse:record#generated-2", "token": "generated", "ordinal": 2,
                "selector": 0, "type_index": 0,
                "type_id": "114d8790d011f8d10008cabc0663dc09",
                "payload_offset": 0, "payload_len": 0, "payload_sha256": "",
                "trailing_payload_len": 0, "trailer_len": 0, "trailer_sha256": ""
            }))?);
            let mut ir = cadmpeg_ir::CadIr::empty();
            ir.model.sketch_entities.push(neutral.clone());
            if mode == 6 {
                let SketchGeometryDefinition::Nurbs { curve } =
                    ir.model.sketch_entities[0].geometry.definition()
                else {
                    panic!("neutral cubic");
                };
                let mut changed = curve.clone();
                changed
                    .try_map_control_points(|_, point| {
                        let p = point.get();
                        cadmpeg_ir::units::FinitePoint2::new(Point2::new(p.u + 1.0, p.v)).ok_or(())
                    })
                    .expect("synthetic translated curve");
                ir.model.sketch_entities[0].geometry = SketchGeometry::nurbs(changed);
            }
            if mode == 7 {
                ir.model.sketch_entities[0].endpoint_refs.reverse();
            }
            let mut findings = Vec::new();
            super::super::validate_sketches(&data, &ir, &mut findings);
            assert_eq!(findings.is_empty(), mode == 0, "source form {mode}");
            for finding in &findings {
                assert!(finding.message.contains("admitted source curve form"));
            }
            ir.model.sketch_entities[0].geometry =
                SketchGeometry::native(cadmpeg_core::nonblank_literal!("spline"));
            ir.model.sketch_entities[0].endpoint_refs.clear();
            findings.clear();
            super::super::validate_sketches(&data, &ir, &mut findings);
            assert!(findings.is_empty());
        }
    }
    Ok(())
}

#[test]
fn spline_midpoint_validation_requires_native_locus_semantics() -> TestResult {
    use cadmpeg_ir::sketches::{
        SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId, SketchLocus,
    };
    for mode in 0..5 {
        let sources = if mode == 0 {
            crate::test_support::sketch_curves::open_cubic_entities()
        } else {
            crate::test_support::sketch_curves::line_midpoint_entities()
        };
        let mut data = empty_data();
        data.pm_dc_sketch_entities = sources;
        let mut wire = serde_json::to_value(crate::test_support::sketch_curves::spline_midpoint())?;
        if mode == 2 {
            wire["header"]["parameter"] = serde_json::json!(PmDcReference::from_packed(4));
        } else if mode == 3 {
            wire["kind"]["value"] = serde_json::json!(1);
        } else if mode == 4 {
            wire["kind"]["value"] = serde_json::Value::Null;
        }
        let native: crate::sketch::PmDcSketchConstraint = serde_json::from_value(wire)?;
        let native_ref = native.id();
        data.pm_dc_sketch_constraints.push(native);
        sketch_records(&mut data)?;
        for (ordinal, record_type) in [
            (2, "114d8790d011f8d10008cabc0663dc09"),
            (9, "bf70e821d011d0d20008ccbc0663dc09"),
        ] {
            data.records.push(serde_json::from_value(serde_json::json!({
            "id": format!("inventor:rse:record#generated-{ordinal}"), "token": "generated", "ordinal": ordinal,
            "selector": 0, "type_index": 0, "type_id": record_type,
            "payload_offset": 0, "payload_len": 0, "payload_sha256": "",
            "trailing_payload_len": 0, "trailer_len": 0, "trailer_sha256": ""
        }))?);
        }
        let index =
            cadmpeg_core::decode::unique_index(data.pm_dc_sketch_entities.iter().map(|source| {
                (
                    (
                        source.identity.segment_token.as_str(),
                        source.identity.record_ordinal,
                    ),
                    source,
                )
            }));
        let sketch = SketchId::mint("inventor:design:sketch#generated-2")?;
        let point = SketchEntityId::mint("inventor:design:sketch-entity#generated-3")?;
        let curve = SketchEntityId::mint("inventor:design:sketch-entity#generated-7")?;
        let mut ir = cadmpeg_ir::CadIr::empty();
        for (ordinal, id) in [(0, point.clone()), (2, curve.clone())] {
            let source = &data.pm_dc_sketch_entities[ordinal];
            ir.model.sketch_entities.push(
                SketchEntity::new(
                    id,
                    sketch.clone(),
                    crate::sketch::project_geometry(source, &index).ok_or("source geometry")?,
                )
                .with_native_ref(Some(source.id()))
                .with_endpoint_refs(if ordinal == 2 {
                    vec![
                        if mode == 0 {
                            "inventor:pmdc:sketch-entity#generated-3".to_owned()
                        } else {
                            "inventor:pmdc:sketch-entity#generated-5".to_owned()
                        },
                        "inventor:pmdc:sketch-entity#generated-4".to_owned(),
                    ]
                } else {
                    Vec::new()
                }),
            );
        }
        ir.model.sketch_constraints.push(SketchConstraint {
            id: SketchConstraintId::mint("inventor:design:sketch-constraint#generated-9")?,
            sketch,
            definition: SketchConstraintDefinitionInput::Midpoint {
                point: SketchLocus::Entity(point.clone()),
                entity: curve.clone(),
            }
            .try_into()?,
            name: None,
            driving: None,
            active: None,
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: Some(native_ref),
        });
        let mut findings = Vec::new();
        super::super::validate_sketches(&data, &ir, &mut findings);
        assert_eq!(
            findings.len(),
            usize::from(mode != 1),
            "source controls {mode}"
        );
        if let Some(finding) = findings.first() {
            assert!(finding.message.contains("resolved line or circular arc"));
        }
        ir.model.sketch_constraints[0].definition = SketchConstraintDefinitionInput::Native {
            native_kind: cadmpeg_core::nonblank_literal!("midpoint"),
            native_state: None,
            native_flags: None,
            native_properties: std::collections::BTreeMap::new(),
            entities: vec![curve, point],
            parameter: None,
            operands: Vec::new(),
        }
        .try_into()?;
        findings.clear();
        super::super::validate_sketches(&data, &ir, &mut findings);
        assert!(findings.is_empty());
    }
    Ok(())
}
