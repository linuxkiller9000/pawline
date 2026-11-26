//! In-Rust conversion of the Laya int8-quantized ONNX model (MatMulNBits) to an
//! all-fp32 model with external tensor data. This replaces the python converter
//! so the whole one-time setup happens inside the app with no python/pip.
//!
//! Dequantization (mirrors tools/laya-setup.sh, verified bit-identical on the
//! real model): for each MatMulNBits node with weight B[N, K] (uint8, block 32)
//! and scales S[N, K/32] (fp32): W[n, k] = (B[n, k] - 128) * S[n, k / 32], and
//! the node becomes a plain MatMul over W^T ([K, N] row-major fp32).
//!
//! The ONNX file is rewritten with a minimal protobuf wire reader/writer: every
//! field that is not touched is re-emitted byte-for-byte, so unknown fields and
//! everything else about the model is preserved exactly.

// ---------- protobuf wire primitives ----------

#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Varint(u64),
    Fixed64(u64),
    Len(Vec<u8>),
    Fixed32(u32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub num: u32,
    pub wire: u8,
    pub val: Val,
}

pub fn read_varint(buf: &[u8], pos: &mut usize) -> Result<u64, String> {
    let mut v: u64 = 0;
    for i in 0..10 {
        let b = *buf.get(*pos).ok_or("protobuf: varint runs past end of buffer")?;
        *pos += 1;
        if i == 9 && b > 1 {
            return Err("protobuf: varint longer than 64 bits".into());
        }
        v |= ((b & 0x7f) as u64) << (7 * i);
        if b & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err("protobuf: unterminated varint".into())
}

pub fn write_varint(mut v: u64, out: &mut Vec<u8>) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

pub fn read_fields(buf: &[u8]) -> Result<Vec<Field>, String> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < buf.len() {
        let key = read_varint(buf, &mut pos)?;
        let num = (key >> 3) as u32;
        let wire = (key & 7) as u8;
        if num == 0 {
            return Err("protobuf: field number 0".into());
        }
        let val = match wire {
            0 => Val::Varint(read_varint(buf, &mut pos)?),
            1 => {
                let b = buf
                    .get(pos..pos + 8)
                    .ok_or("protobuf: fixed64 runs past end of buffer")?;
                pos += 8;
                Val::Fixed64(u64::from_le_bytes(b.try_into().unwrap()))
            }
            2 => {
                let len = read_varint(buf, &mut pos)? as usize;
                let b = buf
                    .get(pos..pos + len)
                    .ok_or("protobuf: length field runs past end of buffer")?
                    .to_vec();
                pos += len;
                Val::Len(b)
            }
            5 => {
                let b = buf
                    .get(pos..pos + 4)
                    .ok_or("protobuf: fixed32 runs past end of buffer")?;
                pos += 4;
                Val::Fixed32(u32::from_le_bytes(b.try_into().unwrap()))
            }
            w => return Err(format!("protobuf: unsupported wire type {w}")),
        };
        out.push(Field { num, wire, val });
    }
    Ok(out)
}

pub fn write_fields(fields: &[Field], out: &mut Vec<u8>) {
    for f in fields {
        write_varint(((f.num as u64) << 3) | f.wire as u64, out);
        match &f.val {
            Val::Varint(v) => write_varint(*v, out),
            Val::Fixed64(v) => out.extend_from_slice(&v.to_le_bytes()),
            Val::Fixed32(v) => out.extend_from_slice(&v.to_le_bytes()),
            Val::Len(b) => {
                write_varint(b.len() as u64, out);
                out.extend_from_slice(b);
            }
        }
    }
}

// ---------- zero-copy span reader (used for the big model pass) ----------

#[derive(Debug)]
pub enum ValRef<'a> {
    Varint(u64),
    Fixed64(u64),
    Len(&'a [u8]),
    Fixed32(u32),
}

#[derive(Debug)]
pub struct FieldSpan<'a> {
    pub num: u32,
    pub wire: u8,
    pub val: ValRef<'a>,
    /// the complete encoded key+value bytes, re-emitted verbatim for untouched fields
    pub raw: &'a [u8],
}

