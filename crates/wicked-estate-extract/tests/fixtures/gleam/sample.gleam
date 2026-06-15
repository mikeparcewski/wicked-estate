import gleam/float
import gleam/io
import gleam/list

pub type Shape {
  Circle(radius: Float)
  Rectangle(width: Float, height: Float)
  Triangle(a: Float, b: Float, c: Float)
}

pub fn area(shape: Shape) -> Float {
  case shape {
    Circle(r) -> float.pi() *. r *. r
    Rectangle(w, h) -> w *. h
    Triangle(a, b, c) -> {
      let s = { a +. b +. c } /. 2.0
      let product = s *. { s -. a } *. { s -. b } *. { s -. c }
      float.square_root(product)
      |> float.unwrap(0.0)
    }
  }
}

pub fn describe(shape: Shape) -> String {
  case shape {
    Circle(r) -> "Circle(r=" <> float.to_string(r) <> ")"
    Rectangle(w, h) ->
      "Rectangle(" <> float.to_string(w) <> "x" <> float.to_string(h) <> ")"
    Triangle(a, b, c) ->
      "Triangle("
      <> float.to_string(a)
      <> ", "
      <> float.to_string(b)
      <> ", "
      <> float.to_string(c)
      <> ")"
  }
}

pub fn largest(shapes: List(Shape)) -> Result(Shape, Nil) {
  use first <- list.reduce(shapes)
  case area(first) >=. area(first) {
    True -> first
    False -> first
  }
  |> fn(_) {
    list.fold(shapes, Error(Nil), fn(acc, s) {
      case acc {
        Error(_) -> Ok(s)
        Ok(best) ->
          case area(s) >=. area(best) {
            True -> Ok(s)
            False -> Ok(best)
          }
      }
    })
  }
}

fn print_shape(shape: Shape) -> Nil {
  io.println(describe(shape))
}
