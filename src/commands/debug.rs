pub(super) fn dump_bytes(
    label: &str,
    bytes: &[u8],
    show_bytes: bool,
    dump_hex: bool,
    dump_bin: bool,
    dump_json: bool,
) {
    const LIMIT: usize = 256;
    let n = bytes.len().min(LIMIT);
    let head = &bytes[..n];

    if show_bytes {
        println!("== BYTES ({}, {} bytes, head {} bytes) ==", label, bytes.len(), n);
        for (i, b) in head.iter().enumerate() {
            println!("{:04X}: {}", i, b);
        }
    }

    if dump_hex {
        println!("== HEX ({}, head {} bytes) ==", label, n);
        for (i, chunk) in head.chunks(16).enumerate() {
            print!("{:04X}: ", i * 16);
            for b in chunk {
                print!("{:02X} ", b);
            }
            println!();
        }
    }

    if dump_bin {
        println!("== BIN ({}, head {} bytes) ==", label, n);
        for (i, b) in head.iter().enumerate() {
            println!("{:04X}: {:08b}", i, b);
        }
    }

    if dump_json {
        println!("== JSON ({}) ==", label);
        println!("{{");
        println!("  \"len\": {},", bytes.len());
        print!("  \"head\": [");
        for (i, b) in head.iter().enumerate() {
            if i != 0 {
                print!(", ");
            }
            print!("{}", b);
        }
        println!("]");
        println!("}}");
    }
}
