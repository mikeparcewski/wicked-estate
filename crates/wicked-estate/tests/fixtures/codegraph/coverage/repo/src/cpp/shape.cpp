namespace geo {

class Shape {
public:
    virtual double area() const { return 0; }
    virtual ~Shape() {}
};

class Circle : public Shape {
public:
    double r = 1;
    double area() const override { return 3.14 * r * r; }
};

Circle make_circle() {
    Circle c;
    c.area();
    return c;
}

}
