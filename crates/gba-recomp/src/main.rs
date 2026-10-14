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

mod compat;
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
        "bundle" => run_bundle(args.collect()),
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
    let mut mode = ExecutionMode::StrictAot;
    let mut profile = compat::ProfileOptions {
        frames: 600,
        inputs: Vec::new(),
        follow_direct_branches: true,
        ram_code: true,
        embed_rom: false,
    };
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--rom" => rom_path = Some(PathBuf::from(args.next().ok_or_else(usage)?)),
            "--out" => output = Some(PathBuf::from(args.next().ok_or_else(usage)?)),
            "--opt" => optimization = Some(parse_optimization(&args.next().ok_or_else(usage)?)?),
            "--mode" => mode = parse_execution_mode(&args.next().ok_or_else(usage)?)?,
            "--profile-frames" => {
                profile.frames = args
                    .next()
                    .ok_or_else(usage)?
                    .parse()
                    .map_err(|_| "--profile-frames must be an integer".to_owned())?;
            }
            "--profile-input" => {
                profile
                    .inputs
                    .push(compat::parse_input(&args.next().ok_or_else(usage)?)?);
            }
            "--no-static-branches" => profile.follow_direct_branches = false,
            "--no-ram-code" => profile.ram_code = false,
            "--embed-rom" => profile.embed_rom = true,
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
    println!("execution_mode: {}", mode.as_str());
    require_available_mode(mode)?;
    if mode == ExecutionMode::AotCompatible {
        let report = compat::build_package(&rom_path, &output, optimization, &profile)?;
        println!("package: {}", report.path.display());
        print!("{}", report.manifest);
        return Ok(());
    }
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

/// `bundle` (SPEC-066): an `aot-compatible` package whose executable embeds
/// the ROM. Defaults: O2 and a 3000-frame profile; all `compile` flags work.
fn run_bundle(arguments: Vec<String>) -> Result<(), String> {
    let mut args = vec![
        "--mode".to_owned(),
        "aot-compatible".to_owned(),
        "--embed-rom".to_owned(),
    ];
    if !arguments.iter().any(|a| a == "--opt") {
        args.extend(["--opt".to_owned(), "O2".to_owned()]);
    }
    if !arguments.iter().any(|a| a == "--profile-frames") {
        args.extend(["--profile-frames".to_owned(), "3000".to_owned()]);
    }
    if arguments.iter().any(|a| a == "--mode") {
        return Err("bundle always builds aot-compatible packages; remove --mode".to_owned());
    }
    args.extend(arguments);
    println!(
        "bundle: the ROM is embedded in the executable; the package contains the game itself and must not be redistributed"
    );
    run_compile(args)
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
    let lld_threads = parse_lld_threads(env::var("GBA_RECOMP_LINK_THREADS").ok().as_deref())?;
    let mut command = Command::new(&clang);
    command
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-fuse-ld=lld"])
        .arg(format!("-Wl,--threads={lld_threads}"))
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

fn parse_lld_threads(value: Option<&str>) -> Result<u8, String> {
    let Some(value) = value else {
        return Ok(10);
    };
    let threads = value
        .parse::<u8>()
        .map_err(|_| "GBA_RECOMP_LINK_THREADS must be an integer from 1 to 10".to_owned())?;
    if !(1..=10).contains(&threads) {
        return Err("GBA_RECOMP_LINK_THREADS must be from 1 to 10".to_owned());
    }
    Ok(threads)
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
#include <stdlib.h>
#include <string.h>
#include "gba_abi_v4.h"

static uint32_t read_le32(const uint8_t *bytes) {{
    return (uint32_t)bytes[0] | ((uint32_t)bytes[1] << 8) |
           ((uint32_t)bytes[2] << 16) | ((uint32_t)bytes[3] << 24);
}}

static uint16_t read_le16(const uint8_t *bytes) {{
    return (uint16_t)bytes[0] | ((uint16_t)bytes[1] << 8);
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
            fprintf(stderr, "guest stopped: reason=%u pc=0x%08X address=0x%08X fault_code=%u r0=%u r1=%u cpsr=0x%08X guest_cycle=%" PRIu64 "\n", reason, runtime.base.cpu.r[15], exit.fault_address, exit.fault_code, runtime.base.cpu.r[0], runtime.base.cpu.r[1], runtime.base.cpu.cpsr, runtime.base.cpu.guest_cycle);
            return 4;
        }}
    }}
    printf("{{\"status\":\"budget_exhausted\",\"steps\":%" PRIu64 ",\"r0\":%u,\"r1\":%u,\"pc\":\"0x%08X\",\"cpsr\":\"0x%08X\",\"guest_cycle\":%" PRIu64 ",\"ewram_word0\":%u,\"iwram_word0\":%u", executed, runtime.base.cpu.r[0], runtime.base.cpu.r[1], runtime.base.cpu.r[15], runtime.base.cpu.cpsr, runtime.base.cpu.guest_cycle, read_le32(machine.memory.ewram), read_le32(machine.memory.iwram));
    if (getenv("GBA_RECOMP_DUMP_IO") != NULL) {{
        printf(",\"io_ie\":%u,\"io_if\":%u,\"io_ime\":%u",
               read_le16(&machine.io[0x200]), read_le16(&machine.io[0x202]),
               read_le16(&machine.io[0x208]));
    }}
    puts("}}");
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
        "{{\n  \"format_version\": 1,\n  \"rom_sha256\": \"{}\",\n  \"rom_size\": {},\n  \"game_code\": \"{}\",\n  \"rom_header_version\": {},\n  \"entrypoint\": \"0x{:08X}\",\n  \"ir_version\": {},\n  \"abi_version\": {},\n  \"machine_state_version\": {},\n  \"bios_hle_version\": {},\n  \"llvm_version\": \"{}\",\n  \"target_triple\": \"{}\",\n  \"optimization\": \"{}\",\n  \"execution_mode\": \"{}\",\n  \"timing_model\": \"one abstract unit per instruction\",\n  \"interrupt_model\": \"none\",\n  \"io_initialization\": \"all-zero\",\n  \"instructions_aot\": {},\n  \"run_instruction_budget\": {}\n}}\n",
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
        ExecutionMode::StrictAot.as_str(),
        instruction_count(program),
        step_limit,
    )
}

