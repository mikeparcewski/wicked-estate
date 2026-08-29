#ifndef GUARDED_HEADER_HPP
#define GUARDED_HEADER_HPP

#include <cstdint>

class Widget; // forward declaration — must not become a Class node

namespace geo {

class Foo {
public:
    int bar(int x);
    void reset();
    virtual void pure() = 0;
    int count;
    static int shared;
};

struct Bar {
    int a;
    void m();
};

inline int Foo::bar(int x) { return x + 1; }

} // namespace geo

extern "C" {
void c_api(void);
}

inline void useFoo(geo::Foo *f) {
    struct Bar *b = nullptr;
    (void)b;
    (void)f;
}

#endif // GUARDED_HEADER_HPP
