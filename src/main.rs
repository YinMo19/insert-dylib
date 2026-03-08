use std::cmp::min;
use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::mem::size_of;
use std::path::Path;
use std::process;
use std::ptr;

const MH_MAGIC: u32 = 0xfeedface;
const MH_CIGAM: u32 = 0xcefaedfe;
const MH_MAGIC_64: u32 = 0xfeedfacf;
const MH_CIGAM_64: u32 = 0xcffaedfe;

const FAT_MAGIC: u32 = 0xcafebabe;
const FAT_CIGAM: u32 = 0xbebafeca;

const LC_SEGMENT: u32 = 0x1;
const LC_SYMTAB: u32 = 0x2;
const LC_LOAD_DYLIB: u32 = 0xc;
const LC_LOAD_WEAK_DYLIB: u32 = 0x80000018;
const LC_SEGMENT_64: u32 = 0x19;
const LC_CODE_SIGNATURE: u32 = 0x1d;
const LC_VERSION_MIN_MACOSX: u32 = 0x24;
const LC_VERSION_MIN_IPHONEOS: u32 = 0x25;
const LC_BUILD_VERSION: u32 = 0x32;

const PLATFORM_MACOS: u32 = 1;
const PLATFORM_IOS: u32 = 2;

const BUFSIZE: usize = 512;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct MachHeader {
    magic: u32,
    cputype: i32,
    cpusubtype: i32,
    filetype: u32,
    ncmds: u32,
    sizeofcmds: u32,
    flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct MachHeader64 {
    magic: u32,
    cputype: i32,
    cpusubtype: i32,
    filetype: u32,
    ncmds: u32,
    sizeofcmds: u32,
    flags: u32,
    reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct LoadCommand {
    cmd: u32,
    cmdsize: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct Dylib {
    name: u32,
    timestamp: u32,
    current_version: u32,
    compatibility_version: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct DylibCommand {
    cmd: u32,
    cmdsize: u32,
    dylib: Dylib,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct SegmentCommand {
    cmd: u32,
    cmdsize: u32,
    segname: [u8; 16],
    vmaddr: u32,
    vmsize: u32,
    fileoff: u32,
    filesize: u32,
    maxprot: i32,
    initprot: i32,
    nsects: u32,
    flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct SegmentCommand64 {
    cmd: u32,
    cmdsize: u32,
    segname: [u8; 16],
    vmaddr: u64,
    vmsize: u64,
    fileoff: u64,
    filesize: u64,
    maxprot: i32,
    initprot: i32,
    nsects: u32,
    flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct SymtabCommand {
    cmd: u32,
    cmdsize: u32,
    symoff: u32,
    nsyms: u32,
    stroff: u32,
    strsize: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct LinkeditDataCommand {
    cmd: u32,
    cmdsize: u32,
    dataoff: u32,
    datasize: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct VersionMinCommand {
    cmd: u32,
    cmdsize: u32,
    version: u32,
    sdk: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct BuildVersionCommand {
    cmd: u32,
    cmdsize: u32,
    platform: u32,
    minos: u32,
    sdk: u32,
    ntools: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct FatHeader {
    magic: u32,
    nfat_arch: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct FatArch {
    cputype: i32,
    cpusubtype: i32,
    offset: u32,
    size: u32,
    align: u32,
}

#[derive(Clone, Debug, Default)]
struct Options {
    inplace: bool,
    weak: bool,
    overwrite: bool,
    codesig_flag: u8,
    all_yes: bool,
    ios: bool,
    ios_dylib_path: Option<String>,
}

#[derive(Clone, Copy, Debug, Default)]
struct PlatformRewriteStats {
    platform_commands: usize,
    rewritten_commands: usize,
}

fn is_64_bit(magic: u32) -> bool {
    magic == MH_MAGIC_64 || magic == MH_CIGAM_64
}

fn should_swap(magic: u32) -> bool {
    magic == FAT_CIGAM || magic == MH_CIGAM_64 || magic == MH_CIGAM
}

fn swap32(value: u32, magic: u32) -> u32 {
    if should_swap(magic) {
        value.swap_bytes()
    } else {
        value
    }
}

fn swap64(value: u64, magic: u32) -> u64 {
    if should_swap(magic) {
        value.swap_bytes()
    } else {
        value
    }
}

fn round_up(value: u64, align: u64) -> u64 {
    if align == 0 {
        value
    } else {
        (value + align - 1) & !(align - 1)
    }
}

fn absdiff(lhs: u64, rhs: u64) -> u64 {
    lhs.abs_diff(rhs)
}

fn usage() -> ! {
    println!("Usage: insert-dylib dylib_path binary_path [new_binary_path]");
    println!(
        "Option flags: --inplace --weak --overwrite --strip-codesig --no-strip-codesig --all-yes --ios --dylib-path <path>"
    );
    println!("Note: --ios requires --dylib-path <local_dylib_file>.");
    process::exit(1);
}

fn ask(options: &Options, prompt: &str) -> io::Result<bool> {
    print!("{prompt} [y/n] ");
    io::stdout().flush()?;

    if options.all_yes {
        println!("y");
        return Ok(true);
    }

    loop {
        let mut line = String::new();
        let bytes = io::stdin().read_line(&mut line)?;
        if bytes == 0 {
            return Ok(false);
        }

        match line.chars().next() {
            Some('y' | 'Y') => return Ok(true),
            Some('n' | 'N') => return Ok(false),
            _ => {
                print!("Please enter y or n: ");
                io::stdout().flush()?;
            }
        }
    }
}

fn read_struct<T: Copy>(file: &mut File) -> io::Result<T> {
    let mut buf = vec![0_u8; size_of::<T>()];
    file.read_exact(&mut buf)?;

    let value = unsafe { ptr::read_unaligned(buf.as_ptr() as *const T) };
    Ok(value)
}

fn write_struct<T: Copy>(file: &mut File, value: &T) -> io::Result<()> {
    let bytes =
        unsafe { std::slice::from_raw_parts((value as *const T) as *const u8, size_of::<T>()) };
    file.write_all(bytes)
}

fn peek_struct<T: Copy>(file: &mut File) -> io::Result<T> {
    let pos = file.stream_position()?;
    let value = read_struct::<T>(file)?;
    file.seek(SeekFrom::Start(pos))?;
    Ok(value)
}

fn read_load_command(file: &mut File, cmdsize: usize) -> io::Result<Vec<u8>> {
    let pos = file.stream_position()?;
    let mut buf = vec![0_u8; cmdsize];
    file.read_exact(&mut buf)?;
    file.seek(SeekFrom::Start(pos))?;
    Ok(buf)
}

fn fbzero(file: &mut File, offset: u64, mut len: u64) -> io::Result<()> {
    static ZEROS: [u8; BUFSIZE] = [0; BUFSIZE];

    file.seek(SeekFrom::Start(offset))?;
    while len != 0 {
        let size = min(len as usize, ZEROS.len());
        file.write_all(&ZEROS[..size])?;
        len -= size as u64;
    }

    Ok(())
}

fn fmemmove(file: &mut File, mut dst: u64, mut src: u64, mut len: u64) -> io::Result<()> {
    let mut buf = [0_u8; BUFSIZE];

    while len != 0 {
        let size = min(len as usize, buf.len());
        file.seek(SeekFrom::Start(src))?;
        file.read_exact(&mut buf[..size])?;
        file.seek(SeekFrom::Start(dst))?;
        file.write_all(&buf[..size])?;

        len -= size as u64;
        src += size as u64;
        dst += size as u64;
    }

    Ok(())
}

fn struct_from_bytes<T: Copy>(bytes: &[u8]) -> io::Result<T> {
    if bytes.len() < size_of::<T>() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "buffer too small for structure",
        ));
    }

    let value = unsafe { ptr::read_unaligned(bytes.as_ptr() as *const T) };
    Ok(value)
}

fn write_struct_into_prefix<T: Copy>(bytes: &mut [u8], value: &T) {
    let src =
        unsafe { std::slice::from_raw_parts((value as *const T) as *const u8, size_of::<T>()) };
    bytes[..src.len()].copy_from_slice(src);
}

fn parse_fixed_c_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn parse_c_string(bytes: &[u8], offset: usize) -> String {
    if offset >= bytes.len() {
        return String::new();
    }
    parse_fixed_c_string(&bytes[offset..])
}

fn check_load_commands(
    file: &mut File,
    mh: &mut MachHeader,
    header_offset: u64,
    commands_offset: u64,
    dylib_path: &str,
    slice_size: &mut u64,
    options: &Options,
) -> io::Result<bool> {
    file.seek(SeekFrom::Start(commands_offset))?;

    let ncmds = swap32(mh.ncmds, mh.magic);

    let mut linkedit_32_pos: Option<u64> = None;
    let mut linkedit_64_pos: Option<u64> = None;
    let mut linkedit_32: Option<SegmentCommand> = None;
    let mut linkedit_64: Option<SegmentCommand64> = None;

    let mut symtab_pos: Option<u64> = None;
    let mut symtab_size: u32 = 0;

    for i in 0..ncmds {
        let lc: LoadCommand = peek_struct(file)?;

        let cmdsize = swap32(lc.cmdsize, mh.magic);
        let cmd = swap32(lc.cmd, mh.magic);

        match cmd {
            LC_CODE_SIGNATURE => {
                if i == ncmds - 1 {
                    if options.codesig_flag == 2 {
                        return Ok(true);
                    }

                    if options.codesig_flag == 0
                        && !ask(options, "LC_CODE_SIGNATURE load command found. Remove it?")?
                    {
                        return Ok(true);
                    }

                    let cmd_bytes = read_load_command(file, cmdsize as usize)?;
                    let cmd_data: LinkeditDataCommand = struct_from_bytes(&cmd_bytes)?;

                    let command_offset = file.stream_position()?;
                    fbzero(file, command_offset, cmdsize as u64)?;

                    let dataoff = swap32(cmd_data.dataoff, mh.magic) as u64;
                    let datasize = swap32(cmd_data.datasize, mh.magic) as u64;

                    let mut linkedit_fileoff = 0_u64;
                    let mut linkedit_filesize = 0_u64;

                    if let Some(seg) = linkedit_32 {
                        linkedit_fileoff = swap32(seg.fileoff, mh.magic) as u64;
                        linkedit_filesize = swap32(seg.filesize, mh.magic) as u64;
                    } else if let Some(seg) = linkedit_64 {
                        linkedit_fileoff = swap64(seg.fileoff, mh.magic);
                        linkedit_filesize = swap64(seg.filesize, mh.magic);
                    } else {
                        eprintln!("Warning: __LINKEDIT segment not found.");
                    }

                    if linkedit_32_pos.is_some() || linkedit_64_pos.is_some() {
                        if linkedit_fileoff + linkedit_filesize != *slice_size {
                            eprintln!(
                                "Warning: __LINKEDIT segment is not at the end of the file, so codesign will not work on the patched binary."
                            );
                        } else if dataoff + datasize != *slice_size {
                            eprintln!(
                                "Warning: Codesignature is not at the end of __LINKEDIT segment, so codesign will not work on the patched binary."
                            );
                        } else {
                            *slice_size -= datasize;

                            if let Some(symtab_cmd_pos) = symtab_pos {
                                file.seek(SeekFrom::Start(symtab_cmd_pos))?;
                                let mut symtab_bytes =
                                    read_load_command(file, symtab_size as usize)?;
                                let mut symtab: SymtabCommand = struct_from_bytes(&symtab_bytes)?;

                                let strsize = swap32(symtab.strsize, mh.magic) as i64;
                                let stroff = swap32(symtab.stroff, mh.magic) as i64;
                                let diff_size = stroff + strsize - *slice_size as i64;

                                if (-0x10..=0).contains(&diff_size) {
                                    let new_strsize = (strsize - diff_size) as u32;
                                    symtab.strsize = swap32(new_strsize, mh.magic);
                                    write_struct_into_prefix(&mut symtab_bytes, &symtab);

                                    file.seek(SeekFrom::Start(symtab_cmd_pos))?;
                                    file.write_all(&symtab_bytes)?;
                                } else {
                                    eprintln!(
                                        "Warning: String table doesn't appear right before code signature. codesign might not work on the patched binary. (0x{:x})",
                                        diff_size
                                    );
                                }
                            } else {
                                eprintln!(
                                    "Warning: LC_SYMTAB load command not found. codesign might not work on the patched binary."
                                );
                            }

                            linkedit_filesize -= datasize;
                            let linkedit_vmsize = round_up(linkedit_filesize, 0x1000);

                            if let Some(pos) = linkedit_32_pos
                                && let Some(mut seg) = linkedit_32
                            {
                                seg.filesize = swap32(linkedit_filesize as u32, mh.magic);
                                seg.vmsize = swap32(linkedit_vmsize as u32, mh.magic);

                                file.seek(SeekFrom::Start(pos))?;
                                write_struct(file, &seg)?;
                            } else if let Some(pos) = linkedit_64_pos
                                && let Some(mut seg) = linkedit_64
                            {
                                seg.filesize = swap64(linkedit_filesize, mh.magic);
                                seg.vmsize = swap64(linkedit_vmsize, mh.magic);

                                file.seek(SeekFrom::Start(pos))?;
                                write_struct(file, &seg)?;
                            }

                            mh.ncmds = swap32(ncmds - 1, mh.magic);
                            mh.sizeofcmds =
                                swap32(swap32(mh.sizeofcmds, mh.magic) - cmdsize, mh.magic);

                            return Ok(true);
                        }
                    }

                    fbzero(file, header_offset + dataoff, datasize)?;

                    mh.ncmds = swap32(ncmds - 1, mh.magic);
                    mh.sizeofcmds = swap32(swap32(mh.sizeofcmds, mh.magic) - cmdsize, mh.magic);

                    return Ok(true);
                }

                println!("LC_CODE_SIGNATURE is not the last load command, so couldn't remove.");
            }
            LC_LOAD_DYLIB | LC_LOAD_WEAK_DYLIB => {
                let dylib_bytes = read_load_command(file, cmdsize as usize)?;
                let dylib_command: DylibCommand = struct_from_bytes(&dylib_bytes)?;
                let name_offset = swap32(dylib_command.dylib.name, mh.magic) as usize;
                let name = parse_c_string(&dylib_bytes, name_offset);

                if name == dylib_path
                    && !ask(
                        options,
                        "Binary already contains a load command for that dylib. Continue anyway?",
                    )?
                {
                    return Ok(false);
                }
            }
            LC_SEGMENT => {
                let seg_bytes = read_load_command(file, cmdsize as usize)?;
                let seg: SegmentCommand = struct_from_bytes(&seg_bytes)?;
                if parse_fixed_c_string(&seg.segname) == "__LINKEDIT" {
                    linkedit_32_pos = Some(file.stream_position()?);
                    linkedit_32 = Some(seg);
                }
            }
            LC_SEGMENT_64 => {
                let seg_bytes = read_load_command(file, cmdsize as usize)?;
                let seg: SegmentCommand64 = struct_from_bytes(&seg_bytes)?;
                if parse_fixed_c_string(&seg.segname) == "__LINKEDIT" {
                    linkedit_64_pos = Some(file.stream_position()?);
                    linkedit_64 = Some(seg);
                }
            }
            LC_SYMTAB => {
                symtab_pos = Some(file.stream_position()?);
                symtab_size = cmdsize;
            }
            _ => {}
        }

        file.seek(SeekFrom::Current(cmdsize as i64))?;
    }

    Ok(true)
}

fn insert_dylib(
    file: &mut File,
    header_offset: u64,
    dylib_path: &str,
    slice_size: &mut u64,
    options: &Options,
) -> io::Result<bool> {
    file.seek(SeekFrom::Start(header_offset))?;

    let mut mh: MachHeader = read_struct(file)?;
    if mh.magic != MH_MAGIC_64
        && mh.magic != MH_CIGAM_64
        && mh.magic != MH_MAGIC
        && mh.magic != MH_CIGAM
    {
        println!("Unknown magic: 0x{:x}", mh.magic);
        return Ok(false);
    }

    let commands_offset = header_offset
        + if is_64_bit(mh.magic) {
            size_of::<MachHeader64>() as u64
        } else {
            size_of::<MachHeader>() as u64
        };

    let cont = check_load_commands(
        file,
        &mut mh,
        header_offset,
        commands_offset,
        dylib_path,
        slice_size,
        options,
    )?;

    if !cont {
        return Ok(true);
    }

    let path_padding = 8_usize;
    let dylib_path_len = dylib_path.len();
    let dylib_path_size = (dylib_path_len & !(path_padding - 1)) + path_padding;
    let cmdsize = (size_of::<DylibCommand>() + dylib_path_size) as u32;

    let dylib_command = DylibCommand {
        cmd: swap32(
            if options.weak {
                LC_LOAD_WEAK_DYLIB
            } else {
                LC_LOAD_DYLIB
            },
            mh.magic,
        ),
        cmdsize: swap32(cmdsize, mh.magic),
        dylib: Dylib {
            name: swap32(size_of::<DylibCommand>() as u32, mh.magic),
            timestamp: 0,
            current_version: 0,
            compatibility_version: 0,
        },
    };

    let mut sizeofcmds = swap32(mh.sizeofcmds, mh.magic);

    let insert_pos = commands_offset + sizeofcmds as u64;
    file.seek(SeekFrom::Start(insert_pos))?;

    let mut space = vec![0_u8; cmdsize as usize];
    let read_len = file.read(&mut space)?;
    let empty = read_len == space.len() && space.iter().all(|b| *b == 0);
    if !empty
        && !ask(
            options,
            "It doesn't seem like there is enough empty space. Continue anyway?",
        )?
    {
        return Ok(false);
    }

    file.seek(SeekFrom::Start(insert_pos))?;

    let mut dylib_path_padded = vec![0_u8; dylib_path_size];
    dylib_path_padded[..dylib_path_len].copy_from_slice(dylib_path.as_bytes());

    write_struct(file, &dylib_command)?;
    file.write_all(&dylib_path_padded)?;

    mh.ncmds = swap32(swap32(mh.ncmds, mh.magic) + 1, mh.magic);
    sizeofcmds += cmdsize;
    mh.sizeofcmds = swap32(sizeofcmds, mh.magic);

    file.seek(SeekFrom::Start(header_offset))?;
    write_struct(file, &mh)?;

    Ok(true)
}

fn rewrite_macho_platform_to_ios_slice(
    file: &mut File,
    header_offset: u64,
) -> io::Result<PlatformRewriteStats> {
    file.seek(SeekFrom::Start(header_offset))?;

    let mh: MachHeader = read_struct(file)?;
    if mh.magic != MH_MAGIC_64
        && mh.magic != MH_CIGAM_64
        && mh.magic != MH_MAGIC
        && mh.magic != MH_CIGAM
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Unknown Mach-O magic: 0x{:x}", mh.magic),
        ));
    }

    let commands_offset = header_offset
        + if is_64_bit(mh.magic) {
            size_of::<MachHeader64>() as u64
        } else {
            size_of::<MachHeader>() as u64
        };

    file.seek(SeekFrom::Start(commands_offset))?;
    let ncmds = swap32(mh.ncmds, mh.magic);
    let mut stats = PlatformRewriteStats::default();

    for _ in 0..ncmds {
        let command_offset = file.stream_position()?;
        let lc: LoadCommand = peek_struct(file)?;
        let cmd = swap32(lc.cmd, mh.magic);
        let cmdsize = swap32(lc.cmdsize, mh.magic);

        if cmdsize < size_of::<LoadCommand>() as u32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "load command size is smaller than load_command",
            ));
        }

        match cmd {
            LC_BUILD_VERSION => {
                if cmdsize < size_of::<BuildVersionCommand>() as u32 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "LC_BUILD_VERSION command is truncated",
                    ));
                }

                let mut cmd_data: BuildVersionCommand = read_struct(file)?;
                let platform = swap32(cmd_data.platform, mh.magic);
                stats.platform_commands += 1;

                if platform == PLATFORM_MACOS {
                    cmd_data.platform = swap32(PLATFORM_IOS, mh.magic);
                    file.seek(SeekFrom::Start(command_offset))?;
                    write_struct(file, &cmd_data)?;
                    stats.rewritten_commands += 1;
                }
            }
            LC_VERSION_MIN_MACOSX => {
                if cmdsize < size_of::<VersionMinCommand>() as u32 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "LC_VERSION_MIN_MACOSX command is truncated",
                    ));
                }

                let mut cmd_data: VersionMinCommand = read_struct(file)?;
                cmd_data.cmd = swap32(LC_VERSION_MIN_IPHONEOS, mh.magic);
                file.seek(SeekFrom::Start(command_offset))?;
                write_struct(file, &cmd_data)?;
                stats.platform_commands += 1;
                stats.rewritten_commands += 1;
            }
            LC_VERSION_MIN_IPHONEOS => {
                stats.platform_commands += 1;
            }
            _ => {}
        }

        file.seek(SeekFrom::Start(command_offset + cmdsize as u64))?;
    }

    Ok(stats)
}

