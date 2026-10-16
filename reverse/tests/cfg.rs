use reverse::*;

#[test]
fn absent_cfg_core_is_explicit() {
    if !backend_available() {
        assert_eq!(
            analyze(&[0xc3], 0, &[0], &[], CfgLimits::default()),
            Err(CfgError::CoreUnavailable)
        );
    }
}

#[cfg(whale_wave_reverse)]
mod native {
    use super::*;

    fn limits() -> CfgLimits {
        CfgLimits {
            max_input_bytes: 65536,
            max_instructions: 256,
            max_blocks: 128,
            max_edges: 1024,
            max_stops: 128,
            max_work: 1_000_000,
            max_workspace_bytes: 4 * 1024 * 1024,
            follow_calls: true,
        }
    }

    fn graph(bytes: &[u8], seeds: &[usize]) -> ControlFlowGraph {
        analyze(bytes, 0x1000, seeds, &[], limits()).unwrap()
    }

    fn offsets(g: &ControlFlowGraph) -> Vec<usize> {
        let mut v: Vec<_> = g.instructions.iter().map(|i| i.offset).collect();
        v.sort_unstable();
        v
    }

    fn edge(g: &ControlFlowGraph, from: usize, kind: EdgeKind, target: CfgTarget) {
        assert!(
            g.edges.contains(&Edge { from, kind, target }),
            "missing edge {from} {kind:?} {target:?}: {:?}",
            g.edges
        );
    }

    #[test]
    fn diamond_and_late_backward_leaders_split_existing_blocks_in_wave() {
        let g = graph(&[0x75, 3, 0x90, 0xeb, 1, 0x90, 0xc3], &[0]);
        assert_eq!(offsets(&g), [0, 2, 3, 5, 6]);
        assert_eq!(
            g.blocks.iter().map(|b| (b.offset, b.end)).collect::<Vec<_>>(),
            [(0, 2), (2, 5), (5, 6), (6, 7)]
        );
        edge(&g, 0, EdgeKind::ConditionalTaken, CfgTarget::Address(0x1005));
        edge(&g, 0, EdgeKind::ConditionalFallThrough, CfgTarget::Address(0x1002));
        edge(&g, 3, EdgeKind::Jump, CfgTarget::Address(0x1006));
        edge(&g, 5, EdgeKind::FallThrough, CfgTarget::Address(0x1006));
        assert!(g.stops.is_empty());
        let g = graph(&[0x90, 0x90, 0x90, 0xeb, 0xfc], &[0]);
        assert_eq!(
            g.blocks.iter().map(|b| (b.offset, b.end)).collect::<Vec<_>>(),
            [(0, 1), (1, 5)]
        );
        edge(&g, 0, EdgeKind::FallThrough, CfgTarget::Address(0x1001));
        edge(&g, 3, EdgeKind::Jump, CfgTarget::Address(0x1001));
    }

    #[test]
    fn entries_decode_backward_callees_and_targets_after_unsupported_gaps_on_demand() {
        let bytes = [0xc3, 0x0f, 0x0b, 0x90, 0xe8, 0xf7, 0xff, 0xff, 0xff, 0xc3];
        let g = graph(&bytes, &[4]);
        assert_eq!(offsets(&g), [0, 4, 9]);
        edge(&g, 4, EdgeKind::Call, CfgTarget::Address(0x1000));
        let g = analyze(&bytes, 0x1000, &[4], &[], CfgLimits { follow_calls: false, ..limits() })
            .unwrap();
        assert_eq!(offsets(&g), [4, 9]);
        let g = graph(&[0xeb, 2, 0x0f, 0x0b, 0x90, 0xc3], &[0]);
        assert_eq!(offsets(&g), [0, 4, 5]);
        assert!(g.stops.is_empty());
    }

