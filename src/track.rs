use crate::heapless_vec::HeaplessString;

#[repr(C)]
#[derive(Clone)]
pub struct Track {
    pub name: HeaplessString<64>,
    pub col: Colour,
}

pub type Color = Colour;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Colour {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}
