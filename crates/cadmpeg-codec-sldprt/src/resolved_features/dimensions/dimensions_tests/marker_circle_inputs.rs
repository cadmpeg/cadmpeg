use super::super::{
    native_dimensioned_circle_construction_state, radial_circle_records, radial_dimension_radius,
    MarkerCircleInputs,
};
use crate::records::operand_tag::NativeOperandTag;
use crate::records::{
    FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind, FeatureInputRelationFamily,
    FeatureInputRelationInstance, SketchInputEntity, SketchInputKind,
};
use crate::resolved_features::LEGACY_EXTENDED_SKETCH_MARKER;
use cadmpeg_ir::features::{
    DesignParameter, DimensionDisplay, FeatureId, ParameterId, ParameterValue,
};
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::units::FiniteVector;
use std::collections::BTreeMap;

fn radial_record(radial_index: u16) -> Vec<u8> {
    let mut record = vec![0; 112];
    record[..LEGACY_EXTENDED_SKETCH_MARKER.len()].copy_from_slice(LEGACY_EXTENDED_SKETCH_MARKER);
    record[5..13].fill(0xff);
    record[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    record[17..21].copy_from_slice(&2u32.to_le_bytes());
    record[23..31].copy_from_slice(&[0x04, 0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00]);
    record[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    record[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    record[56..58].copy_from_slice(&radial_index.to_le_bytes());
    record[58..60].copy_from_slice(&radial_index.to_le_bytes());
    record[60..64].copy_from_slice(&1u32.to_le_bytes());
    record[64..72].copy_from_slice(&(-1.0f64).to_le_bytes());
    record[72..76].copy_from_slice(&(-1i32).to_le_bytes());
    for at in (78..94).step_by(4) {
        record[at..at + 4].copy_from_slice(&(-2i32).to_le_bytes());
    }
    record
}

#[test]
fn marker_circle_inputs_match_the_parameter_and_lane_scans() {
    let mut state = 0x2545_f491_u64;
    let mut next = |bound: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        u32::try_from((state >> 33) % bound).unwrap()
    };
    let features = ["feature-0", "feature-1", "feature-2", "feature-3"];
    let feature_ids =
        features.map(|feature| FeatureId::mint(format!("synthetic:test:id#{feature}")).unwrap());
    let mut constructions = 0;
    for _ in 0..32 {
        let lanes = (0..4)
            .map(|position| {
                let id = format!("lane-{}", position % 3);
                let mut payload = Vec::new();
                for _ in 0..next(4) {
                    payload.extend(radial_record(u16::try_from(1 + next(5)).unwrap()));
                }
                payload.extend([0; 16]);
                let sketch_entities = (0..next(12))
                    .map(|ordinal| {
                        let offset = u64::from(next(64));
                        let kind = [SketchInputKind::Point, SketchInputKind::LineOrCircle]
                            [usize::try_from(next(2)).unwrap()];
                        let mut marker = SketchInputEntity::new(
                            format!("marker-{}", next(10)),
                            id.clone(),
                            ordinal,
                            offset,
                            kind,
                        );
                        marker.feature_ref = (next(5) != 0)
                            .then(|| features[usize::try_from(next(3)).unwrap()].to_owned());
                        let [u, v] = [next(4), next(4)].map(|grid| f64::from(grid) * 0.001);
                        marker.coordinates_m =
                            (next(6) != 0).then(|| FiniteVector::new([u, v])).flatten();
                        marker
                    })
                    .collect();
                let relation_instances = (0..next(4))
                    .map(|ordinal| FeatureInputRelationInstance {
                        id: format!("relation-{position}-{ordinal}"),
                        parent: id.clone(),
                        ordinal, sequence: ordinal,
                        offset: Some(0),
                        family: if next(3) == 0 {
                            FeatureInputRelationFamily::PointPointDistance
                        } else {
                            FeatureInputRelationFamily::CircleDiameter
                        },
                        class_ref: "class".into(),
                        feature_ref: features[usize::try_from(next(4)).unwrap()].into(),
                        scalars: crate::records::relation_scalars::RelationScalars::from_refs(
                            vec!["scalar".into()],
                            None,
                            None,
                        )
                        .unwrap(),
                        operands: vec![FeatureInputOperand {
                            offset: 0,
                            reference_ref: "reference".into(),
                            kind: FeatureInputOperandKind::Native(
                                NativeOperandTag::try_from(0x829a).unwrap(),
                            ),
                            entity_index: 0,
                            entity_ref: Some(format!("marker-{}", next(10))),
                        }],
                    })
                    .collect();
                FeatureInputLane {
                    id,
                    configuration: None,
                    native_payload: payload.into(),
                    classes: Vec::new(),
                    names: Vec::new(),
                    scalars: Vec::new(),
                    relation_bindings: Vec::new(),
                    relation_instances,
                    body_selections: Vec::new(),
                    edge_selections: Vec::new(),
                    surface_selections: Vec::new(),
                    generated_surface_identities: Vec::new(),
                    references: Vec::new(),
                    sketch_entities,
                }
            })
            .collect::<Vec<_>>();
        let parameters = (0..12)
            .map(|ordinal| DesignParameter {
                driving: true,
                id: ParameterId::mint(format!("synthetic:test:id#parameter-{ordinal}")).unwrap(),
                owner: (next(5) != 0)
                    .then(|| feature_ids[usize::try_from(next(4)).unwrap()].clone()),
                ordinal,
                name: "radial".into(),
                expression: String::new(),
                display: [
                    None,
                    Some(DimensionDisplay::Radius),
                    Some(DimensionDisplay::Diameter),
                ][usize::try_from(next(3)).unwrap()],
                value: Some(ParameterValue::Length(
                    Length::new(f64::from(next(5)) - 1.0).unwrap(),
                )),
                properties: BTreeMap::new(),
                pmi: None,
                native_ref: None,
            })
            .collect::<Vec<_>>();
        let inputs = MarkerCircleInputs::new(&parameters, &lanes);

        for id in &feature_ids {
            let expected = parameters
                .iter()
                .filter(|parameter| parameter.owner.as_ref() == Some(id))
                .filter_map(|parameter| {
                    radial_dimension_radius(parameter).map(|radius| (parameter.ordinal, radius))
                })
                .collect::<Vec<_>>();
            let indexed = inputs
                .radial_dimensions(id)
                .iter()
                .map(|(parameter, radius)| (parameter.ordinal, *radius))
                .collect::<Vec<_>>();
            assert_eq!(indexed, expected);
        }
        for lane in &lanes {
            let last = lanes.iter().rfind(|other| other.id == lane.id).unwrap();
            assert_eq!(
                inputs.lane_radial_records(&lane.id),
                radial_circle_records(&last.native_payload)
            );
        }
        for feature in features {
            let owned =
                |marker: &&SketchInputEntity| marker.feature_ref.as_deref() == Some(feature);
            let expected = lanes
                .iter()
                .enumerate()
                .filter(|(_, lane)| lane.sketch_entities.iter().any(|marker| owned(&marker)))
                .map(|(position, lane)| {
                    let markers = lane.sketch_entities.iter().filter(owned);
                    (
                        position,
                        markers.map(std::ptr::from_ref).collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>();
            let groups = inputs.markers(feature);
            assert!(groups
                .iter()
                .all(|group| std::ptr::eq(group.lane, &raw const lanes[group.position])));
            let indexed = groups
                .iter()
                .map(|group| {
                    let markers = group
                        .markers
                        .iter()
                        .map(|marker| std::ptr::from_ref(*marker));
                    (group.position, markers.collect::<Vec<_>>())
                })
                .collect::<Vec<_>>();
            assert_eq!(indexed, expected);

            let expected = lanes
                .iter()
                .flat_map(|lane| &lane.relation_instances)
                .filter(|relation| {
                    relation.feature_ref == feature
                        && relation.family == FeatureInputRelationFamily::CircleDiameter
                })
                .map(std::ptr::from_ref)
                .collect::<Vec<_>>();
            let indexed = inputs
                .circle_diameters(feature)
                .iter()
                .map(|relation| std::ptr::from_ref(*relation))
                .collect::<Vec<_>>();
            assert_eq!(indexed, expected);

            let markers = lanes
                .iter()
                .flat_map(|lane| &lane.sketch_entities)
                .filter(owned)
                .collect::<Vec<_>>();
            for center in &markers {
                let Some([cu, cv]) = center.coordinates_m.map(FiniteVector::get) else {
                    continue;
                };
                for radial in &markers {
                    let Some([ru, rv]) = radial.coordinates_m.map(FiniteVector::get) else {
                        continue;
                    };
                    let radius = (ru - cu).hypot(rv - cv) * 1000.0;
                    let expected = native_dimensioned_circle_construction_state(
                        &lanes, feature, center, radius,
                    );
                    constructions += usize::from(expected.is_some());
                    assert_eq!(inputs.construction_state(feature, center, radius), expected);
                }
            }
        }
    }
    assert!(constructions > 0);
}
