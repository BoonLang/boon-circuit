# NovyWave Real Waveform Fixtures

These small files exercise the generic Wellen bridge without placing decoded
waveform data in Boon source:

- `simple.vcd` and `simple_test.ghw` come from the NovyWave test corpus.
- `basic_test.fst` comes from the upstream `ekiwi/wellen` Verilator fixture
  corpus, which is distributed under the repository's BSD-3-Clause license.

The functional bridge tests detect each format from file contents through
official `wellen`; they do not branch on these filenames.
