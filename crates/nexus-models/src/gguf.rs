//! GGUF header parsing (MODEL-MANAGER §5). Reads the key/value header only, never the tensors, and
//! guards every length so a corrupt or hostile file cannot make us allocate without bound.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde::{Deserialize, Serialize};

/// `GGUF` as a little-endian u32.
pub const MAGIC: u32 = 0x4655_4747;
/// Longest string value we will read (64 MiB).
pub const MAX_STRING: u64 = 64 * 1024 * 1024;
/// Most array items we will walk.
pub const MAX_ARRAY_ITEMS: u64 = 10_000_000;
/// Most key/value pairs a header may declare.
pub const MAX_KV: u64 = 100_000;
/// Strings longer than this are not kept in memory (only their length is recorded).
const KEEP_STRING: u64 = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum GgufError {
    #[error("not a GGUF file (bad magic)")]
    BadMagic,
    #[error("unsupported GGUF version {0} (this reader handles 2 and 3)")]
    BadVersion(u32),
    #[error("the GGUF header is corrupt: {0}")]
    Corrupt(&'static str),
    #[error("could not read the file: {0}")]
    Io(#[from] std::io::Error),
}

/// What the app needs to know about a model file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct GgufMeta {
    pub version: u32,
    pub tensor_count: u64,
    pub kv_count: u64,
    pub architecture: Option<String>,
    pub name: Option<String>,
    /// Human name of `general.file_type` (`Q4_K_M`, `Q8_0`, ...), when known.
    pub quant: Option<String>,
    pub file_type: Option<u32>,
    pub context_length: Option<u64>,
    pub block_count: Option<u64>,
    pub embedding_length: Option<u64>,
    pub head_count: Option<u64>,
    pub head_count_kv: Option<u64>,
    pub license: Option<String>,
    /// `tokenizer.chat_template` exists.
    pub has_chat_template: bool,
    pub chat_template_len: Option<u64>,
    /// The template mentions `enable_thinking`.
    pub supports_thinking_toggle: bool,
}

/// Name of a `general.file_type` value (llama.cpp `llama_ftype`).
pub fn quant_name(file_type: u32) -> Option<&'static str> {
    Some(match file_type {
        0 => "F32",
        1 => "F16",
        2 => "Q4_0",
        3 => "Q4_1",
        7 => "Q8_0",
        8 => "Q5_0",
        9 => "Q5_1",
        10 => "Q2_K",
        11 => "Q3_K_S",
        12 => "Q3_K_M",
        13 => "Q3_K_L",
        14 => "Q4_K_S",
        15 => "Q4_K_M",
        16 => "Q5_K_S",
        17 => "Q5_K_M",
        18 => "Q6_K",
        19 => "IQ2_XXS",
        20 => "IQ2_XS",
        24 => "IQ1_S",
        25 => "IQ4_NL",
        30 => "IQ4_XS",
        32 => "BF16",
        _ => return None,
    })
}

#[derive(Clone, Debug)]
enum Value {
    UInt(u64),
    Int(i64),
    Float(f64),
    Str { text: Option<String>, len: u64 },
    Skipped,
}

struct Reader<R: Read + Seek> {
    r: R,
    /// Bytes still allowed before the file's end (`None` = unknown).
    remaining: Option<u64>,
}

impl<R: Read + Seek> Reader<R> {
    fn take(&mut self, n: u64) -> Result<(), GgufError> {
        if let Some(rem) = self.remaining {
            if n > rem {
                return Err(GgufError::Corrupt("a length runs past the end of the file"));
            }
            self.remaining = Some(rem - n);
        }
        Ok(())
    }
    fn bytes<const N: usize>(&mut self) -> Result<[u8; N], GgufError> {
        self.take(N as u64)?;
        let mut b = [0u8; N];
        self.r.read_exact(&mut b)?;
        Ok(b)
    }
    fn u8(&mut self) -> Result<u8, GgufError> {
        Ok(self.bytes::<1>()?[0])
    }
    fn u16(&mut self) -> Result<u16, GgufError> {
        Ok(u16::from_le_bytes(self.bytes()?))
    }
    fn u32(&mut self) -> Result<u32, GgufError> {
        Ok(u32::from_le_bytes(self.bytes()?))
    }
    fn u64(&mut self) -> Result<u64, GgufError> {
        Ok(u64::from_le_bytes(self.bytes()?))
    }
    fn skip(&mut self, n: u64) -> Result<(), GgufError> {
        self.take(n)?;
        let off = i64::try_from(n).map_err(|_| GgufError::Corrupt("a length is too large"))?;
        self.r.seek(SeekFrom::Current(off))?;
        Ok(())
    }
    fn string(&mut self, keep: bool) -> Result<(Option<String>, u64), GgufError> {
        let len = self.u64()?;
        if len > MAX_STRING {
            return Err(GgufError::Corrupt("a string is longer than 64 MB"));
        }
        if keep && len <= KEEP_STRING {
            self.take(len)?;
            let mut buf = vec![0u8; len as usize];
            self.r.read_exact(&mut buf)?;
            Ok((Some(String::from_utf8_lossy(&buf).into_owned()), len))
        } else {
            self.skip(len)?;
            Ok((None, len))
        }
    }

