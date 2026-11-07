@Deprecated
class Boxed<T> {
  def copy(value) {
    def nested = { item -> item }
    return nested(value)
  }
  def café() { return 1 }
}