fn rewrite_dylib_platform_to_ios(path: &str) -> io::Result<PlatformRewriteStats> {
    let mut file = File::options().read(true).write(true).open(path)?;
    let magic: u32 = read_struct(&mut file)?;

    match magic {
        FAT_MAGIC | FAT_CIGAM => {
            file.seek(SeekFrom::Start(0))?;
            let fat_header: FatHeader = read_struct(&mut file)?;
            let nfat_arch = swap32(fat_header.nfat_arch, magic) as usize;
            let archs = read_fat_arches(&mut file, nfat_arch)?;

            let mut total = PlatformRewriteStats::default();
            for arch in archs {
                let offset = swap32(arch.offset, magic) as u64;
                let stats = rewrite_macho_platform_to_ios_slice(&mut file, offset)?;
                total.platform_commands += stats.platform_commands;
                total.rewritten_commands += stats.rewritten_commands;
            }
            Ok(total)
        }
        MH_MAGIC_64 | MH_CIGAM_64 | MH_MAGIC | MH_CIGAM => {
            file.seek(SeekFrom::Start(0))?;
            rewrite_macho_platform_to_ios_slice(&mut file, 0)
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Unknown Mach-O magic: 0x{:x}", magic),
        )),
    }
}

fn parse_args() -> (Options, Vec<String>) {
    let mut options = Options::default();
    let mut positional = Vec::new();

    let mut args = env::args().skip(1).peekable();
    let mut parsing_options = true;
    while let Some(arg) = args.next() {
        if parsing_options {
            if arg == "--" {
                parsing_options = false;
                continue;
            }

            if arg.starts_with("--") {
                match arg.as_str() {
                    "--inplace" => options.inplace = true,
                    "--weak" => options.weak = true,
                    "--overwrite" => options.overwrite = true,
                    "--strip-codesig" => options.codesig_flag = 1,
                    "--no-strip-codesig" => options.codesig_flag = 2,
                    "--all-yes" => options.all_yes = true,
                    "--ios" => options.ios = true,
                    "--dylib-path" => {
                        let path = args.next().unwrap_or_else(|| usage());
                        options.ios_dylib_path = Some(path);
                    }
                    _ => {
                        if let Some(path) = arg.strip_prefix("--dylib-path=") {
                            if path.is_empty() {
                                usage();
                            }
                            options.ios_dylib_path = Some(path.to_string());
                        } else {
                            usage();
                        }
                    }
                }
                continue;
            }
        }

        positional.push(arg);
    }

    if positional.len() < 2 || positional.len() > 3 {
        usage();
    }

    (options, positional)
}

