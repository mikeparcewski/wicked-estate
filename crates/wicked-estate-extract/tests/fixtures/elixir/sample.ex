defmodule Geometry do
  @moduledoc """
  Basic geometric shape calculations.
  """

  import Enum, only: [max_by: 2, map: 2]
  alias Geometry.Shape, as: Shape
  use Agent

  defstruct [:kind, :params]

  @type t :: %__MODULE__{kind: atom(), params: list(number())}

  @doc "Compute the area of a shape struct."
  def area(%__MODULE__{kind: :circle, params: [r]}), do: :math.pi() * r * r
  def area(%__MODULE__{kind: :rectangle, params: [w, h]}), do: w * h
  def area(%__MODULE__{kind: :triangle, params: [a, b, c]}) do
    s = (a + b + c) / 2.0
    :math.sqrt(s * (s - a) * (s - b) * (s - c))
  end

  @doc "Return the shape with the largest area from a list."
  def largest(shapes) when is_list(shapes) do
    Enum.max_by(shapes, &area/1)
  end

  @doc "Describe a shape as a human-readable string."
  def describe(%__MODULE__{kind: :circle, params: [r]}),
    do: "Circle with radius #{r}"
  def describe(%__MODULE__{kind: :rectangle, params: [w, h]}),
    do: "Rectangle #{w}x#{h}"
  def describe(%__MODULE__{kind: :triangle, params: params}),
    do: "Triangle with sides #{Enum.join(params, ", ")}"

  @doc "Build a circle shape."
  def circle(r), do: %__MODULE__{kind: :circle, params: [r]}

  defp validate_positive(n) when n > 0, do: :ok
  defp validate_positive(_), do: {:error, :non_positive}
end
