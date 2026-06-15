-module(geometry).
-export([area/1, perimeter/1, describe/1]).

-record(circle,    {radius}).
-record(rectangle, {width, height}).
-record(triangle,  {a, b, c}).

%% @doc Compute the area of a shape record.
area(#circle{radius = R}) ->
    math:pi() * R * R;
area(#rectangle{width = W, height = H}) ->
    W * H;
area(#triangle{a = A, b = B, c = C}) ->
    S = (A + B + C) / 2.0,
    math:sqrt(S * (S - A) * (S - B) * (S - C)).

%% @doc Compute the perimeter of a shape record.
perimeter(#circle{radius = R}) ->
    2.0 * math:pi() * R;
perimeter(#rectangle{width = W, height = H}) ->
    2.0 * (W + H);
perimeter(#triangle{a = A, b = B, c = C}) ->
    A + B + C.

%% @doc Describe a shape as a binary string.
describe(#circle{radius = R}) ->
    io_lib:format("Circle(r=~.2f)", [R]);
describe(#rectangle{width = W, height = H}) ->
    io_lib:format("Rectangle(~.2f x ~.2f)", [W, H]);
describe(#triangle{a = A, b = B, c = C}) ->
    io_lib:format("Triangle(~.2f, ~.2f, ~.2f)", [A, B, C]).
