pub mod core;
// pub mod backends;

pub use core::linker::Linker;

mod diagnostic;

pub use diagnostic::{LinkError, LinkErrorKind};
