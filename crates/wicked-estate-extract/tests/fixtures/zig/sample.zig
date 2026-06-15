const std = @import("std");

/// Computes the nth Fibonacci number iteratively.
pub fn fibonacci(n: u64) u64 {
    if (n <= 1) return n;
    var a: u64 = 0;
    var b: u64 = 1;
    var i: u64 = 2;
    while (i <= n) : (i += 1) {
        const tmp = a + b;
        a = b;
        b = tmp;
    }
    return b;
}

/// Returns true if n is prime, false otherwise.
pub fn isPrime(n: u64) bool {
    if (n < 2) return false;
    if (n == 2) return true;
    if (n % 2 == 0) return false;
    var d: u64 = 3;
    while (d * d <= n) : (d += 2) {
        if (n % d == 0) return false;
    }
    return true;
}

/// Collects Fibonacci numbers up to limit that are also prime.
pub fn fibonacciPrimes(limit: u64, allocator: std.mem.Allocator) ![]u64 {
    var list = std.ArrayList(u64).init(allocator);
    var i: u64 = 0;
    while (true) {
        const f = fibonacci(i);
        if (f > limit) break;
        if (isPrime(f)) {
            try list.append(f);
        }
        i += 1;
    }
    return list.toOwnedSlice();
}

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    const primes = try fibonacciPrimes(1000, allocator);
    defer allocator.free(primes);

    const stdout = std.io.getStdOut().writer();
    try stdout.print("Fibonacci primes up to 1000: ", .{});
    for (primes, 0..) |p, idx| {
        if (idx > 0) try stdout.print(", ", .{});
        try stdout.print("{}", .{p});
    }
    try stdout.print("\n", .{});
}
