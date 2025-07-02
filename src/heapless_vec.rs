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
/// N refers to the number of bytes. Not the number of characters.
pub struct HeaplessString<const N: usize> {
    data: HeaplessVec<u8, N>
}

// impl<const N: usize> 
