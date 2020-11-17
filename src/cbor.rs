//! Simple and easy CBOR serialization.

use std::{
    convert::{TryFrom, TryInto},
    io,
};

use crate::{Error, Result};

/// Recursion limit for nested Cbor objects.
const RECURSION_LIMIT: u32 = 1000;

/// Cbor type parametrised over list type and map type. Use one of the
/// conversion trait to convert language-native-type to a Cbor variant.
#[derive(Clone)]
pub enum Cbor {
    Major0(Info, u64),              // uint 0-23,24,25,26,27
    Major1(Info, u64),              // nint 0-23,24,25,26,27
    Major2(Info, Vec<u8>),          // byts 0-23,24,25,26,27,31
    Major3(Info, Vec<u8>),          // text 0-23,24,25,26,27,31
    Major4(Info, Vec<Cbor>),        // list 0-23,24,25,26,27,31
    Major5(Info, Vec<(Key, Cbor)>), // dict 0-23,24,25,26,27,31
    Major6(Info, Tag),              // tags similar to major0
    Major7(Info, SimpleValue),      // type refer SimpleValue
}

impl Cbor {
    /// Serialize this cbor value.
    pub fn encode(self, buf: &mut Vec<u8>) -> Result<usize> {
        self.do_encode(buf, 1)
    }

    fn do_encode(&self, buf: &mut Vec<u8>, depth: u32) -> Result<usize> {
        if depth > RECURSION_LIMIT {
            return err_at!(FailCbor, msg: "encode recursion limit exceeded");
        }

        let major = self.to_major_val();
        match self {
            Cbor::Major0(info, num) => {
                let n = encode_hdr(major, *info, buf)?;
                Ok(n + encode_addnl(*num, buf)?)
            }
            Cbor::Major1(info, num) => {
                let n = encode_hdr(major, *info, buf)?;
                Ok(n + encode_addnl(*num, buf)?)
            }
            Cbor::Major2(info, byts) => {
                let n = encode_hdr(major, *info, buf)?;
                let m = encode_addnl(err_at!(FailConvert, u64::try_from(byts.len()))?, buf)?;
                buf.copy_from_slice(&byts);
                Ok(n + m + byts.len())
            }
            Cbor::Major3(info, text) => {
                let n = encode_hdr(major, *info, buf)?;
                let m = encode_addnl(err_at!(FailCbor, u64::try_from(text.len()))?, buf)?;
                buf.copy_from_slice(&text);
                Ok(n + m + text.len())
            }
            Cbor::Major4(info, list) => {
                let n = encode_hdr(major, *info, buf)?;
                let m = encode_addnl(err_at!(FailConvert, u64::try_from(list.len()))?, buf)?;
                let mut acc = 0;
                for x in list.iter() {
                    acc += x.do_encode(buf, depth + 1)?;
                }
                Ok(n + m + acc)
            }
            Cbor::Major5(info, map) => {
                let n = encode_hdr(major, *info, buf)?;
                let m = encode_addnl(err_at!(FailConvert, u64::try_from(map.len()))?, buf)?;
                let mut acc = 0;
                for (key, val) in map.iter() {
                    let key: Cbor = key.clone().try_into()?;
                    acc += key.do_encode(buf, depth + 1)?;
                    acc += val.do_encode(buf, depth + 1)?;
                }
                Ok(n + m + acc)
            }
            Cbor::Major6(info, tagg) => {
                let n = encode_hdr(major, *info, buf)?;
                let m = tagg.encode(buf)?;
                Ok(n + m)
            }
            Cbor::Major7(info, sval) => {
                let n = encode_hdr(major, *info, buf)?;
                let m = sval.encode(buf)?;
                Ok(n + m)
            }
        }
    }

    /// Deserialize a bytes from reader `r` to Cbor value.
    pub fn decode<R: io::Read>(r: &mut R) -> Result<Cbor> {
        Self::do_decode(r, 1)
    }