pub fn read_spans(buf: &[u8]) -> Result<Vec<FieldSpan<'_>>, String> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < buf.len() {
        let start = pos;
        let key = read_varint(buf, &mut pos)?;
        let num = (key >> 3) as u32;
        let wire = (key & 7) as u8;
        if num == 0 {
            return Err("protobuf: field number 0".into());
        }
        let val = match wire {
            0 => ValRef::Varint(read_varint(buf, &mut pos)?),
            1 => {
                let b = buf.get(pos..pos + 8).ok_or("protobuf: fixed64 eof")?;
                pos += 8;
                ValRef::Fixed64(u64::from_le_bytes(b.try_into().unwrap()))
            }
            2 => {
                let len = read_varint(buf, &mut pos)? as usize;
                let b = buf.get(pos..pos + len).ok_or("protobuf: len eof")?;
                pos += len;
                ValRef::Len(b)
            }
            5 => {
                let b = buf.get(pos..pos + 4).ok_or("protobuf: fixed32 eof")?;
                pos += 4;
                ValRef::Fixed32(u32::from_le_bytes(b.try_into().unwrap()))
            }
            w => return Err(format!("protobuf: unsupported wire type {w}")),
        };
        out.push(FieldSpan { num, wire, val, raw: &buf[start..pos] });
    }
    Ok(out)
}

fn span_len<'a>(s: &FieldSpan<'a>, num: u32) -> Option<&'a [u8]> {
    if s.num != num {
        return None;
    }
    match s.val {
        ValRef::Len(b) => Some(b),
        _ => None,
    }
}

fn span_varint(s: &FieldSpan, num: u32) -> Option<u64> {
    if s.num != num {
        return None;
    }
    match s.val {
        ValRef::Varint(v) => Some(v),
        _ => None,
    }
}

fn span_string<'a>(s: &FieldSpan<'a>, num: u32) -> Option<&'a str> {
    span_len(s, num).and_then(|b| std::str::from_utf8(b).ok())
}

fn varint_field(num: u32, v: u64) -> Field {
    Field { num, wire: 0, val: Val::Varint(v) }
}

fn len_field(num: u32, b: Vec<u8>) -> Field {
    Field { num, wire: 2, val: Val::Len(b) }
}

fn string_field(num: u32, s: &str) -> Field {
    len_field(num, s.as_bytes().to_vec())
}




// ---------- ONNX MatMulNBits -> MatMul conversion ----------

use std::io::Write;

pub const EXTERNAL_DATA_NAME: &str = "laya_fp32.onnx.data";

#[derive(Debug, Clone)]
pub struct ConvertStats {
    pub nodes_converted: usize,
    pub initializers_replaced: usize,
    pub external_bytes: usize,
}

/// Header-only view of a TensorProto: no payload is copied.
struct TensorHeader<'a> {
    message: &'a [u8],
    name: &'a str,
    data_type: u64,
    dims: Vec<u64>,
}

fn parse_tensor_header(message: &[u8]) -> Result<TensorHeader<'_>, String> {
    let fields = read_spans(message)?;
    let mut name = "";
    let mut data_type = 0u64;
    let mut dims = Vec::new();
    for f in &fields {
        match f.num {
            1 => match f.val {
                ValRef::Varint(v) => dims.push(v),
                ValRef::Len(b) => {
                    let mut pos = 0usize;
                    while pos < b.len() {
                        dims.push(read_varint(b, &mut pos)?);
                    }
                }
                _ => {}
            },
            2 => {
                if let ValRef::Varint(v) = f.val {
                    data_type = v;
                }
            }
            8 => {
                if let ValRef::Len(b) = f.val {
                    name = std::str::from_utf8(b).map_err(|_| "tensor name not utf-8")?;
                }
            }
            _ => {}
        }
    }
    Ok(TensorHeader { message, name, data_type, dims })
}

