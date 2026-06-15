<?php

declare(strict_types=1);

use RuntimeException;
use DateTimeInterface;
use InvalidArgumentException;

interface Formatter {
    public function format(string $input): string;
}

class TextFormatter implements Formatter {
    private string $prefix;
    private int $maxLength;

    public function __construct(string $prefix = '', int $maxLength = 255) {
        $this->prefix = $prefix;
        $this->maxLength = $maxLength;
    }

    public function format(string $input): string {
        $trimmed = $this->trim($input);
        return $this->prefix . $trimmed;
    }

    public function truncate(string $input): string {
        return substr($input, 0, $this->maxLength);
    }

    private function trim(string $input): string {
        return trim($input);
    }
}

function buildFormatter(string $prefix): TextFormatter {
    return new TextFormatter($prefix, 100);
}

function applyFormat(Formatter $fmt, string $text): string {
    return $fmt->format($text);
}
