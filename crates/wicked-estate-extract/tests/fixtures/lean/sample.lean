namespace Geometry

inductive Shape where
  | circle    : Float → Shape
  | rectangle : Float → Float → Shape
  | triangle  : Float → Float → Float → Shape
  deriving Repr

def area : Shape → Float
  | .circle r       => Float.pi * r * r
  | .rectangle w h  => w * h
  | .triangle a b c =>
    let s := (a + b + c) / 2
    Float.sqrt (s * (s - a) * (s - b) * (s - c))

def perimeter : Shape → Float
  | .circle r       => 2 * Float.pi * r
  | .rectangle w h  => 2 * (w + h)
  | .triangle a b c => a + b + c

theorem circle_area_pos (r : Float) (hr : 0 < r) : 0 < area (.circle r) := by
  simp [area]
  positivity

instance : ToString Shape where
  toString s := match s with
    | .circle r      => s!"Circle(r={r})"
    | .rectangle w h => s!"Rect({w}x{h})"
    | .triangle _ _ _ => "Triangle"

end Geometry