/// Borrow the uint8 payload of a tensor (raw_data, or packed int32_data copied out).
fn tensor_u8<'a>(t: &TensorHeader<'a>, scratch: &'a mut Vec<u8>) -> Result<&'a [u8], String> {
    let fields = read_spans(t.message)?;
    for f in &fields {
        if f.num == 9 {
            if let ValRef::Len(b) = f.val {
                return Ok(b);
            }
        }
    }
    for f in &fields {
        if f.num == 5 {
            match f.val {
                ValRef::Len(b) => {
                    let mut pos = 0usize;
                    while pos < b.len() {
                        scratch.push(read_varint(b, &mut pos)? as u8);
                    }
                    return Ok(scratch);
                }
                ValRef::Varint(v) => {
                    scratch.push(v as u8);
                    return Ok(scratch);
                }
                _ => {}
            }
        }
    }
    Err(format!("tensor {}: no uint8 payload", t.name))
}

/// Stream the fp32 payload of a tensor: calls `emit(index, value)` per element.
fn tensor_f32_each(t: &TensorHeader, mut emit: impl FnMut(usize, f32)) -> Result<usize, String> {
    let fields = read_spans(t.message)?;
    let mut count = 0usize;
    for f in &fields {
        if f.num == 9 {
            if let ValRef::Len(b) = f.val {
                if b.len() % 4 != 0 {
                    return Err(format!("tensor {}: raw float payload misaligned", t.name));
                }
                for c in b.chunks_exact(4) {
                    emit(count, f32::from_le_bytes(c.try_into().unwrap()));
                    count += 1;
                }
                return Ok(count);
            }
        }
    }
    for f in &fields {
        if f.num == 4 {
            match f.val {
                ValRef::Fixed32(v) => {
                    emit(count, f32::from_bits(v));
                    count += 1;
                }
                ValRef::Len(b) => {
                    if b.len() % 4 != 0 {
                        return Err(format!("tensor {}: float_data misaligned", t.name));
                    }
                    for c in b.chunks_exact(4) {
                        emit(count, f32::from_le_bytes(c.try_into().unwrap()));
                        count += 1;
                    }
                }
                _ => {}
            }
        }
    }
    if count == 0 {
        return Err(format!("tensor {}: no float payload", t.name));
    }
    Ok(count)
}

fn external_data_tensor(name: &str, dims: &[u64], offset: u64, length: u64) -> Result<Vec<u8>, String> {
    let entry = |k: &str, v: &str| -> Result<Vec<u8>, String> {
        let mut e = Vec::new();
        write_fields(&[string_field(1, k), string_field(2, v)], &mut e);
        Ok(e)
    };
    let mut fields = Vec::new();
    for d in dims {
        fields.push(varint_field(1, *d));
    }
    fields.push(varint_field(2, 1)); // FLOAT
    fields.push(string_field(8, name));
    fields.push(len_field(13, entry("location", EXTERNAL_DATA_NAME)?));
    fields.push(len_field(13, entry("offset", &offset.to_string())?));
    fields.push(len_field(13, entry("length", &length.to_string())?));
    fields.push(varint_field(14, 1)); // data_location = EXTERNAL
    let mut out = Vec::new();
    write_fields(&fields, &mut out);
    Ok(out)
}

