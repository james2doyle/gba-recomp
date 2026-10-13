#![deny(unsafe_op_in_unsafe_fn)]

use std::ffi::{CStr, CString};
use std::path::Path;
use std::ptr;

use gba_ir::{
    ArmInstruction, ArmOperation, ArmProgram, GbaIrBlock, ThumbInstruction, ThumbOperation,
};
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

pub fn emit_arm_program(
    program: &ArmProgram,
    output: &Path,
    optimization: Optimization,
) -> Result<EmitInfo, String> {
    program.verify().map_err(|error| error.to_string())?;
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
    unsafe { emit_arm_program_llvm(program, output, optimization) }
}

unsafe fn emit_arm_program_llvm(
    program: &ArmProgram,
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
    let target_error =
        unsafe { LLVMGetTargetFromTriple(triple.as_ptr(), &mut llvm_target, &mut error_message) };
    if target_error != 0 {
        return Err(unsafe { take_llvm_message(error_message) });
    }
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
    let module_name = CString::new("gba_aot_program").expect("static name contains no NUL");
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

    let i32_type = unsafe { LLVMInt32TypeInContext(context) };
    let i64_type = unsafe { LLVMInt64TypeInContext(context) };
    let regs_type = unsafe { LLVMArrayType2(i32_type, 16) };
    let bank5_type = unsafe { LLVMArrayType2(i32_type, 5) };
    let bank6x2_type = unsafe { LLVMArrayType2(LLVMArrayType2(i32_type, 2), 6) };
    let mut runtime_fields = [
        regs_type,
        i32_type,
        i32_type,
        i64_type,
        bank5_type,
        bank5_type,
        bank6x2_type,
        bank5_type,
    ];
    let runtime_cpu_type =
        unsafe { LLVMStructTypeInContext(context, runtime_fields.as_mut_ptr(), 8, 0) };
    let mut runtime_v3_fields = [
        runtime_cpu_type,
        unsafe { LLVMPointerType(i32_type, 0) },
        i32_type,
    ];
    let runtime_v3_type =
        unsafe { LLVMStructTypeInContext(context, runtime_v3_fields.as_mut_ptr(), 3, 0) };
    let mut exit_v2_fields = [i32_type, i32_type];
    let exit_v2_type =
        unsafe { LLVMStructTypeInContext(context, exit_v2_fields.as_mut_ptr(), 2, 0) };
    let mut exit_fields = [i32_type, i32_type, i32_type, i32_type];
    let exit_type = unsafe { LLVMStructTypeInContext(context, exit_fields.as_mut_ptr(), 4, 0) };
    let result = unsafe {
        validate_abi_layout_v2(target_data, runtime_cpu_type, exit_v2_type)
            .and_then(|()| validate_abi_layout_v3(target_data, runtime_v3_type, exit_type))
            .and_then(|()| {
                build_arm_program_function(
                    context,
                    module,
                    builder,
                    runtime_cpu_type,
                    runtime_v3_type,
                    exit_type,
                    program,
                )
            })
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

unsafe fn build_arm_program_function(
    context: LLVMContextRef,
    module: LLVMModuleRef,
    builder: LLVMBuilderRef,
    runtime_cpu_type: LLVMTypeRef,
    runtime_v3_type: LLVMTypeRef,
    exit_type: LLVMTypeRef,
    program: &ArmProgram,
) -> Result<(), String> {
    let i32_type = unsafe { LLVMInt32TypeInContext(context) };
    let i64_type = unsafe { LLVMInt64TypeInContext(context) };
    let mut parameters = unsafe {
        [
            LLVMPointerType(runtime_v3_type, 0),
            LLVMPointerType(exit_type, 0),
        ]
    };
    let function_type = unsafe { LLVMFunctionType(i32_type, parameters.as_mut_ptr(), 2, 0) };
    let function_name = CString::new("gba_block_v3").expect("static name contains no NUL");
    let function = unsafe { LLVMAddFunction(module, function_name.as_ptr(), function_type) };
    if function.is_null() {
        return Err("LLVMAddFunction returned null".to_owned());
    }
    let runtime = unsafe { LLVMGetParam(function, 0) };
    let exit = unsafe { LLVMGetParam(function, 1) };
    let mut helper_parameters =
        unsafe { [LLVMPointerType(runtime_cpu_type, 0), i32_type, i32_type] };
    let helper_type = unsafe {
        LLVMFunctionType(
            i32_type,
            helper_parameters.as_mut_ptr(),
            helper_parameters.len() as u32,
            0,
        )
    };
    let helper_name = CString::new("gba_msr_cpsr_v2").expect("static name contains no NUL");
    let msr_helper = unsafe { LLVMAddFunction(module, helper_name.as_ptr(), helper_type) };
    if msr_helper.is_null() {
        return Err("LLVMAddFunction for gba_msr_cpsr_v2 returned null".to_owned());
    }
    let mut store_parameters = unsafe {
        [
            LLVMPointerType(runtime_v3_type, 0),
            LLVMPointerType(exit_type, 0),
            i32_type,
            i32_type,
        ]
    };
    let store_helper_type = unsafe {
        LLVMFunctionType(
            i32_type,
            store_parameters.as_mut_ptr(),
            store_parameters.len() as u32,
            0,
        )
    };
    let store_helper_name = CString::new("gba_store32_v3").expect("static name contains no NUL");
    let store_helper =
        unsafe { LLVMAddFunction(module, store_helper_name.as_ptr(), store_helper_type) };
    if store_helper.is_null() {
        return Err("LLVMAddFunction for gba_store32_v3 returned null".to_owned());
    }
    let mut push_parameters = unsafe {
        [
            LLVMPointerType(runtime_v3_type, 0),
            LLVMPointerType(exit_type, 0),
            i32_type,
        ]
    };
    let push_helper_type = unsafe {
        LLVMFunctionType(
            i32_type,
            push_parameters.as_mut_ptr(),
            push_parameters.len() as u32,
            0,
        )
    };
    let push_helper_name = CString::new("gba_push_thumb_v3").expect("static name contains no NUL");
    let push_helper =
        unsafe { LLVMAddFunction(module, push_helper_name.as_ptr(), push_helper_type) };
    if push_helper.is_null() {
        return Err("LLVMAddFunction for gba_push_thumb_v3 returned null".to_owned());
    }
    let entry = unsafe { append_block(context, function, "entry") };
    let fault = unsafe { append_block(context, function, "fault") };
    let dispatch = unsafe { append_block(context, function, "dispatch") };
    let missing = unsafe { append_block(context, function, "missing_code") };
    unsafe { LLVMPositionBuilderAtEnd(builder, entry) };

    let cycles_ptr = unsafe {
        LLVMBuildStructGEP2(
            builder,
            runtime_cpu_type,
            runtime,
            3,
            c"cycles_ptr".as_ptr(),
        )
    };
    let cycles = unsafe { LLVMBuildLoad2(builder, i64_type, cycles_ptr, c"cycles".as_ptr()) };
    let max_cycles = unsafe { LLVMConstInt(i64_type, u64::MAX, 0) };
    let cycle_overflow = unsafe {
        LLVMBuildICmp(
            builder,
            LLVMIntPredicate::LLVMIntEQ,
            cycles,
            max_cycles,
            c"cycle_overflow".as_ptr(),
        )
    };
    let abi_version_ptr = unsafe {
        LLVMBuildStructGEP2(
            builder,
            runtime_cpu_type,
            runtime,
            2,
            c"abi_version_ptr".as_ptr(),
        )
    };
    let abi_version =
        unsafe { LLVMBuildLoad2(builder, i32_type, abi_version_ptr, c"abi_version".as_ptr()) };
    let cpu_abi_mismatch = unsafe {
        LLVMBuildICmp(
            builder,
            LLVMIntPredicate::LLVMIntNE,
            abi_version,
            LLVMConstInt(i32_type, 2, 0),
            c"abi_mismatch".as_ptr(),
        )
    };
    let outer_abi_version_ptr = unsafe {
        LLVMBuildStructGEP2(
            builder,
            runtime_v3_type,
            runtime,
            2,
            c"outer_abi_version_ptr".as_ptr(),
        )
    };
    let outer_abi_version = unsafe {
        LLVMBuildLoad2(
            builder,
            i32_type,
            outer_abi_version_ptr,
            c"outer_abi_version".as_ptr(),
        )
    };
    let outer_abi_mismatch = unsafe {
        LLVMBuildICmp(
            builder,
            LLVMIntPredicate::LLVMIntNE,
            outer_abi_version,
            LLVMConstInt(i32_type, 3, 0),
            c"outer_abi_mismatch".as_ptr(),
        )
    };
    let abi_mismatch = unsafe {
        LLVMBuildOr(
            builder,
            cpu_abi_mismatch,
            outer_abi_mismatch,
            c"any_abi_mismatch".as_ptr(),
        )
    };
    let must_fault = unsafe {
        LLVMBuildOr(
            builder,
            cycle_overflow,
            abi_mismatch,
            c"must_fault".as_ptr(),
        )
    };
    unsafe { LLVMBuildCondBr(builder, must_fault, fault, dispatch) };

    unsafe { LLVMPositionBuilderAtEnd(builder, fault) };
    let pc_pointer = unsafe { register_pointer(builder, context, runtime_cpu_type, runtime, 15) };
    let pc = unsafe { LLVMBuildLoad2(builder, i32_type, pc_pointer, c"pc_on_fault".as_ptr()) };
    unsafe {
        store_exit(builder, exit_type, exit, i32_type, 4, pc);
        LLVMBuildRet(builder, LLVMConstInt(i32_type, 4, 0));
    }

    unsafe { LLVMPositionBuilderAtEnd(builder, dispatch) };
    let pc_pointer = unsafe { register_pointer(builder, context, runtime_cpu_type, runtime, 15) };
    let pc = unsafe { LLVMBuildLoad2(builder, i32_type, pc_pointer, c"pc_dispatch".as_ptr()) };
    let cpsr_pointer = unsafe {
        LLVMBuildStructGEP2(
            builder,
            runtime_cpu_type,
            runtime,
            1,
            c"dispatch_cpsr_ptr".as_ptr(),
        )
    };
    let cpsr =
        unsafe { LLVMBuildLoad2(builder, i32_type, cpsr_pointer, c"dispatch_cpsr".as_ptr()) };
    let thumb_bit = unsafe {
        LLVMBuildAnd(
            builder,
            cpsr,
            LLVMConstInt(i32_type, 0x20, 0),
            c"thumb_bit".as_ptr(),
        )
    };
    let thumb_active = unsafe {
        LLVMBuildICmp(
            builder,
            LLVMIntPredicate::LLVMIntNE,
            thumb_bit,
            LLVMConstInt(i32_type, 0, 0),
            c"thumb_active".as_ptr(),
        )
    };
    let arm_dispatch = unsafe { append_block(context, function, "arm_dispatch") };
    let thumb_dispatch = unsafe { append_block(context, function, "thumb_dispatch") };
    unsafe { LLVMBuildCondBr(builder, thumb_active, thumb_dispatch, arm_dispatch) };
    unsafe { LLVMPositionBuilderAtEnd(builder, arm_dispatch) };
    let arm_switch =
        unsafe { LLVMBuildSwitch(builder, pc, missing, program.instructions.len() as u32) };
    let mut arm_blocks = Vec::with_capacity(program.instructions.len());
    for instruction in &program.instructions {
        let name = format!("guest_{:08X}", instruction.guest_pc);
        let block = unsafe { append_block(context, function, &name) };
        unsafe {
            LLVMAddCase(
                arm_switch,
                LLVMConstInt(i32_type, u64::from(instruction.guest_pc), 0),
                block,
            )
        };
        arm_blocks.push(block);
    }

    unsafe { LLVMPositionBuilderAtEnd(builder, thumb_dispatch) };
    let thumb_switch = unsafe {
        LLVMBuildSwitch(
            builder,
            pc,
            missing,
            program.thumb_instructions.len() as u32,
        )
    };
    let mut thumb_blocks = Vec::with_capacity(program.thumb_instructions.len());
    for instruction in &program.thumb_instructions {
        let name = format!("thumb_{:08X}", instruction.guest_pc);
        let block = unsafe { append_block(context, function, &name) };
        unsafe {
            LLVMAddCase(
                thumb_switch,
                LLVMConstInt(i32_type, u64::from(instruction.guest_pc), 0),
                block,
            )
        };
        thumb_blocks.push(block);
    }

    unsafe { LLVMPositionBuilderAtEnd(builder, missing) };
    unsafe {
        store_exit(builder, exit_type, exit, i32_type, 3, pc);
        LLVMBuildRet(builder, LLVMConstInt(i32_type, 3, 0));
    }

    for (instruction, block) in program.instructions.iter().zip(arm_blocks) {
        unsafe { LLVMPositionBuilderAtEnd(builder, block) };
        unsafe {
            lower_arm_instruction(
                context,
                builder,
                runtime_cpu_type,
                exit_type,
                runtime,
                exit,
                cycles,
                function,
                helper_type,
                msr_helper,
                store_helper_type,
                store_helper,
                instruction,
            )
        }?;
    }
    for (instruction, block) in program.thumb_instructions.iter().zip(thumb_blocks) {
        unsafe { LLVMPositionBuilderAtEnd(builder, block) };
        unsafe {
            lower_thumb_instruction(
                context,
                builder,
                function,
                runtime_cpu_type,
                exit_type,
                runtime,
                exit,
                cycles,
                instruction,
                push_helper_type,
                push_helper,
            )
        }?;
    }
    Ok(())
}

unsafe fn append_block(
    context: LLVMContextRef,
    function: LLVMValueRef,
    name: &str,
) -> LLVMBasicBlockRef {
    let name = CString::new(name).expect("block name contains no NUL");
    unsafe { LLVMAppendBasicBlockInContext(context, function, name.as_ptr()) }
}

unsafe fn lower_arm_instruction(
    context: LLVMContextRef,
    builder: LLVMBuilderRef,
    runtime_type: LLVMTypeRef,
    exit_type: LLVMTypeRef,
    runtime: LLVMValueRef,
    exit: LLVMValueRef,
    cycles: LLVMValueRef,
    function: LLVMValueRef,
    msr_helper_type: LLVMTypeRef,
    msr_helper: LLVMValueRef,
    store_helper_type: LLVMTypeRef,
    store_helper: LLVMValueRef,
    instruction: &ArmInstruction,
) -> Result<(), String> {
    let i32_type = unsafe { LLVMInt32TypeInContext(context) };
    let cpsr_pointer =
        unsafe { LLVMBuildStructGEP2(builder, runtime_type, runtime, 1, c"cpsr_ptr".as_ptr()) };
    let cpsr = unsafe { LLVMBuildLoad2(builder, i32_type, cpsr_pointer, c"cpsr".as_ptr()) };
    let condition = unsafe { arm_condition_value(builder, context, cpsr, instruction.condition)? };
    let pc_plus_four = instruction.guest_pc.wrapping_add(4);
    let default_next_pc = unsafe { LLVMConstInt(i32_type, u64::from(pc_plus_four), 0) };
    let next_pc = match instruction.operation {
        ArmOperation::MovImmediate { rd, value } => {
            let pointer = unsafe { register_pointer(builder, context, runtime_type, runtime, rd) };
            let old = unsafe { LLVMBuildLoad2(builder, i32_type, pointer, c"old_rd".as_ptr()) };
            let new = unsafe {
                LLVMBuildSelect(
                    builder,
                    condition,
                    LLVMConstInt(i32_type, u64::from(value), 0),
                    old,
                    c"mov_result".as_ptr(),
                )
            };
            unsafe { LLVMBuildStore(builder, new, pointer) };
            default_next_pc
        }
        ArmOperation::MovRegister { rd, rm } => {
            let source = if rm == 15 {
                unsafe {
                    LLVMConstInt(i32_type, u64::from(instruction.guest_pc.wrapping_add(8)), 0)
                }
            } else {
                let source_pointer =
                    unsafe { register_pointer(builder, context, runtime_type, runtime, rm) };
                unsafe { LLVMBuildLoad2(builder, i32_type, source_pointer, c"mov_source".as_ptr()) }
            };
            let destination_pointer =
                unsafe { register_pointer(builder, context, runtime_type, runtime, rd) };
            let old = unsafe {
                LLVMBuildLoad2(
                    builder,
                    i32_type,
                    destination_pointer,
                    c"old_mov_rd".as_ptr(),
                )
            };
            let new = unsafe {
                LLVMBuildSelect(
                    builder,
                    condition,
                    source,
                    old,
                    c"mov_register_result".as_ptr(),
                )
            };
            unsafe { LLVMBuildStore(builder, new, destination_pointer) };
            default_next_pc
        }
        ArmOperation::LoadRomLiteral { rd, value, .. } => {
            let pointer = unsafe { register_pointer(builder, context, runtime_type, runtime, rd) };
            let old =
                unsafe { LLVMBuildLoad2(builder, i32_type, pointer, c"old_literal_rd".as_ptr()) };
            let new = unsafe {
                LLVMBuildSelect(
                    builder,
                    condition,
                    LLVMConstInt(i32_type, u64::from(value), 0),
                    old,
                    c"literal_load_result".as_ptr(),
                )
            };
            unsafe { LLVMBuildStore(builder, new, pointer) };
            default_next_pc
        }
        ArmOperation::StoreWord { rd, rn, offset, up } => {
            let base_pointer =
                unsafe { register_pointer(builder, context, runtime_type, runtime, rn) };
            let value_pointer =
                unsafe { register_pointer(builder, context, runtime_type, runtime, rd) };
            let base =
                unsafe { LLVMBuildLoad2(builder, i32_type, base_pointer, c"store_base".as_ptr()) };
            let value = unsafe {
                LLVMBuildLoad2(builder, i32_type, value_pointer, c"store_value".as_ptr())
            };
            let offset_value = unsafe { LLVMConstInt(i32_type, u64::from(offset), 0) };
            let address = unsafe {
                if up {
                    LLVMBuildAdd(builder, base, offset_value, c"store_address".as_ptr())
                } else {
                    LLVMBuildSub(builder, base, offset_value, c"store_address".as_ptr())
                }
            };
            let perform_store = unsafe {
                append_block(
                    context,
                    function,
                    &format!("store_effect_{:08X}", instruction.guest_pc),
                )
            };
            let skip_store = unsafe {
                append_block(
                    context,
                    function,
                    &format!("store_skip_{:08X}", instruction.guest_pc),
                )
            };
            let commit = unsafe {
                append_block(
                    context,
                    function,
                    &format!("store_commit_{:08X}", instruction.guest_pc),
                )
            };
            let failed = unsafe {
                append_block(
                    context,
                    function,
                    &format!("store_fault_{:08X}", instruction.guest_pc),
                )
            };
            unsafe { LLVMBuildCondBr(builder, condition, perform_store, skip_store) };

            unsafe { LLVMPositionBuilderAtEnd(builder, perform_store) };
            let mut arguments = [runtime, exit, address, value];
            let status = unsafe {
                LLVMBuildCall2(
                    builder,
                    store_helper_type,
                    store_helper,
                    arguments.as_mut_ptr(),
                    arguments.len() as u32,
                    c"store_status".as_ptr(),
                )
            };
            let succeeded = unsafe {
                LLVMBuildICmp(
                    builder,
                    LLVMIntPredicate::LLVMIntEQ,
                    status,
                    LLVMConstInt(i32_type, 0, 0),
                    c"store_succeeded".as_ptr(),
                )
            };
            unsafe { LLVMBuildCondBr(builder, succeeded, commit, failed) };

            unsafe { LLVMPositionBuilderAtEnd(builder, failed) };
            unsafe { LLVMBuildRet(builder, status) };

            unsafe { LLVMPositionBuilderAtEnd(builder, skip_store) };
            unsafe { LLVMBuildBr(builder, commit) };
            unsafe { LLVMPositionBuilderAtEnd(builder, commit) };
            default_next_pc
        }
        ArmOperation::LoadPcRelativeWord { .. } => {
            return Err(format!(
                "unresolved PC-relative ROM literal at 0x{:08X}",
                instruction.guest_pc
            ));
        }
        ArmOperation::AddImmediate { rd, rn, value } => {
            let source_pointer =
                unsafe { register_pointer(builder, context, runtime_type, runtime, rn) };
            let destination_pointer =
                unsafe { register_pointer(builder, context, runtime_type, runtime, rd) };
            let source = unsafe {
                LLVMBuildLoad2(builder, i32_type, source_pointer, c"add_source".as_ptr())
            };
            let old = unsafe {
                LLVMBuildLoad2(builder, i32_type, destination_pointer, c"old_rd".as_ptr())
            };
            let sum = unsafe {
                LLVMBuildAdd(
                    builder,
                    source,
                    LLVMConstInt(i32_type, u64::from(value), 0),
                    c"add_result".as_ptr(),
                )
            };
            let new =
                unsafe { LLVMBuildSelect(builder, condition, sum, old, c"add_commit".as_ptr()) };
            unsafe { LLVMBuildStore(builder, new, destination_pointer) };
            default_next_pc
        }
        ArmOperation::AddPcRelativeImmediate { rd, value } => {
            let pointer = unsafe { register_pointer(builder, context, runtime_type, runtime, rd) };
            let old =
                unsafe { LLVMBuildLoad2(builder, i32_type, pointer, c"old_pc_add_rd".as_ptr()) };
            let result = instruction.guest_pc.wrapping_add(8).wrapping_add(value);
            let new = unsafe {
                LLVMBuildSelect(
                    builder,
                    condition,
                    LLVMConstInt(i32_type, u64::from(result), 0),
                    old,
                    c"pc_add_commit".as_ptr(),
                )
            };
            unsafe { LLVMBuildStore(builder, new, pointer) };
            default_next_pc
        }
        ArmOperation::CompareImmediate { rn, value } => {
            let source_pointer =
                unsafe { register_pointer(builder, context, runtime_type, runtime, rn) };
            let source = unsafe {
                LLVMBuildLoad2(builder, i32_type, source_pointer, c"cmp_source".as_ptr())
            };
            let new_cpsr = unsafe { lower_cmp_flags(builder, i32_type, cpsr, source, value) };
            let committed = unsafe {
                LLVMBuildSelect(builder, condition, new_cpsr, cpsr, c"cmp_commit".as_ptr())
            };
            unsafe { LLVMBuildStore(builder, committed, cpsr_pointer) };
            default_next_pc
        }
        ArmOperation::MsrCpsrRegister { rm, field_mask } => {
            let source_pointer =
                unsafe { register_pointer(builder, context, runtime_type, runtime, rm) };
            let source = unsafe {
                LLVMBuildLoad2(builder, i32_type, source_pointer, c"msr_source".as_ptr())
            };
            let selected_mask = unsafe {
                LLVMBuildSelect(
                    builder,
                    condition,
                    LLVMConstInt(i32_type, u64::from(field_mask), 0),
                    LLVMConstInt(i32_type, 0, 0),
                    c"msr_selected_mask".as_ptr(),
                )
            };
            let mut arguments = [runtime, source, selected_mask];
            let status = unsafe {
                LLVMBuildCall2(
                    builder,
                    msr_helper_type,
                    msr_helper,
                    arguments.as_mut_ptr(),
                    arguments.len() as u32,
                    c"msr_status".as_ptr(),
                )
            };
            let succeeded = unsafe {
                LLVMBuildICmp(
                    builder,
                    LLVMIntPredicate::LLVMIntEQ,
                    status,
                    LLVMConstInt(i32_type, 0, 0),
                    c"msr_succeeded".as_ptr(),
                )
            };
            let success_block = unsafe {
                append_block(
                    context,
                    function,
                    &format!("msr_success_{:08X}", instruction.guest_pc),
                )
            };
            let failure_block = unsafe {
                append_block(
                    context,
                    function,
                    &format!("msr_failure_{:08X}", instruction.guest_pc),
                )
            };
            unsafe { LLVMBuildCondBr(builder, succeeded, success_block, failure_block) };
            unsafe { LLVMPositionBuilderAtEnd(builder, failure_block) };
            let current_pc_pointer =
                unsafe { register_pointer(builder, context, runtime_type, runtime, 15) };
            let current_pc = unsafe {
                LLVMBuildLoad2(
                    builder,
                    i32_type,
                    current_pc_pointer,
                    c"msr_failure_pc".as_ptr(),
                )
            };
            let reason_pointer = unsafe {
                LLVMBuildStructGEP2(
                    builder,
                    exit_type,
                    exit,
                    0,
                    c"msr_failure_reason_ptr".as_ptr(),
                )
            };
            let exit_pc_pointer = unsafe {
                LLVMBuildStructGEP2(
                    builder,
                    exit_type,
                    exit,
                    1,
                    c"msr_failure_exit_pc_ptr".as_ptr(),
                )
            };
            unsafe {
                LLVMBuildStore(builder, status, reason_pointer);
                LLVMBuildStore(builder, current_pc, exit_pc_pointer);
                LLVMBuildRet(builder, status);
            }
            unsafe { LLVMPositionBuilderAtEnd(builder, success_block) };
            default_next_pc
        }
        ArmOperation::BranchExchange { rm } => {
            let target = if rm == 15 {
                unsafe {
                    LLVMConstInt(i32_type, u64::from(instruction.guest_pc.wrapping_add(8)), 0)
                }
            } else {
                let target_pointer =
                    unsafe { register_pointer(builder, context, runtime_type, runtime, rm) };
                unsafe { LLVMBuildLoad2(builder, i32_type, target_pointer, c"bx_target".as_ptr()) }
            };
            let bit_zero = unsafe {
                LLVMBuildAnd(
                    builder,
                    target,
                    LLVMConstInt(i32_type, 1, 0),
                    c"bx_target_bit_zero".as_ptr(),
                )
            };
            let enters_thumb = unsafe {
                LLVMBuildICmp(
                    builder,
                    LLVMIntPredicate::LLVMIntNE,
                    bit_zero,
                    LLVMConstInt(i32_type, 0, 0),
                    c"bx_enters_thumb".as_ptr(),
                )
            };
            let t_bit = unsafe {
                LLVMBuildShl(
                    builder,
                    LLVMBuildZExt(builder, enters_thumb, i32_type, c"bx_t_bit_value".as_ptr()),
                    LLVMConstInt(i32_type, 5, 0),
                    c"bx_t_bit".as_ptr(),
                )
            };
            let cpsr_without_t = unsafe {
                LLVMBuildAnd(
                    builder,
                    cpsr,
                    LLVMConstInt(i32_type, u64::from(!0x20_u32), 0),
                    c"bx_cpsr_without_t".as_ptr(),
                )
            };
            let switched_cpsr =
                unsafe { LLVMBuildOr(builder, cpsr_without_t, t_bit, c"bx_cpsr_result".as_ptr()) };
            let committed_cpsr = unsafe {
                LLVMBuildSelect(
                    builder,
                    condition,
                    switched_cpsr,
                    cpsr,
                    c"bx_cpsr_commit".as_ptr(),
                )
            };
            unsafe { LLVMBuildStore(builder, committed_cpsr, cpsr_pointer) };

            let thumb_pc = unsafe {
                LLVMBuildAnd(
                    builder,
                    target,
                    LLVMConstInt(i32_type, u64::from(!1_u32), 0),
                    c"bx_thumb_pc".as_ptr(),
                )
            };
            let arm_pc = unsafe {
                LLVMBuildAnd(
                    builder,
                    target,
                    LLVMConstInt(i32_type, u64::from(!3_u32), 0),
                    c"bx_arm_pc".as_ptr(),
                )
            };
            let aligned_target = unsafe {
                LLVMBuildSelect(
                    builder,
                    enters_thumb,
                    thumb_pc,
                    arm_pc,
                    c"bx_aligned_target".as_ptr(),
                )
            };
            unsafe {
                LLVMBuildSelect(
                    builder,
                    condition,
                    aligned_target,
                    default_next_pc,
                    c"bx_next_pc".as_ptr(),
                )
            }
        }
        ArmOperation::Branch { target, link } => {
            if link {
                let lr_pointer =
                    unsafe { register_pointer(builder, context, runtime_type, runtime, 14) };
                let old_lr =
                    unsafe { LLVMBuildLoad2(builder, i32_type, lr_pointer, c"old_lr".as_ptr()) };
                let new_lr = unsafe {
                    LLVMBuildSelect(
                        builder,
                        condition,
                        LLVMConstInt(i32_type, u64::from(pc_plus_four), 0),
                        old_lr,
                        c"link_commit".as_ptr(),
                    )
                };
                unsafe { LLVMBuildStore(builder, new_lr, lr_pointer) };
            }
            unsafe {
                LLVMBuildSelect(
                    builder,
                    condition,
                    LLVMConstInt(i32_type, u64::from(target), 0),
                    LLVMConstInt(i32_type, u64::from(pc_plus_four), 0),
                    c"branch_target".as_ptr(),
                )
            }
        }
    };
    let pc_pointer = unsafe { register_pointer(builder, context, runtime_type, runtime, 15) };
    unsafe { LLVMBuildStore(builder, next_pc, pc_pointer) };
    let cycles_pointer =
        unsafe { LLVMBuildStructGEP2(builder, runtime_type, runtime, 3, c"cycles_ptr".as_ptr()) };
    let next_cycles = unsafe {
        LLVMBuildAdd(
            builder,
            cycles,
            LLVMConstInt(LLVMInt64TypeInContext(context), 1, 0),
            c"next_cycle".as_ptr(),
        )
    };
    unsafe { LLVMBuildStore(builder, next_cycles, cycles_pointer) };
    unsafe {
        store_exit(builder, exit_type, exit, i32_type, 0, next_pc);
        LLVMBuildRet(builder, LLVMConstInt(i32_type, 0, 0));
    }
    Ok(())
}

unsafe fn lower_thumb_instruction(
    context: LLVMContextRef,
    builder: LLVMBuilderRef,
    function: LLVMValueRef,
    runtime_type: LLVMTypeRef,
    exit_type: LLVMTypeRef,
    runtime: LLVMValueRef,
    exit: LLVMValueRef,
    cycles: LLVMValueRef,
    instruction: &ThumbInstruction,
    push_helper_type: LLVMTypeRef,
    push_helper: LLVMValueRef,
) -> Result<(), String> {
    let i32_type = unsafe { LLVMInt32TypeInContext(context) };
    match instruction.operation {
        ThumbOperation::BranchExchange { rm } => {
            let target = if rm == 15 {
                unsafe {
                    LLVMConstInt(i32_type, u64::from(instruction.guest_pc.wrapping_add(4)), 0)
                }
            } else {
                let target_pointer =
                    unsafe { register_pointer(builder, context, runtime_type, runtime, rm) };
                unsafe {
                    LLVMBuildLoad2(
                        builder,
                        i32_type,
                        target_pointer,
                        c"thumb_bx_target".as_ptr(),
                    )
                }
            };
            let bit_zero = unsafe {
                LLVMBuildAnd(
                    builder,
                    target,
                    LLVMConstInt(i32_type, 1, 0),
                    c"thumb_bx_target_bit_zero".as_ptr(),
                )
            };
            let enters_thumb = unsafe {
                LLVMBuildICmp(
                    builder,
                    LLVMIntPredicate::LLVMIntNE,
                    bit_zero,
                    LLVMConstInt(i32_type, 0, 0),
                    c"thumb_bx_enters_thumb".as_ptr(),
                )
            };
            let thumb_pc = unsafe {
                LLVMBuildAnd(
                    builder,
                    target,
                    LLVMConstInt(i32_type, u64::from(!1_u32), 0),
                    c"thumb_bx_thumb_pc".as_ptr(),
                )
            };
            let arm_pc = unsafe {
                LLVMBuildAnd(
                    builder,
                    target,
                    LLVMConstInt(i32_type, u64::from(!3_u32), 0),
                    c"thumb_bx_arm_pc".as_ptr(),
                )
            };
            let next_pc = unsafe {
                LLVMBuildSelect(
                    builder,
                    enters_thumb,
                    thumb_pc,
                    arm_pc,
                    c"thumb_bx_next_pc".as_ptr(),
                )
            };
            let cpsr_pointer = unsafe {
                LLVMBuildStructGEP2(builder, runtime_type, runtime, 1, c"cpsr_ptr".as_ptr())
            };
            let cpsr = unsafe { LLVMBuildLoad2(builder, i32_type, cpsr_pointer, c"cpsr".as_ptr()) };
            let t_bit = unsafe {
                LLVMBuildShl(
                    builder,
                    LLVMBuildZExt(
                        builder,
                        enters_thumb,
                        i32_type,
                        c"thumb_bx_t_bit_value".as_ptr(),
                    ),
                    LLVMConstInt(i32_type, 5, 0),
                    c"thumb_bx_t_bit".as_ptr(),
                )
            };
            let cpsr_without_t = unsafe {
                LLVMBuildAnd(
                    builder,
                    cpsr,
                    LLVMConstInt(i32_type, u64::from(!0x20_u32), 0),
                    c"thumb_bx_cpsr_without_t".as_ptr(),
                )
            };
            let switched_cpsr = unsafe {
                LLVMBuildOr(
                    builder,
                    cpsr_without_t,
                    t_bit,
                    c"thumb_bx_cpsr_result".as_ptr(),
                )
            };
            unsafe { LLVMBuildStore(builder, switched_cpsr, cpsr_pointer) };
            unsafe {
                commit_thumb_instruction(
                    context,
                    builder,
                    runtime_type,
                    exit_type,
                    runtime,
                    exit,
                    cycles,
                    i32_type,
                    next_pc,
                )
            };
        }
        ThumbOperation::Push { register_list } => {
            let mut arguments = [runtime, exit, unsafe {
                LLVMConstInt(i32_type, u64::from(register_list), 0)
            }];
            let status = unsafe {
                LLVMBuildCall2(
                    builder,
                    push_helper_type,
                    push_helper,
                    arguments.as_mut_ptr(),
                    arguments.len() as u32,
                    c"thumb_push_status".as_ptr(),
                )
            };
            let succeeded = unsafe {
                LLVMBuildICmp(
                    builder,
                    LLVMIntPredicate::LLVMIntEQ,
                    status,
                    LLVMConstInt(i32_type, 0, 0),
                    c"thumb_push_succeeded".as_ptr(),
                )
            };
            let success = unsafe {
                append_block(
                    context,
                    function,
                    &format!("thumb_push_success_{:08X}", instruction.guest_pc),
                )
            };
            let failure = unsafe {
                append_block(
                    context,
                    function,
                    &format!("thumb_push_failure_{:08X}", instruction.guest_pc),
                )
            };
            unsafe { LLVMBuildCondBr(builder, succeeded, success, failure) };
            unsafe { LLVMPositionBuilderAtEnd(builder, failure) };
            unsafe { LLVMBuildRet(builder, status) };
            unsafe { LLVMPositionBuilderAtEnd(builder, success) };
            let next_pc = unsafe {
                LLVMConstInt(i32_type, u64::from(instruction.guest_pc.wrapping_add(2)), 0)
            };
            unsafe {
                commit_thumb_instruction(
                    context,
                    builder,
                    runtime_type,
                    exit_type,
                    runtime,
                    exit,
                    cycles,
                    i32_type,
                    next_pc,
                )
            };
        }
        ThumbOperation::MovImmediate { rd, value } => {
            let destination_pointer =
                unsafe { register_pointer(builder, context, runtime_type, runtime, rd) };
            let immediate = unsafe { LLVMConstInt(i32_type, u64::from(value), 0) };
            unsafe { LLVMBuildStore(builder, immediate, destination_pointer) };

            let cpsr_pointer = unsafe {
                LLVMBuildStructGEP2(builder, runtime_type, runtime, 1, c"cpsr_ptr".as_ptr())
            };
            let old_cpsr =
                unsafe { LLVMBuildLoad2(builder, i32_type, cpsr_pointer, c"cpsr".as_ptr()) };
            let cpsr_without_nz = unsafe {
                LLVMBuildAnd(
                    builder,
                    old_cpsr,
                    LLVMConstInt(i32_type, 0x3FFF_FFFF, 0),
                    c"thumb_mov_imm_cpsr_without_nz".as_ptr(),
                )
            };
            let cpsr = if value == 0 {
                unsafe {
                    LLVMBuildOr(
                        builder,
                        cpsr_without_nz,
                        LLVMConstInt(i32_type, 0x4000_0000, 0),
                        c"thumb_mov_imm_cpsr_zero".as_ptr(),
                    )
                }
            } else {
                cpsr_without_nz
            };
            unsafe { LLVMBuildStore(builder, cpsr, cpsr_pointer) };

            let next_pc = unsafe {
                LLVMConstInt(i32_type, u64::from(instruction.guest_pc.wrapping_add(2)), 0)
            };
            unsafe {
                commit_thumb_instruction(
                    context,
                    builder,
                    runtime_type,
                    exit_type,
                    runtime,
                    exit,
                    cycles,
                    i32_type,
                    next_pc,
                )
            };
        }
        ThumbOperation::MovRegister { rd, rm } => {
            let source = if rm == 15 {
                unsafe {
                    LLVMConstInt(i32_type, u64::from(instruction.guest_pc.wrapping_add(4)), 0)
                }
            } else {
                let source_pointer =
                    unsafe { register_pointer(builder, context, runtime_type, runtime, rm) };
                unsafe {
                    LLVMBuildLoad2(
                        builder,
                        i32_type,
                        source_pointer,
                        c"thumb_mov_source".as_ptr(),
                    )
                }
            };
            let destination_pointer =
                unsafe { register_pointer(builder, context, runtime_type, runtime, rd) };
            unsafe { LLVMBuildStore(builder, source, destination_pointer) };
            let next_pc = unsafe {
                LLVMConstInt(i32_type, u64::from(instruction.guest_pc.wrapping_add(2)), 0)
            };
            unsafe {
                commit_thumb_instruction(
                    context,
                    builder,
                    runtime_type,
                    exit_type,
                    runtime,
                    exit,
                    cycles,
                    i32_type,
                    next_pc,
                )
            };
        }
    }
    Ok(())
}

unsafe fn commit_thumb_instruction(
    context: LLVMContextRef,
    builder: LLVMBuilderRef,
    runtime_type: LLVMTypeRef,
    exit_type: LLVMTypeRef,
    runtime: LLVMValueRef,
    exit: LLVMValueRef,
    cycles: LLVMValueRef,
    i32_type: LLVMTypeRef,
    next_pc: LLVMValueRef,
) {
    let pc_pointer = unsafe { register_pointer(builder, context, runtime_type, runtime, 15) };
    unsafe { LLVMBuildStore(builder, next_pc, pc_pointer) };
    let cycles_pointer =
        unsafe { LLVMBuildStructGEP2(builder, runtime_type, runtime, 3, c"cycles_ptr".as_ptr()) };
    let next_cycles = unsafe {
        LLVMBuildAdd(
            builder,
            cycles,
            LLVMConstInt(LLVMInt64TypeInContext(context), 1, 0),
            c"thumb_next_cycle".as_ptr(),
        )
    };
    unsafe { LLVMBuildStore(builder, next_cycles, cycles_pointer) };
    unsafe {
        store_exit(builder, exit_type, exit, i32_type, 0, next_pc);
        LLVMBuildRet(builder, LLVMConstInt(i32_type, 0, 0));
    }
}

unsafe fn arm_condition_value(
    builder: LLVMBuilderRef,
    context: LLVMContextRef,
    cpsr: LLVMValueRef,
    condition: u8,
) -> Result<LLVMValueRef, String> {
    let i32_type = unsafe { LLVMInt32TypeInContext(context) };
    let i1_type = unsafe { LLVMInt1TypeInContext(context) };
    if condition == 14 {
        return Ok(unsafe { LLVMConstInt(i1_type, 1, 0) });
    }
    let flag = |bit: u32, name: &'static CStr| unsafe {
        let masked = LLVMBuildAnd(
            builder,
            cpsr,
            LLVMConstInt(i32_type, 1_u64 << bit, 0),
            c"flag_mask".as_ptr(),
        );
        LLVMBuildICmp(
            builder,
            LLVMIntPredicate::LLVMIntNE,
            masked,
            LLVMConstInt(i32_type, 0, 0),
            name.as_ptr(),
        )
    };
    let n = flag(31, c"flag_n");
    let z = flag(30, c"flag_z");
    let c = flag(29, c"flag_c");
    let v = flag(28, c"flag_v");
    let not = |value, name| unsafe { LLVMBuildNot(builder, value, name) };
    let and = |left, right, name| unsafe { LLVMBuildAnd(builder, left, right, name) };
    let or = |left, right, name| unsafe { LLVMBuildOr(builder, left, right, name) };
    let equal = |left, right, name| unsafe {
        LLVMBuildICmp(builder, LLVMIntPredicate::LLVMIntEQ, left, right, name)
    };
    let not_equal = |left, right, name| unsafe {
        LLVMBuildICmp(builder, LLVMIntPredicate::LLVMIntNE, left, right, name)
    };
    let result = match condition {
        0 => z,
        1 => not(z, c"condition_ne".as_ptr()),
        2 => c,
        3 => not(c, c"condition_cc".as_ptr()),
        4 => n,
        5 => not(n, c"condition_pl".as_ptr()),
        6 => v,
        7 => not(v, c"condition_vc".as_ptr()),
        8 => and(
            c,
            not(z, c"condition_hi_z".as_ptr()),
            c"condition_hi".as_ptr(),
        ),
        9 => or(
            not(c, c"condition_ls_c".as_ptr()),
            z,
            c"condition_ls".as_ptr(),
        ),
        10 => equal(n, v, c"condition_ge".as_ptr()),
        11 => not_equal(n, v, c"condition_lt".as_ptr()),
        12 => and(
            not(z, c"condition_gt_z".as_ptr()),
            equal(n, v, c"condition_gt_nv".as_ptr()),
            c"condition_gt".as_ptr(),
        ),
        13 => or(
            z,
            not_equal(n, v, c"condition_le_nv".as_ptr()),
            c"condition_le".as_ptr(),
        ),
        _ => return Err(format!("undefined ARM condition {condition}")),
    };
    Ok(result)
}

unsafe fn lower_cmp_flags(
    builder: LLVMBuilderRef,
    i32_type: LLVMTypeRef,
    old_cpsr: LLVMValueRef,
    lhs: LLVMValueRef,
    immediate: u32,
) -> LLVMValueRef {
    let rhs = unsafe { LLVMConstInt(i32_type, u64::from(immediate), 0) };
    let result = unsafe { LLVMBuildSub(builder, lhs, rhs, c"cmp_result".as_ptr()) };
    let negative = unsafe {
        LLVMBuildAnd(
            builder,
            result,
            LLVMConstInt(i32_type, 0x8000_0000, 0),
            c"cmp_negative".as_ptr(),
        )
    };
    let zero = unsafe {
        LLVMBuildICmp(
            builder,
            LLVMIntPredicate::LLVMIntEQ,
            result,
            LLVMConstInt(i32_type, 0, 0),
            c"cmp_zero".as_ptr(),
        )
    };
    let carry = unsafe {
        LLVMBuildICmp(
            builder,
            LLVMIntPredicate::LLVMIntUGE,
            lhs,
            rhs,
            c"cmp_carry".as_ptr(),
        )
    };
    let lhs_xor_rhs = unsafe { LLVMBuildXor(builder, lhs, rhs, c"cmp_lhs_xor_rhs".as_ptr()) };
    let lhs_xor_result =
        unsafe { LLVMBuildXor(builder, lhs, result, c"cmp_lhs_xor_result".as_ptr()) };
    let overflow_bits = unsafe {
        LLVMBuildAnd(
            builder,
            lhs_xor_rhs,
            lhs_xor_result,
            c"cmp_overflow_bits".as_ptr(),
        )
    };
    let overflow_set = unsafe {
        LLVMBuildICmp(
            builder,
            LLVMIntPredicate::LLVMIntNE,
            LLVMBuildAnd(
                builder,
                overflow_bits,
                LLVMConstInt(i32_type, 0x8000_0000, 0),
                c"cmp_overflow_sign".as_ptr(),
            ),
            LLVMConstInt(i32_type, 0, 0),
            c"cmp_overflow".as_ptr(),
        )
    };
    let z32 = unsafe { LLVMBuildZExt(builder, zero, i32_type, c"cmp_z32".as_ptr()) };
    let c32 = unsafe { LLVMBuildZExt(builder, carry, i32_type, c"cmp_c32".as_ptr()) };
    let v32 = unsafe { LLVMBuildZExt(builder, overflow_set, i32_type, c"cmp_v32".as_ptr()) };
    let z_flag = unsafe {
        LLVMBuildShl(
            builder,
            z32,
            LLVMConstInt(i32_type, 30, 0),
            c"cmp_z_flag".as_ptr(),
        )
    };
    let c_flag = unsafe {
        LLVMBuildShl(
            builder,
            c32,
            LLVMConstInt(i32_type, 29, 0),
            c"cmp_c_flag".as_ptr(),
        )
    };
    let v_flag = unsafe {
        LLVMBuildShl(
            builder,
            v32,
            LLVMConstInt(i32_type, 28, 0),
            c"cmp_v_flag".as_ptr(),
        )
    };
    let low_cpsr = unsafe {
        LLVMBuildAnd(
            builder,
            old_cpsr,
            LLVMConstInt(i32_type, 0x0FFF_FFFF, 0),
            c"cmp_preserved_cpsr".as_ptr(),
        )
    };
    let flags = unsafe { LLVMBuildOr(builder, negative, z_flag, c"cmp_flags_nz".as_ptr()) };
    let flags = unsafe { LLVMBuildOr(builder, flags, c_flag, c"cmp_flags_nzc".as_ptr()) };
    let flags = unsafe { LLVMBuildOr(builder, flags, v_flag, c"cmp_flags_nzcv".as_ptr()) };
    unsafe { LLVMBuildOr(builder, low_cpsr, flags, c"cmp_new_cpsr".as_ptr()) }
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

unsafe fn validate_abi_layout_v2(
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
        unsafe { LLVMOffsetOfElement(target_data, runtime_type, 4) },
        unsafe { LLVMOffsetOfElement(target_data, runtime_type, 5) },
        unsafe { LLVMOffsetOfElement(target_data, runtime_type, 6) },
        unsafe { LLVMOffsetOfElement(target_data, runtime_type, 7) },
    ];
    let exit_size = unsafe { LLVMABISizeOfType(target_data, exit_type) };
    let exit_alignment = unsafe { LLVMABIAlignmentOfType(target_data, exit_type) };
    let exit_offsets = [
        unsafe { LLVMOffsetOfElement(target_data, exit_type, 0) },
        unsafe { LLVMOffsetOfElement(target_data, exit_type, 1) },
    ];
    if runtime_size != 192
        || runtime_alignment != 8
        || runtime_offsets != [0, 64, 68, 72, 80, 100, 120, 168]
        || exit_size != 8
        || exit_alignment != 4
        || exit_offsets != [0, 4]
    {
        return Err(format!(
            "LLVM target ABI layout does not match gba_abi_v2: runtime size/alignment/offsets={runtime_size}/{runtime_alignment}/{runtime_offsets:?}, exit={exit_size}/{exit_alignment}/{exit_offsets:?}"
        ));
    }
    Ok(())
}

unsafe fn validate_abi_layout_v3(
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
    ];
    let exit_size = unsafe { LLVMABISizeOfType(target_data, exit_type) };
    let exit_alignment = unsafe { LLVMABIAlignmentOfType(target_data, exit_type) };
    let exit_offsets = [
        unsafe { LLVMOffsetOfElement(target_data, exit_type, 0) },
        unsafe { LLVMOffsetOfElement(target_data, exit_type, 1) },
        unsafe { LLVMOffsetOfElement(target_data, exit_type, 2) },
        unsafe { LLVMOffsetOfElement(target_data, exit_type, 3) },
    ];
    if runtime_size != 208
        || runtime_alignment != 8
        || runtime_offsets != [0, 192, 200]
        || exit_size != 16
        || exit_alignment != 4
        || exit_offsets != [0, 4, 8, 12]
    {
        return Err(format!(
            "LLVM target ABI layout does not match gba_abi_v3: runtime size/alignment/offsets={runtime_size}/{runtime_alignment}/{runtime_offsets:?}, exit={exit_size}/{exit_alignment}/{exit_offsets:?}"
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
