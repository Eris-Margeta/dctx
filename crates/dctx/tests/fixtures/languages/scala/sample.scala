@deprecated("fixture", "1")
class Boxed[T] {
  def copy[U](value: U): U = {
    def nested(item: U): U = item
    nested(value)
  }
  def café(): Int = 1
}
