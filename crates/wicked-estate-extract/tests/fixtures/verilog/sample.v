// Half-adder: sum = A ^ B, carry = A & B
module half_adder (
    input  wire a,
    input  wire b,
    output wire sum,
    output wire carry
);
    assign sum   = a ^ b;
    assign carry = a & b;
endmodule


// Full-adder built from two half-adders and an OR gate.
module full_adder (
    input  wire a,
    input  wire b,
    input  wire cin,
    output wire sum,
    output wire cout
);
    wire s1, c1, c2;

    half_adder ha1 (
        .a     (a),
        .b     (b),
        .sum   (s1),
        .carry (c1)
    );

    half_adder ha2 (
        .a     (s1),
        .b     (cin),
        .sum   (sum),
        .carry (c2)
    );

    assign cout = c1 | c2;
endmodule


// 4-bit ripple-carry adder instantiating four full-adders.
module ripple_adder_4 (
    input  wire [3:0] a,
    input  wire [3:0] b,
    input  wire       cin,
    output wire [3:0] sum,
    output wire       cout
);
    wire c0, c1, c2;

    full_adder fa0 (.a(a[0]), .b(b[0]), .cin(cin), .sum(sum[0]), .cout(c0));
    full_adder fa1 (.a(a[1]), .b(b[1]), .cin(c0),  .sum(sum[1]), .cout(c1));
    full_adder fa2 (.a(a[2]), .b(b[2]), .cin(c1),  .sum(sum[2]), .cout(c2));
    full_adder fa3 (.a(a[3]), .b(b[3]), .cin(c2),  .sum(sum[3]), .cout(cout));
endmodule
