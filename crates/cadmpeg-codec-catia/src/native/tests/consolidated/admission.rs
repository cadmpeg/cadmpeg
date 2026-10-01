use crate::test_support::test_b2::{
    b2_cylinder_stream, b2_implicit_axis_cylinder_stream, b2_range_origin_cylinder_stream,
    b2_embedded_cylinder_stream, b2_width_coded_owner_with_allocation_stream,
    b2_fixed_owner_boundary_cycle_stream,
};

#[test]
fn consolidated_cylinder_deserialization_admits_each_layout_chart() {
    let mut bytes = b2_cylinder_stream();
    bytes.extend_from_slice(&b2_implicit_axis_cylinder_stream());
    bytes.extend_from_slice(&b2_range_origin_cylinder_stream());
    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.consolidated_cylinders.len(), 3);
    for cylinder in &native.consolidated_cylinders {
        let valid = serde_json::to_value(cylinder).expect("cylinder wire");
        assert_eq!(serde_json::from_value::<crate::native::CatiaConsolidatedCylinder>(valid.clone()).expect("admitted chart"), *cylinder);
        for (field, value) in [
            ("axis", serde_json::json!([1.0, 0.0, 0.0])),
            ("reference_direction", serde_json::json!([1.0, 0.0, 0.0])),
            ("frame_token", serde_json::json!(255)),
            ("range_origin", serde_json::json!(0.0)),
        ] {
            if valid["payload"].get(field).is_none() { continue; }
            if valid["payload"][field] == value { continue; }
            let mut invalid = valid.clone();
            invalid["payload"][field] = value;
            assert!(serde_json::from_value::<crate::native::CatiaConsolidatedCylinder>(invalid).is_err(), "layout {} field {field}", valid["layout"]);
        }
        let mut invalid = valid;
        invalid["u_range"] = serde_json::json!([0.0, 0.1]);
        // A partial interval is valid only for the range-origin layout, whose redundant origin must also match.
        assert!(serde_json::from_value::<crate::native::CatiaConsolidatedCylinder>(invalid).is_err());
    }
}

#[test]
fn embedded_cylinder_deserialization_uses_the_layout_5a_admission() {
    let native = crate::native::CatiaNative::decode(&b2_embedded_cylinder_stream());
    assert!(!native.consolidated_embedded_cylinders.is_empty());
    for cylinder in &native.consolidated_embedded_cylinders {
        let valid = serde_json::to_value(cylinder).expect("embedded wire");
        assert_eq!(serde_json::from_value::<crate::native::CatiaConsolidatedEmbeddedCylinder>(valid.clone()).expect("admitted embedded chart"), *cylinder);
        for (field, value) in [
            ("frame_token", serde_json::json!(255)),
            ("axis", serde_json::json!([0.0, 0.0, 1.0])),
            ("reference_direction", serde_json::json!([1.0, 0.0, 0.0])),
            ("u_range", serde_json::json!([0.0, 0.1])),
        ] {
            let mut invalid = valid.clone();
            invalid[field] = value;
            assert!(serde_json::from_value::<crate::native::CatiaConsolidatedEmbeddedCylinder>(invalid).is_err(), "{field}");
        }
    }
}

#[test]
fn fixed_nine_wire_rejects_nested_metadata_instead_of_dropping_it() {
    let fixtures = [
        b2_width_coded_owner_with_allocation_stream().0,
        crate::test_support::test_b2::b2_owner_chart_stream(0x28),
        b2_fixed_owner_boundary_cycle_stream().0,
    ];
    for (bytes, field) in fixtures.into_iter().zip(["identity_targets", "owner_chart", "boundary_cycle"]) {
        let native = crate::native::CatiaNative::decode(&bytes);
        let packet = &native.consolidated_owner_packets[0];
        let valid = serde_json::to_value(packet).expect("fixed-nine wire");
        assert!(valid.get(field).is_some(), "fixture carries {field}");
        assert!(serde_json::from_value::<crate::native::CatiaConsolidatedOwnerPacket>(valid.clone()).is_ok());
        for remove_top in [true, false] {
            let mut invalid = valid.clone();
            let nested = invalid[field].clone();
            invalid["payload"][field] = nested;
            if remove_top { invalid.as_object_mut().expect("packet object").remove(field); }
            let error = serde_json::from_value::<crate::native::CatiaConsolidatedOwnerPacket>(invalid).expect_err("nested metadata is refused");
            assert!(error.to_string().contains("packet level"));
        }
    }
}

#[test]
fn counted_owner_tail_deserialization_rejects_empty_and_retains_arbitrary_nonempty_bytes() {
    use crate::families::b2::counted_owner_tail::CountedOwnerTail;
    assert!(CountedOwnerTail::new(vec![]).is_none());
    assert!(serde_json::from_value::<CountedOwnerTail>(serde_json::json!("")).is_err());
    for bytes in [vec![0], vec![3], vec![0x83, 0x41, 0x92, 0, 1]] {
        let tail = CountedOwnerTail::new(bytes.clone()).expect("nonempty class-0x62 tail");
        let wire = serde_json::to_value(&tail).expect("tail wire");
        let decoded: CountedOwnerTail = serde_json::from_value(wire).expect("admitted tail");
        assert_eq!(decoded.as_slice(), bytes);
    }
}
