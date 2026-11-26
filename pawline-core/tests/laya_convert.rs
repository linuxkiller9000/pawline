use pawline_core::laya_convert::*;

fn vf(num: u32, v: u64) -> Field {
    Field { num, wire: 0, val: Val::Varint(v) }
}
fn sf(num: u32, s: &str) -> Field {
    Field { num, wire: 2, val: Val::Len(s.as_bytes().to_vec()) }
}
fn lf(num: u32, b: Vec<u8>) -> Field {
    Field { num, wire: 2, val: Val::Len(b) }
}

fn tensor_raw(name: &str, dtype: u64, dims: &[u64], raw: Vec<u8>) -> Vec<u8> {
    let mut fields: Vec<Field> = dims.iter().map(|d| vf(1, *d)).collect();
    fields.push(vf(2, dtype));
    fields.push(sf(8, name));
    fields.push(lf(9, raw));
    let mut out = Vec::new();
    write_fields(&fields, &mut out);
    out
}

/// Tiny graph: MatMulNBits(x, B[2,64]u8, S[2,2]f32) -> y, then MatMul(y, C[64,2]) -> z
fn tiny_model() -> Vec<u8> {
    let b: Vec<u8> = (0..128u32).map(|i| ((i * 7 + 3) % 256) as u8).collect();
    let s: Vec<f32> = vec![0.5, -1.25, 2.0, 0.03125];
    let s_raw: Vec<u8> = s.iter().flat_map(|v| v.to_le_bytes()).collect();
    let c: Vec<f32> = (0..128u32).map(|i| (i as f32 - 64.0) / 128.0).collect();
    let c_raw: Vec<u8> = c.iter().flat_map(|v| v.to_le_bytes()).collect();

    // MatMulNBits node with a domain and an attribute (must be dropped)
    let mut attr = Vec::new();
    write_fields(&[sf(1, "bits"), vf(3, 8), vf(20, 2)], &mut attr); // name, i=8, type=INT
    let mut attr2 = Vec::new();
    write_fields(&[sf(1, "block_size"), vf(3, 32), vf(20, 2)], &mut attr2);
    let mut node1 = Vec::new();
    write_fields(
        &[
            sf(1, "x"),
            sf(1, "B"),
            sf(1, "S"),
            sf(2, "y"),
            sf(3, "mnb0"),
            sf(4, "MatMulNBits"),
            lf(5, attr),
            lf(5, attr2),
            sf(7, "com.microsoft"),
        ],
        &mut node1,
    );
    let mut node2 = Vec::new();
    write_fields(&[sf(1, "y"), sf(1, "C"), sf(2, "z"), sf(4, "MatMul")], &mut node2);

    let mut graph = Vec::new();
    write_fields(
        &[
            lf(1, node1),
            lf(1, node2),
            sf(2, "tiny"),
            lf(5, tensor_raw("B", 2, &[2, 64], b.clone())),
            lf(5, tensor_raw("S", 1, &[2, 2], s_raw.clone())),
            lf(5, tensor_raw("C", 1, &[64, 2], c_raw.clone())),
        ],
        &mut graph,
    );

    let mut model = Vec::new();
    write_fields(&[vf(1, 9), sf(2, "test-producer"), lf(7, graph)], &mut model);
    model
}

