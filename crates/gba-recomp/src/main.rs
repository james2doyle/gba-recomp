use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{SystemTime, UNIX_EPOCH};

use gba_ir::{ArmProgram, GBA_IR_VERSION};
use gba_llvm::{Optimization, emit_arm_program, emit_block, native_target};
use gba_runtime_abi::{
    ABI_HEADER_V2, ABI_HEADER_V3, ABI_HEADER_V4, ABI_RUNTIME_HLE_V1_SOURCE, ABI_RUNTIME_V2_SOURCE,
    ABI_RUNTIME_V3_SOURCE, GBA_ABI_VERSION_V4, GBA_MACHINE_STATE_VERSION_V1,
};

mod decoder;
mod discover;
mod rom;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("gba-recomp: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        return Err(usage());
    };
    match command.as_str() {
        "emit-fixture" => run_emit_fixture(args.collect()),
        "inspect-rom" => run_inspect_rom(args.collect()),
        "compile" => run_compile(args.collect()),
        _ => Err(format!("unknown command `{command}`\n{}", usage())),
    }
}

fn run_emit_fixture(arguments: Vec<String>) -> Result<(), String> {
    let mut args = arguments.into_iter();
    let mut output = None;
    let mut optimization = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--out" => output = Some(PathBuf::from(args.next().ok_or_else(usage)?)),
            "--opt" => optimization = Some(parse_optimization(&args.next().ok_or_else(usage)?)?),
            _ => return Err(format!("unknown argument `{flag}`\n{}", usage())),
        }
    }
    let output = output.ok_or_else(usage)?;
    let optimization = optimization.ok_or_else(usage)?;
    let block = gba_ir::alu_fixture();
    block
        .verify()
        .map_err(|error| format!("GBA IR rejected: {error}"))?;
    let info = emit_block(&block, &output, optimization)?;
    println!("object: {}", output.display());
    print_emit_info(&info);
    Ok(())
}

fn run_inspect_rom(arguments: Vec<String>) -> Result<(), String> {
    let mut args = arguments.into_iter();
    let mut rom = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--rom" => rom = Some(PathBuf::from(args.next().ok_or_else(usage)?)),
            _ => return Err(format!("unknown argument `{flag}`\n{}", usage())),
        }
    }
    let rom = rom.ok_or_else(usage)?;
    let image = rom::RomImage::read(&rom)?;
    println!(
        "{{\"sha256\":\"{}\",\"size\":{},\"title\":\"{}\",\"game_code\":\"{}\",\"maker_code\":\"{}\",\"version\":{},\"fixed_value\":{},\"stored_checksum\":{},\"calculated_checksum\":{},\"entrypoint\":\"0x{:08X}\"}}",
        image.sha256,
        image.size(),
        json_escape(&image.header.title),
        json_escape(&image.header.game_code),
        json_escape(&image.header.maker_code),
        image.header.version,
        image.header.fixed_value,
        image.header.stored_checksum,
        image.header.calculated_checksum,
        rom::ROM_BASE,
    );
    Ok(())
}

fn run_compile(arguments: Vec<String>) -> Result<(), String> {
    let mut args = arguments.into_iter();
    let mut rom_path = None;
    let mut output = None;
    let mut optimization = None;
    let mut steps = 100_u64;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--rom" => rom_path = Some(PathBuf::from(args.next().ok_or_else(usage)?)),
            "--out" => output = Some(PathBuf::from(args.next().ok_or_else(usage)?)),
            "--opt" => optimization = Some(parse_optimization(&args.next().ok_or_else(usage)?)?),
            "--steps" => {
                steps = args
                    .next()
                    .ok_or_else(usage)?
                    .parse()
                    .map_err(|_| "--steps must be an integer greater than zero".to_owned())?;
                if steps == 0 {
                    return Err("--steps must be greater than zero".to_owned());
                }
            }
            _ => return Err(format!("unknown argument `{flag}`\n{}", usage())),
        }
    }
    let rom_path = rom_path.ok_or_else(usage)?;
    let output = output.ok_or_else(usage)?;
    let optimization = optimization.ok_or_else(usage)?;
    let image = rom::RomImage::read(&rom_path)?;
    let program = discover::discover_arm_program(&image)?;
    let package = build_native_package(&image, &program, optimization, steps, &output)?;
    println!("package: {}", package.display());
    println!("rom_sha256: {}", image.sha256);
    println!("instructions_aot: {}", instruction_count(&program));
    println!("optimization: {}", optimization.as_str());
    println!("target: {}", native_target().triple);
    Ok(())
}

