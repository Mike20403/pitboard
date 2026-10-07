//! Read a PE's import directory to list the DLLs it imports, so R1's `+crt-static` check can
//! see whether a build takes its C runtime from `vcruntime*.dll` or `api-ms-win-crt-*`.
//! Pure byte parsing, so it is tested on every system; every offset is checked, so a
//! malformed file yields nothing rather than a panic.

/// The DLL names a PE imports, in import-directory order. Empty if the bytes are not a PE
/// the reader understands.
pub fn import_dlls(bytes: &[u8]) -> Vec<String> {
    parse(bytes).unwrap_or_default()
}

/// Whether any of `dlls` is a C runtime DLL.
pub fn takes_c_runtime_from_dll(dlls: &[String]) -> bool {
    dlls.iter().any(|d| {
        let l = d.to_ascii_lowercase();
        l.starts_with("vcruntime") || l.starts_with("api-ms-win-crt") || l.starts_with("ucrtbase")
    })
}

fn u16le(b: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        b.get(off..off.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn u32le(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(off..off.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn parse(b: &[u8]) -> Option<Vec<String>> {
    if b.get(0..2)? != b"MZ" {
        return None;
    }
    let pe_off = u32le(b, 0x3C)? as usize;
    if b.get(pe_off..pe_off.checked_add(4)?)? != b"PE\0\0" {
        return None;
    }
    let coff = pe_off + 4;
    let num_sections = u16le(b, coff.checked_add(2)?)? as usize;
    let opt_size = u16le(b, coff.checked_add(16)?)? as usize;
    let opt = coff.checked_add(20)?;
    // 0x10b PE32, 0x20b PE32+; their data directories start at different offsets.
    let data_dirs = match u16le(b, opt)? {
        0x10b => opt.checked_add(96)?,
        0x20b => opt.checked_add(112)?,
        _ => return None,
    };
    // The import directory is data directory entry 1: an RVA, then a size.
    let import_rva = u32le(b, data_dirs.checked_add(8)?)?;
    if import_rva == 0 {
        return Some(Vec::new());
    }
    let secs = read_sections(b, opt.checked_add(opt_size)?, num_sections)?;
    let import_off = rva_to_off(import_rva, &secs)?;

    let mut out = Vec::new();
    for i in 0..4096usize {
        // IMAGE_IMPORT_DESCRIPTOR: 20 bytes; Name at 12, FirstThunk at 16.
        let entry = import_off.checked_add(i.checked_mul(20)?)?;
        let name_rva = u32le(b, entry.checked_add(12)?)?;
        let first_thunk = u32le(b, entry.checked_add(16)?)?;
        if name_rva == 0 && first_thunk == 0 {
            break;
        }
        if let Some(name_off) = rva_to_off(name_rva, &secs) {
            out.push(read_cstr(b, name_off));
        }
    }
    Some(out)
}

struct Section {
    virtual_address: u32,
    virtual_size: u32,
    raw_pointer: u32,
    raw_size: u32,
}

fn read_sections(b: &[u8], off: usize, n: usize) -> Option<Vec<Section>> {
    (0..n.min(96))
        .map(|i| {
            let s = off.checked_add(i.checked_mul(40)?)?;
            Some(Section {
                virtual_size: u32le(b, s.checked_add(8)?)?,
                virtual_address: u32le(b, s.checked_add(12)?)?,
                raw_size: u32le(b, s.checked_add(16)?)?,
                raw_pointer: u32le(b, s.checked_add(20)?)?,
            })
        })
        .collect()
}

fn rva_to_off(rva: u32, secs: &[Section]) -> Option<usize> {
    secs.iter().find_map(|s| {
        let size = s.virtual_size.max(s.raw_size);
        let end = s.virtual_address.checked_add(size)?;
        (rva >= s.virtual_address && rva < end)
            .then(|| s.raw_pointer.checked_add(rva - s.virtual_address))
            .flatten()
            .map(|o| o as usize)
    })
}

fn read_cstr(b: &[u8], off: usize) -> String {
    b.get(off..)
        .unwrap_or_default()
        .iter()
        .take(256)
        .take_while(|c| **c != 0)
        .map(|c| *c as char)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal PE32+ with one section holding an import directory of two DLLs.
    fn tiny_pe(dlls: &[&str]) -> Vec<u8> {
        let mut b = vec![0u8; 0x400];
        let put16 =
            |b: &mut Vec<u8>, at: usize, v: u16| b[at..at + 2].copy_from_slice(&v.to_le_bytes());
        let put32 =
            |b: &mut Vec<u8>, at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
        b[0..2].copy_from_slice(b"MZ");
        put32(&mut b, 0x3C, 0x40);
        b[0x40..0x44].copy_from_slice(b"PE\0\0");
        let coff = 0x44;
        put16(&mut b, coff, 0x8664);
        put16(&mut b, coff + 2, 1); // one section
        put16(&mut b, coff + 16, 240); // PE32+ optional header with 16 directories
        let opt = coff + 20;
        put16(&mut b, opt, 0x20b);
        put32(&mut b, opt + 112 + 8, 0x1000); // the import directory's RVA
        put32(&mut b, opt + 112 + 12, 60);
        let sec = opt + 240;
        put32(&mut b, sec + 8, 0x200); // virtual size
        put32(&mut b, sec + 12, 0x1000); // virtual address
        put32(&mut b, sec + 16, 0x200); // raw size
        put32(&mut b, sec + 20, 0x200); // raw pointer
        for (i, dll) in dlls.iter().enumerate() {
            let desc = 0x200 + i * 20;
            let name_rva = 0x1100 + (i as u32) * 0x20;
            put32(&mut b, desc + 12, name_rva);
            put32(&mut b, desc + 16, 0x1080);
            let at = 0x200 + (name_rva - 0x1000) as usize;
            b[at..at + dll.len()].copy_from_slice(dll.as_bytes());
        }
        b
    }

    #[test]
    fn a_pe32_plus_lists_its_imports_in_order() {
        let pe = tiny_pe(&["KERNEL32.dll", "VCRUNTIME140.dll"]);
        let dlls = import_dlls(&pe);
        assert_eq!(
            dlls,
            vec!["KERNEL32.dll".to_string(), "VCRUNTIME140.dll".into()]
        );
        assert!(takes_c_runtime_from_dll(&dlls));
        assert!(!takes_c_runtime_from_dll(&import_dlls(&tiny_pe(&[
            "KERNEL32.dll"
        ]))));
    }

    #[test]
    fn non_pe_bytes_yield_nothing() {
        assert!(import_dlls(b"not a pe at all").is_empty());
        assert!(import_dlls(&[]).is_empty());
        let mut b = vec![0u8; 0x100];
        b[0..2].copy_from_slice(b"MZ");
        b[0x3C] = 0x80;
        assert!(import_dlls(&b).is_empty());
    }

    #[test]
    fn offsets_that_overflow_yield_nothing_rather_than_panic() {
        let mut pe = tiny_pe(&["KERNEL32.dll"]);
        let sec = 0x44 + 20 + 240;
        pe[sec + 8..sec + 12].copy_from_slice(&u32::MAX.to_le_bytes());
        pe[sec + 12..sec + 16].copy_from_slice(&0xFFFF_F000u32.to_le_bytes());
        pe[sec + 20..sec + 24].copy_from_slice(&u32::MAX.to_le_bytes());
        let _ = import_dlls(&pe);
        let mut far = tiny_pe(&["KERNEL32.dll"]);
        far[0x3C..0x40].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(import_dlls(&far).is_empty());
    }

    /// The test program itself is a real PE on Windows, and imports kernel32.
    #[cfg(windows)]
    #[test]
    fn this_test_program_reads_as_a_pe_that_imports_kernel32() {
        let bytes = std::fs::read(std::env::current_exe().unwrap()).unwrap();
        let dlls = import_dlls(&bytes);
        assert!(
            dlls.iter().any(|d| d.eq_ignore_ascii_case("kernel32.dll")),
            "{dlls:?}"
        );
    }
}
