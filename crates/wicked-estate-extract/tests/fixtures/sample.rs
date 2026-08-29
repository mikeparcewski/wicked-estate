use std::fmt;
use crate::utils::helper;

pub const MAX_DISTANCE: f64 = 1000.0;
pub static ORIGIN: Point = Point { x: 0.0, y: 0.0 };

pub type Distance = f64;

pub struct Point {
    pub x: f64,
    pub y: f64,
}

pub enum Direction {
    North,
    South,
    East,
    West,
}

pub trait Drawable {
    fn draw(&self);
}

impl Point {
    pub fn translate(&mut self, dx: f64, dy: f64) {
        self.x += dx;
        self.y += dy;
    }
}

pub struct Rect {
    pub w: f64,
    pub h: f64,
}

// Second impl with a same-named method: scheme 3 nests it under Rect#, distinct
// from Point#translate(). (scm-anchors D3).
impl Rect {
    pub fn translate(&mut self, dx: f64, dy: f64) {
        self.w += dx;
        self.h += dy;
    }
}

pub struct Holder<T> {
    pub v: T,
}

// Anchor branch: generic_type — `impl Holder<T>` anchors under Holder.
impl<T> Holder<T> {
    pub fn get(&self) -> &T {
        &self.v
    }
}

// Anchor branch: trait impl with plain type_identifier.
impl Drawable for Rect {
    fn draw(&self) {}
}

// Anchor branch: scoped_type_identifier — anchors under Widget.
impl Drawable for crate::ext::Widget {
    fn draw(&self) {}
}

// Anchor branch: path-qualified generic (generic_type over scoped_type_identifier)
// — anchors under Wrap.
impl<T> Drawable for crate::ext::Wrap<T> {
    fn draw(&self) {}
}

pub fn distance(a: &Point, b: &Point) -> Distance {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    helper(dx * dx + dy * dy)
}

pub fn main_entry() {
    let p = Point { x: 1.0, y: 2.0 };
    let q = Point { x: 4.0, y: 6.0 };
    let d = distance(&p, &q);
    println!("{}", d);
}