fn instruction_count(program: &ArmProgram) -> usize {
    program.instructions.len() + program.thumb_instructions.len()
}

/// Package execution mode (SPEC-055). `strict-aot` is the default and never
/// calls an interpreter or JIT; `aot-compatible` must be selected explicitly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExecutionMode {
    StrictAot,
    AotCompatible,
}

impl ExecutionMode {
    fn as_str(self) -> &'static str {
        match self {
            ExecutionMode::StrictAot => "strict-aot",
            ExecutionMode::AotCompatible => "aot-compatible",
        }
    }
}

fn parse_execution_mode(value: &str) -> Result<ExecutionMode, String> {
    match value {
        "strict-aot" => Ok(ExecutionMode::StrictAot),
        "aot-compatible" => Ok(ExecutionMode::AotCompatible),
        _ => Err(format!(
            "unsupported execution mode `{value}`; use strict-aot or aot-compatible"
        )),
    }
}

/// IAC-02/IAC-09: `aot-compatible` links the `gba-core` interpreter through
/// the `gba-native-rt` runtime (SPEC-062). Without that library the mode is
/// rejected before any work instead of degrading to `strict-aot`.
fn require_available_mode(mode: ExecutionMode) -> Result<(), String> {
    match mode {
        ExecutionMode::StrictAot => Ok(()),
        ExecutionMode::AotCompatible => compat::runtime_library()
            .map(|_| ())
            .map_err(|e| format!("execution mode `aot-compatible` is not available: {e}; the mode was not changed to strict-aot")),
    }
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
        "usage:\n  gba-recomp inspect-rom --rom <path.gba>\n  gba-recomp compile --rom <path.gba> --out <package-dir> --opt O0|O2 [--mode strict-aot|aot-compatible] [--steps N]\n       [--profile-frames N] [--profile-input FRAME:LEN:KEYS]... [--no-static-branches] [--no-ram-code] [--embed-rom]  (aot-compatible)\n  gba-recomp bundle --rom <path.gba> --out <package-dir> [--opt O0|O2] [--profile-frames N] [--profile-input ...]  (ROM embedded)\n  gba-recomp emit-fixture --out <object.o> --opt O0|O2\n       native target: {}",
        target.triple
    )
}