#[test]
fn converts_matmul_n_bits_and_preserves_everything_else() {
    let model = tiny_model();
    let mut data = Vec::new();
    let (out, stats) = convert_onnx_bytes(&model, &mut data, &mut |_| Ok(())).expect("convert");
    assert_eq!(stats.nodes_converted, 1);
    assert_eq!(stats.initializers_replaced, 2);
    assert_eq!(stats.external_bytes, 64 * 2 * 4);

    // top level preserved
    let mf = read_fields(&out).unwrap();
    assert_eq!(first_varint_pub(&mf, 1), Some(9));
    let graph = mf.iter().find(|f| f.num == 7).unwrap();
    let Val::Len(gb) = &graph.val else { panic!("graph") };
    let gf = read_fields(gb).unwrap();

    // nodes: rewritten MatMul first, verbatim second node after
    let nodes: Vec<&[u8]> = gf
        .iter()
        .filter(|f| f.num == 1)
        .map(|f| match &f.val {
            Val::Len(b) => b.as_slice(),
            _ => panic!(),
        })
        .collect();
    assert_eq!(nodes.len(), 2);
    let n0 = read_fields(nodes[0]).unwrap();
    assert_eq!(all_strings_pub(&n0, 4), vec!["MatMul"]);
    assert_eq!(all_strings_pub(&n0, 1), vec!["x", "B_fp32"]);
    assert_eq!(all_strings_pub(&n0, 2), vec!["y"]);
    assert_eq!(all_strings_pub(&n0, 3), vec!["mnb0"]);
    assert!(n0.iter().all(|f| f.num != 5), "attributes must be dropped");
    assert!(n0.iter().all(|f| f.num != 7), "domain must be dropped");
    let n1 = read_fields(nodes[1]).unwrap();
    assert_eq!(all_strings_pub(&n1, 4), vec!["MatMul"]);
    assert_eq!(all_strings_pub(&n1, 1), vec!["y", "C"]);

    // initializers: C kept verbatim (same bytes), B/S gone, mnb0_fp32 appended
    let inits: Vec<&[u8]> = gf
        .iter()
        .filter(|f| f.num == 5)
        .map(|f| match &f.val {
            Val::Len(b) => b.as_slice(),
            _ => panic!(),
        })
        .collect();
    assert_eq!(inits.len(), 2);
    let c = read_fields(inits[0]).unwrap();
    assert_eq!(all_strings_pub(&c, 8), vec!["C"]);
    assert!(c.iter().any(|f| f.num == 9), "kept initializer stays raw_data");
    let w = read_fields(inits[1]).unwrap();
    assert_eq!(all_strings_pub(&w, 8), vec!["B_fp32"]);
    assert_eq!(first_varint_pub(&w, 2), Some(1)); // FLOAT
    assert_eq!(first_varint_pub(&w, 14), Some(1)); // external
    let dims: Vec<u64> = w
        .iter()
        .filter(|f| f.num == 1)
        .map(|f| match f.val {
            Val::Varint(v) => v,
            _ => panic!(),
        })
        .collect();
    assert_eq!(dims, vec![64, 2]);
    let ext: Vec<String> = w
        .iter()
        .filter(|f| f.num == 13)
        .map(|f| match &f.val {
            Val::Len(b) => String::from_utf8(b.clone()).unwrap(),
            _ => panic!(),
        })
        .collect();
    assert_eq!(ext.len(), 3);
    assert!(ext[0].contains("location"));
    assert!(ext[0].contains(EXTERNAL_DATA_NAME));

    // external payload: W^T[k, n] = (B[n,k]-128) * S[n, k/32]
    assert_eq!(data.len(), 64 * 2 * 4);
    let b: Vec<u8> = (0..128u32).map(|i| ((i * 7 + 3) % 256) as u8).collect();
    let s: [f32; 4] = [0.5, -1.25, 2.0, 0.03125];
    for k in 0..64usize {
        for n in 0..2usize {
            let got = f32::from_le_bytes(data[(k * 2 + n) * 4..][..4].try_into().unwrap());
            let want = (b[n * 64 + k] as f32 - 128.0) * s[n * 2 + k / 32];
            assert_eq!(got.to_bits(), want.to_bits(), "k={k} n={n}");
        }
    }
}

#[test]
fn rejects_matmul_n_bits_with_extra_inputs() {
    let mut node1 = Vec::new();
    write_fields(
        &[sf(1, "x"), sf(1, "B"), sf(1, "S"), sf(1, "extra"), sf(2, "y"), sf(4, "MatMulNBits")],
        &mut node1,
    );
    let mut graph = Vec::new();
    write_fields(&[lf(1, node1), sf(2, "tiny")], &mut graph);
    let mut model = Vec::new();
    write_fields(&[vf(1, 9), lf(7, graph)], &mut model);
    let mut data = Vec::new();
    let err = convert_onnx_bytes(&model, &mut data, &mut |_| Ok(())).unwrap_err();
    assert!(err.contains("expected 3"), "{err}");
}

#[test]
fn wire_roundtrip_preserves_bytes() {
    let fields = vec![
        vf(1, 300),
        sf(4, "hello"),
        Field { num: 5, wire: 5, val: Val::Fixed32(0xdeadbeef) },
        Field { num: 6, wire: 1, val: Val::Fixed64(0x0123456789abcdef) },
        lf(15, vec![1, 2, 3, 4, 5]),
    ];
    let mut buf = Vec::new();
    write_fields(&fields, &mut buf);
    assert_eq!(read_fields(&buf).unwrap(), fields);
}

// tiny public wrappers so the test can use the private helpers
fn first_varint_pub(fields: &[Field], num: u32) -> Option<u64> {
    fields.iter().find(|f| f.num == num).and_then(|f| match f.val {
        Val::Varint(v) => Some(v),
        _ => None,
    })
}
fn all_strings_pub<'a>(fields: &'a [Field], num: u32) -> Vec<&'a str> {
    fields
        .iter()
        .filter(|f| f.num == num)
        .filter_map(|f| match &f.val {
            Val::Len(b) => std::str::from_utf8(b).ok(),
            _ => None,
        })
        .collect()
}

