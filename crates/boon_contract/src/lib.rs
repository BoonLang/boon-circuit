#![forbid(unsafe_code)]

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::io::{self, Write};
use std::str::FromStr;

pub const SOURCE_BUNDLE_DIGEST_V1_DOMAIN: &[u8] = b"boon.source-bundle.v1\0";
pub const CANONICAL_SERDE_CBOR_V1: &str = "boon.canonical-serde-cbor.v1";

/// Canonical bytes for compiler-owned, ordered serde DTOs.
///
/// V1 admits structs, sequences and maps whose Rust representation already
/// defines a stable order (notably `Vec` and `BTreeMap`). Schema owners must
/// never feed an unordered map into this boundary. `ciborium` emits the
/// shortest deterministic scalar representation; schema/domain separation is
/// supplied by the caller's digest domain.
pub fn canonical_serde_cbor_v1<T: Serialize + ?Sized>(
    value: &T,
) -> Result<Vec<u8>, CanonicalEncodingError> {
    let mut bytes = Vec::new();
    ciborium::ser::into_writer(value, &mut bytes)
        .map_err(|error| CanonicalEncodingError::new(error.to_string()))?;
    Ok(bytes)
}

pub fn canonical_serde_hash_v1<T: Serialize + ?Sized>(
    domain: &[u8],
    value: &T,
) -> Result<[u8; 32], CanonicalEncodingError> {
    canonical_serde_hash_v1_with_buffer(domain, value, &mut Vec::new())
}

/// Canonically hashes a potentially large value without materializing its
/// encoded CBOR payload.
///
/// The V1 digest commits the encoded byte length before the bytes themselves,
/// so a deterministic counting pass is followed by a hashing pass. Both use
/// the same `ciborium` serializer as [`canonical_serde_hash_v1`], preserving
/// the exact digest contract while bounding auxiliary memory.
pub fn canonical_serde_hash_v1_streaming<T: Serialize + ?Sized>(
    domain: &[u8],
    value: &T,
) -> Result<[u8; 32], CanonicalEncodingError> {
    canonical_serde_hashes_v1_streaming([domain], value).map(|[digest]| digest)
}

/// Streaming counterpart to [`canonical_serde_hashes_v1_with_buffer`].
/// Serialization is counted once and streamed once regardless of domain count.
pub fn canonical_serde_hashes_v1_streaming<const N: usize, T: Serialize + ?Sized>(
    domains: [&[u8]; N],
    value: &T,
) -> Result<[[u8; 32]; N], CanonicalEncodingError> {
    let mut counter = CanonicalLengthWriter::default();
    ciborium::ser::into_writer(value, &mut counter)
        .map_err(|error| CanonicalEncodingError::new(error.to_string()))?;

    let mut hashers = domains.map(|domain| {
        let mut hasher = Sha256::new();
        hasher.update(domain);
        hasher.update(counter.length.to_be_bytes());
        hasher
    });
    let mut writer = CanonicalHashesWriter {
        hashers: &mut hashers,
        length: 0,
    };
    ciborium::ser::into_writer(value, &mut writer)
        .map_err(|error| CanonicalEncodingError::new(error.to_string()))?;
    if writer.length != counter.length {
        return Err(CanonicalEncodingError::new(
            "canonical streaming serialization changed length between passes",
        ));
    }
    Ok(hashers.map(|hasher| hasher.finalize().into()))
}

#[derive(Default)]
struct CanonicalLengthWriter {
    length: u64,
}

