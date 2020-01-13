/// A type for representing strings. Implementation is an immutable wrapper around Array<u8>.
use std::fmt;
use std::hash::{Hash, Hasher};
use std::slice;
use std::str;

use crate::error::{ErrorKind, RuntimeError};
use crate::hashable::Hashable;
use crate::memory::MutatorView;
use crate::printer::Print;
use crate::rawarray::{ArraySize, RawArray};
use crate::safeptr::MutatorScope;

/// While Text is somewhat similar to Symbol, it is instead garbage-collected heap allocated and not interned.
#[derive(Clone)]
pub struct Text {
    content: RawArray<u8>,
}

impl Text {
    /// The originating &str must be owned by a SymbolMap hash table
    pub fn new_empty() -> Text {
        Text {
            content: RawArray::new(),
        }
    }

    /// Initialize a Text object from a &str slice
    pub fn new_from_str<'guard>(
        mem: &'guard MutatorView,
        from_str: &str,
    ) -> Result<Text, RuntimeError> {
        let len = from_str.len();
        let from_ptr = from_str.as_ptr();

        if len > (ArraySize::max_value() as usize) {
            return Err(RuntimeError::new(ErrorKind::BadAllocationRequest));
        }

        let content = RawArray::with_capacity(mem, len as ArraySize)?;

        if let Some(to_ptr) = content.as_ptr() {
            unsafe { from_ptr.copy_to_nonoverlapping(to_ptr as *mut u8, len) }
            Ok(Text { content: content })
        } else {
            panic!("Text content array expected to have backing storage")
        }
    }

    unsafe fn unguarded_as_str(&self) -> &str {
        if let Some(ptr) = self.content.as_ptr() {
            let slice = slice::from_raw_parts(ptr, self.content.capacity() as usize);
            str::from_utf8(slice).unwrap()
        } else {
            &""
        }
    }

    pub fn as_str<'guard>(&self, _guard: &'guard dyn MutatorScope) -> &str {
        unsafe { self.unguarded_as_str() }
    }
}

impl Print for Text {
    fn print<'guard>(
        &self,
        guard: &'guard dyn MutatorScope,
        f: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(f, "{}", self.as_str(guard))
    }
}

impl Hashable for Text {
    fn hash<'guard, H: Hasher>(&self, guard: &'guard dyn MutatorScope, h: &mut H) {
        self.as_str(guard).hash(h)
    }
}

#[cfg(test)]
mod test {
    use super::Text;
    use crate::error::RuntimeError;
    use crate::memory::{Memory, Mutator, MutatorView};

    #[test]
    fn text_empty_string() {
        let mem = Memory::new();

        struct Test {}
        impl Mutator for Test {
            type Input = ();
            type Output = ();

            fn run(
                &self,
                view: &MutatorView,
                _input: Self::Input,
            ) -> Result<Self::Output, RuntimeError> {
                let text = Text::new_empty();
                assert!(text.as_str(view) == "");

                Ok(())
            }
        }

        let test = Test {};
        mem.mutate(&test, ()).unwrap();
    }

    #[test]
    fn text_nonempty_string() {
        let mem = Memory::new();

        struct Test {}
        impl Mutator for Test {
            type Input = ();
            type Output = ();

            fn run(
                &self,
                view: &MutatorView,
                _input: Self::Input,
            ) -> Result<Self::Output, RuntimeError> {
                let expected = "こんにちは";
                let text = Text::new_from_str(view, expected)?;
                let got = text.as_str(view);

                assert!(got == expected);

                Ok(())
            }
        }

        let test = Test {};
        mem.mutate(&test, ()).unwrap();
    }
}