#[test]
fn rejects_wrong_quantization_parameters() {
    let b: Vec<u8> = vec![128u8; 64];
    let s_raw: Vec<u8> = [1.0f32].iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut attr = Vec::new();
    write_fields(&[sf(1, "bits"), vf(3, 4), vf(20, 2)], &mut attr);
    let mut node1 = Vec::new();
    write_fields(
        &[sf(1, "x"), sf(1, "B"), sf(1, "S"), sf(2, "y"), sf(4, "MatMulNBits"), lf(5, attr)],
        &mut node1,
    );
    let mut graph = Vec::new();
    write_fields(
        &[
            lf(1, node1),
            lf(5, tensor_raw("B", 2, &[1, 64], b)),
            lf(5, tensor_raw("S", 1, &[1, 2], s_raw)),
        ],
        &mut graph,
    );
    let mut model = Vec::new();
    write_fields(&[vf(1, 9), lf(7, graph)], &mut model);
    let mut data = Vec::new();
    let err = convert_onnx_bytes(&model, &mut data, &mut |_| Ok(())).unwrap_err();
    assert!(err.contains("bits"), "{err}");
}

#[test]
fn accepts_three_dim_weight_layout_like_the_real_model() {
    // real Laya layout: B [N, K/32, 32] uint8, S [N, K/32] f32, K/N attributes
    let b: Vec<u8> = (0..(2 * 2 * 32) as u32).map(|i| ((i * 5 + 1) % 256) as u8).collect();
    let s_raw: Vec<u8> = [1.5f32, 0.25, 2.0, 4.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut a1 = Vec::new();
    write_fields(&[sf(1, "bits"), vf(3, 8), vf(20, 2)], &mut a1);
    let mut a2 = Vec::new();
    write_fields(&[sf(1, "block_size"), vf(3, 32), vf(20, 2)], &mut a2);
    let mut a3 = Vec::new();
    write_fields(&[sf(1, "K"), vf(3, 64), vf(20, 2)], &mut a3);
    let mut a4 = Vec::new();
    write_fields(&[sf(1, "N"), vf(3, 2), vf(20, 2)], &mut a4);
    let mut node1 = Vec::new();
    write_fields(
        &[sf(1, "x"), sf(1, "B"), sf(1, "S"), sf(2, "y"), sf(4, "MatMulNBits"),
          lf(5, a1), lf(5, a2), lf(5, a3), lf(5, a4)],
        &mut node1,
    );
    let mut graph = Vec::new();
    write_fields(
        &[lf(1, node1), lf(5, tensor_raw("B", 2, &[2, 2, 32], b.clone())), lf(5, tensor_raw("S", 1, &[2, 2], s_raw))],
        &mut graph,
    );
    let mut model = Vec::new();
    write_fields(&[vf(1, 9), lf(7, graph)], &mut model);
    let mut data = Vec::new();
    let (out, stats) = convert_onnx_bytes(&model, &mut data, &mut |_| Ok(())).expect("convert 3-D");
    assert_eq!(stats.nodes_converted, 1);
    assert_eq!(data.len(), 64 * 2 * 4);
    // W^T[k, n] = (B[n][k]-128)*S[n][k/32], B flat is [n][block][32] row-major = [n][k]
    let s: [f32; 4] = [1.5, 0.25, 2.0, 4.0];
    for k in 0..64usize {
        for n in 0..2usize {
            let got = f32::from_le_bytes(data[(k * 2 + n) * 4..][..4].try_into().unwrap());
            let want = (b[n * 64 + k] as f32 - 128.0) * s[n * 2 + k / 32];
            assert_eq!(got.to_bits(), want.to_bits(), "k={k} n={n}");
        }
    }
    let mf = read_fields(&out).unwrap();
    let Val::Len(gb) = &mf.iter().find(|f| f.num == 7).unwrap().val else { panic!() };
    let gf = read_fields(gb).unwrap();
    let nodes: Vec<&[u8]> = gf.iter().filter(|f| f.num == 1).filter_map(|f| match &f.val { Val::Len(b) => Some(b.as_slice()), _ => None }).collect();
    let n0 = read_fields(nodes[0]).unwrap();
    assert_eq!(all_strings_pub(&n0, 4), vec!["MatMul"]);
    assert_eq!(all_strings_pub(&n0, 1), vec!["x", "B_fp32"]);
}
