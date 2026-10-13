#![deny(unsafe_op_in_unsafe_fn)]

use std::ffi::{CStr, CString};
use std::path::Path;
use std::ptr;

use gba_ir::GbaIrBlock;
use llvm_sys::LLVMIntPredicate;
use llvm_sys::analysis::{LLVMVerifierFailureAction, LLVMVerifyModule};
use llvm_sys::core::*;
use llvm_sys::error::{LLVMDisposeErrorMessage, LLVMGetErrorMessage};
use llvm_sys::prelude::*;
use llvm_sys::target::*;
use llvm_sys::target_machine::*;
use llvm_sys::transforms::pass_builder::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Optimization {
    O0,
    O2,
}

impl Optimization {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::O0 => "O0",
            Self::O2 => "O2",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct NativeTarget {
    pub triple: &'static str,
    pub cpu: &'static str,
    pub features: &'static str,
    pub object_format: &'static str,
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub const fn native_target() -> NativeTarget {
    NativeTarget {
        triple: "x86_64-unknown-linux-gnu",
        cpu: "x86-64",
        features: "",
        object_format: "ELF64",
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub const fn native_target() -> NativeTarget {
    NativeTarget {
        triple: "aarch64-apple-darwin",
        cpu: "generic",
        features: "",
        object_format: "Mach-O 64-bit",
    }
}

#[cfg(not(any(
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "macos", target_arch = "aarch64")
)))]
compile_error!("GBA-010 supports only Linux x86-64 and macOS arm64 native hosts");

pub struct EmitInfo {
    pub llvm_version: &'static str,
    pub triple: &'static str,
    pub cpu: &'static str,
    pub object_format: &'static str,
    pub data_layout: String,
    pub optimization: Optimization,
}

pub fn emit_block(
    block: &GbaIrBlock,
    output: &Path,
    optimization: Optimization,
) -> Result<EmitInfo, String> {
    block.verify().map_err(|error| error.to_string())?;
    if output.exists() {
        std::fs::remove_file(output)
            .map_err(|error| format!("cannot replace {}: {error}", output.display()))?;
    }
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;

    // All LLVM handles are created, used, and disposed within this call. The only
    // guest addresses in the module are integer register values, never host pointers.
    unsafe { emit_block_llvm(block, output, optimization) }
}

