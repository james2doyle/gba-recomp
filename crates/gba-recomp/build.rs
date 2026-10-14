use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=LLVM_CONFIG");
    println!("cargo:rerun-if-env-changed=LLVM_SYS_231_PREFIX");
    println!("cargo:rerun-if-env-changed=SDKROOT");
    println!("cargo:rerun-if-env-changed=GBA_RECOMP_CARGO_LINK_THREADS");

    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("Cargo sets CARGO_CFG_TARGET_OS");
    println!("cargo:rustc-link-arg=-fuse-ld=lld");
    println!(
        "cargo:rustc-link-arg=-Wl,--threads={}",
        cargo_link_threads()
    );

    match target_os.as_str() {
        "macos" => {
            let sysroot = env::var_os("SDKROOT")
                .map(PathBuf::from)
                .filter(|path| path.is_dir())
                .unwrap_or_else(macos_sdk_path);
            println!("cargo:rustc-link-arg=-isysroot");
            println!("cargo:rustc-link-arg={}", sysroot.display());
            println!("cargo:rustc-link-arg=-mmacosx-version-min=14.0.0");
        }
        "linux" => {}
        other => panic!("unsupported compiler host OS: {other}"),
    }
}

fn cargo_link_threads() -> u8 {
    let value = match env::var("GBA_RECOMP_CARGO_LINK_THREADS") {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => return 10,
        Err(error) => panic!("cannot read GBA_RECOMP_CARGO_LINK_THREADS: {error}"),
    };
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        panic!("GBA_RECOMP_CARGO_LINK_THREADS must be a decimal integer from 1 to 10");
    }
    let threads = value.parse::<u8>().unwrap_or_else(|_| {
        panic!("GBA_RECOMP_CARGO_LINK_THREADS must be a decimal integer from 1 to 10")
    });
    if !(1..=10).contains(&threads) {
        panic!("GBA_RECOMP_CARGO_LINK_THREADS must be from 1 to 10");
    }
    threads
}

fn macos_sdk_path() -> PathBuf {
    let output = Command::new("xcrun")
        .args(["--sdk", "macosx", "--show-sdk-path"])
        .output()
        .unwrap_or_else(|error| panic!("cannot query the macOS SDK with xcrun: {error}"));
    if !output.status.success() {
        panic!("xcrun --sdk macosx --show-sdk-path failed");
    }
    let path = String::from_utf8(output.stdout)
        .expect("xcrun returned non-UTF-8 SDK path")
        .trim()
        .to_owned();
    let path = PathBuf::from(path);
    if !path.is_dir() {
        panic!("macOS SDK does not exist: {}", path.display());
    }
    path
}
