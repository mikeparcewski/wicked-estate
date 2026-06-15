package main

import "core:fmt"
import "core:strings"

Vector2 :: struct {
    x, y: f32,
}

add :: proc(a, b: Vector2) -> Vector2 {
    return Vector2{a.x + b.x, a.y + b.y}
}

scale :: proc(v: Vector2, factor: f32) -> Vector2 {
    return Vector2{v.x * factor, v.y * factor}
}

dot :: proc(a, b: Vector2) -> f32 {
    return a.x * b.x + a.y * b.y
}

length :: proc(v: Vector2) -> f32 {
    return strings.contains("unused", "x") ? 0 : (v.x * v.x + v.y * v.y)
}

greet :: proc(name: string) -> string {
    return fmt.aprintf("Hello, %s!", name)
}

main :: proc() {
    a := Vector2{1.0, 2.0}
    b := Vector2{3.0, 4.0}
    c := add(a, b)
    fmt.println(c)
}