unsafe fn emit_block_llvm(
    block: &GbaIrBlock,
    output: &Path,
    optimization: Optimization,
) -> Result<EmitInfo, String> {
    let target = native_target();
    let triple = CString::new(target.triple).expect("static triple contains no NUL");
    let cpu = CString::new(target.cpu).expect("static CPU contains no NUL");
    let features = CString::new(target.features).expect("static features contain no NUL");
    let target_name = triple.to_string_lossy();
    let target_prefix = if target.triple.starts_with("x86_64-") {
        "x86"
    } else {
        "aarch64"
    };
    unsafe { initialize_backend(target_prefix) }?;
    let codegen_level = match optimization {
        Optimization::O0 => LLVMCodeGenOptLevel::LLVMCodeGenLevelNone,
        Optimization::O2 => LLVMCodeGenOptLevel::LLVMCodeGenLevelDefault,
    };

    let mut llvm_target = ptr::null_mut();
    let mut error_message = ptr::null_mut();
    // SAFETY: all strings are NUL-terminated and output pointers are valid.
    let target_error =
        unsafe { LLVMGetTargetFromTriple(triple.as_ptr(), &mut llvm_target, &mut error_message) };
    if target_error != 0 {
        return Err(unsafe { take_llvm_message(error_message) });
    }

    // SAFETY: target handle came from LLVMGetTargetFromTriple and the C strings
    // remain live for the duration of this call.
    let machine = unsafe {
        LLVMCreateTargetMachine(
            llvm_target,
            triple.as_ptr(),
            cpu.as_ptr(),
            features.as_ptr(),
            codegen_level,
            LLVMRelocMode::LLVMRelocPIC,
            LLVMCodeModel::LLVMCodeModelSmall,
        )
    };
    if machine.is_null() {
        return Err(format!(
            "LLVM could not create TargetMachine for {target_name}"
        ));
    }

    let context = unsafe { LLVMContextCreate() };
    if context.is_null() {
        unsafe { LLVMDisposeTargetMachine(machine) };
        return Err("LLVMContextCreate returned null".to_owned());
    }
    let module_name = CString::new("gba_alu_fixture").expect("static name contains no NUL");
    let module = unsafe { LLVMModuleCreateWithNameInContext(module_name.as_ptr(), context) };
    if module.is_null() {
        unsafe {
            LLVMContextDispose(context);
            LLVMDisposeTargetMachine(machine);
        }
        return Err("LLVMModuleCreateWithNameInContext returned null".to_owned());
    }
    let builder = unsafe { LLVMCreateBuilderInContext(context) };
    if builder.is_null() {
        unsafe {
            LLVMDisposeModule(module);
            LLVMContextDispose(context);
            LLVMDisposeTargetMachine(machine);
        }
        return Err("LLVMCreateBuilderInContext returned null".to_owned());
    }

    let target_data = unsafe { LLVMCreateTargetDataLayout(machine) };
    if target_data.is_null() {
        unsafe {
            LLVMDisposeBuilder(builder);
            LLVMDisposeModule(module);
            LLVMContextDispose(context);
            LLVMDisposeTargetMachine(machine);
        }
        return Err("LLVMCreateTargetDataLayout returned null".to_owned());
    }
    let layout_raw = unsafe { LLVMCopyStringRepOfTargetData(target_data) };
    if layout_raw.is_null() {
        unsafe {
            LLVMDisposeTargetData(target_data);
            LLVMDisposeBuilder(builder);
            LLVMDisposeModule(module);
            LLVMContextDispose(context);
            LLVMDisposeTargetMachine(machine);
        }
        return Err("LLVMCopyStringRepOfTargetData returned null".to_owned());
    }
    let data_layout = unsafe { CStr::from_ptr(layout_raw) }
        .to_string_lossy()
        .into_owned();
    let layout = CString::new(data_layout.as_str()).expect("LLVM DataLayout contains no NUL");
    unsafe {
        LLVMSetTarget(module, triple.as_ptr());
        LLVMSetDataLayout(module, layout.as_ptr());
        LLVMDisposeMessage(layout_raw);
    }

    let result = unsafe {
        build_alu_function(
            context,
            module,
            builder,
            target_data,
            &block.instructions[0],
        )
        .and_then(|()| verify_module(module, "before optimization"))
        .and_then(|()| run_optimization(module, machine, optimization))
        .and_then(|()| verify_module(module, "after optimization"))
        .and_then(|()| emit_object(machine, module, output))
    };

    unsafe {
        LLVMDisposeTargetData(target_data);
        LLVMDisposeBuilder(builder);
        LLVMDisposeModule(module);
        LLVMContextDispose(context);
        LLVMDisposeTargetMachine(machine);
    }

    result.map(|()| EmitInfo {
        llvm_version: env!("GBA_LLVM_VERSION"),
        triple: target.triple,
        cpu: target.cpu,
        object_format: target.object_format,
        data_layout,
        optimization,
    })
}

unsafe fn initialize_backend(target: &str) -> Result<(), String> {
    match target {
        "x86" => unsafe {
            LLVMInitializeX86TargetInfo();
            LLVMInitializeX86Target();
            LLVMInitializeX86TargetMC();
            LLVMInitializeX86AsmPrinter();
        },
        "aarch64" => unsafe {
            LLVMInitializeAArch64TargetInfo();
            LLVMInitializeAArch64Target();
            LLVMInitializeAArch64TargetMC();
            LLVMInitializeAArch64AsmPrinter();
        },
        _ => return Err(format!("unsupported LLVM backend {target}")),
    }
    Ok(())
}

