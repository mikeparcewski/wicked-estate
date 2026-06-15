use strict;
use warnings;
use POSIX qw(floor ceil);

package Util;

my $MAX_VALUE = 1_000_000;
my $DEFAULT_BASE = 10;

sub clamp {
    my ($val, $min, $max) = @_;
    return $min if $val < $min;
    return $max if $val > $max;
    return $val;
}

sub round_to {
    my ($val, $places) = @_;
    $places //= 0;
    my $factor = $DEFAULT_BASE ** $places;
    return floor($val * $factor + 0.5) / $factor;
}

sub summarize {
    my @nums = @_;
    my $total = 0;
    for my $n (@nums) {
        $total += clamp($n, 0, $MAX_VALUE);
    }
    my $mean = @nums ? round_to($total / scalar(@nums), 2) : 0;
    return ($total, $mean);
}

package main;

my ($sum, $avg) = Util::summarize(3, 7, 1500, 42);
print "sum=$sum avg=$avg\n";
