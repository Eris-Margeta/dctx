package sample

type Boxed[T any] struct { Value T }
func (box Boxed[T]) Copy() T {
    nested := func(value T) T { return value }
    return nested(box.Value)
}
func café() int { return 1 }
