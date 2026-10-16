// SPDX-License-Identifier: MPL-2.0
//! Synthetic 64-bit addresses, byte initialization and separately copied capabilities.
use super::{integer, FunctionPointer, FunctionTarget, Value};

use crate::{ConstValue, Target, Type, TypeLayout};

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

static NEXT_ARENA: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug)]
pub struct MemoryLimits {
    pub max_bytes: u64,
    pub max_allocations: usize,
    pub max_pointer_fragments: usize,
    /// Bytes processed plus metadata entries examined, independently of IR steps.
    pub max_work: u64,
}

impl Default for MemoryLimits {
    fn default() -> Self {
        Self {
            max_bytes: 64 * 1024 * 1024,
            max_allocations: 16_384,
            max_pointer_fragments: 262_144,
            max_work: 256 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryResource {
    Bytes,
    Allocations,
    PointerFragments,
    Work,
    HostAllocation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemoryTrap {
    NullPointer,
    MissingProvenance,
    ExpiredAllocation,
    OutOfBounds,
    AddressOverflow,
    PermissionDenied,
    Misaligned { address: u64, alignment: u32 },
    Uninitialized { offset: u64 },
    InvalidBool { byte: u8 },
    OverlappingCopy,
    DifferentAllocation,
    InvalidLifetimeEnd,
}

impl std::fmt::Display for MemoryTrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NullPointer => f.write_str("null pointer access"),
            Self::MissingProvenance => f.write_str("pointer has no tracked allocation provenance"),
            Self::ExpiredAllocation => f.write_str("allocation lifetime has ended"),
            Self::OutOfBounds => f.write_str("pointer range is outside its allocation"),
            Self::AddressOverflow => f.write_str("64-bit address arithmetic overflow"),
            Self::PermissionDenied => f.write_str("pointer access permission denied"),
            Self::Misaligned { address, alignment } => {
                write!(f, "address {address:#x} does not satisfy alignment {alignment}")
            },
            Self::Uninitialized { offset } => {
                write!(f, "uninitialized byte at allocation offset {offset}")
            },
            Self::InvalidBool { byte } => write!(f, "invalid Bool storage byte {byte:#x}"),
            Self::OverlappingCopy => f.write_str("overlapping memcpy ranges"),
            Self::DifferentAllocation => {
                f.write_str("pointer operands belong to different allocations")
            },
            Self::InvalidLifetimeEnd => f.write_str(
                "lifetime_end requires the current frame\'s exact stack allocation base",
            ),
        }
    }
}

#[derive(Debug)]
pub(super) enum Fault {
    Trap(MemoryTrap),
    Limit { resource: MemoryResource, limit: u64 },
}

impl From<MemoryTrap> for Fault {
    fn from(t: MemoryTrap) -> Self {
        Self::Trap(t)
    }
}

type Result<T> = std::result::Result<T, Fault>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Capability {
    arena: u64,
    allocation: usize,
    generation: u64,
    lower: u64,
    upper: u64,
    offset: u64,
    read: bool,
    write: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Pointer {
    pub address: u64,
    capability: Option<Capability>,
}

impl Pointer {
    pub fn raw(address: u64) -> Self {
        Self { address, capability: None }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum StoredPointer {
    Data(Pointer),
    Function(FunctionPointer),
}

impl StoredPointer {
    fn bits(&self) -> u64 {
        match self {
            Self::Data(p) => p.address,
            Self::Function(p) => p.bits(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Fragment {
    pointer: StoredPointer,
    byte: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AllocationKind {
    Stack,
    Global,
}

struct Allocation {
    kind: AllocationKind,
    address: u64,
    generation: u64,
    alive: bool,
    read: bool,
    write: bool,
    bytes: Vec<u8>,
    initialized: Vec<u8>,
    fragments: HashMap<usize, Fragment>,
}

#[derive(Clone, Debug)]
pub(super) enum GepStep {
    Stride(u64),
    Field(u64),
}

pub(super) struct Memory {
    arena: u64,
    allocations: Vec<Allocation>,
    limits: MemoryLimits,
    bytes: u64,
    fragments: usize,
    work: u64,
    next_address: u64,
}

fn layout(ty: &Type) -> TypeLayout {
    crate::layout_of_with_limit(ty, Target::X86_64WhaleLinux, crate::MAX_IR_NESTING)
        .expect("bounded layout checked before execution")
}

impl Memory {
    pub fn new(limits: MemoryLimits) -> Self {
        Self {
            arena: NEXT_ARENA.fetch_add(1, Ordering::Relaxed),
            allocations: Vec::new(),
            limits,
            bytes: 0,
            fragments: 0,
            work: 0,
            next_address: 0x1000,
        }
    }

    fn limit(resource: MemoryResource, limit: u64) -> Fault {
        Fault::Limit { resource, limit }
    }

    fn charge(&mut self, work: u64) -> Result<()> {
        self.work = self
            .work
            .checked_add(work)
            .filter(|n| *n <= self.limits.max_work)
            .ok_or_else(|| Self::limit(MemoryResource::Work, self.limits.max_work))?;
        Ok(())
    }

    pub fn allocate(&mut self, size: u64, natural: u32, align: u32) -> Result<Pointer> {
        if self.allocations.len() >= self.limits.max_allocations {
            return Err(Self::limit(
                MemoryResource::Allocations,
                self.limits.max_allocations as u64,
            ));
        }
        let bytes = self
            .bytes
            .checked_add(size)
            .filter(|n| *n <= self.limits.max_bytes)
            .ok_or_else(|| Self::limit(MemoryResource::Bytes, self.limits.max_bytes))?;
        let length =
            usize::try_from(size).map_err(|_| Self::limit(MemoryResource::HostAllocation, size))?;
        let alignment = u64::from(align.max(natural));
        let address = self
            .next_address
            .checked_add(alignment - 1)
            .map(|a| a & !(alignment - 1))
            .ok_or(MemoryTrap::AddressOverflow)?;
        let next = address
            .checked_add(size.max(1))
            .and_then(|a| a.checked_add(1))
            .ok_or(MemoryTrap::AddressOverflow)?;
        self.charge(
            size.checked_mul(2)
                .and_then(|n| n.checked_add(1))
                .ok_or(MemoryTrap::AddressOverflow)?,
        )?;
        let mut data = Vec::new();
        let mut initialized = Vec::new();
        data.try_reserve_exact(length)
            .map_err(|_| Self::limit(MemoryResource::HostAllocation, size))?;
        initialized
            .try_reserve_exact(length)
            .map_err(|_| Self::limit(MemoryResource::HostAllocation, size))?;
        self.allocations
            .try_reserve(1)
            .map_err(|_| Self::limit(MemoryResource::HostAllocation, size))?;
        // Physical zero bytes are not semantic initialization.
        data.resize(length, 0);
        initialized.resize(length, 0);
        let capability = Capability {
            arena: self.arena,
            allocation: self.allocations.len(),
            generation: 0,
            lower: 0,
            upper: size,
            offset: 0,
            read: true,
            write: true,
        };
        self.allocations.push(Allocation {
            kind: AllocationKind::Stack,
            address,
            generation: 0,
            alive: true,
            read: true,
            write: true,
            bytes: data,
            initialized,
            fragments: HashMap::new(),
        });
        self.bytes = bytes;
        self.next_address = next;
        Ok(Pointer { address, capability: Some(capability) })
    }

    pub fn allocate_global(&mut self, size: u64, natural: u32, align: u32) -> Result<Pointer> {
        let pointer = self.allocate(size, natural, align)?;
        self.allocations[pointer.capability.unwrap().allocation].kind = AllocationKind::Global;
        Ok(pointer)
    }

    pub fn seal_global(&mut self, mut pointer: Pointer, readonly: bool) -> Result<Pointer> {
        let (id, _) = self.identity(pointer)?;
        if self.allocations[id].kind != AllocationKind::Global {
            return Err(MemoryTrap::InvalidLifetimeEnd.into());
        }
        self.allocations[id].write = !readonly;
        pointer.capability.as_mut().unwrap().write = !readonly;
        Ok(pointer)
    }

    pub fn retire_stack(&mut self, pointer: Pointer, owner_start: usize) -> Result<()> {
        let (id, capability) = self.identity(pointer)?;
        if id < owner_start
            || self.allocations[id].kind != AllocationKind::Stack
            || capability.offset != 0
            || capability.lower != 0
            || capability.upper != self.allocations[id].bytes.len() as u64
        {
            return Err(MemoryTrap::InvalidLifetimeEnd.into());
        }
        self.charge(1 + self.allocations[id].fragments.len() as u64)?;
        self.retire_allocation(id);
        Ok(())
    }

    fn retire_allocation(&mut self, id: usize) {
        let allocation = &mut self.allocations[id];
        if allocation.alive {
            allocation.alive = false;
            self.fragments -= allocation.fragments.len();
            allocation.fragments.clear();
            allocation.bytes.clear();
            allocation.initialized.clear();
        }
    }

    pub fn difference(&self, lhs: Pointer, rhs: Pointer) -> Result<i128> {
        let (a, left) = self.identity(lhs)?;
        let (b, right) = self.identity(rhs)?;
        if a != b || left.generation != right.generation {
            return Err(MemoryTrap::DifferentAllocation.into());
        }
        Ok(i128::from(left.offset) - i128::from(right.offset))
    }

    pub fn compare_order(&self, lhs: Pointer, rhs: Pointer) -> Result<std::cmp::Ordering> {
        Ok(self.difference(lhs, rhs)?.cmp(&0))
    }

    fn identity(&self, pointer: Pointer) -> Result<(usize, Capability)> {
        if pointer.address == 0 {
            return Err(MemoryTrap::NullPointer.into());
        }
        let cap = pointer.capability.ok_or(MemoryTrap::MissingProvenance)?;
        if cap.arena != self.arena {
            return Err(MemoryTrap::ExpiredAllocation.into());
        }
        let allocation =
            self.allocations.get(cap.allocation).ok_or(MemoryTrap::ExpiredAllocation)?;
        if !allocation.alive || allocation.generation != cap.generation {
            return Err(MemoryTrap::ExpiredAllocation.into());
        }
        if cap.lower > cap.offset
            || cap.offset > cap.upper
            || cap.upper > allocation.bytes.len() as u64
            || allocation.address.checked_add(cap.offset) != Some(pointer.address)
        {
            return Err(MemoryTrap::OutOfBounds.into());
        }
        Ok((cap.allocation, cap))
    }

    fn access(
        &self,
        pointer: Pointer,
        n: u64,
        align: u32,
        write: bool,
    ) -> Result<(usize, usize, usize)> {
        let (id, cap) = self.identity(pointer)?;
        let end = cap.offset.checked_add(n).ok_or(MemoryTrap::AddressOverflow)?;
        if end > cap.upper {
            return Err(MemoryTrap::OutOfBounds.into());
        }
        let allocation = &self.allocations[id];
        let permitted =
            if write { cap.write && allocation.write } else { cap.read && allocation.read };
        if !permitted {
            return Err(MemoryTrap::PermissionDenied.into());
        }
        if pointer.address % u64::from(align) != 0 {
            return Err(
                MemoryTrap::Misaligned { address: pointer.address, alignment: align }.into()
            );
        }
        Ok((id, cap.offset as usize, end as usize))
    }

    pub fn gep(
        &self,
        pointer: Pointer,
        plan: &[GepStep],
        indices: &[ConstValue],
    ) -> Result<Pointer> {
        if indices.is_empty() {
            return Ok(pointer);
        }
        let (_, mut cap) = self.identity(pointer)?;
        let base = i128::from(pointer.address) - i128::from(cap.offset);
        let mut offset = i128::from(cap.offset);
        for (step, index) in plan.iter().zip(indices) {
            let delta = match step {
                GepStep::Field(offset) => i128::from(*offset),
                GepStep::Stride(stride) => {
                    let index = match index {
                        ConstValue::I(v) => *v,
                        ConstValue::U(v) => {
                            i128::try_from(*v).map_err(|_| MemoryTrap::AddressOverflow)?
                        },
                        _ => unreachable!("verified integer GEP index"),
                    };
                    index.checked_mul(i128::from(*stride)).ok_or(MemoryTrap::AddressOverflow)?
                },
            };
            offset = offset.checked_add(delta).ok_or(MemoryTrap::AddressOverflow)?;
            let address = base.checked_add(offset).ok_or(MemoryTrap::AddressOverflow)?;
            if !(0..=i128::from(u64::MAX)).contains(&address) {
                return Err(MemoryTrap::AddressOverflow.into());
            }
        }
        let offset = u64::try_from(offset).map_err(|_| MemoryTrap::OutOfBounds)?;
        if offset < cap.lower || offset > cap.upper {
            return Err(MemoryTrap::OutOfBounds.into());
        }
        let address = u64::try_from(base + i128::from(offset)).unwrap();
        cap.offset = offset;
        Ok(Pointer { address, capability: Some(cap) })
    }

    pub fn equal(&self, lhs: Pointer, rhs: Pointer) -> Result<bool> {
        if lhs.address == 0 && rhs.address == 0 {
            return Ok(true);
        }
        if lhs.address == 0 {
            self.identity(rhs)?;
            return Ok(false);
        }
        if rhs.address == 0 {
            self.identity(lhs)?;
            return Ok(false);
        }
        let (_, a) = self.identity(lhs)?;
        let (_, b) = self.identity(rhs)?;
        Ok((a.allocation, a.generation, a.offset) == (b.allocation, b.generation, b.offset))
    }

    fn replace_fragments(
        &mut self,
        id: usize,
        start: usize,
        end: usize,
        new: &[(usize, Fragment)],
    ) -> Result<()> {
        let allocation = &self.allocations[id];
        self.charge(3 * allocation.fragments.capacity() as u64 + new.len() as u64)?;
        let removed =
            self.allocations[id].fragments.keys().filter(|i| **i >= start && **i < end).count();
        let count = self.fragments - removed + new.len();
        if count > self.limits.max_pointer_fragments {
            return Err(Self::limit(
                MemoryResource::PointerFragments,
                self.limits.max_pointer_fragments as u64,
            ));
        }
        self.allocations[id]
            .fragments
            .try_reserve(new.len())
            .map_err(|_| Self::limit(MemoryResource::HostAllocation, new.len() as u64))?;
        self.allocations[id].fragments.retain(|i, _| *i < start || *i >= end);
        self.allocations[id].fragments.extend(new.iter().cloned());
        self.fragments = count;
        Ok(())
    }

    pub fn uninit(&mut self, pointer: Pointer, size: u64, align: u32) -> Result<()> {
        let (id, start, end) = self.access(pointer, size, align, true)?;
        self.charge((end - start) as u64)?;
        self.replace_fragments(id, start, end, &[])?;
        self.allocations[id].initialized[start..end].fill(0);
        Ok(())
    }

    pub fn copy(&mut self, dst: Pointer, src: Pointer, size: u64, align: u32) -> Result<()> {
        // No access occurs for an empty operation, even for null/raw pointers.
        if size == 0 {
            return Ok(());
        }
        let (source, ss, se) = self.access(src, size, align, false)?;
        let (destination, ds, de) = self.access(dst, size, align, true)?;
        if source == destination && ss < de && ds < se {
            return Err(MemoryTrap::OverlappingCopy.into());
        }
        self.charge(
            size.checked_mul(4)
                .ok_or(MemoryTrap::AddressOverflow)?
                .checked_add(self.allocations[source].fragments.capacity() as u64)
                .ok_or(MemoryTrap::AddressOverflow)?,
        )?;
        let source = &self.allocations[source];
        let mut data = Vec::new();
        let mut initialized = Vec::new();
        let mut fragments = Vec::new();
        let count = source.fragments.len().min(se - ss);
        data.try_reserve_exact(se - ss)
            .map_err(|_| Self::limit(MemoryResource::HostAllocation, size))?;
        initialized
            .try_reserve_exact(se - ss)
            .map_err(|_| Self::limit(MemoryResource::HostAllocation, size))?;
        fragments
            .try_reserve_exact(count)
            .map_err(|_| Self::limit(MemoryResource::HostAllocation, count as u64))?;
        data.extend_from_slice(&source.bytes[ss..se]);
        initialized.extend_from_slice(&source.initialized[ss..se]);
        fragments.extend(
            source
                .fragments
                .iter()
                .filter(|(at, _)| **at >= ss && **at < se)
                .map(|(at, tag)| (ds + (*at - ss), tag.clone())),
        );
        // Reserve/check metadata before any destination bytes are changed.
        self.replace_fragments(destination, ds, de, &fragments)?;
        self.allocations[destination].bytes[ds..de].copy_from_slice(&data);
        self.allocations[destination].initialized[ds..de].copy_from_slice(&initialized);
        Ok(())
    }

    pub fn set(&mut self, dst: Pointer, byte: u8, size: u64, align: u32) -> Result<()> {
        if size == 0 {
            return Ok(());
        }
        let (id, start, end) = self.access(dst, size, align, true)?;
        self.charge(size.checked_mul(2).ok_or(MemoryTrap::AddressOverflow)?)?;
        self.replace_fragments(id, start, end, &[])?;
        self.allocations[id].bytes[start..end].fill(byte);
        self.allocations[id].initialized[start..end].fill(1);
        Ok(())
    }

    #[cfg(test)]
    pub fn load(&mut self, pointer: Pointer, ty: &Type, align: u32) -> Result<Value> {
        self.load_with_signature(pointer, ty, align, None)
    }

    pub fn load_with_signature(
        &mut self,
        pointer: Pointer,
        ty: &Type,
        align: u32,
        signature: Option<&Arc<crate::FunctionSignature>>,
    ) -> Result<Value> {
        let (id, start, end) = self.access(pointer, layout(ty).size, align, false)?;
        self.charge((end - start) as u64)?;
        if let Type::Tuple(fields) = ty {
            let offsets = layout(ty).field_offsets;
            let a = self.load_scalar(id, start + offsets[0] as usize, &fields[0], None)?;
            let b = self.load_scalar(id, start + offsets[1] as usize, &fields[1], None)?;
            return Ok(Value::Checked(a.scalar().clone(), b.boolean()));
        }
        self.load_scalar(id, start, ty, signature)
    }

    fn load_scalar(
        &self,
        id: usize,
        start: usize,
        ty: &Type,
        callable_signature: Option<&Arc<crate::FunctionSignature>>,
    ) -> Result<Value> {
        let allocation = &self.allocations[id];
        let n = layout(ty).size as usize;
        for offset in start..start + n {
            if allocation.initialized[offset] == 0 {
                return Err(MemoryTrap::Uninitialized { offset: offset as u64 }.into());
            }
        }
        let bytes = &allocation.bytes[start..start + n];
        if *ty == Type::Bool {
            return match bytes[0] {
                0 => Ok(Value::Scalar(ConstValue::Bool(false))),
                1 => Ok(Value::Scalar(ConstValue::Bool(true))),
                byte => Err(MemoryTrap::InvalidBool { byte }.into()),
            };
        }
        if matches!(ty, Type::Ptr(_) | Type::FnPtr(_)) {
            let address = u64::from_le_bytes(bytes.try_into().unwrap());
            let complete = allocation.fragments.get(&start).filter(|fragment| {
                fragment.byte == 0
                    && fragment.pointer.bits() == address
                    && (0..8).all(|i| {
                        allocation.fragments.get(&(start + i))
                            == Some(&Fragment { pointer: fragment.pointer.clone(), byte: i as u8 })
                    })
            });
            return Ok(match ty {
                Type::Ptr(_) => Value::Pointer(if address == 0 {
                    Pointer::raw(0)
                } else {
                    match complete.map(|f| &f.pointer) {
                        Some(StoredPointer::Data(pointer)) => *pointer,
                        _ => Pointer::raw(address),
                    }
                }),
                Type::FnPtr(signature) => {
                    let signature = callable_signature
                        .cloned()
                        .unwrap_or_else(|| Arc::new((**signature).clone()));
                    let target = if address == 0 {
                        FunctionTarget::Null
                    } else {
                        match complete.map(|f| &f.pointer) {
                            Some(StoredPointer::Function(pointer))
                                if pointer.signature == signature =>
                            {
                                return Ok(Value::FunctionPointer(pointer.clone()))
                            },
                            _ => FunctionTarget::InvalidBits(address),
                        }
                    };
                    Value::FunctionPointer(FunctionPointer { signature, target })
                },
                _ => unreachable!(),
            });
        }
        let mut bits = [0u8; 16];
        bits[..n].copy_from_slice(bytes);
        Ok(Value::Scalar(integer::pack(ty, u128::from_le_bytes(bits))))
    }

    pub fn store(&mut self, pointer: Pointer, ty: &Type, value: &Value, align: u32) -> Result<()> {
        let (id, start, end) = self.access(pointer, layout(ty).size, align, true)?;
        let mut bytes = Vec::new();
        let mut fragments = Vec::new();
        if let (Type::Tuple(fields), Value::Checked(v, flag)) = (ty, value) {
            let offsets = layout(ty).field_offsets;
            Self::encode(
                &fields[0],
                &Value::Scalar(v.clone()),
                start + offsets[0] as usize,
                &mut bytes,
                &mut fragments,
            );
            Self::encode(
                &fields[1],
                &Value::Scalar(ConstValue::Bool(*flag)),
                start + offsets[1] as usize,
                &mut bytes,
                &mut fragments,
            );
        } else {
            Self::encode(ty, value, start, &mut bytes, &mut fragments);
        }
        self.charge((end - start) as u64)?;
        self.replace_fragments(id, start, end, &fragments)?;
        for (at, byte) in bytes {
            self.allocations[id].bytes[at] = byte;
            self.allocations[id].initialized[at] = 1;
        }
        Ok(())
    }

    fn encode(
        ty: &Type,
        value: &Value,
        start: usize,
        bytes: &mut Vec<(usize, u8)>,
        fragments: &mut Vec<(usize, Fragment)>,
    ) {
        let n = layout(ty).size as usize;
        let data = match value {
            Value::Pointer(p) => {
                for i in 0..8 {
                    fragments.push((
                        start + i,
                        Fragment { pointer: StoredPointer::Data(*p), byte: i as u8 },
                    ));
                }
                u128::from(p.address).to_le_bytes()
            },
            Value::FunctionPointer(p) => {
                for i in 0..8 {
                    fragments.push((
                        start + i,
                        Fragment { pointer: StoredPointer::Function(p.clone()), byte: i as u8 },
                    ));
                }
                u128::from(p.bits()).to_le_bytes()
            },
            Value::Scalar(v) => {
                integer::raw(v, integer::shape(ty).map_or(1, |s| s.0)).to_le_bytes()
            },
            _ => unreachable!("verified scalar or checked storage"),
        };
        bytes.extend(data[..n].iter().enumerate().map(|(i, b)| (start + i, *b)));
    }

    pub fn allocation_count(&self) -> usize {
        self.allocations.len()
    }

    pub fn retire_from(&mut self, start: usize) -> Result<()> {
        self.charge((self.allocations.len() - start) as u64)?;
        for id in start..self.allocations.len() {
            if self.allocations[id].kind == AllocationKind::Stack {
                self.retire_allocation(id);
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn retire_all(&mut self) {
        self.retire_from(0).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trapped<T>(result: Result<T>, expected: MemoryTrap) {
        assert!(matches!(result, Err(Fault::Trap(t)) if t == expected));
    }

    fn advance(m: &Memory, p: Pointer, n: i128) -> Pointer {
        m.gep(p, &[GepStep::Stride(1)], &[ConstValue::I(n)]).unwrap()
    }

    #[test]
    fn permissions_lifetime_generation_and_restricted_ranges_are_checked() {
        let mut m = Memory::new(MemoryLimits::default());
        let p = m.allocate(8, 8, 8).unwrap();
        m.set(p, 0, 8, 8).unwrap();
        let mut readonly = p;
        readonly.capability.as_mut().unwrap().write = false;
        assert!(m.load(readonly, &Type::U64, 8).is_ok());
        trapped(
            m.store(readonly, &Type::U64, &Value::Scalar(ConstValue::U(0)), 8),
            MemoryTrap::PermissionDenied,
        );
        m.allocations[0].read = false;
        trapped(m.load(p, &Type::U8, 1), MemoryTrap::PermissionDenied);
        m.allocations[0].read = true;
        let mut narrow = p;
        narrow.capability.as_mut().unwrap().upper = 4;
        trapped(m.load(narrow, &Type::U64, 1), MemoryTrap::OutOfBounds);
        m.allocations[0].generation += 1;
        trapped(m.load(p, &Type::U8, 1), MemoryTrap::ExpiredAllocation);
        m.allocations[0].generation -= 1;
        m.retire_all();
        trapped(m.load(p, &Type::U8, 1), MemoryTrap::ExpiredAllocation);
        // Empty copy/set do not access even an expired capability.
        m.copy(p, p, 0, 8).unwrap();
        m.set(p, 1, 0, 8).unwrap();
    }

    #[test]
    fn split_pointer_copies_reassemble_only_matching_byte_fragments() {
        let mut m = Memory::new(MemoryLimits::default());
        let object = m.allocate(4, 4, 4).unwrap();
        let source = m.allocate(8, 8, 8).unwrap();
        let dest = m.allocate(8, 8, 8).unwrap();
        let ty = Type::ptr_to(Type::U32);
        m.store(source, &ty, &Value::Pointer(object), 8).unwrap();
        m.copy(dest, source, 4, 1).unwrap();
        trapped(m.load(dest, &ty, 8), MemoryTrap::Uninitialized { offset: 4 });
        m.copy(advance(&m, dest, 4), advance(&m, source, 4), 4, 1).unwrap();
        assert_eq!(m.load(dest, &ty, 8).unwrap().pointer(), object);
        let p = advance(&m, dest, 7);
        // Replacing even identical bytes destroys that fragment's authority.
        m.store(p, &Type::U8, &Value::Scalar(ConstValue::U(0)), 1).unwrap();
        let raw = m.load(dest, &ty, 8).unwrap().pointer();
        assert_eq!(raw.address, object.address);
        trapped(m.load(raw, &Type::U32, 4), MemoryTrap::MissingProvenance);
    }

    #[test]
    fn failed_copy_limits_and_invalid_ranges_do_not_partially_write() {
        let mut m =
            Memory::new(MemoryLimits { max_pointer_fragments: 8, ..MemoryLimits::default() });
        let object = m.allocate(1, 1, 1).unwrap();
        let source = m.allocate(8, 8, 8).unwrap();
        let dest = m.allocate(8, 8, 8).unwrap();
        m.store(source, &Type::ptr_to(Type::U8), &Value::Pointer(object), 8).unwrap();
        m.set(dest, 255, 8, 8).unwrap();
        assert!(matches!(
            m.copy(dest, source, 8, 8),
            Err(Fault::Limit { resource: MemoryResource::PointerFragments, .. })
        ));
        assert_eq!(m.load(dest, &Type::U64, 8).unwrap().scalar(), &ConstValue::U(u64::MAX.into()));
        trapped(m.copy(dest, source, 9, 1), MemoryTrap::OutOfBounds);
        assert_eq!(m.load(dest, &Type::U64, 8).unwrap().scalar(), &ConstValue::U(u64::MAX.into()));
        let before = m.allocations[2].bytes.clone();
        m.limits.max_work = m.work;
        assert!(matches!(
            m.set(dest, 0, 8, 8),
            Err(Fault::Limit { resource: MemoryResource::Work, .. })
        ));
        assert_eq!(before, m.allocations[2].bytes);
    }

    #[test]
    fn negative_gep_can_return_from_one_past_and_uninit_keeps_physical_bytes() {
        let mut m = Memory::new(MemoryLimits::default());
        let p = m.allocate(8, 8, 8).unwrap();
        let end = advance(&m, p, 8);
        assert_eq!(advance(&m, end, -8), p);
        m.set(p, 42, 8, 1).unwrap();
        m.uninit(p, 8, 1).unwrap();
        assert_eq!(m.allocations[0].bytes, vec![42; 8]);
        trapped(m.load(p, &Type::U8, 1), MemoryTrap::Uninitialized { offset: 0 });
    }
}

#[cfg(test)]
mod generated;
