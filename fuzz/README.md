# Fuzz targets

libFuzzer targets for the surfaces that parse what a peer sends. They need a
nightly toolchain and `cargo-fuzz`:

```bash
rustup toolchain install nightly && cargo install cargo-fuzz
just fuzz                     # every target for 30 s: a regression guard
just fuzz-long <target> 1800  # one target for 30 minutes, keeping its corpus
```

A crash lands in `artifacts/<target>/`. Check its input in as a seed under
`seeds/<target>/` with the fix, so the bounded run replays it.

Each target's doc comment says what it checks. Throughput varies a lot
between them, and some of that is by design:

- `uri_template` spends most inputs matching URIs against fixed templates,
  which runs at matching speed. The rest compile a template of their own,
  each new to the compile cache, so they run at the speed of compiling a
  regex: a few hundred to a few thousand inputs a second. That half measures
  what a gateway pays for each new upstream template, not what a match costs.
- `sonic_decode` parses every input twice, once with each JSON backend.
- `request_state` and `request_binding` seal, open or digest each input
  cryptographically.
