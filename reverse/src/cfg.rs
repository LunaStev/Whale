// SPDX-License-Identifier: MPL-2.0
//! The host validates/copies buffers; Wave owns decoding, reachability, overlaps,
//! relocation target selection, cross references and block formation.
use crate::{DecodeError, Instruction};

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CfgTarget {
    Address(u64),
    Symbol(u64),
    Unresolved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlowOverride {
    pub field_offset: usize,
    pub width: u8,
    pub target: CfgTarget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CfgLimits {
    pub max_input_bytes: usize,
    pub max_instructions: usize,
    pub max_blocks: usize,
    pub max_edges: usize,
    pub max_stops: usize,
    pub max_work: u64,
    /// Combined owned workspace, output and normalized input buffer limit.
    pub max_workspace_bytes: usize,
    pub follow_calls: bool,
}

impl Default for CfgLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 8 * 1024 * 1024,
            max_instructions: 16384,
            max_blocks: 8192,
            max_edges: 65536,
            max_stops: 8192,
            max_work: 64 * 1024 * 1024,
            max_workspace_bytes: 128 * 1024 * 1024,
            follow_calls: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CfgResource {
    InputBytes,
    Instructions,
    Blocks,
    Edges,
    Stops,
    Work,
    Memory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CfgError {
    CoreUnavailable,
    Invalid(&'static str),
    Limit { resource: CfgResource, limit: u64 },
    Allocation,
    DecodeOutput(DecodeError),
}

impl fmt::Display for CfgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CoreUnavailable => {
                f.write_str("Wave CFG core unavailable; enable the linked Wave reverse core")
            },
            Self::Invalid(reason) => write!(f, "invalid Wave CFG input/result: {reason}"),
            Self::Limit { resource, limit } => {
                write!(f, "Wave CFG {resource:?} limit exceeded ({limit})")
            },
            Self::Allocation => f.write_str("unable to allocate bounded Wave CFG buffers"),
            Self::DecodeOutput(error) => write!(f, "Wave CFG decode record: {error}"),
        }
    }
}

impl std::error::Error for CfgError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Self::DecodeOutput(e) = self {
            Some(e)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeKind {
    FallThrough,
    Call,
    Jump,
    ConditionalTaken,
    ConditionalFallThrough,
    LoopTaken,
    LoopFallThrough,
    IndirectCall,
    IndirectJump,
    Syscall,
    Memory,
    Address,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    pub from: usize,
    pub kind: EdgeKind,
    pub target: CfgTarget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopKind {
    Invalid,
    Truncated,
    Unsupported,
    Overlong,
    Overlap,
    EndOfRange,
    IndirectJump,
    Syscall,
    AddressOverflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stop {
    pub offset: usize,
    pub kind: StopKind,
    pub conflict: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BasicBlock {
    pub offset: usize,
    pub end: usize,
    pub instruction_indices: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlFlowGraph {
    pub base: u64,
    /// Discovery order; basic blocks are separately ordered by byte offset.
    pub instructions: Vec<Instruction>,
    pub blocks: Vec<BasicBlock>,
    pub edges: Vec<Edge>,
    pub stops: Vec<Stop>,
    pub work: u64,
}

/// Analyze one explicitly scoped byte region. Addresses outside it remain edges,
/// never guessed code. Seeds are region offsets. Symbol IDs belong to the host.
/// Overlays name encoded fields; intersections of a different width resolve to
/// Unresolved rather than reading a relocation placeholder as a branch target.
pub fn analyze(
    input: &[u8],
    base: u64,
    seeds: &[usize],
    overlays: &[FlowOverride],
    limits: CfgLimits,
) -> Result<ControlFlowGraph, CfgError> {
    #[cfg(not(whale_wave_reverse))]
    {
        let _ = (input, base, seeds, overlays, limits);
        Err(CfgError::CoreUnavailable)
    }
    #[cfg(whale_wave_reverse)]
    {
        native::analyze(input, base, seeds, overlays, limits)
    }
}

#[cfg(whale_wave_reverse)]
mod native {
    use super::*;

    use crate::{ABI_VERSION, DECODE_WORDS};

    use std::collections::HashSet;

    extern "C" {
        fn whale_wave_reverse_abi() -> u64;

        fn whale_wave_reverse_cfg(
            abi: u64,
            data: *const u8,
            length: u64,
            base: u64,
            seeds: *const u64,
            seed_count: u64,
            overlays: *const u64,
            overlay_count: u64,
            options: *const u64,
            options_count: u64,
            workspace: *mut u64,
            workspace_words: u64,
            output: *mut u64,
            output_words: u64,
        ) -> i32;
    }

    fn limit(resource: CfgResource, limit: usize) -> CfgError {
        CfgError::Limit { resource, limit: limit as u64 }
    }

    fn allocated<T>(capacity: usize) -> Result<Vec<T>, CfgError> {
        let mut v = Vec::new();
        v.try_reserve_exact(capacity).map_err(|_| CfgError::Allocation)?;
        Ok(v)
    }

    fn words(count: usize) -> Result<Vec<u64>, CfgError> {
        let mut v = allocated(count)?;
        v.resize(count, 0);
        Ok(v)
    }

    pub(super) fn analyze(
        input: &[u8],
        base: u64,
        seeds: &[usize],
        overlays: &[FlowOverride],
        l: CfgLimits,
    ) -> Result<ControlFlowGraph, CfgError> {
        let max_input = l.max_input_bytes.min(16 * 1024 * 1024);
        if input.len() > max_input {
            return Err(limit(CfgResource::InputBytes, max_input));
        }
        if base.checked_add(input.len() as u64).is_none() {
            return Err(CfgError::Invalid("region address overflow"));
        }
        if l.max_instructions > 1_000_000
            || l.max_blocks > 1_000_000
            || l.max_edges > 4_000_000
            || l.max_stops > 1_000_000
        {
            return Err(CfgError::Invalid("hard output ceiling"));
        }
        if seeds.len() > l.max_blocks {
            return Err(limit(CfgResource::Blocks, l.max_blocks));
        }
        if overlays.len() > l.max_edges || overlays.len() > 1_000_000 {
            return Err(limit(CfgResource::Edges, l.max_edges.min(1_000_000)));
        }
        let block_base = 16 + l.max_instructions * 97;
        let edge_base = block_base + l.max_blocks * 5;
        let stop_base = edge_base + l.max_edges * 6;
        let total_words = stop_base + l.max_stops * 4;
        let workspace_words = input.len() + l.max_blocks + total_words;
        let max_memory = l.max_workspace_bytes.min(512 * 1024 * 1024);
        let memory = workspace_words
            .checked_add(total_words)
            .and_then(|n| n.checked_add(overlays.len() * 4 + seeds.len()))
            .and_then(|n| n.checked_mul(8))
            .and_then(|n| n.checked_add(std::mem::size_of_val(overlays)))
            .filter(|n| *n <= max_memory)
            .ok_or(limit(CfgResource::Memory, max_memory))?;
        let _ = memory;
        // Reserve an O(n log n) normalization budget before any input scan or
        // sorting. Wave receives the remaining shared analysis-work allowance.
        let sort_work = |count: usize| {
            (count as u64) * (u64::from(usize::BITS - count.max(1).leading_zeros()) + 1)
        };
        let host_work = sort_work(seeds.len()) + sort_work(overlays.len());
        if host_work > l.max_work {
            return Err(CfgError::Limit { resource: CfgResource::Work, limit: l.max_work });
        }
        if seeds.iter().any(|offset| *offset >= input.len()) {
            return Err(CfgError::Invalid("seed outside region"));
        }
        let mut normalized = allocated(overlays.len())?;
        normalized.extend_from_slice(overlays);
        normalized.sort_unstable_by_key(|o| o.field_offset);
        let mut previous_end = 0;
        for o in &normalized {
            let end = o
                .field_offset
                .checked_add(o.width as usize)
                .filter(|end| *end <= input.len())
                .ok_or(CfgError::Invalid("relocation range"))?;
            if o.width == 0 || o.width > 8 || o.field_offset < previous_end {
                return Err(CfgError::Invalid("overlapping or empty relocation fields"));
            }
            previous_end = end;
        }
        let mut raw_overlays = allocated(normalized.len() * 4)?;
        for o in normalized {
            let (kind, value) = match o.target {
                CfgTarget::Unresolved => (0, 0),
                CfgTarget::Address(v) => (1, v),
                CfgTarget::Symbol(v) => (2, v),
            };
            raw_overlays.extend_from_slice(&[o.field_offset as u64, o.width as u64, kind, value]);
        }
        let mut raw_seeds = allocated(seeds.len())?;
        raw_seeds.extend(seeds.iter().map(|offset| *offset as u64));
        raw_seeds.sort_unstable();
        raw_seeds.dedup();
        let options = [
            l.max_instructions as u64,
            l.max_blocks as u64,
            l.max_edges as u64,
            l.max_stops as u64,
            l.max_work - host_work,
            u64::from(l.follow_calls),
        ];
        let mut workspace = words(workspace_words)?;
        let mut output = words(total_words)?;
        // SAFETY: all input/output/scratch vectors are live, disjoint and sized
        // to the checked ABI formulas; indices and ranges were checked above.
        // The pinned linked Wave core is trusted code and retains no pointers.
        let status = unsafe {
            if whale_wave_reverse_abi() != ABI_VERSION {
                return Err(CfgError::Invalid("ABI version"));
            }
            whale_wave_reverse_cfg(
                ABI_VERSION,
                input.as_ptr(),
                input.len() as u64,
                base,
                raw_seeds.as_ptr(),
                raw_seeds.len() as u64,
                raw_overlays.as_ptr(),
                (raw_overlays.len() / 4) as u64,
                options.as_ptr(),
                6,
                workspace.as_mut_ptr(),
                workspace_words as u64,
                output.as_mut_ptr(),
                total_words as u64,
            )
        };
        let resource = match status {
            0 => None,
            7 => Some((CfgResource::Work, l.max_work)),
            8 => Some((CfgResource::Instructions, l.max_instructions as u64)),
            9 => Some((CfgResource::Blocks, l.max_blocks as u64)),
            10 => Some((CfgResource::Edges, l.max_edges as u64)),
            11 => Some((CfgResource::Stops, l.max_stops as u64)),
            _ => return Err(CfgError::Invalid("kernel rejected input/capacity/ABI")),
        };
        if let Some((resource, limit)) = resource {
            return Err(CfgError::Limit { resource, limit });
        }
        if output[0] != 1
            || output[1] != total_words as u64
            || output[8] != 16
            || output[9] != block_base as u64
            || output[10] != edge_base as u64
            || output[11] != stop_base as u64
            || output[6] > l.max_work - host_work
        {
            return Err(CfgError::Invalid("result header"));
        }
        let count = |at: usize, max: usize| {
            usize::try_from(output[at])
                .ok()
                .filter(|n| *n <= max)
                .ok_or(CfgError::Invalid("result count"))
        };
        let instruction_count = count(2, l.max_instructions)?;
        let block_count = count(3, l.max_blocks)?;
        let edge_count = count(4, l.max_edges)?;
        let stop_count = count(5, l.max_stops)?;
        let mut instructions = allocated(instruction_count)?;
        let mut starts = HashSet::new();
        starts.try_reserve(instruction_count).map_err(|_| CfgError::Allocation)?;
        for index in 0..instruction_count {
            let at = 16 + index * 97;
            let offset =
                usize::try_from(output[at]).map_err(|_| CfgError::Invalid("instruction offset"))?;
            if !starts.insert(offset) {
                return Err(CfgError::Invalid("duplicate instruction"));
            }
            let mut record = [0; DECODE_WORDS];
            record.copy_from_slice(&output[at + 1..at + 97]);
            instructions.push(
                crate::decode_record(input, offset, &record).map_err(CfgError::DecodeOutput)?,
            );
        }
        let mut blocks = allocated(block_count)?;
        let mut previous = 0;
        let mut covered = 0;
        for index in 0..block_count {
            let at = block_base + index * 5;
            let offset =
                usize::try_from(output[at]).map_err(|_| CfgError::Invalid("block offset"))?;
            let end = usize::try_from(output[at + 1])
                .ok()
                .filter(|n| *n <= input.len())
                .ok_or(CfgError::Invalid("block end"))?;
            let first = usize::try_from(output[at + 2])
                .map_err(|_| CfgError::Invalid("first instruction"))?;
            let count =
                usize::try_from(output[at + 3]).map_err(|_| CfgError::Invalid("block length"))?;
            let last = first
                .checked_add(count)
                .filter(|n| *n <= instructions.len() && count > 0)
                .ok_or(CfgError::Invalid("block instruction range"))?;
            if offset < previous || offset >= end || instructions[first].offset != offset {
                return Err(CfgError::Invalid("block ordering"));
            }
            let mut cursor = offset;
            for instruction in &instructions[first..last] {
                if instruction.offset != cursor {
                    return Err(CfgError::Invalid("noncontiguous block"));
                }
                cursor += instruction.length as usize;
            }
            if cursor != end {
                return Err(CfgError::Invalid("block coverage"));
            }
            previous = end;
            covered += count;
            let mut instruction_indices = allocated(count)?;
            instruction_indices.extend(first..last);
            blocks.push(BasicBlock { offset, end, instruction_indices });
        }
        if covered != instructions.len() {
            return Err(CfgError::Invalid("incomplete block partition"));
        }
        let mut edges = allocated(edge_count)?;
        for index in 0..edge_count {
            let at = edge_base + index * 6;
            let from = usize::try_from(output[at])
                .ok()
                .filter(|n| starts.contains(n))
                .ok_or(CfgError::Invalid("edge source"))?;
            let kind = match output[at + 1] {
                0 => EdgeKind::FallThrough,
                1 => EdgeKind::Call,
                2 => EdgeKind::Jump,
                3 => EdgeKind::ConditionalTaken,
                4 => EdgeKind::ConditionalFallThrough,
                5 => EdgeKind::LoopTaken,
                6 => EdgeKind::LoopFallThrough,
                7 => EdgeKind::IndirectCall,
                8 => EdgeKind::IndirectJump,
                9 => EdgeKind::Syscall,
                10 => EdgeKind::Memory,
                11 => EdgeKind::Address,
                _ => return Err(CfgError::Invalid("edge kind")),
            };
            let target = match output[at + 2] {
                0 => CfgTarget::Unresolved,
                1 => CfgTarget::Address(output[at + 3]),
                2 => CfgTarget::Symbol(output[at + 3]),
                _ => return Err(CfgError::Invalid("edge target")),
            };
            edges.push(Edge { from, kind, target });
        }
        let mut stops = allocated(stop_count)?;
        for index in 0..stop_count {
            let at = stop_base + index * 4;
            let offset = usize::try_from(output[at])
                .ok()
                .filter(|n| *n <= input.len())
                .ok_or(CfgError::Invalid("stop offset"))?;
            let kind = match output[at + 1] {
                1 => StopKind::Invalid,
                2 => StopKind::Truncated,
                3 => StopKind::Unsupported,
                4 => StopKind::Overlong,
                5 => StopKind::Overlap,
                6 => StopKind::EndOfRange,
                7 => StopKind::IndirectJump,
                8 => StopKind::Syscall,
                9 => StopKind::AddressOverflow,
                _ => return Err(CfgError::Invalid("stop kind")),
            };
            let conflict = if kind == StopKind::Overlap {
                Some(
                    usize::try_from(output[at + 2])
                        .ok()
                        .filter(|n| starts.contains(n))
                        .ok_or(CfgError::Invalid("overlap source"))?,
                )
            } else {
                None
            };
            stops.push(Stop { offset, kind, conflict });
        }
        Ok(ControlFlowGraph {
            base,
            instructions,
            blocks,
            edges,
            stops,
            work: output[6] + host_work,
        })
    }
}
