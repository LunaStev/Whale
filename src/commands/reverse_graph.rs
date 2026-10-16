// SPDX-License-Identifier: MPL-2.0
use super::reverse_render::{stream_structured, BoundedOutput, InstructionView, Stream};

use reverse::{
    image::{CodeRegion, Image},
    CfgTarget, ControlFlowGraph, EdgeKind,
};

use serde::{ser::SerializeMap, Serialize, Serializer};

use std::fmt::Write;

#[derive(Serialize)]
struct Block<'a> {
    offset: usize,
    end: usize,
    instruction_indices: &'a [usize],
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Target<'a> {
    Address { address: u64 },
    Symbol { id: u64, name: &'a str },
    Unresolved,
}

#[derive(Serialize)]
struct Edge<'a> {
    from: usize,
    kind: String,
    target: Target<'a>,
}

#[derive(Serialize)]
struct Stop {
    offset: usize,
    kind: String,
    conflict: Option<usize>,
}

#[derive(Serialize)]
struct Relocation<'a> {
    offset: usize,
    symbol: &'a str,
    addend: i64,
    kind: String,
}

struct Report<'a, 'b> {
    image: &'a Image<'b>,
    region: &'a CodeRegion<'b>,
    result: &'a ControlFlowGraph,
    names: &'a [&'a str],
    mode: &'a str,
    entry: usize,
}

fn retained(kind: EdgeKind, mode: &str) -> bool {
    mode != "xrefs"
        || !matches!(
            kind,
            EdgeKind::FallThrough | EdgeKind::ConditionalFallThrough | EdgeKind::LoopFallThrough
        )
}

impl Serialize for Report<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(12))?;
        let graph = self.result;
        map.serialize_entry("schema", &1)?;
        map.serialize_entry("kind", self.mode)?;
        map.serialize_entry("region", &self.region.id)?;
        map.serialize_entry("base", &self.region.address)?;
        map.serialize_entry("entry_offset", &self.entry)?;
        map.serialize_entry("backend", reverse::backend_name())?;
        map.serialize_entry("work", &graph.work)?;
        map.serialize_entry(
            "blocks",
            &Stream(graph.blocks.iter().map(|b| Block {
                offset: b.offset,
                end: b.end,
                instruction_indices: &b.instruction_indices,
            })),
        )?;
        map.serialize_entry(
            "instructions",
            &Stream(graph.instructions.iter().map(|instruction| InstructionView {
                instruction,
                image: self.image,
                region: self.region.id,
                base: self.region.address,
            })),
        )?;
        map.serialize_entry(
            "edges",
            &Stream(graph.edges.iter().filter(|e| retained(e.kind, self.mode)).map(|e| Edge {
                from: e.from,
                kind: format!("{:?}", e.kind),
                target: match e.target {
                    CfgTarget::Address(address) => Target::Address { address },
                    CfgTarget::Symbol(id) => Target::Symbol { id, name: self.names[id as usize] },
                    CfgTarget::Unresolved => Target::Unresolved,
                },
            })),
        )?;
        map.serialize_entry(
            "stops",
            &Stream(graph.stops.iter().map(|s| Stop {
                offset: s.offset,
                kind: format!("{:?}", s.kind),
                conflict: s.conflict,
            })),
        )?;
        map.serialize_entry(
            "relocations",
            &Stream(self.image.relocations.iter().filter(|r| r.region == self.region.id).map(
                |r| Relocation {
                    offset: r.offset,
                    symbol: &r.symbol,
                    addend: r.addend,
                    kind: format!("{:?}", r.kind),
                },
            )),
        )?;
        map.end()
    }
}

pub(super) struct RenderOptions<'a> {
    pub mode: &'a str,
    pub entry: usize,
    pub structured: bool,
    pub limit: usize,
}

pub(super) fn render(
    image: &Image<'_>,
    region: &CodeRegion<'_>,
    result: &ControlFlowGraph,
    names: &[&str],
    options: RenderOptions<'_>,
) -> Result<String, String> {
    let RenderOptions { mode, entry, structured, limit } = options;
    if result
        .edges
        .iter()
        .any(|e| matches!(e.target,CfgTarget::Symbol(id) if id>=names.len() as u64))
    {
        return Err("Wave graph returned an unknown symbol identity".into());
    }
    if structured {
        return stream_structured(&Report { image, region, result, names, mode, entry }, limit);
    }
    let mut output = BoundedOutput::new(limit);
    let write = (|| -> std::fmt::Result {
        writeln!(
            output,
            "{mode} region={} {:?} entry=0x{:x} work={}",
            region.id,
            region.name,
            region.address + entry as u64,
            result.work
        )?;
        if mode == "cfg" {
            for b in &result.blocks {
                writeln!(output, "block +0x{:x}..+0x{:x}", b.offset, b.end)?;
            }
        }
        for edge in result.edges.iter().filter(|e| retained(e.kind, mode)) {
            write!(output, "+0x{:x} {:?} -> ", edge.from, edge.kind)?;
            match edge.target {
                CfgTarget::Address(address) => writeln!(output, "0x{address:x}")?,
                CfgTarget::Symbol(id) => writeln!(output, "symbol {:?}", names[id as usize])?,
                CfgTarget::Unresolved => writeln!(output, "unresolved")?,
            }
        }
        for stop in &result.stops {
            writeln!(
                output,
                "stop +0x{:x} {:?} conflict={:?}",
                stop.offset, stop.kind, stop.conflict
            )?;
        }
        Ok(())
    })();
    write.map_err(|_| "reverse output byte limit exceeded")?;
    output.finish()
}