    #[test]
    fn recursive_calls_and_indirect_targets_have_bounded_explicit_edges() {
        let g = graph(&[0xe8, 0xfb, 0xff, 0xff, 0xff, 0xc3], &[0]);
        assert_eq!(offsets(&g), [0, 5]);
        edge(&g, 0, EdgeKind::Call, CfgTarget::Address(0x1000));
        let g = graph(&[0xff, 0xd0, 0xff, 0xe0, 0x90, 0xc3], &[0]);
        assert_eq!(offsets(&g), [0, 2]);
        edge(&g, 0, EdgeKind::IndirectCall, CfgTarget::Unresolved);
        edge(&g, 2, EdgeKind::IndirectJump, CfgTarget::Unresolved);
        assert!(g.stops.iter().any(|s| s.kind == StopKind::IndirectJump && s.offset == 2));
        let g = graph(&[0x0f, 5, 0xc3], &[0]);
        assert_eq!(offsets(&g), [0]);
        edge(&g, 0, EdgeKind::Syscall, CfgTarget::Unresolved);
        assert_eq!(g.stops[0].kind, StopKind::Syscall);
    }

    #[test]
    fn unknown_truncated_invalid_overlong_and_overlapping_entries_stop_without_guessing() {
        for (bytes, kind) in [
            (vec![0xc5], StopKind::Unsupported),
            (vec![0x48, 0xb8], StopKind::Truncated),
            (vec![0x8d, 0xc0], StopKind::Invalid),
            (vec![0x66; 15], StopKind::Overlong),
        ] {
            let g = graph(&bytes, &[0]);
            assert!(g.instructions.is_empty());
            assert!(g.blocks.is_empty());
            assert_eq!(g.stops[0].kind, kind);
        }
        let g = graph(&[0xeb, 0xff], &[0]);
        assert_eq!(offsets(&g), [0]);
        assert!(g.stops.contains(&Stop { offset: 1, kind: StopKind::Overlap, conflict: Some(0) }));
        let g = graph(&[0xb8, 0x90, 0xc3, 0, 0, 0xe9, 0xf6, 0xff, 0xff, 0xff], &[2, 5]);
        assert_eq!(offsets(&g), [2, 5]);
        assert!(g.stops.contains(&Stop { offset: 0, kind: StopKind::Overlap, conflict: Some(2) }));
    }

    #[test]
    fn relocation_overlays_override_placeholders_before_target_selection() {
        let bytes = [0xe8, 0, 0, 0, 0, 0xc3, 0x0f, 0x0b, 0xc3];
        let overlay = FlowOverride { field_offset: 1, width: 4, target: CfgTarget::Symbol(77) };
        let g = analyze(&bytes, 0x1000, &[0], &[overlay], limits()).unwrap();
        assert_eq!(offsets(&g), [0, 5]);
        edge(&g, 0, EdgeKind::Call, CfgTarget::Symbol(77));
        assert!(!g
            .edges
            .iter()
            .any(|e| e.kind == EdgeKind::Call && e.target == CfgTarget::Address(0x1005)));
        let g = analyze(
            &bytes,
            0x1000,
            &[0],
            &[FlowOverride { target: CfgTarget::Address(0x1008), ..overlay }],
            limits(),
        )
        .unwrap();
        assert_eq!(offsets(&g), [0, 5, 8]);
        let g = analyze(
            &bytes,
            0x1000,
            &[0],
            &[FlowOverride { field_offset: 2, width: 2, target: CfgTarget::Address(0x1008) }],
            limits(),
        )
        .unwrap();
        edge(&g, 0, EdgeKind::Call, CfgTarget::Unresolved);
        assert_eq!(offsets(&g), [0, 5]);
        let g = analyze(
            &bytes,
            0x1000,
            &[0],
            &[FlowOverride { target: CfgTarget::Unresolved, ..overlay }],
            limits(),
        )
        .unwrap();
        edge(&g, 0, EdgeKind::Call, CfgTarget::Unresolved);
        assert!(g.stops.iter().all(|s| s.kind != StopKind::AddressOverflow));
    }