impl Write for CanonicalLengthWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let length = u64::try_from(bytes.len())
            .map_err(|_| io::Error::other("canonical payload chunk exceeds u64"))?;
        self.length = self
            .length
            .checked_add(length)
            .ok_or_else(|| io::Error::other("canonical payload exceeds u64"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct CanonicalHashesWriter<'a, const N: usize> {
    hashers: &'a mut [Sha256; N],
    length: u64,
}

impl<const N: usize> Write for CanonicalHashesWriter<'_, N> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let length = u64::try_from(bytes.len())
            .map_err(|_| io::Error::other("canonical payload chunk exceeds u64"))?;
        self.length = self
            .length
            .checked_add(length)
            .ok_or_else(|| io::Error::other("canonical payload exceeds u64"))?;
        for hasher in self.hashers.iter_mut() {
            hasher.update(bytes);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Canonically hashes `value` while reusing caller-owned encoding storage.
///
/// This is byte-for-byte equivalent to [`canonical_serde_hash_v1`]. It exists
/// for compiler passes that hash large inventories of small records and would
/// otherwise allocate a fresh `Vec` for every record.
pub fn canonical_serde_hash_v1_with_buffer<T: Serialize + ?Sized>(
    domain: &[u8],
    value: &T,
    bytes: &mut Vec<u8>,
) -> Result<[u8; 32], CanonicalEncodingError> {
    canonical_serde_hashes_v1_with_buffer([domain], value, bytes).map(|[digest]| digest)
}

/// Canonically serializes `value` once and hashes those exact bytes under
/// multiple domains.
///
/// This preserves the byte contract of [`canonical_serde_hash_v1`] for each
/// result while avoiding repeated serialization when one compiler artifact is
/// intentionally committed by more than one domain-separated digest.
pub fn canonical_serde_hashes_v1_with_buffer<const N: usize, T: Serialize + ?Sized>(
    domains: [&[u8]; N],
    value: &T,
    bytes: &mut Vec<u8>,
) -> Result<[[u8; 32]; N], CanonicalEncodingError> {
    bytes.clear();
    ciborium::ser::into_writer(value, &mut *bytes)
        .map_err(|error| CanonicalEncodingError::new(error.to_string()))?;
    let length = u64::try_from(bytes.len())
        .map_err(|_| CanonicalEncodingError::new("canonical payload exceeds u64"))?
        .to_be_bytes();
    Ok(domains.map(|domain| {
        let mut hasher = Sha256::new();
        hasher.update(domain);
        hasher.update(length);
        hasher.update(bytes.as_slice());
        hasher.finalize().into()
    }))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalEncodingError {
    message: String,
}

impl CanonicalEncodingError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CanonicalEncodingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for CanonicalEncodingError {}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceBundleDigestV1([u8; 32]);

impl SourceBundleDigestV1 {
    pub fn new<'a>(
        entrypoint: &str,
        units: impl IntoIterator<Item = SourceBundleUnit<'a>>,
    ) -> Result<Self, SourceBundleError> {
        Ok(CanonicalSourceBundleV1::new(entrypoint, units)?.digest())
    }

    fn from_canonical(
        entrypoint: &str,
        units: &[CanonicalSourceUnitV1<'_>],
    ) -> Result<Self, SourceBundleError> {
        let mut hasher = Sha256::new();
        hasher.update(SOURCE_BUNDLE_DIGEST_V1_DOMAIN);
        hash_part(&mut hasher, entrypoint.as_bytes());
        hasher.update(
            u64::try_from(units.len())
                .map_err(|_| SourceBundleError::new("source bundle unit count exceeds u64"))?
                .to_be_bytes(),
        );
        for unit in units {
            hash_part(&mut hasher, unit.path.as_bytes());
            hash_part(&mut hasher, unit.source.as_bytes());
        }
        Ok(Self(hasher.finalize().into()))
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(self) -> String {
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            use fmt::Write as _;
            write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
        }
        output
    }
}

/// The sole accepted compiler-facing representation of a source bundle.
///
/// Constructing this value normalizes and validates every path, sorts units by
/// normalized path bytes, rejects aliases and missing entrypoints, and computes
/// the digest over the same representation exposed to the compiler.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalSourceBundleV1<'a> {
    entrypoint: String,
    units: Vec<CanonicalSourceUnitV1<'a>>,
    digest: SourceBundleDigestV1,
}

impl<'a> CanonicalSourceBundleV1<'a> {
    pub fn new(
        entrypoint: &str,
        units: impl IntoIterator<Item = SourceBundleUnit<'a>>,
    ) -> Result<Self, SourceBundleError> {
        let entrypoint = normalize_source_path(entrypoint)?;
        let mut units = units
            .into_iter()
            .map(|unit| {
                Ok(CanonicalSourceUnitV1 {
                    path: normalize_source_path(unit.path)?,
                    source: unit.source,
                })
            })
            .collect::<Result<Vec<_>, SourceBundleError>>()?;
        units.sort_by(|left, right| left.path.as_bytes().cmp(right.path.as_bytes()));

        let mut paths = BTreeSet::new();
        for unit in &units {
            if !paths.insert(unit.path.as_str()) {
                return Err(SourceBundleError::new(format!(
                    "source bundle contains duplicate normalized path `{}`",
                    unit.path
                )));
            }
        }
        if !paths.contains(entrypoint.as_str()) {
            return Err(SourceBundleError::new(format!(
                "source bundle entrypoint `{entrypoint}` is not one of its units"
            )));
        }

        let digest = SourceBundleDigestV1::from_canonical(&entrypoint, &units)?;
        Ok(Self {
            entrypoint,
            units,
            digest,
        })
    }

    pub fn entrypoint(&self) -> &str {
        &self.entrypoint
    }

    pub fn units(&self) -> &[CanonicalSourceUnitV1<'a>] {
        &self.units
    }

    pub const fn digest(&self) -> SourceBundleDigestV1 {
        self.digest
    }
}

impl fmt::Display for SourceBundleDigestV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for SourceBundleDigestV1 {
    type Err = SourceBundleError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(SourceBundleError::new(
                "SourceBundleDigestV1 must be exactly 64 lowercase hexadecimal characters",
            ));
        }
        let mut bytes = [0_u8; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
                .map_err(|_| SourceBundleError::new("invalid SourceBundleDigestV1 hexadecimal"))?;
        }
        Ok(Self(bytes))
    }
}

