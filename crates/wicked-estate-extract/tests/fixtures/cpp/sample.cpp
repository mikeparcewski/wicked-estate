#include <iostream>
#include <vector>
#include <stdexcept>
#include <string>

class Matrix {
public:
    Matrix(size_t rows, size_t cols)
        : rows_(rows), cols_(cols), data_(rows * cols, 0.0) {}

    double &at(size_t r, size_t c) {
        if (r >= rows_ || c >= cols_) {
            throw std::out_of_range("Matrix index out of range");
        }
        return data_[r * cols_ + c];
    }

    double at(size_t r, size_t c) const {
        if (r >= rows_ || c >= cols_) {
            throw std::out_of_range("Matrix index out of range");
        }
        return data_[r * cols_ + c];
    }

    Matrix multiply(const Matrix &other) const {
        if (cols_ != other.rows_) {
            throw std::invalid_argument("Incompatible dimensions for multiply");
        }
        Matrix result(rows_, other.cols_);
        for (size_t i = 0; i < rows_; ++i) {
            for (size_t k = 0; k < cols_; ++k) {
                for (size_t j = 0; j < other.cols_; ++j) {
                    result.at(i, j) += at(i, k) * other.at(k, j);
                }
            }
        }
        return result;
    }

    void print(const std::string &label = "") const {
        if (!label.empty()) {
            std::cout << label << ":\n";
        }
        for (size_t i = 0; i < rows_; ++i) {
            for (size_t j = 0; j < cols_; ++j) {
                std::cout << at(i, j);
                if (j + 1 < cols_) std::cout << "\t";
            }
            std::cout << "\n";
        }
    }

    size_t rows() const { return rows_; }
    size_t cols() const { return cols_; }

private:
    size_t rows_;
    size_t cols_;
    std::vector<double> data_;
};

int main() {
    Matrix a(2, 3);
    a.at(0, 0) = 1; a.at(0, 1) = 2; a.at(0, 2) = 3;
    a.at(1, 0) = 4; a.at(1, 1) = 5; a.at(1, 2) = 6;

    Matrix b(3, 2);
    b.at(0, 0) = 7; b.at(0, 1) = 8;
    b.at(1, 0) = 9; b.at(1, 1) = 10;
    b.at(2, 0) = 11; b.at(2, 1) = 12;

    Matrix c = a.multiply(b);
    a.print("A");
    b.print("B");
    c.print("A x B");
    return 0;
}