#[cfg(test)]
mod execution_mode_tests {
    use super::{ExecutionMode, parse_execution_mode, require_available_mode, run_compile};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn execution_mode_parses_only_contracted_values() {
        assert_eq!(
            parse_execution_mode("strict-aot").unwrap(),
            ExecutionMode::StrictAot
        );
        assert_eq!(
            parse_execution_mode("aot-compatible").unwrap(),
            ExecutionMode::AotCompatible
        );
        assert_eq!(ExecutionMode::StrictAot.as_str(), "strict-aot");
        assert_eq!(ExecutionMode::AotCompatible.as_str(), "aot-compatible");
        for rejected in ["", "strict", "STRICT-AOT", "jit", "interpreter", "aot"] {
            assert!(parse_execution_mode(rejected).is_err(), "{rejected:?}");
        }
    }

    #[test]
    fn compatible_mode_without_runtime_library_is_rejected_not_downgraded() {
        assert!(require_available_mode(ExecutionMode::StrictAot).is_ok());
        // SAFETY of test env mutation: this test only reads its own variable.
        unsafe { std::env::set_var("GBA_NATIVE_RT_LIB", "/nonexistent/libgba_native_rt.a") };
        let error = require_available_mode(ExecutionMode::AotCompatible).unwrap_err();
        unsafe { std::env::remove_var("GBA_NATIVE_RT_LIB") };
        assert!(error.contains("aot-compatible"), "{error}");
        assert!(error.contains("not changed to strict-aot"), "{error}");
    }

    #[test]
    fn compile_rejects_compatible_mode_before_reading_rom_or_writing_package() {
        let output = std::env::temp_dir().join(format!(
            "gba-recomp-compatible-rejected-{}",
            std::process::id()
        ));
        let error = run_compile(args(&[
            "--rom",
            "/nonexistent/rom.gba",
            "--out",
            output.to_str().unwrap(),
            "--opt",
            "O0",
            "--mode",
            "aot-compatible",
        ]))
        .unwrap_err();
        // Either the runtime library is missing or the ROM cannot be read;
        // in both cases nothing is published.
        assert!(
            error.contains("aot-compatible") || error.contains("cannot read ROM"),
            "{error}"
        );
        assert!(!output.exists());

        let error = run_compile(args(&[
            "--rom", "x.gba", "--out", "y", "--opt", "O0", "--mode", "jit",
        ]))
        .unwrap_err();
        assert!(error.contains("unsupported execution mode"), "{error}");
    }
}

#[cfg(test)]
mod link_thread_tests {
    use super::{Optimization, build_native_package, parse_lld_threads};
    use crate::rom::{ROM_BASE, RomImage, header_checksum};
    use gba_ir::{
        ArmInstruction, ArmOperation, ArmProgram, GBA_IR_VERSION, ThumbInstruction,
        ThumbLdmiaRomCase, ThumbLdmiaRomTable, ThumbOperation,
    };
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_image() -> RomImage {
        let mut bytes = vec![0; 256];
        bytes[0..4].copy_from_slice(&0xEAFF_FFFE_u32.to_le_bytes());
        bytes[0xA0..0xAC].copy_from_slice(b"AOT TEST ROM");
        bytes[0xAC..0xB0].copy_from_slice(b"TEST");
        bytes[0xB0..0xB2].copy_from_slice(b"00");
        bytes[0xB2] = 0x96;
        bytes[0xBD] = header_checksum(&bytes);
        RomImage::from_bytes(bytes).unwrap()
    }

    fn run_program(program: &ArmProgram, label: &str, expected: &str) {
        let image = test_image();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let output_path =
            std::env::temp_dir().join(format!("gba-recomp-{label}-{}-{stamp}", std::process::id()));
        let package =
            build_native_package(&image, program, Optimization::O0, 8, &output_path).unwrap();
        let output = Command::new(package.join("game")).output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        std::fs::remove_dir_all(&package).unwrap();
        assert!(!output.status.success(), "guest unexpectedly completed");
        assert!(
            stderr.contains(expected),
            "expected {expected:?} in stderr, got {stderr:?}"
        );
    }

    #[test]
    fn lld_thread_count_defaults_and_enforces_host_budget() {
        assert_eq!(parse_lld_threads(None).unwrap(), 10);
        assert_eq!(parse_lld_threads(Some("1")).unwrap(), 1);
        assert_eq!(parse_lld_threads(Some("4")).unwrap(), 4);
        assert_eq!(parse_lld_threads(Some("10")).unwrap(), 10);
        assert!(parse_lld_threads(Some("0")).is_err());
        assert!(parse_lld_threads(Some("11")).is_err());
        assert!(parse_lld_threads(Some("fast")).is_err());
    }

