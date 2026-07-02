//! Compress / decompress — Gzip, Zlib, raw Deflate (via flate2).
use std::io::{Read, Write};

use flate2::Compression;

use super::basex::decoded;
use super::prelude::*;

fn io<E: std::fmt::Display>(e: E) -> CoreError {
    CoreError::Other(format!("压缩/解压失败: {e}"))
}

struct Comp;
impl Node for Comp {
    fn run(&self, inputs: &PortMap, params: &serde_json::Value, _c: &mut NodeCtx) -> Result<PortMap, CoreError> {
        let data = in_bytes(inputs, "data")?;
        let out = match pstr(params, "format", "Gzip") {
            "Zlib" => {
                let mut e = flate2::write::ZlibEncoder::new(Vec::new(), Compression::default());
                e.write_all(&data).map_err(io)?;
                e.finish().map_err(io)?
            }
            "Raw Deflate" => {
                let mut e = flate2::write::DeflateEncoder::new(Vec::new(), Compression::default());
                e.write_all(&data).map_err(io)?;
                e.finish().map_err(io)?
            }
            _ => {
                let mut e = flate2::write::GzEncoder::new(Vec::new(), Compression::default());
                e.write_all(&data).map_err(io)?;
                e.finish().map_err(io)?
            }
        };
        let mut m = PortMap::new();
        m.insert("hex".to_string(), PortValue::Text(hex::encode(&out)));
        m.insert("bytes".to_string(), PortValue::Bytes(Arc::from(out.into_boxed_slice())));
        Ok(m)
    }
}

struct Decomp;
impl Node for Decomp {
    fn run(&self, inputs: &PortMap, params: &serde_json::Value, _c: &mut NodeCtx) -> Result<PortMap, CoreError> {
        let data = in_bytes(inputs, "data")?;
        let fmt = match pstr(params, "format", "自动") {
            "自动" => {
                if data.starts_with(b"BZh") {
                    "Bzip2"
                } else if data.starts_with(&[0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00]) {
                    "XZ"
                } else if data.starts_with(&[0x5D, 0x00, 0x00]) {
                    "LZMA"
                } else if data.starts_with(&[0x1f, 0x8b]) {
                    "Gzip"
                } else if data.first() == Some(&0x78) {
                    "Zlib"
                } else {
                    "Raw Deflate"
                }
            }
            f => f,
        };
        let lzma_err = |e| CoreError::Other(format!("解压失败: {e:?}"));
        let mut out = Vec::new();
        match fmt {
            "Zlib" => flate2::read::ZlibDecoder::new(&data[..]).read_to_end(&mut out).map_err(io)?,
            "Raw Deflate" => flate2::read::DeflateDecoder::new(&data[..]).read_to_end(&mut out).map_err(io)?,
            "Bzip2" => bzip2_rs::DecoderReader::new(&data[..]).read_to_end(&mut out).map_err(io)?,
            "XZ" => {
                let mut r: &[u8] = &data;
                lzma_rs::xz_decompress(&mut r, &mut out).map_err(lzma_err)?;
                out.len()
            }
            "LZMA" => {
                let mut r: &[u8] = &data;
                lzma_rs::lzma_decompress(&mut r, &mut out).map_err(lzma_err)?;
                out.len()
            }
            _ => flate2::read::GzDecoder::new(&data[..]).read_to_end(&mut out).map_err(io)?,
        };
        Ok(decoded(out))
    }
}

pub fn register(reg: &mut NodeRegistry) {
    reg.register(
        desc(
            "compress",
            ARC,
            "压缩",
            AMBER,
            vec![req("data", "输入", PortType::Any)],
            vec![
                req("hex", "hex", PortType::Text),
                opt("bytes", "字节", PortType::Bytes),
            ],
            vec![ParamSpec::select("format", "格式", &["Gzip", "Zlib", "Raw Deflate"], "Gzip")],
        ),
        Arc::new(|| Arc::new(Comp)),
    );
    reg.register(
        desc(
            "decompress",
            ARC,
            "解压缩",
            AMBER,
            vec![req("data", "字节", PortType::Any)],
            vec![
                req("text", "文本", PortType::Text),
                opt("bytes", "字节", PortType::Bytes),
            ],
            vec![ParamSpec::select(
                "format",
                "格式",
                &["自动", "Gzip", "Zlib", "Raw Deflate", "Bzip2", "XZ", "LZMA"],
                "自动",
            )],
        ),
        Arc::new(|| Arc::new(Decomp)),
    );
}
