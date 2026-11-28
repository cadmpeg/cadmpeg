// SPDX-License-Identifier: Apache-2.0
//! Equivalence tests verifying that `wire_walk` matches the legacy JSON
//! serialization + `pending.pop()` walk byte-for-byte in exact finding order.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::text::NonBlankString;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::walk_design;
use crate::design::configuration::{
    Arrangement, ArrangementKind, DesignConfiguration, FeatureStateOverride, MaterialOverride,
    MaterialRef, Overrides, ParameterOverride, Publication,
};
use crate::design::construction::{
    Construction, ConstructionInput, InputReference, InputRole, Relation, SourceMode, Subject,
};
use crate::design::container::{Container, OrderedKind, UnorderedKind};
use crate::design::dimension::{Dimension, ParameterRef, RefTarget};
use crate::design::evaluation::EvaluationStatus;
use crate::design::expr::{
    BinaryOp, Expression, Measure, MeasureKind, MeasureTarget, Neutral, Node,
};
use crate::design::feature::{Feature, FeatureDefinition, FeatureOperation};
use crate::design::ids::{
    ArrangementId, ContainerId, ParameterProgramId, PublicationId, SheetMetalRuleId,
    SpatialSketchConstraintId,
};
use crate::design::names::{CodecId, CodecName, NativeNameValue};
use crate::design::ops::shared::Span;
use crate::design::ops::sheet_metal::{BendSpec, SheetMetalRule};
use crate::design::ops::sketch::{
    EntityDefinition, Operand, PlanarCurve, Sketch, SketchConstraint, SketchEntity, SketchPlane,
    SketchRelation, SketchSolve, Spatial, SpatialCurve, SpatialSketch,
};
use crate::design::ops::swept::{Extrude, ExtrudeDirection, ExtrudeOrigin};
use crate::design::options::{NativeOption, NativeOptions, SolidOutput};
use crate::design::parameter::{DesignParameter, ParameterDefinition};
use crate::design::program::{ParameterProgram, ProgramBody, Statement};
use crate::design::selection::{
    Anchor, BoundaryUse, Fragment, Members, PickDomain, PickTarget, Picked, Picks, RuleTerm,
    Selection, SelectionRule, SketchRegion,
};
use crate::design::units::ValueKind;
use crate::document::Design;
use crate::features::geometry::FinitePoint3;
use crate::features::{ConfigurationId, FeatureId, ParameterId};
use crate::ids::ProductDefinitionId;
use crate::math::{Point2, Point3};
use crate::report::check::Check;
use crate::report::Severity;
use crate::scalar::Length;
use crate::sketches::{
    SketchConstraintId, SketchEntityId, SketchId, SpatialSketchEntityId, SpatialSketchId,
};
use crate::units::FinitePoint2;
use crate::validate::design::ops::{options, sketches};
use crate::validate::design::records::DesignRecords;

fn id<T: DeserializeOwned>(raw: &str) -> T {
    cadmpeg_core::serde_erase::from_json_value(&Value::String(format!("test:design:id#{raw}")))
        .unwrap()
}

fn codec() -> CodecId {
    CodecId::new("f3d").unwrap()
}

fn opt_map(keys: &[&str], region: Option<(&str, &str)>) -> NativeOptions {
    let mut map = NativeOptions::new();
    for key in keys {
        let val = match region {
            Some((sketch, ent)) if *key == "sel" => NativeOption::OrderedSelection(Selection {
                at: Anchor::Input,
                picks: Picks::Named(Members::new(vec![region_pick(sketch, ent)]).unwrap()),
                native: None,
            }),
            _ => NativeOption::Bool(Dimension::Literal(true)),
        };
        map.insert(CodecName::new(&codec(), key).unwrap(), val);
    }
    map
}

fn region_pick(sketch: &str, ent: &str) -> Picked {
    Picked {
        target: PickTarget::SketchRegion {
            sketch: id(sketch),
            region: SketchRegion {
                outer: vec![BoundaryUse {
                    entity: id(ent),
                    fragment: Some(Fragment { index: 0, count: 1 }),
                }],
                holes: Vec::new(),
            },
        },
        context: None,
        native: None,
        domain: None,
        witness: None,
        resolved: None,
    }
}

