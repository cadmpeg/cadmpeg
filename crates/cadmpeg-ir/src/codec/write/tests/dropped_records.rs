// SPDX-License-Identifier: Apache-2.0
//! The CADIR writer drops and reports what it cannot write (CADIR v11 §3.4).

use super::*;

/// Plans a CADIR write of `ir`, which must not refuse, and returns the
/// messages of its losses of `code` and the document its bytes read back as.
fn written_with_losses(ir: &CadIr, code: &str) -> (Vec<String>, CadIr) {
    let plan = CadirEncoder
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .expect("the CADIR writer drops what it cannot write and does not refuse");
    let messages = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == code)
        .map(|loss| loss.message.clone())
        .collect();
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    let read = CadIr::from_json(std::str::from_utf8(&written).unwrap())
        .expect("the written bytes read back");
    (messages, read)
}

/// The CADIR wire leaves out the legacy features and their parent relations
/// (§3.4): a relation the model refuses is content the writer does not
/// write, so it charges no `export:record.invalid`, a write under
/// `--reject-errors` admits it, and the bytes equal those of the document
/// without the features.
#[test]
fn the_cadir_writer_does_not_check_feature_parent_relations_it_omits() {
    use crate::features::tree::{FeatureTreeNodeRole, TreeChildren};
    use crate::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

    const PARENT: &str = "test:writer:feature#0-parent";
    const CHILD: &str = "test:writer:feature#1-child";
    let feature = |id: &str, ordinal: u64, definition: FeatureDefinition| Feature {
        product_definition: None,
        id: FeatureId::mint(id).unwrap(),
        ordinal,
        name: None,
        suppressed: None,
        dependencies: crate::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),
        evaluation: crate::features::evaluation::FeatureEvaluation::from_definition(definition),
        native_ref: None,
    };
    let stored = || FeatureDefinition::Operation(FeatureOperation::StoredGeometry {});
    let tree_node = || {
        FeatureDefinition::Operation(FeatureOperation::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: TreeChildren::new(vec![FeatureId::mint(CHILD).unwrap()], None).unwrap(),
        })
    };
    let write = |ir: &CadIr| {
        let reject_errors = Strictness::default().with_code("export:record.invalid");
        let plan = CadirEncoder
            .plan(
                EncodeInput::new(ir, None).with_strictness(&reject_errors),
                TargetRequest::Inherit,
            )
            .expect("the writer checks no relation it omits");
        let invalid = plan
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.invalid")
            .map(|loss| loss.message.clone())
            .collect::<Vec<_>>();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        (bytes, invalid)
    };
    let (without_features, _) = write(&unit_cube().unwrap());
    for mutation in [
        "valid",
        "missing parent",
        "missing child",
        "ordinal",
        "tree ownership",
        "two tree parents",
    ] {
        let mut ir = unit_cube().unwrap();
        ir.model.features.push(feature(PARENT, 0, stored()));
        ir.model.features.push(feature(CHILD, 1, stored()));
        ir.model
            .set_feature_regeneration_parent(
                &FeatureId::mint(CHILD).unwrap(),
                &FeatureId::mint(PARENT).unwrap(),
            )
            .unwrap();
        let unit_cube_features = ir.model.features.len() - 2;
        match mutation {
            "valid" => {}
            "missing parent" => {
                ir.model.features.remove(unit_cube_features);
            }
            "missing child" => {
                ir.model.features.remove(unit_cube_features + 1);
            }
            "ordinal" => ir.model.features[unit_cube_features + 1].ordinal = 0,
            "tree ownership" => ir.model.features[unit_cube_features]
                .evaluation
                .set_definition(tree_node()),
            "two tree parents" => {
                ir.model
                    .features
                    .push(feature("test:writer:feature#2-node", 2, tree_node()));
                ir.model
                    .features
                    .push(feature("test:writer:feature#3-node", 3, tree_node()));
            }
            _ => unreachable!(),
        }
        let (bytes, invalid) = write(&ir);
        assert!(invalid.is_empty(), "{mutation}: {invalid:?}");
        assert!(bytes == without_features, "{mutation}");
    }
}

