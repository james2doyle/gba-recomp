use std::env;
use std::path::PathBuf;
use std::process::Command;

const EXPECTED_LLVM: &str = "23.1.3";
const LINK_COMPONENTS: &[&str] = &[
    "core",
    "analysis",
    "passes",
    "target",
    "x86codegen",
    "aarch64codegen",
];

fn main() {
    println!("cargo:rerun-if-env-changed=LLVM_CONFIG");
    println!("cargo:rerun-if-env-changed=LLVM_SYS_231_PREFIX");

    let binary = llvm_config_path();
    let version = llvm_config(&binary, &["--version"]);
    if version != EXPECTED_LLVM {
        panic!(
            "LLVM {EXPECTED_LLVM} is required by toolchain.lock.toml, found {version} at {}",
            binary.display()
        );
    }
    let shared_mode = llvm_config(&binary, &["--shared-mode"]);
    if shared_mode != "static" {
        panic!(
            "static LLVM libraries are required by toolchain.lock.toml, found {shared_mode} at {}",
            binary.display()
        );
    }
    println!("cargo:rustc-env=GBA_LLVM_VERSION={version}");

    let libdir = llvm_config(&binary, &["--libdir"]);
    println!("cargo:rustc-link-search=native={libdir}");

    let mut libnames_args = vec!["--libnames", "--link-static"];
    libnames_args.extend_from_slice(LINK_COMPONENTS);
    let libnames = llvm_config(&binary, &libnames_args);
    for filename in libnames.split_whitespace() {
        let library = filename
            .strip_prefix("lib")
            .and_then(|name| name.strip_suffix(".a"))
            .unwrap_or_else(|| panic!("unexpected static LLVM library name `{filename}`"));
        if !PathBuf::from(&libdir).join(filename).is_file() {
            panic!("LLVM component archive is missing: {libdir}/{filename}");
        }
        println!("cargo:rustc-link-lib=static={library}");
    }

    let mut system_libs_args = vec!["--system-libs", "--link-static"];
    system_libs_args.extend_from_slice(LINK_COMPONENTS);
    for flag in llvm_config(&binary, &system_libs_args).split_whitespace() {
        emit_system_library(flag);
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("Cargo sets CARGO_CFG_TARGET_OS");
    let cpp_runtime = match target_os.as_str() {
        "macos" => {
            println!("cargo:rustc-link-lib=static=c++");
            println!("cargo:rustc-link-lib=static=c++abi");
            println!("cargo:rustc-link-lib=static=unwind");
            return;
        }
        "linux" => "stdc++",
        _ => panic!("unsupported compiler host OS for LLVM C++ runtime: {target_os}"),
    };
    println!("cargo:rustc-link-lib=dylib={cpp_runtime}");
}

fn llvm_config(binary: &PathBuf, args: &[&str]) -> String {
    let output = Command::new(binary)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("cannot run {}: {error}", binary.display()));
    if !output.status.success() {
        panic!("{} {} failed", binary.display(), args.join(" "));
    }
    String::from_utf8(output.stdout)
        .unwrap_or_else(|_| panic!("{} returned non-UTF-8 output", binary.display()))
        .trim()
        .to_owned()
}

fn emit_system_library(flag: &str) {
    if let Some(name) = flag.strip_prefix("-l") {
        println!("cargo:rustc-link-lib=dylib={name}");
        return;
    }
    let path = PathBuf::from(flag);
    if !path.is_file() {
        panic!("LLVM system library is missing or unsupported: {flag}");
    }
    let parent = path
        .parent()
        .unwrap_or_else(|| panic!("LLVM system library path has no parent: {flag}"));
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_else(|| panic!("LLVM system library name is not UTF-8: {flag}"));
    let kind_and_name = filename
        .strip_prefix("lib")
        .and_then(|name| {
            name.strip_suffix(".a")
                .map(|stem| ("static", stem))
                .or_else(|| name.strip_suffix(".dylib").map(|stem| ("dylib", stem)))
                .or_else(|| name.strip_suffix(".so").map(|stem| ("dylib", stem)))
        })
        .unwrap_or_else(|| panic!("unrecognized LLVM system library filename: {filename}"));
    println!("cargo:rustc-link-search=native={}", parent.display());
    println!(
        "cargo:rustc-link-lib={}={}",
        kind_and_name.0, kind_and_name.1
    );
}

fn llvm_config_path() -> PathBuf {
    if let Some(path) = env::var_os("LLVM_CONFIG") {
        return PathBuf::from(path);
    }
    if let Some(prefix) = env::var_os("LLVM_SYS_231_PREFIX") {
        return PathBuf::from(prefix).join("bin/llvm-config");
    }
    PathBuf::from("llvm-config")
}
