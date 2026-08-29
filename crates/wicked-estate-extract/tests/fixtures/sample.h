#pragma once
#include <cstdint>

class Foo {
public:
  int  bar(int x);
  void reset();
  int  inlineDef() { return 1; }
  virtual void pure() = 0;
  int count;
  static int shared;
};
int freestanding();
int definedHere() { return 0; }
struct Bar { int a; void m(); };
