// SPDX-License-Identifier: Apache-2.0
//! Input selections retained by decoded datum operands.

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::names::{NativeName, NativeNameValue};
use cadmpeg_ir::design::selection::{Anchor, Members, OrderedSelection, PickTarget, Picked, Picks};
use cadmpeg_ir::features::selections::{EdgeSelection, FaceSelection, VertexSelection};

fn selection(target: PickTarget, native: Option<NativeName>) -> Option<OrderedSelection> {
    Some(OrderedSelection {
        at: Anchor::Input,
        picks: Picks::Named(
            Members::new(vec![Picked {
                target,
                native,
                context: None,
                domain: None,
                witness: None,
                resolved: None,
            }])
            .ok()?,
        ),
        native: None,
    })
}

pub(super) fn target(target: PickTarget) -> Option<OrderedSelection> {
    selection(target, None)
}

fn native(text: &str) -> Option<OrderedSelection> {
    selection(
        PickTarget::Native { within: None },
        Some(NativeName {
            codec: cadmpeg_core::nonblank_literal!("inventor").into(),
            value: NativeNameValue::Text(NonBlankString::new(text)?),
        }),
    )
}

pub(super) fn vertex(vertex: &VertexSelection) -> Option<OrderedSelection> {
    native(match vertex {
        VertexSelection::Native(reference)
        | VertexSelection::Generated {
            native: reference, ..
        } => reference.as_str(),
        VertexSelection::Historical { native, .. } => native.as_str(),
        VertexSelection::Unresolved => return None,
    })
}

pub(super) fn face(face: &FaceSelection) -> Option<OrderedSelection> {
    native(match face {
        FaceSelection::Native(reference)
        | FaceSelection::Resolved {
            native: reference, ..
        } => reference,
        FaceSelection::Historical { native, .. }
        | FaceSelection::HistoricalPartial { native, .. } => native.as_str(),
        FaceSelection::Generated { native, .. } => native.as_str(),
        FaceSelection::Unresolved | FaceSelection::Faces(_) => return None,
    })
}

pub(super) fn edge(edge: &EdgeSelection) -> Option<OrderedSelection> {
    native(match edge {
        EdgeSelection::Native(reference)
        | EdgeSelection::Resolved {
            native: reference, ..
        } => reference,
        EdgeSelection::Historical { native, .. }
        | EdgeSelection::HistoricalPartial { native, .. } => native.as_str(),
        EdgeSelection::Generated { native, .. } => native.as_str(),
        _ => return None,
    })
}
