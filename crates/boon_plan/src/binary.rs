use crate::PlanError;
use serde::Serialize;
use serde::ser::{
    self, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
    SerializeTupleStruct, SerializeTupleVariant,
};

pub(crate) fn encode<T>(value: &T) -> Result<Vec<u8>, PlanError>
where
    T: Serialize,
{
    let mut encoder = Encoder::default();
    value.serialize(&mut encoder)?;
    Ok(encoder.bytes)
}

#[derive(Default)]
struct Encoder {
    bytes: Vec<u8>,
}

impl Encoder {
    fn tag(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn len(&mut self, value: usize) {
        self.bytes.extend_from_slice(&(value as u64).to_le_bytes());
    }
}

struct Compound<'a> {
    encoder: &'a mut Encoder,
}

impl<'a> ser::Serializer for &'a mut Encoder {
    type Ok = ();
    type Error = PlanError;
    type SerializeSeq = Compound<'a>;
    type SerializeTuple = Compound<'a>;
    type SerializeTupleStruct = Compound<'a>;
    type SerializeTupleVariant = Compound<'a>;
    type SerializeMap = Compound<'a>;
    type SerializeStruct = Compound<'a>;
    type SerializeStructVariant = Compound<'a>;

    fn serialize_bool(self, value: bool) -> Result<(), PlanError> {
        self.tag(1);
        self.tag(u8::from(value));
        Ok(())
    }

    fn serialize_i8(self, value: i8) -> Result<(), PlanError> {
        self.serialize_i64(value as i64)
    }

    fn serialize_i16(self, value: i16) -> Result<(), PlanError> {
        self.serialize_i64(value as i64)
    }

    fn serialize_i32(self, value: i32) -> Result<(), PlanError> {
        self.serialize_i64(value as i64)
    }

    fn serialize_i64(self, value: i64) -> Result<(), PlanError> {
        self.tag(2);
        self.bytes.extend_from_slice(&value.to_le_bytes());
        Ok(())
    }

    fn serialize_u8(self, value: u8) -> Result<(), PlanError> {
        self.serialize_u64(value as u64)
    }

    fn serialize_u16(self, value: u16) -> Result<(), PlanError> {
        self.serialize_u64(value as u64)
    }

    fn serialize_u32(self, value: u32) -> Result<(), PlanError> {
        self.serialize_u64(value as u64)
    }

    fn serialize_u64(self, value: u64) -> Result<(), PlanError> {
        self.tag(3);
        self.bytes.extend_from_slice(&value.to_le_bytes());
        Ok(())
    }

    fn serialize_f32(self, value: f32) -> Result<(), PlanError> {
        self.tag(4);
        self.bytes.extend_from_slice(&value.to_bits().to_le_bytes());
        Ok(())
    }

    fn serialize_f64(self, value: f64) -> Result<(), PlanError> {
        self.tag(5);
        self.bytes.extend_from_slice(&value.to_bits().to_le_bytes());
        Ok(())
    }

    fn serialize_char(self, value: char) -> Result<(), PlanError> {
        self.serialize_u32(value as u32)
    }

    fn serialize_str(self, value: &str) -> Result<(), PlanError> {
        self.tag(6);
        self.len(value.len());
        self.bytes.extend_from_slice(value.as_bytes());
        Ok(())
    }

    fn serialize_bytes(self, value: &[u8]) -> Result<(), PlanError> {
        self.tag(7);
        self.len(value.len());
        self.bytes.extend_from_slice(value);
        Ok(())
    }

    fn serialize_none(self) -> Result<(), PlanError> {
        self.tag(8);
        Ok(())
    }

    fn serialize_some<T>(self, value: &T) -> Result<(), PlanError>
    where
        T: ?Sized + Serialize,
    {
        self.tag(9);
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<(), PlanError> {
        self.tag(10);
        Ok(())
    }

    fn serialize_unit_struct(self, name: &'static str) -> Result<(), PlanError> {
        self.tag(11);
        self.serialize_str(name)
    }

    fn serialize_unit_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
    ) -> Result<(), PlanError> {
        self.tag(12);
        self.serialize_str(name)?;
        self.serialize_u32(variant_index)?;
        self.serialize_str(variant)
    }

