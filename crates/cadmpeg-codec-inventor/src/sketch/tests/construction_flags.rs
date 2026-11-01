// SPDX-License-Identifier: Apache-2.0
//! Entity-flag construction state of projected sketch curves.

use super::super::{
    project, PmDcSketchEntityKind, PmDcSketchEntityPayload, SketchInventory, SketchProjection,
};
use super::{closed_square_inventory, half_disc_inventory};
use crate::record_identity::Located;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

/// Projects `inventory` after setting the entity flags of its first
/// record that `select` accepts.
fn project_with_flags(
    mut inventory: SketchInventory,
    select: fn(&PmDcSketchEntityKind) -> bool,
    flags: u32,
) -> SketchProjection {
    let index = inventory
        .entities
        .iter()
        .position(|entity| select(&entity.kind))
        .expect("fixture entity");
    let mut payload = PmDcSketchEntityPayload::clone(&inventory.entities[index]);
    payload.entity_flags = flags;
    let identity = inventory.entities[index].identity.clone();
    inventory.entities[index] = Located::new(
        payload,
        identity.type_id.clone(),
        identity.segment_token.clone(),
        identity.record_ordinal,
    );
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    project(&ctx, &inventory, &[]).expect("sketch projection")
}

fn is_line(kind: &PmDcSketchEntityKind) -> bool {
    matches!(kind, PmDcSketchEntityKind::Line { .. })
}

fn is_circle(kind: &PmDcSketchEntityKind) -> bool {
    matches!(kind, PmDcSketchEntityKind::Circle { .. })
}

fn is_point(kind: &PmDcSketchEntityKind) -> bool {
    matches!(kind, PmDcSketchEntityKind::Point { .. })
}

#[test]
fn the_line_construction_bit_marks_a_line_as_construction_geometry() {
    for flags in [
        0x20_0000,
        0x20_0000 | 0x40,
        0x20_0000 | 0x40 | 0x8_0000,
        0x20_0000 | 0x200 | 0x8000,
    ] {
        let projected = project_with_flags(closed_square_inventory().0, is_line, flags);
        let marked = projected
            .entities
            .iter()
            .filter(|entity| entity.construction)
            .count();
        assert_eq!(marked, 1, "flags {flags:#x}");
        assert_eq!(projected.unresolved_construction, 0, "flags {flags:#x}");
        // The three remaining sides do not close a loop.
        assert!(projected.sketches[0].profiles.is_empty());
    }
}

#[test]
fn a_construction_line_with_an_unresolved_bit_keeps_the_unresolved_count() {
    let projected = project_with_flags(closed_square_inventory().0, is_line, 0x20_0000 | 0x100);
    assert_eq!(
        projected.entities.iter().filter(|e| e.construction).count(),
        1
    );
    assert_eq!(projected.unresolved_construction, 1);
}

#[test]
fn profile_wire_flag_bits_leave_a_line_in_the_profile_table() {
    for flags in [
        0,
        0x40,
        0x40 | 0x1000 | 0x4_0000,
        0x200,
        0x240,
        0x8000,
        0x8040,
    ] {
        let projected = project_with_flags(closed_square_inventory().0, is_line, flags);
        assert!(projected.entities.iter().all(|entity| !entity.construction));
        assert_eq!(projected.unresolved_construction, 0, "flags {flags:#x}");
        assert_eq!(projected.sketches[0].profiles.len(), 1, "flags {flags:#x}");
    }
}

#[test]
fn unresolved_flag_bits_keep_a_line_out_of_the_profile_table() {
    for flags in [0x20, 0x100, 0x800, 0x800_0000, 0x100_0000, 0x200 | 0x100] {
        let projected = project_with_flags(closed_square_inventory().0, is_line, flags);
        assert!(projected.entities.iter().all(|entity| !entity.construction));
        assert_eq!(projected.unresolved_construction, 1, "flags {flags:#x}");
        assert!(
            projected.sketches[0].profiles.is_empty(),
            "flags {flags:#x}"
        );
    }
}

#[test]
fn the_line_only_flag_bits_stay_unresolved_on_a_circular_arc() {
    for (flags, resolved) in [
        (0x200, true),
        (0x200 | 0x40, true),
        (0x8000, false),
        (0x20_0000, false),
    ] {
        let projected = project_with_flags(half_disc_inventory(), is_circle, flags);
        assert!(projected.entities.iter().all(|entity| !entity.construction));
        assert_eq!(
            projected.unresolved_construction,
            usize::from(!resolved),
            "flags {flags:#x}"
        );
        assert_eq!(
            projected.sketches[0].profiles.len(),
            usize::from(resolved),
            "flags {flags:#x}"
        );
    }
}

#[test]
fn the_construction_bit_on_a_point_does_not_mark_construction() {
    let projected = project_with_flags(closed_square_inventory().0, is_point, 0x20_0000);
    assert!(projected.entities.iter().all(|entity| !entity.construction));
    assert_eq!(projected.unresolved_construction, 0);
    assert_eq!(projected.sketches[0].profiles.len(), 1);
}
