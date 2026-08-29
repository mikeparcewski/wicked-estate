package geometry

import (
	"fmt"
	"math"
)

const Pi = 3.14159
const MaxPoints = 1000

type Degrees = float64

type UserID string
type Handler func(x int) error
type Matrix [][]float64

type Point struct {
	X float64
	Y float64
}

type Shape interface {
	Area() float64
}

type Circle struct {
	Center Point
	Radius float64
}

type Bounds struct {
	minX, minY float64
}

var defaultOrigin Point

func NewCircle(center Point, radius float64) *Circle {
	return &Circle{Center: center, Radius: radius}
}

func (c *Circle) Area() float64 {
	return math.Pi * c.Radius * c.Radius
}

func Distance(a, b Point) float64 {
	dx := a.X - b.X
	dy := a.Y - b.Y
	return math.Sqrt(dx*dx + dy*dy)
}

func Describe(s Shape) string {
	return fmt.Sprintf("area=%.2f", s.Area())
}
