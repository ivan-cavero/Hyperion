# Frame corpus

The first corpus entry is intentionally documented here while the binary seeds
are accumulated by `cargo fuzz`. The frame target accepts arbitrary bytes and
must never panic on malformed length prefixes or payloads.
