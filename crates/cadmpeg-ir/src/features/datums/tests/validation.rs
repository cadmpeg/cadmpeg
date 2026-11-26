use super::*;
use crate::design::selection::{
    Anchor, DatumPart, Members, PickTarget, Picked, Picks, PrincipalEntity, Selection, Witness,
};
use crate::report::{check::Check, Severity};

fn picked(target: PickTarget) -> Picked {
    Picked {
        target,
        context: None,
        native: None,
        domain: None,
        witness: None,
        resolved: None,
    }
}

fn consumer(pick: Picked) -> crate::features::Feature {
    feature(
        DatumAxisDraft {
            origin: Some(origin()),
            direction: Some(direction()),
            source_settings: std::collections::BTreeMap::from([(
                crate::codec_name!("test:selection"),
                NativeOption::Selection(Selection {
                    at: Anchor::Input,
                    picks: Picks::Named(Members::new(vec![pick]).unwrap()),
                    native: None,
                }),
            )]),
            ..Default::default()
        }
        .into_operation()
        .unwrap(),
        2,
    )
}

#[test]
fn datum_axis_witness_validation_uses_local_typed_arenas_and_distinct_identities() {
    for case in 0..4 {
        let mut body = crate::document::Topology::default();
        let vertex: crate::ids::VertexId = "test:witness:vertex#one".try_into().unwrap();
        let point: crate::ids::PointId = "test:witness:point#one".try_into().unwrap();
        let body_id: crate::ids::BodyId = "test:witness:body#one".try_into().unwrap();
        let region: crate::ids::RegionId = "test:witness:region#one".try_into().unwrap();
        let shell: crate::ids::ShellId = "test:witness:shell#one".try_into().unwrap();
        body.bodies.push(crate::topology::Body {
            unstated: None,
            id: body_id.clone(),
            kind: crate::topology::BodyKind::General,
            transform: None,
            name: None,
            color: None,
            visible: None,
            native_ref: None,
        });
        body.regions.push(crate::topology::Region::new(
            region.clone(),
            body_id,
            vec![shell.clone()],
        ));
        body.shells.push(crate::topology::Shell::with_free_vertex(
            shell,
            region,
            vertex.clone(),
        ));
        if case != 1 && case != 3 {
            body.points.push(crate::topology::Point::new(
                point.clone(),
                FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap(),
                None,
            ));
        }
        body.vertices.push(crate::topology::Vertex {
            id: vertex.clone(),
            point: if case == 1 {
                vertex.as_str().try_into().unwrap()
            } else {
                point
            },
            tolerance: None,
            native_ref: None,
        });
        if case == 2 {
            body.vertices.push(body.vertices[0].clone());
        }
        let mut pick = picked(PickTarget::Principal {
            entity: PrincipalEntity::Origin,
            container: None,
        });
        pick.witness = Some(Witness {
            point: None,
            direction: None,
            endpoints: None,
            frame: None,
            curves: Vec::new(),
            surface: None,
            body: Some(Box::new(body)),
        });
        let mut ir = crate::CadIr::empty();
        ir.model.features.push(consumer(pick));
        let report = crate::validate::validate_neutral(&ir, Vec::new()).unwrap();
        let errors = report
            .findings
            .iter()
            .filter(|finding| finding.severity == Severity::Error)
            .collect::<Vec<_>>();
        assert_eq!(errors.is_empty(), case == 0, "case {case}: {errors:?}");
        if case == 2 {
            assert!(errors
                .iter()
                .any(|finding| finding.check == Check::Identity));
        }
        if case == 1 || case == 3 {
            assert!(errors
                .iter()
                .any(|finding| finding.check == Check::ReferentialIntegrity));
        }
    }
}