impl Serialize for SourceBundleDigestV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for SourceBundleDigestV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceBundleUnit<'a> {
    pub path: &'a str,
    pub source: &'a str,
}

impl<'a> SourceBundleUnit<'a> {
    pub const fn new(path: &'a str, source: &'a str) -> Self {
        Self { path, source }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalSourceUnitV1<'a> {
    path: String,
    source: &'a str,
}

impl CanonicalSourceUnitV1<'_> {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub const fn source(&self) -> &str {
        self.source
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceBundleError {
    message: String,
}

impl SourceBundleError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for SourceBundleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for SourceBundleError {}

pub fn normalize_source_path(path: &str) -> Result<String, SourceBundleError> {
    let normalized = path.replace('\\', "/");
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.starts_with("//")
        || normalized
            .as_bytes()
            .get(1)
            .is_some_and(|byte| *byte == b':')
    {
        return Err(SourceBundleError::new(format!(
            "source path `{path}` must be project-relative"
        )));
    }
    if normalized
        .split('/')
        .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return Err(SourceBundleError::new(format!(
            "source path `{path}` contains an empty, `.` or `..` component"
        )));
    }
    Ok(normalized)
}

fn hash_part(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update(
        u64::try_from(bytes.len())
            .expect("Rust byte slices cannot exceed u64")
            .to_be_bytes(),
    );
    hasher.update(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct GoldenFixture {
        schema: String,
        entrypoint: String,
        units: Vec<GoldenUnit>,
        canonical_entrypoint: String,
        canonical_paths: Vec<String>,
        digest: String,
    }

    #[derive(Deserialize)]
    struct GoldenUnit {
        path: String,
        source: String,
    }

    fn fixture(order: bool) -> SourceBundleDigestV1 {
        let units = [
            SourceBundleUnit::new("app/main.bn", "value: helper.value\n"),
            SourceBundleUnit::new("app/helper.bn", "helper: [value: 42]\n"),
        ];
        SourceBundleDigestV1::new(
            "app/main.bn",
            if order {
                units.into_iter().collect::<Vec<_>>()
            } else {
                units.into_iter().rev().collect::<Vec<_>>()
            },
        )
        .unwrap()
    }

    #[test]
    fn digest_is_order_independent_and_path_normalized() {
        assert_eq!(fixture(true), fixture(false));
        assert_eq!(
            fixture(true).to_string(),
            "035895963ca61071fd82d558448599a6d59a9049cfedc8c8168590c36682bdbe"
        );
        assert_eq!(
            fixture(true),
            SourceBundleDigestV1::new(
                "app\\main.bn",
                [
                    SourceBundleUnit::new("app\\helper.bn", "helper: [value: 42]\n"),
                    SourceBundleUnit::new("app\\main.bn", "value: helper.value\n"),
                ],
            )
            .unwrap()
        );
    }

    #[test]
    fn canonical_bundle_exposes_the_exact_hashed_compiler_input() {
        let fixture: GoldenFixture = serde_json::from_str(include_str!(
            "../../../fixtures/contracts/source_bundle_digest_v1.json"
        ))
        .unwrap();
        assert_eq!(fixture.schema, "boon.source-bundle-golden.v1");
        let bundle = CanonicalSourceBundleV1::new(
            &fixture.entrypoint,
            fixture
                .units
                .iter()
                .map(|unit| SourceBundleUnit::new(&unit.path, &unit.source)),
        )
        .unwrap();
        assert_eq!(bundle.entrypoint(), fixture.canonical_entrypoint);
        assert_eq!(
            bundle
                .units()
                .iter()
                .map(|unit| unit.path())
                .collect::<Vec<_>>(),
            fixture
                .canonical_paths
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
        );
        assert_eq!(bundle.digest().to_string(), fixture.digest);
    }

    #[test]
    fn entrypoint_paths_and_exact_source_bytes_are_identity() {
        let base = fixture(true);
        let other_entrypoint = SourceBundleDigestV1::new(
            "app/helper.bn",
            [
                SourceBundleUnit::new("app/main.bn", "value: helper.value\n"),
                SourceBundleUnit::new("app/helper.bn", "helper: [value: 42]\n"),
            ],
        )
        .unwrap();
        let other_source = SourceBundleDigestV1::new(
            "app/main.bn",
            [
                SourceBundleUnit::new("app/main.bn", "value: helper.value\r\n"),
                SourceBundleUnit::new("app/helper.bn", "helper: [value: 42]\n"),
            ],
        )
        .unwrap();
        assert_ne!(base, other_entrypoint);
        assert_ne!(base, other_source);
    }

    #[test]
    fn invalid_and_ambiguous_bundles_fail_closed() {
        for path in [
            "",
            "/main.bn",
            "../main.bn",
            "./main.bn",
            "a//main.bn",
            "C:\\main.bn",
        ] {
            assert!(normalize_source_path(path).is_err(), "{path}");
        }
        assert!(
            SourceBundleDigestV1::new(
                "main.bn",
                [
                    SourceBundleUnit::new("main.bn", "a"),
                    SourceBundleUnit::new(".\\main.bn", "b"),
                ],
            )
            .is_err()
        );
        assert!(
            SourceBundleDigestV1::new("missing.bn", [SourceBundleUnit::new("main.bn", "a")],)
                .is_err()
        );
    }

    #[test]
    fn text_and_serde_round_trip_as_canonical_hex() {
        let digest = fixture(true);
        let text = digest.to_string();
        assert_eq!(text.len(), 64);
        assert_eq!(text.parse::<SourceBundleDigestV1>().unwrap(), digest);
        assert!("A".repeat(64).parse::<SourceBundleDigestV1>().is_err());
        let json = serde_json::to_string(&digest).unwrap();
        assert_eq!(
            serde_json::from_str::<SourceBundleDigestV1>(&json).unwrap(),
            digest
        );
    }

    #[test]
    fn canonical_serde_cbor_is_deterministic_and_domain_separated() {
        let value = std::collections::BTreeMap::from([
            ("alpha".to_owned(), vec![1_u64, 2, 3]),
            ("beta".to_owned(), vec![5_u64, 8]),
        ]);
        let first = canonical_serde_cbor_v1(&value).unwrap();
        let second = canonical_serde_cbor_v1(&value).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            first,
            vec![
                0xa2, 0x65, b'a', b'l', b'p', b'h', b'a', 0x83, 0x01, 0x02, 0x03, 0x64, b'b', b'e',
                b't', b'a', 0x82, 0x05, 0x08,
            ],
            "canonical CBOR V1 bytes changed"
        );
        assert_eq!(
            canonical_serde_hash_v1(b"first-domain\0", &value).unwrap(),
            canonical_serde_hash_v1(b"first-domain\0", &value).unwrap()
        );
        let mut scratch = Vec::new();
        assert_eq!(
            canonical_serde_hash_v1(b"first-domain\0", &value).unwrap(),
            canonical_serde_hash_v1_with_buffer(b"first-domain\0", &value, &mut scratch).unwrap(),
            "caller-owned encoding storage must not change canonical hashes"
        );
        assert_eq!(
            canonical_serde_hash_v1(b"first-domain\0", &value).unwrap(),
            canonical_serde_hash_v1_streaming(b"first-domain\0", &value).unwrap(),
            "streaming canonical hashing must preserve exact V1 bytes"
        );
        let [first_domain, second_domain] = canonical_serde_hashes_v1_with_buffer(
            [b"first-domain\0", b"second-domain\0"],
            &value,
            &mut scratch,
        )
        .unwrap();
        assert_eq!(
            [first_domain, second_domain],
            canonical_serde_hashes_v1_streaming([b"first-domain\0", b"second-domain\0"], &value,)
                .unwrap(),
            "multi-domain streaming must preserve exact V1 bytes"
        );
        assert_eq!(
            first_domain,
            canonical_serde_hash_v1(b"first-domain\0", &value).unwrap()
        );
        assert_eq!(
            second_domain,
            canonical_serde_hash_v1(b"second-domain\0", &value).unwrap()
        );
        assert_eq!(
            canonical_serde_hash_v1(b"first-domain\0", &value).unwrap(),
            [
                0xa6, 0xcf, 0x3b, 0x79, 0x59, 0xdb, 0x82, 0x10, 0x33, 0x69, 0x9a, 0x5a, 0x9d, 0x3b,
                0x87, 0xf2, 0x87, 0xd7, 0x9b, 0x01, 0x48, 0xdc, 0x1c, 0x03, 0x18, 0xd7, 0x32, 0xc3,
                0xfc, 0x2a, 0x25, 0x46,
            ],
            "canonical CBOR V1 domain hash changed"
        );
        assert_ne!(
            canonical_serde_hash_v1(b"first-domain\0", &value).unwrap(),
            canonical_serde_hash_v1(b"second-domain\0", &value).unwrap()
        );
    }
}
