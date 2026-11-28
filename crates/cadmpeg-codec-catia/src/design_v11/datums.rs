// SPDX-License-Identifier: Apache-2.0
//! The part's points, planes, and axis systems as datums (§6.2).

use std::collections::BTreeMap;

use cadmpeg_ir::design::construction::{
    kinds, Construction, ConstructionInput, InputReference, InputRole, Relation, Subject,
};
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::datum::{Datum, DatumElement};
use cadmpeg_ir::design::selection::{
    Anchor, Members, OrderedSelection, PickTarget, Picked, Picks, PrincipalEntity,
};
use cadmpeg_ir::features::PrincipalPlane;

/// The origin feature of one of the part's xy, yz, and zx planes: one
/// `Coincident` input on the matching principal plane.
pub(super) fn principal(plane: PrincipalPlane, at: Anchor) -> Option<FeatureOperation> {
    let entity = match plane {
        PrincipalPlane::Top => PrincipalEntity::XyPlane,
        PrincipalPlane::Right => PrincipalEntity::YzPlane,
        PrincipalPlane::Front => PrincipalEntity::XzPlane,
    };
    let pick = Picked {
        target: PickTarget::Principal {
            entity,
            container: None,
        },
        native: None,
        context: None,
        domain: None,
        witness: None,
        resolved: None,
    };
    let selection = OrderedSelection {
        at,
        picks: Picks::Named(Members::new(vec![pick]).ok()?),
        native: None,
    };
    let construction = Construction::<kinds::Plane> {
        inputs: vec![ConstructionInput {
            role: Some(InputRole {
                subject: Subject::Whole,
                relation: Relation::Coincident,
            }),
            reference: Some(InputReference::Picks(selection)),
            value: None,
            reversed: None,
        }],
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved: None,
        resolved_in: BTreeMap::default(),
    };
    Some(FeatureOperation::Datum(Box::new(Datum {
        element: DatumElement::Plane(construction),
        link: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    })))
}