fn build_native_package(
    image: &rom::RomImage,
    program: &ArmProgram,
    optimization: Optimization,
    step_limit: u64,
    output: &Path,
) -> Result<PathBuf, String> {
    if output.exists() {
        return Err(format!(
            "output path already exists: {}; choose a new package path",
            output.display()
        ));
    }
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create output parent {}: {error}", parent.display()))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock before Unix epoch: {error}"))?
        .as_nanos();
    let output_name = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "output path must have a UTF-8 directory name".to_owned())?;
    let temporary = parent.join(format!(".{output_name}.tmp-{}-{stamp}", std::process::id()));
    fs::create_dir(&temporary)
        .map_err(|error| format!("cannot create temporary package: {error}"))?;

    let result = (|| {
        let object = temporary.join("game.o");
        let runner_source = temporary.join("runner.c");
        let header_v2 = temporary.join("gba_abi_v2.h");
        let header_v3 = temporary.join("gba_abi_v3.h");
        let header_v4 = temporary.join("gba_abi_v4.h");
        let runtime_v2_source = temporary.join("runtime_v2.c");
        let runtime_v3_source = temporary.join("runtime_v3.c");
        let runtime_hle_source = temporary.join("runtime_hle_v1.c");
        let executable = temporary.join("game");
        let emit_info = emit_arm_program(program, &object, optimization)?;
        fs::write(&header_v2, ABI_HEADER_V2)
            .map_err(|error| format!("cannot write embedded ABI v2 header: {error}"))?;
        fs::write(&header_v3, ABI_HEADER_V3)
            .map_err(|error| format!("cannot write embedded ABI v3 header: {error}"))?;
        fs::write(&header_v4, ABI_HEADER_V4)
            .map_err(|error| format!("cannot write embedded ABI v4 header: {error}"))?;
        fs::write(&runtime_v2_source, ABI_RUNTIME_V2_SOURCE)
            .map_err(|error| format!("cannot write embedded ABI v2 runtime: {error}"))?;
        fs::write(&runtime_v3_source, ABI_RUNTIME_V3_SOURCE)
            .map_err(|error| format!("cannot write embedded ABI v3 runtime: {error}"))?;
        fs::write(&runtime_hle_source, ABI_RUNTIME_HLE_V1_SOURCE)
            .map_err(|error| format!("cannot write embedded HLE v1 runtime: {error}"))?;
        fs::write(&runner_source, render_runner(step_limit))
            .map_err(|error| format!("cannot write generated host runner: {error}"))?;
        link_runner(
            &runner_source,
            &runtime_v2_source,
            &runtime_v3_source,
            &runtime_hle_source,
            &object,
            &header_v4,
            &executable,
        )?;
        let manifest = render_manifest(image, program, optimization, step_limit, &emit_info);
        fs::write(temporary.join("manifest.json"), manifest)
            .map_err(|error| format!("cannot write package manifest: {error}"))?;
        fs::remove_file(&runner_source)
            .map_err(|error| format!("cannot remove build-only runner source: {error}"))?;
        fs::remove_file(&header_v2)
            .map_err(|error| format!("cannot remove build-only ABI v2 header: {error}"))?;
        fs::remove_file(&header_v3)
            .map_err(|error| format!("cannot remove build-only ABI v3 header: {error}"))?;
        fs::remove_file(&header_v4)
            .map_err(|error| format!("cannot remove build-only ABI v4 header: {error}"))?;
        fs::remove_file(&runtime_v2_source)
            .map_err(|error| format!("cannot remove build-only ABI v2 runtime: {error}"))?;
        fs::remove_file(&runtime_v3_source)
            .map_err(|error| format!("cannot remove build-only ABI v3 runtime: {error}"))?;
        fs::remove_file(&runtime_hle_source)
            .map_err(|error| format!("cannot remove build-only HLE v1 runtime: {error}"))?;
        fs::remove_file(&object)
            .map_err(|error| format!("cannot remove build-only object: {error}"))?;
        fs::rename(&temporary, output)
            .map_err(|error| format!("cannot publish package {}: {error}", output.display()))?;
        Ok(output.to_path_buf())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

fn link_runner(
    runner_source: &Path,
    runtime_v2_source: &Path,
    runtime_v3_source: &Path,
    runtime_hle_source: &Path,
    object: &Path,
    header: &Path,
    executable: &Path,
) -> Result<(), String> {
    let prefix = env::var_os("LLVM_SYS_231_PREFIX").ok_or_else(|| {
        "set LLVM_SYS_231_PREFIX to the locked LLVM 23.1.3 installation".to_owned()
    })?;
    let prefix = PathBuf::from(prefix);
    let llvm_config = prefix.join("bin/llvm-config");
    let version = Command::new(&llvm_config)
        .arg("--version")
        .output()
        .map_err(|error| format!("cannot run {}: {error}", llvm_config.display()))?;
    if !version.status.success() || String::from_utf8_lossy(&version.stdout).trim() != "23.1.3" {
        return Err(format!(
            "LLVM 23.1.3 is required for final linking; {} reported {:?}",
            llvm_config.display(),
            String::from_utf8_lossy(&version.stdout).trim()
        ));
    }
    let clang = prefix.join("bin/clang");
    let mut command = Command::new(&clang);
    command
        .args([
            "-std=c11",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-fuse-ld=lld",
            "-Wl,--threads=10",
        ])
        .arg("-I")
        .arg(header.parent().expect("header has package directory"))
        .arg(runner_source)
        .arg(runtime_v2_source)
        .arg(runtime_v3_source)
        .arg(runtime_hle_source)
        .arg(object)
        .arg("-o")
        .arg(executable);
    #[cfg(target_os = "macos")]
    {
        let sysroot = env::var_os("SDKROOT")
            .map(PathBuf::from)
            .filter(|path| path.is_dir());
        let sysroot = match sysroot {
            Some(path) => path,
            None => macos_sdk_path()?,
        };
        command
            .arg("-isysroot")
            .arg(sysroot)
            .arg("-mmacosx-version-min=14.0.0");
    }
    let output = command
        .output()
        .map_err(|error| format!("cannot run {}: {error}", clang.display()))?;
    if !output.status.success() {
        return Err(format!(
            "native link failed with {}:\n{}{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_sdk_path() -> Result<PathBuf, String> {
    let output = Command::new("xcrun")
        .args(["--sdk", "macosx", "--show-sdk-path"])
        .output()
        .map_err(|error| format!("cannot query macOS SDK using xcrun: {error}"))?;
    if !output.status.success() {
        return Err("xcrun --sdk macosx --show-sdk-path failed".to_owned());
    }
    let path = String::from_utf8(output.stdout)
        .map_err(|error| format!("xcrun returned non-UTF-8 SDK path: {error}"))?;
    let path = PathBuf::from(path.trim());
    if !path.is_dir() {
        return Err(format!("macOS SDK does not exist: {}", path.display()));
    }
    Ok(path)
}

fn render_runner(step_limit: u64) -> String {
    format!(
        r#"#include <inttypes.h>
#include <stdio.h>
#include <string.h>
#include "gba_abi_v4.h"

static uint32_t read_le32(const uint8_t *bytes) {{
    return (uint32_t)bytes[0] | ((uint32_t)bytes[1] << 8) |
           ((uint32_t)bytes[2] << 16) | ((uint32_t)bytes[3] << 24);
}}

int main(void) {{
    GbaRuntimeV4 runtime;
    GbaMachineStateV1 machine;
    GbaExitV3 exit;
    memset(&runtime, 0, sizeof(runtime));
    memset(&machine, 0, sizeof(machine));
    memset(&exit, 0, sizeof(exit));
    runtime.base.cpu.abi_version = GBA_ABI_VERSION_V2;
    runtime.base.cpu.cpsr = UINT32_C(0x1F);
    runtime.base.cpu.r[15] = UINT32_C(0x08000000);
    runtime.base.memory = &machine.memory;
    runtime.base.abi_version = GBA_ABI_VERSION_V3;
    runtime.machine = &machine;
    runtime.abi_version = GBA_ABI_VERSION_V4;
    machine.abi_version = GBA_MACHINE_STATE_VERSION_V1;
    uint64_t executed = 0;
    for (; executed < UINT64_C({step_limit}); ++executed) {{
        uint32_t reason = gba_block_v4(&runtime, &exit);
        if (reason != exit.reason) {{
            fprintf(stderr, "ABI return/exit mismatch: %u/%u\n", reason, exit.reason);
            return 3;
        }}
        if (reason != GBA_EXIT_CONTINUE_V2) {{
            fprintf(stderr, "guest stopped: reason=%u pc=0x%08X address=0x%08X fault_code=%u\n", reason, runtime.base.cpu.r[15], exit.fault_address, exit.fault_code);
            return 4;
        }}
    }}
    printf("{{\"status\":\"budget_exhausted\",\"steps\":%" PRIu64 ",\"r0\":%u,\"r1\":%u,\"pc\":\"0x%08X\",\"cpsr\":\"0x%08X\",\"guest_cycle\":%" PRIu64 ",\"ewram_word0\":%u,\"iwram_word0\":%u}}\n", executed, runtime.base.cpu.r[0], runtime.base.cpu.r[1], runtime.base.cpu.r[15], runtime.base.cpu.cpsr, runtime.base.cpu.guest_cycle, read_le32(machine.memory.ewram), read_le32(machine.memory.iwram));
    return 0;
}}
"#
    )
}

fn render_manifest(
    image: &rom::RomImage,
    program: &ArmProgram,
    optimization: Optimization,
    step_limit: u64,
    emit_info: &gba_llvm::EmitInfo,
) -> String {
    format!(
        "{{\n  \"format_version\": 1,\n  \"rom_sha256\": \"{}\",\n  \"rom_size\": {},\n  \"game_code\": \"{}\",\n  \"rom_header_version\": {},\n  \"entrypoint\": \"0x{:08X}\",\n  \"ir_version\": {},\n  \"abi_version\": {},\n  \"machine_state_version\": {},\n  \"bios_hle_version\": {},\n  \"llvm_version\": \"{}\",\n  \"target_triple\": \"{}\",\n  \"optimization\": \"{}\",\n  \"execution_mode\": \"strict-aot\",\n  \"timing_model\": \"one abstract unit per instruction\",\n  \"instructions_aot\": {},\n  \"run_instruction_budget\": {}\n}}\n",
        image.sha256,
        image.size(),
        json_escape(&image.header.game_code),
        image.header.version,
        rom::ROM_BASE,
        GBA_IR_VERSION,
        GBA_ABI_VERSION_V4,
        GBA_MACHINE_STATE_VERSION_V1,
        1,
        emit_info.llvm_version,
        emit_info.triple,
        optimization.as_str(),
        instruction_count(program),
        step_limit,
    )
}

fn instruction_count(program: &ArmProgram) -> usize {
    program.instructions.len() + program.thumb_instructions.len()
}

fn parse_optimization(value: &str) -> Result<Optimization, String> {
    match value {
        "O0" => Ok(Optimization::O0),
        "O2" => Ok(Optimization::O2),
        _ => Err(format!("unsupported optimization `{value}`; use O0 or O2")),
    }
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => escaped.push(' '),
            character => escaped.push(character),
        }
    }
    escaped
}

fn print_emit_info(info: &gba_llvm::EmitInfo) {
    println!("LLVM: {}", info.llvm_version);
    println!(
        "target: {} ({}, {})",
        info.triple, info.cpu, info.object_format
    );
    println!("optimization: {}", info.optimization.as_str());
    println!("DataLayout: {}", info.data_layout);
}

fn usage() -> String {
    let target = native_target();
    format!(
        "usage:\n  gba-recomp inspect-rom --rom <path.gba>\n  gba-recomp compile --rom <path.gba> --out <package-dir> --opt O0|O2 [--steps N]\n  gba-recomp emit-fixture --out <object.o> --opt O0|O2\n       native target: {}",
        target.triple
    )
}
