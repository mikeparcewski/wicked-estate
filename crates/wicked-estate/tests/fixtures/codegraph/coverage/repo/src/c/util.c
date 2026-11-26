#include "util.h"

int add(int a, int b) { return a + b; }

struct point origin(void) {
    struct point p = { add(0, 0), 0 };
    return p;
}
