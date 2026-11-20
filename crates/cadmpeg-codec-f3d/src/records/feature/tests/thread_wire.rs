use crate::records::feature::thread::{DesignThreadConstruction, DesignThreadNominalSize};

fn wire(text: &str, number: &str, angle: Option<&str>) -> String {
    let angle = angle.map_or_else(String::new, |angle| format!(",\"angle\":{angle}"));
    format!("{{\"form\":\"standard\",\"designation_offset\":38,\"designation\":\"G 1/8-28\",\"nominal_size_text\":\"{text}\",\"nominal_size\":{number},\"profile\":\"BSP Pipe Threads\",\"major_diameter\":1.0,\"minor_diameter\":0.5,\"pitch\":0.1,\"pitch_diameter\":0.75,\"modeled\":false{angle},\"face_group_record_indices\":[10]}}")
}

#[test]
fn a_fractional_nominal_size_keeps_its_spelling_and_value() {
    let json = wire("1/8", "0.125", None);
    let thread: DesignThreadConstruction = serde_json::from_str(&json).expect("fractional size");
    assert_eq!(thread.nominal_size.text(), "1/8");
    assert_eq!(thread.nominal_size.value(), Some(0.125));
    assert!(!thread.nominal_size.is_decimal());
    assert_eq!(serde_json::to_string(&thread).unwrap(), json);
    assert!(DesignThreadNominalSize::try_from("8".to_owned())
        .unwrap()
        .is_decimal());
    for text in ["0/8", "1/0", "1/x", "/8", "1/", "1/2/3", "-1/8", "1.5/2"] {
        assert!(
            DesignThreadNominalSize::try_from(text.to_owned()).is_err(),
            "{text}"
        );
    }
}

#[test]
fn the_thread_angle_defaults_to_sixty_degrees_and_round_trips_otherwise() {
    let sixty: DesignThreadConstruction =
        serde_json::from_str(&wire("1/8", "0.125", None)).unwrap();
    assert_eq!(sixty.angle.degrees(), 60.0);
    let json = wire("1/8", "0.125", Some("55.0"));
    let whitworth: DesignThreadConstruction = serde_json::from_str(&json).unwrap();
    assert_eq!(whitworth.angle.degrees(), 55.0);
    assert_eq!(serde_json::to_string(&whitworth).unwrap(), json);
    for angle in ["0.0", "180.0", "-55.0"] {
        assert!(
            serde_json::from_str::<DesignThreadConstruction>(&wire("1/8", "0.125", Some(angle)))
                .is_err(),
            "{angle}"
        );
    }
}
