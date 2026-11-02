use std::env;
use std::fs;
use std::path::PathBuf;

use embed_manifest::{embed_manifest, new_manifest};

const ICON: &str = "assets/icon.ico";
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
/// 图标组的资源 ID，main.rs 用 LoadImageW(hinst, 1, ...) 读取；资源管理器取第一个图标组作为 exe 图标
const ICON_GROUP_ID: u16 = 1;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={ICON}");
    if env::var_os("CARGO_CFG_WINDOWS").is_none() {
        return;
    }
    // 嵌入清单：启用 Win10/11 原生控件样式和高 DPI 支持
    embed_manifest(new_manifest("SitReminder")).expect("unable to embed manifest");
    embed_icon();
}

/// 把 ico 文件拆成 RT_ICON + RT_GROUP_ICON 资源嵌入 exe。
/// 资源文件直接按格式写出，不依赖 rc.exe / windres：
/// MSVC 下生成 .res 交给 link.exe，GNU 下生成含 .rsrc 节的 COFF 目标文件交给 ld。
fn embed_icon() {
    let ico = fs::read(ICON).expect("unable to read icon");
    let resources = ico_to_resources(&ico);
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let path = if env::var("CARGO_CFG_TARGET_ENV").unwrap() == "msvc" {
        let p = out.join("icon.res");
        fs::write(&p, res_file(&resources)).unwrap();
        p
    } else {
        let machine = match env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
            "x86" => (0x014c, 7),   // IMAGE_FILE_MACHINE_I386, IMAGE_REL_I386_DIR32NB
            "x86_64" => (0x8664, 3), // IMAGE_FILE_MACHINE_AMD64, IMAGE_REL_AMD64_ADDR32NB
            "aarch64" => (0xaa64, 2), // IMAGE_FILE_MACHINE_ARM64, IMAGE_REL_ARM64_ADDR32NB
            arch => panic!("unsupported arch {arch}"),
        };
        let p = out.join("icon.o");
        fs::write(&p, coff_file(&resources, machine)).unwrap();
        p
    };
    println!("cargo:rustc-link-arg-bins={}", path.display());
}

struct Resource {
    kind: u16,
    id: u16,
    data: Vec<u8>,
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}

/// ICONDIR 的每个条目变成一个 RT_ICON；目录本身改写为 GRPICONDIR（偏移量换成资源 ID）
fn ico_to_resources(ico: &[u8]) -> Vec<Resource> {
    assert_eq!(u16_at(ico, 2), 1, "{ICON} is not an icon file");
    let count = u16_at(ico, 4) as usize;
    let mut group = ico[..6].to_vec();
    let mut res = Vec::new();
    for i in 0..count {
        let e = &ico[6 + i * 16..6 + (i + 1) * 16];
        let (size, offset) = (u32_at(e, 8) as usize, u32_at(e, 12) as usize);
        let id = i as u16 + 1;
        group.extend_from_slice(&e[..12]);
        group.extend_from_slice(&id.to_le_bytes());
        res.push(Resource { kind: RT_ICON, id, data: ico[offset..offset + size].to_vec() });
    }
    res.push(Resource { kind: RT_GROUP_ICON, id: ICON_GROUP_ID, data: group });
    res
}

fn pad(buf: &mut Vec<u8>, align: usize) {
    while buf.len() % align != 0 {
        buf.push(0);
    }
}

/// .res 格式：一个空的起始条目，然后每个资源一个 32 字节头 + 数据（4 字节对齐）
fn res_file(resources: &[Resource]) -> Vec<u8> {
    let mut buf = Vec::new();
    let entry = |buf: &mut Vec<u8>, kind: u16, id: u16, flags: u16, data: &[u8]| {
        buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
        buf.extend_from_slice(&32u32.to_le_bytes()); // HeaderSize
        buf.extend_from_slice(&[0xff, 0xff]);
        buf.extend_from_slice(&kind.to_le_bytes());
        buf.extend_from_slice(&[0xff, 0xff]);
        buf.extend_from_slice(&id.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes()); // DataVersion
        buf.extend_from_slice(&flags.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes()); // LanguageId: 中性
        buf.extend_from_slice(&[0; 8]); // Version, Characteristics
        buf.extend_from_slice(data);
        pad(buf, 4);
    };
    entry(&mut buf, 0, 0, 0, &[]);
    for r in resources {
        entry(&mut buf, r.kind, r.id, 0x1010, &r.data); // MOVEABLE | DISCARDABLE
    }
    buf
}

