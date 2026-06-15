module Geometry

export Shape, Circle, Rectangle, Triangle, area, perimeter, describe, largest

abstract type Shape end

struct Circle <: Shape
    radius::Float64
end

struct Rectangle <: Shape
    width::Float64
    height::Float64
end

struct Triangle <: Shape
    a::Float64
    b::Float64
    c::Float64
end

"""
    area(s::Shape) -> Float64

Compute the area of a shape via multiple dispatch.
"""
function area(s::Circle)
    return π * s.radius^2
end

function area(s::Rectangle)
    return s.width * s.height
end

function area(s::Triangle)
    p = (s.a + s.b + s.c) / 2.0
    return sqrt(p * (p - s.a) * (p - s.b) * (p - s.c))
end

"""
    perimeter(s::Shape) -> Float64

Compute the perimeter of a shape via multiple dispatch.
"""
function perimeter(s::Circle)
    return 2π * s.radius
end

function perimeter(s::Rectangle)
    return 2.0 * (s.width + s.height)
end

function perimeter(s::Triangle)
    return s.a + s.b + s.c
end

"""
    describe(s::Shape) -> String

Return a human-readable description of s.
"""
function describe(s::Circle)
    return "Circle(r=$(s.radius))"
end

function describe(s::Rectangle)
    return "Rectangle($(s.width)×$(s.height))"
end

function describe(s::Triangle)
    return "Triangle($(s.a), $(s.b), $(s.c))"
end

"""
    largest(shapes) -> Shape

Return the shape with the maximum area.
"""
function largest(shapes)
    return reduce((a, b) -> area(a) >= area(b) ? a : b, shapes)
end

end # module Geometry