fn measure_region(sketch: &str, ent: &str) -> Measure {
    Measure {
        kind: MeasureKind::Length,
        of: MeasureTarget::Selection(Selection {
            at: Anchor::Current,
            picks: Picks::Named(Members::new(vec![region_pick(sketch, ent)]).unwrap()),
            native: None,
        }),
        cached: None,
    }
}

fn legacy_json_checks(
    design: &Design,
    index: &DesignRecords<'_>,
) -> Vec<(Check, Severity, Option<String>, String)> {
    let wire = cadmpeg_core::serde_erase::serialize_to_json_value(&|adapter| {
        serde::Serialize::serialize(design, adapter)
    })
    .unwrap();

    let mut findings = Vec::new();
    // Legacy check_native_keys
    let mut pending = vec![(&wire, "design")];
    while let Some((value, owner)) = pending.pop() {
        match value {
            Value::Object(map) => {
                let owner = map.get("id").and_then(Value::as_str).unwrap_or(owner);
                let keys = |field: &str| {
                    map.get(field)
                        .and_then(Value::as_object)
                        .map(|options| options.keys().map(String::as_str).collect::<Vec<_>>())
                        .unwrap_or_default()
                };
                let native = keys("native_options");
                let settings = keys("source_settings");
                for key in native.iter().filter(|key| settings.contains(key)) {
                    findings.push((
                        Check::Design,
                        Severity::Error,
                        Some(owner.to_owned()),
                        format!(
                            "key {key:?} is in both native_options and source_settings of one record"
                        ),
                    ));
                }
                pending.extend(map.values().map(|value| (value, owner)));
            }
            Value::Array(items) => pending.extend(items.iter().map(|value| (value, owner))),
            _ => {}
        }
    }

    // Legacy tagged(wire, "sketch_region") -> check_regions
    let mut found = Vec::new();
    let mut pending = vec![&wire];
    while let Some(value) = pending.pop() {
        match value {
            Value::Object(map) => {
                if map.get("kind").and_then(Value::as_str) == Some("sketch_region") {
                    found.push(value);
                }
                pending.extend(map.values());
            }
            Value::Array(items) => pending.extend(items),
            _ => {}
        }
    }
    for found in found {
        if let Ok(PickTarget::SketchRegion { sketch, region }) =
            cadmpeg_core::serde_erase::from_json_value::<PickTarget>(found)
        {
            let at = sketch.as_str();
            if region.outer.is_empty() {
                findings.push((
                    Check::Design,
                    Severity::Error,
                    Some(at.to_owned()),
                    "a SketchRegion's outer boundary is empty".to_owned(),
                ));
            }
            for boundary in region.outer.iter().chain(region.holes.iter().flatten()) {
                let entity = index.sketch_entities.get(boundary.entity.as_str()).copied();
                let valid = entity.is_some_and(|entity| {
                    entity.sketch == sketch && entity.construction != Some(true)
                });
                if !valid {
                    findings.push((
                        Check::Design,
                        Severity::Error,
                        Some(at.to_owned()),
                        format!(
                            "a SketchRegion boundary use names {}, which is no non-construction curve of the sketch",
                            boundary.entity.as_str()
                        ),
                    ));
                }
                if boundary
                    .fragment
                    .as_ref()
                    .is_some_and(|f| f.index >= f.count)
                {
                    findings.push((
                        Check::Design,
                        Severity::Error,
                        Some(at.to_owned()),
                        "a SketchRegion boundary fragment's index is not below its count"
                            .to_owned(),
                    ));
                }
            }
        }
    }
    findings
}

