// SPDX-License-Identifier: Apache-2.0
//! The reading half of §2.5's one zero: a float field is read by its value,
//! so `-0`, `-0.0`, and `0` read as 0.
//!
//! [`Zeroed`] forwards a `Deserializer` and wraps every visitor, seed, and
//! access it hands on, so each float the inner deserializer visits, at any
//! depth and native records included, reaches the reading type with negative
//! zero replaced by zero. An integer field keeps `serde_json`'s integer
//! reading: `1.0`, `1e0`, and `-0` are floats, and an integer field refuses
//! them.

use serde::de::{
    self, DeserializeSeed, Deserializer, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor,
};

/// Reads `text` as one JSON value of `T`, every float zero as 0.
pub(super) fn from_str<T: de::DeserializeOwned>(text: &str) -> Result<T, serde_json::Error> {
    let mut deserializer = crate::bounds::deserializer(text)?;
    let value = T::deserialize(Zeroed(&mut deserializer))?;
    deserializer.end()?;
    Ok(value)
}

/// A deserializer whose floats read negative zero as zero.
struct Zeroed<D>(D);

/// A visitor that receives negative zero as zero.
struct Visit<V>(V);

/// A seed whose value is read through [`Zeroed`].
struct Seed<S>(S);

/// A sequence, map, enum, or variant access whose values are read through
/// [`Zeroed`].
struct Access<A>(A);

macro_rules! forward_deserialize {
    ($($method:ident($($argument:ident: $type:ty),*)),* $(,)?) => {$(
        fn $method<V: Visitor<'de>>(
            self,
            $($argument: $type,)*
            visitor: V,
        ) -> Result<V::Value, Self::Error> {
            self.0.$method($($argument,)* Visit(visitor))
        }
    )*};
}