    fn serialize_newtype_struct<T>(self, name: &'static str, value: &T) -> Result<(), PlanError>
    where
        T: ?Sized + Serialize,
    {
        self.tag(13);
        self.serialize_str(name)?;
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<(), PlanError>
    where
        T: ?Sized + Serialize,
    {
        self.tag(14);
        self.serialize_str(name)?;
        self.serialize_u32(variant_index)?;
        self.serialize_str(variant)?;
        value.serialize(self)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Compound<'a>, PlanError> {
        self.tag(15);
        self.len(len.ok_or_else(|| PlanError::new("plan sequence length is unknown"))?);
        Ok(Compound { encoder: self })
    }

    fn serialize_tuple(self, len: usize) -> Result<Compound<'a>, PlanError> {
        self.tag(16);
        self.len(len);
        Ok(Compound { encoder: self })
    }

    fn serialize_tuple_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Compound<'a>, PlanError> {
        self.tag(17);
        self.serialize_str(name)?;
        self.len(len);
        Ok(Compound { encoder: self })
    }

    fn serialize_tuple_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Compound<'a>, PlanError> {
        self.tag(18);
        self.serialize_str(name)?;
        self.serialize_u32(variant_index)?;
        self.serialize_str(variant)?;
        self.len(len);
        Ok(Compound { encoder: self })
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Compound<'a>, PlanError> {
        self.tag(19);
        self.len(len.ok_or_else(|| PlanError::new("plan map length is unknown"))?);
        Ok(Compound { encoder: self })
    }

    fn serialize_struct(self, name: &'static str, len: usize) -> Result<Compound<'a>, PlanError> {
        self.tag(20);
        self.serialize_str(name)?;
        self.len(len);
        Ok(Compound { encoder: self })
    }

    fn serialize_struct_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Compound<'a>, PlanError> {
        self.tag(21);
        self.serialize_str(name)?;
        self.serialize_u32(variant_index)?;
        self.serialize_str(variant)?;
        self.len(len);
        Ok(Compound { encoder: self })
    }

    fn collect_str<T>(self, value: &T) -> Result<(), PlanError>
    where
        T: ?Sized + std::fmt::Display,
    {
        self.serialize_str(&value.to_string())
    }
}

impl SerializeSeq for Compound<'_> {
    type Ok = ();
    type Error = PlanError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), PlanError>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(&mut *self.encoder)
    }

    fn end(self) -> Result<(), PlanError> {
        Ok(())
    }
}

impl SerializeTuple for Compound<'_> {
    type Ok = ();
    type Error = PlanError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), PlanError>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(&mut *self.encoder)
    }

    fn end(self) -> Result<(), PlanError> {
        Ok(())
    }
}

impl SerializeTupleStruct for Compound<'_> {
    type Ok = ();
    type Error = PlanError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), PlanError>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(&mut *self.encoder)
    }

    fn end(self) -> Result<(), PlanError> {
        Ok(())
    }
}

impl SerializeTupleVariant for Compound<'_> {
    type Ok = ();
    type Error = PlanError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), PlanError>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(&mut *self.encoder)
    }

    fn end(self) -> Result<(), PlanError> {
        Ok(())
    }
}

impl SerializeMap for Compound<'_> {
    type Ok = ();
    type Error = PlanError;

    fn serialize_key<T>(&mut self, key: &T) -> Result<(), PlanError>
    where
        T: ?Sized + Serialize,
    {
        key.serialize(&mut *self.encoder)
    }

    fn serialize_value<T>(&mut self, value: &T) -> Result<(), PlanError>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(&mut *self.encoder)
    }

    fn end(self) -> Result<(), PlanError> {
        Ok(())
    }
}

impl SerializeStruct for Compound<'_> {
    type Ok = ();
    type Error = PlanError;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), PlanError>
    where
        T: ?Sized + Serialize,
    {
        key.serialize(&mut *self.encoder)?;
        value.serialize(&mut *self.encoder)
    }

    fn end(self) -> Result<(), PlanError> {
        Ok(())
    }
}

impl SerializeStructVariant for Compound<'_> {
    type Ok = ();
    type Error = PlanError;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), PlanError>
    where
        T: ?Sized + Serialize,
    {
        key.serialize(&mut *self.encoder)?;
        value.serialize(&mut *self.encoder)
    }

    fn end(self) -> Result<(), PlanError> {
        Ok(())
    }
}