    fn do_decode<R: io::Read>(r: &mut R, depth: u32) -> Result<Cbor> {
        if depth > RECURSION_LIMIT {
            return err_at!(FailCbor, msg: "decode recursion limt exceeded");
        }

        let (major, info) = decode_hdr(r)?;

        let val = match (major, info) {
            (0, info) => Cbor::Major0(info, decode_addnl(info, r)?),
            (1, info) => Cbor::Major1(info, decode_addnl(info, r)?),
            (2, Info::Indefinite) => {
                let mut data: Vec<u8> = Vec::default();
                loop {
                    match Self::do_decode(r, depth + 1)? {
                        Cbor::Major2(_, chunk) => data.extend_from_slice(&chunk),
                        Cbor::Major7(_, SimpleValue::Break) => break,
                        _ => err_at!(FailConvert, msg: "expected byte chunk")?,
                    }
                }
                Cbor::Major2(info, data)
            }
            (2, info) => {
                let n: usize = err_at!(FailConvert, decode_addnl(info, r)?.try_into())?;
                let mut data = vec![0; n];
                err_at!(IOError, r.read(&mut data))?;
                Cbor::Major2(info, data)
            }
            (3, Info::Indefinite) => {
                let mut text: Vec<u8> = Vec::default();
                loop {
                    match Self::do_decode(r, depth + 1)? {
                        Cbor::Major3(_, chunk) => text.extend_from_slice(&chunk),
                        Cbor::Major7(_, SimpleValue::Break) => break,
                        _ => err_at!(FailConvert, msg: "expected byte chunk")?,
                    }
                }
                Cbor::Major3(info, text)
            }
            (3, info) => {
                let n: usize = err_at!(FailConvert, decode_addnl(info, r)?.try_into())?;
                let mut text = vec![0; n];
                err_at!(IOError, r.read(&mut text))?;
                Cbor::Major3(info, text)
            }
            (4, Info::Indefinite) => {
                let mut list: Vec<Cbor> = vec![];
                loop {
                    match Self::do_decode(r, depth + 1)? {
                        Cbor::Major7(_, SimpleValue::Break) => break,
                        item => list.push(item),
                    }
                }
                Cbor::Major4(info, list)
            }
            (4, info) => {
                let mut list: Vec<Cbor> = vec![];
                let n = decode_addnl(info, r)?;
                for _ in 0..n {
                    list.push(Self::do_decode(r, depth + 1)?);
                }
                Cbor::Major4(info, list)
            }
            (5, Info::Indefinite) => {
                let mut map: Vec<(Key, Cbor)> = Vec::default();
                loop {
                    let key = Self::do_decode(r, depth + 1)?.try_into()?;
                    let val = match Self::do_decode(r, depth + 1)? {
                        Cbor::Major7(_, SimpleValue::Break) => break,
                        val => val,
                    };
                    map.push((key, val));
                }
                Cbor::Major5(info, map)
            }
            (5, info) => {
                let mut map: Vec<(Key, Cbor)> = Vec::default();
                let n = decode_addnl(info, r)?;
                for _ in 0..n {
                    let key = Self::do_decode(r, depth + 1)?.try_into()?;
                    let val = Self::do_decode(r, depth + 1)?;
                    map.push((key, val));
                }
                Cbor::Major5(info, map)
            }
            (6, info) => Cbor::Major6(info, Tag::decode(info, r)?),
            (7, info) => Cbor::Major7(info, SimpleValue::decode(info, r)?),
            _ => unreachable!(),
        };
        Ok(val)
    }

    fn to_major_val(&self) -> u8 {
        match self {
            Cbor::Major0(_, _) => 0,
            Cbor::Major1(_, _) => 1,
            Cbor::Major2(_, _) => 2,
            Cbor::Major3(_, _) => 3,
            Cbor::Major4(_, _) => 4,
            Cbor::Major5(_, _) => 5,
            Cbor::Major6(_, _) => 6,
            Cbor::Major7(_, _) => 7,
        }
    }
}

