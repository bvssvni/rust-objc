use std::ffi::CStr;
use std::fmt;
use std::str;
use libc::{c_char, c_void};
use malloc_buf::MallocBuffer;

use block::Block;
use runtime::{Class, Object, Sel};

#[cfg(target_pointer_width = "64")]
const CODE_INLINE_CAP: usize = 30;

#[cfg(target_pointer_width = "32")]
const CODE_INLINE_CAP: usize = 14;

enum Code {
    Slice(&'static str),
    Owned(String),
    Inline(u8, [u8; CODE_INLINE_CAP]),
    Malloc(MallocBuffer<u8>)
}

/// An Objective-C type encoding.
///
/// For more information, see Apple's documentation:
/// https://developer.apple.com/library/mac/documentation/Cocoa/Conceptual/ObjCRuntimeGuide/Articles/ocrtTypeEncodings.html
pub struct Encoding {
    code: Code,
}

impl Encoding {
    /// Constructs an `Encoding` from its string representation.
    /// Unsafe because the caller must ensure the string is a valid encoding.
    pub unsafe fn from_str(code: &str) -> Encoding {
        from_str(code)
    }

    /// Returns self as a `str`.
    pub fn as_str(&self) -> &str {
        match self.code {
            Code::Slice(code) => code,
            Code::Owned(ref code) => code,
            Code::Inline(len, ref bytes) => unsafe {
                str::from_utf8_unchecked(&bytes[..len as usize])
            },
            Code::Malloc(ref buf) => unsafe {
                str::from_utf8_unchecked(&buf[..buf.len() - 1])
            },
        }
    }
}

impl Clone for Encoding {
    fn clone(&self) -> Encoding {
        if let Code::Slice(code) = self.code {
            from_static_str(code)
        } else {
            from_str(self.as_str())
        }
    }
}

impl PartialEq for Encoding {
    fn eq(&self, other: &Encoding) -> bool {
        self.as_str() == other.as_str()
    }
}

impl fmt::Debug for Encoding {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

pub fn from_static_str(code: &'static str) -> Encoding {
    Encoding { code: Code::Slice(code) }
}

pub fn from_str(code: &str) -> Encoding {
    if code.len() > CODE_INLINE_CAP {
        Encoding { code: Code::Owned(code.to_string()) }
    } else {
        let mut bytes = [0; CODE_INLINE_CAP];
        for (dst, byte) in bytes.iter_mut().zip(code.bytes()) {
            *dst = byte;
        }
        Encoding { code: Code::Inline(code.len() as u8, bytes) }
    }
}

pub unsafe fn from_malloc_str(ptr: *mut c_char) -> Encoding {
    let s = CStr::from_ptr(ptr);
    let bytes = s.to_bytes_with_nul();
    assert!(str::from_utf8(bytes).is_ok());
    let buf = MallocBuffer::new(ptr as *mut u8, bytes.len()).unwrap();
    Encoding { code: Code::Malloc(buf) }
}

/// Types that have an Objective-C type encoding.
///
/// Unsafe because Objective-C will make assumptions about the type (like its
/// size and alignment) from its encoding, so the implementer must verify that
/// the encoding is accurate.
pub unsafe trait Encode : Sized {
    /// Returns the Objective-C type encoding for Self.
    fn encode() -> Encoding;
}

macro_rules! encode_impls {
    ($($t:ty : $s:expr,)*) => ($(
        unsafe impl Encode for $t {
            fn encode() -> Encoding { from_static_str($s) }
        }
    )*);
}

encode_impls!(
    i8: "c",
    i16: "s",
    i32: "i",
    i64: "q",
    u8: "C",
    u16: "S",
    u32: "I",
    u64: "Q",
    f32: "f",
    f64: "d",
    bool: "B",
    (): "v",
    *mut c_char: "*",
    *const c_char: "r*",
    *mut c_void: "^v",
    *const c_void: "r^v",
    Sel: ":",
);

unsafe impl Encode for isize {
    #[cfg(target_pointer_width = "32")]
    fn encode() -> Encoding { i32::encode() }

    #[cfg(target_pointer_width = "64")]
    fn encode() -> Encoding { i64::encode() }
}

unsafe impl Encode for usize {
    #[cfg(target_pointer_width = "32")]
    fn encode() -> Encoding { u32::encode() }

    #[cfg(target_pointer_width = "64")]
    fn encode() -> Encoding { u64::encode() }
}

macro_rules! encode_message_impl {
    ($code:expr, $name:ident) => (
        encode_message_impl!($code, $name,);
    );
    ($code:expr, $name:ident, $($t:ident),*) => (
        unsafe impl<'a $(, $t)*> $crate::Encode for &'a $name<$($t),*> {
            fn encode() -> Encoding { from_static_str($code) }
        }

        unsafe impl<'a $(, $t)*> $crate::Encode for &'a mut $name<$($t),*> {
            fn encode() -> Encoding { from_static_str($code) }
        }

        unsafe impl<'a $(, $t)*> $crate::Encode for Option<&'a $name<$($t),*>> {
            fn encode() -> Encoding { from_static_str($code) }
        }

        unsafe impl<'a $(, $t)*> $crate::Encode for Option<&'a mut $name<$($t),*>> {
            fn encode() -> Encoding { from_static_str($code) }
        }

        unsafe impl<$($t),*> $crate::Encode for *const $name<$($t),*> {
            fn encode() -> Encoding { from_static_str($code) }
        }

        unsafe impl<$($t),*> $crate::Encode for *mut $name<$($t),*> {
            fn encode() -> Encoding { from_static_str($code) }
        }
    );
}

encode_message_impl!("@", Object);

encode_message_impl!("#", Class);

encode_message_impl!("@?", Block, A, R);

#[cfg(test)]
mod tests {
    use runtime::{Class, Object, Sel};
    use super::Encode;

    #[test]
    fn test_encode() {
        assert!(u32::encode().as_str() == "I");
        assert!(<()>::encode().as_str() == "v");
        assert!(<&Object>::encode().as_str() == "@");
        assert!(<*mut Object>::encode().as_str() == "@");
        assert!(<&Class>::encode().as_str() == "#");
        assert!(Sel::encode().as_str() == ":");
    }

    #[test]
    fn test_inline_encoding() {
        let enc = super::from_str("C");
        assert!(enc.as_str() == "C");

        let enc2 = enc.clone();
        assert!(enc2 == enc);
        assert!(enc2.as_str() == "C");
    }

    #[test]
    fn test_owned_encoding() {
        let s = "{Test=CCCCCCCCCCCCCCCCCCCCCCCCC}";
        let enc = super::from_str(s);
        assert!(enc.as_str() == s);

        let enc2 = enc.clone();
        assert!(enc2 == enc);
        assert!(enc2.as_str() == s);
    }
}
