package main

import (
	"fmt"
	"math"
	"sort"
)

// Point represents a 2D coordinate.
type Point struct {
	X, Y float64
}

// distance returns the Euclidean distance between two points.
func distance(a, b Point) float64 {
	dx := a.X - b.X
	dy := a.Y - b.Y
	return math.Sqrt(dx*dx + dy*dy)
}

// closestPair finds the two closest points in a slice and returns their distance.
func closestPair(points []Point) (Point, Point, float64) {
	if len(points) < 2 {
		panic("need at least two points")
	}
	best := math.MaxFloat64
	var pa, pb Point
	for i := 0; i < len(points); i++ {
		for j := i + 1; j < len(points); j++ {
			d := distance(points[i], points[j])
			if d < best {
				best = d
				pa, pb = points[i], points[j]
			}
		}
	}
	return pa, pb, best
}

// sortByX returns a new slice of points sorted by X coordinate.
func sortByX(points []Point) []Point {
	out := make([]Point, len(points))
	copy(out, points)
	sort.Slice(out, func(i, j int) bool {
		return out[i].X < out[j].X
	})
	return out
}

func main() {
	points := []Point{
		{1, 2}, {4, 6}, {7, 1}, {3, 3}, {5, 5},
	}

	sorted := sortByX(points)
	fmt.Println("Points sorted by X:")
	for _, p := range sorted {
		fmt.Printf("  (%.1f, %.1f)\n", p.X, p.Y)
	}

	a, b, d := closestPair(points)
	fmt.Printf("Closest pair: (%.1f,%.1f) and (%.1f,%.1f), distance=%.4f\n",
		a.X, a.Y, b.X, b.Y, d)
}
