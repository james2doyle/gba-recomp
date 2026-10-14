import importlib.util
from pathlib import Path

import lldb


probe_path = Path.cwd() / "knowledge/evidence/leafgreen-bx-r0-lldb-probe.py"
probe_spec = importlib.util.spec_from_file_location("leafgreen_probe", probe_path)
probe = importlib.util.module_from_spec(probe_spec)
probe_spec.loader.exec_module(probe)

WANTED = {
    0x080003B8,
    0x080003C4,
    0x080003C6,
    0x080003CA,
    0x080006B2,
    0x080006B4,
    0x080006B6,
    0x080006B8,
    0x080006BA,
    0x08000B54,
    0x08000B58,
    0x08000B60,
    0x08000B62,
    0x08000B64,
    0x08000B66,
    0x08000B68,
    0x08000B6A,
}


def av_repr(value, process, target):
    if value.GetType().IsReferenceType() or value.GetType().IsPointerType():
        value = value.Dereference()
    if probe.enum_tag(value) == 0:
        return "Unknown"
    return probe.btree_set_values(value, process, target)


def state_cb(frame, bp_loc, internal_dict):
    instruction = frame.FindVariable("instruction")
    if not instruction.IsValid():
        return False
    pc = instruction.GetChildMemberWithName("guest_pc").GetValueAsUnsigned()
    if pc not in WANTED:
        return False

    process = frame.GetThread().GetProcess()
    target = process.GetTarget()
    state_value = frame.FindVariable("state")
    state = state_value.Dereference() if state_value.GetType().IsReferenceType() or state_value.GetType().IsPointerType() else state_value
    registers = state.GetChildMemberWithName("registers")
    registers_out = {
        "r%d" % index: av_repr(registers.GetChildAtIndex(index), process, target)
        for index in range(6)
    }
    registers_out["sp"] = av_repr(registers.GetChildAtIndex(13), process, target)
    registers_out["lr"] = av_repr(registers.GetChildAtIndex(14), process, target)
    try:
        memory = probe.abstract_map_entries(state, process, target, "memory_words")
        io = probe.abstract_map_entries(state, process, target, "io_halfwords")
    except Exception as exc:
        memory = "unavailable:%s" % exc
        io = "unavailable:%s" % exc

    encoding = instruction.GetChildMemberWithName("encoding").GetValueAsUnsigned()
    print(
        "STATIC_STATE pc=0x%08X encoding=0x%04X registers=%r memory=%r io=%r"
        % (pc, encoding, registers_out, memory, io)
    )
    return False


def __lldb_init_module(debugger, internal_dict):
    target = debugger.GetSelectedTarget()
    breakpoint = target.BreakpointCreateByRegex(".*process_thumb_instruction.*")
    breakpoint.SetScriptCallbackFunction("leafgreen_after_waitcnt_state_trace.state_cb")
    breakpoint.SetAutoContinue(True)
    print("STATIC_STATE_TRACE breakpoints=%s" % breakpoint.GetNumLocations())