fn convert_graph(graph: &[u8], data_out: &mut impl Write, data_len: &mut u64) -> Result<(Vec<u8>, ConvertStats), String> {
    let fields = read_spans(graph)?;
    // header-only pass over initializers (no payload copies)
    let mut headers: Vec<TensorHeader> = Vec::new();
    for f in &fields {
        if let Some(b) = span_len(f, 5) {
            headers.push(parse_tensor_header(b)?);
        }
    }

    let mut out: Vec<u8> = Vec::with_capacity(graph.len());
    let mut stats = ConvertStats { nodes_converted: 0, initializers_replaced: 0, external_bytes: 0 };
    let mut replaced: Vec<&str> = Vec::new();
    let mut new_initializers: Vec<Vec<u8>> = Vec::new();

    for f in &fields {
        if f.num == 1 {
            let node_bytes = span_len(f, 1).ok_or("graph: node field not length-delimited")?;
            let nf = read_spans(node_bytes)?;
            let op_type = nf.iter().find_map(|x| span_string(x, 4)).unwrap_or("");
            if op_type != "MatMulNBits" {
                out.extend_from_slice(f.raw);
                continue;
            }
            let inputs: Vec<&str> = nf.iter().filter_map(|x| span_string(x, 1)).collect();
            if inputs.len() != 3 {
                return Err(format!(
                    "MatMulNBits with {} inputs (expected 3: input, weight, scales; no bias/zero-points/g_idx)",
                    inputs.len()
                ));
            }
            // quantization parameters come from the node attributes (bits=8, block_size=32)
            let mut attr_k: Option<u64> = None;
            let mut attr_n: Option<u64> = None;
            let mut saw_bits = false;
            let mut saw_block = false;
            for af in nf.iter().filter(|x| x.num == 5) {
                let ValRef::Len(ab) = af.val else { continue };
                let attr = read_spans(ab)?;
                let aname = attr.iter().find_map(|x| span_string(x, 1)).unwrap_or("");
                let ival = attr.iter().find_map(|x| span_varint(x, 3));
                match (aname, ival) {
                    ("bits", Some(8)) => saw_bits = true,
                    ("bits", v) => return Err(format!("unsupported quantization bits={v:?}")),
                    ("block_size", Some(32)) => saw_block = true,
                    ("block_size", v) => return Err(format!("unsupported block_size={v:?}")),
                    ("K", v) => attr_k = v,
                    ("N", v) => attr_n = v,
                    _ => {}
                }
            }
            if !saw_bits || !saw_block {
                return Err("MatMulNBits missing bits=8 / block_size=32 attributes".into());
            }
            let node_name = nf.iter().find_map(|x| span_string(x, 3)).unwrap_or("");
            // same naming as the python converter: <weight tensor name>_fp32
            let wname = format!("{}_fp32", inputs[1]);

            let b_t = headers
                .iter()
                .find(|t| t.name == inputs[1])
                .ok_or_else(|| format!("weight tensor {} not an initializer", inputs[1]))?;
            let s_t = headers
                .iter()
                .find(|t| t.name == inputs[2])
                .ok_or_else(|| format!("scales tensor {} not an initializer", inputs[2]))?;
            if b_t.data_type != 2 {
                return Err(format!("weight {}: expected UINT8 (2), got {}", b_t.name, b_t.data_type));
            }
            if s_t.data_type != 1 {
                return Err(format!("scales {}: expected FLOAT (1), got {}", s_t.name, s_t.data_type));
            }
            // weight layout is either [N, K] (2-D) or [N, K/32, 32] (3-D, the real Laya model)
            let (n, k) = match b_t.dims.as_slice() {
                [n, k] => (*n as usize, *k as usize),
                [n, blocks, 32] => (*n as usize, (blocks * 32) as usize),
                d => return Err(format!("weight {}: unsupported dims {d:?}", b_t.name)),
            };
            if s_t.dims.len() != 2 {
                return Err(format!("scales {}: must be 2-D, got {:?}", s_t.name, s_t.dims));
            }
            let blocks = s_t.dims[1] as usize;
            if blocks * 32 != k {
                return Err(format!(
                    "scales imply {}*32={} columns but weight has {k} (block size 32 required)",
                    s_t.dims[1],
                    blocks * 32
                ));
            }
            if s_t.dims[0] as usize != n {
                return Err("scales/weight row mismatch".into());
            }
            if let Some(ak) = attr_k {
                if ak as usize != k {
                    return Err(format!("attribute K={ak} but weight has {k} columns"));
                }
            }
            if let Some(an) = attr_n {
                if an as usize != n {
                    return Err(format!("attribute N={an} but weight has {n} rows"));
                }
            }

            // decode scales once (small: n * k/32 floats), stream weight bytes
            let mut s: Vec<f32> = Vec::with_capacity(n * blocks);
            let got = tensor_f32_each(s_t, |_, v| s.push(v))?;
            if got != n * blocks {
                return Err(format!("scales {}: {got} floats for {n}x{blocks}", s_t.name));
            }
            let mut scratch = Vec::new();
            let b = tensor_u8(b_t, &mut scratch)?;
            if b.len() != n * k {
                return Err(format!("weight {}: {} bytes for {n}x{k}", b_t.name, b.len()));
            }

            // W^T in row-major [K, N], fp32 little-endian, streamed to the data file
            let offset = *data_len;
            let mut buf = Vec::with_capacity(1 << 20);
            for kk in 0..k {
                for nn in 0..n {
                    let w = (b[nn * k + kk] as f32 - 128.0) * s[nn * blocks + kk / 32];
                    buf.extend_from_slice(&w.to_le_bytes());
                    if buf.len() == 1 << 20 {
                        data_out.write_all(&buf).map_err(|e| format!("write external data: {e}"))?;
                        buf.clear();
                    }
                }
            }
            if !buf.is_empty() {
                data_out.write_all(&buf).map_err(|e| format!("write external data: {e}"))?;
            }
            let length = (n * k * 4) as u64;
            *data_len += length;
            new_initializers.push(external_data_tensor(&wname, &[k as u64, n as u64], offset, length)?);
            replaced.push(b_t.name);
            replaced.push(s_t.name);
            stats.initializers_replaced += 2;
            stats.external_bytes += length as usize;
            stats.nodes_converted += 1;

            // rewritten node: inputs [x, wname], outputs/name verbatim, op_type MatMul
            let mut node_fields: Vec<Field> = vec![string_field(1, inputs[0]), string_field(1, &wname)];
            for of in nf.iter().filter(|x| x.num == 2) {
                node_fields.push(len_field(2, span_len(of, 2).unwrap().to_vec()));
            }
            if !node_name.is_empty() {
                node_fields.push(string_field(3, node_name));
            }
            node_fields.push(string_field(4, "MatMul"));
            let mut nb = Vec::new();
            write_fields(&node_fields, &mut nb);
            write_fields(&[len_field(1, nb)], &mut out);
        } else if f.num == 5 {
            // initializer: keep only tensors not consumed by a MatMulNBits rewrite
            let b = span_len(f, 5).unwrap();
            let h = headers.iter().find(|t| t.message == b).ok_or("graph: initializer mismatch")?;
            if !replaced.contains(&h.name) {
                out.extend_from_slice(f.raw);
            }
        } else {
            out.extend_from_slice(f.raw);
        }
    }
    for ni in new_initializers {
        write_fields(&[len_field(5, ni)], &mut out);
    }
    Ok((out, stats))
}