#[test]
fn wire_walk_matches_legacy_json_walk_identically() {
    let mut ir = crate::document::CadIr::empty();
    let def_id: ProductDefinitionId = id("def1");
    let fid: FeatureId = id("f1");
    let fid2: FeatureId = id("f2");
    let pid: ParameterId = id("p1");

    let mut limit_pick = region_pick("sk_lim_start", "bad_e1");
    limit_pick.domain = Some(PickDomain::Limits {
        start: Some(Box::new(region_pick("sk_dom_start", "bad_e2"))),
        end: Some(Box::new(region_pick("sk_dom_end", "bad_e3"))),
    });

    let extrude = FeatureOperation::Extrude(Box::new(Extrude {
        profile: Selection {
            at: Anchor::Input,
            picks: Picks::Rule {
                terms: vec![RuleTerm {
                    rule: SelectionRule::Explicit,
                    seeds: Members::new(vec![limit_pick]).unwrap(),
                    propagation: None,
                }],
                excluded: vec![RuleTerm {
                    rule: SelectionRule::Explicit,
                    seeds: Members::new(vec![region_pick("sk_excl", "bad_e4")]).unwrap(),
                    propagation: None,
                }],
                resolved: Some(Members::new(vec![region_pick("sk_res", "bad_e5")]).unwrap()),
            },
            native: None,
        },
        loops: None,
        direction: Some(ExtrudeDirection::ProfileNormal { reversed: None }),
        origin: Some(ExtrudeOrigin::ProfilePlane),
        span: Some(Span::Symmetric {
            amount: Dimension::Literal(Length::new(10.0).unwrap()),
            measure: None,
        }),
        draft: None,
        fit: None,
        outside: None,
        sheet_cut: None,
        output: SolidOutput {
            body: None,
            boolean: None,
        },
        native_options: opt_map(&["z_dup", "a_dup"], None),
        source_settings: opt_map(&["a_dup", "z_dup"], None),
    }));

    ir.design.features.push(Feature {
        id: fid.clone(),
        name: None,
        suppressed: None,
        definition: FeatureDefinition {
            operation: extrude,
            assembly: None,
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: Some(Box::new(Construction {
            inputs: vec![ConstructionInput {
                role: Some(InputRole {
                    subject: Subject::Whole,
                    relation: Relation::Coincident,
                }),
                reference: Some(InputReference::Picks(Selection {
                    at: Anchor::Input,
                    picks: Picks::Named(
                        Members::new(vec![region_pick("sk_place", "bad_e6")]).unwrap(),
                    ),
                    native: None,
                })),
                value: None,
                reversed: None,
            }],
            after: Vec::new(),
            solution: None,
            mode: Some(SourceMode {
                codec: codec(),
                method: NativeNameValue::Text(NonBlankString::new("m").unwrap()),
                native_options: opt_map(&["mode_dup"], None),
                source_settings: opt_map(&["mode_dup"], None),
            }),
            resolved: None,
            resolved_in: BTreeMap::new(),
        })),
        component: None,
        driven_by: None,
        native_ref: None,
    });

    ir.design.features.push(Feature {
        id: fid2.clone(),
        name: None,
        suppressed: None,
        definition: FeatureDefinition {
            operation: FeatureOperation::Native {
                kind: CodecName::new(&codec(), "custom").unwrap(),
                native_options: opt_map(&["sel", "nat_dup"], Some(("sk_nat", "bad_e7"))),
                source_settings: opt_map(&["nat_dup"], None),
            },
            assembly: None,
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: None,
    });

    ir.design.containers.push(Container::Ordered {
        id: id::<ContainerId>("c_ord"),
        kind: OrderedKind::Timeline,
        members: Vec::new(),
        folders: Vec::new(),
        rollback: None,
        placement: Some(Box::new(Construction {
            inputs: Vec::new(),
            after: Vec::new(),
            solution: None,
            mode: Some(SourceMode {
                codec: codec(),
                method: NativeNameValue::Text(NonBlankString::new("cm").unwrap()),
                native_options: opt_map(&["c_dup"], None),
                source_settings: opt_map(&["c_dup"], None),
            }),
            resolved: None,
            resolved_in: BTreeMap::new(),
        })),
    });
    ir.design.containers.push(Container::Unordered {
        id: id::<ContainerId>("c_unord"),
        kind: UnorderedKind::Group,
        members: BTreeSet::new(),
    });

    ir.design.parameters.push(DesignParameter {
        id: pid.clone(),
        scope: def_id.clone(),
        owner: None,
        position: None,
        name: None,
        kind: ValueKind::Integer,
        unit: None,
        definition: ParameterDefinition::Expression(Expression::Neutral(Neutral(Node::Binary {
            op: BinaryOp::Equal,
            left: Box::new(Node::Measure(Box::new(measure_region("sk_m1", "bad_m1")))),
            right: Box::new(Node::Measure(Box::new(measure_region("sk_m2", "bad_m2")))),
        }))),
        value: None,
        display: None,
        tolerance: None,
        decimals: None,
        comment: None,
        native_options: opt_map(&["p_dup"], None),
        source_settings: opt_map(&["p_dup"], None),
        native_ref: None,
    });

    ir.design.programs.push(ParameterProgram {
        id: id::<ParameterProgramId>("prog1"),
        scope: def_id.clone(),
        owner: None,
        stage: None,
        name: None,
        body: ProgramBody::Neutral(vec![Statement::Assign {
            target: ParameterRef {
                target: RefTarget::Parameter(pid.clone()),
                through: None,
            },
            value: Neutral(Node::Measure(Box::new(measure_region("sk_prog", "bad_p")))),
        }]),
        native_ref: None,
    });

    ir.design.publications.push(Publication {
        id: id::<PublicationId>("pub1"),
        scope: def_id.clone(),
        name: NonBlankString::new("P").unwrap(),
        target: region_pick("sk_pub", "bad_pub"),
    });

    ir.design.configurations.push(DesignConfiguration {
        id: id::<ConfigurationId>("cfg1"),
        scope: def_id.clone(),
        name: "C".to_owned(),
        position: None,
        parent: None,
        comment: None,
        overrides: Overrides {
            parameters: BTreeMap::from([(
                pid.clone(),
                ParameterOverride {
                    expression: Some(Expression::Neutral(Neutral(Node::Measure(Box::new(
                        measure_region("sk_cfg_param", "bad_cp"),
                    ))))),
                    value: None,
                },
            )]),
            features: BTreeMap::from([(
                fid.clone(),
                FeatureStateOverride {
                    suppressed: None,
                    definition: Some(FeatureDefinition {
                        operation: FeatureOperation::Native {
                            kind: CodecName::new(&codec(), "ov").unwrap(),
                            native_options: opt_map(&["cfg_ov_dup"], None),
                            source_settings: opt_map(&["cfg_ov_dup"], None),
                        },
                        assembly: None,
                        refine: None,
                        fuzzy_tolerance: None,
                    }),
                },
            )]),
            occurrences: BTreeMap::new(),
            definitions: BTreeMap::new(),
        },
        active_arrangements: BTreeSet::new(),
        materials: vec![MaterialOverride {
            target: Some(Selection {
                at: Anchor::Current,
                picks: Picks::Named(Members::new(vec![region_pick("sk_mat", "bad_mat")]).unwrap()),
                native: None,
            }),
            material: MaterialRef::Named(NonBlankString::new("Steel").unwrap()),
        }],
        native_ref: None,
    });

    ir.design.arrangements.push(Arrangement {
        id: id::<ArrangementId>("arr1"),
        scope: def_id.clone(),
        name: NonBlankString::new("A").unwrap(),
        kind: ArrangementKind::Positional,
        configuration: None,
        overrides: Overrides {
            parameters: BTreeMap::new(),
            features: BTreeMap::from([(
                fid.clone(),
                FeatureStateOverride {
                    suppressed: None,
                    definition: Some(FeatureDefinition {
                        operation: FeatureOperation::Native {
                            kind: CodecName::new(&codec(), "arr_ov").unwrap(),
                            native_options: opt_map(&["arr_ov_dup"], None),
                            source_settings: opt_map(&["arr_ov_dup"], None),
                        },
                        assembly: None,
                        refine: None,
                        fuzzy_tolerance: None,
                    }),
                },
            )]),
            occurrences: BTreeMap::new(),
            definitions: BTreeMap::new(),
        },
        appearances: Vec::new(),
        moves: Vec::new(),
        nested: Vec::new(),
        native_ref: None,
    });

    ir.design.sheet_metal_rules.push(SheetMetalRule {
        id: id::<SheetMetalRuleId>("smr1"),
        scope: def_id.clone(),
        name: None,
        thickness: None,
        bend: BendSpec {
            radius: None,
            radius_side: None,
            relief: None,
        },
        allowance: None,
        native_options: opt_map(&["smr_dup"], None),
        source_settings: opt_map(&["smr_dup"], None),
        native_ref: None,
    });

    ir.design.sketches.push(Sketch {
        id: id::<SketchId>("sk1"),
        owner: fid.clone(),
        name: None,
        plane: SketchPlane::Block,
        solve: SketchSolve {
            state: None,
            status: EvaluationStatus::Current,
        },
        native_options: opt_map(&["sk_dup"], None),
        source_settings: opt_map(&["sk_dup"], None),
        native_ref: None,
    });

    ir.design.sketch_entities.push(SketchEntity {
        id: id::<SketchEntityId>("se1"),
        sketch: id::<SketchId>("sk1"),
        definition: EntityDefinition::Free(PlanarCurve::Point(
            FinitePoint2::new(Point2::new(0.0, 0.0)).unwrap(),
        )),
        roles: Vec::new(),
        construction: Some(false),
        centerline: None,
        native: None,
        native_options: opt_map(&["se_dup"], None),
        source_settings: opt_map(&["se_dup"], None),
        native_ref: None,
    });

    ir.design.sketch_constraints.push(SketchConstraint {
        id: id::<SketchConstraintId>("sc1"),
        sketch: id::<SketchId>("sk1"),
        relation: SketchRelation::Fixed {
            of: Operand::Model(Box::new(Selection {
                at: Anchor::Input,
                picks: Picks::Named(Members::new(vec![region_pick("sk_con", "bad_con")]).unwrap()),
                native: None,
            })),
        },
        name: None,
        active: None,
        native_options: opt_map(&["sc_dup"], None),
        source_settings: opt_map(&["sc_dup"], None),
        native_ref: None,
    });

    ir.design.spatial_sketches.push(SpatialSketch {
        id: id::<SpatialSketchId>("ssk1"),
        owner: fid.clone(),
        name: None,
        solve: SketchSolve {
            state: None,
            status: EvaluationStatus::Current,
        },
        native_options: opt_map(&["ssk_dup"], None),
        source_settings: opt_map(&["ssk_dup"], None),
        native_ref: None,
    });

    ir.design
        .spatial_sketch_entities
        .push(SketchEntity::<Spatial> {
            id: id::<SpatialSketchEntityId>("sse1"),
            sketch: id::<SpatialSketchId>("ssk1"),
            definition: EntityDefinition::Free(SpatialCurve::Point(
                FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
            )),
            roles: Vec::new(),
            construction: None,
            centerline: None,
            native: None,
            native_options: opt_map(&["sse_dup"], None),
            source_settings: opt_map(&["sse_dup"], None),
            native_ref: None,
        });

    ir.design
        .spatial_sketch_constraints
        .push(SketchConstraint::<Spatial> {
            id: id::<SpatialSketchConstraintId>("ssc1"),
            sketch: id::<SpatialSketchId>("ssk1"),
            relation: SketchRelation::FixedLength { of: Vec::new() },
            name: None,
            active: None,
            native_options: opt_map(&["ssc_dup"], None),
            source_settings: opt_map(&["ssc_dup"], None),
            native_ref: None,
        });

    let records = DesignRecords::new(&ir);
    let legacy = legacy_json_checks(&ir.design, &records);

    let walked = walk_design(&ir.design);
    let mut typed_findings = Vec::new();
    options::check_native_keys(&walked.native_keys, &mut typed_findings);
    sketches::check_regions(&records, &walked.sketch_regions, &mut typed_findings);
    let typed: Vec<(Check, Severity, Option<String>, String)> = typed_findings
        .into_iter()
        .map(|f| (f.check, f.severity, f.entity, f.message))
        .collect();

    assert_eq!(typed, legacy);
    assert!(
        typed.len() >= 20,
        "expected rich coverage of both native_keys and sketch_regions, got {}",
        typed.len()
    );
}
