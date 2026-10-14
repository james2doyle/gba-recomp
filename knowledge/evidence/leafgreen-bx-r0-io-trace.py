import lldb
import sys
sys.path.insert(0, "/tmp")
import leafgreen_lldb_probe as probe


def value_tag(value):
    try:
        return value.GetChildAtIndex(0).GetChildAtIndex(0).GetChildMemberWithName("$discr$").GetValueAsUnsigned()
    except Exception:
        return None


def av_repr(value, process, target):
    if value.GetType().IsReferenceType() or value.GetType().IsPointerType():
        value = value.Dereference()
    tag = value_tag(value)
    if tag == 0:
        return "Unknown"
    try:
        return probe.btree_set_values(value, process, target)
    except Exception as exc:
        return "unavailable:%s" % exc


def frame_chain(frame):
    thread = frame.GetThread()
    process = thread.GetProcess()
    target = process.GetTarget()
    rows = []
    for i in range(min(thread.GetNumFrames(), 8)):
        current = thread.GetFrameAtIndex(i)
        if not current.IsValid():
            break
        name = current.GetFunctionName() or current.GetDisplayFunctionName()
        row = {"function": name}
        ins = current.FindVariable("instruction")
        if ins.IsValid():
            pc = ins.GetChildMemberWithName("guest_pc")
            enc = ins.GetChildMemberWithName("encoding")
            if pc.IsValid():
                row["pc"] = "0x%08X" % pc.GetValueAsUnsigned()
            if enc.IsValid():
                row["encoding"] = "0x%04X" % enc.GetValueAsUnsigned()
        if "invalidate_store" in name:
            addresses = current.FindVariable("addresses")
            count = current.FindVariable("byte_count")
            mask = current.FindVariable("align_mask")
            if addresses.IsValid():
                row["addresses"] = av_repr(addresses, process, target)
            if count.IsValid():
                row["byte_count"] = count.GetValue()
            if mask.IsValid():
                row["align_mask"] = mask.GetValue()
        if "process_thumb_instruction" in name and row.get("pc") == "0x080003B8":
            state = current.FindVariable("state")
            if state.IsValid():
                st = state.Dereference() if state.GetType().IsReferenceType() or state.GetType().IsPointerType() else state
                regs = st.GetChildMemberWithName("registers")
                row["regs"] = {"r0": av_repr(regs.GetChildAtIndex(0), process, target),
                               "r1": av_repr(regs.GetChildAtIndex(1), process, target),
                               "sp": av_repr(regs.GetChildAtIndex(13), process, target)}
                row["io"] = probe.abstract_map_entries(st, process, target, "io_halfwords")
        rows.append(row)
    return rows


def clear_io_cb(frame, bp_loc, internal_dict):
    print("IO_TRACE clear_io stack=%r" % frame_chain(frame))
    return False


def set_io_cb(frame, bp_loc, internal_dict):
    value = frame.FindVariable("value")
    tag = value_tag(value.Dereference()) if value.IsValid() and (value.GetType().IsReferenceType() or value.GetType().IsPointerType()) else (value_tag(value) if value.IsValid() else None)
    address = frame.FindVariable("address")
    addr = "0x%08X" % address.GetValueAsUnsigned() if address.IsValid() else "?"
    if tag == 0:
        print("IO_TRACE set_io_unknown address=%s stack=%r" % (addr, frame_chain(frame)))
    return False


def __lldb_init_module(debugger, internal_dict):
    target = debugger.GetSelectedTarget()
    bp_clear = target.BreakpointCreateByRegex(".*AbstractState.*clear_io")
    bp_clear.SetScriptCallbackFunction("leafgreen_io_trace.clear_io_cb")
    bp_clear.SetAutoContinue(True)
    bp_set = target.BreakpointCreateByRegex(".*AbstractState.*set_io_halfword")
    bp_set.SetScriptCallbackFunction("leafgreen_io_trace.set_io_cb")
    bp_set.SetAutoContinue(True)
    print("IO_TRACE breakpoints clear=%s set=%s" % (bp_clear.GetNumLocations(), bp_set.GetNumLocations()))
