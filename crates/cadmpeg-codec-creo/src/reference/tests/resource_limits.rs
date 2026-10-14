// SPDX-License-Identifier: Apache-2.0

const LINE: &[u8] = b"ent_list(line)\0\xe0\x00entity(line)\0\xf1\xe3\xf7\x11\
    \xf6\xe2\x02\x48\x10\x00\xeb\x10\x00\x00\x00\x00\x02\
    \x18\x41\x93\x8a\x07\xa0\xe6\xf8\x55\x8c\x3e\x32\xfb\x7f\x13\x0b\
    \x18\x93\x27\x14\x0f\x41\xcd\xf1\x8c\x3e\x32\xfb\x7f\x13\x0b\
    \xe0\x00entity(text)\0";
const LINE3D: &[u8] = b"ent_list(line3d)\0\x23\xe3\x23\x0d\xe2\x02\x48\x10\x00\
    \x0f\x0f\x0f\xe4\x0f\x0f\xe4";
const ARC_Z: &[u8] = b"ent_list(arc_z)\0\xe2\x2d\xe3\x2d\x0f\xe2\x01\
    \xe4\xe4\x0f\x0f\x43\xf0\x00\x0f\x0f\xe0\x00ent_list(line3d)\0";
const POSITIONAL_CONIC: &[u8] = b"ent_list(conic)\0\xf2\xf7\x0e\xe2\x2b\xe3\
    \x2b\x1e\xe2\x02\x48\x10\x00\xeb\x10\x00\x00\x00\x00\x01\
    \xe4\x0f\x0f\x43\xf0\x00\x0f\x0f\x0f\x11\x43\xf0\x00\xe4\
    \xe4\x0f\x0f\x0f\xe4\x0f\x0f\x0f\xe4\x43\xf0\x00\x0f\x0f\
    \xe2\x2c\xf7\x10\xe3\xe0\x00ent_list(text)\0";
const NAMED_CONIC: &[u8] = b"ent_list(conic)\0\
    \xe0\x01id\0\x2a\xe0\x01type\0\x1e\
    \xe0\x00gen_info\0\xe2\xf7\x13\x02\x48\x10\x00\xeb\x10\x00\x00\x00\x00\
    \xe0\x01flip\0\x01\
    \xe0\x02end1\0\xf8\x03\xe4\x0f\x0f\
    \xe0\x02end2\0\xf8\x03\x43\xf0\x00\x0f\x0f\
    \xe0\x02t0\0\x0f\xe0\x02t1\0\x11\
    \xe0\x02c1\0\x43\xf0\x00\xe0\x02c2\0\xe4\
    \xe0\x02local_sys\0\xf9\x04\x03\x18\xe4\x0f\xe4\x18\xe5\x0f\x18\xe6\
    \xf2\xf7\x0e\xe3";

#[test]
fn named_conic_image_decodes_one_conic() {
    assert_eq!(super::super::named_conics(NAMED_CONIC).len(), 1);
}

#[test]
fn positional_conic_image_decodes_one_conic() {
    assert_eq!(
        super::super::positional_conics(POSITIONAL_CONIC)
            .expect("positional conic image decodes")
            .len(),
        1
    );
}

#[test]
fn reference_line_image_decodes_one_line() {
    assert_eq!(super::super::lines(LINE).len(), 1);
}

#[test]
fn line3d_image_decodes_one_line() {
    assert_eq!(super::super::line3d_lines(LINE3D).len(), 1);
}

#[test]
fn arc_z_image_decodes_one_circle() {
    assert_eq!(super::super::arc_z_circles(ARC_Z).len(), 1);
}

#[test]
fn reference_ellipse_conic_yields_one_carrier() {
    let conic = super::super::ReferenceConic {
        entity_id: 7,
        type_id: super::super::ConicType::Ellipse,
        flip: 1,
        start: cadmpeg_ir::features::FinitePoint3::new([-3.0, 2.0, 4.0].into())
            .expect("finite start"),
        end: cadmpeg_ir::features::FinitePoint3::new([2.0, 4.0, 4.0].into()).expect("finite end"),
        parameter_start: None,
        parameter_end: None,
        coefficient_1: cadmpeg_ir::scalar::FiniteReal::new(-5.0).expect("finite coefficient"),
        coefficient_2: cadmpeg_ir::scalar::FiniteReal::new(2.0).expect("finite coefficient"),
        local_system: cadmpeg_ir::units::FiniteVector::new([
            1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 4.0,
        ]),
        body: Vec::new(),
        offset: 10,
    };
    let conics = [conic];
    assert_eq!(super::super::ellipse_carriers(&conics).len(), 1);
}

#[test]
fn arc_z_zero_body_yields_no_circle() {
    let body = [0; 64];
    let cache = crate::scalar::ScalarCache::from_section(&body);
    let circle = super::super::arc_z_fields(&body, &cache, 1);
    assert!(circle.is_none());
}
