// SPDX-License-Identifier: Apache-2.0
//! Synthetic Design sketch stream records.

use crate::test_support::{lp_utf16, push_reference_u64};

pub(super) fn generated_design_bulkstream() -> (Vec<u8>, Vec<(u64, u64)>) {
    fn reference(relation: &mut [u8], at: usize, target: u32) {
        relation[at] = 1;
        relation[at + 1..at + 9].copy_from_slice(&u64::from(target).to_le_bytes());
    }

    fn close_current_point(out: &mut Vec<u8>, paired_reference: u32, owner_reference: u32) {
        out.extend_from_slice(&[0; 16]);
        out.push(1);
        out.extend_from_slice(&[0; 12]);
        out.extend_from_slice(&1.0f32.to_le_bytes());
        out.extend_from_slice(&1.0f32.to_le_bytes());
        out.extend_from_slice(&[0, 1, 0, 0, 0]);
        push_reference_u64(out, u64::from(paired_reference));
        push_reference_u64(out, u64::from(owner_reference));
    }

    fn push_point_companion(
        out: &mut Vec<u8>,
        class_tag: &str,
        record_index: u32,
        point_record_index: u32,
    ) {
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(class_tag.as_bytes());
        out.extend_from_slice(&record_index.to_le_bytes());
        out.extend_from_slice(&[0; 15]);
        push_reference_u64(out, u64::from(point_record_index));
    }

    let mut out = Vec::new();
    let mut records = vec![(899, 0)];
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"263");
    out.extend_from_slice(&899u32.to_le_bytes());
    out.extend_from_slice(&[0; crate::design::body::GENERATED_BODY_MAP_ZERO_PREFIX_LEN]);
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&42u64.to_le_bytes());
    out.extend_from_slice(&985u64.to_le_bytes());
    out.extend_from_slice(&1793u64.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    lp_utf16(&mut out, "BREP.synthetic.smbh");
    records.push((
        985,
        u64::try_from(out.len()).expect("synthetic Design record offset"),
    ));
    let node_guid = "ABCD0000-1111-8222-A333-444444444444";
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"256");
    out.extend_from_slice(&985u64.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
    lp_utf16(&mut out, "0_985");
    lp_utf16(&mut out, node_guid);
    out.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    lp_utf16(&mut out, "99999999-8888-8777-A666-555555555555");
    lp_utf16(
        &mut out,
        crate::design::presentation::PHYSICAL_MATERIAL_LIBRARY_ID,
    );
    lp_utf16(&mut out, "PrismMaterial-018");
    push_reference_u64(&mut out, 7);
    out.push(0);
    push_reference_u64(&mut out, 986);
    lp_utf16(&mut out, "Body");
    out.extend_from_slice(&1.0f32.to_le_bytes());
    out.extend_from_slice(&[1, 1]);
    lp_utf16(&mut out, "11111111-2222-3333-4444-555555555555");
    lp_utf16(&mut out, crate::design::presentation::APPEARANCE_LIBRARY_ID);
    lp_utf16(&mut out, "Prism-001");
    records.push((
        900,
        u64::try_from(out.len()).expect("synthetic Design record offset"),
    ));
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"260");
    out.extend_from_slice(&900u32.to_le_bytes());
    out.extend_from_slice(&[0; 10]);
    lp_utf16(&mut out, node_guid);
    out.extend_from_slice(&[0, 1, 1]);
    out.extend_from_slice(&985u64.to_le_bytes());
    records.push((
        277,
        u64::try_from(out.len()).expect("synthetic Design record offset"),
    ));
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"257");
    out.extend_from_slice(&277u64.to_le_bytes());
    out.extend_from_slice(&[0u8; 5]);
    out.push(1);
    out.extend_from_slice(&[0u8; 4]);
    lp_utf16(&mut out, "0_277");
    out.extend_from_slice(&584u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.push(1);
    out.extend_from_slice(&2u32.to_le_bytes());
    for reference in [33u32, 44] {
        out.push(1);
        out.extend_from_slice(&reference.to_le_bytes());
        out.extend_from_slice(&[0u8; 6]);
    }
    for (class_tag, record_index, members) in
        [("259", 33u32, [100u32, 200u32]), ("259", 44, [300, 400])]
    {
        records.push((
            u64::from(record_index),
            u64::try_from(out.len()).expect("synthetic Design record offset"),
        ));
        // A relation is `u8 1`, the counted `(reference, relation ordinal)`
        // pairs, the property-block presence byte, `ParentNode`, the u64
        // mask, the counted return run, and one zero byte.
        let mut relation = vec![0u8; 101];
        relation[0..4].copy_from_slice(&3u32.to_le_bytes());
        relation[4..7].copy_from_slice(class_tag.as_bytes());
        relation[7..11].copy_from_slice(&record_index.to_le_bytes());
        relation[19] = 1;
        relation[20..24].copy_from_slice(&2u32.to_le_bytes());
        reference(&mut relation, 24, members[0]);
        reference(&mut relation, 39, members[1]);
        reference(&mut relation, 55, 277);
        let state = if record_index == 33 { 0x10u64 } else { 0x04 };
        relation[66..74].copy_from_slice(&state.to_le_bytes());
        relation[74..78].copy_from_slice(&2u32.to_le_bytes());
        reference(&mut relation, 78, members[1]);
        reference(&mut relation, 89, members[0]);
        out.extend_from_slice(&relation);
    }
    for (record_index, persistent_id, coordinates) in [
        (100u32, 500u64, [1.25f64, -2.5f64]),
        (200, 501, [3.0, 4.0]),
        (300, 502, [-1.0, 0.5]),
        (400, 503, [2.0, 1.0]),
    ] {
        records.push((
            u64::from(record_index),
            u64::try_from(out.len()).expect("synthetic Design record offset"),
        ));
        let paired_reference = record_index + 1;
        let mut point = vec![0u8; 105];
        point[0..4].copy_from_slice(&3u32.to_le_bytes());
        point[4..7].copy_from_slice(b"266");
        point[7..11].copy_from_slice(&record_index.to_le_bytes());
        point[20] = 1;
        point[21..25].copy_from_slice(&1u32.to_le_bytes());
        point[25..29].copy_from_slice(&6u32.to_le_bytes());
        point[29..35].copy_from_slice(b"pt_tag");
        point[35..39].copy_from_slice(&23u32.to_le_bytes());
        point[39..62].copy_from_slice(b"IntrinsicMetaTypeuint64");
        point[62..70].copy_from_slice(&persistent_id.to_le_bytes());
        point[70] = 1;
        point[71..75].copy_from_slice(&paired_reference.to_le_bytes());
        point[89..97].copy_from_slice(&coordinates[0].to_le_bytes());
        point[97..105].copy_from_slice(&coordinates[1].to_le_bytes());
        close_current_point(&mut point, paired_reference, 277);
        out.extend_from_slice(&point);
        records.push((
            u64::from(paired_reference),
            u64::try_from(out.len()).expect("synthetic Design record offset"),
        ));
        push_point_companion(&mut out, "267", paired_reference, record_index);
    }
    records.push((
        600,
        u64::try_from(out.len()).expect("synthetic Design record offset"),
    ));
    let mut curve = vec![0u8; 229];
    curve[0..4].copy_from_slice(&3u32.to_le_bytes());
    curve[4..7].copy_from_slice(b"280");
    curve[7..11].copy_from_slice(&600u32.to_le_bytes());
    curve[20] = 1;
    curve[21..25].copy_from_slice(&2u32.to_le_bytes());
    curve[25..29].copy_from_slice(&14u32.to_le_bytes());
    curve[29..43].copy_from_slice(b"crv_primary_id");
    curve[43..47].copy_from_slice(&23u32.to_le_bytes());
    curve[47..70].copy_from_slice(b"IntrinsicMetaTypeuint64");
    curve[70..78].copy_from_slice(&440u64.to_le_bytes());
    curve[78..82].copy_from_slice(&16u32.to_le_bytes());
    curve[82..98].copy_from_slice(b"crv_secondary_id");
    curve[98..102].copy_from_slice(&23u32.to_le_bytes());
    curve[102..125].copy_from_slice(b"IntrinsicMetaTypeuint64");
    curve[125..133].copy_from_slice(&0u64.to_le_bytes());
    for (ordinal, value) in [
        1.0f64,
        2.0,
        0.0,
        0.0,
        0.0,
        1.0,
        1.0,
        0.0,
        0.0,
        3.0,
        0.0,
        std::f64::consts::PI,
    ]
    .into_iter()
    .enumerate()
    {
        let offset = 133 + ordinal * 8;
        curve[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    push_reference_u64(&mut curve, 277);
    out.extend_from_slice(&curve);
    records.push((
        700,
        u64::try_from(out.len()).expect("synthetic Design record offset"),
    ));
    let mut alternate_point = vec![0u8; 157];
    alternate_point[0..4].copy_from_slice(&3u32.to_le_bytes());
    alternate_point[4..7].copy_from_slice(b"266");
    alternate_point[7..11].copy_from_slice(&700u32.to_le_bytes());
    alternate_point[20] = 1;
    alternate_point[21..25].copy_from_slice(&2u32.to_le_bytes());
    alternate_point[25..29].copy_from_slice(&13u32.to_le_bytes());
    alternate_point[29..42].copy_from_slice(b"EntityGenesis");
    alternate_point[42..46].copy_from_slice(&23u32.to_le_bytes());
    alternate_point[46..69].copy_from_slice(b"IntrinsicMetaTypeuint64");
    alternate_point[69..77].copy_from_slice(&9u64.to_le_bytes());
    alternate_point[77..81].copy_from_slice(&6u32.to_le_bytes());
    alternate_point[81..87].copy_from_slice(b"pt_tag");
    alternate_point[87..91].copy_from_slice(&23u32.to_le_bytes());
    alternate_point[91..114].copy_from_slice(b"IntrinsicMetaTypeuint64");
    alternate_point[114..122].copy_from_slice(&600u64.to_le_bytes());
    alternate_point[122] = 1;
    alternate_point[123..127].copy_from_slice(&701u32.to_le_bytes());
    alternate_point[141..149].copy_from_slice(&(-4.0f64).to_le_bytes());
    alternate_point[149..157].copy_from_slice(&5.0f64.to_le_bytes());
    close_current_point(&mut alternate_point, 701, 277);
    out.extend_from_slice(&alternate_point);
    records.push((
        701,
        u64::try_from(out.len()).expect("synthetic Design record offset"),
    ));
    push_point_companion(&mut out, "267", 701, 700);

    records.push((
        800,
        u64::try_from(out.len()).expect("synthetic Design record offset"),
    ));
    let mut alternate_curve = vec![0u8; 443];
    alternate_curve[0..4].copy_from_slice(&3u32.to_le_bytes());
    alternate_curve[4..7].copy_from_slice(b"265");
    alternate_curve[7..11].copy_from_slice(&800u32.to_le_bytes());
    alternate_curve[20] = 1;
    alternate_curve[21..25].copy_from_slice(&3u32.to_le_bytes());
    alternate_curve[25..29].copy_from_slice(&13u32.to_le_bytes());
    alternate_curve[29..42].copy_from_slice(b"EntityGenesis");
    alternate_curve[42..46].copy_from_slice(&23u32.to_le_bytes());
    alternate_curve[46..69].copy_from_slice(b"IntrinsicMetaTypeuint64");
    alternate_curve[69..77].copy_from_slice(&10u64.to_le_bytes());
    alternate_curve[77..81].copy_from_slice(&14u32.to_le_bytes());
    alternate_curve[81..95].copy_from_slice(b"crv_primary_id");
    alternate_curve[95..99].copy_from_slice(&23u32.to_le_bytes());
    alternate_curve[99..122].copy_from_slice(b"IntrinsicMetaTypeuint64");
    alternate_curve[122..130].copy_from_slice(&700u64.to_le_bytes());
    alternate_curve[130..134].copy_from_slice(&16u32.to_le_bytes());
    alternate_curve[134..150].copy_from_slice(b"crv_secondary_id");
    alternate_curve[150..154].copy_from_slice(&23u32.to_le_bytes());
    alternate_curve[154..177].copy_from_slice(b"IntrinsicMetaTypeuint64");
    alternate_curve[177..185].copy_from_slice(&0u64.to_le_bytes());
    alternate_curve[185..193].copy_from_slice(&42u64.to_le_bytes());
    alternate_curve[193..197].copy_from_slice(&3u32.to_le_bytes());
    alternate_curve[197..200].copy_from_slice(b"365");
    alternate_curve[200..204].copy_from_slice(&800u32.to_le_bytes());
    alternate_curve[273] = 1;
    alternate_curve[275..279].copy_from_slice(&2u32.to_le_bytes());
    alternate_curve[279..287].copy_from_slice(&1.0e-9f64.to_le_bytes());
    alternate_curve[287..291].copy_from_slice(&6u32.to_le_bytes());
    alternate_curve[291..295].copy_from_slice(&6u32.to_le_bytes());
    alternate_curve[295..299].copy_from_slice(&8u32.to_le_bytes());
    for (ordinal, knot) in [0.0f64, 0.0, 0.0, 1.0, 1.0, 1.0].into_iter().enumerate() {
        let offset = 299 + ordinal * 8;
        alternate_curve[offset..offset + 8].copy_from_slice(&knot.to_le_bytes());
    }
    alternate_curve[347..351].copy_from_slice(&0u32.to_le_bytes());
    alternate_curve[351..355].copy_from_slice(&0u32.to_le_bytes());
    alternate_curve[355..359].copy_from_slice(&8u32.to_le_bytes());
    alternate_curve[359..363].copy_from_slice(&3u32.to_le_bytes());
    alternate_curve[363..367].copy_from_slice(&3u32.to_le_bytes());
    alternate_curve[367..371].copy_from_slice(&8u32.to_le_bytes());
    for (ordinal, coordinate) in [0.0f64, 0.0, 0.0, 1.0, 2.0, 0.0, 3.0, 1.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = 371 + ordinal * 8;
        alternate_curve[offset..offset + 8].copy_from_slice(&coordinate.to_le_bytes());
    }
    push_reference_u64(&mut alternate_curve, 277);
    out.extend_from_slice(&alternate_curve);
    records.push((
        9000,
        u64::try_from(out.len()).expect("synthetic BodiesRoot offset"),
    ));
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"258");
    out.extend_from_slice(&9000u32.to_le_bytes());
    out.extend_from_slice(&10u32.to_le_bytes());
    out.extend_from_slice(b"BodiesRoot");
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&10u32.to_le_bytes());
    out.extend_from_slice(b"BodiesRoot");
    out.extend_from_slice(&2u32.to_le_bytes());
    for entity_suffix in [985u64, 8422] {
        out.push(1);
        out.extend_from_slice(&entity_suffix.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out.push(0);
    records.push((
        123,
        u64::try_from(out.len()).expect("synthetic recipe offset"),
    ));
    let mut recipe_prefix = vec![0u8; 27];
    recipe_prefix[0..4].copy_from_slice(&3u32.to_le_bytes());
    recipe_prefix[4..7].copy_from_slice(b"264");
    recipe_prefix[7..11].copy_from_slice(&123u32.to_le_bytes());
    recipe_prefix[11..15].copy_from_slice(&123i32.to_le_bytes());
    recipe_prefix[23..27].copy_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&recipe_prefix);
    out.extend_from_slice(b"body_recipe_data");
    out.extend_from_slice(&(-1i64).to_le_bytes());
    for value in [2i32, 0, -1, 1, -1] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(b"pt_tag");
    out.extend_from_slice(&23u32.to_le_bytes());
    out.extend_from_slice(b"IntrinsicMetaTypeuint64");
    out.extend_from_slice(&439u64.to_le_bytes());
    records.push((
        4645,
        u64::try_from(out.len()).expect("synthetic lost edge offset"),
    ));
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"264");
    out.extend_from_slice(&4645u32.to_le_bytes());
    out.extend_from_slice(&[0; 14]);
    out.extend_from_slice(&19u32.to_le_bytes());
    out.extend_from_slice(b"EDGE_REFERENCE_LOST");
    records.push((
        4646,
        u64::try_from(out.len()).expect("synthetic lost edge end offset"),
    ));
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"264");
    out.extend_from_slice(&4646u32.to_le_bytes());
    out.extend_from_slice(b"body_recipe_data");
    (out, records)
}

