#include <string>
#include <cmath>

#define MAX_SCALE 100.0

using Scalar = double;
typedef unsigned int uint32;

struct Vector3 {
    double x, y, z;
};

enum class Axis {
    X,
    Y,
    Z
};

class Transform {
public:
    explicit Transform(Scalar scale) : scale_(scale) {}

    Vector3 apply(const Vector3& v) const {
        return multiply(v, scale_);
    }

    double norm(const Vector3& v) const {
        return std::sqrt(v.x * v.x + v.y * v.y + v.z * v.z);
    }

private:
    Scalar scale_;

    Vector3 multiply(const Vector3& v, double s) const {
        return {v.x * s, v.y * s, v.z * s};
    }
};

double dot(const Vector3& a, const Vector3& b) {
    return a.x * b.x + a.y * b.y + a.z * b.z;
}

int main() {
    Transform t(2.0);
    Vector3 v{1.0, 2.0, 3.0};
    Vector3 scaled = t.apply(v);
    double d = dot(v, scaled);
    return 0;
}
