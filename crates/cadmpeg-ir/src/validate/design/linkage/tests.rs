// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_core::decode::ContentScope;
use serde_json::Map;

use crate::design::construction::{AxisValue, Construction};
use crate::design::container::Container;
use crate::design::feature::{Feature, FeatureOperation, Link, OperationFamily};
use crate::design::ops::datum::{Datum, DatumElement};
use crate::design::refs::Member;
use crate::features::FinitePoint3;
use crate::ids::Identity;
use crate::math::Point3;
use crate::units::UnitVector3;
use crate::validate::design::fixture::{
    assert_finding, assert_no_design_finding, document, feature, unresolved,
};
use crate::NativeRecord;

#[test]
fn the_baseline_document_breaks_no_rule() {
    let ir = document();
    let report = crate::validate::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(report.findings.is_empty(), "{:#?}", report.findings);
}

#[test]
fn design_records_outside_the_scope_break_the_scope_invariants() {
    let mut ir = document();
    let design = std::mem::take(&mut ir.design);
    ir.set_scope(ContentScope::geometry()).unwrap();
    ir.design = design;
    assert_finding(&ir, "breaks the scope invariants");
}

#[test]
fn design_identities_are_unique() {
    let mut ir = document();
    ir.design
        .features
        .push(unresolved("fillet", OperationFamily::Fillet));
    assert_finding(&ir, "is not unique among the design records");
}

/// A datum axis feature whose record and link name native records.
fn linked_datum(record: &str, link: &str) -> Feature {
    let mut datum = unresolved("datum", OperationFamily::Datum);
    datum.native_ref = Some(record.to_owned());
    datum.definition.operation = FeatureOperation::Datum(Box::new(Datum {
        element: DatumElement::Axis(Construction {
            inputs: Vec::new(),
            after: Vec::new(),
            solution: None,
            mode: None,
            resolved: Some(AxisValue {
                origin: FinitePoint3::new(Point3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                })
                .unwrap(),
                direction: UnitVector3::Z_AXIS,
            }),
            resolved_in: BTreeMap::new(),
        }),
        link: Some(Link {
            source: None,
            document: None,
            transform: None,
            scale: None,
            associative: None,
            state: None,
            native_ref: Some(link.to_owned()),
        }),
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }));
    datum
}

#[test]
fn a_datum_link_native_ref_resolves() {
    let mut ir = document();
    ir.native.namespace_mut("sldprt");
    ir.design.features.push(linked_datum(
        "sldprt:feature#missing",
        "sldprt:link#missing",
    ));
    assert_finding(&ir, "native_ref `sldprt:link#missing` does not resolve");

    let (record, link) = ("sldprt:native:feature#0", "sldprt:native:link#0");
    ir.native.namespace_mut("sldprt").arenas_mut().insert(
        "records".into(),
        [record, link]
            .into_iter()
            .map(|id| NativeRecord::new(Identity::new(id).unwrap(), Map::new()).unwrap())
            .collect(),
    );
    ir.native.finalize();
    ir.design.features.pop();
    ir.design.features.push(linked_datum(record, link));
    let Container::Ordered { members, .. } = &mut ir.design.containers[0] else {
        unreachable!()
    };
    members.insert(0, Member::Feature(feature("datum")));
    assert_no_design_finding(&ir);
}
