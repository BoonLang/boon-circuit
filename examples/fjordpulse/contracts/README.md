# FjordPulse Contract Assets

`reference/` is a byte-for-byte copy of the public HTTP and realtime contract
assets from FjordPulse commit
`dd6e750c2ca9dec3041f66ceda31d30379d4027a`. It is an immutable test oracle.

The Boon implementation must preserve these contracts except for fields covered
by an approved entry in
`../traceability/compatibility_delta_ledger.json`. Changed map, Admin database,
service-topology, and deployment contracts belong under `target/` with paired
valid and invalid fixtures. Code must never silently edit a reference fixture
to make an implementation pass.

Run `cargo xtask fjordpulse-traceability verify --reference
/home/martinkavik/repos/FjordPulse` to verify the pinned source inventory. The
repository-local contract verifier additionally checks that every copied file
still matches the digest recorded in the parity manifest.
