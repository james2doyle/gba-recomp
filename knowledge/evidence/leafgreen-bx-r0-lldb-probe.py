import lldb

def child(v, name):
    return v.GetChildMemberWithName(name)

def enum_tag(v):
    variants = v.GetChildAtIndex(0)
    return variants.GetChildAtIndex(0).GetChildMemberWithName("$discr$").GetValueAsUnsigned()

def btree_set_values(v, process, target):
    if enum_tag(v) == 0:
        return "Unknown"
    variants = v.GetChildAtIndex(0)
    constants = variants.GetChildAtIndex(1).GetChildMemberWithName("value")
    bset = child(child(constants, "__0"), "map")
    count = child(bset, "length").GetValueAsUnsigned()
    root = child(bset, "root")
    some = root.GetChildAtIndex(0).GetChildAtIndex(1)
    nref = child(child(some, "value"), "__0")
    ptr = child(child(nref, "node"), "pointer")
    leaf = ptr.Dereference()
    keys = child(leaf, "keys")
    out = []
    for i in range(count):
        key = keys.GetChildAtIndex(i).GetChildMemberWithName("value").GetChildMemberWithName("value").GetChildMemberWithName("__0").GetValueAsUnsigned()
        out.append(key)
    return out

def abstract_map_entries(state, process, target, field="memory_words"):
    arc = child(state, field)
    arc_ptr = child(child(arc, "ptr"), "pointer")
    inner_type = arc_ptr.GetType().GetPointeeType()
    inner = target.CreateValueFromAddress("memory_arc_inner", lldb.SBAddress(arc_ptr.GetValueAsUnsigned(), target), inner_type)
    mapv = child(inner, "data")
    count = child(mapv, "length").GetValueAsUnsigned()
    if count == 0:
        return []
    root = child(mapv, "root")
    some = root.GetChildAtIndex(0).GetChildAtIndex(1)
    nref = child(child(some, "value"), "__0")
    ptr = child(child(nref, "node"), "pointer")
    leaf = ptr.Dereference()
    keys = child(leaf, "keys")
    vals = child(leaf, "vals")
    entries = []
    for i in range(count):
        key = keys.GetChildAtIndex(i).GetChildMemberWithName("value").GetChildMemberWithName("value").GetChildMemberWithName("__0").GetValueAsUnsigned()
        val = vals.GetChildAtIndex(i).GetChildMemberWithName("value").GetChildMemberWithName("value").GetChildMemberWithName("__0")
        tag = enum_tag(val)
        if tag == 0:
            val_repr = "Unknown"
        else:
            val_repr = btree_set_values(val, process, target)
        entries.append((key, val_repr))
    return entries

def probe(frame, bp_loc, internal_dict):
    process = frame.GetThread().GetProcess()
    target = process.GetTarget()
    ins = frame.FindVariable("instruction")
    pc = child(ins, "guest_pc").GetValueAsUnsigned()
    enc = child(ins, "encoding").GetValueAsUnsigned()
    state_ptr = frame.FindVariable("state")
    state = state_ptr.Dereference()
    regs = child(state, "registers")
    values = {}
    for i in [0, 4, 5, 13, 14]:
        av = regs.GetChildAtIndex(i)
        tag = enum_tag(av)
        values[f"r{i}"] = "Unknown" if tag == 0 else btree_set_values(av, process, target)
    try:
        shadow = abstract_map_entries(state, process, target)
        io = abstract_map_entries(state, process, target, "io_halfwords")
    except Exception as exc:
        shadow = f"unavailable: {exc}"
        io = f"unavailable: {exc}"
    extra = {}
    for name in ["address", "exact_address", "store_keeps_irq_quiescent", "register", "offset", "register_list"]:
        local = frame.FindVariable(name)
        if local.IsValid():
            if name == "address" and local.GetTypeName() == "gba_recomp::discover::AbstractValue":
                try:
                    extra[name] = btree_set_values(local, process, target)
                except Exception as exc:
                    extra[name] = f"unavailable: {exc}"
            else:
                extra[name] = local.GetValue() or local.GetSummary()
    print(f"PROBE pc=0x{pc:08X} enc=0x{enc:04X} regs={values} locals={extra} shadow={shadow} io={io}")
    return False