    fn scalar_size(ty: u32) -> Option<u64> {
        Some(match ty {
            0 | 1 | 7 => 1,
            2 | 3 => 2,
            4..=6 => 4,
            10..=12 => 8,
            _ => return None,
        })
    }

    fn value(&mut self, ty: u32, keep: bool) -> Result<Value, GgufError> {
        Ok(match ty {
            0 => Value::UInt(u64::from(self.u8()?)),
            1 => Value::Int(i64::from(self.u8()? as i8)),
            2 => Value::UInt(u64::from(self.u16()?)),
            3 => Value::Int(i64::from(self.u16()? as i16)),
            4 => Value::UInt(u64::from(self.u32()?)),
            5 => Value::Int(i64::from(self.u32()? as i32)),
            6 => Value::Float(f64::from(f32::from_le_bytes(self.bytes()?))),
            7 => {
                self.u8()?;
                Value::Skipped
            }
            8 => {
                let (text, len) = self.string(keep)?;
                Value::Str { text, len }
            }
            9 => {
                let elem = self.u32()?;
                let n = self.u64()?;
                if n > MAX_ARRAY_ITEMS {
                    return Err(GgufError::Corrupt("an array has more than 10 million items"));
                }
                if elem == 9 {
                    return Err(GgufError::Corrupt("nested arrays are not supported"));
                }
                if let Some(size) = Self::scalar_size(elem) {
                    self.skip(
                        n.checked_mul(size)
                            .ok_or(GgufError::Corrupt("array size overflows"))?,
                    )?;
                } else if elem == 8 {
                    for _ in 0..n {
                        self.string(false)?;
                    }
                } else {
                    return Err(GgufError::Corrupt("unknown array element type"));
                }
                Value::Skipped
            }
            10 => Value::UInt(self.u64()?),
            11 => Value::Int(self.u64()? as i64),
            12 => Value::Float(f64::from_le_bytes(self.bytes()?)),
            _ => return Err(GgufError::Corrupt("unknown value type")),
        })
    }
}

fn as_u64(v: &Value) -> Option<u64> {
    match v {
        Value::UInt(u) => Some(*u),
        Value::Int(i) => u64::try_from(*i).ok(),
        Value::Float(f) if *f >= 0.0 && f.fract() == 0.0 => Some(*f as u64),
        _ => None,
    }
}

fn as_string(v: &Value) -> Option<String> {
    match v {
        Value::Str { text, .. } => text.clone(),
        _ => None,
    }
}

/// Parse the header of a GGUF stream. `file_len` (when known) bounds every length.
pub fn parse_reader<R: Read + Seek>(r: R, file_len: Option<u64>) -> Result<GgufMeta, GgufError> {
    let mut rd = Reader {
        r,
        remaining: file_len,
    };
    if rd.u32().map_err(|_| GgufError::BadMagic)? != MAGIC {
        return Err(GgufError::BadMagic);
    }
    let version = rd.u32()?;
    if !(2..=3).contains(&version) {
        return Err(GgufError::BadVersion(version));
    }
    let tensor_count = rd.u64()?;
    let kv_count = rd.u64()?;
    if kv_count > MAX_KV {
        return Err(GgufError::Corrupt("too many key/value pairs"));
    }
    let mut meta = GgufMeta {
        version,
        tensor_count,
        kv_count,
        ..GgufMeta::default()
    };
    let mut arch: Option<String> = None;
    let mut per_arch: Vec<(String, Value)> = Vec::new();
    for _ in 0..kv_count {
        let (key, _) = rd.string(true)?;
        let key = key.ok_or(GgufError::Corrupt("a key is too long"))?;
        let ty = rd.u32()?;
        let keep = matches!(
            key.as_str(),
            "general.architecture" | "general.name" | "general.license" | "tokenizer.chat_template"
        );
        let wants = keep
            || key.starts_with("general.file_type")
            || arch.as_ref().is_some_and(|a| key.starts_with(&format!("{a}.")));
        let v = rd.value(ty, keep)?;
        match key.as_str() {
            "general.architecture" => {
                arch = as_string(&v);
                meta.architecture = arch.clone();
            }
            "general.name" => meta.name = as_string(&v),
            "general.license" => meta.license = as_string(&v),
            "general.file_type" => {
                meta.file_type = as_u64(&v).and_then(|x| u32::try_from(x).ok());
                meta.quant = meta.file_type.and_then(quant_name).map(str::to_owned);
            }
            "tokenizer.chat_template" => {
                if let Value::Str { text, len } = &v {
                    meta.has_chat_template = *len > 0;
                    meta.chat_template_len = Some(*len);
                    meta.supports_thinking_toggle =
                        text.as_deref().is_some_and(|t| t.contains("enable_thinking"));
                }
            }
            _ if wants => per_arch.push((key, v)),
            _ => {}
        }
    }
    if let Some(a) = &arch {
        let get = |suffix: &str| {
            per_arch
                .iter()
                .find(|(k, _)| *k == format!("{a}.{suffix}"))
                .and_then(|(_, v)| as_u64(v))
        };
        meta.context_length = get("context_length");
        meta.block_count = get("block_count");
        meta.embedding_length = get("embedding_length");
        meta.head_count = get("attention.head_count");
        meta.head_count_kv = get("attention.head_count_kv");
    }
    Ok(meta)
}

