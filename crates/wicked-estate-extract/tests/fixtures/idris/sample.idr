module Geometry

import Data.List

%default total

-- Data type
data Shape
  = Circle Double
  | Rectangle Double Double
  | Triangle Double Double Double

-- Type class for describable things
interface Describable a where
  describe : a -> String

-- Instance for Shape
Describable Shape where
  describe (Circle r)       = "Circle(r=" ++ show r ++ ")"
  describe (Rectangle w h)  = "Rectangle(" ++ show w ++ "x" ++ show h ++ ")"
  describe (Triangle a b c) = "Triangle(" ++ show a ++ ", " ++ show b ++ ", " ++ show c ++ ")"

-- Area computation
area : Shape -> Double
area (Circle r)       = pi * r * r
area (Rectangle w h)  = w * h
area (Triangle a b c) =
  let s = (a + b + c) / 2.0
  in sqrt (s * (s - a) * (s - b) * (s - c))

-- Perimeter computation
perimeter : Shape -> Double
perimeter (Circle r)       = 2.0 * pi * r
perimeter (Rectangle w h)  = 2.0 * (w + h)
perimeter (Triangle a b c) = a + b + c

-- Return the shape with the largest area
largest : (shapes : List Shape) -> (NonEmpty shapes) => Shape
largest [s]      = s
largest (s :: rest@(_ :: _)) =
  let best = largest rest
  in if area s >= area best then s else best
