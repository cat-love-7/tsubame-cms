use std::{
    fmt::{Debug, Display},
    hash::Hash,
    ops::Deref,
};

use serde::{Deserialize, Serialize};

/// A numeric id.
///
/// `Copy` is implemented by hand instead of derived: `#[derive(Copy)]` would add a
/// `T: Copy` bound, but the only thing held about `T` is `PhantomData<fn() -> T>`, which is
/// `Copy` for every `T`. Deriving silently makes `CollectionItemId` (whose `T` is a `Vec`)
/// non-`Copy`, which is not intended.
#[derive(Default)]
pub struct UintId<T>(u64, std::marker::PhantomData<fn() -> T>);

impl<T> Copy for UintId<T> {}

impl<T> PartialEq for UintId<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T> Eq for UintId<T> {}

impl<T> Hash for UintId<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<T> PartialOrd for UintId<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

impl<T> Ord for UintId<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl<T> Deref for UintId<T> {
    type Target = u64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<T> UintId<T> {
    pub fn from_u64(id: u64) -> Self {
        UintId(id, std::marker::PhantomData)
    }
    pub fn from_le_bytes(bytes: [u8; 8]) -> Self {
        UintId(u64::from_le_bytes(bytes), std::marker::PhantomData)
    }
}
impl<T> Debug for UintId<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl<T> Clone for UintId<T> {
    fn clone(&self) -> Self {
        UintId(self.0, std::marker::PhantomData)
    }
}
impl<T> Serialize for UintId<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u64(self.0)
    }
}
impl<'de, T> Deserialize<'de> for UintId<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let id = u64::deserialize(deserializer)?;
        Ok(UintId(id, std::marker::PhantomData))
    }
}
impl<T> Display for UintId<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug, Default)]
pub struct StringId<T>(pub String, pub std::marker::PhantomData<fn() -> T>);
impl<T> PartialEq for StringId<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl<T> Eq for StringId<T> {}
impl<T> Hash for StringId<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}
impl<T> PartialOrd for StringId<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(&other.0)
    }
}
impl<T> Ord for StringId<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}
impl<T> Display for StringId<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl<T> Deref for StringId<T> {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<T> Serialize for StringId<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}
impl<'de, T> Deserialize<'de> for StringId<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(StringId(s, std::marker::PhantomData))
    }
}

impl<T> From<&str> for StringId<T> {
    fn from(value: &str) -> Self {
        StringId(value.to_string(), std::marker::PhantomData)
    }
}