/// 5-bit value for additional info.
#[derive(Copy, Clone)]
pub enum Info {
    /// additional info is part of this info.
    Tiny(u8), // 0..=23
    /// additional info of 8-bit unsigned integer.
    U8,
    /// additional info of 16-bit unsigned integer.
    U16,
    /// additional info of 32-bit unsigned integer.
    U32,
    /// additional info of 64-bit unsigned integer.
    U64,
    /// Reserved.
    Reserved28,
    /// Reserved.
    Reserved29,
    /// Reserved.
    Reserved30,
    /// Indefinite encoding.
    Indefinite,
}

impl TryFrom<u8> for Info {
    type Error = Error;

    fn try_from(b: u8) -> Result<Info> {
        let val = match b {
            0..=23 => Info::Tiny(b),
            24 => Info::U8,
            25 => Info::U16,
            26 => Info::U32,
            27 => Info::U64,
            28 => Info::Reserved28,
            29 => Info::Reserved29,
            30 => Info::Reserved30,
            31 => Info::Indefinite,
            _ => err_at!(Fatal, msg: "unreachable")?,
        };

        Ok(val)
    }
}

impl From<u64> for Info {
    fn from(num: u64) -> Info {
        match num {
            0..=23 => Info::Tiny(num as u8),
            n if n <= (u8::MAX as u64) => Info::U8,
            n if n <= (u16::MAX as u64) => Info::U16,
            n if n <= (u32::MAX as u64) => Info::U32,
            _ => Info::U64,
        }
    }
}

impl TryFrom<usize> for Info {
    type Error = Error;

    fn try_from(num: usize) -> Result<Info> {
        Ok(err_at!(FailConvert, u64::try_from(num))?.into())
    }
}

fn encode_hdr(major: u8, info: Info, buf: &mut Vec<u8>) -> Result<usize> {
    let info = match info {
        Info::Tiny(val) if val <= 23 => val,
        Info::Tiny(val) => err_at!(FailCbor, msg: "{} > 23", val)?,
        Info::U8 => 24,
        Info::U16 => 25,
        Info::U32 => 26,
        Info::U64 => 27,
        Info::Reserved28 => 28,
        Info::Reserved29 => 29,
        Info::Reserved30 => 30,
        Info::Indefinite => 31,
    };
    buf.push((major as u8) << 5 | info);
    Ok(1)
}

fn decode_hdr<R: io::Read>(r: &mut R) -> Result<(u8, Info)> {
    let mut scratch = [0_u8; 8];
    err_at!(IOError, r.read(&mut scratch[..1]))?;

    let b = scratch[0];

    let major = (b & 0xe0) >> 5;
    let info = b & 0x1f;
    Ok((major, info.try_into()?))
}

fn encode_addnl(num: u64, buf: &mut Vec<u8>) -> Result<usize> {
    let mut scratch = [0_u8; 8];
    let n = match num {
        0..=23 => 0,
        n if n <= (u8::MAX as u64) => {
            scratch.copy_from_slice(&(n as u8).to_be_bytes());
            1
        }
        n if n <= (u16::MAX as u64) => {
            scratch.copy_from_slice(&(n as u16).to_be_bytes());
            2
        }
        n if n <= (u32::MAX as u64) => {
            scratch.copy_from_slice(&(n as u32).to_be_bytes());
            4
        }
        n => {
            scratch.copy_from_slice(&n.to_be_bytes());
            8
        }
    };
    buf.copy_from_slice(&scratch[..n]);
    Ok(n)
}

fn decode_addnl<R: io::Read>(info: Info, r: &mut R) -> Result<u64> {
    let mut scratch = [0_u8; 8];
    let num = match info {
        Info::Tiny(num) => num as u64,
        Info::U8 => {
            err_at!(IOError, r.read(&mut scratch[..1]))?;
            u8::from_be_bytes(scratch[..1].try_into().unwrap()) as u64
        }
        Info::U16 => {
            err_at!(IOError, r.read(&mut scratch[..2]))?;
            u16::from_be_bytes(scratch[..2].try_into().unwrap()) as u64
        }
        Info::U32 => {
            err_at!(IOError, r.read(&mut scratch[..4]))?;
            u32::from_be_bytes(scratch[..4].try_into().unwrap()) as u64
        }
        Info::U64 => {
            err_at!(IOError, r.read(&mut scratch[..8]))?;
            u64::from_be_bytes(scratch[..8].try_into().unwrap()) as u64
        }
        Info::Indefinite => 0,
        _ => err_at!(FailCbor, msg: "no additional value")?,
    };
    Ok(num)
}