/// COFF 目标文件：单个 .rsrc 节，内含 类型 → ID → 语言 三级资源目录。
/// 数据条目里的 RVA 先写节内偏移，再用 ADDR32NB 重定位加上节的 RVA。
fn coff_file(resources: &[Resource], (machine, reloc_type): (u16, u16)) -> Vec<u8> {
    let mut kinds: Vec<u16> = resources.iter().map(|r| r.kind).collect();
    kinds.sort();
    kinds.dedup();
    let mut sorted: Vec<&Resource> = resources.iter().collect();
    sorted.sort_by_key(|r| (r.kind, r.id));
    let dir = |n: usize| 16 + 8 * n;

    // 先算出各部分在节内的起始位置：根目录 | 各类型的 ID 目录 | 各资源的语言目录 | 数据条目 | 数据
    let id_dirs = dir(kinds.len());
    let lang_dirs = id_dirs + kinds.iter().map(|k| dir(sorted.iter().filter(|r| r.kind == *k).count())).sum::<usize>();
    let data_entries = lang_dirs + sorted.len() * dir(1);
    let mut data_pos = Vec::new();
    let mut p = data_entries + sorted.len() * 16;
    for r in &sorted {
        data_pos.push(p);
        p = (p + r.data.len() + 7) & !7;
    }

    let mut sec = Vec::new();
    let table = |sec: &mut Vec<u8>, n: usize| {
        sec.extend_from_slice(&[0; 14]);
        sec.extend_from_slice(&(n as u16).to_le_bytes());
    };
    let entry = |sec: &mut Vec<u8>, id: u16, offset: usize, subdir: bool| {
        sec.extend_from_slice(&(id as u32).to_le_bytes());
        sec.extend_from_slice(&(offset as u32 | if subdir { 0x8000_0000 } else { 0 }).to_le_bytes());
    };
    table(&mut sec, kinds.len());
    let mut next = id_dirs;
    for k in &kinds {
        entry(&mut sec, *k, next, true);
        next += dir(sorted.iter().filter(|r| r.kind == *k).count());
    }
    let mut i = 0;
    for k in &kinds {
        let group: Vec<_> = sorted.iter().filter(|r| r.kind == *k).collect();
        table(&mut sec, group.len());
        for r in group {
            entry(&mut sec, r.id, lang_dirs + dir(1) * i, true);
            i += 1;
        }
    }
    for i in 0..sorted.len() {
        table(&mut sec, 1);
        entry(&mut sec, 0, data_entries + 16 * i, false);
    }
    let mut relocs = Vec::new();
    for (r, pos) in sorted.iter().zip(&data_pos) {
        relocs.push(sec.len() as u32);
        sec.extend_from_slice(&(*pos as u32).to_le_bytes());
        sec.extend_from_slice(&(r.data.len() as u32).to_le_bytes());
        sec.extend_from_slice(&[0; 8]);
    }
    for (r, pos) in sorted.iter().zip(&data_pos) {
        sec.resize(*pos, 0);
        sec.extend_from_slice(&r.data);
    }
    pad(&mut sec, 8);

    // 文件头(20) + 节头(40) + 节数据 + 重定位(10 each) + 符号表(节符号 + 辅助符号) + 空字符串表
    let raw_ptr = 60u32;
    let reloc_ptr = raw_ptr + sec.len() as u32;
    let sym_ptr = reloc_ptr + 10 * relocs.len() as u32;
    let mut obj = Vec::new();
    obj.extend_from_slice(&machine.to_le_bytes());
    obj.extend_from_slice(&1u16.to_le_bytes()); // 节数
    obj.extend_from_slice(&0u32.to_le_bytes()); // 时间戳
    obj.extend_from_slice(&sym_ptr.to_le_bytes());
    obj.extend_from_slice(&2u32.to_le_bytes()); // 符号数
    obj.extend_from_slice(&[0; 4]); // 可选头大小、特征
    obj.extend_from_slice(b".rsrc\0\0\0");
    obj.extend_from_slice(&[0; 8]); // VirtualSize, VirtualAddress
    obj.extend_from_slice(&(sec.len() as u32).to_le_bytes());
    obj.extend_from_slice(&raw_ptr.to_le_bytes());
    obj.extend_from_slice(&reloc_ptr.to_le_bytes());
    obj.extend_from_slice(&[0; 4]); // 行号指针
    obj.extend_from_slice(&(relocs.len() as u16).to_le_bytes());
    obj.extend_from_slice(&[0; 2]); // 行号数
    obj.extend_from_slice(&0xc030_0040u32.to_le_bytes()); // INITIALIZED_DATA | ALIGN_4BYTES | READ | WRITE
    obj.extend_from_slice(&sec);
    for addr in &relocs {
        obj.extend_from_slice(&addr.to_le_bytes());
        obj.extend_from_slice(&0u32.to_le_bytes()); // 符号 0：.rsrc 节符号
        obj.extend_from_slice(&reloc_type.to_le_bytes());
    }
    obj.extend_from_slice(b".rsrc\0\0\0");
    obj.extend_from_slice(&0u32.to_le_bytes()); // 值
    obj.extend_from_slice(&1u16.to_le_bytes()); // 节号
    obj.extend_from_slice(&[0, 0, 3, 1]); // 类型 0、STATIC、1 个辅助符号
    obj.extend_from_slice(&(sec.len() as u32).to_le_bytes());
    obj.extend_from_slice(&(relocs.len() as u16).to_le_bytes());
    obj.extend_from_slice(&[0; 12]);
    obj.extend_from_slice(&4u32.to_le_bytes()); // 字符串表（仅长度字段）
    obj
}
