<?php
function load() {
    try { parse(); } catch (Exception $e) {}
    try { parse(); } catch (Exception $e) { /* ignore */ }
}

function sum($a, $b) { return add($a, $b); }

function build() {
    $w = new Widget();
    if ($w !== null) {
        $w->init();
    }
    return $w;
}

function bump($counter) {
    // increment counter
    $counter->increment();
}
