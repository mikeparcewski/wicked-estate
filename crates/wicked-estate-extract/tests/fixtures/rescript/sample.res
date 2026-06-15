module Geometry = {
  type point = {x: float, y: float}

  let makePoint = (x, y) => {x, y}

  let distance = (a, b) => {
    let dx = a.x -. b.x
    let dy = a.y -. b.y
    Js.Math.sqrt(dx *. dx +. dy *. dy)
  }

  let translate = (p, dx, dy) => {
    {x: p.x +. dx, y: p.y +. dy}
  }

  let describe = point =>
    switch point {
    | {x: 0.0, y: 0.0} => "origin"
    | {x, y} => `(${Float.toString(x)}, ${Float.toString(y)})`
    }
}

let origin = Geometry.makePoint(0.0, 0.0)
let p = Geometry.makePoint(3.0, 4.0)
Js.log(Geometry.distance(origin, p))
