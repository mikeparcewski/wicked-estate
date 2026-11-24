package p;
public class Shapes {
  int f(int x) { return x; }
  int f(String s) { return f(s.length()); }
  record Point(int a) { int g() { return a; } }
  int use() { return new Point(1).g(); }
}
