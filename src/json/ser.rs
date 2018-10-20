use std::{error, fmt, io};

use serde::ser::{self, Serializer, Serialize, SerializeSeq, SerializeStructVariant,
                 SerializeStruct, SerializeMap, SerializeTupleVariant, SerializeTupleStruct,
                 SerializeTuple};
use ryu_ecmascript;
use base64;

use super::super::{is_f64_valid, is_i64_valid, is_u64_valid};

/// Everything that can go wrong during json serialization.
#[derive(Debug)]
pub enum EncodeJsonError {
    /// An IO error occured on the underlying writer.
    Io(io::Error),
    /// Tried to serialize a number forbidden by the ssb data format (an inifinity, NaN or -0.0).
    InvalidFloat(f64),
    /// Tried to serialize an unsigned integer larger than 2^53 (these are not
    /// guaranteed to be represented correctly in a 64 bit float).
    InvalidUnsignedInteger(u64),
    /// Tried to serialize an signed integer with absolute value larger than 2^53 (these are not
    /// guaranteed to be represented correctly in a 64 bit float).
    InvalidSignedInteger(i64),
    /// Can only serialize collections whose length is known upfront.
    UnknownLength,
    /// Custom, stringly-typed error.
    Message(String),
}

impl fmt::Display for EncodeJsonError {
    fn fmt(&self, f: &mut fmt::Formatter) -> std::result::Result<(), fmt::Error> {
        fmt::Debug::fmt(self, f)
    }
}

impl error::Error for EncodeJsonError {}

impl ser::Error for EncodeJsonError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        EncodeJsonError::Message(msg.to_string())
    }
}

impl From<io::Error> for EncodeJsonError {
    fn from(e: io::Error) -> Self {
        EncodeJsonError::Io(e)
    }
}

/// A structure for serializing data into the legacy json encoding.
pub struct JsonSerializer<W> {
    writer: W,
    // If true omits whitespace, else produces the signing format.
    compact: bool,
    indent: usize,
}

impl<W> JsonSerializer<W>
    where W: io::Write
{
    /// Creates a new serializer.
    ///
    /// If `compact`, this omits all whitespace. For signing or signature checking,
    /// set `compact` to `false`.
    #[inline]
    pub fn new(writer: W, compact: bool) -> Self {
        JsonSerializer {
            writer,
            compact,
            indent: 0,
        }
    }

    /// Unwrap the `Writer` from the `Serializer`.
    pub fn into_inner(self) -> W {
        self.writer
    }

    // Writes the correct number of spaces as indentation.
    fn write_indent(&mut self) -> Result<(), EncodeJsonError> {
        for _ in 0..self.indent {
            self.writer.write_all(b"  ")?;
        }
        Ok(())
    }
}

/// Serialize the given data structure as JSON into the IO stream.
pub fn to_writer<W, T: ?Sized>(writer: W, value: &T, compact: bool) -> Result<(), EncodeJsonError>
    where W: io::Write,
          T: Serialize
{
    let mut ser = JsonSerializer::new(writer, compact);
    value.serialize(&mut ser)
}

/// Serialize the given data structure  as JSON into a JSON byte vector.
pub fn to_vec<T: ?Sized>(value: &T, compact: bool) -> Vec<u8>
    where T: Serialize
{
    let mut writer = Vec::with_capacity(128);
    to_writer(&mut writer, value, compact).unwrap();
    writer
}

/// Serialize the given data structure as JSON into a `String`.
pub fn to_string<T: ?Sized>(value: &T, compact: bool) -> String
    where T: Serialize
{
    let vec = to_vec(value, compact);
    let string = unsafe {
        // We do not emit invalid UTF-8.
        String::from_utf8_unchecked(vec)
    };
    string
}