/// Add a localized Sketch scope and its identity placement carrier to the
/// generated Design stream. The existing stream already carries the sketch
/// entity header, points, curves, and relations; this variant closes the
/// scope-to-placement join so the normal projection pipeline can materialize
/// that graph as a neutral Sketch.
pub(super) fn generated_design_sketch_bulkstream() -> (Vec<u8>, Vec<(u64, u64)>) {
    let (mut out, mut records) = generated_design_bulkstream();

    let scope_record = 1_100_u32;
    let placement_record = 584_u32;
    let scope_class_tag = 268_u32;
    let placement_class_tag = 269_u32;

    let scope_offset = u64::try_from(out.len()).expect("synthetic Sketch scope offset");
    out.extend_from_slice(&3_u32.to_le_bytes());
    out.extend_from_slice(scope_class_tag.to_string().as_bytes());
    out.extend_from_slice(&scope_record.to_le_bytes());
    out.extend_from_slice(&[0; 10]);
    out.extend_from_slice(&2_u32.to_le_bytes());
    for reference in [277_u32, placement_record] {
        out.push(1);
        out.extend_from_slice(&reference.to_le_bytes());
        out.extend_from_slice(&[0; 6]);
    }
    out.extend_from_slice(&1_u32.to_le_bytes());
    lp_utf16(&mut out, "Sketch");
    let mut tail = [0_u8; 78];
    tail[..4].copy_from_slice(&1_u32.to_le_bytes());
    tail[30..34].copy_from_slice(&u32::MAX.to_le_bytes());
    out.extend_from_slice(&tail);
    out.extend_from_slice(&3_u32.to_le_bytes());
    out.extend_from_slice(scope_class_tag.to_string().as_bytes());
    out.extend_from_slice(&scope_record.to_le_bytes());
    records.push((u64::from(scope_record), scope_offset));

    let placement_offset = u64::try_from(out.len()).expect("synthetic Sketch placement offset");
    out.extend_from_slice(&3_u32.to_le_bytes());
    out.extend_from_slice(placement_class_tag.to_string().as_bytes());
    out.extend_from_slice(&placement_record.to_le_bytes());
    out.extend_from_slice(&[0; 190]);
    out.extend_from_slice(&3_u32.to_le_bytes());
    out.extend_from_slice(b"261");
    out.extend_from_slice(&placement_record.to_le_bytes());
    records.push((u64::from(placement_record), placement_offset));

    (out, records)
}
