//! Small preview of a picture Pawline itself saved. PNG, JPEG and still WebP.
//! The decoders are memory-safe Rust. Size is checked from the header before any pixels are decoded.
use std::path::Path;
use tiny_skia::Pixmap;

const MAX_FILE: u64 = 8 * 1024 * 1024;
const MAX_SIDE: u32 = 4096;
const MAX_PIXELS: u64 = 16_000_000;

/// Decode `path` and shrink it to fit inside `box_w` x `box_h` (never enlarged). None for anything unusable.
pub fn load(path: &Path, box_w: u32, box_h: u32) -> Option<Pixmap> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let (w, h, rgba) = decode(&bytes)?;
    shrink(w, h, &rgba, box_w, box_h)
}

/// Same as `load`, for bytes already read through a confined handle. None for anything unusable.
pub fn from_bytes(bytes: &[u8], box_w: u32, box_h: u32) -> Option<Pixmap> {
    if bytes.len() as u64 > MAX_FILE {
        return None;
    }
    let (w, h, rgba) = decode(bytes)?;
    shrink(w, h, &rgba, box_w, box_h)
}

fn decode(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
        dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut rd = dec.read_info().ok()?;
        let info = rd.info();
        let (w, h) = (info.width, info.height);
        if !dims_ok(w, h) {
            return None;
        }
        let mut buf = vec![0u8; rd.output_buffer_size()];
        let fr = rd.next_frame(&mut buf).ok()?;
        let px = &buf[..fr.buffer_size()];
        let mut out = Vec::with_capacity(w as usize * h as usize * 4);
        match fr.color_type {
            png::ColorType::Rgba => out.extend_from_slice(px),
            png::ColorType::Rgb => px.chunks_exact(3).for_each(|c| out.extend_from_slice(&[c[0], c[1], c[2], 255])),
            png::ColorType::Grayscale => px.iter().for_each(|g| out.extend_from_slice(&[*g, *g, *g, 255])),
            png::ColorType::GrayscaleAlpha => px.chunks_exact(2).for_each(|c| out.extend_from_slice(&[c[0], c[0], c[0], c[1]])),
            _ => return None,
        }
        (out.len() == w as usize * h as usize * 4).then_some((w, h, out))
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        let mut dec = jpeg_decoder::Decoder::new(std::io::Cursor::new(bytes));
        dec.read_info().ok()?;
        let info = dec.info()?;
        let (w, h) = (info.width as u32, info.height as u32);
        if !dims_ok(w, h) {
            return None;
        }
        let px = dec.decode().ok()?;
        let mut out = Vec::with_capacity(w as usize * h as usize * 4);
        match info.pixel_format {
            jpeg_decoder::PixelFormat::RGB24 => px.chunks_exact(3).for_each(|c| out.extend_from_slice(&[c[0], c[1], c[2], 255])),
            jpeg_decoder::PixelFormat::L8 => px.iter().for_each(|g| out.extend_from_slice(&[*g, *g, *g, 255])),
            _ => return None,
        }
        (out.len() == w as usize * h as usize * 4).then_some((w, h, out))
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        let mut dec = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes)).ok()?;
        let (w, h) = dec.dimensions();
        if !dims_ok(w, h) || dec.is_animated() {
            return None;
        }
        let mut buf = vec![0u8; dec.output_buffer_size()?];
        dec.read_image(&mut buf).ok()?;
        let mut out = Vec::with_capacity(w as usize * h as usize * 4);
        if dec.has_alpha() {
            out = buf;
        } else {
            buf.chunks_exact(3).for_each(|c| out.extend_from_slice(&[c[0], c[1], c[2], 255]));
        }
        (out.len() == w as usize * h as usize * 4).then_some((w, h, out))
    } else {
        None
    }
}

fn dims_ok(w: u32, h: u32) -> bool {
    w > 0 && h > 0 && w <= MAX_SIDE && h <= MAX_SIDE && (w as u64) * (h as u64) <= MAX_PIXELS
}

