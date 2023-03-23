use compact_str::CompactString;
use enum_as_inner::EnumAsInner;
#[cfg(feature = "serde")]
use serde::{ser::SerializeMap, Deserialize, Serialize};

pub type NbtByte = i8;
pub type NbtShort = i16;
pub type NbtInt = i32;
pub type NbtLong = i64;
pub type NbtFloat = f32;
pub type NbtDouble = f64;
pub type NbtByteArray = Vec<u8>;
pub type NbtString = CompactString;
pub type NbtIntArray = Vec<i32>;
pub type NbtLongArray = Vec<i64>;

pub const END_ID: u8 = 0;
pub const BYTE_ID: u8 = 1;
pub const SHORT_ID: u8 = 2;
pub const INT_ID: u8 = 3;
pub const LONG_ID: u8 = 4;
pub const FLOAT_ID: u8 = 5;
pub const DOUBLE_ID: u8 = 6;
pub const BYTE_ARRAY_ID: u8 = 7;
pub const STRING_ID: u8 = 8;
pub const LIST_ID: u8 = 9;
pub const COMPOUND_ID: u8 = 10;
pub const INT_ARRAY_ID: u8 = 11;
pub const LONG_ARRAY_ID: u8 = 12;

/// An NBT value.
#[derive(Clone, Debug, PartialEq, Default, EnumAsInner)]
#[repr(u8)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize), serde(untagged))]
pub enum Tag {
    #[default]
    End = END_ID,
    Byte(NbtByte) = BYTE_ID,
    Short(NbtShort) = SHORT_ID,
    Int(NbtInt) = INT_ID,
    Long(NbtLong) = LONG_ID,
    Float(NbtFloat) = FLOAT_ID,
    Double(NbtDouble) = DOUBLE_ID,
    ByteArray(NbtByteArray) = BYTE_ARRAY_ID,
    String(NbtString) = STRING_ID,
    List(NbtList) = LIST_ID,
    Compound(NbtCompound) = COMPOUND_ID,
    IntArray(NbtIntArray) = INT_ARRAY_ID,
    LongArray(NbtLongArray) = LONG_ARRAY_ID,
}

/// An NBT value.
#[derive(Clone, Debug, PartialEq)]
#[repr(u8)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize), serde(untagged))]
pub enum NbtList {
    Empty = END_ID,
    Byte(Vec<NbtByte>) = BYTE_ID,
    Short(Vec<NbtShort>) = SHORT_ID,
    Int(Vec<NbtInt>) = INT_ID,
    Long(Vec<NbtLong>) = LONG_ID,
    Float(Vec<NbtFloat>) = FLOAT_ID,
    Double(Vec<NbtDouble>) = DOUBLE_ID,
    ByteArray(Vec<NbtByteArray>) = BYTE_ARRAY_ID,
    String(Vec<NbtString>) = STRING_ID,
    List(Vec<NbtList>) = LIST_ID,
    Compound(Vec<NbtCompound>) = COMPOUND_ID,
    IntArray(Vec<NbtIntArray>) = INT_ARRAY_ID,
    LongArray(Vec<NbtLongArray>) = LONG_ARRAY_ID,
}

impl Tag {
    /// Get the numerical ID of the tag type.
    #[inline]
    pub fn id(&self) -> u8 {
        // SAFETY: Because `Self` is marked `repr(u8)`, its layout is a `repr(C)`
        // `union` between `repr(C)` structs, each of which has the `u8`
        // discriminant as its first field, so we can read the discriminant
        // without offsetting the pointer.
        unsafe { *<*const _>::from(self).cast::<u8>() }
    }
}
impl NbtList {
    /// Get the numerical ID of the tag type.
    #[inline]
    pub fn id(&self) -> u8 {
        // SAFETY: Because `Self` is marked `repr(u8)`, its layout is a `repr(C)`
        // `union` between `repr(C)` structs, each of which has the `u8`
        // discriminant as its first field, so we can read the discriminant
        // without offsetting the pointer.
        unsafe { *<*const _>::from(self).cast::<u8>() }
    }
}

// thanks to Moulberry/Graphite for the idea to use a vec and binary search
#[derive(Debug, Clone)]
pub struct NbtCompound {
    sorted: bool,
    inner: Vec<(NbtString, Tag)>,
}
impl NbtCompound {
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            sorted: false,
            inner: Vec::with_capacity(capacity),
        }
    }

    #[inline]
    fn binary_search(&self, key: &NbtString) -> Result<usize, usize> {
        self.inner.binary_search_by(|(k, _)| k.cmp(key))
    }

    /// Get a reference to the value corresponding to the key in this compound.
    ///
    /// If you previously used [`Self::insert_unsorted`] without [`Self::sort`],
    /// this function may return incorrect results.
    #[inline]
    pub fn get(&mut self, key: &NbtString) -> Option<&Tag> {
        if !self.sorted {
            self.sort()
        }
        self.binary_search(key).ok().map(|i| &self.inner[i].1)
    }

    /// Insert an item into the compound, returning the previous value if it
    /// existed.
    ///
    /// If you're adding many items at once, it's more efficient to use
    /// [`Self::insert_unsorted`] and then [`Self::sort`] after everything is
    /// inserted.
    #[inline]
    pub fn insert(&mut self, key: NbtString, value: Tag) {
        self.inner.push((key, value));
    }

    #[inline]
    pub fn sort(&mut self) {
        self.sorted = true;
        self.inner.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, (CompactString, Tag)> {
        self.inner.iter()
    }
}
#[cfg(feature = "serde")]
impl Serialize for NbtCompound {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.inner.len()))?;
        for (key, value) in &self.inner {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}
#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for NbtCompound {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use std::collections::BTreeMap;
        let map = <BTreeMap<NbtString, Tag> as Deserialize>::deserialize(deserializer)?;
        Ok(Self {
            inner: map.into_iter().collect(),
            sorted: false,
        })
    }
}

impl FromIterator<(NbtString, Tag)> for NbtCompound {
    fn from_iter<T: IntoIterator<Item = (NbtString, Tag)>>(iter: T) -> Self {
        let inner = iter.into_iter().collect::<Vec<_>>();
        Self {
            inner,
            sorted: false,
        }
    }
}

impl PartialEq for NbtCompound {
    /// Compare two NBT compounds for equality, ignoring the order of the keys.
    /// Note that this will execute fastest if the keys are already sorted with
    /// [`Self::sort`].
    fn eq(&self, other: &Self) -> bool {
        if self.inner.len() != other.inner.len() {
            return false;
        }
        if self.inner == other.inner {
            return true;
        }
        if !self.sorted && !other.sorted {
            // neither are sorted, so sort both
            let mut a = self.clone();
            let mut b = other.clone();
            a.sort();
            b.sort();
            a == b
        } else if !self.sorted {
            // only self is sorted, so sort self
            let mut a = self.clone();
            a.sort();
            a == *other
        } else if !other.sorted {
            // only other is sorted, so sort other
            let mut b = other.clone();
            b.sort();
            *self == b
        } else {
            self.inner == other.inner
        }
    }
}
