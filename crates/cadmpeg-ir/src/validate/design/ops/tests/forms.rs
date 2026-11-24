// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::{assert_finding, design_findings, with};
use crate::design::feature::FeatureOperation;
use crate::design::ops::surface::Form;
use crate::document::Topology;
use crate::subd::{SubdEdge, SubdEdgeTag};

#[test]
fn a_form_cage_meets_the_subdivision_scheme_rules() {
    let form = |intervals: [f64; 2]| {
        let mut cage = Topology::default();
        let mut subd = crate::examples::directed_subd_sum()
            .unwrap()
            .geometry
            .subds
            .remove(0);
        subd.cage
            .edit_edges(|edges| {
                edges[0] = SubdEdge::new(
                    [0, 1],
                    [0.25, 0.75],
                    SubdEdgeTag::Crease,
                    Some(intervals[0]),
                    [0.125, 0.875],
                )?;
                edges[1] = SubdEdge::new(
                    [1, 2],
                    [0.0, 0.5],
                    SubdEdgeTag::SmoothX,
                    Some(intervals[1]),
                    [0.25, 0.75],
                )?;
                Ok(())
            })
            .unwrap();
        let message = format!(
            "Form.cage: record {}: Catmull-Clark cage edges state different knot intervals",
            subd.id.as_str()
        );
        cage.subds.push(subd);
        let ir = with(FeatureOperation::Form(Box::new(Form {
            cage,
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        })));
        (ir, message)
    };
    let (ir, message) = form([1.0, 1.0]);
    assert!(design_findings(&ir)
        .iter()
        .all(|finding| !finding.message.starts_with("Form.cage")));
    let (ir, message_two) = form([1.0, 2.0]);
    assert_eq!(message, message_two);
    assert_finding(&ir, &message);
}
