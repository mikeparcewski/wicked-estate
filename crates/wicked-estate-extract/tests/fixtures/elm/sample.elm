module Geometry exposing (Shape, area, perimeter, describe)

import List exposing (foldl)


type alias Radius =
    Float


type Shape
    = Circle Radius
    | Rectangle Float Float
    | Triangle Float Float Float


area : Shape -> Float
area shape =
    case shape of
        Circle r ->
            pi * r * r

        Rectangle w h ->
            w * h

        Triangle a b c ->
            let
                s =
                    (a + b + c) / 2.0
            in
            sqrt (s * (s - a) * (s - b) * (s - c))


perimeter : Shape -> Float
perimeter shape =
    case shape of
        Circle r ->
            2.0 * pi * r

        Rectangle w h ->
            2.0 * (w + h)

        Triangle a b c ->
            a + b + c


describe : Shape -> String
describe shape =
    case shape of
        Circle r ->
            "Circle with radius " ++ String.fromFloat r

        Rectangle w h ->
            "Rectangle " ++ String.fromFloat w ++ "x" ++ String.fromFloat h

        Triangle a b c ->
            "Triangle("
                ++ String.fromFloat a
                ++ ", "
                ++ String.fromFloat b
                ++ ", "
                ++ String.fromFloat c
                ++ ")"