/// Box-filter shrink to fit, then premultiply for tiny-skia.
fn shrink(w: u32, h: u32, rgba: &[u8], bw: u32, bh: u32) -> Option<Pixmap> {
    let scale = (bw as f32 / w as f32).min(bh as f32 / h as f32).min(1.0);
    let (tw, th) = (((w as f32 * scale).round() as u32).max(1), ((h as f32 * scale).round() as u32).max(1));
    let mut pm = Pixmap::new(tw, th)?;
    let data = pm.data_mut();
    for ty in 0..th {
        let (y0, y1) = (ty * h / th, (((ty + 1) * h).div_ceil(th)).min(h).max(ty * h / th + 1));
        for tx in 0..tw {
            let (x0, x1) = (tx * w / tw, (((tx + 1) * w).div_ceil(tw)).min(w).max(tx * w / tw + 1));
            let (mut r, mut g, mut b, mut a, mut n) = (0u32, 0u32, 0u32, 0u32, 0u32);
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = ((y * w + x) * 4) as usize;
                    r += rgba[i] as u32;
                    g += rgba[i + 1] as u32;
                    b += rgba[i + 2] as u32;
                    a += rgba[i + 3] as u32;
                    n += 1;
                }
            }
            let (a8, o) = ((a / n) as u32, ((ty * tw + tx) * 4) as usize);
            data[o] = ((r / n) * a8 / 255) as u8;
            data[o + 1] = ((g / n) * a8 / 255) as u8;
            data[o + 2] = ((b / n) * a8 / 255) as u8;
            data[o + 3] = a8 as u8;
        }
    }
    Some(pm)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes(w: u32, h: u32, c: [u8; 3]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().unwrap();
        let px: Vec<u8> = (0..w * h).flat_map(|_| c).collect();
        wr.write_image_data(&px).unwrap();
        drop(wr);
        out
    }

    #[test]
    fn png_is_decoded_and_shrunk_but_never_enlarged() {
        let (w, h, d) = decode(&png_bytes(40, 20, [10, 200, 30])).unwrap();
        assert_eq!((w, h), (40, 20));
        let pm = shrink(w, h, &d, 20, 20).unwrap();
        assert_eq!((pm.width(), pm.height()), (20, 10));
        assert_eq!(&pm.data()[..4], &[10, 200, 30, 255]);
        let small = shrink(w, h, &d, 400, 400).unwrap();
        assert_eq!((small.width(), small.height()), (40, 20));
    }

    #[test]
    fn jpeg_is_decoded() {
        let (w, h, d) = decode(include_bytes!("../testdata/t.jpg")).unwrap();
        assert_eq!((w, h), (16, 8));
        let mid = ((4 * 16 + 8) * 4) as usize;
        assert!(d[mid] > 150 && d[mid + 1] < 80 && d[mid + 2] < 80, "mostly red: {:?}", &d[mid..mid + 3]);
    }

    #[test]
    fn webp_is_decoded() {
        // 1x1 lossless WebP
        let b: &[u8] = &[0x52,0x49,0x46,0x46,0x1a,0,0,0,0x57,0x45,0x42,0x50,0x56,0x50,0x38,0x4c,0x0d,0,0,0,0x2f,0,0,0,0x10,0x07,0x10,0x11,0x11,0x88,0x88,0xfe,7,0];
        let (w, h, d) = decode(b).expect("webp");
        assert_eq!((w, h, d.len()), (1, 1, 4));
    }

    #[test]
    fn junk_oversized_and_webp_are_refused() {
        assert!(decode(b"not an image").is_none());
        assert!(decode(b"RIFF\0\0\0\0WEBPxx").is_none(), "truncated webp");
        assert!(!dims_ok(5000, 10));
        assert!(!dims_ok(4096, 4096) || 4096u64 * 4096 <= MAX_PIXELS);
        assert!(decode(&png_bytes(8, 8, [1, 2, 3])[..20]).is_none(), "truncated png");
    }
}

#[cfg(test)]
mod from_bytes_tests {
    use super::*;

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut e = png::Encoder::new(&mut out, w, h);
            e.set_color(png::ColorType::Rgba);
            e.set_depth(png::BitDepth::Eight);
            let mut wr = e.write_header().unwrap();
            wr.write_image_data(&vec![200u8; (w * h * 4) as usize]).unwrap();
        }
        out
    }

    #[test]
    fn bytes_are_shrunk_never_enlarged_and_junk_is_refused() {
        let big = from_bytes(&png_bytes(800, 300), 220, 150).unwrap();
        assert_eq!((big.width(), big.height()), (220, 83));
        let small = from_bytes(&png_bytes(20, 10), 220, 150).unwrap();
        assert_eq!((small.width(), small.height()), (20, 10));
        assert!(from_bytes(b"not a picture", 220, 150).is_none());
        assert!(from_bytes(&[], 220, 150).is_none());
    }
}