/// Convert a full ONNX ModelProto byte buffer. Streams the external fp32 tensor
/// payload to `data_out` and returns the rewritten model bytes.
pub fn convert_onnx_bytes(
    input: &[u8],
    data_out: &mut impl Write,
) -> Result<(Vec<u8>, ConvertStats), String> {
    let fields = read_spans(input)?;
    let mut saw_graph = false;
    let mut out: Vec<u8> = Vec::new();
    let mut stats = ConvertStats { nodes_converted: 0, initializers_replaced: 0, external_bytes: 0 };
    let mut data_len: u64 = 0;
    for f in &fields {
        if f.num == 7 && f.wire == 2 {
            let graph = span_len(f, 7).ok_or("onnx: graph not length-delimited")?;
            let (g, gs) = convert_graph(graph, data_out, &mut data_len)?;
            stats.nodes_converted += gs.nodes_converted;
            stats.initializers_replaced += gs.initializers_replaced;
            stats.external_bytes += gs.external_bytes;
            write_fields(&[len_field(7, g)], &mut out);
            saw_graph = true;
        } else {
            out.extend_from_slice(f.raw);
        }
    }
    if !saw_graph {
        return Err("onnx: no graph field in model".into());
    }
    if stats.nodes_converted == 0 {
        return Err("onnx: no MatMulNBits nodes found - already converted or wrong model".into());
    }
    Ok((out, stats))
}
