module Sample where

import Data.List (sort, nub)
import Data.Maybe (fromMaybe, mapMaybe)

-- Type alias
type Name = String
type Score = Double

-- Data type
data Shape
  = Circle Double
  | Rectangle Double Double
  | Triangle Double Double Double
  deriving (Show, Eq)

-- Type class instance
class Describable a where
  describe :: a -> String

instance Describable Shape where
  describe (Circle r)        = "Circle with radius " ++ show r
  describe (Rectangle w h)   = "Rectangle " ++ show w ++ "x" ++ show h
  describe (Triangle a b c)  = "Triangle with sides " ++ show a ++ ", " ++ show b ++ ", " ++ show c

-- Compute the area of a shape
area :: Shape -> Double
area (Circle r)       = pi * r * r
area (Rectangle w h)  = w * h
area (Triangle a b c) =
  let s = (a + b + c) / 2
  in sqrt (s * (s - a) * (s - b) * (s - c))

-- Rank shapes by area descending
rankByArea :: [Shape] -> [Shape]
rankByArea shapes =
  let pairs = map (\s -> (area s, s)) shapes
      sorted = reverse $ sort $ map fst pairs
  in mapMaybe (\score -> lookup score pairs) sorted

-- Score a named entity
scoreEntity :: Name -> Shape -> (Name, Score)
scoreEntity name shape = (name, area shape)
