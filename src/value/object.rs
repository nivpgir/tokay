use std::any::{Any, TypeId};

use super::{Dict, RefValue, Value};
use crate::vm::{Accept, Context, Reject};

// BoxedObject
// ----------------------------------------------------------------------------

pub type BoxedObject = Box<dyn Object>;

// CloneBoxedObject
// ----------------------------------------------------------------------------

pub trait CloneBoxedObject {
    fn clone_dyn(&self) -> BoxedObject;
}

impl<T> CloneBoxedObject for T
where
    T: 'static + Object + Clone,
{
    fn clone_dyn(&self) -> BoxedObject {
        Box::new(self.clone())
    }
}

// Object
// ----------------------------------------------------------------------------

/// Describes an interface to a callable object.
pub trait Object: CloneBoxedObject + std::any::Any + std::fmt::Debug {
    /// Object ID (unique memory address)
    fn id(&self) -> usize {
        self as *const Self as *const () as usize
    }

    /// Object type name.
    fn name(&self) -> &'static str;

    /// Object representation in Tokay code
    fn repr(&self) -> String {
        format!("<{} {:p}>", self.name(), self)
    }

    /// Object as bool
    fn is_true(&self) -> bool {
        true
    }

    /// Object as i64
    fn to_i64(&self) -> i64 {
        0
    }

    /// Object as f64
    fn to_f64(&self) -> f64 {
        0.0
    }

    /// Object as usize
    fn to_usize(&self) -> usize {
        self.id()
    }

    /// Object as String
    fn to_string(&self) -> String {
        self.repr()
    }

    /// Check whether the callable accepts any arguments.
    fn is_callable(&self, with_arguments: bool) -> bool;

    /// Check whether the callable is consuming
    fn is_consuming(&self) -> bool;

    /// Check whether the callable is nullable
    fn is_nullable(&self) -> bool {
        false
    }

    /// Call a value with a given context, argument and named argument set.
    fn call(
        &self,
        _context: &mut Context,
        _args: usize,
        _nargs: Option<Dict>,
    ) -> Result<Accept, Reject> {
        panic!("{} cannot be called.", self.name())
    }
}

// The next piece of code including the comment was kindly borrowed from
// https://gitlab.freedesktop.org/dbus/zbus/-/blob/main/zbus/src/interface.rs#L123
//
// Note: while it is possible to implement this without `unsafe`, it currently requires a helper
// trait with a blanket impl that creates `dyn Any` refs.  It's simpler (and more performant) to
// just check the type ID and do the downcast ourself.
//
// See https://github.com/rust-lang/rust/issues/65991 for a rustc feature that will make it
// possible to get a `dyn Any` ref directly from a `dyn Interface` ref; once that is stable, we can
// remove this unsafe code.
impl dyn Object {
    /// Return Any of self
    pub(crate) fn downcast_ref<T: Any>(&self) -> Option<&T> {
        if <dyn Object as Any>::type_id(self) == TypeId::of::<T>() {
            // SAFETY: If type ID matches, it means object is of type T
            Some(unsafe { &*(self as *const dyn Object as *const T) })
        } else {
            None
        }
    }

    /// Return Any of self
    pub(crate) fn downcast_mut<T: Any>(&mut self) -> Option<&mut T> {
        if <dyn Object as Any>::type_id(self) == TypeId::of::<T>() {
            // SAFETY: If type ID matches, it means object is of type T
            Some(unsafe { &mut *(self as *mut dyn Object as *mut T) })
        } else {
            None
        }
    }
}

/*
Value could make use of BoxedObject as a trait object, but this requires implementation
of several other trait on BoxedObject. But this looses the possibility of doing PartialEq
and PartialOrd on the current implementation, which IS important.

Here is the link for a playground started on this:
https://play.rust-lang.org/?version=stable&mode=debug&edition=2021&gist=4d7fda9b8391506736837f93124a16f4

fixme: Need help with this!
*/

impl Clone for BoxedObject {
    fn clone(&self) -> Self {
        self.clone_dyn()
    }
}

impl PartialEq for BoxedObject {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl PartialOrd for BoxedObject {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.id().partial_cmp(&other.id())
    }
}

// https://github.com/rust-lang/rust/issues/31740#issuecomment-700950186
impl PartialEq<&Self> for BoxedObject {
    fn eq(&self, other: &&Self) -> bool {
        self.id() == other.id()
    }
}

impl<T: Object> From<Box<T>> for RefValue {
    fn from(value: Box<T>) -> Self {
        Value::Object(value).into()
    }
}