/// A procedural construction has one owning carrier (§3.4). The writer keeps
/// a construction that two carriers name with the first of them in identity
/// order and states the other as its retained cache (§3.3); it drops a
/// construction that no carrier names. Each charges one
/// `export:record.invalid` that names the construction, and the shared one
/// names both carriers. The two cube lines read back as lines.
#[test]
fn the_cadir_writer_keeps_one_owner_of_a_procedural_construction() {
    use crate::geometry::procedural_curves::ProceduralCurveDefinition;
    use crate::geometry::{CurveGeometry, ProceduralCurve, SolvedCurveGeometry};
    use crate::ids::ProceduralCurveId;

    const ORPHAN: &str = "test:writer:curve-construction#orphan";
    const SHARED: &str = "test:writer:curve-construction#shared";
    let mut ir = unit_cube().unwrap();
    let mut carriers = Vec::new();
    for carrier in &mut ir.geometry.curves[..2] {
        let CurveGeometry::Solved(solved) = carrier.geometry.clone() else {
            panic!("the unit cube's curves are solved");
        };
        assert!(matches!(solved, SolvedCurveGeometry::Line(_)));
        carrier.geometry = CurveGeometry::Procedural {
            construction: ProceduralCurveId::mint(SHARED).unwrap(),
            cache: Some(solved.clone()),
        };
        carriers.push((carrier.id.clone(), solved));
    }
    carriers.sort_by(|left, right| left.0.as_str().cmp(right.0.as_str()));
    for construction in [ORPHAN, SHARED] {
        ir.geometry.procedural_curves.push(ProceduralCurve::new(
            ProceduralCurveId::mint(construction).unwrap(),
            ProceduralCurveDefinition::Exact { cache: None },
        ));
    }
    assert!(ir.to_canonical_json().is_err());

    let (invalid, read) = written_with_losses(&ir, "record.invalid");
    assert_eq!(invalid.len(), 2, "{invalid:?}");
    let orphan = invalid.iter().find(|loss| loss.contains(ORPHAN)).unwrap();
    assert!(!orphan.contains(SHARED), "{orphan}");
    let shared = invalid.iter().find(|loss| loss.contains(SHARED)).unwrap();
    assert!(
        carriers
            .iter()
            .all(|(carrier, _)| shared.contains(carrier.as_str())),
        "{shared}"
    );
    let constructions = read
        .geometry
        .procedural_curves
        .iter()
        .map(|construction| construction.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(constructions, [SHARED]);
    assert_eq!(read.geometry.curves.len(), ir.geometry.curves.len());
    assert_eq!(read.geometry.edges.len(), ir.geometry.edges.len());
    let stated = |carrier: &crate::ids::CurveId| {
        read.geometry
            .curves
            .iter()
            .find(|curve| &curve.id == carrier)
            .unwrap()
            .geometry
            .clone()
    };
    let [(owner, owner_line), (released, released_line)] = carriers.as_slice() else {
        unreachable!();
    };
    assert_eq!(
        stated(owner),
        CurveGeometry::Procedural {
            construction: ProceduralCurveId::mint(SHARED).unwrap(),
            cache: Some(owner_line.clone()),
        }
    );
    assert_eq!(
        stated(released),
        CurveGeometry::Solved(released_line.clone())
    );
}

/// A stored state's delta is written as its own topology, so a construction
/// in it has one owning carrier of the delta (§3.4). The writer keeps a
/// shared construction with its first carrier and drops an unnamed one from
/// that delta alone: the evaluation and its other records stay.
#[test]
fn the_cadir_writer_keeps_a_stored_state_whose_delta_holds_an_unowned_construction() {
    use crate::geometry::procedural_curves::ProceduralCurveDefinition;
    use crate::geometry::{CurveGeometry, ProceduralCurve};
    use crate::ids::ProceduralCurveId;

    const ORPHAN: &str = "test:writer:curve-construction#orphan";
    const SHARED: &str = "test:writer:curve-construction#shared";
    let mut ir = crate::validate::design::fixture::document();
    let mut curves = ir.geometry.curves[..2].to_vec();
    let mut solved = Vec::new();
    for curve in &mut curves {
        let CurveGeometry::Solved(line) = curve.geometry.clone() else {
            panic!("the fixture's curves are solved");
        };
        curve.geometry = CurveGeometry::Procedural {
            construction: ProceduralCurveId::mint(SHARED).unwrap(),
            cache: Some(line.clone()),
        };
        solved.push((curve.id.clone(), line));
    }
    solved.sort_by(|left, right| left.0.as_str().cmp(right.0.as_str()));
    let evaluation = ir.design.evaluations[0].id.clone();
    let delta = &mut ir.design.evaluations[0]
        .content
        .as_mut()
        .expect("the fixture stores the state")
        .delta;
    let faces = delta.records.faces.len();
    delta.records.curves = curves;
    delta.records.procedural_curves = [ORPHAN, SHARED]
        .into_iter()
        .map(|construction| {
            ProceduralCurve::new(
                ProceduralCurveId::mint(construction).unwrap(),
                ProceduralCurveDefinition::Exact { cache: None },
            )
        })
        .collect();
    ir.finalize();
    assert!(ir.to_canonical_json().is_err());

    let (invalid, read) = written_with_losses(&ir, "record.invalid");
    assert_eq!(invalid.len(), 2, "{invalid:?}");
    assert!(
        invalid
            .iter()
            .all(|loss| loss.contains(evaluation.as_str())),
        "{invalid:?}"
    );
    let shared = invalid.iter().find(|loss| loss.contains(SHARED)).unwrap();
    assert!(
        solved
            .iter()
            .all(|(carrier, _)| shared.contains(carrier.as_str())),
        "{shared}"
    );
    assert!(
        invalid.iter().any(|loss| loss.contains(ORPHAN)),
        "{invalid:?}"
    );
    let stored = read
        .design
        .evaluations
        .iter()
        .find(|stored| stored.id == evaluation)
        .and_then(|stored| stored.content.as_ref())
        .expect("the evaluation keeps its stored state");
    let records = &stored.delta.records;
    assert_eq!(records.faces.len(), faces);
    let constructions = records
        .procedural_curves
        .iter()
        .map(|construction| construction.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(constructions, [SHARED]);
    let stated = |carrier: &crate::ids::CurveId| {
        records
            .curves
            .iter()
            .find(|curve| &curve.id == carrier)
            .unwrap()
            .geometry
            .clone()
    };
    let [(owner, owner_line), (released, released_line)] = solved.as_slice() else {
        unreachable!();
    };
    assert_eq!(
        stated(owner),
        CurveGeometry::Procedural {
            construction: ProceduralCurveId::mint(SHARED).unwrap(),
            cache: Some(owner_line.clone()),
        }
    );
    assert_eq!(
        stated(released),
        CurveGeometry::Solved(released_line.clone())
    );
}

/// An expression whose tree has no canonical text is a record CADIR cannot
/// write (§3.4): the writer drops the record that holds it, writes the
/// rest, and names the record.
#[test]
fn the_cadir_writer_drops_a_record_whose_expression_does_not_print() {
    use crate::design::expr::{Expression, Neutral, Node};
    use crate::design::parameter::ParameterDefinition;

    let mut ir = unit_cube().unwrap();
    ir.design.parameters.push(negated_parameter("kept", 1));
    let mut empty = negated_parameter("empty", 1);
    empty.definition =
        ParameterDefinition::Expression(Expression::Neutral(Neutral(Node::All(Vec::new()))));
    ir.design.parameters.push(empty);
    assert!(ir.to_canonical_json().is_err());

    let (invalid, read) = written_with_losses(&ir, "record.invalid");
    let [loss] = invalid.as_slice() else {
        panic!("one loss for the one record: {invalid:?}");
    };
    assert!(
        loss.contains("test:design:parameter#empty")
            && loss.contains("design.parameters")
            && !loss.contains("#kept"),
        "{loss}"
    );
    let kept = read
        .design
        .parameters
        .iter()
        .map(|parameter| parameter.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(kept, ["test:design:parameter#kept"]);
}

/// Sets each document-level field of `ir` to a value other than its default:
/// `fragment`, `repro`, source metadata and tolerances.
fn state_document_fields(ir: &mut CadIr) {
    ir.fragment = true;
    ir.source = Some(
        cadmpeg_core::serde_erase::json::from_value(serde_json::json!({
            "identity": {"kind": "unclassified", "format": "test"},
            "attributes": {},
        }))
        .unwrap(),
    );
    ir.tolerances.linear =
        crate::scalar::PositiveLength::new(ir.tolerances.linear.get() * 2.0).unwrap();
    let finding = Finding {
        rule: crate::rule_name!("test.unresolved-link"),
        check: Check::NativeLinks,
        severity: Severity::Error,
        entity: None,
        related: Vec::new(),
        message: "unresolved link".into(),
    };
    ir.repro = Some(Box::new(crate::repro::BundleMetadata {
        signature: crate::repro::Signature::new(
            &finding,
            crate::codec_id!("cadir"),
            crate::repro::Stage::Check,
        ),
        roots: vec!["test:design:parameter#root".to_owned()],
        context: crate::attributes::AttributeJson::Null,
        manifest: Vec::new(),
        incomplete_reason: None,
    }));
}

/// `written` holds no record and states each document-level field as `ir`
/// states it, except that `repro.roots` is empty: a root names a record of
/// the file (§3.4).
fn assert_document_fields_without_records(ir: &CadIr, written: &CadIr) {
    assert_eq!(written.entity_count(), 0);
    assert!(written.native.0.is_empty());
    assert_eq!(written.fragment, ir.fragment);
    let mut repro = ir.repro.clone();
    if let Some(repro) = &mut repro {
        repro.roots.clear();
    }
    assert_eq!(written.repro, repro);
    assert!(written.repro.is_some());
    assert_eq!(written.scope(), ir.scope());
    assert_eq!(written.source, ir.source);
    assert_eq!(written.tolerances, ir.tolerances);
}

/// An output from which every record was dropped is the CADIR document with
/// its document-level fields and no records, with `export:output.empty`
/// (§3.4): here the one record is refused.
#[test]
fn the_cadir_writer_writes_the_empty_document_where_it_drops_every_record() {
    use crate::design::expr::{Expression, Neutral, Node};
    use crate::design::parameter::ParameterDefinition;

    let mut ir = CadIr::empty();
    state_document_fields(&mut ir);
    let mut empty = negated_parameter("empty", 1);
    empty.definition =
        ParameterDefinition::Expression(Expression::Neutral(Neutral(Node::Any(Vec::new()))));
    ir.design.parameters.push(empty);

    let (losses, read) = written_with_losses(&ir, "output.empty");
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert_document_fields_without_records(&ir, &read);
}

/// A scope that excludes every record the document holds leaves an output
/// from which every record was dropped: the CADIR document with its
/// document-level fields, its scope among them, and no records, with
/// `export:output.empty` (§3.4).
#[test]
fn the_cadir_writer_writes_the_empty_document_where_the_scope_excludes_every_record() {
    let mut ir = CadIr::empty();
    state_document_fields(&mut ir);
    ir.design.parameters.push(negated_parameter("kept", 1));
    let mut value = cadmpeg_core::serde_erase::json::to_value(&ir).unwrap();
    value["scope"] = serde_json::json!({"model": true, "design": false, "process": false});
    let read = CadIr::read(
        value.to_string().as_bytes(),
        &crate::document::read::ReadOptions::default(),
    )
    .expect("the read keeps the scope as written");

    assert_ne!(
        read.document.scope(),
        cadmpeg_core::decode::ContentScope::full()
    );

    let (losses, written) = written_with_losses(&read.document, "output.empty");
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert_document_fields_without_records(&read.document, &written);
}

/// The CADIR wire leaves out the legacy model arenas (§3.4): a document whose
/// records all lie in them writes no record, so its output is the CADIR
/// document with its document-level fields and no records, with
/// `export:output.empty`.
#[test]
fn the_cadir_writer_writes_the_empty_document_for_records_the_wire_omits() {
    use crate::features::{
        Feature, FeatureContent, FeatureDefinition, FeatureId, FeatureOperation,
    };

    let mut ir = CadIr::empty();
    state_document_fields(&mut ir);
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint("test:writer:feature#legacy").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: crate::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: crate::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
        ),
        native_ref: None,
    });

    let (losses, written) = written_with_losses(&ir, "output.empty");
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert_document_fields_without_records(&ir, &written);
}