    #[test]
    fn arm_branch_guard_rejects_unmarked_edge_even_when_pc_has_another_predecessor() {
        let program = ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![
                ArmInstruction {
                    guest_pc: ROM_BASE,
                    encoding: 0x0A00_0002,
                    condition: 0,
                    operation: ArmOperation::Branch {
                        target: ROM_BASE + 0x10,
                        link: false,
                        reachable_successors: 0b01,
                    },
                },
                ArmInstruction {
                    guest_pc: ROM_BASE + 4,
                    encoding: 0xEAFF_FFFE,
                    condition: 14,
                    operation: ArmOperation::Branch {
                        target: ROM_BASE + 4,
                        link: false,
                        reachable_successors: 0b01,
                    },
                },
                ArmInstruction {
                    guest_pc: ROM_BASE + 0x10,
                    encoding: 0xEAFF_FFFB,
                    condition: 14,
                    operation: ArmOperation::Branch {
                        target: ROM_BASE + 4,
                        link: false,
                        reachable_successors: 0b01,
                    },
                },
            ],
            thumb_instructions: vec![],
            thumb_ldmia_rom_tables: vec![],
            branch_targets: vec![],
        };

        run_program(
            &program,
            "arm-branch-edge-guard",
            "guest stopped: reason=3 pc=0x08000000",
        );
    }

    #[test]
    fn thumb_ldmia_table_miss_faults_at_guest_base_without_fallback() {
        let table = ThumbLdmiaRomTable {
            cases: vec![
                ThumbLdmiaRomCase {
                    base_address: ROM_BASE + 0xF0,
                    values_by_register: [0x1234_5678, 0, 0, 0, 0, 0, 0, 0],
                },
                ThumbLdmiaRomCase {
                    base_address: ROM_BASE + 0xF4,
                    values_by_register: [0x8765_4321, 0, 0, 0, 0, 0, 0, 0],
                },
            ],
        };
        let program = ArmProgram {
            version: GBA_IR_VERSION,
            instructions: vec![
                ArmInstruction {
                    guest_pc: ROM_BASE,
                    encoding: 0xE59F_3018,
                    condition: 14,
                    operation: ArmOperation::LoadRomLiteral {
                        rd: 3,
                        guest_address: ROM_BASE + 0x20,
                        value: ROM_BASE + 0x100,
                    },
                },
                ArmInstruction {
                    guest_pc: ROM_BASE + 4,
                    encoding: 0xE59F_2018,
                    condition: 14,
                    operation: ArmOperation::LoadRomLiteral {
                        rd: 2,
                        guest_address: ROM_BASE + 0x24,
                        value: ROM_BASE + 0x85,
                    },
                },
                ArmInstruction {
                    guest_pc: ROM_BASE + 8,
                    encoding: 0xE12F_FF12,
                    condition: 14,
                    operation: ArmOperation::BranchExchange { rm: 2 },
                },
                ArmInstruction {
                    guest_pc: ROM_BASE + 0x0C,
                    encoding: 0xEAFF_FFFE,
                    condition: 14,
                    operation: ArmOperation::Branch {
                        target: ROM_BASE + 0x0C,
                        link: false,
                        reachable_successors: 0b01,
                    },
                },
            ],
            thumb_instructions: vec![
                ThumbInstruction {
                    guest_pc: ROM_BASE + 0x84,
                    encoding: 0xCB01,
                    operation: ThumbOperation::LoadRomMultipleIncrementAfterCases {
                        base_register: 3,
                        register_list: 1,
                        table_id: 0,
                    },
                },
                ThumbInstruction {
                    guest_pc: ROM_BASE + 0x86,
                    encoding: 0x4710,
                    operation: ThumbOperation::BranchExchange { rm: 2 },
                },
            ],
            thumb_ldmia_rom_tables: vec![table],
            branch_targets: vec![
                gba_ir::StaticBranchTarget {
                    source_mode: gba_ir::IsaMode::Arm,
                    source_pc: ROM_BASE + 8,
                    target_mode: gba_ir::IsaMode::Thumb,
                    target_pc: ROM_BASE + 0x84,
                },
                gba_ir::StaticBranchTarget {
                    source_mode: gba_ir::IsaMode::Thumb,
                    source_pc: ROM_BASE + 0x86,
                    target_mode: gba_ir::IsaMode::Arm,
                    target_pc: ROM_BASE + 0x0C,
                },
            ],
        };

        run_program(
            &program,
            "thumb-ldmia-table-miss",
            "guest stopped: reason=4 pc=0x08000084 address=0x08000100 fault_code=7",
        );
    }
}
