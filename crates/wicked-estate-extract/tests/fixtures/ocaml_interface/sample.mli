module Geometry : sig

  type shape =
    | Circle of float
    | Rectangle of float * float
    | Triangle of float * float * float

  val area : shape -> float

  val perimeter : shape -> float

  val describe : shape -> string

  val largest : shape list -> shape option

end
