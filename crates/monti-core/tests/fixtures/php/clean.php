<?php
function parse_or($s, $fallback) {
    try {
        return parse($s);
    } catch (ParseError $e) {
        return $fallback;
    }
}
