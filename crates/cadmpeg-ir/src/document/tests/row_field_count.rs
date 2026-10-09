// SPDX-License-Identifier: Apache-2.0

use serde::ser::{Impossible, SerializeStruct};
use serde::{Serialize, Serializer};

type Error = serde::de::value::Error;

/// Counts emitted top-level fields without traversing their values.
struct Fields {
    announced: usize,
    emitted: usize,
}

impl SerializeStruct for Fields {
    type Ok = usize;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        _key: &'static str,
        _value: &T,
    ) -> Result<(), Error> {
        self.emitted += 1;
        Ok(())
    }

    fn end(self) -> Result<usize, Error> {
        assert_eq!(self.announced, self.emitted);
        Ok(self.emitted)
    }
}

struct FieldCount;

macro_rules! unsupported {
    ($($name:ident ($($arg:ident: $ty:ty),*);)*) => {
        $(fn $name(self, $(_: $ty),*) -> Result<Self::Ok, Error> {
            Err(serde::ser::Error::custom("expected a struct"))
        })*
    };
}

impl Serializer for FieldCount {
    type Ok = usize;
    type Error = Error;
    type SerializeSeq = Impossible<usize, Error>;
    type SerializeTuple = Impossible<usize, Error>;
    type SerializeTupleStruct = Impossible<usize, Error>;
    type SerializeTupleVariant = Impossible<usize, Error>;
    type SerializeMap = Impossible<usize, Error>;
    type SerializeStruct = Fields;
    type SerializeStructVariant = Impossible<usize, Error>;

    unsupported! {
        serialize_bool(value: bool); serialize_i8(value: i8); serialize_i16(value: i16);
        serialize_i32(value: i32); serialize_i64(value: i64); serialize_i128(value: i128);
        serialize_u8(value: u8); serialize_u16(value: u16); serialize_u32(value: u32);
        serialize_u64(value: u64); serialize_u128(value: u128);
        serialize_f32(value: f32); serialize_f64(value: f64); serialize_char(value: char);
        serialize_str(value: &str); serialize_bytes(value: &[u8]);
        serialize_none(); serialize_unit(); serialize_unit_struct(name: &'static str);
        serialize_unit_variant(name: &'static str, index: u32, variant: &'static str);
    }
    fn serialize_some<T: Serialize + ?Sized>(self, _value: &T) -> Result<usize, Error> {
        Err(serde::ser::Error::custom("expected a struct"))
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _name: &'static str, _value: &T) -> Result<usize, Error> {
        Err(serde::ser::Error::custom("expected a struct"))
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(self, _name: &'static str, _index: u32, _variant: &'static str, _value: &T) -> Result<usize, Error> {
        Err(serde::ser::Error::custom("expected a struct"))
    }
    fn serialize_seq(self, _length: Option<usize>) -> Result<Self::SerializeSeq, Error> {
        Err(serde::ser::Error::custom("expected a struct"))
    }
    fn serialize_tuple(self, _length: usize) -> Result<Self::SerializeTuple, Error> {
        Err(serde::ser::Error::custom("expected a struct"))
    }
    fn serialize_tuple_struct(self, _name: &'static str, _length: usize) -> Result<Self::SerializeTupleStruct, Error> {
        Err(serde::ser::Error::custom("expected a struct"))
    }
    fn serialize_tuple_variant(self, _name: &'static str, _index: u32, _variant: &'static str, _length: usize) -> Result<Self::SerializeTupleVariant, Error> {
        Err(serde::ser::Error::custom("expected a struct"))
    }
    fn serialize_map(self, _length: Option<usize>) -> Result<Self::SerializeMap, Error> {
        Err(serde::ser::Error::custom("expected a struct"))
    }
    fn serialize_struct(self, _name: &'static str, length: usize) -> Result<Fields, Error> {
        Ok(Fields { announced: length, emitted: 0 })
    }
    fn serialize_struct_variant(self, _name: &'static str, _index: u32, _variant: &'static str, _length: usize) -> Result<Self::SerializeStructVariant, Error> {
        Err(serde::ser::Error::custom("expected a struct"))
    }
}

#[test]
fn solved_geometry_row_field_counts_match_optional_source_association() {
    use crate::geometry::{Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry};
    let source: crate::provenance::SourceObjectAssociation = serde_json::from_value(serde_json::json!({
        "format": "rhino", "object_id": "test-source:é"
    })).unwrap();
    for source_object in [None, Some(source)] {
        let count = 2 + usize::from(source_object.is_some());
        let surface = Surface {
            id: "test:model:surface#fields".try_into().unwrap(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: source_object.clone(),
        };
        let curve = Curve {
            id: "test:model:curve#fields".try_into().unwrap(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
            source_object,
        };
        assert_eq!(super::super::SurfaceWire(&surface).serialize(FieldCount).unwrap(), count);
        assert_eq!(super::super::CurveWire(&curve).serialize(FieldCount).unwrap(), count);
        assert_eq!(serde_json::to_value(super::super::SurfaceWire(&surface)).unwrap(), serde_json::to_value(surface).unwrap());
        assert_eq!(serde_json::to_value(super::super::CurveWire(&curve)).unwrap(), serde_json::to_value(curve).unwrap());
    }
}