#[test]
fn datum_axis_bound_dimensions_resolve_within_the_consuming_definition() {
    for (feature_scope, parameter_scope, valid) in [
        (None, None, true),
        (Some("a"), Some("a"), true),
        (Some("a"), Some("b"), false),
        (Some("a"), None, false),
        (None, Some("a"), false),
    ] {
        let id = |key| {
            crate::ids::ProductDefinitionId::mint(format!("test:model:definition#{key}")).unwrap()
        };
        let parameter: ParameterId = "test:model:parameter#length".try_into().unwrap();
        let mut owner = feature(
            DatumAxisDraft {
                native_options: std::collections::BTreeMap::from([(
                    crate::codec_name!("test:length"),
                    NativeOption::Length(Dimension::Parameter(parameter.clone())),
                )]),
                ..Default::default()
            }
            .into_operation()
            .unwrap(),
            1,
        );
        owner.product_definition = feature_scope.map(id);
        let mut ir = crate::CadIr::empty();
        for key in ["a", "b"]
            .into_iter()
            .filter(|key| feature_scope == Some(*key) || parameter_scope == Some(*key))
        {
            let mut definition = crate::products::ProductDefinition::new(
                id(key),
                crate::products::Storage::Embedded,
            );
            definition.kind = Some(crate::products::ProductDefinitionKind::Part);
            ir.model.product_definitions.push(definition);
        }
        ir.model
            .parameters
            .push(crate::features::parameters::DesignParameter {
                product_definition: parameter_scope.map(id),
                id: parameter,
                owner: None,
                ordinal: 0,
                name: "Length".into(),
                expression: "1 mm".into(),
                display: None,
                value: Some(crate::features::parameters::ParameterValue::Length(
                    crate::scalar::Length::new(1.0).unwrap(),
                )),
                dependencies: DistinctMembers::default(),
                properties: std::collections::BTreeMap::new(),
                pmi: None,
                native_ref: None,
            });
        let identity = owner.id.clone();
        ir.model.features.push(owner);
        crate::examples::add_document_root(&mut ir);
        let report = crate::validate::validate_neutral(&ir, Vec::new()).unwrap();
        if valid {
            assert!(
                !report
                    .findings
                    .iter()
                    .any(|finding| finding.severity == Severity::Error),
                "{feature_scope:?}/{parameter_scope:?}: {:?}",
                report.findings
            );
        }
        let errors = report
            .findings
            .iter()
            .filter(|finding| {
                finding.severity == Severity::Error
                    && finding.entity.as_deref() == Some(identity.as_str())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            errors.is_empty(),
            valid,
            "{feature_scope:?}/{parameter_scope:?}: {errors:?}"
        );
    }
}

#[test]
fn datum_axis_datum_parts_match_the_selected_known_family() {
    for (family, part, valid) in [
        (0, None, true),
        (
            1,
            Some(DatumPart::Principal(PrincipalEntity::XyPlane)),
            true,
        ),
        (
            0,
            Some(DatumPart::Principal(PrincipalEntity::XyPlane)),
            false,
        ),
        (0, Some(DatumPart::Point(0)), false),
        (1, Some(DatumPart::Point(0)), false),
        (2, Some(DatumPart::Point(0)), true),
        (2, Some(DatumPart::Point(1)), false),
    ] {
        let operation = if family == 1 {
            FeatureOperation::DatumCoordinateSystem {
                frame: crate::features::geometry::FeatureCoordinateFrame::new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            }
        } else if family == 2 {
            FeatureOperation::DatumPoint {
                position: origin(),
                construction: None,
            }
        } else {
            FeatureOperation::datum_axis(origin(), direction())
        };
        let datum = feature(operation, 1);
        let mut axis = consumer(picked(PickTarget::Datum {
            feature: datum.id.clone(),
            part,
        }));
        axis.dependencies.insert(datum.id.clone());
        let identity = axis.id.clone();
        let mut ir = crate::CadIr::empty();
        ir.model.features.extend([datum, axis]);
        let report = crate::validate::validate_neutral(&ir, Vec::new()).unwrap();
        if valid {
            assert!(
                !report
                    .findings
                    .iter()
                    .any(|finding| finding.severity == Severity::Error),
                "family={family}: {:?}",
                report.findings
            );
        }
        let errors = report
            .findings
            .iter()
            .filter(|finding| {
                finding.severity == Severity::Error
                    && finding.entity.as_deref() == Some(identity.as_str())
            })
            .collect::<Vec<_>>();
        assert_eq!(errors.is_empty(), valid, "family={family}: {errors:?}");
    }
}

#[test]
fn datum_axis_unresolved_point_part_keeps_its_cardinality_uncertainty() {
    let datum = feature(
        FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPoint,
        },
        1,
    );
    let mut axis = consumer(picked(PickTarget::Datum {
        feature: datum.id.clone(),
        part: Some(DatumPart::Point(9)),
    }));
    axis.dependencies.insert(datum.id.clone());
    let identity = axis.id.clone();
    let mut ir = crate::CadIr::empty();
    ir.model.features.extend([datum, axis]);
    let report = crate::validate::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(!report
        .findings
        .iter()
        .any(|finding| finding.severity == Severity::Error));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.check == Check::ReferentialIntegrity
            && finding.severity == Severity::Warning
            && finding.entity.as_deref() == Some(identity.as_str())));
}

#[test]
fn datum_axis_option_keys_have_one_classification_and_distinct_literals_survive() {
    let key = crate::codec_name!("test:model:parameter#one");
    let other = crate::codec_name!("test:placed:parameter#one");
    for equal_values in [false, true] {
        let draft = DatumAxisDraft {
            native_options: std::collections::BTreeMap::from([(
                key.clone(),
                NativeOption::Integer(Dimension::Literal(1)),
            )]),
            source_settings: std::collections::BTreeMap::from([(
                key.clone(),
                NativeOption::Integer(Dimension::Literal(if equal_values { 1 } else { 2 })),
            )]),
            ..Default::default()
        };
        assert!(draft.clone().into_operation().is_err());
        assert!(DatumAxis::try_from(draft).is_err());
    }
    let error = serde_json::from_str::<DatumAxis>(r#"{"inputs":[],"native_options":{"test:one":{"kind":"integer","value":{"kind":"literal","value":1}}},"source_settings":{"test:one":{"kind":"integer","value":{"kind":"literal","value":2}}}}"#).unwrap_err();
    assert!(error.to_string().contains("one classification"));
    let axis = DatumAxis::try_from(DatumAxisDraft {
        native_options: std::collections::BTreeMap::from([(
            key,
            NativeOption::Integer(Dimension::Literal(1)),
        )]),
        source_settings: std::collections::BTreeMap::from([(
            other,
            NativeOption::Integer(Dimension::Literal(2)),
        )]),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(axis.native_options().len(), 1);
    assert_eq!(axis.source_settings().len(), 1);
    let axis: DatumAxis =
        serde_json::from_str(&cadmpeg_core::serde_erase::json::to_string(&axis).unwrap()).unwrap();
    let replacements = std::collections::BTreeMap::from([(
        "test:model:parameter#one".to_owned(),
        "test:placed:parameter#one".to_owned(),
    )]);
    let replacements = TextReplacements::build(replacements.iter()).unwrap();
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut map = IdentityMap::new(|id: &str| Ok(id.replace("test:model:", "test:placed:")))
        .with_text_replacements(&replacements);
    let axis = axis.rewrite_identities(&ctx, &mut map).unwrap();
    map.finish().unwrap();
    assert!(axis
        .native_options()
        .contains_key("test:model:parameter#one"));
    assert!(axis
        .source_settings()
        .contains_key("test:placed:parameter#one"));
}