fn read_fat_arches(file: &mut File, count: usize) -> io::Result<Vec<FatArch>> {
    let mut archs = Vec::with_capacity(count);
    for _ in 0..count {
        archs.push(read_struct::<FatArch>(file)?);
    }
    Ok(archs)
}

fn write_fat_arches(file: &mut File, archs: &[FatArch]) -> io::Result<()> {
    for arch in archs {
        write_struct(file, arch)?;
    }
    Ok(())
}

fn run() -> io::Result<i32> {
    let (options, positional) = parse_args();

    let lc_name = if options.weak {
        "LC_LOAD_WEAK_DYLIB"
    } else {
        "LC_LOAD_DYLIB"
    };

    let dylib_path = positional[0].clone();
    let mut binary_path = positional[1].clone();

    if fs::metadata(&binary_path).is_err() {
        eprintln!("{binary_path}: not found");
        return Ok(1);
    }

    if !dylib_path.starts_with('@')
        && fs::metadata(&dylib_path).is_err()
        && !ask(
            &options,
            "The provided dylib path doesn't exist. Continue anyway?",
        )?
    {
        return Ok(1);
    }

    if options.ios_dylib_path.is_some() && !options.ios {
        eprintln!("--dylib-path can only be used together with --ios.");
        return Ok(1);
    }

    if options.ios {
        let ios_dylib_path = options.ios_dylib_path.as_deref().unwrap_or_else(|| {
            eprintln!("--ios requires --dylib-path <local_dylib_file>.");
            usage();
        });

        if fs::metadata(ios_dylib_path).is_err() {
            eprintln!("{ios_dylib_path}: not found (--dylib-path)");
            return Ok(1);
        }

        let stats = rewrite_dylib_platform_to_ios(ios_dylib_path)?;
        if stats.platform_commands == 0 {
            println!("No platform load command found in {ios_dylib_path}; skipped --ios rewrite.");
        } else if stats.rewritten_commands == 0 {
            println!("{ios_dylib_path} already declares iOS platform; no rewrite needed.");
        } else {
            println!(
                "Rewrote {} platform load command(s) from macOS to iOS in {ios_dylib_path}.",
                stats.rewritten_commands
            );
        }
    }

    if !options.inplace {
        let new_binary_path = if positional.len() == 3 {
            positional[2].clone()
        } else {
            format!("{binary_path}_patched")
        };

        if !options.overwrite
            && Path::new(&new_binary_path).exists()
            && !ask(
                &options,
                &format!("{new_binary_path} already exists. Overwrite it?"),
            )?
        {
            return Ok(1);
        }

        if Path::new(&new_binary_path).exists() {
            fs::remove_file(&new_binary_path)?;
        }

        if fs::copy(&binary_path, &new_binary_path).is_err() {
            println!("Failed to create {new_binary_path}");
            return Ok(1);
        }

        binary_path = new_binary_path;
    }

    let mut file = File::options().read(true).write(true).open(&binary_path)?;

    let mut success = true;
    let mut file_size = file.seek(SeekFrom::End(0))?;
    file.seek(SeekFrom::Start(0))?;

    let magic: u32 = read_struct(&mut file)?;

    match magic {
        FAT_MAGIC | FAT_CIGAM => {
            file.seek(SeekFrom::Start(0))?;
            let fat_header: FatHeader = read_struct(&mut file)?;
            let nfat_arch = swap32(fat_header.nfat_arch, magic) as usize;

            println!("Binary is a fat binary with {nfat_arch} archs.");

            let mut archs = read_fat_arches(&mut file, nfat_arch)?;
            let mut fails = 0_usize;

            let mut offset = if nfat_arch > 0 {
                swap32(archs[0].offset, magic) as u64
            } else {
                0
            };

            for (i, arch) in archs.iter_mut().enumerate() {
                let orig_offset = swap32(arch.offset, magic) as u64;
                let orig_slice_size = swap32(arch.size, magic) as u64;
                let align = swap32(arch.align, magic);
                let align_value = 1_u64.checked_shl(align).unwrap_or(1);

                offset = round_up(offset, align_value);
                if orig_offset != offset {
                    fmemmove(&mut file, offset, orig_offset, orig_slice_size)?;
                    let zero_start = min(offset, orig_offset) + orig_slice_size;
                    fbzero(&mut file, zero_start, absdiff(offset, orig_offset))?;

                    arch.offset = swap32(offset as u32, magic);
                }

                let mut slice_size = orig_slice_size;
                if !insert_dylib(&mut file, offset, &dylib_path, &mut slice_size, &options)? {
                    println!("Failed to add {lc_name} to arch #{}!", i + 1);
                    fails += 1;
                }

                if slice_size < orig_slice_size && i < nfat_arch - 1 {
                    fbzero(&mut file, offset + slice_size, orig_slice_size - slice_size)?;
                }

                file_size = offset + slice_size;
                offset += slice_size;
                arch.size = swap32(slice_size as u32, magic);
            }

            file.seek(SeekFrom::Start(0))?;
            write_struct(&mut file, &fat_header)?;
            write_fat_arches(&mut file, &archs)?;

            file.flush()?;
            file.set_len(file_size)?;

            if fails == 0 {
                println!("Added {lc_name} to all archs in {binary_path}");
            } else if fails == nfat_arch {
                println!("Failed to add {lc_name} to any archs.");
                success = false;
            } else {
                println!(
                    "Added {lc_name} to {}/{} archs in {binary_path}",
                    nfat_arch - fails,
                    nfat_arch
                );
            }
        }
        MH_MAGIC_64 | MH_CIGAM_64 | MH_MAGIC | MH_CIGAM => {
            if insert_dylib(&mut file, 0, &dylib_path, &mut file_size, &options)? {
                file.set_len(file_size)?;
                println!("Added {lc_name} to {binary_path}");
            } else {
                println!("Failed to add {lc_name}!");
                success = false;
            }
        }
        _ => {
            println!("Unknown magic: 0x{:x}", magic);
            return Ok(1);
        }
    }

    drop(file);

    if !success {
        if !options.inplace {
            let _ = fs::remove_file(&binary_path);
        }
        return Ok(1);
    }

    Ok(0)
}

fn main() {
    match run() {
        Ok(code) => {
            if code != 0 {
                process::exit(code);
            }
        }
        Err(err) => {
            eprintln!("{err}");
            process::exit(1);
        }
    }
}