/// Major type 7, simple-value
#[derive(Copy, Clone)]
pub enum SimpleValue {
    /// 0..=19 and 28..=30 and 32..=255 unassigned
    Unassigned,
    /// Boolean type, value true.
    True, // 20, tiny simple-value
    /// Boolean type, value false.
    False, // 21, tiny simple-value
    /// Null unitary type, can be used in place of optional types.
    Null, // 22, tiny simple-value
    /// Undefined unitary type.
    Undefined, // 23, tiny simple-value
    /// Reserver.
    Reserved24(u8), // 24, one-byte simple-value
    /// 16-bit floating point.
    F16(u16), // 25, not-implemented
    /// 32-bit floating point.
    F32(f32), // 26, single-precision float
    /// 64-bit floating point.
    F64(f64), // 27, single-precision float
    /// Break stop for indefinite encoding.
    Break, // 31
}

impl TryFrom<SimpleValue> for Cbor {
    type Error = Error;

    fn try_from(sval: SimpleValue) -> Result<Cbor> {
        use SimpleValue::*;

        let val = match sval {
            Unassigned => err_at!(FailConvert, msg: "simple-value-unassigned")?,
            True => Cbor::Major7(Info::Tiny(20), sval),
            False => Cbor::Major7(Info::Tiny(21), sval),
            Null => Cbor::Major7(Info::Tiny(22), sval),
            Undefined => err_at!(FailConvert, msg: "simple-value-undefined")?,
            Reserved24(_) => err_at!(FailConvert, msg: "simple-value-unassigned1")?,
            F16(_) => err_at!(FailConvert, msg: "simple-value-f16")?,
            F32(_) => Cbor::Major7(Info::U32, sval),
            F64(_) => Cbor::Major7(Info::U64, sval),
            Break => err_at!(FailConvert, msg: "simple-value-break")?,
        };

        Ok(val)
    }
}

impl SimpleValue {
    fn encode(&self, buf: &mut Vec<u8>) -> Result<usize> {
        use SimpleValue::*;

        let mut scratch = [0_u8; 8];
        let n = match self {
            True | False | Null | Undefined | Break | Unassigned => 0,
            Reserved24(num) => {
                scratch[0] = *num;
                1
            }
            F16(f) => {
                scratch.copy_from_slice(&f.to_be_bytes());
                2
            }
            F32(f) => {
                scratch.copy_from_slice(&f.to_be_bytes());
                4
            }
            F64(f) => {
                scratch.copy_from_slice(&f.to_be_bytes());
                8
            }
        };
        buf.copy_from_slice(&scratch[..n]);
        Ok(n)
    }

    fn decode<R: io::Read>(info: Info, r: &mut R) -> Result<SimpleValue> {
        let mut scratch = [0_u8; 8];
        let val = match info {
            Info::Tiny(20) => SimpleValue::True,
            Info::Tiny(21) => SimpleValue::False,
            Info::Tiny(22) => SimpleValue::Null,
            Info::Tiny(23) => err_at!(FailCbor, msg: "simple-value-undefined")?,
            Info::Tiny(_) => err_at!(FailCbor, msg: "simple-value-unassigned")?,
            Info::U8 => err_at!(FailCbor, msg: "simple-value-unassigned1")?,
            Info::U16 => err_at!(FailCbor, msg: "simple-value-f16")?,
            Info::U32 => {
                err_at!(IOError, r.read(&mut scratch[..4]))?;
                let val = f32::from_be_bytes(scratch[..4].try_into().unwrap());
                SimpleValue::F32(val)
            }
            Info::U64 => {
                err_at!(IOError, r.read(&mut scratch[..8]))?;
                let val = f64::from_be_bytes(scratch[..8].try_into().unwrap());
                SimpleValue::F64(val)
            }
            Info::Reserved28 => err_at!(FailCbor, msg: "simple-value-reserved")?,
            Info::Reserved29 => err_at!(FailCbor, msg: "simple-value-reserved")?,
            Info::Reserved30 => err_at!(FailCbor, msg: "simple-value-reserved")?,
            Info::Indefinite => err_at!(FailCbor, msg: "simple-value-break")?,
        };
        Ok(val)
    }
}

