module Geometry = struct

  type shape =
    | Circle of float
    | Rectangle of float * float
    | Triangle of float * float * float

  let area = function
    | Circle r -> Float.pi *. r *. r
    | Rectangle (w, h) -> w *. h
    | Triangle (a, b, c) ->
        let s = (a +. b +. c) /. 2.0 in
        sqrt (s *. (s -. a) *. (s -. b) *. (s -. c))

  let perimeter = function
    | Circle r -> 2.0 *. Float.pi *. r
    | Rectangle (w, h) -> 2.0 *. (w +. h)
    | Triangle (a, b, c) -> a +. b +. c

  let describe shape =
    match shape with
    | Circle r -> Printf.sprintf "Circle(r=%.2f)" r
    | Rectangle (w, h) -> Printf.sprintf "Rectangle(%.2f x %.2f)" w h
    | Triangle (a, b, c) -> Printf.sprintf "Triangle(%.2f, %.2f, %.2f)" a b c

  let largest shapes =
    match shapes with
    | [] -> None
    | _ ->
        let by_area s = area s in
        Some (List.fold_left
          (fun acc s -> if by_area s > by_area acc then s else acc)
          (List.hd shapes)
          (List.tl shapes))

end
