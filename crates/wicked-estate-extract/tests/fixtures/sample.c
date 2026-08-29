#include <stdio.h>
#include <math.h>

#define MAX_VECTORS 100
#define EPSILON 1e-9

typedef struct Vector2 Vector2;
typedef struct Vector2 Vec2;
typedef unsigned int uint;

struct Vector2 {
    double x;
    double y;
};

enum Color {
    RED,
    GREEN,
    BLUE
};

double magnitude(struct Vector2 v) {
    return sqrt(v.x * v.x + v.y * v.y);
}

struct Vector2 scale(struct Vector2 v, double factor) {
    struct Vector2 result;
    result.x = v.x * factor;
    result.y = v.y * factor;
    return result;
}

int main(void) {
    struct Vector2 v = {3.0, 4.0};
    double m = magnitude(v);
    struct Vector2 scaled = scale(v, 2.0);
    printf("magnitude=%.2f scaled=(%.1f,%.1f)\n", m, scaled.x, scaled.y);
    return 0;
}
