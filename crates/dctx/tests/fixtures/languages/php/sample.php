<?php
namespace Sample;
#[Deprecated]
class Boxed {
    public function copy($value) {
        function nested($item) { return $item; }
        return nested($value);
    }
    public function café() { return 1; }
}