/// Major type 6, Tag values.
#[derive(Clone)]
pub enum Tag {
    /// Don't worry about the type wrapped by the tag-value, just encode
    /// the tag and leave the subsequent encoding at caller's discretion.
    Value(u64),
}

impl From<Tag> for u64 {
    fn from(tag: Tag) -> u64 {
        match tag {
            Tag::Value(val) => val,
        }
    }
}

impl From<u64> for Tag {
    fn from(tag: u64) -> Tag {
        Tag::Value(tag)
    }
}

impl Tag {
    fn encode(&self, buf: &mut Vec<u8>) -> Result<usize> {
        match self {
            Tag::Value(val) => encode_addnl(*val, buf),
        }
    }

    fn decode<R: io::Read>(info: Info, r: &mut R) -> Result<Tag> {
        let tag = Tag::Value(decode_addnl(info, r)?);
        Ok(tag)
    }
}

/// Possible types that can be used as key in cbor-map.
#[derive(Clone)]
pub enum Key {
    U64(u64),
    N64(i64),
    Bytes(Vec<u8>),
    Text(String),
    Bool(bool),
    F32(f32),
    F64(f64),
}

impl TryFrom<Key> for Cbor {
    type Error = Error;

    fn try_from(key: Key) -> Result<Cbor> {
        let val = match key {
            Key::U64(key) => Cbor::Major0(key.into(), key),
            Key::N64(key) if key >= 0 => {
                let val = err_at!(FailConvert, u64::try_from(key))?;
                Cbor::Major0(val.into(), val)
            }
            Key::N64(key) => {
                let val = err_at!(FailConvert, u64::try_from(key.abs() - 1))?;
                Cbor::Major1(val.into(), val)
            }
            Key::Bytes(key) => Cbor::Major2(err_at!(FailConvert, key.len().try_into())?, key),
            Key::Text(key) => Cbor::Major3(err_at!(FailConvert, key.len().try_into())?, key.into()),
            Key::Bool(true) => SimpleValue::True.try_into()?,
            Key::Bool(false) => SimpleValue::False.try_into()?,
            Key::F32(key) => SimpleValue::F32(key).try_into()?,
            Key::F64(key) => SimpleValue::F64(key).try_into()?,
        };

        Ok(val)
    }
}

impl TryFrom<Cbor> for Key {
    type Error = Error;

    fn try_from(val: Cbor) -> Result<Key> {
        use std::str::from_utf8;

        let key = match val {
            Cbor::Major0(_, key) => Key::U64(key),
            Cbor::Major1(_, key) => Key::N64(-err_at!(FailConvert, i64::try_from(key + 1))?),
            Cbor::Major2(_, key) => Key::Bytes(key),
            Cbor::Major3(_, key) => Key::Text(err_at!(FailConvert, from_utf8(&key))?.to_string()),
            Cbor::Major7(_, SimpleValue::True) => Key::Bool(true),
            Cbor::Major7(_, SimpleValue::False) => Key::Bool(false),
            Cbor::Major7(_, SimpleValue::F32(key)) => Key::F32(key),
            Cbor::Major7(_, SimpleValue::F64(key)) => Key::F64(key),
            _ => err_at!(FailKey, msg: "cbor not a valid key")?,
        };

        Ok(key)
    }
}
