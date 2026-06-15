module sample;

import std.stdio;
import std.algorithm : sort, map;
import std.array : array;
import std.math : sqrt;

struct Vec2 {
    double x, y;

    double length() const {
        return sqrt(x * x + y * y);
    }

    Vec2 opAdd(Vec2 other) const {
        return Vec2(x + other.x, y + other.y);
    }

    Vec2 opSub(Vec2 other) const {
        return Vec2(x - other.x, y - other.y);
    }

    double dot(Vec2 other) const {
        return x * other.x + y * other.y;
    }
}

double angleBetween(Vec2 a, Vec2 b) {
    import std.math : acos;
    double denom = a.length() * b.length();
    if (denom == 0.0) return 0.0;
    return acos(a.dot(b) / denom);
}

Vec2[] sortByLength(Vec2[] vecs) {
    Vec2[] copy = vecs.dup;
    sort!((a, b) => a.length() < b.length())(copy);
    return copy;
}

void printVec(Vec2 v, string label = "") {
    if (label.length > 0)
        writef("%s: ", label);
    writefln("(%.3f, %.3f)  |len|=%.3f", v.x, v.y, v.length());
}

void main() {
    Vec2[] vecs = [
        Vec2(3.0, 4.0),
        Vec2(1.0, 0.0),
        Vec2(5.0, 12.0),
        Vec2(0.0, 2.0),
    ];

    writeln("Sorted by length:");
    foreach (v; sortByLength(vecs))
        printVec(v);

    Vec2 a = Vec2(1.0, 0.0);
    Vec2 b = Vec2(0.0, 1.0);
    import std.math : PI;
    writefln("Angle between (1,0) and (0,1): %.4f rad (expected %.4f)",
             angleBetween(a, b), PI / 2.0);
}
