type
  Point = object
    x, y: float

proc newPoint(x, y: float): Point =
  Point(x: x, y: y)

proc distance(a, b: Point): float =
  let dx = a.x - b.x
  let dy = a.y - b.y
  sqrt(dx * dx + dy * dy)

proc translate[T](p: Point, dx: T, dy: T): Point =
  Point(x: p.x + float(dx), y: p.y + float(dy))

let origin = newPoint(0.0, 0.0)
let p = newPoint(3.0, 4.0)
echo distance(origin, p)
echo translate(p, 1, 2)
