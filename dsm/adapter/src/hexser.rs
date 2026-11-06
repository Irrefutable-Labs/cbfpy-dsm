//! Hashes and signatures as hexadecimal in stored and transmitted JSON.

use serde::{Deserialize, Deserializer, Serializer};

fn encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn decode(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return Err("not hexadecimal".to_string());
    }
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

pub fn to_hex(bytes: &[u8]) -> String {
    encode(bytes)
}

pub mod array {
    use super::*;

    pub fn serialize<S: Serializer>(bytes: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let text = String::deserialize(d)?;
        decode(&text)
            .map_err(serde::de::Error::custom)?
            .try_into()
            .map_err(|_| serde::de::Error::custom("not 32 bytes"))
    }
}

pub mod option_array {
    use super::*;

    pub fn serialize<S: Serializer>(bytes: &Option<[u8; 32]>, s: S) -> Result<S::Ok, S::Error> {
        match bytes {
            Some(b) => s.serialize_some(&encode(b)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<[u8; 32]>, D::Error> {
        match Option::<String>::deserialize(d)? {
            None => Ok(None),
            Some(text) => decode(&text)
                .map_err(serde::de::Error::custom)?
                .try_into()
                .map(Some)
                .map_err(|_| serde::de::Error::custom("not 32 bytes")),
        }
    }
}

pub mod bytes {
    use super::*;

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        decode(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
