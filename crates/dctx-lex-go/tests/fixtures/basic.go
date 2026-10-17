//go:build ignore

// Package demo is a sample.
package demo

import "fmt"
import alias "example.com/alias"
import . "example.com/dot"
import _ "example.com/blank"
import (
	"strings"
	renamed "example.com/renamed"
)

// Box stores one value.
type Box[T any] struct {
	Value T
	fmt.Stringer
}

// Reader reads bytes.
type Reader interface {
	Read(p []byte) (int, error)
	fmt.Stringer
	~int | string
}

// ID aliases int.
type ID = int

const (
	// One is first.
	One = 1
	Two = 2
)

var (
	Name string = "func Decoy() {}"
	Count = len(Name)
)

// Add adds a and b.
func Add[T ~int](a, b T) T {
	return a
}

// Serve handles a box.
func (s *Box[int]) Serve(r Reader) error {
	fmt.Println(Name)
	_ = strings.TrimSpace(Name)
	return nil
}

func main() {
	raw := `func Hidden() {}`
	r := 'x'
	_ = raw
	_ = r
	Add(One, Two)
	var box Box[int]
	_ = box.Serve(nil)
}

func sKeep() {
	_ = Name
}
