import lldb
import sys
sys.path.insert(0, "/tmp")
import leafgreen_lldb_probe
WANTED = {0x08000000, 0x08000208, 0x0800020C, 0x08000210, 0x08000214, 0x08000218, 0x0800021C, 0x08000230, 0x08000B48, 0x08000B50, 0x08000B66, 0x08000B68, 0x08000B6A}


def callback(frame, bp_loc, internal_dict):
    ins = frame.FindVariable("instruction")
    if not ins.IsValid():
        return False
    pc = ins.GetChildMemberWithName("guest_pc").GetValueAsUnsigned()
    if pc not in WANTED:
        return False
    expression = frame.EvaluateExpression("*(unsigned long *)&state->registers[13] == 0")
    print("DISCRIMINANT_PROBE pc=0x%08X expression=%s error=%s" % (pc, expression.GetValue() or expression.GetSummary(), expression.GetError().GetCString() if expression.GetError().Fail() else "none"))
    return leafgreen_lldb_probe.probe(frame, bp_loc, internal_dict)


def add(target, line):
    bp = target.BreakpointCreateByLocation("discover.rs", line)
    bp.SetScriptCallbackFunction("leafgreen_discriminant_probe.callback")
    bp.SetAutoContinue(True)
    return bp


def __lldb_init_module(debugger, internal_dict):
    target = debugger.GetSelectedTarget()
    bps = [add(target, line) for line in [1053, 1477, 1579, 1601, 1442]]
    print("DISCRIMINANT_PROBE breakpoints=%s" % [bp.GetNumLocations() for bp in bps])
