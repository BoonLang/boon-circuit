# Number Specialization Experiment

Status: rejected and removed on 2026-07-22.

## Decision Gate

The unified goal allowed an internal numeric specialization only when it met
all of these conditions:

- Boon's observable value remained one finite IEEE-754 binary64 `NUMBER`;
- every measured candidate result was bit-for-bit equal to the baseline;
- aggregate throughput improved by at least 10%;
- no measured workload regressed by more than 2%.

The candidate failed the performance and storage gates, so no specialization
code remains in the compiler, runtime, value representation, or executor.

## Candidate

The experiment compared the existing canonical `f64` storage with a hidden
tagged representation:

```text
Specialized = Integer(i64) | Real(f64)
```

Integer addition, subtraction, and multiplication stayed in `i64` only while
the exact result remained inside the binary64 exact-integer range. All mixed,
fractional, division, and out-of-range operations fell back to canonical
binary64. Zero normalization and finite-result checks matched `FiniteReal`.

This was intentionally the strongest plausible specialization for Boon's
counter-heavy code without adding a second observable number type.

## Method

The temporary Rust harness was compiled with:

```text
rustc -C opt-level=3 -C target-cpu=native
```

Environment:

```text
rustc 1.96.0 (ac68faa20 2026-05-25)
x86_64 Intel Core i7-9700K CPU at 3.60 GHz
```

Three deterministic 4,096-operation workloads were measured:

- `counter`: exact integer increments, decrements, and resets;
- `mixed`: integer and fractional arithmetic with periodic resets;
- `waveform`: fractional add, subtract, multiply, and divide operations.

Before timing, the harness compared the baseline and candidate result bits
after every operation. Each timing sample executed 512 rounds, or 2,097,152
operations. Eleven samples per representation were alternated to reduce order
bias, and the median was reported. The complete experiment was then repeated
three times. The temporary source had SHA-256
`de34165eba18e6358bd39028b80d200d9b2884db35d3da2d869c9ca7959fa0bc`.

## Results

Representative median timings:

| Workload | Binary64 baseline | Tagged candidate | Improvement |
| --- | ---: | ---: | ---: |
| counter | 4,660,294 ns | 12,829,433 ns | -175.29% |
| mixed | 5,183,192 ns | 15,280,365 ns | -194.81% |
| waveform | 5,737,574 ns | 16,066,500 ns | -180.02% |
| aggregate | 15,581,060 ns | 44,176,298 ns | -183.53% |

The three repeated aggregate results were `-183.50%`, `-182.56%`, and
`-183.07%`. `size_of` also increased from 8 bytes for the baseline scalar to
16 bytes for the tagged candidate.

## Conclusion

The hidden integer representation makes the common path substantially slower
and doubles scalar storage. It is rejected. Boon retains one direct finite
binary64 representation, exact bounded integer conversions, and explicit GPU
precision profiles where a target genuinely requires them.
