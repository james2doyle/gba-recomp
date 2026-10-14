//! Native context menu (SPEC-063 CTX-63-01).
//!
//! macOS: an `NSMenu` built through the Objective-C runtime and shown at the
//! mouse position; the call is modal and returns the chosen action id.
//! Other systems have no adapter yet: [`available`] is false and the caller
//! opens the in-window menu instead.

pub enum CtxItem {
    Action {
        id: u32,
        title: String,
        checked: bool,
    },
    Separator,
    Submenu {
        title: String,
        items: Vec<CtxItem>,
    },
}

pub fn action(id: u32, title: impl Into<String>, checked: bool) -> CtxItem {
    CtxItem::Action {
        id,
        title: title.into(),
        checked,
    }
}

pub fn submenu(title: impl Into<String>, items: Vec<CtxItem>) -> CtxItem {
    CtxItem::Submenu {
        title: title.into(),
        items,
    }
}

#[cfg(target_os = "macos")]
pub fn available() -> bool {
    true
}

#[cfg(target_os = "macos")]
pub fn popup(items: &[CtxItem]) -> Option<u32> {
    macos::popup(items)
}

#[cfg(not(target_os = "macos"))]
pub fn available() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub fn popup(_items: &[CtxItem]) -> Option<u32> {
    None
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod macos {
    use std::ffi::{CString, c_char, c_void};
    use std::sync::Once;
    use std::sync::atomic::{AtomicI64, Ordering};

    use super::CtxItem;

    type Id = *mut c_void;
    type Sel = *mut c_void;
    type Class = *mut c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct NSPoint {
        x: f64,
        y: f64,
    }

    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> Class;
        fn sel_registerName(name: *const c_char) -> Sel;
        fn objc_msgSend();
        fn objc_allocateClassPair(superclass: Class, name: *const c_char, extra: usize) -> Class;
        fn class_addMethod(cls: Class, name: Sel, imp: *const c_void, types: *const c_char)
        -> bool;
        fn objc_registerClassPair(cls: Class);
        fn objc_autoreleasePoolPush() -> *mut c_void;
        fn objc_autoreleasePoolPop(pool: *mut c_void);
    }

    #[link(name = "AppKit", kind = "framework")]
    unsafe extern "C" {}

    /// Tag of the item chosen in the last popup (-1 = none).
    static SELECTED: AtomicI64 = AtomicI64::new(-1);
    static REGISTER: Once = Once::new();

    fn sel(name: &str) -> Sel {
        let c = CString::new(name).expect("selector has no NUL");
        // SAFETY: registers/looks up a selector from a valid C string.
        unsafe { sel_registerName(c.as_ptr()) }
    }

    fn class(name: &str) -> Class {
        let c = CString::new(name).expect("class name has no NUL");
        // SAFETY: looks up a class by valid C string; null if missing.
        unsafe { objc_getClass(c.as_ptr()) }
    }

    // Typed views of objc_msgSend for the signatures we use. On arm64 and
    // x86_64 these small return types use the plain (non-stret) entry point.
    macro_rules! send {
        ($recv:expr, $sel:expr $(, $arg:expr => $ty:ty)* ; -> $ret:ty) => {{
            // SAFETY: objc_msgSend is called with the exact argument and
            // return types of the target method.
            let f: unsafe extern "C" fn(Id, Sel $(, $ty)*) -> $ret =
                unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
            unsafe { f($recv, $sel $(, $arg)*) }
        }};
    }

    extern "C" fn menu_action(_this: Id, _cmd: Sel, sender: Id) {
        let tag = send!(sender, sel("tag"); -> isize);
        SELECTED.store(tag as i64, Ordering::SeqCst);
    }

    fn target_class() -> Class {
        REGISTER.call_once(|| {
            let name = CString::new("GbaRecompMenuTarget").expect("static");
            // SAFETY: creates and registers a new NSObject subclass with one
            // method `menuAction:` (types "v@:@") implemented by `menu_action`.
            unsafe {
                let cls = objc_allocateClassPair(class("NSObject"), name.as_ptr(), 0);
                if !cls.is_null() {
                    let types = CString::new("v@:@").expect("static");
                    class_addMethod(
                        cls,
                        sel("menuAction:"),
                        menu_action as *const c_void,
                        types.as_ptr(),
                    );
                    objc_registerClassPair(cls);
                }
            }
        });
        class("GbaRecompMenuTarget")
    }

    fn ns_string(text: &str) -> Id {
        let c = CString::new(text.replace('\0', "")).expect("NUL removed");
        send!(class("NSString"), sel("stringWithUTF8String:"), c.as_ptr() => *const c_char; -> Id)
    }

    fn build(items: &[CtxItem], target: Id) -> Id {
        let menu: Id = send!(class("NSMenu"), sel("alloc"); -> Id);
        let menu: Id = send!(menu, sel("initWithTitle:"), ns_string("") => Id; -> Id);
        send!(menu, sel("setAutoenablesItems:"), false => bool; -> ());
        for item in items {
            match item {
                CtxItem::Separator => {
                    let sep: Id = send!(class("NSMenuItem"), sel("separatorItem"); -> Id);
                    send!(menu, sel("addItem:"), sep => Id; -> ());
                }
                CtxItem::Action { id, title, checked } => {
                    let mi: Id = send!(class("NSMenuItem"), sel("alloc"); -> Id);
                    let mi: Id = send!(
                        mi,
                        sel("initWithTitle:action:keyEquivalent:"),
                        ns_string(title) => Id,
                        sel("menuAction:") => Sel,
                        ns_string("") => Id;
                        -> Id
                    );
                    send!(mi, sel("setTarget:"), target => Id; -> ());
                    send!(mi, sel("setTag:"), *id as isize => isize; -> ());
                    send!(mi, sel("setState:"), isize::from(*checked) => isize; -> ());
                    send!(mi, sel("setEnabled:"), true => bool; -> ());
                    send!(menu, sel("addItem:"), mi => Id; -> ());
                    send!(mi, sel("release"); -> ());
                }
                CtxItem::Submenu { title, items } => {
                    let mi: Id = send!(class("NSMenuItem"), sel("alloc"); -> Id);
                    let mi: Id = send!(
                        mi,
                        sel("initWithTitle:action:keyEquivalent:"),
                        ns_string(title) => Id,
                        std::ptr::null_mut() => Sel,
                        ns_string("") => Id;
                        -> Id
                    );
                    let sub = build(items, target);
                    send!(sub, sel("setTitle:"), ns_string(title) => Id; -> ());
                    send!(mi, sel("setSubmenu:"), sub => Id; -> ());
                    send!(sub, sel("release"); -> ());
                    send!(menu, sel("addItem:"), mi => Id; -> ());
                    send!(mi, sel("release"); -> ());
                }
            }
        }
        menu
    }

    pub fn popup(items: &[CtxItem]) -> Option<u32> {
        // SAFETY: autorelease pool brackets all temporary Cocoa objects.
        let pool = unsafe { objc_autoreleasePoolPush() };
        SELECTED.store(-1, Ordering::SeqCst);
        let cls = target_class();
        let result = if cls.is_null() {
            None
        } else {
            let target: Id = send!(cls, sel("alloc"); -> Id);
            let target: Id = send!(target, sel("init"); -> Id);
            let menu = build(items, target);
            let at: NSPoint = send!(class("NSEvent"), sel("mouseLocation"); -> NSPoint);
            let _shown: bool = send!(
                menu,
                sel("popUpMenuPositioningItem:atLocation:inView:"),
                std::ptr::null_mut() => Id,
                at => NSPoint,
                std::ptr::null_mut() => Id;
                -> bool
            );
            send!(menu, sel("release"); -> ());
            send!(target, sel("release"); -> ());
            let tag = SELECTED.load(Ordering::SeqCst);
            u32::try_from(tag).ok()
        };
        // SAFETY: pops the pool pushed above.
        unsafe { objc_autoreleasePoolPop(pool) };
        result
    }
}
