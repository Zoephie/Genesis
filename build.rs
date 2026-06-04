use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const MAIN_ICON_ID: u16 = 1;
const FIRST_IMAGE_ID: u16 = 2;
const ICON_MEMORY_FLAGS: u16 = 0x1030;
const LANG_EN_US: u16 = 0x0409;

#[derive(Clone)]
struct IconEntry {
    width: u8,
    height: u8,
    color_count: u8,
    reserved: u8,
    planes: u16,
    bit_count: u16,
    bytes_in_res: u32,
    image_offset: u32,
}

fn main() {
    println!("cargo:rerun-if-changed=icons/Genesis.ico");

    if env::var_os("CARGO_CFG_WINDOWS").is_none() {
        return;
    }

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let ico_path = manifest_dir.join("icons").join("Genesis.ico");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let res_path = out_dir.join("Genesis.res");

    write_icon_resource(&ico_path, &res_path).expect("write Genesis icon resource");
    println!("cargo:rustc-link-arg-bin=Genesis={}", res_path.display());
}

fn write_icon_resource(ico_path: &Path, res_path: &Path) -> io::Result<()> {
    let ico = fs::read(ico_path)?;
    let entries = parse_ico_entries(&ico)?;
    let mut res = Vec::new();

    write_resource_record(&mut res, 0, 0, 0, 0, &[]);

    for (index, entry) in entries.iter().enumerate() {
        let image_id = FIRST_IMAGE_ID + index as u16;
        let start = entry.image_offset as usize;
        let end = start
            .checked_add(entry.bytes_in_res as usize)
            .ok_or_else(|| invalid_data("icon image range overflow"))?;
        let image = ico
            .get(start..end)
            .ok_or_else(|| invalid_data("icon image range is out of bounds"))?;
        write_resource_record(
            &mut res,
            RT_ICON,
            image_id,
            ICON_MEMORY_FLAGS,
            LANG_EN_US,
            image,
        );
    }

    let mut group = Vec::with_capacity(6 + entries.len() * 14);
    push_u16(&mut group, 0);
    push_u16(&mut group, 1);
    push_u16(&mut group, entries.len() as u16);
    for (index, entry) in entries.iter().enumerate() {
        group.push(entry.width);
        group.push(entry.height);
        group.push(entry.color_count);
        group.push(entry.reserved);
        push_u16(&mut group, entry.planes);
        push_u16(&mut group, entry.bit_count);
        push_u32(&mut group, entry.bytes_in_res);
        push_u16(&mut group, FIRST_IMAGE_ID + index as u16);
    }

    write_resource_record(
        &mut res,
        RT_GROUP_ICON,
        MAIN_ICON_ID,
        ICON_MEMORY_FLAGS,
        LANG_EN_US,
        &group,
    );

    fs::write(res_path, res)
}

fn parse_ico_entries(ico: &[u8]) -> io::Result<Vec<IconEntry>> {
    if ico.len() < 6 {
        return Err(invalid_data("ICO header is too short"));
    }
    if read_u16(ico, 0)? != 0 || read_u16(ico, 2)? != 1 {
        return Err(invalid_data("file is not a Windows icon"));
    }
    let count = read_u16(ico, 4)? as usize;
    if count == 0 {
        return Err(invalid_data("ICO has no images"));
    }
    let table_len = 6usize
        .checked_add(
            count
                .checked_mul(16)
                .ok_or_else(|| invalid_data("ICO table overflow"))?,
        )
        .ok_or_else(|| invalid_data("ICO table overflow"))?;
    if ico.len() < table_len {
        return Err(invalid_data("ICO image table is truncated"));
    }

    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let offset = 6 + index * 16;
        entries.push(IconEntry {
            width: ico[offset],
            height: ico[offset + 1],
            color_count: ico[offset + 2],
            reserved: ico[offset + 3],
            planes: read_u16(ico, offset + 4)?,
            bit_count: read_u16(ico, offset + 6)?,
            bytes_in_res: read_u32(ico, offset + 8)?,
            image_offset: read_u32(ico, offset + 12)?,
        });
    }
    Ok(entries)
}

fn write_resource_record(
    out: &mut Vec<u8>,
    type_id: u16,
    name_id: u16,
    memory_flags: u16,
    language_id: u16,
    data: &[u8],
) {
    align_to_dword(out);
    let start = out.len();
    push_u32(out, data.len() as u32);
    push_u32(out, 0);
    push_ordinal(out, type_id);
    push_ordinal(out, name_id);
    align_to_dword(out);
    push_u32(out, 0);
    push_u16(out, memory_flags);
    push_u16(out, language_id);
    push_u32(out, 0);
    push_u32(out, 0);

    let header_size = (out.len() - start) as u32;
    out[start + 4..start + 8].copy_from_slice(&header_size.to_le_bytes());
    out.extend_from_slice(data);
    align_to_dword(out);
}

fn push_ordinal(out: &mut Vec<u8>, value: u16) {
    push_u16(out, 0xffff);
    push_u16(out, value);
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn align_to_dword(out: &mut Vec<u8>) {
    while out.len() % 4 != 0 {
        out.push(0);
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> io::Result<u16> {
    let data = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| invalid_data("unexpected end of file"))?;
    Ok(u16::from_le_bytes([data[0], data[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> io::Result<u32> {
    let data = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| invalid_data("unexpected end of file"))?;
    Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
