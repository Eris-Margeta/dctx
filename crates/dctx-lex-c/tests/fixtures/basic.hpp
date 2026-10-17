/// Shapes live in demo.
#include <string>
#include "box.h"
#define API
namespace demo {
/// A boxed value.
template <typename T>
class Box {
public:
  explicit Box(T value);
  T area() const;
  T value;
private:
  T hidden;
};
/// Area multiplies the sides.
template <typename T>
T Box<T>::area() const { return value; }
using Name = std::string;
enum class Kind { One, Two };
union Bits { int raw; char bytes[4]; };
void run(const Box<int>& box) {
  const char* raw = R"cpp(int Hidden() { fake(); })cpp";
  box.area();
  auto* node = &box;
  node->value;
  (void)raw;
}
}