/// Parse the header of the file at `path`.
pub fn parse_file(path: &Path) -> Result<GgufMeta, GgufError> {
    let f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    parse_reader(std::io::BufReader::new(f), Some(len))
}

/// Builds small GGUF headers for tests (here and in the downloader's tests).
#[doc(hidden)]
pub mod fixture {
    /// One key/value to write.
    pub enum Kv<'a> {
        Str(&'a str, &'a str),
        U32(&'a str, u32),
        U64(&'a str, u64),
        StrArray(&'a str, &'a [&'a str]),
        F32Array(&'a str, usize),
    }

    fn put_str(out: &mut Vec<u8>, s: &str) {
        out.extend((s.len() as u64).to_le_bytes());
        out.extend(s.as_bytes());
    }

    /// A syntactically valid GGUF file (header, no real tensors).
    pub fn build(version: u32, kvs: &[Kv<'_>]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(super::MAGIC.to_le_bytes());
        out.extend(version.to_le_bytes());
        out.extend(0u64.to_le_bytes());
        out.extend((kvs.len() as u64).to_le_bytes());
        for kv in kvs {
            match kv {
                Kv::Str(k, v) => {
                    put_str(&mut out, k);
                    out.extend(8u32.to_le_bytes());
                    put_str(&mut out, v);
                }
                Kv::U32(k, v) => {
                    put_str(&mut out, k);
                    out.extend(4u32.to_le_bytes());
                    out.extend(v.to_le_bytes());
                }
                Kv::U64(k, v) => {
                    put_str(&mut out, k);
                    out.extend(10u32.to_le_bytes());
                    out.extend(v.to_le_bytes());
                }
                Kv::StrArray(k, items) => {
                    put_str(&mut out, k);
                    out.extend(9u32.to_le_bytes());
                    out.extend(8u32.to_le_bytes());
                    out.extend((items.len() as u64).to_le_bytes());
                    for i in *items {
                        put_str(&mut out, i);
                    }
                }
                Kv::F32Array(k, n) => {
                    put_str(&mut out, k);
                    out.extend(9u32.to_le_bytes());
                    out.extend(6u32.to_le_bytes());
                    out.extend((*n as u64).to_le_bytes());
                    out.extend(std::iter::repeat_n(0u8, n * 4));
                }
            }
        }
        out
    }

    /// A typical small chat model header.
    pub fn sample() -> Vec<u8> {
        build(
            3,
            &[
                Kv::Str("general.architecture", "llama"),
                Kv::Str("general.name", "Tiny Test"),
                Kv::U32("general.file_type", 15),
                Kv::Str("general.license", "apache-2.0"),
                Kv::U64("llama.context_length", 131_072),
                Kv::U32("llama.block_count", 16),
                Kv::U32("llama.embedding_length", 2048),
                Kv::U32("llama.attention.head_count", 32),
                Kv::U32("llama.attention.head_count_kv", 8),
                Kv::StrArray("tokenizer.ggml.tokens", &["a", "b", "c"]),
                Kv::F32Array("tokenizer.ggml.scores", 50_000),
                Kv::Str(
                    "tokenizer.chat_template",
                    "{% if enable_thinking %}<think>{% endif %}{{ messages }}",
                ),
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::{Kv, build, sample};
    use super::*;
    use std::io::Cursor;

    fn parse(bytes: &[u8]) -> Result<GgufMeta, GgufError> {
        parse_reader(Cursor::new(bytes.to_vec()), Some(bytes.len() as u64))
    }

    #[test]
    fn reads_the_fields_the_app_needs_and_skips_big_arrays() {
        let m = parse(&sample()).unwrap();
        assert_eq!((m.version, m.kv_count), (3, 12));
        assert_eq!(m.architecture.as_deref(), Some("llama"));
        assert_eq!(m.name.as_deref(), Some("Tiny Test"));
        assert_eq!(m.quant.as_deref(), Some("Q4_K_M"));
        assert_eq!(
            (m.context_length, m.block_count, m.embedding_length),
            (Some(131_072), Some(16), Some(2048))
        );
        assert_eq!((m.head_count, m.head_count_kv), (Some(32), Some(8)));
        assert_eq!(m.license.as_deref(), Some("apache-2.0"));
        assert!(m.has_chat_template && m.supports_thinking_toggle);
    }

    #[test]
    fn a_missing_chat_template_is_reported() {
        let m = parse(&build(
            3,
            &[
                Kv::Str("general.architecture", "qwen2"),
                Kv::U32("qwen2.block_count", 4),
            ],
        ))
        .unwrap();
        assert!(!m.has_chat_template && !m.supports_thinking_toggle);
        assert_eq!(m.block_count, Some(4));
    }

    #[test]
    fn bad_magic_and_versions_are_refused() {
        assert!(matches!(parse(b"GGML\x03\0\0\0"), Err(GgufError::BadMagic)));
        assert!(matches!(parse(b""), Err(GgufError::BadMagic)));
        let mut v = sample();
        v[4..8].copy_from_slice(&9u32.to_le_bytes());
        assert!(matches!(parse(&v), Err(GgufError::BadVersion(9))));
        assert!(parse(&build(2, &[Kv::Str("general.architecture", "llama")])).is_ok());
    }

    #[test]
    fn hostile_lengths_do_not_allocate_or_read_past_the_end() {
        // A string claiming 4 GB.
        let mut b = Vec::new();
        b.extend(MAGIC.to_le_bytes());
        b.extend(3u32.to_le_bytes());
        b.extend(0u64.to_le_bytes());
        b.extend(1u64.to_le_bytes());
        b.extend(u64::from(u32::MAX).to_le_bytes());
        assert!(matches!(parse(&b), Err(GgufError::Corrupt(_))));
        // More key/value pairs than the cap.
        let mut b = Vec::new();
        b.extend(MAGIC.to_le_bytes());
        b.extend(3u32.to_le_bytes());
        b.extend(0u64.to_le_bytes());
        b.extend((MAX_KV + 1).to_le_bytes());
        assert!(matches!(parse(&b), Err(GgufError::Corrupt(_))));
        // An array with 11 million items.
        let mut b = build(3, &[]);
        b[16..24].copy_from_slice(&1u64.to_le_bytes());
        b.extend(1u64.to_le_bytes());
        b.push(b'k');
        b.extend(9u32.to_le_bytes());
        b.extend(4u32.to_le_bytes());
        b.extend((MAX_ARRAY_ITEMS + 1).to_le_bytes());
        assert!(matches!(parse(&b), Err(GgufError::Corrupt(_))));
    }

    #[test]
    fn a_truncated_file_is_an_error_not_a_panic() {
        let full = sample();
        for cut in [10, 30, 100, full.len() - 5] {
            assert!(parse(&full[..cut]).is_err(), "cut at {cut}");
        }
    }

    #[test]
    fn quant_names() {
        assert_eq!(quant_name(7), Some("Q8_0"));
        assert_eq!(quant_name(999), None);
    }

    /// Needs the real model: `cargo test -p nexus-models -- --ignored`.
    #[test]
    #[ignore = "needs vendor/models"]
    fn parses_the_real_default_model() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/models");
        let Some(f) = std::fs::read_dir(&dir).ok().and_then(|d| {
            d.flatten()
                .flat_map(|e| std::fs::read_dir(e.path()).into_iter().flatten().flatten())
                .find(|e| e.path().extension().is_some_and(|x| x == "gguf"))
        }) else {
            panic!("no .gguf under {}", dir.display());
        };
        let m = parse_file(&f.path()).unwrap();
        assert!(m.architecture.is_some() && m.block_count.is_some(), "{m:?}");
    }
}
