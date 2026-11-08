class Boxed {
  [int] Copy([int] $value) {
    return $value
  }
}
function Invoke-Copy {
  param([int] $value)
  function Invoke-Nested { return 1 }
  Invoke-Nested
}
function café { return 1 }
