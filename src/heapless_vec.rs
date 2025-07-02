use std::{fmt::Debug, mem, ops::Index};

use crate::error::{err, Error};

#[repr(C)]
union MaybeUninit<T: Copy> {
    uninit: (),
    pub value: T,
}

impl<T: Copy> Default for MaybeUninit<T> {
    fn default() -> Self {
        Self { uninit: () }
    }
}

#[repr(C)]
#[derive(Clone)]
/// Real-time safe, fixed-size, FFI friendly vector.
pub struct HeaplessVec<T: Copy, const N: usize> {
    count: usize,
    data: [MaybeUninit<T>; N],
}

impl<T: Copy> Clone for MaybeUninit<T> {
    fn clone(&self) -> Self {
        unsafe { MaybeUninit { value: self.value } }
    }
}

impl<T: Copy + Debug, const N: usize> Debug for HeaplessVec<T, N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = String::new();
        for i in 0..self.count {
            if i > 0 {
                s.push_str(", ");
            }
            unsafe {
                s.push_str(&format!("{:?}", self.data[i].value));
            }
        }
        write!(f, "[{}]", s)
    }
}

impl<T: Copy, const N: usize> Default for HeaplessVec<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy, const N: usize> HeaplessVec<T, N> {
    pub fn new() -> Self {
        Self {
            count: 0,
            data: unsafe { mem::zeroed() },
        }
    }

    pub fn push(&mut self, value: T) -> Result<(), Error> {
        if self.count < N {
            self.data[self.count].value = value;
            self.count += 1;
            Ok(())
        } else {
            err("HeaplessVec: vector is full")
        }
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.count > 0 {
            self.count -= 1;
            Some(unsafe { mem::take(&mut self.data[self.count]).value })
        } else {
            None
        }
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn iter(&self) -> HeaplessVecIter<T, N> {
        HeaplessVecIter {
            vec: self,
            index: 0,
        }
    }

    pub fn last_mut(&mut self) -> Option<&mut T> {
        if self.count == 0 {
            return None;
        }

        Some(unsafe { &mut self.data[self.count - 1].value })
    }

    pub fn clear(&mut self) {
        while !self.is_empty() {
            self.pop();
        }
    }

    pub fn as_slice(&self) -> &[T] {
        // Faily sure this won't break...
        unsafe {
            std::slice::from_raw_parts(
                self.data.as_ptr() as *const _,
                self.len(),
            )
        }
    }
}

impl<T: Copy + PartialEq, const N: usize> HeaplessVec<T, N> {
    pub fn contains(&self, item: T) -> bool {
        !self.iter().all(|i| *i != item)
    }
}

pub struct HeaplessVecIter<'a, T: Copy, const N: usize> {
    vec: &'a HeaplessVec<T, N>,
    index: usize,
}

impl<'a, T: Copy, const N: usize> Iterator for HeaplessVecIter<'a, T, N> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.vec.count {
            let item = unsafe { &self.vec.data[self.index].value };
            self.index += 1;
            Some(item)
        } else {
            None
        }
    }
}

impl<T: Copy, const N: usize> Index<usize> for HeaplessVec<T, N> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        if index < self.count {
            unsafe { &self.data[index].value }
        } else {
            panic!("Index out of bounds");
        }
    }
}

#[repr(C)]
#[derive(Clone)]
/// Real-time safe, fixed-size, FFI friendly String.
/// Call `to_string` or `as_str` to get a normal string type.
/// N refers to the number of bytes, of characters.
/// Stored as UTF-8.
pub struct HeaplessString<const N: usize> {
    data: HeaplessVec<u8, N>,
}

impl<const N: usize> HeaplessString<N> {
    pub fn new() -> Self {
        Self {
            data: HeaplessVec::new(),
        }
    }

    pub fn from_str(s: &str) -> Result<Self, Error> {
        let mut string = Self::new();
        string.push_str(s)?;
        Ok(string)
    }

    pub fn push_str(&mut self, s: &str) -> Result<(), Error> {
        for byte in s.bytes() {
            self.data.push(byte)?;
        }
        Ok(())
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(self.data.as_slice()).expect("Invalid UTF-8")
    }
}

impl<const N: usize> Default for HeaplessString<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Debug for HeaplessString<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "\"{}\"", self.as_str())
    }
}
 
impl<const N: usize> ToString for HeaplessString<N> {
    fn to_string(&self) -> String {
        self.as_str().to_string()
    }
}

#[cfg(feature = "serde")]
impl<const N: usize> serde::Serialize for HeaplessString<N> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

#[cfg(feature = "serde")]
impl<'a, const N: usize> serde::Deserialize<'a> for HeaplessString<N> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'a>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

#[no_mangle]
pub unsafe extern "C" fn push_c_str_to_heapless_string(
    heapless_string: *mut HeaplessString<256>,
    c_str: *const std::ffi::c_char,
) -> bool {
    if c_str.is_null() {
        eprintln!("Error: Null pointer passed to push_c_str_to_heapless_string");
        return false;
    }

    let mut len = 0;
    while unsafe { *c_str.add(len) } != 0 && len < 255 {
        len += 1;
    }

    let slice = unsafe { std::slice::from_raw_parts(c_str as *const u8, len) };
    let Ok(s) = std::str::from_utf8(slice) else {
        eprintln!("Error: Invalid UTF-8 string passed to push_c_str_to_heapless_string");
        return false;
    };
    
    if (*heapless_string).push_str(s).is_err() {
        eprintln!("Error: HeaplessString was full.");
        return false;
    }

    true
}
