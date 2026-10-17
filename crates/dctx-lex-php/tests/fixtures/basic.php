<?php
namespace App\Domain;

use Vendor\Support\{Base, Contract as Port};
use function Vendor\Tools\{format, render as view};
use const Vendor\Config\VERSION;
require_once 'bootstrap.php';
include ('view.phtml');

/** Reusable logging. */
trait Logging {
    protected function log(string $message): void { record($message); }
}

interface Contract extends \Countable {
    public function count(): int;
}

/** Handles requests. More detail is omitted from the summary. */
#[Route('/service')]
final class Service extends Base implements Contract, \JsonSerializable {
    use Logging;
    public const string KIND = 'service', OTHER = 'other';
    private readonly ?Port $port;
    protected int $count = 0, $total = 1;

    /** Runs a request. */
    public function run(Port $port, array $options = []): \Vendor\Result {
        $this->log('running');
        Helper::build();
        $kind = Service::KIND;
        $type = Service::class;
        $port->unknown();
        $callback();
        $$variable();
        Helper::$dynamic();
        eval('invented_call();');
        require $options['path'];
        return \Vendor\Tools\format(new \Vendor\Result());
    }

    public function count(): int { return $this->count; }
}

enum Status: string implements Contract {
    case Ready = 'ready';
    public function count(): int { return 1; }
}

/** Creates a service. */
function create(Port $port): Service {
    return new Service();
}

const GLOBAL_FLAG = true;
?>
<article>function HtmlFake() { phantom(); }</article>
