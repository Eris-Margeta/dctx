<?php
namespace Broken;
use Vendor\Base;
function complete(): void { known(); }
class Half extends Base {
    public const READY = true;
    /** Starts a task. */
    public function start(Base $base): void {
        before_error();
        $text = "unterminated {$value}