unsafe fn build_alu_function(
    context: LLVMContextRef,
    module: LLVMModuleRef,
    builder: LLVMBuilderRef,
    target_data: LLVMTargetDataRef,
    add: &gba_ir::AddRegister,
) -> Result<(), String> {
    let i32_type = unsafe { LLVMInt32TypeInContext(context) };
    let i64_type = unsafe { LLVMInt64TypeInContext(context) };
    let regs_type = unsafe { LLVMArrayType2(i32_type, 16) };
    let mut runtime_fields = [regs_type, i32_type, i32_type, i64_type];
    let runtime_type =
        unsafe { LLVMStructTypeInContext(context, runtime_fields.as_mut_ptr(), 4, 0) };
    let mut exit_fields = [i32_type, i32_type];
    let exit_type = unsafe { LLVMStructTypeInContext(context, exit_fields.as_mut_ptr(), 2, 0) };
    unsafe { validate_abi_layout(target_data, runtime_type, exit_type) }?;
    let mut parameters = unsafe {
        [
            LLVMPointerType(runtime_type, 0),
            LLVMPointerType(exit_type, 0),
        ]
    };
    let function_type = unsafe { LLVMFunctionType(i32_type, parameters.as_mut_ptr(), 2, 0) };
    let function_name = CString::new("gba_block_v1").expect("static name contains no NUL");
    let function = unsafe { LLVMAddFunction(module, function_name.as_ptr(), function_type) };
    if function.is_null() {
        return Err("LLVMAddFunction returned null".to_owned());
    }
    let runtime = unsafe { LLVMGetParam(function, 0) };
    let exit = unsafe { LLVMGetParam(function, 1) };
    let entry_name = CString::new("entry").expect("static name contains no NUL");
    let overflow_name = CString::new("cycle_overflow").expect("static name contains no NUL");
    let commit_name = CString::new("commit_add").expect("static name contains no NUL");
    let entry = unsafe { LLVMAppendBasicBlockInContext(context, function, entry_name.as_ptr()) };
    let overflow =
        unsafe { LLVMAppendBasicBlockInContext(context, function, overflow_name.as_ptr()) };
    let commit = unsafe { LLVMAppendBasicBlockInContext(context, function, commit_name.as_ptr()) };
    unsafe { LLVMPositionBuilderAtEnd(builder, entry) };

    let cycles_ptr =
        unsafe { LLVMBuildStructGEP2(builder, runtime_type, runtime, 3, c"cycles".as_ptr()) };
    let cycles = unsafe { LLVMBuildLoad2(builder, i64_type, cycles_ptr, c"cycle".as_ptr()) };
    let cycle_max = unsafe { LLVMConstInt(i64_type, u64::MAX, 0) };
    let is_cycle_overflow = unsafe {
        LLVMBuildICmp(
            builder,
            LLVMIntPredicate::LLVMIntEQ,
            cycles,
            cycle_max,
            c"cycle_overflowed".as_ptr(),
        )
    };
    let abi_version_ptr = unsafe {
        LLVMBuildStructGEP2(
            builder,
            runtime_type,
            runtime,
            2,
            c"abi_version_ptr".as_ptr(),
        )
    };
    let abi_version =
        unsafe { LLVMBuildLoad2(builder, i32_type, abi_version_ptr, c"abi_version".as_ptr()) };
    let abi_mismatch = unsafe {
        LLVMBuildICmp(
            builder,
            LLVMIntPredicate::LLVMIntNE,
            abi_version,
            LLVMConstInt(i32_type, 1, 0),
            c"abi_mismatch".as_ptr(),
        )
    };
    let must_fault = unsafe {
        LLVMBuildOr(
            builder,
            is_cycle_overflow,
            abi_mismatch,
            c"must_fault".as_ptr(),
        )
    };
    unsafe { LLVMBuildCondBr(builder, must_fault, overflow, commit) };

    unsafe { LLVMPositionBuilderAtEnd(builder, overflow) };
    let pc_ptr_on_fault = unsafe { register_pointer(builder, context, runtime_type, runtime, 15) };
    let pc_on_fault =
        unsafe { LLVMBuildLoad2(builder, i32_type, pc_ptr_on_fault, c"pc_fault".as_ptr()) };
    unsafe {
        store_exit(builder, exit_type, exit, i32_type, 4, pc_on_fault);
        LLVMBuildRet(builder, LLVMConstInt(i32_type, 4, 0));
    }

    unsafe { LLVMPositionBuilderAtEnd(builder, commit) };
    let lhs_ptr = unsafe { register_pointer(builder, context, runtime_type, runtime, add.rn) };
    let rhs_ptr = unsafe { register_pointer(builder, context, runtime_type, runtime, add.rm) };
    let dst_ptr = unsafe { register_pointer(builder, context, runtime_type, runtime, add.rd) };
    let lhs = unsafe { LLVMBuildLoad2(builder, i32_type, lhs_ptr, c"lhs".as_ptr()) };
    let rhs = unsafe { LLVMBuildLoad2(builder, i32_type, rhs_ptr, c"rhs".as_ptr()) };
    let sum = unsafe { LLVMBuildAdd(builder, lhs, rhs, c"sum".as_ptr()) };
    unsafe { LLVMBuildStore(builder, sum, dst_ptr) };

    let pc_ptr = unsafe { register_pointer(builder, context, runtime_type, runtime, 15) };
    let pc = unsafe { LLVMBuildLoad2(builder, i32_type, pc_ptr, c"pc".as_ptr()) };
    let next_pc = unsafe {
        LLVMBuildAdd(
            builder,
            pc,
            LLVMConstInt(i32_type, 4, 0),
            c"next_pc".as_ptr(),
        )
    };
    unsafe { LLVMBuildStore(builder, next_pc, pc_ptr) };

    let next_cycles = unsafe {
        LLVMBuildAdd(
            builder,
            cycles,
            LLVMConstInt(i64_type, 1, 0),
            c"next_cycle".as_ptr(),
        )
    };
    unsafe { LLVMBuildStore(builder, next_cycles, cycles_ptr) };
    unsafe {
        store_exit(builder, exit_type, exit, i32_type, 0, next_pc);
        LLVMBuildRet(builder, LLVMConstInt(i32_type, 0, 0));
    }
    Ok(())
}