    #[test]
    fn memory_and_symbolic_immediate_xrefs_do_not_fabricate_code_reachability() {
        let bytes = [0x48, 0x8b, 0x05, 2, 0, 0, 0, 0xc3, 0x90, 0x90];
        let g = graph(&bytes, &[0]);
        assert_eq!(offsets(&g), [0, 7]);
        edge(&g, 0, EdgeKind::Memory, CfgTarget::Address(0x1009));
        let g = analyze(
            &bytes,
            0x1000,
            &[0],
            &[FlowOverride { field_offset: 3, width: 4, target: CfgTarget::Symbol(9) }],
            limits(),
        )
        .unwrap();
        edge(&g, 0, EdgeKind::Memory, CfgTarget::Symbol(9));
        let g = graph(&[0x48, 0x8b, 0x04, 0x25, 0xfc, 0xff, 0xff, 0xff, 0xc3], &[0]);
        edge(&g, 0, EdgeKind::Memory, CfgTarget::Address(u64::MAX - 3));
        let g = graph(&[0x48, 0x8d, 0x05, 0, 0, 0, 0, 0xc3], &[0]);
        edge(&g, 0, EdgeKind::Address, CfgTarget::Address(0x1007));
        let bytes = [0xb8, 0, 0, 0, 0, 0xc3];
        let g = graph(&bytes, &[0]);
        assert!(g.edges.is_empty());
        let g = analyze(
            &bytes,
            0x1000,
            &[0],
            &[FlowOverride { field_offset: 1, width: 4, target: CfgTarget::Symbol(3) }],
            limits(),
        )
        .unwrap();
        edge(&g, 0, EdgeKind::Address, CfgTarget::Symbol(3));
    }

    #[test]
    fn indexed_symbol_components_are_xrefs_without_becoming_effective_code_addresses() {
        let bytes = [0x48, 0x8b, 4, 0x85, 0, 0, 0, 0, 0xc3];
        let g = graph(&bytes, &[0]);
        assert!(g.edges.is_empty());
        let g = analyze(
            &bytes,
            0x1000,
            &[0],
            &[FlowOverride { field_offset: 4, width: 4, target: CfgTarget::Symbol(42) }],
            limits(),
        )
        .unwrap();
        edge(&g, 0, EdgeKind::Memory, CfgTarget::Symbol(42));
        assert_eq!(offsets(&g), [0, 8]);
    }

    #[test]
    fn rip_references_include_trailing_immediates_and_never_resolve_indirect_contents() {
        let g = graph(&[0xc7, 5, 0, 0, 0, 0, 1, 0, 0, 0, 0xc3], &[0]);
        edge(&g, 0, EdgeKind::Memory, CfgTarget::Address(0x100a));
        let g = graph(&[0xff, 0x25, 0, 0, 0, 0, 0xc3], &[0]);
        assert_eq!(offsets(&g), [0]);
        edge(&g, 0, EdgeKind::Memory, CfgTarget::Address(0x1006));
        edge(&g, 0, EdgeKind::IndirectJump, CfgTarget::Unresolved);
    }

    #[test]
    fn large_overlay_sets_use_shared_normalization_and_binary_lookup_budgets() {
        let mut bytes = Vec::new();
        let mut overlays = Vec::new();
        for index in 0..256 {
            overlays.push(FlowOverride {
                field_offset: bytes.len() + 1,
                width: 4,
                target: CfgTarget::Symbol(index),
            });
            bytes.extend_from_slice(&[0xe8, 0, 0, 0, 0]);
        }
        bytes.push(0xc3);
        for index in 0..1000 {
            overlays.push(FlowOverride {
                field_offset: bytes.len(),
                width: 1,
                target: CfgTarget::Symbol(1000 + index),
            });
            bytes.push(0x90);
        }
        let budget = CfgLimits {
            max_instructions: 300,
            max_blocks: 300,
            max_edges: 1600,
            max_work: 70_000,
            ..limits()
        };
        let g = analyze(&bytes, 0, &[0], &overlays, budget).unwrap();
        assert_eq!(g.instructions.len(), 257);
        assert_eq!(g.edges.len(), 512);
        assert!(g.work <= 70_000);
        assert!(matches!(
            analyze(&bytes, 0, &[0], &overlays, CfgLimits { max_work: 1256, ..budget }),
            Err(CfgError::Limit { resource: CfgResource::Work, .. })
        ));
    }

