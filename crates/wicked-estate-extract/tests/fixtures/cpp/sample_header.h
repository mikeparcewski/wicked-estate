#ifndef SAMPLE_HEADER_H
#define SAMPLE_HEADER_H

#include <cstdint>

namespace util {

class Accumulator {
public:
    void add(std::int64_t v);
    std::int64_t total() const { return total_; }

private:
    std::int64_t total_;
};

inline std::int64_t doubled(const Accumulator &acc) {
    return acc.total() * 2;
}

} // namespace util

#endif // SAMPLE_HEADER_H
