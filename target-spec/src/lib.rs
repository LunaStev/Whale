// SPDX-License-Identifier: MPL-2.0

//! Output target contracts, independent of the build host and code generation.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Endian {
    Little,
    Big,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DataLayout {
    pub ptr_bits: u32,
    pub endian: Endian,
}

impl DataLayout {
    /// The initial AMD64 output layout, regardless of the build host.
    pub fn default_64bit_le() -> Self {
        Target::X86_64WhaleLinux.data_layout()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObjectFormat {
    ELF64,
}

/// Machine identity is not a promise that an encoder exists for that machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Machine {
    X86_64,
    AArch64,
    RiscV64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ObjectTarget {
    pub format: ObjectFormat,
    pub machine: Machine,
    pub endian: Endian,
    pub address_bits: u32,
}

impl ObjectTarget {
    /// Reject unsupported combinations before interpreting relocation kinds or bytes.
    pub fn validate(self) -> Result<(), TargetError> {
        if self == Target::X86_64WhaleLinux.object_target() {
            Ok(())
        } else {
            Err(TargetError::UnsupportedObjectTarget(self))
        }
    }

    /// Return an ELF machine code only for a supported ELF encoding contract.
    pub fn elf_machine(self) -> Result<u16, TargetError> {
        self.validate()?;
        match self.machine {
            Machine::X86_64 => Ok(62), // EM_X86_64
            _ => Err(TargetError::UnsupportedObjectTarget(self)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    X86_64WhaleLinux,
}

impl Target {
    pub const SUPPORTED: &'static [Self] = &[Self::X86_64WhaleLinux];

    pub fn lookup(name: &str) -> Result<Self, TargetError> {
        Self::SUPPORTED
            .iter()
            .copied()
            .find(|target| target.name() == name)
            .ok_or_else(|| TargetError::UnsupportedTarget(name.to_owned()))
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::X86_64WhaleLinux => "x86_64-whale-linux",
        }
    }

    pub const fn data_layout(self) -> DataLayout {
        match self {
            Self::X86_64WhaleLinux => DataLayout {
                ptr_bits: 64,
                endian: Endian::Little,
            },
        }
    }

    pub const fn object_target(self) -> ObjectTarget {
        match self {
            Self::X86_64WhaleLinux => ObjectTarget {
                format: ObjectFormat::ELF64,
                machine: Machine::X86_64,
                endian: Endian::Little,
                address_bits: self.data_layout().ptr_bits,
            },
        }
    }

    pub fn validate_layout(self, actual: DataLayout) -> Result<(), TargetError> {
        if actual == self.data_layout() {
            Ok(())
        } else {
            Err(TargetError::LayoutMismatch {
                target: self,
                actual,
            })
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetError {
    UnsupportedTarget(String),
    LayoutMismatch { target: Target, actual: DataLayout },
    UnsupportedObjectTarget(ObjectTarget),
}

impl fmt::Display for TargetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedTarget(name) => {
                write!(f, "unsupported target {name:?}; supported targets: ")?;
                for (i, target) in Target::SUPPORTED.iter().enumerate() {
                    if i != 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", target.name())?;
                }
                Ok(())
            }
            Self::LayoutMismatch { target, actual } => write!(
                f,
                "data layout {actual:?} does not match target {} (expected {:?})",
                target.name(),
                target.data_layout()
            ),
            Self::UnsupportedObjectTarget(target) => write!(
                f,
                "unsupported object target {target:?}; supported object target: {:?}",
                Target::X86_64WhaleLinux.object_target()
            ),
        }
    }
}

impl std::error::Error for TargetError {}
