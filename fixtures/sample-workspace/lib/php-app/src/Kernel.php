<?php
namespace App;

use App\Http\Router;
use Monolog\Logger;

// TODO: middleware pipeline
final class Kernel { public function __construct(private Router $r, private Logger $l) {} }
