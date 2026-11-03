use crate::families::standard::records::{
    standard_surface_record_groups, standard_surface_records, surface_prefixes,
    StandardSurfaceRecord,
};

const SURFACES: [(u8, u8); 5] = [(0x32, 2), (0x33, 26), (0x34, 26), (0x35, 18), (0x38, 30)];

fn analytic_record(kind: u8, parameter_length: u8) -> Vec<u8> {
    let length = usize::from(parameter_length) + 47;
    let mut bytes = vec![1, 0, 0, 0, parameter_length, 0, 0x33, kind];
    bytes.resize(length, 0);
    bytes[length - 1] = 1;
    bytes.push(0x60);
    bytes
}

fn assert_strict_rejection(bytes: &[u8]) {
    assert_eq!(surface_prefixes(bytes).len(), 1);
    assert!(standard_surface_records(bytes, 1).is_none());
    assert!(crate::test_support::with_service_context(|ctx| {
        standard_surface_record_groups(ctx, bytes)
    })
    .expect("parse strict surface groups")
    .is_empty());
}

#[test]
fn strict_roster_rejects_nonfinite_bounds() {
    for (kind, parameter_length) in SURFACES {
        let mut bytes = analytic_record(kind, parameter_length);
        let bounds = usize::from(parameter_length) + 6;
        bytes[bounds..bounds + 4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert_strict_rejection(&bytes);
    }
}

#[test]
fn strict_roster_rejects_negative_extents() {
    for (kind, parameter_length) in SURFACES {
        let mut bytes = analytic_record(kind, parameter_length);
        let extent = usize::from(parameter_length) + 6 + 12;
        bytes[extent..extent + 4].copy_from_slice(&(-1.0f32).to_le_bytes());
        assert_strict_rejection(&bytes);
    }
}

#[test]
fn strict_roster_rejects_noncontaining_sphere() {
    for (kind, parameter_length) in SURFACES {
        let mut bytes = analytic_record(kind, parameter_length);
        let bounds = usize::from(parameter_length) + 6;
        bytes[bounds..bounds + 4].copy_from_slice(&1.0f32.to_le_bytes());
        assert_strict_rejection(&bytes);
    }
}

#[test]
fn strict_roster_rejects_nonfinite_curved_carrier() {
    for (kind, parameter_length) in SURFACES {
        if parameter_length == 2 {
            continue;
        }
        for offset in (8..usize::from(parameter_length) + 6).step_by(4) {
            for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                let mut bytes = analytic_record(kind, parameter_length);
                bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
                assert_strict_rejection(&bytes);
            }
        }
    }
}

#[test]
fn strict_roster_preserves_raw_zero_boundary_forms() {
    for (kind, parameter_length) in SURFACES {
        let bytes = analytic_record(kind, parameter_length);
        let records = standard_surface_records(&bytes, 1).expect("one raw surface record");
        assert_eq!(records.len(), 1);
        let StandardSurfaceRecord::Analytic(prefix) = records[0] else {
            panic!("analytic surface record");
        };
        assert_eq!(prefix.pos, 5);
        assert_eq!(prefix.target, 1);
        assert_eq!(prefix.kind.marker(), kind);
        assert_eq!(
            crate::test_support::with_service_context(|ctx| {
                standard_surface_record_groups(ctx, &bytes)
            })
            .expect("one raw surface group")
            .len(),
            1
        );
    }
}

#[test]
fn strict_roster_does_not_promote_embedded_freeform_after_analytic_rejection() {
    let mut bytes = analytic_record(0x33, 26);
    let mut freeform = vec![0x9a, 0x78, 0x56, 0, 0, 0];
    for value in [0.0f32, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 2.0] {
        freeform.extend_from_slice(&value.to_le_bytes());
    }
    freeform.push(1);
    assert_eq!(freeform.len(), 47);
    bytes[26..73].copy_from_slice(&freeform);
    bytes[8..12].copy_from_slice(&f32::NAN.to_be_bytes());
    assert_strict_rejection(&bytes);
}
