use super::super::CurveOffsetCoordinate;

#[test]
fn offset_coordinate_admission_and_wire_accept_exactly_three_coordinates() {
    for raw in u8::MIN..=u8::MAX {
        let constructed = CurveOffsetCoordinate::try_new(raw);
        let decoded = serde_json::from_str::<CurveOffsetCoordinate>(&raw.to_string());
        if (1..=3).contains(&raw) {
            let value = constructed.unwrap();
            assert_eq!(value.get(), raw);
            assert_eq!(decoded.unwrap(), value);
            assert_eq!(serde_json::to_string(&value).unwrap(), raw.to_string());
        } else {
            assert!(constructed.is_err());
            assert!(decoded.is_err());
        }
    }
}
