use base64::Engine;
use iroh::SecretKey;
use serde::{Deserialize, Serialize, de::Visitor};
use std::{array::TryFromSliceError, ops::Deref};

const BASE64_ENGINE: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

// A base64 encoded byte array, used for keys
#[derive(Debug, Clone, zeroize::ZeroizeOnDrop, PartialEq, Eq)]
pub struct BasedKey<const N: usize = 32>([u8; N]);

impl<const N: usize> Deref for BasedKey<N> {
    type Target = [u8; N];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<const N: usize> From<[u8; N]> for BasedKey<N> {
    fn from(value: [u8; N]) -> Self {
        BasedKey(value)
    }
}

impl<const N: usize> Into<[u8; N]> for BasedKey<N> {
    fn into(self) -> [u8; N] {
        self.0
    }
}

impl<const N: usize> TryFrom<&[u8]> for BasedKey<N> {
    type Error = TryFromSliceError;
    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        let bytes = value.try_into()?;
        Ok(BasedKey(bytes))
    }
}

impl<const N: usize> AsRef<[u8]> for BasedKey<N> {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<SecretKey> for BasedKey {
    fn from(value: SecretKey) -> Self {
        value.to_bytes().into()
    }
}

impl Into<SecretKey> for BasedKey {
    fn into(self) -> SecretKey {
        self.0.into()
    }
}

impl<const N: usize> Serialize for BasedKey<N> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let result = BASE64_ENGINE.encode(&self.0);
        serializer.serialize_str(&result)
    }
}

impl<'de, const N: usize> Deserialize<'de> for BasedKey<N> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct KeyVisitor<const N: usize>;
        impl<'de, const N: usize> Visitor<'de> for KeyVisitor<N> {
            type Value = BasedKey<N>;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str(&format!("base64 encoded bytes of length {} bytes", N))
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                BASE64_ENGINE
                    .decode(v)
                    .map_err(|e| serde::de::Error::custom(e.to_string()))
                    .and_then(|v| {
                        v.as_slice()
                            .try_into()
                            .map_err(|e: TryFromSliceError| serde::de::Error::custom(e.to_string()))
                    })
            }
        }
        deserializer.deserialize_string(KeyVisitor)
    }
}