unsafe fn validate_abi_layout(
    target_data: LLVMTargetDataRef,
    runtime_type: LLVMTypeRef,
    exit_type: LLVMTypeRef,
) -> Result<(), String> {
    let runtime_size = unsafe { LLVMABISizeOfType(target_data, runtime_type) };
    let runtime_alignment = unsafe { LLVMABIAlignmentOfType(target_data, runtime_type) };
    let runtime_offsets = [
        unsafe { LLVMOffsetOfElement(target_data, runtime_type, 0) },
        unsafe { LLVMOffsetOfElement(target_data, runtime_type, 1) },
        unsafe { LLVMOffsetOfElement(target_data, runtime_type, 2) },
        unsafe { LLVMOffsetOfElement(target_data, runtime_type, 3) },
    ];
    let exit_size = unsafe { LLVMABISizeOfType(target_data, exit_type) };
    let exit_alignment = unsafe { LLVMABIAlignmentOfType(target_data, exit_type) };
    let exit_offsets = [
        unsafe { LLVMOffsetOfElement(target_data, exit_type, 0) },
        unsafe { LLVMOffsetOfElement(target_data, exit_type, 1) },
    ];
    if runtime_size != 80
        || runtime_alignment != 8
        || runtime_offsets != [0, 64, 68, 72]
        || exit_size != 8
        || exit_alignment != 4
        || exit_offsets != [0, 4]
    {
        return Err(format!(
            "LLVM target ABI layout does not match gba_abi_v1: runtime size/alignment/offsets={runtime_size}/{runtime_alignment}/{runtime_offsets:?}, exit={exit_size}/{exit_alignment}/{exit_offsets:?}"
        ));
    }
    Ok(())
}

unsafe fn register_pointer(
    builder: LLVMBuilderRef,
    context: LLVMContextRef,
    runtime_type: LLVMTypeRef,
    runtime: LLVMValueRef,
    register: u8,
) -> LLVMValueRef {
    let i32_type = unsafe { LLVMInt32TypeInContext(context) };
    let regs_pointer =
        unsafe { LLVMBuildStructGEP2(builder, runtime_type, runtime, 0, c"regs".as_ptr()) };
    let regs_type = unsafe { LLVMArrayType2(i32_type, 16) };
    let mut indices = unsafe {
        [
            LLVMConstInt(i32_type, 0, 0),
            LLVMConstInt(i32_type, register.into(), 0),
        ]
    };
    unsafe {
        LLVMBuildGEP2(
            builder,
            regs_type,
            regs_pointer,
            indices.as_mut_ptr(),
            2,
            c"reg_ptr".as_ptr(),
        )
    }
}

