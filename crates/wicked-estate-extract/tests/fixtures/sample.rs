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