impl<'de, D: Deserializer<'de>> Deserializer<'de> for Zeroed<D> {
    type Error = D::Error;

    forward_deserialize!(
        deserialize_any(),
        deserialize_bool(),
        deserialize_i8(),
        deserialize_i16(),
        deserialize_i32(),
        deserialize_i64(),
        deserialize_i128(),
        deserialize_u8(),
        deserialize_u16(),
        deserialize_u32(),
        deserialize_u64(),
        deserialize_u128(),
        deserialize_f32(),
        deserialize_f64(),
        deserialize_char(),
        deserialize_str(),
        deserialize_string(),
        deserialize_bytes(),
        deserialize_byte_buf(),
        deserialize_option(),
        deserialize_unit(),
        deserialize_unit_struct(name: &'static str),
        deserialize_newtype_struct(name: &'static str),
        deserialize_seq(),
        deserialize_tuple(len: usize),
        deserialize_tuple_struct(name: &'static str, len: usize),
        deserialize_map(),
        deserialize_struct(name: &'static str, fields: &'static [&'static str]),
        deserialize_enum(name: &'static str, variants: &'static [&'static str]),
        deserialize_identifier(),
        deserialize_ignored_any(),
    );

    fn is_human_readable(&self) -> bool {
        self.0.is_human_readable()
    }
}

macro_rules! forward_visit {
    ($($method:ident($type:ty)),* $(,)?) => {$(
        fn $method<E: de::Error>(self, value: $type) -> Result<Self::Value, E> {
            self.0.$method(value)
        }
    )*};
}

impl<'de, V: Visitor<'de>> Visitor<'de> for Visit<V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.expecting(formatter)
    }

    forward_visit!(
        visit_bool(bool),
        visit_i8(i8),
        visit_i16(i16),
        visit_i32(i32),
        visit_i64(i64),
        visit_i128(i128),
        visit_u8(u8),
        visit_u16(u16),
        visit_u32(u32),
        visit_u64(u64),
        visit_u128(u128),
        visit_char(char),
        visit_str(&str),
        visit_borrowed_str(&'de str),
        visit_string(String),
        visit_bytes(&[u8]),
        visit_borrowed_bytes(&'de [u8]),
        visit_byte_buf(Vec<u8>),
    );

    fn visit_f32<E: de::Error>(self, value: f32) -> Result<Self::Value, E> {
        self.0.visit_f32(value + 0.0)
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
        self.0.visit_f64(value + 0.0)
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        self.0.visit_none()
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        self.0.visit_unit()
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        self.0.visit_some(Zeroed(deserializer))
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        self.0.visit_newtype_struct(Zeroed(deserializer))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, access: A) -> Result<Self::Value, A::Error> {
        self.0.visit_seq(Access(access))
    }

    fn visit_map<A: MapAccess<'de>>(self, access: A) -> Result<Self::Value, A::Error> {
        self.0.visit_map(Access(access))
    }

    fn visit_enum<A: EnumAccess<'de>>(self, access: A) -> Result<Self::Value, A::Error> {
        self.0.visit_enum(Access(access))
    }
}

impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for Seed<S> {
    type Value = S::Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        self.0.deserialize(Zeroed(deserializer))
    }
}

impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for Access<A> {
    type Error = A::Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Self::Error> {
        self.0.next_element_seed(Seed(seed))
    }

    fn size_hint(&self) -> Option<usize> {
        self.0.size_hint()
    }
}

impl<'de, A: MapAccess<'de>> MapAccess<'de> for Access<A> {
    type Error = A::Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Self::Error> {
        self.0.next_key_seed(Seed(seed))
    }

    fn next_value_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<T::Value, Self::Error> {
        self.0.next_value_seed(Seed(seed))
    }

    fn size_hint(&self) -> Option<usize> {
        self.0.size_hint()
    }
}

impl<'de, A: EnumAccess<'de>> EnumAccess<'de> for Access<A> {
    type Error = A::Error;
    type Variant = Access<A::Variant>;

    fn variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<(T::Value, Self::Variant), Self::Error> {
        self.0
            .variant_seed(Seed(seed))
            .map(|(value, variant)| (value, Access(variant)))
    }
}

impl<'de, A: VariantAccess<'de>> VariantAccess<'de> for Access<A> {
    type Error = A::Error;

    fn unit_variant(self) -> Result<(), Self::Error> {
        self.0.unit_variant()
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<T::Value, Self::Error> {
        self.0.newtype_variant_seed(Seed(seed))
    }

    fn tuple_variant<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.0.tuple_variant(len, Visit(visitor))
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.0.struct_variant(fields, Visit(visitor))
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::from_str;
    use crate::CadIr;

    #[derive(Debug, Deserialize)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    enum Tagged {
        Point { x: f64 },
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum External {
        Pair(f64, f64),
        Scale(f32),
    }

    #[derive(Debug, Deserialize)]
    struct Floats {
        plain: f64,
        optional: Option<f64>,
        list: Vec<f64>,
        tagged: Tagged,
        external: Vec<External>,
        open: serde_json::Value,
    }

    fn is_negative_zero(value: f64) -> bool {
        value == 0.0 && value.is_sign_negative()
    }

    #[test]
    fn a_float_field_reads_its_value_and_one_zero() {
        let text = r#"{
            "plain": -0,
            "optional": -0.0,
            "list": [-0.0, 0, -0, 1, 1.00, 1e0],
            "tagged": {"kind": "point", "x": -0.0},
            "external": [{"pair": [-0.0, -0]}, {"scale": -0.0}],
            "open": {"deep": [[-0.0]]}
        }"#;
        let bare: Floats = serde_json::from_str(text).unwrap();
        assert!(
            is_negative_zero(bare.plain),
            "the fixture holds a negative zero"
        );
        let read: Floats = from_str(text).unwrap();
        assert_eq!(read.plain.to_bits(), 0);
        assert_eq!(read.optional.map(f64::to_bits), Some(0));
        assert_eq!(
            read.list
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
            [0.0, 0.0, 0.0, 1.0, 1.0, 1.0].map(f64::to_bits)
        );
        let Tagged::Point { x } = read.tagged;
        assert_eq!(x.to_bits(), 0);
        let [External::Pair(first, second), External::Scale(scale)] = read.external.as_slice()
        else {
            panic!("two external variants: {:?}", read.external);
        };
        assert_eq!([first.to_bits(), second.to_bits()], [0, 0]);
        assert_eq!(scale.to_bits(), 0);
        assert_eq!(read.open["deep"][0][0].as_f64().map(f64::to_bits), Some(0));
    }

    #[test]
    fn an_integer_field_reads_an_integer_spelling_only() {
        assert_eq!(from_str::<i64>("-1").unwrap(), -1);
        assert_eq!(from_str::<u32>("0").unwrap(), 0);
        for spelling in ["1.0", "1e0", "-0", "-0.0"] {
            assert!(from_str::<i64>(spelling).is_err(), "{spelling}");
            assert!(from_str::<u32>(spelling).is_err(), "{spelling}");
        }
    }

    #[test]
    fn a_document_read_holds_zero_where_its_text_writes_negative_zero() {
        let text = crate::examples::unit_cube()
            .unwrap()
            .to_canonical_json()
            .unwrap();
        let negative = text.replace(": 0.0", ": -0.0");
        assert_ne!(negative, text, "the fixture writes a negative zero");
        let coordinates = |document: &CadIr| {
            document
                .geometry
                .points
                .iter()
                .flat_map(|point| {
                    let position = point.position();
                    [position.x, position.y, position.z]
                })
                .collect::<Vec<_>>()
        };
        let bare: CadIr = serde_json::from_str(&negative).unwrap();
        assert!(coordinates(&bare).into_iter().any(is_negative_zero));
        let read = CadIr::from_json(&negative).unwrap();
        assert!(!coordinates(&read).into_iter().any(is_negative_zero));
        assert_eq!(read.to_canonical_json().unwrap(), text);
    }
}
