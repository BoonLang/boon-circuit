# Effects observation harness (review measurement, 2026-09-30)

Standalone Cargo package that observes today's transient-effect staging from
outside the workspace. Files carry `.txt` suffixes so the workspace parser
corpus check and Cargo never pick them up from `docs/`.

Reconstruct and run (from any directory outside the repo):

```bash
H=/path/to/scratch/effects/harness; mkdir -p $H/src $H/..
cp Cargo.toml.txt $H/Cargo.toml; cp lib.rs $H/src/lib.rs
cp /home/martinkavik/repos/boon-circuit/Cargo.lock $H/Cargo.lock
cp /home/martinkavik/repos/boon-circuit/rust-toolchain.toml $H/
for f in *.bn.txt; do cp $f $H/../${f%.txt}; done
cd $H && EFFECTS_DIR=$H/.. CARGO_TARGET_DIR=/home/martinkavik/repos/boon-circuit/target \
  cargo test -- --nocapture --test-threads=1
```

`Cargo.toml.txt` points at the workspace crates by absolute path and copies the
workspace `[profile.*]` tables so the shared target dir reuses existing rlibs.
The dependency build took 1m31s wall on 2026-09-30 (see
`../raw/effects_harness_build_deps.log`); the six probes then run in 0.4 s.