unsafe fn store_exit(
    builder: LLVMBuilderRef,
    exit_type: LLVMTypeRef,
    exit: LLVMValueRef,
    i32_type: LLVMTypeRef,
    reason: u64,
    next_pc: LLVMValueRef,
) {
    let reason_ptr =
        unsafe { LLVMBuildStructGEP2(builder, exit_type, exit, 0, c"reason_ptr".as_ptr()) };
    let pc_ptr =
        unsafe { LLVMBuildStructGEP2(builder, exit_type, exit, 1, c"exit_pc_ptr".as_ptr()) };
    unsafe {
        LLVMBuildStore(builder, LLVMConstInt(i32_type, reason, 0), reason_ptr);
        LLVMBuildStore(builder, next_pc, pc_ptr);
    }
}

unsafe fn verify_module(module: LLVMModuleRef, phase: &str) -> Result<(), String> {
    let mut message = ptr::null_mut();
    let invalid = unsafe {
        LLVMVerifyModule(
            module,
            LLVMVerifierFailureAction::LLVMReturnStatusAction,
            &mut message,
        )
    };
    if invalid != 0 {
        let detail = unsafe { take_llvm_message(message) };
        return Err(format!("LLVM IR verification failed {phase}: {detail}"));
    }
    if !message.is_null() {
        unsafe { LLVMDisposeMessage(message) };
    }
    Ok(())
}

unsafe fn run_optimization(
    module: LLVMModuleRef,
    machine: LLVMTargetMachineRef,
    optimization: Optimization,
) -> Result<(), String> {
    if optimization == Optimization::O0 {
        return Ok(());
    }
    let options = unsafe { LLVMCreatePassBuilderOptions() };
    if options.is_null() {
        return Err("LLVMCreatePassBuilderOptions returned null".to_owned());
    }
    unsafe { LLVMPassBuilderOptionsSetVerifyEach(options, 1) };
    let error = unsafe { LLVMRunPasses(module, c"default<O2>".as_ptr(), machine, options) };
    unsafe { LLVMDisposePassBuilderOptions(options) };
    if !error.is_null() {
        let message = unsafe { LLVMGetErrorMessage(error) };
        let detail = if message.is_null() {
            "LLVM optimization pipeline failed without a diagnostic".to_owned()
        } else {
            let detail = unsafe { CStr::from_ptr(message) }
                .to_string_lossy()
                .into_owned();
            unsafe { LLVMDisposeErrorMessage(message) };
            detail
        };
        return Err(detail);
    }
    Ok(())
}

unsafe fn emit_object(
    machine: LLVMTargetMachineRef,
    module: LLVMModuleRef,
    output: &Path,
) -> Result<(), String> {
    let path = CString::new(output.as_os_str().as_encoded_bytes())
        .map_err(|_| "object output path contains an embedded NUL".to_owned())?;
    let mut message = ptr::null_mut();
    let failed = unsafe {
        LLVMTargetMachineEmitToFile(
            machine,
            module,
            path.as_ptr() as *mut _,
            LLVMCodeGenFileType::LLVMObjectFile,
            &mut message,
        )
    };
    if failed != 0 {
        let detail = unsafe { take_llvm_message(message) };
        return Err(format!("LLVM object emission failed: {detail}"));
    }
    if !message.is_null() {
        unsafe { LLVMDisposeMessage(message) };
    }
    Ok(())
}

unsafe fn take_llvm_message(message: *mut std::ffi::c_char) -> String {
    if message.is_null() {
        return "LLVM returned no diagnostic".to_owned();
    }
    let result = unsafe { CStr::from_ptr(message) }
        .to_string_lossy()
        .into_owned();
    unsafe { LLVMDisposeMessage(message) };
    result
}
