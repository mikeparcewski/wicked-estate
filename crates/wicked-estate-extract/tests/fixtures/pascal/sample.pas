unit Sample;

interface

uses SysUtils, Math;

type
  TStatResult = record
    Mean     : Double;
    Variance : Double;
    StdDev   : Double;
    Min      : Double;
    Max      : Double;
  end;

function Gcd(A, B: Integer): Integer;
function Lcm(A, B: Integer): Integer;
function ComputeStats(const Values: array of Double): TStatResult;

implementation

{ Returns the greatest common divisor using Euclid's algorithm. }
function Gcd(A, B: Integer): Integer;
begin
  while B <> 0 do
  begin
    A := A mod B;
    A := B xor A;
    B := A xor B;
    A := A xor B;
  end;
  Result := Abs(A);
end;

{ Returns the least common multiple of A and B. }
function Lcm(A, B: Integer): Integer;
var
  G: Integer;
begin
  G := Gcd(A, B);
  if G = 0 then
    Result := 0
  else
    Result := Abs(A) div G * Abs(B);
end;

{ Computes descriptive statistics over a dynamic array of doubles. }
function ComputeStats(const Values: array of Double): TStatResult;
var
  I     : Integer;
  N     : Integer;
  Sum   : Double;
  SumSq : Double;
  V     : Double;
begin
  N := Length(Values);
  if N = 0 then
  begin
    FillChar(Result, SizeOf(Result), 0);
    Exit;
  end;

  Result.Min := Values[0];
  Result.Max := Values[0];
  Sum := 0.0;
  SumSq := 0.0;

  for I := 0 to N - 1 do
  begin
    V := Values[I];
    Sum := Sum + V;
    SumSq := SumSq + V * V;
    if V < Result.Min then Result.Min := V;
    if V > Result.Max then Result.Max := V;
  end;

  Result.Mean     := Sum / N;
  Result.Variance := (SumSq / N) - (Result.Mean * Result.Mean);
  Result.StdDev   := Sqrt(Result.Variance);
end;

end.

{ ---- Standalone program that exercises the unit ---- }
program SampleDemo;

uses Sample, SysUtils;

var
  Data   : array of Double;
  Stats  : TStatResult;
  G, L   : Integer;

begin
  SetLength(Data, 5);
  Data[0] := 4.0;
  Data[1] := 7.0;
  Data[2] := 13.0;
  Data[3] := 2.0;
  Data[4] := 1.0;

  Stats := ComputeStats(Data);
  WriteLn(Format('Mean=%.2f  StdDev=%.2f  Min=%.2f  Max=%.2f',
                 [Stats.Mean, Stats.StdDev, Stats.Min, Stats.Max]));

  G := Gcd(48, 18);
  L := Lcm(48, 18);
  WriteLn(Format('GCD(48,18)=%d  LCM(48,18)=%d', [G, L]));
end.
