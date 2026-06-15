import "math" as math;
import "utils" as $utils;

def square: . * .;

def sum_array: reduce .[] as $x (0; . + $x);

def normalize(total): if total == 0 then 0 else . / total end;

def top_n(n): sort_by(-.) | .[0:n];

.items
| map(select(.active == true))
| map(.score | square)
| top_n(5)
| sum_array
| normalize(100)
