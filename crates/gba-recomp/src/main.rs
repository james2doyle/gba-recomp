use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use gba_ir::alu_fixture;
use gba_llvm::{Optimization, emit_block, native_target};

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
    if command != "emit-fixture" {
        return Err(format!("unknown command `{command}`\n{}", usage()));
    }

    let mut output = None;
    let mut optimization = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--out" => {
                let value = args.next().ok_or_else(usage)?;
                output = Some(PathBuf::from(value));
            }
            "--opt" => {
                let value = args.next().ok_or_else(usage)?;
                optimization = Some(match value.as_str() {
                    "O0" => Optimization::O0,
                    "O2" => Optimization::O2,
                    _ => return Err(format!("unsupported optimization `{value}`; use O0 or O2")),
                });
            }
            _ => return Err(format!("unknown argument `{flag}`\n{}", usage())),
        }
    }

    let output = output.ok_or_else(usage)?;
    let optimization = optimization.ok_or_else(usage)?;
    let block = alu_fixture();
    block
        .verify()
        .map_err(|error| format!("GBA IR rejected: {error}"))?;
    let info = emit_block(&block, &output, optimization)?;
    println!("object: {}", output.display());
    println!("LLVM: {}", info.llvm_version);
    println!(
        "target: {} ({}, {})",
        info.triple, info.cpu, info.object_format
    );
    println!("optimization: {}", info.optimization.as_str());
    println!("DataLayout: {}", info.data_layout);
    Ok(())
}

fn usage() -> String {
    let target = native_target();
    format!(
        "usage: gba-recomp emit-fixture --out <object.o> --opt O0|O2\n       native target: {}",
        target.triple
    )
}