impl<'a, W> Serializer for &'a mut JsonSerializer<W>
    where W: io::Write
{
    type Ok = ();
    type Error = EncodeJsonError;

    type SerializeSeq = CollectionSerializer<'a, W>;
    type SerializeTuple = CollectionSerializer<'a, W>;
    type SerializeTupleStruct = CollectionSerializer<'a, W>;
    type SerializeTupleVariant = CollectionSerializer<'a, W>;
    type SerializeMap = CollectionSerializer<'a, W>;
    type SerializeStruct = CollectionSerializer<'a, W>;
    type SerializeStructVariant = CollectionSerializer<'a, W>;

    fn is_human_readable(&self) -> bool {
        !self.compact
    }

    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        let s = if v {
            b"true" as &[u8]
        } else {
            b"false" as &[u8]
        };
        Ok(self.writer.write_all(s)?)
    }

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        self.serialize_i64(v as i64)
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        self.serialize_i64(v as i64)
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        self.serialize_i64(v as i64)
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        if is_i64_valid(v) {
            Err(EncodeJsonError::InvalidSignedInteger(v))
        } else {
            self.serialize_f64(v as f64)
        }
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        self.serialize_u64(v as u64)
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        self.serialize_u64(v as u64)
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        self.serialize_u64(v as u64)
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        if is_u64_valid(v) {
            Err(EncodeJsonError::InvalidUnsignedInteger(v))
        } else {
            self.serialize_f64(v as f64)
        }
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        self.serialize_f64(v as f64)
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        if is_f64_valid(v) {
            let mut buffer = ryu_ecmascript::Buffer::new();
            let s = buffer.format::<f64>(v.into());
            Ok(self.writer.write_all(s.as_bytes())?)
        } else {
            Err(EncodeJsonError::InvalidFloat(v))
        }
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        self.serialize_str(&v.to_string())
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        self.writer.write_all(b"\"")?;

        for byte in v.bytes() {
            match byte {
                0x00 => self.writer.write_all(br"\u0000")?,
                0x01 => self.writer.write_all(br"\u0001")?,
                0x02 => self.writer.write_all(br"\u0002")?,
                0x03 => self.writer.write_all(br"\u0003")?,
                0x04 => self.writer.write_all(br"\u0004")?,
                0x05 => self.writer.write_all(br"\u0005")?,
                0x06 => self.writer.write_all(br"\u0006")?,
                0x07 => self.writer.write_all(br"\u0007")?,
                0x08 => self.writer.write_all(br"\b")?,
                0x09 => self.writer.write_all(br"\t")?,
                0x0A => self.writer.write_all(br"\n")?,
                0x0B => self.writer.write_all(br"\u000b")?,
                0x0C => self.writer.write_all(br"\f")?,
                0x0D => self.writer.write_all(br"\r")?,
                0x0E => self.writer.write_all(br"\u000e")?,
                0x0F => self.writer.write_all(br"\u000f")?,
                0x10 => self.writer.write_all(br"\u0010")?,
                0x11 => self.writer.write_all(br"\u0011")?,
                0x12 => self.writer.write_all(br"\u0012")?,
                0x13 => self.writer.write_all(br"\u0013")?,
                0x14 => self.writer.write_all(br"\u0014")?,
                0x15 => self.writer.write_all(br"\u0015")?,
                0x16 => self.writer.write_all(br"\u0016")?,
                0x17 => self.writer.write_all(br"\u0017")?,
                0x18 => self.writer.write_all(br"\u0018")?,
                0x19 => self.writer.write_all(br"\u0019")?,
                0x1A => self.writer.write_all(br"\u001a")?,
                0x1B => self.writer.write_all(br"\u001b")?,
                0x1C => self.writer.write_all(br"\u001c")?,
                0x1D => self.writer.write_all(br"\u001d")?,
                0x1E => self.writer.write_all(br"\u001e")?,
                0x1F => self.writer.write_all(br"\u001f")?,
                0x22 => self.writer.write_all(b"\\\"")?,
                0x5C => self.writer.write_all(br"\\")?,
                other => self.writer.write_all(&[other])?,
            }
        }

        self.writer
            .write_all(b"\"")
            .map_err(EncodeJsonError::Io)
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        self.serialize_str(&base64::encode(v))
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        self.serialize_unit()
    }

    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
        where T: ?Sized + Serialize
    {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Ok(self.writer.write_all(b"null")?)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        self.serialize_unit()
    }

    fn serialize_unit_variant(self,
                              _name: &'static str,
                              _variant_index: u32,
                              variant: &'static str)
                              -> Result<Self::Ok, Self::Error> {
        self.serialize_str(variant)
    }

    fn serialize_newtype_struct<T>(self,
                                   _name: &'static str,
                                   value: &T)
                                   -> Result<Self::Ok, Self::Error>
        where T: ?Sized + Serialize
    {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized>(self,
                                            _name: &'static str,
                                            _variant_index: u32,
                                            variant: &'static str,
                                            value: &T)
                                            -> Result<Self::Ok, Self::Error>
        where T: Serialize
    {
        self.writer.write_all(b"{")?;
        self.indent += 1;

        if !self.compact {
            self.writer.write_all(b"\n")?;
            self.write_indent()?;
        }

        variant.serialize(&mut *self)?;
        self.writer.write_all(b":")?;

        if !self.compact {
            self.writer.write_all(b" ")?;
        }

        value.serialize(&mut *self)?;

        if !self.compact {
            self.writer.write_all(b"\n")?;
            self.write_indent()?;
        }

        self.indent -= 1;
        self.writer.write_all(b"}").map_err(EncodeJsonError::Io)
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, EncodeJsonError> {
        match _len {
            None => return Err(EncodeJsonError::UnknownLength),
            Some(len) => {
                self.writer.write_all(b"[")?;
                self.indent += 1;
                Ok(CollectionSerializer::new(&mut *self, len == 0))
            }
        }
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, EncodeJsonError> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(self,
                              _name: &'static str,
                              len: usize)
                              -> Result<Self::SerializeTupleStruct, EncodeJsonError> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(self,
                               _name: &'static str,
                               _variant_index: u32,
                               variant: &'static str,
                               _len: usize)
                               -> Result<Self::SerializeTupleVariant, EncodeJsonError> {
        self.writer.write_all(b"{")?;
        self.indent += 1;

        if !self.compact {
            self.writer.write_all(b"\n")?;
            self.write_indent()?;
        }

        variant.serialize(&mut *self)?;
        self.writer.write_all(b":")?;

        if !self.compact {
            self.writer.write_all(b" ")?;
        }

        self.writer.write_all(b"[")?;
        self.indent += 1;

        Ok(CollectionSerializer::new(&mut *self, false))
    }

    fn serialize_map(self, len_: Option<usize>) -> Result<Self::SerializeMap, EncodeJsonError> {
        match len_ {
            None => return Err(EncodeJsonError::UnknownLength),
            Some(len) => {
                self.writer.write_all(b"{")?;
                self.indent += 1;
                Ok(CollectionSerializer::new(&mut *self, len == 0))
            }
        }
    }

    fn serialize_struct(self,
                        _name: &'static str,
                        len: usize)
                        -> Result<Self::SerializeStruct, EncodeJsonError> {
        self.serialize_map(Some(len))
    }

    fn serialize_struct_variant(self,
                                _name: &'static str,
                                _variant_index: u32,
                                variant: &'static str,
                                _len: usize)
                                -> Result<Self::SerializeStructVariant, EncodeJsonError> {
        self.writer.write_all(b"{")?;
        self.indent += 1;

        if !self.compact {
            self.writer.write_all(b"\n")?;
            self.write_indent()?;
        }

        variant.serialize(&mut *self)?;
        self.writer.write_all(b":")?;

        if !self.compact {
            self.writer.write_all(b" ")?;
        }

        self.writer.write_all(b"{")?;
        self.indent += 1;

        Ok(CollectionSerializer::new(&mut *self, false))
    }
}

#[doc(hidden)]
pub struct CollectionSerializer<'a, W> {
    ser: &'a mut JsonSerializer<W>,
    first: bool,
    empty: bool,
}

impl<'a, W: io::Write> CollectionSerializer<'a, W> {
    fn new(ser: &'a mut JsonSerializer<W>, empty: bool) -> CollectionSerializer<'a, W> {
        CollectionSerializer {
            ser,
            first: true,
            empty,
        }
    }
}

impl<'a, W> SerializeSeq for CollectionSerializer<'a, W>
    where W: io::Write
{
    type Ok = ();
    type Error = EncodeJsonError;

    fn serialize_element<T: ?Sized>(&mut self, value: &T) -> Result<(), Self::Error>
        where T: Serialize
    {
        if self.first {
            self.first = false;
        } else {
            self.ser.writer.write_all(b",")?;
        }

        if !self.ser.compact {
            self.ser.writer.write_all(b"\n")?;
            self.ser.write_indent()?;
        }

        value.serialize(&mut *self.ser)?;

        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        if !self.ser.compact {
            self.ser.indent -= 1;
            if !self.empty {
                self.ser.writer.write_all(b"\n")?;
                self.ser.write_indent()?;
            }
        }

        self.ser
            .writer
            .write_all(b"]")
            .map_err(EncodeJsonError::Io)
    }
}

impl<'a, W> SerializeTuple for CollectionSerializer<'a, W>
    where W: io::Write
{
    type Ok = ();
    type Error = EncodeJsonError;

    fn serialize_element<T: ?Sized>(&mut self, value: &T) -> Result<(), Self::Error>
        where T: Serialize
    {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        SerializeSeq::end(self)
    }
}

impl<'a, W> SerializeTupleStruct for CollectionSerializer<'a, W>
    where W: io::Write
{
    type Ok = ();
    type Error = EncodeJsonError;

    fn serialize_field<T: ?Sized>(&mut self, value: &T) -> Result<(), Self::Error>
        where T: Serialize
    {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        SerializeSeq::end(self)
    }
}

impl<'a, W> SerializeTupleVariant for CollectionSerializer<'a, W>
    where W: io::Write
{
    type Ok = ();
    type Error = EncodeJsonError;

    fn serialize_field<T: ?Sized>(&mut self, value: &T) -> Result<(), Self::Error>
        where T: Serialize
    {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        if !self.ser.compact {
            self.ser.indent -= 1;
            if !self.empty {
                self.ser.writer.write_all(b"\n")?;
                self.ser.write_indent()?;
            }
        }

        self.ser.writer.write_all(b"]")?;

        if !self.ser.compact {
            self.ser.indent -= 1;
            if !self.empty {
                self.ser.writer.write_all(b"\n")?;
                self.ser.write_indent()?;
            }
        }

        self.ser
            .writer
            .write_all(b"}")
            .map_err(EncodeJsonError::Io)
    }
}

impl<'a, W> SerializeMap for CollectionSerializer<'a, W>
    where W: io::Write
{
    type Ok = ();
    type Error = EncodeJsonError;

    fn serialize_key<T: ?Sized>(&mut self, key: &T) -> Result<(), Self::Error>
        where T: Serialize
    {
        if self.first {
            self.first = false;
        } else {
            self.ser.writer.write_all(b",")?;
        }

        if !self.ser.compact {
            self.ser.writer.write_all(b"\n")?;
            self.ser.write_indent()?;
        }

        key.serialize(&mut *self.ser)?;

        if self.ser.compact {
            self.ser
                .writer
                .write_all(b":")
                .map_err(EncodeJsonError::Io)
        } else {
            self.ser
                .writer
                .write_all(b": ")
                .map_err(EncodeJsonError::Io)
        }
    }

    fn serialize_value<T: ?Sized>(&mut self, value: &T) -> Result<(), Self::Error>
        where T: Serialize
    {
        value.serialize(&mut *self.ser)?;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        if !self.ser.compact {
            self.ser.indent -= 1;
            if !self.empty {
                self.ser.writer.write_all(b"\n")?;
                self.ser.write_indent()?;
            }
        }

        self.ser
            .writer
            .write_all(b"}")
            .map_err(EncodeJsonError::Io)
    }
}

impl<'a, W> SerializeStruct for CollectionSerializer<'a, W>
    where W: io::Write
{
    type Ok = ();
    type Error = EncodeJsonError;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), EncodeJsonError>
        where T: ?Sized + Serialize
    {
        SerializeMap::serialize_entry(self, key, value)
    }

    fn end(self) -> Result<(), EncodeJsonError> {
        SerializeMap::end(self)
    }
}

impl<'a, W> SerializeStructVariant for CollectionSerializer<'a, W>
    where W: io::Write
{
    type Ok = ();
    type Error = EncodeJsonError;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), EncodeJsonError>
        where T: ?Sized + Serialize
    {
        SerializeMap::serialize_entry(self, key, value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        if !self.ser.compact {
            self.ser.indent -= 1;
            if !self.empty {
                self.ser.writer.write_all(b"\n")?;
                self.ser.write_indent()?;
            }
        }

        self.ser.writer.write_all(b"}")?;

        if !self.ser.compact {
            self.ser.indent -= 1;
            if !self.empty {
                self.ser.writer.write_all(b"\n")?;
                self.ser.write_indent()?;
            }
        }

        self.ser
            .writer
            .write_all(b"}")
            .map_err(EncodeJsonError::Io)
    }
}
