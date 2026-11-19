// SPDX-License-Identifier: Apache-2.0
//! The v11 operation of one feature-tree node: the operation family its
//! native class establishes, or the native operation where none does.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::construction::{
    Construction, ConstructionInput, InputReference, InputRole, Relation, Subject,
};
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ops::datum::{Datum, DatumElement};
use cadmpeg_ir::design::options::NativeOptions;
use cadmpeg_ir::design::selection::{
    Anchor, Members, PickTarget, Picked, Picks, PrincipalEntity, Selection,
};

use crate::classification::{classify, principal_plane_with_siblings, FeatureClass};
use crate::history::classify::principal_plane_in_history;
use crate::records::{Feature, FeatureSource};

/// The class of the native base body, whose stored geometry is the feature's
/// definition.
const BASE_BODY_CLASS: &str = "moBaseBody_c";

/// The operation of `feature`, a node of `history` that is a feature.
///
/// A family whose operands this unit does not read is `Unresolved`. A base
/// body is `Stored` with origin `Base` once the state after it names its
/// bodies; until then its bodies are typed with the carrier unread, so it is
/// `Native`. A node of no established class is `Native`.
pub(super) fn operation(
    feature: &Feature,
    history: &[Feature],
    by_source: &HashMap<FeatureSource, &Feature>,
) -> FeatureOperation {
    if feature.input_class.as_deref() == Some(BASE_BODY_CLASS) {
        return native(feature);
    }
    if principal_plane_with_siblings(feature, history).is_some()
        || principal_plane_in_history(feature, by_source, history).is_some()
    {
        return unresolved(OperationFamily::Datum);
    }
    match classify(feature) {
        Some(class) => family(feature, class).map_or_else(|| native(feature), unresolved),
        None => native(feature),
    }
}

/// The datum of the Origin feature (§6.2): one point coincident with the
/// definition's principal origin.
pub(super) fn origin() -> FeatureOperation {
    let pick = Picked {
        target: PickTarget::Principal {
            entity: PrincipalEntity::Origin,
            container: None,
        },
        context: None,
        native: None,
        domain: None,
        witness: None,
        resolved: None,
    };
    let input = ConstructionInput {
        role: Some(InputRole {
            subject: Subject::Whole,
            relation: Relation::Coincident,
        }),
        reference: Some(InputReference::Picks(Selection {
            at: Anchor::Input,
            picks: Members::new(vec![pick]).map_or(Picks::Unread, Picks::Named),
            native: None,
        })),
        value: None,
        reversed: None,
    };
    FeatureOperation::Datum(Box::new(Datum {
        element: DatumElement::Points(vec![Construction {
            inputs: vec![input],
            after: Vec::new(),
            solution: None,
            mode: None,
            resolved: None,
            resolved_in: BTreeMap::new(),
        }]),
        link: None,
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    }))
}

fn unresolved(family: OperationFamily) -> FeatureOperation {
    FeatureOperation::Unresolved { family }
}

/// The native operation of `feature`, written `sldprt:<type>`.
fn native(feature: &Feature) -> FeatureOperation {
    let kind = NonBlankString::new(format!("sldprt:{}", feature.kind))
        .unwrap_or_else(|| cadmpeg_core::nonblank_literal!("sldprt:feature"));
    FeatureOperation::Native {
        kind,
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    }
}

/// The §6 operation family of a native feature class, or `None` for a class
/// part 6 gives no family of its own.
fn family(feature: &Feature, class: FeatureClass) -> Option<OperationFamily> {
    Some(match class {
        FeatureClass::Sketch => {
            if feature.kind.eq_ignore_ascii_case("3DSketch")
                || feature.input_class.as_deref() == Some("mo3DProfileFeature_c")
            {
                OperationFamily::SpatialSketch
            } else {
                OperationFamily::Sketch
            }
        }
        FeatureClass::SketchBlockDefinition | FeatureClass::SketchBlockInstance => return None,
        FeatureClass::ReferencePlane
        | FeatureClass::ReferenceAxis
        | FeatureClass::ReferencePoint
        | FeatureClass::CoordinateSystem => OperationFamily::Datum,
        FeatureClass::EquationCurve
        | FeatureClass::ProjectedCurve
        | FeatureClass::CompositeCurve
        | FeatureClass::Helix => OperationFamily::Curve,
        FeatureClass::Wrap => OperationFamily::Emboss,
        FeatureClass::Extrude => OperationFamily::Extrude,
        FeatureClass::Revolve => OperationFamily::Revolve,
        FeatureClass::Sweep => OperationFamily::Sweep,
        FeatureClass::Loft => OperationFamily::Loft,
        FeatureClass::Rib => OperationFamily::Rib,
        FeatureClass::Fillet => OperationFamily::Fillet,
        FeatureClass::Chamfer => OperationFamily::Chamfer,
        FeatureClass::Shell => OperationFamily::Shell,
        FeatureClass::Thicken => OperationFamily::Thicken,
        FeatureClass::Draft => OperationFamily::Draft,
        FeatureClass::Dome => OperationFamily::Dome,
        FeatureClass::Hole => OperationFamily::Hole,
        FeatureClass::CosmeticThread => OperationFamily::Thread,
        FeatureClass::OffsetSurface => OperationFamily::Offset,
        FeatureClass::KnitSurface => OperationFamily::Knit,
        FeatureClass::FilledSurface => OperationFamily::Patch,
        FeatureClass::ExtendSurface => OperationFamily::Extend,
        FeatureClass::RuledSurface => OperationFamily::ImplicitSweep,
        FeatureClass::TrimSurface | FeatureClass::SplitFace | FeatureClass::CutWithSurface => {
            OperationFamily::Split
        }
        FeatureClass::Combine => OperationFamily::Combine,
        FeatureClass::DeleteBody => OperationFamily::DeleteBodies,
        FeatureClass::DeleteFace => OperationFamily::DeleteFaces,
        FeatureClass::ReplaceFace => OperationFamily::ReplaceFaces,
        FeatureClass::MoveFace => OperationFamily::MoveFaces,
        FeatureClass::MoveBody | FeatureClass::Scale => OperationFamily::TransformBodies,
        FeatureClass::Flex => OperationFamily::Flex,
        FeatureClass::Pattern => OperationFamily::Pattern,
    })
}