    #[test]
    fn scope_address_overflow_and_end_of_region_are_explicit() {
        let g = graph(&[0xeb, 0x7f], &[0]);
        assert_eq!(offsets(&g), [0]);
        edge(&g, 0, EdgeKind::Jump, CfgTarget::Address(0x1081));
        let g = analyze(&[0xeb, 0x7f], u64::MAX - 2, &[0], &[], limits()).unwrap();
        edge(&g, 0, EdgeKind::Jump, CfgTarget::Unresolved);
        assert_eq!(g.stops[0].kind, StopKind::AddressOverflow);
        assert!(matches!(
            analyze(&[0xc3, 0xc3], u64::MAX, &[0], &[], limits()),
            Err(CfgError::Invalid(_))
        ));
        let g = graph(&[0x90], &[0]);
        assert_eq!(g.stops[0].kind, StopKind::EndOfRange);
        edge(&g, 0, EdgeKind::FallThrough, CfgTarget::Address(0x1001));
    }

    #[test]
    fn every_resource_budget_and_invalid_boundary_has_a_structured_error() {
        for (resource, limits, bytes) in [
            (CfgResource::InputBytes, CfgLimits { max_input_bytes: 0, ..limits() }, vec![0xc3]),
            (CfgResource::Instructions, CfgLimits { max_instructions: 0, ..limits() }, vec![0xc3]),
            (CfgResource::Blocks, CfgLimits { max_blocks: 0, ..limits() }, vec![0xc3]),
            (CfgResource::Edges, CfgLimits { max_edges: 0, ..limits() }, vec![0xeb, 0]),
            (CfgResource::Stops, CfgLimits { max_stops: 0, ..limits() }, vec![0xc5]),
            (CfgResource::Work, CfgLimits { max_work: 0, ..limits() }, vec![0xc3]),
            (CfgResource::Memory, CfgLimits { max_workspace_bytes: 0, ..limits() }, vec![0xc3]),
        ] {
            assert!(
                matches!(analyze(&bytes,0,&[0],&[],limits),Err(CfgError::Limit{resource:r,..}) if r==resource),
                "{resource:?}"
            );
        }
        assert!(matches!(analyze(&[0xc3], 0, &[1], &[], limits()), Err(CfgError::Invalid(_))));
        let overlays = [
            FlowOverride { field_offset: 1, width: 4, target: CfgTarget::Symbol(0) },
            FlowOverride { field_offset: 2, width: 1, target: CfgTarget::Symbol(1) },
        ];
        assert!(matches!(
            analyze(&[0xe8, 0, 0, 0, 0, 0xc3], 0, &[0], &overlays, limits()),
            Err(CfgError::Invalid(_))
        ));
    }

    #[test]
    fn seed_and_overlay_normalization_is_deterministic() {
        let bytes = [0xb8, 0, 0, 0, 0, 0xc3, 0xb8, 0, 0, 0, 0, 0xc3];
        let a = FlowOverride { field_offset: 1, width: 4, target: CfgTarget::Symbol(1) };
        let b = FlowOverride { field_offset: 7, width: 4, target: CfgTarget::Symbol(2) };
        let first = analyze(&bytes, 0, &[6, 0], &[b, a], limits()).unwrap();
        let second = analyze(&bytes, 0, &[0, 6], &[a, b], limits()).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn raw_cfg_failures_never_publish_partial_graph_output() {
        extern "C" {
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
        let bytes = [0xc3u8];
        let seeds = [0u64];
        for (abi, max_work, capacity, expected) in
            [(0, 1000, 488, 6), (1, 0, 488, 7), (1, 1000, 0, 5)]
        {
            let options = [4u64, 4, 8, 4, max_work, 1];
            let total = 16 + 4 * 97 + 4 * 5 + 8 * 6 + 4 * 4;
            let mut workspace = vec![0u64; 1 + 4 + total];
            let mut output = vec![u64::MAX; total];
            // SAFETY: valid disjoint aligned buffers, scalar rejection cases.
            let status = unsafe {
                whale_wave_reverse_cfg(
                    abi,
                    bytes.as_ptr(),
                    1,
                    0,
                    seeds.as_ptr(),
                    1,
                    std::ptr::null(),
                    0,
                    options.as_ptr(),
                    6,
                    workspace.as_mut_ptr(),
                    workspace.len() as u64,
                    output.as_mut_ptr(),
                    capacity,
                )
            };
            assert_eq!(status, expected);
            assert!(output.iter().all(|v| *v == u64::MAX));
        }
    }
}
